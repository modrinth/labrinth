use std::sync::Arc;

use crate::database;
use crate::file_hosting::FileHost;
use crate::models::images::Image;
use crate::models::pats::Scopes;
use crate::queue::session::AuthQueue;
use crate::routes::ApiError;
use crate::util::routes::read_from_payload;
use crate::{auth::get_user_from_headers, models::images::ImageContext};
use actix_web::{delete, patch, post, web, HttpRequest, HttpResponse};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(images_add);
    cfg.service(web::scope("image").service(image_delete));
}

#[derive(Serialize, Deserialize)]
pub struct Extension {
    pub ext: String,
}

#[derive(Serialize, Deserialize)]
pub struct ImageUpload {
    pub ids: String,
}

#[post("image")]
pub async fn images_add(
    req: HttpRequest,
    web::Query(ext): web::Query<Extension>,
    file_host: web::Data<Arc<dyn FileHost + Send + Sync>>,
    mut payload: web::Payload,
    pool: web::Data<PgPool>,
    redis: web::Data<deadpool_redis::Pool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    if let Some(content_type) = crate::util::ext::get_image_content_type(&ext.ext) {
        let cdn_url = dotenvy::var("CDN_URL")?;
        let user = get_user_from_headers(
            &req,
            &**pool,
            &redis,
            &session_queue,
            Some(&[Scopes::IMAGE_POST]),
        )
        .await?
        .1;

        let bytes =
            read_from_payload(&mut payload, 1_048_576, "Icons must be smaller than 1MiB").await?;

        let hash = sha1::Sha1::from(&bytes).hexdigest();
        let upload_data = file_host
            .upload_file(
                content_type,
                &format!("data/cached_images/{}.{}", hash, ext.ext),
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
    pub contexts: Vec<(ImageContext, i32)>,
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

    let mut scopes = edit
        .contexts
        .iter()
        .map(|(context, _)| match context {
            ImageContext::Project => Scopes::PROJECT_WRITE,
            ImageContext::ThreadMessage => Scopes::THREAD_WRITE,
            ImageContext::Unknown => Scopes::NONE,
        })
        .collect::<Vec<Scopes>>();
    scopes.push(Scopes::IMAGE_WRITE);

    let image_data = database::models::Image::get(&string, &**pool, &redis).await?;
    let user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::IMAGE_WRITE]),
    )
    .await?
    .1;

    let mut transaction = pool.begin().await?;
    if let Some(data) = image_data {
        if user.id == data.owner_id.into() {
            sqlx::query!(
                "
                    DELETE FROM images_threads
                    WHERE image_id = $1
                ",
                data.id.0 as i64
            )
            .execute(&mut transaction)
            .await?;

            sqlx::query!(
                "
                    DELETE FROM images_mods
                    WHERE image_id = $1
                ",
                data.id.0 as i64
            )
            .execute(&mut transaction)
            .await?;

            for (context, id) in edit.contexts {
                match context {
                    ImageContext::Project => {
                        sqlx::query!(
                            "
                                INSERT INTO images_mods (image_id, mod_id)
                                VALUES ($1, $2)
                            ",
                            data.id.0 as i64,
                            id as i64
                        )
                        .execute(&mut transaction)
                        .await?;
                    }
                    ImageContext::ThreadMessage => {
                        sqlx::query!(
                            "
                                INSERT INTO images_threads (image_id, thread_message_id)
                                VALUES ($1, $2)
                            ",
                            data.id.0 as i64,
                            id as i64
                        )
                        .execute(&mut transaction)
                        .await?;
                    }
                    ImageContext::Unknown => {}
                }
            }

            return Ok(HttpResponse::Ok().finish());
        }
    }
    transaction.commit().await?;
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
    let user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::IMAGE_DELETE]),
    )
    .await?
    .1;

    let mut transaction = pool.begin().await?;
    if let Some(data) = image_data {
        if user.id == data.owner_id.into() {
            database::models::Image::remove(data.id, &mut transaction, &redis).await?;
            return Ok(HttpResponse::Ok().finish());
        }
    }
    transaction.commit().await?;
    Ok(HttpResponse::NotFound().body(""))
}
