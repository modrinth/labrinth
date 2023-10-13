use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{database::redis::RedisPool, models::pats::Scopes};

use super::{DatabaseError, OAuthClientId, OAuthRedirectUriId, UserId};

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct OAuthRedirectUri {
    pub id: OAuthRedirectUriId,
    pub client_id: OAuthClientId,
    pub uri: String,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct OAuthClient {
    pub id: OAuthClientId,
    pub name: String,
    pub icon: Option<String>,
    pub max_scopes: Scopes,
    pub secret: String,
    pub redirect_uris: Vec<OAuthRedirectUri>,
    pub created: DateTime<Utc>,
    pub created_by: UserId,
}

impl OAuthClient {
    pub async fn get<'a, E, T: ToString>(
        id: T,
        exec: E,
        redis: &RedisPool,
    ) -> Result<Option<OAuthClient>, DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        return Ok(Some(OAuthClient {
            id: OAuthClientId(1),
            name: "Test Client".to_string(),
            icon: None,
            max_scopes: Scopes::all(),
            redirect_uris: vec![],
            secret: "hashed secret".to_string(),
            created: Utc::now(),
            created_by: UserId(1),
        }));
    }
}
