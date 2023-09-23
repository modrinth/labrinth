use actix_web::{get, web, HttpRequest, HttpResponse};
use chrono::{Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::collections::HashMap;

use crate::{
    auth::{filter_authorized_projects, filter_authorized_versions, get_user_from_headers},
    database::models::{project_item, user_item, version_item},
    models::{
        ids::{
            base62_impl::{parse_base62, to_base62},
            ProjectId, VersionId,
        },
        pats::Scopes,
    },
    queue::session::AuthQueue,
};

use super::ApiError;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("analytics")
            .service(playtimes_get)
            .service(views_get)
            .service(downloads_get)
            .service(countries_downloads_get)
            .service(countries_views_get),
    );
}

/// The json data to be passed to fetch analytic data
/// Either a list of project_ids or version_ids can be used, but not both. Unauthorized projects/versions will be filtered out.
/// start_date and end_date are optional, and default to two weeks ago, and the maximum date respectively.
/// resolution_minutes is optional. This refers to the window by which we are looking (every day, every minute, etc) and defaults to 1440 (1 day)
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GetData {
    // only one of project_ids or version_ids should be used
    // if neither are provided, all projects the user has access to will be used
    pub project_ids: Option<Vec<String>>,
    pub version_ids: Option<Vec<String>>,

    pub start_date: Option<NaiveDate>, // defaults to 2 weeks ago
    pub end_date: Option<NaiveDate>,   // defaults to now

    pub resolution_minutes: Option<u32>, // defaults to 1 day. Ignored in routes that do not aggregate over a resolution (eg: /countries)
}

/// Get playtime data for a set of projects or versions
/// Data is returned as a hashmap of project/version ids to a hashmap of days to playtime data
/// eg:
/// {
///     "4N1tEhnO": {
///         "20230824": 23
///    }
///}
/// Either a list of project_ids or version_ids can be used, but not both. Unauthorized projects/versions will be filtered out.
#[derive(Serialize, Deserialize, Clone)]
pub struct FetchedPlaytime {
    pub time: u64,
    pub total_seconds: u64,
    pub loader_seconds: HashMap<String, u64>,
    pub game_version_seconds: HashMap<String, u64>,
    pub parent_seconds: HashMap<VersionId, u64>,
}
#[get("playtime")]
pub async fn playtimes_get(
    req: HttpRequest,
    clickhouse: web::Data<clickhouse::Client>,
    data: web::Query<GetData>,
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

    let project_ids = data.project_ids.clone();
    let version_ids = data.version_ids.clone();

    if project_ids.is_some() && version_ids.is_some() {
        return Err(ApiError::InvalidInput(
            "Only one of 'project_ids' or 'version_ids' should be used.".to_string(),
        ));
    }

    let start_date = data
        .start_date
        .unwrap_or(Utc::now().naive_utc().date() - Duration::weeks(2));
    let end_date = data.end_date.unwrap_or(Utc::now().naive_utc().date());
    let resolution_minutes = data.resolution_minutes.unwrap_or(60 * 24);

    // Convert String list to list of ProjectIds or VersionIds
    // - Filter out unauthorized projects/versions
    // - If no project_ids or version_ids are provided, we default to all projects the user has access to
    let (project_ids, version_ids) =
        filter_allowed_ids(project_ids, version_ids, user_option, &pool, &redis).await?;

    // Get the views
    let playtimes = crate::clickhouse::fetch_playtimes(
        project_ids,
        version_ids,
        start_date,
        end_date,
        resolution_minutes,
        clickhouse.into_inner(),
    )
    .await?;

    let mut hm = HashMap::new();
    for playtime in playtimes {
        let id_string = to_base62(playtime.id);
        if !hm.contains_key(&id_string) {
            hm.insert(id_string.clone(), HashMap::new());
        }
        if let Some(hm) = hm.get_mut(&id_string) {
            hm.insert(playtime.time.to_string(), playtime.total_seconds);
        }
    }

    Ok(HttpResponse::Ok().json(hm))
}

/// Get view data for a set of projects or versions
/// Data is returned as a hashmap of project/version ids to a hashmap of days to views
/// eg:
/// {
///     "4N1tEhnO": {
///         "20230824": 1090
///    }
///}
/// Either a list of project_ids or version_ids can be used, but not both. Unauthorized projects/versions will be filtered out.
#[get("views")]
pub async fn views_get(
    req: HttpRequest,
    clickhouse: web::Data<clickhouse::Client>,
    data: web::Query<GetData>,
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

    let project_ids = data.project_ids.clone();
    let version_ids = data.version_ids.clone();

    if project_ids.is_some() && version_ids.is_some() {
        return Err(ApiError::InvalidInput(
            "Only one of 'project_ids' or 'version_ids' should be used.".to_string(),
        ));
    }

    let start_date = data
        .start_date
        .unwrap_or(Utc::now().naive_utc().date() - Duration::weeks(2));
    let end_date = data.end_date.unwrap_or(Utc::now().naive_utc().date());
    let resolution_minutes = data.resolution_minutes.unwrap_or(60 * 24);

    // Convert String list to list of ProjectIds or VersionIds
    // - Filter out unauthorized projects/versions
    // - If no project_ids or version_ids are provided, we default to all projects the user has access to
    let (project_ids, version_ids) =
        filter_allowed_ids(project_ids, version_ids, user_option, &pool, &redis).await?;

    // Get the views
    let views = crate::clickhouse::fetch_views(
        project_ids,
        version_ids,
        start_date,
        end_date,
        resolution_minutes,
        clickhouse.into_inner(),
    )
    .await?;

    let mut hm = HashMap::new();
    for views in views {
        let id_string = to_base62(views.id);
        if !hm.contains_key(&id_string) {
            hm.insert(id_string.clone(), HashMap::new());
        }
        if let Some(hm) = hm.get_mut(&id_string) {
            hm.insert(views.time.to_string(), views.total_views);
        }
    }

    Ok(HttpResponse::Ok().json(hm))
}

/// Get download data for a set of projects or versions
/// Data is returned as a hashmap of project/version ids to a hashmap of days to downloads
/// eg:
/// {
///     "4N1tEhnO": {
///         "20230824": 32
///    }
///}
/// Either a list of project_ids or version_ids can be used, but not both. Unauthorized projects/versions will be filtered out.
#[get("downloads")]
pub async fn downloads_get(
    req: HttpRequest,
    clickhouse: web::Data<clickhouse::Client>,
    data: web::Query<GetData>,
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

    let project_ids = data.project_ids.clone();
    let version_ids = data.version_ids.clone();

    if project_ids.is_some() && version_ids.is_some() {
        return Err(ApiError::InvalidInput(
            "Only one of 'project_ids' or 'version_ids' should be used.".to_string(),
        ));
    }

    let start_date = data
        .start_date
        .unwrap_or(Utc::now().naive_utc().date() - Duration::weeks(2));
    let end_date = data.end_date.unwrap_or(Utc::now().naive_utc().date());
    let resolution_minutes = data.resolution_minutes.unwrap_or(60 * 24);

    // Convert String list to list of ProjectIds or VersionIds
    // - Filter out unauthorized projects/versions
    // - If no project_ids or version_ids are provided, we default to all projects the user has access to
    let (project_ids, version_ids) =
        filter_allowed_ids(project_ids, version_ids, user_option, &pool, &redis).await?;

    // Get the downloads
    let downloads = crate::clickhouse::fetch_downloads(
        project_ids,
        version_ids,
        start_date,
        end_date,
        resolution_minutes,
        clickhouse.into_inner(),
    )
    .await?;

    let mut hm = HashMap::new();
    for downloads in downloads {
        let id_string = to_base62(downloads.id);
        if !hm.contains_key(&id_string) {
            hm.insert(id_string.clone(), HashMap::new());
        }
        if let Some(hm) = hm.get_mut(&id_string) {
            hm.insert(downloads.time.to_string(), downloads.total_downloads);
        }
    }

    Ok(HttpResponse::Ok().json(hm))
}

/// Get country data for a set of projects or versions
/// Data is returned as a hashmap of project/version ids to a hashmap of coutnry to downloads.
/// Unknown countries are labeled "".
/// This is usuable to see significant performing countries per project
/// eg:
/// {
///     "4N1tEhnO": {
///         "CAN":  22
///    }
///}
/// Either a list of project_ids or version_ids can be used, but not both. Unauthorized projects/versions will be filtered out.
/// For this endpoint, provided dates are a range to aggregate over, not specific days to fetch
#[get("countries/downloads")]
pub async fn countries_downloads_get(
    req: HttpRequest,
    clickhouse: web::Data<clickhouse::Client>,
    data: web::Query<GetData>,
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

    let project_ids = data.project_ids.clone();
    let version_ids = data.version_ids.clone();

    if project_ids.is_some() && version_ids.is_some() {
        return Err(ApiError::InvalidInput(
            "Only one of 'project_ids' or 'version_ids' should be used.".to_string(),
        ));
    }

    let start_date = data
        .start_date
        .unwrap_or(Utc::now().naive_utc().date() - Duration::weeks(2));
    let end_date = data.end_date.unwrap_or(Utc::now().naive_utc().date());

    // Convert String list to list of ProjectIds or VersionIds
    // - Filter out unauthorized projects/versions
    // - If no project_ids or version_ids are provided, we default to all projects the user has access to
    let (project_ids, version_ids) =
        filter_allowed_ids(project_ids, version_ids, user_option, &pool, &redis).await?;

    // Get the countries
    let countries = crate::clickhouse::fetch_countries(
        project_ids,
        version_ids,
        start_date,
        end_date,
        clickhouse.into_inner(),
    )
    .await?;

    let mut hm = HashMap::new();
    for views in countries {
        let id_string = to_base62(views.id);
        if !hm.contains_key(&id_string) {
            hm.insert(id_string.clone(), HashMap::new());
        }
        if let Some(hm) = hm.get_mut(&id_string) {
            hm.insert(views.country, views.total_downloads);
        }
    }

    Ok(HttpResponse::Ok().json(hm))
}

/// Get country data for a set of projects or versions
/// Data is returned as a hashmap of project/version ids to a hashmap of coutnry to views.
/// Unknown countries are labeled "".
/// This is usuable to see significant performing countries per project
/// eg:
/// {
///     "4N1tEhnO": {
///         "CAN":  56165
///    }
///}
/// Either a list of project_ids or version_ids can be used, but not both. Unauthorized projects/versions will be filtered out.
/// For this endpoint, provided dates are a range to aggregate over, not specific days to fetch
#[get("countries/views")]
pub async fn countries_views_get(
    req: HttpRequest,
    clickhouse: web::Data<clickhouse::Client>,
    data: web::Query<GetData>,
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

    let project_ids = data.project_ids.clone();
    let version_ids = data.version_ids.clone();

    if project_ids.is_some() && version_ids.is_some() {
        return Err(ApiError::InvalidInput(
            "Only one of 'project_ids' or 'version_ids' should be used.".to_string(),
        ));
    }

    let start_date = data
        .start_date
        .unwrap_or(Utc::now().naive_utc().date() - Duration::weeks(2));
    let end_date = data.end_date.unwrap_or(Utc::now().naive_utc().date());

    // Convert String list to list of ProjectIds or VersionIds
    // - Filter out unauthorized projects/versions
    // - If no project_ids or version_ids are provided, we default to all projects the user has access to
    let (project_ids, version_ids) =
        filter_allowed_ids(project_ids, version_ids, user_option, &pool, &redis).await?;

    // Get the countries
    let countries = crate::clickhouse::fetch_countries(
        project_ids,
        version_ids,
        start_date,
        end_date,
        clickhouse.into_inner(),
    )
    .await?;

    let mut hm = HashMap::new();
    for views in countries {
        let id_string = to_base62(views.id);
        if !hm.contains_key(&id_string) {
            hm.insert(id_string.clone(), HashMap::new());
        }
        if let Some(hm) = hm.get_mut(&id_string) {
            hm.insert(views.country, views.total_views);
        }
    }

    Ok(HttpResponse::Ok().json(hm))
}

async fn filter_allowed_ids(
    mut project_ids: Option<Vec<String>>,
    version_ids: Option<Vec<String>>,
    user_option: Option<crate::models::users::User>,
    pool: &web::Data<PgPool>,
    redis: &deadpool_redis::Pool,
) -> Result<(Option<Vec<ProjectId>>, Option<Vec<VersionId>>), ApiError> {
    if project_ids.is_some() && version_ids.is_some() {
        return Err(ApiError::InvalidInput(
            "Only one of 'project_ids' or 'version_ids' should be used.".to_string(),
        ));
    }

    // If no project_ids or version_ids are provided, we default to all projects the user has access to
    if project_ids.is_none() && version_ids.is_none() {
        if let Some(user) = &user_option {
            project_ids = Some(
                user_item::User::get_projects(user.id.into(), &***pool)
                    .await?
                    .into_iter()
                    .map(|x| ProjectId::from(x).to_string())
                    .collect(),
            );
        }
    }

    // Convert String list to list of ProjectIds or VersionIds
    // - Filter out unauthorized projects/versions

    let project_ids = if let Some(project_ids) = project_ids {
        // Submitted project_ids are filtered by the user's permissions
        let ids = project_ids
            .iter()
            .map(|id| Ok(ProjectId(parse_base62(id)?).into()))
            .collect::<Result<Vec<_>, ApiError>>()?;
        let projects = project_item::Project::get_many_ids(&ids, &***pool, redis).await?;
        let ids: Vec<ProjectId> = filter_authorized_projects(projects, &user_option, pool)
            .await?
            .into_iter()
            .map(|x| x.id)
            .collect::<Vec<_>>();
        Some(ids)
    } else {
        None
    };
    let version_ids = if let Some(version_ids) = version_ids {
        // Submitted version_ids are filtered by the user's permissions
        let ids = version_ids
            .iter()
            .map(|id| Ok(VersionId(parse_base62(id)?).into()))
            .collect::<Result<Vec<_>, ApiError>>()?;
        let versions = version_item::Version::get_many(&ids, &***pool, redis).await?;
        let ids: Vec<VersionId> = filter_authorized_versions(versions, &user_option, pool)
            .await?
            .into_iter()
            .map(|x| x.id)
            .collect::<Vec<_>>();
        Some(ids)
    } else {
        None
    };

    // Only one of project_ids or version_ids will be Some
    Ok((project_ids, version_ids))
}
