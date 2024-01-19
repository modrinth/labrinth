use super::ApiError;
use crate::database::models::categories::{Category, LinkPlatform, ProjectType, ReportType};
use crate::database::models::loader_fields::{
    Game, Loader, LoaderField, LoaderFieldEnumValue, LoaderFieldType,
};
use crate::database::redis::RedisPool;
use crate::util::extract::{Extension, Json, Path, Query};
use axum::routing::get;
use axum::Router;
use std::collections::HashMap;

use itertools::Itertools;
use serde_json::Value;
use sqlx::PgPool;

pub fn config() -> Router {
    Router::new().nest(
        "/tag",
        Router::new()
            .route("/category", get(category_list))
            .route("/loader", get(loader_list))
            .route("/game", get(games_list))
            .route("/loader_field", get(loader_fields_list))
            .route("/license", get(license_list))
            .route("/license/:id", get(license_text))
            .route("/link_platform", get(link_platform_list))
            .route("/report_type", get(report_type_list))
            .route("/project_type", get(project_type_list)),
    )
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct GameData {
    pub slug: String,
    pub name: String,
    pub icon: Option<String>,
    pub banner: Option<String>,
}

pub async fn games_list(
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
) -> Result<Json<Vec<GameData>>, ApiError> {
    let results = Game::list(&pool, &redis)
        .await?
        .into_iter()
        .map(|x| GameData {
            slug: x.slug,
            name: x.name,
            icon: x.icon_url,
            banner: x.banner_url,
        })
        .collect::<Vec<_>>();

    Ok(Json(results))
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct CategoryData {
    pub icon: String,
    pub name: String,
    pub project_type: String,
    pub header: String,
}

pub async fn category_list(
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
) -> Result<Json<Vec<CategoryData>>, ApiError> {
    let results = Category::list(&pool, &redis)
        .await?
        .into_iter()
        .map(|x| CategoryData {
            icon: x.icon,
            name: x.category,
            project_type: x.project_type,
            header: x.header,
        })
        .collect::<Vec<_>>();

    Ok(Json(results))
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct LoaderData {
    pub icon: String,
    pub name: String,
    pub supported_project_types: Vec<String>,
    pub supported_games: Vec<String>,
    pub supported_fields: Vec<String>, // Available loader fields for this loader
    pub metadata: Value,
}

pub async fn loader_list(
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
) -> Result<Json<Vec<LoaderData>>, ApiError> {
    let loaders = Loader::list(&pool, &redis).await?;

    let loader_fields = LoaderField::get_fields_per_loader(
        &loaders.iter().map(|x| x.id).collect_vec(),
        &pool,
        &redis,
    )
    .await?;

    let mut results = loaders
        .into_iter()
        .map(|x| LoaderData {
            icon: x.icon,
            name: x.loader,
            supported_project_types: x.supported_project_types,
            supported_games: x.supported_games,
            supported_fields: loader_fields
                .get(&x.id)
                .map(|x| x.iter().map(|x| x.field.clone()).collect_vec())
                .unwrap_or_default(),
            metadata: x.metadata,
        })
        .collect::<Vec<_>>();

    results.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    Ok(Json(results))
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct LoaderFieldsEnumQuery {
    pub loader_field: String,
    pub filters: Option<HashMap<String, Value>>, // For metadata
}

// Provides the variants for any enumerable loader field.
pub async fn loader_fields_list(
    Extension(pool): Extension<PgPool>,
    Query(query): Query<LoaderFieldsEnumQuery>,
    Extension(redis): Extension<RedisPool>,
) -> Result<Json<Vec<LoaderFieldEnumValue>>, ApiError> {
    let loader_field = LoaderField::get_fields_all(&pool, &redis)
        .await?
        .into_iter()
        .find(|x| x.field == query.loader_field)
        .ok_or_else(|| {
            ApiError::InvalidInput(format!(
                "'{}' was not a valid loader field.",
                query.loader_field
            ))
        })?;

    let loader_field_enum_id = match loader_field.field_type {
        LoaderFieldType::Enum(enum_id) | LoaderFieldType::ArrayEnum(enum_id) => enum_id,
        _ => {
            return Err(ApiError::InvalidInput(format!(
                "'{}' is not an enumerable field, but an '{}' field.",
                query.loader_field,
                loader_field.field_type.to_str()
            )))
        }
    };

    let results: Vec<_> = if let Some(filters) = query.filters {
        LoaderFieldEnumValue::list_filter(loader_field_enum_id, filters, &pool, &redis).await?
    } else {
        LoaderFieldEnumValue::list(loader_field_enum_id, &pool, &redis).await?
    };

    Ok(Json(results))
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct License {
    pub short: String,
    pub name: String,
}

pub async fn license_list() -> Json<Vec<License>> {
    let licenses = spdx::identifiers::LICENSES;
    let mut results: Vec<License> = Vec::with_capacity(licenses.len());

    for (short, name, _) in licenses {
        results.push(License {
            short: short.to_string(),
            name: name.to_string(),
        });
    }

    Json(results)
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct LicenseText {
    pub title: String,
    pub body: String,
}

pub async fn license_text(Path(license_id): Path<String>) -> Result<Json<LicenseText>, ApiError> {
    if license_id == *crate::models::projects::DEFAULT_LICENSE_ID {
        return Ok(Json(LicenseText {
            title: "All Rights Reserved".to_string(),
            body: "All rights reserved unless explicitly stated.".to_string(),
        }));
    }

    if let Some(license) = spdx::license_id(&license_id) {
        return Ok(Json(LicenseText {
            title: license.full_name.to_string(),
            body: license.text().to_string(),
        }));
    }

    Err(ApiError::InvalidInput(
        "Invalid SPDX identifier specified".to_string(),
    ))
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct LinkPlatformQueryData {
    pub name: String,
    pub donation: bool,
}

pub async fn link_platform_list(
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
) -> Result<Json<Vec<LinkPlatformQueryData>>, ApiError> {
    let results: Vec<LinkPlatformQueryData> = LinkPlatform::list(&pool, &redis)
        .await?
        .into_iter()
        .map(|x| LinkPlatformQueryData {
            name: x.name,
            donation: x.donation,
        })
        .collect();
    Ok(Json(results))
}

pub async fn report_type_list(
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
) -> Result<Json<Vec<String>>, ApiError> {
    let results = ReportType::list(&pool, &redis).await?;
    Ok(Json(results))
}

pub async fn project_type_list(
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
) -> Result<Json<Vec<String>>, ApiError> {
    let results = ProjectType::list(&pool, &redis).await?;
    Ok(Json(results))
}
