// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! /api/v0/* — session-authed JSON endpoints (token CRUD, /me).

use crate::common;

use common::Service as _;
use rama::http::{Body, Method, Request, StatusCode};

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

#[tokio::test]
async fn personal_token_priority_creation_editing_and_inheritance() {
    use aiplane_core::server::{db::tokens, priority::Priority};
    let state = common::state_with_chat_pool("http://unused.invalid").await;
    let cookie = common::seed_session(&state, "alice", "alice@example.com").await;
    let app = common::app(state.clone());
    let response = app
        .serve(req_with_cookie(
            Method::POST,
            "/api/v0/tokens",
            &cookie,
            Some(r#"{"name":"background","priority":"low"}"#),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let created: serde_json::Value =
        serde_json::from_slice(&common::read_body(response).await).unwrap();
    let id = created["token"]["id"].as_str().unwrap();
    assert_eq!(
        tokens::find_by_id(&state.db, id)
            .await
            .unwrap()
            .unwrap()
            .priority,
        Some(Priority::Low)
    );
    for priority in [Some("medium"), Some("low"), None] {
        let body = serde_json::json!({"priority": priority}).to_string();
        let response = app
            .serve(req_with_cookie(
                Method::PUT,
                &format!("/api/v0/tokens/{id}/priority"),
                &cookie,
                Some(&body),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            tokens::find_by_id(&state.db, id)
                .await
                .unwrap()
                .unwrap()
                .priority
                .map(Priority::as_str),
            priority
        );
        let response = app
            .serve(req_with_cookie(
                Method::GET,
                "/api/v0/tokens/details",
                &cookie,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let details: serde_json::Value =
            serde_json::from_slice(&common::read_body(response).await).unwrap();
        assert_eq!(details["owner_priority"], "medium");
        assert_eq!(details["max_priority"], "medium");
        assert_eq!(details["priority_enabled"], false);
        assert_eq!(
            details["tokens"][0]["priority"],
            serde_json::json!(priority)
        );
    }
    tokens::set_priority(&state.db, id, Some(Priority::Low))
        .await
        .unwrap();
    let response = app
        .serve(req_with_cookie(
            Method::POST,
            &format!("/api/v0/tokens/{id}/rotate"),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        tokens::find_by_id(&state.db, id)
            .await
            .unwrap()
            .unwrap()
            .priority,
        Some(Priority::Low)
    );
}

#[tokio::test]
async fn personal_token_priority_uses_live_group_ceiling_and_admin_rights() {
    use aiplane_core::server::{db::gateway_groups, priority::Priority};
    let state = common::state_with_chat_pool("http://unused.invalid").await;
    let cookie = common::seed_session(&state, "alice", "alice@example.com").await;
    gateway_groups::upsert_group(&state.db, "priority-group", "", false, true)
        .await
        .unwrap();
    let app = common::app(state.clone());
    for (ceiling, admin, value, status) in [
        (Priority::Low, false, "medium", StatusCode::FORBIDDEN),
        (Priority::High, false, "high", StatusCode::OK),
        (Priority::Low, true, "high", StatusCode::OK),
    ] {
        gateway_groups::upsert_group(&state.db, "priority-group", "", admin, true)
            .await
            .unwrap();
        gateway_groups::set_priority(&state.db, "priority-group", Some(ceiling))
            .await
            .unwrap();
        state
            .rbac
            .reload(gateway_groups::load_snapshot(&state.db).await.unwrap());
        let response = app
            .serve(req_with_cookie(
                Method::GET,
                "/api/v0/tokens/details",
                &cookie,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let details: serde_json::Value =
            serde_json::from_slice(&common::read_body(response).await).unwrap();
        assert_eq!(details["owner_priority"], ceiling.as_str());
        assert_eq!(
            details["max_priority"],
            if admin { "high" } else { ceiling.as_str() }
        );
        let body = serde_json::json!({"name":"background","priority":value}).to_string();
        let response = app
            .serve(req_with_cookie(
                Method::POST,
                "/api/v0/tokens",
                &cookie,
                Some(&body),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        if status == StatusCode::OK {
            let created: serde_json::Value =
                serde_json::from_slice(&common::read_body(response).await).unwrap();
            let id = created["token"]["id"].as_str().unwrap();
            let update = serde_json::json!({"priority": value}).to_string();
            let response = app
                .serve(req_with_cookie(
                    Method::PUT,
                    &format!("/api/v0/tokens/{id}/priority"),
                    &cookie,
                    Some(&update),
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }
    }
}

#[tokio::test]
async fn personal_token_priority_rejects_escalation_invalid_values_and_foreign_tokens() {
    use aiplane_core::server::{db::tokens, priority::Priority};
    let state = common::state_with_chat_pool("http://unused.invalid").await;
    let cookie = common::seed_session(&state, "alice", "alice@example.com").await;
    let (_, id) = common::seed_user_with_token_id(&state, "alice").await;
    let foreign_cookie = common::seed_session(&state, "bob", "bob@example.com").await;
    tokens::set_priority(&state.db, &id, Some(Priority::High))
        .await
        .unwrap();
    let app = common::app(state.clone());
    for (value, status) in [
        ("high", StatusCode::FORBIDDEN),
        ("urgent", StatusCode::BAD_REQUEST),
    ] {
        let body = serde_json::json!({"name":"background","priority":value}).to_string();
        let response = app
            .serve(req_with_cookie(
                Method::POST,
                "/api/v0/tokens",
                &cookie,
                Some(&body),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        let update = serde_json::json!({"priority":value}).to_string();
        let response = app
            .serve(req_with_cookie(
                Method::PUT,
                &format!("/api/v0/tokens/{id}/priority"),
                &cookie,
                Some(&update),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        assert_eq!(
            tokens::find_by_id(&state.db, &id)
                .await
                .unwrap()
                .unwrap()
                .priority,
            Some(Priority::High)
        );
    }
    assert_eq!(
        tokens::list_for_user(&state.db, "alice")
            .await
            .unwrap()
            .len(),
        1
    );
    for token_id in [&id, "missing"] {
        let response = app
            .serve(req_with_cookie(
                Method::PUT,
                &format!("/api/v0/tokens/{token_id}/priority"),
                &foreign_cookie,
                Some(r#"{"priority":"low"}"#),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    let response = app
        .serve(common::req(
            Method::PUT,
            &format!("/api/v0/tokens/{id}/priority"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        tokens::find_by_id(&state.db, &id)
            .await
            .unwrap()
            .unwrap()
            .priority,
        Some(Priority::High)
    );
}

#[tokio::test]
async fn me_anonymous_is_401() {
    let state = common::state_with_chat_pool("http://unused.invalid").await;
    let app = common::app(state);
    let resp = app
        .serve(common::req(Method::GET, "/api/v0/me"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn agent_browser_control_rejects_an_untrusted_origin() {
    let state = common::state_with_chat_pool("http://unused.invalid").await;
    let cookie = common::seed_session(&state, "alice", "alice@example.com").await;
    let app = common::app(state);
    for action in ["claim", "frame", "input", "finish", "abort"] {
        let path =
            format!("/api/v0/chat/sessions/session/turns/turn/browser-handover/request/{action}");
        let response = app
            .serve(
                Request::builder()
                    .method(Method::POST)
                    .uri(path)
                    .header("cookie", format!("id={cookie}"))
                    .header("origin", "https://attacker.example")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{action}");
        assert_eq!(response.headers()["cache-control"], "no-store, private");
    }
}

#[tokio::test]
async fn me_with_session_returns_user() {
    let state = common::state_with_chat_pool("http://unused.invalid").await;
    let cookie = common::seed_session(&state, "alice", "alice@example.com").await;
    let app = common::app(state);
    let resp = app
        .serve(req_with_cookie(Method::GET, "/api/v0/me", &cookie, None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let parsed: serde_json::Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(parsed["id"], "alice");
    assert_eq!(parsed["email"], "alice@example.com");
}

#[tokio::test]
async fn create_then_list_then_revoke_then_delete() {
    let state = common::state_with_chat_pool("http://unused.invalid").await;
    let cookie = common::seed_session(&state, "bob", "bob@example.com").await;
    let app = common::app(state);

    // List → empty
    let resp = app
        .serve(req_with_cookie(
            Method::GET,
            "/api/v0/tokens",
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let list: serde_json::Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 0);

    // Create
    let resp = app
        .serve(req_with_cookie(
            Method::POST,
            "/api/v0/tokens",
            &cookie,
            Some(r#"{"name":"laptop","ttl_days":30}"#),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let parsed: serde_json::Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert!(parsed["plaintext"].as_str().unwrap().starts_with("gwk_"));
    let token_id = parsed["token"]["id"].as_str().unwrap().to_string();

    // Revoke
    let resp = app
        .serve(req_with_cookie(
            Method::POST,
            &format!("/api/v0/tokens/{token_id}/revoke"),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let parsed: serde_json::Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(parsed["revoked"], true);

    // Delete (hard-removes the revoked row)
    let resp = app
        .serve(req_with_cookie(
            Method::DELETE,
            &format!("/api/v0/tokens/{token_id}"),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let parsed: serde_json::Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(parsed["deleted"], true);

    // List → empty again
    let resp = app
        .serve(req_with_cookie(
            Method::GET,
            "/api/v0/tokens",
            &cookie,
            None,
        ))
        .await
        .unwrap();
    let list: serde_json::Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert!(list.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn rotate_swaps_secret_then_refuses_revoked() {
    let state = common::state_with_chat_pool("http://unused.invalid").await;
    let cookie = common::seed_session(&state, "dave", "dave@example.com").await;
    let app = common::app(state);

    // Create a token, capture its first plaintext + id.
    let resp = app
        .serve(req_with_cookie(
            Method::POST,
            "/api/v0/tokens",
            &cookie,
            Some(r#"{"name":"ci","ttl_days":30}"#),
        ))
        .await
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    let token_id = parsed["token"]["id"].as_str().unwrap().to_string();
    let first_plaintext = parsed["plaintext"].as_str().unwrap().to_string();

    // Rotate → 200 with a fresh plaintext, same id + name.
    let resp = app
        .serve(req_with_cookie(
            Method::POST,
            &format!("/api/v0/tokens/{token_id}/rotate"),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let parsed: serde_json::Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    let second_plaintext = parsed["plaintext"].as_str().unwrap().to_string();
    assert!(second_plaintext.starts_with("gwk_"));
    assert_ne!(
        first_plaintext, second_plaintext,
        "rotation must issue a new secret"
    );
    assert_eq!(parsed["token"]["id"], token_id);
    assert_eq!(parsed["token"]["name"], "ci");
    assert_eq!(parsed["token"]["revoked"], false);

    // Revoke, then rotate again → 404 (a revoked token can't be resurrected).
    let resp = app
        .serve(req_with_cookie(
            Method::POST,
            &format!("/api/v0/tokens/{token_id}/revoke"),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = app
        .serve(req_with_cookie(
            Method::POST,
            &format!("/api/v0/tokens/{token_id}/rotate"),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn cannot_delete_active_token() {
    let state = common::state_with_chat_pool("http://unused.invalid").await;
    let cookie = common::seed_session(&state, "carol", "carol@example.com").await;
    let app = common::app(state);

    let resp = app
        .serve(req_with_cookie(
            Method::POST,
            "/api/v0/tokens",
            &cookie,
            Some(r#"{"name":"active","ttl_days":30}"#),
        ))
        .await
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    let token_id = parsed["token"]["id"].as_str().unwrap().to_string();

    let resp = app
        .serve(req_with_cookie(
            Method::DELETE,
            &format!("/api/v0/tokens/{token_id}"),
            &cookie,
            None,
        ))
        .await
        .unwrap();
    // Hard-delete refuses non-revoked tokens — keeps the audit trail
    // honest if a token gets stolen.
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let parsed: serde_json::Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(parsed["error"]["code"], "token_active");
}

/// Revoke and delete answer by meaning: a token that is missing or someone
/// else's is a 404, revoking twice or deleting an active token a 409.
#[tokio::test]
async fn token_revoke_and_delete_refuse_with_their_status() {
    let state = common::state_with_chat_pool("http://unused.invalid").await;
    let cookie = common::seed_session(&state, "erin", "erin@example.com").await;
    let other = common::seed_session(&state, "frank", "frank@example.com").await;
    let app = common::app(state);
    let resp = app
        .serve(req_with_cookie(
            Method::POST,
            "/api/v0/tokens",
            &cookie,
            Some(r#"{"name":"laptop","ttl_days":30}"#),
        ))
        .await
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    let id = parsed["token"]["id"].as_str().unwrap().to_string();
    let call = |method: Method, path: String, who: &str| req_with_cookie(method, &path, who, None);
    let code = |resp: rama::http::Response| async move {
        let status = resp.status();
        let body: serde_json::Value =
            serde_json::from_slice(&common::read_body(resp).await).unwrap_or_default();
        (
            status,
            body["error"]["code"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
        )
    };

    for (method, path, who, status, expected) in [
        (
            Method::DELETE,
            format!("/api/v0/tokens/{id}"),
            &cookie,
            StatusCode::CONFLICT,
            "token_active",
        ),
        (
            Method::POST,
            format!("/api/v0/tokens/{id}/revoke"),
            &other,
            StatusCode::NOT_FOUND,
            "not_found",
        ),
        (
            Method::POST,
            "/api/v0/tokens/nope/revoke".to_string(),
            &cookie,
            StatusCode::NOT_FOUND,
            "not_found",
        ),
        (
            Method::POST,
            format!("/api/v0/tokens/{id}/revoke"),
            &cookie,
            StatusCode::OK,
            "",
        ),
        (
            Method::POST,
            format!("/api/v0/tokens/{id}/revoke"),
            &cookie,
            StatusCode::CONFLICT,
            "token_already_revoked",
        ),
        (
            Method::DELETE,
            format!("/api/v0/tokens/{id}"),
            &other,
            StatusCode::NOT_FOUND,
            "not_found",
        ),
        (
            Method::DELETE,
            format!("/api/v0/tokens/{id}"),
            &cookie,
            StatusCode::OK,
            "",
        ),
        (
            Method::DELETE,
            format!("/api/v0/tokens/{id}"),
            &cookie,
            StatusCode::NOT_FOUND,
            "not_found",
        ),
    ] {
        let (got, got_code) = code(
            app.serve(call(method.clone(), path.clone(), who))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(
            (got, got_code.as_str()),
            (status, expected),
            "{method} {path}"
        );
    }
}
