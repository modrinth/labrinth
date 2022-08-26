use crate::database::models::DatabaseError;
use crate::util;
use crate::util::webhook::DiscordWebhook;
use tokio::sync::Mutex;

pub struct WebhookQueue {
    queue: Mutex<Vec<(DiscordWebhook, String)>>,
}

// Places webhook sending in a queue to not block version creation
impl WebhookQueue {
    pub fn new() -> Self {
        Self {
            queue: Mutex::new(Vec::with_capacity(1000)),
        }
    }

    pub async fn add(&self, webhook: DiscordWebhook, webhook_url: String) {
        self.queue.lock().await.push((webhook, webhook_url));
    }

    pub async fn take(&self) -> Vec<(DiscordWebhook, String)> {
        let mut queue = self.queue.lock().await;
        let len = queue.len();

        std::mem::replace(&mut queue, Vec::with_capacity(len))
    }

    pub async fn index(&self) -> Result<(), DatabaseError> {
        let queue = self.take().await;

        if !queue.is_empty() {
            for (webhook, webhook_url) in queue {
                util::webhook::send_generic_webhook(&webhook, webhook_url)
                    .await
                    .ok();
            }
        }

        Ok(())
    }
}
