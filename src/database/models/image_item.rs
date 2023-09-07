use super::ids::*;
use crate::database::models::DatabaseError;
use chrono::{DateTime, Utc};
use redis::cmd;
use serde::{Deserialize, Serialize};

const IMAGES_NAMESPACE: &str = "images";
const DEFAULT_EXPIRY: i64 = 1800; // 30 minutes

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Image {
    pub id: ImageId,
    pub url: String,
    pub size: u64,
    pub created: DateTime<Utc>,
    pub owner_id: UserId,

    // context it is associated with
    pub context_type_id: ImageContextTypeId,
    pub context_id: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QueryImage {
    pub id: ImageId,
    pub url: String,
    pub size: u64,
    pub created: DateTime<Utc>,
    pub owner_id: UserId,

    // context it is associated with
    pub context_type_name: String,
    pub context_id: Option<i64>,
}

impl Image {
    pub async fn insert(
        &self,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), DatabaseError> {
        sqlx::query!(
            "
            INSERT INTO uploaded_images (
                id, url, size, created, owner_id, context_type, context_id
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, $7
            );
            ",
            self.id as ImageId,
            self.url,
            self.size as i64,
            self.created,
            self.owner_id as UserId,
            self.context_type_id.0,
            self.context_id.map(|x| x as i64),
        )
        .execute(&mut *transaction)
        .await?;

        Ok(())
    }

    pub async fn remove(
        id: ImageId,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        redis: &deadpool_redis::Pool,
    ) -> Result<Option<()>, DatabaseError> {
        let image = Self::get(id, &mut *transaction, redis).await?;

        if let Some(image) = image {
            sqlx::query!(
                "
                DELETE FROM uploaded_images
                WHERE id = $1
                ",
                id as ImageId,
            )
            .execute(&mut *transaction)
            .await?;

            Image::clear_cache(image.id, redis).await?;

            Ok(Some(()))
        } else {
            Ok(None)
        }
    }

    pub async fn get_many_contexted(
        context_type: ImageContextTypeId,
        context_id: i64,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<Vec<QueryImage>, sqlx::Error> {
        use futures::stream::TryStreamExt;

        sqlx::query!(
            "
            SELECT i.id, i.url, i.size, i.created, i.owner_id, t.name, i.context_id
            FROM uploaded_images i
            LEFT JOIN uploaded_images_context t ON t.id = i.context_type
            WHERE i.context_type = $1
            AND i.context_id = $2
            GROUP BY i.id, t.id
            ",
            context_type.0,
            context_id,
        )
        .fetch_many(transaction)
        .try_filter_map(|e| async {
            Ok(e.right().map(|row| {
                let id = ImageId(row.id);

                QueryImage {
                    id,
                    url: row.url,
                    size: row.size as u64,
                    created: row.created,
                    owner_id: UserId(row.owner_id),
                    context_type_name: row.name,
                    context_id: row.context_id,
                }
            }))
        })
        .try_collect::<Vec<QueryImage>>()
        .await
    }

    pub async fn get<'a, 'b, E>(
        id: ImageId,
        executor: E,
        redis: &deadpool_redis::Pool,
    ) -> Result<Option<QueryImage>, DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        Image::get_many(&[id], executor, redis)
            .await
            .map(|x| x.into_iter().next())
    }

    pub async fn get_many<'a, E>(
        image_ids: &[ImageId],
        exec: E,
        redis: &deadpool_redis::Pool,
    ) -> Result<Vec<QueryImage>, DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        use futures::TryStreamExt;

        if image_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut redis = redis.get().await?;

        let mut found_images = Vec::new();
        let mut remaining_ids = image_ids.to_vec();

        let image_ids = image_ids.iter().map(|x| x.0).collect::<Vec<_>>();

        if !image_ids.is_empty() {
            let images = cmd("MGET")
                .arg(
                    image_ids
                        .iter()
                        .map(|x| format!("{}:{}", IMAGES_NAMESPACE, x))
                        .collect::<Vec<_>>(),
                )
                .query_async::<_, Vec<Option<String>>>(&mut redis)
                .await?;

            for image in images {
                if let Some(image) = image.and_then(|x| serde_json::from_str::<QueryImage>(&x).ok()) {
                    remaining_ids.retain(|x| image.id.0 != x.0);
                    found_images.push(image);
                    continue;
                }
            }
        }

        if !remaining_ids.is_empty() {
            let db_images: Vec<QueryImage> = sqlx::query!(
                "
                SELECT i.id, i.url, i.size, i.created, i.owner_id, t.name, i.context_id
                FROM uploaded_images i
                LEFT JOIN uploaded_images_context t ON t.id = i.context_type
                WHERE i.id = ANY($1)
                GROUP BY i.id, t.id;
                ",
                &remaining_ids.iter().map(|x| x.0).collect::<Vec<_>>(),
            )
            .fetch_many(exec)
            .try_filter_map(|e| async {
                Ok(e.right().map(|i| {
                    let id = i.id;

                    QueryImage {
                        id: ImageId(id),
                        url: i.url,
                        size: i.size as u64,
                        created: i.created,
                        owner_id: UserId(i.owner_id),
                        context_type_name: i.name,
                        context_id: i.context_id,
                    }
                }))
            })
            .try_collect::<Vec<QueryImage>>()
            .await?;

            for image in db_images {
                cmd("SET")
                    .arg(format!("{}:{}", IMAGES_NAMESPACE, image.id.0))
                    .arg(serde_json::to_string(&image)?)
                    .arg("EX")
                    .arg(DEFAULT_EXPIRY)
                    .query_async::<_, ()>(&mut redis)
                    .await?;

                found_images.push(image);
            }
        }

        Ok(found_images)
    }

    pub async fn clear_cache(
        id: ImageId,
        redis: &deadpool_redis::Pool,
    ) -> Result<(), DatabaseError> {
        let mut redis = redis.get().await?;
        let mut cmd = cmd("DEL");

        cmd.arg(format!("{}:{}", IMAGES_NAMESPACE, id.0));
        cmd.query_async::<_, ()>(&mut redis).await?;

        Ok(())
    }
}
