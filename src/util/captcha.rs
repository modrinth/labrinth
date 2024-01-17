use crate::routes::ApiError;
use crate::util::ip::get_ip_addr;
use axum::http::HeaderMap;
use serde::Deserialize;
use serde_json::json;
use std::net::SocketAddr;

pub async fn check_turnstile_captcha(
    addr: &SocketAddr,
    headers: &HeaderMap,
    challenge: &str,
) -> Result<bool, ApiError> {
    let ip_addr = get_ip_addr(addr, headers);

    let client = reqwest::Client::new();

    #[derive(Deserialize)]
    struct Response {
        success: bool,
    }

    let val: Response = client
        .post("https://challenges.cloudflare.com/turnstile/v0/siteverify")
        .json(&json!({
            "secret": dotenvy::var("TURNSTILE_SECRET")?,
            "response": challenge,
            "remoteip": ip_addr,
        }))
        .send()
        .await
        .map_err(|_| ApiError::Turnstile)?
        .json()
        .await
        .map_err(|_| ApiError::Turnstile)?;

    Ok(val.success)
}
