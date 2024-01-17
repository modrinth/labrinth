use std::collections::HashMap;

use crate::{
    assert_status,
    common::{
        api_common::{
            models::{CommonItemType, CommonProject, CommonVersion},
            request_data::{ImageData, ProjectCreationRequestData},
            ApiProject, AppendsOptionalPat,
        },
        dummy_data::TestFile,
    },
};
use async_trait::async_trait;
use axum_test::{http::StatusCode, TestResponse};
use bytes::Bytes;
use labrinth::models::v2::{projects::LegacyProject, search::LegacySearchResults};
use serde_json::json;

use crate::common::database::MOD_USER_PAT;

use super::{
    request_data::{self, get_public_project_creation_data},
    ApiV2,
};

impl ApiV2 {
    pub async fn get_project_deserialized(
        &self,
        id_or_slug: &str,
        pat: Option<&str>,
    ) -> LegacyProject {
        let resp = self.get_project(id_or_slug, pat).await;
        assert_status!(&resp, StatusCode::OK);
        resp.json()
    }

    pub async fn get_user_projects_deserialized(
        &self,
        user_id_or_username: &str,
        pat: Option<&str>,
    ) -> Vec<LegacyProject> {
        let resp = self.get_user_projects(user_id_or_username, pat).await;
        assert_status!(&resp, StatusCode::OK);
        resp.json()
    }

    pub async fn search_deserialized(
        &self,
        query: Option<&str>,
        facets: Option<serde_json::Value>,
        pat: Option<&str>,
    ) -> LegacySearchResults {
        let mut req = self.test_server.get("/v2/search");

        if let Some(query) = query {
            req = req.add_query_param("query", query);
        }

        if let Some(facets) = facets {
            req = req.add_query_param("facets", &facets.to_string());
        }

        let resp = req.append_pat(pat).await;
        assert_status!(&resp, StatusCode::OK);
        resp.json()
    }
}

#[async_trait(?Send)]
impl ApiProject for ApiV2 {
    async fn add_public_project(
        &self,
        slug: &str,
        version_jar: Option<TestFile>,
        modify_json: Option<json_patch::Patch>,
        pat: Option<&str>,
    ) -> (CommonProject, Vec<CommonVersion>) {
        let creation_data = get_public_project_creation_data(slug, version_jar, modify_json);

        // Add a project.
        let slug = creation_data.slug.clone();
        let resp = self.create_project(creation_data, pat).await;
        assert_status!(&resp, StatusCode::OK);

        // Approve as a moderator.
        let resp = self
            .test_server
            .patch(&format!("/v2/project/{slug}"))
            .append_pat(MOD_USER_PAT)
            .json(&json!({
                "status": "approved"
            }))
            .await;
        assert_status!(&resp, StatusCode::NO_CONTENT);

        let project = self.get_project_deserialized_common(&slug, pat).await;

        // Get project's versions
        let resp = self
            .test_server
            .get(&format!("/v2/project/{slug}/version"))
            .append_pat(pat)
            .await;
        let versions: Vec<CommonVersion> = resp.json();
        (project, versions)
    }

    async fn get_public_project_creation_data_json(
        &self,
        slug: &str,
        version_jar: Option<&TestFile>,
    ) -> serde_json::Value {
        request_data::get_public_project_creation_data_json(slug, version_jar)
    }

    async fn create_project(
        &self,
        creation_data: ProjectCreationRequestData,
        pat: Option<&str>,
    ) -> TestResponse {
        self.test_server
            .post("/v2/project")
            .append_pat(pat)
            .multipart(creation_data.multipart_data)
            .await
    }

    async fn remove_project(&self, project_slug_or_id: &str, pat: Option<&str>) -> TestResponse {
        self.test_server
            .delete(&format!("/v2/project/{project_slug_or_id}"))
            .append_pat(pat)
            .await
    }

    async fn get_project(&self, id_or_slug: &str, pat: Option<&str>) -> TestResponse {
        self.test_server
            .get(&format!("/v2/project/{id_or_slug}"))
            .append_pat(pat)
            .await
    }

    async fn get_project_deserialized_common(
        &self,
        id_or_slug: &str,
        pat: Option<&str>,
    ) -> CommonProject {
        let resp = self.get_project(id_or_slug, pat).await;
        assert_status!(&resp, StatusCode::OK);
        // First, deserialize to the non-common format (to test the response is valid for this api version)
        let project: LegacyProject = resp.json();
        // Then, deserialize to the common format
        let value = serde_json::to_value(project).unwrap();
        serde_json::from_value(value).unwrap()
    }

    async fn get_projects(&self, ids_or_slugs: &[&str], pat: Option<&str>) -> TestResponse {
        let ids_or_slugs = serde_json::to_string(ids_or_slugs).unwrap();
        self.test_server
            .get("/v2/projects")
            .add_query_param("ids", &ids_or_slugs)
            .append_pat(pat)
            .await
    }

    async fn get_project_dependencies(&self, id_or_slug: &str, pat: Option<&str>) -> TestResponse {
        self.test_server
            .get(&format!("/v2/project/{id_or_slug}/dependencies"))
            .append_pat(pat)
            .await
    }

    async fn get_user_projects(
        &self,
        user_id_or_username: &str,
        pat: Option<&str>,
    ) -> TestResponse {
        self.test_server
            .get(&format!("/v2/user/{}/projects", user_id_or_username))
            .append_pat(pat)
            .await
    }

    async fn get_user_projects_deserialized_common(
        &self,
        user_id_or_username: &str,
        pat: Option<&str>,
    ) -> Vec<CommonProject> {
        let resp = self.get_user_projects(user_id_or_username, pat).await;
        assert_status!(&resp, StatusCode::OK);
        // First, deserialize to the non-common format (to test the response is valid for this api version)
        let projects: Vec<LegacyProject> = resp.json();
        // Then, deserialize to the common format
        let value = serde_json::to_value(projects).unwrap();
        serde_json::from_value(value).unwrap()
    }

    async fn edit_project(
        &self,
        id_or_slug: &str,
        patch: serde_json::Value,
        pat: Option<&str>,
    ) -> TestResponse {
        self.test_server
            .patch(&format!("/v2/project/{id_or_slug}"))
            .json(&patch)
            .append_pat(pat)
            .await
    }

    async fn edit_project_bulk(
        &self,
        ids_or_slugs: &[&str],
        patch: serde_json::Value,
        pat: Option<&str>,
    ) -> TestResponse {
        let projects_str = ids_or_slugs
            .iter()
            .map(|s| format!("\"{}\"", s))
            .collect::<Vec<_>>()
            .join(",");
        self.test_server
            .patch("/v2/projects")
            .add_query_param("ids", &format!("[{projects_str}]"))
            .json(&patch)
            .append_pat(pat)
            .await
    }

    async fn edit_project_icon(
        &self,
        id_or_slug: &str,
        icon: Option<ImageData>,
        pat: Option<&str>,
    ) -> TestResponse {
        if let Some(icon) = icon {
            // If an icon is provided, upload it
            self.test_server
                .patch(&format!("/v2/project/{id_or_slug}/icon"))
                .add_query_param("ext", &icon.extension)
                .bytes(icon.icon.into())
                .append_pat(pat)
                .await
        } else {
            // If no icon is provided, delete the icon
            self.test_server
                .delete(&format!("/v2/project/{id_or_slug}/icon"))
                .append_pat(pat)
                .await
        }
    }

    async fn create_report(
        &self,
        report_type: &str,
        id: &str,
        item_type: CommonItemType,
        body: &str,
        pat: Option<&str>,
    ) -> TestResponse {
        self.test_server
            .post("/v2/report")
            .json(&json!(
                {
                    "report_type": report_type,
                    "item_id": id,
                    "item_type": item_type.as_str(),
                    "body": body,
                }
            ))
            .append_pat(pat)
            .await
    }

    async fn get_report(&self, id: &str, pat: Option<&str>) -> TestResponse {
        self.test_server
            .get(&format!("/v2/report/{id}"))
            .append_pat(pat)
            .await
    }

    async fn get_reports(&self, ids: &[&str], pat: Option<&str>) -> TestResponse {
        let ids_str = serde_json::to_string(ids).unwrap();
        self.test_server
            .get("/v2/reports")
            .add_query_param("ids", &ids_str)
            .append_pat(pat)
            .await
    }

    async fn get_user_reports(&self, pat: Option<&str>) -> TestResponse {
        self.test_server.get("/v2/report").append_pat(pat).await
    }

    async fn delete_report(&self, id: &str, pat: Option<&str>) -> TestResponse {
        self.test_server
            .delete(&format!("/v2/report/{id}"))
            .append_pat(pat)
            .await
    }

    async fn edit_report(
        &self,
        id: &str,
        patch: serde_json::Value,
        pat: Option<&str>,
    ) -> TestResponse {
        self.test_server
            .patch(&format!("/v2/report/{id}"))
            .json(&patch)
            .append_pat(pat)
            .await
    }

    async fn get_thread(&self, id: &str, pat: Option<&str>) -> TestResponse {
        self.test_server
            .get(&format!("/v2/thread/{id}"))
            .append_pat(pat)
            .await
    }

    async fn get_threads(&self, ids: &[&str], pat: Option<&str>) -> TestResponse {
        let ids_str = serde_json::to_string(ids).unwrap();
        self.test_server
            .get("/v2/threads")
            .add_query_param("ids", &ids_str)
            .append_pat(pat)
            .await
    }

    async fn write_to_thread(
        &self,
        id: &str,
        r#type: &str,
        content: &str,
        pat: Option<&str>,
    ) -> TestResponse {
        self.test_server
            .post(&format!("/v2/thread/{id}"))
            .json(&json!(
                {
                    "body": {
                        "type": r#type,
                        "body": content,
                    }
                }
            ))
            .append_pat(pat)
            .await
    }

    async fn get_moderation_inbox(&self, pat: Option<&str>) -> TestResponse {
        self.test_server
            .get("/v2/thread/inbox")
            .append_pat(pat)
            .await
    }

    async fn read_thread(&self, id: &str, pat: Option<&str>) -> TestResponse {
        self.test_server
            .post(&format!("/v2/thread/{id}/read"))
            .append_pat(pat)
            .await
    }

    async fn delete_thread_message(&self, id: &str, pat: Option<&str>) -> TestResponse {
        self.test_server
            .delete(&format!("/v2/message/{id}"))
            .append_pat(pat)
            .await
    }

    async fn add_gallery_item(
        &self,
        id_or_slug: &str,
        image: ImageData,
        featured: bool,
        title: Option<String>,
        description: Option<String>,
        ordering: Option<i32>,
        pat: Option<&str>,
    ) -> TestResponse {
        let mut req = self
            .test_server
            .post(&format!("/v2/project/{id_or_slug}/gallery",))
            .add_query_param("ext", image.extension)
            .add_query_param("featured", featured);

        if let Some(title) = title {
            req = req.add_query_param("title", title);
        }

        if let Some(description) = description {
            req = req.add_query_param("description", description);
        }

        if let Some(ordering) = ordering {
            req = req.add_query_param("ordering", ordering);
        }

        req.append_pat(pat).bytes(Bytes::from(image.icon)).await
    }

    async fn edit_gallery_item(
        &self,
        id_or_slug: &str,
        image_url: &str,
        patch: HashMap<String, String>,
        pat: Option<&str>,
    ) -> TestResponse {
        let mut req = self
            .test_server
            .patch(&format!("/v2/project/{id_or_slug}/gallery"))
            .add_query_param("url", image_url);

        for (key, value) in patch {
            req = req.add_query_param(&key, &value);
        }

        req.append_pat(pat).await
    }

    async fn remove_gallery_item(
        &self,
        id_or_slug: &str,
        url: &str,
        pat: Option<&str>,
    ) -> TestResponse {
        self.test_server
            .delete(&format!("/v2/project/{id_or_slug}/gallery"))
            .add_query_param("url", url)
            .append_pat(pat)
            .await
    }
}
