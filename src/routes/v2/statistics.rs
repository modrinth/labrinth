use crate::routes::{v3, ApiError, v2_reroute};
use actix_web::{get, web, HttpResponse};
use sqlx::PgPool;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(get_stats);
}

#[get("statistics")]
pub async fn get_stats(pool: web::Data<PgPool>) -> Result<HttpResponse, ApiError> {
    v3::statistics::get_stats(pool).await.or_else(v2_reroute::flatten_404_error)
}
