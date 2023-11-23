use std::sync::Arc;

use crate::database::redis::RedisPool;
use crate::file_hosting::FileHost;
use crate::models::ids::ThreadMessageId;
use crate::models::threads::{MessageBody, ThreadId};
use crate::queue::session::AuthQueue;
use crate::routes::{v3, ApiError, v2_reroute};
use actix_web::{delete, get, post, web, HttpRequest, HttpResponse};
use serde::Deserialize;
use sqlx::PgPool;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("thread")
            .service(moderation_inbox)
            .service(thread_get)
            .service(thread_send_message)
            .service(thread_read),
    );
    cfg.service(web::scope("message").service(message_delete));
    cfg.service(threads_get);
}

#[get("{id}")]
pub async fn thread_get(
    req: HttpRequest,
    info: web::Path<(ThreadId,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    v2_reroute::convert_v3_no_extract(v3::threads::thread_get(req, info, pool, redis, session_queue).await?)
}

#[derive(Deserialize)]
pub struct ThreadIds {
    pub ids: String,
}

#[get("threads")]
pub async fn threads_get(
    req: HttpRequest,
    web::Query(ids): web::Query<ThreadIds>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    v2_reroute::convert_v3_no_extract(v3::threads::threads_get(
        req,
        web::Query(v3::threads::ThreadIds { ids: ids.ids }),
        pool,
        redis,
        session_queue,
    )
    .await?)
}

#[derive(Deserialize)]
pub struct NewThreadMessage {
    pub body: MessageBody,
}

#[post("{id}")]
pub async fn thread_send_message(
    req: HttpRequest,
    info: web::Path<(ThreadId,)>,
    pool: web::Data<PgPool>,
    new_message: web::Json<NewThreadMessage>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let new_message = new_message.into_inner();
    v2_reroute::convert_v3_no_extract(v3::threads::thread_send_message(
        req,
        info,
        pool,
        web::Json(v3::threads::NewThreadMessage {
            body: new_message.body,
        }),
        redis,
        session_queue,
    )
    .await?)
}

#[get("inbox")]
pub async fn moderation_inbox(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    v2_reroute::convert_v3_no_extract(v3::threads::moderation_inbox(req, pool, redis, session_queue).await?)
}

#[post("{id}/read")]
pub async fn thread_read(
    req: HttpRequest,
    info: web::Path<(ThreadId,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    v2_reroute::convert_v3_no_extract(v3::threads::thread_read(req, info, pool, redis, session_queue).await?)
}

#[delete("{id}")]
pub async fn message_delete(
    req: HttpRequest,
    info: web::Path<(ThreadMessageId,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
    file_host: web::Data<Arc<dyn FileHost + Send + Sync>>,
) -> Result<HttpResponse, ApiError> {
    v2_reroute::convert_v3_no_extract(v3::threads::message_delete(req, info, pool, redis, session_queue, file_host).await?)
}
