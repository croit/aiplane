// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The JSON event protocol for live chat streaming (issue #22, phase 2).
//!
//! The legacy wire ships server-rendered HTML diffs (`datastar-patch-elements`
//! frames produced by [`crate::render::TurnStream`]); this module is its
//! data-only counterpart. Same subscription model — the worker's
//! [`TurnUpdate`] broadcast, DB as the single source of truth, coalesced
//! flushes — but each flush computes *what changed since this subscriber last
//! looked* and emits it as structured JSON events:
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
//! A client that reconnects just re-attaches: the initial [`ChatEvent::Snapshot`]
//! rebuilt from the DB subsumes anything missed, exactly like the legacy tail.
//! There is no `Last-Event-ID` replay because the DB *is* the replayer.
//!
//! Why a sibling of the HTML loop rather than a rewrite: the legacy pages
//! stay alive through migration phases 2–6 (issue #22), so both wires run
//! off the same broadcast until the HTML one is deleted.

use std::time::Duration;

use rama::http::{Body, Response, StatusCode, header};
use serde::Serialize;
use tokio::sync::broadcast;

use crate::db::{self, TurnStatus, TurnWithTools};
use crate::workers::{ToolPromptEvent, TurnUpdate};

/// Same coalescing contract as the HTML loop ([`crate::chat`]): cap the event
/// rate without visibly changing liveness — the trailing flush guarantees the
/// final state always lands.
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
    /// Bytes of `content` already emitted as `turn_delta`s.
    content_sent: usize,
    /// Bytes of `reasoning` already emitted.
    reasoning_sent: usize,
    /// Tool calls already announced, with the status last sent. Insertion
    /// order matches the row order (seq), so `started` events replay in the
    /// order the model issued them.
    tools_sent: Vec<(String, String)>,
    /// Whether [`ChatEvent::TurnFinalized`] has been emitted. The feed is
    /// done afterwards; the stream loop closes on it.
    finalized: bool,
}

impl JsonTurnFeed {
    pub fn new(assistant_turn_id: &str) -> Self {
        Self {
            assistant_turn_id: assistant_turn_id.to_string(),
            content_sent: 0,
            reasoning_sent: 0,
            tools_sent: Vec::new(),
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

/// Cursor-based append: emit a delta for everything past `*sent`. A shrink
/// (or a non-prefix rewrite — a hand-edited row, a retry that reused the id)
/// cannot be expressed as an append, so the cursor resets and the full text
/// is re-sent as one delta; the client's per-turn buffer then converges on
/// exactly the DB's content.
fn emit_append(
    events: &mut Vec<ChatEvent>,
    turn_id: &str,
    text: &str,
    sent: &mut usize,
    make: impl Fn(String, String, bool) -> ChatEvent,
) {
    let reset = if text.get(*sent..).is_some() {
        // Ordinary append (or nothing new).
        text.len() < *sent
    } else {
        // Cursor stranded mid-char by a rewrite: resend everything.
        true
    };
    if reset {
        *sent = 0;
    }
    let delta = match text.get(*sent..) {
        Some(delta) if !delta.is_empty() => delta,
        _ => return,
    };
    events.push(make(turn_id.to_string(), delta.to_string(), reset));
    *sent = text.len();
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
    tx: crate::chat::SseTx,
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
        let deadline = last_emit.map_or_else(tokio::time::Instant::now, |last| last + EVENT_COALESCE);
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
                // The datastar-only wire; JSON subscribers get the structured
                // `Prompt` twin instead.
                Ok(TurnUpdate::Inject(_)) => {}
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
    tx: &mut crate::chat::SseTx,
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
