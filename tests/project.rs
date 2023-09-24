use actix_web::{App, test::{self, init_service, TestRequest}, HttpResponse, web, dev::{ServiceResponse, Service}};
use common::database::TemporaryDatabase;
use serde_json::json;

use crate::common::{setup, actix::generate_multipart};

// importing common module.
mod common;

#[actix_rt::test]
async fn test_get_project() {
    let debug_time_start_0 = std::time::Instant::now();
    // Test setup and dummy data
    let db = TemporaryDatabase::create_with_dummy().await;
    let debug_time_start_1 = std::time::Instant::now();
    let labrinth_config = setup(&db).await;
    let debug_time_start_2 = std::time::Instant::now();
    println!("Setup time: {:?}", debug_time_start_2 - debug_time_start_1);

    let app = App::new()
        .configure(|cfg | labrinth::app_config(cfg, labrinth_config.clone()));
    let test_app = test::init_service(app).await;

    let debug_time_start_3 = std::time::Instant::now();
    println!("Init time: {:?}", debug_time_start_3 - debug_time_start_2);

    ///////////////////////////////////////////////
    // Perform request on dumy data
    println!("Sending request");
    let req = test::TestRequest::get()
        .uri("/v2/project/G8")
        .append_header(("Authorization","mrp_patuser"))
        .to_request();

    let debug_time_start_3_1 = std::time::Instant::now();
    let resp = test::call_service(&test_app, req).await;
    let debug_time_start_3_2 = std::time::Instant::now();
    println!("RESPONSE TIME: {:?}", debug_time_start_3_2 - debug_time_start_3_1);
    println!("Response: {:?}", resp.response().body());
    let status = resp.status();
    assert_eq!(status, 200);
    let body : serde_json::Value = test::read_body_json(resp).await;
    assert!(body.get("id").is_some());
    assert_eq!(body.get("slug").unwrap(), &json!("testslug"));

    let debug_time_start_4 = std::time::Instant::now();
    println!("Request time: {:?}", debug_time_start_4 - debug_time_start_3);

    ///////////////////////////////////////////////
    // Perform request on dumy data
    println!("///////////////////////////////////////////////////////////////");
    println!("Sending request");
    let req = test::TestRequest::get()
        .uri("/v2/project/G8")
        .append_header(("Authorization","mrp_patuser"))
        .to_request();

    let debug_time_start_3_1 = std::time::Instant::now();
    let resp = test::call_service(&test_app, req).await;
    let debug_time_start_3_2 = std::time::Instant::now();
    println!("RESPONSE TIME: {:?}", debug_time_start_3_2 - debug_time_start_3_1);
    println!("Response: {:?}", resp.response().body());
    let status = resp.status();
    assert_eq!(status, 200);
    let body : serde_json::Value = test::read_body_json(resp).await;
    assert!(body.get("id").is_some());
    assert_eq!(body.get("slug").unwrap(), &json!("testslug"));

    let debug_time_start_4 = std::time::Instant::now();
    println!("Request time: {:?}", debug_time_start_4 - debug_time_start_3);

    /////////////////////////////////////
    // Request should fail on non-existent project
    println!("Requesting non-existent project");
    let req = test::TestRequest::get()
        .uri("/v2/project/nonexistent")
        .append_header(("Authorization","mrp_patuser"))
        .to_request();

    let resp = test::call_service(&test_app, req).await;
    println!("Response: {:?}", resp.response().body());
    assert_eq!(resp.status(), 404);

    let debug_time_start_5 = std::time::Instant::now();
    println!("Request time: {:?}", debug_time_start_5 - debug_time_start_4);

    // Similarly, request should fail on non-authorized user
    println!("Requesting project as non-authorized user");
    let req = test::TestRequest::get()
    .uri("/v2/project/G8")
    .append_header(("Authorization","mrp_patenemy"))
    .to_request();

    let resp = test::call_service(&test_app, req).await;
    println!("Response: {:?}", resp.response().body());
    assert_eq!(resp.status(), 404);

    let debug_time_start_6 = std::time::Instant::now();
    println!("Request time: {:?}", debug_time_start_6 - debug_time_start_5);

    // Cleanup test db
    db.cleanup().await;

    let debug_time_start_7 = std::time::Instant::now();
    println!("Cleanup time: {:?}", debug_time_start_7 - debug_time_start_6);

    println!("Total time: {:?}", debug_time_start_7 - debug_time_start_0);
    panic!("Test panic");
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
    let jar_bytes: &[u8] = include_bytes!("../tests/files/basic-mod.jar");

    // let mut data = HashMap::new();

    let json_data = json!(
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
            data: common::actix::MultipartSegmentData::Binary(jar_bytes.to_vec())
        }
    ]);

    println!("Sending request");

    let req = test::TestRequest::post()
        .uri("/v2/project")
        .append_header(("Authorization","mrp_patuser"))
        .append_header(("Content-Type", format!("multipart/form-data; boundary={}", boundary)))
        .set_payload(multipart)
        .to_request();

    let resp = test::call_service(&test_app, req).await;

    let status = resp.status();
    println!("Response: {:?}", resp.response().body());
    println!("Response: {:?}", test::read_body(resp).await);

    assert_eq!(status, 200);

    // Cleanup test db
    db.cleanup().await;

}
