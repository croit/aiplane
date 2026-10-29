// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The agent architect end to end (`docs/agent-builder.md` → "Agent architect"): a
//! person's chat that runs as the architect persona, driven by a scripted
//! model through the real router, worker and database.
//!
//! `alice` manages agents and holds the `get_current_timestamp` tool; `plain`
//! manages nothing. The one model upstream answers three kinds of calls: the
//! architect's own rounds (in script order), the prompt assistant's
//! structured proposal, and the draft agent's test turn.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use rama::Service;
use rama::http::{Body, Method, Request, StatusCode};
use serde_json::{Value, json};
use session_core::db::{self as chat, ToolCallStatus, TurnStatus};
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

const ARCHITECT_MARK: &str = "You are the agent architect";
const DRAFT_ANSWER: &str = "Hello, I am Harald.";

fn sse(delta: &Value) -> ResponseTemplate {
    let body = format!(
        "data: {}\n\ndata: [DONE]\n\n",
        json!({"choices": [{"index": 0, "delta": delta}]})
    );
    ResponseTemplate::new(200).set_body_raw(body, "text/event-stream")
}

fn call(id: &str, name: &str, args: Value) -> Value {
    json!({"tool_calls": [{"index": 0, "id": id, "type": "function",
        "function": {"name": name, "arguments": args.to_string()}}]})
}

fn proposal() -> Value {
    json!({
        "task": "You answer questions about Acme orders.",
        "tone": { "response": "Short and friendly.", "chips": ["friendly"] },
        "scope": { "topics": ["Acme orders"], "refusal": "I only help with Acme orders.",
                   "strict": false },
        "abilities": [],
        "slots": [],
        "identity": { "method": "none", "why": "public answers" },
        "handoffs": [],
        "tests": []
    })
}

/// Answers the architect's rounds from `script`, in order; anything else by
/// what it asks for.
struct Upstream {
    script: Vec<Value>,
    served: AtomicUsize,
}

impl wiremock::Respond for Upstream {
    fn respond(&self, req: &wiremock::Request) -> ResponseTemplate {
        let body: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
        if body.get("response_format").is_some() {
            return ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{ "message": { "role": "assistant",
                                            "content": proposal().to_string() } }],
                "usage": { "prompt_tokens": 10, "completion_tokens": 10, "total_tokens": 20 },
            }));
        }
        let system = body["messages"][0]["content"].as_str().unwrap_or_default();
        if !system.contains(ARCHITECT_MARK) {
            return sse(&json!({ "content": DRAFT_ANSWER }));
        }
        let i = self.served.fetch_add(1, Ordering::SeqCst);
        sse(&self.script[i.min(self.script.len() - 1)])
    }
}

fn chat_pool(upstream: &MockServer) -> UpstreamPoolConfig {
    UpstreamPoolConfig {
        voices: Default::default(),
        offer_voices: Vec::new(),
        allowed_groups: Vec::new(),
        fallback_offline: None,
        compliance: Default::default(),
        enforce_limits: true,
        kind: PoolKind::Chat,
        strategy: PickerStrategy::RoundRobin,
        models: Vec::new(),
        backend: vec![common::mock_backend("mock", &upstream.uri())],
    }
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

struct Fx {
    state: RamaState,
    llm: MockServer,
    alice: String,
    plain: String,
}

async fn fixture(script: Vec<Value>) -> Fx {
    let pool = db::open(std::path::Path::new(":memory:")).await.unwrap();
    let llm = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(Upstream {
            script,
            served: AtomicUsize::new(0),
        })
        .mount(&llm)
        .await;
    let mut pools = HashMap::new();
    pools.insert("pool".to_string(), chat_pool(&llm));
    let registry = upstreams::UpstreamRegistry::new(&pools).unwrap();
    common::seed_pool_models(&registry, "pool", 0, &["arch-model"]);
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
    );
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
    gateway_groups::set_tools_for_group(&pool, "managers", &["get_current_timestamp".into()])
        .await
        .unwrap();
    state.reload_rbac().await;
    let alice = person(&state, "alice", &["managers"]).await;
    let plain = person(&state, "plain", &[]).await;
    Fx {
        state,
        llm,
        alice,
        plain,
    }
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

    async fn get(&self, uri: &str) -> Value {
        let (status, body) = self.send(&self.alice, Method::GET, uri, None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    /// Start a new architect conversation and say `message` in it; the
    /// finished assistant turn.
    async fn converse(&self, message: &str) -> (String, chat::TurnWithTools) {
        let (status, started) = self
            .post(
                &self.alice,
                "/api/v0/agent-architect",
                json!({ "title": "Agent architect: new agent" }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{started}");
        let session_id = started["session_id"].as_str().unwrap().to_string();
        assert_eq!(started["model"], "arch-model");
        let (status, sent) = self
            .post(
                &self.alice,
                &format!("/api/v0/chat/sessions/{session_id}/messages"),
                json!({ "model": started["model"], "message": message }),
            )
            .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{sent}");
        let turn_id = sent["assistant_turn_id"].as_str().unwrap().to_string();
        for _ in 0..400 {
            let turn = chat::get_turn_with_tools(&self.state.db, &session_id, &turn_id)
                .await
                .unwrap()
                .unwrap();
            if turn.turn.status != TurnStatus::InProgress
                && self.state.chats.running_for_user("alice") == 0
            {
                return (session_id, turn);
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("the architect's turn never finished");
    }

    async fn agent_named(&self, name: &str) -> Value {
        let list = self.get("/api/v0/agents").await;
        let id = list["agents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["name"] == name)
            .unwrap_or_else(|| panic!("no agent {name}: {list}"))["id"]
            .as_str()
            .unwrap()
            .to_string();
        self.get(&format!("/api/v0/agents/{id}")).await["agent"].clone()
    }
}

fn output(turn: &chat::TurnWithTools, i: usize) -> Value {
    serde_json::from_str(turn.tool_calls[i].output_json.as_deref().unwrap_or("null"))
        .unwrap_or(Value::Null)
}

#[tokio::test]
async fn a_scripted_conversation_plans_creates_updates_and_tests_a_draft() {
    let fx = fixture(vec![
        call("c1", "list_agents", json!({})),
        call("c2", "create_agent_draft", json!({ "display": "Harald" })),
        call(
            "c3",
            "propose_setup",
            json!({ "agent_id": "harald", "scenario": "Answer questions about Acme orders." }),
        ),
        call(
            "c4",
            "update_agent_draft",
            json!({ "agent_id": "harald", "changes": {
                "model": "arch-model",
                "task": "You answer questions about Acme orders.",
                "abilities": [{ "id": "get_current_timestamp", "why": "delivery times" }],
                "slots": [{ "name": "order", "label": "Order number", "type": "text",
                            "choices": [] }]
            }}),
        ),
        call(
            "c5",
            "update_agent_draft",
            json!({ "agent_id": "harald", "changes": {
                "abilities": [{ "id": "run_in_sandbox", "why": "run code" }]
            }}),
        ),
        call(
            "c6",
            "run_test_turn",
            json!({ "agent_id": "harald", "message": "Hi" }),
        ),
        call("c7", "publish_agent", json!({ "agent_id": "harald" })),
        json!({ "content": "Harald is drafted. Review it and press Publish on /agents/harald." }),
    ])
    .await;

    let (session_id, turn) = fx.converse("Build me an agent for Acme orders").await;
    assert_eq!(turn.turn.status, TurnStatus::Completed, "{turn:?}");

    let calls: Vec<(&str, ToolCallStatus)> = turn
        .tool_calls
        .iter()
        .map(|c| (c.name.as_str(), c.status))
        .collect();
    assert_eq!(
        calls,
        [
            ("list_agents", ToolCallStatus::Completed),
            ("create_agent_draft", ToolCallStatus::Completed),
            ("propose_setup", ToolCallStatus::Completed),
            ("update_agent_draft", ToolCallStatus::Completed),
            ("update_agent_draft", ToolCallStatus::Errored),
            ("run_test_turn", ToolCallStatus::Completed),
            ("publish_agent", ToolCallStatus::Errored),
        ],
        "every call is recorded in the conversation"
    );
    assert_eq!(output(&turn, 0)["agents"], json!([]));
    assert_eq!(
        output(&turn, 2)["steps"]["task"]["orchestration"],
        proposal()["task"]
    );
    let updated = output(&turn, 3);
    assert_eq!(
        updated["changed"],
        json!(["model", "task", "abilities", "slots"])
    );
    let revision = updated["revision"].as_i64().expect("an undoable revision");
    let refused = turn.tool_calls[4]
        .output_json
        .as_deref()
        .unwrap_or_default();
    assert!(
        refused.contains("not an ability you may grant"),
        "{refused}"
    );
    assert_eq!(output(&turn, 5)["answer"], DRAFT_ANSWER);

    let harald = fx.agent_named("harald").await;
    assert_eq!(harald["display"], "Harald");
    assert_eq!(
        harald["live_version"],
        Value::Null,
        "the architect cannot publish"
    );
    let draft = &harald["draft_spec"];
    assert_eq!(draft["main"]["model"], "arch-model");
    assert_eq!(draft["main"]["tools"], json!(["get_current_timestamp"]));
    assert_eq!(draft["state"]["order"]["type"], "string");
    let mut grants: Vec<String> = harald["grants"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| format!("{}:{}", g["kind"], g["ref"]))
        .collect();
    grants.sort();
    assert_eq!(
        grants,
        [
            "\"model\":\"arch-model\"",
            "\"tool\":\"get_current_timestamp\""
        ],
        "nothing the person lacks was granted"
    );

    let architect = aiplane_agents::db::architect_sessions::get(&fx.state.db, &session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(architect.agent_id.as_deref(), harald["id"].as_str());

    let requests: Vec<Value> = fx
        .llm
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| serde_json::from_slice(&r.body).unwrap())
        .collect();
    let first = requests
        .iter()
        .find(|r| {
            r["messages"][0]["content"]
                .as_str()
                .is_some_and(|s| s.contains(ARCHITECT_MARK))
        })
        .unwrap();
    let mut offered: Vec<&str> = first["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["function"]["name"].as_str().unwrap())
        .collect();
    offered.sort();
    assert_eq!(
        offered,
        [
            "create_agent_draft",
            "list_agents",
            "list_grantable",
            "propose_setup",
            "read_agent",
            "run_test_turn",
            "update_agent_draft",
        ],
        "only the architect's tools, and no publish"
    );

    let id = harald["id"].as_str().unwrap();
    let (status, restored) = fx
        .post(
            &fx.alice,
            &format!("/api/v0/agents/{id}/draft/restore"),
            json!({ "revision": revision }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{restored}");
    assert_eq!(
        restored["draft_spec"],
        json!({ "profile": { "display": "Harald" } }),
        "back to the draft the create left, on the gateway's default chat model"
    );
    assert_eq!(
        restored["revoked"],
        json!([{ "kind": "tool", "ref": "get_current_timestamp" }]),
        "the undone change's tool goes, the default model the restored draft runs on stays"
    );
    assert!(
        restored["revision"].is_i64(),
        "an undo can itself be undone"
    );
    let after = fx.get(&format!("/api/v0/agents/{id}")).await["agent"].clone();
    let grants: Vec<String> = after["grants"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| {
            format!(
                "{}:{}",
                g["kind"].as_str().unwrap(),
                g["ref"].as_str().unwrap()
            )
        })
        .collect();
    assert_eq!(grants, ["model:arch-model"]);
    let events = fx
        .get(&format!(
            "/api/v0/agents/{id}/activity?kind=agent_draft_updated,grant_removed"
        ))
        .await;
    let kinds: Vec<&str> = events["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds[..2],
        ["grant_removed", "agent_draft_updated"],
        "{events}"
    );
    let undo = &events["events"][1]["detail"];
    assert_eq!(undo["restored"], revision);
    assert_eq!(undo["revoking"][0]["ref"], "get_current_timestamp");
}

/// Undoing a change keeps a grant the live version still uses.
#[tokio::test]
async fn undo_keeps_a_grant_the_live_version_uses() {
    let fx = fixture(vec![
        call("c1", "create_agent_draft", json!({ "display": "Live" })),
        call(
            "c2",
            "update_agent_draft",
            json!({ "agent_id": "live", "changes": {
                "task": "You tell the time.",
                "abilities": [{ "id": "get_current_timestamp", "why": "the time" }]
            }}),
        ),
        json!({ "content": "Done." }),
    ])
    .await;
    let (_, turn) = fx.converse("Make an agent called Live").await;
    let revision = output(&turn, 1)["revision"].as_i64().expect("a revision");
    let live = fx.agent_named("live").await;
    let id = live["id"].as_str().unwrap();
    let (status, published) = fx
        .post(
            &fx.alice,
            &format!("/api/v0/agents/{id}/publish"),
            json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{published}");

    let (status, restored) = fx
        .post(
            &fx.alice,
            &format!("/api/v0/agents/{id}/draft/restore"),
            json!({ "revision": revision }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{restored}");
    assert_eq!(restored["revoked"], json!([]));
    let after = fx.agent_named("live").await;
    assert!(
        after["grants"]
            .to_string()
            .contains("get_current_timestamp"),
        "{after}"
    );
}

#[tokio::test]
async fn reopening_continues_the_conversation_and_fresh_starts_a_new_one() {
    let fx = fixture(vec![json!({ "content": "Hi" })]).await;
    let start = |fresh: bool| {
        fx.post(
            &fx.alice,
            "/api/v0/agent-architect",
            json!({ "title": "Agent architect", "fresh": fresh }),
        )
    };
    let (status, first) = start(false).await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, again) = start(false).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again["session_id"], first["session_id"]);
    assert_eq!(again["resumed"], true);
    let (_, fresh) = start(true).await;
    assert_ne!(fresh["session_id"], first["session_id"]);

    let sessions = fx.get("/api/v0/chat/sessions").await;
    assert!(
        sessions.to_string().contains("Agent architect"),
        "the architect's conversations are the person's chats: {sessions}"
    );
}

#[tokio::test]
async fn only_an_agent_manager_may_talk_to_the_architect() {
    let fx = fixture(vec![json!({ "content": "Hi" })]).await;
    let (status, body) = fx
        .post(
            &fx.plain,
            "/api/v0/agent-architect",
            json!({ "title": "x" }),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let (status, body) = fx
        .post(
            &fx.alice,
            "/api/v0/agent-architect",
            json!({ "title": "x", "agent_id": "nobody-shared-this" }),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

async fn personal_converse(fx: &Fx) -> (String, chat::TurnWithTools) {
    let (status, started) = fx
        .post(
            &fx.alice,
            "/api/v0/agent-architect",
            json!({"title":"Plan a personal agent", "dot_owner":"personal", "fresh":true}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{started}");
    let id = started["session_id"].as_str().unwrap().to_string();
    let (status, sent) = fx
        .post(
            &fx.alice,
            &format!("/api/v0/chat/sessions/{id}/messages"),
            json!({"model":started["model"],"message":"Build a clock assistant"}),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{sent}");
    let turn_id = sent["assistant_turn_id"].as_str().unwrap();
    for _ in 0..400 {
        let turn = chat::get_turn_with_tools(&fx.state.db, &id, turn_id)
            .await
            .unwrap()
            .unwrap();
        if turn.turn.status != TurnStatus::InProgress
            && fx.state.chats.running_for_user("alice") == 0
        {
            return (id, turn);
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("personal planning did not finish");
}

async fn enable_personal(fx: &Fx) {
    gateway_groups::set_can_manage_dots(&fx.state.db, "managers", true)
        .await
        .unwrap();
    fx.state.reload_rbac().await;
}

#[tokio::test]
async fn personal_planning_consumes_no_slot_and_applies_only_the_reviewed_revision_once() {
    let fx=fixture(vec![call("p1","propose_dot_setup",json!({"name":"Clock","display":"My clock","description":"Time questions","changes":{"task":"Answer time questions.","abilities":[{"id":"get_current_timestamp","why":"current time"}]}})),json!({"content":"Review your agent."})]).await;
    enable_personal(&fx).await;
    let (id, turn) = personal_converse(&fx).await;
    assert_eq!(
        turn.tool_calls[0].status,
        ToolCallStatus::Completed,
        "{turn:?}"
    );
    assert!(
        aiplane_agents::db::agents::list_personal_dots(&fx.state.db, "alice")
            .await
            .unwrap()
            .is_empty()
    );
    let uri = format!("/api/v0/agent-architect/{id}/dot-plan");
    let (status, plan) = fx.send(&fx.alice, Method::GET, &uri, None).await;
    assert_eq!(status, StatusCode::OK, "{plan}");
    assert_eq!(plan["proposal"]["name"], "Clock");
    assert_eq!(
        plan["proposal"]["draft_spec"]["main"]["tools"],
        json!(["get_current_timestamp"])
    );
    assert_eq!(plan["dot_id"], Value::Null);
    let (status, _) = fx.post(&fx.alice, &uri, json!({"revision":"stale"})).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let revision = plan["proposal"]["revision"].clone();
    let (status, applied) = fx.post(&fx.alice, &uri, json!({"revision":revision})).await;
    assert_eq!(status, StatusCode::CREATED, "{applied}");
    let (status, repeated) = fx.post(&fx.alice, &uri, json!({"revision":revision})).await;
    assert_eq!(status, StatusCode::OK, "{repeated}");
    assert_eq!(repeated["dot"]["id"], applied["dot"]["id"]);
    assert_eq!(
        aiplane_agents::db::agents::list_personal_dots(&fx.state.db, "alice")
            .await
            .unwrap()
            .len(),
        1
    );
    let (status, _) = fx.send(&fx.plain, Method::GET, &uri, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn personal_planning_uses_dot_permission_without_agent_management() {
    let fx = fixture(vec![json!({"content":"What should your agent do?"})]).await;
    let (status, _) = fx
        .post(
            &fx.plain,
            "/api/v0/agent-architect",
            json!({"title":"My agent","dot_owner":"personal"}),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    gateway_groups::set_can_manage_dots(&fx.state.db, "everyone", true)
        .await
        .unwrap();
    fx.state.reload_rbac().await;
    let (status, body) = fx
        .post(
            &fx.plain,
            "/api/v0/agent-architect",
            json!({"title":"My agent","dot_owner":"personal"}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (status, _) = fx
        .post(
            &fx.plain,
            "/api/v0/agent-architect",
            json!({"title":"Ordinary agent"}),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn personal_apply_rechecks_permission_and_does_not_create_an_ordinary_agent() {
    let fx = fixture(vec![
        call("bad", "create_agent_draft", json!({"display":"Forbidden"})),
        call(
            "p1",
            "propose_dot_setup",
            json!({"name":"Clock","changes":{"task":"Answer time questions."}}),
        ),
        json!({"content":"Review."}),
    ])
    .await;
    enable_personal(&fx).await;
    let (id, turn) = personal_converse(&fx).await;
    assert_eq!(turn.tool_calls[0].status, ToolCallStatus::Errored);
    assert_eq!(turn.tool_calls[1].status, ToolCallStatus::Completed);
    assert_eq!(
        aiplane_agents::db::agents::publication_status(&fx.state.db)
            .await
            .unwrap()
            .len(),
        0
    );
    let uri = format!("/api/v0/agent-architect/{id}/dot-plan");
    let (_, plan) = fx.send(&fx.alice, Method::GET, &uri, None).await;
    gateway_groups::set_can_manage_dots(&fx.state.db, "managers", false)
        .await
        .unwrap();
    let (status, _) = fx
        .post(
            &fx.alice,
            &uri,
            json!({"revision":plan["proposal"]["revision"]}),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(
        aiplane_agents::db::agents::list_personal_dots(&fx.state.db, "alice")
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn a_personal_plan_reports_unsupported_requirements_and_refuses_incomplete_apply() {
    let fx=fixture(vec![call("p1","propose_dot_setup",json!({"name":"Clock","changes":{"task":"Summarize every morning.","schedule":"Every morning","scope":{"topics":["Time"],"refusal":"Only time","strict":true}}})),json!({"content":"Some requirements need setup."})]).await;
    enable_personal(&fx).await;
    let (id, turn) = personal_converse(&fx).await;
    assert_eq!(
        turn.tool_calls[0].status,
        ToolCallStatus::Completed,
        "{turn:?}"
    );
    let uri = format!("/api/v0/agent-architect/{id}/dot-plan");
    let (_, plan) = fx.send(&fx.alice, Method::GET, &uri, None).await;
    assert_eq!(plan["proposal"]["dropped"].as_array().unwrap().len(), 2);
    assert_eq!(plan["proposal"]["unresolved"].as_array().unwrap().len(), 2);
    assert!(plan["proposal"]["draft_spec"]["scope"].is_null());
    let (status, _) = fx
        .post(
            &fx.alice,
            &uri,
            json!({"revision":plan["proposal"]["revision"]}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(
        aiplane_agents::db::agents::list_personal_dots(&fx.state.db, "alice")
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn personal_apply_rechecks_disabled_tools_and_active_limit() {
    let fx=fixture(vec![call("p1","propose_dot_setup",json!({"name":"Clock","changes":{"task":"Answer time questions.","abilities":[{"id":"get_current_timestamp","why":"current time"}]}})),json!({"content":"Review."})]).await;
    enable_personal(&fx).await;
    let (id, _) = personal_converse(&fx).await;
    let uri = format!("/api/v0/agent-architect/{id}/dot-plan");
    let (_, plan) = fx.send(&fx.alice, Method::GET, &uri, None).await;
    aiplane_core::server::db::user_tool_prefs::set(
        &fx.state.db,
        "alice",
        "get_current_timestamp",
        false,
    )
    .await
    .unwrap();
    let (status, _) = fx
        .post(
            &fx.alice,
            &uri,
            json!({"revision":plan["proposal"]["revision"]}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    aiplane_core::server::db::user_tool_prefs::set(
        &fx.state.db,
        "alice",
        "get_current_timestamp",
        true,
    )
    .await
    .unwrap();
    for name in ["One", "Two", "Three", "Four"] {
        let (status, body) = fx
            .post(&fx.alice, "/api/v0/dots", json!({"name":name}))
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }
    let (status, body) = fx
        .post(
            &fx.alice,
            &uri,
            json!({"revision":plan["proposal"]["revision"]}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"]["code"], "active_dot_limit");
    assert_eq!(
        aiplane_agents::db::agents::list_personal_dots(&fx.state.db, "alice")
            .await
            .unwrap()
            .len(),
        4
    );
}

#[tokio::test]
async fn personal_apply_waits_for_an_active_planning_turn() {
    let fx = fixture(vec![
        call(
            "p1",
            "propose_dot_setup",
            json!({"name":"Clock","changes":{"task":"Answer time questions."}}),
        ),
        json!({"content":"Review."}),
    ])
    .await;
    enable_personal(&fx).await;
    let (id, _) = personal_converse(&fx).await;
    let uri = format!("/api/v0/agent-architect/{id}/dot-plan");
    let (_, plan) = fx.send(&fx.alice, Method::GET, &uri, None).await;
    let running = chat::create_assistant_turn_in_progress(
        &fx.state.db,
        &id,
        "running-plan-turn",
        "arch-model",
    )
    .await
    .unwrap();
    let (status, body) = fx
        .post(
            &fx.alice,
            &uri,
            json!({"revision":plan["proposal"]["revision"]}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"]["code"], "dot_plan_busy");
    chat::finalize_turn(&fx.state.db, &running.id, TurnStatus::Completed, None)
        .await
        .unwrap();
    let (status, body) = fx
        .post(
            &fx.alice,
            &uri,
            json!({"revision":plan["proposal"]["revision"]}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

#[tokio::test]
async fn a_new_accepted_user_request_invalidates_a_previous_dot_proposal_without_a_replacement() {
    let fx = fixture(vec![
        call(
            "p1",
            "propose_dot_setup",
            json!({"name":"Clock","changes":{"task":"Answer time questions."}}),
        ),
        json!({"content":"Review this plan."}),
        json!({"content":"Which reports should the new assistant summarize?"}),
    ])
    .await;
    enable_personal(&fx).await;
    let (id, _) = personal_converse(&fx).await;
    let uri = format!("/api/v0/agent-architect/{id}/dot-plan");
    let (_, plan) = fx.send(&fx.alice, Method::GET, &uri, None).await;
    let revision = plan["proposal"]["revision"].clone();
    let (status, sent) = fx
        .post(
            &fx.alice,
            &format!("/api/v0/chat/sessions/{id}/messages"),
            json!({"model":"arch-model","message":"Change it into a reporting assistant."}),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{sent}");
    let (status, current) = fx.send(&fx.alice, Method::GET, &uri, None).await;
    assert_eq!(status, StatusCode::OK, "{current}");
    assert_eq!(current["proposal"], Value::Null);
    let turn_id = sent["assistant_turn_id"].as_str().unwrap();
    for _ in 0..400 {
        let turn = chat::get_turn(&fx.state.db, &id, turn_id)
            .await
            .unwrap()
            .unwrap();
        if turn.status != TurnStatus::InProgress && fx.state.chats.running_for_user("alice") == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(
        chat::get_turn(&fx.state.db, &id, turn_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        TurnStatus::Completed
    );
    let (_, current) = fx.send(&fx.alice, Method::GET, &uri, None).await;
    assert_eq!(current["proposal"], Value::Null);
    let (status, _) = fx.post(&fx.alice, &uri, json!({"revision":revision})).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(
        aiplane_agents::db::agents::list_personal_dots(&fx.state.db, "alice")
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn personal_planning_exposes_actual_defaults_and_rejects_encoded_changes_without_an_approval_error()
 {
    let fx=fixture(vec![call("r1","list_grantable",json!({})),call("p1","propose_dot_setup",json!({"name":"Clock","changes":"{\"task\":\"Report the time\"}"})),call("p2","propose_dot_setup",json!({"name":"Clock","changes":{"task":"Report the time.","abilities":[{"id":"get_current_timestamp","why":"current time"}]}})),json!({"content":"Review this plan."})]).await;
    enable_personal(&fx).await;
    let (id, turn) = personal_converse(&fx).await;
    assert_eq!(turn.tool_calls[0].status, ToolCallStatus::Completed);
    assert_eq!(turn.tool_calls[1].status, ToolCallStatus::Errored);
    let invalid = turn.tool_calls[1].output_json.as_deref().unwrap();
    assert!(invalid.contains("expected a map"), "{invalid}");
    assert!(!invalid.to_lowercase().contains("approval"), "{invalid}");
    assert_eq!(turn.tool_calls[2].status, ToolCallStatus::Completed);
    let resources = output(&turn, 0);
    let time = resources["abilities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|ability| ability["id"] == "get_current_timestamp")
        .unwrap();
    assert_eq!(time["read_only"], true);
    assert_eq!(time["default_permission"], "always_allow");
    assert_eq!(resources["default_memory_tools"], json!([]));
    let (_, plan) = fx
        .send(
            &fx.alice,
            Method::GET,
            &format!("/api/v0/agent-architect/{id}/dot-plan"),
            None,
        )
        .await;
    assert_eq!(plan["proposal"]["unresolved"], json!([]));
    assert_eq!(plan["proposal"]["tools"][0]["permission"], "always_allow");
    assert_eq!(
        plan["proposal"]["tools"][0]["capability_key"],
        aiplane_runtime::server::tools::catalog::entry_key_for("get_current_timestamp")
    );
    assert!(
        aiplane_agents::db::agents::list_personal_dots(&fx.state.db, "alice")
            .await
            .unwrap()
            .is_empty()
    );
}
