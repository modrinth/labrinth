use actix_web::test;
use common::{permissions::{PermissionsTestContext, PermissionsTest}, database::generate_random_name};
use labrinth::models::teams::{ProjectPermissions, OrganizationPermissions};
use serde_json::json;

use crate::common::environment::TestEnvironment;

mod common;

#[actix_rt::test]
async fn patch_project_permissions() {
    let test_env = TestEnvironment::build_with_dummy().await;

    // for each permission covered by EDIT_DETAILS, ensure the permission is required
    let success_permissions = ProjectPermissions::EDIT_DETAILS;
    let test_pairs = [
        ("slug", json!("")), // generated in the test to not collide slugs
        ("title", json!("randomname")),
        ("description", json!("randomdescription")),
        ("categories", json!(["combat", "economy"])),
        ("client_side", json!("unsupported")),
        ("server_side", json!("unsupported")),
        // ("status", json!("processing")), // TODO: Project submitted for review with no initial versions\
        // ("required_status", json!("draft")),
        ("additional_categories", json!(["decoration"])),
        ("issues_url", json!("https://issues.com")),
        ("source_url", json!("https://source.com")),
        ("wiki_url", json!("https://wiki.com")),
        ("donation_urls", json!([{
            "id": "paypal",
            "platform": "Paypal",
            "url": "https://paypal.com"
        }])),
        ("discord_url", json!("https://discord.com")),
        ("license_id", json!("MIT"))
    ];
    for (key, value) in test_pairs {
        let req_gen = |ctx: &PermissionsTestContext| test::TestRequest::patch()
        .uri(&format!("/v2/project/{}", ctx.project_id.unwrap()))
        .set_json(json!({
            key: if key == "slug" {
                json!(generate_random_name("randomslug"))
            } else {
                value.clone()
            },
        }));
    
        PermissionsTest::new(&test_env)
        .project_permissions_test(success_permissions, req_gen).await.unwrap();
    
    }


    test_env.cleanup().await;
}
