// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The `/api/v0/chat/*` JSON surface for the SvelteKit SPA (issue #22,
//! phase 2): session CRUD, the JSON submit, and the JSON-SSE event stream.
//!
//! The legacy wire (multipart POST → datastar HTML patches) stays alive
//! unchanged beside it — see `mod.rs` for the shared submit core both
//! surfaces call. This module only translates: requests to
//! [`ChatSubmit`]/ids, results to JSON, and the worker broadcast to the
//! event protocol in `session_core::chat_json`.
//!
//! Wire contract (also in `docs/openapi.json`, enforced by the drift test):
//!
//! * `GET  /api/v0/chat/sessions` — the sidebar list
//! * `POST /api/v0/chat/sessions` — mint a session
//! * `GET  /api/v0/chat/sessions/{id}` — full snapshot (owner or shared)
//! * `DELETE /api/v0/chat/sessions/{id}` — owner-only, sweeps attachments
//! * `POST /api/v0/chat/sessions/{id}/pin` — toggle pin
//! * `POST /api/v0/chat/sessions/{id}/messages` — submit `{model, message}`
//! * `POST /api/v0/chat/sessions/{id}/cancel` — stop the live turn
//! * `GET  /api/v0/chat/sessions/{id}/events` — the SSE event stream

use std::sync::Arc;

use rama::http::service::web::extract::{Path, State};
use rama::http::{Request, Response, StatusCode};
use serde::Deserialize;
use serde_json::json;

use gateway_runtime::rama_server::state::RamaState;

use gateway_core::server::db::users::User;

use super::{ChatSubmit, RequestCtx, SubmitTurnError, TurnPath, submit_turn};
use crate::pages::{json_error, require_session_json};
use session_core::db as chat;

fn ok_json(status: StatusCode, body: serde_json::Value) -> Response {
    use rama::http::header;
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(body.to_string().into())
        .expect("static JSON response")
}

/// GET /api/v0/chat/sessions — every conversation of the signed-in user,
/// pinned first then most-recent (the sidebar's order). With `?q=` the FTS
/// content search answers instead: title matches first, then content hits
/// with a highlighted snippet.
pub async fn sessions_list(
    State(state): State<Arc<RamaState>>,
    rama::http::service::web::extract::Query(params): rama::http::service::web::extract::Query<
        SessionsQuery,
    >,
    req: Request,
) -> Response {
    let (_session, user) = match require_session_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    if let Some(q) = params.q.filter(|q| !q.trim().is_empty()) {
        let hits = chat::search_sessions(&state.db, &user.id, q.trim(), 50)
            .await
            .unwrap_or_default();
        return ok_json(
            StatusCode::OK,
            json!({
                "sessions": hits.iter().map(|h| serde_json::json!({
                    "id": h.session_id,
                    "title": h.title,
                    "updated_at": h.updated_at.to_string(),
                    "pinned": h.pinned,
                    "snippet": h.snippet,
                })).collect::<Vec<_>>(),
            }),
        );
    }
    match chat::list_sessions(&state.db, &user.id).await {
        Ok(sessions) => ok_json(StatusCode::OK, json!({ "sessions": sessions })),
        Err(err) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            &err.to_string(),
        ),
    }
}

#[derive(serde::Deserialize, Default)]
pub struct SessionsQuery {
    q: Option<String>,
}

/// POST /api/v0/chat/sessions — mint an empty conversation.
pub async fn session_create(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, user) = match require_session_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    match chat::create_session(&state.db, &user.id).await {
        Ok(session) => ok_json(StatusCode::CREATED, json!({ "session": session })),
        Err(err) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            &err.to_string(),
        ),
    }
}

/// GET /api/v0/chat/sessions/{id} — one conversation with every turn.
/// Readable by the owner and, for shared sessions, any signed-in user.
pub async fn session_get(
    Path(session_id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = match require_session_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let session = match readable_session(&state, &user.id, &session_id).await {
        Ok(Some(s)) => s,
        Ok(None) => return not_found_conversation(),
        Err(resp) => return resp,
    };
    let turns = match chat::list_turns(&state.db, &session_id).await {
        Ok(t) => t,
        Err(err) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                &err.to_string(),
            );
        }
    };
    ok_json(
        StatusCode::OK,
        json!({ "session": session, "turns": turns }),
    )
}

/// DELETE /api/v0/chat/sessions/{id} — owner-only. Also reclaims the
/// session's attachment objects, exactly like the legacy form route.
pub async fn session_delete(
    Path(session_id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = match require_session_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    if !user_owns(&state, &user.id, &session_id).await {
        return not_found_conversation();
    }
    // Deleting the conversation deletes its files: seq 0 covers every turn.
    // Read before the rows go — see `doomed_attachments`.
    let orphaned = super::doomed_attachments(&state, &session_id, 0).await;
    let deleted = match chat::delete_session(&state.db, &user.id, &session_id).await {
        Ok(v) => v,
        Err(err) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                &err.to_string(),
            );
        }
    };
    if deleted {
        super::reclaim_attachments(&state, orphaned);
        Response::builder()
            .status(StatusCode::NO_CONTENT)
            .body(rama::http::Body::empty())
            .expect("static empty response")
    } else {
        not_found_conversation()
    }
}

/// POST /api/v0/chat/sessions/{id}/pin — set the sidebar pin explicitly
/// (`{"pinned": true|false}`). An explicit value, not a toggle: an API
/// caller shouldn't need the current state to express the wanted one, and
/// a retry must be idempotent.
pub async fn session_pin(
    Path(session_id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = match require_session_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    if !user_owns(&state, &user.id, &session_id).await {
        return not_found_conversation();
    }
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return json_error(StatusCode::BAD_REQUEST, "invalid_request", &msg),
    };
    let parsed: PinBody = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => {
            return json_error(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                &format!("parsing the pin body: {err}"),
            );
        }
    };
    match chat::set_pinned(&state.db, &user.id, &session_id, parsed.pinned).await {
        Ok(true) => ok_json(StatusCode::OK, json!({ "pinned": parsed.pinned })),
        Ok(false) => not_found_conversation(),
        Err(err) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            &err.to_string(),
        ),
    }
}

#[derive(Deserialize)]
struct PinBody {
    pinned: bool,
}

#[derive(Deserialize)]
struct MessageBody {
    model: String,
    message: String,
    #[serde(default)]
    voice: bool,
}

/// POST /api/v0/chat/sessions/{id}/messages — submit a turn as JSON.
///
/// Returns `202` with both turn ids the moment the worker is spawned; the
/// reply itself arrives on `GET …/events`, which the client opens (or has
/// open) as its SSE subscription. No SSE body here — that's the protocol
/// split from the legacy wire, where this POST *was* the stream.
pub async fn message_send(
    Path(session_id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = match require_session_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    // Two request shapes, one code path: JSON `{model, message}` for plain
    // text, multipart/form-data (like the legacy composer) when attachments
    // ride along — they must upload under the user-turn's S3 prefix, which
    // only exists at parse time.
    let content_type = req
        .headers()
        .get(rama::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let user_turn_id = uuid::Uuid::new_v4().to_string();
    let (_, body) = req.into_parts();
    let submit = if content_type.starts_with("multipart/form-data") {
        let bytes = match session_core::chrome::read_body_to_bytes(body).await {
            Ok(b) => b,
            Err(msg) => return json_error(StatusCode::BAD_REQUEST, "invalid_request", &msg),
        };
        match super::parse_chat_submit(&content_type, bytes, &user_turn_id, &state).await {
            Ok(mut s) => {
                s.user_text = s.user_text.trim().to_string();
                s
            }
            Err(msg) => return json_error(StatusCode::BAD_REQUEST, "invalid_request", &msg),
        }
    } else {
        let bytes = match session_core::chrome::read_body_to_bytes(body).await {
            Ok(b) => b,
            Err(msg) => return json_error(StatusCode::BAD_REQUEST, "invalid_request", &msg),
        };
        let parsed: MessageBody = match serde_json::from_slice(&bytes) {
            Ok(p) => p,
            Err(err) => {
                return json_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    &format!("parsing the message body: {err}"),
                );
            }
        };
        ChatSubmit {
            model: parsed.model,
            user_text: parsed.message.trim().to_string(),
            attachments: Vec::new(),
            voice: parsed.voice,
            user_turn_id,
        }
    };
    let has_attachments = !submit.attachments.is_empty();
    if submit.user_text.is_empty() && !has_attachments {
        return json_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "message must not be empty",
        );
    }

    let active = match chat::get_session(&state.db, &user.id, &session_id).await {
        Ok(Some(s)) => s,
        Ok(None) => return not_found_conversation(),
        Err(err) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                &err.to_string(),
            );
        }
    };

    // Same request facts the HTML composer captures pre-parse.
    let ctx = RequestCtx {
        client_ip: None,
        secure: state.public_url().starts_with("https://"),
        voice_mode: submit.voice,
    };
    match submit_turn(&state, &user, &active, submit, ctx).await {
        Ok(submitted) => ok_json(
            StatusCode::ACCEPTED,
            json!({
                "user_turn_id": submitted.user_turn.id,
                "assistant_turn_id": submitted.assistant_turn.id,
            }),
        ),
        Err(SubmitTurnError::RateLimited) => json_error(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "rate limit or quota exceeded — see /usage",
        ),
        Err(SubmitTurnError::Busy) => json_error(
            StatusCode::CONFLICT,
            "turn_in_progress",
            "this user's previous turn is still streaming — cancel it first",
        ),
        Err(SubmitTurnError::Db(msg)) => {
            json_error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", &msg)
        }
    }
}

/// POST /api/v0/chat/sessions/{id}/cancel — ask the live worker to stop at
/// its next checkpoint. Idempotent: cancelling with nothing running is an
/// honest `{"cancelled": false}`, not an error.
pub async fn session_cancel(
    Path(session_id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = match require_session_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    if !user_owns(&state, &user.id, &session_id).await {
        return not_found_conversation();
    }
    let cancelled = session_core::chat::cancel_turn(&state.chats, &user.id, &session_id);
    ok_json(StatusCode::OK, json!({ "cancelled": cancelled }))
}

/// GET /api/v0/chat/sessions/{id}/events — the JSON-SSE stream.
///
/// Attach semantics (mirrors the legacy `/chat/{id}/tail` handshake, on the
/// JSON wire):
///
/// * A worker is live for this session → one `snapshot` (with
///   `live_turn_id`) then deltas until `turn_finalized`, then the stream
///   closes.
/// * Nothing live → one `snapshot` plus `idle`, and the stream closes
///   immediately. The client closes its EventSource on `idle`/`turn_finalized`
///   so the auto-reconnect doesn't hammer a quiet session.
///
/// Reconnect is the DB replay: a fresh attach's `snapshot` subsumes
/// everything missed — no `Last-Event-ID` machinery by design.
pub async fn session_events(
    Path(session_id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = match require_session_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    match readable_session(&state, &user.id, &session_id).await {
        Ok(Some(_)) => {}
        Ok(None) => return not_found_conversation(),
        Err(resp) => return resp,
    }

    // Owners attach to their own live worker; a shared-session viewer finds
    // none (workers are keyed by owner) and reads the static snapshot —
    // same behaviour as the legacy tail.
    let live = state
        .chats
        .get(&user.id)
        .filter(|w| w.session_id == session_id);

    match live {
        Some(worker) => {
            // Subscribe BEFORE reading the snapshot: anything the worker
            // commits after this point arrives as a tick; anything before
            // is in the snapshot. No gap.
            let broadcast_rx = worker.broadcast.subscribe();
            let turns = match chat::list_turns(&state.db, &session_id).await {
                Ok(t) => t,
                Err(err) => {
                    return json_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "internal_error",
                        &err.to_string(),
                    );
                }
            };
            let (tx, rx) = rama::futures::channel::mpsc::unbounded::<
                Result<rama::bytes::Bytes, std::io::Error>,
            >();
            let initial = vec![session_core::chat_json::ChatEvent::Snapshot {
                live_turn_id: Some(worker.turn_id.clone()),
                turns,
            }];
            tokio::spawn(session_core::chat_json::run_json_turn_stream(
                state.db.clone(),
                session_id,
                worker.turn_id.clone(),
                broadcast_rx,
                initial,
                tx,
            ));
            session_core::chat_json::json_stream_response(rx)
        }
        None => {
            let turns = match chat::list_turns(&state.db, &session_id).await {
                Ok(t) => t,
                Err(err) => {
                    return json_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "internal_error",
                        &err.to_string(),
                    );
                }
            };
            session_core::chrome::sse_response(&[
                session_core::chat_json::sse_json(&session_core::chat_json::ChatEvent::Snapshot {
                    live_turn_id: None,
                    turns,
                }),
                session_core::chat_json::sse_json(&session_core::chat_json::ChatEvent::Idle),
            ])
        }
    }
}

fn not_found_conversation() -> Response {
    json_error(StatusCode::NOT_FOUND, "not_found", "no such conversation")
}

async fn user_owns(state: &RamaState, user_id: &str, session_id: &str) -> bool {
    matches!(
        chat::get_session(&state.db, user_id, session_id).await,
        Ok(Some(_))
    )
}

async fn readable_session(
    state: &RamaState,
    user_id: &str,
    session_id: &str,
) -> Result<Option<chat::Session>, Response> {
    match chat::get_session_readable(&state.db, user_id, session_id).await {
        Ok(s) => Ok(s),
        Err(err) => Err(json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            &err.to_string(),
        )),
    }
}

// ---------------------------------------------------------------------------
// Turn actions (retry/edit/share/fork/effort/export) — issue #22 P5/P6.

/// Named (not positional) path extraction: rama's tuple `Path` reads
/// captures in a non-deterministic order, so any route with two or more
/// params must map by name — the same reason the legacy handlers use
/// `TurnPath`.
#[derive(serde::Deserialize)]
pub struct RetryBody {
    pub model: String,
}

/// POST /api/v0/chat/sessions/{id}/turns/{turn_id}/retry — drop this
/// assistant reply + everything below, regenerate from the preceding user
/// turn. 202 + turn ids like a fresh submit; the reply streams on /events.
pub async fn turn_retry(
    Path(TurnPath {
        id: session_id,
        turn_id,
    }): Path<TurnPath>,
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
        Err(msg) => return json_error(StatusCode::BAD_REQUEST, "invalid_request", &msg),
    };
    let parsed: RetryBody = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => {
            return json_error(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                &format!("parsing the retry body: {err}"),
            );
        }
    };
    let turn = match load_owned_turn(&state, &user, &session_id, &turn_id).await {
        Ok(t) => t,
        Err(resp) => return resp,
    };
    if turn.role != chat::TurnRole::Assistant {
        return json_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "only assistant turns can be retried",
        );
    }
    let orphaned = super::doomed_attachments(&state, &session_id, turn.seq).await;
    if let Err(err) = chat::delete_turns_from_seq(&state.db, &session_id, turn.seq).await {
        return json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            &err.to_string(),
        );
    }
    super::reclaim_attachments(&state, orphaned);
    match start_regeneration_json(&state, &user, &session_id, parsed.model).await {
        Ok(ids) => ok_json(StatusCode::ACCEPTED, ids),
        Err(resp) => resp,
    }
}

#[derive(serde::Deserialize)]
pub struct EditBody {
    pub model: String,
    pub message: String,
}

/// POST /api/v0/chat/sessions/{id}/turns/{turn_id}/edit — rewrite a user
/// message, drop everything below, regenerate.
pub async fn turn_edit(
    Path(TurnPath {
        id: session_id,
        turn_id,
    }): Path<TurnPath>,
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
        Err(msg) => return json_error(StatusCode::BAD_REQUEST, "invalid_request", &msg),
    };
    let parsed: EditBody = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => {
            return json_error(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                &format!("parsing the edit body: {err}"),
            );
        }
    };
    let text = parsed.message.trim().to_string();
    if text.is_empty() {
        return json_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "the message must not be empty",
        );
    }
    let turn = match load_owned_turn(&state, &user, &session_id, &turn_id).await {
        Ok(t) => t,
        Err(resp) => return resp,
    };
    if turn.role != chat::TurnRole::User {
        return json_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "only your own messages can be edited",
        );
    }
    if let Err(err) = chat::update_user_turn_content(&state.db, &session_id, &turn_id, &text).await
    {
        return json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            &err.to_string(),
        );
    }
    let orphaned = super::doomed_attachments(&state, &session_id, turn.seq + 1).await;
    if let Err(err) = chat::delete_turns_from_seq(&state.db, &session_id, turn.seq + 1).await {
        return json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            &err.to_string(),
        );
    }
    super::reclaim_attachments(&state, orphaned);
    match start_regeneration_json(&state, &user, &session_id, parsed.model).await {
        Ok(ids) => ok_json(StatusCode::ACCEPTED, ids),
        Err(resp) => resp,
    }
}

/// The regeneration half shared by retry + edit: reserve the worker, insert
/// the in-progress assistant row, spawn. Returns the ids a fresh submit
/// would return.
async fn start_regeneration_json(
    state: &Arc<RamaState>,
    user: &User,
    session_id: &str,
    model: String,
) -> Result<serde_json::Value, Response> {
    let assistant_turn_id = uuid::Uuid::new_v4().to_string();
    let worker = match state
        .chats
        .register(&user.id, &assistant_turn_id, session_id)
    {
        session_core::RegisterOutcome::Registered { worker } => worker,
        session_core::RegisterOutcome::Busy { .. } => {
            return Err(json_error(
                StatusCode::CONFLICT,
                "turn_in_progress",
                "this user's previous turn is still streaming — cancel it first",
            ));
        }
    };
    let assistant_turn = match chat::create_assistant_turn_in_progress(
        &state.db,
        session_id,
        &assistant_turn_id,
        &model,
    )
    .await
    {
        Ok(t) => t,
        Err(err) => {
            state.chats.clear(&user.id, &worker);
            return Err(json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                &err.to_string(),
            ));
        }
    };
    let _ = chat::touch_session(&state.db, session_id).await;
    super::spawn_assistant_worker(
        state,
        user,
        session_id,
        &assistant_turn_id,
        &model,
        &worker,
        super::RequestCtx {
            client_ip: None,
            secure: state.public_url().starts_with("https://"),
            voice_mode: false,
        },
    )
    .await;
    Ok(serde_json::json!({
        "user_turn_id": serde_json::Value::Null,
        "assistant_turn_id": assistant_turn.id,
    }))
}

async fn load_owned_turn(
    state: &RamaState,
    user: &User,
    session_id: &str,
    turn_id: &str,
) -> Result<chat::Turn, Response> {
    match chat::get_session(&state.db, &user.id, session_id).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return Err(json_error(
                StatusCode::NOT_FOUND,
                "not_found",
                "no such conversation",
            ));
        }
        Err(err) => {
            return Err(json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                &err.to_string(),
            ));
        }
    }
    match chat::get_turn(&state.db, session_id, turn_id).await {
        Ok(Some(t)) => Ok(t),
        Ok(None) => Err(json_error(
            StatusCode::NOT_FOUND,
            "not_found",
            "no such turn",
        )),
        Err(err) => Err(json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            &err.to_string(),
        )),
    }
}

#[derive(serde::Deserialize)]
pub struct ShareBody {
    pub shared: bool,
}

/// POST /api/v0/chat/sessions/{id}/share — toggle the read-only share flag
/// (any signed-in user with the id may then read).
pub async fn session_share(
    Path(session_id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = match require_session_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    if !user_owns(&state, &user.id, &session_id).await {
        return not_found_conversation();
    }
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return json_error(StatusCode::BAD_REQUEST, "invalid_request", &msg),
    };
    let parsed: ShareBody = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => {
            return json_error(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                &format!("parsing the share body: {err}"),
            );
        }
    };
    match chat::set_shared(&state.db, &user.id, &session_id, parsed.shared).await {
        Ok(_) => ok_json(
            StatusCode::OK,
            serde_json::json!({ "shared": parsed.shared }),
        ),
        Err(err) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            &err.to_string(),
        ),
    }
}

#[derive(serde::Deserialize)]
pub struct EffortBody {
    /// fast | standard | deep | max
    pub effort: String,
}

/// POST /api/v0/chat/sessions/{id}/effort — the per-conversation reasoning
/// effort knob.
pub async fn session_effort(
    Path(session_id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = match require_session_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    if !user_owns(&state, &user.id, &session_id).await {
        return not_found_conversation();
    }
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return json_error(StatusCode::BAD_REQUEST, "invalid_request", &msg),
    };
    let parsed: EffortBody = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => {
            return json_error(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                &format!("parsing the effort body: {err}"),
            );
        }
    };
    if !matches!(parsed.effort.as_str(), "fast" | "standard" | "deep" | "max") {
        return json_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "effort must be fast | standard | deep | max",
        );
    }
    match gateway_core::server::db::chat_session_settings::set_effort(
        &state.db,
        &session_id,
        &parsed.effort,
    )
    .await
    {
        Ok(()) => ok_json(
            StatusCode::OK,
            serde_json::json!({ "effort": parsed.effort }),
        ),
        Err(err) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            &err.to_string(),
        ),
    }
}

/// GET /api/v0/chat/sessions/{id}/export.md — the conversation as a
/// self-contained Markdown document.
pub async fn session_export_markdown(
    Path(session_id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    use rama::http::header;
    let (_session, user) = match require_session_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let session = match readable_session(&state, &user.id, &session_id).await {
        Ok(Some(s)) => s,
        Ok(None) => return not_found_conversation(),
        Err(resp) => return resp,
    };
    let turns = match chat::list_turns(&state.db, &session_id).await {
        Ok(t) => t,
        Err(err) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                &err.to_string(),
            );
        }
    };
    let opts = session_core::export::ExportOpts {
        base_url: &state.public_url(),
    };
    let body = session_core::export::to_markdown(&session, &turns, &opts);
    let filename = format!(
        "{}.md",
        session
            .title
            .as_deref()
            .map(super::first_message_title)
            .unwrap_or_else(|| session.id.clone())
            .replace(['/', ' '], "-")
    );
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/markdown; charset=utf-8")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", filename.trim_matches('-')),
        )
        .body(body.into())
        .expect("static file response")
}

// ---------------------------------------------------------------------------
// Per-conversation capabilities (tool overlay)

/// GET /api/v0/chat/sessions/{id}/capabilities — the caller's granted tools
/// with this conversation's on/off overlay applied.
pub async fn capabilities_list(
    Path(session_id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = match require_session_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    if !user_owns(&state, &user.id, &session_id).await {
        return not_found_conversation();
    }
    let entries = crate::pages::tool_toggles::entries_for_roles(&state, &user.roles);
    let overlay = gateway_core::server::db::chat_session_tools::enabled_keys_for_session(
        &state.db,
        &session_id,
    )
    .await
    .unwrap_or_default();
    let tools: Vec<_> = entries
        .into_iter()
        .map(|e| {
            serde_json::json!({
                "key": e.key,
                "title": e.title,
                "enabled": overlay.contains(&e.key),
            })
        })
        .collect();
    ok_json(StatusCode::OK, serde_json::json!({ "tools": tools }))
}

#[derive(serde::Deserialize)]
pub struct CapabilityBody {
    pub tool_key: String,
    pub enabled: bool,
}

/// POST /api/v0/chat/sessions/{id}/capabilities — set one tool's overlay
/// state for this conversation.
pub async fn capabilities_set(
    Path(session_id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = match require_session_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    if !user_owns(&state, &user.id, &session_id).await {
        return not_found_conversation();
    }
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return json_error(StatusCode::BAD_REQUEST, "invalid_request", &msg),
    };
    let parsed: CapabilityBody = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => {
            return json_error(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                &format!("parsing the capability body: {err}"),
            );
        }
    };
    match gateway_core::server::db::chat_session_tools::set(
        &state.db,
        &session_id,
        &parsed.tool_key,
        parsed.enabled,
        "manual",
    )
    .await
    {
        Ok(()) => ok_json(
            StatusCode::OK,
            serde_json::json!({ "tool_key": parsed.tool_key, "enabled": parsed.enabled }),
        ),
        Err(err) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            &err.to_string(),
        ),
    }
}
