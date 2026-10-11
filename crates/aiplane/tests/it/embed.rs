// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The public agent endpoint end to end (`docs/agent-visitors.md`):
//! embed keys managed under `/api/v0/agents/{id}/embed-keys`, and anonymous
//! visitors on `/api/v0/embed/*` with a `gwv_` token, an origin allowlist,
//! a sliding idle TTL and buffered answers.
//!
//! Built on the agent fixture of `agents.rs`: `alice` manages the agents,
//! `bob` is a manager without a share.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use rama::Service;
use rama::http::{Body, HeaderMap, Method, Request, StatusCode, header};
use serde_json::{Value, json};
use tokio::sync::Notify;

use crate::agents::{self, Fx, spec};
use crate::common;

use aiplane_agents::db::run_sessions;
use aiplane_core::server::crypto::sha256_hex;
use aiplane_runtime::agents::embed::{AgentTurnRunner, LiveAgentRunner, OpenedTurn};
use aiplane_runtime::rama_server::state::RamaState;
use session_core::db as chat;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SITE: &str = "https://www.example.com";
const OTHER_SITE: &str = "https://evil.example";
const ANSWER: &str = "Hello visitor, how can I help?";
const PARTIAL: &str = "Let me check the ord";

// Visitor limits, owner budget and retention (`docs/agent-visitors.md` → "Rates").
mod limits;
// The body cap on the public embed routes.
mod body_cap;
// Suspended agent runs: secure input, approvals, resume.
mod suspend;
// Human in the loop: the inbox, responders, channels (`docs/agent-hil.md`).
mod hil;
// Identity verifiers: a one-time code through the agent's connector, a
// website's signed identity token.
mod verifiers;
// Voice in the widget: transcription and spoken answers.
mod voice;

/// Stands in for the agent turn runner, which the router
/// (`docs/agent-runs.md` → "The router") builds on the real driver. The endpoint's contract with it is only "drive
/// the opened assistant turn to a terminal status", so this double writes a
/// partial answer, optionally waits for the test to let it go, then
/// completes the turn — enough to observe buffering, the in-progress guard
/// and resume without an upstream.
#[derive(Default)]
struct ScriptedRunner {
    hold: Option<Arc<Notify>>,
    finish: bool,
    seen: Mutex<Vec<OpenedTurn>>,
}

impl ScriptedRunner {
    fn answering() -> Self {
        Self {
            finish: true,
            ..Default::default()
        }
    }

    fn held(gate: Arc<Notify>) -> Self {
        Self {
            hold: Some(gate),
            finish: true,
            ..Default::default()
        }
    }
}

#[async_trait::async_trait]
impl AgentTurnRunner for ScriptedRunner {
    async fn run(&self, state: Arc<RamaState>, turn: OpenedTurn) {
        self.seen.lock().unwrap().push(turn.clone());
        chat::set_content(&state.db, &turn.turn_id, PARTIAL)
            .await
            .unwrap();
        if let Some(gate) = &self.hold {
            gate.notified().await;
        }
        if self.finish {
            chat::set_content(&state.db, &turn.turn_id, ANSWER)
                .await
                .unwrap();
            chat::finalize_turn(&state.db, &turn.turn_id, chat::TurnStatus::Completed, None)
                .await
                .unwrap();
        }
    }
}

struct Embed {
    fx: Fx,
    agent: String,
    key_id: String,
    key: String,
    runner: Arc<ScriptedRunner>,
}

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Value,
}

impl Embed {
    async fn send(
        &self,
        method: Method,
        uri: &str,
        origin: Option<&str>,
        bearer: Option<&str>,
        body: Option<Value>,
    ) -> Reply {
        let resp = self.raw(method, uri, origin, bearer, body).await;
        let status = resp.status();
        let headers = resp.headers().clone();
        let bytes = common::read_body(resp).await;
        Reply {
            status,
            headers,
            body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        }
    }

    async fn raw(
        &self,
        method: Method,
        uri: &str,
        origin: Option<&str>,
        bearer: Option<&str>,
        body: Option<Value>,
    ) -> rama::http::Response {
        let mut req = Request::builder().method(method).uri(uri);
        if let Some(o) = origin {
            req = req.header(header::ORIGIN, o);
        }
        if let Some(b) = bearer {
            req = req.header(header::AUTHORIZATION, format!("Bearer {b}"));
        }
        let req = match body {
            Some(b) => req
                .header("content-type", "application/json")
                .body(Body::from(b.to_string())),
            None => req.body(Body::empty()),
        }
        .unwrap();
        common::app(self.fx.state.clone()).serve(req).await.unwrap()
    }

    async fn start_with(&self, key: &str, origin: Option<&str>) -> Reply {
        self.send(
            Method::POST,
            "/api/v0/embed/sessions",
            origin,
            None,
            Some(json!({ "key": key })),
        )
        .await
    }

    /// A visitor token for a fresh conversation from [`SITE`].
    async fn visitor(&self) -> String {
        let r = self.start_with(&self.key, Some(SITE)).await;
        assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
        r.body["token"].as_str().unwrap().to_string()
    }

    async fn say(&self, token: &str, text: &str) -> Reply {
        self.send(
            Method::POST,
            "/api/v0/embed/messages",
            Some(SITE),
            Some(token),
            Some(json!({ "text": text })),
        )
        .await
    }

    async fn say_in(&self, token: &str, text: &str, accept_language: &str) -> Reply {
        let req = Request::builder()
            .method(Method::POST)
            .uri("/api/v0/embed/messages")
            .header(header::ORIGIN, SITE)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::ACCEPT_LANGUAGE, accept_language)
            .header("content-type", "application/json")
            .body(Body::from(json!({ "text": text }).to_string()))
            .unwrap();
        let resp = common::app(self.fx.state.clone()).serve(req).await.unwrap();
        let status = resp.status();
        let headers = resp.headers().clone();
        let bytes = common::read_body(resp).await;
        Reply {
            status,
            headers,
            body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        }
    }

    async fn resume(&self, token: &str) -> Reply {
        self.send(
            Method::GET,
            "/api/v0/embed/session",
            Some(SITE),
            Some(token),
            None,
        )
        .await
    }

    async fn conversation_of(&self, token: &str) -> String {
        let hash = sha256_hex(token.as_bytes());
        sqlx::query_scalar("SELECT session_id FROM visitor_sessions WHERE token_hash = ?")
            .bind(hash)
            .fetch_one(&self.fx.state.db)
            .await
            .unwrap()
    }

    async fn wait_terminal(&self, session_id: &str, turn_id: &str) -> chat::Turn {
        for _ in 0..200 {
            let t = chat::get_turn(&self.fx.state.db, session_id, turn_id)
                .await
                .unwrap()
                .unwrap();
            if t.status != chat::TurnStatus::InProgress {
                return t;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("turn {turn_id} never finished");
    }
}

/// A published agent with one embed key for [`SITE`], served by `runner`
/// (no runner at all when `None`).
async fn embed_with(runner: Option<ScriptedRunner>, extra_spec: Option<Value>) -> Embed {
    let mut fx = agents::fixture().await;
    let installed = runner.is_some();
    let runner = Arc::new(runner.unwrap_or_default());
    if installed {
        fx.state = fx.state.clone().with_agent_runner(runner.clone());
    }
    fx.state = fx.state.clone().with_trusted_proxies(
        aiplane_core::server::trusted_proxies::TrustedProxies::parse("10.0.0.0/8").unwrap(),
    );
    published(fx, runner, extra_spec).await
}

/// The same, served end to end: the production runner on a pool whose
/// backend is `upstream`.
async fn embed_live(upstream: &MockServer, extra_spec: Option<Value>) -> Embed {
    let mut fx = agents::fixture_on(Some(&upstream.uri())).await;
    fx.state = fx
        .state
        .clone()
        .with_agent_runner(Arc::new(LiveAgentRunner));
    published(fx, Arc::default(), extra_spec).await
}

async fn published(fx: Fx, runner: Arc<ScriptedRunner>, extra_spec: Option<Value>) -> Embed {
    let agent = fx.runnable("support").await;
    if let Some(extra) = extra_spec {
        let mut s = spec("v1");
        for (k, v) in extra.as_object().unwrap() {
            s[k] = v.clone();
        }
        let (status, body) = fx.put_draft(&fx.alice, &agent, s).await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    let (status, body) = fx.publish(&fx.alice, &agent).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (key_id, key) = new_key(&fx, &agent, &[SITE]).await;
    Embed {
        fx,
        agent,
        key_id,
        key,
        runner,
    }
}

async fn embed() -> Embed {
    embed_with(Some(ScriptedRunner::answering()), None).await
}

async fn new_key(fx: &Fx, agent: &str, origins: &[&str]) -> (String, String) {
    let (status, body) = fx
        .post(
            &fx.alice,
            &format!("/api/v0/agents/{agent}/embed-keys"),
            json!({ "name": "website", "origins": origins }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    (
        body["embed_key"]["id"].as_str().unwrap().to_string(),
        body["key"].as_str().unwrap().to_string(),
    )
}

fn code(r: &Reply) -> &str {
    r.body["error"]["code"].as_str().unwrap_or("")
}

/// `(event, data)` for every frame of an SSE body.
fn frames(body: &[u8]) -> Vec<(String, Value)> {
    String::from_utf8_lossy(body)
        .split("\n\n")
        .filter_map(|block| {
            let mut event = None;
            let mut data = None;
            for line in block.lines() {
                if let Some(e) = line.strip_prefix("event: ") {
                    event = Some(e.to_string());
                } else if let Some(d) = line.strip_prefix("data: ") {
                    data = serde_json::from_str(d).ok();
                }
            }
            Some((event?, data?))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Embed-key management

#[tokio::test]
async fn embed_keys_need_a_manager_with_a_share() {
    let e = embed().await;
    let fx = &e.fx;
    let uri = format!("/api/v0/agents/{}/embed-keys", e.agent);
    let body = json!({ "name": "x", "origins": [SITE] });

    let (status, _) = fx.send(None, Method::GET, &uri, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = fx.post(&fx.plain, &uri, body.clone()).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = fx.get(&fx.bob, &uri).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "no share, no agent");

    let (status, _) = fx.share(&fx.alice, &e.agent, "user", "bob", "read").await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, listed) = fx.get(&fx.bob, &uri).await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    let (status, refused) = fx.post(&fx.bob, &uri, body.clone()).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{refused}");
    let revoke = format!("{uri}/{}/revoke", e.key_id);
    let (status, _) = fx.post(&fx.bob, &revoke, json!({})).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = fx.post(&fx.root, &uri, body).await;
    assert_eq!(status, StatusCode::CREATED, "an admin needs no share");
}

#[tokio::test]
async fn a_key_is_shown_once_and_listed_without_it() {
    let e = embed().await;
    assert!(e.key.starts_with("gwe_"), "{}", e.key);
    let (status, body) =
        e.fx.get(
            &e.fx.alice,
            &format!("/api/v0/agents/{}/embed-keys", e.agent),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let keys = body["embed_keys"].as_array().unwrap();
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0]["id"], e.key_id.as_str());
    assert_eq!(keys[0]["origins"], json!([SITE]));
    assert_eq!(keys[0]["revoked_at"], Value::Null);
    assert!(
        !body.to_string().contains(&e.key),
        "the key appears only in the create response"
    );
}

#[tokio::test]
async fn a_key_needs_a_name_and_exact_origins() {
    let e = embed().await;
    let uri = format!("/api/v0/agents/{}/embed-keys", e.agent);
    for body in [
        json!({ "name": "x", "origins": ["https://www.example.com/"] }),
        json!({ "name": "x", "origins": ["*"] }),
        json!({ "name": "x", "origins": [] }),
        json!({ "name": " ", "origins": [SITE] }),
        json!({ "name": "x", "origins": [SITE], "key": "gwe_mine" }),
    ] {
        let (status, reply) = e.fx.post(&e.fx.alice, &uri, body.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body} → {reply}");
    }
}

#[tokio::test]
async fn revoking_a_key_is_audited_and_happens_once() {
    let e = embed().await;
    let revoke = format!("/api/v0/agents/{}/embed-keys/{}/revoke", e.agent, e.key_id);
    let (status, _) = e.fx.post(&e.fx.alice, &revoke, json!({})).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, body) = e.fx.post(&e.fx.alice, &revoke, json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let kinds = e.fx.audit_kinds(&e.agent).await;
    assert!(
        kinds.contains(&"embed_key_created".to_string()),
        "{kinds:?}"
    );
    assert_eq!(kinds.last().unwrap(), "embed_key_revoked");
}

// ---------------------------------------------------------------------------
// Starting a visitor session

#[tokio::test]
async fn a_visitor_session_starts_from_an_allowed_origin() {
    let e = embed().await;
    let r = e.start_with(&e.key, Some(SITE)).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    assert!(r.body["token"].as_str().unwrap().starts_with("gwv_"));
    assert_eq!(r.body["idle_ttl_secs"], 1800, "30 minutes by default");
    assert_eq!(r.body["agent"]["display"], "support");
    assert_eq!(
        r.headers.get(header::ACCESS_CONTROL_ALLOW_ORIGIN).unwrap(),
        SITE
    );
}

#[tokio::test]
async fn the_idle_ttl_comes_from_the_live_versions_publish_settings() {
    let e = embed_with(
        Some(ScriptedRunner::answering()),
        Some(json!({ "publish": { "idle_ttl": "45m" } })),
    )
    .await;
    let r = e.start_with(&e.key, Some(SITE)).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    assert_eq!(r.body["idle_ttl_secs"], 2700);
}

#[tokio::test]
async fn a_wrong_or_missing_origin_is_refused() {
    let e = embed().await;
    let r = e.start_with(&e.key, Some(OTHER_SITE)).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert_eq!(code(&r), "origin_not_allowed");
    assert!(
        r.body["error"]["message"]
            .as_str()
            .unwrap()
            .contains(OTHER_SITE)
    );
    assert!(
        r.headers.get(header::ACCESS_CONTROL_ALLOW_ORIGIN).is_none(),
        "a non-allowlisted origin is never reflected"
    );
    let r = e.start_with(&e.key, None).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert_eq!(code(&r), "origin_not_allowed");

    let token = e.visitor().await;
    let r = e
        .send(
            Method::GET,
            "/api/v0/embed/session",
            Some(OTHER_SITE),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert_eq!(code(&r), "origin_not_allowed");
}

#[tokio::test]
async fn an_origin_another_key_allows_is_still_refused_for_this_key() {
    let e = embed().await;
    let other = e.fx.runnable("sales").await;
    e.fx.publish(&e.fx.alice, &other).await;
    new_key(&e.fx, &other, &[OTHER_SITE]).await;
    let r = e.start_with(&e.key, Some(OTHER_SITE)).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert_eq!(code(&r), "origin_not_allowed");
}

#[tokio::test]
async fn an_unknown_or_revoked_key_is_refused() {
    let e = embed().await;
    let r = e.start_with("gwe_nope", Some(SITE)).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    assert_eq!(code(&r), "embed_key_invalid");
    let forged = format!("gwe_{}", "0".repeat(64));
    assert_eq!(
        code(&e.start_with(&forged, Some(SITE)).await),
        "embed_key_invalid"
    );

    let token = e.visitor().await;
    let revoke = format!("/api/v0/agents/{}/embed-keys/{}/revoke", e.agent, e.key_id);
    e.fx.post(&e.fx.alice, &revoke, json!({})).await;

    let r = e.start_with(&e.key, Some(SITE)).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert_eq!(code(&r), "embed_key_revoked");
    let r = e.say(&token, "still there?").await;
    assert_eq!(
        r.status,
        StatusCode::FORBIDDEN,
        "an open conversation ends too"
    );
    assert_eq!(code(&r), "embed_key_revoked");
}

#[tokio::test]
async fn an_unpublished_or_disabled_agent_is_refused() {
    let e = embed().await;
    let draft_only = e.fx.runnable("draft-only").await;
    let (_, key) = new_key(&e.fx, &draft_only, &[SITE]).await;
    let r = e.start_with(&key, Some(SITE)).await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert_eq!(code(&r), "agent_not_published");

    let token = e.visitor().await;
    let (status, _) =
        e.fx.post(
            &e.fx.alice,
            &format!("/api/v0/system-principals/{}/disable", e.agent),
            json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let r = e.start_with(&e.key, Some(SITE)).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert_eq!(code(&r), "agent_disabled");
    assert_eq!(code(&e.resume(&token).await), "agent_disabled");
}

// ---------------------------------------------------------------------------
// Conversation, reload and expiry

#[tokio::test]
async fn a_message_runs_as_the_agent_on_its_live_version() {
    let e = embed().await;
    let token = e.visitor().await;
    let r = e.say(&token, "  hi there ").await;
    assert_eq!(r.status, StatusCode::ACCEPTED, "{}", r.body);
    let session = e.conversation_of(&token).await;
    let turn_id = r.body["turn_id"].as_str().unwrap();
    e.wait_terminal(&session, turn_id).await;

    let seen = e.runner.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 1);
    let run = &seen[0];
    assert_eq!(run.agent_id, e.agent);
    assert_eq!(run.version, 1);
    assert_eq!(run.session_id, session);
    assert_eq!(run.turn_id, turn_id);
    assert!(run.visitor_id.is_some());
    let owner = run_sessions::session_owner(&e.fx.state.db, &session)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        owner,
        run_sessions::SessionOwner::Principal(e.agent.clone())
    );
}

#[tokio::test]
async fn a_reload_within_the_ttl_resumes_the_same_conversation() {
    let e = embed().await;
    let token = e.visitor().await;
    let r = e.say(&token, "where is my order?").await;
    let session = e.conversation_of(&token).await;
    e.wait_terminal(&session, r.body["turn_id"].as_str().unwrap())
        .await;

    let resumed = e.resume(&token).await;
    assert_eq!(resumed.status, StatusCode::OK, "{}", resumed.body);
    let turns = resumed.body["turns"].as_array().unwrap();
    assert_eq!(turns.len(), 2);
    assert_eq!(turns[0]["turn"]["user_content"], "where is my order?");
    assert_eq!(turns[1]["turn"]["content"], ANSWER);
    assert_eq!(resumed.body["live_turn_id"], Value::Null);
    assert_eq!(e.conversation_of(&token).await, session);

    let second = e.say(&token, "thanks").await;
    assert_eq!(second.status, StatusCode::ACCEPTED);
    e.wait_terminal(&session, second.body["turn_id"].as_str().unwrap())
        .await;
    let turns = chat::list_turns(&e.fx.state.db, &session).await.unwrap();
    assert_eq!(turns.len(), 4, "one conversation, both exchanges");
}

#[tokio::test]
async fn every_accepted_request_slides_the_expiry() {
    let e = embed().await;
    let token = e.visitor().await;
    let hash = sha256_hex(token.as_bytes());
    let soon = jiff::Timestamp::now()
        .checked_add(jiff::SignedDuration::from_secs(60))
        .unwrap();
    sqlx::query("UPDATE visitor_sessions SET expires_at = ? WHERE token_hash = ?")
        .bind(soon.to_string())
        .bind(&hash)
        .execute(&e.fx.state.db)
        .await
        .unwrap();
    let r = e.resume(&token).await;
    assert_eq!(r.status, StatusCode::OK);
    let slid: jiff::Timestamp = r.body["expires_at"].as_str().unwrap().parse().unwrap();
    assert!(
        slid > soon
            .checked_add(jiff::SignedDuration::from_secs(25 * 60))
            .unwrap(),
        "slid to ~30 minutes from now, got {slid}"
    );
}

#[tokio::test]
async fn after_the_ttl_a_new_session_starts() {
    let e = embed().await;
    let token = e.visitor().await;
    e.say(&token, "hello").await;
    let old = e.conversation_of(&token).await;
    sqlx::query("UPDATE visitor_sessions SET expires_at = '2026-01-01T00:00:00Z'")
        .execute(&e.fx.state.db)
        .await
        .unwrap();

    for r in [e.resume(&token).await, e.say(&token, "again").await] {
        assert_eq!(r.status, StatusCode::UNAUTHORIZED);
        assert_eq!(code(&r), "visitor_session_expired");
    }

    let fresh = e.visitor().await;
    assert_ne!(fresh, token);
    let new = e.conversation_of(&fresh).await;
    assert_ne!(new, old);
    let resumed = e.resume(&fresh).await;
    assert_eq!(
        resumed.body["turns"],
        json!([]),
        "a new conversation starts empty"
    );
}

#[tokio::test]
async fn an_unknown_or_malformed_visitor_token_is_refused() {
    let e = embed().await;
    for bearer in [None, Some("gwv_nope"), Some(e.key.as_str())] {
        let r = e
            .send(
                Method::GET,
                "/api/v0/embed/session",
                Some(SITE),
                bearer,
                None,
            )
            .await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED, "{bearer:?}");
        assert_eq!(code(&r), "visitor_session_invalid");
    }
}

#[tokio::test]
async fn a_message_must_have_text_and_nothing_else() {
    let e = embed().await;
    let token = e.visitor().await;
    assert_eq!(e.say(&token, "   ").await.status, StatusCode::BAD_REQUEST);
    assert_eq!(
        e.say(&token, &"x".repeat(8_001)).await.status,
        StatusCode::BAD_REQUEST
    );
    let r = e
        .send(
            Method::POST,
            "/api/v0/embed/messages",
            Some(SITE),
            Some(&token),
            Some(json!({ "text": "hi", "session_id": "someone-elses" })),
        )
        .await;
    assert_eq!(
        r.status,
        StatusCode::BAD_REQUEST,
        "the token names the conversation"
    );
}

#[tokio::test]
async fn without_a_runner_a_message_is_refused_and_nothing_stored() {
    let e = embed_with(None, None).await;
    let token = e.visitor().await;
    let r = e.say(&token, "hello").await;
    assert_eq!(r.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(code(&r), "agent_runtime_unavailable");
    let session = e.conversation_of(&token).await;
    assert!(
        chat::list_turns(&e.fx.state.db, &session)
            .await
            .unwrap()
            .is_empty()
    );
}

// ---------------------------------------------------------------------------
// The event stream

#[tokio::test]
async fn an_answer_reaches_the_visitor_whole_once_its_turn_is_done() {
    let gate = Arc::new(Notify::new());
    let e = embed_with(Some(ScriptedRunner::held(gate.clone())), None).await;
    let token = e.visitor().await;
    let r = e.say(&token, "hi").await;
    assert_eq!(r.status, StatusCode::ACCEPTED, "{}", r.body);
    let turn_id = r.body["turn_id"].as_str().unwrap().to_string();
    let session = e.conversation_of(&token).await;
    for _ in 0..200 {
        let t = chat::get_turn(&e.fx.state.db, &session, &turn_id)
            .await
            .unwrap()
            .unwrap();
        if t.content.as_deref() == Some(PARTIAL) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    assert!(
        e.fx.state.chats.holds(&e.agent, &session, &turn_id),
        "the turn runs on a worker of the session worker registry"
    );
    let busy = e.say(&token, "are you there?").await;
    assert_eq!(busy.status, StatusCode::CONFLICT);
    assert_eq!(code(&busy), "turn_in_progress");
    let resumed = e.resume(&token).await;
    assert_eq!(resumed.body["live_turn_id"], turn_id.as_str());
    assert_eq!(resumed.body["turns"][1]["turn"]["content"], Value::Null);

    let resp = e
        .raw(
            Method::GET,
            "/api/v0/embed/events",
            Some(SITE),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get(header::CONTENT_TYPE).unwrap(),
        "text/event-stream"
    );
    assert_eq!(
        resp.headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .unwrap(),
        SITE
    );
    let body = tokio::spawn(common::read_body(resp));
    tokio::time::sleep(Duration::from_millis(100)).await;
    gate.notify_one();
    let body = tokio::time::timeout(Duration::from_secs(10), body)
        .await
        .expect("the stream ends once the turn is done")
        .unwrap();

    assert!(
        !String::from_utf8_lossy(&body).contains(PARTIAL),
        "a partial answer never reaches a visitor"
    );
    let frames = frames(&body);
    let names: Vec<&str> = frames.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["snapshot", "turn_delta", "turn_finalized"]);
    let snapshot = &frames[0].1;
    assert_eq!(snapshot["live_turn_id"], turn_id.as_str());
    assert_eq!(snapshot["turns"][0]["turn"]["user_content"], "hi");
    assert_eq!(snapshot["turns"][1]["turn"]["content"], Value::Null);
    assert_eq!(frames[1].1["text_delta"], ANSWER);
    assert_eq!(frames[1].1["full"], true);
    assert_eq!(frames[2].1["status"], "completed");
    assert!(
        e.fx.state.chats.get(&e.agent, &session).is_none(),
        "the answer is delivered only once the worker has left the registry"
    );
    for _ in 0..200 {
        if e.say(&token, "thanks").await.status == StatusCode::ACCEPTED {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the conversation stayed blocked after its turn finished");
}

#[tokio::test]
async fn a_quiet_conversation_streams_its_snapshot_then_idle() {
    let e = embed().await;
    let token = e.visitor().await;
    let resp = e
        .raw(
            Method::GET,
            "/api/v0/embed/events",
            Some(SITE),
            Some(&token),
            None,
        )
        .await;
    let body = common::read_body(resp).await;
    let names: Vec<String> = frames(&body).into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, ["snapshot", "idle"]);
}

#[tokio::test]
async fn a_runner_that_leaves_its_turn_unfinished_does_not_wedge_the_conversation() {
    let e = embed_with(Some(ScriptedRunner::default()), None).await;
    let token = e.visitor().await;
    let r = e.say(&token, "hi").await;
    let session = e.conversation_of(&token).await;
    let t = e
        .wait_terminal(&session, r.body["turn_id"].as_str().unwrap())
        .await;
    assert_eq!(t.status, chat::TurnStatus::Errored);

    let resumed = e.resume(&token).await;
    let answer = &resumed.body["turns"][1]["turn"];
    assert_eq!(answer["status"], "errored");
    assert!(
        !answer["error_message"]
            .as_str()
            .unwrap()
            .contains("agent run"),
        "the visitor gets a generic error, not the internal one"
    );
    for _ in 0..200 {
        if e.say(&token, "again").await.status == StatusCode::ACCEPTED {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the conversation stayed blocked");
}

/// `GET uri` as the visitor, from [`SITE`], asking for `accept_language`.
async fn get_in(e: &Embed, uri: &str, token: &str, accept_language: &str) -> Vec<u8> {
    let req = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .header(header::ORIGIN, SITE)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::ACCEPT_LANGUAGE, accept_language)
        .body(Body::empty())
        .unwrap();
    let resp = common::app(e.fx.state.clone()).serve(req).await.unwrap();
    common::read_body(resp).await.to_vec()
}

#[tokio::test]
async fn an_errored_answer_reads_the_generic_error_in_the_visitors_language() {
    const GERMAN: &str = "Etwas ist schiefgelaufen. Bitte versuchen Sie es erneut.";
    let gate = Arc::new(Notify::new());
    let e = embed_with(
        Some(ScriptedRunner {
            hold: Some(gate.clone()),
            finish: false,
            ..Default::default()
        }),
        None,
    )
    .await;
    let token = e.visitor().await;
    let r = e.say(&token, "hi").await;
    assert_eq!(r.status, StatusCode::ACCEPTED, "{}", r.body);
    let turn_id = r.body["turn_id"].as_str().unwrap().to_string();

    let stream = {
        let e_state = e.fx.state.clone();
        let token = token.clone();
        tokio::spawn(async move {
            let req = Request::builder()
                .method(Method::GET)
                .uri("/api/v0/embed/events")
                .header(header::ORIGIN, SITE)
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::ACCEPT_LANGUAGE, "de")
                .body(Body::empty())
                .unwrap();
            let resp = common::app(e_state).serve(req).await.unwrap();
            common::read_body(resp).await
        })
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
    gate.notify_one();
    let body = tokio::time::timeout(Duration::from_secs(20), stream)
        .await
        .expect("the stream ends with the turn")
        .unwrap();
    let finalized = frames(&body)
        .into_iter()
        .find(|(n, _)| n == "turn_finalized")
        .expect("a turn_finalized frame")
        .1;
    assert_eq!(finalized["turn_id"], turn_id.as_str());
    assert_eq!(finalized["status"], "errored");
    assert_eq!(finalized["error_message"], GERMAN);

    let session: Value =
        serde_json::from_slice(&get_in(&e, "/api/v0/embed/session", &token, "de").await).unwrap();
    assert_eq!(
        session["turns"][1]["turn"]["error_message"], GERMAN,
        "{session}"
    );
}

// ---------------------------------------------------------------------------
// Isolation

#[tokio::test]
async fn a_visitor_token_reaches_nothing_but_the_embed_routes() {
    let e = embed().await;
    let token = e.visitor().await;
    let session = e.conversation_of(&token).await;
    for (method, uri) in [
        (Method::GET, "/api/v0/me".to_string()),
        (Method::GET, "/api/v0/chat/sessions".to_string()),
        (Method::GET, format!("/api/v0/chat/sessions/{session}")),
        (
            Method::GET,
            format!("/api/v0/chat/sessions/{session}/events"),
        ),
        (
            Method::POST,
            format!("/api/v0/chat/sessions/{session}/messages"),
        ),
        (Method::GET, "/api/v0/agents".to_string()),
        (
            Method::GET,
            format!("/api/v0/agents/{}/embed-keys", e.agent),
        ),
        (Method::GET, "/api/v0/system-principals".to_string()),
        (Method::GET, "/api/v0/tokens".to_string()),
        (Method::GET, "/v1/models".to_string()),
        (Method::POST, "/v1/chat/completions".to_string()),
    ] {
        let r = e
            .send(
                method.clone(),
                &uri,
                Some(SITE),
                Some(&token),
                Some(json!({})),
            )
            .await;
        assert_eq!(
            r.status,
            StatusCode::UNAUTHORIZED,
            "{method} {uri}: {}",
            r.body
        );
        if uri.starts_with("/api/") {
            assert!(
                r.headers.get(header::ACCESS_CONTROL_ALLOW_ORIGIN).is_none(),
                "{uri} must not be opened to the embedding site"
            );
        }
    }
}

#[tokio::test]
async fn a_visitor_only_ever_sees_its_own_conversation() {
    let e = embed().await;
    let other_agent = e.fx.runnable("sales").await;
    e.fx.publish(&e.fx.alice, &other_agent).await;
    let (_, other_key) = new_key(&e.fx, &other_agent, &[SITE]).await;

    let mine = e.visitor().await;
    let neighbour = e.visitor().await;
    let stranger = e.start_with(&other_key, Some(SITE)).await.body["token"]
        .as_str()
        .unwrap()
        .to_string();
    for (token, text) in [
        (&neighbour, "neighbour secret"),
        (&stranger, "other agent secret"),
    ] {
        let r = e.say(token, text).await;
        assert_eq!(r.status, StatusCode::ACCEPTED, "{}", r.body);
        let session = e.conversation_of(token).await;
        e.wait_terminal(&session, r.body["turn_id"].as_str().unwrap())
            .await;
    }
    assert_ne!(
        e.conversation_of(&mine).await,
        e.conversation_of(&neighbour).await
    );

    let resumed = e.resume(&mine).await;
    assert_eq!(resumed.body["turns"], json!([]));
    let resp = e
        .raw(
            Method::GET,
            "/api/v0/embed/events",
            Some(SITE),
            Some(&mine),
            None,
        )
        .await;
    let body = String::from_utf8_lossy(&common::read_body(resp).await).to_string();
    assert!(!body.contains("secret"), "{body}");
}

#[tokio::test]
async fn keys_and_visitor_tokens_are_stored_only_as_hashes() {
    let e = embed().await;
    let token = e.visitor().await;
    let db = &e.fx.state.db;
    let key_hash: String = sqlx::query_scalar("SELECT key_hash FROM agent_embed_keys")
        .fetch_one(db)
        .await
        .unwrap();
    assert_eq!(key_hash, sha256_hex(e.key.as_bytes()));
    let token_hash: String = sqlx::query_scalar("SELECT token_hash FROM visitor_sessions")
        .fetch_one(db)
        .await
        .unwrap();
    assert_eq!(token_hash, sha256_hex(token.as_bytes()));

    let dump: String = sqlx::query_scalar(
        "SELECT group_concat(v, '|') FROM (
           SELECT id || name || key_hash || origins || created_by AS v FROM agent_embed_keys
           UNION ALL
           SELECT id || token_hash || session_id || COALESCE(client_ip, '') FROM visitor_sessions
           UNION ALL
           SELECT kind || detail FROM agent_audit
         )",
    )
    .fetch_one(db)
    .await
    .unwrap();
    assert!(!dump.contains(&e.key), "the embed key is stored in clear");
    assert!(
        !dump.contains(&token),
        "the visitor token is stored in clear"
    );
}

// ---------------------------------------------------------------------------
// CORS

fn preflight(uri: &str, origin: &str) -> Request {
    Request::builder()
        .method(Method::OPTIONS)
        .uri(uri)
        .header(header::ORIGIN, origin)
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
        .header(
            header::ACCESS_CONTROL_REQUEST_HEADERS,
            "authorization,content-type",
        )
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn a_preflight_is_answered_only_for_an_embeddable_origin() {
    let e = embed().await;
    let app = common::app(e.fx.state.clone());
    let ok = app
        .serve(preflight("/api/v0/embed/messages", SITE))
        .await
        .unwrap();
    assert_eq!(ok.status(), StatusCode::NO_CONTENT);
    let h = ok.headers();
    assert_eq!(h.get(header::ACCESS_CONTROL_ALLOW_ORIGIN).unwrap(), SITE);
    assert_eq!(
        h.get(header::ACCESS_CONTROL_ALLOW_HEADERS).unwrap(),
        "authorization, content-type"
    );
    assert!(h.get(header::ACCESS_CONTROL_ALLOW_CREDENTIALS).is_none());

    let refused = app
        .serve(preflight("/api/v0/embed/messages", OTHER_SITE))
        .await
        .unwrap();
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);
    assert!(
        refused
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none()
    );

    let revoke = format!("/api/v0/agents/{}/embed-keys/{}/revoke", e.agent, e.key_id);
    e.fx.post(&e.fx.alice, &revoke, json!({})).await;
    let after = app
        .serve(preflight("/api/v0/embed/sessions", SITE))
        .await
        .unwrap();
    assert_eq!(
        after.status(),
        StatusCode::FORBIDDEN,
        "a revoked key opens no origin"
    );

    let (_, _) = new_key(&e.fx, &e.agent, &[OTHER_SITE]).await;
    let opened = app
        .serve(preflight("/api/v0/embed/sessions", OTHER_SITE))
        .await
        .unwrap();
    assert_eq!(
        opened.status(),
        StatusCode::NO_CONTENT,
        "a new key opens its origin at once"
    );
    let (status, body) =
        e.fx.post(
            &e.fx.alice,
            &format!("/api/v0/system-principals/{}/disable", e.agent),
            json!({}),
        )
        .await;
    assert!(status.is_success(), "{status}: {body}");
    let disabled = app
        .serve(preflight("/api/v0/embed/sessions", OTHER_SITE))
        .await
        .unwrap();
    assert_eq!(
        disabled.status(),
        StatusCode::FORBIDDEN,
        "a disabled agent opens no origin, at once"
    );
}

#[tokio::test]
async fn an_embeddable_origin_gets_no_cors_on_any_other_api_route() {
    let e = embed().await;
    let app = common::app(e.fx.state.clone());
    for uri in ["/api/v0/me", "/api/v0/chat/sessions", "/api/v0/agents"] {
        let resp = app.serve(preflight(uri, SITE)).await.unwrap();
        assert_ne!(resp.status(), StatusCode::NO_CONTENT, "{uri}");
        assert!(
            resp.headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .is_none(),
            "{uri}"
        );
    }
}

// ---------------------------------------------------------------------------
// Version pinning and the spec's own origins

#[tokio::test]
async fn a_conversation_stays_on_the_version_it_started_on() {
    let e = embed().await;
    let old = e.visitor().await;
    e.fx.put_draft(&e.fx.alice, &e.agent, spec("v2")).await;
    let (status, body) = e.fx.publish(&e.fx.alice, &e.agent).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let new = e.visitor().await;

    for token in [&old, &new] {
        let r = e.say(token, "hi").await;
        assert_eq!(r.status, StatusCode::ACCEPTED, "{}", r.body);
        let session = e.conversation_of(token).await;
        e.wait_terminal(&session, r.body["turn_id"].as_str().unwrap())
            .await;
    }
    let versions: Vec<i64> = e
        .runner
        .seen
        .lock()
        .unwrap()
        .iter()
        .map(|t| t.version)
        .collect();
    assert_eq!(versions, [1, 2], "the old conversation keeps version 1");
}

#[tokio::test]
async fn an_origin_must_also_be_in_the_specs_publish_origins_when_it_sets_them() {
    let e = embed_with(
        Some(ScriptedRunner::answering()),
        Some(json!({ "publish": { "origins": [OTHER_SITE] } })),
    )
    .await;
    let r = e.start_with(&e.key, Some(SITE)).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert_eq!(code(&r), "origin_not_allowed");
    assert!(
        r.body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("publish.origins"),
        "{}",
        r.body
    );

    let (_, both) = new_key(&e.fx, &e.agent, &[SITE, OTHER_SITE]).await;
    let r = e.start_with(&both, Some(OTHER_SITE)).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
}

// ---------------------------------------------------------------------------
// End to end: the production runner on a wiremock upstream

#[tokio::test]
async fn a_visitor_gets_the_main_agents_answer_from_the_real_runner() {
    let upstream = MockServer::start().await;
    let streamed = [
        json!({"choices": [{"index": 0, "delta": {"content": "Your order "}}]}),
        json!({"choices": [{"index": 0, "delta": {"content": "ships today."}}]}),
    ];
    let sse = streamed
        .iter()
        .map(|c| format!("data: {c}\n\n"))
        .collect::<String>()
        + "data: [DONE]\n\n";
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream"))
        .mount(&upstream)
        .await;
    let e = embed_live(&upstream, None).await;

    let token = e.visitor().await;
    let r = e.say(&token, "where is my order?").await;
    assert_eq!(r.status, StatusCode::ACCEPTED, "{}", r.body);
    let resp = e
        .raw(
            Method::GET,
            "/api/v0/embed/events",
            Some(SITE),
            Some(&token),
            None,
        )
        .await;
    let body = tokio::time::timeout(Duration::from_secs(20), common::read_body(resp))
        .await
        .expect("the stream ends once the answer is in");
    let frames = frames(&body);
    let names: Vec<&str> = frames.iter().map(|(n, _)| n.as_str()).collect();
    let deltas: Vec<&Value> = frames
        .iter()
        .filter(|(n, _)| n == "turn_delta")
        .map(|(_, d)| d)
        .collect();
    if names.last() == Some(&"turn_finalized") {
        assert_eq!(
            deltas.len(),
            1,
            "one whole answer, not a token stream: {names:?}"
        );
        assert_eq!(deltas[0]["text_delta"], "Your order ships today.");
        assert_eq!(deltas[0]["full"], true);
        assert_eq!(frames.last().unwrap().1["status"], "completed");
    } else {
        assert_eq!(
            names,
            ["snapshot", "idle"],
            "the turn finished before attaching"
        );
        assert_eq!(
            frames[0].1["turns"][1]["turn"]["content"],
            "Your order ships today."
        );
    }

    let session = e.conversation_of(&token).await;
    let turns = chat::list_turns(&e.fx.state.db, &session).await.unwrap();
    assert_eq!(turns[1].turn.status, chat::TurnStatus::Completed);
    let sent: Value =
        serde_json::from_slice(&upstream.received_requests().await.unwrap()[0].body).unwrap();
    assert_eq!(sent["model"], "m");
    let system = sent["messages"][0]["content"].as_str().unwrap();
    assert!(
        system.contains("v1"),
        "the agent's own instructions: {system}"
    );
    assert_eq!(
        sent["messages"].as_array().unwrap().last().unwrap()["content"],
        "where is my order?"
    );
}

#[tokio::test]
async fn the_public_path_withholds_an_unverified_identifier_in_the_visitors_language() {
    let upstream = MockServer::start().await;
    let sse = format!(
        "data: {}\n\ndata: [DONE]\n\n",
        json!({"choices": [{"index": 0, "delta": {"content": "Your invoice RE-424242 is due."}}]})
    );
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream"))
        .mount(&upstream)
        .await;
    let filter = json!({ "publish": {
        "origins": [SITE],
        "output_filter": { "patterns": { "invoice": "RE-\\d{6}" } }
    } });
    let e = embed_live(&upstream, Some(filter)).await;

    let token = e.visitor().await;
    let r = e.say_in(&token, "what do I owe?", "de").await;
    assert_eq!(r.status, StatusCode::ACCEPTED, "{}", r.body);
    let resp = e
        .raw(
            Method::GET,
            "/api/v0/embed/events",
            Some(SITE),
            Some(&token),
            None,
        )
        .await;
    let body = tokio::time::timeout(Duration::from_secs(20), common::read_body(resp))
        .await
        .expect("the stream ends once the answer is in");
    let frames = frames(&body);
    let delivered: String = frames
        .iter()
        .filter(|(n, _)| n == "turn_delta")
        .map(|(_, d)| d["text_delta"].as_str().unwrap().to_string())
        .chain(
            frames
                .iter()
                .filter(|(n, _)| n == "snapshot")
                .filter_map(|(_, d)| {
                    d["turns"][1]["turn"]["content"]
                        .as_str()
                        .map(str::to_string)
                }),
        )
        .collect();
    let german = session_core::i18n::t(session_core::i18n::Lang::De, "agent-output-withheld");
    assert_eq!(delivered, german, "{frames:?}");
    let audit = aiplane_agents::db::agent_audit::for_principal(&e.fx.state.db, &e.agent)
        .await
        .unwrap();
    assert!(audit.iter().any(|a| a.kind == "output_blocked"));
}

#[tokio::test]
async fn a_draft_reaches_the_test_chat_and_never_the_public_path() {
    let upstream = MockServer::start().await;
    let sse = format!(
        "data: {}\n\ndata: [DONE]\n\n",
        json!({"choices": [{"index": 0, "delta": {"content": "ok"}}]})
    );
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream"))
        .mount(&upstream)
        .await;
    let e = embed_live(&upstream, None).await;
    let (status, body) =
        e.fx.put_draft(&e.fx.alice, &e.agent, spec("DRAFT-ONLY"))
            .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, test) = crate::common::test_chat_turn(
        &e.fx.state,
        &e.fx.alice,
        &e.agent,
        json!({ "source": "draft", "message": "manager here" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{test}");
    let token = e.visitor().await;
    let r = e.say(&token, "visitor here").await;
    assert_eq!(r.status, StatusCode::ACCEPTED, "{}", r.body);
    let session = e.conversation_of(&token).await;
    let turns = chat::list_turns(&e.fx.state.db, &session).await.unwrap();
    e.wait_terminal(&session, &turns[1].turn.id).await;

    let systems: Vec<String> = upstream
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| {
            let body: Value = serde_json::from_slice(&r.body).unwrap();
            body["messages"][0]["content"].as_str().unwrap().to_string()
        })
        .collect();
    assert_eq!(systems.len(), 2, "{systems:?}");
    assert!(
        systems[0].contains("DRAFT-ONLY"),
        "the test chat runs the draft"
    );
    assert!(
        !systems[1].contains("DRAFT-ONLY") && systems[1].contains("v1"),
        "the visitor runs published version 1: {}",
        systems[1]
    );

    let db = e.fx.state.db.clone();
    let version = |id: String| {
        let db = db.clone();
        async move {
            sqlx::query_scalar::<_, Option<i64>>(
                "SELECT agent_version FROM chat_sessions WHERE id = ?",
            )
            .bind(id)
            .fetch_one(&db)
            .await
            .unwrap()
        }
    };
    assert_eq!(version(session).await, Some(1));
    assert_eq!(
        version(test["session_id"].as_str().unwrap().to_string()).await,
        Some(0)
    );
}
