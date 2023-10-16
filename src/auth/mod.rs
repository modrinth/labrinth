pub mod checks;
pub mod email;
pub mod flows;
pub mod pats;
pub mod session;
mod templates;
pub mod validate;
pub use checks::{
    filter_authorized_projects, filter_authorized_versions, is_authorized, is_authorized_version,
};
// pub use pat::{generate_pat, PersonalAccessToken};
pub use validate::{check_is_moderator_from_headers, get_user_from_headers};

use crate::file_hosting::FileHostingError;
use crate::models::error::ApiError;
use actix_web::http::StatusCode;
use actix_web::HttpResponse;
use thiserror::Error;

use self::flows::{OAuthInit, ValidatedRedirectUri};

#[derive(Error, Debug)]
pub enum AuthenticationError {
    #[error("Environment Error")]
    Env(#[from] dotenvy::Error),
    #[error("An unknown database error occurred: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("Database Error: {0}")]
    Database(#[from] crate::database::models::DatabaseError),
    #[error("Error while parsing JSON: {0}")]
    SerDe(#[from] serde_json::Error),
    #[error("Error while communicating to external provider")]
    Reqwest(#[from] reqwest::Error),
    #[error("Error uploading user profile picture")]
    FileHosting(#[from] FileHostingError),
    #[error("Error while decoding PAT: {0}")]
    Decoding(#[from] crate::models::ids::DecodingError),
    #[error("{0}")]
    Mail(#[from] email::MailError),
    #[error("Invalid Authentication Credentials")]
    InvalidCredentials,
    #[error("Authentication method was not valid")]
    InvalidAuthMethod,
    #[error("GitHub Token from incorrect Client ID")]
    InvalidClientId,
    #[error("User email/account is already registered on Modrinth")]
    DuplicateUser,
    #[error("Invalid state sent, you probably need to get a new websocket")]
    SocketError,
    #[error("Invalid callback URL specified")]
    Url,
}

impl actix_web::ResponseError for AuthenticationError {
    fn status_code(&self) -> StatusCode {
        match self {
            AuthenticationError::Env(..) => StatusCode::INTERNAL_SERVER_ERROR,
            AuthenticationError::Sqlx(..) => StatusCode::INTERNAL_SERVER_ERROR,
            AuthenticationError::Database(..) => StatusCode::INTERNAL_SERVER_ERROR,
            AuthenticationError::SerDe(..) => StatusCode::BAD_REQUEST,
            AuthenticationError::Reqwest(..) => StatusCode::INTERNAL_SERVER_ERROR,
            AuthenticationError::InvalidCredentials => StatusCode::UNAUTHORIZED,
            AuthenticationError::Decoding(..) => StatusCode::BAD_REQUEST,
            AuthenticationError::Mail(..) => StatusCode::INTERNAL_SERVER_ERROR,
            AuthenticationError::InvalidAuthMethod => StatusCode::UNAUTHORIZED,
            AuthenticationError::InvalidClientId => StatusCode::UNAUTHORIZED,
            AuthenticationError::Url => StatusCode::BAD_REQUEST,
            AuthenticationError::FileHosting(..) => StatusCode::INTERNAL_SERVER_ERROR,
            AuthenticationError::DuplicateUser => StatusCode::BAD_REQUEST,
            AuthenticationError::SocketError => StatusCode::BAD_REQUEST,
        }
    }

    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status_code()).json(ApiError {
            error: self.error_name(),
            description: &self.to_string(),
        })
    }
}

impl AuthenticationError {
    pub fn error_name(&self) -> &'static str {
        match self {
            AuthenticationError::Env(..) => "environment_error",
            AuthenticationError::Sqlx(..) => "database_error",
            AuthenticationError::Database(..) => "database_error",
            AuthenticationError::SerDe(..) => "invalid_input",
            AuthenticationError::Reqwest(..) => "network_error",
            AuthenticationError::InvalidCredentials => "invalid_credentials",
            AuthenticationError::Decoding(..) => "decoding_error",
            AuthenticationError::Mail(..) => "mail_error",
            AuthenticationError::InvalidAuthMethod => "invalid_auth_method",
            AuthenticationError::InvalidClientId => "invalid_client_id",
            AuthenticationError::Url => "url_error",
            AuthenticationError::FileHosting(..) => "file_hosting",
            AuthenticationError::DuplicateUser => "duplicate_user",
            AuthenticationError::SocketError => "socket",
        }
    }
}

#[derive(thiserror::Error, Debug)]
#[error("{}", .error_type)]
pub struct OAuthError {
    #[source]
    pub error_type: OAuthErrorType,

    pub state: Option<String>,
    pub valid_redirect_uri: Option<ValidatedRedirectUri>,
}

impl OAuthError {
    /// The OAuth request failed either because of an invalid redirection URI
    /// or before we could validate the one we were given, so return an error
    /// directly to the caller
    ///
    /// See: IETF RFC 6749 4.1.2.1 (https://datatracker.ietf.org/doc/html/rfc6749#section-4.1.2.1)
    pub fn error(error_type: impl Into<OAuthErrorType>) -> Self {
        Self {
            error_type: error_type.into(),
            valid_redirect_uri: None,
            state: None,
        }
    }

    /// The OAuth request failed for a reason other than an invalid redirection URI
    /// So send the error in url-encoded form to the redirect URI
    ///
    /// See: IETF RFC 6749 4.1.2.1 (https://datatracker.ietf.org/doc/html/rfc6749#section-4.1.2.1)
    pub fn redirect(
        err: impl Into<OAuthErrorType>,
        state: &Option<String>,
        valid_redirect_uri: &ValidatedRedirectUri,
    ) -> Self {
        Self {
            error_type: err.into(),
            state: state.clone(),
            valid_redirect_uri: Some(valid_redirect_uri.clone()),
        }
    }
}

impl actix_web::ResponseError for OAuthError {
    fn status_code(&self) -> StatusCode {
        match self.error_type {
            OAuthErrorType::AuthenticationError(_)
            | OAuthErrorType::UnrecognizedClient { client_id: _ }
            | OAuthErrorType::FailedScopeParse(_)
            | OAuthErrorType::ScopesTooBroad => {
                if self.valid_redirect_uri.is_some() {
                    StatusCode::FOUND
                } else {
                    StatusCode::INTERNAL_SERVER_ERROR
                }
            }
            OAuthErrorType::InvalidRedirectUri(_)
            | OAuthErrorType::ClientMissingRedirectURI { client_id: _ }
            | OAuthErrorType::InvalidAcceptFlowId => StatusCode::BAD_REQUEST,
        }
    }

    fn error_response(&self) -> HttpResponse {
        if let Some(ValidatedRedirectUri(mut redirect_uri)) = self.valid_redirect_uri.clone() {
            redirect_uri = format!(
                "{}?error={}&error_description={}",
                redirect_uri.to_string(),
                self.error_type.error_name(),
                self.error_type.to_string(),
            );

            if let Some(state) = self.state.as_ref() {
                redirect_uri = format!("{}&state={}", redirect_uri, state);
            }

            redirect_uri = urlencoding::encode(&redirect_uri).to_string();
            HttpResponse::Found()
                .append_header(("Location".to_string(), redirect_uri))
                .finish()
        } else {
            HttpResponse::build(self.status_code()).json(ApiError {
                error: &self.error_type.error_name(),
                description: &self.error_type.to_string(),
            })
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum OAuthErrorType {
    #[error(transparent)]
    AuthenticationError(#[from] AuthenticationError),
    #[error("Client {} not recognized", .client_id.0)]
    UnrecognizedClient {
        client_id: crate::database::models::OAuthClientId,
    },
    #[error("Client {} has no redirect URIs specified", .client_id.0)]
    ClientMissingRedirectURI {
        client_id: crate::database::models::OAuthClientId,
    },
    #[error("The provided redirect URI did not match any configured in the client")]
    InvalidRedirectUri(String),
    #[error("The provided scope was malformed or did not correspond to known scopes ({0})")]
    FailedScopeParse(bitflags::parser::ParseError),
    #[error(
        "The provided scope requested scopes broader than the developer app is configured with"
    )]
    ScopesTooBroad,
    #[error("The provided flow id was invalid")]
    InvalidAcceptFlowId,
}

impl From<crate::database::models::DatabaseError> for OAuthErrorType {
    fn from(value: crate::database::models::DatabaseError) -> Self {
        OAuthErrorType::AuthenticationError(value.into())
    }
}

impl From<sqlx::Error> for OAuthErrorType {
    fn from(value: sqlx::Error) -> Self {
        OAuthErrorType::AuthenticationError(value.into())
    }
}

impl OAuthErrorType {
    pub fn error_name(&self) -> String {
        // IETF RFC 6749 4.1.2.1 (https://datatracker.ietf.org/doc/html/rfc6749#autoid-38)
        match self {
            OAuthErrorType::InvalidRedirectUri(_)
            | OAuthErrorType::ClientMissingRedirectURI { client_id: _ } => "invalid_uri",
            OAuthErrorType::AuthenticationError(_) | OAuthErrorType::InvalidAcceptFlowId => {
                "server_error"
            }
            OAuthErrorType::UnrecognizedClient { client_id: _ } => "invalid_request",
            OAuthErrorType::FailedScopeParse(_) | OAuthErrorType::ScopesTooBroad => "invalid_scope",
        }
        .to_string()
    }
}
