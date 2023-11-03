use std::{collections::HashMap, iter::FromIterator};

use crate::{
    auth::{filter_authorized_projects, filter_authorized_versions, get_user_from_headers},
    database::{
        self,
        models::{
            event_item::{EventData, EventSelector, EventType},
            DatabaseError,
        },
        redis::RedisPool,
    },
    models::{
        feeds::{FeedItem, FeedItemBody},
        ids::{ProjectId, VersionId},
        pats::Scopes,
        projects::{Project, Version},
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
    let (_, current_user) = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::USER_WRITE]),
    )
    .await?;

    let target = DBUser::get(&target_id, &**pool, &redis)
        .await?
        .ok_or_else(|| ApiError::InvalidInput("The specified user does not exist!".to_string()))?;

    DBUserFollow {
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
                        "You are already following this user!".to_string(),
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

    let target = DBUser::get(&target_id, &**pool, &redis)
        .await?
        .ok_or_else(|| ApiError::InvalidInput("The specified user does not exist!".to_string()))?;

    DBUserFollow::unfollow(current_user.id.into(), target.id, &**pool).await?;

    Ok(HttpResponse::NoContent().body(""))
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

    // Feed by default shows the following:
    // - Projects created by users you follow
    // - Projects created by organizations you follow
    // - Versions created by users you follow
    // - Versions created by organizations you follow
    let event_types = [EventType::ProjectPublished, EventType::VersionCreated];
    let selectors = followed_users
        .into_iter()
        .flat_map(|follow| {
            event_types.iter().map(move |event_type| EventSelector {
                id: follow.target_id.into(),
                event_type: *event_type,
            })
        })
        .chain(followed_organizations.into_iter().flat_map(|follow| {
            event_types.iter().map(move |event_type| EventSelector {
                id: follow.target_id.into(),
                event_type: *event_type,
            })
        }))
        .collect_vec();
    let events = DBEvent::get_events(&[], &selectors, &**pool)
        .await?
        .into_iter()
        .skip(params.offset.unwrap_or(0))
        .take(params.offset.unwrap_or(usize::MAX))
        .collect_vec();

    let mut feed_items: Vec<FeedItem> = Vec::new();
    let authorized_versions =
        prefetch_authorized_event_versions(&events, &pool, &redis, &current_user).await?;
    let authorized_version_project_ids = authorized_versions
        .values()
        .map(|versions| versions.project_id)
        .collect_vec();
    let authorized_projects = prefetch_authorized_event_projects(
        &events,
        Some(&authorized_version_project_ids),
        &pool,
        &redis,
        &current_user,
    )
    .await?;

    for event in events {
        let body = match event.event_data {
            EventData::ProjectPublished {
                project_id,
                creator_id,
            } => authorized_projects.get(&project_id.into()).map(|p| {
                FeedItemBody::ProjectPublished {
                    project_id: project_id.into(),
                    creator_id: creator_id.into(),
                    project_title: p.title.clone(),
                }
            }),
            EventData::VersionCreated {
                version_id,
                creator_id,
            } => {
                let authorized_version = authorized_versions.get(&version_id.into());
                let authorized_project =
                    authorized_version.and_then(|v| authorized_projects.get(&v.project_id));
                if let (Some(authorized_version), Some(authorized_project)) =
                    (authorized_version, authorized_project)
                {
                    Some(FeedItemBody::VersionCreated {
                        project_id: authorized_project.id,
                        version_id: authorized_version.id,
                        creator_id: creator_id.into(),
                        project_title: authorized_project.title.clone(),
                    })
                } else {
                    None
                }
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

async fn prefetch_authorized_event_projects(
    events: &[db_models::Event],
    additional_ids: Option<&[ProjectId]>,
    pool: &web::Data<PgPool>,
    redis: &RedisPool,
    current_user: &User,
) -> Result<HashMap<ProjectId, Project>, ApiError> {
    let mut project_ids = events
        .iter()
        .filter_map(|e| match &e.event_data {
            EventData::ProjectPublished {
                project_id,
                creator_id: _,
            } => Some(*project_id),
            EventData::VersionCreated { .. } => None,
        })
        .collect_vec();
    if let Some(additional_ids) = additional_ids {
        project_ids.extend(
            additional_ids
                .iter()
                .copied()
                .map(db_models::ProjectId::from),
        );
    }
    let projects = db_models::Project::get_many_ids(&project_ids, &***pool, redis).await?;
    let authorized_projects =
        filter_authorized_projects(projects, Some(current_user), pool).await?;
    Ok(HashMap::<ProjectId, Project>::from_iter(
        authorized_projects.into_iter().map(|p| (p.id, p)),
    ))
}

async fn prefetch_authorized_event_versions(
    events: &[db_models::Event],
    pool: &web::Data<PgPool>,
    redis: &RedisPool,
    current_user: &User,
) -> Result<HashMap<VersionId, Version>, ApiError> {
    let version_ids = events
        .iter()
        .filter_map(|e| match &e.event_data {
            EventData::VersionCreated {
                version_id,
                creator_id: _,
            } => Some(*version_id),
            EventData::ProjectPublished { .. } => None,
        })
        .collect_vec();
    let versions = db_models::Version::get_many(&version_ids, &***pool, redis).await?;
    let authorized_versions =
        filter_authorized_versions(versions, Some(current_user), pool).await?;
    Ok(HashMap::<VersionId, Version>::from_iter(
        authorized_versions.into_iter().map(|v| (v.id, v)),
    ))
}
