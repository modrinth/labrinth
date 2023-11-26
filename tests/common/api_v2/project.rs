use crate::common::{
    api_common::{
        models::{CommonImageData, CommonProject, CommonVersion},
        request_data::ProjectCreationRequestData,
        Api, ApiProject,
    },
    dummy_data::TestFile,
};
use actix_http::StatusCode;
use actix_web::{
    dev::ServiceResponse,
    test::{self, TestRequest},
};
use async_trait::async_trait;
use bytes::Bytes;
use labrinth::{
    models::v2::projects::LegacyProject, search::SearchResults, util::actix::AppendsMultipart,
};
use serde_json::json;

use crate::common::{asserts::assert_status, database::MOD_USER_PAT};

use super::{
    request_data::{self, get_public_project_creation_data},
    ApiV2,
};

impl ApiV2 {
    pub async fn get_project_deserialized(&self, id_or_slug: &str, pat: &str) -> LegacyProject {
        let resp = self.get_project(id_or_slug, pat).await;
        assert_eq!(resp.status(), 200);
        test::read_body_json(resp).await
    }

    pub async fn get_user_projects_deserialized(
        &self,
        user_id_or_username: &str,
        pat: &str,
    ) -> Vec<LegacyProject> {
        let resp = self.get_user_projects(user_id_or_username, pat).await;
        assert_eq!(resp.status(), 200);
        test::read_body_json(resp).await
    }
}

#[async_trait(?Send)]
impl ApiProject for ApiV2 {
    async fn add_public_project(
        &self,
        slug: &str,
        version_jar: Option<TestFile>,
        modify_json: Option<json_patch::Patch>,
        pat: &str,
    ) -> (CommonProject, Vec<CommonVersion>) {
        let creation_data = get_public_project_creation_data(slug, version_jar, modify_json);

        // Add a project.
        let slug = creation_data.slug.clone();
        let resp = self.create_project(creation_data, pat).await;
        assert_status(&resp, StatusCode::OK);

        // Approve as a moderator.
        let req = TestRequest::patch()
            .uri(&format!("/v2/project/{}", slug))
            .append_header(("Authorization", MOD_USER_PAT))
            .set_json(json!(
                {
                    "status": "approved"
                }
            ))
            .to_request();
        let resp = self.call(req).await;
        assert_status(&resp, StatusCode::NO_CONTENT);

        let project = self.get_project_deserialized_common(&slug, pat).await;

        // Get project's versions
        let req = TestRequest::get()
            .uri(&format!("/v2/project/{}/version", slug))
            .append_header(("Authorization", pat))
            .to_request();
        let resp = self.call(req).await;
        let versions: Vec<CommonVersion> = test::read_body_json(resp).await;

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
        pat: &str,
    ) -> ServiceResponse {
        println!("Creating v2 project");
        let req = TestRequest::post()
            .uri("/v2/project")
            .append_header(("Authorization", pat))
            .set_multipart(creation_data.segment_data)
            .to_request();
        let r = self.call(req).await;
        println!("Response: {:?}", r.response().body());
        r
    }

    async fn remove_project(&self, project_slug_or_id: &str, pat: &str) -> ServiceResponse {
        let req = test::TestRequest::delete()
            .uri(&format!("/v2/project/{project_slug_or_id}"))
            .append_header(("Authorization", pat))
            .to_request();
        let resp = self.call(req).await;
        assert_eq!(resp.status(), 204);
        resp
    }

    async fn get_project(&self, id_or_slug: &str, pat: &str) -> ServiceResponse {
        let req = TestRequest::get()
            .uri(&format!("/v2/project/{id_or_slug}"))
            .append_header(("Authorization", pat))
            .to_request();
        self.call(req).await
    }

    async fn get_project_deserialized_common(&self, id_or_slug: &str, pat: &str) -> CommonProject {
        let resp = self.get_project(id_or_slug, pat).await;
        assert_eq!(resp.status(), 200);
        // First, deserialize to the non-common format (to test the response is valid for this api version)
        let project: LegacyProject = test::read_body_json(resp).await;
        // Then, deserialize to the common format
        let value = serde_json::to_value(project).unwrap();
        serde_json::from_value(value).unwrap()
    }

    async fn get_user_projects(&self, user_id_or_username: &str, pat: &str) -> ServiceResponse {
        let req = test::TestRequest::get()
            .uri(&format!("/v2/user/{}/projects", user_id_or_username))
            .append_header(("Authorization", pat))
            .to_request();
        self.call(req).await
    }

    async fn get_user_projects_deserialized_common(
        &self,
        user_id_or_username: &str,
        pat: &str,
    ) -> Vec<CommonProject> {
        let resp = self.get_user_projects(user_id_or_username, pat).await;
        assert_eq!(resp.status(), 200);
        // First, deserialize to the non-common format (to test the response is valid for this api version)
        let projects: Vec<LegacyProject> = test::read_body_json(resp).await;
        // Then, deserialize to the common format
        let value = serde_json::to_value(projects).unwrap();
        serde_json::from_value(value).unwrap()
    }

    async fn edit_project(
        &self,
        id_or_slug: &str,
        patch: serde_json::Value,
        pat: &str,
    ) -> ServiceResponse {
        let req = test::TestRequest::patch()
            .uri(&format!("/v2/project/{id_or_slug}"))
            .append_header(("Authorization", pat))
            .set_json(patch)
            .to_request();

        self.call(req).await
    }

    async fn edit_project_bulk(
        &self,
        ids_or_slugs: &[&str],
        patch: serde_json::Value,
        pat: &str,
    ) -> ServiceResponse {
        let projects_str = ids_or_slugs
            .iter()
            .map(|s| format!("\"{}\"", s))
            .collect::<Vec<_>>()
            .join(",");
        let req = test::TestRequest::patch()
            .uri(&format!(
                "/v2/projects?ids={encoded}",
                encoded = urlencoding::encode(&format!("[{projects_str}]"))
            ))
            .append_header(("Authorization", pat))
            .set_json(patch)
            .to_request();

        self.call(req).await
    }

    async fn edit_project_icon(
        &self,
        id_or_slug: &str,
        icon: Option<CommonImageData>,
        pat: &str,
    ) -> ServiceResponse {
        if let Some(icon) = icon {
            // If an icon is provided, upload it
            let req = test::TestRequest::patch()
                .uri(&format!(
                    "/v2/project/{id_or_slug}/icon?ext={ext}",
                    ext = icon.extension
                ))
                .append_header(("Authorization", pat))
                .set_payload(Bytes::from(icon.icon))
                .to_request();

            self.call(req).await
        } else {
            // If no icon is provided, delete the icon
            let req = test::TestRequest::delete()
                .uri(&format!("/v2/project/{id_or_slug}/icon"))
                .append_header(("Authorization", pat))
                .to_request();

            self.call(req).await
        }
    }

    async fn search_deserialized_common(
        &self,
        query: Option<&str>,
        facets: Option<serde_json::Value>,
        pat: &str,
    ) -> SearchResults {
        let query_field = if let Some(query) = query {
            format!("&query={}", urlencoding::encode(query))
        } else {
            "".to_string()
        };

        let facets_field = if let Some(facets) = facets {
            format!("&facets={}", urlencoding::encode(&facets.to_string()))
        } else {
            "".to_string()
        };

        let req = test::TestRequest::get()
            .uri(&format!("/v2/search?{}{}", query_field, facets_field))
            .append_header(("Authorization", pat))
            .to_request();
        let resp = self.call(req).await;
        let status = resp.status();
        println!("Body: {:?}", resp.response().body());
        assert_eq!(status, 200);
        test::read_body_json(resp).await
    }
}
