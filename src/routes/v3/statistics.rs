use crate::{database::redis::RedisPool, routes::ApiError};
use actix_web::{web, HttpResponse};
use sqlx::PgPool;

const STATISTICS_NAMESPACE: &str = "statistics";
const STATISTICS_EXPIRY: i64 = 60 * 30; // 30 minutes

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("statistics", web::get().to(get_stats));
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct V3Stats {
    pub projects: i64,
    pub versions: i64,
    pub authors: i64,
    pub files: i64,
}

pub async fn get_stats(
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
) -> Result<HttpResponse, ApiError> {
    let mut redis = redis.connect().await?;

    let projects = if let Some(project_count) = redis
        .get_deserialized_from_json::<i64>(STATISTICS_NAMESPACE, "projects")
        .await?
    {
        project_count
    } else {
        let count = sqlx::query!(
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
        .fetch_one(&**pool)
        .await?
        .count
        .unwrap();

        redis
            .set_serialized_to_json(
                STATISTICS_NAMESPACE,
                "projects",
                count,
                Some(STATISTICS_EXPIRY),
            )
            .await?;

        count
    };

    let versions = if let Some(version_count) = redis
        .get_deserialized_from_json::<i64>(STATISTICS_NAMESPACE, "versions")
        .await?
    {
        version_count
    } else {
        let count = sqlx::query!(
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
        .fetch_one(&**pool)
        .await?
        .count
        .unwrap();

        redis
            .set_serialized_to_json(
                STATISTICS_NAMESPACE,
                "versions",
                count,
                Some(STATISTICS_EXPIRY),
            )
            .await?;

        count
    };

    let authors = if let Some(author_count) = redis
        .get_deserialized_from_json::<i64>(STATISTICS_NAMESPACE, "authors")
        .await?
    {
        author_count
    } else {
        let count = sqlx::query!(
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
        .fetch_one(&**pool)
        .await?
        .count
        .unwrap();

        redis
            .set_serialized_to_json(
                STATISTICS_NAMESPACE,
                "authors",
                count,
                Some(STATISTICS_EXPIRY),
            )
            .await?;

        count
    };

    let files = if let Some(file_count) = redis
        .get_deserialized_from_json::<i64>(STATISTICS_NAMESPACE, "files")
        .await?
    {
        file_count
    } else {
        let count = sqlx::query!(
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
        .fetch_one(&**pool)
        .await?
        .count
        .unwrap();

        redis
            .set_serialized_to_json(
                STATISTICS_NAMESPACE,
                "files",
                count,
                Some(STATISTICS_EXPIRY),
            )
            .await?;

        count
    };

    let v3_stats = V3Stats {
        projects,
        versions,
        authors,
        files,
    };

    Ok(HttpResponse::Ok().json(v3_stats))
}
