use crate::models::analytics::Download;
use crate::models::analytics::PageView;
use dashmap::DashSet;

pub struct AnalyticsQueue {
    views_queue: DashSet<PageView>,
    downloads_queue: DashSet<Download>,
}

// Batches analytics data points + transactions every few minutes
impl AnalyticsQueue {
    pub fn new() -> Self {
        AnalyticsQueue {
            views_queue: DashSet::with_capacity(1000),
            downloads_queue: DashSet::with_capacity(1000),
        }
    }

    pub async fn add_view(&self, page_view: PageView) {
        self.views_queue.insert(page_view);
    }

    pub async fn add_download(&self, download: Download) {
        self.downloads_queue.insert(download);
    }

    pub async fn index(&self, client: clickhouse::Client) -> Result<(), clickhouse::error::Error> {
        let views_queue = self.views_queue.clone();
        self.views_queue.clear();

        let downloads_queue = self.downloads_queue.clone();
        self.downloads_queue.clear();

        if !views_queue.is_empty() || !downloads_queue.is_empty() {
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
        }

        Ok(())
    }
}
