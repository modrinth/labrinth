use super::ids::{Base62Id, ProjectId};
use super::teams::TeamId;
use crate::database;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The ID of a specific collection, encoded as base62 for usage in the API
#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Base62Id")]
#[serde(into = "Base62Id")]
pub struct CollectionId(pub u64);

/// A collection returned from the API
#[derive(Serialize, Deserialize, Clone)]
pub struct Collection {
    /// The ID of the collection, encoded as a base62 string.
    pub id: CollectionId,
    /// The slug of a collection, used for vanity URLs
    pub slug: Option<String>,
    /// The team of people that has ownership of this collection.
    pub team: TeamId,
    /// The title or name of the collection.
    pub title: String,
    /// A short description of the collection.
    pub description: String,
    /// A long form description of the collection.
    pub body: String,

    /// An icon URL for the collection.
    pub icon_url: Option<String>,
    /// Color of the collection.
    pub color: Option<u32>,

    /// Whether the collection is public or not
    pub public: bool,

    /// The date at which the collection was first published.
    pub published: DateTime<Utc>,

    /// The date at which the collection was updated.
    pub updated: DateTime<Utc>,

    /// A list of ProjectIds that are in this collection.
    pub projects: Vec<ProjectId>,
}

impl From<database::models::Collection> for Collection {
    fn from(c: database::models::Collection) -> Self {
        Self {
            id: c.id.into(),
            slug: c.slug,
            team: c.team_id.into(),
            title: c.title,
            description: c.description,
            body: c.body,
            published: c.published,
            updated: c.updated,
            projects: c.projects.into_iter().map(|x| x.into()).collect(),
            icon_url: c.icon_url,
            color: c.color,
            public: c.public,
        }
    }
}