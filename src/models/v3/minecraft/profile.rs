use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    database::{
        self,
        models::{LoaderFieldEnumValueId, LoaderId},
    },
    models::ids::{Base62Id, UserId, VersionId},
};

/// The ID of a specific profile, encoded as base62 for usage in the API
#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(from = "Base62Id")]
#[serde(into = "Base62Id")]
pub struct MinecraftProfileId(pub u64);

/// A project returned from the API
#[derive(Serialize, Deserialize, Clone)]
pub struct MinecraftProfile {
    /// The ID of the profile, encoded as a base62 string.
    pub id: MinecraftProfileId,

    /// The person that has ownership of this profile.
    pub owner_id: UserId,
    /// The title or name of the project.
    pub name: String,
    /// The date at which the project was first created.
    pub created: DateTime<Utc>,
    /// The date at which the project was last updated.
    pub updated: DateTime<Utc>,
    /// The icon of the project.
    pub icon_url: Option<String>,

    /// The loader id
    pub loader_id: LoaderId,
    /// The loader version
    pub loader_version: String,
    /// Minecraft game version id
    pub game_version_id: LoaderFieldEnumValueId,

    /// Modrinth-associated versions
    pub versions: Vec<VersionId>,
    /// Overrides for this profile- only install paths are given,
    /// hashes are looked up in the CDN by the client
    pub override_install_paths: Vec<PathBuf>,
}

impl From<database::models::minecraft_profile_item::MinecraftProfile> for MinecraftProfile {
    fn from(profile: database::models::minecraft_profile_item::MinecraftProfile) -> Self {
        Self {
            id: profile.id.into(),
            owner_id: profile.owner_id.into(),
            name: profile.name,
            created: profile.created,
            updated: profile.updated,
            icon_url: profile.icon_url,
            loader_id: profile.loader_id.into(),
            loader_version: profile.loader_version,
            game_version_id: profile.game_version_id.into(),
            versions: profile.versions.into_iter().map(Into::into).collect(),
            override_install_paths: profile
                .overrides
                .into_iter()
                .map(|(_, v)| v.into())
                .collect(),
        }
    }
}
