use crate::database::redis::RedisPool;
use crate::file_hosting::FileHost;
use crate::models::notifications::Notification;
use crate::models::projects::Project;
use crate::models::users::{Badges, Role, User};
use crate::models::v2::notifications::LegacyNotification;
use crate::models::v2::projects::LegacyProject;
use crate::models::v2::user::LegacyUser;
use crate::queue::session::AuthQueue;
use crate::routes::{v2_reroute, v3, ApiError};
use actix_web::{delete, get, patch, web, HttpRequest, HttpResponse};
use lazy_static::lazy_static;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::sync::Arc;
use validator::Validate;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(user_auth_get);
    cfg.service(users_get);

    cfg.service(
        web::scope("user")
            .service(user_get)
            .service(projects_list)
            .service(user_delete)
            .service(user_edit)
            .service(user_icon_edit)
            .service(user_notifications)
            .service(user_follows),
    );
}

#[get("user")]
pub async fn user_auth_get(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<HttpResponse, ApiError> {
    let response = v3::users::user_auth_get(req, pool, redis, session_queue)
        .await
        .or_else(v2_reroute::flatten_404_error)?;

    // Convert response to V2 format
    match v2_reroute::extract_ok_json::<User>(response).await {
        Ok(user) => {
            let user = LegacyUser::from(user);
            Ok(Json(user))
        }
        Err(response) => Ok(response),
    }
}

#[derive(Serialize, Deserialize)]
pub struct UserIds {
    pub ids: String,
}

#[get("users")]
pub async fn users_get(
    Query(ids): Query<UserIds>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
) -> Result<HttpResponse, ApiError> {
    let response = v3::users::users_get(Query(v3::users::UserIds { ids: ids.ids }), pool, redis)
        .await
        .or_else(v2_reroute::flatten_404_error)?;

    // Convert response to V2 format
    match v2_reroute::extract_ok_json::<Vec<User>>(response).await {
        Ok(users) => {
            let legacy_users: Vec<LegacyUser> = users.into_iter().map(LegacyUser::from).collect();
            Ok(Json(legacy_users))
        }
        Err(response) => Ok(response),
    }
}

#[get("{id}")]
pub async fn user_get(
    Path(info): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
) -> Result<HttpResponse, ApiError> {
    let response = v3::users::user_get(info, pool, redis)
        .await
        .or_else(v2_reroute::flatten_404_error)?;

    // Convert response to V2 format
    match v2_reroute::extract_ok_json::<User>(response).await {
        Ok(user) => {
            let user = LegacyUser::from(user);
            Ok(Json(user))
        }
        Err(response) => Ok(response),
    }
}

#[get("{user_id}/projects")]
pub async fn projects_list(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<HttpResponse, ApiError> {
    let response = v3::users::projects_list(req, info, pool.clone(), redis.clone(), session_queue)
        .await
        .or_else(v2_reroute::flatten_404_error)?;

    // Convert to V2 projects
    match v2_reroute::extract_ok_json::<Vec<Project>>(response).await {
        Ok(project) => {
            let legacy_projects = LegacyProject::from_many(project, &pool, &redis).await?;
            Ok(Json(legacy_projects))
        }
        Err(response) => Ok(response),
    }
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

#[patch("{id}")]
pub async fn user_edit(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<String>,
    new_user: Json<EditUser>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<HttpResponse, ApiError> {
    let new_user = new_user.into_inner();
    // Returns NoContent, so we don't need to convert to V2
    v3::users::user_edit(
        req,
        info,
        Json(v3::users::EditUser {
            username: new_user.username,
            name: new_user.name,
            bio: new_user.bio,
            role: new_user.role,
            badges: new_user.badges,
            venmo_handle: None,
        }),
        pool,
        redis,
        session_queue,
    )
    .await
    .or_else(v2_reroute::flatten_404_error)
}

#[derive(Serialize, Deserialize)]
pub struct FileExt {
    pub ext: String,
}

#[patch("{id}/icon")]
#[allow(clippy::too_many_arguments)]
pub async fn user_icon_edit(
    Query(ext): Query<FileExt>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(file_host): Extension<Arc<dyn FileHost + Send + Sync>>,
    payload: web::Payload,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<HttpResponse, ApiError> {
    // Returns NoContent, so we don't need to convert to V2
    v3::users::user_icon_edit(
        Query(v3::users::Extension { ext: ext.ext }),
        req,
        info,
        pool,
        redis,
        file_host,
        payload,
        session_queue,
    )
    .await
    .or_else(v2_reroute::flatten_404_error)
}

#[delete("{id}")]
pub async fn user_delete(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<HttpResponse, ApiError> {
    // Returns NoContent, so we don't need to convert to V2
    v3::users::user_delete(req, info, pool, redis, session_queue)
        .await
        .or_else(v2_reroute::flatten_404_error)
}

#[get("{id}/follows")]
pub async fn user_follows(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<HttpResponse, ApiError> {
    let response = v3::users::user_follows(req, info, pool.clone(), redis.clone(), session_queue)
        .await
        .or_else(v2_reroute::flatten_404_error)?;

    // Convert to V2 projects
    match v2_reroute::extract_ok_json::<Vec<Project>>(response).await {
        Ok(project) => {
            let legacy_projects = LegacyProject::from_many(project, &pool, &redis).await?;
            Ok(Json(legacy_projects))
        }
        Err(response) => Ok(response),
    }
}

#[get("{id}/notifications")]
pub async fn user_notifications(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<HttpResponse, ApiError> {
    let response = v3::users::user_notifications(req, info, pool, redis, session_queue)
        .await
        .or_else(v2_reroute::flatten_404_error)?;
    // Convert response to V2 format
    match v2_reroute::extract_ok_json::<Vec<Notification>>(response).await {
        Ok(notifications) => {
            let legacy_notifications: Vec<LegacyNotification> = notifications
                .into_iter()
                .map(LegacyNotification::from)
                .collect();
            Ok(Json(legacy_notifications))
        }
        Err(response) => Ok(response),
    }
}
