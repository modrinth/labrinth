use labrinth::database::redis::RedisPool;
use sqlx::{postgres::PgPoolOptions, Executor, PgPool};
use std::time::Duration;
use url::Url;

pub const ADMIN_USER_ID: i64 = 1;
pub const MOD_USER_ID: i64 = 2;
pub const USER_USER_ID: i64 = 3;
pub const FRIEND_USER_ID: i64 = 4;
pub const ENEMY_USER_ID: i64 = 5;

pub struct TemporaryDatabase {
    pub pool: PgPool,
    pub redis_pool: RedisPool,
    pub database_name: String,
}

impl TemporaryDatabase {
    pub async fn create() -> Self {
        let temp_database_name = generate_random_database_name();
        println!("Creating temporary database: {}", &temp_database_name);

        let database_url = dotenvy::var("DATABASE_URL").expect("No database URL");
        let mut url = Url::parse(&database_url).expect("Invalid database URL");
        let pool = PgPool::connect(&database_url)
            .await
            .expect("Connection to database failed");

        // Create the temporary database
        let create_db_query = format!("CREATE DATABASE {}", &temp_database_name);

        sqlx::query(&create_db_query)
            .execute(&pool)
            .await
            .expect("Database creation failed");

        pool.close().await;

        // Modify the URL to switch to the temporary database
        url.set_path(&format!("/{}", &temp_database_name));
        let temp_db_url = url.to_string();

        let pool = PgPoolOptions::new()
            .min_connections(0)
            .max_connections(4)
            .max_lifetime(Some(Duration::from_secs(60 * 60)))
            .connect(&temp_db_url)
            .await
            .expect("Connection to temporary database failed");

        // Performs migrations
        let migrations = sqlx::migrate!("./migrations");
        migrations.run(&pool).await.expect("Migrations failed");

        // Gets new Redis pool
        let redis_pool = RedisPool::new(Some(temp_database_name.clone()));

        Self {
            pool,
            database_name: temp_database_name,
            redis_pool,
        }
    }

    pub async fn create_with_dummy() -> Self {
        let db = Self::create().await;
        db.add_dummy_data().await;
        db
    }

    pub async fn cleanup(mut self) {
        let database_url = dotenvy::var("DATABASE_URL").expect("No database URL");
        self.pool.close().await;

        self.pool = PgPool::connect(&database_url)
            .await
            .expect("Connection to main database failed");

        // Forcibly terminate all existing connections to this version of the temporary database
        // We are done and deleting it, so we don't need them anymore
        let terminate_query = format!(
            "SELECT pg_terminate_backend(pg_stat_activity.pid) FROM pg_stat_activity WHERE datname = '{}' AND pid <> pg_backend_pid()",
            &self.database_name
        );
        sqlx::query(&terminate_query)
            .execute(&self.pool)
            .await
            .unwrap();

        // Execute the deletion query asynchronously
        let drop_db_query = format!("DROP DATABASE IF EXISTS {}", &self.database_name);
        sqlx::query(&drop_db_query)
            .execute(&self.pool)
            .await
            .expect("Database deletion failed");
    }

    /*
        Adds the following dummy data to the database:
        - 5 users (admin, mod, user, friend, enemy)
            - Admin and mod have special powers, the others do not
            - User is our mock user. Friend and enemy can be used to simulate a collaborator to user to be given permnissions on a project,
            whereas enemy might be banned or otherwise not given permission. (These are arbitrary and dependent on the test)
        - PATs for each of the five users, with full privileges (for testing purposes).
            - 'mrp_patadmin' for admin, etc
        - 1 game version (1.20.1)
        - 1 dummy project called 'testslug' (and testslug2) with the following properties:
        - several categories, tags, etc

        This is a test function, so it panics on error.
    */
    pub async fn add_dummy_data(&self) {
        let pool = &self.pool.clone();
        pool.execute(include_str!("../files/dummy_data.sql"))
            .await
            .unwrap();
    }
}

fn generate_random_database_name() -> String {
    // Generate a random database name here
    // You can use your logic to create a unique name
    // For example, you can use a random string as you did before
    // or append a timestamp, etc.

    // We will use a random string starting with "labrinth_tests_db_"
    // and append a 6-digit number to it.
    let mut database_name = String::from("labrinth_tests_db_");
    database_name.push_str(&rand::random::<u64>().to_string()[..6]);
    database_name
}
