use crate::search::models::{SearchRequest, SearchMod};
use meilisearch_sdk::client::Client;
use meilisearch_sdk::search::Query;

pub mod indexing;
pub mod models;

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

    client.get_index(format!("{}_mods", index).as_ref()).unwrap()
        .search::<SearchMod>(&query).unwrap().hits
}