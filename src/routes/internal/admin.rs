use crate::auth::validate::get_user_record_from_bearer_token;
use crate::auth::AuthenticationError;
use crate::database::redis::RedisPool;
use crate::models::analytics::Download;
use crate::models::ids::ProjectId;
use crate::models::pats::Scopes;
use crate::queue::analytics::AnalyticsQueue;
use crate::queue::maxmind::MaxMindIndexer;
use crate::queue::session::AuthQueue;
use crate::routes::ApiError;
use crate::search::SearchConfig;
use crate::util::date::get_current_tenths_of_ms;
use crate::util::extract::{ConnectInfo, Extension, Json};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{patch, post};
use axum::Router;
use serde::Deserialize;
use sqlx::PgPool;
use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

pub fn config() -> Router {
    Router::new().nest(
        "/admin",
        Router::new()
            .route("/_count-download", patch(count_download))
            .route("/_force_reindex", post(force_reindex)),
    )
}

#[derive(Deserialize)]
pub struct DownloadBody {
    pub url: String,
    pub project_id: ProjectId,
    pub version_name: String,

    pub ip: String,
    pub headers: HashMap<String, String>,
}

pub const ADMIN_KEY_HEADER: &str = "Modrinth-Admin";
fn check_admin_key(headers: &HeaderMap) -> Result<(), ApiError> {
    let admin_key = dotenvy::var("LABRINTH_ADMIN_KEY")?;

    if headers
        .get(ADMIN_KEY_HEADER)
        .map_or(false, |it| it.as_bytes() == admin_key.as_bytes())
    {
        Ok(())
    } else {
        Err(ApiError::Authentication(
            AuthenticationError::InvalidCredentials,
        ))
    }
}

// This is an internal route, cannot be used without key

pub async fn count_download(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(maxmind): Extension<Arc<MaxMindIndexer>>,
    Extension(analytics_queue): Extension<Arc<AnalyticsQueue>>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
    Json(download_body): Json<DownloadBody>,
) -> Result<StatusCode, ApiError> {
    check_admin_key(&headers)?;
    let token = download_body
        .headers
        .iter()
        .find(|x| x.0.to_lowercase() == "authorization")
        .map(|x| &**x.1);

    let user =
        get_user_record_from_bearer_token(&addr, &headers, token, &pool, &redis, &session_queue)
            .await
            .ok()
            .flatten();

    let project_id: crate::database::models::ids::ProjectId = download_body.project_id.into();

    let id_option = crate::models::ids::base62_impl::parse_base62(&download_body.version_name)
        .ok()
        .map(|x| x as i64);

    let (version_id, project_id) = if let Some(version) = sqlx::query!(
        "
            SELECT v.id id, v.mod_id mod_id FROM files f
            INNER JOIN versions v ON v.id = f.version_id
            WHERE f.url = $1
            ",
        download_body.url,
    )
    .fetch_optional(&pool)
    .await?
    {
        (version.id, version.mod_id)
    } else if let Some(version) = sqlx::query!(
        "
        SELECT id, mod_id FROM versions
        WHERE ((version_number = $1 OR id = $3) AND mod_id = $2)
        ",
        download_body.version_name,
        project_id as crate::database::models::ids::ProjectId,
        id_option
    )
    .fetch_optional(&pool)
    .await?
    {
        (version.id, version.mod_id)
    } else {
        return Err(ApiError::InvalidInput(
            "Specified version does not exist!".to_string(),
        ));
    };

    let url = url::Url::parse(&download_body.url)
        .map_err(|_| ApiError::InvalidInput("invalid download URL specified!".to_string()))?;

    let ip = crate::routes::analytics::convert_to_ip_v6(&download_body.ip)
        .unwrap_or_else(|_| Ipv4Addr::new(127, 0, 0, 1).to_ipv6_mapped());

    analytics_queue.add_download(Download {
        recorded: get_current_tenths_of_ms(),
        domain: url.host_str().unwrap_or_default().to_string(),
        site_path: url.path().to_string(),
        user_id: user
            .and_then(|(scopes, x)| {
                if scopes.contains(Scopes::PERFORM_ANALYTICS) {
                    Some(x.id.0 as u64)
                } else {
                    None
                }
            })
            .unwrap_or(0),
        project_id: project_id as u64,
        version_id: version_id as u64,
        ip,
        country: maxmind.query(ip).await.unwrap_or_default(),
        user_agent: download_body
            .headers
            .get("user-agent")
            .cloned()
            .unwrap_or_default(),
        headers: download_body
            .headers
            .clone()
            .into_iter()
            .filter(|x| !crate::routes::analytics::FILTERED_HEADERS.contains(&&*x.0.to_lowercase()))
            .collect(),
    });

    Ok(StatusCode::NO_CONTENT)
}

pub async fn force_reindex(
    headers: HeaderMap,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(config): Extension<SearchConfig>,
) -> Result<StatusCode, ApiError> {
    check_admin_key(&headers)?;
    use crate::search::indexing::index_projects;
    index_projects(pool.clone(), redis.clone(), &config).await?;

    Ok(StatusCode::NO_CONTENT)
}
