use actix_web::{web, HttpRequest, HttpResponse};
use sqlx::PgPool;

use crate::{
    auth::get_user_from_headers,
    database::{models::User, redis::RedisPool},
    models::{ids::UserId, pats::Scopes, projects::Project},
    queue::session::AuthQueue,
};

use super::ApiError;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(web::scope("user").route("{user_id}/projects", web::get().to(projects_list)));
}

pub async fn projects_list(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::PROJECT_READ]),
    )
    .await
    .map(|x| x.1)
    .ok();

    let id_option = User::get(&info.into_inner().0, &**pool, &redis).await?;

    if let Some(id) = id_option.map(|x| x.id) {
        let user_id: UserId = id.into();

        let can_view_private = user
            .map(|y| y.role.is_mod() || y.id == user_id)
            .unwrap_or(false);

        let project_data = User::get_projects(id, &**pool, &redis).await?;

        let response: Vec<_> =
            crate::database::Project::get_many_ids(&project_data, &**pool, &redis)
                .await?
                .into_iter()
                .filter(|x| can_view_private || x.inner.status.is_searchable())
                .map(Project::from)
                .collect();

        Ok(HttpResponse::Ok().json(response))
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
}
