use crate::auth::get_user_from_headers;
use crate::models::analytics::PageView;
use crate::models::pats::Scopes;
use crate::queue::maxmind::MaxMindIndexer;
use crate::queue::session::AuthQueue;
use crate::routes::ApiError;
use crate::util::env::parse_strings_from_var;
use crate::AnalyticsQueue;
use actix_web::{post, web};
use actix_web::{HttpRequest, HttpResponse};
use chrono::Utc;
use serde::Deserialize;
use sqlx::PgPool;
use std::collections::HashMap;
use std::net::{AddrParseError, IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::Arc;
use url::Url;
use uuid::Uuid;

// TODO: configure CORs with separate env variable for env funcs
pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(web::scope("analytics").service(page_view_ingest));
}

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
#[post("view")]
pub async fn page_view_ingest(
    req: HttpRequest,
    maxmind: web::Data<Arc<MaxMindIndexer>>,
    analytics_queue: web::Data<Arc<AnalyticsQueue>>,
    session_queue: web::Data<AuthQueue>,
    url_input: web::Json<UrlInput>,
    pool: web::Data<PgPool>,
    redis: web::Data<deadpool_redis::Pool>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(&req, &**pool, &redis, &session_queue, None)
        .await
        .ok();
    let conn_info = req.connection_info().peer_addr().map(|x| x.to_string());

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

    let headers = req
        .headers()
        .into_iter()
        .map(|(key, val)| {
            (
                key.to_string().to_lowercase(),
                val.to_str().unwrap_or_default().to_string(),
            )
        })
        .collect::<HashMap<String, String>>();

    let ip = convert_to_ip_v6(if let Some(header) = headers.get("cf-connecting-ip") {
        header
    } else {
        conn_info.as_deref().unwrap_or_default()
    })
    .unwrap_or_else(|_| Ipv4Addr::new(127, 0, 0, 1).to_ipv6_mapped());

    let mut view = PageView {
        id: Uuid::new_v4(),
        recorded: Utc::now().timestamp_nanos() / 100_000,
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
                    crate::database::models::Project::get(segments_vec[1], &**pool, &redis).await?;

                if let Some(project) = project {
                    view.project_id = project.inner.id.0 as u64;
                }
            }
        }
    }

    if let Some((_, user)) = user {
        view.user_id = user.id.0;
    }

    analytics_queue.add_view(view).await;

    Ok(HttpResponse::NoContent().body(""))
}

// TODO: add playtime ingest route
#[derive(Deserialize)]
pub struct PlaytimeInput {
    pub projects: HashMap<crate::models::ids::VersionId, u16>,
}

#[post("playtime")]
pub async fn playtime_ingest(
    req: HttpRequest,
    maxmind: web::Data<Arc<MaxMindIndexer>>,
    analytics_queue: web::Data<Arc<AnalyticsQueue>>,
    session_queue: web::Data<AuthQueue>,
    playtime_input: web::Json<PlaytimeInput>,
    pool: web::Data<PgPool>,
    redis: web::Data<deadpool_redis::Pool>,
) -> Result<HttpResponse, ApiError> {
    let (_, user) = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::PERFORM_ANALYTICS]),
    )
    .await?;

    // let versions = crate::database::models::Version::get_many(
    //     &playtime_input.projects.iter().map(|x| x.0).collect(),
    //     &**pool,
    //     &redis,
    // )
    // .await?;

    Ok(HttpResponse::NoContent().finish())
}
