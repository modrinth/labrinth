use crate::auth::get_user_from_headers;
use crate::database::models::flow_item::Flow;
use crate::database::models::oauth_client_authorization_item::OAuthClientAuthorization;
use crate::database::models::oauth_client_item::OAuthClient as DBOAuthClient;
use crate::database::models::{
    generate_oauth_client_authorization_id, OAuthClientAuthorizationId, OAuthClientId,
};
use crate::database::redis::RedisPool;
use crate::models::pats::Scopes;
use crate::queue::session::AuthQueue;
use actix_web::web::{scope, Data, Query, ServiceConfig};
use actix_web::{get, web, HttpRequest, HttpResponse};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPool;

use self::errors::{OAuthError, OAuthErrorType};

pub mod errors;

pub fn config(cfg: &mut ServiceConfig) {
    cfg.service(
        scope("auth/oauth")
            .service(init_oauth)
            .service(accept_client_scopes),
    );
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

#[get("authorize")]
pub async fn init_oauth(
    req: HttpRequest,
    Query(oauth_info): Query<OAuthInit>,
    pool: Data<PgPool>,
    redis: Data<RedisPool>,
    session_queue: Data<AuthQueue>,
) -> Result<HttpResponse, OAuthError> {
    let user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::USER_AUTH_WRITE]),
    )
    .await
    .map_err(OAuthError::error)?
    .1;

    let client = DBOAuthClient::get(oauth_info.client_id, &**pool)
        .await
        .map_err(OAuthError::error)?;

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
        match existing_authorization {
            Some(existing_authorization)
                if existing_authorization.scopes.contains(requested_scopes) =>
            {
                init_oauth_code_flow(
                    user.id.into(),
                    client.id,
                    existing_authorization.id,
                    requested_scopes,
                    redirect_uri,
                    oauth_info.redirect_uri,
                    oauth_info.state,
                    &redis,
                )
                .await
            }
            _ => {
                let flow_id = Flow::InitOAuthAppApproval {
                    user_id: user.id.into(),
                    client_id: client.id,
                    scopes: requested_scopes,
                    validated_redirect_uri: redirect_uri.clone(),
                    original_redirect_uri: oauth_info.redirect_uri.clone(),
                    state: oauth_info.state.clone(),
                }
                .insert(Duration::minutes(30), &redis)
                .await
                .map_err(|e| OAuthError::redirect(e, &oauth_info.state, &redirect_uri))?;

                let access_request = OAuthClientAccessRequest {
                    client_id: client.id,
                    client_name: client.name,
                    client_icon: client.icon_url,
                    flow_id,
                    requested_scopes,
                };
                Ok(HttpResponse::Ok().json(access_request))
            }
        }
    } else {
        Err(OAuthError::error(OAuthErrorType::UnrecognizedClient {
            client_id: oauth_info.client_id,
        }))
    }
}

#[derive(Deserialize)]
pub struct AcceptOAuthClientScopes {
    pub flow: String,
}

#[get("accept")]
pub async fn accept_client_scopes(
    req: HttpRequest,
    accept_body: web::Json<AcceptOAuthClientScopes>,
    pool: Data<PgPool>,
    redis: Data<RedisPool>,
    session_queue: Data<AuthQueue>,
) -> Result<HttpResponse, OAuthError> {
    //TODO: Any way to do this without getting the user?
    let user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::USER_AUTH_WRITE]),
    )
    .await
    .map_err(|e| OAuthError::error(e))?
    .1;

    let flow = Flow::get(&accept_body.flow, &redis)
        .await
        .map_err(|e| OAuthError::error(e))?;
    if let Some(Flow::InitOAuthAppApproval {
        user_id,
        client_id,
        scopes,
        validated_redirect_uri,
        original_redirect_uri,
        state,
    }) = flow
    {
        let mut transaction = pool.begin().await.map_err(OAuthError::error)?;

        let auth_id = generate_oauth_client_authorization_id(&mut transaction)
            .await
            .map_err(OAuthError::error)?;
        OAuthClientAuthorization {
            id: auth_id,
            client_id,
            user_id,
            scopes,
            created: Utc::now(),
        }
        .insert(&mut transaction)
        .await
        .map_err(OAuthError::error)?;

        transaction.commit().await.map_err(OAuthError::error)?;

        init_oauth_code_flow(
            user_id,
            client_id,
            auth_id,
            scopes,
            validated_redirect_uri,
            original_redirect_uri,
            state,
            &redis,
        )
        .await
    } else {
        Err(OAuthError::error(OAuthErrorType::InvalidAcceptFlowId))
    }
}

async fn init_oauth_code_flow(
    user_id: crate::database::models::UserId,
    client_id: OAuthClientId,
    authorization_id: OAuthClientAuthorizationId,
    scopes: Scopes,
    validated_redirect_uri: ValidatedRedirectUri,
    original_redirect_uri: Option<String>,
    state: Option<String>,
    redis: &RedisPool,
) -> Result<HttpResponse, OAuthError> {
    let code = Flow::OAuthAuthorizationCodeSupplied {
        user_id,
        client_id,
        authorization_id,
        scopes,
        validated_redirect_uri: validated_redirect_uri.clone(),
        original_redirect_uri,
    }
    .insert(Duration::minutes(10), redis)
    .await
    .map_err(|e| OAuthError::redirect(e, &state, &validated_redirect_uri.clone()))?;

    let mut redirect_params = vec![format!("code={code}")];
    if let Some(state) = state {
        redirect_params.push(format!("state={state}"));
    }

    let redirect_uri = append_params_to_uri(&validated_redirect_uri.0, &redirect_params);

    // IETF RFC 6749 Section 4.1.2 (https://datatracker.ietf.org/doc/html/rfc6749#section-4.1.2)
    Ok(HttpResponse::Found()
        .append_header(("Location", redirect_uri))
        .finish())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ValidatedRedirectUri(pub String);

impl ValidatedRedirectUri {
    pub fn validate<'a>(
        to_validate: &Option<String>,
        validate_against: impl IntoIterator<Item = &'a str> + Clone,
        client_id: OAuthClientId,
    ) -> Result<Self, OAuthError> {
        if let Some(first_client_redirect_uri) = validate_against.clone().into_iter().next() {
            if let Some(to_validate) = to_validate {
                if validate_against
                    .into_iter()
                    .any(|uri| same_uri_except_query_components(&uri, to_validate))
                {
                    return Ok(ValidatedRedirectUri(to_validate.clone()));
                } else {
                    return Err(OAuthError::error(OAuthErrorType::InvalidRedirectUri(
                        to_validate.clone(),
                    )));
                }
            } else {
                return Ok(ValidatedRedirectUri(first_client_redirect_uri.to_string()));
            }
        } else {
            return Err(OAuthError::error(
                OAuthErrorType::ClientMissingRedirectURI { client_id },
            ));
        }
    }
}

fn same_uri_except_query_components(a: &str, b: &str) -> bool {
    let mut a_components = a.split('?');
    let mut b_components = b.split('?');
    a_components.next() == b_components.next()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_for_none_returns_first_valid_uri() {
        let validate_against = vec!["https://modrinth.com/a"];

        let validated =
            ValidatedRedirectUri::validate(&None, validate_against.clone(), OAuthClientId(0))
                .unwrap();

        assert_eq!(validate_against[0], validated.0);
    }

    #[test]
    fn validate_for_valid_uri_returns_first_matching_uri_ignoring_query_params() {
        let validate_against = vec![
            "https://modrinth.com/a?q3=p3&q4=p4",
            "https://modrinth.com/a/b/c?q1=p1&q2=p2",
        ];
        let to_validate = "https://modrinth.com/a/b/c?query0=param0&query1=param1".to_string();

        let validated = ValidatedRedirectUri::validate(
            &Some(to_validate.clone()),
            validate_against,
            OAuthClientId(0),
        )
        .unwrap();

        assert_eq!(to_validate, validated.0);
    }

    #[test]
    fn validate_for_invalid_uri_returns_err() {
        let validate_against = vec!["https://modrinth.com/a"];
        let to_validate = "https://modrinth.com/a/b".to_string();

        let validated =
            ValidatedRedirectUri::validate(&Some(to_validate), validate_against, OAuthClientId(0));

        assert!(
            validated.is_err_and(|e| matches!(e.error_type, OAuthErrorType::InvalidRedirectUri(_)))
        );
    }
}
