use crate::{
    auth::get_user_from_headers,
    database::{self, models::DatabaseError, redis::RedisPool},
    models::pats::Scopes,
    queue::session::AuthQueue,
    routes::ApiError,
};
use actix_web::{
    delete, post,
    web::{self},
    HttpRequest, HttpResponse,
};
use sqlx::PgPool;

use database::models as db_models;
use database::models::creator_follows::UserFollow as DBUserFollow;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(web::scope("user").service(user_follow));
}

#[post("{id}/follow")]
pub async fn user_follow(
    req: HttpRequest,
    target_id: web::Path<String>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let (_, current_user) = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::USER_WRITE]),
    )
    .await?;

    let target_user = db_models::User::get(&target_id.into_inner(), &**pool, &redis)
        .await?
        .ok_or_else(|| ApiError::InvalidInput("The specified user does not exist!".to_string()))?;

    DBUserFollow {
        follower_id: current_user.id.into(),
        target_id: target_user.id,
    }
    .insert(&**pool)
    .await
    .map_err(|e| match e {
        DatabaseError::Database(e)
            if e.as_database_error()
                .is_some_and(|e| e.is_unique_violation()) =>
        {
            ApiError::InvalidInput("You are already following this user!".to_string())
        }
        e => e.into(),
    })?;

    Ok(HttpResponse::NoContent().body(""))
}

#[delete("{id}/follow")]
pub async fn user_unfollow(
    req: HttpRequest,
    target_id: web::Path<String>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let (_, current_user) = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::USER_WRITE]),
    )
    .await?;

    let target_user = db_models::User::get(&target_id.into_inner(), &**pool, &redis)
        .await?
        .ok_or_else(|| ApiError::InvalidInput("The specified user does not exist!".to_string()))?;

    DBUserFollow::unfollow(current_user.id.into(), target_user.id, &**pool).await?;

    Ok(HttpResponse::NoContent().body(""))
}
