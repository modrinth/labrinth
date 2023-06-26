/*!
This auth module is how we allow for authentication within the Modrinth sphere.
It uses a self-hosted Ory Kratos instance on the backend, powered by our Minos backend.

 Applications interacting with the authenticated API (a very small portion - notifications, private projects, editing/creating projects
and versions) should include the Ory authentication cookie in their requests. This cookie is set by the Ory Kratos instance and Minos provides function to access these.

In addition, you can use a logged-in-account to generate a PAT.
This token can be passed in as a Bearer token in the Authorization header, as an alternative to a cookie.
This is useful for applications that don't have a frontend, or for applications that need to access the authenticated API on behalf of a user.

Just as a summary: Don't implement this flow in your application!
*/

use crate::database::models::generate_state_id;
use crate::models::error::ApiError;
use crate::models::ids::base62_impl::{parse_base62, to_base62};
use crate::models::ids::DecodingError;

use crate::parse_strings_from_var;
use crate::util::auth::get_user_from_github_token;

use actix_web::http::StatusCode;
use actix_web::web::{scope, Data, Query, ServiceConfig};
use actix_web::{get, HttpResponse};
use chrono::Utc;

use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPool;
use thiserror::Error;

pub fn config(cfg: &mut ServiceConfig) {
    cfg.service(scope("auth").service(auth_callback).service(init));
}

#[derive(Error, Debug)]
pub enum AuthorizationError {
    #[error("Environment Error")]
    Env(#[from] dotenvy::Error),
    #[error("An unknown database error occured: {0}")]
    SqlxDatabase(#[from] sqlx::Error),
    #[error("Database Error: {0}")]
    Database(#[from] crate::database::models::DatabaseError),
    #[error("Error while parsing JSON: {0}")]
    SerDe(#[from] serde_json::Error),
    #[error("Error with communicating to Minos")]
    Minos(#[from] reqwest::Error),
    #[error("Invalid Authentication credentials")]
    InvalidCredentials,
    #[error("Authentication Error: {0}")]
    Authentication(#[from] crate::util::auth::AuthenticationError),
    #[error("Error while decoding Base62")]
    Decoding(#[from] DecodingError),
    #[error("Invalid callback URL specified")]
    Url,
    #[error("User does not exist! Please use the ory login.")]
    DatabaseMismatch,
}
impl actix_web::ResponseError for AuthorizationError {
    fn status_code(&self) -> StatusCode {
        match self {
            AuthorizationError::Env(..) => StatusCode::INTERNAL_SERVER_ERROR,
            AuthorizationError::SqlxDatabase(..) => StatusCode::INTERNAL_SERVER_ERROR,
            AuthorizationError::Database(..) => StatusCode::INTERNAL_SERVER_ERROR,
            AuthorizationError::SerDe(..) => StatusCode::BAD_REQUEST,
            AuthorizationError::Minos(..) => StatusCode::INTERNAL_SERVER_ERROR,
            AuthorizationError::InvalidCredentials => StatusCode::UNAUTHORIZED,
            AuthorizationError::Decoding(..) => StatusCode::BAD_REQUEST,
            AuthorizationError::Authentication(..) => StatusCode::UNAUTHORIZED,
            AuthorizationError::Url => StatusCode::BAD_REQUEST,
            AuthorizationError::DatabaseMismatch => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status_code()).json(ApiError {
            error: match self {
                AuthorizationError::Env(..) => "environment_error",
                AuthorizationError::SqlxDatabase(..) => "database_error",
                AuthorizationError::Database(..) => "database_error",
                AuthorizationError::SerDe(..) => "invalid_input",
                AuthorizationError::Minos(..) => "network_error",
                AuthorizationError::InvalidCredentials => "invalid_credentials",
                AuthorizationError::Decoding(..) => "decoding_error",
                AuthorizationError::Authentication(..) => "authentication_error",
                AuthorizationError::Url => "url_error",
                AuthorizationError::DatabaseMismatch => "database_mismatch",
            },
            description: &self.to_string(),
        })
    }
}

#[derive(Serialize, Deserialize)]
pub struct AuthorizationInit {
    pub url: String,
}
#[derive(Serialize, Deserialize)]
pub struct Authorization {
    pub code: String,
    pub state: String,
}

#[derive(Serialize, Deserialize)]
pub struct AccessToken {
    pub access_token: String,
    pub scope: String,
    pub token_type: String,
}

// Init link takes us to Minos API and calls back to callback endpoint with a code and state
//http://<URL>:8000/api/v1/auth/init?url=https%3A%2F%2Fmodrinth.com%2Fmods
#[get("init")]
pub async fn init(
    Query(info): Query<AuthorizationInit>, // callback url
    client: Data<PgPool>,
) -> Result<HttpResponse, AuthorizationError> {
    let url = url::Url::parse(&info.url).map_err(|_| AuthorizationError::Url)?;

    let allowed_callback_urls = parse_strings_from_var("ALLOWED_CALLBACK_URLS").unwrap_or_default();
    let domain = url.host_str().ok_or(AuthorizationError::Url)?; // TODO: change back to .domain() (host_str is so we can use 127.0.0.1)
    if !allowed_callback_urls.iter().any(|x| domain.ends_with(x)) && domain != "modrinth.com" {
        return Err(AuthorizationError::Url);
    }

    let mut transaction = client.begin().await?;

    let state = generate_state_id(&mut transaction).await?;

    sqlx::query!(
        "
        INSERT INTO states (id, url)
        VALUES ($1, $2)
        ",
        state.0,
        info.url
    )
    .execute(&mut *transaction)
    .await?;

    transaction.commit().await?;

    let client_id = dotenvy::var("GITHUB_CLIENT_ID")?;
    let url = format!(
        "https://github.com/login/oauth/authorize?client_id={}&state={}&scope={}",
        client_id,
        to_base62(state.0 as u64),
        "read%3Auser"
    );
    Ok(HttpResponse::TemporaryRedirect()
        .append_header(("Location", &*url))
        .json(AuthorizationInit { url }))
}

#[get("callback")]
pub async fn auth_callback(
    Query(state): Query<Authorization>,
    client: Data<PgPool>,
) -> Result<HttpResponse, AuthorizationError> {
    let mut transaction = client.begin().await?;
    let state_id: u64 = parse_base62(&state.state)?;

    let result_option = sqlx::query!(
        "
        SELECT url, expires FROM states
        WHERE id = $1
        ",
        state_id as i64
    )
    .fetch_optional(&mut *transaction)
    .await?;

    // Extract cookie header from request
    if let Some(result) = result_option {
        // Extract cookie header to get authenticated user from Minos
        let duration: chrono::Duration = result.expires - Utc::now();
        if duration.num_seconds() < 0 {
            return Err(AuthorizationError::InvalidCredentials);
        }
        sqlx::query!(
            "
                DELETE FROM states
                WHERE id = $1
                ",
            state_id as i64
        )
        .execute(&mut *transaction)
        .await?;

        let client_id = dotenvy::var("GITHUB_CLIENT_ID")?;
        let client_secret = dotenvy::var("GITHUB_CLIENT_SECRET")?;

        let url = format!(
            "https://github.com/login/oauth/access_token?client_id={}&client_secret={}&code={}",
            client_id, client_secret, state.code
        );

        let token: AccessToken = reqwest::Client::new()
            .post(&url)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await?
            .json()
            .await?;

        let user_result =
            get_user_from_github_token(&token.access_token, &mut *transaction).await?;

        // Cookies exist, but user does not exist in database, meaning they are invalid
        if user_result.is_none() {
            return Err(AuthorizationError::DatabaseMismatch);
        }
        transaction.commit().await?;

        let redirect_url = if result.url.contains('?') {
            format!("{}&code={}", result.url, token.access_token)
        } else {
            format!("{}?code={}", result.url, token.access_token)
        };

        Ok(HttpResponse::TemporaryRedirect()
            .append_header(("Location", &*redirect_url))
            .json(AuthorizationInit { url: redirect_url }))
    } else {
        Err(AuthorizationError::InvalidCredentials)
    }
}
