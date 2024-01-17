use crate::database::models::categories::LinkPlatform;
use crate::database::models::{project_item, version_item};
use crate::database::redis::RedisPool;
use crate::file_hosting::FileHost;
use crate::models::projects::{
    Link, MonetizationStatus, ProjectStatus, SearchRequest, Version,
};
use axum::Router;
use crate::models::v2::projects::{DonationLink, LegacyProject, LegacySideType, LegacyVersion};
use crate::models::v2::search::LegacySearchResults;
use crate::queue::session::AuthQueue;
use crate::routes::v3::projects::ProjectIds;
use crate::routes::{v2_reroute, v3, ApiErrorV2};
use crate::search::{search_for_project, SearchConfig, SearchError};
use axum::http::{HeaderMap, StatusCode};
use crate::util::extract::{ConnectInfo, Extension, Json, Query, Path};
use axum::routing::{get, post, patch};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use v3::ApiError;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use validator::Validate;

pub fn config() -> Router {
    Router::new()
        .route("/search", get(project_search))
        .route("/projects_random", get(random_projects_get))
        .route("/projects", get(projects_get).patch(projects_edit))
        .nest(
            "/project",
            Router::new()
                .route("/:id", get(project_get).patch(project_edit).delete(project_delete))
                .route("/:id/check", get(project_get_check))
                .route("/:id/icon", patch(project_icon_edit).delete(delete_project_icon))
                .route("/:id/gallery", post(add_gallery_item).patch(edit_gallery_item).delete(delete_gallery_item))
                .route("/:id/follow", post(project_follow).delete(project_unfollow))
                .route("/:id/members", get(super::teams::team_members_get_project))
                .route("/:id/dependencies", get(dependency_list))
                .route("/:id/versions", get(super::versions::version_list))
                .route("/:id/versions/:version_id", get(super::versions::version_project_get))
        )
}


pub async fn project_search(
    Query(info): Query<SearchRequest>,
    Extension(config): Extension<SearchConfig>,
) -> Result<Json<LegacySearchResults>, SearchError> {
    // Search now uses loader_fields instead of explicit 'client_side' and 'server_side' fields
    // While the backend for this has changed, it doesnt affect much
    // in the API calls except that 'versions:x' is now 'game_versions:x'
    let facets: Option<Vec<Vec<String>>> = if let Some(facets) = info.facets {
        let facets = serde_json::from_str::<Vec<Vec<String>>>(&facets)?;

        // These loaders specifically used to be combined with 'mod' to be a plugin, but now
        // they are their own loader type. We will convert 'mod' to 'mod' OR 'plugin'
        // as it essentially was before.
        let facets = v2_reroute::convert_plugin_loader_facets_v3(facets);

        Some(
            facets
                .into_iter()
                .map(|facet| {
                    facet
                        .into_iter()
                        .map(|facet| {
                            if let Some((key, operator, val)) = parse_facet(&facet) {
                                format!(
                                    "{}{}{}",
                                    match key.as_str() {
                                        "versions" => "game_versions",
                                        "project_type" => "project_types",
                                        "title" => "name",
                                        x => x,
                                    },
                                    operator,
                                    val
                                )
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
    let results = LegacySearchResults::from(results);

    Ok(Json(results))
}

/// Parses a facet into a key, operator, and value
fn parse_facet(facet: &str) -> Option<(String, String, String)> {
    let mut key = String::new();
    let mut operator = String::new();
    let mut val = String::new();

    let mut iterator = facet.chars();
    while let Some(char) = iterator.next() {
        match char {
            ':' | '=' => {
                operator.push(char);
                val = iterator.collect::<String>();
                return Some((key, operator, val));
            }
            '<' | '>' => {
                operator.push(char);
                if let Some(next_char) = iterator.next() {
                    if next_char == '=' {
                        operator.push(next_char);
                    } else {
                        val.push(next_char);
                    }
                }
                val.push_str(&iterator.collect::<String>());
                return Some((key, operator, val));
            }
            ' ' => continue,
            _ => key.push(char),
        }
    }

    None
}

#[derive(Deserialize, Validate)]
pub struct RandomProjects {
    #[validate(range(min = 1, max = 100))]
    pub count: u32,
}

pub async fn random_projects_get(
    Query(count): Query<RandomProjects>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
) -> Result<Json<Vec<LegacyProject>>, ApiErrorV2> {
    let count = v3::projects::RandomProjects { count: count.count };

    let Json(projects) = v3::projects::random_projects_get(
        Query(count),
        Extension(pool.clone()),
        Extension(redis.clone()),
    )
        .await?;
    // Convert response to V2 format
    let legacy_projects = LegacyProject::from_many(projects, &pool, &redis).await.map_err(ApiError::from)?;
    Ok(Json(legacy_projects))
}

pub async fn projects_get(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(ids): Query<ProjectIds>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<Json<Vec<LegacyProject>>, ApiErrorV2> {
    // Call V3 project creation
    let Json(projects) =
        v3::projects::projects_get(
            ConnectInfo(addr),
            headers,
            Query(v3::projects::ProjectIds { ids: ids.ids }),
            Extension(pool.clone()),
            Extension(redis.clone()),
            Extension(session_queue.clone()),
        )
            .await?;

    // Convert response to V2 format
    let legacy_projects = LegacyProject::from_many(projects, &pool, &redis).await.map_err(ApiError::from)?;
    Ok(Json(legacy_projects))
}

pub async fn project_get(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<Json<LegacyProject>, ApiErrorV2> {
    // Convert V2 data to V3 data
    // Call V3 project creation
    let Json(project) = v3::projects::project_get(
        ConnectInfo(addr),
        headers,
        Path(info.clone()),
        Extension(pool.clone()),
        Extension(redis.clone()),
        Extension(session_queue.clone()),
    )
        .await?;

    // Convert response to V2 format
    let version_item = match project.versions.first() {
        Some(vid) => version_item::Version::get((*vid).into(), &pool, &redis).await.map_err(ApiError::from)?,
        None => None,
    };
    let project = LegacyProject::from(project, version_item);
    Ok(Json(project))
}

//checks the validity of a project id or slug
pub async fn project_get_check(
    Path(info): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
) -> Result<Json<serde_json::Value>, ApiErrorV2> {
    // Returns an id only, do not need to convert
    Ok(v3::projects::project_get_check(
        Path(info),
        Extension(pool),
        Extension(redis),
    )
        .await?)
}

#[derive(Serialize)]
pub struct DependencyInfo {
    pub projects: Vec<LegacyProject>,
    pub versions: Vec<LegacyVersion>,
}

pub async fn dependency_list(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<Json<DependencyInfo>, ApiErrorV2> {
    // TODO: tests, probably
    let Json(dependency_info) =
        v3::projects::dependency_list(
            ConnectInfo(addr),
            headers,
            Path(info.clone()),
            Extension(pool.clone()),
            Extension(redis.clone()),
            Extension(session_queue.clone()),
        )
            .await?;

        let converted_projects =
            LegacyProject::from_many(dependency_info.projects, &pool, &redis).await.map_err(ApiError::from)?;
        let converted_versions = dependency_info
            .versions
            .into_iter()
            .map(LegacyVersion::from)
            .collect();

        Ok(Json(DependencyInfo {
            projects: converted_projects,
            versions: converted_versions,
        }))
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
    pub client_side: Option<LegacySideType>,
    pub server_side: Option<LegacySideType>,
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

pub async fn project_edit(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(search_config): Extension<SearchConfig>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
    Json(v2_new_project): Json<EditProject>,
) -> Result<StatusCode, ApiErrorV2> {
    let client_side = v2_new_project.client_side;
    let server_side = v2_new_project.server_side;
    let new_slug = v2_new_project.slug.clone();

    // TODO: Some kind of handling here to ensure project type is fine.
    // We expect the version uploaded to be of loader type modpack, but there might  not be a way to check here for that.
    // After all, theoretically, they could be creating a genuine 'fabric' mod, and modpack no longer carries information on whether its a mod or modpack,
    // as those are out to the versions.

    // Ideally this would, if the project 'should' be a modpack:
    // - change the loaders to mrpack only
    // - add categories to the project for the corresponding loaders

    let mut new_links = HashMap::new();
    if let Some(issues_url) = v2_new_project.issues_url {
        if let Some(issues_url) = issues_url {
            new_links.insert("issues".to_string(), Some(issues_url));
        } else {
            new_links.insert("issues".to_string(), None);
        }
    }

    if let Some(source_url) = v2_new_project.source_url {
        if let Some(source_url) = source_url {
            new_links.insert("source".to_string(), Some(source_url));
        } else {
            new_links.insert("source".to_string(), None);
        }
    }

    if let Some(wiki_url) = v2_new_project.wiki_url {
        if let Some(wiki_url) = wiki_url {
            new_links.insert("wiki".to_string(), Some(wiki_url));
        } else {
            new_links.insert("wiki".to_string(), None);
        }
    }

    if let Some(discord_url) = v2_new_project.discord_url {
        if let Some(discord_url) = discord_url {
            new_links.insert("discord".to_string(), Some(discord_url));
        } else {
            new_links.insert("discord".to_string(), None);
        }
    }

    // In v2, setting donation links resets all other donation links
    // (resetting to the new ones)
    if let Some(donation_urls) = v2_new_project.donation_urls {
        // Fetch current donation links from project so we know what to delete
        let fetched_example_project = project_item::Project::get(&info, &pool, &redis).await.map_err(ApiError::from)?;
        let donation_links = fetched_example_project
            .map(|x| {
                x.urls
                    .into_iter()
                    .filter_map(|l| {
                        if l.donation {
                            Some(Link::from(l)) // TODO: tests
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        // Set existing donation links to None
        for old_link in donation_links {
            new_links.insert(old_link.platform, None);
        }

        // Add new donation links
        for donation_url in donation_urls {
            new_links.insert(donation_url.id, Some(donation_url.url));
        }
    }

    let new_project = v3::projects::EditProject {
        name: v2_new_project.title,
        summary: v2_new_project.description, // Description becomes summary
        description: v2_new_project.body,    // Body becomes description
        categories: v2_new_project.categories,
        additional_categories: v2_new_project.additional_categories,
        license_url: v2_new_project.license_url,
        link_urls: Some(new_links),
        license_id: v2_new_project.license_id,
        slug: v2_new_project.slug,
        status: v2_new_project.status,
        requested_status: v2_new_project.requested_status,
        moderation_message: v2_new_project.moderation_message,
        moderation_message_body: v2_new_project.moderation_message_body,
        monetization_status: v2_new_project.monetization_status,
    };

    // This returns 204 or failure so we don't need to do anything with it
    let project_id = info.clone();
    let mut response = v3::projects::project_edit(
        ConnectInfo(addr),
        headers.clone(),
        Path(info),
        Extension(pool.clone()),
        Extension(search_config.clone()),
        Extension(redis.clone()),
        Extension(session_queue.clone()),
        Json(new_project),
    )
    .await?;

    // If client and server side were set, we will call
    // the version setting route for each version to set the side types for each of them.
    if response.is_success() && (client_side.is_some() || server_side.is_some()) {
        let project_item =
            project_item::Project::get(&new_slug.unwrap_or(project_id), &pool, &redis).await.map_err(ApiError::from)?;
        let version_ids = project_item.map(|x| x.versions).unwrap_or_default();
        let versions = version_item::Version::get_many(&version_ids, &pool, &redis).await.map_err(ApiError::from)?;
        for version in versions {
            let version = Version::from(version);
            let mut fields = version.fields;
            let (current_client_side, current_server_side) =
                v2_reroute::convert_side_types_v2(&fields, None);
            let client_side = client_side.unwrap_or(current_client_side);
            let server_side = server_side.unwrap_or(current_server_side);
            fields.extend(v2_reroute::convert_side_types_v3(client_side, server_side));

            response = v3::versions::version_edit(
                ConnectInfo(addr),
                headers.clone(),
                Path(version.id),
                Extension(pool.clone()),
                Extension(redis.clone()),
                Extension(session_queue.clone()),
                Json(v3::versions::EditVersion {
                    fields,
                    ..Default::default()
                }),
            ).await?;
        }
    }
    Ok(response)
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

pub async fn projects_edit(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(ids): Query<ProjectIds>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
    Json(bulk_edit_project): Json<BulkEditProject>,
) -> Result<StatusCode, ApiErrorV2> {
    let mut link_urls = HashMap::new();

    // If we are *setting* donation links, we will set every possible donation link to None, as
    // setting will delete all of them then 're-add' the ones we want to keep
    if let Some(donation_url) = bulk_edit_project.donation_urls {
        let link_platforms = LinkPlatform::list(&pool, &redis).await.map_err(ApiError::from)?;
        for link in link_platforms {
            if link.donation {
                link_urls.insert(link.name, None);
            }
        }
        // add
        for donation_url in donation_url {
            link_urls.insert(donation_url.id, Some(donation_url.url));
        }
    }

    // For every delete, we will set the link to None
    if let Some(donation_url) = bulk_edit_project.remove_donation_urls {
        for donation_url in donation_url {
            link_urls.insert(donation_url.id, None);
        }
    }

    // For every add, we will set the link to the new url
    if let Some(donation_url) = bulk_edit_project.add_donation_urls {
        for donation_url in donation_url {
            link_urls.insert(donation_url.id, Some(donation_url.url));
        }
    }

    if let Some(issue_url) = bulk_edit_project.issues_url {
        if let Some(issue_url) = issue_url {
            link_urls.insert("issues".to_string(), Some(issue_url));
        } else {
            link_urls.insert("issues".to_string(), None);
        }
    }

    if let Some(source_url) = bulk_edit_project.source_url {
        if let Some(source_url) = source_url {
            link_urls.insert("source".to_string(), Some(source_url));
        } else {
            link_urls.insert("source".to_string(), None);
        }
    }

    if let Some(wiki_url) = bulk_edit_project.wiki_url {
        if let Some(wiki_url) = wiki_url {
            link_urls.insert("wiki".to_string(), Some(wiki_url));
        } else {
            link_urls.insert("wiki".to_string(), None);
        }
    }

    if let Some(discord_url) = bulk_edit_project.discord_url {
        if let Some(discord_url) = discord_url {
            link_urls.insert("discord".to_string(), Some(discord_url));
        } else {
            link_urls.insert("discord".to_string(), None);
        }
    }

    Ok(v3::projects::projects_edit(
        ConnectInfo(addr),
        headers,
        Query(ids),
        Extension(pool.clone()),
        Extension(redis.clone()),
        Extension(session_queue.clone()),
        Json(v3::projects::BulkEditProject {
            categories: bulk_edit_project.categories,
            add_categories: bulk_edit_project.add_categories,
            remove_categories: bulk_edit_project.remove_categories,
            additional_categories: bulk_edit_project.additional_categories,
            add_additional_categories: bulk_edit_project.add_additional_categories,
            remove_additional_categories: bulk_edit_project.remove_additional_categories,
            link_urls: Some(link_urls),
        }),
    )
    .await?)
}

#[derive(Serialize, Deserialize)]
pub struct FileExt {
    pub ext: String,
}

#[allow(clippy::too_many_arguments)]
pub async fn project_icon_edit(
    Query(ext): Query<FileExt>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(file_host): Extension<Arc<dyn FileHost + Send + Sync>>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
    payload: bytes::Bytes,
) -> Result<StatusCode, ApiErrorV2> {
    
    Ok(v3::projects::project_icon_edit(
        Query(v3::projects::FileExt { ext: ext.ext }),
        ConnectInfo(addr),
        headers,
        Path(info),
        Extension(pool),
        Extension(redis),
        Extension(file_host),
        Extension(session_queue),
        payload,
    )
    .await?)
}

pub async fn delete_project_icon(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(file_host): Extension<Arc<dyn FileHost + Send + Sync>>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<StatusCode, ApiErrorV2> {
    
    Ok(v3::projects::delete_project_icon(
        ConnectInfo(addr),
        headers,
        Path(info),
        Extension(pool),
        Extension(redis),
        Extension(file_host),
        Extension(session_queue),
    )
        .await?)
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

#[allow(clippy::too_many_arguments)]
pub async fn add_gallery_item(
    Query(ext): Query<FileExt>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(item): Query<GalleryCreateQuery>,
    Path(info): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(file_host): Extension<Arc<dyn FileHost + Send + Sync>>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
    payload: bytes::Bytes,
) -> Result<StatusCode, ApiErrorV2> {
    
    Ok(v3::projects::add_gallery_item(
        Query(v3::projects::FileExt { ext: ext.ext }),
        ConnectInfo(addr),
        headers,
        Query(v3::projects::GalleryCreateQuery {
            featured: item.featured,
            name: item.title,
            description: item.description,
            ordering: item.ordering,
        }),
        Path(info),
        Extension(pool),
        Extension(redis),
        Extension(file_host),
        Extension(session_queue),
        payload,
    )
    .await?)
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

pub async fn edit_gallery_item(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(item): Query<GalleryEditQuery>,
    Path(info): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<StatusCode, ApiErrorV2> {
    
    Ok(v3::projects::edit_gallery_item(
        ConnectInfo(addr),
        headers,
        Query(v3::projects::GalleryEditQuery {
            url: item.url,
            featured: item.featured,
            name: item.title,
            description: item.description,
            ordering: item.ordering,
        }),
        Path(info),
        Extension(pool),
        Extension(redis),
        Extension(session_queue),
    )
    .await?)
}

#[derive(Serialize, Deserialize)]
pub struct GalleryDeleteQuery {
    pub url: String,
}

pub async fn delete_gallery_item(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(item): Query<GalleryDeleteQuery>,
    Path(info): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(file_host): Extension<Arc<dyn FileHost + Send + Sync>>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<StatusCode, ApiErrorV2> {
    
    Ok(v3::projects::delete_gallery_item(
        ConnectInfo(addr),
        headers,
        Query(v3::projects::GalleryDeleteQuery { url: item.url }),
        Path(info),
        Extension(pool),
        Extension(redis),
        Extension(file_host),
        Extension(session_queue),
    )
    .await?)
}

pub async fn project_delete(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(search_config): Extension<SearchConfig>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<StatusCode, ApiErrorV2> {
    
    Ok(v3::projects::project_delete(
        ConnectInfo(addr),
        headers,
        Path(info),
        Extension(pool),
        Extension(redis),
        Extension(search_config),
        Extension(session_queue),
    )
        .await?)
}

pub async fn project_follow(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<StatusCode, ApiErrorV2> {
    
    Ok(v3::projects::project_follow(
        ConnectInfo(addr),
        headers,
        Path(info),
        Extension(pool),
        Extension(redis),
        Extension(session_queue),
    )
        .await?)
}

pub async fn project_unfollow(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(info): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(redis): Extension<RedisPool>,
    Extension(session_queue): Extension<Arc<AuthQueue>>,
) -> Result<StatusCode, ApiErrorV2> {
    
    Ok(v3::projects::project_unfollow(
        ConnectInfo(addr),
        headers,
        Path(info),
        Extension(pool),
        Extension(redis),
        Extension(session_queue),

            )
        .await?)
}
