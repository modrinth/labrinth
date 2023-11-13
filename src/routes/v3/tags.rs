use std::collections::HashMap;

use super::ApiError;
use crate::database::models::categories::{Category, DonationPlatform, ProjectType, ReportType};
use crate::database::models::loader_fields::{
    Loader, LoaderField, LoaderFieldEnumValue, LoaderFieldType,
};
use crate::database::redis::RedisPool;
use actix_web::{web, HttpResponse};
use serde_json::Value;
use sqlx::PgPool;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("tag")
            .route("category", web::get().to(category_list))
            .route("loader", web::get().to(loader_list)),
    )
    .route("loader_fields", web::get().to(loader_fields_list))
    .route("license", web::get().to(license_list))
    .route("license/{id}", web::get().to(license_text))
    .route("donation_platform", web::get().to(donation_platform_list))
    .route("report_type", web::get().to(report_type_list))
    .route("project_type", web::get().to(project_type_list));
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct CategoryData {
    pub icon: String,
    pub name: String,
    pub project_type: String,
    pub header: String,
}

pub async fn category_list(
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
) -> Result<HttpResponse, ApiError> {
    let results = Category::list(&**pool, &redis)
        .await?
        .into_iter()
        .map(|x| CategoryData {
            icon: x.icon,
            name: x.category,
            project_type: x.project_type,
            header: x.header,
        })
        .collect::<Vec<_>>();

    Ok(HttpResponse::Ok().json(results))
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct LoaderData {
    pub icon: String,
    pub name: String,
    pub supported_project_types: Vec<String>,
    pub supported_games: Vec<String>,
}

pub async fn loader_list(
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
) -> Result<HttpResponse, ApiError> {
    let mut results = Loader::list(&**pool, &redis)
        .await?
        .into_iter()
        .map(|x| LoaderData {
            icon: x.icon,
            name: x.loader,
            supported_project_types: x.supported_project_types,
            supported_games: x
                .supported_games
                .iter()
                .map(|x| x.name().to_string())
                .collect(),
        })
        .collect::<Vec<_>>();

    results.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    Ok(HttpResponse::Ok().json(results))
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct LoaderFieldsEnumQuery {
    pub loader_field: String,
    pub filters: Option<HashMap<String, Value>>, // For metadata
}

// Provides the variants for any enumerable loader field.
pub async fn loader_fields_list(
    pool: web::Data<PgPool>,
    query: web::Query<LoaderFieldsEnumQuery>,
    redis: web::Data<RedisPool>,
) -> Result<HttpResponse, ApiError> {
    let query = query.into_inner();
    let loader_field = LoaderField::get_field(&query.loader_field, &**pool, &redis)
        .await?
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
        LoaderFieldEnumValue::list_filter(loader_field_enum_id, filters, &**pool, &redis).await?
    } else {
        LoaderFieldEnumValue::list(loader_field_enum_id, &**pool, &redis).await?
    };

    Ok(HttpResponse::Ok().json(results))
}

#[derive(serde::Serialize)]
pub struct License {
    short: String,
    name: String,
}

pub async fn license_list() -> HttpResponse {
    let licenses = spdx::identifiers::LICENSES;
    let mut results: Vec<License> = Vec::with_capacity(licenses.len());

    for (short, name, _) in licenses {
        results.push(License {
            short: short.to_string(),
            name: name.to_string(),
        });
    }

    HttpResponse::Ok().json(results)
}

#[derive(serde::Serialize)]
pub struct LicenseText {
    title: String,
    body: String,
}

pub async fn license_text(params: web::Path<(String,)>) -> Result<HttpResponse, ApiError> {
    let license_id = params.into_inner().0;

    if license_id == *crate::models::projects::DEFAULT_LICENSE_ID {
        return Ok(HttpResponse::Ok().json(LicenseText {
            title: "All Rights Reserved".to_string(),
            body: "All rights reserved unless explicitly stated.".to_string(),
        }));
    }

    if let Some(license) = spdx::license_id(&license_id) {
        return Ok(HttpResponse::Ok().json(LicenseText {
            title: license.full_name.to_string(),
            body: license.text().to_string(),
        }));
    }

    Err(ApiError::InvalidInput(
        "Invalid SPDX identifier specified".to_string(),
    ))
}

#[derive(serde::Serialize)]
pub struct DonationPlatformQueryData {
    short: String,
    name: String,
}

pub async fn donation_platform_list(
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
) -> Result<HttpResponse, ApiError> {
    let results: Vec<DonationPlatformQueryData> = DonationPlatform::list(&**pool, &redis)
        .await?
        .into_iter()
        .map(|x| DonationPlatformQueryData {
            short: x.short,
            name: x.name,
        })
        .collect();
    Ok(HttpResponse::Ok().json(results))
}

pub async fn report_type_list(
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
) -> Result<HttpResponse, ApiError> {
    let results = ReportType::list(&**pool, &redis).await?;
    Ok(HttpResponse::Ok().json(results))
}

pub async fn project_type_list(
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
) -> Result<HttpResponse, ApiError> {
    let results = ProjectType::list(&**pool, &redis).await?;
    Ok(HttpResponse::Ok().json(results))
}