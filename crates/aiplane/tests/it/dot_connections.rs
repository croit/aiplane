// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use super::agents::{Fx, fixture};
use aiplane_core::server::db::mcp_connections::{self, ConnectionOwner, NewConnection};
use rama::http::{Method, StatusCode};
use serde_json::{Value, json};

async fn setup(fx: &Fx) -> String {
    let (status, created) = fx
        .post(&fx.alice, "/api/v0/dots", json!({"name":"Harald"}))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    sqlx::query("INSERT INTO mcp_catalog_connectors(key,name,url,auth,scope,use_dcr,client_id,enabled,created_at,updated_at) VALUES('workspace','Actual Workspace','https://workspace.example.test/mcp','oauth2','per_user',0,'client',1,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')")
        .execute(&fx.state.db).await.unwrap();
    created["dot"]["id"].as_str().unwrap().to_owned()
}

async fn connection(fx: &Fx, owner: ConnectionOwner) -> NewConnection {
    NewConnection {
        owner,
        connector_key: "workspace".into(),
        catalog_generation: aiplane_core::server::db::mcp_catalog::get(&fx.state.db, "workspace")
            .await
            .unwrap()
            .unwrap()
            .dispatch_generation,
        access_token_ct: vec![0, 1, 255],
        access_token_nonce: vec![2],
        refresh_token_ct: Some(vec![3]),
        refresh_token_nonce: Some(vec![4]),
        token_expires_at: None,
        scopes: vec!["mail.read".into()],
        dcr_client_id: None,
        dcr_client_secret_ct: None,
        dcr_client_secret_nonce: None,
        token_url: Some("https://workspace.example.test/token".into()),
    }
}

fn row(body: &Value) -> &Value {
    body["connections"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["key"] == "workspace")
        .unwrap()
}

#[tokio::test]
async fn changed_catalogue_resource_requires_reauthorization_without_erasing_credentials() {
    let fx = fixture().await;
    let dot = setup(&fx).await;
    let user = mcp_connections::upsert_connection(
        &fx.state.db,
        connection(&fx, ConnectionOwner::User("alice".into())).await,
    )
    .await
    .unwrap()
    .unwrap();
    let own = mcp_connections::upsert_connection(
        &fx.state.db,
        connection(&fx, ConnectionOwner::Principal(dot.clone())).await,
    )
    .await
    .unwrap()
    .unwrap();
    let path = format!("/api/v0/dots/{dot}/connections/workspace");
    let (status, selected) = fx.send(Some(&fx.alice), Method::PUT, &path, Some(json!({
        "mode":"principal","expected_binding_generation":null,"connection_id":own.connection_id,"connection_generation":own.generation
    }))).await;
    assert_eq!(status, StatusCode::OK, "{selected}");
    super::dot_oauth::change_catalogue_url(&fx, "https://replacement.example.test/mcp").await;
    for (mode, identity) in [("principal", &own), ("user", &user)] {
        let (status, rejected) = fx
            .send(
                Some(&fx.alice),
                Method::PUT,
                &path,
                Some(json!({
                    "mode": mode,
                    "expected_binding_generation": row(&selected)["selected"]["binding_generation"],
                    "connection_id": identity.connection_id,
                    "connection_generation": identity.generation
                })),
            )
            .await;
        assert_eq!(status, StatusCode::CONFLICT, "{rejected}");
        assert_eq!(rejected["error"]["code"], "connection_unavailable");
    }
    let (status, inventory) = fx
        .get(&fx.alice, &format!("/api/v0/dots/{dot}/connections"))
        .await;
    assert_eq!(status, StatusCode::OK, "{inventory}");
    let account = row(&inventory);
    for (credential, identity) in [("user_connection", &user), ("principal_connection", &own)] {
        assert_eq!(account[credential]["id"], identity.connection_id);
        assert_eq!(account[credential]["generation"], identity.generation);
        assert_eq!(account[credential]["needs_reauth"], true);
    }
    assert_eq!(account["can_connect"], true);
    assert_eq!(account["can_disconnect"], true);
    assert_eq!(account["selected"]["connection_id"], Value::Null);
    assert_eq!(
        account["selected"]["binding_generation"],
        row(&selected)["selected"]["binding_generation"]
    );
    let (status, integrations) = fx.get(&fx.alice, "/api/v0/integrations").await;
    assert_eq!(status, StatusCode::OK, "{integrations}");
    let ordinary = integrations["connectors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["key"] == "workspace")
        .unwrap();
    assert_eq!(ordinary["connected"], true);
    assert_eq!(ordinary["errored"], true);
    assert_eq!(ordinary["needs_reauth"], true);
    assert_eq!(ordinary["tools"], Value::Null);
    assert_eq!(ordinary["tool_error"], Value::Null);
}

#[tokio::test]
async fn personal_dot_connection_inventory_uses_actual_catalogue_and_needs_no_user_account() {
    let fx = fixture().await;
    let dot = setup(&fx).await;
    let path = format!("/api/v0/dots/{dot}/connections");
    let (status, body) = fx.get(&fx.alice, &path).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let account = row(&body);
    assert_eq!(account["connector"]["title"], "Actual Workspace");
    assert_eq!(account["supported_modes"], json!(["user", "principal"]));
    assert_eq!(account["selected"]["mode"], "user");
    assert_eq!(account["selected"]["binding_generation"], Value::Null);
    assert_eq!(account["user_connection"]["connected"], false);
    assert_eq!(account["can_connect"], true);
    assert_eq!(fx.get(&fx.bob, &path).await.0, StatusCode::NOT_FOUND);
    assert_eq!(
        fx.send(None, Method::GET, &path, None).await.0,
        StatusCode::UNAUTHORIZED
    );
    aiplane_agents::db::agents::set_personal_dot_active(&fx.state.db, &dot, "alice", false, 4)
        .await
        .unwrap();
    assert_eq!(fx.get(&fx.alice, &path).await.0, StatusCode::OK);
    assert!(!body.to_string().contains("access_token"));
    assert!(!body.to_string().contains("refresh_token"));
    assert!(!body.to_string().contains("last_error"));
}

#[tokio::test]
async fn personal_dot_identity_selection_and_disconnect_preserve_the_user_account() {
    let fx = fixture().await;
    let dot = setup(&fx).await;
    let user = mcp_connections::upsert_connection(
        &fx.state.db,
        connection(&fx, ConnectionOwner::User("alice".into())).await,
    )
    .await
    .unwrap()
    .unwrap();
    let own = mcp_connections::upsert_connection(
        &fx.state.db,
        connection(&fx, ConnectionOwner::Principal(dot.clone())).await,
    )
    .await
    .unwrap()
    .unwrap();
    let path = format!("/api/v0/dots/{dot}/connections/workspace");
    let selection = json!({"mode":"principal","expected_binding_generation":null,"connection_id":own.connection_id,"connection_generation":own.generation});
    let (status, saved) = fx
        .send(Some(&fx.alice), Method::PUT, &path, Some(selection.clone()))
        .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    let selected = row(&saved)["selected"].clone();
    assert_eq!(selected["mode"], "principal");
    assert_eq!(selected["connection_generation"], own.generation);
    assert!(selected["binding_generation"].is_string());
    let mut same = selection.clone();
    same["expected_binding_generation"] = selected["binding_generation"].clone();
    let (status, unchanged) = fx
        .send(Some(&fx.alice), Method::PUT, &path, Some(same))
        .await;
    assert_eq!(status, StatusCode::OK, "{unchanged}");
    assert_eq!(row(&unchanged)["selected"], selected);
    assert_eq!(
        fx.send(Some(&fx.alice), Method::PUT, &path, Some(selection))
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        fx.send(
            Some(&fx.bob),
            Method::DELETE,
            &path,
            Some(json!({"connection_id":own.connection_id,"connection_generation":own.generation}))
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let stale = json!({"connection_id":own.connection_id,"connection_generation":"stale"});
    assert_eq!(
        fx.send(Some(&fx.alice), Method::DELETE, &path, Some(stale))
            .await
            .0,
        StatusCode::CONFLICT
    );
    let (status, removed) = fx
        .send(
            Some(&fx.alice),
            Method::DELETE,
            &path,
            Some(json!({"connection_id":own.connection_id,"connection_generation":own.generation})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{removed}");
    assert_eq!(row(&removed)["selected"]["mode"], "principal");
    assert_eq!(row(&removed)["principal_connection"], Value::Null);
    assert!(
        mcp_connections::get_connection(
            &fx.state.db,
            &ConnectionOwner::Principal(dot),
            "workspace"
        )
        .await
        .unwrap()
        .is_none()
    );
    assert_eq!(
        mcp_connections::get_connection(
            &fx.state.db,
            &ConnectionOwner::User("alice".into()),
            "workspace"
        )
        .await
        .unwrap()
        .unwrap()
        .identity(),
        user
    );
}

#[tokio::test]
async fn disabled_connector_hides_resource_metadata_but_allows_owned_local_disconnect() {
    let fx = fixture().await;
    let dot = setup(&fx).await;
    let own = mcp_connections::upsert_connection(
        &fx.state.db,
        connection(&fx, ConnectionOwner::Principal(dot.clone())).await,
    )
    .await
    .unwrap()
    .unwrap();
    let path = format!("/api/v0/dots/{dot}/connections/workspace");
    let (status, _) = fx
        .send(
            Some(&fx.alice),
            Method::PUT,
            &path,
            Some(json!({
                "mode":"principal", "expected_binding_generation":null,
                "connection_id":own.connection_id, "connection_generation":own.generation
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    aiplane_core::server::db::mcp_catalog::set_enabled(&fx.state.db, "workspace", false)
        .await
        .unwrap();
    let (status, inventory) = fx
        .get(&fx.alice, &format!("/api/v0/dots/{dot}/connections"))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(row(&inventory)["connector"], Value::Null);
    assert_eq!(row(&inventory)["supported_modes"], json!([]));
    assert_eq!(row(&inventory)["can_connect"], false);
    assert_eq!(
        row(&inventory)["principal_connection"]["id"],
        own.connection_id
    );
    assert_eq!(
        row(&inventory)["principal_connection"]["generation"],
        own.generation
    );
    assert_eq!(row(&inventory)["can_disconnect"], true);
    assert_eq!(row(&inventory)["selected"]["connection_id"], Value::Null);
    let (status, _) = fx
        .send(
            Some(&fx.alice),
            Method::DELETE,
            &path,
            Some(json!({
                "connection_id":own.connection_id, "connection_generation":own.generation
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        mcp_connections::get_connection(
            &fx.state.db,
            &ConnectionOwner::Principal(dot),
            "workspace"
        )
        .await
        .unwrap()
        .is_none()
    );
}

#[tokio::test]
async fn unselected_owned_credentials_remain_erasable_without_catalogue_access() {
    for restriction in ["disabled", "forbidden", "missing"] {
        let fx = fixture().await;
        let dot = setup(&fx).await;
        let user = mcp_connections::upsert_connection(
            &fx.state.db,
            connection(&fx, ConnectionOwner::User("alice".into())).await,
        )
        .await
        .unwrap()
        .unwrap();
        let own = mcp_connections::upsert_connection(
            &fx.state.db,
            connection(&fx, ConnectionOwner::Principal(dot.clone())).await,
        )
        .await
        .unwrap()
        .unwrap();
        match restriction {
            "disabled" => {
                aiplane_core::server::db::mcp_catalog::set_enabled(
                    &fx.state.db,
                    "workspace",
                    false,
                )
                .await
                .unwrap();
            }
            "forbidden" => {
                sqlx::query("UPDATE mcp_catalog_connectors SET allowed_groups='[\"other-group\"]' WHERE key='workspace'").execute(&fx.state.db).await.unwrap();
            }
            "missing" => {
                aiplane_core::server::db::mcp_catalog::delete(&fx.state.db, "workspace")
                    .await
                    .unwrap();
            }
            _ => unreachable!(),
        }
        aiplane_agents::db::agents::set_personal_dot_active(&fx.state.db, &dot, "alice", false, 4)
            .await
            .unwrap();
        let inventory_path = format!("/api/v0/dots/{dot}/connections");
        let (status, inventory) = fx.get(&fx.alice, &inventory_path).await;
        assert_eq!(status, StatusCode::OK, "{restriction}: {inventory}");
        let account = row(&inventory);
        assert_eq!(account["connector"], Value::Null);
        assert_eq!(account["supported_modes"], json!([]));
        assert_eq!(account["can_connect"], false);
        assert_eq!(account["can_disconnect"], true);
        assert_eq!(account["principal_connection"]["id"], own.connection_id);
        assert_eq!(
            account["principal_connection"]["generation"],
            own.generation
        );
        assert_eq!(account["user_connection"]["id"], Value::Null);
        assert_eq!(account["user_connection"]["generation"], Value::Null);
        assert_eq!(account["user_connection"]["connected"], false);
        assert_eq!(account["selected"]["binding_generation"], Value::Null);
        assert_eq!(account["selected"]["connection_id"], Value::Null);
        assert!(!inventory.to_string().contains("Actual Workspace"));
        let path = format!("{inventory_path}/workspace");
        let (status, erased) = fx
            .send(
                Some(&fx.alice),
                Method::DELETE,
                &path,
                Some(json!({
                    "connection_id":account["principal_connection"]["id"],
                    "connection_generation":account["principal_connection"]["generation"]
                })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{restriction}: {erased}");
        assert!(erased["connections"].as_array().unwrap().is_empty());
        assert_eq!(
            mcp_connections::get_connection(
                &fx.state.db,
                &ConnectionOwner::User("alice".into()),
                "workspace"
            )
            .await
            .unwrap()
            .unwrap()
            .identity(),
            user
        );
        assert!(
            mcp_connections::get_connection(
                &fx.state.db,
                &ConnectionOwner::Principal(dot),
                "workspace"
            )
            .await
            .unwrap()
            .is_none()
        );
    }
}

#[tokio::test]
async fn saved_unavailable_tools_preserve_rules_and_require_explicit_removal() {
    let fx = fixture().await;
    let dot = setup(&fx).await;
    let before = aiplane_agents::db::agents::get_personal_dot(&fx.state.db, "alice", &dot)
        .await
        .unwrap()
        .unwrap();
    let missing = "mcp__workspace__write_mail";
    let spec = json!({"main":{"tools":[missing],"tool_resources":{(missing):{"permission":"always_ask","actions":{"send":"deny"}}}}}).to_string();
    aiplane_agents::db::agents::update_personal_dot_draft(
        &fx.state.db,
        &dot,
        "alice",
        &before.agent.draft_spec,
        &spec,
    )
    .await
    .unwrap();
    let path = format!("/api/v0/dots/{dot}/access");
    let (status, inventory) = fx.get(&fx.alice, &path).await;
    assert_eq!(status, StatusCode::OK, "{inventory}");
    let tool = inventory["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == missing)
        .unwrap();
    assert_eq!(tool["selected"], true);
    assert_eq!(tool["available"], false);
    assert_eq!(tool["label"], Value::Null);
    assert_eq!(tool["description"], Value::Null);
    assert_eq!(tool["identity"], "unavailable");
    assert_eq!(tool["action_rules"], json!([{"id":"send","rule":"deny"}]));
    let (status, _) = fx.send(Some(&fx.alice),Method::PUT,&path,Some(json!({"revision":inventory["revision"],"tools":[{"id":missing,"permission":"always_ask","actions":[{"id":"send","rule":"deny"}]}]}))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        aiplane_agents::db::agents::get_personal_dot(&fx.state.db, "alice", &dot)
            .await
            .unwrap()
            .unwrap()
            .agent
            .draft_spec,
        spec
    );
    assert_eq!(
        fx.send(
            Some(&fx.alice),
            Method::PUT,
            &path,
            Some(json!({"revision":inventory["revision"],"tools":[]}))
        )
        .await
        .0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn all_connection_routes_require_a_session_and_hide_foreign_dot_ownership() {
    let fx = fixture().await;
    let dot = setup(&fx).await;
    for (method, suffix, body) in [
        (Method::GET, "", None),
        (
            Method::PUT,
            "/workspace",
            Some(
                json!({"mode":"principal","expected_binding_generation":null,"connection_id":"unknown","connection_generation":"unknown"}),
            ),
        ),
        (
            Method::DELETE,
            "/workspace",
            Some(json!({"connection_id":null,"connection_generation":null})),
        ),
        (
            Method::POST,
            "/workspace/connect",
            Some(json!({"connection_id":null,"connection_generation":null})),
        ),
    ] {
        let path = format!("/api/v0/dots/{dot}/connections{suffix}");
        assert_eq!(
            fx.send(None, method.clone(), &path, body.clone()).await.0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            fx.send(Some(&fx.bob), method, &path, body).await.0,
            StatusCode::NOT_FOUND
        );
    }
}

#[tokio::test]
async fn malformed_and_oversized_connection_versions_never_initialize_authorization() {
    let fx = fixture().await;
    let dot = setup(&fx).await;
    let path = format!("/api/v0/dots/{dot}/connections/workspace/connect");
    for body in [
        json!({"connection_id":"unknown","connection_generation":null}),
        json!({"connection_id":null,"connection_generation":null,"owner":"bob"}),
    ] {
        assert_eq!(
            fx.post(&fx.alice, &path, body).await.0,
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        fx.post(
            &fx.alice,
            &path,
            json!({"connection_id":"x".repeat(16*1024+1),"connection_generation":"unknown"})
        )
        .await
        .0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert!(
        mcp_connections::get_connection(
            &fx.state.db,
            &ConnectionOwner::Principal(dot),
            "workspace"
        )
        .await
        .unwrap()
        .is_none()
    );
}

#[tokio::test]
async fn personal_inventory_retains_owned_versions_after_current_management_permission_is_revoked()
{
    let fx = fixture().await;
    let dot = setup(&fx).await;
    let own = mcp_connections::upsert_connection(
        &fx.state.db,
        connection(&fx, ConnectionOwner::Principal(dot.clone())).await,
    )
    .await
    .unwrap()
    .unwrap();
    aiplane_core::server::db::gateway_groups::set_can_manage_dots(&fx.state.db, "managers", false)
        .await
        .unwrap();
    let base = format!("/api/v0/dots/{dot}/connections");
    let (status, inventory) = fx.get(&fx.alice, &base).await;
    assert_eq!(status, StatusCode::OK, "{inventory}");
    let current = row(&inventory);
    assert_eq!(current["can_select"], false);
    assert_eq!(current["can_connect"], false);
    assert_eq!(current["can_disconnect"], false);
    assert_eq!(current["principal_connection"]["id"], own.connection_id);
    assert_eq!(
        current["principal_connection"]["generation"],
        own.generation
    );
    assert!(current["user_connection"].is_object());
    for (method, suffix, body) in [
        (
            Method::PUT,
            "/workspace",
            json!({"mode":"principal","expected_binding_generation":null,
            "connection_id":own.connection_id,"connection_generation":own.generation}),
        ),
        (
            Method::POST,
            "/workspace/connect",
            json!({"connection_id":own.connection_id,"connection_generation":own.generation}),
        ),
        (
            Method::DELETE,
            "/workspace",
            json!({"connection_id":own.connection_id,"connection_generation":own.generation}),
        ),
    ] {
        let (status, error) = fx
            .send(
                Some(&fx.alice),
                method,
                &format!("{base}{suffix}"),
                Some(body),
            )
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{error}");
    }
    assert_eq!(
        mcp_connections::get_connection(&fx.state.db, &own.owner, "workspace")
            .await
            .unwrap()
            .unwrap()
            .identity(),
        own
    );
}
