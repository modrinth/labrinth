use crate::database::models::team_item::Team;
use crate::database::models::Item;
use crate::database::Result;
use bson::{Bson, Document};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
pub struct Mod {
    pub id: i32,
    //Todo: Move to own table
    pub team: Team,
    pub title: String,
    pub description: String,
    pub body_url: String,
    pub published: String,
    pub downloads: i32,
    pub categories: Vec<String>,
    pub version_ids: Vec<i32>,
    pub icon_url: Option<String>,
    pub issues_url: Option<String>,
    pub source_url: Option<String>,
    pub wiki_url: Option<String>,
}
impl Item for Mod {
    fn get_collection() -> &'static str {
        "mods"
    }

    fn from_doc(elem: Document) -> Result<Box<Mod>> {
        let result: Mod = bson::from_bson(Bson::from(elem))?;
        Ok(Box::from(result))
    }
}
