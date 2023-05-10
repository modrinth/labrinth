/*!
Current edition of Ory kratos does not support PAT access of data, so this module is how we allow for PAT authentication.


Just as a summary: Don't implement this flow in your application!
*/

use crate::database;
use crate::database::models::{PatToken, UserId};
use crate::models::ids::base62_impl::{parse_base62};


use crate::models::users::{self, Badges, RecipientType, RecipientWallet};


use chrono::{NaiveDateTime, Utc};

use serde::{Deserialize, Serialize};

use super::auth::AuthenticationError;

#[derive(Serialize, Deserialize)]
pub struct PersonalAccessToken {
    pub id: String,
    pub access_token: String,
    pub scope: String,
    pub user_id: users::UserId,
    pub expires_at: NaiveDateTime,
}

pub fn extract_pat_id_from_str(pat: &str) -> Result<PatToken, AuthenticationError> {
    // If the PAT starts with "mod_", then we remove it and parse the rest as a base62 number
    let access_token_id = if pat.starts_with("mod_") && pat.len() > 4 {
        parse_base62(&pat[4..])? as i64
    } else {
        parse_base62(pat)? as i64
    };
    Ok(PatToken(access_token_id))
}

// Find user from PAT token
// Separate to user_items as it may yet include further behaviour.
pub async fn get_user_from_pat<'a, E>(
    access_token: &str,
    executor: E,
) -> Result<Option<database::models::User>, AuthenticationError>
where
    E: sqlx::Executor<'a, Database = sqlx::Postgres>,
{
    let access_token_id = extract_pat_id_from_str(access_token)?;
    let row = sqlx::query!(
        "
                SELECT pats.expires_at,
                    u.id, u.name, u.kratos_id, u.email,
                    u.avatar_url, u.username, u.bio,
                    u.created, u.role, u.badges,
                    u.balance, u.payout_wallet, u.payout_wallet_type,
                    u.payout_address, u.github_id
                FROM pats LEFT OUTER JOIN users u ON pats.user_id = u.id
                WHERE access_token = $1
                ",
        access_token_id.0
    )
    .fetch_optional(executor)
    .await?;
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
            badges: Badges::from_bits(row.badges as u64).unwrap_or_default(),
            balance: row.balance,
            payout_wallet: row.payout_wallet.map(|x| RecipientWallet::from_string(&x)),
            payout_wallet_type: row
                .payout_wallet_type
                .map(|x| RecipientType::from_string(&x)),
            payout_address: row.payout_address,
        }));
    }
    Ok(None)
}
