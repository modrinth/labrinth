use std::net::SocketAddr;
use std::sync::Arc;

use crate::database::redis::RedisPool;
use crate::file_hosting::FileHost;
use crate::models::ids::ThreadMessageId;
use crate::models::threads::{MessageBody, ThreadId};
use crate::models::v2::threads::LegacyThread;
use crate::queue::session::AuthQueue;
use crate::routes::{v3, ApiErrorV2};
use crate::util::extract::{ConnectInfo, Extension, Json, Path, Query};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{delete, get, post};
use axum::Router;
use serde::Deserialize;
use sqlx::PgPool;

pub fn config() -> Router {
    Router::new()
        .nest(
            "/thread",
            Router::new()
                .route("/inbox", get(moderation_inbox))
                .route("/:id", get(thread_get).post(thread_send_message))
                .route("/:id/read", post(thread_read)),
        )
        .nest(
            "/message",
            Router::new().route("/:id", delete(message_delete)),
        )
        .route("/threads", get(threads_get))
}

pub async fn thread_get(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<ThreadId>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<Json<LegacyThread>, ApiErrorV2> {
    let Json(thread) = v3::threads::thread_get(
        ConnectInfo(addr),
        headers,
        Path(info),
        Extension(pool),
        Extension(redis),
        Extension(session_queue),
    )
    .await?;

    // Convert response to V2 format
    let thread = LegacyThread::from(thread);
    Ok(Json(thread))
}

#[derive(Deserialize)]
pub struct ThreadIds {
    pub ids: String,
}

pub async fn threads_get(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(ids): Query<ThreadIds>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<Json<Vec<LegacyThread>>, ApiErrorV2> {
    let Json(threads) = v3::threads::threads_get(
        ConnectInfo(addr),
        headers,
        Query(v3::threads::ThreadIds { ids: ids.ids }),
        Extension(pool),
        Extension(redis),
        Extension(session_queue),
    )
    .await?;

    // Convert response to V2 format
    let threads = threads
        .into_iter()
        .map(LegacyThread::from)
        .collect::<Vec<_>>();
    Ok(Json(threads))
}

#[derive(Deserialize)]
pub struct NewThreadMessage {
    pub body: MessageBody,
}

pub async fn thread_send_message(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<ThreadId>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
    Json(new_message): Json<NewThreadMessage>,
) -> Result<StatusCode, ApiErrorV2> {
    Ok(v3::threads::thread_send_message(
        ConnectInfo(addr),
        headers,
        Path(info),
        Extension(pool),
        Extension(redis),
        Extension(session_queue),
        Json(v3::threads::NewThreadMessage {
            body: new_message.body,
        }),
    )
    .await?)
}

pub async fn moderation_inbox(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<Json<Vec<LegacyThread>>, ApiErrorV2> {
    let Json(threads) = v3::threads::moderation_inbox(
        ConnectInfo(addr),
        headers,
        Extension(pool),
        Extension(redis),
        Extension(session_queue),
    )
    .await?;

    // Convert response to V2 format
    let threads = threads
        .into_iter()
        .map(LegacyThread::from)
        .collect::<Vec<_>>();
    Ok(Json(threads))
}

pub async fn thread_read(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<ThreadId>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<StatusCode, ApiErrorV2> {
    Ok(v3::threads::thread_read(
        ConnectInfo(addr),
        headers,
        Path(info),
        Extension(pool),
        Extension(redis),
        Extension(session_queue),
    )
    .await?)
}

pub async fn message_delete(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<ThreadMessageId>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
    Extension(file_host): Extension<Arc<dyn FileHost + Send + Sync>>,
) -> Result<StatusCode, ApiErrorV2> {
    Ok(v3::threads::message_delete(
        ConnectInfo(addr),
        headers,
        Path(info),
        Extension(pool),
        Extension(redis),
        Extension(session_queue),
        Extension(file_host),
    )
    .await?)
}
