use async_trait::async_trait;
use axum_test::http::StatusCode;
use axum_test::TestResponse;
use labrinth::models::{
    notifications::Notification,
    teams::{OrganizationPermissions, ProjectPermissions, TeamMember},
};
use serde_json::json;

use crate::{
    assert_status,
    common::api_common::{
        models::{CommonNotification, CommonTeamMember},
        ApiTeams, AppendsOptionalPat,
    },
};

use super::ApiV3;

impl ApiV3 {
    pub async fn get_organization_members_deserialized(
        &self,
        id_or_title: &str,
        pat: Option<&str>,
    ) -> Vec<TeamMember> {
        let resp = self.get_organization_members(id_or_title, pat).await;
        assert_status!(&resp, StatusCode::OK);
        resp.json()
    }

    pub async fn get_team_members_deserialized(
        &self,
        team_id: &str,
        pat: Option<&str>,
    ) -> Vec<TeamMember> {
        let resp = self.get_team_members(team_id, pat).await;
        assert_status!(&resp, StatusCode::OK);
        resp.json()
    }

    pub async fn get_project_members_deserialized(
        &self,
        project_id: &str,
        pat: Option<&str>,
    ) -> Vec<TeamMember> {
        let resp = self.get_project_members(project_id, pat).await;
        assert_status!(&resp, StatusCode::OK);
        resp.json()
    }
}

#[async_trait(?Send)]
impl ApiTeams for ApiV3 {
    async fn get_team_members(&self, id_or_title: &str, pat: Option<&str>) -> TestResponse {
        self.test_server
            .get(&format!("/v3/team/{id_or_title}/members"))
            .append_pat(pat)
            .await
    }

    async fn get_team_members_deserialized_common(
        &self,
        id_or_title: &str,
        pat: Option<&str>,
    ) -> Vec<CommonTeamMember> {
        let resp = self.get_team_members(id_or_title, pat).await;
        assert_status!(&resp, StatusCode::OK);
        // First, deserialize to the non-common format (to test the response is valid for this api version)
        let v: Vec<TeamMember> = resp.json();
        // Then, deserialize to the common format
        let value = serde_json::to_value(v).unwrap();
        serde_json::from_value(value).unwrap()
    }

    async fn get_teams_members(&self, ids_or_titles: &[&str], pat: Option<&str>) -> TestResponse {
        let ids_or_titles = serde_json::to_string(ids_or_titles).unwrap();
        self.test_server
            .get(
                "/v3/teams/members",
            )
            .add_query_param("ids", &ids_or_titles)
            .append_pat(pat)
            .await
    }

    async fn get_project_members(&self, id_or_title: &str, pat: Option<&str>) -> TestResponse {
        self.test_server
            .get(&format!("/v3/project/{id_or_title}/members"))
            .append_pat(pat)
            .await
    }

    async fn get_project_members_deserialized_common(
        &self,
        id_or_title: &str,
        pat: Option<&str>,
    ) -> Vec<CommonTeamMember> {
        let resp = self.get_project_members(id_or_title, pat).await;
        assert_status!(&resp, StatusCode::OK);
        // First, deserialize to the non-common format (to test the response is valid for this api version)
        let v: Vec<TeamMember> = resp.json();
        // Then, deserialize to the common format
        let value = serde_json::to_value(v).unwrap();
        serde_json::from_value(value).unwrap()
    }

    async fn get_organization_members(&self, id_or_title: &str, pat: Option<&str>) -> TestResponse {
        self.test_server
            .get(&format!("/v3/organization/{id_or_title}/members"))
            .append_pat(pat)
            .await
    }

    async fn get_organization_members_deserialized_common(
        &self,
        id_or_title: &str,
        pat: Option<&str>,
    ) -> Vec<CommonTeamMember> {
        let resp = self.get_organization_members(id_or_title, pat).await;
        assert_status!(&resp, StatusCode::OK);
        // First, deserialize to the non-common format (to test the response is valid for this api version)
        let v: Vec<TeamMember> = resp.json();
        // Then, deserialize to the common format
        let value = serde_json::to_value(v).unwrap();
        serde_json::from_value(value).unwrap()
    }

    async fn join_team(&self, team_id: &str, pat: Option<&str>) -> TestResponse {
        self.test_server
            .post(&format!("/v3/team/{team_id}/join"))
            .append_pat(pat)
            .await
    }

    async fn remove_from_team(
        &self,
        team_id: &str,
        user_id: &str,
        pat: Option<&str>,
    ) -> TestResponse {
        self.test_server
            .delete(&format!("/v3/team/{team_id}/members/{user_id}"))
            .append_pat(pat)
            .await
    }

    async fn edit_team_member(
        &self,
        team_id: &str,
        user_id: &str,
        patch: serde_json::Value,
        pat: Option<&str>,
    ) -> TestResponse {
        self.test_server
            .patch(&format!("/v3/team/{team_id}/members/{user_id}"))
            .append_pat(pat)
            .json(&patch)
            .await
    }

    async fn transfer_team_ownership(
        &self,
        team_id: &str,
        user_id: &str,
        pat: Option<&str>,
    ) -> TestResponse {
        self.test_server
            .patch(&format!("/v3/team/{team_id}/owner"))
            .append_pat(pat)
            .json(&json!({
                "user_id": user_id,
            }))
            .await
    }

    async fn get_user_notifications(&self, user_id: &str, pat: Option<&str>) -> TestResponse {
        self.test_server
            .get(&format!("/v3/user/{user_id}/notifications"))
            .append_pat(pat)
            .await
    }

    async fn get_user_notifications_deserialized_common(
        &self,
        user_id: &str,
        pat: Option<&str>,
    ) -> Vec<CommonNotification> {
        let resp = self.get_user_notifications(user_id, pat).await;
        assert_status!(&resp, StatusCode::OK);
        // First, deserialize to the non-common format (to test the response is valid for this api version)
        let v: Vec<Notification> = resp.json();
        // Then, deserialize to the common format
        let value = serde_json::to_value(v).unwrap();
        serde_json::from_value(value).unwrap()
    }

    async fn get_notification(&self, notification_id: &str, pat: Option<&str>) -> TestResponse {
        self.test_server
            .get(&format!("/v3/notification/{notification_id}"))
            .append_pat(pat)
            .await
    }

    async fn get_notifications(
        &self,
        notification_ids: &[&str],
        pat: Option<&str>,
    ) -> TestResponse {
        let notification_ids = serde_json::to_string(notification_ids).unwrap();
        self.test_server
            .get(
                "/v3/notifications",
            )
            .add_query_param("ids", &notification_ids)
            .append_pat(pat)
            .await
    }

    async fn mark_notification_read(
        &self,
        notification_id: &str,
        pat: Option<&str>,
    ) -> TestResponse {
        self.test_server
            .patch(&format!("/v3/notification/{notification_id}"))
            .append_pat(pat)
            .await
    }

    async fn mark_notifications_read(
        &self,
        notification_ids: &[&str],
        pat: Option<&str>,
    ) -> TestResponse {
        let notification_ids = serde_json::to_string(notification_ids).unwrap();
        self.test_server
            .patch(
                "/v3/notifications",
            )
            .add_query_param("ids", &notification_ids)
            .append_pat(pat)
            .await
    }

    async fn add_user_to_team(
        &self,
        team_id: &str,
        user_id: &str,
        project_permissions: Option<ProjectPermissions>,
        organization_permissions: Option<OrganizationPermissions>,
        pat: Option<&str>,
    ) -> TestResponse {
        self.test_server
            .post(&format!("/v3/team/{team_id}/members"))
            .append_pat(pat)
            .json(&json!({
                "user_id": user_id,
                "permissions" : project_permissions.map(|p| p.bits()).unwrap_or_default(),
                "organization_permissions" : organization_permissions.map(|p| p.bits()),
            }))
            .await
    }

    async fn delete_notification(&self, notification_id: &str, pat: Option<&str>) -> TestResponse {
        self.test_server
            .delete(&format!("/v3/notification/{notification_id}"))
            .append_pat(pat)
            .await
    }

    async fn delete_notifications(
        &self,
        notification_ids: &[&str],
        pat: Option<&str>,
    ) -> TestResponse {
        let notification_ids = serde_json::to_string(notification_ids).unwrap();
        self.test_server
            .delete(
                "/v3/notifications",
            )
            .add_query_param("ids", &notification_ids)
            .append_pat(pat)
            .await
    }
}
