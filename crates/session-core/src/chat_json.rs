// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The JSON event protocol for live chat streaming.
//!
//! One subscription model: the worker's [`TurnUpdate`] broadcast, the DB as
//! the single source of truth, and coalesced flushes. Each flush computes
//! *what changed since this subscriber last looked* and emits it as
//! structured JSON events:
//!
//! ```text
//! event: snapshot        → full session state (the DB replayer on attach)
//! event: turn_delta      → appended assistant text
//! event: reasoning_delta → appended reasoning text
//! event: tool_call_started / tool_call_done
//! event: turn_finalized  → terminal state + timing
//! event: sidebar_changed → the SPA refetches the session list
//! event: info            → transient notice (e.g. vision fallback)
//! event: tool_prompt     → human-in-loop prompt (ask_user / location)
//! event: idle            → no live worker; the stream ends here
//! ```
//!
//! A client that reconnects just re-attaches: the initial
//! [`ChatEvent::Snapshot`] is rebuilt from the DB and subsumes anything
//! missed. There is no `Last-Event-ID` replay because the DB *is* the
//! replayer — which is also why a dropped connection costs nothing but the
//! reconnect.

use std::time::Duration;

use rama::http::{Body, Response, StatusCode, header};
use serde::Serialize;
use tokio::sync::broadcast;

use crate::db::{self, TurnStatus, TurnWithTools};
use crate::workers::{ToolPromptEvent, TurnUpdate};

/// Cap the event rate without visibly changing liveness — the trailing flush
/// guarantees the final state always lands.
const EVENT_COALESCE: Duration = Duration::from_millis(120);

/// One JSON event on the wire. Serialized as `{event, data}` by
/// [`sse_json`]; `event` (the SSE event name) comes from
/// [`ChatEvent::name`].
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatEvent {
    /// Full session state, sent once on attach. The reconnect replayer.
    Snapshot {
        /// The live assistant turn id, when a worker is streaming into this
        /// session — clients key their "streaming" state off this.
        #[serde(skip_serializing_if = "Option::is_none")]
        live_turn_id: Option<String>,
        /// Every turn of the session in `seq` order, newest last.
        turns: Vec<TurnWithTools>,
        /// User turns that have been sent but not started yet.
        ///
        /// Carried rather than derived: "a trailing user turn with no answer"
        /// looks like the same thing and is not — a turn whose assistant row
        /// failed to insert has that shape with nothing queued, and a client
        /// guessing from it renders a spinner on a message that will never
        /// start. The server asks its work queue; this is the answer.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        waiting_turn_ids: Vec<String>,
    },
    /// Text appended to the assistant turn's `content` since the last event.
    /// `full: true` marks a cursor reset — the row was rewritten and
    /// `text_delta` carries the *whole* text, so the client replaces its
    /// buffer instead of appending (an append would keep stale bytes; the
    /// client cannot detect a rewrite on its own).
    TurnDelta {
        turn_id: String,
        text_delta: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        full: bool,
    },
    /// Text appended to the assistant turn's `reasoning`. Same `full`
    /// contract as [`ChatEvent::TurnDelta`].
    ReasoningDelta {
        turn_id: String,
        text_delta: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        full: bool,
    },
    /// The model invoked a tool.
    ToolCallStarted {
        turn_id: String,
        tool_call_id: String,
        name: String,
        /// Raw JSON arguments string (the model's `arguments` verbatim).
        arguments: String,
    },
    /// A tool call reached a terminal status.
    ToolCallDone {
        turn_id: String,
        tool_call_id: String,
        status: String,
        /// Raw JSON output string, when the call produced one. Full payload —
        /// truncating is a display decision, so it belongs to the client.
        #[serde(skip_serializing_if = "Option::is_none")]
        output: Option<String>,
    },
    /// The assistant turn ended. Terminal: no further deltas for this turn.
    TurnFinalized {
        turn_id: String,
        status: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        error_message: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        /// Wall-clock span of the turn, from row creation to completion.
        /// `None` while the driver never stamped a completion.
        duration_ms: Option<i64>,
    },
    /// A mid-turn interjection appeared, or the one already on screen
    /// reached its outcome.
    ///
    /// One event for both because the client merges on `id`: a note is drawn
    /// the moment it is typed (`pending`) and then re-labelled when it is
    /// either handed to the model (`delivered`) or re-sent as its own message
    /// (`resent`). Splitting it would make the client handle an "added" it
    /// may never have seen, on a stream it attached to late.
    Steer {
        turn_id: String,
        id: String,
        text: String,
        status: String,
    },
    /// Session metadata changed (title generated, pin toggled, …). The
    /// client refetches the session list; the event carries no payload by
    /// design — the list endpoint is the source of truth.
    SidebarChanged,
    /// Transient notice rendered as a dismissible banner.
    Info { message: String },
    /// A human-in-loop tool prompt (see [`ToolPromptEvent`]).
    ToolPrompt(Box<ToolPromptEvent>),
    /// No live worker for this session; nothing more will arrive. Sent
    /// instead of hanging an open stream on a quiet session — the client
    /// treats the stream close after this event as "idle", not "error", and
    /// its `EventSource` reconnect behavior stays cheap.
    Idle,
}

impl ChatEvent {
    /// The SSE `event:` name. Stable wire contract — clients dispatch on it.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Snapshot { .. } => "snapshot",
            Self::TurnDelta { .. } => "turn_delta",
            Self::ReasoningDelta { .. } => "reasoning_delta",
            Self::ToolCallStarted { .. } => "tool_call_started",
            Self::ToolCallDone { .. } => "tool_call_done",
            Self::TurnFinalized { .. } => "turn_finalized",
            Self::Steer { .. } => "steer",
            Self::SidebarChanged => "sidebar_changed",
            Self::Info { .. } => "info",
            Self::ToolPrompt(_) => "tool_prompt",
            Self::Idle => "idle",
        }
    }
}

/// Frame one event as an SSE block: `event:` + JSON `data:` (multi-line safe —
/// the JSON is single-line because we control serialization; newlines inside
/// string values are escaped by serde).
pub fn sse_json(event: &ChatEvent) -> rama::bytes::Bytes {
    let data = serde_json::to_string(event).unwrap_or_else(|err| {
        // ChatEvent is plain data; serialization cannot fail outside of I/O
        // errors serde_json doesn't produce for strings. Fall back to an
        // info event rather than killing the stream over one bad frame.
        tracing::warn!(error = %err, "chat_json: serializing event failed");
        serde_json::to_string(&ChatEvent::Info {
            message: "event serialization failed".into(),
        })
        .unwrap_or_else(|_| "{}".into())
    });
    rama::bytes::Bytes::from(format!("event: {}\ndata: {}\n\n", event.name(), data))
}

/// Per-subscriber diff state: what this subscriber has already seen for the
/// live turn. Computes deltas the same way the HTML loop computes patches —
/// by comparing the current DB row against the last-sent state — but the
/// JSON wire is append-only text, so the state is a pair of cursors plus the
/// tool-call statuses.
#[derive(Debug)]
pub struct JsonTurnFeed {
    assistant_turn_id: String,
    /// The `content` already emitted as `turn_delta`s — the text itself, not
    /// just its length, because only the text can tell an append apart from
    /// an in-place rewrite. See [`emit_append`].
    content_sent: String,
    /// The `reasoning` already emitted, same reasoning.
    reasoning_sent: String,
    /// Tool calls already announced, with the status last sent. Insertion
    /// order matches the row order (seq), so `started` events replay in the
    /// order the model issued them.
    tools_sent: Vec<(String, String)>,
    /// Interjections already announced, with the status last sent — same
    /// shape and same reason as `tools_sent`.
    steers_sent: Vec<(String, String)>,
    /// Whether [`ChatEvent::TurnFinalized`] has been emitted. The feed is
    /// done afterwards; the stream loop closes on it.
    finalized: bool,
}

impl JsonTurnFeed {
    pub fn new(assistant_turn_id: &str) -> Self {
        Self {
            assistant_turn_id: assistant_turn_id.to_string(),
            content_sent: String::new(),
            reasoning_sent: String::new(),
            tools_sent: Vec::new(),
            steers_sent: Vec::new(),
            finalized: false,
        }
    }

    /// Whether the finalize event already went out — the stream loop's cue
    /// to close.
    pub fn is_finalized(&self) -> bool {
        self.finalized
    }

    /// Diff the current DB row into events. Empty when nothing changed since
    /// the last call (the common case between coalesced ticks).
    pub fn diff(&mut self, current: &TurnWithTools) -> Vec<ChatEvent> {
        let turn = &current.turn;
        if turn.id != self.assistant_turn_id {
            // Not the turn this feed tracks (caller mixed rows). Treat as
            // nothing-to-say rather than corrupting the cursors.
            return Vec::new();
        }
        let mut events = Vec::new();

        let content = turn.content.as_deref().unwrap_or("");
        emit_append(
            &mut events,
            &turn.id,
            content,
            &mut self.content_sent,
            |turn_id, text_delta, full| ChatEvent::TurnDelta {
                turn_id,
                text_delta,
                full,
            },
        );

        let reasoning = turn.reasoning.as_deref().unwrap_or("");
        emit_append(
            &mut events,
            &turn.id,
            reasoning,
            &mut self.reasoning_sent,
            |turn_id, text_delta, full| ChatEvent::ReasoningDelta {
                turn_id,
                text_delta,
                full,
            },
        );

        for call in &current.tool_calls {
            match self.tools_sent.iter_mut().find(|(id, _)| *id == call.id) {
                Some(entry) => {
                    if entry.1 != call.status.as_str() {
                        entry.1 = call.status.as_str().to_string();
                        events.push(ChatEvent::ToolCallDone {
                            turn_id: turn.id.clone(),
                            tool_call_id: call.id.clone(),
                            status: call.status.as_str().to_string(),
                            output: call.output_json.clone(),
                        });
                    }
                }
                None => {
                    self.tools_sent
                        .push((call.id.clone(), call.status.as_str().to_string()));
                    events.push(ChatEvent::ToolCallStarted {
                        turn_id: turn.id.clone(),
                        tool_call_id: call.id.clone(),
                        name: call.name.clone(),
                        arguments: call.arguments_json.clone(),
                    });
                    // A tool can appear already-completed (fast call between
                    // two ticks) — announce both halves, not just the start.
                    if call.status.as_str() != "running" {
                        events.push(ChatEvent::ToolCallDone {
                            turn_id: turn.id.clone(),
                            tool_call_id: call.id.clone(),
                            status: call.status.as_str().to_string(),
                            output: call.output_json.clone(),
                        });
                    }
                }
            }
        }

        for steer in &current.steers {
            // Compare before allocating: on a quiet flush — the common case —
            // nothing has changed and the `to_string` would be thrown away.
            let status = steer.status.as_str();
            match self.steers_sent.iter_mut().find(|(id, _)| *id == steer.id) {
                Some(entry) if entry.1 == status => continue,
                Some(entry) => entry.1 = status.to_string(),
                None => self
                    .steers_sent
                    .push((steer.id.clone(), status.to_string())),
            }
            let status = status.to_string();
            events.push(ChatEvent::Steer {
                turn_id: turn.id.clone(),
                id: steer.id.clone(),
                text: steer.text.clone(),
                status,
            });
        }

        if !self.finalized && turn.status != TurnStatus::InProgress {
            self.finalized = true;
            events.push(ChatEvent::TurnFinalized {
                turn_id: turn.id.clone(),
                status: turn.status.as_str().to_string(),
                error_message: turn.error_message.clone(),
                model: turn.model.clone(),
                duration_ms: turn
                    .completed_at
                    .and_then(|done| (done - turn.created_at).total(jiff::Unit::Millisecond).ok())
                    .map(|ms| ms as i64)
                    .filter(|ms| *ms >= 0),
            });
        }
        events
    }
}

/// Emit a delta for whatever is new in `text`. A shrink, or a non-prefix
/// rewrite (a hand-edited row, a retry that reused the id, a tool that
/// rewrites the content it already wrote), cannot be expressed as an append,
/// so the whole text is re-sent as one delta flagged `full` and the client's
/// per-turn buffer converges on exactly the DB's content.
///
/// `sent` holds the previously emitted text rather than a byte count, and
/// that is the whole point. A cursor alone cannot detect a rewrite whose new
/// length is greater than or equal to what was already sent — which is
/// exactly what `typst_render` does when it strips its own markers and
/// appends the next render. With a bare cursor the client kept the stale
/// first render and appended a fragment of the second, healing only on
/// reconnect.
fn emit_append(
    events: &mut Vec<ChatEvent>,
    turn_id: &str,
    text: &str,
    sent: &mut String,
    make: impl Fn(String, String, bool) -> ChatEvent,
) {
    if text == sent {
        return; // nothing new — the common case between coalesced ticks
    }
    let reset = !text.starts_with(sent.as_str());
    let delta = if reset { text } else { &text[sent.len()..] };
    if delta.is_empty() {
        // A pure truncation to the empty string still has to reach the
        // client, or it would keep rendering text the row no longer has.
        if reset {
            events.push(make(turn_id.to_string(), String::new(), true));
            sent.clear();
        }
        return;
    }
    events.push(make(turn_id.to_string(), delta.to_string(), reset));
    if reset {
        sent.clear();
        sent.push_str(text);
    } else {
        // The common case: `delta` is exactly the tail, so appending it costs
        // O(delta) rather than re-copying the whole turn on every tick.
        sent.push_str(delta);
    }
}

/// The streaming half of the JSON protocol: subscribe to a worker's
/// broadcast and turn its signals + the DB row into JSON events on the
/// response channel. Mirrors `spawn_session_stream_response` in
/// [`crate::chat`] — same select-loop shape, same coalescing, same
/// "Finalized flushes authoritatively then closes" contract — with the HTML
/// diffing swapped for [`JsonTurnFeed`].
///
/// Returns the response body channel pair; the handler wraps `rx` into the
/// SSE response. The task ends when the channel closes (client went away),
/// the turn finalizes, or the turn row vanishes (session deleted mid-stream).
pub async fn run_json_turn_stream(
    pool: db::Pool,
    session_id: String,
    assistant_turn_id: String,
    mut broadcast_rx: broadcast::Receiver<TurnUpdate>,
    initial: Vec<ChatEvent>,
    tx: SseTx,
) {
    use rama::futures::sink::SinkExt;

    let mut tx = tx;
    let mut feed = JsonTurnFeed::new(&assistant_turn_id);
    let mut dirty = false;
    let mut last_emit: Option<tokio::time::Instant> = None;

    // Whatever the caller staged (the snapshot, typically) goes out first,
    // before any interleaving can reorder the wire.
    for event in initial {
        if tx.send(Ok(sse_json(&event))).await.is_err() {
            return;
        }
    }

    loop {
        // Flush at most every EVENT_COALESCE while dirty; the deadline
        // branch wakes exactly when the window elapses. A Finalized flush
        // is authoritative regardless of the window (it must land).
        let deadline =
            last_emit.map_or_else(tokio::time::Instant::now, |last| last + EVENT_COALESCE);
        tokio::select! {
            biased;
            update = broadcast_rx.recv() => match update {
                Ok(TurnUpdate::Tick) => dirty = true,
                Ok(TurnUpdate::SidebarChanged) => {
                    if tx.send(Ok(sse_json(&ChatEvent::SidebarChanged))).await.is_err() {
                        return;
                    }
                }
                Ok(TurnUpdate::InfoMessage(message)) => {
                    if tx.send(Ok(sse_json(&ChatEvent::Info { message }))).await.is_err() {
                        return;
                    }
                }
                Ok(TurnUpdate::Prompt(event)) => {
                    if tx.send(Ok(sse_json(&ChatEvent::ToolPrompt(Box::new((*event).clone()))))).await.is_err() {
                        return;
                    }
                }
                Ok(TurnUpdate::Finalized) => {
                    flush(&pool, &session_id, &assistant_turn_id, &mut feed, &mut tx).await;
                    return;
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    // Missed ticks are subsumed by the next DB re-read.
                    dirty = true;
                }
                Err(broadcast::error::RecvError::Closed) => {
                    // Worker gone without Finalized (killed): flush what's
                    // committed and end the stream.
                    flush(&pool, &session_id, &assistant_turn_id, &mut feed, &mut tx).await;
                    return;
                }
            },
            _ = tokio::time::sleep_until(deadline), if dirty => {
                flush(&pool, &session_id, &assistant_turn_id, &mut feed, &mut tx).await;
                last_emit = Some(tokio::time::Instant::now());
                dirty = false;
            }
        }
        if feed.is_finalized() {
            return;
        }
    }
}

/// Read the turn, diff it, send the events. A vanished row (session deleted
/// mid-stream) ends the stream.
async fn flush(
    pool: &db::Pool,
    session_id: &str,
    assistant_turn_id: &str,
    feed: &mut JsonTurnFeed,
    tx: &mut SseTx,
) {
    use rama::futures::sink::SinkExt;
    let turns = match db::get_turn_with_tools(pool, session_id, assistant_turn_id).await {
        Ok(t) => t,
        Err(err) => {
            tracing::warn!(error = %err, "chat_json: reading turn failed");
            return;
        }
    };
    let Some(current) = turns else { return };
    for event in feed.diff(&current) {
        if tx.send(Ok(sse_json(&event))).await.is_err() {
            return;
        }
    }
}

/// Serve a conversation whose message is waiting for a free slot: snapshot
/// first, then hold the stream open until its turn actually starts, and hand
/// over to [`run_json_turn_stream`] when it does.
///
/// Without this the stream would end at `idle` — there is no worker to tail —
/// and the page would sit on "waiting" long after the answer began, because
/// the worker that eventually starts belongs to a conversation nothing is
/// attached to. The alternative, a client asking again every few seconds, is a
/// poll standing in for an event the server already has.
///
/// Bounded by [`WAIT_FOR_START`]: an unbounded wait would pin a connection and
/// a task for as long as the queue stays congested. On expiry the stream ends
/// with `idle`, exactly as a quiet conversation does.
pub async fn stream_until_started(
    pool: db::Pool,
    workers: std::sync::Arc<crate::workers::SessionWorkers>,
    user_id: String,
    session_id: String,
    turns: Vec<TurnWithTools>,
    mut starts: broadcast::Receiver<crate::workers::WorkerStarted>,
    tx: SseTx,
) {
    use rama::futures::sink::SinkExt;

    let mut tx = tx;
    let waiting_turn_ids = waiting_ids(&pool, &session_id).await;
    let snapshot = ChatEvent::Snapshot {
        live_turn_id: None,
        turns,
        waiting_turn_ids,
    };
    if tx.send(Ok(sse_json(&snapshot))).await.is_err() {
        return;
    }

    // The worker may have been registered between the caller's lookup and its
    // subscribe; check once more before waiting on the channel.
    let worker = match workers.get(&user_id, &session_id) {
        Some(worker) => Some(worker),
        None => {
            wait_for_start(&workers, &mut starts, &user_id, &session_id, || {
                tx.is_closed()
            })
            .await
        }
    };
    let Some(worker) = worker else {
        // Either the wait expired, or the turn started *and finished* inside
        // it. A fresh snapshot covers the second case; `idle` closes the
        // stream for both, and the client re-attaches on its next interaction.
        match db::list_turns(&pool, &session_id).await {
            Ok(turns) => {
                let waiting_turn_ids = waiting_ids(&pool, &session_id).await;
                let _ = tx
                    .send(Ok(sse_json(&ChatEvent::Snapshot {
                        live_turn_id: None,
                        turns,
                        waiting_turn_ids,
                    })))
                    .await;
            }
            Err(err) => {
                tracing::warn!(error = %err, %session_id, "re-reading a conversation after the wait");
            }
        }
        let _ = tx.send(Ok(sse_json(&ChatEvent::Idle))).await;
        return;
    };
    // A second snapshot, and it is load-bearing: `live_turn_id` is the only
    // thing that tells the client a turn is streaming, and the snapshot it
    // already has says `null`. Without this the deltas arrive into a page that
    // still believes it is idle — no stop button, no interrupt, and no
    // reconnect if the stream drops.
    let initial = match db::list_turns(&pool, &session_id).await {
        Ok(turns) => vec![ChatEvent::Snapshot {
            live_turn_id: Some(worker.turn_id.clone()),
            turns,
            // Nothing is waiting any more: this turn is the one that was.
            waiting_turn_ids: Vec::new(),
        }],
        Err(err) => {
            tracing::warn!(error = %err, %session_id, "re-reading a conversation as its turn starts");
            let _ = tx.send(Ok(sse_json(&ChatEvent::Idle))).await;
            return;
        }
    };
    run_json_turn_stream(
        pool,
        session_id,
        worker.turn_id.clone(),
        worker.broadcast.subscribe(),
        initial,
        tx,
    )
    .await;
}

/// The conversation's waiting user turns, for a snapshot. An unreadable queue
/// degrades to "nothing waiting": the transcript is still correct, one message
/// just renders without its spinner until the next snapshot.
async fn waiting_ids(pool: &db::Pool, session_id: &str) -> Vec<String> {
    db::list_pending_for_session(pool, session_id)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|pending| pending.turn_id)
        .collect()
}

/// How long a stream waits for a waiting turn to start before ending as idle.
///
/// Long enough to cover an ordinary queue (the turn ahead of it finishing),
/// short enough that a congested gateway is not holding connections open for
/// hours.
const WAIT_FOR_START: Duration = Duration::from_secs(300);

/// How often the wait looks up to see whether the reader is still there.
const DISCONNECT_CHECK: Duration = Duration::from_secs(5);

/// Wait for a worker to be registered for this conversation.
///
/// The frame is only a wake-up: what it carries may already be stale by the
/// time it is read, so the registry is asked for the live handle — the one
/// with the broadcast channel this stream then tails. `None` means the wait
/// expired, or the turn was over before the handle could be taken.
async fn wait_for_start(
    workers: &crate::workers::SessionWorkers,
    starts: &mut broadcast::Receiver<crate::workers::WorkerStarted>,
    user_id: &str,
    session_id: &str,
    is_gone: impl Fn() -> bool,
) -> Option<crate::workers::ActiveWorker> {
    let deadline = tokio::time::Instant::now() + WAIT_FOR_START;
    loop {
        // Wake up regularly even when nothing is happening, so a viewer who
        // closed the tab is not held for the whole deadline.
        let next = (tokio::time::Instant::now() + DISCONNECT_CHECK).min(deadline);
        match tokio::time::timeout_at(next, starts.recv()).await {
            Ok(Ok(started)) => {
                if started.user_id == user_id && started.session_id == session_id {
                    return workers.get(user_id, session_id);
                }
            }
            // Lagged: starts were missed while this subscriber was slow, and
            // one of them may have been ours. Ask the registry rather than
            // waiting for the next frame.
            Ok(Err(broadcast::error::RecvError::Lagged(_))) => {
                if let Some(worker) = workers.get(user_id, session_id) {
                    return Some(worker);
                }
            }
            // The registry is gone: nothing will ever start.
            Ok(Err(broadcast::error::RecvError::Closed)) => return None,
            // Nothing happened in this slice. Give up at the deadline, or when
            // the reader has gone away.
            Err(_) => {
                if tokio::time::Instant::now() >= deadline || is_gone() {
                    return None;
                }
            }
        }
    }
}

/// An SSE response whose body is fed by `rx` — the JSON twin of the
/// streaming response constructor in [`crate::chat`]. EventSource on the
/// client reconnects automatically; each attach replays a fresh snapshot.
pub fn json_stream_response(
    rx: rama::futures::channel::mpsc::UnboundedReceiver<Result<rama::bytes::Bytes, std::io::Error>>,
) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header("x-accel-buffering", "no")
        .body(Body::from_stream(rx))
        .unwrap()
}

/// Sender end of the per-request SSE channel. The streaming task fills it;
/// the response body drains it.
pub type SseTx =
    rama::futures::channel::mpsc::UnboundedSender<Result<rama::bytes::Bytes, std::io::Error>>;

/// Flip the cancel flag on the active worker for this (user_id, session_id)
/// pair. Returns true if a worker was found and flagged, false if nothing was
/// running. Pure registry op — the auth check and the response shape live in
/// the handler.
pub fn cancel_turn(
    workers: &crate::workers::SessionWorkers,
    user_id: &str,
    session_id: &str,
) -> bool {
    let Some(worker) = workers.get(user_id, session_id) else {
        return false;
    };
    worker
        .cancel
        .store(true, std::sync::atomic::Ordering::SeqCst);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{ToolCall, ToolCallStatus, Turn, TurnRole};
    use jiff::Timestamp;

    fn turn(status: TurnStatus, content: &str, reasoning: &str) -> TurnWithTools {
        TurnWithTools {
            turn: Turn {
                id: "t1".into(),
                session_id: "s1".into(),
                seq: 2,
                role: TurnRole::Assistant,
                user_content: None,
                model: Some("m".into()),
                content: Some(content.into()),
                reasoning: if reasoning.is_empty() {
                    None
                } else {
                    Some(reasoning.into())
                },
                reasoning_elapsed_ms: None,
                reasoning_started_at: None,
                status,
                error_message: None,
                created_at: Timestamp::now(),
                completed_at: None,
            },
            tool_calls: Vec::new(),
            steers: Vec::new(),
        }
    }

    fn tool(id: &str, name: &str, status: ToolCallStatus, output: Option<&str>) -> ToolCall {
        ToolCall {
            id: id.into(),
            turn_id: "t1".into(),
            seq: 1,
            name: name.into(),
            arguments_json: "{}".into(),
            output_json: output.map(str::to_string),
            status,
            created_at: Timestamp::now(),
            completed_at: None,
        }
    }

    fn steer_row(id: &str, text: &str, status: crate::db::SteerStatus) -> crate::db::TurnSteer {
        crate::db::TurnSteer {
            id: id.into(),
            turn_id: "t1".into(),
            seq: 0,
            text: text.into(),
            status,
            created_at: Timestamp::now(),
            settled_at: None,
        }
    }

    /// An interjection is part of the conversation from the moment it is
    /// typed, so it goes out while still `pending` — waiting for the model to
    /// read it would leave the user staring at a composer that swallowed
    /// their sentence. The outcome then arrives as a second event on the same
    /// id, and nothing is re-sent in between.
    #[test]
    fn a_steer_is_announced_when_typed_and_again_when_it_settles() {
        use crate::db::SteerStatus;
        let mut feed = JsonTurnFeed::new("t1");
        let mut row = turn(TurnStatus::InProgress, "Hello", "");
        feed.diff(&row); // drain the content delta

        row.steers = vec![steer_row("n1", "in euros", SteerStatus::Pending)];
        assert_eq!(
            feed.diff(&row),
            vec![ChatEvent::Steer {
                turn_id: "t1".into(),
                id: "n1".into(),
                text: "in euros".into(),
                status: "pending".into(),
            }]
        );

        // Unchanged row: nothing repeats.
        assert!(feed.diff(&row).is_empty());

        row.steers = vec![steer_row("n1", "in euros", SteerStatus::Delivered)];
        assert_eq!(
            feed.diff(&row),
            vec![ChatEvent::Steer {
                turn_id: "t1".into(),
                id: "n1".into(),
                text: "in euros".into(),
                status: "delivered".into(),
            }]
        );
    }

    /// A client attaching after the fact gets the note from the snapshot, so
    /// a feed that starts mid-turn must still announce what it finds rather
    /// than assuming the client saw it.
    #[test]
    fn a_steer_already_settled_at_attach_is_announced_once() {
        use crate::db::SteerStatus;
        let mut feed = JsonTurnFeed::new("t1");
        let mut row = turn(TurnStatus::InProgress, "", "");
        row.steers = vec![steer_row("n1", "in euros", SteerStatus::Resent)];
        let first = feed.diff(&row);
        assert_eq!(first.len(), 1);
        assert!(matches!(
            &first[0],
            ChatEvent::Steer { status, .. } if status == "resent"
        ));
        assert!(feed.diff(&row).is_empty());
    }

    #[test]
    fn deltas_append_and_cursor_advances() {
        let mut feed = JsonTurnFeed::new("t1");
        let mut row = turn(TurnStatus::InProgress, "Hello", "think");
        assert_eq!(
            feed.diff(&row),
            vec![
                ChatEvent::TurnDelta {
                    turn_id: "t1".into(),
                    text_delta: "Hello".into(),
                    full: false
                },
                ChatEvent::ReasoningDelta {
                    turn_id: "t1".into(),
                    text_delta: "think".into(),
                    full: false
                },
            ]
        );
        // Nothing changed → nothing sent.
        assert!(feed.diff(&row).is_empty());

        row.turn.content = Some("Hello, world!".into());
        assert_eq!(
            feed.diff(&row),
            vec![ChatEvent::TurnDelta {
                turn_id: "t1".into(),
                text_delta: ", world!".into(),
                full: false
            }]
        );
    }

    #[test]
    fn a_shrink_resends_the_full_text() {
        let mut feed = JsonTurnFeed::new("t1");
        let mut row = turn(TurnStatus::InProgress, "long first draft", "");
        feed.diff(&row);
        row.turn.content = Some("short".into());
        assert_eq!(
            feed.diff(&row),
            vec![ChatEvent::TurnDelta {
                turn_id: "t1".into(),
                text_delta: "short".into(),
                full: true
            }],
            "a rewrite is flagged: the client replaces its buffer instead of appending"
        );
        // And growth continues normally from there.
        row.turn.content = Some("short + more".into());
        assert_eq!(
            feed.diff(&row),
            vec![ChatEvent::TurnDelta {
                turn_id: "t1".into(),
                text_delta: " + more".into(),
                full: false
            }]
        );
    }

    /// A rewrite that does not shrink is still a rewrite.
    ///
    /// This is the shape `typst_render` produces: it strips the markers it
    /// wrote earlier and appends the next render, so the content changes in
    /// place and ends up at least as long as before. A length-only cursor
    /// reads that as an append and tells the client to tack the tail onto a
    /// buffer whose head is now wrong — two renders in one turn left the
    /// stale first one on screen with a fragment of the second glued to it,
    /// healing only on reconnect.
    #[test]
    fn an_in_place_rewrite_that_grows_is_still_flagged_full() {
        let mut feed = JsonTurnFeed::new("t1");
        let mut row = turn(TurnStatus::InProgress, "[render-a] here it is", "");
        feed.diff(&row);
        // Same length class, different prefix — the marker was replaced.
        row.turn.content = Some("[render-b] here it is, and more".into());
        assert_eq!(
            feed.diff(&row),
            vec![ChatEvent::TurnDelta {
                turn_id: "t1".into(),
                text_delta: "[render-b] here it is, and more".into(),
                full: true
            }],
            "the new text does not start with what was already sent, so it \
             cannot be an append however much longer it got"
        );
    }

    /// Clearing the content has to reach the client too.
    #[test]
    fn a_rewrite_to_empty_tells_the_client_to_clear() {
        let mut feed = JsonTurnFeed::new("t1");
        let mut row = turn(TurnStatus::InProgress, "something", "");
        feed.diff(&row);
        row.turn.content = Some(String::new());
        assert_eq!(
            feed.diff(&row),
            vec![ChatEvent::TurnDelta {
                turn_id: "t1".into(),
                text_delta: String::new(),
                full: true
            }],
            "an empty `full` delta is the only way to say 'drop what you have'"
        );
    }

    #[test]
    fn tool_calls_announce_start_and_done_exactly_once() {
        let mut feed = JsonTurnFeed::new("t1");
        let mut row = turn(TurnStatus::InProgress, "", "");
        row.tool_calls = vec![tool("c1", "search", ToolCallStatus::Running, None)];
        assert!(matches!(
            feed.diff(&row)[0],
            ChatEvent::ToolCallStarted { ref tool_call_id, .. } if tool_call_id == "c1"
        ));
        assert!(feed.diff(&row).is_empty(), "no status change, no event");

        row.tool_calls[0].status = ToolCallStatus::Completed;
        row.tool_calls[0].output_json = Some(r#"{"ok":true}"#.into());
        assert_eq!(
            feed.diff(&row),
            vec![ChatEvent::ToolCallDone {
                turn_id: "t1".into(),
                tool_call_id: "c1".into(),
                status: "completed".into(),
                output: Some(r#"{"ok":true}"#.into()),
            }]
        );
        assert!(
            feed.diff(&row).is_empty(),
            "terminal status must not repeat"
        );
    }

    #[test]
    fn a_fast_tool_that_lands_between_ticks_announces_both_halves() {
        let mut feed = JsonTurnFeed::new("t1");
        let mut row = turn(TurnStatus::InProgress, "", "");
        row.tool_calls = vec![tool("c1", "time", ToolCallStatus::Completed, Some("12:00"))];
        let events = feed.diff(&row);
        assert!(matches!(events[0], ChatEvent::ToolCallStarted { .. }));
        assert!(matches!(events[1], ChatEvent::ToolCallDone { .. }));
    }

    #[test]
    fn finalization_emits_pending_deltas_then_the_finalized_event_once() {
        let mut feed = JsonTurnFeed::new("t1");
        let mut row = turn(TurnStatus::InProgress, "partial", "");
        feed.diff(&row);

        row.turn.status = TurnStatus::Completed;
        row.turn.content = Some("partial and done".into());
        row.turn.completed_at = Some(Timestamp::now());
        let events = feed.diff(&row);
        assert_eq!(events.len(), 2);
        assert!(
            matches!(&events[0], ChatEvent::TurnDelta { text_delta, full: false, .. } if text_delta == " and done")
        );
        let ChatEvent::TurnFinalized {
            status,
            duration_ms,
            ..
        } = &events[1]
        else {
            panic!("expected finalize");
        };
        assert_eq!(status, "completed");
        assert!(duration_ms.is_some());

        assert!(feed.diff(&row).is_empty(), "finalized feed is done");
        assert!(feed.is_finalized());
    }

    #[test]
    fn an_errored_finalization_carries_the_message() {
        let mut feed = JsonTurnFeed::new("t1");
        let mut row = turn(TurnStatus::Errored, "", "");
        row.turn.error_message = Some("upstream 502".into());
        let ChatEvent::TurnFinalized { error_message, .. } = &feed.diff(&row)[0] else {
            panic!("expected finalize");
        };
        assert_eq!(error_message.as_deref(), Some("upstream 502"));
    }

    #[test]
    fn a_feed_only_speaks_for_its_own_turn() {
        let mut feed = JsonTurnFeed::new("t1");
        let mut other = turn(TurnStatus::InProgress, "hello", "");
        other.turn.id = "other".into();
        assert!(feed.diff(&other).is_empty());
    }

    #[test]
    fn the_sse_frame_carries_the_event_name_and_one_data_line() {
        let frame = sse_json(&ChatEvent::Info {
            message: "hi".into(),
        });
        let text = String::from_utf8_lossy(&frame);
        assert_eq!(
            text,
            "event: info\ndata: {\"type\":\"info\",\"message\":\"hi\"}\n\n"
        );
    }

    #[test]
    fn multi_line_json_stays_a_single_data_line() {
        // Newlines inside values are escaped by serde; the SSE framing must
        // never break a payload across `data:` lines.
        let frame = sse_json(&ChatEvent::Info {
            message: "line1\nline2".into(),
        });
        assert_eq!(frame.iter().filter(|b| **b == b'\n').count(), 3);
    }
}
