use crate::database::models::TeamMember;
use crate::models::teams::TeamId;
use crate::routes::ApiError;
use actix_web::{get, delete, post, patch, web, HttpResponse, HttpRequest};
use sqlx::PgPool;
use crate::models::users::UserId;
use crate::auth::get_user_from_headers;
use serde::{Deserialize, Serialize};

#[get("{id}/members")]
pub async fn team_members_get(
    info: web::Path<(TeamId,)>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let id = info.into_inner().0;
    let members_data = TeamMember::get_from_team(id.into(), &**pool).await?;

    let team_members: Vec<crate::models::teams::TeamMember> = members_data
        .into_iter()
        .map(|data| crate::models::teams::TeamMember {
            user_id: data.user_id.into(),
            name: data.name,
            role: data.role,
            permissions: data.permissions as u64
        })
        .collect();

    Ok(HttpResponse::Ok().json(team_members))
}

#[post("{id}/join")]
pub async fn join_team(
    req: HttpRequest,
    info: web::Path<(TeamId,)>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let team_id = info.into_inner().0.into();
    let current_user = get_user_from_headers(req.headers(), &**pool).await.map_err(|_| ApiError::AuthenticationError)?;

    TeamMember::edit_team_member(team_id, current_user.id.into(), None, None, Some(true), &**pool).await?;

    Ok(HttpResponse::Ok().body(""))
}

#[post("{id}/members")]
pub async fn add_team_member(
    req: HttpRequest,
    info: web::Path<(TeamId,)>,
    pool: web::Data<PgPool>,
    new_member: web::Json<crate::models::teams::TeamMember>,
) -> Result<HttpResponse, ApiError> {
    let team_id = info.into_inner().0.into();

    let mut transaction = pool.begin().await.map_err(|e| ApiError::DatabaseError(e.into()))?;

    let current_user = get_user_from_headers(req.headers(), &**pool).await.map_err(|_| ApiError::AuthenticationError)?;
    let team_member = TeamMember::get_from_user_id(team_id, current_user.id.into(), &**pool).await?;

    if let Some(member) = team_member {
        if (member.permissions & (1 << 4)) != 0 && new_member.role != crate::models::teams::OWNER_ROLE {
            // TODO: Prevent user from giving another user permissions they do not have
            let new_id = crate::database::models::ids::generate_team_member_id(&mut transaction).await?;
            TeamMember {
                id: new_id,
                team_id,
                user_id: new_member.user_id.into(),
                name: new_member.name.clone(),
                role: new_member.role.clone(),
                permissions: new_member.permissions as i64,
                accepted: false
            }.insert(&mut transaction).await.map_err(|e| ApiError::DatabaseError(e.into()))?;

            transaction.commit().await.map_err(|e| ApiError::DatabaseError(e.into()))?;

            Ok(HttpResponse::Ok().body(""))
        } else {
            Err(ApiError::AuthenticationError)
        }
    } else {
        Err(ApiError::AuthenticationError)
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct EditTeamMember {
    pub permissions: Option<u64>,
    pub role: Option<String>,
}

#[patch("{id}/members/{user_id}")]
pub async fn edit_team_member(
    req: HttpRequest,
    info: web::Path<(TeamId, UserId)>,
    pool: web::Data<PgPool>,
    edit_member: web::Json<EditTeamMember>,
) -> Result<HttpResponse, ApiError> {
    let ids = info.into_inner();
    let id = ids.0.into();
    let user_id = ids.1.into();

    let current_user = get_user_from_headers(req.headers(), &**pool).await.map_err(|_| ApiError::AuthenticationError)?;
    let team_member = TeamMember::get_from_user_id(id, current_user.id.into(), &**pool).await?;

    if let Some(member) = team_member {
        if (member.permissions & (1 << 6)) != 0 && edit_member.role.clone().unwrap_or("".to_string()) != "Owner".to_string() {
            // TODO: Prevent user from giving another user permissions they do not have
            TeamMember::edit_team_member(id, user_id, edit_member.permissions.map(|x| x as i64), edit_member.role.clone(), None, &**pool).await?;

            Ok(HttpResponse::Ok().body(""))
        } else {
            Err(ApiError::AuthenticationError)
        }
    } else {
        Err(ApiError::AuthenticationError)
    }
}

#[delete("{id}/members/{user_id}")]
pub async fn remove_team_member(
    req: HttpRequest,
    info: web::Path<(TeamId, UserId)>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let ids = info.into_inner();
    let id = ids.0.into();
    let user_id = ids.1.into();

    let current_user = get_user_from_headers(req.headers(), &**pool).await.map_err(|_| ApiError::AuthenticationError)?;
    let team_member = TeamMember::get_from_user_id(id, current_user.id.into(), &**pool).await?;

    if let Some(member) = team_member {
        let delete_member_option = TeamMember::get_from_user_id(id, user_id, &**pool).await?;

        if let Some(delete_member) = delete_member_option {
            if delete_member.accepted {
                if (member.permissions & (1 << 5)) != 0 {
                    TeamMember::delete(id, user_id, &**pool).await?;
                    Ok(HttpResponse::Ok().body(""))
                } else {
                    Err(ApiError::AuthenticationError)
                }
            } else {
                if (member.permissions & (1 << 4)) != 0 {
                    TeamMember::delete(id, user_id, &**pool).await?;
                    Ok(HttpResponse::Ok().body(""))
                } else {
                    Err(ApiError::AuthenticationError)
                }
            }
        } else {
            Ok(HttpResponse::NotFound().body(""))
        }
    } else {
        Err(ApiError::AuthenticationError)
    }
}