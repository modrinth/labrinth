use async_trait::async_trait;
use axum_test::{http::StatusCode, TestResponse};
use labrinth::routes::v2::tags::{
    CategoryData, DonationPlatformQueryData, GameVersionQueryData, LoaderData,
};

use crate::{
    assert_status,
    common::{
        api_common::{
            models::{CommonCategoryData, CommonLoaderData},
            ApiTags, AppendsOptionalPat,
        },
        database::ADMIN_USER_PAT,
    },
};

use super::ApiV2;

impl ApiV2 {
    async fn get_side_types(&self) -> TestResponse {
        self.test_server
            .get(&"/v2/tag/side_type")
            .append_pat(ADMIN_USER_PAT)
            .await
    }

    pub async fn get_side_types_deserialized(&self) -> Vec<String> {
        let resp = self.get_side_types().await;
        assert_status!(&resp, StatusCode::OK);
        resp.json()
    }

    pub async fn get_game_versions(&self) -> TestResponse {
        self.test_server
            .get(&"/v2/tag/game_version")
            .append_pat(ADMIN_USER_PAT)
            .await
    }

    pub async fn get_game_versions_deserialized(&self) -> Vec<GameVersionQueryData> {
        let resp = self.get_game_versions().await;
        assert_status!(&resp, StatusCode::OK);
        resp.json()
    }

    pub async fn get_loaders_deserialized(&self) -> Vec<LoaderData> {
        let resp = self.get_loaders().await;
        assert_status!(&resp, StatusCode::OK);
        resp.json()
    }

    pub async fn get_categories_deserialized(&self) -> Vec<CategoryData> {
        let resp = self.get_categories().await;
        assert_status!(&resp, StatusCode::OK);
        resp.json()
    }

    pub async fn get_donation_platforms(&self) -> TestResponse {
        self.test_server
            .get(&"/v2/tag/donation_platform")
            .append_pat(ADMIN_USER_PAT)
            .await
    }

    pub async fn get_donation_platforms_deserialized(&self) -> Vec<DonationPlatformQueryData> {
        let resp = self.get_donation_platforms().await;
        assert_status!(&resp, StatusCode::OK);
        resp.json()
    }
}

#[async_trait(?Send)]
impl ApiTags for ApiV2 {
    async fn get_loaders(&self) -> TestResponse {
        self.test_server
            .get(&"/v2/tag/loader")
            .append_pat(ADMIN_USER_PAT)
            .await
    }

    async fn get_loaders_deserialized_common(&self) -> Vec<CommonLoaderData> {
        let resp = self.get_loaders().await;
        assert_status!(&resp, StatusCode::OK);
        // First, deserialize to the non-common format (to test the response is valid for this api version)
        let v: Vec<LoaderData> = resp.json();
        // Then, deserialize to the common format
        let value = serde_json::to_value(v).unwrap();
        serde_json::from_value(value).unwrap()
    }

    async fn get_categories(&self) -> TestResponse {
        self.test_server
            .get(&"/v2/tag/category")
            .append_pat(ADMIN_USER_PAT)
            .await
    }

    async fn get_categories_deserialized_common(&self) -> Vec<CommonCategoryData> {
        let resp = self.get_categories().await;
        assert_status!(&resp, StatusCode::OK);
        // First, deserialize to the non-common format (to test the response is valid for this api version)
        let v: Vec<CategoryData> = resp.json();
        // Then, deserialize to the common format
        let value = serde_json::to_value(v).unwrap();
        serde_json::from_value(value).unwrap()
    }
}
