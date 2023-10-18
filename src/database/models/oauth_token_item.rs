use super::{DatabaseError, OAuthAccessTokenId, OAuthClientAuthorizationId, OAuthClientId, UserId};
use crate::models::pats::Scopes;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct OAuthAccessToken {
    pub id: OAuthAccessTokenId,
    pub authorization_id: OAuthClientAuthorizationId,
    pub token_hash: String,
    pub scopes: Scopes,
    pub created: DateTime<Utc>,
    pub expires: DateTime<Utc>,
    pub last_used: Option<DateTime<Utc>>,

    // Stored separately inside oauth_client_authorizations table
    pub client_id: OAuthClientId,
    pub user_id: UserId,
}

impl OAuthAccessToken {
    pub async fn get(
        token_hash: String,
        exec: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    ) -> Result<Option<OAuthAccessToken>, DatabaseError> {
        let value = sqlx::query!(
            "
            SELECT
                tokens.id,
                tokens.authorization_id,
                tokens.token_hash,
                tokens.scopes,
                tokens.created,
                tokens.expires,
                tokens.last_used,
                auths.client_id,
                auths.user_id
            FROM oauth_access_tokens tokens
            JOIN oauth_client_authorizations auths
            ON tokens.authorization_id = auths.id
            WHERE tokens.token_hash = $1
            ",
            token_hash
        )
        .fetch_optional(exec)
        .await?;

        return Ok(value.map(|r| OAuthAccessToken {
            id: OAuthAccessTokenId(r.id),
            authorization_id: OAuthClientAuthorizationId(r.authorization_id),
            token_hash: r.token_hash,
            scopes: Scopes::from_postgres(r.scopes),
            created: r.created,
            expires: r.expires,
            last_used: r.last_used,
            client_id: OAuthClientId(r.client_id),
            user_id: UserId(r.user_id),
        }));
    }

    pub async fn insert(
        &self,
        exec: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    ) -> Result<(), DatabaseError> {
        sqlx::query!(
            "
            INSERT INTO oauth_access_tokens (
                id, authorization_id, token_hash, scopes, expires, last_used
            )
            Values (
                $1, $2, $3, $4, $5, $6
            )
            ",
            self.id.0,
            self.authorization_id.0,
            self.token_hash,
            self.scopes.to_postgres(),
            self.expires,
            Option::<DateTime<Utc>>::None
        )
        .execute(exec)
        .await?;

        Ok(())
    }
}
