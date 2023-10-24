use crate::{
    auth::get_user_from_headers,
    database::{
        self,
        models::{
            event_item::{EventData, EventSelector, EventType},
            DatabaseError,
        },
        redis::RedisPool,
    },
    models::{
        feed_item::{FeedItem, FeedItemBody},
        pats::Scopes,
    },
    queue::session::AuthQueue,
    routes::ApiError,
};
use actix_web::{
    delete, get, post,
    web::{self},
    HttpRequest, HttpResponse,
};
use itertools::Itertools;
use sqlx::PgPool;

use database::models as db_models;
use database::models::creator_follows::UserFollow as DBUserFollow;
use database::models::event_item::Event as DBEvent;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("user")
            .service(user_follow)
            .service(user_unfollow),
    );
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

#[get("feed")]
pub async fn current_user_feed(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let (_, current_user) = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::NOTIFICATION_READ]),
    )
    .await?;

    let followed_users =
        DBUserFollow::get_follows_from_follower(current_user.id.into(), &**pool).await?;

    let selectors = followed_users
        .into_iter()
        .map(|follow| EventSelector {
            id: follow.target_id.into(),
            event_type: EventType::ProjectCreated,
        })
        .collect_vec();
    let events = DBEvent::get_events(&[], &selectors, &**pool).await?;

    let mut feed_items: Vec<FeedItem> = Vec::new();
    for event in events {
        let body = match event.event_data {
            EventData::ProjectCreated {
                project_id,
                creator_id,
            } => {
                let project = db_models::Project::get_id(project_id, &**pool, &redis).await?;
                project.map(|p| FeedItemBody::ProjectCreated {
                    project_id: project_id.into(),
                    creator_id: creator_id.into(),
                    project_title: p.inner.title,
                })
            }
        };

        if let Some(body) = body {
            let feed_item = FeedItem {
                id: event.id.into(),
                body,
                time: event.time,
            };

            feed_items.push(feed_item);
        }
    }

    todo!();
}
