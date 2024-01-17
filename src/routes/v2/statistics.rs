use crate::routes::{
    v3,
    ApiError,
};
use axum::{Router, routing::get};
use crate::util::extract::{Extension, Json};
use sqlx::PgPool;

pub fn config() -> Router {
    Router::new()
        .route("/statistics", get(get_stats))
}

#[derive(serde::Serialize)]
pub struct V2Stats {
    pub projects: Option<i64>,
    pub versions: Option<i64>,
    pub authors: Option<i64>,
    pub files: Option<i64>,
}

pub async fn get_stats(Extension(pool): Extension<PgPool>) -> Result<Json<V2Stats>, ApiError> {
    let Json(stats) = v3::statistics::get_stats(
        Extension(pool),
    )
        .await?;

    let stats = V2Stats {
        projects: stats.projects,
        versions: stats.versions,
        authors: stats.authors,
        files: stats.files,
    };
    Ok(Json(stats))
}
