use crate::{
    database::{self, redis::RedisPool},
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
    crate::routes::v3::follow::follow(
        req,
        target_id,
        pool,
        redis,
        session_queue,
        |id, pool, redis| async move { DBOrganization::get(&id, &**pool, &redis).await },
        |follower_id, target_id, pool| async move {
            DBOrganizationFollow {
                follower_id,
                target_id,
            }
            .insert(&**pool)
            .await
        },
        "organization",
    )
    .await
}

#[delete("{id}/follow")]
pub async fn organization_unfollow(
    req: HttpRequest,
    target_id: web::Path<String>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    crate::routes::v3::follow::unfollow(
        req,
        target_id,
        pool,
        redis,
        session_queue,
        |id, pool, redis| async move { DBOrganization::get(&id, &**pool, &redis).await },
        |follower_id, target_id, pool| async move {
            DBOrganizationFollow::unfollow(follower_id, target_id, &**pool).await
        },
        "organization",
    )
    .await
}
