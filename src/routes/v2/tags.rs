use std::collections::HashMap;

use super::ApiError;
use crate::database::redis::RedisPool;
use crate::models::v2::projects::LegacySideType;
use crate::routes::v2_reroute::capitalize_first;
use crate::routes::v3;
use crate::routes::v3::tags::LoaderFieldsEnumQuery;
use crate::util::extract::{Extension, Json, Path, Query};
use axum::routing::get;
use axum::Router;
use chrono::{DateTime, Utc};
use itertools::Itertools;

use sqlx::PgPool;

pub fn config() -> Router {
    Router::new().nest(
        "/tag",
        Router::new()
            .route("/category", get(category_list))
            .route("/loader", get(loader_list))
            .route("/game_version", get(game_version_list))
            .route("/license", get(license_list))
            .route("/license/:id", get(license_text))
            .route("/donation_platform", get(donation_platform_list))
            .route("/report_type", get(report_type_list))
            .route("/project_type", get(project_type_list))
            .route("/side_type", get(side_type_list)),
    )
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
    let Json(categories) = v3::tags::category_list(Extension(pool), Extension(redis)).await?;

    // Convert to V2 format
    let categories = categories
        .into_iter()
        .map(|c| CategoryData {
            icon: c.icon,
            name: c.name,
            project_type: c.project_type,
            header: c.header,
        })
        .collect::<Vec<_>>();
    Ok(Json(categories))
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct LoaderData {
    pub icon: String,
    pub name: String,
    pub supported_project_types: Vec<String>,
}

pub async fn loader_list(
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
) -> Result<Json<Vec<LoaderData>>, ApiError> {
    let Json(loaders) = v3::tags::loader_list(Extension(pool), Extension(redis)).await?;

    // Convert to V2 format

    let loaders = loaders
        .into_iter()
        .filter(|l| &*l.name != "mrpack")
        .map(|l| {
            let mut supported_project_types = l.supported_project_types;
            // Add generic 'project' type to all loaders, which is the v2 representation of
            // a project type before any versions are set.
            supported_project_types.push("project".to_string());

            if ["forge", "fabric", "quilt", "neoforge"].contains(&&*l.name) {
                supported_project_types.push("modpack".to_string());
            }

            if supported_project_types.contains(&"datapack".to_string())
                || supported_project_types.contains(&"plugin".to_string())
            {
                supported_project_types.push("mod".to_string());
            }

            LoaderData {
                icon: l.icon,
                name: l.name,
                supported_project_types,
            }
        })
        .collect::<Vec<_>>();
    Ok(Json(loaders))
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

pub async fn game_version_list(
    Extension(pool): Extension<PgPool>,
    Query(query): Query<GameVersionQuery>,
    Extension(redis): Extension<RedisPool>,
) -> Result<Json<Vec<GameVersionQueryData>>, ApiError> {
    let mut filters = HashMap::new();
    if let Some(type_) = &query.type_ {
        filters.insert("type".to_string(), serde_json::json!(type_));
    }
    if let Some(major) = query.major {
        filters.insert("major".to_string(), serde_json::json!(major));
    }
    let Json(fields) = v3::tags::loader_fields_list(
        Extension(pool),
        Query(LoaderFieldsEnumQuery {
            loader_field: "game_versions".to_string(),
            filters: Some(filters),
        }),
        Extension(redis),
    )
    .await?;

    // Convert to V2 format
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
    Ok(Json(fields))
}

#[derive(serde::Serialize)]
pub struct License {
    pub short: String,
    pub name: String,
}

pub async fn license_list() -> Json<Vec<License>> {
    let Json(licenses) = v3::tags::license_list().await;

    // Convert to V2 format
    let licenses = licenses
        .into_iter()
        .map(|l| License {
            short: l.short,
            name: l.name,
        })
        .collect::<Vec<_>>();
    Json(licenses)
}

#[derive(serde::Serialize)]
pub struct LicenseText {
    pub title: String,
    pub body: String,
}

pub async fn license_text(Path(params): Path<String>) -> Result<Json<LicenseText>, ApiError> {
    let Json(license) = v3::tags::license_text(Path(params)).await?;

    // Convert to V2 format
    let license = LicenseText {
        title: license.title,
        body: license.body,
    };
    Ok(Json(license))
}

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq, Debug)]
pub struct DonationPlatformQueryData {
    // The difference between name and short is removed in v3.
    // Now, the 'id' becomes the name, and the 'name' is removed (the frontend uses the id as the name)
    // pub short: String,
    pub short: String,
    pub name: String,
}

pub async fn donation_platform_list(
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
) -> Result<Json<Vec<DonationPlatformQueryData>>, ApiError> {
    let Json(platforms) = v3::tags::link_platform_list(Extension(pool), Extension(redis)).await?;

    // Convert to V2 format
    let platforms = platforms
        .into_iter()
        .filter_map(|p| {
            if p.donation {
                Some(DonationPlatformQueryData {
                    // Short vs name is no longer a recognized difference in v3.
                    // We capitalize to recreate the old behavior, with some special handling.
                    // This may result in different behaviour for platforms added after the v3 migration.
                    name: match p.name.as_str() {
                        "bmac" => "Buy Me A Coffee".to_string(),
                        "github" => "GitHub Sponsors".to_string(),
                        "ko-fi" => "Ko-fi".to_string(),
                        "paypal" => "PayPal".to_string(),
                        // Otherwise, capitalize it
                        _ => capitalize_first(&p.name),
                    },
                    short: p.name,
                })
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    Ok(Json(platforms))
}

pub async fn report_type_list(
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
) -> Result<Json<Vec<String>>, ApiError> {
    // This returns a list of strings directly, so we don't need to convert to v2 format.
    Ok(v3::tags::report_type_list(Extension(pool), Extension(redis)).await?)
}

pub async fn project_type_list(
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
) -> Result<Json<Vec<String>>, ApiError> {
    // This returns a list of strings directly, so we don't need to convert to v2 format.
    Ok(v3::tags::project_type_list(Extension(pool), Extension(redis)).await?)
}

pub async fn side_type_list() -> Result<Json<Vec<String>>, ApiError> {
    // Original side types are no longer reflected in the database.
    // Therefore, we hardcode and return all the fields that are supported by our v2 conversion logic.
    let side_types = [
        LegacySideType::Required,
        LegacySideType::Optional,
        LegacySideType::Unsupported,
        LegacySideType::Unknown,
    ];
    let side_types = side_types.iter().map(|s| s.to_string()).collect_vec();
    Ok(Json(side_types))
}
