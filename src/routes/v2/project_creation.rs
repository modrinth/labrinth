use crate::database::models::version_item;
use crate::database::redis::RedisPool;
use crate::file_hosting::FileHost;
use crate::models::projects::Project;
use crate::models::v2::projects::LegacyProject;
use crate::queue::session::AuthQueue;
use crate::routes::v3::project_creation::CreateError;
use crate::routes::{v2_reroute, v3};
use actix_multipart::Multipart;
use actix_web::web::Data;
use actix_web::{post, HttpRequest, HttpResponse};
use serde_json::json;
use sqlx::postgres::PgPool;
use std::sync::Arc;

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
        let client_side = json["client_side"]
            .as_str()
            .unwrap_or("required")
            .to_string();
        let server_side = json["server_side"]
            .as_str()
            .unwrap_or("required")
            .to_string();
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
    })
    .await?;

    // Call V3 project creation
    let response = v3::project_creation::project_create(
        req,
        payload,
        client.clone(),
        redis.clone(),
        file_host,
        session_queue,
    )
    .await?;

    // Convert response to V2 format
    match v2_reroute::extract_ok_json::<Project>(response).await {
        Ok(project) => {
            let version_item = match project.versions.first() {
                Some(vid) => version_item::Version::get((*vid).into(), &**client, &redis).await?,
                None => None,
            };
            let project = LegacyProject::from(project, version_item);
            Ok(HttpResponse::Ok().json(project))
        }
        Err(response) => Ok(response),
    }
}
