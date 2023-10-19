use actix_http::StatusCode;
use actix_web::test::{self, TestRequest};
use labrinth::models::{oauth_clients::OAuthClientCreationResult, pats::Scopes};
use reqwest::header::AUTHORIZATION;
use serde_json::json;

use crate::common::asserts::assert_status;

use super::ApiV3;

impl ApiV3 {
    pub async fn add_oauth_client(
        &self,
        name: String,
        max_scopes: Scopes,
        redirect_uris: Vec<String>,
        pat: &str,
    ) -> OAuthClientCreationResult {
        let max_scopes = max_scopes.bits();
        let req = TestRequest::post()
            .uri("/v3/oauth_app")
            .append_header((AUTHORIZATION, pat))
            .set_json(json!({
                "name": name,
                "max_scopes": max_scopes,
                "redirect_uris": redirect_uris
            }))
            .to_request();

        let resp = self.call(req).await;
        assert_status(&resp, StatusCode::OK);

        test::read_body_json(resp).await
    }
}
