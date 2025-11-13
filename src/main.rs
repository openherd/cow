mod handlers;
mod state;
mod types;
mod validation;

use axum::{
    routing::{delete, get, patch, post},
    Router,
};
use chrono::Utc;
use clap::{Parser, Subcommand};
use state::{AppState as CoreState, PeerStatus, SharedState};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tower_http::cors::CorsLayer;

#[derive(Parser)]
#[command(name = "openherd-cow")]
#[command(about = "OpenHerd Cow", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    EnrollAdmin { password: String },

    DenrollAdmin { password: String },

    Serve,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let db = sled::open("./data").expect("failed to open sled DB");
    let state: SharedState = Arc::new(Mutex::new(CoreState::new(db.clone())));

    {
        let mut s = state.lock().unwrap();
        if let Ok(Some(admin_bytes)) = db.get(b"__admin_passwords__") {
            if let Ok(passwords) = serde_json::from_slice::<Vec<String>>(&admin_bytes) {
                s.admin_passwords = passwords;
            }
        }
    }

    match cli.command.unwrap_or(Commands::Serve) {
        Commands::EnrollAdmin { password } => {
            let mut s = state.lock().unwrap();
            if !s.admin_passwords.contains(&password) {
                s.admin_passwords.push(password.clone());
                let bytes = serde_json::to_vec(&s.admin_passwords).unwrap();
                db.insert(b"__admin_passwords__", bytes).unwrap();
                db.flush().unwrap();
                println!("Admin enrolled successfully");
            } else {
                println!("Admin already exists");
            }
            return;
        }
        Commands::DenrollAdmin { password } => {
            let mut s = state.lock().unwrap();
            s.admin_passwords.retain(|p| p != &password);
            let bytes = serde_json::to_vec(&s.admin_passwords).unwrap();
            db.insert(b"__admin_passwords__", bytes).unwrap();
            db.flush().unwrap();
            println!("Admin denrolled successfully");
            return;
        }
        Commands::Serve => {}
    }

    {
        {
            let mut s = state.lock().unwrap();
            for item in s.db.iter() {
                if let Ok((k, v)) = item {
                    if k.starts_with(b"post:") {
                        if let Ok(env) = serde_json::from_slice::<types::Envelope>(&v) {
                            s.memory.insert(env.id.clone(), env);
                        } else {
                            let _ = s.db.remove(k);
                        }
                    }
                }
            }
        }

        {
            let mut s = state.lock().unwrap();
            if let Ok(contents) = std::fs::read_to_string("./labels.json") {
                if let Ok(labels) = serde_json::from_str::<Vec<types::ModerationLabel>>(&contents) {
                    for label in labels {
                        s.label_definitions.insert(label.label, label.description);
                    }
                    println!(
                        "✓ Loaded {} label definitions from labels.json",
                        s.label_definitions.len()
                    );
                } else {
                    eprintln!("Failed to parse labels.json");
                }
            } else {
                eprintln!("labels.json not found, starting with empty label definitions");
            }
        }
    }

    let app = Router::new()
        .route("/_openherd/outbox", get(handlers::outbox))
        .route("/_openherd/inbox", post(handlers::inbox))
        .route("/_openherd/peers", get(handlers::peers))
        .route("/_openherd/sync", post(handlers::sync))
        .route(
            "/_openherd/karma/:code/upvote",
            patch(handlers::karma_upvote),
        )
        .route(
            "/_openherd/karma/:code/downvote",
            patch(handlers::karma_downvote),
        )
        .route("/_openherd/karma/:code", delete(handlers::karma_revoke))
        .route("/_openherd/karma/:code/", get(handlers::karma_metadata))
        .route("/_openherd/karma/lookup", post(handlers::karma_lookup))
        .route(
            "/_openherd/moderation/lookup",
            post(handlers::moderation_lookup),
        )
        .route(
            "/_openherd/moderation/labels",
            get(handlers::moderation_labels),
        )
        .route(
            "/_openherd/moderation/report",
            post(handlers::moderation_report),
        )
        .route("/_openherd/admin", get(handlers::admin_ui))
        .route("/_openherd/admin/reports", post(handlers::admin_reports))
        .route(
            "/_openherd/admin/accept",
            post(handlers::admin_accept_report),
        )
        .route(
            "/_openherd/admin/delete/:id",
            delete(handlers::admin_delete_report),
        )
        .route(
            "/_openherd/admin/karma/codes",
            post(handlers::admin_generate_karma_codes),
        )
        .route(
            "/_openherd/admin/karma/codes.txt",
            post(handlers::admin_generate_karma_codes_text),
        )
        .route(
            "/_openherd/admin/moderation/labels",
            post(handlers::admin_add_label),
        )
        .route(
            "/_openherd/admin/moderation/labels/:label",
            delete(handlers::admin_delete_label),
        )
        .layer(CorsLayer::permissive())
        .with_state(state.clone());

    tokio::spawn(peer_monitor(state.clone()));

    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();

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
                    s.peers.insert(
                        addr.clone(),
                        PeerStatus {
                            failures: 0,
                            last_ok: Some(Utc::now()),
                        },
                    );
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
