use super::ids::*;

pub struct TeamBuilder {
    pub members: Vec<TeamMemberBuilder>,
}
pub struct TeamMemberBuilder {
    pub user_id: UserId,
    pub name: String,
    pub role: String,
    pub permissions: i64,
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
                name: member.name,
                role: member.role,
                permissions: member.permissions,
                accepted: member.accepted,
            };

            sqlx::query!(
                "
                INSERT INTO team_members (id, team_id, user_id, member_name, role, permissions, accepted)
                VALUES ($1, $2, $3, $4, $5, $6, TRUE)
                ",
                team_member.id as TeamMemberId,
                team_member.team_id as TeamId,
                team_member.user_id as UserId,
                team_member.name,
                team_member.role,
                team_member.permissions
            )
            .execute(&mut *transaction)
            .await?;
        }

        Ok(team_id)
    }
}

/// A team of users who control a mod
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
    /// The name of the user
    pub name: String,
    pub role: String,
    pub permissions: i64,
    pub accepted: bool,
}

impl TeamMember {
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
            SELECT id, user_id, member_name, role, permissions, accepted
            FROM team_members
            WHERE (team_id = $1 AND accepted = TRUE)
            ",
            id as TeamId,
        )
        .fetch_many(executor)
        .try_filter_map(|e| async {
            Ok(e.right().map(|m| TeamMember {
                id: TeamMemberId(m.id),
                team_id: id,
                user_id: UserId(m.user_id),
                name: m.member_name,
                role: m.role,
                permissions: m.permissions,
                accepted: m.accepted,
            }))
        })
        .try_collect::<Vec<TeamMember>>()
        .await?;

        Ok(team_members)
    }

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
            SELECT id, team_id, member_name, role, permissions, accepted
            FROM team_members
            WHERE (user_id = $1 AND accepted = TRUE)
            ",
            id as UserId,
        )
        .fetch_many(executor)
        .try_filter_map(|e| async {
            Ok(e.right().map(|m| TeamMember {
                id: TeamMemberId(m.id),
                team_id: TeamId(m.team_id),
                user_id: id,
                name: m.member_name,
                role: m.role,
                permissions: m.permissions,
                accepted: m.accepted,
            }))
        })
        .try_collect::<Vec<TeamMember>>()
        .await?;

        Ok(team_members)
    }

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
            SELECT id, team_id, member_name, role, permissions, accepted
            FROM team_members
            WHERE user_id = $1
            ",
            id as UserId,
        )
        .fetch_many(executor)
        .try_filter_map(|e| async {
            Ok(e.right().map(|m| TeamMember {
                id: TeamMemberId(m.id),
                team_id: TeamId(m.team_id),
                user_id: id,
                name: m.member_name,
                role: m.role,
                permissions: m.permissions,
                accepted: m.accepted,
            }))
        })
        .try_collect::<Vec<TeamMember>>()
        .await?;

        Ok(team_members)
    }

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
            SELECT id, user_id, member_name, role, permissions, accepted
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
                name: m.member_name,
                role: m.role,
                permissions: m.permissions,
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
                id, user_id, member_name, role, permissions, accepted
            )
            VALUES (
                $1, $2, $3, $4, $5,
                $6
            )
            ",
            self.id as TeamMemberId,
            self.user_id as UserId,
            self.name,
            self.role,
            self.permissions,
            self.accepted
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
        sqlx::query!(
            "
            DELETE FROM team_members
            WHERE (team_id = $1 AND user_id = $2 AND NOT role = $3)
            ",
            id as TeamId,
            user_id as UserId,
            crate::models::teams::OWNER_ROLE
        )
        .execute(executor)
        .await?;

        Ok(())
    }

    pub async fn edit_team_member<'a, 'b, E>(
        id: TeamId,
        user_id: UserId,
        new_permissions: Option<i64>,
        new_role: Option<String>,
        new_accepted: Option<bool>,
        executor: E,
    ) -> Result<(), super::DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        let mut query = "UPDATE team_members".to_string();
        let mut current_index: i16 = 3;

        if new_permissions.is_some() {
            current_index += 1;
            query.push_str(&format!("\nSET permissions = ${}", current_index));
        }

        if new_role.is_some() {
            current_index += 1;
            query.push_str(&format!("\nSET role = ${}", current_index));
        }

        if new_accepted.is_some() {
            current_index += 1;
            query.push_str(&format!("\nSET accepted = ${}", current_index));
        }

        query += "\nWHERE (team_id = $1 AND user_id = $2 AND NOT role = $3)";

        let mut query = sqlx::query(&*query)
            .bind(id as TeamId)
            .bind(user_id as UserId)
            .bind::<String>(crate::models::teams::OWNER_ROLE.to_string());

        if let Some(permissions) = new_permissions {
            query = query.bind(permissions);
        }

        if let Some(role) = new_role {
            query = query.bind(role);
        }

        if let Some(accepted) = new_accepted {
            query = query.bind::<bool>(accepted);
        }

        query.execute(executor).await?;

        Ok(())
    }
}
