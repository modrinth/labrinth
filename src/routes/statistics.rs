use crate::routes::ApiError;
use actix_web::{get, web, HttpRequest, HttpResponse};
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
            status = ( SELECT id FROM statuses WHERE status = $1 ) OR
            status = ( SELECT id FROM statuses WHERE status = $2 )
        ",
        crate::models::projects::ProjectStatus::Approved.as_str(),
        crate::models::projects::ProjectStatus::Archived.as_str()
    )
    .fetch_one(&**pool)
    .await?;

    let versions = sqlx::query!(
        "
        SELECT COUNT(v.id)
        FROM versions v
        INNER JOIN mods m on v.mod_id = m.id
        WHERE
            status = ( SELECT id FROM statuses WHERE status = $1 ) OR
            status = ( SELECT id FROM statuses WHERE status = $2 )
        ",
        crate::models::projects::ProjectStatus::Approved.as_str(),
        crate::models::projects::ProjectStatus::Archived.as_str()
    )
    .fetch_one(&**pool)
    .await?;

    let users = sqlx::query!("SELECT COUNT(id) FROM users")
        .fetch_one(&**pool)
        .await?;

    let authors = sqlx::query!(
        "
        SELECT COUNT(DISTINCT u.id)
        FROM users u
        INNER JOIN team_members tm on u.id = tm.user_id AND tm.accepted = TRUE
        INNER JOIN mods m on tm.team_id = m.team_id AND (
            m.status = ( SELECT s.id FROM statuses s WHERE s.status = $1 ) OR
            m.status = ( SELECT s.id FROM statuses s WHERE s.status = $2 )
        )
        ",
        crate::models::projects::ProjectStatus::Approved.as_str(),
        crate::models::projects::ProjectStatus::Archived.as_str()
    )
    .fetch_one(&**pool)
    .await?;

    let files = sqlx::query!(
        "
        SELECT COUNT(f.id) FROM files f
        INNER JOIN versions v on f.version_id = v.id
        INNER JOIN mods m on v.mod_id = m.id
        WHERE
            status = ( SELECT id FROM statuses WHERE status = $1 ) OR
            status = ( SELECT id FROM statuses WHERE status = $2 )
        ",
        crate::models::projects::ProjectStatus::Approved.as_str(),
        crate::models::projects::ProjectStatus::Archived.as_str()
    )
    .fetch_one(&**pool)
    .await?;

    let json = json!({
        "projects": projects.count,
        "versions": versions.count,
        "users": users.count,
        "authors": authors.count,
        "files": files.count,
    });

    Ok(HttpResponse::Ok().json(json))
}
