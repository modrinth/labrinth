use actix_multipart::Multipart;
use actix_web::{web::Payload, HttpRequest};
use futures::{TryStreamExt, StreamExt};

use super::{v3::project_creation::CreateError, ApiError};

// const for ignore_headers
const IGNORE_HEADERS: [&str; 3] = [
    "content-type",
    "content-length",
    "accept-encoding",
];

pub async fn reroute_patch(url : &str, req : HttpRequest, json : serde_json::Value) -> Result<reqwest::Response, ApiError> {
    // Forwarding headers
    let mut headers = reqwest::header::HeaderMap::new();
    for (key, value) in req.headers() {
        if !IGNORE_HEADERS.contains(&key.as_str()) {
            headers.insert(key.clone(), value.clone());
        }
    }

    // Sending the request
    let client = reqwest::Client::new();
    Ok(client.patch(url)
        .headers(headers)
        .json(&json)
        .send()
        .await?)
}

pub async fn reroute_multipart(url : &str, req : HttpRequest, mut payload : Multipart, closure: impl Fn(&mut serde_json::Value)) -> Result<reqwest::Response, CreateError> {
    println!("print 3!");

    // Forwarding headers
    let mut headers = reqwest::header::HeaderMap::new();
    for (key, value) in req.headers() {
        if !IGNORE_HEADERS.contains(&key.as_str()) {
            headers.insert(key.clone(), value.clone());
        }
    }
    println!("print 4!");

    // Forwarding multipart data
    let mut body = reqwest::multipart::Form::new();
    println!("print 5!");

    // Data field
    if let Ok(Some(mut field)) = payload.try_next().await {
        // The first multipart field must be named "data" and contain a JSON
        let content_disposition = field.content_disposition();
        let name = content_disposition
            .get_name()
            .ok_or_else(|| CreateError::MissingValueError(String::from("Missing content name")))?;

        if name != "data" {
            return Err(CreateError::InvalidInput(String::from(
                "`data` field must come before file fields",
            )));
        }
        println!("print 7!");

        let mut data = Vec::new();
        while let Some(chunk) = field.next().await {
            data.extend_from_slice(&chunk.map_err(CreateError::MultipartError)?);
        }
        let mut data: serde_json::Value = serde_json::from_slice(&data)?;

        // Now that we have the json data, execute the closure
        closure(&mut data);

        // Re-encode the json data and add it to the body
        let data = serde_json::to_string(&data)?;
        body = body.part("data", reqwest::multipart::Part::text(data));
    }

    // Forward every other field exactly as is
    while let Ok(Some(field)) = payload.try_next().await {
        let content_type = field.content_type().map(|ct| ct.to_string()).unwrap_or("text/plain".to_string());
        let field_name = field.name().to_string();
        let content_disposition = field.content_disposition().clone();
        let filename = content_disposition.get_filename().unwrap_or_default().to_string();
        
        let bytes: Vec<u8> = field
            .map(|chunk| chunk.unwrap().to_vec())  // Convert each chunk to Vec<u8>
            .fold(Vec::new(), |mut acc, vec| {  // Collect all chunks into one Vec<u8>
                acc.extend(vec);
                async move { acc }
            })
            .await;
        
        let part = reqwest::multipart::Part::bytes(bytes)
            .file_name(filename)
            .mime_str(&content_type)
            .unwrap();
        
        body = body.part(field_name, part);
    }

    // Sending the request
    let client = reqwest::Client::new();
    Ok(client.post(url)
        .headers(headers)
        .multipart(body)
        .send()
        .await?)
}