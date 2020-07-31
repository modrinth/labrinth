use crate::database::models;
use crate::database::models::version_item::VersionBuilder;
use crate::file_hosting::FileHost;
use crate::models::mods::{Version, GameVersion, ModId, VersionId, VersionType, VersionFile, ModLoader};
use crate::routes::mod_creation::{CreateError, UploadedFile};
use actix_multipart::{Field, Multipart};
use actix_web::web::Data;
use actix_web::{post, HttpResponse};
use futures::stream::StreamExt;
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPool;

#[derive(Serialize, Deserialize, Clone)]
pub struct InitialVersionData {
    pub mod_id: Option<ModId>,
    pub file_parts: Vec<String>,
    pub version_number: String,
    pub version_title: String,
    pub version_body: String,
    pub dependencies: Vec<VersionId>,
    pub game_versions: Vec<GameVersion>,
    pub release_channel: VersionType,
    pub loaders: Vec<ModLoader>,
}

#[post("api/v1/version")]
pub async fn version_create(
    payload: Multipart,
    client: Data<PgPool>,
    file_host: Data<std::sync::Arc<dyn FileHost + Send + Sync>>,
) -> Result<HttpResponse, CreateError> {
    let mut transaction = client.begin().await?;
    let mut uploaded_files = Vec::new();

    let result = version_create_inner(
        payload,
        &mut transaction,
        &***file_host,
        &mut uploaded_files,
    )
    .await;

    if result.is_err() {
        let undo_result = super::mod_creation::undo_uploads(&***file_host, &uploaded_files).await;
        let rollback_result = transaction.rollback().await;

        if let Err(e) = undo_result {
            return Err(e);
        }
        if let Err(e) = rollback_result {
            return Err(e.into());
        }
    } else {
        transaction.commit().await?;
    }

    result
}


async fn version_create_inner(
    mut payload: Multipart,
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    file_host: &dyn FileHost,
    uploaded_files: &mut Vec<UploadedFile>,
) -> Result<HttpResponse, CreateError> {
    //TODO: Check if mod exists, check if version number is unique
    let cdn_url = dotenv::var("CDN_URL")?;

    let mut initial_version_data = None;
    let mut version_builder = None;

    while let Some(item) = payload.next().await {
        let mut field: Field = item.map_err(CreateError::MultipartError)?;
        let content_disposition = field.content_disposition().ok_or_else(|| {
            CreateError::MissingValueError("Missing content disposition".to_string())
        })?;
        let name = content_disposition
            .get_name()
            .ok_or_else(|| CreateError::MissingValueError("Missing content name".to_string()))?;

        if name == "data" {
            let mut data = Vec::new();
            while let Some(chunk) = field.next().await {
                data.extend_from_slice(&chunk.map_err(CreateError::MultipartError)?);
            }

            let version_create_data: InitialVersionData = serde_json::from_slice(&data)?;
            initial_version_data = Some(version_create_data.clone());

            let mod_id: ModId = version_create_data.mod_id.ok_or_else(|| {
                CreateError::InvalidInput("Mod id is required for version data".to_string())
            })?;

            let results = sqlx::query!("SELECT EXISTS(SELECT 1 FROM mods WHERE id=$1)", mod_id.0 as i64).fetch_one(&mut *transaction).await?;

            if !results.exists.unwrap_or(true) {
                return Err(CreateError::InvalidInput("An invalid mod id was supplied".to_string()));
            }

            let version_id: VersionId = models::generate_version_id(transaction).await?.into();
            let body_url = format!("data/{}/changelogs/{}/body.md", mod_id, version_id);

            let uploaded_text = file_host
                .upload_file(
                    "text/plain",
                    &body_url,
                    version_create_data.version_body.clone().into_bytes(),
                )
                .await?;

            uploaded_files.push(UploadedFile {
                file_id: uploaded_text.file_id.clone(),
                file_name: uploaded_text.file_name.clone(),
            });

            // TODO: do a real lookup for the channels
            let release_channel = match version_create_data.release_channel {
                VersionType::Release => models::ChannelId(0),
                VersionType::Beta => models::ChannelId(2),
                VersionType::Alpha => models::ChannelId(4),
            };

            version_builder = Some(VersionBuilder {
                version_id: version_id.into(),
                mod_id: models::ModId(mod_id.0 as i64),
                name: version_create_data.version_title.clone(),
                version_number: version_create_data.version_number.clone(),
                changelog_url: Some(format!("{}/{}", cdn_url, body_url)),
                files: Vec::with_capacity(1),
                dependencies: version_create_data
                    .dependencies
                    .iter()
                    .map(|x| (*x).into())
                    .collect::<Vec<_>>(),
                // TODO: add game_versions and loaders info
                game_versions: vec![],
                loaders: vec![],
                release_channel,
            });

            continue;
        }

        let file_name = content_disposition.get_filename().ok_or_else(|| {
            CreateError::MissingValueError("Missing content file name".to_string())
        })?;
        let file_extension = if let Some(last_period) = file_name.rfind('.') {
            file_name.get((last_period + 1)..).unwrap_or("")
        } else {
            return Err(CreateError::MissingValueError(
                "Missing content file extension".to_string(),
            ));
        };

        if &*file_extension == "jar" {
            let version = version_builder.as_mut().ok_or_else(|| {
                CreateError::InvalidInput(String::from("`data` field must come before file fields"))
            })?;

            let mut data = Vec::new();
            while let Some(chunk) = field.next().await {
                data.extend_from_slice(&chunk.map_err(CreateError::MultipartError)?);
            }

            let upload_data = file_host
                .upload_file(
                    "application/java-archive",
                    &format!(
                        "{}/{}/{}",
                        version.mod_id.0, version.version_number, file_name
                    ),
                    data.to_vec(),
                )
                .await?;

            uploaded_files.push(UploadedFile {
                file_id: upload_data.file_id.clone(),
                file_name: upload_data.file_name.clone(),
            });

            // Add the newly uploaded file to the existing or new version

            // TODO: Malware scan + file validation
            version
                .files
                .push(models::version_item::VersionFileBuilder {
                    filename: file_name.to_string(),
                    url: format!("{}/{}", cdn_url, upload_data.file_name),
                    hashes: vec![models::version_item::HashBuilder {
                        algorithm: "sha1".to_string(),
                        // This is an invalid cast - the database expects the hash's
                        // bytes, but this is the string version.
                        hash: upload_data.content_sha1.into_bytes(),
                    }],
                });
        }
    }

    let version_data_safe = initial_version_data
        .ok_or_else(|| CreateError::InvalidInput("`data` field is required".to_string()))?;
    let version_builder_safe = version_builder
        .ok_or_else(|| CreateError::InvalidInput("`data` field is required".to_string()))?;


    let response = Version {
        id: VersionId(version_builder_safe.version_id.0 as u64),
        mod_id: ModId(version_builder_safe.mod_id.0 as u64),
        name: version_builder_safe.name.clone(),
        version_number: version_builder_safe.version_number.clone(),
        changelog_url: version_builder_safe.changelog_url.clone(),
        date_published: chrono::Utc::now(),
        downloads: 0,
        version_type: version_data_safe.release_channel,
        files: version_builder_safe.files.iter().map(|x| VersionFile {
            // TODO: Put real data here
            hashes: vec![],
            url: String::new(),
        }).collect::<Vec<_>>(),
        dependencies: version_data_safe.dependencies,
        game_versions: version_data_safe.game_versions,
        loaders: version_data_safe.loaders,
        
    };
    
    version_builder_safe.insert(transaction).await?;

    Ok(HttpResponse::Ok().json(response).into())
}