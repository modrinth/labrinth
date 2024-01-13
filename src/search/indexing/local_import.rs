use chrono::{DateTime, Utc};
use dashmap::DashMap;
use futures::TryStreamExt;
use itertools::Itertools;
use log::info;
use std::collections::HashMap;

use super::IndexingError;
use crate::database::models::{ProjectId, VersionId};
use crate::database::redis::RedisPool;
use crate::search::UploadSearchProject;
use sqlx::postgres::PgPool;

pub async fn index_local(
    pool: &PgPool,
    redis: &RedisPool,
) -> Result<Vec<UploadSearchProject>, IndexingError> {
    info!("Indexing local projects!");

    // todo: loaders, project type, game versions
    struct PartialProject {
        id: ProjectId,
        name: String,
        summary: String,
        downloads: i32,
        follows: i32,
        icon_url: Option<String>,
        updated: DateTime<Utc>,
        approved: DateTime<Utc>,
        slug: Option<String>,
        color: Option<i32>,
        license: String,
    }

    use futures::TryStreamExt;
    let db_projects = sqlx::query!(
        "
        SELECT m.id id, m.name name, m.summary summary, m.downloads downloads, m.follows follows,
        m.icon_url icon_url, m.updated updated, m.approved approved, m.published, m.license license, m.slug slug, m.color
        FROM mods m
        WHERE m.status = ANY($1)
        GROUP BY m.id;
        ",
        &*crate::models::projects::ProjectStatus::iterator()
        .filter(|x| x.is_searchable())
        .map(|x| x.to_string())
        .collect::<Vec<String>>(),
    )
        .fetch_many(&*pool)
        .try_filter_map(|e| async {
            Ok(e.right().map(|m| {

            PartialProject {
                id: ProjectId(m.id),
                name: m.name,
                summary: m.summary,
                downloads: m.downloads,
                follows: m.follows,
                icon_url: m.icon_url,
                updated: m.updated,
                approved: m.approved.unwrap_or(m.published),
                slug: m.slug,
                color: m.color,
                license: m.license,
            }}))
        })
        .try_collect::<Vec<PartialProject>>()
        .await?;

    let project_ids = db_projects.iter().map(|x| x.id.0).collect::<Vec<i64>>();

    struct PartialGallery {
        url: String,
        featured: bool,
        ordering: i64,
    }

    info!("Indexing local gallery!");

    let mods_gallery: DashMap<ProjectId, Vec<PartialGallery>> = sqlx::query!(
        "
        SELECT mod_id, image_url, featured, ordering
        FROM mods_gallery
        WHERE mod_id = ANY($1)
        ",
        &*project_ids,
    )
    .fetch(&*pool)
    .try_fold(
        DashMap::new(),
        |acc: DashMap<ProjectId, Vec<PartialGallery>>, m| {
            acc.entry(ProjectId(m.mod_id))
                .or_default()
                .push(PartialGallery {
                    url: m.image_url,
                    featured: m.featured.unwrap_or(false),
                    ordering: m.ordering,
                });
            async move { Ok(acc) }
        },
    )
    .await?;

    info!("Indexing local categories!");

    let categories: DashMap<ProjectId, Vec<(String, bool)>> = sqlx::query!(
        "
        SELECT mc.joining_mod_id mod_id, c.category name, mc.is_additional is_additional
        FROM mods_categories mc
        INNER JOIN categories c ON mc.joining_category_id = c.id
        WHERE joining_mod_id = ANY($1)
        ",
        &*project_ids,
    )
    .fetch(&*pool)
    .try_fold(
        DashMap::new(),
        |acc: DashMap<ProjectId, Vec<(String, bool)>>, m| {
            acc.entry(ProjectId(m.mod_id))
                .or_default()
                .push((m.name, m.is_additional));
            async move { Ok(acc) }
        },
    )
    .await?;

    struct PartialVersion {
        id: VersionId,
    }

    info!("Indexing local versions!");

    let versions: DashMap<ProjectId, Vec<PartialVersion>> = sqlx::query!(
        "
        SELECT v.id, v.mod_id
        FROM versions v
        WHERE mod_id = ANY($1)
        ",
        &project_ids,
    )
    .fetch(&*pool)
    .try_fold(
        DashMap::new(),
        |acc: DashMap<ProjectId, Vec<PartialVersion>>, m| {
            acc.entry(ProjectId(m.mod_id))
                .or_default()
                .push(PartialVersion {
                    id: VersionId(m.id),
                });
            async move { Ok(acc) }
        },
    )
    .await?;

    info!("Indexing local org owners!");

    let mods_org_owners: DashMap<ProjectId, String> = sqlx::query!(
        "
        SELECT m.id mod_id, u.username
        FROM mods m
        INNER JOIN organizations o ON o.id = m.organization_id
        INNER JOIN team_members tm ON tm.is_owner = TRUE and tm.team_id = o.team_id
        INNER JOIN users u ON u.id = tm.user_id
        WHERE m.id = ANY($1)
        ",
        &*project_ids,
    )
    .fetch(&*pool)
    .try_fold(DashMap::new(), |acc: DashMap<ProjectId, String>, m| {
        acc.insert(ProjectId(m.mod_id), m.username);
        async move { Ok(acc) }
    })
    .await?;

    info!("Indexing local team owners!");

    let mods_team_owners: DashMap<ProjectId, String> = sqlx::query!(
        "
        SELECT m.id mod_id, u.username
        FROM mods m
        INNER JOIN team_members tm ON tm.is_owner = TRUE and tm.team_id = m.team_id
        INNER JOIN users u ON u.id = tm.user_id
        WHERE m.id = ANY($1)
        ",
        &project_ids,
    )
    .fetch(&*pool)
    .try_fold(DashMap::new(), |acc: DashMap<ProjectId, String>, m| {
        acc.insert(ProjectId(m.mod_id), m.username);
        async move { Ok(acc) }
    })
    .await?;

    let mut uploads = Vec::new();

    let total_len = db_projects.len();
    let mut count = 0;
    for project in db_projects {
        count += 1;
        info!("projects index prog: {count}/{total_len}");

        let owner = if let Some((_, org_owner)) = mods_org_owners.remove(&project.id) {
            org_owner
        } else if let Some((_, team_owner)) = mods_team_owners.remove(&project.id) {
            team_owner
        } else {
            println!(
                "org owner not found for project {} id: {}!",
                project.name, project.id.0
            );
            continue;
        };

        let license = match project.license.split(' ').next() {
            Some(license) => license.to_string(),
            None => project.license.clone(),
        };

        let open_source = match spdx::license_id(&license) {
            Some(id) => id.is_osi_approved(),
            _ => false,
        };

        let (featured_gallery, gallery) =
            if let Some((_, mut gallery)) = mods_gallery.remove(&project.id) {
                let mut vals = Vec::new();
                let mut featured = None;

                for x in gallery
                    .into_iter()
                    .sorted_by(|a, b| a.ordering.cmp(&b.ordering))
                {
                    if x.featured && featured.is_none() {
                        featured = Some(x.url);
                    } else {
                        vals.push(x.url);
                    }
                }

                (featured, vals)
            } else {
                (None, vec![])
            };

        let (categories, display_categories) =
            if let Some((_, mut categories)) = categories.remove(&project.id) {
                let mut vals = Vec::new();
                let mut featured_vals = Vec::new();

                for (val, featured) in categories {
                    if featured {
                        featured_vals.push(val.clone());
                    }
                    vals.push(val);
                }

                (vals, featured_vals)
            } else {
                (vec![], vec![])
            };

        if let Some((_, versions)) = versions.remove(&project.id) {
            for version in versions {
                let usp = UploadSearchProject {
                    version_id: crate::models::ids::VersionId::from(version.id).to_string(),
                    project_id: crate::models::ids::ProjectId::from(project.id).to_string(),
                    name: project.name.clone(),
                    summary: project.summary.clone(),
                    categories: categories.clone(),
                    display_categories: display_categories.clone(),
                    follows: project.follows,
                    downloads: project.downloads,
                    icon_url: project.icon_url.clone(),
                    author: owner.clone(),
                    date_created: project.approved,
                    created_timestamp: project.approved.timestamp(),
                    date_modified: project.updated,
                    modified_timestamp: project.updated.timestamp(),
                    license: license.clone(),
                    slug: project.slug.clone(),
                    // TODO
                    project_types: vec![],
                    gallery: gallery.clone(),
                    featured_gallery: featured_gallery.clone(),
                    open_source,
                    color: project.color.map(|x| x as u32),
                    // TODO
                    loader_fields: HashMap::new(),
                    // TODO
                    project_loader_fields: HashMap::new(),
                    // TODO
                    loaders: vec![],
                    // TODO
                    versions: vec![],
                };

                uploads.push(usp);
            }
        }
    }

    Ok(uploads)
}
