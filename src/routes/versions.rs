use super::ApiError;
use crate::database;
use crate::models;
use actix_web::{get, web, HttpResponse};
use sqlx::PgPool;

#[get("api/v1/mod/{mod_id}/version/{version_id}")]
pub async fn version_get(
    info: web::Path<(models::ids::ModId, models::ids::VersionId)>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let id = info.1;
    let mod_data = database::models::Version::get_full(id.into(), &**pool)
        .await
        .map_err(|e| ApiError::DatabaseError(e.into()))?;

    if let Some(data) = mod_data {
        use models::mods::VersionType;

        if models::ids::ModId::from(data.mod_id) != info.0 {
            // Version doesn't belong to that mod
            return Ok(HttpResponse::NotFound().body(""));
        }

        let response = models::mods::Version {
            id: data.id.into(),
            mod_id: data.mod_id.into(),

            name: data.name,
            version_number: data.version_number,
            changelog_url: data.changelog_url,
            date_published: data.date_published,
            downloads: data.downloads as u32,
            version_type: match data.release_channel.as_str() {
                "release" => VersionType::Release,
                "beta" => VersionType::Beta,
                "alpha" => VersionType::Alpha,
                _ => VersionType::Alpha,
            },

            files: data
                .files
                .into_iter()
                .map(|f| {
                    models::mods::VersionFile {
                        url: f.url,
                        filename: f.filename,
                        // FIXME: Hashes are currently stored as an ascii byte slice instead
                        // of as an actual byte array in the database
                        hashes: f
                            .hashes
                            .into_iter()
                            .map(|(k, v)| Some((k, String::from_utf8(v).ok()?)))
                            .collect::<Option<_>>()
                            .unwrap_or_else(Default::default),
                    }
                })
                .collect(),
            dependencies: Vec::new(), // TODO: dependencies
            game_versions: data
                .game_versions
                .into_iter()
                .map(models::mods::GameVersion)
                .collect(),
            loaders: data
                .loaders
                .into_iter()
                .map(models::mods::ModLoader)
                .collect(),
        };
        Ok(HttpResponse::Ok().json(response))
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
}
