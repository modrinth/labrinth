use chrono::{DateTime, Utc};
use itertools::Itertools;
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

pub struct OAuthClientAuthorizationWithClientInfo {
    pub id: OAuthClientAuthorizationId,
    pub client_id: OAuthClientId,
    pub user_id: UserId,
    pub scopes: Scopes,
    pub created: DateTime<Utc>,
    pub client_name: String,
    pub client_icon_url: Option<String>,
    pub client_created_by: UserId,
}

impl OAuthClientAuthorization {
    pub async fn get(
        client_id: OAuthClientId,
        user_id: UserId,
        exec: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    ) -> Result<Option<OAuthClientAuthorization>, DatabaseError> {
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

        Ok(value.map(|r| OAuthClientAuthorization {
            id: OAuthClientAuthorizationId(r.id),
            client_id: OAuthClientId(r.client_id),
            user_id: UserId(r.user_id),
            scopes: Scopes::from_postgres(r.scopes),
            created: r.created,
        }))
    }

    pub async fn get_all_for_user(
        user_id: UserId,
        exec: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    ) -> Result<Vec<OAuthClientAuthorizationWithClientInfo>, DatabaseError> {
        let results = sqlx::query!(
            "
            SELECT 
                auths.id,
                auths.client_id,
                auths.user_id,
                auths.scopes,
                auths.created,
                clients.name as client_name,
                clients.icon_url as client_icon_url,
                clients.created_by as client_created_by
            FROM oauth_client_authorizations auths
            JOIN oauth_clients clients ON clients.id = auths.client_id
            WHERE user_id=$1
            ",
            user_id.0
        )
        .fetch_all(exec)
        .await?;

        Ok(results
            .into_iter()
            .map(|r| OAuthClientAuthorizationWithClientInfo {
                id: OAuthClientAuthorizationId(r.id),
                client_id: OAuthClientId(r.client_id),
                user_id: UserId(r.user_id),
                scopes: Scopes::from_postgres(r.scopes),
                created: r.created,
                client_name: r.client_name,
                client_icon_url: r.client_icon_url,
                client_created_by: UserId(r.client_created_by),
            })
            .collect_vec())
    }

    pub async fn upsert(
        id: OAuthClientAuthorizationId,
        client_id: OAuthClientId,
        user_id: UserId,
        scopes: Scopes,
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
            ON CONFLICT (id)
            DO UPDATE SET scopes = EXCLUDED.scopes
            ",
            id.0,
            client_id.0,
            user_id.0,
            scopes.bits() as i64,
        )
        .execute(&mut **transaction)
        .await?;

        Ok(())
    }

    pub async fn remove(
        client_id: OAuthClientId,
        user_id: UserId,
        exec: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    ) -> Result<(), DatabaseError> {
        sqlx::query!(
            "
            DELETE FROM oauth_client_authorizations
            WHERE client_id=$1 AND user_id=$2
            ",
            client_id.0,
            user_id.0
        )
        .execute(exec)
        .await?;

        Ok(())
    }
}
