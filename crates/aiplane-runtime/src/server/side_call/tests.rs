// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use std::collections::HashMap;

use aiplane_core::server::db::limits::{self, Dimension, SubjectType, Window};
use aiplane_core::server::principal::GrantSet;
use aiplane_core::server::run_chain::Frame;
use aiplane_core::server::upstreams::{
    self,
    config::{BackendConfig, PickerStrategy, UpstreamPoolConfig},
};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;

const MODEL: &str = "side-model";

async fn upstream(content: Value) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{ "message": { "role": "assistant", "content": content } }],
            "usage": { "prompt_tokens": 30, "completion_tokens": 12, "total_tokens": 42 },
        })))
        .mount(&server)
        .await;
    server
}

/// A gateway with one chat pool serving [`MODEL`] on `upstream`, usage
/// metrics on and limits enforced.
async fn gateway(upstream: &MockServer) -> RamaState {
    let db = aiplane_core::server::db::open(std::path::Path::new(":memory:"))
        .await
        .unwrap();
    let pools = HashMap::from([(
        "pool".to_string(),
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
            backend: vec![BackendConfig {
                priority: Default::default(),
                alias: Default::default(),
                supports_edit: false,
                extra_headers: Default::default(),
                enabled: true,
                name: "mock".into(),
                base_url: upstream.uri(),
                api_key_env: None,
                api_key: None,
                weight: 1,
                max_inflight: 16,
                health_path: "/models".into(),
                models: Vec::new(),
            }],
        },
    )]);
    let registry = upstreams::UpstreamRegistry::new(&pools).unwrap();
    registry.pools()[0].backends[0].set_models([MODEL.to_string()].into());
    let config = aiplane_core::server::Config {
        gateway: aiplane_core::server::config::GatewayConfig {
            upstream_wait_secs: 0,
            ..Default::default()
        },
        ..Default::default()
    };
    let app = crate::server::AppState::new(
        config,
        db.clone(),
        registry,
        Arc::new(crate::server::tools::ToolRegistry::new()),
        Arc::new(aiplane_core::server::rbac::Resolver::empty()),
    );
    let sessions = aiplane_core::rama_server::SessionStore::new(db.clone(), [7u8; 32]);
    RamaState::new(app, sessions, aiplane_core::server::usage::spawn(db, 90))
}

fn call<'a>(access: &'a PoolAccess, input: &'a str, no_think: bool) -> SideCall<'a> {
    SideCall {
        purpose: "test_purpose",
        model: MODEL,
        access,
        instructions: "Answer.",
        input,
        temperature: 0.0,
        max_tokens: Some(64),
        no_think,
        timeout: Duration::from_secs(10),
    }
}

fn shape() -> JsonShape<'static> {
    JsonShape {
        name: "verdict",
        schema: json!({ "type": "object", "properties": { "ok": { "type": "boolean" } } }),
    }
}

fn alice() -> Payer {
    Payer::person(
        "alice",
        &[],
        Some("alice@example.com".into()),
        UsageSource::Chat,
    )
}

fn agent_principal() -> SystemPrincipal {
    SystemPrincipal {
        id: "agent-1".into(),
        name: "support".into(),
        grants: Arc::new(GrantSet::default()),
    }
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

async fn usage_rows(state: &RamaState) -> Vec<(String, String, String, Option<String>, i64)> {
    tokio::time::sleep(Duration::from_millis(800)).await;
    sqlx::query_as(
        "SELECT user_id, source, model, agent_id, total_tokens FROM usage_events ORDER BY created_at",
    )
    .fetch_all(&state.db)
    .await
    .unwrap()
}

#[tokio::test]
async fn a_json_call_switches_reasoning_off_and_reads_a_fenced_object() {
    let server = upstream(json!("```json\n{\"ok\": true}\n```")).await;
    let state = gateway(&server).await;

    let answered = ask_json(
        &state,
        &alice(),
        call(&PoolAccess::all(), "data", true),
        shape(),
    )
    .await;

    assert_eq!(answered.tokens(), 42);
    assert_eq!(answered.answer.unwrap(), json!({ "ok": true }));
    let body = &sent(&server).await[0];
    assert_eq!(body["model"], MODEL);
    assert_eq!(body["chat_template_kwargs"]["enable_thinking"], false);
    assert_eq!(body["stream"], false);
    assert_eq!(body["max_tokens"], 64);
    assert_eq!(body["messages"][0]["content"], "Answer.");
    assert_eq!(body["messages"][1]["content"], "data\n\n/no_think");
    assert_eq!(body["response_format"]["json_schema"]["name"], "verdict");
    assert_eq!(body["response_format"]["json_schema"]["strict"], true);
    assert_eq!(answered.exchange.request, *body);
    assert_eq!(answered.exchange.backend.as_deref(), Some("mock"));
}

#[tokio::test]
async fn a_call_is_a_usage_row_of_its_payer() {
    let server = upstream(json!("Short title")).await;
    let state = gateway(&server).await;

    let answered = ask_text(&state, &alice(), call(&PoolAccess::all(), "hello", false)).await;

    assert_eq!(answered.answer.unwrap(), "Short title");
    assert_eq!(sent(&server).await[0]["messages"][1]["content"], "hello");
    assert_eq!(
        usage_rows(&state).await,
        [(
            "alice".to_string(),
            "chat".to_string(),
            MODEL.to_string(),
            None,
            42
        )]
    );
}

#[tokio::test]
async fn an_agents_call_is_booked_to_the_agent_at_the_root_of_its_run() {
    let server = upstream(json!("{\"ok\": false}")).await;
    let state = gateway(&server).await;
    let principal = agent_principal();
    let run = Arc::new(RunChain::root(
        "conversation",
        None,
        Frame::for_principal(&principal, Some(1)),
    ));

    let answered = ask_json(
        &state,
        &Payer::agent(&principal, Some(run)),
        call(&PoolAccess::all(), "data", false),
        shape(),
    )
    .await;

    assert_eq!(answered.answer.unwrap(), json!({ "ok": false }));
    assert_eq!(
        usage_rows(&state).await,
        [(
            "agent-1".to_string(),
            "agent".to_string(),
            MODEL.to_string(),
            Some("agent-1".to_string()),
            42
        )]
    );
}

#[tokio::test]
async fn a_persons_spent_limit_refuses_before_the_model_is_called() {
    let server = upstream(json!("never")).await;
    let state = gateway(&server).await;
    limits::upsert(
        &state.db,
        SubjectType::User,
        "alice",
        None,
        Dimension::Requests,
        Window::Hour,
        0.0,
    )
    .await
    .unwrap();

    let answered = ask_text(&state, &alice(), call(&PoolAccess::all(), "hello", false)).await;

    let Err(SideCallError::OverBudget(exceeded)) = answered.answer else {
        panic!("the limit refuses the call");
    };
    assert_eq!(exceeded.dimension, Dimension::Requests);
    assert!(
        answered
            .exchange
            .error
            .as_deref()
            .unwrap()
            .contains("limit reached")
    );
    assert!(sent(&server).await.is_empty(), "the model is never asked");
    assert!(usage_rows(&state).await.is_empty());
}

#[tokio::test]
async fn an_agents_spent_operator_limit_refuses_before_the_model_is_called() {
    let server = upstream(json!("never")).await;
    let state = gateway(&server).await;
    limits::upsert(
        &state.db,
        SubjectType::System,
        "agent-1",
        None,
        Dimension::Requests,
        Window::Hour,
        0.0,
    )
    .await
    .unwrap();

    let answered = ask_json(
        &state,
        &Payer::agent(&agent_principal(), None),
        call(&PoolAccess::all(), "data", false),
        shape(),
    )
    .await;

    assert!(matches!(answered.answer, Err(SideCallError::OverBudget(_))));
    assert!(sent(&server).await.is_empty(), "the model is never asked");
}

#[tokio::test]
async fn an_upstream_error_is_a_failure_that_names_the_status() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503).set_body_string("overloaded"))
        .mount(&server)
        .await;
    let state = gateway(&server).await;

    let answered = ask_text(&state, &alice(), call(&PoolAccess::all(), "hello", false)).await;

    let err = answered.answer.unwrap_err().to_string();
    assert!(err.contains("503") && err.contains("overloaded"), "{err}");
    assert_eq!(answered.exchange.status, Some(503));
    assert_eq!(answered.exchange.error.as_deref(), Some(err.as_str()));
}

#[tokio::test]
async fn a_text_answer_without_content_reads_as_empty() {
    let server = upstream(Value::Null).await;
    let state = gateway(&server).await;

    let answered = ask_text(&state, &alice(), call(&PoolAccess::all(), "hello", false)).await;

    assert_eq!(answered.answer.unwrap(), "");
}

#[test]
fn a_json_object_is_found_inside_prose() {
    assert_eq!(
        json_object("Here you go: {\"a\": \"}\"} hope it helps").unwrap(),
        json!({ "a": "}" })
    );
    assert!(json_object("[1, 2]").is_err());
    assert!(json_object("no object here").is_err());
}

#[test]
fn a_think_block_is_stripped_only_when_balanced() {
    assert_eq!(strip_think_block("<THINK>x</THINK>Title"), "Title");
    assert_eq!(strip_think_block("<think>open"), "<think>open");
    assert_eq!(strip_think_block("plain"), "plain");
}
