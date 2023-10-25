use assert_matches::assert_matches;
use common::{
    api_v2::organization::deser_organization,
    database::{FRIEND_USER_PAT, USER_USER_ID, USER_USER_PAT},
    environment::with_test_environment,
    request_data::get_public_project_creation_data,
};
use labrinth::models::feed_item::FeedItemBody;

use crate::common::dummy_data::DummyProjectAlpha;

mod common;

#[actix_rt::test]
async fn user_feed_before_following_user_shows_no_projects() {
    with_test_environment(|env| async move {
        let feed = env.v3.get_feed(FRIEND_USER_PAT).await;

        assert_eq!(feed.len(), 0);
    })
    .await
}

#[actix_rt::test]
async fn user_feed_after_following_user_shows_previously_created_public_projects() {
    with_test_environment(|env| async move {
        let DummyProjectAlpha {
            project_id: alpha_project_id,
            ..
        } = env.dummy.as_ref().unwrap().project_alpha.clone();
        env.v3.follow_user(USER_USER_ID, FRIEND_USER_PAT).await;

        let feed = env.v3.get_feed(FRIEND_USER_PAT).await;

        assert_eq!(feed.len(), 1);
        assert_matches!(
            feed[0].body,
            FeedItemBody::ProjectCreated { project_id, .. } if project_id.to_string() == alpha_project_id
        )
    })
    .await
}

#[actix_rt::test]
async fn user_feed_when_following_user_that_creates_project_as_org_only_shows_event_when_following_org(
) {
    with_test_environment(|env| async move {
        let resp = env
            .v2
            .create_organization("test", "desc", USER_USER_ID)
            .await;
        let organization = deser_organization(resp).await;
        let org_id = organization.id.to_string();
        let project_create_data = get_public_project_creation_data("a", None, Some(&org_id));
        let (project, _) = env
            .v2
            .add_public_project(project_create_data, USER_USER_PAT)
            .await;

        env.v3.follow_user(USER_USER_ID, FRIEND_USER_PAT).await;
        let feed = env.v3.get_feed(FRIEND_USER_PAT).await;
        assert_eq!(feed.len(), 1);
        assert_matches!(feed[0].body, FeedItemBody::ProjectCreated { project_id, .. } if project_id != project.id);

        env.v3.follow_organization(&org_id, FRIEND_USER_PAT).await;
        let feed = env.v3.get_feed(FRIEND_USER_PAT).await;
        assert_eq!(feed.len(), 1);
        assert_matches!(feed[0].body, FeedItemBody::ProjectCreated { project_id, .. } if project_id == project.id);
    })
    .await;
}
