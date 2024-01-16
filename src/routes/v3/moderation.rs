use super::ApiError;
use crate::database;
use crate::database::redis::RedisPool;
use crate::models::projects::ProjectStatus;
use crate::queue::session::AuthQueue;
use crate::{auth::check_is_moderator_from_headers, models::pats::Scopes};
use axum::http::HeaderMap;
use axum::routing::get;
use axum::{Router};
use serde::Deserialize;
use sqlx::PgPool;
use std::net::SocketAddr;
use std::sync::Arc;
use crate::util::extract::{Json, Query, Extension, ConnectInfo};

pub fn config() -> Router {
    Router::new().route("/moderation/projects", get(get_projects))
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
) -> Result<Json<Vec<crate::models::projects::Project>>, ApiError> {
    check_is_moderator_from_headers(
        &addr,
        &headers,
        &pool,
        &redis,
        &session_queue,
        Some(&[Scopes::PROJECT_READ]),
    )
    .await?;

    use futures::stream::TryStreamExt;

    let project_ids = sqlx::query!(
        "
        SELECT id FROM mods
        WHERE status = $1
        ORDER BY queued ASC
        LIMIT $2;
        ",
        ProjectStatus::Processing.as_str(),
        count.count as i64
    )
    .fetch_many(&pool)
    .try_filter_map(|e| async { Ok(e.right().map(|m| database::models::ProjectId(m.id))) })
    .try_collect::<Vec<database::models::ProjectId>>()
    .await?;

    let projects: Vec<_> = database::Project::get_many_ids(&project_ids, &pool, &redis)
        .await?
        .into_iter()
        .map(crate::models::projects::Project::from)
        .collect();

    Ok(Json(projects))
}
