//! "Database" for Hydra
use axum::extract::ws::WebSocket;
use dashmap::DashMap;

pub struct ActiveSockets {
    pub auth_sockets: DashMap<String, WebSocket>,
}

impl Default for ActiveSockets {
    fn default() -> Self {
        Self {
            auth_sockets: DashMap::new(),
        }
    }
}
