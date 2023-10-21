use crate::auth::get_user_from_headers;
use crate::database::models::notification_item::NotificationBuilder;
use crate::database::models::version_item::{
    DependencyBuilder, VersionBuilder, VersionFileBuilder,
};
use crate::database::models::{self, image_item, Organization};
use crate::database::redis::RedisPool;
use crate::file_hosting::FileHost;
use crate::models::images::{Image, ImageContext, ImageId};
use crate::models::notifications::NotificationBody;
use crate::models::pack::PackFileHash;
use crate::models::pats::Scopes;
use crate::models::projects::{
    Dependency, DependencyType, FileType, GameVersion, Loader, ProjectId, Version, VersionFile,
    VersionId, VersionStatus, VersionType,
};
use crate::models::teams::ProjectPermissions;
use crate::queue::session::AuthQueue;
use crate::routes::{v2_reroute, v3};
use crate::routes::v3::project_creation::CreateError;
use crate::util::routes::read_from_field;
use crate::util::validate::validation_errors_to_string;
use crate::validate::{validate_file, ValidationResult};
use actix_multipart::{Field, Multipart};
use actix_web::web::Data;
use actix_web::{post, web, HttpRequest, HttpResponse};
use chrono::Utc;
use futures::stream::StreamExt;
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPool;
use std::collections::HashMap;
use serde_json::json;
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
    pub game_versions: Vec<GameVersion>,
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
    mut payload: Multipart,
    client: Data<PgPool>,
    redis: Data<RedisPool>,
    file_host: Data<Arc<dyn FileHost + Send + Sync>>,
    session_queue: Data<AuthQueue>,
) -> Result<HttpResponse, CreateError> {
    // TODO: should call this from the v3
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

    
    }).await;

    // Call V3 project creation
    let response= v3::version_creation::version_create(req, payload, client.clone(), redis.clone(), file_host, session_queue).await?;

    // Convert response to V2 forma
    match v2_reroute::extract_ok_json(response).await {
        Ok(mut json) => {
            // Get game_versions out of loaders, and flatten loadedrs
            let mut game_versions = Vec::new();
            let mut loaders = Vec::new();
            if let Some(loaders_json) = json["loaders"].as_array() {
                for loader_json in loaders_json {
                    if let Some(loader) = loader_json["loader"].as_str() {
                        loaders.push(loader.to_string());
                    }
                    if let Some(game_versions_json) = loader_json["game_versions"].as_array() {
                        for game_version_json in game_versions_json {
                            if let Some(game_version) = game_version_json.as_str() {
                                game_versions.push(game_version.to_string());
                            }
                        }
                    }
                }
            }
            json["game_versions"] = json!(game_versions);
            json["loaders"] = json!(loaders);
            
            println!("Completed version creation: {:?}", json);
            Ok(HttpResponse::Ok().json(json))
    },
        Err(response) =>    Ok(response)
    }
}

// under /api/v1/version/{version_id}
#[post("{version_id}/file")]
pub async fn upload_file_to_version(
    req: HttpRequest,
    url_data: web::Path<(VersionId,)>,
    mut payload: Multipart,
    client: Data<PgPool>,
    redis: Data<RedisPool>,
    file_host: Data<Arc<dyn FileHost + Send + Sync>>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, CreateError> {
    // TODO: do we need to modify this?

    let response= v3::version_creation::upload_file_to_version(req, url_data, payload, client.clone(), redis.clone(), file_host, session_queue).await?;


    Ok(response)
}