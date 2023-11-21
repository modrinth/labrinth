use std::{collections::HashMap, sync::Arc};

use actix_web::{web, HttpRequest, HttpResponse};
use lazy_static::lazy_static;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use validator::Validate;

use crate::{
    auth::get_user_from_headers,
    database::{models::User, redis::RedisPool},
    file_hosting::FileHost,
    models::{
        collections::{Collection, CollectionStatus},
        ids::UserId,
        notifications::Notification,
        pats::Scopes,
        projects::Project,
        users::{Badges, Role},
    },
    queue::session::AuthQueue,
    util::{routes::read_from_payload, validate::validation_errors_to_string},
};

use super::ApiError;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("user", web::get().to(user_auth_get));
    cfg.route("users", web::get().to(users_get));

    cfg.service(
        web::scope("user")
            .route("{user_id}/projects", web::get().to(projects_list))
            .route("{id}", web::get().to(user_get))
            .route("{user_id}/collections", web::get().to(collections_list))
            .route("{user_id}/organizations", web::get().to(orgs_list))
            .route("{id}", web::patch().to(user_edit))
            .route("{id}/icon", web::patch().to(user_icon_edit))
            .route("{id}", web::delete().to(user_delete))
            .route("{id}/follows", web::get().to(user_follows))
            .route("{id}/notifications", web::get().to(user_notifications)),
    );
}

pub async fn projects_list(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::PROJECT_READ]),
    )
    .await
    .map(|x| x.1)
    .ok();

    let id_option = User::get(&info.into_inner().0, &**pool, &redis).await?;

    if let Some(id) = id_option.map(|x| x.id) {
        let user_id: UserId = id.into();

        let can_view_private = user
            .map(|y| y.role.is_mod() || y.id == user_id)
            .unwrap_or(false);

        let project_data = User::get_projects(id, &**pool, &redis).await?;

        let response: Vec<_> =
            crate::database::Project::get_many_ids(&project_data, &**pool, &redis)
                .await?
                .into_iter()
                .filter(|x| can_view_private || x.inner.status.is_searchable())
                .map(Project::from)
                .collect();

        Ok(HttpResponse::Ok().json(response))
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
}

pub async fn user_auth_get(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let (scopes, mut user) = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::USER_READ]),
    )
    .await?;

    if !scopes.contains(Scopes::USER_READ_EMAIL) {
        user.email = None;
    }

    if !scopes.contains(Scopes::PAYOUTS_READ) {
        user.payout_data = None;
    }

    Ok(HttpResponse::Ok().json(user))
}

#[derive(Serialize, Deserialize)]
pub struct UserIds {
    pub ids: String,
}

pub async fn users_get(
    web::Query(ids): web::Query<UserIds>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
) -> Result<HttpResponse, ApiError> {
    let user_ids = serde_json::from_str::<Vec<String>>(&ids.ids)?;

    let users_data = User::get_many(&user_ids, &**pool, &redis).await?;

    let users: Vec<crate::models::users::User> = users_data.into_iter().map(From::from).collect();

    Ok(HttpResponse::Ok().json(users))
}

pub async fn user_get(
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
) -> Result<HttpResponse, ApiError> {
    let user_data = User::get(&info.into_inner().0, &**pool, &redis).await?;

    if let Some(data) = user_data {
        let response: crate::models::users::User = data.into();
        Ok(HttpResponse::Ok().json(response))
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
}

pub async fn collections_list(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::COLLECTION_READ]),
    )
    .await
    .map(|x| x.1)
    .ok();

    let id_option = User::get(&info.into_inner().0, &**pool, &redis).await?;

    if let Some(id) = id_option.map(|x| x.id) {
        let user_id: UserId = id.into();

        let can_view_private = user
            .map(|y| y.role.is_mod() || y.id == user_id)
            .unwrap_or(false);

        let project_data = User::get_collections(id, &**pool).await?;

        let response: Vec<_> =
            crate::database::models::Collection::get_many(&project_data, &**pool, &redis)
                .await?
                .into_iter()
                .filter(|x| can_view_private || matches!(x.status, CollectionStatus::Listed))
                .map(Collection::from)
                .collect();

        Ok(HttpResponse::Ok().json(response))
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
}

pub async fn orgs_list(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::PROJECT_READ]),
    )
    .await
    .map(|x| x.1)
    .ok();

    let id_option = User::get(&info.into_inner().0, &**pool, &redis).await?;

    if let Some(id) = id_option.map(|x| x.id) {
        let org_data = User::get_organizations(id, &**pool).await?;

        let organizations_data =
            crate::database::models::organization_item::Organization::get_many_ids(
                &org_data, &**pool, &redis,
            )
            .await?;

        let team_ids = organizations_data
            .iter()
            .map(|x| x.team_id)
            .collect::<Vec<_>>();

        let teams_data = crate::database::models::TeamMember::get_from_team_full_many(
            &team_ids, &**pool, &redis,
        )
        .await?;
        let users = User::get_many_ids(
            &teams_data.iter().map(|x| x.user_id).collect::<Vec<_>>(),
            &**pool,
            &redis,
        )
        .await?;

        let mut organizations = vec![];
        let mut team_groups = HashMap::new();
        for item in teams_data {
            team_groups.entry(item.team_id).or_insert(vec![]).push(item);
        }

        for data in organizations_data {
            let members_data = team_groups.remove(&data.team_id).unwrap_or(vec![]);
            let logged_in = user
                .as_ref()
                .and_then(|user| {
                    members_data
                        .iter()
                        .find(|x| x.user_id == user.id.into() && x.accepted)
                })
                .is_some();

            let team_members: Vec<_> = members_data
                .into_iter()
                .filter(|x| logged_in || x.accepted || id == x.user_id)
                .flat_map(|data| {
                    users.iter().find(|x| x.id == data.user_id).map(|user| {
                        crate::models::teams::TeamMember::from(data, user.clone(), !logged_in)
                    })
                })
                .collect();

            let organization = crate::models::organizations::Organization::from(data, team_members);
            organizations.push(organization);
        }

        Ok(HttpResponse::Ok().json(organizations))
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
}

lazy_static! {
    static ref RE_URL_SAFE: Regex = Regex::new(r"^[a-zA-Z0-9_-]*$").unwrap();
}

#[derive(Serialize, Deserialize, Validate)]
pub struct EditUser {
    #[validate(length(min = 1, max = 39), regex = "RE_URL_SAFE")]
    pub username: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    #[validate(length(min = 1, max = 64), regex = "RE_URL_SAFE")]
    pub name: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    #[validate(length(max = 160))]
    pub bio: Option<Option<String>>,
    pub role: Option<Role>,
    pub badges: Option<Badges>,
    #[validate(length(max = 160))]
    pub venmo_handle: Option<String>,
}

pub async fn user_edit(
    req: HttpRequest,
    info: web::Path<(String,)>,
    new_user: web::Json<EditUser>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let (scopes, user) = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::USER_WRITE]),
    )
    .await?;

    new_user
        .validate()
        .map_err(|err| ApiError::Validation(validation_errors_to_string(err, None)))?;

    let id_option = User::get(&info.into_inner().0, &**pool, &redis).await?;

    if let Some(actual_user) = id_option {
        let id = actual_user.id;
        let user_id: UserId = id.into();

        if user.id == user_id || user.role.is_mod() {
            let mut transaction = pool.begin().await?;

            if let Some(username) = &new_user.username {
                let existing_user_id_option = User::get(username, &**pool, &redis).await?;

                if existing_user_id_option
                    .map(|x| UserId::from(x.id))
                    .map(|id| id == user.id)
                    .unwrap_or(true)
                {
                    sqlx::query!(
                        "
                        UPDATE users
                        SET username = $1
                        WHERE (id = $2)
                        ",
                        username,
                        id as crate::database::models::ids::UserId,
                    )
                    .execute(&mut *transaction)
                    .await?;
                } else {
                    return Err(ApiError::InvalidInput(format!(
                        "Username {username} is taken!"
                    )));
                }
            }

            if let Some(name) = &new_user.name {
                sqlx::query!(
                    "
                    UPDATE users
                    SET name = $1
                    WHERE (id = $2)
                    ",
                    name.as_deref(),
                    id as crate::database::models::ids::UserId,
                )
                .execute(&mut *transaction)
                .await?;
            }

            if let Some(bio) = &new_user.bio {
                sqlx::query!(
                    "
                    UPDATE users
                    SET bio = $1
                    WHERE (id = $2)
                    ",
                    bio.as_deref(),
                    id as crate::database::models::ids::UserId,
                )
                .execute(&mut *transaction)
                .await?;
            }

            if let Some(role) = &new_user.role {
                if !user.role.is_admin() {
                    return Err(ApiError::CustomAuthentication(
                        "You do not have the permissions to edit the role of this user!"
                            .to_string(),
                    ));
                }

                let role = role.to_string();

                sqlx::query!(
                    "
                    UPDATE users
                    SET role = $1
                    WHERE (id = $2)
                    ",
                    role,
                    id as crate::database::models::ids::UserId,
                )
                .execute(&mut *transaction)
                .await?;
            }

            if let Some(badges) = &new_user.badges {
                if !user.role.is_admin() {
                    return Err(ApiError::CustomAuthentication(
                        "You do not have the permissions to edit the badges of this user!"
                            .to_string(),
                    ));
                }

                sqlx::query!(
                    "
                    UPDATE users
                    SET badges = $1
                    WHERE (id = $2)
                    ",
                    badges.bits() as i64,
                    id as crate::database::models::ids::UserId,
                )
                .execute(&mut *transaction)
                .await?;
            }

            if let Some(venmo_handle) = &new_user.venmo_handle {
                if !scopes.contains(Scopes::PAYOUTS_WRITE) {
                    return Err(ApiError::CustomAuthentication(
                        "You do not have the permissions to edit the venmo handle of this user!"
                            .to_string(),
                    ));
                }

                sqlx::query!(
                    "
                    UPDATE users
                    SET venmo_handle = $1
                    WHERE (id = $2)
                    ",
                    venmo_handle,
                    id as crate::database::models::ids::UserId,
                )
                .execute(&mut *transaction)
                .await?;
            }

            User::clear_caches(&[(id, Some(actual_user.username))], &redis).await?;
            transaction.commit().await?;
            Ok(HttpResponse::NoContent().body(""))
        } else {
            Err(ApiError::CustomAuthentication(
                "You do not have permission to edit this user!".to_string(),
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

#[allow(clippy::too_many_arguments)]
pub async fn user_icon_edit(
    web::Query(ext): web::Query<Extension>,
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
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
            Some(&[Scopes::USER_WRITE]),
        )
        .await?
        .1;
        let id_option = User::get(&info.into_inner().0, &**pool, &redis).await?;

        if let Some(actual_user) = id_option {
            if user.id != actual_user.id.into() && !user.role.is_mod() {
                return Err(ApiError::CustomAuthentication(
                    "You don't have permission to edit this user's icon.".to_string(),
                ));
            }

            let icon_url = actual_user.avatar_url;
            let user_id: UserId = actual_user.id.into();

            if let Some(icon) = icon_url {
                let name = icon.split(&format!("{cdn_url}/")).nth(1);

                if let Some(icon_path) = name {
                    file_host.delete_file_version("", icon_path).await?;
                }
            }

            let bytes =
                read_from_payload(&mut payload, 2097152, "Icons must be smaller than 2MiB").await?;

            let hash = sha1::Sha1::from(&bytes).hexdigest();
            let upload_data = file_host
                .upload_file(
                    content_type,
                    &format!("user/{}/{}.{}", user_id, hash, ext.ext),
                    bytes.freeze(),
                )
                .await?;

            sqlx::query!(
                "
                UPDATE users
                SET avatar_url = $1
                WHERE (id = $2)
                ",
                format!("{}/{}", cdn_url, upload_data.file_name),
                actual_user.id as crate::database::models::ids::UserId,
            )
            .execute(&**pool)
            .await?;
            User::clear_caches(&[(actual_user.id, None)], &redis).await?;

            Ok(HttpResponse::NoContent().body(""))
        } else {
            Ok(HttpResponse::NotFound().body(""))
        }
    } else {
        Err(ApiError::InvalidInput(format!(
            "Invalid format for user icon: {}",
            ext.ext
        )))
    }
}

#[derive(Deserialize)]
pub struct RemovalType {
    #[serde(default = "default_removal")]
    pub removal_type: String,
}

fn default_removal() -> String {
    "partial".into()
}

pub async fn user_delete(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    removal_type: web::Query<RemovalType>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::USER_DELETE]),
    )
    .await?
    .1;
    let id_option = User::get(&info.into_inner().0, &**pool, &redis).await?;

    if let Some(id) = id_option.map(|x| x.id) {
        if !user.role.is_admin() && user.id != id.into() {
            return Err(ApiError::CustomAuthentication(
                "You do not have permission to delete this user!".to_string(),
            ));
        }

        let mut transaction = pool.begin().await?;

        let result = User::remove(
            id,
            removal_type.removal_type == "full",
            &mut transaction,
            &redis,
        )
        .await?;

        transaction.commit().await?;

        if result.is_some() {
            Ok(HttpResponse::NoContent().body(""))
        } else {
            Ok(HttpResponse::NotFound().body(""))
        }
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
}

pub async fn user_follows(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::USER_READ]),
    )
    .await?
    .1;
    let id_option = User::get(&info.into_inner().0, &**pool, &redis).await?;

    if let Some(id) = id_option.map(|x| x.id) {
        if !user.role.is_admin() && user.id != id.into() {
            return Err(ApiError::CustomAuthentication(
                "You do not have permission to see the projects this user follows!".to_string(),
            ));
        }

        use futures::TryStreamExt;

        let project_ids = sqlx::query!(
            "
            SELECT mf.mod_id FROM mod_follows mf
            WHERE mf.follower_id = $1
            ",
            id as crate::database::models::ids::UserId,
        )
        .fetch_many(&**pool)
        .try_filter_map(|e| async {
            Ok(e.right()
                .map(|m| crate::database::models::ProjectId(m.mod_id)))
        })
        .try_collect::<Vec<crate::database::models::ProjectId>>()
        .await?;

        let projects: Vec<_> =
            crate::database::Project::get_many_ids(&project_ids, &**pool, &redis)
                .await?
                .into_iter()
                .map(Project::from)
                .collect();

        Ok(HttpResponse::Ok().json(projects))
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
}

pub async fn user_notifications(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::NOTIFICATION_READ]),
    )
    .await?
    .1;
    let id_option = User::get(&info.into_inner().0, &**pool, &redis).await?;

    if let Some(id) = id_option.map(|x| x.id) {
        if !user.role.is_admin() && user.id != id.into() {
            return Err(ApiError::CustomAuthentication(
                "You do not have permission to see the notifications of this user!".to_string(),
            ));
        }

        let mut notifications: Vec<Notification> =
            crate::database::models::notification_item::Notification::get_many_user(
                id, &**pool, &redis,
            )
            .await?
            .into_iter()
            .map(Into::into)
            .collect();

        notifications.sort_by(|a, b| b.created.cmp(&a.created));
        Ok(HttpResponse::Ok().json(notifications))
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
}
