pub mod models;
mod postgres_database;

pub use models::Mod;
pub use models::Version;
pub use postgres_database::connect;
use thiserror::Error;

type Result<T> = std::result::Result<T, sqlx::error::Error>;
