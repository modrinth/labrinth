#![allow(dead_code)]

use labrinth::models::feed_item::{FeedItem, FeedItemBody};

pub fn assert_status(response: &actix_web::dev::ServiceResponse, status: actix_http::StatusCode) {
    assert_eq!(response.status(), status, "{:#?}", response.response());
}

pub fn assert_any_status_except(
    response: &actix_web::dev::ServiceResponse,
    status: actix_http::StatusCode,
) {
    assert_ne!(response.status(), status, "{:#?}", response.response());
}

pub fn assert_feed_contains_project_created(
    feed: &[FeedItem],
    expected_project_id: labrinth::models::projects::ProjectId,
) {
    assert!(feed.iter().any(|fi| matches!(fi.body, FeedItemBody::ProjectCreated { project_id, .. } if project_id == expected_project_id)), "{:#?}", &feed);
}
