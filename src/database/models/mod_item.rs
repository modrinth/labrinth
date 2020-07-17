use crate::database::models::team_item::Team;
use crate::database::Result;
use bson::{Bson, Document};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
pub struct Mod {
    /// The ID for the mod, must be serializable to base62
    pub id: i32,
    //Todo: Move to own table
    /// The team that owns the mod
    pub team: Team,
    pub title: String,
    pub description: String,
    pub body_url: String,
    pub published: String,
    pub downloads: i32,
    pub categories: Vec<String>,
    ///A vector of Version IDs specifying the mod version of a dependency
    pub version_ids: Vec<i32>,
    pub icon_url: Option<String>,
    pub issues_url: Option<String>,
    pub source_url: Option<String>,
    pub wiki_url: Option<String>,
}
