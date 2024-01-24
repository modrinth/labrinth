use std::{collections::HashMap, path::PathBuf};

use itertools::Itertools;

use crate::{
    database::{models::VersionId, redis::RedisPool},
    models::{self, projects::FileType},
    routes::CommonError,
};

use super::{client_profile_item, generate_file_id, ClientProfileId, DatabaseError, FileId};

#[derive(Clone, Debug)]
pub struct VersionFileBuilder {
    pub url: String,
    pub filename: String,
    pub hashes: Vec<HashBuilder>,
    pub primary: bool,
    // Whether a new file should be generated or an existing one should be used
    // If one is provided, that file will be connected to the version instead of creating a new one
    // This is used on rare allowable hash collisions, such as two unapproved versions
    // No two approved versions should ever have the same file- this is enforced elsewhere
    pub existing_file: Option<FileId>,
    pub size: u32,
    pub file_type: Option<FileType>,
}

#[derive(Clone, Debug)]
pub struct HashBuilder {
    pub algorithm: String,
    pub hash: Vec<u8>,
}

impl VersionFileBuilder {
    pub async fn insert(
        self,
        version_id: VersionId,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<FileId, DatabaseError> {
        let file_id = if let Some(file_id) = self.existing_file {
            file_id
        } else {
            let file_id = generate_file_id(&mut *transaction).await?;

            sqlx::query!(
                "
                INSERT INTO files (id, url, filename, size, file_type)
                VALUES ($1, $2, $3, $4, $5)
                ",
                file_id as FileId,
                self.url,
                self.filename,
                self.size as i32,
                self.file_type.map(|x| x.as_str()),
            )
            .execute(&mut **transaction)
            .await?;

            for hash in self.hashes {
                sqlx::query!(
                    "
                    INSERT INTO hashes (file_id, algorithm, hash)
                    VALUES ($1, $2, $3)
                    ",
                    file_id as FileId,
                    hash.algorithm,
                    hash.hash,
                )
                .execute(&mut **transaction)
                .await?;
            }

            file_id
        };

        sqlx::query!(
            "
            INSERT INTO versions_files (version_id, file_id, is_primary)
            VALUES ($1, $2, $3)
            ",
            version_id as VersionId,
            file_id as FileId,
            self.primary,
        )
        .execute(&mut **transaction)
        .await?;

        Ok(file_id)
    }
}

#[derive(Clone, Debug)]
pub struct ClientProfileFileBuilder {
    pub url: String,
    pub filename: String,
    pub hashes: Vec<HashBuilder>,
    pub install_path: PathBuf,
    // Whether a new file should be generated or an existing one should be used
    // If one is provided, that file will be connected to the profile instead of creating a new one
    pub existing_file: Option<FileId>,
    pub size: u32,
    pub file_type: Option<FileType>,
}

impl ClientProfileFileBuilder {
    pub async fn insert(
        self,
        profile_id: ClientProfileId,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<FileId, DatabaseError> {
        let file_id = if let Some(file_id) = self.existing_file {
            file_id
        } else {
            let file_id = generate_file_id(&mut *transaction).await?;

            sqlx::query!(
                "
                INSERT INTO files (id, url, filename, size, file_type)
                VALUES ($1, $2, $3, $4, $5)
                ",
                file_id as FileId,
                self.url,
                self.filename,
                self.size as i32,
                self.file_type.map(|x| x.as_str()),
            )
            .execute(&mut **transaction)
            .await?;

            for hash in self.hashes {
                sqlx::query!(
                    "
                    INSERT INTO hashes (file_id, algorithm, hash)
                    VALUES ($1, $2, $3)
                    ",
                    file_id as FileId,
                    hash.algorithm,
                    hash.hash,
                )
                .execute(&mut **transaction)
                .await?;
            }

            file_id
        };

        sqlx::query!(
            "
            INSERT INTO shared_profiles_files (shared_profile_id, file_id, install_path)
            VALUES ($1, $2, $3)
            ",
            profile_id as ClientProfileId,
            file_id as FileId,
            self.install_path.to_string_lossy().to_string(),
        )
        .execute(&mut **transaction)
        .await?;

        Ok(file_id)
    }
}

// Remove files that are not referenced by any versions_files or shared_profiles_files
// This is a separate function because it is used in multiple places
// Returns a list of hashes that were deleted, so they can be removed from the file host
pub async fn remove_unreferenced_files(
    file_ids: Vec<FileId>,
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<Vec<String>, DatabaseError> {
    let file_ids = file_ids.into_iter().map(|x| x.0).collect::<Vec<_>>();

    // Check if any versions_files or shared_profiles_files still reference the file- these files should not be deleted
    let referenced_files = sqlx::query!(
        "
        SELECT f.id
        FROM files f
        LEFT JOIN versions_files vf ON vf.file_id = f.id
        LEFT JOIN shared_profiles_files spf ON spf.file_id = f.id
        WHERE f.id = ANY($1) AND (vf.version_id IS NOT NULL OR spf.shared_profile_id IS NOT NULL)
        ",
        &file_ids[..],
    )
    .fetch_all(&mut **transaction)
    .await?
    .into_iter()
    .filter_map(|x| x.id)
    .collect::<Vec<_>>();

    // Filter out the referenced files
    let file_ids = file_ids
        .into_iter()
        .filter(|x| !referenced_files.contains(x))
        .collect::<Vec<_>>();

    // Delete hashes for the files remaining
    let hashes: Vec<String> = sqlx::query!(
        "
        DELETE FROM hashes
        WHERE EXISTS(
            SELECT 1 FROM files WHERE
                (files.id = ANY($1) AND hashes.file_id = files.id)
        )
        RETURNING encode(hashes.hash, 'escape') hash
        ",
        &file_ids[..],
    )
    .fetch_all(&mut **transaction)
    .await?
    .into_iter()
    .filter_map(|x| x.hash)
    .collect::<Vec<_>>();

    // Delete files remaining
    sqlx::query!(
        "
        DELETE FROM files
        WHERE files.id = ANY($1)
        ",
        &file_ids[..],
    )
    .execute(&mut **transaction)
    .await?;

    Ok(hashes)
}

// Converts shared_profiles_files to shared_profiles_versions for cases of
// hash collisions for files that versions now 'own'.
// It also ensures that all files have at exactly one approved version- the one that was just approved.
// It returns a schema error if any file has multiple approved versions (reverting the transaction)
// (Before they are approved, uploaded files can have hash collections)
// This is a separate function because it is used in multiple places.
pub async fn convert_hash_collisions_to_versions<T>(
    approved_version_ids: &[VersionId],
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    redis: &RedisPool,
) -> Result<(), T>
where
    T: CommonError + From<DatabaseError> + From<sqlx::error::Error>,
{
    // First, get all file id associated with these versions
    let file_ids: HashMap<FileId, VersionId> = sqlx::query!(
        "
        SELECT version_id, file_id
        FROM versions_files
        WHERE version_id = ANY($1)
        ",
        &approved_version_ids.iter().map(|x| x.0).collect::<Vec<_>>()[..],
    )
    .fetch_all(&mut **transaction)
    .await?
    .into_iter()
    .map(|x| (FileId(x.file_id), VersionId(x.version_id)))
    .collect();

    // For each file, get all approved project's versions that have that file
    let existing_approved_versions: HashMap<FileId, Vec<VersionId>> = sqlx::query!(
        "
        SELECT version_id, file_id
        FROM versions_files vf
        LEFT JOIN versions v ON v.id = vf.version_id
        LEFT JOIN mods m ON m.id = v.mod_id
        WHERE m.status = ANY($1) AND file_id = ANY($2::bigint[])
        ",
        &*crate::models::projects::ProjectStatus::iterator()
            .filter(|x| x.is_approved())
            .map(|x| x.to_string())
            .collect::<Vec<String>>(),
        &file_ids.keys().map(|x| x.0).collect::<Vec<_>>()[..],
    )
    .fetch_all(&mut **transaction)
    .await?
    .into_iter()
    .map(|x| (FileId(x.file_id), VersionId(x.version_id)))
    .into_group_map();

    // Ensure that all files have at exactly one approved version- the one that was just approved
    for (file_id, version_ids) in existing_approved_versions {
        let Some(intended_version_id) = file_ids.get(&file_id) else {
            continue;
        };

        if version_ids.len() != 1 || !version_ids.contains(intended_version_id) {
            let versions: Vec<models::v3::projects::VersionId> =
                version_ids.iter().map(|x| (*x).into()).collect();
            return Err(T::invalid_input(format!(
                "File {} has existing or multiple approved versions: {}",
                file_id.0,
                versions.into_iter().join(", ")
            )));
        }
    }

    // Delete all shared_profiles_files that reference these files
    let shared_profile_ids: Vec<(ClientProfileId, FileId)> = sqlx::query!(
        "
        DELETE FROM shared_profiles_files
        WHERE file_id = ANY($1::bigint[])
        RETURNING shared_profile_id, file_id
        ",
        &file_ids.keys().map(|x| x.0).collect::<Vec<_>>()[..],
    )
    .fetch_all(&mut **transaction)
    .await?
    .into_iter()
    .map(|x| (ClientProfileId(x.shared_profile_id), FileId(x.file_id)))
    .collect();

    // Add as versions
    let versions_to_add: Vec<(ClientProfileId, VersionId)> = shared_profile_ids
        .into_iter()
        .filter_map(|(profile_id, file_id)| file_ids.get(&file_id).map(|x| (profile_id, *x)))
        .collect();
    let (client_profile_ids, version_ids): (Vec<_>, Vec<_>) =
        versions_to_add.iter().map(|x| (x.0 .0, x.1 .0)).unzip();
    sqlx::query!(
        "
        INSERT INTO shared_profiles_versions (shared_profile_id, version_id)
        SELECT * FROM UNNEST($1::bigint[], $2::bigint[])
        ",
        &client_profile_ids[..],
        &version_ids[..],
    )
    .execute(&mut **transaction)
    .await?;

    // Set updated of all hit profiles
    sqlx::query!(
        "
        UPDATE shared_profiles
        SET updated = NOW()
        WHERE id = ANY($1::bigint[])
        ",
        &client_profile_ids[..],
    )
    .execute(&mut **transaction)
    .await?;

    // Clear cache of all hit profiles
    for profile_id in client_profile_ids {
        client_profile_item::ClientProfile::clear_cache(ClientProfileId(profile_id), redis).await?;
    }

    Ok(())
}
