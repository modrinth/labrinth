use crate::models::error::ApiError;
use crate::util::extract::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;

pub async fn not_found() -> (StatusCode, Json<ApiError<'static>>) {
    (
        StatusCode::NOT_FOUND,
        Json(ApiError {
            error: "not_found",
            description: "the requested route does not exist".to_string(),
        }),
    )
}

pub async fn api_v1_gone() -> impl IntoResponse {
    (
        StatusCode::GONE,
        Json(
            ApiError {
                error:"api_deprecated",
                description: "You are using an application that uses an outdated version of Modrinth's API. Please either update it or switch to another application. For developers: https://docs.modrinth.com/docs/migrations/v1-to-v2/".to_string()
            }
        )
    )
}
