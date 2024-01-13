use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    database,
    models::ids::{Base62Id, UserId, VersionId},
};

/// The ID of a specific profile, encoded as base62 for usage in the API
#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(from = "Base62Id")]
#[serde(into = "Base62Id")]
pub struct ClientProfileId(pub u64);

/// A project returned from the API
#[derive(Serialize, Deserialize, Clone)]
pub struct ClientProfile {
    /// The ID of the profile, encoded as a base62 string.
    pub id: ClientProfileId,

    /// The person that has ownership of this profile.
    pub owner_id: UserId,
    /// The title or name of the project.
    pub name: String,
    /// The date at which the project was first created.
    pub created: DateTime<Utc>,
    /// The date at which the project was last updated (versions/override were added/removed)
    pub updated: DateTime<Utc>,
    /// The icon of the project.
    pub icon_url: Option<String>,

    // Users that are associated with this profile
    // Hidden if the user is not the owner
    pub users: Option<Vec<UserId>>,

    /// The loader
    pub loader: String,
    /// The loader version
    pub loader_version: String,

    /// Game-specific information
    #[serde(flatten)]
    pub game: ClientProfileMetadata,

    /// Modrinth-associated versions
    pub versions: Vec<VersionId>,
    /// Overrides for this profile- only install paths are given,
    /// hashes are looked up in the CDN by the client
    pub override_install_paths: Vec<PathBuf>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "game")]
pub enum ClientProfileMetadata {
    #[serde(rename = "minecraft-java")]
    Minecraft {
        /// Game Id (constant for Minecraft)
        game_name: String,
        /// Client game version id
        game_version: String,
    },
    #[serde(rename = "unknown")]
    Unknown,
}

impl From<database::models::client_profile_item::ClientProfileMetadata> for ClientProfileMetadata {
    fn from(game: database::models::client_profile_item::ClientProfileMetadata) -> Self {
        match game {
            database::models::client_profile_item::ClientProfileMetadata::Minecraft {
                game_name,
                game_version,
                ..
            } => Self::Minecraft {
                game_name,
                game_version,
            },
            database::models::client_profile_item::ClientProfileMetadata::Unknown { .. } => {
                Self::Unknown
            }
        }
    }
}

impl ClientProfile {
    pub fn from(
        profile: database::models::client_profile_item::ClientProfile,
        current_user_id: Option<database::models::ids::UserId>,
    ) -> Self {
        let users = if Some(profile.owner_id) == current_user_id {
            Some(profile.users.into_iter().map(|v| v.into()).collect())
        } else {
            None
        };

        Self {
            id: profile.id.into(),
            owner_id: profile.owner_id.into(),
            name: profile.name,
            created: profile.created,
            updated: profile.updated,
            icon_url: profile.icon_url,
            users,
            loader: profile.loader,
            loader_version: profile.loader_version,
            game: profile.game.into(),
            versions: profile.versions.into_iter().map(Into::into).collect(),
            override_install_paths: profile.overrides.into_iter().map(|(_, v)| v).collect(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ClientProfileShareLink {
    pub url_identifier: String,
    pub url: String, // Includes the url identifier, intentionally redundant
    pub profile_id: ClientProfileId,
    pub created: DateTime<Utc>,
    pub expires: DateTime<Utc>,
}

impl From<database::models::client_profile_item::ClientProfileLink> for ClientProfileShareLink {
    fn from(link: database::models::client_profile_item::ClientProfileLink) -> Self {
        // Generate URL for easy access
        let profile_id: ClientProfileId = link.shared_profile_id.into();
        let url = format!(
            "{}/v3/client/profile/{}/accept/{}",
            dotenvy::var("SELF_ADDR").unwrap(),
            profile_id,
            link.link_identifier
        );

        Self {
            url_identifier: link.link_identifier,
            url,
            profile_id,
            created: link.created,
            expires: link.expires,
        }
    }
}
