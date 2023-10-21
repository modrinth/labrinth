use super::version_creation::InitialVersionData;
use crate::auth::{get_user_from_headers, AuthenticationError};
use crate::database::models::loader_fields::Game;
use crate::database::models::thread_item::ThreadBuilder;
use crate::database::models::{self, image_item, User, version_item};
use crate::database::redis::RedisPool;
use crate::file_hosting::{FileHost, FileHostingError};
use crate::models::error::ApiError;
use crate::models::ids::ImageId;
use crate::models::images::{Image, ImageContext};
use crate::models::pats::Scopes;
use crate::models::projects::{
    DonationLink, License, MonetizationStatus, ProjectId, ProjectStatus, SideType, VersionId,
    VersionStatus,
};
use actix_web::http::header::HeaderValue;


use crate::models::teams::ProjectPermissions;
use crate::models::threads::ThreadType;
use crate::models::users::UserId;
use crate::queue::session::AuthQueue;
use crate::routes::{v3, v2_reroute};
use crate::routes::v3::project_creation::CreateError;
use crate::search::indexing::IndexingError;
use crate::util::routes::read_from_field;
use crate::util::validate::validation_errors_to_string;
use actix_multipart::{Field, Multipart};
use actix_web::http::StatusCode;
use actix_web::web::Data;
use actix_web::{post, HttpRequest, HttpResponse};
use bytes::Bytes;
use chrono::Utc;
use futures::TryStreamExt;
use futures::stream::StreamExt;
use image::ImageError;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPool;
use std::sync::Arc;
use thiserror::Error;
use validator::Validate;
use serde_json::json;

pub fn config(cfg: &mut actix_web::web::ServiceConfig) {
    cfg.service(project_create);
}

#[post("project")]
pub async fn project_create(
    req: HttpRequest,
    payload: Multipart,
    client: Data<PgPool>,
    redis: Data<RedisPool>,
    file_host: Data<Arc<dyn FileHost + Send + Sync>>,
    session_queue: Data<AuthQueue>,
) -> Result<HttpResponse, CreateError> {

    // Convert V2 multipart payload to V3 multipart payload
    let mut saved_slug = None;
    let payload = v2_reroute::alter_actix_multipart(payload, req.headers().clone(), |json| {
        // Convert input data to V3 format
        println!("ABOUT TO ALTER ACTIX MULTIPART {}", json.to_string());
        // Save slug for out of closure
        saved_slug = Some(json["slug"].as_str().unwrap_or("").to_string());

        // Set game name (all v2 projects are minecraft-java)
        json["game_name"] = json!("minecraft-java");

        // Loader fields are now a struct, containing all versionfields
        // loaders: ["fabric"]
        // game_versions: ["1.16.5", "1.17"]
        // -> becomes ->
        // loaders: [{"loader": "fabric", "game_versions": ["1.16.5", "1.17"]}]

        // Side types will be applied to each version
        let client_side = json["client_side"].as_str().unwrap_or("required").to_string();
        let server_side = json["server_side"].as_str().unwrap_or("required").to_string();
        json["client_side"] = json!(null);
        json["server_side"] = json!(null);

        if let Some(versions) = json["initial_versions"].as_array_mut() {
            for version in versions {
                // Construct loader object with version fields
                // V2 fields becoming loader fields are:
                // - client_side
                // - server_side
                // - game_versions
                let mut loaders = vec![];
                for loader in version["loaders"].as_array().unwrap_or(&Vec::new()) {
                    let loader = loader.as_str().unwrap_or("");
                    loaders.push(json!({
                        "loader": loader,
                        "game_versions": version["game_versions"].as_array(),
                        "client_side": client_side,
                        "server_side": server_side,
                    }));
                }
                version["loaders"] = json!(loaders);
            }
        }
        println!("JUST ALTER ACTIX MULTIPART {}", json.to_string());
        println!("Done;");

    }).await;

    // Call V3 project creation
    let response= v3::project_creation::project_create(req, payload, client.clone(), redis.clone(), file_host, session_queue).await?;

    // Convert response to V2 forma
    match v2_reroute::extract_ok_json(response).await {
        Ok(mut json) => {
        v2_reroute::set_side_types_from_versions(&mut json, &**client, &redis).await?;
        Ok(HttpResponse::Ok().json(json))
    },
        Err(response) =>    Ok(response)
    }

    // TODO: Convert response to V2 format
}

