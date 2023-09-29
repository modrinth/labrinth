use actix_web::{App, test::{self, TestRequest}, dev::ServiceResponse};
use chrono::Utc;
use common::{database::TemporaryDatabase, actix::AppendsMultipart};
use labrinth::{models::pats::Scopes, database::{self, models::generate_pat_id}};
use serde_json::json;

use crate::common::{setup, database::{USER_USER_ID, ENEMY_USER_ID, ADMIN_USER_ID, FRIEND_USER_ID, MOD_USER_ID}};

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
    let app = App::new()
        .configure(|cfg | labrinth::app_config(cfg, labrinth_config.clone()));
    let test_app = test::init_service(app).await;

    // User reading
    println!("Testing user reading...");
    let read_user = Scopes::USER_READ;
    let request_generator = || {
        test::TestRequest::get()
        .uri("/v2/user")
    };
    let read_user = test_scope(&test_app, &db, request_generator, all_scopes_except(read_user), read_user, USER_USER_ID).await;
    assert!(read_user["email"].as_str().is_none()); // email should not be present
    assert!(read_user["payout_data"].as_object().is_none()); // payout should not be present

    // Email reading
    println!("Testing email reading...");
    let read_email = Scopes::USER_READ | Scopes::USER_READ_EMAIL;
    let request_generator = || {
        test::TestRequest::get()
        .uri("/v2/user")
    };
    let read_email_test = test_scope(&test_app, &db, request_generator,  all_scopes_except(read_email), read_email, USER_USER_ID).await;
    assert_eq!(read_email_test["email"], json!("user@modrinth.com")); // email should be present

    // Payout reading
    println!("Testing payout reading...");
    let read_payout = Scopes::USER_READ | Scopes::PAYOUTS_READ;
    let request_generator = || {
        test::TestRequest::get()
        .uri("/v2/user")
    };
    let read_payout_test = test_scope(&test_app, &db, request_generator, all_scopes_except(read_payout), read_payout, USER_USER_ID).await;
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
    test_scope(&test_app, &db, request_generator, all_scopes_except(write_user), write_user, ADMIN_USER_ID).await;

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
    test_scope(&test_app, &db, request_generator, failure_write_user_payout, write_user_payout, USER_USER_ID).await;

    // User deletion
    // (The failure is first, and this is the last test for this test function, we can delete it and use the same PAT for both tests)
    println!("Testing user deletion...");
    let delete_user = Scopes::USER_DELETE;
    let request_generator = || {
        test::TestRequest::delete()
        .uri("/v2/user/enemy")
    };
    test_scope(&test_app, &db, request_generator, all_scopes_except(delete_user), delete_user, ENEMY_USER_ID).await;

    // Cleanup test db
    db.cleanup().await;
}

// Notifications
#[actix_rt::test]
pub async fn test_notifications_scopes() {
    let db = TemporaryDatabase::create_with_dummy().await;
    let labrinth_config = setup(&db).await;
    let app = App::new()
        .configure(|cfg | labrinth::app_config(cfg, labrinth_config.clone()));
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
    let request_generator = || {
        test::TestRequest::get()
        .uri("/v2/user/4/notifications")
    };
    let notifications = test_scope(&test_app, &db, request_generator, all_scopes_except(read_notifications), read_notifications, FRIEND_USER_ID).await;
    let notification_id = notifications.as_array().unwrap()[0]["id"].as_str().unwrap();

    let request_generator = || {
        test::TestRequest::get()
        .uri(&format!("/v2/notifications?ids=[{uri}]", uri=urlencoding::encode(&format!("\"{notification_id}\""))))
    };
    test_scope(&test_app, &db, request_generator, all_scopes_except(read_notifications), read_notifications, FRIEND_USER_ID).await;

    let request_generator = || {
        test::TestRequest::get()
        .uri(&format!("/v2/notification/{notification_id}"))
    };
    test_scope(&test_app, &db, request_generator, all_scopes_except(read_notifications), read_notifications, FRIEND_USER_ID).await;

    // Notification mark as read
    println!("Testing marking notifications as read...");

    let write_notifications = Scopes::NOTIFICATION_WRITE;
    let request_generator = || {
        test::TestRequest::patch()
        .uri(&format!("/v2/notifications?ids=[{uri}]", uri=urlencoding::encode(&format!("\"{notification_id}\""))))
    };
    test_scope(&test_app, &db, request_generator, all_scopes_except(write_notifications), write_notifications, FRIEND_USER_ID).await;
    let request_generator = || {
        test::TestRequest::patch()
        .uri(&format!("/v2/notification/{notification_id}"))
    };
    test_scope(&test_app, &db, request_generator, all_scopes_except(write_notifications), write_notifications, FRIEND_USER_ID).await;

    // Notification delete
    println!("Testing deleting notifications...");
    let request_generator = || {
        test::TestRequest::delete()
        .uri(&format!("/v2/notification/{notification_id}"))
    };
    test_scope(&test_app, &db, request_generator, all_scopes_except(write_notifications), write_notifications, FRIEND_USER_ID).await;

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
    let request_generator = || {
        test::TestRequest::get()
        .uri("/v2/user/2/notifications")
    };
    let notifications = test_scope(&test_app, &db, request_generator, all_scopes_except(read_notifications), read_notifications, MOD_USER_ID).await;
    let notification_id = notifications.as_array().unwrap()[0]["id"].as_str().unwrap();
    
    let request_generator = || {
        test::TestRequest::delete()
        .uri(&format!("/v2/notifications?ids=[{uri}]", uri=urlencoding::encode(&format!("\"{notification_id}\""))))
    };
    test_scope(&test_app, &db, request_generator, all_scopes_except(write_notifications), write_notifications, MOD_USER_ID).await;
      
    // Cleanup test db
    db.cleanup().await;
}


// User authentication
#[actix_rt::test]
pub async fn test_user_auth() {
    let db = TemporaryDatabase::create_with_dummy().await;
    let labrinth_config = setup(&db).await;
    let app = App::new()
        .configure(|cfg | labrinth::app_config(cfg, labrinth_config.clone()));
    let test_app = test::init_service(app).await;

    // TODO: Test user auth scopes

    // Cleanup test db
    db.cleanup().await;
}

// Project version creation scopes
#[actix_rt::test]
pub async fn test_project_version_create() {
    let db = TemporaryDatabase::create_with_dummy().await;
    let labrinth_config = setup(&db).await;
    let app = App::new()
        .configure(|cfg | labrinth::app_config(cfg, labrinth_config.clone()));
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
        data: common::actix::MultipartSegmentData::Text(serde_json::to_string(&json_data).unwrap())
    };
    let file_segment = common::actix::MultipartSegment {
        name: "basic-mod.jar".to_string(),
        filename: Some("basic-mod.jar".to_string()),
        content_type: Some("application/java-archive".to_string()),
        data: common::actix::MultipartSegmentData::Binary(include_bytes!("../tests/files/basic-mod.jar").to_vec())
    };

    let request_generator = || {
        test::TestRequest::post()
        .uri(&format!("/v2/project"))
        .set_multipart(vec![json_segment.clone(), file_segment.clone()])
    };
    let project = test_scope(&test_app, &db, request_generator, all_scopes_except(create_project), create_project, USER_USER_ID).await;
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
        data: common::actix::MultipartSegmentData::Text(serde_json::to_string(&json_data).unwrap())
    };
    let file_segment = common::actix::MultipartSegment {
        name: "basic-mod-different.jar".to_string(),
        filename: Some("basic-mod.jar".to_string()),
        content_type: Some("application/java-archive".to_string()),
        data: common::actix::MultipartSegmentData::Binary(include_bytes!("../tests/files/basic-mod-different.jar").to_vec())
    };

    let request_generator = || {
        test::TestRequest::post()
        .uri(&format!("/v2/version"))
        .set_multipart(vec![json_segment.clone(), file_segment.clone()])
    };
    test_scope(&test_app, &db, request_generator, all_scopes_except(create_version), create_version, USER_USER_ID).await;


    // Cleanup test db
    db.cleanup().await;
}


// Project scopes
// Version scopes

// Report scopes

// Thread scopes

// Pat scopes

// Session scopes

// Analytics scopes

// Collection scopes


// A reusable test that works for any scope test that:
// - returns a 401 if the scope is not present
// - returns a 200-299 if the scope is present
// - returns a JSON body on a successful request
// Some tests (ie: USER_READ_EMAIL) will still need to have additional checks (ie: email is present/absent) because it doesn't affect the response code
// test_app is the generated test app from init_service
// Closure generates a TestRequest. The authorization header (if any) will be overwritten by the generated PAT
async fn test_scope<T>(test_app : &impl actix_web::dev::Service<actix_http::Request, Response = ServiceResponse, Error = actix_web::Error>, db : &TemporaryDatabase, request_generator : T, failure_scopes: Scopes, success_scopes : Scopes, user_id : i64) -> serde_json::Value 
where T : Fn() -> TestRequest
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
    assert_eq!(resp.status(), 401);
    
    let req = request_generator()
        .append_header(("Authorization", access_token.as_str()))
        .to_request();
    let resp = test::call_service(&test_app, req).await;
    assert!(resp.status().is_success());
    let body = if resp.status() == 200 {
        test::read_body_json(resp).await
    } else {
        serde_json::Value::Null
    };
    body
}

// Creates a PAT with the given scopes, and returns the access token
// this allows us to make PATs with scopes that are not allowed to be created by PATs
async fn create_test_pat(scopes : Scopes, user_id : i64, db : &TemporaryDatabase) -> String {
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
fn all_scopes_except(success_scopes : Scopes) -> Scopes {
    Scopes::ALL ^ success_scopes
}
