use std::collections::HashMap;

use crate::auth::{filter_authorized_projects, get_user_from_headers};
use crate::database::models::team_item::TeamMember;
use crate::database::models::{generate_organization_id, team_item, Organization};
use crate::models::ids::base62_impl::parse_base62;
use crate::models::organizations::OrganizationId;
use crate::models::pats::Scopes;
use crate::models::teams::{OrganizationPermissions, Permissions, ProjectPermissions};
use crate::queue::session::AuthQueue;
use crate::routes::v2::project_creation::CreateError;
use crate::routes::ApiError;
use crate::util::validate::validation_errors_to_string;
use crate::{database, models};
use actix_web::{delete, get, patch, post, web, HttpRequest, HttpResponse};
use rust_decimal::Decimal;
use serde::Deserialize;
use sqlx::PgPool;
use validator::Validate;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(organizations_get).service(organization_create);
    cfg.service(
        web::scope("organization")
            .service(organization_get)
            .service(organizations_edit)
            .service(organization_delete)
            .service(organization_projects_get)
            .service(organization_projects_add)
            .service(organization_projects_remove)
            .service(super::teams::team_members_get_organization),
    );
}

#[derive(Deserialize, Validate)]
pub struct NewOrganization {
    #[validate(
        length(min = 3, max = 64),
        custom(function = "crate::util::validate::validate_name")
    )]
    pub name: String,
    #[validate(length(min = 3, max = 256))]
    pub description: String,
    #[validate(
        length(min = 3, max = 64),
        regex = "crate::util::validate::RE_URL_SAFE"
    )]
    pub slug: String,
    #[serde(default = "crate::models::teams::ProjectPermissions::default")]
    pub default_project_permissions: ProjectPermissions,
}

#[post("organization")]
pub async fn organization_create(
    req: HttpRequest,
    new_organization: web::Json<NewOrganization>,
    pool: web::Data<PgPool>,
    redis: web::Data<deadpool_redis::Pool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, CreateError> {
    let current_user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::ORGANIZATION_CREATE]),
    )
    .await?
    .1;

    new_organization
        .validate()
        .map_err(|err| CreateError::ValidationError(validation_errors_to_string(err, None)))?;

    let mut transaction = pool.begin().await?;

    // Try slug
    let slug_organization_id_option: Option<OrganizationId> =
        serde_json::from_str(&format!("\"{}\"", new_organization.slug)).ok();

    if let Some(slug_organization_id) = slug_organization_id_option {
        let slug_organization_id: models::ids::OrganizationId = slug_organization_id;
        let results = sqlx::query!(
            "
            SELECT EXISTS(SELECT 1 FROM organizations WHERE id=$1)
            ",
            slug_organization_id.0 as i64
        )
        .fetch_one(&mut *transaction)
        .await
        .map_err(|e| CreateError::DatabaseError(e.into()))?;

        if results.exists.unwrap_or(false) {
            return Err(CreateError::SlugCollision);
        }
    }

    {
        let results = sqlx::query!(
            "
            SELECT EXISTS(SELECT 1 FROM organizations WHERE slug = LOWER($1))
            ",
            new_organization.slug
        )
        .fetch_one(&mut *transaction)
        .await
        .map_err(|e| CreateError::DatabaseError(e.into()))?;

        if results.exists.unwrap_or(false) {
            return Err(CreateError::SlugCollision);
        }
    }

    // Gets the user database data
    let user_data =
        crate::database::models::User::get_id(current_user.id.into(), &mut transaction, &redis)
            .await?
            .ok_or_else(|| CreateError::InvalidInput("User not found".to_owned()))?;

    let organization_id = generate_organization_id(&mut transaction).await?;

    // Create organization managerial team
    let team = team_item::TeamBuilder {
        members: vec![team_item::TeamMemberBuilder {
            user_id: current_user.id.into(),
            role: crate::models::teams::OWNER_ROLE.to_owned(),
            permissions: Some(Permissions::Organization(
                crate::models::teams::OrganizationPermissions::ALL,
            )),
            accepted: true,
            payouts_split: Decimal::ONE_HUNDRED,
            ordering: 0,
        }],
    };
    let team_id = team.insert(&mut transaction).await?;

    // Create organization
    let organization = Organization {
        id: organization_id,
        name: new_organization.name.clone(),
        slug: new_organization.slug.clone(),
        description: new_organization.description.clone(),
        default_project_permissions: new_organization.default_project_permissions,
        team_id,
    };
    organization.clone().insert(&mut transaction).await?;

    transaction.commit().await?;

    // Only member is the owner, the logged in one
    let member_data = TeamMember::get_from_team_full(team_id, &**pool, &redis)
        .await?
        .into_iter()
        .next();
    let members_data = if let Some(member_data) = member_data {
        vec![crate::models::teams::TeamMember::from(
            member_data,
            user_data,
            false,
        )]
    } else {
        vec![]
    };

    let organization = models::organizations::Organization::from(organization, members_data);

    Ok(HttpResponse::Ok().json(organization))
}

#[get("{id}")]
pub async fn organization_get(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<deadpool_redis::Pool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let id = info.into_inner().0;
    let current_user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::ORGANIZATION_READ]),
    )
    .await
    .map(|x| x.1)
    .ok();
    let user_id = current_user.as_ref().map(|x| x.id.into());

    let organization_data = Organization::get(&id, &**pool, &redis).await?;
    if let Some(data) = organization_data {
        let members_data = TeamMember::get_from_team_full(data.team_id, &**pool, &redis).await?;

        let users = crate::database::models::User::get_many_ids(
            &members_data.iter().map(|x| x.user_id).collect::<Vec<_>>(),
            &**pool,
            &redis,
        )
        .await?;
        let logged_in = current_user
            .and_then(|user| {
                members_data
                    .iter()
                    .find(|x| x.user_id == user.id.into() && x.accepted)
            })
            .is_some();
        let team_members: Vec<_> = members_data
            .into_iter()
            .filter(|x| {
                logged_in
                    || x.accepted
                    || user_id
                        .map(|y: crate::database::models::UserId| y == x.user_id)
                        .unwrap_or(false)
            })
            .flat_map(|data| {
                users.iter().find(|x| x.id == data.user_id).map(|user| {
                    crate::models::teams::TeamMember::from(data, user.clone(), !logged_in)
                })
            })
            .collect();

        let organization = models::organizations::Organization::from(data, team_members);
        return Ok(HttpResponse::Ok().json(organization));
    }
    Ok(HttpResponse::NotFound().body(""))
}

#[derive(Deserialize)]
pub struct OrganizationIds {
    pub ids: String,
}
#[get("organizations")]
pub async fn organizations_get(
    req: HttpRequest,
    web::Query(ids): web::Query<OrganizationIds>,
    pool: web::Data<PgPool>,
    redis: web::Data<deadpool_redis::Pool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let ids = serde_json::from_str::<Vec<&str>>(&ids.ids)?;
    let organizations_data = Organization::get_many(&ids, &**pool, &redis).await?;
    let team_ids = organizations_data
        .iter()
        .map(|x| x.team_id)
        .collect::<Vec<_>>();

    let teams_data = TeamMember::get_from_team_full_many(&team_ids, &**pool, &redis).await?;
    let users = crate::database::models::User::get_many_ids(
        &teams_data.iter().map(|x| x.user_id).collect::<Vec<_>>(),
        &**pool,
        &redis,
    )
    .await?;

    let current_user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::ORGANIZATION_READ]),
    )
    .await
    .map(|x| x.1)
    .ok();
    let user_id = current_user.as_ref().map(|x| x.id.into());

    let mut organizations = vec![];

    let mut team_groups = HashMap::new();
    for item in teams_data {
        team_groups.entry(item.team_id).or_insert(vec![]).push(item);
    }

    for data in organizations_data {
        let members_data = team_groups.remove(&data.team_id).unwrap_or(vec![]);
        let logged_in = current_user
            .as_ref()
            .and_then(|user| {
                members_data
                    .iter()
                    .find(|x| x.user_id == user.id.into() && x.accepted)
            })
            .is_some();

        let team_members: Vec<_> = members_data
            .into_iter()
            .filter(|x| {
                logged_in
                    || x.accepted
                    || user_id
                        .map(|y: crate::database::models::UserId| y == x.user_id)
                        .unwrap_or(false)
            })
            .flat_map(|data| {
                users.iter().find(|x| x.id == data.user_id).map(|user| {
                    crate::models::teams::TeamMember::from(data, user.clone(), !logged_in)
                })
            })
            .collect();

        let organization = models::organizations::Organization::from(data, team_members);
        organizations.push(organization);
    }

    Ok(HttpResponse::Ok().json(organizations))
}

#[derive(Deserialize, Validate)]
pub struct OrganizationEdit {
    #[validate(
        length(min = 3, max = 64),
        custom(function = "crate::util::validate::validate_name")
    )]
    pub name: Option<String>,
    #[validate(length(min = 3, max = 256))]
    pub description: Option<String>,
    #[validate(
        length(min = 3, max = 64),
        regex = "crate::util::validate::RE_URL_SAFE"
    )]
    pub slug: Option<String>,
    pub default_project_permissions: Option<ProjectPermissions>,
}

#[patch("{id}")]
pub async fn organizations_edit(
    req: HttpRequest,
    info: web::Path<(String,)>,
    new_organization: web::Json<OrganizationEdit>,
    pool: web::Data<PgPool>,
    redis: web::Data<deadpool_redis::Pool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::ORGANIZATION_WRITE]),
    )
    .await?
    .1;

    new_organization
        .validate()
        .map_err(|err| ApiError::Validation(validation_errors_to_string(err, None)))?;

    let string = info.into_inner().0;
    let result = database::models::Organization::get(&string, &**pool, &redis).await?;
    if let Some(organization_item) = result {
        let id = organization_item.id;

        let team_member = database::models::TeamMember::get_from_user_id(
            organization_item.team_id,
            user.id.into(),
            &**pool,
        )
        .await?;

        let permissions =
            OrganizationPermissions::get_permissions_by_role(&user.role, &team_member);

        if let Some(perms) = permissions {
            let mut transaction = pool.begin().await?;
            if let Some(name) = &new_organization.name {
                if !perms.contains(OrganizationPermissions::EDIT_DETAILS) {
                    return Err(ApiError::CustomAuthentication(
                        "You do not have the permissions to edit the name of this organization!"
                            .to_string(),
                    ));
                }
                sqlx::query!(
                    "
                    UPDATE organizations
                    SET name = $1
                    WHERE (id = $2)
                    ",
                    name.trim(),
                    id as database::models::ids::OrganizationId,
                )
                .execute(&mut *transaction)
                .await?;
            }
            if let Some(description) = &new_organization.description {
                if !perms.contains(OrganizationPermissions::EDIT_DETAILS) {
                    return Err(ApiError::CustomAuthentication(
                        "You do not have the permissions to edit the description of this organization!"
                            .to_string(),
                    ));
                }
                sqlx::query!(
                    "
                    UPDATE organizations
                    SET name = $1
                    WHERE (id = $2)
                    ",
                    description,
                    id as database::models::ids::OrganizationId,
                )
                .execute(&mut *transaction)
                .await?;
            }

            if let Some(default_project_permissions) = &new_organization.default_project_permissions
            {
                if !perms.contains(OrganizationPermissions::EDIT_PROJECT_DEFAULT_PERMISSIONS) {
                    return Err(ApiError::CustomAuthentication(
                        "You do not have the permissions to edit the default project permissions of this organization!"
                            .to_string(),
                    ));
                }
                sqlx::query!(
                    "
                    UPDATE organizations
                    SET default_project_permissions = $1
                    WHERE (id = $2)
                    ",
                    default_project_permissions.bits() as i64,
                    id as database::models::ids::OrganizationId,
                )
                .execute(&mut *transaction)
                .await?;
            }

            if let Some(slug) = &new_organization.slug {
                if !perms.contains(OrganizationPermissions::EDIT_DETAILS) {
                    return Err(ApiError::CustomAuthentication(
                        "You do not have the permissions to edit the slug of this organization!"
                            .to_string(),
                    ));
                }

                let slug_organization_id_option: Option<u64> = parse_base62(slug).ok();
                if let Some(slug_organization_id) = slug_organization_id_option {
                    let results = sqlx::query!(
                        "
                        SELECT EXISTS(SELECT 1 FROM organizations WHERE id=$1)
                        ",
                        slug_organization_id as i64
                    )
                    .fetch_one(&mut *transaction)
                    .await?;

                    if results.exists.unwrap_or(true) {
                        return Err(ApiError::InvalidInput(
                            "Slug collides with other organization's id!".to_string(),
                        ));
                    }
                }

                // Make sure the new slug is different from the old one
                // We are able to unwrap here because the slug is always set
                if !slug.eq(&organization_item.slug.clone()) {
                    let results = sqlx::query!(
                        "
                      SELECT EXISTS(SELECT 1 FROM organizations WHERE slug = LOWER($1))
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
                    UPDATE organizations
                    SET slug = LOWER($1)
                    WHERE (id = $2)
                    ",
                    Some(slug),
                    id as database::models::ids::OrganizationId,
                )
                .execute(&mut *transaction)
                .await?;
            }

            database::models::Organization::clear_cache(
                organization_item.id,
                Some(organization_item.slug),
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

#[delete("{id}")]
pub async fn organization_delete(
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
        Some(&[Scopes::ORGANIZATION_DELETE]),
    )
    .await?
    .1;
    let string = info.into_inner().0;

    let organization = database::models::Organization::get(&string, &**pool, &redis)
        .await?
        .ok_or_else(|| {
            ApiError::InvalidInput("The specified organization does not exist!".to_string())
        })?;

    if !user.role.is_admin() {
        let team_member = database::models::TeamMember::get_from_user_id_organization(
            organization.id,
            user.id.into(),
            &**pool,
        )
        .await
        .map_err(ApiError::Database)?
        .ok_or_else(|| {
            ApiError::InvalidInput("The specified organization does not exist!".to_string())
        })?;

        let permissions =
            OrganizationPermissions::get_permissions_by_role(&user.role, &Some(team_member))
                .unwrap_or_default();

        if !permissions.contains(OrganizationPermissions::DELETE_ORGANIZATION) {
            return Err(ApiError::CustomAuthentication(
                "You don't have permission to delete this organization!".to_string(),
            ));
        }
    }

    let mut transaction = pool.begin().await?;
    let result =
        database::models::Organization::remove(organization.id, &mut transaction, &redis).await?;

    transaction.commit().await?;

    if result.is_some() {
        Ok(HttpResponse::NoContent().body(""))
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
}

#[get("{id}/projects")]
pub async fn organization_projects_get(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<deadpool_redis::Pool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let info = info.into_inner().0;
    let current_user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::ORGANIZATION_READ]),
    )
    .await
    .map(|x| x.1)
    .ok();

    let possible_organization_id: Option<u64> = parse_base62(&info).ok();
    use futures::TryStreamExt;

    let project_ids = sqlx::query!(
        "
        SELECT m.id FROM organizations o
        LEFT JOIN mods m ON m.id = o.id
        WHERE (o.id = $1 AND $1 IS NOT NULL) OR (o.slug = $2 AND $2 IS NOT NULL)
        ",
        possible_organization_id.map(|x| x as i64),
        info
    )
    .fetch_many(&**pool)
    .try_filter_map(|e| async { Ok(e.right().map(|m| crate::database::models::ProjectId(m.id))) })
    .try_collect::<Vec<crate::database::models::ProjectId>>()
    .await?;

    let projects_data =
        crate::database::models::Project::get_many_ids(&project_ids, &**pool, &redis).await?;

    let projects = filter_authorized_projects(projects_data, &current_user, &pool).await?;
    Ok(HttpResponse::Ok().json(projects))
}

#[derive(Deserialize)]
pub struct OrganizationProjectAdd {
    pub project_id: String, // Also allow slug
}
#[post("{id}/projects")]
pub async fn organization_projects_add(
    req: HttpRequest,
    info: web::Path<(String,)>,
    project_info: web::Json<OrganizationProjectAdd>,
    pool: web::Data<PgPool>,
    redis: web::Data<deadpool_redis::Pool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let info = info.into_inner().0;
    let current_user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::PROJECT_WRITE, Scopes::ORGANIZATION_WRITE]),
    )
    .await?
    .1;

    let organization = database::models::Organization::get(&info, &**pool, &redis)
        .await?
        .ok_or_else(|| {
            ApiError::InvalidInput("The specified organization does not exist!".to_string())
        })?;

    let project_item = database::models::Project::get(&project_info.project_id, &**pool, &redis)
        .await?
        .ok_or_else(|| {
            ApiError::InvalidInput("The specified project does not exist!".to_string())
        })?;
    if project_item.inner.organization_id.is_some() {
        return Err(ApiError::InvalidInput(
            "The specified project is already owned by an organization!".to_string(),
        ));
    }

    let project_team_member = database::models::TeamMember::get_from_user_id_project(
        project_item.inner.id,
        current_user.id.into(),
        &**pool,
    )
    .await?
    .ok_or_else(|| ApiError::InvalidInput("You are not a member of this project!".to_string()))?;

    let organization_team_member = database::models::TeamMember::get_from_user_id_organization(
        organization.id,
        current_user.id.into(),
        &**pool,
    )
    .await?
    .ok_or_else(|| {
        ApiError::InvalidInput("You are not a member of this organization!".to_string())
    })?;

    // Require ownership of a project to add it to an organization
    if !current_user.role.is_admin()
        && !project_team_member
            .role
            .eq(crate::models::teams::OWNER_ROLE)
    {
        return Err(ApiError::CustomAuthentication(
            "You need to be an owner of a project to add it to an organization!".to_string(),
        ));
    }

    let permissions = OrganizationPermissions::get_permissions_by_role(
        &current_user.role,
        &Some(organization_team_member),
    )
    .unwrap_or_default();
    if permissions.contains(OrganizationPermissions::ADD_PROJECT) {
        let mut transaction = pool.begin().await?;
        sqlx::query!(
            "
            UPDATE mods
            SET organization_id = $1
            WHERE (id = $2)
            ",
            organization.id as database::models::OrganizationId,
            project_item.inner.id as database::models::ids::ProjectId
        )
        .execute(&mut *transaction)
        .await?;

        transaction.commit().await?;

        database::models::TeamMember::clear_cache(project_item.inner.team_id, &redis).await?;
        database::models::Project::clear_cache(
            project_item.inner.id,
            project_item.inner.slug,
            None,
            &redis,
        )
        .await?;
    } else {
        return Err(ApiError::CustomAuthentication(
            "You do not have permission to add projects to this organization!".to_string(),
        ));
    }
    Ok(HttpResponse::Ok().finish())
}

#[delete("{organization_id}/projects/{project_id}")]
pub async fn organization_projects_remove(
    req: HttpRequest,
    info: web::Path<(String, String)>,
    pool: web::Data<PgPool>,
    redis: web::Data<deadpool_redis::Pool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let (organization_id, project_id) = info.into_inner();
    let current_user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::PROJECT_WRITE, Scopes::ORGANIZATION_WRITE]),
    )
    .await?
    .1;

    let organization = database::models::Organization::get(&organization_id, &**pool, &redis)
        .await?
        .ok_or_else(|| {
            ApiError::InvalidInput("The specified organization does not exist!".to_string())
        })?;

    let project_item = database::models::Project::get(&project_id, &**pool, &redis)
        .await?
        .ok_or_else(|| {
            ApiError::InvalidInput("The specified project does not exist!".to_string())
        })?;

    if !project_item
        .inner
        .organization_id
        .eq(&Some(organization.id))
    {
        return Err(ApiError::InvalidInput(
            "The specified project is not owned by this organization!".to_string(),
        ));
    }

    let organization_team_member = database::models::TeamMember::get_from_user_id_organization(
        organization.id,
        current_user.id.into(),
        &**pool,
    )
    .await?
    .ok_or_else(|| {
        ApiError::InvalidInput("You are not a member of this organization!".to_string())
    })?;

    let permissions = OrganizationPermissions::get_permissions_by_role(
        &current_user.role,
        &Some(organization_team_member),
    )
    .unwrap_or_default();
    if permissions.contains(OrganizationPermissions::REMOVE_PROJECT) {
        let mut transaction = pool.begin().await?;
        sqlx::query!(
            "
            UPDATE mods
            SET organization_id = NULL
            WHERE (id = $1)
            ",
            project_item.inner.id as database::models::ids::ProjectId
        )
        .execute(&mut *transaction)
        .await?;

        transaction.commit().await?;

        database::models::TeamMember::clear_cache(project_item.inner.team_id, &redis).await?;
        database::models::Project::clear_cache(
            project_item.inner.id,
            project_item.inner.slug,
            None,
            &redis,
        )
        .await?;
    } else {
        return Err(ApiError::CustomAuthentication(
            "You do not have permission to add projects to this organization!".to_string(),
        ));
    }
    Ok(HttpResponse::Ok().finish())
}
