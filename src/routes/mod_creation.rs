use crate::database::models::{FileHash, Team, Version, VersionFile};
use crate::file_hosting::{FileHost, FileHostingError};
use crate::models::error::ApiError;
use crate::models::ids::random_base62;
use crate::models::mods::{GameVersion, ModId, VersionId, VersionType};
use crate::models::teams::{TeamId, TeamMember};
use actix_multipart::{Field, Multipart};
use actix_web::http::StatusCode;
use actix_web::web::Data;
use actix_web::{post, HttpResponse};
use chrono::Utc;
use futures::stream::StreamExt;
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPool;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CreateError {
    #[error("Environment Error")]
    EnvError(#[from] dotenv::Error),
    #[error("Error while adding project to database")]
    DatabaseError(#[from] sqlx::error::Error),
    #[error("Error while parsing multipart payload")]
    MultipartError(actix_multipart::MultipartError),
    #[error("Error while parsing JSON: {0}")]
    SerDeError(#[from] serde_json::Error),
    #[error("Error while uploading file")]
    FileHostingError(#[from] FileHostingError),
    #[error("{}", .0)]
    MissingValueError(String),
    #[error("Error while trying to generate random ID")]
    RandomIdError,
    #[error("Invalid format for mod icon: {0}")]
    InvalidIconFormat(String),
    #[error("Error with multipart data: {0}")]
    InvalidInput(String),
}

impl actix_web::ResponseError for CreateError {
    fn status_code(&self) -> StatusCode {
        match self {
            CreateError::EnvError(..) => StatusCode::INTERNAL_SERVER_ERROR,
            CreateError::DatabaseError(..) => StatusCode::INTERNAL_SERVER_ERROR,
            CreateError::FileHostingError(..) => StatusCode::INTERNAL_SERVER_ERROR,
            CreateError::SerDeError(..) => StatusCode::BAD_REQUEST,
            CreateError::MultipartError(..) => StatusCode::BAD_REQUEST,
            CreateError::MissingValueError(..) => StatusCode::BAD_REQUEST,
            CreateError::InvalidIconFormat(..) => StatusCode::BAD_REQUEST,
            CreateError::InvalidInput(..) => StatusCode::BAD_REQUEST,
            CreateError::RandomIdError => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status_code()).json(ApiError {
            error: match self {
                CreateError::EnvError(..) => "environment_error",
                CreateError::DatabaseError(..) => "database_error",
                CreateError::FileHostingError(..) => "file_hosting_error",
                CreateError::SerDeError(..) => "invalid_input",
                CreateError::MultipartError(..) => "invalid_input",
                CreateError::MissingValueError(..) => "invalid_input",
                CreateError::RandomIdError => "id_generation_error",
                CreateError::InvalidIconFormat(..) => "invalid_input",
                CreateError::InvalidInput(..) => "invalid_input",
            },
            description: &self.to_string(),
        })
    }
}

#[derive(Serialize, Deserialize, Clone)]
struct InitialVersionData {
    pub file_parts: Vec<String>,
    pub version_number: String,
    pub version_title: String,
    pub version_body: String,
    pub dependencies: Vec<VersionId>,
    pub game_versions: Vec<GameVersion>,
    pub version_type: VersionType,
    pub loaders: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone)]
struct ModCreateData {
    /// The title or name of the mod.
    pub mod_name: String,
    /// The namespace of the mod
    pub mod_namespace: String,
    /// A short description of the mod.
    pub mod_description: String,
    /// A long description of the mod, in markdown.
    pub mod_body: String,
    /// A list of initial versions to upload with the created mod
    pub initial_versions: Vec<InitialVersionData>,
    /// The team of people that has ownership of this mod.
    pub team_members: Vec<TeamMember>,
    /// A list of the categories that the mod is in.
    pub categories: Vec<String>,
    /// An optional link to where to submit bugs or issues with the mod.
    pub issues_url: Option<String>,
    /// An optional link to the source code for the mod.
    pub source_url: Option<String>,
    /// An optional link to the mod's wiki page or other relevant information.
    pub wiki_url: Option<String>,
}

const ID_RETRY_COUNT: usize = 20;

macro_rules! generate_ids {
    ($function_name:ident, $return_type:ty, $id_length:expr, $select_stmnt:literal, $id_function:expr) => {
        async fn $function_name(
            con: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        ) -> Result<$return_type, CreateError> {
            let length = $id_length;
            let mut id = random_base62(length);
            let mut retry_count = 0;

            // Check if ID is unique
            loop {
                let results = sqlx::query!($select_stmnt, id as i64)
                    .fetch_one(&mut *con)
                    .await?;

                if results.exists.unwrap_or(true) {
                    id = random_base62(length);
                } else {
                    break;
                }

                retry_count += 1;
                if retry_count > ID_RETRY_COUNT {
                    return Err(CreateError::RandomIdError);
                }
            }

            Ok($id_function(id))
        }
    };
}

generate_ids!(
    generate_mod_id,
    ModId,
    8,
    "SELECT EXISTS(SELECT 1 FROM mods WHERE id=$1)",
    ModId
);
generate_ids!(
    generate_version_id,
    VersionId,
    8,
    "SELECT EXISTS(SELECT 1 FROM versions WHERE id=$1)",
    VersionId
);
generate_ids!(
    generate_team_id,
    TeamId,
    8,
    "SELECT EXISTS(SELECT 1 FROM teams WHERE id=$1)",
    TeamId
);

struct UploadedFile {
    file_id: String,
    file_name: String,
}

async fn undo_uploads(
    file_host: &dyn FileHost,
    uploaded_files: &[UploadedFile],
) -> Result<(), CreateError> {
    for file in uploaded_files {
        file_host
            .delete_file_version(&file.file_id, &file.file_name)
            .await?;
    }
    Ok(())
}

#[post("api/v1/mod")]
pub async fn mod_create(
    payload: Multipart,
    client: Data<PgPool>,
    file_host: Data<std::sync::Arc<dyn FileHost + Send + Sync>>,
) -> Result<HttpResponse, CreateError> {
    let mut transaction = client.begin().await?;
    let mut uploaded_files = Vec::new();

    let result = mod_create_inner(
        payload,
        &mut transaction,
        &***file_host,
        &mut uploaded_files,
    )
    .await;

    if result.is_err() {
        let undo_result = undo_uploads(&***file_host, &uploaded_files).await;
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

async fn mod_create_inner(
    mut payload: Multipart,
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    file_host: &dyn FileHost,
    uploaded_files: &mut Vec<UploadedFile>,
) -> Result<HttpResponse, CreateError> {
    let cdn_url = dotenv::var("CDN_URL")?;

    let mod_id = generate_mod_id(transaction).await?;

    let mut created_versions: Vec<Version> = vec![];

    let mut mod_create_data: Option<ModCreateData> = None;
    let mut icon_url = "".to_string();

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
            mod_create_data = Some(serde_json::from_slice(&data)?);
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

        if name == "icon" {
            icon_url = process_icon_upload(
                uploaded_files,
                mod_id,
                file_name,
                file_extension,
                file_host,
                field,
                &cdn_url,
            )
            .await?;
            continue;
        }

        if &*file_extension == "jar" {
            let create_data = mod_create_data.as_ref().ok_or_else(|| {
                CreateError::InvalidInput(String::from("`data` field must come before file fields"))
            })?;

            let version_data = create_data
                .initial_versions
                .iter()
                .find(|x| x.file_parts.iter().any(|n| n == name))
                .ok_or_else(|| {
                    CreateError::InvalidInput(format!(
                        "Jar file `{}` (field {}) isn't specified in the versions data",
                        file_name, name
                    ))
                })?;

            // If a version has already been created for this version, add the
            // file to it instead of creating a new version.

            let created_version = if let Some(created_version) = created_versions
                .iter_mut()
                .find(|x| x.version_number == version_data.version_number)
            {
                created_version
            } else {
                let version_id = generate_version_id(transaction).await?;

                let body_url = format!("data/{}/changelogs/{}/body.md", mod_id, version_id);

                let uploaded_text = file_host
                    .upload_file(
                        "text/plain",
                        &body_url,
                        version_data.version_body.clone().into_bytes(),
                    )
                    .await?;

                uploaded_files.push(UploadedFile {
                    file_id: uploaded_text.file_id.clone(),
                    file_name: uploaded_text.file_name.clone(),
                });

                let version = Version {
                    version_id: version_id.0 as i64,
                    mod_id: mod_id.0 as i64,
                    name: version_data.version_title.clone(),
                    version_number: version_data.version_number.clone(),
                    changelog_url: Some(format!("{}/{}", cdn_url, body_url)),
                    date_published: Utc::now().to_rfc2822(),
                    downloads: 0,
                    version_type: version_data.version_type.to_string(),
                    files: Vec::with_capacity(1),
                    dependencies: version_data
                        .dependencies
                        .iter()
                        .map(|x| x.0 as i64)
                        .collect::<Vec<_>>(),
                    game_versions: vec![],
                    loaders: vec![],
                };

                created_versions.push(version);
                created_versions.last_mut().unwrap()
            };

            // Upload the new jar file

            let mut data = Vec::new();
            while let Some(chunk) = field.next().await {
                data.extend_from_slice(&chunk.map_err(CreateError::MultipartError)?);
            }

            let upload_data = file_host
                .upload_file(
                    "application/java-archive",
                    &format!(
                        "{}/{}/{}",
                        create_data.mod_namespace.replace(".", "/"),
                        version_data.version_number,
                        file_name
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
            created_version.files.push(VersionFile {
                hashes: vec![FileHash {
                    algorithm: "sha1".to_string(),
                    hash: upload_data.content_sha1,
                }],
                url: format!("{}/{}", cdn_url, upload_data.file_name),
            });
        }
    }

    let create_data = if let Some(create_data) = mod_create_data {
        create_data
    } else {
        return Err(CreateError::InvalidInput(String::from(
            "Multipart upload missing `data` field",
        )));
    };

    let body_url = format!("data/{}/body.md", mod_id);

    let upload_data = file_host
        .upload_file("text/plain", &body_url, create_data.mod_body.into_bytes())
        .await?;

    uploaded_files.push(UploadedFile {
        file_id: upload_data.file_id.clone(),
        file_name: upload_data.file_name.clone(),
    });

    // TODO: add team members to the database

    let team_id = generate_team_id(&mut *transaction).await?;

    let team = Team {
        id: team_id.0 as i64,
        members: create_data
            .team_members
            .into_iter()
            .map(|x| crate::database::models::TeamMember {
                user_id: x.user_id.0 as i64,
                name: x.name,
                role: x.role,
            })
            .collect(),
    };

    sqlx::query!(
        "
        INSERT INTO teams (id)
        VALUES ($1)
        ",
        team.id
    )
    .execute(&mut *transaction)
    .await?;

    // Insert the new mod into the database

    sqlx::query(
        "
        INSERT INTO mods (id, team_id, title, description, body_url, icon_url, issues_url, source_url, wiki_url)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        "
    )
    .bind(mod_id.0 as i64)
    .bind(team.id)
    .bind(create_data.mod_name)
    .bind(create_data.mod_description)
    .bind(format!("{}/{}", cdn_url, body_url))
    .bind(icon_url)
    .bind(create_data.issues_url)
    .bind(create_data.source_url)
    .bind(create_data.wiki_url)
    .execute(&mut *transaction).await?;

    // TODO: insert categories into the database

    // Insert each created version into the database
    for version in &created_versions {
        sqlx::query!(
            "
            INSERT INTO versions (id, mod_id, name, version_number, changelog_url, release_channel)
            VALUES ($1, $2, $3, $4, $5, $6)
            ",
            version.version_id as i64,
            version.mod_id as i64,
            version.name,
            version.version_number,
            version.changelog_url,
            0, // TODO: add default release channels, and match them here
        )
        .execute(&mut *transaction)
        .await?;

        // TODO: insert dependencies
        // TODO: insert game versions
        // TODO: insert loaders
        // TODO: insert version files and file hashes
    }

    Ok(HttpResponse::Ok().into())
}

async fn process_icon_upload(
    uploaded_files: &mut Vec<UploadedFile>,
    mod_id: ModId,
    file_name: &str,
    file_extension: &str,
    file_host: &dyn FileHost,
    mut field: actix_multipart::Field,
    cdn_url: &str,
) -> Result<String, CreateError> {
    if let Some(content_type) = get_image_content_type(file_extension) {
        let mut data = Vec::new();
        while let Some(chunk) = field.next().await {
            data.extend_from_slice(&chunk.map_err(CreateError::MultipartError)?);
        }

        let upload_data = file_host
            .upload_file(
                content_type,
                &format!("mods/icons/{}/{}", mod_id, file_name),
                data,
            )
            .await?;

        uploaded_files.push(UploadedFile {
            file_id: upload_data.file_id.clone(),
            file_name: upload_data.file_name.clone(),
        });

        Ok(format!("{}/{}", cdn_url, upload_data.file_name))
    } else {
        Err(CreateError::InvalidIconFormat(file_extension.to_string()))
    }
}

fn get_image_content_type(extension: &str) -> Option<&'static str> {
    let content_type = match &*extension {
        "bmp" => "image/bmp",
        "gif" => "image/gif",
        "jpeg" | "jpg" | "jpe" => "image/jpeg",
        "png" => "image/png",
        "svg" | "svgz" => "image/svg+xml",
        "webp" => "image/webp",
        "rgb" => "image/x-rgb",
        _ => "",
    };

    if content_type != "" {
        Some(content_type)
    } else {
        None
    }
}
