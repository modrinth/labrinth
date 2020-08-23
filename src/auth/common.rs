use crate::auth::schemas::{
    ErrorContainer, LoginRequest, RecoveryRequest, RegistrationRequest, SettingsRequest,
};
use crate::auth::KratosError;
use serde_json::Value;

pub async fn get_schema(api_url: String, id: String) -> Result<Value, KratosError> {
    let response = reqwest::Client::new()
        .get(&format!("{}/schemas/{}", api_url, id))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .send()
        .await?;

    if response.status().is_success() {
        Ok(response.json().await?)
    } else {
        Err(KratosError::AuthenticationError(response.json().await?))
    }
}

pub async fn get_login_request(api_url: String, id: String) -> Result<LoginRequest, KratosError> {
    let response = reqwest::Client::new()
        .get(&format!(
            "{}/self-service/browser/flows/requests/login?request={}",
            api_url, id
        ))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .send()
        .await?;

    if response.status().is_success() {
        Ok(response.json().await?)
    } else {
        Err(KratosError::AuthenticationError(response.json().await?))
    }
}

pub async fn get_recovery_request(
    api_url: String,
    id: String,
) -> Result<RecoveryRequest, KratosError> {
    let response = reqwest::Client::new()
        .get(&format!(
            "{}/self-service/browser/flows/requests/login?request={}",
            api_url, id
        ))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .send()
        .await?;

    if response.status().is_success() {
        Ok(response.json().await?)
    } else {
        Err(KratosError::AuthenticationError(response.json().await?))
    }
}

pub async fn get_registration_request(
    api_url: String,
    id: String,
) -> Result<RegistrationRequest, KratosError> {
    let response = reqwest::Client::new()
        .get(&format!(
            "{}/self-service/browser/flows/requests/registration?request={}",
            api_url, id
        ))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .send()
        .await?;

    if response.status().is_success() {
        Ok(response.json().await?)
    } else {
        Err(KratosError::AuthenticationError(response.json().await?))
    }
}

pub async fn get_settings_request(
    api_url: String,
    id: String,
) -> Result<SettingsRequest, KratosError> {
    let response = reqwest::Client::new()
        .get(&format!(
            "{}/self-service/browser/flows/requests/settings?request={}",
            api_url, id
        ))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .send()
        .await?;

    if response.status().is_success() {
        Ok(response.json().await?)
    } else {
        Err(KratosError::AuthenticationError(response.json().await?))
    }
}

pub async fn get_verification_request(
    api_url: String,
    id: String,
) -> Result<SettingsRequest, KratosError> {
    let response = reqwest::Client::new()
        .get(&format!(
            "{}/self-service/browser/flows/requests/verification?request={}",
            api_url, id
        ))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .send()
        .await?;

    if response.status().is_success() {
        Ok(response.json().await?)
    } else {
        Err(KratosError::AuthenticationError(response.json().await?))
    }
}

pub async fn get_user_error(api_url: String, error: String) -> Result<ErrorContainer, KratosError> {
    let response = reqwest::Client::new()
        .get(&format!(
            "{}/self-service/errors?request={}",
            api_url, error
        ))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .send()
        .await?;

    if response.status().is_success() {
        Ok(response.json().await?)
    } else {
        Err(KratosError::AuthenticationError(response.json().await?))
    }
}
