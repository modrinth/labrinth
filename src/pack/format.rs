use crate::models::mods::{ModId, SideType};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackFormat {
    pub format_type: String,
    pub format_version: i32,
    pub id: ModId,
    pub name: String,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub updates: Option<String>,
    pub release_date: Option<DateTime<Utc>>,

}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackFile {
    pub path: String,
    pub hashes: std::collections::HashMap<String, String>,
    pub env: Environment,
    pub downloads: Vec<String>,
}

#[derive(Serialize, Deserialize)]
pub struct Environment {
    pub client: SideType,
    pub server: SideType
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub enum PackDependency {
    Forge,
    FabricLoader,
    Minecraft,
}

impl std::fmt::Display for PackDependency {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(fmt, "{}", self.as_str())
    }
}

impl PackDependency {
    // These are constant, so this can remove unneccessary allocations (`to_string`)
    pub fn as_str(&self) -> &'static str {
        match self {
            PackDependency::Forge => "forge",
            PackDependency::FabricLoader => "fabric-loader",
            PackDependency::Minecraft => "minecraft",
        }
    }
}