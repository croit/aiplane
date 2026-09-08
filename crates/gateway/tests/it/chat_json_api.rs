// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! `/api/v0/chat/*` — the JSON chat API for the SvelteKit SPA (issue #22
//! phase 2): session CRUD, the JSON submit, and the JSON-SSE event stream.
//!
//! The crown case is `a_submitted_turn_streams_over_the_events_endpoint`:
//! a real wiremock upstream streams chunks through the real worker into
//! SQLite, and the events endpoint replays them as JSON events — snapshot →
//! turn_delta → turn_finalized → close. That is the whole phase-2 wire in
//! one test.

use crate::common;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use common::Service as _;
use gateway::rama_server::{RamaState, SessionStore, router::router};
use gateway_core::server::config::Config;
use gateway_core::server::db;
use gateway_core::server::rbac::Resolver;
use gateway_core::server::upstreams::{
    self,
    config::{BackendConfig, PickerStrategy, PoolKind, UpstreamPoolConfig},
};
use gateway_runtime::server::AppState;
use gateway_runtime::server::tools::ToolRegistry;
use rama::http::body::util::BodyExt;
use rama::http::{Body, Method, Request, StatusCode, header};
use session_core::db as chat;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// A state wired to a (mocked) chat upstream advertising `model-a`.
async fn state_with_chat(upstream_uri: &str) -> RamaState {
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
                alias: None,
                probe_models: true,
                supports_edit: false,
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
    let app = AppState::new(
        Config::default(),
        pool.clone(),
        registry,
        Arc::new(ToolRegistry::new()),
        Arc::new(Resolver::empty()),
    );
    let sessions = SessionStore::new(pool, common::TEST_SECRET);
    RamaState::new(
        app,
        sessions,
        gateway_core::server::usage::UsageHandle::disabled(),
    )
}

async fn setup(upstream_uri: &str) -> (Arc<RamaState>, String) {
    let state = state_with_chat(upstream_uri).await;
    let cookie = common::seed_session(&state, "alice", "alice@example.com").await;
    (Arc::new(state), cookie)
}

fn json_req(method: Method, uri: String, cookie: &str, body: Option<String>) -> Request {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("cookie", format!("id={cookie}"));
    if let Some(body) = body {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
        builder.body(Body::from(body)).unwrap()
    } else {
        builder.body(Body::empty()).unwrap()
    }
}

async fn body_string(resp: rama::http::Response) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8_lossy(&bytes).to_string()
}

/// `event: <name>` → the JSON `data:` payload of its first frame, for every
/// frame in an SSE body.
fn sse_frames(body: &str) -> Vec<(String, serde_json::Value)> {
    let mut out = Vec::new();
    for block in body.split("\n\n") {
        let Some(event_line) = block.lines().find(|l| l.starts_with("event: ")) else {
            continue;
        };
        let Some(data_line) = block.lines().find(|l| l.starts_with("data: ")) else {
            continue;
        };
        let data = serde_json::from_str(data_line.trim_start_matches("data: "))
            .unwrap_or_else(|err| panic!("frame is not one JSON line: {err}\n{block}"));
        out.push((event_line.trim_start_matches("event: ").to_string(), data));
    }
    out
}

async fn mount_streaming_upstream(upstream: &MockServer, chunks: &[&str], delay_ms: u64) {
    let mut body = String::new();
    for chunk in chunks {
        body.push_str(&format!(
            "data: {{\"choices\":[{{\"delta\":{{\"content\":\"{chunk}\"}}}}]}}\n\n"
        ));
    }
    body.push_str("data: [DONE]\n\n");
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(body, "text/event-stream")
                .set_delay(Duration::from_millis(delay_ms)),
        )
        .mount(upstream)
        .await;
}

#[tokio::test]
async fn session_crud_round_trips() {
    let (state, cookie) = setup("http://unused.invalid").await;
    let app = router(state.clone());

    // Anonymous is a 401 envelope, not an HTML redirect.
    let resp = app
        .serve(
            Request::get("/api/v0/chat/sessions")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body = body_string(resp).await;
    assert!(body.contains("\"code\":\"unauthorized\""), "{body}");

    // Empty list, then create, then the row is there.
    let resp = app
        .serve(json_req(
            Method::GET,
            "/api/v0/chat/sessions".into(),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body_string(resp).await).unwrap()["sessions"]
            .as_array()
            .unwrap()
            .len(),
        0
    );

    let resp = app
        .serve(json_req(
            Method::POST,
            "/api/v0/chat/sessions".into(),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let created: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    let id = created["session"]["id"].as_str().unwrap().to_string();

    // Snapshot of a fresh session: metadata, no turns.
    let resp = app
        .serve(json_req(
            Method::GET,
            format!("/api/v0/chat/sessions/{id}"),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let snapshot: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    assert_eq!(snapshot["session"]["id"], id.as_str());
    assert_eq!(snapshot["turns"].as_array().unwrap().len(), 0);

    // Pin, then the list reflects it first.
    let resp = app
        .serve(json_req(
            Method::POST,
            format!("/api/v0/chat/sessions/{id}/pin"),
            &cookie,
            Some(r#"{"pinned": true}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(body_string(resp).await.contains("true"));
    let resp = app
        .serve(json_req(
            Method::GET,
            "/api/v0/chat/sessions".into(),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    let list: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    assert_eq!(list["sessions"][0]["pinned"], serde_json::json!(true));

    // Delete → 204, and a second delete is a 404.
    let resp = app
        .serve(json_req(
            Method::DELETE,
            format!("/api/v0/chat/sessions/{id}"),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let resp = app
        .serve(json_req(
            Method::DELETE,
            format!("/api/v0/chat/sessions/{id}"),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn an_unknown_session_is_a_404_everywhere() {
    let (state, cookie) = setup("http://unused.invalid").await;
    let app = router(state);
    for (method, uri, body) in [
        (Method::GET, "/api/v0/chat/sessions/nope", None),
        (
            Method::POST,
            "/api/v0/chat/sessions/nope/messages",
            Some(r#"{"model":"model-a","message":"hi"}"#.to_string()),
        ),
        (Method::POST, "/api/v0/chat/sessions/nope/cancel", None),
        (Method::GET, "/api/v0/chat/sessions/nope/events", None),
    ] {
        let label = method.clone();
        let resp = app
            .serve(json_req(method, uri.into(), &cookie, body))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND, "{label} {uri}");
    }
}

#[tokio::test]
async fn an_empty_message_is_rejected_before_anything_is_persisted() {
    let (state, cookie) = setup("http://unused.invalid").await;
    let app = router(state.clone());
    let session = chat::create_session(&state.db, "alice").await.unwrap();

    let resp = app
        .serve(json_req(
            Method::POST,
            format!("/api/v0/chat/sessions/{}/messages", session.id),
            &cookie,
            Some(r#"{"model":"model-a","message":"   "}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        chat::list_turns(&state.db, &session.id)
            .await
            .unwrap()
            .len(),
        0,
        "no rows for a refused submit"
    );
}

/// The crown case: submit via JSON, then watch the reply arrive as JSON
/// events — snapshot (live) → turn_delta → turn_finalized → stream close.
#[tokio::test]
async fn a_submitted_turn_streams_over_the_events_endpoint() {
    let upstream = MockServer::start().await;
    // The delay keeps the worker in-flight while the test attaches the
    // events stream — the live path, not the already-finished one.
    mount_streaming_upstream(&upstream, &["Hel", "lo"], 400).await;

    let (state, cookie) = setup(&upstream.uri()).await;
    let app = router(state.clone());
    let session = chat::create_session(&state.db, "alice").await.unwrap();

    let resp = app
        .serve(json_req(
            Method::POST,
            format!("/api/v0/chat/sessions/{}/messages", session.id),
            &cookie,
            Some(r#"{"model":"model-a","message":"hi"}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    let accepted: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    assert!(accepted["assistant_turn_id"].is_string());

    let resp = app
        .serve(json_req(
            Method::GET,
            format!("/api/v0/chat/sessions/{}/events", session.id),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("text/event-stream")
    );
    // Collecting the body only resolves once the stream closes — i.e. after
    // turn_finalized. That is itself an assertion.
    let body = body_string(resp).await;
    let frames = sse_frames(&body);
    let names: Vec<&str> = frames.iter().map(|(n, _)| n.as_str()).collect();

    let snapshot_idx = names
        .iter()
        .position(|n| *n == "snapshot")
        .expect("snapshot first");
    assert_eq!(snapshot_idx, 0, "the attach replay leads: {names:?}");
    assert!(
        frames[0].1["live_turn_id"].is_string(),
        "a worker is live: {}",
        frames[0].1
    );

    // The deltas build the full text; a coalescing subscriber may see one or
    // two, but their concatenation must be the whole reply.
    let streamed: String = frames
        .iter()
        .filter(|(n, _)| n == "turn_delta")
        .map(|(_, d)| d["text_delta"].as_str().unwrap_or(""))
        .collect();
    assert_eq!(streamed, "Hello");

    let finalized = frames
        .iter()
        .find(|(n, _)| n == "turn_finalized")
        .unwrap_or_else(|| panic!("expected turn_finalized in {names:?}"));
    assert_eq!(finalized.1["status"], "completed");
    assert!(finalized.1["duration_ms"].is_i64());
    assert_eq!(finalized.1["turn_id"], accepted["assistant_turn_id"]);
}

/// Attaching with nothing live: snapshot + idle, and the stream ends —
/// the client contract for "nothing is streaming".
#[tokio::test]
async fn an_idle_session_answers_snapshot_then_idle_and_closes() {
    let (state, cookie) = setup("http://unused.invalid").await;
    let app = router(state.clone());
    let session = chat::create_session(&state.db, "alice").await.unwrap();
    chat::create_user_turn(&state.db, &session.id, "u1", "hello there")
        .await
        .unwrap();

    let resp = app
        .serve(json_req(
            Method::GET,
            format!("/api/v0/chat/sessions/{}/events", session.id),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    let frames = sse_frames(&body);
    let names: Vec<&str> = frames.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, vec!["snapshot", "idle"], "{body}");
    assert!(frames[0].1["live_turn_id"].is_null());
    // The snapshot carries the persisted user turn.
    assert_eq!(
        frames[0].1["turns"][0]["turn"]["user_content"],
        serde_json::json!("hello there")
    );
}

/// A second submit while the user's turn is still streaming is a 409 —
/// one live worker per user, the same invariant the legacy wire enforces.
#[tokio::test]
async fn a_second_submit_while_streaming_is_a_409() {
    let upstream = MockServer::start().await;
    mount_streaming_upstream(&upstream, &["slow"], 5_000).await;
    let (state, cookie) = setup(&upstream.uri()).await;
    let app = router(state.clone());
    let session = chat::create_session(&state.db, "alice").await.unwrap();

    let first = app
        .serve(json_req(
            Method::POST,
            format!("/api/v0/chat/sessions/{}/messages", session.id),
            &cookie,
            Some(r#"{"model":"model-a","message":"one"}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::ACCEPTED);

    let second = app
        .serve(json_req(
            Method::POST,
            format!("/api/v0/chat/sessions/{}/messages", session.id),
            &cookie,
            Some(r#"{"model":"model-a","message":"two"}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(second.status(), StatusCode::CONFLICT);
    assert!(body_string(second).await.contains("turn_in_progress"));

    // Cancel so the worker (and its slow upstream request) ends with the
    // test instead of lingering.
    let cancel = app
        .serve(json_req(
            Method::POST,
            format!("/api/v0/chat/sessions/{}/cancel", session.id),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(cancel.status(), StatusCode::OK);
    assert!(body_string(cancel).await.contains("\"cancelled\":true"));
}

/// A finished turn replays from the DB: attach after the fact and the
/// snapshot alone tells the whole story.
#[tokio::test]
async fn reconnecting_after_completion_replays_from_the_db() {
    let upstream = MockServer::start().await;
    mount_streaming_upstream(&upstream, &["done"], 0).await;
    let (state, cookie) = setup(&upstream.uri()).await;
    let app = router(state.clone());
    let session = chat::create_session(&state.db, "alice").await.unwrap();

    let resp = app
        .serve(json_req(
            Method::POST,
            format!("/api/v0/chat/sessions/{}/messages", session.id),
            &cookie,
            Some(r#"{"model":"model-a","message":"hi"}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::ACCEPTED);

    // Wait for the *assistant* row to settle (the user row is born
    // Completed; matching on status alone would grab that).
    let assistant_id = loop {
        let turns = chat::list_turns(&state.db, &session.id).await.unwrap();
        if let Some(t) = turns.iter().find(|t| {
            t.turn.role == chat::TurnRole::Assistant
                && t.turn.status != chat::TurnStatus::InProgress
        }) {
            break t.turn.id.clone();
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    };

    let resp = app
        .serve(json_req(
            Method::GET,
            format!("/api/v0/chat/sessions/{}/events", session.id),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    let frames = sse_frames(&body_string(resp).await);
    assert_eq!(frames[0].0, "snapshot");
    let assistant = frames[0].1["turns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["turn"]["id"] == assistant_id.as_str())
        .unwrap();
    assert_eq!(assistant["turn"]["content"], serde_json::json!("done"));
    assert_eq!(assistant["turn"]["status"], serde_json::json!("completed"));
}
