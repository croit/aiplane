// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use super::agents::{Fx, fixture};
use crate::common;
use aiplane_core::server::db::{
    WriteTx, gateway_groups,
    mcp_connections::{self, ConnectionOwner},
};
use rama::{
    Service,
    http::{Body, Request, StatusCode, header},
};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

async fn setup(
    delay: Duration,
    reached: Option<Arc<tokio::sync::Notify>>,
) -> (Fx, MockServer, String) {
    let fx = fixture().await;
    let provider = MockServer::start().await;
    Mock::given(method("POST")).and(path("/token")).respond_with(move |_:&wiremock::Request| {
        if let Some(reached)=&reached {reached.notify_one();}
        ResponseTemplate::new(200).set_delay(delay).set_body_json(json!({"access_token":"provider-access","refresh_token":"provider-refresh","token_type":"Bearer","expires_in":3600,"scope":"mail.read"}))
    }).mount(&provider).await;
    let (status, created) = fx
        .post(
            &fx.alice,
            "/api/v0/agents",
            json!({"kind": "personal", "name":"Harald"}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let owned_agent = created["agent"]["id"].as_str().unwrap().to_owned();
    sqlx::query("INSERT INTO mcp_catalog_connectors(key,name,url,auth,scope,use_dcr,client_id,authorize_url,token_url,enabled,created_at,updated_at) VALUES('workspace','Actual Workspace',?,'oauth2','per_user',0,'client',?,?,1,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')")
        .bind(format!("{}/mcp",provider.uri())).bind(format!("{}/authorize",provider.uri())).bind(format!("{}/token",provider.uri())).execute(&fx.state.db).await.unwrap();
    (fx, provider, owned_agent)
}
async fn start(fx: &Fx, owned_agent: &str, version: Value) -> (StatusCode, Value) {
    fx.post(
        &fx.alice,
        &format!("/api/v0/agents/{owned_agent}/connections/workspace/connect"),
        version,
    )
    .await
}
async fn fresh_start(fx: &Fx, owned_agent: &str) -> String {
    let (status, body) = start(
        fx,
        owned_agent,
        json!({"connection_id":null,"connection_generation":null}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let url = reqwest::Url::parse(body["authorization_url"].as_str().unwrap()).unwrap();
    assert!(url.query_pairs().any(|(key, _)| key == "code_challenge"));
    url.query_pairs()
        .find(|(key, _)| key == "state")
        .unwrap()
        .1
        .into_owned()
}
async fn callback(
    state: aiplane::rama_server::RamaState,
    cookie: &str,
    query: &str,
) -> (StatusCode, Option<String>) {
    let request = Request::builder()
        .uri(format!("/integrations/callback?{query}"))
        .header(header::COOKIE, format!("id={cookie}"))
        .body(Body::empty())
        .unwrap();
    let response = common::app(state).serve(request).await.unwrap();
    let status = response.status();
    let location = response
        .headers()
        .get(header::LOCATION)
        .map(|value| value.to_str().unwrap().to_owned());
    common::read_body(response).await;
    (status, location)
}
fn own(owned_agent: &str) -> ConnectionOwner {
    ConnectionOwner::Principal(owned_agent.to_owned())
}

pub(crate) async fn change_catalogue_url(fx: &Fx, url: &str) -> String {
    let current = aiplane_core::server::db::mcp_catalog::get(&fx.state.db, "workspace")
        .await
        .unwrap()
        .unwrap();
    let (status, body) = fx.send(Some(&fx.root), rama::http::Method::PUT, "/api/v0/admin/connectors", Some(json!({
        "key":current.key,"title":current.name,"description":current.description.unwrap_or_default(),
        "icon":current.icon.unwrap_or_default(),"category":current.category.unwrap_or_default(),
        "base_url":url,"scope":current.scope.as_str(),"auth_type":current.auth.as_str(),
        "scopes":current.scopes,"client_id":current.client_id.unwrap_or_default(),
        "use_dcr":current.use_dcr,"authorize_url":current.authorize_url.unwrap_or_default(),
        "token_url":current.token_url.unwrap_or_default(),"registration_url":current.registration_url.unwrap_or_default(),
        "groups":current.allowed_groups,"audit":current.audit,"overwrite":true
    }))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    aiplane_core::server::db::mcp_catalog::get(&fx.state.db, "workspace")
        .await
        .unwrap()
        .unwrap()
        .dispatch_generation
}

#[tokio::test]
async fn agent_oauth_selected_tool_inventory_reports_actual_principal_identity() {
    let (fx, provider, owned_agent) = setup(Duration::ZERO, None).await;
    gateway_groups::set_tools_for_group(&fx.state.db, "managers", &["mcp__workspace".into()])
        .await
        .unwrap();
    fx.state.reload_rbac().await;
    Mock::given(method("POST"))
        .and(path("/mcp"))
        .respond_with(|request: &wiremock::Request| {
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            let result = match body["method"].as_str() {
                Some("initialize") => json!({"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"workspace","version":"1"}}),
                Some("tools/list") => json!({"tools":[{"name":"read_workspace","description":"Read the test workspace.","inputSchema":{"type":"object"},"annotations":{"readOnlyHint":true}}]}),
                _ => return ResponseTemplate::new(202),
            };
            ResponseTemplate::new(200).set_body_json(json!({"jsonrpc":"2.0","id":body["id"],"result":result}))
        })
        .mount(&provider)
        .await;
    let state = fresh_start(&fx, &owned_agent).await;
    assert_eq!(
        callback(
            fx.state.clone(),
            &fx.alice,
            &format!("state={state}&code=accepted")
        )
        .await
        .0,
        StatusCode::SEE_OTHER
    );
    let connection = mcp_connections::get_connection(&fx.state.db, &own(&owned_agent), "workspace")
        .await
        .unwrap()
        .unwrap();
    let (status, selected) = fx.send(Some(&fx.alice), rama::http::Method::PUT,
        &format!("/api/v0/agents/{owned_agent}/connections/workspace"),
        Some(json!({"mode":"principal","expected_binding_generation":null,"connection_id":connection.id,"connection_generation":connection.generation}))).await;
    assert_eq!(status, StatusCode::OK, "{selected}");
    let (status, access) = fx
        .get(&fx.alice, &format!("/api/v0/agents/{owned_agent}/access"))
        .await;
    assert_eq!(status, StatusCode::OK, "{access}");
    let tools: Vec<_> = access["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|tool| tool["connector_key"] == "workspace")
        .collect();
    assert_eq!(tools.len(), 1, "{access}");
    assert_eq!(tools[0]["available"], true);
    assert_eq!(tools[0]["identity"], "principal");
}

#[tokio::test]
async fn agent_oauth_catalogue_change_before_callback_prevents_token_exchange() {
    let (fx, provider, owned_agent) = setup(Duration::ZERO, None).await;
    let state = fresh_start(&fx, &owned_agent).await;
    change_catalogue_url(&fx, &format!("{}/changed-resource", provider.uri())).await;
    let (status, _) = callback(
        fx.state.clone(),
        &fx.alice,
        &format!("state={state}&code=accepted"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(provider.received_requests().await.unwrap().is_empty());
    assert!(
        mcp_connections::get_connection(&fx.state.db, &own(&owned_agent), "workspace")
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        mcp_connections::take_pending(&fx.state.db, &state)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn agent_oauth_catalogue_change_during_reauthorization_preserves_old_credentials() {
    let reached = Arc::new(tokio::sync::Notify::new());
    let (fx, provider, owned_agent) =
        setup(Duration::from_millis(300), Some(reached.clone())).await;
    let initial = fresh_start(&fx, &owned_agent).await;
    assert_eq!(
        callback(
            fx.state.clone(),
            &fx.alice,
            &format!("state={initial}&code=initial")
        )
        .await
        .0,
        StatusCode::SEE_OTHER
    );
    tokio::time::timeout(Duration::from_secs(3), reached.notified())
        .await
        .unwrap();
    let before = mcp_connections::get_connection(&fx.state.db, &own(&owned_agent), "workspace")
        .await
        .unwrap()
        .unwrap();
    let (status, body) = start(
        &fx,
        &owned_agent,
        json!({"connection_id":before.id,"connection_generation":before.generation}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let url = reqwest::Url::parse(body["authorization_url"].as_str().unwrap()).unwrap();
    let state = url
        .query_pairs()
        .find(|(key, _)| key == "state")
        .unwrap()
        .1
        .into_owned();
    let server_state = fx.state.clone();
    let cookie = fx.alice.clone();
    let worker = tokio::spawn(async move {
        callback(
            server_state,
            &cookie,
            &format!("state={state}&code=replacement"),
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(3), reached.notified())
        .await
        .unwrap();
    let replacement =
        change_catalogue_url(&fx, &format!("{}/changed-resource", provider.uri())).await;
    assert_eq!(worker.await.unwrap().0, StatusCode::FORBIDDEN);
    let after = mcp_connections::get_connection(&fx.state.db, &own(&owned_agent), "workspace")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.identity(), before.identity());
    assert_eq!(after.catalog_generation, before.catalog_generation);
    assert_ne!(
        after.catalog_generation.as_deref(),
        Some(replacement.as_str())
    );
    assert_eq!(after.access_token_ct, before.access_token_ct);
    assert_eq!(after.access_token_nonce, before.access_token_nonce);
    assert_eq!(after.refresh_token_ct, before.refresh_token_ct);
    assert_eq!(after.refresh_token_nonce, before.refresh_token_nonce);
    assert_eq!(after.status, before.status);
    assert!(after.authorization_generation.is_none());
    assert_eq!(provider.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn agent_oauth_catalogue_change_during_registration_never_reserves_credentials() {
    let (fx, provider, owned_agent) = setup(Duration::ZERO, None).await;
    let reached = Arc::new(tokio::sync::Notify::new());
    let registered = reached.clone();
    Mock::given(method("POST"))
        .and(path("/register"))
        .respond_with(move |_: &wiremock::Request| {
            registered.notify_one();
            ResponseTemplate::new(201)
                .set_delay(Duration::from_millis(300))
                .set_body_json(
                    json!({"client_id":"registered-fixture","client_secret":"registered-secret"}),
                )
        })
        .mount(&provider)
        .await;
    sqlx::query("UPDATE mcp_catalog_connectors SET client_id=NULL,use_dcr=1,registration_url=? WHERE key='workspace'").bind(format!("{}/register",provider.uri())).execute(&fx.state.db).await.unwrap();
    let server_state = fx.state.clone();
    let cookie = fx.alice.clone();
    let agent_for_worker = owned_agent.clone();
    let worker = tokio::spawn(async move {
        let app = common::app(server_state);
        let request = Request::builder()
            .method(rama::http::Method::POST)
            .uri(format!(
                "/api/v0/agents/{agent_for_worker}/connections/workspace/connect"
            ))
            .header(header::COOKIE, format!("id={cookie}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"connection_id":null,"connection_generation":null}).to_string(),
            ))
            .unwrap();
        let response = app.serve(request).await.unwrap();
        let status = response.status();
        common::read_body(response).await;
        status
    });
    tokio::time::timeout(Duration::from_secs(3), reached.notified())
        .await
        .unwrap();
    change_catalogue_url(&fx, &format!("{}/changed-resource", provider.uri())).await;
    assert_eq!(worker.await.unwrap(), StatusCode::CONFLICT);
    assert!(
        mcp_connections::get_connection(&fx.state.db, &own(&owned_agent), "workspace")
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM pending_mcp_oauth WHERE principal_id=?")
            .bind(&owned_agent)
            .fetch_one(&fx.state.db)
            .await
            .unwrap(),
        0
    );
    let requests = provider.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].url.path(), "/register");
}

#[tokio::test]
async fn agent_oauth_shared_callback_freezes_owner_and_preserves_explicit_selection() {
    let (fx, provider, owned_agent) = setup(Duration::ZERO, None).await;
    let state = fresh_start(&fx, &owned_agent).await;
    let reservation =
        mcp_connections::get_connection(&fx.state.db, &own(&owned_agent), "workspace")
            .await
            .unwrap()
            .unwrap();
    assert_eq!(reservation.status, mcp_connections::STATUS_PENDING);
    let (status, location) = callback(
        fx.state.clone(),
        &fx.alice,
        &format!("state={state}&code=accepted&owner=bob"),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        location.as_deref(),
        Some(format!("/agents/{owned_agent}?tab=tools").as_str())
    );
    let saved = mcp_connections::get_connection(&fx.state.db, &own(&owned_agent), "workspace")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.status, mcp_connections::STATUS_CONNECTED);
    assert_ne!(saved.generation, reservation.generation);
    assert!(saved.authorization_generation.is_none());
    assert!(saved.authorization_expires_at.is_none());
    assert_eq!(
        fx.state
            .crypto
            .open_str(
                saved.access_token_nonce.as_ref().unwrap(),
                saved.access_token_ct.as_ref().unwrap()
            )
            .unwrap(),
        "provider-access"
    );
    assert!(
        mcp_connections::get_connection(
            &fx.state.db,
            &ConnectionOwner::User("alice".into()),
            "workspace"
        )
        .await
        .unwrap()
        .is_none()
    );
    let (_, inventory) = fx
        .get(
            &fx.alice,
            &format!("/api/v0/agents/{owned_agent}/connections"),
        )
        .await;
    let row = inventory["connections"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["key"] == "workspace")
        .unwrap();
    assert_eq!(row["selected"]["mode"], "user");
    assert_eq!(provider.received_requests().await.unwrap().len(), 1);
    assert!(
        callback(
            fx.state.clone(),
            &fx.alice,
            &format!("state={state}&code=replay")
        )
        .await
        .0
        .is_server_error()
    );
    assert_eq!(provider.received_requests().await.unwrap().len(), 1);
}
#[tokio::test]
async fn agent_oauth_provider_error_consumes_and_cancels_only_its_reservation() {
    let (fx, provider, owned_agent) = setup(Duration::ZERO, None).await;
    let state = fresh_start(&fx, &owned_agent).await;
    let status = callback(
        fx.state.clone(),
        &fx.alice,
        &format!("state={state}&error=access_denied"),
    )
    .await
    .0;
    assert!(status.is_server_error());
    assert!(
        mcp_connections::get_connection(&fx.state.db, &own(&owned_agent), "workspace")
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        mcp_connections::take_pending(&fx.state.db, &state)
            .await
            .unwrap()
            .is_none()
    );
    assert!(provider.received_requests().await.unwrap().is_empty());
}
#[tokio::test]
async fn agent_oauth_expired_and_consumed_callbacks_never_exchange_or_save() {
    for expired in [false, true] {
        let (fx, provider, owned_agent) = setup(Duration::ZERO, None).await;
        let state = fresh_start(&fx, &owned_agent).await;
        if expired {
            let mut tx = WriteTx::begin(&fx.state.db).await.unwrap();
            sqlx::query(
                "UPDATE pending_mcp_oauth SET expires_at='2000-01-01T00:00:00Z' WHERE state=?",
            )
            .bind(&state)
            .execute(&mut *tx)
            .await
            .unwrap();
            sqlx::query("UPDATE mcp_connections SET authorization_expires_at='2000-01-01T00:00:00Z' WHERE principal_id=?").bind(&owned_agent).execute(&mut *tx).await.unwrap();
            tx.commit().await.unwrap();
        } else {
            mcp_connections::take_pending(&fx.state.db, &state)
                .await
                .unwrap()
                .unwrap();
        }
        assert!(
            callback(
                fx.state.clone(),
                &fx.alice,
                &format!("state={state}&code=accepted")
            )
            .await
            .0
            .is_server_error()
        );
        assert!(provider.received_requests().await.unwrap().is_empty());
        let connection =
            mcp_connections::get_connection(&fx.state.db, &own(&owned_agent), "workspace")
                .await
                .unwrap();
        assert!(
            connection.is_none_or(|row| row.status == mcp_connections::STATUS_PENDING
                && row.access_token_ct.is_none())
        );
    }
}
#[tokio::test]
async fn agent_oauth_frozen_start_rejects_stale_snapshot_and_revoked_current_rights() {
    let (fx, provider, owned_agent) = setup(Duration::ZERO, None).await;
    let state = fresh_start(&fx, &owned_agent).await;
    assert_eq!(
        start(
            &fx,
            &owned_agent,
            json!({"connection_id":null,"connection_generation":null})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let pending_state: String =
        sqlx::query_scalar("SELECT state FROM pending_mcp_oauth WHERE principal_id=?")
            .bind(&owned_agent)
            .fetch_one(&fx.state.db)
            .await
            .unwrap();
    assert_eq!(pending_state, state);
    gateway_groups::set_can_manage_personal_agents(&fx.state.db, "managers", false)
        .await
        .unwrap();
    fx.state.reload_rbac().await;
    assert_eq!(
        callback(
            fx.state.clone(),
            &fx.alice,
            &format!("state={state}&code=accepted")
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert!(
        mcp_connections::get_connection(&fx.state.db, &own(&owned_agent), "workspace")
            .await
            .unwrap()
            .is_none()
    );
    assert!(provider.received_requests().await.unwrap().is_empty());
}
#[tokio::test]
async fn agent_oauth_disconnect_during_exchange_cannot_recreate_credentials() {
    let reached = Arc::new(tokio::sync::Notify::new());
    // The fake provider delays its actual HTTP response to expose the network await.
    let (fx, _provider, owned_agent) =
        setup(Duration::from_millis(300), Some(reached.clone())).await;
    let state = fresh_start(&fx, &owned_agent).await;
    let server_state = fx.state.clone();
    let cookie = fx.alice.clone();
    let worker = tokio::spawn(async move {
        callback(
            server_state,
            &cookie,
            &format!("state={state}&code=accepted"),
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(3), reached.notified())
        .await
        .unwrap();
    let connection = mcp_connections::get_connection(&fx.state.db, &own(&owned_agent), "workspace")
        .await
        .unwrap()
        .unwrap();
    let (status,body)=fx.send(Some(&fx.alice),rama::http::Method::DELETE,&format!("/api/v0/agents/{owned_agent}/connections/workspace"),Some(json!({"connection_id":connection.id,"connection_generation":connection.generation}))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(worker.await.unwrap().0, StatusCode::FORBIDDEN);
    assert!(
        mcp_connections::get_connection(&fx.state.db, &own(&owned_agent), "workspace")
            .await
            .unwrap()
            .is_none()
    );
}
