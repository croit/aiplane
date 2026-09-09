// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The user-facing `/api/v0` workspace surfaces for the SPA (issue #22,
//! P5): memories, scheduled actions, and webhooks. Thin JSON translations
//! of the legacy form handlers one layer down — the legacy pages stay
//! alive beside them until phase 6.

use std::sync::Arc;

use rama::http::service::web::extract::{Path, State};
use rama::http::{Request, Response, StatusCode};

use gateway_core::server::auth::token as auth_token;
use gateway_core::server::db;
use gateway_runtime::rama_server::state::RamaState;
use gateway_runtime::server::scheduled::{self, cron::Cron};
use gateway_runtime::server::webhooks;

use super::{json_error, json_ok, require_session_json};

fn bad_request(message: impl Into<String>) -> Response {
    json_error(StatusCode::BAD_REQUEST, "invalid_request", &message.into())
}

fn internal(message: impl std::fmt::Display) -> Response {
    json_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal_error",
        &message.to_string(),
    )
}

// ---------------------------------------------------------------------------
// Memories

/// GET /api/v0/memories — the caller's structured memories.
pub async fn memories_list(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, user) = match require_session_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    match db::user_memories::list_for_user(&state.db, &user.id, 500).await {
        Ok(rows) => json_ok(
            StatusCode::OK,
            serde_json::json!({ "memories": rows.iter().map(memory_json).collect::<Vec<_>>() }),
        ),
        Err(err) => internal(err),
    }
}

fn memory_json(m: &db::user_memories::Memory) -> serde_json::Value {
    serde_json::json!({
        "id": m.id,
        "kind": m.kind.as_str(),
        "content": m.content,
        "created_at": m.created_at.to_string(),
    })
}

#[derive(serde::Deserialize)]
pub struct MemoryBody {
    pub kind: String,
    pub content: String,
}

fn parse_kind(kind: &str) -> Option<db::user_memories::MemoryKind> {
    match kind {
        "preference" => Some(db::user_memories::MemoryKind::Preference),
        "project" => Some(db::user_memories::MemoryKind::Project),
        "fact" => Some(db::user_memories::MemoryKind::Fact),
        _ => None,
    }
}

/// POST /api/v0/memories
pub async fn memories_create(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, user) = match require_session_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return bad_request(msg),
    };
    let parsed: MemoryBody = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => return bad_request(format!("parsing the memory body: {err}")),
    };
    let Some(kind) = parse_kind(&parsed.kind) else {
        return bad_request(format!(
            "unknown memory kind: {} (preference | project | fact)",
            parsed.kind
        ));
    };
    let content = parsed.content.trim();
    if content.is_empty() {
        return bad_request("the memory content must not be empty");
    }
    match db::user_memories::insert(&state.db, &user.id, kind, content).await {
        Ok(m) => json_ok(StatusCode::CREATED, memory_json(&m)),
        Err(err) => internal(err),
    }
}

/// PUT /api/v0/memories/{id}
pub async fn memories_update(
    Path(id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = match require_session_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return bad_request(msg),
    };
    let parsed: MemoryBody = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => return bad_request(format!("parsing the memory body: {err}")),
    };
    let Some(kind) = parse_kind(&parsed.kind) else {
        return bad_request(format!("unknown memory kind: {}", parsed.kind));
    };
    let content = parsed.content.trim();
    if content.is_empty() {
        return bad_request("the memory content must not be empty");
    }
    match db::user_memories::update(&state.db, &user.id, &id, kind, content).await {
        Ok(Some(m)) => json_ok(StatusCode::OK, memory_json(&m)),
        Ok(None) => json_error(StatusCode::NOT_FOUND, "not_found", "no such memory"),
        Err(err) => internal(err),
    }
}

/// DELETE /api/v0/memories/{id}
pub async fn memories_delete(
    Path(id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = match require_session_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    match db::user_memories::delete(&state.db, &user.id, &id).await {
        Ok(true) => Response::builder()
            .status(StatusCode::NO_CONTENT)
            .body(rama::http::Body::empty())
            .expect("static empty response"),
        Ok(false) => json_error(StatusCode::NOT_FOUND, "not_found", "no such memory"),
        Err(err) => internal(err),
    }
}

// ---------------------------------------------------------------------------
// Scheduled actions

fn action_json(a: &scheduled::ScheduledAction) -> serde_json::Value {
    serde_json::json!({
        "id": a.id,
        "name": a.name,
        "prompt": a.prompt,
        "model": a.model,
        "cron": a.cron,
        "timezone": a.timezone,
        "tools_enabled": a.tools_enabled,
        "reuse_conversation": a.reuse_conversation,
        "reuse_rounds": a.reuse_rounds,
        "enabled": a.enabled,
        "next_run_at": a.next_run_at.map(|t| t.to_string()),
        "last_session_id": a.last_session_id,
        "last_status": a.last_status,
    })
}

/// GET /api/v0/scheduled — the caller's actions.
pub async fn scheduled_list(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, user) = match require_session_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    match scheduled::list_for_user(&state.db, &user.id).await {
        Ok(rows) => json_ok(
            StatusCode::OK,
            serde_json::json!({
                "actions": rows.iter().map(action_json).collect::<Vec<_>>(),
                "models": state.upstreams.models_with_compliance_for_kind(
                    gateway_core::server::upstreams::PoolKind::Chat).into_iter().map(|(id, _)| id).collect::<Vec<_>>(),
            }),
        ),
        Err(err) => internal(err),
    }
}

#[derive(serde::Deserialize)]
pub struct ScheduledBody {
    pub name: String,
    pub prompt: String,
    pub model: String,
    pub cron: String,
    #[serde(default)]
    pub timezone: String,
    #[serde(default)]
    pub tools_enabled: bool,
    #[serde(default)]
    pub reuse_conversation: bool,
    #[serde(default)]
    pub reuse_rounds: i64,
}

async fn compute_next(cron: &str, tz_name: &str) -> Result<Option<jiff::Timestamp>, String> {
    let parsed = Cron::parse(cron).map_err(|e| e.to_string())?;
    let tz =
        jiff::tz::TimeZone::get(tz_name).map_err(|_| format!("unknown timezone: {tz_name}"))?;
    Ok(parsed.next_after(jiff::Timestamp::now(), &tz))
}

/// POST /api/v0/scheduled
pub async fn scheduled_create(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, user) = match require_session_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return bad_request(msg),
    };
    let parsed: ScheduledBody = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => return bad_request(format!("parsing the action body: {err}")),
    };
    if parsed.name.trim().is_empty() || parsed.prompt.trim().is_empty() {
        return bad_request("an action needs a name and a prompt");
    }
    let tz = if parsed.timezone.trim().is_empty() {
        user.timezone.clone().unwrap_or_else(|| "UTC".into())
    } else {
        parsed.timezone.trim().to_string()
    };
    let next = match compute_next(&parsed.cron, &tz).await {
        Ok(n) => n,
        Err(e) => return bad_request(e),
    };
    let new = scheduled::NewAction {
        user_id: user.id.clone(),
        name: parsed.name.trim().to_string(),
        prompt: parsed.prompt,
        model: parsed.model,
        cron: parsed.cron,
        timezone: tz,
        tools_enabled: parsed.tools_enabled,
        reuse_conversation: parsed.reuse_conversation,
        reuse_rounds: parsed.reuse_rounds,
        next_run_at: next,
    };
    match scheduled::create(&state.db, new).await {
        Ok(a) => json_ok(StatusCode::CREATED, action_json(&a)),
        Err(err) => internal(err),
    }
}

/// PUT /api/v0/scheduled/{id}
pub async fn scheduled_update(
    Path(id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = match require_session_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return bad_request(msg),
    };
    let parsed: ScheduledBody = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => return bad_request(format!("parsing the action body: {err}")),
    };
    let next = match compute_next(&parsed.cron, &parsed.timezone).await {
        Ok(n) => n,
        Err(e) => return bad_request(e),
    };
    let edit = scheduled::EditAction {
        name: parsed.name.trim().to_string(),
        prompt: parsed.prompt,
        model: parsed.model,
        cron: parsed.cron,
        timezone: parsed.timezone,
        tools_enabled: parsed.tools_enabled,
        reuse_conversation: parsed.reuse_conversation,
        reuse_rounds: parsed.reuse_rounds,
        next_run_at: next,
    };
    match scheduled::update(&state.db, &user.id, &id, edit).await {
        Ok(true) => {
            let a = scheduled::get(&state.db, &user.id, &id)
                .await
                .ok()
                .flatten();
            match a {
                Some(a) => json_ok(StatusCode::OK, action_json(&a)),
                None => json_ok(StatusCode::OK, serde_json::json!({ "ok": true })),
            }
        }
        Ok(false) => json_error(StatusCode::NOT_FOUND, "not_found", "no such action"),
        Err(err) => internal(err),
    }
}

#[derive(serde::Deserialize)]
pub struct EnabledBody {
    pub enabled: bool,
}

/// POST /api/v0/scheduled/{id}/toggle — pause/resume (recomputes next run
/// on resume, exactly like the form path).
pub async fn scheduled_toggle(
    Path(id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = match require_session_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return bad_request(msg),
    };
    let parsed: EnabledBody = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => return bad_request(format!("parsing the toggle body: {err}")),
    };
    let Some(action) = scheduled::get(&state.db, &user.id, &id)
        .await
        .ok()
        .flatten()
    else {
        return json_error(StatusCode::NOT_FOUND, "not_found", "no such action");
    };
    let next = if parsed.enabled {
        match compute_next(&action.cron, &action.timezone).await {
            Ok(n) => n,
            Err(e) => return bad_request(e),
        }
    } else {
        None
    };
    match scheduled::set_enabled(&state.db, &user.id, &id, parsed.enabled, next).await {
        Ok(true) => json_ok(
            StatusCode::OK,
            serde_json::json!({ "enabled": parsed.enabled }),
        ),
        Ok(false) => json_error(StatusCode::NOT_FOUND, "not_found", "no such action"),
        Err(err) => internal(err),
    }
}

/// DELETE /api/v0/scheduled/{id}
pub async fn scheduled_delete(
    Path(id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = match require_session_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    match scheduled::delete(&state.db, &user.id, &id).await {
        Ok(true) => Response::builder()
            .status(StatusCode::NO_CONTENT)
            .body(rama::http::Body::empty())
            .expect("static empty response"),
        Ok(false) => json_error(StatusCode::NOT_FOUND, "not_found", "no such action"),
        Err(err) => internal(err),
    }
}

#[derive(serde::Deserialize)]
pub struct CronPreviewBody {
    pub cron: String,
    #[serde(default)]
    pub timezone: String,
}

/// POST /api/v0/scheduled/preview — validate + describe a cron expression,
/// with its next three fire times. Pure computation, no writes.
pub async fn scheduled_preview(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, user) = match require_session_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return bad_request(msg),
    };
    let parsed: CronPreviewBody = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => return bad_request(format!("parsing the preview body: {err}")),
    };
    let tz_name = if parsed.timezone.trim().is_empty() {
        user.timezone.clone().unwrap_or_else(|| "UTC".into())
    } else {
        parsed.timezone.trim().to_string()
    };
    let Ok(tz) = jiff::tz::TimeZone::get(&tz_name) else {
        return bad_request(format!("unknown timezone: {tz_name}"));
    };
    let Ok(cron) = Cron::parse(&parsed.cron) else {
        return bad_request(format!(
            "not a valid 5-field cron expression: {}",
            parsed.cron
        ));
    };
    json_ok(
        StatusCode::OK,
        serde_json::json!({
            "summary": cron.describe(),
            "upcoming": cron.upcoming(jiff::Timestamp::now(), &tz, 3)
                .iter()
                .map(|t| t.to_string())
                .collect::<Vec<_>>(),
        }),
    )
}

// ---------------------------------------------------------------------------
// Webhooks

fn webhook_json(w: &webhooks::Webhook) -> serde_json::Value {
    serde_json::json!({
        "id": w.id,
        "name": w.name,
        "prompt": w.prompt,
        "model": w.model,
        "tools_enabled": w.tools_enabled,
        "synchronous": w.synchronous,
        "reuse_conversation": w.reuse_conversation,
        "reuse_rounds": w.reuse_rounds,
        "enabled": w.enabled,
        "last_fired_at": w.last_fired_at.map(|t| t.to_string()),
        "last_status": w.last_status,
    })
}

/// GET /api/v0/webhooks — the caller's webhooks.
pub async fn webhooks_list(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, user) = match require_session_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    match webhooks::list_for_user(&state.db, &user.id).await {
        Ok(rows) => json_ok(
            StatusCode::OK,
            serde_json::json!({
                "webhooks": rows.iter().map(webhook_json).collect::<Vec<_>>(),
                "models": state.upstreams.models_with_compliance_for_kind(
                    gateway_core::server::upstreams::PoolKind::Chat).into_iter().map(|(id, _)| id).collect::<Vec<_>>(),
            }),
        ),
        Err(err) => internal(err),
    }
}

#[derive(serde::Deserialize)]
pub struct WebhookBody {
    pub name: String,
    pub prompt: String,
    pub model: String,
    #[serde(default)]
    pub tools_enabled: bool,
    #[serde(default)]
    pub synchronous: bool,
    #[serde(default)]
    pub reuse_conversation: bool,
    #[serde(default)]
    pub reuse_rounds: i64,
}

/// POST /api/v0/webhooks — create; the trigger secret is minted once and
/// returned exactly once.
pub async fn webhooks_create(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, user) = match require_session_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return bad_request(msg),
    };
    let parsed: WebhookBody = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => return bad_request(format!("parsing the webhook body: {err}")),
    };
    if parsed.name.trim().is_empty() || parsed.prompt.trim().is_empty() {
        return bad_request("a webhook needs a name and a prompt");
    }
    let (secret, hash) = auth_token::mint_webhook();
    let new = webhooks::NewWebhook {
        user_id: user.id.clone(),
        name: parsed.name.trim().to_string(),
        prompt: parsed.prompt,
        model: parsed.model,
        tools_enabled: parsed.tools_enabled,
        synchronous: parsed.synchronous,
        reuse_conversation: parsed.reuse_conversation,
        reuse_rounds: parsed.reuse_rounds,
        secret_hash: hash,
    };
    match webhooks::create(&state.db, new).await {
        Ok(w) => json_ok(
            StatusCode::CREATED,
            serde_json::json!({ "webhook": webhook_json(&w), "secret": secret }),
        ),
        Err(err) => internal(err),
    }
}

/// PUT /api/v0/webhooks/{id}
pub async fn webhooks_update(
    Path(id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = match require_session_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return bad_request(msg),
    };
    let parsed: WebhookBody = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => return bad_request(format!("parsing the webhook body: {err}")),
    };
    let edit = webhooks::EditWebhook {
        name: parsed.name.trim().to_string(),
        prompt: parsed.prompt,
        model: parsed.model,
        tools_enabled: parsed.tools_enabled,
        synchronous: parsed.synchronous,
        reuse_conversation: parsed.reuse_conversation,
        reuse_rounds: parsed.reuse_rounds,
    };
    match webhooks::update(&state.db, &user.id, &id, edit).await {
        Ok(true) => {
            let w = webhooks::get(&state.db, &user.id, &id).await.ok().flatten();
            match w {
                Some(w) => json_ok(StatusCode::OK, webhook_json(&w)),
                None => json_ok(StatusCode::OK, serde_json::json!({ "ok": true })),
            }
        }
        Ok(false) => json_error(StatusCode::NOT_FOUND, "not_found", "no such webhook"),
        Err(err) => internal(err),
    }
}

/// POST /api/v0/webhooks/{id}/toggle
pub async fn webhooks_toggle(
    Path(id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = match require_session_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return bad_request(msg),
    };
    let parsed: EnabledBody = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => return bad_request(format!("parsing the toggle body: {err}")),
    };
    match webhooks::set_enabled(&state.db, &user.id, &id, parsed.enabled).await {
        Ok(true) => json_ok(
            StatusCode::OK,
            serde_json::json!({ "enabled": parsed.enabled }),
        ),
        Ok(false) => json_error(StatusCode::NOT_FOUND, "not_found", "no such webhook"),
        Err(err) => internal(err),
    }
}

/// POST /api/v0/webhooks/{id}/rotate — mint a fresh trigger secret; the
/// old one stops working immediately. Plaintext returned exactly once.
pub async fn webhooks_rotate(
    Path(id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = match require_session_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let (secret, hash) = auth_token::mint_webhook();
    match webhooks::rotate_secret(&state.db, &user.id, &id, &hash).await {
        Ok(true) => json_ok(StatusCode::OK, serde_json::json!({ "secret": secret })),
        Ok(false) => json_error(StatusCode::NOT_FOUND, "not_found", "no such webhook"),
        Err(err) => internal(err),
    }
}

/// DELETE /api/v0/webhooks/{id}
pub async fn webhooks_delete(
    Path(id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = match require_session_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    match webhooks::delete(&state.db, &user.id, &id).await {
        Ok(true) => Response::builder()
            .status(StatusCode::NO_CONTENT)
            .body(rama::http::Body::empty())
            .expect("static empty response"),
        Ok(false) => json_error(StatusCode::NOT_FOUND, "not_found", "no such webhook"),
        Err(err) => internal(err),
    }
}

/// GET /api/v0/webhooks/{id}/runs — the run history.
///
/// Resolve the webhook through the caller first: `list_runs` is keyed only by
/// webhook id, so querying it straight from the path would hand any signed-in
/// user another account's run history — prompts and replayed payloads
/// included — for any id they can name. `webhooks::get` is owner-scoped, and
/// an id that is not the caller's reads as missing.
pub async fn webhooks_runs(
    Path(id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = match require_session_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let hook = match webhooks::get(&state.db, &user.id, &id).await {
        Ok(Some(h)) => h,
        Ok(None) => return json_error(StatusCode::NOT_FOUND, "not_found", "no such webhook"),
        Err(err) => return internal(err),
    };
    match webhooks::list_runs(&state.db, &hook.id, 50).await {
        Ok(runs) => json_ok(
            StatusCode::OK,
            serde_json::json!({ "runs": runs.iter().map(run_json).collect::<Vec<_>>() }),
        ),
        Err(err) => internal(err),
    }
}

fn run_json(r: &webhooks::WebhookRun) -> serde_json::Value {
    serde_json::json!({
        "id": r.id,
        "status": r.status,
        "error": r.error,
        "fired_at": r.fired_at.to_string(),
        "source": r.source,
        "session_id": r.session_id,
        "prompt": r.prompt,
    })
}
