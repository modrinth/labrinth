use super::ids::{Base62Id, ThreadMessageId};
use crate::database::models::image_item::Image as DBImage;
use crate::models::ids::{ProjectId, UserId};
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

    pub mod_id: Option<ProjectId>,
    pub thread_message_id: Option<ThreadMessageId>,
}

impl From<DBImage> for Image {
    fn from(x: DBImage) -> Self {
        Image {
            id: x.id.into(),
            url: x.url,
            size: x.size,
            created: x.created,
            owner_id: x.owner_id.into(),

            mod_id: x.mod_id.map(|x| x.into()),
            thread_message_id: x.thread_message_id.map(|x| x.into()),
        }
    }
}
