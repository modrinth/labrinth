use meilisearch_sdk::document::Document;
use serde::{Serialize, Deserialize};
//TODO: Move theses structs to the needed place, and convert everything to a standard mod.

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub url: String,
    pub thumbnail_url: String,
    pub is_default: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Category {
    pub name: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Author {
    pub name: String,
    pub url: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CurseVersion {
    pub game_version: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CurseForgeMod {
    pub id: i32,
    pub name: String,
    pub authors: Vec<Author>,
    pub attachments: Vec<Attachment>,
    pub website_url: String,
    pub summary: String,
    pub download_count: f32,
    pub categories: Vec<Category>,
    pub game_version_latest_files: Vec<CurseVersion>,
    pub date_created: String,
    pub date_modified: String,
    pub game_slug: String,
}

#[derive(Serialize, Deserialize)]
pub struct SearchRequest {
    pub query: Option<String>,
    pub filters: Option<String>,
    pub version: Option<String>,
    pub offset: Option<String>,
    pub index: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SearchMod {
    pub mod_id: i32,
    pub author: String,
    pub title: String,
    pub description: String,
    pub keywords: Vec<String>,
    pub versions: Vec<String>,
    pub downloads: i32,
    pub page_url: String,
    pub icon_url: String,
    pub author_url: String,
    pub date_created: String,
    pub created: i64,
    pub date_modified: String,
    pub updated: i64,
    pub latest_version: String,
    pub empty: String,
}

impl Document for SearchMod {
    type UIDType = i32;

    fn get_uid(&self) -> &Self::UIDType {
        &self.mod_id
    }
}