use crate::database::models::project_item::{GalleryItem, LinkUrl};
use crate::models::error::ApiError;
use crate::models::projects::{MonetizationStatus, ProjectStatus, SearchRequest};
use actix_web::http::StatusCode;
use actix_web::HttpResponse;
use chrono::{DateTime, Utc};
use itertools::Itertools;
use meilisearch_sdk::client::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::Write;
use std::sync::atomic::AtomicU64;
use thiserror::Error;

pub mod indexing;

#[derive(Error, Debug)]
pub enum SearchError {
    #[error("MeiliSearch Error: {0}")]
    MeiliSearch(#[from] meilisearch_sdk::errors::Error),
    #[error("Error while serializing or deserializing JSON: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("Error while parsing an integer: {0}")]
    IntParsing(#[from] std::num::ParseIntError),
    #[error("Error while formatting strings: {0}")]
    FormatError(#[from] std::fmt::Error),
    #[error("Environment Error")]
    Env(#[from] dotenvy::Error),
    #[error("Invalid index to sort by: {0}")]
    InvalidIndex(String),
}

impl actix_web::ResponseError for SearchError {
    fn status_code(&self) -> StatusCode {
        match self {
            SearchError::Env(..) => StatusCode::INTERNAL_SERVER_ERROR,
            SearchError::MeiliSearch(..) => StatusCode::BAD_REQUEST,
            SearchError::Serde(..) => StatusCode::BAD_REQUEST,
            SearchError::IntParsing(..) => StatusCode::BAD_REQUEST,
            SearchError::InvalidIndex(..) => StatusCode::BAD_REQUEST,
            SearchError::FormatError(..) => StatusCode::BAD_REQUEST,
        }
    }

    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status_code()).json(ApiError {
            error: match self {
                SearchError::Env(..) => "environment_error",
                SearchError::MeiliSearch(..) => "meilisearch_error",
                SearchError::Serde(..) => "invalid_input",
                SearchError::IntParsing(..) => "invalid_input",
                SearchError::InvalidIndex(..) => "invalid_input",
                SearchError::FormatError(..) => "invalid_input",
            },
            description: &self.to_string(),
        })
    }
}

#[derive(Debug)]
pub struct SearchConfig {
    pub address: String,
    pub key: String,
    pub meta_namespace: String,

    pub swap_index_id: AtomicU64,
    pub max_swap: u64,
}

impl SearchConfig {
    // Panics if the environment variables are not set,
    // but these are already checked for on startup.
    pub fn new(meta_namespace: Option<String>) -> Self {
        let address = dotenvy::var("MEILISEARCH_ADDR").expect("MEILISEARCH_ADDR not set");
        let key = dotenvy::var("MEILISEARCH_KEY").expect("MEILISEARCH_KEY not set");
        let swap_index_id = AtomicU64::new(0);

        Self {
            address,
            key,
            meta_namespace: meta_namespace.unwrap_or_default(),
            swap_index_id,
            max_swap: 2, // Number of indices to swap between
                         // TODO: make env var
        }
    }

    pub fn make_client(&self) -> Client {
        Client::new(self.address.as_str(), Some(self.key.as_str()))
    }

    // Next: true if we want the next index (we are preparing the next swap), false if we want the current index (searching)
    pub fn get_index_name(&self, index: &str, next: bool) -> String {
        let alt = if next { 1 } else { 0 };
        let index_id = self
            .swap_index_id
            .load(std::sync::atomic::Ordering::Relaxed);
        println!(
            "Getting index name: {}_{}_{}",
            self.meta_namespace,
            index,
            (index_id + alt) % self.max_swap
        );
        format!(
            "{}_{}_{}",
            self.meta_namespace,
            index,
            (index_id + alt) % self.max_swap
        )
    }

    pub fn swap_index(&self) {
        let index_id = self
            .swap_index_id
            .load(std::sync::atomic::Ordering::Relaxed);
        self.swap_index_id.store(
            (index_id + 1) % self.max_swap,
            std::sync::atomic::Ordering::Relaxed,
        );
    }
}

/// A project document used for uploading projects to MeiliSearch's indices.
/// This contains some extra data that is not returned by search results.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct UploadSearchProject {
    pub version_id: String,
    pub project_id: String,
    pub project_types: Vec<String>,
    pub slug: Option<String>,
    pub author: String,
    pub name: String,
    pub summary: String,
    pub categories: Vec<String>,
    pub display_categories: Vec<String>,
    pub follows: i32,
    pub downloads: i32,
    pub icon_url: Option<String>,
    pub license: String,
    pub gallery: Vec<String>,
    pub featured_gallery: Option<String>,
    /// RFC 3339 formatted creation date of the project
    pub date_created: DateTime<Utc>,
    /// Unix timestamp of the creation date of the project
    pub created_timestamp: i64,
    /// RFC 3339 formatted date/time of last major modification (update)
    pub date_modified: DateTime<Utc>,
    /// Unix timestamp of the last major modification
    pub modified_timestamp: i64,
    pub open_source: bool,
    pub color: Option<u32>,

    // Hidden fields to get the Project model out of the search results.
    pub license_url: Option<String>,
    pub monetization_status: Option<MonetizationStatus>,
    pub team_id: String,
    pub thread_id: String,
    pub versions: Vec<String>,
    pub date_published: DateTime<Utc>,
    pub date_queued: Option<DateTime<Utc>>,
    pub status: ProjectStatus,
    pub requested_status: Option<ProjectStatus>,
    pub loaders: Vec<String>, // Search uses loaders as categories- this is purely for the Project model.
    pub links: Vec<LinkUrl>,
    pub gallery_items: Vec<GalleryItem>, // Gallery *only* urls are stored in gallery, but the gallery items are stored here- required for the Project model.
    pub games: Vec<String>,
    pub organization_id: Option<String>,
    pub project_loader_fields: HashMap<String, Vec<serde_json::Value>>, // Aggregation of loader_fields from all versions of the project, allowing for reconstruction of the Project model.

    #[serde(flatten)]
    pub loader_fields: HashMap<String, Vec<serde_json::Value>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SearchResults {
    pub hits: Vec<ResultSearchProject>,
    pub page: usize,
    pub hits_per_page: usize,
    pub total_hits: usize,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ResultSearchProject {
    pub version_id: String,
    pub project_id: String,
    pub project_types: Vec<String>,
    pub slug: Option<String>,
    pub author: String,
    pub name: String,
    pub summary: String,
    pub categories: Vec<String>,
    pub display_categories: Vec<String>,
    pub downloads: i32,
    pub follows: i32,
    pub icon_url: Option<String>,
    /// RFC 3339 formatted creation date of the project
    pub date_created: String,
    /// RFC 3339 formatted modification date of the project
    pub date_modified: String,
    pub license: String,
    pub gallery: Vec<String>,
    pub featured_gallery: Option<String>,
    pub color: Option<u32>,

    // Hidden fields to get the Project model out of the search results.
    pub license_url: Option<String>,
    pub monetization_status: Option<String>,
    pub team_id: String,
    pub thread_id: String,
    pub versions: Vec<String>,
    pub date_published: String,
    pub date_queued: Option<String>,
    pub status: String,
    pub requested_status: Option<String>,
    pub loaders: Vec<String>, // Search uses loaders as categories- this is purely for the Project model.
    pub links: Vec<LinkUrl>,
    pub gallery_items: Vec<GalleryItem>, // Gallery *only* urls are stored in gallery, but the gallery items are stored here- required for the Project model.
    pub games: Vec<String>,
    pub organization_id: Option<String>,
    pub project_loader_fields: HashMap<String, Vec<serde_json::Value>>, // Aggregation of loader_fields from all versions of the project, allowing for reconstruction of the Project model.

    #[serde(flatten)]
    pub loader_fields: HashMap<String, Vec<serde_json::Value>>,
}

pub fn get_sort_index(
    config: &SearchConfig,
    index: &str,
) -> Result<(String, [&'static str; 1]), SearchError> {
    let projects_name = config.get_index_name("projects", false);
    let projects_filtered_name = config.get_index_name("projects_filtered", false);
    println!("Getting sort index: {}", index);
    println!("Projects name: {}", projects_name);
    println!("Projects filtered name: {}", projects_filtered_name);
    Ok(match index {
        "relevance" => (projects_name, ["downloads:desc"]),
        "downloads" => (projects_filtered_name, ["downloads:desc"]),
        "follows" => (projects_name, ["follows:desc"]),
        "updated" => (projects_name, ["date_modified:desc"]),
        "newest" => (projects_name, ["date_created:desc"]),
        i => return Err(SearchError::InvalidIndex(i.to_string())),
    })
}

pub async fn search_for_project(
    info: &SearchRequest,
    config: &SearchConfig,
) -> Result<SearchResults, SearchError> {
    let client = Client::new(&*config.address, Some(&*config.key));

    let offset: usize = info.offset.as_deref().unwrap_or("0").parse()?;
    let index = info.index.as_deref().unwrap_or("relevance");
    let limit = info.limit.as_deref().unwrap_or("10").parse()?;

    let sort = get_sort_index(config, index)?;
    let meilisearch_index = client.get_index(sort.0).await?;

    let mut filter_string = String::new();

    // Convert offset and limit to page and hits_per_page
    let hits_per_page = limit;
    let page = offset / limit + 1;

    let results = {
        let mut query = meilisearch_index.search();
        query
            .with_page(page)
            .with_hits_per_page(hits_per_page)
            .with_query(info.query.as_deref().unwrap_or_default())
            .with_sort(&sort.1);

        if let Some(new_filters) = info.new_filters.as_deref() {
            query.with_filter(new_filters);
        } else {
            let facets = if let Some(facets) = &info.facets {
                Some(serde_json::from_str::<Vec<Vec<Value>>>(facets)?)
            } else {
                None
            };

            let filters: Cow<_> = match (info.filters.as_deref(), info.version.as_deref()) {
                (Some(f), Some(v)) => format!("({f}) AND ({v})").into(),
                (Some(f), None) => f.into(),
                (None, Some(v)) => v.into(),
                (None, None) => "".into(),
            };

            if let Some(facets) = facets {
                // Search can now *optionally* have a third inner array: So Vec(AND)<Vec(OR)<Vec(AND)< _ >>>
                // For every inner facet, we will check if it can be deserialized into a Vec<&str>, and do so.
                // If not, we will assume it is a single facet and wrap it in a Vec.
                let facets: Vec<Vec<Vec<String>>> = facets
                    .into_iter()
                    .map(|facets| {
                        facets
                            .into_iter()
                            .map(|facet| {
                                if facet.is_array() {
                                    serde_json::from_value::<Vec<String>>(facet).unwrap_or_default()
                                } else {
                                    vec![serde_json::from_value::<String>(facet)
                                        .unwrap_or_default()]
                                }
                            })
                            .collect_vec()
                    })
                    .collect_vec();

                filter_string.push('(');
                for (index, facet_outer_list) in facets.iter().enumerate() {
                    filter_string.push('(');

                    for (facet_outer_index, facet_inner_list) in facet_outer_list.iter().enumerate()
                    {
                        filter_string.push('(');
                        for (facet_inner_index, facet) in facet_inner_list.iter().enumerate() {
                            filter_string.push_str(&facet.replace(':', " = "));
                            if facet_inner_index != (facet_inner_list.len() - 1) {
                                filter_string.push_str(" AND ")
                            }
                        }
                        filter_string.push(')');

                        if facet_outer_index != (facet_outer_list.len() - 1) {
                            filter_string.push_str(" OR ")
                        }
                    }

                    filter_string.push(')');

                    if index != (facets.len() - 1) {
                        filter_string.push_str(" AND ")
                    }
                }
                filter_string.push(')');

                if !filters.is_empty() {
                    write!(filter_string, " AND ({filters})")?;
                }
            } else {
                filter_string.push_str(&filters);
            }

            if !filter_string.is_empty() {
                query.with_filter(&filter_string);
            }
        }

        query.execute::<ResultSearchProject>().await?
    };

    Ok(SearchResults {
        hits: results.hits.into_iter().map(|r| r.result).collect(),
        page: results.page.unwrap_or_default(),
        hits_per_page: results.hits_per_page.unwrap_or_default(),
        total_hits: results.total_hits.unwrap_or_default(),
    })
}
