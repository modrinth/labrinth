use bytes::{BytesMut, Bytes};

pub struct MultipartSegment {
    pub name : String,
    pub filename : Option<String>,
    pub content_type : Option<String>,
    pub data : MultipartSegmentData
}

pub enum MultipartSegmentData {
    Text(String),
    Binary(Vec<u8>)
}

pub fn generate_multipart(data: Vec<MultipartSegment>) -> (String, Bytes) {
    let mut boundary = String::from("----WebKitFormBoundary");
    boundary.push_str(&rand::random::<u64>().to_string());
    boundary.push_str(&rand::random::<u64>().to_string());
    boundary.push_str(&rand::random::<u64>().to_string());

    let mut payload = BytesMut::new();

    for segment in data {
        payload.extend_from_slice(format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"",
            boundary = boundary,
            name = segment.name
        ).as_bytes());

        if let Some(filename) = &segment.filename {
            payload.extend_from_slice(format!("; filename=\"{filename}\"", filename = filename).as_bytes());
        }
        if let Some(content_type) = &segment.content_type {
            payload.extend_from_slice(format!("\r\nContent-Type: {content_type}", content_type = content_type).as_bytes());
        }
        payload.extend_from_slice(b"\r\n\r\n");

        match &segment.data {
            MultipartSegmentData::Text(text) => {
                payload.extend_from_slice(text.as_bytes());
            },
            MultipartSegmentData::Binary(binary) => {
                payload.extend_from_slice(binary);
            }
        }
        payload.extend_from_slice(b"\r\n");
    }
    payload.extend_from_slice(format!("--{boundary}--\r\n", boundary = boundary).as_bytes());
    
    (boundary, Bytes::from(payload))
}
