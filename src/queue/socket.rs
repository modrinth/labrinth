//! "Database" for Hydra
use dashmap::DashMap;
use tokio::sync::mpsc::UnboundedSender;

pub enum WebSocketMessage {
    Text(String),
    Close,
}

pub struct ActiveSockets {
    pub auth_sockets: DashMap<String, UnboundedSender<WebSocketMessage>>,
}

impl Default for ActiveSockets {
    fn default() -> Self {
        Self {
            auth_sockets: DashMap::new(),
        }
    }
}
