use crate::auth::checks::is_visible_collection;
use crate::auth::{filter_visible_collections, get_user_from_headers};
use crate::database::models::{collection_item, generate_collection_id, project_item};
use crate::database::redis::RedisPool;
use crate::file_hosting::FileHost;
use crate::models::collections::{Collection, CollectionStatus};
use crate::models::ids::base62_impl::parse_base62;
use crate::models::ids::{CollectionId, ProjectId};
use crate::models::pats::Scopes;
use crate::queue::session::AuthQueue;
use crate::routes::v3::project_creation::CreateError;
use crate::routes::ApiError;
use crate::util::routes::read_from_payload;
use crate::util::validate::validation_errors_to_string;
use crate::{database, models};
use axum::extract::{ConnectInfo, Path, Query};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, patch, post};
use axum::{Extension, Json, Router};
use bytes::Bytes;
use chrono::Utc;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::net::SocketAddr;
use std::sync::Arc;
use validator::Validate;

pub fn config() -> Router {
    Router::new()
        .route("/collections", get(collections_get))
        .route("/collection", post(collection_create))
        .route(
            "/collection/:id",
            get(collection_get)
                .delete(collection_delete)
                .patch(collection_edit),
        )
        .route(
            "/collection/:id/icon",
            patch(collection_icon_edit).delete(delete_collection_icon),
        )
}

#[derive(Serialize, Deserialize, Validate, Clone)]
pub struct CollectionCreateData {
    #[validate(
        length(min = 3, max = 64),
        custom(function = "crate::util::validate::validate_name")
    )]
    /// The title or name of the project.
    pub name: String,
    #[validate(length(min = 3, max = 255))]
    /// A short description of the collection.
    pub description: Option<String>,
    #[validate(length(max = 32))]
    #[serde(default = "Vec::new")]
    /// A list of initial projects to use with the created collection
    pub projects: Vec<String>,
}

pub async fn collection_create(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(client): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
    Json(collection_create_data): Json<CollectionCreateData>,
) -> Result<Json<Collection>, CreateError> {
    // The currently logged in user
    let current_user = get_user_from_headers(
        &addr,
        &headers,
        &client,
        &redis,
        &session_queue,
        Some(&[Scopes::COLLECTION_CREATE]),
    )
    .await?
    .1;

    collection_create_data
        .validate()
        .map_err(|err| CreateError::InvalidInput(validation_errors_to_string(err, None)))?;

    let mut transaction = client.begin().await?;

    let collection_id: CollectionId = generate_collection_id(&mut transaction).await?.into();

    let initial_project_ids = project_item::Project::get_many(
        &collection_create_data.projects,
        &mut *transaction,
        &redis,
    )
    .await?
    .into_iter()
    .map(|x| x.inner.id.into())
    .collect::<Vec<ProjectId>>();

    let collection_builder_actual = collection_item::CollectionBuilder {
        collection_id: collection_id.into(),
        user_id: current_user.id.into(),
        name: collection_create_data.name,
        description: collection_create_data.description,
        status: CollectionStatus::Listed,
        projects: initial_project_ids
            .iter()
            .copied()
            .map(|x| x.into())
            .collect(),
    };
    let collection_builder = collection_builder_actual.clone();

    let now = Utc::now();
    collection_builder_actual.insert(&mut transaction).await?;

    let response = Collection {
        id: collection_id,
        user: collection_builder.user_id.into(),
        name: collection_builder.name.clone(),
        description: collection_builder.description.clone(),
        created: now,
        updated: now,
        icon_url: None,
        color: None,
        status: collection_builder.status,
        projects: initial_project_ids,
    };
    transaction.commit().await?;

    Ok(Json(response))
}

#[derive(Serialize, Deserialize)]
pub struct CollectionIds {
    pub ids: String,
}
pub async fn collections_get(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(ids): Query<CollectionIds>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<Json<Vec<Collection>>, ApiError> {
    let ids = serde_json::from_str::<Vec<&str>>(&ids.ids)?;
    let ids = ids
        .into_iter()
        .map(|x| parse_base62(x).map(|x| database::models::CollectionId(x as i64)))
        .collect::<Result<Vec<_>, _>>()?;

    let collections_data = database::models::Collection::get_many(&ids, &pool, &redis).await?;

    let user_option = get_user_from_headers(
        &addr,
        &headers,
        &pool,
        &redis,
        &session_queue,
        Some(&[Scopes::COLLECTION_READ]),
    )
    .await
    .map(|x| x.1)
    .ok();

    let collections = filter_visible_collections(collections_data, &user_option).await?;

    Ok(Json(collections))
}

pub async fn collection_get(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<Json<Collection>, ApiError> {
    let id = database::models::CollectionId(parse_base62(&info)? as i64);
    let collection_data = database::models::Collection::get(id, &pool, &redis).await?;
    let user_option = get_user_from_headers(
        &addr,
        &headers,
        &pool,
        &redis,
        &session_queue,
        Some(&[Scopes::COLLECTION_READ]),
    )
    .await
    .map(|x| x.1)
    .ok();

    if let Some(data) = collection_data {
        if is_visible_collection(&data, &user_option).await? {
            return Ok(Json(Collection::from(data)));
        }
    }
    Err(ApiError::NotFound)
}

#[derive(Deserialize, Validate)]
pub struct EditCollection {
    #[validate(
        length(min = 3, max = 64),
        custom(function = "crate::util::validate::validate_name")
    )]
    pub name: Option<String>,
    #[validate(length(min = 3, max = 256))]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    pub description: Option<Option<String>>,
    pub status: Option<CollectionStatus>,
    #[validate(length(max = 1024))]
    pub new_projects: Option<Vec<String>>,
}

pub async fn collection_edit(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
    Json(new_collection): Json<EditCollection>,
) -> Result<StatusCode, ApiError> {
    let user = get_user_from_headers(
        &addr,
        &headers,
        &pool,
        &redis,
        &session_queue,
        Some(&[Scopes::COLLECTION_WRITE]),
    )
    .await?
    .1;

    new_collection
        .validate()
        .map_err(|err| ApiError::Validation(validation_errors_to_string(err, None)))?;

    let id = database::models::CollectionId(parse_base62(&info)? as i64);
    let result = database::models::Collection::get(id, &pool, &redis).await?;

    if let Some(collection_item) = result {
        if !can_modify_collection(&collection_item, &user) {
            return Err(ApiError::CustomAuthentication(
                "You do not have permissions to modify this collection!".to_string(),
            ));
        }

        let id = collection_item.id;

        let mut transaction = pool.begin().await?;

        if let Some(name) = &new_collection.name {
            sqlx::query!(
                "
                UPDATE collections
                SET name = $1
                WHERE (id = $2)
                ",
                name.trim(),
                id as database::models::ids::CollectionId,
            )
            .execute(&mut *transaction)
            .await?;
        }

        if let Some(description) = &new_collection.description {
            sqlx::query!(
                "
                UPDATE collections
                SET description = $1
                WHERE (id = $2)
                ",
                description.as_ref(),
                id as database::models::ids::CollectionId,
            )
            .execute(&mut *transaction)
            .await?;
        }

        if let Some(status) = &new_collection.status {
            if !(user.role.is_mod()
                || collection_item.status.is_approved() && status.can_be_requested())
            {
                return Err(ApiError::CustomAuthentication(
                    "You don't have permission to set this status!".to_string(),
                ));
            }

            sqlx::query!(
                "
                UPDATE collections
                SET status = $1
                WHERE (id = $2)
                ",
                status.to_string(),
                id as database::models::ids::CollectionId,
            )
            .execute(&mut *transaction)
            .await?;
        }

        if let Some(new_project_ids) = &new_collection.new_projects {
            // Delete all existing projects
            sqlx::query!(
                "
                DELETE FROM collections_mods
                WHERE collection_id = $1
                ",
                collection_item.id as database::models::ids::CollectionId,
            )
            .execute(&mut *transaction)
            .await?;

            let collection_item_ids = new_project_ids
                .iter()
                .map(|_| collection_item.id.0)
                .collect_vec();
            let mut validated_project_ids = Vec::new();
            for project_id in new_project_ids {
                let project = database::models::Project::get(project_id, &pool, &redis)
                    .await?
                    .ok_or_else(|| {
                        ApiError::InvalidInput(format!(
                            "The specified project {project_id} does not exist!"
                        ))
                    })?;
                validated_project_ids.push(project.inner.id.0);
            }
            // Insert- don't throw an error if it already exists
            sqlx::query!(
                "
                INSERT INTO collections_mods (collection_id, mod_id)
                SELECT * FROM UNNEST ($1::int8[], $2::int8[])
                ON CONFLICT DO NOTHING
                ",
                &collection_item_ids[..],
                &validated_project_ids[..],
            )
            .execute(&mut *transaction)
            .await?;

            sqlx::query!(
                "
                UPDATE collections
                SET updated = NOW()
                WHERE id = $1
                ",
                collection_item.id as database::models::ids::CollectionId,
            )
            .execute(&mut *transaction)
            .await?;
        }

        transaction.commit().await?;
        database::models::Collection::clear_cache(collection_item.id, &redis).await?;

        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound)
    }
}

#[derive(Serialize, Deserialize)]
pub struct FileExt {
    pub ext: String,
}

#[allow(clippy::too_many_arguments)]
pub async fn collection_icon_edit(
    Query(ext): Query<FileExt>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(file_host): Extension<Arc<dyn FileHost + Send + Sync>>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
    payload: Bytes,
) -> Result<StatusCode, ApiError> {
    if let Some(content_type) = crate::util::ext::get_image_content_type(&ext.ext) {
        let cdn_url = dotenvy::var("CDN_URL")?;
        let user = get_user_from_headers(
            &addr,
            &headers,
            &pool,
            &redis,
            &session_queue,
            Some(&[Scopes::COLLECTION_WRITE]),
        )
        .await?
        .1;

        let id = database::models::CollectionId(parse_base62(&info)? as i64);
        let collection_item = database::models::Collection::get(id, &pool, &redis)
            .await?
            .ok_or_else(|| {
                ApiError::InvalidInput("The specified collection does not exist!".to_string())
            })?;

        if !can_modify_collection(&collection_item, &user) {
            return Err(ApiError::CustomAuthentication(
                "You do not have permissions to modify this collection!".to_string(),
            ));
        }

        if let Some(icon) = collection_item.icon_url {
            let name = icon.split(&format!("{cdn_url}/")).nth(1);

            if let Some(icon_path) = name {
                file_host.delete_file_version("", icon_path).await?;
            }
        }

        let bytes = read_from_payload(payload, 262144, "Icons must be smaller than 256KiB").await?;

        let color = crate::util::img::get_color_from_img(&bytes)?;

        let hash = sha1::Sha1::from(&bytes).hexdigest();
        let collection_id: CollectionId = collection_item.id.into();
        let upload_data = file_host
            .upload_file(
                content_type,
                &format!("data/{}/{}.{}", collection_id, hash, ext.ext),
                bytes,
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

        transaction.commit().await?;
        database::models::Collection::clear_cache(collection_item.id, &redis).await?;

        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::InvalidInput(format!(
            "Invalid format for collection icon: {}",
            ext.ext
        )))
    }
}

pub async fn delete_collection_icon(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(string): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(file_host): Extension<Arc<dyn FileHost + Send + Sync>>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<StatusCode, ApiError> {
    let user = get_user_from_headers(
        &addr,
        &headers,
        &pool,
        &redis,
        &session_queue,
        Some(&[Scopes::COLLECTION_WRITE]),
    )
    .await?
    .1;

    let id = database::models::CollectionId(parse_base62(&string)? as i64);
    let collection_item = database::models::Collection::get(id, &pool, &redis)
        .await?
        .ok_or_else(|| {
            ApiError::InvalidInput("The specified collection does not exist!".to_string())
        })?;
    if !can_modify_collection(&collection_item, &user) {
        return Err(ApiError::CustomAuthentication(
            "You do not have permissions to modify this collection!".to_string(),
        ));
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

    transaction.commit().await?;
    database::models::Collection::clear_cache(collection_item.id, &redis).await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn collection_delete(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(string): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<StatusCode, ApiError> {
    let user = get_user_from_headers(
        &addr,
        &headers,
        &pool,
        &redis,
        &session_queue,
        Some(&[Scopes::COLLECTION_DELETE]),
    )
    .await?
    .1;

    let id = database::models::CollectionId(parse_base62(&string)? as i64);
    let collection = database::models::Collection::get(id, &pool, &redis)
        .await?
        .ok_or_else(|| {
            ApiError::InvalidInput("The specified collection does not exist!".to_string())
        })?;
    if !can_modify_collection(&collection, &user) {
        return Err(ApiError::CustomAuthentication(
            "You do not have permissions to modify this collection!".to_string(),
        ));
    }
    let mut transaction = pool.begin().await?;

    let result =
        database::models::Collection::remove(collection.id, &mut transaction, &redis).await?;

    transaction.commit().await?;
    database::models::Collection::clear_cache(collection.id, &redis).await?;

    if result.is_some() {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound)
    }
}

fn can_modify_collection(
    collection: &database::models::Collection,
    user: &models::users::User,
) -> bool {
    collection.user_id == user.id.into() || user.role.is_mod()
}
