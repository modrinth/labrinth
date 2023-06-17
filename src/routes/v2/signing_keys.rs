use crate::models::signing_keys::SigningKey;
use crate::routes::ApiError;
use crate::util::auth::get_user_from_headers;
use crate::{database, models::ids::SigningKeyId};
use actix_web::{delete, get, web, HttpRequest, HttpResponse};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use database::models::signing_key_item::SigningKey as DBSigningKey;
use database::models::SigningKeyId as DBSigningKeyId;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(keys_get);
    cfg.service(keys_delete);

    cfg.service(web::scope("key").service(key_get).service(key_delete));
}

#[derive(Serialize, Deserialize)]
pub struct SigningKeyIds {
    pub ids: String,
}

#[get("keys")]
pub async fn keys_get(
    web::Query(ids): web::Query<SigningKeyIds>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let key_ids: Vec<DBSigningKeyId> =
        serde_json::from_str::<Vec<SigningKeyId>>(ids.ids.as_str())?
            .into_iter()
            .map(DBSigningKeyId::from)
            .collect();

    let keys_data: Vec<DBSigningKey> =
        DBSigningKey::get_many(&key_ids, &**pool).await?;

    let keys: Vec<SigningKey> =
        keys_data.into_iter().map(SigningKey::from).collect();

    Ok(HttpResponse::Ok().json(keys))
}

#[get("{id}")]
pub async fn key_get(
    info: web::Path<(SigningKeyId,)>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let id = info.into_inner().0;

    let Some(key_data) = DBSigningKey::get(id.into(), &**pool).await? else {
        return Ok(HttpResponse::NotFound().body(""));
    };

    Ok(HttpResponse::Ok().json(SigningKey::from(key_data)))
}

#[delete("{id}")]
pub async fn key_delete(
    req: HttpRequest,
    info: web::Path<(SigningKeyId,)>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(req.headers(), &**pool).await?;

    let id = info.into_inner().0;

    let Some(data) = DBSigningKey::get(id.into(), &**pool).await? else {
        return Ok(HttpResponse::NotFound().body(""));
    };

    if data.owner_id == user.id.into() || user.role.is_admin() {
        let mut transaction = pool.begin().await?;

        DBSigningKey::remove(id.into(), &mut transaction).await?;

        transaction.commit().await?;

        Ok(HttpResponse::NoContent().body(""))
    } else {
        Err(ApiError::CustomAuthentication(
            "You are not authorized to delete this key!".to_string(),
        ))
    }
}

#[delete("keys")]
pub async fn keys_delete(
    req: HttpRequest,
    web::Query(ids): web::Query<SigningKeyIds>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(req.headers(), &**pool).await?;

    let key_ids = serde_json::from_str::<Vec<SigningKeyId>>(&ids.ids)?
        .into_iter()
        .map(|x| x.into())
        .collect::<Vec<_>>();

    let mut transaction = pool.begin().await?;

    let keys_data = DBSigningKey::get_many(&key_ids, &**pool).await?;

    let mut keys: Vec<DBSigningKeyId> = Vec::new();

    for key in keys_data {
        if key.owner_id == user.id.into() || user.role.is_admin() {
            keys.push(key.id);
        }
    }

    DBSigningKey::remove_many(&keys, &mut transaction).await?;

    transaction.commit().await?;

    Ok(HttpResponse::NoContent().body(""))
}
