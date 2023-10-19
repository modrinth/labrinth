pub use super::ApiError;
use crate::util::cors::default_cors;
use actix_web::{web, HttpResponse};
use serde_json::json;

pub mod projects;
pub mod project_creation;
pub mod tags;
pub mod versions;
pub mod version_creation;
pub mod version_file;


pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("v3")
            .wrap(default_cors())
            .configure(project_creation::config)
            .configure(projects::config)
            .configure(tags::config)
            .configure(version_file::config)
            .configure(versions::config),
    );
}

pub async fn hello_world() -> Result<HttpResponse, ApiError> {
    Ok(HttpResponse::Ok().json(json!({
        "hello": "world",
    })))
}
