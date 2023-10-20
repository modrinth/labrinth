use futures::TryStreamExt;
use log::info;

use super::IndexingError;
use crate::database::models::ProjectId;
use crate::search::UploadSearchProject;
use sqlx::postgres::PgPool;

pub async fn index_local(pool: PgPool) -> Result<Vec<UploadSearchProject>, IndexingError> {
    info!("Indexing local projects!");

    Ok(
        sqlx::query!(
            "
            SELECT m.id id, v.id version_id, m.project_type project_type, m.title title, m.description description, m.downloads downloads, m.follows follows,
            m.icon_url icon_url, m.published published, m.approved approved, m.updated updated,
            m.team_id team_id, m.license license, m.slug slug, m.status status_name, m.color color,
            pt.name project_type_name, u.username username,
            ARRAY_AGG(DISTINCT c.category) filter (where c.category is not null and mc.is_additional is false) categories,
            ARRAY_AGG(DISTINCT c.category) filter (where c.category is not null and mc.is_additional is true) additional_categories,
            ARRAY_AGG(DISTINCT lo.loader) filter (where lo.loader is not null) loaders,
            ARRAY_AGG(DISTINCT mg.image_url) filter (where mg.image_url is not null and mg.featured is false) gallery,
            ARRAY_AGG(DISTINCT mg.image_url) filter (where mg.image_url is not null and mg.featured is true) featured_gallery,
            JSONB_AGG(
                DISTINCT jsonb_build_object(
                    'field_id', vf.field_id,
                    'int_value', vf.int_value,
                    'enum_value', vf.enum_value,
                    'string_value', vf.string_value,
                    'field', lf.field,
                    'field_type', lf.field_type,
                    'enum_type', lf.enum_type,
                    'enum_name', lfe.enum_name
                )
            ) version_fields

            FROM versions v
            INNER JOIN mods m ON v.mod_id = m.id AND m.status = ANY($2)
            LEFT OUTER JOIN mods_categories mc ON joining_mod_id = m.id
            LEFT OUTER JOIN categories c ON mc.joining_category_id = c.id
            LEFT OUTER JOIN loaders_versions lv ON lv.version_id = v.id
            LEFT OUTER JOIN loaders lo ON lo.id = lv.loader_id
            LEFT OUTER JOIN mods_gallery mg ON mg.mod_id = m.id
            INNER JOIN project_types pt ON pt.id = m.project_type
            INNER JOIN team_members tm ON tm.team_id = m.team_id AND tm.role = $3 AND tm.accepted = TRUE
            INNER JOIN users u ON tm.user_id = u.id
            LEFT OUTER JOIN version_fields vf on v.id = vf.version_id
            LEFT OUTER JOIN loader_fields lf on vf.field_id = lf.id
            LEFT OUTER JOIN loader_field_enums lfe on lf.enum_type = lfe.id
            WHERE v.status != ANY($1)
            GROUP BY v.id, m.id, pt.id, u.id;
            ",
            &*crate::models::projects::VersionStatus::iterator().filter(|x| x.is_hidden()).map(|x| x.to_string()).collect::<Vec<String>>(),
            &*crate::models::projects::ProjectStatus::iterator().filter(|x| x.is_searchable()).map(|x| x.to_string()).collect::<Vec<String>>(),
            crate::models::teams::OWNER_ROLE,
        )
            .fetch_many(&pool)
            .try_filter_map(|e| async {
                Ok(e.right().map(|m| {
                    let mut additional_categories = m.additional_categories.unwrap_or_default();
                    let mut categories = m.categories.unwrap_or_default();

                    categories.append(&mut m.loaders.unwrap_or_default());

                    let display_categories = categories.clone();
                    categories.append(&mut additional_categories);

                    let project_id: crate::models::projects::ProjectId = ProjectId(m.id).into();
                    let version_id: crate::models::projects::ProjectId = ProjectId(m.version_id).into();

                    let license = match m.license.split(' ').next() {
                        Some(license) => license.to_string(),
                        None => m.license,
                    };

                    let open_source = match spdx::license_id(&license) {
                        Some(id) => id.is_osi_approved(),
                        _ => false,
                    };

                    UploadSearchProject {
                        version_id: version_id.to_string(),
                        project_id: project_id.to_string(),
                        title: m.title,
                        description: m.description,
                        categories,
                        follows: m.follows,
                        downloads: m.downloads,
                        icon_url: m.icon_url.unwrap_or_default(),
                        author: m.username,
                        date_created: m.approved.unwrap_or(m.published),
                        created_timestamp: m.approved.unwrap_or(m.published).timestamp(),
                        date_modified: m.updated,
                        modified_timestamp: m.updated.timestamp(),
                        license,
                        slug: m.slug,
                        project_type: m.project_type_name,
                        gallery: m.gallery.unwrap_or_default(),
                        display_categories,
                        open_source,
                        color: m.color.map(|x| x as u32),
                        featured_gallery: m.featured_gallery.unwrap_or_default().first().cloned(),
                    }
                }))
            })
            .try_collect::<Vec<_>>()
            .await?
    )
}
