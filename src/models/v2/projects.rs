use std::convert::TryFrom;

use std::collections::HashMap;

use super::super::ids::OrganizationId;
use super::super::teams::TeamId;
use super::super::users::UserId;
use crate::database;
use crate::database::models::DatabaseError;
use crate::database::redis::RedisPool;
use crate::models::ids::{ProjectId, VersionId};
use crate::models::projects::{
    Dependency, License, Link, Loader, ModeratorMessage, MonetizationStatus, Project,
    ProjectStatus, Version, VersionFile, VersionStatus, VersionType,
};
use crate::models::threads::ThreadId;
use crate::queue::session::AuthQueue;
use crate::routes::v2_reroute::capitalize_first;
use crate::routes::v3::versions::{VersionListFilters, VersionListFiltersWithProjects};
use crate::routes::{v2_reroute, v3};
use actix_web::web::Data;
use actix_web::{web, HttpRequest};
use chrono::{DateTime, Utc};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use validator::Validate;

/// A project returned from the API
#[derive(Serialize, Deserialize, Clone)]
pub struct LegacyProject {
    /// Relevant V2 fields- these were removed or modfified in V3,
    /// and are now part of the dynamic fields system
    /// The support range for the client project*
    pub client_side: LegacySideType,
    /// The support range for the server project
    pub server_side: LegacySideType,
    /// A list of game versions this project supports
    pub game_versions: Vec<String>,

    // All other fields are the same as V3
    // If they change, or their constituent types change, we may need to
    // add a new struct for them here.
    pub id: ProjectId,
    pub slug: Option<String>,
    pub project_type: String,
    pub team: TeamId,
    pub organization: Option<OrganizationId>,
    pub title: String,
    pub description: String,
    pub body: String,
    pub body_url: Option<String>,
    pub published: DateTime<Utc>,
    pub updated: DateTime<Utc>,
    pub approved: Option<DateTime<Utc>>,
    pub queued: Option<DateTime<Utc>>,
    pub status: ProjectStatus,
    pub requested_status: Option<ProjectStatus>,
    pub moderator_message: Option<ModeratorMessage>,
    pub license: License,
    pub downloads: u32,
    pub followers: u32,
    pub categories: Vec<String>,
    pub additional_categories: Vec<String>,
    pub loaders: Vec<String>,
    pub versions: Vec<VersionId>,
    pub icon_url: Option<String>,
    pub issues_url: Option<String>,
    pub source_url: Option<String>,
    pub wiki_url: Option<String>,
    pub discord_url: Option<String>,
    pub donation_urls: Option<Vec<DonationLink>>,
    pub gallery: Vec<LegacyGalleryItem>,
    pub color: Option<u32>,
    pub thread_id: ThreadId,
    pub monetization_status: MonetizationStatus,
}

impl LegacyProject {
    // Convert from a standard V3 project to a V2 project
    // Requires any queried versions to be passed in, to get access to certain version fields contained within.
    // - This can be any version, because the fields are ones that used to be on the project itself.
    // - Its conceivable that certain V3 projects that have many different ones may not have the same fields on all of them.
    // TODO: Should this return an error instead for v2 users?
    // It's safe to use a db version_item for this as the only info is side types, game versions, and loader fields (for loaders), which used to be public on project anyway.
    fn from_inner(
        data: Project,
        versions_option: Option<Version>,
        visible_version_ids: Vec<database::models::ids::VersionId>,
    ) -> Self {
        let mut client_side = LegacySideType::Unknown;
        let mut server_side = LegacySideType::Unknown;
        let mut game_versions = Vec::new();

        // V2 versions only have one project type- v3 versions can rarely have multiple.
        // We'll prioritize 'modpack' first, then 'mod', and if neither are found, use the first one.
        // If there are no project types, default to 'project'
        let mut project_types = data.project_types;
        if project_types.contains(&"modpack".to_string()) {
            project_types = vec!["modpack".to_string()];
        } else if project_types.contains(&"mod".to_string()) {
            project_types = vec!["mod".to_string()];
        }
        let project_type = project_types
            .first()
            .cloned()
            .unwrap_or("project".to_string()); // Default to 'project' if none are found

        let mut project_type = if project_type == "datapack" || project_type == "plugin" {
            // These are not supported in V2, so we'll just use 'mod' instead
            "mod".to_string()
        } else {
            project_type
        };

        let mut loaders = data.loaders;

        if let Some(versions_item) = versions_option {
            game_versions = versions_item
                .fields
                .iter()
                .find_map(|(name, value)| {
                    if *name == "game_versions" {
                        value.as_array().map(|v| {
                            v.iter()
                                .filter_map(|gv| gv.as_str().map(|gv| gv.to_string()))
                                .collect::<Vec<_>>()
                        })
                    } else {
                        None
                    }
                })
                .unwrap_or(Vec::new());

            // Extract side types from remaining fields (singleplayer, client_only, etc)
            (client_side, server_side) = v2_reroute::convert_side_types_v2(&versions_item.fields);

            // - if loader is mrpack, this is a modpack
            // the loaders are whatever the corresponding loader fields are
            if versions_item
                .loaders
                .contains(&Loader("mrpack".to_string()))
            {
                project_type = "modpack".to_string();
                if let Some(mrpack_loaders) = data.fields.iter().find(|f| f.0 == "mrpack_loaders") {
                    let values = mrpack_loaders
                        .1
                        .iter()
                        .filter_map(|v| v.as_str())
                        .map(|v| v.to_string())
                        .collect::<Vec<_>>();

                    // drop mrpack from loaders
                    loaders = loaders
                        .into_iter()
                        .filter(|l| l != "mrpack")
                        .collect::<Vec<_>>();
                    // and replace with mrpack_loaders
                    loaders.extend(values);
                    // remove duplicate loaders
                    loaders = loaders.into_iter().unique().collect::<Vec<_>>();
                }
            }
        }

        let issues_url = data.link_urls.get("issues").map(|l| l.url.clone());
        let source_url = data.link_urls.get("source").map(|l| l.url.clone());
        let wiki_url = data.link_urls.get("wiki").map(|l| l.url.clone());
        let discord_url = data.link_urls.get("discord").map(|l| l.url.clone());

        let donation_urls = data
            .link_urls
            .iter()
            .filter(|(_, l)| l.donation)
            .map(|(_, l)| DonationLink::try_from(l.clone()).ok())
            .collect::<Option<Vec<_>>>();

        Self {
            id: data.id,
            slug: data.slug,
            project_type,
            team: data.team_id,
            organization: data.organization,
            title: data.name,
            description: data.summary, // V2 description is V3 summary
            body: data.description,    // V2 body is V3 description
            body_url: None,            // Always None even in V2
            published: data.published,
            updated: data.updated,
            approved: data.approved,
            queued: data.queued,
            status: data.status,
            requested_status: data.requested_status,
            moderator_message: data.moderator_message,
            license: data.license,
            downloads: data.downloads,
            followers: data.followers,
            categories: data.categories,
            additional_categories: data.additional_categories,
            loaders,
            versions: visible_version_ids.into_iter().map(|i| i.into()).collect(),
            icon_url: data.icon_url,
            issues_url,
            source_url,
            wiki_url,
            discord_url,
            donation_urls,
            gallery: data
                .gallery
                .into_iter()
                .map(LegacyGalleryItem::from)
                .collect(),
            color: data.color,
            thread_id: data.thread_id,
            monetization_status: data.monetization_status,
            client_side,
            server_side,
            game_versions,
        }
    }

    pub async fn from(
        req: HttpRequest,
        client: Data<PgPool>,
        redis: Data<RedisPool>,
        session_queue: Data<AuthQueue>,
        project: Project,
    ) -> Result<Self, DatabaseError> {
        // Call v3 project version get
        let project_ids = serde_json::to_string(&[project.id])?;
        let found_versions = match v3::versions::version_list_inner(
            req,
            web::Query(VersionListFiltersWithProjects {
                ids: project_ids,
                filters: VersionListFilters::default(),
            }),
            client.clone(),
            redis.clone(),
            session_queue.clone(),
        )
        .await
        {
            Ok(versions) => versions,
            Err(_) => vec![],
        };

        let version_ids = found_versions
            .iter()
            .map(|v| v.id.into())
            .collect::<Vec<_>>();
        let version_item = found_versions.into_iter().next().take();
        let project = LegacyProject::from_inner(project, version_item, version_ids);
        Ok(project)
    }

    pub async fn from_many(
        req: HttpRequest,
        client: Data<PgPool>,
        redis: Data<RedisPool>,
        session_queue: Data<AuthQueue>,
        projects: Vec<Project>,
    ) -> Result<Vec<Self>, DatabaseError> {
        let project_ids =
            serde_json::to_string(&projects.iter().map(|p| p.id).collect::<Vec<_>>())?;
        // Call v3 project version get
        let found_versions = v3::versions::version_list_inner(
            req,
            web::Query(VersionListFiltersWithProjects {
                ids: project_ids,
                filters: VersionListFilters::default(),
            }),
            client.clone(),
            redis.clone(),
            session_queue.clone(),
        )
        .await
        .unwrap_or_default();

        let proj_version_hashmap = found_versions.into_iter().fold(
            HashMap::new(),
            |mut acc: HashMap<ProjectId, Vec<_>>, version| {
                acc.entry(version.project_id).or_default().push(version);
                acc
            },
        );

        Ok(projects
            .into_iter()
            .map(|project| {
                let found_version = proj_version_hashmap
                    .get(&project.id)
                    .cloned()
                    .unwrap_or_default();
                let version_ids = found_version
                    .iter()
                    .map(|v| v.id.into())
                    .collect::<Vec<_>>();
                let version_item = found_version.into_iter().next().take();
                LegacyProject::from_inner(project, version_item, version_ids)
            })
            .collect())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, Copy)]
#[serde(rename_all = "kebab-case")]
pub enum LegacySideType {
    Required,
    Optional,
    Unsupported,
    Unknown,
}

impl std::fmt::Display for LegacySideType {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(fmt, "{}", self.as_str())
    }
}

impl LegacySideType {
    // These are constant, so this can remove unneccessary allocations (`to_string`)
    pub fn as_str(&self) -> &'static str {
        match self {
            LegacySideType::Required => "required",
            LegacySideType::Optional => "optional",
            LegacySideType::Unsupported => "unsupported",
            LegacySideType::Unknown => "unknown",
        }
    }

    pub fn from_string(string: &str) -> LegacySideType {
        match string {
            "required" => LegacySideType::Required,
            "optional" => LegacySideType::Optional,
            "unsupported" => LegacySideType::Unsupported,
            _ => LegacySideType::Unknown,
        }
    }
}

/// A specific version of a project
#[derive(Serialize, Deserialize, Clone)]
pub struct LegacyVersion {
    /// Relevant V2 fields- these were removed or modfified in V3,
    /// and are now part of the dynamic fields system
    /// A list of game versions this project supports
    pub game_versions: Vec<String>,

    /// A list of loaders this project supports (has a newtype struct)
    pub loaders: Vec<Loader>,

    // TODO: should we remove this? as this is a v3 field and tests for it should be isolated to v3
    // it allows us to keep tests that use this struct in common
    pub ordering: Option<i32>,

    pub id: VersionId,
    pub project_id: ProjectId,
    pub author_id: UserId,
    pub featured: bool,
    pub name: String,
    pub version_number: String,
    pub changelog: String,
    pub changelog_url: Option<String>,
    pub date_published: DateTime<Utc>,
    pub downloads: u32,
    pub version_type: VersionType,
    pub status: VersionStatus,
    pub requested_status: Option<VersionStatus>,
    pub files: Vec<VersionFile>,
    pub dependencies: Vec<Dependency>,
}

impl From<Version> for LegacyVersion {
    fn from(data: Version) -> Self {
        let mut game_versions = Vec::new();
        if let Some(value) = data.fields.get("game_versions").and_then(|v| v.as_array()) {
            for gv in value {
                if let Some(game_version) = gv.as_str() {
                    game_versions.push(game_version.to_string());
                }
            }
        }

        // - if loader is mrpack, this is a modpack
        // the v2 loaders are whatever the corresponding loader fields are
        let mut loaders = data.loaders.into_iter().map(|l| l.0).collect::<Vec<_>>();
        if loaders.contains(&"mrpack".to_string()) {
            if let Some((_, mrpack_loaders)) = data
                .fields
                .into_iter()
                .find(|(key, _)| key == "mrpack_loaders")
            {
                if let Ok(mrpack_loaders) = serde_json::from_value(mrpack_loaders) {
                    loaders = mrpack_loaders;
                }
            }
        }
        let loaders = loaders.into_iter().map(Loader).collect::<Vec<_>>();

        Self {
            id: data.id,
            project_id: data.project_id,
            author_id: data.author_id,
            featured: data.featured,
            name: data.name,
            version_number: data.version_number,
            changelog: data.changelog,
            changelog_url: None, // Always None even in V2
            date_published: data.date_published,
            downloads: data.downloads,
            version_type: data.version_type,
            status: data.status,
            requested_status: data.requested_status,
            files: data.files,
            dependencies: data.dependencies,
            game_versions,
            ordering: data.ordering,
            loaders,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LegacyGalleryItem {
    pub url: String,
    pub featured: bool,
    pub title: Option<String>,
    pub description: Option<String>,
    pub created: DateTime<Utc>,
    pub ordering: i64,
}

impl LegacyGalleryItem {
    fn from(data: crate::models::projects::GalleryItem) -> Self {
        Self {
            url: data.url,
            featured: data.featured,
            title: data.name,
            description: data.description,
            created: data.created,
            ordering: data.ordering,
        }
    }
}

#[derive(Serialize, Deserialize, Validate, Clone, Eq, PartialEq)]
pub struct DonationLink {
    pub id: String,
    pub platform: String,
    #[validate(
        custom(function = "crate::util::validate::validate_url"),
        length(max = 2048)
    )]
    pub url: String,
}

impl TryFrom<Link> for DonationLink {
    type Error = String;
    fn try_from(link: Link) -> Result<Self, String> {
        if !link.donation {
            return Err("Not a donation".to_string());
        }
        Ok(Self {
            platform: capitalize_first(&link.platform),
            url: link.url,
            id: link.platform,
        })
    }
}
