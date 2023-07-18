use super::ids::*;
use crate::database::models::DatabaseError;
use crate::models::ids::base62_impl::{parse_base62, to_base62};
use crate::models::pats::Scopes;
use chrono::{DateTime, Utc};
use redis::cmd;
use serde::{Deserialize, Serialize};

const PATS_NAMESPACE: &str = "pats";
const PATS_TOKENS_NAMESPACE: &str = "pats_tokens";
const PATS_USERS_NAMESPACE: &str = "pats_users";
const DEFAULT_EXPIRY: i64 = 1800; // 30 minutes

#[derive(Deserialize, Serialize)]
pub struct PersonalAccessToken {
    pub id: PatId,
    pub name: String,
    pub access_token: String,
    pub scopes: Scopes,
    pub user_id: UserId,
    pub created: DateTime<Utc>,
    pub expires: DateTime<Utc>,
    pub last_used: Option<DateTime<Utc>>,
}

impl PersonalAccessToken {
    pub async fn insert(
        &self,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), DatabaseError> {
        sqlx::query!(
            "
            INSERT INTO pats (
                id, name, access_token, scopes, user_id,
                expires
            )
            VALUES (
                $1, $2, $3, $4, $5,
                $6
            )
            ",
            self.id as PatId,
            self.name,
            self.access_token,
            self.scopes.bits() as i64,
            self.user_id as UserId,
            self.expires
        )
        .execute(&mut *transaction)
        .await?;

        Ok(())
    }

    pub async fn get<'a, E, T: ToString>(
        id: T,
        exec: E,
        redis: &deadpool_redis::Pool,
    ) -> Result<Option<PersonalAccessToken>, DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        Self::get_many(&[id], exec, redis)
            .await
            .map(|x| x.into_iter().next())
    }

    pub async fn get_many_ids<'a, E>(
        pat_ids: &[PatId],
        exec: E,
        redis: &deadpool_redis::Pool,
    ) -> Result<Vec<PersonalAccessToken>, DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        let ids = pat_ids
            .iter()
            .map(|x| crate::models::ids::PatId::from(*x))
            .collect::<Vec<_>>();
        PersonalAccessToken::get_many(&ids, exec, redis).await
    }

    pub async fn get_many<'a, E, T: ToString>(
        pat_strings: &[T],
        exec: E,
        redis: &deadpool_redis::Pool,
    ) -> Result<Vec<PersonalAccessToken>, DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        use futures::TryStreamExt;

        if pat_strings.is_empty() {
            return Ok(Vec::new());
        }

        let mut redis = redis.get().await?;

        let mut found_pats = Vec::new();
        let mut remaining_strings = pat_strings
            .iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>();

        let mut pat_ids = pat_strings
            .iter()
            .flat_map(|x| parse_base62(&x.to_string()).map(|x| x as i64))
            .collect::<Vec<_>>();

        pat_ids.append(
            &mut cmd("MGET")
                .arg(
                    pat_strings
                        .iter()
                        .map(|x| format!("{}:{}", PATS_TOKENS_NAMESPACE, x.to_string()))
                        .collect::<Vec<_>>(),
                )
                .query_async::<_, Vec<Option<i64>>>(&mut redis)
                .await?
                .into_iter()
                .flatten()
                .collect(),
        );

        if !pat_ids.is_empty() {
            let pats = cmd("MGET")
                .arg(
                    pat_ids
                        .iter()
                        .map(|x| format!("{}:{}", PATS_NAMESPACE, x))
                        .collect::<Vec<_>>(),
                )
                .query_async::<_, Vec<Option<String>>>(&mut redis)
                .await?;

            for pat in pats {
                if let Some(pat) =
                    pat.and_then(|x| serde_json::from_str::<PersonalAccessToken>(&x).ok())
                {
                    remaining_strings
                        .retain(|x| &to_base62(pat.id.0 as u64) != x && &pat.access_token != x);
                    found_pats.push(pat);
                    continue;
                }
            }
        }

        if !remaining_strings.is_empty() {
            let pat_ids_parsed: Vec<i64> = remaining_strings
                .iter()
                .flat_map(|x| parse_base62(&x.to_string()).ok())
                .map(|x| x as i64)
                .collect();
            let db_pats: Vec<PersonalAccessToken> = sqlx::query!(
                "
                SELECT id, name, access_token, scopes, user_id, created, expires, last_used
                FROM pats
                WHERE id = ANY($1) OR access_token = ANY($2)
                ORDER BY created DESC
                ",
                &pat_ids_parsed,
                &remaining_strings
                    .into_iter()
                    .map(|x| x.to_string())
                    .collect::<Vec<_>>(),
            )
            .fetch_many(exec)
            .try_filter_map(|e| async {
                Ok(e.right().map(|x| PersonalAccessToken {
                    id: PatId(x.id),
                    name: x.name,
                    access_token: x.access_token,
                    scopes: Scopes::from_bits(x.scopes as u64).unwrap_or(Scopes::NONE),
                    user_id: UserId(x.user_id),
                    created: x.created,
                    expires: x.expires,
                    last_used: x.last_used,
                }))
            })
            .try_collect::<Vec<PersonalAccessToken>>()
            .await?;

            for pat in db_pats {
                cmd("SET")
                    .arg(format!("{}:{}", PATS_NAMESPACE, pat.id.0))
                    .arg(serde_json::to_string(&pat)?)
                    .arg("EX")
                    .arg(DEFAULT_EXPIRY)
                    .query_async::<_, ()>(&mut redis)
                    .await?;

                cmd("SET")
                    .arg(format!("{}:{}", PATS_TOKENS_NAMESPACE, pat.access_token))
                    .arg(pat.id.0)
                    .arg("EX")
                    .arg(DEFAULT_EXPIRY)
                    .query_async::<_, ()>(&mut redis)
                    .await?;
                found_pats.push(pat);
            }
        }

        Ok(found_pats)
    }

    pub async fn get_user_pats<'a, E>(
        user_id: UserId,
        exec: E,
        redis: &deadpool_redis::Pool,
    ) -> Result<Vec<PatId>, DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        let mut redis = redis.get().await?;
        let res = cmd("GET")
            .arg(format!("{}:{}", PATS_USERS_NAMESPACE, user_id.0))
            .query_async::<_, Option<String>>(&mut redis)
            .await?
            .and_then(|x| serde_json::from_str::<Vec<i64>>(&x).ok());

        if let Some(res) = res {
            return Ok(res.into_iter().map(PatId).collect());
        }

        use futures::TryStreamExt;
        let db_pats: Vec<PatId> = sqlx::query!(
            "
                SELECT id
                FROM pats
                WHERE user_id = $1
                ORDER BY created DESC
                ",
            user_id.0,
        )
        .fetch_many(exec)
        .try_filter_map(|e| async { Ok(e.right().map(|x| PatId(x.id))) })
        .try_collect::<Vec<PatId>>()
        .await?;

        cmd("SET")
            .arg(format!("{}:{}", PATS_USERS_NAMESPACE, user_id.0))
            .arg(serde_json::to_string(&db_pats)?)
            .arg("EX")
            .arg(DEFAULT_EXPIRY)
            .query_async::<_, ()>(&mut redis)
            .await?;

        Ok(db_pats)
    }

    pub async fn clear_cache(
        clear_pats: Vec<(Option<PatId>, Option<String>, Option<UserId>)>,
        redis: &deadpool_redis::Pool,
    ) -> Result<(), DatabaseError> {
        if clear_pats.is_empty() {
            return Ok(());
        }

        let mut redis = redis.get().await?;
        let mut cmd = cmd("DEL");

        for (id, token, user_id) in clear_pats {
            if let Some(id) = id {
                cmd.arg(format!("{}:{}", PATS_NAMESPACE, id.0));
            }
            if let Some(token) = token {
                cmd.arg(format!("{}:{}", PATS_TOKENS_NAMESPACE, token));
            }
            if let Some(user_id) = user_id {
                cmd.arg(format!("{}:{}", PATS_USERS_NAMESPACE, user_id.0));
            }
        }

        cmd.query_async::<_, ()>(&mut redis).await?;

        Ok(())
    }

    pub async fn remove(
        id: PatId,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<Option<()>, sqlx::error::Error> {
        sqlx::query!(
            "
            DELETE FROM pats WHERE id = $1
            ",
            id as PatId,
        )
        .execute(&mut *transaction)
        .await?;

        Ok(Some(()))
    }
}
