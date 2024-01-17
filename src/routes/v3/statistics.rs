use crate::routes::ApiError;
use crate::util::extract::{Extension, Json};
use axum::routing::get;
use axum::Router;
use sqlx::PgPool;

pub fn config() -> Router {
    Router::new().route("/statistics", get(get_stats))
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct V3Stats {
    pub projects: Option<i64>,
    pub versions: Option<i64>,
    pub authors: Option<i64>,
    pub files: Option<i64>,
}

pub async fn get_stats(Extension(pool): Extension<PgPool>) -> Result<Json<V3Stats>, ApiError> {
    let projects = sqlx::query!(
        "
        SELECT COUNT(id)
        FROM mods
        WHERE status = ANY($1)
        ",
        &*crate::models::projects::ProjectStatus::iterator()
            .filter(|x| x.is_searchable())
            .map(|x| x.to_string())
            .collect::<Vec<String>>(),
    )
    .fetch_one(&pool)
    .await?;

    let versions = sqlx::query!(
        "
        SELECT COUNT(v.id)
        FROM versions v
        INNER JOIN mods m on v.mod_id = m.id AND m.status = ANY($1)
        WHERE v.status = ANY($2)
        ",
        &*crate::models::projects::ProjectStatus::iterator()
            .filter(|x| x.is_searchable())
            .map(|x| x.to_string())
            .collect::<Vec<String>>(),
        &*crate::models::projects::VersionStatus::iterator()
            .filter(|x| x.is_listed())
            .map(|x| x.to_string())
            .collect::<Vec<String>>(),
    )
    .fetch_one(&pool)
    .await?;

    let authors = sqlx::query!(
        "
        SELECT COUNT(DISTINCT u.id)
        FROM users u
        INNER JOIN team_members tm on u.id = tm.user_id AND tm.accepted = TRUE
        INNER JOIN mods m on tm.team_id = m.team_id AND m.status = ANY($1)
        ",
        &*crate::models::projects::ProjectStatus::iterator()
            .filter(|x| x.is_searchable())
            .map(|x| x.to_string())
            .collect::<Vec<String>>(),
    )
    .fetch_one(&pool)
    .await?;

    let files = sqlx::query!(
        "
        SELECT COUNT(f.id) FROM files f
        INNER JOIN versions v on f.version_id = v.id AND v.status = ANY($2)
        INNER JOIN mods m on v.mod_id = m.id AND m.status = ANY($1)
        ",
        &*crate::models::projects::ProjectStatus::iterator()
            .filter(|x| x.is_searchable())
            .map(|x| x.to_string())
            .collect::<Vec<String>>(),
        &*crate::models::projects::VersionStatus::iterator()
            .filter(|x| x.is_listed())
            .map(|x| x.to_string())
            .collect::<Vec<String>>(),
    )
    .fetch_one(&pool)
    .await?;

    let v3_stats = V3Stats {
        projects: projects.count,
        versions: versions.count,
        authors: authors.count,
        files: files.count,
    };

    Ok(Json(v3_stats))
}
