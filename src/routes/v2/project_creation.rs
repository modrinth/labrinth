use super::version_creation::InitialVersionData;
use crate::auth::{get_user_from_headers, AuthenticationError};
use crate::database::models::loader_fields::Game;
use crate::database::models::thread_item::ThreadBuilder;
use crate::database::models::{self, image_item, User};
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
) -> Result<HttpResponse, CreateError> {

    // Redirects to V3 route
    let self_addr = dotenvy::var("SELF_ADDR")?;
    let url = format!("{self_addr}/v3/project");
    let response = v2_reroute::reroute_multipart(&url, req, payload, |json | {
        // Convert input data to V3 format
        json["game_name"] = json!("minecraft_java");
    }).await?;    
    let response = HttpResponse::build(response.status())
        .content_type(response.headers().get("content-type").and_then(|h| h.to_str().ok()).unwrap_or_default())
        .body(response.bytes().await.unwrap_or_default());

    // TODO: Convert response to V2 format
    Ok(response)
}

