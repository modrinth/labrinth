use crate::common::{
    api_common::ApiProject,
    api_v2::ApiV2,
    database::{FRIEND_USER_ID, FRIEND_USER_PAT, USER_USER_PAT},
    dummy_data::TestFile,
    environment::{with_test_environment, TestEnvironment},
    permissions::{PermissionsTest, PermissionsTestContext},
};
use actix_web::test;
use itertools::Itertools;
use labrinth::{
    database::models::project_item::PROJECTS_SLUGS_NAMESPACE,
    models::teams::ProjectPermissions,
    util::actix::{AppendsMultipart, MultipartSegment, MultipartSegmentData},
};
use serde_json::json;

#[actix_rt::test]
async fn test_project_type_sanity() {
    with_test_environment(None, |test_env: TestEnvironment<ApiV2>| async move {
        let api = &test_env.api;

        // Perform all other patch tests on both 'mod' and 'modpack'
        for (mod_or_modpack, slug, file) in [
            ("mod", "test-mod", TestFile::build_random_jar()),
            ("modpack", "test-modpack", TestFile::build_random_mrpack()),
        ] {
            let (test_project, test_version) = api
                .add_public_project(slug, Some(file), None, USER_USER_PAT)
                .await;
            let test_project_slug = test_project.slug.as_ref().unwrap();

            // TODO:
            // assert_eq!(test_project.project_type, mod_or_modpack);
            assert_eq!(test_project.loaders, vec!["fabric"]);
            assert_eq!(test_version[0].loaders, vec!["fabric"]);

            let project = api
                .get_project_deserialized(test_project_slug, USER_USER_PAT)
                .await;
            assert_eq!(test_project.loaders, vec!["fabric"]);
            assert_eq!(project.project_type, mod_or_modpack);

            let version = api
                .get_version_deserialized(&test_version[0].id.to_string(), USER_USER_PAT)
                .await;
            assert_eq!(
                version.loaders.iter().map(|x| &x.0).collect_vec(),
                vec!["fabric"]
            );
        }

        // TODO: as we get more complicated strucures with v3 testing, and alpha/beta get more complicated, we should add more tests here,
        // to ensure that projects created with v3 routes are still valid and work with v3 routes.
    })
    .await;
}

#[actix_rt::test]
async fn test_add_remove_project() {
    // Test setup and dummy data
    with_test_environment(None, |test_env: TestEnvironment<ApiV2>| async move {
        let api = &test_env.api;

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
        let json_segment = MultipartSegment {
            name: "data".to_string(),
            filename: None,
            content_type: Some("application/json".to_string()),
            data: MultipartSegmentData::Text(serde_json::to_string(&json_data).unwrap()),
        };

        // Basic json, with a different file
        json_data["initial_versions"][0]["file_parts"][0] = json!("basic-mod-different.jar");
        let json_diff_file_segment = MultipartSegment {
            data: MultipartSegmentData::Text(serde_json::to_string(&json_data).unwrap()),
            ..json_segment.clone()
        };

        // Basic json, with a different file, and a different slug
        json_data["slug"] = json!("new_demo");
        json_data["initial_versions"][0]["file_parts"][0] = json!("basic-mod-different.jar");
        let json_diff_slug_file_segment = MultipartSegment {
            data: MultipartSegmentData::Text(serde_json::to_string(&json_data).unwrap()),
            ..json_segment.clone()
        };

        // Basic file
        let file_segment = MultipartSegment {
            name: "basic-mod.jar".to_string(),
            filename: Some("basic-mod.jar".to_string()),
            content_type: Some("application/java-archive".to_string()),
            // TODO: look at these: can be simplified with TestFile
            data: MultipartSegmentData::Binary(
                include_bytes!("../../tests/files/basic-mod.jar").to_vec(),
            ),
        };

        // Differently named file, with the same content (for hash testing)
        let file_diff_name_segment = MultipartSegment {
            name: "basic-mod-different.jar".to_string(),
            filename: Some("basic-mod-different.jar".to_string()),
            content_type: Some("application/java-archive".to_string()),
            data: MultipartSegmentData::Binary(
                include_bytes!("../../tests/files/basic-mod.jar").to_vec(),
            ),
        };

        // Differently named file, with different content
        let file_diff_name_content_segment = MultipartSegment {
            name: "basic-mod-different.jar".to_string(),
            filename: Some("basic-mod-different.jar".to_string()),
            content_type: Some("application/java-archive".to_string()),
            data: MultipartSegmentData::Binary(
                include_bytes!("../../tests/files/basic-mod-different.jar").to_vec(),
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
        let hash = sha1::Sha1::from(include_bytes!("../../tests/files/basic-mod.jar"))
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
        let resp = test_env.api.remove_project("demo", USER_USER_PAT).await;
        assert_eq!(resp.status(), 204);

        // Confirm that the project is gone from the cache
        let mut redis_conn = test_env.db.redis_pool.connect().await.unwrap();
        assert_eq!(
            redis_conn
                .get(PROJECTS_SLUGS_NAMESPACE, "demo")
                .await
                .unwrap()
                .map(|x| x.parse::<i64>().unwrap()),
            None
        );
        assert_eq!(
            redis_conn
                .get(PROJECTS_SLUGS_NAMESPACE, &id)
                .await
                .unwrap()
                .map(|x| x.parse::<i64>().unwrap()),
            None
        );

        // Old slug no longer works
        let resp = api.get_project("demo", USER_USER_PAT).await;
        assert_eq!(resp.status(), 404);
    })
    .await;
}

#[actix_rt::test]
async fn permissions_upload_version() {
    with_test_environment(None, |test_env: TestEnvironment<ApiV2>| async move {
        let alpha_project_id = &test_env.dummy.as_ref().unwrap().project_alpha.project_id;
        let alpha_version_id = &test_env.dummy.as_ref().unwrap().project_alpha.version_id;
        let alpha_team_id = &test_env.dummy.as_ref().unwrap().project_alpha.team_id;
        let alpha_file_hash = &test_env.dummy.as_ref().unwrap().project_alpha.file_hash;

        let upload_version = ProjectPermissions::UPLOAD_VERSION;

        // Upload version with basic-mod.jar
        let req_gen = |ctx: &PermissionsTestContext| {
            test::TestRequest::post().uri("/v2/version").set_multipart([
                MultipartSegment {
                    name: "data".to_string(),
                    filename: None,
                    content_type: Some("application/json".to_string()),
                    data: MultipartSegmentData::Text(
                        serde_json::to_string(&json!({
                            "project_id": ctx.project_id.unwrap(),
                            "file_parts": ["basic-mod.jar"],
                            "version_number": "1.0.0",
                            "version_title": "1.0.0",
                            "version_type": "release",
                            "dependencies": [],
                            "game_versions": ["1.20.1"],
                            "loaders": ["fabric"],
                            "featured": false,

                        }))
                        .unwrap(),
                    ),
                },
                MultipartSegment {
                    name: "basic-mod.jar".to_string(),
                    filename: Some("basic-mod.jar".to_string()),
                    content_type: Some("application/java-archive".to_string()),
                    data: MultipartSegmentData::Binary(
                        include_bytes!("../../tests/files/basic-mod.jar").to_vec(),
                    ),
                },
            ])
        };
        PermissionsTest::new(&test_env)
            .simple_project_permissions_test(upload_version, req_gen)
            .await
            .unwrap();

        // Upload file to existing version
        // Uses alpha project, as it has an existing version
        let req_gen = |_: &PermissionsTestContext| {
            test::TestRequest::post()
                .uri(&format!("/v2/version/{}/file", alpha_version_id))
                .set_multipart([
                    MultipartSegment {
                        name: "data".to_string(),
                        filename: None,
                        content_type: Some("application/json".to_string()),
                        data: MultipartSegmentData::Text(
                            serde_json::to_string(&json!({
                                "file_parts": ["basic-mod-different.jar"],
                            }))
                            .unwrap(),
                        ),
                    },
                    MultipartSegment {
                        name: "basic-mod-different.jar".to_string(),
                        filename: Some("basic-mod-different.jar".to_string()),
                        content_type: Some("application/java-archive".to_string()),
                        data: MultipartSegmentData::Binary(
                            include_bytes!("../../tests/files/basic-mod-different.jar").to_vec(),
                        ),
                    },
                ])
        };
        PermissionsTest::new(&test_env)
            .with_existing_project(alpha_project_id, alpha_team_id)
            .with_user(FRIEND_USER_ID, FRIEND_USER_PAT, true)
            .simple_project_permissions_test(upload_version, req_gen)
            .await
            .unwrap();

        // Patch version
        // Uses alpha project, as it has an existing version
        let req_gen = |_: &PermissionsTestContext| {
            test::TestRequest::patch()
                .uri(&format!("/v2/version/{}", alpha_version_id))
                .set_json(json!({
                    "name": "Basic Mod",
                }))
        };
        PermissionsTest::new(&test_env)
            .with_existing_project(alpha_project_id, alpha_team_id)
            .with_user(FRIEND_USER_ID, FRIEND_USER_PAT, true)
            .simple_project_permissions_test(upload_version, req_gen)
            .await
            .unwrap();

        // Delete version file
        // Uses alpha project, as it has an existing version
        let delete_version = ProjectPermissions::DELETE_VERSION;
        let req_gen = |_: &PermissionsTestContext| {
            test::TestRequest::delete().uri(&format!("/v2/version_file/{}", alpha_file_hash))
        };

        PermissionsTest::new(&test_env)
            .with_existing_project(alpha_project_id, alpha_team_id)
            .with_user(FRIEND_USER_ID, FRIEND_USER_PAT, true)
            .simple_project_permissions_test(delete_version, req_gen)
            .await
            .unwrap();

        // Delete version
        // Uses alpha project, as it has an existing version
        let req_gen = |_: &PermissionsTestContext| {
            test::TestRequest::delete().uri(&format!("/v2/version/{}", alpha_version_id))
        };
        PermissionsTest::new(&test_env)
            .with_existing_project(alpha_project_id, alpha_team_id)
            .with_user(FRIEND_USER_ID, FRIEND_USER_PAT, true)
            .simple_project_permissions_test(delete_version, req_gen)
            .await
            .unwrap();
    })
    .await;
}

#[actix_rt::test]
pub async fn test_patch_v2() {
    // Hits V3-specific patchable fields
    // Other fields are tested in test_patch_project (the v2 version of that test)
    with_test_environment(None, |test_env : TestEnvironment<ApiV2>| async move {
        let api = &test_env.api;

        let alpha_project_slug = &test_env.dummy.as_ref().unwrap().project_alpha.project_slug;

        // Sucessful request to patch many fields.
        let resp = api
            .edit_project(
                alpha_project_slug,
                json!({
                    "client_side": "optional",
                    "server_side": "required",
                }),
                USER_USER_PAT,
            )
            .await;
        assert_eq!(resp.status(), 204);

        let project = api
            .get_project_deserialized(alpha_project_slug, USER_USER_PAT)
            .await;

            assert_eq!(project.client_side.as_str(), "optional");
            assert_eq!(project.server_side.as_str(), "required");
        }).await;
}

