use crate::common::api_common::ApiProject;
use crate::common::api_v2::ApiV2;
use crate::common::dummy_data::TestFile;
use crate::common::environment::with_test_environment;
use crate::common::environment::TestEnvironment;
use crate::common::scopes::ScopeTest;
use actix_web::test;
use labrinth::models::pats::Scopes;
use labrinth::util::actix::AppendsMultipart;
use labrinth::util::actix::MultipartSegment;
use labrinth::util::actix::MultipartSegmentData;

use serde_json::json;
// Project version creation scopes
#[actix_rt::test]
pub async fn project_version_create_scopes() {
    with_test_environment(None, |test_env: TestEnvironment<ApiV2>| async move {
        // Create project
        let api = &test_env.api;
        let create_project = Scopes::PROJECT_CREATE;
        let json_data = api
            .get_public_project_creation_data_json("demo", Some(&TestFile::BasicMod))
            .await;
        let json_segment = MultipartSegment {
            name: "data".to_string(),
            filename: None,
            content_type: Some("application/json".to_string()),
            data: MultipartSegmentData::Text(serde_json::to_string(&json_data).unwrap()),
        };
        let file_segment = MultipartSegment {
            name: "basic-mod.jar".to_string(),
            filename: Some("basic-mod.jar".to_string()),
            content_type: Some("application/java-archive".to_string()),
            data: MultipartSegmentData::Binary(
                include_bytes!("../../tests/files/basic-mod.jar").to_vec(),
            ),
        };

        let req_gen = || {
            test::TestRequest::post()
                .uri("/v2/project")
                .set_multipart(vec![json_segment.clone(), file_segment.clone()])
        };
        let (_, success) = ScopeTest::new(&test_env)
            .test(req_gen, create_project)
            .await
            .unwrap();
        let project_id = success["id"].as_str().unwrap();

        // Add version to project
        let create_version = Scopes::VERSION_CREATE;
        let json_data = json!(
                {
                    "project_id": project_id,
                    "file_parts": ["basic-mod-different.jar"],
                    "version_number": "1.2.3.4",
                    "version_title": "start",
                    "dependencies": [],
                    "game_versions": ["1.20.1"] ,
                    "client_side": "required",
                    "server_side": "optional",
                    "release_channel": "release",
                    "loaders": ["fabric"],
                    "featured": true
                }
        );
        let json_segment = MultipartSegment {
            name: "data".to_string(),
            filename: None,
            content_type: Some("application/json".to_string()),
            data: MultipartSegmentData::Text(serde_json::to_string(&json_data).unwrap()),
        };
        let file_segment = MultipartSegment {
            name: "basic-mod-different.jar".to_string(),
            filename: Some("basic-mod.jar".to_string()),
            content_type: Some("application/java-archive".to_string()),
            data: MultipartSegmentData::Binary(
                include_bytes!("../../tests/files/basic-mod-different.jar").to_vec(),
            ),
        };

        let req_gen = || {
            test::TestRequest::post()
                .uri("/v2/version")
                .set_multipart(vec![json_segment.clone(), file_segment.clone()])
        };
        ScopeTest::new(&test_env)
            .test(req_gen, create_version)
            .await
            .unwrap();
    })
    .await;
}

#[actix_rt::test]
pub async fn project_version_create_scopes_v2() {
    with_test_environment(None, |test_env: TestEnvironment<ApiV2>| async move {
        // TODO: If possible, find a way to use generic api functions with the Permissions/Scopes test, then this can be recombined with the V2 version of this test
        let api = &test_env.api;

        // Create project
        let create_project = Scopes::PROJECT_CREATE;
        let json_data = api
            .get_public_project_creation_data_json("demo", Some(&TestFile::BasicMod))
            .await;
        let json_segment = MultipartSegment {
            name: "data".to_string(),
            filename: None,
            content_type: Some("application/json".to_string()),
            data: MultipartSegmentData::Text(serde_json::to_string(&json_data).unwrap()),
        };
        let file_segment = MultipartSegment {
            name: "basic-mod.jar".to_string(),
            filename: Some("basic-mod.jar".to_string()),
            content_type: Some("application/java-archive".to_string()),
            data: MultipartSegmentData::Binary(
                include_bytes!("../../tests/files/basic-mod.jar").to_vec(),
            ),
        };

        let req_gen = || {
            test::TestRequest::post()
                .uri("/v2/project")
                .set_multipart(vec![json_segment.clone(), file_segment.clone()])
        };
        let (_, success) = ScopeTest::new(&test_env)
            .test(req_gen, create_project)
            .await
            .unwrap();
        let project_id = success["id"].as_str().unwrap();

        // Add version to project
        let create_version = Scopes::VERSION_CREATE;
        let json_data = json!(
                {
                    "project_id": project_id,
                    "file_parts": ["basic-mod-different.jar"],
                    "version_number": "1.2.3.4",
                    "version_title": "start",
                    "dependencies": [],
                    "game_versions": ["1.20.1"] ,
                    "client_side": "required",
                    "server_side": "optional",
                    "release_channel": "release",
                    "loaders": ["fabric"],
                    "featured": true
                }
        );
        let json_segment = MultipartSegment {
            name: "data".to_string(),
            filename: None,
            content_type: Some("application/json".to_string()),
            data: MultipartSegmentData::Text(serde_json::to_string(&json_data).unwrap()),
        };
        let file_segment = MultipartSegment {
            name: "basic-mod-different.jar".to_string(),
            filename: Some("basic-mod.jar".to_string()),
            content_type: Some("application/java-archive".to_string()),
            data: MultipartSegmentData::Binary(
                include_bytes!("../../tests/files/basic-mod-different.jar").to_vec(),
            ),
        };

        let req_gen = || {
            test::TestRequest::post()
                .uri("/v2/version")
                .set_multipart(vec![json_segment.clone(), file_segment.clone()])
        };
        ScopeTest::new(&test_env)
            .test(req_gen, create_version)
            .await
            .unwrap();
    })
    .await;
}
