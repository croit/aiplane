// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Cross-organization delegation, dispatch, result and durable waiting
//! (`docs/agents-cross-organization.md`): a personal agent's `forward_request`
//! addressing an organization agent — the receiver runs as its own principal on
//! its published spec, the caller reads only the bounded answer, a child pause
//! suspends the caller on the same request, and the child's pause settles in
//! the receiver organization's inbox while the caller's mirror only settles
//! with the child's bounded answer.

use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::time::Duration;

use crate::common::Service as _;
use aiplane::rama_server::{RamaState, router::router};
use aiplane_agents::db::{
    agent_delegation, agent_sessions, agents, child_sessions, system_principals::NewPrincipal,
};
use aiplane_runtime::agents::router::ForwardRequest;
use aiplane_runtime::server::tools::ToolRegistry;
use aiplane_tools::memory::Remember;
use rama::http::StatusCode;
use serde_json::Value;
use session_core::db::{self as chat, ToolCallStatus, TurnStatus};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer};

use super::agents::Fx;
use super::turn_suspension::{Scripted, conversation, json_body, post, tool_call, wait_for_status};

async fn fixture_with_forward(upstream: &MockServer) -> Fx {
    let uri = upstream.uri();
    super::agents::fixture_with(
        Some(&uri),
        ToolRegistry::new()
            .with(Remember)
            .with(ForwardRequest::personal()),
        &["remember", "forward_request"],
    )
    .await
}

async fn caller_agent(state: &RamaState, name: &str, spec: &str) -> String {
    agents::create_personal_agent(
        &state.db,
        &NewPrincipal {
            name,
            display: name,
            description: "",
        },
        spec,
        "alice",
        4,
    )
    .await
    .unwrap()
    .1
    .unwrap()
    .principal
    .id
}

async fn organization(fx: &Fx, name: &str, groups: &[&str]) -> Value {
    let (status, created) = fx
        .post(
            &fx.root,
            "/api/v0/admin/organizations",
            serde_json::json!({"name":name,"description":"Actual support organization","member_groups":groups}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    created
}

fn created_id(row: &Value) -> String {
    row["id"].as_str().unwrap().to_string()
}

async fn org_agent(fx: &Fx, org: &Value, name: &str) -> String {
    let (status, created) = fx
        .post(
            &fx.alice,
            &format!("/api/v0/organizations/{}/agents", created_id(&org["organization"])),
            serde_json::json!({"name":name,"display":name,"description":"Actual organization assistant"}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    created_id(&created["agent"])
}

async fn publish_org_agent(fx: &Fx, owned_agent: &str, spec: Value, tools: &[&str]) {
    assert_eq!(
        fx.grant(&fx.alice, owned_agent, "model", "m").await,
        StatusCode::CREATED
    );
    for tool in tools {
        assert_eq!(
            fx.grant(&fx.alice, owned_agent, "tool", tool).await,
            StatusCode::CREATED
        );
    }
    let (status, saved) = fx.put_draft(&fx.alice, owned_agent, spec).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    let (status, published) = fx.publish(&fx.alice, owned_agent).await;
    assert_eq!(status, StatusCode::CREATED, "{published}");
}

/// One consent decision of a direction, on a fresh edge.
async fn set_direction(
    fx: &Fx,
    caller: &str,
    receiver: &str,
    direction: agent_delegation::ConsentDirection,
) {
    let edit = agent_delegation::CrossBoundaryConsentEdit {
        actor_user_id: "alice",
        caller_agent_id: caller,
        receiver_agent_id: receiver,
        direction,
        enabled: true,
        expected_generation: None,
    };
    assert!(
        matches!(
            agent_delegation::set_cross_boundary_consent(&fx.state.db, edit)
                .await
                .unwrap(),
            agent_delegation::ConsentSaved::Saved(_)
        ),
        "{direction:?} consent saved"
    );
}

/// Alice loses every group: membership is rechecked live at dispatch.
async fn remove_alice_from_groups(state: &RamaState) {
    let now = jiff::Timestamp::now();
    aiplane_core::server::db::users::upsert(
        &state.db,
        &aiplane_core::server::db::users::User {
            id: "alice".into(),
            email: "alice@example.com".into(),
            name: None,
            roles: vec![],
            created_at: now,
            updated_at: now,
            timezone: None,
            speech_voice: None,
        },
    )
    .await
    .unwrap();
    state.reload_rbac().await;
}

/// Both consent decisions of one direction, chained on the edge generation.
/// Returns the edge the second decision saved.
async fn consent(fx: &Fx, caller: &str, receiver: &str) -> agent_delegation::AgentDelegationEdge {
    let edit = |direction| agent_delegation::CrossBoundaryConsentEdit {
        actor_user_id: "alice",
        caller_agent_id: caller,
        receiver_agent_id: receiver,
        direction,
        enabled: true,
        expected_generation: None,
    };
    let saved = agent_delegation::set_cross_boundary_consent(
        &fx.state.db,
        edit(agent_delegation::ConsentDirection::Outbound),
    )
    .await
    .unwrap();
    let agent_delegation::ConsentSaved::Saved(edge) = saved else {
        panic!("outbound consent saved");
    };
    let mut inbound = edit(agent_delegation::ConsentDirection::Inbound);
    inbound.expected_generation = Some(&edge.generation);
    let saved = agent_delegation::set_cross_boundary_consent(&fx.state.db, inbound)
        .await
        .unwrap();
    let agent_delegation::ConsentSaved::Saved(edge) = saved else {
        panic!("inbound consent saved");
    };
    edge
}

/// Alice's conversation, submitted against the fixture's `m` model.
async fn submit(state: &Arc<RamaState>, cookie: &str, session_id: &str) -> String {
    let resp = router(state.clone())
        .serve(post(
            format!("/api/v0/chat/sessions/{session_id}/messages"),
            cookie,
            serde_json::json!({"model": "m", "message": "Open a ticket for the broken printer"}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    json_body(resp).await["assistant_turn_id"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn cross_boundary_dispatch_runs_the_organization_agent_with_its_own_authority_and_returns_its_answer()
 {
    let upstream = MockServer::start().await;
    let fx = fixture_with_forward(&upstream).await;
    let state = Arc::new(fx.state.clone());
    let caller = caller_agent(
        &fx.state,
        "Caller",
        r#"{"main":{"tools":["forward_request"],"tool_resources":{"forward_request":{"permission":"always_allow"}}}}"#,
    )
    .await;
    let org = organization(&fx, "Ticket Support", &["managers"]).await;
    let receiver = org_agent(&fx, &org, "Tickets").await;
    publish_org_agent(
        &fx,
        &receiver,
        serde_json::json!({
            "main": {
                "model": "m",
                "instructions": {"orchestration": "Answer the request directly."}
            },
            "finish": {"schema": {"type": "object", "required": ["answer"], "properties": {"answer": {"type": "string"}}}}
        }),
        &[],
    )
    .await;
    consent(&fx, &caller, &receiver).await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(Scripted {
            deltas: vec![
                tool_call(
                    "forward_request",
                    serde_json::json!({"receiver_agent_id": receiver, "message": "Open a ticket for the broken printer"}),
                ),
                tool_call(
                    "finish",
                    serde_json::json!({"result": {"answer": "Ticket 42 opened."}}),
                ),
                serde_json::json!({"content": "The ticket agent opened the ticket."}),
            ],
            served: AtomicUsize::new(0),
            delay: Duration::ZERO,
        })
        .mount(&upstream)
        .await;
    let session_id = conversation(&state).await;
    assert!(
        agent_sessions::link(&fx.state.db, "alice", &caller, &session_id)
            .await
            .unwrap()
    );
    let turn_id = submit(&state, &fx.alice, &session_id).await;
    let completed = wait_for_status(&state, &session_id, &turn_id, TurnStatus::Completed).await;

    let call = completed
        .tool_calls
        .iter()
        .find(|call| call.name == "forward_request")
        .unwrap();
    assert_eq!(call.status, ToolCallStatus::Completed);
    let result: Value = serde_json::from_str(call.output_json.as_deref().unwrap()).unwrap();
    assert_eq!(result["forwarded"], serde_json::json!(true));
    assert_eq!(result["receiver_agent_id"], serde_json::json!(receiver));

    let child_session: String = sqlx::query_scalar(
        "SELECT child_session_id FROM cross_boundary_delegations WHERE parent_turn_id = ?",
    )
    .bind(&turn_id)
    .fetch_one(&fx.state.db)
    .await
    .unwrap();
    let (user_id, principal_id): (Option<String>, Option<String>) =
        sqlx::query_as("SELECT user_id, principal_id FROM chat_sessions WHERE id = ?")
            .bind(&child_session)
            .fetch_one(&fx.state.db)
            .await
            .unwrap();
    assert!(user_id.is_none());
    assert_eq!(principal_id.as_deref(), Some(receiver.as_str()));
    let child_turn: String =
        sqlx::query_scalar("SELECT id FROM chat_turns WHERE session_id = ? AND role = 'assistant'")
            .bind(&child_session)
            .fetch_one(&fx.state.db)
            .await
            .unwrap();
    let outcome = &result["outcome"];
    assert_eq!(outcome["status"], serde_json::json!("finished"), "{result}");
    assert_eq!(
        outcome["result"]["answer"],
        serde_json::json!("Ticket 42 opened.")
    );
    assert!(
        matches!(
            child_sessions::child_of_turn(&fx.state.db, &child_turn)
                .await
                .unwrap(),
            Some(child_sessions::LocalChild::Principal(_))
        ),
        "the child is the organization principal's session"
    );
    assert_eq!(upstream_requests(&upstream).await, 3);
}

#[tokio::test]
async fn cross_boundary_dispatch_requires_the_full_consent_pair_and_membership() {
    for case in [
        RefusalCase::NoEdge,
        RefusalCase::OutboundOnly,
        RefusalCase::InboundOnly,
        RefusalCase::CallerNotAMember,
    ] {
        cross_boundary_dispatch_refuses(case).await;
    }
}

#[derive(Clone, Copy)]
enum RefusalCase {
    NoEdge,
    OutboundOnly,
    InboundOnly,
    CallerNotAMember,
}

async fn cross_boundary_dispatch_refuses(case: RefusalCase) {
    let upstream = MockServer::start().await;
    let fx = fixture_with_forward(&upstream).await;
    let state = Arc::new(fx.state.clone());
    let caller = caller_agent(
        &fx.state,
        "Caller",
        r#"{"main":{"tools":["forward_request"],"tool_resources":{"forward_request":{"permission":"always_allow"}}}}"#,
    )
    .await;
    let org = organization(&fx, "Ticket Support", &["managers"]).await;
    let receiver = org_agent(&fx, &org, "Tickets").await;
    publish_org_agent(
        &fx,
        &receiver,
        serde_json::json!({"main":{"model":"m","instructions":{"orchestration":"Answer the request directly."}}}),
        &[],
    )
    .await;
    match case {
        RefusalCase::NoEdge => {}
        RefusalCase::OutboundOnly => {
            set_direction(
                &fx,
                &caller,
                &receiver,
                agent_delegation::ConsentDirection::Outbound,
            )
            .await
        }
        RefusalCase::InboundOnly => {
            set_direction(
                &fx,
                &caller,
                &receiver,
                agent_delegation::ConsentDirection::Inbound,
            )
            .await
        }
        RefusalCase::CallerNotAMember => {
            consent(&fx, &caller, &receiver).await;
            remove_alice_from_groups(&fx.state).await;
        }
    }
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(Scripted {
            deltas: vec![
                tool_call(
                    "forward_request",
                    serde_json::json!({"receiver_agent_id": receiver, "message": "Open a ticket"}),
                ),
                serde_json::json!({"content": "The receiver was unavailable."}),
            ],
            served: AtomicUsize::new(0),
            delay: Duration::ZERO,
        })
        .mount(&upstream)
        .await;
    let session_id = conversation(&state).await;
    assert!(
        agent_sessions::link(&fx.state.db, "alice", &caller, &session_id)
            .await
            .unwrap()
    );
    let turn_id = submit(&state, &fx.alice, &session_id).await;
    let completed = wait_for_status(&state, &session_id, &turn_id, TurnStatus::Completed).await;

    let call = completed
        .tool_calls
        .iter()
        .find(|call| call.name == "forward_request")
        .unwrap();
    assert_eq!(call.status, ToolCallStatus::Errored);
    let links: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cross_boundary_delegations")
        .fetch_one(&fx.state.db)
        .await
        .unwrap();
    assert_eq!(links, 0);
    let child_sessions: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM chat_sessions WHERE principal_id = ?")
            .bind(&receiver)
            .fetch_one(&fx.state.db)
            .await
            .unwrap();
    assert_eq!(child_sessions, 0);
    assert_eq!(upstream_requests(&upstream).await, 2);
}

#[tokio::test]
async fn cross_boundary_dispatch_pause_propagates_to_the_caller() {
    let upstream = MockServer::start().await;
    let fx = fixture_with_forward(&upstream).await;
    let state = Arc::new(fx.state.clone());
    let caller = caller_agent(
        &fx.state,
        "Caller",
        r#"{"main":{"tools":["forward_request"],"tool_resources":{"forward_request":{"permission":"always_allow"}}}}"#,
    )
    .await;
    let org = organization(&fx, "Ticket Support", &["managers"]).await;
    let receiver = org_agent(&fx, &org, "Tickets").await;
    publish_org_agent(
        &fx,
        &receiver,
        serde_json::json!({
            "main": {
                "model": "m",
                "instructions": {"orchestration": "Save the lesson."},
                "tools": ["remember"],
                "tool_resources": {"remember": {"permission": "always_ask"}}
            },
            "finish": {"schema": {"type": "object", "required": ["answer"], "properties": {"answer": {"type": "string"}}}}
        }),
        &["remember"],
    )
    .await;
    consent(&fx, &caller, &receiver).await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(Scripted {
            deltas: vec![
                tool_call(
                    "forward_request",
                    serde_json::json!({"receiver_agent_id": receiver, "message": "Save this explicit lesson"}),
                ),
                tool_call(
                    "remember",
                    serde_json::json!({"content": "Org approved lesson", "kind": "fact"}),
                ),
            ],
            served: AtomicUsize::new(0),
            delay: Duration::ZERO,
        })
        .mount(&upstream)
        .await;
    let session_id = conversation(&state).await;
    assert!(
        agent_sessions::link(&fx.state.db, "alice", &caller, &session_id)
            .await
            .unwrap()
    );
    let turn_id = submit(&state, &fx.alice, &session_id).await;
    let paused = wait_for_status(&state, &session_id, &turn_id, TurnStatus::Suspended).await;
    assert!(paused.suspension.is_some());
    let pause = chat::get_suspension(&fx.state.db, &turn_id)
        .await
        .unwrap()
        .unwrap();
    let child_turn = pause.child_turn.as_deref().unwrap().to_owned();

    let child_session: String = sqlx::query_scalar(
        "SELECT child_session_id FROM cross_boundary_delegations WHERE parent_turn_id = ?",
    )
    .bind(&turn_id)
    .fetch_one(&fx.state.db)
    .await
    .unwrap();
    let (user_id, principal_id): (Option<String>, Option<String>) =
        sqlx::query_as("SELECT user_id, principal_id FROM chat_sessions WHERE id = ?")
            .bind(&child_session)
            .fetch_one(&fx.state.db)
            .await
            .unwrap();
    assert!(user_id.is_none());
    assert_eq!(principal_id.as_deref(), Some(receiver.as_str()));
    assert!(
        matches!(
            child_sessions::child_of_turn(&fx.state.db, &child_turn)
                .await
                .unwrap(),
            Some(child_sessions::LocalChild::Principal(_))
        ),
        "the waiting child is the organization principal's session"
    );
    let child_pause = chat::get_suspension(&fx.state.db, &child_turn)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        child_pause.kind,
        session_core::db::SuspensionKind::Approval,
        "the child waits on its own remember approval"
    );
    assert_eq!(pause.child_turn.as_deref(), Some(child_turn.as_str()));
    assert_eq!(upstream_requests(&upstream).await, 2);
}

async fn upstream_requests(server: &MockServer) -> usize {
    server.received_requests().await.unwrap().len()
}

// ---------------------------------------------------------------------------
// Durable waiting (`docs/agents-cross-organization.md` → "Child linkage,
// resume, cancellation"): the child's pause settles in the receiver
// organization's inbox, the caller's mirror only settles with the child's
// bounded answer, and both sides' cancellations and the expiry sweeps keep
// the pair consistent.

/// The shared setup: alice's caller agent delegating to an organization agent
/// whose `remember` asks, both consent decisions saved, and the pair waiting
/// on the child's approval. `tail` scripts the upstream requests after the
/// pause; the dispatch and the pause are scripted here.
struct PausedDelegation {
    fx: Fx,
    state: Arc<RamaState>,
    org: Value,
    caller: String,
    receiver: String,
    edge: agent_delegation::AgentDelegationEdge,
    session_id: String,
    turn_id: String,
    child_session: String,
    child_turn: String,
}

async fn paused_delegation(tail: Vec<Value>) -> PausedDelegation {
    let upstream = MockServer::start().await;
    let mut fx = fixture_with_forward(&upstream).await;
    fx.state = fx.state.clone().with_agent_runner(std::sync::Arc::new(
        aiplane_runtime::agents::embed::LiveAgentRunner,
    ));
    let state = Arc::new(fx.state.clone());
    let caller = caller_agent(
        &fx.state,
        "Caller",
        r#"{"main":{"tools":["forward_request"],"tool_resources":{"forward_request":{"permission":"always_allow"}}}}"#,
    )
    .await;
    let org = organization(&fx, "Ticket Support", &["managers"]).await;
    let receiver = org_agent(&fx, &org, "Tickets").await;
    publish_org_agent(
        &fx,
        &receiver,
        serde_json::json!({
            "main": {
                "model": "m",
                "instructions": {"orchestration": "Save the lesson."},
                "tools": ["remember"],
                "tool_resources": {"remember": {"permission": "always_ask"}}
            },
            "finish": {"schema": {"type": "object", "required": ["answer"], "properties": {"answer": {"type": "string"}}}}
        }),
        &["remember"],
    )
    .await;
    let edge = consent(&fx, &caller, &receiver).await;
    let deltas = vec![
        tool_call(
            "forward_request",
            serde_json::json!({"receiver_agent_id": receiver, "message": "Save this explicit lesson"}),
        ),
        tool_call(
            "remember",
            serde_json::json!({"content": "Org approved lesson", "kind": "fact"}),
        ),
    ]
    .into_iter()
    .chain(tail)
    .collect();
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(Scripted {
            deltas,
            served: AtomicUsize::new(0),
            delay: Duration::ZERO,
        })
        .mount(&upstream)
        .await;
    let session_id = conversation(&state).await;
    assert!(
        agent_sessions::link(&fx.state.db, "alice", &caller, &session_id)
            .await
            .unwrap()
    );
    let turn_id = submit(&state, &fx.alice, &session_id).await;
    let paused = wait_for_status(&state, &session_id, &turn_id, TurnStatus::Suspended).await;
    assert!(paused.suspension.is_some());
    let pause = chat::get_suspension(&fx.state.db, &turn_id)
        .await
        .unwrap()
        .unwrap();
    let child_turn = pause.child_turn.as_deref().unwrap().to_owned();
    let child_session: String = sqlx::query_scalar(
        "SELECT child_session_id FROM cross_boundary_delegations WHERE parent_turn_id = ?",
    )
    .bind(&turn_id)
    .fetch_one(&fx.state.db)
    .await
    .unwrap();
    PausedDelegation {
        fx,
        state,
        org,
        caller,
        receiver,
        edge,
        session_id,
        turn_id,
        child_session,
        child_turn,
    }
}

/// The child's pause, as bob (a member holding a write share) sees it.
async fn child_inbox_item(p: &PausedDelegation) -> (Value, String) {
    assert_eq!(
        p.fx.share(&p.fx.alice, &p.receiver, "user", "bob", "write")
            .await
            .0,
        StatusCode::CREATED
    );
    let (status, inbox) = p.fx.get(&p.fx.bob, "/api/v0/agents/inbox").await;
    assert_eq!(status, StatusCode::OK, "{inbox}");
    let item = inbox["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["turn_id"] == p.child_turn)
        .unwrap_or_else(|| panic!("the child's pause is in bob's inbox: {inbox}"));
    assert_eq!(item["call"]["name"], "remember");
    let request_id = item["id"].as_str().unwrap().to_string();
    (inbox, request_id)
}

async fn answer_inbox(
    p: &PausedDelegation,
    request_id: &str,
    decision: &str,
) -> (StatusCode, Value) {
    p.fx.post(
        &p.fx.bob,
        &format!("/api/v0/agents/inbox/{request_id}/answer"),
        serde_json::json!({"decision": decision}),
    )
    .await
}

async fn settled_forward_call(p: &PausedDelegation) -> Value {
    let completed =
        wait_for_status(&p.state, &p.session_id, &p.turn_id, TurnStatus::Completed).await;
    let call = completed
        .tool_calls
        .iter()
        .find(|call| call.name == "forward_request")
        .unwrap();
    assert_eq!(call.status, ToolCallStatus::Completed);
    let result: Value = serde_json::from_str(call.output_json.as_deref().unwrap()).unwrap();
    assert_eq!(result["forwarded"], serde_json::json!(true));
    assert_eq!(result["receiver_agent_id"], serde_json::json!(p.receiver));
    result
}

/// Move a suspension's deadline into the past, so the sweeps pick it up.
async fn expire_suspension(p: &PausedDelegation, turn_id: &str) {
    sqlx::query("UPDATE chat_turn_suspensions SET expires_at = ? WHERE turn_id = ?")
        .bind((jiff::Timestamp::now() - jiff::SignedDuration::from_secs(1)).to_string())
        .bind(turn_id)
        .execute(&p.state.db)
        .await
        .unwrap();
}

#[tokio::test]
async fn cross_boundary_member_approval_settles_the_caller_with_the_bounded_answer() {
    let p = paused_delegation(vec![
        tool_call(
            "finish",
            serde_json::json!({"result": {"answer": "The lesson is saved."}}),
        ),
        serde_json::json!({"content": "The org agent saved the lesson."}),
    ])
    .await;
    let p = &p;
    let (_, request_id) = child_inbox_item(p).await;
    // The caller's mirror is not in her inbox: the decision belongs to the
    // organization, and she only ever sees her own tool call.
    let (status, personal) = p.fx.get(&p.fx.alice, "/api/v0/agents/inbox").await;
    assert_eq!(status, StatusCode::OK, "{personal}");
    assert!(
        personal["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["turn_id"] != p.turn_id),
        "the mirror stays out of the caller's inbox: {personal}"
    );
    let (status, body) = answer_inbox(p, &request_id, "allow_once").await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(
        wait_for_status(
            &p.state,
            &p.child_session,
            &p.child_turn,
            TurnStatus::Completed
        )
        .await
        .turn
        .status,
        TurnStatus::Completed
    );
    let result = settled_forward_call(p).await;
    let outcome = &result["outcome"];
    assert_eq!(outcome["status"], serde_json::json!("finished"), "{result}");
    assert_eq!(
        outcome["result"]["answer"],
        serde_json::json!("The lesson is saved.")
    );
    let (status, inbox) = p.fx.get(&p.fx.bob, "/api/v0/agents/inbox").await;
    assert_eq!(status, StatusCode::OK, "{inbox}");
    assert!(
        inbox["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["turn_id"] != p.child_turn),
        "the answered pause drops from bob's inbox: {inbox}"
    );
}

#[tokio::test]
async fn cross_boundary_member_denial_settles_the_caller_and_writes_nothing() {
    let p = paused_delegation(vec![
        tool_call(
            "finish",
            serde_json::json!({"result": {"answer": "The save was declined; nothing was stored."}}),
        ),
        serde_json::json!({"content": "The org agent declined the save."}),
    ])
    .await;
    let p = &p;
    let (_, request_id) = child_inbox_item(p).await;
    let (status, body) = answer_inbox(p, &request_id, "deny").await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let result = settled_forward_call(p).await;
    assert_eq!(
        result["outcome"]["status"],
        serde_json::json!("finished"),
        "{result}"
    );
    let notes = aiplane_agents::db::agent_memories::list(
        &p.state.db,
        aiplane_agents::db::agent_memories::MemoryScope::Agent {
            owner: &aiplane_agents::db::agents::AgentOwner::Organization(created_id(
                &p.org["organization"],
            )),
            agent_id: &p.receiver,
        },
        10,
    )
    .await
    .unwrap()
    .unwrap();
    assert!(notes.is_empty(), "{notes:?}");
}

#[tokio::test]
async fn cross_boundary_child_pause_expires_and_settles_the_caller() {
    let p = paused_delegation(vec![
        tool_call(
            "finish",
            serde_json::json!({"result": {"answer": "The save expired."}}),
        ),
        serde_json::json!({"content": "The org agent expired the save."}),
    ])
    .await;
    let p = &p;
    expire_suspension(p, &p.child_turn).await;
    aiplane_runtime::agents::resume::resume_expired(&p.state).await;
    let result = settled_forward_call(p).await;
    assert_eq!(
        result["outcome"]["status"],
        serde_json::json!("finished"),
        "{result}"
    );
    assert_eq!(
        result["outcome"]["result"]["answer"],
        serde_json::json!("The save expired.")
    );
}

#[tokio::test]
async fn cross_boundary_member_cancelling_the_child_settles_the_caller_with_cancellation() {
    let p = paused_delegation(vec![
        serde_json::json!({"content": "The org run was cancelled."}),
    ])
    .await;
    let p = &p;
    child_inbox_item(p).await;
    let (status, body) =
        p.fx.post(
            &p.fx.bob,
            &format!(
                "/api/v0/agents/{}/test/{}/cancel",
                p.receiver, p.child_session
            ),
            serde_json::json!({"turn_id": p.child_turn}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        chat::get_turn(&p.state.db, &p.child_session, &p.child_turn)
            .await
            .unwrap()
            .unwrap()
            .status,
        TurnStatus::Cancelled
    );
    let result = settled_forward_call(p).await;
    assert_eq!(
        result["outcome"]["status"],
        serde_json::json!("incomplete"),
        "{result}"
    );
    assert_eq!(
        result["outcome"]["reason"]["kind"],
        serde_json::json!("cancelled")
    );
}

#[tokio::test]
async fn cross_boundary_caller_cancelling_the_root_cancels_the_organization_child() {
    let p = paused_delegation(vec![]).await;
    let p = &p;
    let (status, body) =
        p.fx.post(
            &p.fx.alice,
            &format!("/api/v0/chat/sessions/{}/cancel", p.session_id),
            serde_json::json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["cancelled"], serde_json::json!(true));
    assert_eq!(
        chat::get_turn(&p.state.db, &p.session_id, &p.turn_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        TurnStatus::Cancelled
    );
    assert_eq!(
        chat::get_turn(&p.state.db, &p.child_session, &p.child_turn)
            .await
            .unwrap()
            .unwrap()
            .status,
        TurnStatus::Cancelled
    );
}

#[tokio::test]
async fn cross_boundary_revoked_consent_delivers_nothing_while_waiting() {
    let p = paused_delegation(vec![
        tool_call(
            "finish",
            serde_json::json!({"result": {"answer": "The lesson is saved."}}),
        ),
        serde_json::json!({"content": "The org agent saved the lesson."}),
    ])
    .await;
    let p = &p;
    let (_, request_id) = child_inbox_item(p).await;
    let edge = agent_delegation::set_cross_boundary_consent(
        &p.state.db,
        agent_delegation::CrossBoundaryConsentEdit {
            actor_user_id: "alice",
            caller_agent_id: &p.caller,
            receiver_agent_id: &p.receiver,
            direction: agent_delegation::ConsentDirection::Inbound,
            enabled: false,
            expected_generation: Some(&p.edge.generation),
        },
    )
    .await
    .unwrap();
    let agent_delegation::ConsentSaved::Saved(_) = edge else {
        panic!("the inbound consent was revoked");
    };
    // The member's answer no longer matches the consent under which the data
    // crossed the boundary, so the child cannot resume.
    let (status, body) = answer_inbox(p, &request_id, "allow_once").await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"]["code"], "not_suspended");
    assert_eq!(
        chat::get_turn(&p.state.db, &p.child_session, &p.child_turn)
            .await
            .unwrap()
            .unwrap()
            .status,
        TurnStatus::Suspended
    );
    assert_eq!(
        chat::get_turn(&p.state.db, &p.session_id, &p.turn_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        TurnStatus::Suspended
    );
    // The revocation cancels the pair; nothing is delivered to the caller.
    aiplane_runtime::agents::resume::cancel_personal_tree(&p.state, "alice", &p.turn_id)
        .await
        .unwrap();
    assert_eq!(
        chat::get_turn(&p.state.db, &p.session_id, &p.turn_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        TurnStatus::Cancelled
    );
    assert_eq!(
        chat::get_turn(&p.state.db, &p.child_session, &p.child_turn)
            .await
            .unwrap()
            .unwrap()
            .status,
        TurnStatus::Cancelled
    );
}

#[tokio::test]
async fn cross_boundary_waiting_mirror_refuses_the_personal_path_until_the_child_settles() {
    let p = paused_delegation(vec![
        tool_call(
            "finish",
            serde_json::json!({"result": {"answer": "The save expired."}}),
        ),
        serde_json::json!({"content": "The org agent expired the save."}),
    ])
    .await;
    let p = &p;
    expire_suspension(p, &p.turn_id).await;
    // The personal sweep refuses while the child still waits: the decision
    // belongs to the organization, so the mirror stays suspended.
    aiplane_api::pages::chat::resume_expired_suspensions(&p.state).await;
    assert_eq!(
        chat::get_turn(&p.state.db, &p.session_id, &p.turn_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        TurnStatus::Suspended
    );
    expire_suspension(p, &p.child_turn).await;
    aiplane_runtime::agents::resume::resume_expired(&p.state).await;
    let result = settled_forward_call(p).await;
    assert_eq!(
        result["outcome"]["status"],
        serde_json::json!("finished"),
        "{result}"
    );
}

fn peer_in<'a>(view: &'a Value, agent_id: &str) -> &'a Value {
    view["peers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|peer| peer["agent"]["id"] == agent_id)
        .unwrap_or_else(|| panic!("peer {agent_id} in the communication view: {view}"))
}

#[tokio::test]
async fn personal_communication_view_lists_member_organization_agents_and_both_sides_save_their_side()
 {
    let upstream = MockServer::start().await;
    let fx = fixture_with_forward(&upstream).await;
    let caller = caller_agent(
        &fx.state,
        "Caller",
        r#"{"main":{"tools":["forward_request"],"tool_resources":{"forward_request":{"permission":"always_allow"}}}}"#,
    )
    .await;
    let org = organization(&fx, "Ticket Support", &["managers"]).await;
    let receiver = org_agent(&fx, &org, "Tickets").await;

    // The member's organization agent is listed, marked cross-organization,
    // with no consent yet.
    let (status, view) = fx
        .get(&fx.alice, &format!("/api/v0/agents/{caller}/communication"))
        .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    let peer = peer_in(&view, &receiver);
    assert_eq!(peer["cross_organization"], serde_json::json!(true));
    assert_eq!(
        peer["outgoing"]["outbound_enabled"],
        serde_json::json!(false)
    );
    assert_eq!(
        peer["incoming"]["inbound_enabled"],
        serde_json::json!(false)
    );

    // The personal owner enables only their side: outbound.
    let (status, view) = fx
        .put(
            &fx.alice,
            &format!("/api/v0/agents/{caller}/communication"),
            serde_json::json!({
                "peer_id": receiver,
                "direction": "outbound",
                "enabled": true,
                "expected_generation": null
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    let peer = peer_in(&view, &receiver);
    assert_eq!(
        peer["outgoing"]["outbound_enabled"],
        serde_json::json!(true)
    );
    assert_eq!(
        peer["incoming"]["inbound_enabled"],
        serde_json::json!(false)
    );
    let generation = peer["outgoing"]["generation"].as_str().unwrap().to_string();

    // The personal side may not set the organization's side.
    let (status, body) = fx
        .put(
            &fx.alice,
            &format!("/api/v0/agents/{caller}/communication"),
            serde_json::json!({
                "peer_id": receiver,
                "direction": "inbound",
                "enabled": true,
                "expected_generation": &generation
            }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"]["code"], "invalid_consent");

    // The organization member writer enables inbound.
    let (status, view) = fx
        .put(
            &fx.alice,
            &format!("/api/v0/agents/{receiver}/communication"),
            serde_json::json!({
                "peer_id": caller,
                "enabled": true,
                "expected_generation": &generation
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["callers"].as_array().unwrap().len(), 1);
    let grant = &view["callers"][0];
    assert_eq!(grant["agent"]["id"], serde_json::json!(caller));
    assert_eq!(grant["agent"]["name"], serde_json::json!("Caller"));
    assert_eq!(grant["owner"]["id"], serde_json::json!("alice"));
    assert_eq!(
        grant["incoming"]["inbound_enabled"],
        serde_json::json!(true)
    );

    // The personal view now shows both sides enabled.
    let (status, view) = fx
        .get(&fx.alice, &format!("/api/v0/agents/{caller}/communication"))
        .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    let peer = peer_in(&view, &receiver);
    assert_eq!(
        peer["outgoing"]["outbound_enabled"],
        serde_json::json!(true)
    );
    assert_eq!(peer["incoming"]["inbound_enabled"], serde_json::json!(true));
}

#[tokio::test]
async fn organization_communication_requires_member_write_and_rejects_out_of_scope_agents() {
    let upstream = MockServer::start().await;
    let fx = fixture_with_forward(&upstream).await;
    let caller = caller_agent(&fx.state, "Caller", r#"{"main":{}}"#).await;
    let org = organization(&fx, "Ticket Support", &["managers"]).await;
    let receiver = org_agent(&fx, &org, "Tickets").await;

    // A member without a share sees nothing on the organization surface.
    let (status, body) = fx
        .get(&fx.bob, &format!("/api/v0/agents/{receiver}/communication"))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (status, body) = fx
        .put(
            &fx.bob,
            &format!("/api/v0/agents/{receiver}/communication"),
            serde_json::json!({"peer_id": caller, "enabled": true, "expected_generation": null}),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    // A read share may list the grants, not change them.
    assert_eq!(
        fx.share(&fx.alice, &receiver, "user", "bob", "read")
            .await
            .0,
        StatusCode::CREATED
    );
    let (status, view) = fx
        .get(&fx.bob, &format!("/api/v0/agents/{receiver}/communication"))
        .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert!(view["callers"].as_array().unwrap().is_empty());
    let (status, body) = fx
        .put(
            &fx.bob,
            &format!("/api/v0/agents/{receiver}/communication"),
            serde_json::json!({"peer_id": caller, "enabled": true, "expected_generation": null}),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    // A personal agent's communication is its own consent with its peers,
    // never the organization's list of callers.
    let (status, body) = fx
        .get(&fx.alice, &format!("/api/v0/agents/{caller}/communication"))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["peers"].is_array() && body.get("callers").is_none(),
        "{body}"
    );

    // The organization side only grants on personal caller agents.
    let (status, body) = fx
        .put(
            &fx.alice,
            &format!("/api/v0/agents/{receiver}/communication"),
            serde_json::json!({"peer_id": receiver, "enabled": true, "expected_generation": null}),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (status, body) = fx
        .put(
            &fx.alice,
            &format!("/api/v0/agents/{receiver}/communication"),
            serde_json::json!({"peer_id": "missing", "enabled": true, "expected_generation": null}),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

#[tokio::test]
async fn personal_revocation_cancels_the_in_flight_pair() {
    let p = paused_delegation(vec![
        tool_call(
            "finish",
            serde_json::json!({"result": {"answer": "The lesson is saved."}}),
        ),
        serde_json::json!({"content": "The org agent saved the lesson."}),
    ])
    .await;
    let p = &p;
    let (status, view) =
        p.fx.put(
            &p.fx.alice,
            &format!("/api/v0/agents/{}/communication", p.caller),
            serde_json::json!({
                "peer_id": p.receiver,
                "direction": "outbound",
                "enabled": false,
                "expected_generation": p.edge.generation
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    let peer = peer_in(&view, &p.receiver);
    assert_eq!(
        peer["outgoing"]["outbound_enabled"],
        serde_json::json!(false)
    );
    assert_eq!(
        chat::get_turn(&p.state.db, &p.session_id, &p.turn_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        TurnStatus::Cancelled
    );
    assert_eq!(
        chat::get_turn(&p.state.db, &p.child_session, &p.child_turn)
            .await
            .unwrap()
            .unwrap()
            .status,
        TurnStatus::Cancelled
    );
}

#[tokio::test]
async fn organization_revocation_cancels_the_in_flight_pair() {
    let p = paused_delegation(vec![
        tool_call(
            "finish",
            serde_json::json!({"result": {"answer": "The lesson is saved."}}),
        ),
        serde_json::json!({"content": "The org agent saved the lesson."}),
    ])
    .await;
    let p = &p;
    let (status, view) =
        p.fx.put(
            &p.fx.alice,
            &format!("/api/v0/agents/{}/communication", p.receiver),
            serde_json::json!({
                "peer_id": p.caller,
                "enabled": false,
                "expected_generation": p.edge.generation
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    let callers = view["callers"].as_array().unwrap();
    assert_eq!(callers.len(), 1);
    assert_eq!(
        callers[0]["incoming"]["inbound_enabled"],
        serde_json::json!(false)
    );
    assert_eq!(
        chat::get_turn(&p.state.db, &p.session_id, &p.turn_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        TurnStatus::Cancelled
    );
    assert_eq!(
        chat::get_turn(&p.state.db, &p.child_session, &p.child_turn)
            .await
            .unwrap()
            .unwrap()
            .status,
        TurnStatus::Cancelled
    );
}
