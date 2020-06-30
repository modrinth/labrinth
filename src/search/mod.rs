use actix_web::{get, web, HttpResponse};
use meilisearch_sdk::client::Client;
use meilisearch_sdk::document::Document;
use meilisearch_sdk::search::Query;
use serde::{Deserialize, Serialize};

pub mod indexing;

#[derive(Serialize, Deserialize)]
pub struct SearchRequest {
    pub query: Option<String>,
    pub filters: Option<String>,
    pub version: Option<String>,
    pub offset: Option<String>,
    pub index: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SearchMod {
    pub mod_id: i32,
    pub author: String,
    pub title: String,
    pub description: String,
    pub keywords: Vec<String>,
    pub versions: Vec<String>,
    pub downloads: i32,
    pub page_url: String,
    pub icon_url: String,
    pub author_url: String,
    pub date_created: String,
    pub created: i64,
    pub date_modified: String,
    pub updated: i64,
    pub latest_version: String,
    pub empty: String,
}

impl Document for SearchMod {
    type UIDType = i32;

    fn get_uid(&self) -> &Self::UIDType {
        &self.mod_id
    }
}

#[get("api/v1/search")]
pub fn search_endpoint(web::Query(info): web::Query<SearchRequest>) -> HttpResponse {
    //TODO: Fix this line with anyhow
    let body = serde_json::to_string(&search(&info)).unwrap();

    HttpResponse::Ok()
        .content_type("application/json")
        .body(body)
}

fn search(info: &SearchRequest) -> Vec<SearchMod> {
    let client = Client::new("http://localhost:7700", "");

    let search_query: &str;
    let mut filters = String::new();
    let mut offset = 0;
    let mut index = "relevance";

    match info.query.as_ref() {
        Some(q) => search_query = q,
        None => search_query = "{}{}{}",
    }

    if let Some(f) = info.filters.as_ref() {
        filters = f.clone();
    }

    if let Some(v) = info.version.as_ref() {
        if filters.is_empty() {
            filters = v.clone();
        } else {
            filters = format!("({}) AND ({})", filters, v);
        }
    }

    if let Some(o) = info.offset.as_ref() {
        offset = o.parse().unwrap();
    }

    if let Some(s) = info.index.as_ref() {
        index = s;
    }

    let mut query = Query::new(search_query).with_limit(10).with_offset(offset);

    if !filters.is_empty() {
        query = query.with_filters(&filters);
    }

    client
        .get_index(format!("{}_mods", index).as_ref())
        .unwrap()
        .search::<SearchMod>(&query)
        .unwrap()
        .hits
}
