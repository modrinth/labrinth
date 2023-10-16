use chrono::{DateTime, Utc};
use itertools::Itertools;
use serde::{Deserialize, Serialize};

use crate::models::pats::Scopes;

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
    pub icon_url: Option<String>,
    pub max_scopes: Scopes,
    pub secret_hash: String,
    pub redirect_uris: Vec<OAuthRedirectUri>,
    pub created: DateTime<Utc>,
    pub created_by: UserId,
}

impl OAuthClient {
    pub async fn get<'a, E>(
        id: OAuthClientId,
        exec: E,
    ) -> Result<Option<OAuthClient>, DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        let value = sqlx::query!(
            "
            SELECT
                clients.id,
                clients.name,
                clients.icon_url,
                clients.max_scopes,
                clients.secret_hash,
                clients.created,
                clients.created_by,
                uris.uri_ids,
                uris.uri_vals
            FROM oauth_clients clients
            LEFT JOIN (
                SELECT client_id, array_agg(id) as uri_ids, array_agg(uri) as uri_vals
                FROM oauth_client_redirect_uris
                GROUP BY client_id
            ) uris ON clients.id = uris.client_id
            WHERE clients.id = $1
            ",
            id.0
        )
        .fetch_optional(exec)
        .await?;

        return Ok(value.map(|r| {
            let redirects =
                if let (Some(ids), Some(uris)) = (r.uri_ids.as_ref(), r.uri_vals.as_ref()) {
                    ids.iter()
                        .zip(uris.iter())
                        .map(|(id, uri)| OAuthRedirectUri {
                            id: OAuthRedirectUriId(*id),
                            client_id: OAuthClientId(r.id.clone()),
                            uri: uri.to_string(),
                        })
                        .collect()
                } else {
                    vec![]
                };

            OAuthClient {
                id: OAuthClientId(r.id),
                name: r.name,
                icon_url: r.icon_url,
                max_scopes: Scopes::from_bits(r.max_scopes as u64).unwrap_or(Scopes::NONE),
                secret_hash: r.secret_hash,
                redirect_uris: redirects,
                created: r.created,
                created_by: UserId(r.created_by),
            }
        }));
    }

    pub async fn insert(
        &self,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), DatabaseError> {
        sqlx::query!(
            "
            INSERT INTO oauth_clients (
                id, name, icon_url, max_scopes, secret_hash, created_by
            )
            VALUES (
                $1, $2, $3, $4, $5, $6
            )
            ",
            self.id.0,
            self.name,
            self.icon_url,
            self.max_scopes.bits() as i64,
            self.secret_hash,
            self.created_by.0
        )
        .execute(&mut *transaction)
        .await?;

        self.insert_redirect_uris(transaction).await?;

        Ok(())
    }

    async fn insert_redirect_uris(
        &self,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), DatabaseError> {
        let (ids, client_ids, uris): (Vec<_>, Vec<_>, Vec<_>) = self
            .redirect_uris
            .iter()
            .map(|r| (r.id.0, r.client_id.0, r.uri.clone()))
            .multiunzip();
        sqlx::query!(
            "
            INSERT INTO oauth_client_redirect_uris (id, client_id, uri)
            SELECT * FROM UNNEST($1::bigint[], $2::bigint[], $3::varchar[])
            ",
            &ids[..],
            &client_ids[..],
            &uris[..],
        )
        .execute(&mut *transaction)
        .await?;

        Ok(())
    }
}
