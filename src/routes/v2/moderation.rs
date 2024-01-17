use std::net::SocketAddr;
use std::sync::Arc;

use crate::models::v2::projects::LegacyProject;
use crate::queue::session::AuthQueue;
use crate::routes::{v3, ApiErrorV2};
use crate::database::redis::RedisPool;
use crate::util::extract::{ConnectInfo, Extension, Json, Query};
use axum::Router;
use axum::http::HeaderMap;
use axum::routing::get;
use serde::Deserialize;
use sqlx::PgPool;
use v3::ApiError;

pub fn config() -> Router {
    Router::new()
        .route("/moderation/projects", get(get_projects))
}

#[derive(Deserialize)]
pub struct ResultCount {
    #[serde(default = "default_count")]
    pub count: i16,
}

fn default_count() -> i16 {
    100
}

pub async fn get_projects(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Query(count): Query<ResultCount>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<Json<Vec<LegacyProject>>, ApiErrorV2> {
    let Json(response) = v3::moderation::get_projects(
        ConnectInfo(addr),
        headers,
        Extension(pool.clone()),
        Extension(redis.clone()),
        Query(v3::moderation::ResultCount { count: count.count }),
        Extension(session_queue),
    )
    .await?;

    let legacy_projects = LegacyProject::from_many(response, &pool, &redis).await.map_err(ApiError::from)?;
    Ok(Json(legacy_projects))
}
