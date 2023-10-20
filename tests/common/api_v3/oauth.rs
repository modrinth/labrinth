use actix_web::{
    dev::ServiceResponse,
    test::{self, TestRequest},
};
use labrinth::auth::oauth::{AcceptOAuthClientScopes, TokenRequest, TokenResponse};
use reqwest::header::AUTHORIZATION;

use super::ApiV3;

impl ApiV3 {
    pub async fn oauth_authorize(
        &self,
        client_id: &str,
        scope: &str,
        redirect_uri: &str,
        state: Option<&str>,
        pat: &str,
    ) -> ServiceResponse {
        let uri = generate_authorize_uri(client_id, scope, redirect_uri, state);
        let req = TestRequest::get()
            .uri(&uri)
            .append_header((AUTHORIZATION, pat))
            .to_request();
        self.call(req).await
    }

    pub async fn oauth_accept(&self, flow: &str, pat: &str) -> ServiceResponse {
        self.call(
            TestRequest::post()
                .uri("/v3/auth/oauth/accept")
                .append_header((AUTHORIZATION, pat))
                .set_json(AcceptOAuthClientScopes {
                    flow: flow.to_string(),
                })
                .to_request(),
        )
        .await
    }

    pub async fn oauth_token(
        &self,
        auth_code: String,
        original_redirect_uri: Option<String>,
        client_id: String,
        client_secret: &str,
    ) -> ServiceResponse {
        self.call(
            TestRequest::post()
                .uri("/v3/auth/oauth/token")
                .append_header((AUTHORIZATION, client_secret))
                .set_form(TokenRequest {
                    grant_type: "authorization_code".to_string(),
                    code: auth_code,
                    redirect_uri: original_redirect_uri,
                    client_id,
                })
                .to_request(),
        )
        .await
    }

    pub async fn get_oauth_access_token(
        &self,
        auth_code: String,
        original_redirect_uri: Option<String>,
        client_id: String,
        client_secret: &str,
    ) -> String {
        let response = self
            .oauth_token(auth_code, original_redirect_uri, client_id, client_secret)
            .await;
        let token_resp: TokenResponse = test::read_body_json(response).await;
        token_resp.access_token
    }
}

pub fn generate_authorize_uri(
    client_id: &str,
    scope: &str,
    redirect_uri: &str,
    state: Option<&str>,
) -> String {
    format!(
        "/v3/auth/oauth/authorize?client_id={}&redirect_uri={}\
            &scope={}{}",
        urlencoding::encode(client_id),
        urlencoding::encode(redirect_uri),
        urlencoding::encode(scope),
        if let Some(state) = state {
            urlencoding::encode(state).to_string()
        } else {
            "".to_string()
        },
    )
    .to_string()
}
