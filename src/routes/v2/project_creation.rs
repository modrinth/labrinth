use crate::database::models::version_item;
use crate::database::redis::RedisPool;
use crate::file_hosting::FileHost;
use crate::models;
use crate::models::ids::base62_impl::parse_base62;
use crate::models::ids::ImageId;
use crate::models::projects::{DonationLink, Project, ProjectStatus, SideType};
use crate::models::v2::projects::LegacyProject;
use crate::queue::session::AuthQueue;
use crate::routes::v3::project_creation::default_project_type;
use crate::routes::v3::project_creation::{CreateError, NewGalleryItem};
use crate::routes::{v2_reroute, v3};
use actix_multipart::Multipart;
use actix_web::web::Data;
use actix_web::{post, HttpRequest, HttpResponse};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::postgres::PgPool;

use std::collections::HashMap;
use std::sync::Arc;
use validator::Validate;

use super::version_creation::InitialVersionData;

pub fn config(cfg: &mut actix_web::web::ServiceConfig) {
    cfg.service(project_create);
}

pub fn default_requested_status() -> ProjectStatus {
    ProjectStatus::Approved
}

#[derive(Serialize, Deserialize, Validate, Clone)]
struct ProjectCreateData {
    #[validate(
        length(min = 3, max = 64),
        custom(function = "crate::util::validate::validate_name")
    )]
    #[serde(alias = "mod_name")]
    /// The title or name of the project.
    pub title: String,
    #[validate(length(min = 1, max = 64))]
    #[serde(default = "default_project_type")]
    /// The project type of this mod
    pub project_type: String,
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
    pub description: String,
    #[validate(length(max = 65536))]
    #[serde(alias = "mod_body")]
    /// A long description of the project, in markdown.
    pub body: String,

    /// The support range for the client project
    pub client_side: SideType,
    /// The support range for the server project
    pub server_side: SideType,

    #[validate(length(max = 32))]
    #[validate]
    /// A list of initial versions to upload with the created project
    pub initial_versions: Vec<InitialVersionData>,
    #[validate(length(max = 3))]
    /// A list of the categories that the project is in.
    pub categories: Vec<String>,
    #[validate(length(max = 256))]
    #[serde(default = "Vec::new")]
    /// A list of the categories that the project is in.
    pub additional_categories: Vec<String>,

    #[validate(
        custom(function = "crate::util::validate::validate_url"),
        length(max = 2048)
    )]
    /// An optional link to where to submit bugs or issues with the project.
    pub issues_url: Option<String>,
    #[validate(
        custom(function = "crate::util::validate::validate_url"),
        length(max = 2048)
    )]
    /// An optional link to the source code for the project.
    pub source_url: Option<String>,
    #[validate(
        custom(function = "crate::util::validate::validate_url"),
        length(max = 2048)
    )]
    /// An optional link to the project's wiki page or other relevant information.
    pub wiki_url: Option<String>,
    #[validate(
        custom(function = "crate::util::validate::validate_url"),
        length(max = 2048)
    )]
    /// An optional link to the project's license page
    pub license_url: Option<String>,
    #[validate(
        custom(function = "crate::util::validate::validate_url"),
        length(max = 2048)
    )]
    /// An optional link to the project's discord.
    pub discord_url: Option<String>,
    /// An optional list of all donation links the project has\
    #[validate]
    pub donation_urls: Option<Vec<DonationLink>>,

    /// An optional boolean. If true, the project will be created as a draft.
    pub is_draft: Option<bool>,

    /// The license id that the project follows
    pub license_id: String,

    #[validate(length(max = 64))]
    #[validate]
    /// The multipart names of the gallery items to upload
    pub gallery_items: Option<Vec<NewGalleryItem>>,
    #[serde(default = "default_requested_status")]
    /// The status of the mod to be set once it is approved
    pub requested_status: ProjectStatus,

    // Associations to uploaded images in body/description
    #[validate(length(max = 10))]
    #[serde(default)]
    pub uploaded_images: Vec<ImageId>,

    /// The id of the organization to create the project in
    pub organization_id: Option<models::ids::OrganizationId>,
}

#[post("project")]
pub async fn project_create(
    req: HttpRequest,
    payload: Multipart,
    client: Data<PgPool>,
    redis: Data<RedisPool>,
    file_host: Data<Arc<dyn FileHost + Send + Sync>>,
    session_queue: Data<AuthQueue>,
) -> Result<HttpResponse, CreateError> {
    // Convert V2 multipart payload to V3 multipart payload
    let payload = v2_reroute::alter_actix_multipart(
        payload,
        req.headers().clone(),
        |legacy_create: ProjectCreateData| {
            // Set game name (all v2 projects are minecraft-java)
            let game_name = "minecraft-java".to_string();

            // Side types will be applied to each version
            let client_side = legacy_create.client_side;
            let server_side = legacy_create.server_side;

            let initial_versions = legacy_create.initial_versions.into_iter().map(|v| {
                let mut fields = HashMap::new();
                fields.insert("client_side".to_string(), json!(client_side));
                fields.insert("server_side".to_string(), json!(server_side));
                fields.insert("game_versions".to_string(), json!(v.game_versions));

                v3::version_creation::InitialVersionData {
                    project_id: v.project_id,
                    file_parts: v.file_parts,
                    version_number: v.version_number,
                    version_title: v.version_title,
                    version_body: v.version_body,
                    dependencies: v.dependencies,
                    release_channel: v.release_channel,
                    loaders: v.loaders,
                    featured: v.featured,
                    primary_file: v.primary_file,
                    status: v.status,
                    file_types: v.file_types,
                    uploaded_images: v.uploaded_images,
                    fields,
                }
            });
            Ok(v3::project_creation::ProjectCreateData {
                title: legacy_create.title,
                project_type: legacy_create.project_type,
                slug: legacy_create.slug,
                description: legacy_create.description,
                body: legacy_create.body,
                game_name,
                initial_versions: initial_versions.collect(),
                categories: legacy_create.categories,
                additional_categories: legacy_create.additional_categories,
                issues_url: legacy_create.issues_url,
                source_url: legacy_create.source_url,
                wiki_url: legacy_create.wiki_url,
                license_url: legacy_create.license_url,
                discord_url: legacy_create.discord_url,
                donation_urls: legacy_create.donation_urls,
                is_draft: legacy_create.is_draft,
                license_id: legacy_create.license_id,
                gallery_items: legacy_create.gallery_items,
                requested_status: legacy_create.requested_status,
                uploaded_images: legacy_create.uploaded_images,
                organization_id: legacy_create.organization_id,
            })
        },
    )
    .await?;

    // Call V3 project creation
    let response = v3::project_creation::project_create(
        req,
        payload,
        client.clone(),
        redis.clone(),
        file_host,
        session_queue,
        Some(&[Scopes::PROJECT_CREATE]),
    )
    .await?
    .1;

    let project_id: ProjectId = models::generate_project_id(transaction).await?.into();

    let project_create_data;
    let mut versions;
    let mut versions_map = std::collections::HashMap::new();
    let mut gallery_urls = Vec::new();

    let all_game_versions =
        models::categories::GameVersion::list(&mut **transaction, redis).await?;
    let all_loaders = models::categories::Loader::list(&mut **transaction, redis).await?;

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

        let slug_project_id_option: Option<u64> = parse_base62(&create_data.slug).ok();

        if let Some(slug_project_id) = slug_project_id_option {
            let results = sqlx::query!(
                "
                SELECT EXISTS(SELECT 1 FROM mods WHERE id=$1)
                ",
                slug_project_id as i64
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

        // Create VersionBuilders for the versions specified in `initial_versions`
        versions = Vec::with_capacity(create_data.initial_versions.len());
        for (i, data) in create_data.initial_versions.iter().enumerate() {
            // Create a map of multipart field names to version indices
            for name in &data.file_parts {
                if versions_map.insert(name.to_owned(), i).is_some() {
                    // If the name is already used
                    return Err(CreateError::InvalidInput(String::from(
                        "Duplicate multipart field name",
                    )));
                }
            }
            versions.push(
                create_initial_version(
                    data,
                    project_id,
                    current_user.id,
                    &all_game_versions,
                    &all_loaders,
                    &create_data.project_type,
                    transaction,
                )
                .await?,
            );
        }
        project_create_data = create_data;
    }

    let project_type_id = models::categories::ProjectType::get_id(
        project_create_data.project_type.as_str(),
        &mut **transaction,
    )
    .await?
    .ok_or_else(|| {
        CreateError::InvalidInput(format!(
            "Project Type {} does not exist.",
            project_create_data.project_type.clone()
        ))
    })?;

    let mut icon_data = None;

    let mut error = None;
    while let Some(item) = payload.next().await {
        let mut field: Field = item?;

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

            if let Some(gallery_items) = &project_create_data.gallery_items {
                if gallery_items.iter().filter(|a| a.featured).count() > 1 {
                    return Err(CreateError::InvalidInput(String::from(
                        "Only one gallery image can be featured.",
                    )));
                }

                if let Some(item) = gallery_items.iter().find(|x| x.item == name) {
                    let data = read_from_field(
                        &mut field,
                        5 * (1 << 20),
                        "Gallery image exceeds the maximum of 5MiB.",
                    )
                    .await?;

                    let hash = sha1::Sha1::from(&data).hexdigest();
                    let (_, file_extension) =
                        super::version_creation::get_name_ext(&content_disposition)?;
                    let content_type = crate::util::ext::get_image_content_type(file_extension)
                        .ok_or_else(|| {
                            CreateError::InvalidIconFormat(file_extension.to_string())
                        })?;

                    let url = format!("data/{project_id}/images/{hash}.{file_extension}");
                    let upload_data = file_host
                        .upload_file(content_type, &url, data.freeze())
                        .await?;

                    uploaded_files.push(UploadedFile {
                        file_id: upload_data.file_id,
                        file_name: upload_data.file_name,
                    });

                    gallery_urls.push(crate::models::projects::GalleryItem {
                        url: format!("{cdn_url}/{url}"),
                        featured: item.featured,
                        title: item.title.clone(),
                        description: item.description.clone(),
                        created: Utc::now(),
                        ordering: item.ordering,
                    });

                    return Ok(());
                }
            }

            let index = if let Some(i) = versions_map.get(name) {
                *i
            } else {
                return Err(CreateError::InvalidInput(format!(
                    "File `{file_name}` (field {name}) isn't specified in the versions data"
                )));
            };

            // `index` is always valid for these lists
            let created_version = versions.get_mut(index).unwrap();
            let version_data = project_create_data.initial_versions.get(index).unwrap();

            // Upload the new jar file
            super::version_creation::upload_file(
                &mut field,
                file_host,
                version_data.file_parts.len(),
                uploaded_files,
                &mut created_version.files,
                &mut created_version.dependencies,
                &cdn_url,
                &content_disposition,
                project_id,
                created_version.version_id.into(),
                &project_create_data.project_type,
                version_data.loaders.clone(),
                version_data.game_versions.clone(),
                all_game_versions.clone(),
                version_data.primary_file.is_some(),
                version_data.primary_file.as_deref() == Some(name),
                None,
                transaction,
            )
            .await?;

            Ok(())
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
        // Check to make sure that all specified files were uploaded
        for (version_data, builder) in project_create_data
            .initial_versions
            .iter()
            .zip(versions.iter())
        {
            if version_data.file_parts.len() != builder.files.len() {
                return Err(CreateError::InvalidInput(String::from(
                    "Some files were specified in initial_versions but not uploaded",
                )));
            }
        }

        // Convert the list of category names to actual categories
        let mut categories = Vec::with_capacity(project_create_data.categories.len());
        for category in &project_create_data.categories {
            let id = models::categories::Category::get_id_project(
                category,
                project_type_id,
                &mut **transaction,
            )
            .await?
            .ok_or_else(|| CreateError::InvalidCategory(category.clone()))?;
            categories.push(id);
        }

        let mut additional_categories =
            Vec::with_capacity(project_create_data.additional_categories.len());
        for category in &project_create_data.additional_categories {
            let id = models::categories::Category::get_id_project(
                category,
                project_type_id,
                &mut **transaction,
            )
            .await?
            .ok_or_else(|| CreateError::InvalidCategory(category.clone()))?;
            additional_categories.push(id);
        }

        let team = models::team_item::TeamBuilder {
            members: vec![models::team_item::TeamMemberBuilder {
                user_id: current_user.id.into(),
                role: crate::models::teams::OWNER_ROLE.to_owned(),
                // Allow all permissions for project creator, even if attached to a project
                permissions: ProjectPermissions::all(),
                organization_permissions: None,
                accepted: true,
                payouts_split: Decimal::ONE_HUNDRED,
                ordering: 0,
            }],
        };

        let team_id = team.insert(transaction).await?;

        let status;
        if project_create_data.is_draft.unwrap_or(false) {
            status = ProjectStatus::Draft;
        } else {
            status = ProjectStatus::Processing;

            if project_create_data.initial_versions.is_empty() {
                return Err(CreateError::InvalidInput(String::from(
                    "Project submitted for review with no initial versions",
                )));
            }
        }

        if !project_create_data.requested_status.can_be_requested() {
            return Err(CreateError::InvalidInput(String::from(
                "Specified requested status is not allowed to be requested",
            )));
        }

        let client_side_id = models::categories::SideType::get_id(
            project_create_data.client_side.as_str(),
            &mut **transaction,
        )
        .await?
        .ok_or_else(|| {
            CreateError::InvalidInput("Client side type specified does not exist.".to_string())
        })?;

        let server_side_id = models::categories::SideType::get_id(
            project_create_data.server_side.as_str(),
            &mut **transaction,
        )
        .await?
        .ok_or_else(|| {
            CreateError::InvalidInput("Server side type specified does not exist.".to_string())
        })?;

        let license_id =
            spdx::Expression::parse(&project_create_data.license_id).map_err(|err| {
                CreateError::InvalidInput(format!("Invalid SPDX license identifier: {err}"))
            })?;

        let mut donation_urls = vec![];

        if let Some(urls) = &project_create_data.donation_urls {
            for url in urls {
                let platform_id =
                    models::categories::DonationPlatform::get_id(&url.id, &mut **transaction)
                        .await?
                        .ok_or_else(|| {
                            CreateError::InvalidInput(format!(
                                "Donation platform {} does not exist.",
                                url.id.clone()
                            ))
                        })?;

                donation_urls.push(models::project_item::DonationUrl {
                    platform_id,
                    platform_short: "".to_string(),
                    platform_name: "".to_string(),
                    url: url.url.clone(),
                })
            }
        }

        let project_builder_actual = models::project_item::ProjectBuilder {
            project_id: project_id.into(),
            project_type_id,
            team_id,
            organization_id: project_create_data.organization_id,
            title: project_create_data.title,
            description: project_create_data.description,
            body: project_create_data.body,
            icon_url: icon_data.clone().map(|x| x.0),
            issues_url: project_create_data.issues_url,
            source_url: project_create_data.source_url,
            wiki_url: project_create_data.wiki_url,

            license_url: project_create_data.license_url,
            discord_url: project_create_data.discord_url,
            categories,
            additional_categories,
            initial_versions: versions,
            status,
            requested_status: Some(project_create_data.requested_status),
            client_side: client_side_id,
            server_side: server_side_id,
            license: license_id.to_string(),
            slug: Some(project_create_data.slug),
            donation_urls,
            gallery_items: gallery_urls
                .iter()
                .map(|x| models::project_item::GalleryItem {
                    image_url: x.url.clone(),
                    featured: x.featured,
                    title: x.title.clone(),
                    description: x.description.clone(),
                    created: x.created,
                    ordering: x.ordering,
                })
                .collect(),
            color: icon_data.and_then(|x| x.1),
            monetization_status: MonetizationStatus::Monetized,
        };
        let project_builder = project_builder_actual.clone();

        let now = Utc::now();

        let id = project_builder_actual.insert(transaction).await?;
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
        .insert(transaction)
        .await?;

        let response = crate::models::projects::Project {
            id: project_id,
            slug: project_builder.slug.clone(),
            project_type: project_create_data.project_type.clone(),
            team: team_id.into(),
            organization: project_create_data.organization_id.map(|x| x.into()),
            title: project_builder.title.clone(),
            description: project_builder.description.clone(),
            body: project_builder.body.clone(),
            body_url: None,
            published: now,
            updated: now,
            approved: None,
            queued: None,
            status,
            requested_status: project_builder.requested_status,
            moderator_message: None,
            license: License {
                id: project_create_data.license_id.clone(),
                name: "".to_string(),
                url: project_builder.license_url.clone(),
            },
            client_side: project_create_data.client_side,
            server_side: project_create_data.server_side,
            downloads: 0,
            followers: 0,
            categories: project_create_data.categories,
            additional_categories: project_create_data.additional_categories,
            game_versions: vec![],
            loaders: vec![],
            versions: project_builder
                .initial_versions
                .iter()
                .map(|v| v.version_id.into())
                .collect::<Vec<_>>(),
            icon_url: project_builder.icon_url.clone(),
            issues_url: project_builder.issues_url.clone(),
            source_url: project_builder.source_url.clone(),
            wiki_url: project_builder.wiki_url.clone(),
            discord_url: project_builder.discord_url.clone(),
            donation_urls: project_create_data.donation_urls.clone(),
            gallery: gallery_urls,
            color: project_builder.color,
            thread_id: thread_id.into(),
            monetization_status: MonetizationStatus::Monetized,
        };

        Ok(HttpResponse::Ok().json(response))
    }
}

async fn create_initial_version(
    version_data: &InitialVersionData,
    project_id: ProjectId,
    author: UserId,
    all_game_versions: &[models::categories::GameVersion],
    all_loaders: &[models::categories::Loader],
    project_type: &str,
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<models::version_item::VersionBuilder, CreateError> {
    if version_data.project_id.is_some() {
        return Err(CreateError::InvalidInput(String::from(
            "Found project id in initial version for new project",
        )));
    }

    version_data
        .validate()
        .map_err(|err| CreateError::ValidationError(validation_errors_to_string(err, None)))?;

    // Randomly generate a new id to be used for the version
    let version_id: VersionId = models::generate_version_id(transaction).await?.into();

    let game_versions = version_data
        .game_versions
        .iter()
        .map(|x| {
            all_game_versions
                .iter()
                .find(|y| y.version == x.0)
                .ok_or_else(|| CreateError::InvalidGameVersion(x.0.clone()))
                .map(|y| y.id)
        })
        .collect::<Result<Vec<models::GameVersionId>, CreateError>>()?;

    let loaders = version_data
        .loaders
        .iter()
        .map(|x| {
            all_loaders
                .iter()
                .find(|y| {
                    y.loader == x.0
                        && y.supported_project_types
                            .contains(&project_type.to_string())
                })
                .ok_or_else(|| CreateError::InvalidLoader(x.0.clone()))
                .map(|y| y.id)
        })
        .collect::<Result<Vec<models::LoaderId>, CreateError>>()?;

    let dependencies = version_data
        .dependencies
        .iter()
        .map(|d| models::version_item::DependencyBuilder {
            version_id: d.version_id.map(|x| x.into()),
            project_id: d.project_id.map(|x| x.into()),
            dependency_type: d.dependency_type.to_string(),
            file_name: None,
        })
        .collect::<Vec<_>>();

    let version = models::version_item::VersionBuilder {
        version_id: version_id.into(),
        project_id: project_id.into(),
        author_id: author.into(),
        name: version_data.version_title.clone(),
        version_number: version_data.version_number.clone(),
        changelog: version_data.version_body.clone().unwrap_or_default(),
        files: Vec::new(),
        dependencies,
        game_versions,
        loaders,
        featured: version_data.featured,
        status: VersionStatus::Listed,
        version_type: version_data.release_channel.to_string(),
        requested_status: None,
    };

    Ok(version)
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
