use axum::extract::FromRequest;
use axum::extract::FromRequestParts;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
pub use axum::extract::ConnectInfo;

#[derive(FromRequest, FromRequestParts)]
#[from_request(via(axum::Json), rejection(crate::routes::ApiError))]
pub struct Json<T>(pub T);

impl<T> IntoResponse for Json<T>
where
    T: Serialize,
{
    fn into_response(self) -> Response {
        axum::Json::<T>::into_response(axum::Json(self.0))
    }
}

#[derive(FromRequest, FromRequestParts)]
#[from_request(via(axum::Form), rejection(crate::routes::ApiError))]
pub struct Form<T>(pub T);

#[derive(FromRequest, FromRequestParts)]
#[from_request(via(axum::extract::Path), rejection(crate::routes::ApiError))]
pub struct Path<T>(pub T);

#[derive(FromRequest, FromRequestParts)]
#[from_request(via(axum::extract::Query), rejection(crate::routes::ApiError))]
pub struct Query<T>(pub T);

#[derive(FromRequest, FromRequestParts)]
#[from_request(via(axum::Extension), rejection(crate::routes::ApiError))]
pub struct Extension<T>(pub T);

// #[derive(FromRequest, FromRequestParts)]
// #[from_request(via(axum::extract::ConnectInfo), rejection(crate::routes::ApiError))]
// pub struct ConnectInfo<T>(pub T);

#[derive(FromRequest)]
#[from_request(rejection(crate::routes::ApiError))]
pub struct WebSocketUpgrade(pub(crate) axum::extract::WebSocketUpgrade);

#[derive(FromRequest)]
#[from_request(rejection(crate::routes::ApiError))]
pub struct Multipart(axum::extract::Multipart);

#[derive(FromRequest)]
#[from_request(rejection(crate::routes::ApiError))]
pub struct StringExtract(String);

#[derive(FromRequest)]
#[from_request(rejection(crate::routes::ApiError))]
pub struct BytesExtract(pub(crate) bytes::Bytes);
