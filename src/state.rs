use crate::types::Envelope;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerStatus {
    pub failures: u8,
    pub last_ok: Option<DateTime<Utc>>,
}

impl Default for PeerStatus {
    fn default() -> Self {
        Self { failures: 0, last_ok: None }
    }
}

pub struct AppState {
    pub memory: HashMap<String, Envelope>,
    pub db: sled::Db,
    pub peers: HashMap<String, PeerStatus>,
}

impl AppState {
    pub fn new(db: sled::Db) -> Self {
        Self { memory: HashMap::new(), db, peers: HashMap::new() }
    }
}

pub type SharedState = std::sync::Arc<std::sync::Mutex<AppState>>;
