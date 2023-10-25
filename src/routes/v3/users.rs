use crate::{
    auth::get_user_from_headers,
    database::{
        self,
        models::event_item::{EventData, EventSelector, EventType},
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
use database::models::user_item::User as DBUser;

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
    crate::routes::v3::follow::follow(
        req,
        target_id,
        pool,
        redis,
        session_queue,
        |id, pool, redis| async move { DBUser::get(&id, &**pool, &redis).await },
        |follower_id, target_id, pool| async move {
            DBUserFollow {
                follower_id,
                target_id,
            }
            .insert(&**pool)
            .await
        },
        "user",
    )
    .await
}

#[delete("{id}/follow")]
pub async fn user_unfollow(
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
        |id, pool, redis| async move { DBUser::get(&id, &**pool, &redis).await },
        |follower_id, target_id, pool| async move {
            DBUserFollow::unfollow(follower_id, target_id, &**pool).await
        },
        "organization",
    )
    .await
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

    Ok(HttpResponse::Ok().json(feed_items))
}
