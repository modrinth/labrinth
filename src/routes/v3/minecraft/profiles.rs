use crate::auth::{get_user_from_headers, AuthenticationError};
use crate::database::models::legacy_loader_fields::MinecraftGameVersion;
use crate::database::models::{
    generate_minecraft_profile_id, minecraft_profile_item, MinecraftProfileId,
};
use crate::database::redis::RedisPool;
use crate::models::ids::base62_impl::parse_base62;
use crate::models::ids::VersionId;
use crate::models::minecraft::profile::MinecraftProfile;
use crate::models::pats::Scopes;
use crate::queue::session::AuthQueue;
use crate::routes::v3::project_creation::CreateError;
use crate::routes::ApiError;
use crate::util::validate::validation_errors_to_string;
use crate::{database, models};
use actix_web::web::Data;
use actix_web::{web, HttpRequest, HttpResponse};
use chrono::Utc;
use rand::distributions::Alphanumeric;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::path::PathBuf;
use validator::Validate;

pub fn config(cfg: &mut web::ServiceConfig) {
    // make sure this is routed in front of /minecraft // TODO
    cfg.route("profile", web::post().to(profile_create));
}

//TODO: They might require a specific hash so we can compare file to when it was uploaded ?or just date  I guess
// todo unwrap()

#[derive(Serialize, Deserialize, Validate, Clone)]
pub struct ProfileCreateData {
    #[validate(
        length(min = 3, max = 64),
        custom(function = "crate::util::validate::validate_name")
    )]
    /// The title or name of the profile.
    pub name: String,
    #[validate(
        custom(function = "crate::util::validate::validate_url"),
        length(max = 255)
    )]
    // The icon url of the profile.
    // TODO: upload
    pub icon_url: Option<String>,

    // The loader string (parsed to a loader)
    pub loader: String,
    // The loader version
    pub loader_version: String,
    // The game version string (parsed to a game version)
    pub game_version: String,
}

pub async fn profile_create(
    req: HttpRequest,
    profile_create_data: web::Json<ProfileCreateData>,
    client: Data<PgPool>,
    redis: Data<RedisPool>,
    session_queue: Data<AuthQueue>,
) -> Result<HttpResponse, CreateError> {
    let profile_create_data = profile_create_data.into_inner();

    // The currently logged in user
    let current_user = get_user_from_headers(
        &req,
        &**client,
        &redis,
        &session_queue,
        Some(&[Scopes::MINECRAFT_PROFILE_CREATE]),
    )
    .await?
    .1;

    profile_create_data
        .validate()
        .map_err(|err| CreateError::InvalidInput(validation_errors_to_string(err, None)))?;

    let game_version_id = MinecraftGameVersion::list(&**client, &redis)
        .await?
        .into_iter()
        .find(|x| x.version == profile_create_data.game_version)
        .ok_or_else(|| CreateError::InvalidInput("Invalid Minecraft game version".to_string()))?
        .id;

    let loader_id = database::models::loader_fields::Loader::get_id(
        &profile_create_data.loader,
        &**client,
        &redis,
    )
    .await?
    .ok_or_else(|| CreateError::InvalidInput("Invalid loader".to_string()))?;

    let mut transaction = client.begin().await?;

    let profile_id: MinecraftProfileId = generate_minecraft_profile_id(&mut transaction)
        .await?
        .into();

    let profile_builder_actual = minecraft_profile_item::MinecraftProfile {
        id: profile_id,
        name: profile_create_data.name.clone(),
        owner_id: current_user.id.into(),
        icon_url: profile_create_data.icon_url.clone(),
        created: Utc::now(),
        updated: Utc::now(),
        game_version_id,
        loader_id,
        loader_version: profile_create_data.loader_version,
        versions: Vec::new(),
        overrides: Vec::new(),
    };
    let profile_builder = profile_builder_actual.clone();
    profile_builder_actual.insert(&mut transaction).await?;
    transaction.commit().await?;

    let profile = models::minecraft::profile::MinecraftProfile::from(profile_builder);
    Ok(HttpResponse::Ok().json(profile))
}

#[derive(Serialize, Deserialize)]
pub struct MinecraftProfileIds {
    pub ids: String,
}
pub async fn profiles_get(
    web::Query(ids): web::Query<MinecraftProfileIds>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
) -> Result<HttpResponse, ApiError> {
    // No user check ,as any user/scope can view profiles.
    // In addition, private information (ie: CDN links, tokens, anything outside of the list of hosted versions and install paths) is not returned
    let ids = serde_json::from_str::<Vec<&str>>(&ids.ids)?;
    let ids = ids
        .into_iter()
        .map(|x| parse_base62(x).map(|x| database::models::MinecraftProfileId(x as i64)))
        .collect::<Result<Vec<_>, _>>()?;

    let profiles_data =
        database::models::minecraft_profile_item::MinecraftProfile::get_many(&ids, &**pool, &redis)
            .await?;
    let profiles = profiles_data
        .into_iter()
        .map(|data| MinecraftProfile::from(data))
        .collect::<Vec<_>>();

    Ok(HttpResponse::Ok().json(profiles))
}

pub async fn profile_get(
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
) -> Result<HttpResponse, ApiError> {
    let string = info.into_inner().0;

    // No user check ,as any user/scope can view profiles.
    // In addition, private information (ie: CDN links, tokens, anything outside of the list of hosted versions and install paths) is not returned
    let id = database::models::MinecraftProfileId(parse_base62(&string)? as i64);
    let profile_data =
        database::models::minecraft_profile_item::MinecraftProfile::get(id, &**pool, &redis)
            .await?;
    if let Some(data) = profile_data {
        return Ok(HttpResponse::Ok().json(MinecraftProfile::from(data)));
    }
    Err(ApiError::NotFound)
}

#[derive(Serialize, Deserialize)]
pub struct ProfileDownload {
    // temporary authorization token for the CDN, for downloading the profile files
    pub auth_token: String,

    // Version ids for modrinth-hosted versions
    pub version_ids: Vec<VersionId>,

    // The override cdns for the profile:
    // (cdn url, install path)
    pub override_cdns: Vec<(String, PathBuf)>,
}

pub async fn profile_download(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let url_identifier = info.into_inner().0;

    // Must be logged in to download
    let user_option = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::MINECRAFT_PROFILE_DOWNLOAD]),
    )
    .await?;

    // Fetch the profile information of the desired minecraft profile
    let Some(profile_link_data) =
        database::models::minecraft_profile_item::MinecraftProfileLink::get_url(
            &url_identifier,
            &**pool,
        )
        .await?
    else {
        return Err(ApiError::NotFound);
    };

    let Some(profile) = database::models::minecraft_profile_item::MinecraftProfile::get(
        profile_link_data.shared_profile_id,
        &**pool,
        &redis,
    )
    .await?
    else {
        return Err(ApiError::NotFound);
    };

    let cdn_downloads_required = profile.overrides.len();

    let mut transaction = pool.begin().await?;

    // Check no token exists for the username and profile
    let existing_token =
        database::models::minecraft_profile_item::MinecraftProfileLinkToken::get_from_link_user(
            profile_link_data.id,
            user_option.1.id.into(),
            &mut *transaction,
        )
        .await?;
    if let Some(token) = existing_token {
        // Check if the token is still valid
        if token.expires > Utc::now() {
            // Simply return the token
            transaction.commit().await?;
            return Ok(HttpResponse::Ok().json(ProfileDownload {
                auth_token: token.token,
                version_ids: profile.versions.iter().map(|x| (*x).into()).collect(),
                override_cdns: profile.overrides,
            }));
        }

        // If we're here, the token is invalid, so delete it, and create a new one if we can
        database::models::minecraft_profile_item::MinecraftProfileLinkToken::delete(
            &token.token,
            &mut transaction,
        )
        .await?;
    }

    // If there's no token, or the token is invalid, create a new one
    if profile_link_data.uses_remaining < 1 {
        return Err(ApiError::InvalidInput(
            "No more downloads remaining".to_string(),
        ));
    }

    // Reduce the number of downloads remaining
    sqlx::query!(
        "UPDATE shared_profiles_links SET uses_remaining = uses_remaining - 1 WHERE id = $1",
        profile_link_data.id.0
    )
    .execute(&mut *transaction)
    .await?;

    // Create a new cdn auth token
    let token = database::models::minecraft_profile_item::MinecraftProfileLinkToken {
        token: ChaCha20Rng::from_entropy()
            .sample_iter(&Alphanumeric)
            .take(32)
            .map(char::from)
            .collect::<String>(),
        shared_profiles_links_id: profile_link_data.shared_profile_id,
        created: Utc::now(),
        expires: Utc::now() + chrono::Duration::minutes(5),
    };
    token.insert(&mut transaction).await?;

    // TODO: Create download header to authorize the CDN
    // TODO: only if we have enough downloads left on the profile
    // (and de increment it)

    // TODO: check user, so same user cant request all of t hem

    // TODO: maybe we should not have a limit number of uses on the token itself? isntead just limit it to like, 5 minutes so it can be retried

    transaction.commit().await?;

    Ok(HttpResponse::Ok().json(ProfileDownload {
        auth_token: token.token,
        version_ids: profile.versions.iter().map(|x| (*x).into()).collect(),
        override_cdns: profile.overrides,
    }))
}

// Used by cloudflare to check headers and permit CDN downloads for a pack
pub async fn profile_token_check(
    req: HttpRequest,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    // Extract token from 'authorization' of headers
    let token = req
        .headers()
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| ApiError::Authentication(AuthenticationError::InvalidAuthMethod))?;

    let token = database::models::minecraft_profile_item::MinecraftProfileLinkToken::get_token(
        &token, &**pool,
    )
    .await?;

    if let Some(token) = token {
        if token.expires > Utc::now() {
            return Ok(HttpResponse::Ok().finish());
        }
    }

    Err(ApiError::NotFound)
}
