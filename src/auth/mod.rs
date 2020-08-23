use async_trait::async_trait;
use thiserror::Error;

pub mod administrative;
pub mod common;
pub mod health;
pub mod schemas;

#[derive(Error, Debug)]
pub enum KratosError {
    #[error("Error while sending HTTP request to authentication server")]
    HttpError(#[from] reqwest::Error),
    #[error("Authentication error: {0}")]
    AuthenticationError(serde_json::Value),
}
