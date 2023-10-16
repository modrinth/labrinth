use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::models::pats::Scopes;

use super::{DatabaseError, OAuthClientAuthorizationId, OAuthClientId, UserId};

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct OAuthClientAuthorization {
    pub id: OAuthClientAuthorizationId,
    pub client_id: OAuthClientId,
    pub user_id: UserId,
    pub scopes: Scopes,
    pub created: DateTime<Utc>,
}

impl OAuthClientAuthorization {
    pub async fn get<'a, E>(
        client_id: OAuthClientId,
        user_id: UserId,
        exec: E,
    ) -> Result<Option<OAuthClientAuthorization>, DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        let value = sqlx::query!(
            "
            SELECT id, client_id, user_id, scopes, created
            FROM oauth_client_authorizations
            WHERE client_id=$1 AND user_id=$2
            ",
            client_id.0,
            user_id.0,
        )
        .fetch_optional(exec)
        .await?;

        return Ok(value.map(|r| OAuthClientAuthorization {
            id: OAuthClientAuthorizationId(r.id),
            client_id: OAuthClientId(r.client_id),
            user_id: UserId(r.user_id),
            scopes: Scopes::from_bits(r.scopes as u64).unwrap_or(Scopes::NONE),
            created: r.created,
        }));
    }

    pub async fn insert(
        &self,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), DatabaseError> {
        sqlx::query!(
            "
            INSERT INTO oauth_client_authorizations (
                id, client_id, user_id, scopes
            )
            VALUES (
                $1, $2, $3, $4
            )
            ",
            self.id.0,
            self.client_id.0,
            self.user_id.0,
            self.scopes.bits() as i64,
        )
        .execute(&mut *transaction)
        .await?;

        Ok(())
    }
}
