use std::collections::HashMap;

use super::ApiError;
use crate::database::models::loader_fields::{Game, Loader, LoaderFieldEnum, LoaderFieldEnumValue};
use crate::database::redis::RedisPool;
use actix_web::{web, HttpResponse};
use serde_json::Value;
use sqlx::PgPool;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(web::scope("tag").route("loader", web::get().to(loader_list)))
        .route("loader_fields", web::get().to(loader_fields_list));
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct LoaderData {
    icon: String,
    name: String,
    supported_project_types: Vec<String>,
}

#[derive(serde::Deserialize)]
pub struct LoaderList {
    pub game: String,
}

pub async fn loader_list(
    data: web::Query<LoaderList>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
) -> Result<HttpResponse, ApiError> {
    let game = Game::from_name(&data.game).ok_or_else(|| {
        ApiError::InvalidInput(format!("'{}' is not a supported game.", data.game))
    })?;
    let mut results = Loader::list(game, &**pool, &redis)
        .await?
        .into_iter()
        .map(|x| LoaderData {
            icon: x.icon,
            name: x.loader,
            supported_project_types: x.supported_project_types,
        })
        .collect::<Vec<_>>();

    results.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    Ok(HttpResponse::Ok().json(results))
}

// TODO: write tests for this and all other v3/tags and v2/tags functoins
#[derive(serde::Deserialize, serde::Serialize)]
pub struct LoaderFieldsEnumQuery {
    pub game: String,
    pub field: String,
    pub filters: Option<HashMap<String, Value>>, // For metadata
}

pub async fn loader_fields_list(
    pool: web::Data<PgPool>,
    query: web::Query<LoaderFieldsEnumQuery>,
    redis: web::Data<RedisPool>,
) -> Result<HttpResponse, ApiError> {
    let query = query.into_inner();
    let loader_field_enum = LoaderFieldEnum::get(&query.field, &query.game, &**pool, &redis)
        .await?
        .ok_or_else(|| {
            ApiError::InvalidInput(format!(
                "'{}' was not a valid enumerable loader field for game {}.",
                query.field, query.game
            ))
        })?;

    let results: Vec<_> = if let Some(filters) = query.filters {
        LoaderFieldEnumValue::list_filter(loader_field_enum.id, filters, &**pool, &redis).await?
    } else {
        LoaderFieldEnumValue::list(loader_field_enum.id, &**pool, &redis).await?
    };

    Ok(HttpResponse::Ok().json(results))
}
