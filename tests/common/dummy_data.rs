use std::io::{Write, Cursor};

use actix_web::test::{self, TestRequest};
use labrinth::{models::projects::Project, models::projects::Version};
use serde_json::json;
use sqlx::Executor;
use zip::{write::FileOptions, ZipWriter, CompressionMethod};

use crate::common::{actix::AppendsMultipart, database::USER_USER_PAT};

use super::{
    actix::{MultipartSegment, MultipartSegmentData},
    environment::TestEnvironment,
    request_data::get_public_project_creation_data,
};

#[allow(dead_code)]
pub const DUMMY_CATEGORIES: &'static [&str] = &[
    "combat",
    "decoration",
    "economy",
    "food",
    "magic",
    "mobs",
    "optimization",
];

#[allow(dead_code)]
pub enum TestFile {
    DummyProjectAlpha,
    DummyProjectBeta,
    BasicMod,
    BasicModDifferent,
    // Randomly generates a valid .jar with a random hash. 
    // Unlike the other dummy jar files, this one is not a static file.
    // and BasicModRandom.bytes() will return a different file each time.
    BasicModRandom {
        filename: String,
        bytes: Vec<u8>, 
    },
    BasicModpackRandom {
        filename: String,
        bytes: Vec<u8>, 
    },
}

impl TestFile {
    pub fn build_random_jar() -> Self {
        let filename = format!("random-mod-{}.jar", rand::random::<u64>());

        let fabric_mod_json = serde_json::json!({
            "schemaVersion": 1,
            "id": filename,
            "version": "1.0.1",
          
            "name": filename,
            "description": "Does nothing",
            "authors": [
              "user"
            ],
            "contact": {
              "homepage": "https://www.modrinth.com",
              "sources": "https://www.modrinth.com",
              "issues": "https://www.modrinth.com"
            },
          
            "license": "MIT",
            "icon": "none.png",
          
            "environment": "client",
            "entrypoints": {
              "main": [
                "io.github.modrinth.Modrinth"
              ]
            },
            "depends": {
              "minecraft": ">=1.20-"
            }
          }
        ).to_string();

        // Create a simulated zip file
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut cursor);
            zip.start_file("fabric.mod.json", FileOptions::default().compression_method(CompressionMethod::Stored)).unwrap();
            zip.write_all(fabric_mod_json.as_bytes()).unwrap();
            zip.finish().unwrap();
        }
        let bytes = cursor.into_inner();

        TestFile::BasicModRandom {
            filename,
            bytes,
        }
    }

    pub fn build_random_mrpack() -> Self {
        let filename = format!("random-modpack-{}.mrpack", rand::random::<u64>());

        let modrinth_index_json = serde_json::json!({
            "formatVersion": 1,
            "game": "minecraft",
            "versionId": "1.20.1-9.6",
            "name": filename,
            "files": [],
            "dependencies": {
                "fabric-loader": "0.14.22",
                "minecraft": "1.20.1"
            }
        }
        ).to_string();

        // Create a simulated zip file
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut cursor);
            zip.start_file("modrinth.index.json", FileOptions::default().compression_method(CompressionMethod::Stored)).unwrap();
            zip.write_all(modrinth_index_json.as_bytes()).unwrap();
            zip.finish().unwrap();
        }
        let bytes = cursor.into_inner();

        TestFile::BasicModpackRandom {
            filename,
            bytes,
        }
    }

}

#[derive(Clone)]
pub struct DummyData {
    pub alpha_team_id: String,
    pub beta_team_id: String,

    pub alpha_project_id: String,
    pub beta_project_id: String,

    pub alpha_project_slug: String,
    pub beta_project_slug: String,

    pub alpha_version_id: String,
    pub beta_version_id: String,

    pub alpha_thread_id: String,
    pub beta_thread_id: String,

    pub alpha_file_hash: String,
    pub beta_file_hash: String,
}

pub async fn add_dummy_data(test_env: &TestEnvironment) -> DummyData {
    // Adds basic dummy data to the database directly with sql (user, pats)
    let pool = &test_env.db.pool.clone();
    pool.execute(include_str!("../files/dummy_data.sql"))
        .await
        .unwrap();

    let (alpha_project, alpha_version) = add_project_alpha(test_env).await;
    let (beta_project, beta_version) = add_project_beta(test_env).await;

    DummyData {
        alpha_team_id: alpha_project.team.to_string(),
        beta_team_id: beta_project.team.to_string(),

        alpha_project_id: alpha_project.id.to_string(),
        beta_project_id: beta_project.id.to_string(),

        alpha_project_slug: alpha_project.slug.unwrap(),
        beta_project_slug: beta_project.slug.unwrap(),

        alpha_version_id: alpha_version.id.to_string(),
        beta_version_id: beta_version.id.to_string(),

        alpha_thread_id: alpha_project.thread_id.to_string(),
        beta_thread_id: beta_project.thread_id.to_string(),

        alpha_file_hash: alpha_version.files[0].hashes["sha1"].clone(),
        beta_file_hash: beta_version.files[0].hashes["sha1"].clone(),
    }
}

pub async fn add_project_alpha(test_env: &TestEnvironment) -> (Project, Version) {
    test_env
        .v2
        .add_public_project(get_public_project_creation_data(
            "alpha",
            TestFile::DummyProjectAlpha,
        ))
        .await
}

pub async fn add_project_beta(test_env: &TestEnvironment) -> (Project, Version) {
    // Adds dummy data to the database with sqlx (projects, versions, threads)
    // Generate test project data.
    let jar = TestFile::DummyProjectBeta;
    let json_data = json!(
        {
            "title": "Test Project Beta",
            "slug": "beta",
            "description": "A dummy project for testing with.",
            "body": "This project is not-yet-approved, and versions are draft.",
            "client_side": "required",
            "server_side": "optional",
            "initial_versions": [{
                "file_parts": [jar.filename()],
                "version_number": "1.2.3",
                "version_title": "start",
                "status": "unlisted",
                "requested_status": "unlisted",
                "dependencies": [],
                "game_versions": ["1.20.1"] ,
                "release_channel": "release",
                "loaders": ["fabric"],
                "featured": true
            }],
            "status": "private",
            "requested_status": "private",
            "categories": [],
            "license_id": "MIT"
        }
    );

    // Basic json
    let json_segment = MultipartSegment {
        name: "data".to_string(),
        filename: None,
        content_type: Some("application/json".to_string()),
        data: MultipartSegmentData::Text(serde_json::to_string(&json_data).unwrap()),
    };

    // Basic file
    let file_segment = MultipartSegment {
        name: jar.filename(),
        filename: Some(jar.filename()),
        content_type: Some("application/java-archive".to_string()),
        data: MultipartSegmentData::Binary(jar.bytes()),
    };

    // Add a project.
    let req = TestRequest::post()
        .uri("/v2/project")
        .append_header(("Authorization", USER_USER_PAT))
        .set_multipart(vec![json_segment.clone(), file_segment.clone()])
        .to_request();
    let resp = test_env.call(req).await;

    assert_eq!(resp.status(), 200);

    // Get project
    let req = TestRequest::get()
        .uri("/v2/project/beta")
        .append_header(("Authorization", USER_USER_PAT))
        .to_request();
    let resp = test_env.call(req).await;
    let project: Project = test::read_body_json(resp).await;

    // Get project's versions
    let req = TestRequest::get()
        .uri("/v2/project/beta/version")
        .append_header(("Authorization", USER_USER_PAT))
        .to_request();
    let resp = test_env.call(req).await;
    let versions: Vec<Version> = test::read_body_json(resp).await;
    let version = versions.into_iter().next().unwrap();

    (project, version)
}

impl TestFile {
    pub fn filename(&self) -> String {
        match self {
            TestFile::DummyProjectAlpha => "dummy-project-alpha.jar",
            TestFile::DummyProjectBeta => "dummy-project-beta.jar",
            TestFile::BasicMod => "basic-mod.jar",
            TestFile::BasicModDifferent => "basic-mod-different.jar",
            TestFile::BasicModRandom { filename, .. } => filename,
            TestFile::BasicModpackRandom { filename, .. } => filename,
        }
        .to_string()
    }

    pub fn bytes(&self) -> Vec<u8> {
        match self {
            TestFile::DummyProjectAlpha => {
                include_bytes!("../../tests/files/dummy-project-alpha.jar").to_vec()
            }
            TestFile::DummyProjectBeta => {
                include_bytes!("../../tests/files/dummy-project-beta.jar").to_vec()
            }
            TestFile::BasicMod => include_bytes!("../../tests/files/basic-mod.jar").to_vec(),
            TestFile::BasicModDifferent => {
                include_bytes!("../../tests/files/basic-mod-different.jar").to_vec()
            },
            TestFile::BasicModRandom { bytes, .. } => bytes.clone(),
            TestFile::BasicModpackRandom { bytes, .. } => bytes.clone(),
        }
    }

    pub fn project_type(&self) -> String {
        match self {
            TestFile::DummyProjectAlpha => "mod",
            TestFile::DummyProjectBeta => "mod",
            TestFile::BasicMod => "mod",
            TestFile::BasicModDifferent => "mod",
            TestFile::BasicModRandom { .. } => "mod",

            TestFile::BasicModpackRandom { .. } => "modpack",
        }.to_string()
    }
}
