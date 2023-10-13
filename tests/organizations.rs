use crate::common::{
    database::USER_USER_ID, dummy_data::DummyImage, environment::TestEnvironment,
    request_data::get_icon_data,
};
use common::database::USER_USER_PAT;
use labrinth::models::teams::OrganizationPermissions;
use serde_json::json;

mod common;

#[actix_rt::test]
async fn create_organization() {
    let test_env = TestEnvironment::build(None).await;
    let api = &test_env.v2;
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
        let resp = api
            .create_organization(title, "theta_description", USER_USER_PAT)
            .await;
        assert_eq!(resp.status(), 400);
    }

    // Failed creations description:
    // - too short slug
    // - too long slug
    for description in ["a", &"a".repeat(300)] {
        let resp = api
            .create_organization("theta", description, USER_USER_PAT)
            .await;
        assert_eq!(resp.status(), 400);
    }

    // Create 'theta' organization
    let resp = api
        .create_organization("theta", "not url safe%&^!#$##!@#$%^&", USER_USER_PAT)
        .await;
    assert_eq!(resp.status(), 200);

    // Get organization using slug
    let theta = api
        .get_organization_deserialized("theta", USER_USER_PAT)
        .await;
    assert_eq!(theta.title, "theta");
    assert_eq!(theta.description, "not url safe%&^!#$##!@#$%^&");
    assert_eq!(resp.status(), 200);

    // Get created team
    let members = api
        .get_organization_members_deserialized("theta", USER_USER_PAT)
        .await;

    // Should only be one member, which is USER_USER_ID, and is the owner with full permissions
    assert_eq!(members[0].user.id.to_string(), USER_USER_ID);
    assert_eq!(
        members[0].organization_permissions,
        Some(OrganizationPermissions::all())
    );
    assert_eq!(members[0].role, "Owner");

    test_env.cleanup().await;
}

#[actix_rt::test]
async fn patch_organization() {
    let test_env = TestEnvironment::build(None).await;
    let api = &test_env.v2;

    let zeta_organization_id = &test_env.dummy.as_ref().unwrap().zeta_organization_id;

    // Create 'theta' organization
    let resp = api
        .create_organization("theta", "theta_description", USER_USER_PAT)
        .await;
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
        let resp = api
            .edit_organization(
                zeta_organization_id,
                json!({
                    "title": title,
                    "description": "theta_description"
                }),
                USER_USER_PAT,
            )
            .await;
        assert_eq!(resp.status(), 400);
    }

    // Failed patch to zeta description:
    // - too short description
    // - too long description
    for description in ["a", &"a".repeat(300)] {
        let resp = api
            .edit_organization(
                zeta_organization_id,
                json!({
                    "description": description
                }),
                USER_USER_PAT,
            )
            .await;
        assert_eq!(resp.status(), 400);
    }

    // Successful patch to many fields
    let resp = api
        .edit_organization(
            zeta_organization_id,
            json!({
                "title": "new_title",
                "description": "not url safe%&^!#$##!@#$%^&" // not-URL-safe description should still work
            }),
            USER_USER_PAT,
        )
        .await;
    assert_eq!(resp.status(), 204);

    // Get project using new slug
    let new_title = api
        .get_organization_deserialized("new_title", USER_USER_PAT)
        .await;
    assert_eq!(new_title.title, "new_title");
    assert_eq!(new_title.description, "not url safe%&^!#$##!@#$%^&");

    test_env.cleanup().await;
}

// add/remove icon
#[actix_rt::test]
async fn add_remove_icon() {
    let test_env = TestEnvironment::build(None).await;
    let api = &test_env.v2;
    let zeta_organization_id = &test_env.dummy.as_ref().unwrap().zeta_organization_id;

    // Get project
    let resp = test_env
        .v2
        .get_organization_deserialized(zeta_organization_id, USER_USER_PAT)
        .await;
    assert_eq!(resp.icon_url, None);

    // Icon edit
    // Uses alpha organization to delete this icon
    let resp = api
        .edit_organization_icon(
            zeta_organization_id,
            Some(get_icon_data(DummyImage::SmallIcon)),
            USER_USER_PAT,
        )
        .await;
    assert_eq!(resp.status(), 204);

    // Get project
    let zeta_org = api
        .get_organization_deserialized(zeta_organization_id, USER_USER_PAT)
        .await;
    assert!(zeta_org.icon_url.is_some());

    // Icon delete
    // Uses alpha organization to delete added icon
    let resp = api
        .edit_organization_icon(zeta_organization_id, None, USER_USER_PAT)
        .await;
    assert_eq!(resp.status(), 204);

    // Get project
    let zeta_org = api
        .get_organization_deserialized(zeta_organization_id, USER_USER_PAT)
        .await;
    assert!(zeta_org.icon_url.is_none());

    test_env.cleanup().await;
}

// delete org
#[actix_rt::test]
async fn delete_org() {
    let test_env = TestEnvironment::build(None).await;
    let api = &test_env.v2;
    let zeta_organization_id = &test_env.dummy.as_ref().unwrap().zeta_organization_id;

    let resp = api
        .delete_organization(zeta_organization_id, USER_USER_PAT)
        .await;
    assert_eq!(resp.status(), 204);

    // Get organization, which should no longer exist
    let resp = api
        .get_organization(zeta_organization_id, USER_USER_PAT)
        .await;
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
        let resp = test_env
            .v2
            .organization_add_project(zeta_organization_id, alpha, USER_USER_PAT)
            .await;
        assert_eq!(resp.status(), 200);

        // Get organization projects
        let projects = test_env
            .v2
            .get_organization_projects_deserialized(zeta_organization_id, USER_USER_PAT)
            .await;
        assert_eq!(projects[0].id.to_string(), alpha_project_id);
        assert_eq!(projects[0].slug, Some(alpha_project_slug.to_string()));

        // Remove project from organization
        let resp = test_env
            .v2
            .organization_remove_project(zeta_organization_id, alpha, USER_USER_PAT)
            .await;
        assert_eq!(resp.status(), 200);

        // Get organization projects
        let projects = test_env
            .v2
            .get_organization_projects_deserialized(zeta_organization_id, USER_USER_PAT)
            .await;
        assert!(projects.is_empty());
    }

    test_env.cleanup().await;
}
