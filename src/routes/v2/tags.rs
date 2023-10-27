use std::collections::HashMap;

use super::ApiError;
use crate::database::models::categories::{Category, DonationPlatform, ProjectType, ReportType};
use crate::database::models::loader_fields::{Game, LoaderFieldEnumValue};
use crate::database::redis::RedisPool;
use crate::routes::v3::tags::{LoaderFieldsEnumQuery, LoaderList};
use crate::routes::{v2_reroute, v3};
use actix_web::{get, web, HttpResponse};
use chrono::{DateTime, Utc};
use sqlx::PgPool;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("tag")
            .service(category_list)
            .service(loader_list)
            .service(game_version_list)
            .service(license_list)
            .service(license_text)
            .service(donation_platform_list)
            .service(report_type_list)
            .service(project_type_list)
            .service(side_type_list),
    );
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct CategoryData {
    pub icon: String,
    pub name: String,
    pub project_type: String,
    pub header: String,
}

#[get("category")]
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
    icon: String,
    name: String,
    supported_project_types: Vec<String>,
}

#[get("loader")]
pub async fn loader_list(
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
) -> Result<HttpResponse, ApiError> {
    let response = v3::tags::loader_list(
        web::Query(LoaderList {
            game: Game::MinecraftJava.name().to_string(),
        }),
        pool,
        redis,
    )
    .await?;
    Ok(response)
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct GameVersionQueryData {
    pub version: String,
    pub version_type: String,
    pub date: DateTime<Utc>,
    pub major: bool,
}

#[derive(serde::Deserialize)]
pub struct GameVersionQuery {
    #[serde(rename = "type")]
    type_: Option<String>,
    major: Option<bool>,
}

#[get("game_version")]
pub async fn game_version_list(
    pool: web::Data<PgPool>,
    query: web::Query<GameVersionQuery>,
    redis: web::Data<RedisPool>,
) -> Result<HttpResponse, ApiError> {
    let mut filters = HashMap::new();
    if let Some(type_) = &query.type_ {
        filters.insert("type".to_string(), serde_json::json!(type_));
    }
    if let Some(major) = query.major {
        filters.insert("major".to_string(), serde_json::json!(major));
    }
    let response = v3::tags::loader_fields_list(
        pool,
        web::Query(LoaderFieldsEnumQuery {
            loader_field: "game_versions".to_string(),
            filters: Some(filters),
        }),
        redis,
    )
    .await?;

    // Convert to V2 format
    Ok(
        match v2_reroute::extract_ok_json::<Vec<LoaderFieldEnumValue>>(response).await {
            Ok(fields) => {
                let fields = fields
                    .into_iter()
                    .map(|f| GameVersionQueryData {
                        version: f.value,
                        version_type: f
                            .metadata
                            .get("type")
                            .and_then(|m| m.as_str())
                            .unwrap_or_default()
                            .to_string(),
                        date: f.created,
                        major: f
                            .metadata
                            .get("major")
                            .and_then(|m| m.as_bool())
                            .unwrap_or_default(),
                    })
                    .collect::<Vec<_>>();
                HttpResponse::Ok().json(fields)
            }
            Err(response) => response,
        },
    )
}

#[derive(serde::Serialize)]
pub struct License {
    short: String,
    name: String,
}

#[get("license")]
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

#[get("license/{id}")]
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

#[get("donation_platform")]
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

#[get("report_type")]
pub async fn report_type_list(
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
) -> Result<HttpResponse, ApiError> {
    let results = ReportType::list(&**pool, &redis).await?;
    Ok(HttpResponse::Ok().json(results))
}

#[get("project_type")]
pub async fn project_type_list(
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
) -> Result<HttpResponse, ApiError> {
    let results = ProjectType::list(&**pool, &redis).await?;
    Ok(HttpResponse::Ok().json(results))
}

#[get("side_type")]
pub async fn side_type_list(
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
) -> Result<HttpResponse, ApiError> {
    let response = v3::tags::loader_fields_list(
        pool,
        web::Query(LoaderFieldsEnumQuery {
            loader_field: "client_side".to_string(), // same as server_side
            filters: None,
        }),
        redis,
    )
    .await?;

    // Convert to V2 format
    Ok(
        match v2_reroute::extract_ok_json::<Vec<LoaderFieldEnumValue>>(response).await {
            Ok(fields) => {
                let fields = fields.into_iter().map(|f| f.value).collect::<Vec<_>>();
                HttpResponse::Ok().json(fields)
            }
            Err(response) => response,
        },
    )
}
