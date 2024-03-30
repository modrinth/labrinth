mod notifications;
pub(crate) mod project_creation;
mod projects;
mod reports;
mod statistics;
pub mod tags;
mod teams;
mod threads;
mod users;
mod version_creation;
pub mod version_file;
mod versions;

pub use super::ApiError;
use crate::util::cors::default_cors;
use axum::Router;

pub fn config() -> Router {
    Router::new().nest(
        "/v2",
        Router::new()
            .merge(super::internal::admin::config())
            .merge(super::internal::session::config())
            .merge(super::internal::flows::config())
            .merge(super::internal::pats::config())
            .merge(notifications::config())
            .merge(project_creation::config())
            .merge(projects::config())
            .merge(reports::config())
            .merge(statistics::config())
            .merge(tags::config())
            .merge(teams::config())
            .merge(threads::config())
            .merge(users::config())
            .merge(version_file::config())
            .merge(versions::config())
            .layer(default_cors()),
    )
}
