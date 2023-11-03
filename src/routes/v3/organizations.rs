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

use database::models::creator_follows::OrganizationFollow as DBOrganizationFollow;
use database::models::organization_item::Organization as DBOrganization;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("organization")
            .service(organization_follow)
            .service(organization_unfollow),
    );
}

#[post("{id}/follow")]
pub async fn organization_follow(
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

    let target = DBOrganization::get(&target_id, &**pool, &redis)
        .await?
        .ok_or_else(|| {
            ApiError::InvalidInput("The specified organization does not exist!".to_string())
        })?;

    DBOrganizationFollow {
        follower_id: current_user.id.into(),
        target_id: target.id,
    }
    .insert(&**pool)
    .await
    .map_err(|e| match e {
        DatabaseError::Database(e) => {
            if let Some(db_err) = e.as_database_error() {
                if db_err.is_unique_violation() {
                    return ApiError::InvalidInput(
                        "You are already following this organization!".to_string(),
                    );
                }
            }
            e.into()
        }
        e => e.into(),
    })?;

    Ok(HttpResponse::NoContent().body(""))
}

#[delete("{id}/follow")]
pub async fn organization_unfollow(
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

    let target = DBOrganization::get(&target_id, &**pool, &redis)
        .await?
        .ok_or_else(|| {
            ApiError::InvalidInput("The specified organization does not exist!".to_string())
        })?;

    DBOrganizationFollow::unfollow(current_user.id.into(), target.id, &**pool).await?;

    Ok(HttpResponse::NoContent().body(""))
}
