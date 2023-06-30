use crate::database::models::user_item;
use crate::models::users::{Role, User, UserId, UserPayoutData};
use actix_web::http::header::HeaderMap;
use reqwest::header::{HeaderValue, AUTHORIZATION};
use crate::auth::AuthenticationError;
use crate::auth::flows::{AuthProvider};
use crate::auth::get_user_from_pat;

pub async fn get_user_from_headers<'a, E>(
    headers: &HeaderMap,
    executor: E,
) -> Result<User, AuthenticationError>
where
    E: sqlx::Executor<'a, Database = sqlx::Postgres> + Copy,
{
    let token: Option<&HeaderValue> = headers.get(AUTHORIZATION);

    // Fetch DB user record and minos user from headers
    let db_user = get_user_record_from_bearer_token(
        token
            .ok_or_else(|| AuthenticationError::InvalidAuthMethod)?
            .to_str()
            .map_err(|_| AuthenticationError::InvalidCredentials)?,
        executor,
    )
    .await?
    .ok_or_else(|| AuthenticationError::InvalidCredentials)?;

    let user = User {
        id: UserId::from(db_user.id),
        github_id: db_user.github_id.map(|x| x as u64),
        // discord_id: minos_user.discord_id,
        // google_id: minos_user.google_id,
        // microsoft_id: minos_user.microsoft_id,
        // apple_id: minos_user.apple_id,
        // gitlab_id: minos_user.gitlab_id,
        username: db_user.username,
        name: db_user.name,
        email: db_user.email,
        avatar_url: db_user.avatar_url,
        bio: db_user.bio,
        created: db_user.created,
        role: Role::from_string(&db_user.role),
        badges: db_user.badges,
        payout_data: Some(UserPayoutData {
            balance: db_user.balance,
            payout_wallet: db_user.payout_wallet,
            payout_wallet_type: db_user.payout_wallet_type,
            payout_address: db_user.payout_address,
        }),
    };
    Ok(user)
}

pub async fn get_user_record_from_bearer_token<'a, 'b, E>(
    token: &str,
    executor: E,
) -> Result<Option<user_item::User>, AuthenticationError>
where
    E: sqlx::Executor<'a, Database = sqlx::Postgres> + Copy,
{
    if token.starts_with("Bearer ") {
        let token: &str = token.trim_start_matches("Bearer ");

        // Tokens beginning with Ory are considered to be Kratos tokens (in reality, extracted cookies) and can be forwarded to Minos
        let possible_user = match token.split_once('_') {
            Some(("modrinth", _)) => get_user_from_pat(token, executor).await?,
            Some(("github", _)) | Some(("gho", _)) | Some(("ghp", _)) => {
                let user = AuthProvider::GitHub.get_user(token).await?;
                let id = AuthProvider::GitHub.get_user_id(&user.id, executor).await?;

                user_item::User::get(id.ok_or_else(|| AuthenticationError::InvalidCredentials)?, executor).await?
            }
            _ => return Err(AuthenticationError::InvalidAuthMethod),
        };
        Ok(possible_user)
    } else {
        Err(AuthenticationError::InvalidAuthMethod)
    }
}

pub async fn check_is_moderator_from_headers<'a, 'b, E>(
    headers: &HeaderMap,
    executor: E,
) -> Result<User, AuthenticationError>
where
    E: sqlx::Executor<'a, Database = sqlx::Postgres> + Copy,
{
    let user = get_user_from_headers(headers, executor).await?;

    if user.role.is_mod() {
        Ok(user)
    } else {
        Err(AuthenticationError::InvalidCredentials)
    }
}
