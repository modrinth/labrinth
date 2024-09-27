use crate::auth::validate::get_user_record_from_bearer_token;
use crate::auth::{get_user_from_headers, AuthenticationError};
use crate::database::redis::RedisPool;
use crate::models::analytics::Download;
use crate::models::ids::ProjectId;
use crate::models::pats::Scopes;
use crate::models::users::UserId;
use crate::queue::analytics::AnalyticsQueue;
use crate::queue::maxmind::MaxMindIndexer;
use crate::queue::session::AuthQueue;
use crate::routes::ApiError;
use crate::search::SearchConfig;
use crate::util::date::get_current_tenths_of_ms;
use crate::util::guards::admin_key_guard;
use actix_web::{patch, post, web, HttpRequest, HttpResponse};
use serde::Deserialize;
use sqlx::PgPool;
use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::Arc;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("admin")
            .service(count_download)
            .service(force_reindex)
            .service(gdpr_export),
    );
}

#[derive(Deserialize)]
pub struct DownloadBody {
    pub url: String,
    pub project_id: ProjectId,
    pub version_name: String,

    pub ip: String,
    pub headers: HashMap<String, String>,
}

// This is an internal route, cannot be used without key
#[patch("/_count-download", guard = "admin_key_guard")]
#[allow(clippy::too_many_arguments)]
pub async fn count_download(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    maxmind: web::Data<Arc<MaxMindIndexer>>,
    analytics_queue: web::Data<Arc<AnalyticsQueue>>,
    session_queue: web::Data<AuthQueue>,
    download_body: web::Json<DownloadBody>,
) -> Result<HttpResponse, ApiError> {
    let token = download_body
        .headers
        .iter()
        .find(|x| x.0.to_lowercase() == "authorization")
        .map(|x| &**x.1);

    let user = get_user_record_from_bearer_token(&req, token, &**pool, &redis, &session_queue)
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
    .fetch_optional(pool.as_ref())
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
    .fetch_optional(pool.as_ref())
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

    Ok(HttpResponse::NoContent().body(""))
}

#[post("/_force_reindex", guard = "admin_key_guard")]
pub async fn force_reindex(
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    config: web::Data<SearchConfig>,
) -> Result<HttpResponse, ApiError> {
    use crate::search::indexing::index_projects;
    let redis = redis.get_ref();
    index_projects(pool.as_ref().clone(), redis.clone(), &config).await?;
    Ok(HttpResponse::NoContent().finish())
}

#[derive(Deserialize)]
pub struct GDPRExport {
    pub user_id: UserId,
}

#[post("/_gdpr-export")]
pub async fn gdpr_export(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
    gdpr_export: web::Json<GDPRExport>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &*session_queue,
        Some(&[Scopes::USER_READ]),
    )
    .await?
    .1;

    if !user.role.is_admin() {
        return Err(ApiError::Authentication(
            AuthenticationError::InvalidCredentials,
        ));
    }

    let user = crate::database::models::User::get_id(gdpr_export.user_id.into(), &**pool, &redis)
        .await?
        .ok_or(ApiError::NotFound)?;
    let user_id = user.id;

    let user = crate::models::users::User::from_full(user);

    let collection_ids = crate::database::models::User::get_collections(user_id, &**pool).await?;
    let collections =
        crate::database::models::Collection::get_many(&collection_ids, &**pool, &redis)
            .await?
            .into_iter()
            .map(|x| crate::models::collections::Collection::from(x))
            .collect::<Vec<_>>();

    let follows = crate::database::models::User::get_follows(user_id, &**pool)
        .await?
        .into_iter()
        .map(|x| crate::models::ids::ProjectId::from(x))
        .collect::<Vec<_>>();

    let projects = crate::database::models::User::get_projects(user_id, &**pool, &redis)
        .await?
        .into_iter()
        .map(|x| crate::models::ids::ProjectId::from(x))
        .collect::<Vec<_>>();

    let org_ids = crate::database::models::User::get_organizations(user_id, &**pool).await?;
    let orgs = crate::database::models::organization_item::Organization::get_many_ids(
        &org_ids, &**pool, &redis,
    )
    .await?
    .into_iter()
    // TODO: add team members
    .map(|x| crate::models::organizations::Organization::from(x, vec![]))
    .collect::<Vec<_>>();

    let notifs = crate::database::models::notification_item::Notification::get_many_user(
        user_id, &**pool, &redis,
    )
    .await?
    .into_iter()
    .map(|x| crate::models::notifications::Notification::from(x))
    .collect::<Vec<_>>();

    let oauth_clients =
        crate::database::models::oauth_client_item::OAuthClient::get_all_user_clients(
            user_id, &**pool,
        )
        .await?
        .into_iter()
        .map(|x| crate::models::oauth_clients::OAuthClient::from(x))
        .collect::<Vec<_>>();

    let oauth_authorizations = crate::database::models::oauth_client_authorization_item::OAuthClientAuthorization::get_all_for_user(
        user_id, &**pool,
    )
        .await?
        .into_iter()
        .map(|x| crate::models::oauth_clients::OAuthClientAuthorization::from(x))
        .collect::<Vec<_>>();

    let pat_ids = crate::database::models::pat_item::PersonalAccessToken::get_user_pats(
        user_id, &**pool, &redis,
    )
    .await?;
    let pats = crate::database::models::pat_item::PersonalAccessToken::get_many_ids(
        &pat_ids, &**pool, &redis,
    )
    .await?
    .into_iter()
    .map(|x| crate::models::pats::PersonalAccessToken::from(x, false))
    .collect::<Vec<_>>();

    let payout_ids =
        crate::database::models::payout_item::Payout::get_all_for_user(user_id, &**pool).await?;

    let payouts = crate::database::models::payout_item::Payout::get_many(&payout_ids, &**pool)
        .await?
        .into_iter()
        .map(|x| crate::models::payouts::Payout::from(x))
        .collect::<Vec<_>>();

    let report_ids =
        crate::database::models::user_item::User::get_reports(user_id, &**pool).await?;
    let reports = crate::database::models::report_item::Report::get_many(&report_ids, &**pool)
        .await?
        .into_iter()
        .map(|x| crate::models::reports::Report::from(x))
        .collect::<Vec<_>>();

    let message_ids = sqlx::query!(
        "
        SELECT id FROM threads_messages WHERE author_id = $1 AND hide_identity = FALSE
        ",
        user_id.0
    )
    .fetch_all(pool.as_ref())
    .await?
    .into_iter()
    .map(|x| crate::database::models::ids::ThreadMessageId(x.id))
    .collect::<Vec<_>>();

    let messages =
        crate::database::models::thread_item::ThreadMessage::get_many(&message_ids, &**pool)
            .await?
            .into_iter()
            .map(|x| crate::models::threads::ThreadMessage::from(x, &user))
            .collect::<Vec<_>>();

    let uploaded_images_ids = sqlx::query!(
        "SELECT id FROM uploaded_images WHERE owner_id = $1",
        user_id.0
    )
    .fetch_all(pool.as_ref())
    .await?
    .into_iter()
    .map(|x| crate::database::models::ids::ImageId(x.id))
    .collect::<Vec<_>>();

    let uploaded_images =
        crate::database::models::image_item::Image::get_many(&uploaded_images_ids, &**pool, &redis)
            .await?
            .into_iter()
            .map(|x| crate::models::images::Image::from(x))
            .collect::<Vec<_>>();

    let subscriptions =
        crate::database::models::user_subscription_item::UserSubscriptionItem::get_all_user(
            user_id, &**pool,
        )
        .await?
        .into_iter()
        .map(|x| crate::models::billing::UserSubscription::from(x))
        .collect::<Vec<_>>();

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "user": user,
        "collections": collections,
        "follows": follows,
        "projects": projects,
        "orgs": orgs,
        "notifs": notifs,
        "oauth_clients": oauth_clients,
        "oauth_authorizations": oauth_authorizations,
        "pats": pats,
        "payouts": payouts,
        "reports": reports,
        "messages": messages,
        "uploaded_images": uploaded_images,
        "subscriptions": subscriptions,
    })))
}
