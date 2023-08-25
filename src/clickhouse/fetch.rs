use std::sync::Arc;

use crate::{
    models::
        ids::{ProjectId, VersionId},
    routes::ApiError,
};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

#[derive(clickhouse::Row, Serialize, Deserialize, Clone, Debug)]
pub struct ReturnPlaytimes {
    pub time: u64,
    pub project_id: u64,
    pub id: u64,
    pub total_seconds: u64,
    pub loader_seconds: Vec<(String, u64)>,
    pub game_version_seconds: Vec<(String, u64)>,
    pub parent_seconds: Vec<(u64, u64)>,
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
    pub time: u64,
    pub id: u64,
    pub total_views: u64,
}


// Only one of project_id or version_id should be used
// Fetches playtimes as a Vec of ReturnPlaytimes
pub async fn fetch_playtimes(
    projects: Option<Vec<ProjectId>>,
    versions: Option<Vec<VersionId>>,
    start_date: NaiveDate,
    end_date: NaiveDate,
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

    let mut query = client.query(&format!(
        "
        WITH
        loader_grouping AS
        (
            SELECT
                toStartOfInterval(recorded, toIntervalMinute(?)) AS time,
                project_id,
                version_id,
                loader,
                sum(seconds) AS temp_loader_seconds
            FROM playtime
            WHERE loader != ''
            GROUP BY
                time,
                project_id,
                version_id,
                loader
        ),
        game_version_grouping AS
        (
            SELECT
                toStartOfInterval(recorded, toIntervalMinute(?)) AS time,
                project_id,
                version_id,
                game_version,
                sum(seconds) AS temp_game_version_seconds
            FROM playtime
            WHERE game_version != '' 
            GROUP BY
            time,
                project_id,
                version_id,
                game_version
        ),
        parent_grouping AS
        (
            SELECT
                toStartOfInterval(recorded, toIntervalMinute(?)) AS time,
                project_id,
                version_id,
                parent,
                sum(seconds) AS temp_parent_seconds
            FROM playtime
            WHERE parent != 0
            GROUP BY
            time,
                project_id,
                version_id,
                parent
        )
        SELECT
            toYYYYMMDDhhmmss(l.time),
            l.project_id,
            l.{project_or_version},
            sum(l.temp_loader_seconds) AS total_seconds,
            array_aggDistinct((l.loader, l.temp_loader_seconds)) AS loader_seconds,
            array_aggDistinct((g.game_version, g.temp_game_version_seconds)) AS game_version_seconds,
            array_aggDistinct((p.parent, p.temp_parent_seconds)) AS parent_seconds
        FROM loader_grouping AS l
        LEFT JOIN game_version_grouping AS g ON (l.time = g.time) AND (l.{project_or_version} = g.{project_or_version})
        LEFT JOIN parent_grouping AS p   ON (l.time = p.time) AND (l.{project_or_version} = p.{project_or_version})
        WHERE l.time >= toDate(?) AND l.time <= toDate(?)
        AND l.{project_or_version} IN ? 
        GROUP BY
            l.time,
            l.project_id,
            l.{project_or_version}
        "
        )).bind(resolution_minute).bind(resolution_minute).bind(resolution_minute).bind(start_date).bind(end_date);

    if projects.is_some() {
        query = query.bind(projects.unwrap().iter().map(|x| x.0).collect::<Vec<_>>());
    } else if versions.is_some() {
        query = query.bind(versions.unwrap().iter().map(|x| x.0).collect::<Vec<_>>());
    }

    Ok(query.fetch_all().await?)
}

// Fetches views as a Vec of ReturnViews
pub async fn fetch_views(
    projects: Option<Vec<ProjectId>>,
    versions: Option<Vec<VersionId>>,
    start_date: NaiveDate,
    end_date: NaiveDate,
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
                toYYYYMMDDhhmmss((toStartOfInterval(recorded, toIntervalMinute(?)) AS time)),
                {project_or_version},
                count(id) AS total_views
            FROM views
            WHERE time >= toDate(?) AND time <= toDate(?)
            AND {project_or_version} IN ? 
            GROUP BY
            time,
        {project_or_version}
                    "
        ))
        .bind(resolution_minutes)
        .bind(start_date)
        .bind(end_date);

    if projects.is_some() {
        query = query.bind(projects.unwrap().iter().map(|x| x.0).collect::<Vec<_>>());
    } else if versions.is_some() {
        query = query.bind(versions.unwrap().iter().map(|x| x.0).collect::<Vec<_>>());
    }

    Ok(query.fetch_all().await?)
}

// Fetches countries as a Vec of ReturnCountry
pub async fn fetch_countries(
    projects: Option<Vec<ProjectId>>,
    versions: Option<Vec<VersionId>>,
    start_date: NaiveDate,
    end_date: NaiveDate,
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
            WHERE toYYYYMMDDhhmmss(recorded) >= toYYYYMMDDhhmmss(toDate(?)) AND toYYYYMMDDhhmmss(recorded) <= toYYYYMMDDhhmmss(toDate(?))
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
            WHERE toYYYYMMDDhhmmss(recorded) >= toYYYYMMDDhhmmss(toDate(?)) AND toYYYYMMDDhhmmss(recorded) <= toYYYYMMDDhhmmss(toDate(?))
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
        )).bind(start_date).bind(end_date).bind(start_date).bind(end_date);

    if projects.is_some() {
        query = query.bind(projects.unwrap().iter().map(|x| x.0).collect::<Vec<_>>());
    } else if versions.is_some() {
        query = query.bind(versions.unwrap().iter().map(|x| x.0).collect::<Vec<_>>());
    }

    Ok(query.fetch_all().await?)
}

