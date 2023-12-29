use std::path::PathBuf;

use actix_web::test;
use common::api_v3::ApiV3;
use common::database::*;
use common::environment::with_test_environment;
use common::environment::TestEnvironment;
use labrinth::models::minecraft::profile::MinecraftProfile;

use crate::common::api_v3::minecraft_profile::MinecraftProfileOverride;
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
        let alpha_version_id_parsed = test_env.dummy.project_alpha.version_id_parsed;

        // Attempt to create a simple profile with invalid data, these should fail.
        // - fake loader
        // - fake loader version for loader
        // - unparseable version (not to be confused with parseable but nonexistent version, which is simply ignored)
        // - fake game version
        let resp = api
            .create_minecraft_profile("test", "fake-loader", "1.0.0", "1.20.1", vec![], USER_USER_PAT)
            .await;
        assert_eq!(resp.status(), 400);

        // Currently fake version for loader is not checked
        // let resp = api
        //     .create_minecraft_profile("test", "fabric", "fake", "1.20.1", vec![], USER_USER_PAT)
        //     .await;
        // assert_eq!(resp.status(), 400);

        let resp = api
            .create_minecraft_profile("test", "fabric", "1.0.0", "1.20.1", vec!["unparseable-version"], USER_USER_PAT)
            .await;
        assert_eq!(resp.status(), 400);

        let resp = api
            .create_minecraft_profile("test", "fabric", "1.0.0", "1.19.1", vec![], USER_USER_PAT)
            .await;
        assert_eq!(resp.status(), 400);

        // Create a simple profile
        // should succeed
        let profile = api
            .create_minecraft_profile("test", "fabric", "1.0.0", "1.20.1", vec![], USER_USER_PAT)
            .await;
        println!("{:?}", profile.response().body());
        assert_eq!(profile.status(), 200);
        let profile : MinecraftProfile = test::read_body_json(profile).await;
        let id = profile.id.to_string();

        // Get the profile and check the properties are correct
        let profile = api
            .get_minecraft_profile_deserialized(&id, USER_USER_PAT)
            .await;

        assert_eq!(profile.name, "test");
        assert_eq!(profile.loader, "fabric");
        assert_eq!(profile.loader_version, "1.0.0");
        assert_eq!(profile.versions, vec![]);
        assert_eq!(profile.icon_url, None);

        println!("Profile id is {}", profile.id.to_string());

        // Modify the profile illegally in the same ways
        let resp = api
            .edit_minecraft_profile(
                &profile.id.to_string(),
                None,
                Some("fake-loader"),
                None,
                None,
                USER_USER_PAT,
            )
            .await;
        println!("{:?}", resp.response().body());
        assert_eq!(resp.status(), 400);

        // Currently fake version for loader is not checked
        // let resp = api
        //     .edit_minecraft_profile(
        //         &profile.id.to_string(),
        //         None,
        //         Some("fabric"),
        //         Some("fake"),
        //         None,
        //         USER_USER_PAT,
        //     )
        //     .await;

        println!("{:?}", resp.response().body());
        assert_eq!(resp.status(), 400);

        let resp = api
            .edit_minecraft_profile(
                &profile.id.to_string(),
                None,
                Some("fabric"),
                None,
                Some(vec!["unparseable-version"]),
                USER_USER_PAT,
            )
            .await;
        println!("{:?}", resp.response().body());
        assert_eq!(resp.status(), 400);

        // Can't modify the profile as another user
        let resp = api
            .edit_minecraft_profile(
                &profile.id.to_string(),
                None,
                Some("fabric"),
                None,
                None,
                FRIEND_USER_PAT,
            )
            .await;
        println!("{:?}", resp.response().body());
        assert_eq!(resp.status(), 401);

        //  Get and make sure the properties are the same
        let profile = api
            .get_minecraft_profile_deserialized(&id, USER_USER_PAT)
            .await;

        assert_eq!(profile.name, "test");
        assert_eq!(profile.loader, "fabric");
        assert_eq!(profile.loader_version, "1.0.0");
        assert_eq!(profile.versions, vec![]);
        assert_eq!(profile.icon_url, None);

        // A successful modification
        let resp = api
            .edit_minecraft_profile(
                &profile.id.to_string(),
                Some("test2"),
                Some("forge"),
                Some("1.0.1"),
                Some(vec![&alpha_version_id]),
                USER_USER_PAT,
            )
            .await;
        println!("{:?}", resp.response().body());
        assert_eq!(resp.status(), 200);

        // Get the profile and check the properties
        let profile = api
            .get_minecraft_profile_deserialized(&id, USER_USER_PAT)
            .await;

        println!("{:?}", serde_json::to_string(&profile)); 

        assert_eq!(profile.name, "test2");
        assert_eq!(profile.loader, "forge");
        assert_eq!(profile.loader_version, "1.0.1");
        assert_eq!(profile.versions, vec![alpha_version_id_parsed]);
        assert_eq!(profile.icon_url, None);

        // Modify the profile again
        let resp = api
            .edit_minecraft_profile(
                &profile.id.to_string(),
                Some("test3"),
                Some("fabric"),
                Some("1.0.0"),
                Some(vec![]),
                USER_USER_PAT,
            )
            .await;
        println!("{:?}", resp.response().body());
        assert_eq!(resp.status(), 200);

        // Get the profile and check the properties
        let profile = api
            .get_minecraft_profile_deserialized(&id, USER_USER_PAT)
            .await;

        assert_eq!(profile.name, "test3");
        assert_eq!(profile.loader, "fabric");
        assert_eq!(profile.loader_version, "1.0.0");
        assert_eq!(profile.versions, vec![]);
        assert_eq!(profile.icon_url, None);
        
    }).await;
}

#[actix_rt::test]
async fn download_profile() {
    with_test_environment(None, |test_env: TestEnvironment<ApiV3>| async move {
    // Get download links for a created profile (including failure), create a share link, and create the correct number of tokens based on that
    // They should expire after a time
    let api = &test_env.api;

    // Create a simple profile
    let profile = api
        .create_minecraft_profile("test", "fabric", "1.0.0" ,"1.20.1", vec![], USER_USER_PAT)
        .await;
    assert_eq!(profile.status(), 200);  
    let profile : MinecraftProfile = test::read_body_json(profile).await;
    let id = profile.id.to_string();

    // Add an override file to the profile
    let resp = api
        .add_minecraft_profile_overrides(&id, vec![MinecraftProfileOverride::new(TestFile::BasicMod, "mods/test.jar")], USER_USER_PAT)
        .await;
    println!("{:?}", resp.response().body());
    assert_eq!(resp.status(), 204);
    
    println!("Here123123123123213");

    // As 'user', try to generate a download link for the profile
    let share_link = api
        .generate_minecraft_profile_share_link_deserialized(&id, USER_USER_PAT)
        .await;
    // Links should add up
    assert_eq!(share_link.uses_remaining, 5);
    assert_eq!(share_link.url , format!("{}/v3/minecraft/profile/{}/download/{}", dotenvy::var("SELF_ADDR").unwrap(), id, share_link.url_identifier));

    // As 'friend', try to get the download links for the profile
    // *Anyone* with the link can get
    let mut download = api
        .download_minecraft_profile_deserialized(&share_link.url_identifier, FRIEND_USER_PAT)
        .await;

    // Download url should be:
    // - CDN url
    // "custom_files"
    // - hash
    assert_eq!(download.override_cdns.len(), 1);
    let override_file_url = download.override_cdns.remove(0).0;
    let hash = sha1::Sha1::from(&TestFile::BasicMod.bytes()).hexdigest();
    assert_eq!(override_file_url, format!("{}/custom_files/{}", dotenvy::var("CDN_URL").unwrap(), hash));

       
    // This generates a token, and now the link should have 4 uses remaining
    let share_link = api
        .get_minecraft_profile_share_link_deserialized(&id, &share_link.url_identifier, USER_USER_PAT)
        .await;
    println!("\n\n{:?}", serde_json::to_string(&share_link));
    assert_eq!(share_link.uses_remaining, 4);

    // Check cloudflare helper route with a bad token (eg: the profile id), should fail
    let resp = api
        .check_download_minecraft_profile_token(&share_link.url_identifier, &override_file_url).await;
    println!("{:?}", resp.response().body());
    assert_eq!(resp.status(), 401);
    let resp = api
    .check_download_minecraft_profile_token(&share_link.url, &override_file_url).await;
println!("{:?}", resp.response().body());
assert_eq!(resp.status(), 401);

    let resp = api
        .check_download_minecraft_profile_token(&id, &override_file_url).await;
    assert_eq!(resp.status(), 401);

    // Check cloudflare helper route to confirm this is a valid allowable access token
    // We attach it as an authorization token and call the route
    let download = api
        .check_download_minecraft_profile_token(&download.auth_token, &override_file_url).await;
    println!("{:?}", download.response().body());
    assert_eq!(download.status(), 200);
    

    }).await;
}

#[actix_rt::test]
async fn add_remove_profile_icon() {
    with_test_environment(None, |test_env: TestEnvironment<ApiV3>| async move {
        // Add and remove an icon from a profile
        let api = &test_env.api;

        // Create a simple profile
        let profile = api
            .create_minecraft_profile("test", "fabric", "1.0.0", "1.20.1", vec![], USER_USER_PAT)
            .await;
        assert_eq!(profile.status(), 200);
        let profile : MinecraftProfile = test::read_body_json(profile).await;

        // Add an icon to the profile
        let icon = api
            .edit_minecraft_profile_icon(&profile.id.to_string(), Some(DummyImage::SmallIcon.get_icon_data()), USER_USER_PAT)
            .await;
        println!("{:?}", icon.response().body());
        assert_eq!(icon.status(), 204);

        // Get the profile and check the icon
        let profile = api
            .get_minecraft_profile_deserialized(&profile.id.to_string(), USER_USER_PAT)
            .await;
        assert!(profile.icon_url.is_some());

        // Remove the icon from the profile
        let icon = api
            .edit_minecraft_profile_icon(&profile.id.to_string(), None, USER_USER_PAT)
            .await;
        assert_eq!(icon.status(), 204);

        // Get the profile and check the icon
        let profile = api
            .get_minecraft_profile_deserialized(&profile.id.to_string(), USER_USER_PAT)
            .await;
        assert!(profile.icon_url.is_none());
    }).await;
}

#[actix_rt::test]
async fn add_remove_profile_versions() {
    with_test_environment(None, |test_env: TestEnvironment<ApiV3>| async move {
        // Add and remove versions from a profile
        let api = &test_env.api;
        let alpha_version_id = test_env.dummy.project_alpha.version_id.to_string();
        // Create a simple profile
        let profile = api
            .create_minecraft_profile("test", "fabric", "1.0.0", "1.20.1", vec![], USER_USER_PAT)
            .await;
        assert_eq!(profile.status(), 200);
        let profile : MinecraftProfile = test::read_body_json(profile).await;

        // Add a hosted version to the profile
        let resp = api
            .edit_minecraft_profile(&profile.id.to_string(), None, None, None, Some(vec![&alpha_version_id]), USER_USER_PAT)
            .await;
        println!("{:?}", resp.response().body());
        assert_eq!(resp.status(), 200);

        // Add an override file to the profile
        let resp = api
            .add_minecraft_profile_overrides(&profile.id.to_string(), vec![MinecraftProfileOverride::new(TestFile::BasicMod, "mods/test.jar")], USER_USER_PAT)
            .await;
        println!("{:?}", resp.response().body());
        assert_eq!(resp.status(), 204);

        // Get the profile and check the versions
        let profile = api
            .get_minecraft_profile_deserialized(&profile.id.to_string(), USER_USER_PAT)
            .await;
        assert_eq!(profile.versions, vec![test_env.dummy.project_alpha.version_id_parsed]);
        assert_eq!(profile.override_install_paths, vec![PathBuf::from("mods/test.jar")]);

        // 
    }).await;
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
            .create_minecraft_profile("test", "fabric", "1.0.0", "1.20.1", vec![&beta_version_id, &alpha_version_id], FRIEND_USER_PAT)
            .await;
        println!("{:?}", profile.response().body());
        assert_eq!(profile.status(), 200);
        let profile : MinecraftProfile = test::read_body_json(profile).await;
        assert_eq!(profile.versions, vec![alpha_version_id_parsed]);
        
        // Edit profile, as FRIEND, with beta version, which is not visible to FRIEND
        // This should fail
        let resp = api
            .edit_minecraft_profile(&profile.id.to_string(), None, None, None, Some(vec![&beta_version_id]), FRIEND_USER_PAT)
            .await;
        println!("{:?}", resp.response().body());
        assert_eq!(resp.status(), 200);

        // Get the profile and check the versions
        // Empty, because alpha is removed, and beta is not visible
        let profile = api
            .get_minecraft_profile_deserialized(&profile.id.to_string(), FRIEND_USER_PAT)
            .await;
        assert_eq!(profile.versions, vec![]);
    }).await;
}

// try all file system related thinghs
// go thru all the stuff in the linear issue
