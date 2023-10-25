use super::ApiError;
use crate::{
    auth::get_user_from_headers,
    database::{models::DatabaseError, redis::RedisPool},
    models::pats::Scopes,
    queue::session::AuthQueue,
};
use actix_web::{web, HttpRequest, HttpResponse};
use sqlx::PgPool;

use crate::database::models as db_models;

pub trait HasId<T> {
    fn id(&self) -> T;
}

impl HasId<db_models::UserId> for db_models::User {
    fn id(&self) -> db_models::UserId {
        self.id
    }
}

impl HasId<db_models::OrganizationId> for db_models::Organization {
    fn id(&self) -> db_models::OrganizationId {
        self.id
    }
}

pub async fn follow<T, TId, Fut1, Fut2>(
    req: HttpRequest,
    target_id: web::Path<String>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
    get_db_model: impl FnOnce(String, web::Data<PgPool>, web::Data<RedisPool>) -> Fut1,
    insert_follow: impl FnOnce(db_models::UserId, TId, web::Data<PgPool>) -> Fut2,
    error_word: &str,
) -> Result<HttpResponse, ApiError>
where
    Fut1: futures::Future<Output = Result<Option<T>, DatabaseError>>,
    Fut2: futures::Future<Output = Result<(), DatabaseError>>,
    T: HasId<TId>,
{
    let (_, current_user) = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::USER_WRITE]),
    )
    .await?;

    let target = get_db_model(target_id.into_inner(), pool.clone(), redis.clone())
        .await?
        .ok_or_else(|| {
            ApiError::InvalidInput(format!("The specified {} does not exist!", error_word))
        })?;

    insert_follow(current_user.id.into(), target.id(), pool.clone())
        .await
        .map_err(|e| match e {
            DatabaseError::Database(e)
                if e.as_database_error()
                    .is_some_and(|e| e.is_unique_violation()) =>
            {
                ApiError::InvalidInput(format!("You are already following this {}!", error_word))
            }
            e => e.into(),
        })?;

    Ok(HttpResponse::NoContent().body(""))
}

pub async fn unfollow<T, TId, Fut1, Fut2>(
    req: HttpRequest,
    target_id: web::Path<String>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
    get_db_model: impl FnOnce(String, web::Data<PgPool>, web::Data<RedisPool>) -> Fut1,
    unfollow: impl FnOnce(db_models::UserId, TId, web::Data<PgPool>) -> Fut2,
    error_word: &str,
) -> Result<HttpResponse, ApiError>
where
    Fut1: futures::Future<Output = Result<Option<T>, DatabaseError>>,
    Fut2: futures::Future<Output = Result<(), DatabaseError>>,
    T: HasId<TId>,
{
    let (_, current_user) = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::USER_WRITE]),
    )
    .await?;

    let target = get_db_model(target_id.into_inner(), pool.clone(), redis.clone())
        .await?
        .ok_or_else(|| {
            ApiError::InvalidInput(format!("The specified {} does not exist!", error_word))
        })?;

    unfollow(current_user.id.into(), target.id(), pool.clone()).await?;

    Ok(HttpResponse::NoContent().body(""))
}
