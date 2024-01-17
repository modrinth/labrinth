use crate::database;
use crate::database::models::generate_pat_id;
use crate::util::extract::{ConnectInfo, Extension, Json, Path};
use axum::http::HeaderMap;
use axum::routing::{get, patch, post};
use axum::Router;
use std::net::SocketAddr;
use std::sync::Arc;

use crate::auth::get_user_from_headers;
use crate::routes::ApiError;

use crate::database::redis::RedisPool;
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use rand::distributions::Alphanumeric;
use rand::Rng;
use rand_chacha::rand_core::SeedableRng;
use rand_chacha::ChaCha20Rng;

use crate::models::pats::{PersonalAccessToken, Scopes};
use crate::queue::session::AuthQueue;
use crate::util::validate::validation_errors_to_string;
use serde::Deserialize;
use sqlx::postgres::PgPool;
use validator::Validate;

pub fn config() -> Router {
    Router::new()
        .route("/pat", get(get_pats))
        .route("/pat", post(create_pat))
        .route("/pat/:id", patch(edit_pat).delete(delete_pat))
}

pub async fn get_pats(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<Json<Vec<PersonalAccessToken>>, ApiError> {
    let user = get_user_from_headers(
        &addr,
        &headers,
        &pool,
        &redis,
        &session_queue,
        Some(&[Scopes::PAT_READ]),
    )
    .await?
    .1;

    let pat_ids = database::models::pat_item::PersonalAccessToken::get_user_pats(
        user.id.into(),
        &pool,
        &redis,
    )
    .await?;
    let pats =
        database::models::pat_item::PersonalAccessToken::get_many_ids(&pat_ids, &pool, &redis)
            .await?;

    Ok(Json(
        pats.into_iter()
            .map(|x| PersonalAccessToken::from(x, false))
            .collect::<Vec<_>>(),
    ))
}

#[derive(Deserialize, Validate)]
pub struct NewPersonalAccessToken {
    pub scopes: Scopes,
    #[validate(length(min = 3, max = 255))]
    pub name: String,
    pub expires: DateTime<Utc>,
}

pub async fn create_pat(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
    Json(info): Json<NewPersonalAccessToken>,
) -> Result<Json<PersonalAccessToken>, ApiError> {
    info.validate()
        .map_err(|err| ApiError::InvalidInput(validation_errors_to_string(err, None)))?;

    if info.scopes.is_restricted() {
        return Err(ApiError::InvalidInput(
            "Invalid scopes requested!".to_string(),
        ));
    }
    if info.expires < Utc::now() {
        return Err(ApiError::InvalidInput(
            "Expire date must be in the future!".to_string(),
        ));
    }

    let user = get_user_from_headers(
        &addr,
        &headers,
        &pool,
        &redis,
        &session_queue,
        Some(&[Scopes::PAT_CREATE]),
    )
    .await?
    .1;

    let mut transaction = pool.begin().await?;

    let id = generate_pat_id(&mut transaction).await?;

    let token = ChaCha20Rng::from_entropy()
        .sample_iter(&Alphanumeric)
        .take(60)
        .map(char::from)
        .collect::<String>();
    let token = format!("mrp_{}", token);

    let name = info.name.clone();
    database::models::pat_item::PersonalAccessToken {
        id,
        name: name.clone(),
        access_token: token.clone(),
        scopes: info.scopes,
        user_id: user.id.into(),
        created: Utc::now(),
        expires: info.expires,
        last_used: None,
    }
    .insert(&mut transaction)
    .await?;

    transaction.commit().await?;
    database::models::pat_item::PersonalAccessToken::clear_cache(
        vec![(None, None, Some(user.id.into()))],
        &redis,
    )
    .await?;

    Ok(Json(PersonalAccessToken {
        id: id.into(),
        name,
        access_token: Some(token),
        scopes: info.scopes,
        user_id: user.id,
        created: Utc::now(),
        expires: info.expires,
        last_used: None,
    }))
}

#[derive(Deserialize, Validate)]
pub struct ModifyPersonalAccessToken {
    pub scopes: Option<Scopes>,
    #[validate(length(min = 3, max = 255))]
    pub name: Option<String>,
    pub expires: Option<DateTime<Utc>>,
}

pub async fn edit_pat(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
    Json(info): Json<ModifyPersonalAccessToken>,
) -> Result<StatusCode, ApiError> {
    let user = get_user_from_headers(
        &addr,
        &headers,
        &pool,
        &redis,
        &session_queue,
        Some(&[Scopes::PAT_WRITE]),
    )
    .await?
    .1;

    let pat = database::models::pat_item::PersonalAccessToken::get(&id, &pool, &redis).await?;

    if let Some(pat) = pat {
        if pat.user_id == user.id.into() {
            let mut transaction = pool.begin().await?;

            if let Some(scopes) = &info.scopes {
                if scopes.is_restricted() {
                    return Err(ApiError::InvalidInput(
                        "Invalid scopes requested!".to_string(),
                    ));
                }

                sqlx::query!(
                    "
                    UPDATE pats
                    SET scopes = $1
                    WHERE id = $2
                    ",
                    scopes.bits() as i64,
                    pat.id.0
                )
                .execute(&mut *transaction)
                .await?;
            }
            if let Some(name) = &info.name {
                sqlx::query!(
                    "
                    UPDATE pats
                    SET name = $1
                    WHERE id = $2
                    ",
                    name,
                    pat.id.0
                )
                .execute(&mut *transaction)
                .await?;
            }
            if let Some(expires) = &info.expires {
                if expires < &Utc::now() {
                    return Err(ApiError::InvalidInput(
                        "Expire date must be in the future!".to_string(),
                    ));
                }

                sqlx::query!(
                    "
                    UPDATE pats
                    SET expires = $1
                    WHERE id = $2
                    ",
                    expires,
                    pat.id.0
                )
                .execute(&mut *transaction)
                .await?;
            }

            transaction.commit().await?;
            database::models::pat_item::PersonalAccessToken::clear_cache(
                vec![(Some(pat.id), Some(pat.access_token), Some(pat.user_id))],
                &redis,
            )
            .await?;
        }
    }

    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_pat(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<StatusCode, ApiError> {
    let user = get_user_from_headers(
        &addr,
        &headers,
        &pool,
        &redis,
        &session_queue,
        Some(&[Scopes::PAT_DELETE]),
    )
    .await?
    .1;
    let pat = database::models::pat_item::PersonalAccessToken::get(&id, &pool, &redis).await?;

    if let Some(pat) = pat {
        if pat.user_id == user.id.into() {
            let mut transaction = pool.begin().await?;
            database::models::pat_item::PersonalAccessToken::remove(pat.id, &mut transaction)
                .await?;
            transaction.commit().await?;
            database::models::pat_item::PersonalAccessToken::clear_cache(
                vec![(Some(pat.id), Some(pat.access_token), Some(pat.user_id))],
                &redis,
            )
            .await?;
        }
    }

    Ok(StatusCode::NO_CONTENT)
}
