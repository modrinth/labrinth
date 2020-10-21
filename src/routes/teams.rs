use actix_web::{web, HttpResponse};
use sqlx::PgPool;
use crate::models::teams::TeamId;
use crate::routes::ApiError;
use crate::database::models::TeamMember;

#[get("{id}/members")]
pub async fn team_members_get(
    info: web::Path<(TeamId,)>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let members_data = TeamMember::get_from_team(info.0.into(), &**pool)
        .await
        .map_err(|e| ApiError::DatabaseError(e.into()))?;

    let team_members: Vec<crate::models::teams::TeamMember> = members_data
        .into_iter()
        .map(|data| crate::models::teams::TeamMember {
            user_id: data.user_id.into(),
            name: data.name,
            role: data.role
        })
        .collect();

    Ok(HttpResponse::Ok().json(team_members))
}