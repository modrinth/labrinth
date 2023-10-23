use crate::auth::{filter_authorized_projects, get_user_from_headers, is_authorized};
use crate::database::models as db_models;
use crate::database::models::ids as db_ids;
use crate::database::models::notification_item::NotificationBuilder;
use crate::database::models::project_item::ModCategory;
use crate::database::models::thread_item::ThreadMessageBuilder;
use crate::database::redis::RedisPool;
use crate::models;
use crate::models::ids::base62_impl::parse_base62;
use crate::models::images::ImageContext;
use crate::models::notifications::NotificationBody;
use crate::models::pats::Scopes;
use crate::models::projects::{
    DonationLink, MonetizationStatus, Project, ProjectId, ProjectStatus, SearchRequest,
};
use crate::models::teams::ProjectPermissions;
use crate::models::threads::MessageBody;
use crate::queue::session::AuthQueue;
use crate::routes::ApiError;
use crate::search::{search_for_project, SearchConfig, SearchError};
use crate::util::img;
use crate::util::validate::validation_errors_to_string;
use actix_web::{get, web, HttpRequest, HttpResponse};
use futures::TryStreamExt;
use meilisearch_sdk::indexes::IndexesResults;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use validator::Validate;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("project")
            .route("{id}", web::get().to(project_get))
            .route("projects", web::get().to(projects_get))
            .route("{id}", web::patch().to(project_edit))
            .service(
                web::scope("{project_id}")
                    .route("versions", web::get().to(super::versions::version_list)),
            ),
    );
}

#[derive(Serialize, Deserialize)]
pub struct ProjectIds {
    pub ids: String,
}

pub async fn projects_get(
    req: HttpRequest,
    web::Query(ids): web::Query<ProjectIds>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let ids = serde_json::from_str::<Vec<&str>>(&ids.ids)?;
    let projects_data = db_models::Project::get_many(&ids, &**pool, &redis).await?;

    let user_option = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::PROJECT_READ]),
    )
    .await
    .map(|x| x.1)
    .ok();

    let projects = filter_authorized_projects(projects_data, &user_option, &pool).await?;

    Ok(HttpResponse::Ok().json(projects))
}

pub async fn project_get(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let string = info.into_inner().0;

    let project_data = db_models::Project::get(&string, &**pool, &redis).await?;
    let user_option = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::PROJECT_READ]),
    )
    .await
    .map(|x| x.1)
    .ok();

    if let Some(data) = project_data {
        if is_authorized(&data.inner, &user_option, &pool).await? {
            return Ok(HttpResponse::Ok().json(Project::from(data)));
        }
    }
    Ok(HttpResponse::NotFound().body(""))
}

#[derive(Serialize, Deserialize, Validate)]
pub struct EditProject {
    #[validate(
        length(min = 3, max = 64),
        custom(function = "crate::util::validate::validate_name")
    )]
    pub title: Option<String>,
    #[validate(length(min = 3, max = 256))]
    pub description: Option<String>,
    #[validate(length(max = 65536))]
    pub body: Option<String>,
    #[validate(length(max = 3))]
    pub categories: Option<Vec<String>>,
    #[validate(length(max = 256))]
    pub additional_categories: Option<Vec<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    #[validate(
        custom(function = "crate::util::validate::validate_url"),
        length(max = 2048)
    )]
    pub issues_url: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    #[validate(
        custom(function = "crate::util::validate::validate_url"),
        length(max = 2048)
    )]
    pub source_url: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    #[validate(
        custom(function = "crate::util::validate::validate_url"),
        length(max = 2048)
    )]
    pub wiki_url: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    #[validate(
        custom(function = "crate::util::validate::validate_url"),
        length(max = 2048)
    )]
    pub license_url: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    #[validate(
        custom(function = "crate::util::validate::validate_url"),
        length(max = 2048)
    )]
    pub discord_url: Option<Option<String>>,
    #[validate]
    pub donation_urls: Option<Vec<DonationLink>>,
    pub license_id: Option<String>,
    #[validate(
        length(min = 3, max = 64),
        regex = "crate::util::validate::RE_URL_SAFE"
    )]
    pub slug: Option<String>,
    pub status: Option<ProjectStatus>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    pub requested_status: Option<Option<ProjectStatus>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    #[validate(length(max = 2000))]
    pub moderation_message: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    #[validate(length(max = 65536))]
    pub moderation_message_body: Option<Option<String>>,
    pub monetization_status: Option<MonetizationStatus>,
}

pub async fn project_edit(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    config: web::Data<SearchConfig>,
    new_project: web::Json<EditProject>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::PROJECT_WRITE]),
    )
    .await?
    .1;

    new_project
        .validate()
        .map_err(|err| ApiError::Validation(validation_errors_to_string(err, None)))?;

    let string = info.into_inner().0;
    let result = db_models::Project::get(&string, &**pool, &redis).await?;

    if let Some(project_item) = result {
        let id = project_item.inner.id;

        let (team_member, organization_team_member) =
            db_models::TeamMember::get_for_project_permissions(
                &project_item.inner,
                user.id.into(),
                &**pool,
            )
            .await?;

        let permissions = ProjectPermissions::get_permissions_by_role(
            &user.role,
            &team_member,
            &organization_team_member,
        );

        if let Some(perms) = permissions {
            let mut transaction = pool.begin().await?;

            if let Some(title) = &new_project.title {
                if !perms.contains(ProjectPermissions::EDIT_DETAILS) {
                    return Err(ApiError::CustomAuthentication(
                        "You do not have the permissions to edit the title of this project!"
                            .to_string(),
                    ));
                }

                sqlx::query!(
                    "
                    UPDATE mods
                    SET title = $1
                    WHERE (id = $2)
                    ",
                    title.trim(),
                    id as db_ids::ProjectId,
                )
                .execute(&mut *transaction)
                .await?;
            }

            if let Some(description) = &new_project.description {
                if !perms.contains(ProjectPermissions::EDIT_DETAILS) {
                    return Err(ApiError::CustomAuthentication(
                        "You do not have the permissions to edit the description of this project!"
                            .to_string(),
                    ));
                }

                sqlx::query!(
                    "
                    UPDATE mods
                    SET description = $1
                    WHERE (id = $2)
                    ",
                    description,
                    id as db_ids::ProjectId,
                )
                .execute(&mut *transaction)
                .await?;
            }

            if let Some(status) = &new_project.status {
                if !perms.contains(ProjectPermissions::EDIT_DETAILS) {
                    return Err(ApiError::CustomAuthentication(
                        "You do not have the permissions to edit the status of this project!"
                            .to_string(),
                    ));
                }

                if !(user.role.is_mod()
                    || !project_item.inner.status.is_approved()
                        && status == &ProjectStatus::Processing
                    || project_item.inner.status.is_approved() && status.can_be_requested())
                {
                    return Err(ApiError::CustomAuthentication(
                        "You don't have permission to set this status!".to_string(),
                    ));
                }

                if status == &ProjectStatus::Processing {
                    if project_item.versions.is_empty() {
                        return Err(ApiError::InvalidInput(String::from(
                            "Project submitted for review with no initial versions",
                        )));
                    }

                    sqlx::query!(
                        "
                        UPDATE mods
                        SET moderation_message = NULL, moderation_message_body = NULL, queued = NOW()
                        WHERE (id = $1)
                        ",
                        id as db_ids::ProjectId,
                    )
                    .execute(&mut *transaction)
                    .await?;

                    sqlx::query!(
                        "
                        UPDATE threads
                        SET show_in_mod_inbox = FALSE
                        WHERE id = $1
                        ",
                        project_item.thread_id as db_ids::ThreadId,
                    )
                    .execute(&mut *transaction)
                    .await?;
                }

                if status.is_approved() && !project_item.inner.status.is_approved() {
                    sqlx::query!(
                        "
                        UPDATE mods
                        SET approved = NOW()
                        WHERE id = $1 AND approved IS NULL
                        ",
                        id as db_ids::ProjectId,
                    )
                    .execute(&mut *transaction)
                    .await?;
                }
                if status.is_searchable() && !project_item.inner.webhook_sent {
                    if let Ok(webhook_url) = dotenvy::var("PUBLIC_DISCORD_WEBHOOK") {
                        crate::util::webhook::send_discord_webhook(
                            project_item.inner.id.into(),
                            &pool,
                            &redis,
                            webhook_url,
                            None,
                        )
                        .await
                        .ok();

                        sqlx::query!(
                            "
                            UPDATE mods
                            SET webhook_sent = TRUE
                            WHERE id = $1
                            ",
                            id as db_ids::ProjectId,
                        )
                        .execute(&mut *transaction)
                        .await?;
                    }
                }

                if user.role.is_mod() {
                    if let Ok(webhook_url) = dotenvy::var("MODERATION_DISCORD_WEBHOOK") {
                        crate::util::webhook::send_discord_webhook(
                            project_item.inner.id.into(),
                            &pool,
                            &redis,
                            webhook_url,
                            Some(
                                format!(
                                    "**[{}]({}/user/{})** changed project status from **{}** to **{}**",
                                    user.username,
                                    dotenvy::var("SITE_URL")?,
                                    user.username,
                                    &project_item.inner.status.as_friendly_str(),
                                    status.as_friendly_str(),
                                )
                                .to_string(),
                            ),
                        )
                        .await
                        .ok();
                    }
                }

                if team_member.map(|x| !x.accepted).unwrap_or(true) {
                    let notified_members = sqlx::query!(
                        "
                        SELECT tm.user_id id
                        FROM team_members tm
                        WHERE tm.team_id = $1 AND tm.accepted
                        ",
                        project_item.inner.team_id as db_ids::TeamId
                    )
                    .fetch_many(&mut *transaction)
                    .try_filter_map(|e| async { Ok(e.right().map(|c| db_models::UserId(c.id))) })
                    .try_collect::<Vec<_>>()
                    .await?;

                    NotificationBuilder {
                        body: NotificationBody::StatusChange {
                            project_id: project_item.inner.id.into(),
                            old_status: project_item.inner.status,
                            new_status: *status,
                        },
                    }
                    .insert_many(notified_members, &mut transaction, &redis)
                    .await?;
                }

                ThreadMessageBuilder {
                    author_id: Some(user.id.into()),
                    body: MessageBody::StatusChange {
                        new_status: *status,
                        old_status: project_item.inner.status,
                    },
                    thread_id: project_item.thread_id,
                }
                .insert(&mut transaction)
                .await?;

                sqlx::query!(
                    "
                    UPDATE mods
                    SET status = $1
                    WHERE (id = $2)
                    ",
                    status.as_str(),
                    id as db_ids::ProjectId,
                )
                .execute(&mut *transaction)
                .await?;

                if project_item.inner.status.is_searchable() && !status.is_searchable() {
                    delete_from_index(id.into(), config).await?;
                }
            }

            if let Some(requested_status) = &new_project.requested_status {
                if !perms.contains(ProjectPermissions::EDIT_DETAILS) {
                    return Err(ApiError::CustomAuthentication(
                        "You do not have the permissions to edit the requested status of this project!"
                            .to_string(),
                    ));
                }

                if !requested_status
                    .map(|x| x.can_be_requested())
                    .unwrap_or(true)
                {
                    return Err(ApiError::InvalidInput(String::from(
                        "Specified status cannot be requested!",
                    )));
                }

                sqlx::query!(
                    "
                    UPDATE mods
                    SET requested_status = $1
                    WHERE (id = $2)
                    ",
                    requested_status.map(|x| x.as_str()),
                    id as db_ids::ProjectId,
                )
                .execute(&mut *transaction)
                .await?;
            }

            if perms.contains(ProjectPermissions::EDIT_DETAILS) {
                if new_project.categories.is_some() {
                    sqlx::query!(
                        "
                        DELETE FROM mods_categories
                        WHERE joining_mod_id = $1 AND is_additional = FALSE
                        ",
                        id as db_ids::ProjectId,
                    )
                    .execute(&mut *transaction)
                    .await?;
                }

                if new_project.additional_categories.is_some() {
                    sqlx::query!(
                        "
                        DELETE FROM mods_categories
                        WHERE joining_mod_id = $1 AND is_additional = TRUE
                        ",
                        id as db_ids::ProjectId,
                    )
                    .execute(&mut *transaction)
                    .await?;
                }
            }

            if let Some(categories) = &new_project.categories {
                edit_project_categories(
                    categories,
                    &perms,
                    id as db_ids::ProjectId,
                    false,
                    &mut transaction,
                )
                .await?;
            }

            if let Some(categories) = &new_project.additional_categories {
                edit_project_categories(
                    categories,
                    &perms,
                    id as db_ids::ProjectId,
                    true,
                    &mut transaction,
                )
                .await?;
            }

            if let Some(issues_url) = &new_project.issues_url {
                if !perms.contains(ProjectPermissions::EDIT_DETAILS) {
                    return Err(ApiError::CustomAuthentication(
                        "You do not have the permissions to edit the issues URL of this project!"
                            .to_string(),
                    ));
                }

                sqlx::query!(
                    "
                    UPDATE mods
                    SET issues_url = $1
                    WHERE (id = $2)
                    ",
                    issues_url.as_deref(),
                    id as db_ids::ProjectId,
                )
                .execute(&mut *transaction)
                .await?;
            }

            if let Some(source_url) = &new_project.source_url {
                if !perms.contains(ProjectPermissions::EDIT_DETAILS) {
                    return Err(ApiError::CustomAuthentication(
                        "You do not have the permissions to edit the source URL of this project!"
                            .to_string(),
                    ));
                }

                sqlx::query!(
                    "
                    UPDATE mods
                    SET source_url = $1
                    WHERE (id = $2)
                    ",
                    source_url.as_deref(),
                    id as db_ids::ProjectId,
                )
                .execute(&mut *transaction)
                .await?;
            }

            if let Some(wiki_url) = &new_project.wiki_url {
                if !perms.contains(ProjectPermissions::EDIT_DETAILS) {
                    return Err(ApiError::CustomAuthentication(
                        "You do not have the permissions to edit the wiki URL of this project!"
                            .to_string(),
                    ));
                }

                sqlx::query!(
                    "
                    UPDATE mods
                    SET wiki_url = $1
                    WHERE (id = $2)
                    ",
                    wiki_url.as_deref(),
                    id as db_ids::ProjectId,
                )
                .execute(&mut *transaction)
                .await?;
            }

            if let Some(license_url) = &new_project.license_url {
                if !perms.contains(ProjectPermissions::EDIT_DETAILS) {
                    return Err(ApiError::CustomAuthentication(
                        "You do not have the permissions to edit the license URL of this project!"
                            .to_string(),
                    ));
                }

                sqlx::query!(
                    "
                    UPDATE mods
                    SET license_url = $1
                    WHERE (id = $2)
                    ",
                    license_url.as_deref(),
                    id as db_ids::ProjectId,
                )
                .execute(&mut *transaction)
                .await?;
            }

            if let Some(discord_url) = &new_project.discord_url {
                if !perms.contains(ProjectPermissions::EDIT_DETAILS) {
                    return Err(ApiError::CustomAuthentication(
                        "You do not have the permissions to edit the discord URL of this project!"
                            .to_string(),
                    ));
                }

                sqlx::query!(
                    "
                    UPDATE mods
                    SET discord_url = $1
                    WHERE (id = $2)
                    ",
                    discord_url.as_deref(),
                    id as db_ids::ProjectId,
                )
                .execute(&mut *transaction)
                .await?;
            }

            if let Some(slug) = &new_project.slug {
                if !perms.contains(ProjectPermissions::EDIT_DETAILS) {
                    return Err(ApiError::CustomAuthentication(
                        "You do not have the permissions to edit the slug of this project!"
                            .to_string(),
                    ));
                }

                let slug_project_id_option: Option<u64> = parse_base62(slug).ok();
                if let Some(slug_project_id) = slug_project_id_option {
                    let results = sqlx::query!(
                        "
                        SELECT EXISTS(SELECT 1 FROM mods WHERE id=$1)
                        ",
                        slug_project_id as i64
                    )
                    .fetch_one(&mut *transaction)
                    .await?;

                    if results.exists.unwrap_or(true) {
                        return Err(ApiError::InvalidInput(
                            "Slug collides with other project's id!".to_string(),
                        ));
                    }
                }

                // Make sure the new slug is different from the old one
                // We are able to unwrap here because the slug is always set
                if !slug.eq(&project_item.inner.slug.clone().unwrap_or_default()) {
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
                    UPDATE mods
                    SET slug = LOWER($1)
                    WHERE (id = $2)
                    ",
                    Some(slug),
                    id as db_ids::ProjectId,
                )
                .execute(&mut *transaction)
                .await?;
            }

            if let Some(license) = &new_project.license_id {
                if !perms.contains(ProjectPermissions::EDIT_DETAILS) {
                    return Err(ApiError::CustomAuthentication(
                        "You do not have the permissions to edit the license of this project!"
                            .to_string(),
                    ));
                }

                let mut license = license.clone();

                if license.to_lowercase() == "arr" {
                    license = models::projects::DEFAULT_LICENSE_ID.to_string();
                }

                spdx::Expression::parse(&license).map_err(|err| {
                    ApiError::InvalidInput(format!("Invalid SPDX license identifier: {err}"))
                })?;

                sqlx::query!(
                    "
                    UPDATE mods
                    SET license = $1
                    WHERE (id = $2)
                    ",
                    license,
                    id as db_ids::ProjectId,
                )
                .execute(&mut *transaction)
                .await?;
            }
            if let Some(donations) = &new_project.donation_urls {
                if !perms.contains(ProjectPermissions::EDIT_DETAILS) {
                    return Err(ApiError::CustomAuthentication(
                        "You do not have the permissions to edit the donation links of this project!"
                            .to_string(),
                    ));
                }

                sqlx::query!(
                    "
                    DELETE FROM mods_donations
                    WHERE joining_mod_id = $1
                    ",
                    id as db_ids::ProjectId,
                )
                .execute(&mut *transaction)
                .await?;

                for donation in donations {
                    let platform_id = db_models::categories::DonationPlatform::get_id(
                        &donation.id,
                        &mut *transaction,
                    )
                    .await?
                    .ok_or_else(|| {
                        ApiError::InvalidInput(format!(
                            "Platform {} does not exist.",
                            donation.id.clone()
                        ))
                    })?;

                    sqlx::query!(
                        "
                        INSERT INTO mods_donations (joining_mod_id, joining_platform_id, url)
                        VALUES ($1, $2, $3)
                        ",
                        id as db_ids::ProjectId,
                        platform_id as db_ids::DonationPlatformId,
                        donation.url
                    )
                    .execute(&mut *transaction)
                    .await?;
                }
            }

            if let Some(moderation_message) = &new_project.moderation_message {
                if !user.role.is_mod()
                    && (!project_item.inner.status.is_approved() || moderation_message.is_some())
                {
                    return Err(ApiError::CustomAuthentication(
                        "You do not have the permissions to edit the moderation message of this project!"
                            .to_string(),
                    ));
                }

                sqlx::query!(
                    "
                    UPDATE mods
                    SET moderation_message = $1
                    WHERE (id = $2)
                    ",
                    moderation_message.as_deref(),
                    id as db_ids::ProjectId,
                )
                .execute(&mut *transaction)
                .await?;
            }

            if let Some(moderation_message_body) = &new_project.moderation_message_body {
                if !user.role.is_mod()
                    && (!project_item.inner.status.is_approved()
                        || moderation_message_body.is_some())
                {
                    return Err(ApiError::CustomAuthentication(
                        "You do not have the permissions to edit the moderation message body of this project!"
                            .to_string(),
                    ));
                }

                sqlx::query!(
                    "
                    UPDATE mods
                    SET moderation_message_body = $1
                    WHERE (id = $2)
                    ",
                    moderation_message_body.as_deref(),
                    id as db_ids::ProjectId,
                )
                .execute(&mut *transaction)
                .await?;
            }

            if let Some(body) = &new_project.body {
                if !perms.contains(ProjectPermissions::EDIT_BODY) {
                    return Err(ApiError::CustomAuthentication(
                        "You do not have the permissions to edit the body of this project!"
                            .to_string(),
                    ));
                }

                sqlx::query!(
                    "
                    UPDATE mods
                    SET body = $1
                    WHERE (id = $2)
                    ",
                    body,
                    id as db_ids::ProjectId,
                )
                .execute(&mut *transaction)
                .await?;
            }

            if let Some(monetization_status) = &new_project.monetization_status {
                if !perms.contains(ProjectPermissions::EDIT_DETAILS) {
                    return Err(ApiError::CustomAuthentication(
                        "You do not have the permissions to edit the monetization status of this project!"
                            .to_string(),
                    ));
                }

                if (*monetization_status == MonetizationStatus::ForceDemonetized
                    || project_item.inner.monetization_status
                        == MonetizationStatus::ForceDemonetized)
                    && !user.role.is_mod()
                {
                    return Err(ApiError::CustomAuthentication(
                        "You do not have the permissions to edit the monetization status of this project!"
                            .to_string(),
                    ));
                }

                sqlx::query!(
                    "
                    UPDATE mods
                    SET monetization_status = $1
                    WHERE (id = $2)
                    ",
                    monetization_status.as_str(),
                    id as db_ids::ProjectId,
                )
                .execute(&mut *transaction)
                .await?;
            }

            // check new description and body for links to associated images
            // if they no longer exist in the description or body, delete them
            let checkable_strings: Vec<&str> = vec![&new_project.description, &new_project.body]
                .into_iter()
                .filter_map(|x| x.as_ref().map(|y| y.as_str()))
                .collect();

            let context = ImageContext::Project {
                project_id: Some(id.into()),
            };

            img::delete_unused_images(context, checkable_strings, &mut transaction, &redis).await?;
            db_models::Project::clear_cache(
                project_item.inner.id,
                project_item.inner.slug,
                None,
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

pub async fn edit_project_categories(
    categories: &Vec<String>,
    perms: &ProjectPermissions,
    project_id: db_ids::ProjectId,
    additional: bool,
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<(), ApiError> {
    if !perms.contains(ProjectPermissions::EDIT_DETAILS) {
        let additional_str = if additional { "additional " } else { "" };
        return Err(ApiError::CustomAuthentication(format!(
            "You do not have the permissions to edit the {additional_str}categories of this project!"
        )));
    }

    let mut mod_categories = Vec::new();
    for category in categories {
        let category_id = db_models::categories::Category::get_id(category, &mut *transaction)
            .await?
            .ok_or_else(|| {
                ApiError::InvalidInput(format!("Category {} does not exist.", category.clone()))
            })?;
        mod_categories.push(ModCategory::new(project_id, category_id, additional));
    }
    ModCategory::insert_many(mod_categories, &mut *transaction).await?;

    Ok(())
}

#[get("search")]
pub async fn project_search(
    web::Query(info): web::Query<SearchRequest>,
    config: web::Data<SearchConfig>,
) -> Result<HttpResponse, SearchError> {
    let results = search_for_project(&info, &config).await?;
    Ok(HttpResponse::Ok().json(results))
}

pub async fn delete_from_index(
    id: ProjectId,
    config: web::Data<SearchConfig>,
) -> Result<(), meilisearch_sdk::errors::Error> {
    let client = meilisearch_sdk::client::Client::new(&*config.address, &*config.key);

    let indexes: IndexesResults = client.get_indexes().await?;

    for index in indexes.results {
        index.delete_document(id.to_string()).await?;
    }

    Ok(())
}
