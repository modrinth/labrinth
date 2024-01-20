use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    database,
    models::ids::{Base62Id, UserId},
};

/// The ID of a specific profile, encoded as base62 for usage in the API
#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(from = "Base62Id")]
#[serde(into = "Base62Id")]
pub struct ClientProfileId(pub u64);

/// The ID of a specific profile link, encoded as base62 for usage in the API
#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(from = "Base62Id")]
#[serde(into = "Base62Id")]
pub struct ClientProfileLinkId(pub u64);

/// A project returned from the API
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
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

    /// The loader
    pub loader: String,

    /// Game-specific information
    #[serde(flatten)]
    pub game: ClientProfileMetadata,

    // The following fields are hidden if the user is not the owner
    /// The share links for this profile
    pub share_links: Option<Vec<ClientProfileShareLink>>,
    // Users that are associated with this profile
    pub users: Option<Vec<UserId>>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(tag = "game")]
pub enum ClientProfileMetadata {
    #[serde(rename = "minecraft-java")]
    Minecraft {
        /// Client game version id
        game_version: String,
        /// Loader version
        loader_version: String,
    },
    #[serde(rename = "unknown")]
    Unknown,
}

impl From<database::models::client_profile_item::ClientProfileMetadata> for ClientProfileMetadata {
    fn from(game: database::models::client_profile_item::ClientProfileMetadata) -> Self {
        match game {
            database::models::client_profile_item::ClientProfileMetadata::Minecraft {
                loader_version,
                game_version,
                ..
            } => Self::Minecraft {
                loader_version,
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
        profile: database::models::client_profile_item::QueryClientProfile,
        current_user_id: Option<database::models::ids::UserId>,
    ) -> Self {
        let mut users = None;
        let mut share_links = None;
        if Some(profile.inner.owner_id) == current_user_id {
            users = Some(profile.inner.users.into_iter().map(|v| v.into()).collect());
            share_links = Some(profile.links.into_iter().map(|v| v.into()).collect());
        };

        Self {
            id: profile.inner.id.into(),
            owner_id: profile.inner.owner_id.into(),
            name: profile.inner.name,
            created: profile.inner.created,
            updated: profile.inner.updated,
            icon_url: profile.inner.icon_url,
            users,
            loader: profile.inner.loader,
            game: profile.inner.metadata.into(),
            share_links,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct ClientProfileShareLink {
    pub id: ClientProfileLinkId, // The url identifier, encoded as base62
    pub profile_id: ClientProfileId,
    pub created: DateTime<Utc>,
    pub expires: DateTime<Utc>,
}

impl From<database::models::client_profile_item::ClientProfileLink> for ClientProfileShareLink {
    fn from(link: database::models::client_profile_item::ClientProfileLink) -> Self {
        // Generate URL for easy access
        let profile_id: ClientProfileId = link.shared_profile_id.into();
        let link_id: ClientProfileLinkId = link.id.into();

        Self {
            id: link_id,
            profile_id,
            created: link.created,
            expires: link.expires,
        }
    }
}
