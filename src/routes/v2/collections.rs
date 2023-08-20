use crate::auth::checks::{filter_authorized_collections, is_authorized_collection};
use crate::auth::get_user_from_headers;
use crate::database;
use crate::file_hosting::FileHost;
use crate::models;
use crate::models::collections::Collection;
use crate::models::ids::base62_impl::parse_base62;
use crate::models::ids::CollectionId;
use crate::models::pats::Scopes;
use crate::models::teams::Permissions;
use crate::queue::session::AuthQueue;
use crate::routes::ApiError;
use crate::util::routes::read_from_payload;
use crate::util::validate::validation_errors_to_string;
use actix_web::{delete, get, patch, web, HttpRequest, HttpResponse};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use std::sync::Arc;
use validator::Validate;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(collections_get);
    cfg.service(super::project_creation::collection_create);
    cfg.service(
        web::scope("collection")
            .service(collection_get)
            .service(collection_get_check)
            .service(collection_delete)
            .service(collection_edit)
            .service(collection_icon_edit)
            .service(delete_collection_icon)
            .service(super::teams::team_members_get_collection),
    );
}

#[derive(Serialize, Deserialize)]
pub struct CollectionIds {
    pub ids: String,
}
#[get("collections")]
pub async fn collections_get(
    req: HttpRequest,
    web::Query(ids): web::Query<CollectionIds>,
    pool: web::Data<PgPool>,
    redis: web::Data<deadpool_redis::Pool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let ids = serde_json::from_str::<Vec<&str>>(&ids.ids)?;
    let collections_data = database::models::Collection::get_many(&ids, &**pool, &redis).await?;

    let user_option = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::COLLECTION_READ]),
    )
    .await
    .map(|x| x.1)
    .ok();

    let collections = filter_authorized_collections(collections_data, &user_option, &pool).await?;

    Ok(HttpResponse::Ok().json(collections))
}

#[get("{id}")]
pub async fn collection_get(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<deadpool_redis::Pool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let string = info.into_inner().0;

    let collection_data = database::models::Collection::get(&string, &**pool, &redis).await?;
    let user_option = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::COLLECTION_READ]),
    )
    .await
    .map(|x| x.1)
    .ok();

    if let Some(data) = collection_data {
        if is_authorized_collection(&data, &user_option, &pool).await? {
            return Ok(HttpResponse::Ok().json(Collection::from(data)));
        }
    }
    Ok(HttpResponse::NotFound().body(""))
}

//checks the validity of a project id or slug
#[get("{id}/check")]
pub async fn collection_get_check(
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<deadpool_redis::Pool>,
) -> Result<HttpResponse, ApiError> {
    let slug = info.into_inner().0;

    let collection_data = database::models::Collection::get(&slug, &**pool, &redis).await?;

    if let Some(collection) = collection_data {
        Ok(HttpResponse::Ok().json(json! ({
            "id": models::ids::CollectionId::from(collection.id)
        })))
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
}

#[derive(Deserialize, Validate)]
pub struct EditCollection {
    #[validate(
        length(min = 3, max = 64),
        custom(function = "crate::util::validate::validate_name")
    )]
    pub title: Option<String>,
    #[validate(length(min = 3, max = 256))]
    pub description: Option<String>,
    #[validate(length(max = 65536))]
    pub body: Option<String>,
    #[validate(
        length(min = 3, max = 64),
        regex = "crate::util::validate::RE_URL_SAFE"
    )]
    pub slug: Option<String>,
    #[validate(length(max = 64))]
    pub add_projects: Option<Vec<String>>,
    #[validate(length(max = 64))]
    pub remove_projects: Option<Vec<String>>,
}

#[patch("{id}")]
pub async fn collection_edit(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    new_collection: web::Json<EditCollection>,
    redis: web::Data<deadpool_redis::Pool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::COLLECTION_WRITE]),
    )
    .await?
    .1;

    new_collection
        .validate()
        .map_err(|err| ApiError::Validation(validation_errors_to_string(err, None)))?;

    let string = info.into_inner().0;
    let result = database::models::Collection::get(&string, &**pool, &redis).await?;

    if let Some(collection_item) = result {
        let id = collection_item.id;

        let team_member = database::models::TeamMember::get_from_user_id(
            collection_item.team_id,
            user.id.into(),
            &**pool,
        )
        .await?;
        let permissions;

        if user.role.is_admin() {
            permissions = Some(Permissions::ALL)
        } else if let Some(ref member) = team_member {
            permissions = Some(member.permissions)
        } else if user.role.is_mod() {
            permissions = Some(Permissions::EDIT_DETAILS | Permissions::EDIT_BODY)
        } else {
            permissions = None
        }

        if let Some(perms) = permissions {
            let mut transaction = pool.begin().await?;

            if let Some(title) = &new_collection.title {
                if !perms.contains(Permissions::EDIT_DETAILS) {
                    return Err(ApiError::CustomAuthentication(
                        "You do not have the permissions to edit the title of this collection!"
                            .to_string(),
                    ));
                }

                sqlx::query!(
                    "
                    UPDATE collections
                    SET title = $1
                    WHERE (id = $2)
                    ",
                    title.trim(),
                    id as database::models::ids::CollectionId,
                )
                .execute(&mut *transaction)
                .await?;
            }

            if let Some(description) = &new_collection.description {
                if !perms.contains(Permissions::EDIT_DETAILS) {
                    return Err(ApiError::CustomAuthentication(
                        "You do not have the permissions to edit the description of this collection!"
                            .to_string(),
                    ));
                }

                sqlx::query!(
                    "
                    UPDATE collections
                    SET description = $1
                    WHERE (id = $2)
                    ",
                    description,
                    id as database::models::ids::CollectionId,
                )
                .execute(&mut *transaction)
                .await?;
            }

            if let Some(slug) = &new_collection.slug {
                if !perms.contains(Permissions::EDIT_DETAILS) {
                    return Err(ApiError::CustomAuthentication(
                        "You do not have the permissions to edit the slug of this collection!"
                            .to_string(),
                    ));
                }

                let slug_collection_id_option: Option<u64> = parse_base62(slug).ok();
                if let Some(slug_collection_id) = slug_collection_id_option {
                    let results = sqlx::query!(
                        "
                        SELECT EXISTS(SELECT 1 FROM collections WHERE id=$1)
                        ",
                        slug_collection_id as i64
                    )
                    .fetch_one(&mut *transaction)
                    .await?;

                    if results.exists.unwrap_or(true) {
                        return Err(ApiError::InvalidInput(
                            "Slug collides with other collections's id!".to_string(),
                        ));
                    }
                }

                // Make sure the new slug is different from the old one
                // We are able to unwrap here because the slug is always set
                if !slug.eq(&new_collection.slug.clone().unwrap_or_default()) {
                    let results = sqlx::query!(
                        "
                      SELECT EXISTS(SELECT 1 FROM mods WHERE slug = LOWER($1))
                      ",
                        slug
                    )
                    .fetch_one(&mut *transaction)
                    .await?;

                    if results.exists.unwrap_or(true) {
                        return Err(ApiError::InvalidInput(
                            "Slug collides with other project's id!".to_string(),
                        ));
                    }
                }

                sqlx::query!(
                    "
                    UPDATE collections
                    SET slug = LOWER($1)
                    WHERE (id = $2)
                    ",
                    Some(slug),
                    id as database::models::ids::CollectionId,
                )
                .execute(&mut *transaction)
                .await?;
            }

            if let Some(body) = &new_collection.body {
                if !perms.contains(Permissions::EDIT_BODY) {
                    return Err(ApiError::CustomAuthentication(
                        "You do not have the permissions to edit the body of this collection!"
                            .to_string(),
                    ));
                }

                sqlx::query!(
                    "
                    UPDATE collections
                    SET body = $1
                    WHERE (id = $2)
                    ",
                    body,
                    id as database::models::ids::CollectionId,
                )
                .execute(&mut *transaction)
                .await?;
            }

            if let Some(add_project_ids) = &new_collection.add_projects {
                for project_id in add_project_ids {
                    let project = database::models::Project::get(project_id, &**pool, &redis)
                        .await?
                        .ok_or_else(|| {
                            ApiError::InvalidInput(format!(
                                "The specified project {project_id} does not exist!"
                            ))
                        })?;

                    // Insert- don't throw an error if it already exists
                    sqlx::query!(
                        "
                                INSERT INTO collections_mods (collection_id, mod_id)
                                VALUES ($1, $2)
                                ON CONFLICT DO NOTHING
                                ",
                        collection_item.id as database::models::ids::CollectionId,
                        project.inner.id as database::models::ids::ProjectId,
                    )
                    .execute(&mut *transaction)
                    .await?;
                }
            }
            if let Some(remove_project_ids) = &new_collection.remove_projects {
                for project_id in remove_project_ids {
                    let project = database::models::Project::get(project_id, &**pool, &redis)
                        .await?
                        .ok_or_else(|| {
                            ApiError::InvalidInput(format!(
                                "The specified project {project_id} does not exist!"
                            ))
                        })?;
                    if collection_item.projects.contains(&project.inner.id) {
                        sqlx::query!(
                            "
                            DELETE FROM collections_mods
                            WHERE collection_id = $1 AND mod_id = $2
                            ",
                            collection_item.id as database::models::ids::CollectionId,
                            project.inner.id as database::models::ids::ProjectId,
                        )
                        .execute(&mut *transaction)
                        .await?;
                    } else {
                        return Err(ApiError::InvalidInput(format!(
                            "The specified project {project_id} is not in this collection!"
                        )));
                    }
                }
            }

            database::models::Collection::clear_cache(
                collection_item.id,
                collection_item.slug,
                &redis,
            )
            .await?;

            transaction.commit().await?;
            Ok(HttpResponse::NoContent().body(""))
        } else {
            Err(ApiError::CustomAuthentication(
                "You do not have permission to edit this project!".to_string(),
            ))
        }
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
}

#[derive(Serialize, Deserialize)]
pub struct Extension {
    pub ext: String,
}

#[patch("{id}/icon")]
#[allow(clippy::too_many_arguments)]
pub async fn collection_icon_edit(
    web::Query(ext): web::Query<Extension>,
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<deadpool_redis::Pool>,
    file_host: web::Data<Arc<dyn FileHost + Send + Sync>>,
    mut payload: web::Payload,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    if let Some(content_type) = crate::util::ext::get_image_content_type(&ext.ext) {
        let cdn_url = dotenvy::var("CDN_URL")?;
        let user = get_user_from_headers(
            &req,
            &**pool,
            &redis,
            &session_queue,
            Some(&[Scopes::COLLECTION_WRITE]),
        )
        .await?
        .1;
        let string = info.into_inner().0;

        let collection_item = database::models::Collection::get(&string, &**pool, &redis)
            .await?
            .ok_or_else(|| {
                ApiError::InvalidInput("The specified collection does not exist!".to_string())
            })?;

        if !user.role.is_mod() {
            let team_member = database::models::TeamMember::get_from_user_id(
                collection_item.team_id,
                user.id.into(),
                &**pool,
            )
            .await
            .map_err(ApiError::Database)?
            .ok_or_else(|| {
                ApiError::InvalidInput("The specified collection does not exist!".to_string())
            })?;

            if !team_member.permissions.contains(Permissions::EDIT_DETAILS) {
                return Err(ApiError::CustomAuthentication(
                    "You don't have permission to edit this collection's icon.".to_string(),
                ));
            }
        }

        if let Some(icon) = collection_item.icon_url {
            let name = icon.split(&format!("{cdn_url}/")).nth(1);

            if let Some(icon_path) = name {
                file_host.delete_file_version("", icon_path).await?;
            }
        }

        let bytes =
            read_from_payload(&mut payload, 262144, "Icons must be smaller than 256KiB").await?;

        let color = crate::util::img::get_color_from_img(&bytes)?;

        let hash = sha1::Sha1::from(&bytes).hexdigest();
        let collection_id: CollectionId = collection_item.id.into();
        let upload_data = file_host
            .upload_file(
                content_type,
                &format!("data/{}/{}.{}", collection_id, hash, ext.ext),
                bytes.freeze(),
            )
            .await?;

        let mut transaction = pool.begin().await?;

        sqlx::query!(
            "
            UPDATE collections
            SET icon_url = $1, color = $2
            WHERE (id = $3)
            ",
            format!("{}/{}", cdn_url, upload_data.file_name),
            color.map(|x| x as i32),
            collection_item.id as database::models::ids::CollectionId,
        )
        .execute(&mut *transaction)
        .await?;

        database::models::Collection::clear_cache(collection_item.id, collection_item.slug, &redis)
            .await?;

        transaction.commit().await?;

        Ok(HttpResponse::NoContent().body(""))
    } else {
        Err(ApiError::InvalidInput(format!(
            "Invalid format for collection icon: {}",
            ext.ext
        )))
    }
}

#[delete("{id}/icon")]
pub async fn delete_collection_icon(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<deadpool_redis::Pool>,
    file_host: web::Data<Arc<dyn FileHost + Send + Sync>>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::COLLECTION_WRITE]),
    )
    .await?
    .1;
    let string = info.into_inner().0;

    let collection_item = database::models::Collection::get(&string, &**pool, &redis)
        .await?
        .ok_or_else(|| {
            ApiError::InvalidInput("The specified collection does not exist!".to_string())
        })?;

    if !user.role.is_mod() {
        let team_member = database::models::TeamMember::get_from_user_id(
            collection_item.team_id,
            user.id.into(),
            &**pool,
        )
        .await
        .map_err(ApiError::Database)?
        .ok_or_else(|| {
            ApiError::InvalidInput("The specified collection does not exist!".to_string())
        })?;

        if !team_member.permissions.contains(Permissions::EDIT_DETAILS) {
            return Err(ApiError::CustomAuthentication(
                "You don't have permission to edit this collection's icon.".to_string(),
            ));
        }
    }

    let cdn_url = dotenvy::var("CDN_URL")?;
    if let Some(icon) = collection_item.icon_url {
        let name = icon.split(&format!("{cdn_url}/")).nth(1);

        if let Some(icon_path) = name {
            file_host.delete_file_version("", icon_path).await?;
        }
    }

    let mut transaction = pool.begin().await?;

    sqlx::query!(
        "
        UPDATE collections
        SET icon_url = NULL, color = NULL
        WHERE (id = $1)
        ",
        collection_item.id as database::models::ids::CollectionId,
    )
    .execute(&mut *transaction)
    .await?;

    database::models::Collection::clear_cache(collection_item.id, collection_item.slug, &redis)
        .await?;

    transaction.commit().await?;

    Ok(HttpResponse::NoContent().body(""))
}

#[delete("{id}")]
pub async fn collection_delete(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<deadpool_redis::Pool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::COLLECTION_DELETE]),
    )
    .await?
    .1;
    let string = info.into_inner().0;

    let collection = database::models::Collection::get(&string, &**pool, &redis)
        .await?
        .ok_or_else(|| {
            ApiError::InvalidInput("The specified collection does not exist!".to_string())
        })?;

    if !user.role.is_admin() {
        let team_member = database::models::TeamMember::get_from_user_id_collection(
            collection.id,
            user.id.into(),
            &**pool,
        )
        .await
        .map_err(ApiError::Database)?
        .ok_or_else(|| {
            ApiError::InvalidInput("The specified collection does not exist!".to_string())
        })?;

        if !team_member
            .permissions
            .contains(Permissions::DELETE_COLLECTION)
        {
            return Err(ApiError::CustomAuthentication(
                "You don't have permission to delete this collection!".to_string(),
            ));
        }
    }

    let mut transaction = pool.begin().await?;

    let result =
        database::models::Collection::remove(collection.id, &mut transaction, &redis).await?;

    transaction.commit().await?;

    if result.is_some() {
        Ok(HttpResponse::NoContent().body(""))
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
}
