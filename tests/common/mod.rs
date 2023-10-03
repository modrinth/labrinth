use labrinth::clickhouse;
use labrinth::{
    file_hosting, queue,
    util::env::{parse_strings_from_var, parse_var},
    LabrinthConfig,
};
use std::sync::Arc;

use self::database::TemporaryDatabase;

pub mod actix;
pub mod database;

pub async fn setup(db: &TemporaryDatabase) -> LabrinthConfig {
    println!("Setting up labrinth config");

    dotenvy::dotenv().ok();

    if check_test_vars() {
        println!("Some environment variables are missing!");
    }

    let pool = db.pool.clone();
    let redis_pool = db.redis_pool.clone();
    let file_host: Arc<dyn file_hosting::FileHost + Send + Sync> =
        Arc::new(file_hosting::MockHost::new());
    let mut clickhouse = clickhouse::init_client().await.unwrap();

    let maxmind_reader = Arc::new(queue::maxmind::MaxMindIndexer::new().await.unwrap());

    labrinth::app_setup(
        pool.clone(),
        redis_pool.clone(),
        &mut clickhouse,
        file_host.clone(),
        maxmind_reader.clone(),
    )
}

// This is so that env vars not used immediately don't panic at runtime
fn check_test_vars() -> bool {
    let mut failed = false;

    fn check_var<T: std::str::FromStr>(var: &'static str) -> bool {
        let check = parse_var::<T>(var).is_none();
        if check {
            println!(
                "Variable `{}` missing in dotenv or not of type `{}`",
                var,
                std::any::type_name::<T>()
            );
        }
        check
    }

    failed |= check_var::<String>("DATABASE_URL");
    failed |= check_var::<String>("MEILISEARCH_ADDR");
    failed |= check_var::<String>("MEILISEARCH_KEY");
    failed |= check_var::<String>("BIND_ADDR");
    failed |= check_var::<String>("SELF_ADDR");

    failed |= check_var::<String>("MOCK_FILE_PATH");

    failed |= check_var::<usize>("LOCAL_INDEX_INTERVAL");
    failed |= check_var::<usize>("VERSION_INDEX_INTERVAL");

    if parse_strings_from_var("WHITELISTED_MODPACK_DOMAINS").is_none() {
        println!("Variable `WHITELISTED_MODPACK_DOMAINS` missing in dotenv or not a json array of strings");
        failed |= true;
    }

    if parse_strings_from_var("ALLOWED_CALLBACK_URLS").is_none() {
        println!(
            "Variable `ALLOWED_CALLBACK_URLS` missing in dotenv or not a json array of strings"
        );
        failed |= true;
    }

    failed |= check_var::<String>("PAYPAL_API_URL");
    failed |= check_var::<String>("PAYPAL_CLIENT_ID");
    failed |= check_var::<String>("PAYPAL_CLIENT_SECRET");

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

    failed |= check_var::<String>("TURNSTILE_SECRET");

    failed |= check_var::<String>("SMTP_USERNAME");
    failed |= check_var::<String>("SMTP_PASSWORD");
    failed |= check_var::<String>("SMTP_HOST");

    failed |= check_var::<String>("SITE_VERIFY_EMAIL_PATH");
    failed |= check_var::<String>("SITE_RESET_PASSWORD_PATH");

    failed |= check_var::<String>("BEEHIIV_PUBLICATION_ID");
    failed |= check_var::<String>("BEEHIIV_API_KEY");

    if parse_strings_from_var("ANALYTICS_ALLOWED_ORIGINS").is_none() {
        println!(
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
