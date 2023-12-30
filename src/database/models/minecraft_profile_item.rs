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

pub const MINECRAFT_PROFILES_NAMESPACE: &str = "minecraft_profiles";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MinecraftProfile {
    pub id: MinecraftProfileId,
    pub name: String,
    pub owner_id: UserId,
    pub icon_url: Option<String>,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,

    pub game_version_id: LoaderFieldEnumValueId,
    pub loader_version: String,

    pub maximum_users: i32,
    pub users: Vec<UserId>,

    // These represent the same loader
    pub loader_id: LoaderId,
    pub loader: String,

    pub versions: Vec<VersionId>,
    pub overrides: Vec<Override>,
}

impl MinecraftProfile {
    pub async fn insert(
        &self,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), DatabaseError> {
        sqlx::query!(
            "
            INSERT INTO shared_profiles (
                id, name, owner_id, icon_url, created, updated,
                game_version_id, loader_id, loader_version, maximum_users
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, 
                $7, $8, $9, $10
            )
            ",
            self.id as MinecraftProfileId,
            self.name,
            self.owner_id as UserId,
            self.icon_url,
            self.created,
            self.updated,
            self.game_version_id as LoaderFieldEnumValueId,
            self.loader_id as LoaderId,
            self.loader_version,
            self.maximum_users,
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
                self.id as MinecraftProfileId,
                user_id.0,
            )
            .execute(&mut **transaction)
            .await?;
        }

        Ok(())
    }

    pub async fn remove(
        id: MinecraftProfileId,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        redis: &RedisPool,
    ) -> Result<Option<()>, DatabaseError> {
        // Delete shared_profiles_links_tokens
        sqlx::query!(
            "
            DELETE FROM cdn_auth_tokens
            WHERE shared_profile_id = $1
            ",
            id as MinecraftProfileId,
        )
        .execute(&mut **transaction)
        .await?;

        // Delete shared_profiles_links
        sqlx::query!(
            "
            DELETE FROM shared_profiles_links
            WHERE shared_profile_id = $1
            ",
            id as MinecraftProfileId,
        )
        .execute(&mut **transaction)
        .await?;

        // Delete shared_profiles_users
        sqlx::query!(
            "
            DELETE FROM shared_profiles_users
            WHERE shared_profile_id = $1
            ",
            id as MinecraftProfileId,
        )
        .execute(&mut **transaction)
        .await?;

        sqlx::query!(
            "
            DELETE FROM shared_profiles_mods
            WHERE shared_profile_id = $1
            ",
            id as MinecraftProfileId,
        )
        .execute(&mut **transaction)
        .await?;

        sqlx::query!(
            "
            DELETE FROM shared_profiles_links
            WHERE shared_profile_id = $1
            ",
            id as MinecraftProfileId,
        )
        .execute(&mut **transaction)
        .await?;

        sqlx::query!(
            "
            DELETE FROM shared_profiles
            WHERE id = $1
            ",
            id as MinecraftProfileId,
        )
        .execute(&mut **transaction)
        .await?;

        MinecraftProfile::clear_cache(id, redis).await?;

        Ok(Some(()))
    }

    pub async fn get<'a, 'b, E>(
        id: MinecraftProfileId,
        executor: E,
        redis: &RedisPool,
    ) -> Result<Option<MinecraftProfile>, DatabaseError>
    where
        E: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        Self::get_many(&[id], executor, redis)
            .await
            .map(|x| x.into_iter().next())
    }

    pub async fn get_many<'a, E>(
        ids: &[MinecraftProfileId],
        exec: E,
        redis: &RedisPool,
    ) -> Result<Vec<MinecraftProfile>, DatabaseError>
    where
        E: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut redis = redis.connect().await?;
        let mut exec = exec.acquire().await?;

        let mut found_profiles = Vec::new();
        let mut remaining_ids: Vec<MinecraftProfileId> = ids.to_vec();

        if !ids.is_empty() {
            let profiles = redis
                .multi_get::<String>(MINECRAFT_PROFILES_NAMESPACE, ids.iter().map(|x| x.0))
                .await?;
            for profile in profiles {
                if let Some(profile) =
                    profile.and_then(|x| serde_json::from_str::<MinecraftProfile>(&x).ok())
                {
                    remaining_ids.retain(|x| profile.id != *x);
                    found_profiles.push(profile);
                    continue;
                }
            }
        }

        if !remaining_ids.is_empty() {
            type AttachedProjectsMap = (
                DashMap<MinecraftProfileId, Vec<VersionId>>,
                DashMap<MinecraftProfileId, Vec<Override>>,
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
                            .entry(MinecraftProfileId(m.shared_profile_id))
                            .or_default()
                            .push(version_id);
                    }

                    if let (Some(install_path), Some(file_hash)) = (install_path, file_hash) {
                        acc_overrides
                            .entry(MinecraftProfileId(m.shared_profile_id))
                            .or_default()
                            .push((file_hash, PathBuf::from(install_path)));
                    }

                    async move { Ok((acc_versions, acc_overrides)) }
                },
            )
            .await?;

            // One to many for shared_profiles to loaders, so can safely group by shared_profile_id
            let db_profiles: Vec<MinecraftProfile> = sqlx::query!(
                "
                SELECT sp.id, sp.name, sp.owner_id, sp.icon_url, sp.created, sp.updated, sp.game_version_id, sp.loader_id, l.loader, sp.loader_version, sp.maximum_users,
                ARRAY_AGG(DISTINCT spu.user_id) filter (WHERE spu.user_id IS NOT NULL) as users
                FROM shared_profiles sp                
                LEFT JOIN loaders l ON l.id = sp.loader_id
                LEFT JOIN shared_profiles_users spu ON spu.shared_profile_id = sp.id
                WHERE sp.id = ANY($1)
                GROUP BY sp.id, l.id
                ",
                &remaining_ids.iter().map(|x| x.0).collect::<Vec<i64>>()
            )
                .fetch_many(&mut *exec)
                .try_filter_map(|e| async {
                    Ok(e.right().map(|m| {
                        let id = MinecraftProfileId(m.id);
                        let versions = shared_profiles_mods.0.get(&id).map(|x| x.value().clone()).unwrap_or_default();
                        let files = shared_profiles_mods.1.get(&id).map(|x| x.value().clone()).unwrap_or_default();
                        MinecraftProfile {
                            id,
                            name: m.name,
                            icon_url: m.icon_url,
                            updated: m.updated,
                            created: m.created,
                            owner_id: UserId(m.owner_id),
                            game_version_id: LoaderFieldEnumValueId(m.game_version_id),
                            users: m.users.unwrap_or_default().into_iter().map(UserId).collect(),
                            loader_id: LoaderId(m.loader_id),
                            loader_version: m.loader_version,
                            loader: m.loader,
                            maximum_users: m.maximum_users,
                            versions,
                            overrides: files
                        }
                    }))
                })
                .try_collect::<Vec<MinecraftProfile>>()
                .await?;

            for profile in db_profiles {
                redis
                    .set_serialized_to_json(
                        MINECRAFT_PROFILES_NAMESPACE,
                        profile.id.0,
                        &profile,
                        None,
                    )
                    .await?;
                found_profiles.push(profile);
            }
        }

        Ok(found_profiles)
    }

    pub async fn clear_cache(
        id: MinecraftProfileId,
        redis: &RedisPool,
    ) -> Result<(), DatabaseError> {
        let mut redis = redis.connect().await?;

        redis
            .delete_many([(MINECRAFT_PROFILES_NAMESPACE, Some(id.0.to_string()))])
            .await?;
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MinecraftProfileLink {
    pub id: MinecraftProfileLinkId,
    pub link_identifier: String,
    pub shared_profile_id: MinecraftProfileId,
    pub created: DateTime<Utc>,
    pub expires: DateTime<Utc>,
}

impl MinecraftProfileLink {
    pub async fn insert(
        &self,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), DatabaseError> {
        sqlx::query!(
            "
            INSERT INTO shared_profiles_links (
                id, link, shared_profile_id, created, expires
            )
            VALUES (
                $1, $2, $3, $4, $5
            )
            ",
            self.id.0,
            self.link_identifier,
            self.shared_profile_id.0,
            self.created,
            self.expires,
        )
        .execute(&mut **transaction)
        .await?;

        Ok(())
    }

    pub async fn list<'a, 'b, E>(
        shared_profile_id: MinecraftProfileId,
        executor: E,
    ) -> Result<Vec<MinecraftProfileLink>, DatabaseError>
    where
        E: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut exec = executor.acquire().await?;

        let links = sqlx::query!(
            "
            SELECT id, link, shared_profile_id, created, expires
            FROM shared_profiles_links spl
            WHERE spl.shared_profile_id = $1
            ",
            shared_profile_id.0
        )
        .fetch_many(&mut *exec)
        .try_filter_map(|e| async {
            Ok(e.right().map(|m| MinecraftProfileLink {
                id: MinecraftProfileLinkId(m.id),
                link_identifier: m.link,
                shared_profile_id: MinecraftProfileId(m.shared_profile_id),
                created: m.created,
                expires: m.expires,
            }))
        })
        .try_collect::<Vec<MinecraftProfileLink>>()
        .await?;

        Ok(links)
    }

    pub async fn get<'a, 'b, E>(
        id: MinecraftProfileLinkId,
        executor: E,
    ) -> Result<Option<MinecraftProfileLink>, DatabaseError>
    where
        E: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut exec = executor.acquire().await?;

        let link = sqlx::query!(
            "
            SELECT id, link, shared_profile_id, created, expires
            FROM shared_profiles_links spl
            WHERE spl.id = $1
            ",
            id.0
        )
        .fetch_optional(&mut *exec)
        .await?
        .map(|m| MinecraftProfileLink {
            id: MinecraftProfileLinkId(m.id),
            link_identifier: m.link,
            shared_profile_id: MinecraftProfileId(m.shared_profile_id),
            created: m.created,
            expires: m.expires,
        });

        Ok(link)
    }

    pub async fn get_url<'a, 'b, E>(
        url_identifier: &str,
        executor: E,
    ) -> Result<Option<MinecraftProfileLink>, DatabaseError>
    where
        E: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut exec = executor.acquire().await?;

        let link = sqlx::query!(
            "
            SELECT id, link, shared_profile_id, created, expires
            FROM shared_profiles_links spl
            WHERE spl.link = $1
            ",
            url_identifier
        )
        .fetch_optional(&mut *exec)
        .await?
        .map(|m| MinecraftProfileLink {
            id: MinecraftProfileLinkId(m.id),
            link_identifier: m.link,
            shared_profile_id: MinecraftProfileId(m.shared_profile_id),
            created: m.created,
            expires: m.expires,
        });

        Ok(link)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MinecraftProfileLinkToken {
    pub token: String,
    pub shared_profile_id: MinecraftProfileId,
    pub user_id: UserId,
    pub created: DateTime<Utc>,
    pub expires: DateTime<Utc>,
}

impl MinecraftProfileLinkToken {
    pub async fn insert(
        &self,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), DatabaseError> {
        sqlx::query!(
            "
            INSERT INTO cdn_auth_tokens (
                token, shared_profile_id, user_id, created, expires
            )
            VALUES (
                $1, $2, $3, $4, $5
            )
            ",
            self.token,
            self.shared_profile_id.0,
            self.user_id.0,
            self.created,
            self.expires,
        )
        .execute(&mut **transaction)
        .await?;

        Ok(())
    }

    pub async fn get_token<'a, 'b, E>(
        token: &str,
        executor: E,
    ) -> Result<Option<MinecraftProfileLinkToken>, DatabaseError>
    where
        E: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut exec = executor.acquire().await?;

        let token = sqlx::query!(
            "
            SELECT token, user_id, shared_profile_id, created, expires
            FROM cdn_auth_tokens cat
            WHERE cat.token = $1
            ",
            token
        )
        .fetch_optional(&mut *exec)
        .await?
        .map(|m| MinecraftProfileLinkToken {
            token: m.token,
            user_id: UserId(m.user_id),
            shared_profile_id: MinecraftProfileId(m.shared_profile_id),
            created: m.created,
            expires: m.expires,
        });

        Ok(token)
    }

    // Get existing token for profile and user
    pub async fn get_from_profile_user<'a, 'b, E>(
        profile_id: MinecraftProfileId,
        user_id: UserId,
        executor: E,
    ) -> Result<Option<MinecraftProfileLinkToken>, DatabaseError>
    where
        E: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut exec = executor.acquire().await?;

        let token = sqlx::query!(
            "
            SELECT cat.token, cat.user_id, cat.shared_profile_id, cat.created, cat.expires
            FROM cdn_auth_tokens cat
            INNER JOIN shared_profiles sp ON sp.id = cat.shared_profile_id
            WHERE sp.id = $1 AND cat.user_id = $2
            ",
            profile_id.0,
            user_id.0
        )
        .fetch_optional(&mut *exec)
        .await?
        .map(|m| MinecraftProfileLinkToken {
            token: m.token,
            user_id: UserId(m.user_id),
            shared_profile_id: MinecraftProfileId(m.shared_profile_id),
            created: m.created,
            expires: m.expires,
        });

        Ok(token)
    }

    pub async fn delete(
        token: &str,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), DatabaseError> {
        sqlx::query!(
            "
            DELETE FROM cdn_auth_tokens
            WHERE token = $1
            ",
            token
        )
        .execute(&mut **transaction)
        .await?;

        Ok(())
    }

    pub async fn delete_all(
        shared_profile_id: MinecraftProfileId,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), DatabaseError> {
        sqlx::query!(
            "
            DELETE FROM cdn_auth_tokens
            WHERE shared_profile_id = $1
            ",
            shared_profile_id.0
        )
        .execute(&mut **transaction)
        .await?;

        Ok(())
    }
}

pub struct MinecraftProfileOverride {
    pub file_hash: String,
    pub url: String,
    pub install_path: PathBuf,
}
