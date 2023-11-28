use std::collections::HashMap;
use std::sync::Arc;

use dashmap::DashSet;
use futures::TryStreamExt;
use log::info;

use super::IndexingError;
use crate::database::models::loader_fields::VersionField;
use crate::database::models::{ProjectId, VersionId, project_item, version_item, self};
use crate::database::redis::RedisPool;
use crate::search::UploadSearchProject;
use sqlx::postgres::PgPool;

pub async fn index_local(
    pool: PgPool,
    redis: &RedisPool,
) -> Result<(Vec<UploadSearchProject>, Vec<String>), IndexingError> {
    info!("Indexing local projects!");
    let loader_field_keys: Arc<DashSet<String>> = Arc::new(DashSet::new());

    let all_visible_ids : HashMap<VersionId, (ProjectId, String)> = sqlx::query!(
        "
        SELECT v.id id, m.id mod_id, u.username owner_username
        
        FROM versions v
        INNER JOIN mods m ON v.mod_id = m.id AND m.status = ANY($2)
        INNER JOIN team_members tm ON tm.team_id = m.team_id AND tm.role = $3 AND tm.accepted = TRUE
        INNER JOIN users u ON tm.user_id = u.id
        WHERE v.status != ANY($1)
        GROUP BY v.id, m.id, u.id
        ORDER BY m.id DESC;
        ",
        &*crate::models::projects::VersionStatus::iterator().filter(|x| x.is_hidden()).map(|x| x.to_string()).collect::<Vec<String>>(),
        &*crate::models::projects::ProjectStatus::iterator().filter(|x| x.is_searchable()).map(|x| x.to_string()).collect::<Vec<String>>(),
        crate::models::teams::OWNER_ROLE,
    ).fetch_many(&pool)
            .try_filter_map(|e| {
                async move {
                    Ok(e.right().map(|m| {
                        let project_id: ProjectId = ProjectId(m.mod_id).into();
                        let version_id: VersionId = VersionId(m.id).into();
                        (version_id, (project_id, m.owner_username))
                    }))
                }
            })
            .try_collect::<HashMap<_,_>>()
            .await?;


    let project_ids = all_visible_ids.values().map(|(project_id, _)| project_id).cloned().collect::<Vec<_>>();
    let projects : HashMap<_,_> = project_item::Project::get_many_ids (&project_ids,&pool, &mut redis).await?.into_iter().map(|p| (p.inner.id, p) ).collect();

    let version_ids = all_visible_ids.iter().map(|(version_id, _)| version_id).cloned().collect::<Vec<_>>();
    let versions : HashMap<_,_> = version_item::Version::get_many(&version_ids,&pool, &mut redis).await?.into_iter().map(|v| (v.inner.id, v) ).collect();

    let mut uploads = Vec::new();
    for (version_id, (project_id, owner_username)) in all_visible_ids {
        let m = projects.get(&project_id);
        let v = versions.get(&version_id);

        let m = match m {
            Some(m) => m,
            None => continue,
        };

        let v = match v {
            Some(v) => v,
            None => continue,
        };
        
        let version_id : crate::models::projects::VersionId = v.inner.id.into();
        let project_id : crate::models::projects::ProjectId = m.inner.id.into();

        let mut additional_categories = m.additional_categories;
        let mut categories = m.categories;

        categories.append(&mut m.inner.loaders);

        let display_categories = categories.clone();
        categories.append(&mut additional_categories);

        let version_fields = v.version_fields;
        let loader_fields : HashMap<String, Vec<String>> = version_fields.into_iter().map(|vf| {
            (vf.field_name, vf.value.as_strings())
        }).collect();
        for v in loader_fields.keys().cloned() {
            loader_field_keys.insert(v);
        }

        let license = match m.inner.license.split(' ').next() {
            Some(license) => license.to_string(),
            None => m.inner.license,
        };

        let open_source = match spdx::license_id(&license) {
            Some(id) => id.is_osi_approved(),
            _ => false,
        };

        // SPECIAL BEHAVIOUR
        // Todo: revisit.
        // For consistency with v2 searching, we consider the loader field 'mrpack_loaders' to be a category.
        // These were previously considered the loader, and in v2, the loader is a category for searching.
        // So to avoid breakage or awkward conversions, we just consider those loader_fields to be categories.
        // The loaders are kept in loader_fields as well, so that no information is lost on retrieval.
        let mrpack_loaders = loader_fields.get("mrpack_loaders").cloned().unwrap_or_default();
        categories.extend(mrpack_loaders);


        let usp = UploadSearchProject {
            version_id: version_id.to_string(),
            project_id: project_id.to_string(),
            title: m.inner.title,
            description: m.inner.description,
            categories,
            follows: m.inner.follows,
            downloads: m.inner.downloads,
            icon_url: m.inner.icon_url.unwrap_or_default(),
            author: owner_username,
            date_created: m.inner.approved.unwrap_or(m.inner.published),
            created_timestamp: m.inner.approved.unwrap_or(m.inner.published).timestamp(),
            date_modified: m.inner.updated,
            modified_timestamp: m.inner.updated.timestamp(),
            license,
            slug: m.inner.slug,
            project_types: m.project_types.unwrap_or_default(),
            gallery: m.inner.gallery.unwrap_or_default(),
            display_categories,
            open_source,
            color: m.inner.color.map(|x| x as u32),
            featured_gallery: m.inner.featured_gallery.unwrap_or_default().first().cloned(),
            loader_fields
        };

        uploads.push(usp);
    }

    Ok((
        uploads,
        Arc::try_unwrap(loader_field_keys)
            .unwrap_or_default()
            .into_iter()
            .collect(),
    ))
}
