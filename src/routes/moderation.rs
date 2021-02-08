use super::ApiError;
use crate::auth::check_is_moderator_from_headers;
use crate::database;
use crate::models::mods::{ModId, ModStatus, Mod};
use crate::models::teams::TeamId;
use actix_web::{get, web, HttpRequest, HttpResponse};
use serde::{Deserialize, Serialize};
use sqlx::types::chrono::{DateTime, Utc};
use sqlx::PgPool;

#[derive(Deserialize)]
pub struct ResultCount {
    #[serde(default = "default_count")]
    count: i16,
}

fn default_count() -> i16 {
    100
}

#[get("mods")]
pub async fn mods(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    count: web::Query<ResultCount>,
) -> Result<HttpResponse, ApiError> {
    check_is_moderator_from_headers(req.headers(), &**pool).await?;

    use futures::stream::TryStreamExt;

    let mod_ids = sqlx::query!(
        "
        SELECT id FROM mods
        WHERE status = (
            SELECT id FROM statuses WHERE status = $1
        )
        ORDER BY updated ASC
        LIMIT $2;
        ",
        ModStatus::Processing.as_str(),
        count.count as i64
    )
    .fetch_many(&**pool)
    .try_filter_map(|e| async {
        Ok(e.right().map(|m| database::models::ids::ModId(m.id)))
    })
    .try_collect::<Vec<ModId>>()
    .await
    .map_err(|e| ApiError::DatabaseError(e.into()))?;

    let mods : Vec<Mod> = database::models::mod_item::Mod::get_many_full(mod_ids, &**pool).await?.into_iter().map(|x| super::mods::convert_mod(x)).collect();

    Ok(HttpResponse::Ok().json(mods))
}
