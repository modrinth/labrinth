use actix_http::StatusCode;
use actix_web::test;
use common::environment::with_test_environment;
use labrinth::database::models::project_item::{PROJECTS_NAMESPACE, PROJECTS_SLUGS_NAMESPACE};
use labrinth::models::ids::base62_impl::parse_base62;
use serde_json::json;

use crate::common::database::*;

use crate::common::dummy_data::DUMMY_CATEGORIES;
use crate::common::{actix::AppendsMultipart, environment::TestEnvironment};

// importing common module.
mod common;

#[actix_rt::test]
async fn test_get_project() {
    // Test setup and dummy data
    let test_env = TestEnvironment::build(None).await;
    let alpha_project_id = &test_env.dummy.as_ref().unwrap().alpha_project_id;
    let beta_project_id = &test_env.dummy.as_ref().unwrap().beta_project_id;
    let alpha_project_slug = &test_env.dummy.as_ref().unwrap().alpha_project_slug;
    let alpha_version_id = &test_env.dummy.as_ref().unwrap().alpha_version_id;

    // Perform request on dummy data
    let req = test::TestRequest::get()
        .uri(&format!("/v2/project/{alpha_project_id}"))
        .append_header(("Authorization", USER_USER_PAT))
        .to_request();
    let resp = test_env.call(req).await;
    let status = resp.status();
    let body: serde_json::Value = test::read_body_json(resp).await;

    assert_eq!(status, 200);
    assert_eq!(body["id"], json!(alpha_project_id));
    assert_eq!(body["slug"], json!(alpha_project_slug));
    let versions = body["versions"].as_array().unwrap();
    assert_eq!(versions[0], json!(alpha_version_id));

    // Confirm that the request was cached
    assert_eq!(
        test_env
            .db
            .redis_pool
            .get::<i64, _>(PROJECTS_SLUGS_NAMESPACE, alpha_project_slug)
            .await
            .unwrap(),
        Some(parse_base62(alpha_project_id).unwrap() as i64)
    );

    let cached_project = test_env
        .db
        .redis_pool
        .get::<String, _>(PROJECTS_NAMESPACE, parse_base62(alpha_project_id).unwrap())
        .await
        .unwrap()
        .unwrap();
    let cached_project: serde_json::Value = serde_json::from_str(&cached_project).unwrap();
    assert_eq!(cached_project["inner"]["slug"], json!(alpha_project_slug));

    // Make the request again, this time it should be cached
    let req = test::TestRequest::get()
        .uri(&format!("/v2/project/{alpha_project_id}"))
        .append_header(("Authorization", USER_USER_PAT))
        .to_request();
    let resp = test_env.call(req).await;
    let status = resp.status();
    assert_eq!(status, 200);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["id"], json!(alpha_project_id));
    assert_eq!(body["slug"], json!(alpha_project_slug));

    // Request should fail on non-existent project
    let req = test::TestRequest::get()
        .uri("/v2/project/nonexistent")
        .append_header(("Authorization", USER_USER_PAT))
        .to_request();

    let resp = test_env.call(req).await;
    assert_eq!(resp.status(), 404);

    // Similarly, request should fail on non-authorized user, on a yet-to-be-approved or hidden project, with a 404 (hiding the existence of the project)
    let req = test::TestRequest::get()
        .uri(&format!("/v2/project/{beta_project_id}"))
        .append_header(("Authorization", ENEMY_USER_PAT))
        .to_request();

    let resp = test_env.call(req).await;
    assert_eq!(resp.status(), 404);

    // Cleanup test db
    test_env.cleanup().await;
}

#[actix_rt::test]
async fn test_add_remove_project() {
    // Test setup and dummy data
    let test_env = TestEnvironment::build(None).await;
    let api = &test_env.v2;

    // Generate test project data.
    let mut json_data = json!(
        {
            "title": "Test_Add_Project project",
            "slug": "demo",
            "description": "Example description.",
            "body": "Example body.",
            "client_side": "required",
            "server_side": "optional",
            "initial_versions": [{
                "file_parts": ["basic-mod.jar"],
                "version_number": "1.2.3",
                "version_title": "start",
                "dependencies": [],
                "game_versions": ["1.20.1"] ,
                "release_channel": "release",
                "loaders": ["fabric"],
                "featured": true
            }],
            "categories": [],
            "license_id": "MIT"
        }
    );

    // Basic json
    let json_segment = common::actix::MultipartSegment {
        name: "data".to_string(),
        filename: None,
        content_type: Some("application/json".to_string()),
        data: common::actix::MultipartSegmentData::Text(serde_json::to_string(&json_data).unwrap()),
    };

    // Basic json, with a different file
    json_data["initial_versions"][0]["file_parts"][0] = json!("basic-mod-different.jar");
    let json_diff_file_segment = common::actix::MultipartSegment {
        data: common::actix::MultipartSegmentData::Text(serde_json::to_string(&json_data).unwrap()),
        ..json_segment.clone()
    };

    // Basic json, with a different file, and a different slug
    json_data["slug"] = json!("new_demo");
    json_data["initial_versions"][0]["file_parts"][0] = json!("basic-mod-different.jar");
    let json_diff_slug_file_segment = common::actix::MultipartSegment {
        data: common::actix::MultipartSegmentData::Text(serde_json::to_string(&json_data).unwrap()),
        ..json_segment.clone()
    };

    // Basic file
    let file_segment = common::actix::MultipartSegment {
        name: "basic-mod.jar".to_string(),
        filename: Some("basic-mod.jar".to_string()),
        content_type: Some("application/java-archive".to_string()),
        data: common::actix::MultipartSegmentData::Binary(
            include_bytes!("../tests/files/basic-mod.jar").to_vec(),
        ),
    };

    // Differently named file, with the same content (for hash testing)
    let file_diff_name_segment = common::actix::MultipartSegment {
        name: "basic-mod-different.jar".to_string(),
        filename: Some("basic-mod-different.jar".to_string()),
        content_type: Some("application/java-archive".to_string()),
        data: common::actix::MultipartSegmentData::Binary(
            include_bytes!("../tests/files/basic-mod.jar").to_vec(),
        ),
    };

    // Differently named file, with different content
    let file_diff_name_content_segment = common::actix::MultipartSegment {
        name: "basic-mod-different.jar".to_string(),
        filename: Some("basic-mod-different.jar".to_string()),
        content_type: Some("application/java-archive".to_string()),
        data: common::actix::MultipartSegmentData::Binary(
            include_bytes!("../tests/files/basic-mod-different.jar").to_vec(),
        ),
    };

    // Add a project- simple, should work.
    let req = test::TestRequest::post()
        .uri("/v2/project")
        .append_header(("Authorization", USER_USER_PAT))
        .set_multipart(vec![json_segment.clone(), file_segment.clone()])
        .to_request();
    let resp = test_env.call(req).await;

    let status = resp.status();
    assert_eq!(status, 200);

    // Get the project we just made, and confirm that it's correct
    let project = api.get_project_deserialized("demo", USER_USER_PAT).await;
    assert!(project.versions.len() == 1);
    let uploaded_version_id = project.versions[0];

    // Checks files to ensure they were uploaded and correctly identify the file
    let hash = sha1::Sha1::from(include_bytes!("../tests/files/basic-mod.jar"))
        .digest()
        .to_string();
    let version = api
        .get_version_from_hash_deserialized(&hash, "sha1", USER_USER_PAT)
        .await;
    assert_eq!(version.id, uploaded_version_id);

    // Reusing with a different slug and the same file should fail
    // Even if that file is named differently
    let req = test::TestRequest::post()
        .uri("/v2/project")
        .append_header(("Authorization", USER_USER_PAT))
        .set_multipart(vec![
            json_diff_slug_file_segment.clone(), // Different slug, different file name
            file_diff_name_segment.clone(),      // Different file name, same content
        ])
        .to_request();

    let resp = test_env.call(req).await;
    assert_eq!(resp.status(), 400);

    // Reusing with the same slug and a different file should fail
    let req = test::TestRequest::post()
        .uri("/v2/project")
        .append_header(("Authorization", USER_USER_PAT))
        .set_multipart(vec![
            json_diff_file_segment.clone(), // Same slug, different file name
            file_diff_name_content_segment.clone(), // Different file name, different content
        ])
        .to_request();

    let resp = test_env.call(req).await;
    assert_eq!(resp.status(), 400);

    // Different slug, different file should succeed
    let req = test::TestRequest::post()
        .uri("/v2/project")
        .append_header(("Authorization", USER_USER_PAT))
        .set_multipart(vec![
            json_diff_slug_file_segment.clone(), // Different slug, different file name
            file_diff_name_content_segment.clone(), // Different file name, same content
        ])
        .to_request();

    let resp = test_env.call(req).await;
    assert_eq!(resp.status(), 200);

    // Get
    let project = api.get_project_deserialized("demo", USER_USER_PAT).await;
    let id = project.id.to_string();

    // Remove the project
    let resp = test_env.v2.remove_project("demo", USER_USER_PAT).await;
    assert_eq!(resp.status(), 204);

    // Confirm that the project is gone from the cache
    assert_eq!(
        test_env
            .db
            .redis_pool
            .get::<i64, _>(PROJECTS_SLUGS_NAMESPACE, "demo")
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        test_env
            .db
            .redis_pool
            .get::<i64, _>(PROJECTS_SLUGS_NAMESPACE, id)
            .await
            .unwrap(),
        None
    );

    // Old slug no longer works
    let resp = api.get_project("demo", USER_USER_PAT).await;
    assert_eq!(resp.status(), 404);

    // Cleanup test db
    test_env.cleanup().await;
}

#[actix_rt::test]
pub async fn test_patch_project() {
    let test_env = TestEnvironment::build(None).await;
    let api = &test_env.v2;

    let alpha_project_slug = &test_env.dummy.as_ref().unwrap().alpha_project_slug;
    let beta_project_slug = &test_env.dummy.as_ref().unwrap().beta_project_slug;

    // First, we do some patch requests that should fail.
    // Failure because the user is not authorized.
    let resp = api
        .edit_project(
            alpha_project_slug,
            json!({
                "title": "Test_Add_Project project - test 1",
            }),
            ENEMY_USER_PAT,
        )
        .await;
    assert_eq!(resp.status(), 401);

    // Failure because we are setting URL fields to invalid urls.
    for url_type in ["issues_url", "source_url", "wiki_url", "discord_url"] {
        let resp = api
            .edit_project(
                alpha_project_slug,
                json!({
                    url_type: "w.fake.url",
                }),
                USER_USER_PAT,
            )
            .await;
        assert_eq!(resp.status(), 400);
    }

    // Failure because these are illegal requested statuses for a normal user.
    for req in ["unknown", "processing", "withheld", "scheduled"] {
        let resp = api
            .edit_project(
                alpha_project_slug,
                json!({
                    "requested_status": req,
                }),
                USER_USER_PAT,
            )
            .await;
        assert_eq!(resp.status(), 400);
    }

    // Failure because these should not be able to be set by a non-mod
    for key in ["moderation_message", "moderation_message_body"] {
        let resp = api
            .edit_project(
                alpha_project_slug,
                json!({
                    key: "test",
                }),
                USER_USER_PAT,
            )
            .await;
        assert_eq!(resp.status(), 401);

        // (should work for a mod, though)
        let resp = api
            .edit_project(
                alpha_project_slug,
                json!({
                    key: "test",
                }),
                MOD_USER_PAT,
            )
            .await;
        assert_eq!(resp.status(), 204);
    }

    // Failed patch to alpha slug:
    // - slug collision with beta
    // - too short slug
    // - too long slug
    // - not url safe slug
    // - not url safe slug
    for slug in [
        beta_project_slug,
        "a",
        &"a".repeat(100),
        "not url safe%&^!#$##!@#$%^&*()",
    ] {
        let resp = api
            .edit_project(
                alpha_project_slug,
                json!({
                    "slug": slug, // the other dummy project has this slug
                }),
                USER_USER_PAT,
            )
            .await;
        assert_eq!(resp.status(), 400);
    }

    // Not allowed to directly set status, as 'beta_project_slug' (the other project) is "processing" and cannot have its status changed like this.
    let resp = api
        .edit_project(
            beta_project_slug,
            json!({
                "status": "private"
            }),
            USER_USER_PAT,
        )
        .await;
    assert_eq!(resp.status(), 401);

    // Sucessful request to patch many fields.
    let resp = api
        .edit_project(
            alpha_project_slug,
            json!({
                "slug": "newslug",
                "title": "New successful title",
                "description": "New successful description",
                "body": "New successful body",
                "categories": [DUMMY_CATEGORIES[0]],
                "license_id": "MIT",
                "issues_url": "https://github.com",
                "discord_url": "https://discord.gg",
                "wiki_url": "https://wiki.com",
                "client_side": "optional",
                "server_side": "required",
                "donation_urls": [{
                    "id": "patreon",
                    "platform": "Patreon",
                    "url": "https://patreon.com"
                }]
            }),
            USER_USER_PAT,
        )
        .await;
    assert_eq!(resp.status(), 204);

    // Old slug no longer works
    let resp = api.get_project(alpha_project_slug, USER_USER_PAT).await;
    assert_eq!(resp.status(), 404);

    // New slug does work
    let project = api.get_project_deserialized("newslug", USER_USER_PAT).await;
    assert_eq!(project.slug, Some("newslug".to_string()));
    assert_eq!(project.title, "New successful title");
    assert_eq!(project.description, "New successful description");
    assert_eq!(project.body, "New successful body");
    assert_eq!(project.categories, vec![DUMMY_CATEGORIES[0]]);
    assert_eq!(project.license.id, "MIT");
    assert_eq!(project.issues_url, Some("https://github.com".to_string()));
    assert_eq!(project.discord_url, Some("https://discord.gg".to_string()));
    assert_eq!(project.wiki_url, Some("https://wiki.com".to_string()));
    assert_eq!(project.client_side.to_string(), "optional");
    assert_eq!(project.server_side.to_string(), "required");
    assert_eq!(project.donation_urls.unwrap()[0].url, "https://patreon.com");

    // Cleanup test db
    test_env.cleanup().await;
}

#[actix_rt::test]
pub async fn test_bulk_edit_categories() {
    with_test_environment(|test_env| async move {
        let api = &test_env.v2;
        let alpha_project_id: &str = &test_env.dummy.as_ref().unwrap().alpha_project_id;
        let beta_project_id: &str = &test_env.dummy.as_ref().unwrap().beta_project_id;

        let resp = api
            .edit_project_bulk(
                [alpha_project_id, beta_project_id],
                json!({
                    "categories": [DUMMY_CATEGORIES[0], DUMMY_CATEGORIES[3]],
                    "add_categories": [DUMMY_CATEGORIES[1], DUMMY_CATEGORIES[2]],
                    "remove_categories": [DUMMY_CATEGORIES[3]],
                    "additional_categories": [DUMMY_CATEGORIES[4], DUMMY_CATEGORIES[6]],
                    "add_additional_categories": [DUMMY_CATEGORIES[5]],
                    "remove_additional_categories": [DUMMY_CATEGORIES[6]],
                }),
                ADMIN_USER_PAT,
            )
            .await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let alpha_body = api
            .get_project_deserialized(alpha_project_id, ADMIN_USER_PAT)
            .await;
        assert_eq!(alpha_body.categories, DUMMY_CATEGORIES[0..=2]);
        assert_eq!(alpha_body.additional_categories, DUMMY_CATEGORIES[4..=5]);

        let beta_body = api
            .get_project_deserialized(beta_project_id, ADMIN_USER_PAT)
            .await;
        assert_eq!(beta_body.categories, alpha_body.categories);
        assert_eq!(
            beta_body.additional_categories,
            alpha_body.additional_categories,
        );
    })
    .await;
}

// TODO: Missing routes on projects
// TODO: using permissions/scopes, can we SEE projects existence that we are not allowed to? (ie 401 instead of 404)
