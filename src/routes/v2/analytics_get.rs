use std::{collections::HashMap, sync::Arc};

use actix_web::{get, web, HttpRequest, HttpResponse};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::{
    auth::{filter_authorized_projects, filter_authorized_versions, get_user_from_headers},
    database::models::{project_item, version_item},
    models::{
        ids::{
            base62_impl::{parse_base62, to_base62},
            ProjectId, VersionId,
        },
        pats::Scopes,
    },
    queue::{analytics::AnalyticsQueue, session::AuthQueue},
};

use super::ApiError;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(web::scope("analytics").service(playtimes_get));
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GetPlaytimes {
    pub project_ids: Option<Vec<String>>,
    pub version_ids: Option<Vec<String>>,
    pub start_date: Option<NaiveDate>,
    pub end_date: Option<NaiveDate>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct FetchedPlaytime {
    pub day: u32,
    pub total_seconds: u64,
    pub loader_seconds: HashMap<String, u64>,
    pub game_version_seconds: HashMap<String, u64>,
    pub parent_seconds: HashMap<VersionId, u64>,
}

/// Get playtime data for a set of projects or versions
/// Data is returned as a hashmap of project/version ids to a hashmap of days to playtime data
/// eg:
/// {
///     "4N1tEhnO": {
///         "20230824": {
///             "day": 20230824,
///             "total_seconds": 23,
///             "loader_seconds": {
///                 "bukkit": 23
///             },
///             "game_version_seconds": {
///                 "1.2.3": 23
///             },
///             "parent_seconds": {
///                 "": 0
///             }
///         }
///    }
///}
/// Either a list of project_ids or version_ids can be used, but not both. Unauthorized projects/versions will be filtered out.
/// loader_seconds, game_version_seconds, and parent_seconds are a how many of the total seconds were spent in each loader, game version, and parent version respectively.
#[get("playtime")]
pub async fn playtimes_get(
    req: HttpRequest,
    analytics_queue: web::Data<Arc<AnalyticsQueue>>,
    clickhouse: web::Data<clickhouse::Client>,
    data: web::Json<GetPlaytimes>,
    session_queue: web::Data<AuthQueue>,
    pool: web::Data<PgPool>,
    redis: web::Data<deadpool_redis::Pool>,
) -> Result<HttpResponse, ApiError> {
    let user_option = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::ANALYTICS]),
    )
    .await
    .map(|x| x.1)
    .ok();

    if data.project_ids.is_some() == data.version_ids.is_some() {
        return Err(ApiError::InvalidInput(
            "Exactly one of 'project_ids' or 'version_ids' should be used.".to_string(),
        ));
    }

    let start_date = data.start_date.unwrap_or(NaiveDate::MIN);
    let end_date = data.end_date.unwrap_or(NaiveDate::MAX);

    let mut hm = HashMap::new();

    let playtimes = if let Some(project_ids) = data.project_ids.clone() {
        // Submitted project_ids are filtered by the user's permissions
        let ids = project_ids
            .iter()
            .map(|id| Ok(ProjectId(parse_base62(id)?).into()))
            .collect::<Result<Vec<_>, ApiError>>()?;
        let projects = project_item::Project::get_many_ids(&ids, &**pool, &redis).await?;
        let ids: Vec<ProjectId> = filter_authorized_projects(projects, &user_option, &pool)
            .await?
            .into_iter()
            .map(|x| x.id)
            .collect::<Vec<_>>();

        for id in &ids {
            hm.insert(to_base62(id.0), HashMap::new());
        }
        // Get the playtimes
        analytics_queue
            .fetch_playtimes(
                Some(ids),
                None,
                start_date,
                end_date,
                clickhouse.into_inner(),
            )
            .await?
    } else if let Some(version_ids) = data.version_ids.clone() {
        // Submitted version_ids are filtered by the user's permissions
        let ids = version_ids
            .iter()
            .map(|id| Ok(VersionId(parse_base62(id)?).into()))
            .collect::<Result<Vec<_>, ApiError>>()?;
        let versions = version_item::Version::get_many(&ids, &**pool, &redis).await?;
        let ids: Vec<VersionId> = filter_authorized_versions(versions, &user_option, &pool)
            .await?
            .into_iter()
            .map(|x| x.id)
            .collect::<Vec<_>>();

        for id in &ids {
            hm.insert(to_base62(id.0), HashMap::new());
        }
        // Get the playtimes
        analytics_queue
            .fetch_playtimes(
                None,
                Some(ids),
                start_date,
                end_date,
                clickhouse.into_inner(),
            )
            .await?
    } else {
        // unreachable
        return Err(ApiError::InvalidInput(
            "Exactly one of 'project_ids' or 'version_ids' must be used.".to_string(),
        ));
    };

    for playtime in playtimes {
        let id_string = to_base62(playtime.id);
        if let Some(hm) = hm.get_mut(&id_string) {
            hm.insert(
                playtime.day.to_string(),
                FetchedPlaytime {
                    day: playtime.day,
                    total_seconds: playtime.total_seconds,
                    loader_seconds: playtime
                        .loader_seconds
                        .into_iter()
                        .collect::<HashMap<_, _>>(),
                    game_version_seconds: playtime
                        .game_version_seconds
                        .into_iter()
                        .collect::<HashMap<_, _>>(),
                    parent_seconds: playtime
                        .parent_seconds
                        .into_iter()
                        .map(|(k, v)| (VersionId(k), v))
                        .collect::<HashMap<_, _>>(),
                },
            );
        }
    }

    Ok(HttpResponse::Ok().json(hm))
}
