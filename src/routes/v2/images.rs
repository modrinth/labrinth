use std::sync::Arc;

use crate::auth::get_user_from_headers;
use crate::database;
use crate::database::models::categories::ImageContextType;
use crate::database::models::{ids, project_item, report_item, thread_item, version_item};
use crate::file_hosting::FileHost;
use crate::models::ids::base62_impl::parse_base62;
use crate::models::ids::{ImageId, ProjectId, ThreadMessageId, VersionId};
use crate::models::images::Image;
use crate::models::reports::ReportId;
use crate::queue::session::AuthQueue;
use crate::routes::ApiError;
use crate::util::routes::read_from_payload;
use actix_web::{patch, post, web, HttpRequest, HttpResponse};
use ids::ImageContextTypeId;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(images_add);
    cfg.service(web::scope("image").service(image_edit));
}

#[derive(Serialize, Deserialize)]
pub struct ImageUpload {
    pub ext: String,

    // Context must be an allowed context
    // currently: project, version, thread_message, report
    pub context: String,
}

#[post("image")]
pub async fn images_add(
    req: HttpRequest,
    web::Query(data): web::Query<ImageUpload>,
    file_host: web::Data<Arc<dyn FileHost + Send + Sync>>,
    mut payload: web::Payload,
    pool: web::Data<PgPool>,
    redis: web::Data<deadpool_redis::Pool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    if let Some(content_type) = crate::util::ext::get_image_content_type(&data.ext) {
        let context = ImageContextTypeId::get_id(&data.context, &**pool).await?;
        let context = match context {
            Some(x) => x,
            None => {
                return Err(ApiError::InvalidInput(
                    "Context must be one of: project, version, thread_message, report".to_string(),
                ))
            }
        };

        let relevant_scope = ImageContextType::relevant_scope(&data.context).ok_or_else(|| {
            ApiError::InvalidInput(format!("Invalid image context: {}", &data.context))
        })?;
        let scopes = vec![relevant_scope];

        let cdn_url = dotenvy::var("CDN_URL")?;
        let user = get_user_from_headers(&req, &**pool, &redis, &session_queue, Some(&scopes))
            .await?
            .1;

        let bytes =
            read_from_payload(&mut payload, 1_048_576, "Icons must be smaller than 1MiB").await?;

        let hash = sha1::Sha1::from(&bytes).hexdigest();
        let upload_data = file_host
            .upload_file(
                content_type,
                &format!("data/cached_images/{}.{}", hash, data.ext),
                bytes.freeze(),
            )
            .await?;

        let mut transaction = pool.begin().await?;

        let db_image: database::models::Image = database::models::Image {
            id: database::models::generate_image_id(&mut transaction).await?,
            url: format!("{}/{}", cdn_url, upload_data.file_name),
            size: upload_data.content_length as u64,
            created: chrono::Utc::now(),
            owner_id: database::models::UserId::from(user.id),
            context_type_id: context,
            context_id: None,
        };

        // Insert
        db_image.insert(&mut transaction).await?;

        let image = Image {
            id: db_image.id.into(),
            url: db_image.url,
            size: db_image.size,
            created: db_image.created,
            owner_id: db_image.owner_id.into(),

            project_id: if data.context == "project" {
                db_image.context_id.map(ProjectId)
            } else {
                None
            },
            version_id: if data.context == "version" {
                db_image.context_id.map(VersionId)
            } else {
                None
            },
            thread_message_id: if data.context == "thread_message" {
                db_image.context_id.map(ThreadMessageId)
            } else {
                None
            },
            report_id: if data.context == "report" {
                db_image.context_id.map(ReportId)
            } else {
                None
            },
        };

        transaction.commit().await?;

        Ok(HttpResponse::Ok().json(image))
    } else {
        Err(ApiError::InvalidInput(
            "The specified file is not an image!".to_string(),
        ))
    }
}

// Associate an image with a context
// One of project_id, version_id, thread_message_id, or report_id must be specified
#[derive(Deserialize, Serialize)]
pub struct ImageEdit {
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    pub thread_message_id: Option<String>,
    pub report_id: Option<String>,
}

/// Associates an image with a project or thread message
/// If an image has no contexts associated with it, it may be deleted
#[patch("{id}")]
pub async fn image_edit(
    req: HttpRequest,
    info: web::Path<(String,)>,
    web::Query(edit): web::Query<ImageEdit>,
    pool: web::Data<PgPool>,
    redis: web::Data<deadpool_redis::Pool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let string: String = info.into_inner().0;
    let image_id = ImageId(parse_base62(&string)?);
    let image_data = database::models::Image::get(image_id.into(), &**pool, &redis).await?;

    if let Some(data) = image_data {
        let mut transaction = pool.begin().await?;

        // Get scopes needed depending on context
        let relevant_scope =
            ImageContextType::relevant_scope(&data.context_type_name).ok_or_else(|| {
                ApiError::InvalidInput(format!(
                    "Invalid image context: {}",
                    &data.context_type_name
                ))
            })?;

        let scopes = vec![relevant_scope];
        let user = get_user_from_headers(&req, &**pool, &redis, &session_queue, Some(&scopes))
            .await?
            .1;

        if user.id == data.owner_id.into() {
            let mut checked_id: Option<i64> = None;
            if let Some(project_id) = edit.project_id {
                checked_id = project_item::Project::get(&project_id, &mut transaction, &redis)
                    .await?
                    .map(|x| x.inner.id.0);
            }

            if let Some(version_id) = edit.version_id {
                let new_id = serde_json::from_str::<ids::VersionId>(&version_id)?;
                checked_id = version_item::Version::get(new_id, &mut transaction, &redis)
                    .await?
                    .map(|x| x.inner.id.0);
            }

            if let Some(thread_message_id) = edit.thread_message_id {
                let new_id = serde_json::from_str::<ids::ThreadMessageId>(&thread_message_id)?;
                checked_id = thread_item::ThreadMessage::get(new_id, &mut transaction)
                    .await?
                    .map(|x| x.id.0);
            }

            if let Some(report_id) = edit.report_id {
                let new_id = serde_json::from_str::<ids::ReportId>(&report_id)?;
                checked_id = report_item::Report::get(new_id, &mut transaction)
                    .await?
                    .map(|x| x.id.0);
            }

            if let Some(new_id) = checked_id {
                sqlx::query!(
                    "
                    UPDATE uploaded_images
                    SET context_id = $1
                    WHERE id = $2
                    ",
                    new_id,
                    data.id as database::models::ImageId,
                )
                .execute(&mut transaction)
                .await?;
                transaction.commit().await?;
                return Ok(HttpResponse::Ok().finish());
            } else {
                return Err(ApiError::InvalidInput(
                    "One of 'project_id', 'version_id', 'thread_message_id', or 'report_id' must be specified!"
                        .to_string(),
                ));
            }
        }

        transaction.commit().await?;
    }
    Ok(HttpResponse::NotFound().body(""))
}
