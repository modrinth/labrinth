use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::ids::*;
use super::teams::Team;

#[derive(Serialize, Deserialize)]
pub struct Mod {
    pub id: ModId,
    // TODO: send partial team structure to reduce requests, but avoid sending
    // unnecessary info
    pub team: Team,

    pub title: String,
    pub description: String,
    pub published: DateTime<Utc>,

    pub downloads: u32,
    pub categories: Vec<String>,
    pub versions: Vec<VersionId>,

    pub issues_url: Option<String>,
    pub source_url: Option<String>,
    pub wiki_url: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct Version {
    pub id: VersionId,
    pub mod_id: ModId,

    pub title: String,
    pub changelog_url: String,
    pub date_published: DateTime<Utc>,
    pub downloads: u32,
    pub version_type: VersionType,

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

#[derive(Serialize, Deserialize)]
pub enum VersionType {
    Release,
    Beta,
    Alpha,
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
