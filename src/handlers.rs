use crate::{
    state::{PeerStatus, SharedState},
    types::{
        AdminAuth, ApiResponse, Envelope, KarmaCode, KarmaGenerateRequest, KarmaMetadata,
        ModerationAction, ModerationLabel, ModerationReport, SyncRequest, SyncResponse,
    },
    validation::validate_envelope,
};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{Html, Json},
};
use chrono::Utc;
use rand::{distributions::Alphanumeric, Rng};
use reqwest::StatusCode as HttpStatus;
use std::time::Duration;
use url::Url;

pub async fn outbox(State(state): State<SharedState>) -> Result<Json<Vec<Envelope>>, StatusCode> {
    let state = state
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let envelopes: Vec<Envelope> = state.memory.values().cloned().collect();
    Ok(Json(envelopes))
}

pub async fn inbox(
    State(state): State<SharedState>,
    Json(envelopes): Json<Vec<Envelope>>,
) -> Result<Json<ApiResponse>, StatusCode> {
    let mut s = state
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

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
    let s = state
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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
        let mut s = state
            .lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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
        let s = state
            .lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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
        let mut s = state
            .lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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

    Ok(Json(SyncResponse {
        ok: true,
        message: "Sync complete".to_string(),
    }))
}

fn apply_karma_internal(
    s: &mut crate::state::AppState,
    karma_code: KarmaCode,
    code: &str,
    envelope: &Envelope,
    direction: &str,
) -> Result<(), StatusCode> {
    if karma_code.expires < Utc::now() {
        return Err(StatusCode::GONE);
    }
    if karma_code.current_post.is_some() {
        return Err(StatusCode::CONFLICT);
    }

    if let Some(ref vt) = karma_code.vote_type {
        if vt != direction {
            return Err(StatusCode::BAD_REQUEST);
        }
    }
    let post_id = envelope.id.clone();
    let delta = if direction == "upvote" { 1 } else { -1 };
    if let Some(kc) = s.karma_codes.get_mut(code) {
        kc.current_post = Some(post_id.clone());
        kc.used_direction = Some(direction.to_string());
        if kc.vote_type.is_none() {
            kc.vote_type = Some(direction.to_string());
        }
    }
    *s.karma_votes.entry(post_id).or_insert(0) += delta;
    Ok(())
}

pub async fn karma_upvote(
    State(state): State<SharedState>,
    Path(code): Path<String>,
    Json(envelope): Json<Envelope>,
) -> Result<Json<ApiResponse>, StatusCode> {
    let mut s = state
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let karma_code = s
        .karma_codes
        .get(&code)
        .ok_or(StatusCode::NOT_FOUND)?
        .clone();
    validate_envelope(&envelope).map_err(|_| StatusCode::BAD_REQUEST)?;
    apply_karma_internal(&mut s, karma_code, &code, &envelope, "upvote")?;
    Ok(Json(ApiResponse { ok: true }))
}

pub async fn karma_downvote(
    State(state): State<SharedState>,
    Path(code): Path<String>,
    Json(envelope): Json<Envelope>,
) -> Result<Json<ApiResponse>, StatusCode> {
    let mut s = state
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let karma_code = s
        .karma_codes
        .get(&code)
        .ok_or(StatusCode::NOT_FOUND)?
        .clone();
    validate_envelope(&envelope).map_err(|_| StatusCode::BAD_REQUEST)?;
    apply_karma_internal(&mut s, karma_code, &code, &envelope, "downvote")?;
    Ok(Json(ApiResponse { ok: true }))
}

pub async fn karma_revoke(
    State(state): State<SharedState>,
    Path(code): Path<String>,
) -> Result<Json<ApiResponse>, StatusCode> {
    let mut s = state
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let karma_code = s
        .karma_codes
        .get(&code)
        .ok_or(StatusCode::NOT_FOUND)?
        .clone();
    if let Some(post_id) = &karma_code.current_post {
        let direction = karma_code
            .used_direction
            .as_deref()
            .or(karma_code.vote_type.as_deref())
            .unwrap_or("upvote");
        let delta = if direction == "upvote" { -1 } else { 1 };
        if let Some(score) = s.karma_votes.get_mut(post_id) {
            *score += delta;
        }
    }

    if let Some(kc) = s.karma_codes.get_mut(&code) {
        kc.current_post = None;
        kc.used_direction = None;
        if kc.vote_type.is_some() { /* keep constraint */ }
    }

    Ok(Json(ApiResponse { ok: true }))
}

pub async fn karma_metadata(
    State(state): State<SharedState>,
    Path(code): Path<String>,
) -> Result<Json<KarmaMetadata>, StatusCode> {
    let s = state
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let karma_code = s.karma_codes.get(&code).ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(KarmaMetadata {
        code: karma_code.code.clone(),
        expires: karma_code.expires,
        current_post: karma_code.current_post.clone(),
    }))
}

pub async fn karma_lookup(
    State(state): State<SharedState>,
    Json(post_ids): Json<Vec<String>>,
) -> Result<Json<Vec<i32>>, StatusCode> {
    let s = state
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let scores: Vec<i32> = post_ids
        .iter()
        .map(|id| s.karma_votes.get(id).copied().unwrap_or(0))
        .collect();

    Ok(Json(scores))
}

pub async fn moderation_lookup(
    State(state): State<SharedState>,
    Json(post_ids): Json<Vec<String>>,
) -> Result<Json<Vec<Option<String>>>, StatusCode> {
    let s = state
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let labels: Vec<Option<String>> = post_ids
        .iter()
        .map(|id| s.post_labels.get(id).cloned())
        .collect();

    Ok(Json(labels))
}

pub async fn moderation_labels(
    State(state): State<SharedState>,
) -> Result<Json<Vec<ModerationLabel>>, StatusCode> {
    let s = state
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let list = s
        .label_definitions
        .iter()
        .map(|(k, v)| ModerationLabel {
            label: k.clone(),
            description: v.clone(),
        })
        .collect();
    Ok(Json(list))
}

pub async fn moderation_report(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(reports): Json<Vec<ModerationReport>>,
) -> Result<Json<ApiResponse>, StatusCode> {
    let mut s = state
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let reporter_ip = headers
        .get("X-Forwarded-For")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or(s).trim().to_string())
        .or_else(|| {
            headers
                .get("X-Real-IP")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "unknown".to_string());

    for mut report in reports {
        report.reported_at = Utc::now();
        report.reporter_ip = Some(reporter_ip.clone());
        report.id = uuid::Uuid::new_v4().to_string();

        if report.reason.trim().is_empty() {
            continue;
        }

        s.moderation_reports.push(report);
    }

    Ok(Json(ApiResponse { ok: true }))
}

pub async fn admin_ui() -> Html<&'static str> {
    Html(include_str!("../static/admin.html"))
}

pub async fn admin_reports(
    State(state): State<SharedState>,
    Json(auth): Json<AdminAuth>,
) -> Result<Json<Vec<ModerationReport>>, StatusCode> {
    let s = state
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if !s.is_admin(&auth.password) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    Ok(Json(s.moderation_reports.clone()))
}

pub async fn admin_accept_report(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(action): Json<ModerationAction>,
) -> Result<Json<ApiResponse>, StatusCode> {
    let mut s = state
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let password = headers
        .get("X-Admin-Password")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if !s.is_admin(password) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let report = s
        .moderation_reports
        .iter()
        .find(|r| r.id == action.report_id)
        .ok_or(StatusCode::NOT_FOUND)?;

    let post_id = report.post.id.clone();

    if let Some(label) = action.label {
        s.post_labels.insert(post_id, label);
    }

    s.moderation_reports.retain(|r| r.id != action.report_id);

    Ok(Json(ApiResponse { ok: true }))
}

pub async fn admin_delete_report(
    State(state): State<SharedState>,
    Path(report_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<ApiResponse>, StatusCode> {
    let mut s = state
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let password = headers
        .get("X-Admin-Password")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if !s.is_admin(password) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    s.moderation_reports.retain(|r| r.id != report_id);

    Ok(Json(ApiResponse { ok: true }))
}

pub async fn admin_add_label(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(label): Json<ModerationLabel>,
) -> Result<Json<ApiResponse>, StatusCode> {
    let mut s = state
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let password = headers
        .get("X-Admin-Password")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;
    if !s.is_admin(password) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    if s.label_definitions.contains_key(&label.label) {
        return Err(StatusCode::BAD_REQUEST);
    }
    s.label_definitions
        .insert(label.label.clone(), label.description.clone());

    let labels_vec: Vec<ModerationLabel> = s
        .label_definitions
        .iter()
        .map(|(l, d)| ModerationLabel {
            label: l.clone(),
            description: d.clone(),
        })
        .collect();
    if let Ok(json) = serde_json::to_string_pretty(&labels_vec) {
        let _ = std::fs::write("./labels.json", json);
    }

    Ok(Json(ApiResponse { ok: true }))
}

pub async fn admin_delete_label(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(label): Path<String>,
) -> Result<Json<ApiResponse>, StatusCode> {
    let mut s = state
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let password = headers
        .get("X-Admin-Password")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;
    if !s.is_admin(password) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    s.label_definitions.remove(&label);
    s.post_labels.retain(|_, l| l != &label);

    let labels_vec: Vec<ModerationLabel> = s
        .label_definitions
        .iter()
        .map(|(l, d)| ModerationLabel {
            label: l.clone(),
            description: d.clone(),
        })
        .collect();
    if let Ok(json) = serde_json::to_string_pretty(&labels_vec) {
        let _ = std::fs::write("./labels.json", json);
    }

    Ok(Json(ApiResponse { ok: true }))
}

pub async fn admin_generate_karma_codes(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(req): Json<KarmaGenerateRequest>,
) -> Result<Json<Vec<KarmaCode>>, StatusCode> {
    let mut s = state
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let password = headers
        .get("X-Admin-Password")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if !s.is_admin(password) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let vt_opt: Option<String> = None;

    let mut created = Vec::new();
    for _ in 0..req.count.max(1) {
        let raw: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(10)
            .map(char::from)
            .collect::<String>()
            .to_uppercase();
        let code = format!("{}-{}", &raw[0..5], &raw[5..10]);

        let kc = KarmaCode {
            code: code.clone(),
            issuer: req.issuer.clone(),
            vote_type: vt_opt.clone(),
            expires: req.expires,
            region: req.region.clone(),
            current_post: None,
            used_direction: None,
        };
        s.karma_codes.insert(code.clone(), kc.clone());
        created.push(kc);
    }

    Ok(Json(created))
}

pub async fn admin_generate_karma_codes_text(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(req): Json<KarmaGenerateRequest>,
) -> Result<String, StatusCode> {
    let mut s = state
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let password = headers
        .get("X-Admin-Password")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;
    if !s.is_admin(password) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let vt_opt: Option<String> = None;
    let mut lines = vec![req.issuer.clone()];
    for _ in 0..req.count.max(1) {
        let raw: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(10)
            .map(char::from)
            .collect::<String>()
            .to_uppercase();
        let code = format!("{}-{}", &raw[0..5], &raw[5..10]);
        let kc = KarmaCode {
            code: code.clone(),
            issuer: req.issuer.clone(),
            vote_type: vt_opt.clone(),
            expires: req.expires,
            region: req.region.clone(),
            current_post: None,
            used_direction: None,
        };
        s.karma_codes.insert(code.clone(), kc);
        lines.push(code);
    }
    Ok(lines.join("\n"))
}
