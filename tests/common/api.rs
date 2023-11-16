use std::{rc::Rc, sync::Arc};

use actix_web::{dev::ServiceResponse, test::{TestRequest, self}};
use async_trait::async_trait;
use chrono::{Utc, DateTime};
use futures::Future;
use labrinth::models::{v2::projects::LegacyProject, projects::{ProjectId, ProjectStatus, ModeratorMessage, License, VersionId, GalleryItem, DonationLink, MonetizationStatus}, teams::TeamId, organizations::OrganizationId, threads::ThreadId};
use serde::Deserialize;

use super::{environment::{LocalService, TestEnvironment}, api_v2::ApiV2, api_v3::ApiV3, dummy_data::{DummyData, self}};

#[async_trait(?Send)]
impl Api for ApiV3 {
    async fn get_project(&self, id_or_slug: &str, pat: &str) -> ServiceResponse {
        let req = TestRequest::get()
            .uri(&format!("/v3/project/{id_or_slug}"))
            .append_header(("Authorization", pat))
            .to_request();
        self.call(req).await
    }

    async fn get_project_deserialized_common(&self, id_or_slug: &str, pat: &str) -> CommonProject {
        let resp = self.get_project(id_or_slug, pat).await;
        assert_eq!(resp.status(), 200);
        test::read_body_json(resp).await
    }
}