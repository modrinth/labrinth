use crate::auth::schemas::Identity;
use crate::auth::KratosError;

pub async fn get_identities(api_url: String) -> Result<Vec<Identity>, KratosError> {
    let response = reqwest::Client::new()
        .get(&format!("{}/identities", api_url))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .send()
        .await?;

    if response.status().is_success() {
        Ok(response.json().await?)
    } else {
        Err(KratosError::AuthenticationError(response.json().await?))
    }
}

pub async fn create_identity(api_url: String, identity: Identity) -> Result<Identity, KratosError> {
    let response = reqwest::Client::new()
        .post(&format!("{}/identities", api_url))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .json(&identity)
        .send()
        .await?;

    if response.status().is_success() {
        Ok(response.json().await?)
    } else {
        Err(KratosError::AuthenticationError(response.json().await?))
    }
}

pub async fn get_identity(api_url: String, id: String) -> Result<Identity, KratosError> {
    let response = reqwest::Client::new()
        .put(&format!("{}/identities/{}", api_url, id))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .send()
        .await?;

    if response.status().is_success() {
        Ok(response.json().await?)
    } else {
        Err(KratosError::AuthenticationError(response.json().await?))
    }
}

pub async fn update_identity(api_url: String, identity: Identity) -> Result<Identity, KratosError> {
    let response = reqwest::Client::new()
        .post(&format!("{}/identities/{:?}", api_url, identity.id))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .json(&identity)
        .send()
        .await?;

    if response.status().is_success() {
        Ok(response.json().await?)
    } else {
        Err(KratosError::AuthenticationError(response.json().await?))
    }
}

pub async fn delete_identity(api_url: String, id: String) -> Result<(), KratosError> {
    let response = reqwest::Client::new()
        .delete(&format!("{}/identities/{:?}", api_url, id))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .send()
        .await?;

    if response.status().is_success() {
        Ok(())
    } else {
        Err(KratosError::AuthenticationError(response.json().await?))
    }
}
