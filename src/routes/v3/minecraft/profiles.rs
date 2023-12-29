use crate::auth::checks::filter_visible_version_ids;
use crate::auth::{get_user_from_headers, AuthenticationError};
use crate::database::models::legacy_loader_fields::MinecraftGameVersion;
use crate::database::models::{
    generate_minecraft_profile_id, generate_minecraft_profile_link_id, minecraft_profile_item,
    version_item,
};
use crate::database::redis::RedisPool;
use crate::file_hosting::FileHost;
use crate::models::ids::base62_impl::parse_base62;
use crate::models::ids::VersionId;
use crate::models::minecraft::profile::{
    MinecraftProfile, MinecraftProfileId, MinecraftProfileShareLink, DEFAULT_PROFILE_MAX_USERS,
};
use crate::models::pats::Scopes;
use crate::queue::session::AuthQueue;
use crate::routes::v3::project_creation::CreateError;
use crate::routes::ApiError;
use crate::util::routes::{read_from_field, read_from_payload};
use crate::util::validate::validation_errors_to_string;
use crate::{database, models};
use actix_multipart::{Field, Multipart};
use actix_web::web::Data;
use actix_web::{web, HttpRequest, HttpResponse};
use chrono::Utc;
use futures::StreamExt;
use itertools::Itertools;
use rand::distributions::Alphanumeric;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::path::PathBuf;
use std::sync::Arc;
use validator::Validate;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("minecraft")
            .route("profile", web::post().to(profile_create))
            .route("check_token", web::get().to(profile_token_check))
            .service(
                web::scope("profile")
                    .route("{id}", web::get().to(profile_get))
                    .route("{id}", web::patch().to(profile_edit))
                    .route("{id}", web::delete().to(profile_delete))
                    .route(
                        "{id}/override",
                        web::post().to(minecraft_profile_add_override),
                    )
                    .route("{id}/share", web::get().to(profile_share))
                    .route(
                        "{id}/share/{url_identifier}",
                        web::get().to(profile_link_get),
                    )
                    .route(
                        "{id}/accept/{url_identifier}",
                        web::post().to(accept_share_link),
                    )
                    .route("{id}/download", web::get().to(profile_download))
                    .route("{id}/icon", web::patch().to(profile_icon_edit))
                    .route("{id}/icon", web::delete().to(delete_profile_icon)),
            ),
    );
}

#[derive(Serialize, Deserialize, Validate, Clone)]
pub struct ProfileCreateData {
    #[validate(
        length(min = 3, max = 64),
        custom(function = "crate::util::validate::validate_name")
    )]
    /// The title or name of the profile.
    pub name: String,
    // The loader string (parsed to a loader)
    pub loader: String,
    // The loader version
    pub loader_version: String,
    // The game version string (parsed to a game version)
    pub game_version: String,
    // The list of versions to include in the profile (does not include overrides)
    pub versions: Vec<VersionId>,
}

// Create a new minecraft profile
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

    let profile_id: database::models::MinecraftProfileId =
        generate_minecraft_profile_id(&mut transaction).await?;

    let version_ids = profile_create_data
        .versions
        .into_iter()
        .map(|x| x.into())
        .collect::<Vec<_>>();
    let versions = version_item::Version::get_many(&version_ids, &**client, &redis)
        .await?
        .into_iter()
        .map(|x| x.inner)
        .collect::<Vec<_>>();

    // Filters versions authorized to see
    let versions = filter_visible_version_ids(
        versions.iter().collect_vec(),
        &Some(current_user.clone()),
        &client,
        &redis,
    )
    .await
    .map_err(|_| CreateError::InvalidInput("Could not fetch submitted version ids".to_string()))?;

    let profile_builder_actual = minecraft_profile_item::MinecraftProfile {
        id: profile_id,
        name: profile_create_data.name.clone(),
        owner_id: current_user.id.into(),
        icon_url: None,
        created: Utc::now(),
        updated: Utc::now(),
        game_version_id,
        loader_id,
        loader: profile_create_data.loader,
        loader_version: profile_create_data.loader_version,
        maximum_users: DEFAULT_PROFILE_MAX_USERS as i32,
        users: vec![current_user.id.into()],
        versions,
        overrides: Vec::new(),
    };
    let profile_builder = profile_builder_actual.clone();
    profile_builder_actual.insert(&mut transaction).await?;
    transaction.commit().await?;

    let profile = models::minecraft::profile::MinecraftProfile::from(
        profile_builder,
        Some(current_user.id.into()),
    );
    Ok(HttpResponse::Ok().json(profile))
}

#[derive(Serialize, Deserialize)]
pub struct MinecraftProfileIds {
    pub ids: String,
}
// Get several minecraft profiles by their ids
pub async fn profiles_get(
    req: HttpRequest,
    web::Query(ids): web::Query<MinecraftProfileIds>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let user_id = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        None, // No scopes required to read your own links
    )
    .await
    .ok()
    .map(|x| x.1.id.into());

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
        .map(|x| MinecraftProfile::from(x, user_id))
        .collect::<Vec<_>>();

    Ok(HttpResponse::Ok().json(profiles))
}

// Get a minecraft profile by its id
pub async fn profile_get(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let string = info.into_inner().0;

    let user_id = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        None, // No scopes required to read your own links
    )
    .await
    .ok()
    .map(|x| x.1.id.into());

    // No user check ,as any user/scope can view profiles.
    // In addition, private information (ie: CDN links, tokens, anything outside of the list of hosted versions and install paths) is not returned
    let id = database::models::MinecraftProfileId(parse_base62(&string)? as i64);
    let profile_data =
        database::models::minecraft_profile_item::MinecraftProfile::get(id, &**pool, &redis)
            .await?;
    if let Some(data) = profile_data {
        return Ok(HttpResponse::Ok().json(MinecraftProfile::from(data, user_id)));
    }
    Err(ApiError::NotFound)
}

#[derive(Serialize, Deserialize, Validate, Clone)]
pub struct EditMinecraftProfile {
    #[validate(
        length(min = 3, max = 64),
        custom(function = "crate::util::validate::validate_name")
    )]
    /// The title or name of the profile.
    pub name: Option<String>,
    #[validate(
        custom(function = "crate::util::validate::validate_url"),
        length(max = 255)
    )]
    // The loader string (parsed to a loader)
    pub loader: Option<String>,
    // The loader version
    pub loader_version: Option<String>,
    // The game version string (parsed to a game version)
    pub game_version: Option<String>,
    // The list of versions to include in the profile (does not include overrides)
    pub versions: Option<Vec<VersionId>>,
}

// Edit a minecraft profile
pub async fn profile_edit(
    req: HttpRequest,
    info: web::Path<(String,)>,
    edit_data: web::Json<EditMinecraftProfile>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let string = info.into_inner().0;
    let edit_data = edit_data.into_inner();
    // Must be logged in to edit
    let user_option = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::MINECRAFT_PROFILE_WRITE]),
    )
    .await?;

    // Confirm this is our project, then if so, edit
    let id = database::models::MinecraftProfileId(parse_base62(&string)? as i64);
    let mut transaction = pool.begin().await?;
    let profile_data = database::models::minecraft_profile_item::MinecraftProfile::get(
        id,
        &mut *transaction,
        &redis,
    )
    .await?;

    if let Some(data) = profile_data {
        if data.owner_id == user_option.1.id.into() {
            // Edit the profile
            if let Some(name) = edit_data.name {
                sqlx::query!(
                    "UPDATE shared_profiles SET name = $1 WHERE id = $2",
                    name,
                    data.id.0
                )
                .execute(&mut *transaction)
                .await?;
            }
            if let Some(loader) = edit_data.loader {
                let loader_id = database::models::loader_fields::Loader::get_id(
                    &loader,
                    &mut *transaction,
                    &redis,
                )
                .await?
                .ok_or_else(|| ApiError::InvalidInput("Invalid loader".to_string()))?;

                sqlx::query!(
                    "UPDATE shared_profiles SET loader_id = $1 WHERE id = $2",
                    loader_id.0,
                    data.id.0
                )
                .execute(&mut *transaction)
                .await?;
            }
            if let Some(loader_version) = edit_data.loader_version {
                sqlx::query!(
                    "UPDATE shared_profiles SET loader_version = $1 WHERE id = $2",
                    loader_version,
                    data.id.0
                )
                .execute(&mut *transaction)
                .await?;
            }
            if let Some(game_version) = edit_data.game_version {
                let new_game_id =
                    database::models::legacy_loader_fields::MinecraftGameVersion::list(
                        &**pool, &redis,
                    )
                    .await?
                    .into_iter()
                    .find(|x| x.version == game_version)
                    .ok_or_else(|| {
                        ApiError::InvalidInput("Invalid Minecraft game version".to_string())
                    })?
                    .id;

                sqlx::query!(
                    "UPDATE shared_profiles SET game_version_id = $1 WHERE id = $2",
                    new_game_id.0,
                    data.id.0
                )
                .execute(&mut *transaction)
                .await?;
            }
            if let Some(versions) = edit_data.versions {
                let version_ids = versions.into_iter().map(|x| x.into()).collect::<Vec<_>>();
                let versions =
                    version_item::Version::get_many(&version_ids, &mut *transaction, &redis)
                        .await?
                        .into_iter()
                        .map(|x| x.inner)
                        .collect::<Vec<_>>();

                // Filters versions authorized to see
                let versions = filter_visible_version_ids(
                    versions.iter().collect_vec(),
                    &Some(user_option.1.clone()),
                    &pool,
                    &redis,
                )
                .await
                .map_err(|_| {
                    ApiError::InvalidInput("Could not fetch submitted version ids".to_string())
                })?;

                // Remove all shared profile mods of this profile where version_id is set
                sqlx::query!(
                    "DELETE FROM shared_profiles_mods WHERE shared_profile_id = $1 AND version_id IS NOT NULL",
                    data.id.0
                )
                .execute(&mut *transaction)
                .await?;

                // Insert all new shared profile mods
                for v in versions {
                    sqlx::query!(
                        "INSERT INTO shared_profiles_mods (shared_profile_id, version_id) VALUES ($1, $2)",
                        data.id.0,
                        v.0
                    )
                    .execute(&mut *transaction)
                    .await?;
                }
            }
            transaction.commit().await?;
            minecraft_profile_item::MinecraftProfile::clear_cache(data.id, &redis).await?;
            return Ok(HttpResponse::Ok().finish());
        } else {
            return Err(ApiError::CustomAuthentication(
                "You are not the owner of this profile".to_string(),
            ));
        }
    }
    Err(ApiError::NotFound)
}

// Delete a minecraft profile
pub async fn profile_delete(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let string = info.into_inner().0;

    // Must be logged in to delete
    let user_option = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::MINECRAFT_PROFILE_WRITE]),
    )
    .await?;

    // Confirm this is our project, then if so, delete
    let id = database::models::MinecraftProfileId(parse_base62(&string)? as i64);
    let profile_data =
        database::models::minecraft_profile_item::MinecraftProfile::get(id, &**pool, &redis)
            .await?;
    if let Some(data) = profile_data {
        if data.owner_id == user_option.1.id.into() {
            let mut transaction = pool.begin().await?;
            database::models::minecraft_profile_item::MinecraftProfile::remove(
                data.id,
                &mut transaction,
                &redis,
            )
            .await?;
            transaction.commit().await?;
            minecraft_profile_item::MinecraftProfile::clear_cache(data.id, &redis).await?;
            return Ok(HttpResponse::Ok().finish());
        }
    }

    Err(ApiError::NotFound)
}

// Share a minecraft profile with a friend.
// This generates a link struct, including the field 'url'
// that can be shared with friends to generate a token a limited number of times.
// TODO: This link should not be an API link, but a modrinth:// link that is translatable to an API link by the launcher
pub async fn profile_share(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let string = info.into_inner().0;

    // Must be logged in to share
    let user_option = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::MINECRAFT_PROFILE_WRITE]),
    )
    .await?;

    // Confirm this is our project, then if so, share
    let id = database::models::MinecraftProfileId(parse_base62(&string)? as i64);
    let profile_data =
        database::models::minecraft_profile_item::MinecraftProfile::get(id, &**pool, &redis)
            .await?;

    if let Some(data) = profile_data {
        if data.owner_id == user_option.1.id.into() {
            // Generate a share link identifier
            let identifier = ChaCha20Rng::from_entropy()
                .sample_iter(&Alphanumeric)
                .take(8)
                .map(char::from)
                .collect::<String>();

            // Generate a new share link id
            let mut transaction = pool.begin().await?;
            let profile_link_id = generate_minecraft_profile_link_id(&mut transaction).await?;

            let link = database::models::minecraft_profile_item::MinecraftProfileLink {
                id: profile_link_id,
                shared_profile_id: data.id,
                link_identifier: identifier.clone(),
                created: Utc::now(),
                expires: Utc::now() + chrono::Duration::days(7),
            };
            link.insert(&mut transaction).await?;
            transaction.commit().await?;
            minecraft_profile_item::MinecraftProfile::clear_cache(data.id, &redis).await?;
            return Ok(HttpResponse::Ok().json(MinecraftProfileShareLink::from(link)));
        }
    }
    Err(ApiError::NotFound)
}

// See the status of a link to a profile by its id
// This is used by the to check if the link is expired, etc.
pub async fn profile_link_get(
    req: HttpRequest,
    info: web::Path<(String, String)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let url_identifier = info.into_inner().1;
    // Must be logged in to check
    let user_option = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        None, // No scopes required to read your own links
    )
    .await?;

    // Confirm this is our project, then if so, share
    let link_data = database::models::minecraft_profile_item::MinecraftProfileLink::get_url(
        &url_identifier,
        &**pool,
    )
    .await?
    .ok_or_else(|| ApiError::NotFound)?;

    let data = database::models::minecraft_profile_item::MinecraftProfile::get(
        link_data.shared_profile_id,
        &**pool,
        &redis,
    )
    .await?
    .ok_or_else(|| ApiError::NotFound)?;

    // Only view link meta information if the user is the owner of the profile
    if data.owner_id == user_option.1.id.into() {
        Ok(HttpResponse::Ok().json(MinecraftProfileShareLink::from(link_data)))
    } else {
        Err(ApiError::NotFound)
    }
}

// Accept a share link to a profile
// This adds the user to the team
// TODO: With above change, this is the API link that is translated from a modrinth:// link by the launcher, which would then download it
pub async fn accept_share_link(
    req: HttpRequest,
    info: web::Path<(MinecraftProfileId, String)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let (profile_id, url_identifier) = info.into_inner();

    // Must be logged in to accept
    let user_option = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::MINECRAFT_PROFILE_WRITE]),
    )
    .await?;

    // Fetch the profile information of the desired minecraft profile
    let link_data = database::models::minecraft_profile_item::MinecraftProfileLink::get_url(
        &url_identifier,
        &**pool,
    )
    .await?
    .ok_or_else(|| ApiError::NotFound)?;

    // Confirm it matches the profile id
    if link_data.shared_profile_id != profile_id.into() {
        return Err(ApiError::NotFound);
    }

    let data = database::models::minecraft_profile_item::MinecraftProfile::get(
        link_data.shared_profile_id,
        &**pool,
        &redis,
    )
    .await?
    .ok_or_else(|| ApiError::NotFound)?;

    // Confirm this is not our profile
    if data.owner_id == user_option.1.id.into() {
        return Err(ApiError::InvalidInput(
            "You cannot accept your own share link".to_string(),
        ));
    }

    // Confirm we are not already on the team
    if data.users.iter().any(|x| *x == user_option.1.id.into()) {
        return Err(ApiError::InvalidInput(
            "You are already on this profile's team".to_string(),
        ));
    }

    // Confirm we are not over the maximum users
    if data.maximum_users <= data.users.len() as i32 {
        return Err(ApiError::InvalidInput(
            "This profile has too many users".to_string(),
        ));
    }

    // Add the user to the team
    sqlx::query!(
        "INSERT INTO shared_profiles_users (shared_profile_id, user_id) VALUES ($1, $2)",
        data.id.0 as i64,
        user_option.1.id.0 as i64
    )
    .execute(&**pool)
    .await?;
    minecraft_profile_item::MinecraftProfile::clear_cache(data.id, &redis).await?;

    Ok(HttpResponse::NoContent().finish())
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

// Download a minecraft profile
// Only the owner of the profile or an invited user can download
pub async fn profile_download(
    req: HttpRequest,
    info: web::Path<(MinecraftProfileId,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let cdn_url = dotenvy::var("CDN_URL")?;
    let profile_id = info.into_inner().0;

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
    let Some(profile) = database::models::minecraft_profile_item::MinecraftProfile::get(
        profile_id.into(),
        &**pool,
        &redis,
    )
    .await?
    else {
        return Err(ApiError::NotFound);
    };

    // Check if this user is on the profile user list
    if !profile.users.contains(&user_option.1.id.into()) {
        return Err(ApiError::CustomAuthentication(
            "You are not on this profile's team".to_string(),
        ));
    }

    let mut transaction = pool.begin().await?;

    // Check no token exists for the username and profile
    let existing_token =
        database::models::minecraft_profile_item::MinecraftProfileLinkToken::get_from_profile_user(
            profile.id,
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
                override_cdns: profile
                    .overrides
                    .into_iter()
                    .map(|x| (format!("{}/custom_files/{}", cdn_url, x.0), x.1))
                    .collect::<Vec<_>>(),
            }));
        }

        // If we're here, the token is invalid, so delete it, and create a new one if we can
        database::models::minecraft_profile_item::MinecraftProfileLinkToken::delete(
            &token.token,
            &mut transaction,
        )
        .await?;
    }

    // Create a new cdn auth token
    let token = database::models::minecraft_profile_item::MinecraftProfileLinkToken {
        user_id: user_option.1.id.into(), // This user is requesting the download
        token: ChaCha20Rng::from_entropy()
            .sample_iter(&Alphanumeric)
            .take(32)
            .map(char::from)
            .collect::<String>(),
        shared_profiles_id: profile.id,
        created: Utc::now(),
        expires: Utc::now() + chrono::Duration::minutes(5),
    };
    token.insert(&mut transaction).await?;

    let override_cdns = profile
        .overrides
        .into_iter()
        .map(|x| (format!("{}/custom_files/{}", cdn_url, x.0), x.1))
        .collect::<Vec<_>>();
    transaction.commit().await?;
    minecraft_profile_item::MinecraftProfile::clear_cache(profile.id, &redis).await?;

    Ok(HttpResponse::Ok().json(ProfileDownload {
        auth_token: token.token,
        version_ids: profile.versions.iter().map(|x| (*x).into()).collect(),
        override_cdns,
    }))
}

#[derive(Deserialize)]
pub struct TokenUrl {
    pub url: String, // TODO: could take a vec instead?
}

// Used by cloudflare to check headers and permit CDN downloads for a pack
// Checks headers for 'authorization: xxyyzz' where xxyyzz is a valid token
// that allows for downloading of url 'url'
pub async fn profile_token_check(
    req: HttpRequest,
    file_url: web::Query<TokenUrl>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
) -> Result<HttpResponse, ApiError> {
    let cdn_url = dotenvy::var("CDN_URL")?;
    let file_url = file_url.into_inner().url;

    // Extract token from 'authorization' of headers
    let token = req
        .headers()
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| ApiError::Authentication(AuthenticationError::InvalidAuthMethod))?;

    let token = database::models::minecraft_profile_item::MinecraftProfileLinkToken::get_token(
        token, &**pool,
    )
    .await?
    .ok_or_else(|| ApiError::Authentication(AuthenticationError::InvalidAuthMethod))?;

    if token.expires <= Utc::now() {
        return Err(ApiError::Authentication(
            AuthenticationError::InvalidAuthMethod,
        ));
    }

    // Get valid urls for the profile
    let profile = database::models::minecraft_profile_item::MinecraftProfile::get(
        token.shared_profiles_id,
        &**pool,
        &redis,
    )
    .await?
    .ok_or_else(|| ApiError::Authentication(AuthenticationError::InvalidAuthMethod))?;

    // Check the token is valid for the requested file
    let file_url_hash = file_url
        .split(&format!("{cdn_url}/custom_files/"))
        .nth(1)
        .ok_or_else(|| ApiError::Authentication(AuthenticationError::InvalidAuthMethod))?;

    let valid = profile.overrides.iter().any(|x| x.0 == file_url_hash);
    if !valid {
        Err(ApiError::Authentication(
            AuthenticationError::InvalidAuthMethod,
        ))
    } else {
        Ok(HttpResponse::Ok().finish())
    }
}

#[derive(Serialize, Deserialize)]
pub struct Extension {
    pub ext: String,
}

#[allow(clippy::too_many_arguments)]
pub async fn profile_icon_edit(
    web::Query(ext): web::Query<Extension>,
    req: HttpRequest,
    info: web::Path<(MinecraftProfileId,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    file_host: web::Data<Arc<dyn FileHost + Send + Sync>>,
    mut payload: web::Payload,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    if let Some(content_type) = crate::util::ext::get_image_content_type(&ext.ext) {
        let cdn_url = dotenvy::var("CDN_URL")?;
        let user = get_user_from_headers(
            &req,
            &**pool,
            &redis,
            &session_queue,
            Some(&[Scopes::MINECRAFT_PROFILE_WRITE]),
        )
        .await?
        .1;
        let id = info.into_inner().0;

        let profile_item = database::models::minecraft_profile_item::MinecraftProfile::get(
            id.into(),
            &**pool,
            &redis,
        )
        .await?
        .ok_or_else(|| {
            ApiError::InvalidInput("The specified profile does not exist!".to_string())
        })?;

        if !user.role.is_mod() && profile_item.owner_id != user.id.into() {
            return Err(ApiError::CustomAuthentication(
                "You don't have permission to edit this profile's icon.".to_string(),
            ));
        }

        if let Some(icon) = profile_item.icon_url {
            let name = icon.split(&format!("{cdn_url}/")).nth(1);

            if let Some(icon_path) = name {
                file_host.delete_file_version("", icon_path).await?;
            }
        }

        let bytes =
            read_from_payload(&mut payload, 262144, "Icons must be smaller than 256KiB").await?;

        let color = crate::util::img::get_color_from_img(&bytes)?;

        let hash = sha1::Sha1::from(&bytes).hexdigest();
        let id: MinecraftProfileId = profile_item.id.into();
        let upload_data = file_host
            .upload_file(
                content_type,
                &format!("data/{}/{}.{}", id, hash, ext.ext),
                bytes.freeze(),
            )
            .await?;

        let mut transaction = pool.begin().await?;

        sqlx::query!(
            "
            UPDATE shared_profiles
            SET icon_url = $1, color = $2
            WHERE (id = $3)
            ",
            format!("{}/{}", cdn_url, upload_data.file_name),
            color.map(|x| x as i32),
            profile_item.id as database::models::ids::MinecraftProfileId,
        )
        .execute(&mut *transaction)
        .await?;

        transaction.commit().await?;
        database::models::minecraft_profile_item::MinecraftProfile::clear_cache(
            profile_item.id,
            &redis,
        )
        .await?;

        Ok(HttpResponse::NoContent().body(""))
    } else {
        Err(ApiError::InvalidInput(format!(
            "Invalid format for project icon: {}",
            ext.ext
        )))
    }
}

pub async fn delete_profile_icon(
    req: HttpRequest,
    info: web::Path<(MinecraftProfileId,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    file_host: web::Data<Arc<dyn FileHost + Send + Sync>>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::MINECRAFT_PROFILE_WRITE]),
    )
    .await?
    .1;
    let id = info.into_inner().0;

    let profile_item =
        database::models::minecraft_profile_item::MinecraftProfile::get(id.into(), &**pool, &redis)
            .await?
            .ok_or_else(|| {
                ApiError::InvalidInput("The specified profile does not exist!".to_string())
            })?;

    if !user.role.is_mod() && profile_item.owner_id != user.id.into() {
        return Err(ApiError::CustomAuthentication(
            "You don't have permission to edit this profile's icon.".to_string(),
        ));
    }

    let cdn_url = dotenvy::var("CDN_URL")?;
    if let Some(icon) = profile_item.icon_url {
        let name = icon.split(&format!("{cdn_url}/")).nth(1);

        if let Some(icon_path) = name {
            file_host.delete_file_version("", icon_path).await?;
        }
    }

    let mut transaction = pool.begin().await?;

    sqlx::query!(
        "
        UPDATE shared_profiles
        SET icon_url = NULL, color = NULL
        WHERE (id = $1)
        ",
        profile_item.id as database::models::ids::MinecraftProfileId,
    )
    .execute(&mut *transaction)
    .await?;

    transaction.commit().await?;

    database::models::minecraft_profile_item::MinecraftProfile::clear_cache(
        profile_item.id,
        &redis,
    )
    .await?;

    Ok(HttpResponse::NoContent().body(""))
}

// Add a new override mod to a minecraft profile, by uploading it to the CDN
// Accepts a multipart field
// the first part is called `data` and contains a json array of objects with the following fields:
// file_name: String
// install_path: String
// The rest of the parts are files, and their install paths are matched to the install paths in the data field
#[derive(Serialize, Deserialize)]
struct MultipartFile {
    pub file_name: String,
    pub install_path: String,
}

#[allow(clippy::too_many_arguments)]
pub async fn minecraft_profile_add_override(
    req: HttpRequest,
    client_id: web::Path<MinecraftProfileId>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    file_host: web::Data<Arc<dyn FileHost + Send + Sync>>,
    mut payload: Multipart,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, CreateError> {
    let client_id = client_id.into_inner();
    let user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::MINECRAFT_PROFILE_WRITE]),
    )
    .await?
    .1;

    // Check if this is our profile
    let profile_item = database::models::minecraft_profile_item::MinecraftProfile::get(
        client_id.into(),
        &**pool,
        &redis,
    )
    .await?
    .ok_or_else(|| {
        CreateError::InvalidInput("The specified profile does not exist!".to_string())
    })?;

    if !user.role.is_mod() && profile_item.owner_id != user.id.into() {
        return Err(CreateError::CustomAuthenticationError(
            "You don't have permission to add overrides.".to_string(),
        ));
    }

    struct UploadedFile {
        pub install_path: String,
        pub hash: String,
    }

    let mut error = None;
    let mut uploaded_files = Vec::new();

    let files: Vec<MultipartFile> = {
        // First, get the data field
        let mut field = payload.next().await.ok_or_else(|| {
            CreateError::InvalidInput(String::from("Upload must have a data field"))
        })??;

        let content_disposition = field.content_disposition().clone();
        // Allow any content type
        let name = content_disposition
            .get_name()
            .ok_or_else(|| CreateError::InvalidInput(String::from("Upload must have a name")))?;

        if name == "data" {
            let mut d: Vec<u8> = Vec::new();
            while let Some(chunk) = field.next().await {
                d.extend_from_slice(&chunk.map_err(CreateError::MultipartError)?);
            }
            serde_json::from_slice(&d)?
        } else {
            return Err(CreateError::InvalidInput(String::from(
                "`data` field must come before file fields",
            )));
        }
    };

    while let Some(item) = payload.next().await {
        let mut field: Field = item?;
        if error.is_some() {
            continue;
        }
        let result = async {
            let content_disposition = field.content_disposition().clone();
            let content_type = field
                .content_type()
                .map(|x| x.essence_str())
                .unwrap_or_else(|| "application/octet-stream")
                .to_string();
            // Allow any content type
            let name = content_disposition.get_name().ok_or_else(|| {
                CreateError::InvalidInput(String::from("Upload must have a name"))
            })?;

            let data =
                read_from_field(&mut field, 262144, "Icons must be smaller than 256KiB").await?;
            let install_path = files
                .iter()
                .find(|x| x.file_name == name)
                .ok_or_else(|| {
                    CreateError::InvalidInput(format!(
                        "No matching file name in `data` for file '{}'",
                        name
                    ))
                })?
                .install_path
                .clone();

            let hash = sha1::Sha1::from(&data).hexdigest();

            file_host
                .upload_file(
                    &content_type,
                    &format!("custom_files/{hash}"),
                    data.freeze(),
                )
                .await?;

            uploaded_files.push(UploadedFile { install_path, hash });
            Ok(())
        }
        .await;

        if result.is_err() {
            error = result.err();
        }
    }

    if let Some(error) = error {
        return Err(error);
    }

    let mut transaction = pool.begin().await?;

    let (ids, hashes, install_paths): (Vec<_>, Vec<_>, Vec<_>) = uploaded_files
        .into_iter()
        .map(|f| (profile_item.id.0, f.hash, f.install_path))
        .multiunzip();

    sqlx::query!(
        "
            INSERT INTO shared_profiles_mods (shared_profile_id, file_hash, install_path)
            SELECT * FROM UNNEST($1::bigint[], $2::text[], $3::text[])
            ",
        &ids[..],
        &hashes[..],
        &install_paths[..],
    )
    .execute(&mut *transaction)
    .await?;

    transaction.commit().await?;

    database::models::minecraft_profile_item::MinecraftProfile::clear_cache(
        profile_item.id,
        &redis,
    )
    .await?;

    Ok(HttpResponse::NoContent().body(""))
}
