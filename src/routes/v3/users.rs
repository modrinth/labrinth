use std::{collections::HashMap, iter::FromIterator};

use crate::{
    auth::{filter_authorized_projects, get_user_from_headers},
    database::{
        self,
        models::event_item::{EventData, EventSelector, EventType},
        redis::RedisPool,
    },
    models::{
        feed_item::{FeedItem, FeedItemBody},
        ids::ProjectId,
        pats::Scopes,
        projects::Project,
        users::User,
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
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use database::models as db_models;
use database::models::creator_follows::OrganizationFollow as DBOrganizationFollow;
use database::models::creator_follows::UserFollow as DBUserFollow;
use database::models::event_item::Event as DBEvent;
use database::models::user_item::User as DBUser;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("user")
            .service(user_follow)
            .service(user_unfollow)
            .service(current_user_feed),
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

#[derive(Serialize, Deserialize)]
pub struct FeedParameters {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[get("feed")]
pub async fn current_user_feed(
    req: HttpRequest,
    web::Query(params): web::Query<FeedParameters>,
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
        DBUserFollow::get_follows_by_follower(current_user.id.into(), &**pool).await?;
    let followed_organizations =
        DBOrganizationFollow::get_follows_by_follower(current_user.id.into(), &**pool).await?;

    let selectors = followed_users
        .into_iter()
        .map(|follow| EventSelector {
            id: follow.target_id.into(),
            event_type: EventType::ProjectCreated,
        })
        .chain(
            followed_organizations
                .into_iter()
                .map(|follow| EventSelector {
                    id: follow.target_id.into(),
                    event_type: EventType::ProjectCreated,
                }),
        )
        .collect_vec();
    let events = DBEvent::get_events(&[], &selectors, &**pool)
        .await?
        .into_iter()
        .skip(params.offset.unwrap_or(0))
        .take(params.offset.unwrap_or(usize::MAX))
        .collect_vec();

    let mut feed_items: Vec<FeedItem> = Vec::new();
    let authorized_projects =
        prefetch_authorized_event_projects(&events, &pool, &redis, &current_user).await?;
    for event in events {
        let body =
            match event.event_data {
                EventData::ProjectCreated {
                    project_id,
                    creator_id,
                } => authorized_projects.get(&project_id.into()).map(|p| {
                    FeedItemBody::ProjectCreated {
                        project_id: project_id.into(),
                        creator_id: creator_id.into(),
                        project_title: p.title.clone(),
                    }
                }),
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

async fn prefetch_authorized_event_projects(
    events: &[db_models::Event],
    pool: &web::Data<PgPool>,
    redis: &RedisPool,
    current_user: &User,
) -> Result<HashMap<ProjectId, Project>, ApiError> {
    let project_ids = events
        .iter()
        .filter_map(|e| match &e.event_data {
            EventData::ProjectCreated {
                project_id,
                creator_id: _,
            } => Some(project_id.clone()),
        })
        .collect_vec();
    let projects = db_models::Project::get_many_ids(&project_ids, &***pool, &redis).await?;
    let authorized_projects =
        filter_authorized_projects(projects, Some(&current_user), &pool).await?;
    Ok(HashMap::<ProjectId, Project>::from_iter(
        authorized_projects.into_iter().map(|p| (p.id, p)),
    ))
}
