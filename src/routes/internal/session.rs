use crate::auth::{get_user_from_headers, AuthenticationError};
use crate::database::models::session_item::Session as DBSession;
use crate::database::models::session_item::SessionBuilder;
use crate::database::models::UserId;
use crate::database::redis::RedisPool;
use crate::models::pats::Scopes;
use crate::models::sessions::Session;
use crate::queue::session::AuthQueue;
use crate::routes::ApiError;
use crate::util::env::parse_var;
use axum::http::header::AUTHORIZATION;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{delete, get, post};
use axum::{Router};
use chrono::Utc;
use rand::distributions::Alphanumeric;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;
use sqlx::PgPool;
use std::net::SocketAddr;
use std::sync::Arc;
use woothee::parser::Parser;
use crate::util::extract::{Json, Path, Extension, ConnectInfo};

pub fn config() -> Router {
    Router::new().nest(
        "/session",
        Router::new()
            .route("/list", get(list))
            .route("/:id", delete(delete_session))
            .route("/refresh", post(refresh)),
    )
}

pub struct SessionMetadata {
    pub city: Option<String>,
    pub country: Option<String>,
    pub ip: String,

    pub os: Option<String>,
    pub platform: Option<String>,
    pub user_agent: String,
}

pub async fn get_session_metadata(
    addr: &SocketAddr,
    headers: &HeaderMap,
) -> Result<SessionMetadata, AuthenticationError> {
    let ip_addr = if parse_var("CLOUDFLARE_INTEGRATION").unwrap_or(false) {
        if let Some(header) = headers.get("CF-Connecting-IP") {
            header.to_str().ok().map(|x| x.to_string())
        } else {
            Some(addr.ip().to_string())
        }
    } else {
        Some(addr.ip().to_string())
    };

    let country = headers.get("cf-ipcountry").and_then(|x| x.to_str().ok());
    let city = headers.get("cf-ipcity").and_then(|x| x.to_str().ok());

    let user_agent = headers
        .get("user-agent")
        .and_then(|x| x.to_str().ok())
        .ok_or_else(|| AuthenticationError::InvalidCredentials)?;

    let parser = Parser::new();
    let info = parser.parse(user_agent);
    let os = if let Some(info) = info {
        Some((info.os, info.name))
    } else {
        None
    };

    Ok(SessionMetadata {
        os: os.map(|x| x.0.to_string()),
        platform: os.map(|x| x.1.to_string()),
        city: city.map(|x| x.to_string()),
        country: country.map(|x| x.to_string()),
        ip: ip_addr
            .ok_or_else(|| AuthenticationError::InvalidCredentials)?
            .to_string(),
        user_agent: user_agent.to_string(),
    })
}

pub async fn issue_session(
    addr: &SocketAddr,
    headers: &HeaderMap,
    user_id: UserId,
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    redis: &RedisPool,
) -> Result<DBSession, AuthenticationError> {
    let metadata = get_session_metadata(addr, headers).await?;

    let session = ChaCha20Rng::from_entropy()
        .sample_iter(&Alphanumeric)
        .take(60)
        .map(char::from)
        .collect::<String>();

    let session = format!("mra_{session}");

    let id = SessionBuilder {
        session,
        user_id,
        os: metadata.os,
        platform: metadata.platform,
        city: metadata.city,
        country: metadata.country,
        ip: metadata.ip,
        user_agent: metadata.user_agent,
    }
    .insert(transaction)
    .await?;

    let session = DBSession::get_id(id, &mut **transaction, redis)
        .await?
        .ok_or_else(|| AuthenticationError::InvalidCredentials)?;

    DBSession::clear_cache(
        vec![(
            Some(session.id),
            Some(session.session.clone()),
            Some(session.user_id),
        )],
        redis,
    )
    .await?;

    Ok(session)
}

pub async fn list(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<Json<Vec<Session>>, ApiError> {
    let current_user = get_user_from_headers(
        &addr,
        &headers,
        &pool,
        &redis,
        &session_queue,
        Some(&[Scopes::SESSION_READ]),
    )
    .await?
    .1;

    let session = headers
        .get(AUTHORIZATION)
        .and_then(|x| x.to_str().ok())
        .ok_or_else(|| AuthenticationError::InvalidCredentials)?;

    let session_ids = DBSession::get_user_sessions(current_user.id.into(), &pool, &redis).await?;
    let sessions = DBSession::get_many_ids(&session_ids, &pool, &redis)
        .await?
        .into_iter()
        .filter(|x| x.expires > Utc::now())
        .map(|x| Session::from(x, false, Some(session)))
        .collect::<Vec<_>>();

    Ok(Json(sessions))
}

pub async fn delete_session(
    Path(info): Path<String>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<StatusCode, ApiError> {
    let current_user = get_user_from_headers(
        &addr,
        &headers,
        &pool,
        &redis,
        &session_queue,
        Some(&[Scopes::SESSION_DELETE]),
    )
    .await?
    .1;

    let session = DBSession::get(info, &pool, &redis).await?;

    if let Some(session) = session {
        if session.user_id == current_user.id.into() {
            let mut transaction = pool.begin().await?;
            DBSession::remove(session.id, &mut transaction).await?;
            transaction.commit().await?;
            DBSession::clear_cache(
                vec![(
                    Some(session.id),
                    Some(session.session),
                    Some(session.user_id),
                )],
                &redis,
            )
            .await?;
        }
    }

    Ok(StatusCode::NO_CONTENT)
}

pub async fn refresh(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<Json<Session>, ApiError> {
    let current_user = get_user_from_headers(&addr, &headers, &pool, &redis, &session_queue, None)
        .await?
        .1;
    let session = headers
        .get(AUTHORIZATION)
        .and_then(|x| x.to_str().ok())
        .ok_or_else(|| ApiError::Authentication(AuthenticationError::InvalidCredentials))?;

    let session = DBSession::get(session, &pool, &redis).await?;

    if let Some(session) = session {
        if current_user.id != session.user_id.into() || session.refresh_expires < Utc::now() {
            return Err(ApiError::Authentication(
                AuthenticationError::InvalidCredentials,
            ));
        }

        let mut transaction = pool.begin().await?;

        DBSession::remove(session.id, &mut transaction).await?;
        let new_session =
            issue_session(&addr, &headers, session.user_id, &mut transaction, &redis).await?;
        transaction.commit().await?;
        DBSession::clear_cache(
            vec![(
                Some(session.id),
                Some(session.session),
                Some(session.user_id),
            )],
            &redis,
        )
        .await?;

        Ok(Json(Session::from(new_session, true, None)))
    } else {
        Err(ApiError::Authentication(
            AuthenticationError::InvalidCredentials,
        ))
    }
}
