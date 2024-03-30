use super::ApiError;
use crate::database::redis::RedisPool;
use crate::models::projects::VersionType;
use crate::models::v2::projects::{LegacyProject, LegacyVersion};
use crate::queue::session::AuthQueue;
use crate::routes::v3::version_file::HashQuery;
use crate::routes::{v3, ApiErrorV2};
use crate::util::extract::{ConnectInfo, Extension, Json, Path, Query};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Router;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

pub fn config() -> Router {
    Router::new()
        .nest(
            "/version_file",
            Router::new()
                .route("/:hash", get(get_version_from_hash).delete(delete_file))
                .route("/:hash/download", get(download_version))
                .route("/:hash/update", post(get_update_from_hash))
                .route("/project", post(get_projects_from_hashes)),
        )
        .nest(
            "/version_files",
            Router::new()
                .route("/", post(get_versions_from_hashes))
                .route("/update", post(update_files))
                .route("/update_individual", post(update_individual_files)),
        )
}

// under /api/v1/version_file/{hash}
pub async fn get_version_from_hash(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Query(hash_query): Query<HashQuery>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<Json<LegacyVersion>, ApiErrorV2> {
    let Json(version) = v3::version_file::get_version_from_hash(
        ConnectInfo(addr),
        headers,
        Path(info),
        Extension(pool),
        Extension(redis),
        Query(hash_query),
        Extension(session_queue),
    )
    .await?;

    // Convert response to V2 format
    let version = LegacyVersion::from(version);
    Ok(Json(version))
}

// under /api/v1/version_file/{hash}/download
pub async fn download_version(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Query(hash_query): Query<HashQuery>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<impl IntoResponse, ApiErrorV2> {
    // Returns TemporaryRedirect, so no need to convert to V2
    Ok(v3::version_file::download_version(
        ConnectInfo(addr),
        headers,
        Path(info),
        Extension(pool),
        Extension(redis),
        Query(hash_query),
        Extension(session_queue),
    )
    .await?)
}

// under /api/v1/version_file/{hash}
pub async fn delete_file(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Query(hash_query): Query<HashQuery>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<StatusCode, ApiErrorV2> {
    Ok(v3::version_file::delete_file(
        ConnectInfo(addr),
        headers,
        Path(info),
        Extension(pool),
        Extension(redis),
        Query(hash_query),
        Extension(session_queue),
    )
    .await?)
}

#[derive(Serialize, Deserialize)]
pub struct UpdateData {
    pub loaders: Option<Vec<String>>,
    pub game_versions: Option<Vec<String>>,
    pub version_types: Option<Vec<VersionType>>,
}

pub async fn get_update_from_hash(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Query(hash_query): Query<HashQuery>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
    Json(update_data): Json<UpdateData>,
) -> Result<Json<LegacyVersion>, ApiErrorV2> {
    let mut loader_fields = HashMap::new();
    let mut game_versions = vec![];
    for gv in update_data.game_versions.into_iter().flatten() {
        game_versions.push(serde_json::json!(gv.clone()));
    }
    if !game_versions.is_empty() {
        loader_fields.insert("game_versions".to_string(), game_versions);
    }
    let update_data = v3::version_file::UpdateData {
        loaders: update_data.loaders.clone(),
        version_types: update_data.version_types.clone(),
        loader_fields: Some(loader_fields),
    };

    let Json(version) = v3::version_file::get_update_from_hash(
        ConnectInfo(addr),
        headers,
        Path(info),
        Extension(pool),
        Extension(redis),
        Query(hash_query),
        Extension(session_queue),
        Json(update_data),
    )
    .await?;

    // Convert response to V2 format
    let version = LegacyVersion::from(version);
    Ok(Json(version))
}

// Requests above with multiple versions below
#[derive(Deserialize)]
pub struct FileHashes {
    pub algorithm: Option<String>,
    pub hashes: Vec<String>,
}

// under /api/v2/version_files
pub async fn get_versions_from_hashes(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
    Json(file_data): Json<FileHashes>,
) -> Result<Json<HashMap<String, LegacyVersion>>, ApiErrorV2> {
    let file_data = v3::version_file::FileHashes {
        algorithm: file_data.algorithm,
        hashes: file_data.hashes,
    };
    let Json(map) = v3::version_file::get_versions_from_hashes(
        ConnectInfo(addr),
        headers,
        Extension(pool),
        Extension(redis),
        Extension(session_queue),
        Json(file_data),
    )
    .await?;

    // Convert to V2
    let map = map
        .into_iter()
        .map(|(hash, version)| {
            let v2_version = LegacyVersion::from(version);
            (hash, v2_version)
        })
        .collect::<HashMap<_, _>>();
    Ok(Json(map))
}

pub async fn get_projects_from_hashes(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
    Json(file_data): Json<FileHashes>,
) -> Result<Json<HashMap<String, LegacyProject>>, ApiErrorV2> {
    let file_data = v3::version_file::FileHashes {
        algorithm: file_data.algorithm,
        hashes: file_data.hashes,
    };
    let Json(projects_hashes) = v3::version_file::get_projects_from_hashes(
        ConnectInfo(addr),
        headers,
        Extension(pool.clone()),
        Extension(redis.clone()),
        Extension(session_queue),
        Json(file_data),
    )
    .await?;

    // Convert to V2
    let hash_to_project_id = projects_hashes
        .iter()
        .map(|(hash, project)| {
            let project_id = project.id;
            (hash.clone(), project_id)
        })
        .collect::<HashMap<_, _>>();
    let legacy_projects =
        LegacyProject::from_many(projects_hashes.into_values().collect(), &pool, &redis)
            .await
            .map_err(ApiError::from)?;
    let legacy_projects_hashes = hash_to_project_id
        .into_iter()
        .filter_map(|(hash, project_id)| {
            let legacy_project = legacy_projects.iter().find(|x| x.id == project_id)?.clone();
            Some((hash, legacy_project))
        })
        .collect::<HashMap<_, _>>();

    Ok(Json(legacy_projects_hashes))
}

#[derive(Deserialize)]
pub struct ManyUpdateData {
    pub algorithm: Option<String>, // Defaults to calculation based on size of hash
    pub hashes: Vec<String>,
    pub loaders: Option<Vec<String>>,
    pub game_versions: Option<Vec<String>>,
    pub version_types: Option<Vec<VersionType>>,
}

pub async fn update_files(
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Json(update_data): Json<ManyUpdateData>,
) -> Result<Json<HashMap<String, LegacyVersion>>, ApiErrorV2> {
    let update_data = v3::version_file::ManyUpdateData {
        loaders: update_data.loaders.clone(),
        version_types: update_data.version_types.clone(),
        game_versions: update_data.game_versions.clone(),
        algorithm: update_data.algorithm,
        hashes: update_data.hashes,
    };

    let Json(map) = v3::version_file::update_files(
        Extension(pool),
        Extension(redis),
        Json(update_data),
    )
    .await?;

    // Convert response to V2 format
    let map = map
        .into_iter()
        .map(|(hash, version)| {
            let v2_version = LegacyVersion::from(version);
            (hash, v2_version)
        })
        .collect::<HashMap<_, _>>();
    Ok(Json(map))
}

#[derive(Serialize, Deserialize)]
pub struct FileUpdateData {
    pub hash: String,
    pub loaders: Option<Vec<String>>,
    pub game_versions: Option<Vec<String>>,
    pub version_types: Option<Vec<VersionType>>,
}

#[derive(Deserialize)]
pub struct ManyFileUpdateData {
    pub algorithm: Option<String>, // Defaults to calculation based on size of hash
    pub hashes: Vec<FileUpdateData>,
}

pub async fn update_individual_files(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
    Json(update_data): Json<ManyFileUpdateData>,
) -> Result<Json<HashMap<String, LegacyVersion>>, ApiErrorV2> {
    let update_data = v3::version_file::ManyFileUpdateData {
        algorithm: update_data.algorithm,
        hashes: update_data
            .hashes
            .into_iter()
            .map(|x| {
                let mut loader_fields = HashMap::new();
                let mut game_versions = vec![];
                for gv in x.game_versions.into_iter().flatten() {
                    game_versions.push(serde_json::json!(gv.clone()));
                }
                if !game_versions.is_empty() {
                    loader_fields.insert("game_versions".to_string(), game_versions);
                }
                v3::version_file::FileUpdateData {
                    hash: x.hash.clone(),
                    loaders: x.loaders.clone(),
                    loader_fields: Some(loader_fields),
                    version_types: x.version_types,
                }
            })
            .collect(),
    };

    let Json(version_map) = v3::version_file::update_individual_files(
        ConnectInfo(addr),
        headers,
        Extension(pool),
        Extension(redis),
        Extension(session_queue),
        Json(update_data),
    )
    .await?;

    // Convert response to V2 format
    let version_map = version_map
        .into_iter()
        .map(|(hash, version)| {
            let v2_version = LegacyVersion::from(version);
            (hash, v2_version)
        })
        .collect::<HashMap<_, _>>();
    Ok(Json(version_map))
}
