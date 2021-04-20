use super::ApiError;
use crate::auth::check_is_moderator_from_headers;
use crate::database;
use crate::models::projects::{Project, ProjectStatus};
use actix_web::{get, web, HttpRequest, HttpResponse};
use serde::Deserialize;
use sqlx::PgPool;

#[derive(Deserialize)]
pub struct ResultCount {
    #[serde(default = "default_count")]
    count: i16,
}

fn default_count() -> i16 {
    100
}

#[get("projects")]
pub async fn get_projects(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    count: web::Query<ResultCount>,
) -> Result<HttpResponse, ApiError> {
    check_is_moderator_from_headers(req.headers(), &**pool).await?;

    use futures::stream::TryStreamExt;

    let project_ids = sqlx::query!(
        "
        SELECT id FROM mods
        WHERE status = (
            SELECT id FROM statuses WHERE status = $1
        )
        ORDER BY updated ASC
        LIMIT $2;
        ",
        ProjectStatus::Processing.as_str(),
        count.count as i64
    )
    .fetch_many(&**pool)
    .try_filter_map(|e| async { Ok(e.right().map(|m| database::models::ProjectId(m.id))) })
    .try_collect::<Vec<database::models::ProjectId>>()
    .await
    .map_err(|e| ApiError::DatabaseError(e.into()))?;

    let projects: Vec<Project> = database::Project::get_many_full(project_ids, &**pool)
        .await
        .map_err(|e| ApiError::DatabaseError(e.into()))?
        .into_iter()
        .map(super::projects::convert_project)
        .collect();

    Ok(HttpResponse::Ok().json(projects))
}
