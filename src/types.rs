use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub signature: String,
    #[serde(rename = "publicKey")]
    pub public_key: String,
    pub id: String,
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Post {
    pub id: String,
    pub text: String,
    pub latitude: f64,
    pub longitude: f64,
    pub date: DateTime<Utc>,
    pub parent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiResponse {
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRequest {
    pub address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResponse {
    pub ok: bool,
    pub message: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("Invalid signature")]
    InvalidSignature,
    #[error("Invalid public key")]
    InvalidPublicKey,
    #[error("Post ID does not match key fingerprint")]
    IdMismatch,
    #[error("Invalid post data: {0}")]
    InvalidPostData(String),
    #[error("PGP error: {0}")]
    PgpError(#[from] pgp::errors::Error),
    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),
}


