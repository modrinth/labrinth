use std::path::PathBuf;

use super::ids::*;
use crate::database::models::DatabaseError;
use crate::database::redis::RedisPool;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use futures::TryStreamExt;
use serde::{Deserialize, Serialize};

// Hash and install path
type Override = (String, PathBuf);

pub const CLIENT_PROFILES_NAMESPACE: &str = "client_profiles";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClientProfile {
    pub id: ClientProfileId,
    pub name: String,
    pub owner_id: UserId,
    pub icon_url: Option<String>,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,

    pub game_id: GameId,
    pub game_name: String,
    pub metadata: ClientProfileMetadata,

    pub users: Vec<UserId>,

    // These represent the same loader
    pub loader_id: LoaderId,
    pub loader: String,

    pub versions: Vec<VersionId>,
    pub overrides: Vec<Override>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QueryClientProfile {
    pub inner: ClientProfile,
    pub links: Vec<ClientProfileLink>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ClientProfileMetadata {
    Minecraft {
        loader_version: String,
        game_version_id: LoaderFieldEnumValueId,
        // TODO: Currently, we store the game_version directly. If client profiles use more than just Minecraft,
        // this should change to use a variant of dynamic loader field system that versions use, and fields like
        // this would be loaded dynamically from the loader_field_enum_values table.
        game_version: String,
    },
    Unknown,
}

impl ClientProfile {
    pub async fn insert(
        &self,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), DatabaseError> {
        let metadata = serde_json::to_value(&self.metadata).map_err(|e| {
            DatabaseError::SchemaError(format!("Could not serialize metadata: {}", e))
        })?;

        sqlx::query!(
            "
            INSERT INTO shared_profiles (
                id, name, owner_id, icon_url, created, updated,
                loader_id, game_id, metadata
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, 
                $7, $8, $9
            )
            ",
            self.id as ClientProfileId,
            self.name,
            self.owner_id as UserId,
            self.icon_url,
            self.created,
            self.updated,
            self.loader_id as LoaderId,
            self.game_id.0,
            metadata
        )
        .execute(&mut **transaction)
        .await?;

        // Insert users
        for user_id in &self.users {
            sqlx::query!(
                "
                INSERT INTO shared_profiles_users (
                    shared_profile_id, user_id
                )
                VALUES (
                    $1, $2
                )
                ",
                self.id as ClientProfileId,
                user_id.0,
            )
            .execute(&mut **transaction)
            .await?;
        }

        // Insert versions
        for version_id in &self.versions {
            sqlx::query!(
                "
                INSERT INTO shared_profiles_mods (
                    shared_profile_id, version_id
                )
                VALUES (
                    $1, $2
                )
                ",
                self.id as ClientProfileId,
                version_id.0,
            )
            .execute(&mut **transaction)
            .await?;
        }

        Ok(())
    }

    // Returns the hashes of the files that were deleted, so they can be deleted from the file host
    pub async fn remove(
        id: ClientProfileId,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        redis: &RedisPool,
    ) -> Result<Vec<String>, DatabaseError> {
        // Delete shared_profiles_links
        sqlx::query!(
            "
            DELETE FROM shared_profiles_links
            WHERE shared_profile_id = $1
            ",
            id as ClientProfileId,
        )
        .execute(&mut **transaction)
        .await?;

        // Delete shared_profiles_users
        sqlx::query!(
            "
            DELETE FROM shared_profiles_users
            WHERE shared_profile_id = $1
            ",
            id as ClientProfileId,
        )
        .execute(&mut **transaction)
        .await?;

        // Deletes attached files- we return the hashes so we can delete them from the file host if needed
        let deleted_hashes = sqlx::query!(
            "
            DELETE FROM shared_profiles_mods
            WHERE shared_profile_id = $1
            RETURNING file_hash
            ",
            id as ClientProfileId,
        )
        .fetch_all(&mut **transaction)
        .await?
        .into_iter()
        .filter_map(|x| x.file_hash)
        .collect::<Vec<_>>();

        sqlx::query!(
            "
            DELETE FROM shared_profiles_links
            WHERE shared_profile_id = $1
            ",
            id as ClientProfileId,
        )
        .execute(&mut **transaction)
        .await?;

        sqlx::query!(
            "
            DELETE FROM shared_profiles
            WHERE id = $1
            ",
            id as ClientProfileId,
        )
        .execute(&mut **transaction)
        .await?;

        ClientProfile::clear_cache(id, redis).await?;

        Ok(deleted_hashes)
    }

    pub async fn get<'a, 'b, E>(
        id: ClientProfileId,
        executor: E,
        redis: &RedisPool,
    ) -> Result<Option<QueryClientProfile>, DatabaseError>
    where
        E: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        Self::get_many(&[id], executor, redis)
            .await
            .map(|x| x.into_iter().next())
    }

    pub async fn get_ids_for_user<'a, E>(
        user_id: UserId,
        exec: E,
    ) -> Result<Vec<ClientProfileId>, DatabaseError>
    where
        E: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut exec = exec.acquire().await?;
        let db_profiles: Vec<ClientProfileId> = sqlx::query!(
            "
            SELECT sp.id
            FROM shared_profiles sp                
            LEFT JOIN shared_profiles_users spu ON spu.shared_profile_id = sp.id
            WHERE spu.user_id = $1
            ",
            user_id.0
        )
        .fetch_many(&mut *exec)
        .try_filter_map(|e| async { Ok(e.right().map(|m| ClientProfileId(m.id))) })
        .try_collect::<Vec<ClientProfileId>>()
        .await?;
        Ok(db_profiles)
    }

    pub async fn get_many<'a, E>(
        ids: &[ClientProfileId],
        exec: E,
        redis: &RedisPool,
    ) -> Result<Vec<QueryClientProfile>, DatabaseError>
    where
        E: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut redis = redis.connect().await?;
        let mut exec = exec.acquire().await?;

        let mut found_profiles = Vec::new();
        let mut remaining_ids: Vec<ClientProfileId> = ids.to_vec();

        if !ids.is_empty() {
            let profiles = redis
                .multi_get::<String>(CLIENT_PROFILES_NAMESPACE, ids.iter().map(|x| x.0))
                .await?;
            for profile in profiles {
                if let Some(profile) =
                    profile.and_then(|x| serde_json::from_str::<QueryClientProfile>(&x).ok())
                {
                    remaining_ids.retain(|x| profile.inner.id != *x);
                    found_profiles.push(profile);
                    continue;
                }
            }
        }

        if !remaining_ids.is_empty() {
            type AttachedProjectsMap = (
                DashMap<ClientProfileId, Vec<VersionId>>,
                DashMap<ClientProfileId, Vec<Override>>,
            );
            let shared_profiles_mods: AttachedProjectsMap = sqlx::query!(
                "
                SELECT shared_profile_id, version_id, file_hash, install_path
                FROM shared_profiles_mods spm
                WHERE spm.shared_profile_id = ANY($1)
                ",
                &remaining_ids.iter().map(|x| x.0).collect::<Vec<i64>>()
            )
            .fetch(&mut *exec)
            .try_fold(
                (DashMap::new(), DashMap::new()),
                |(acc_versions, acc_overrides): AttachedProjectsMap, m| {
                    let version_id = m.version_id.map(VersionId);
                    let file_hash = m.file_hash;
                    let install_path = m.install_path;
                    if let Some(version_id) = version_id {
                        acc_versions
                            .entry(ClientProfileId(m.shared_profile_id))
                            .or_default()
                            .push(version_id);
                    }

                    if let (Some(install_path), Some(file_hash)) = (install_path, file_hash) {
                        acc_overrides
                            .entry(ClientProfileId(m.shared_profile_id))
                            .or_default()
                            .push((file_hash, PathBuf::from(install_path)));
                    }

                    async move { Ok((acc_versions, acc_overrides)) }
                },
            )
            .await?;

            let shared_profiles_links: DashMap<ClientProfileId, Vec<ClientProfileLink>> =
                sqlx::query!(
                    "
                    SELECT id, shared_profile_id, created, expires
                    FROM shared_profiles_links spl
                    WHERE spl.shared_profile_id = ANY($1)
                    ",
                    &remaining_ids.iter().map(|x| x.0).collect::<Vec<i64>>()
                )
                .fetch(&mut *exec)
                .try_fold(
                    DashMap::new(),
                    |acc_links: DashMap<ClientProfileId, Vec<ClientProfileLink>>, m| {
                        let link = ClientProfileLink {
                            id: ClientProfileLinkId(m.id),
                            shared_profile_id: ClientProfileId(m.shared_profile_id),
                            created: m.created,
                            expires: m.expires,
                        };
                        acc_links
                            .entry(ClientProfileId(m.shared_profile_id))
                            .or_default()
                            .push(link);
                        async move { Ok(acc_links) }
                    },
                )
                .await?;

            // One to many for shared_profiles to loaders, so can safely group by shared_profile_id
            let db_profiles: Vec<QueryClientProfile> = sqlx::query!(
                r#"
                SELECT sp.id, sp.name, sp.owner_id, sp.icon_url, sp.created, sp.updated, sp.loader_id,
                l.loader, g.name as game_name, g.id as game_id, sp.metadata,
                ARRAY_AGG(DISTINCT spu.user_id) filter (WHERE spu.user_id IS NOT NULL) as users,
                ARRAY_AGG(DISTINCT spl.id) filter (WHERE spl.id IS NOT NULL) as links
                FROM shared_profiles sp                
                LEFT JOIN shared_profiles_links spl ON spl.shared_profile_id = sp.id
                LEFT JOIN loaders l ON l.id = sp.loader_id
                LEFT JOIN shared_profiles_users spu ON spu.shared_profile_id = sp.id
                INNER JOIN games g ON g.id = sp.game_id
                WHERE sp.id = ANY($1)
                GROUP BY sp.id, l.id, g.id
                "#,
                &remaining_ids.iter().map(|x| x.0).collect::<Vec<i64>>()
            )
                .fetch_many(&mut *exec)
                .try_filter_map(|e| async {
                    Ok(e.right().map(|m| {
                        let id = ClientProfileId(m.id);
                        let versions = shared_profiles_mods.0.get(&id).map(|x| x.value().clone()).unwrap_or_default();
                        let files = shared_profiles_mods.1.get(&id).map(|x| x.value().clone()).unwrap_or_default();
                        let links = shared_profiles_links.remove(&id).map(|x| x.1).unwrap_or_default();
                        let game_id = GameId(m.game_id);
                        let metadata = serde_json::from_value::<ClientProfileMetadata>(m.metadata).unwrap_or(ClientProfileMetadata::Unknown);
                        QueryClientProfile {
                            inner: ClientProfile {
                                id,
                                name: m.name,
                                icon_url: m.icon_url,
                                updated: m.updated,
                                created: m.created,
                                owner_id: UserId(m.owner_id),
                                game_id,
                                users: m.users.unwrap_or_default().into_iter().map(UserId).collect(),
                                loader_id: LoaderId(m.loader_id),
                                game_name: m.game_name,
                                metadata,
                                loader: m.loader,
                                versions,
                                overrides: files
                            },
                            links
                        }
                    }))
                })
                .try_collect::<Vec<QueryClientProfile>>()
                .await?;

            for profile in db_profiles {
                redis
                    .set_serialized_to_json(
                        CLIENT_PROFILES_NAMESPACE,
                        profile.inner.id.0,
                        &profile,
                        None,
                    )
                    .await?;
                found_profiles.push(profile);
            }
        }

        Ok(found_profiles)
    }

    pub async fn clear_cache(id: ClientProfileId, redis: &RedisPool) -> Result<(), DatabaseError> {
        let mut redis = redis.connect().await?;

        redis
            .delete_many([(CLIENT_PROFILES_NAMESPACE, Some(id.0.to_string()))])
            .await?;
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClientProfileLink {
    pub id: ClientProfileLinkId,
    pub shared_profile_id: ClientProfileId,
    pub created: DateTime<Utc>,
    pub expires: DateTime<Utc>,
}

impl ClientProfileLink {
    pub async fn insert(
        &self,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), DatabaseError> {
        sqlx::query!(
            "
            INSERT INTO shared_profiles_links (
                id, shared_profile_id, created, expires
            )
            VALUES (
                $1, $2, $3, $4
            )
            ",
            self.id.0,
            self.shared_profile_id.0,
            self.created,
            self.expires,
        )
        .execute(&mut **transaction)
        .await?;

        Ok(())
    }

    pub async fn list<'a, 'b, E>(
        shared_profile_id: ClientProfileId,
        executor: E,
    ) -> Result<Vec<ClientProfileLink>, DatabaseError>
    where
        E: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut exec = executor.acquire().await?;

        let links = sqlx::query!(
            "
            SELECT id, shared_profile_id, created, expires
            FROM shared_profiles_links spl
            WHERE spl.shared_profile_id = $1
            ",
            shared_profile_id.0
        )
        .fetch_many(&mut *exec)
        .try_filter_map(|e| async {
            Ok(e.right().map(|m| ClientProfileLink {
                id: ClientProfileLinkId(m.id),
                shared_profile_id: ClientProfileId(m.shared_profile_id),
                created: m.created,
                expires: m.expires,
            }))
        })
        .try_collect::<Vec<ClientProfileLink>>()
        .await?;

        Ok(links)
    }

    pub async fn get<'a, 'b, E>(
        id: ClientProfileLinkId,
        executor: E,
    ) -> Result<Option<ClientProfileLink>, DatabaseError>
    where
        E: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut exec = executor.acquire().await?;

        let link = sqlx::query!(
            "
            SELECT id, shared_profile_id, created, expires
            FROM shared_profiles_links spl
            WHERE spl.id = $1
            ",
            id.0
        )
        .fetch_optional(&mut *exec)
        .await?
        .map(|m| ClientProfileLink {
            id: ClientProfileLinkId(m.id),
            shared_profile_id: ClientProfileId(m.shared_profile_id),
            created: m.created,
            expires: m.expires,
        });

        Ok(link)
    }
}

pub struct ClientProfileOverride {
    pub file_hash: String,
    pub url: String,
    pub install_path: PathBuf,
}
