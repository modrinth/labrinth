use actix_http::StatusCode;
use actix_web::{
    dev::ServiceResponse,
    test::{self, TestRequest},
};
use labrinth::models::feed_item::FeedItem;

use crate::common::{actix::TestRequestExtensions, asserts::assert_status};

use super::ApiV3;

impl ApiV3 {
    pub async fn follow_user(&self, user_id: &str, pat: &str) -> ServiceResponse {
        let req = TestRequest::post()
            .uri(&format!("/v3/user/{}/follow", user_id))
            .append_auth(pat)
            .to_request();

        self.call(req).await
    }

    pub async fn unfollow_user(&self, user_id: &str, pat: &str) -> ServiceResponse {
        let req = TestRequest::delete()
            .uri(&format!("/v3/user/{}/follow", user_id))
            .append_auth(pat)
            .to_request();

        self.call(req).await
    }

    pub async fn get_feed(&self, pat: &str) -> Vec<FeedItem> {
        let req = TestRequest::get()
            .uri("/v3/user/feed")
            .append_auth(pat)
            .to_request();
        let resp = self.call(req).await;
        assert_status(&resp, StatusCode::OK);

        test::read_body_json(resp).await
    }
}
