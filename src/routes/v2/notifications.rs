use crate::database::redis::RedisPool;
use crate::models::ids::NotificationId;
use crate::models::v2::notifications::LegacyNotification;
use crate::queue::session::AuthQueue;
use crate::routes::v3;
use crate::routes::ApiErrorV2;
use crate::util::extract::{ConnectInfo, Extension, Json, Path, Query};
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::routing::get;
use axum::Router;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::net::SocketAddr;
use std::sync::Arc;

pub fn config() -> Router {
    Router::new()
        .route(
            "/notifications",
            get(notifications_get)
                .patch(notifications_read)
                .delete(notifications_delete),
        )
        .nest(
            "/notification",
            Router::new().route(
                "/:id",
                get(notification_get)
                    .patch(notification_read)
                    .delete(notification_delete),
            ),
        )
}

#[derive(Serialize, Deserialize)]
pub struct NotificationIds {
    pub ids: String,
}

pub async fn notifications_get(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(ids): Query<NotificationIds>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<Json<Vec<LegacyNotification>>, ApiErrorV2> {
    let Json(notifications) = v3::notifications::notifications_get(
        ConnectInfo(addr),
        headers,
        Query(v3::notifications::NotificationIds { ids: ids.ids }),
        Extension(pool),
        Extension(redis),
        Extension(session_queue),
    )
    .await?;

    let legacy_notifications = notifications
        .into_iter()
        .map(LegacyNotification::from)
        .collect();
    Ok(Json(legacy_notifications))
}

pub async fn notification_get(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<NotificationId>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<Json<LegacyNotification>, ApiErrorV2> {
    let Json(notification) = v3::notifications::notification_get(
        ConnectInfo(addr),
        headers,
        Path(info),
        Extension(pool),
        Extension(redis),
        Extension(session_queue),
    )
    .await?;

    Ok(Json(LegacyNotification::from(notification)))
}

pub async fn notification_read(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<NotificationId>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<StatusCode, ApiErrorV2> {
    Ok(v3::notifications::notification_read(
        ConnectInfo(addr),
        headers,
        Path(info),
        Extension(pool),
        Extension(redis),
        Extension(session_queue),
    )
    .await?)
}

pub async fn notification_delete(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<NotificationId>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<StatusCode, ApiErrorV2> {
    Ok(v3::notifications::notification_delete(
        ConnectInfo(addr),
        headers,
        Path(info),
        Extension(pool),
        Extension(redis),
        Extension(session_queue),
    )
    .await?)
}

pub async fn notifications_read(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(ids): Query<NotificationIds>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<StatusCode, ApiErrorV2> {
    Ok(v3::notifications::notifications_read(
        ConnectInfo(addr),
        headers,
        Query(v3::notifications::NotificationIds { ids: ids.ids }),
        Extension(pool),
        Extension(redis),
        Extension(session_queue),
    )
    .await?)
}

pub async fn notifications_delete(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(ids): Query<NotificationIds>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<StatusCode, ApiErrorV2> {
    Ok(v3::notifications::notifications_delete(
        ConnectInfo(addr),
        headers,
        Query(v3::notifications::NotificationIds { ids: ids.ids }),
        Extension(pool),
        Extension(redis),
        Extension(session_queue),
    )
    .await?)
}
