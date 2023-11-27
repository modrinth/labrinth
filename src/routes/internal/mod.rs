mod admin;
pub mod flows;
pub mod pats;
pub mod session;

pub use super::ApiError;
use super::v3::oauth_clients;
use crate::util::cors::default_cors;

pub fn config(cfg: &mut actix_web::web::ServiceConfig) {
    cfg.service(
        actix_web::web::scope("_internal")
            .wrap(default_cors())
            .configure(admin::config)
            // TODO: write tests that catch these
            .configure(oauth_clients::config)
            .configure(session::config)
            .configure(flows::config)
            .configure(pats::config)

    );
}
