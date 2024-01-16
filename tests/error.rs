use axum_test::http::StatusCode;

use bytes::Bytes;
use common::api_common::ApiProject;

use common::api_v3::ApiV3;
use common::database::USER_USER_PAT;
use common::environment::{with_test_environment, TestEnvironment};

mod common;

#[tokio::test]
pub async fn error_404_body() {
    with_test_environment(None, |test_env: TestEnvironment<ApiV3>| async move {
        // v3 errors should have 404 as non-blank body, for missing resources
        let api = &test_env.api;
        let resp = api.get_project("does-not-exist", USER_USER_PAT).await;
        assert_status!(&resp, StatusCode::NOT_FOUND);
        let body = resp.as_bytes();
        let empty_bytes = Bytes::from_static(b"");
        assert_ne!(body, &empty_bytes);
    })
    .await;
}
