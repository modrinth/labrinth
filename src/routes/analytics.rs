use crate::auth::get_user_from_headers;
use crate::database::redis::RedisPool;
use crate::models::analytics::{PageView, Playtime};
use crate::models::pats::Scopes;
use crate::queue::analytics::AnalyticsQueue;
use crate::queue::maxmind::MaxMindIndexer;
use crate::queue::session::AuthQueue;
use crate::routes::ApiError;
use crate::util::date::get_current_tenths_of_ms;
use crate::util::env::parse_strings_from_var;
use crate::util::extract::{ConnectInfo, Extension, Json};
use crate::util::ip::get_ip_addr;
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;
use sqlx::PgPool;
use std::collections::HashMap;
use std::net::{AddrParseError, IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use url::Url;

pub const FILTERED_HEADERS: &[&str] = &[
    "authorization",
    "cookie",
    "modrinth-admin",
    // we already retrieve/use these elsewhere- so they are unneeded
    "user-agent",
    "cf-connecting-ip",
    "cf-ipcountry",
    "x-forwarded-for",
    "x-real-ip",
    // We don't need the information vercel provides from its headers
    "x-vercel-ip-city",
    "x-vercel-ip-timezone",
    "x-vercel-ip-longitude",
    "x-vercel-proxy-signature",
    "x-vercel-ip-country-region",
    "x-vercel-forwarded-for",
    "x-vercel-proxied-for",
    "x-vercel-proxy-signature-ts",
    "x-vercel-ip-latitude",
    "x-vercel-ip-country",
];

pub fn convert_to_ip_v6(src: &str) -> Result<Ipv6Addr, AddrParseError> {
    let ip_addr: IpAddr = src.parse()?;

    Ok(match ip_addr {
        IpAddr::V4(x) => x.to_ipv6_mapped(),
        IpAddr::V6(x) => x,
    })
}

#[derive(Deserialize)]
pub struct UrlInput {
    url: String,
}

//this route should be behind the cloudflare WAF to prevent non-browsers from calling it
pub async fn page_view_ingest(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(maxmind): Extension<Arc<MaxMindIndexer>>,
    Extension(analytics_queue): Extension<Arc<AnalyticsQueue>>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Json(url_input): Json<UrlInput>,
) -> Result<StatusCode, ApiError> {
    let user = get_user_from_headers(&addr, &headers, &pool, &redis, &session_queue, None)
        .await
        .ok();

    let ip = convert_to_ip_v6(&get_ip_addr(&addr, &headers))
        .unwrap_or_else(|_| Ipv4Addr::new(127, 0, 0, 1).to_ipv6_mapped());

    let url = Url::parse(&url_input.url)
        .map_err(|_| ApiError::InvalidInput("invalid page view URL specified!".to_string()))?;

    let domain = url
        .host_str()
        .ok_or_else(|| ApiError::InvalidInput("invalid page view URL specified!".to_string()))?;

    let allowed_origins = parse_strings_from_var("CORS_ALLOWED_ORIGINS").unwrap_or_default();
    if !(domain.ends_with(".modrinth.com")
        || domain == "modrinth.com"
        || allowed_origins.contains(&"*".to_string()))
    {
        return Err(ApiError::InvalidInput(
            "invalid page view URL specified!".to_string(),
        ));
    }

    let headers = headers
        .into_iter()
        .flat_map(|(key, val)| {
            key.map(|key| {
                (
                    key.to_string().to_lowercase(),
                    val.to_str().unwrap_or_default().to_string(),
                )
            })
        })
        .collect::<HashMap<String, String>>();

    let mut view = PageView {
        recorded: get_current_tenths_of_ms(),
        domain: domain.to_string(),
        site_path: url.path().to_string(),
        user_id: 0,
        project_id: 0,
        ip,
        country: maxmind.query(ip).await.unwrap_or_default(),
        user_agent: headers.get("user-agent").cloned().unwrap_or_default(),
        headers: headers
            .into_iter()
            .filter(|x| !FILTERED_HEADERS.contains(&&*x.0))
            .collect(),
    };

    if let Some(segments) = url.path_segments() {
        let segments_vec = segments.collect::<Vec<_>>();

        if segments_vec.len() >= 2 {
            const PROJECT_TYPES: &[&str] = &[
                "mod",
                "modpack",
                "plugin",
                "resourcepack",
                "shader",
                "datapack",
            ];

            if PROJECT_TYPES.contains(&segments_vec[0]) {
                let project =
                    crate::database::models::Project::get(segments_vec[1], &pool, &redis).await?;

                if let Some(project) = project {
                    view.project_id = project.inner.id.0 as u64;
                }
            }
        }
    }

    if let Some((_, user)) = user {
        view.user_id = user.id.0;
    }

    analytics_queue.add_view(view);

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, Debug)]
pub struct PlaytimeInput {
    seconds: u16,
    loader: String,
    game_version: String,
    parent: Option<crate::models::ids::VersionId>,
}

pub async fn playtime_ingest(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(analytics_queue): Extension<Arc<AnalyticsQueue>>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Json(playtimes): Json<HashMap<crate::models::ids::VersionId, PlaytimeInput>>,
) -> Result<StatusCode, ApiError> {
    let (_, user) = get_user_from_headers(
        &addr,
        &headers,
        &pool,
        &redis,
        &session_queue,
        Some(&[Scopes::PERFORM_ANALYTICS]),
    )
    .await?;

    if playtimes.len() > 2000 {
        return Err(ApiError::InvalidInput(
            "Too much playtime entered for version!".to_string(),
        ));
    }

    let versions = crate::database::models::Version::get_many(
        &playtimes.iter().map(|x| (*x.0).into()).collect::<Vec<_>>(),
        &pool,
        &redis,
    )
    .await?;

    for (id, playtime) in playtimes {
        if playtime.seconds > 300 {
            continue;
        }

        if let Some(version) = versions.iter().find(|x| id == x.inner.id.into()) {
            analytics_queue.add_playtime(Playtime {
                recorded: get_current_tenths_of_ms(),
                seconds: playtime.seconds as u64,
                user_id: user.id.0,
                project_id: version.inner.project_id.0 as u64,
                version_id: version.inner.id.0 as u64,
                loader: playtime.loader,
                game_version: playtime.game_version,
                parent: playtime.parent.map(|x| x.0).unwrap_or(0),
            });
        }
    }

    Ok(StatusCode::NO_CONTENT)
}
