use common::{
    database::{FRIEND_USER_ID, FRIEND_USER_PAT, USER_USER_ID, USER_USER_PAT},
    dummy_data,
    environment::with_test_environment,
};

use crate::common::dummy_data::DummyJarFile;

mod common;

#[actix_rt::test]
pub async fn get_user_projects_after_creating_project_returns_new_project() {
    with_test_environment(|test_env| async move {
        test_env
            .get_user_projects_deserialized(USER_USER_ID, USER_USER_PAT)
            .await;

        let (project, _) =
            dummy_data::add_public_dummy_project("slug", DummyJarFile::BasicMod, &test_env).await;

        let resp_projects = test_env
            .get_user_projects_deserialized(USER_USER_ID, USER_USER_PAT)
            .await;
        assert!(resp_projects.iter().any(|p| p.id == project.id));
    })
    .await;
}

#[actix_rt::test]
pub async fn get_user_projects_after_deleting_project_shows_removal() {
    with_test_environment(|test_env| async move {
        let (project, _) =
            dummy_data::add_public_dummy_project("iota", DummyJarFile::BasicMod, &test_env).await;
        test_env
            .get_user_projects_deserialized(USER_USER_ID, USER_USER_PAT)
            .await;

        test_env
            .remove_project(&project.slug.as_ref().unwrap(), USER_USER_PAT)
            .await;

        let resp_projects = test_env
            .get_user_projects_deserialized(USER_USER_ID, USER_USER_PAT)
            .await;
        assert!(!resp_projects.iter().any(|p| p.id == project.id));
    })
    .await;
}

#[actix_rt::test]
pub async fn get_user_projects_after_joining_team_shows_team_projects() {
    with_test_environment(|test_env| async move {
        let alpha_team_id = &test_env.dummy.as_ref().unwrap().alpha_team_id;
        let alpha_project_id = &test_env.dummy.as_ref().unwrap().alpha_project_id;
        test_env
            .get_user_projects_deserialized(FRIEND_USER_ID, FRIEND_USER_PAT)
            .await;

        test_env
            .add_user_to_team(alpha_team_id, FRIEND_USER_ID, USER_USER_PAT)
            .await;
        test_env.join_team(&alpha_team_id, FRIEND_USER_PAT).await;

        let projects = test_env
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
        let alpha_team_id = &test_env.dummy.as_ref().unwrap().alpha_team_id;
        let alpha_project_id = &test_env.dummy.as_ref().unwrap().alpha_project_id;
        test_env
            .add_user_to_team(alpha_team_id, FRIEND_USER_ID, USER_USER_PAT)
            .await;
        test_env.join_team(&alpha_team_id, FRIEND_USER_PAT).await;
        test_env
            .get_user_projects_deserialized(FRIEND_USER_ID, FRIEND_USER_PAT)
            .await;

        test_env
            .remove_from_team(&alpha_team_id, FRIEND_USER_ID, USER_USER_PAT)
            .await;

        let projects = test_env
            .get_user_projects_deserialized(FRIEND_USER_ID, FRIEND_USER_PAT)
            .await;
        assert!(!projects
            .iter()
            .any(|p| p.id.to_string() == *alpha_project_id));
    })
    .await;
}
