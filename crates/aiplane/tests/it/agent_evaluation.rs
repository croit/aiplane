// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Agent evaluation end to end (`docs/agent-builder.md` → "Evaluation"): stored test cases run through the real run path on wiremock
//! upstreams, a Goal-Plan-Action report per case, and the publish guard.
//!
//! Agent `support` routes `technical` (no gate beyond the issue) and
//! `billing` (the issue and a `host`-written `verified` subject, binding the
//! customer) to two sub-agents. `alice` and `bob` hold `can_manage_agents`;
//! `plain` holds nothing.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use rama::Service;
use rama::http::{Body, Method, Request, StatusCode};
use serde_json::{Value, json};
use wiremock::matchers::{body_string_contains, method, path};
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

async fn llm(deltas: Vec<Value>) -> MockServer {
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
    json!({ "tool_calls": [{"index": 0, "id": id, "type": "function",
        "function": {"name": name, "arguments": args.to_string()}}] })
}

fn text(s: &str) -> Value {
    json!({ "content": s })
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

struct Fx {
    state: RamaState,
    alice: String,
    bob: String,
    plain: String,
    main_llm: MockServer,
    support: String,
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

fn finish_script() -> Vec<Value> {
    vec![call(
        "f1",
        "finish",
        json!({ "result": { "answer": "Done by the sub-agent." } }),
    )]
}

fn support_spec(tech: &str, billing: &str, extra_publish: Value) -> Value {
    json!({
        "main": {
            "model": "main-model",
            "instructions": { "orchestration": "Find the issue, then forward it." },
            "budget": { "rounds": 6 }
        },
        "state": {
            "issue": { "type": "enum", "values": ["billing", "technical"], "set_by": ["llm"] },
            "verified": { "type": "subject", "set_by": ["host"] }
        },
        "routes": {
            "technical": {
                "description": "Technical problems",
                "when": { "slot": "issue", "eq": "technical" },
                "agent": tech,
                "task": "Help with the technical issue."
            },
            "billing": {
                "description": "Invoices",
                "when": { "all": [
                    { "slot": "issue", "eq": "billing" },
                    { "slot": "verified", "provenance": "host" }
                ] },
                "agent": billing,
                "task": "Handle the invoice question of {verified.customer_id}.",
                "bind": { "customer": "state.verified.customer_id" }
            }
        },
        "publish": extra_publish
    })
}

async fn fixture(main_script: Vec<Value>) -> Fx {
    let pool = db::open(std::path::Path::new(":memory:")).await.unwrap();
    let main_llm = llm(main_script).await;
    let sub_llm = llm(finish_script()).await;
    let mut pools = HashMap::new();
    pools.insert("main-pool".to_string(), chat_pool(&main_llm));
    pools.insert("tech-pool".to_string(), chat_pool(&sub_llm));
    let registry = upstreams::UpstreamRegistry::new(&pools).unwrap();
    common::seed_pool_models(&registry, "main-pool", 0, &["main-model"]);
    common::seed_pool_models(&registry, "tech-pool", 0, &["tech-model"]);
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
    gateway_groups::set_tools_for_group(&pool, "everyone", &["get_current_timestamp".into()])
        .await
        .unwrap();
    state.reload_rbac().await;
    let alice = person(&state, "alice", &["managers"]).await;
    let bob = person(&state, "bob", &["managers"]).await;
    let plain = person(&state, "plain", &[]).await;
    let mut fx = Fx {
        state,
        alice,
        bob,
        plain,
        main_llm,
        support: String::new(),
    };
    fx.support = fx.build_support("", json!({})).await;
    fx
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

    async fn post(&self, uri: &str, body: Value) -> (StatusCode, Value) {
        self.send(&self.alice, Method::POST, uri, Some(body)).await
    }

    async fn get(&self, uri: &str) -> (StatusCode, Value) {
        self.send(&self.alice, Method::GET, uri, None).await
    }

    async fn create(&self, name: &str) -> String {
        let (status, body) = self.post("/api/v0/agents", json!({ "name": name })).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        body["agent"]["id"].as_str().unwrap().to_string()
    }

    async fn grant(&self, id: &str, kind: &str, reference: &str) {
        let (status, body) = self
            .post(
                &format!("/api/v0/system-principals/{id}/grants"),
                json!({ "kind": kind, "ref": reference }),
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

    async fn publish(&self, id: &str) -> (StatusCode, Value) {
        self.post(&format!("/api/v0/agents/{id}/publish"), json!({}))
            .await
    }

    async fn sub_agent(&self, name: &str, spec: Value) -> String {
        let id = self.create(name).await;
        self.grant(&id, "model", "tech-model").await;
        self.save_draft(&id, spec).await;
        let (status, body) = self.publish(&id).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        id
    }

    async fn build_support(&self, suffix: &str, publish_settings: Value) -> String {
        let finish = json!({ "schema": { "type": "object", "required": ["answer"],
            "properties": { "answer": { "type": "string" } } } });
        let tech = self
            .sub_agent(
                &format!("tech{suffix}"),
                json!({ "main": { "model": "tech-model",
                    "instructions": { "orchestration": "Solve it." } }, "finish": finish }),
            )
            .await;
        let billing = self.create(&format!("billing{suffix}")).await;
        self.grant(&billing, "model", "tech-model").await;
        self.grant(&billing, "tool", "get_current_timestamp").await;
        self.save_draft(
            &billing,
            json!({ "main": { "model": "tech-model",
                "instructions": { "orchestration": "Answer the invoice question." },
                "tools": ["get_current_timestamp"],
                "tool_resources": { "get_current_timestamp": {
                    "bind": { "customer_id": "route.customer" } } } },
                "finish": finish }),
        )
        .await;
        let (status, body) = self.publish(&billing).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");

        let support = self.create(&format!("support{suffix}")).await;
        self.grant(&support, "model", "main-model").await;
        self.save_draft(&support, support_spec(&tech, &billing, publish_settings))
            .await;
        support
    }

    async fn add_case(&self, name: &str, script: Value, expect: Value) -> String {
        let (status, body) = self
            .post(
                &format!("/api/v0/agents/{}/tests", self.support),
                json!({ "name": name, "script": script, "expect": expect }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        body["case"]["id"].as_str().unwrap().to_string()
    }

    async fn run_suite(&self, source: &str) -> (StatusCode, Value) {
        self.post(
            &format!("/api/v0/agents/{}/tests/run", self.support),
            json!({ "source": source }),
        )
        .await
    }
}

fn result<'a>(run: &'a Value, case: &str) -> &'a Value {
    run["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["case_name"] == case)
        .unwrap_or_else(|| panic!("no result for `{case}` in {run}"))
}

fn technical_script() -> Vec<Value> {
    vec![
        call("c1", "set_issue", json!({ "value": "technical" })),
        call("c2", "forward_request", json!({})),
        text("The technician looked into it."),
    ]
}

#[tokio::test]
async fn a_passing_suite_is_stored_with_a_goal_plan_action_report_per_case() {
    let mut script = technical_script();
    script.push(call("c3", "set_issue", json!({ "value": "billing" })));
    script.push(text("Please verify yourself first."));
    let fx = fixture(script).await;
    fx.add_case(
        "technical request is routed",
        json!([{ "say": "My router is broken" }]),
        json!({
            "gates": { "technical": { "open": true },
                       "billing": { "open": false, "missing": ["verified"] } },
            "route": "technical",
            "sub_agents": { "called": ["technical"], "not_called": ["billing"] },
            "tools": { "called": ["set_issue", "forward_request"], "not_called": ["get_current_timestamp"] },
            "answer": { "contains": ["technician"], "not_contains": ["refund"] },
            "filter": "passed",
            "finished": true
        }),
    )
    .await;
    fx.add_case(
        "billing waits for verification",
        json!([{ "say": "A question about my invoice" }]),
        json!({
            "gates": { "billing": { "open": false, "missing": ["verified"] } },
            "route": null,
            "sub_agents": { "not_called": ["billing", "technical"] },
            "finished": true
        }),
    )
    .await;

    let (status, run) = fx.run_suite("draft").await;

    assert_eq!(status, StatusCode::CREATED, "{run}");
    assert_eq!(run["green"], true, "{run}");
    assert_eq!(
        (run["passed"].as_i64(), run["failed"].as_i64()),
        (Some(2), Some(0))
    );
    assert_eq!(run["source"], "draft");
    let first = result(&run, "technical request is routed");
    assert_eq!(first["passed"], true, "{first}");
    for section in ["goal", "plan", "action"] {
        assert_eq!(first["report"][section]["passed"], true, "{first}");
    }
    assert_eq!(
        first["report"]["plan"]["checks"].as_array().unwrap().len(),
        4
    );
    assert_eq!(
        first["report"]["turns"][0]["answer"],
        "The technician looked into it."
    );
    assert!(first["report"]["rubric"].is_null());

    let (status, stored) = fx
        .get(&format!(
            "/api/v0/agents/{}/test-runs/{}",
            fx.support,
            run["id"].as_str().unwrap()
        ))
        .await;
    assert_eq!(status, StatusCode::OK, "{stored}");
    assert_eq!(stored["results"].as_array().unwrap().len(), 2);
    let (_, runs) = fx
        .get(&format!("/api/v0/agents/{}/test-runs", fx.support))
        .await;
    assert_eq!(runs["runs"][0]["id"], run["id"]);
    assert!(runs["runs"][0].get("results").is_none());
}

#[tokio::test]
async fn a_gate_expectation_the_agent_does_not_meet_fails_the_plan_only() {
    let fx = fixture(technical_script()).await;
    fx.add_case(
        "wrongly expects a closed gate",
        json!([{ "say": "My router is broken" }]),
        json!({ "gates": { "technical": { "open": false } }, "finished": true }),
    )
    .await;

    let (status, run) = fx.run_suite("draft").await;

    assert_eq!(status, StatusCode::CREATED, "{run}");
    assert_eq!(run["green"], false);
    let case = result(&run, "wrongly expects a closed gate");
    assert_eq!(case["passed"], false);
    let report = &case["report"];
    assert_eq!(report["goal"]["passed"], true);
    assert_eq!(report["plan"]["passed"], false);
    assert_eq!(report["action"]["passed"], true);
    let message = report["plan"]["checks"][0]["message"].as_str().unwrap();
    assert!(
        message.contains("route `technical` is open, expected closed"),
        "{message}"
    );
}

#[tokio::test]
async fn a_route_expectation_the_router_does_not_meet_fails_with_the_route_it_picked() {
    let fx = fixture(technical_script()).await;
    fx.add_case(
        "expects billing",
        json!([{ "say": "My router is broken" }]),
        json!({ "route": "billing" }),
    )
    .await;

    let (_, run) = fx.run_suite("draft").await;

    let report = &result(&run, "expects billing")["report"];
    assert_eq!(report["passed"], false);
    assert_eq!(report["plan"]["checks"][0]["actual"], "technical");
    let message = report["plan"]["checks"][0]["message"].as_str().unwrap();
    assert!(
        message.contains("picked `technical`, expected `billing`"),
        "{message}"
    );
}

#[tokio::test]
async fn a_trusted_write_between_messages_opens_the_gate_and_the_bound_value_is_checked() {
    let fx = fixture(vec![
        call("c1", "set_issue", json!({ "value": "billing" })),
        text("Who am I speaking to?"),
        call("c2", "forward_request", json!({})),
        text("Your invoice is handled."),
    ])
    .await;
    fx.add_case(
        "verified customer reaches billing",
        json!([
            { "say": "Question about my invoice" },
            { "write": { "slot": "verified", "value": { "customer_id": "C-1" }, "writer": "host" } },
            { "say": "I am verified now" }
        ]),
        json!({
            "gates": { "billing": { "open": true } },
            "route": "billing",
            "sub_agents": { "called": ["billing"] },
            "bound": [{ "route": "billing", "name": "customer", "equals": "C-1" }],
            "answer": { "contains": ["handled"] }
        }),
    )
    .await;
    fx.add_case(
        "wrong customer is caught",
        json!([{ "say": "x" }]),
        json!({ "bound": [{ "route": "billing", "name": "customer", "equals": "C-2" }] }),
    )
    .await;

    let (status, run) = fx.run_suite("draft").await;

    assert_eq!(status, StatusCode::CREATED, "{run}");
    let verified = result(&run, "verified customer reaches billing");
    assert_eq!(verified["passed"], true, "{verified}");
    let slots = verified["report"]["debug"]["slots"].as_array().unwrap();
    let slot = slots.iter().find(|s| s["slot"] == "verified").unwrap();
    assert_eq!(slot["provenance"], "host");
    assert_eq!(result(&run, "wrong customer is caught")["passed"], false);
}

#[tokio::test]
async fn a_script_cannot_write_a_slot_the_spec_does_not_let_that_writer_set() {
    let fx = fixture(vec![text("Hello.")]).await;
    fx.add_case(
        "writes the model's slot as the host",
        json!([
            { "say": "hi" },
            { "write": { "slot": "issue", "value": "technical", "writer": "host" } }
        ]),
        json!({ "finished": true }),
    )
    .await;

    let (_, run) = fx.run_suite("draft").await;

    let case = result(&run, "writes the model's slot as the host");
    assert_eq!(case["passed"], false);
    let error = case["report"]["error"].as_str().unwrap();
    assert!(error.contains("could not write `issue`"), "{error}");
}

#[tokio::test]
async fn the_output_filter_outcome_is_an_expectation() {
    let fx = fixture(vec![text("Your invoice is RE-123456.")]).await;
    let (status, body) = fx
        .send(
            &fx.alice,
            Method::PUT,
            &format!("/api/v0/agents/{}/draft", fx.support),
            Some(json!({ "spec": {
                "main": { "model": "main-model",
                          "instructions": { "orchestration": "Answer." } },
                "publish": { "output_filter": {
                    "patterns": { "invoice": "RE-\\d{6}" }, "action": "withhold" } }
            } })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    fx.add_case(
        "an untraceable invoice number is withheld",
        json!([{ "say": "What is my invoice number?" }]),
        json!({ "filter": "withheld", "answer": { "not_contains": ["RE-123456"] }, "finished": true }),
    )
    .await;
    fx.add_case(
        "wrongly expects the filter to pass",
        json!([{ "say": "again" }]),
        json!({ "filter": "passed" }),
    )
    .await;

    let (_, run) = fx.run_suite("draft").await;

    let withheld = result(&run, "an untraceable invoice number is withheld");
    assert_eq!(withheld["passed"], true, "{withheld}");
    let wrong = result(&run, "wrongly expects the filter to pass");
    assert_eq!(wrong["passed"], false);
    assert_eq!(wrong["report"]["goal"]["checks"][0]["actual"], "withheld");
}

#[tokio::test]
async fn a_version_is_tested_as_it_was_published_while_the_draft_moves_on() {
    let fx = fixture(vec![text("Version one answers.")]).await;
    let (status, body) = fx.publish(&fx.support).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    fx.add_case(
        "answers",
        json!([{ "say": "hello" }]),
        json!({ "finished": true }),
    )
    .await;
    let (status, body) = fx.run_suite("version:7").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (status, body) = fx.run_suite("whenever").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    let (status, run) = fx.run_suite("version:1").await;

    assert_eq!(status, StatusCode::CREATED, "{run}");
    assert_eq!(run["source"], "version:1");
    assert_eq!(run["version"], 1);
    assert_eq!(run["green"], true);
}

#[tokio::test]
async fn publishing_is_blocked_until_the_suite_is_green_for_this_draft_when_the_setting_is_on() {
    let fx = fixture(vec![text("An answer.")]).await;
    let guarded = fx
        .build_support("-guarded", json!({ "require_passing_tests": true }))
        .await;
    // `fx.support` keeps the unguarded agent; the guard test works on its own.
    let tests = |suffix: &str| format!("/api/v0/agents/{guarded}/{suffix}");
    let add = |name: &str, expect: Value| {
        let tests = tests("tests");
        let fx = &fx;
        let name = name.to_string();
        async move {
            let (status, body) = fx
                .post(
                    &tests,
                    json!({ "name": name, "script": [{ "say": "hello" }], "expect": expect }),
                )
                .await;
            assert_eq!(status, StatusCode::CREATED, "{body}");
            body["case"]["id"].as_str().unwrap().to_string()
        }
    };

    let (status, body) = fx.publish(&guarded).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["error"]["code"], "agent_tests_failing");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("no test cases")
    );

    let failing = add("expects a route", json!({ "route": "technical" })).await;
    let (status, body) = fx.publish(&guarded).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("run the suite")
    );

    let (status, run) = fx
        .post(&tests("tests/run"), json!({ "source": "draft" }))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{run}");
    assert_eq!(run["green"], false);
    let (status, body) = fx.publish(&guarded).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let listed = body["error"]["failing"].as_array().unwrap();
    assert_eq!(listed.len(), 1, "{body}");
    assert_eq!(listed[0]["case_name"], "expects a route");
    assert!(
        listed[0]["problems"][0].as_str().unwrap().contains("plan:"),
        "{body}"
    );

    let (status, body) = fx
        .send(
            &fx.alice,
            Method::PUT,
            &tests(&format!("tests/{failing}")),
            Some(
                json!({ "name": "expects a route", "script": [{ "say": "hello" }],
                         "expect": { "route": null } }),
            ),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = fx.publish(&guarded).await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "an edited suite is not what ran: {body}"
    );

    let (_, run) = fx
        .post(&tests("tests/run"), json!({ "source": "draft" }))
        .await;
    assert_eq!(run["green"], true, "{run}");
    let (_, listing) = fx.get(&tests("tests")).await;
    assert_eq!(listing["latest_draft_run_current"], true);
    let (status, body) = fx.publish(&guarded).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let (_, detail) = fx.get(&format!("/api/v0/agents/{guarded}")).await;
    let mut draft = detail["agent"]["draft_spec"].clone();
    draft["main"]["instructions"]["response"] = json!("Be brief.");
    fx.save_draft(&guarded, draft).await;
    let (status, body) = fx.publish(&guarded).await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a changed draft needs a new run: {body}"
    );
}

/// The at-rest key rotation re-seals every credential in a draft, which
/// gives it new ciphertext but leaves its meaning alone: a green run stays
/// the evidence for it. Done here as the pass does it, by sealing the same
/// credential again — what the run tested is unchanged.
#[tokio::test]
async fn a_resealed_credential_leaves_a_green_run_current() {
    let fx = fixture(vec![text("An answer.")]).await;
    let guarded = fx
        .build_support("-sealed", json!({ "require_passing_tests": true }))
        .await;
    let (_, detail) = fx.get(&format!("/api/v0/agents/{guarded}")).await;
    let mut draft = detail["agent"]["draft_spec"].clone();
    draft["verifiers"] = json!({ "site": {
        "kind": "host_jwt", "algorithm": "HS256",
        "secret": "the-website-and-the-agent-share-this-secret",
        "issuer": "https://www.example.com", "audience": "support",
        "claims": { "verified": { "customer_id": "sub" } } } });
    fx.save_draft(&guarded, draft).await;
    let tests = |suffix: &str| format!("/api/v0/agents/{guarded}/{suffix}");
    let (status, body) = fx
        .post(
            &tests("tests"),
            json!({ "name": "answers", "script": [{ "say": "hello" }],
                    "expect": { "route": null } }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (_, run) = fx
        .post(&tests("tests/run"), json!({ "source": "draft" }))
        .await;
    assert_eq!(run["green"], true, "{run}");

    let stored: String = sqlx::query_scalar("SELECT draft_spec FROM agents WHERE principal_id = ?")
        .bind(&guarded)
        .fetch_one(&fx.state.db)
        .await
        .unwrap();
    let mut stored: Value = serde_json::from_str(&stored).unwrap();
    let sealed = stored["verifiers"]["site"]["secret_sealed"]
        .as_str()
        .unwrap()
        .to_string();
    let plain = fx.state.crypto.open_from_string(&sealed).unwrap();
    let resealed = fx.state.crypto.seal_to_string(&plain).unwrap();
    assert_ne!(sealed, resealed);
    stored["verifiers"]["site"]["secret_sealed"] = json!(resealed);
    sqlx::query("UPDATE agents SET draft_spec = ? WHERE principal_id = ?")
        .bind(stored.to_string())
        .bind(&guarded)
        .execute(&fx.state.db)
        .await
        .unwrap();

    let (_, listing) = fx.get(&tests("tests")).await;
    assert_eq!(listing["latest_draft_run_current"], true, "{listing}");
    let (status, body) = fx.publish(&guarded).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

#[tokio::test]
async fn a_suite_run_leaves_the_analytics_untouched() {
    let fx = fixture(technical_script()).await;
    fx.add_case(
        "routes",
        json!([{ "say": "My router is broken" }]),
        json!({ "route": "technical" }),
    )
    .await;
    let analytics = format!("/api/v0/agents/{}/analytics", fx.support);
    let (_, before) = fx.get(&analytics).await;

    let (status, run) = fx.run_suite("draft").await;
    assert_eq!(status, StatusCode::CREATED, "{run}");
    assert_eq!(run["green"], true, "{run}");

    let (_, after) = fx.get(&analytics).await;
    assert_eq!(after["conversations"], 0);
    assert_eq!(after["turns"], 0);
    assert_eq!(after["routes_chosen"], json!({}));
    assert_eq!(after["sub_agents"]["dispatched"], 0);
    let counted = |mut a: Value| {
        a.as_object_mut().unwrap().remove("from");
        a.as_object_mut().unwrap().remove("to");
        a
    };
    assert_eq!(counted(before), counted(after));

    let sessions: Vec<(Option<i64>,)> = sqlx::query_as(
        "SELECT agent_version FROM chat_sessions WHERE principal_id = ? AND parent_turn_id IS NULL",
    )
    .bind(&fx.support)
    .fetch_all(&fx.state.db)
    .await
    .unwrap();
    assert_eq!(
        sessions,
        [(Some(0),)],
        "the case ran as a test conversation"
    );
}

#[tokio::test]
async fn the_rubric_is_judged_apart_and_never_changes_the_deterministic_result() {
    let fx = fixture(vec![text("It is fine.")]).await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_string_contains("grade a conversation"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{ "message": { "content":
                json!({ "passed": false, "reason": "It was not apologetic." }).to_string() } }]
        })))
        .with_priority(1)
        .mount(&fx.main_llm)
        .await;
    let (status, body) = fx
        .post(
            &format!("/api/v0/agents/{}/tests", fx.support),
            json!({ "name": "graded", "script": [{ "say": "My order is late" }],
                    "expect": { "finished": true }, "rubric": "The answer apologises." }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let (_, run) = fx.run_suite("draft").await;

    let case = result(&run, "graded");
    assert_eq!(case["passed"], true, "{case}");
    assert_eq!(case["report"]["rubric"]["verdict"], "failed", "{case}");
    assert_eq!(case["report"]["rubric"]["reason"], "It was not apologetic.");
    assert_eq!(run["green"], true);
    assert_eq!(run["rubric"]["failed"], 1);
}

#[tokio::test]
async fn cases_are_validated_when_stored_and_names_are_unique() {
    let fx = fixture(vec![text("ok")]).await;
    let uri = format!("/api/v0/agents/{}/tests", fx.support);

    for (script, expect, at) in [
        (json!([]), json!({ "finished": true }), "script"),
        (
            json!([{ "write": { "slot": "verified", "value": {}, "writer": "host" } }]),
            json!({ "finished": true }),
            "script[0]",
        ),
        (
            json!([{ "say": "x" }, { "write": { "slot": "issue", "value": "a", "writer": "llm" } }]),
            json!({ "finished": true }),
            "script[1].write.writer",
        ),
        (json!([{ "say": "x" }]), json!({}), "expect"),
        (json!([{ "say": "x" }]), json!({ "answers": {} }), "expect"),
    ] {
        let (status, body) = fx
            .post(
                &uri,
                json!({ "name": "bad", "script": script, "expect": expect }),
            )
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert_eq!(body["error"]["code"], "invalid_test_case");
        assert_eq!(body["error"]["issues"][0]["path"], at, "{body}");
    }

    fx.add_case("one", json!([{ "say": "x" }]), json!({ "finished": true }))
        .await;
    let (status, body) = fx
        .post(
            &uri,
            json!({ "name": "one", "script": [{ "say": "x" }], "expect": { "finished": true } }),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");

    let (status, body) = fx.run_suite("draft").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (_, listing) = fx.get(&uri).await;
    let id = listing["cases"][0]["id"].as_str().unwrap().to_string();
    let (status, _) = fx
        .send(&fx.alice, Method::DELETE, &format!("{uri}/{id}"), None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, body) = fx.run_suite("draft").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "no cases left: {body}");
}

#[tokio::test]
async fn the_tests_follow_the_share_rules_of_the_other_agent_routes() {
    let fx = fixture(vec![text("ok")]).await;
    let uri = format!("/api/v0/agents/{}/tests", fx.support);
    let case = json!({ "name": "c", "script": [{ "say": "x" }], "expect": { "finished": true } });

    let (status, _) = fx.send(&fx.plain, Method::GET, &uri, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "no agent permission");
    let (status, _) = fx.send(&fx.bob, Method::GET, &uri, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "no share");

    let (status, _) = fx
        .post(
            &format!("/api/v0/agents/{}/shares", fx.support),
            json!({ "subject_kind": "user", "subject_id": "bob", "access": "read" }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = fx.send(&fx.bob, Method::GET, &uri, None).await;
    assert_eq!(status, StatusCode::OK, "a read share lists the cases");
    let (status, _) = fx.send(&fx.bob, Method::POST, &uri, Some(case)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "writing a case needs write");
    let (status, _) = fx
        .send(
            &fx.bob,
            Method::POST,
            &format!("{uri}/run"),
            Some(json!({ "source": "draft" })),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a run drives the agent's tools"
    );
    let (status, _) = fx
        .send(
            &fx.bob,
            Method::GET,
            &format!("/api/v0/agents/{}/test-runs", fx.support),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
}
