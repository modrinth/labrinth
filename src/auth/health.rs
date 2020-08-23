use crate::auth::schemas::{HealthNotReadyStatus, HealthStatus, Version};
use crate::auth::KratosError;

pub async fn check_alive_status(api_url: String) -> Result<HealthStatus, KratosError> {
    let response = reqwest::Client::new()
        .get(&format!("{}/health/alive", api_url))
        .send()
        .await?;

    if response.status().is_success() {
        Ok(response.json().await?)
    } else {
        Err(KratosError::AuthenticationError(response.json().await?))
    }
}

pub async fn check_readiness_status(api_url: String) -> Result<HealthStatus, KratosError> {
    let response = reqwest::Client::new()
        .get(&format!("{}/health/ready", api_url))
        .send()
        .await?;

    if response.status().is_success() {
        Ok(response.json().await?)
    } else {
        Err(KratosError::AuthenticationError(response.json().await?))
    }
}

pub async fn get_service_version(api_url: String) -> Result<Version, KratosError> {
    let response = reqwest::Client::new()
        .get(&format!("{}/version", api_url))
        .send()
        .await?;

    Ok(response.json().await?)
}
