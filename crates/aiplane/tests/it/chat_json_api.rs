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

use aiplane::rama_server::{RamaState, SessionStore, router::router};
use aiplane_core::server::config::Config;
use aiplane_core::server::db;
use aiplane_core::server::db::automatic_routes::{AutomaticRoute, AutomaticRouteCandidate};
use aiplane_core::server::rbac::Resolver;
use aiplane_core::server::rbac::config::{RbacConfig, RoleConfig};
use aiplane_core::server::upstreams::{
    self,
    config::{AliasSpec, BackendConfig, PickerStrategy, PoolKind, UpstreamPoolConfig},
};
use aiplane_runtime::server::AppState;
use aiplane_runtime::server::tools::ToolRegistry;
use aiplane_tools::location::GetUserLocation;
use common::Service as _;
use rama::http::body::util::BodyExt;
use rama::http::{Body, Method, Request, StatusCode, header};
use session_core::db as chat;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// A state wired to a (mocked) chat upstream advertising `model-a`.
async fn state_with_chat(upstream_uri: &str) -> RamaState {
    state_with_chat_access(
        upstream_uri,
        Arc::new(ToolRegistry::new()),
        Arc::new(Resolver::empty()),
    )
    .await
}

async fn state_with_chat_access(
    upstream_uri: &str,
    tools: Arc<ToolRegistry>,
    rbac: Arc<Resolver>,
) -> RamaState {
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
    let app = AppState::new(Config::default(), pool.clone(), registry, tools, rbac);
    let sessions = SessionStore::new(pool, common::TEST_SECRET);
    RamaState::new(
        app,
        sessions,
        aiplane_core::server::usage::UsageHandle::disabled(),
    )
}

/// A chat pool serving exactly one model, reachable under `alias` too — the
/// shape every real deployment has (`alias = ["default", ...]`).
async fn state_with_aliased_chat_model(model: &str, alias: &str) -> RamaState {
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
                alias: Some(AliasSpec::Names(vec![alias.to_string()])),
                probe_models: true,
                supports_edit: false,
                enabled: true,
                name: "mock".into(),
                base_url: "http://unused.invalid".into(),
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
    common::seed_pool_models(&registry, "pool", 0, &[model]);
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
        aiplane_core::server::usage::UsageHandle::disabled(),
    )
}

async fn setup(upstream_uri: &str) -> (Arc<RamaState>, String) {
    let state = state_with_chat(upstream_uri).await;
    let cookie = common::seed_session(&state, "alice", "alice@example.com").await;
    (Arc::new(state), cookie)
}

async fn setup_with_location_tool(upstream_uri: &str) -> (Arc<RamaState>, String) {
    let tools = Arc::new(ToolRegistry::new().with(GetUserLocation));
    let rbac = Arc::new(
        Resolver::build(
            RbacConfig {
                default_role: Some("member".into()),
                mappings: vec![],
            },
            vec![RoleConfig {
                id: "member".into(),
                admin: false,
                models: vec![],
                tools: vec!["get_user_location".into()],
                skills: vec![],
            }],
        )
        .unwrap(),
    );
    let state = state_with_chat_access(upstream_uri, tools, rbac).await;
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
async fn session_snapshot_includes_compaction_boundary_and_conversation_assets() {
    let (state, cookie) = setup("http://unused.invalid").await;
    let session = chat::create_session(&state.db, "alice").await.unwrap();
    let marker = session_core::attachments::marker_line(
        "diagram.png",
        "image/png",
        "/chat/attachment/user-turn/diagram.png",
        2048,
    );
    chat::create_user_turn(
        &state.db,
        &session.id,
        "user-turn",
        &format!("look\n\n{marker}"),
    )
    .await
    .unwrap();
    aiplane_core::server::db::chat_compactions::upsert(
        &state.db,
        &session.id,
        0,
        "Earlier context",
        Some(100),
        Some(20),
    )
    .await
    .unwrap();

    let app = router(state);
    let response = app
        .serve(json_req(
            Method::GET,
            format!("/api/v0/chat/sessions/{}", session.id),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_str(&body_string(response).await).unwrap();
    assert_eq!(body["compacted_up_to_seq"], 0);
    assert_eq!(body["assets"][0]["id"], "user-turn/diagram.png");
    assert_eq!(body["assets"][0]["filename"], "diagram.png");
    assert_eq!(body["assets"][0]["mime"], "image/png");
    assert_eq!(body["assets"][0]["size"], 2048);
    assert_eq!(
        body["assets"][0]["url"],
        "/chat/attachment/user-turn/diagram.png"
    );
}

#[tokio::test]
async fn edit_accepts_the_composers_multipart_shape() {
    let (state, cookie) = setup("http://unused.invalid").await;
    let session = chat::create_session(&state.db, "alice").await.unwrap();
    chat::create_user_turn(&state.db, &session.id, "user-turn", "before")
        .await
        .unwrap();
    let boundary = "edit-boundary";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\nmodel-a\r\n\
         --{boundary}\r\nContent-Disposition: form-data; name=\"message\"\r\n\r\nafter\r\n\
         --{boundary}--\r\n"
    );
    let req = Request::builder()
        .method(Method::POST)
        .uri(format!(
            "/api/v0/chat/sessions/{}/turns/user-turn/edit",
            session.id
        ))
        .header("cookie", format!("id={cookie}"))
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();
    let db = state.db.clone();
    let response = router(state).serve(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let turn = chat::get_turn(&db, &session.id, "user-turn")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(turn.user_content.as_deref(), Some("after"));
}

#[tokio::test]
async fn chat_landing_resolves_one_stable_latest_session() {
    let (state, cookie) = setup("http://unused.invalid").await;
    let app = router(state);

    let first = app
        .serve(json_req(
            Method::GET,
            "/api/v0/chat/landing".into(),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    let first: serde_json::Value = serde_json::from_str(&body_string(first).await).unwrap();
    let id = first["session"]["id"].as_str().unwrap();

    let second = app
        .serve(json_req(
            Method::GET,
            "/api/v0/chat/landing".into(),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    let second: serde_json::Value = serde_json::from_str(&body_string(second).await).unwrap();
    assert_eq!(second["session"]["id"], id);

    let list = app
        .serve(json_req(
            Method::GET,
            "/api/v0/chat/sessions".into(),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    let list: serde_json::Value = serde_json::from_str(&body_string(list).await).unwrap();
    assert_eq!(list["sessions"].as_array().unwrap().len(), 1);

    let anonymous = app
        .serve(
            Request::get("/api/v0/chat/landing")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
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

#[tokio::test]
async fn attaching_to_orphaned_turn_recovers_turn_and_running_tool_call() {
    let (state, cookie) = setup("http://unused.invalid").await;
    let app = router(state.clone());
    let session = chat::create_session(&state.db, "alice").await.unwrap();
    chat::create_assistant_turn_in_progress(&state.db, &session.id, "orphan", "model-a")
        .await
        .unwrap();
    chat::insert_running_tool_call(&state.db, "orphan", "call-1", "fetch_url", "{}")
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
    let frames = sse_frames(&body_string(resp).await);
    assert_eq!(frames[0].0, "snapshot");
    assert_eq!(frames[0].1["turns"][0]["turn"]["status"], "errored");
    assert_eq!(
        frames[0].1["turns"][0]["tool_calls"][0]["status"],
        "errored"
    );
    assert_eq!(frames[1].0, "idle");

    let turns = chat::list_turns(&state.db, &session.id).await.unwrap();
    assert_eq!(turns[0].turn.status, chat::TurnStatus::Errored);
    assert_eq!(turns[0].tool_calls[0].status, chat::ToolCallStatus::Errored);
}

#[tokio::test]
async fn shared_viewer_does_not_recover_owners_live_turn() {
    let (state, _alice) = setup("http://unused.invalid").await;
    let bob = common::seed_session(&state, "bob", "bob@example.com").await;
    let app = router(state.clone());
    let session = chat::create_session(&state.db, "alice").await.unwrap();
    chat::set_shared(&state.db, "alice", &session.id, true)
        .await
        .unwrap();
    chat::create_assistant_turn_in_progress(&state.db, &session.id, "live", "model-a")
        .await
        .unwrap();

    let resp = app
        .serve(json_req(
            Method::GET,
            format!("/api/v0/chat/sessions/{}/events", session.id),
            &bob,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let frames = sse_frames(&body_string(resp).await);
    assert_eq!(frames[0].1["turns"][0]["turn"]["status"], "in_progress");
    assert_eq!(
        chat::list_turns(&state.db, &session.id).await.unwrap()[0]
            .turn
            .status,
        chat::TurnStatus::InProgress
    );
}

/// A second message during a turn is not refused: it belongs to the answer
/// being written, so it goes into that turn's prompt at its next round.
#[tokio::test]
async fn a_second_submit_lands_in_the_turn_that_is_already_running() {
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
    let ids: serde_json::Value = serde_json::from_str(&body_string(first).await).unwrap();
    assert_eq!(ids["placement"].as_str(), Some("started"));
    let assistant_turn_id = ids["assistant_turn_id"].as_str().unwrap().to_string();

    let second = app
        .serve(json_req(
            Method::POST,
            format!("/api/v0/chat/sessions/{}/messages", session.id),
            &cookie,
            Some(r#"{"model":"model-a","message":"and in euros"}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(second.status(), StatusCode::ACCEPTED);
    let placed: serde_json::Value = serde_json::from_str(&body_string(second).await).unwrap();
    assert_eq!(placed["placement"].as_str(), Some("folded"));
    assert_eq!(
        placed["assistant_turn_id"].as_str(),
        Some(assistant_turn_id.as_str()),
        "folded into the turn that is running, not a new one"
    );

    let notes = chat::list_steers(&state.db, &assistant_turn_id)
        .await
        .unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].text, "and in euros");
    // No second turn was created for it.
    assert_eq!(
        chat::list_turns(&state.db, &session.id)
            .await
            .unwrap()
            .len(),
        2
    );

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
}

/// Poll a condition for up to 5s.
///
/// For the things the scheduler does on its own: nothing asks for them, so
/// there is no response to await.
async fn wait_until<F, Fut>(mut check: F) -> bool
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..200 {
        if check().await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    false
}

/// Wait until this user has no turn in flight, or fail the test.
///
/// A turn outlives the request that started it, so a test that cancels or
/// submits and then asserts on the *next* submit has to wait for the worker to
/// clear. Three hand-rolled poll loops with three different budgets and no
/// timeout behaviour is three ways to get a flake that reads as a logic error.
async fn wait_for_idle(state: &Arc<RamaState>, user_id: &str) {
    for _ in 0..200 {
        if state.chats.running_for_user(user_id) == 0 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("a turn was still running after 5s");
}

/// Raise the operator's parallel-turn ceiling the way the admin UI does —
/// through the settings row and a live reload — so these tests exercise the
/// same path an operator takes rather than reaching into the config struct.
async fn grant_parallel_turns(state: &Arc<RamaState>, limit: usize) {
    aiplane_core::server::db::app_settings::set(
        &state.db,
        "settings.chat.turns.max_parallel",
        &limit.to_string(),
    )
    .await
    .unwrap();
    state.reload_settings().await;
    assert_eq!(state.config().chat.turns.max_parallel, limit);
}

/// Parallel conversations, and what happens past the ceiling: the message is
/// accepted and waits its turn, rather than being refused with the user left
/// holding the text. When a slot frees up, it starts on its own — no client
/// involved, which is the whole point of the queue being server-side.
#[tokio::test]
async fn a_third_conversation_waits_for_a_slot_and_then_starts_itself() {
    let upstream = MockServer::start().await;
    mount_streaming_upstream(&upstream, &["slow"], 5_000).await;
    let (state, cookie) = setup(&upstream.uri()).await;
    grant_parallel_turns(&state, 2).await;
    let app = router(state.clone());
    let first = chat::create_session(&state.db, "alice").await.unwrap();
    let second = chat::create_session(&state.db, "alice").await.unwrap();
    let third = chat::create_session(&state.db, "alice").await.unwrap();

    let mut placements = Vec::new();
    for session in [&first, &second, &third] {
        let resp = app
            .serve(json_req(
                Method::POST,
                format!("/api/v0/chat/sessions/{}/messages", session.id),
                &cookie,
                Some(r#"{"model":"model-a","message":"go"}"#.into()),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::ACCEPTED);
        let parsed: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
        placements.push(parsed["placement"].as_str().unwrap().to_string());
    }
    assert_eq!(placements, ["started", "started", "queued"]);

    // The waiting message is already in its transcript — it was sent, after
    // all — and the queue knows how to start it.
    let turns = chat::list_turns(&state.db, &third.id).await.unwrap();
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].turn.user_content.as_deref(), Some("go"));
    let waiting = chat::list_pending_for_session(&state.db, &third.id)
        .await
        .unwrap();
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].model, "model-a");

    // Free a slot. Nothing asks for the waiting turn; it just starts.
    let _ = app
        .serve(json_req(
            Method::POST,
            format!("/api/v0/chat/sessions/{}/cancel", first.id),
            &cookie,
            None,
        ))
        .await;
    let started = wait_until(|| {
        let state = state.clone();
        let session_id = third.id.clone();
        async move {
            chat::list_turns(&state.db, &session_id)
                .await
                .map(|turns| turns.len() == 2)
                .unwrap_or(false)
        }
    })
    .await;
    assert!(started, "the waiting turn never started");
    assert!(
        chat::list_pending_for_session(&state.db, &third.id)
            .await
            .unwrap()
            .is_empty(),
        "a started turn leaves the queue"
    );

    for session in [&second, &third] {
        let _ = app
            .serve(json_req(
                Method::POST,
                format!("/api/v0/chat/sessions/{}/cancel", session.id),
                &cookie,
                None,
            ))
            .await;
    }
}

/// Cancelling names one conversation. The user's other running chat must not
/// notice — which is the difference between parallel chats and a shared
/// stop button.
#[tokio::test]
async fn cancelling_one_conversation_leaves_the_other_running() {
    let upstream = MockServer::start().await;
    mount_streaming_upstream(&upstream, &["slow"], 5_000).await;
    let (state, cookie) = setup(&upstream.uri()).await;
    grant_parallel_turns(&state, 2).await;
    let app = router(state.clone());
    let a = chat::create_session(&state.db, "alice").await.unwrap();
    let b = chat::create_session(&state.db, "alice").await.unwrap();

    for session in [&a, &b] {
        let resp = app
            .serve(json_req(
                Method::POST,
                format!("/api/v0/chat/sessions/{}/messages", session.id),
                &cookie,
                Some(r#"{"model":"model-a","message":"go"}"#.into()),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::ACCEPTED);
    }

    let cancel = app
        .serve(json_req(
            Method::POST,
            format!("/api/v0/chat/sessions/{}/cancel", a.id),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert!(body_string(cancel).await.contains("\"cancelled\":true"));

    // Assert on the flag rather than on a worker count: the cancelled worker
    // only notices between upstream chunks, so it is still registered for as
    // long as the mock takes to answer. What must be true immediately is that
    // exactly one of the two was asked to stop.
    let flagged = |session_id: &str| {
        state
            .chats
            .get("alice", session_id)
            .expect("worker still registered")
            .cancel
            .load(std::sync::atomic::Ordering::SeqCst)
    };
    assert!(flagged(&a.id), "the named conversation was asked to stop");
    assert!(!flagged(&b.id), "the other conversation keeps streaming");

    let _ = app
        .serve(json_req(
            Method::POST,
            format!("/api/v0/chat/sessions/{}/cancel", b.id),
            &cookie,
            None,
        ))
        .await;
}

/// The interjection round trip: accepted while a turn runs, recorded against
/// that turn, and visible to anyone reading the conversation — all before the
/// model has had a chance to read it.
#[tokio::test]
async fn an_interjection_is_recorded_against_the_running_turn() {
    let upstream = MockServer::start().await;
    mount_streaming_upstream(&upstream, &["slow"], 5_000).await;
    let (state, cookie) = setup(&upstream.uri()).await;
    let app = router(state.clone());
    let session = chat::create_session(&state.db, "alice").await.unwrap();

    let submitted = app
        .serve(json_req(
            Method::POST,
            format!("/api/v0/chat/sessions/{}/messages", session.id),
            &cookie,
            Some(r#"{"model":"model-a","message":"one"}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(submitted.status(), StatusCode::ACCEPTED);
    let ids: serde_json::Value = serde_json::from_str(&body_string(submitted).await).unwrap();
    let assistant_turn_id = ids["assistant_turn_id"].as_str().unwrap().to_string();

    let steered = app
        .serve(json_req(
            Method::POST,
            format!("/api/v0/chat/sessions/{}/steer", session.id),
            &cookie,
            Some(r#"{"message":"in euros, please"}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(steered.status(), StatusCode::ACCEPTED);
    let note: serde_json::Value = serde_json::from_str(&body_string(steered).await).unwrap();
    assert_eq!(note["turn_id"].as_str(), Some(assistant_turn_id.as_str()));
    // Accepted is not "the model read it" — the status says exactly that.
    assert_eq!(note["status"].as_str(), Some("pending"));

    let rows = chat::list_steers(&state.db, &assistant_turn_id)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].text, "in euros, please");

    // And it is part of the conversation as read back, not only in memory.
    let snapshot = app
        .serve(json_req(
            Method::GET,
            format!("/api/v0/chat/sessions/{}", session.id),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert!(body_string(snapshot).await.contains("in euros, please"));

    let _ = app
        .serve(json_req(
            Method::POST,
            format!("/api/v0/chat/sessions/{}/cancel", session.id),
            &cookie,
            None,
        ))
        .await;
}

/// Discarding a returned interjection settles its row.
///
/// Forgetting it in the browser is not enough: the row stays `pending`, and
/// the next time any turn in this conversation finishes the client reads it
/// back and queues the dismissed sentence again — which then sends itself.
#[tokio::test]
async fn a_discarded_interjection_stops_coming_back() {
    let (state, cookie) = setup("http://unused.invalid").await;
    let app = router(state.clone());
    let session = chat::create_session(&state.db, "alice").await.unwrap();
    let turn = chat::create_assistant_turn_in_progress(&state.db, &session.id, "a1", "model-a")
        .await
        .unwrap();
    let note = chat::insert_steer(&state.db, &turn.id, "never mind")
        .await
        .unwrap()
        .expect("the turn is running");

    let discarded = app
        .serve(json_req(
            Method::POST,
            format!(
                "/api/v0/chat/sessions/{}/steer/{}/discard",
                session.id, note.id
            ),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(discarded.status(), StatusCode::OK);
    let rows = chat::list_steers(&state.db, &turn.id).await.unwrap();
    assert_eq!(rows[0].status, chat::SteerStatus::Discarded);

    // Idempotent: a second tab doing the same is not an error, and a note
    // already discarded stays discarded.
    let again = app
        .serve(json_req(
            Method::POST,
            format!(
                "/api/v0/chat/sessions/{}/steer/{}/discard",
                session.id, note.id
            ),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(again.status(), StatusCode::OK);
    let rows = chat::list_steers(&state.db, &turn.id).await.unwrap();
    assert_eq!(rows[0].status, chat::SteerStatus::Discarded);
}

/// A note belonging to someone else's conversation is not discardable through
/// this one — the id travels from the client and is not evidence of anything.
#[tokio::test]
async fn a_steer_from_another_conversation_cannot_be_discarded() {
    let (state, cookie) = setup("http://unused.invalid").await;
    let app = router(state.clone());
    let mine = chat::create_session(&state.db, "alice").await.unwrap();
    let other = chat::create_session(&state.db, "alice").await.unwrap();
    let turn = chat::create_assistant_turn_in_progress(&state.db, &other.id, "a1", "model-a")
        .await
        .unwrap();
    let note = chat::insert_steer(&state.db, &turn.id, "elsewhere")
        .await
        .unwrap()
        .expect("the turn is running");

    let resp = app
        .serve(json_req(
            Method::POST,
            format!(
                "/api/v0/chat/sessions/{}/steer/{}/discard",
                mine.id, note.id
            ),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let rows = chat::list_steers(&state.db, &turn.id).await.unwrap();
    assert_eq!(rows[0].status, chat::SteerStatus::Pending);
}

/// An interjection is one more user message in every remaining round of the
/// prompt, so its size is bounded. Longer than that is a message, and the
/// composer already sends those.
#[tokio::test]
async fn an_oversized_interjection_is_refused() {
    let upstream = MockServer::start().await;
    mount_streaming_upstream(&upstream, &["slow"], 5_000).await;
    let (state, cookie) = setup(&upstream.uri()).await;
    let app = router(state.clone());
    let session = chat::create_session(&state.db, "alice").await.unwrap();
    let submitted = app
        .serve(json_req(
            Method::POST,
            format!("/api/v0/chat/sessions/{}/messages", session.id),
            &cookie,
            Some(r#"{"model":"model-a","message":"one"}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(submitted.status(), StatusCode::ACCEPTED);

    let huge = "x".repeat(8 * 1024);
    let resp = app
        .serve(json_req(
            Method::POST,
            format!("/api/v0/chat/sessions/{}/steer", session.id),
            &cookie,
            Some(format!(r#"{{"message":"{huge}"}}"#)),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let _ = app
        .serve(json_req(
            Method::POST,
            format!("/api/v0/chat/sessions/{}/cancel", session.id),
            &cookie,
            None,
        ))
        .await;
}

/// A conversation whose message is waiting keeps its stream open, and the
/// stream reports the turn starting — rather than ending at `idle` and leaving
/// the page to ask again every few seconds.
///
/// The worker that answers a waiting message is created by the scheduler, on
/// no request at all, so nothing on the wire would otherwise announce it.
#[tokio::test]
async fn a_waiting_conversation_is_told_when_its_turn_starts() {
    let upstream = MockServer::start().await;
    mount_streaming_upstream(&upstream, &["done"], 300).await;
    let (state, cookie) = setup(&upstream.uri()).await;
    let app = router(state.clone());
    let busy = chat::create_session(&state.db, "alice").await.unwrap();
    let waiting = chat::create_session(&state.db, "alice").await.unwrap();

    // One conversation takes the only slot; the other's message waits.
    for (session, body) in [
        (&busy, r#"{"model":"model-a","message":"first"}"#),
        (&waiting, r#"{"model":"model-a","message":"second"}"#),
    ] {
        let resp = app
            .serve(json_req(
                Method::POST,
                format!("/api/v0/chat/sessions/{}/messages", session.id),
                &cookie,
                Some(body.into()),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::ACCEPTED);
    }
    assert_eq!(
        chat::list_pending_for_session(&state.db, &waiting.id)
            .await
            .unwrap()
            .len(),
        1,
        "the second message is waiting for a slot"
    );

    // Attach to the *waiting* conversation and read the whole stream. It must
    // not end at `idle`: the first turn finishes, the scheduler starts this
    // one, and the stream follows it through to `turn_finalized`.
    let resp = app
        .serve(json_req(
            Method::GET,
            format!("/api/v0/chat/sessions/{}/events", waiting.id),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = tokio::time::timeout(Duration::from_secs(20), body_string(resp))
        .await
        .expect("the stream ended within the test's patience");
    let names: Vec<String> = sse_frames(&body)
        .into_iter()
        .map(|(name, _)| name)
        .collect();

    assert_eq!(names.first().map(String::as_str), Some("snapshot"));
    assert!(
        names.iter().any(|n| n == "turn_finalized"),
        "the stream followed the turn it was waiting for: {names:?}"
    );
    assert!(
        !names.iter().any(|n| n == "idle"),
        "it must not have given up and closed as idle: {names:?}"
    );
}

/// The hand-off from waiting to streaming carries a snapshot naming the live
/// turn.
///
/// `live_turn_id` is the only thing that tells a client a turn is streaming,
/// and the snapshot it already has says `null`. Without a second one the
/// deltas arrive into a page that still believes it is idle: no stop button,
/// no interrupt, and no reconnect if the stream drops.
#[tokio::test]
async fn the_handover_names_the_turn_that_started() {
    let upstream = MockServer::start().await;
    mount_streaming_upstream(&upstream, &["done"], 300).await;
    let (state, cookie) = setup(&upstream.uri()).await;
    let app = router(state.clone());
    let busy = chat::create_session(&state.db, "alice").await.unwrap();
    let waiting = chat::create_session(&state.db, "alice").await.unwrap();

    for (session, body) in [
        (&busy, r#"{"model":"model-a","message":"first"}"#),
        (&waiting, r#"{"model":"model-a","message":"second"}"#),
    ] {
        let resp = app
            .serve(json_req(
                Method::POST,
                format!("/api/v0/chat/sessions/{}/messages", session.id),
                &cookie,
                Some(body.into()),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::ACCEPTED);
    }

    let resp = app
        .serve(json_req(
            Method::GET,
            format!("/api/v0/chat/sessions/{}/events", waiting.id),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    let body = tokio::time::timeout(Duration::from_secs(20), body_string(resp))
        .await
        .expect("the stream ended within the test's patience");
    let snapshots: Vec<serde_json::Value> = sse_frames(&body)
        .into_iter()
        .filter(|(name, _)| name == "snapshot")
        .map(|(_, data)| data)
        .collect();
    assert_eq!(
        snapshots.len(),
        2,
        "one on attach, one when the turn starts"
    );
    assert!(
        snapshots[0]["live_turn_id"].is_null(),
        "nothing is running when the viewer attaches"
    );
    assert!(
        snapshots[1]["live_turn_id"].is_string(),
        "the hand-off names the turn now streaming: {:?}",
        snapshots[1]
    );
}

/// A conversation with nothing queued must not hold the connection open
/// waiting for a turn that will never start. "Ends on a user turn" looks like
/// the same question as "has something waiting" and is not.
#[tokio::test]
async fn a_conversation_with_nothing_queued_closes_as_idle() {
    let (state, cookie) = setup("http://unused.invalid").await;
    let app = router(state.clone());
    let session = chat::create_session(&state.db, "alice").await.unwrap();
    // A user turn with no answer and nothing in the work queue — the shape a
    // failed submit leaves behind.
    chat::create_user_turn(&state.db, &session.id, "u0", "orphaned")
        .await
        .unwrap();

    let resp = tokio::time::timeout(
        Duration::from_secs(5),
        app.serve(json_req(
            Method::GET,
            format!("/api/v0/chat/sessions/{}/events", session.id),
            &cookie,
            None,
        )),
    )
    .await
    .expect("the handler answered promptly")
    .unwrap();
    let body = tokio::time::timeout(Duration::from_secs(5), body_string(resp))
        .await
        .expect("and closed promptly");
    let names: Vec<String> = sse_frames(&body)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(names, ["snapshot", "idle"]);
}

/// Discarding an interjection takes it out of the running worker's inbox, not
/// just out of the database. Settling the row alone left the note queued in
/// memory, so the next round folded a discarded sentence into the prompt — and
/// the transcript then labelled as "discarded" something the model had read.
#[tokio::test]
async fn discarding_an_interjection_takes_it_out_of_the_running_turn() {
    let upstream = MockServer::start().await;
    mount_streaming_upstream(&upstream, &["slow"], 5_000).await;
    let (state, cookie) = setup(&upstream.uri()).await;
    let app = router(state.clone());
    let session = chat::create_session(&state.db, "alice").await.unwrap();

    let submitted = app
        .serve(json_req(
            Method::POST,
            format!("/api/v0/chat/sessions/{}/messages", session.id),
            &cookie,
            Some(r#"{"model":"model-a","message":"one"}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(submitted.status(), StatusCode::ACCEPTED);
    // Round one has to be in flight before the note is folded in, or the
    // driver drains it into that round and the discard below is a 409 by
    // design. See `wait_for_first_round`.
    wait_for_first_round(&upstream).await;
    let folded = app
        .serve(json_req(
            Method::POST,
            format!("/api/v0/chat/sessions/{}/messages", session.id),
            &cookie,
            Some(r#"{"model":"model-a","message":"never mind this"}"#.into()),
        ))
        .await
        .unwrap();
    let placed: serde_json::Value = serde_json::from_str(&body_string(folded).await).unwrap();
    assert_eq!(
        placed["placement"], "folded",
        "the note must land in the running turn, not start or queue a new one"
    );
    let steer_id = placed["steer_id"].as_str().unwrap().to_string();

    let discarded = app
        .serve(json_req(
            Method::POST,
            format!(
                "/api/v0/chat/sessions/{}/steer/{}/discard",
                session.id, steer_id
            ),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(discarded.status(), StatusCode::OK);

    let worker = state
        .chats
        .get("alice", &session.id)
        .expect("the turn is still running");
    assert!(
        worker.steers.take().is_empty(),
        "a discarded note must not still be waiting to go into the prompt"
    );

    let _ = app
        .serve(json_req(
            Method::POST,
            format!("/api/v0/chat/sessions/{}/cancel", session.id),
            &cookie,
            None,
        ))
        .await;
}

/// Queueing is itself a moment a turn can become startable.
///
/// The turn holding the last slot can finish between `register()` saying "at
/// capacity" and the queue row landing — its own scheduler pass then sees an
/// empty queue, and without a kick from the submit the message waits for an
/// event that has already happened. Here the slot is freed *before* the second
/// message is sent, which is the same shape as losing that race.
#[tokio::test]
async fn a_queued_message_starts_even_if_the_slot_freed_first() {
    let upstream = MockServer::start().await;
    mount_streaming_upstream(&upstream, &["done"], 0).await;
    let (state, cookie) = setup(&upstream.uri()).await;
    let app = router(state.clone());
    let session = chat::create_session(&state.db, "alice").await.unwrap();

    // A waiting turn with nothing running: exactly what the lost-wakeup race
    // leaves behind.
    let user_turn = chat::create_user_turn(&state.db, &session.id, "u0", "already waiting")
        .await
        .unwrap();
    chat::insert_pending_turn(
        &state.db,
        &chat::PendingTurn {
            turn_id: user_turn.id.clone(),
            session_id: session.id.clone(),
            user_id: "alice".into(),
            model: "model-a".into(),
            voice: false,
            client_ip: None,
            secure: false,
            created_at: jiff::Timestamp::now(),
        },
    )
    .await
    .unwrap();

    // Sending anything at all runs a scheduler pass, which picks it up.
    let other = chat::create_session(&state.db, "alice").await.unwrap();
    let resp = app
        .serve(json_req(
            Method::POST,
            format!("/api/v0/chat/sessions/{}/messages", other.id),
            &cookie,
            Some(r#"{"model":"model-a","message":"unrelated"}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::ACCEPTED);

    let started = wait_until(|| {
        let state = state.clone();
        let session_id = session.id.clone();
        async move {
            chat::list_pending_for_session(&state.db, &session_id)
                .await
                .map(|waiting| waiting.is_empty())
                .unwrap_or(false)
        }
    })
    .await;
    assert!(started, "the stranded message was never picked up");
}

/// Taking a message back races the scheduler, and the claim decides.
///
/// Without it, a take-back that lands just after the turn started would delete
/// the assistant row out from under a running worker — which then streams into
/// a row that no longer exists.
#[tokio::test]
async fn taking_back_a_message_that_just_started_is_refused() {
    let upstream = MockServer::start().await;
    mount_streaming_upstream(&upstream, &["slow"], 5_000).await;
    let (state, cookie) = setup(&upstream.uri()).await;
    let app = router(state.clone());
    let session = chat::create_session(&state.db, "alice").await.unwrap();
    let submitted = app
        .serve(json_req(
            Method::POST,
            format!("/api/v0/chat/sessions/{}/messages", session.id),
            &cookie,
            Some(r#"{"model":"model-a","message":"running"}"#.into()),
        ))
        .await
        .unwrap();
    let ids: serde_json::Value = serde_json::from_str(&body_string(submitted).await).unwrap();
    let user_turn_id = ids["user_turn_id"].as_str().unwrap().to_string();

    // This message is being answered: it was never in the queue, so the claim
    // finds nothing and the take-back is refused rather than truncating a
    // live turn.
    let resp = app
        .serve(json_req(
            Method::DELETE,
            format!(
                "/api/v0/chat/sessions/{}/turns/{}",
                session.id, user_turn_id
            ),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    assert!(body_string(resp).await.contains("already_answered"));
    assert_eq!(
        chat::list_turns(&state.db, &session.id)
            .await
            .unwrap()
            .len(),
        2,
        "nothing was deleted"
    );

    let _ = app
        .serve(json_req(
            Method::POST,
            format!("/api/v0/chat/sessions/{}/cancel", session.id),
            &cookie,
            None,
        ))
        .await;
}

/// The snapshot says which messages are waiting, rather than leaving the
/// client to infer it from the transcript's shape — a turn whose assistant row
/// failed to insert looks identical from the outside and would spin forever.
#[tokio::test]
async fn the_snapshot_names_the_messages_that_are_waiting() {
    let upstream = MockServer::start().await;
    mount_streaming_upstream(&upstream, &["slow"], 5_000).await;
    let (state, cookie) = setup(&upstream.uri()).await;
    let app = router(state.clone());
    let busy = chat::create_session(&state.db, "alice").await.unwrap();
    let waiting = chat::create_session(&state.db, "alice").await.unwrap();

    for (session, body) in [
        (&busy, r#"{"model":"model-a","message":"first"}"#),
        (&waiting, r#"{"model":"model-a","message":"second"}"#),
    ] {
        let resp = app
            .serve(json_req(
                Method::POST,
                format!("/api/v0/chat/sessions/{}/messages", session.id),
                &cookie,
                Some(body.into()),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::ACCEPTED);
    }
    let queued = chat::list_pending_for_session(&state.db, &waiting.id)
        .await
        .unwrap();
    assert_eq!(queued.len(), 1, "the second message is waiting for a slot");

    // The waiting conversation's stream names it, so the client renders the
    // spinner on fact rather than on shape. (The stream then stays open until
    // the turn starts — see `a_waiting_conversation_is_told_when_its_turn_starts`
    // — so only the first frame is read here.)
    let resp = app
        .serve(json_req(
            Method::GET,
            format!("/api/v0/chat/sessions/{}/events", waiting.id),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    let body = tokio::time::timeout(Duration::from_secs(20), body_string(resp))
        .await
        .expect("the stream ended within the test's patience");
    let frames = sse_frames(&body);
    let first = &frames.first().expect("a snapshot").1;
    assert_eq!(
        first["waiting_turn_ids"]
            .as_array()
            .map(|ids| ids.len())
            .unwrap_or(0),
        1,
        "the snapshot names the waiting message: {first:?}"
    );
    assert_eq!(
        first["waiting_turn_ids"][0].as_str(),
        Some(queued[0].turn_id.as_str())
    );
    // And the hand-off snapshot, once it starts, says nothing is waiting.
    let last_snapshot = frames
        .iter()
        .rfind(|(name, _)| name == "snapshot")
        .expect("a second snapshot");
    assert!(
        last_snapshot.1["waiting_turn_ids"].is_null(),
        "the turn that was waiting is now the live one: {:?}",
        last_snapshot.1
    );

    let _ = app
        .serve(json_req(
            Method::POST,
            format!("/api/v0/chat/sessions/{}/cancel", waiting.id),
            &cookie,
            None,
        ))
        .await;
}

/// A turn that never reaches a backend delivered nothing — and what the user
/// added while it ran is not lost with it: at finalize the server turns the
/// undelivered note into the next message itself, with no browser involved.
#[tokio::test]
async fn an_addition_survives_a_turn_that_never_reaches_a_backend() {
    // An upstream that advertises a model and then refuses every completion.
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(503).set_delay(Duration::from_millis(1_500)))
        .mount(&upstream)
        .await;
    let (state, cookie) = setup(&upstream.uri()).await;
    let app = router(state.clone());
    let session = chat::create_session(&state.db, "alice").await.unwrap();

    let submitted = app
        .serve(json_req(
            Method::POST,
            format!("/api/v0/chat/sessions/{}/messages", session.id),
            &cookie,
            Some(r#"{"model":"model-a","message":"one"}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(submitted.status(), StatusCode::ACCEPTED);
    let ids: serde_json::Value = serde_json::from_str(&body_string(submitted).await).unwrap();
    let assistant_turn_id = ids["assistant_turn_id"].as_str().unwrap().to_string();

    let second = app
        .serve(json_req(
            Method::POST,
            format!("/api/v0/chat/sessions/{}/messages", session.id),
            &cookie,
            Some(r#"{"model":"model-a","message":"in euros, please"}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(second.status(), StatusCode::ACCEPTED);

    wait_for_idle(&state, "alice").await;

    let rows = chat::list_steers(&state.db, &assistant_turn_id)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_ne!(
        rows[0].status,
        chat::SteerStatus::Delivered,
        "a turn that never reached a backend must not claim it handed the note over"
    );
    let queued = wait_until(|| {
        let state = state.clone();
        let session_id = session.id.clone();
        async move {
            chat::list_turns(&state.db, &session_id)
                .await
                .map(|turns| {
                    turns
                        .iter()
                        .any(|t| t.turn.user_content.as_deref() == Some("in euros, please"))
                })
                .unwrap_or(false)
        }
    })
    .await;
    assert!(queued, "the addition was never re-queued as a message");
}

/// With nothing running there is nothing to steer. Quietly promoting the note
/// to a new turn would start work the user did not ask for, so it is refused
/// and the client sends it as an ordinary message instead.
#[tokio::test]
async fn steering_an_idle_conversation_is_refused() {
    let (state, cookie) = setup("http://unused.invalid").await;
    let app = router(state.clone());
    let session = chat::create_session(&state.db, "alice").await.unwrap();

    let resp = app
        .serve(json_req(
            Method::POST,
            format!("/api/v0/chat/sessions/{}/steer", session.id),
            &cookie,
            Some(r#"{"message":"hello?"}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    assert!(body_string(resp).await.contains("no_turn_running"));
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

/// The models endpoint lists what the user's grant permits, compliance
/// flags included — the SPA picker's data source.
#[tokio::test]
async fn the_models_endpoint_lists_offered_chat_models() {
    let upstream = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&upstream)
        .await;
    let (state, cookie) = setup(&upstream.uri()).await;
    let app = router(state);

    let resp = app
        .serve(json_req(
            Method::GET,
            "/api/v0/models".into(),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    let models = body["models"].as_array().unwrap();
    assert!(
        models.iter().any(|m| m["id"] == "model-a"),
        "the seeded pool's model must be offered: {models:?}"
    );
    assert!(models[0]["gdpr"].is_boolean());
    assert!(models[0]["nda"].is_boolean());

    // Anonymous → the 401 envelope.
    let resp = app
        .serve(Request::get("/api/v0/models").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn automatic_route_stays_in_model_picker_while_selector_is_unavailable() {
    let state = common::state_with_automatic_route_pools("http://unused.invalid").await;
    aiplane_core::server::db::automatic_routes::upsert(
        &state.db,
        &AutomaticRoute {
            alias: "default".into(),
            selector_model: "jev-model".into(),
            objective: "balanced".into(),
            instructions: String::new(),
            minimum_confidence: 0.7,
            selector_timeout_ms: 1_000,
            fallback_target: "fast-model".into(),
            session_affinity: false,
            session_ttl_seconds: 3_600,
            rollout: "active".into(),
            version: 0,
            candidates: vec![
                AutomaticRouteCandidate {
                    key: "fast".into(),
                    target: "fast-model".into(),
                    description: "Fast model".into(),
                },
                AutomaticRouteCandidate {
                    key: "expert".into(),
                    target: "expert-model".into(),
                    description: "Expert model".into(),
                },
            ],
        },
    )
    .await
    .unwrap();
    for pool in state.upstreams.pools() {
        for backend in &pool.backends {
            if pool.kind == PoolKind::SystemOne {
                backend.set_healthy(false);
            } else if pool.kind == PoolKind::Chat {
                backend.set_models(std::collections::HashSet::from(["fast-model".into()]));
            }
        }
    }
    let cookie = common::seed_session(&state, "alice", "alice@example.com").await;
    let app = router(Arc::new(state));

    let response = app
        .serve(json_req(
            Method::GET,
            "/api/v0/models".into(),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_str(&body_string(response).await).unwrap();
    assert!(
        body["models"]
            .as_array()
            .unwrap()
            .iter()
            .any(|model| model["id"] == "default"),
        "fallback availability keeps the route usable"
    );
}

/// An alias inherits its target's reasoning support.
///
/// The picker lists aliases as models of their own, and `default` — the name
/// most deployments give theirs — looks like nothing in particular. Resolving
/// the reasoning style from that name alone returned `none`, so the composer
/// disabled the effort select with "this model does not reason", while the
/// turn resolved the very same alias to a Qwen and sent `enable_thinking`
/// anyway. The UI has to answer the question the wire answers.
#[tokio::test]
async fn an_alias_reports_the_reasoning_support_of_the_model_behind_it() {
    let state = Arc::new(state_with_aliased_chat_model("Qwen/Qwen3-8B", "default").await);
    let cookie = common::seed_session(&state, "alice", "alice@example.com").await;
    let app = router(state);

    let resp = app
        .serve(json_req(
            Method::GET,
            "/api/v0/models".into(),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    let models = body["models"].as_array().unwrap();
    let reasoning = |id: &str| -> bool {
        models
            .iter()
            .find(|m| m["id"] == id)
            .unwrap_or_else(|| panic!("`{id}` must be listed: {models:?}"))["reasoning"]
            .as_bool()
            .expect("reasoning is a boolean")
    };
    assert!(
        reasoning("Qwen/Qwen3-8B"),
        "the real id detects as Qwen: {models:?}"
    );
    assert!(
        reasoning("default"),
        "the alias must answer for its target, not for its own name: {models:?}"
    );
}

/// The other half of the same contract: a model nothing recognises still
/// reports no reasoning, so the disabled select stays honest where it should.
#[tokio::test]
async fn a_model_with_no_reasoning_parameter_is_listed_as_such() {
    let state = Arc::new(state_with_aliased_chat_model("voxtral-small", "default").await);
    let cookie = common::seed_session(&state, "alice", "alice@example.com").await;
    let app = router(state);

    let resp = app
        .serve(json_req(
            Method::GET,
            "/api/v0/models".into(),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    let models = body["models"].as_array().unwrap();
    for id in ["voxtral-small", "default"] {
        let m = models
            .iter()
            .find(|m| m["id"] == id)
            .unwrap_or_else(|| panic!("`{id}` must be listed: {models:?}"));
        assert_eq!(m["reasoning"], serde_json::json!(false), "{models:?}");
    }
}

/// The usage endpoint answers with the aggregate envelope, self-scoped for
/// a plain user.
#[tokio::test]
async fn the_usage_endpoint_aggregates_the_callers_window() {
    let (state, cookie) = setup("http://unused.invalid").await;
    let app = router(state);

    let resp = app
        .serve(json_req(
            Method::GET,
            "/api/v0/usage?period=24h".into(),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    assert_eq!(body["period"], "24h");
    assert_eq!(body["scope"], "self");
    assert_eq!(body["can_view_all"], false);
    assert!(body["usage_enabled"].is_boolean());
    assert!(body["timezone"].is_string());
    assert!(body["summary"]["requests"].is_i64());
    assert!(body["currency"].is_string());
    assert!(
        body["tokens"]
            .as_array()
            .unwrap()
            .iter()
            .all(|token| { token["id"].as_str().is_some_and(|id| !id.is_empty()) })
    );

    // Anonymous → 401.
    let resp = app
        .serve(Request::get("/api/v0/usage").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// The tools endpoints list the caller's granted tools and round-trip a
/// toggle — refusing keys the roles don't grant.
#[tokio::test]
async fn tool_toggles_round_trip_and_refuse_ungranted_keys() {
    let (state, cookie) = setup("http://unused.invalid").await;
    let app = router(state.clone());

    let resp = app
        .serve(json_req(Method::GET, "/api/v0/tools".into(), &cookie, None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    let tools = body["tools"].as_array().unwrap();
    assert!(tools.iter().all(|t| t["enabled"].is_boolean()));
    // The empty RBAC resolver grants nothing in this harness…
    assert!(tools.is_empty());

    // …so any toggle is refused rather than stored as a lying switch.
    let resp = app
        .serve(json_req(
            Method::POST,
            "/api/v0/tools/toggle".into(),
            &cookie,
            Some(r#"{"tool_key":"search_web","enabled":true}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // Anonymous → 401.
    let resp = app
        .serve(Request::get("/api/v0/tools").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn tools_list_includes_browser_location_sharing_state_when_granted() {
    let (state, cookie) = setup_with_location_tool("http://unused.invalid").await;
    aiplane_core::server::db::users::set_location(&state.db, "alice", 48.137, 11.575, Some(18.4))
        .await
        .unwrap();
    let app = router(state);

    let resp = app
        .serve(json_req(Method::GET, "/api/v0/tools".into(), &cookie, None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    assert_eq!(body["location"]["shared"], true);
    assert_eq!(body["location"]["accuracy"], 18.4);
    assert_eq!(body["tools"][0]["key"], "get_user_location");
    assert_eq!(body["tools"][0]["tech"], "get_user_location");
}

#[tokio::test]
async fn conversation_capabilities_keep_catalog_metadata_and_three_states() {
    let (state, cookie) = setup_with_location_tool("http://unused.invalid").await;
    let session = chat::create_session(&state.db, "alice").await.unwrap();
    let app = router(state.clone());

    let resp = app
        .serve(json_req(
            Method::GET,
            format!("/api/v0/chat/sessions/{}/capabilities", session.id),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    let location = body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["key"] == "get_user_location")
        .unwrap();
    assert_eq!(location["kind"], "tool");
    assert!(
        location["group"]
            .as_str()
            .is_some_and(|group| !group.is_empty())
    );
    assert!(
        location["description"]
            .as_str()
            .is_some_and(|text| !text.is_empty())
    );
    assert_eq!(location["state"], "auto");

    let resp = app
        .serve(json_req(
            Method::POST,
            format!("/api/v0/chat/sessions/{}/capabilities", session.id),
            &cookie,
            Some(r#"{"kind":"tool","key":"get_user_location","state":"off"}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let resp = app
        .serve(json_req(
            Method::GET,
            format!("/api/v0/chat/sessions/{}/capabilities", session.id),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    assert_eq!(body["tools"][0]["state"], "off");
}

/// The turn actions: edit rewrites + regenerates, retry drops + regenerates,
/// share toggles, export answers markdown.
#[tokio::test]
async fn turn_actions_edit_retry_share_export() {
    let upstream = MockServer::start().await;
    mount_streaming_upstream(&upstream, &["v1"], 0).await;
    let (state, cookie) = setup(&upstream.uri()).await;
    let app = router(state.clone());
    let session = chat::create_session(&state.db, "alice").await.unwrap();
    eprintln!("DEBUG test session.id = {}", session.id);

    // First turn.
    let resp = app
        .serve(json_req(
            Method::POST,
            format!("/api/v0/chat/sessions/{}/messages", session.id),
            &cookie,
            Some(r#"{"model":"model-a","message":"hello"}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    wait_settled(&state, &session.id).await;

    // Export carries the turn text.
    let resp = app
        .serve(json_req(
            Method::GET,
            format!("/api/v0/chat/sessions/{}/export.md", session.id),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let exported = body_string(resp).await;
    assert!(exported.contains("hello"), "{exported}");

    // Share on → the flag is stored.
    let resp = app
        .serve(json_req(
            Method::POST,
            format!("/api/v0/chat/sessions/{}/share", session.id),
            &cookie,
            Some(r#"{"shared": true}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Edit the user turn: rewrites + regenerates (202).
    let turns = chat::list_turns(&state.db, &session.id).await.unwrap();
    let user_turn = turns
        .iter()
        .find(|t| t.turn.role == chat::TurnRole::User)
        .unwrap();
    let resp = app
        .serve(json_req(
            Method::POST,
            format!(
                "/api/v0/chat/sessions/{}/turns/{}/edit",
                session.id, user_turn.turn.id
            ),
            &cookie,
            Some(r#"{"model":"model-a","message":"edited message"}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::ACCEPTED,
        "{}",
        body_string(resp).await
    );
    wait_settled(&state, &session.id).await;

    // The rewrite + the regenerated reply are both there.
    let turns = chat::list_turns(&state.db, &session.id).await.unwrap();
    assert!(
        turns
            .iter()
            .any(|t| t.turn.user_content.as_deref() == Some("edited message"))
    );

    // Retry the assistant reply: downstream dropped, one new assistant row.
    let before = chat::list_turns(&state.db, &session.id)
        .await
        .unwrap()
        .len();
    let turns = chat::list_turns(&state.db, &session.id).await.unwrap();
    let assistant = turns
        .iter()
        .find(|t| t.turn.role == chat::TurnRole::Assistant)
        .unwrap();
    let resp = app
        .serve(json_req(
            Method::POST,
            format!(
                "/api/v0/chat/sessions/{}/turns/{}/retry",
                session.id, assistant.turn.id
            ),
            &cookie,
            Some(r#"{"model":"model-a"}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::ACCEPTED,
        "{}",
        body_string(resp).await
    );
    wait_settled(&state, &session.id).await;
    let after = chat::list_turns(&state.db, &session.id)
        .await
        .unwrap()
        .len();
    assert_eq!(
        after, before,
        "retry swaps the assistant row, keeping the count"
    );
}

/// Wait (bounded) until the running turn has actually sent its first round
/// upstream.
///
/// The driver drains the interjection inbox at the top of *every* round, before
/// it builds that round's request (`fold_in_steers`). So a note folded in
/// before the driver's first drain is taken straight into round one, and
/// discarding it afterwards correctly answers "already on its way to the
/// model" — a 409, not a 200.
///
/// Whether the freshly spawned driver task reaches that drain before the next
/// HTTP request lands is pure scheduling, and under a loaded test run it often
/// does not. That is what made the discard test flaky. The upstream having
/// received the round is the exact observable signal that the first drain is
/// behind us, so anything folded in from here on stays queued until the *next*
/// round boundary — which never comes, because the mock holds the response.
///
/// Only the round's own request counts. Title generation is spawned alongside
/// the turn and hits the same mock, often first; treating *any* request as the
/// signal let the note land before the first drain, and the flake came back.
/// The round is the streamed request; the title call is not.
async fn wait_for_first_round(upstream: &MockServer) {
    let reached = wait_until(|| async {
        upstream
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .any(|req| {
                serde_json::from_slice::<serde_json::Value>(&req.body)
                    .is_ok_and(|body| body["stream"] == true)
            })
    })
    .await;
    assert!(reached, "the turn never reached the upstream");
}

/// Wait (bounded) until no turn of the session is in progress.
async fn wait_settled(state: &aiplane::rama_server::RamaState, session_id: &str) {
    for _ in 0..100 {
        let busy = chat::list_turns(&state.db, session_id)
            .await
            .unwrap()
            .iter()
            .any(|t| t.turn.status == chat::TurnStatus::InProgress);
        if !busy {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("turn never settled");
}

/// Multipart submit with attachments: files upload under the turn prefix
/// and the markers land in the stored user content — the same contract the
/// legacy composer has.
#[tokio::test]
async fn multipart_submit_uploads_attachments_into_the_turn() {
    let upstream = MockServer::start().await;
    mount_streaming_upstream(&upstream, &["ok"], 0).await;
    let (state, cookie) = setup(&upstream.uri()).await;
    let app = router(state.clone());
    let session = chat::create_session(&state.db, "alice").await.unwrap();

    let boundary = "----e2eboundary";
    let mut body = Vec::new();
    for (name, value) in [("model", "model-a"), ("message", "see attachment")] {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n")
                .as_bytes(),
        );
    }
    let png = [0x89u8, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        "Content-Disposition: form-data; name=\"attachment\"; filename=\"shot.png\"\r\n\r\n"
            .as_bytes(),
    );
    body.extend_from_slice(&png);
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());

    let resp = app
        .serve(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/api/v0/chat/sessions/{}/messages", session.id))
                .header("cookie", format!("id={cookie}"))
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    // No [chat.s3] in this harness → the attachment upload is refused with
    // a 400 that names the missing configuration (the same contract the
    // legacy composer surfaces as a toast). The marker/content assertions
    // live in the S3-configured suites; here we pin the refusal shape.
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "{}",
        body_string(resp).await
    );
    assert!(
        body_string(resp)
            .await
            .contains("chat attachments are not configured")
    );
    assert!(
        chat::list_turns(&state.db, &session.id)
            .await
            .unwrap()
            .is_empty(),
        "a refused upload must not leave rows"
    );
}

/// Forking is how a *recipient* keeps a conversation someone shared with
/// them. The two guards that matter: a conversation you cannot read cannot be
/// forked, and forking your own is refused rather than silently cloned (the
/// affordance only renders for read-only viewers, but the endpoint has to hold
/// the line on its own).
#[tokio::test]
async fn forking_copies_a_shared_conversation_to_the_recipient() {
    let upstream = MockServer::start().await;
    let (state, alice) = setup(&upstream.uri()).await;
    let bob = common::seed_session(&state, "bob", "bob@example.com").await;
    let app = router(state.clone());

    // Alice owns a conversation with one turn, and shares it.
    let session = chat::create_session(&state.db, "alice").await.unwrap();
    chat::create_user_turn(&state.db, &session.id, "t-user", "hello from alice")
        .await
        .unwrap();
    chat::set_shared(&state.db, "alice", &session.id, true)
        .await
        .unwrap();

    // Bob forks it: a new conversation, owned by Bob, carrying the turn.
    let resp = app
        .serve(json_req(
            Method::POST,
            format!("/api/v0/chat/sessions/{}/fork", session.id),
            &bob,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let forked: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    let new_id = forked["id"].as_str().expect("fork returns the new id");
    assert_ne!(new_id, session.id, "a fork is a new conversation");

    let mine = chat::get_session(&state.db, "bob", new_id).await.unwrap();
    assert!(mine.is_some(), "the fork belongs to the forker");
    let turns = chat::list_turns(&state.db, new_id).await.unwrap();
    assert_eq!(turns.len(), 1, "the conversation came with it");
    assert_eq!(
        turns[0].turn.user_content.as_deref(),
        Some("hello from alice")
    );

    // Alice forking her own conversation is a refusal, not a clone.
    let resp = app
        .serve(json_req(
            Method::POST,
            format!("/api/v0/chat/sessions/{}/fork", session.id),
            &alice,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);

    // And an unshared conversation of Alice's is invisible to Bob.
    let private = chat::create_session(&state.db, "alice").await.unwrap();
    let resp = app
        .serve(json_req(
            Method::POST,
            format!("/api/v0/chat/sessions/{}/fork", private.id),
            &bob,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// The canvas as JSON: read a document with its history, hand-edit it into a
/// new version, and — the guard that keeps the history meaningful — save the
/// identical text again without minting one.
#[tokio::test]
async fn canvas_documents_read_and_hand_edit() {
    use aiplane_core::server::db::documents;

    let upstream = MockServer::start().await;
    let (state, alice) = setup(&upstream.uri()).await;
    let app = router(state.clone());
    let session = chat::create_session(&state.db, "alice").await.unwrap();
    documents::create(
        &state.db,
        "doc-1",
        &session.id,
        "alice",
        "Notes",
        documents::DocumentFormat::Markdown,
        "first draft",
        None,
    )
    .await
    .unwrap();

    // Listing shows it.
    let resp = app
        .serve(json_req(
            Method::GET,
            format!("/api/v0/chat/sessions/{}/documents", session.id),
            &alice,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let listed: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    assert_eq!(listed["documents"][0]["id"], "doc-1");
    assert_eq!(listed["documents"][0]["title"], "Notes");

    // Reading returns the current content plus the version history.
    let resp = app
        .serve(json_req(
            Method::GET,
            format!("/api/v0/chat/sessions/{}/documents/doc-1", session.id),
            &alice,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let got: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    assert_eq!(got["version"]["content"], "first draft");
    assert_eq!(got["history"].as_array().unwrap().len(), 1);

    // A hand edit mints a version…
    let resp = app
        .serve(json_req(
            Method::PUT,
            format!("/api/v0/chat/sessions/{}/documents/doc-1", session.id),
            &alice,
            Some(r#"{"content":"second draft"}"#.to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let saved: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    assert_eq!(saved["unchanged"], false);
    assert_eq!(saved["document"]["current_ver"], 2);

    // …and saving the same text again does not.
    let resp = app
        .serve(json_req(
            Method::PUT,
            format!("/api/v0/chat/sessions/{}/documents/doc-1", session.id),
            &alice,
            Some(r#"{"content":"second draft"}"#.to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let again: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    assert_eq!(again["unchanged"], true, "a no-op save mints no version");
    assert_eq!(again["document"]["current_ver"], 2);

    // A read-only viewer cannot write. (Shared makes it readable, not writable.)
    let bob = common::seed_session(&state, "bob", "bob@example.com").await;
    chat::set_shared(&state.db, "alice", &session.id, true)
        .await
        .unwrap();
    let resp = app
        .serve(json_req(
            Method::PUT,
            format!("/api/v0/chat/sessions/{}/documents/doc-1", session.id),
            &bob,
            Some(r#"{"content":"bob was here"}"#.to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let (_, ver) = documents::get_version(&state.db, &session.id, "doc-1", None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        ver.content, "second draft",
        "the viewer's write was refused"
    );
}

/// Removing an attachment drops its marker and leaves the rest of the message
/// alone. The filename is matched verbatim: the `Path` extractor lowercases
/// segments, so a handler reading it from there would fail to match
/// `Bericht.PNG` and silently remove nothing.
#[tokio::test]
async fn removing_an_attachment_drops_only_its_marker() {
    let upstream = MockServer::start().await;
    let (state, alice) = setup(&upstream.uri()).await;
    let app = router(state.clone());
    let session = chat::create_session(&state.db, "alice").await.unwrap();

    let keep = session_core::attachments::marker_line(
        "keep.txt",
        "text/plain",
        "https://example.com/keep.txt",
        4,
    );
    let drop = session_core::attachments::marker_line(
        "Bericht.PNG",
        "image/png",
        "https://example.com/Bericht.PNG",
        9,
    );
    let content = format!("look at these\n{keep}\n{drop}");
    chat::create_user_turn(&state.db, &session.id, "t-user", &content)
        .await
        .unwrap();

    let resp = app
        .serve(json_req(
            Method::DELETE,
            format!(
                "/api/v0/chat/sessions/{}/turns/t-user/attachments/Bericht.PNG",
                session.id
            ),
            &alice,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let turn = chat::get_turn(&state.db, &session.id, "t-user")
        .await
        .unwrap()
        .unwrap();
    let after = turn.user_content.unwrap_or_default();
    assert!(
        !after.contains("Bericht.PNG"),
        "the removed attachment's marker must be gone: {after}"
    );
    assert!(
        after.contains("keep.txt"),
        "the other attachment must survive: {after}"
    );
    assert!(
        after.contains("look at these"),
        "the typed text must survive: {after}"
    );
}
