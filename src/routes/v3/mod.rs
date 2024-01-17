pub use super::ApiError;
use crate::util::cors::default_cors;
use axum::Router;

pub mod analytics_get;
pub mod collections;
pub mod images;
pub mod moderation;
pub mod notifications;
pub mod organizations;
pub mod payouts;
pub mod project_creation;
pub mod projects;
pub mod reports;
pub mod statistics;
pub mod tags;
pub mod teams;
pub mod threads;
pub mod users;
pub mod version_creation;
pub mod version_file;
pub mod versions;

pub mod oauth_clients;

pub fn config() -> Router {
    Router::new().nest(
        "/v3",
        Router::new()
            .merge(analytics_get::config())
            .merge(collections::config())
            .merge(images::config())
            .merge(moderation::config())
            .merge(notifications::config())
            .merge(organizations::config())
            //todo: .merge(project_creation::config())
            .merge(projects::config())
            .merge(reports::config())
            .merge(statistics::config())
            .merge(tags::config())
            .merge(teams::config())
            .merge(threads::config())
            .merge(users::config())
            .merge(version_file::config())
            .merge(payouts::config())
            .merge(versions::config())
            .layer(default_cors()),
    )
}
