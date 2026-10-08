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
//! * `POST /api/v0/chat/sessions/{id}/cancel` — stop the live turn, or give
//!   up on a suspended one
//! * `POST /api/v0/chat/sessions/{id}/turns/{turn_id}/resume` — answer a
//!   suspended turn's decision and continue it
//! * `GET  /api/v0/chat/sessions/{id}/events` — the SSE event stream

use std::sync::Arc;

use rama::http::service::web::extract::{Path, State};
use rama::http::{Request, Response, StatusCode};
use serde::Deserialize;

use aiplane_features::server::chat_attachments;
use aiplane_runtime::rama_server::state::RamaState;

use aiplane_core::server::db::users::User;

use super::{
    ChatSubmit, DocumentPath, RequestCtx, SteerPath, SubmitOutcome, SubmitTurnError, TurnPath,
    submit_turn,
};
use crate::pages::{bad_request, internal, json_error, json_ok as ok_json, not_found, read_json};
use session_core::db as chat;
use session_core::i18n::{Lang, t};

/// The request facts a turn needs, read off the request while it is intact.
///
/// Must be called before `req.into_parts()`: the socket peer lives in the
/// request's extensions, not its headers. `client_ip` is the sole input to the GeoIP
/// path in `get_user_location` — the fallback for when the browser declines
/// to share a precise position — so a hardcoded `None` leaves that tool
/// waiting out its timeout and then failing. `transport_is_secure` reads the
/// forwarded-proto header rather than just the configured public URL, so a
/// gateway behind a TLS-terminating proxy is not reported as plaintext.
pub(crate) fn request_ctx(state: &RamaState, req: &Request, voice_mode: bool) -> RequestCtx {
    RequestCtx {
        client_ip: state.client_ip(req),
        secure: aiplane_features::server::geoip::transport_is_secure(
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
        let sessions = hits
            .into_iter()
            .map(|h| SessionMatch {
                id: h.session_id,
                title: h.title,
                updated_at: h.updated_at.to_string(),
                pinned: h.pinned,
                snippet: h.snippet,
            })
            .collect();
        return ok_json(StatusCode::OK, SessionsList::Found { sessions });
    }
    match chat::list_sessions(&state.db, &user.id).await {
        Ok(sessions) => ok_json(StatusCode::OK, SessionsList::Listed { sessions }),
        Err(err) => internal(err),
    }
}

#[derive(serde::Deserialize, Default, schemars::JsonSchema)]
pub struct SessionsQuery {
    /// Search the caller's conversations for this text instead of listing them.
    q: Option<String>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum SessionsList {
    /// Without `q`: every conversation, pinned first, then most recent.
    Listed { sessions: Vec<chat::Session> },
    /// With `q`: title matches first, then content matches.
    Found { sessions: Vec<SessionMatch> },
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct SessionMatch {
    pub id: String,
    pub title: Option<String>,
    pub updated_at: String,
    pub pinned: bool,
    /// An HTML-escaped excerpt of the match, the match itself wrapped in `<b>`.
    pub snippet: String,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct SessionEnvelope {
    pub session: chat::Session,
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
    ok_json(StatusCode::OK, SessionEnvelope { session })
}

/// POST /api/v0/chat/sessions — mint an empty conversation.
pub async fn session_create(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, user) = require_session_json!(state, req);
    match chat::create_session(&state.db, &user.id).await {
        Ok(session) => ok_json(StatusCode::CREATED, SessionEnvelope { session }),
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
    let effort =
        match aiplane_core::server::db::chat_session_settings::get_effort(&state.db, &session_id)
            .await
        {
            Ok(stored) => {
                aiplane_core::server::reasoning::Effort::from_db(stored.as_deref()).as_str()
            }
            Err(err) => return internal(err),
        };
    let compacted_up_to_seq =
        match aiplane_core::server::db::chat_compactions::get(&state.db, &session_id).await {
            Ok(compaction) => compaction.map(|value| value.up_to_seq),
            Err(err) => return internal(err),
        };
    let assets = match aiplane_features::server::chat_attachments::list_session_attachments(
        &state.db,
        &session_id,
    )
    .await
    {
        Ok(assets) => assets
            .into_iter()
            .map(|asset| SessionAsset {
                url: aiplane_features::server::chat_attachments::proxy_url(
                    &asset.turn_id,
                    &asset.filename,
                ),
                id: asset.id,
                turn_id: asset.turn_id,
                filename: asset.filename,
                mime: asset.mime,
                size: asset.size,
            })
            .collect(),
        Err(err) => return internal(err),
    };
    ok_json(
        StatusCode::OK,
        SessionDetail {
            session,
            turns,
            effort,
            compacted_up_to_seq,
            assets,
        },
    )
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct SessionDetail {
    pub session: chat::Session,
    pub turns: Vec<chat::TurnWithTools>,
    /// The conversation's effort level — the stored one, or the default
    /// (`low`) when none was chosen.
    pub effort: &'static str,
    /// The highest turn `seq` a compaction summary replaces when the
    /// conversation is replayed to the model; `null` when nothing was compacted.
    pub compacted_up_to_seq: Option<i64>,
    /// Every file attached to or produced in the conversation.
    pub assets: Vec<SessionAsset>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct SessionAsset {
    /// `<turn_id>/<filename>`.
    pub id: String,
    pub turn_id: String,
    pub filename: String,
    pub mime: String,
    /// Bytes.
    pub size: u64,
    /// Where the browser downloads the file.
    pub url: String,
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
    // Deleting the conversation deletes its files.
    let config = state.config();
    let deleted = match chat_attachments::delete_reclaiming(
        &state.db,
        config.chat.s3.as_ref(),
        &[chat_attachments::Doomed::session(&session_id)],
        chat::delete_session(&state.db, &user.id, &session_id),
    )
    .await
    {
        Ok(reclaim) => reclaim,
        Err(err) => {
            return internal(err);
        }
    };
    if let Some(reclaim) = deleted {
        reclaim.in_background();
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
        Ok(true) => ok_json(
            StatusCode::OK,
            Pinned {
                pinned: parsed.pinned,
            },
        ),
        Ok(false) => not_found_conversation(),
        Err(err) => internal(err),
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct PinBody {
    pinned: bool,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct Pinned {
    pub pinned: bool,
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
            if let Err(err) = aiplane_features::server::chat_attachments::copy_object(
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
        Forked {
            id: new_session.id,
            title: new_session.title,
        },
    )
}

/// The caller's new copy of the conversation.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct Forked {
    pub id: String,
    pub title: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct MessageBody {
    /// The model to answer with, as `GET /api/v0/models` lists it.
    model: String,
    message: String,
    /// The message was dictated; the answer is read aloud.
    #[serde(default)]
    voice: bool,
}

/// Where a submitted message went. Only the server knows whether a worker is
/// running, so it says.
#[derive(serde::Serialize, schemars::JsonSchema)]
#[serde(tag = "placement", rename_all = "snake_case")]
pub enum Submitted {
    /// A new turn is running; its reply arrives on `GET …/events`.
    Started {
        user_turn_id: String,
        assistant_turn_id: String,
    },
    /// A turn was already running, so the message was added to its prompt.
    Folded {
        assistant_turn_id: String,
        steer_id: String,
    },
    /// Every parallel slot is in use; the message starts when one frees up.
    Queued { user_turn_id: String },
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
    // 404 path, so `chat_attachments::delete_reclaiming` can never find
    // those objects to sweep them. The quota enforcer runs later still.
    let active = match chat::get_session(&state.db, &user.id, &session_id).await {
        Ok(Some(s)) => s,
        Ok(None) => return not_found_conversation(),
        Err(err) => {
            return internal(err);
        }
    };

    // Before the body too: a refused message must not upload attachments.
    if let Err(err) = super::refuse_while_paused(&state, &active.id).await {
        return submit_refusal(err);
    }

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
        }
    };

    let has_attachments = !submit.attachments.is_empty();
    if submit.user_text.is_empty() && !has_attachments {
        return bad_request("message must not be empty");
    }

    ctx.voice_mode = submit.voice;
    match submit_turn(&state, &user, &active, submit, ctx).await {
        // `placement` is the honest half of the answer. The client no longer
        // decides where a message goes — only the server knows whether a
        // worker is running — so it has to be told: `started` renders as a
        // turn, `folded` as an addition to the answer being written, `queued`
        // as a message waiting for a slot.
        Ok(SubmitOutcome::Started(submitted)) => ok_json(
            StatusCode::ACCEPTED,
            Submitted::Started {
                user_turn_id: submitted.user_turn.id,
                assistant_turn_id: submitted.assistant_turn.id,
            },
        ),
        Ok(SubmitOutcome::Folded {
            assistant_turn_id,
            steer_id,
        }) => ok_json(
            StatusCode::ACCEPTED,
            Submitted::Folded {
                assistant_turn_id,
                steer_id,
            },
        ),
        Ok(SubmitOutcome::Queued { user_turn_id }) => {
            ok_json(StatusCode::ACCEPTED, Submitted::Queued { user_turn_id })
        }
        Err(err) => submit_refusal(err),
    }
}

/// The one place a refused submit becomes a response.
///
/// Short, now: a busy conversation and a full set of parallel slots are no
/// longer refusals — a message sent into either is accepted and answered
/// later. What is left is the caller having no budget, and storage failing.
fn submit_refusal(err: SubmitTurnError) -> Response {
    match err {
        SubmitTurnError::RateLimited(exceeded) => over_budget(&exceeded),
        SubmitTurnError::DecisionPending(message) => {
            json_error(StatusCode::CONFLICT, "decision_pending", &message)
        }
        SubmitTurnError::Db(msg) => {
            json_error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", &msg)
        }
    }
}

/// What a signed-in user over a limit is told, whether they sent a message or
/// spoke to the chat's voice input or read-aloud: the refusal, and in
/// `Retry-After` how long until the breached window moves on.
pub fn over_budget(exceeded: &aiplane_core::server::limits::LimitExceeded) -> Response {
    let mut response = json_error(
        StatusCode::TOO_MANY_REQUESTS,
        "rate_limited",
        "rate limit or quota exceeded — see /usage",
    );
    response.headers_mut().insert(
        rama::http::header::RETRY_AFTER,
        rama::http::HeaderValue::from(exceeded.retry_after_secs.max(0)),
    );
    response
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct SteerBody {
    /// At most 4 KiB.
    message: String,
}

/// The recorded interjection. The model has not necessarily seen it yet.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct SteerAccepted {
    pub id: String,
    pub turn_id: String,
    pub status: &'static str,
}

/// Ceiling on what may be folded into a running turn, in bytes.
///
/// Generous for a sentence or two of correction, which is what an addition is
/// for, and far below anything that meaningfully grows the prompt — which it
/// does twice over, since a delivered note also replays in the history of
/// every later turn.
///
/// Anything longer is not refused: it is a message, and it waits its turn like
/// one. Only the *folding* is bounded.
pub(crate) const MAX_STEER_BYTES: usize = 4 * 1024;

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

    // Same gate a message goes through, on the running turn's model, and the
    // same response. An interjection is not free: it is a row, and it is one
    // more user message folded into every remaining round of the prompt, so an
    // unbounded stream of them inflates the upstream request at nobody's
    // expense but the operator's.
    let model = match chat::get_turn(&state.db, &session_id, &worker.turn_id).await {
        Ok(turn) => turn.and_then(|turn| turn.model).unwrap_or_default(),
        Err(err) => return internal(err),
    };
    if let Some(exceeded) = state
        .limit_exceeded(
            &user.id,
            &state.role_ids_for(&user.roles),
            None,
            &model,
            state
                .upstreams
                .enforce_limits_for_model(&model, aiplane_core::server::upstreams::PoolKind::Chat),
        )
        .await
    {
        return submit_refusal(SubmitTurnError::RateLimited(exceeded));
    }

    // The insert is conditional on the turn still running, so the answer
    // finishing between the lookup above and this write is a `None` rather
    // than a row against a finished turn.
    let steer = match chat::insert_steer(&state.db, &worker.turn_id, &text).await {
        Ok(Some(steer)) => steer,
        Ok(None) => {
            return json_error(
                StatusCode::CONFLICT,
                "no_turn_running",
                "the answer finished before this reached it — send it as a message instead",
            );
        }
        Err(err) => return internal(err),
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
        SteerAccepted {
            id: steer.id,
            turn_id: steer.turn_id,
            status: steer.status.as_str(),
        },
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
    // Take it out of the live worker's inbox *first*, and let that decide.
    //
    // A note is only discardable while it is still waiting to be folded in.
    // Once the driver has drained it into a prompt the row is still `pending`
    // for a moment — delivery is recorded after the upstream accepts the round
    // — and settling it here would put "discarded" in the transcript for a
    // sentence the model is reading right now.
    if let Some(worker) = state.chats.get(&user.id, &session_id)
        && worker.turn_id == steer.turn_id
        && !worker.steers.remove(&steer.id)
        && steer.status == chat::SteerStatus::Pending
    {
        return json_error(
            StatusCode::CONFLICT,
            "already_in_flight",
            "this addition is already on its way to the model",
        );
    }
    match chat::settle_steer(&state.db, &steer.id, chat::SteerStatus::Discarded).await {
        // Already settled: delivered, re-sent, or discarded in another tab.
        // Nothing to do and nothing to complain about — the caller wanted it
        // gone and it is.
        Ok(_) => ok_json(StatusCode::OK, SteerDiscarded { id: steer.id }),
        Err(err) => internal(err),
    }
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct SteerDiscarded {
    pub id: String,
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
    let mut cancelled = session_core::chat_json::cancel_turn(&state.chats, &user.id, &session_id);
    // No worker, but a turn paused for a decision is the conversation's live
    // turn all the same: stopping it is giving up on the decision.
    if !cancelled {
        cancelled = match chat::suspended_turn_in_session(&state.db, &session_id).await {
            Ok(Some(turn_id)) => match chat::cancel_suspended_turn(&state.db, &turn_id).await {
                Ok(done) => done,
                Err(err) => return internal(err),
            },
            Ok(None) => false,
            Err(err) => return internal(err),
        };
        if cancelled {
            let state = state.clone();
            let user_id = user.id.clone();
            tokio::spawn(async move {
                super::start_pending_turns(&state, &user_id).await;
            });
        }
    }
    ok_json(StatusCode::OK, Cancelled { cancelled })
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct Cancelled {
    /// `false` when nothing was running or waiting for a decision.
    pub cancelled: bool,
}

/// What a resume carries: the decision's shape, its value when it has one,
/// and optionally the request it answers.
#[derive(Deserialize, schemars::JsonSchema)]
pub struct ResumeBody {
    pub decision: chat::DecisionKind,
    #[serde(default)]
    pub value: Option<serde_json::Value>,
    /// The `request_id` of the `suspended` event being answered. Optional;
    /// when present, an answer to an older pause of the same turn is refused.
    #[serde(default)]
    pub request_id: Option<String>,
}

impl ResumeBody {
    pub(crate) fn decision(self) -> Result<chat::Decision, &'static str> {
        decision_from(self.decision, self.value)
    }
}

/// The decision a resume body names: its shape, and the value only a `value`
/// decision carries.
pub(crate) fn decision_from(
    kind: chat::DecisionKind,
    value: Option<serde_json::Value>,
) -> Result<chat::Decision, &'static str> {
    match (kind, value) {
        (chat::DecisionKind::AllowOnce, None) => Ok(chat::Decision::AllowOnce),
        (chat::DecisionKind::Deny, None) => Ok(chat::Decision::Deny {
            reason: chat::DenyReason::User,
        }),
        (chat::DecisionKind::Value, Some(value)) => Ok(chat::Decision::Value { value }),
        (chat::DecisionKind::Value, None) => {
            Err("a `value` decision needs a `value` field carrying it")
        }
        (_, Some(_)) => Err("only a `value` decision carries a `value` field"),
    }
}

/// POST /api/v0/chat/sessions/{id}/turns/{turn_id}/resume — answer the
/// decision a suspended turn waits for, and continue it.
///
/// Owner-only. `202` once the turn's worker is running again; the rest of
/// the turn arrives on `GET …/events` like any other. `409` when the turn is
/// not waiting (never paused, or already answered), when the answer is for an
/// older pause, or when the conversation has no free slot right now.
pub async fn turn_resume(
    Path(TurnPath {
        id: session_id,
        turn_id,
    }): Path<TurnPath>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    use aiplane_runtime::suspend::ResumeRefused;
    let (_session, user) = require_session_json!(state, req);
    let ctx = request_ctx(&state, &req, false);
    let turn = match load_owned_turn(&state, &user, &session_id, &turn_id).await {
        Ok(turn) => turn,
        Err(resp) => return resp,
    };
    let (_, body) = req.into_parts();
    let body: ResumeBody = match read_json(body, "the resume body").await {
        Ok(body) => body,
        Err(resp) => return resp,
    };
    let request_id = body.request_id.clone();
    let decision = match body.decision() {
        Ok(decision) => decision,
        Err(msg) => return bad_request(msg),
    };
    match super::resume_turn(&state, &user, &turn, request_id.as_deref(), decision, ctx).await {
        Ok(()) => ok_json(
            StatusCode::ACCEPTED,
            Resumed {
                assistant_turn_id: turn.id,
            },
        ),
        Err(super::ResumeTurnError::Busy) => json_error(
            StatusCode::CONFLICT,
            "turn_in_progress",
            "another turn is running in this conversation or every parallel slot is in use —              answer again once it has finished",
        ),
        Err(super::ResumeTurnError::Refused(refused)) => match refused {
            ResumeRefused::NotSuspended | ResumeRefused::StaleRequest { .. } => {
                json_error(StatusCode::CONFLICT, "not_suspended", &refused.to_string())
            }
            ResumeRefused::NotOffered { .. } => json_error(
                StatusCode::BAD_REQUEST,
                "decision_not_offered",
                &refused.to_string(),
            ),
            ResumeRefused::Storage(err) => internal(err),
        },
    }
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct Resumed {
    pub assistant_turn_id: String,
}

/// The conversation's waiting user turns, for a snapshot.
///
/// An unreadable queue degrades to "nothing waiting": the transcript is still
/// right, one message just renders without its spinner until the next
/// snapshot.
async fn waiting_turn_ids(state: &Arc<RamaState>, session_id: &str) -> Vec<String> {
    chat::list_pending_for_session(&state.db, session_id)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|pending| pending.turn_id)
        .collect()
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
    let owns_session = match readable_session(&state, &user.id, &session_id).await {
        Ok(Some(session)) => session.user_id == user.id,
        Ok(None) => return not_found_conversation(),
        Err(resp) => return resp,
    };

    // Subscribe to worker starts BEFORE looking for one. The two steps have a
    // gap, and the worker this viewer is waiting for may be registered inside
    // it — which is exactly the case that matters here, because a message
    // waiting for a free slot starts on the scheduler's schedule, not on any
    // request.
    let starts = state.chats.subscribe_starts();
    // Owners attach to their own live worker; a shared-session viewer finds
    // none (workers are keyed by owner) and reads the static snapshot —
    // same behaviour as the legacy tail.
    let mut live = state.chats.get(&user.id, &session_id);
    let mut prior_turns = Vec::new();
    if live.is_none() {
        prior_turns = match chat::list_turns(&state.db, &session_id).await {
            Ok(turns) => turns,
            Err(err) => return internal(err),
        };
        live = state.chats.get(&user.id, &session_id);
    }

    match live {
        Some(worker) => live_stream(&state, &worker, session_id).await,
        None => {
            let orphan_ids: Vec<_> = prior_turns
                .iter()
                .filter(|turn| owns_session && turn.turn.status == chat::TurnStatus::InProgress)
                .map(|turn| turn.turn.id.clone())
                .collect();
            let error_message = t(Lang::En, "chat-error-turn-interrupted");
            for turn_id in &orphan_ids {
                if let Err(err) =
                    chat::error_interrupted_turn(&state.db, turn_id, &error_message).await
                {
                    return internal(err);
                }
            }
            let turns = if orphan_ids.is_empty() {
                prior_turns
            } else {
                match chat::list_turns(&state.db, &session_id).await {
                    Ok(turns) => turns,
                    Err(err) => return internal(err),
                }
            };
            // Is anything here sent but not started? Ask the work queue, not
            // the transcript: "ends on a user turn" *looks* like the same
            // question and is not. A turn whose assistant row failed to insert
            // leaves that shape with nothing queued, and would pin a task and
            // a connection for the whole wait on every attach, forever.
            //
            // Only for the owner. Workers are keyed by owner, so a viewer of a
            // shared conversation could never match one — they would wait out
            // the full deadline and then be told `idle`.
            let waiting = owns_session
                && !chat::list_pending_for_session(&state.db, &session_id)
                    .await
                    .unwrap_or_default()
                    .is_empty();
            if !waiting {
                return quiet_stream(turns);
            }
            // Hold the stream open until the scheduler starts it. Ending at
            // `idle` here would leave the page showing "waiting" long after
            // the answer began, with nothing left to tell it otherwise — and
            // a client that asks again every few seconds is a poll standing in
            // for an event the server already has.
            let (tx, rx) = rama::futures::channel::mpsc::unbounded::<
                Result<rama::bytes::Bytes, std::io::Error>,
            >();
            tokio::spawn(session_core::chat_json::stream_until_started(
                state.db.clone(),
                state.chats.clone(),
                user.id.clone(),
                session_id,
                turns,
                starts,
                tx,
            ));
            session_core::chat_json::json_stream_response(rx)
        }
    }
}

/// Tail `worker`'s turn of conversation `session_id`: a snapshot, then the
/// turn's frames until it ends. Shared by every stream of a conversation that
/// has a worker, whoever owns it.
pub(crate) async fn live_stream(
    state: &Arc<RamaState>,
    worker: &session_core::workers::ActiveWorker,
    session_id: String,
) -> Response {
    // Subscribe BEFORE reading the snapshot: anything the worker commits
    // after this point arrives as a tick; anything before is in the
    // snapshot. No gap.
    let broadcast_rx = worker.broadcast.subscribe();
    let turns = match chat::list_turns(&state.db, &session_id).await {
        Ok(t) => t,
        Err(err) => return internal(err),
    };
    let (tx, rx) =
        rama::futures::channel::mpsc::unbounded::<Result<rama::bytes::Bytes, std::io::Error>>();
    let waiting_turn_ids = waiting_turn_ids(state, &session_id).await;
    let initial = vec![session_core::chat_json::ChatEvent::Snapshot {
        live_turn_id: Some(worker.turn_id.clone()),
        turns,
        waiting_turn_ids,
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

/// A conversation nothing is running in: its snapshot, then `idle`.
pub(crate) fn quiet_stream(turns: Vec<chat::TurnWithTools>) -> Response {
    session_core::chrome::sse_response(&[
        session_core::chat_json::sse_json(&session_core::chat_json::ChatEvent::Snapshot {
            live_turn_id: None,
            turns,
            waiting_turn_ids: Vec::new(),
        }),
        session_core::chat_json::sse_json(&session_core::chat_json::ChatEvent::Idle),
    ])
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
// Turn actions (retry/edit/share/fork/effort/export).

/// Named (not positional) path extraction: rama's tuple `Path` reads
/// captures in a non-deterministic order, so any route with two or more
/// params must map by name — the same reason the legacy handlers use
/// `TurnPath`.
#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct RetryBody {
    pub model: String,
}

/// DELETE /api/v0/chat/sessions/{id}/turns/{turn_id} — take back a message
/// that has not been answered yet.
///
/// Only a user turn with nothing after it: once an answer exists (or is being
/// written), removing the question alone would leave a reply to a message
/// nobody can read. Editing and retrying, which rewrite history deliberately,
/// have their own endpoints.
pub async fn turn_delete(
    Path(TurnPath {
        id: session_id,
        turn_id,
    }): Path<TurnPath>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = require_session_json!(state, req);
    if !user_owns(&state, &user.id, &session_id).await {
        return not_found_conversation();
    }
    let turns = match chat::list_turns(&state.db, &session_id).await {
        Ok(turns) => turns,
        Err(err) => return internal(err),
    };
    let Some(index) = turns.iter().position(|t| t.turn.id == turn_id) else {
        return not_found_conversation();
    };
    let target = &turns[index];
    if target.turn.role != chat::TurnRole::User {
        return bad_request("only a message can be taken back, not an answer");
    }
    if turns.len() > index + 1 {
        return json_error(
            StatusCode::CONFLICT,
            "already_answered",
            "this message is already being answered — cancel the turn instead",
        );
    }
    // Claim it out of the work queue, and treat losing that claim as "too
    // late". Reading the transcript and then deleting leaves a window in which
    // the scheduler starts this very turn: the delete would take the freshly
    // created assistant row with it, and its worker would go on streaming into
    // a row that no longer exists. This is the same one-shot claim the
    // scheduler uses, so exactly one of the two wins.
    match chat::delete_pending_turn(&state.db, &turn_id).await {
        Ok(true) => {}
        Ok(false) => {
            return json_error(
                StatusCode::CONFLICT,
                "already_answered",
                "this message just started being answered — cancel the turn instead",
            );
        }
        Err(err) => return internal(err),
    }
    match super::truncate_reclaiming(&state, &session_id, target.turn.seq).await {
        Ok(()) => {
            let _ = chat::touch_session(&state.db, &session_id).await;
            ok_json(StatusCode::OK, TurnDeleted { deleted: turn_id })
        }
        Err(err) => internal(err),
    }
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct TurnDeleted {
    /// The id of the message taken back.
    pub deleted: String,
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
    if let Err(err) = super::truncate_reclaiming(&state, &session_id, turn.seq).await {
        return internal(err);
    }
    match start_regeneration_json(&state, &user, &session_id, parsed.model, ctx).await {
        Ok(ids) => ok_json(StatusCode::ACCEPTED, ids),
        Err(resp) => resp,
    }
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
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
    if let Err(err) = super::truncate_reclaiming(&state, &session_id, turn.seq + 1).await {
        return internal(err);
    }
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
) -> Result<Regenerated, Response> {
    let assistant_turn_id = uuid::Uuid::new_v4().to_string();
    let worker = match state.chats.register(
        &user.id,
        &assistant_turn_id,
        session_id,
        state.config().chat.turns.max_parallel,
    ) {
        session_core::RegisterOutcome::Registered { worker } => worker,
        // Regeneration is not a message: a retry or an edit rewrites history
        // that a running turn is reading, so there is nothing sensible to
        // queue. These stay refusals.
        session_core::RegisterOutcome::Busy { .. } => {
            return Err(json_error(
                StatusCode::CONFLICT,
                "turn_in_progress",
                "this conversation is still streaming a turn — cancel it first",
            ));
        }
        session_core::RegisterOutcome::AtCapacity { running, limit } => {
            return Err(json_error(
                StatusCode::CONFLICT,
                "at_capacity",
                &format!(
                    "{running} of {limit} parallel conversations are already \
                     streaming for this user"
                ),
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
        None,
    )
    .await;
    Ok(Regenerated {
        user_turn_id: (),
        assistant_turn_id: assistant_turn.id,
    })
}

/// The answer being regenerated; it streams on `GET …/events`.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct Regenerated {
    /// Always `null`: a regeneration adds no message.
    pub user_turn_id: (),
    pub assistant_turn_id: String,
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

#[derive(serde::Deserialize, schemars::JsonSchema)]
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
            Shared {
                shared: parsed.shared,
            },
        ),
        Err(err) => internal(err),
    }
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct Shared {
    pub shared: bool,
}

#[derive(serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct EffortBody {
    /// off | low | medium | high | xhigh
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
    if aiplane_core::server::reasoning::Effort::parse(&parsed.effort).is_none() {
        return bad_request(format!(
            "unknown effort {:?}: use off, low, medium, high or xhigh",
            parsed.effort
        ));
    }
    match aiplane_core::server::db::chat_session_settings::set_effort(
        &state.db,
        &session_id,
        &parsed.effort,
    )
    .await
    {
        Ok(()) => ok_json(StatusCode::OK, parsed),
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
    match aiplane_features::server::typst::compile_source(&source).await {
        Ok(pdf) => download("application/pdf", &export_filename(&session, "pdf"), pdf),
        Err(aiplane_features::server::typst::CompileError::BinaryNotFound) => json_error(
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
    use aiplane_core::server::db::documents;

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
            DocumentsList {
                documents: docs.iter().map(document_summary).collect(),
            },
        ),
        Err(err) => internal(err),
    }
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct DocumentsList {
    pub documents: Vec<DocumentSummary>,
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
    use aiplane_core::server::db::documents;

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
        DocumentDetail {
            document: document_summary(&doc),
            version: DocumentVersionView {
                version: ver.version,
                author: ver.author.as_str(),
                created_at: ver.created_at.to_string(),
                content: ver.content,
                summary: ver.summary,
                turn_id: ver.turn_id,
            },
            history: history
                .iter()
                .map(|v| DocumentHistoryEntry {
                    version: v.version,
                    summary: v.summary.clone(),
                    created_at: v.created_at.to_string(),
                    chars: v.chars,
                    author: v.author.as_str(),
                })
                .collect(),
        },
    )
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct DocumentDetail {
    pub document: DocumentSummary,
    /// The requested revision, or the current one.
    pub version: DocumentVersionView,
    /// Every revision, without its content.
    pub history: Vec<DocumentHistoryEntry>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct DocumentVersionView {
    pub version: i64,
    pub content: String,
    pub summary: Option<String>,
    /// The turn that wrote this revision; `null` for a hand edit.
    pub turn_id: Option<String>,
    pub author: &'static str,
    pub created_at: String,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct DocumentHistoryEntry {
    pub version: i64,
    pub summary: Option<String>,
    pub created_at: String,
    /// Length of the revision's content.
    pub chars: i64,
    pub author: &'static str,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct DocumentEditBody {
    content: String,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct DocumentSaved {
    pub document: DocumentSummary,
    /// The content equalled the current revision, so no version was added.
    pub unchanged: bool,
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
    use aiplane_core::server::db::documents;

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
        DocumentSaved {
            document: document_summary(&current),
            unchanged,
        },
    )
}

/// The wire shape of a document. Its own type rather than a `Serialize`
/// derive on the DB row: the API contract and the table are free to drift.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct DocumentSummary {
    pub id: String,
    pub title: String,
    pub format: &'static str,
    pub current_ver: i64,
    pub created_at: String,
    pub updated_at: String,
}

fn document_summary(doc: &aiplane_core::server::db::documents::Document) -> DocumentSummary {
    DocumentSummary {
        id: doc.id.clone(),
        title: doc.title.clone(),
        format: doc.format.as_str(),
        current_ver: doc.current_ver,
        created_at: doc.created_at.to_string(),
        updated_at: doc.updated_at.to_string(),
    }
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
            aiplane_features::server::chat_attachments::delete(cfg, &turn_id, &filename).await
    {
        tracing::warn!(error = %err, %turn_id, %filename, "attachment S3 delete (marker already removed)");
    }

    ok_json(StatusCode::OK, AttachmentRemoved { removed: filename })
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct AttachmentRemoved {
    /// The removed file's name.
    pub removed: String,
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
    ok_json(StatusCode::OK, Capabilities { tools })
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct Capabilities {
    /// The caller's granted tools and skills with this conversation's overlay.
    pub tools: Vec<CapabilityView>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct CapabilityView {
    key: String,
    /// `skill` or a tool kind.
    kind: &'static str,
    title: String,
    description: String,
    group: String,
    order: u8,
    /// `on`, `off` or `auto`; a skill is `on` or `auto`.
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
    let states =
        aiplane_core::server::db::chat_session_tools::states_for_session(&state.db, session_id)
            .await
            .unwrap_or_default();
    let loaded =
        aiplane_core::server::db::chat_session_skills::loaded_for_session(&state.db, session_id)
            .await
            .unwrap_or_default();
    crate::pages::tool_toggles::capabilities_for_user(state, &user.roles, &user.id)
        .await
        .into_iter()
        .map(|entry| CapabilityView {
            state: if entry.kind == "skill" {
                if loaded.contains(&entry.key) {
                    "on"
                } else {
                    "auto"
                }
            } else {
                tool_state(states.get(&entry.key))
            },
            key: entry.key,
            kind: entry.kind,
            title: entry.title,
            description: entry.description,
            group: entry.group,
            order: entry.order,
            can_disable: entry.kind != "skill",
            icon: entry.icon,
        })
        .collect()
}

/// Also the response: the overlay state as saved.
#[derive(serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct CapabilityBody {
    pub kind: String,
    pub key: String,
    /// `on`, `off` or `auto`; a skill takes `on` or `auto`.
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
                aiplane_core::server::db::chat_session_skills::record(
                    &state.db,
                    &session_id,
                    &parsed.key,
                )
                .await
            }
            "auto" => {
                aiplane_core::server::db::chat_session_skills::remove(
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
            Ok(()) => ok_json(StatusCode::OK, parsed),
            Err(err) => internal(err),
        };
    }
    debug_assert!(capability.can_disable);
    let result = match parsed.state.as_str() {
        "on" => {
            aiplane_core::server::db::chat_session_tools::set(
                &state.db,
                &session_id,
                &parsed.key,
                true,
                "manual",
            )
            .await
        }
        "off" => {
            aiplane_core::server::db::chat_session_tools::set(
                &state.db,
                &session_id,
                &parsed.key,
                false,
                "manual",
            )
            .await
        }
        "auto" => {
            aiplane_core::server::db::chat_session_tools::clear(&state.db, &session_id, &parsed.key)
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
        Ok(()) => ok_json(StatusCode::OK, parsed),
        Err(err) => internal(err),
    }
}

// ---------------------------------------------------------------------------
// Token self-service: model allowlist + quota (owner-side, like /tokens)

#[derive(serde::Deserialize, schemars::JsonSchema)]
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
    use aiplane_core::server::db::limits;
    use aiplane_core::server::db::token_models;
    use aiplane_core::server::db::tokens;

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
        Ok(()) => ok_json(StatusCode::OK, OwnerModels { models: to_store }),
        Err(err) => internal(err),
    }
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct OwnerModels {
    /// The token's model allowlist; empty means every model the owner may use.
    pub models: Vec<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
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
    use aiplane_core::server::db::limits::{self, Dimension, ManagedBy, SubjectType, Window};

    let (_session, user) = require_session_json!(state, req);
    // Owner-only: another user's token reads as missing — no cross-account
    // probing.
    let is_owner = matches!(
        aiplane_core::server::db::tokens::find_by_id(&state.db, &token_id).await,
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
        Ok(_) => ok_json(StatusCode::OK, crate::pages::Done::OK),
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
    use aiplane_core::server::db::limits;

    let (_session, user) = require_session_json!(state, req);
    match aiplane_core::server::db::tokens::find_by_id(&state.db, &token_id).await {
        Ok(Some(t)) if t.user_id == user.id => {}
        Ok(_) => return json_error(StatusCode::NOT_FOUND, "not_found", "no such token"),
        Err(err) => {
            return internal(err);
        }
    }
    match limits::delete_owner_rule(&state.db, &token_id, &rule_id).await {
        Ok(true) => ok_json(StatusCode::OK, QuotaRuleDeleted { deleted: true }),
        Ok(false) => not_found("no such quota rule on this token"),
        Err(err) => internal(err),
    }
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct QuotaRuleDeleted {
    /// Always `true`.
    pub deleted: bool,
}

#[derive(serde::Deserialize)]
pub struct TokenRulePath {
    pub id: String,
    pub rule_id: String,
}

#[derive(serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
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
    use aiplane_core::server::db::user_mcp::{AskOverApi, set_token_policy};

    let (_session, user) = require_session_json!(state, req);
    // Owner-only, and a foreign token reads as missing (no probing for live
    // ids across accounts).
    match aiplane_core::server::db::tokens::find_by_id(&state.db, &token_id).await {
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
        Ok(()) => ok_json(StatusCode::OK, parsed),
        Err(err) => {
            tracing::warn!(error = %err, %token_id, "set token mcp policy");
            internal(err)
        }
    }
}
