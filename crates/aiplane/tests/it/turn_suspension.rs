// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Durable suspend and resume of a chat turn (`docs/agent-hil.md` → "Suspend and resume"),
//! end to end.
//!
//! A tool wrapped in `AskFirst` pauses its turn for the owner's approval. What
//! this pins, through the real router, worker and SQLite file:
//!   - the stream tells the client what the turn waits for, then ends without
//!     calling the turn finished;
//!   - the pause survives a restart — the second process is built from the
//!     same database file and nothing else — and
//!     `POST …/turns/{turn_id}/resume` continues the same turn to its answer;
//!   - a denial reaches the model as a tool error, an expiry takes the
//!     timeout fallback;
//!   - only the turn's owner may answer, only with a decision the request
//!     offers, and only while it is waiting;
//!   - a paused conversation refuses new messages until it is settled, and
//!     cancelling it gives up on the decision;
//!   - a connector tool in `ask` mode is offered in chat and waits the same
//!     way: it runs once approved and never when denied;
//!   - `schedule_action` asks the same way, and its approval survives a
//!     restart.

use crate::common;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use aiplane::rama_server::{RamaState, SessionStore, router::router};
use aiplane_core::server::db;
use aiplane_core::server::db::mcp_catalog;
use aiplane_core::server::rbac::Resolver;
use aiplane_core::server::rbac::config::{RbacConfig, RoleConfig};
use aiplane_core::server::upstreams::{
    self,
    config::{PickerStrategy, PoolKind, UpstreamPoolConfig},
};
use aiplane_runtime::server::AppState;
use aiplane_runtime::server::tools::ToolRegistry;
use aiplane_runtime::server::tools::ask_first::AskFirst;
use aiplane_runtime::server::tools::echo::Echo;
use common::Service as _;
use rama::http::body::util::BodyExt;
use rama::http::{Body, Method, Request, StatusCode};
use serde_json::{Value, json};
use session_core::db::{self as chat, TimeoutFallback, ToolCallStatus, TurnStatus};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Answers each upstream round with the next scripted delta. Lives on the
/// mock server, so it keeps counting across the simulated restart.
pub(crate) struct Scripted {
    pub(crate) deltas: Vec<Value>,
    pub(crate) served: AtomicUsize,
    pub(crate) delay: Duration,
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

pub(crate) fn tool_call(name: &str, arguments: Value) -> Value {
    json!({"tool_calls": [{"index": 0, "id": "call-1", "type": "function",
        "function": {"name": name, "arguments": arguments.to_string()}}]})
}

fn echo_call() -> Value {
    tool_call("company_echo", json!({"message": "ship it"}))
}

/// An upstream whose first round calls `call` and whose second answers.
async fn upstream_calling(call: Value, answer: &str, delay: Duration) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(Scripted {
            deltas: vec![call, json!({"content": answer})],
            served: AtomicUsize::new(0),
            delay,
        })
        .mount(&server)
        .await;
    server
}

async fn upstream(answer: &str, delay: Duration) -> MockServer {
    upstream_calling(echo_call(), answer, delay).await
}

/// One process's state over the database file at `db_path`: migrations and
/// the startup sweep run, the registries are new.
async fn boot(db_path: &Path, upstream_url: &str, approval_timeout: Duration) -> Arc<RamaState> {
    boot_with(
        db_path,
        upstream_url,
        ToolRegistry::new().with(AskFirst::new(Echo, approval_timeout)),
        &["company_echo"],
    )
    .await
}

/// [`boot`] with the tools the registry holds and the member role grants.
pub(crate) async fn boot_with(
    db_path: &Path,
    upstream_url: &str,
    tools: ToolRegistry,
    granted: &[&str],
) -> Arc<RamaState> {
    boot_as(db_path, upstream_url, tools, granted, false).await
}

/// [`boot_with`], everyone an admin when `admin`: they hold every
/// management permission, so they reach the personal-agent routes.
pub(crate) async fn boot_as(
    db_path: &Path,
    upstream_url: &str,
    tools: ToolRegistry,
    granted: &[&str],
    admin: bool,
) -> Arc<RamaState> {
    let pool = db::open(db_path).await.unwrap();
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
            backend: vec![common::mock_backend("mock", upstream_url)],
        },
    );
    let registry = upstreams::UpstreamRegistry::new(&pools).unwrap();
    common::seed_pool_models(&registry, "pool", 0, &["model-a"]);
    let rbac = Resolver::build(
        RbacConfig {
            default_role: Some("member".into()),
            mappings: vec![],
        },
        vec![RoleConfig {
            id: "member".into(),
            admin,
            tools: granted.iter().map(|t| t.to_string()).collect(),
            skills: vec![],
        }],
    )
    .unwrap();
    let app = AppState::new(
        common::test_config(),
        pool.clone(),
        registry,
        Arc::new(tools),
        Arc::new(rbac),
    );
    let sessions = SessionStore::new(pool, common::TEST_SECRET);
    Arc::new(RamaState::new(
        app,
        sessions,
        aiplane_core::server::usage::UsageHandle::disabled(),
    ))
}

const AN_HOUR: Duration = Duration::from_secs(3600);

#[derive(Clone, Copy)]
enum DotResumeCase {
    ApproveAfterRestart,
    Deny,
    SelectionRevoked,
    UserToolDisabled,
}

async fn dot_memory_approval(case: DotResumeCase) {
    use aiplane_agents::db::{agents, dot_memories, dot_sessions, system_principals};
    use aiplane_tools::memory::Remember;
    let directory = tempfile::tempdir().unwrap();
    let db_path = directory.path().join("dots.db");
    let server = upstream_calling(
        tool_call(
            "remember",
            json!({"content": "Private approved lesson", "kind": "fact"}),
        ),
        "Done.",
        Duration::ZERO,
    )
    .await;
    let mut state = boot_with(
        &db_path,
        &server.uri(),
        ToolRegistry::new().with(Remember),
        &["remember"],
    )
    .await;
    let cookie = common::seed_session(&state, "alice", "alice@example.com").await;
    let (_, dot) = agents::create_personal_dot(
        &state.db,
        &system_principals::NewPrincipal { name: "approval-dot", display: "Approval Dot", description: "" },
        r#"{"main":{"tools":["remember"],"tool_resources":{"remember":{"permission":"always_ask"}}}}"#,
        "alice", 4,
    ).await.unwrap();
    let dot_id = dot.unwrap().principal.id;
    let session_id = conversation(&state).await;
    assert!(
        dot_sessions::link(&state.db, "alice", &dot_id, &session_id)
            .await
            .unwrap()
    );
    let turn_id = submit(&state, &cookie, &session_id).await;
    let paused = wait_for_status(&state, &session_id, &turn_id, TurnStatus::Suspended).await;
    assert!(paused.suspension.is_some());
    assert!(
        dot_memories::list(
            &state.db,
            aiplane_agents::db::dot_memories::MemoryScope::Dot {
                owner: &aiplane_agents::db::agents::DotOwner::User("alice".into()),
                dot_id: &dot_id
            },
            10
        )
        .await
        .unwrap()
        .unwrap()
        .is_empty()
    );
    let app = router(state.clone());
    let inbox = json_body(
        app.serve(get("/api/v0/agents/inbox".into(), &cookie))
            .await
            .unwrap(),
    )
    .await;
    let item = inbox["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["turn_id"] == turn_id)
        .unwrap();
    assert_eq!(item["call"]["name"], "remember");
    let request_id = item["id"].as_str().unwrap().to_string();
    let bob = common::seed_session(&state, "bob", "bob@example.com").await;
    let other_inbox = json_body(
        app.serve(get("/api/v0/agents/inbox".into(), &bob))
            .await
            .unwrap(),
    )
    .await;
    assert!(other_inbox["items"].as_array().unwrap().is_empty());
    match case {
        DotResumeCase::ApproveAfterRestart => {
            state = boot_with(
                &db_path,
                &server.uri(),
                ToolRegistry::new().with(Remember),
                &["remember"],
            )
            .await;
        }
        DotResumeCase::SelectionRevoked => {
            agents::update_draft(
                &state.db,
                &dot_id,
                r#"{"main":{"tools":[]}}"#,
                "alice",
                agents::DraftChange::default(),
            )
            .await
            .unwrap()
            .unwrap();
        }
        DotResumeCase::UserToolDisabled => {
            db::user_tool_prefs::set(&state.db, "alice", "memory", false)
                .await
                .unwrap();
        }
        DotResumeCase::Deny => {}
    }
    let decision = if matches!(case, DotResumeCase::Deny) {
        "deny"
    } else {
        "allow_once"
    };
    let (status, body) = if matches!(case, DotResumeCase::ApproveAfterRestart) {
        let response = router(state.clone())
            .serve(post(
                format!("/api/v0/agents/inbox/{request_id}/answer"),
                &cookie,
                json!({"decision": decision}),
            ))
            .await
            .unwrap();
        (response.status(), json_body(response).await)
    } else {
        resume(
            &state,
            &cookie,
            &session_id,
            &turn_id,
            json!({"decision": decision}),
        )
        .await
    };
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let done = wait_for_status(&state, &session_id, &turn_id, TurnStatus::Completed).await;
    let memories = dot_memories::list(
        &state.db,
        aiplane_agents::db::dot_memories::MemoryScope::Dot {
            owner: &aiplane_agents::db::agents::DotOwner::User("alice".into()),
            dot_id: &dot_id,
        },
        10,
    )
    .await
    .unwrap()
    .unwrap();
    if matches!(case, DotResumeCase::ApproveAfterRestart) {
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].content, "Private approved lesson");
        assert_eq!(done.tool_calls[0].status, ToolCallStatus::Completed);
    } else {
        assert!(
            memories.is_empty(),
            "a denied or revoked call must not write"
        );
        assert_eq!(done.tool_calls[0].status, ToolCallStatus::Errored);
    }
    assert!(
        db::user_memories::list_for_user(&state.db, "alice", 10)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn personal_dot_approval_survives_restart_and_writes_only_its_memory() {
    dot_memory_approval(DotResumeCase::ApproveAfterRestart).await;
}

#[tokio::test]
async fn personal_dot_denial_never_writes_memory() {
    dot_memory_approval(DotResumeCase::Deny).await;
}

#[tokio::test]
async fn personal_dot_resume_rechecks_its_selected_tools() {
    dot_memory_approval(DotResumeCase::SelectionRevoked).await;
}

#[tokio::test]
async fn personal_dot_resume_rechecks_the_users_enabled_tools() {
    dot_memory_approval(DotResumeCase::UserToolDisabled).await;
}

/// Two personal agents made the way the product makes them — created by
/// hand, consents set on their Collaboration tabs — can hand work to each
/// other with nothing else configured: the starter tools give the caller
/// "Ask another agent", which asks its owner once before it delegates.
#[tokio::test]
async fn personal_agents_made_by_hand_delegate_once_both_consents_are_set() {
    use aiplane_agents::db::dot_sessions;
    use aiplane_runtime::agents::router::ForwardRequest;
    use aiplane_tools::memory::Remember;

    let directory = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    let state = boot_as(
        &directory.path().join("hand-made.db"),
        &server.uri(),
        ToolRegistry::new()
            .with(Remember)
            .with(ForwardRequest::personal()),
        &["remember", "forward_request"],
        true,
    )
    .await;
    let cookie = common::seed_session(&state, "alice", "alice@example.com").await;
    let mut ids = Vec::new();
    for name in ["Planner", "Researcher"] {
        let resp = router(state.clone())
            .serve(post(
                "/api/v0/dots".into(),
                &cookie,
                json!({ "name": name }),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        ids.push(
            json_body(resp).await["dot"]["id"]
                .as_str()
                .unwrap()
                .to_owned(),
        );
    }
    let access = json_body(
        router(state.clone())
            .serve(get(format!("/api/v0/dots/{}/access", ids[0]), &cookie))
            .await
            .unwrap(),
    )
    .await;
    let forward = access["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["id"] == "forward_request")
        .unwrap();
    assert_eq!(forward["selected"], true, "{access}");
    assert_eq!(
        forward["permission"], "always_ask",
        "a tool that is not read-only asks first, as in the conversational setup"
    );

    let outbound = router(state.clone())
        .serve(put(
            format!("/api/v0/dots/{}/communication", ids[0]),
            &cookie,
            json!({ "peer_id": ids[1], "direction": "outbound", "enabled": true, "expected_generation": null }),
        ))
        .await
        .unwrap();
    assert_eq!(outbound.status(), StatusCode::OK);
    let generation = json_body(outbound).await["peers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|peer| peer["dot"]["id"] == ids[1].as_str())
        .unwrap()["outgoing"]["generation"]
        .clone();
    let inbound = router(state.clone())
        .serve(put(
            format!("/api/v0/dots/{}/communication", ids[1]),
            &cookie,
            json!({ "peer_id": ids[0], "direction": "inbound", "enabled": true, "expected_generation": generation }),
        ))
        .await
        .unwrap();
    assert_eq!(inbound.status(), StatusCode::OK);

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(Scripted {
            deltas: vec![
                tool_call(
                    "forward_request",
                    json!({"receiver_dot_id": ids[1], "message": "Find the Ceph release date"}),
                ),
                json!({"content": "Ceph Tentacle shipped in 2025."}),
                json!({"content": "The researcher says Ceph Tentacle shipped in 2025."}),
            ],
            served: AtomicUsize::new(0),
            delay: Duration::ZERO,
        })
        .mount(&server)
        .await;
    let session_id = conversation(&state).await;
    assert!(
        dot_sessions::link(&state.db, "alice", &ids[0], &session_id)
            .await
            .unwrap()
    );
    let turn_id = submit(&state, &cookie, &session_id).await;
    wait_for_status(&state, &session_id, &turn_id, TurnStatus::Suspended).await;
    let pause = chat::get_suspension(&state.db, &turn_id)
        .await
        .unwrap()
        .unwrap();
    let (status, body) = resume(
        &state,
        &cookie,
        &session_id,
        &turn_id,
        json!({"decision": "allow_once", "request_id": pause.request_id}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let done = wait_for_status(&state, &session_id, &turn_id, TurnStatus::Completed).await;
    assert_eq!(done.tool_calls.len(), 1);
    assert_eq!(done.tool_calls[0].name, "forward_request");
    assert_eq!(done.tool_calls[0].status, ToolCallStatus::Completed);
    assert!(
        done.tool_calls[0]
            .output_json
            .as_deref()
            .is_some_and(|result| result.contains("Ceph Tentacle shipped in 2025.")),
        "the caller reads the receiver's answer: {:?}",
        done.tool_calls[0].output_json
    );
}

#[tokio::test]
async fn personal_delegation_resumes_from_the_root_after_restart_and_writes_only_receiver_memory() {
    use aiplane_agents::db::{
        agents, dot_delegation, dot_memories, dot_sessions, system_principals,
    };
    use aiplane_runtime::agents::router::ForwardRequest;
    use aiplane_tools::memory::Remember;

    let directory = tempfile::tempdir().unwrap();
    let db_path = directory.path().join("delegation.db");
    let server = MockServer::start().await;
    let mut state = boot_with(
        &db_path,
        &server.uri(),
        ToolRegistry::new()
            .with(Remember)
            .with(ForwardRequest::personal()),
        &["remember", "forward_request"],
    )
    .await;
    let cookie = common::seed_session(&state, "alice", "alice@example.com").await;
    let mut ids = Vec::new();
    for (name, spec) in [
        (
            "Caller",
            r#"{"main":{"tools":["forward_request"],"tool_resources":{"forward_request":{"permission":"always_allow"}}}}"#,
        ),
        (
            "Receiver",
            r#"{"main":{"tools":["remember"],"tool_resources":{"remember":{"permission":"always_ask"}}}}"#,
        ),
    ] {
        let (_, dot) = agents::create_personal_dot(
            &state.db,
            &system_principals::NewPrincipal {
                name,
                display: name,
                description: "",
            },
            spec,
            "alice",
            4,
        )
        .await
        .unwrap();
        ids.push(dot.unwrap().principal.id);
    }
    let saved = dot_delegation::set_consent(
        &state.db,
        dot_delegation::ConsentEdit {
            owner_user_id: "alice",
            dot_id: &ids[0],
            peer_id: &ids[1],
            direction: dot_delegation::ConsentDirection::Outbound,
            enabled: true,
            expected_generation: None,
        },
    )
    .await
    .unwrap();
    let dot_delegation::ConsentSaved::Saved(edge) = saved else {
        panic!("outbound consent saved");
    };
    assert!(matches!(
        dot_delegation::set_consent(
            &state.db,
            dot_delegation::ConsentEdit {
                owner_user_id: "alice",
                dot_id: &ids[1],
                peer_id: &ids[0],
                direction: dot_delegation::ConsentDirection::Inbound,
                enabled: true,
                expected_generation: Some(&edge.generation),
            }
        )
        .await
        .unwrap(),
        dot_delegation::ConsentSaved::Saved(_)
    ));
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(Scripted {
            deltas: vec![
                tool_call(
                    "forward_request",
                    json!({"receiver_dot_id": ids[1], "message": "Save this explicit lesson"}),
                ),
                tool_call(
                    "remember",
                    json!({"content": "Receiver approved lesson", "kind": "fact"}),
                ),
                json!({"content": "Lesson saved."}),
                json!({"content": "Receiver completed the task."}),
            ],
            served: AtomicUsize::new(0),
            delay: Duration::ZERO,
        })
        .mount(&server)
        .await;
    let session_id = conversation(&state).await;
    assert!(
        dot_sessions::link(&state.db, "alice", &ids[0], &session_id)
            .await
            .unwrap()
    );
    let turn_id = submit(&state, &cookie, &session_id).await;
    let paused = wait_for_status(&state, &session_id, &turn_id, TurnStatus::Suspended).await;
    assert!(paused.suspension.is_some());
    let pause = chat::get_suspension(&state.db, &turn_id)
        .await
        .unwrap()
        .unwrap();
    let child_turn = pause.child_turn.as_deref().unwrap().to_owned();
    let Some(aiplane_agents::db::child_sessions::LocalChild::Personal(child)) =
        aiplane_agents::db::child_sessions::child_of_turn(&state.db, &child_turn)
            .await
            .unwrap()
    else {
        panic!("actual delegated personal child");
    };
    let child_session = child.session.session_id;
    assert!(
        dot_memories::list(
            &state.db,
            aiplane_agents::db::dot_memories::MemoryScope::Dot {
                owner: &aiplane_agents::db::agents::DotOwner::User("alice".into()),
                dot_id: &ids[1]
            },
            10
        )
        .await
        .unwrap()
        .unwrap()
        .is_empty()
    );
    state = boot_with(
        &db_path,
        &server.uri(),
        ToolRegistry::new()
            .with(Remember)
            .with(ForwardRequest::personal()),
        &["remember", "forward_request"],
    )
    .await;
    let (status, body) = resume(
        &state,
        &cookie,
        &session_id,
        &turn_id,
        json!({"decision": "allow_once", "request_id": pause.request_id}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let completed = wait_for_status(&state, &session_id, &turn_id, TurnStatus::Completed).await;
    assert_eq!(completed.tool_calls.len(), 1);
    assert_eq!(completed.tool_calls[0].status, ToolCallStatus::Completed);
    assert_eq!(
        chat::get_turn(&state.db, &child_session, &child_turn)
            .await
            .unwrap()
            .unwrap()
            .status,
        TurnStatus::Completed
    );
    let notes = dot_memories::list(
        &state.db,
        aiplane_agents::db::dot_memories::MemoryScope::Dot {
            owner: &aiplane_agents::db::agents::DotOwner::User("alice".into()),
            dot_id: &ids[1],
        },
        10,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].content, "Receiver approved lesson");
    assert!(
        dot_memories::list(
            &state.db,
            aiplane_agents::db::dot_memories::MemoryScope::Dot {
                owner: &aiplane_agents::db::agents::DotOwner::User("alice".into()),
                dot_id: &ids[0]
            },
            10
        )
        .await
        .unwrap()
        .unwrap()
        .is_empty()
    );
    assert!(
        db::user_memories::list_for_user(&state.db, "alice", 10)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 4);
}

pub(crate) fn post(uri: String, cookie: &str, body: Value) -> Request {
    common::post_json(&uri, cookie, &body.to_string())
}

fn put(uri: String, cookie: &str, body: Value) -> Request {
    Request::builder()
        .method(Method::PUT)
        .uri(uri)
        .header("cookie", format!("id={cookie}"))
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn get(uri: String, cookie: &str) -> Request {
    Request::builder()
        .method(Method::GET)
        .uri(uri)
        .header("cookie", format!("id={cookie}"))
        .body(Body::empty())
        .unwrap()
}

pub(crate) async fn json_body(resp: rama::http::Response) -> Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

fn sse_frames(body: &str) -> Vec<(String, Value)> {
    body.split("\n\n")
        .filter_map(|block| {
            let event = block.lines().find_map(|l| l.strip_prefix("event: "))?;
            let data = block.lines().find_map(|l| l.strip_prefix("data: "))?;
            Some((event.to_string(), serde_json::from_str(data).unwrap()))
        })
        .collect()
}

/// A conversation of alice's that already has a title, so the first submit
/// does not spend a scripted upstream round on generating one.
pub(crate) async fn conversation(state: &Arc<RamaState>) -> String {
    let session = chat::create_session(&state.db, "alice").await.unwrap();
    chat::set_session_title(&state.db, &session.id, "approvals")
        .await
        .unwrap();
    session.id
}

/// Submit "echo something" and return the assistant turn id.
pub(crate) async fn submit(state: &Arc<RamaState>, cookie: &str, session_id: &str) -> String {
    let resp = router(state.clone())
        .serve(post(
            format!("/api/v0/chat/sessions/{session_id}/messages"),
            cookie,
            json!({"model": "model-a", "message": "echo something"}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    json_body(resp).await["assistant_turn_id"]
        .as_str()
        .unwrap()
        .to_string()
}

pub(crate) async fn wait_for_status(
    state: &Arc<RamaState>,
    session_id: &str,
    turn_id: &str,
    want: TurnStatus,
) -> chat::TurnWithTools {
    for _ in 0..200 {
        let turn = chat::get_turn_with_tools(&state.db, session_id, turn_id)
            .await
            .unwrap()
            .unwrap();
        if turn.turn.status == want && state.chats.running_for_user("alice") == 0 {
            return turn;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let last = chat::get_turn_with_tools(&state.db, session_id, turn_id).await;
    panic!("turn {turn_id} never reached {want:?}: {last:?}");
}

pub(crate) async fn resume(
    state: &Arc<RamaState>,
    cookie: &str,
    session_id: &str,
    turn_id: &str,
    body: Value,
) -> (StatusCode, Value) {
    let resp = router(state.clone())
        .serve(post(
            format!("/api/v0/chat/sessions/{session_id}/turns/{turn_id}/resume"),
            cookie,
            body,
        ))
        .await
        .unwrap();
    let status = resp.status();
    (status, json_body(resp).await)
}

/// A paused conversation: alice's turn waits for approval of `company_echo`.
async fn paused(
    db_path: &Path,
    upstream_url: &str,
    approval_timeout: Duration,
) -> (Arc<RamaState>, String, String, String) {
    let state = boot(db_path, upstream_url, approval_timeout).await;
    let cookie = common::seed_session(&state, "alice", "alice@example.com").await;
    let session_id = conversation(&state).await;
    let turn_id = submit(&state, &cookie, &session_id).await;
    wait_for_status(&state, &session_id, &turn_id, TurnStatus::Suspended).await;
    (state, cookie, session_id, turn_id)
}

fn tool_message(request: &Value) -> Value {
    request["messages"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .clone()
}

pub(crate) async fn upstream_requests(server: &MockServer) -> Vec<Value> {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| serde_json::from_slice(&r.body).unwrap())
        .collect()
}

#[tokio::test]
async fn the_live_stream_announces_the_pause_and_ends_without_finalizing() {
    let dir = tempfile::tempdir().unwrap();
    let server = upstream("Echoed.", Duration::from_millis(300)).await;
    let state = boot(&dir.path().join("gateway.db"), &server.uri(), AN_HOUR).await;
    let cookie = common::seed_session(&state, "alice", "alice@example.com").await;
    let session_id = conversation(&state).await;
    let turn_id = submit(&state, &cookie, &session_id).await;

    let resp = router(state.clone())
        .serve(get(
            format!("/api/v0/chat/sessions/{session_id}/events"),
            &cookie,
        ))
        .await
        .unwrap();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let frames = sse_frames(&String::from_utf8_lossy(&bytes));
    let names: Vec<&str> = frames.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names.first(), Some(&"snapshot"), "{names:?}");
    let (_, suspended) = frames
        .iter()
        .find(|(n, _)| n == "suspended")
        .unwrap_or_else(|| panic!("no suspended event in {names:?}"));
    assert_eq!(suspended["turn_id"], turn_id);
    assert_eq!(suspended["kind"], "approval");
    assert_eq!(suspended["tool"], "company_echo");
    assert_eq!(suspended["tool_call_id"], "call-1");
    assert_eq!(suspended["options"], json!(["allow_once", "deny"]));
    assert!(suspended["request_id"].is_string());
    assert!(
        !names.contains(&"turn_finalized"),
        "a pause is not an ending: {names:?}"
    );
}

#[tokio::test]
async fn a_paused_turn_survives_a_restart_and_resumes_to_completion() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("gateway.db");
    let server = upstream("Echoed: ship it", Duration::ZERO).await;
    let (before, cookie, session_id, turn_id) = paused(&db_path, &server.uri(), AN_HOUR).await;
    before.db.close().await;
    drop(before);

    let after = boot(&db_path, &server.uri(), AN_HOUR).await;
    // The attach after the restart: the turn is still waiting, and says for
    // what — the startup sweep did not take it for a crashed one.
    let resp = router(after.clone())
        .serve(get(
            format!("/api/v0/chat/sessions/{session_id}/events"),
            &cookie,
        ))
        .await
        .unwrap();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let frames = sse_frames(&String::from_utf8_lossy(&bytes));
    let snapshot = &frames[0].1;
    let waiting = snapshot["turns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["turn"]["id"] == turn_id.as_str())
        .unwrap();
    assert_eq!(waiting["turn"]["status"], "suspended");
    assert_eq!(waiting["tool_calls"][0]["status"], "running");
    let request_id = waiting["suspension"]["request_id"]
        .as_str()
        .unwrap()
        .to_string();

    let (status, body) = resume(
        &after,
        &cookie,
        &session_id,
        &turn_id,
        json!({"decision": "allow_once", "request_id": request_id}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(body["assistant_turn_id"], turn_id.as_str());

    let done = wait_for_status(&after, &session_id, &turn_id, TurnStatus::Completed).await;
    assert_eq!(done.turn.content.as_deref(), Some("Echoed: ship it"));
    assert_eq!(done.suspension, None);
    assert_eq!(done.tool_calls.len(), 1);
    assert_eq!(done.tool_calls[0].status, ToolCallStatus::Completed);
    let sent = upstream_requests(&server).await;
    assert_eq!(sent.len(), 2);
    let result = tool_message(&sent[1]);
    assert_eq!(result["tool_call_id"], "call-1");
    assert!(result["content"].as_str().unwrap().contains("ship it"));
}

#[tokio::test]
async fn a_denied_call_reaches_the_model_as_a_tool_error() {
    let dir = tempfile::tempdir().unwrap();
    let server = upstream("Understood, I will not.", Duration::ZERO).await;
    let (state, cookie, session_id, turn_id) =
        paused(&dir.path().join("gateway.db"), &server.uri(), AN_HOUR).await;

    let (status, body) = resume(
        &state,
        &cookie,
        &session_id,
        &turn_id,
        json!({"decision": "deny"}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");

    let done = wait_for_status(&state, &session_id, &turn_id, TurnStatus::Completed).await;
    assert_eq!(
        done.turn.content.as_deref(),
        Some("Understood, I will not.")
    );
    assert_eq!(done.tool_calls[0].status, ToolCallStatus::Errored);
    let result = tool_message(&upstream_requests(&server).await[1]);
    let content = result["content"].as_str().unwrap();
    assert!(content.contains("declined"), "{content}");
    assert!(!content.contains("ship it"), "the tool must not have run");
}

#[tokio::test]
async fn an_expired_approval_takes_the_deny_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let server = upstream("Nobody approved it.", Duration::ZERO).await;
    let (state, _cookie, session_id, turn_id) = paused(
        &dir.path().join("gateway.db"),
        &server.uri(),
        Duration::ZERO,
    )
    .await;
    let waiting = chat::get_suspension(&state.db, &turn_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(waiting.on_timeout, TimeoutFallback::Deny);

    aiplane_api::pages::chat::resume_expired_suspensions(&state).await;

    let done = wait_for_status(&state, &session_id, &turn_id, TurnStatus::Completed).await;
    assert_eq!(done.turn.content.as_deref(), Some("Nobody approved it."));
    assert_eq!(done.tool_calls[0].status, ToolCallStatus::Errored);
    let result = tool_message(&upstream_requests(&server).await[1]);
    assert!(result["content"].as_str().unwrap().contains("expired"));
}

#[tokio::test]
async fn a_suspension_that_has_not_expired_is_left_waiting() {
    let dir = tempfile::tempdir().unwrap();
    let server = upstream("unused", Duration::ZERO).await;
    let (state, _cookie, session_id, turn_id) =
        paused(&dir.path().join("gateway.db"), &server.uri(), AN_HOUR).await;

    aiplane_api::pages::chat::resume_expired_suspensions(&state).await;

    let turn = chat::get_turn(&state.db, &session_id, &turn_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(turn.status, TurnStatus::Suspended);
    assert_eq!(upstream_requests(&server).await.len(), 1);
}

#[tokio::test]
async fn only_the_owner_answers_only_with_an_offered_decision_and_only_once() {
    let dir = tempfile::tempdir().unwrap();
    let server = upstream("Echoed.", Duration::ZERO).await;
    let (state, cookie, session_id, turn_id) =
        paused(&dir.path().join("gateway.db"), &server.uri(), AN_HOUR).await;
    let bob = common::seed_session(&state, "bob", "bob@example.com").await;

    let (status, _) = resume(
        &state,
        &bob,
        &session_id,
        &turn_id,
        json!({"decision": "allow_once"}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "bob cannot see alice's turn");

    let (status, body) = resume(
        &state,
        &cookie,
        &session_id,
        &turn_id,
        json!({"decision": "value", "value": "yes"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "decision_not_offered", "{body}");

    let (status, _) = resume(
        &state,
        &cookie,
        &session_id,
        &turn_id,
        json!({"decision": "allow_once", "request_id": "an-older-pause"}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(
        chat::get_suspension(&state.db, &turn_id)
            .await
            .unwrap()
            .is_some(),
        "refused answers claim nothing"
    );

    let (status, _) = resume(
        &state,
        &cookie,
        &session_id,
        &turn_id,
        json!({"decision": "allow_once"}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    wait_for_status(&state, &session_id, &turn_id, TurnStatus::Completed).await;
    let (status, body) = resume(
        &state,
        &cookie,
        &session_id,
        &turn_id,
        json!({"decision": "allow_once"}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "not_suspended", "{body}");
}

/// A message into `session_id`: the status and the body.
async fn say(state: &Arc<RamaState>, cookie: &str, session_id: &str) -> (StatusCode, Value) {
    let resp = router(state.clone())
        .serve(post(
            format!("/api/v0/chat/sessions/{session_id}/messages"),
            cookie,
            json!({"model": "model-a", "message": "anything else?"}),
        ))
        .await
        .unwrap();
    let status = resp.status();
    (status, json_body(resp).await)
}

/// A message sent while a call waits for a decision would run before it, in
/// a context the decision was not asked about. It is refused, saying what to
/// do, and nothing is stored or queued.
fn assert_refused_while_paused(status: StatusCode, body: &Value) {
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"]["code"], "decision_pending", "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("decision")),
        "{body}"
    );
}

#[tokio::test]
async fn a_paused_conversation_refuses_new_messages_until_the_decision_is_given() {
    let dir = tempfile::tempdir().unwrap();
    let server = upstream("Done.", Duration::ZERO).await;
    let (state, cookie, session_id, turn_id) =
        paused(&dir.path().join("gateway.db"), &server.uri(), AN_HOUR).await;
    let before = chat::list_turns(&state.db, &session_id)
        .await
        .unwrap()
        .len();

    let (status, body) = say(&state, &cookie, &session_id).await;
    assert_refused_while_paused(status, &body);
    assert_eq!(
        chat::list_turns(&state.db, &session_id)
            .await
            .unwrap()
            .len(),
        before,
        "nothing stored"
    );
    assert!(
        chat::list_pending_for_session(&state.db, &session_id)
            .await
            .unwrap()
            .is_empty(),
        "nothing queued"
    );

    let (status, body) = resume(
        &state,
        &cookie,
        &session_id,
        &turn_id,
        json!({"decision": "deny"}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    wait_for_status(&state, &session_id, &turn_id, TurnStatus::Completed).await;

    let (status, body) = say(&state, &cookie, &session_id).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(body["placement"], "started");
}

#[tokio::test]
async fn cancelling_a_paused_turn_gives_up_on_it_and_frees_the_conversation() {
    let dir = tempfile::tempdir().unwrap();
    let server = upstream("Done.", Duration::ZERO).await;
    let (state, cookie, session_id, turn_id) =
        paused(&dir.path().join("gateway.db"), &server.uri(), AN_HOUR).await;

    let resp = router(state.clone())
        .serve(post(
            format!("/api/v0/chat/sessions/{session_id}/cancel"),
            &cookie,
            json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(json_body(resp).await["cancelled"], true);
    let cancelled = chat::get_turn_with_tools(&state.db, &session_id, &turn_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(cancelled.turn.status, TurnStatus::Cancelled);
    assert_eq!(cancelled.tool_calls[0].status, ToolCallStatus::Errored);

    let (status, body) = say(&state, &cookie, &session_id).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(body["placement"], "started");
}

/// A connector with one shared identity whose one tool its server marks
/// destructive, so the tool defaults to `ask`. Counts the calls that reach it.
async fn crm_connector(state: &RamaState, calls: Arc<AtomicUsize>) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(move |request: &wiremock::Request| {
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            let result = match body["method"].as_str() {
                Some("initialize") => json!({
                    "protocolVersion": "2025-06-18",
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "crm", "version": "1"},
                }),
                Some("tools/list") => json!({
                    "tools": [{"name": "delete_contact", "description": "Delete a contact",
                        "inputSchema": {"type": "object"},
                        "annotations": {"destructiveHint": true, "readOnlyHint": false}}]
                }),
                Some("tools/call") => {
                    calls.fetch_add(1, Ordering::SeqCst);
                    json!({"content": [{"type": "text", "text": "contact 7 deleted"}], "isError": false})
                }
                Some("notifications/initialized") => return ResponseTemplate::new(202),
                other => panic!("unexpected MCP method: {other:?}"),
            };
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/json")
                .set_body_json(json!({"jsonrpc": "2.0", "id": body["id"], "result": result}))
        })
        .mount(&server)
        .await;
    mcp_catalog::create(
        &state.db,
        mcp_catalog::ConnectorInput {
            key: "crm".into(),
            name: "CRM".into(),
            description: None,
            icon: None,
            category: None,
            url: server.uri(),
            auth: mcp_catalog::AuthKind::None,
            scope: mcp_catalog::Scope::Global,
            audit: false,
            use_dcr: false,
            client_id: None,
            client_secret_ct: None,
            client_secret_nonce: None,
            authorize_url: None,
            token_url: None,
            registration_url: None,
            scopes: vec![],
            allowed_groups: vec![],
        },
    )
    .await
    .unwrap();
    mcp_catalog::set_enabled(&state.db, "crm", true)
        .await
        .unwrap();
    server
}

#[tokio::test]
async fn a_connector_tool_in_ask_mode_waits_in_chat_and_runs_only_once_approved() {
    for (decision, runs) in [("allow_once", 1), ("deny", 0)] {
        let dir = tempfile::tempdir().unwrap();
        let server = upstream_calling(
            tool_call("mcp__crm__delete_contact", json!({"id": 7})),
            "Handled.",
            Duration::ZERO,
        )
        .await;
        let state = boot_with(
            &dir.path().join("gateway.db"),
            &server.uri(),
            ToolRegistry::new(),
            &["mcp__crm"],
        )
        .await;
        let calls = Arc::new(AtomicUsize::new(0));
        let _crm = crm_connector(&state, calls.clone()).await;
        let cookie = common::seed_session(&state, "alice", "alice@example.com").await;
        let session_id = conversation(&state).await;
        db::chat_session_tools::set(&state.db, &session_id, "mcp__crm", true, "user")
            .await
            .unwrap();

        let turn_id = submit(&state, &cookie, &session_id).await;
        let paused = wait_for_status(&state, &session_id, &turn_id, TurnStatus::Suspended).await;
        let waiting = paused.suspension.expect("the call waits for alice");
        assert_eq!(waiting.tool.as_deref(), Some("mcp__crm__delete_contact"));
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "nothing ran before the decision"
        );

        let (status, body) = resume(
            &state,
            &cookie,
            &session_id,
            &turn_id,
            json!({"decision": decision, "request_id": waiting.request_id}),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        let done = wait_for_status(&state, &session_id, &turn_id, TurnStatus::Completed).await;
        assert_eq!(done.turn.content.as_deref(), Some("Handled."));
        assert_eq!(calls.load(Ordering::SeqCst), runs, "{decision}");
        let result = tool_message(&upstream_requests(&server).await[1]);
        let content = result["content"].to_string();
        assert_eq!(
            content.contains("contact 7 deleted"),
            runs == 1,
            "{decision}: {content}"
        );
    }
}

#[tokio::test]
async fn scheduling_waits_for_an_approval_that_survives_a_restart() {
    use aiplane_runtime::server::scheduled;
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("gateway.db");
    let server = upstream_calling(
        tool_call(
            "schedule_action",
            json!({"name": "Weekly summary", "prompt": "Summarise last week.", "cron": "0 8 * * 1"}),
        ),
        "Scheduled.",
        Duration::ZERO,
    )
    .await;
    let registry = || ToolRegistry::new().with(aiplane_tools::schedule::ScheduleAction);
    let before = boot_with(&db_path, &server.uri(), registry(), &["schedule_action"]).await;
    let cookie = common::seed_session(&before, "alice", "alice@example.com").await;
    let session_id = conversation(&before).await;
    let turn_id = submit(&before, &cookie, &session_id).await;
    let paused = wait_for_status(&before, &session_id, &turn_id, TurnStatus::Suspended).await;
    let waiting = paused.suspension.expect("the schedule waits for alice");
    assert_eq!(waiting.tool.as_deref(), Some("schedule_action"));
    assert!(
        waiting
            .message
            .as_deref()
            .is_some_and(|m| m.contains("Weekly summary")),
        "{waiting:?}"
    );
    assert!(
        scheduled::list_for_user(&before.db, "alice")
            .await
            .unwrap()
            .is_empty(),
        "nothing is scheduled before the approval"
    );
    before.db.close().await;
    drop(before);

    let after = boot_with(&db_path, &server.uri(), registry(), &["schedule_action"]).await;
    let (status, body) = resume(
        &after,
        &cookie,
        &session_id,
        &turn_id,
        json!({"decision": "allow_once", "request_id": waiting.request_id}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let done = wait_for_status(&after, &session_id, &turn_id, TurnStatus::Completed).await;
    assert_eq!(done.turn.content.as_deref(), Some("Scheduled."));
    let stored = scheduled::list_for_user(&after.db, "alice").await.unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].name, "Weekly summary");
    assert!(!stored[0].tools_enabled);
}

#[tokio::test]
async fn background_resume_busy_leaves_original_approval_unclaimed_for_retry() {
    use aiplane_core::server::db::usage::UsageSource;
    use aiplane_runtime::server::{headless, scheduled};
    let dir = tempfile::tempdir().unwrap();
    let server = upstream("Echoed: ship it", Duration::ZERO).await;
    let state = boot_with(
        &dir.path().join("gateway.db"),
        &server.uri(),
        ToolRegistry::new().with(Echo),
        &["company_echo"],
    )
    .await;
    let cookie = common::seed_session(&state, "alice", "alice@example.com").await;
    let (_, dot) = aiplane_agents::db::agents::create_personal_dot(
        &state.db,
        &aiplane_agents::db::system_principals::NewPrincipal {
            name: "Harald",
            display: "Harald",
            description: "",
        },
        r#"{"main":{"tools":["company_echo"]}}"#,
        "alice",
        4,
    )
    .await
    .unwrap();
    let dot = dot.unwrap().principal.id;
    let (session, turn) = headless::open_session(
        &state.db,
        headless::OpenParams {
            owner: headless::Owner::User("alice"),
            title: "Approval",
            prompt: "echo something",
            model: "model-a",
            existing_session: None,
            dot_id: Some(&dot),
        },
    )
    .await
    .unwrap();
    let action = scheduled::create(
        &state.db,
        scheduled::NewAction {
            owner: aiplane_runtime::server::background::AutomationOwner::User("alice".into()),
            created_by: "alice".into(),
            agent_version: None,
            name: "Approval".into(),
            prompt: "echo something".into(),
            model: Some("model-a".into()),
            cron: "* * * * *".into(),
            timezone: "UTC".into(),
            tools_enabled: true,
            reuse_conversation: true,
            reuse_rounds: 3,
            next_run_at: None,
            dot_id: Some(dot),
            linked_session_id: None,
        },
    )
    .await
    .unwrap();
    let run = scheduled::record_run_start(&state.db, &action.id)
        .await
        .unwrap();
    scheduled::attach_run_turn(&state.db, &run, &session, &turn, true, Some(6))
        .await
        .unwrap();
    let params = headless::DriveParams {
        actor: aiplane_runtime::agent_run::Actor::person("alice", vec![]),
        session_id: session.clone(),
        assistant_turn_id: turn.clone(),
        model: "model-a".into(),
        source: UsageSource::Scheduled,
        history_limit: Some(6),
    };
    headless::drive(&state, params).await;
    let waiting = chat::get_suspension(&state.db, &turn)
        .await
        .unwrap()
        .unwrap();
    let competitor = match state
        .chats
        .register_background("alice", "competing-worker", &session)
    {
        session_core::workers::RegisterOutcome::Registered { worker } => worker,
        _ => panic!("idle session expected"),
    };
    let body = json!({"decision":"allow_once","request_id":waiting.request_id});
    let (status, response) = resume(&state, &cookie, &session, &turn, body.clone()).await;
    assert_eq!(status, StatusCode::CONFLICT, "{response}");
    let still_waiting = chat::get_suspension(&state.db, &turn)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(still_waiting, waiting);
    assert_eq!(
        chat::get_turn(&state.db, &session, &turn)
            .await
            .unwrap()
            .unwrap()
            .status,
        TurnStatus::Suspended
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
    assert!(state.chats.holds("alice", &session, "competing-worker"));
    state.chats.clear("alice", &competitor);
    let (status, response) = resume(&state, &cookie, &session, &turn, body).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{response}");
    let done = wait_for_status(&state, &session, &turn, TurnStatus::Completed).await;
    assert!(done.suspension.is_none());
    assert!(
        done.tool_calls[0]
            .output_json
            .as_deref()
            .unwrap()
            .contains("ship it")
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}
