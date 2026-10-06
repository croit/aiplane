// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Suspended agent runs over HTTP (`docs/agent-hil.md` → "Suspend and resume"):
//! a visitor answering a secure input on `/api/v0/embed/resume`, an approval
//! only staff may give on `/api/v0/agents/{id}/conversations/…/resume`, a
//! message queued behind a pending decision, and the test chat's pause.
//!
//! The production runner drives each turn on a scripted wiremock upstream.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rama::http::{Method, StatusCode};
use serde_json::{Value, json};
use sqlx::Row;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::{Embed, SITE, code, frames, new_key};
use crate::agents::{self, TIME};
use crate::common;

use aiplane_runtime::agents::embed::LiveAgentRunner;
use aiplane_runtime::server::tools::ToolRegistry;
use aiplane_runtime::server::tools::ask_first::AskFirst;
use aiplane_runtime::server::tools::check_code::{CHECK_CODE, CheckCode};
use aiplane_runtime::server::tools::echo::Echo;
use aiplane_runtime::server::tools::time::CurrentTimestamp;
use session_core::db as chat;

const CODE: &str = "481516";
const ECHO: &str = "company_echo";

/// A chat upstream answering each round with the next scripted delta.
struct Scripted {
    deltas: Vec<Value>,
    served: AtomicUsize,
}

impl wiremock::Respond for Scripted {
    fn respond(&self, _req: &wiremock::Request) -> ResponseTemplate {
        let i = self.served.fetch_add(1, Ordering::SeqCst);
        let delta = &self.deltas[i.min(self.deltas.len() - 1)];
        let sse = format!(
            "data: {}\n\ndata: [DONE]\n\n",
            json!({"choices": [{"index": 0, "delta": delta}]})
        );
        ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream")
    }
}

async fn upstream(deltas: Vec<Value>) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(Scripted {
            deltas,
            served: AtomicUsize::new(0),
        })
        .mount(&server)
        .await;
    server
}

fn call(id: &str, name: &str, args: Value) -> Value {
    json!({"tool_calls": [{"index": 0, "id": id, "type": "function",
        "function": {"name": name, "arguments": args.to_string()}}]})
}

fn text(s: &str) -> Value {
    json!({ "content": s })
}

async fn sent(server: &MockServer) -> Vec<Value> {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| serde_json::from_slice(&r.body).unwrap())
        .collect()
}

fn spec_with(tools: &[&str]) -> Value {
    json!({ "main": {
        "model": "m",
        "instructions": { "orchestration": "Help the visitor." },
        "tools": tools
    } })
}

/// A published agent whose tools pause: `check_code` asks the visitor for
/// [`CODE`], `company_echo` waits for an approval. Served by the production
/// runner on `llm`.
async fn gated(llm: &MockServer) -> Embed {
    let hour = Duration::from_secs(3600);
    let tools = ToolRegistry::new()
        .with(CurrentTimestamp)
        .with(AskFirst::new(Echo, hour))
        .with(CheckCode::new(CODE, hour));
    let mut fx = agents::fixture_with(Some(&llm.uri()), tools, &[TIME, CHECK_CODE, ECHO]).await;
    fx.state = fx
        .state
        .clone()
        .with_agent_runner(Arc::new(LiveAgentRunner));
    let agent = fx.runnable("support").await;
    for tool in [CHECK_CODE, ECHO] {
        assert_eq!(
            fx.grant(&fx.alice, &agent, "tool", tool).await,
            StatusCode::CREATED
        );
    }
    let (status, body) = fx
        .put_draft(&fx.alice, &agent, spec_with(&[CHECK_CODE, ECHO]))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = fx.publish(&fx.alice, &agent).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (key_id, key) = new_key(&fx, &agent, &[SITE]).await;
    Embed {
        fx,
        agent,
        key_id,
        key,
        runner: Arc::default(),
    }
}

impl Embed {
    async fn last_turn(&self, token: &str) -> chat::Turn {
        let session = self.conversation_of(token).await;
        let turns = chat::list_turns(&self.fx.state.db, &session).await.unwrap();
        turns.last().unwrap().turn.clone()
    }

    /// Wait until the conversation's latest turn is no longer running and no
    /// runner holds the conversation.
    ///
    /// The claim is looked at before the turn is read: a run finishes its row
    /// and only then lets the output filter overwrite it, releasing the claim
    /// last. A row read after a free claim is the delivered one; read the
    /// other way round, it can be the unfiltered answer of a run whose claim
    /// dropped in between.
    pub(super) async fn settled(&self, token: &str) -> chat::Turn {
        let session = self.conversation_of(token).await;
        for _ in 0..500 {
            let held = self.fx.state.chats.get(&self.agent, &session).is_some();
            let t = self.last_turn(token).await;
            if !held
                && t.status != chat::TurnStatus::InProgress
                && t.role == chat::TurnRole::Assistant
            {
                return t;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the conversation never settled");
    }

    pub(super) async fn event_frames(&self, token: &str) -> Vec<(String, Value)> {
        let resp = self
            .raw(
                Method::GET,
                "/api/v0/embed/events",
                Some(SITE),
                Some(token),
                None,
            )
            .await;
        let body = tokio::time::timeout(Duration::from_secs(20), common::read_body(resp))
            .await
            .expect("the stream ends");
        frames(&body)
    }

    pub(super) async fn answer(&self, token: &str, body: Value) -> super::Reply {
        self.send(
            Method::POST,
            "/api/v0/embed/resume",
            Some(SITE),
            Some(token),
            Some(body),
        )
        .await
    }

    fn staff_resume_uri(&self, session: &str, turn: &str) -> String {
        format!(
            "/api/v0/agents/{}/conversations/{session}/turns/{turn}/resume",
            self.agent
        )
    }
}

/// Every text value in every table, FTS shadow tables included.
pub(super) async fn every_stored_text(db: &aiplane_core::server::db::Pool) -> String {
    let tables: Vec<String> =
        sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type = 'table'")
            .fetch_all(db)
            .await
            .unwrap();
    let mut all = String::new();
    for table in tables {
        let rows = sqlx::query(&format!("SELECT * FROM \"{table}\""))
            .fetch_all(db)
            .await
            .unwrap_or_default();
        for row in rows {
            for i in 0..row.len() {
                if let Ok(Some(text)) = row.try_get::<Option<String>, _>(i) {
                    all.push_str(&text);
                    all.push('\n');
                }
            }
        }
    }
    all
}

/// Every log line written while the guard lives, at every level.
#[derive(Clone, Default)]
pub(super) struct Logs(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Logs {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Logs {
    pub(super) fn capture(&self) -> tracing::subscriber::DefaultGuard {
        let sink = self.clone();
        // Everything the production filter lets through at `RUST_LOG=trace`.
        let subscriber = tracing_subscriber::fmt()
            .with_env_filter(aiplane::logging::filter(
                tracing_subscriber::EnvFilter::new("trace"),
            ))
            .with_ansi(false)
            .with_writer(move || sink.clone())
            .finish();
        tracing::subscriber::set_default(subscriber)
    }

    pub(super) fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}

#[tokio::test]
async fn a_visitors_secure_input_reaches_the_tool_and_is_stored_or_logged_nowhere() {
    let logs = Logs::default();
    let _guard = logs.capture();
    let llm = upstream(vec![
        call("k1", CHECK_CODE, json!({})),
        text("Thanks, you are verified."),
    ])
    .await;
    let e = gated(&llm).await;
    let token = e.visitor().await;
    let r = e.say(&token, "It's me, Alice.").await;
    assert_eq!(r.status, StatusCode::ACCEPTED, "{}", r.body);
    assert_eq!(r.body["placement"], "started");
    let paused = e.settled(&token).await;
    assert_eq!(paused.status, chat::TurnStatus::Suspended);

    let frames = e.event_frames(&token).await;
    let names: Vec<&str> = frames.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["snapshot", "suspended"], "{frames:?}");
    let frame = &frames[1].1;
    assert_eq!(frame["turn_id"], paused.id.as_str());
    assert_eq!(frame["kind"], "secure_input");
    assert_eq!(frame["message"], "Enter the code we sent you.");
    assert_eq!(frame["options"], json!(["value", "deny"]));
    assert!(frame["expires_at"].is_string());
    let request_id = frame["request_id"].as_str().unwrap().to_string();
    for internal in ["tool", "tool_call_id"] {
        assert!(frame.get(internal).is_none(), "no `{internal}`: {frame}");
    }
    let snapshot = &frames[0].1["turns"][1];
    assert_eq!(snapshot["suspension"]["request_id"], request_id.as_str());
    assert!(snapshot["suspension"].get("tool").is_none());
    let session = e.resume(&token).await;
    assert_eq!(
        session.body["turns"][1]["suspension"]["kind"],
        "secure_input"
    );

    let stale = e
        .answer(
            &token,
            json!({"request_id": "req-old", "decision": "value", "value": "1"}),
        )
        .await;
    assert_eq!(stale.status, StatusCode::CONFLICT, "{}", stale.body);
    assert_eq!(code(&stale), "not_suspended");
    let wrong = e
        .answer(
            &token,
            json!({"request_id": request_id, "decision": "allow_once"}),
        )
        .await;
    assert_eq!(wrong.status, StatusCode::BAD_REQUEST, "{}", wrong.body);
    assert_eq!(code(&wrong), "decision_not_offered");

    let r = e
        .answer(
            &token,
            json!({"request_id": request_id, "decision": "value", "value": CODE}),
        )
        .await;
    assert_eq!(r.status, StatusCode::ACCEPTED, "{}", r.body);
    assert_eq!(r.body["turn_id"], paused.id.as_str());
    assert!(!r.body.to_string().contains(CODE));
    let done = e.settled(&token).await;
    assert_eq!(done.id, paused.id);
    assert_eq!(done.status, chat::TurnStatus::Completed);
    let frames = e.event_frames(&token).await;
    assert_eq!(
        frames[0].1["turns"][1]["turn"]["content"],
        "Thanks, you are verified."
    );

    let requests = sent(&llm).await;
    assert_eq!(requests.len(), 2);
    assert!(
        requests[1].to_string().contains("verified\\\": true"),
        "the tool got the code and the model its verdict: {}",
        requests[1]
    );
    for request in &requests {
        assert!(!request.to_string().contains(CODE), "never to the model");
    }
    assert!(
        !every_stored_text(&e.fx.state.db).await.contains(CODE),
        "never in the transcript, the tool rows, the audit trail or anywhere else stored"
    );
    let logged = logs.text();
    assert!(
        logged.contains("resuming a suspended agent run"),
        "the capture sees the resume path's own lines"
    );
    assert!(!logged.contains(CODE), "never in a log line");
}

#[tokio::test]
async fn a_visitor_cannot_give_an_approval_and_staff_can() {
    let llm = upstream(vec![
        call("e1", ECHO, json!({"message": "refund RE-1"})),
        text("Approved and done."),
    ])
    .await;
    let e = gated(&llm).await;
    let token = e.visitor().await;
    e.say(&token, "Refund RE-1 please.").await;
    let paused = e.settled(&token).await;
    assert_eq!(paused.status, chat::TurnStatus::Suspended);

    let frames = e.event_frames(&token).await;
    let frame = &frames.last().unwrap().1;
    assert_eq!(frame["kind"], "approval");
    assert_eq!(
        frame["options"],
        json!([]),
        "nothing for the visitor to decide"
    );
    let request_id = frame["request_id"].as_str().unwrap().to_string();
    let r = e
        .answer(
            &token,
            json!({"request_id": request_id, "decision": "allow_once"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{}", r.body);
    assert_eq!(code(&r), "decision_for_staff");
    assert_eq!(
        e.last_turn(&token).await.status,
        chat::TurnStatus::Suspended,
        "still waiting"
    );

    let session = e.conversation_of(&token).await;
    let uri = e.staff_resume_uri(&session, &paused.id);
    let body = json!({"decision": "allow_once", "request_id": request_id});
    let fx = &e.fx;
    let (status, _) = fx.send(None, Method::POST, &uri, Some(body.clone())).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = fx.post(&fx.plain, &uri, body.clone()).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "not an agent manager");
    let (status, _) = fx.post(&fx.bob, &uri, body.clone()).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "no share, no agent");
    let (status, reply) = fx.post(&fx.alice, &uri, body.clone()).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{reply}");
    assert_eq!(
        reply,
        json!({ "turn_id": paused.id }),
        "the turn continues on its stream"
    );
    let done = e.settled(&token).await;
    assert_eq!(done.id, paused.id);
    assert_eq!(done.status, chat::TurnStatus::Completed);
    assert_eq!(done.content.as_deref(), Some("Approved and done."));
    let (status, again) = fx.post(&fx.alice, &uri, body).await;
    assert_eq!(status, StatusCode::CONFLICT, "{again}");
    assert_eq!(again["error"]["code"], "not_suspended");

    assert!(
        sent(&llm).await[1].to_string().contains("refund RE-1"),
        "the approved call ran"
    );
    let frames = e.event_frames(&token).await;
    assert_eq!(
        frames[0].1["turns"][1]["turn"]["content"],
        "Approved and done."
    );
    let audit = e.fx.audit_kinds(&e.agent).await;
    assert!(audit.contains(&"run_resumed".to_string()), "{audit:?}");
}

#[tokio::test]
async fn a_message_sent_while_a_decision_is_pending_runs_after_it() {
    let llm = upstream(vec![
        call("e1", ECHO, json!({"message": "x"})),
        text("Done."),
        text("And here is the second answer."),
    ])
    .await;
    let e = gated(&llm).await;
    let token = e.visitor().await;
    e.say(&token, "Ship it.").await;
    let paused = e.settled(&token).await;

    let queued = e.say(&token, "Also, where is my order?").await;
    assert_eq!(queued.status, StatusCode::ACCEPTED, "{}", queued.body);
    assert_eq!(queued.body["placement"], "queued");
    assert_eq!(queued.body["turn_id"], Value::Null);
    let user_turn = queued.body["user_turn_id"].as_str().unwrap().to_string();
    let second = e.say(&token, "Hello?").await;
    assert_eq!(second.status, StatusCode::CONFLICT, "{}", second.body);
    let frames = e.event_frames(&token).await;
    assert_eq!(frames[0].1["waiting_turn_ids"], json!([user_turn]));
    assert_eq!(frames.last().unwrap().0, "suspended");

    let session = e.conversation_of(&token).await;
    let (status, reply) =
        e.fx.post(
            &e.fx.alice,
            &e.staff_resume_uri(&session, &paused.id),
            json!({"decision": "deny"}),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{reply}");
    e.settled(&token).await;
    let turns = chat::list_turns(&e.fx.state.db, &session).await.unwrap();
    let texts: Vec<(chat::TurnRole, Option<String>)> = turns
        .iter()
        .map(|t| {
            (
                t.turn.role,
                t.turn.content.clone().or(t.turn.user_content.clone()),
            )
        })
        .collect();
    assert_eq!(turns.len(), 4, "{texts:?}");
    assert_eq!(turns[1].turn.content.as_deref(), Some("Done."));
    assert_eq!(turns[2].turn.id, user_turn);
    assert_eq!(turns[3].turn.status, chat::TurnStatus::Completed);
    assert_eq!(
        turns[3].turn.content.as_deref(),
        Some("And here is the second answer.")
    );
}

#[tokio::test]
async fn the_test_chat_shows_its_manager_a_pause_they_can_answer() {
    let llm = upstream(vec![
        call("k1", CHECK_CODE, json!({})),
        text("Verified in the test chat."),
    ])
    .await;
    let e = gated(&llm).await;
    let fx = &e.fx;
    let (status, paused) = crate::common::test_chat_turn(
        &fx.state,
        &fx.alice,
        &e.agent,
        json!({ "message": "test me" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{paused}");
    assert_eq!(paused["status"], "suspended");
    assert_eq!(paused["answer"], Value::Null);
    assert_eq!(paused["suspension"]["kind"], "secure_input");
    assert_eq!(
        paused["suspension"]["tool"], CHECK_CODE,
        "a manager sees the tool"
    );
    let session = paused["session_id"].as_str().unwrap();
    let turn = paused["turn_id"].as_str().unwrap();

    let (status, refused) = fx
        .post(
            &fx.alice,
            &format!("/api/v0/agents/{}/test/messages", e.agent),
            json!({ "message": "next", "session_id": session }),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["error"]["code"], "decision_pending");

    let (status, resumed) = fx
        .post(
            &fx.alice,
            &e.staff_resume_uri(session, turn),
            json!({ "decision": "value", "value": CODE }),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{resumed}");
    let done =
        crate::common::test_chat_settled(&fx.state, &fx.alice, &e.agent, session, turn).await;
    assert_eq!(done["status"], "completed");
    assert_eq!(done["answer"], "Verified in the test chat.");
    assert!(
        done["debug"]["slots"].is_array() && done["debug"]["routes"].is_array(),
        "the resumed test turn has its debug view: {done}"
    );
    assert!(!every_stored_text(&fx.state.db).await.contains(CODE));
}
