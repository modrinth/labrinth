#![allow(dead_code)]

use std::net::SocketAddr;
use super::api_common::{Api, ApiBuildable};
use async_trait::async_trait;
use axum::http::{HeaderValue, HeaderName};
use axum_test::{TestServer, TestResponse};
use labrinth::LabrinthConfig;
use std::sync::Arc;

pub mod project;
pub mod request_data;
pub mod tags;
pub mod team;
pub mod user;
pub mod version;

#[derive(Clone)]
pub struct ApiV2 {
    pub test_server: Arc<TestServer>,
}

#[async_trait(?Send)]
impl ApiBuildable for ApiV2 {
    async fn build(labrinth_config: LabrinthConfig) -> Self {        
        let app = labrinth::app_config(labrinth_config).into_make_service_with_connect_info::<SocketAddr>();
        let test_server = Arc::new(TestServer::new(app).unwrap());

        Self { test_server }
    }
}

#[async_trait(?Send)]
impl Api for ApiV2 {
    async fn reset_search_index(&self) -> TestResponse {
        self.test_server.post(&"/v2/admin/_force_reindex")
        .add_header(
            HeaderName::from_static("Modrinth-Admin"),
            HeaderValue::from_str(&dotenvy::var("LABRINTH_ADMIN_KEY").unwrap()).unwrap(),
        )
     .await
    }

    fn get_test_server(&self) -> Arc<TestServer> {
        self.test_server.clone()
    }
}
