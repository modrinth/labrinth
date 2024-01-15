use crate::file_hosting::FileHostingError;
use crate::routes::analytics::{page_view_ingest, playtime_ingest};
use crate::util::cors::default_cors;
use crate::util::env::parse_strings_from_var;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};
use axum::{Json, Router};
use futures::FutureExt;
use serde_json::json;
use tower_http::cors::CorsLayer;

pub mod internal;
// pub mod v2;
// pub mod v3;

pub mod v2_reroute;

mod analytics;
mod index;
// mod maven;
mod not_found;
// mod updates;

pub use self::not_found::not_found;

pub fn root_config() -> Router {
    // TODO: analytics cors
    // let analytics_cors_layer = CorsLayer::new()
    //     .allow_origin_fn(|origin, _req_head| {
    //         let allowed_origins = parse_strings_from_var("ANALYTICS_ALLOWED_ORIGINS").unwrap_or_default();
    //
    //         allowed_origins.contains(&"*".to_string())
    //             || allowed_origins.contains(&origin.to_string())
    //     })
    //     .allow_methods(vec!["GET", "POST"])
    //     .allow_headers(vec!["Authorization", "Accept", "Content-Type"])
    //     .max_age(3600);

    Router::new()
        .nest("/maven", maven::config().layer(default_cors()))
        .nest("/updates", updates::config().layer(default_cors()))
        .nest(
            "/analytics",
            Router::new()
                .route("/view", post(analytics::page_view_ingest))
                .route("/playtime", post(analytics::playtime_ingest))
                .layer(analytics_cors_layer),
        )
        .nest(
            "/api/v1/",
            Router::new()
                .route("*path", any(|| {
                    (
                        StatusCode::GONE,
                        Json( crate::models::error::ApiError {error:"api_deprecated",description:"You are using an application that uses an outdated version of Modrinth's API. Please either update it or switch to another application. For developers: https://docs.modrinth.com/docs/migrations/v1-to-v2/"})
                    )
                }).layer(default_cors()))
            ,
        )
        .nest(
            "/",
            Router::new()
                .route("/", get(index::index_get))
                .merge(axum::service::get(ServeDir::new("assets/")).handle_error(
                    |error| async move {
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            format!("Unhandled internal error: {}", error),
                        )
                    },
                ))
                .layer(default_cors()),
        )
}

#[derive(thiserror::Error, Debug)]
pub enum ApiError {
    #[error("Environment Error")]
    Env(#[from] dotenvy::Error),
    #[error("Error while uploading file: {0}")]
    FileHosting(#[from] FileHostingError),
    #[error("Database Error: {0}")]
    Database(#[from] crate::database::models::DatabaseError),
    #[error("Database Error: {0}")]
    SqlxDatabase(#[from] sqlx::Error),
    #[error("Clickhouse Error: {0}")]
    Clickhouse(#[from] clickhouse::error::Error),
    #[error("Internal server error: {0}")]
    Xml(String),
    #[error("Deserialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Authentication Error: {0}")]
    Authentication(#[from] crate::auth::AuthenticationError),
    #[error("Authentication Error: {0}")]
    CustomAuthentication(String),
    #[error("Invalid Input: {0}")]
    InvalidInput(String),
    #[error("Error while validating input: {0}")]
    Validation(String),
    #[error("Search Error: {0}")]
    Search(#[from] meilisearch_sdk::errors::Error),
    #[error("Indexing Error: {0}")]
    Indexing(#[from] crate::search::indexing::IndexingError),
    #[error("Payments Error: {0}")]
    Payments(String),
    #[error("Discord Error: {0}")]
    Discord(String),
    #[error("Captcha Error. Try resubmitting the form.")]
    Turnstile,
    #[error("Error while decoding Base62: {0}")]
    Decoding(#[from] crate::models::ids::DecodingError),
    #[error("Image Parsing Error: {0}")]
    ImageParse(#[from] image::ImageError),
    #[error("Password Hashing Error: {0}")]
    PasswordHashing(#[from] argon2::password_hash::Error),
    #[error("Password strength checking error: {0}")]
    PasswordStrengthCheck(#[from] zxcvbn::ZxcvbnError),
    #[error("{0}")]
    Mail(#[from] crate::auth::email::MailError),
    #[error("Error while rerouting request: {0}")]
    Reroute(#[from] reqwest::Error),
    #[error("Resource not found")]
    NotFound,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status_code = match &self {
            ApiError::Env(..) => StatusCode::INTERNAL_SERVER_ERROR,
            ApiError::Database(..) => StatusCode::INTERNAL_SERVER_ERROR,
            ApiError::SqlxDatabase(..) => StatusCode::INTERNAL_SERVER_ERROR,
            ApiError::Clickhouse(..) => StatusCode::INTERNAL_SERVER_ERROR,
            ApiError::Authentication(..) => StatusCode::UNAUTHORIZED,
            ApiError::CustomAuthentication(..) => StatusCode::UNAUTHORIZED,
            ApiError::Xml(..) => StatusCode::INTERNAL_SERVER_ERROR,
            ApiError::Json(..) => StatusCode::BAD_REQUEST,
            ApiError::Search(..) => StatusCode::INTERNAL_SERVER_ERROR,
            ApiError::Indexing(..) => StatusCode::INTERNAL_SERVER_ERROR,
            ApiError::FileHosting(..) => StatusCode::INTERNAL_SERVER_ERROR,
            ApiError::InvalidInput(..) => StatusCode::BAD_REQUEST,
            ApiError::Validation(..) => StatusCode::BAD_REQUEST,
            ApiError::Payments(..) => StatusCode::FAILED_DEPENDENCY,
            ApiError::Discord(..) => StatusCode::FAILED_DEPENDENCY,
            ApiError::Turnstile => StatusCode::BAD_REQUEST,
            ApiError::Decoding(..) => StatusCode::BAD_REQUEST,
            ApiError::ImageParse(..) => StatusCode::BAD_REQUEST,
            ApiError::PasswordHashing(..) => StatusCode::INTERNAL_SERVER_ERROR,
            ApiError::PasswordStrengthCheck(..) => StatusCode::BAD_REQUEST,
            ApiError::Mail(..) => StatusCode::INTERNAL_SERVER_ERROR,
            ApiError::Reroute(..) => StatusCode::INTERNAL_SERVER_ERROR,
            ApiError::NotFound => StatusCode::NOT_FOUND,
        };

        let error_message = crate::models::error::ApiError {
            error: match &self {
                ApiError::Env(..) => "environment_error",
                ApiError::SqlxDatabase(..) => "database_error",
                ApiError::Database(..) => "database_error",
                ApiError::Authentication(..) => "unauthorized",
                ApiError::CustomAuthentication(..) => "unauthorized",
                ApiError::Xml(..) => "xml_error",
                ApiError::Json(..) => "json_error",
                ApiError::Search(..) => "search_error",
                ApiError::Indexing(..) => "indexing_error",
                ApiError::FileHosting(..) => "file_hosting_error",
                ApiError::InvalidInput(..) => "invalid_input",
                ApiError::Validation(..) => "invalid_input",
                ApiError::Payments(..) => "payments_error",
                ApiError::Discord(..) => "discord_error",
                ApiError::Turnstile => "turnstile_error",
                ApiError::Decoding(..) => "decoding_error",
                ApiError::ImageParse(..) => "invalid_image",
                ApiError::PasswordHashing(..) => "password_hashing_error",
                ApiError::PasswordStrengthCheck(..) => "strength_check_error",
                ApiError::Mail(..) => "mail_error",
                ApiError::Clickhouse(..) => "clickhouse_error",
                ApiError::Reroute(..) => "reroute_error",
                ApiError::NotFound => "not_found",
            },
            description: &*self.to_string(),
        };

        (status_code, Json(error_message)).into_response()
    }
}
