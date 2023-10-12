use actix_web::test;
use common::permissions::{PermissionsTest, PermissionsTestContext};
use labrinth::models::teams::{OrganizationPermissions, ProjectPermissions};
use serde_json::json;

use crate::common::environment::TestEnvironment;

mod common;

#[actix_rt::test]
async fn project_permissions_consistency_test() {
    let test_env = TestEnvironment::build(Some(8)).await;

    // Full project permissions test with EDIT_DETAILS
    let success_permissions = ProjectPermissions::EDIT_DETAILS;
    let req_gen = |ctx: &PermissionsTestContext| {
        test::TestRequest::patch()
            .uri(&format!("/v2/project/{}", ctx.project_id.unwrap()))
            .set_json(json!({
                "title": "Example title - changed.",
            }))
    };
    PermissionsTest::new(&test_env)
        .full_project_permissions_test(success_permissions, req_gen)
        .await
        .unwrap();

    // We do a test with more specific permissions, to ensure that *exactly* the permissions at each step are as expected
    let success_permissions = ProjectPermissions::EDIT_DETAILS
        | ProjectPermissions::REMOVE_MEMBER
        | ProjectPermissions::DELETE_VERSION
        | ProjectPermissions::VIEW_PAYOUTS;
    let req_gen = |ctx: &PermissionsTestContext| {
        test::TestRequest::patch()
            .uri(&format!("/v2/project/{}", ctx.project_id.unwrap()))
            .set_json(json!({
                "title": "Example title - changed.",
            }))
    };
    PermissionsTest::new(&test_env)
        .full_project_permissions_test(success_permissions, req_gen)
        .await
        .unwrap();

    test_env.cleanup().await;
}

#[actix_rt::test]
async fn organization_permissions_consistency_test() {
    let test_env = TestEnvironment::build(None).await;

    // Full organization permissions test
    let success_permissions = OrganizationPermissions::EDIT_DETAILS;
    let req_gen = |ctx: &PermissionsTestContext| {
        test::TestRequest::patch()
            .uri(&format!(
                "/v2/organization/{}",
                ctx.organization_id.unwrap()
            ))
            .set_json(json!({
                "description": "Example description - changed.",
            }))
    };
    PermissionsTest::new(&test_env)
        .full_organization_permissions_tests(success_permissions, req_gen)
        .await
        .unwrap();

    test_env.cleanup().await;
}
