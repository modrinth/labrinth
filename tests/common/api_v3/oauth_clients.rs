use axum_test::{http::StatusCode, TestResponse};
use labrinth::{
    models::{
        oauth_clients::{OAuthClient, OAuthClientAuthorization},
        pats::Scopes,
    },
    routes::v3::oauth_clients::OAuthClientEdit,
};
use serde_json::json;

use crate::{assert_status, common::api_common::AppendsOptionalPat};

use super::ApiV3;

impl ApiV3 {
    pub async fn add_oauth_client(
        &self,
        name: String,
        max_scopes: Scopes,
        redirect_uris: Vec<String>,
        pat: Option<&str>,
    ) -> TestResponse {
        let max_scopes = max_scopes.bits();
        self.test_server
            .post("/_internal/oauth/app")
            .append_pat(pat)
            .json(&json!({
                "name": name,
                "max_scopes": max_scopes,
                "redirect_uris": redirect_uris
            }))
            .await
    }

    pub async fn get_user_oauth_clients(
        &self,
        user_id: &str,
        pat: Option<&str>,
    ) -> Vec<OAuthClient> {
        let resp = self
            .test_server
            .get(&format!("/v3/user/{}/oauth_apps", user_id))
            .append_pat(pat)
            .await;
        assert_status!(&resp, StatusCode::OK);
        resp.json()
    }

    pub async fn get_oauth_client(&self, client_id: String, pat: Option<&str>) -> TestResponse {
        self.test_server
            .get(&format!("/_internal/oauth/app/{}", client_id))
            .append_pat(pat)
            .await
    }

    pub async fn edit_oauth_client(
        &self,
        client_id: &str,
        edit: OAuthClientEdit,
        pat: Option<&str>,
    ) -> TestResponse {
        self.test_server
            .patch(&format!(
                "/_internal/oauth/app/{}",
                urlencoding::encode(client_id)
            ))
            .append_pat(pat)
            .json(&edit)
            .await
    }

    pub async fn delete_oauth_client(&self, client_id: &str, pat: Option<&str>) -> TestResponse {
        self.test_server
            .delete(&format!("/_internal/oauth/app/{}", client_id))
            .append_pat(pat)
            .await
    }

    pub async fn revoke_oauth_authorization(
        &self,
        client_id: &str,
        pat: Option<&str>,
    ) -> TestResponse {
        self.test_server
            .delete("/_internal/oauth/authorizations")
            .add_query_param("client_id", client_id)
            .append_pat(pat)
            .await
    }

    pub async fn get_user_oauth_authorizations(
        &self,
        pat: Option<&str>,
    ) -> Vec<OAuthClientAuthorization> {
        let resp = self
            .test_server
            .get("/_internal/oauth/authorizations")
            .append_pat(pat)
            .await;
        assert_status!(&resp, StatusCode::OK);
        resp.json()
    }
}
