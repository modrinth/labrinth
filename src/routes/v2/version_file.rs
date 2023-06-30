use super::ApiError;
use crate::models::ids::VersionId;
use crate::models::projects::{Project, Version, VersionType};
use crate::models::teams::Permissions;
use crate::auth::{
    filter_authorized_projects, filter_authorized_versions, get_user_from_headers,
    is_authorized_version,
};
use crate::{database, models};
use actix_web::{delete, get, post, web, HttpRequest, HttpResponse};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::collections::HashMap;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("version_file")
            .service(delete_file)
            .service(get_version_from_hash)
            .service(download_version)
            .service(get_update_from_hash),
    );

    cfg.service(
        web::scope("version_files")
            .service(get_versions_from_hashes)
            .service(update_files),
    );
}

#[derive(Deserialize)]
pub struct HashQuery {
    #[serde(default = "default_algorithm")]
    pub algorithm: String,
    pub version_id: Option<VersionId>,
}

fn default_algorithm() -> String {
    "sha1".into()
}

// under /api/v1/version_file/{hash}
#[get("{version_id}")]
pub async fn get_version_from_hash(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<deadpool_redis::Pool>,
    hash_query: web::Query<HashQuery>,
) -> Result<HttpResponse, ApiError> {
    let user_option = get_user_from_headers(req.headers(), &**pool).await.ok();

    let hash = info.into_inner().0.to_lowercase();
    let file = database::models::Version::get_file_from_hash(
        hash_query.algorithm.clone(),
        hash,
        &**pool,
        &redis,
    )
    .await?;

    if let Some(file) = file {
        let version = database::models::Version::get(file.version_id, &**pool, &redis).await?;

        if let Some(version) = version {
            if !is_authorized_version(&version.inner, &user_option, &pool).await? {
                return Ok(HttpResponse::NotFound().body(""));
            }

            Ok(HttpResponse::Ok().json(models::projects::Version::from(version)))
        } else {
            Ok(HttpResponse::NotFound().body(""))
        }
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
}

#[derive(Serialize, Deserialize)]
pub struct DownloadRedirect {
    pub url: String,
}

// under /api/v1/version_file/{hash}/download
#[get("{version_id}/download")]
pub async fn download_version(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<deadpool_redis::Pool>,
    hash_query: web::Query<HashQuery>,
) -> Result<HttpResponse, ApiError> {
    let user_option = get_user_from_headers(req.headers(), &**pool).await.ok();

    let hash = info.into_inner().0.to_lowercase();
    let file = database::models::Version::get_file_from_hash(
        hash_query.algorithm.clone(),
        hash,
        &**pool,
        &redis,
    )
    .await?;

    if let Some(file) = file {
        let version = database::models::Version::get(file.version_id, &**pool, &redis).await?;

        if let Some(version) = version {
            if !is_authorized_version(&version.inner, &user_option, &pool).await? {
                return Ok(HttpResponse::NotFound().body(""));
            }

            Ok(HttpResponse::TemporaryRedirect()
                .append_header(("Location", &*file.url))
                .json(DownloadRedirect { url: file.url }))
        } else {
            Ok(HttpResponse::NotFound().body(""))
        }
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
}

// under /api/v1/version_file/{hash}
#[delete("{version_id}")]
pub async fn delete_file(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<deadpool_redis::Pool>,
    hash_query: web::Query<HashQuery>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(req.headers(), &**pool).await?;

    let hash = info.into_inner().0.to_lowercase();

    // TODO: find a way to fit this with the cache
    let result = sqlx::query!(
        "
        SELECT f.id id, f.version_id version_id, f.filename filename, v.version_number version_number, v.mod_id project_id FROM hashes h
        INNER JOIN files f ON h.file_id = f.id
        INNER JOIN versions v ON v.id = f.version_id
        WHERE h.algorithm = $2 AND h.hash = $1
        ORDER BY v.date_published ASC
        ",
        hash.as_bytes(),
        hash_query.algorithm
    )
        .fetch_all(&**pool)
        .await?;

    if let Some(row) = result.iter().find_or_first(|x| {
        hash_query.version_id.is_none()
            || Some(x.version_id) == hash_query.version_id.map(|x| x.0 as i64)
    }) {
        if !user.role.is_admin() {
            let team_member = database::models::TeamMember::get_from_user_id_version(
                database::models::ids::VersionId(row.version_id),
                user.id.into(),
                &**pool,
            )
            .await
            .map_err(ApiError::Database)?
            .ok_or_else(|| {
                ApiError::CustomAuthentication(
                    "You don't have permission to delete this file!".to_string(),
                )
            })?;

            if !team_member
                .permissions
                .contains(Permissions::DELETE_VERSION)
            {
                return Err(ApiError::CustomAuthentication(
                    "You don't have permission to delete this file!".to_string(),
                ));
            }
        }

        use futures::stream::TryStreamExt;

        let files = sqlx::query!(
            "
            SELECT f.id id FROM files f
            WHERE f.version_id = $1
            ",
            row.version_id
        )
        .fetch_many(&**pool)
        .try_filter_map(|e| async { Ok(e.right().map(|_| ())) })
        .try_collect::<Vec<()>>()
        .await?;

        if files.len() < 2 {
            return Err(ApiError::InvalidInput(
                "Versions must have at least one file uploaded to them".to_string(),
            ));
        }

        let mut transaction = pool.begin().await?;

        sqlx::query!(
            "
            DELETE FROM hashes
            WHERE file_id = $1
            ",
            row.id
        )
        .execute(&mut *transaction)
        .await?;

        sqlx::query!(
            "
            DELETE FROM files
            WHERE files.id = $1
            ",
            row.id,
        )
        .execute(&mut *transaction)
        .await?;

        database::models::Version::clear_cache(
            database::models::ids::VersionId(row.version_id),
            &redis,
        )
        .await?;

        transaction.commit().await?;

        Ok(HttpResponse::NoContent().body(""))
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
}

#[derive(Deserialize)]
pub struct UpdateData {
    pub loaders: Option<Vec<String>>,
    pub game_versions: Option<Vec<String>>,
    pub version_types: Option<Vec<VersionType>>,
}

#[post("{version_id}/update")]
pub async fn get_update_from_hash(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<deadpool_redis::Pool>,
    hash_query: web::Query<HashQuery>,
    update_data: web::Json<UpdateData>,
) -> Result<HttpResponse, ApiError> {
    let user_option = get_user_from_headers(req.headers(), &**pool).await.ok();
    let hash = info.into_inner().0.to_lowercase();

    if let Some(file) = database::models::Version::get_file_from_hash(
        hash_query.algorithm.clone(),
        hash,
        &**pool,
        &redis,
    )
    .await?
    {
        if let Some(project) =
            database::models::Project::get_id(file.project_id, &**pool, &redis).await?
        {
            let mut versions =
                database::models::Version::get_many(&project.versions, &**pool, &redis)
                    .await?
                    .into_iter()
                    .filter(|x| {
                        let mut bool = true;

                        if let Some(version_types) = &update_data.version_types {
                            bool &= version_types
                                .iter()
                                .any(|y| y.as_str() == x.inner.version_type);
                        }
                        if let Some(loaders) = &update_data.loaders {
                            bool &= x.loaders.iter().any(|y| loaders.contains(y));
                        }
                        if let Some(game_versions) = &update_data.game_versions {
                            bool &= x.game_versions.iter().any(|y| game_versions.contains(y));
                        }

                        bool
                    })
                    .sorted_by(|a, b| b.inner.date_published.cmp(&a.inner.date_published))
                    .collect::<Vec<_>>();

            if let Some(first) = versions.pop() {
                if !is_authorized_version(&first.inner, &user_option, &pool).await? {
                    return Ok(HttpResponse::NotFound().body(""));
                }

                return Ok(HttpResponse::Ok().json(models::projects::Version::from(first)));
            }
        }
    }

    Ok(HttpResponse::NotFound().body(""))
}

// Requests above with multiple versions below
#[derive(Deserialize)]
pub struct FileHashes {
    pub algorithm: String,
    pub hashes: Vec<String>,
}

// under /api/v2/version_files
#[post("")]
pub async fn get_versions_from_hashes(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    redis: web::Data<deadpool_redis::Pool>,
    file_data: web::Json<FileHashes>,
) -> Result<HttpResponse, ApiError> {
    let user_option = get_user_from_headers(req.headers(), &**pool).await.ok();
    let hashes_parsed: Vec<(String, String)> = file_data
        .hashes
        .iter()
        .map(|x| (file_data.algorithm.clone(), x.to_lowercase()))
        .collect();

    let files =
        database::models::Version::get_files_from_hash(&hashes_parsed, &**pool, &redis).await?;

    let version_ids = files.iter().map(|x| x.version_id).collect::<Vec<_>>();
    let versions_data = filter_authorized_versions(
        database::models::Version::get_many(&version_ids, &**pool, &redis).await?,
        &user_option,
        &pool,
    )
    .await?;

    // TODO: switch to for loop like updates
    let response: HashMap<String, Version> = files
        .into_iter()
        .filter_map(|row| {
            versions_data
                .iter()
                .find(|x| database::models::VersionId::from(x.id) == row.version_id)
                .and_then(|v| {
                    row.hashes
                        .get(&file_data.algorithm)
                        .map(|hash| (hash.clone(), v.clone()))
                })
        })
        .collect();
    Ok(HttpResponse::Ok().json(response))
}

#[post("project")]
pub async fn get_projects_from_hashes(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    redis: web::Data<deadpool_redis::Pool>,
    file_data: web::Json<FileHashes>,
) -> Result<HttpResponse, ApiError> {
    let user_option = get_user_from_headers(req.headers(), &**pool).await.ok();
    let hashes_parsed: Vec<(String, String)> = file_data
        .hashes
        .iter()
        .map(|x| (file_data.algorithm.clone(), x.to_lowercase()))
        .collect();

    let files =
        database::models::Version::get_files_from_hash(&hashes_parsed, &**pool, &redis).await?;

    let project_ids = files.iter().map(|x| x.project_id).collect::<Vec<_>>();

    let projects_data = filter_authorized_projects(
        database::models::Project::get_many_ids(&project_ids, &**pool, &redis).await?,
        &user_option,
        &pool,
    )
    .await?;

    // TODO: switch to for loop like updates
    let response: HashMap<String, Project> = files
        .into_iter()
        .filter_map(|row| {
            projects_data
                .iter()
                .find(|x| x.id == row.project_id.into())
                .and_then(|p| {
                    row.hashes
                        .get(&file_data.algorithm)
                        .map(|hash| (hash.clone(), p.clone()))
                })
        })
        .collect();
    Ok(HttpResponse::Ok().json(response))
}

#[derive(Deserialize)]
pub struct ManyUpdateData {
    pub algorithm: String,
    pub hashes: Vec<String>,
    pub loaders: Option<Vec<String>>,
    pub game_versions: Option<Vec<String>>,
    pub version_types: Option<Vec<VersionType>>,
}

#[post("update")]
pub async fn update_files(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    redis: web::Data<deadpool_redis::Pool>,
    update_data: web::Json<ManyUpdateData>,
) -> Result<HttpResponse, ApiError> {
    let user_option = get_user_from_headers(req.headers(), &**pool).await.ok();
    let hashes_parsed: Vec<(String, String)> = update_data
        .hashes
        .iter()
        .map(|x| (update_data.algorithm.clone(), x.to_lowercase()))
        .collect();

    let files =
        database::models::Version::get_files_from_hash(&hashes_parsed, &**pool, &redis).await?;

    let projects = database::models::Project::get_many_ids(
        &files.iter().map(|x| x.project_id).collect::<Vec<_>>(),
        &**pool,
        &redis,
    )
    .await?;
    let all_versions = database::models::Version::get_many(
        &projects
            .iter()
            .flat_map(|x| x.versions.clone())
            .collect::<Vec<_>>(),
        &**pool,
        &redis,
    )
    .await?;

    let mut response = HashMap::new();

    for project in projects {
        for file in files.iter().filter(|x| x.project_id == project.inner.id) {
            let version = all_versions
                .iter()
                .filter(|x| {
                    let mut bool = true;

                    if let Some(version_types) = &update_data.version_types {
                        bool &= version_types
                            .iter()
                            .any(|y| y.as_str() == x.inner.version_type);
                    }
                    if let Some(loaders) = &update_data.loaders {
                        bool &= x.loaders.iter().any(|y| loaders.contains(y));
                    }
                    if let Some(game_versions) = &update_data.game_versions {
                        bool &= x.game_versions.iter().any(|y| game_versions.contains(y));
                    }

                    bool
                })
                .sorted_by(|a, b| b.inner.date_published.cmp(&a.inner.date_published))
                .next();

            if let Some(version) = version {
                if is_authorized_version(&version.inner, &user_option, &pool).await? {
                    response.insert(
                        file.hashes.get(&update_data.algorithm),
                        models::projects::Version::from(version.clone()),
                    );
                }
            }
        }
    }

    Ok(HttpResponse::Ok().json(response))
}
