#![allow(dead_code)]

use labrinth::models::feeds::{FeedItem, FeedItemBody};

use crate::common::get_json_val_str;
use itertools::Itertools;
use labrinth::models::v2::projects::LegacyVersion;

pub fn assert_status(response: &actix_web::dev::ServiceResponse, status: actix_http::StatusCode) {
    assert_eq!(response.status(), status, "{:#?}", response.response());
}

pub fn assert_version_ids(versions: &[LegacyVersion], expected_ids: Vec<String>) {
    let version_ids = versions
        .iter()
        .map(|v| get_json_val_str(v.id))
        .collect_vec();
    assert_eq!(version_ids, expected_ids);
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
    assert!(feed.iter().any(|fi| matches!(fi.body, FeedItemBody::ProjectPublished { project_id, .. } if project_id == expected_project_id)), "{:#?}", &feed);
}
pub fn assert_feed_contains_version_created(
    feed: &[FeedItem],
    expected_version_id: labrinth::models::projects::VersionId,
) {
    assert!(feed.iter().any(|fi| matches!(fi.body, FeedItemBody::VersionCreated { version_id, .. } if version_id == expected_version_id)), "{:#?}", &feed);
}
