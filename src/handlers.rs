use crate::{
    state::{SharedState, PeerStatus},
    types::{Envelope, ApiResponse, SyncRequest, SyncResponse},
    validation::validate_envelope,
};
use axum::{extract::State, http::StatusCode, response::Json};
use chrono::Utc;
use reqwest::StatusCode as HttpStatus;
use std::time::Duration;
use url::Url;

pub async fn outbox(State(state): State<SharedState>) -> Result<Json<Vec<Envelope>>, StatusCode> {
    let state = state.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let envelopes: Vec<Envelope> = state.memory.values().cloned().collect();
    Ok(Json(envelopes))
}

pub async fn inbox(
    State(state): State<SharedState>,
    Json(envelopes): Json<Vec<Envelope>>,
) -> Result<Json<ApiResponse>, StatusCode> {
    let mut s = state.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut imported_count = 0;
    let mut errors = Vec::new();

    for envelope in envelopes {
        match validate_envelope(&envelope) {
            Ok(_post) => {
                let id = envelope.id.clone();
               
                match serde_json::to_vec(&envelope) {
                    Ok(bytes) => {
                        if let Err(e) = s.db.insert(id.as_bytes(), bytes) {
                            eprintln!("DB insert error for {}: {}", id, e);
                        }
                    }
                    Err(e) => eprintln!("Serialization error for {}: {}", id, e),
                }
               
                s.memory.insert(id, envelope);
                imported_count += 1;
            }
            Err(e) => {
                errors.push(format!("Error validating post {}: {}", envelope.id, e));
            }
        }
    }

    if imported_count == 0 && !errors.is_empty() {
        eprintln!("All posts failed validation: {:?}", errors);
        return Err(StatusCode::BAD_REQUEST);
    }

    if !errors.is_empty() {
        eprintln!("Some posts failed validation: {:?}", errors);
    }

    println!("Successfully imported {} posts", imported_count);

    if let Err(e) = s.db.flush() {
        eprintln!("DB flush error: {}", e);
    }

    Ok(Json(ApiResponse { ok: true }))
}

pub async fn peers(State(state): State<SharedState>) -> Result<Json<Vec<String>>, StatusCode> {
    let s = state.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let list: Vec<String> = s.peers.keys().cloned().collect();
    Ok(Json(list))
}

pub async fn sync(
    State(state): State<SharedState>,
    Json(body): Json<SyncRequest>,
) -> Result<Json<SyncResponse>, StatusCode> {
   
    let base = match Url::parse(&body.address) {
        Ok(url) if url.scheme() == "http" || url.scheme() == "https" => {
            url.as_str().trim_end_matches('/').to_string()
        }
        _ => {
            return Ok(Json(SyncResponse {
                ok: false,
                message: "Invalid URL format".to_string(),
            }));
        }
    };

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| {
            eprintln!("Failed to build HTTP client: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

   
    let outbox_url = format!("{}/_openherd/outbox", base);
    let resp = match client.get(&outbox_url).send().await {
        Ok(r) => r,
        Err(e) => {
            return Ok(Json(SyncResponse {
                ok: false,
                message: format!("Failed to fetch remote outbox: {}", e),
            }));
        }
    };
    
    if resp.status() != HttpStatus::OK {
        return Ok(Json(SyncResponse {
            ok: false,
            message: format!("Remote outbox returned status {}", resp.status()),
        }));
    }
    
    let incoming: Vec<Envelope> = match resp.json().await {
        Ok(data) => data,
        Err(e) => {
            return Ok(Json(SyncResponse {
                ok: false,
                message: format!("Failed to parse remote outbox: {}", e),
            }));
        }
    };

   
    {
        let mut s = state.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        for env in incoming.into_iter() {
            if let Ok(_p) = validate_envelope(&env) {
                let id = env.id.clone();
                if let Ok(bytes) = serde_json::to_vec(&env) {
                    let _ = s.db.insert(id.as_bytes(), bytes);
                }
                s.memory.insert(id, env);
            }
        }
        let _ = s.db.flush();
    }

   
    let posts_to_send: Vec<Envelope> = {
        let s = state.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        s.memory.values().take(10_000).cloned().collect()
    };

    let inbox_url = format!("{}/_openherd/inbox", base);
    let post_resp = match client.post(&inbox_url).json(&posts_to_send).send().await {
        Ok(r) => r,
        Err(e) => {
            return Ok(Json(SyncResponse {
                ok: false,
                message: format!("Failed to push to remote inbox: {}", e),
            }));
        }
    };
    
    if post_resp.status() != HttpStatus::OK {
        return Ok(Json(SyncResponse {
            ok: false,
            message: format!("Remote inbox returned status {}", post_resp.status()),
        }));
    }

   
    {
        let mut s = state.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        s.peers
            .entry(base.clone())
            .and_modify(|p| {
                p.failures = 0;
                p.last_ok = Some(Utc::now());
            })
            .or_insert(PeerStatus {
                failures: 0,
                last_ok: Some(Utc::now()),
            });
    }

    Ok(Json(SyncResponse { ok: true, message: "Sync complete".to_string() }))
}

