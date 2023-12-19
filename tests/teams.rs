use crate::common::{
    api_common::{ApiTeams, AppendsOptionalPat},
    database::*,
};
use actix_web::test;
use common::{
    api_v3::ApiV3,
    environment::{with_test_environment, with_test_environment_all, TestEnvironment},
};
use labrinth::models::teams::{OrganizationPermissions, ProjectPermissions};
use rust_decimal::Decimal;
use serde_json::json;

mod common;

#[actix_rt::test]
async fn test_get_team() {
    // Test setup and dummy data
    with_test_environment_all(None, |test_env| async move {
        let alpha_project_id = &test_env.dummy.as_ref().unwrap().project_alpha.project_id;
        let alpha_team_id = &test_env.dummy.as_ref().unwrap().project_alpha.team_id;
        let zeta_organization_id = &test_env
            .dummy
            .as_ref()
            .unwrap()
            .organization_zeta
            .organization_id;
        let zeta_team_id = &test_env.dummy.as_ref().unwrap().organization_zeta.team_id;

        // Perform tests for an organization team and a project team
        for (team_association_id, team_association, team_id) in [
            (alpha_project_id, "project", alpha_team_id),
            (zeta_organization_id, "organization", zeta_team_id),
        ] {
            // A non-member of the team should get basic info but not be able to see private data
            for uri in [
                format!("/v3/team/{team_id}/members"),
                format!("/v3/{team_association}/{team_association_id}/members"),
            ] {
                let req = test::TestRequest::get()
                    .uri(&uri)
                    .append_pat(FRIEND_USER_PAT)
                    .to_request();

                let resp = test_env.call(req).await;
                assert_eq!(resp.status(), 200);
                let value: serde_json::Value = test::read_body_json(resp).await;
                assert_eq!(value[0]["user"]["id"], USER_USER_ID);
                assert!(value[0]["permissions"].is_null());
            }

            // A non-accepted member of the team should:
            // - not be able to see private data about the team, but see all members including themselves
            // - should not appear in the team members list to enemy users
            let req = test::TestRequest::post()
                .uri(&format!("/v3/team/{team_id}/members"))
                .append_pat(USER_USER_PAT)
                .set_json(&json!({
                    "user_id": FRIEND_USER_ID,
                }))
                .to_request();
            let resp = test_env.call(req).await;
            assert_eq!(resp.status(), 204);

            for uri in [
                format!("/v3/team/{team_id}/members"),
                format!("/v3/{team_association}/{team_association_id}/members"),
            ] {
                let req = test::TestRequest::get()
                    .uri(&uri)
                    .append_pat(FRIEND_USER_PAT)
                    .to_request();
                let resp = test_env.call(req).await;
                assert_eq!(resp.status(), 200);
                let value: serde_json::Value = test::read_body_json(resp).await;
                let members = value.as_array().unwrap();
                assert!(members.len() == 2); // USER_USER_ID and FRIEND_USER_ID should be in the team
                let user_user = members
                    .iter()
                    .find(|x| x["user"]["id"] == USER_USER_ID)
                    .unwrap();
                let friend_user = members
                    .iter()
                    .find(|x| x["user"]["id"] == FRIEND_USER_ID)
                    .unwrap();
                assert_eq!(user_user["user"]["id"], USER_USER_ID);
                assert!(user_user["permissions"].is_null()); // Should not see private data of the team
                assert_eq!(friend_user["user"]["id"], FRIEND_USER_ID);
                assert!(friend_user["permissions"].is_null());

                let req = test::TestRequest::get()
                    .uri(&uri)
                    .append_pat(ENEMY_USER_PAT)
                    .to_request();
                let resp = test_env.call(req).await;
                assert_eq!(resp.status(), 200);
                let value: serde_json::Value = test::read_body_json(resp).await;
                let members = value.as_array().unwrap();
                assert_eq!(members.len(), 1); // Only USER_USER_ID should be in the team
                assert_eq!(members[0]["user"]["id"], USER_USER_ID);
                assert!(members[0]["permissions"].is_null());
            }
            // An accepted member of the team should appear in the team members list
            // and should be able to see private data about the team
            let req = test::TestRequest::post()
                .uri(&format!("/v3/team/{team_id}/join"))
                .append_pat(FRIEND_USER_PAT)
                .to_request();
            let resp = test_env.call(req).await;
            assert_eq!(resp.status(), 204);

            for uri in [
                format!("/v3/team/{team_id}/members"),
                format!("/v3/{team_association}/{team_association_id}/members"),
            ] {
                let req = test::TestRequest::get()
                    .uri(&uri)
                    .append_pat(FRIEND_USER_PAT)
                    .to_request();
                let resp = test_env.call(req).await;
                assert_eq!(resp.status(), 200);
                let value: serde_json::Value = test::read_body_json(resp).await;
                let members = value.as_array().unwrap();
                assert!(members.len() == 2); // USER_USER_ID and FRIEND_USER_ID should be in the team
                let user_user = members
                    .iter()
                    .find(|x| x["user"]["id"] == USER_USER_ID)
                    .unwrap();
                let friend_user = members
                    .iter()
                    .find(|x| x["user"]["id"] == FRIEND_USER_ID)
                    .unwrap();
                assert_eq!(user_user["user"]["id"], USER_USER_ID);
                assert!(!user_user["permissions"].is_null()); // SHOULD see private data of the team
                assert_eq!(friend_user["user"]["id"], FRIEND_USER_ID);
                assert!(!friend_user["permissions"].is_null());
            }
        }
    })
    .await;
}

#[actix_rt::test]
async fn test_get_team_project_orgs() {
    // Test setup and dummy data
    with_test_environment_all(None, |test_env| async move {
        let alpha_project_id = &test_env.dummy.as_ref().unwrap().project_alpha.project_id;
        let alpha_team_id = &test_env.dummy.as_ref().unwrap().project_alpha.team_id;
        let zeta_organization_id = &test_env
            .dummy
            .as_ref()
            .unwrap()
            .organization_zeta
            .organization_id;
        let zeta_team_id = &test_env.dummy.as_ref().unwrap().organization_zeta.team_id;

        // Attach alpha to zeta
        let req = test::TestRequest::post()
            .uri(&format!("/v3/organization/{zeta_organization_id}/projects"))
            .append_pat(USER_USER_PAT)
            .set_json(json!({
                "project_id": alpha_project_id,
            }))
            .to_request();
        let resp = test_env.call(req).await;
        assert_eq!(resp.status(), 204);

        // Invite and add friend to zeta
        let req = test::TestRequest::post()
            .uri(&format!("/v3/team/{zeta_team_id}/members"))
            .append_pat(USER_USER_PAT)
            .set_json(json!({
                "user_id": FRIEND_USER_ID,
            }))
            .to_request();
        let resp = test_env.call(req).await;
        assert_eq!(resp.status(), 204);

        let req = test::TestRequest::post()
            .uri(&format!("/v3/team/{zeta_team_id}/join"))
            .append_pat(FRIEND_USER_PAT)
            .to_request();
        let resp = test_env.call(req).await;
        assert_eq!(resp.status(), 204);

        // The team members route from teams (on a project's team):
        // - the members of the project team specifically
        // - not the ones from the organization
        let req = test::TestRequest::get()
            .uri(&format!("/v3/team/{alpha_team_id}/members"))
            .append_pat(FRIEND_USER_PAT)
            .to_request();
        let resp = test_env.call(req).await;
        assert_eq!(resp.status(), 200);
        let value: serde_json::Value = test::read_body_json(resp).await;
        let members = value.as_array().unwrap();
        assert_eq!(members.len(), 1);

        // The team members route from project should show:
        // - the members of the project team including the ones from the organization
        let req = test::TestRequest::get()
            .uri(&format!("/v3/project/{alpha_project_id}/members"))
            .append_pat(FRIEND_USER_PAT)
            .to_request();
        let resp = test_env.call(req).await;
        assert_eq!(resp.status(), 200);
        let value: serde_json::Value = test::read_body_json(resp).await;
        let members = value.as_array().unwrap();
        assert_eq!(members.len(), 2);
    })
    .await;
}

// edit team member (Varying permissions, varying roles)
#[actix_rt::test]
async fn test_patch_project_team_member() {
    // Test setup and dummy data
    with_test_environment_all(None, |test_env| async move {
        let api = &test_env.api;

        let alpha_team_id = &test_env.dummy.as_ref().unwrap().project_alpha.team_id;

        // Edit team as admin/mod but not a part of the team should be OK
        let resp = api.edit_team_member(alpha_team_id, USER_USER_ID, json!({}), ADMIN_USER_PAT).await;
        assert_eq!(resp.status(), 204);

        // As a non-owner with full permissions, attempt to edit the owner's permissions
        let resp = api.edit_team_member(alpha_team_id, USER_USER_ID, json!({
            "permissions": 0
        }), ADMIN_USER_PAT).await;
        assert_eq!(resp.status(), 400);

        // Should not be able to edit organization permissions of a project team
        let resp = api.edit_team_member(alpha_team_id, USER_USER_ID, json!({
            "organization_permissions": 0
        }), USER_USER_PAT).await;
        assert_eq!(resp.status(), 400);

        // Should not be able to add permissions to a user that the adding-user does not have
        // (true for both project and org)

        // first, invite friend
        let resp = api.add_user_to_team(alpha_team_id, FRIEND_USER_ID,
            Some(ProjectPermissions::EDIT_MEMBER | ProjectPermissions::EDIT_BODY),
            None, USER_USER_PAT).await;
        assert_eq!(resp.status(), 204);

        // accept
        let resp = api.join_team(alpha_team_id, FRIEND_USER_PAT).await;
        assert_eq!(resp.status(), 204);

        // try to add permissions
        let resp = api.edit_team_member(alpha_team_id, FRIEND_USER_ID, json!({
            "permissions": (ProjectPermissions::EDIT_MEMBER | ProjectPermissions::EDIT_DETAILS).bits()
        }), FRIEND_USER_PAT).await; // should this be friend_user_pat
        assert_eq!(resp.status(), 400);

        // Cannot set payouts outside of 0 and 5000
        for payout in [-1, 5001] {
            let resp = api.edit_team_member(alpha_team_id, FRIEND_USER_ID, json!({
                "payouts_split": payout
            }), USER_USER_PAT).await;
            assert_eq!(resp.status(), 400);
        }

        // Successful patch
        let resp = api.edit_team_member(alpha_team_id, FRIEND_USER_ID, json!({
                "payouts_split": 51,
                "permissions": ProjectPermissions::EDIT_MEMBER.bits(), // reduces permissions
                "role": "membe2r",
                "ordering": 5
        }), FRIEND_USER_PAT).await;
        assert_eq!(resp.status(), 204);

        // Check results
        let members = api.get_team_members_deserialized_common(alpha_team_id, FRIEND_USER_PAT).await;
        let member = members.iter().find(|x| x.user.id.0 == FRIEND_USER_ID_PARSED as u64).unwrap();
        assert_eq!(member.payouts_split, Decimal::from_f64_retain(51.0));
        assert_eq!(member.permissions.unwrap(), ProjectPermissions::EDIT_MEMBER);
        assert_eq!(member.role, "membe2r");
        assert_eq!(member.ordering, 5);
    }).await;
}

// edit team member (Varying permissions, varying roles)
#[actix_rt::test]
async fn test_patch_organization_team_member() {
    // Test setup and dummy data
    with_test_environment_all(None, |test_env| async move {
        let zeta_team_id = &test_env.dummy.as_ref().unwrap().organization_zeta.team_id;

        // Edit team as admin/mod but not a part of the team should be OK
        let req = test::TestRequest::patch()
            .uri(&format!("/v3/team/{zeta_team_id}/members/{USER_USER_ID}"))
            .set_json(json!({}))
            .append_pat(ADMIN_USER_PAT)
            .to_request();
        let resp = test_env.call(req).await;
        assert_eq!(resp.status(), 204);

        // As a non-owner with full permissions, attempt to edit the owner's permissions
        let req = test::TestRequest::patch()
            .uri(&format!("/v3/team/{zeta_team_id}/members/{USER_USER_ID}"))
            .append_pat(ADMIN_USER_PAT)
            .set_json(json!({
                "permissions": 0
            }))
            .to_request();
        let resp = test_env.call(req).await;
        assert_eq!(resp.status(), 400);

        // Should not be able to add permissions to a user that the adding-user does not have
        // (true for both project and org)

        // first, invite friend
        let req = test::TestRequest::post()
            .uri(&format!("/v3/team/{zeta_team_id}/members"))
            .append_pat(USER_USER_PAT)
            .set_json(json!({
                "user_id": FRIEND_USER_ID,
                "organization_permissions": (OrganizationPermissions::EDIT_MEMBER | OrganizationPermissions::EDIT_MEMBER_DEFAULT_PERMISSIONS).bits(),
            })).to_request();
        let resp = test_env.call(req).await;
        assert_eq!(resp.status(), 204);

        // accept
        let req = test::TestRequest::post()
            .uri(&format!("/v3/team/{zeta_team_id}/join"))
            .append_pat(FRIEND_USER_PAT)
            .to_request();
        let resp = test_env.call(req).await;
        assert_eq!(resp.status(), 204);

        // try to add permissions- fails, as we do not have EDIT_DETAILS
        let req = test::TestRequest::patch()
        .uri(&format!("/v3/team/{zeta_team_id}/members/{FRIEND_USER_ID}"))
        .append_pat(FRIEND_USER_PAT)
            .set_json(json!({
                "organization_permissions": (OrganizationPermissions::EDIT_MEMBER | OrganizationPermissions::EDIT_DETAILS).bits()
            }))
            .to_request();
        let resp = test_env.call(req).await;

        assert_eq!(resp.status(), 400);

        // Cannot set payouts outside of 0 and 5000
        for payout in [-1, 5001] {
            let req = test::TestRequest::patch()
                .uri(&format!("/v3/team/{zeta_team_id}/members/{FRIEND_USER_ID}"))
                .append_pat(USER_USER_PAT)
                .set_json(json!({
                    "payouts_split": payout
                }))
                .to_request();
            let resp = test_env.call(req).await;
            assert_eq!(resp.status(), 400);
        }

        // Successful patch
        let req = test::TestRequest::patch()
            .uri(&format!("/v3/team/{zeta_team_id}/members/{FRIEND_USER_ID}"))
            .append_pat(FRIEND_USER_PAT)
            .set_json(json!({
                "payouts_split": 51,
                "organization_permissions": (OrganizationPermissions::EDIT_MEMBER).bits(), // reduces permissions
                "permissions": (ProjectPermissions::EDIT_MEMBER).bits(),
                "role": "very-cool-member",
                "ordering": 5
            }))
            .to_request();
        let resp = test_env.call(req).await;

        assert_eq!(resp.status(), 204);

        // Check results
        let req = test::TestRequest::get()
            .uri(&format!("/v3/team/{zeta_team_id}/members"))
            .append_pat(FRIEND_USER_PAT)
            .to_request();
        let resp = test_env.call(req).await;
        assert_eq!(resp.status(), 200);
        let value: serde_json::Value = test::read_body_json(resp).await;
        let member = value
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["user"]["id"] == FRIEND_USER_ID)
            .unwrap();
        assert_eq!(member["payouts_split"], 51.0);
        assert_eq!(
            member["organization_permissions"],
            OrganizationPermissions::EDIT_MEMBER.bits()
        );
        assert_eq!(
            member["permissions"],
            ProjectPermissions::EDIT_MEMBER.bits()
        );
        assert_eq!(member["role"], "very-cool-member");
        assert_eq!(member["ordering"], 5);

    }).await;
}

// trasnfer ownership (requires being owner, etc)
#[actix_rt::test]
async fn transfer_ownership_v3() {
    // Test setup and dummy data
    with_test_environment(None, |test_env: TestEnvironment<ApiV3>| async move {
        let api = &test_env.api;

        let alpha_team_id = &test_env.dummy.as_ref().unwrap().project_alpha.team_id;

        // Cannot set friend as owner (not a member)
        let resp = api
            .transfer_team_ownership(alpha_team_id, FRIEND_USER_ID, USER_USER_PAT)
            .await;
        assert_eq!(resp.status(), 400);
        let resp = api
            .transfer_team_ownership(alpha_team_id, FRIEND_USER_ID, FRIEND_USER_PAT)
            .await;
        assert_eq!(resp.status(), 401);

        // first, invite friend
        let resp = api
            .add_user_to_team(alpha_team_id, FRIEND_USER_ID, None, None, USER_USER_PAT)
            .await;
        assert_eq!(resp.status(), 204);

        // still cannot set friend as owner (not accepted)
        let resp = api
            .transfer_team_ownership(alpha_team_id, FRIEND_USER_ID, USER_USER_PAT)
            .await;
        assert_eq!(resp.status(), 400);

        // accept
        let resp = api.join_team(alpha_team_id, FRIEND_USER_PAT).await;
        assert_eq!(resp.status(), 204);

        // Cannot set ourselves as owner if we are not owner
        let resp = api
            .transfer_team_ownership(alpha_team_id, FRIEND_USER_ID, FRIEND_USER_PAT)
            .await;
        assert_eq!(resp.status(), 401);

        // Can set friend as owner
        let resp = api
            .transfer_team_ownership(alpha_team_id, FRIEND_USER_ID, USER_USER_PAT)
            .await;
        assert_eq!(resp.status(), 204);

        // Check
        let members = api
            .get_team_members_deserialized(alpha_team_id, USER_USER_PAT)
            .await;
        let friend_member = members
            .iter()
            .find(|x| x.user.id.0 == FRIEND_USER_ID_PARSED as u64)
            .unwrap();
        assert_eq!(friend_member.role, "Member"); // her role does not actually change, but is_owner is set to true
        assert!(friend_member.is_owner);
        assert_eq!(
            friend_member.permissions.unwrap(),
            ProjectPermissions::all()
        );

        let user_member = members
            .iter()
            .find(|x| x.user.id.0 == USER_USER_ID_PARSED as u64)
            .unwrap();
        assert_eq!(user_member.role, "Owner"); // We are the 'owner', but we are not actually the owner!
        assert!(!user_member.is_owner);
        assert_eq!(user_member.permissions.unwrap(), ProjectPermissions::all());

        // Confirm that user, a user who still has full permissions, cannot then remove the owner
        let resp = api
            .remove_from_team(alpha_team_id, FRIEND_USER_ID, USER_USER_PAT)
            .await;
        assert_eq!(resp.status(), 401);

        // V3 only- confirm the owner can change their role without losing ownership
        let resp = api
            .edit_team_member(
                alpha_team_id,
                FRIEND_USER_ID,
                json!({
                    "role": "Member"
                }),
                FRIEND_USER_PAT,
            )
            .await;
        assert_eq!(resp.status(), 204);

        let members = api
            .get_team_members_deserialized(alpha_team_id, USER_USER_PAT)
            .await;
        let friend_member = members
            .iter()
            .find(|x| x.user.id.0 == FRIEND_USER_ID_PARSED as u64)
            .unwrap();
        assert_eq!(friend_member.role, "Member");
        assert!(friend_member.is_owner);
    })
    .await;
}

// This test is currently not working.
// #[actix_rt::test]
// pub async fn no_acceptance_permissions() {
//     // Adding a user to a project team in an organization, when that user is in the organization but not the team,
//     // should have those permissions apply regardless of whether the user has accepted the invite or not.

//     // This is because project-team permission overrriding must be possible, and this overriding can decrease the number of permissions a user has.

//     let test_env = TestEnvironment::build(None).await;
//     let api = &test_env.api;

//     let alpha_team_id = &test_env.dummy.as_ref().unwrap().project_alpha.team_id;
//     let alpha_project_id = &test_env.dummy.as_ref().unwrap().project_alpha.project_id;
//     let zeta_organization_id = &test_env.dummy.as_ref().unwrap().zeta_organization_id;
//     let zeta_team_id = &test_env.dummy.as_ref().unwrap().zeta_team_id;

//     // Link alpha team to zeta org
//     let resp = api.organization_add_project(zeta_organization_id, alpha_project_id, USER_USER_PAT).await;
//     assert_eq!(resp.status(), 200);

//     // Invite friend to zeta team with all project default permissions
//     let resp = api.add_user_to_team(&zeta_team_id, FRIEND_USER_ID, Some(ProjectPermissions::all()), Some(OrganizationPermissions::all()), USER_USER_PAT).await;
//     assert_eq!(resp.status(), 204);

//     // Accept invite to zeta team
//     let resp = api.join_team(&zeta_team_id, FRIEND_USER_PAT).await;
//     assert_eq!(resp.status(), 204);

//     // Attempt, as friend, to edit details of alpha project (should succeed, org invite accepted)
//     let resp = api.edit_project(alpha_project_id, json!({
//         "title": "new name"
//     }), FRIEND_USER_PAT).await;
//     assert_eq!(resp.status(), 204);

//     // Invite friend to alpha team with *no* project permissions
//     let resp = api.add_user_to_team(&alpha_team_id, FRIEND_USER_ID, Some(ProjectPermissions::empty()), None, USER_USER_PAT).await;
//     assert_eq!(resp.status(), 204);

//     // Do not accept invite to alpha team

//     // Attempt, as friend, to edit details of alpha project (should fail now, even though user has not accepted invite)
//     let resp = api.edit_project(alpha_project_id, json!({
//         "title": "new name"
//     }), FRIEND_USER_PAT).await;
//     assert_eq!(resp.status(), 401);

//     test_env.cleanup().await;
// }
