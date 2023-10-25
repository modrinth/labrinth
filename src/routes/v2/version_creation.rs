use crate::database::redis::RedisPool;
use crate::file_hosting::FileHost;
use crate::models::ids::ImageId;
use crate::models::projects::{
    Dependency, FileType, Loader, ProjectId, Version, VersionId, VersionStatus, VersionType,
};
use crate::models::v2::projects::LegacyVersion;
use crate::queue::session::AuthQueue;
use crate::routes::v3::project_creation::CreateError;
use crate::routes::{v2_reroute, v3};
use actix_multipart::Multipart;
use actix_web::web::Data;
use actix_web::{post, web, HttpRequest, HttpResponse};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::postgres::PgPool;
use std::collections::HashMap;
use std::sync::Arc;
use validator::Validate;

fn default_requested_status() -> VersionStatus {
    VersionStatus::Listed
}

#[derive(Serialize, Deserialize, Validate, Clone)]
pub struct InitialVersionData {
    #[serde(alias = "mod_id")]
    pub project_id: Option<ProjectId>,
    #[validate(length(min = 1, max = 256))]
    pub file_parts: Vec<String>,
    #[validate(
        length(min = 1, max = 32),
        regex = "crate::util::validate::RE_URL_SAFE"
    )]
    pub version_number: String,
    #[validate(
        length(min = 1, max = 64),
        custom(function = "crate::util::validate::validate_name")
    )]
    #[serde(alias = "name")]
    pub version_title: String,
    #[validate(length(max = 65536))]
    #[serde(alias = "changelog")]
    pub version_body: Option<String>,
    #[validate(
        length(min = 0, max = 4096),
        custom(function = "crate::util::validate::validate_deps")
    )]
    pub dependencies: Vec<Dependency>,
    #[validate(length(min = 1))]
    pub game_versions: Vec<String>,
    #[serde(alias = "version_type")]
    pub release_channel: VersionType,
    #[validate(length(min = 1))]
    pub loaders: Vec<Loader>,
    pub featured: bool,
    pub primary_file: Option<String>,
    #[serde(default = "default_requested_status")]
    pub status: VersionStatus,
    #[serde(default = "HashMap::new")]
    pub file_types: HashMap<String, Option<FileType>>,
    // Associations to uploaded images in changelog
    #[validate(length(max = 10))]
    #[serde(default)]
    pub uploaded_images: Vec<ImageId>,
}

#[derive(Serialize, Deserialize, Clone)]
struct InitialFileData {
    #[serde(default = "HashMap::new")]
    pub file_types: HashMap<String, Option<FileType>>,
}

// under `/api/v1/version`
#[post("version")]
pub async fn version_create(
    req: HttpRequest,
    payload: Multipart,
    client: Data<PgPool>,
    redis: Data<RedisPool>,
    file_host: Data<Arc<dyn FileHost + Send + Sync>>,
    session_queue: Data<AuthQueue>,
) -> Result<HttpResponse, CreateError> {
    let payload = v2_reroute::alter_actix_multipart(payload, req.headers().clone(), |json| {
        // Convert input data to V3 format

        // Loader fields are now a struct, containing all versionfields
        // loaders: ["fabric"]
        // game_versions: ["1.16.5", "1.17"]
        // -> becomes ->
        // loaders: [{"loader": "fabric", "game_versions": ["1.16.5", "1.17"]}]
        let mut loaders = vec![];
        for loader in json["loaders"].as_array().unwrap_or(&Vec::new()) {
            let loader = loader.as_str().unwrap_or("");
            loaders.push(json!({
                "loader": loader,
                "game_versions": json["game_versions"].as_array(),
            }));
        }
        json["loaders"] = json!(loaders);
    })
    .await?;

    // Call V3 project creation
    let response = v3::version_creation::version_create(
        req,
        payload,
        client.clone(),
        redis.clone(),
        file_host,
        session_queue,
    )
    .await?;

    // Convert response to V2 format
    match v2_reroute::extract_ok_json::<Version>(response).await {
        Ok(version) => {
            let v2_version = LegacyVersion::from(version);
            Ok(HttpResponse::Ok().json(v2_version))
        }
        Err(response) => Ok(response),
    }
}

// under /api/v1/version/{version_id}
#[post("{version_id}/file")]
pub async fn upload_file_to_version(
    req: HttpRequest,
    url_data: web::Path<(VersionId,)>,
    payload: Multipart,
    client: Data<PgPool>,
    redis: Data<RedisPool>,
    file_host: Data<Arc<dyn FileHost + Send + Sync>>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, CreateError> {
    let response = v3::version_creation::upload_file_to_version(
        req,
        url_data,
        payload,
        client.clone(),
        redis.clone(),
        file_host,
        session_queue,
    )
    .await?;
    Ok(response)
}
