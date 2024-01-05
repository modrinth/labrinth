use std::path::PathBuf;

use actix_http::StatusCode;
use actix_web::{
    dev::ServiceResponse,
    test::{self, TestRequest},
};
use bytes::Bytes;
use itertools::Itertools;
use labrinth::{
    models::client::profile::{ClientProfile, ClientProfileShareLink},
    routes::v3::client::profiles::ProfileDownload,
    util::actix::{AppendsMultipart, MultipartSegment, MultipartSegmentData},
};
use serde_json::json;

use crate::common::{
    api_common::{request_data::ImageData, Api, AppendsOptionalPat},
    asserts::assert_status,
    dummy_data::TestFile,
};

use super::ApiV3;
pub struct ClientProfileOverride {
    pub file_name: String,
    pub install_path: String,
    pub bytes: Vec<u8>,
}

impl ClientProfileOverride {
    pub fn new(test_file: TestFile, install_path: &str) -> Self {
        Self {
            file_name: test_file.filename(),
            install_path: install_path.to_string(),
            bytes: test_file.bytes(),
        }
    }
}

impl ApiV3 {
    pub async fn create_client_profile(
        &self,
        name: &str,
        loader: &str,
        loader_version: &str,
        game_version: &str,
        versions: Vec<&str>,
        pat: Option<&str>,
    ) -> ServiceResponse {
        let req = test::TestRequest::post()
            .uri("/v3/client/profile")
            .append_pat(pat)
            .set_json(json!({
                "name": name,
                "loader": loader,
                "loader_version": loader_version,
                "game": "minecraft-java",
                "game_version": game_version,
                "versions": versions
            }))
            .to_request();
        self.call(req).await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn edit_client_profile(
        &self,
        id: &str,
        name: Option<&str>,
        loader: Option<&str>,
        loader_version: Option<&str>,
        versions: Option<Vec<&str>>,
        remove_users: Option<Vec<&str>>,
        pat: Option<&str>,
    ) -> ServiceResponse {
        let req = test::TestRequest::patch()
            .uri(&format!("/v3/client/profile/{}", id))
            .append_pat(pat)
            .set_json(json!({
                "name": name,
                "loader": loader,
                "loader_version": loader_version,
                "versions": versions,
                "remove_users": remove_users
            }))
            .to_request();
        self.call(req).await
    }

    pub async fn get_client_profile(&self, id: &str, pat: Option<&str>) -> ServiceResponse {
        let req = TestRequest::get()
            .uri(&format!("/v3/client/profile/{}", id))
            .append_pat(pat)
            .to_request();
        self.call(req).await
    }

    pub async fn get_client_profile_deserialized(
        &self,
        id: &str,
        pat: Option<&str>,
    ) -> ClientProfile {
        let resp = self.get_client_profile(id, pat).await;
        assert_status(&resp, StatusCode::OK);
        test::read_body_json(resp).await
    }

    pub async fn delete_client_profile(&self, id: &str, pat: Option<&str>) -> ServiceResponse {
        let req = TestRequest::delete()
            .uri(&format!("/v3/client/profile/{}", id))
            .append_pat(pat)
            .to_request();
        self.call(req).await
    }

    pub async fn edit_client_profile_icon(
        &self,
        id: &str,
        icon: Option<ImageData>,
        pat: Option<&str>,
    ) -> ServiceResponse {
        if let Some(icon) = icon {
            let req = TestRequest::patch()
                .uri(&format!(
                    "/v3/client/profile/{}/icon?ext={}",
                    id, icon.extension
                ))
                .append_pat(pat)
                .set_payload(Bytes::from(icon.icon))
                .to_request();
            self.call(req).await
        } else {
            let req = TestRequest::delete()
                .uri(&format!("/v3/client/profile/{}/icon", id))
                .append_pat(pat)
                .to_request();
            self.call(req).await
        }
    }

    pub async fn add_client_profile_overrides(
        &self,
        id: &str,
        overrides: Vec<ClientProfileOverride>,
        pat: Option<&str>,
    ) -> ServiceResponse {
        let mut data = Vec::new();
        let mut multipart_segments: Vec<MultipartSegment> = Vec::new();
        for override_ in overrides {
            data.push(serde_json::json!({
                "file_name": override_.file_name,
                "install_path": override_.install_path,
            }));
            multipart_segments.push(MultipartSegment {
                name: override_.file_name.clone(),
                filename: Some(override_.file_name),
                content_type: None,
                data: MultipartSegmentData::Binary(override_.bytes.to_vec()),
            });
        }
        let multipart_segments = std::iter::once(MultipartSegment {
            name: "data".to_string(),
            filename: None,
            content_type: Some("application/json".to_string()),
            data: MultipartSegmentData::Text(serde_json::to_string(&data).unwrap()),
        })
        .chain(multipart_segments.into_iter())
        .collect_vec();

        let req = TestRequest::post()
            .uri(&format!("/v3/client/profile/{}/override", id))
            .append_pat(pat)
            .set_multipart(multipart_segments)
            .to_request();
        self.call(req).await
    }

    pub async fn delete_client_profile_overrides(
        &self,
        id: &str,
        install_paths: Option<&[&PathBuf]>,
        hashes: Option<&[&str]>,
        pat: Option<&str>,
    ) -> ServiceResponse {
        let req = TestRequest::delete()
            .uri(&format!("/v3/client/profile/{}/override", id))
            .set_json(json!({
                "install_paths": install_paths,
                "hashes": hashes
            }))
            .append_pat(pat)
            .to_request();
        self.call(req).await
    }

    pub async fn generate_client_profile_share_link(
        &self,
        id: &str,
        pat: Option<&str>,
    ) -> ServiceResponse {
        let req = TestRequest::get()
            .uri(&format!("/v3/client/profile/{}/share", id))
            .append_pat(pat)
            .to_request();
        self.call(req).await
    }

    pub async fn generate_client_profile_share_link_deserialized(
        &self,
        id: &str,
        pat: Option<&str>,
    ) -> ClientProfileShareLink {
        let resp = self.generate_client_profile_share_link(id, pat).await;
        assert_status(&resp, StatusCode::OK);
        test::read_body_json(resp).await
    }

    pub async fn get_client_profile_share_link(
        &self,
        profile_id: &str,
        url_identifier: &str,
        pat: Option<&str>,
    ) -> ServiceResponse {
        let req = TestRequest::get()
            .uri(&format!(
                "/v3/client/profile/{}/share/{}",
                profile_id, url_identifier
            ))
            .append_pat(pat)
            .to_request();
        self.call(req).await
    }

    pub async fn get_client_profile_share_link_deserialized(
        &self,
        profile_id: &str,
        url_identifier: &str,
        pat: Option<&str>,
    ) -> ClientProfileShareLink {
        let resp = self
            .get_client_profile_share_link(profile_id, url_identifier, pat)
            .await;
        assert_status(&resp, StatusCode::OK);
        test::read_body_json(resp).await
    }

    pub async fn accept_client_profile_share_link(
        &self,
        profile_id: &str,
        url_identifier: &str,
        pat: Option<&str>,
    ) -> ServiceResponse {
        let req = TestRequest::post()
            .uri(&format!(
                "/v3/client/profile/{}/accept/{}",
                profile_id, url_identifier
            ))
            .append_pat(pat)
            .to_request();
        self.call(req).await
    }

    // Get links and token
    pub async fn download_client_profile(
        &self,
        profile_id: &str,
        pat: Option<&str>,
    ) -> ServiceResponse {
        let req = TestRequest::get()
            .uri(&format!("/v3/client/profile/{}/download", profile_id))
            .append_pat(pat)
            .to_request();
        self.call(req).await
    }

    pub async fn download_client_profile_deserialized(
        &self,
        profile_id: &str,
        pat: Option<&str>,
    ) -> ProfileDownload {
        let resp = self.download_client_profile(profile_id, pat).await;
        assert_status(&resp, StatusCode::OK);
        test::read_body_json(resp).await
    }

    pub async fn check_download_client_profile_token(
        &self,
        url: &str, // Full URL, the route will parse it
        pat: Option<&str>,
    ) -> ServiceResponse {
        let req = TestRequest::get()
            .uri(&format!(
                "/v3/client/check_token?url={url}",
                url = urlencoding::encode(url)
            ))
            .append_pat(pat)
            .to_request();
        self.call(req).await
    }
}
