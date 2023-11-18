use common::{
    database::{FRIEND_USER_ID, FRIEND_USER_PAT, USER_USER_ID, USER_USER_PAT},
    environment::with_test_environment,
};

use crate::common::api_v3::request_data::get_public_project_creation_data;
use common::dummy_data::TestFile;

mod common;

// user GET (permissions, different users)
// users GET
// user auth
// user projects get
// user collections get
// patch user
// patch user icon
// user follows

#[actix_rt::test]
pub async fn get_user_projects_after_creating_project_returns_new_project() {
    with_test_environment(|test_env| async move {
        let api = test_env.v3;
        api.get_user_projects_deserialized(USER_USER_ID, USER_USER_PAT)
            .await;

        let (project, _) = api
            .add_public_project(
                get_public_project_creation_data("slug", Some(TestFile::BasicMod), None),
                USER_USER_PAT,
            )
            .await;

        let resp_projects = api
            .get_user_projects_deserialized(USER_USER_ID, USER_USER_PAT)
            .await;
        assert!(resp_projects.iter().any(|p| p.id == project.id));
    })
    .await;
}

#[actix_rt::test]
pub async fn get_user_projects_after_deleting_project_shows_removal() {
    with_test_environment(|test_env| async move {
        let api = test_env.v3;
        let (project, _) = api
            .add_public_project(
                get_public_project_creation_data("iota", Some(TestFile::BasicMod), None),
                USER_USER_PAT,
            )
            .await;
        api.get_user_projects_deserialized(USER_USER_ID, USER_USER_PAT)
            .await;

        api.remove_project(project.slug.as_ref().unwrap(), USER_USER_PAT)
            .await;

        let resp_projects = api
            .get_user_projects_deserialized(USER_USER_ID, USER_USER_PAT)
            .await;
        assert!(!resp_projects.iter().any(|p| p.id == project.id));
    })
    .await;
}

#[actix_rt::test]
pub async fn get_user_projects_after_joining_team_shows_team_projects() {
    with_test_environment(|test_env| async move {
        let alpha_team_id = &test_env.dummy.as_ref().unwrap().project_alpha.team_id;
        let alpha_project_id = &test_env.dummy.as_ref().unwrap().project_alpha.project_id;
        let api = test_env.v3;
        api.get_user_projects_deserialized(FRIEND_USER_ID, FRIEND_USER_PAT)
            .await;

        api.add_user_to_team(alpha_team_id, FRIEND_USER_ID, None, None, USER_USER_PAT)
            .await;
        api.join_team(alpha_team_id, FRIEND_USER_PAT).await;

        let projects = api
            .get_user_projects_deserialized(FRIEND_USER_ID, FRIEND_USER_PAT)
            .await;
        assert!(projects
            .iter()
            .any(|p| p.id.to_string() == *alpha_project_id));
    })
    .await;
}

#[actix_rt::test]
pub async fn get_user_projects_after_leaving_team_shows_no_team_projects() {
    with_test_environment(|test_env| async move {
        let alpha_team_id = &test_env.dummy.as_ref().unwrap().project_alpha.team_id;
        let alpha_project_id = &test_env.dummy.as_ref().unwrap().project_alpha.project_id;
        let api = test_env.v3;
        api.add_user_to_team(alpha_team_id, FRIEND_USER_ID, None, None, USER_USER_PAT)
            .await;
        api.join_team(alpha_team_id, FRIEND_USER_PAT).await;
        api.get_user_projects_deserialized(FRIEND_USER_ID, FRIEND_USER_PAT)
            .await;

        api.remove_from_team(alpha_team_id, FRIEND_USER_ID, USER_USER_PAT)
            .await;

        let projects = api
            .get_user_projects_deserialized(FRIEND_USER_ID, FRIEND_USER_PAT)
            .await;
        assert!(!projects
            .iter()
            .any(|p| p.id.to_string() == *alpha_project_id));
    })
    .await;
}
