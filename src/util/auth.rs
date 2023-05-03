use crate::database;
use crate::database::models::project_item::QueryProject;
use crate::database::models::version_item::QueryVersion;
use crate::database::{models, Project, Version};
use crate::models::users::{Role, User, UserId, UserPayoutData, Badges};
use crate::routes::ApiError;
use actix_web::http::header::HeaderMap;
use actix_web::web;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use thiserror::Error;
use crate::Utc;
use actix_web::http::header::COOKIE;

#[derive(Error, Debug)]
pub enum AuthenticationError {
    #[error("An unknown database error occurred")]
    Sqlx(#[from] sqlx::Error),
    #[error("Database Error: {0}")]
    Database(#[from] models::DatabaseError),
    #[error("Error while parsing JSON: {0}")]
    SerDe(#[from] serde_json::Error),
    #[error("Error while communicating to GitHub OAuth2: {0}")]
    Github(#[from] reqwest::Error),
    #[error("Invalid Authentication Credentials")]
    InvalidCredentials,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct MinosUser {
    pub id: String, // This is the unique generated Ory name
    pub name: Option<String>,
    pub email: Option<String>,
    pub github_id: Option<i64>,
}

// Insert a new user into the database from a MinosUser without a corresponding entry
pub async fn insert_new_user(
    transaction : &mut sqlx::Transaction<'_, sqlx::Postgres>,
    minos_user: MinosUser,
) -> Result<(), AuthenticationError> {
    let user_id =
    crate::database::models::generate_user_id(transaction)
        .await?;

    // TODO: username should reflect a real addable username, not just this dummy one for new accounts
    let username = minos_user
        .name
        .clone()
        .unwrap_or_else(|| format!("modrinth-{}", user_id.0));

    database::models::User {
        id: user_id,
        github_id: minos_user.github_id,
        kratos_id: minos_user.id,
        username,
        name: minos_user.name,
        email: minos_user.email,
        avatar_url: None,
        bio: None,
        created: Utc::now(),
        role: Role::Developer.to_string(),
        badges: Badges::default(),
        balance: Decimal::ZERO,
        payout_wallet: None,
        payout_wallet_type: None,
        payout_address: None,
    }
    .insert(transaction)
    .await?;

    Ok(())

}

// To get a user, we pass either the access token or the authorization header through to minos
// Access token is passed automatically through cookies
pub async fn get_minos_user_from_minos(cookies: Option<&str>, access_token: Option<&str>,
) -> Result<MinosUser, AuthenticationError> {
    
    let mut req = reqwest::Client::new()
        .get(dotenvy::var("MINOS_URL").unwrap()+"/user/session")
        .header(reqwest::header::USER_AGENT, "Modrinth");
        // .header(reqwest::header::ACCESS_CONTROL_ALLOW_CREDENTIALS, "true");

    if let Some(cookies) = cookies {
        req = req        .header(reqwest::header::COOKIE, cookies);
    }
    dbg!("Hello111", &req);

    // Add the access token if it exists
    if let Some(access_token) = access_token {
        req = req.header(
            reqwest::header::AUTHORIZATION,
            format!("token {access_token}")
        );
        dbg!("Hello12", &req);

    }

    let res = req
        .send()
        .await?
        .error_for_status()?;

    dbg!("Hello");
    dbg!(&res);
    Ok(res
        .json()
        .await?)
}
// Extract MinosUser from oprtional token and cookie headers
// If both are present, token is used
// If neither are present, InvalidCredentials is returned
pub async fn get_minos_user_from_headers(
    token : Option<&reqwest::header::HeaderValue>,
    cookies : Option<&reqwest::header::HeaderValue>,
) -> Result<MinosUser, AuthenticationError> {
    if let (None, None) = (token, cookies)  {
        return Err(AuthenticationError::InvalidCredentials) // No credentials passed
    }

    let token: Option<&str>  = if let Some(token) = token {
        Some(token.to_str().map_err(|_| AuthenticationError::InvalidCredentials)?)
    } else {
        None
    };

    let cookies: Option<&str>  = if let Some(cookies) = cookies {
        Some(cookies.to_str().map_err(|_| AuthenticationError::InvalidCredentials)?)
    } else {
        None
    };

    Ok(get_minos_user_from_minos(cookies, token).await?)
}

pub async fn get_user_from_headers<'a, 'b, E>(
    headers: &HeaderMap,
    executor: E,
) -> Result<User, AuthenticationError>
where
    E: sqlx::Executor<'a, Database = sqlx::Postgres>,
{
    let token: Option<&reqwest::header::HeaderValue> = headers.get("Authorization");
    let cookies_unparsed: Option<&reqwest::header::HeaderValue> = headers.get(COOKIE);
    let minos_user = get_minos_user_from_headers(token, cookies_unparsed).await?;
    let res =
        models::User::get_from_minos_kratos_id(minos_user.id, executor).await?;

        match res {
            Some(result) => Ok(User {
                id: UserId::from(result.id),
                github_id: result.github_id.map(|i| i as u64),
                username: result.username,
                name: result.name,
                email: result.email,
                avatar_url: result.avatar_url,
                bio: result.bio,
                created: result.created,
                role: Role::from_string(&result.role),
                badges: result.badges,
                payout_data: Some(UserPayoutData {
                    balance: result.balance,
                    payout_wallet: result.payout_wallet,
                    payout_wallet_type: result.payout_wallet_type,
                    payout_address: result.payout_address,
                }),
            }),
            None => Err(AuthenticationError::InvalidCredentials),
        }
    }

pub async fn check_is_moderator_from_headers<'a, 'b, E>(
    headers: &HeaderMap,
    executor: E,
) -> Result<User, AuthenticationError>
where
    E: sqlx::Executor<'a, Database = sqlx::Postgres>,
{
    let user = get_user_from_headers(headers, executor).await?;

    if user.role.is_mod() {
        Ok(user)
    } else {
        Err(AuthenticationError::InvalidCredentials)
    }
}

pub async fn is_authorized(
    project_data: &Project,
    user_option: &Option<User>,
    pool: &web::Data<PgPool>,
) -> Result<bool, ApiError> {
    let mut authorized = !project_data.status.is_hidden();

    if let Some(user) = &user_option {
        if !authorized {
            if user.role.is_mod() {
                authorized = true;
            } else {
                let user_id: models::ids::UserId = user.id.into();

                let project_exists = sqlx::query!(
                    "SELECT EXISTS(SELECT 1 FROM team_members WHERE team_id = $1 AND user_id = $2)",
                    project_data.team_id as database::models::ids::TeamId,
                    user_id as database::models::ids::UserId,
                )
                .fetch_one(&***pool)
                .await?
                .exists;

                authorized = project_exists.unwrap_or(false);
            }
        }
    }

    Ok(authorized)
}

pub async fn filter_authorized_projects(
    projects: Vec<QueryProject>,
    user_option: &Option<User>,
    pool: &web::Data<PgPool>,
) -> Result<Vec<crate::models::projects::Project>, ApiError> {
    let mut return_projects = Vec::new();
    let mut check_projects = Vec::new();

    for project in projects {
        if !project.inner.status.is_hidden()
            || user_option
                .as_ref()
                .map(|x| x.role.is_mod())
                .unwrap_or(false)
        {
            return_projects.push(project.into());
        } else if user_option.is_some() {
            check_projects.push(project);
        }
    }

    if !check_projects.is_empty() {
        if let Some(user) = user_option {
            let user_id: models::ids::UserId = user.id.into();

            use futures::TryStreamExt;

            sqlx::query!(
                "
                SELECT m.id id, m.team_id team_id FROM team_members tm
                INNER JOIN mods m ON m.team_id = tm.team_id
                WHERE tm.team_id = ANY($1) AND tm.user_id = $2
                ",
                &check_projects
                    .iter()
                    .map(|x| x.inner.team_id.0)
                    .collect::<Vec<_>>(),
                user_id as database::models::ids::UserId,
            )
            .fetch_many(&***pool)
            .try_for_each(|e| {
                if let Some(row) = e.right() {
                    check_projects.retain(|x| {
                        let bool = x.inner.id.0 == row.id
                            && x.inner.team_id.0 == row.team_id;

                        if bool {
                            return_projects.push(x.clone().into());
                        }

                        !bool
                    });
                }

                futures::future::ready(Ok(()))
            })
            .await?;
        }
    }

    Ok(return_projects)
}

pub async fn is_authorized_version(
    version_data: &Version,
    user_option: &Option<User>,
    pool: &web::Data<PgPool>,
) -> Result<bool, ApiError> {
    let mut authorized = !version_data.status.is_hidden();

    if let Some(user) = &user_option {
        if !authorized {
            if user.role.is_mod() {
                authorized = true;
            } else {
                let user_id: models::ids::UserId = user.id.into();

                let version_exists = sqlx::query!(
                    "SELECT EXISTS(SELECT 1 FROM mods m INNER JOIN team_members tm ON tm.team_id = m.team_id AND user_id = $2 WHERE m.id = $1)",
                    version_data.project_id as database::models::ids::ProjectId,
                    user_id as database::models::ids::UserId,
                )
                    .fetch_one(&***pool)
                    .await?
                    .exists;

                authorized = version_exists.unwrap_or(false);
            }
        }
    }

    Ok(authorized)
}

pub async fn filter_authorized_versions(
    versions: Vec<QueryVersion>,
    user_option: &Option<User>,
    pool: &web::Data<PgPool>,
) -> Result<Vec<crate::models::projects::Version>, ApiError> {
    let mut return_versions = Vec::new();
    let mut check_versions = Vec::new();

    for version in versions {
        if !version.inner.status.is_hidden()
            || user_option
                .as_ref()
                .map(|x| x.role.is_mod())
                .unwrap_or(false)
        {
            return_versions.push(version.into());
        } else if user_option.is_some() {
            check_versions.push(version);
        }
    }

    if !check_versions.is_empty() {
        if let Some(user) = user_option {
            let user_id: models::ids::UserId = user.id.into();

            use futures::TryStreamExt;

            sqlx::query!(
                "
                SELECT m.id FROM mods m
                INNER JOIN team_members tm ON tm.team_id = m.team_id AND user_id = $2
                WHERE m.id = ANY($1)
                ",
                &check_versions.iter().map(|x| x.inner.project_id.0).collect::<Vec<_>>(),
                user_id as database::models::ids::UserId,
            )
                .fetch_many(&***pool)
                .try_for_each(|e| {
                    if let Some(row) = e.right() {
                        check_versions.retain(|x| {
                            let bool = x.inner.project_id.0 == row.id;

                            if bool {
                                return_versions.push(x.clone().into());
                            }

                            !bool
                        });
                    }

                    futures::future::ready(Ok(()))
                }).await?;
        }
    }

    Ok(return_versions)
}
