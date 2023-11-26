use crate::database::redis::RedisPool;
use crate::file_hosting::FileHost;
use crate::models::projects::Project;
use crate::models::users::{Badges, Role};
use crate::models::v2::projects::LegacyProject;
use crate::queue::payouts::PayoutsQueue;
use crate::queue::session::AuthQueue;
use crate::routes::{v2_reroute, v3, ApiError};
use actix_web::{delete, get, patch, post, web, HttpRequest, HttpResponse};
use lazy_static::lazy_static;
use regex::Regex;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::Mutex;
use validator::Validate;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(user_auth_get);
    cfg.service(users_get);

    cfg.service(
        web::scope("user")
            .service(user_get)
            .service(orgs_list)
            .service(projects_list)
            .service(collections_list)
            .service(user_delete)
            .service(user_edit)
            .service(user_icon_edit)
            .service(user_notifications)
            .service(user_follows)
            .service(user_payouts)
            .service(user_payouts_fees)
            .service(user_payouts_request),
    );
}

#[get("user")]
pub async fn user_auth_get(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    v2_reroute::convert_v3_no_extract(v3::users::user_auth_get(req, pool, redis, session_queue).await?)
}

#[derive(Serialize, Deserialize)]
pub struct UserIds {
    pub ids: String,
}

#[get("users")]
pub async fn users_get(
    web::Query(ids): web::Query<UserIds>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
) -> Result<HttpResponse, ApiError> {
    v2_reroute::convert_v3_no_extract(v3::users::users_get(web::Query(v3::users::UserIds { ids: ids.ids }), pool, redis).await?)
}

#[get("{id}")]
pub async fn user_get(
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
) -> Result<HttpResponse, ApiError> {
    v2_reroute::convert_v3_no_extract(v3::users::user_get(info, pool, redis).await?)
}

#[get("{user_id}/projects")]
pub async fn projects_list(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let response =
        v3::users::projects_list(req, info, pool.clone(), redis.clone(), session_queue).await?;

    // Convert to V2 projects
    match v2_reroute::extract_ok_json::<Vec<Project>>(response).await {
        Ok(project) => {
            let legacy_projects = LegacyProject::from_many(project, &**pool, &redis).await?;
            Ok(HttpResponse::Ok().json(legacy_projects))
        }
        Err(response) => Ok(response),
    }
}

#[get("{user_id}/collections")]
pub async fn collections_list(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    v2_reroute::convert_v3_no_extract(v3::users::collections_list(req, info, pool, redis, session_queue).await?)
}

#[get("{user_id}/organizations")]
pub async fn orgs_list(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    v2_reroute::convert_v3_no_extract(v3::users::orgs_list(req, info, pool, redis, session_queue).await?)
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
    req: HttpRequest,
    info: web::Path<(String,)>,
    new_user: web::Json<EditUser>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let new_user = new_user.into_inner();
    v2_reroute::convert_v3_no_extract(v3::users::user_edit(
        req,
        info,
        web::Json(v3::users::EditUser {
            username: new_user.username,
            name: new_user.name,
            bio: new_user.bio,
            role: new_user.role,
            badges: new_user.badges,
        }),
        pool,
        redis,
        session_queue,
    )
    .await?)
}

#[derive(Serialize, Deserialize)]
pub struct Extension {
    pub ext: String,
}

#[patch("{id}/icon")]
#[allow(clippy::too_many_arguments)]
pub async fn user_icon_edit(
    web::Query(ext): web::Query<Extension>,
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    file_host: web::Data<Arc<dyn FileHost + Send + Sync>>,
    payload: web::Payload,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    v2_reroute::convert_v3_no_extract(v3::users::user_icon_edit(
        web::Query(v3::users::Extension { ext: ext.ext }),
        req,
        info,
        pool,
        redis,
        file_host,
        payload,
        session_queue,
    )
    .await?)
}

#[derive(Deserialize)]
pub struct RemovalType {
    #[serde(default = "default_removal")]
    removal_type: String,
}

fn default_removal() -> String {
    "partial".into()
}

#[delete("{id}")]
pub async fn user_delete(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    removal_type: web::Query<RemovalType>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let removal_type = removal_type.into_inner();
    v2_reroute::convert_v3_no_extract(v3::users::user_delete(
        req,
        info,
        pool,
        web::Query(v3::users::RemovalType {
            removal_type: removal_type.removal_type,
        }),
        redis,
        session_queue,
    )
    .await?)
}

#[get("{id}/follows")]
pub async fn user_follows(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    v2_reroute::convert_v3_no_extract(v3::users::user_follows(req, info, pool, redis, session_queue).await?)
}

#[get("{id}/notifications")]
pub async fn user_notifications(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    v2_reroute::convert_v3_no_extract(v3::users::user_notifications(req, info, pool, redis, session_queue).await?)
}

#[get("{id}/payouts")]
pub async fn user_payouts(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    v2_reroute::convert_v3_no_extract(v3::users::user_payouts(req, info, pool, redis, session_queue).await?)
}

#[derive(Deserialize)]
pub struct FeeEstimateAmount {
    amount: Decimal,
}

#[get("{id}/payouts_fees")]
pub async fn user_payouts_fees(
    req: HttpRequest,
    info: web::Path<(String,)>,
    web::Query(amount): web::Query<FeeEstimateAmount>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
    payouts_queue: web::Data<Mutex<PayoutsQueue>>,
) -> Result<HttpResponse, ApiError> {
    v2_reroute::convert_v3_no_extract(v3::users::user_payouts_fees(
        req,
        info,
        web::Query(v3::users::FeeEstimateAmount {
            amount: amount.amount,
        }),
        pool,
        redis,
        session_queue,
        payouts_queue,
    )
    .await?)
}

#[derive(Deserialize)]
pub struct PayoutData {
    amount: Decimal,
}

#[post("{id}/payouts")]
pub async fn user_payouts_request(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    data: web::Json<PayoutData>,
    payouts_queue: web::Data<Mutex<PayoutsQueue>>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    v2_reroute::convert_v3_no_extract(v3::users::user_payouts_request(
        req,
        info,
        pool,
        web::Json(v3::users::PayoutData {
            amount: data.amount,
        }),
        payouts_queue,
        redis,
        session_queue,
    )
    .await?)
}
