use crate::database::models::{FileHash, Mod, Team, Version, VersionFile};
use crate::file_hosting::{upload_file, FileHostingError, UploadUrlData};
use crate::models::error::ApiError;
use crate::models::ids::random_base62;
use crate::models::mods::{GameVersion, ModId, VersionId, VersionType};
use crate::models::teams::TeamMember;
use actix_multipart::{Field, Multipart};
use actix_web::http::StatusCode;
use actix_web::web::Data;
use actix_web::{post, HttpResponse};
use bson::doc;
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
            },
            description: &self.to_string(),
        })
    }
}

#[derive(Serialize, Deserialize, Clone)]
struct InitialVersionData {
    pub file_indexes: Vec<i32>,
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

#[post("api/v1/mod")]
pub async fn mod_create(
    mut payload: Multipart,
    client: Data<PgPool>,
    upload_url: Data<UploadUrlData>,
) -> Result<HttpResponse, CreateError> {
    //TODO Switch to transactions for safer database and file upload calls (once it is implemented in the APIs)
    let cdn_url = dotenv::var("CDN_URL")?;

    // let db = client.database("modrinth");

    // let mods = db.collection("mods");
    // let versions = db.collection("versions");

    let mut mod_id = ModId(random_base62(8));
    let mut retry_count: i32 = 0;

    // Check if ID is unique
    loop {
        let results = sqlx::query!(
            "
            SELECT EXISTS(SELECT 1 FROM mods WHERE id=$1)
            ",
            mod_id.0 as i64
        )
        .fetch_one(client.as_ref())
        .await
        .expect("TODO: error handling");

        if results.exists.expect("TODO: error handling") {
            mod_id = ModId(random_base62(8));
        } else {
            break;
        }

        retry_count += 1;
        if retry_count > 20 {
            return Err(CreateError::RandomIdError);
        }
    }

    let mut created_versions: Vec<Version> = vec![];

    let mut mod_create_data: Option<ModCreateData> = None;
    let mut icon_url = "".to_string();

    let mut current_file_index = 0;
    while let Some(item) = payload.next().await {
        let mut field: Field = item.map_err(CreateError::MultipartError)?;
        let content_disposition = field.content_disposition().ok_or_else(|| {
            CreateError::MissingValueError("Missing content disposition!".to_string())
        })?;
        let name = content_disposition
            .get_name()
            .ok_or_else(|| CreateError::MissingValueError("Missing content name!".to_string()))?;

        while let Some(chunk) = field.next().await {
            let data = &chunk.map_err(CreateError::MultipartError)?;

            if name == "data" {
                mod_create_data = Some(serde_json::from_slice(&data)?);
            } else {
                let file_name = content_disposition.get_filename().ok_or_else(|| {
                    CreateError::MissingValueError("Missing content file name".to_string())
                })?;
                let file_extension = if let Some(last_period) = file_name.rfind('.') {
                    file_name.get(last_period + 1..).unwrap_or("")
                } else {
                    return Err(CreateError::MissingValueError(
                        "Missing content file extension".to_string(),
                    ));
                };

                if let Some(create_data) = &mod_create_data {
                    if name == "icon" {
                        if let Some(ext) = get_image_content_type(file_extension) {
                            let upload_data = upload_file(
                                upload_url.get_ref(),
                                ext,
                                &format!("mods/icons/{}/{}", mod_id, file_name),
                                data.to_vec(),
                            )
                            .await?;

                            icon_url = format!("{}/{}", cdn_url, upload_data.file_name);
                        } else {
                            return Err(CreateError::InvalidIconFormat(file_extension.to_string()));
                        }
                    } else if &*file_extension == "jar" {
                        let initial_version_data = create_data
                            .initial_versions
                            .iter()
                            .position(|x| x.file_indexes.contains(&current_file_index));

                        if let Some(version_data_index) = initial_version_data {
                            let version_data = create_data
                                .initial_versions
                                .get(version_data_index)
                                .ok_or_else(|| {
                                    CreateError::MissingValueError(
                                        "Missing file extension!".to_string(),
                                    )
                                })?
                                .clone();

                            let mut created_version_filter = created_versions
                                .iter_mut()
                                .filter(|x| x.version_number == version_data.version_number);

                            match created_version_filter.next() {
                                Some(created_version) => {
                                    let upload_data = upload_file(
                                        upload_url.get_ref(),
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

                                    created_version.files.push(VersionFile {
                                        hashes: vec![FileHash {
                                            algorithm: "sha1".to_string(),
                                            hash: upload_data.content_sha1,
                                        }],
                                        url: format!("{}/{}", cdn_url, upload_data.file_name),
                                    });
                                }
                                None => {
                                    //Check if ID is unique
                                    let mut version_id = VersionId(random_base62(8));
                                    retry_count = 0;

                                    loop {
                                        let results = sqlx::query!(
                                            "
                                            SELECT EXISTS(SELECT 1 FROM mods WHERE id=$1)
                                            ",
                                            version_id.0 as i64
                                        )
                                        .fetch_one(client.as_ref())
                                        .await
                                        .expect("TODO: error handling");

                                        if results.exists.expect("TODO: error handling") {
                                            version_id = VersionId(random_base62(8));
                                        } else {
                                            break;
                                        }

                                        retry_count += 1;
                                        if retry_count > 20 {
                                            return Err(CreateError::RandomIdError);
                                        }
                                    }

                                    let body_url = format!(
                                        "data/{}/changelogs/{}/body.md",
                                        mod_id, version_id
                                    );

                                    upload_file(
                                        upload_url.get_ref(),
                                        "text/plain",
                                        &body_url,
                                        version_data.version_body.into_bytes(),
                                    )
                                    .await?;

                                    let upload_data = upload_file(
                                        upload_url.get_ref(),
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

                                    let version = Version {
                                        version_id: version_id.0 as i64,
                                        mod_id: mod_id.0 as i64,
                                        name: version_data.version_title,
                                        version_number: version_data.version_number.clone(),
                                        changelog_url: Some(format!("{}/{}", cdn_url, body_url)),
                                        date_published: Utc::now().to_rfc2822(),
                                        downloads: 0,
                                        version_type: version_data.version_type.to_string(),
                                        files: vec![VersionFile {
                                            hashes: vec![FileHash {
                                                algorithm: "sha1".to_string(),
                                                hash: upload_data.content_sha1,
                                            }],
                                            url: format!("{}/{}", cdn_url, upload_data.file_name),
                                        }],
                                        dependencies: version_data
                                            .dependencies
                                            .into_iter()
                                            .map(|x| x.0 as i64)
                                            .collect::<Vec<_>>(),
                                        game_versions: vec![],
                                        loaders: vec![],
                                    };
                                    //TODO: Malware scan + file validation

                                    created_versions.push(version);
                                }
                            }
                        }
                    }
                }
            }
        }

        current_file_index += 1;
    }

    for version in &created_versions {
        // TODO: race condition with mod / version ids
        // let result = sqlx::query!(
        //     "
        //     INSERT INTO versions (id, mod_id, name, version_number, changelog_url, release_channel)
        //     VALUES ($1, $2, $3, $4, $5, $6)
        //     ",
        //     version.id.0 as i64,
        //     version.mod_id.0 as i64,
        //     version.name,
        //     version.version_number,
        //     version.changelog_url,
        //     0, // TODO
        // ).execute(client.as_ref());
        sqlx::query(
            "
            INSERT INTO versions (id, mod_id, name, version_number, changelog_url, release_channel)
            VALUES ($1, $2, $3, $4, $5, $6)
            ",
        )
        .bind(version.version_id)
        .bind(version.mod_id)
        .bind(&version.name)
        .bind(&version.version_number)
        .bind(&version.changelog_url)
        .bind(0) // TODO: release channel
        .execute(client.as_ref())
        .await?;
    }

    if let Some(create_data) = mod_create_data {
        let body_url = format!("data/{}/body.md", mod_id);

        upload_file(
            upload_url.get_ref(),
            "text/plain",
            &body_url,
            create_data.mod_body.into_bytes(),
        )
        .await?;

        let created_mod: Mod = Mod {
            id: mod_id.0 as i64,
            team: Team {
                id: random_base62(8) as i64,
                members: create_data
                    .team_members
                    .into_iter()
                    .map(|x| crate::database::models::TeamMember {
                        user_id: x.user_id.0 as i64,
                        name: x.name,
                        role: x.role,
                    })
                    .collect(),
            },
            title: create_data.mod_name,
            icon_url: Some(icon_url),
            description: create_data.mod_description,
            body_url: format!("{}/{}", cdn_url, body_url),
            published: Utc::now().to_rfc2822(),
            downloads: 0,
            categories: create_data.categories,
            version_ids: created_versions
                .into_iter()
                .map(|x| x.version_id as i32)
                .collect::<Vec<_>>(),
            issues_url: create_data.issues_url,
            source_url: create_data.source_url,
            wiki_url: create_data.wiki_url,
        };

        sqlx::query!(
            "
            INSERT INTO teams (id)
            VALUES ($1)
            ",
            created_mod.team.id
        )
        .execute(client.as_ref())
        .await?;
        // TODO: add team members

        sqlx::query(
            "
            INSERT INTO mods (id, team_id, title, description, body_url, icon_url, issues_url, source_url, wiki_url)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "
        )
        .bind(created_mod.id)
        .bind(created_mod.team.id)
        .bind(created_mod.title)
        .bind(created_mod.description)
        .bind(created_mod.body_url)
        .bind(created_mod.icon_url)
        .bind(created_mod.issues_url)
        .bind(created_mod.source_url)
        .bind(created_mod.wiki_url)
        .execute(client.as_ref()).await?;
    }

    Ok(HttpResponse::Ok().into())
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
