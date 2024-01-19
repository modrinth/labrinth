#![allow(dead_code)]
// The structures for project/version creation.
// These are created differently, but are essentially the same between versions.

use axum_test::multipart::{MultipartForm, Part};

use crate::common::dummy_data::TestFile;

pub struct ProjectCreationRequestData {
    pub slug: String,
    pub jar: Option<TestFile>,
    pub multipart_data: MultipartForm,
}

pub struct VersionCreationRequestData {
    pub version: String,
    pub jar: Option<TestFile>,
    pub multipart_data: MultipartForm,
}

pub struct ImageData {
    pub filename: String,
    pub extension: String,
    pub icon: Vec<u8>,
}

// Converts a json and a jar into a multipart upload
pub fn get_public_creation_data_multipart(
    json_data: &serde_json::Value,
    version_jar: Option<&TestFile>,
) -> MultipartForm {
    let mut form = MultipartForm::new();

    // Basic json
    let part = Part::text(serde_json::to_string(json_data).unwrap()).mime_type("application/json");
    form = form.add_part("data", part);

    if let Some(jar) = version_jar {
        // Basic file
        let part = Part::bytes(jar.bytes())
            .file_name(jar.filename())
            .mime_type("application/java-archive");
        form = form.add_part(jar.filename(), part);
    }
    form
}
