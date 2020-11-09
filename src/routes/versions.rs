use super::ApiError;
use crate::auth::{check_is_moderator_from_headers, get_user_from_headers};
use crate::database;
use crate::file_hosting::FileHost;
use crate::models;
use crate::models::users::Role;
use actix_web::{delete, get, patch, web, HttpRequest, HttpResponse};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::sync::Arc;

// TODO: this needs filtering, and a better response type
// Currently it only gives a list of ids, which have to be
// requested manually.  This route could give a list of the
// ids as well as the supported versions and loaders, or
// other info that is needed for selecting the right version.
#[get("version")]
pub async fn version_list(
    info: web::Path<(models::ids::ModId,)>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let id = info.into_inner().0.into();

    let mod_exists = sqlx::query!(
        "SELECT EXISTS(SELECT 1 FROM mods WHERE id = $1)",
        id as database::models::ModId,
    )
    .fetch_one(&**pool)
    .await
    .map_err(|e| ApiError::DatabaseError(e.into()))?
    .exists;

    if mod_exists.unwrap_or(false) {
        let mod_data = database::models::Version::get_mod_versions(id, &**pool)
            .await
            .map_err(|e| ApiError::DatabaseError(e.into()))?;

        let response = mod_data
            .into_iter()
            .map(|v| v.into())
            .collect::<Vec<models::ids::VersionId>>();

        Ok(HttpResponse::Ok().json(response))
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
}

#[derive(Serialize, Deserialize)]
pub struct VersionIds {
    pub ids: String,
}

#[get("versions")]
pub async fn versions_get(
    web::Query(ids): web::Query<VersionIds>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let version_ids = serde_json::from_str::<Vec<models::ids::VersionId>>(&*ids.ids)?
        .into_iter()
        .map(|x| x.into())
        .collect();
    let versions_data = database::models::Version::get_many_full(version_ids, &**pool)
        .await
        .map_err(|e| ApiError::DatabaseError(e.into()))?;

    let versions: Vec<models::mods::Version> = versions_data
        .into_iter()
        .filter_map(|v| v)
        .map(convert_version)
        .collect();

    Ok(HttpResponse::Ok().json(versions))
}

#[get("{version_id}")]
pub async fn version_get(
    info: web::Path<(models::ids::VersionId,)>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let id = info.into_inner().0;
    let version_data = database::models::Version::get_full(id.into(), &**pool)
        .await
        .map_err(|e| ApiError::DatabaseError(e.into()))?;

    if let Some(data) = version_data {
        Ok(HttpResponse::Ok().json(convert_version(data)))
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
}

fn convert_version(data: database::models::version_item::QueryVersion) -> models::mods::Version {
    use models::mods::VersionType;

    models::mods::Version {
        id: data.id.into(),
        mod_id: data.mod_id.into(),
        author_id: data.author_id.into(),

        name: data.name,
        version_number: data.version_number,
        changelog_url: data.changelog_url,
        date_published: data.date_published,
        downloads: data.downloads as u32,
        version_type: match data.release_channel.as_str() {
            "release" => VersionType::Release,
            "beta" => VersionType::Beta,
            "alpha" => VersionType::Alpha,
            "release-hidden" => VersionType::ReleaseHidden,
            "beta-hidden" => VersionType::BetaHidden,
            "alpha-hidden" => VersionType::AlphaHidden,
            _ => VersionType::ReleaseHidden,
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
    }
}

#[derive(Serialize, Deserialize)]
pub struct EditVersion {
    pub name: Option<String>,
    pub version_number: Option<String>,
    pub changelog: Option<String>,
    pub version_type: Option<models::mods::VersionType>,
    pub dependencies: Option<Vec<models::ids::VersionId>>,
    pub game_versions: Option<Vec<models::mods::GameVersion>>,
    pub loaders: Option<Vec<models::mods::ModLoader>>,
}

#[patch("{id}")]
pub async fn version_edit(
    req: HttpRequest,
    info: web::Path<(models::ids::VersionId,)>,
    pool: web::Data<PgPool>,
    file_host: web::Data<Arc<dyn FileHost + Send + Sync>>,
    new_version: web::Json<EditVersion>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(req.headers(), &**pool)
        .await
        .map_err(|_| ApiError::AuthenticationError)?;

    let version_id = info.into_inner().0;
    let id = version_id.into();

    let result = database::models::Version::get_full(id, &**pool)
        .await
        .map_err(|e| ApiError::DatabaseError(e.into()))?;

    if let Some(version_item) = result {
        let is_moderator = user.role == Role::Moderator || user.role == Role::Admin;

        if is_moderator
        /* TODO: Make user be able to edit their own mods, by checking permissions */
        {
            let mut transaction = pool
                .begin()
                .await
                .map_err(|e| ApiError::DatabaseError(e.into()))?;

            if let Some(name) = &new_version.name {
                sqlx::query!(
                    "
                    UPDATE versions
                    SET name = $1
                    WHERE (id = $2)
                    ",
                    name,
                    id as database::models::ids::VersionId,
                )
                .execute(&mut *transaction)
                .await
                .map_err(|e| ApiError::DatabaseError(e.into()))?;
            }

            if let Some(number) = &new_version.version_number {
                sqlx::query!(
                    "
                    UPDATE versions
                    SET version_number = $1
                    WHERE (id = $2)
                    ",
                    number,
                    id as database::models::ids::VersionId,
                )
                .execute(&mut *transaction)
                .await
                .map_err(|e| ApiError::DatabaseError(e.into()))?;
            }

            if let Some(version_type) = &new_version.version_type {
                if version_type.as_str().ends_with("hidden") && !is_moderator {
                    return Err(ApiError::AuthenticationError);
                }

                let channel = database::models::ids::ChannelId::get_id(
                    version_type.as_str(),
                    &mut *transaction,
                )
                .await?
                .ok_or_else(|| {
                    ApiError::InvalidInput(
                        "No database entry for version type provided.".to_string(),
                    )
                })?;

                sqlx::query!(
                    "
                    UPDATE versions
                    SET release_channel = $1
                    WHERE (id = $2)
                    ",
                    channel as database::models::ids::ChannelId,
                    id as database::models::ids::VersionId,
                )
                .execute(&mut *transaction)
                .await
                .map_err(|e| ApiError::DatabaseError(e.into()))?;
            }

            if let Some(dependencies) = &new_version.dependencies {
                sqlx::query!(
                    "
                    DELETE FROM dependencies WHERE dependent_id = $1
                    ",
                    id as database::models::ids::VersionId,
                )
                .execute(&mut *transaction)
                .await
                .map_err(|e| ApiError::DatabaseError(e.into()))?;

                for dependency in dependencies {
                    let dependency_id: database::models::ids::VersionId = dependency.clone().into();

                    sqlx::query!(
                        "
                        INSERT INTO dependencies (dependent_id, dependency_id)
                        VALUES ($1, $2)
                        ",
                        id as database::models::ids::VersionId,
                        dependency_id as database::models::ids::VersionId,
                    )
                    .execute(&mut *transaction)
                    .await
                    .map_err(|e| ApiError::DatabaseError(e.into()))?;
                }
            }

            if let Some(loaders) = &new_version.loaders {
                sqlx::query!(
                    "
                    DELETE FROM loaders_versions WHERE version_id = $1
                    ",
                    id as database::models::ids::VersionId,
                )
                .execute(&mut *transaction)
                .await
                .map_err(|e| ApiError::DatabaseError(e.into()))?;

                for loader in loaders {
                    let loader_id =
                        database::models::categories::Loader::get_id(&loader.0, &mut *transaction)
                            .await?
                            .ok_or_else(|| {
                                ApiError::InvalidInput(
                                    "No database entry for loader provided.".to_string(),
                                )
                            })?;

                    sqlx::query!(
                        "
                        INSERT INTO loaders_versions (loader_id, version_id)
                        VALUES ($1, $2)
                        ",
                        loader_id as database::models::ids::LoaderId,
                        id as database::models::ids::VersionId,
                    )
                    .execute(&mut *transaction)
                    .await
                    .map_err(|e| ApiError::DatabaseError(e.into()))?;
                }
            }

            if let Some(body) = &new_version.changelog {
                let mod_id: models::mods::ModId = version_item.mod_id.into();
                let body_path = format!(
                    "data/{}/versions/{}/changelog.md",
                    mod_id, version_item.version_number
                );

                file_host.delete_file_version("", &*body_path).await?;

                file_host
                    .upload_file("text/plain", &body_path, body.clone().into_bytes())
                    .await?;
            }

            transaction
                .commit()
                .await
                .map_err(|e| ApiError::DatabaseError(e.into()))?;
            Ok(HttpResponse::Ok().body(""))
        } else {
            Err(ApiError::AuthenticationError)
        }
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
}

#[delete("{version_id}")]
pub async fn version_delete(
    req: HttpRequest,
    info: web::Path<(models::ids::VersionId,)>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    check_is_moderator_from_headers(
        req.headers(),
        &mut *pool
            .acquire()
            .await
            .map_err(|e| ApiError::DatabaseError(e.into()))?,
    )
    .await
    .map_err(|_| ApiError::AuthenticationError)?;

    // TODO: check if the mod exists and matches the version id
    let id = info.into_inner().0;
    let result = database::models::Version::remove_full(id.into(), &**pool)
        .await
        .map_err(|e| ApiError::DatabaseError(e.into()))?;

    if result.is_some() {
        Ok(HttpResponse::Ok().body(""))
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
}

// under /api/v1/file/{hash}
#[get("{version_id}")]
pub async fn get_version_from_hash(
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let hash = info.into_inner().0;

    let result = sqlx::query!(
        "
        SELECT version_id FROM files
        INNER JOIN hashes ON hash = $1
        ",
        hash.as_bytes(),
    )
    .fetch_optional(&**pool)
    .await
    .map_err(|e| ApiError::DatabaseError(e.into()))?;

    if let Some(id) = result {
        let version_data = database::models::Version::get_full(
            database::models::VersionId(id.version_id),
            &**pool,
        )
        .await
        .map_err(|e| ApiError::DatabaseError(e.into()))?;

        if let Some(data) = version_data {
            Ok(HttpResponse::Ok().json(convert_version(data)))
        } else {
            Ok(HttpResponse::NotFound().body(""))
        }
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
}

// under /api/v1/file/{hash}
#[delete("{version_id}")]
pub async fn delete_file(
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    file_host: web::Data<Arc<dyn FileHost + Send + Sync>>,
) -> Result<HttpResponse, ApiError> {
    let hash = info.into_inner().0;

    let result = sqlx::query!(
        "
        SELECT version_id, filename FROM files
        INNER JOIN hashes ON hash = $1
        ",
        hash.as_bytes(),
    )
    .fetch_optional(&**pool)
    .await
    .map_err(|e| ApiError::DatabaseError(e.into()))?;

    if let Some(row) = result {
        let version_data = database::models::Version::get_full(
            database::models::VersionId(row.version_id),
            &**pool,
        )
        .await
        .map_err(|e| ApiError::DatabaseError(e.into()))?;

        if let Some(data) = version_data {
            sqlx::query!(
                "
                DELETE FROM hashes
                WHERE hash = $1
                ",
                hash.as_bytes(),
            )
            .execute(&**pool)
            .await
            .map_err(|e| ApiError::DatabaseError(e.into()))?;

            sqlx::query!(
                "
                DELETE FROM files
                WHERE files.version_id = $1
                ",
                data.id as database::models::ids::VersionId,
            )
            .execute(&**pool)
            .await
            .map_err(|e| ApiError::DatabaseError(e.into()))?;

            let mod_id: models::mods::ModId = data.mod_id.into();
            file_host
                .delete_file_version(
                    "",
                    &format!(
                        "data/{}/versions/{}/{}",
                        mod_id, data.version_number, row.filename
                    ),
                )
                .await?;

            Ok(HttpResponse::Ok().body(""))
        } else {
            Ok(HttpResponse::NotFound().body(""))
        }
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
}
