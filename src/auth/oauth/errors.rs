use super::ValidatedRedirectUri;
use crate::auth::AuthenticationError;
use crate::models::error::ApiError;
use actix_web::http::StatusCode;
use actix_web::HttpResponse;

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
