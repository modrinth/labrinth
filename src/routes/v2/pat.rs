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

use super::auth::AuthorizationError;



#[derive(Serialize, Deserialize)]
pub struct PersonalAccessToken {
    pub id: String,
    pub access_token: String,
    pub scope: String,
    pub username: String,
    pub expires_at: NaiveDateTime,
}

#[derive(Deserialize)]
pub struct CreatePersonalAccessToken {
    pub scope: String,
    pub expire_in_days: i64, // resets expiry to expire_in_days days from now
}

#[derive(Deserialize)]
pub struct ModifyPersonalAccessToken {
    pub access_token: String,
    pub scope: Option<String>,
    pub expire_in_days: Option<i64>, // resets expiry to expire_in_days days from now
}

// POST /pat
// Create a new personal access token for the given user. Minos/Kratos cookie must be attached for it to work.
#[post("pat")]
pub async fn create_pat(
    req: HttpRequest,
    Query(info): Query<CreatePersonalAccessToken>, // callback url
    pool: Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let user: crate::models::users::User =
        get_user_from_headers(req.headers(), &**pool).await?;

    let mut transaction = pool.begin().await?;

    let pat = generate_pat_id(&mut transaction).await?;
    let access_token = generate_pat_token(&mut transaction).await?;
    let expiry = Utc::now().naive_utc() + Duration::days(info.expire_in_days);

    sqlx::query!(
        "
            INSERT INTO pats (id, access_token, username, scope, expires_at)
            VALUES ($1, $2, $3, $4, $5)
            ",
        pat.0,
        access_token.0,
        user.username,
        info.scope,
        expiry
    )
    .execute(&mut *transaction)
    .await?;

    transaction.commit().await?;

    Ok(HttpResponse::Ok().json(PersonalAccessToken {
        id: to_base62(pat.0 as u64),
        access_token: to_base62(access_token.0 as u64),
        scope: info.scope,
        username: user.username,
        expires_at: expiry,
    }))
}


// PATCH /pat
// Edit an access token for the given user. 'None' will mean not edited. Minos/Kratos cookie must be attached for it to work.
#[patch("pat")]
pub async fn edit_pat(
    req: HttpRequest,
    Query(info): Query<ModifyPersonalAccessToken>, // callback url
    pool: Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let user: crate::models::users::User =
        get_user_from_headers(req.headers(), &**pool).await?;
    let access_token: PatToken =
        PatToken(parse_base62(&info.access_token)? as i64);

    // Get the singular PAT and user combination (failing immediately if it doesn't exist)
    let mut transaction = pool.begin().await?;
    let row = sqlx::query!(
        "
        SELECT id, access_token, scope, username, expires_at FROM pats
        WHERE access_token = $1 AND username = $2
        ",
        access_token.0,
        user.username
    )
    .fetch_one(&**pool)
    .await?;

    let pat = PersonalAccessToken {
        id: to_base62(row.id as u64),
        access_token: to_base62(row.access_token as u64),
        username: row.username,

        scope: info.scope.unwrap_or(row.scope),
        expires_at: info
            .expire_in_days
            .map(|d| Utc::now().naive_utc() + Duration::days(d))
            .unwrap_or(row.expires_at),
    };

    sqlx::query!(
        "
        UPDATE pats SET
            access_token = $1,
            scope = $2,
            username = $3,
            expires_at = $4
        WHERE id = $5
        ",
        parse_base62(&pat.access_token)? as i64,
        pat.scope,
        pat.username,
        pat.expires_at,
        parse_base62(&pat.id)? as i64
    )
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;

    Ok(HttpResponse::Ok().json(pat))
}

// DELETE /pat
// Delete a personal access token for the given user. Minos/Kratos cookie must be attached for it to work.
#[delete("pat")]
pub async fn delete_pat(
    req: HttpRequest,
    Query(access_token): Query<String>, // callback url
    pool: Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let user: crate::models::users::User =
        get_user_from_headers(req.headers(), &**pool).await?;

    let access_token: PatToken = PatToken(parse_base62(&access_token)? as i64);

    // Get the singular PAT and user combination (failing immediately if it doesn't exist)
    let pat_id = sqlx::query!(
        "
        SELECT id FROM pats
        WHERE access_token = $1 AND username = $2
        ",
        access_token.0,
        user.username
    )
    .fetch_one(&**pool)
    .await?
    .id;

    let mut transaction = pool.begin().await?;
    sqlx::query!(
        "
        DELETE FROM pats
        WHERE id = $1
        ",
        pat_id,
    )
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;

    Ok(HttpResponse::Ok().finish())
}
