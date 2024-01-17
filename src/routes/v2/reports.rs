use std::net::SocketAddr;
use std::sync::Arc;

use crate::database::redis::RedisPool;
use crate::models::ids::ImageId;
use crate::models::reports::ItemType;
use crate::models::v2::reports::LegacyReport;
use crate::queue::session::AuthQueue;
use crate::routes::{v3, ApiError};
use axum::http::{HeaderMap, StatusCode};
use crate::util::extract::{ConnectInfo, Extension, Json, Query, Path};
use axum::routing::get;
use axum::Router;
use serde::Deserialize;
use sqlx::PgPool;
use validator::Validate;

pub fn config() -> Router {
    Router::new()
        .route("/report", get(reports).post(report_create))
        .route("/reports", get(reports_get))
        .route("/report/:id", get(report_get).patch(report_edit).delete(report_delete))
}
#[derive(Deserialize, Validate)]
pub struct CreateReport {
    pub report_type: String,
    pub item_id: String,
    pub item_type: ItemType,
    pub body: String,
    // Associations to uploaded images
    #[validate(length(max = 10))]
    #[serde(default)]
    pub uploaded_images: Vec<ImageId>,
}

pub async fn report_create(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
    Json(create_report): Json<CreateReport>,
) -> Result<Json<LegacyReport>, ApiError> {
    let Json(report) = v3::reports::report_create(
        ConnectInfo(addr),
        headers,
        Extension(pool),
        Extension(redis),
        Extension(session_queue),
        Json(v3::reports::CreateReport {
            report_type: create_report.report_type,
            item_id: create_report.item_id,
            item_type: create_report.item_type,
            body: create_report.body,
            uploaded_images: create_report.uploaded_images,
        }),
    )
        .await?;

    // Convert response to V2 format
    let report = LegacyReport::from(report);
    Ok(Json(report))
}

#[derive(Deserialize)]
pub struct ReportsRequestOptions {
    #[serde(default = "default_count")]
    count: i16,
    #[serde(default = "default_all")]
    all: bool,
}

fn default_count() -> i16 {
    100
}
fn default_all() -> bool {
    true
}

pub async fn reports(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Query(count): Query<ReportsRequestOptions>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<Json<Vec<LegacyReport>>, ApiError> {
    let Json(reports) = v3::reports::reports(
        ConnectInfo(addr),
        headers,
        Extension(pool),
        Extension(redis),
        Query(v3::reports::ReportsRequestOptions {
            count: count.count,
            all: count.all,
        }),
        Extension(session_queue),
    )
    .await?;

    // Convert response to V2 format
    let reports: Vec<_> = reports.into_iter().map(LegacyReport::from).collect();
    Ok(Json(reports))
}

#[derive(Deserialize)]
pub struct ReportIds {
    pub ids: String,
}

pub async fn reports_get(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(ids): Query<ReportIds>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<Json<Vec<LegacyReport>>, ApiError> {
    let Json(reports) = v3::reports::reports_get(
        ConnectInfo(addr),
        headers,
        Query(v3::reports::ReportIds { ids: ids.ids }),
        Extension(pool),
        Extension(redis),
        Extension(session_queue),
    )
    .await?;

    // Convert response to V2 format
    let reports: Vec<_> = reports.into_iter().map(LegacyReport::from).collect();
    Ok(Json(reports))
}

pub async fn report_get(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Path(info): Path<crate::models::reports::ReportId>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<Json<LegacyReport>, ApiError> {
    let Json(report) = v3::reports::report_get(
        ConnectInfo(addr),
        headers,
        Extension(pool),
        Extension(redis),
        Path(info),
        Extension(session_queue),
    )
        .await?;

    // Convert response to V2 format
    let report = LegacyReport::from(report);
    Ok(Json(report))
}

#[derive(Deserialize, Validate)]
pub struct EditReport {
    #[validate(length(max = 65536))]
    pub body: Option<String>,
    pub closed: Option<bool>,
}

pub async fn report_edit(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Path(info) : Path<crate::models::reports::ReportId>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
    Json(edit_report): Json<EditReport>,
) -> Result<StatusCode, ApiError> {
    
   Ok( v3::reports::report_edit(
        ConnectInfo(addr),
        headers,
        Extension(pool),
        Extension(redis),
        Path(info),
        Extension(session_queue),
        Json(v3::reports::EditReport {
            body: edit_report.body,
            closed: edit_report.closed,
        })
    )
    .await?)
}

pub async fn report_delete(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Path(info): Path<crate::models::reports::ReportId>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<StatusCode, ApiError> {
    
   Ok( v3::reports::report_delete(
        ConnectInfo(addr),
        headers,
        Extension(pool),
        Path(info),
        Extension(redis),
        Extension(session_queue),
   )
        .await?)
}
