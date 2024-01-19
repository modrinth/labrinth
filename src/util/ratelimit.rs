use crate::routes::ApiError;
use crate::util::extract::{ConnectInfo, Extension};
use crate::util::ip::get_ip_addr;
use axum::extract::Request;
use axum::http::HeaderMap;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use governor::clock::{Clock, DefaultClock};
use governor::{middleware, state, RateLimiter};
use std::net::SocketAddr;
use std::sync::Arc;

pub type KeyedRateLimiter<K = String, MW = middleware::StateInformationMiddleware> =
    RateLimiter<K, state::keyed::DefaultKeyedStateStore<K>, DefaultClock, MW>;

pub async fn ratelimit(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(rate_limiter): Extension<Arc<KeyedRateLimiter>>,
    request: Request,
    next: Next,
) -> Response {
    if let Some(key) = request.headers().get("x-ratelimit-key") {
        if key.to_str().ok() == dotenvy::var("RATE_LIMIT_IGNORE_KEY").ok().as_deref() {
            return next.run(request).await;
        }
    }

    let ip = get_ip_addr(&addr, &headers);

    match rate_limiter.check_key(&ip) {
        Ok(snapshot) => {
            let mut response = next.run(request).await;

            let headers = response.headers_mut();
            headers.insert(
                "x-ratelimit-limit",
                snapshot.quota().burst_size().get().into(),
            );
            headers.insert(
                "x-ratelimit-remaining",
                snapshot.remaining_burst_capacity().into(),
            );
            headers.insert(
                "x-ratelimit-reset",
                snapshot
                    .quota()
                    .burst_size_replenished_in()
                    .as_secs()
                    .into(),
            );

            response
        }
        Err(negative) => {
            let wait_time = negative.wait_time_from(DefaultClock::default().now());

            let mut response = ApiError::RateLimitError(
                wait_time.as_millis(),
                negative.quota().burst_size().get(),
            )
            .into_response();
            let headers = response.headers_mut();

            headers.insert(
                "x-ratelimit-limit",
                negative.quota().burst_size().get().into(),
            );
            headers.insert("x-ratelimit-remaining", 0.into());
            headers.insert("x-ratelimit-reset", wait_time.as_secs().into());

            response
        }
    }
}
