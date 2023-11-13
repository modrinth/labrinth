use actix_web::test;
use common::environment::TestEnvironment;
use futures::StreamExt;
use labrinth::database::models::version_item::VERSIONS_NAMESPACE;
use labrinth::models::ids::base62_impl::parse_base62;
use labrinth::models::projects::{Loader, ProjectId, VersionId, VersionStatus, VersionType};
use labrinth::routes::v2::version_file::FileUpdateData;
use serde_json::json;

use crate::common::database::*;

use crate::common::dummy_data::TestFile;
use crate::common::request_data::get_public_version_creation_data;

// importing common module.
mod common;

#[actix_rt::test]
async fn test_get_version() {
    // Test setup and dummy data
    let test_env = TestEnvironment::build(None).await;
    let api = &test_env.v2;
    let alpha_project_id: &String = &test_env.dummy.as_ref().unwrap().project_alpha.project_id;
    let alpha_version_id = &test_env.dummy.as_ref().unwrap().project_alpha.version_id;
    let beta_version_id = &test_env.dummy.as_ref().unwrap().project_beta.version_id;

    // Perform request on dummy data
    let version = api
        .get_version_deserialized(alpha_version_id, USER_USER_PAT)
        .await;
    assert_eq!(&version.project_id.to_string(), alpha_project_id);
    assert_eq!(&version.id.to_string(), alpha_version_id);

    let cached_project = test_env
        .db
        .redis_pool
        .get::<String, _>(VERSIONS_NAMESPACE, parse_base62(alpha_version_id).unwrap())
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
    assert_eq!(resp.status(), 404);

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
    assert_eq!(resp.status(), 200);
    let resp = api.get_version(beta_version_id, ENEMY_USER_PAT).await;
    assert_eq!(resp.status(), 404);

    // Cleanup test db
    test_env.cleanup().await;
}

#[actix_rt::test]

async fn version_updates() {
    // Test setup and dummy data
    let test_env = TestEnvironment::build(None).await;
    let api = &test_env.v2;

    let alpha_project_id: &String = &test_env.dummy.as_ref().unwrap().project_alpha.project_id;
    let alpha_version_id = &test_env.dummy.as_ref().unwrap().project_alpha.version_id;
    let beta_version_id = &test_env.dummy.as_ref().unwrap().project_beta.version_id;
    let alpha_version_hash = &test_env.dummy.as_ref().unwrap().project_alpha.file_hash;
    let beta_version_hash = &test_env.dummy.as_ref().unwrap().project_beta.file_hash;

    // Quick test, using get version from hash
    let version = api
        .get_version_from_hash_deserialized(alpha_version_hash, "sha1", USER_USER_PAT)
        .await;
    assert_eq!(&version.id.to_string(), alpha_version_id);

    // Get versions from hash
    let versions = api
        .get_versions_from_hashes_deserialized(
            &[alpha_version_hash.as_str(), beta_version_hash.as_str()],
            "sha1",
            USER_USER_PAT,
        )
        .await;
    assert_eq!(versions.len(), 2);
    assert_eq!(
        &versions[alpha_version_hash].id.to_string(),
        alpha_version_id
    );
    assert_eq!(&versions[beta_version_hash].id.to_string(), beta_version_id);

    // When there is only the one version, there should be no updates
    let version = api
        .get_update_from_hash_deserialized(
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
        .update_files_deserialized(
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
        let version = api
            .add_public_version(
                get_public_version_creation_data(
                    ProjectId(parse_base62(alpha_project_id).unwrap()),
                    version_number,
                    TestFile::build_random_jar(),
                ),
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
            assert_eq!(resp.status(), 200);
            let body: serde_json::Value = test::read_body_json(resp).await;
            let id = body["id"].as_str().unwrap();
            assert_eq!(id, &result_id.to_string());
        } else {
            assert_eq!(resp.status(), 404);
        }

        // update_files
        let versions = api
            .update_files_deserialized(
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
        let hashes = vec![FileUpdateData {
            hash: alpha_version_hash.to_string(),
            loaders,
            game_versions,
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
        .get_project_versions_deserialized(
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
        .get_project_versions_deserialized(
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

    // Cleanup test db
    test_env.cleanup().await;
}

#[actix_rt::test]
pub async fn test_patch_version() {
    let test_env = TestEnvironment::build(None).await;
    let api = &test_env.v2;

    let alpha_version_id = &test_env.dummy.as_ref().unwrap().project_alpha.version_id;

    // // First, we do some patch requests that should fail.
    // // Failure because the user is not authorized.
    let resp = api
        .edit_version(
            alpha_version_id,
            json!({
                "name": "test 1",
            }),
            ENEMY_USER_PAT,
        )
        .await;
    assert_eq!(resp.status(), 401);

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
        assert_eq!(resp.status(), 400);
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
                // // "dependencies": [], TODO: test this
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
    assert_eq!(resp.status(), 204);

    let version = api
        .get_version_deserialized(alpha_version_id, USER_USER_PAT)
        .await;
    assert_eq!(version.name, "new version name");
    assert_eq!(version.version_number, "1.3.0");
    assert_eq!(version.changelog, "new changelog");
    assert_eq!(
        version.version_type,
        serde_json::from_str::<VersionType>("\"beta\"").unwrap()
    );
    assert_eq!(version.game_versions, vec!["1.20.5"]);
    assert_eq!(version.loaders, vec![Loader("forge".to_string())]);
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
    assert_eq!(resp.status(), 204);

    let version = api
        .get_version_deserialized(alpha_version_id, USER_USER_PAT)
        .await;
    assert_eq!(version.game_versions, vec!["1.20.1", "1.20.2", "1.20.4"]);
    assert_eq!(version.loaders, vec![Loader("forge".to_string())]); // From last patch

    let resp = api
        .edit_version(
            alpha_version_id,
            json!({
                "loaders": ["fabric"],
            }),
            USER_USER_PAT,
        )
        .await;
    assert_eq!(resp.status(), 204);

    let version = api
        .get_version_deserialized(alpha_version_id, USER_USER_PAT)
        .await;
    assert_eq!(version.game_versions, vec!["1.20.1", "1.20.2", "1.20.4"]); // From last patch
    assert_eq!(version.loaders, vec![Loader("fabric".to_string())]);

    // Cleanup test db
    test_env.cleanup().await;
}

#[actix_rt::test]
pub async fn test_project_versions() {
    let test_env = TestEnvironment::build(None).await;
    let api = &test_env.v2;
    let alpha_project_id: &String = &test_env.dummy.as_ref().unwrap().project_alpha.project_id;
    let alpha_version_id = &test_env.dummy.as_ref().unwrap().project_alpha.version_id;
    let _beta_version_id = &test_env.dummy.as_ref().unwrap().project_beta.version_id;
    let _alpha_version_hash = &test_env.dummy.as_ref().unwrap().project_alpha.file_hash;
    let _beta_version_hash = &test_env.dummy.as_ref().unwrap().project_beta.file_hash;

    let versions = api
        .get_project_versions_deserialized(
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

    test_env.cleanup().await;
}
use crate::common::{asserts::assert_status, get_json_val_str};
use actix_http::StatusCode;
use common::{
    asserts::assert_version_ids, database::USER_USER_PAT, environment::with_test_environment,
};

#[actix_rt::test]
async fn can_create_version_with_ordering() {
    with_test_environment(|env| async move {
        let alpha_project_id = env.dummy.as_ref().unwrap().project_alpha.project_id.clone();

        let new_version_id = get_json_val_str(
            env.v2
                .create_default_version(&alpha_project_id, Some(1), USER_USER_PAT)
                .await
                .id,
        );

        let versions = env
            .v2
            .get_versions(vec![new_version_id.clone()], USER_USER_PAT)
            .await;
        assert_eq!(versions[0].ordering, Some(1));
    })
    .await;
}

#[actix_rt::test]
async fn edit_version_ordering_works() {
    with_test_environment(|env| async move {
        let alpha_version_id = env.dummy.as_ref().unwrap().project_alpha.version_id.clone();

        let resp = env
            .v2
            .edit_version_ordering(&alpha_version_id, Some(10), USER_USER_PAT)
            .await;
        assert_status(&resp, StatusCode::NO_CONTENT);

        let versions = env
            .v2
            .get_versions(vec![alpha_version_id.clone()], USER_USER_PAT)
            .await;
        assert_eq!(versions[0].ordering, Some(10));
    })
    .await;
}

#[actix_rt::test]
async fn version_ordering_for_specified_orderings_orders_lower_order_first() {
    with_test_environment(|env| async move {
        let alpha_project_id = env.dummy.as_ref().unwrap().project_alpha.project_id.clone();
        let alpha_version_id = env.dummy.as_ref().unwrap().project_alpha.version_id.clone();
        let new_version_id = get_json_val_str(
            env.v2
                .create_default_version(&alpha_project_id, Some(1), USER_USER_PAT)
                .await
                .id,
        );
        env.v2
            .edit_version_ordering(&alpha_version_id, Some(10), USER_USER_PAT)
            .await;

        let versions = env
            .v2
            .get_versions(
                vec![alpha_version_id.clone(), new_version_id.clone()],
                USER_USER_PAT,
            )
            .await;
        assert_version_ids(&versions, vec![new_version_id, alpha_version_id]);
    })
    .await;
}

#[actix_rt::test]
async fn version_ordering_when_unspecified_orders_oldest_first() {
    with_test_environment(|env| async move {
        let alpha_project_id = &env.dummy.as_ref().unwrap().project_alpha.project_id.clone();
        let alpha_version_id = env.dummy.as_ref().unwrap().project_alpha.version_id.clone();
        let new_version_id = get_json_val_str(
            env.v2
                .create_default_version(alpha_project_id, None, USER_USER_PAT)
                .await
                .id,
        );

        let versions = env
            .v2
            .get_versions(
                vec![alpha_version_id.clone(), new_version_id.clone()],
                USER_USER_PAT,
            )
            .await;
        assert_version_ids(&versions, vec![alpha_version_id, new_version_id]);
    })
    .await
}

#[actix_rt::test]
async fn version_ordering_when_specified_orders_specified_before_unspecified() {
    with_test_environment(|env| async move {
        let alpha_project_id = &env.dummy.as_ref().unwrap().project_alpha.project_id.clone();
        let alpha_version_id = env.dummy.as_ref().unwrap().project_alpha.version_id.clone();
        let new_version_id = get_json_val_str(
            env.v2
                .create_default_version(alpha_project_id, Some(10000), USER_USER_PAT)
                .await
                .id,
        );
        env.v2
            .edit_version_ordering(&alpha_version_id, None, USER_USER_PAT)
            .await;

        let versions = env
            .v2
            .get_versions(
                vec![alpha_version_id.clone(), new_version_id.clone()],
                USER_USER_PAT,
            )
            .await;
        assert_version_ids(&versions, vec![new_version_id, alpha_version_id]);
    })
    .await;
}