use super::ids::UserId;
use crate::database::models::{LoaderId, ProjectId};
use sqlx::Executor;

pub struct Webhook {
    pub webhook_url: String,
    pub projects: Vec<ProjectId>,
    pub loaders: Vec<LoaderId>,
}

impl Webhook {
    pub async fn insert<'a, E>(
        &self,
        user_id: UserId,
        exec: E,
    ) -> Result<(), sqlx::error::Error>
    where
        E: Executor<'a, Database = sqlx::Postgres> + Copy,
    {
        let result = sqlx::query!(
            "
            INSERT INTO webhooks (url, user_id)
            VALUES ($1, $2) RETURNING id
            ",
            self.webhook_url,
            user_id.0
        )
        .fetch_one(exec)
        .await?;

        for loader_id in &self.loaders {
            sqlx::query!(
                "
                INSERT INTO loaders_webhooks (loader_id, webhook_id)
                VALUES ($1, $2)
                ",
                loader_id.0,
                result.id,
            )
            .execute(exec)
            .await?;
        }

        for project_id in &self.projects {
            sqlx::query!(
                "
                INSERT INTO mods_webhooks (mod_id, webhook_id)
                VALUES ($1, $2)
                ",
                project_id.0,
                result.id,
            )
            .execute(exec)
            .await?;
        }

        Ok(())
    }

    pub async fn remove<'a, E>(
        url: &str,
        user_id: UserId,
        exec: E,
    ) -> Result<Option<()>, sqlx::error::Error>
    where
        E: Executor<'a, Database = sqlx::Postgres> + Copy,
    {
        let webhook = sqlx::query!(
            "
            SELECT id FROM webhooks
            WHERE url = $1 AND user_id = $2
            ",
            url,
            user_id.0
        )
        .fetch_optional(exec)
        .await?;

        if let Some(webhook) = webhook {
            sqlx::query!(
                "
                DELETE FROM loaders_webhooks
                WHERE webhook_id = $1
                ",
                webhook.id
            )
            .execute(exec)
            .await?;

            sqlx::query!(
                "
                DELETE FROM mods_webhooks
                WHERE webhook_id = $1
                ",
                webhook.id
            )
            .execute(exec)
            .await?;

            let result = sqlx::query!(
                "
                DELETE FROM webhooks
                WHERE id = $1
                ",
                webhook.id
            )
            .execute(exec)
            .await?;

            if result.rows_affected() == 0 {
                // Nothing was deleted
                Ok(None)
            } else {
                Ok(Some(()))
            }
        } else {
            Ok(None)
        }
    }
}
