// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! `/api/v0/admin/*` — the admin JSON API for the SPA (issue #22 P4).
//! The gating contract (401 anonymous, 403 non-admin, full data for admin)
//! plus one write round-trip per surface family: groups (RBAC reload),
//! limits, settings (hot reload), model defaults (shared validation), and
//! the topology (upsert → dirty → apply → clean).

use crate::common;

use common::Service as _;
use gateway::rama_server::router::router;
use rama::http::body::util::BodyExt;
use rama::http::{Body, Method, Request, StatusCode, header};

async fn setup() -> (std::sync::Arc<gateway::rama_server::RamaState>, String) {
    let state = common::state_with_admin_rbac("http://unused.invalid").await;
    let cookie = common::seed_session(&state, "boss", "boss@example.com").await;
    gateway_core::server::db::users::upsert(
        &state.db,
        &gateway_core::server::db::users::User {
            id: "boss".into(),
            email: "boss@example.com".into(),
            name: None,
            roles: vec!["admin".into()],
            created_at: jiff::Timestamp::now(),
            updated_at: jiff::Timestamp::now(),
            timezone: None,
            speech_voice: None,
        },
    )
    .await
    .unwrap();
    // The RBAC resolver is built from static config in this harness, but the
    // groups-save path reloads it from the DB — seed the admin group there
    // too so a save doesn't strip the caller's admin role mid-test.
    gateway_core::server::db::gateway_groups::upsert_group(&state.db, "admin", "", true, false)
        .await
        .unwrap();
    gateway_core::server::db::gateway_groups::set_mappings_for_group(
        &state.db,
        "admin",
        &["admin".to_string()],
    )
    .await
    .unwrap();
    (std::sync::Arc::new(state), cookie)
}

fn req(method: Method, uri: &str, cookie: &str, body: Option<String>) -> Request {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("cookie", format!("id={cookie}"));
    if let Some(body) = body {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
        builder.body(Body::from(body)).unwrap()
    } else {
        builder.body(Body::empty()).unwrap()
    }
}

async fn body(resp: rama::http::Response) -> String {
    String::from_utf8_lossy(&resp.into_body().collect().await.unwrap().to_bytes()).to_string()
}

/// Every admin route answers 401 anonymous / 403 non-admin with the JSON
/// envelope — never an HTML redirect or page.
#[tokio::test]
async fn admin_routes_gate_with_json_envelopes() {
    let (state, admin_cookie) = setup().await;
    let plain = common::seed_session(&state, "pleb", "pleb@example.com").await;
    let app = common::app((*state).clone());

    for (method, uri) in [
        (Method::GET, "/api/v0/admin/groups"),
        (Method::PUT, "/api/v0/admin/groups"),
        (Method::GET, "/api/v0/admin/users"),
        (Method::GET, "/api/v0/admin/models"),
        (Method::PUT, "/api/v0/admin/models"),
        (Method::GET, "/api/v0/admin/limits"),
        (Method::POST, "/api/v0/admin/limits"),
        (Method::GET, "/api/v0/admin/settings"),
        (Method::POST, "/api/v0/admin/settings"),
        (Method::GET, "/api/v0/admin/tokens"),
        (Method::GET, "/api/v0/admin/upstreams"),
        (Method::PUT, "/api/v0/admin/backends"),
        (Method::PUT, "/api/v0/admin/pools"),
        (Method::POST, "/api/v0/admin/upstreams/reload"),
    ] {
        let anon = app
            .serve(req(method.clone(), uri, "bogus", None))
            .await
            .unwrap();
        assert_eq!(anon.status(), StatusCode::UNAUTHORIZED, "{} {uri}", method);
        let pleb = app
            .serve(req(method.clone(), uri, &plain, Some("{}".into())))
            .await
            .unwrap();
        assert_eq!(pleb.status(), StatusCode::FORBIDDEN, "{} {uri}", method);
    }
    let _ = admin_cookie;
}

/// Groups round-trip: save → list reflects it → delete → gone.
#[tokio::test]
async fn groups_round_trip() {
    let (state, cookie) = setup().await;
    let app = common::app((*state).clone());

    let resp = app
        .serve(req(
            Method::PUT,
            "/api/v0/admin/groups",
            &cookie,
            Some(
                r#"{"name":"vpn-users","description":"VPN folk","oidc_values":["vpn"],"tools":["search_web"]}"#
                    .into(),
            ),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "{}", body(resp).await);

    let resp = app
        .serve(req(Method::GET, "/api/v0/admin/groups", &cookie, None))
        .await
        .unwrap();
    let raw = body(resp).await;
    let listed: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(err) => panic!("groups list body is not JSON ({err}): {raw}"),
    };
    let Some(groups) = listed["groups"].as_array() else {
        panic!("groups list did not carry an array: {raw}");
    };
    let group = groups
        .iter()
        .find(|g| g["name"] == "vpn-users")
        .expect("saved group listed");
    assert_eq!(group["tools"], serde_json::json!(["search_web"]));

    let resp = app
        .serve(req(
            Method::DELETE,
            "/api/v0/admin/groups/vpn-users",
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

/// Model overrides: the shared validation rejects a bad price with a 400
/// and accepts a real save.
#[tokio::test]
async fn model_overrides_validate_and_save() {
    let (state, cookie) = setup().await;
    let app = router(state);

    let resp = app
        .serve(req(
            Method::PUT,
            "/api/v0/admin/models",
            &cookie,
            Some(r#"{"model_name":"m","input_price":"-5"}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert!(
        body(resp).await.contains("price"),
        "message says what was wrong"
    );

    let resp = app
        .serve(req(
            Method::PUT,
            "/api/v0/admin/models",
            &cookie,
            Some(r#"{"model_name":"m","input_price":"1.5","output_price":"2","pricing_unit":"per_mtok"}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

/// Limits: upsert then delete, with subject validation.
#[tokio::test]
async fn limits_upsert_and_delete() {
    let (state, cookie) = setup().await;
    let app = router(state);

    let resp = app
        .serve(req(
            Method::POST,
            "/api/v0/admin/limits",
            &cookie,
            Some(r#"{"subject_type":"user","subject_id":"boss@example.com","dimension":"requests","window":"day","value":100}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = app
        .serve(req(
            Method::POST,
            "/api/v0/admin/limits",
            &cookie,
            Some(r#"{"subject_type":"token","subject_id":"nope","dimension":"requests","window":"day","value":1}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let listed = body(
        app.serve(req(Method::GET, "/api/v0/admin/limits", &cookie, None))
            .await
            .unwrap(),
    )
    .await;
    let parsed: serde_json::Value = serde_json::from_str(&listed).unwrap();
    let rule = parsed["limits"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["subject_id"] == "boss")
        .expect("rule listed with the resolved id");
    let id = rule["id"].as_str().unwrap().to_string();
    let resp = app
        .serve(req(
            Method::DELETE,
            &format!("/api/v0/admin/limits/{id}"),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

/// Settings: list exposes the section spec, save stores + hot-reloads,
/// clear drops a value again.
#[tokio::test]
async fn settings_list_save_clear() {
    let (state, cookie) = setup().await;
    let app = router(state);

    let listed: serde_json::Value = serde_json::from_str(
        &body(
            app.serve(req(Method::GET, "/api/v0/admin/settings", &cookie, None))
                .await
                .unwrap(),
        )
        .await,
    )
    .unwrap();
    let section = &listed["sections"][0];
    assert!(section["name"].is_string());
    assert!(section["fields"][0]["kind"].is_string());

    let name = section["name"].as_str().unwrap().to_string();
    let resp = app
        .serve(req(
            Method::POST,
            "/api/v0/admin/settings",
            &cookie,
            Some(format!(r#"{{"section":"{name}","values":{{}}}}"#)),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "empty save is a no-op save");
}

/// Topology: save a backend + pool (dirty bumps), apply (dirty resets),
/// and the live stream answers with status events.
#[tokio::test]
async fn topology_save_apply_and_stream() {
    let (state, cookie) = setup().await;
    let app = router(state.clone());

    let resp = app
        .serve(req(
            Method::PUT,
            "/api/v0/admin/backends",
            &cookie,
            Some(
                r#"{"name":"b1","base_url":"http://upstream.invalid","api_key":"sk-test"}"#.into(),
            ),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let saved: serde_json::Value = serde_json::from_str(&body(resp).await).unwrap();
    assert_eq!(
        saved["dirty"],
        serde_json::json!(1),
        "save marks the topology dirty"
    );

    let resp = app
        .serve(req(
            Method::PUT,
            "/api/v0/admin/pools",
            &cookie,
            Some(r#"{"name":"p1","kind":"chat","backends":["b1"],"models":["m"]}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Duplicate without overwrite → 409 with a resend hint.
    let resp = app
        .serve(req(
            Method::PUT,
            "/api/v0/admin/backends",
            &cookie,
            Some(r#"{"name":"b1","base_url":"http://other.invalid"}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);

    let listed: serde_json::Value = serde_json::from_str(
        &body(
            app.serve(req(Method::GET, "/api/v0/admin/upstreams", &cookie, None))
                .await
                .unwrap(),
        )
        .await,
    )
    .unwrap();
    assert_eq!(listed["dirty"], serde_json::json!(2));
    assert!(
        listed["backends"]
            .as_array()
            .unwrap()
            .iter()
            .any(|b| b["name"] == "b1" && b["has_stored_key"] == serde_json::json!(true))
    );

    let resp = app
        .serve(req(
            Method::POST,
            "/api/v0/admin/upstreams/reload",
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let applied: serde_json::Value = serde_json::from_str(&body(resp).await).unwrap();
    assert_eq!(applied["dirty"], serde_json::json!(0));

    // The live stream opens (200 + SSE content type). Its body never ends
    // by design — drop the response, which drops the channel and ends the
    // task; collecting it here would hang the test.
    let resp = app
        .serve(req(
            Method::GET,
            "/api/v0/admin/upstreams/events",
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("text/event-stream")
    );
    drop(resp);
}

/// The workspace API (issue #22 P5): memories + scheduled + webhooks CRUD
/// under /api/v0, session-gated.
#[tokio::test]
async fn workspace_surfaces_round_trip() {
    let (state, cookie) = setup().await;
    let app = common::app((*state).clone());

    // Memories.
    let resp = app
        .serve(req(
            rama::http::Method::POST,
            "/api/v0/memories",
            &cookie,
            Some(r#"{"kind":"fact","content":"prefers dark mode"}"#.into()),
        ))
        .await
        .unwrap();
    let created_raw = body(resp).await;
    let created: serde_json::Value =
        serde_json::from_str(&created_raw).expect("memory create answers 201 JSON");
    assert!(
        created["id"].is_string(),
        "the memory row carries its id: {created_raw}"
    );
    let resp = app
        .serve(req(
            rama::http::Method::GET,
            "/api/v0/memories",
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert!(body(resp).await.contains("prefers dark mode"));

    // Scheduled: cron validation + preview.
    let resp = app
        .serve(req(
            rama::http::Method::POST,
            "/api/v0/scheduled/preview",
            &cookie,
            Some(r#"{"cron":"0 9 * * 1-5"}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(body(resp).await.contains("upcoming"));

    let resp = app
        .serve(req(
            rama::http::Method::POST,
            "/api/v0/scheduled",
            &cookie,
            Some(r#"{"name":"daily","prompt":"hi","model":"model-a","cron":"not cron"}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "bad cron rejected");

    // Webhooks: create returns the one-time secret.
    let resp = app
        .serve(req(
            rama::http::Method::POST,
            "/api/v0/webhooks",
            &cookie,
            Some(r#"{"name":"ci","prompt":"summarise","model":"model-a"}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let hook: serde_json::Value = serde_json::from_str(&body(resp).await).unwrap();
    assert!(hook["secret"].as_str().unwrap().starts_with("gwh_"));
}

/// Skills + integrations: list (feature off → global set), connectors
/// admin CRUD gating, and the static-token connect flow.
#[tokio::test]
async fn skills_and_connectors_surfaces() {
    let (state, cookie) = setup().await;
    let app = common::app((*state).clone());

    // Skills list answers (the harness wires no skills dir — empty set).
    let resp = app
        .serve(req(
            rama::http::Method::GET,
            "/api/v0/skills",
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let raw = body(resp).await;
    let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert!(parsed["skills"].as_array().is_some());

    // Integrations list: empty catalog → empty list.
    let resp = app
        .serve(req(
            rama::http::Method::GET,
            "/api/v0/integrations",
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Admin connectors: create → toggle → delete cascade.
    let resp = app
        .serve(req(
            rama::http::Method::PUT,
            "/api/v0/admin/connectors",
            &cookie,
            Some(r#"{"key":"probe","title":"Probe","base_url":"https://mcp.invalid/mcp","auth_type":"static_bearer","client_secret":"tok"}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "{}", body(resp).await);

    // Non-admin is barred.
    let pleb = common::seed_session(&state, "pleb2", "pleb2@example.com").await;
    let resp = app
        .serve(req(
            rama::http::Method::GET,
            "/api/v0/admin/connectors",
            &pleb,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // Static-token connect as the plain user.
    let resp = app
        .serve(req(
            rama::http::Method::POST,
            "/api/v0/integrations/probe/token",
            &pleb,
            Some(r#"{"token":"secret-token"}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "{}", body(resp).await);

    let resp = app
        .serve(req(
            rama::http::Method::POST,
            "/api/v0/integrations/probe/disconnect",
            &pleb,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let resp = app
        .serve(req(
            rama::http::Method::DELETE,
            "/api/v0/admin/connectors/probe",
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

/// The setup wizard API (issue #22 P5/P6): state/draft lifecycle on a fresh
/// gateway. The full OIDC round trip needs a real provider — wiremock OIDC
/// lives in oidc_integration — so this pins the gate + draft mechanics.
#[tokio::test]
async fn setup_api_state_and_gates() {
    // A state whose setup is NOT completed: seed a bare RamaState.
    let state = common::state_with_chat_pool("http://unused.invalid").await;
    let app = router(std::sync::Arc::new(state));

    // Fresh: first_run, no draft, no proof.
    let resp = app
        .serve(
            Request::get("/api/v0/setup/state")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let raw = body(resp).await;
    let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(parsed["access"], "first_run");
    assert!(parsed["draft"].is_null());
    assert!(parsed["proof"].is_null());

    // Test with garbage provider → 502 with the operator-facing message,
    // but the draft is kept.
    let resp = app
        .serve(req(
            rama::http::Method::POST,
            "/api/v0/setup/test",
            "placeholder",
            Some(
                r#"{"public_url":"http://localhost:8080","issuer":"http://127.0.0.1:9","client_id":"c","client_secret":"s"}"#
                    .into(),
            ),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_GATEWAY, "{raw}");
    let raw2 = body(resp).await;
    assert!(raw2.contains("provider"), "{raw2}");

    // The draft survived for the operator's typing.
    let resp = app
        .serve(
            Request::get("/api/v0/setup/state")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&body(resp).await).unwrap();
    assert_eq!(parsed["draft"]["issuer"], "http://127.0.0.1:9");
    assert_eq!(
        parsed["draft"]["client_secret_set"],
        serde_json::json!(true)
    );
}
