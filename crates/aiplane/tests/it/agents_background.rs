// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use rama::http::{Method, StatusCode};
use serde_json::{Value, json};

use super::agents::fixture;

fn schedule(owned_agent: Option<&str>, session: Option<&str>) -> Value {
    json!({"name":"Background", "prompt":"Review updates", "model":"m", "cron":"0 * * * *",
        "timezone":"UTC", "tools_enabled":true, "reuse_conversation":true,"reuse_rounds":5,
        "personal_agent_id":owned_agent, "linked_session_id":session})
}

#[tokio::test]
async fn agent_background_bindings_are_owned_and_linked_chats_match() {
    let fx = fixture().await;
    let (_, created) = fx
        .post(
            &fx.alice,
            "/api/v0/agents",
            json!({"kind": "personal", "name":"Worker"}),
        )
        .await;
    let owned_agent = created["agent"]["id"].as_str().unwrap();
    let (_, chat) = fx
        .post(
            &fx.alice,
            &format!("/api/v0/agents/{owned_agent}/sessions"),
            json!({}),
        )
        .await;
    let session = chat["session"]["id"].as_str().unwrap();
    let (status, action) = fx
        .post(
            &fx.alice,
            "/api/v0/scheduled",
            schedule(Some(owned_agent), Some(session)),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{action}");
    assert_eq!(action["personal_agent_id"], owned_agent);
    assert_eq!(action["agent"]["name"], "Worker");
    assert_eq!(
        fx.post(
            &fx.bob,
            "/api/v0/scheduled",
            schedule(Some(owned_agent), None)
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        fx.post(
            &fx.alice,
            "/api/v0/scheduled",
            schedule(None, Some(session))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (status, listing) = fx
        .get(
            &fx.alice,
            &format!("/api/v0/chat/sessions?agent_id={owned_agent}"),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{listing}");
    assert!(
        listing["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|chat| chat["id"] == session)
    );
    let (_, listing) = fx.get(&fx.alice, "/api/v0/chat/sessions?agent_id=").await;
    assert!(
        !listing["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|chat| chat["id"] == session)
    );
    let id = action["id"].as_str().unwrap();
    let (status, updated) = fx
        .send(
            Some(&fx.alice),
            Method::PUT,
            &format!("/api/v0/scheduled/{id}"),
            Some({
                let mut body = schedule(None, None);
                body["expected_configuration_generation"] =
                    action["configuration_generation"].clone();
                body
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert!(updated["last_session_id"].is_null());
}

#[tokio::test]
async fn agent_background_unchanged_inactive_bindings_can_be_edited_but_not_newly_selected() {
    let fx = fixture().await;
    let (_, created) = fx
        .post(
            &fx.alice,
            "/api/v0/agents",
            json!({"kind": "personal", "name":"Paused"}),
        )
        .await;
    let owned_agent = created["agent"]["id"].as_str().unwrap();
    let (status, action) = fx
        .post(
            &fx.alice,
            "/api/v0/scheduled",
            schedule(Some(owned_agent), None),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{action}");
    fx.send(
        Some(&fx.alice),
        Method::PUT,
        &format!("/api/v0/agents/{owned_agent}/active"),
        Some(json!({"active":false})),
    )
    .await;
    assert_eq!(
        fx.post(
            &fx.alice,
            "/api/v0/scheduled",
            schedule(Some(owned_agent), None)
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (status, updated) = fx
        .send(
            Some(&fx.alice),
            Method::PUT,
            &format!("/api/v0/scheduled/{}", action["id"].as_str().unwrap()),
            Some({
                let mut body = schedule(Some(owned_agent), None);
                body["expected_configuration_generation"] =
                    action["configuration_generation"].clone();
                body
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["agent"]["active"], false);
}

#[tokio::test]
async fn scheduled_delete_conflicts_with_the_exact_running_turn_and_then_succeeds() {
    use aiplane_runtime::server::scheduled;
    use session_core::db as chat;

    let fx = fixture().await;
    let (status, action) = fx
        .post(&fx.alice, "/api/v0/scheduled", schedule(None, None))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{action}");
    let id = action["id"].as_str().unwrap();
    let session = chat::create_session(&fx.state.db, "alice").await.unwrap();
    chat::create_assistant_turn_in_progress(&fx.state.db, &session.id, "running-turn", "m")
        .await
        .unwrap();
    let run = scheduled::record_run_start(&fx.state.db, id).await.unwrap();
    scheduled::attach_run_turn(
        &fx.state.db,
        &run,
        &session.id,
        "running-turn",
        true,
        Some(10),
    )
    .await
    .unwrap();
    let path = format!("/api/v0/scheduled/{id}");
    assert_eq!(
        fx.send(Some(&fx.bob), Method::DELETE, &path, None).await.0,
        StatusCode::NOT_FOUND
    );
    let (status, error) = fx.send(Some(&fx.alice), Method::DELETE, &path, None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{error}");
    assert_eq!(error["error"]["type"], "run_pending");
    assert!(
        scheduled::get(
            &fx.state.db,
            &aiplane_runtime::server::background::AutomationOwner::User("alice".into()),
            id
        )
        .await
        .unwrap()
        .is_some()
    );
    chat::finalize_turn(
        &fx.state.db,
        "running-turn",
        chat::TurnStatus::Cancelled,
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        fx.send(Some(&fx.alice), Method::DELETE, &path, None)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn agent_webhook_fire_and_rerun_wait_for_approval_and_cannot_be_deleted_until_stopped() {
    use super::agents::{TIME, fixture_on};
    use aiplane_runtime::server::webhooks;
    use session_core::db as chat;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let upstream = MockServer::start().await;
    let chunk = json!({"id":"approval-response", "object":"chat.completion.chunk", "created":0, "model":"m",
        "choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"approval-call","type":"function",
            "function":{"name":TIME,"arguments":"{}"}}]},"finish_reason":"tool_calls"}]});
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(format!("data: {chunk}\n\ndata: [DONE]\n\n")),
        )
        .mount(&upstream)
        .await;
    let fx = fixture_on(Some(&upstream.uri())).await;
    let (_, created) = fx
        .post(
            &fx.alice,
            "/api/v0/agents",
            json!({"kind": "personal", "name":"Approver"}),
        )
        .await;
    let agent_id = created["agent"]["id"].as_str().unwrap();
    let access_url = format!("/api/v0/agents/{agent_id}/access");
    let (access_status, access) = fx.get(&fx.alice, &access_url).await;
    assert_eq!(access_status, StatusCode::OK, "{access}");
    let (status, access) = fx
        .send(
            Some(&fx.alice),
            Method::PUT,
            &access_url,
            Some(json!({"revision":access["revision"],"tools":[{"id":TIME,"permission":"always_ask"}]})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{access}");
    let (status, created) = fx.post(&fx.alice, "/api/v0/webhooks", json!({"name":"Approve updates","prompt":"Review payload",
        "model":"m","tools_enabled":true,"synchronous":true,"reuse_conversation":false,"reuse_rounds":5,"personal_agent_id":agent_id})).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let hook_id = created["webhook"]["id"].as_str().unwrap();
    let secret = created["secret"].as_str().unwrap();
    let (status, fire) = fx
        .send(
            None,
            Method::POST,
            &format!("/hooks/{secret}"),
            Some(json!({"event":"update"})),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{fire}");
    assert_eq!(fire["status"], "waiting");
    let first = fire["session_id"].as_str().unwrap();
    let runs = webhooks::list_runs(&fx.state.db, hook_id, 10)
        .await
        .unwrap();
    assert_eq!(runs[0].status.as_deref(), Some("waiting"));
    assert_eq!(
        chat::get_turn(
            &fx.state.db,
            first,
            runs[0].assistant_turn_id.as_deref().unwrap()
        )
        .await
        .unwrap()
        .unwrap()
        .status,
        chat::TurnStatus::Suspended
    );
    let (status, rerun) = fx
        .post(
            &fx.alice,
            &format!("/api/v0/webhooks/{hook_id}/rerun"),
            json!({"prompt":"Review again", "expected_configuration_generation": created["webhook"]["configuration_generation"]}),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{rerun}");
    assert_eq!(rerun["status"], "waiting");
    let second = rerun["session_id"].as_str().unwrap();
    assert_ne!(first, second);
    for session in [first, second] {
        assert_eq!(
            aiplane_agents::db::agent_sessions::for_session(&fx.state.db, "alice", session)
                .await
                .unwrap()
                .unwrap()
                .agent_id,
            agent_id
        );
    }
    let delete = format!("/api/v0/webhooks/{hook_id}");
    assert_eq!(
        fx.send(Some(&fx.bob), Method::DELETE, &delete, None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        fx.send(Some(&fx.alice), Method::DELETE, &delete, None)
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        fx.post(
            &fx.alice,
            &format!("/api/v0/webhooks/{hook_id}/toggle"),
            json!({"enabled":false, "expected_configuration_generation": created["webhook"]["configuration_generation"]})
        )
        .await
        .0,
        StatusCode::OK
    );
    for session in [first, second] {
        let (status, result) = fx
            .post(
                &fx.alice,
                &format!("/api/v0/chat/sessions/{session}/cancel"),
                json!({}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{result}");
    }
    let runs = webhooks::list_runs(&fx.state.db, hook_id, 10)
        .await
        .unwrap();
    assert!(
        runs.iter()
            .all(|run| run.status.as_deref() == Some("error"))
    );
    assert_eq!(
        fx.send(Some(&fx.alice), Method::DELETE, &delete, None)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(upstream.received_requests().await.unwrap().len(), 2);
}

/// A background run bound to an agent, suspended for approval, must still resume
/// when the agent's principal is disabled — it degrades to the person, the
/// pre-agents behavior — instead of failing with `agent_unavailable`.
#[tokio::test]
async fn suspended_background_run_resumes_as_person_when_its_agent_principal_is_disabled() {
    use super::agents::{TIME, fixture_on};
    use super::turn_suspension::{Scripted, tool_call};
    use aiplane_agents::db::system_principals;
    use session_core::db as chat;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer};

    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(Scripted {
            deltas: vec![tool_call(TIME, json!({})), json!({"content": "Done."})],
            served: std::sync::atomic::AtomicUsize::new(0),
            delay: std::time::Duration::ZERO,
        })
        .mount(&upstream)
        .await;
    let fx = fixture_on(Some(&upstream.uri())).await;
    let (_, created) = fx
        .post(
            &fx.alice,
            "/api/v0/agents",
            json!({"kind": "personal", "name":"Approver"}),
        )
        .await;
    let agent_id = created["agent"]["id"].as_str().unwrap();
    let access_url = format!("/api/v0/agents/{agent_id}/access");
    let (access_status, access) = fx.get(&fx.alice, &access_url).await;
    assert_eq!(access_status, StatusCode::OK, "{access}");
    let (status, access) = fx
        .send(
            Some(&fx.alice),
            Method::PUT,
            &access_url,
            Some(json!({"revision":access["revision"],"tools":[{"id":TIME,"permission":"always_ask"}]})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{access}");
    let (status, created) = fx
        .post(
            &fx.alice,
            "/api/v0/webhooks",
            json!({"name":"Approve updates","prompt":"Review payload","model":"m","tools_enabled":true,"synchronous":true,"reuse_conversation":false,"reuse_rounds":5,"personal_agent_id":agent_id}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let hook_id = created["webhook"]["id"].as_str().unwrap();
    let secret = created["secret"].as_str().unwrap();
    let (status, fire) = fx
        .send(
            None,
            Method::POST,
            &format!("/hooks/{secret}"),
            Some(json!({"event":"update"})),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{fire}");
    assert_eq!(fire["status"], "waiting");
    let session = fire["session_id"].as_str().unwrap();
    let runs = aiplane_runtime::server::webhooks::list_runs(&fx.state.db, hook_id, 10)
        .await
        .unwrap();
    let turn_id = runs[0].assistant_turn_id.as_deref().unwrap();
    let suspension = chat::get_suspension(&fx.state.db, turn_id)
        .await
        .unwrap()
        .unwrap();
    // Disable the agent's principal: the suspended run must still resume.
    system_principals::disable(&fx.state.db, agent_id, "alice")
        .await
        .unwrap();
    let (status, body) = fx
        .send(
            Some(&fx.alice),
            Method::POST,
            &format!("/api/v0/chat/sessions/{session}/turns/{turn_id}/resume"),
            Some(json!({"decision":"allow_once","request_id":suspension.request_id})),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
}

#[tokio::test]
async fn background_openapi_describes_binding_waiting_and_delete_conflicts() {
    let fx = fixture().await;
    let (status, document) = fx.get(&fx.alice, "/openapi.json").await;
    assert_eq!(status, StatusCode::OK, "{document}");
    for path in ["/api/v0/scheduled/{id}", "/api/v0/webhooks/{id}"] {
        assert!(document["paths"][path]["delete"]["responses"]["409"].is_object());
    }
    assert!(
        document["paths"]["/api/v0/webhooks/{id}/rerun"]["post"]["responses"]["202"].is_object()
    );
    for name in ["ScheduledBody", "WebhookBody"] {
        assert!(
            document["components"]["schemas"][name]["properties"]["personal_agent_id"].is_object(),
            "missing binding property in {name}"
        );
    }
}
