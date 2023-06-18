use super::ids::Base62Id;
use crate::models::ids::UserId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Base62Id")]
#[serde(into = "Base62Id")]
pub struct SigningKeyId(pub u64);

#[derive(Serialize, Deserialize)]
pub struct SigningKey {
    pub id: SigningKeyId,
    pub owner: UserId,
    #[serde(rename = "type")]
    pub key_type: String,
    pub body: String,
    pub created: DateTime<Utc>,
}

use crate::database::models::signing_key_item::SigningKey as DBSigningKey;

impl From<DBSigningKey> for SigningKey {
    fn from(value: DBSigningKey) -> Self {
        SigningKey {
            id: value.id.into(),
            owner: value.owner_id.into(),
            key_type: value.body.type_str().to_string(),
            body: value.body.to_body(),
            created: value.created,
        }
    }
}
