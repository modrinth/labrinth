use axum::routing::get;
use axum_prometheus::PrometheusMetricLayer;
use env_logger::Env;
use labrinth::database::redis::RedisPool;
use labrinth::file_hosting::S3Host;
use labrinth::search;
use labrinth::{check_env_vars, clickhouse, database, file_hosting, queue};
use log::{error, info};
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::compression::CompressionLayer;

#[derive(Clone)]
pub struct Pepper {
    pub pepper: String,
}

#[cfg(not(tarpaulin_include))]
#[tokio::main]
async fn main() -> std::io::Result<()> {
    dotenvy::dotenv().ok();
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    if check_env_vars() {
        error!("Some environment variables are missing!");
    }

    // DSN is from SENTRY_DSN env variable.
    // Has no effect if not set.
    // let sentry = sentry::init(sentry::ClientOptions {
    //     release: sentry::release_name!(),
    //     traces_sample_rate: 0.1,
    //     ..Default::default()
    // });
    // if sentry.is_enabled() {
    //     info!("Enabled Sentry integration");
    //     std::env::set_var("RUST_BACKTRACE", "1");
    // }

    info!(
        "Starting Labrinth on {}",
        dotenvy::var("BIND_ADDR").unwrap()
    );

    database::check_for_migrations()
        .await
        .expect("An error occurred while running migrations.");

    // Database Connector
    let pool = database::connect()
        .await
        .expect("Database connection failed");

    // Redis connector
    let redis_pool = RedisPool::new(None);

    let storage_backend = dotenvy::var("STORAGE_BACKEND").unwrap_or_else(|_| "local".to_string());

    let file_host: Arc<dyn file_hosting::FileHost + Send + Sync> = match storage_backend.as_str() {
        "backblaze" => Arc::new(
            file_hosting::BackblazeHost::new(
                &dotenvy::var("BACKBLAZE_KEY_ID").unwrap(),
                &dotenvy::var("BACKBLAZE_KEY").unwrap(),
                &dotenvy::var("BACKBLAZE_BUCKET_ID").unwrap(),
            )
            .await,
        ),
        "s3" => Arc::new(
            S3Host::new(
                &dotenvy::var("S3_BUCKET_NAME").unwrap(),
                &dotenvy::var("S3_REGION").unwrap(),
                &dotenvy::var("S3_URL").unwrap(),
                &dotenvy::var("S3_ACCESS_TOKEN").unwrap(),
                &dotenvy::var("S3_SECRET").unwrap(),
            )
            .unwrap(),
        ),
        "local" => Arc::new(file_hosting::MockHost::new()),
        _ => panic!("Invalid storage backend specified. Aborting startup!"),
    };

    info!("Initializing clickhouse connection");
    let clickhouse = clickhouse::init_client().await.unwrap();

    let maxmind_reader = Arc::new(queue::maxmind::MaxMindIndexer::new().await.unwrap());

    let search_config = search::SearchConfig::new(None);
    info!("Starting Actix HTTP server!");

    let labrinth_config = labrinth::app_setup(
        pool.clone(),
        redis_pool.clone(),
        search_config.clone(),
        clickhouse,
        file_host.clone(),
        maxmind_reader.clone(),
    );

    let (prometheus_layer, metric_handle) = PrometheusMetricLayer::pair();

    let app = labrinth::app_config(labrinth_config)
        .route("/metrics", get(|| async move { metric_handle.render() }))
        .layer(prometheus_layer)
        .layer(
            CompressionLayer::new()
                .br(true)
                .deflate(true)
                .gzip(true)
                .zstd(true),
        )
        .into_make_service_with_connect_info::<SocketAddr>();
    // TODO: wrap sentry here
    // TODO: ratelimiter

    // run our app with hyper, listening globally on port 3000
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await?;
    axum::serve(listener, app).await
}
