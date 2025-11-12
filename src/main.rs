mod types;
mod validation;
mod handlers;
mod state;

use axum::{routing::{get, post}, Router};
use chrono::Utc;
use std::sync::{Arc, Mutex};
use tower_http::cors::CorsLayer;
use std::time::Duration;
use state::{AppState as CoreState, SharedState, PeerStatus};

#[tokio::main]
async fn main() {
    let db = sled::open("./data").expect("failed to open sled DB");
    let state: SharedState = Arc::new(Mutex::new(CoreState::new(db)));
   
    {
        let mut s = state.lock().unwrap();
        for item in s.db.iter() {
            if let Ok((k, v)) = item {
                if let Ok(env) = serde_json::from_slice::<types::Envelope>(&v) {
                    s.memory.insert(env.id.clone(), env);
                } else {
                   
                    let _ = s.db.remove(k);
                }
            }
        }
    }

   
    let app = Router::new()
        .route("/_openherd/outbox", get(handlers::outbox))
        .route("/_openherd/inbox", post(handlers::inbox))
        .route("/_openherd/peers", get(handlers::peers))
        .route("/_openherd/sync", post(handlers::sync))
        .layer(CorsLayer::permissive())
        .with_state(state.clone());

   
    tokio::spawn(peer_monitor(state.clone()));

   
    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap();
    
    println!("OpenHerd server running on http://{}", addr);
    
    axum::serve(listener, app).await.unwrap();
}

async fn peer_monitor(state: SharedState) {
    let client = reqwest::Client::new();
    loop {
        tokio::time::sleep(Duration::from_secs(120)).await;
       
        let peers: Vec<String> = {
            let s = state.lock().unwrap();
            s.peers.keys().cloned().collect()
        };

        for addr in peers {
            let outbox_url = format!("{}/_openherd/outbox", addr.trim_end_matches('/'));
            let ok = match client.get(&outbox_url).send().await {
                Ok(resp) => resp.status().is_success(),
                Err(_) => false,
            };

            let mut s = state.lock().unwrap();
            if ok {
                if let Some(p) = s.peers.get_mut(&addr) {
                    p.failures = 0;
                    p.last_ok = Some(Utc::now());
                } else {
                    s.peers.insert(addr.clone(), PeerStatus { failures: 0, last_ok: Some(Utc::now()) });
                }
            } else {
                if let Some(p) = s.peers.get_mut(&addr) {
                    p.failures = p.failures.saturating_add(1);
                    if p.failures >= 5 {
                        s.peers.remove(&addr);
                    }
                } else {
                   
                }
            }
        }
    }
}
