use axum::Json;
use crate::models::error::ApiError;

pub async fn not_found() -> Json<ApiError> {
   Json(ApiError {
        error: "not_found",
        description: "the requested route does not exist",
    })
}
