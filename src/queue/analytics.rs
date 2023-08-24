use std::sync::Arc;

use crate::{
    models::{
        analytics::{Download, PageView, Playtime},
        ids::{ProjectId, VersionId},
    },
    routes::ApiError,
};
use chrono::NaiveDate;
use dashmap::DashSet;
use serde::{Deserialize, Serialize};

pub struct AnalyticsQueue {
    views_queue: DashSet<PageView>,
    downloads_queue: DashSet<Download>,
    playtime_queue: DashSet<Playtime>,
}

// Batches analytics data points + transactions every few minutes
impl AnalyticsQueue {
    pub fn new() -> Self {
        AnalyticsQueue {
            views_queue: DashSet::with_capacity(1000),
            downloads_queue: DashSet::with_capacity(1000),
            playtime_queue: DashSet::with_capacity(1000),
        }
    }

    pub async fn add_view(&self, page_view: PageView) {
        self.views_queue.insert(page_view);
    }

    pub async fn add_download(&self, download: Download) {
        self.downloads_queue.insert(download);
    }

    pub async fn add_playtime(&self, playtime: Playtime) {
        self.playtime_queue.insert(playtime);
    }

    pub async fn index(&self, client: clickhouse::Client) -> Result<(), clickhouse::error::Error> {
        let views_queue = self.views_queue.clone();
        self.views_queue.clear();

        let downloads_queue = self.downloads_queue.clone();
        self.downloads_queue.clear();

        let playtime_queue = self.playtime_queue.clone();
        self.playtime_queue.clear();

        if !views_queue.is_empty() || !downloads_queue.is_empty() || !playtime_queue.is_empty() {
            let mut views = client.insert("views")?;

            for view in views_queue {
                views.write(&view).await?;
            }

            views.end().await?;

            let mut downloads = client.insert("downloads")?;

            for download in downloads_queue {
                downloads.write(&download).await?;
            }

            downloads.end().await?;

            let mut playtimes = client.insert("playtime")?;

            for playtime in playtime_queue {
                playtimes.write(&playtime).await?;
            }

            playtimes.end().await?;
        }

        Ok(())
    }

    // Only one of project_id or version_id should be used
    pub async fn fetch_playtimes(
        &self,
        projects: Option<Vec<ProjectId>>,
        versions: Option<Vec<VersionId>>,
        start_date: NaiveDate,
        end_date: NaiveDate,
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
                    toYYYYMMDD(recorded) AS day,
                    project_id,
                    version_id,
                    loader,
                    sum(seconds) AS temp_loader_seconds
                FROM playtime
                WHERE loader != ''
                GROUP BY
                    day,
                    project_id,
                    version_id,
                    loader
            ),
            game_version_grouping AS
            (
                SELECT
                    toYYYYMMDD(recorded) AS day,
                    project_id,
                    version_id,
                    game_version,
                    sum(seconds) AS temp_game_version_seconds
                FROM playtime
                WHERE game_version != '' 
                GROUP BY
                    day,
                    project_id,
                    version_id,
                    game_version
            ),
            parent_grouping AS
            (
                SELECT
                    toYYYYMMDD(recorded) AS day,
                    project_id,
                    version_id,
                    parent,
                    sum(seconds) AS temp_parent_seconds
                FROM playtime
                WHERE parent != 0
                GROUP BY
                    day,
                    project_id,
                    version_id,
                    parent
            )
        SELECT
            l.day,
            l.project_id,
            l.{project_or_version},
            sum(l.temp_loader_seconds) AS total_seconds,
            array_aggDistinct((l.loader, l.temp_loader_seconds)) AS loader_seconds,
            array_aggDistinct((g.game_version, g.temp_game_version_seconds)) AS game_version_seconds,
            array_aggDistinct((p.parent, p.temp_parent_seconds)) AS parent_seconds
        FROM loader_grouping AS l
        LEFT JOIN game_version_grouping AS g ON (l.day = g.day) AND (l.{project_or_version} = g.{project_or_version})
        LEFT JOIN parent_grouping AS p   ON (l.day = p.day) AND (l.{project_or_version} = p.{project_or_version})
        WHERE l.day >= toYYYYMMDD(toDate(?)) AND l.day <= toYYYYMMDD(toDate(?))
        AND l.{project_or_version} IN ? 
        GROUP BY
            l.day,
            l.project_id,
            l.{project_or_version}
                        "
            )).bind(start_date).bind(end_date);

        if projects.is_some() {
            query = query.bind(projects.unwrap().iter().map(|x| x.0).collect::<Vec<_>>());
        } else if versions.is_some() {
            query = query.bind(versions.unwrap().iter().map(|x| x.0).collect::<Vec<_>>());
        }

        Ok(query.fetch_all().await?)
    }
}

#[derive(clickhouse::Row, Serialize, Deserialize, Clone, Debug)]
pub struct ReturnPlaytimes {
    pub day: u32,
    pub project_id: u64,
    pub id: u64,
    pub total_seconds: u64,
    pub loader_seconds: Vec<(String, u64)>,
    pub game_version_seconds: Vec<(String, u64)>,
    pub parent_seconds: Vec<(u64, u64)>,
}
