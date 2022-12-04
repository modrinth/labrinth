use crate::routes::ApiError;
use actix_web::{get, web, HttpResponse};
use serde_json::json;
use sqlx::PgPool;

#[get("statistics")]
pub async fn get_stats(
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let projects = sqlx::query!(
        "
        SELECT COUNT(id)
        FROM mods
        WHERE
            status = $1 OR
            status = $2
        ",
        crate::models::projects::ProjectStatus::Approved.as_str(),
        crate::models::projects::ProjectStatus::Archived.as_str()
    )
    .fetch_one(&**pool);

    // TODO: add version statuses check
    let versions = sqlx::query!(
        "
        SELECT COUNT(v.id)
        FROM versions v
        INNER JOIN mods m on v.mod_id = m.id
        WHERE
            m.status = $1 OR
            m.status = $2
        ",
        crate::models::projects::ProjectStatus::Approved.as_str(),
        crate::models::projects::ProjectStatus::Archived.as_str()
    )
    .fetch_one(&**pool);

    let authors = sqlx::query!(
        "
        SELECT COUNT(DISTINCT u.id)
        FROM users u
        INNER JOIN team_members tm on u.id = tm.user_id AND tm.accepted = TRUE
        INNER JOIN mods m on tm.team_id = m.team_id AND (
            m.status = $1 OR
            m.status = $2
        )
        ",
        crate::models::projects::ProjectStatus::Approved.as_str(),
        crate::models::projects::ProjectStatus::Archived.as_str()
    )
    .fetch_one(&**pool);

    let files = sqlx::query!(
        "
        SELECT COUNT(f.id) FROM files f
        INNER JOIN versions v on f.version_id = v.id
        INNER JOIN mods m on v.mod_id = m.id
        WHERE
            m.status = $1 OR
            m.status = $2
        ",
        crate::models::projects::ProjectStatus::Approved.as_str(),
        crate::models::projects::ProjectStatus::Archived.as_str()
    )
    .fetch_one(&**pool);

    let (projects, versions, authors, files) =
        futures::future::try_join4(projects, versions, authors, files).await?;

    let json = json!({
        "projects": projects.count,
        "versions": versions.count,
        "authors": authors.count,
        "files": files.count,
    });

    Ok(HttpResponse::Ok().json(json))
}
