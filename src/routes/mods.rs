use actix_web::{get, web, HttpResponse};
use crate::models::mods::SearchRequest;

#[get("api/v1/mods")]
pub fn search_endpoint(web::Query(info): web::Query<SearchRequest>) -> HttpResponse {
    //TODO: Fix this line with anyhow
    let body = serde_json::to_string(&search(&info)).unwrap();

    HttpResponse::Ok()
        .content_type("application/json")
        .body(body)
}