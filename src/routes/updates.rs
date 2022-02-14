use std::collections::HashMap;

use actix_web::{get, web, HttpResponse};
use serde::Serialize;
use sqlx::PgPool;

use crate::database;
use crate::models::projects::{Version, VersionType};

use super::ApiError;

#[get("{id}/forge_updates.json")]
pub async fn forge_updates(
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let string = info.into_inner().0;

    let project = database::models::Project::get_from_slug_or_project_id(string.clone(), &**pool)
        .await?
        .ok_or_else(|| {
            ApiError::InvalidInputError("The specified project does not exist!".to_string())
        })?;

    let version_ids = database::models::Version::get_project_versions(
        project.id,
        None,
        Some(vec!["forge".to_string()]),
        &**pool,
    )
    .await?;

    let mut versions = database::models::Version::get_many_full(version_ids, &**pool).await?;
    versions.sort_by(|a, b| b.date_published.cmp(&a.date_published));

    #[derive(Serialize)]
    struct ForgeUpdates {
        homepage: String,
        promos: HashMap<String, String>,
    }

    let mut response = ForgeUpdates {
        homepage: format!(
            "{}/mod/{}",
            dotenv::var("SITE_URL").unwrap_or_default(),
            string
        ),
        promos: HashMap::new(),
    };

    for version in versions {
        let version = Version::from(version);

        if version.version_type == VersionType::Release {
            for game_version in &version.game_versions {
                response
                    .promos
                    .entry(format!("{}-recommended", game_version.0))
                    .or_insert_with(|| version.version_number.clone());
            }
        }

        for game_version in &version.game_versions {
            response
                .promos
                .entry(format!("{}-latest", game_version.0))
                .or_insert_with(|| version.version_number.clone());
        }
    }

    Ok(HttpResponse::Ok().json(response))
}
