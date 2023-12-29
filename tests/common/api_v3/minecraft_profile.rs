use actix_web::{
    dev::ServiceResponse,
    test::{self, TestRequest},
};
use bytes::Bytes;
use itertools::Itertools;
use labrinth::{models::minecraft::profile::{MinecraftProfile, MinecraftProfileShareLink}, util::actix::{MultipartSegment, MultipartSegmentData, AppendsMultipart}, routes::v3::minecraft::profiles::ProfileDownload};
use serde_json::json;

use crate::common::{api_common::{request_data::ImageData, Api, AppendsOptionalPat}, dummy_data::TestFile};

use super::ApiV3;
pub struct MinecraftProfileOverride {
    pub file_name: String,
    pub install_path: String,
    pub bytes: Vec<u8>
}

impl MinecraftProfileOverride {
    pub fn new(test_file : TestFile, install_path : &str) -> Self {
        Self {
            file_name: test_file.filename(),
            install_path: install_path.to_string(),
            bytes: test_file.bytes(),
        }
    }
}


impl ApiV3 {
    pub async fn create_minecraft_profile(
        &self,
        name: &str,
        loader: &str,
        loader_version: &str,
        game_version: &str,
        versions: Vec<&str>,
        pat: Option<&str>,
    ) -> ServiceResponse {
        let req = test::TestRequest::post()
            .uri("/v3/minecraft/profile")
            .append_pat(pat)
            .set_json(json!({
                "name": name,
                "loader": loader,
                "loader_version": loader_version,
                "game_version": game_version,
                "versions": versions
            }))
            .to_request();
        self.call(req).await
    }

    pub async fn edit_minecraft_profile(
        &self,
        id: &str,
        name: Option<&str>,
        loader: Option<&str>,
        loader_version: Option<&str>,
        versions: Option<Vec<&str>>,
        pat: Option<&str>,
    ) -> ServiceResponse {
        let req = test::TestRequest::patch()
            .uri(&format!("/v3/minecraft/profile/{}", id))
            .append_pat(pat)
            .set_json(json!({
                "name": name,
                "loader": loader,
                "loader_version": loader_version,
                "versions": versions
            }))
            .to_request();
        self.call(req).await
    }

    pub async fn get_minecraft_profile(&self, id: &str, pat: Option<&str>) -> ServiceResponse {
        let req = TestRequest::get()
            .uri(&format!("/v3/minecraft/profile/{}", id))
            .append_pat(pat)
            .to_request();
        self.call(req).await
    }

    pub async fn get_minecraft_profile_deserialized(&self, id: &str, pat: Option<&str>) -> MinecraftProfile {
        let resp = self.get_minecraft_profile(id, pat).await;
        assert_eq!(resp.status(), 200);
        test::read_body_json(resp).await
    }

    pub async fn edit_minecraft_profile_icon(
        &self,
        id: &str,
        icon: Option<ImageData>,
        pat: Option<&str>,
    ) -> ServiceResponse {
        if let Some(icon) = icon {
            let req = TestRequest::patch()
                .uri(&format!("/v3/minecraft/profile/{}/icon?ext={}", id, icon.extension))
                .append_pat(pat)
                .set_payload(Bytes::from(icon.icon))
                .to_request();
            self.call(req).await
        } else {
            let req = TestRequest::delete()
                .uri(&format!("/v3/minecraft/profile/{}/icon", id))
                .append_pat(pat)
                .to_request();
            self.call(req).await
        }
    }

    pub async fn add_minecraft_profile_overrides(
        &self,
        id: &str,
        overrides: Vec<MinecraftProfileOverride>,
        pat: Option<&str>,
    ) -> ServiceResponse {
        let mut data = Vec::new();
        let mut multipart_segments : Vec<MultipartSegment> = Vec::new();
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
        }).chain(multipart_segments.into_iter()).collect_vec();

        let req = TestRequest::post()
            .uri(&format!("/v3/minecraft/profile/{}/override", id))
            .append_pat(pat)
            .set_multipart(multipart_segments)
            .to_request();
        self.call(req).await
    }

    pub async fn delete_minecraft_profile_override(
        &self,
        id: &str,
        file_name: &str,
        pat: Option<&str>,
    ) -> ServiceResponse {
        let req = TestRequest::delete()
            .uri(&format!("/v3/minecraft/profile/{}/overrides/{}", id, file_name))
            .append_pat(pat)
            .to_request();
        self.call(req).await
    }

    pub async fn generate_minecraft_profile_share_link(
        &self,
        id: &str,
        pat: Option<&str>,
    ) -> ServiceResponse {
        let req = TestRequest::get()
            .uri(&format!("/v3/minecraft/profile/{}/share", id))
            .append_pat(pat)
            .to_request();
        self.call(req).await
    }

    pub async fn generate_minecraft_profile_share_link_deserialized(
        &self,
        id: &str,
        pat: Option<&str>,
    ) -> MinecraftProfileShareLink {
        let resp = self.generate_minecraft_profile_share_link(id, pat).await;
        assert_eq!(resp.status(), 200);
        test::read_body_json(resp).await
    }

    pub async fn get_minecraft_profile_share_link(
        &self,
        profile_id: &str,
        url_identifier: &str,
        pat: Option<&str>
    ) -> ServiceResponse {
        let req = TestRequest::get()
            .uri(&format!("/v3/minecraft/profile/{}/share/{}", profile_id, url_identifier))
            .append_pat(pat)
            .to_request();
        self.call(req).await
    }

    pub async fn get_minecraft_profile_share_link_deserialized(
        &self,
        profile_id: &str,
        url_identifier: &str,
        pat: Option<&str>
    ) -> MinecraftProfileShareLink {
        let resp = self.get_minecraft_profile_share_link(profile_id, url_identifier, pat).await;
        assert_eq!(resp.status(), 200);
        test::read_body_json(resp).await
    }    

    pub async fn download_minecraft_profile(
        &self,
        url_identifier: &str,
        pat: Option<&str>,
    ) -> ServiceResponse {
        let req = TestRequest::get()
            .uri(&format!("/v3/minecraft/profile/{}/download", url_identifier))
            .append_pat(pat)
            .to_request();
        self.call(req).await
    }

     pub async fn download_minecraft_profile_deserialized(
        &self,
        url_identifier: &str,
        pat: Option<&str>,
     )-> ProfileDownload {
        let resp = self.download_minecraft_profile(url_identifier, pat).await;
        assert_eq!(resp.status(), 200);
        test::read_body_json(resp).await
     }

    pub async fn check_download_minecraft_profile_token(
        &self,
        token: &str,
        url: &str, // Full URL, the route will parse it
    ) -> ServiceResponse {
        let req = TestRequest::get()
            .uri(&format!("/v3/minecraft/check_token?url={url}", url=urlencoding::encode(url)))
            .append_header(("Authorization", token))
            .to_request();
        self.call(req).await
    }


}

