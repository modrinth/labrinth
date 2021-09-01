use super::ids::*;
use crate::database::models::User;
use crate::models::teams::Permissions;

pub struct TeamBuilder {
    pub members: Vec<TeamMemberBuilder>,
}
pub struct TeamMemberBuilder {
    pub user_id: UserId,
    pub role: String,
    pub permissions: Permissions,
    pub accepted: bool,
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
            let team_member = TeamMember {
                id: team_member_id,
                team_id,
                user_id: member.user_id,
                role: member.role,
                permissions: member.permissions,
                accepted: member.accepted,
            };

            sqlx::query!(
                "
                INSERT INTO team_members (id, team_id, user_id, role, permissions, accepted)
                VALUES ($1, $2, $3, $4, $5, $6)
                ",
                team_member.id as TeamMemberId,
                team_member.team_id as TeamId,
                team_member.user_id as UserId,
                team_member.role,
                team_member.permissions.bits() as i64,
                team_member.accepted,
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

/// A member of a team
pub struct TeamMember {
    pub id: TeamMemberId,
    pub team_id: TeamId,
    /// The ID of the user associated with the member
    pub user_id: UserId,
    pub role: String,
    pub permissions: Permissions,
    pub accepted: bool,
}

/// A member of a team
pub struct QueryTeamMember {
    pub id: TeamMemberId,
    pub team_id: TeamId,
    /// The user associated with the member
    pub user: User,
    pub role: String,
    pub permissions: Permissions,
    pub accepted: bool,
}

impl TeamMember {
    /// Lists the members of a team
    pub async fn get_from_team<'a, 'b, E>(
        id: TeamId,
        executor: E,
    ) -> Result<Vec<TeamMember>, super::DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        use futures::stream::TryStreamExt;

        let team_members = sqlx::query!(
            "
            SELECT id, user_id, role, permissions, accepted
            FROM team_members
            WHERE team_id = $1
            ",
            id as TeamId,
        )
        .fetch_many(executor)
        .try_filter_map(|e| async {
            if let Some(m) = e.right() {
                let permissions = Permissions::from_bits(m.permissions as u64);
                if let Some(perms) = permissions {
                    Ok(Some(Ok(TeamMember {
                        id: TeamMemberId(m.id),
                        team_id: id,
                        user_id: UserId(m.user_id),
                        role: m.role,
                        permissions: perms,
                        accepted: m.accepted,
                    })))
                } else {
                    Ok(Some(Err(super::DatabaseError::BitflagError)))
                }
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

    // Lists the full members of a team
    pub async fn get_from_team_full<'a, 'b, E>(
        id: TeamId,
        executor: E,
    ) -> Result<Vec<QueryTeamMember>, super::DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        use futures::stream::TryStreamExt;

        let team_members = sqlx::query!(
            "
            SELECT tm.id id, tm.role member_role, tm.permissions permissions, tm.accepted accepted,
            u.id user_id, u.github_id github_id, u.name user_name, u.email email,
            u.avatar_url avatar_url, u.username username, u.bio bio,
            u.created created, u.role user_role
            FROM team_members tm
            INNER JOIN users u ON u.id = tm.user_id
            WHERE tm.team_id = $1
            ",
            id as TeamId,
        )
        .fetch_many(executor)
        .try_filter_map(|e| async {
            if let Some(m) = e.right() {
                let permissions = Permissions::from_bits(m.permissions as u64);
                if let Some(perms) = permissions {
                    Ok(Some(Ok(QueryTeamMember {
                        id: TeamMemberId(m.id),
                        team_id: id,
                        role: m.member_role,
                        permissions: perms,
                        accepted: m.accepted,
                        user: User {
                            id: UserId(m.user_id),
                            github_id: m.github_id,
                            name: m.user_name,
                            email: m.email,
                            avatar_url: m.avatar_url,
                            username: m.username,
                            bio: m.bio,
                            created: m.created,
                            role: m.user_role,
                        },
                    })))
                } else {
                    Ok(Some(Err(super::DatabaseError::BitflagError)))
                }
            } else {
                Ok(None)
            }
        })
        .try_collect::<Vec<Result<QueryTeamMember, super::DatabaseError>>>()
        .await?;

        let team_members = team_members
            .into_iter()
            .collect::<Result<Vec<QueryTeamMember>, super::DatabaseError>>()?;

        Ok(team_members)
    }

    /// Lists the team members for a user.  Does not list pending requests.
    pub async fn get_from_user_public<'a, 'b, E>(
        id: UserId,
        executor: E,
    ) -> Result<Vec<TeamMember>, super::DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        use futures::stream::TryStreamExt;

        let team_members = sqlx::query!(
            "
            SELECT id, team_id, role, permissions, accepted
            FROM team_members
            WHERE (user_id = $1 AND accepted = TRUE)
            ",
            id as UserId,
        )
        .fetch_many(executor)
        .try_filter_map(|e| async {
            if let Some(m) = e.right() {
                let permissions = Permissions::from_bits(m.permissions as u64);
                if let Some(perms) = permissions {
                    Ok(Some(Ok(TeamMember {
                        id: TeamMemberId(m.id),
                        team_id: TeamId(m.team_id),
                        user_id: id,
                        role: m.role,
                        permissions: perms,
                        accepted: m.accepted,
                    })))
                } else {
                    Ok(Some(Err(super::DatabaseError::BitflagError)))
                }
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

    /// Lists the team members for a user. Includes pending requests.
    pub async fn get_from_user_private<'a, 'b, E>(
        id: UserId,
        executor: E,
    ) -> Result<Vec<TeamMember>, super::DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        use futures::stream::TryStreamExt;

        let team_members = sqlx::query!(
            "
            SELECT id, team_id, role, permissions, accepted
            FROM team_members
            WHERE user_id = $1
            ",
            id as UserId,
        )
        .fetch_many(executor)
        .try_filter_map(|e| async {
            if let Some(m) = e.right() {
                let permissions = Permissions::from_bits(m.permissions as u64);
                if let Some(perms) = permissions {
                    Ok(Some(Ok(TeamMember {
                        id: TeamMemberId(m.id),
                        team_id: TeamId(m.team_id),
                        user_id: id,
                        role: m.role,
                        permissions: perms,
                        accepted: m.accepted,
                    })))
                } else {
                    Ok(Some(Err(super::DatabaseError::BitflagError)))
                }
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

    /// Gets a team member from a user id and team id.  Does not return pending members.
    pub async fn get_from_user_id<'a, 'b, E>(
        id: TeamId,
        user_id: UserId,
        executor: E,
    ) -> Result<Option<Self>, super::DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        let result = sqlx::query!(
            "
            SELECT id, user_id, role, permissions, accepted
            FROM team_members
            WHERE (team_id = $1 AND user_id = $2 AND accepted = TRUE)
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
                permissions: Permissions::from_bits(m.permissions as u64)
                    .ok_or(super::DatabaseError::BitflagError)?,
                accepted: m.accepted,
            }))
        } else {
            Ok(None)
        }
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
            SELECT id, user_id, role, permissions, accepted
            FROM team_members
            WHERE (team_id = $1 AND user_id = $2)
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
                permissions: Permissions::from_bits(m.permissions as u64)
                    .ok_or(super::DatabaseError::BitflagError)?,
                accepted: m.accepted,
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
                id, team_id, user_id, role, permissions, accepted
            )
            VALUES (
                $1, $2, $3, $4, $5, $6
            )
            ",
            self.id as TeamMemberId,
            self.team_id as TeamId,
            self.user_id as UserId,
            self.role,
            self.permissions.bits() as i64,
            self.accepted,
        )
        .execute(&mut *transaction)
        .await?;

        Ok(())
    }

    pub async fn delete<'a, 'b, E>(
        id: TeamId,
        user_id: UserId,
        executor: E,
    ) -> Result<(), super::DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        use sqlx::Done;
        let result = sqlx::query!(
            "
            DELETE FROM team_members
            WHERE (team_id = $1 AND user_id = $2 AND NOT role = $3)
            ",
            id as TeamId,
            user_id as UserId,
            crate::models::teams::OWNER_ROLE,
        )
        .execute(executor)
        .await?;

        if result.rows_affected() != 1 {
            return Err(super::DatabaseError::Other(format!(
                "Deleting a member failed; {} rows deleted",
                result.rows_affected()
            )));
        }

        Ok(())
    }

    pub async fn edit_team_member(
        id: TeamId,
        user_id: UserId,
        new_permissions: Option<Permissions>,
        new_role: Option<String>,
        new_accepted: Option<bool>,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), super::DatabaseError> {
        if let Some(permissions) = new_permissions {
            sqlx::query!(
                "
                UPDATE team_members
                SET permissions = $1
                WHERE (team_id = $2 AND user_id = $3 AND NOT role = $4)
                ",
                permissions.bits() as i64,
                id as TeamId,
                user_id as UserId,
                crate::models::teams::OWNER_ROLE,
            )
            .execute(&mut *transaction)
            .await?;
        }

        if let Some(role) = new_role {
            sqlx::query!(
                "
                UPDATE team_members
                SET role = $1
                WHERE (team_id = $2 AND user_id = $3 AND NOT role = $4)
                ",
                role,
                id as TeamId,
                user_id as UserId,
                crate::models::teams::OWNER_ROLE,
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
                    WHERE (team_id = $1 AND user_id = $2 AND NOT role = $3)
                    ",
                    id as TeamId,
                    user_id as UserId,
                    crate::models::teams::OWNER_ROLE,
                )
                .execute(&mut *transaction)
                .await?;
            }
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
            SELECT tm.id, tm.team_id, tm.user_id, tm.role, tm.permissions, tm.accepted FROM mods m
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
                permissions: Permissions::from_bits(m.permissions as u64)
                    .ok_or(super::DatabaseError::BitflagError)?,
                accepted: m.accepted,
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
            SELECT tm.id, tm.team_id, tm.user_id, tm.role, tm.permissions, tm.accepted FROM versions v
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
                permissions: Permissions::from_bits(m.permissions as u64)
                    .ok_or(super::DatabaseError::BitflagError)?,
                accepted: m.accepted,
            }))
        } else {
            Ok(None)
        }
    }
}
