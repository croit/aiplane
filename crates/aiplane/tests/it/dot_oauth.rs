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
        .post(&fx.alice, "/api/v0/dots", json!({"name":"Harald"}))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let dot = created["dot"]["id"].as_str().unwrap().to_owned();
    sqlx::query("INSERT INTO mcp_catalog_connectors(key,name,url,auth,scope,use_dcr,client_id,authorize_url,token_url,enabled,created_at,updated_at) VALUES('workspace','Actual Workspace',?,'oauth2','per_user',0,'client',?,?,1,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')")
        .bind(format!("{}/mcp",provider.uri())).bind(format!("{}/authorize",provider.uri())).bind(format!("{}/token",provider.uri())).execute(&fx.state.db).await.unwrap();
    (fx, provider, dot)
}
async fn start(fx: &Fx, dot: &str, version: Value) -> (StatusCode, Value) {
    fx.post(
        &fx.alice,
        &format!("/api/v0/dots/{dot}/connections/workspace/connect"),
        version,
    )
    .await
}
async fn fresh_start(fx: &Fx, dot: &str) -> String {
    let (status, body) = start(
        fx,
        dot,
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
fn own(dot: &str) -> ConnectionOwner {
    ConnectionOwner::Principal(dot.to_owned())
}

#[tokio::test]
async fn dot_oauth_shared_callback_freezes_owner_and_preserves_explicit_selection() {
    let (fx, provider, dot) = setup(Duration::ZERO, None).await;
    let state = fresh_start(&fx, &dot).await;
    let reservation = mcp_connections::get_connection(&fx.state.db, &own(&dot), "workspace")
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
        Some(format!("/dots?dot_id={dot}").as_str())
    );
    let saved = mcp_connections::get_connection(&fx.state.db, &own(&dot), "workspace")
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
        .get(&fx.alice, &format!("/api/v0/dots/{dot}/connections"))
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
async fn dot_oauth_provider_error_consumes_and_cancels_only_its_reservation() {
    let (fx, provider, dot) = setup(Duration::ZERO, None).await;
    let state = fresh_start(&fx, &dot).await;
    let status = callback(
        fx.state.clone(),
        &fx.alice,
        &format!("state={state}&error=access_denied"),
    )
    .await
    .0;
    assert!(status.is_server_error());
    assert!(
        mcp_connections::get_connection(&fx.state.db, &own(&dot), "workspace")
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
async fn dot_oauth_expired_and_consumed_callbacks_never_exchange_or_save() {
    for expired in [false, true] {
        let (fx, provider, dot) = setup(Duration::ZERO, None).await;
        let state = fresh_start(&fx, &dot).await;
        if expired {
            let mut tx = WriteTx::begin(&fx.state.db).await.unwrap();
            sqlx::query(
                "UPDATE pending_mcp_oauth SET expires_at='2000-01-01T00:00:00Z' WHERE state=?",
            )
            .bind(&state)
            .execute(&mut *tx)
            .await
            .unwrap();
            sqlx::query("UPDATE mcp_connections SET authorization_expires_at='2000-01-01T00:00:00Z' WHERE principal_id=?").bind(&dot).execute(&mut *tx).await.unwrap();
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
        let connection = mcp_connections::get_connection(&fx.state.db, &own(&dot), "workspace")
            .await
            .unwrap();
        assert!(
            connection.is_none_or(|row| row.status == mcp_connections::STATUS_PENDING
                && row.access_token_ct.is_none())
        );
    }
}
#[tokio::test]
async fn dot_oauth_frozen_start_rejects_stale_snapshot_and_revoked_current_rights() {
    let (fx, provider, dot) = setup(Duration::ZERO, None).await;
    let state = fresh_start(&fx, &dot).await;
    assert_eq!(
        start(
            &fx,
            &dot,
            json!({"connection_id":null,"connection_generation":null})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let pending_state: String =
        sqlx::query_scalar("SELECT state FROM pending_mcp_oauth WHERE principal_id=?")
            .bind(&dot)
            .fetch_one(&fx.state.db)
            .await
            .unwrap();
    assert_eq!(pending_state, state);
    gateway_groups::set_can_manage_dots(&fx.state.db, "managers", false)
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
        mcp_connections::get_connection(&fx.state.db, &own(&dot), "workspace")
            .await
            .unwrap()
            .is_none()
    );
    assert!(provider.received_requests().await.unwrap().is_empty());
}
#[tokio::test]
async fn dot_oauth_disconnect_during_exchange_cannot_recreate_credentials() {
    let reached = Arc::new(tokio::sync::Notify::new());
    // The fake provider delays its actual HTTP response to expose the network await.
    let (fx, _provider, dot) = setup(Duration::from_millis(300), Some(reached.clone())).await;
    let state = fresh_start(&fx, &dot).await;
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
    let connection = mcp_connections::get_connection(&fx.state.db, &own(&dot), "workspace")
        .await
        .unwrap()
        .unwrap();
    let (status,body)=fx.send(Some(&fx.alice),rama::http::Method::DELETE,&format!("/api/v0/dots/{dot}/connections/workspace"),Some(json!({"connection_id":connection.id,"connection_generation":connection.generation}))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(worker.await.unwrap().0, StatusCode::FORBIDDEN);
    assert!(
        mcp_connections::get_connection(&fx.state.db, &own(&dot), "workspace")
            .await
            .unwrap()
            .is_none()
    );
}
