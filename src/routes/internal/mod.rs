pub(crate) mod admin;
pub mod flows;
pub mod pats;
pub mod session;

use super::v3::oauth_clients;
pub use super::ApiError;
use crate::util::cors::default_cors;
use axum::Router;

pub fn config() -> Router {
    Router::new().nest(
        "/_internal",
        Router::new()
            .merge(admin::config())
            .merge(oauth_clients::config())
            .merge(session::config())
            .merge(flows::config())
            .merge(pats::config())
            .layer(default_cors()),
    )
}
