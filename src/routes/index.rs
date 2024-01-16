use axum::http::StatusCode;
use crate::util::extract::Json;
use serde_json::{json, Value};

pub async fn index_get() -> (StatusCode, Json<Value>) {
    (
        StatusCode::OK,
        Json(json!({
            "name": "modrinth-labrinth",
            "version": env!("CARGO_PKG_VERSION"),
            "documentation": "https://docs.modrinth.com",
            "about": "Welcome traveler!"
        })),
    )
}
