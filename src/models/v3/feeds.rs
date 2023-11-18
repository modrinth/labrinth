use super::ids::Base62Id;
use super::ids::OrganizationId;
use super::users::UserId;
use crate::models::ids::{ProjectId, VersionId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::database::models::event_item as DBEvent;

#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(from = "Base62Id")]
#[serde(into = "Base62Id")]
pub struct FeedItemId(pub u64);

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CreatorId {
    User { id: UserId },
    Organization { id: OrganizationId },
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FeedItem {
    pub id: FeedItemId,
    pub body: FeedItemBody,
    pub time: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FeedItemBody {
    ProjectPublished {
        project_id: ProjectId,
        creator_id: CreatorId,
        project_title: String,
    },
    VersionCreated {
        project_id: ProjectId,
        version_id: VersionId,
        creator_id: CreatorId,
        project_title: String,
    },
}

impl From<crate::database::models::event_item::CreatorId> for CreatorId {
    fn from(value: crate::database::models::event_item::CreatorId) -> Self {
        match value {
            DBEvent::CreatorId::User(user_id) => CreatorId::User { id: user_id.into() },
            DBEvent::CreatorId::Organization(organization_id) => CreatorId::Organization {
                id: organization_id.into(),
            },
        }
    }
}
