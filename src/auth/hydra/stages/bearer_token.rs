//! Minecraft bearer token
use crate::auth::hydra::HydraError;
use serde_json::json;

use super::REQWEST_CLIENT;

const MCSERVICES_AUTH_URL: &str = "https://api.minecraftservices.com/launcher/login";

pub async fn fetch_bearer(token: &str, uhs: &str) -> Result<String, HydraError> {
    let body = REQWEST_CLIENT
        .post(MCSERVICES_AUTH_URL)
        .json(&json!({
            "xtoken": format!("XBL3.0 x={};{}", uhs, token),
            "platform": "PC_LAUNCHER"
        }))
        .send()
        .await?
        .text()
        .await?;

    serde_json::from_str::<serde_json::Value>(&body)?
        .get("access_token")
        .and_then(serde_json::Value::as_str)
        .map(String::from)
        .ok_or(HydraError::Authorization(
            "Response didn't contain valid bearer token".to_string(),
        ))
}
