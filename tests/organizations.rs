use crate::common::{database::USER_USER_ID, environment::TestEnvironment};
use actix_web::test;
use bytes::Bytes;
use common::database::USER_USER_PAT;
use labrinth::models::teams::OrganizationPermissions;
use serde_json::json;

mod common;

#[actix_rt::test]
async fn create_organization() {
    let test_env = TestEnvironment::build(None).await;
    let zeta_organization_slug = &test_env.dummy.as_ref().unwrap().zeta_organization_id;

    // Failed creations title:
    // - slug collision with zeta
    // - too short slug
    // - too long slug
    // - not url safe slug
    for title in [
        zeta_organization_slug,
        "a",
        &"a".repeat(100),
        "not url safe%&^!#$##!@#$%^&*()",
    ] {
        let req = test::TestRequest::post()
            .uri("/v2/organization")
            .append_header(("Authorization", USER_USER_PAT))
            .set_json(json!({
                "title": title,
                "description": "theta_description"
            }))
            .to_request();
        let resp = test_env.call(req).await;
        assert_eq!(resp.status(), 400);
    }

    // Failed creations description:
    // - too short slug
    // - too long slug
    for description in ["a", &"a".repeat(300)] {
        let req = test::TestRequest::post()
            .uri("/v2/organization")
            .append_header(("Authorization", USER_USER_PAT))
            .set_json(json!({
                "title": "theta",
                "description": description
            }))
            .to_request();
        let resp = test_env.call(req).await;
        assert_eq!(resp.status(), 400);
    }

    // Create 'theta' organization
    let req = test::TestRequest::post()
        .uri("/v2/organization")
        .append_header(("Authorization", USER_USER_PAT))
        .set_json(json!({
            "title": "theta",
            "description": "not url safe%&^!#$##!@#$%^&"
        }))
        .to_request();
    let resp = test_env.call(req).await;
    assert_eq!(resp.status(), 200);

    // Get project using slug
    let req = test::TestRequest::get()
        .uri("/v2/organization/theta")
        .append_header(("Authorization", USER_USER_PAT))
        .to_request();

    let resp = test_env.call(req).await;
    assert_eq!(resp.status(), 200);
    let value: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(value["title"], "theta");
    assert_eq!(value["description"], "not url safe%&^!#$##!@#$%^&");

    // Get created team
    let req = test::TestRequest::get()
        .uri("/v2/organization/theta/members")
        .append_header(("Authorization", USER_USER_PAT))
        .to_request();
    let resp = test_env.call(req).await;

    // Should only be one member, which is USER_USER_ID, and is the owner with full permissions
    assert_eq!(resp.status(), 200);
    let value: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(value[0]["user"]["id"], USER_USER_ID);
    assert_eq!(
        value[0]["permissions"],
        OrganizationPermissions::all().bits()
    );
    assert_eq!(value[0]["role"], "Owner");

    test_env.cleanup().await;
}

#[actix_rt::test]
async fn patch_organization() {
    let test_env = TestEnvironment::build(None).await;
    let zeta_organization_id = &test_env.dummy.as_ref().unwrap().zeta_organization_id;

    // Create 'theta' organization
    let req = test::TestRequest::post()
        .uri("/v2/organization")
        .append_header(("Authorization", USER_USER_PAT))
        .set_json(json!({
            "title": "theta",
            "description": "theta_description"
        }))
        .to_request();
    let resp = test_env.call(req).await;
    assert_eq!(resp.status(), 200);

    // Failed patch to zeta slug:
    // - slug collision with theta
    // - too short slug
    // - too long slug
    // - not url safe slug
    for title in [
        "theta",
        "a",
        &"a".repeat(100),
        "not url safe%&^!#$##!@#$%^&*()",
    ] {
        let req = test::TestRequest::patch()
            .uri(&format!("/v2/organization/{zeta_organization_id}"))
            .append_header(("Authorization", USER_USER_PAT))
            .set_json(json!({
                "title": title,
                "description": "theta_description"
            }))
            .to_request();

        let resp = test_env.call(req).await;
        assert_eq!(resp.status(), 400);
    }

    // Failed patch to zeta description:
    // - too short description
    // - too long description
    for description in ["a", &"a".repeat(300)] {
        let req = test::TestRequest::patch()
            .uri(&format!("/v2/organization/{zeta_organization_id}"))
            .append_header(("Authorization", USER_USER_PAT))
            .set_json(json!({
                "description": description
            }))
            .to_request();

        let resp = test_env.call(req).await;
        assert_eq!(resp.status(), 400);
    }

    // Successful patch to many fields
    let req = test::TestRequest::patch()
        .uri(&format!("/v2/organization/{zeta_organization_id}"))
        .append_header(("Authorization", USER_USER_PAT))
        .set_json(json!({
            "title": "new_title",
            "description": "not url safe%&^!#$##!@#$%^&" // not-URL-safe description should still work
        }))
        .to_request();

    let resp = test_env.call(req).await;
    assert_eq!(resp.status(), 204);

    // Get project using new slug
    let req = test::TestRequest::get()
        .uri("/v2/organization/new_title")
        .append_header(("Authorization", USER_USER_PAT))
        .to_request();

    let resp = test_env.call(req).await;
    assert_eq!(resp.status(), 200);
    let value: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(value["title"], "new_title");
    assert_eq!(value["description"], "not url safe%&^!#$##!@#$%^&");

    test_env.cleanup().await;
}

// add/remove icon
#[actix_rt::test]
async fn add_remove_icon() {
    let test_env = TestEnvironment::build(None).await;
    let zeta_organization_id = &test_env.dummy.as_ref().unwrap().zeta_organization_id;

    // Get project
    let req = test::TestRequest::get()
        .uri(&format!(
            "/v2/organization/{zeta_organization_id}",
            zeta_organization_id = zeta_organization_id
        ))
        .append_header(("Authorization", USER_USER_PAT))
        .to_request();

    let resp = test_env.call(req).await;
    assert_eq!(resp.status(), 200);

    let value: serde_json::Value = test::read_body_json(resp).await;
    assert!(value["icon_url"].is_null());

    // Icon edit
    // Uses alpha organization to delete this icon
    let req = test::TestRequest::patch()
        .uri(&format!(
            "/v2/organization/{zeta_organization_id}/icon?ext=png"
        ))
        .append_header(("Authorization", USER_USER_PAT))
        .set_payload(Bytes::from(
            include_bytes!("../tests/files/200x200.png") as &[u8]
        ))
        .to_request();
    let resp = test_env.call(req).await;
    assert_eq!(resp.status(), 204);

    // Get project
    let req = test::TestRequest::get()
        .uri(&format!(
            "/v2/organization/{zeta_organization_id}",
            zeta_organization_id = zeta_organization_id
        ))
        .append_header(("Authorization", USER_USER_PAT))
        .to_request();

    let resp = test_env.call(req).await;
    assert_eq!(resp.status(), 200);
    let value: serde_json::Value = test::read_body_json(resp).await;
    assert!(!value["icon_url"].is_null());

    // Icon delete
    // Uses alpha project to delete added icon
    let req = test::TestRequest::delete()
        .uri(&format!("/v2/organization/{zeta_organization_id}/icon"))
        .append_header(("Authorization", USER_USER_PAT))
        .to_request();
    let resp = test_env.call(req).await;
    assert_eq!(resp.status(), 204);

    // Get project
    let req = test::TestRequest::get()
        .uri(&format!(
            "/v2/organization/{zeta_organization_id}",
            zeta_organization_id = zeta_organization_id
        ))
        .append_header(("Authorization", USER_USER_PAT))
        .to_request();

    let resp = test_env.call(req).await;
    assert_eq!(resp.status(), 200);
    let value: serde_json::Value = test::read_body_json(resp).await;
    assert!(value["icon_url"].is_null());

    test_env.cleanup().await;
}

// delete org
#[actix_rt::test]
async fn delete_org() {
    let test_env = TestEnvironment::build(None).await;
    let zeta_organization_id = &test_env.dummy.as_ref().unwrap().zeta_organization_id;

    let req = test::TestRequest::delete()
        .uri(&format!("/v2/organization/{zeta_organization_id}"))
        .append_header(("Authorization", USER_USER_PAT))
        .to_request();

    let resp = test_env.call(req).await;
    assert_eq!(resp.status(), 204);

    // Get organization, which should no longer exist
    let req = test::TestRequest::get()
        .uri(&format!("/v2/organization/{zeta_organization_id}"))
        .append_header(("Authorization", USER_USER_PAT))
        .to_request();

    let resp = test_env.call(req).await;
    assert_eq!(resp.status(), 404);

    test_env.cleanup().await;
}

// add/remove organization projects
#[actix_rt::test]
async fn add_remove_organization_projects() {
    let test_env = TestEnvironment::build(None).await;
    let alpha_project_id: &str = &test_env.dummy.as_ref().unwrap().alpha_project_id;
    let alpha_project_slug: &str = &test_env.dummy.as_ref().unwrap().alpha_project_slug;
    let zeta_organization_id: &str = &test_env.dummy.as_ref().unwrap().zeta_organization_id;

    // Add/remove project to organization, first by ID, then by slug
    for alpha in [alpha_project_id, alpha_project_slug] {
        let req = test::TestRequest::post()
            .uri(&format!("/v2/organization/{zeta_organization_id}/projects"))
            .append_header(("Authorization", USER_USER_PAT))
            .set_json(json!({
                "project_id": alpha
            }))
            .to_request();

        let resp = test_env.call(req).await;
        assert_eq!(resp.status(), 200);

        // Get organization projects
        let req = test::TestRequest::get()
            .uri(&format!("/v2/organization/{zeta_organization_id}/projects"))
            .append_header(("Authorization", USER_USER_PAT))
            .to_request();

        let resp = test_env.call(req).await;
        assert_eq!(resp.status(), 200);
        let value: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(value[0]["id"], json!(alpha_project_id));
        assert_eq!(value[0]["slug"], json!(alpha_project_slug));

        // Remove project from organization
        let req = test::TestRequest::delete()
            .uri(&format!(
                "/v2/organization/{zeta_organization_id}/projects/{alpha_project_id}"
            ))
            .append_header(("Authorization", USER_USER_PAT))
            .to_request();

        let resp = test_env.call(req).await;
        assert_eq!(resp.status(), 200);

        // Get organization projects
        let req = test::TestRequest::get()
            .uri(&format!("/v2/organization/{zeta_organization_id}/projects"))
            .append_header(("Authorization", USER_USER_PAT))
            .to_request();
        let resp = test_env.call(req).await;
        let value: serde_json::Value = test::read_body_json(resp).await;
        assert!(value.as_array().unwrap().is_empty());
    }

    test_env.cleanup().await;
}
