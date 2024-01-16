use axum::extract::{ConnectInfo, Query};
use axum::http::HeaderMap;
use axum::routing::post;
use axum::{Extension, Json, Router};
use std::net::SocketAddr;
use std::sync::Arc;

use crate::auth::checks::{is_team_member_project, is_team_member_version};
use crate::auth::get_user_from_headers;
use crate::database;
use crate::database::models::{project_item, report_item, thread_item, version_item};
use crate::database::redis::RedisPool;
use crate::file_hosting::FileHost;
use crate::models::ids::{ThreadMessageId, VersionId};
use crate::models::images::{Image, ImageContext};
use crate::models::reports::ReportId;
use crate::queue::session::AuthQueue;
use crate::routes::ApiError;
use crate::util::routes::read_from_payload;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use super::threads::is_authorized_thread;

pub fn config() -> Router {
    Router::new().route("image", post(images_add))
}

#[derive(Serialize, Deserialize)]
pub struct ImageUpload {
    pub ext: String,

    // Context must be an allowed context
    // currently: project, version, thread_message, report
    pub context: String,

    // Optional context id to associate with
    pub project_id: Option<String>, // allow slug or id
    pub version_id: Option<VersionId>,
    pub thread_message_id: Option<ThreadMessageId>,
    pub report_id: Option<ReportId>,
}

pub async fn images_add(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(data): Query<ImageUpload>,
    Extension(file_host): Extension<Arc<dyn FileHost + Send + Sync>>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
    payload: bytes::Bytes,
) -> Result<Json<Image>, ApiError> {
    if let Some(content_type) = crate::util::ext::get_image_content_type(&data.ext) {
        let mut context = ImageContext::from_str(&data.context, None);

        let scopes = vec![context.relevant_scope()];

        let cdn_url = dotenvy::var("CDN_URL")?;
        let user = get_user_from_headers(
            &addr,
            &headers,
            &pool,
            &redis,
            &session_queue,
            Some(&scopes),
        )
        .await?
        .1;

        // Attempt to associated a supplied id with the context
        // If the context cannot be found, or the user is not authorized to upload images for the context, return an error
        match &mut context {
            ImageContext::Project { project_id } => {
                if let Some(id) = data.project_id {
                    let project = project_item::Project::get(&id, &pool, &redis).await?;
                    if let Some(project) = project {
                        if is_team_member_project(&project.inner, &Some(user.clone()), &pool)
                            .await?
                        {
                            *project_id = Some(project.inner.id.into());
                        } else {
                            return Err(ApiError::CustomAuthentication(
                                "You are not authorized to upload images for this project"
                                    .to_string(),
                            ));
                        }
                    } else {
                        return Err(ApiError::InvalidInput(
                            "The project could not be found.".to_string(),
                        ));
                    }
                }
            }
            ImageContext::Version { version_id } => {
                if let Some(id) = data.version_id {
                    let version = version_item::Version::get(id.into(), &pool, &redis).await?;
                    if let Some(version) = version {
                        if is_team_member_version(
                            &version.inner,
                            &Some(user.clone()),
                            &pool,
                            &redis,
                        )
                        .await?
                        {
                            *version_id = Some(version.inner.id.into());
                        } else {
                            return Err(ApiError::CustomAuthentication(
                                "You are not authorized to upload images for this version"
                                    .to_string(),
                            ));
                        }
                    } else {
                        return Err(ApiError::InvalidInput(
                            "The version could not be found.".to_string(),
                        ));
                    }
                }
            }
            ImageContext::ThreadMessage { thread_message_id } => {
                if let Some(id) = data.thread_message_id {
                    let thread_message = thread_item::ThreadMessage::get(id.into(), &pool)
                        .await?
                        .ok_or_else(|| {
                        ApiError::InvalidInput("The thread message could not found.".to_string())
                    })?;
                    let thread = thread_item::Thread::get(thread_message.thread_id, &pool)
                        .await?
                        .ok_or_else(|| {
                            ApiError::InvalidInput(
                                "The thread associated with the thread message could not be found"
                                    .to_string(),
                            )
                        })?;
                    if is_authorized_thread(&thread, &user, &pool).await? {
                        *thread_message_id = Some(thread_message.id.into());
                    } else {
                        return Err(ApiError::CustomAuthentication(
                            "You are not authorized to upload images for this thread message"
                                .to_string(),
                        ));
                    }
                }
            }
            ImageContext::Report { report_id } => {
                if let Some(id) = data.report_id {
                    let report = report_item::Report::get(id.into(), &pool)
                        .await?
                        .ok_or_else(|| {
                            ApiError::InvalidInput("The report could not be found.".to_string())
                        })?;
                    let thread = thread_item::Thread::get(report.thread_id, &pool)
                        .await?
                        .ok_or_else(|| {
                            ApiError::InvalidInput(
                                "The thread associated with the report could not be found."
                                    .to_string(),
                            )
                        })?;
                    if is_authorized_thread(&thread, &user, &pool).await? {
                        *report_id = Some(report.id.into());
                    } else {
                        return Err(ApiError::CustomAuthentication(
                            "You are not authorized to upload images for this report".to_string(),
                        ));
                    }
                }
            }
            ImageContext::Unknown => {
                return Err(ApiError::InvalidInput(
                    "Context must be one of: project, version, thread_message, report".to_string(),
                ));
            }
        }

        // Upload the image to the file host
        let bytes =
            read_from_payload(payload, 1_048_576, "Icons must be smaller than 1MiB").await?;

        let hash = sha1::Sha1::from(&bytes).hexdigest();
        let upload_data = file_host
            .upload_file(
                content_type,
                &format!("data/cached_images/{}.{}", hash, data.ext),
                bytes,
            )
            .await?;

        let mut transaction = pool.begin().await?;

        let db_image: database::models::Image = database::models::Image {
            id: database::models::generate_image_id(&mut transaction).await?,
            url: format!("{}/{}", cdn_url, upload_data.file_name),
            size: upload_data.content_length as u64,
            created: chrono::Utc::now(),
            owner_id: database::models::UserId::from(user.id),
            context: context.context_as_str().to_string(),
            project_id: if let ImageContext::Project {
                project_id: Some(id),
            } = context
            {
                Some(database::models::ProjectId::from(id))
            } else {
                None
            },
            version_id: if let ImageContext::Version {
                version_id: Some(id),
            } = context
            {
                Some(database::models::VersionId::from(id))
            } else {
                None
            },
            thread_message_id: if let ImageContext::ThreadMessage {
                thread_message_id: Some(id),
            } = context
            {
                Some(database::models::ThreadMessageId::from(id))
            } else {
                None
            },
            report_id: if let ImageContext::Report {
                report_id: Some(id),
            } = context
            {
                Some(database::models::ReportId::from(id))
            } else {
                None
            },
        };

        // Insert
        db_image.insert(&mut transaction).await?;

        let image = Image {
            id: db_image.id.into(),
            url: db_image.url,
            size: db_image.size,
            created: db_image.created,
            owner_id: db_image.owner_id.into(),
            context,
        };

        transaction.commit().await?;

        Ok(Json(image))
    } else {
        Err(ApiError::InvalidInput(
            "The specified file is not an image!".to_string(),
        ))
    }
}
