use actix_web::{
    dev::ServiceResponse,
    test::{self, TestRequest},
};
use labrinth::routes::v3::tags::{CategoryData, LoaderData};

use crate::common::database::ADMIN_USER_PAT;

use super::ApiV3;

impl ApiV3 {
    pub async fn get_loaders(&self) -> ServiceResponse {
        let req = TestRequest::get()
            .uri("/v3/tag/loader")
            .append_header(("Authorization", ADMIN_USER_PAT))
            .to_request();
        self.call(req).await
    }

    pub async fn get_loaders_deserialized(&self) -> Vec<LoaderData> {
        let resp = self.get_loaders().await;
        assert_eq!(resp.status(), 200);
        test::read_body_json(resp).await
    }

    pub async fn get_categories(&self) -> ServiceResponse {
        let req = TestRequest::get()
            .uri("/v3/tag/category")
            .append_header(("Authorization", ADMIN_USER_PAT))
            .to_request();
        self.call(req).await
    }

    pub async fn get_categories_deserialized(&self) -> Vec<CategoryData> {
        let resp = self.get_categories().await;
        assert_eq!(resp.status(), 200);
        test::read_body_json(resp).await
    }
}
