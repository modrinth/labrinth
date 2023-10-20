use std::borrow::BorrowMut;
use actix_multipart::Multipart;
use actix_web::{test::TestRequest, HttpResponse};
use bytes::{Bytes, BytesMut};
use actix_web::http::header::{TryIntoHeaderPair,HeaderMap, HeaderName};
use futures::{StreamExt, stream, Future};
use serde_json::{Value, json};
use actix_web::test;

use crate::{database::{models::{version_item, DatabaseError}, redis::RedisPool}, models::ids::VersionId};

use super::ApiError;


pub async fn set_side_types_from_versions<'a, E>(json : &mut serde_json::Value, exec: E, redis: &RedisPool) -> Result<(), DatabaseError>
where E : sqlx::Executor<'a, Database = sqlx::Postgres>
{
    json["client_side"] = json!("required"); // default to required
    json["server_side"] = json!("required");
    let version_id = json["versions"].as_array().and_then(|a| a.iter().next());
    if let Some(version_id) = version_id {
        let version_id = serde_json::from_value::<VersionId>(version_id.clone())?;
        let versions_item = version_item::Version::get(version_id.into(), exec, &redis).await?;
        println!("Got versions item: {:?}", serde_json::to_string(&versions_item));
        if let Some(versions_item) = versions_item {
            println!("Got versions item: {:?}", serde_json::to_string(&versions_item));
            json["client_side"] = versions_item.version_fields.iter().find(|f| f.field_name == "client_side").map(|f| f.value.serialize_internal()).unwrap_or(json!("required"));
            json["server_side"] = versions_item.version_fields.iter().find(|f| f.field_name == "server_side").map(|f| f.value.serialize_internal()).unwrap_or(json!("server_side"));
        }
    }
    Ok(())
}


// TODO: this is not an ideal way to do this, but it works for now
pub async fn extract_ok_json(mut response : HttpResponse) -> Result<serde_json::Value, HttpResponse> {
    if response.status() == actix_web::http::StatusCode::OK {
        // Takes json out of HttpResponse, mutates it, then regenerates the HttpResponse
        // actix client
        let body = response.into_body();
        let bytes = actix_web::body::to_bytes(body).await.unwrap();
        let mut json_value: Value = serde_json::from_slice(&bytes).unwrap();
        Ok(json_value)
    } else {
        Err(response)
    }
}

pub async fn alter_actix_multipart(mut multipart: Multipart, mut headers: HeaderMap,   mut closure: impl FnMut(&mut serde_json::Value)) -> Multipart {
    let mut segments: Vec<MultipartSegment> = Vec::new();

    if let Some(mut field) = multipart.next().await {
        let mut field = field.unwrap();
        let content_disposition = field.content_disposition().clone(); // This unwrap is okay because we expect every field to have content disposition
        let field_name = content_disposition.get_name().unwrap_or(""); // replace unwrap_or as you see fit
        let field_filename = content_disposition.get_filename();
        let field_content_type = field.content_type();
        let field_content_type = field_content_type.map(|ct| ct.to_string());

        let mut buffer = Vec::new();
        while let Some(chunk) = field.next().await {
            // let data = chunk.map_err(|e| ApiError::from(e))?;
            let data = chunk.unwrap();//.map_err(|e| ApiError::from(e))?;
            buffer.extend_from_slice(&data);
        }

        {
            let mut json_value: Value = serde_json::from_slice(&buffer).unwrap();
            closure(&mut json_value);
            buffer = serde_json::to_vec(&json_value).unwrap();
        }

        segments.push(MultipartSegment { name: field_name.to_string(),
             filename: field_filename.map(|s| s.to_string()),
             content_type: field_content_type, 
             data: MultipartSegmentData::Binary(buffer)
         })

    }

    while let Some(mut field) = multipart.next().await {
        let mut field = field.unwrap();
        let content_disposition = field.content_disposition().clone(); // This unwrap is okay because we expect every field to have content disposition
        let field_name = content_disposition.get_name().unwrap_or(""); // replace unwrap_or as you see fit
        let field_filename = content_disposition.get_filename();
        let field_content_type = field.content_type();
        let field_content_type = field_content_type.map(|ct| ct.to_string());

        let mut buffer = Vec::new();
        while let Some(chunk) = field.next().await {
            // let data = chunk.map_err(|e| ApiError::from(e))?;
            let data = chunk.unwrap();//.map_err(|e| ApiError::from(e))?;
            buffer.extend_from_slice(&data);
        }

        segments.push(MultipartSegment { name: field_name.to_string(),
             filename: field_filename.map(|s| s.to_string()),
             content_type: field_content_type, 
             data: MultipartSegmentData::Binary(buffer)
         })

    }

    let (boundary, payload) = generate_multipart(segments);

    match ("Content-Type", format!("multipart/form-data; boundary={}", boundary).as_str()).try_into_pair() {
        Ok((key, value)) => {
            headers.insert(key, value);
        }
        Err(err) => {
            panic!("Error inserting test header: {:?}.", err);
        }
    };

    let new_multipart = Multipart::new(&headers, stream::once(async { Ok(payload) }));

    new_multipart
}




// Multipart functionality (actix-test does not innately support multipart)
#[derive(Debug, Clone)]
pub struct MultipartSegment {
    pub name: String,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub data: MultipartSegmentData,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum MultipartSegmentData {
    Text(String),
    Binary(Vec<u8>),
}

pub trait AppendsMultipart {
    fn set_multipart(self, data: impl IntoIterator<Item = MultipartSegment>) -> Self;
}

impl AppendsMultipart for TestRequest {
    fn set_multipart(self, data: impl IntoIterator<Item = MultipartSegment>) -> Self {
        let (boundary, payload) = generate_multipart(data);
        self.append_header((
            "Content-Type",
            format!("multipart/form-data; boundary={}", boundary),
        ))
        .set_payload(payload)
    }
}

fn generate_multipart(data: impl IntoIterator<Item = MultipartSegment>) -> (String, Bytes) {
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
            MultipartSegmentData::Text(text) => {
                payload.extend_from_slice(text.as_bytes());
            }
            MultipartSegmentData::Binary(binary) => {
                payload.extend_from_slice(binary);
            }
        }
        payload.extend_from_slice(b"\r\n");
    }
    payload.extend_from_slice(format!("--{boundary}--\r\n", boundary = boundary).as_bytes());

    (boundary, Bytes::from(payload))
}
