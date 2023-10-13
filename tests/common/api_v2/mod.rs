#![allow(dead_code)]

use super::{
    actix::AppendsMultipart,
    asserts::assert_status,
    database::{MOD_USER_PAT, USER_USER_PAT},
    environment::LocalService,
    request_data::{ProjectCreationRequestData, ImageData},
};
use actix_http::StatusCode;
use actix_web::{
    dev::ServiceResponse,
    test::{self, TestRequest},
};
use bytes::Bytes;
use labrinth::models::{
    notifications::Notification,
    projects::{Project, Version}, organizations::Organization, teams::{TeamMember, ProjectPermissions, OrganizationPermissions},
};
use serde_json::json;
use std::rc::Rc;


#[derive(Clone)]
pub struct ApiV2 {
    pub test_app: Rc<dyn LocalService>,
}

impl ApiV2 {
    pub async fn call(&self, req: actix_http::Request) -> ServiceResponse {
        self.test_app.call(req).await.unwrap()
    }

    pub async fn add_public_project(
        &self,
        creation_data: ProjectCreationRequestData,
        pat: &str
    ) -> (Project, Vec<Version>) {
        // Add a project.
        let req = TestRequest::post()
            .uri("/v2/project")
            .append_header(("Authorization", pat))
            .set_multipart(creation_data.segment_data)
            .to_request();
        let resp = self.call(req).await;
        assert_status(resp, StatusCode::OK);

        // Approve as a moderator.
        let req = TestRequest::patch()
            .uri(&format!("/v2/project/{}", creation_data.slug))
            .append_header(("Authorization", MOD_USER_PAT))
            .set_json(json!(
                {
                    "status": "approved"
                }
            ))
            .to_request();
        let resp = self.call(req).await;
        assert_status(resp, StatusCode::NO_CONTENT);

        let project = self
            .get_project_deserialized(&creation_data.slug, pat)
            .await;

        // Get project's versions
        let req = TestRequest::get()
            .uri(&format!("/v2/project/{}/version", creation_data.slug))
            .append_header(("Authorization", pat))
            .to_request();
        let resp = self.call(req).await;
        let versions: Vec<Version> = test::read_body_json(resp).await;

        (project, versions)
    }

    pub async fn remove_project(&self, project_slug_or_id: &str, pat: &str) -> ServiceResponse {
        let req = test::TestRequest::delete()
            .uri(&format!("/v2/project/{project_slug_or_id}"))
            .append_header(("Authorization", pat))
            .to_request();
        let resp = self.call(req).await;
        assert_eq!(resp.status(), 204);
        resp
    }

    pub async fn get_project(&self, id_or_slug: &str, pat: &str) -> ServiceResponse {
        let req = TestRequest::get()
            .uri(&format!("/v2/project/{id_or_slug}"))
            .append_header(("Authorization", pat))
            .to_request();
        self.call(req).await

    }
    pub async fn get_project_deserialized(&self, id_or_slug: &str, pat: &str) -> Project {
        let resp = self.get_project(id_or_slug, pat).await;
        assert_eq!(resp.status(), 200);
        test::read_body_json(resp).await
    }

    pub async fn get_user_projects_deserialized(
        &self,
        user_id_or_username: &str,
        pat: &str,
    ) -> Vec<Project> {
        let req = test::TestRequest::get()
            .uri(&format!("/v2/user/{}/projects", user_id_or_username))
            .append_header(("Authorization", pat))
            .to_request();
        let resp = self.call(req).await;
        assert_eq!(resp.status(), 200);
        test::read_body_json(resp).await
    }

    pub async fn get_version_from_hash(
        &self,
        hash: &str,
        algorithm: &str,
        pat: &str,
    ) -> ServiceResponse {
        let req = test::TestRequest::get()
            .uri(&format!("/v2/version_file/{hash}?algorithm={algorithm}"))
            .append_header(("Authorization", pat))
            .to_request();
        self.call(req).await
    }

    pub async fn get_version_from_hash_deserialized(
        &self,
        hash: &str,
        algorithm: &str,
        pat: &str,
    ) -> Version {
        let resp = self.get_version_from_hash(hash, algorithm, pat).await;
        assert_eq!(resp.status(), 200);
        test::read_body_json(resp).await
    }

    pub async fn add_user_to_team(
        &self,
        team_id: &str,
        user_id: &str,
        project_permissions: Option<ProjectPermissions>,
        organization_permissions: Option<OrganizationPermissions>,
        pat: &str,
    ) -> ServiceResponse {
        let req = test::TestRequest::post()
            .uri(&format!("/v2/team/{team_id}/members"))
            .append_header(("Authorization", pat))
            .set_json(json!( {
                "user_id": user_id,
                "permissions" : project_permissions.map(|p| p.bits()).unwrap_or_default(),
                "organization_permissions" : organization_permissions.map(|p| p.bits()),
            }))
            .to_request();
        self.call(req).await
    }


    pub async fn edit_project(&self, id_or_slug : &str, patch : serde_json::Value, pat: &str) -> ServiceResponse {
        let req = test::TestRequest::patch()
        .uri(&format!("/v2/project/{id_or_slug}"))
        .append_header(("Authorization", pat))
        .set_json(patch)
        .to_request();
        
        self.call(req).await
    }

    pub async fn edit_project_bulk(&self, ids_or_slugs : impl IntoIterator<Item = &str>, patch : serde_json::Value, pat: &str) -> ServiceResponse {
        // "/v2/projects?ids={}",
        // urlencoding::encode(&format!("[\"{alpha_project_id}\",\"{beta_project_id}\"]"))

        let projects_str = ids_or_slugs.into_iter().map(|s| format!("\"{}\"", s)).collect::<Vec<_>>().join(",");
        let req = test::TestRequest::patch()
        .uri(&format!("/v2/projects?ids={encoded}", encoded = urlencoding::encode(&format!("[{projects_str}]"))))
        .append_header(("Authorization", pat))
        .set_json(patch)
        .to_request();
        
        self.call(req).await
    }

    pub async fn edit_project_icon(&self, id_or_slug : &str, icon: Option<ImageData>, pat: &str) -> ServiceResponse {
        if let Some(icon) = icon {
            // If an icon is provided, upload it
            let req = test::TestRequest::patch()
            .uri(&format!("/v2/project/{id_or_slug}/icon?ext={ext}", ext = icon.extension))
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


    pub async fn join_team(&self, team_id: &str, pat: &str) -> ServiceResponse {
        let req = test::TestRequest::post()
            .uri(&format!("/v2/team/{team_id}/join"))
            .append_header(("Authorization", pat))
            .to_request();
        self.call(req).await
    }

    pub async fn remove_from_team(
        &self,
        team_id: &str,
        user_id: &str,
        pat: &str,
    ) -> ServiceResponse {
        let req = test::TestRequest::delete()
            .uri(&format!("/v2/team/{team_id}/members/{user_id}"))
            .append_header(("Authorization", pat))
            .to_request();
        self.call(req).await
    }

    pub async fn edit_team_member(
        &self,
        team_id: &str,
        user_id: &str,
        patch: serde_json::Value,
        pat: &str,
    ) -> ServiceResponse {
        let req = test::TestRequest::patch()
            .uri(&format!("/v2/team/{team_id}/members/{user_id}"))
            .append_header(("Authorization", pat))
            .set_json(patch)
            .to_request();
        self.call(req).await
    }

    pub async fn transfer_team_ownership(
        &self,
        team_id: &str,
        user_id: &str,
        pat: &str,
    ) -> ServiceResponse {
        let req = test::TestRequest::patch()
            .uri(&format!("/v2/team/{team_id}/owner"))
            .append_header(("Authorization", pat))
            .set_json(json!({
                "user_id": user_id,
            }))
            .to_request();
        self.call(req).await
    }
    
    pub async fn get_user_notifications_deserialized(
        &self,
        user_id: &str,
        pat: &str,
    ) -> Vec<Notification> {
        let req = test::TestRequest::get()
            .uri(&format!("/v2/user/{user_id}/notifications"))
            .append_header(("Authorization", pat))
            .to_request();
        let resp = self.call(req).await;
        test::read_body_json(resp).await
    }

    pub async fn mark_notification_read(
        &self,
        notification_id: &str,
        pat: &str,
    ) -> ServiceResponse {
        let req = test::TestRequest::patch()
            .uri(&format!("/v2/notification/{notification_id}"))
            .append_header(("Authorization", pat))
            .to_request();
        self.call(req).await
    }

    pub async fn delete_notification(&self, notification_id: &str, pat: &str) -> ServiceResponse {
        let req = test::TestRequest::delete()
            .uri(&format!("/v2/notification/{notification_id}"))
            .append_header(("Authorization", pat))
            .to_request();
        self.call(req).await
    }

    pub async fn create_organization(&self, organization_title: &str, description : &str, pat: &str) -> ServiceResponse {
        let req = test::TestRequest::post()
        .uri(&format!("/v2/organization"))
        .append_header(("Authorization", pat))
        .set_json(json!({
            "title": organization_title,
            "description": description,
        }))
        .to_request();
        self.call(req).await
    }

    pub async fn get_organization(&self, id_or_title : &str, pat: &str) -> ServiceResponse {
        let req = TestRequest::get()
        .uri(&format!("/v2/organization/{id_or_title}"))
        .append_header(("Authorization", pat))
        .to_request();
        self.call(req).await
    }

    pub async fn get_organization_deserialized(&self, id_or_title : &str, pat: &str) -> Organization{
        let resp = self.get_organization(id_or_title, pat).await;
        assert_eq!(resp.status(), 200);
        test::read_body_json(resp).await
    }

    pub async fn get_organization_projects(&self, id_or_title : &str, pat: &str) -> ServiceResponse {
        let req = test::TestRequest::get()
        .uri(&format!("/v2/organization/{id_or_title}/projects"))
        .append_header(("Authorization", pat))
        .to_request();
        self.call(req).await
    }

    pub async fn get_organization_projects_deserialized(&self, id_or_title : &str, pat: &str) -> Vec<Project> {
        let resp = self.get_organization_projects(id_or_title, pat).await;
        assert_eq!(resp.status(), 200);
        test::read_body_json(resp).await
    }

    pub async fn get_team_members(&self, id_or_title : &str, pat: &str) -> ServiceResponse {
        let req = test::TestRequest::get()
        .uri(&format!("/v2/team/{id_or_title}/members"))
        .append_header(("Authorization", pat))
        .to_request();
        self.call(req).await
    }

    pub async fn get_team_members_deserialized(&self, id_or_title : &str, pat: &str) -> Vec<TeamMember> {
        let resp = self.get_team_members(id_or_title, pat).await;
        assert_eq!(resp.status(), 200);
        test::read_body_json(resp).await
    }

    pub async fn get_organization_members(&self, id_or_title : &str, pat: &str) -> ServiceResponse {
        let req = test::TestRequest::get()
        .uri(&format!("/v2/organization/{id_or_title}/members"))
        .append_header(("Authorization", pat))
        .to_request();
        self.call(req).await
    }

    pub async fn get_project_members_deserialized(&self, id_or_title : &str, pat: &str) -> Vec<TeamMember> {
        let resp = self.get_project_members(id_or_title, pat).await;
        assert_eq!(resp.status(), 200);
        test::read_body_json(resp).await
    }

    pub async fn get_project_members(&self, id_or_title : &str, pat: &str) -> ServiceResponse {
        let req = test::TestRequest::get()
        .uri(&format!("/v2/project/{id_or_title}/members"))
        .append_header(("Authorization", pat))
        .to_request();
        self.call(req).await
    }

    pub async fn get_organization_members_deserialized(&self, id_or_title : &str, pat: &str) -> Vec<TeamMember> {
        let resp = self.get_organization_members(id_or_title, pat).await;
        assert_eq!(resp.status(), 200);
        test::read_body_json(resp).await
    }


    pub async fn edit_organization(&self, id_or_title : &str, patch : serde_json::Value, pat: &str) -> ServiceResponse {
        let req = test::TestRequest::patch()
        .uri(&format!("/v2/organization/{id_or_title}"))
        .append_header(("Authorization", pat))
        .set_json(patch)
        .to_request();
        
        self.call(req).await
    }

    pub async fn edit_organization_icon(&self, id_or_title : &str, icon: Option<ImageData>, pat: &str) -> ServiceResponse {
        if let Some(icon) = icon {
            // If an icon is provided, upload it
            let req = test::TestRequest::patch()
            .uri(&format!("/v2/organization/{id_or_title}/icon?ext={ext}", ext = icon.extension))
            .append_header(("Authorization", pat))
            .set_payload(Bytes::from(icon.icon))
            .to_request();
            
            self.call(req).await
        } else {
            // If no icon is provided, delete the icon
            let req = test::TestRequest::delete()
            .uri(&format!("/v2/organization/{id_or_title}/icon"))
            .append_header(("Authorization", pat))
            .to_request();
            
            self.call(req).await
        }
    }

    pub async fn delete_organization(&self, id_or_title : &str, pat: &str) -> ServiceResponse {
        let req = test::TestRequest::delete()
        .uri(&format!("/v2/organization/{id_or_title}"))
        .append_header(("Authorization", pat))
        .to_request();
        
        self.call(req).await
    }

    pub async fn organization_add_project(&self, id_or_title : &str, project_id_or_slug : &str, pat: &str) -> ServiceResponse {
        let req = test::TestRequest::post()
        .uri(&format!("/v2/organization/{id_or_title}/projects"))
        .append_header(("Authorization", pat))
        .set_json(json!({
            "project_id": project_id_or_slug,
        }))
        .to_request();
        
        self.call(req).await
    }

    pub async fn organization_remove_project(&self, id_or_title : &str, project_id_or_slug : &str, pat: &str) -> ServiceResponse {
        let req = test::TestRequest::delete()
        .uri(&format!("/v2/organization/{id_or_title}/projects/{project_id_or_slug}"))
        .append_header(("Authorization", pat))
        .to_request();
        
        self.call(req).await
    }

}
