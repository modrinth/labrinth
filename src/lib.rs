#![allow(clippy::too_many_arguments)]

use axum::extract::DefaultBodyLimit;
use axum::routing::any;
use axum::{Extension, Router};
use std::sync::Arc;

use database::redis::RedisPool;
use queue::{
    analytics::AnalyticsQueue, payouts::PayoutsQueue, session::AuthQueue, socket::ActiveSockets,
};
use sqlx::Postgres;
use tracing::{info, warn};

extern crate clickhouse as clickhouse_crate;
use clickhouse_crate::Client;

use crate::routes::not_found;
use crate::scheduler::schedule;
use crate::util::cors::default_cors;
use crate::{
    queue::payouts::process_payout,
    search::indexing::index_projects,
    util::env::{parse_strings_from_var, parse_var},
};

pub mod auth;
pub mod clickhouse;
pub mod database;
pub mod file_hosting;
pub mod models;
pub mod queue;
pub mod routes;
pub mod scheduler;
pub mod search;
pub mod util;
pub mod validate;

#[derive(Clone)]
pub struct Pepper {
    pub pepper: String,
}

#[derive(Clone)]
pub struct LabrinthConfig {
    pub pool: sqlx::Pool<Postgres>,
    pub redis_pool: RedisPool,
    pub clickhouse: Client,
    pub file_host: Arc<dyn file_hosting::FileHost + Send + Sync>,
    pub maxmind: Arc<queue::maxmind::MaxMindIndexer>,
    pub ip_salt: Pepper,
    pub search_config: search::SearchConfig,
    pub session_queue: Arc<AuthQueue>,
    pub payouts_queue: Arc<PayoutsQueue>,
    pub analytics_queue: Arc<AnalyticsQueue>,
    pub active_sockets: Arc<ActiveSockets>,
}

pub fn app_setup(
    pool: sqlx::Pool<Postgres>,
    redis_pool: RedisPool,
    search_config: search::SearchConfig,
    clickhouse: Client,
    file_host: Arc<dyn file_hosting::FileHost + Send + Sync>,
    maxmind: Arc<queue::maxmind::MaxMindIndexer>,
    testing: bool,
) -> LabrinthConfig {
    info!(
        "Starting Labrinth on {}",
        dotenvy::var("BIND_ADDR").unwrap()
    );

    // The interval in seconds at which the local database is indexed
    // for searching.  Defaults to 1 hour if unset.
    let local_index_interval =
        std::time::Duration::from_secs(parse_var("LOCAL_INDEX_INTERVAL").unwrap_or(3600));

    let session_queue = Arc::new(AuthQueue::new());
    let analytics_queue = Arc::new(AnalyticsQueue::new());

    let pool_ref = pool.clone();
    let search_config_ref = search_config.clone();
    let redis_pool_ref = redis_pool.clone();

    if !testing {
        schedule(local_index_interval, move || {
            let pool_ref = pool_ref.clone();
            let redis_pool_ref = redis_pool_ref.clone();
            let search_config_ref = search_config_ref.clone();
            async move {
                info!("Indexing local database");
                let result = index_projects(pool_ref, redis_pool_ref.clone(), &search_config_ref).await;
                if let Err(e) = result {
                    warn!("Local project indexing failed: {:?}", e);
                }
                info!("Done indexing local database");
            }
        });

        // Changes statuses of scheduled projects/versions
        let pool_ref = pool.clone();
        // TODO: Clear cache when these are run
        schedule(std::time::Duration::from_secs(60 * 5), move || {
            let pool_ref = pool_ref.clone();
            info!("Releasing scheduled versions/projects!");

            async move {
                let projects_results = sqlx::query!(
                    "
                    UPDATE mods
                    SET status = requested_status
                    WHERE status = $1 AND approved < CURRENT_DATE AND requested_status IS NOT NULL
                    ",
                    crate::models::projects::ProjectStatus::Scheduled.as_str(),
                )
                .execute(&pool_ref)
                .await;

                if let Err(e) = projects_results {
                    warn!("Syncing scheduled releases for projects failed: {:?}", e);
                }

                let versions_results = sqlx::query!(
                    "
                    UPDATE versions
                    SET status = requested_status
                    WHERE status = $1 AND date_published < CURRENT_DATE AND requested_status IS NOT NULL
                    ",
                    crate::models::projects::VersionStatus::Scheduled.as_str(),
                )
                .execute(&pool_ref)
                .await;

                if let Err(e) = versions_results {
                    warn!("Syncing scheduled releases for versions failed: {:?}", e);
                }

                info!("Finished releasing scheduled versions/projects");
            }
        });

        scheduler::schedule_versions(pool.clone(), redis_pool.clone());

        let pool_ref = pool.clone();
        let redis_ref = redis_pool.clone();
        let session_queue_ref = session_queue.clone();
        schedule(std::time::Duration::from_secs(60 * 30), move || {
            let pool_ref = pool_ref.clone();
            let redis_ref = redis_ref.clone();
            let session_queue_ref = session_queue_ref.clone();

            async move {
                info!("Indexing sessions queue");
                let result = session_queue_ref.index(&pool_ref, &redis_ref).await;
                if let Err(e) = result {
                    warn!("Indexing sessions queue failed: {:?}", e);
                }
                info!("Done indexing sessions queue");
            }
        });

        let reader = maxmind.clone();
        {
            let reader_ref = reader;
            schedule(std::time::Duration::from_secs(60 * 60 * 24), move || {
                let reader_ref = reader_ref.clone();

                async move {
                    info!("Downloading MaxMind GeoLite2 country database");
                    let result = reader_ref.index().await;
                    if let Err(e) = result {
                        warn!(
                            "Downloading MaxMind GeoLite2 country database failed: {:?}",
                            e
                        );
                    }
                    info!("Done downloading MaxMind GeoLite2 country database");
                }
            });
        }
        info!("Downloading MaxMind GeoLite2 country database");

        {
            let client_ref = clickhouse.clone();
            let analytics_queue_ref = analytics_queue.clone();
            let pool_ref = pool.clone();
            let redis_ref = redis_pool.clone();
            schedule(std::time::Duration::from_secs(15), move || {
                let client_ref = client_ref.clone();
                let analytics_queue_ref = analytics_queue_ref.clone();
                let pool_ref = pool_ref.clone();
                let redis_ref = redis_ref.clone();

                async move {
                    info!("Indexing analytics queue");
                    let result = analytics_queue_ref
                        .index(client_ref, &redis_ref, &pool_ref)
                        .await;
                    if let Err(e) = result {
                        warn!("Indexing analytics queue failed: {:?}", e);
                    }
                    info!("Done indexing analytics queue");
                }
            });
        }

        {
            let pool_ref = pool.clone();
            let redis_ref = redis_pool.clone();
            let client_ref = clickhouse.clone();
            schedule(std::time::Duration::from_secs(60 * 60 * 6), move || {
                let pool_ref = pool_ref.clone();
                let redis_ref = redis_ref.clone();
                let client_ref = client_ref.clone();

                async move {
                    info!("Started running payouts");
                    let result = process_payout(&pool_ref, &redis_ref, &client_ref).await;
                    if let Err(e) = result {
                        warn!("Payouts run failed: {:?}", e);
                    }
                    info!("Done running payouts");
                }
            });
        }
    }

    let ip_salt = Pepper {
        pepper: models::ids::Base62Id(models::ids::random_base62(11)).to_string(),
    };

    let payouts_queue = Arc::new(PayoutsQueue::new());
    let active_sockets = Arc::new(ActiveSockets::default());

    LabrinthConfig {
        pool,
        redis_pool,
        clickhouse: clickhouse.clone(),
        file_host,
        maxmind,
        ip_salt,
        search_config,
        session_queue,
        payouts_queue,
        analytics_queue,
        active_sockets,
    }
}

pub fn app_config(labrinth_config: LabrinthConfig) -> Router {
    Router::new()
        .merge(routes::v2::config())
        .merge(routes::v3::config())
        .merge(routes::internal::config())
        .merge(routes::root_config())
        .fallback_service(any(not_found).layer(default_cors()))
        .layer(Extension(labrinth_config.redis_pool.clone()))
        .layer(Extension(labrinth_config.pool.clone()))
        .layer(Extension(labrinth_config.file_host.clone()))
        .layer(Extension(labrinth_config.search_config.clone()))
        .layer(Extension(labrinth_config.session_queue.clone()))
        .layer(Extension(labrinth_config.payouts_queue.clone()))
        .layer(Extension(labrinth_config.ip_salt.clone()))
        .layer(Extension(labrinth_config.analytics_queue.clone()))
        .layer(Extension(labrinth_config.clickhouse.clone()))
        .layer(Extension(labrinth_config.maxmind.clone()))
        .layer(Extension(labrinth_config.active_sockets.clone()))
        .layer(DefaultBodyLimit::max(5 * 1024))
}

// This is so that env vars not used immediately don't panic at runtime
pub fn check_env_vars() -> bool {
    let mut failed = false;

    fn check_var<T: std::str::FromStr>(var: &'static str) -> bool {
        let check = parse_var::<T>(var).is_none();
        if check {
            warn!(
                "Variable `{}` missing in dotenv or not of type `{}`",
                var,
                std::any::type_name::<T>()
            );
        }
        check
    }

    failed |= check_var::<String>("SITE_URL");
    failed |= check_var::<String>("CDN_URL");
    failed |= check_var::<String>("LABRINTH_ADMIN_KEY");
    failed |= check_var::<String>("RATE_LIMIT_IGNORE_KEY");
    failed |= check_var::<String>("DATABASE_URL");
    failed |= check_var::<String>("MEILISEARCH_ADDR");
    failed |= check_var::<String>("MEILISEARCH_KEY");
    failed |= check_var::<String>("REDIS_URL");
    failed |= check_var::<String>("BIND_ADDR");
    failed |= check_var::<String>("SELF_ADDR");

    failed |= check_var::<String>("STORAGE_BACKEND");

    let storage_backend = dotenvy::var("STORAGE_BACKEND").ok();
    match storage_backend.as_deref() {
        Some("backblaze") => {
            failed |= check_var::<String>("BACKBLAZE_KEY_ID");
            failed |= check_var::<String>("BACKBLAZE_KEY");
            failed |= check_var::<String>("BACKBLAZE_BUCKET_ID");
        }
        Some("s3") => {
            failed |= check_var::<String>("S3_ACCESS_TOKEN");
            failed |= check_var::<String>("S3_SECRET");
            failed |= check_var::<String>("S3_URL");
            failed |= check_var::<String>("S3_REGION");
            failed |= check_var::<String>("S3_BUCKET_NAME");
        }
        Some("local") => {
            failed |= check_var::<String>("MOCK_FILE_PATH");
        }
        Some(backend) => {
            warn!("Variable `STORAGE_BACKEND` contains an invalid value: {}. Expected \"backblaze\", \"s3\", or \"local\".", backend);
            failed |= true;
        }
        _ => {
            warn!("Variable `STORAGE_BACKEND` is not set!");
            failed |= true;
        }
    }

    failed |= check_var::<usize>("LOCAL_INDEX_INTERVAL");
    failed |= check_var::<usize>("VERSION_INDEX_INTERVAL");

    if parse_strings_from_var("WHITELISTED_MODPACK_DOMAINS").is_none() {
        warn!("Variable `WHITELISTED_MODPACK_DOMAINS` missing in dotenv or not a json array of strings");
        failed |= true;
    }

    if parse_strings_from_var("ALLOWED_CALLBACK_URLS").is_none() {
        warn!("Variable `ALLOWED_CALLBACK_URLS` missing in dotenv or not a json array of strings");
        failed |= true;
    }

    failed |= check_var::<String>("GITHUB_CLIENT_ID");
    failed |= check_var::<String>("GITHUB_CLIENT_SECRET");
    failed |= check_var::<String>("GITLAB_CLIENT_ID");
    failed |= check_var::<String>("GITLAB_CLIENT_SECRET");
    failed |= check_var::<String>("DISCORD_CLIENT_ID");
    failed |= check_var::<String>("DISCORD_CLIENT_SECRET");
    failed |= check_var::<String>("MICROSOFT_CLIENT_ID");
    failed |= check_var::<String>("MICROSOFT_CLIENT_SECRET");
    failed |= check_var::<String>("GOOGLE_CLIENT_ID");
    failed |= check_var::<String>("GOOGLE_CLIENT_SECRET");
    failed |= check_var::<String>("STEAM_API_KEY");

    failed |= check_var::<String>("TREMENDOUS_API_URL");
    failed |= check_var::<String>("TREMENDOUS_API_KEY");
    failed |= check_var::<String>("TREMENDOUS_PRIVATE_KEY");

    failed |= check_var::<String>("PAYPAL_API_URL");
    failed |= check_var::<String>("PAYPAL_WEBHOOK_ID");
    failed |= check_var::<String>("PAYPAL_CLIENT_ID");
    failed |= check_var::<String>("PAYPAL_CLIENT_SECRET");

    failed |= check_var::<String>("TURNSTILE_SECRET");

    failed |= check_var::<String>("SMTP_USERNAME");
    failed |= check_var::<String>("SMTP_PASSWORD");
    failed |= check_var::<String>("SMTP_HOST");

    failed |= check_var::<String>("SITE_VERIFY_EMAIL_PATH");
    failed |= check_var::<String>("SITE_RESET_PASSWORD_PATH");

    failed |= check_var::<String>("BEEHIIV_PUBLICATION_ID");
    failed |= check_var::<String>("BEEHIIV_API_KEY");

    if parse_strings_from_var("ANALYTICS_ALLOWED_ORIGINS").is_none() {
        warn!(
            "Variable `ANALYTICS_ALLOWED_ORIGINS` missing in dotenv or not a json array of strings"
        );
        failed |= true;
    }

    failed |= check_var::<String>("CLICKHOUSE_URL");
    failed |= check_var::<String>("CLICKHOUSE_USER");
    failed |= check_var::<String>("CLICKHOUSE_PASSWORD");
    failed |= check_var::<String>("CLICKHOUSE_DATABASE");

    failed |= check_var::<String>("MAXMIND_LICENSE_KEY");

    failed |= check_var::<u64>("PAYOUTS_BUDGET");

    failed
}
