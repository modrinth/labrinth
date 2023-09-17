use crate::models::teams::ProjectPermissions;

use super::{ids::*, TeamMember};
use itertools::Itertools;
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

    pub async fn get_id<'a, 'b, E>(
        id: OrganizationId,
        exec: E,
        redis: &deadpool_redis::Pool,
    ) -> Result<Option<Self>, super::DatabaseError>
    where 
    E: sqlx::Executor<'a, Database = sqlx::Postgres> {
        Self::get_many_ids(&[id], exec, redis).await.map(|x| x.into_iter().next())
    }

   pub async fn get_many_ids<'a, 'b, E>(
        organization_ids: &[OrganizationId],
        exec: E,
        redis: &deadpool_redis::Pool,
   ) -> Result<Vec<Self>, super::DatabaseError>
    where 
    E: sqlx::Executor<'a, Database = sqlx::Postgres> {
        if organization_ids.is_empty() {
            return Ok(Vec::new());
        }

        use futures::stream::TryStreamExt;

        let mut organization_ids_parsed: Vec<i64> = organization_ids.iter().map(|x| x.0).collect();

        let mut redis = redis.get().await?;

        let mut found_organizations = Vec::new();

        println!("Getting organizations from cache");
        let organizations = cmd("MGET")
            .arg(
                organization_ids_parsed
                    .iter()
                    .map(|x| format!("{}:{}", ORGANIZATIONS_NAMESPACE, x))
                    .collect::<Vec<_>>(),
            )
            .query_async::<_, Vec<Option<String>>>(&mut redis)
            .await?;


        for organization_raw in organizations {
            if let Some(mut organization) = organization_raw
                .clone()
                .and_then(|x| serde_json::from_str::<Vec<Organization>>(&x).ok())
            {
                if let Some(organization_id) = organization.first().map(|x| x.id) {
                    organization_ids_parsed.retain(|x| &organization_id.0 != x);
                }

                found_organizations.append(&mut organization);
                continue;
            }
        }
        println!("Found {} organizations in cache", found_organizations.len());
        println!("Remaining organizations to get: {}", organization_ids_parsed.len());

        if !organization_ids_parsed.is_empty() {
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
                    Ok(e.right().map(|m|
                        Organization {
                            id: OrganizationId(m.id),
                            name: m.name,
                            slug: m.slug,
                            team_id: TeamId(m.team_id),
                            description: m.description,
                            default_project_permissions: ProjectPermissions::from_bits(m.default_project_permissions as u64).unwrap_or_default(),
                        }
                    ))
                })
                .try_collect::<Vec<Organization>>()
                .await?;

            for org in organizations {
                cmd("SET")
                .arg(format!("{}:{}", ORGANIZATIONS_NAMESPACE, org.id.0))
                .arg(serde_json::to_string(&org)?)
                .arg("EX")
                .arg(DEFAULT_EXPIRY)
                .query_async::<_, ()>(&mut redis)
                .await?;

                found_organizations.push(org);
            }
        }
        println!("Found {} organizations in database", found_organizations.len());

        Ok(found_organizations)
    }

    // Gets organization associated with a project ID, if it exists and there is one
    pub async fn get_associated_organization_project_id<'a, 'b, E>(
        project_id: ProjectId,
        exec: E,
    ) -> Result<Option<Self>, super::DatabaseError>
    where E: sqlx::Executor<'a, Database = sqlx::Postgres> {
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
                default_project_permissions: ProjectPermissions::from_bits(result.default_project_permissions as u64).unwrap_or_default(),
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn clear_cache(
        id: OrganizationId,
        redis: &deadpool_redis::Pool,
    ) -> Result<(), super::DatabaseError> {
        let mut redis = redis.get().await?;
        // TODO slugs
        cmd("DEL")
            .arg(format!("{}:{}", ORGANIZATIONS_NAMESPACE, id.0))
            .query_async::<_, ()>(&mut redis)
            .await?;

        Ok(())
    }
}
