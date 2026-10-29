// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use rama::http::{Method, StatusCode};
use serde_json::json;

use super::agents::{Fx, TIME, fixture};

async fn access_revision(fx: &Fx, user: &str, url: &str) -> String {
    let (status, body) = fx.get(user, url).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["revision"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn personal_agent_access_is_owner_scoped_and_records_spec_revisions() {
    let fx = fixture().await;
    let (_, created) = fx
        .post(
            &fx.alice,
            "/api/v0/agents",
            json!({"kind": "personal", "name":"Access"}),
        )
        .await;
    let id = created["agent"]["id"].as_str().unwrap();
    let url = format!("/api/v0/agents/{id}/access");
    let (status, body) = fx.get(&fx.alice, &url).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let tool = body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["id"] == TIME)
        .unwrap();
    assert_eq!(
        tool["selected"], true,
        "a personal agent made by hand starts with the current time"
    );
    assert_eq!(tool["permission"], "always_allow");
    assert!(!tool["description"].as_str().unwrap().is_empty());
    assert_eq!(fx.get(&fx.bob, &url).await.0, StatusCode::NOT_FOUND);
    assert_eq!(
        fx.send(Some(&fx.bob), Method::PUT, &url, Some(json!({"tools":[]})))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    aiplane_agents::db::agents::update_draft(
        &fx.state.db,
        id,
        &json!({"main":{"tools":[],"instructions":{"response":"Be concise"}}}).to_string(),
        "alice",
        aiplane_agents::db::agents::DraftChange::default(),
    )
    .await
    .unwrap();
    let revision = access_revision(&fx, &fx.alice, &url).await;
    let (status, saved) = fx
        .send(
            Some(&fx.alice),
            Method::PUT,
            &url,
            Some(json!({"revision":revision,"tools":[{"id":TIME,"permission":"always_ask"}]})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    let tool = saved["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["id"] == TIME)
        .unwrap();
    assert_eq!(tool["selected"], true);
    assert_eq!(tool["permission"], "always_ask");
    let owned_agent = aiplane_agents::db::agents::get_personal_agent(&fx.state.db, "alice", id)
        .await
        .unwrap()
        .unwrap();
    let spec: serde_json::Value = serde_json::from_str(&owned_agent.agent.draft_spec).unwrap();
    assert_eq!(spec["main"]["tools"], json!([TIME]));
    assert_eq!(spec["main"]["instructions"]["response"], "Be concise");
    let revisions: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM agent_draft_revisions WHERE principal_id = ?")
            .bind(id)
            .fetch_one(&fx.state.db)
            .await
            .unwrap();
    assert_eq!(revisions, 2);
}

#[tokio::test]
async fn personal_agent_access_rejects_a_stale_editor_before_replacing_newer_rules() {
    let fx = fixture().await;
    let (_, created) = fx
        .post(
            &fx.alice,
            "/api/v0/agents",
            json!({"kind": "personal", "name":"Stale access"}),
        )
        .await;
    let id = created["agent"]["id"].as_str().unwrap();
    let url = format!("/api/v0/agents/{id}/access");
    let stale_revision = access_revision(&fx, &fx.alice, &url).await;
    let newer = json!({
        "main": {
            "tools": [],
            "tool_resources": {}
        }
    });
    aiplane_agents::db::agents::update_draft(
        &fx.state.db,
        id,
        &newer.to_string(),
        "alice",
        Default::default(),
    )
    .await
    .unwrap();

    let (status, response) = fx
        .send(
            Some(&fx.alice),
            Method::PUT,
            &url,
            Some(json!({
                "revision": stale_revision,
                "tools": [{"id": TIME, "permission": "always_ask"}]
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{response}");
    let saved = aiplane_agents::db::agents::get_personal_agent(&fx.state.db, "alice", id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&saved.agent.draft_spec).unwrap(),
        newer
    );
    assert_ne!(access_revision(&fx, &fx.alice, &url).await, stale_revision);
}

#[tokio::test]
async fn personal_agent_access_rejects_unknown_duplicate_and_invalid_permissions() {
    let fx = fixture().await;
    let (_, created) = fx
        .post(
            &fx.alice,
            "/api/v0/agents",
            json!({"kind": "personal", "name":"Rules"}),
        )
        .await;
    let id = created["agent"]["id"].as_str().unwrap();
    let url = format!("/api/v0/agents/{id}/access");
    let revision = access_revision(&fx, &fx.alice, &url).await;
    for (body, expected) in [
        (
            json!({"revision":revision,"tools":[{"id":"unknown","permission":"always_allow"}]}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"revision":revision,"tools":[{"id":TIME,"permission":"invalid"}]}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"revision":revision,"tools":[{"id":TIME,"permission":"always_ask"},{"id":TIME,"permission":"always_allow"}]}),
            StatusCode::BAD_REQUEST,
        ),
    ] {
        let (status, response) = fx
            .send(Some(&fx.alice), Method::PUT, &url, Some(body))
            .await;
        assert_eq!(status, expected, "{response}");
    }
    let owned_agent = aiplane_agents::db::agents::get_personal_agent(&fx.state.db, "alice", id)
        .await
        .unwrap()
        .unwrap();
    let spec: serde_json::Value = serde_json::from_str(&owned_agent.agent.draft_spec).unwrap();
    assert_eq!(
        spec["main"]["tools"],
        json!([TIME]),
        "a refused save keeps the starter selection"
    );
}

#[tokio::test]
async fn personal_agent_access_cannot_select_a_user_disabled_tool() {
    let fx = fixture().await;
    let (_, created) = fx
        .post(
            &fx.alice,
            "/api/v0/agents",
            json!({"kind": "personal", "name":"Disabled"}),
        )
        .await;
    let id = created["agent"]["id"].as_str().unwrap();
    let revision = access_revision(&fx, &fx.alice, &format!("/api/v0/agents/{id}/access")).await;
    let (status, cleared) = fx
        .send(
            Some(&fx.alice),
            Method::PUT,
            &format!("/api/v0/agents/{id}/access"),
            Some(json!({"revision": revision, "tools": []})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{cleared}");
    aiplane_core::server::db::user_tool_prefs::set(&fx.state.db, "alice", TIME, false)
        .await
        .unwrap();
    let url = format!("/api/v0/agents/{id}/access");
    let (status, view) = fx.get(&fx.alice, &url).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert!(view["tools"].as_array().unwrap().is_empty());
    let (status, response) = fx
        .send(
            Some(&fx.alice),
            Method::PUT,
            &url,
            Some(json!({"revision":view["revision"],"tools":[{"id":TIME,"permission":"always_allow"}]})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{response}");
}

async fn action_fixture() -> super::agents::Fx {
    use aiplane_core::server::config::SandboxConfig;
    use aiplane_runtime::server::tools::sandbox::{BrowsePage, SandboxClient};
    use std::sync::Arc;
    let client = SandboxClient::new(
        Arc::new(SandboxConfig {
            enabled: true,
            runner_url: "http://127.0.0.1:9".into(),
            timeout_secs: 1,
            max_artifact_bytes: 1024,
        }),
        aiplane_runtime::server::state::RuntimeSettings::new_handle("http://localhost"),
    );
    super::agents::fixture_with(
        None,
        aiplane_runtime::server::tools::ToolRegistry::new()
            .with(BrowsePage(client))
            .with(aiplane_runtime::server::tools::time::CurrentTimestamp),
        &["browse_page", TIME],
    )
    .await
}

#[tokio::test]
async fn personal_agent_action_rules_use_actual_catalogue_and_preserve_saved_unavailable_actions() {
    let fx = action_fixture().await;
    let (_, created) = fx
        .post(
            &fx.alice,
            "/api/v0/agents",
            json!({"kind": "personal", "name":"Actions"}),
        )
        .await;
    let id = created["agent"]["id"].as_str().unwrap();
    let url = format!("/api/v0/agents/{id}/access");
    let (status, inventory) = fx.get(&fx.alice, &url).await;
    assert_eq!(status, StatusCode::OK, "{inventory}");
    let browse = inventory["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == "browse_page")
        .unwrap();
    let read = browse["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == "read")
        .unwrap();
    assert!(!read["description"].as_str().unwrap().is_empty());
    let time = inventory["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == TIME)
        .unwrap();
    assert!(time["actions"].is_null());
    let draft = json!({"main":{"tools":["browse_page"],"instructions":{"response":"Be concise"},"tool_resources":{"browse_page":{"permission":"always_ask","approval_timeout":"5m","bind":{"project":{"const":"example"}},"actions":{"removed":"deny"}}}}});
    aiplane_agents::db::agents::update_draft(
        &fx.state.db,
        id,
        &draft.to_string(),
        "alice",
        Default::default(),
    )
    .await
    .unwrap();
    let (_, inventory) = fx.get(&fx.alice, &url).await;
    let browse = inventory["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == "browse_page")
        .unwrap();
    assert_eq!(
        browse["action_rules"],
        json!([{"id":"removed","rule":"deny"}])
    );
    let body = json!({"revision":inventory["revision"],"tools":[{"id":"browse_page","permission":"always_ask","actions":[{"id":"read","rule":"allow"},{"id":"fill","rule":"deny"},{"id":"click","rule":"ask"}]}]});
    assert_eq!(
        fx.send(Some(&fx.bob), Method::PUT, &url, Some(body.clone()))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let (status, access) = fx
        .send(Some(&fx.alice), Method::PUT, &url, Some(body))
        .await;
    assert_eq!(status, StatusCode::OK, "{access}");
    let owned_agent = aiplane_agents::db::agents::get_personal_agent(&fx.state.db, "alice", id)
        .await
        .unwrap()
        .unwrap();
    let saved_spec: serde_json::Value =
        serde_json::from_str(&owned_agent.agent.draft_spec).unwrap();
    assert_eq!(
        saved_spec["main"]["tool_resources"]["browse_page"]["actions"],
        json!({"read":"allow","fill":"deny","click":"ask"})
    );
    assert_eq!(
        saved_spec["main"]["tool_resources"]["browse_page"]["bind"],
        draft["main"]["tool_resources"]["browse_page"]["bind"]
    );
    assert_eq!(
        saved_spec["main"]["tool_resources"]["browse_page"]["approval_timeout"],
        "5m"
    );
    assert_eq!(
        saved_spec["main"]["instructions"],
        draft["main"]["instructions"]
    );
    let revisions: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM agent_draft_revisions WHERE principal_id=?")
            .bind(id)
            .fetch_one(&fx.state.db)
            .await
            .unwrap();
    assert_eq!(revisions, 2);
    let (status, cleared_access) = fx
        .send(
            Some(&fx.alice),
            Method::PUT,
            &url,
        Some(json!({"revision":access["revision"],"tools":[{"id":"browse_page","permission":"always_ask"}]})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{cleared_access}");
    let owned_agent = aiplane_agents::db::agents::get_personal_agent(&fx.state.db, "alice", id)
        .await
        .unwrap()
        .unwrap();
    let saved_spec: serde_json::Value =
        serde_json::from_str(&owned_agent.agent.draft_spec).unwrap();
    assert_eq!(
        saved_spec["main"]["tool_resources"]["browse_page"]["actions"],
        json!({})
    );
}

#[tokio::test]
async fn personal_agent_action_rules_reject_duplicates_unknown_values_and_unsupported_tools_without_revision()
 {
    let fx = action_fixture().await;
    let (_, created) = fx
        .post(
            &fx.alice,
            "/api/v0/agents",
            json!({"kind": "personal", "name":"Invalid actions"}),
        )
        .await;
    let id = created["agent"]["id"].as_str().unwrap();
    let url = format!("/api/v0/agents/{id}/access");
    let revision = access_revision(&fx, &fx.alice, &url).await;
    for (tool, actions) in [
        (
            "browse_page",
            json!([{"id":"read","rule":"allow"},{"id":"read","rule":"deny"}]),
        ),
        ("browse_page", json!([{"id":"read","rule":"invalid"}])),
        ("browse_page", json!([{"id":"unknown","rule":"deny"}])),
        (TIME, json!([{"id":"read","rule":"allow"}])),
    ] {
        let (status, response) = fx
            .send(
                Some(&fx.alice),
                Method::PUT,
                &url,
                Some(json!({"revision":revision,"tools":[{"id":tool,"permission":"always_ask","actions":actions}]})),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{response}");
    }
    let revisions: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM agent_draft_revisions WHERE principal_id=?")
            .bind(id)
            .fetch_one(&fx.state.db)
            .await
            .unwrap();
    assert_eq!(revisions, 0);
    aiplane_core::server::db::user_tool_prefs::set(&fx.state.db, "alice", "browse_page", false)
        .await
        .unwrap();
    let (status,response)=fx.send(Some(&fx.alice),Method::PUT,&url,Some(json!({"revision":revision,"tools":[{"id":"browse_page","permission":"always_ask","actions":[{"id":"read","rule":"allow"}]}]}))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{response}");
}
