#![allow(dead_code)]

use super::api_common::{Api, ApiBuildable};
use async_trait::async_trait;
use axum::http::{HeaderName, HeaderValue};
use axum_test::{TestResponse, TestServer};
use labrinth::LabrinthConfig;
use std::net::SocketAddr;
use std::sync::Arc;

pub mod collections;
pub mod oauth;
pub mod oauth_clients;
pub mod organization;
pub mod project;
pub mod request_data;
pub mod tags;
pub mod team;
pub mod user;
pub mod version;

#[derive(Clone)]
pub struct ApiV3 {
    pub test_server: Arc<TestServer>,
}

#[async_trait(?Send)]
impl ApiBuildable for ApiV3 {
    async fn build(labrinth_config: LabrinthConfig) -> Self {
        let app = labrinth::app_config(labrinth_config)
            .into_make_service_with_connect_info::<SocketAddr>();
        let test_server = Arc::new(TestServer::new(app).unwrap());
        Self { test_server }
    }
}

#[async_trait(?Send)]
impl Api for ApiV3 {
    async fn reset_search_index(&self) -> TestResponse {
        self.test_server
            .post(&"/_internal/admin/_force_reindex")
            .add_header(
                HeaderName::from_static("modrinth-admin"),
                HeaderValue::from_str(&dotenvy::var("LABRINTH_ADMIN_KEY").unwrap()).unwrap(),
            )
            .await
    }

    fn get_test_server(&self) -> Arc<TestServer> {
        self.test_server.clone()
    }
}
