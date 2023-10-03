use actix_web::{test, App};
use common::database::TemporaryDatabase;
use labrinth::database::models::project_item::{PROJECTS_NAMESPACE, PROJECTS_SLUGS_NAMESPACE};
use serde_json::json;

use crate::common::{actix::AppendsMultipart, setup};

// importing common module.
mod common;

#[actix_rt::test]
async fn test_get_project() {
    // Test setup and dummy data
    let db = TemporaryDatabase::create_with_dummy().await;
    let labrinth_config = setup(&db).await;
    let app = App::new().configure(|cfg| labrinth::app_config(cfg, labrinth_config.clone()));
    let test_app = test::init_service(app).await;

    // Cache should default to unpopulated
    assert!(db
        .redis_pool
        .get::<String, _>(PROJECTS_NAMESPACE, 1000)
        .await
        .unwrap()
        .is_none());

    // Perform request on dummy data
    println!("Sending request");
    let req = test::TestRequest::get()
        .uri("/v2/project/G8")
        .append_header(("Authorization", "mrp_patuser"))
        .to_request();
    let resp = test::call_service(&test_app, req).await;
    let status = resp.status();
    let body: serde_json::Value = test::read_body_json(resp).await;

    assert_eq!(status, 200);
    assert_eq!(body["id"], json!("G8"));
    assert_eq!(body["slug"], json!("testslug"));
    let versions = body["versions"].as_array().unwrap();
    assert!(versions.len() > 0);
    assert_eq!(versions[0], json!("Hk"));

    // Confirm that the request was cached
    println!("Confirming cache");
    assert_eq!(
        db.redis_pool
            .get::<i64, _>(PROJECTS_SLUGS_NAMESPACE, "testslug")
            .await
            .unwrap(),
        Some(1000)
    );

    let cached_project = db
        .redis_pool
        .get::<String, _>(PROJECTS_NAMESPACE, 1000)
        .await
        .unwrap()
        .unwrap();
    let cached_project: serde_json::Value = serde_json::from_str(&cached_project).unwrap();
    assert_eq!(cached_project["inner"]["slug"], json!("testslug"));

    // Make the request again, this time it should be cached
    let req = test::TestRequest::get()
        .uri("/v2/project/G8")
        .append_header(("Authorization", "mrp_patuser"))
        .to_request();
    let resp = test::call_service(&test_app, req).await;
    let status = resp.status();
    assert_eq!(status, 200);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["id"], json!("G8"));
    assert_eq!(body["slug"], json!("testslug"));

    // Request should fail on non-existent project
    println!("Requesting non-existent project");
    let req = test::TestRequest::get()
        .uri("/v2/project/nonexistent")
        .append_header(("Authorization", "mrp_patuser"))
        .to_request();

    let resp = test::call_service(&test_app, req).await;
    assert_eq!(resp.status(), 404);

    // Similarly, request should fail on non-authorized user, on a yet-to-be-approved or hidden project, with a 404 (hiding the existence of the project)
    println!("Requesting project as non-authorized user");
    let req = test::TestRequest::get()
        .uri("/v2/project/G9")
        .append_header(("Authorization", "mrp_patenemy"))
        .to_request();

    let resp = test::call_service(&test_app, req).await;
    assert_eq!(resp.status(), 404);

    // Cleanup test db
    db.cleanup().await;
}

#[actix_rt::test]
async fn test_add_remove_project() {
    // Test setup and dummy data
    let db = TemporaryDatabase::create_with_dummy().await;
    let labrinth_config = setup(&db).await;
    let app = App::new().configure(|cfg| labrinth::app_config(cfg, labrinth_config.clone()));
    let test_app = test::init_service(app).await;

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
        .append_header(("Authorization", "mrp_patuser"))
        .set_multipart(vec![json_segment.clone(), file_segment.clone()])
        .to_request();
    let resp = test::call_service(&test_app, req).await;

    let status = resp.status();
    assert_eq!(status, 200);

    // Get the project we just made, and confirm that it's correct
    let req = test::TestRequest::get()
        .uri("/v2/project/demo")
        .append_header(("Authorization", "mrp_patuser"))
        .to_request();

    let resp = test::call_service(&test_app, req).await;
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = test::read_body_json(resp).await;
    let versions = body["versions"].as_array().unwrap();
    assert!(versions.len() == 1);
    let uploaded_version_id = &versions[0];

    // Checks files to ensure they were uploaded and correctly identify the file
    let hash = sha1::Sha1::from(include_bytes!("../tests/files/basic-mod.jar").to_vec())
        .digest()
        .to_string();
    let req = test::TestRequest::get()
        .uri(&format!("/v2/version_file/{hash}?algorithm=sha1"))
        .append_header(("Authorization", "mrp_patuser"))
        .to_request();

    let resp = test::call_service(&test_app, req).await;
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = test::read_body_json(resp).await;
    let file_version_id = &body["id"];
    assert_eq!(&file_version_id, &uploaded_version_id);

    // Reusing with a different slug and the same file should fail
    // Even if that file is named differently
    let req = test::TestRequest::post()
        .uri("/v2/project")
        .append_header(("Authorization", "mrp_patuser"))
        .set_multipart(vec![
            json_diff_slug_file_segment.clone(), // Different slug, different file name
            file_diff_name_segment.clone(),      // Different file name, same content
        ])
        .to_request();

    let resp = test::call_service(&test_app, req).await;
    println!("Different slug, same file: {:?}", resp.response().body());
    assert_eq!(resp.status(), 400);

    // Reusing with the same slug and a different file should fail
    let req = test::TestRequest::post()
        .uri("/v2/project")
        .append_header(("Authorization", "mrp_patuser"))
        .set_multipart(vec![
            json_diff_file_segment.clone(), // Same slug, different file name
            file_diff_name_content_segment.clone(), // Different file name, different content
        ])
        .to_request();

    let resp = test::call_service(&test_app, req).await;
    println!("Same slug, different file: {:?}", resp.response().body());
    assert_eq!(resp.status(), 400);

    // Different slug, different file should succeed
    let req = test::TestRequest::post()
        .uri("/v2/project")
        .append_header(("Authorization", "mrp_patuser"))
        .set_multipart(vec![
            json_diff_slug_file_segment.clone(), // Different slug, different file name
            file_diff_name_content_segment.clone(), // Different file name, same content
        ])
        .to_request();

    let resp = test::call_service(&test_app, req).await;
    println!(
        "Different slug, different file: {:?}",
        resp.response().body()
    );
    assert_eq!(resp.status(), 200);

    // Get
    let req = test::TestRequest::get()
        .uri("/v2/project/demo")
        .append_header(("Authorization", "mrp_patuser"))
        .to_request();
    let resp = test::call_service(&test_app, req).await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = test::read_body_json(resp).await;
    let id = body["id"].to_string();

    // Remove the project
    let req = test::TestRequest::delete()
        .uri("/v2/project/demo")
        .append_header(("Authorization", "mrp_patuser"))
        .to_request();
    let resp = test::call_service(&test_app, req).await;
    assert_eq!(resp.status(), 204);

    // Confirm that the project is gone from the cache
    assert_eq!(
        db.redis_pool
            .get::<i64, _>(PROJECTS_SLUGS_NAMESPACE, "demo")
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        db.redis_pool
            .get::<i64, _>(PROJECTS_SLUGS_NAMESPACE, id)
            .await
            .unwrap(),
        None
    );

    // Old slug no longer works
    let req = test::TestRequest::get()
        .uri("/v2/project/demo")
        .append_header(("Authorization", "mrp_patuser"))
        .to_request();
    let resp = test::call_service(&test_app, req).await;
    assert_eq!(resp.status(), 404);

    // Cleanup test db
    db.cleanup().await;
}

#[actix_rt::test]
pub async fn test_patch_project() {
    let db = TemporaryDatabase::create_with_dummy().await;
    let labrinth_config = setup(&db).await;
    let app = App::new().configure(|cfg| labrinth::app_config(cfg, labrinth_config.clone()));
    let test_app = test::init_service(app).await;

    // First, we do some patch requests that should fail.
    // Failure because the user is not authorized.
    let req = test::TestRequest::patch()
        .uri("/v2/project/testslug")
        .append_header(("Authorization", "mrp_patenemy"))
        .set_json(json!({
            "title": "Test_Add_Project project - test 1",
        }))
        .to_request();
    let resp = test::call_service(&test_app, req).await;
    assert_eq!(resp.status(), 401);

    // Failure because we are setting URL fields to invalid urls.
    for url_type in ["issues_url", "source_url", "wiki_url", "discord_url"] {
        let req = test::TestRequest::patch()
            .uri("/v2/project/testslug")
            .append_header(("Authorization", "mrp_patuser"))
            .set_json(json!({
                url_type: "w.fake.url",
            }))
            .to_request();
        let resp = test::call_service(&test_app, req).await;
        assert_eq!(resp.status(), 400);
    }

    // Failure because these are illegal requested statuses for a normal user.
    for req in ["unknown", "processing", "withheld", "scheduled"] {
        let req = test::TestRequest::patch()
            .uri("/v2/project/testslug")
            .append_header(("Authorization", "mrp_patuser"))
            .set_json(json!({
                "requested_status": req,
            }))
            .to_request();
        let resp = test::call_service(&test_app, req).await;
        assert_eq!(resp.status(), 400);
    }

    // Failure because these should not be able to be set by a non-mod
    for key in ["moderation_message", "moderation_message_body"] {
        let req = test::TestRequest::patch()
            .uri("/v2/project/testslug")
            .append_header(("Authorization", "mrp_patuser"))
            .set_json(json!({
                key: "test",
            }))
            .to_request();
        let resp = test::call_service(&test_app, req).await;
        assert_eq!(resp.status(), 401);

        // (should work for a mod, though)
        let req = test::TestRequest::patch()
            .uri("/v2/project/testslug")
            .append_header(("Authorization", "mrp_patmoderator"))
            .set_json(json!({
                key: "test",
            }))
            .to_request();
        let resp = test::call_service(&test_app, req).await;
        assert_eq!(resp.status(), 204);
    }

    // Failure because the slug is already taken.
    let req = test::TestRequest::patch()
        .uri("/v2/project/testslug")
        .append_header(("Authorization", "mrp_patuser"))
        .set_json(json!({
            "slug": "testslug2", // the other dummy project has this slug
        }))
        .to_request();
    let resp = test::call_service(&test_app, req).await;
    assert_eq!(resp.status(), 400);

    // Not allowed to directly set status, as 'testslug2' (the other project) is "processing" and cannot have its status changed like this.
    let req = test::TestRequest::patch()
        .uri("/v2/project/testslug2")
        .append_header(("Authorization", "mrp_patuser"))
        .set_json(json!({
            "status": "private"
        }))
        .to_request();
    let resp = test::call_service(&test_app, req).await;
    assert_eq!(resp.status(), 401);

    // Sucessful request to patch many fields.
    let req = test::TestRequest::patch()
        .uri("/v2/project/testslug")
        .append_header(("Authorization", "mrp_patuser"))
        .set_json(json!({
            "slug": "newslug",
            "title": "New successful title",
            "description": "New successful description",
            "body": "New successful body",
            "categories": ["combat"],
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
        }))
        .to_request();
    let resp = test::call_service(&test_app, req).await;
    assert_eq!(resp.status(), 204);

    // Old slug no longer works
    let req = test::TestRequest::get()
        .uri("/v2/project/testslug")
        .append_header(("Authorization", "mrp_patuser"))
        .to_request();
    let resp = test::call_service(&test_app, req).await;
    assert_eq!(resp.status(), 404);

    // Old slug no longer works
    let req = test::TestRequest::get()
        .uri("/v2/project/newslug")
        .append_header(("Authorization", "mrp_patuser"))
        .to_request();
    let resp = test::call_service(&test_app, req).await;
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["slug"], json!("newslug"));
    assert_eq!(body["title"], json!("New successful title"));
    assert_eq!(body["description"], json!("New successful description"));
    assert_eq!(body["body"], json!("New successful body"));
    assert_eq!(body["categories"], json!(["combat"]));
    assert_eq!(body["license"]["id"], json!("MIT"));
    assert_eq!(body["issues_url"], json!("https://github.com"));
    assert_eq!(body["discord_url"], json!("https://discord.gg"));
    assert_eq!(body["wiki_url"], json!("https://wiki.com"));
    assert_eq!(body["client_side"], json!("optional"));
    assert_eq!(body["server_side"], json!("required"));
    assert_eq!(
        body["donation_urls"][0]["url"],
        json!("https://patreon.com")
    );

    // Cleanup test db
    db.cleanup().await;
}

// TODO: you are missing a lot of routes on projects here
// TODO: using permissions/scopes, can we SEE projects existence that we are not allowed to? (ie 401 isntead of 404)
