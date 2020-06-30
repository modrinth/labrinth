use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// TODO: use serde to parse this as base62
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub struct ModId(pub String);

// TODO: use serde to parse this as base62
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub struct UserId(String);

// TODO: what format should this be?
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub struct VersionId(String);

// TODO: permissions, role names, etc
#[derive(Serialize, Deserialize)]
pub struct Team {
    users: Vec<(String, UserId)>,
}

#[derive(Serialize, Deserialize)]
pub struct Mod {
    pub id: ModId,
    pub team: Team,

    pub title: String,
    pub description: String,
    pub published: DateTime<Utc>,

    pub downloads: u32,
    pub categories: Vec<String>,
    pub versions: Vec<VersionId>,

    pub body_url: String,
    pub icon_url: String,
}

#[derive(Serialize, Deserialize)]
pub struct Version {
    pub id: VersionId,
    pub mod_id: ModId,

    pub title: String,
    pub changelog_url: String,
    pub date_published: DateTime<Utc>,
    pub downloads: u32,
    pub files: Vec<VersionFile>,
    pub dependencies: Vec<ModId>,
    pub game_versions: Vec<GameVersion>,
}

/// A single mod file, with a url for the file and the file's hash
#[derive(Serialize, Deserialize)]
pub struct VersionFile {
    // TODO: hashing algorithm?
    pub hash: String,
    pub url: String,
}

/// A specific version of Minecraft
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub struct GameVersion(pub String);

#[derive(Serialize, Deserialize)]
pub struct SearchRequest {
    pub query: Option<String>,
    pub filters: Option<String>,
    pub version: Option<String>,
    pub offset: Option<String>,
    pub index: Option<String>,
}
