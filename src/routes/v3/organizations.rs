use actix_web::{web, HttpRequest, HttpResponse};
use sqlx::PgPool;

use crate::{
    auth::{filter_authorized_projects, get_user_from_headers},
    database::redis::RedisPool,
    models::{ids::base62_impl::parse_base62, pats::Scopes},
    queue::session::AuthQueue,
};

use super::ApiError;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("organization").route("{id}/projects", web::get().to(organization_projects_get)),
    );
}

pub async fn organization_projects_get(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let info = info.into_inner().0;
    let current_user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::ORGANIZATION_READ, Scopes::PROJECT_READ]),
    )
    .await
    .map(|x| x.1)
    .ok();

    let possible_organization_id: Option<u64> = parse_base62(&info).ok();
    use futures::TryStreamExt;

    let project_ids = sqlx::query!(
        "
        SELECT m.id FROM organizations o
        INNER JOIN mods m ON m.organization_id = o.id
        WHERE (o.id = $1 AND $1 IS NOT NULL) OR (o.title = $2 AND $2 IS NOT NULL)
        ",
        possible_organization_id.map(|x| x as i64),
        info
    )
    .fetch_many(&**pool)
    .try_filter_map(|e| async { Ok(e.right().map(|m| crate::database::models::ProjectId(m.id))) })
    .try_collect::<Vec<crate::database::models::ProjectId>>()
    .await?;

    let projects_data =
        crate::database::models::Project::get_many_ids(&project_ids, &**pool, &redis).await?;

    let projects = filter_authorized_projects(projects_data, &current_user, &pool).await?;
    Ok(HttpResponse::Ok().json(projects))
}
