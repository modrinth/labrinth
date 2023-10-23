use super::ApiError;
use crate::auth::{get_user_from_headers, is_authorized_version};
use crate::database::redis::RedisPool;
use crate::models::ids::VersionId;
use crate::models::pats::Scopes;
use crate::models::projects::VersionType;
use crate::queue::session::AuthQueue;
use crate::{database, models};
use actix_web::{web, HttpRequest, HttpResponse};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::collections::HashMap;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("version_file")
            .route("{version_id}/update", web::post().to(get_update_from_hash)),
    );
    cfg.service(
        web::scope("version_files")
            .route("update", web::post().to(update_files))
            .route("update_individual", web::post().to(update_individual_files)),
    );
}

#[derive(Serialize, Deserialize)]
pub struct HashQuery {
    #[serde(default = "default_algorithm")]
    pub algorithm: String,
    pub version_id: Option<VersionId>,
}

pub fn default_algorithm() -> String {
    "sha1".into()
}

#[derive(Deserialize)]
pub struct UpdateData {
    pub loaders: Option<Vec<String>>,
    pub version_types: Option<Vec<VersionType>>,
    /*
       Loader fields to filter with:
       "game_versions": ["1.16.5", "1.17"]

       Returns if it matches any of the values
    */
    pub loader_fields: Option<HashMap<String, Vec<serde_json::Value>>>,
}

// TODO: Requires testing for v2 and v3 (errors were uncaught by cargo test)
pub async fn get_update_from_hash(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    hash_query: web::Query<HashQuery>,
    update_data: web::Json<UpdateData>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let user_option = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::VERSION_READ]),
    )
    .await
    .map(|x| x.1)
    .ok();
    let hash = info.into_inner().0.to_lowercase();

    if let Some(file) = database::models::Version::get_file_from_hash(
        hash_query.algorithm.clone(),
        hash,
        hash_query.version_id.map(|x| x.into()),
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

                        if let Some(loader_fields) = &update_data.loader_fields {
                            for (key, value) in loader_fields {
                                bool &= x.version_fields.iter().any(|y| {
                                    y.field_name == *key
                                        && value.contains(&y.value.serialize_internal())
                                });
                            }
                        }
                        bool
                    })
                    .sorted_by(|a, b| a.inner.date_published.cmp(&b.inner.date_published))
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

#[derive(Deserialize)]
pub struct ManyUpdateData {
    #[serde(default = "default_algorithm")]
    pub algorithm: String,
    pub hashes: Vec<String>,
    pub loaders: Option<Vec<String>>,
    pub loader_fields: Option<HashMap<String, Vec<serde_json::Value>>>,
    pub version_types: Option<Vec<VersionType>>,
}
pub async fn update_files(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    update_data: web::Json<ManyUpdateData>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let user_option = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::VERSION_READ]),
    )
    .await
    .map(|x| x.1)
    .ok();

    let files = database::models::Version::get_files_from_hash(
        update_data.algorithm.clone(),
        &update_data.hashes,
        &**pool,
        &redis,
    )
    .await?;

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
                .filter(|x| x.inner.project_id == file.project_id)
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
                    if let Some(loader_fields) = &update_data.loader_fields {
                        for (key, value) in loader_fields {
                            bool &= x.version_fields.iter().any(|y| {
                                y.field_name == *key
                                    && value.contains(&y.value.serialize_internal())
                            });
                        }
                    }

                    bool
                })
                .sorted_by(|a, b| b.inner.date_published.cmp(&a.inner.date_published))
                .next();

            if let Some(version) = version {
                if is_authorized_version(&version.inner, &user_option, &pool).await? {
                    if let Some(hash) = file.hashes.get(&update_data.algorithm) {
                        response.insert(
                            hash.clone(),
                            models::projects::Version::from(version.clone()),
                        );
                    }
                }
            }
        }
    }

    Ok(HttpResponse::Ok().json(response))
}

#[derive(Deserialize)]
pub struct FileUpdateData {
    pub hash: String,
    pub loaders: Option<Vec<String>>,
    pub loader_fields: Option<HashMap<String, Vec<serde_json::Value>>>,
    pub version_types: Option<Vec<VersionType>>,
}

#[derive(Deserialize)]
pub struct ManyFileUpdateData {
    #[serde(default = "default_algorithm")]
    pub algorithm: String,
    pub hashes: Vec<FileUpdateData>,
}

pub async fn update_individual_files(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    update_data: web::Json<ManyFileUpdateData>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let user_option = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::VERSION_READ]),
    )
    .await
    .map(|x| x.1)
    .ok();

    let files = database::models::Version::get_files_from_hash(
        update_data.algorithm.clone(),
        &update_data
            .hashes
            .iter()
            .map(|x| x.hash.clone())
            .collect::<Vec<_>>(),
        &**pool,
        &redis,
    )
    .await?;

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
            if let Some(hash) = file.hashes.get(&update_data.algorithm) {
                if let Some(query_file) = update_data.hashes.iter().find(|x| &x.hash == hash) {
                    let version = all_versions
                        .iter()
                        .filter(|x| x.inner.project_id == file.project_id)
                        .filter(|x| {
                            let mut bool = true;

                            if let Some(version_types) = &query_file.version_types {
                                bool &= version_types
                                    .iter()
                                    .any(|y| y.as_str() == x.inner.version_type);
                            }
                            if let Some(loaders) = &query_file.loaders {
                                bool &= x.loaders.iter().any(|y| loaders.contains(y));
                            }
                            if let Some(loader_fields) = &query_file.loader_fields {
                                for (key, value) in loader_fields {
                                    bool &= x.version_fields.iter().any(|y| {
                                        y.field_name == *key
                                            && value.contains(&y.value.serialize_internal())
                                    });
                                }
                            }

                            bool
                        })
                        .sorted_by(|a, b| b.inner.date_published.cmp(&a.inner.date_published))
                        .next();

                    if let Some(version) = version {
                        if is_authorized_version(&version.inner, &user_option, &pool).await? {
                            response.insert(
                                hash.clone(),
                                models::projects::Version::from(version.clone()),
                            );
                        }
                    }
                }
            }
        }
    }

    Ok(HttpResponse::Ok().json(response))
}
