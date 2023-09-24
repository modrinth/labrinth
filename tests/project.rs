use actix_web::{App, test};
use common::database::TemporaryDatabase;
use labrinth::database::models::project_item::{PROJECTS_NAMESPACE, PROJECTS_SLUGS_NAMESPACE};
use serde_json::json;

use crate::common::{setup, actix::generate_multipart};

// importing common module.
mod common;

#[actix_rt::test]
async fn test_get_project() {
    // Test setup and dummy data
    let db = TemporaryDatabase::create_with_dummy().await;
    let labrinth_config = setup(&db).await;
    let app = App::new()
        .configure(|cfg | labrinth::app_config(cfg, labrinth_config.clone()));
    let test_app = test::init_service(app).await;

    // Cache should default to unpopulated
    assert!(db.redis_pool.get::<String, _>(PROJECTS_NAMESPACE, 1000).await.unwrap().is_none());

    // Perform request on dummy data
    println!("Sending request");
    let req = test::TestRequest::get()
        .uri("/v2/project/G8")
        .append_header(("Authorization","mrp_patuser"))
        .to_request();
    let resp = test::call_service(&test_app, req).await;
    let status = resp.status();
    let body : serde_json::Value = test::read_body_json(resp).await;

    assert_eq!(status, 200);
    assert!(body.get("id").is_some());
    assert_eq!(body.get("slug").unwrap(), &json!("testslug"));
    let versions = body.get("versions").unwrap().as_array().unwrap();
    assert!(versions.len() > 0);
    assert_eq!(versions[0], json!("Hk"));

    // Confirm that the request was cached
    println!("Confirming cache");
    assert_eq!(db.redis_pool.get::<i64, _>(PROJECTS_SLUGS_NAMESPACE, "testslug").await.unwrap(), Some(1000));
    
    let cached_project = db.redis_pool.get::<String, _>(PROJECTS_NAMESPACE, 1000).await.unwrap().unwrap();
    let cached_project : serde_json::Value = serde_json::from_str(&cached_project).unwrap();
    println!("Cached project: {:?}", cached_project);
    println!("Cached project: {:?}", cached_project.to_string());
    println!("{:?}",cached_project.as_object().unwrap());
    assert_eq!(cached_project.get("inner").unwrap().get("slug").unwrap(), &json!("testslug"));

    // Make the request again, this time it should be cached
    let req = test::TestRequest::get()
        .uri("/v2/project/G8")
        .append_header(("Authorization","mrp_patuser"))
        .to_request();
    let resp = test::call_service(&test_app, req).await;
    let status = resp.status();
    assert_eq!(status, 200);

    let body : serde_json::Value = test::read_body_json(resp).await;
    assert!(body.get("id").is_some());
    assert_eq!(body.get("slug").unwrap(), &json!("testslug"));

    // Request should fail on non-existent project
    println!("Requesting non-existent project");
    let req = test::TestRequest::get()
        .uri("/v2/project/nonexistent")
        .append_header(("Authorization","mrp_patuser"))
        .to_request();

    let resp = test::call_service(&test_app, req).await;
    println!("Response: {:?}", resp.response().body());
    assert_eq!(resp.status(), 404);

    // Similarly, request should fail on non-authorized user, with a 404 (hiding the existence of the project)
    println!("Requesting project as non-authorized user");
    let req = test::TestRequest::get()
        .uri("/v2/project/G8")
        .append_header(("Authorization","mrp_patenemy"))
        .to_request();

    let resp = test::call_service(&test_app, req).await;
    println!("Response: {:?}", resp.response().body());
    assert_eq!(resp.status(), 404);

    // Cleanup test db
    db.cleanup().await;
}

#[actix_rt::test]
async fn test_add_project() {    
    // Test setup and dummy data
    let db = TemporaryDatabase::create_with_dummy().await;
    let labrinth_config = setup(&db).await;
    let app = App::new()
        .configure(|cfg | labrinth::app_config(cfg, labrinth_config.clone()));
    let test_app = test::init_service(app).await;

    // Generate project data.
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

    let (boundary, multipart) = generate_multipart(vec![
        common::actix::MultipartSegment {
            name: "data".to_string(),
            filename: None,
            content_type: Some("application/json".to_string()),
            data: common::actix::MultipartSegmentData::Text(serde_json::to_string(&json_data).unwrap())
        },
        common::actix::MultipartSegment {
            name: "basic-mod.jar".to_string(),
            filename: Some("basic-mod.jar".to_string()),
            content_type: Some("application/java-archive".to_string()),
            data: common::actix::MultipartSegmentData::Binary(include_bytes!("../tests/files/basic-mod.jar").to_vec())
        }
    ]);

    let req = test::TestRequest::post()
        .uri("/v2/project")
        .append_header(("Authorization","mrp_patuser"))
        .append_header(("Content-Type", format!("multipart/form-data; boundary={}", boundary)))
        .set_payload(multipart)
        .to_request();

    let resp = test::call_service(&test_app, req).await;

    let status = resp.status();
    assert_eq!(status, 200);

    // Get the project we just made
    let req = test::TestRequest::get()
        .uri("/v2/project/demo")
        .append_header(("Authorization","mrp_patuser"))
        .to_request();

    let resp = test::call_service(&test_app, req).await;
    assert_eq!(resp.status(), 200);

    let body : serde_json::Value = test::read_body_json(resp).await;
    let versions = body.get("versions").unwrap().as_array().unwrap();
    assert!(versions.len() == 1);

    // Reusing with a different slug and the same file should fail
    // Even if that file is named differently
    json_data["slug"] = json!("new_demo");
    json_data["initial_versions"][0]["file_parts"][0] = json!("basic-mod-different.jar");
    println!("JSON data: {:?}", json_data.to_string());
    let (boundary, multipart) = generate_multipart(vec![
        common::actix::MultipartSegment {
            name: "data".to_string(),
            filename: None,
            content_type: Some("application/json".to_string()),
            data: common::actix::MultipartSegmentData::Text(serde_json::to_string(&json_data).unwrap())
        },
        common::actix::MultipartSegment {
            name: "basic-mod-different.jar".to_string(),
            filename: Some("basic-mod-different.jar".to_string()),
            content_type: Some("application/java-archive".to_string()),
            data: common::actix::MultipartSegmentData::Binary(include_bytes!("../tests/files/basic-mod.jar").to_vec())
        }
    ]);
    let req = test::TestRequest::post()
        .uri("/v2/project")
        .append_header(("Authorization","mrp_patuser"))
        .append_header(("Content-Type", format!("multipart/form-data; boundary={}", boundary)))
        .set_payload(multipart)
        .to_request();

    let resp = test::call_service(&test_app, req).await;
    println!("Different slug,s same file (with diff name): {:?}", resp.response().body());
    println!("Response: {:?}", resp.response().body());
    assert_eq!(resp.status(), 400);

    // Reusing with the same slug and a different file should fail
    json_data["slug"] = json!("demo");
    json_data["initial_versions"][0]["file_parts"][0] = json!("basic-mod-different.jar");
    let (boundary, multipart) = generate_multipart(vec![
        common::actix::MultipartSegment {
            name: "data".to_string(),
            filename: None,
            content_type: Some("application/json".to_string()),
            data: common::actix::MultipartSegmentData::Text(serde_json::to_string(&json_data).unwrap())
        },
        common::actix::MultipartSegment {
            name: "basic-mod-different.jar".to_string(),
            filename: Some("basic-mod-different.jar".to_string()),
            content_type: Some("application/java-archive".to_string()),
            data: common::actix::MultipartSegmentData::Binary(include_bytes!("../tests/files/basic-mod-different.jar").to_vec())
        }
    ]);
    let req = test::TestRequest::post()
    .uri("/v2/project")
        .append_header(("Authorization","mrp_patuser"))
        .append_header(("Content-Type", format!("multipart/form-data; boundary={}", boundary)))
        .set_payload(multipart)
        .to_request();

    let resp = test::call_service(&test_app, req).await;
    println!("Same slug truly different file: {:?}", resp.response().body());
    println!("Response: {:?}", resp.response().body());
    assert_eq!(resp.status(), 400);
    
    // Different slug, different file should succeed
    json_data["slug"] = json!("new_demo");
    json_data["initial_versions"][0]["file_parts"][0] = json!("basic-mod-different.jar");
    let (boundary, multipart) = generate_multipart(vec![
        common::actix::MultipartSegment {
            name: "data".to_string(),
            filename: None,
            content_type: Some("application/json".to_string()),
            data: common::actix::MultipartSegmentData::Text(serde_json::to_string(&json_data).unwrap())
        },
        common::actix::MultipartSegment {
            name: "basic-mod-different.jar".to_string(),
            filename: Some("basic-mod-different.jar".to_string()),
            content_type: Some("application/java-archive".to_string()),
            data: common::actix::MultipartSegmentData::Binary(include_bytes!("../tests/files/basic-mod-different.jar").to_vec())
        }
    ]);
    let req = test::TestRequest::post()
        .uri("/v2/project")
        .append_header(("Authorization","mrp_patuser"))
        .append_header(("Content-Type", format!("multipart/form-data; boundary={}", boundary)))
        .set_payload(multipart)
        .to_request();

    let resp = test::call_service(&test_app, req).await;
    assert_eq!(resp.status(), 200);

    // Cleanup test db
    db.cleanup().await;
}
