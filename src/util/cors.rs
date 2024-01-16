use crate::util::env::parse_strings_from_var;
use axum::http::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use axum::http::Method;
use std::time::Duration;
use tower_http::cors::{Any, CorsLayer, MaxAge};

pub fn default_cors() -> CorsLayer {
    CorsLayer::very_permissive().max_age(MaxAge::exact(Duration::from_secs(3600)))
}

pub fn analytics_cors() -> CorsLayer {
    let layer = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([AUTHORIZATION, ACCEPT, CONTENT_TYPE])
        .max_age(MaxAge::exact(Duration::from_secs(3600)));

    let allowed_origins = parse_strings_from_var("ANALYTICS_ALLOWED_ORIGINS").unwrap_or_default();

    if allowed_origins.contains(&"*".to_string()) {
        layer.allow_origin(Any)
    } else {
        layer.allow_origin(
            allowed_origins
                .into_iter()
                .map(|x| {
                    x.parse()
                        .expect("Allowed origin did not serialize into header value")
                })
                .collect::<Vec<_>>(),
        )
    }
}
