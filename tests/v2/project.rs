use crate::common::{
    database::{ENEMY_USER_PAT, MOD_USER_PAT, USER_USER_PAT},
    dummy_data::{TestFile, DUMMY_CATEGORIES},
    environment::TestEnvironment,
    request_data,
};
use itertools::Itertools;
use serde_json::json;

#[actix_rt::test]
async fn test_project_type_sanity() {
    let test_env = TestEnvironment::build(None).await;
    let api = &test_env.v2;

    // Perform all other patch tests on both 'mod' and 'modpack'
    let test_creation_mod = request_data::get_public_project_creation_data(
        "test-mod",
        Some(TestFile::build_random_jar()),
    );
    let test_creation_modpack = request_data::get_public_project_creation_data(
        "test-modpack",
        Some(TestFile::build_random_mrpack()),
    );
    for (mod_or_modpack, test_creation_data) in [
        ("mod", test_creation_mod),
        ("modpack", test_creation_modpack),
    ] {
        let (test_project, test_version) = api
            .add_public_project(test_creation_data, USER_USER_PAT)
            .await;
        let test_project_slug = test_project.slug.as_ref().unwrap();

        assert_eq!(test_project.project_type, mod_or_modpack);
        assert_eq!(test_project.loaders, vec!["fabric"]);
        assert_eq!(
            test_version[0].loaders.iter().map(|x| &x.0).collect_vec(),
            vec!["fabric"]
        );

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
}

#[actix_rt::test]
pub async fn test_patch_project() {
    let test_env = TestEnvironment::build(None).await;
    let api = &test_env.v2;

    let alpha_project_slug = &test_env.dummy.as_ref().unwrap().project_alpha.project_slug;
    let beta_project_slug = &test_env.dummy.as_ref().unwrap().project_beta.project_slug;

    // First, we do some patch requests that should fail.
    // Failure because the user is not authorized.
    let resp = api
        .edit_project(
            alpha_project_slug,
            json!({
                "title": "Test_Add_Project project - test 1",
            }),
            ENEMY_USER_PAT,
        )
        .await;
    assert_eq!(resp.status(), 401);

    // Failure because we are setting URL fields to invalid urls.
    for url_type in ["issues_url", "source_url", "wiki_url", "discord_url"] {
        let resp = api
            .edit_project(
                alpha_project_slug,
                json!({
                    url_type: "w.fake.url",
                }),
                USER_USER_PAT,
            )
            .await;
        assert_eq!(resp.status(), 400);
    }

    // Failure because these are illegal requested statuses for a normal user.
    for req in ["unknown", "processing", "withheld", "scheduled"] {
        let resp = api
            .edit_project(
                alpha_project_slug,
                json!({
                    "requested_status": req,
                }),
                USER_USER_PAT,
            )
            .await;
        assert_eq!(resp.status(), 400);
    }

    // Failure because these should not be able to be set by a non-mod
    for key in ["moderation_message", "moderation_message_body"] {
        let resp = api
            .edit_project(
                alpha_project_slug,
                json!({
                    key: "test",
                }),
                USER_USER_PAT,
            )
            .await;
        assert_eq!(resp.status(), 401);

        // (should work for a mod, though)
        let resp = api
            .edit_project(
                alpha_project_slug,
                json!({
                    key: "test",
                }),
                MOD_USER_PAT,
            )
            .await;
        assert_eq!(resp.status(), 204);
    }

    // Failed patch to alpha slug:
    // - slug collision with beta
    // - too short slug
    // - too long slug
    // - not url safe slug
    // - not url safe slug
    for slug in [
        beta_project_slug,
        "a",
        &"a".repeat(100),
        "not url safe%&^!#$##!@#$%^&*()",
    ] {
        let resp = api
            .edit_project(
                alpha_project_slug,
                json!({
                    "slug": slug, // the other dummy project has this slug
                }),
                USER_USER_PAT,
            )
            .await;
        assert_eq!(resp.status(), 400);
    }

    // Not allowed to directly set status, as 'beta_project_slug' (the other project) is "processing" and cannot have its status changed like this.
    let resp = api
        .edit_project(
            beta_project_slug,
            json!({
                "status": "private"
            }),
            USER_USER_PAT,
        )
        .await;
    assert_eq!(resp.status(), 401);

    // Sucessful request to patch many fields.
    let resp = api
        .edit_project(
            alpha_project_slug,
            json!({
                "slug": "newslug",
                "title": "New successful title",
                "description": "New successful description",
                "body": "New successful body",
                "categories": [DUMMY_CATEGORIES[0]],
                "license_id": "MIT",
                "issues_url": "https://github.com",
                "discord_url": "https://discord.gg",
                "wiki_url": "https://wiki.com",
                "client_side": "optional",
                "server_side": "required",
                "donation_urls": [{
                    "id": "patreon",
                    "platform": "Patreon",
                    "url": "https://patreon.com"
                }]
            }),
            USER_USER_PAT,
        )
        .await;
    assert_eq!(resp.status(), 204);

    // Old slug no longer works
    let resp = api.get_project(alpha_project_slug, USER_USER_PAT).await;
    assert_eq!(resp.status(), 404);

    // New slug does work
    let project = api.get_project_deserialized("newslug", USER_USER_PAT).await;

    assert_eq!(project.slug.unwrap(), "newslug");
    assert_eq!(project.title, "New successful title");
    assert_eq!(project.description, "New successful description");
    assert_eq!(project.body, "New successful body");
    assert_eq!(project.categories, vec![DUMMY_CATEGORIES[0]]);
    assert_eq!(project.license.id, "MIT");
    assert_eq!(project.issues_url, Some("https://github.com".to_string()));
    assert_eq!(project.discord_url, Some("https://discord.gg".to_string()));
    assert_eq!(project.wiki_url, Some("https://wiki.com".to_string()));
    assert_eq!(project.client_side.as_str(), "optional");
    assert_eq!(project.server_side.as_str(), "required");
    assert_eq!(project.donation_urls.unwrap()[0].url, "https://patreon.com");

    // Cleanup test db
    test_env.cleanup().await;
}
