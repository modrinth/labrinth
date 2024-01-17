use crate::database::redis::RedisPool;
use crate::file_hosting::FileHost;
use crate::models::users::{Badges, Role};
use crate::models::v2::notifications::LegacyNotification;
use crate::models::v2::projects::LegacyProject;
use crate::models::v2::user::LegacyUser;
use crate::queue::session::AuthQueue;
use crate::routes::{v3, ApiError};
use crate::util::extract::{ConnectInfo, Extension, Json, Path, Query};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, patch};
use axum::Router;
use lazy_static::lazy_static;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::net::SocketAddr;
use std::sync::Arc;
use validator::Validate;

pub fn config() -> Router {
    Router::new()
        .route("/user", get(user_auth_get))
        .route("/users", get(users_get))
        .nest(
            "/user",
            Router::new()
                .route("/:id/projects", get(projects_list))
                .route("/:id", get(user_get).patch(user_edit).delete(user_delete))
                .route("/:id/icon", patch(user_icon_edit))
                .route("/:id/follows", get(user_follows))
                .route("/:id/notifications", get(user_notifications)),
        )
}

pub async fn user_auth_get(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<Json<LegacyUser>, ApiError> {
    let Json(user) = v3::users::user_auth_get(
        ConnectInfo(addr),
        headers,
        Extension(pool),
        Extension(redis),
        Extension(session_queue),
    )
    .await?;

    // Convert response to V2 format
    let user = LegacyUser::from(user);
    Ok(Json(user))
}

#[derive(Serialize, Deserialize)]
pub struct UserIds {
    pub ids: String,
}

pub async fn users_get(
    Query(ids): Query<UserIds>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
) -> Result<Json<Vec<LegacyUser>>, ApiError> {
    let Json(users) = v3::users::users_get(
        Query(v3::users::UserIds { ids: ids.ids }),
        Extension(pool),
        Extension(redis),
    )
    .await?;

    // Convert response to V2 format
    let users = users.into_iter().map(LegacyUser::from).collect();
    Ok(Json(users))
}

pub async fn user_get(
    Path(info): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
) -> Result<Json<LegacyUser>, ApiError> {
    let Json(user) = v3::users::user_get(Path(info), Extension(pool), Extension(redis)).await?;

    // Convert response to V2 format
    let user = LegacyUser::from(user);
    Ok(Json(user))
}

pub async fn projects_list(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<Json<Vec<LegacyProject>>, ApiError> {
    let Json(projects) = v3::users::projects_list(
        ConnectInfo(addr),
        headers,
        Path(info),
        Extension(pool.clone()),
        Extension(redis.clone()),
        Extension(session_queue),
    )
    .await?;

    // Convert to V2 projects
    let projects = LegacyProject::from_many(projects, &pool, &redis).await?;
    Ok(Json(projects))
}

lazy_static! {
    static ref RE_URL_SAFE: Regex = Regex::new(r"^[a-zA-Z0-9_-]*$").unwrap();
}

#[derive(Serialize, Deserialize, Validate)]
pub struct EditUser {
    #[validate(length(min = 1, max = 39), regex = "RE_URL_SAFE")]
    pub username: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    #[validate(length(min = 1, max = 64), regex = "RE_URL_SAFE")]
    pub name: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    #[validate(length(max = 160))]
    pub bio: Option<Option<String>>,
    pub role: Option<Role>,
    pub badges: Option<Badges>,
}

pub async fn user_edit(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
    Json(new_user): Json<EditUser>,
) -> Result<StatusCode, ApiError> {
    Ok(v3::users::user_edit(
        ConnectInfo(addr),
        headers,
        Path(info),
        Extension(pool),
        Extension(redis),
        Extension(session_queue),
        Json(v3::users::EditUser {
            username: new_user.username,
            name: new_user.name,
            bio: new_user.bio,
            role: new_user.role,
            badges: new_user.badges,
            venmo_handle: None,
        }),
    )
    .await?)
}

#[derive(Serialize, Deserialize)]
pub struct FileExt {
    pub ext: String,
}

#[allow(clippy::too_many_arguments)]
pub async fn user_icon_edit(
    Query(ext): Query<FileExt>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(file_host): Extension<Arc<dyn FileHost + Send + Sync>>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
    payload: bytes::Bytes,
) -> Result<StatusCode, ApiError> {
    Ok(v3::users::user_icon_edit(
        Query(v3::users::FileExt { ext: ext.ext }),
        ConnectInfo(addr),
        headers,
        Path(info),
        Extension(pool),
        Extension(redis),
        Extension(file_host),
        Extension(session_queue),
        payload,
    )
    .await?)
}

pub async fn user_delete(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<StatusCode, ApiError> {
    Ok(v3::users::user_delete(
        ConnectInfo(addr),
        headers,
        Path(info),
        Extension(pool),
        Extension(redis),
        Extension(session_queue),
    )
    .await?)
}

pub async fn user_follows(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<Json<Vec<LegacyProject>>, ApiError> {
    let Json(projects) = v3::users::user_follows(
        ConnectInfo(addr),
        headers,
        Path(info),
        Extension(pool.clone()),
        Extension(redis.clone()),
        Extension(session_queue),
    )
    .await?;

    // Convert to V2 projects
    let projects = LegacyProject::from_many(projects, &pool, &redis).await?;
    Ok(Json(projects))
}

pub async fn user_notifications(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<Json<Vec<LegacyNotification>>, ApiError> {
    let Json(notifications) = v3::users::user_notifications(
        ConnectInfo(addr),
        headers,
        Path(info),
        Extension(pool),
        Extension(redis),
        Extension(session_queue),
    )
    .await?;

    // Convert response to V2 format
    let notifications = notifications
        .into_iter()
        .map(LegacyNotification::from)
        .collect::<Vec<_>>();
    Ok(Json(notifications))
}
