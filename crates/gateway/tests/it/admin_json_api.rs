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
use wiremock::matchers::{header as header_matches, method};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn setup() -> (std::sync::Arc<gateway::rama_server::RamaState>, String) {
    let state = common::state_with_admin_rbac("http://unused.invalid").await;
    setup_state(state).await
}

async fn setup_state(
    state: gateway::rama_server::RamaState,
) -> (std::sync::Arc<gateway::rama_server::RamaState>, String) {
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
        (Method::GET, "/api/v0/admin/automatic-routes"),
        (Method::PUT, "/api/v0/admin/automatic-routes"),
        (Method::PUT, "/api/v0/admin/model-defaults"),
        (Method::PUT, "/api/v0/admin/search-settings"),
        (Method::GET, "/api/v0/admin/skills/example/archive"),
        (Method::GET, "/api/v0/admin/connectors/example/audit"),
        (Method::GET, "/api/v0/comfyui/catalog"),
        (Method::POST, "/api/v0/comfyui/reload"),
        (Method::GET, "/api/v0/comfyui/health"),
        (Method::GET, "/api/v0/admin/limits"),
        (Method::POST, "/api/v0/admin/limits"),
        (Method::GET, "/api/v0/admin/settings"),
        (Method::POST, "/api/v0/admin/settings"),
        (Method::GET, "/api/v0/admin/tokens"),
        (Method::GET, "/api/v0/admin/upstreams"),
        (Method::PUT, "/api/v0/admin/backends"),
        (Method::POST, "/api/v0/admin/backends/test"),
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

#[tokio::test]
async fn automatic_routes_round_trip_through_the_admin_api() {
    let (state, cookie) = setup().await;
    assert!(!state.automatic_router.is_route("default").await.unwrap());
    let app = common::app((*state).clone());
    let value = serde_json::json!({
        "alias": "default",
        "selector_model": "jev-model",
        "objective": "balanced",
        "instructions": "Prefer expert for difficult coding tasks.",
        "minimum_confidence": 0.7,
        "selector_timeout_ms": 1000,
        "fallback_target": "fast-model",
        "session_affinity": true,
        "session_ttl_seconds": 3600,
        "rollout": "shadow",
        "candidates": [
            {"key": "fast", "target": "fast-model", "description": "Fast model"},
            {"key": "expert", "target": "expert-model", "description": "Expert model"}
        ]
    });
    let saved = app
        .serve(req(
            Method::PUT,
            "/api/v0/admin/automatic-routes",
            &cookie,
            Some(value.to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(saved.status(), StatusCode::OK, "{}", body(saved).await);
    assert!(state.automatic_router.is_route("default").await.unwrap());

    let listed = app
        .serve(req(
            Method::GET,
            "/api/v0/admin/automatic-routes",
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(listed.status(), StatusCode::OK);
    let listed: serde_json::Value = serde_json::from_str(&body(listed).await).unwrap();
    assert_eq!(listed["routes"][0]["alias"], "default");
    assert_eq!(listed["routes"][0]["version"], 1);
    assert!(listed["candidate_models"].is_array());

    let deleted = app
        .serve(req(
            Method::DELETE,
            "/api/v0/admin/automatic-routes/default",
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
    assert!(!state.automatic_router.is_route("default").await.unwrap());
}

#[tokio::test]
async fn comfyui_catalog_preserves_operator_workflows_and_recent_jobs() {
    let state = common::state_with_admin_rbac_and_comfyui("http://unused.invalid").await;
    let handle = state.comfyui().unwrap();
    let workflow_dir = handle.store.dir().join("browser_fixture");
    std::fs::create_dir_all(&workflow_dir).unwrap();
    std::fs::write(
        workflow_dir.join("manifest.toml"),
        r#"id = "browser_fixture"
title = "Browser fixture"
description = "Produces a browser-test image."
output_kind = "image"
output_node_id = "9"
output_filename_prefix = "browser-fixture"

[[params]]
key = "prompt"
node_id = "6"
input_key = "text"
required = true
description = "What to draw."

[params.schema]
type = "string"
"#,
    )
    .unwrap();
    std::fs::write(
        workflow_dir.join("workflow.json"),
        r#"{"6":{"inputs":{"text":"{{prompt}}"}},"9":{"inputs":{}}}"#,
    )
    .unwrap();
    assert_eq!(handle.store.reload().total, 1);

    let job_id = gateway_features::server::comfyui::jobs::create(
        &state.db,
        "prompt-1",
        "session-1",
        "turn-1",
        "boss",
        "browser_fixture",
        "image",
        "9",
        "browser-fixture",
    )
    .await
    .unwrap();
    gateway_features::server::comfyui::jobs::complete(
        &state.db,
        job_id,
        "browser-fixture-1.png",
        "image/png",
    )
    .await
    .unwrap();

    let (state, cookie) = setup_state(state).await;
    let response = router(state)
        .serve(req(Method::GET, "/api/v0/comfyui/catalog", &cookie, None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response: serde_json::Value = serde_json::from_str(&body(response).await).unwrap();
    assert_eq!(response["timeout_secs"], 5);
    assert_eq!(response["queue_poll_interval_ms"], 10);
    assert_eq!(response["max_concurrent_jobs"], 1);
    assert_eq!(
        response["workflows"][0]["tool_id"],
        "comfyui_browser_fixture"
    );
    assert_eq!(response["workflows"][0]["output_node_id"], "9");
    assert_eq!(
        response["workflows"][0]["filename_prefix"],
        "browser-fixture"
    );
    assert_eq!(response["workflows"][0]["params"][0]["key"], "prompt");
    assert_eq!(response["jobs"][0]["status"], "completed");
    assert_eq!(
        response["jobs"][0]["output_filename"],
        "browser-fixture-1.png"
    );
}

#[tokio::test]
async fn comfyui_health_reports_an_unreachable_worker_as_a_200_verdict() {
    // The operator page asks this endpoint "is the worker up?". A down
    // worker must answer that question, not fail the request — otherwise the
    // page can only show a generic error and the admin is back to grepping
    // logs, which is the thing the probe exists to replace.
    // The helper points the ComfyUI client at `http://unused.invalid` —
    // a reserved TLD that can never resolve (RFC 6761), so the probe fails
    // to connect without the test depending on the network.
    let state = common::state_with_admin_rbac_and_comfyui("http://unused.invalid").await;
    let (state, cookie) = setup_state(state).await;
    let response = router(state)
        .serve(req(Method::GET, "/api/v0/comfyui/health", &cookie, None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response: serde_json::Value = serde_json::from_str(&body(response).await).unwrap();
    assert_eq!(response["reachable"], false);
    assert_eq!(response["base_url"], "http://unused.invalid");
    assert!(response["error"].is_string());
    assert!(response["worker"].is_null());
}

#[tokio::test]
async fn users_roster_includes_resolved_roles_impersonation_state_and_audit() {
    use gateway_core::server::db::{audit, users};

    let (state, cookie) = setup().await;
    let now = jiff::Timestamp::now();
    users::upsert(
        &state.db,
        &users::User {
            id: "member".into(),
            email: "member@example.com".into(),
            name: Some("Member User".into()),
            roles: vec!["engineering".into()],
            created_at: now,
            updated_at: now,
            timezone: None,
            speech_voice: None,
        },
    )
    .await
    .unwrap();
    audit::record(
        &state.db,
        "boss",
        "boss@example.com",
        "member",
        "member@example.com",
        audit::Action::Start,
    )
    .await
    .unwrap();

    let response = common::app((*state).clone())
        .serve(req(Method::GET, "/api/v0/admin/users", &cookie, None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let value: serde_json::Value = serde_json::from_str(&body(response).await).unwrap();
    assert_eq!(value["allow_impersonation"], true);
    assert_eq!(value["current_user_id"], "boss");
    let boss = value["users"]
        .as_array()
        .unwrap()
        .iter()
        .find(|user| user["id"] == "boss")
        .unwrap();
    assert_eq!(boss["oidc_groups"], serde_json::json!(["admin"]));
    assert_eq!(boss["gateway_roles"], serde_json::json!(["admin"]));
    assert_eq!(value["audit"][0]["action"], "start");
    assert_eq!(value["audit"][0]["actor_email"], "boss@example.com");
    assert_eq!(value["audit"][0]["target_email"], "member@example.com");
}

#[tokio::test]
async fn admin_tokens_include_dates_and_preserve_the_independent_operator_allowlist() {
    use gateway_core::server::db::tokens;

    let (state, cookie) = setup().await;
    let now = jiff::Timestamp::now();
    tokens::insert(
        &state.db,
        &tokens::Token {
            id: "token-one".into(),
            user_id: "boss".into(),
            name: "Build agent".into(),
            hash: "irrelevant-hash".into(),
            created_at: now,
            last_used_at: None,
            expires_at: now + jiff::Span::new().hours(30 * 24),
            revoked_at: None,
            tools_enabled: false,
        },
    )
    .await
    .unwrap();
    let app = common::app((*state).clone());

    let response = app
        .serve(req(Method::GET, "/api/v0/admin/tokens", &cookie, None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let listed: serde_json::Value = serde_json::from_str(&body(response).await).unwrap();
    assert_eq!(listed["timezone"], "UTC");
    assert!(listed["tokens"][0]["last_used_at"].is_null());
    assert_eq!(listed["tokens"][0]["admin_models"], serde_json::Value::Null);

    let response = app
        .serve(req(
            Method::PUT,
            "/api/v0/admin/tokens/token-one/models",
            &cookie,
            Some(serde_json::json!({ "restrict": true, "models": ["model-a"] }).to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .serve(req(Method::GET, "/api/v0/admin/tokens", &cookie, None))
        .await
        .unwrap();
    let listed: serde_json::Value = serde_json::from_str(&body(response).await).unwrap();
    assert_eq!(
        listed["tokens"][0]["admin_models"],
        serde_json::json!(["model-a"])
    );
}

#[tokio::test]
async fn backend_connection_test_uses_unsaved_fields_and_discovers_models() {
    let upstream = MockServer::start().await;
    Mock::given(method("GET"))
        .and(header_matches("authorization", "Bearer typed-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [
                {"id": "repo/model-b"},
                {"id": "repo/model-a"},
                {"id": "repo/model-b"}
            ]
        })))
        .mount(&upstream)
        .await;

    let (state, cookie) = setup().await;
    let response = router(state)
        .serve(req(
            Method::POST,
            "/api/v0/admin/backends/test",
            &cookie,
            Some(
                serde_json::json!({
                    "name": "not-saved",
                    "base_url": upstream.uri(),
                    "api_key": "typed-key",
                    "health_path": "/models"
                })
                .to_string(),
            ),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let tested: serde_json::Value = serde_json::from_str(&body(response).await).unwrap();
    assert_eq!(tested["outcome"], "success");
    assert_eq!(tested["code"], "ok");
    assert_eq!(tested["key_source"]["kind"], "typed");
    assert_eq!(
        tested["models"],
        serde_json::json!(["repo/model-a", "repo/model-b"])
    );
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
                r#"{"name":"vpn-users","description":"VPN folk","is_admin":false,"is_default":true,"oidc_values":["vpn","remote"],"tools":["search_web"],"skills":["incident-response"]}"#
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
    assert_eq!(group["description"], "VPN folk");
    assert_eq!(group["is_admin"], false);
    assert_eq!(group["is_default"], true);
    assert_eq!(group["oidc_values"], serde_json::json!(["remote", "vpn"]));
    assert_eq!(group["tools"], serde_json::json!(["search_web"]));
    assert_eq!(group["skills"], serde_json::json!(["incident-response"]));

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
            Some(r#"{"model_name":"m","input_price":"1.5","output_price":"2","pricing_unit":"tokens","context_window":"65536","reasoning_style":"qwen","budget_standard":"1024","cap_vision":"true","fallback_tools":"model-a","defaults_toml":"temperature = 0.4"}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let listed = app
        .serve(req(Method::GET, "/api/v0/admin/models", &cookie, None))
        .await
        .unwrap();
    assert_eq!(listed.status(), StatusCode::OK);
    let listed: serde_json::Value = serde_json::from_str(&body(listed).await).unwrap();
    let offered = listed["models"]
        .as_array()
        .unwrap()
        .iter()
        .find(|model| model["name"] == "model-a")
        .expect("offered model is represented");
    assert_eq!(offered["kind"], "chat");
    assert_eq!(offered["alias_target"], serde_json::Value::Null);
    assert_eq!(offered["resolved_reasoning_style"], "none");
    assert_eq!(offered["uses_token_budget"], false);
    assert_eq!(offered["effort_levels"], serde_json::json!([]));
    assert_eq!(listed["all_models"], serde_json::json!(["model-a"]));
    let chat_default = listed["feature_defaults"]
        .as_array()
        .unwrap()
        .iter()
        .find(|default| default["feature"] == "chat")
        .expect("chat default picker is represented");
    assert_eq!(chat_default["available"], serde_json::json!(["model-a"]));
    let configured = listed["models"]
        .as_array()
        .unwrap()
        .iter()
        .find(|model| model["name"] == "m")
        .expect("configured but unadvertised model remains editable");
    assert_eq!(configured["resolved_reasoning_style"], "qwen");
    assert_eq!(configured["uses_token_budget"], true);
    assert_eq!(configured["defaults"]["context_window"], 65536);
    assert_eq!(configured["defaults"]["budget_standard"], 1024);
    assert_eq!(configured["defaults"]["capabilities"]["vision"], true);
    assert_eq!(
        configured["defaults"]["capabilities"]["fallback_tools"],
        "model-a"
    );
    assert_eq!(configured["defaults"]["defaults_toml"], "temperature = 0.4");
}

#[tokio::test]
async fn feature_defaults_and_search_settings_round_trip() {
    let (state, cookie) = setup().await;
    let app = router(state);

    let default = app
        .serve(req(
            Method::PUT,
            "/api/v0/admin/model-defaults",
            &cookie,
            Some(r#"{"feature":"chat","model":"model-a"}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(default.status(), StatusCode::OK);

    let search = app
		.serve(req(
			Method::PUT,
			"/api/v0/admin/search-settings",
			&cookie,
			Some(r#"{"provider":"searxng","searxng_url":"https://search.example.test","brave_api_key":"","clear_brave_key":false}"#.into()),
		))
		.await
		.unwrap();
    assert_eq!(search.status(), StatusCode::OK);

    let listed = app
        .serve(req(Method::GET, "/api/v0/admin/models", &cookie, None))
        .await
        .unwrap();
    let listed: serde_json::Value = serde_json::from_str(&body(listed).await).unwrap();
    let chat = listed["feature_defaults"]
        .as_array()
        .unwrap()
        .iter()
        .find(|default| default["feature"] == "chat")
        .unwrap();
    assert_eq!(chat["model"], "model-a");
    assert_eq!(listed["search"]["provider"], "searxng");
    assert_eq!(
        listed["search"]["searxng_url"],
        "https://search.example.test"
    );
}

#[tokio::test]
async fn admin_skills_preserve_detail_download_and_effective_grants() {
    let temp = tempfile::tempdir().unwrap();
    let skill_dir = temp.path().join("release-helper");
    std::fs::create_dir_all(skill_dir.join("references")).unwrap();
    std::fs::write(
		skill_dir.join("SKILL.md"),
		"---\nname: release-helper\ntitle: Release Helper\ndescription: Prepares a release.\n---\n\n# Steps\n\nShip carefully.\n",
	)
	.unwrap();
    std::fs::write(skill_dir.join("references/example.md"), "Example").unwrap();
    let state =
        std::sync::Arc::new(common::state_with_user_skills(temp.path().to_path_buf()).await);
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
    for (name, is_admin) in [("admin", true), ("user", false)] {
        gateway_core::server::db::gateway_groups::upsert_group(
            &state.db, name, "", is_admin, false,
        )
        .await
        .unwrap();
    }
    gateway_core::server::db::gateway_groups::set_mappings_for_group(
        &state.db,
        "admin",
        &["admin".to_string()],
    )
    .await
    .unwrap();
    state.rbac.reload(
        gateway_core::server::db::gateway_groups::load_snapshot(&state.db)
            .await
            .unwrap(),
    );
    gateway_core::server::db::skill_grants::set_for_skill(&state.db, "*", &["admin".to_string()])
        .await
        .unwrap();
    gateway_core::server::db::skill_grants::set_for_skill(
        &state.db,
        "release-helper",
        &["user".to_string()],
    )
    .await
    .unwrap();
    state.rbac.set_skill_grant_overlay(
        gateway_core::server::db::skill_grants::all(&state.db)
            .await
            .unwrap(),
    );
    let app = common::app((*state).clone());

    let listed = app
        .serve(req(Method::GET, "/api/v0/admin/skills", &cookie, None))
        .await
        .unwrap();
    assert_eq!(listed.status(), StatusCode::OK);
    let listed: serde_json::Value = serde_json::from_str(&body(listed).await).unwrap();
    assert_eq!(listed["configured"], true);
    assert_eq!(listed["directory_accessible"], true);
    assert_eq!(listed["groups"], serde_json::json!(["admin", "user"]));
    let skill = &listed["skills"][0];
    assert_eq!(skill["name"], "release-helper");
    assert_eq!(skill["body"], "# Steps\n\nShip carefully.\n");
    assert_eq!(skill["files"], serde_json::json!(["references/example.md"]));
    assert_eq!(skill["all_skills_groups"], serde_json::json!(["admin"]));
    assert_eq!(skill["granted_groups"], serde_json::json!(["user"]));

    let archive = app
        .serve(req(
            Method::GET,
            "/api/v0/admin/skills/release-helper/archive",
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(archive.status(), StatusCode::OK);
    assert_eq!(archive.headers()[header::CONTENT_TYPE], "application/zip");
    assert_eq!(
        archive.headers()[header::CONTENT_DISPOSITION],
        "attachment; filename=\"release-helper.skill\""
    );

    let saved = app
        .serve(req(
            Method::PUT,
            "/api/v0/admin/skills/grants",
            &cookie,
            Some(
                serde_json::json!({
                    "skill": "release-helper",
                    "roles": ["admin", "user", "missing"]
                })
                .to_string(),
            ),
        ))
        .await
        .unwrap();
    assert_eq!(saved.status(), StatusCode::OK);
    assert_eq!(
        gateway_core::server::db::skill_grants::roles_for_skill(&state.db, "release-helper")
            .await
            .unwrap(),
        vec!["user".to_string()]
    );
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
    assert!(section["enabled"].is_boolean() || section["enabled"].is_null());
    assert!(section["fields"][0]["kind"].is_string());
    assert!(section["fields"][0]["span"].is_string());
    assert!(section["fields"][0]["models"].is_array());
    assert!(listed["needs_backend"].is_boolean());

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
            Some(r#"{"name":"p1","kind":"system_one","backends":["b1"],"models":["m"]}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = app
        .serve(req(
            Method::PUT,
            "/api/v0/admin/backends",
            &cookie,
            Some(
                r#"{"name":"b1","base_url":"http://upstream.invalid","pool":null,"overwrite":true}"#
                    .into(),
            ),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "null unassigns a backend");

    let resp = app
        .serve(req(
            Method::PUT,
            "/api/v0/admin/backends",
            &cookie,
            Some(
                r#"{"name":"b1","base_url":"http://upstream.invalid","pool":"p1","overwrite":true}"#
                    .into(),
            ),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "a backend can be reassigned");

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
    assert_eq!(listed["dirty"], serde_json::json!(4));
    assert!(listed["all_models"].is_array());
    assert!(listed["coverage"].is_object());
    assert!(
        listed["pending_changes"]
            .as_array()
            .is_some_and(|changes| changes.iter().any(|change| change["code"] == "pool_added"))
    );
    assert!(
        listed["backends"]
            .as_array()
            .unwrap()
            .iter()
            .any(|b| b["name"] == "b1" && b["has_stored_key"] == serde_json::json!(true))
    );
    assert_eq!(listed["pools"][0]["backends"], serde_json::json!(["b1"]));
    assert_eq!(listed["pools"][0]["kind"], "system_one");
    assert!(
        listed["pool_kinds"]
            .as_array()
            .is_some_and(|kinds| kinds.iter().any(|kind| kind == "system_one"))
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

    let listed: serde_json::Value = serde_json::from_str(
        &body(
            app.serve(req(Method::GET, "/api/v0/admin/upstreams", &cookie, None))
                .await
                .unwrap(),
        )
        .await,
    )
    .unwrap();
    assert_eq!(listed["pending_changes"], serde_json::json!([]));

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

    let resp = app
        .serve(req(
            rama::http::Method::POST,
            "/api/v0/scheduled",
            &cookie,
            Some(
                r#"{"name":"daily","prompt":"hi","model":"model-a","cron":"0 9 * * *","timezone":"Europe/Berlin","tools_enabled":true,"reuse_conversation":true,"reuse_rounds":7}"#
                    .into(),
            ),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let resp = app
        .serve(req(
            rama::http::Method::GET,
            "/api/v0/scheduled",
            &cookie,
            None,
        ))
        .await
        .unwrap();
    let scheduled: serde_json::Value = serde_json::from_str(&body(resp).await).unwrap();
    assert_eq!(scheduled["default_timezone"], "UTC");
    assert_eq!(scheduled["actions"][0]["timezone"], "Europe/Berlin");
    assert_eq!(
        scheduled["actions"][0]["schedule_summary"],
        "At 09:00, every day."
    );
    assert_eq!(scheduled["actions"][0]["tools_enabled"], true);
    assert_eq!(scheduled["actions"][0]["reuse_rounds"], 7);
    assert!(scheduled["actions"][0]["last_run_at"].is_null());
    assert!(scheduled["actions"][0]["last_error"].is_null());
    assert!(
        scheduled["models"].as_array().is_some_and(|models| {
            !models.is_empty()
                && models.iter().all(|model| {
                    model["id"].is_string()
                        && model["gdpr"].is_boolean()
                        && model["nda"].is_boolean()
                })
        }),
        "scheduled model choices expose compliance metadata: {scheduled}"
    );

    // Webhooks: create returns the one-time secret.
    let resp = app
        .serve(req(
            rama::http::Method::POST,
            "/api/v0/webhooks",
            &cookie,
            Some(
                r#"{"name":"ci","prompt":"summarise","model":"model-a","tools_enabled":true,"synchronous":true,"reuse_conversation":true,"reuse_rounds":9}"#.into(),
            ),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let hook: serde_json::Value = serde_json::from_str(&body(resp).await).unwrap();
    assert!(hook["secret"].as_str().unwrap().starts_with("gwh_"));
    assert_eq!(hook["webhook"]["tools_enabled"], true);
    assert_eq!(hook["webhook"]["synchronous"], true);
    assert_eq!(hook["webhook"]["reuse_conversation"], true);
    assert_eq!(hook["webhook"]["reuse_rounds"], 9);
    assert_eq!(hook["webhook"]["has_payload"], false);

    let listed = app
        .serve(req(
            rama::http::Method::GET,
            "/api/v0/webhooks",
            &cookie,
            None,
        ))
        .await
        .unwrap();
    let listed: serde_json::Value = serde_json::from_str(&body(listed).await).unwrap();
    assert_eq!(
        listed["webhooks"][0]["last_session_id"],
        serde_json::Value::Null
    );
    assert_eq!(listed["webhooks"][0]["last_error"], serde_json::Value::Null);
    assert!(
        listed["models"].as_array().is_some_and(|models| {
            !models.is_empty()
                && models.iter().all(|model| {
                    model["id"].is_string()
                        && model["gdpr"].is_boolean()
                        && model["nda"].is_boolean()
                })
        }),
        "webhook model choices expose compliance metadata: {listed}"
    );

    // The run history is the owner's alone. `webhooks::list_runs` is keyed
    // only by webhook id, so a handler that queried it straight from the path
    // would hand a stranger another account's prompts and replayed payloads.
    let hook_id = hook["webhook"]["id"]
        .as_str()
        .expect("create returns the webhook");
    let intruder = common::seed_session(&state, "mallory", "mallory@example.com").await;
    let resp = app
        .serve(req(
            rama::http::Method::GET,
            &format!("/api/v0/webhooks/{hook_id}/runs"),
            &intruder,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "another user's webhook must read as missing, not as a run history"
    );

    // The owner still gets theirs.
    gateway_runtime::server::webhooks::record_run_start(
        &state.db,
        hook_id,
        "session-1",
        "summarise",
        r#"{"event":"deploy"}"#,
        "fire",
    )
    .await
    .unwrap();
    let resp = app
        .serve(req(
            rama::http::Method::GET,
            &format!("/api/v0/webhooks/{hook_id}/runs"),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let runs: serde_json::Value = serde_json::from_str(&body(resp).await).unwrap();
    assert_eq!(runs["runs"][0]["payload"], r#"{"event":"deploy"}"#);
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
			Some(r#"{"key":"probe","title":"Probe","description":"Searches a private service.","icon":"🔎","category":"Search","base_url":"https://mcp.invalid/mcp","scope":"per_user","auth_type":"static_bearer","client_secret":"tok","scopes":["search.read"],"groups":[],"audit":true}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "{}", body(resp).await);

    let listed = app
        .serve(req(Method::GET, "/api/v0/admin/connectors", &cookie, None))
        .await
        .unwrap();
    let listed: serde_json::Value = serde_json::from_str(&body(listed).await).unwrap();
    assert!(listed["redirect_uri"].as_str().is_some());
    let connector = listed["connectors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|connector| connector["key"] == "probe")
        .unwrap();
    assert_eq!(connector["description"], "Searches a private service.");
    assert_eq!(connector["icon"], "🔎");
    assert_eq!(connector["category"], "Search");
    assert_eq!(connector["scope"], "per_user");
    assert_eq!(connector["audit"], true);
    assert_eq!(connector["has_secret"], true);
    assert_eq!(connector["needs_setup"], false);
    assert_eq!(connector["seeded"], false);

    let resp = app
		.serve(req(
			Method::PUT,
			"/api/v0/admin/connectors",
			&cookie,
			Some(r#"{"key":"oauth-probe","title":"OAuth Probe","base_url":"https://oauth.invalid/mcp","scope":"per_user","auth_type":"oauth2","client_json":"{\"web\":{\"client_id\":\"client-id\",\"client_secret\":\"client-secret\",\"auth_uri\":\"https://oauth.invalid/authorize\",\"token_uri\":\"https://oauth.invalid/token\"}}","registration_url":"https://oauth.invalid/register","scopes":["openid"],"use_dcr":false}"#.into()),
		))
		.await
		.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "{}", body(resp).await);
    let listed = app
        .serve(req(Method::GET, "/api/v0/admin/connectors", &cookie, None))
        .await
        .unwrap();
    let listed: serde_json::Value = serde_json::from_str(&body(listed).await).unwrap();
    let oauth = listed["connectors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|connector| connector["key"] == "oauth-probe")
        .unwrap();
    assert_eq!(oauth["client_id"], "client-id");
    assert_eq!(oauth["has_secret"], true);
    assert_eq!(oauth["authorize_url"], "https://oauth.invalid/authorize");
    assert_eq!(oauth["token_url"], "https://oauth.invalid/token");
    assert_eq!(oauth["registration_url"], "https://oauth.invalid/register");
    assert_eq!(oauth["use_dcr"], false);
    assert_eq!(oauth["needs_setup"], false);

    let resp = app
        .serve(req(
            Method::GET,
            "/integrations/callback?error=access_denied&error_description=Consent%20cancelled",
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        resp.headers()[rama::http::header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/html")
    );
    let error_page = body(resp).await;
    assert!(error_page.contains("That connection did not complete"));
    assert!(error_page.contains("access_denied"));
    assert!(error_page.contains("Consent cancelled"));
    assert!(error_page.contains("Back to the app"));

    gateway_core::server::db::mcp_audit::record(
        &state.db,
        "boss",
        "probe",
        "probe_search",
        Some(r#"{"query":"status"}"#),
        "ok",
        None,
        Some("session-1"),
    )
    .await
    .unwrap();
    let audit = app
        .serve(req(
            Method::GET,
            "/api/v0/admin/connectors/probe/audit",
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(audit.status(), StatusCode::OK);
    let audit: serde_json::Value = serde_json::from_str(&body(audit).await).unwrap();
    assert_eq!(audit["connector"]["title"], "Probe");
    assert_eq!(audit["events"][0]["tool_id"], "probe_search");
    assert_eq!(audit["events"][0]["user_email"], "boss@example.com");
    assert_eq!(audit["events"][0]["outcome"], "ok");

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

    // A connector is created disabled (`mcp_catalog::create` inserts
    // `enabled = 0`), and a disabled connector is not connectable — it is not
    // yet offered to anyone. The credential this endpoint seals is the whole
    // point of the gate.
    let resp = app
        .serve(req(
            rama::http::Method::POST,
            "/api/v0/integrations/probe/token",
            &pleb,
            Some(r#"{"token":"secret-token"}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "a disabled connector must not accept a user token"
    );

    // An empty token would seal fine and leave the connector permanently
    // "connected", sending `Authorization: Bearer ` on every call.
    let resp = app
        .serve(req(
            rama::http::Method::POST,
            "/api/v0/admin/connectors/probe/toggle",
            &cookie,
            Some(r#"{"enabled":true}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "{}", body(resp).await);

    let resp = app
        .serve(req(
            rama::http::Method::GET,
            "/api/v0/integrations",
            &pleb,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let integrations: serde_json::Value = serde_json::from_str(&body(resp).await).unwrap();
    let probe = &integrations["connectors"][0];
    assert_eq!(probe["key"], "probe");
    assert_eq!(probe["auth_type"], "static_bearer");
    assert_eq!(probe["is_global"], false);
    assert_eq!(probe["needs_setup"], false);
    assert_eq!(probe["connected"], false);
    assert_eq!(probe["errored"], false);
    assert_eq!(probe["needs_reauth"], false);
    assert!(probe["tools"].is_null());
    assert!(probe["tool_error"].is_null());

    let resp = app
        .serve(req(
            rama::http::Method::POST,
            "/api/v0/integrations/probe/token",
            &pleb,
            Some(r#"{"token":"   "}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "a blank token is not a credential"
    );

    // Static-token connect as the plain user, now that it is enabled.
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
            "/api/v0/integrations/probe/tools/mode",
            &pleb,
            Some(r#"{"tool":"probe_search","mode":"ask"}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::NO_CONTENT,
        "{}",
        body(resp).await
    );
    let modes = gateway_core::server::db::user_mcp::tool_modes(&state.db, "pleb2", "probe")
        .await
        .unwrap();
    assert_eq!(
        modes.get("probe_search"),
        Some(&gateway_core::server::db::user_mcp::ToolMode::Ask)
    );

    let resp = app
        .serve(req(
            rama::http::Method::POST,
            "/api/v0/integrations/probe/tools/all",
            &pleb,
            Some(r#"{"mode":"sometimes"}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let resp = app
        .serve(req(
            rama::http::Method::POST,
            "/api/v0/integrations/probe/retry",
            &pleb,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

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

#[tokio::test]
async fn private_skills_authoring_round_trip() {
    let temp = tempfile::tempdir().unwrap();
    let state =
        std::sync::Arc::new(common::state_with_user_skills(temp.path().to_path_buf()).await);
    let cookie = common::seed_session(&state, "writer", "writer@example.com").await;
    let app = common::app((*state).clone());
    let manifest = "---\nname: release-helper\ntitle: Release Helper\ndescription: Prepares a release.\n---\n\n# Steps\n\nShip carefully.\n";

    let resp = app
        .serve(req(
            Method::POST,
            "/api/v0/skills",
            &cookie,
            Some(serde_json::json!({ "name": "", "manifest": manifest }).to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED, "{}", body(resp).await);

    let resp = app
        .serve(req(Method::GET, "/api/v0/skills", &cookie, None))
        .await
        .unwrap();
    let listed: serde_json::Value = serde_json::from_str(&body(resp).await).unwrap();
    assert_eq!(listed["skills"][0]["name"], "release-helper");
    assert_eq!(listed["skills"][0]["title"], "Release Helper");

    let resp = app
        .serve(req(
            Method::GET,
            "/api/v0/skills/release-helper/body",
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let detail: serde_json::Value = serde_json::from_str(&body(resp).await).unwrap();
    assert_eq!(detail["body"], "# Steps\n\nShip carefully.\n");
    assert_eq!(detail["manifest"], manifest);

    let resp = app
        .serve(req(
            Method::DELETE,
            "/api/v0/skills/release-helper",
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
