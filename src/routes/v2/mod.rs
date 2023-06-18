mod admin;
mod auth;
mod moderation;
mod notifications;
pub(crate) mod project_creation;
mod projects;
mod reports;
mod signing_keys;
mod statistics;
mod tags;
mod teams;
mod users;
mod version_creation;
mod version_file;
mod versions;

pub use super::ApiError;

pub fn config(cfg: &mut actix_web::web::ServiceConfig) {
    cfg.service(
        actix_web::web::scope("v2")
            .configure(admin::config)
            .configure(auth::config)
            .configure(moderation::config)
            .configure(notifications::config)
            .configure(project_creation::config)
            .configure(signing_keys::config)
            // SHOULD CACHE
            .configure(projects::config)
            .configure(reports::config)
            // should cache in future
            .configure(statistics::config)
            // should cache in future
            .configure(tags::config)
            // should cache
            .configure(teams::config)
            // should cache
            .configure(users::config)
            // should cache in future
            .configure(version_file::config)
            // SHOULD CACHE
            .configure(versions::config),
    );
}
