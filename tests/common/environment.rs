#![allow(dead_code)]

use super::{
    asserts::assert_status,
    database::{TemporaryDatabase, FRIEND_USER_ID, USER_USER_PAT},
    dummy_data,
};
use crate::common::setup;
use actix_http::StatusCode;
use actix_web::{dev::ServiceResponse, test, App};
use futures::Future;
use labrinth::models::{notifications::Notification, projects::Project};
use serde_json::json;

pub async fn with_test_environment<Fut>(f: impl FnOnce(TestEnvironment) -> Fut)
where
    Fut: Future<Output = ()>,
{
    let test_env = TestEnvironment::build_with_dummy().await;
    let db = test_env.db.clone();

    f(test_env).await;

    db.cleanup().await;
}

// A complete test environment, with a test actix app and a database.
// Must be called in an #[actix_rt::test] context. It also simulates a
// temporary sqlx db like #[sqlx::test] would.
// Use .call(req) on it directly to make a test call as if test::call_service(req) were being used.
pub struct TestEnvironment {
    test_app: Box<dyn LocalService>,
    pub db: TemporaryDatabase,

    pub dummy: Option<dummy_data::DummyData>,
}

impl TestEnvironment {
    pub async fn build_with_dummy() -> Self {
        let mut test_env = Self::build().await;
        let dummy = dummy_data::add_dummy_data(&test_env).await;
        test_env.dummy = Some(dummy);
        test_env
    }

    pub async fn build() -> Self {
        let db = TemporaryDatabase::create().await;
        let labrinth_config = setup(&db).await;
        let app = App::new().configure(|cfg| labrinth::app_config(cfg, labrinth_config.clone()));
        let test_app = test::init_service(app).await;
        Self {
            test_app: Box::new(test_app),
            db,
            dummy: None,
        }
    }
    pub async fn cleanup(self) {
        self.db.cleanup().await;
    }

    pub async fn call(&self, req: actix_http::Request) -> ServiceResponse {
        self.test_app.call(req).await.unwrap()
    }

    pub async fn generate_friend_user_notification(&self) {
        let resp = self
            .add_user_to_team(
                &self.dummy.as_ref().unwrap().alpha_team_id,
                FRIEND_USER_ID,
                USER_USER_PAT,
            )
            .await;
        assert_status(resp, StatusCode::NO_CONTENT);
    }

    pub async fn remove_project(&self, project_slug_or_id: &str, pat: &str) -> ServiceResponse {
        let req = test::TestRequest::delete()
            .uri(&format!("/v2/project/{project_slug_or_id}"))
            .append_header(("Authorization", pat))
            .to_request();
        let resp = self.call(req).await;
        assert_eq!(resp.status(), 204);
        resp
    }

    pub async fn get_user_projects_deserialized(
        &self,
        user_id_or_username: &str,
        pat: &str,
    ) -> Vec<Project> {
        let req = test::TestRequest::get()
            .uri(&format!("/v2/user/{}/projects", user_id_or_username))
            .append_header(("Authorization", pat))
            .to_request();
        let resp = self.call(req).await;
        assert_eq!(resp.status(), 200);
        test::read_body_json(resp).await
    }

    pub async fn add_user_to_team(
        &self,
        team_id: &str,
        user_id: &str,
        pat: &str,
    ) -> ServiceResponse {
        let req = test::TestRequest::post()
            .uri(&format!("/v2/team/{team_id}/members"))
            .append_header(("Authorization", pat))
            .set_json(json!( {
                "user_id": user_id
            }))
            .to_request();
        self.call(req).await
    }

    pub async fn join_team(&self, team_id: &str, pat: &str) -> ServiceResponse {
        let req = test::TestRequest::post()
            .uri(&format!("/v2/team/{team_id}/join"))
            .append_header(("Authorization", pat))
            .to_request();
        self.call(req).await
    }

    pub async fn remove_from_team(
        &self,
        team_id: &str,
        user_id: &str,
        pat: &str,
    ) -> ServiceResponse {
        let req = test::TestRequest::delete()
            .uri(&format!("/v2/team/{team_id}/members/{user_id}"))
            .append_header(("Authorization", pat))
            .to_request();
        self.call(req).await
    }

    pub async fn get_user_notifications_deserialized(
        &self,
        user_id: &str,
        pat: &str,
    ) -> Vec<Notification> {
        let req = test::TestRequest::get()
            .uri(&format!("/v2/user/{user_id}/notifications"))
            .append_header(("Authorization", pat))
            .to_request();
        let resp = self.call(req).await;
        test::read_body_json(resp).await
    }

    pub async fn mark_notification_read(
        &self,
        notification_id: &str,
        pat: &str,
    ) -> ServiceResponse {
        let req = test::TestRequest::patch()
            .uri(&format!("/v2/notification/{notification_id}"))
            .append_header(("Authorization", pat))
            .to_request();
        self.call(req).await
    }

    pub async fn delete_notification(&self, notification_id: &str, pat: &str) -> ServiceResponse {
        let req = test::TestRequest::delete()
            .uri(&format!("/v2/notification/{notification_id}"))
            .append_header(("Authorization", pat))
            .to_request();
        self.call(req).await
    }
}

trait LocalService {
    fn call(
        &self,
        req: actix_http::Request,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ServiceResponse, actix_web::Error>>>,
    >;
}
impl<S> LocalService for S
where
    S: actix_web::dev::Service<
        actix_http::Request,
        Response = ServiceResponse,
        Error = actix_web::Error,
    >,
    S::Future: 'static,
{
    fn call(
        &self,
        req: actix_http::Request,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ServiceResponse, actix_web::Error>>>,
    > {
        Box::pin(self.call(req))
    }
}
