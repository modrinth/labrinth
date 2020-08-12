mod index;
mod mod_creation;
pub mod mods;
mod not_found;
mod tags;
mod version_creation;
pub mod versions;

pub use tags::config as tags_config;

pub use self::index::index_get;
pub use self::mod_creation::mod_create;
pub use self::mods::mod_search;
pub use self::not_found::not_found;
pub use self::version_creation::upload_file_to_version;
pub use self::version_creation::version_create;

#[derive(thiserror::Error, Debug)]
pub enum ApiError {
    #[error("Internal server error")]
    DatabaseError(#[from] crate::database::models::DatabaseError),
}

impl actix_web::ResponseError for ApiError {
    fn status_code(&self) -> actix_web::http::StatusCode {
        match self {
            ApiError::DatabaseError(..) => actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> actix_web::web::HttpResponse {
        actix_web::web::HttpResponse::build(self.status_code()).json(
            crate::models::error::ApiError {
                error: match self {
                    ApiError::DatabaseError(..) => "database_error",
                },
                description: &self.to_string(),
            },
        )
    }
}
