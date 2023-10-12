use actix_web::test;
use bytes::Bytes;
use common::{permissions::{PermissionsTestContext, PermissionsTest}, database::{generate_random_name, FRIEND_USER_ID, FRIEND_USER_PAT, USER_USER_PAT}};
use labrinth::models::teams::{OrganizationPermissions, ProjectPermissions};
use serde_json::json;
use crate::common::{environment::TestEnvironment, database::{MOD_USER_PAT, MOD_USER_ID, ADMIN_USER_PAT}};

mod common;

#[actix_rt::test]
async fn patch_organization_permissions() {
    let test_env = TestEnvironment::build(Some(8)).await;
    
    // For each permission covered by EDIT_DETAILS, ensure the permission is required
    let edit_details = OrganizationPermissions::EDIT_DETAILS;
    let test_pairs = [
        ("title", json!("")), // generated in the test to not collide slugs
        ("description", json!("New description"))
    ];

    for (key, value) in test_pairs {
        let req_gen = |ctx: &PermissionsTestContext| test::TestRequest::patch()
        .uri(&format!("/v2/organization/{}", ctx.organization_id.unwrap()))
        .set_json(json!({
            key: if key == "title" {
                json!(generate_random_name("randomslug"))
            } else {
                value.clone()
            },
        }));
        PermissionsTest::new(&test_env)
        .simple_organization_permissions_test(edit_details, req_gen).await.unwrap();
    }

    test_env.cleanup().await;
}

// Not covered by PATCH /organization
#[actix_rt::test]
async fn edit_details() {
    let test_env = TestEnvironment::build(None).await;
   
    let zeta_organization_id = &test_env.dummy.as_ref().unwrap().zeta_organization_id;
    let zeta_team_id = &test_env.dummy.as_ref().unwrap().zeta_team_id;
   
    let edit_details = OrganizationPermissions::EDIT_DETAILS;

    // Icon edit
    // Uses alpha organization to delete this icon
    let req_gen = |ctx: &PermissionsTestContext| test::TestRequest::patch()
            .uri(&format!("/v2/organization/{}/icon?ext=png", ctx.organization_id.unwrap()))
            .set_payload(Bytes::from(
                include_bytes!("../tests/files/200x200.png") as &[u8]
            ));
    PermissionsTest::new(&test_env)
    .with_existing_organization(zeta_organization_id, zeta_team_id)
    .with_user(FRIEND_USER_ID, FRIEND_USER_PAT, true)
    .simple_organization_permissions_test(edit_details, req_gen).await.unwrap();
            
    // Icon delete
    // Uses alpha project to delete added icon
    let req_gen = |ctx: &PermissionsTestContext| test::TestRequest::delete()
    .uri(&format!("/v2/organization/{}/icon?ext=png", ctx.organization_id.unwrap()));
    PermissionsTest::new(&test_env)
    .with_existing_organization(zeta_organization_id, zeta_team_id)
    .with_user(FRIEND_USER_ID, FRIEND_USER_PAT, true)
    .simple_organization_permissions_test(edit_details, req_gen).await.unwrap();
}

#[actix_rt::test]
async fn manage_invites() {
    // Add member, remove member, edit member
    let test_env = TestEnvironment::build(None).await;
    
    let zeta_organization_id = &test_env.dummy.as_ref().unwrap().zeta_organization_id;
    let zeta_team_id = &test_env.dummy.as_ref().unwrap().zeta_team_id;

    let manage_invites = OrganizationPermissions::MANAGE_INVITES;

    // Add member
    let req_gen = |ctx: &PermissionsTestContext| test::TestRequest::post()
    .uri(&format!("/v2/team/{}/members", ctx.team_id.unwrap()))
    .set_json(json!({
        "user_id": MOD_USER_ID,
        "permissions": 0,
        "organization_permissions": 0,
    }));
    PermissionsTest::new(&test_env)
    .with_existing_organization(zeta_organization_id, zeta_team_id)
    .with_user(FRIEND_USER_ID, FRIEND_USER_PAT, true)
    .simple_organization_permissions_test(manage_invites, req_gen).await.unwrap();

    // Edit member
    let edit_member = OrganizationPermissions::EDIT_MEMBER;
    let req_gen = |ctx: &PermissionsTestContext| test::TestRequest::patch()
    .uri(&format!("/v2/team/{}/members/{MOD_USER_ID}", ctx.team_id.unwrap()))
    .set_json(json!({
        "organization_permissions": 0,
    }));
    PermissionsTest::new(&test_env)
    .with_existing_organization(zeta_organization_id, zeta_team_id)
    .with_user(FRIEND_USER_ID, FRIEND_USER_PAT, true)
    .simple_organization_permissions_test(edit_member, req_gen).await.unwrap();

    // remove member
    // requires manage_invites if they have not yet accepted the invite
    let req_gen = |ctx: &PermissionsTestContext| test::TestRequest::delete()
    .uri(&format!("/v2/team/{}/members/{MOD_USER_ID}", ctx.team_id.unwrap()));
    PermissionsTest::new(&test_env)
    .with_existing_organization(zeta_organization_id, zeta_team_id)
    .with_user(FRIEND_USER_ID, FRIEND_USER_PAT, true)
    .simple_organization_permissions_test(manage_invites, req_gen).await.unwrap();

    // re-add member for testing
    let req = test::TestRequest::post()
    .uri(&format!("/v2/team/{}/members", zeta_team_id))
    .append_header(("Authorization", ADMIN_USER_PAT))
    .set_json(json!({
        "user_id": MOD_USER_ID,
    })).to_request();
    let resp = test_env.call(req).await;
    assert_eq!(resp.status(), 204);

    // Accept invite
    let req = test::TestRequest::post()
    .uri(&format!("/v2/team/{}/join", zeta_team_id))
    .append_header(("Authorization", MOD_USER_PAT)).to_request();
    let resp = test_env.call(req).await;
    assert_eq!(resp.status(), 204);

    // remove existing member (requires remove_member)
    let remove_member = OrganizationPermissions::REMOVE_MEMBER;
    let req_gen = |ctx: &PermissionsTestContext| test::TestRequest::delete()
    .uri(&format!("/v2/team/{}/members/{MOD_USER_ID}", ctx.team_id.unwrap()));

    PermissionsTest::new(&test_env)
    .with_existing_organization(zeta_organization_id, zeta_team_id)
    .with_user(FRIEND_USER_ID, FRIEND_USER_PAT, true)
    .simple_organization_permissions_test(remove_member, req_gen).await.unwrap();

    test_env.cleanup().await;

}

#[actix_rt::test]
async fn add_remove_project() {
        let test_env = TestEnvironment::build(None).await;
    
        let alpha_project_id = &test_env.dummy.as_ref().unwrap().alpha_project_id;
        let alpha_team_id = &test_env.dummy.as_ref().unwrap().alpha_team_id;
        let zeta_organization_id = &test_env.dummy.as_ref().unwrap().zeta_organization_id;
        let zeta_team_id = &test_env.dummy.as_ref().unwrap().zeta_team_id;
    
        let add_project = OrganizationPermissions::ADD_PROJECT;
    
        // First, we add FRIEND_USER_ID to the alpha project and transfer ownership to them
        // This is because the ownership of a project is needed to add it to an organization
        let req = test::TestRequest::post()
        .uri(&format!("/v2/team/{alpha_team_id}/members"))
        .append_header(("Authorization", USER_USER_PAT))
        .set_json(json!({
            "user_id": FRIEND_USER_ID,
        })).to_request();
        let resp = test_env.call(req).await;
        assert_eq!(resp.status(), 204);

        let req = test::TestRequest::post()
        .uri(&format!("/v2/team/{alpha_team_id}/join"))
        .append_header(("Authorization", FRIEND_USER_PAT)).to_request();
        let resp = test_env.call(req).await;
        assert_eq!(resp.status(), 204);

        let req = test::TestRequest::patch()
        .uri(&format!("/v2/team/{alpha_team_id}/owner"))
        .append_header(("Authorization", USER_USER_PAT))
        .set_json(json!({
            "user_id": FRIEND_USER_ID,
        }))
        .to_request();
        let resp = test_env.call(req).await;
        assert_eq!(resp.status(), 204);

        // Now, FRIEND_USER_ID owns the alpha project
        // Add alpha project to zeta organization
        let req_gen = |ctx: &PermissionsTestContext| 
        test::TestRequest::post()
        .uri(&format!("/v2/organization/{}/projects", ctx.organization_id.unwrap()))
        .set_json(json!({
            "project_id": alpha_project_id,
        }));
        PermissionsTest::new(&test_env)
        .with_existing_organization(zeta_organization_id, zeta_team_id)
        .with_user(FRIEND_USER_ID, FRIEND_USER_PAT, true)
        .simple_organization_permissions_test(add_project, req_gen).await.unwrap();
    
        // Remove alpha project from zeta organization
        let remove_project = OrganizationPermissions::REMOVE_PROJECT;
        let req_gen = |ctx: &PermissionsTestContext| 
        test::TestRequest::delete()
        .uri(&format!("/v2/organization/{}/projects/{alpha_project_id}", ctx.organization_id.unwrap()));
        PermissionsTest::new(&test_env)
        .with_existing_organization(zeta_organization_id, zeta_team_id)
        .with_user(FRIEND_USER_ID, FRIEND_USER_PAT, true)
        .simple_organization_permissions_test(remove_project, req_gen).await.unwrap();
    
        test_env.cleanup().await;    
}

#[actix_rt::test]
async fn delete_organization() {
        let test_env = TestEnvironment::build(None).await;
        let delete_organization = OrganizationPermissions::DELETE_ORGANIZATION;

        // Now, FRIEND_USER_ID owns the alpha project
        // Add alpha project to zeta organization
        let req_gen = |ctx: &PermissionsTestContext| 
        test::TestRequest::delete()
        .uri(&format!("/v2/organization/{}", ctx.organization_id.unwrap()));
        PermissionsTest::new(&test_env)
        .simple_organization_permissions_test(delete_organization, req_gen).await.unwrap();
    
        test_env.cleanup().await;    
}

#[actix_rt::test]
async fn add_default_project_permissions() {
        let test_env = TestEnvironment::build(None).await;
        let zeta_organization_id = &test_env.dummy.as_ref().unwrap().zeta_organization_id;
        let zeta_team_id = &test_env.dummy.as_ref().unwrap().zeta_team_id;


        // Add member
        let add_member_default_permissions = OrganizationPermissions::MANAGE_INVITES | OrganizationPermissions::EDIT_MEMBER_DEFAULT_PERMISSIONS;
        
        // Failure test should include MANAGE_INVITES, as it is required to add
        // default permissions on an invited user, but should still fail without EDIT_MEMBER_DEFAULT_PERMISSIONS
        let failure_with_add_member = (OrganizationPermissions::all() ^ add_member_default_permissions) | OrganizationPermissions::MANAGE_INVITES;

        let req_gen = |ctx: &PermissionsTestContext| test::TestRequest::post()
        .uri(&format!("/v2/team/{}/members", ctx.team_id.unwrap()))
        .set_json(json!({
            "user_id": MOD_USER_ID,
            // do not set permissions as it will be set to default, which is non-zero
            "organization_permissions": 0,
        }));
        PermissionsTest::new(&test_env)
        .with_existing_organization(zeta_organization_id, zeta_team_id)
        .with_user(FRIEND_USER_ID, FRIEND_USER_PAT, true)
        .with_failure_permissions(None, Some(failure_with_add_member))
        .simple_organization_permissions_test(add_member_default_permissions, req_gen).await.unwrap();
    
        // Now that member is added, modify default permissions
        let modify_member_default_permission = OrganizationPermissions::EDIT_MEMBER | OrganizationPermissions::EDIT_MEMBER_DEFAULT_PERMISSIONS;

        // Failure test should include MANAGE_INVITES, as it is required to add
        // default permissions on an invited user, but should still fail without EDIT_MEMBER_DEFAULT_PERMISSIONS
        let failure_with_modify_member = (OrganizationPermissions::all() ^ add_member_default_permissions) | OrganizationPermissions::EDIT_MEMBER;
        
        let req_gen = |ctx: &PermissionsTestContext| test::TestRequest::patch()
        .uri(&format!("/v2/team/{}/members/{MOD_USER_ID}", ctx.team_id.unwrap()))
        .set_json(json!({
            "permissions": ProjectPermissions::EDIT_DETAILS.bits(),
        }));
        PermissionsTest::new(&test_env)
        .with_existing_organization(zeta_organization_id, zeta_team_id)
        .with_user(FRIEND_USER_ID, FRIEND_USER_PAT, true)
        .with_failure_permissions(None, Some(failure_with_modify_member))
        .simple_organization_permissions_test(modify_member_default_permission, req_gen).await.unwrap();

        test_env.cleanup().await;    
}