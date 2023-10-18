use std::{collections::HashSet, fmt::Display, iter::FromIterator};

use actix_web::{
    delete, get, patch, post,
    web::{self},
    HttpRequest, HttpResponse,
};
use chrono::Utc;
use itertools::Itertools;
use rand::{distributions::Alphanumeric, Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;
use serde::Deserialize;
use sha2::Digest;
use sqlx::PgPool;
use validator::Validate;

use super::ApiError;
use crate::{
    auth::get_user_from_headers,
    database::{
        models::{
            generate_oauth_client_id, generate_oauth_redirect_id,
            oauth_client_item::{OAuthClient, OAuthRedirectUri},
            DatabaseError, OAuthClientId, User, UserId,
        },
        redis::RedisPool,
    },
    models::{
        self, ids::base62_impl::parse_base62, oauth_clients::OAuthClientCreationResult,
        pats::Scopes,
    },
    queue::session::AuthQueue,
    routes::v2::project_creation::CreateError,
    util::validate::validation_errors_to_string,
};

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(oauth_client_create);
    cfg.service(get_user_clients);
    cfg.service(oauth_client_delete);
}

#[get("user/{user_id}/oauth_apps")]
pub async fn get_user_clients(
    req: HttpRequest,
    info: web::Path<String>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let current_user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::OAUTH_CLIENT_READ]),
    )
    .await?
    .1;

    let target_user = User::get(&info.into_inner(), &**pool, &redis).await?;

    if let Some(target_user) = target_user {
        let target_user_id: models::ids::UserId = target_user.id.into();
        current_user.validate_can_interact_with_oauth_client(target_user_id)?;

        let clients = OAuthClient::get_all_user_clients(target_user.id, &**pool).await?;

        let response = clients
            .into_iter()
            .map(|c| models::oauth_clients::OAuthClient::from(c))
            .collect_vec();

        Ok(HttpResponse::Ok().json(response))
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
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

    pub max_scopes: Scopes,

    #[validate(length(min = 1))]
    pub redirect_uris: Vec<String>,
}

#[post("oauth_app")]
pub async fn oauth_client_create<'a>(
    req: HttpRequest,
    new_oauth_app: web::Json<NewOAuthApp>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, CreateError> {
    let current_user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::OAUTH_CLIENT_WRITE]),
    )
    .await?
    .1;

    new_oauth_app
        .validate()
        .map_err(|e| CreateError::ValidationError(validation_errors_to_string(e, None)))?;

    let mut transaction = pool.begin().await?;

    let client_id = generate_oauth_client_id(&mut transaction).await?;

    let client_secret = generate_oauth_client_secret();
    let client_secret_hash = format!("{:x}", sha2::Sha512::digest(client_secret.as_bytes()));

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
        secret_hash: client_secret_hash,
    };
    client.clone().insert(&mut transaction).await?;

    transaction.commit().await?;

    let client = models::oauth_clients::OAuthClient::from(client);

    Ok(HttpResponse::Ok().json(OAuthClientCreationResult {
        client,
        client_secret,
    }))
}

#[delete("oauth_app/{id}")]
pub async fn oauth_client_delete<'a>(
    req: HttpRequest,
    client_id: web::Path<String>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let current_user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::OAUTH_CLIENT_DELETE]),
    )
    .await?
    .1;

    let client = get_oauth_client_from_str_id(client_id.into_inner(), &**pool).await?;
    if let Some(client) = client {
        current_user.validate_can_interact_with_oauth_client(client.created_by.into())?;
        OAuthClient::remove(client.id, &**pool).await?;

        Ok(HttpResponse::NoContent().body(""))
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
}

#[derive(Deserialize, Validate)]
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
}

#[patch("oauth_app/{id}")]
pub async fn oauth_client_edit(
    req: HttpRequest,
    client_id: web::Path<String>,
    client_updates: web::Json<OAuthClientEdit>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let current_user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::OAUTH_CLIENT_DELETE]),
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

    if let Some(existing_client) =
        get_oauth_client_from_str_id(client_id.into_inner(), &**pool).await?
    {
        current_user.validate_can_interact_with_oauth_client(existing_client.created_by.into())?;

        let mut updated_client = existing_client.clone();
        let OAuthClientEdit {
            name,
            icon_url,
            max_scopes,
            redirect_uris,
        } = client_updates.into_inner();
        if let Some(name) = name {
            updated_client.name = name;
        }

        if let Some(icon_url) = icon_url {
            updated_client.icon_url = icon_url;
        }

        if let Some(max_scopes) = max_scopes {
            updated_client.max_scopes = max_scopes;
        }

        let mut transaction = pool.begin().await?;
        updated_client
            .update_editable_fields(&mut transaction)
            .await?;

        if let Some(redirects) = redirect_uris {
            edit_redirects(redirects, &existing_client, &mut transaction).await?;
        }

        transaction.commit().await?;

        Ok(HttpResponse::Ok().body(""))
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
}

fn generate_oauth_client_secret() -> String {
    ChaCha20Rng::from_entropy()
        .sample_iter(&Alphanumeric)
        .take(32)
        .map(char::from)
        .collect::<String>()
}

async fn get_oauth_client_from_str_id(
    client_id: String,
    exec: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
) -> Result<Option<OAuthClient>, ApiError> {
    let client_id = OAuthClientId(parse_base62(&client_id)? as i64);
    Ok(OAuthClient::get(client_id, exec).await?)
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
    OAuthClient::insert_redirect_uris(&redirects_to_add, &mut *transaction).await?;

    let mut redirects_to_remove = existing_client.redirect_uris.clone();
    redirects_to_remove.retain(|r| !updated_redirects.contains(&r.uri));
    OAuthClient::remove_redirect_uris(redirects_to_remove.iter().map(|r| r.id), &mut *transaction)
        .await?;

    Ok(())
}