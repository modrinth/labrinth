use crate::models::error::ApiError;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;

pub async fn not_found() -> Json<ApiError<'static>> {
    Json(ApiError {
        error: "not_found",
        description: "the requested route does not exist",
    })
}

pub async fn api_v1_gone() -> impl IntoResponse {
    (
        StatusCode::GONE,
        Json(
            ApiError {
                error:"api_deprecated",
                description: "You are using an application that uses an outdated version of Modrinth's API. Please either update it or switch to another application. For developers: https://docs.modrinth.com/docs/migrations/v1-to-v2/"
            }
        )
    )
}
