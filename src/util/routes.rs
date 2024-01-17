use crate::routes::v3::project_creation::CreateError;
use crate::routes::ApiError;
use bytes::{Bytes, BytesMut};
use futures::StreamExt;
use crate::util::extract::BytesExtract;

use super::multipart::FieldWrapper;

pub async fn read_from_payload(
    payload: BytesExtract,
    cap: usize,
    err_msg: &'static str,
) -> Result<Bytes, ApiError> {
    if payload.0.len() >= cap {
        return Err(ApiError::InvalidInput(String::from(err_msg)));
    }

    Ok(payload.0)
}

pub async fn read_from_field(
    field: &mut FieldWrapper<'_>,
    cap: usize,
    err_msg: &'static str,
) -> Result<BytesMut, CreateError> {
    let mut bytes = BytesMut::new();
    while let Some(chunk) = field.next().await {
        if bytes.len() >= cap {
            return Err(CreateError::InvalidInput(String::from(err_msg)));
        } else {
            bytes.extend_from_slice(&chunk?);
        }
    }
    Ok(bytes)
}
