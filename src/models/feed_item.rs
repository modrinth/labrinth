use super::ids::Base62Id;
use super::ids::OrganizationId;
use super::users::UserId;
use crate::models::ids::ProjectId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::database::models::event_item as DBEvent;

#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Base62Id")]
#[serde(into = "Base62Id")]
pub struct FeedItemId(pub u64);

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CreatorId {
    User(UserId),
    Organization(OrganizationId),
}

#[derive(Serialize, Deserialize)]
pub struct FeedItem {
    pub id: FeedItemId,
    pub body: FeedItemBody,
    pub time: DateTime<Utc>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FeedItemBody {
    ProjectCreated {
        project_id: ProjectId,
        creator_id: CreatorId,
        project_title: String,
    },
}

impl From<crate::database::models::event_item::CreatorId> for CreatorId {
    fn from(value: crate::database::models::event_item::CreatorId) -> Self {
        match value {
            DBEvent::CreatorId::User(user_id) => CreatorId::User(user_id.into()),
            DBEvent::CreatorId::Organization(organization_id) => {
                CreatorId::Organization(organization_id.into())
            }
        }
    }
}
