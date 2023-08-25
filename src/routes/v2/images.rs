use std::sync::Arc;

use crate::database;
use crate::database::models::{ids, project_item, thread_item, version_item};
use crate::file_hosting::FileHost;
use crate::models::images::Image;
use crate::queue::session::AuthQueue;
use crate::routes::ApiError;
use crate::util::routes::read_from_payload;
use crate::{auth::get_user_from_headers, models::images::ImageContext};
use actix_web::{delete, patch, post, web, HttpRequest, HttpResponse};
use ids::ThreadMessageId;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(images_add);
    cfg.service(web::scope("image").service(image_delete));
}

#[derive(Serialize, Deserialize)]
pub struct ImageUpload {
    pub ext: String,
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
        let context = ImageContext::from_str(&data.context, None);
        if matches!(context, ImageContext::Unknown) {
            return Err(ApiError::InvalidInput(format!(
                "Invalid context specified: {}!",
                data.context
            )));
        }
        let scopes = vec![context.relevant_scope()];

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
            context,
        };

        // Insert
        db_image.insert(&mut transaction).await?;

        let image = Image::from(db_image);

        transaction.commit().await?;

        Ok(HttpResponse::Ok().json(image))
    } else {
        Err(ApiError::InvalidInput(
            "The specified file is not an image!".to_string(),
        ))
    }
}

#[derive(Deserialize, Serialize)]
pub struct ImageEdit {
    pub set_id: String,
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
    let image_data = database::models::Image::get(&string, &**pool, &redis).await?;

    if let Some(data) = image_data {
        let mut transaction = pool.begin().await?;

        let scopes = vec![data.context.relevant_scope()];
        let user = get_user_from_headers(&req, &**pool, &redis, &session_queue, Some(&scopes))
            .await?
            .1;

        if user.id == data.owner_id.into() {
            let checked_id: Option<i64> = match data.context {
                ImageContext::Project { .. } => {
                    project_item::Project::get(&edit.set_id, &mut transaction, &redis)
                        .await?
                        .map(|x| x.inner.id.0)
                }
                ImageContext::Version { .. } => {
                    let new_id = serde_json::from_str::<ids::VersionId>(&edit.set_id)?;
                    version_item::Version::get(new_id, &mut transaction, &redis)
                        .await?
                        .map(|x| x.inner.id.0)
                }
                ImageContext::ThreadMessage { .. } => {
                    let new_id = serde_json::from_str::<ThreadMessageId>(&edit.set_id)?;
                    thread_item::ThreadMessage::get(new_id, &mut transaction)
                        .await?
                        .map(|x| x.id.0)
                }
                ImageContext::Unknown => None,
            };

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
            }
        }

        transaction.commit().await?;
    }
    Ok(HttpResponse::NotFound().body(""))
}

#[delete("{id}")]
pub async fn image_delete(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<deadpool_redis::Pool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let string: String = info.into_inner().0;

    let image_data = database::models::Image::get(&string, &**pool, &redis).await?;
    if let Some(data) = image_data {
        let scopes = vec![data.context.relevant_scope()];
        let user = get_user_from_headers(&req, &**pool, &redis, &session_queue, Some(&scopes))
            .await?
            .1;

        let mut transaction = pool.begin().await?;
        if user.id == data.owner_id.into() {
            database::models::Image::remove(data.id, &mut transaction, &redis).await?;
            return Ok(HttpResponse::Ok().finish());
        }
        transaction.commit().await?;
    }

    Ok(HttpResponse::NotFound().body(""))
}
