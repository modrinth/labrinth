use super::AuthProvider;
use crate::auth::AuthenticationError;
use crate::database::models::user_item;
use crate::database::redis::RedisPool;
use crate::models::pats::Scopes;
use crate::models::users::User;
use crate::queue::session::AuthQueue;
use crate::routes::internal::session::get_session_metadata;
use actix_web::http::header::{HeaderValue, AUTHORIZATION};
use actix_web::HttpRequest;
use base64::Engine;
use chrono::Utc;

pub async fn get_user_from_headers<'a, E>(
    req: &HttpRequest,
    executor: E,
    redis: &RedisPool,
    session_queue: &AuthQueue,
    required_scopes: Option<&[Scopes]>,
) -> Result<(Scopes, User), AuthenticationError>
where
    E: sqlx::Executor<'a, Database = sqlx::Postgres> + Copy,
{
    // Fetch DB user record and minos user from headers
    let (scopes, db_user) =
        get_user_record_from_bearer_token(req, None, executor, redis, session_queue)
            .await?
            .ok_or_else(|| AuthenticationError::InvalidCredentials)?;

    let user = User::from_full(db_user);

    if let Some(required_scopes) = required_scopes {
        for scope in required_scopes {
            if !scopes.contains(*scope) {
                return Err(AuthenticationError::InvalidCredentials);
            }
        }
    }

    Ok((scopes, user))
}

pub async fn get_user_record_from_bearer_token<'a, 'b, E>(
    req: &HttpRequest,
    token: Option<&str>,
    executor: E,
    redis: &RedisPool,
    session_queue: &AuthQueue,
) -> Result<Option<(Scopes, user_item::User)>, AuthenticationError>
where
    E: sqlx::Executor<'a, Database = sqlx::Postgres> + Copy,
{
    // This is silly, but the compiler kept complaining and this is the only way this would work
    let mut temp: String = String::new();
    if token.is_none() {
        temp = extract_authorization_header(req)?;
    }
    let token = token.unwrap_or(&temp);

    let possible_user = match token.split_once('_') {
        Some(("mrp", _)) => {
            let pat =
                crate::database::models::pat_item::PersonalAccessToken::get(token, executor, redis)
                    .await?
                    .ok_or_else(|| AuthenticationError::InvalidCredentials)?;

            if pat.expires < Utc::now() {
                return Err(AuthenticationError::InvalidCredentials);
            }

            let user = user_item::User::get_id(pat.user_id, executor, redis).await?;

            session_queue.add_pat(pat.id).await;

            user.map(|x| (pat.scopes, x))
        }
        Some(("mra", _)) => {
            let session =
                crate::database::models::session_item::Session::get(token, executor, redis)
                    .await?
                    .ok_or_else(|| AuthenticationError::InvalidCredentials)?;

            if session.expires < Utc::now() {
                return Err(AuthenticationError::InvalidCredentials);
            }

            let user = user_item::User::get_id(session.user_id, executor, redis).await?;

            let rate_limit_ignore = dotenvy::var("RATE_LIMIT_IGNORE_KEY")?;
            if !req
                .headers()
                .get("x-ratelimit-key")
                .and_then(|x| x.to_str().ok())
                .map(|x| x == rate_limit_ignore)
                .unwrap_or(false)
            {
                let metadata = get_session_metadata(req).await?;
                session_queue.add_session(session.id, metadata).await;
            }

            user.map(|x| (Scopes::all(), x))
        }
        Some(("mro", _)) => {
            use crate::database::models::oauth_token_item::OAuthAccessToken;

            let hash = OAuthAccessToken::hash_token(token);
            let access_token =
                crate::database::models::oauth_token_item::OAuthAccessToken::get(hash, executor)
                    .await?
                    .ok_or(AuthenticationError::InvalidCredentials)?;

            if access_token.expires < Utc::now() {
                return Err(AuthenticationError::InvalidCredentials);
            }

            let user = user_item::User::get_id(access_token.user_id, executor, redis).await?;

            session_queue.add_oauth_access_token(access_token.id).await;

            user.map(|u| (access_token.scopes, u))
        }
        Some(("github", _)) | Some(("gho", _)) | Some(("ghp", _)) => {
            let user = AuthProvider::GitHub.get_user(token).await?;
            let id = AuthProvider::GitHub.get_user_id(&user.id, executor).await?;

            let user = user_item::User::get_id(
                id.ok_or_else(|| AuthenticationError::InvalidCredentials)?,
                executor,
                redis,
            )
            .await?;

            user.map(|x| ((Scopes::all() ^ Scopes::restricted()), x))
        }
        _ => return Err(AuthenticationError::InvalidAuthMethod),
    };
    Ok(possible_user)
}

pub fn extract_authorization_header(req: &HttpRequest) -> Result<String, AuthenticationError> {
    let headers = req.headers();
    let token_val: Option<&HeaderValue> = headers.get(AUTHORIZATION);
    let val = token_val
        .ok_or_else(|| AuthenticationError::InvalidAuthMethod)?
        .to_str()
        .map_err(|_| AuthenticationError::InvalidCredentials)?;

    return match val.split_once(' ') {
        Some(("Bearer", token)) => Ok(token.trim().to_string()),
        Some(("Basic", token)) => {
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(token.trim())
                .map_err(|_| AuthenticationError::InvalidCredentials)?;

            let credentials: String =
                String::from_utf8(decoded).map_err(|_| AuthenticationError::InvalidCredentials)?;

            Ok(credentials
                .split_once(':')
                .ok_or_else(|| AuthenticationError::InvalidCredentials)?
                .1
                .trim()
                .to_string())
        }
        _ => Ok(val.trim().to_string()),
    };
}

pub async fn check_is_moderator_from_headers<'a, 'b, E>(
    req: &HttpRequest,
    executor: E,
    redis: &RedisPool,
    session_queue: &AuthQueue,
    required_scopes: Option<&[Scopes]>,
) -> Result<User, AuthenticationError>
where
    E: sqlx::Executor<'a, Database = sqlx::Postgres> + Copy,
{
    let user = get_user_from_headers(req, executor, redis, session_queue, required_scopes)
        .await?
        .1;

    if user.role.is_mod() {
        Ok(user)
    } else {
        Err(AuthenticationError::InvalidCredentials)
    }
}
