// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! /api/v0/rag/* — session-authed RAG admin endpoints.
//!
//! Exercises the CRUD + reindex surface against the in-memory test
//! state. The indexer worker itself is `None` in this state (no
//! upstreams pool is wired here), but the API doesn't depend on it —
//! everything it does runs through the rag DB tables.

use crate::common;

use common::Service as _;
use rama::http::{Body, Method, Request, StatusCode};
use serde_json::{Value, json};

fn req_with_cookie(method: Method, uri: &str, cookie: &str, body: Option<&str>) -> Request {
    let mut b = Request::builder()
        .method(method)
        .uri(uri)
        .header("cookie", format!("id={cookie}"));
    if body.is_some() {
        b = b.header("content-type", "application/json");
    }
    b.body(Body::from(body.unwrap_or("").to_string())).unwrap()
}

fn create_body() -> &'static str {
    r#"{
        "name": "gateway-repo",
        "description": "the gateway codebase",
        "git_url": "https://example.invalid/gateway.git",
        "git_ref": "main",
        "embedding_model": "embed-1",
        "include_globs": ["*.rs"],
        "exclude_globs": ["target/"],
        "chunk_size": 512,
        "chunk_overlap": 64
    }"#
}

/// Seed a session whose user carries the `"admin"` role so the admin
/// gate on `/api/v0/rag/*` lets it through. Mirrors `seed_admin` in
/// `admin_models.rs`. Must be paired with `state_with_admin_rbac`, whose
/// resolver maps the `"admin"` OIDC value to an admin-flagged role.
async fn seed_admin(state: &aiplane::rama_server::RamaState, user_id: &str) -> String {
    use aiplane_core::server::db::users;
    use jiff::Timestamp;
    let cookie = common::seed_session(state, user_id, &format!("{user_id}@example.com")).await;
    let now = Timestamp::now();
    users::upsert(
        &state.db,
        &users::User {
            id: user_id.into(),
            email: format!("{user_id}@example.com"),
            name: None,
            roles: vec!["admin".into()],
            created_at: now,
            updated_at: now,
            timezone: None,
            speech_voice: None,
        },
    )
    .await
    .unwrap();
    cookie
}

#[tokio::test]
async fn list_collections_anonymous_is_401() {
    let state = common::state_with_chat_pool("http://unused.invalid").await;
    let app = common::app(state);
    let resp = app
        .serve(common::req(Method::GET, "/api/v0/rag/collections"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn non_admin_is_forbidden_on_every_endpoint() {
    // The RAG registry is operator-global and admin-only. A signed-in
    // but non-admin user must be blocked from the whole API. Regression
    // guard for the missing authorization gate (previously any active
    // session passed, letting any user read/mutate/delete collections).
    // The gate runs before any DB lookup, so 403 (not 404) is expected
    // even for a non-existent id.
    let state = common::state_with_admin_rbac("http://unused.invalid").await;
    let cookie = common::seed_session(&state, "mallory", "mallory@example.com").await;
    let app = common::app(state);

    let cases: [(Method, &str, Option<&str>); 6] = [
        (Method::GET, "/api/v0/rag/collections", None),
        (Method::POST, "/api/v0/rag/collections", Some(create_body())),
        (Method::GET, "/api/v0/rag/collections/1", None),
        (
            Method::PATCH,
            "/api/v0/rag/collections/1",
            Some(r#"{"git_ref":"evil"}"#),
        ),
        (Method::POST, "/api/v0/rag/collections/1/reindex", None),
        (Method::DELETE, "/api/v0/rag/collections/1", None),
    ];
    for (method, uri, body) in cases {
        let resp = app
            .serve(req_with_cookie(method.clone(), uri, &cookie, body))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "non-admin must get 403 on {method} {uri}"
        );
    }
}

#[tokio::test]
async fn full_create_get_list_update_reindex_delete_round_trip() {
    let state = common::state_with_admin_rbac("http://unused.invalid").await;
    let cookie = seed_admin(&state, "alice").await;
    let db = state.db.clone();
    let app = common::app(state);

    // Empty list to start.
    let resp = app
        .serve(req_with_cookie(
            Method::GET,
            "/api/v0/rag/collections",
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let parsed: Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(parsed["data"].as_array().unwrap().len(), 0);

    // Create.
    let resp = app
        .serve(req_with_cookie(
            Method::POST,
            "/api/v0/rag/collections",
            &cookie,
            Some(create_body()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let created: Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(created["name"], "gateway-repo");
    assert_eq!(created["pat_set"], false);
    assert_eq!(created["status"], "unconfigured");
    let id = created["id"].as_i64().unwrap();

    // Get one.
    let resp = app
        .serve(req_with_cookie(
            Method::GET,
            &format!("/api/v0/rag/collections/{id}"),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let fetched: Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(fetched["id"], id);
    assert_eq!(fetched["description"], "the gateway codebase");
    assert_eq!(fetched["include_globs"], json!(["*.rs"]));

    // Update: set a PAT + tweak embedding_model.
    let resp = app
        .serve(req_with_cookie(
            Method::PATCH,
            &format!("/api/v0/rag/collections/{id}"),
            &cookie,
            Some(r#"{"pat": "ghp_secretvalue", "embedding_model": "embed-2"}"#),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let updated: Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(updated["pat_set"], true, "PAT must read as set");
    assert!(
        updated.get("pat").is_none(),
        "raw PAT must NEVER appear in the response shape"
    );
    assert_eq!(updated["embedding_model"], "embed-2");

    // Add a source and move its lifecycle to error. Collection status must
    // follow the searchable source, never a separate collection lifecycle.
    {
        use aiplane_core::server::db::rag as rag_db;
        let source = rag_db::add_ref(&db, id, "main", None, true).await.unwrap();
        rag_db::set_ref_status(&db, source.id, rag_db::CollectionStatus::Error)
            .await
            .unwrap();
    }
    let resp = app
        .serve(req_with_cookie(
            Method::POST,
            &format!("/api/v0/rag/collections/{id}/reindex"),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let after: Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(after["status"], "pending");

    // List should now have one entry.
    let resp = app
        .serve(req_with_cookie(
            Method::GET,
            "/api/v0/rag/collections",
            &cookie,
            None,
        ))
        .await
        .unwrap();
    let listed: Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(listed["data"].as_array().unwrap().len(), 1);

    // Delete.
    let resp = app
        .serve(req_with_cookie(
            Method::DELETE,
            &format!("/api/v0/rag/collections/{id}"),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Get on deleted id → 404.
    let resp = app
        .serve(req_with_cookie(
            Method::GET,
            &format!("/api/v0/rag/collections/{id}"),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn collection_status_is_derived_from_its_search_ref() {
    use aiplane_core::server::db::rag as rag_db;

    let state = common::state_with_admin_rbac("http://unused.invalid").await;
    let cookie = seed_admin(&state, "alice").await;
    let db = state.db.clone();
    let app = common::app(state);

    let resp = app
        .serve(req_with_cookie(
            Method::POST,
            "/api/v0/rag/collections",
            &cookie,
            Some(create_body()),
        ))
        .await
        .unwrap();
    let created: Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    let id = created["id"].as_i64().unwrap();
    assert_eq!(created["status"], "unconfigured");

    let source = rag_db::add_ref(&db, id, "main", None, true).await.unwrap();
    rag_db::set_ref_status(&db, source.id, rag_db::CollectionStatus::Ready)
        .await
        .unwrap();

    let resp = app
        .serve(req_with_cookie(
            Method::GET,
            &format!("/api/v0/rag/collections/{id}"),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let collection: Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(collection["status"], "ready");
}

#[tokio::test]
async fn providers_endpoint_describes_each_source_and_its_fields() {
    let state = common::state_with_admin_rbac("http://unused.invalid").await;
    let cookie = seed_admin(&state, "alice").await;
    let app = common::app(state);

    let resp = app
        .serve(req_with_cookie(
            Method::GET,
            "/api/v0/rag/providers",
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let parsed: Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert!(parsed["embedding_models"].is_array());
    assert!(parsed.get("default_embedding").is_some());
    let kinds: Vec<&str> = parsed["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["kind"].as_str().unwrap())
        .collect();
    assert!(kinds.contains(&"git"));
    assert!(kinds.contains(&"webdav"));

    // A client with no compiled-in knowledge of the provider can build a
    // form from this: which fields exist, which are required, which are
    // secret. That is what makes the API extensible rather than just the UI.
    let dav = parsed["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["kind"] == "webdav")
        .expect("webdav is described");
    let fields = dav["fields"].as_array().unwrap();
    let password = fields
        .iter()
        .find(|f| f["key"] == "password")
        .expect("the credential field is described");
    assert_eq!(password["secret"], true);
    assert_eq!(password["required"], true);
    let base_url = fields.iter().find(|f| f["key"] == "base_url").unwrap();
    assert_eq!(base_url["secret"], false);
}

#[tokio::test]
async fn creating_a_remote_source_seals_its_secret_and_needs_no_git_url() {
    let state = common::state_with_admin_rbac("http://unused.invalid").await;
    let cookie = seed_admin(&state, "alice").await;
    let db = state.db.clone();
    let app = common::app(state);

    let body = r#"{
        "name": "docs",
        "git_url": "",
        "embedding_model": "embed-1",
        "source_kind": "webdav",
        "source_config": {
            "base_url": "https://cloud.example.com",
            "username": "svc",
            "password": "app-pw",
            "root": "Finance"
        }
    }"#;
    let resp = app
        .serve(req_with_cookie(
            Method::POST,
            "/api/v0/rag/collections",
            &cookie,
            Some(body),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let created: Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(created["source_kind"], "webdav");
    assert_eq!(created["source_config"]["root"], "Finance");
    assert_eq!(created["source_secrets_set"], true);
    assert!(
        created["source_config"].get("password").is_none(),
        "a secret must never come back in the config map: {created}"
    );
    assert!(
        !serde_json::to_string(&created).unwrap().contains("app-pw"),
        "the plaintext credential must never be serialised to a client"
    );

    // And it really is sealed on disk, not merely hidden from the view.
    let id = created["id"].as_i64().unwrap();
    let stored = aiplane_core::server::db::rag::find_collection_by_id(&db, id)
        .await
        .unwrap()
        .unwrap();
    assert!(stored.source.secrets.is_some());
    assert!(!stored.source.config.contains_key("password"));
}

#[tokio::test]
async fn creating_a_remote_source_with_a_bad_setting_is_a_400_not_a_broken_collection() {
    let state = common::state_with_admin_rbac("http://unused.invalid").await;
    let cookie = seed_admin(&state, "alice").await;
    let app = common::app(state);

    // No password: the provider declares it required.
    let body = r#"{
        "name": "docs",
        "git_url": "",
        "embedding_model": "embed-1",
        "source_kind": "webdav",
        "source_config": {"base_url": "https://cloud.example.com", "username": "svc"}
    }"#;
    let resp = app
        .serve(req_with_cookie(
            Method::POST,
            "/api/v0/rag/collections",
            &cookie,
            Some(body),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let parsed: Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    let msg = parsed.to_string().to_lowercase();
    assert!(
        msg.contains("password"),
        "the caller is told which field: {msg}"
    );
}

#[tokio::test]
async fn an_unknown_source_kind_lists_the_known_ones() {
    let state = common::state_with_admin_rbac("http://unused.invalid").await;
    let cookie = seed_admin(&state, "alice").await;
    let app = common::app(state);

    let body = r#"{
        "name": "docs",
        "git_url": "",
        "embedding_model": "embed-1",
        "source_kind": "dropbox",
        "source_config": {}
    }"#;
    let resp = app
        .serve(req_with_cookie(
            Method::POST,
            "/api/v0/rag/collections",
            &cookie,
            Some(body),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let parsed: Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    let msg = parsed.to_string();
    assert!(msg.contains("git") && msg.contains("webdav"), "{msg}");
}

#[tokio::test]
async fn a_source_config_without_its_kind_is_rejected() {
    let state = common::state_with_admin_rbac("http://unused.invalid").await;
    let cookie = seed_admin(&state, "alice").await;
    let app = common::app(state);

    let resp = app
        .serve(req_with_cookie(
            Method::POST,
            "/api/v0/rag/collections",
            &cookie,
            Some(create_body()),
        ))
        .await
        .unwrap();
    let created: Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    let id = created["id"].as_i64().unwrap();

    // A settings map means nothing without the kind whose schema defines it,
    // so accepting one alone would silently apply it to the wrong provider.
    let resp = app
        .serve(req_with_cookie(
            Method::PATCH,
            &format!("/api/v0/rag/collections/{id}"),
            &cookie,
            Some(r#"{"source_config": {"base_url": "https://cloud.example.com"}}"#),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn patching_a_git_collection_onto_a_remote_source_replaces_it() {
    let state = common::state_with_admin_rbac("http://unused.invalid").await;
    let cookie = seed_admin(&state, "alice").await;
    let db = state.db.clone();
    let app = common::app(state);

    let resp = app
        .serve(req_with_cookie(
            Method::POST,
            "/api/v0/rag/collections",
            &cookie,
            Some(create_body()),
        ))
        .await
        .unwrap();
    let created: Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    let id = created["id"].as_i64().unwrap();
    assert_eq!(created["source_kind"], "git");

    let patch = r#"{
        "source_kind": "webdav",
        "source_config": {
            "base_url": "https://cloud.example.com",
            "username": "svc",
            "password": "app-pw"
        }
    }"#;
    let resp = app
        .serve(req_with_cookie(
            Method::PATCH,
            &format!("/api/v0/rag/collections/{id}"),
            &cookie,
            Some(patch),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let updated: Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(updated["source_kind"], "webdav");
    assert_eq!(updated["source_secrets_set"], true);

    let stored = aiplane_core::server::db::rag::find_collection_by_id(&db, id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.source.kind, "webdav");
    assert!(stored.source.secrets.is_some());
}

#[tokio::test]
async fn create_rejects_duplicate_name_with_a_helpful_400() {
    let state = common::state_with_admin_rbac("http://unused.invalid").await;
    let cookie = seed_admin(&state, "alice").await;
    let app = common::app(state);

    let _ = app
        .serve(req_with_cookie(
            Method::POST,
            "/api/v0/rag/collections",
            &cookie,
            Some(create_body()),
        ))
        .await
        .unwrap();
    let resp = app
        .serve(req_with_cookie(
            Method::POST,
            "/api/v0/rag/collections",
            &cookie,
            Some(create_body()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let parsed: Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    let msg = parsed["error"]["message"].as_str().unwrap();
    assert!(msg.contains("already exists"), "{msg}");
}

#[tokio::test]
async fn create_validates_inputs() {
    let state = common::state_with_admin_rbac("http://unused.invalid").await;
    let cookie = seed_admin(&state, "alice").await;
    let app = common::app(state);

    // chunk_overlap >= chunk_size
    let body = r#"{
        "name": "bad", "git_url": "u", "embedding_model": "m",
        "chunk_size": 100, "chunk_overlap": 100
    }"#;
    let resp = app
        .serve(req_with_cookie(
            Method::POST,
            "/api/v0/rag/collections",
            &cookie,
            Some(body),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

/// A collection may only be restricted to groups that exist. An unmatchable name
/// makes it invisible and unsearchable rather than restricted, which reads as the
/// collection having broken.
#[tokio::test]
async fn collection_access_rejects_a_group_that_does_not_exist() {
    let state = common::state_with_admin_rbac("http://unused.invalid").await;
    let cookie = seed_admin(&state, "alice").await;
    aiplane_core::server::db::gateway_groups::upsert_group(
        &state.db,
        "engineering",
        "",
        false,
        false,
    )
    .await
    .unwrap();
    let app = common::app(state);

    let resp = app
        .serve(req_with_cookie(
            Method::POST,
            "/api/v0/rag/collections",
            &cookie,
            Some(create_body()),
        ))
        .await
        .unwrap();
    let created: Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    let id = created["id"].as_i64().unwrap();

    let resp = app
        .serve(req_with_cookie(
            Method::PATCH,
            &format!("/api/v0/rag/collections/{id}"),
            &cookie,
            Some(r#"{"allowed_groups": ["egineering"]}"#),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let raw = String::from_utf8(common::read_body(resp).await.to_vec()).unwrap();
    assert!(raw.contains("egineering"), "{raw}");

    // The rejection left the collection alone rather than half-applying.
    let resp = app
        .serve(req_with_cookie(
            Method::GET,
            &format!("/api/v0/rag/collections/{id}"),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    let after: Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(after["allowed_groups"], serde_json::json!([]));
}

/// The collection editor renders its access picker from this payload.
#[tokio::test]
async fn providers_endpoint_carries_the_group_vocabulary() {
    let state = common::state_with_admin_rbac("http://unused.invalid").await;
    let cookie = seed_admin(&state, "alice").await;
    aiplane_core::server::db::gateway_groups::upsert_group(
        &state.db,
        "engineering",
        "",
        false,
        false,
    )
    .await
    .unwrap();
    let app = common::app(state);

    let resp = app
        .serve(req_with_cookie(
            Method::GET,
            "/api/v0/rag/providers",
            &cookie,
            None,
        ))
        .await
        .unwrap();
    let listed: Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(listed["groups"], serde_json::json!(["engineering"]));
}

/// The scheduled re-sync is the only way a pull-only source (a mailing-list
/// archive; a plain WebDAV share) ever gets newer, so its contract is pinned:
/// off unless asked for, a real schedule when asked, and a refusal — not a
/// clamp — for an interval that would re-index continuously.
#[tokio::test]
async fn a_refresh_interval_round_trips_and_refuses_a_runaway_schedule() {
    let state = common::state_with_admin_rbac("http://unused.invalid").await;
    let cookie = seed_admin(&state, "alice").await;
    let app = common::app(state);

    let resp = app
        .serve(req_with_cookie(
            Method::POST,
            "/api/v0/rag/collections",
            &cookie,
            Some(create_body()),
        ))
        .await
        .unwrap();
    let created: Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    let id = created["id"].as_i64().unwrap();
    assert_eq!(
        created["refresh_interval_mins"], 0,
        "a caller that says nothing keeps the behaviour it had before the field existed"
    );

    let resp = app
        .serve(req_with_cookie(
            Method::PATCH,
            &format!("/api/v0/rag/collections/{id}"),
            &cookie,
            Some(r#"{"refresh_interval_mins": 1440}"#),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let patched: Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(patched["refresh_interval_mins"], 1440);

    for bad in ["1", "-5", "600000"] {
        let resp = app
            .serve(req_with_cookie(
                Method::PATCH,
                &format!("/api/v0/rag/collections/{id}"),
                &cookie,
                Some(&format!(r#"{{"refresh_interval_mins": {bad}}}"#)),
            ))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "`{bad}` minutes should be refused, not stored or clamped"
        );
    }

    // And it survives the round trip on create, too.
    let body = r#"{
        "name": "mail", "embedding_model": "m",
        "source_kind": "hyperkitty",
        "source_config": { "list_url": "https://lists.example.com/hyperkitty/list/users@example.com/" },
        "refresh_interval_mins": 1440
    }"#;
    let resp = app
        .serve(req_with_cookie(
            Method::POST,
            "/api/v0/rag/collections",
            &cookie,
            Some(body),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let created: Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(created["refresh_interval_mins"], 1440);
    assert_eq!(created["source_kind"], "hyperkitty");
}

#[tokio::test]
async fn update_with_empty_body_returns_current_state() {
    let state = common::state_with_admin_rbac("http://unused.invalid").await;
    let cookie = seed_admin(&state, "alice").await;
    let app = common::app(state);
    let resp = app
        .serve(req_with_cookie(
            Method::POST,
            "/api/v0/rag/collections",
            &cookie,
            Some(create_body()),
        ))
        .await
        .unwrap();
    let created: Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    let id = created["id"].as_i64().unwrap();

    let resp = app
        .serve(req_with_cookie(
            Method::PATCH,
            &format!("/api/v0/rag/collections/{id}"),
            &cookie,
            Some("{}"),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v: Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(v["id"], id);
    assert_eq!(v["embedding_model"], "embed-1");
}

#[tokio::test]
async fn update_can_clear_pat() {
    let state = common::state_with_admin_rbac("http://unused.invalid").await;
    let cookie = seed_admin(&state, "alice").await;
    let app = common::app(state);
    // Create with a PAT.
    let body = r#"{
        "name": "with-pat",
        "git_url": "https://example.invalid/private.git",
        "embedding_model": "embed-1",
        "pat": "ghp_token"
    }"#;
    let resp = app
        .serve(req_with_cookie(
            Method::POST,
            "/api/v0/rag/collections",
            &cookie,
            Some(body),
        ))
        .await
        .unwrap();
    let created: Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(created["pat_set"], true);
    let id = created["id"].as_i64().unwrap();

    // PATCH with `"pat": null` → cleared.
    let resp = app
        .serve(req_with_cookie(
            Method::PATCH,
            &format!("/api/v0/rag/collections/{id}"),
            &cookie,
            Some(r#"{"pat": null}"#),
        ))
        .await
        .unwrap();
    let v: Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(v["pat_set"], false);
}

/// Sources are what a collection actually indexes, so adding them is the
/// endpoint that makes the rest useful. Covers the aggregate shape (a list in
/// one call), the idempotence a re-paste depends on, and the primary switch.
#[tokio::test]
async fn refs_add_list_primary_and_dedupe() {
    let state = common::state_with_admin_rbac("http://unused.invalid").await;
    let cookie = seed_admin(&state, "boss").await;
    let app = common::app(state);

    let resp = app
        .serve(req_with_cookie(
            Method::POST,
            "/api/v0/rag/collections",
            &cookie,
            Some(create_body()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let created: Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    let id = created["id"].as_i64().unwrap();

    // Two sources in one call; the second names its own ref, the first
    // inherits the collection's.
    let body = json!({
        "sources": [
            { "url": "https://example.invalid/one.git" },
            { "url": "https://example.invalid/two.git", "git_ref": "develop" },
        ]
    })
    .to_string();
    let resp = app
        .serve(req_with_cookie(
            Method::POST,
            &format!("/api/v0/rag/collections/{id}/refs"),
            &cookie,
            Some(&body),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let added: Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(added["added"].as_array().unwrap().len(), 2);
    assert_eq!(added["skipped"], 0);
    assert_eq!(
        added["added"][0]["git_ref"], "main",
        "an entry without a ref inherits the collection's"
    );
    assert_eq!(added["added"][1]["git_ref"], "develop");

    // Re-submitting the same list adds nothing: a bulk re-paste is idempotent.
    let resp = app
        .serve(req_with_cookie(
            Method::POST,
            &format!("/api/v0/rag/collections/{id}/refs"),
            &cookie,
            Some(&body),
        ))
        .await
        .unwrap();
    let again: Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(again["added"].as_array().unwrap().len(), 0);
    assert_eq!(again["skipped"], 2);

    // The first source of an empty collection became the primary.
    let resp = app
        .serve(req_with_cookie(
            Method::GET,
            &format!("/api/v0/rag/collections/{id}/refs"),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    let listed: Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    let refs = listed["data"].as_array().unwrap();
    assert_eq!(refs.len(), 2);
    let first = refs.iter().find(|r| r["is_primary"] == true).unwrap();
    let second = refs.iter().find(|r| r["is_primary"] == false).unwrap();
    let second_id = second["id"].as_i64().unwrap();

    // Switching the primary moves it, rather than adding a second one.
    let resp = app
        .serve(req_with_cookie(
            Method::POST,
            &format!("/api/v0/rag/collections/{id}/refs/{second_id}/primary"),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let resp = app
        .serve(req_with_cookie(
            Method::GET,
            &format!("/api/v0/rag/collections/{id}/refs"),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    let listed: Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    let primaries: Vec<i64> = listed["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["is_primary"] == true)
        .map(|r| r["id"].as_i64().unwrap())
        .collect();
    assert_eq!(
        primaries,
        vec![second_id],
        "exactly one primary, and it moved"
    );

    // A ref id from another collection must not repoint this one's primary.
    let first_id = first["id"].as_i64().unwrap();
    let resp = app
        .serve(req_with_cookie(
            Method::POST,
            &format!(
                "/api/v0/rag/collections/{}/refs/{first_id}/primary",
                id + 999
            ),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// Extraction profiles: create, edit, and the two refusals that keep a
/// collection from losing the fields it indexes against.
#[tokio::test]
async fn profiles_create_update_and_guarded_delete() {
    let state = common::state_with_admin_rbac("http://unused.invalid").await;
    let cookie = seed_admin(&state, "boss").await;
    let app = common::app(state);

    let body = json!({
        "name": "invoices",
        "description": "invoice metadata",
        "prompt": "Extract the invoice fields.",
        "fields": [{ "key": "total", "label": "Total", "type": "number" }],
    })
    .to_string();
    let resp = app
        .serve(req_with_cookie(
            Method::POST,
            "/api/v0/rag/profiles",
            &cookie,
            Some(&body),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // The name is the handle, so a second profile cannot take it.
    let resp = app
        .serve(req_with_cookie(
            Method::POST,
            "/api/v0/rag/profiles",
            &cookie,
            Some(&body),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // Editing reports which collections must re-index — none yet.
    let edit = json!({
        "name": "invoices",
        "prompt": "Extract the invoice fields, including tax.",
        "fields": [{ "key": "total", "label": "Total", "type": "number" }],
    })
    .to_string();
    let resp = app
        .serve(req_with_cookie(
            Method::PUT,
            "/api/v0/rag/profiles/invoices",
            &cookie,
            Some(&edit),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let saved: Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(saved["reindex_required_by"].as_array().unwrap().len(), 0);

    // An empty prompt is refused rather than silently stored.
    let resp = app
        .serve(req_with_cookie(
            Method::PUT,
            "/api/v0/rag/profiles/invoices",
            &cookie,
            Some(r#"{"name":"invoices","prompt":"  ","fields":[]}"#),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // Unknown names 404 on both verbs.
    let resp = app
        .serve(req_with_cookie(
            Method::DELETE,
            "/api/v0/rag/profiles/nope",
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // Deleting an unused profile works.
    let resp = app
        .serve(req_with_cookie(
            Method::DELETE,
            "/api/v0/rag/profiles/invoices",
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn collection_editor_round_trips_search_access_and_profile_settings() {
    let state = common::state_with_admin_rbac("http://unused.invalid").await;
    let cookie = seed_admin(&state, "boss").await;
    let db = state.db.clone();
    let app = common::app(state);

    let profile = json!({
        "name": "contracts",
        "description": "contract metadata",
        "prompt": "Extract the contract fields.",
        "fields": [{ "key": "party", "label": "Party", "type": "text" }],
    })
    .to_string();
    let resp = app
        .serve(req_with_cookie(
            Method::POST,
            "/api/v0/rag/profiles",
            &cookie,
            Some(&profile),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let create = json!({
        "name": "legal",
        "git_url": "https://example.invalid/legal.git",
        "embedding_model": "embed-1",
        "search_mode": "aggregate",
    })
    .to_string();
    let resp = app
        .serve(req_with_cookie(
            Method::POST,
            "/api/v0/rag/collections",
            &cookie,
            Some(&create),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let created: Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(created["search_mode"], "aggregate");
    assert_eq!(created["sync_hook_set"], false);
    assert_eq!(created["allowed_groups"], json!([]));
    let id = created["id"].as_i64().unwrap();

    // `allowed_groups` names groups, and a name that matches none is refused —
    // it would hide the collection rather than restrict it. Create them first.
    for group in ["legal", "admins"] {
        aiplane_core::server::db::gateway_groups::upsert_group(&db, group, "", false, false)
            .await
            .unwrap();
    }
    let patch = json!({
        "profile": "contracts",
        "extraction_model": "chat-1",
        "allowed_groups": ["legal", "admins"],
    })
    .to_string();
    let resp = app
        .serve(req_with_cookie(
            Method::PATCH,
            &format!("/api/v0/rag/collections/{id}"),
            &cookie,
            Some(&patch),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let updated: Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert!(updated["profile_id"].is_i64());
    assert_eq!(updated["extraction_model"], "chat-1");
    assert_eq!(updated["allowed_groups"], json!(["legal", "admins"]));

    let resp = app
        .serve(req_with_cookie(
            Method::GET,
            "/api/v0/rag/profiles",
            &cookie,
            None,
        ))
        .await
        .unwrap();
    let profiles: Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    let saved = profiles["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|profile| profile["name"] == "contracts")
        .unwrap();
    assert_eq!(saved["prompt"], "Extract the contract fields.");
    assert_eq!(saved["builtin"], false);
}

#[tokio::test]
async fn ref_editor_and_index_log_are_available_to_the_admin_ui() {
    use aiplane_core::server::db::rag::{self as rag_db, LogLevel, NewLogEntry};

    let state = common::state_with_admin_rbac("http://unused.invalid").await;
    let cookie = seed_admin(&state, "boss").await;
    let db = state.db.clone();
    let app = common::app(state);

    let resp = app
        .serve(req_with_cookie(
            Method::POST,
            "/api/v0/rag/collections",
            &cookie,
            Some(create_body()),
        ))
        .await
        .unwrap();
    let created: Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    let id = created["id"].as_i64().unwrap();
    let source = rag_db::add_ref(&db, id, "main", None, true).await.unwrap();

    rag_db::insert_log_entry(
        &db,
        &NewLogEntry {
            ref_id: source.id,
            collection_id: id,
            level: LogLevel::Info,
            phase: "ready".into(),
            message: "Indexed source".into(),
            commit_sha: Some("0123456789abcdef".into()),
            files: Some(12),
            chunks: Some(48),
            duration_ms: Some(900),
        },
    )
    .await
    .unwrap();

    let patch = json!({
        "git_url": "https://example.invalid/renamed.git",
        "git_ref": "stable",
    })
    .to_string();
    let resp = app
        .serve(req_with_cookie(
            Method::PATCH,
            &format!("/api/v0/rag/collections/{id}/refs/{}", source.id),
            &cookie,
            Some(&patch),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let updated: Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(updated["git_ref"], "stable");
    assert_eq!(updated["git_url"], "https://example.invalid/renamed.git");

    let resp = app
        .serve(req_with_cookie(
            Method::GET,
            &format!("/api/v0/rag/collections/{id}/refs/{}/log", source.id),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let log: Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(log["data"][0]["message"], "Indexed source");
    assert_eq!(log["data"][0]["files"], 12);
    assert_eq!(log["data"][0]["chunks"], 48);

    let resp = app
        .serve(req_with_cookie(
            Method::GET,
            &format!("/api/v0/rag/collections/{id}/refs"),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    let refs: Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(refs["data"][0]["document_count"], 12);
}
