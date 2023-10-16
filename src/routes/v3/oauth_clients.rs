use actix_web::{
    post,
    web::{self},
    HttpRequest, HttpResponse,
};
use chrono::Utc;
use rand::{distributions::Alphanumeric, Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;
use serde::Deserialize;
use sha2::Digest;
use sqlx::PgPool;
use validator::Validate;

use crate::{
    auth::get_user_from_headers,
    database::{
        models::{
            generate_oauth_client_id, generate_oauth_redirect_id,
            oauth_client_item::{OAuthClient, OAuthRedirectUri},
        },
        redis::RedisPool,
    },
    models::{self, oauth_clients::OAuthClientCreationResult, pats::Scopes},
    queue::session::AuthQueue,
    routes::v2::project_creation::CreateError,
    util::validate::validation_errors_to_string,
};

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(oauth_client_create);
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
    //TODO: Figure out a better error type
    let current_user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::OAUTH_CLIENT_CREATE]),
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

    let mut redirect_uris = vec![];
    for uri in new_oauth_app.redirect_uris.iter() {
        let id = generate_oauth_redirect_id(&mut transaction).await?;
        redirect_uris.push(OAuthRedirectUri {
            id,
            client_id,
            uri: uri.to_string(),
        });
    }

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

fn generate_oauth_client_secret() -> String {
    ChaCha20Rng::from_entropy()
        .sample_iter(&Alphanumeric)
        .take(32)
        .map(char::from)
        .collect::<String>()
}
