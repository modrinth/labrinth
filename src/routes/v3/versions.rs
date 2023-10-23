use std::collections::HashMap;

use super::ApiError;
use crate::auth::{filter_authorized_versions, get_user_from_headers, is_authorized};
use crate::database;
use crate::database::models::loader_fields::{LoaderField, LoaderFieldEnumValue, VersionField};
use crate::database::models::version_item::{DependencyBuilder, LoaderVersion};
use crate::database::models::Organization;
use crate::database::redis::RedisPool;
use crate::models;
use crate::models::ids::VersionId;
use crate::models::images::ImageContext;
use crate::models::pats::Scopes;
use crate::models::projects::{Dependency, FileType, LoaderStruct, VersionStatus, VersionType};
use crate::models::teams::ProjectPermissions;
use crate::queue::session::AuthQueue;
use crate::util::img;
use crate::util::validate::validation_errors_to_string;
use actix_web::{web, HttpRequest, HttpResponse};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use validator::Validate;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route(
        "version",
        web::post().to(super::version_creation::version_create),
    );
    cfg.route(
        "{id}",
        web::post().to(super::version_creation::version_create),
    );

    cfg.service(
        web::scope("version")
            .route("{id}", web::patch().to(version_edit))
            .route(
                "{version_id}/file",
                web::post().to(super::version_creation::upload_file_to_version),
            ),
    );
}
#[derive(Serialize, Deserialize, Validate, Default, Debug)]
pub struct EditVersion {
    #[validate(
        length(min = 1, max = 64),
        custom(function = "crate::util::validate::validate_name")
    )]
    pub name: Option<String>,
    #[validate(
        length(min = 1, max = 32),
        regex = "crate::util::validate::RE_URL_SAFE"
    )]
    pub version_number: Option<String>,
    #[validate(length(max = 65536))]
    pub changelog: Option<String>,
    pub version_type: Option<models::projects::VersionType>,
    #[validate(
        length(min = 0, max = 4096),
        custom(function = "crate::util::validate::validate_deps")
    )]
    pub dependencies: Option<Vec<Dependency>>,
    pub loaders: Option<Vec<LoaderStruct>>,
    pub featured: Option<bool>,
    pub primary_file: Option<(String, String)>,
    pub downloads: Option<u32>,
    pub status: Option<VersionStatus>,
    pub file_types: Option<Vec<EditVersionFileType>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct EditVersionFileType {
    pub algorithm: String,
    pub hash: String,
    pub file_type: Option<FileType>,
}

// TODO: Avoid this 'helper' pattern here and similar fnunctoins- a macro might be the best bet here to ensure it's callable from both v2 and v3
// (web::Path can't be recreated naturally)
pub async fn version_edit(
    req: HttpRequest,
    info: web::Path<(VersionId,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    new_version: web::Json<serde_json::Value>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let new_version: EditVersion = serde_json::from_value(new_version.into_inner())?;
    version_edit_helper(
        req,
        info.into_inner(),
        pool,
        redis,
        new_version,
        session_queue,
    )
    .await
}
pub async fn version_edit_helper(
    req: HttpRequest,
    info: (VersionId,),
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    new_version: EditVersion,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::VERSION_WRITE]),
    )
    .await?
    .1;

    new_version
        .validate()
        .map_err(|err| ApiError::Validation(validation_errors_to_string(err, None)))?;

    let version_id = info.0;
    let id = version_id.into();

    let result = database::models::Version::get(id, &**pool, &redis).await?;

    if let Some(version_item) = result {
        let project_item =
            database::models::Project::get_id(version_item.inner.project_id, &**pool, &redis)
                .await?;

        let team_member = database::models::TeamMember::get_from_user_id_project(
            version_item.inner.project_id,
            user.id.into(),
            &**pool,
        )
        .await?;

        let organization = Organization::get_associated_organization_project_id(
            version_item.inner.project_id,
            &**pool,
        )
        .await?;

        let organization_team_member = if let Some(organization) = &organization {
            database::models::TeamMember::get_from_user_id(
                organization.team_id,
                user.id.into(),
                &**pool,
            )
            .await?
        } else {
            None
        };

        let permissions = ProjectPermissions::get_permissions_by_role(
            &user.role,
            &team_member,
            &organization_team_member,
        );

        if let Some(perms) = permissions {
            if !perms.contains(ProjectPermissions::UPLOAD_VERSION) {
                return Err(ApiError::CustomAuthentication(
                    "You do not have the permissions to edit this version!".to_string(),
                ));
            }

            let mut transaction = pool.begin().await?;

            if let Some(name) = &new_version.name {
                sqlx::query!(
                    "
                    UPDATE versions
                    SET name = $1
                    WHERE (id = $2)
                    ",
                    name.trim(),
                    id as database::models::ids::VersionId,
                )
                .execute(&mut *transaction)
                .await?;
            }

            if let Some(number) = &new_version.version_number {
                sqlx::query!(
                    "
                    UPDATE versions
                    SET version_number = $1
                    WHERE (id = $2)
                    ",
                    number,
                    id as database::models::ids::VersionId,
                )
                .execute(&mut *transaction)
                .await?;
            }

            if let Some(version_type) = &new_version.version_type {
                sqlx::query!(
                    "
                    UPDATE versions
                    SET version_type = $1
                    WHERE (id = $2)
                    ",
                    version_type.as_str(),
                    id as database::models::ids::VersionId,
                )
                .execute(&mut *transaction)
                .await?;
            }

            if let Some(dependencies) = &new_version.dependencies {
                if let Some(project) = project_item {
                    if project.project_type != "modpack" {
                        sqlx::query!(
                            "
                            DELETE FROM dependencies WHERE dependent_id = $1
                            ",
                            id as database::models::ids::VersionId,
                        )
                        .execute(&mut *transaction)
                        .await?;

                        let builders = dependencies
                            .iter()
                            .map(|x| database::models::version_item::DependencyBuilder {
                                project_id: x.project_id.map(|x| x.into()),
                                version_id: x.version_id.map(|x| x.into()),
                                file_name: x.file_name.clone(),
                                dependency_type: x.dependency_type.to_string(),
                            })
                            .collect::<Vec<database::models::version_item::DependencyBuilder>>();

                        DependencyBuilder::insert_many(
                            builders,
                            version_item.inner.id,
                            &mut transaction,
                        )
                        .await?;
                    }
                }
            }

            // if let Some(game_versions) = &new_version.game_versions {
            //     sqlx::query!(
            //         "
            //         DELETE FROM game_versions_versions WHERE joining_version_id = $1
            //         ",
            //         id as database::models::ids::VersionId,
            //     )
            //     .execute(&mut *transaction)
            //     .await?;

            //     let mut version_versions = Vec::new();
            //     for game_version in game_versions {
            //         let game_version_id = database::models::categories::GameVersion::get_id(
            //             &game_version.0,
            //             &mut *transaction,
            //         )
            //         .await?
            //         .ok_or_else(|| {
            //             ApiError::InvalidInput(
            //                 "No database entry for game version provided.".to_string(),
            //             )
            //         })?;

            //         version_versions.push(VersionVersion::new(game_version_id, id));
            //     }
            //     VersionVersion::insert_many(version_versions, &mut transaction).await?;

            //     database::models::Project::update_game_versions(
            //         version_item.inner.project_id,
            //         &mut transaction,
            //     )
            //     .await?;
            // }

            if let Some(loaders) = &new_version.loaders {
                sqlx::query!(
                    "
                    DELETE FROM loaders_versions WHERE version_id = $1
                    ",
                    id as database::models::ids::VersionId,
                )
                .execute(&mut *transaction)
                .await?;

                sqlx::query!(
                    "
                    DELETE FROM version_fields WHERE version_id = $1
                    ",
                    id as database::models::ids::VersionId,
                )
                .execute(&mut *transaction)
                .await?;

                // TODO: This should check if loader supports project type like the version_creation route
                let mut loader_versions = Vec::new();
                let mut version_fields = Vec::new();
                for loader in loaders {
                    let loader_name = loader.loader.0.clone();

                    let loader_id = database::models::loader_fields::Loader::get_id(
                        &loader_name,
                        &mut *transaction,
                    )
                    .await?
                    .ok_or_else(|| {
                        ApiError::InvalidInput("No database entry for loader provided.".to_string())
                    })?;
                    loader_versions.push(LoaderVersion::new(loader_id, id));
                    let loader_fields =
                        LoaderField::get_fields(loader_id, &mut *transaction, &redis).await?;
                    let mut loader_field_enum_values =
                        LoaderFieldEnumValue::list_many_loader_fields(
                            &loader_fields,
                            &mut *transaction,
                            &redis,
                        )
                        .await?;

                    for (key, value) in loader.fields.iter() {
                        let loader_field = loader_fields.iter().find(|lf| &lf.field == key).ok_or_else(|| {
                            ApiError::InvalidInput(format!("Loader field '{key}' does not exist for loader '{loader_name}'"))
                        })?;
                        let enum_variants = loader_field_enum_values
                            .remove(&loader_field.id)
                            .unwrap_or_default();
                        let vf: VersionField = VersionField::check_parse(
                            version_id.into(),
                            loader_field.clone(),
                            value.clone(),
                            enum_variants,
                        )
                        .map_err(ApiError::InvalidInput)?;
                        version_fields.push(vf);
                    }
                }
                LoaderVersion::insert_many(loader_versions, &mut transaction).await?;
                VersionField::insert_many(version_fields, &mut transaction).await?;

                database::models::Project::update_loaders(
                    version_item.inner.project_id,
                    &mut transaction,
                )
                .await?;
            }

            if let Some(featured) = &new_version.featured {
                sqlx::query!(
                    "
                    UPDATE versions
                    SET featured = $1
                    WHERE (id = $2)
                    ",
                    featured,
                    id as database::models::ids::VersionId,
                )
                .execute(&mut *transaction)
                .await?;
            }

            if let Some(primary_file) = &new_version.primary_file {
                let result = sqlx::query!(
                    "
                    SELECT f.id id FROM hashes h
                    INNER JOIN files f ON h.file_id = f.id
                    WHERE h.algorithm = $2 AND h.hash = $1
                    ",
                    primary_file.1.as_bytes(),
                    primary_file.0
                )
                .fetch_optional(&**pool)
                .await?
                .ok_or_else(|| {
                    ApiError::InvalidInput(format!(
                        "Specified file with hash {} does not exist.",
                        primary_file.1.clone()
                    ))
                })?;

                sqlx::query!(
                    "
                    UPDATE files
                    SET is_primary = FALSE
                    WHERE (version_id = $1)
                    ",
                    id as database::models::ids::VersionId,
                )
                .execute(&mut *transaction)
                .await?;

                sqlx::query!(
                    "
                    UPDATE files
                    SET is_primary = TRUE
                    WHERE (id = $1)
                    ",
                    result.id,
                )
                .execute(&mut *transaction)
                .await?;
            }

            if let Some(body) = &new_version.changelog {
                sqlx::query!(
                    "
                    UPDATE versions
                    SET changelog = $1
                    WHERE (id = $2)
                    ",
                    body,
                    id as database::models::ids::VersionId,
                )
                .execute(&mut *transaction)
                .await?;
            }

            if let Some(downloads) = &new_version.downloads {
                if !user.role.is_mod() {
                    return Err(ApiError::CustomAuthentication(
                        "You don't have permission to set the downloads of this mod".to_string(),
                    ));
                }

                sqlx::query!(
                    "
                    UPDATE versions
                    SET downloads = $1
                    WHERE (id = $2)
                    ",
                    *downloads as i32,
                    id as database::models::ids::VersionId,
                )
                .execute(&mut *transaction)
                .await?;

                let diff = *downloads - (version_item.inner.downloads as u32);

                sqlx::query!(
                    "
                    UPDATE mods
                    SET downloads = downloads + $1
                    WHERE (id = $2)
                    ",
                    diff as i32,
                    version_item.inner.project_id as database::models::ids::ProjectId,
                )
                .execute(&mut *transaction)
                .await?;
            }

            if let Some(status) = &new_version.status {
                if !status.can_be_requested() {
                    return Err(ApiError::InvalidInput(
                        "The requested status cannot be set!".to_string(),
                    ));
                }

                sqlx::query!(
                    "
                    UPDATE versions
                    SET status = $1
                    WHERE (id = $2)
                    ",
                    status.as_str(),
                    id as database::models::ids::VersionId,
                )
                .execute(&mut *transaction)
                .await?;
            }

            if let Some(file_types) = &new_version.file_types {
                for file_type in file_types {
                    let result = sqlx::query!(
                        "
                        SELECT f.id id FROM hashes h
                        INNER JOIN files f ON h.file_id = f.id
                        WHERE h.algorithm = $2 AND h.hash = $1
                        ",
                        file_type.hash.as_bytes(),
                        file_type.algorithm
                    )
                    .fetch_optional(&**pool)
                    .await?
                    .ok_or_else(|| {
                        ApiError::InvalidInput(format!(
                            "Specified file with hash {} does not exist.",
                            file_type.algorithm.clone()
                        ))
                    })?;

                    sqlx::query!(
                        "
                        UPDATE files
                        SET file_type = $2
                        WHERE (id = $1)
                        ",
                        result.id,
                        file_type.file_type.as_ref().map(|x| x.as_str()),
                    )
                    .execute(&mut *transaction)
                    .await?;
                }
            }

            // delete any images no longer in the changelog
            let checkable_strings: Vec<&str> = vec![&new_version.changelog]
                .into_iter()
                .filter_map(|x| x.as_ref().map(|y| y.as_str()))
                .collect();
            let context = ImageContext::Version {
                version_id: Some(version_item.inner.id.into()),
            };

            img::delete_unused_images(context, checkable_strings, &mut transaction, &redis).await?;

            database::models::Version::clear_cache(&version_item, &redis).await?;
            database::models::Project::clear_cache(
                version_item.inner.project_id,
                None,
                Some(true),
                &redis,
            )
            .await?;
            transaction.commit().await?;
            Ok(HttpResponse::NoContent().body(""))
        } else {
            Err(ApiError::CustomAuthentication(
                "You do not have permission to edit this version!".to_string(),
            ))
        }
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
}

#[derive(Deserialize)]
pub struct VersionListFilters {
    pub loaders: Option<String>,
    pub featured: Option<bool>,
    pub version_type: Option<VersionType>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    /*
        Loader fields to filter with:
        "game_versions": ["1.16.5", "1.17"]

        Returns if it matches any of the values
    */
    pub loader_fields: Option<String>,
}

pub async fn version_list(
    req: HttpRequest,
    info: web::Path<(String,)>,
    web::Query(filters): web::Query<VersionListFilters>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let string = info.into_inner().0;

    let result = database::models::Project::get(&string, &**pool, &redis).await?;

    let user_option = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::PROJECT_READ, Scopes::VERSION_READ]),
    )
    .await
    .map(|x| x.1)
    .ok();

    if let Some(project) = result {
        if !is_authorized(&project.inner, &user_option, &pool).await? {
            return Ok(HttpResponse::NotFound().body(""));
        }

        let loader_field_filters = filters.loader_fields.as_ref().map(|x| {
            serde_json::from_str::<HashMap<String, Vec<serde_json::Value>>>(x).unwrap_or_default()
        });
        let loader_filters = filters
            .loaders
            .as_ref()
            .map(|x| serde_json::from_str::<Vec<String>>(x).unwrap_or_default());
        let mut versions = database::models::Version::get_many(&project.versions, &**pool, &redis)
            .await?
            .into_iter()
            .skip(filters.offset.unwrap_or(0))
            .take(filters.limit.unwrap_or(usize::MAX))
            .filter(|x| {
                let mut bool = true;

                // TODO: theres a lot of repeated logic here with the similar filterings in super::version_file, abstract it
                if let Some(version_type) = filters.version_type {
                    bool &= &*x.inner.version_type == version_type.as_str();
                }
                if let Some(loaders) = &loader_filters {
                    bool &= x.loaders.iter().any(|y| loaders.contains(y));
                }

                if let Some(loader_fields) = &loader_field_filters {
                    for (key, value) in loader_fields {
                        bool &= x.version_fields.iter().any(|y| {
                            y.field_name == *key && value.contains(&y.value.serialize_internal())
                        });
                    }
                }

                bool
            })
            .collect::<Vec<_>>();

        let mut response = versions
            .iter()
            .filter(|version| {
                filters
                    .featured
                    .map(|featured| featured == version.inner.featured)
                    .unwrap_or(true)
            })
            .cloned()
            .collect::<Vec<_>>();

        versions.sort_by(|a, b| b.inner.date_published.cmp(&a.inner.date_published));

        // Attempt to populate versions with "auto featured" versions
        if response.is_empty() && !versions.is_empty() && filters.featured.unwrap_or(false) {
            // let (loaders, game_versions) = futures::future::try_join(
            //     database::models::loader_fields::Loader::list(&**pool, &redis),
            //     database::models::loader_fields::GameVersion::list_filter(
            //         None,
            //         Some(true),
            //         &**pool,
            //         &redis,
            //     ),
            // )
            // .await?;

            // let mut joined_filters = Vec::new();
            // for game_version in &game_versions {
            //     for loader in &loaders {
            //         joined_filters.push((game_version, loader))
            //     }
            // }

            // joined_filters.into_iter().for_each(|filter| {
            //     versions
            //         .iter()
            //         .find(|version| {
            //             // version.game_versions.contains(&filter.0.version)
            //                 // &&
            //                 version.loaders.contains(&filter.1.loader)
            //         })
            //         .map(|version| response.push(version.clone()))
            //         .unwrap_or(());
            // });

            if response.is_empty() {
                versions
                    .into_iter()
                    .for_each(|version| response.push(version));
            }
        }

        response.sort_by(|a, b| b.inner.date_published.cmp(&a.inner.date_published));
        response.dedup_by(|a, b| a.inner.id == b.inner.id);

        let response = filter_authorized_versions(response, &user_option, &pool).await?;

        Ok(HttpResponse::Ok().json(response))
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
}
