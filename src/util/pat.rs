/*!
Current edition of Ory kratos does not support PAT access of data, so this module is how we allow for PAT authentication.


Just as a summary: Don't implement this flow in your application!
*/

use crate::database::models::{
    generate_pat_id, generate_pat_token, PatToken,
};
use crate::models::ids::base62_impl::{parse_base62, to_base62};

use crate::routes::ApiError;
use crate::util::auth::{
    get_user_from_headers,
};

use actix_web::web::{Data, Query};
use actix_web::{delete, patch, post, HttpRequest, HttpResponse};
use chrono::{Duration, NaiveDateTime, Utc};

use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPool;

use super::auth::MinosUser;


#[derive(Serialize, Deserialize)]
pub struct PersonalAccessToken {
    pub id: String,
    pub access_token: String,
    pub scope: String,
    pub username: String,
    pub expires_at: NaiveDateTime,
}

// Check if a PAT is valid, and if so, return the username of the user it belongs to.
pub async fn get_user_from_pat(access_token : &str, pool: Data<PgPool>) -> Result<Option<String>,ApiError> {
    let mut transaction = pool.begin().await?;

    let access_id = parse_base62(&access_token)? as i64; 

    let row = sqlx::query!(
        "
            SELECT u.kratos_id, u.username, u.email, u.github_id, pats.expires_at
            FROM pats LEFT OUTER JOIN users u ON pats.username = u.username
            WHERE access_token = $1
            ",
        access_id
    )
    .fetch_optional(&mut *transaction).await?;

    if let Some(row) = row {
        let minos_user = MinosUser {
            id: row.kratos_id,
            name: row.username,
            email: row.email,
            github_id: row.github_id,
        };
        if row.expires_at < Utc::now().naive_utc() {
            return Ok(None);
        }
        return Ok(Some(minos_user));

    }

    Ok(None)
}


