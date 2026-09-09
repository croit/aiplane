// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Multi-conversation chat page.
//!
//! Routes:
//!
//! | Method | Path                       | What |
//! |--------|----------------------------|------|
//! | GET    | /chat                      | redirect to latest session (or create one) |
//! | GET    | /chat/{id}                 | render a specific session |
//! | POST   | /chat/sessions             | create a fresh session + nav to it |
//! | POST   | /chat/{id}/messages        | submit a user message; spawns worker; SSE-tails the live broadcast |
//! | GET    | /chat/{id}/tail            | subscribe to whatever worker is running for this user + session |
//! | POST   | /chat/{id}/cancel          | flip the worker's cancel flag |
//! | POST   | /chat/{id}/delete          | remove the session + nav to the next one |
//!
//! Worker lifecycle: `POST /chat/{id}/messages` creates the user turn,
//! creates the assistant turn (status `in_progress`), then spawns
//! `worker::run_chat_turn`. The worker writes content / reasoning /
//! tool-call deltas straight to SQLite and broadcasts a `Tick` after
//! every DB write. All HTTP subscribers (the messages POST itself + any
//! tail GET) re-read the row from the DB on each tick and emit the
//! same `mode outer` patch keyed to `#turn-<uuid>`. DB is the source of
//! truth; nothing the subscriber emits depends on in-memory state.

use std::sync::Arc;

use rama::http::service::web::extract::State;
use rama::http::{Request, Response};

use session_core::i18n::{Lang, t};
use session_core::{RegisterOutcome, TurnUpdate};

use session_core::db as chat;

use gateway_core::server::db::users::User;
use gateway_features::server::chat_attachments;
use gateway_runtime::rama_server::state::RamaState;

pub mod json_api;
mod title;

// ---------------------------------------------------------------------------
// GET /chat — redirect to latest (or new) session.

// ---------------------------------------------------------------------------
// GET /chat/{id} — render a specific session.

// ---------------------------------------------------------------------------
// POST /chat/sessions — new session + nav to it.

// ---------------------------------------------------------------------------
// POST /chat/{id}/delete — drop the session + nav to the next one.

// ---------------------------------------------------------------------------
// POST /chat/{id}/share — owner toggles the read-only share flag. Shared
// sessions are readable by any signed-in user who knows the (UUID) link.

// ---------------------------------------------------------------------------
// POST /chat/{id}/pin — owner toggles the conversation's pinned flag. Pinned
// conversations float to the top of the sidebar list. Pure UI affordance —
// pinning never changes who can read the session.

// ---------------------------------------------------------------------------
// GET /chat/search — search across the user's conversations.

// ---------------------------------------------------------------------------
// POST /chat/{id}/capabilities — owner pins/unpins a tool, MCP integration, or
// skill for this conversation (the composer's "+" menu). Writes the same
// per-conversation overlay the model drives via `enable_tools` / `read_skill`,
// with `source = "user"`, and re-patches the `#capabilities` region.

// ---------------------------------------------------------------------------
// POST /chat/{id}/effort — owner sets the conversation's "Denkaufwand". One
// knob driving both the reasoning budget and the tool-round cap.

// ---------------------------------------------------------------------------
// POST /chat/{id}/fork — copy a shared conversation into the viewer's
// account so the recipient can keep chatting (and re-share their copy).

// ---------------------------------------------------------------------------
// POST /chat/{id}/messages — submit + spawn worker + SSE.

/// Why a submit was refused, before anything was persisted. Both submit
/// surfaces (the legacy multipart → SSE-HTML composer and the JSON API)
/// map these onto their own response shapes.
#[derive(Debug)]
pub(crate) enum SubmitTurnError {
    RateLimited,
    /// This user's previous turn is still streaming — the registry refused.
    Busy,
    /// A DB write failed after the worker slot was reserved; the message is
    /// the human-readable cause. The slot has been released by the time this
    /// travels to the caller.
    Db(String),
}

/// A turn accepted into the worker: both rows persisted, worker spawned,
/// title generation scheduled.
///
/// No broadcast receiver rides along: the SPA opens `GET …/events` as its own
/// subscription, and that stream's first frame is a DB snapshot, so nothing is
/// lost between this call returning and the client attaching.
pub(crate) struct SubmittedTurn {
    pub user_turn: chat::Turn,
    pub assistant_turn: chat::Turn,
}

/// The submit core shared by the HTML composer and the JSON API: rate/quota
/// gate, worker-slot reservation, turn persistence, heuristic title, worker
/// spawn, LLM title generation. Errors leave no rows behind and release the
/// worker slot.
///
/// Ordering is load-bearing and commented where it's subtle — see the inline
/// notes; they were part of [`chat_message_send`] before the extraction and
/// describe bugs the order prevents.
pub(crate) async fn submit_turn(
    state: &Arc<RamaState>,
    user: &User,
    active: &chat::Session,
    submit: ChatSubmit,
    req: RequestCtx,
) -> Result<SubmittedTurn, SubmitTurnError> {
    // Rate-limit / quota gate — before reserving a worker or touching the DB,
    // so an over-budget user is turned away cleanly (details on `/usage`).
    {
        let role_ids = state.role_ids_for(&user.roles);
        if state.enforcer.check(&user.id, &role_ids).await.is_err() {
            return Err(SubmitTurnError::RateLimited);
        }
    }

    // Build the final user_text: typed text + per-attachment marker
    // (and an inlined fenced block for `text/*`-like attachments so
    // the model reads the bytes directly on the current turn).
    let user_msg = augment_user_text(&submit.user_turn_id, &submit);

    // Reserve the per-user worker slot BEFORE persisting anything.
    // The old order (create turns → register) leaked orphaned
    // `in_progress` rows whenever register returned Busy (a quick
    // double-click, a datastar retry on a flaky connection): the rows
    // sat in the DB forever showing the thinking spinner, and the
    // user would see a duplicate of their conversation after reload
    // because the *next* submit succeeded and produced a parallel
    // (user + completed-assistant) pair. The pre-generated id is the
    // turn we'll insert immediately below, so the worker entry's
    // `turn_id` always matches the row that exists.
    let assistant_turn_id = uuid::Uuid::new_v4().to_string();
    let outcome = state
        .chats
        .register(&user.id, &assistant_turn_id, &active.id);
    let worker = match outcome {
        RegisterOutcome::Registered { worker } => worker,
        RegisterOutcome::Busy { .. } => return Err(SubmitTurnError::Busy),
    };

    // Slot held. Any early-return from here must `state.chats.clear`
    // the worker so the next submit isn't permanently blocked.
    let user_turn_id = submit.user_turn_id.clone();
    let user_turn =
        match chat::create_user_turn(&state.db, &active.id, &user_turn_id, &user_msg).await {
            Ok(t) => t,
            Err(err) => {
                state.chats.clear(&user.id, &worker);
                return Err(SubmitTurnError::Db(err.to_string()));
            }
        };
    // Auto-title on the first user turn. Two-stage so the sidebar
    // never sits on "Untitled chat" for long:
    //   1. Immediately persist a heuristic title (the user message,
    //      single-lined and truncated) so the row has something to
    //      show in the time it takes the model to respond.
    //   2. Spawn a background LLM call that asks for a tight 3-6 word
    //      title and overwrites the heuristic when it lands (~hundreds
    //      of ms typically).
    // Both stages push a `TurnUpdate::SidebarChanged` through the
    // worker's broadcast — the heuristic one fires synchronously below
    // (right after the assistant turn insert), the LLM-gen one fires
    // inside `generate_session_title` if the worker is still live.
    let auto_titled = active.title.is_none();
    if auto_titled {
        // Title from the user-typed prefix only — attachment markers
        // would make a noisy sidebar title.
        let fallback = first_message_title(&submit.user_text);
        let _ = chat::set_session_title(&state.db, &active.id, &fallback).await;
    }
    let assistant_turn = match chat::create_assistant_turn_in_progress(
        &state.db,
        &active.id,
        &assistant_turn_id,
        &submit.model,
    )
    .await
    {
        Ok(t) => t,
        Err(err) => {
            state.chats.clear(&user.id, &worker);
            return Err(SubmitTurnError::Db(err.to_string()));
        }
    };
    let _ = chat::touch_session(&state.db, &active.id).await;

    // Push the heuristic-titled sidebar row update into the broadcast
    // *now* (synchronously, before any other tasks can send) so the
    // forwarding subscriber's first action after the initial bubble
    // append is to repaint the sidebar row with the new title. Without
    // this the sidebar would sit on "Untitled chat" until LLM-gen
    // lands — which might race the worker's Finalized and miss the
    // window.
    if auto_titled {
        let _ = worker.broadcast.send(TurnUpdate::SidebarChanged);
    }

    spawn_assistant_worker(
        state,
        user,
        &active.id,
        &assistant_turn_id,
        &submit.model,
        &worker,
        req,
    )
    .await;

    // Background LLM call that names the conversation. If the worker
    // is still live when this lands, the title-gen task broadcasts a
    // second `SidebarChanged` with the better name; otherwise the
    // user sees it on their next page interaction.
    if auto_titled {
        tokio::spawn(title::generate_session_title(
            state.clone(),
            user.id.clone(),
            active.id.clone(),
            submit.user_text.clone(),
            submit.model.clone(),
        ));
    }

    Ok(SubmittedTurn {
        user_turn,
        assistant_turn,
    })
}

// ---------------------------------------------------------------------------
// GET /chat/{id}/tail — attach to whatever worker is running for this
// user + session.

// ---------------------------------------------------------------------------
// GET /chat/{id}/turns/{turn_id}/thinking — the on-demand live reasoning
// sub-stream. The main turn stream never carries a still-growing thinking
// body (a collapsed trace costs zero bytes); expanding the `<details>`
// fires the `data-on:toggle` → `@get` that lands here. Readable-gated like
// the tail so a shared-chat viewer can also expand the trace.

// ---------------------------------------------------------------------------
// GET /chat/{id}/document/{doc_id} — render a document (optionally an older
// `?version=N`) into the canvas slot. This is the datastar `@get` target for
// the panel's document- and version-switchers: it returns a single SSE patch
// replacing `#document-canvas-slot`'s contents. Readable-gated like the tail
// (owner or a shared viewer); a missing doc/version closes with no change.

/// Path params for the two document routes, as a **named struct**.
///
/// Not `Path<(String, String)>`: a tuple is deserialised from the matcher's
/// param map in *map* order, not path order, so a two-param tuple binds
/// `(doc_id, id)` or `(id, doc_id)` depending on hash iteration — the same
/// build can flip between requests. It cost an afternoon here: the canvas
/// switcher would intermittently look up the session id as a document id and
/// answer with an empty patch. Named fields deserialise by key, which is why
/// every other multi-param route in this file (`TurnPath`,
/// `AttachmentRemovePath`) is a struct too.
#[derive(serde::Deserialize)]
pub struct DocumentPath {
    pub id: String,
    pub doc_id: String,
}

// ---------------------------------------------------------------------------
// POST /chat/{id}/document/{doc_id}/edit — the user's own hand edit.
//
// The canvas was assistant-only: when the model got a passage wrong, the only
// recourse was asking it to try again. This saves the panel's textarea as a
// new version authored by the *user*, which the request context then tells the
// model about so its next edit builds on the correction instead of reverting
// it.
//
// Owner-only (not `get_session_readable`): a shared conversation is readable
// by anyone holding the link, and writing to someone else's document is not
// reading. The UI hides the affordance for them; this is what enforces it.

/// Pull `version=N` out of a raw query string (`a=b&version=3`). `None`
/// when absent or unparseable, which the caller reads as "latest".
fn parse_version_query(q: &str) -> Option<i64> {
    q.split('&')
        .find_map(|kv| kv.strip_prefix("version="))
        .and_then(|v| v.parse().ok())
}

// ---------------------------------------------------------------------------
// POST /chat/{id}/cancel — flip the cancel flag.

// ---------------------------------------------------------------------------
// POST /chat/{id}/turns/{turn_id}/retry  and  …/edit
//
// Retry re-generates an assistant reply; edit rewrites a user message
// and re-generates from it. Both drop the target turn's downstream
// turns (everything below the regeneration point) and re-run the model
// with the currently-selected model. Reuses the same worker machinery
// as a fresh message via `start_regeneration`.

#[derive(serde::Deserialize)]
pub struct TurnPath {
    pub id: String,
    pub turn_id: String,
}

/// The attachments a pending `delete_turns_from_seq` is about to
/// orphan. Read *before* the delete — afterwards the markers are gone
/// and the bucket objects are unreferenced forever. Empty when
/// attachments aren't configured or the read fails: reclaiming is
/// housekeeping and must never block the user's retry/edit.
async fn doomed_attachments(
    state: &Arc<RamaState>,
    session_id: &str,
    from_seq: i64,
) -> Vec<chat_attachments::AttachmentRef> {
    if state.config().chat.s3.is_none() {
        return Vec::new();
    }
    match chat_attachments::attachments_from_seq(&state.db, session_id, from_seq).await {
        Ok(refs) => refs,
        Err(err) => {
            tracing::warn!(error = %err, %session_id, "listing attachments of doomed turns");
            Vec::new()
        }
    }
}

/// Fire-and-forget the bucket DELETEs for turns that are already gone
/// from the DB. Off the request path: a slow or flaky bucket must not
/// delay the regeneration the user is waiting on, and a failed DELETE
/// only leaves an orphan (which a later delete of the same key would
/// clean up — S3 DELETE is idempotent).
fn reclaim_attachments(state: &Arc<RamaState>, orphaned: Vec<chat_attachments::AttachmentRef>) {
    if orphaned.is_empty() {
        return;
    }
    let state = state.clone();
    tokio::spawn(async move {
        if let Some(cfg) = state.config().chat.s3.as_ref() {
            chat_attachments::reclaim_all(cfg, &orphaned).await;
        }
    });
}

/// Request-derived bits the worker needs that aren't part of the chat
/// session itself: the caller's source IP (for GeoIP) and whether the
/// browser is on a secure context (so a precise-location prompt can even
/// succeed). Bundled so the worker/regeneration signatures stay legible.
pub(crate) struct RequestCtx {
    client_ip: Option<String>,
    secure: bool,
    /// This turn came from voice-conversation mode → the driver injects the
    /// brevity/spoken-style directive. False for retry/edit regeneration.
    voice_mode: bool,
}

/// Build the caller's tool context + allowed-tool set and spawn the
/// per-turn worker that drives `assistant_turn_id`, clearing the
/// registry slot on exit. The single home for the worker/driver wiring
/// shared by the message-send and retry/edit (regeneration) paths — the
/// caller owns the worker registration, the assistant-turn row, and the
/// SSE response framing; this owns everything between.
async fn spawn_assistant_worker(
    state: &Arc<RamaState>,
    user: &User,
    session_id: &str,
    assistant_turn_id: &str,
    model: &str,
    worker: &session_core::workers::ActiveWorker,
    req: RequestCtx,
) {
    // Per-conversation tool overlay. The driver re-resolves the allowed-tool
    // set per round via `allowed_tools_for_session` (core ∪ this-conversation's
    // enabled, intersected with the user's RBAC grant), so a mid-turn
    // `enable_tools` call by the model surfaces the new schemas on the next
    // round. The chat path always goes through this overlay; the proxy path
    // uses the unfiltered per-user set. Everything but the two interactive
    // handles below is shared with the headless scheduler via
    // `build_tool_context`.
    let tool_ctx = gateway_runtime::openai_driver::build_tool_context(
        state,
        gateway_runtime::openai_driver::TurnFacts {
            user_id: user.id.clone(),
            roles: user.roles.clone(),
            session_id: session_id.to_string(),
            assistant_turn_id: assistant_turn_id.to_string(),
            client_ip: req.client_ip,
            // Chat path: hand the tools the live turn's broadcast + the
            // feedback hubs, so `get_user_location` can prompt for a precise
            // position and `ask_user` can ask a question — both mid-turn, both
            // waiting for the browser's reply.
            chat_feedback: Some(gateway_runtime::server::tools::ChatFeedback {
                broadcast: worker.broadcast.clone(),
                hub: state.location_feedback.clone(),
                ask_hub: state.ask_feedback.clone(),
                secure: req.secure,
            }),
            model: Some(model.to_string()),
            // Session path: access is exactly the user's group grant.
            pool_access: None,
        },
    );
    let driver = Box::new(gateway_runtime::openai_driver::OpenAiDriver {
        state: state.clone(),
        tool_ctx,
        source: gateway_core::server::db::usage::UsageSource::Chat,
        history_limit: None,
        voice_mode: req.voice_mode,
    });
    let driver_ctx = session_core::driver::SessionContext {
        user_id: Some(user.id.clone()),
        session_id: session_id.to_string(),
        assistant_turn_id: assistant_turn_id.to_string(),
        model: model.to_string(),
        cancel: worker.cancel.clone(),
        broadcast: worker.broadcast.clone(),
    };
    let worker_state = state.clone();
    let worker_for_task = worker.clone();
    let user_id_for_clear = user.id.clone();
    let pool_for_worker = state.db.clone();
    let session_id_for_push = session_id.to_string();
    let turn_id_for_push = assistant_turn_id.to_string();
    tokio::spawn(async move {
        session_core::worker::run_session_turn(pool_for_worker, driver, driver_ctx).await;
        worker_state
            .chats
            .clear(&user_id_for_clear, &worker_for_task);
        // Turn's done — ping the user's subscribed browsers (no-op unless push
        // is enabled and they opted in). Runs after the worker so the row is
        // finalized and the status is readable.
        notify_turn_complete(
            &worker_state,
            &user_id_for_clear,
            &session_id_for_push,
            &turn_id_for_push,
        )
        .await;
    });
}

/// Fire a Web Push "turn finished" notification to the user's subscribed
/// browsers once a turn they started finalizes.
///
/// No-op unless push is enabled and the user has at least one subscription.
/// Best-effort: every failure is logged, never surfaced — the turn itself
/// already succeeded. Subscriptions the push service reports gone (404/410)
/// are pruned. The server always sends; whether the user is *actually* looking
/// at the app is decided client-side in the service worker, which suppresses
/// the notification when a focused tab already has this conversation open.
async fn notify_turn_complete(
    state: &RamaState,
    user_id: &str,
    session_id: &str,
    assistant_turn_id: &str,
) {
    use gateway_features::server::push::{PushMessage, SendOutcome};
    use session_core::db::TurnStatus;

    let Some(push) = state.push.clone() else {
        return;
    };

    // Only notify on a real end state. `Cancelled` means the user pressed stop
    // (they're present), and `InProgress` shouldn't reach here.
    let turns = match session_core::db::list_turns(&state.db, session_id).await {
        Ok(t) => t,
        Err(err) => {
            tracing::warn!(error = %err, "push: reading finalized turn");
            return;
        }
    };
    let Some(view) = turns.iter().find(|t| t.turn.id == assistant_turn_id) else {
        return;
    };
    let errored = match view.turn.status {
        TurnStatus::Completed => false,
        TurnStatus::Errored => true,
        _ => return,
    };

    let subs = match gateway_core::server::db::push_subscriptions::list_for_user(&state.db, user_id)
        .await
    {
        Ok(s) if !s.is_empty() => s,
        Ok(_) => return,
        Err(err) => {
            tracing::warn!(error = %err, "push: listing subscriptions");
            return;
        }
    };

    // Conversation title for the heading; an untitled chat gets a localized
    // fallback per subscription below.
    let session_title = session_core::db::get_session(&state.db, user_id, session_id)
        .await
        .ok()
        .flatten()
        .and_then(|s| s.title)
        .filter(|t| !t.trim().is_empty());

    let url = format!("/chat/{session_id}");
    for sub in subs {
        let lang = sub
            .lang
            .as_deref()
            .and_then(Lang::from_code)
            .unwrap_or(Lang::En);
        // Cap the title: it's a user-influenced conversation title, and the
        // whole payload rides in one aes128gcm record with a 4 KB budget (and
        // FCM's own 4 KB body cap). 80 chars is plenty for a heading.
        let title = session_title
            .clone()
            .map(|t| session_core::text::truncate_chars(&t, 80))
            .unwrap_or_else(|| t(lang, "push-untitled-conversation"));
        let body = t(
            lang,
            if errored {
                "push-turn-error-body"
            } else {
                "push-turn-complete-body"
            },
        );
        let message = PushMessage {
            title,
            body,
            url: url.clone(),
            tag: session_id.to_string(),
        };
        if push.send(&sub, &message).await == SendOutcome::Gone
            && let Err(err) =
                gateway_core::server::db::push_subscriptions::delete(&state.db, &sub.id).await
        {
            tracing::warn!(error = %err, "push: pruning gone subscription");
        }
    }
}

// ---------------------------------------------------------------------------
// Sidebar emitter glue.
//
// The shared streaming loop in `session_core::chat::spawn_session_stream_response`
// invokes a per-binary callback whenever a `TurnUpdate::SidebarChanged`
// arrives. The gateway's sidebar is the chat-list — repatch the
// session row whose title just changed so the new title appears
// without waiting for the user's next nav.

/// Truncated first user message → session title. Trimmed to one line,
/// at most 64 chars, plus an ellipsis when truncated.
fn first_message_title(msg: &str) -> String {
    const MAX: usize = 64;
    let single_line: String = msg
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .chars()
        .take(MAX)
        .collect();
    if msg.chars().count() > MAX {
        format!("{single_line}…")
    } else {
        single_line
    }
}

// ---------------------------------------------------------------------------
// Multipart parsing for /chat/{id}/messages
//
// The composer posts `multipart/form-data` with three named parts:
//   - `model`       text
//   - `message`     text (user-typed prose)
//   - `attachment`  file, repeated 0..N times
//
// Each `attachment` part is uploaded to S3 immediately (so we can
// reference the public URL from the user_text marker) and the raw
// bytes are then dropped — we don't keep them in memory past the
// upload.

pub(crate) struct ChatSubmit {
    pub(crate) model: String,
    pub(crate) user_text: String,
    pub(crate) attachments: Vec<UploadedAttachment>,
    /// This turn was submitted from voice-conversation mode — the worker
    /// injects the brevity/spoken-style directive. Per-turn only; not persisted.
    pub(crate) voice: bool,
    /// The pre-generated user-turn id. Attachment uploads are keyed by it
    /// *before* the turn row exists (the S3 prefix must match the row that
    /// lands later), so whoever parses the submit mints it and it travels
    /// with the payload into [`submit_turn`].
    pub(crate) user_turn_id: String,
}

pub(crate) struct UploadedAttachment {
    outcome: chat_attachments::UploadOutcome,
}

async fn parse_chat_submit(
    content_type: &str,
    body: rama::bytes::Bytes,
    turn_id: &str,
    state: &RamaState,
) -> Result<ChatSubmit, String> {
    // Kept in a local so the returned struct can own it without borrowing
    // the parameter.
    let boundary = multer::parse_boundary(content_type).map_err(|err| {
        format!(
            "expected multipart/form-data submit (the composer should set \
             enctype=\"multipart/form-data\"): {err}"
        )
    })?;
    let stream =
        rama::futures::stream::once(async move { Ok::<_, std::convert::Infallible>(body) });
    let mut mp = multer::Multipart::new(stream, boundary);

    let mut model: Option<String> = None;
    let mut user_text = String::new();
    let mut attachments: Vec<UploadedAttachment> = Vec::new();
    let mut voice = false;
    let user_turn_id = turn_id.to_string();

    // Track the filenames already claimed under this turn so each upload
    // lands on a distinct S3 key. Seeded with any filenames already
    // attached to the turn (empty on the new-message path; populated when
    // editing a turn that already has attachments). Without this, several
    // clipboard-pasted images — the browser names every one `image.png` —
    // upload to the SAME key and overwrite each other, so every marker
    // ends up pointing at the last image. Dedup renames the collisions to
    // `image-2.png`, `image-3.png`, … exactly like `reserve_filename`.
    let existing_content = chat::get_content(&state.db, turn_id)
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
    let mut used_names = session_core::attachments::existing_filenames(&existing_content);

    while let Some(field) = mp.next_field().await.map_err(|e| e.to_string())? {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "model" => {
                model = Some(field.text().await.map_err(|e| e.to_string())?);
            }
            "message" => {
                user_text = field.text().await.map_err(|e| e.to_string())?;
            }
            "voice" => {
                let v = field.text().await.map_err(|e| e.to_string())?;
                voice = matches!(v.trim(), "true" | "1" | "on");
            }
            "attachment" => {
                // Browsers always emit the `attachment` part for the
                // hidden `<input type="file">` even when no file was
                // picked — `filename=""` + zero bytes. Skip those so
                // a plain-text send doesn't fail upload validation.
                let filename = field.file_name().map(str::to_string).unwrap_or_default();
                let mime = field
                    .content_type()
                    .map(|m| m.to_string())
                    .unwrap_or_else(|| "application/octet-stream".to_string());
                let bytes = field.bytes().await.map_err(|e| e.to_string())?.to_vec();
                if filename.is_empty() && bytes.is_empty() {
                    continue;
                }
                // Bound, not chained: `config()` hands back an owned snapshot,
                // and borrowing `[chat.s3]` out of a temporary would drop it at
                // the end of the statement.
                let config = state.config();
                let cfg = config.chat.s3.as_ref().ok_or_else(|| {
                    "chat attachments are not configured (enable [chat.s3] \
                         at /admin/settings)"
                        .to_string()
                })?;
                // Nameless blobs (some drag/paste sources) get a
                // mime-appropriate default before dedup so we never upload
                // an empty-named object.
                let desired = if filename.trim().is_empty() {
                    format!(
                        "pasted{}",
                        chat_attachments::ext_for_mime(&mime).unwrap_or(".bin")
                    )
                } else {
                    filename
                };
                let filename =
                    session_core::attachments::dedupe_filename_against(&used_names, &desired);
                used_names.insert(filename.clone());
                let outcome = chat_attachments::upload(cfg, turn_id, &filename, &mime, bytes)
                    .await
                    .map_err(|e| format!("upload `{filename}`: {e}"))?;
                attachments.push(UploadedAttachment { outcome });
            }
            _ => {
                // Ignore unknown fields — datastar may emit a few
                // bookkeeping bits that we don't care about.
            }
        }
    }

    let model = model
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "no model selected".to_string())?;
    Ok(ChatSubmit {
        model,
        user_text: user_text.trim().to_string(),
        attachments,
        voice,
        user_turn_id,
    })
}

/// Build the final `user_text` that we persist into `chat_turns`.
/// Layout:
///
///   <user-typed text>
///
///   [gw-attachment file="…" mime="…" url="…" size=N]
///   …
///
/// The marker is the only thing that goes into `user_text` — no
/// fenced-block inlining of text contents, since the LLM payload
/// rewrites the marker to an opaque-id stub anyway and the model
/// fetches bytes on demand via `fetch_attachment`. Inlining would
/// just bloat the persisted row without anyone reading it (the
/// chat-bubble renderer skips the fenced block via `split_markers`).
fn augment_user_text(turn_id: &str, submit: &ChatSubmit) -> String {
    let mut out = submit.user_text.clone();
    for att in &submit.attachments {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(&chat_attachments::marker_line(turn_id, &att.outcome));
    }
    out
}

// Attachment URLs are baked into each marker at write time by
// `chat_attachments::marker_line` — see that module for the
// `proxy_url(turn_id, filename)` helper. The renderer just reads
// `att.url` and drops it straight into `<img src>` / chip hrefs.
// The S3 bucket is never reached directly from a browser or
// upstream LLM; bytes always stream through the gateway with
// session + turn-ownership checks applied first. The original
// "unauthenticated egress" concern that motivated the
// presign-everywhere design is gone: the proxy route requires the
// session cookie AND verifies the turn belongs to the cookie
// holder.

// ---------------------------------------------------------------------------
// GET /chat/{id}/export.md  and  GET /chat/{id}/export.pdf
//
// Download the whole conversation as a self-contained document. Both
// formats share the same gate as the chat view (owner OR shared) and the
// same body builder in `session_core::export`; only the serialization and
// the response headers differ. The Markdown path is pure-Rust; the PDF
// path shells out to the bundled `typst` CLI (the same engine the letter
// templates use).

// ---------------------------------------------------------------------------
// GET /chat/attachment/{turn_id}/{filename} — bytes for one attachment.

/// `<turn_id>/<filename>` for `GET /chat/attachment/{turn_id}/{filename}`,
/// read off the **raw** request URI.
///
/// Not the `Path` extractor: rama's router matches on
/// `uri.path().to_lowercase()` and the `UriParams` it hands the extractor come
/// from that lowercased string — and it never percent-decodes them. Either
/// mangling asks the bucket for a key that was never written (`bericht.md` for
/// a stored `Bericht.md`, or a literal `%20` where the name has a space), and
/// the browser shows the download as "file wasn't available on site".
/// `req.uri()` is untouched. Same reason `proxy::retrieve_model` and
/// `sandbox_api::download` parse by hand.
///
/// Both mount points are accepted: `/api/v0/chat/attachment/…`, the canonical
/// one the SPA calls, and the bare `/chat/attachment/…` that every stored
/// attachment marker carries (`chat_attachments::proxy_url` writes it into
/// turn content, so it is a data format and cannot be renamed without a
/// content migration). Accepting only one of them was why the route 404'd:
/// the handler is mounted under `/api/v0` and this stripped the other prefix.
///
/// `None` when the path isn't `<turn>/<file>` with both segments non-empty
/// and no extra segment beyond the filename.
fn attachment_path_parts(path: &str) -> Option<(String, String)> {
    let rest = path
        .strip_prefix("/api/v0/chat/attachment/")
        .or_else(|| path.strip_prefix("/chat/attachment/"))?;
    let (turn_id, filename) = rest.split_once('/')?;
    let turn_id = percent_decode_segment(turn_id);
    let filename = percent_decode_segment(filename);
    if turn_id.is_empty() || filename.is_empty() || filename.contains('/') {
        return None;
    }
    Some((turn_id, filename))
}

/// Percent-decode one path segment. `+` stays a literal plus (it is not a
/// space in a path — that's `application/x-www-form-urlencoded`, which is why
/// this isn't `pages::skills::percent_decode`). Malformed escapes pass
/// through untouched rather than eating characters.
fn percent_decode_segment(s: &str) -> String {
    if !s.contains('%') {
        return s.to_string();
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            )
        {
            out.push((hi * 16 + lo) as u8);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Stream one attachment's bytes through the gateway, gated by the
/// session cookie + a check that the turn belongs to the caller's
/// user. Bucket never sees a browser request; the LLM never sees a
/// presigned URL.
pub async fn chat_attachment(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let lang = Lang::from_headers(req.headers());
    let Some((turn_id, filename)) = attachment_path_parts(req.uri().path()) else {
        return attachment_error(
            rama::http::StatusCode::BAD_REQUEST,
            &t(lang, "chat-error-bad-filename"),
        );
    };
    // 401 (not redirect) — `<img src>` will just show broken-image
    // if the cookie went bad, and a 401 is honest in operator logs.
    let session = match state.sessions.lookup_from_headers(req.headers()).await {
        Ok(Some(s)) => s,
        _ => {
            return attachment_error(
                rama::http::StatusCode::UNAUTHORIZED,
                &t(lang, "chat-error-auth-required"),
            );
        }
    };
    // Readable = the turn's session is owned by the caller OR shared. Mirrors
    // the chat-view gate so attachments in a shared conversation are
    // fetchable by a viewer, while a private turn's files stay owner-only.
    // 404 (not 403) on miss/denied — don't leak whether the turn exists.
    match session_core::db::turn_session_readable(&state.db, &turn_id, &session.user_id).await {
        Ok(true) => {}
        Ok(false) => {
            tracing::warn!(
                requester = %session.user_id, %turn_id,
                "rejected attachment fetch (not owner, not shared)",
            );
            return attachment_error(
                rama::http::StatusCode::NOT_FOUND,
                &t(lang, "chat-error-no-such-turn"),
            );
        }
        Err(err) => {
            tracing::warn!(error = %err, "turn_session_readable");
            return attachment_error(
                rama::http::StatusCode::INTERNAL_SERVER_ERROR,
                &t(lang, "chat-error-db-error"),
            );
        }
    }
    let config = state.config();
    let Some(cfg) = config.chat.s3.as_ref() else {
        return attachment_error(
            rama::http::StatusCode::SERVICE_UNAVAILABLE,
            &t(lang, "chat-error-attachments-not-configured"),
        );
    };
    let fetched = match chat_attachments::fetch(cfg, &turn_id, &filename).await {
        Ok(f) => f,
        Err(chat_attachments::AttachmentError::BadFilename(_)) => {
            return attachment_error(
                rama::http::StatusCode::BAD_REQUEST,
                &t(lang, "chat-error-bad-filename"),
            );
        }
        Err(err) => {
            tracing::warn!(error = %err, %turn_id, %filename, "attachment fetch");
            return attachment_error(
                rama::http::StatusCode::NOT_FOUND,
                &t(lang, "chat-error-attachment-not-found"),
            );
        }
    };
    Response::builder()
        .status(rama::http::StatusCode::OK)
        .header(rama::http::header::CONTENT_TYPE, fetched.mime)
        .header(rama::http::header::CONTENT_LENGTH, fetched.bytes.len())
        // Content-addressed: <turn_id> is a UUID, filename is fixed
        // for that turn — the bytes can't change. 1 h max-age keeps
        // a viewing session cheap; not `immutable` so future
        // delete/replace semantics don't get cache-pinned forever.
        .header(rama::http::header::CACHE_CONTROL, "private, max-age=3600")
        .body(fetched.bytes.into())
        .unwrap_or_else(|err| {
            tracing::error!(error = %err, "attachment response build");
            attachment_error(
                rama::http::StatusCode::INTERNAL_SERVER_ERROR,
                "response build",
            )
        })
}

fn attachment_error(status: rama::http::StatusCode, msg: &str) -> Response {
    Response::builder()
        .status(status)
        .header(
            rama::http::header::CONTENT_TYPE,
            "text/plain; charset=utf-8",
        )
        .body(msg.to_string().into())
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point of parsing by hand: a stored `Bericht.md` must not
    /// become `bericht.md`, and `%20` must become a space — the object key is
    /// built from these two strings.
    #[test]
    fn attachment_path_keeps_case_and_decodes_escapes() {
        assert_eq!(
            attachment_path_parts("/chat/attachment/t-1/Bericht.md"),
            Some(("t-1".to_string(), "Bericht.md".to_string()))
        );
        assert_eq!(
            attachment_path_parts("/chat/attachment/t-1/Bericht%20Q3.md"),
            Some(("t-1".to_string(), "Bericht Q3.md".to_string()))
        );
        assert_eq!(
            attachment_path_parts("/chat/attachment/t-1/%C3%9Cbersicht.md"),
            Some(("t-1".to_string(), "Übersicht.md".to_string()))
        );
    }

    /// Both mount points resolve.
    ///
    /// The handler is registered under `/api/v0/…` for the SPA, and under the
    /// bare `/chat/attachment/…` that every stored marker carries. Parsing
    /// only one of them 404'd the other — and for the bare path that 404 was
    /// invisible, because the SPA catch-all answered it with the app shell at
    /// 200 text/html: a broken `<img>` and a "download" that saved
    /// `index.html` under the attachment's name.
    #[test]
    fn attachment_path_parses_both_the_api_and_the_stored_prefix() {
        let expected = Some(("t-1".to_string(), "Bericht.md".to_string()));
        assert_eq!(
            attachment_path_parts("/api/v0/chat/attachment/t-1/Bericht.md"),
            expected,
            "the route the SPA calls"
        );
        assert_eq!(
            attachment_path_parts("/chat/attachment/t-1/Bericht.md"),
            expected,
            "the path `proxy_url` writes into stored turn content"
        );
    }

    /// A `/` smuggled in as `%2F` would punch out of the filename segment and
    /// let a caller reach another turn's prefix in the bucket.
    #[test]
    fn attachment_path_rejects_malformed_and_traversing_paths() {
        for path in [
            "/chat/attachment/t-1/",
            "/chat/attachment//x.png",
            "/chat/attachment/t-1",
            "/chat/attachment/t-1/a%2F..%2Fb.png",
            "/chat/attachment/t-1/sub/x.png",
            "/elsewhere/t-1/x.png",
        ] {
            assert_eq!(attachment_path_parts(path), None, "should reject {path}");
        }
    }

    /// `+` is a literal plus in a path segment (`C++ notes.md`), unlike in a
    /// form body; a truncated escape is left alone instead of swallowing the
    /// characters after it.
    #[test]
    fn percent_decode_segment_leaves_plus_and_bad_escapes_alone() {
        assert_eq!(percent_decode_segment("C++ notes.md"), "C++ notes.md");
        assert_eq!(percent_decode_segment("ends-with-%2"), "ends-with-%2");
        assert_eq!(percent_decode_segment("bad-%zz-seq"), "bad-%zz-seq");
        assert_eq!(percent_decode_segment("plain.md"), "plain.md");
    }
}
