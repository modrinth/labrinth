use crate::database::Result;
use bson::{Bson, Document};
use serde::{Deserialize, Serialize};

//TODO: Files should probably be moved to their own table
#[derive(Deserialize, Serialize)]
pub struct Version {
    ///The unqiue VersionId of this version
    pub version_id: i32,
    /// The ModId of the mod that this version belongs to
    pub mod_id: i32,
    pub name: String,
    pub number: String,
    pub changelog_url: Option<String>,
    pub date_published: String,
    pub downloads: i32,
    pub files: Vec<VersionFile>,
    pub dependencies: Vec<i32>,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub version_type: String,
}

#[derive(Serialize, Deserialize)]
pub struct VersionFile {
    pub hashes: Vec<FileHash>,
    pub url: String,
}

/// A hash of a mod's file
#[derive(Serialize, Deserialize)]
pub struct FileHash {
    pub algorithm: String,
    pub hash: String,
}
