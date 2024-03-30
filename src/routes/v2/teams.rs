use std::net::SocketAddr;
use std::sync::Arc;

use crate::database::redis::RedisPool;
use crate::models::teams::{OrganizationPermissions, ProjectPermissions, TeamId};
use crate::models::users::UserId;
use crate::models::v2::teams::LegacyTeamMember;
use crate::queue::session::AuthQueue;
use crate::routes::{v3, ApiErrorV2};
use crate::util::extract::{ConnectInfo, Extension, Json, Path, Query};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{delete, get, patch, post};
use axum::Router;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

pub fn config() -> Router {
    Router::new().route("/teams", get(teams_get)).nest(
        "/team",
        Router::new()
            .route("/:id/join", post(join_team))
            .route("/:id/owner", patch(transfer_ownership))
            .route("/:id/members", get(team_members_get).post(add_team_member))
            .route(
                "/:id/members/:user_id",
                delete(remove_team_member).patch(edit_team_member),
            ),
    )
}

// Returns all members of a project,
// including the team members of the project's team, but
// also the members of the organization's team if the project is associated with an organization
// (Unlike team_members_get_project, which only returns the members of the project's team)
// They can be differentiated by the "organization_permissions" field being null or not
pub async fn team_members_get_project(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<Json<Vec<LegacyTeamMember>>, ApiErrorV2> {
    let Json(members) = v3::teams::team_members_get_project(
        ConnectInfo(addr),
        headers,
        Path(info),
        Extension(pool),
        Extension(redis),
        Extension(session_queue),
    )
    .await?;
    // Convert response to V2 format
    let members = members
        .into_iter()
        .map(LegacyTeamMember::from)
        .collect::<Vec<_>>();
    Ok(Json(members))
}

// Returns all members of a team, but not necessarily those of a project-team's organization (unlike team_members_get_project)
pub async fn team_members_get(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    info: Path<TeamId>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<Json<Vec<LegacyTeamMember>>, ApiErrorV2> {
    let Json(members) = v3::teams::team_members_get(
        ConnectInfo(addr),
        headers,
        info,
        Extension(pool),
        Extension(redis),
        Extension(session_queue),
    )
    .await?;

    // Convert response to V2 format
    let members = members
        .into_iter()
        .map(LegacyTeamMember::from)
        .collect::<Vec<_>>();

    Ok(Json(members))
}

#[derive(Serialize, Deserialize)]
pub struct TeamIds {
    pub ids: String,
}

pub async fn teams_get(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(ids): Query<TeamIds>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<Json<Vec<Vec<LegacyTeamMember>>>, ApiErrorV2> {
    let Json(teams) = v3::teams::teams_get(
        ConnectInfo(addr),
        headers,
        Query(v3::teams::TeamIds { ids: ids.ids }),
        Extension(pool),
        Extension(redis),
        Extension(session_queue),
    )
    .await?;

    // Convert response to V2 format
    let teams = teams
        .into_iter()
        .map(|members| {
            members
                .into_iter()
                .map(LegacyTeamMember::from)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    Ok(Json(teams))
}

pub async fn join_team(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    info: Path<TeamId>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<StatusCode, ApiErrorV2> {
    Ok(v3::teams::join_team(
        ConnectInfo(addr),
        headers,
        info,
        Extension(pool),
        Extension(redis),
        Extension(session_queue),
    )
    .await?)
}

fn default_role() -> String {
    "Member".to_string()
}

fn default_ordering() -> i64 {
    0
}

#[derive(Serialize, Deserialize, Clone)]
pub struct NewTeamMember {
    pub user_id: UserId,
    #[serde(default = "default_role")]
    pub role: String,
    #[serde(default)]
    pub permissions: ProjectPermissions,
    #[serde(default)]
    pub organization_permissions: Option<OrganizationPermissions>,
    #[serde(default)]
    #[serde(with = "rust_decimal::serde::float")]
    pub payouts_split: Decimal,
    #[serde(default = "default_ordering")]
    pub ordering: i64,
}

#[axum::debug_handler]
pub async fn add_team_member(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<TeamId>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
    Json(new_member): Json<NewTeamMember>,
) -> Result<StatusCode, ApiErrorV2> {
    Ok(v3::teams::add_team_member(
        ConnectInfo(addr),
        headers,
        Path(info),
        Extension(pool),
        Extension(redis),
        Extension(session_queue),
        Json(v3::teams::NewTeamMember {
            user_id: new_member.user_id,
            role: new_member.role.clone(),
            permissions: new_member.permissions,
            organization_permissions: new_member.organization_permissions,
            payouts_split: new_member.payouts_split,
            ordering: new_member.ordering,
        }),
    )
    .await?)
}

#[derive(Serialize, Deserialize, Clone)]
pub struct EditTeamMember {
    pub permissions: Option<ProjectPermissions>,
    pub organization_permissions: Option<OrganizationPermissions>,
    pub role: Option<String>,
    pub payouts_split: Option<Decimal>,
    pub ordering: Option<i64>,
}

pub async fn edit_team_member(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<(TeamId, UserId)>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
    Json(edit_member): Json<EditTeamMember>,
) -> Result<StatusCode, ApiErrorV2> {
    Ok(v3::teams::edit_team_member(
        ConnectInfo(addr),
        headers,
        Path(info),
        Extension(pool),
        Extension(redis),
        Extension(session_queue),
        Json(v3::teams::EditTeamMember {
            permissions: edit_member.permissions,
            organization_permissions: edit_member.organization_permissions,
            role: edit_member.role,
            payouts_split: edit_member.payouts_split,
            ordering: edit_member.ordering,
        }),
    )
    .await?)
}

#[derive(Deserialize)]
pub struct TransferOwnership {
    pub user_id: UserId,
}

pub async fn transfer_ownership(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<TeamId>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
    Json(new_owner): Json<TransferOwnership>,
) -> Result<StatusCode, ApiErrorV2> {
    Ok(v3::teams::transfer_ownership(
        ConnectInfo(addr),
        headers,
        Path(info),
        Extension(pool),
        Extension(redis),
        Extension(session_queue),
        Json(v3::teams::TransferOwnership {
            user_id: new_owner.user_id,
        }),
    )
    .await?)
}

pub async fn remove_team_member(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<(TeamId, UserId)>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<StatusCode, ApiErrorV2> {
    Ok(v3::teams::remove_team_member(
        ConnectInfo(addr),
        headers,
        Path(info),
        Extension(pool),
        Extension(redis),
        Extension(session_queue),
    )
    .await?)
}
