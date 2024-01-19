use crate::util::env::parse_var;
use axum::http::HeaderMap;
use std::net::SocketAddr;

pub fn get_ip_addr(addr: &SocketAddr, headers: &HeaderMap) -> String {
    if parse_var("CLOUDFLARE_INTEGRATION").unwrap_or(false) {
        if let Some(header) = headers.get("CF-Connecting-IP") {
            if let Some(header) = header.to_str().ok().map(|x| x.to_string()) {
                return header;
            }
        }
    }

    addr.ip().to_string()
}
