use super::ids::*;
use crate::database::models::DatabaseError;
use crate::models::images::ImageContext;
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
    pub context: ImageContext, // uses model Ids, not database Ids
}

impl Image {
    pub async fn insert(
        &self,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), DatabaseError> {
        sqlx::query!(
            "
            INSERT INTO uploaded_images (
                id, url, size, created, owner_id, context, context_id
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
            self.context.context_as_str(),
            self.context.inner_id().map(|x| x as i64),
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
        context: ImageContext,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<Vec<Image>, sqlx::Error> {
        use futures::stream::TryStreamExt;

        sqlx::query!(
            "
            SELECT i.id, i.url, i.size, i.created, i.owner_id, i.context, i.context_id
            FROM uploaded_images i
            WHERE i.context = $1
            AND i.context_id = $2
            GROUP BY i.id
            ",
            context.context_as_str(),
            context.inner_id().map(|x| x as i64)
        )
        .fetch_many(transaction)
        .try_filter_map(|e| async {
            Ok(e.right().map(|row| {
                let id = ImageId(row.id);

                Image {
                    id,
                    url: row.url,
                    size: row.size as u64,
                    created: row.created,
                    owner_id: UserId(row.owner_id),
                    context: ImageContext::from_str(&row.context, row.context_id.map(|x| x as u64)),
                }
            }))
        })
        .try_collect::<Vec<Image>>()
        .await
    }

    pub async fn get<'a, 'b, E>(
        id: ImageId,
        executor: E,
        redis: &deadpool_redis::Pool,
    ) -> Result<Option<Image>, DatabaseError>
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
    ) -> Result<Vec<Image>, DatabaseError>
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
                if let Some(image) = image.and_then(|x| serde_json::from_str::<Image>(&x).ok()) {
                    remaining_ids.retain(|x| image.id.0 != x.0);
                    found_images.push(image);
                    continue;
                }
            }
        }

        if !remaining_ids.is_empty() {
            let db_images: Vec<Image> = sqlx::query!(
                "
                SELECT i.id, i.url, i.size, i.created, i.owner_id, i.context, i.context_id
                FROM uploaded_images i
                WHERE i.id = ANY($1)
                GROUP BY i.id;
                ",
                &remaining_ids.iter().map(|x| x.0).collect::<Vec<_>>(),
            )
            .fetch_many(exec)
            .try_filter_map(|e| async {
                Ok(e.right().map(|i| {
                    let id = i.id;

                    Image {
                        id: ImageId(id),
                        url: i.url,
                        size: i.size as u64,
                        created: i.created,
                        owner_id: UserId(i.owner_id),
                        context: ImageContext::from_str(&i.context, i.context_id.map(|x| x as u64)),
                    }
                }))
            })
            .try_collect::<Vec<Image>>()
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
