// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use rama::http::{Method, StatusCode};
use serde_json::json;

use super::agents::{TIME, fixture};

#[tokio::test]
async fn personal_dot_access_is_owner_scoped_and_records_spec_revisions() {
    let fx = fixture().await;
    let (_, created) = fx
        .post(&fx.alice, "/api/v0/dots", json!({"name":"Access"}))
        .await;
    let id = created["dot"]["id"].as_str().unwrap();
    let url = format!("/api/v0/dots/{id}/access");
    let (status, body) = fx.get(&fx.alice, &url).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let tool = body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["id"] == TIME)
        .unwrap();
    assert_eq!(tool["selected"], false);
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
    let (status, saved) = fx
        .send(
            Some(&fx.alice),
            Method::PUT,
            &url,
            Some(json!({"tools":[{"id":TIME,"permission":"always_ask"}]})),
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
    let dot = aiplane_agents::db::agents::get_personal_dot(&fx.state.db, "alice", id)
        .await
        .unwrap()
        .unwrap();
    let spec: serde_json::Value = serde_json::from_str(&dot.agent.draft_spec).unwrap();
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
async fn personal_dot_access_rejects_unknown_duplicate_and_invalid_permissions() {
    let fx = fixture().await;
    let (_, created) = fx
        .post(&fx.alice, "/api/v0/dots", json!({"name":"Rules"}))
        .await;
    let id = created["dot"]["id"].as_str().unwrap();
    let url = format!("/api/v0/dots/{id}/access");
    for (body, expected) in [
        (
            json!({"tools":[{"id":"unknown","permission":"always_allow"}]}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"tools":[{"id":TIME,"permission":"invalid"}]}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"tools":[{"id":TIME,"permission":"always_ask"},{"id":TIME,"permission":"always_allow"}]}),
            StatusCode::BAD_REQUEST,
        ),
    ] {
        let (status, response) = fx
            .send(Some(&fx.alice), Method::PUT, &url, Some(body))
            .await;
        assert_eq!(status, expected, "{response}");
    }
    let dot = aiplane_agents::db::agents::get_personal_dot(&fx.state.db, "alice", id)
        .await
        .unwrap()
        .unwrap();
    let spec: serde_json::Value = serde_json::from_str(&dot.agent.draft_spec).unwrap();
    assert_eq!(spec["main"]["tools"], json!([]));
}

#[tokio::test]
async fn personal_dot_access_cannot_select_a_user_disabled_tool() {
    let fx = fixture().await;
    let (_, created) = fx
        .post(&fx.alice, "/api/v0/dots", json!({"name":"Disabled"}))
        .await;
    let id = created["dot"]["id"].as_str().unwrap();
    aiplane_core::server::db::user_tool_prefs::set(&fx.state.db, "alice", TIME, false)
        .await
        .unwrap();
    let url = format!("/api/v0/dots/{id}/access");
    let (status, view) = fx.get(&fx.alice, &url).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert!(view["tools"].as_array().unwrap().is_empty());
    let (status, response) = fx
        .send(
            Some(&fx.alice),
            Method::PUT,
            &url,
            Some(json!({"tools":[{"id":TIME,"permission":"always_allow"}]})),
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
async fn personal_dot_action_rules_use_actual_catalogue_and_preserve_saved_unavailable_actions() {
    let fx = action_fixture().await;
    let (_, created) = fx
        .post(&fx.alice, "/api/v0/dots", json!({"name":"Actions"}))
        .await;
    let id = created["dot"]["id"].as_str().unwrap();
    let url = format!("/api/v0/dots/{id}/access");
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
    let body = json!({"tools":[{"id":"browse_page","permission":"always_ask","actions":[{"id":"read","rule":"allow"},{"id":"fill","rule":"deny"},{"id":"click","rule":"ask"}]}]});
    assert_eq!(
        fx.send(Some(&fx.bob), Method::PUT, &url, Some(body.clone()))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let (status, saved) = fx
        .send(Some(&fx.alice), Method::PUT, &url, Some(body))
        .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    let dot = aiplane_agents::db::agents::get_personal_dot(&fx.state.db, "alice", id)
        .await
        .unwrap()
        .unwrap();
    let saved: serde_json::Value = serde_json::from_str(&dot.agent.draft_spec).unwrap();
    assert_eq!(
        saved["main"]["tool_resources"]["browse_page"]["actions"],
        json!({"read":"allow","fill":"deny","click":"ask"})
    );
    assert_eq!(
        saved["main"]["tool_resources"]["browse_page"]["bind"],
        draft["main"]["tool_resources"]["browse_page"]["bind"]
    );
    assert_eq!(
        saved["main"]["tool_resources"]["browse_page"]["approval_timeout"],
        "5m"
    );
    assert_eq!(saved["main"]["instructions"], draft["main"]["instructions"]);
    let revisions: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM agent_draft_revisions WHERE principal_id=?")
            .bind(id)
            .fetch_one(&fx.state.db)
            .await
            .unwrap();
    assert_eq!(revisions, 2);
    let (status, _) = fx
        .send(
            Some(&fx.alice),
            Method::PUT,
            &url,
            Some(json!({"tools":[{"id":"browse_page","permission":"always_ask"}]})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let dot = aiplane_agents::db::agents::get_personal_dot(&fx.state.db, "alice", id)
        .await
        .unwrap()
        .unwrap();
    let saved: serde_json::Value = serde_json::from_str(&dot.agent.draft_spec).unwrap();
    assert_eq!(
        saved["main"]["tool_resources"]["browse_page"]["actions"],
        json!({})
    );
}

#[tokio::test]
async fn personal_dot_action_rules_reject_duplicates_unknown_values_and_unsupported_tools_without_revision()
 {
    let fx = action_fixture().await;
    let (_, created) = fx
        .post(&fx.alice, "/api/v0/dots", json!({"name":"Invalid actions"}))
        .await;
    let id = created["dot"]["id"].as_str().unwrap();
    let url = format!("/api/v0/dots/{id}/access");
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
                Some(json!({"tools":[{"id":tool,"permission":"always_ask","actions":actions}]})),
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
    let (status,response)=fx.send(Some(&fx.alice),Method::PUT,&url,Some(json!({"tools":[{"id":"browse_page","permission":"always_ask","actions":[{"id":"read","rule":"allow"}]}]}))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{response}");
}
