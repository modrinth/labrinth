use std::collections::HashMap;
use crate::database::models::{generate_state_id, StateId};
use crate::models::ids::base62_impl::{parse_base62, to_base62};

use crate::parse_strings_from_var;

use actix_web::web::{scope, Data, Query, ServiceConfig};
use actix_web::{get, HttpResponse};
use chrono::Utc;
use reqwest::header::AUTHORIZATION;
use rust_decimal::Decimal;

use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPool;
use crate::auth::AuthenticationError;
use crate::models::users::{Badges, Role};

pub fn config(cfg: &mut ServiceConfig) {
    cfg.service(scope("auth").service(auth_callback).service(init));
}

#[derive(Serialize, Deserialize, Default, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AuthProvider {
    #[default]
    GitHub,
    Discord,
    Microsoft,
    // GitLab,
    // Google,
    // Apple,
}

#[derive(Debug)]
pub struct TempUser {
    pub id: String,
    pub username: String,
    pub email: Option<String>,

    pub avatar_url: Option<String>,
    pub bio: Option<String>,
    pub name: Option<String>,
}

impl AuthProvider {
    pub fn get_redirect_url(&self, state: StateId) -> Result<String, AuthenticationError> {
        let state = to_base62(state.0 as u64);
        Ok(match self {
            AuthProvider::GitHub => {
                let client_id = dotenvy::var("GITHUB_CLIENT_ID")?;

                format!(
                    "https://github.com/login/oauth/authorize?client_id={}&state={}&scope=read%3Auser%20user%3Aemail",
                    client_id,
                    state,
                )
            },
            AuthProvider::Discord => {
                let client_id = dotenvy::var("DISCORD_CLIENT_ID")?;

                format!("https://discord.com/api/oauth2/authorize?client_id={}&state={}&response_type=code&scope=identify%20email", client_id, state)
            },
            AuthProvider::Microsoft => {
                let client_id = dotenvy::var("MICROSOFT_CLIENT_ID")?;

                format!("https://login.live.com/oauth20_authorize.srf?client_id={}&response_type=code&scope=user.read&state={}&prompt=select_account", client_id, state)
            },
            // AuthProvider::GitLab => {
            //     "".to_string()
            // }
            // AuthProvider::Google => {
            //     "".to_string()
            // }
            // AuthProvider::Apple => {
            //     "".to_string()
            // }
        })
    }

    pub async fn get_token(&self, code: &str) -> Result<String, AuthenticationError> {
        #[derive(Deserialize)]
        struct AccessToken {
            pub access_token: String,
        }

        let res = match self {
            AuthProvider::GitHub => {
                let client_id = dotenvy::var("GITHUB_CLIENT_ID")?;
                let client_secret = dotenvy::var("GITHUB_CLIENT_SECRET")?;

                let url = format!(
                    "https://github.com/login/oauth/access_token?client_id={}&client_secret={}&code={}",
                    client_id, client_secret, code
                );

                let token: AccessToken = reqwest::Client::new()
                    .post(&url)
                    .header(reqwest::header::ACCEPT, "application/json")
                    .send()
                    .await?
                    .json()
                    .await?;

                token.access_token
            },
            AuthProvider::Discord => {
                let client_id = dotenvy::var("DISCORD_CLIENT_ID")?;
                let client_secret = dotenvy::var("DISCORD_CLIENT_SECRET")?;

                let mut map = HashMap::new();
                map.insert("client_id", &*client_id);
                map.insert("client_secret", &*client_secret);
                map.insert("code", code);
                map.insert("grant_type", "authorization_code");

                let token: AccessToken = reqwest::Client::new()
                    .post("https://discord.com/api/v10/oauth2/token",)
                    .header(reqwest::header::ACCEPT, "application/json")
                    .form(&map)
                    .send()
                    .await?
                    .json()
                    .await?;

                token.access_token
            },
            AuthProvider::Microsoft => {
                let client_id = dotenvy::var("MICROSOFT_CLIENT_ID")?;
                let client_secret = dotenvy::var("MICROSOFT_CLIENT_SECRET")?;

                let mut map = HashMap::new();
                map.insert("client_id", &*client_id);
                map.insert("client_secret", &*client_secret);
                map.insert("code", code);
                map.insert("grant_type", "authorization_code");

                let token: AccessToken = reqwest::Client::new()
                    .post("https://login.live.com/oauth20_token.srf",)
                    .header(reqwest::header::ACCEPT, "application/json")
                    .form(&map)
                    .send()
                    .await?
                    .json()
                    .await?;

                let token = serde_json::from_str::<AccessToken>(&token)?;

                token.access_token
            }
            // AuthProvider::GitLab => {}
            // AuthProvider::Google => {}
            // AuthProvider::Apple => { "".to_string() }
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
                    .await?.json().await?;

                let id = discord_user.id.clone();
                TempUser {
                    id: discord_user.id.parse().map_err(|_| AuthenticationError::InvalidCredentials)?,
                    username: discord_user.username,
                    email: discord_user.email,
                    avatar_url: discord_user.avatar.map(|x| format!("https://cdn.discordapp.com/avatars/{}/{}.webp", id, x)),
                    bio: None,
                    name: discord_user.global_name,
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
                    username: microsoft_user.user_principal_name.split("@").next().unwrap_or_default().to_string(),
                    email: microsoft_user.mail,
                    avatar_url: None,
                    bio: None,
                    name: microsoft_user.display_name,
                }
            }
        };

        Ok(res)
    }

    pub async fn get_user_id<'a, 'b, E>(&self, id: &str, executor: E) -> Result<Option<crate::database::models::UserId>, AuthenticationError>
        where
            E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        Ok(match self {
            AuthProvider::GitHub => {
                let value = sqlx::query!(
                    "SELECT id FROM users WHERE github_id = $1",
                    id.parse::<i64>().ok()
                ).fetch_optional(executor).await?;

                value.map(|x| crate::database::models::UserId(x.id))
            }
            AuthProvider::Discord => {
                let value = sqlx::query!(
                    "SELECT id FROM users WHERE discord_id = $1",
                    id.parse::<i64>().ok()
                ).fetch_optional(executor).await?;

                value.map(|x| crate::database::models::UserId(x.id))
            },
            AuthProvider::Microsoft => {
                let value = sqlx::query!(
                    "SELECT id FROM users WHERE microsoft_id = $1",
                    id
                ).fetch_optional(executor).await?;

                value.map(|x| crate::database::models::UserId(x.id))
            }
        })
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            AuthProvider::GitHub => "github",
            AuthProvider::Discord => "discord",
            AuthProvider::Microsoft => "microsoft",
            // AuthProvider::GitLab => "gitlab",
            // AuthProvider::Google => "google",
            // AuthProvider::Apple => "apple",
        }
    }

    pub fn from_str(string: &str) -> AuthProvider {
        match string {
            "github" => AuthProvider::GitHub,
            "discord" => AuthProvider::Discord,
            "microsoft" => AuthProvider::Microsoft,
            // "gitlab" => AuthProvider::GitLab,
            // "google" => AuthProvider::Google,
            // "apple" => AuthProvider::Apple,
            _ => AuthProvider::GitHub,
        }
    }
}

impl std::fmt::Display for AuthProvider {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        fmt.write_str(self.as_str())
    }
}

#[derive(Serialize, Deserialize)]
pub struct AuthorizationInit {
    pub url: String,
    #[serde(default)]
    pub provider: AuthProvider,
}
#[derive(Serialize, Deserialize)]
pub struct Authorization {
    pub code: String,
    pub state: String,
}

// Init link takes us to GitHub API and calls back to callback endpoint with a code and state
// http://localhost:8000/auth/init?url=https://modrinth.com
#[get("init")]
pub async fn init(
    Query(info): Query<AuthorizationInit>, // callback url
    client: Data<PgPool>,
) -> Result<HttpResponse, AuthenticationError> {
    let url = url::Url::parse(&info.url).map_err(|_| AuthenticationError::Url)?;

    let allowed_callback_urls = parse_strings_from_var("ALLOWED_CALLBACK_URLS").unwrap_or_default();
    let domain = url.host_str().ok_or(AuthenticationError::Url)?;
    if !allowed_callback_urls.iter().any(|x| domain.ends_with(x)) && domain != "modrinth.com" {
        return Err(AuthenticationError::Url);
    }

    let mut transaction = client.begin().await?;

    let state = generate_state_id(&mut transaction).await?;

    sqlx::query!(
        "
        INSERT INTO states (id, url, provider)
        VALUES ($1, $2, $3)
        ",
        state.0,
        info.url,
        info.provider.to_string()
    )
        .execute(&mut *transaction)
        .await?;

    transaction.commit().await?;

    let url = info.provider.get_redirect_url(state)?;
    Ok(HttpResponse::TemporaryRedirect()
        .append_header(("Location", &*url))
        .json(serde_json::json!({ "url": url })))
}

#[get("callback")]
pub async fn auth_callback(
    Query(state): Query<Authorization>,
    client: Data<PgPool>,
) -> Result<HttpResponse, AuthenticationError> {
    let mut transaction = client.begin().await?;
    let state_id: u64 = parse_base62(&state.state)?;

    let result_option = sqlx::query!(
        "
        SELECT url, expires, provider FROM states
        WHERE id = $1
        ",
        state_id as i64
    )
        .fetch_optional(&mut *transaction)
        .await?;

    // Extract cookie header from request
    if let Some(result) = result_option {
        // Extract cookie header to get authenticated user from Minos
        let duration: chrono::Duration = result.expires - Utc::now();
        if duration.num_seconds() < 0 {
            return Err(AuthenticationError::InvalidCredentials);
        }
        sqlx::query!(
            "
            DELETE FROM states
            WHERE id = $1
            ",
            state_id as i64
        )
            .execute(&mut *transaction)
            .await?;

        let provider = AuthProvider::from_str(&result.provider);

        let token = provider.get_token(&state.code).await?;
        let oauth_user = provider.get_user(&token).await?;
        let user_id = if let Some(user_id) = provider.get_user_id(&oauth_user.id, &mut *transaction).await? { user_id } else {
            let user_id =
                crate::database::models::generate_user_id(&mut transaction)
                    .await?;

            let mut username_increment: i32 = 0;
            let mut username = None;

            while username.is_none() {
                let test_username = format!(
                    "{}{}",
                    oauth_user.username,
                    if username_increment > 0 {
                        username_increment.to_string()
                    } else {
                        "".to_string()
                    }
                );

                let new_id = crate::database::models::User::get_id_from_username_or_id(
                    &test_username,
                    &**client,
                )
                    .await?;

                if new_id.is_none() {
                    username = Some(test_username);
                } else {
                    username_increment += 1;
                }
            }

            // TODO: Trim + validate data here
            if let Some(username) = username {
                crate::database::models::User {
                    id: user_id,
                    github_id: if provider == AuthProvider::GitHub { oauth_user.id.clone().parse().ok() } else { None },
                    discord_id: if provider == AuthProvider::Discord { oauth_user.id.parse().ok() } else { None },
                    gitlab_id: None,
                    google_id: None,
                    apple_id: None,
                    microsoft_id: if provider == AuthProvider::Microsoft { oauth_user.id.parse().ok() } else { None },
                    username,
                    name: oauth_user.name,
                    email: oauth_user.email,
                    avatar_url: oauth_user.avatar_url,
                    bio: oauth_user.bio,
                    created: Utc::now(),
                    role: Role::Developer.to_string(),
                    badges: Badges::default(),
                    balance: Decimal::ZERO,
                    payout_wallet: None,
                    payout_wallet_type: None,
                    payout_address: None,
                }
                    .insert(&mut transaction)
                    .await?;

                user_id
            } else {
                return Err(AuthenticationError::InvalidCredentials);
            }
        };

        transaction.commit().await?;

        // TODO: Issue modrinth token here
        let redirect_url = if result.url.contains('?') {
            format!("{}&code={}", result.url, token)
        } else {
            format!("{}?code={}", result.url, token)
        };

        Ok(HttpResponse::TemporaryRedirect()
            .append_header(("Location", &*redirect_url))
            .json(serde_json::json!({ "url": redirect_url })))
    } else {
        Err(AuthenticationError::InvalidCredentials)
    }
}
