use crate::auth::email::send_email;
use crate::auth::validate::get_user_record_from_bearer_token;
use crate::auth::{get_user_from_headers, AuthProvider, AuthenticationError};
use crate::database::models::flow_item::Flow;
use crate::database::redis::RedisPool;
use crate::file_hosting::FileHost;
use crate::models::ids::base62_impl::{parse_base62, to_base62};
use crate::models::ids::random_base62_rng;
use crate::models::pats::Scopes;
use crate::models::users::{Badges, Role};
use crate::queue::session::AuthQueue;
use crate::queue::socket::ActiveSockets;
use crate::routes::internal::pats::NewPersonalAccessToken;
use crate::routes::internal::session::{delete_session, issue_session, list, refresh};
use crate::routes::ApiError;
use crate::util::captcha::check_turnstile_captcha;
use crate::util::env::parse_strings_from_var;
use crate::util::ext::{get_image_content_type, get_image_ext};
use crate::util::validate::{validation_errors_to_string, RE_URL_SAFE};
use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{ConnectInfo, Query, WebSocketUpgrade};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect};
use axum::routing::{delete, get, patch, post};
use axum::{Extension, Json, Router};
use base64::Engine;
use chrono::{Duration, Utc};
use hyper::StatusCode;
use rand_chacha::rand_core::SeedableRng;
use rand_chacha::ChaCha20Rng;
use reqwest::header::AUTHORIZATION;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPool;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use validator::Validate;

pub fn config() -> Router {
    Router::new().nest(
        "/auth",
        Router::new()
            .route("/ws", get(ws_init))
            .route("/init", get(init))
            .route("/callback", get(auth_callback))
            // above is todo
            .route("/provider", delete(delete_auth_provider))
            .route("/create", post(create_account_with_password))
            .route("/login", post(login_password))
            .route("/login/2fa", post(login_2fa))
            .route("/2fa/get_secret", post(begin_2fa_flow))
            .route("/2fa", post(finish_2fa_flow).delete(remove_2fa))
            .route("/password/reset", post(reset_password_begin))
            .route("/password", patch(change_password))
            .route("/email/resend_verify", post(resend_verify_email))
            .route("/email", patch(set_email))
            .route("/email/verify", post(verify_email))
            .route("email/subscribe", post(subscribe_newsletter)),
    )
}

#[derive(Debug)]
pub struct TempUser {
    pub id: String,
    pub username: String,
    pub email: Option<String>,

    pub avatar_url: Option<String>,
    pub bio: Option<String>,
    pub name: Option<String>,

    pub country: Option<String>,
}

impl TempUser {
    async fn create_account(
        self,
        provider: AuthProvider,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        client: &PgPool,
        file_host: &Arc<dyn FileHost + Send + Sync>,
        redis: &RedisPool,
    ) -> Result<crate::database::models::UserId, AuthenticationError> {
        if let Some(email) = &self.email {
            if crate::database::models::User::get_email(email, client)
                .await?
                .is_some()
            {
                return Err(AuthenticationError::DuplicateUser);
            }
        }

        let user_id = crate::database::models::generate_user_id(transaction).await?;

        let mut username_increment: i32 = 0;
        let mut username = None;

        while username.is_none() {
            let test_username = format!(
                "{}{}",
                self.username,
                if username_increment > 0 {
                    username_increment.to_string()
                } else {
                    "".to_string()
                }
            );

            let new_id = crate::database::models::User::get(&test_username, client, redis).await?;

            if new_id.is_none() {
                username = Some(test_username);
            } else {
                username_increment += 1;
            }
        }

        let avatar_url = if let Some(avatar_url) = self.avatar_url {
            let cdn_url = dotenvy::var("CDN_URL")?;

            let res = reqwest::get(&avatar_url).await?;
            let headers = res.headers().clone();

            let img_data = if let Some(content_type) = headers
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|ct| ct.to_str().ok())
            {
                get_image_ext(content_type).map(|ext| (ext, content_type))
            } else if let Some(ext) = avatar_url.rsplit('.').next() {
                get_image_content_type(ext).map(|content_type| (ext, content_type))
            } else {
                None
            };

            if let Some((ext, content_type)) = img_data {
                let bytes = res.bytes().await?;
                let hash = sha1::Sha1::from(&bytes).hexdigest();

                let upload_data = file_host
                    .upload_file(
                        content_type,
                        &format!(
                            "user/{}/{}.{}",
                            crate::models::users::UserId::from(user_id),
                            hash,
                            ext
                        ),
                        bytes,
                    )
                    .await?;

                Some(format!("{}/{}", cdn_url, upload_data.file_name))
            } else {
                None
            }
        } else {
            None
        };

        if let Some(username) = username {
            crate::database::models::User {
                id: user_id,
                github_id: if provider == AuthProvider::GitHub {
                    Some(
                        self.id
                            .clone()
                            .parse()
                            .map_err(|_| AuthenticationError::InvalidCredentials)?,
                    )
                } else {
                    None
                },
                discord_id: if provider == AuthProvider::Discord {
                    Some(
                        self.id
                            .parse()
                            .map_err(|_| AuthenticationError::InvalidCredentials)?,
                    )
                } else {
                    None
                },
                gitlab_id: if provider == AuthProvider::GitLab {
                    Some(
                        self.id
                            .parse()
                            .map_err(|_| AuthenticationError::InvalidCredentials)?,
                    )
                } else {
                    None
                },
                google_id: if provider == AuthProvider::Google {
                    Some(self.id.clone())
                } else {
                    None
                },
                steam_id: if provider == AuthProvider::Steam {
                    Some(
                        self.id
                            .parse()
                            .map_err(|_| AuthenticationError::InvalidCredentials)?,
                    )
                } else {
                    None
                },
                microsoft_id: if provider == AuthProvider::Microsoft {
                    Some(self.id.clone())
                } else {
                    None
                },
                password: None,
                paypal_id: if provider == AuthProvider::PayPal {
                    Some(self.id)
                } else {
                    None
                },
                paypal_country: self.country,
                paypal_email: if provider == AuthProvider::PayPal {
                    self.email.clone()
                } else {
                    None
                },
                venmo_handle: None,
                totp_secret: None,
                username,
                name: self.name,
                email: self.email,
                email_verified: true,
                avatar_url,
                bio: self.bio,
                created: Utc::now(),
                role: Role::Developer.to_string(),
                badges: Badges::default(),
                balance: Decimal::ZERO,
            }
            .insert(transaction)
            .await?;

            Ok(user_id)
        } else {
            Err(AuthenticationError::InvalidCredentials)
        }
    }
}

impl AuthProvider {
    pub fn get_redirect_url(&self, state: String) -> Result<String, AuthenticationError> {
        let self_addr = dotenvy::var("SELF_ADDR")?;
        let raw_redirect_uri = format!("{}/v2/auth/callback", self_addr);
        let redirect_uri = urlencoding::encode(&raw_redirect_uri);

        Ok(match self {
            AuthProvider::GitHub => {
                let client_id = dotenvy::var("GITHUB_CLIENT_ID")?;

                format!(
                    "https://github.com/login/oauth/authorize?client_id={}&state={}&scope=read%3Auser%20user%3Aemail&redirect_uri={}",
                    client_id,
                    state,
                    redirect_uri,
                )
            }
            AuthProvider::Discord => {
                let client_id = dotenvy::var("DISCORD_CLIENT_ID")?;

                format!("https://discord.com/api/oauth2/authorize?client_id={}&state={}&response_type=code&scope=identify%20email&redirect_uri={}", client_id, state, redirect_uri)
            }
            AuthProvider::Microsoft => {
                let client_id = dotenvy::var("MICROSOFT_CLIENT_ID")?;

                format!("https://login.live.com/oauth20_authorize.srf?client_id={}&response_type=code&scope=user.read&state={}&prompt=select_account&redirect_uri={}", client_id, state, redirect_uri)
            }
            AuthProvider::GitLab => {
                let client_id = dotenvy::var("GITLAB_CLIENT_ID")?;

                format!(
                    "https://gitlab.com/oauth/authorize?client_id={}&state={}&scope=read_user+profile+email&response_type=code&redirect_uri={}",
                    client_id,
                    state,
                    redirect_uri,
                )
            }
            AuthProvider::Google => {
                let client_id = dotenvy::var("GOOGLE_CLIENT_ID")?;

                format!(
                    "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&state={}&scope={}&response_type=code&redirect_uri={}",
                    client_id,
                    state,
                    urlencoding::encode("https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile"),
                    redirect_uri,
                )
            }
            AuthProvider::Steam => {
                format!(
                    "https://steamcommunity.com/openid/login?openid.ns={}&openid.mode={}&openid.return_to={}{}{}&openid.realm={}&openid.identity={}&openid.claimed_id={}",
                    urlencoding::encode("http://specs.openid.net/auth/2.0"),
                    "checkid_setup",
                    redirect_uri, urlencoding::encode("?state="), state,
                    self_addr,
                    "http://specs.openid.net/auth/2.0/identifier_select",
                    "http://specs.openid.net/auth/2.0/identifier_select",
                )
            }
            AuthProvider::PayPal => {
                let api_url = dotenvy::var("PAYPAL_API_URL")?;
                let client_id = dotenvy::var("PAYPAL_CLIENT_ID")?;

                let auth_url = if api_url.contains("sandbox") {
                    "sandbox.paypal.com"
                } else {
                    "paypal.com"
                };

                format!(
                    "https://{auth_url}/connect?flowEntry=static&client_id={client_id}&scope={}&response_type=code&redirect_uri={redirect_uri}&state={state}",
                    urlencoding::encode("openid email address https://uri.paypal.com/services/paypalattributes"),
                )
            }
        })
    }

    pub async fn get_token(
        &self,
        query: HashMap<String, String>,
    ) -> Result<String, AuthenticationError> {
        let redirect_uri = format!("{}/v2/auth/callback", dotenvy::var("SELF_ADDR")?);

        #[derive(Deserialize)]
        struct AccessToken {
            pub access_token: String,
        }

        let res = match self {
            AuthProvider::GitHub => {
                let code = query
                    .get("code")
                    .ok_or_else(|| AuthenticationError::InvalidCredentials)?;
                let client_id = dotenvy::var("GITHUB_CLIENT_ID")?;
                let client_secret = dotenvy::var("GITHUB_CLIENT_SECRET")?;

                let url = format!(
                    "https://github.com/login/oauth/access_token?client_id={}&client_secret={}&code={}&redirect_uri={}",
                    client_id, client_secret, code, redirect_uri
                );

                let token: AccessToken = reqwest::Client::new()
                    .post(&url)
                    .header(reqwest::header::ACCEPT, "application/json")
                    .send()
                    .await?
                    .json()
                    .await?;

                token.access_token
            }
            AuthProvider::Discord => {
                let code = query
                    .get("code")
                    .ok_or_else(|| AuthenticationError::InvalidCredentials)?;
                let client_id = dotenvy::var("DISCORD_CLIENT_ID")?;
                let client_secret = dotenvy::var("DISCORD_CLIENT_SECRET")?;

                let mut map = HashMap::new();
                map.insert("client_id", &*client_id);
                map.insert("client_secret", &*client_secret);
                map.insert("code", code);
                map.insert("grant_type", "authorization_code");
                map.insert("redirect_uri", &redirect_uri);

                let token: AccessToken = reqwest::Client::new()
                    .post("https://discord.com/api/v10/oauth2/token")
                    .header(reqwest::header::ACCEPT, "application/json")
                    .form(&map)
                    .send()
                    .await?
                    .json()
                    .await?;

                token.access_token
            }
            AuthProvider::Microsoft => {
                let code = query
                    .get("code")
                    .ok_or_else(|| AuthenticationError::InvalidCredentials)?;
                let client_id = dotenvy::var("MICROSOFT_CLIENT_ID")?;
                let client_secret = dotenvy::var("MICROSOFT_CLIENT_SECRET")?;

                let mut map = HashMap::new();
                map.insert("client_id", &*client_id);
                map.insert("client_secret", &*client_secret);
                map.insert("code", code);
                map.insert("grant_type", "authorization_code");
                map.insert("redirect_uri", &redirect_uri);

                let token: AccessToken = reqwest::Client::new()
                    .post("https://login.live.com/oauth20_token.srf")
                    .header(reqwest::header::ACCEPT, "application/json")
                    .form(&map)
                    .send()
                    .await?
                    .json()
                    .await?;

                token.access_token
            }
            AuthProvider::GitLab => {
                let code = query
                    .get("code")
                    .ok_or_else(|| AuthenticationError::InvalidCredentials)?;
                let client_id = dotenvy::var("GITLAB_CLIENT_ID")?;
                let client_secret = dotenvy::var("GITLAB_CLIENT_SECRET")?;

                let mut map = HashMap::new();
                map.insert("client_id", &*client_id);
                map.insert("client_secret", &*client_secret);
                map.insert("code", code);
                map.insert("grant_type", "authorization_code");
                map.insert("redirect_uri", &redirect_uri);

                let token: AccessToken = reqwest::Client::new()
                    .post("https://gitlab.com/oauth/token")
                    .header(reqwest::header::ACCEPT, "application/json")
                    .form(&map)
                    .send()
                    .await?
                    .json()
                    .await?;

                token.access_token
            }
            AuthProvider::Google => {
                let code = query
                    .get("code")
                    .ok_or_else(|| AuthenticationError::InvalidCredentials)?;
                let client_id = dotenvy::var("GOOGLE_CLIENT_ID")?;
                let client_secret = dotenvy::var("GOOGLE_CLIENT_SECRET")?;

                let mut map = HashMap::new();
                map.insert("client_id", &*client_id);
                map.insert("client_secret", &*client_secret);
                map.insert("code", code);
                map.insert("grant_type", "authorization_code");
                map.insert("redirect_uri", &redirect_uri);

                let token: AccessToken = reqwest::Client::new()
                    .post("https://oauth2.googleapis.com/token")
                    .header(reqwest::header::ACCEPT, "application/json")
                    .form(&map)
                    .send()
                    .await?
                    .json()
                    .await?;

                token.access_token
            }
            AuthProvider::Steam => {
                let mut form = HashMap::new();

                let signed = query
                    .get("openid.signed")
                    .ok_or_else(|| AuthenticationError::InvalidCredentials)?;
                form.insert(
                    "openid.assoc_handle".to_string(),
                    &**query
                        .get("openid.assoc_handle")
                        .ok_or_else(|| AuthenticationError::InvalidCredentials)?,
                );
                form.insert("openid.signed".to_string(), &**signed);
                form.insert(
                    "openid.sig".to_string(),
                    &**query
                        .get("openid.sig")
                        .ok_or_else(|| AuthenticationError::InvalidCredentials)?,
                );
                form.insert("openid.ns".to_string(), "http://specs.openid.net/auth/2.0");
                form.insert("openid.mode".to_string(), "check_authentication");

                for val in signed.split(',') {
                    if let Some(arr_val) = query.get(&format!("openid.{}", val)) {
                        form.insert(format!("openid.{}", val), &**arr_val);
                    }
                }

                let res = reqwest::Client::new()
                    .post("https://steamcommunity.com/openid/login")
                    .header("Accept-language", "en")
                    .form(&form)
                    .send()
                    .await?
                    .text()
                    .await?;

                if res.contains("is_valid:true") {
                    let identity = query
                        .get("openid.identity")
                        .ok_or_else(|| AuthenticationError::InvalidCredentials)?;

                    identity
                        .rsplit('/')
                        .next()
                        .ok_or_else(|| AuthenticationError::InvalidCredentials)?
                        .to_string()
                } else {
                    return Err(AuthenticationError::InvalidCredentials);
                }
            }
            AuthProvider::PayPal => {
                let code = query
                    .get("code")
                    .ok_or_else(|| AuthenticationError::InvalidCredentials)?;
                let api_url = dotenvy::var("PAYPAL_API_URL")?;
                let client_id = dotenvy::var("PAYPAL_CLIENT_ID")?;
                let client_secret = dotenvy::var("PAYPAL_CLIENT_SECRET")?;

                let mut map = HashMap::new();
                map.insert("code", code.as_str());
                map.insert("grant_type", "authorization_code");

                let token: AccessToken = reqwest::Client::new()
                    .post(&format!("{api_url}oauth2/token"))
                    .header(reqwest::header::ACCEPT, "application/json")
                    .header(
                        AUTHORIZATION,
                        format!(
                            "Basic {}",
                            base64::engine::general_purpose::STANDARD
                                .encode(format!("{client_id}:{client_secret}"))
                        ),
                    )
                    .form(&map)
                    .send()
                    .await?
                    .json()
                    .await?;

                token.access_token
            }
        };

        Ok(res)
    }

    pub async fn get_user(&self, token: &str) -> Result<TempUser, AuthenticationError> {
        let res = match self {
            AuthProvider::GitHub => {
                let response = reqwest::Client::new()
                    .get("https://api.github.com/user")
                    .header(reqwest::header::USER_AGENT, "Modrinth")
                    .header(AUTHORIZATION, format!("token {token}"))
                    .send()
                    .await?;

                if token.starts_with("gho_") {
                    let client_id = response
                        .headers()
                        .get("x-oauth-client-id")
                        .and_then(|x| x.to_str().ok());

                    if client_id != Some(&*dotenvy::var("GITHUB_CLIENT_ID").unwrap()) {
                        return Err(AuthenticationError::InvalidClientId);
                    }
                }

                #[derive(Serialize, Deserialize, Debug)]
                pub struct GitHubUser {
                    pub login: String,
                    pub id: u64,
                    pub avatar_url: String,
                    pub name: Option<String>,
                    pub email: Option<String>,
                    pub bio: Option<String>,
                }

                let github_user: GitHubUser = response.json().await?;

                TempUser {
                    id: github_user.id.to_string(),
                    username: github_user.login,
                    email: github_user.email,
                    avatar_url: Some(github_user.avatar_url),
                    bio: github_user.bio,
                    name: github_user.name,
                    country: None,
                }
            }
            AuthProvider::Discord => {
                #[derive(Serialize, Deserialize, Debug)]
                pub struct DiscordUser {
                    pub username: String,
                    pub id: String,
                    pub avatar: Option<String>,
                    pub global_name: Option<String>,
                    pub email: Option<String>,
                }

                let discord_user: DiscordUser = reqwest::Client::new()
                    .get("https://discord.com/api/v10/users/@me")
                    .header(reqwest::header::USER_AGENT, "Modrinth")
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .send()
                    .await?
                    .json()
                    .await?;

                let id = discord_user.id.clone();
                TempUser {
                    id: discord_user.id,
                    username: discord_user.username,
                    email: discord_user.email,
                    avatar_url: discord_user
                        .avatar
                        .map(|x| format!("https://cdn.discordapp.com/avatars/{}/{}.webp", id, x)),
                    bio: None,
                    name: discord_user.global_name,
                    country: None,
                }
            }
            AuthProvider::Microsoft => {
                #[derive(Deserialize, Debug)]
                #[serde(rename_all = "camelCase")]
                pub struct MicrosoftUser {
                    pub id: String,
                    pub display_name: Option<String>,
                    pub mail: Option<String>,
                    pub user_principal_name: String,
                }

                let microsoft_user: MicrosoftUser = reqwest::Client::new()
                    .get("https://graph.microsoft.com/v1.0/me?$select=id,displayName,mail,userPrincipalName")
                    .header(reqwest::header::USER_AGENT, "Modrinth")
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .send()
                    .await?.json().await?;

                TempUser {
                    id: microsoft_user.id,
                    username: microsoft_user
                        .user_principal_name
                        .split('@')
                        .next()
                        .unwrap_or_default()
                        .to_string(),
                    email: microsoft_user.mail,
                    avatar_url: None,
                    bio: None,
                    name: microsoft_user.display_name,
                    country: None,
                }
            }
            AuthProvider::GitLab => {
                #[derive(Serialize, Deserialize, Debug)]
                pub struct GitLabUser {
                    pub id: i32,
                    pub username: String,
                    pub email: Option<String>,
                    pub avatar_url: Option<String>,
                    pub name: Option<String>,
                    pub bio: Option<String>,
                }

                let gitlab_user: GitLabUser = reqwest::Client::new()
                    .get("https://gitlab.com/api/v4/user")
                    .header(reqwest::header::USER_AGENT, "Modrinth")
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .send()
                    .await?
                    .json()
                    .await?;

                TempUser {
                    id: gitlab_user.id.to_string(),
                    username: gitlab_user.username,
                    email: gitlab_user.email,
                    avatar_url: gitlab_user.avatar_url,
                    bio: gitlab_user.bio,
                    name: gitlab_user.name,
                    country: None,
                }
            }
            AuthProvider::Google => {
                #[derive(Deserialize, Debug)]
                pub struct GoogleUser {
                    pub id: String,
                    pub email: String,
                    pub name: Option<String>,
                    pub bio: Option<String>,
                    pub picture: Option<String>,
                }

                let google_user: GoogleUser = reqwest::Client::new()
                    .get("https://www.googleapis.com/userinfo/v2/me")
                    .header(reqwest::header::USER_AGENT, "Modrinth")
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .send()
                    .await?
                    .json()
                    .await?;

                TempUser {
                    id: google_user.id,
                    username: google_user
                        .email
                        .split('@')
                        .next()
                        .unwrap_or_default()
                        .to_string(),
                    email: Some(google_user.email),
                    avatar_url: google_user.picture,
                    bio: None,
                    name: google_user.name,
                    country: None,
                }
            }
            AuthProvider::Steam => {
                let api_key = dotenvy::var("STEAM_API_KEY")?;

                #[derive(Deserialize)]
                struct SteamResponse {
                    response: Players,
                }

                #[derive(Deserialize)]
                struct Players {
                    players: Vec<Player>,
                }

                #[derive(Deserialize)]
                struct Player {
                    steamid: String,
                    personaname: String,
                    profileurl: String,
                    avatar: Option<String>,
                }

                let response: String = reqwest::get(
                    &format!(
                        "https://api.steampowered.com/ISteamUser/GetPlayerSummaries/v0002/?key={}&steamids={}",
                        api_key,
                        token
                    )
                )
                    .await?
                    .text()
                    .await?;

                let mut response: SteamResponse = serde_json::from_str(&response)?;

                if let Some(player) = response.response.players.pop() {
                    let username = player
                        .profileurl
                        .trim_matches('/')
                        .rsplit('/')
                        .next()
                        .unwrap_or(&player.steamid)
                        .to_string();
                    TempUser {
                        id: player.steamid,
                        username,
                        email: None,
                        avatar_url: player.avatar,
                        bio: None,
                        name: Some(player.personaname),
                        country: None,
                    }
                } else {
                    return Err(AuthenticationError::InvalidCredentials);
                }
            }
            AuthProvider::PayPal => {
                #[derive(Deserialize, Debug)]
                pub struct PayPalUser {
                    pub payer_id: String,
                    pub email: String,
                    pub picture: Option<String>,
                    pub address: PayPalAddress,
                }

                #[derive(Deserialize, Debug)]
                pub struct PayPalAddress {
                    pub country: String,
                }

                let api_url = dotenvy::var("PAYPAL_API_URL")?;

                let paypal_user: PayPalUser = reqwest::Client::new()
                    .get(&format!(
                        "{api_url}identity/openidconnect/userinfo?schema=openid"
                    ))
                    .header(reqwest::header::USER_AGENT, "Modrinth")
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .send()
                    .await?
                    .json()
                    .await?;

                TempUser {
                    id: paypal_user.payer_id,
                    username: paypal_user
                        .email
                        .split('@')
                        .next()
                        .unwrap_or_default()
                        .to_string(),
                    email: Some(paypal_user.email),
                    avatar_url: paypal_user.picture,
                    bio: None,
                    name: None,
                    country: Some(paypal_user.address.country),
                }
            }
        };

        Ok(res)
    }

    pub async fn get_user_id<'a, 'b, E>(
        &self,
        id: &str,
        executor: E,
    ) -> Result<Option<crate::database::models::UserId>, AuthenticationError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        Ok(match self {
            AuthProvider::GitHub => {
                let value = sqlx::query!(
                    "SELECT id FROM users WHERE github_id = $1",
                    id.parse::<i64>()
                        .map_err(|_| AuthenticationError::InvalidCredentials)?
                )
                .fetch_optional(executor)
                .await?;

                value.map(|x| crate::database::models::UserId(x.id))
            }
            AuthProvider::Discord => {
                let value = sqlx::query!(
                    "SELECT id FROM users WHERE discord_id = $1",
                    id.parse::<i64>()
                        .map_err(|_| AuthenticationError::InvalidCredentials)?
                )
                .fetch_optional(executor)
                .await?;

                value.map(|x| crate::database::models::UserId(x.id))
            }
            AuthProvider::Microsoft => {
                let value = sqlx::query!("SELECT id FROM users WHERE microsoft_id = $1", id)
                    .fetch_optional(executor)
                    .await?;

                value.map(|x| crate::database::models::UserId(x.id))
            }
            AuthProvider::GitLab => {
                let value = sqlx::query!(
                    "SELECT id FROM users WHERE gitlab_id = $1",
                    id.parse::<i64>()
                        .map_err(|_| AuthenticationError::InvalidCredentials)?
                )
                .fetch_optional(executor)
                .await?;

                value.map(|x| crate::database::models::UserId(x.id))
            }
            AuthProvider::Google => {
                let value = sqlx::query!("SELECT id FROM users WHERE google_id = $1", id)
                    .fetch_optional(executor)
                    .await?;

                value.map(|x| crate::database::models::UserId(x.id))
            }
            AuthProvider::Steam => {
                let value = sqlx::query!(
                    "SELECT id FROM users WHERE steam_id = $1",
                    id.parse::<i64>()
                        .map_err(|_| AuthenticationError::InvalidCredentials)?
                )
                .fetch_optional(executor)
                .await?;

                value.map(|x| crate::database::models::UserId(x.id))
            }
            AuthProvider::PayPal => {
                let value = sqlx::query!("SELECT id FROM users WHERE paypal_id = $1", id)
                    .fetch_optional(executor)
                    .await?;

                value.map(|x| crate::database::models::UserId(x.id))
            }
        })
    }

    pub async fn update_user_id(
        &self,
        user_id: crate::database::models::UserId,
        id: Option<&str>,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), AuthenticationError> {
        match self {
            AuthProvider::GitHub => {
                sqlx::query!(
                    "
                    UPDATE users
                    SET github_id = $2
                    WHERE (id = $1)
                    ",
                    user_id as crate::database::models::UserId,
                    id.and_then(|x| x.parse::<i64>().ok())
                )
                .execute(&mut **transaction)
                .await?;
            }
            AuthProvider::Discord => {
                sqlx::query!(
                    "
                    UPDATE users
                    SET discord_id = $2
                    WHERE (id = $1)
                    ",
                    user_id as crate::database::models::UserId,
                    id.and_then(|x| x.parse::<i64>().ok())
                )
                .execute(&mut **transaction)
                .await?;
            }
            AuthProvider::Microsoft => {
                sqlx::query!(
                    "
                    UPDATE users
                    SET microsoft_id = $2
                    WHERE (id = $1)
                    ",
                    user_id as crate::database::models::UserId,
                    id,
                )
                .execute(&mut **transaction)
                .await?;
            }
            AuthProvider::GitLab => {
                sqlx::query!(
                    "
                    UPDATE users
                    SET gitlab_id = $2
                    WHERE (id = $1)
                    ",
                    user_id as crate::database::models::UserId,
                    id.and_then(|x| x.parse::<i64>().ok())
                )
                .execute(&mut **transaction)
                .await?;
            }
            AuthProvider::Google => {
                sqlx::query!(
                    "
                    UPDATE users
                    SET google_id = $2
                    WHERE (id = $1)
                    ",
                    user_id as crate::database::models::UserId,
                    id,
                )
                .execute(&mut **transaction)
                .await?;
            }
            AuthProvider::Steam => {
                sqlx::query!(
                    "
                    UPDATE users
                    SET steam_id = $2
                    WHERE (id = $1)
                    ",
                    user_id as crate::database::models::UserId,
                    id.and_then(|x| x.parse::<i64>().ok())
                )
                .execute(&mut **transaction)
                .await?;
            }
            AuthProvider::PayPal => {
                if id.is_none() {
                    sqlx::query!(
                        "
                        UPDATE users
                        SET paypal_country = NULL, paypal_email = NULL, paypal_id = NULL
                        WHERE (id = $1)
                        ",
                        user_id as crate::database::models::UserId,
                    )
                    .execute(&mut **transaction)
                    .await?;
                } else {
                    sqlx::query!(
                        "
                        UPDATE users
                        SET paypal_id = $2
                        WHERE (id = $1)
                        ",
                        user_id as crate::database::models::UserId,
                        id,
                    )
                    .execute(&mut **transaction)
                    .await?;
                }
            }
        }

        Ok(())
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            AuthProvider::GitHub => "GitHub",
            AuthProvider::Discord => "Discord",
            AuthProvider::Microsoft => "Microsoft",
            AuthProvider::GitLab => "GitLab",
            AuthProvider::Google => "Google",
            AuthProvider::Steam => "Steam",
            AuthProvider::PayPal => "PayPal",
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct AuthorizationInit {
    pub url: String,
    #[serde(default)]
    pub provider: AuthProvider,
    pub token: Option<String>,
}
#[derive(Serialize, Deserialize)]
pub struct Authorization {
    pub code: String,
    pub state: String,
}

// Init link takes us to GitHub API and calls back to callback endpoint with a code and state
// http://localhost:8000/auth/init?url=https://modrinth.com
pub async fn init(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(info): Query<AuthorizationInit>, // callback url
    Extension(client): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<(Redirect, Json<serde_json::Value>), AuthenticationError> {
    let url = url::Url::parse(&info.url).map_err(|_| AuthenticationError::Url)?;

    let allowed_callback_urls = parse_strings_from_var("ALLOWED_CALLBACK_URLS").unwrap_or_default();
    let domain = url.host_str().ok_or(AuthenticationError::Url)?;
    if !allowed_callback_urls.iter().any(|x| domain.ends_with(x)) && domain != "modrinth.com" {
        return Err(AuthenticationError::Url);
    }

    let user_id = if let Some(token) = info.token {
        let (_, user) = get_user_record_from_bearer_token(
            &addr,
            &headers,
            Some(&token),
            &**client,
            &redis,
            &session_queue,
        )
        .await?
        .ok_or_else(|| AuthenticationError::InvalidCredentials)?;

        Some(user.id)
    } else {
        None
    };

    let state = Flow::OAuth {
        user_id,
        url: Some(info.url),
        provider: info.provider,
    }
    .insert(Duration::minutes(30), &redis)
    .await?;

    let url = info.provider.get_redirect_url(state)?;

    Ok((
        Redirect::temporary(&*url),
        Json(serde_json::json!({ "url": url })),
    ))
}

#[derive(Serialize, Deserialize)]
pub struct WsInit {
    pub provider: AuthProvider,
}

pub async fn ws_init(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(info): Query<WsInit>,
    ws: WebSocketUpgrade,
    Extension(active_sockets): Extension<RwLock<ActiveSockets>>,
    Extension(redis): Extension<RedisPool>,
) -> impl IntoResponse {
    async fn sock(
        mut socket: WebSocket,
        who: SocketAddr,
        provider: AuthProvider,
        active_sockets: RwLock<ActiveSockets>,
        redis: &RedisPool,
    ) -> Result<(), ApiError> {
        let flow = Flow::OAuth {
            user_id: None,
            url: None,
            provider,
        }
        .insert(Duration::minutes(30), &redis)
        .await?;

        if let Ok(state) = flow {
            if let Ok(url) = provider.get_redirect_url(state.clone()) {
                socket
                    .send(Message::Text(serde_json::json!({ "url": url }).to_string()))
                    .await?;

                let db = active_sockets.write().await;
                db.auth_sockets.insert(state, socket);
            }
        }

        Ok(())
    }

    ws.on_upgrade(move |socket| sock(socket, addr, info.provider, active_sockets, &redis))
}

pub async fn auth_callback(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
    Extension(active_sockets): Extension<RwLock<ActiveSockets>>,
    Extension(client): Extension<PgPool>,
    Extension(file_host): Extension<Arc<dyn FileHost + Send + Sync>>,
    Extension(redis): Extension<RedisPool>,
) -> Result<impl IntoResponse, crate::auth::templates::ErrorPage> {
    let state_string = query
        .get("state")
        .ok_or_else(|| AuthenticationError::InvalidCredentials)?
        .clone();

    let state = state_string.clone();
    let res: Result<impl IntoResponse, AuthenticationError> = async move {

        let flow = Flow::get(&state, &redis).await?;

        // Extract cookie header from request
        if let Some(Flow::OAuth {
                        user_id,
                        provider,
                        url,
                    }) = flow
        {
            Flow::remove(&state, &redis).await?;

            let token = provider.get_token(query).await?;
            let oauth_user = provider.get_user(&token).await?;

            let user_id_opt = provider.get_user_id(&oauth_user.id, &**client).await?;

            let mut transaction = client.begin().await?;
            if let Some(id) = user_id {
                if user_id_opt.is_some() {
                    return Err(AuthenticationError::DuplicateUser);
                }

                provider
                    .update_user_id(id, Some(&oauth_user.id), &mut transaction)
                    .await?;

                let user = crate::database::models::User::get_id(id, &**client, &redis).await?;

                if provider == AuthProvider::PayPal  {
                    sqlx::query!(
                        "
                        UPDATE users
                        SET paypal_country = $1, paypal_email = $2, paypal_id = $3
                        WHERE (id = $4)
                        ",
                        oauth_user.country,
                        oauth_user.email,
                        oauth_user.id,
                        id as crate::database::models::ids::UserId,
                    )
                        .execute(&mut *transaction)
                        .await?;
                } else if let Some(email) = user.and_then(|x| x.email) {
                    send_email(
                        email,
                        "Authentication method added",
                        &format!("When logging into Modrinth, you can now log in using the {} authentication provider.", provider.as_str()),
                        "If you did not make this change, please contact us immediately through our support channels on Discord or via email (support@modrinth.com).",
                        None,
                    )?;
                }

                transaction.commit().await?;
                crate::database::models::User::clear_caches(&[(id, None)], &redis).await?;

                if let Some(url) = url {
                    Ok((Redirect::temporary(&*url), Json(serde_json::json!({ "url": url }))))
                } else {
                    Err(AuthenticationError::InvalidCredentials)
                }
            } else {
                let user_id = if let Some(user_id) = user_id_opt {
                    let user = crate::database::models::User::get_id(user_id, &**client, &redis)
                        .await?
                        .ok_or_else(|| AuthenticationError::InvalidCredentials)?;

                    if user.totp_secret.is_some() {
                        let flow = Flow::Login2FA { user_id: user.id }
                            .insert(Duration::minutes(30), &redis)
                            .await?;

                        if let Some(url) = url {
                            let redirect_url = format!(
                                "{}{}error=2fa_required&flow={}",
                                url,
                                if url.contains('?') { "&" } else { "?" },
                                flow
                            );

                            Ok((Redirect::temporary(&*redirect_url), Json(serde_json::json!({ "url": redirect_url }))))
                        } else {
                            let mut ws_conn = {
                                let db = active_sockets.read().await;

                                let mut x = db
                                    .auth_sockets
                                    .get_mut(&state)
                                    .ok_or_else(|| AuthenticationError::SocketError)?;

                                x.value_mut().clone()
                            };

                            ws_conn
                                .text(
                                    serde_json::json!({
                                        "error": "2fa_required",
                                        "flow": flow,
                                    }).to_string()
                                )
                                .await.map_err(|_| AuthenticationError::SocketError)?;

                            let _ = ws_conn.close(None).await;

                            return Ok(crate::auth::templates::Success {
                                icon: user.avatar_url.as_deref().unwrap_or("https://cdn-raw.modrinth.com/placeholder.svg"),
                                name: &user.username,
                            }.render());
                        }
                    }

                    user_id
                } else {
                    oauth_user.create_account(provider, &mut transaction, &client, &file_host, &redis).await?
                };

                let session = issue_session(&addr, &headers, user_id, &mut transaction, &redis).await?;
                transaction.commit().await?;

                if let Some(url) = url {
                    let redirect_url = format!(
                        "{}{}code={}{}",
                        url,
                        if url.contains('?') { '&' } else { '?' },
                        session.session,
                        if user_id_opt.is_none() {
                            "&new_account=true"
                        } else {
                            ""
                        }
                    );

                    Ok((Redirect::temporary(&*redirect_url), Json(serde_json::json!({ "url": redirect_url }))))
                } else {
                    let user = crate::database::models::user_item::User::get_id(
                        user_id,
                        &**client,
                        &redis,
                    )
                        .await?.ok_or_else(|| AuthenticationError::InvalidCredentials)?;

                    let mut ws_conn = {
                        let db = active_sockets.read().await;

                        let mut x = db
                            .auth_sockets
                            .get_mut(&state)
                            .ok_or_else(|| AuthenticationError::SocketError)?;

                        x.value_mut()
                    };

                    ws_conn
                        .send(
                            Message::Text(
                                serde_json::json!({
                                        "code": session.session,
                                    }).to_string()
                            )

                        )
                        .await.map_err(|_| AuthenticationError::SocketError)?;
                    let _ = ws_conn.close().await;

                    return Ok(crate::auth::templates::Success {
                        icon: user.avatar_url.as_deref().unwrap_or("https://cdn-raw.modrinth.com/placeholder.svg"),
                        name: &user.username,
                    }.render());
                }
            }
        } else {
            Err::<impl IntoResponse, AuthenticationError>(AuthenticationError::InvalidCredentials)
        }
    }.await;

    // Because this is callback route, if we have an error, we need to ensure we close the original socket if it exists
    if let Err(ref e) = res {
        let db = active_sockets.read().await;
        let mut x = db.auth_sockets.get_mut(&state_string);

        if let Some(x) = x.as_mut() {
            let mut ws_conn = x.value_mut();

            ws_conn
                .send(Message::Text(
                    serde_json::json!({
                            "error": &e.error_name(),
                            "description": &e.to_string(),
                        }
                    )
                    .to_string(),
                ))
                .await
                .map_err(|_| AuthenticationError::SocketError)?;
            let _ = ws_conn.close().await;
        }
    }

    Ok(res?)
}

#[derive(Deserialize)]
pub struct DeleteAuthProvider {
    pub provider: AuthProvider,
}

pub async fn delete_auth_provider(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Json(delete_provider): Json<DeleteAuthProvider>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<StatusCode, ApiError> {
    let user = get_user_from_headers(
        &addr,
        &headers,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::USER_AUTH_WRITE]),
    )
    .await?
    .1;

    if !user.auth_providers.map(|x| x.len() > 1).unwrap_or(false)
        && !user.has_password.unwrap_or(false)
    {
        return Err(ApiError::InvalidInput(
            "You must have another authentication method added to this account!".to_string(),
        ));
    }

    let mut transaction = pool.begin().await?;

    delete_provider
        .provider
        .update_user_id(user.id.into(), None, &mut transaction)
        .await?;

    if delete_provider.provider != AuthProvider::PayPal {
        if let Some(email) = user.email {
            send_email(
                email,
                "Authentication method removed",
                &format!("When logging into Modrinth, you can no longer log in using the {} authentication provider.", delete_provider.provider.as_str()),
                "If you did not make this change, please contact us immediately through our support channels on Discord or via email (support@modrinth.com).",
                None,
            )?;
        }
    }

    transaction.commit().await?;
    crate::database::models::User::clear_caches(&[(user.id.into(), None)], &redis).await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn sign_up_beehiiv(email: &str) -> Result<(), AuthenticationError> {
    let id = dotenvy::var("BEEHIIV_PUBLICATION_ID")?;
    let api_key = dotenvy::var("BEEHIIV_API_KEY")?;
    let site_url = dotenvy::var("SITE_URL")?;

    let client = reqwest::Client::new();
    client
        .post(&format!(
            "https://api.beehiiv.com/v2/publications/{id}/subscriptions"
        ))
        .header(AUTHORIZATION, format!("Bearer {}", api_key))
        .json(&serde_json::json!({
            "email": email,
            "utm_source": "modrinth",
            "utm_medium": "account_creation",
            "referring_site": site_url,
        }))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    Ok(())
}

#[derive(Deserialize, Validate)]
pub struct NewAccount {
    #[validate(length(min = 1, max = 39), regex = "RE_URL_SAFE")]
    pub username: String,
    #[validate(length(min = 8, max = 256))]
    pub password: String,
    #[validate(email)]
    pub email: String,
    pub challenge: String,
    pub sign_up_newsletter: Option<bool>,
}

pub async fn create_account_with_password(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Json(new_account): Json<NewAccount>,
) -> Result<Json<crate::models::sessions::Session>, ApiError> {
    new_account
        .validate()
        .map_err(|err| ApiError::InvalidInput(validation_errors_to_string(err, None)))?;

    if !check_turnstile_captcha(&addr, &headers, &new_account.challenge).await? {
        return Err(ApiError::Turnstile);
    }

    if crate::database::models::User::get(&new_account.username, &**pool, &redis)
        .await?
        .is_some()
    {
        return Err(ApiError::InvalidInput("Username is taken!".to_string()));
    }

    let mut transaction = pool.begin().await?;
    let user_id = crate::database::models::generate_user_id(&mut transaction).await?;

    let score = zxcvbn::zxcvbn(
        &new_account.password,
        &[&new_account.username, &new_account.email],
    )?;

    if score.score() < 3 {
        return Err(ApiError::InvalidInput(
            if let Some(feedback) = score.feedback().clone().and_then(|x| x.warning()) {
                format!("Password too weak: {}", feedback)
            } else {
                "Specified password is too weak! Please improve its strength.".to_string()
            },
        ));
    }

    let hasher = Argon2::default();
    let salt = SaltString::generate(&mut ChaCha20Rng::from_entropy());
    let password_hash = hasher
        .hash_password(new_account.password.as_bytes(), &salt)?
        .to_string();

    if crate::database::models::User::get_email(&new_account.email, &**pool)
        .await?
        .is_some()
    {
        return Err(ApiError::InvalidInput(
            "Email is already registered on Modrinth!".to_string(),
        ));
    }

    let flow = Flow::ConfirmEmail {
        user_id,
        confirm_email: new_account.email.clone(),
    }
    .insert(Duration::hours(24), &redis)
    .await?;

    send_email_verify(
        new_account.email.clone(),
        flow,
        &format!("Welcome to Modrinth, {}!", new_account.username),
    )?;

    crate::database::models::User {
        id: user_id,
        github_id: None,
        discord_id: None,
        gitlab_id: None,
        google_id: None,
        steam_id: None,
        microsoft_id: None,
        password: Some(password_hash),
        paypal_id: None,
        paypal_country: None,
        paypal_email: None,
        venmo_handle: None,
        totp_secret: None,
        username: new_account.username.clone(),
        name: Some(new_account.username),
        email: Some(new_account.email.clone()),
        email_verified: false,
        avatar_url: None,
        bio: None,
        created: Utc::now(),
        role: Role::Developer.to_string(),
        badges: Badges::default(),
        balance: Decimal::ZERO,
    }
    .insert(&mut transaction)
    .await?;

    let session = issue_session(&addr, &headers, user_id, &mut transaction, &redis).await?;
    let res = crate::models::sessions::Session::from(session, true, None);

    if new_account.sign_up_newsletter.unwrap_or(false) {
        sign_up_beehiiv(&new_account.email).await?;
    }

    transaction.commit().await?;

    Ok(Json(res))
}

#[derive(Deserialize, Validate)]
pub struct Login {
    pub username: String,
    pub password: String,
    pub challenge: String,
}

pub async fn login_password(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Json(login): Json<Login>,
) -> Result<impl IntoResponse, ApiError> {
    if !check_turnstile_captcha(&addr, &headers, &login.challenge).await? {
        return Err(ApiError::Turnstile);
    }

    let user = if let Some(user) =
        crate::database::models::User::get(&login.username, &**pool, &redis).await?
    {
        user
    } else {
        let user = crate::database::models::User::get_email(&login.username, &**pool)
            .await?
            .ok_or_else(|| AuthenticationError::InvalidCredentials)?;

        crate::database::models::User::get_id(user, &**pool, &redis)
            .await?
            .ok_or_else(|| AuthenticationError::InvalidCredentials)?
    };

    let hasher = Argon2::default();
    hasher
        .verify_password(
            login.password.as_bytes(),
            &PasswordHash::new(
                &user
                    .password
                    .ok_or_else(|| AuthenticationError::InvalidCredentials)?,
            )?,
        )
        .map_err(|_| AuthenticationError::InvalidCredentials)?;

    if user.totp_secret.is_some() {
        let flow = Flow::Login2FA { user_id: user.id }
            .insert(Duration::minutes(30), &redis)
            .await?;

        Ok(Json(serde_json::json!({
            "error": "2fa_required",
            "description": "2FA is required to complete this operation.",
            "flow": flow,
        })))
    } else {
        let mut transaction = pool.begin().await?;
        let session = issue_session(&addr, &headers, user.id, &mut transaction, &redis).await?;
        let res = crate::models::sessions::Session::from(session, true, None);
        transaction.commit().await?;

        Ok(Json(res))
    }
}

#[derive(Deserialize, Validate)]
pub struct Login2FA {
    pub code: String,
    pub flow: String,
}

async fn validate_2fa_code(
    input: String,
    secret: String,
    allow_backup: bool,
    user_id: crate::database::models::UserId,
    redis: &RedisPool,
    pool: &PgPool,
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<bool, AuthenticationError> {
    let totp = totp_rs::TOTP::new(
        totp_rs::Algorithm::SHA1,
        6,
        1,
        30,
        totp_rs::Secret::Encoded(secret)
            .to_bytes()
            .map_err(|_| AuthenticationError::InvalidCredentials)?,
    )
    .map_err(|_| AuthenticationError::InvalidCredentials)?;
    let token = totp
        .generate_current()
        .map_err(|_| AuthenticationError::InvalidCredentials)?;

    if input == token {
        Ok(true)
    } else if allow_backup {
        let backup_codes = crate::database::models::User::get_backup_codes(user_id, pool).await?;

        if !backup_codes.contains(&input) {
            Ok(false)
        } else {
            let code = parse_base62(&input).unwrap_or_default();

            sqlx::query!(
                "
                    DELETE FROM user_backup_codes
                    WHERE user_id = $1 AND code = $2
                    ",
                user_id as crate::database::models::ids::UserId,
                code as i64,
            )
            .execute(&mut **transaction)
            .await?;

            crate::database::models::User::clear_caches(&[(user_id, None)], redis).await?;

            Ok(true)
        }
    } else {
        Err(AuthenticationError::InvalidCredentials)
    }
}

pub async fn login_2fa(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Json(login): Json<Login2FA>,
) -> Result<Json<crate::models::sessions::Session>, ApiError> {
    let flow = Flow::get(&login.flow, &redis)
        .await?
        .ok_or_else(|| AuthenticationError::InvalidCredentials)?;

    if let Flow::Login2FA { user_id } = flow {
        let user = crate::database::models::User::get_id(user_id, &**pool, &redis)
            .await?
            .ok_or_else(|| AuthenticationError::InvalidCredentials)?;

        let mut transaction = pool.begin().await?;
        if !validate_2fa_code(
            login.code.clone(),
            user.totp_secret
                .ok_or_else(|| AuthenticationError::InvalidCredentials)?,
            true,
            user.id,
            &redis,
            &pool,
            &mut transaction,
        )
        .await?
        {
            return Err(ApiError::Authentication(
                AuthenticationError::InvalidCredentials,
            ));
        }
        Flow::remove(&login.flow, &redis).await?;

        let session = issue_session(&addr, &headers, user_id, &mut transaction, &redis).await?;
        let res = crate::models::sessions::Session::from(session, true, None);
        transaction.commit().await?;

        Ok(Json(res))
    } else {
        Err(ApiError::Authentication(
            AuthenticationError::InvalidCredentials,
        ))
    }
}

pub async fn begin_2fa_flow(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let user = get_user_from_headers(
        &addr,
        &headers,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::USER_AUTH_WRITE]),
    )
    .await?
    .1;

    if !user.has_totp.unwrap_or(false) {
        let string = totp_rs::Secret::generate_secret();
        let encoded = string.to_encoded();

        let flow = Flow::Initialize2FA {
            user_id: user.id.into(),
            secret: encoded.to_string(),
        }
        .insert(Duration::minutes(30), &redis)
        .await?;

        Ok(Json(serde_json::json!({
            "secret": encoded.to_string(),
            "flow": flow,
        })))
    } else {
        Err(ApiError::InvalidInput(
            "User already has 2FA enabled on their account!".to_string(),
        ))
    }
}

pub async fn finish_2fa_flow(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Json(login): Json<Login2FA>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let flow = Flow::get(&login.flow, &redis)
        .await?
        .ok_or_else(|| AuthenticationError::InvalidCredentials)?;

    if let Flow::Initialize2FA { user_id, secret } = flow {
        let user = get_user_from_headers(
            &addr,
            &headers,
            &**pool,
            &redis,
            &session_queue,
            Some(&[Scopes::USER_AUTH_WRITE]),
        )
        .await?
        .1;

        if user.id != user_id.into() {
            return Err(ApiError::Authentication(
                AuthenticationError::InvalidCredentials,
            ));
        }

        let mut transaction = pool.begin().await?;

        if !validate_2fa_code(
            login.code.clone(),
            secret.clone(),
            false,
            user.id.into(),
            &redis,
            &pool,
            &mut transaction,
        )
        .await?
        {
            return Err(ApiError::Authentication(
                AuthenticationError::InvalidCredentials,
            ));
        }

        Flow::remove(&login.flow, &redis).await?;

        sqlx::query!(
            "
            UPDATE users
            SET totp_secret = $1
            WHERE (id = $2)
            ",
            secret,
            user_id as crate::database::models::ids::UserId,
        )
        .execute(&mut *transaction)
        .await?;

        sqlx::query!(
            "
            DELETE FROM user_backup_codes
            WHERE user_id = $1
            ",
            user_id as crate::database::models::ids::UserId,
        )
        .execute(&mut *transaction)
        .await?;

        let mut codes = Vec::new();

        for _ in 0..6 {
            let mut rng = ChaCha20Rng::from_entropy();
            let val = random_base62_rng(&mut rng, 11);

            sqlx::query!(
                "
                INSERT INTO user_backup_codes (
                    user_id, code
                )
                VALUES (
                    $1, $2
                )
                ",
                user_id as crate::database::models::ids::UserId,
                val as i64,
            )
            .execute(&mut *transaction)
            .await?;

            codes.push(to_base62(val));
        }

        if let Some(email) = user.email {
            send_email(
                email,
                "Two-factor authentication enabled",
                "When logging into Modrinth, you can now enter a code generated by your authenticator app in addition to entering your usual email address and password.",
                "If you did not make this change, please contact us immediately through our support channels on Discord or via email (support@modrinth.com).",
                None,
            )?;
        }

        transaction.commit().await?;
        crate::database::models::User::clear_caches(&[(user.id.into(), None)], &redis).await?;

        Ok(Json(serde_json::json!({
            "backup_codes": codes,
        })))
    } else {
        Err(ApiError::Authentication(
            AuthenticationError::InvalidCredentials,
        ))
    }
}

#[derive(Deserialize)]
pub struct Remove2FA {
    pub code: String,
}

pub async fn remove_2fa(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Json(login): Json<Remove2FA>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<StatusCode, ApiError> {
    let (scopes, user) =
        get_user_record_from_bearer_token(&addr, &headers, None, &**pool, &redis, &session_queue)
            .await?
            .ok_or_else(|| AuthenticationError::InvalidCredentials)?;

    if !scopes.contains(Scopes::USER_AUTH_WRITE) {
        return Err(ApiError::Authentication(
            AuthenticationError::InvalidCredentials,
        ));
    }

    let mut transaction = pool.begin().await?;

    if !validate_2fa_code(
        login.code.clone(),
        user.totp_secret.ok_or_else(|| {
            ApiError::InvalidInput("User does not have 2FA enabled on the account!".to_string())
        })?,
        true,
        user.id,
        &redis,
        &pool,
        &mut transaction,
    )
    .await?
    {
        return Err(ApiError::Authentication(
            AuthenticationError::InvalidCredentials,
        ));
    }

    sqlx::query!(
        "
        UPDATE users
        SET totp_secret = NULL
        WHERE (id = $1)
        ",
        user.id as crate::database::models::ids::UserId,
    )
    .execute(&mut *transaction)
    .await?;

    sqlx::query!(
        "
        DELETE FROM user_backup_codes
        WHERE user_id = $1
        ",
        user.id as crate::database::models::ids::UserId,
    )
    .execute(&mut *transaction)
    .await?;

    if let Some(email) = user.email {
        send_email(
            email,
            "Two-factor authentication removed",
            "When logging into Modrinth, you no longer need two-factor authentication to gain access.",
            "If you did not make this change, please contact us immediately through our support channels on Discord or via email (support@modrinth.com).",
            None,
        )?;
    }

    transaction.commit().await?;
    crate::database::models::User::clear_caches(&[(user.id, None)], &redis).await?;

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct ResetPassword {
    pub username: String,
    pub challenge: String,
}

pub async fn reset_password_begin(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Json(reset_password): Json<ResetPassword>,
) -> Result<StatusCode, ApiError> {
    if !check_turnstile_captcha(&addr, &headers, &reset_password.challenge).await? {
        return Err(ApiError::Turnstile);
    }

    let user = if let Some(user_id) =
        crate::database::models::User::get_email(&reset_password.username, &**pool).await?
    {
        crate::database::models::User::get_id(user_id, &**pool, &redis).await?
    } else {
        crate::database::models::User::get(&reset_password.username, &**pool, &redis).await?
    };

    if let Some(user) = user {
        let flow = Flow::ForgotPassword { user_id: user.id }
            .insert(Duration::hours(24), &redis)
            .await?;

        if let Some(email) = user.email {
            send_email(
                email,
                "Reset your password",
                "Please visit the following link below to reset your password. If the button does not work, you can copy the link and paste it into your browser.",
                "If you did not request for your password to be reset, you can safely ignore this email.",
                Some(("Reset password", &format!("{}/{}?flow={}", dotenvy::var("SITE_URL")?,  dotenvy::var("SITE_RESET_PASSWORD_PATH")?, flow))),
            )?;
        }
    }

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, Validate)]
pub struct ChangePassword {
    pub flow: Option<String>,
    pub old_password: Option<String>,
    pub new_password: Option<String>,
}

pub async fn change_password(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Json(change_password): Json<ChangePassword>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<StatusCode, ApiError> {
    let user = if let Some(flow) = &change_password.flow {
        let flow = Flow::get(flow, &redis).await?;

        if let Some(Flow::ForgotPassword { user_id }) = flow {
            let user = crate::database::models::User::get_id(user_id, &**pool, &redis)
                .await?
                .ok_or_else(|| AuthenticationError::InvalidCredentials)?;

            Some(user)
        } else {
            None
        }
    } else {
        None
    };

    let user = if let Some(user) = user {
        user
    } else {
        let (scopes, user) = get_user_record_from_bearer_token(
            &addr,
            &headers,
            None,
            &**pool,
            &redis,
            &session_queue,
        )
        .await?
        .ok_or_else(|| AuthenticationError::InvalidCredentials)?;

        if !scopes.contains(Scopes::USER_AUTH_WRITE) {
            return Err(ApiError::Authentication(
                AuthenticationError::InvalidCredentials,
            ));
        }

        if let Some(pass) = user.password.as_ref() {
            let old_password = change_password.old_password.as_ref().ok_or_else(|| {
                ApiError::CustomAuthentication(
                    "You must specify the old password to change your password!".to_string(),
                )
            })?;

            let hasher = Argon2::default();
            hasher.verify_password(old_password.as_bytes(), &PasswordHash::new(pass)?)?;
        }

        user
    };

    let mut transaction = pool.begin().await?;

    let update_password = if let Some(new_password) = &change_password.new_password {
        let score = zxcvbn::zxcvbn(
            new_password,
            &[
                &user.username,
                &user.email.clone().unwrap_or_default(),
                &user.name.unwrap_or_default(),
            ],
        )?;

        if score.score() < 3 {
            return Err(ApiError::InvalidInput(
                if let Some(feedback) = score.feedback().clone().and_then(|x| x.warning()) {
                    format!("Password too weak: {}", feedback)
                } else {
                    "Specified password is too weak! Please improve its strength.".to_string()
                },
            ));
        }

        let hasher = Argon2::default();
        let salt = SaltString::generate(&mut ChaCha20Rng::from_entropy());
        let password_hash = hasher
            .hash_password(new_password.as_bytes(), &salt)?
            .to_string();

        Some(password_hash)
    } else {
        if !(user.github_id.is_some()
            || user.gitlab_id.is_some()
            || user.microsoft_id.is_some()
            || user.google_id.is_some()
            || user.steam_id.is_some()
            || user.discord_id.is_some())
        {
            return Err(ApiError::InvalidInput(
                "You must have another authentication method added to remove password authentication!".to_string(),
            ));
        }

        None
    };

    sqlx::query!(
        "
        UPDATE users
        SET password = $1
        WHERE (id = $2)
        ",
        update_password,
        user.id as crate::database::models::ids::UserId,
    )
    .execute(&mut *transaction)
    .await?;

    if let Some(flow) = &change_password.flow {
        Flow::remove(flow, &redis).await?;
    }

    if let Some(email) = user.email {
        let changed = if update_password.is_some() {
            "changed"
        } else {
            "removed"
        };

        send_email(
            email,
            &format!("Password {}", changed),
            &format!("Your password has been {} on your account.", changed),
            "If you did not make this change, please contact us immediately through our support channels on Discord or via email (support@modrinth.com).",
            None,
        )?;
    }

    transaction.commit().await?;
    crate::database::models::User::clear_caches(&[(user.id, None)], &redis).await?;

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, Validate)]
pub struct SetEmail {
    #[validate(email)]
    pub email: String,
}

pub async fn set_email(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Json(email): Json<SetEmail>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<StatusCode, ApiError> {
    email
        .validate()
        .map_err(|err| ApiError::InvalidInput(validation_errors_to_string(err, None)))?;

    let user = get_user_from_headers(
        &addr,
        &headers,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::USER_AUTH_WRITE]),
    )
    .await?
    .1;

    let mut transaction = pool.begin().await?;

    sqlx::query!(
        "
        UPDATE users
        SET email = $1, email_verified = FALSE
        WHERE (id = $2)
        ",
        email.email,
        user.id.0 as i64,
    )
    .execute(&mut *transaction)
    .await?;

    if let Some(user_email) = user.email {
        send_email(
            user_email,
            "Email changed",
            &format!("Your email has been updated to {} on your account.", email.email),
            "If you did not make this change, please contact us immediately through our support channels on Discord or via email (support@modrinth.com).",
            None,
        )?;
    }

    let flow = Flow::ConfirmEmail {
        user_id: user.id.into(),
        confirm_email: email.email.clone(),
    }
    .insert(Duration::hours(24), &redis)
    .await?;

    send_email_verify(
        email.email.clone(),
        flow,
        "We need to verify your email address.",
    )?;

    transaction.commit().await?;
    crate::database::models::User::clear_caches(&[(user.id.into(), None)], &redis).await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn resend_verify_email(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<StatusCode, ApiError> {
    let user = get_user_from_headers(
        &addr,
        &headers,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::USER_AUTH_WRITE]),
    )
    .await?
    .1;

    if let Some(email) = user.email {
        if user.email_verified.unwrap_or(false) {
            return Err(ApiError::InvalidInput(
                "User email is already verified!".to_string(),
            ));
        }

        let flow = Flow::ConfirmEmail {
            user_id: user.id.into(),
            confirm_email: email.clone(),
        }
        .insert(Duration::hours(24), &redis)
        .await?;

        send_email_verify(email, flow, "We need to verify your email address.")?;

        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::InvalidInput(
            "User does not have an email.".to_string(),
        ))
    }
}

#[derive(Deserialize)]
pub struct VerifyEmail {
    pub flow: String,
}

pub async fn verify_email(
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Json(email): Json<VerifyEmail>,
) -> Result<StatusCode, ApiError> {
    let flow = Flow::get(&email.flow, &redis).await?;

    if let Some(Flow::ConfirmEmail {
        user_id,
        confirm_email,
    }) = flow
    {
        let user = crate::database::models::User::get_id(user_id, &**pool, &redis)
            .await?
            .ok_or_else(|| AuthenticationError::InvalidCredentials)?;

        if user.email != Some(confirm_email) {
            return Err(ApiError::InvalidInput(
                "E-mail does not match verify email. Try re-requesting the verification link."
                    .to_string(),
            ));
        }

        let mut transaction = pool.begin().await?;

        sqlx::query!(
            "
            UPDATE users
            SET email_verified = TRUE
            WHERE (id = $1)
            ",
            user.id as crate::database::models::ids::UserId,
        )
        .execute(&mut *transaction)
        .await?;

        Flow::remove(&email.flow, &redis).await?;
        transaction.commit().await?;
        crate::database::models::User::clear_caches(&[(user.id, None)], &redis).await?;

        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::InvalidInput(
            "Flow does not exist. Try re-requesting the verification link.".to_string(),
        ))
    }
}

pub async fn subscribe_newsletter(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<StatusCode, ApiError> {
    let user = get_user_from_headers(
        &addr,
        &headers,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::USER_AUTH_WRITE]),
    )
    .await?
    .1;

    if let Some(email) = user.email {
        sign_up_beehiiv(&email).await?;

        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::InvalidInput(
            "User does not have an email.".to_string(),
        ))
    }
}

fn send_email_verify(
    email: String,
    flow: String,
    opener: &str,
) -> Result<(), crate::auth::email::MailError> {
    send_email(
        email,
        "Verify your email",
        opener,
        "Please visit the following link below to verify your email. If the button does not work, you can copy the link and paste it into your browser. This link expires in 24 hours.",
        Some(("Verify email", &format!("{}/{}?flow={}", dotenvy::var("SITE_URL")?,  dotenvy::var("SITE_VERIFY_EMAIL_PATH")?, flow))),
    )
}
