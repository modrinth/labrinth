use std::{pin::Pin, task::{Context, Poll}};

use async_trait::async_trait;
use axum::{extract::{FromRequest, multipart::{MultipartRejection, MultipartError}, Request}, http::HeaderMap};
use bytes::{Bytes, BytesMut};
use futures::Stream;
use futures_lite::StreamExt;

pub enum MultipartWrapper {
    Axum(axum::extract::Multipart),
    Labrinth(multer::Multipart<'static>)
}

pub enum FieldWrapper<'a> {
    Axum(axum::extract::multipart::Field<'a>),
    Labrinth(multer::Field<'static>, &'a mut MultipartWrapper)
}

#[derive(Debug, thiserror::Error)]
pub enum MultipartErrorWrapper {
    #[error("Axum Error: {0}")]
    Axum(MultipartError),
    #[error("Rerouting Error: {0}")]
    Labrinth(multer::Error)
}

impl MultipartErrorWrapper {
    pub fn from_multer(err: multer::Error) -> Self {
        Self::Labrinth(err)
    }
}

impl From<MultipartError> for MultipartErrorWrapper {
    fn from(err: MultipartError) -> Self {
        Self::Axum(err)
    }
}

#[async_trait]
impl<S> FromRequest<S> for MultipartWrapper
where
    S: Send + Sync,
{
    type Rejection = MultipartRejection;

    async fn from_request(req: Request, _state: &S) -> Result<Self, Self::Rejection> {
        axum::extract::Multipart::from_request(req, _state)
            .await.map(MultipartWrapper::Axum)
    }
}

impl MultipartWrapper {
    /// Yields the next [`Field`] if available.
    pub async fn next_field(&mut self) -> Result<Option<FieldWrapper<'_>>, MultipartErrorWrapper> {
        match self {
            MultipartWrapper::Axum(inner) => {
                let field = inner.next_field().await?;
                Ok(field.map(FieldWrapper::Axum))
            },
            MultipartWrapper::Labrinth(inner) => {
                let field = inner.next_field().await.map_err(MultipartErrorWrapper::from_multer)?;
                Ok(field.map( move |f| FieldWrapper::Labrinth(f, self)))
            }
        }
    }
}

impl Stream for FieldWrapper<'_> {
    type Item = Result<Bytes, MultipartErrorWrapper>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match *self {
            FieldWrapper::Axum(ref mut inner) => inner.poll_next(cx).map_err(MultipartErrorWrapper::from),
            FieldWrapper::Labrinth(ref mut inner, _) => inner.poll_next(cx).map_err(MultipartErrorWrapper::from_multer)
        }
    }
}

impl<'a> FieldWrapper<'a> {
        /// The field name found in the
        /// [`Content-Disposition`](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Content-Disposition)
        /// header.
        pub fn name(&self) -> Option<&str> {
            match self {
                FieldWrapper::Axum(inner) => inner.name(),
                FieldWrapper::Labrinth(inner, _) => inner.name()
            }
        }
    
        /// The file name found in the
        /// [`Content-Disposition`](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Content-Disposition)
        /// header.
        pub fn file_name(&self) -> Option<&str> {
            match self {
                FieldWrapper::Axum(inner) => inner.file_name(),
                FieldWrapper::Labrinth(inner, _) => inner.file_name()
            }
        }
    
        /// Get the [content type](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Content-Type) of the field.
        pub fn content_type(&self) -> Option<&str> {
            match self {
                FieldWrapper::Axum(inner) => inner.content_type().map(|m| m.as_ref()),
                FieldWrapper::Labrinth(inner, _) => inner.content_type().map(|m| m.as_ref())
            }
        }
    
        /// Get a map of headers as [`HeaderMap`].
        pub fn headers(&self) -> &HeaderMap {
            match self {
                FieldWrapper::Axum(inner) => inner.headers(),
                FieldWrapper::Labrinth(inner, _) => inner.headers()
            }
        }
    
        /// Get the full data of the field as [`Bytes`].
        pub async fn bytes(self) -> Result<Bytes, MultipartErrorWrapper> {
            match self {
                FieldWrapper::Axum(inner) => inner.bytes().await.map_err(MultipartErrorWrapper::from),
                FieldWrapper::Labrinth(inner, _) => inner.bytes().await.map_err(MultipartErrorWrapper::from_multer)
            }
        }
    
        /// Get the full field data as text.
        pub async fn text(self) -> Result<String, MultipartErrorWrapper> {
            match self {
                FieldWrapper::Axum(inner) => inner.text().await.map_err(MultipartErrorWrapper::from),
                FieldWrapper::Labrinth(inner, _) => inner.text().await.map_err(MultipartErrorWrapper::from_multer)
            }
        }
    
        /// Stream a chunk of the field data.
        ///
        /// When the field data has been exhausted, this will return [`None`].
        ///
        /// Note this does the same thing as `Field`'s [`Stream`] implementation.
        pub async fn chunk(&mut self) -> Result<Option<Bytes>, MultipartErrorWrapper> {
            match self {
                FieldWrapper::Axum(inner) => inner.chunk().await.map_err(MultipartErrorWrapper::from),
                FieldWrapper::Labrinth(inner, _) => inner.chunk().await.map_err(MultipartErrorWrapper::from_multer)
            }
        }
    
}

// Multipart functionality for axum
// Primarily for testing or some implementations of route-redirection
// (axum does not innately support building multipart)
// TODO: This is temporary in conversion to axum. Remove this and find a way to stream it- this loads the entire payload into memory
#[derive(Debug, Clone)]
pub struct MultipartBuildSegment {
    pub name: String,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub data: MultipartBuildSegmentData,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum MultipartBuildSegmentData {
    Text(String),
    Binary(Vec<u8>),
}

pub fn generate_multipart(data: impl IntoIterator<Item = MultipartBuildSegment>) -> MultipartWrapper {
    let mut boundary: String = String::from("----WebKitFormBoundary");
    boundary.push_str(&rand::random::<u64>().to_string());
    boundary.push_str(&rand::random::<u64>().to_string());
    boundary.push_str(&rand::random::<u64>().to_string());

    let mut payload = BytesMut::new();

    for segment in data {
        payload.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"",
                boundary = boundary,
                name = segment.name
            )
            .as_bytes(),
        );

        if let Some(filename) = &segment.filename {
            payload.extend_from_slice(
                format!("; filename=\"{filename}\"", filename = filename).as_bytes(),
            );
        }
        if let Some(content_type) = &segment.content_type {
            payload.extend_from_slice(
                format!(
                    "\r\nContent-Type: {content_type}",
                    content_type = content_type
                )
                .as_bytes(),
            );
        }
        payload.extend_from_slice(b"\r\n\r\n");

        match &segment.data {
            MultipartBuildSegmentData::Text(text) => {
                payload.extend_from_slice(text.as_bytes());
            }
            MultipartBuildSegmentData::Binary(binary) => {
                payload.extend_from_slice(binary);
            }
        }
        payload.extend_from_slice(b"\r\n");
    }
    payload.extend_from_slice(format!("--{boundary}--\r\n", boundary = boundary).as_bytes());

    let multipart = multer::Multipart::new(
        
        futures::stream::once(async move { Ok::<_, MultipartErrorWrapper>(Bytes::from(payload)) }),
        boundary,
    );

    //TODO: this should create a direct stream
    MultipartWrapper::Labrinth(multipart)
}