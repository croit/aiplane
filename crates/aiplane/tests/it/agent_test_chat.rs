// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The agent builder's internal test chat end to end
//! (`docs/agent-builder.md` → "Test chat"): `POST /api/v0/agents/{id}/test/messages`
//! runs the **draft** through the real run path on wiremock upstreams in the
//! background; the conversation streams on `…/test/{session}/events`, and
//! `…/turns/{turn}/debug` gives each turn's debug view only a manager gets.
//!
//! `alice` and `bob` hold `can_manage_agents`; `plain` holds nothing. The
//! main agent (`support`) sets an `issue` slot, then `forward_request`
//! reaches the `tech` sub-agent, which finishes.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use rama::Service;
use rama::http::{Body, Method, Request, StatusCode};
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::common::{self, TEST_SECRET};

use aiplane::rama_server::{RamaState, SessionStore};
use aiplane_core::server::db::{self, gateway_groups, users};
use aiplane_core::server::rbac::Resolver;
use aiplane_core::server::upstreams::{
    self,
    config::{PickerStrategy, PoolKind, UpstreamPoolConfig},
};
use aiplane_runtime::server::AppState;
use aiplane_runtime::server::tools::ToolRegistry;
use aiplane_runtime::server::tools::time::CurrentTimestamp;

struct Scripted {
    deltas: Vec<Value>,
    served: AtomicUsize,
    delay: std::time::Duration,
}

impl wiremock::Respond for Scripted {
    fn respond(&self, _req: &wiremock::Request) -> ResponseTemplate {
        let i = self.served.fetch_add(1, Ordering::SeqCst);
        let delta = &self.deltas[i.min(self.deltas.len() - 1)];
        let sse = format!(
            "data: {}\n\ndata: [DONE]\n\n",
            json!({"choices": [{"index": 0, "delta": delta}]})
        );
        ResponseTemplate::new(200)
            .set_body_raw(sse, "text/event-stream")
            .set_delay(self.delay)
    }
}

async fn llm(deltas: Vec<Value>) -> MockServer {
    llm_after(deltas, std::time::Duration::ZERO).await
}

/// [`llm`], answering each round only after `delay`: long enough for a
/// stream to attach while the turn still runs.
async fn llm_after(deltas: Vec<Value>, delay: std::time::Duration) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(Scripted {
            deltas,
            served: AtomicUsize::new(0),
            delay,
        })
        .mount(&server)
        .await;
    server
}

fn call(id: &str, name: &str, args: Value) -> Value {
    json!({ "tool_calls": [{"index": 0, "id": id, "type": "function",
        "function": {"name": name, "arguments": args.to_string()}}] })
}

fn text(s: &str) -> Value {
    json!({ "content": s })
}

async fn requests(server: &MockServer) -> Vec<Value> {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| serde_json::from_slice(&r.body).unwrap())
        .collect()
}

struct Fx {
    state: RamaState,
    alice: String,
    bob: String,
    plain: String,
    main_llm: MockServer,
    /// Kept alive for the fixture's `guard-pool`.
    _guard_llm: MockServer,
}

/// A topic guard upstream that judges every message out of scope.
async fn off_topic_guard() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{ "message": { "role": "assistant",
                                       "content": json!({ "verdict": "out_of_scope" }).to_string() } }]
        })))
        .mount(&server)
        .await;
    server
}

async fn person(state: &RamaState, id: &str, roles: &[&str]) -> String {
    let now = jiff::Timestamp::now();
    users::upsert(
        &state.db,
        &users::User {
            id: id.into(),
            email: format!("{id}@example.com"),
            name: None,
            roles: roles.iter().map(|r| r.to_string()).collect(),
            created_at: now,
            updated_at: now,
            timezone: None,
            speech_voice: None,
        },
    )
    .await
    .unwrap();
    let session = state.sessions.create(id).await.unwrap();
    state.sessions.sign(&session.id)
}

fn chat_pool(upstream: &MockServer) -> UpstreamPoolConfig {
    UpstreamPoolConfig {
        voices: Default::default(),
        offer_voices: Vec::new(),
        allowed_groups: Vec::new(),
        fallback_offline: None,
        level_rank: 0,
        enforce_limits: true,
        kind: PoolKind::Chat,
        strategy: PickerStrategy::RoundRobin,
        models: Vec::new(),
        backend: vec![common::mock_backend("mock", &upstream.uri())],
    }
}

async fn fixture(main_script: Vec<Value>, tech_script: Vec<Value>) -> (Fx, MockServer) {
    fixture_with(llm(main_script).await, tech_script).await
}

async fn fixture_with(main_llm: MockServer, tech_script: Vec<Value>) -> (Fx, MockServer) {
    let pool = db::open(std::path::Path::new(":memory:")).await.unwrap();
    let tech_llm = llm(tech_script).await;
    let guard_llm = off_topic_guard().await;
    let mut pools = HashMap::new();
    pools.insert("main-pool".to_string(), chat_pool(&main_llm));
    pools.insert("tech-pool".to_string(), chat_pool(&tech_llm));
    pools.insert("guard-pool".to_string(), chat_pool(&guard_llm));
    let registry = upstreams::UpstreamRegistry::new(&pools).unwrap();
    common::seed_pool_models(&registry, "main-pool", 0, &["main-model"]);
    common::seed_pool_models(&registry, "tech-pool", 0, &["tech-model"]);
    common::seed_pool_models(&registry, "guard-pool", 0, &["guard-model"]);
    let app = AppState::new(
        common::test_config(),
        pool.clone(),
        registry,
        Arc::new(ToolRegistry::new().with(CurrentTimestamp)),
        Arc::new(Resolver::empty()),
    );
    let state = RamaState::new(
        app,
        SessionStore::new(pool.clone(), TEST_SECRET),
        aiplane_core::server::usage::UsageHandle::disabled(),
    )
    .with_agent_runner(Arc::new(aiplane_runtime::agents::embed::LiveAgentRunner));
    gateway_groups::upsert_group(&pool, "everyone", "", false, true)
        .await
        .unwrap();
    gateway_groups::upsert_group(&pool, "managers", "", false, false)
        .await
        .unwrap();
    gateway_groups::set_can_manage_agents(&pool, "managers", true)
        .await
        .unwrap();
    gateway_groups::set_mappings_for_group(&pool, "managers", &["managers".into()])
        .await
        .unwrap();
    gateway_groups::set_tools_for_group(&pool, "everyone", &["get_current_timestamp".into()])
        .await
        .unwrap();
    state.reload_rbac().await;
    let alice = person(&state, "alice", &["managers"]).await;
    let bob = person(&state, "bob", &["managers"]).await;
    let plain = person(&state, "plain", &[]).await;
    (
        Fx {
            state,
            alice,
            bob,
            plain,
            main_llm,
            _guard_llm: guard_llm,
        },
        tech_llm,
    )
}

impl Fx {
    async fn send(
        &self,
        cookie: &str,
        method: Method,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let req = Request::builder()
            .method(method)
            .uri(uri)
            .header("cookie", format!("id={cookie}"));
        let req = match body {
            Some(b) => req
                .header("content-type", "application/json")
                .body(Body::from(b.to_string())),
            None => req.body(Body::empty()),
        }
        .unwrap();
        let resp = common::app(self.state.clone()).serve(req).await.unwrap();
        let status = resp.status();
        let bytes = common::read_body(resp).await;
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn post(&self, cookie: &str, uri: &str, body: Value) -> (StatusCode, Value) {
        self.send(cookie, Method::POST, uri, Some(body)).await
    }

    async fn create(&self, name: &str, spec: Option<Value>) -> String {
        let mut body = json!({ "name": name });
        if let Some(spec) = spec {
            body["spec"] = spec;
        }
        let (status, body) = self.post(&self.alice, "/api/v0/agents", body).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        body["agent"]["id"].as_str().unwrap().to_string()
    }

    async fn grant_model(&self, id: &str, model: &str) {
        let (status, body) = self
            .post(
                &self.alice,
                &format!("/api/v0/system-principals/{id}/grants"),
                json!({ "kind": "model", "ref": model }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }

    async fn save_draft(&self, id: &str, spec: Value) {
        let (status, body) = self
            .send(
                &self.alice,
                Method::PUT,
                &format!("/api/v0/agents/{id}/draft"),
                Some(json!({ "spec": spec })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }

    async fn publish(&self, id: &str) {
        let (status, body) = self
            .post(
                &self.alice,
                &format!("/api/v0/agents/{id}/publish"),
                json!({}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }

    async fn turn(&self, cookie: &str, id: &str, body: Value) -> (StatusCode, Value) {
        common::test_chat_turn(&self.state, cookie, id, body).await
    }

    /// The published `tech` sub-agent and the unpublished `support` main
    /// agent whose draft routes to it.
    async fn support(&self, orchestration: &str) -> (String, String) {
        let tech = self.create("tech", None).await;
        self.grant_model(&tech, "tech-model").await;
        self.save_draft(
            &tech,
            json!({ "main": { "model": "tech-model",
                "instructions": { "orchestration": "Solve the technical issue." } },
                "finish": { "schema": { "type": "object", "required": ["answer"],
                    "properties": { "answer": { "type": "string" } } } } }),
        )
        .await;
        self.publish(&tech).await;

        let support = self.create("support", None).await;
        self.grant_model(&support, "main-model").await;
        self.save_draft(&support, support_spec(&tech, orchestration))
            .await;
        (support, tech)
    }
}

fn support_spec(tech: &str, orchestration: &str) -> Value {
    json!({
        "main": {
            "model": "main-model",
            "instructions": { "orchestration": orchestration },
            "budget": { "rounds": 6 }
        },
        "state": {
            "issue": { "type": "enum", "values": ["billing", "technical"], "set_by": ["llm"] },
            "email": { "type": "email", "set_by": ["llm"] }
        },
        "routes": {
            "technical": {
                "description": "Technical problems",
                "when": { "slot": "issue", "eq": "technical" },
                "agent": tech,
                "task": "Help with the technical issue."
            }
        }
    })
}

fn support_script() -> Vec<Value> {
    vec![
        call("c1", "set_issue", json!({ "value": "technical" })),
        call("c2", "forward_request", json!({})),
        text("The technician looked into it."),
    ]
}

fn tech_script() -> Vec<Value> {
    vec![call(
        "f1",
        "finish",
        json!({ "result": { "answer": "Restart the router." } }),
    )]
}

#[tokio::test]
async fn a_draft_that_was_never_published_answers_with_the_manager_debug_view() {
    let (fx, _tech_llm) = fixture(support_script(), tech_script()).await;
    let (support, tech) = fx.support("Find out what is wrong, then forward it.").await;

    let (status, body) = fx
        .turn(
            &fx.alice,
            &support,
            json!({ "source": "draft", "message": "My router is broken" }),
        )
        .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "completed", "{body}");
    assert_eq!(body["answer"], "The technician looked into it.");
    assert_eq!(body["agent_version"], 0);
    assert!(body["session_id"].as_str().is_some());

    let slots = body["debug"]["slots"].as_array().unwrap();
    let issue = slots.iter().find(|s| s["slot"] == "issue").unwrap();
    assert_eq!(issue["status"], "set");
    assert_eq!(issue["value"], "technical");
    assert_eq!(issue["provenance"], "llm");
    let email = slots.iter().find(|s| s["slot"] == "email").unwrap();
    assert_eq!(email["status"], "missing");

    let routes = body["debug"]["routes"].as_array().unwrap();
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0]["route"], "technical");
    assert_eq!(routes[0]["open"], true);

    let routing = body["debug"]["routing"].as_array().unwrap();
    assert_eq!(routing.len(), 1);
    assert_eq!(routing[0]["picked"], "technical");

    let subs = body["debug"]["sub_agents"].as_array().unwrap();
    assert_eq!(subs.len(), 1, "{body}");
    assert_eq!(subs[0]["sub_agent_id"], tech);
    assert_eq!(subs[0]["outcome"]["status"], "finished");
    assert_eq!(
        subs[0]["outcome"]["result"]["answer"],
        "Restart the router."
    );

    let tools: Vec<&str> = body["debug"]["tool_calls"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["tool"].as_str().unwrap())
        .collect();
    assert_eq!(tools, ["set_issue", "forward_request"]);
}

#[tokio::test]
async fn the_test_conversation_streams_like_any_chat_and_stays_out_of_the_inbox() {
    let slow = llm_after(
        vec![text("Streamed answer.")],
        std::time::Duration::from_millis(400),
    )
    .await;
    let (fx, _tech_llm) = fixture_with(slow, tech_script()).await;
    let (support, _) = fx.support("Answer.").await;

    let (status, sent) = fx
        .post(
            &fx.alice,
            &format!("/api/v0/agents/{support}/test/messages"),
            json!({ "source": "draft", "message": "hello" }),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{sent}");
    assert_eq!(sent["agent_version"], 0);
    let session = sent["session_id"].as_str().unwrap();
    let turn = sent["turn_id"].as_str().unwrap();

    let resp = common::app(fx.state.clone())
        .serve(
            Request::builder()
                .uri(format!("/api/v0/agents/{support}/test/{session}/events"))
                .header("cookie", format!("id={}", fx.alice))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = tokio::time::timeout(std::time::Duration::from_secs(20), common::read_body(resp))
        .await
        .expect("the stream ends with the turn");
    let frames: Vec<(String, Value)> = String::from_utf8_lossy(&body)
        .split("\n\n")
        .filter_map(|block| {
            let event = block.lines().find_map(|l| l.strip_prefix("event: "))?;
            let data = block.lines().find_map(|l| l.strip_prefix("data: "))?;
            Some((event.to_string(), serde_json::from_str(data).unwrap()))
        })
        .collect();
    let names: Vec<&str> = frames.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names.first(), Some(&"snapshot"), "{names:?}");
    assert_eq!(frames[0].1["live_turn_id"], turn, "attached while it ran");
    let streamed: String = frames
        .iter()
        .filter(|(n, f)| n == "turn_delta" && f["turn_id"] == turn)
        .map(|(_, f)| f["text_delta"].as_str().unwrap())
        .collect();
    assert_eq!(streamed, "Streamed answer.");
    assert_eq!(names.last(), Some(&"turn_finalized"), "{names:?}");

    let done = common::test_chat_settled(&fx.state, &fx.alice, &support, session, turn).await;
    assert_eq!(done["answer"], "Streamed answer.");
    assert!(done["debug"]["routes"].is_array(), "{done}");

    let (_, inbox) = fx
        .send(&fx.alice, Method::GET, "/api/v0/agents/inbox", None)
        .await;
    assert_eq!(
        inbox["count"], 0,
        "a test conversation is never an inbox item"
    );
    for uri in [
        format!("/api/v0/agents/{support}/test/nope/events"),
        format!("/api/v0/agents/{support}/test/nope/turns/{turn}/debug"),
    ] {
        let (status, body) = fx.send(&fx.alice, Method::GET, &uri, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}: {body}");
    }
    let (status, _) = fx
        .send(
            &fx.plain,
            Method::GET,
            &format!("/api/v0/agents/{support}/test/{session}/events"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "not an agent manager");
}

#[tokio::test]
async fn the_debug_view_shows_the_topic_guards_verdict() {
    let (fx, _tech_llm) = fixture(vec![text("Diesel engines …")], tech_script()).await;
    let website = fx.create("website", None).await;
    fx.grant_model(&website, "main-model").await;
    fx.grant_model(&website, "guard-model").await;
    let mut spec = json!({
        "main": { "model": "main-model",
                  "instructions": { "orchestration": "Answer questions about Ceph." } },
        "scope": { "topics": ["Ceph storage"], "strict": true, "classifier_model": "guard-model" }
    });
    let (status, body) = fx
        .send(
            &fx.alice,
            Method::PUT,
            &format!("/api/v0/agents/{website}/draft"),
            Some(json!({ "spec": spec })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["error"]["issues"][0]["path"], "scope.refusal");
    spec["scope"]["refusal"] = json!("I only talk about Ceph.");
    fx.save_draft(&website, spec).await;

    let (status, body) = fx
        .turn(
            &fx.alice,
            &website,
            json!({ "source": "draft", "message": "How does a diesel engine work?" }),
        )
        .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["answer"], "I only talk about Ceph.");
    assert_eq!(
        body["debug"]["scope"],
        json!({ "verdict": "out_of_scope", "topics": ["Ceph storage"] })
    );
    assert!(requests(&fx.main_llm).await.is_empty());
}

#[tokio::test]
async fn the_test_chat_runs_the_draft_not_the_published_version() {
    let (fx, _tech_llm) = fixture(vec![text("ok")], tech_script()).await;
    let (support, tech) = fx.support("LIVE-INSTRUCTIONS").await;
    fx.publish(&support).await;
    fx.save_draft(&support, support_spec(&tech, "DRAFT-INSTRUCTIONS"))
        .await;

    let (status, body) = fx
        .turn(
            &fx.alice,
            &support,
            json!({ "source": "draft", "message": "hello" }),
        )
        .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let seen = requests(&fx.main_llm).await;
    let system = seen[0]["messages"][0]["content"].as_str().unwrap();
    assert!(system.contains("DRAFT-INSTRUCTIONS"), "{system}");
    assert!(!system.contains("LIVE-INSTRUCTIONS"), "{system}");
}

#[tokio::test]
async fn a_second_turn_continues_the_conversation_and_keeps_its_state() {
    let (fx, _tech_llm) = fixture(
        vec![
            call("c1", "set_email", json!({ "value": "ada@example.com" })),
            text("Thanks, Ada."),
            text("Anything else?"),
        ],
        tech_script(),
    )
    .await;
    let (support, _) = fx.support("Collect the email.").await;

    let (_, first) = fx
        .turn(
            &fx.alice,
            &support,
            json!({ "source": "draft", "message": "I am ada@example.com" }),
        )
        .await;
    let session = first["session_id"].as_str().unwrap();
    let (status, second) = fx
        .turn(
            &fx.alice,
            &support,
            json!({ "source": "draft", "message": "that is all", "session_id": session }),
        )
        .await;

    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(second["session_id"], session);
    assert_ne!(second["turn_id"], first["turn_id"]);
    let email = second["debug"]["slots"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["slot"] == "email")
        .unwrap()
        .clone();
    assert_eq!(email["value"], "ada@example.com");
    assert!(
        second["debug"]["tool_calls"].as_array().unwrap().is_empty(),
        "the debug view lists this turn's calls only: {second}"
    );
}

#[tokio::test]
async fn an_unknown_session_empty_message_and_ungranted_model_say_what_to_fix() {
    let (fx, _tech_llm) = fixture(vec![text("ok")], tech_script()).await;
    let (support, _) = fx.support("x").await;

    let (status, body) = fx
        .turn(
            &fx.alice,
            &support,
            json!({ "source": "draft", "message": "hi", "session_id": "nope" }),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["error"]["code"], "unknown_session");

    let (status, _) = fx
        .turn(
            &fx.alice,
            &support,
            json!({ "source": "draft", "message": "  " }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let bare = fx.create("bare", None).await;
    fx.grant_model(&bare, "tech-model").await;
    fx.save_draft(&bare, json!({ "main": { "model": "tech-model" } }))
        .await;
    let (status, _) = fx
        .turn(
            &fx.alice,
            &bare,
            json!({ "source": "draft", "message": "hi" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = fx
        .post(
            &fx.alice,
            &format!("/api/v0/system-principals/{bare}/grants/revoke"),
            json!({ "kind": "model", "ref": "tech-model" }),
        )
        .await;
    assert!(status.is_success(), "{status} {body}");
    let (status, body) = fx
        .turn(
            &fx.alice,
            &bare,
            json!({ "source": "draft", "message": "hi" }),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["error"]["code"], "agent_model_not_granted");
    let message = body["error"]["message"].as_str().unwrap();
    assert!(
        message.contains("tech-model") && message.contains("kind `model`"),
        "{message}"
    );
}

#[tokio::test]
async fn only_a_manager_with_a_write_share_may_run_the_draft() {
    let (fx, _tech_llm) = fixture(vec![text("ok")], tech_script()).await;
    let (support, _) = fx.support("x").await;
    let message = json!({ "source": "draft", "message": "hi" });

    let (status, _) = fx.turn(&fx.plain, &support, message.clone()).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "no agent-management permission"
    );

    let (status, _) = fx.turn(&fx.bob, &support, message.clone()).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "no share");

    let (status, _) = fx
        .post(
            &fx.alice,
            &format!("/api/v0/agents/{support}/shares"),
            json!({ "subject_kind": "user", "subject_id": "bob", "access": "read" }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, body) = fx.turn(&fx.bob, &support, message).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"]["code"], "agent_write_required");
}

#[tokio::test]
async fn the_identity_says_whether_the_agents_section_is_open_to_the_user() {
    let (fx, _tech_llm) = fixture(vec![text("ok")], tech_script()).await;

    let (_, alice) = fx.send(&fx.alice, Method::GET, "/api/v0/me", None).await;
    let (_, plain) = fx.send(&fx.plain, Method::GET, "/api/v0/me", None).await;

    assert_eq!(alice["can_manage_agents"], true);
    assert_eq!(plain["can_manage_agents"], false);
}

#[tokio::test]
async fn the_builder_is_offered_exactly_what_the_manager_could_grant() {
    let (fx, _tech_llm) = fixture(vec![text("ok")], tech_script()).await;

    let (status, body) = fx
        .send(&fx.alice, Method::GET, "/api/v0/agent-resources", None)
        .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let models: Vec<&str> = body["models"]["chat"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["id"].as_str())
        .collect();
    assert_eq!(models, ["guard-model", "main-model", "tech-model"]);
    let grants: Vec<&Value> = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| &i["grant"])
        .collect();
    assert_eq!(
        grants,
        [&json!({ "kind": "tool", "refs": ["get_current_timestamp"] })]
    );

    let (status, _) = fx
        .send(&fx.plain, Method::GET, "/api/v0/agent-resources", None)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

/// The setup assistant's starter templates (`web/src/lib/agent-templates.json`)
/// with every `@key` string translated, as the SPA sends them.
fn templates_in(lang: session_core::i18n::Lang) -> Vec<(String, Value)> {
    fn resolve(v: &Value, lang: session_core::i18n::Lang) -> Value {
        match v {
            Value::String(s) => match s.strip_prefix('@') {
                Some(key) => Value::String(session_core::i18n::t(lang, key)),
                None => v.clone(),
            },
            Value::Array(items) => items.iter().map(|i| resolve(i, lang)).collect(),
            Value::Object(map) => map
                .iter()
                .map(|(k, v)| (k.clone(), resolve(v, lang)))
                .collect(),
            other => other.clone(),
        }
    }
    let all: Value =
        serde_json::from_str(include_str!("../../../../web/src/lib/agent-templates.json")).unwrap();
    all.as_object()
        .unwrap()
        .iter()
        .filter(|(name, _)| !name.starts_with('_'))
        .map(|(name, spec)| (name.clone(), resolve(spec, lang)))
        .collect()
}

#[tokio::test]
async fn every_setup_template_is_a_valid_draft_in_every_language() {
    let (fx, _tech_llm) = fixture(vec![text("ok")], tech_script()).await;
    for lang in session_core::i18n::Lang::ALL {
        let templates = templates_in(lang);
        assert_eq!(templates.len(), 5, "faq, support, leads, internal, blank");
        for (name, spec) in templates {
            let text = spec.to_string();
            assert!(
                !text.contains("\"@"),
                "{name} ({lang:?}) kept a key: {text}"
            );
            let (status, body) = fx
                .post(
                    &fx.alice,
                    "/api/v0/agents",
                    json!({ "name": format!("{name}-{}", lang.code()), "spec": spec }),
                )
                .await;
            assert_eq!(status, StatusCode::CREATED, "{name} ({lang:?}): {body}");
        }
    }
}
