use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{database::redis::RedisPool, models::pats::Scopes};

use super::{DatabaseError, OAuthClientAuthorizationId, OAuthClientId, UserId};

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct OAuthClientAuthorization {
    pub id: OAuthClientAuthorizationId,
    pub client_id: OAuthClientId,
    pub user_id: UserId,
    pub scopes: Scopes,
    pub created: DateTime<Utc>,
    // last_used?
}

impl OAuthClientAuthorization {
    pub async fn get<'a, E>(
        client_id: OAuthClientId,
        user_id: UserId,
        exec: E,
        redis: &RedisPool,
    ) -> Result<Option<OAuthClientAuthorization>, DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        return Ok(Some(OAuthClientAuthorization {
            id: OAuthClientAuthorizationId(1),
            client_id: OAuthClientId(1),
            user_id: UserId(1),
            scopes: Scopes::all(),
            created: Utc::now(),
        }));
    }
}
