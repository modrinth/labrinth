use super::ids::Base62Id;
use crate::database::models::image_item::Image as DBImage;
use crate::models::ids::UserId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Base62Id")]
#[serde(into = "Base62Id")]
pub struct ImageId(pub u64);

#[derive(Serialize, Deserialize)]
pub struct Image {
    pub id: ImageId,
    pub url: String,
    pub size: u64,
    pub created: DateTime<Utc>,
    pub owner_id: UserId,
}

impl From<DBImage> for Image {
    fn from(x: DBImage) -> Self {
        Image {
            id: x.id.into(),
            url: x.url,
            size: x.size,
            created: x.created,
            owner_id: x.owner_id.into(),
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImageContext {
    Project,
    ThreadMessage,
    Unknown,
}

impl std::fmt::Display for ImageContext {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(fmt, "{}", self.as_str())
    }
}

impl ImageContext {
    pub fn as_str(&self) -> &'static str {
        match self {
            ImageContext::Project => "project",
            ImageContext::ThreadMessage => "thread_message",
            ImageContext::Unknown => "unknown",
        }
    }
}
