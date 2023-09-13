//! Routes for Hydra
use actix_web::{web, HttpResponse};
use hyper::StatusCode;
use thiserror::Error;

use crate::{database::models::DatabaseError, models::error::ApiError};

mod callback;
mod init;
mod stages;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("hydra")
            .service(init::route)
            .service(callback::route),
    );
}

#[derive(Error, Debug)]
pub enum HydraError {
    #[error("Environment Error")]
    Env(#[from] dotenvy::Error),
    #[error("Error while communicating to external provider")]
    Reqwest(#[from] reqwest::Error),
    #[error("Failed to authorize: {0}")]
    Authorization(String),
    #[error("Error while parsing JSON")]
    Serde(#[from] serde_json::Error),
    #[error("Database error: {0}")]
    Database(#[from] DatabaseError),
}

impl actix_web::ResponseError for HydraError {
    fn status_code(&self) -> StatusCode {
        match self {
            HydraError::Env(..) => StatusCode::INTERNAL_SERVER_ERROR,
            HydraError::Reqwest(..) => StatusCode::INTERNAL_SERVER_ERROR,
            HydraError::Authorization(..) => StatusCode::BAD_REQUEST,
            HydraError::Serde(..) => StatusCode::INTERNAL_SERVER_ERROR,
            HydraError::Database(..) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status_code()).json(ApiError {
            error: self.error_name(),
            description: &self.to_string(),
        })
    }
}

impl HydraError {
    pub fn error_name(&self) -> &'static str {
        match self {
            HydraError::Env(..) => "environment_error",
            HydraError::Reqwest(..) => "internal_error",
            HydraError::Authorization(..) => "missing_token",
            HydraError::Serde(..) => "internal_error",
            HydraError::Database(..) => "database_error",
        }
    }
}
