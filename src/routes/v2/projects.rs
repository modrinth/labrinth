use crate::auth::{get_user_from_headers, is_authorized};
use crate::database;
use crate::database::models::project_item::{GalleryItem, ModCategory};
use crate::database::models::{image_item, project_item, version_item};
use crate::database::redis::RedisPool;
use crate::file_hosting::FileHost;
use crate::models;
use crate::models::images::ImageContext;
use crate::models::pats::Scopes;
use crate::models::projects::{
    DonationLink, MonetizationStatus, Project, ProjectId, ProjectStatus, SearchRequest, SideType,
};
use crate::models::teams::ProjectPermissions;
use crate::models::v2::projects::LegacyProject;
use crate::queue::session::AuthQueue;
use crate::routes::v3::projects::{delete_from_index, ProjectIds};
use crate::routes::{v2_reroute, v3, ApiError};
use crate::search::{search_for_project, SearchConfig, SearchError};
use crate::util::routes::read_from_payload;
use crate::util::validate::validation_errors_to_string;
use actix_web::{delete, get, patch, post, web, HttpRequest, HttpResponse};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;
use validator::Validate;

use database::models as db_models;
use db_models::ids as db_ids;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(project_search);
    cfg.service(projects_get);
    cfg.service(projects_edit);
    cfg.service(random_projects_get);

    cfg.service(
        web::scope("project")
            .service(project_get)
            .service(project_get_check)
            .service(project_delete)
            .service(project_edit)
            .service(project_icon_edit)
            .service(delete_project_icon)
            .service(add_gallery_item)
            .service(edit_gallery_item)
            .service(delete_gallery_item)
            .service(project_follow)
            .service(project_unfollow)
            .service(project_schedule)
            .service(super::teams::team_members_get_project)
            .service(
                web::scope("{project_id}")
                    .service(super::versions::version_list)
                    .service(super::versions::version_project_get)
                    .service(dependency_list),
            ),
    );
}

#[get("search")]
pub async fn project_search(
    web::Query(info): web::Query<SearchRequest>,
    config: web::Data<SearchConfig>,
) -> Result<HttpResponse, SearchError> {
    // TODO: make this nicer
    // Search now uses loader_fields instead of explicit 'client_side' and 'server_side' fields
    // While the backend for this has changed, it doesnt affect much
    // in the API calls except that 'versions:x' is now 'game_versions:x'
    let facets: Option<Vec<Vec<String>>> = if let Some(facets) = info.facets {
        let facets = serde_json::from_str::<Vec<Vec<&str>>>(&facets)?;
        Some(
            facets
                .into_iter()
                .map(|facet| {
                    facet
                        .into_iter()
                        .map(|facet| {
                            let version = match facet.split(':').nth(1) {
                                Some(version) => version,
                                None => return facet.to_string(),
                            };

                            if facet.starts_with("versions:") {
                                format!("game_versions:{}", version)
                            } else {
                                facet.to_string()
                            }
                        })
                        .collect::<Vec<_>>()
                })
                .collect(),
        )
    } else {
        None
    };

    let info = SearchRequest {
        facets: facets.and_then(|x| serde_json::to_string(&x).ok()),
        ..info
    };

    let results = search_for_project(&info, &config).await?;

    // TODO: convert to v2 format-we may need a new v2 struct for this for 'original' format

    Ok(HttpResponse::Ok().json(results))
}

#[derive(Deserialize, Validate)]
pub struct RandomProjects {
    #[validate(range(min = 1, max = 100))]
    pub count: u32,
}

#[get("projects_random")]
pub async fn random_projects_get(
    web::Query(count): web::Query<RandomProjects>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
) -> Result<HttpResponse, ApiError> {
    let count = v3::projects::RandomProjects { count: count.count };

    let response =
        v3::projects::random_projects_get(web::Query(count), pool.clone(), redis.clone()).await?;
    // Convert response to V2 format
    match v2_reroute::extract_ok_json::<Project>(response).await {
        Ok(project) => {
            let version_item = match project.versions.first() {
                Some(vid) => version_item::Version::get((*vid).into(), &**pool, &redis).await?,
                None => None,
            };
            let project = LegacyProject::from(project, version_item);
            Ok(HttpResponse::Ok().json(project))
        }
        Err(response) => Ok(response),
    }
}

#[get("projects")]
pub async fn projects_get(
    req: HttpRequest,
    web::Query(ids): web::Query<ProjectIds>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    // Call V3 project creation
    let response = v3::projects::projects_get(
        req,
        web::Query(ids),
        pool.clone(),
        redis.clone(),
        session_queue,
    )
    .await?;

    // Convert response to V2 format
    match v2_reroute::extract_ok_json::<Vec<Project>>(response).await {
        Ok(project) => {
            let legacy_projects = LegacyProject::from_many(project, &**pool, &redis).await?;
            Ok(HttpResponse::Ok().json(legacy_projects))
        }
        Err(response) => Ok(response),
    }
}

#[get("{id}")]
pub async fn project_get(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    // Convert V2 data to V3 data

    // Call V3 project creation
    let response =
        v3::projects::project_get(req, info, pool.clone(), redis.clone(), session_queue).await?;

    // Convert response to V2 format
    match v2_reroute::extract_ok_json::<Project>(response).await {
        Ok(project) => {
            let version_item = match project.versions.first() {
                Some(vid) => version_item::Version::get((*vid).into(), &**pool, &redis).await?,
                None => None,
            };
            let project = LegacyProject::from(project, version_item);
            Ok(HttpResponse::Ok().json(project))
        }
        Err(response) => Ok(response),
    }
}

//checks the validity of a project id or slug
#[get("{id}/check")]
pub async fn project_get_check(
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
) -> Result<HttpResponse, ApiError> {
    let slug = info.into_inner().0;

    let project_data = db_models::Project::get(&slug, &**pool, &redis).await?;

    if let Some(project) = project_data {
        Ok(HttpResponse::Ok().json(json! ({
            "id": models::ids::ProjectId::from(project.inner.id)
        })))
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
}

#[derive(Serialize)]
struct DependencyInfo {
    pub projects: Vec<Project>,
    pub versions: Vec<models::projects::Version>,
}

#[get("dependencies")]
pub async fn dependency_list(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let string = info.into_inner().0;

    let result = db_models::Project::get(&string, &**pool, &redis).await?;

    let user_option = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::PROJECT_READ]),
    )
    .await
    .map(|x| x.1)
    .ok();

    if let Some(project) = result {
        if !is_authorized(&project.inner, &user_option, &pool).await? {
            return Ok(HttpResponse::NotFound().body(""));
        }

        let dependencies =
            database::Project::get_dependencies(project.inner.id, &**pool, &redis).await?;

        let project_ids = dependencies
            .iter()
            .filter_map(|x| {
                if x.0.is_none() {
                    if let Some(mod_dependency_id) = x.2 {
                        Some(mod_dependency_id)
                    } else {
                        x.1
                    }
                } else {
                    x.1
                }
            })
            .collect::<Vec<_>>();

        let dep_version_ids = dependencies
            .iter()
            .filter_map(|x| x.0)
            .collect::<Vec<db_models::VersionId>>();
        let (projects_result, versions_result) = futures::future::try_join(
            database::Project::get_many_ids(&project_ids, &**pool, &redis),
            database::Version::get_many(&dep_version_ids, &**pool, &redis),
        )
        .await?;

        let mut projects = projects_result
            .into_iter()
            .map(models::projects::Project::from)
            .collect::<Vec<_>>();
        let mut versions = versions_result
            .into_iter()
            .map(models::projects::Version::from)
            .collect::<Vec<_>>();

        projects.sort_by(|a, b| b.published.cmp(&a.published));
        projects.dedup_by(|a, b| a.id == b.id);

        versions.sort_by(|a, b| b.date_published.cmp(&a.date_published));
        versions.dedup_by(|a, b| a.id == b.id);

        Ok(HttpResponse::Ok().json(DependencyInfo { projects, versions }))
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
}

#[derive(Serialize, Deserialize, Validate)]
pub struct EditProject {
    #[validate(
        length(min = 3, max = 64),
        custom(function = "crate::util::validate::validate_name")
    )]
    pub title: Option<String>,
    #[validate(length(min = 3, max = 256))]
    pub description: Option<String>,
    #[validate(length(max = 65536))]
    pub body: Option<String>,
    #[validate(length(max = 3))]
    pub categories: Option<Vec<String>>,
    #[validate(length(max = 256))]
    pub additional_categories: Option<Vec<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    #[validate(
        custom(function = "crate::util::validate::validate_url"),
        length(max = 2048)
    )]
    pub issues_url: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    #[validate(
        custom(function = "crate::util::validate::validate_url"),
        length(max = 2048)
    )]
    pub source_url: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    #[validate(
        custom(function = "crate::util::validate::validate_url"),
        length(max = 2048)
    )]
    pub wiki_url: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    #[validate(
        custom(function = "crate::util::validate::validate_url"),
        length(max = 2048)
    )]
    pub license_url: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    #[validate(
        custom(function = "crate::util::validate::validate_url"),
        length(max = 2048)
    )]
    pub discord_url: Option<Option<String>>,
    #[validate]
    pub donation_urls: Option<Vec<DonationLink>>,
    pub license_id: Option<String>,
    pub client_side: Option<SideType>,
    pub server_side: Option<SideType>,
    #[validate(
        length(min = 3, max = 64),
        regex = "crate::util::validate::RE_URL_SAFE"
    )]
    pub slug: Option<String>,
    pub status: Option<ProjectStatus>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    pub requested_status: Option<Option<ProjectStatus>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    #[validate(length(max = 2000))]
    pub moderation_message: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    #[validate(length(max = 65536))]
    pub moderation_message_body: Option<Option<String>>,
    pub monetization_status: Option<MonetizationStatus>,
}

#[patch("{id}")]
pub async fn project_edit(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    config: web::Data<SearchConfig>,
    new_project: web::Json<EditProject>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let v2_new_project = new_project.into_inner();
    let client_side = v2_new_project.client_side.clone();
    let server_side = v2_new_project.server_side.clone();
    let new_slug = v2_new_project.slug.clone();

    let new_project = v3::projects::EditProject {
        title: v2_new_project.title,
        description: v2_new_project.description,
        body: v2_new_project.body,
        categories: v2_new_project.categories,
        additional_categories: v2_new_project.additional_categories,
        issues_url: v2_new_project.issues_url,
        source_url: v2_new_project.source_url,
        wiki_url: v2_new_project.wiki_url,
        license_url: v2_new_project.license_url,
        discord_url: v2_new_project.discord_url,
        donation_urls: v2_new_project.donation_urls,
        license_id: v2_new_project.license_id,
        slug: v2_new_project.slug,
        status: v2_new_project.status,
        requested_status: v2_new_project.requested_status,
        moderation_message: v2_new_project.moderation_message,
        moderation_message_body: v2_new_project.moderation_message_body,
        monetization_status: v2_new_project.monetization_status,
    };

    // This returns 204 or failure so we don't need to do anything with it
    let project_id = info.clone().0;
    let mut response = v3::projects::project_edit(
        req.clone(),
        info,
        pool.clone(),
        config,
        web::Json(new_project),
        redis.clone(),
        session_queue.clone(),
    )
    .await?;

    // If client and server side were set, we will call
    // the version setting route for each version to set the side types for each of them.
    if response.status().is_success() && (client_side.is_some() || server_side.is_some()) {
        let project_item =
            project_item::Project::get(&new_slug.unwrap_or(project_id), &**pool, &redis).await?;
        let version_ids = project_item.map(|x| x.versions).unwrap_or_default();
        let versions = version_item::Version::get_many(&version_ids, &**pool, &redis).await?;
        for version in versions {
            let mut fields = HashMap::new();
            fields.insert("client_side".to_string(), json!(client_side));
            fields.insert("server_side".to_string(), json!(server_side));
            response = v3::versions::version_edit_helper(
                req.clone(),
                (version.inner.id.into(),),
                pool.clone(),
                redis.clone(),
                v3::versions::EditVersion {
                    fields,
                    ..Default::default()
                },
                session_queue.clone(),
            )
            .await?;
        }
    }
    Ok(response)
}

#[derive(derive_new::new)]
pub struct CategoryChanges<'a> {
    pub categories: &'a Option<Vec<String>>,
    pub add_categories: &'a Option<Vec<String>>,
    pub remove_categories: &'a Option<Vec<String>>,
}

#[derive(Deserialize, Validate)]
pub struct BulkEditProject {
    #[validate(length(max = 3))]
    pub categories: Option<Vec<String>>,
    #[validate(length(max = 3))]
    pub add_categories: Option<Vec<String>>,
    pub remove_categories: Option<Vec<String>>,

    #[validate(length(max = 256))]
    pub additional_categories: Option<Vec<String>>,
    #[validate(length(max = 3))]
    pub add_additional_categories: Option<Vec<String>>,
    pub remove_additional_categories: Option<Vec<String>>,

    #[validate]
    pub donation_urls: Option<Vec<DonationLink>>,
    #[validate]
    pub add_donation_urls: Option<Vec<DonationLink>>,
    #[validate]
    pub remove_donation_urls: Option<Vec<DonationLink>>,

    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    #[validate(
        custom(function = "crate::util::validate::validate_url"),
        length(max = 2048)
    )]
    pub issues_url: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    #[validate(
        custom(function = "crate::util::validate::validate_url"),
        length(max = 2048)
    )]
    pub source_url: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    #[validate(
        custom(function = "crate::util::validate::validate_url"),
        length(max = 2048)
    )]
    pub wiki_url: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    #[validate(
        custom(function = "crate::util::validate::validate_url"),
        length(max = 2048)
    )]
    pub discord_url: Option<Option<String>>,
}

#[patch("projects")]
pub async fn projects_edit(
    req: HttpRequest,
    web::Query(ids): web::Query<ProjectIds>,
    pool: web::Data<PgPool>,
    bulk_edit_project: web::Json<BulkEditProject>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::PROJECT_WRITE]),
    )
    .await?
    .1;

    bulk_edit_project
        .validate()
        .map_err(|err| ApiError::Validation(validation_errors_to_string(err, None)))?;

    let project_ids: Vec<db_ids::ProjectId> = serde_json::from_str::<Vec<ProjectId>>(&ids.ids)?
        .into_iter()
        .map(|x| x.into())
        .collect();

    let projects_data = db_models::Project::get_many_ids(&project_ids, &**pool, &redis).await?;

    if let Some(id) = project_ids
        .iter()
        .find(|x| !projects_data.iter().any(|y| x == &&y.inner.id))
    {
        return Err(ApiError::InvalidInput(format!(
            "Project {} not found",
            ProjectId(id.0 as u64)
        )));
    }

    let team_ids = projects_data
        .iter()
        .map(|x| x.inner.team_id)
        .collect::<Vec<db_models::TeamId>>();
    let team_members =
        db_models::TeamMember::get_from_team_full_many(&team_ids, &**pool, &redis).await?;

    let organization_ids = projects_data
        .iter()
        .filter_map(|x| x.inner.organization_id)
        .collect::<Vec<db_models::OrganizationId>>();
    let organizations =
        db_models::Organization::get_many_ids(&organization_ids, &**pool, &redis).await?;

    let organization_team_ids = organizations
        .iter()
        .map(|x| x.team_id)
        .collect::<Vec<db_models::TeamId>>();
    let organization_team_members =
        db_models::TeamMember::get_from_team_full_many(&organization_team_ids, &**pool, &redis)
            .await?;

    let categories = db_models::categories::Category::list(&**pool, &redis).await?;
    let donation_platforms = db_models::categories::DonationPlatform::list(&**pool, &redis).await?;

    let mut transaction = pool.begin().await?;

    for project in projects_data {
        if !user.role.is_mod() {
            let team_member = team_members
                .iter()
                .find(|x| x.team_id == project.inner.team_id && x.user_id == user.id.into());

            let organization = project
                .inner
                .organization_id
                .and_then(|oid| organizations.iter().find(|x| x.id == oid));

            let organization_team_member = if let Some(organization) = organization {
                organization_team_members
                    .iter()
                    .find(|x| x.team_id == organization.team_id && x.user_id == user.id.into())
            } else {
                None
            };

            let permissions = ProjectPermissions::get_permissions_by_role(
                &user.role,
                &team_member.cloned(),
                &organization_team_member.cloned(),
            )
            .unwrap_or_default();

            if team_member.is_some() {
                if !permissions.contains(ProjectPermissions::EDIT_DETAILS) {
                    return Err(ApiError::CustomAuthentication(format!(
                        "You do not have the permissions to bulk edit project {}!",
                        project.inner.title
                    )));
                }
            } else if project.inner.status.is_hidden() {
                return Err(ApiError::InvalidInput(format!(
                    "Project {} not found",
                    ProjectId(project.inner.id.0 as u64)
                )));
            } else {
                return Err(ApiError::CustomAuthentication(format!(
                    "You are not a member of project {}!",
                    project.inner.title
                )));
            };
        }

        bulk_edit_project_categories(
            &categories,
            &project.categories,
            project.inner.id as db_ids::ProjectId,
            CategoryChanges::new(
                &bulk_edit_project.categories,
                &bulk_edit_project.add_categories,
                &bulk_edit_project.remove_categories,
            ),
            3,
            false,
            &mut transaction,
        )
        .await?;

        bulk_edit_project_categories(
            &categories,
            &project.additional_categories,
            project.inner.id as db_ids::ProjectId,
            CategoryChanges::new(
                &bulk_edit_project.additional_categories,
                &bulk_edit_project.add_additional_categories,
                &bulk_edit_project.remove_additional_categories,
            ),
            256,
            true,
            &mut transaction,
        )
        .await?;

        let project_donations: Vec<DonationLink> = project
            .donation_urls
            .into_iter()
            .map(|d| DonationLink {
                id: d.platform_short,
                platform: d.platform_name,
                url: d.url,
            })
            .collect();
        let mut set_donation_links =
            if let Some(donation_links) = bulk_edit_project.donation_urls.clone() {
                donation_links
            } else {
                project_donations.clone()
            };

        if let Some(delete_donations) = &bulk_edit_project.remove_donation_urls {
            for donation in delete_donations {
                if let Some(pos) = set_donation_links
                    .iter()
                    .position(|x| donation.url == x.url && donation.id == x.id)
                {
                    set_donation_links.remove(pos);
                }
            }
        }

        if let Some(add_donations) = &bulk_edit_project.add_donation_urls {
            set_donation_links.append(&mut add_donations.clone());
        }

        if set_donation_links != project_donations {
            sqlx::query!(
                "
                DELETE FROM mods_donations
                WHERE joining_mod_id = $1
                ",
                project.inner.id as db_ids::ProjectId,
            )
            .execute(&mut *transaction)
            .await?;

            for donation in set_donation_links {
                let platform_id = donation_platforms
                    .iter()
                    .find(|x| x.short == donation.id)
                    .ok_or_else(|| {
                        ApiError::InvalidInput(format!(
                            "Platform {} does not exist.",
                            donation.id.clone()
                        ))
                    })?
                    .id;

                sqlx::query!(
                    "
                    INSERT INTO mods_donations (joining_mod_id, joining_platform_id, url)
                    VALUES ($1, $2, $3)
                    ",
                    project.inner.id as db_ids::ProjectId,
                    platform_id as db_ids::DonationPlatformId,
                    donation.url
                )
                .execute(&mut *transaction)
                .await?;
            }
        }

        if let Some(issues_url) = &bulk_edit_project.issues_url {
            sqlx::query!(
                "
                UPDATE mods
                SET issues_url = $1
                WHERE (id = $2)
                ",
                issues_url.as_deref(),
                project.inner.id as db_ids::ProjectId,
            )
            .execute(&mut *transaction)
            .await?;
        }

        if let Some(source_url) = &bulk_edit_project.source_url {
            sqlx::query!(
                "
                UPDATE mods
                SET source_url = $1
                WHERE (id = $2)
                ",
                source_url.as_deref(),
                project.inner.id as db_ids::ProjectId,
            )
            .execute(&mut *transaction)
            .await?;
        }

        if let Some(wiki_url) = &bulk_edit_project.wiki_url {
            sqlx::query!(
                "
                UPDATE mods
                SET wiki_url = $1
                WHERE (id = $2)
                ",
                wiki_url.as_deref(),
                project.inner.id as db_ids::ProjectId,
            )
            .execute(&mut *transaction)
            .await?;
        }

        if let Some(discord_url) = &bulk_edit_project.discord_url {
            sqlx::query!(
                "
                UPDATE mods
                SET discord_url = $1
                WHERE (id = $2)
                ",
                discord_url.as_deref(),
                project.inner.id as db_ids::ProjectId,
            )
            .execute(&mut *transaction)
            .await?;
        }

        db_models::Project::clear_cache(project.inner.id, project.inner.slug, None, &redis).await?;
    }

    transaction.commit().await?;

    Ok(HttpResponse::NoContent().body(""))
}

pub async fn bulk_edit_project_categories(
    all_db_categories: &[db_models::categories::Category],
    project_categories: &Vec<String>,
    project_id: db_ids::ProjectId,
    bulk_changes: CategoryChanges<'_>,
    max_num_categories: usize,
    is_additional: bool,
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<(), ApiError> {
    let mut set_categories = if let Some(categories) = bulk_changes.categories.clone() {
        categories
    } else {
        project_categories.clone()
    };

    if let Some(delete_categories) = &bulk_changes.remove_categories {
        for category in delete_categories {
            if let Some(pos) = set_categories.iter().position(|x| x == category) {
                set_categories.remove(pos);
            }
        }
    }

    if let Some(add_categories) = &bulk_changes.add_categories {
        for category in add_categories {
            if set_categories.len() < max_num_categories {
                set_categories.push(category.clone());
            } else {
                break;
            }
        }
    }

    if &set_categories != project_categories {
        sqlx::query!(
            "
            DELETE FROM mods_categories
            WHERE joining_mod_id = $1 AND is_additional = $2
            ",
            project_id as db_ids::ProjectId,
            is_additional
        )
        .execute(&mut *transaction)
        .await?;

        let mut mod_categories = Vec::new();
        for category in set_categories {
            let category_id = all_db_categories
                .iter()
                .find(|x| x.category == category)
                .ok_or_else(|| {
                    ApiError::InvalidInput(format!("Category {} does not exist.", category.clone()))
                })?
                .id;
            mod_categories.push(ModCategory::new(project_id, category_id, is_additional));
        }
        ModCategory::insert_many(mod_categories, &mut *transaction).await?;
    }

    Ok(())
}

#[derive(Deserialize)]
pub struct SchedulingData {
    pub time: DateTime<Utc>,
    pub requested_status: ProjectStatus,
}

#[post("{id}/schedule")]
pub async fn project_schedule(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
    scheduling_data: web::Json<SchedulingData>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::PROJECT_WRITE]),
    )
    .await?
    .1;

    if scheduling_data.time < Utc::now() {
        return Err(ApiError::InvalidInput(
            "You cannot schedule a project to be released in the past!".to_string(),
        ));
    }

    if !scheduling_data.requested_status.can_be_requested() {
        return Err(ApiError::InvalidInput(
            "Specified requested status cannot be requested!".to_string(),
        ));
    }

    let string = info.into_inner().0;
    let result = db_models::Project::get(&string, &**pool, &redis).await?;

    if let Some(project_item) = result {
        let (team_member, organization_team_member) =
            db_models::TeamMember::get_for_project_permissions(
                &project_item.inner,
                user.id.into(),
                &**pool,
            )
            .await?;

        let permissions = ProjectPermissions::get_permissions_by_role(
            &user.role,
            &team_member.clone(),
            &organization_team_member.clone(),
        )
        .unwrap_or_default();

        if !user.role.is_mod() && !permissions.contains(ProjectPermissions::EDIT_DETAILS) {
            return Err(ApiError::CustomAuthentication(
                "You do not have permission to edit this project's scheduling data!".to_string(),
            ));
        }

        if !project_item.inner.status.is_approved() {
            return Err(ApiError::InvalidInput(
                "This project has not been approved yet. Submit to the queue with the private status to schedule it in the future!".to_string(),
            ));
        }

        sqlx::query!(
            "
            UPDATE mods
            SET status = $1, approved = $2
            WHERE (id = $3)
            ",
            ProjectStatus::Scheduled.as_str(),
            scheduling_data.time,
            project_item.inner.id as db_ids::ProjectId,
        )
        .execute(&**pool)
        .await?;

        db_models::Project::clear_cache(
            project_item.inner.id,
            project_item.inner.slug,
            None,
            &redis,
        )
        .await?;

        Ok(HttpResponse::NoContent().body(""))
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
}

#[derive(Serialize, Deserialize)]
pub struct Extension {
    pub ext: String,
}

#[patch("{id}/icon")]
#[allow(clippy::too_many_arguments)]
pub async fn project_icon_edit(
    web::Query(ext): web::Query<Extension>,
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    file_host: web::Data<Arc<dyn FileHost + Send + Sync>>,
    mut payload: web::Payload,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    if let Some(content_type) = crate::util::ext::get_image_content_type(&ext.ext) {
        let cdn_url = dotenvy::var("CDN_URL")?;
        let user = get_user_from_headers(
            &req,
            &**pool,
            &redis,
            &session_queue,
            Some(&[Scopes::PROJECT_WRITE]),
        )
        .await?
        .1;
        let string = info.into_inner().0;

        let project_item = db_models::Project::get(&string, &**pool, &redis)
            .await?
            .ok_or_else(|| {
                ApiError::InvalidInput("The specified project does not exist!".to_string())
            })?;

        if !user.role.is_mod() {
            let (team_member, organization_team_member) =
                db_models::TeamMember::get_for_project_permissions(
                    &project_item.inner,
                    user.id.into(),
                    &**pool,
                )
                .await?;

            // Hide the project
            if team_member.is_none() && organization_team_member.is_none() {
                return Err(ApiError::CustomAuthentication(
                    "The specified project does not exist!".to_string(),
                ));
            }

            let permissions = ProjectPermissions::get_permissions_by_role(
                &user.role,
                &team_member,
                &organization_team_member,
            )
            .unwrap_or_default();

            if !permissions.contains(ProjectPermissions::EDIT_DETAILS) {
                return Err(ApiError::CustomAuthentication(
                    "You don't have permission to edit this project's icon.".to_string(),
                ));
            }
        }

        if let Some(icon) = project_item.inner.icon_url {
            let name = icon.split(&format!("{cdn_url}/")).nth(1);

            if let Some(icon_path) = name {
                file_host.delete_file_version("", icon_path).await?;
            }
        }

        let bytes =
            read_from_payload(&mut payload, 262144, "Icons must be smaller than 256KiB").await?;

        let color = crate::util::img::get_color_from_img(&bytes)?;

        let hash = sha1::Sha1::from(&bytes).hexdigest();
        let project_id: ProjectId = project_item.inner.id.into();
        let upload_data = file_host
            .upload_file(
                content_type,
                &format!("data/{}/{}.{}", project_id, hash, ext.ext),
                bytes.freeze(),
            )
            .await?;

        let mut transaction = pool.begin().await?;

        sqlx::query!(
            "
            UPDATE mods
            SET icon_url = $1, color = $2
            WHERE (id = $3)
            ",
            format!("{}/{}", cdn_url, upload_data.file_name),
            color.map(|x| x as i32),
            project_item.inner.id as db_ids::ProjectId,
        )
        .execute(&mut *transaction)
        .await?;

        db_models::Project::clear_cache(
            project_item.inner.id,
            project_item.inner.slug,
            None,
            &redis,
        )
        .await?;

        transaction.commit().await?;

        Ok(HttpResponse::NoContent().body(""))
    } else {
        Err(ApiError::InvalidInput(format!(
            "Invalid format for project icon: {}",
            ext.ext
        )))
    }
}

#[delete("{id}/icon")]
pub async fn delete_project_icon(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    file_host: web::Data<Arc<dyn FileHost + Send + Sync>>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::PROJECT_WRITE]),
    )
    .await?
    .1;
    let string = info.into_inner().0;

    let project_item = db_models::Project::get(&string, &**pool, &redis)
        .await?
        .ok_or_else(|| {
            ApiError::InvalidInput("The specified project does not exist!".to_string())
        })?;

    if !user.role.is_mod() {
        let (team_member, organization_team_member) =
            db_models::TeamMember::get_for_project_permissions(
                &project_item.inner,
                user.id.into(),
                &**pool,
            )
            .await?;

        // Hide the project
        if team_member.is_none() && organization_team_member.is_none() {
            return Err(ApiError::CustomAuthentication(
                "The specified project does not exist!".to_string(),
            ));
        }
        let permissions = ProjectPermissions::get_permissions_by_role(
            &user.role,
            &team_member,
            &organization_team_member,
        )
        .unwrap_or_default();

        if !permissions.contains(ProjectPermissions::EDIT_DETAILS) {
            return Err(ApiError::CustomAuthentication(
                "You don't have permission to edit this project's icon.".to_string(),
            ));
        }
    }

    let cdn_url = dotenvy::var("CDN_URL")?;
    if let Some(icon) = project_item.inner.icon_url {
        let name = icon.split(&format!("{cdn_url}/")).nth(1);

        if let Some(icon_path) = name {
            file_host.delete_file_version("", icon_path).await?;
        }
    }

    let mut transaction = pool.begin().await?;

    sqlx::query!(
        "
        UPDATE mods
        SET icon_url = NULL, color = NULL
        WHERE (id = $1)
        ",
        project_item.inner.id as db_ids::ProjectId,
    )
    .execute(&mut *transaction)
    .await?;

    db_models::Project::clear_cache(project_item.inner.id, project_item.inner.slug, None, &redis)
        .await?;

    transaction.commit().await?;

    Ok(HttpResponse::NoContent().body(""))
}

#[derive(Serialize, Deserialize, Validate)]
pub struct GalleryCreateQuery {
    pub featured: bool,
    #[validate(length(min = 1, max = 255))]
    pub title: Option<String>,
    #[validate(length(min = 1, max = 2048))]
    pub description: Option<String>,
    pub ordering: Option<i64>,
}

#[post("{id}/gallery")]
#[allow(clippy::too_many_arguments)]
pub async fn add_gallery_item(
    web::Query(ext): web::Query<Extension>,
    req: HttpRequest,
    web::Query(item): web::Query<GalleryCreateQuery>,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    file_host: web::Data<Arc<dyn FileHost + Send + Sync>>,
    mut payload: web::Payload,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    if let Some(content_type) = crate::util::ext::get_image_content_type(&ext.ext) {
        item.validate()
            .map_err(|err| ApiError::Validation(validation_errors_to_string(err, None)))?;

        let cdn_url = dotenvy::var("CDN_URL")?;
        let user = get_user_from_headers(
            &req,
            &**pool,
            &redis,
            &session_queue,
            Some(&[Scopes::PROJECT_WRITE]),
        )
        .await?
        .1;
        let string = info.into_inner().0;

        let project_item = db_models::Project::get(&string, &**pool, &redis)
            .await?
            .ok_or_else(|| {
                ApiError::InvalidInput("The specified project does not exist!".to_string())
            })?;

        if project_item.gallery_items.len() > 64 {
            return Err(ApiError::CustomAuthentication(
                "You have reached the maximum of gallery images to upload.".to_string(),
            ));
        }

        if !user.role.is_admin() {
            let (team_member, organization_team_member) =
                db_models::TeamMember::get_for_project_permissions(
                    &project_item.inner,
                    user.id.into(),
                    &**pool,
                )
                .await?;

            // Hide the project
            if team_member.is_none() && organization_team_member.is_none() {
                return Err(ApiError::CustomAuthentication(
                    "The specified project does not exist!".to_string(),
                ));
            }

            let permissions = ProjectPermissions::get_permissions_by_role(
                &user.role,
                &team_member,
                &organization_team_member,
            )
            .unwrap_or_default();

            if !permissions.contains(ProjectPermissions::EDIT_DETAILS) {
                return Err(ApiError::CustomAuthentication(
                    "You don't have permission to edit this project's gallery.".to_string(),
                ));
            }
        }

        let bytes = read_from_payload(
            &mut payload,
            5 * (1 << 20),
            "Gallery image exceeds the maximum of 5MiB.",
        )
        .await?;
        let hash = sha1::Sha1::from(&bytes).hexdigest();

        let id: ProjectId = project_item.inner.id.into();
        let url = format!("data/{}/images/{}.{}", id, hash, &*ext.ext);

        let file_url = format!("{cdn_url}/{url}");
        if project_item
            .gallery_items
            .iter()
            .any(|x| x.image_url == file_url)
        {
            return Err(ApiError::InvalidInput(
                "You may not upload duplicate gallery images!".to_string(),
            ));
        }

        file_host
            .upload_file(content_type, &url, bytes.freeze())
            .await?;

        let mut transaction = pool.begin().await?;

        if item.featured {
            sqlx::query!(
                "
                UPDATE mods_gallery
                SET featured = $2
                WHERE mod_id = $1
                ",
                project_item.inner.id as db_ids::ProjectId,
                false,
            )
            .execute(&mut *transaction)
            .await?;
        }

        let gallery_item = vec![db_models::project_item::GalleryItem {
            image_url: file_url,
            featured: item.featured,
            title: item.title,
            description: item.description,
            created: Utc::now(),
            ordering: item.ordering.unwrap_or(0),
        }];
        GalleryItem::insert_many(gallery_item, project_item.inner.id, &mut transaction).await?;

        db_models::Project::clear_cache(
            project_item.inner.id,
            project_item.inner.slug,
            None,
            &redis,
        )
        .await?;

        transaction.commit().await?;

        Ok(HttpResponse::NoContent().body(""))
    } else {
        Err(ApiError::InvalidInput(format!(
            "Invalid format for gallery image: {}",
            ext.ext
        )))
    }
}

#[derive(Serialize, Deserialize, Validate)]
pub struct GalleryEditQuery {
    /// The url of the gallery item to edit
    pub url: String,
    pub featured: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    #[validate(length(min = 1, max = 255))]
    pub title: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    #[validate(length(min = 1, max = 2048))]
    pub description: Option<Option<String>>,
    pub ordering: Option<i64>,
}

#[patch("{id}/gallery")]
pub async fn edit_gallery_item(
    req: HttpRequest,
    web::Query(item): web::Query<GalleryEditQuery>,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::PROJECT_WRITE]),
    )
    .await?
    .1;
    let string = info.into_inner().0;

    item.validate()
        .map_err(|err| ApiError::Validation(validation_errors_to_string(err, None)))?;

    let project_item = db_models::Project::get(&string, &**pool, &redis)
        .await?
        .ok_or_else(|| {
            ApiError::InvalidInput("The specified project does not exist!".to_string())
        })?;

    if !user.role.is_mod() {
        let (team_member, organization_team_member) =
            db_models::TeamMember::get_for_project_permissions(
                &project_item.inner,
                user.id.into(),
                &**pool,
            )
            .await?;

        // Hide the project
        if team_member.is_none() && organization_team_member.is_none() {
            return Err(ApiError::CustomAuthentication(
                "The specified project does not exist!".to_string(),
            ));
        }
        let permissions = ProjectPermissions::get_permissions_by_role(
            &user.role,
            &team_member,
            &organization_team_member,
        )
        .unwrap_or_default();

        if !permissions.contains(ProjectPermissions::EDIT_DETAILS) {
            return Err(ApiError::CustomAuthentication(
                "You don't have permission to edit this project's gallery.".to_string(),
            ));
        }
    }
    let mut transaction = pool.begin().await?;

    let id = sqlx::query!(
        "
        SELECT id FROM mods_gallery
        WHERE image_url = $1
        ",
        item.url
    )
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or_else(|| {
        ApiError::InvalidInput(format!(
            "Gallery item at URL {} is not part of the project's gallery.",
            item.url
        ))
    })?
    .id;

    let mut transaction = pool.begin().await?;

    if let Some(featured) = item.featured {
        if featured {
            sqlx::query!(
                "
                UPDATE mods_gallery
                SET featured = $2
                WHERE mod_id = $1
                ",
                project_item.inner.id as db_ids::ProjectId,
                false,
            )
            .execute(&mut *transaction)
            .await?;
        }

        sqlx::query!(
            "
            UPDATE mods_gallery
            SET featured = $2
            WHERE id = $1
            ",
            id,
            featured
        )
        .execute(&mut *transaction)
        .await?;
    }
    if let Some(title) = item.title {
        sqlx::query!(
            "
            UPDATE mods_gallery
            SET title = $2
            WHERE id = $1
            ",
            id,
            title
        )
        .execute(&mut *transaction)
        .await?;
    }
    if let Some(description) = item.description {
        sqlx::query!(
            "
            UPDATE mods_gallery
            SET description = $2
            WHERE id = $1
            ",
            id,
            description
        )
        .execute(&mut *transaction)
        .await?;
    }
    if let Some(ordering) = item.ordering {
        sqlx::query!(
            "
            UPDATE mods_gallery
            SET ordering = $2
            WHERE id = $1
            ",
            id,
            ordering
        )
        .execute(&mut *transaction)
        .await?;
    }

    db_models::Project::clear_cache(project_item.inner.id, project_item.inner.slug, None, &redis)
        .await?;

    transaction.commit().await?;

    Ok(HttpResponse::NoContent().body(""))
}

#[derive(Serialize, Deserialize)]
pub struct GalleryDeleteQuery {
    pub url: String,
}

#[delete("{id}/gallery")]
pub async fn delete_gallery_item(
    req: HttpRequest,
    web::Query(item): web::Query<GalleryDeleteQuery>,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    file_host: web::Data<Arc<dyn FileHost + Send + Sync>>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::PROJECT_WRITE]),
    )
    .await?
    .1;
    let string = info.into_inner().0;

    let project_item = db_models::Project::get(&string, &**pool, &redis)
        .await?
        .ok_or_else(|| {
            ApiError::InvalidInput("The specified project does not exist!".to_string())
        })?;

    if !user.role.is_mod() {
        let (team_member, organization_team_member) =
            db_models::TeamMember::get_for_project_permissions(
                &project_item.inner,
                user.id.into(),
                &**pool,
            )
            .await?;

        // Hide the project
        if team_member.is_none() && organization_team_member.is_none() {
            return Err(ApiError::CustomAuthentication(
                "The specified project does not exist!".to_string(),
            ));
        }

        let permissions = ProjectPermissions::get_permissions_by_role(
            &user.role,
            &team_member,
            &organization_team_member,
        )
        .unwrap_or_default();

        if !permissions.contains(ProjectPermissions::EDIT_DETAILS) {
            return Err(ApiError::CustomAuthentication(
                "You don't have permission to edit this project's gallery.".to_string(),
            ));
        }
    }
    let mut transaction = pool.begin().await?;

    let id = sqlx::query!(
        "
        SELECT id FROM mods_gallery
        WHERE image_url = $1
        ",
        item.url
    )
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or_else(|| {
        ApiError::InvalidInput(format!(
            "Gallery item at URL {} is not part of the project's gallery.",
            item.url
        ))
    })?
    .id;

    let cdn_url = dotenvy::var("CDN_URL")?;
    let name = item.url.split(&format!("{cdn_url}/")).nth(1);

    if let Some(icon_path) = name {
        file_host.delete_file_version("", icon_path).await?;
    }

    let mut transaction = pool.begin().await?;

    sqlx::query!(
        "
        DELETE FROM mods_gallery
        WHERE id = $1
        ",
        id
    )
    .execute(&mut *transaction)
    .await?;

    db_models::Project::clear_cache(project_item.inner.id, project_item.inner.slug, None, &redis)
        .await?;

    transaction.commit().await?;

    Ok(HttpResponse::NoContent().body(""))
}

#[delete("{id}")]
pub async fn project_delete(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    config: web::Data<SearchConfig>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::PROJECT_DELETE]),
    )
    .await?
    .1;
    let string = info.into_inner().0;

    let project = db_models::Project::get(&string, &**pool, &redis)
        .await?
        .ok_or_else(|| {
            ApiError::InvalidInput("The specified project does not exist!".to_string())
        })?;

    if !user.role.is_admin() {
        let (team_member, organization_team_member) =
            db_models::TeamMember::get_for_project_permissions(
                &project.inner,
                user.id.into(),
                &**pool,
            )
            .await?;

        // Hide the project
        if team_member.is_none() && organization_team_member.is_none() {
            return Err(ApiError::CustomAuthentication(
                "The specified project does not exist!".to_string(),
            ));
        }

        let permissions = ProjectPermissions::get_permissions_by_role(
            &user.role,
            &team_member,
            &organization_team_member,
        )
        .unwrap_or_default();

        if !permissions.contains(ProjectPermissions::DELETE_PROJECT) {
            return Err(ApiError::CustomAuthentication(
                "You don't have permission to delete this project!".to_string(),
            ));
        }
    }

    let mut transaction = pool.begin().await?;
    let context = ImageContext::Project {
        project_id: Some(project.inner.id.into()),
    };
    let uploaded_images = db_models::Image::get_many_contexted(context, &mut transaction).await?;
    for image in uploaded_images {
        image_item::Image::remove(image.id, &mut transaction, &redis).await?;
    }

    sqlx::query!(
        "
        DELETE FROM collections_mods
        WHERE mod_id = $1
        ",
        project.inner.id as db_ids::ProjectId,
    )
    .execute(&mut *transaction)
    .await?;

    let result = db_models::Project::remove(project.inner.id, &mut transaction, &redis).await?;

    transaction.commit().await?;

    delete_from_index(project.inner.id.into(), config).await?;

    if result.is_some() {
        Ok(HttpResponse::NoContent().body(""))
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
}

#[post("{id}/follow")]
pub async fn project_follow(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::USER_WRITE]),
    )
    .await?
    .1;
    let string = info.into_inner().0;

    let result = db_models::Project::get(&string, &**pool, &redis)
        .await?
        .ok_or_else(|| {
            ApiError::InvalidInput("The specified project does not exist!".to_string())
        })?;

    let user_id: db_ids::UserId = user.id.into();
    let project_id: db_ids::ProjectId = result.inner.id;

    if !is_authorized(&result.inner, &Some(user), &pool).await? {
        return Ok(HttpResponse::NotFound().body(""));
    }

    let following = sqlx::query!(
        "
        SELECT EXISTS(SELECT 1 FROM mod_follows mf WHERE mf.follower_id = $1 AND mf.mod_id = $2)
        ",
        user_id as db_ids::UserId,
        project_id as db_ids::ProjectId
    )
    .fetch_one(&**pool)
    .await?
    .exists
    .unwrap_or(false);

    if !following {
        let mut transaction = pool.begin().await?;

        sqlx::query!(
            "
            UPDATE mods
            SET follows = follows + 1
            WHERE id = $1
            ",
            project_id as db_ids::ProjectId,
        )
        .execute(&mut *transaction)
        .await?;

        sqlx::query!(
            "
            INSERT INTO mod_follows (follower_id, mod_id)
            VALUES ($1, $2)
            ",
            user_id as db_ids::UserId,
            project_id as db_ids::ProjectId
        )
        .execute(&mut *transaction)
        .await?;

        transaction.commit().await?;

        Ok(HttpResponse::NoContent().body(""))
    } else {
        Err(ApiError::InvalidInput(
            "You are already following this project!".to_string(),
        ))
    }
}

#[delete("{id}/follow")]
pub async fn project_unfollow(
    req: HttpRequest,
    info: web::Path<(String,)>,
    pool: web::Data<PgPool>,
    redis: web::Data<RedisPool>,
    session_queue: web::Data<AuthQueue>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(
        &req,
        &**pool,
        &redis,
        &session_queue,
        Some(&[Scopes::USER_WRITE]),
    )
    .await?
    .1;
    let string = info.into_inner().0;

    let result = db_models::Project::get(&string, &**pool, &redis)
        .await?
        .ok_or_else(|| {
            ApiError::InvalidInput("The specified project does not exist!".to_string())
        })?;

    let user_id: db_ids::UserId = user.id.into();
    let project_id = result.inner.id;

    let following = sqlx::query!(
        "
        SELECT EXISTS(SELECT 1 FROM mod_follows mf WHERE mf.follower_id = $1 AND mf.mod_id = $2)
        ",
        user_id as db_ids::UserId,
        project_id as db_ids::ProjectId
    )
    .fetch_one(&**pool)
    .await?
    .exists
    .unwrap_or(false);

    if following {
        let mut transaction = pool.begin().await?;

        sqlx::query!(
            "
            UPDATE mods
            SET follows = follows - 1
            WHERE id = $1
            ",
            project_id as db_ids::ProjectId,
        )
        .execute(&mut *transaction)
        .await?;

        sqlx::query!(
            "
            DELETE FROM mod_follows
            WHERE follower_id = $1 AND mod_id = $2
            ",
            user_id as db_ids::UserId,
            project_id as db_ids::ProjectId
        )
        .execute(&mut *transaction)
        .await?;

        transaction.commit().await?;

        Ok(HttpResponse::NoContent().body(""))
    } else {
        Err(ApiError::InvalidInput(
            "You are not following this project!".to_string(),
        ))
    }
}
