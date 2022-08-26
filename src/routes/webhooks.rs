use super::ApiError;
use crate::database;
use crate::database::models::webhooks::Webhook;
use crate::database::models::UserId;
use crate::util::auth::get_user_from_headers;
use crate::util::validate::validation_errors_to_string;
use actix_web::{delete, post, web, HttpRequest, HttpResponse};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use validator::Validate;

#[derive(Serialize, Deserialize, Validate)]
pub struct WebhookData {
    #[validate(
        custom(function = "crate::util::validate::validate_url"),
        length(max = 255)
    )]
    pub webhook_url: String,
    #[validate(length(min = 1, max = 255))]
    pub projects: Vec<crate::models::ids::ProjectId>,
    #[validate(length(min = 1))]
    pub loaders: Vec<crate::models::projects::Loader>,
}

#[post("follow")]
pub async fn follow_project_updates_discord(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    webhook_data: web::Json<WebhookData>,
) -> Result<HttpResponse, ApiError> {
    webhook_data.validate().map_err(|err| {
        ApiError::Validation(validation_errors_to_string(err, None))
    })?;

    let user = get_user_from_headers(req.headers(), &**pool).await?;

    let mut transaction = pool.begin().await?;

    let mut loaders: Vec<database::models::LoaderId> =
        Vec::with_capacity(webhook_data.loaders.len());
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

    let result =
        Webhook::insert(&webhook, UserId::from(user.id), &**pool).await;

    transaction.commit().await?;

    if let Err(error) = result {
        Err(ApiError::SqlxDatabase(error))
    } else {
        Ok(HttpResponse::NoContent().body(""))
    }
}

#[derive(Deserialize)]
pub struct WebhookDeletionData {
    pub webhook_url: String,
}

#[delete("unfollow")]
pub async fn unfollow_project_updates_discord(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    webhook_data: web::Json<WebhookDeletionData>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(req.headers(), &**pool).await?;

    let result = Webhook::remove(
        &webhook_data.webhook_url,
        UserId::from(user.id),
        &**pool,
    )
    .await;

    if result.is_ok() {
        Ok(HttpResponse::NoContent().body(""))
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
}
