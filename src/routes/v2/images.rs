use std::sync::Arc;

use crate::auth::get_user_from_headers;
use crate::database;
use crate::file_hosting::FileHost;
use crate::models::images::Image;
use crate::models::pats::Scopes;
use crate::queue::session::AuthQueue;
use crate::routes::ApiError;
use crate::util::routes::read_from_payload;
use actix_web::{delete, post, web, HttpRequest, HttpResponse};
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
            mod_id: None,
            thread_message_id: None,
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
