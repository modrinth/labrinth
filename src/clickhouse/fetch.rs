use std::sync::Arc;

use crate::{
    models::ids::{ProjectId, VersionId},
    routes::ApiError,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(clickhouse::Row, Serialize, Deserialize, Clone, Debug)]
pub struct ReturnPlaytimes {
    pub time: u32,
    pub id: u64,
    pub total_seconds: u64,
}

#[derive(clickhouse::Row, Serialize, Deserialize, Clone, Debug)]
pub struct ReturnCountry {
    pub country: String,
    pub id: u64,
    pub total_views: u64,
    pub total_downloads: u64,
}

#[derive(clickhouse::Row, Serialize, Deserialize, Clone, Debug)]
pub struct ReturnViews {
    pub time: u32,
    pub id: u64,
    pub total_views: u64,
}

#[derive(clickhouse::Row, Serialize, Deserialize, Clone, Debug)]
pub struct ReturnDownloads {
    pub time: u32,
    pub id: u64,
    pub total_downloads: u64,
}

// Only one of project_id or version_id should be used
// Fetches playtimes as a Vec of ReturnPlaytimes
pub async fn fetch_playtimes(
    projects: Option<Vec<ProjectId>>,
    versions: Option<Vec<VersionId>>,
    start_date: DateTime<Utc>,
    end_date: DateTime<Utc>,
    resolution_minute: u32,
    client: Arc<clickhouse::Client>,
) -> Result<Vec<ReturnPlaytimes>, ApiError> {
    let project_or_version = if projects.is_some() && versions.is_none() {
        "project_id"
    } else if versions.is_some() {
        "version_id"
    } else {
        return Err(ApiError::InvalidInput(
            "Only one of 'project_id' or 'version_id' should be used.".to_string(),
        ));
    };

    let mut query = client
        .query(&format!(
            "
        SELECT
            toUnixTimestamp(toStartOfInterval(recorded, toIntervalMinute(?))) AS time,
            {project_or_version} AS id,
            SUM(seconds) AS total_seconds
        FROM playtime
        WHERE recorded BETWEEN ? AND ?
        AND {project_or_version} IN ? 
        GROUP BY
            time,
            {project_or_version}
        "
        ))
        .bind(resolution_minute)
        .bind(start_date.timestamp())
        .bind(end_date.timestamp());

    if let Some(projects) = projects {
        query = query.bind(projects.iter().map(|x| x.0).collect::<Vec<_>>());
    } else if let Some(versions) = versions {
        query = query.bind(versions.iter().map(|x| x.0).collect::<Vec<_>>());
    }

    Ok(query.fetch_all().await?)
}

// Fetches views as a Vec of ReturnViews
pub async fn fetch_views(
    projects: Option<Vec<ProjectId>>,
    versions: Option<Vec<VersionId>>,
    start_date: DateTime<Utc>,
    end_date: DateTime<Utc>,
    resolution_minutes: u32,
    client: Arc<clickhouse::Client>,
) -> Result<Vec<ReturnViews>, ApiError> {
    let project_or_version = if projects.is_some() && versions.is_none() {
        "project_id"
    } else if versions.is_some() {
        "version_id"
    } else {
        return Err(ApiError::InvalidInput(
            "Only one of 'project_id' or 'version_id' should be used.".to_string(),
        ));
    };

    let mut query = client
        .query(&format!(
            "
            SELECT  
                toUnixTimestamp(toStartOfInterval(recorded, toIntervalMinute(?))) AS time,
                {project_or_version} AS id,
                count(views.id) AS total_views
            FROM views
            WHERE recorded BETWEEN ? AND ?
                  AND {project_or_version} IN ?
            GROUP BY
            time, {project_or_version}
            "
        ))
        .bind(resolution_minutes)
        .bind(start_date.timestamp())
        .bind(end_date.timestamp());

    if let Some(projects) = projects {
        query = query.bind(projects.iter().map(|x| x.0).collect::<Vec<_>>());
    } else if let Some(versions) = versions {
        query = query.bind(versions.iter().map(|x| x.0).collect::<Vec<_>>());
    }

    Ok(query.fetch_all().await?)
}

// Fetches downloads as a Vec of ReturnDownloads
pub async fn fetch_downloads(
    projects: Option<Vec<ProjectId>>,
    versions: Option<Vec<VersionId>>,
    start_date: DateTime<Utc>,
    end_date: DateTime<Utc>,
    resolution_minutes: u32,
    client: Arc<clickhouse::Client>,
) -> Result<Vec<ReturnDownloads>, ApiError> {
    let project_or_version = if projects.is_some() && versions.is_none() {
        "project_id"
    } else if versions.is_some() {
        "version_id"
    } else {
        return Err(ApiError::InvalidInput(
            "Only one of 'project_id' or 'version_id' should be used.".to_string(),
        ));
    };

    let mut query = client
        .query(&format!(
            "
            SELECT  
                toUnixTimestamp(toStartOfInterval(recorded, toIntervalMinute(?))) AS time,
                {project_or_version} as id,
                count(downloads.id) AS total_downloads
            FROM downloads
            WHERE recorded BETWEEN ? AND ?
                  AND {project_or_version} IN ?
            GROUP BY time, {project_or_version}
            "
        ))
        .bind(resolution_minutes)
        .bind(start_date.timestamp())
        .bind(end_date.timestamp());

    if let Some(projects) = projects {
        query = query.bind(projects.iter().map(|x| x.0).collect::<Vec<_>>());
    } else if let Some(versions) = versions {
        query = query.bind(versions.iter().map(|x| x.0).collect::<Vec<_>>());
    }

    Ok(query.fetch_all().await?)
}

// Fetches countries as a Vec of ReturnCountry
pub async fn fetch_countries(
    projects: Option<Vec<ProjectId>>,
    versions: Option<Vec<VersionId>>,
    start_date: DateTime<Utc>,
    end_date: DateTime<Utc>,
    client: Arc<clickhouse::Client>,
) -> Result<Vec<ReturnCountry>, ApiError> {
    let project_or_version = if projects.is_some() && versions.is_none() {
        "project_id"
    } else if versions.is_some() {
        "version_id"
    } else {
        return Err(ApiError::InvalidInput(
            "Only one of 'project_id' or 'version_id' should be used.".to_string(),
        ));
    };

    let mut query = client.query(&format!(
            "
            WITH view_grouping AS (
            SELECT
                country,
                {project_or_version},
                count(id) AS total_views
            FROM views
            WHERE recorded BETWEEN ? AND ?
            GROUP BY
                country,
                {project_or_version}
            ),
            download_grouping AS (
            SELECT
                country,
                {project_or_version},
                count(id) AS total_downloads
            FROM downloads
            WHERE recorded BETWEEN ? AND ?
            GROUP BY
                country,
                {project_or_version}
            )

            SELECT
                v.country,
                v.{project_or_version},
                v.total_views,
                d.total_downloads
            FROM view_grouping AS v
            LEFT JOIN download_grouping AS d ON (v.country = d.country) AND (v.{project_or_version} = d.{project_or_version})
            WHERE {project_or_version} IN ?
            "
        )).bind(start_date.timestamp()).bind(end_date.timestamp()).bind(start_date.timestamp()).bind(end_date.timestamp());

    if let Some(projects) = projects {
        query = query.bind(projects.iter().map(|x| x.0).collect::<Vec<_>>());
    } else if let Some(versions) = versions {
        query = query.bind(versions.iter().map(|x| x.0).collect::<Vec<_>>());
    }

    Ok(query.fetch_all().await?)
}
