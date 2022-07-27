use actix_web::{delete, post, web, HttpResponse};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use validator::{validate_url, HasLen, Validate};

use crate::database;
use crate::database::models::{webhooks::Webhook, DatabaseError, LoaderId};
use crate::models::ids::ProjectId;
use crate::models::projects::Loader;

use super::ApiError;

#[derive(Serialize, Deserialize, Validate)]
pub struct WebhookData {
    #[validate(url, length(max = 255))]
    pub webhook_url: String,
    #[validate(length(min = 1, max = 255))]
    pub projects: Vec<ProjectId>,
    #[validate(length(min = 1))]
    pub loaders: Vec<Loader>,
}

#[post("follow")]
pub async fn follow_project_updates_discord(
    pool: web::Data<PgPool>,
    webhook_data: web::Json<WebhookData>,
) -> Result<HttpResponse, ApiError> {
    if webhook_data.projects.is_empty() || webhook_data.projects.length() > 255
    {
        return Err(ApiError::InvalidInput(
            "You can only follow between 1 and 255 projects".to_string(),
        ));
    }
    if webhook_data.loaders.is_empty() {
        return Err(ApiError::InvalidInput(
            "You must follow at least one loader".to_string(),
        ));
    }
    if !validate_url(&webhook_data.webhook_url) {
        return Err(ApiError::InvalidInput(
            "Invalid webhook URL provided".to_string(),
        ));
    }

    let mut transaction = pool.begin().await.map_err(DatabaseError::from)?;

    let mut loaders: Vec<LoaderId> =
        Vec::with_capacity(webhook_data.loaders.length() as usize);
    for loader in &webhook_data.loaders {
        let loader_id = database::models::categories::Loader::get_id(
            &loader.0,
            &mut *transaction,
        )
        .await?
        .ok_or_else(|| {
            ApiError::InvalidInput(
                "No database entry for loader provided.".to_string(),
            )
        })?;
        loaders.push(loader_id);
    }

    let webhook = Webhook {
        webhook_url: webhook_data.webhook_url.clone(),
        projects: webhook_data
            .projects
            .clone()
            .into_iter()
            .map(|x| database::models::ids::ProjectId::from(x))
            .collect(),
        loaders,
    };

    let result = Webhook::insert(&webhook, &**pool).await;

    transaction.commit().await.map_err(DatabaseError::from)?;

    if let Ok(_result) = result {
        Ok(HttpResponse::NoContent().body(""))
    } else {
        Err(ApiError::InvalidInput("".to_string()))
    }
}

#[derive(Deserialize)]
pub struct WebhookDeletionData {
    pub webhook_url: String,
}

#[delete("unfollow")]
pub async fn unfollow_project_updates_discord(
    pool: web::Data<PgPool>,
    webhook_data: web::Json<WebhookDeletionData>,
) -> Result<HttpResponse, ApiError> {
    let result = Webhook::remove(&webhook_data.webhook_url, &**pool).await?;

    if let Some(_result) = result {
        Ok(HttpResponse::NoContent().body(""))
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
}
