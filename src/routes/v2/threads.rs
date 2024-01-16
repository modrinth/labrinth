use std::sync::Arc;

use crate::database::redis::RedisPool;
use crate::file_hosting::FileHost;
use crate::models::ids::ThreadMessageId;
use crate::models::threads::{MessageBody, Thread, ThreadId};
use crate::models::v2::threads::LegacyThread;
use crate::queue::session::AuthQueue;
use crate::routes::{v2_reroute, v3, ApiError};
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
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    info: web::Path<(ThreadId,)>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<HttpResponse, ApiError> {
    v3::threads::thread_get(req, info, pool, redis, session_queue)
        .await
        .or_else(v2_reroute::flatten_404_error)
}

#[derive(Deserialize)]
pub struct ThreadIds {
    pub ids: String,
}

#[get("threads")]
pub async fn threads_get(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(ids): Query<ThreadIds>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<HttpResponse, ApiError> {
    let response = v3::threads::threads_get(
        req,
        Query(v3::threads::ThreadIds { ids: ids.ids }),
        pool,
        redis,
        session_queue,
    )
    .await
    .or_else(v2_reroute::flatten_404_error)?;

    // Convert response to V2 format
    match v2_reroute::extract_ok_json::<Vec<Thread>>(response).await {
        Ok(threads) => {
            let threads = threads
                .into_iter()
                .map(LegacyThread::from)
                .collect::<Vec<_>>();
            Ok(Json(threads))
        }
        Err(response) => Ok(response),
    }
}

#[derive(Deserialize)]
pub struct NewThreadMessage {
    pub body: MessageBody,
}

#[post("{id}")]
pub async fn thread_send_message(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    info: web::Path<(ThreadId,)>,
    Extension(pool): Extension<PgPool>,
    new_message: Json<NewThreadMessage>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<HttpResponse, ApiError> {
    let new_message = new_message.into_inner();
    // Returns NoContent, so we don't need to convert the response
    v3::threads::thread_send_message(
        req,
        info,
        pool,
        Json(v3::threads::NewThreadMessage {
            body: new_message.body,
        }),
        redis,
        session_queue,
    )
    .await
    .or_else(v2_reroute::flatten_404_error)
}

#[get("inbox")]
pub async fn moderation_inbox(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<HttpResponse, ApiError> {
    let response = v3::threads::moderation_inbox(req, pool, redis, session_queue)
        .await
        .or_else(v2_reroute::flatten_404_error)?;

    // Convert response to V2 format
    match v2_reroute::extract_ok_json::<Vec<Thread>>(response).await {
        Ok(threads) => {
            let threads = threads
                .into_iter()
                .map(LegacyThread::from)
                .collect::<Vec<_>>();
            Ok(Json(threads))
        }
        Err(response) => Ok(response),
    }
}

#[post("{id}/read")]
pub async fn thread_read(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    info: web::Path<(ThreadId,)>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<HttpResponse, ApiError> {
    // Returns NoContent, so we don't need to convert the response
    v3::threads::thread_read(req, info, pool, redis, session_queue)
        .await
        .or_else(v2_reroute::flatten_404_error)
}

#[delete("{id}")]
pub async fn message_delete(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    info: web::Path<(ThreadMessageId,)>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
    Extension(file_host): Extension<Arc<dyn FileHost + Send + Sync>>,
) -> Result<HttpResponse, ApiError> {
    // Returns NoContent, so we don't need to convert the response
    v3::threads::message_delete(req, info, pool, redis, session_queue, file_host)
        .await
        .or_else(v2_reroute::flatten_404_error)
}
