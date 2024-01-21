use std::path::PathBuf;

use crate::{database::{models::VersionId, redis::RedisPool}, models::projects::FileType};

use super::{generate_file_id, ClientProfileId, DatabaseError, FileId};


#[derive(Clone, Debug)]
pub struct VersionFileBuilder {
    pub url: String,
    pub filename: String,
    pub hashes: Vec<HashBuilder>,
    pub primary: bool,
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
    // If one is providded, that file will be connected to the profile instead of creating a new one
    pub existing_file : Option<FileId>,
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
    file_ids : Vec<FileId>,
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
    let hashes : Vec<String> = sqlx::query!(
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