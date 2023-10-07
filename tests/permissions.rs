use actix_web::test;
use common::permissions::{PermissionsTestContext, PermissionsTest};
use labrinth::models::teams::{ProjectPermissions, OrganizationPermissions};
use serde_json::json;

use crate::common::environment::TestEnvironment;

mod common;

#[actix_rt::test]
async fn temporary_project_test() {
    let test_env = TestEnvironment::build_with_dummy().await;

    let req_gen = |ctx: &PermissionsTestContext| test::TestRequest::patch()
    .uri(&format!("/v2/project/{}", ctx.project_id.unwrap()))
    .set_json(json!({
        "description": "Example description - changed.",
    }));
    let success_permissions = ProjectPermissions::EDIT_DETAILS | ProjectPermissions::EDIT_BODY;

    PermissionsTest::new(&test_env).project_permissions_test(success_permissions, req_gen).await.unwrap();

    test_env.cleanup().await;
}

#[actix_rt::test]
async fn temporary_organization_test() {
    let test_env = TestEnvironment::build_with_dummy().await;

    let req_gen = |ctx: &PermissionsTestContext| test::TestRequest::patch()
    .uri(&format!("/v2/organization/{}", ctx.organization_id.unwrap()))
    .set_json(json!({
        "description": "Example description - changed.",
    }));
    let success_permissions = OrganizationPermissions::EDIT_DETAILS | OrganizationPermissions::EDIT_BODY;

    PermissionsTest::new(&test_env).organization_permissions_tests(success_permissions, req_gen).await;

    test_env.cleanup().await;
}

