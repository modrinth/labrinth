use crate::routes::v3::project_creation::CreateError;
use crate::routes::ApiError;
use actix_multipart::Field;
use actix_web::http::header::CONTENT_LENGTH;
use actix_web::web::Payload;
use bytes::{Bytes, BytesMut};
use futures::StreamExt;

pub async fn read_from_payload(
    payload: Payload,
    cap: usize,
    err_msg: &'static str,
) -> Result<Bytes, ApiError> {
    payload
        .to_bytes_limited(cap)
        .await
        .map_err(|_| ApiError::InvalidInput(String::from(err_msg)))?
        .map_err(|_| ApiError::InvalidInput("Unable to parse bytes in payload sent!".to_string()))
}

pub async fn read_from_field(
    field: &mut Field,
    cap: usize,
    err_msg: &'static str,
) -> Result<Bytes, CreateError> {
    /// Sensible default (32kB) for initial, bounded allocation when collecting body bytes.
    const INITIAL_ALLOC_BYTES: usize = 32 * 1024;

    let capacity = match field.headers().get(&CONTENT_LENGTH) {
        None => INITIAL_ALLOC_BYTES,
        Some(len) => match len.to_str().ok().and_then(|len| len.parse::<u64>().ok()) {
            None => INITIAL_ALLOC_BYTES,
            Some(len) if len as usize > cap => {
                return Err(CreateError::InvalidInput(String::from(err_msg)))
            }
            Some(len) => (len as usize).min(INITIAL_ALLOC_BYTES),
        },
    };

    let mut bytes = BytesMut::with_capacity(capacity);
    while let Some(chunk) = field.next().await {
        if bytes.len() >= cap {
            return Err(CreateError::InvalidInput(String::from(err_msg)));
        } else {
            bytes.extend_from_slice(&chunk?);
        }
    }
    Ok(bytes.freeze())
}
