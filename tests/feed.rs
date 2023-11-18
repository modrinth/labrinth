use crate::common::{
    asserts::{assert_feed_contains_project_created, assert_feed_contains_version_created},
    dummy_data::DummyProjectAlpha,
};
use assert_matches::assert_matches;
use common::{
    database::{FRIEND_USER_PAT, USER_USER_ID, USER_USER_PAT},
    environment::with_test_environment,
};
use labrinth::models::{feeds::FeedItemBody, ids::base62_impl::parse_base62, projects::ProjectId};

mod common;

#[actix_rt::test]
async fn get_feed_before_following_user_shows_no_projects() {
    with_test_environment(|env| async move {
        let feed = env.v3.get_feed(FRIEND_USER_PAT).await;

        assert_eq!(feed.len(), 0);
    })
    .await
}

#[actix_rt::test]
async fn get_feed_after_following_user_shows_previously_created_public_projects() {
    with_test_environment(|env| async move {
        let DummyProjectAlpha {
            project_id: alpha_project_id,
            ..
        } = env.dummy.as_ref().unwrap().project_alpha.clone();
        env.v3.follow_user(USER_USER_ID, FRIEND_USER_PAT).await;

        let feed = env.v3.get_feed(FRIEND_USER_PAT).await;

        assert_eq!(feed.len(), 1);
        assert_feed_contains_project_created(
            &feed,
            ProjectId(parse_base62(&alpha_project_id).unwrap()),
        );
    })
    .await
}

#[actix_rt::test]
async fn get_feed_after_following_user_shows_previously_created_public_versions() {
    with_test_environment(|env| async move {
        let DummyProjectAlpha {
            project_id: alpha_project_id,
            ..
        } = env.dummy.as_ref().unwrap().project_alpha.clone();

        // Add version
        let v = env
            .v3
            .create_default_version(&alpha_project_id, None, USER_USER_PAT)
            .await;

        env.v3.follow_user(USER_USER_ID, FRIEND_USER_PAT).await;

        let feed = env.v3.get_feed(FRIEND_USER_PAT).await;

        assert_eq!(feed.len(), 2);
        assert_feed_contains_project_created(
            &feed,
            ProjectId(parse_base62(&alpha_project_id).unwrap()),
        );
        assert_feed_contains_version_created(&feed, v.id);
        // Notably, this should *not* have a projectupdated from the publishing.
    })
    .await
}

#[actix_rt::test]
async fn get_feed_when_following_user_that_creates_project_as_org_only_shows_event_when_following_org(
) {
    with_test_environment(|env| async move {
        let org_id = env.v3.create_organization_deserialized("test", "desc", USER_USER_PAT).await.id.to_string();
        let project = env.v3.add_default_org_project(&org_id, USER_USER_PAT).await;

        env.v3.follow_user(USER_USER_ID, FRIEND_USER_PAT).await;
        let feed = env.v3.get_feed(FRIEND_USER_PAT).await;
        assert_eq!(feed.len(), 1);

        assert_matches!(feed[0].body, FeedItemBody::ProjectPublished { project_id, .. } if project_id != project.id);

        env.v3.follow_organization(&org_id, FRIEND_USER_PAT).await;
        let feed = env.v3.get_feed(FRIEND_USER_PAT).await;
        assert_eq!(feed.len(), 2);
        assert_feed_contains_project_created(&feed, project.id);
    })
    .await;
}

#[actix_rt::test]
async fn get_feed_after_unfollowing_user_no_longer_shows_feed_items() {
    with_test_environment(|env| async move {
        env.v3.follow_user(USER_USER_ID, FRIEND_USER_PAT).await;

        env.v3.unfollow_user(USER_USER_ID, FRIEND_USER_PAT).await;
        let feed = env.v3.get_feed(FRIEND_USER_PAT).await;

        assert_eq!(feed.len(), 0);
    })
    .await;
}

#[actix_rt::test]
async fn get_feed_after_unfollowing_organization_no_longer_shows_feed_items() {
    with_test_environment(|env| async move {
        let org_id = env
            .v3
            .create_organization_deserialized("test", "desc", USER_USER_PAT)
            .await
            .id
            .to_string();
        env.v3.add_default_org_project(&org_id, USER_USER_PAT).await;
        env.v3.follow_organization(&org_id, FRIEND_USER_PAT).await;

        env.v3.unfollow_organization(&org_id, FRIEND_USER_PAT).await;
        let feed = env.v3.get_feed(FRIEND_USER_PAT).await;

        assert_eq!(feed.len(), 0);
    })
    .await;
}
