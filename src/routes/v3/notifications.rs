use crate::auth::get_user_from_headers;
use crate::database;
use crate::database::redis::RedisPool;
use crate::models::ids::NotificationId;
use crate::models::notifications::Notification;
use crate::models::pats::Scopes;
use crate::queue::session::AuthQueue;
use crate::routes::ApiError;
use axum::extract::{ConnectInfo, Path, Query};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::get;
use axum::{Extension, Json, Router};
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
        .route(
            "/notification/:id",
            get(notification_get)
                .patch(notification_read)
                .delete(notification_delete),
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
) -> Result<Json<Vec<Notification>>, ApiError> {
    let user = get_user_from_headers(
        &addr,
        &headers,
        &pool,
        &redis,
        &session_queue,
        Some(&[Scopes::NOTIFICATION_READ]),
    )
    .await?
    .1;

    use database::models::notification_item::Notification as DBNotification;
    use database::models::NotificationId as DBNotificationId;

    let notification_ids: Vec<DBNotificationId> =
        serde_json::from_str::<Vec<NotificationId>>(ids.ids.as_str())?
            .into_iter()
            .map(DBNotificationId::from)
            .collect();

    let notifications_data: Vec<DBNotification> =
        database::models::notification_item::Notification::get_many(&notification_ids, &pool)
            .await?;

    let notifications: Vec<Notification> = notifications_data
        .into_iter()
        .filter(|n| n.user_id == user.id.into() || user.role.is_admin())
        .map(Notification::from)
        .collect();

    Ok(Json(notifications))
}

pub async fn notification_get(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<NotificationId>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<Json<Notification>, ApiError> {
    let user = get_user_from_headers(
        &addr,
        &headers,
        &pool,
        &redis,
        &session_queue,
        Some(&[Scopes::NOTIFICATION_READ]),
    )
    .await?
    .1;

    let notification_data =
        database::models::notification_item::Notification::get(info.into(), &pool).await?;

    if let Some(data) = notification_data {
        if user.id == data.user_id.into() || user.role.is_admin() {
            Ok(Json(Notification::from(data)))
        } else {
            Err(ApiError::NotFound)
        }
    } else {
        Err(ApiError::NotFound)
    }
}

pub async fn notification_read(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<NotificationId>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<StatusCode, ApiError> {
    let user = get_user_from_headers(
        &addr,
        &headers,
        &pool,
        &redis,
        &session_queue,
        Some(&[Scopes::NOTIFICATION_WRITE]),
    )
    .await?
    .1;

    let notification_data =
        database::models::notification_item::Notification::get(id.into(), &pool).await?;

    if let Some(data) = notification_data {
        if data.user_id == user.id.into() || user.role.is_admin() {
            let mut transaction = pool.begin().await?;

            database::models::notification_item::Notification::read(
                id.into(),
                &mut transaction,
                &redis,
            )
            .await?;

            transaction.commit().await?;

            Ok(StatusCode::NO_CONTENT)
        } else {
            Err(ApiError::CustomAuthentication(
                "You are not authorized to read this notification!".to_string(),
            ))
        }
    } else {
        Err(ApiError::NotFound)
    }
}

pub async fn notification_delete(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<NotificationId>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<StatusCode, ApiError> {
    let user = get_user_from_headers(
        &addr,
        &headers,
        &pool,
        &redis,
        &session_queue,
        Some(&[Scopes::NOTIFICATION_WRITE]),
    )
    .await?
    .1;

    let notification_data =
        database::models::notification_item::Notification::get(id.into(), &pool).await?;

    if let Some(data) = notification_data {
        if data.user_id == user.id.into() || user.role.is_admin() {
            let mut transaction = pool.begin().await?;

            database::models::notification_item::Notification::remove(
                id.into(),
                &mut transaction,
                &redis,
            )
            .await?;

            transaction.commit().await?;

            Ok(StatusCode::NO_CONTENT)
        } else {
            Err(ApiError::CustomAuthentication(
                "You are not authorized to delete this notification!".to_string(),
            ))
        }
    } else {
        Err(ApiError::NotFound)
    }
}

pub async fn notifications_read(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(ids): Query<NotificationIds>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<StatusCode, ApiError> {
    let user = get_user_from_headers(
        &addr,
        &headers,
        &pool,
        &redis,
        &session_queue,
        Some(&[Scopes::NOTIFICATION_WRITE]),
    )
    .await?
    .1;

    let notification_ids = serde_json::from_str::<Vec<NotificationId>>(&ids.ids)?
        .into_iter()
        .map(|x| x.into())
        .collect::<Vec<_>>();

    let mut transaction = pool.begin().await?;

    let notifications_data =
        database::models::notification_item::Notification::get_many(&notification_ids, &pool)
            .await?;

    let mut notifications: Vec<database::models::ids::NotificationId> = Vec::new();

    for notification in notifications_data {
        if notification.user_id == user.id.into() || user.role.is_admin() {
            notifications.push(notification.id);
        }
    }

    database::models::notification_item::Notification::read_many(
        &notifications,
        &mut transaction,
        &redis,
    )
    .await?;

    transaction.commit().await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn notifications_delete(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(ids): Query<NotificationIds>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<StatusCode, ApiError> {
    let user = get_user_from_headers(
        &addr,
        &headers,
        &pool,
        &redis,
        &session_queue,
        Some(&[Scopes::NOTIFICATION_WRITE]),
    )
    .await?
    .1;

    let notification_ids = serde_json::from_str::<Vec<NotificationId>>(&ids.ids)?
        .into_iter()
        .map(|x| x.into())
        .collect::<Vec<_>>();

    let mut transaction = pool.begin().await?;

    let notifications_data =
        database::models::notification_item::Notification::get_many(&notification_ids, &pool)
            .await?;

    let mut notifications: Vec<database::models::ids::NotificationId> = Vec::new();

    for notification in notifications_data {
        if notification.user_id == user.id.into() || user.role.is_admin() {
            notifications.push(notification.id);
        }
    }

    database::models::notification_item::Notification::remove_many(
        &notifications,
        &mut transaction,
        &redis,
    )
    .await?;

    transaction.commit().await?;

    Ok(StatusCode::NO_CONTENT)
}
