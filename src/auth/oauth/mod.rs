use crate::auth::get_user_from_headers;
use crate::auth::oauth::uris::{OAuthRedirectUris, ValidatedRedirectUri};
use crate::auth::validate::extract_authorization_header;
use crate::database::models::flow_item::Flow;
use crate::database::models::oauth_client_authorization_item::OAuthClientAuthorization;
use crate::database::models::oauth_client_item::OAuthClient as DBOAuthClient;
use crate::database::models::oauth_token_item::OAuthAccessToken;
use crate::database::models::{
    generate_oauth_access_token_id, generate_oauth_client_authorization_id,
    OAuthClientAuthorizationId,
};
use crate::database::redis::RedisPool;
use crate::models::ids::OAuthClientId;
use crate::models::pats::Scopes;
use crate::queue::session::AuthQueue;
use axum::extract::{ConnectInfo, Query};
use axum::http::header::{CACHE_CONTROL, LOCATION, PRAGMA};
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Extension, Form, Json, Router};
use chrono::Duration;
use rand::distributions::Alphanumeric;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPool;
use std::net::SocketAddr;
use std::sync::Arc;

use self::errors::{OAuthError, OAuthErrorType};

use super::AuthenticationError;

pub mod errors;
pub mod uris;

pub fn config() -> Router {
    Router::new()
        .route("/authorize", get(init_oauth))
        .service("/accept", post(accept_client_scopes))
        .service("/reject", post(reject_client_scopes))
        .service("/token", post(request_token))
}

#[derive(Serialize, Deserialize)]
pub struct OAuthInit {
    pub client_id: OAuthClientId,
    pub redirect_uri: Option<String>,
    pub scope: Option<String>,
    pub state: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct OAuthClientAccessRequest {
    pub flow_id: String,
    pub client_id: OAuthClientId,
    pub client_name: String,
    pub client_icon: Option<String>,
    pub requested_scopes: Scopes,
}

pub async fn init_oauth(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(oauth_info): Query<OAuthInit>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<Json<OAuthClientAccessRequest>, OAuthError> {
    let user = get_user_from_headers(
        &addr,
        &headers,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::USER_AUTH_WRITE]),
    )
    .await?
    .1;

    let client_id = oauth_info.client_id.into();
    let client = DBOAuthClient::get(client_id, &**pool).await?;

    if let Some(client) = client {
        let redirect_uri = ValidatedRedirectUri::validate(
            &oauth_info.redirect_uri,
            client.redirect_uris.iter().map(|r| r.uri.as_ref()),
            client.id,
        )?;

        let requested_scopes = oauth_info
            .scope
            .as_ref()
            .map_or(Ok(client.max_scopes), |s| {
                Scopes::parse_from_oauth_scopes(s).map_err(|e| {
                    OAuthError::redirect(
                        OAuthErrorType::FailedScopeParse(e),
                        &oauth_info.state,
                        &redirect_uri,
                    )
                })
            })?;

        if !client.max_scopes.contains(requested_scopes) {
            return Err(OAuthError::redirect(
                OAuthErrorType::ScopesTooBroad,
                &oauth_info.state,
                &redirect_uri,
            ));
        }

        let existing_authorization =
            OAuthClientAuthorization::get(client.id, user.id.into(), &**pool)
                .await
                .map_err(|e| OAuthError::redirect(e, &oauth_info.state, &redirect_uri))?;
        let redirect_uris =
            OAuthRedirectUris::new(oauth_info.redirect_uri.clone(), redirect_uri.clone());
        match existing_authorization {
            Some(existing_authorization)
                if existing_authorization.scopes.contains(requested_scopes) =>
            {
                init_oauth_code_flow(
                    user.id.into(),
                    client.id.into(),
                    existing_authorization.id,
                    requested_scopes,
                    redirect_uris,
                    oauth_info.state,
                    &redis,
                )
                .await
            }
            _ => {
                let flow_id = Flow::InitOAuthAppApproval {
                    user_id: user.id.into(),
                    client_id: client.id,
                    existing_authorization_id: existing_authorization.map(|a| a.id),
                    scopes: requested_scopes,
                    redirect_uris,
                    state: oauth_info.state.clone(),
                }
                .insert(Duration::minutes(30), &redis)
                .await
                .map_err(|e| OAuthError::redirect(e, &oauth_info.state, &redirect_uri))?;

                let access_request = OAuthClientAccessRequest {
                    client_id: client.id.into(),
                    client_name: client.name,
                    client_icon: client.icon_url,
                    flow_id,
                    requested_scopes,
                };
                Ok(Json(access_request))
            }
        }
    } else {
        Err(OAuthError::error(OAuthErrorType::InvalidClientId(
            client_id,
        )))
    }
}

#[derive(Serialize, Deserialize)]
pub struct RespondToOAuthClientScopes {
    pub flow: String,
}

pub async fn accept_client_scopes(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(accept_body): Json<RespondToOAuthClientScopes>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<impl IntoResponse, OAuthError> {
    accept_or_reject_client_scopes(true, addr, headers, accept_body, pool, redis, session_queue)
        .await
}

pub async fn reject_client_scopes(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<RespondToOAuthClientScopes>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<impl IntoResponse, OAuthError> {
    accept_or_reject_client_scopes(false, addr, headers, body, pool, redis, session_queue).await
}

#[derive(Serialize, Deserialize)]
pub struct TokenRequest {
    pub grant_type: String,
    pub code: String,
    pub redirect_uri: Option<String>,
    pub client_id: OAuthClientId,
}

#[derive(Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: i64,
}

/// Params should be in the urlencoded request body
/// And client secret should be in the HTTP basic authorization header
/// Per IETF RFC6749 Section 4.1.3 (https://datatracker.ietf.org/doc/html/rfc6749#section-4.1.3)
pub async fn request_token(
    headers: HeaderMap,
    Form(req_params): Form<TokenRequest>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
) -> Result<impl IntoResponse, OAuthError> {
    let req_client_id = req_params.client_id;
    let client = DBOAuthClient::get(req_client_id.into(), &**pool).await?;
    if let Some(client) = client {
        authenticate_client_token_request(&headers, &client)?;

        // Ensure auth code is single use
        // per IETF RFC6749 Section 10.5 (https://datatracker.ietf.org/doc/html/rfc6749#section-10.5)
        let flow = Flow::take_if(
            &req_params.code,
            |f| matches!(f, Flow::OAuthAuthorizationCodeSupplied { .. }),
            &redis,
        )
        .await?;
        if let Some(Flow::OAuthAuthorizationCodeSupplied {
            user_id,
            client_id,
            authorization_id,
            scopes,
            original_redirect_uri,
        }) = flow
        {
            // https://datatracker.ietf.org/doc/html/rfc6749#section-4.1.3
            if req_client_id != client_id.into() {
                return Err(OAuthError::error(OAuthErrorType::UnauthorizedClient));
            }

            if original_redirect_uri != req_params.redirect_uri {
                return Err(OAuthError::error(OAuthErrorType::RedirectUriChanged(
                    req_params.redirect_uri.clone(),
                )));
            }

            if req_params.grant_type != "authorization_code" {
                return Err(OAuthError::error(
                    OAuthErrorType::OnlySupportsAuthorizationCodeGrant(
                        req_params.grant_type.clone(),
                    ),
                ));
            }

            let scopes = scopes - Scopes::restricted();

            let mut transaction = pool.begin().await?;
            let token_id = generate_oauth_access_token_id(&mut transaction).await?;
            let token = generate_access_token();
            let token_hash = OAuthAccessToken::hash_token(&token);
            let time_until_expiration = OAuthAccessToken {
                id: token_id,
                authorization_id,
                token_hash,
                scopes,
                created: Default::default(),
                expires: Default::default(),
                last_used: None,
                client_id,
                user_id,
            }
            .insert(&mut *transaction)
            .await?;

            transaction.commit().await?;

            // IETF RFC6749 Section 5.1 (https://datatracker.ietf.org/doc/html/rfc6749#section-5.1)

            Ok((
                [(CACHE_CONTROL, "no-store"), (PRAGMA, "no-cache")],
                Json(TokenResponse {
                    access_token: token,
                    token_type: "Bearer".to_string(),
                    expires_in: time_until_expiration.num_seconds(),
                }),
            ))
        } else {
            Err(OAuthError::error(OAuthErrorType::InvalidAuthCode))
        }
    } else {
        Err(OAuthError::error(OAuthErrorType::InvalidClientId(
            req_client_id.into(),
        )))
    }
}

pub async fn accept_or_reject_client_scopes(
    accept: bool,
    addr: SocketAddr,
    headers: HeaderMap,
    body: RespondToOAuthClientScopes,
    pool: PgPool,
    redis: RedisPool,
    session_queue: Arc<AuthQueue>,
) -> Result<impl IntoResponse, OAuthError> {
    let current_user = get_user_from_headers(
        &addr,
        &headers,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::SESSION_ACCESS]),
    )
    .await?
    .1;

    let flow = Flow::take_if(
        &body.flow,
        |f| matches!(f, Flow::InitOAuthAppApproval { .. }),
        &redis,
    )
    .await?;
    if let Some(Flow::InitOAuthAppApproval {
        user_id,
        client_id,
        existing_authorization_id,
        scopes,
        redirect_uris,
        state,
    }) = flow
    {
        if current_user.id != user_id.into() {
            return Err(OAuthError::error(AuthenticationError::InvalidCredentials));
        }

        if accept {
            let mut transaction = pool.begin().await?;

            let auth_id = match existing_authorization_id {
                Some(id) => id,
                None => generate_oauth_client_authorization_id(&mut transaction).await?,
            };
            OAuthClientAuthorization::upsert(auth_id, client_id, user_id, scopes, &mut transaction)
                .await?;

            transaction.commit().await?;

            init_oauth_code_flow(
                user_id,
                client_id.into(),
                auth_id,
                scopes,
                redirect_uris,
                state,
                &redis,
            )
            .await
        } else {
            Err(OAuthError::redirect(
                OAuthErrorType::AccessDenied,
                &state,
                &redirect_uris.validated,
            ))
        }
    } else {
        Err(OAuthError::error(OAuthErrorType::InvalidAcceptFlowId))
    }
}

fn authenticate_client_token_request(
    headers: &HeaderMap,
    client: &DBOAuthClient,
) -> Result<(), OAuthError> {
    let client_secret = extract_authorization_header(headers)?;
    let hashed_client_secret = DBOAuthClient::hash_secret(client_secret);
    if client.secret_hash != hashed_client_secret {
        Err(OAuthError::error(
            OAuthErrorType::ClientAuthenticationFailed,
        ))
    } else {
        Ok(())
    }
}

fn generate_access_token() -> String {
    let random = ChaCha20Rng::from_entropy()
        .sample_iter(&Alphanumeric)
        .take(60)
        .map(char::from)
        .collect::<String>();
    format!("mro_{}", random)
}

async fn init_oauth_code_flow(
    user_id: crate::database::models::UserId,
    client_id: OAuthClientId,
    authorization_id: OAuthClientAuthorizationId,
    scopes: Scopes,
    redirect_uris: OAuthRedirectUris,
    state: Option<String>,
    redis: &RedisPool,
) -> Result<impl IntoResponse, OAuthError> {
    let code = Flow::OAuthAuthorizationCodeSupplied {
        user_id,
        client_id: client_id.into(),
        authorization_id,
        scopes,
        original_redirect_uri: redirect_uris.original.clone(),
    }
    .insert(Duration::minutes(10), redis)
    .await
    .map_err(|e| OAuthError::redirect(e, &state, &redirect_uris.validated.clone()))?;

    let mut redirect_params = vec![format!("code={code}")];
    if let Some(state) = state {
        redirect_params.push(format!("state={state}"));
    }

    let redirect_uri = append_params_to_uri(&redirect_uris.validated.0, &redirect_params);

    Ok(([(LOCATION, &*redirect_uri)], redirect_uri))
}

fn append_params_to_uri(uri: &str, params: &[impl AsRef<str>]) -> String {
    let mut uri = uri.to_string();
    let mut connector = if uri.contains('?') { "&" } else { "?" };
    for param in params {
        uri.push_str(&format!("{}{}", connector, param.as_ref()));
        connector = "&";
    }

    uri
}
