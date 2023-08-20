use super::ids::*;
use crate::database::models;
use crate::database::models::DatabaseError;
use crate::models::ids::base62_impl::{parse_base62, to_base62};
use chrono::{DateTime, Utc};
use redis::cmd;
use serde::{Deserialize, Serialize};

const COLLECTIONS_NAMESPACE: &str = "collections";
const COLLECTIONS_SLUGS_NAMESPACE: &str = "collections_slugs";
const DEFAULT_EXPIRY: i64 = 1800; // 30 minutes

#[derive(Clone)]
pub struct CollectionBuilder {
    pub collection_id: CollectionId,
    pub team_id: TeamId,
    pub title: String,
    pub description: String,
    pub body: String,
    pub icon_url: Option<String>,
    pub color: Option<u32>,
    pub public: bool,
    pub slug: Option<String>,
    pub projects: Vec<ProjectId>,
}

impl CollectionBuilder {
    pub async fn insert(
        self,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<CollectionId, DatabaseError> {
        let collection_struct = Collection {
            id: self.collection_id,
            slug: self.slug,
            team_id: self.team_id,
            title: self.title,
            description: self.description,
            body: self.body,
            published: Utc::now(),
            updated: Utc::now(),
            icon_url: self.icon_url,
            color: self.color,
            public: self.public,
            projects: self.projects,
        };
        collection_struct.insert(&mut *transaction).await?;

        Ok(self.collection_id)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Collection {
    pub id: CollectionId,
    pub team_id: TeamId,
    pub title: String,
    pub description: String,
    pub body: String,
    pub published: DateTime<Utc>,
    pub updated: DateTime<Utc>,
    pub icon_url: Option<String>,
    pub color: Option<u32>,
    pub slug: Option<String>,
    pub public: bool,
    pub projects: Vec<ProjectId>,
}

impl Collection {
    pub async fn insert(
        &self,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), DatabaseError> {
        sqlx::query!(
            "
            INSERT INTO collections (
                id, team_id, title, description, body,
                published, icon_url, slug
            )
            VALUES (
                $1, $2, $3, $4, $5,
                $6, $7, LOWER($8)
            )
            ",
            self.id as CollectionId,
            self.team_id as TeamId,
            &self.title,
            &self.description,
            &self.body,
            self.published,
            self.icon_url.as_ref(),
            self.slug.as_ref(),
        )
        .execute(&mut *transaction)
        .await?;

        Ok(())
    }

    pub async fn remove(
        id: CollectionId,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        redis: &deadpool_redis::Pool,
    ) -> Result<Option<()>, DatabaseError> {
        let collection = Self::get_id(id, &mut *transaction, redis).await?;

        if let Some(collection) = collection {
            sqlx::query!(
                "
                DELETE FROM collections_mods
                WHERE collection_id = $1
                ",
                id as CollectionId,
            )
            .execute(&mut *transaction)
            .await?;

            sqlx::query!(
                "
                DELETE FROM collections
                WHERE id = $1
                ",
                id as CollectionId,
            )
            .execute(&mut *transaction)
            .await?;

            models::TeamMember::clear_cache(collection.team_id, redis).await?;
            models::Collection::clear_cache(collection.id, collection.slug, redis).await?;

            Ok(Some(()))
        } else {
            Ok(None)
        }
    }

    pub async fn get<'a, 'b, E>(
        string: &str,
        executor: E,
        redis: &deadpool_redis::Pool,
    ) -> Result<Option<Collection>, DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        Collection::get_many(&[string], executor, redis)
            .await
            .map(|x| x.into_iter().next())
    }

    pub async fn get_id<'a, 'b, E>(
        id: CollectionId,
        executor: E,
        redis: &deadpool_redis::Pool,
    ) -> Result<Option<Collection>, DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        Collection::get_many(
            &[crate::models::ids::CollectionId::from(id)],
            executor,
            redis,
        )
        .await
        .map(|x| x.into_iter().next())
    }

    pub async fn get_many<'a, E, T: ToString>(
        collection_strings: &[T],
        exec: E,
        redis: &deadpool_redis::Pool,
    ) -> Result<Vec<Collection>, DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        use futures::TryStreamExt;

        if collection_strings.is_empty() {
            return Ok(Vec::new());
        }

        let mut redis = redis.get().await?;

        let mut found_collections = Vec::new();
        let mut remaining_strings = collection_strings
            .iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>();

        let mut collection_ids = collection_strings
            .iter()
            .flat_map(|x| parse_base62(&x.to_string()).map(|x| x as i64))
            .collect::<Vec<_>>();

        collection_ids.append(
            &mut cmd("MGET")
                .arg(
                    collection_strings
                        .iter()
                        .map(|x| {
                            format!(
                                "{}:{}",
                                COLLECTIONS_SLUGS_NAMESPACE,
                                x.to_string().to_lowercase()
                            )
                        })
                        .collect::<Vec<_>>(),
                )
                .query_async::<_, Vec<Option<i64>>>(&mut redis)
                .await?
                .into_iter()
                .flatten()
                .collect(),
        );

        if !collection_ids.is_empty() {
            let collections = cmd("MGET")
                .arg(
                    collection_ids
                        .iter()
                        .map(|x| format!("{}:{}", COLLECTIONS_NAMESPACE, x))
                        .collect::<Vec<_>>(),
                )
                .query_async::<_, Vec<Option<String>>>(&mut redis)
                .await?;

            for collection in collections {
                if let Some(collection) =
                    collection.and_then(|x| serde_json::from_str::<Collection>(&x).ok())
                {
                    remaining_strings.retain(|x| {
                        &to_base62(collection.id.0 as u64) != x
                            && collection.slug.as_ref().map(|x| x.to_lowercase())
                                != Some(x.to_lowercase())
                    });
                    found_collections.push(collection);
                    continue;
                }
            }
        }

        if !remaining_strings.is_empty() {
            let collection_ids_parsed: Vec<i64> = remaining_strings
                .iter()
                .flat_map(|x| parse_base62(&x.to_string()).ok())
                .map(|x| x as i64)
                .collect();
            let db_collections: Vec<Collection> = sqlx::query!(
                "
                SELECT c.id id, c.title title, c.description description,
                c.icon_url icon_url, c.color color, c.body body, c.published published,
                c.updated updated, c.team_id team_id, c.slug slug, c.public public,
                ARRAY_AGG(DISTINCT m.id) filter (where m.id is not null) mods
                FROM collections c
                LEFT JOIN collections_mods cm ON cm.collection_id = c.id
                LEFT JOIN mods m ON m.id = cm.mod_id
                WHERE c.id = ANY($1) OR c.slug = ANY($2)
                GROUP BY c.id;
                ",
                &collection_ids_parsed,
                &remaining_strings
                    .into_iter()
                    .map(|x| x.to_string().to_lowercase())
                    .collect::<Vec<_>>(),
            )
            .fetch_many(exec)
            .try_filter_map(|e| async {
                Ok(e.right().map(|m| {
                    let id = m.id;

                    Collection {
                        id: CollectionId(id),
                        team_id: TeamId(m.team_id),
                        title: m.title.clone(),
                        description: m.description.clone(),
                        icon_url: m.icon_url.clone(),
                        color: m.color.map(|x| x as u32),
                        published: m.published,
                        updated: m.updated,
                        slug: m.slug.clone(),
                        body: m.body.clone(),
                        public: m.public,
                        projects: m
                            .mods
                            .unwrap_or_default()
                            .into_iter()
                            .map(ProjectId)
                            .collect(),
                    }
                }))
            })
            .try_collect::<Vec<Collection>>()
            .await?;

            for collection in db_collections {
                cmd("SET")
                    .arg(format!("{}:{}", COLLECTIONS_NAMESPACE, collection.id.0))
                    .arg(serde_json::to_string(&collection)?)
                    .arg("EX")
                    .arg(DEFAULT_EXPIRY)
                    .query_async::<_, ()>(&mut redis)
                    .await?;

                if let Some(slug) = &collection.slug {
                    cmd("SET")
                        .arg(format!(
                            "{}:{}",
                            COLLECTIONS_SLUGS_NAMESPACE,
                            slug.to_lowercase()
                        ))
                        .arg(collection.id.0)
                        .arg("EX")
                        .arg(DEFAULT_EXPIRY)
                        .query_async::<_, ()>(&mut redis)
                        .await?;
                }
                found_collections.push(collection);
            }
        }

        Ok(found_collections)
    }

    pub async fn clear_cache(
        id: CollectionId,
        slug: Option<String>,
        redis: &deadpool_redis::Pool,
    ) -> Result<(), DatabaseError> {
        let mut redis = redis.get().await?;
        let mut cmd = cmd("DEL");

        cmd.arg(format!("{}:{}", COLLECTIONS_NAMESPACE, id.0));
        if let Some(slug) = slug {
            cmd.arg(format!(
                "{}:{}",
                COLLECTIONS_SLUGS_NAMESPACE,
                slug.to_lowercase()
            ));
        }

        cmd.query_async::<_, ()>(&mut redis).await?;

        Ok(())
    }
}
