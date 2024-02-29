use crate::auth::{get_user_from_headers, AuthenticationError};
use crate::database::models::thread_item::ThreadBuilder;
use crate::database::models::{self, image_item, User};
use crate::database::redis::RedisPool;
use crate::file_hosting::{FileHost, FileHostingError};
use crate::models::error::ApiError;
use crate::models::ids::{ImageId, OrganizationId};
use crate::models::images::{Image, ImageContext};
use crate::models::pats::Scopes;
use crate::models::projects::{License, Link, MonetizationStatus, ProjectId, ProjectStatus};
use crate::models::teams::ProjectPermissions;
use crate::models::threads::ThreadType;
use crate::queue::session::AuthQueue;
use crate::search::indexing::IndexingError;
use crate::util::routes::read_from_field;
use crate::util::validate::validation_errors_to_string;
use actix_multipart::{Field, Multipart};
use actix_web::http::StatusCode;
use actix_web::web::{self, Data};
use actix_web::{HttpRequest, HttpResponse};
use chrono::Utc;
use futures::stream::StreamExt;
use image::ImageError;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPool;
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use validator::Validate;

pub fn config(cfg: &mut actix_web::web::ServiceConfig) {
    cfg.route("project", web::post().to(project_create));
}

#[derive(Error, Debug)]
pub enum CreateError {
    #[error("Environment Error")]
    EnvError(#[from] dotenvy::Error),
    #[error("An unknown database error occurred")]
    SqlxDatabaseError(#[from] sqlx::Error),
    #[error("Database Error: {0}")]
    DatabaseError(#[from] models::DatabaseError),
    #[error("Indexing Error: {0}")]
    IndexingError(#[from] IndexingError),
    #[error("Error while parsing multipart payload: {0}")]
    MultipartError(#[from] actix_multipart::MultipartError),
    #[error("Error while parsing JSON: {0}")]
    SerDeError(#[from] serde_json::Error),
    #[error("Error while validating input: {0}")]
    ValidationError(String),
    #[error("Error while uploading file: {0}")]
    FileHostingError(#[from] FileHostingError),
    #[error("Error while validating uploaded file: {0}")]
    FileValidationError(#[from] crate::validate::ValidationError),
    #[error("{}", .0)]
    MissingValueError(String),
    #[error("Invalid format for image: {0}")]
    InvalidIconFormat(String),
    #[error("Error with multipart data: {0}")]
    InvalidInput(String),
    #[error("Invalid game version: {0}")]
    InvalidGameVersion(String),
    #[error("Invalid loader: {0}")]
    InvalidLoader(String),
    #[error("Invalid category: {0}")]
    InvalidCategory(String),
    #[error("Invalid file type for version file: {0}")]
    InvalidFileType(String),
    #[error("Slug is already taken!")]
    SlugCollision,
    #[error("Authentication Error: {0}")]
    Unauthorized(#[from] AuthenticationError),
    #[error("Authentication Error: {0}")]
    CustomAuthenticationError(String),
    #[error("Image Parsing Error: {0}")]
    ImageError(#[from] ImageError),
    #[error("Reroute Error: {0}")]
    RerouteError(#[from] reqwest::Error),
}

impl actix_web::ResponseError for CreateError {
    fn status_code(&self) -> StatusCode {
        match self {
            CreateError::EnvError(..) => StatusCode::INTERNAL_SERVER_ERROR,
            CreateError::SqlxDatabaseError(..) => StatusCode::INTERNAL_SERVER_ERROR,
            CreateError::DatabaseError(..) => StatusCode::INTERNAL_SERVER_ERROR,
            CreateError::IndexingError(..) => StatusCode::INTERNAL_SERVER_ERROR,
            CreateError::FileHostingError(..) => StatusCode::INTERNAL_SERVER_ERROR,
            CreateError::SerDeError(..) => StatusCode::BAD_REQUEST,
            CreateError::MultipartError(..) => StatusCode::BAD_REQUEST,
            CreateError::MissingValueError(..) => StatusCode::BAD_REQUEST,
            CreateError::InvalidIconFormat(..) => StatusCode::BAD_REQUEST,
            CreateError::InvalidInput(..) => StatusCode::BAD_REQUEST,
            CreateError::InvalidGameVersion(..) => StatusCode::BAD_REQUEST,
            CreateError::InvalidLoader(..) => StatusCode::BAD_REQUEST,
            CreateError::InvalidCategory(..) => StatusCode::BAD_REQUEST,
            CreateError::InvalidFileType(..) => StatusCode::BAD_REQUEST,
            CreateError::Unauthorized(..) => StatusCode::UNAUTHORIZED,
            CreateError::CustomAuthenticationError(..) => StatusCode::UNAUTHORIZED,
            CreateError::SlugCollision => StatusCode::BAD_REQUEST,
            CreateError::ValidationError(..) => StatusCode::BAD_REQUEST,
            CreateError::FileValidationError(..) => StatusCode::BAD_REQUEST,
            CreateError::ImageError(..) => StatusCode::BAD_REQUEST,
            CreateError::RerouteError(..) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status_code()).json(ApiError {
            error: match self {
                CreateError::EnvError(..) => "environment_error",
                CreateError::SqlxDatabaseError(..) => "database_error",
                CreateError::DatabaseError(..) => "database_error",
                CreateError::IndexingError(..) => "indexing_error",
                CreateError::FileHostingError(..) => "file_hosting_error",
                CreateError::SerDeError(..) => "invalid_input",
                CreateError::MultipartError(..) => "invalid_input",
                CreateError::MissingValueError(..) => "invalid_input",
                CreateError::InvalidIconFormat(..) => "invalid_input",
                CreateError::InvalidInput(..) => "invalid_input",
                CreateError::InvalidGameVersion(..) => "invalid_input",
                CreateError::InvalidLoader(..) => "invalid_input",
                CreateError::InvalidCategory(..) => "invalid_input",
                CreateError::InvalidFileType(..) => "invalid_input",
                CreateError::Unauthorized(..) => "unauthorized",
                CreateError::CustomAuthenticationError(..) => "unauthorized",
                CreateError::SlugCollision => "invalid_input",
                CreateError::ValidationError(..) => "invalid_input",
                CreateError::FileValidationError(..) => "invalid_input",
                CreateError::ImageError(..) => "invalid_image",
                CreateError::RerouteError(..) => "reroute_error",
            },
            description: self.to_string(),
        })
    }
}

pub fn default_project_type() -> String {
    "mod".to_string()
}

fn default_requested_status() -> ProjectStatus {
    ProjectStatus::Approved
}

#[derive(Serialize, Deserialize, Validate, Clone)]
pub struct ProjectCreateData {
    #[validate(
        length(min = 3, max = 64),
        custom(function = "crate::util::validate::validate_name")
    )]
    #[serde(alias = "mod_name")]
    /// The title or name of the project.
    pub name: String,
    #[validate(
        length(min = 3, max = 64),
        regex = "crate::util::validate::RE_URL_SAFE"
    )]
    #[serde(alias = "mod_slug")]
    /// The slug of a project, used for vanity URLs
    pub slug: String,
    #[validate(length(min = 3, max = 255))]
    #[serde(alias = "mod_description")]
    /// A short description of the project.
    pub summary: String,
    #[validate(length(max = 65536))]
    #[serde(alias = "mod_body")]
    /// A long description of the project, in markdown.
    pub description: String,

    #[validate(length(max = 3))]
    /// A list of the categories that the project is in.
    pub categories: Vec<String>,
    #[validate(length(max = 256))]
    #[serde(default = "Vec::new")]
    /// A list of the categories that the project is in.
    pub additional_categories: Vec<String>,

    /// An optional link to the project's license page
    pub license_url: Option<String>,
    /// An optional list of all donation links the project has
    #[validate(custom(function = "crate::util::validate::validate_url_hashmap_values"))]
    #[serde(default)]
    pub link_urls: HashMap<String, String>,

    /// The license id that the project follows
    pub license_id: String,

    #[serde(default = "default_requested_status")]
    /// The status of the mod to be set once it is approved
    pub requested_status: ProjectStatus,

    // Associations to uploaded images in body/description
    #[validate(length(max = 10))]
    #[serde(default)]
    pub uploaded_images: Vec<ImageId>,

    /// The id of the organization to create the project in
    pub organization_id: Option<OrganizationId>,
}

pub struct UploadedFile {
    pub file_id: String,
    pub file_name: String,
}

pub async fn undo_uploads(
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

pub async fn project_create(
    req: HttpRequest,
    mut payload: Multipart,
    client: Data<PgPool>,
    redis: Data<RedisPool>,
    file_host: Data<Arc<dyn FileHost + Send + Sync>>,
    session_queue: Data<AuthQueue>,
) -> Result<HttpResponse, CreateError> {
    let mut transaction = client.begin().await?;
    let mut uploaded_files = Vec::new();

    let result = project_create_inner(
        req,
        &mut payload,
        &mut transaction,
        &***file_host,
        &mut uploaded_files,
        &client,
        &redis,
        &session_queue,
    )
    .await;

    if result.is_err() {
        let undo_result = undo_uploads(&***file_host, &uploaded_files).await;
        let rollback_result = transaction.rollback().await;

        undo_result?;
        if let Err(e) = rollback_result {
            return Err(e.into());
        }
    } else {
        transaction.commit().await?;
    }

    result
}
/*

Project Creation Steps:
Get logged in user
    Must match the author in the creation

1. Data
    - Gets "data" field from multipart form; must be first
    - Verification: string lengths
    - Create ProjectBuilder

2. Upload
    - Icon: check file format & size
        - Upload to backblaze & record URL

3. Creation
    - Database stuff
    - Add project data to indexing queue
*/

#[allow(clippy::too_many_arguments)]
async fn project_create_inner(
    req: HttpRequest,
    payload: &mut Multipart,
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    file_host: &dyn FileHost,
    uploaded_files: &mut Vec<UploadedFile>,
    pool: &PgPool,
    redis: &RedisPool,
    session_queue: &AuthQueue,
) -> Result<HttpResponse, CreateError> {
    // The base URL for files uploaded to backblaze
    let cdn_url = dotenvy::var("CDN_URL")?;

    // The currently logged in user
    let current_user = get_user_from_headers(
        &req,
        pool,
        redis,
        session_queue,
        Some(&[Scopes::PROJECT_CREATE]),
    )
    .await?
    .1;

    let project_id: ProjectId = models::generate_project_id(transaction).await?.into();

    let project_create_data: ProjectCreateData;
    {
        // The first multipart field must be named "data" and contain a
        // JSON `ProjectCreateData` object.

        let mut field = payload
            .next()
            .await
            .map(|m| m.map_err(CreateError::MultipartError))
            .unwrap_or_else(|| {
                Err(CreateError::MissingValueError(String::from(
                    "No `data` field in multipart upload",
                )))
            })?;

        let content_disposition = field.content_disposition();
        let name = content_disposition
            .get_name()
            .ok_or_else(|| CreateError::MissingValueError(String::from("Missing content name")))?;

        if name != "data" {
            return Err(CreateError::InvalidInput(String::from(
                "`data` field must come before file fields",
            )));
        }

        let mut data = Vec::new();
        while let Some(chunk) = field.next().await {
            data.extend_from_slice(&chunk.map_err(CreateError::MultipartError)?);
        }
        let create_data: ProjectCreateData = serde_json::from_slice(&data)?;

        create_data
            .validate()
            .map_err(|err| CreateError::InvalidInput(validation_errors_to_string(err, None)))?;

        let slug_project_id_option: Option<ProjectId> =
            serde_json::from_str(&format!("\"{}\"", create_data.slug)).ok();

        if let Some(slug_project_id) = slug_project_id_option {
            let slug_project_id: models::ids::ProjectId = slug_project_id.into();
            let results = sqlx::query!(
                "
                SELECT EXISTS(SELECT 1 FROM mods WHERE id=$1)
                ",
                slug_project_id as models::ids::ProjectId
            )
            .fetch_one(&mut **transaction)
            .await
            .map_err(|e| CreateError::DatabaseError(e.into()))?;

            if results.exists.unwrap_or(false) {
                return Err(CreateError::SlugCollision);
            }
        }

        {
            let results = sqlx::query!(
                "
                SELECT EXISTS(SELECT 1 FROM mods WHERE slug = LOWER($1))
                ",
                create_data.slug
            )
            .fetch_one(&mut **transaction)
            .await
            .map_err(|e| CreateError::DatabaseError(e.into()))?;

            if results.exists.unwrap_or(false) {
                return Err(CreateError::SlugCollision);
            }
        }

        project_create_data = create_data;
    }

    let mut icon_data = None;

    let mut error = None;
    while let Some(item) = payload.next().await {
        let field: Field = item?;

        if error.is_some() {
            continue;
        }

        let result = async {
            let content_disposition = field.content_disposition().clone();

            let name = content_disposition.get_name().ok_or_else(|| {
                CreateError::MissingValueError("Missing content name".to_string())
            })?;

            let (file_name, file_extension) =
                super::version_creation::get_name_ext(&content_disposition)?;

            if name == "icon" {
                if icon_data.is_some() {
                    return Err(CreateError::InvalidInput(String::from(
                        "Projects can only have one icon",
                    )));
                }
                // Upload the icon to the cdn
                icon_data = Some(
                    process_icon_upload(
                        uploaded_files,
                        project_id.0,
                        file_extension,
                        file_host,
                        field,
                        &cdn_url,
                    )
                    .await?,
                );
                return Ok(());
            }
            Err(CreateError::InvalidInput(format!(
                "File `{file_name}` (field {name}) isn't recognized. If it's a version, please use the version creation route after creating your project."
            )))
        }
        .await;

        if result.is_err() {
            error = result.err();
        }
    }

    if let Some(error) = error {
        return Err(error);
    }

    {
        // Convert the list of category names to actual categories
        let mut categories = Vec::with_capacity(project_create_data.categories.len());
        for category in &project_create_data.categories {
            let ids = models::categories::Category::get_ids(category, &mut **transaction).await?;
            if ids.is_empty() {
                return Err(CreateError::InvalidCategory(category.clone()));
            }

            // TODO: We should filter out categories that don't match the project type of any of the versions
            // ie: if mod and modpack both share a name this should only have modpack if it only has a modpack as a version
            categories.extend(ids.values());
        }

        let mut additional_categories =
            Vec::with_capacity(project_create_data.additional_categories.len());
        for category in &project_create_data.additional_categories {
            let ids = models::categories::Category::get_ids(category, &mut **transaction).await?;
            if ids.is_empty() {
                return Err(CreateError::InvalidCategory(category.clone()));
            }
            // TODO: We should filter out categories that don't match the project type of any of the versions
            // ie: if mod and modpack both share a name this should only have modpack if it only has a modpack as a version
            additional_categories.extend(ids.values());
        }

        let mut members = vec![];

        if project_create_data.organization_id.is_none() {
            members.push(models::team_item::TeamMemberBuilder {
                user_id: current_user.id.into(),
                role: crate::models::teams::DEFAULT_ROLE.to_owned(),
                is_owner: true,
                permissions: ProjectPermissions::all(),
                organization_permissions: None,
                accepted: true,
                payouts_split: Decimal::ONE_HUNDRED,
                ordering: 0,
            })
        }

        let team = models::team_item::TeamBuilder { members };

        let team_id = team.insert(&mut *transaction).await?;

        let license_id =
            spdx::Expression::parse(&project_create_data.license_id).map_err(|err| {
                CreateError::InvalidInput(format!("Invalid SPDX license identifier: {err}"))
            })?;

        let mut link_urls = vec![];

        let link_platforms =
            models::categories::LinkPlatform::list(&mut **transaction, redis).await?;
        for (platform, url) in &project_create_data.link_urls {
            let platform_id =
                models::categories::LinkPlatform::get_id(platform, &mut **transaction)
                    .await?
                    .ok_or_else(|| {
                        CreateError::InvalidInput(format!(
                            "Link platform {} does not exist.",
                            platform.clone()
                        ))
                    })?;
            let link_platform = link_platforms
                .iter()
                .find(|x| x.id == platform_id)
                .ok_or_else(|| {
                    CreateError::InvalidInput(format!(
                        "Link platform {} does not exist.",
                        platform.clone()
                    ))
                })?;
            link_urls.push(models::project_item::LinkUrl {
                platform_id,
                platform_name: link_platform.name.clone(),
                url: url.clone(),
                donation: link_platform.donation,
            })
        }

        let project_builder_actual = models::project_item::ProjectBuilder {
            project_id: project_id.into(),
            team_id,
            organization_id: project_create_data.organization_id.map(|x| x.into()),
            name: project_create_data.name,
            summary: project_create_data.summary,
            description: project_create_data.description,
            icon_url: icon_data.clone().map(|x| x.0),

            license_url: project_create_data.license_url,
            categories,
            additional_categories,
            status: ProjectStatus::Draft,
            requested_status: Some(project_create_data.requested_status),
            license: license_id.to_string(),
            slug: Some(project_create_data.slug),
            link_urls,
            color: icon_data.and_then(|x| x.1),
            monetization_status: MonetizationStatus::Monetized,
        };
        let project_builder = project_builder_actual.clone();

        let now = Utc::now();

        let id = project_builder_actual.insert(&mut *transaction).await?;
        User::clear_project_cache(&[current_user.id.into()], redis).await?;

        for image_id in project_create_data.uploaded_images {
            if let Some(db_image) =
                image_item::Image::get(image_id.into(), &mut **transaction, redis).await?
            {
                let image: Image = db_image.into();
                if !matches!(image.context, ImageContext::Project { .. })
                    || image.context.inner_id().is_some()
                {
                    return Err(CreateError::InvalidInput(format!(
                        "Image {} is not unused and in the 'project' context",
                        image_id
                    )));
                }

                sqlx::query!(
                    "
                    UPDATE uploaded_images
                    SET mod_id = $1
                    WHERE id = $2
                    ",
                    id as models::ids::ProjectId,
                    image_id.0 as i64
                )
                .execute(&mut **transaction)
                .await?;

                image_item::Image::clear_cache(image.id.into(), redis).await?;
            } else {
                return Err(CreateError::InvalidInput(format!(
                    "Image {} does not exist",
                    image_id
                )));
            }
        }

        let thread_id = ThreadBuilder {
            type_: ThreadType::Project,
            members: vec![],
            project_id: Some(id),
            report_id: None,
        }
        .insert(&mut *transaction)
        .await?;

        let response = crate::models::projects::Project {
            id: project_id,
            slug: project_builder.slug.clone(),
            project_types: vec![],
            games: vec![],
            team_id: team_id.into(),
            organization: project_create_data.organization_id,
            name: project_builder.name.clone(),
            summary: project_builder.summary.clone(),
            description: project_builder.description.clone(),
            published: now,
            updated: now,
            approved: None,
            queued: None,
            status: ProjectStatus::Draft,
            requested_status: project_builder.requested_status,
            moderator_message: None,
            license: License {
                id: project_create_data.license_id.clone(),
                name: "".to_string(),
                url: project_builder.license_url.clone(),
            },
            downloads: 0,
            followers: 0,
            categories: project_create_data.categories,
            additional_categories: project_create_data.additional_categories,
            loaders: vec![],
            versions: vec![],
            icon_url: project_builder.icon_url.clone(),
            link_urls: project_builder
                .link_urls
                .clone()
                .into_iter()
                .map(|x| (x.platform_name.clone(), Link::from(x)))
                .collect(),
            gallery: vec![], // Gallery items instantiate to empty
            color: project_builder.color,
            thread_id: thread_id.into(),
            monetization_status: MonetizationStatus::Monetized,
            fields: HashMap::new(), // Fields instantiate to empty
        };

        Ok(HttpResponse::Ok().json(response))
    }
}

async fn process_icon_upload(
    uploaded_files: &mut Vec<UploadedFile>,
    id: u64,
    file_extension: &str,
    file_host: &dyn FileHost,
    mut field: Field,
    cdn_url: &str,
) -> Result<(String, Option<u32>), CreateError> {
    if let Some(content_type) = crate::util::ext::get_image_content_type(file_extension) {
        let data = read_from_field(&mut field, 262144, "Icons must be smaller than 256KiB").await?;

        let color = crate::util::img::get_color_from_img(&data)?;

        let hash = sha1::Sha1::from(&data).hexdigest();
        let upload_data = file_host
            .upload_file(
                content_type,
                &format!("data/{id}/{hash}.{file_extension}"),
                data.freeze(),
            )
            .await?;

        uploaded_files.push(UploadedFile {
            file_id: upload_data.file_id,
            file_name: upload_data.file_name.clone(),
        });

        Ok((format!("{}/{}", cdn_url, upload_data.file_name), color))
    } else {
        Err(CreateError::InvalidIconFormat(file_extension.to_string()))
    }
}
