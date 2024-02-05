use std::collections::HashMap;

use crate::common::api_common::ApiVersion;
use crate::common::database::*;
use crate::common::dummy_data::{DummyProjectAlpha, DummyProjectBeta, TestFile};
use crate::common::get_json_val_str;
use actix_http::StatusCode;
use actix_web::test;
use common::api_common::ApiProject;
use common::api_v3::request_data::get_public_project_creation_data;
use common::api_v3::ApiV3;
use common::asserts::assert_common_version_ids;
use common::database::USER_USER_PAT;
use common::environment::{with_test_environment, with_test_environment_all};
use futures::StreamExt;
use labrinth::database::models::version_item::VERSIONS_NAMESPACE;
use labrinth::models::ids::base62_impl::parse_base62;
use labrinth::models::projects::{
    Dependency, DependencyType, Project, VersionId, VersionStatus, VersionType,
};
use labrinth::routes::v3::version_file::FileUpdateData;
use serde_json::json;

// importing common module.
mod common;

#[actix_rt::test]
async fn test_get_version() {
    // Test setup and dummy data
    with_test_environment_all(None, |test_env| async move {
        let api = &test_env.api;
        let DummyProjectAlpha {
            project_id: alpha_project_id,
            version_id: alpha_version_id,
            ..
        } = &test_env.dummy.project_alpha;
        let DummyProjectBeta {
            version_id: beta_version_id,
            ..
        } = &test_env.dummy.project_beta;

        // Perform request on dummy data
        let version = api
            .get_version_deserialized_common(alpha_version_id, USER_USER_PAT)
            .await;
        assert_eq!(&version.project_id.to_string(), alpha_project_id);
        assert_eq!(&version.id.to_string(), alpha_version_id);

        let mut redis_conn = test_env.db.redis_pool.connect().await.unwrap();
        let cached_project = redis_conn
            .get(
                VERSIONS_NAMESPACE,
                &parse_base62(alpha_version_id).unwrap().to_string(),
            )
            .await
            .unwrap()
            .unwrap();
        let cached_project: serde_json::Value = serde_json::from_str(&cached_project).unwrap();
        assert_eq!(
            cached_project["inner"]["project_id"],
            json!(parse_base62(alpha_project_id).unwrap())
        );

        // Request should fail on non-existent version
        let resp = api.get_version("false", USER_USER_PAT).await;
        assert_status!(&resp, StatusCode::NOT_FOUND);

        // Similarly, request should fail on non-authorized user, on a yet-to-be-approved or hidden project, with a 404 (hiding the existence of the project)
        // TODO: beta version should already be draft in dummy data, but theres a bug in finding it that
        api.edit_version(
            beta_version_id,
            json!({
                "status": "draft"
            }),
            USER_USER_PAT,
        )
        .await;
        let resp = api.get_version(beta_version_id, USER_USER_PAT).await;
        assert_status!(&resp, StatusCode::OK);
        let resp = api.get_version(beta_version_id, ENEMY_USER_PAT).await;
        assert_status!(&resp, StatusCode::NOT_FOUND);
    })
    .await;
}

#[actix_rt::test]
async fn version_updates_non_public_is_nothing() {
    // Test setup and dummy data
    with_test_environment(
        None,
        |test_env: common::environment::TestEnvironment<ApiV3>| async move {
            // Confirm that hash-finding functions return nothing for versions attached to non-public projects
            // This is a necessity because now hashes do not uniquely identify versions, and these functions expect being able to find a single version from a hash
            // This is the case even if we are the owner of the project/versions
            let api = &test_env.api;

            // First, create project gamma, which is private
            let gamma_creation_data = get_public_project_creation_data("gamma", None, None);
            let gamma_project = api.create_project(gamma_creation_data, USER_USER_PAT).await;
            let gamma_project: Project = test::read_body_json(gamma_project).await;

            let mut sha1_hashes = vec![];
            // Create 5 versions, and we will add them to both beta and gamma
            for i in 0..5 {
                let file = TestFile::build_random_jar();
                let version = api
                    .add_public_version_deserialized(
                        gamma_project.id,
                        &format!("1.2.3.{}", i),
                        file.clone(),
                        None,
                        None,
                        USER_USER_PAT,
                    )
                    .await;

                api.add_public_version_deserialized(
                    test_env.dummy.project_beta.project_id_parsed,
                    &format!("1.2.3.{}", i),
                    file.clone(),
                    None,
                    None,
                    USER_USER_PAT,
                )
                .await;

                sha1_hashes.push(version.files[0].hashes["sha1"].clone());
            }

            for approved in [false, true] {
                // get_version_from_hash
                let resp = api
                    .get_version_from_hash(&sha1_hashes[0], "sha1", USER_USER_PAT)
                    .await;
                if approved {
                    assert_status!(&resp, StatusCode::OK);
                } else {
                    assert_status!(&resp, StatusCode::NOT_FOUND);
                }

                // get_versions_from_hashes
                let resp = api
                    .get_versions_from_hashes(&[&sha1_hashes[0]], "sha1", USER_USER_PAT)
                    .await;
                assert_status!(&resp, StatusCode::OK);
                let body: HashMap<String, serde_json::Value> = test::read_body_json(resp).await;
                if approved {
                    assert_eq!(body.len(), 1);
                } else {
                    assert_eq!(body.len(), 0);
                }

                // get_update_from_hash
                let resp = api
                    .get_update_from_hash(&sha1_hashes[0], "sha1", None, None, None, USER_USER_PAT)
                    .await;
                if approved {
                    assert_status!(&resp, StatusCode::OK);
                } else {
                    assert_status!(&resp, StatusCode::NOT_FOUND);
                }

                // update_files
                let resp = api
                    .update_files(
                        "sha1",
                        vec![sha1_hashes[0].clone()],
                        None,
                        None,
                        None,
                        USER_USER_PAT,
                    )
                    .await;
                assert_status!(&resp, StatusCode::OK);
                let body: HashMap<String, serde_json::Value> = test::read_body_json(resp).await;
                if approved {
                    assert_eq!(body.len(), 1);
                } else {
                    assert_eq!(body.len(), 0);
                }

                // update_individual_files
                let hashes = vec![FileUpdateData {
                    hash: sha1_hashes[0].clone(),
                    loaders: None,
                    loader_fields: None,
                    version_types: None,
                }];
                let resp = api
                    .update_individual_files("sha1", hashes, USER_USER_PAT)
                    .await;
                assert_status!(&resp, StatusCode::OK);

                let body: HashMap<String, serde_json::Value> = test::read_body_json(resp).await;
                if approved {
                    assert_eq!(body.len(), 1);
                } else {
                    assert_eq!(body.len(), 0);
                }

                // Now, make the project public for the next loop, and confirm that the functions work
                if !approved {
                    api.edit_project(
                        &gamma_project.id.to_string(),
                        json!({
                            "status": "approved",
                        }),
                        MOD_USER_PAT,
                    )
                    .await;
                }
            }
        },
    )
    .await;
}

#[actix_rt::test]
async fn version_updates() {
    // Test setup and dummy data
    with_test_environment(
        None,
        |test_env: common::environment::TestEnvironment<ApiV3>| async move {
            let api = &test_env.api;
            let DummyProjectAlpha {
                project_id: alpha_project_id,
                project_id_parsed: alpha_project_id_parsed,
                version_id: alpha_version_id,
                file_hash: alpha_version_hash,
                ..
            } = &test_env.dummy.project_alpha;
            let DummyProjectBeta {
                file_hash: beta_version_hash,
                ..
            } = &test_env.dummy.project_beta;

            // Quick test, using get version from hash
            let version = api
                .get_version_from_hash_deserialized_common(
                    alpha_version_hash,
                    "sha1",
                    USER_USER_PAT,
                )
                .await;
            assert_eq!(&version.id.to_string(), alpha_version_id);

            // Get versions from hash
            let versions = api
                .get_versions_from_hashes_deserialized_common(
                    &[alpha_version_hash.as_str(), beta_version_hash.as_str()],
                    "sha1",
                    USER_USER_PAT,
                )
                .await;
            assert_eq!(versions.len(), 1); // Beta version should not be returned, not approved yet and hasn't claimed hash
            assert_eq!(
                &versions[alpha_version_hash].id.to_string(),
                alpha_version_id
            );

            // When there is only the one version, there should be no updates
            let version = api
                .get_update_from_hash_deserialized_common(
                    alpha_version_hash,
                    "sha1",
                    None,
                    None,
                    None,
                    USER_USER_PAT,
                )
                .await;
            assert_eq!(&version.id.to_string(), alpha_version_id);

            let versions = api
                .update_files_deserialized_common(
                    "sha1",
                    vec![alpha_version_hash.to_string()],
                    None,
                    None,
                    None,
                    USER_USER_PAT,
                )
                .await;
            assert_eq!(versions.len(), 1);
            assert_eq!(
                &versions[alpha_version_hash].id.to_string(),
                alpha_version_id
            );

            // Add 3 new versions, 1 before, and 2 after, with differing game_version/version_types/loaders
            let mut update_ids = vec![];
            for (version_number, patch_value) in [
                (
                    "0.9.9",
                    json!({
                        "game_versions": ["1.20.1"],
                    }),
                ),
                (
                    "1.5.0",
                    json!({
                        "game_versions": ["1.20.3"],
                        "loaders": ["fabric"],
                    }),
                ),
                (
                    "1.5.1",
                    json!({
                        "game_versions": ["1.20.4"],
                        "loaders": ["forge"],
                        "version_type": "beta"
                    }),
                ),
            ]
            .iter()
            {
                let file = TestFile::build_random_jar();
                let version = api
                    .add_public_version_deserialized(
                        *alpha_project_id_parsed,
                        version_number,
                        file.clone(),
                        None,
                        None,
                        USER_USER_PAT,
                    )
                    .await;
                update_ids.push(version.id);

                // Patch using json
                api.edit_version(&version.id.to_string(), patch_value.clone(), USER_USER_PAT)
                    .await;
            }

            let check_expected = |game_versions: Option<Vec<String>>,
                                  loaders: Option<Vec<String>>,
                                  version_types: Option<Vec<String>>,
                                  result_id: Option<VersionId>| async move {
                let (success, result_id) = match result_id {
                    Some(id) => (true, id),
                    None => (false, VersionId(0)),
                };
                // get_update_from_hash
                let resp = api
                    .get_update_from_hash(
                        alpha_version_hash,
                        "sha1",
                        loaders.clone(),
                        game_versions.clone(),
                        version_types.clone(),
                        USER_USER_PAT,
                    )
                    .await;
                if success {
                    assert_status!(&resp, StatusCode::OK);
                    let body: serde_json::Value = test::read_body_json(resp).await;
                    let id = body["id"].as_str().unwrap();
                    assert_eq!(id, &result_id.to_string());
                } else {
                    assert_status!(&resp, StatusCode::NOT_FOUND);
                }

                // update_files
                let versions = api
                    .update_files_deserialized_common(
                        "sha1",
                        vec![alpha_version_hash.to_string()],
                        loaders.clone(),
                        game_versions.clone(),
                        version_types.clone(),
                        USER_USER_PAT,
                    )
                    .await;
                if success {
                    assert_eq!(versions.len(), 1);
                    let first = versions.iter().next().unwrap();
                    assert_eq!(first.1.id, result_id);
                } else {
                    assert_eq!(versions.len(), 0);
                }

                // update_individual_files
                let mut loader_fields = HashMap::new();
                if let Some(game_versions) = game_versions {
                    loader_fields.insert(
                        "game_versions".to_string(),
                        game_versions
                            .into_iter()
                            .map(|v| json!(v))
                            .collect::<Vec<_>>(),
                    );
                }

                let hashes = vec![FileUpdateData {
                    hash: alpha_version_hash.to_string(),
                    loaders,
                    loader_fields: Some(loader_fields),
                    version_types: version_types.map(|v| {
                        v.into_iter()
                            .map(|v| serde_json::from_str(&format!("\"{v}\"")).unwrap())
                            .collect()
                    }),
                }];
                let versions = api
                    .update_individual_files_deserialized("sha1", hashes, USER_USER_PAT)
                    .await;
                if success {
                    assert_eq!(versions.len(), 1);
                    let first = versions.iter().next().unwrap();
                    assert_eq!(first.1.id, result_id);
                } else {
                    assert_eq!(versions.len(), 0);
                }
            };

            let tests = vec![
                check_expected(
                    Some(vec!["1.20.1".to_string()]),
                    None,
                    None,
                    Some(update_ids[0]),
                ),
                check_expected(
                    Some(vec!["1.20.3".to_string()]),
                    None,
                    None,
                    Some(update_ids[1]),
                ),
                check_expected(
                    Some(vec!["1.20.4".to_string()]),
                    None,
                    None,
                    Some(update_ids[2]),
                ),
                // Loader restrictions
                check_expected(
                    None,
                    Some(vec!["fabric".to_string()]),
                    None,
                    Some(update_ids[1]),
                ),
                check_expected(
                    None,
                    Some(vec!["forge".to_string()]),
                    None,
                    Some(update_ids[2]),
                ),
                // Version type restrictions
                check_expected(
                    None,
                    None,
                    Some(vec!["release".to_string()]),
                    Some(update_ids[1]),
                ),
                check_expected(
                    None,
                    None,
                    Some(vec!["beta".to_string()]),
                    Some(update_ids[2]),
                ),
                // Specific combination
                check_expected(
                    None,
                    Some(vec!["fabric".to_string()]),
                    Some(vec!["release".to_string()]),
                    Some(update_ids[1]),
                ),
                // Impossible combination
                check_expected(
                    None,
                    Some(vec!["fabric".to_string()]),
                    Some(vec!["beta".to_string()]),
                    None,
                ),
                // No restrictions, should do the last one
                check_expected(None, None, None, Some(update_ids[2])),
            ];

            // Wait on all tests, 4 at a time
            futures::stream::iter(tests)
                .buffer_unordered(4)
                .collect::<Vec<_>>()
                .await;

            // We do a couple small tests for get_project_versions_deserialized as well
            // TODO: expand this more.
            let versions = api
                .get_project_versions_deserialized_common(
                    alpha_project_id,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    USER_USER_PAT,
                )
                .await;
            assert_eq!(versions.len(), 4);
            let versions = api
                .get_project_versions_deserialized_common(
                    alpha_project_id,
                    None,
                    Some(vec!["forge".to_string()]),
                    None,
                    None,
                    None,
                    None,
                    USER_USER_PAT,
                )
                .await;
            assert_eq!(versions.len(), 1);
        },
    )
    .await;
}

#[actix_rt::test]
pub async fn test_patch_version() {
    with_test_environment_all(None, |test_env| async move {
        let api = &test_env.api;

        let alpha_version_id = &test_env.dummy.project_alpha.version_id;
        let DummyProjectBeta {
            project_id: beta_project_id,
            project_id_parsed: beta_project_id_parsed,
            ..
        } = &test_env.dummy.project_beta;

        // First, we do some patch requests that should fail.
        // Failure because the user is not authorized.
        let resp = api
            .edit_version(
                alpha_version_id,
                json!({
                    "name": "test 1",
                }),
                ENEMY_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::UNAUTHORIZED);

        // Failure because these are illegal requested statuses for a normal user.
        for req in ["unknown", "scheduled"] {
            let resp = api
                .edit_version(
                    alpha_version_id,
                    json!({
                        "status": req,
                        // requested status it not set here, but in /schedule
                    }),
                    USER_USER_PAT,
                )
                .await;
            assert_status!(&resp, StatusCode::BAD_REQUEST);
        }

        // Sucessful request to patch many fields.
        let resp = api
            .edit_version(
                alpha_version_id,
                json!({
                    "name": "new version name",
                    "version_number": "1.3.0",
                    "changelog": "new changelog",
                    "version_type": "beta",
                    "dependencies": [{
                        "project_id": beta_project_id,
                        "dependency_type": "required",
                        "file_name": "dummy_file_name"
                    }],
                    "game_versions": ["1.20.5"],
                    "loaders": ["forge"],
                    "featured": false,
                    // "primary_file": [], TODO: test this
                    // // "downloads": 0, TODO: moderator exclusive
                    "status": "draft",
                    // // "filetypes": ["jar"], TODO: test this
                }),
                USER_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::NO_CONTENT);

        let version = api
            .get_version_deserialized_common(alpha_version_id, USER_USER_PAT)
            .await;
        assert_eq!(version.name, "new version name");
        assert_eq!(version.version_number, "1.3.0");
        assert_eq!(version.changelog, "new changelog");
        assert_eq!(
            version.version_type,
            serde_json::from_str::<VersionType>("\"beta\"").unwrap()
        );
        assert_eq!(
            version.dependencies,
            vec![Dependency {
                project_id: Some(*beta_project_id_parsed),
                version_id: None,
                file_name: Some("dummy_file_name".to_string()),
                dependency_type: DependencyType::Required
            }]
        );
        assert_eq!(version.loaders, vec!["forge".to_string()]);
        assert!(!version.featured);
        assert_eq!(version.status, VersionStatus::from_string("draft"));

        // These ones are checking the v2-v3 rerouting, we eneusre that only 'game_versions'
        // works as expected, as well as only 'loaders'
        let resp = api
            .edit_version(
                alpha_version_id,
                json!({
                    "game_versions": ["1.20.1", "1.20.2", "1.20.4"],
                }),
                USER_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::NO_CONTENT);

        let version = api
            .get_version_deserialized_common(alpha_version_id, USER_USER_PAT)
            .await;
        assert_eq!(version.loaders, vec!["forge".to_string()]); // From last patch

        let resp = api
            .edit_version(
                alpha_version_id,
                json!({
                    "loaders": ["fabric"],
                }),
                USER_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::NO_CONTENT);

        let version = api
            .get_version_deserialized_common(alpha_version_id, USER_USER_PAT)
            .await;
        assert_eq!(version.loaders, vec!["fabric".to_string()]);
    })
    .await;
}

#[actix_rt::test]
pub async fn test_project_versions() {
    with_test_environment_all(None, |test_env| async move {
        let api = &test_env.api;
        let alpha_project_id: &String = &test_env.dummy.project_alpha.project_id;
        let alpha_version_id = &test_env.dummy.project_alpha.version_id;

        let versions = api
            .get_project_versions_deserialized_common(
                alpha_project_id,
                None,
                None,
                None,
                None,
                None,
                None,
                USER_USER_PAT,
            )
            .await;
        assert_eq!(versions.len(), 1);
        assert_eq!(&versions[0].id.to_string(), alpha_version_id);
    })
    .await;
}

#[actix_rt::test]
async fn can_create_version_with_ordering() {
    with_test_environment(
        None,
        |env: common::environment::TestEnvironment<ApiV3>| async move {
            let alpha_project_id_parsed = env.dummy.project_alpha.project_id_parsed;

            let new_version_id = get_json_val_str(
                env.api
                    .add_public_version_deserialized_common(
                        alpha_project_id_parsed,
                        "1.2.3.4",
                        TestFile::BasicMod,
                        Some(1),
                        None,
                        USER_USER_PAT,
                    )
                    .await
                    .id,
            );

            let versions = env
                .api
                .get_versions_deserialized(vec![new_version_id.clone()], USER_USER_PAT)
                .await;
            assert_eq!(versions[0].ordering, Some(1));
        },
    )
    .await;
}

#[actix_rt::test]
async fn edit_version_ordering_works() {
    with_test_environment(
        None,
        |env: common::environment::TestEnvironment<ApiV3>| async move {
            let alpha_version_id = env.dummy.project_alpha.version_id.clone();

            let resp = env
                .api
                .edit_version_ordering(&alpha_version_id, Some(10), USER_USER_PAT)
                .await;
            assert_status!(&resp, StatusCode::NO_CONTENT);

            let versions = env
                .api
                .get_versions_deserialized(vec![alpha_version_id.clone()], USER_USER_PAT)
                .await;
            assert_eq!(versions[0].ordering, Some(10));
        },
    )
    .await;
}

#[actix_rt::test]
async fn version_ordering_for_specified_orderings_orders_lower_order_first() {
    with_test_environment_all(None, |env| async move {
        let alpha_project_id_parsed = env.dummy.project_alpha.project_id_parsed;
        let alpha_version_id = env.dummy.project_alpha.version_id.clone();
        let new_version_id = get_json_val_str(
            env.api
                .add_public_version_deserialized_common(
                    alpha_project_id_parsed,
                    "1.2.3.4",
                    TestFile::BasicMod,
                    Some(1),
                    None,
                    USER_USER_PAT,
                )
                .await
                .id,
        );
        env.api
            .edit_version_ordering(&alpha_version_id, Some(10), USER_USER_PAT)
            .await;

        let versions = env
            .api
            .get_versions_deserialized_common(
                vec![alpha_version_id.clone(), new_version_id.clone()],
                USER_USER_PAT,
            )
            .await;
        assert_common_version_ids(&versions, vec![new_version_id, alpha_version_id]);
    })
    .await;
}

#[actix_rt::test]
async fn version_ordering_when_unspecified_orders_oldest_first() {
    with_test_environment_all(None, |env| async move {
        let alpha_project_id_parsed = env.dummy.project_alpha.project_id_parsed;
        let alpha_version_id: String = env.dummy.project_alpha.version_id.clone();
        let new_version_id = get_json_val_str(
            env.api
                .add_public_version_deserialized_common(
                    alpha_project_id_parsed,
                    "1.2.3.4",
                    TestFile::BasicMod,
                    None,
                    None,
                    USER_USER_PAT,
                )
                .await
                .id,
        );

        let versions = env
            .api
            .get_versions_deserialized_common(
                vec![alpha_version_id.clone(), new_version_id.clone()],
                USER_USER_PAT,
            )
            .await;
        assert_common_version_ids(&versions, vec![alpha_version_id, new_version_id]);
    })
    .await
}

#[actix_rt::test]
async fn version_ordering_when_specified_orders_specified_before_unspecified() {
    with_test_environment_all(None, |env| async move {
        let alpha_project_id_parsed = env.dummy.project_alpha.project_id_parsed;
        let alpha_version_id = env.dummy.project_alpha.version_id.clone();
        let new_version_id = get_json_val_str(
            env.api
                .add_public_version_deserialized_common(
                    alpha_project_id_parsed,
                    "1.2.3.4",
                    TestFile::BasicMod,
                    Some(1000),
                    None,
                    USER_USER_PAT,
                )
                .await
                .id,
        );
        env.api
            .edit_version_ordering(&alpha_version_id, None, USER_USER_PAT)
            .await;

        let versions = env
            .api
            .get_versions_deserialized_common(
                vec![alpha_version_id.clone(), new_version_id.clone()],
                USER_USER_PAT,
            )
            .await;
        assert_common_version_ids(&versions, vec![new_version_id, alpha_version_id]);
    })
    .await;
}
