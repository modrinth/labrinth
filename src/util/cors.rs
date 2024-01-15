use tower_http::cors::{Any, CorsLayer};

pub fn default_cors() -> CorsLayer {
    CorsLayer::default()
        .allow_headers(Any)
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_credentials(Any)
        .allow_private_network(Any)
}
