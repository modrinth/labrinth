use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, patch, post};
use axum::Router;
use bytes::Bytes;
use std::net::SocketAddr;
use std::{collections::HashSet, fmt::Display, sync::Arc};

use crate::util::extract::{ConnectInfo, Extension, Json, Path, Query};
use chrono::Utc;
use itertools::Itertools;
use rand::{distributions::Alphanumeric, Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use validator::Validate;

use super::ApiError;
use crate::{
    auth::checks::ValidateAllAuthorized,
    file_hosting::FileHost,
    models::{ids::base62_impl::parse_base62, oauth_clients::DeleteOAuthClientQueryParam},
    util::routes::read_from_payload,
};
use crate::{
    auth::{checks::ValidateAuthorized, get_user_from_headers},
    database::{
        models::{
            generate_oauth_client_id, generate_oauth_redirect_id,
            oauth_client_authorization_item::OAuthClientAuthorization,
            oauth_client_item::{OAuthClient, OAuthRedirectUri},
            DatabaseError, OAuthClientId, User,
        },
        redis::RedisPool,
    },
    models::{
        self,
        oauth_clients::{GetOAuthClientsRequest, OAuthClientCreationResult},
        pats::Scopes,
    },
    queue::session::AuthQueue,
    routes::v3::project_creation::CreateError,
    util::validate::validation_errors_to_string,
};

use crate::database::models::oauth_client_item::OAuthClient as DBOAuthClient;
use crate::models::ids::OAuthClientId as ApiOAuthClientId;

pub fn config() -> Router {
    Router::new().nest(
        "/oauth",
        Router::new()
            .merge(crate::auth::oauth::config())
            .route(
                "/authorizations",
                get(get_user_oauth_authorizations).delete(revoke_oauth_authorization),
            )
            .route("/app", post(oauth_client_create))
            .route(
                "/app/:id",
                get(get_client)
                    .patch(oauth_client_edit)
                    .delete(oauth_client_delete),
            )
            .route(
                "/app/:id/icon",
                patch(oauth_client_icon_edit).delete(oauth_client_icon_delete),
            )
            .route("/apps", get(get_clients)),
    )
}

pub async fn get_user_clients(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<Json<Vec<models::oauth_clients::OAuthClient>>, ApiError> {
    let current_user = get_user_from_headers(
        &addr,
        &headers,
        &pool,
        &redis,
        &session_queue,
        Some(&[Scopes::SESSION_ACCESS]),
    )
    .await?
    .1;

    let target_user = User::get(&info, &pool, &redis).await?;

    if let Some(target_user) = target_user {
        let clients = OAuthClient::get_all_user_clients(target_user.id, &pool).await?;
        clients
            .iter()
            .validate_all_authorized(Some(&current_user))?;

        let response = clients
            .into_iter()
            .map(models::oauth_clients::OAuthClient::from)
            .collect_vec();

        Ok(Json(response))
    } else {
        Err(ApiError::NotFound)
    }
}

pub async fn get_client(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<ApiOAuthClientId>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<Json<models::oauth_clients::OAuthClient>, ApiError> {
    let clients = get_clients_inner(&[id], addr, headers, pool, redis, session_queue).await?;
    if let Some(client) = clients.into_iter().next() {
        Ok(Json(client))
    } else {
        Err(ApiError::NotFound)
    }
}

pub async fn get_clients(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(info): Query<GetOAuthClientsRequest>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<Json<Vec<models::oauth_clients::OAuthClient>>, ApiError> {
    let ids: Vec<_> = info
        .ids
        .iter()
        .map(|id| parse_base62(id).map(ApiOAuthClientId))
        .collect::<Result<_, _>>()?;

    let clients = get_clients_inner(&ids, addr, headers, pool, redis, session_queue).await?;

    Ok(Json(clients))
}

#[derive(Deserialize, Validate)]
pub struct NewOAuthApp {
    #[validate(
        custom(function = "crate::util::validate::validate_name"),
        length(min = 3, max = 255)
    )]
    pub name: String,

    #[validate(
        custom(function = "crate::util::validate::validate_url"),
        length(max = 255)
    )]
    pub icon_url: Option<String>,

    #[validate(custom(function = "crate::util::validate::validate_no_restricted_scopes"))]
    pub max_scopes: Scopes,

    pub redirect_uris: Vec<String>,

    #[validate(
        custom(function = "crate::util::validate::validate_url"),
        length(max = 255)
    )]
    pub url: Option<String>,

    #[validate(length(max = 255))]
    pub description: Option<String>,
}

pub async fn oauth_client_create<'a>(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
    Json(new_oauth_app): Json<NewOAuthApp>,
) -> Result<Json<OAuthClientCreationResult>, CreateError> {
    let current_user = get_user_from_headers(
        &addr,
        &headers,
        &pool,
        &redis,
        &session_queue,
        Some(&[Scopes::SESSION_ACCESS]),
    )
    .await?
    .1;

    new_oauth_app
        .validate()
        .map_err(|e| CreateError::ValidationError(validation_errors_to_string(e, None)))?;

    let mut transaction = pool.begin().await?;

    let client_id = generate_oauth_client_id(&mut transaction).await?;

    let client_secret = generate_oauth_client_secret();
    let client_secret_hash = DBOAuthClient::hash_secret(&client_secret);

    let redirect_uris =
        create_redirect_uris(&new_oauth_app.redirect_uris, client_id, &mut transaction).await?;

    let client = OAuthClient {
        id: client_id,
        icon_url: new_oauth_app.icon_url.clone(),
        max_scopes: new_oauth_app.max_scopes,
        name: new_oauth_app.name.clone(),
        redirect_uris,
        created: Utc::now(),
        created_by: current_user.id.into(),
        url: new_oauth_app.url.clone(),
        description: new_oauth_app.description.clone(),
        secret_hash: client_secret_hash,
    };
    client.clone().insert(&mut transaction).await?;

    transaction.commit().await?;

    let client = models::oauth_clients::OAuthClient::from(client);

    Ok(Json(OAuthClientCreationResult {
        client,
        client_secret,
    }))
}

pub async fn oauth_client_delete<'a>(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(client_id): Path<ApiOAuthClientId>,
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
        Some(&[Scopes::SESSION_ACCESS]),
    )
    .await?
    .1;

    let client = OAuthClient::get(client_id.into(), &pool).await?;
    if let Some(client) = client {
        client.validate_authorized(Some(&current_user))?;
        OAuthClient::remove(client.id, &pool).await?;

        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound)
    }
}

#[derive(Serialize, Deserialize, Validate)]
pub struct OAuthClientEdit {
    #[validate(
        custom(function = "crate::util::validate::validate_name"),
        length(min = 3, max = 255)
    )]
    pub name: Option<String>,

    #[validate(
        custom(function = "crate::util::validate::validate_url"),
        length(max = 255)
    )]
    pub icon_url: Option<Option<String>>,

    pub max_scopes: Option<Scopes>,

    #[validate(length(min = 1))]
    pub redirect_uris: Option<Vec<String>>,

    #[validate(
        custom(function = "crate::util::validate::validate_url"),
        length(max = 255)
    )]
    pub url: Option<Option<String>>,

    #[validate(length(max = 255))]
    pub description: Option<Option<String>>,
}

pub async fn oauth_client_edit(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(client_id): Path<ApiOAuthClientId>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
    Json(client_updates): Json<OAuthClientEdit>,
) -> Result<StatusCode, ApiError> {
    let current_user = get_user_from_headers(
        &addr,
        &headers,
        &pool,
        &redis,
        &session_queue,
        Some(&[Scopes::SESSION_ACCESS]),
    )
    .await?
    .1;

    client_updates
        .validate()
        .map_err(|e| ApiError::Validation(validation_errors_to_string(e, None)))?;

    if client_updates.icon_url.is_none()
        && client_updates.name.is_none()
        && client_updates.max_scopes.is_none()
    {
        return Err(ApiError::InvalidInput("No changes provided".to_string()));
    }

    if let Some(existing_client) = OAuthClient::get(client_id.into(), &pool).await? {
        existing_client.validate_authorized(Some(&current_user))?;

        let mut updated_client = existing_client.clone();
        let OAuthClientEdit {
            name,
            icon_url,
            max_scopes,
            redirect_uris,
            url,
            description,
        } = client_updates;
        if let Some(name) = name {
            updated_client.name = name;
        }

        if let Some(icon_url) = icon_url {
            updated_client.icon_url = icon_url;
        }

        if let Some(max_scopes) = max_scopes {
            updated_client.max_scopes = max_scopes;
        }

        if let Some(url) = url {
            updated_client.url = url;
        }

        if let Some(description) = description {
            updated_client.description = description;
        }

        let mut transaction = pool.begin().await?;
        updated_client
            .update_editable_fields(&mut *transaction)
            .await?;

        if let Some(redirects) = redirect_uris {
            edit_redirects(redirects, &existing_client, &mut transaction).await?;
        }

        transaction.commit().await?;

        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound)
    }
}

#[derive(Serialize, Deserialize)]
pub struct FileExt {
    pub ext: String,
}

#[allow(clippy::too_many_arguments)]
pub async fn oauth_client_icon_edit(
    Query(ext): Query<FileExt>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(client_id): Path<ApiOAuthClientId>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(file_host): Extension<Arc<dyn FileHost + Send + Sync>>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
    payload: Bytes,
) -> Result<StatusCode, ApiError> {
    if let Some(content_type) = crate::util::ext::get_image_content_type(&ext.ext) {
        let cdn_url = dotenvy::var("CDN_URL")?;
        let user = get_user_from_headers(
            &addr,
            &headers,
            &pool,
            &redis,
            &session_queue,
            Some(&[Scopes::SESSION_ACCESS]),
        )
        .await?
        .1;

        let client = OAuthClient::get(client_id.into(), &pool)
            .await?
            .ok_or_else(|| {
                ApiError::InvalidInput("The specified client does not exist!".to_string())
            })?;

        client.validate_authorized(Some(&user))?;

        if let Some(ref icon) = client.icon_url {
            let name = icon.split(&format!("{cdn_url}/")).nth(1);

            if let Some(icon_path) = name {
                file_host.delete_file_version("", icon_path).await?;
            }
        }

        let bytes = read_from_payload(payload, 262144, "Icons must be smaller than 256KiB").await?;
        let hash = sha1::Sha1::from(&bytes).hexdigest();
        let upload_data = file_host
            .upload_file(
                content_type,
                &format!("data/{}/{}.{}", client_id, hash, ext.ext),
                bytes,
            )
            .await?;

        let mut transaction = pool.begin().await?;

        let mut editable_client = client.clone();
        editable_client.icon_url = Some(format!("{}/{}", cdn_url, upload_data.file_name));

        editable_client
            .update_editable_fields(&mut *transaction)
            .await?;

        transaction.commit().await?;

        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::InvalidInput(format!(
            "Invalid format for project icon: {}",
            ext.ext
        )))
    }
}

pub async fn oauth_client_icon_delete(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(client_id): Path<ApiOAuthClientId>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(file_host): Extension<Arc<dyn FileHost + Send + Sync>>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<StatusCode, ApiError> {
    let cdn_url = dotenvy::var("CDN_URL")?;
    let user = get_user_from_headers(
        &addr,
        &headers,
        &pool,
        &redis,
        &session_queue,
        Some(&[Scopes::SESSION_ACCESS]),
    )
    .await?
    .1;

    let client = OAuthClient::get(client_id.into(), &pool)
        .await?
        .ok_or_else(|| {
            ApiError::InvalidInput("The specified client does not exist!".to_string())
        })?;
    client.validate_authorized(Some(&user))?;

    if let Some(ref icon) = client.icon_url {
        let name = icon.split(&format!("{cdn_url}/")).nth(1);

        if let Some(icon_path) = name {
            file_host.delete_file_version("", icon_path).await?;
        }
    }

    let mut transaction = pool.begin().await?;

    let mut editable_client = client.clone();
    editable_client.icon_url = None;

    editable_client
        .update_editable_fields(&mut *transaction)
        .await?;
    transaction.commit().await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn get_user_oauth_authorizations(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<Json<Vec<models::oauth_clients::OAuthClientAuthorization>>, ApiError> {
    let current_user = get_user_from_headers(
        &addr,
        &headers,
        &pool,
        &redis,
        &session_queue,
        Some(&[Scopes::SESSION_ACCESS]),
    )
    .await?
    .1;

    let authorizations =
        OAuthClientAuthorization::get_all_for_user(current_user.id.into(), &pool).await?;

    let mapped: Vec<models::oauth_clients::OAuthClientAuthorization> =
        authorizations.into_iter().map(|a| a.into()).collect_vec();

    Ok(Json(mapped))
}

pub async fn revoke_oauth_authorization(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(info): Query<DeleteOAuthClientQueryParam>,
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
        Some(&[Scopes::SESSION_ACCESS]),
    )
    .await?
    .1;

    OAuthClientAuthorization::remove(info.client_id.into(), current_user.id.into(), &pool).await?;

    Ok(StatusCode::NO_CONTENT)
}

fn generate_oauth_client_secret() -> String {
    ChaCha20Rng::from_entropy()
        .sample_iter(&Alphanumeric)
        .take(32)
        .map(char::from)
        .collect::<String>()
}

async fn create_redirect_uris(
    uri_strings: impl IntoIterator<Item = impl Display>,
    client_id: OAuthClientId,
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<Vec<OAuthRedirectUri>, DatabaseError> {
    let mut redirect_uris = vec![];
    for uri in uri_strings.into_iter() {
        let id = generate_oauth_redirect_id(transaction).await?;
        redirect_uris.push(OAuthRedirectUri {
            id,
            client_id,
            uri: uri.to_string(),
        });
    }

    Ok(redirect_uris)
}

async fn edit_redirects(
    redirects: Vec<String>,
    existing_client: &OAuthClient,
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<(), DatabaseError> {
    let updated_redirects: HashSet<String> = redirects.into_iter().collect();
    let original_redirects: HashSet<String> = existing_client
        .redirect_uris
        .iter()
        .map(|r| r.uri.to_string())
        .collect();

    let redirects_to_add = create_redirect_uris(
        updated_redirects.difference(&original_redirects),
        existing_client.id,
        &mut *transaction,
    )
    .await?;
    OAuthClient::insert_redirect_uris(&redirects_to_add, &mut **transaction).await?;

    let mut redirects_to_remove = existing_client.redirect_uris.clone();
    redirects_to_remove.retain(|r| !updated_redirects.contains(&r.uri));
    OAuthClient::remove_redirect_uris(redirects_to_remove.iter().map(|r| r.id), &mut **transaction)
        .await?;

    Ok(())
}

pub async fn get_clients_inner(
    ids: &[ApiOAuthClientId],
    addr: SocketAddr,
    headers: HeaderMap,
    pool: PgPool,
    redis: RedisPool,
    session_queue: Arc<AuthQueue>,
) -> Result<Vec<models::oauth_clients::OAuthClient>, ApiError> {
    let current_user = get_user_from_headers(
        &addr,
        &headers,
        &pool,
        &redis,
        &session_queue,
        Some(&[Scopes::SESSION_ACCESS]),
    )
    .await?
    .1;

    let ids: Vec<OAuthClientId> = ids.iter().map(|i| (*i).into()).collect();
    let clients = OAuthClient::get_many(&ids, &pool).await?;
    clients
        .iter()
        .validate_all_authorized(Some(&current_user))?;

    Ok(clients.into_iter().map(|c| c.into()).collect_vec())
}
