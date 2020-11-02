use actix_web::web;

mod auth;
mod index;
mod mod_creation;
mod mods;
mod not_found;
mod tags;
mod teams;
mod users;
mod version_creation;
mod versions;

pub use auth::config as auth_config;
pub use tags::config as tags_config;

pub use self::index::index_get;
pub use self::not_found::not_found;

pub fn mods_config(cfg: &mut web::ServiceConfig) {
    cfg.service(mods::mod_search);
    cfg.service(mods::mods_get);
    cfg.service(mod_creation::mod_create);

    cfg.service(
        web::scope("mod")
            .service(mods::mod_get)
            .service(mods::mod_delete)
            .service(web::scope("{mod_id}").service(versions::version_list)),
    );
}

pub fn versions_config(cfg: &mut web::ServiceConfig) {
    cfg.service(versions::versions_get);
    cfg.service(
        web::scope("version")
            .service(versions::version_get)
            .service(version_creation::version_create)
            .service(versions::version_delete)
            .service(version_creation::upload_file_to_version),
    );
}

pub fn users_config(cfg: &mut web::ServiceConfig) {
    cfg.service(users::user_auth_get);

    cfg.service(users::users_get);
    cfg.service(
        web::scope("user")
            .service(users::user_get)
            .service(users::mods_list)
            .service(users::user_delete)
            .service(users::teams),
    );
}

pub fn teams_config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("team")
            .service(teams::team_members_get)
            .service(teams::edit_team_member)
            .service(teams::add_team_member)
            .service(teams::join_team)
            .service(teams::remove_team_member),
    );
}

#[derive(thiserror::Error, Debug)]
pub enum ApiError {
    #[error("Internal server error")]
    DatabaseError(#[from] crate::database::models::DatabaseError),
    #[error("Deserialization error: {0}")]
    JsonError(#[from] serde_json::Error),
    #[error("Authentication Error")]
    AuthenticationError,
    #[error("Invalid Input: {0}")]
    InvalidInputError(String),
    #[error("Search Error: {0}")]
    SearchError(#[from] meilisearch_sdk::errors::Error),
}

impl actix_web::ResponseError for ApiError {
    fn status_code(&self) -> actix_web::http::StatusCode {
        match self {
            ApiError::DatabaseError(..) => actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
            ApiError::AuthenticationError => actix_web::http::StatusCode::UNAUTHORIZED,
            ApiError::JsonError(..) => actix_web::http::StatusCode::BAD_REQUEST,
            ApiError::SearchError(..) => actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
            ApiError::InvalidInputError(..) => actix_web::http::StatusCode::BAD_REQUEST,
        }
    }

    fn error_response(&self) -> actix_web::web::HttpResponse {
        actix_web::web::HttpResponse::build(self.status_code()).json(
            crate::models::error::ApiError {
                error: match self {
                    ApiError::DatabaseError(..) => "database_error",
                    ApiError::AuthenticationError => "unauthorized",
                    ApiError::JsonError(..) => "json_error",
                    ApiError::SearchError(..) => "search_error",
                    ApiError::InvalidInputError(..) => "invalid_input",
                },
                description: &self.to_string(),
            },
        )
    }
}
