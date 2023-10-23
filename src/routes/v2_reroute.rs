use actix_multipart::Multipart;
use actix_web::HttpResponse;
use actix_web::http::header::{TryIntoHeaderPair,HeaderMap};
use futures::{StreamExt, stream};
use serde_json::{Value, json};
use crate::{database::{models::{version_item, DatabaseError}, redis::RedisPool}, models::ids::VersionId, util::actix::{MultipartSegment, MultipartSegmentData, generate_multipart}};
use super::v3::project_creation::CreateError;

pub async fn set_side_types_from_versions<'a, E>(json : &mut serde_json::Value, exec: E, redis: &RedisPool) -> Result<(), DatabaseError>
where E : sqlx::Executor<'a, Database = sqlx::Postgres>
{
    json["client_side"] = json!("required"); // default to required
    json["server_side"] = json!("required");
    let version_id = json["versions"].as_array().and_then(|a| a.iter().next());
    if let Some(version_id) = version_id {
        let version_id = serde_json::from_value::<VersionId>(version_id.clone())?;
        let versions_item = version_item::Version::get(version_id.into(), exec, &redis).await?;
        if let Some(versions_item) = versions_item {
            json["client_side"] = versions_item.version_fields.iter().find(|f| f.field_name == "client_side").map(|f| f.value.serialize_internal()).unwrap_or(json!("required"));
            json["server_side"] = versions_item.version_fields.iter().find(|f| f.field_name == "server_side").map(|f| f.value.serialize_internal()).unwrap_or(json!("server_side"));
        }
    }
    Ok(())
}


// TODO: this is not an ideal way to do this, but it works for now
pub async fn extract_ok_json(response : HttpResponse) -> Result<serde_json::Value, HttpResponse> {
    if response.status() == actix_web::http::StatusCode::OK {
        let failure_http_response = || HttpResponse::InternalServerError().json(json!({
            "error": "reroute_error",
            "description": "Could not parse response from V2 redirection of route."
        }));
        // Takes json out of HttpResponse, mutates it, then regenerates the HttpResponse
        let body = response.into_body();
        let bytes = actix_web::body::to_bytes(body).await.map_err(|_| failure_http_response())?;
        let json_value: Value = serde_json::from_slice(&bytes).map_err(|_| failure_http_response())?;
        Ok(json_value)
    } else {
        Err(response)
    }
}

pub async fn alter_actix_multipart(mut multipart: Multipart, mut headers: HeaderMap,   mut closure: impl FnMut(&mut serde_json::Value)) -> Result<Multipart, CreateError> {
    let mut segments: Vec<MultipartSegment> = Vec::new();

    if let Some(field) = multipart.next().await {
        let mut field = field?;
        let content_disposition = field.content_disposition().clone(); // This unwrap is okay because we expect every field to have content disposition
        let field_name = content_disposition.get_name().unwrap_or("");
        let field_filename = content_disposition.get_filename();
        let field_content_type = field.content_type();
        let field_content_type = field_content_type.map(|ct| ct.to_string());

        let mut buffer = Vec::new();
        while let Some(chunk) = field.next().await {
            let data = chunk?;
            buffer.extend_from_slice(&data);
        }

        {
            let mut json_value: Value = serde_json::from_slice(&buffer)?;
            closure(&mut json_value);
            buffer = serde_json::to_vec(&json_value)?;
        }

        segments.push(MultipartSegment { name: field_name.to_string(),
             filename: field_filename.map(|s| s.to_string()),
             content_type: field_content_type, 
             data: MultipartSegmentData::Binary(buffer)
         })

    }

    while let Some(field) = multipart.next().await {
        let mut field = field?;
        let content_disposition = field.content_disposition().clone(); // This unwrap is okay because we expect every field to have content disposition
        let field_name = content_disposition.get_name().unwrap_or(""); // replace unwrap_or as you see fit
        let field_filename = content_disposition.get_filename();
        let field_content_type = field.content_type();
        let field_content_type = field_content_type.map(|ct| ct.to_string());

        let mut buffer = Vec::new();
        while let Some(chunk) = field.next().await {
            let data = chunk?;
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

    Ok(new_multipart)
}


