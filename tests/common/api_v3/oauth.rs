use std::collections::HashMap;

use axum_test::{http::StatusCode, TestResponse};
use labrinth::auth::oauth::{
    OAuthClientAccessRequest, RespondToOAuthClientScopes, TokenRequest, TokenResponse,
};
use axum::http::{header::{AUTHORIZATION, LOCATION}, HeaderValue};

use crate::{
    assert_status,
    common::api_common::AppendsOptionalPat,
};

use super::ApiV3;

impl ApiV3 {
    pub async fn complete_full_authorize_flow(
        &self,
        client_id: &str,
        client_secret: &str,
        scope: Option<&str>,
        redirect_uri: Option<&str>,
        state: Option<&str>,
        user_pat: Option<&str>,
    ) -> String {
        let auth_resp = self
            .oauth_authorize(client_id, scope, redirect_uri, state, user_pat)
            .await;
        let flow_id = get_authorize_accept_flow_id(auth_resp).await;
        let redirect_resp = self.oauth_accept(&flow_id, user_pat).await;
        let auth_code = get_auth_code_from_redirect_params(&redirect_resp).await;
        let token_resp = self
            .oauth_token(auth_code, None, client_id.to_string(), client_secret)
            .await;
        get_access_token(token_resp).await
    }

    pub async fn oauth_authorize(
        &self,
        client_id: &str,
        scope: Option<&str>,
        redirect_uri: Option<&str>,
        state: Option<&str>,
        pat: Option<&str>,
    ) -> TestResponse {
        let uri = generate_authorize_uri(client_id, scope, redirect_uri, state);
        self.test_server
            .get(&uri)
            .append_pat(pat)
            .await
    }

    pub async fn oauth_accept(&self, flow: &str, pat: Option<&str>) -> TestResponse {
        self.test_server
            .post("/_internal/oauth/accept")
            .append_pat(pat)
            .json(&RespondToOAuthClientScopes {
                flow: flow.to_string(),
            })
            .await
    }

    pub async fn oauth_reject(&self, flow: &str, pat: Option<&str>) -> TestResponse {
        self.test_server
            .post("/_internal/oauth/reject")
            .append_pat(pat)
            .json(&RespondToOAuthClientScopes {
                flow: flow.to_string(),
            })
            .await
    }

    pub async fn oauth_token(
        &self,
        auth_code: String,
        original_redirect_uri: Option<String>,
        client_id: String,
        client_secret: &str,
    ) -> TestResponse {
        self.test_server
            .post("/_internal/oauth/token")
            .add_header(AUTHORIZATION, HeaderValue::from_str(client_secret).unwrap())
            .form(&TokenRequest {
                grant_type: "authorization_code".to_string(),
                code: auth_code,
                redirect_uri: original_redirect_uri,
                client_id: serde_json::from_str(&format!("\"{}\"", client_id)).unwrap(),
            })
            .await
    }
}

pub fn generate_authorize_uri(
    client_id: &str,
    scope: Option<&str>,
    redirect_uri: Option<&str>,
    state: Option<&str>,
) -> String {
    format!(
        "/_internal/oauth/authorize?client_id={}{}{}{}",
        urlencoding::encode(client_id),
        optional_query_param("redirect_uri", redirect_uri),
        optional_query_param("scope", scope),
        optional_query_param("state", state),
    )
}

pub async fn get_authorize_accept_flow_id(response: TestResponse) -> String {
    assert_status!(&response, StatusCode::OK);
    response.json::<OAuthClientAccessRequest>().flow_id
}

pub async fn get_auth_code_from_redirect_params(response: &TestResponse) -> String {
    assert_status!(response, StatusCode::OK);
    let query_params = get_redirect_location_query_params(response);
    query_params.get("code").unwrap().to_string()
}

pub async fn get_access_token(response: TestResponse) -> String {
    assert_status!(&response, StatusCode::OK);
    response.json::<TokenResponse>().access_token
}

pub fn get_redirect_location_query_params(
    response: &TestResponse,
) -> HashMap<String, String> {
    let redirect_location = response
        .headers()
        .get(LOCATION)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let redirect_location = redirect_location.split_once('?').unwrap().1;
    serde_urlencoded::from_str(redirect_location).unwrap()
}

fn optional_query_param(key: &str, value: Option<&str>) -> String {
    if let Some(val) = value {
        format!("&{key}={}", urlencoding::encode(val))
    } else {
        "".to_string()
    }
}
