use axum_test::{http::StatusCode, TestResponse};
use bytes::Bytes;
use labrinth::models::{organizations::Organization, users::UserId, v3::projects::Project};
use serde_json::json;

use crate::{
    assert_status,
    common::api_common::{request_data::ImageData, AppendsOptionalPat},
};

use super::ApiV3;

impl ApiV3 {
    pub async fn create_organization(
        &self,
        organization_title: &str,
        organization_slug: &str,
        description: &str,
        pat: Option<&str>,
    ) -> TestResponse {
        self.test_server
            .post(&"/v3/organization")
            .append_pat(pat)
            .json(&json!({
                "name": organization_title,
                "slug": organization_slug,
                "description": description,
            }))
            .await
    }

    pub async fn get_organization(&self, id_or_title: &str, pat: Option<&str>) -> TestResponse {
        self.test_server
            .get(&format!("/v3/organization/{id_or_title}"))
            .append_pat(pat)
            .await
    }

    pub async fn get_organization_deserialized(
        &self,
        id_or_title: &str,
        pat: Option<&str>,
    ) -> Organization {
        let resp = self.get_organization(id_or_title, pat).await;
        assert_status!(&resp, StatusCode::OK);
        resp.json()
    }

    pub async fn get_organizations(
        &self,
        ids_or_titles: &[&str],
        pat: Option<&str>,
    ) -> TestResponse {
        let ids_or_titles = serde_json::to_string(ids_or_titles).unwrap();
        self.test_server
            .get(&format!(
                "/v3/organizations?ids={}",
                urlencoding::encode(&ids_or_titles)
            ))
            .append_pat(pat)
            .await
    }

    pub async fn get_organization_projects(
        &self,
        id_or_title: &str,
        pat: Option<&str>,
    ) -> TestResponse {
        self.test_server
            .get(&format!("/v3/organization/{id_or_title}/projects"))
            .append_pat(pat)
            .await
    }

    pub async fn get_organization_projects_deserialized(
        &self,
        id_or_title: &str,
        pat: Option<&str>,
    ) -> Vec<Project> {
        let resp = self.get_organization_projects(id_or_title, pat).await;
        assert_status!(&resp, StatusCode::OK);
        resp.json()
    }

    pub async fn edit_organization(
        &self,
        id_or_title: &str,
        patch: serde_json::Value,
        pat: Option<&str>,
    ) -> TestResponse {
        self.test_server
            .patch(&format!("/v3/organization/{id_or_title}"))
            .append_pat(pat)
            .json(&patch)
            .await
    }

    pub async fn edit_organization_icon(
        &self,
        id_or_title: &str,
        icon: Option<ImageData>,
        pat: Option<&str>,
    ) -> TestResponse {
        if let Some(icon) = icon {
            // If an icon is provided, upload it
            self.test_server
                .patch(&format!(
                    "/v3/organization/{id_or_title}/icon?ext={ext}",
                    ext = icon.extension
                ))
                .append_pat(pat)
                .bytes(Bytes::from(icon.icon))
                .await
        } else {
            // If no icon is provided, delete the icon
            self.test_server
                .delete(&format!("/v3/organization/{id_or_title}/icon"))
                .append_pat(pat)
                .await
        }
    }

    pub async fn delete_organization(&self, id_or_title: &str, pat: Option<&str>) -> TestResponse {
        self.test_server
            .delete(&format!("/v3/organization/{id_or_title}"))
            .append_pat(pat)
            .await
    }

    pub async fn organization_add_project(
        &self,
        id_or_title: &str,
        project_id_or_slug: &str,
        pat: Option<&str>,
    ) -> TestResponse {
        self.test_server
            .post(&format!("/v3/organization/{id_or_title}/projects"))
            .append_pat(pat)
            .json(&json!({
                "project_id": project_id_or_slug,
            }))
            .await
    }

    pub async fn organization_remove_project(
        &self,
        id_or_title: &str,
        project_id_or_slug: &str,
        new_owner_user_id: UserId,
        pat: Option<&str>,
    ) -> TestResponse {
        self.test_server
            .delete(&format!(
                "/v3/organization/{id_or_title}/projects/{project_id_or_slug}"
            ))
            .append_pat(pat)
            .json(&json!({
                "new_owner": new_owner_user_id,
            }))
            .await
    }
}
