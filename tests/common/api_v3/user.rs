use axum_test::TestResponse;
use async_trait::async_trait;

use crate::common::api_common::{ApiUser, AppendsOptionalPat};

use super::ApiV3;

#[async_trait(?Send)]
impl ApiUser for ApiV3 {
    async fn get_user(&self, user_id_or_username: &str, pat: Option<&str>) -> TestResponse {
        self.test_server
            .get(&format!("/v3/user/{user_id_or_username}"))
            .append_pat(pat)
            .await
    }

    async fn get_current_user(&self, pat: Option<&str>) -> TestResponse {
        self.test_server.get("/v3/user").append_pat(pat).await
    }

    async fn edit_user(
        &self,
        user_id_or_username: &str,
        patch: serde_json::Value,
        pat: Option<&str>,
    ) -> TestResponse {
        self.test_server
            .patch(&format!("/v3/user/{user_id_or_username}"))
            .append_pat(pat)
            .json(&patch)
            .await
    }

    async fn delete_user(&self, user_id_or_username: &str, pat: Option<&str>) -> TestResponse {
        self.test_server
            .delete(&format!("/v3/user/{user_id_or_username}"))
            .append_pat(pat)
            .await
    }
}
