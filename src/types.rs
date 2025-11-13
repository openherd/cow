use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoRegion {
    pub lat: f64,
    pub lon: f64,
    pub radius_km: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KarmaCode {
    pub code: String,
    pub issuer: String,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub vote_type: Option<String>,
    pub expires: DateTime<Utc>,
    pub region: Option<GeoRegion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_post: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_direction: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KarmaMetadata {
    pub code: String,
    pub expires: DateTime<Utc>,
    #[serde(rename = "currentPost")]
    pub current_post: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KarmaLookupRequest {
    pub posts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KarmaGenerateRequest {
    pub count: u32,
    pub issuer: String,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub vote_type: Option<String>,
    pub expires: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<GeoRegion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModerationLabel {
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModerationReport {
    pub post: Envelope,
    pub reason: String,
    #[serde(skip)]
    pub reported_at: DateTime<Utc>,
    #[serde(skip)]
    pub reporter_ip: Option<String>,
    #[serde(skip)]
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModerationLookupRequest {
    pub posts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModerationAction {
    pub report_id: String,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminAuth {
    pub password: String,
}
