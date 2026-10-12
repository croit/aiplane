// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Cross-organization delegation, dispatch and result
//! (`docs/dots-cross-organization.md` → "Dispatch and result"): a personal
//! Dot's `forward_request` addressing an organization Dot — the receiver
//! runs as its own principal on its published spec, the caller reads only
//! the bounded answer, and a child pause suspends the caller on the same
//! request.

use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::time::Duration;

use crate::common::Service as _;
use aiplane::rama_server::{RamaState, router::router};
use aiplane_agents::db::{
    agents, child_sessions, dot_delegation, dot_sessions, system_principals::NewPrincipal,
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

async fn caller_dot(state: &RamaState, name: &str, spec: &str) -> String {
    agents::create_personal_dot(
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

async fn org_dot(fx: &Fx, org: &Value, name: &str) -> String {
    let (status, created) = fx
        .post(
            &fx.alice,
            &format!("/api/v0/organizations/{}/dots", created_id(&org["organization"])),
            serde_json::json!({"name":name,"display":name,"description":"Actual organization assistant"}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    created_id(&created["agent"])
}

async fn publish_org_dot(fx: &Fx, dot: &str, spec: Value, tools: &[&str]) {
    assert_eq!(
        fx.grant(&fx.alice, dot, "model", "m").await,
        StatusCode::CREATED
    );
    for tool in tools {
        assert_eq!(
            fx.grant(&fx.alice, dot, "tool", tool).await,
            StatusCode::CREATED
        );
    }
    let (status, saved) = fx.put_draft(&fx.alice, dot, spec).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    let (status, published) = fx.publish(&fx.alice, dot).await;
    assert_eq!(status, StatusCode::CREATED, "{published}");
}

/// One consent decision of a direction, on a fresh edge.
async fn set_direction(
    fx: &Fx,
    caller: &str,
    receiver: &str,
    direction: dot_delegation::ConsentDirection,
) {
    let edit = dot_delegation::CrossBoundaryConsentEdit {
        actor_user_id: "alice",
        caller_dot_id: caller,
        receiver_dot_id: receiver,
        direction,
        enabled: true,
        expected_generation: None,
    };
    assert!(
        matches!(
            dot_delegation::set_cross_boundary_consent(&fx.state.db, edit)
                .await
                .unwrap(),
            dot_delegation::ConsentSaved::Saved(_)
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
async fn consent(fx: &Fx, caller: &str, receiver: &str) -> dot_delegation::DotDelegationEdge {
    let edit = |direction| dot_delegation::CrossBoundaryConsentEdit {
        actor_user_id: "alice",
        caller_dot_id: caller,
        receiver_dot_id: receiver,
        direction,
        enabled: true,
        expected_generation: None,
    };
    let saved = dot_delegation::set_cross_boundary_consent(
        &fx.state.db,
        edit(dot_delegation::ConsentDirection::Outbound),
    )
    .await
    .unwrap();
    let dot_delegation::ConsentSaved::Saved(edge) = saved else {
        panic!("outbound consent saved");
    };
    let mut inbound = edit(dot_delegation::ConsentDirection::Inbound);
    inbound.expected_generation = Some(&edge.generation);
    let saved = dot_delegation::set_cross_boundary_consent(&fx.state.db, inbound)
        .await
        .unwrap();
    let dot_delegation::ConsentSaved::Saved(edge) = saved else {
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
async fn cross_boundary_dispatch_runs_the_organization_dot_with_its_own_authority_and_returns_its_answer()
 {
    let upstream = MockServer::start().await;
    let fx = fixture_with_forward(&upstream).await;
    let state = Arc::new(fx.state.clone());
    let caller = caller_dot(
        &fx.state,
        "Caller",
        r#"{"main":{"tools":["forward_request"],"tool_resources":{"forward_request":{"permission":"always_allow"}}}}"#,
    )
    .await;
    let org = organization(&fx, "Ticket Support", &["managers"]).await;
    let receiver = org_dot(&fx, &org, "Tickets").await;
    publish_org_dot(
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
                    serde_json::json!({"receiver_dot_id": receiver, "message": "Open a ticket for the broken printer"}),
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
        dot_sessions::link(&fx.state.db, "alice", &caller, &session_id)
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
    assert_eq!(result["receiver_dot_id"], serde_json::json!(receiver));

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
    let caller = caller_dot(
        &fx.state,
        "Caller",
        r#"{"main":{"tools":["forward_request"],"tool_resources":{"forward_request":{"permission":"always_allow"}}}}"#,
    )
    .await;
    let org = organization(&fx, "Ticket Support", &["managers"]).await;
    let receiver = org_dot(&fx, &org, "Tickets").await;
    publish_org_dot(
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
                dot_delegation::ConsentDirection::Outbound,
            )
            .await
        }
        RefusalCase::InboundOnly => {
            set_direction(
                &fx,
                &caller,
                &receiver,
                dot_delegation::ConsentDirection::Inbound,
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
                    serde_json::json!({"receiver_dot_id": receiver, "message": "Open a ticket"}),
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
        dot_sessions::link(&fx.state.db, "alice", &caller, &session_id)
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
    let caller = caller_dot(
        &fx.state,
        "Caller",
        r#"{"main":{"tools":["forward_request"],"tool_resources":{"forward_request":{"permission":"always_allow"}}}}"#,
    )
    .await;
    let org = organization(&fx, "Ticket Support", &["managers"]).await;
    let receiver = org_dot(&fx, &org, "Tickets").await;
    publish_org_dot(
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
                    serde_json::json!({"receiver_dot_id": receiver, "message": "Save this explicit lesson"}),
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
        dot_sessions::link(&fx.state.db, "alice", &caller, &session_id)
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
