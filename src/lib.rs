

use std::sync::Arc;

use actix_web::web;
use database::redis::RedisPool;
use log::{warn, info};
use queue::{session::AuthQueue, socket::ActiveSockets, payouts::PayoutsQueue, analytics::AnalyticsQueue, download::DownloadQueue};
use scheduler::Scheduler;
use sqlx::Postgres;
use tokio::sync::{Mutex, RwLock};

extern crate clickhouse as clickhouse_crate;
use clickhouse_crate::Client;
use util::cors::default_cors;

use crate::{queue::payouts::process_payout, search::indexing::index_projects, util::env::parse_var};

pub mod auth;
pub mod clickhouse;
pub mod database;
pub mod file_hosting;
pub mod models;
pub mod queue;
pub mod ratelimit;
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
    pub scheduler: Arc<Scheduler>,
    pub ip_salt: Pepper,
    pub search_config: search::SearchConfig,
    pub download_queue: web::Data<DownloadQueue>,
    pub session_queue: web::Data<AuthQueue>,
    pub payouts_queue: web::Data<Mutex<PayoutsQueue>>,
    pub analytics_queue: Arc<AnalyticsQueue>,
    pub active_sockets: web::Data<RwLock<ActiveSockets>>,
}

pub fn app_setup(pool: sqlx::Pool<Postgres>, redis_pool: RedisPool, clickhouse : &mut Client, file_host : Arc<dyn file_hosting::FileHost + Send + Sync>, maxmind : Arc<queue::maxmind::MaxMindIndexer>) -> LabrinthConfig {

    info!(
        "Starting Labrinth on {}",
        dotenvy::var("BIND_ADDR").unwrap()
    );

    let search_config = search::SearchConfig {
        address: dotenvy::var("MEILISEARCH_ADDR").unwrap(),
        key: dotenvy::var("MEILISEARCH_KEY").unwrap(),
    };

    let mut scheduler = scheduler::Scheduler::new();

    // The interval in seconds at which the local database is indexed
    // for searching.  Defaults to 1 hour if unset.
    let local_index_interval =
        std::time::Duration::from_secs(parse_var("LOCAL_INDEX_INTERVAL").unwrap_or(3600));

    let pool_ref = pool.clone();
    let search_config_ref = search_config.clone();
    scheduler.run(local_index_interval, move || {
        let pool_ref = pool_ref.clone();
        let search_config_ref = search_config_ref.clone();
        async move {
            info!("Indexing local database");
            let result = index_projects(pool_ref, &search_config_ref).await;
            if let Err(e) = result {
                warn!("Local project indexing failed: {:?}", e);
            }
            info!("Done indexing local database");
        }
    });

    // Changes statuses of scheduled projects/versions
    let pool_ref = pool.clone();
    // TODO: Clear cache when these are run
    scheduler.run(std::time::Duration::from_secs(60 * 5), move || {
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

    scheduler::schedule_versions(&mut scheduler, pool.clone());

    let download_queue = web::Data::new(DownloadQueue::new());

    let pool_ref = pool.clone();
    let download_queue_ref = download_queue.clone();
    scheduler.run(std::time::Duration::from_secs(60 * 5), move || {
        let pool_ref = pool_ref.clone();
        let download_queue_ref = download_queue_ref.clone();

        async move {
            info!("Indexing download queue");
            let result = download_queue_ref.index(&pool_ref).await;
            if let Err(e) = result {
                warn!("Indexing download queue failed: {:?}", e);
            }
            info!("Done indexing download queue");
        }
    });

    let session_queue = web::Data::new(AuthQueue::new());

    let pool_ref = pool.clone();
    let redis_ref = redis_pool.clone();
    let session_queue_ref = session_queue.clone();
    scheduler.run(std::time::Duration::from_secs(60 * 30), move || {
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
        let reader_ref = reader.clone();
        scheduler.run(std::time::Duration::from_secs(60 * 60 * 24), move || {
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

    let analytics_queue = Arc::new(AnalyticsQueue::new());
    {
        let client_ref = clickhouse.clone();
        let analytics_queue_ref = analytics_queue.clone();
        scheduler.run(std::time::Duration::from_secs(60 * 5), move || {
            let client_ref = client_ref.clone();
            let analytics_queue_ref = analytics_queue_ref.clone();

            async move {
                info!("Indexing analytics queue");
                let result = analytics_queue_ref.index(client_ref).await;
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
        scheduler.run(std::time::Duration::from_secs(60 * 60 * 6), move || {
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

    let ip_salt = Pepper {
        pepper: models::ids::Base62Id(models::ids::random_base62(11)).to_string(),
    };

    let payouts_queue = web::Data::new(Mutex::new(PayoutsQueue::new()));
    let active_sockets = web::Data::new(RwLock::new(ActiveSockets::default()));

    LabrinthConfig {
        pool,
        redis_pool,
        clickhouse: clickhouse.clone(),
        file_host,
        maxmind,
        scheduler: Arc::new(scheduler),
        ip_salt,
        download_queue,
        search_config,
        session_queue,
        payouts_queue,
        analytics_queue,
        active_sockets,
    }

}

pub fn app_config(cfg: &mut web::ServiceConfig, labrinth_config: LabrinthConfig) {
    cfg.app_data(
        web::FormConfig::default().error_handler(|err, _req| {
            routes::ApiError::Validation(err.to_string()).into()
        }),
    )
    .app_data(
        web::PathConfig::default().error_handler(|err, _req| {
            routes::ApiError::Validation(err.to_string()).into()
        }),
    )
    .app_data(
        web::QueryConfig::default().error_handler(|err, _req| {
            routes::ApiError::Validation(err.to_string()).into()
        }),
    )
    .app_data(
        web::JsonConfig::default().error_handler(|err, _req| {
            routes::ApiError::Validation(err.to_string()).into()
        }),
    )
.app_data(web::Data::new(labrinth_config.redis_pool.clone()))
    .app_data(web::Data::new(labrinth_config.pool.clone()))
    .app_data(web::Data::new(labrinth_config.file_host.clone()))
    .app_data(web::Data::new(labrinth_config.search_config.clone()))
    .app_data(labrinth_config.download_queue.clone())
    .app_data(labrinth_config.session_queue.clone())
    .app_data(labrinth_config.payouts_queue.clone())
    .app_data(web::Data::new(labrinth_config.ip_salt.clone()))
    .app_data(web::Data::new(labrinth_config.analytics_queue.clone()))
    .app_data(web::Data::new(labrinth_config.clickhouse.clone()))
    .app_data(web::Data::new(labrinth_config.maxmind.clone()))
    .app_data(labrinth_config.active_sockets.clone())
    .configure(routes::v2::config)
    .configure(routes::v3::config)
    .configure(routes::root_config)
    .default_service(web::get().wrap(default_cors()).to(routes::not_found));
}
