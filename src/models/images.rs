use super::{
    ids::{Base62Id, ProjectId, ThreadMessageId},
    pats::Scopes,
};
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

    pub context: ImageContext,
}

impl From<DBImage> for Image {
    fn from(x: DBImage) -> Self {
        Image {
            id: x.id.into(),
            url: x.url,
            size: x.size,
            created: x.created,
            owner_id: x.owner_id.into(),
            context: x.context,
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
pub enum ImageContext {
    Project {
        project_id: Option<ProjectId>,
    },
    ThreadMessage {
        thread_message_id: Option<ThreadMessageId>,
    },
    Unknown,
}

impl ImageContext {
    pub fn context_as_str(&self) -> &'static str {
        match self {
            ImageContext::Project { .. } => "project",
            ImageContext::ThreadMessage { .. } => "thread_message",
            ImageContext::Unknown => "unknown",
        }
    }

    pub fn inner_id(&self) -> Option<u64> {
        match self {
            ImageContext::Project { project_id } => project_id.map(|x| x.0),
            ImageContext::ThreadMessage { thread_message_id } => thread_message_id.map(|x| x.0),
            ImageContext::Unknown => None,
        }
    }
    pub fn relevant_scope(&self) -> Scopes {
        match self {
            ImageContext::Project { .. } => Scopes::PROJECT_WRITE,
            ImageContext::ThreadMessage { .. } => Scopes::THREAD_WRITE,
            ImageContext::Unknown => Scopes::NONE,
        }
    }
    pub fn from_str(context: &str, id: Option<u64>) -> Self {
        match context {
            "project" => ImageContext::Project {
                project_id: id.map(ProjectId),
            },
            "thread_message" => ImageContext::ThreadMessage {
                thread_message_id: id.map(ThreadMessageId),
            },
            _ => ImageContext::Unknown,
        }
    }
}
