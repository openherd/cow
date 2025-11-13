use crate::types::{Envelope, KarmaCode, ModerationReport};
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
        Self {
            failures: 0,
            last_ok: None,
        }
    }
}

pub struct AppState {
    pub memory: HashMap<String, Envelope>,
    pub db: sled::Db,
    pub peers: HashMap<String, PeerStatus>,

    pub karma_codes: HashMap<String, KarmaCode>,
    pub karma_votes: HashMap<String, i32>,

    pub moderation_reports: Vec<ModerationReport>,
    pub post_labels: HashMap<String, String>,
    pub label_definitions: HashMap<String, String>,

    pub admin_passwords: Vec<String>,
}

impl AppState {
    pub fn new(db: sled::Db) -> Self {
        Self {
            memory: HashMap::new(),
            db,
            peers: HashMap::new(),
            karma_codes: HashMap::new(),
            karma_votes: HashMap::new(),
            moderation_reports: Vec::new(),
            post_labels: HashMap::new(),
            label_definitions: HashMap::new(),
            admin_passwords: Vec::new(),
        }
    }

    pub fn is_admin(&self, password: &str) -> bool {
        self.admin_passwords.iter().any(|p| p == password)
    }
}

pub type SharedState = std::sync::Arc<std::sync::Mutex<AppState>>;
