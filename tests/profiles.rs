use std::collections::HashSet;
use std::path::PathBuf;

use actix_http::StatusCode;
use actix_web::test;
use common::api_v3::ApiV3;
use common::database::*;
use common::environment::with_test_environment;
use common::environment::TestEnvironment;
use labrinth::models::client::profile::ClientProfile;
use labrinth::models::client::profile::ClientProfileMetadata;
use labrinth::models::projects::Project;
use labrinth::models::users::UserId;
use sha2::Digest;

use crate::common::api_common::ApiProject;
use crate::common::api_common::ApiVersion;
use crate::common::api_v3::client_profile::ClientProfileOverride;
use crate::common::api_v3::request_data::get_public_project_creation_data;
use crate::common::dummy_data::DummyImage;
use crate::common::dummy_data::TestFile;

mod common;

#[actix_rt::test]
async fn create_modify_profile() {
    // Test setup and dummy data
    with_test_environment(None, |test_env: TestEnvironment<ApiV3>| async move {
        // Create and modifiy a profile with certain properties
        // Check that the properties are correct
        let api = &test_env.api;
        let alpha_version_id = test_env.dummy.project_alpha.version_id.to_string();

        // Attempt to create a simple profile with invalid data, these should fail.
        // - fake loader
        // - fake loader version for loader
        // - unparseable version (not to be confused with parseable but nonexistent version, which is simply ignored)
        // - fake game version
        let resp = api
            .create_client_profile(
                "test",
                "fake-loader",
                "1.0.0",
                "1.20.1",
                vec![],
                USER_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::BAD_REQUEST);

        // Currently fake version for loader is not checked
        // let resp = api
        //     .create_client_profile("test", "fabric", "fake", "1.20.1", vec![], USER_USER_PAT)
        //     .await;
        // assert_status!(&resp, StatusCode::BAD_REQUEST);

        let resp = api
            .create_client_profile(
                "test",
                "fabric",
                "1.0.0",
                "1.20.1",
                vec!["unparseable-version"],
                USER_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::BAD_REQUEST);

        let resp = api
            .create_client_profile("test", "fabric", "1.0.0", "1.19.1", vec![], USER_USER_PAT)
            .await;
        assert_status!(&resp, StatusCode::BAD_REQUEST);

        // Create a simple profile
        // should succeed
        let profile = api
            .create_client_profile("test", "fabric", "1.0.0", "1.20.1", vec![], USER_USER_PAT)
            .await;
        assert_status!(&profile, StatusCode::OK);
        let profile: ClientProfile = test::read_body_json(profile).await;
        let id = profile.id.to_string();

        // Get the profile and check the properties are correct
        let profile = api
            .get_client_profile_deserialized(&id, USER_USER_PAT)
            .await;
        let updated = profile.updated; // Save this- it will update when we modify the versions/overrides
        let ClientProfileMetadata::Minecraft {
            game_version,
            loader_version,
        } = profile.game
        else {
            panic!("Wrong metadata type")
        };
        assert_eq!(profile.name, "test");
        assert_eq!(profile.loader, "fabric");
        assert_eq!(loader_version, "1.0.0");
        assert_eq!(game_version, "1.20.1");
        assert_eq!(profile.share_links.unwrap().len(), 0); // No links yet
        assert_eq!(profile.icon_url, None);

        // Modify the profile illegally in the same ways
        let resp = api
            .edit_client_profile(
                &profile.id.to_string(),
                None,
                Some("fake-loader"),
                None,
                None,
                None,
                None,
                USER_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::BAD_REQUEST);

        // TODO: Currently fake version for loader is not checked
        // let resp = api
        //     .edit_client_profile(
        //         &profile.id.to_string(),
        //         None,
        //         Some("fabric"),
        //         Some("fake"),
        //         None,
        //         USER_USER_PAT,
        //     )
        //     .await;
        // assert_status!(&resp, StatusCode::BAD_REQUEST);

        let resp = api
            .edit_client_profile(
                &profile.id.to_string(),
                None,
                Some("fabric"),
                None,
                Some(vec!["unparseable-version"]),
                None,
                None,
                USER_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::BAD_REQUEST);

        // Can't modify the profile as another user
        let resp = api
            .edit_client_profile(
                &profile.id.to_string(),
                None,
                Some("fabric"),
                None,
                None,
                None,
                None,
                FRIEND_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::UNAUTHORIZED);

        //  Get and make sure the properties are the same
        let profile = api
            .get_client_profile_deserialized(&id, USER_USER_PAT)
            .await;
        assert_eq!(profile.name, "test");
        assert_eq!(profile.loader, "fabric");
        let ClientProfileMetadata::Minecraft {
            game_version,
            loader_version,
        } = profile.game
        else {
            panic!("Wrong metadata type")
        };
        assert_eq!(loader_version, "1.0.0");
        assert_eq!(game_version, "1.20.1");
        assert_eq!(profile.share_links.unwrap().len(), 0);
        assert_eq!(profile.icon_url, None);
        assert_eq!(profile.updated, updated);

        // A successful modification
        let resp = api
            .edit_client_profile(
                &profile.id.to_string(),
                Some("test2"),
                Some("forge"),
                Some("1.0.1"),
                Some(vec![&alpha_version_id]),
                None,
                None,
                USER_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::NO_CONTENT);

        // Get the profile and check the properties
        let profile = api
            .get_client_profile_deserialized(&id, USER_USER_PAT)
            .await;
        assert_eq!(profile.name, "test2");
        assert_eq!(profile.loader, "forge");
        let ClientProfileMetadata::Minecraft {
            game_version,
            loader_version,
        } = profile.game
        else {
            panic!("Wrong metadata type")
        };
        assert_eq!(loader_version, "1.0.1");
        assert_eq!(game_version, "1.20.1");
        assert_eq!(profile.icon_url, None);
        assert!(profile.updated > updated);
        let updated = profile.updated;

        // Modify the profile again
        let resp = api
            .edit_client_profile(
                &profile.id.to_string(),
                Some("test3"),
                Some("fabric"),
                Some("1.0.0"),
                Some(vec![]),
                None,
                None,
                USER_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::NO_CONTENT);

        // Get the profile and check the properties
        let profile = api
            .get_client_profile_deserialized(&id, USER_USER_PAT)
            .await;

        assert_eq!(profile.name, "test3");
        assert_eq!(profile.loader, "fabric");
        let ClientProfileMetadata::Minecraft {
            game_version,
            loader_version,
        } = profile.game
        else {
            panic!("Wrong metadata type")
        };
        assert_eq!(loader_version, "1.0.0");
        assert_eq!(game_version, "1.20.1");
        assert_eq!(profile.icon_url, None);
        assert!(profile.updated > updated);
    })
    .await;
}

#[actix_rt::test]
async fn accept_share_link() {
    with_test_environment(None, |test_env: TestEnvironment<ApiV3>| async move {
        // Get download links for a created profile (including failure), create a share link, and create the correct number of tokens based on that
        // They should expire after a time
        let api = &test_env.api;

        // Create a simple profile
        let profile = api
            .create_client_profile("test", "fabric", "1.0.0", "1.20.1", vec![], USER_USER_PAT)
            .await;
        assert_status!(&profile, StatusCode::OK);
        let id = test::read_body_json::<ClientProfile, _>(profile)
            .await
            .id
            .to_string();

        // get the profile
        let profile = api
            .get_client_profile_deserialized(&id, USER_USER_PAT)
            .await;
        assert_eq!(profile.share_links.unwrap().len(), 0);
        let users: Vec<UserId> = profile.users.unwrap();
        assert_eq!(users.len(), 1);
        assert_eq!(users[0].0, USER_USER_ID_PARSED as u64);

        // Getting user's profiles should return the profile
        let profiles = api
            .get_user_client_profiles_deserialized(USER_USER_PAT)
            .await;
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].id.to_string(), id);
        assert_eq!(profiles[0].owner_id.to_string(), USER_USER_ID);

        // Friend can't see the profile users, links, versions, install paths yet, but can see the profile
        let profile = api
            .get_client_profile_deserialized(&id, FRIEND_USER_PAT)
            .await;
        assert_eq!(profile.users, None);

        // Getting friend's profiles should not return the profile
        let profiles = api
            .get_user_client_profiles_deserialized(FRIEND_USER_PAT)
            .await;
        assert_eq!(profiles.len(), 0);

        // As 'user', try to generate a download link for the profile
        let share_link = api
            .generate_client_profile_share_link_deserialized(&id, USER_USER_PAT)
            .await;

        // Get profile again
        let profile = api
            .get_client_profile_deserialized(&id, USER_USER_PAT)
            .await;
        assert_eq!(profile.share_links.unwrap().len(), 1); // Now has a share link

        // Link is an 'accept' link, when visited using any user token using POST, it should add the user to the profile
        // As 'friend', accept the share link
        let resp = api
            .accept_client_profile_share_link(&share_link.id.to_string(), FRIEND_USER_PAT)
            .await;
        assert_status!(&resp, StatusCode::NO_CONTENT);

        // Profile users should now include the friend
        let profile = api
            .get_client_profile_deserialized(&id, USER_USER_PAT)
            .await;
        let mut users = profile.users.unwrap();
        users.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(users.len(), 2);
        assert_eq!(users[0].0, USER_USER_ID_PARSED as u64);
        assert_eq!(users[1].0, FRIEND_USER_ID_PARSED as u64);

        // Getting friend's profiles should return the profile
        let profiles = api
            .get_user_client_profiles_deserialized(FRIEND_USER_PAT)
            .await;
        assert_eq!(profiles.len(), 1);

        // Add all of test dummy users until we hit the limit
        let dummy_user_pats = [
            USER_USER_PAT,   // Fails because owner (and already added)
            FRIEND_USER_PAT, // Fails because already added
            OTHER_FRIEND_USER_PAT,
            MOD_USER_PAT,
            ADMIN_USER_PAT,
            ENEMY_USER_PAT, // If we add a 'max_users' field, this last test could be modified to fail
        ];
        for (i, pat) in dummy_user_pats.iter().enumerate().take(4 + 1) {
            let resp = api
                .accept_client_profile_share_link(&share_link.id.to_string(), *pat)
                .await;
            if i == 0 || i == 1 {
                assert_status!(&resp, StatusCode::BAD_REQUEST);
            } else {
                assert_status!(&resp, StatusCode::NO_CONTENT);
            }
        }

        // As user, remove share link
        let resp = api
            .edit_client_profile(
                &id,
                None,
                None,
                None,
                None,
                None,
                Some(vec![&share_link.id.to_string()]),
                USER_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::NO_CONTENT);

        // Confirm share link is gone
        let profile = api
            .get_client_profile_deserialized(&id, USER_USER_PAT)
            .await;
        assert_eq!(profile.share_links.unwrap().len(), 0);

        // Friend still has the profile
        let profiles = api
            .get_user_client_profiles_deserialized(USER_USER_PAT)
            .await;
        assert_eq!(profiles.len(), 1);
        let profiles = api
            .get_user_client_profiles_deserialized(FRIEND_USER_PAT)
            .await;
        assert_eq!(profiles.len(), 1);

        // Remove friend
        let resp = api
            .edit_client_profile(
                &id,
                None,
                None,
                None,
                None,
                Some(vec![FRIEND_USER_ID]),
                None,
                USER_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::NO_CONTENT);

        // Confirm friend is no longer on the profile
        let profiles = api
            .get_user_client_profiles_deserialized(USER_USER_PAT)
            .await;
        assert_eq!(profiles.len(), 1);
        let profiles = api
            .get_user_client_profiles_deserialized(FRIEND_USER_PAT)
            .await;
        assert_eq!(profiles.len(), 0);
    })
    .await;
}

#[actix_rt::test]
async fn delete_profile() {
    with_test_environment(None, |test_env: TestEnvironment<ApiV3>| async move {
        // They should expire after a time
        let api = &test_env.api;

        let alpha_version_id = &test_env.dummy.project_alpha.version_id.to_string();

        // Create a simple profile
        let profile = api
            .create_client_profile(
                "test",
                "fabric",
                "1.0.0",
                "1.20.1",
                vec![alpha_version_id],
                USER_USER_PAT,
            )
            .await;
        assert_status!(&profile, StatusCode::OK);
        let profile: ClientProfile = test::read_body_json(profile).await;
        let id = profile.id.to_string();

        // Add an override file to the profile
        let resp = api
            .add_client_profile_overrides(
                &id,
                vec![ClientProfileOverride::new(
                    TestFile::BasicMod,
                    "mods/test.jar",
                )],
                USER_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::NO_CONTENT);

        // Invite a friend to the profile
        let share_link = api
            .generate_client_profile_share_link_deserialized(&id, USER_USER_PAT)
            .await;

        // As friend, try to get the download links for the profile
        // Not invited yet, should fail
        let resp = api
            .download_client_profile_from_profile_id(&id, FRIEND_USER_PAT)
            .await;
        assert_status!(&resp, StatusCode::UNAUTHORIZED);

        // Accept
        let resp = api
            .accept_client_profile_share_link(&share_link.id.to_string(), FRIEND_USER_PAT)
            .await;
        assert_status!(&resp, StatusCode::NO_CONTENT);

        // Get profile from share link
        let profile = api
            .get_client_profile_from_share_link_deserialized(
                &share_link.id.to_string(),
                FRIEND_USER_PAT,
            )
            .await;

        // Get a token as the friend, from the share link id
        let token = api
            .download_client_profile_from_profile_id_deserialized(
                &profile.id.to_string(),
                FRIEND_USER_PAT,
            )
            .await;

        // Confirm it works
        let resp = api
            .check_download_client_profile_token(&token.override_cdns[0].url, FRIEND_USER_PAT)
            .await;
        assert_status!(&resp, StatusCode::OK);

        // Delete the profile as the friend
        // Should fail
        let resp = api.delete_client_profile(&id, FRIEND_USER_PAT).await;
        assert_status!(&resp, StatusCode::UNAUTHORIZED);

        // Delete the profile as the user
        // Should succeed
        let resp = api.delete_client_profile(&id, USER_USER_PAT).await;
        assert_status!(&resp, StatusCode::NO_CONTENT);

        // Confirm the profile is gone
        let resp = api.get_client_profile(&id, USER_USER_PAT).await;
        assert_status!(&resp, StatusCode::NOT_FOUND);

        // Confirm the token is gone
        let resp = api
            .check_download_client_profile_token(&token.override_cdns[0].url, FRIEND_USER_PAT)
            .await;
        assert_status!(&resp, StatusCode::UNAUTHORIZED);
    })
    .await;
}

#[actix_rt::test]
async fn download_profile() {
    with_test_environment(None, |test_env: TestEnvironment<ApiV3>| async move {
        // Get download links for a created profile (including failure), create a share link, and create the correct number of tokens based on that
        // They should expire after a time
        let api = &test_env.api;

        // Create a simple profile
        let profile = api
            .create_client_profile("test", "fabric", "1.0.0", "1.20.1", vec![], USER_USER_PAT)
            .await;
        assert_status!(&profile, StatusCode::OK);
        let profile: ClientProfile = test::read_body_json(profile).await;
        let id = profile.id.to_string();

        // Add an override file to the profile
        let resp = api
            .add_client_profile_overrides(
                &id,
                vec![ClientProfileOverride::new(
                    TestFile::BasicMod,
                    "mods/test.jar",
                )],
                USER_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::NO_CONTENT);

        // As 'user', try to generate a download link for the profile
        let resp = api
            .download_client_profile_from_profile_id(&id, USER_USER_PAT)
            .await;
        assert_status!(&resp, StatusCode::OK);

        // As 'friend', try to get the download links for the profile
        // Not invited yet, should fail
        let resp = api
            .download_client_profile_from_profile_id(&id, FRIEND_USER_PAT)
            .await;
        assert_status!(&resp, StatusCode::UNAUTHORIZED);

        // As 'user', try to generate a share link for the profile, and accept it as 'friend'
        let share_link = api
            .generate_client_profile_share_link_deserialized(&id, USER_USER_PAT)
            .await;
        let resp = api
            .accept_client_profile_share_link(&share_link.id.to_string(), FRIEND_USER_PAT)
            .await;
        assert_status!(&resp, StatusCode::NO_CONTENT);

        // As 'friend', try to get the download links for the profile
        // Should succeed
        let mut download = api
            .download_client_profile_from_profile_id_deserialized(&id, FRIEND_USER_PAT)
            .await;

        // But enemy should fail
        let resp = api
            .download_client_profile_from_profile_id(&id, ENEMY_USER_PAT)
            .await;
        assert_status!(&resp, StatusCode::UNAUTHORIZED);

        // Download url should be:
        // - CDN url
        // "custom_files"
        // - hash
        assert_eq!(download.override_cdns.len(), 1);
        let override_file_url = download.override_cdns.remove(0).url;
        let hash = format!("{:x}", sha2::Sha512::digest(&TestFile::BasicMod.bytes()));
        assert_eq!(
            override_file_url,
            format!("{}/custom_files/{}", dotenvy::var("CDN_URL").unwrap(), hash)
        );

        // Check cloudflare helper route with a bad token (eg: the wrong user, or no user), or bad url should fail
        let resp = api
            .check_download_client_profile_token(&override_file_url, None)
            .await;
        assert_status!(&resp, StatusCode::UNAUTHORIZED);
        let resp = api
            .check_download_client_profile_token(&override_file_url, ENEMY_USER_PAT)
            .await;
        assert_status!(&resp, StatusCode::UNAUTHORIZED);

        let resp = api
            .check_download_client_profile_token("bad_url", FRIEND_USER_PAT)
            .await;
        assert_status!(&resp, StatusCode::UNAUTHORIZED);

        let resp = api
            .check_download_client_profile_token(
                &format!(
                    "{}/custom_files/{}",
                    dotenvy::var("CDN_URL").unwrap(),
                    "example_hash"
                ),
                FRIEND_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::UNAUTHORIZED);

        // Check cloudflare helper route to confirm this is a valid allowable access token
        // We attach it as an authorization token and call the route
        let resp = api
            .check_download_client_profile_token(&override_file_url, FRIEND_USER_PAT)
            .await;
        assert_status!(&resp, StatusCode::OK);

        // As user, remove friend from profile
        let resp = api
            .edit_client_profile(
                &id,
                None,
                None,
                None,
                None,
                Some(vec![FRIEND_USER_ID]),
                None,
                USER_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::NO_CONTENT);

        // Confirm friend is no longer on the profile
        let profile = api
            .get_client_profile_deserialized(&id, USER_USER_PAT)
            .await;
        assert_eq!(profile.users.unwrap().len(), 1);

        // Confirm friend can no longer download the profile
        let resp = api
            .download_client_profile_from_profile_id(&id, FRIEND_USER_PAT)
            .await;
        assert_status!(&resp, StatusCode::UNAUTHORIZED);

        // Confirm token invalidation
        let resp = api
            .check_download_client_profile_token(&override_file_url, FRIEND_USER_PAT)
            .await;
        assert_status!(&resp, StatusCode::UNAUTHORIZED);

        // Confirm user can still download the profile
        let resp = api
            .download_client_profile_from_profile_id_deserialized(&id, USER_USER_PAT)
            .await;
        assert_eq!(resp.override_cdns.len(), 1);
    })
    .await;
}

#[actix_rt::test]
async fn add_remove_profile_icon() {
    with_test_environment(None, |test_env: TestEnvironment<ApiV3>| async move {
        // Add and remove an icon from a profile
        let api = &test_env.api;

        // Create a simple profile
        let profile = api
            .create_client_profile("test", "fabric", "1.0.0", "1.20.1", vec![], USER_USER_PAT)
            .await;
        assert_status!(&profile, StatusCode::OK);
        let profile: ClientProfile = test::read_body_json(profile).await;

        // Add an icon to the profile
        let icon = api
            .edit_client_profile_icon(
                &profile.id.to_string(),
                Some(DummyImage::SmallIcon.get_icon_data()),
                USER_USER_PAT,
            )
            .await;
        assert_status!(&icon, StatusCode::NO_CONTENT);

        // Get the profile and check the icon
        let profile = api
            .get_client_profile_deserialized(&profile.id.to_string(), USER_USER_PAT)
            .await;
        assert!(profile.icon_url.is_some());

        // Remove the icon from the profile
        let icon = api
            .edit_client_profile_icon(&profile.id.to_string(), None, USER_USER_PAT)
            .await;
        assert_status!(&icon, StatusCode::NO_CONTENT);

        // Get the profile and check the icon
        let profile = api
            .get_client_profile_deserialized(&profile.id.to_string(), USER_USER_PAT)
            .await;
        assert!(profile.icon_url.is_none());
    })
    .await;
}

#[actix_rt::test]
async fn add_remove_profile_versions() {
    with_test_environment(None, |test_env: TestEnvironment<ApiV3>| async move {
        // Add and remove versions from a profile
        let api = &test_env.api;
        let alpha_version_id = test_env.dummy.project_alpha.version_id.to_string();
        // Create a simple profile
        let profile = api
            .create_client_profile("test", "fabric", "1.0.0", "1.20.1", vec![], USER_USER_PAT)
            .await;
        assert_status!(&profile, StatusCode::OK);
        let profile: ClientProfile = test::read_body_json(profile).await;
        let updated = profile.updated; // Save this- it will update when we modify the versions/overrides

        // Add a hosted version to the profile
        let resp = api
            .edit_client_profile(
                &profile.id.to_string(),
                None,
                None,
                None,
                Some(vec![&alpha_version_id]),
                None,
                None,
                USER_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::NO_CONTENT);

        // Add an override file to the profile
        let resp = api
            .add_client_profile_overrides(
                &profile.id.to_string(),
                vec![ClientProfileOverride::new(
                    TestFile::BasicMod,
                    "mods/test.jar",
                )],
                USER_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::NO_CONTENT);

        // Add a second version to the profile
        let resp = api
            .add_client_profile_overrides(
                &profile.id.to_string(),
                vec![ClientProfileOverride::new(
                    TestFile::BasicModDifferent,
                    "mods/test_different.jar",
                )],
                USER_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::NO_CONTENT);

        // Get the profile and check the versions
        let profile_downloads = api
            .download_client_profile_from_profile_id_deserialized(
                &profile.id.to_string(),
                USER_USER_PAT,
            )
            .await;
        assert_eq!(
            profile_downloads.version_ids,
            vec![test_env.dummy.project_alpha.version_id_parsed]
        );
        assert_eq!(
            profile_downloads
                .override_cdns
                .into_iter()
                .map(|x| x.install_path)
                .collect::<HashSet<_>>(),
            [
                PathBuf::from("mods/test.jar"),
                PathBuf::from("mods/test_different.jar")
            ]
            .iter()
            .cloned()
            .collect::<HashSet<_>>()
        );

        // Get profile again to confirm update
        let profile = api
            .get_client_profile_deserialized(&profile.id.to_string(), USER_USER_PAT)
            .await;
        assert!(profile.updated > updated);
        let updated = profile.updated;

        // Create a second profile using the same hashes, but ENEMY_USER_PAT
        let profile_enemy = api
            .create_client_profile("test2", "fabric", "1.0.0", "1.20.1", vec![], ENEMY_USER_PAT)
            .await;
        assert_status!(&profile_enemy, StatusCode::OK);
        let profile_enemy: ClientProfile = test::read_body_json(profile_enemy).await;
        let id_enemy = profile_enemy.id.to_string();

        // Add the same override to the profile
        let resp = api
            .add_client_profile_overrides(
                &id_enemy,
                vec![ClientProfileOverride::new(
                    TestFile::BasicMod,
                    "mods/test.jar",
                )],
                ENEMY_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::NO_CONTENT);

        // Get the profile and check the versions
        let profile_enemy = api
            .download_client_profile_from_profile_id_deserialized(&id_enemy, ENEMY_USER_PAT)
            .await;

        assert_eq!(
            profile_enemy
                .override_cdns
                .into_iter()
                .map(|x| x.install_path)
                .collect::<Vec<_>>(),
            vec![PathBuf::from("mods/test.jar")]
        );

        // Attempt to delete the override test.jar from the user's profile
        // Should succeed
        let resp = api
            .delete_client_profile_overrides(
                &profile.id.to_string(),
                Some(&[&PathBuf::from("mods/test.jar")]),
                None,
                USER_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::NO_CONTENT);

        // Should still exist in the enemy's profile, but not the user's
        let profile_enemy_downloads = api
            .download_client_profile_from_profile_id_deserialized(&id_enemy, ENEMY_USER_PAT)
            .await;
        assert_eq!(
            profile_enemy_downloads
                .override_cdns
                .into_iter()
                .map(|x| x.install_path)
                .collect::<Vec<_>>(),
            vec![PathBuf::from("mods/test.jar")]
        );

        let profile_downloads = api
            .download_client_profile_from_profile_id_deserialized(
                &profile.id.to_string(),
                USER_USER_PAT,
            )
            .await;
        assert_eq!(
            profile_downloads
                .override_cdns
                .into_iter()
                .map(|x| x.install_path)
                .collect::<Vec<_>>(),
            vec![PathBuf::from("mods/test_different.jar")]
        );

        // Get profile again to confirm update
        let profile = api
            .get_client_profile_deserialized(&profile.id.to_string(), USER_USER_PAT)
            .await;
        assert!(profile.updated > updated);
        let updated = profile.updated;

        // TODO: put a test here for confirming the file's existence once tests are set up to do so
        // The file should still exist in the CDN here, as the enemy still has it

        // Attempt to delete the override test_different.jar from the enemy's profile (One they don't have)
        // Should fail
        // First, by path
        let resp = api
            .delete_client_profile_overrides(
                &id_enemy,
                Some(&[&PathBuf::from("mods/test_different.jar")]),
                None,
                ENEMY_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::NO_CONTENT); // Allow failure to return success, it just doesn't delete anything

        // Then, by hash
        let resp = api
            .delete_client_profile_overrides(
                &id_enemy,
                None,
                Some(&[format!(
                    "{:x}",
                    sha2::Sha512::digest(&TestFile::BasicModDifferent.bytes())
                )
                .as_str()]),
                ENEMY_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::NO_CONTENT); // Allow failure to return success, it just doesn't delete anything

        // Confirm user still has it
        let profile_downloads = api
            .download_client_profile_from_profile_id_deserialized(
                &profile.id.to_string(),
                USER_USER_PAT,
            )
            .await;
        assert_eq!(
            profile_downloads
                .override_cdns
                .into_iter()
                .map(|x| x.install_path)
                .collect::<Vec<_>>(),
            vec![PathBuf::from("mods/test_different.jar")]
        );

        // TODO: put a test here for confirming the file's existence once tests are set up to do so
        // The file should still exist in the CDN here, as the enemy can't delete it

        // Now delete the override test_different.jar from the user's profile (by hash this time)
        // Should succeed
        let resp = api
            .delete_client_profile_overrides(
                &profile.id.to_string(),
                None,
                Some(&[format!(
                    "{:x}",
                    sha2::Sha512::digest(&TestFile::BasicModDifferent.bytes())
                )
                .as_str()]),
                USER_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::NO_CONTENT);

        // Confirm user no longer has it
        let profile_downloads = api
            .download_client_profile_from_profile_id_deserialized(
                &profile.id.to_string(),
                USER_USER_PAT,
            )
            .await;
        assert_eq!(
            profile_downloads
                .override_cdns
                .into_iter()
                .map(|x| x.install_path)
                .collect::<Vec<_>>(),
            Vec::<PathBuf>::new()
        );

        // Get profile again to confirm update
        let profile = api
            .get_client_profile_deserialized(&profile.id.to_string(), USER_USER_PAT)
            .await;
        assert!(profile.updated > updated);

        // In addition, delete "alpha_version_id" from the user's profile
        // Should succeed
        let resp = api
            .edit_client_profile(
                &profile.id.to_string(),
                None,
                None,
                None,
                Some(vec![]),
                None,
                None,
                USER_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::NO_CONTENT);

        // Confirm user no longer has it
        let profile_downloads = api
            .download_client_profile_from_profile_id_deserialized(
                &profile.id.to_string(),
                USER_USER_PAT,
            )
            .await;
        assert_eq!(profile_downloads.version_ids, vec![]);
    })
    .await;
}

// Profile gotten from share link vs profile gotten from profile id should be the same
#[actix_rt::test]
async fn share_link_profile_same_as_profile_id_profile() {
    with_test_environment(None, |test_env: TestEnvironment<ApiV3>| async move {
        // Get download links for a created profile (including failure), create a share link, and create the correct number of tokens based on that
        // They should expire after a time
        let api = &test_env.api;

        // Create a simple profile
        let profile = api
            .create_client_profile("test", "fabric", "1.0.0", "1.20.1", vec![], USER_USER_PAT)
            .await;
        assert_status!(&profile, StatusCode::OK);
        let profile: ClientProfile = test::read_body_json(profile).await;
        let id = profile.id.to_string();

        // Create a share link for the profile
        let share_link = api
            .generate_client_profile_share_link_deserialized(&id, USER_USER_PAT)
            .await;

        // Get the profile from the share link
        for pat in [USER_USER_PAT, FRIEND_USER_PAT].iter() {
            let profile_from_share_link = api
                .get_client_profile_from_share_link_deserialized(&share_link.id.to_string(), *pat)
                .await;

            let profile_from_profile_id = api.get_client_profile_deserialized(&id, *pat).await;

            assert_eq!(profile_from_share_link, profile_from_profile_id);
        }
    })
    .await;
}

// Cannot add versions you do not have visibility access to
#[actix_rt::test]
async fn hidden_versions_are_forbidden() {
    // Test setup and dummy data
    with_test_environment(None, |test_env: TestEnvironment<ApiV3>| async move {
        let api = &test_env.api;
        let beta_version_id = test_env.dummy.project_beta.version_id.to_string();
        let alpha_version_id = test_env.dummy.project_alpha.version_id.to_string();
        let alpha_version_id_parsed = test_env.dummy.project_alpha.version_id_parsed;

        // Create a simple profile, as FRIEND, with beta version, which is not visible to FRIEND
        // This should not include the beta version
        let profile = api
            .create_client_profile(
                "test",
                "fabric",
                "1.0.0",
                "1.20.1",
                vec![&beta_version_id, &alpha_version_id],
                FRIEND_USER_PAT,
            )
            .await;
        assert_status!(&profile, StatusCode::OK);
        let profile: ClientProfile = test::read_body_json(profile).await;
        let id = profile.id.to_string();

        // Get the profile and check the versions
        let profile_downloads = api
            .download_client_profile_from_profile_id_deserialized(&id, FRIEND_USER_PAT)
            .await;
        assert_eq!(profile_downloads.version_ids, vec![alpha_version_id_parsed]);

        // Edit profile, as FRIEND, with beta version, which is not visible to FRIEND
        // This should fail
        let resp = api
            .edit_client_profile(
                &profile.id.to_string(),
                None,
                None,
                None,
                Some(vec![&beta_version_id]),
                None,
                None,
                FRIEND_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::NO_CONTENT);

        // Get the profile and check the versions
        // Empty, because alpha is removed, and beta is not visible
        let profile_downloads = api
            .download_client_profile_from_profile_id_deserialized(&id, FRIEND_USER_PAT)
            .await;
        assert_eq!(profile_downloads.version_ids, vec![]);
    })
    .await;
}

#[actix_rt::test]
async fn verison_file_hash_collisions_with_shared_profiles() {
    // Test setup and dummy data
    with_test_environment(None, |test_env: TestEnvironment<ApiV3>| async move {
        let api = &test_env.api;

        let test_file_hash_xxx = TestFile::build_random_jar();
        let test_file_hash_yyy = TestFile::build_random_jar();
        let test_file_hash_zzz = TestFile::build_random_jar();

        // Define some comparison projects/profiles that already have these files
        // unapproved project has xxx
        let creation_data =
            get_public_project_creation_data("unapproved", Some(test_file_hash_xxx.clone()), None);
        let unapproved_project = api.create_project(creation_data, USER_USER_PAT).await;
        assert_status!(&unapproved_project, StatusCode::OK);

        // approved project has yyy
        let creation_data =
            get_public_project_creation_data("approved", Some(test_file_hash_yyy.clone()), None);
        let approved_project = api.create_project(creation_data, USER_USER_PAT).await;
        assert_status!(&approved_project, StatusCode::OK);

        // Approve as a moderator.
        let resp = api
            .edit_project(
                "approved",
                serde_json::json!({"status": "approved"}),
                MOD_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::NO_CONTENT);

        // shared profile has zzz
        let existing_profile = api
            .create_client_profile(
                "existing",
                "fabric",
                "1.0.0",
                "1.20.1",
                vec![],
                USER_USER_PAT,
            )
            .await;
        assert_status!(&existing_profile, StatusCode::OK);
        let existing_profile: ClientProfile = test::read_body_json(existing_profile).await;
        let resp = api
            .add_client_profile_overrides(
                &existing_profile.id.to_string(),
                vec![ClientProfileOverride::new(
                    test_file_hash_zzz.clone(),
                    "mods/test0.jar",
                )],
                USER_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::NO_CONTENT);

        let test_data = get_public_project_creation_data("test", None, None);
        let test_project = api.create_project(test_data, USER_USER_PAT).await;
        assert_status!(&test_project, StatusCode::OK);
        let project = test::read_body_json::<Project, _>(test_project).await;

        let test_profile = api
            .create_client_profile("test", "fabric", "1.0.0", "1.20.1", vec![], USER_USER_PAT)
            .await;
        assert_status!(&test_profile, StatusCode::OK);
        let test_profile: ClientProfile = test::read_body_json(test_profile).await;

        // 1. Existing unapproved version file, and we upload a version file with the same hash
        // -> Should succeed- OK to have two unapproved version files with the same hash
        let test_version = api
            .add_public_version(
                project.id,
                "1.0.0",
                test_file_hash_xxx.clone(),
                None,
                None,
                USER_USER_PAT,
            )
            .await;
        assert_status!(&test_version, StatusCode::OK);

        // 2. Existing approved version file, and we upload a version file with the same hash
        // -> Should fail, cannot have two approved version files with the same hash
        let test_version = api
            .add_public_version(
                project.id,
                "1.0.1",
                test_file_hash_yyy.clone(),
                None,
                None,
                USER_USER_PAT,
            )
            .await;
        assert_status!(&test_version, StatusCode::BAD_REQUEST);

        // 3. Existing unapproved version file, and we upload a shared profile override file
        // -> Should succeed- OK, but they should attach to the same file id
        let resp = api
            .add_client_profile_overrides(
                &test_profile.id.to_string(),
                vec![ClientProfileOverride::new(
                    test_file_hash_xxx.clone(),
                    "mods/test1.jar",
                )],
                USER_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::NO_CONTENT);

        let resp = api
            .delete_client_profile_overrides(
                &test_profile.id.to_string(),
                None,
                Some(&[&"mods/test1.jar"]),
                USER_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::NO_CONTENT);

        // 4. Existing approved version file, and we upload a shared profile override file
        // -> Should fail, tell user to attach as version instead of an override
        let resp = api
            .add_client_profile_overrides(
                &test_profile.id.to_string(),
                vec![ClientProfileOverride::new(
                    test_file_hash_yyy.clone(),
                    "mods/test2.jar",
                )],
                USER_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::BAD_REQUEST);

        // 5. Existing shared profile override file, and we upload a shared profile override file
        // -> Should suceced, and they should attach to the same file id
        let resp = api
            .add_client_profile_overrides(
                &test_profile.id.to_string(),
                vec![ClientProfileOverride::new(
                    test_file_hash_zzz.clone(),
                    "mods/test3.jar",
                )],
                USER_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::NO_CONTENT);

        // 6. Existing shared profile override file, and we upload a version file (as of yet unapproved)
        // -> Should succeed, and they should attach to the same file id
        // difficulty comes in on approval, which is tested in 'version_file_hash_collisions_approving'
        let test_version = api
            .add_public_version(
                project.id,
                "1.0.2",
                test_file_hash_zzz.clone(),
                None,
                None,
                USER_USER_PAT,
            )
            .await;
        assert_status!(&test_version, StatusCode::OK);
    })
    .await;
}

#[actix_rt::test]
async fn version_file_hash_collisions_approving() {
    // Test setup and dummy data
    with_test_environment(None, |test_env: TestEnvironment<ApiV3>| async move {
        let api = &test_env.api;
        let test_file_hash_xxx = TestFile::build_random_jar();
        let test_file_hash_yyy = TestFile::build_random_jar();

        // Set up four projects with colliding hashes
        // A: unapproved version file with XXX hash
        // B: unapproved version file with XXX hash
        // C: unapproved version file with YYY hash
        // C: approved project with no versions (but will contain YYY hash)

        let unapproved_project_a = api
            .create_project(
                get_public_project_creation_data(
                    "unapproved_a",
                    Some(test_file_hash_xxx.clone()),
                    None,
                ),
                USER_USER_PAT,
            )
            .await;
        assert_status!(&unapproved_project_a, StatusCode::OK);
        let unapproved_project_a = api
            .get_project_deserialized("unapproved_a", USER_USER_PAT)
            .await;

        let unapproved_project_b = api
            .create_project(
                get_public_project_creation_data(
                    "unapproved_b",
                    Some(test_file_hash_xxx.clone()),
                    None,
                ),
                USER_USER_PAT,
            )
            .await;
        assert_status!(&unapproved_project_b, StatusCode::OK);
        let unapproved_project_b = api
            .get_project_deserialized("unapproved_b", USER_USER_PAT)
            .await;

        let unapproved_project_c = api
            .create_project(
                get_public_project_creation_data(
                    "unapproved_c",
                    Some(test_file_hash_yyy.clone()),
                    None,
                ),
                USER_USER_PAT,
            )
            .await;
        assert_status!(&unapproved_project_c, StatusCode::OK);
        let unapproved_project_c = api
            .get_project_deserialized("unapproved_c", USER_USER_PAT)
            .await;

        let approved_project_d = api
            .create_project(
                get_public_project_creation_data("approved_d", None, None),
                USER_USER_PAT,
            )
            .await;
        assert_status!(&approved_project_d, StatusCode::OK);
        let approved_project_d = api
            .get_project_deserialized("approved_d", USER_USER_PAT)
            .await;

        // Approve as a moderator.
        let resp = api
            .edit_project(
                &approved_project_d.id.to_string(),
                serde_json::json!({"status": "approved"}),
                MOD_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::NO_CONTENT);

        // 1. Approve one of the projects (A), should succeed
        let resp = api
            .edit_project(
                &unapproved_project_a.id.to_string(),
                serde_json::json!({"status": "approved"}),
                MOD_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::NO_CONTENT);

        // 2. Approve the other project (B), should fail- hash collision!
        let resp = api
            .edit_project(
                &unapproved_project_b.id.to_string(),
                serde_json::json!({"status": "approved"}),
                MOD_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::BAD_REQUEST);

        // 3. Attempt to add a version with XXX to the approved project (D), should fail- hash collision!
        let resp = api
            .add_public_version(
                approved_project_d.id,
                "1.0.0",
                test_file_hash_xxx.clone(),
                None,
                None,
                USER_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::BAD_REQUEST);

        // 4. Attempt to add a version with YYY to the approved project (D), should succeed
        let resp = api
            .add_public_version(
                approved_project_d.id,
                "1.0.0",
                test_file_hash_yyy.clone(),
                None,
                None,
                USER_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::OK);

        // 5. Approve the other project (C), should fail- hash collision!
        let resp = api
            .edit_project(
                &unapproved_project_c.id.to_string(),
                serde_json::json!({"status": "approved"}),
                MOD_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::BAD_REQUEST);
    })
    .await;
}

// Has some redundant testing with version_file_hash_collisions_approving, but tests the profile side of things
#[actix_rt::test]
async fn version_file_hash_collisions_approving_with_profile() {
    // Test setup and dummy data
    with_test_environment(None, |test_env: TestEnvironment<ApiV3>| async move {
        // Set up three projects with colliding hashes
        // A: unapproved version file with XXX hash
        // C: approved project with no versions (but will contain YYY hash)
        // Also, set up a shared profile that contains an overrides with XXX hash and YYY hash
        let api = &test_env.api;
        let test_file_hash_xxx = TestFile::build_random_jar();
        let test_file_hash_yyy = TestFile::build_random_jar();

        let unapproved_project_a = api
            .create_project(
                get_public_project_creation_data(
                    "unapproved_a",
                    Some(test_file_hash_xxx.clone()),
                    None,
                ),
                USER_USER_PAT,
            )
            .await;
        assert_status!(&unapproved_project_a, StatusCode::OK);
        let unapproved_project_a = api
            .get_project_deserialized("unapproved_a", USER_USER_PAT)
            .await;

        let approved_project_c = api
            .create_project(
                get_public_project_creation_data("approved_c", None, None),
                USER_USER_PAT,
            )
            .await;
        assert_status!(&approved_project_c, StatusCode::OK);
        let approved_project_c = api
            .get_project_deserialized("approved_c", USER_USER_PAT)
            .await;

        // Approve as a moderator.
        let resp = api
            .edit_project(
                "approved_c",
                serde_json::json!({"status": "approved"}),
                MOD_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::NO_CONTENT);

        let existing_profile = api
            .create_client_profile(
                "existing",
                "fabric",
                "1.0.0",
                "1.20.1",
                vec![],
                USER_USER_PAT,
            )
            .await;
        assert_status!(&existing_profile, StatusCode::OK);
        let existing_profile: ClientProfile = test::read_body_json(existing_profile).await;

        // Attempt to add overrides for XXX and YYY to the shared profile, should succeed
        let resp = api
            .add_client_profile_overrides(
                &existing_profile.id.to_string(),
                vec![
                    ClientProfileOverride::new(test_file_hash_xxx.clone(), "mods/test0.jar"),
                    ClientProfileOverride::new(test_file_hash_yyy.clone(), "mods/test1.jar"),
                ],
                USER_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::NO_CONTENT);

        // Approve one of the projects (A), should succeed
        let resp = api
            .edit_project(
                "unapproved_a",
                serde_json::json!({"status": "approved"}),
                MOD_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::NO_CONTENT);

        // Shared profile should have its XXX override file removed and converted to a version matching
        let version_for_a = api
            .get_version_deserialized(&unapproved_project_a.versions[0].to_string(), USER_USER_PAT)
            .await;
        let profile_downloads = api
            .download_client_profile_from_profile_id_deserialized(
                &existing_profile.id.to_string(),
                USER_USER_PAT,
            )
            .await;
        assert_eq!(
            profile_downloads
                .override_cdns
                .into_iter()
                .map(|x| x.install_path)
                .collect::<HashSet<_>>(),
            [PathBuf::from("mods/test1.jar")]
                .iter()
                .cloned()
                .collect::<HashSet<_>>()
        );
        assert_eq!(profile_downloads.version_ids, vec![version_for_a.id]);

        // Attempt to add a version with YYY to the approved project (C), should succeed
        let resp = api
            .add_public_version(
                approved_project_c.id,
                "1.0.0",
                test_file_hash_yyy.clone(),
                None,
                None,
                USER_USER_PAT,
            )
            .await;
        assert_status!(&resp, StatusCode::OK);

        // Get the profile again, should have a version now
        let approved_project_c = api
            .get_project_deserialized(&approved_project_c.slug.unwrap(), USER_USER_PAT)
            .await;

        // Shared profile should have its YYY override file removed and converted to a version matching
        let version_for_c = api
            .get_version_deserialized(&approved_project_c.versions[0].to_string(), USER_USER_PAT)
            .await;
        let profile_downloads = api
            .download_client_profile_from_profile_id_deserialized(
                &existing_profile.id.to_string(),
                USER_USER_PAT,
            )
            .await;
        assert_eq!(
            profile_downloads
                .override_cdns
                .into_iter()
                .map(|x| x.install_path)
                .collect::<HashSet<_>>(),
            HashSet::<PathBuf>::new()
        );
        assert_eq!(
            profile_downloads.version_ids,
            vec![version_for_a.id, version_for_c.id]
        );
    })
    .await;
}

// TODO: Should we allow multiple overrides at the same path?
// TODO: Potentially setup a filesystem test to ensure that the files are actually being uploaded to the CDN
