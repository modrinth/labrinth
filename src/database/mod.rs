mod mongo_database;
pub mod models;

pub use mongo_database::connect;
pub use models::Mod;
pub use models::Version;
use thiserror::Error;

type Result<T> = std::result::Result<T, DatabaseError>;

#[derive(Error, Debug)]
pub enum DatabaseError {
    #[error("Impossible to find document")]
    NotFound(),
    #[error("BSON deserialization error")]
    BsonError(#[from] bson::de::Error),
    #[error("Local database error")]
    LocalDatabaseError(#[from] mongodb::error::Error),
}
