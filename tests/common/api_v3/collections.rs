use axum_test::{http::StatusCode, TestResponse};
use bytes::Bytes;
use labrinth::models::{collections::Collection, v3::projects::Project};
use serde_json::json;

use crate::{
    assert_status,
    common::api_common::{request_data::ImageData, AppendsOptionalPat},
};

use super::ApiV3;

impl ApiV3 {
    pub async fn create_collection(
        &self,
        collection_title: &str,
        description: &str,
        projects: &[&str],
        pat: Option<&str>,
    ) -> TestResponse {
        self.test_server
            .post(&"/v3/collection")
            .append_pat(pat)
            .json(&json!({
                "name": collection_title,
                "description": description,
                "projects": projects,
            }))
            .await
    }

    pub async fn get_collection(&self, id: &str, pat: Option<&str>) -> TestResponse {
        self.test_server
            .get(&format!("/v3/collection/{id}"))
            .append_pat(pat)
            .await
    }

    pub async fn get_collection_deserialized(&self, id: &str, pat: Option<&str>) -> Collection {
        let resp = self.get_collection(id, pat).await;
        assert_status!(&resp, StatusCode::OK);
        resp.json()
    }

    pub async fn get_collections(&self, ids: &[&str], pat: Option<&str>) -> TestResponse {
        let ids = serde_json::to_string(ids).unwrap();
        self.test_server
            .get(
                "/v3/collections",
            )
            .add_query_param("ids", &ids)
            .append_pat(pat)
            .await
    }

    pub async fn get_collection_projects(&self, id: &str, pat: Option<&str>) -> TestResponse {
        self.test_server
            .get(&format!("/v3/collection/{id}/projects"))
            .append_pat(pat)
            .await
    }

    pub async fn get_collection_projects_deserialized(
        &self,
        id: &str,
        pat: Option<&str>,
    ) -> Vec<Project> {
        let resp = self.get_collection_projects(id, pat).await;
        assert_status!(&resp, StatusCode::OK);
        resp.json()
    }

    pub async fn edit_collection(
        &self,
        id: &str,
        patch: serde_json::Value,
        pat: Option<&str>,
    ) -> TestResponse {
        self.test_server
            .patch(&format!("/v3/collection/{id}"))
            .append_pat(pat)
            .json(&patch)
            .await
    }

    pub async fn edit_collection_icon(
        &self,
        id: &str,
        icon: Option<ImageData>,
        pat: Option<&str>,
    ) -> TestResponse {
        if let Some(icon) = icon {
            // If an icon is provided, upload it
            self.test_server
                .patch(&format!(
                    "/v3/collection/{id}/icon",
                ))
                .add_query_param("ext", icon.extension)
                .append_pat(pat)
                .bytes(Bytes::from(icon.icon))
                .await
        } else {
            // If no icon is provided, delete the icon
            self.test_server
                .delete(&format!("/v3/collection/{id}/icon"))
                .append_pat(pat)
                .await
        }
    }

    pub async fn delete_collection(&self, id: &str, pat: Option<&str>) -> TestResponse {
        self.test_server
            .delete(&format!("/v3/collection/{id}"))
            .append_pat(pat)
            .await
    }

    pub async fn get_user_collections(
        &self,
        user_id_or_username: &str,
        pat: Option<&str>,
    ) -> TestResponse {
        self.test_server
            .get(&format!("/v3/user/{user_id_or_username}/collections"))
            .append_pat(pat)
            .await
    }

    pub async fn get_user_collections_deserialized_common(
        &self,
        user_id_or_username: &str,
        pat: Option<&str>,
    ) -> Vec<Collection> {
        let resp = self.get_user_collections(user_id_or_username, pat).await;
        assert_status!(&resp, StatusCode::OK);
        // First, deserialize to the non-common format (to test the response is valid for this api version)
        let projects: Vec<Project> = resp.json();
        // Then, deserialize to the common format
        let value = serde_json::to_value(projects).unwrap();
        serde_json::from_value(value).unwrap()
    }
}
