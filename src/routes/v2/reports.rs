use crate::database::redis::RedisPool;
use crate::models::ids::ImageId;
use crate::models::reports::{ItemType, Report};
use crate::models::v2::reports::LegacyReport;
use crate::queue::session::AuthQueue;
use crate::routes::{v2_reroute, v3, ApiError};
use actix_web::{delete, get, patch, post, web, HttpRequest, HttpResponse};
use serde::Deserialize;
use sqlx::PgPool;
use validator::Validate;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(reports_get);
    cfg.service(reports);
    cfg.service(report_create);
    cfg.service(report_edit);
    cfg.service(report_delete);
    cfg.service(report_get);
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

#[post("report")]
pub async fn report_create(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    body: web::Payload,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<HttpResponse, ApiError> {
    let response = v3::reports::report_create(req, pool, body, redis, session_queue)
        .await
        .or_else(v2_reroute::flatten_404_error)?;

    // Convert response to V2 format
    match v2_reroute::extract_ok_json::<Report>(response).await {
        Ok(report) => {
            let report = LegacyReport::from(report);
            Ok(Json(report))
        }
        Err(response) => Ok(response),
    }
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

#[get("report")]
pub async fn reports(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    count: Query<ReportsRequestOptions>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<HttpResponse, ApiError> {
    let response = v3::reports::reports(
        req,
        pool,
        redis,
        Query(v3::reports::ReportsRequestOptions {
            count: count.count,
            all: count.all,
        }),
        session_queue,
    )
    .await
    .or_else(v2_reroute::flatten_404_error)?;

    // Convert response to V2 format
    match v2_reroute::extract_ok_json::<Vec<Report>>(response).await {
        Ok(reports) => {
            let reports: Vec<_> = reports.into_iter().map(LegacyReport::from).collect();
            Ok(Json(reports))
        }
        Err(response) => Ok(response),
    }
}

#[derive(Deserialize)]
pub struct ReportIds {
    pub ids: String,
}

#[get("reports")]
pub async fn reports_get(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(ids): Query<ReportIds>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<HttpResponse, ApiError> {
    let response = v3::reports::reports_get(
        req,
        Query(v3::reports::ReportIds { ids: ids.ids }),
        pool,
        redis,
        session_queue,
    )
    .await
    .or_else(v2_reroute::flatten_404_error)?;

    // Convert response to V2 format
    match v2_reroute::extract_ok_json::<Vec<Report>>(response).await {
        Ok(report_list) => {
            let report_list: Vec<_> = report_list.into_iter().map(LegacyReport::from).collect();
            Ok(Json(report_list))
        }
        Err(response) => Ok(response),
    }
}

#[get("report/{id}")]
pub async fn report_get(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    info: web::Path<(crate::models::reports::ReportId,)>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<HttpResponse, ApiError> {
    let response = v3::reports::report_get(req, pool, redis, info, session_queue)
        .await
        .or_else(v2_reroute::flatten_404_error)?;

    // Convert response to V2 format
    match v2_reroute::extract_ok_json::<Report>(response).await {
        Ok(report) => {
            let report = LegacyReport::from(report);
            Ok(Json(report))
        }
        Err(response) => Ok(response),
    }
}

#[derive(Deserialize, Validate)]
pub struct EditReport {
    #[validate(length(max = 65536))]
    pub body: Option<String>,
    pub closed: Option<bool>,
}

#[patch("report/{id}")]
pub async fn report_edit(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    info: web::Path<(crate::models::reports::ReportId,)>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
    edit_report: Json<EditReport>,
) -> Result<HttpResponse, ApiError> {
    let edit_report = edit_report.into_inner();
    // Returns NoContent, so no need to convert
    v3::reports::report_edit(
        req,
        pool,
        redis,
        info,
        session_queue,
        Json(v3::reports::EditReport {
            body: edit_report.body,
            closed: edit_report.closed,
        }),
    )
    .await
    .or_else(v2_reroute::flatten_404_error)
}

#[delete("report/{id}")]
pub async fn report_delete(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    info: web::Path<(crate::models::reports::ReportId,)>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<HttpResponse, ApiError> {
    // Returns NoContent, so no need to convert
    v3::reports::report_delete(req, pool, info, redis, session_queue)
        .await
        .or_else(v2_reroute::flatten_404_error)
}
