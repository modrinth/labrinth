use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use super::ApiError;
use crate::database::redis::RedisPool;
use crate::models;
use crate::models::ids::VersionId;
use crate::models::projects::{Dependency, FileType, VersionStatus, VersionType};
use crate::models::v2::projects::LegacyVersion;
use crate::queue::session::AuthQueue;
use crate::routes::v3;
use crate::search::SearchConfig;
use crate::util::extract::{ConnectInfo, Extension, Json, Query, Path};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{post, get};
use serde::{Deserialize, Serialize};
use axum::Router;

use sqlx::PgPool;
use validator::Validate;

pub fn config() -> Router {
    Router::new()
        .route("/versions", get(versions_get))
        .route("/version", post(super::version_creation::version_create))
        .nest(
            "/version",
            Router::new()
                .route("/:slug", get(version_get).patch(version_edit).delete(version_delete))
                .route("/:slug/file", post(super::version_creation::upload_file_to_version))

        )
}

#[derive(Serialize, Deserialize, Clone)]
pub struct VersionListFilters {
    pub game_versions: Option<String>,
    pub loaders: Option<String>,
    pub featured: Option<bool>,
    pub version_type: Option<VersionType>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

pub async fn version_list(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<String>,
    Query(filters): Query<VersionListFilters>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<Json<Vec<LegacyVersion>>, ApiError> {
    let loaders = if let Some(loaders) = filters.loaders {
        if let Ok(mut loaders) = serde_json::from_str::<Vec<String>>(&loaders) {
            loaders.push("mrpack".to_string());
            Some(loaders)
        } else {
            None
        }
    } else {
        None
    };

    let loader_fields = if let Some(game_versions) = filters.game_versions {
        // TODO: extract this logic which is similar to the other v2->v3 version_file functions
        let mut loader_fields = HashMap::new();
        serde_json::from_str::<Vec<String>>(&game_versions)
            .ok()
            .and_then(|versions| {
                let mut game_versions: Vec<serde_json::Value> = vec![];
                for gv in versions {
                    game_versions.push(serde_json::json!(gv.clone()));
                }
                loader_fields.insert("game_versions".to_string(), game_versions);

                if let Some(ref loaders) = loaders {
                    loader_fields.insert(
                        "loaders".to_string(),
                        loaders
                            .iter()
                            .map(|x| serde_json::json!(x.clone()))
                            .collect(),
                    );
                }

                serde_json::to_string(&loader_fields).ok()
            })
    } else {
        None
    };

    let filters = v3::versions::VersionListFilters {
        loader_fields,
        loaders: loaders.and_then(|x| serde_json::to_string(&x).ok()),
        featured: filters.featured,
        version_type: filters.version_type,
        limit: filters.limit,
        offset: filters.offset,
    };

    let Json(versions) =
        v3::versions::version_list(
            ConnectInfo(addr),
            headers,
            Path(info),
            Query(filters),
            Extension(pool),
            Extension(redis),
            Extension(session_queue),
        )
            .await?;

    // Convert response to V2 format
    let versions = versions
        .into_iter()
        .map(LegacyVersion::from)
        .collect::<Vec<_>>();
    Ok(Json(versions))
}

// Given a project ID/slug and a version slug
pub async fn version_project_get(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<(String, String)>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<Json<LegacyVersion>, ApiError> {
    let Json(response) = v3::versions::version_project_get(
        ConnectInfo(addr),
        headers,
        Path(info),
        Extension(pool),
        Extension(redis),
        Extension(session_queue),
    )
        .await?;

    // Convert response to V2 format
    let version = LegacyVersion::from(response);
    Ok(Json(version))
}

#[derive(Serialize, Deserialize)]
pub struct VersionIds {
    pub ids: String,
}

pub async fn versions_get(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(ids): Query<VersionIds>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<Json<Vec<LegacyVersion>>, ApiError> {
    let ids = v3::versions::VersionIds { ids: ids.ids };
    let Json(versions) = v3::versions::versions_get(
        ConnectInfo(addr),
        headers,
        Query(ids),
        Extension(pool),
        Extension(redis),
        Extension(session_queue),
    )
        .await?;

    // Convert response to V2 format
    let versions = versions
        .into_iter()
        .map(LegacyVersion::from)
        .collect::<Vec<_>>();
    Ok(Json(versions))
}

pub async fn version_get(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<models::ids::VersionId>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<Json<LegacyVersion>, ApiError> {
    let Json(version) = v3::versions::version_get(
        ConnectInfo(addr),
        headers,
        Path(info),
        Extension(pool),
        Extension(redis),
        Extension(session_queue),        
    )
        .await?;

    // Convert response to V2 format
    let version = LegacyVersion::from(version);
    Ok(Json(version))
}

#[derive(Serialize, Deserialize, Validate)]
pub struct EditVersion {
    #[validate(
        length(min = 1, max = 64),
        custom(function = "crate::util::validate::validate_name")
    )]
    pub name: Option<String>,
    #[validate(
        length(min = 1, max = 32),
        regex = "crate::util::validate::RE_URL_SAFE"
    )]
    pub version_number: Option<String>,
    #[validate(length(max = 65536))]
    pub changelog: Option<String>,
    pub version_type: Option<models::projects::VersionType>,
    #[validate(
        length(min = 0, max = 4096),
        custom(function = "crate::util::validate::validate_deps")
    )]
    pub dependencies: Option<Vec<Dependency>>,
    pub game_versions: Option<Vec<String>>,
    pub loaders: Option<Vec<models::projects::Loader>>,
    pub featured: Option<bool>,
    pub primary_file: Option<(String, String)>,
    pub downloads: Option<u32>,
    pub status: Option<VersionStatus>,
    pub file_types: Option<Vec<EditVersionFileType>>,
}

#[derive(Serialize, Deserialize)]
pub struct EditVersionFileType {
    pub algorithm: String,
    pub hash: String,
    pub file_type: Option<FileType>,
}

pub async fn version_edit(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<VersionId>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
    Json(new_version): Json<EditVersion>,
) -> Result<StatusCode, ApiError> {

    let mut fields = HashMap::new();
    if new_version.game_versions.is_some() {
        fields.insert(
            "game_versions".to_string(),
            serde_json::json!(new_version.game_versions),
        );
    }

    // Get the older version to get info from
    let Json(old_version) = v3::versions::version_get(
        ConnectInfo(addr),
        headers.clone(),
        Path(info),
        Extension(pool.clone()),
        Extension(redis.clone()),
        Extension(session_queue.clone())
    )
    .await?;

    // If this has 'mrpack_loaders' as a loader field previously, this is a modpack.
    // Therefore, if we are modifying the 'loader' field in this case,
    // we are actually modifying the 'mrpack_loaders' loader field
    let mut loaders = new_version.loaders.clone();
    if old_version.fields.contains_key("mrpack_loaders") && new_version.loaders.is_some() {
        fields.insert(
            "mrpack_loaders".to_string(),
            serde_json::json!(new_version.loaders),
        );
        loaders = None;
    }

    let new_version = v3::versions::EditVersion {
        name: new_version.name,
        version_number: new_version.version_number,
        changelog: new_version.changelog,
        version_type: new_version.version_type,
        dependencies: new_version.dependencies,
        loaders,
        featured: new_version.featured,
        primary_file: new_version.primary_file,
        downloads: new_version.downloads,
        status: new_version.status,
        file_types: new_version.file_types.map(|v| {
            v.into_iter()
                .map(|evft| v3::versions::EditVersionFileType {
                    algorithm: evft.algorithm,
                    hash: evft.hash,
                    file_type: evft.file_type,
                })
                .collect::<Vec<_>>()
        }),
        ordering: None,
        fields,
    };

    Ok(v3::versions::version_edit(
        ConnectInfo(addr),
        headers,
        Path(info),
        Extension(pool),
        Extension(redis),
        Extension(session_queue),
        Json(new_version),
    )
    .await?)
}

pub async fn version_delete(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<VersionId>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
    Extension(search_config): Extension<SearchConfig>,
) -> Result<StatusCode, ApiError> {

    Ok(v3::versions::version_delete(
        ConnectInfo(addr),
        headers,
        Path(info),
        Extension(pool),
        Extension(redis),
        Extension(session_queue),
        Extension(search_config),
    )
        .await?)
}
