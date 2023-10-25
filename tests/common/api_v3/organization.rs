use actix_web::{dev::ServiceResponse, test::TestRequest};

use crate::common::actix::TestRequestExtensions;

use super::ApiV3;

impl ApiV3 {
    pub async fn follow_organization(&self, organization_id: &str, pat: &str) -> ServiceResponse {
        let req = TestRequest::post()
            .uri(&format!("/v3/organization/{}/follow", organization_id))
            .append_auth(pat)
            .to_request();

        self.call(req).await
    }

    pub async fn unfollow_organization(&self, organization_id: &str, pat: &str) -> ServiceResponse {
        let req = TestRequest::delete()
            .uri(&format!("/v3/organization/{}/follow", organization_id))
            .append_auth(pat)
            .to_request();

        self.call(req).await
    }
}
