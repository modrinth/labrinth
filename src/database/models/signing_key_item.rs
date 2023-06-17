use crate::util::keys::SigningKeyBody;

use super::{ids::*, DatabaseError};
use chrono::{DateTime, Utc};
use futures::stream::{StreamExt, TryStreamExt};

pub struct SigningKey {
    pub id: SigningKeyId,
    pub owner_id: UserId,
    pub body: SigningKeyBody,
    pub created: DateTime<Utc>,
}

impl SigningKey {
    pub async fn insert(
        &self,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), sqlx::error::Error> {
        sqlx::query!(
            "
            INSERT INTO signing_keys (
                id, owner_id, body_type, body
            )
            VALUES (
                $1, $2, $3, $4
            )
            ",
            self.id as SigningKeyId,
            self.owner_id as UserId,
            self.body.type_str() as &'static str,
            self.body.to_body() as String,
        )
        .execute(&mut *transaction)
        .await?;

        Ok(())
    }

    pub async fn get<'a, E>(
        id: SigningKeyId,
        exec: E,
    ) -> Result<Option<SigningKey>, DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres> + Copy,
    {
        Self::get_many(&[id], exec)
            .await
            .map(|x| x.into_iter().next())
    }

    pub async fn get_many<'a, E>(
        key_ids: &[SigningKeyId],
        exec: E,
    ) -> Result<Vec<SigningKey>, DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres> + Copy,
    {
        let key_ids_parsed: Vec<i64> = key_ids.iter().map(|x| x.0).collect();
        let keys = sqlx::query!(
            "
            SELECT k.id, k.owner_id, k.body_type, k.body, k.created
            FROM signing_keys k
            WHERE k.id = ANY($1)
            ORDER BY k.created DESC
            ",
            &key_ids_parsed
        )
        .fetch_many(exec)
        .map(|x| x.map_err(DatabaseError::from))
        .try_filter_map(|e| async {
            Ok(e.right()
                .map(|x| {
                    Ok::<SigningKey, DatabaseError>(SigningKey {
                        id: SigningKeyId(x.id),
                        owner_id: UserId(x.owner_id),
                        body: SigningKeyBody::parse(&x.body_type, &x.body)?,
                        created: x.created,
                    })
                })
                .transpose()?)
        })
        .try_collect::<Vec<SigningKey>>()
        .await?;

        Ok(keys)
    }

    pub async fn get_many_user<'a, E>(
        user_id: UserId,
        exec: E,
    ) -> Result<Vec<SigningKey>, DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres> + Copy,
    {
        sqlx::query!(
            "
            SELECT k.id, k.owner_id, k.body_type, k.body, k.created
            FROM signing_keys k
            WHERE k.owner_id = $1
            ",
            user_id as UserId
        )
        .fetch_many(exec)
        .map(|x| x.map_err(DatabaseError::from))
        .try_filter_map(|e| async {
            Ok(e.right()
                .map(|x| {
                    Ok::<SigningKey, DatabaseError>(SigningKey {
                        id: SigningKeyId(x.id),
                        owner_id: UserId(x.owner_id),
                        body: SigningKeyBody::parse(&x.body_type, &x.body)?,
                        created: x.created,
                    })
                })
                .transpose()?)
        })
        .try_collect::<Vec<SigningKey>>()
        .await
    }

    pub async fn remove_full<'a, E>(
        id: SigningKeyId,
        exec: E,
    ) -> Result<Option<()>, sqlx::Error>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres> + Copy,
    {
        let result = sqlx::query!(
            "
            SELECT EXISTS(SELECT 1 FROM signing_keys WHERE id = $1)
            ",
            id as SigningKeyId
        )
        .fetch_one(exec)
        .await?;

        if !result.exists.unwrap_or(false) {
            return Ok(None);
        }

        sqlx::query!(
            "
            DELETE FROM signing_keys WHERE id = $1
            ",
            id as SigningKeyId,
        )
        .execute(exec)
        .await?;

        Ok(Some(()))
    }
}
