#![allow(dead_code)]

use std::{rc::Rc, sync::Arc};

use super::{database::TemporaryDatabase, dummy_data};
use crate::common::setup;
use actix_web::{dev::ServiceResponse, test, App};
use futures::Future;

pub async fn with_test_environment<Fut>(f: impl FnOnce(TestEnvironment) -> Fut)
where
    Fut: Future<Output = ()>,
{
    let test_env = TestEnvironment::build(None).await;
    let db = test_env.db.clone();

    f(test_env).await;

    db.cleanup().await;
}

// A complete test environment, with a test actix app and a database.
// Must be called in an #[actix_rt::test] context. It also simulates a
// temporary sqlx db like #[sqlx::test] would.
// Use .call(req) on it directly to make a test call as if test::call_service(req) were being used.
#[derive(Clone)]
pub struct TestEnvironment {
    test_app: Rc<dyn LocalService>, // Rc as it's not Send
    pub db: TemporaryDatabase,

    pub dummy: Option<Arc<dummy_data::DummyData>>,
}

impl TestEnvironment {
    pub async fn build(max_connections: Option<u32>) -> Self {
        let db = TemporaryDatabase::create(max_connections).await;
        let mut test_env = Self::build_with_db(db).await;

        let dummy = dummy_data::get_dummy_data(&test_env).await;
        test_env.dummy = Some(Arc::new(dummy));
        test_env
    }

    pub async fn build_with_db(db: TemporaryDatabase) -> Self {
        let labrinth_config = setup(&db).await;
        let app = App::new().configure(|cfg| labrinth::app_config(cfg, labrinth_config.clone()));
        let test_app = test::init_service(app).await;
        Self {
            test_app: Rc::new(test_app),
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
