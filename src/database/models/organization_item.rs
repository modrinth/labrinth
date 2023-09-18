use crate::models::{
    ids::base62_impl::{parse_base62, to_base62},
    teams::ProjectPermissions,
};

use super::{ids::*, TeamMember};
use redis::cmd;
use serde::{Deserialize, Serialize};

const ORGANIZATIONS_NAMESPACE: &str = "organizations";
const ORGANIZATIONS_SLUGS_NAMESPACE: &str = "organizations_slugs";

const DEFAULT_EXPIRY: i64 = 1800;

#[derive(Deserialize, Serialize, Clone)]
/// An organization of users who together control one or more projects and organizations.
pub struct Organization {
    /// The id of the organization
    pub id: OrganizationId,

    /// The slug of the organization
    pub slug: String,

    /// The associated team of the organization
    pub team_id: TeamId,

    /// The name of the organization
    pub name: String,

    /// The description of the organization
    pub description: String,

    /// Default project permissions for associated projects
    pub default_project_permissions: ProjectPermissions,
}

impl Organization {
    pub async fn insert(
        self,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), super::DatabaseError> {
        sqlx::query!(
            "
            INSERT INTO organizations (id, name, slug, team_id, description)
            VALUES ($1, $2, $3, $4, $5)
            ",
            self.id.0,
            self.name,
            self.slug,
            self.team_id as TeamId,
            self.description,
        )
        .execute(&mut *transaction)
        .await?;

        Ok(())
    }

    pub async fn get<'a, E>(
        string: &str,
        exec: E,
        redis: &deadpool_redis::Pool,
    ) -> Result<Option<Self>, super::DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        Self::get_many(&[string], exec, redis)
            .await
            .map(|x| x.into_iter().next())
    }

    pub async fn get_id<'a, 'b, E>(
        id: OrganizationId,
        exec: E,
        redis: &deadpool_redis::Pool,
    ) -> Result<Option<Self>, super::DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        Self::get_many_ids(&[id], exec, redis)
            .await
            .map(|x| x.into_iter().next())
    }

    pub async fn get_many_ids<'a, 'b, E>(
        organization_ids: &[OrganizationId],
        exec: E,
        redis: &deadpool_redis::Pool,
    ) -> Result<Vec<Self>, super::DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        let ids = organization_ids
            .iter()
            .map(|x| crate::models::ids::OrganizationId::from(*x))
            .collect::<Vec<_>>();
        Self::get_many(&ids, exec, redis).await
    }

    pub async fn get_many<'a, E, T: ToString>(
        organization_strings: &[T],
        exec: E,
        redis: &deadpool_redis::Pool,
    ) -> Result<Vec<Self>, super::DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        use futures::stream::TryStreamExt;

        if organization_strings.is_empty() {
            return Ok(Vec::new());
        }

        let mut redis = redis.get().await?;

        let mut found_organizations = Vec::new();
        let mut remaining_strings = organization_strings
            .iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>();

        let mut organization_ids = organization_strings
            .iter()
            .flat_map(|x| parse_base62(&x.to_string()).map(|x| x as i64))
            .collect::<Vec<_>>();

        organization_ids.append(
            &mut cmd("MGET")
                .arg(
                    organization_strings
                        .iter()
                        .map(|x| {
                            format!(
                                "{}:{}",
                                ORGANIZATIONS_SLUGS_NAMESPACE,
                                x.to_string().to_lowercase()
                            )
                        })
                        .collect::<Vec<_>>(),
                )
                .query_async::<_, Vec<Option<i64>>>(&mut redis)
                .await?
                .into_iter()
                .flatten()
                .collect(),
        );

        if !organization_ids.is_empty() {
            let organizations = cmd("MGET")
                .arg(
                    organization_ids
                        .iter()
                        .map(|x| format!("{}:{}", ORGANIZATIONS_NAMESPACE, x))
                        .collect::<Vec<_>>(),
                )
                .query_async::<_, Vec<Option<String>>>(&mut redis)
                .await?;

            for organization in organizations {
                if let Some(organization) =
                    organization.and_then(|x| serde_json::from_str::<Organization>(&x).ok())
                {
                    remaining_strings.retain(|x| {
                        &to_base62(organization.id.0 as u64) != x
                            && organization.slug.to_lowercase() != x.to_lowercase()
                    });
                    found_organizations.push(organization);
                    continue;
                }
            }
        }

        if !remaining_strings.is_empty() {
            let organization_ids_parsed: Vec<i64> = remaining_strings
                .iter()
                .flat_map(|x| parse_base62(&x.to_string()).ok())
                .map(|x| x as i64)
                .collect();
            let organizations: Vec<Organization> = sqlx::query!(
                "
                SELECT id, name, slug, team_id, description, default_project_permissions
                FROM organizations o
                WHERE id = ANY($1)
                ",
                &organization_ids_parsed
            )
            .fetch_many(exec)
            .try_filter_map(|e| async {
                Ok(e.right().map(|m| Organization {
                    id: OrganizationId(m.id),
                    name: m.name,
                    slug: m.slug,
                    team_id: TeamId(m.team_id),
                    description: m.description,
                    default_project_permissions: ProjectPermissions::from_bits(
                        m.default_project_permissions as u64,
                    )
                    .unwrap_or_default(),
                }))
            })
            .try_collect::<Vec<Organization>>()
            .await?;

            for organization in organizations {
                cmd("SET")
                    .arg(format!("{}:{}", ORGANIZATIONS_NAMESPACE, organization.id.0))
                    .arg(serde_json::to_string(&organization)?)
                    .arg("EX")
                    .arg(DEFAULT_EXPIRY)
                    .query_async::<_, ()>(&mut redis)
                    .await?;

                cmd("SET")
                    .arg(format!(
                        "{}:{}",
                        ORGANIZATIONS_SLUGS_NAMESPACE,
                        organization.slug.to_lowercase()
                    ))
                    .arg(organization.id.0)
                    .arg("EX")
                    .arg(DEFAULT_EXPIRY)
                    .query_async::<_, ()>(&mut redis)
                    .await?;
                found_organizations.push(organization);
            }
        }

        Ok(found_organizations)
    }

    // Gets organization associated with a project ID, if it exists and there is one
    pub async fn get_associated_organization_project_id<'a, 'b, E>(
        project_id: ProjectId,
        exec: E,
    ) -> Result<Option<Self>, super::DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        let result = sqlx::query!(
            "
            SELECT o.id, o.name, o.slug, o.team_id, o.description, o.default_project_permissions
            FROM organizations o
            LEFT JOIN mods m ON m.organization_id = o.id
            WHERE m.id = $1
            ",
            project_id as ProjectId,
        )
        .fetch_optional(exec)
        .await?;

        if let Some(result) = result {
            Ok(Some(Organization {
                id: OrganizationId(result.id),
                name: result.name,
                slug: result.slug,
                team_id: TeamId(result.team_id),
                description: result.description,
                default_project_permissions: ProjectPermissions::from_bits(
                    result.default_project_permissions as u64,
                )
                .unwrap_or_default(),
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn remove(
        id: OrganizationId,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        redis: &deadpool_redis::Pool,
    ) -> Result<Option<()>, super::DatabaseError> {
        let project = Self::get_id(id, &mut *transaction, redis).await?;

        if let Some(organization) = project {
            Organization::clear_cache(id, Some(organization.slug), redis).await?;

            sqlx::query!(
                "
                DELETE FROM organizations
                WHERE id = $1
                ",
                id as OrganizationId,
            )
            .execute(&mut *transaction)
            .await?;

            TeamMember::clear_cache(organization.team_id, redis).await?;

            sqlx::query!(
                "
                DELETE FROM team_members
                WHERE team_id = $1
                ",
                organization.team_id as TeamId,
            )
            .execute(&mut *transaction)
            .await?;

            sqlx::query!(
                "
                DELETE FROM teams
                WHERE id = $1
                ",
                organization.team_id as TeamId,
            )
            .execute(&mut *transaction)
            .await?;

            Ok(Some(()))
        } else {
            Ok(None)
        }
    }

    pub async fn clear_cache(
        id: OrganizationId,
        slug: Option<String>,
        redis: &deadpool_redis::Pool,
    ) -> Result<(), super::DatabaseError> {
        let mut redis = redis.get().await?;
        // TODO slugs
        let mut cmd = cmd("DEL");
        cmd.arg(format!("{}:{}", ORGANIZATIONS_NAMESPACE, id.0));
        if let Some(slug) = slug {
            cmd.arg(format!(
                "{}:{}",
                ORGANIZATIONS_SLUGS_NAMESPACE,
                slug.to_lowercase()
            ));
        }
        cmd.query_async::<_, ()>(&mut redis).await?;

        Ok(())
    }
}
