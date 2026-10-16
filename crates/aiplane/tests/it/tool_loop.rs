// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Integration coverage for the tool-call loop on the rama proxy.
//!
//! Drives a two-round chat completion: the wiremock upstream returns a
//! `tool_calls` array on round 1 (calling `company_echo`), then a
//! normal assistant reply on round 2. The gateway should run the tool
//! between rounds, append the result to the messages, and relay the
//! final response with `x-gateway-tool-rounds: 1`.

use crate::common;

use std::collections::HashMap;
use std::sync::Arc;

use aiplane::rama_server::{RamaState, SessionStore, router::router};
use aiplane_core::server::config::Config;
use aiplane_core::server::db;
use aiplane_core::server::db::{tokens, users};
use aiplane_core::server::rbac::Resolver;
use aiplane_core::server::rbac::config::{RbacConfig, RoleConfig, RoleMapping};
use aiplane_core::server::upstreams::{
    self,
    config::{BackendConfig, PickerStrategy, PoolKind, UpstreamPoolConfig},
};
use aiplane_runtime::server::AppState;
use aiplane_runtime::server::tools::{ToolRegistry, echo::Echo, time::CurrentTimestamp};
use common::Service as _;
use jiff::{SignedDuration, Timestamp};
use rama::http::{Body, Method, Request, StatusCode};
use serde_json::json;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Build a state where role "engineer" grants `company_echo`, OIDC
/// role "engineering" maps to "engineer", and the chat pool points at
/// `upstream_uri`.
async fn state_with_tools(upstream_uri: &str) -> RamaState {
    state_with_tool_grants(upstream_uri, vec!["company_echo".into()]).await
}

async fn state_with_tool_grants(upstream_uri: &str, granted_tools: Vec<String>) -> RamaState {
    let pool = db::open(std::path::Path::new(":memory:")).await.unwrap();
    let mut pools = HashMap::new();
    pools.insert(
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
                base_url: upstream_uri.into(),
                api_key_env: None,
                api_key: None,
                weight: 1,
                max_inflight: 16,
                health_path: "/models".into(),
                models: Vec::new(),
            }],
        },
    );
    let registry = upstreams::UpstreamRegistry::new(&pools).unwrap();
    common::seed_pool_models(&registry, "pool", 0, &["model-a"]);

    // Real tool registry (Echo + CurrentTimestamp) so company_echo is
    // dispatchable; `browser_control` as the chat-only tool a token panel
    // must not offer.
    let tools = Arc::new(
        ToolRegistry::new()
            .with(Echo)
            .with(CurrentTimestamp)
            .with(aiplane_tools::browser_control::BrowserControl),
    );

    let rbac = Resolver::build(
        RbacConfig {
            default_role: None,
            mappings: vec![RoleMapping {
                oidc_value: "engineering".into(),
                role: "engineer".into(),
            }],
        },
        vec![RoleConfig {
            id: "engineer".into(),
            admin: false,
            tools: granted_tools,
            skills: vec![],
        }],
    )
    .unwrap();

    let app = AppState::new(
        Config::default(),
        pool.clone(),
        registry,
        tools,
        Arc::new(rbac),
    );
    let sessions = SessionStore::new(pool, common::TEST_SECRET);
    RamaState::new(
        app,
        sessions,
        aiplane_core::server::usage::UsageHandle::disabled(),
    )
}

/// Seed a user with the OIDC role + a bearer token.
async fn seed_engineer_with_bearer(state: &RamaState) -> String {
    seed_engineer_with_bearer_mode(state, true).await
}

async fn seed_engineer_with_bearer_mode(state: &RamaState, always_on: bool) -> String {
    use aiplane_core::server::auth::token;
    let now = Timestamp::now();
    users::upsert(
        &state.db,
        &users::User {
            id: "alice".into(),
            email: "alice@example.com".into(),
            name: None,
            roles: vec!["engineering".into()],
            created_at: now,
            updated_at: now,
            timezone: None,
            speech_voice: None,
        },
    )
    .await
    .unwrap();
    let (plaintext, hash) = token::mint();
    let token_id = Uuid::new_v4().to_string();
    tokens::insert(
        &state.db,
        &tokens::Token {
            priority: None,
            id: token_id.clone(),
            user_id: "alice".into(),
            name: "test".into(),
            hash,
            created_at: now,
            last_used_at: None,
            expires_at: now + SignedDuration::from_hours(1),
            revoked_at: None,
            tools_enabled: true,
        },
    )
    .await
    .unwrap();
    if always_on {
        db::token_tool_prefs::set(&state.db, &token_id, "company_echo", true)
            .await
            .unwrap();
    }
    plaintext
}

#[tokio::test]
async fn token_capability_route_persists_auto_and_rejects_ungranted_keys() {
    let state = state_with_tool_grants(
        "http://unused.invalid",
        vec!["get_current_timestamp".into()],
    )
    .await;
    seed_engineer_with_bearer(&state).await;
    let token_id = tokens::list_for_user(&state.db, "alice").await.unwrap()[0]
        .id
        .clone();
    let session = state.sessions.create("alice").await.unwrap();
    let cookie = state.sessions.sign(&session.id);
    let app = common::app(state);
    let uri = format!("/api/v0/tokens/{token_id}/tools");
    let request = |states: serde_json::Value| {
        Request::builder()
            .method(Method::PUT)
            .uri(&uri)
            .header("cookie", format!("id={cookie}"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"tools_enabled": true, "tool_states": states}).to_string(),
            ))
            .unwrap()
    };
    let saved = app
        .serve(request(json!({"get_current_timestamp": "auto"})))
        .await
        .unwrap();
    let saved_status = saved.status();
    let saved_body = common::read_body(saved).await;
    assert_eq!(
        saved_status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&saved_body)
    );
    let details = app
        .serve(
            Request::builder()
                .method(Method::GET)
                .uri("/api/v0/tokens/details")
                .header("cookie", format!("id={cookie}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body: serde_json::Value =
        serde_json::from_slice(&common::read_body(details).await).unwrap();
    assert_eq!(
        body["tokens"][0]["tool_states"]["get_current_timestamp"],
        "auto"
    );
    assert_eq!(
        app.serve(request(json!({"company_echo": "on"})))
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
}

/// A token only ever reaches `/v1`, where chat-only tools are dropped before
/// its choices are read. Offering a switch for one would be a switch that never
/// does anything, and accepting it would store a choice nothing honours.
#[tokio::test]
async fn token_panel_offers_only_tools_a_token_can_use() {
    let state = state_with_tool_grants(
        "http://unused.invalid",
        vec!["get_current_timestamp".into(), "browser_control".into()],
    )
    .await;
    seed_engineer_with_bearer(&state).await;
    let token_id = tokens::list_for_user(&state.db, "alice").await.unwrap()[0]
        .id
        .clone();
    let session = state.sessions.create("alice").await.unwrap();
    let cookie = state.sessions.sign(&session.id);
    let app = common::app(state);

    let details = app
        .serve(
            Request::builder()
                .method(Method::GET)
                .uri("/api/v0/tokens/details")
                .header("cookie", format!("id={cookie}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body: serde_json::Value =
        serde_json::from_slice(&common::read_body(details).await).unwrap();
    let keys: Vec<&str> = body["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["key"].as_str())
        .collect();
    assert!(keys.contains(&"get_current_timestamp"), "{keys:?}");
    assert!(!keys.contains(&"browser_control"), "{keys:?}");

    let put = app
        .serve(
            Request::builder()
                .method(Method::PUT)
                .uri(format!("/api/v0/tokens/{token_id}/tools"))
                .header("cookie", format!("id={cookie}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"tools_enabled": true, "tool_states": {"browser_control": "on"}})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put.status(), StatusCode::BAD_REQUEST);
}

#[derive(Default)]
struct DiscoveryResponder(std::sync::atomic::AtomicUsize);

#[derive(Default)]
struct StreamingDiscoveryResponder(std::sync::atomic::AtomicUsize);

impl wiremock::Respond for StreamingDiscoveryResponder {
    fn respond(&self, _req: &wiremock::Request) -> ResponseTemplate {
        let round = self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let frames = match round {
            0 | 1 => {
                let (name, arguments) = if round == 0 {
                    ("search_gateway_tools", r#"{"query":"echo"}"#)
                } else {
                    ("company_echo", r#"{"message":"hello"}"#)
                };
                vec![
                    json!({"id": format!("s-{round}"), "object": "chat.completion.chunk", "created": 1, "model": "model-a",
                        "choices": [{"index": 0, "delta": {"role": "assistant", "tool_calls": [{"index": 0, "id": format!("call-{round}"), "type": "function", "function": {"name": name, "arguments": arguments}}]}, "finish_reason": null}]}),
                    json!({"id": format!("s-{round}"), "object": "chat.completion.chunk", "created": 1, "model": "model-a",
                        "choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}]}),
                ]
            }
            _ => vec![
                json!({"id": "s-final", "object": "chat.completion.chunk", "created": 1, "model": "model-a",
                "choices": [{"index": 0, "delta": {"content": "done"}, "finish_reason": "stop"}]}),
            ],
        };
        ResponseTemplate::new(200).set_body_raw(sse_body(&frames), "text/event-stream")
    }
}

impl wiremock::Respond for DiscoveryResponder {
    fn respond(&self, _req: &wiremock::Request) -> ResponseTemplate {
        let round = self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let call = match round {
            0 => Some(("search_gateway_tools", r#"{"query":"echo"}"#)),
            1 => Some(("company_echo", r#"{"message":"hello"}"#)),
            _ => None,
        };
        ResponseTemplate::new(200).set_body_json(match call {
            Some((name, args)) => json!({
                "choices": [{"message": {"role": "assistant", "content": null,
                    "tool_calls": [{"id": format!("call-{round}"), "type": "function",
                        "function": {"name": name, "arguments": args}}]},
                    "finish_reason": "tool_calls"}]
            }),
            None => json!({"choices": [{"message": {"role": "assistant", "content": "done"}, "finish_reason": "stop"}]}),
        })
    }
}

#[tokio::test]
async fn auto_token_discovers_only_the_tool_it_needs() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(DiscoveryResponder::default())
        .mount(&upstream)
        .await;
    let state = state_with_tools(&upstream.uri()).await;
    let bearer = seed_engineer_with_bearer_mode(&state, false).await;
    let app = router(Arc::new(state));
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {bearer}"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"model": "model-a", "messages": [{"role": "user", "content": "echo hello"}]})
                .to_string(),
        ))
        .unwrap();
    let response = app.serve(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let requests = upstream.received_requests().await.unwrap();
    assert_eq!(requests.len(), 3);
    let first: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    let second: serde_json::Value = serde_json::from_slice(&requests[1].body).unwrap();
    let first_names = first["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["function"]["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    let second_names = second["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["function"]["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(first_names, vec!["search_gateway_tools"]);
    assert_eq!(second_names, vec!["search_gateway_tools", "company_echo"]);
}

#[tokio::test]
async fn streaming_auto_token_discovers_only_the_tool_it_needs() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(StreamingDiscoveryResponder::default())
        .mount(&upstream)
        .await;
    let state = state_with_tools(&upstream.uri()).await;
    let bearer = seed_engineer_with_bearer_mode(&state, false).await;
    let app = router(Arc::new(state));
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {bearer}"))
        .header("content-type", "application/json")
        .body(Body::from(json!({"model": "model-a", "stream": true, "messages": [{"role": "user", "content": "echo hello"}]}).to_string()))
        .unwrap();
    let response = app.serve(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = String::from_utf8(common::read_body(response).await.to_vec()).unwrap();
    assert!(body.contains("done"));
    let requests = upstream.received_requests().await.unwrap();
    assert_eq!(requests.len(), 3);
    let first: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    let second: serde_json::Value = serde_json::from_slice(&requests[1].body).unwrap();
    assert_eq!(first["tools"].as_array().unwrap().len(), 1);
    assert!(
        second["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["function"]["name"] == "company_echo")
    );
}

#[tokio::test]
async fn tool_call_loop_runs_one_round_and_relays_final_response() {
    let upstream = MockServer::start().await;

    // The chat-completions mock keeps a counter of how many times it's
    // been called and returns different bodies per call:
    //   round 1 → assistant with a tool_call to company_echo
    //   round 2 → normal assistant reply
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ToolLoopResponder::default())
        .mount(&upstream)
        .await;

    let state = state_with_tools(&upstream.uri()).await;
    let bearer = seed_engineer_with_bearer(&state).await;
    let app = router(Arc::new(state));

    let body = json!({
        "model": "model-a",
        "messages": [{"role": "user", "content": "say hello"}]
    })
    .to_string();
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {bearer}"))
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();
    let resp = app.serve(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // The response should carry the round-2 body (no tool_calls) and the
    // gateway header reporting 1 completed round.
    let rounds = resp
        .headers()
        .get("x-gateway-tool-rounds")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert_eq!(rounds, "1", "expected one tool-loop round, got `{rounds}`");

    let body = common::read_body(resp).await;
    let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let content = parsed["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or_default();
    assert!(
        content.contains("hello echoed"),
        "expected round-2 reply to mention the echo result, got `{content}`"
    );
}

#[tokio::test]
async fn no_grants_and_no_client_tools_skips_the_loop() {
    // Same wiremock but the user has no tool grants AND the request
    // body doesn't carry a client-supplied tools array — the gateway
    // should fast-path stream through with no rounds (no
    // x-gateway-tool-rounds header).
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{"message": {"role": "assistant", "content": "plain reply"}}]
        })))
        .mount(&upstream)
        .await;

    // State without an RBAC mapping for the user's OIDC role → empty
    // allowed-tools.
    let state = common::state_with_chat_pool(&upstream.uri()).await;
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = router(Arc::new(state));

    let body = json!({"model": "model-a", "messages": []}).to_string();
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {bearer}"))
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();
    let resp = app.serve(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        resp.headers().get("x-gateway-tool-rounds").is_none(),
        "fast path should not emit x-gateway-tool-rounds"
    );
}

#[tokio::test]
async fn client_supplied_tools_still_get_gateway_tools_merged_in() {
    // The regression guard for the proxy merge: a client that brings its
    // OWN `tools` array must still have the gateway's tools unioned in and
    // the tool-loop run server-side. Previously the proxy bailed to a
    // byte-dumb passthrough whenever the client sent tools, so gateway
    // tools (e.g. search_web) were unreachable via the API.
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ToolLoopResponder::default())
        .mount(&upstream)
        .await;

    let state = state_with_tools(&upstream.uri()).await;
    let bearer = seed_engineer_with_bearer(&state).await;
    let app = router(Arc::new(state));

    // Client drives its own tool ("client_tool") AND we expect the gateway
    // to add "company_echo" alongside it.
    let body = json!({
        "model": "model-a",
        "messages": [{"role": "user", "content": "echo hello"}],
        "tools": [{
            "type": "function",
            "function": {"name": "client_tool", "description": "client's own", "parameters": {"type": "object"}}
        }]
    })
    .to_string();
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {bearer}"))
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();
    let resp = app.serve(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // The loop ran (gateway tool executed) despite the client bringing
    // tools — the old byte-dumb path would emit no rounds header at all.
    let rounds = resp
        .headers()
        .get("x-gateway-tool-rounds")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert_eq!(
        rounds, "1",
        "expected the gateway tool-loop to run, got `{rounds}`"
    );

    // Prove the union on the wire: the FIRST body the upstream received
    // carries both the client's tool and the injected gateway tool.
    let requests = upstream.received_requests().await.unwrap();
    let first: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    let tool_names: Vec<&str> = first["tools"]
        .as_array()
        .expect("request carries a tools array")
        .iter()
        .filter_map(|t| t["function"]["name"].as_str())
        .collect();
    assert!(
        tool_names.contains(&"client_tool"),
        "client's own tool must survive the merge, got {tool_names:?}"
    );
    assert!(
        tool_names.contains(&"company_echo"),
        "gateway tool must be injected alongside the client's, got {tool_names:?}"
    );
}

/// Assemble an OpenAI-style SSE body from a list of chunk JSON values,
/// terminated with `[DONE]`. Keeps the streaming tests readable instead
/// of hand-escaping `data:` frames.
fn sse_body(chunks: &[serde_json::Value]) -> String {
    let mut out = String::new();
    for chunk in chunks {
        out.push_str(&format!("data: {chunk}\n\n"));
    }
    out.push_str("data: [DONE]\n\n");
    out
}

#[tokio::test]
async fn streaming_gateway_tool_is_hidden_executed_and_final_streamed() {
    // stream:true + a gateway-owned tool_call: the gateway must suppress
    // the tool_call deltas (the client has no implementation and must not
    // see them), run the tool, loop, and stream the final round's text
    // through. The client sees the answer, never the company_echo call.
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(StreamingToolResponder::default())
        .mount(&upstream)
        .await;

    let state = state_with_tools(&upstream.uri()).await;
    let bearer = seed_engineer_with_bearer(&state).await;
    let app = router(Arc::new(state));

    let body = json!({
        "model": "model-a",
        "stream": true,
        "messages": [{"role": "user", "content": "echo hello"}]
    })
    .to_string();
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {bearer}"))
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();
    let resp = app.serve(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get(rama::http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("text/event-stream")
    );
    let body = String::from_utf8(common::read_body(resp).await.to_vec()).unwrap();

    // Final answer streamed through.
    assert!(
        body.contains("hello echoed"),
        "expected the final streamed text, body was:\n{body}"
    );
    // The gateway-owned tool_call was suppressed — never leaked to client.
    assert!(
        !body.contains("company_echo"),
        "gateway tool_call must not reach the client, body was:\n{body}"
    );
    assert!(
        !body.contains("\"tool_calls\""),
        "no tool_calls deltas should survive to the client, body was:\n{body}"
    );
    assert!(
        body.contains("[DONE]"),
        "stream must terminate, body was:\n{body}"
    );
}

#[tokio::test]
async fn streaming_client_tool_is_reemitted_to_client() {
    // stream:true + a CLIENT-owned tool_call (the client brought its own
    // tools, unioned with ours): the gateway suppresses the live deltas
    // while accumulating, then — because the turn calls a tool it doesn't
    // own — re-materialises the full tool_call as a synthesized assistant
    // delta + finish chunk so the client can run it and re-submit.
    let upstream = MockServer::start().await;
    let sse = sse_body(&[
        json!({
            "id": "cc-1", "object": "chat.completion.chunk", "created": 1, "model": "model-a",
            "choices": [{"index": 0, "delta": {"role": "assistant", "content": null, "tool_calls": [
                {"index": 0, "id": "call_x", "type": "function", "function": {"name": "client_tool", "arguments": ""}}
            ]}, "finish_reason": null}]
        }),
        json!({
            "id": "cc-1", "object": "chat.completion.chunk", "created": 1, "model": "model-a",
            "choices": [{"index": 0, "delta": {"tool_calls": [
                {"index": 0, "function": {"arguments": "{\"q\":1}"}}
            ]}, "finish_reason": null}]
        }),
        json!({
            "id": "cc-1", "object": "chat.completion.chunk", "created": 1, "model": "model-a",
            "choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}]
        }),
    ]);
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream"))
        .mount(&upstream)
        .await;

    let state = state_with_tools(&upstream.uri()).await;
    let bearer = seed_engineer_with_bearer(&state).await;
    let app = router(Arc::new(state));

    let body = json!({
        "model": "model-a",
        "stream": true,
        "messages": [{"role": "user", "content": "call my tool"}],
        "tools": [{
            "type": "function",
            "function": {"name": "client_tool", "description": "client's own", "parameters": {"type": "object"}}
        }]
    })
    .to_string();
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {bearer}"))
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();
    let resp = app.serve(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = String::from_utf8(common::read_body(resp).await.to_vec()).unwrap();

    // Find the synthesized assistant tool_calls delta and verify it
    // carries the client's complete call (name + accumulated arguments).
    let synthesized = body
        .lines()
        .filter_map(|l| l.strip_prefix("data: "))
        .filter(|p| *p != "[DONE]")
        .filter_map(|p| serde_json::from_str::<serde_json::Value>(p).ok())
        .find(|v| v.pointer("/choices/0/delta/tool_calls").is_some())
        .expect("a tool_calls delta must reach the client");
    let tc = &synthesized["choices"][0]["delta"]["tool_calls"][0];
    assert_eq!(tc["id"], "call_x");
    assert_eq!(tc["function"]["name"], "client_tool");
    assert_eq!(tc["function"]["arguments"], "{\"q\":1}");

    // Followed by a finish_reason terminator and [DONE].
    assert!(
        body.contains("\"finish_reason\":\"tool_calls\""),
        "expected a tool_calls finish terminator, body was:\n{body}"
    );
    assert!(
        body.contains("[DONE]"),
        "stream must terminate, body was:\n{body}"
    );
}

/// Stateful wiremock responder that returns different bodies on each
/// call. wiremock's stock `ResponseTemplate` only supports static
/// replies; rolling our own gives us the per-round shape control.
#[derive(Default)]
struct ToolLoopResponder {
    counter: std::sync::atomic::AtomicU32,
}

/// Streaming sibling of `ToolLoopResponder`: round 0 streams a
/// gateway-owned tool_call as SSE; round 1 streams a final text reply.
/// Exercises `drive_streaming_tool_loop` end to end.
#[derive(Default)]
struct StreamingToolResponder {
    counter: std::sync::atomic::AtomicU32,
}

impl wiremock::Respond for StreamingToolResponder {
    fn respond(&self, _req: &wiremock::Request) -> ResponseTemplate {
        let round = self
            .counter
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let sse = match round {
            0 => sse_body(&[
                json!({
                    "id": "s-1", "object": "chat.completion.chunk", "created": 1, "model": "model-a",
                    "choices": [{"index": 0, "delta": {"role": "assistant", "content": null, "tool_calls": [
                        {"index": 0, "id": "call_1", "type": "function", "function": {"name": "company_echo", "arguments": "{\"message\":\"hello\"}"}}
                    ]}, "finish_reason": null}]
                }),
                json!({
                    "id": "s-1", "object": "chat.completion.chunk", "created": 1, "model": "model-a",
                    "choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}]
                }),
            ]),
            _ => sse_body(&[
                json!({
                    "id": "s-2", "object": "chat.completion.chunk", "created": 2, "model": "model-a",
                    "choices": [{"index": 0, "delta": {"role": "assistant", "content": "hello echoed"}, "finish_reason": null}]
                }),
                json!({
                    "id": "s-2", "object": "chat.completion.chunk", "created": 2, "model": "model-a",
                    "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]
                }),
            ]),
        };
        ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream")
    }
}

impl wiremock::Respond for ToolLoopResponder {
    fn respond(&self, req: &wiremock::Request) -> ResponseTemplate {
        let round = self
            .counter
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        match round {
            0 => {
                // Round 1: assistant calls company_echo("hello"). The
                // gateway runs the tool, appends `role: "tool"` with
                // the echo result, and re-POSTs.
                ResponseTemplate::new(200).set_body_json(json!({
                    "id": "round-1",
                    "choices": [{
                        "index": 0,
                        "finish_reason": "tool_calls",
                        "message": {
                            "role": "assistant",
                            "content": null,
                            "tool_calls": [{
                                "id": "call_1",
                                "type": "function",
                                "function": {
                                    "name": "company_echo",
                                    "arguments": "{\"message\":\"hello\"}"
                                }
                            }]
                        }
                    }]
                }))
            }
            _ => {
                // Round 2+: pretend we consumed the tool result and
                // can answer normally. We can also peek at the round-2
                // request to confirm the tool result was appended,
                // but for the spike we just need a final reply that
                // says "hello echoed" so the assertion can fire.
                let body_str = std::str::from_utf8(&req.body).unwrap_or("");
                let saw_tool_msg =
                    body_str.contains("\"role\":\"tool\"") && body_str.contains("hello");
                let content = if saw_tool_msg {
                    "the tool said: hello echoed"
                } else {
                    "round 2 reached without tool message — bug in runner"
                };
                ResponseTemplate::new(200).set_body_json(json!({
                    "id": "round-2",
                    "choices": [{
                        "index": 0,
                        "finish_reason": "stop",
                        "message": {"role": "assistant", "content": content}
                    }]
                }))
            }
        }
    }
}

/// Arguments that differ on every round, as a model making progress produces.
/// A model repeating one identical call is the repeated-call guard's business.
fn progressing_args(request: &serde_json::Value) -> String {
    let step = request["messages"].as_array().map_or(0, Vec::len);
    json!({"message": format!("more {step}")}).to_string()
}

/// A model researching without end: it calls `company_echo` on every round,
/// except where `answers(request)` says it would answer instead. `streaming`
/// picks the wire shape.
struct BudgetResponder {
    streaming: bool,
    answers: fn(&serde_json::Value) -> bool,
}

impl wiremock::Respond for BudgetResponder {
    fn respond(&self, req: &wiremock::Request) -> ResponseTemplate {
        let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
        let answer = (self.answers)(&body);
        if !self.streaming {
            return ResponseTemplate::new(200).set_body_json(if answer {
                json!({"choices": [{"message": {"role": "assistant", "content": "Here is what I found."},
                    "finish_reason": "stop"}]})
            } else {
                json!({"choices": [{"message": {"role": "assistant", "content": null,
                    "tool_calls": [{"id": "call-r", "type": "function",
                        "function": {"name": "company_echo", "arguments": progressing_args(&body)}}]},
                    "finish_reason": "tool_calls"}]})
            });
        }
        let frames = if answer {
            vec![
                json!({"id": "s", "object": "chat.completion.chunk", "created": 1, "model": "model-a",
                    "choices": [{"index": 0, "delta": {"role": "assistant", "content": "Here is what I found."}, "finish_reason": null}]}),
                json!({"id": "s", "object": "chat.completion.chunk", "created": 1, "model": "model-a",
                    "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]}),
            ]
        } else {
            vec![
                json!({"id": "s", "object": "chat.completion.chunk", "created": 1, "model": "model-a",
                    "choices": [{"index": 0, "delta": {"role": "assistant", "tool_calls": [{"index": 0, "id": "call-r", "type": "function",
                        "function": {"name": "company_echo", "arguments": progressing_args(&body)}}]}, "finish_reason": null}]}),
                json!({"id": "s", "object": "chat.completion.chunk", "created": 1, "model": "model-a",
                    "choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}]}),
            ]
        };
        ResponseTemplate::new(200).set_body_raw(sse_body(&frames), "text/event-stream")
    }
}

fn honours_tool_choice(body: &serde_json::Value) -> bool {
    body["tool_choice"] == "none"
}

fn answers_only_without_tools(body: &serde_json::Value) -> bool {
    body.get("tools").is_none()
}

fn never_answers(_: &serde_json::Value) -> bool {
    false
}

/// Serve one research request against a `BudgetResponder` upstream.
async fn research_request(
    streaming: bool,
    answers: fn(&serde_json::Value) -> bool,
) -> (rama::http::Response, MockServer) {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(BudgetResponder { streaming, answers })
        .mount(&upstream)
        .await;
    let state = state_with_tools(&upstream.uri()).await;
    let bearer = seed_engineer_with_bearer(&state).await;
    let app = router(Arc::new(state));
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {bearer}"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"model": "model-a", "stream": streaming,
                "messages": [{"role": "user", "content": "research this sender"}]})
            .to_string(),
        ))
        .unwrap();
    (app.serve(req).await.unwrap(), upstream)
}

/// Upstream chat requests so far. Read it only after the body: a stream is
/// produced by a background task, which is still running until then.
async fn chat_calls(upstream: &MockServer) -> usize {
    upstream
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == "/chat/completions")
        .count()
}

fn data_frames(body: &str) -> Vec<serde_json::Value> {
    body.split("\n\n")
        .filter_map(|event| event.strip_prefix("data: "))
        .filter(|payload| *payload != "[DONE]")
        .map(|payload| serde_json::from_str(payload).unwrap())
        .collect()
}

const MAX_ROUNDS: u32 = aiplane_runtime::server::tools::runner::MAX_TOOL_ROUNDS;

/// The production failure: a request that runs out of tool rounds ends in a
/// 200 completion built from what was gathered, flagged in the header and in
/// the body — not in a 500 that throws the work away.
#[tokio::test]
async fn a_request_that_exhausts_the_tool_budget_still_gets_an_answer() {
    let (resp, upstream) = research_request(false, honours_tool_choice).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get("x-gateway-tool-budget-exhausted")
            .and_then(|v| v.to_str().ok()),
        Some("true")
    );
    assert_eq!(
        resp.headers()
            .get("x-gateway-tool-rounds")
            .and_then(|v| v.to_str().ok()),
        Some((MAX_ROUNDS - 1).to_string().as_str())
    );
    let body: serde_json::Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(
        body["choices"][0]["message"]["content"],
        "Here is what I found."
    );
    assert_eq!(body["aiplane"]["tool_budget_exhausted"], true);
    assert_eq!(body["aiplane"]["tool_rounds"], MAX_ROUNDS - 1);
    assert_eq!(
        chat_calls(&upstream).await,
        MAX_ROUNDS as usize,
        "the hard bound holds"
    );
}

#[tokio::test]
async fn a_streamed_request_that_exhausts_the_tool_budget_still_gets_an_answer() {
    let (resp, upstream) = research_request(true, honours_tool_choice).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = String::from_utf8(common::read_body(resp).await.to_vec()).unwrap();
    assert!(body.ends_with("data: [DONE]\n\n"), "{body}");
    assert!(!body.contains("\"tool_calls\""), "{body}");

    let frames = data_frames(&body);
    let content: String = frames
        .iter()
        .filter_map(|f| f["choices"][0]["delta"]["content"].as_str())
        .collect();
    assert_eq!(content, "Here is what I found.");
    let last = frames.last().unwrap();
    assert_eq!(last["choices"][0]["finish_reason"], "stop");
    assert_eq!(
        last["aiplane"],
        json!({"tool_rounds": MAX_ROUNDS - 1, "tool_budget_exhausted": true}),
        "the signal rides the finish chunk: {body}"
    );
    assert_eq!(
        frames.iter().filter(|f| f.get("aiplane").is_some()).count(),
        1
    );
    assert_eq!(chat_calls(&upstream).await, MAX_ROUNDS as usize);
}

/// A model that calls tools despite `tool_choice: "none"` (seen on vLLM) gets
/// one closing round with the tools withheld, streamed like any other.
#[tokio::test]
async fn a_streamed_model_that_ignores_the_final_round_is_closed_without_tools() {
    let (resp, upstream) = research_request(true, answers_only_without_tools).await;
    let body = String::from_utf8(common::read_body(resp).await.to_vec()).unwrap();
    let frames = data_frames(&body);
    assert_eq!(
        frames.last().unwrap()["aiplane"]["tool_budget_exhausted"],
        true,
        "{body}"
    );
    assert!(body.contains("Here is what I found."), "{body}");
    assert_eq!(
        chat_calls(&upstream).await,
        MAX_ROUNDS as usize + 1,
        "one request past the budget"
    );

    let requests = upstream.received_requests().await.unwrap();
    let closing: serde_json::Value =
        serde_json::from_slice(&requests.last().unwrap().body).unwrap();
    assert!(closing.get("tools").is_none());
    assert!(closing.get("tool_choice").is_none());
}

/// No answer even after the closing round: an error a client can tell from a
/// failure before any work — its own code, the rounds that ran in the text.
#[tokio::test]
async fn a_request_that_never_stops_calling_tools_fails_with_a_budget_error() {
    let (resp, upstream) = research_request(false, never_answers).await;
    assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
    let body: serde_json::Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(body["error"]["code"], "tool_budget_exhausted");
    assert!(
        body["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains(&format!("after {} tool rounds", MAX_ROUNDS - 1))),
        "{body}"
    );
    assert_eq!(chat_calls(&upstream).await, MAX_ROUNDS as usize + 1);
}

#[tokio::test]
async fn a_streamed_request_that_never_stops_calling_tools_ends_on_a_budget_error() {
    let (resp, upstream) = research_request(true, never_answers).await;
    let body = String::from_utf8(common::read_body(resp).await.to_vec()).unwrap();
    let frames = data_frames(&body);
    let error = &frames.last().unwrap()["error"];
    assert_eq!(error["code"], "tool_budget_exhausted", "{body}");
    assert!(body.ends_with("data: [DONE]\n\n"));
    assert_eq!(chat_calls(&upstream).await, MAX_ROUNDS as usize + 1);
}

/// Qwen on SGLang/vLLM on its final round: the tool parser is off under
/// `tool_choice: "none"`, so the call it makes anyway arrives as content,
/// split across chunks. Answers only once the tools are gone.
struct WrittenOutCallResponder {
    preamble: &'static str,
}

impl wiremock::Respond for WrittenOutCallResponder {
    fn respond(&self, req: &wiremock::Request) -> ResponseTemplate {
        let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
        let chunk = |delta: serde_json::Value, finish: serde_json::Value| {
            json!({"id": "s", "object": "chat.completion.chunk", "created": 1, "model": "model-a",
                "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]})
        };
        let frames = if body.get("tools").is_none() {
            vec![
                chunk(json!({"content": "Here is what I found."}), json!(null)),
                chunk(json!({}), json!("stop")),
            ]
        } else if body["tool_choice"] == "none" {
            vec![
                chunk(
                    json!({"role": "assistant", "content": format!("{}<tool", self.preamble)}),
                    json!(null),
                ),
                chunk(
                    json!({"content": "_call>\n<function=company_echo>\n<parameter=message>\nmore\n</parameter>\n</function>\n</tool_call>"}),
                    json!(null),
                ),
                chunk(json!({}), json!("stop")),
            ]
        } else {
            vec![
                chunk(
                    json!({"role": "assistant", "tool_calls": [{"index": 0, "id": "call-r", "type": "function",
                    "function": {"name": "company_echo", "arguments": progressing_args(&body)}}]}),
                    json!(null),
                ),
                chunk(json!({}), json!("tool_calls")),
            ]
        };
        ResponseTemplate::new(200).set_body_raw(sse_body(&frames), "text/event-stream")
    }
}

async fn streamed_written_out_call(preamble: &'static str) -> (String, usize) {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(WrittenOutCallResponder { preamble })
        .mount(&upstream)
        .await;
    let state = state_with_tools(&upstream.uri()).await;
    let bearer = seed_engineer_with_bearer(&state).await;
    let app = router(Arc::new(state));
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {bearer}"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"model": "model-a", "stream": true,
                "messages": [{"role": "user", "content": "research this sender"}]})
            .to_string(),
        ))
        .unwrap();
    let body = String::from_utf8(
        common::read_body(app.serve(req).await.unwrap())
            .await
            .to_vec(),
    )
    .unwrap();
    (body, chat_calls(&upstream).await)
}

fn streamed_content(body: &str) -> String {
    data_frames(body)
        .iter()
        .filter_map(|f| {
            f["choices"][0]["delta"]["content"]
                .as_str()
                .map(str::to_owned)
        })
        .collect()
}

#[tokio::test]
async fn a_tool_call_written_out_on_the_final_round_never_reaches_the_client() {
    let (body, calls) = streamed_written_out_call("Found three leads.\n\n").await;
    assert!(!body.contains("tool_call>"), "{body}");
    assert!(!body.contains("function="), "{body}");
    assert_eq!(streamed_content(&body), "Found three leads.\n\n");
    let frames = data_frames(&body);
    assert_eq!(
        frames.last().unwrap()["aiplane"]["tool_budget_exhausted"],
        true,
        "{body}"
    );
    assert_eq!(
        calls, MAX_ROUNDS as usize,
        "the preamble is the answer; no closing round"
    );
}

#[tokio::test]
async fn a_final_round_that_only_writes_out_a_call_gets_a_closing_round() {
    let (body, calls) = streamed_written_out_call("").await;
    assert!(!body.contains("tool_call>"), "{body}");
    assert_eq!(streamed_content(&body), "Here is what I found.");
    let frames = data_frames(&body);
    assert_eq!(
        frames
            .iter()
            .filter(|f| f["choices"][0]["finish_reason"].is_string())
            .count(),
        1,
        "the ignored round's finish must not end the message early: {body}"
    );
    assert_eq!(
        frames.last().unwrap()["aiplane"]["tool_budget_exhausted"],
        true
    );
    assert_eq!(calls, MAX_ROUNDS as usize + 1);
}

/// A model stuck on one call: the same `company_echo` arguments every round,
/// answering only once its tools are taken away.
struct StuckOnOneCall;

impl wiremock::Respond for StuckOnOneCall {
    fn respond(&self, req: &wiremock::Request) -> ResponseTemplate {
        let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
        ResponseTemplate::new(200).set_body_json(if body.get("tools").is_none() {
            json!({"choices": [{"message": {"role": "assistant", "content": "I could not get further."},
                "finish_reason": "stop"}]})
        } else {
            json!({"choices": [{"message": {"role": "assistant", "content": null,
                "tool_calls": [{"id": "call-r", "type": "function",
                    "function": {"name": "company_echo", "arguments": r#"{"message":"same"}"#}}]},
                "finish_reason": "tool_calls"}]})
        })
    }
}

#[tokio::test]
async fn a_model_repeating_one_identical_call_is_stopped_with_a_reason() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(StuckOnOneCall)
        .mount(&upstream)
        .await;
    let state = state_with_tools(&upstream.uri()).await;
    let bearer = seed_engineer_with_bearer(&state).await;
    let app = router(Arc::new(state));
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {bearer}"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"model": "model-a", "messages": [{"role": "user", "content": "go"}]})
                .to_string(),
        ))
        .unwrap();
    let resp = app.serve(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get("x-gateway-tool-budget-exhausted")
            .and_then(|v| v.to_str().ok()),
        Some("true")
    );
    let body: serde_json::Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    let reason = body["aiplane"]["stop_reason"].as_str().unwrap();
    assert!(reason.contains("company_echo"), "{reason}");
    assert_eq!(
        body["choices"][0]["message"]["content"],
        "I could not get further."
    );
    assert!(
        (chat_calls(&upstream).await as u32) < MAX_ROUNDS,
        "stopped long before the round budget"
    );
}

/// A token pinned On for `company_echo`, which alice's group does not grant,
/// next to the granted timestamp tool so the gateway loop is engaged at all.
async fn bearer_with_an_ungranted_tool_pinned_on(state: &RamaState) -> String {
    let bearer = seed_engineer_with_bearer(state).await;
    let token_id = tokens::list_for_user(&state.db, "alice").await.unwrap()[0]
        .id
        .clone();
    db::token_tool_prefs::set(&state.db, &token_id, "get_current_timestamp", true)
        .await
        .unwrap();
    bearer
}

fn completion_request(bearer: &str, stream: bool) -> Request {
    Request::builder()
        .method(Method::POST)
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {bearer}"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"model": "model-a", "stream": stream,
                "messages": [{"role": "user", "content": "call company_echo"}]})
            .to_string(),
        ))
        .unwrap()
}

#[tokio::test]
async fn a_v1_model_naming_an_ungranted_gateway_tool_does_not_run_it() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ToolLoopResponder::default())
        .mount(&upstream)
        .await;
    let state = state_with_tool_grants(&upstream.uri(), vec!["get_current_timestamp".into()]).await;
    let bearer = bearer_with_an_ungranted_tool_pinned_on(&state).await;

    let resp = router(Arc::new(state))
        .serve(completion_request(&bearer, false))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(
        body["choices"][0]["message"]["tool_calls"][0]["function"]["name"], "company_echo",
        "the call is handed back to the client, not run: {body}"
    );
    assert_eq!(chat_calls(&upstream).await, 1);
}

#[tokio::test]
async fn a_streaming_v1_model_naming_an_ungranted_gateway_tool_does_not_run_it() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(StreamingToolResponder::default())
        .mount(&upstream)
        .await;
    let state = state_with_tool_grants(&upstream.uri(), vec!["get_current_timestamp".into()]).await;
    let bearer = bearer_with_an_ungranted_tool_pinned_on(&state).await;

    let resp = router(Arc::new(state))
        .serve(completion_request(&bearer, true))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = String::from_utf8(common::read_body(resp).await.to_vec()).unwrap();
    assert!(body.contains("company_echo"), "{body}");
    assert!(!body.contains("hello echoed"), "{body}");
    assert_eq!(chat_calls(&upstream).await, 1);
}
