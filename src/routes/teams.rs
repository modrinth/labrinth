use crate::database::models::TeamMember;
use crate::models::teams::TeamId;
use crate::routes::ApiError;
use actix_web::{get, delete, post, patch, web, HttpResponse};
use sqlx::PgPool;
use crate::models::users::UserId;

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
        })
        .collect();

    Ok(HttpResponse::Ok().json(team_members))
}

#[post("{id}/members")]
pub async fn add_team_member(
    info: web::Path<(TeamId,)>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {

}

#[patch("{id}/members/{user_id}")]
pub async fn edit_team_member(
    info: web::Path<(TeamId, UserId)>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {

}

#[delete("{id}/members/{user_id}")]
pub async fn remove_team_member(
    info: web::Path<(TeamId, UserId)>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {

}