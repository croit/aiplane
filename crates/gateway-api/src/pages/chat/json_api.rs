// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The `/api/v0/chat/*` JSON surface for the SvelteKit SPA: session CRUD,
//! the submit, and the JSON-SSE event stream.
//!
//! This module only translates. The turn machinery lives in `mod.rs`
//! ([`submit_turn`]) and the wire format in `session_core::chat_json`; what
//! is here maps requests onto [`ChatSubmit`], results onto JSON, and the
//! worker's broadcast onto the event protocol.
//!
//! Wire contract used by the chat JSON API:
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

use super::{
    ChatSubmit, DocumentPath, RequestCtx, SteerPath, SubmitTurnError, TurnPath, submit_turn,
};
use crate::pages::{bad_request, internal, json_error, json_ok as ok_json, not_found, read_json};
use session_core::db as chat;

/// The request facts a turn needs, read off the request while it is intact.
///
/// Must be called before `req.into_parts()`: `peer_ip` wants the whole
/// request, not just its headers. `client_ip` is the sole input to the GeoIP
/// path in `get_user_location` — the fallback for when the browser declines
/// to share a precise position — so a hardcoded `None` leaves that tool
/// waiting out its timeout and then failing. `transport_is_secure` reads the
/// forwarded-proto header rather than just the configured public URL, so a
/// gateway behind a TLS-terminating proxy is not reported as plaintext.
fn request_ctx(state: &RamaState, req: &Request, voice_mode: bool) -> RequestCtx {
    RequestCtx {
        client_ip: gateway_features::server::geoip::client_ip(req.headers())
            .or_else(|| gateway_features::server::geoip::peer_ip(req)),
        secure: gateway_features::server::geoip::transport_is_secure(
            req.headers(),
            &state.public_url(),
        ),
        voice_mode,
    }
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
    let (_session, user) = require_session_json!(state, req);
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
        Err(err) => internal(err),
    }
}

#[derive(serde::Deserialize, Default)]
pub struct SessionsQuery {
    q: Option<String>,
}

/// GET /api/v0/chat/landing — resolve the latest conversation, creating the
/// caller's first conversation when their workspace is empty.
pub async fn session_landing(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, user) = require_session_json!(state, req);
    let session = match chat::latest_session(&state.db, &user.id).await {
        Ok(Some(session)) => session,
        Ok(None) => match chat::create_session(&state.db, &user.id).await {
            Ok(session) => session,
            Err(err) => return internal(err),
        },
        Err(err) => return internal(err),
    };
    ok_json(StatusCode::OK, json!({ "session": session }))
}

/// POST /api/v0/chat/sessions — mint an empty conversation.
pub async fn session_create(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, user) = require_session_json!(state, req);
    match chat::create_session(&state.db, &user.id).await {
        Ok(session) => ok_json(StatusCode::CREATED, json!({ "session": session })),
        Err(err) => internal(err),
    }
}

/// GET /api/v0/chat/sessions/{id} — one conversation with every turn.
/// Readable by the owner and, for shared sessions, any signed-in user.
pub async fn session_get(
    Path(session_id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = require_session_json!(state, req);
    let session = match readable_session(&state, &user.id, &session_id).await {
        Ok(Some(s)) => s,
        Ok(None) => return not_found_conversation(),
        Err(resp) => return resp,
    };
    let turns = match chat::list_turns(&state.db, &session_id).await {
        Ok(t) => t,
        Err(err) => {
            return internal(err);
        }
    };
    let compacted_up_to_seq =
        match gateway_core::server::db::chat_compactions::get(&state.db, &session_id).await {
            Ok(compaction) => compaction.map(|value| value.up_to_seq),
            Err(err) => return internal(err),
        };
    let assets = match gateway_features::server::chat_attachments::list_session_attachments(
        &state.db,
        &session_id,
    )
    .await
    {
        Ok(assets) => assets
            .into_iter()
            .map(|asset| {
                json!({
                    "id": asset.id,
                    "turn_id": asset.turn_id,
                    "filename": asset.filename,
                    "mime": asset.mime,
                    "size": asset.size,
                    "url": gateway_features::server::chat_attachments::proxy_url(
                        &asset.turn_id,
                        &asset.filename,
                    ),
                })
            })
            .collect::<Vec<_>>(),
        Err(err) => return internal(err),
    };
    ok_json(
        StatusCode::OK,
        json!({
            "session": session,
            "turns": turns,
            "compacted_up_to_seq": compacted_up_to_seq,
            "assets": assets,
        }),
    )
}

/// DELETE /api/v0/chat/sessions/{id} — owner-only. Also reclaims the
/// session's attachment objects, exactly like the legacy form route.
pub async fn session_delete(
    Path(session_id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = require_session_json!(state, req);
    if !user_owns(&state, &user.id, &session_id).await {
        return not_found_conversation();
    }
    // Deleting the conversation deletes its files: seq 0 covers every turn.
    // Read before the rows go — see `doomed_attachments`.
    let orphaned = super::doomed_attachments(&state, &session_id, 0).await;
    let deleted = match chat::delete_session(&state.db, &user.id, &session_id).await {
        Ok(v) => v,
        Err(err) => {
            return internal(err);
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
    let (_session, user) = require_session_json!(state, req);
    if !user_owns(&state, &user.id, &session_id).await {
        return not_found_conversation();
    }
    let (_, body) = req.into_parts();
    let parsed: PinBody = match read_json(body, "the pin body").await {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    match chat::set_pinned(&state.db, &user.id, &session_id, parsed.pinned).await {
        Ok(true) => ok_json(StatusCode::OK, json!({ "pinned": parsed.pinned })),
        Ok(false) => not_found_conversation(),
        Err(err) => internal(err),
    }
}

#[derive(Deserialize)]
struct PinBody {
    pinned: bool,
}

/// POST /api/v0/chat/sessions/{id}/fork — take a readable conversation into
/// the caller's own chats as an editable copy.
///
/// Recipient-only, exactly like the legacy handler: the source must be
/// readable (owner or shared) and must NOT already be the caller's. Forking
/// your own conversation is a no-op rather than a clone — the affordance only
/// renders for read-only viewers, but a hand-crafted POST must not let anyone
/// clone-spam their own sessions either.
pub async fn session_fork(
    Path(session_id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = require_session_json!(state, req);
    let src = match readable_session(&state, &user.id, &session_id).await {
        Ok(Some(s)) => s,
        Ok(None) => return not_found_conversation(),
        Err(resp) => return resp,
    };
    if src.user_id == user.id {
        return json_error(
            StatusCode::CONFLICT,
            "already_yours",
            "this conversation is already in your chats",
        );
    }

    let (new_session, copies) = match chat::fork_session(&state.db, &src, &user.id).await {
        Ok(v) => v,
        Err(err) => {
            return internal(err);
        }
    };

    // Best-effort attachment copy, same policy as the legacy fork: a failed
    // object copy leaves a marker pointing at an empty key (a broken preview)
    // while the conversation text — the thing being forked — still lands, so
    // it warns rather than rolling the whole fork back.
    if let Some(cfg) = state.config().chat.s3.as_ref() {
        for c in &copies {
            if let Err(err) = gateway_features::server::chat_attachments::copy_object(
                cfg,
                &c.from_turn_id,
                &c.to_turn_id,
                &c.filename,
            )
            .await
            {
                tracing::warn!(
                    from = %c.from_turn_id, file = %c.filename,
                    "fork: failed to copy attachment object: {err}"
                );
            }
        }
    } else if !copies.is_empty() {
        tracing::warn!(
            count = copies.len(),
            "fork: chat attachments not configured; copied conversation references unreachable files"
        );
    }

    ok_json(
        StatusCode::CREATED,
        json!({ "id": new_session.id, "title": new_session.title }),
    )
}

#[derive(Deserialize)]
struct MessageBody {
    model: String,
    message: String,
    #[serde(default)]
    voice: bool,
    /// Ids of the interjections this submit is the re-send of.
    ///
    /// A note the finished turn never reached stays `pending`, and the client
    /// sends it as an ordinary message. Naming the rows here is what makes
    /// that safe with two tabs open on the same conversation: settling is
    /// `WHERE status = 'pending'`, so exactly one submit can claim a note and
    /// the loser is refused instead of duplicating the message.
    #[serde(default)]
    redeem_steers: Vec<String>,
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
    let (_session, user) = require_session_json!(state, req);
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

    // The same request facts the HTML composer captured. Read off `req` while
    // it is still whole — `into_parts` below takes it apart.
    let mut ctx = request_ctx(&state, &req, false);

    // Ownership BEFORE the body is parsed. Parsing a multipart submit uploads
    // every attachment to S3 under this turn's prefix, and we will not spend
    // storage on a turn the caller does not own: no turn row is created on the
    // 404 path, so `doomed_attachments`/`reclaim_attachments` can never find
    // those objects to sweep them. The quota enforcer runs later still.
    let active = match chat::get_session(&state.db, &user.id, &session_id).await {
        Ok(Some(s)) => s,
        Ok(None) => return not_found_conversation(),
        Err(err) => {
            return internal(err);
        }
    };

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
                return bad_request(format!("parsing the message body: {err}"));
            }
        };
        ChatSubmit {
            model: parsed.model,
            user_text: parsed.message.trim().to_string(),
            attachments: Vec::new(),
            voice: parsed.voice,
            user_turn_id,
            redeem_steers: parsed.redeem_steers,
        }
    };

    let has_attachments = !submit.attachments.is_empty();
    if submit.user_text.is_empty() && !has_attachments {
        return bad_request("message must not be empty");
    }

    // Claim the interjections this message stands in for, before the turn is
    // created. Losing the race means another tab already re-sent them, so the
    // honest answer is to refuse this submit rather than to say the same
    // sentence twice.
    //
    // The claim is a *reservation*, not a record of what happened: if the
    // submit below is refused, every id in `claimed` is released again. Leaving
    // them claimed would mark a note as dealt with while its sentence was never
    // sent, and the client — told "already settled" on the retry — would drop
    // its copy. That is a silent loss of something the user typed.
    let claimed = match chat::claim_steers(&state.db, &active.id, &submit.redeem_steers).await {
        Ok(claimed) => claimed,
        Err(err) => return internal(err),
    };
    // Every named note or none. A partial claim means somebody else already
    // re-sent one of them, and going ahead would put that sentence in front of
    // the model twice — once from the tab that won the claim, once from this
    // message. The ones this call did take are released on the way out.
    if claimed.len() != submit.redeem_steers.len() {
        release_claimed_steers(&state, &claimed).await;
        return json_error(
            StatusCode::CONFLICT,
            "steer_already_settled",
            "these interjections were already answered for elsewhere",
        );
    }

    ctx.voice_mode = submit.voice;
    let outcome = submit_turn(&state, &user, &active, submit, ctx).await;
    if outcome.is_err() {
        release_claimed_steers(&state, &claimed).await;
    }
    match outcome {
        Ok(submitted) => ok_json(
            StatusCode::ACCEPTED,
            json!({
                "user_turn_id": submitted.user_turn.id,
                "assistant_turn_id": submitted.assistant_turn.id,
            }),
        ),
        Err(err) => submit_refusal(err),
    }
}

/// The one place a refused submit becomes a response.
///
/// Both submit paths — a fresh message and a retry/edit regeneration — refuse
/// for the same reasons, and the client branches on `error.code`, so these
/// codes are a wire contract. Written twice, they were free to drift; written
/// once, a new refusal reaches both callers by construction.
fn submit_refusal(err: SubmitTurnError) -> Response {
    match err {
        SubmitTurnError::RateLimited => json_error(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "rate limit or quota exceeded — see /usage",
        ),
        SubmitTurnError::Busy => json_error(
            StatusCode::CONFLICT,
            "turn_in_progress",
            "this conversation is still streaming a turn — cancel it first",
        ),
        // A distinct code, because the client's remedy is distinct: this
        // conversation is idle and the message is worth holding in the
        // composer's queue until one of the user's other chats finishes.
        SubmitTurnError::AtCapacity { running, limit } => json_error(
            StatusCode::CONFLICT,
            "at_capacity",
            &format!(
                "{running} of {limit} parallel conversations are already \
                 streaming for this user"
            ),
        ),
        SubmitTurnError::Db(msg) => {
            json_error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", &msg)
        }
    }
}

#[derive(Deserialize)]
struct SteerBody {
    message: String,
    /// Interjections this one stands in for.
    ///
    /// A note the last turn never reached can be taken back into the composer
    /// and thrown at the *next* turn while that one is still running. It is
    /// still that note's re-send, so it settles the same way a message does —
    /// otherwise the row stays `pending` and the client queues the sentence a
    /// second time when the turn ends.
    #[serde(default)]
    redeem_steers: Vec<String>,
}

/// Ceiling on one interjection, in bytes.
///
/// Generous for a sentence or two of correction, which is what this is for,
/// and far below anything that meaningfully grows the prompt. Longer than this
/// is a message, and the composer already sends those.
const MAX_STEER_BYTES: usize = 4 * 1024;

/// Give back interjection claims whose message was refused.
///
/// Best-effort and deliberately silent: the submit already failed and its own
/// error is what the caller gets. A release that fails leaves the note marked
/// `resent` — the one outcome this exists to avoid — so it is logged loudly
/// enough to find, and no louder.
async fn release_claimed_steers(state: &Arc<RamaState>, claimed: &[String]) {
    for id in claimed {
        if let Err(err) = chat::release_steer(&state.db, id, chat::SteerStatus::Resent).await {
            tracing::error!(error = %err, steer = %id, "could not release a claimed interjection");
        }
    }
}

/// POST /api/v0/chat/sessions/{id}/steer — say something to the turn that is
/// already running.
///
/// Accepted (`202`) means the note was recorded and handed to the live
/// worker, NOT that the model has seen it: it is folded into the prompt at the
/// next round boundary, and a turn that ends before reaching one never carries
/// it. The reply therefore reports the note's id and its status, and the
/// client watches the transcript for the outcome rather than assuming one.
///
/// With nothing running, this is a `409` and not a silent promotion to an
/// ordinary message — the client has a composer for that, and quietly turning
/// an interjection into a new turn would start work the user did not ask for.
pub async fn session_steer(
    Path(session_id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = require_session_json!(state, req);
    if !user_owns(&state, &user.id, &session_id).await {
        return not_found_conversation();
    }
    // Same gate a message goes through, and the same response. An interjection
    // is not free: it is a row, and it is one more user message folded into
    // every remaining round of the prompt, so an unbounded stream of them
    // inflates the upstream request at nobody's expense but the operator's.
    {
        let role_ids = state.role_ids_for(&user.roles);
        if state.enforcer.check(&user.id, &role_ids).await.is_err() {
            return submit_refusal(SubmitTurnError::RateLimited);
        }
    }
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return json_error(StatusCode::BAD_REQUEST, "invalid_request", &msg),
    };
    let parsed: SteerBody = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => return bad_request(format!("parsing the steer body: {err}")),
    };
    let text = parsed.message.trim().to_string();
    if text.is_empty() {
        return bad_request("message must not be empty");
    }
    if text.len() > MAX_STEER_BYTES {
        return bad_request(format!(
            "an interjection is at most {MAX_STEER_BYTES} bytes; send a message instead"
        ));
    }

    let Some(worker) = state.chats.get(&user.id, &session_id) else {
        return json_error(
            StatusCode::CONFLICT,
            "no_turn_running",
            "nothing is streaming in this conversation — send it as a message instead",
        );
    };

    // Settle whatever this interjection stands in for, before recording it.
    // Same all-or-nothing rule as the message path, for the same reason.
    let claimed = match chat::claim_steers(&state.db, &session_id, &parsed.redeem_steers).await {
        Ok(claimed) => claimed,
        Err(err) => return internal(err),
    };
    if claimed.len() != parsed.redeem_steers.len() {
        release_claimed_steers(&state, &claimed).await;
        return json_error(
            StatusCode::CONFLICT,
            "steer_already_settled",
            "these interjections were already answered for elsewhere",
        );
    }

    // The insert is conditional on the turn still running, so the answer
    // finishing between the lookup above and this write is a `None` rather
    // than a row against a finished turn.
    let steer = match chat::insert_steer(&state.db, &worker.turn_id, &text).await {
        Ok(Some(steer)) => steer,
        Ok(None) => {
            // Nothing was recorded, so nothing was re-sent either.
            release_claimed_steers(&state, &claimed).await;
            return json_error(
                StatusCode::CONFLICT,
                "no_turn_running",
                "the answer finished before this reached it — send it as a message instead",
            );
        }
        Err(err) => {
            release_claimed_steers(&state, &claimed).await;
            return internal(err);
        }
    };
    worker.steers.push(session_core::workers::SteerNote {
        id: steer.id.clone(),
        text,
    });

    // Tick so every attached viewer re-reads the turn and draws the note
    // straight away — it is part of the conversation from the moment it is
    // typed, not from the moment the model happens to read it.
    let _ = worker
        .broadcast
        .send(session_core::workers::TurnUpdate::Tick);

    ok_json(
        StatusCode::ACCEPTED,
        json!({
            "id": steer.id,
            "turn_id": steer.turn_id,
            "status": steer.status.as_str(),
        }),
    )
}

/// POST /api/v0/chat/sessions/{id}/steer/{steer_id}/discard — the user threw
/// away an interjection the turn never reached, rather than re-sending it.
///
/// Without this the client could only forget it locally, and the next time any
/// turn in the conversation finished, the still-`pending` row would be read
/// back and queued again — a dismissed sentence returning, and then sending
/// itself.
pub async fn steer_discard(
    Path(SteerPath {
        id: session_id,
        steer_id,
    }): Path<SteerPath>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = require_session_json!(state, req);
    if !user_owns(&state, &user.id, &session_id).await {
        return not_found_conversation();
    }
    let steer = match chat::get_steer(&state.db, &session_id, &steer_id).await {
        Ok(Some(steer)) => steer,
        Ok(None) => return not_found_conversation(),
        Err(err) => return internal(err),
    };
    match chat::settle_steer(&state.db, &steer.id, chat::SteerStatus::Discarded).await {
        // Already settled: delivered, re-sent, or discarded in another tab.
        // Nothing to do and nothing to complain about — the caller wanted it
        // gone and it is.
        Ok(_) => ok_json(StatusCode::OK, json!({ "id": steer.id })),
        Err(err) => internal(err),
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
    let (_session, user) = require_session_json!(state, req);
    if !user_owns(&state, &user.id, &session_id).await {
        return not_found_conversation();
    }
    let cancelled = session_core::chat_json::cancel_turn(&state.chats, &user.id, &session_id);
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
    let (_session, user) = require_session_json!(state, req);
    match readable_session(&state, &user.id, &session_id).await {
        Ok(Some(_)) => {}
        Ok(None) => return not_found_conversation(),
        Err(resp) => return resp,
    }

    // Owners attach to their own live worker; a shared-session viewer finds
    // none (workers are keyed by owner) and reads the static snapshot —
    // same behaviour as the legacy tail.
    let live = state.chats.get(&user.id, &session_id);

    match live {
        Some(worker) => {
            // Subscribe BEFORE reading the snapshot: anything the worker
            // commits after this point arrives as a tick; anything before
            // is in the snapshot. No gap.
            let broadcast_rx = worker.broadcast.subscribe();
            let turns = match chat::list_turns(&state.db, &session_id).await {
                Ok(t) => t,
                Err(err) => {
                    return internal(err);
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
                    return internal(err);
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
        Err(err) => Err(internal(err)),
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
    let (_session, user) = require_session_json!(state, req);
    // Read off the whole request, before `into_parts` takes it apart.
    let ctx = request_ctx(&state, &req, false);
    let (_, body) = req.into_parts();
    let parsed: RetryBody = match read_json(body, "the retry body").await {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    let turn = match load_owned_turn(&state, &user, &session_id, &turn_id).await {
        Ok(t) => t,
        Err(resp) => return resp,
    };
    if turn.role != chat::TurnRole::Assistant {
        return bad_request("only assistant turns can be retried");
    }
    let orphaned = super::doomed_attachments(&state, &session_id, turn.seq).await;
    if let Err(err) = chat::delete_turns_from_seq(&state.db, &session_id, turn.seq).await {
        return internal(err);
    }
    super::reclaim_attachments(&state, orphaned);
    match start_regeneration_json(&state, &user, &session_id, parsed.model, ctx).await {
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
    let (_session, user) = require_session_json!(state, req);
    let ctx = request_ctx(&state, &req, false);
    let content_type = req
        .headers()
        .get(rama::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    let turn = match load_owned_turn(&state, &user, &session_id, &turn_id).await {
        Ok(t) => t,
        Err(resp) => return resp,
    };
    if turn.role != chat::TurnRole::User {
        return bad_request("only your own messages can be edited");
    }
    let (_, body) = req.into_parts();
    let (model, text) = if content_type.starts_with("multipart/form-data") {
        let bytes = match session_core::chrome::read_body_to_bytes(body).await {
            Ok(bytes) => bytes,
            Err(message) => {
                return json_error(StatusCode::BAD_REQUEST, "invalid_request", &message);
            }
        };
        match super::parse_chat_submit(&content_type, bytes, &turn_id, &state).await {
            Ok(submit) => (submit.model, submit.user_text.trim().to_string()),
            Err(message) => {
                return json_error(StatusCode::BAD_REQUEST, "invalid_request", &message);
            }
        }
    } else {
        let parsed: EditBody = match read_json(body, "the edit body").await {
            Ok(parsed) => parsed,
            Err(resp) => return resp,
        };
        (parsed.model, parsed.message.trim().to_string())
    };
    if text.is_empty() {
        return bad_request("the message must not be empty");
    }
    if let Err(err) = chat::update_user_turn_content(&state.db, &session_id, &turn_id, &text).await
    {
        return internal(err);
    }
    let orphaned = super::doomed_attachments(&state, &session_id, turn.seq + 1).await;
    if let Err(err) = chat::delete_turns_from_seq(&state.db, &session_id, turn.seq + 1).await {
        return internal(err);
    }
    super::reclaim_attachments(&state, orphaned);
    match start_regeneration_json(&state, &user, &session_id, model, ctx).await {
        Ok(ids) => ok_json(StatusCode::ACCEPTED, ids),
        Err(resp) => resp,
    }
}

/// The regeneration half shared by retry + edit: reserve the worker, insert
/// the in-progress assistant row, spawn. Returns the ids a fresh submit
/// would return.
/// Re-run the assistant for a conversation whose tail was just removed
/// (a retry or an edited message).
///
/// `ctx` is threaded in rather than rebuilt here: the request facts it carries
/// — the caller's IP and whether the transport was really TLS — only exist on
/// the `Request`, and `get_user_location` has nothing but `client_ip` to fall
/// back on when the browser declines to answer.
async fn start_regeneration_json(
    state: &Arc<RamaState>,
    user: &User,
    session_id: &str,
    model: String,
    ctx: super::RequestCtx,
) -> Result<serde_json::Value, Response> {
    let assistant_turn_id = uuid::Uuid::new_v4().to_string();
    let worker = match state.chats.register(
        &user.id,
        &assistant_turn_id,
        session_id,
        state.config().chat.turns.max_parallel,
    ) {
        session_core::RegisterOutcome::Registered { worker } => worker,
        session_core::RegisterOutcome::Busy { .. } => {
            return Err(submit_refusal(SubmitTurnError::Busy));
        }
        session_core::RegisterOutcome::AtCapacity { running, limit } => {
            return Err(submit_refusal(SubmitTurnError::AtCapacity {
                running,
                limit,
            }));
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
            return Err(internal(err));
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
        ctx,
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
            return Err(not_found("no such conversation"));
        }
        Err(err) => {
            return Err(internal(err));
        }
    }
    match chat::get_turn(&state.db, session_id, turn_id).await {
        Ok(Some(t)) => Ok(t),
        Ok(None) => Err(not_found("no such turn")),
        Err(err) => Err(internal(err)),
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
    let (_session, user) = require_session_json!(state, req);
    if !user_owns(&state, &user.id, &session_id).await {
        return not_found_conversation();
    }
    let (_, body) = req.into_parts();
    let parsed: ShareBody = match read_json(body, "the share body").await {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    match chat::set_shared(&state.db, &user.id, &session_id, parsed.shared).await {
        Ok(_) => ok_json(
            StatusCode::OK,
            serde_json::json!({ "shared": parsed.shared }),
        ),
        Err(err) => internal(err),
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
    let (_session, user) = require_session_json!(state, req);
    if !user_owns(&state, &user.id, &session_id).await {
        return not_found_conversation();
    }
    let (_, body) = req.into_parts();
    let parsed: EffortBody = match read_json(body, "the effort body").await {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    if !matches!(parsed.effort.as_str(), "fast" | "standard" | "deep" | "max") {
        return bad_request("effort must be fast | standard | deep | max");
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
        Err(err) => internal(err),
    }
}

/// GET /api/v0/chat/sessions/{id}/export.md — the conversation as a
/// self-contained Markdown document.
pub async fn session_export_markdown(
    Path(session_id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = require_session_json!(state, req);
    let session = match readable_session(&state, &user.id, &session_id).await {
        Ok(Some(s)) => s,
        Ok(None) => return not_found_conversation(),
        Err(resp) => return resp,
    };
    let turns = match chat::list_turns(&state.db, &session_id).await {
        Ok(t) => t,
        Err(err) => {
            return internal(err);
        }
    };
    let opts = session_core::export::ExportOpts {
        base_url: &state.public_url(),
    };
    let body = session_core::export::to_markdown(&session, &turns, &opts);
    download(
        "text/markdown; charset=utf-8",
        &export_filename(&session, "md"),
        body.into_bytes(),
    )
}

/// GET /api/v0/chat/sessions/{id}/export.pdf — the same conversation as a
/// typeset PDF.
///
/// Needs the `typst` binary the container ships; where it is absent the
/// legacy page answers 503 and so does this, rather than a generic 500 —
/// "PDF export is not available on this deployment" is an operator fact, not
/// a bug in the request.
pub async fn session_export_pdf(
    Path(session_id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = require_session_json!(state, req);
    let session = match readable_session(&state, &user.id, &session_id).await {
        Ok(Some(s)) => s,
        Ok(None) => return not_found_conversation(),
        Err(resp) => return resp,
    };
    let turns = match chat::list_turns(&state.db, &session_id).await {
        Ok(t) => t,
        Err(err) => {
            return internal(err);
        }
    };
    let opts = session_core::export::ExportOpts {
        base_url: &state.public_url(),
    };
    let source = session_core::export::to_typst(&session, &turns, &opts);
    match gateway_features::server::typst::compile_source(&source).await {
        Ok(pdf) => download("application/pdf", &export_filename(&session, "pdf"), pdf),
        Err(gateway_features::server::typst::CompileError::BinaryNotFound) => json_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "unavailable",
            "PDF export is not available on this gateway (no typst binary)",
        ),
        Err(err) => {
            tracing::error!(error = %err, %session_id, "chat PDF export compile");
            internal("could not render the conversation as a PDF")
        }
    }
}

// ---------------------------------------------------------------------------
// Canvas documents
//
// Documents are plain JSON so the SPA can render (and diff) them itself.
// Reads follow the conversation's readability (owner or shared); the hand-edit
// is owner-only.

/// GET /api/v0/chat/sessions/{id}/documents — the conversation's documents,
/// most-recently-updated first. Soft-deleted ones stay hidden, as in the
/// browser listing.
pub async fn documents_list(
    Path(session_id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    use gateway_core::server::db::documents;

    let (_session, user) = require_session_json!(state, req);
    // Every arm, not just `Ok(None)`: matching only the "definitely not
    // readable" case let a DB error fall straight THROUGH the authorization
    // check and on into serving the documents. An authorization gate that
    // cannot answer must refuse, not shrug.
    match readable_session(&state, &user.id, &session_id).await {
        Ok(Some(_)) => {}
        Ok(None) => return not_found_conversation(),
        Err(resp) => return resp,
    }
    match documents::list_for_session(&state.db, &session_id, false).await {
        Ok(docs) => ok_json(
            StatusCode::OK,
            json!({
                "documents": docs.iter().map(document_summary).collect::<Vec<_>>(),
            }),
        ),
        Err(err) => internal(err),
    }
}

/// GET /api/v0/chat/sessions/{id}/documents/{doc_id} — one document with its
/// content and version history. `?version=N` reads an older revision; absent
/// means the current one.
pub async fn document_get(
    Path(DocumentPath {
        id: session_id,
        doc_id,
    }): Path<DocumentPath>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    use gateway_core::server::db::documents;

    let (_session, user) = require_session_json!(state, req);
    // Every arm, not just `Ok(None)`: matching only the "definitely not
    // readable" case let a DB error fall straight THROUGH the authorization
    // check and on into serving the documents. An authorization gate that
    // cannot answer must refuse, not shrug.
    match readable_session(&state, &user.id, &session_id).await {
        Ok(Some(_)) => {}
        Ok(None) => return not_found_conversation(),
        Err(resp) => return resp,
    }
    let version = req.uri().query().and_then(super::parse_version_query);
    let (doc, ver) = match documents::get_version(&state.db, &session_id, &doc_id, version).await {
        Ok(Some(pair)) => pair,
        Ok(None) => return json_error(StatusCode::NOT_FOUND, "not_found", "no such document"),
        Err(err) => {
            return internal(err);
        }
    };
    let history = documents::list_versions(&state.db, &session_id, &doc_id)
        .await
        .unwrap_or_default();
    ok_json(
        StatusCode::OK,
        json!({
            "document": document_summary(&doc),
            "version": {
                "version": ver.version,
                "content": ver.content,
                "summary": ver.summary,
                "turn_id": ver.turn_id,
                "author": ver.author.as_str(),
                "created_at": ver.created_at.to_string(),
            },
            "history": history.iter().map(|v| json!({
                "version": v.version,
                "summary": v.summary,
                "created_at": v.created_at.to_string(),
                "chars": v.chars,
                "author": v.author.as_str(),
            })).collect::<Vec<_>>(),
        }),
    )
}

#[derive(Deserialize)]
struct DocumentEditBody {
    content: String,
}

/// PUT /api/v0/chat/sessions/{id}/documents/{doc_id} — save a hand edit as a
/// new version.
///
/// Owner-only (a shared conversation is read-only). Mirrors the legacy
/// handler's two guards: the same size ceiling the document *tools* write
/// against, so a hand edit can never produce a document the model is then
/// unable to save back; and a no-op save mints no version, keeping the
/// history a list of actual changes.
pub async fn document_edit(
    Path(DocumentPath {
        id: session_id,
        doc_id,
    }): Path<DocumentPath>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    use gateway_core::server::db::documents;

    let (_session, user) = require_session_json!(state, req);
    if !user_owns(&state, &user.id, &session_id).await {
        return not_found_conversation();
    }
    let (_, body) = req.into_parts();
    let parsed: DocumentEditBody = match read_json(body, "the document body").await {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    if parsed.content.len() > documents::MAX_CONTENT_BYTES {
        return json_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "too_large",
            "this document is too large to save",
        );
    }
    // Normalise CRLFs the way the legacy textarea path does: browsers submit
    // `\r\n` per the HTML spec, and leaving them in shows up as a diff on
    // every line of an otherwise-untouched document (and confuses the model's
    // anchored find/replace, which matches on `\n`).
    let content = parsed.content.replace("\r\n", "\n");

    let doc = match documents::get(&state.db, &session_id, &doc_id).await {
        Ok(Some(d)) if !d.is_deleted() => d,
        Ok(_) => return json_error(StatusCode::NOT_FOUND, "not_found", "no such document"),
        Err(err) => {
            return internal(err);
        }
    };
    let unchanged = match documents::get_version(&state.db, &session_id, &doc_id, None).await {
        Ok(Some((_, ver))) => ver.content == content,
        Ok(None) => false,
        Err(err) => {
            return internal(err);
        }
    };
    if !unchanged
        && let Err(err) = documents::append_version(
            &state.db,
            &session_id,
            &doc_id,
            &content,
            Some("Edited by you"),
            None,
            documents::VersionAuthor::User,
        )
        .await
    {
        return internal(err);
    }
    tracing::info!(
        user_id = %user.id, %session_id, document_id = %doc_id,
        title = %doc.title, unchanged, "canvas document hand-edited",
    );
    // Answer with the document as it now stands so the client does not have
    // to guess the new version number.
    let current = documents::get(&state.db, &session_id, &doc_id)
        .await
        .ok()
        .flatten()
        .unwrap_or(doc);
    ok_json(
        StatusCode::OK,
        json!({ "document": document_summary(&current), "unchanged": unchanged }),
    )
}

/// The wire shape of a document. Hand-written rather than a `Serialize` derive
/// on the DB row: the API contract and the table are free to drift.
fn document_summary(doc: &gateway_core::server::db::documents::Document) -> serde_json::Value {
    json!({
        "id": doc.id,
        "title": doc.title,
        "format": doc.format.as_str(),
        "current_ver": doc.current_ver,
        "created_at": doc.created_at.to_string(),
        "updated_at": doc.updated_at.to_string(),
    })
}

/// DELETE /api/v0/chat/sessions/{id}/turns/{turn_id}/attachments/{filename}
/// — drop one attachment from a message.
///
/// Removes the `[gw-attachment …]` marker from whichever column owns it
/// (`user_content` for uploads, `content` for model-generated files) and
/// reclaims the object. Unlike edit/retry this does NOT regenerate the turn.
///
/// The filename is read from the raw URI rather than the `Path` extractor:
/// that extractor lowercases segments, and both the marker match and the S3
/// key need the name verbatim (`pic.PNG` is not `pic.png`).
pub async fn attachment_remove(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, user) = require_session_json!(state, req);
    let Some((session_id, turn_id, filename)) = attachment_delete_path_parts(req.uri().path())
    else {
        return bad_request("malformed attachment path");
    };
    // Owner-only: removing content is a mutation, so a shared (read-only)
    // viewer must not reach it even though they can read the turn.
    if !user_owns(&state, &user.id, &session_id).await {
        return not_found_conversation();
    }
    let turn = match chat::get_turn(&state.db, &session_id, &turn_id).await {
        Ok(Some(t)) => t,
        Ok(None) => return json_error(StatusCode::NOT_FOUND, "not_found", "no such message"),
        Err(err) => {
            return internal(err);
        }
    };

    let is_user = turn.role == chat::TurnRole::User;
    let content = if is_user {
        turn.user_content.clone().unwrap_or_default()
    } else {
        turn.content.clone().unwrap_or_default()
    };
    let new_content =
        session_core::attachments::remove_markers_where(&content, |a| a.filename == filename);
    let write = if is_user {
        chat::update_user_turn_content(&state.db, &session_id, &turn_id, &new_content)
            .await
            .map(|_| ())
    } else {
        chat::set_content(&state.db, &turn_id, &new_content).await
    };
    if let Err(err) = write {
        return internal(err);
    }

    // Reclaim the bytes. Best-effort, exactly as the legacy handler: the
    // marker is already gone, so a failed delete only orphans an object (a
    // later retry is safe — DELETE is idempotent) and must not fail the
    // user's action.
    if let Some(cfg) = state.config().chat.s3.as_ref()
        && let Err(err) =
            gateway_features::server::chat_attachments::delete(cfg, &turn_id, &filename).await
    {
        tracing::warn!(error = %err, %turn_id, %filename, "attachment S3 delete (marker already removed)");
    }

    ok_json(StatusCode::OK, json!({ "removed": filename }))
}

/// Split `/api/v0/chat/sessions/{id}/turns/{turn_id}/attachments/{filename}`
/// into its three parts, with the filename percent-decoded and its case
/// preserved. `None` when the shape does not match or the filename is empty.
fn attachment_delete_path_parts(path: &str) -> Option<(String, String, String)> {
    let (head, file) = path.rsplit_once("/attachments/")?;
    let filename = super::percent_decode_segment(file);
    if filename.is_empty() || filename.contains('/') {
        return None;
    }
    let (head, turn_id) = head.rsplit_once("/turns/")?;
    let session_id = head.rsplit_once("/sessions/")?.1;
    if session_id.is_empty() || turn_id.is_empty() || session_id.contains('/') {
        return None;
    }
    Some((session_id.to_string(), turn_id.to_string(), filename))
}

/// `<conversation title>.<ext>`, path-separator free — the name the browser
/// saves the download under.
fn export_filename(session: &chat::Session, ext: &str) -> String {
    // The title is user-controlled and this lands inside a quoted string in
    // `Content-Disposition`, so an allowlist rather than a list of characters
    // to strip: a `"` would close the quoting early and let the rest of the
    // title be read as header parameters, and a newline would split the
    // header outright. (The page this replaced ran the title through
    // `slugify`; the port swapped in a two-character `replace`.)
    let stem: String = session
        .title
        .as_deref()
        .map(super::first_message_title)
        .unwrap_or_else(|| session.id.clone())
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let stem = stem.trim_matches('-');
    if stem.is_empty() {
        return format!("{}.{ext}", session.id);
    }
    format!("{stem}.{ext}")
}

/// A file download response (`Content-Disposition: attachment`).
fn download(content_type: &str, filename: &str, body: Vec<u8>) -> Response {
    use rama::http::header;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{filename}\""),
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
    let (_session, user) = require_session_json!(state, req);
    if !user_owns(&state, &user.id, &session_id).await {
        return not_found_conversation();
    }
    let tools = capability_views(&state, &user, &session_id).await;
    ok_json(StatusCode::OK, serde_json::json!({ "tools": tools }))
}

#[derive(serde::Serialize)]
struct CapabilityView {
    key: String,
    kind: &'static str,
    title: String,
    description: String,
    group: String,
    order: u8,
    state: &'static str,
    can_disable: bool,
    icon: Option<String>,
}

fn tool_state(value: Option<&bool>) -> &'static str {
    match value {
        Some(true) => "on",
        Some(false) => "off",
        None => "auto",
    }
}

async fn capability_views(state: &RamaState, user: &User, session_id: &str) -> Vec<CapabilityView> {
    use gateway_runtime::server::tools::catalog::Category;

    let states =
        gateway_core::server::db::chat_session_tools::states_for_session(&state.db, session_id)
            .await
            .unwrap_or_default();
    let mut views = crate::pages::tool_toggles::entries_for_roles(state, &user.roles)
        .into_iter()
        .filter(|entry| entry.category != Category::Integrations)
        .map(|entry| CapabilityView {
            state: tool_state(states.get(&entry.key)),
            key: entry.key,
            kind: "tool",
            title: entry.title,
            description: entry.description,
            group: entry.category.label().to_string(),
            order: entry.category.order(),
            can_disable: true,
            icon: None,
        })
        .collect::<Vec<_>>();

    let role_ids = state.role_ids_for(&user.roles);
    let admin = state.rbac.is_admin(&role_ids);
    let connected = gateway_core::server::db::user_mcp::connected_keys(&state.db, &user.id)
        .await
        .unwrap_or_default();
    for connector_key in connected {
        let Ok(Some(connector)) =
            gateway_core::server::db::mcp_catalog::get(&state.db, &connector_key).await
        else {
            continue;
        };
        if !connector.enabled || !connector.allows(&role_ids, admin) {
            continue;
        }
        let key = format!(
            "{}{connector_key}",
            gateway_runtime::server::tools::mcp::MCP_ID_PREFIX
        );
        views.push(CapabilityView {
            state: tool_state(states.get(&key)),
            key,
            kind: "tool",
            title: connector.name,
            description: connector.description.unwrap_or_default(),
            group: Category::Integrations.label().to_string(),
            order: Category::Integrations.order(),
            can_disable: true,
            icon: connector.icon,
        });
    }

    let loaded =
        gateway_core::server::db::chat_session_skills::loaded_for_session(&state.db, session_id)
            .await
            .unwrap_or_default();
    let registry = state.combined_skills_for(&user.id);
    for name in state.allowed_skills_for(&user.roles, &user.id) {
        let (title, description) = registry
            .as_ref()
            .and_then(|skills| skills.get(&name))
            .map(|skill| (skill.title.clone(), skill.description.clone()))
            .unwrap_or_else(|| (name.clone(), String::new()));
        views.push(CapabilityView {
            state: if loaded.contains(&name) { "on" } else { "auto" },
            key: name,
            kind: "skill",
            title,
            description,
            group: "Skills".to_string(),
            order: u8::MAX,
            can_disable: false,
            icon: None,
        });
    }
    views.sort_by(|left, right| {
        left.order
            .cmp(&right.order)
            .then_with(|| left.title.cmp(&right.title))
    });
    views
}

#[derive(serde::Deserialize)]
pub struct CapabilityBody {
    pub kind: String,
    pub key: String,
    pub state: String,
}

/// POST /api/v0/chat/sessions/{id}/capabilities — set one tool's overlay
/// state for this conversation.
pub async fn capabilities_set(
    Path(session_id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = require_session_json!(state, req);
    if !user_owns(&state, &user.id, &session_id).await {
        return not_found_conversation();
    }
    let (_, body) = req.into_parts();
    let parsed: CapabilityBody = match read_json(body, "the capability body").await {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    let available = capability_views(&state, &user, &session_id).await;
    let Some(capability) = available
        .iter()
        .find(|entry| entry.kind == parsed.kind && entry.key == parsed.key)
    else {
        return json_error(
            StatusCode::NOT_FOUND,
            "not_found",
            "that capability is not available to this user",
        );
    };
    if parsed.kind == "skill" {
        let result = match parsed.state.as_str() {
            "on" => {
                gateway_core::server::db::chat_session_skills::record(
                    &state.db,
                    &session_id,
                    &parsed.key,
                )
                .await
            }
            "auto" => {
                gateway_core::server::db::chat_session_skills::remove(
                    &state.db,
                    &session_id,
                    &parsed.key,
                )
                .await
            }
            _ => {
                return json_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "a skill state must be `on` or `auto`",
                );
            }
        };
        return match result {
            Ok(()) => ok_json(
                StatusCode::OK,
                json!({ "kind": parsed.kind, "key": parsed.key, "state": parsed.state }),
            ),
            Err(err) => internal(err),
        };
    }
    debug_assert!(capability.can_disable);
    let result = match parsed.state.as_str() {
        "on" => {
            gateway_core::server::db::chat_session_tools::set(
                &state.db,
                &session_id,
                &parsed.key,
                true,
                "manual",
            )
            .await
        }
        "off" => {
            gateway_core::server::db::chat_session_tools::set(
                &state.db,
                &session_id,
                &parsed.key,
                false,
                "manual",
            )
            .await
        }
        "auto" => {
            gateway_core::server::db::chat_session_tools::clear(&state.db, &session_id, &parsed.key)
                .await
        }
        other => {
            return json_error(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                &format!("`state` must be `on`, `auto` or `off` (got `{other}`)"),
            );
        }
    };
    match result {
        Ok(()) => ok_json(
            StatusCode::OK,
            serde_json::json!({ "kind": parsed.kind, "key": parsed.key, "state": parsed.state }),
        ),
        Err(err) => internal(err),
    }
}

// ---------------------------------------------------------------------------
// Token self-service: model allowlist + quota (owner-side, like /tokens)

#[derive(serde::Deserialize)]
pub struct OwnerModelsBody {
    pub restrict: bool,
    #[serde(default)]
    pub models: Vec<String>,
}

/// PUT /api/v0/chat/sessions/… — no. This is the owner-side token models
/// allowlist (mirrors the legacy /tokens/{id}/models form with
/// ManagedBy::Owner).
pub async fn owner_token_models(
    Path(token_id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    use gateway_core::server::db::limits;
    use gateway_core::server::db::token_models;
    use gateway_core::server::db::tokens;

    let (_session, user) = require_session_json!(state, req);
    // Owner-only: the token must exist AND belong to the caller. Someone
    // else's token is reported the same as a missing one — no probing for
    // live token ids across accounts.
    match tokens::find_by_id(&state.db, &token_id).await {
        Ok(Some(t)) if t.user_id == user.id => {}
        _ => return json_error(StatusCode::NOT_FOUND, "not_found", "no such token"),
    }
    let (_, body) = req.into_parts();
    let parsed: OwnerModelsBody = match read_json(body, "the models body").await {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    if parsed.restrict && parsed.models.is_empty() {
        return bad_request("restricting to an empty list would block every model");
    }
    let to_store: Vec<String> = if parsed.restrict {
        parsed.models
    } else {
        Vec::new()
    };
    match token_models::set_for_token(&state.db, &token_id, &to_store, limits::ManagedBy::Owner)
        .await
    {
        Ok(()) => ok_json(StatusCode::OK, serde_json::json!({ "models": to_store })),
        Err(err) => internal(err),
    }
}

#[derive(serde::Deserialize)]
pub struct OwnerQuotaBody {
    pub dimension: String,
    pub window: String,
    pub value: f64,
}

/// POST /api/v0/tokens/{id}/quota — add an owner-set quota to one token
/// (narrowing only: the owner's own budget still applies).
pub async fn owner_token_quota(
    Path(token_id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    use gateway_core::server::db::limits::{self, Dimension, ManagedBy, SubjectType, Window};

    let (_session, user) = require_session_json!(state, req);
    // Owner-only: another user's token reads as missing — no cross-account
    // probing.
    let is_owner = matches!(
        gateway_core::server::db::tokens::find_by_id(&state.db, &token_id).await,
        Ok(Some(t)) if t.user_id == user.id
    );
    if !is_owner {
        return json_error(StatusCode::NOT_FOUND, "not_found", "no such token");
    }
    let (_, body) = req.into_parts();
    let parsed: OwnerQuotaBody = match read_json(body, "the quota body").await {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    let Some(dimension) = Dimension::parse(&parsed.dimension) else {
        return bad_request("unknown dimension");
    };
    let Some(window) = Window::parse(&parsed.window) else {
        return json_error(StatusCode::BAD_REQUEST, "invalid_request", "unknown window");
    };
    if !parsed.value.is_finite() || parsed.value < 0.0 {
        return bad_request("value must be ≥ 0");
    }
    match limits::upsert_checked(
        &state.db,
        SubjectType::Token,
        &token_id,
        None,
        dimension,
        window,
        parsed.value,
        ManagedBy::Owner,
    )
    .await
    {
        Ok(_) => ok_json(StatusCode::OK, serde_json::json!({ "ok": true })),
        Err(err) => internal(err),
    }
}

/// DELETE /api/v0/tokens/{id}/quota/{rule_id} — drop one of the owner's own
/// quota rules.
///
/// Scoped to owner-managed rules by `delete_owner_rule`: the rule id comes
/// from the client, so a plain delete-by-id would let an owner remove an
/// operator's cap (or a global rule) by naming it. An admin-managed rule
/// therefore reads as "not found" here, exactly as the form handler behaved.
pub async fn owner_token_quota_delete(
    Path(TokenRulePath {
        id: token_id,
        rule_id,
    }): Path<TokenRulePath>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    use gateway_core::server::db::limits;

    let (_session, user) = require_session_json!(state, req);
    match gateway_core::server::db::tokens::find_by_id(&state.db, &token_id).await {
        Ok(Some(t)) if t.user_id == user.id => {}
        Ok(_) => return json_error(StatusCode::NOT_FOUND, "not_found", "no such token"),
        Err(err) => {
            return internal(err);
        }
    }
    match limits::delete_owner_rule(&state.db, &token_id, &rule_id).await {
        Ok(true) => ok_json(StatusCode::OK, serde_json::json!({ "deleted": true })),
        Ok(false) => not_found("no such quota rule on this token"),
        Err(err) => internal(err),
    }
}

#[derive(serde::Deserialize)]
pub struct TokenRulePath {
    pub id: String,
    pub rule_id: String,
}

#[derive(serde::Deserialize)]
pub struct McpPolicyBody {
    /// `true` treats an `ask` connector as `always` for calls made with this
    /// token; `false` blocks them with a "needs approval" tool error.
    pub allow: bool,
}

/// PUT /api/v0/tokens/{id}/mcp-policy — decide what an `ask`-level MCP
/// connector does when the caller is a bearer token rather than a person.
///
/// A token has nobody to prompt, so the default is to block; allowing it is
/// the owner's explicit "run these unattended" for their own token. Stored
/// against `*` — the whole connector set — mirroring the legacy toggle.
pub async fn owner_token_mcp_policy(
    Path(token_id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    use gateway_core::server::db::user_mcp::{AskOverApi, set_token_policy};

    let (_session, user) = require_session_json!(state, req);
    // Owner-only, and a foreign token reads as missing (no probing for live
    // ids across accounts).
    match gateway_core::server::db::tokens::find_by_id(&state.db, &token_id).await {
        Ok(Some(t)) if t.user_id == user.id => {}
        Ok(_) => return json_error(StatusCode::NOT_FOUND, "not_found", "no such token"),
        Err(err) => {
            return internal(err);
        }
    }
    let (_, body) = req.into_parts();
    let parsed: McpPolicyBody = match read_json(body, "the policy body").await {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    let policy = if parsed.allow {
        AskOverApi::Allow
    } else {
        AskOverApi::Block
    };
    match set_token_policy(&state.db, &token_id, "*", policy).await {
        Ok(()) => ok_json(StatusCode::OK, serde_json::json!({ "allow": parsed.allow })),
        Err(err) => {
            tracing::warn!(error = %err, %token_id, "set token mcp policy");
            internal(err)
        }
    }
}
