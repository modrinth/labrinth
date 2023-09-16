use std::collections::HashMap;

use crate::auth::get_user_from_headers;
use crate::database::models::team_item::TeamMember;
use crate::database::models::{Organization, generate_organization_id, team_item};
use crate::models;
use crate::models::pats::Scopes;
use crate::models::organizations::OrganizationId;
use crate::models::teams::{Permissions, OrganizationPermissions, ProjectPermissions};
use crate::queue::session::AuthQueue;
use crate::routes::ApiError;
use actix_web::{get, post, web, HttpRequest, HttpResponse};
use rust_decimal::Decimal;
use serde::Deserialize;
use sqlx::PgPool;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(organizations_get).service(organization_create);
    cfg.service(
        web::scope("organization")
        .service(organization_get)
    );
}

#[derive(Deserialize)]
pub struct NewOrganization {
    pub name: String,
    pub description: String,
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
    ) -> Result<HttpResponse, ApiError> {

    let current_user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::PROJECT_WRITE]),
    ).await?.1;

    let mut transaction = pool.begin().await?;

    // Gets the user database data
    let user_data = crate::database::models::User::get_id(current_user.id.into(), &mut transaction, &redis).await?.ok_or_else(|| ApiError::InvalidInput("User not found".to_owned()))?;

    let organization_id = generate_organization_id(&mut transaction).await?;

    // Create organization managerial team
    let team = team_item::TeamBuilder {
        members: vec![team_item::TeamMemberBuilder {
            user_id: current_user.id.into(),
            role: crate::models::teams::OWNER_ROLE.to_owned(),
            permissions: Some(Permissions::Organization(crate::models::teams::OrganizationPermissions::ALL)),
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
        default_project_permissions: new_organization.default_project_permissions.clone(),
        team_id,
    };
    organization.clone().insert(&mut transaction).await?;
    transaction.commit().await?;

    // Only member is the owner, the logged in one
    let member_data = TeamMember::get_from_team_full(team_id, &**pool, &redis).await?.into_iter().next();
    let members_data = if let Some(member_data) = member_data {
        vec![crate::models::teams::TeamMember::from(member_data, user_data, false)]
    } else {
        vec![]
    };
    
    let organization = models::organizations::Organization::from(organization, members_data);

    Ok(HttpResponse::Ok().json(organization))
}

#[get("{id}")]
pub async fn organization_get(
    req: HttpRequest,
    info: web::Path<(OrganizationId,)>,
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
        Some(&[Scopes::PROJECT_READ]),
    )
    .await
    .map(|x| x.1)
    .ok();
    let user_id = current_user.as_ref().map(|x| x.id.into());

    println!("Getting organization {}", id.0);

    let organization_data = Organization::get_id(id.into(), &**pool, &redis).await?;
    if let Some(data) = organization_data {
        println!("Found organization {}", id.0);

        let members_data = TeamMember::get_from_team_full(data.team_id, &**pool, &redis).await?;

        let users = crate::database::models::User::get_many_ids(
            &members_data.iter().map(|x| x.user_id).collect::<Vec<_>>(),
            &**pool,
            &redis,
        )
        .await?;
        println!("Found {} users", users.len());
        let logged_in = current_user
            .and_then(|user| {
                members_data
                    .iter()
                    .find(|x| x.user_id == user.id.into() && x.accepted)
            })
            .is_some();
            println!("Logged in: {}", logged_in);
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
                users
                    .iter()
                    .find(|x| x.id == data.user_id)
                    .map(|user| crate::models::teams::TeamMember::from(data, user.clone(), !logged_in))
            })
            .collect();
            println!("Found {} team members", team_members.len());

        let organization = models::organizations::Organization::from(data, team_members);
        return Ok(HttpResponse::Ok().json(organization));
    }
println!("Not found organization {}", id.0);
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

    let organization_ids = serde_json::from_str::<Vec<OrganizationId>>(&ids.ids)?
        .into_iter()
        .map(|x| x.into())
        .collect::<Vec<crate::database::models::ids::OrganizationId>>();

    let organizations_data = Organization::get_many_ids(&organization_ids, &**pool, &redis).await?;
    let team_ids = organizations_data.iter().map(|x| x.team_id).collect::<Vec<_>>();


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
        Some(&[Scopes::PROJECT_READ]),
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
        let logged_in = current_user.as_ref()
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
                users
                    .iter()
                    .find(|x| x.id == data.user_id)
                    .map(|user| crate::models::teams::TeamMember::from(data, user.clone(), !logged_in))
            })
            .collect();


        let organization = models::organizations::Organization::from(data, team_members);
        organizations.push(organization);
    }

    Ok(HttpResponse::Ok().json(organizations))
}
