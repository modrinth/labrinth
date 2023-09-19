use super::ids::*;
use crate::models::teams::{OrganizationPermissions, Permissions, ProjectPermissions};
use itertools::Itertools;
use redis::cmd;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

const TEAMS_NAMESPACE: &str = "teams";
const DEFAULT_EXPIRY: i64 = 1800;

pub struct TeamBuilder {
    pub members: Vec<TeamMemberBuilder>,
}
pub struct TeamMemberBuilder {
    pub user_id: UserId,
    pub role: String,
    pub permissions: Option<Permissions>,
    pub accepted: bool,
    pub payouts_split: Decimal,
    pub ordering: i64,
}

impl TeamBuilder {
    pub async fn insert(
        self,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<TeamId, super::DatabaseError> {
        let team_id = generate_team_id(&mut *transaction).await?;

        let team = Team { id: team_id };

        sqlx::query!(
            "
            INSERT INTO teams (id)
            VALUES ($1)
            ",
            team.id as TeamId,
        )
        .execute(&mut *transaction)
        .await?;

        for member in self.members {
            let team_member_id = generate_team_member_id(&mut *transaction).await?;
            let (project_permissions, organization_permissions) = match member.permissions {
                Some(Permissions::Project(p)) => (Some(p), None),
                Some(Permissions::Organization(op)) => (None, Some(op)),
                None => (None, None),
            };

            sqlx::query!(
                "
                INSERT INTO team_members (id, team_id, user_id, role, permissions, organization_permissions, accepted, payouts_split, ordering)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                ",
                team_member_id as TeamMemberId,
                team.id as TeamId,
                member.user_id as UserId,
                member.role,
                project_permissions.map(|p|p.bits() as i64),
                organization_permissions.map(|p|p.bits() as i64),
                member.accepted,
                member.payouts_split,
                member.ordering,
            )
            .execute(&mut *transaction)
            .await?;
        }

        Ok(team_id)
    }
}

/// A team of users who control a project
pub struct Team {
    /// The id of the team
    pub id: TeamId,
}

pub enum TeamAssociationId {
    Project(ProjectId),
    Organization(OrganizationId),
}

impl Team {
    pub async fn get_team_association<'a, 'b, E>(
        id: TeamId,
        executor: E,
    ) -> Result<Option<TeamAssociationId>, super::DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        let result = sqlx::query!(
            "
            SELECT m.id AS pid, NULL AS oid, m.team_id AS team
            FROM mods m
            WHERE m.team_id = ANY($1)
            
            UNION ALL
                    
            SELECT NULL AS pid, o.id AS oid, o.team_id AS team
            FROM organizations o
            WHERE o.team_id = ANY($1)
    ",
            id as TeamId
        )
        .fetch_optional(executor)
        .await?;

        if let Some(t) = result {
            if let Some(pid) = t.pid {
                return Ok(Some(TeamAssociationId::Project(ProjectId(pid))));
            }
            if let Some(oid) = t.oid {
                return Ok(Some(TeamAssociationId::Organization(OrganizationId(oid))));
            }
        }
        Ok(None)
    }
}

/// A member of a team
#[derive(Deserialize, Serialize, Clone)]
pub struct TeamMember {
    pub id: TeamMemberId,
    pub team_id: TeamId,
    /// The ID of the user associated with the member
    pub user_id: UserId,
    pub role: String,
    // Only one of these should be set
    pub permissions: Option<ProjectPermissions>,
    pub organization_permissions: Option<OrganizationPermissions>,
    pub accepted: bool,
    pub payouts_split: Decimal,
    pub ordering: i64,
}

impl TeamMember {
    // Lists the full members of a team
    pub async fn get_from_team_full<'a, 'b, E>(
        id: TeamId,
        executor: E,
        redis: &deadpool_redis::Pool,
    ) -> Result<Vec<TeamMember>, super::DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres> + Copy,
    {
        Self::get_from_team_full_many(&[id], executor, redis).await
    }

    pub async fn get_from_team_full_many<'a, E>(
        team_ids: &[TeamId],
        exec: E,
        redis: &deadpool_redis::Pool,
    ) -> Result<Vec<TeamMember>, super::DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres> + Copy,
    {
        if team_ids.is_empty() {
            return Ok(Vec::new());
        }

        use futures::stream::TryStreamExt;

        let mut team_ids_parsed: Vec<i64> = team_ids.iter().map(|x| x.0).collect();

        let mut redis = redis.get().await?;

        let mut found_teams = Vec::new();

        let teams = cmd("MGET")
            .arg(
                team_ids_parsed
                    .iter()
                    .map(|x| format!("{}:{}", TEAMS_NAMESPACE, x))
                    .collect::<Vec<_>>(),
            )
            .query_async::<_, Vec<Option<String>>>(&mut redis)
            .await?;

        for team_raw in teams {
            if let Some(mut team) = team_raw
                .clone()
                .and_then(|x| serde_json::from_str::<Vec<TeamMember>>(&x).ok())
            {
                if let Some(team_id) = team.first().map(|x| x.team_id) {
                    team_ids_parsed.retain(|x| &team_id.0 != x);
                }

                found_teams.append(&mut team);
                continue;
            }
        }

        if !team_ids_parsed.is_empty() {
            let teams: Vec<TeamMember> = sqlx::query!(
                "
                SELECT tm.id, tm.team_id, tm.role AS member_role, tm.permissions, 
                    tm.organization_permissions, tm.accepted, tm.payouts_split, 
                    tm.ordering, tm.user_id
                FROM team_members tm
                WHERE tm.team_id = ANY($1)
                ORDER BY tm.team_id, tm.ordering;
                ",
                &team_ids_parsed
            )
            .fetch_many(exec)
            .try_filter_map(|e| async {
                Ok(e.right().map(|m| TeamMember {
                    id: TeamMemberId(m.id),
                    team_id: TeamId(m.team_id),
                    role: m.member_role,
                    permissions: m
                        .permissions
                        .map(|p| ProjectPermissions::from_bits(p as u64).unwrap_or_default()),
                    organization_permissions: m.organization_permissions.map(|op| {
                        OrganizationPermissions::from_bits(op as u64).unwrap_or_default()
                    }),
                    accepted: m.accepted,
                    user_id: UserId(m.user_id),
                    payouts_split: m.payouts_split,
                    ordering: m.ordering,
                }))
            })
            .try_collect::<Vec<TeamMember>>()
            .await?;

            for (id, members) in &teams.into_iter().group_by(|x| x.team_id) {
                let mut members = members.collect::<Vec<_>>();

                cmd("SET")
                    .arg(format!("{}:{}", TEAMS_NAMESPACE, id.0))
                    .arg(serde_json::to_string(&members)?)
                    .arg("EX")
                    .arg(DEFAULT_EXPIRY)
                    .query_async::<_, ()>(&mut redis)
                    .await?;

                found_teams.append(&mut members);
            }
        }

        Ok(found_teams)
    }

    pub async fn clear_cache(
        id: TeamId,
        redis: &deadpool_redis::Pool,
    ) -> Result<(), super::DatabaseError> {
        let mut redis = redis.get().await?;
        cmd("DEL")
            .arg(format!("{}:{}", TEAMS_NAMESPACE, id.0))
            .query_async::<_, ()>(&mut redis)
            .await?;

        Ok(())
    }

    /// Gets a team member from a user id and team id.  Does not return pending members.
    pub async fn get_from_user_id<'a, 'b, E>(
        id: TeamId,
        user_id: UserId,
        executor: E,
    ) -> Result<Option<Self>, super::DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        Self::get_from_user_id_many(&[id], user_id, executor)
            .await
            .map(|x| x.into_iter().next())
    }

    /// Gets team members from user ids and team ids.  Does not return pending members.
    pub async fn get_from_user_id_many<'a, 'b, E>(
        team_ids: &[TeamId],
        user_id: UserId,
        executor: E,
    ) -> Result<Vec<Self>, super::DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        use futures::stream::TryStreamExt;

        let team_ids_parsed: Vec<i64> = team_ids.iter().map(|x| x.0).collect();

        let team_members = sqlx::query!(
            "
            SELECT tm.id, tm.team_id, tm.role AS member_role, tm.permissions, 
                tm.organization_permissions, tm.accepted, tm.payouts_split, tm.role,
                tm.ordering, tm.user_id
            FROM team_members tm
            WHERE (tm.team_id = ANY($1) AND tm.user_id = $2 AND tm.accepted = TRUE)
            ORDER BY ordering
            ",
            &team_ids_parsed,
            user_id as UserId
        )
        .fetch_many(executor)
        .try_filter_map(|e| async {
            if let Some(m) = e.right() {
                Ok(Some(Ok(TeamMember {
                    id: TeamMemberId(m.id),
                    team_id: TeamId(m.team_id),
                    user_id,
                    role: m.role,
                    permissions: m
                        .permissions
                        .map(|p| ProjectPermissions::from_bits(p as u64).unwrap_or_default()),
                    organization_permissions: m.organization_permissions.map(|op| {
                        OrganizationPermissions::from_bits(op as u64).unwrap_or_default()
                    }),
                    accepted: m.accepted,
                    payouts_split: m.payouts_split,
                    ordering: m.ordering,
                })))
            } else {
                Ok(None)
            }
        })
        .try_collect::<Vec<Result<TeamMember, super::DatabaseError>>>()
        .await?;

        let team_members = team_members
            .into_iter()
            .collect::<Result<Vec<TeamMember>, super::DatabaseError>>()?;

        Ok(team_members)
    }

    /// Gets a team member from a user id and team id, including pending members.
    pub async fn get_from_user_id_pending<'a, 'b, E>(
        id: TeamId,
        user_id: UserId,
        executor: E,
    ) -> Result<Option<Self>, super::DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        let result = sqlx::query!(
            "
            SELECT tm.id, tm.team_id, tm.role AS member_role, tm.permissions, 
                tm.organization_permissions, tm.accepted, tm.payouts_split, tm.role,
                tm.ordering, tm.user_id
                
            FROM team_members tm
            WHERE (tm.team_id = $1 AND tm.user_id = $2)
            ORDER BY ordering
            ",
            id as TeamId,
            user_id as UserId
        )
        .fetch_optional(executor)
        .await?;

        if let Some(m) = result {
            Ok(Some(TeamMember {
                id: TeamMemberId(m.id),
                team_id: id,
                user_id,
                role: m.role,
                permissions: m
                    .permissions
                    .map(|p| ProjectPermissions::from_bits(p as u64).unwrap_or_default()),
                organization_permissions: m
                    .organization_permissions
                    .map(|op| OrganizationPermissions::from_bits(op as u64).unwrap_or_default()),
                accepted: m.accepted,
                payouts_split: m.payouts_split,
                ordering: m.ordering,
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn insert(
        &self,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), sqlx::error::Error> {
        sqlx::query!(
            "
            INSERT INTO team_members (
                id, team_id, user_id, role, permissions, organization_permissions, accepted
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, $7
            )
            ",
            self.id as TeamMemberId,
            self.team_id as TeamId,
            self.user_id as UserId,
            self.role,
            self.permissions.map(|p| p.bits() as i64),
            self.organization_permissions.map(|op| op.bits() as i64),
            self.accepted,
        )
        .execute(&mut *transaction)
        .await?;

        Ok(())
    }

    pub async fn delete<'a, 'b>(
        id: TeamId,
        user_id: UserId,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), super::DatabaseError> {
        sqlx::query!(
            "
            DELETE FROM team_members
            WHERE (team_id = $1 AND user_id = $2 AND NOT role = $3)
            ",
            id as TeamId,
            user_id as UserId,
            crate::models::teams::OWNER_ROLE,
        )
        .execute(&mut *transaction)
        .await?;

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn edit_team_member(
        id: TeamId,
        user_id: UserId,
        new_permissions: Option<ProjectPermissions>,
        organization_permissions: Option<OrganizationPermissions>,
        new_role: Option<String>,
        new_accepted: Option<bool>,
        new_payouts_split: Option<Decimal>,
        new_ordering: Option<i64>,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), super::DatabaseError> {
        if let Some(permissions) = new_permissions {
            sqlx::query!(
                "
                UPDATE team_members
                SET permissions = $1
                WHERE (team_id = $2 AND user_id = $3)
                ",
                permissions.bits() as i64,
                id as TeamId,
                user_id as UserId,
            )
            .execute(&mut *transaction)
            .await?;
        }

        if let Some(organization_permissions) = organization_permissions {
            sqlx::query!(
                "
                UPDATE team_members
                SET organization_permissions = $1
                WHERE (team_id = $2 AND user_id = $3)
                ",
                organization_permissions.bits() as i64,
                id as TeamId,
                user_id as UserId,
            )
            .execute(&mut *transaction)
            .await?;
        }

        if let Some(role) = new_role {
            sqlx::query!(
                "
                UPDATE team_members
                SET role = $1
                WHERE (team_id = $2 AND user_id = $3)
                ",
                role,
                id as TeamId,
                user_id as UserId,
            )
            .execute(&mut *transaction)
            .await?;
        }

        if let Some(accepted) = new_accepted {
            if accepted {
                sqlx::query!(
                    "
                    UPDATE team_members
                    SET accepted = TRUE
                    WHERE (team_id = $1 AND user_id = $2)
                    ",
                    id as TeamId,
                    user_id as UserId,
                )
                .execute(&mut *transaction)
                .await?;
            }
        }

        if let Some(payouts_split) = new_payouts_split {
            sqlx::query!(
                "
                UPDATE team_members
                SET payouts_split = $1
                WHERE (team_id = $2 AND user_id = $3)
                ",
                payouts_split,
                id as TeamId,
                user_id as UserId,
            )
            .execute(&mut *transaction)
            .await?;
        }

        if let Some(ordering) = new_ordering {
            sqlx::query!(
                "
                UPDATE team_members
                SET ordering = $1
                WHERE (team_id = $2 AND user_id = $3)
                ",
                ordering,
                id as TeamId,
                user_id as UserId,
            )
            .execute(&mut *transaction)
            .await?;
        }

        Ok(())
    }

    pub async fn get_from_user_id_project<'a, 'b, E>(
        id: ProjectId,
        user_id: UserId,
        executor: E,
    ) -> Result<Option<Self>, super::DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        let result = sqlx::query!(
            "
            SELECT tm.id, tm.team_id, tm.user_id, tm.role, tm.permissions, tm.organization_permissions, tm.accepted, tm.payouts_split, tm.ordering FROM mods m
            INNER JOIN team_members tm ON tm.team_id = m.team_id AND user_id = $2 AND accepted = TRUE
            WHERE m.id = $1
            ",
            id as ProjectId,
            user_id as UserId
        )
            .fetch_optional(executor)
            .await?;

        if let Some(m) = result {
            Ok(Some(TeamMember {
                id: TeamMemberId(m.id),
                team_id: TeamId(m.team_id),
                user_id,
                role: m.role,
                permissions: m
                    .permissions
                    .map(|p| ProjectPermissions::from_bits(p as u64).unwrap_or_default()),
                organization_permissions: m
                    .organization_permissions
                    .map(|op| OrganizationPermissions::from_bits(op as u64).unwrap_or_default()),
                accepted: m.accepted,
                payouts_split: m.payouts_split,
                ordering: m.ordering,
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn get_from_user_id_organization<'a, 'b, E>(
        id: OrganizationId,
        user_id: UserId,
        executor: E,
    ) -> Result<Option<Self>, super::DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        let result = sqlx::query!(
            "
            SELECT tm.id, tm.team_id, tm.user_id, tm.role, tm.permissions, tm.organization_permissions, tm.accepted, tm.payouts_split, tm.ordering FROM organizations o
            INNER JOIN team_members tm ON tm.team_id = o.team_id AND user_id = $2 AND accepted = TRUE
            WHERE o.id = $1
            ",
            id as OrganizationId,
            user_id as UserId
        )
            .fetch_optional(executor)
            .await?;

        if let Some(m) = result {
            Ok(Some(TeamMember {
                id: TeamMemberId(m.id),
                team_id: TeamId(m.team_id),
                user_id,
                role: m.role,
                permissions: m
                    .permissions
                    .map(|p| ProjectPermissions::from_bits(p as u64).unwrap_or_default()),
                organization_permissions: m
                    .organization_permissions
                    .map(|op| OrganizationPermissions::from_bits(op as u64).unwrap_or_default()),
                accepted: m.accepted,
                payouts_split: m.payouts_split,
                ordering: m.ordering,
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn get_from_user_id_version<'a, 'b, E>(
        id: VersionId,
        user_id: UserId,
        executor: E,
    ) -> Result<Option<Self>, super::DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        let result = sqlx::query!(
            "
            SELECT tm.id, tm.team_id, tm.user_id, tm.role, tm.permissions, tm.organization_permissions, tm.accepted, tm.payouts_split, tm.ordering, v.mod_id FROM versions v
            INNER JOIN mods m ON m.id = v.mod_id
            INNER JOIN team_members tm ON tm.team_id = m.team_id AND tm.user_id = $2 AND tm.accepted = TRUE
            WHERE v.id = $1
            ",
            id as VersionId,
            user_id as UserId
        )
            .fetch_optional(executor)
            .await?;

        if let Some(m) = result {
            Ok(Some(TeamMember {
                id: TeamMemberId(m.id),
                team_id: TeamId(m.team_id),
                user_id,
                role: m.role,
                permissions: m
                    .permissions
                    .map(|p| ProjectPermissions::from_bits(p as u64).unwrap_or_default()),
                organization_permissions: m
                    .organization_permissions
                    .map(|op| OrganizationPermissions::from_bits(op as u64).unwrap_or_default()),
                accepted: m.accepted,
                payouts_split: m.payouts_split,
                ordering: m.ordering,
            }))
        } else {
            Ok(None)
        }
    }
}
