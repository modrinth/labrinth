/*!
Current edition of Ory kratos does not support PAT access of data, so this module is how we allow for PAT authentication.


Just as a summary: Don't implement this flow in your application!
*/

use crate::database;
use crate::database::models::{
    generate_pat_id, generate_pat_token, PatToken, UserId,
};
use crate::models::ids::base62_impl::{parse_base62, to_base62};

use crate::models::users::{Badges, RecipientWallet, RecipientType};
use crate::routes::ApiError;
use crate::util::auth::{
    get_user_from_headers,
};

use actix_web::web::{Data, Query};
use actix_web::{delete, patch, post, HttpRequest, HttpResponse};
use chrono::{Duration, NaiveDateTime, Utc};

use serde::{Deserialize, Serialize};
use sqlx::Executor;
use sqlx::postgres::PgPool;

use super::auth::{MinosUser, AuthenticationError};


#[derive(Serialize, Deserialize)]
pub struct PersonalAccessToken {
    pub id: String,
    pub access_token: String,
    pub scope: String,
    pub username: String,
    pub expires_at: NaiveDateTime,
}

// Check if a PAT is valid, and if so, return the username of the user it belongs to.
// Separate to user_items as it may yet include further behaviour.
pub async fn get_user_from_pat<'a, E>(access_token : &str, executor : E) -> Result<Option<database::models::User>, AuthenticationError> 
    where         E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    
    {
        let access_id = parse_base62(&access_token)? as i64; 
    
        let row = sqlx::query!(
            "
                SELECT pats.expires_at,
                    u.id, u.name, u.kratos_id, u.email,
                    u.avatar_url, u.username, u.bio,
                    u.created, u.role, u.badges,
                    u.balance, u.payout_wallet, u.payout_wallet_type,
                    u.payout_address, u.github_id
                FROM pats LEFT OUTER JOIN users u ON pats.username = u.username
                WHERE access_token = $1
                ",
            access_id
        )
        .fetch_optional(executor).await?;
        if let Some(row) = row {
            if row.expires_at < Utc::now().naive_utc() {
                return Ok(None);
            }
               return Ok(Some(database::models::User {
                    id: UserId(row.id),
                    kratos_id: row.kratos_id,
                    github_id: row.github_id,
                    name: row.name,
                    email: row.email,
                    avatar_url: row.avatar_url,
                    username: row.username,
                    bio: row.bio,
                    created: row.created,
                    role: row.role,
                    badges: Badges::from_bits(row.badges as u64)
                        .unwrap_or_default(),
                    balance: row.balance,
                    payout_wallet: row
                        .payout_wallet
                        .map(|x| RecipientWallet::from_string(&x)),
                    payout_wallet_type: row
                        .payout_wallet_type
                        .map(|x| RecipientType::from_string(&x)),
                    payout_address: row.payout_address,
                }));
            }
            Ok(None)
        }
    