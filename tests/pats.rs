use actix_web::{
    dev::ServiceResponse,
    test::{self, TestRequest},
    App,
};
use bytes::Bytes;
use chrono::{Duration, Utc};
use common::{actix::AppendsMultipart, database::TemporaryDatabase};
use labrinth::{
    database::{self, models::generate_pat_id},
    models::pats::Scopes,
};
use serde_json::json;

use crate::common::{
    database::{ADMIN_USER_ID, ENEMY_USER_ID, FRIEND_USER_ID, MOD_USER_ID, USER_USER_ID},
    setup,
};

// importing common module.
mod common;

// For each scope, we (using test_scope):
// - create a PAT with a given set of scopes for a function
// - create a PAT with all other scopes for a function
// - test the function with the PAT with the given scopes
// - test the function with the PAT with all other scopes

// Test for users, emails, and payout scopes (not user auth scope or notifs)
#[actix_rt::test]
async fn test_user_scopes() {
    // Test setup and dummy data
    let db = TemporaryDatabase::create_with_dummy().await;
    let labrinth_config = setup(&db).await;
    let app = App::new().configure(|cfg| labrinth::app_config(cfg, labrinth_config.clone()));
    let test_app = test::init_service(app).await;

    // User reading
    println!("Testing user reading...");
    let read_user = Scopes::USER_READ;
    let request_generator = || test::TestRequest::get().uri("/v2/user");
    let (_, read_user) = test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(read_user),
        read_user,
        USER_USER_ID,
        401,
    )
    .await;
    assert!(read_user["email"].as_str().is_none()); // email should not be present
    assert!(read_user["payout_data"].as_object().is_none()); // payout should not be present

    // Email reading
    println!("Testing email reading...");
    let read_email = Scopes::USER_READ | Scopes::USER_READ_EMAIL;
    let request_generator = || test::TestRequest::get().uri("/v2/user");
    let (_, read_email_test) = test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(read_email),
        read_email,
        USER_USER_ID,
        401,
    )
    .await;
    assert_eq!(read_email_test["email"], json!("user@modrinth.com")); // email should be present

    // Payout reading
    println!("Testing payout reading...");
    let read_payout = Scopes::USER_READ | Scopes::PAYOUTS_READ;
    let request_generator = || test::TestRequest::get().uri("/v2/user");
    let (_, read_payout_test) = test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(read_payout),
        read_payout,
        USER_USER_ID,
        401,
    )
    .await;
    assert!(read_payout_test["payout_data"].as_object().is_some()); // payout should be present

    // User writing
    // We use the Admin PAT for this test, on the 'user' user
    println!("Testing user writing...");
    let write_user = Scopes::USER_WRITE;
    let request_generator = || {
        test::TestRequest::patch()
            .uri("/v2/user/user")
            .set_json(json!( {
                // Do not include 'username', as to not change the rest of the tests
                "name": "NewName",
                "bio": "New bio",
                "location": "New location",
                "role": "admin",
                "badges": 5,
                // Do not include payout info, different scope
            }))
    };
    test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(write_user),
        write_user,
        ADMIN_USER_ID,
        401,
    )
    .await;

    // User payout info writing
    println!("Testing user payout info writing...");
    let failure_write_user_payout = all_scopes_except(Scopes::PAYOUTS_WRITE); // Failure case should include USER_WRITE
    let write_user_payout = Scopes::USER_WRITE | Scopes::PAYOUTS_WRITE;
    let request_generator = || {
        test::TestRequest::patch()
            .uri("/v2/user/user")
            .set_json(json!( {
                "payout_data": {
                    "payout_wallet": "paypal",
                    "payout_wallet_type": "email",
                    "payout_address": "test@modrinth.com"
                }
            }))
    };
    test_scope(
        &test_app,
        &db,
        request_generator,
        failure_write_user_payout,
        write_user_payout,
        USER_USER_ID,
        401,
    )
    .await;

    // User deletion
    // (The failure is first, and this is the last test for this test function, we can delete it and use the same PAT for both tests)
    println!("Testing user deletion...");
    let delete_user = Scopes::USER_DELETE;
    let request_generator = || test::TestRequest::delete().uri("/v2/user/enemy");
    test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(delete_user),
        delete_user,
        ENEMY_USER_ID,
        401,
    )
    .await;

    // Cleanup test db
    db.cleanup().await;
}

// Notifications
#[actix_rt::test]
pub async fn test_notifications_scopes() {
    let db = TemporaryDatabase::create_with_dummy().await;
    let labrinth_config = setup(&db).await;
    let app = App::new().configure(|cfg| labrinth::app_config(cfg, labrinth_config.clone()));
    let test_app = test::init_service(app).await;

    // We will invite user 'friend' to project team, and use that as a notification
    // Get notifications
    let req = test::TestRequest::post()
        .uri("/v2/team/1c/members")
        .append_header(("Authorization", "mrp_patuser"))
        .set_json(json!( {
            "user_id": "4" // friend
        }))
        .to_request();
    let resp = test::call_service(&test_app, req).await;
    assert_eq!(resp.status(), 204);

    // Notification get
    println!("Testing getting notifications...");
    let read_notifications = Scopes::NOTIFICATION_READ;
    let request_generator = || test::TestRequest::get().uri("/v2/user/4/notifications");
    let (_, notifications) = test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(read_notifications),
        read_notifications,
        FRIEND_USER_ID,
        401,
    )
    .await;
    let notification_id = notifications.as_array().unwrap()[0]["id"].as_str().unwrap();

    let request_generator = || {
        test::TestRequest::get().uri(&format!(
            "/v2/notifications?ids=[{uri}]",
            uri = urlencoding::encode(&format!("\"{notification_id}\""))
        ))
    };
    test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(read_notifications),
        read_notifications,
        FRIEND_USER_ID,
        401,
    )
    .await;

    let request_generator =
        || test::TestRequest::get().uri(&format!("/v2/notification/{notification_id}"));
    test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(read_notifications),
        read_notifications,
        FRIEND_USER_ID,
        401,
    )
    .await;

    // Notification mark as read
    println!("Testing marking notifications as read...");
    let write_notifications = Scopes::NOTIFICATION_WRITE;
    let request_generator = || {
        test::TestRequest::patch().uri(&format!(
            "/v2/notifications?ids=[{uri}]",
            uri = urlencoding::encode(&format!("\"{notification_id}\""))
        ))
    };
    test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(write_notifications),
        write_notifications,
        FRIEND_USER_ID,
        401,
    )
    .await;
    let request_generator =
        || test::TestRequest::patch().uri(&format!("/v2/notification/{notification_id}"));
    test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(write_notifications),
        write_notifications,
        FRIEND_USER_ID,
        401,
    )
    .await;

    // Notification delete
    println!("Testing deleting notifications...");
    let request_generator =
        || test::TestRequest::delete().uri(&format!("/v2/notification/{notification_id}"));
    test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(write_notifications),
        write_notifications,
        FRIEND_USER_ID,
        401,
    )
    .await;

    // Mass notification delete
    // We invite mod, get the notification ID, and do mass delete using that
    let req = test::TestRequest::post()
        .uri("/v2/team/1c/members")
        .append_header(("Authorization", "mrp_patuser"))
        .set_json(json!( {
            "user_id": "2" // mod
        }))
        .to_request();
    let resp = test::call_service(&test_app, req).await;
    assert_eq!(resp.status(), 204);
    let read_notifications = Scopes::NOTIFICATION_READ;
    let request_generator = || test::TestRequest::get().uri("/v2/user/2/notifications");
    let (_, notifications) = test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(read_notifications),
        read_notifications,
        MOD_USER_ID,
        401,
    )
    .await;
    let notification_id = notifications.as_array().unwrap()[0]["id"].as_str().unwrap();

    let request_generator = || {
        test::TestRequest::delete().uri(&format!(
            "/v2/notifications?ids=[{uri}]",
            uri = urlencoding::encode(&format!("\"{notification_id}\""))
        ))
    };
    test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(write_notifications),
        write_notifications,
        MOD_USER_ID,
        401,
    )
    .await;

    // Cleanup test db
    db.cleanup().await;
}

// Project version creation scopes
#[actix_rt::test]
pub async fn test_project_version_create_scopes() {
    let db = TemporaryDatabase::create_with_dummy().await;
    let labrinth_config = setup(&db).await;
    let app = App::new().configure(|cfg| labrinth::app_config(cfg, labrinth_config.clone()));
    let test_app = test::init_service(app).await;

    // Create project
    println!("Testing creating project...");
    let create_project = Scopes::PROJECT_CREATE;
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
    let json_segment = common::actix::MultipartSegment {
        name: "data".to_string(),
        filename: None,
        content_type: Some("application/json".to_string()),
        data: common::actix::MultipartSegmentData::Text(serde_json::to_string(&json_data).unwrap()),
    };
    let file_segment = common::actix::MultipartSegment {
        name: "basic-mod.jar".to_string(),
        filename: Some("basic-mod.jar".to_string()),
        content_type: Some("application/java-archive".to_string()),
        data: common::actix::MultipartSegmentData::Binary(
            include_bytes!("../tests/files/basic-mod.jar").to_vec(),
        ),
    };

    let request_generator = || {
        test::TestRequest::post()
            .uri(&format!("/v2/project"))
            .set_multipart(vec![json_segment.clone(), file_segment.clone()])
    };
    let (_, project) = test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(create_project),
        create_project,
        USER_USER_ID,
        401,
    )
    .await;
    let project_id = project["id"].as_str().unwrap();

    // Add version to project
    println!("Testing adding version to project...");
    let create_version = Scopes::VERSION_CREATE;
    let json_data = json!(
            {
                "project_id": project_id,
                "file_parts": ["basic-mod-different.jar"],
                "version_number": "1.2.3.4",
                "version_title": "start",
                "dependencies": [],
                "game_versions": ["1.20.1"] ,
                "release_channel": "release",
                "loaders": ["fabric"],
                "featured": true
            }
    );
    let json_segment = common::actix::MultipartSegment {
        name: "data".to_string(),
        filename: None,
        content_type: Some("application/json".to_string()),
        data: common::actix::MultipartSegmentData::Text(serde_json::to_string(&json_data).unwrap()),
    };
    let file_segment = common::actix::MultipartSegment {
        name: "basic-mod-different.jar".to_string(),
        filename: Some("basic-mod.jar".to_string()),
        content_type: Some("application/java-archive".to_string()),
        data: common::actix::MultipartSegmentData::Binary(
            include_bytes!("../tests/files/basic-mod-different.jar").to_vec(),
        ),
    };

    let request_generator = || {
        test::TestRequest::post()
            .uri(&format!("/v2/version"))
            .set_multipart(vec![json_segment.clone(), file_segment.clone()])
    };
    test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(create_version),
        create_version,
        USER_USER_ID,
        401,
    )
    .await;

    // Cleanup test db
    db.cleanup().await;
}

// Project management scopes
#[actix_rt::test]
pub async fn test_project_version_reads_scopes() {
    let db = TemporaryDatabase::create_with_dummy().await;
    let labrinth_config = setup(&db).await;
    let app = App::new().configure(|cfg| labrinth::app_config(cfg, labrinth_config.clone()));
    let test_app = test::init_service(app).await;

    // Project reading
    // Uses 404 as the expected failure code (or 200 and an empty list for mass reads)
    let read_project = Scopes::PROJECT_READ;
    let request_generator = || test::TestRequest::get().uri("/v2/project/G9");
    test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(read_project),
        read_project,
        USER_USER_ID,
        404,
    )
    .await;

    let request_generator = || test::TestRequest::get().uri("/v2/project/G9/dependencies");
    test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(read_project),
        read_project,
        USER_USER_ID,
        404,
    )
    .await;

    let request_generator = || {
        test::TestRequest::get().uri(&format!(
            "/v2/projects?ids=[{uri}]",
            uri = urlencoding::encode(&format!("\"{}\"", "G9"))
        ))
    };
    let (failure, success) = test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(read_project),
        read_project,
        USER_USER_ID,
        200,
    )
    .await;
    assert!(failure.as_array().unwrap().is_empty());
    assert!(!success.as_array().unwrap().is_empty());

    // Team project reading
    let request_generator = || test::TestRequest::get().uri("/v2/project/G9/members");
    test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(read_project),
        read_project,
        USER_USER_ID,
        404,
    )
    .await;

    // Get team members
    // In this case, as these are public endpoints, logging in only is relevant to showing permissions
    // So for our test project (with 1 user, 'user') we will check the permissions before and after having the scope.
    let request_generator = || test::TestRequest::get().uri("/v2/team/1c/members");
    let (failure, success) = test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(read_project),
        read_project,
        USER_USER_ID,
        200,
    )
    .await;
    assert!(!failure.as_array().unwrap()[0].as_object().unwrap()["permissions"].is_number());
    assert!(success.as_array().unwrap()[0].as_object().unwrap()["permissions"].is_number());

    let request_generator = || {
        test::TestRequest::get().uri(&format!(
            "/v2/teams?ids=[{uri}]",
            uri = urlencoding::encode(&format!("\"{}\"", "1c"))
        ))
    };
    let (failure, success) = test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(read_project),
        read_project,
        USER_USER_ID,
        200,
    )
    .await;
    assert!(!failure.as_array().unwrap()[0].as_array().unwrap()[0]
        .as_object()
        .unwrap()["permissions"]
        .is_number());
    assert!(success.as_array().unwrap()[0].as_array().unwrap()[0]
        .as_object()
        .unwrap()["permissions"]
        .is_number());

    // User project reading
    // Test user has two projects, one public and one private
    let request_generator = || test::TestRequest::get().uri("/v2/user/3/projects");
    let (failure, success) = test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(read_project),
        read_project,
        USER_USER_ID,
        200,
    )
    .await;
    assert!(failure
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["status"] == "processing")
        .is_none());
    assert!(success
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["status"] == "processing")
        .is_some());

    // Project metadata reading
    let request_generator =
        || test::TestRequest::get().uri("/maven/maven/modrinth/G9/maven-metadata.xml");
    test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(read_project),
        read_project,
        USER_USER_ID,
        404,
    )
    .await;

    // Version reading
    // First, set version to hidden (which is when the scope is required to read it)
    let read_version = Scopes::VERSION_READ;
    let req = test::TestRequest::patch()
        .uri("/v2/version/Hl")
        .append_header(("Authorization", "mrp_patuser"))
        .set_json(json!({
            "status": "draft"
        }))
        .to_request();
    let resp = test::call_service(&test_app, req).await;
    assert_eq!(resp.status(), 204);

    let request_generator = || test::TestRequest::get().uri("/v2/version_file/111111111");
    test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(read_version),
        read_version,
        USER_USER_ID,
        404,
    )
    .await;

    let request_generator = || test::TestRequest::get().uri("/v2/version_file/111111111/download");
    test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(read_version),
        read_version,
        USER_USER_ID,
        404,
    )
    .await;

    // TODO: it's weird that this is /POST, no?
    // TODO: this scope doesn't actually affect anything, because the Project::get_id contained within disallows hidden versions, which is the point of this scope
    // let request_generator = || {
    //     test::TestRequest::post()
    //     .uri("/v2/version_file/111111111/update")
    //     .set_json(json!({}))
    // };
    // test_scope(&test_app, &db, request_generator, all_scopes_except(read_version), read_version, USER_USER_ID, 404).await;

    // TODO: this shold get, no? with query
    let request_generator = || {
        test::TestRequest::post()
            .uri("/v2/version_files")
            .set_json(json!({
                "hashes": ["111111111"]
            }))
    };
    let (failure, success) = test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(read_version),
        read_version,
        USER_USER_ID,
        200,
    )
    .await;
    assert!(!failure.as_object().unwrap().contains_key("111111111"));
    assert!(success.as_object().unwrap().contains_key("111111111"));

    // Update version file
    // TODO: weird that this is post
    // TODO: this scope doesn't actually affect anything, because the Project::get_id contained within disallows hidden versions, which is the point of this scope

    // let request_generator = || {
    //     test::TestRequest::post()
    //     .uri(&format!("/v2/version_files/update_individual"))
    //     .set_json(json!({
    //         "hashes": [{
    //             "hash": "111111111",
    //         }]
    //     }))
    // };
    // let (failure, success) = test_scope(&test_app, &db, request_generator, all_scopes_except(read_version), read_version, USER_USER_ID, 200).await;
    // assert!(!failure.as_object().unwrap().contains_key("111111111"));
    // assert!(success.as_object().unwrap().contains_key("111111111"));

    // Update version file
    // TODO: this scope doesn't actually affect anything, because the Project::get_id contained within disallows hidden versions, which is the point of this scope
    // let request_generator = || {
    //     test::TestRequest::post()
    //     .uri(&format!("/v2/version_files/update"))
    //     .set_json(json!({
    //         "hashes": ["111111111"]
    //     }))
    // };
    // let (failure, success) = test_scope(&test_app, &db, request_generator, all_scopes_except(read_version), read_version, USER_USER_ID, 200).await;
    // assert!(!failure.as_object().unwrap().contains_key("111111111"));
    // assert!(success.as_object().unwrap().contains_key("111111111"));

    // Both project and version reading
    let read_project_and_version = Scopes::PROJECT_READ | Scopes::VERSION_READ;
    let request_generator = || test::TestRequest::get().uri("/v2/project/G9/version");
    test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(read_project_and_version),
        read_project_and_version,
        USER_USER_ID,
        404,
    )
    .await;

    // TODO: fails for the same reason as above
    // let request_generator = || {
    //     test::TestRequest::get()
    //     .uri("/v2/project/G9/version/Hl")
    // };
    // test_scope(&test_app, &db, request_generator, all_scopes_except(read_project_and_version), read_project_and_version, USER_USER_ID, 404).await;

    // Cleanup test db
    db.cleanup().await;
}

// Project writing
#[actix_rt::test]
pub async fn test_project_write_scopes() {
    let db = TemporaryDatabase::create_with_dummy().await;
    let labrinth_config = setup(&db).await;
    let app = App::new().configure(|cfg| labrinth::app_config(cfg, labrinth_config.clone()));
    let test_app = test::init_service(app).await;

    // Projects writing
    let write_project = Scopes::PROJECT_WRITE;
    let request_generator = || {
        test::TestRequest::patch()
            .uri("/v2/project/G9")
            .set_json(json!(
                {
                    "title": "test_project_version_write_scopes Title",
                }
            ))
    };
    test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(write_project),
        write_project,
        USER_USER_ID,
        401,
    )
    .await;

    let request_generator = || {
        test::TestRequest::patch()
            .uri(&format!(
                "/v2/projects?ids=[{uri}]",
                uri = urlencoding::encode(&format!("\"{}\"", "G9"))
            ))
            .set_json(json!(
                {
                    "description": "test_project_version_write_scopes Description",
                }
            ))
    };
    test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(write_project),
        write_project,
        USER_USER_ID,
        401,
    )
    .await;

    let request_generator = || {
        test::TestRequest::post()
            .uri("/v2/project/G8/schedule") // G8 is an *approved* project, so we can schedule it
            .set_json(json!(
                {
                    "requested_status": "private",
                    "time": Utc::now() + Duration::days(1),
                }
            ))
    };
    test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(write_project),
        write_project,
        USER_USER_ID,
        401,
    )
    .await;

    // Icons and gallery images
    let request_generator = || {
        test::TestRequest::patch()
            .uri("/v2/project/G9/icon?ext=png")
            .set_payload(Bytes::from(
                include_bytes!("../tests/files/200x200.png") as &[u8]
            ))
    };
    test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(write_project),
        write_project,
        USER_USER_ID,
        401,
    )
    .await;

    let request_generator = || test::TestRequest::delete().uri("/v2/project/G9/icon");
    test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(write_project),
        write_project,
        USER_USER_ID,
        401,
    )
    .await;

    let request_generator = || {
        test::TestRequest::post()
            .uri("/v2/project/G9/gallery?ext=png&featured=true")
            .set_payload(Bytes::from(
                include_bytes!("../tests/files/200x200.png") as &[u8]
            ))
    };
    test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(write_project),
        write_project,
        USER_USER_ID,
        401,
    )
    .await;

    // Get project, as we need the gallery image url
    let request_generator = test::TestRequest::get()
        .uri("/v2/project/G9")
        .append_header(("Authorization", "mrp_patuser"))
        .to_request();
    let resp = test::call_service(&test_app, request_generator).await;
    let project: serde_json::Value = test::read_body_json(resp).await;
    let gallery_url = project["gallery"][0]["url"].as_str().unwrap();

    let request_generator =
        || test::TestRequest::patch().uri(&format!("/v2/project/G9/gallery?url={gallery_url}"));
    test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(write_project),
        write_project,
        USER_USER_ID,
        401,
    )
    .await;

    let request_generator =
        || test::TestRequest::delete().uri(&format!("/v2/project/G9/gallery?url={gallery_url}"));
    test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(write_project),
        write_project,
        USER_USER_ID,
        401,
    )
    .await;

    // Team scopes - add user 'friend'
    let request_generator = || {
        test::TestRequest::post()
            .uri(&format!("/v2/team/1c/members"))
            .set_json(json!({
                "user_id": "4"
            }))
    };
    test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(write_project),
        write_project,
        USER_USER_ID,
        401,
    )
    .await;

    // Accept team invite as 'friend'
    let request_generator = || test::TestRequest::post().uri(&format!("/v2/team/1c/join"));
    test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(write_project),
        write_project,
        FRIEND_USER_ID,
        401,
    )
    .await;

    // Patch 'friend' user
    let request_generator = || {
        test::TestRequest::patch()
            .uri(&format!("/v2/team/1c/members/4"))
            .set_json(json!({
                "permissions": 1
            }))
    };
    test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(write_project),
        write_project,
        USER_USER_ID,
        401,
    )
    .await;

    // Transfer ownership to 'friend'
    let request_generator = || {
        test::TestRequest::patch()
            .uri(&format!("/v2/team/1c/owner"))
            .set_json(json!({
                "user_id": "4"
            }))
    };
    test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(write_project),
        write_project,
        USER_USER_ID,
        401,
    )
    .await;

    // Now as 'friend', delete 'user'
    let request_generator = || test::TestRequest::delete().uri(&format!("/v2/team/1c/members/3"));
    test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(write_project),
        write_project,
        FRIEND_USER_ID,
        401,
    )
    .await;

    // Delete project
    // TODO: this route is currently broken,
    // because the Project::get_id contained within Project::remove doesnt include hidden versions, meaning that if there
    // is a hidden version, it will fail to delete the project (with a 500 error, as the versions of a project are not all deleted)
    // let delete_version = Scopes::PROJECT_DELETE;
    // let request_generator = || {
    //     test::TestRequest::delete()
    //     .uri(&format!("/v2/project/G9"))
    // };
    // test_scope(&test_app, &db, request_generator, all_scopes_except(delete_version), delete_version, USER_USER_ID, 401).await;

    // Cleanup test db
    db.cleanup().await;
}

// Version write
#[actix_rt::test]
pub async fn test_version_write_scopes() {
    let db = TemporaryDatabase::create_with_dummy().await;
    let labrinth_config = setup(&db).await;
    let app = App::new().configure(|cfg| labrinth::app_config(cfg, labrinth_config.clone()));
    let test_app = test::init_service(app).await;

    let write_version = Scopes::VERSION_WRITE;

    // Schedule version
    let request_generator = || {
        test::TestRequest::post()
            .uri("/v2/version/Hk/schedule") // Hk is an *approved* version, so we can schedule it
            .set_json(json!(
                {
                    "requested_status": "archived",
                    "time": Utc::now() + Duration::days(1),
                }
            ))
    };
    test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(write_version),
        write_version,
        USER_USER_ID,
        401,
    )
    .await;

    // Patch version
    let request_generator = || {
        test::TestRequest::patch()
            .uri("/v2/version/Hk")
            .set_json(json!(
                {
                    "version_title": "test_version_write_scopes Title",
                }
            ))
    };
    test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(write_version),
        write_version,
        USER_USER_ID,
        401,
    )
    .await;

    // Generate test project data.
    // Basic json
    let json_segment = common::actix::MultipartSegment {
        name: "data".to_string(),
        filename: None,
        content_type: Some("application/json".to_string()),
        data: common::actix::MultipartSegmentData::Text(
            serde_json::to_string(&json!(
                {
                    "file_types": {
                        "simple-zip.zip": "required-resource-pack"
                    },
                }
            ))
            .unwrap(),
        ),
    };

    // Differently named file, with different content
    let content_segment = common::actix::MultipartSegment {
        name: "simple-zip.zip".to_string(),
        filename: Some("simple-zip.zip".to_string()),
        content_type: Some("application/zip".to_string()),
        data: common::actix::MultipartSegmentData::Binary(
            include_bytes!("../tests/files/simple-zip.zip").to_vec(),
        ),
    };

    // Upload version file
    let request_generator = || {
        test::TestRequest::post()
            .uri(&format!("/v2/version/Hk/file"))
            .set_multipart(vec![json_segment.clone(), content_segment.clone()])
    };
    test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(write_version),
        write_version,
        USER_USER_ID,
        401,
    )
    .await;

    //  Delete version file
    // TODO: should this be VERSION_DELETE?
    let request_generator = || {
        test::TestRequest::delete().uri(&format!("/v2/version_file/000000000")) // Delete from Hk, as we uploaded to Hk, and it needs another file
    };
    test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(write_version),
        write_version,
        USER_USER_ID,
        401,
    )
    .await;

    // Delete version
    let delete_version = Scopes::VERSION_DELETE;
    let request_generator = || test::TestRequest::delete().uri(&format!("/v2/version/Hk"));
    test_scope(
        &test_app,
        &db,
        request_generator,
        all_scopes_except(delete_version),
        delete_version,
        USER_USER_ID,
        401,
    )
    .await;

    // Cleanup test db
    db.cleanup().await;
}

// Report scopes

// Thread scopes

// Session scopes

// Analytics scopes

// Collection scopes

// User authentication

// Pat scopes

// Organization scopes

// Some hash/version files functions

// Meta pat stuff

#[actix_rt::test]
pub async fn test_user_auth_scopes() {
    let db = TemporaryDatabase::create_with_dummy().await;
    let labrinth_config = setup(&db).await;
    let app = App::new().configure(|cfg| labrinth::app_config(cfg, labrinth_config.clone()));
    let test_app = test::init_service(app).await;

    // TODO: Test user auth scopes

    // Cleanup test db
    db.cleanup().await;
}

// A reusable test that works for any scope test that:
// - returns a known 'expected_failure_code' if the scope is not present (probably 401)
// - returns a 200-299 if the scope is present
// - returns the failure and success bodies for requests that are 209
// Some tests (ie: USER_READ_EMAIL) will still need to have additional checks (ie: email is present/absent) because it doesn't affect the response code
// test_app is the generated test app from init_service
// Closure generates a TestRequest. The authorization header (if any) will be overwritten by the generated PAT
async fn test_scope<T>(
    test_app: &impl actix_web::dev::Service<
        actix_http::Request,
        Response = ServiceResponse,
        Error = actix_web::Error,
    >,
    db: &TemporaryDatabase,
    request_generator: T,
    failure_scopes: Scopes,
    success_scopes: Scopes,
    user_id: i64,
    expected_failure_code: u16,
) -> (serde_json::Value, serde_json::Value)
where
    T: Fn() -> TestRequest,
{
    // First, create a PAT with all OTHER scopes
    let access_token_all_others = create_test_pat(failure_scopes, user_id, &db).await;

    // Create a PAT with the given scopes
    let access_token = create_test_pat(success_scopes, user_id, &db).await;

    // Perform test twice, once with each PAT
    // the first time, we expect a 401
    // the second time, we expect a 200 or 204, and it will return a JSON body of the response
    let req = request_generator()
        .append_header(("Authorization", access_token_all_others.as_str()))
        .to_request();
    let resp = test::call_service(&test_app, req).await;

    assert_eq!(expected_failure_code, resp.status().as_u16());
    let failure_body = if resp.status() == 200
        && resp.headers().contains_key("Content-Type")
        && resp.headers().get("Content-Type").unwrap() == "application/json"
    {
        test::read_body_json(resp).await
    } else {
        serde_json::Value::Null
    };

    let req = request_generator()
        .append_header(("Authorization", access_token.as_str()))
        .to_request();
    let resp = test::call_service(&test_app, req).await;
    println!(
        "{}: {}",
        resp.status().as_u16(),
        resp.status().canonical_reason().unwrap()
    );
    assert!(resp.status().is_success() || resp.status().is_redirection());
    let success_body = if resp.status() == 200
        && resp.headers().contains_key("Content-Type")
        && resp.headers().get("Content-Type").unwrap() == "application/json"
    {
        test::read_body_json(resp).await
    } else {
        serde_json::Value::Null
    };
    (failure_body, success_body)
}

// Creates a PAT with the given scopes, and returns the access token
// this allows us to make PATs with scopes that are not allowed to be created by PATs
async fn create_test_pat(scopes: Scopes, user_id: i64, db: &TemporaryDatabase) -> String {
    let mut transaction = db.pool.begin().await.unwrap();
    let id = generate_pat_id(&mut transaction).await.unwrap();
    let pat = database::models::pat_item::PersonalAccessToken {
        id,
        name: format!("test_pat_{}", scopes.bits()),
        access_token: format!("mrp_{}", id.0),
        scopes,
        user_id: database::models::ids::UserId(user_id),
        created: Utc::now(),
        expires: Utc::now() + chrono::Duration::days(1),
        last_used: None,
    };
    pat.insert(&mut transaction).await.unwrap();
    transaction.commit().await.unwrap();
    pat.access_token
}

// Inversion of scopes for testing
// ie: To ensure that ONLY this scope is required, we need to create a PAT with all other scopes
fn all_scopes_except(success_scopes: Scopes) -> Scopes {
    Scopes::ALL ^ success_scopes
}
