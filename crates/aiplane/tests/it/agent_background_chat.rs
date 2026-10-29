// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use super::agents::{Fx, fixture};
use rama::http::{Method, StatusCode};
use serde_json::json;
use session_core::db as chat;

async fn running_background() -> (Fx, String) {
    let fx = fixture().await;
    let (_, created) = fx
        .post(
            &fx.alice,
            "/api/v0/agents",
            json!({"kind": "personal", "name":"Background helper"}),
        )
        .await;
    let owned_agent = created["agent"]["id"].as_str().unwrap();
    let (_, created) = fx
        .post(
            &fx.alice,
            &format!("/api/v0/agents/{owned_agent}/sessions"),
            json!({}),
        )
        .await;
    let session = created["session"]["id"].as_str().unwrap().to_string();
    chat::create_turn_pair_if_idle(&fx.state.db, &session, "prompt", "reply", "Work", "m", true)
        .await
        .unwrap()
        .unwrap();
    sqlx::query("INSERT INTO scheduled_actions(id,user_id,name,prompt,model,cron,timezone,personal_agent_id,created_at,updated_at)
        VALUES('action','alice','Work','Work','m','0 * * * *','UTC',?,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')")
        .bind(owned_agent).execute(&fx.state.db).await.unwrap();
    sqlx::query("INSERT INTO scheduled_runs(id,action_id,fired_at,session_id,assistant_turn_id,tools_enabled,configuration_generation,agent_version,created_at)
        SELECT 'run',id,'2026-01-01T00:00:00Z',?,'reply',0,configuration_generation,agent_version,'2026-01-01T00:00:00Z' FROM scheduled_actions WHERE id='action'")
        .bind(&session).execute(&fx.state.db).await.unwrap();
    (fx, session)
}

#[tokio::test]
async fn viewing_background_before_worker_registration_does_not_error_its_turn() {
    let (fx, session) = running_background().await;
    let (status, _) = fx
        .get(
            &fx.alice,
            &format!("/api/v0/chat/sessions/{session}/events"),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let turn = chat::get_turn(&fx.state.db, &session, "reply")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(turn.status, chat::TurnStatus::InProgress);
}

#[tokio::test]
async fn interactive_message_cannot_overlap_a_running_background_pair() {
    let (fx, session) = running_background().await;
    let (status, response) = fx
        .post(
            &fx.alice,
            &format!("/api/v0/chat/sessions/{session}/messages"),
            json!({"model":"m","message":"New work"}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{response}");
    assert_eq!(
        chat::list_turns(&fx.state.db, &session)
            .await
            .unwrap()
            .len(),
        2
    );
    assert_eq!(fx.state.chats.active_count(), 0);
}

#[tokio::test]
async fn history_rewrites_refuse_background_during_registration_handoff() {
    let (fx, session) = running_background().await;
    for (turn, action, body) in [
        ("reply", "retry", json!({"model":"m"})),
        ("prompt", "edit", json!({"model":"m","message":"Changed"})),
    ] {
        let (status, response) = fx
            .post(
                &fx.alice,
                &format!("/api/v0/chat/sessions/{session}/turns/{turn}/{action}"),
                body,
            )
            .await;
        assert_eq!(status, StatusCode::CONFLICT, "{response}");
    }
    let turns = chat::list_turns(&fx.state.db, &session).await.unwrap();
    assert_eq!(turns.len(), 2);
    assert_eq!(turns[0].turn.user_content.as_deref(), Some("Work"));
    assert_eq!(turns[1].turn.status, chat::TurnStatus::InProgress);
    assert_eq!(fx.state.chats.active_count(), 0);
}

#[tokio::test]
async fn background_worker_cannot_receive_interactive_steers_or_history_rewrites() {
    let (fx, session) = running_background().await;
    let worker = match fx
        .state
        .chats
        .register_background("alice", "reply", &session)
    {
        session_core::RegisterOutcome::Registered { worker } => worker,
        _ => panic!("fresh background registration"),
    };
    let (status, response) = fx
        .post(
            &fx.alice,
            &format!("/api/v0/chat/sessions/{session}/messages"),
            json!({"model":"m","message":"Change direction"}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{response}");
    for (method, path, body) in [
        (
            Method::POST,
            format!("/api/v0/chat/sessions/{session}/steer"),
            Some(json!({"message":"Change direction"})),
        ),
        (
            Method::POST,
            format!("/api/v0/chat/sessions/{session}/turns/reply/retry"),
            Some(json!({"model":"m"})),
        ),
        (
            Method::POST,
            format!("/api/v0/chat/sessions/{session}/turns/prompt/edit"),
            Some(json!({"model":"m","message":"Changed"})),
        ),
        (
            Method::DELETE,
            format!("/api/v0/chat/sessions/{session}/turns/prompt"),
            None,
        ),
    ] {
        let (status, response) = fx.send(Some(&fx.alice), method, &path, body).await;
        assert_eq!(status, StatusCode::CONFLICT, "{response}");
    }
    assert!(worker.steers.take().is_empty());
    assert_eq!(
        chat::list_turns(&fx.state.db, &session)
            .await
            .unwrap()
            .len(),
        2
    );
    assert_eq!(fx.state.chats.running_for_user("alice"), 0);
    fx.state.chats.clear("alice", &worker);
}
