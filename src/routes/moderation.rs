use super::ApiError;
use crate::database;
use crate::models;
use actix_web::{get, web, HttpRequest, HttpResponse};
use sqlx::PgPool;

#[get("mods")]
pub async fn mods(
    info: web::Path<(models::ids::ModId,)>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    Ok(HttpResponse::Ok().body(""))
}

#[get("versions")]
pub async fn versions(
    info: web::Path<(models::ids::ModId,)>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    Ok(HttpResponse::Ok().body(""))
}
