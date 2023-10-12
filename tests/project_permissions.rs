use std::sync::Arc;

use actix_web::test;
use bytes::Bytes;
use chrono::{Duration, Utc};
use common::{permissions::{PermissionsTestContext, PermissionsTest}, database::{generate_random_name, FRIEND_USER_ID, FRIEND_USER_PAT, USER_USER_PAT}, actix::{AppendsMultipart, MultipartSegment}};
use labrinth::models::teams::ProjectPermissions;
use serde_json::json;

use crate::common::{environment::TestEnvironment, database::{MOD_USER_PAT, MOD_USER_ID, ADMIN_USER_PAT}};
use futures::stream::StreamExt;

mod common;

#[actix_rt::test]
async fn patch_project_permissions() {
    let test_env = TestEnvironment::build(Some(8)).await;
    let alpha_project_id = &test_env.dummy.as_ref().unwrap().alpha_project_id;
    let alpha_team_id = &test_env.dummy.as_ref().unwrap().alpha_team_id;
    
    // For each permission covered by EDIT_DETAILS, ensure the permission is required
    let edit_details = ProjectPermissions::EDIT_DETAILS;
    let test_pairs = [
        // Body, status, requested_status tested separately
        ("slug", json!("")), // generated in the test to not collide slugs
        ("title", json!("randomname")),
        ("description", json!("randomdescription")),
        ("categories", json!(["combat", "economy"])),
        ("client_side", json!("unsupported")),
        ("server_side", json!("unsupported")),
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

    let arc = Arc::new(&test_env);
    futures::stream::iter(test_pairs).map(|(key, value)| {
        let test_env = arc.clone();
        async move {
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
            .simple_project_permissions_test(edit_details, req_gen).await.unwrap();
        }
    }).buffer_unordered(4).collect::<Vec<_>>().await;

    // Test with status and requested_status
    // This requires a project with a version, so we use alpha_project_id
    let req_gen = |ctx: &PermissionsTestContext| test::TestRequest::patch()
    .uri(&format!("/v2/project/{}", ctx.project_id.unwrap()))
    .set_json(json!({
        "status": "private",
        "requested_status": "private",
    }));
    PermissionsTest::new(&test_env)
    .with_existing_project(alpha_project_id,alpha_team_id)
    .with_user(FRIEND_USER_ID, FRIEND_USER_PAT, true)
    .simple_project_permissions_test(edit_details, req_gen).await.unwrap();

    // Bulk patch projects
    let req_gen = |ctx: &PermissionsTestContext| test::TestRequest::patch()
    .uri(
        &format!("/v2/projects?ids=[{uri}]",
         uri = urlencoding::encode(&format!("\"{}\"", ctx.project_id.unwrap()))))
    .set_json(json!({
        "name": "randomname",
    }));
    PermissionsTest::new(&test_env)
        .simple_project_permissions_test(edit_details, req_gen).await.unwrap();


    // Edit body
    // Cannot bulk edit body
    let edit_body = ProjectPermissions::EDIT_BODY;
    let req_gen = |ctx: &PermissionsTestContext| test::TestRequest::patch()
    .uri(&format!("/v2/project/{}", ctx.project_id.unwrap()))
    .set_json(json!({
        "body": "new body!",
    }));
    PermissionsTest::new(&test_env)
    .simple_project_permissions_test(edit_body, req_gen).await.unwrap();

    test_env.cleanup().await;
}

// Not covered by PATCH /project
#[actix_rt::test]
async fn edit_details() {
    let test_env = TestEnvironment::build(None).await;
   
    let alpha_project_id = &test_env.dummy.as_ref().unwrap().alpha_project_id;
    let alpha_team_id = &test_env.dummy.as_ref().unwrap().alpha_team_id;
    let beta_project_id = &test_env.dummy.as_ref().unwrap().beta_project_id;
    let beta_team_id = &test_env.dummy.as_ref().unwrap().beta_team_id;
    let beta_version_id = &test_env.dummy.as_ref().unwrap().beta_version_id;
    let edit_details = ProjectPermissions::EDIT_DETAILS;

    // Approve beta version as private so we can schedule it
    let req = test::TestRequest::patch()
    .uri(&format!("/v2/version/{beta_version_id}"))
    .append_header(("Authorization", MOD_USER_PAT))
        .set_json(json!({
            "status": "unlisted"
        }))
        .to_request();
    let resp = test_env.call(req).await;
    assert_eq!(resp.status(), 204);

    // Schedule version
    let req_gen = |_: &PermissionsTestContext| {
        test::TestRequest::post()
            .uri(&format!("/v2/version/{beta_version_id}/schedule")) // beta_version_id is an *approved* version, so we can schedule it
            .set_json(json!(
                {
                    "requested_status": "archived",
                    "time": Utc::now() + Duration::days(1),
                }
            ))
    };
    PermissionsTest::new(&test_env)
    .with_existing_project(beta_project_id, beta_team_id)
    .with_user(FRIEND_USER_ID, FRIEND_USER_PAT, true)
    .simple_project_permissions_test(edit_details, req_gen).await.unwrap();

    // Icon edit
    // Uses alpha project to delete this icon
    let req_gen = |ctx: &PermissionsTestContext| test::TestRequest::patch()
            .uri(&format!("/v2/project/{}/icon?ext=png", ctx.project_id.unwrap()))
            .set_payload(Bytes::from(
                include_bytes!("../tests/files/200x200.png") as &[u8]
            ));
    PermissionsTest::new(&test_env)
    .with_existing_project(alpha_project_id,alpha_team_id)
    .with_user(FRIEND_USER_ID, FRIEND_USER_PAT, true)

    .simple_project_permissions_test(edit_details, req_gen).await.unwrap();
            
    // Icon delete
    // Uses alpha project to delete added icon
    let req_gen = |ctx: &PermissionsTestContext| test::TestRequest::delete()
    .uri(&format!("/v2/project/{}/icon?ext=png", ctx.project_id.unwrap()));
    PermissionsTest::new(&test_env)
    .with_existing_project(alpha_project_id,alpha_team_id)
    .with_user(FRIEND_USER_ID, FRIEND_USER_PAT, true)

    .simple_project_permissions_test(edit_details, req_gen).await.unwrap();

    // Add gallery item
    // Uses alpha project to add gallery item so we can get its url
    let req_gen = |ctx: &PermissionsTestContext| {
        test::TestRequest::post()
            .uri(&format!(
                "/v2/project/{}/gallery?ext=png&featured=true",
                ctx.project_id.unwrap()
            ))
            .set_payload(Bytes::from(
                include_bytes!("../tests/files/200x200.png") as &[u8]
            ))
    };
    PermissionsTest::new(&test_env)
    .with_existing_project(alpha_project_id,alpha_team_id)
    .with_user(FRIEND_USER_ID, FRIEND_USER_PAT, true)
.simple_project_permissions_test(edit_details, req_gen)
        .await
        .unwrap();
        // Get project, as we need the gallery image url
    let req = test::TestRequest::get()
        .uri(&format!("/v2/project/{alpha_project_id}"))
        .append_header(("Authorization", USER_USER_PAT))
        .to_request();
    let resp = test_env.call(req).await;
    let project: serde_json::Value = test::read_body_json(resp).await;
    let gallery_url = project["gallery"][0]["url"].as_str().unwrap();


    // Edit gallery item
    // Uses alpha project to edit gallery item
    let req_gen = |ctx: &PermissionsTestContext| {
        test::TestRequest::patch().uri(&format!(
            "/v2/project/{}/gallery?url={gallery_url}", ctx.project_id.unwrap()
        ))
    };
    PermissionsTest::new(&test_env)
    .with_existing_project(alpha_project_id,alpha_team_id)
    .with_user(FRIEND_USER_ID, FRIEND_USER_PAT, true)
.simple_project_permissions_test(edit_details, req_gen)
        .await
        .unwrap();

    // Remove gallery item
    // Uses alpha project to remove gallery item
    let req_gen = |ctx: &PermissionsTestContext| {
        test::TestRequest::delete().uri(&format!(
            "/v2/project/{}/gallery?url={gallery_url}", ctx.project_id.unwrap()
        ))
    };
    PermissionsTest::new(&test_env)
    .with_existing_project(alpha_project_id,alpha_team_id)
    .with_user(FRIEND_USER_ID, FRIEND_USER_PAT, true)
.simple_project_permissions_test(edit_details, req_gen)
        .await
        .unwrap();


}

#[actix_rt::test]
async fn upload_version() {
    let test_env = TestEnvironment::build(None).await;
    let alpha_project_id = &test_env.dummy.as_ref().unwrap().alpha_project_id;
    let alpha_version_id = &test_env.dummy.as_ref().unwrap().alpha_version_id;
    let alpha_team_id = &test_env.dummy.as_ref().unwrap().alpha_team_id;
    let alpha_file_hash = &test_env.dummy.as_ref().unwrap().alpha_file_hash;

    let upload_version = ProjectPermissions::UPLOAD_VERSION;

    // Upload version with basic-mod.jar
    let req_gen = |ctx: &PermissionsTestContext| test::TestRequest::post()
    .uri(&format!("/v2/version"))
    .set_multipart([
        MultipartSegment {
            name: "data".to_string(),
            filename: None,
            content_type: Some("application/json".to_string()),
            data: common::actix::MultipartSegmentData::Text(serde_json::to_string(&json!({
                "project_id": ctx.project_id.unwrap(),
                "file_parts": ["basic-mod.jar"],
                "version_number": "1.0.0",
                "version_title": "1.0.0",
                "version_type": "release",
                "dependencies": [],
                "game_versions": ["1.20.1"],
                "loaders": ["fabric"],
                "featured": false,
                
            })).unwrap()),
        },
        MultipartSegment {
            name: "basic-mod.jar".to_string(),
            filename: Some("basic-mod.jar".to_string()),
            content_type: Some("application/java-archive".to_string()),
            data: common::actix::MultipartSegmentData::Binary(
                include_bytes!("../tests/files/basic-mod.jar").to_vec(),
            ),
        }
    ]);
    PermissionsTest::new(&test_env)
    .simple_project_permissions_test(upload_version, req_gen).await.unwrap();

    // Upload file to existing version
    // Uses alpha project, as it has an existing version
    let req_gen = |_: &PermissionsTestContext| test::TestRequest::post()
    .uri(&format!("/v2/version/{}/file", alpha_version_id))
    .set_multipart([
        MultipartSegment {
            name: "data".to_string(),
            filename: None,
            content_type: Some("application/json".to_string()),
            data: common::actix::MultipartSegmentData::Text(serde_json::to_string(&json!({
                "file_parts": ["basic-mod-different.jar"],                
            })).unwrap()),
        },
        MultipartSegment {
            name: "basic-mod-different.jar".to_string(),
            filename: Some("basic-mod-different.jar".to_string()),
            content_type: Some("application/java-archive".to_string()),
            data: common::actix::MultipartSegmentData::Binary(
                include_bytes!("../tests/files/basic-mod-different.jar").to_vec(),
            ),
        }
    ]);
    PermissionsTest::new(&test_env)
    .with_existing_project(alpha_project_id, alpha_team_id)
    .with_user(FRIEND_USER_ID, FRIEND_USER_PAT, true)
    .simple_project_permissions_test(upload_version, req_gen).await.unwrap();

    // Patch version
    // Uses alpha project, as it has an existing version
    let req_gen = |_: &PermissionsTestContext| test::TestRequest::patch()
    .uri(&format!("/v2/version/{}", alpha_version_id))
    .set_json(json!({
        "name": "Basic Mod",
    }));
    PermissionsTest::new(&test_env)
    .with_existing_project(alpha_project_id, alpha_team_id)
    .with_user(FRIEND_USER_ID, FRIEND_USER_PAT, true)
    .simple_project_permissions_test(upload_version, req_gen).await.unwrap();

    // Delete version file
    // Uses alpha project, as it has an existing version
    let delete_version = ProjectPermissions::DELETE_VERSION;
    let req_gen = |_: &PermissionsTestContext| 
    test::TestRequest::delete()
    .uri(&format!("/v2/version_file/{}", alpha_file_hash));
    
    PermissionsTest::new(&test_env)
    .with_existing_project(alpha_project_id, alpha_team_id)
    .with_user(FRIEND_USER_ID, FRIEND_USER_PAT, true)
    .simple_project_permissions_test(delete_version, req_gen).await.unwrap();

    // Delete version
    // Uses alpha project, as it has an existing version
    let req_gen = |_: &PermissionsTestContext| test::TestRequest::delete()
    .uri(&format!("/v2/version/{}", alpha_version_id));
    PermissionsTest::new(&test_env)
    .with_existing_project(alpha_project_id, alpha_team_id)
    .with_user(FRIEND_USER_ID, FRIEND_USER_PAT, true)
    .simple_project_permissions_test(delete_version, req_gen).await.unwrap();


    test_env.cleanup().await;
}

#[actix_rt::test]
async fn manage_invites() {
    // Add member, remove member, edit member
    let test_env = TestEnvironment::build(None).await;
    let alpha_project_id = &test_env.dummy.as_ref().unwrap().alpha_project_id;
    let alpha_team_id = &test_env.dummy.as_ref().unwrap().alpha_team_id;

    let manage_invites = ProjectPermissions::MANAGE_INVITES;

    // Add member
    let req_gen = |ctx: &PermissionsTestContext| test::TestRequest::post()
    .uri(&format!("/v2/team/{}/members", ctx.team_id.unwrap()))
    .set_json(json!({
        "user_id": MOD_USER_ID,
        "permissions": 0,
    }));
    PermissionsTest::new(&test_env)
    .with_existing_project(alpha_project_id, alpha_team_id)
    .with_user(FRIEND_USER_ID, FRIEND_USER_PAT, true)
    .simple_project_permissions_test(manage_invites, req_gen).await.unwrap();

    // Edit member
    let edit_member = ProjectPermissions::EDIT_MEMBER;
    let req_gen = |ctx: &PermissionsTestContext| test::TestRequest::patch()
    .uri(&format!("/v2/team/{}/members/{MOD_USER_ID}", ctx.team_id.unwrap()))
    .set_json(json!({
        "permissions": 0,
    }));
    PermissionsTest::new(&test_env)
    .with_existing_project(alpha_project_id, alpha_team_id)
    .with_user(FRIEND_USER_ID, FRIEND_USER_PAT, true)
    .simple_project_permissions_test(edit_member, req_gen).await.unwrap();

    // remove member
    // requires manage_invites if they have not yet accepted the invite
    let req_gen = |ctx: &PermissionsTestContext| test::TestRequest::delete()
    .uri(&format!("/v2/team/{}/members/{MOD_USER_ID}", ctx.team_id.unwrap()));
    PermissionsTest::new(&test_env)
    .with_existing_project(alpha_project_id, alpha_team_id)
    .with_user(FRIEND_USER_ID, FRIEND_USER_PAT, true)
    .simple_project_permissions_test(manage_invites, req_gen).await.unwrap();

    // re-add member for testing
    let req = test::TestRequest::post()
    .uri(&format!("/v2/team/{}/members", alpha_team_id))
    .append_header(("Authorization", ADMIN_USER_PAT))
    .set_json(json!({
        "user_id": MOD_USER_ID,
    })).to_request();
    let resp = test_env.call(req).await;
    assert_eq!(resp.status(), 204);

    // Accept invite
    let req = test::TestRequest::post()
    .uri(&format!("/v2/team/{}/join", alpha_team_id))
    .append_header(("Authorization", MOD_USER_PAT)).to_request();
    let resp = test_env.call(req).await;
    assert_eq!(resp.status(), 204);

    // remove existing member (requires remove_member)
    let remove_member = ProjectPermissions::REMOVE_MEMBER;
    let req_gen = |ctx: &PermissionsTestContext| test::TestRequest::delete()
    .uri(&format!("/v2/team/{}/members/{MOD_USER_ID}", ctx.team_id.unwrap()));

    PermissionsTest::new(&test_env)
    .with_existing_project(alpha_project_id, alpha_team_id)
    .with_user(FRIEND_USER_ID, FRIEND_USER_PAT, true)
    .simple_project_permissions_test(remove_member, req_gen).await.unwrap();

    test_env.cleanup().await;

}

#[actix_rt::test]
async fn delete_project() {
    // Add member, remove member, edit member
    let test_env = TestEnvironment::build(None).await;

    let delete_project = ProjectPermissions::DELETE_PROJECT;

    // Delete project
    let req_gen = |ctx: &PermissionsTestContext| test::TestRequest::delete()
    .uri(&format!("/v2/project/{}", ctx.project_id.unwrap()));
    PermissionsTest::new(&test_env)
    .simple_project_permissions_test(delete_project, req_gen).await.unwrap();

    test_env.cleanup().await;
}

// TODO: VIEW_PAYOUTS currently is unused. Add tests when it is used.
// TODO: VIEW_ANALYTICS currently is unused. Add tests when it is used.
