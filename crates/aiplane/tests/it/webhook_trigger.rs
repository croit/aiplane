// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The public webhook trigger (`/hooks/{secret}`): whoever holds the URL
//! decides the body, so the gateway reads at most the payload cap of it.

use rama::http::{Method, Request, StatusCode};

use crate::common;

use aiplane_core::server::auth::token;
use aiplane_runtime::server::webhooks::{self, NewWebhook};

#[tokio::test]
async fn an_endless_body_is_cut_at_the_payload_cap_instead_of_buffered() {
    let state = common::state_no_skills().await;
    common::seed_user_with_token(&state, "owner").await;
    let (secret, secret_hash) = token::mint_webhook();
    let hook = webhooks::create(
        &state.db,
        NewWebhook {
            dot_id: None,
            linked_session_id: None,
            user_id: "owner".into(),
            name: "ci".into(),
            prompt: "summarise".into(),
            model: "model-a".into(),
            tools_enabled: false,
            synchronous: false,
            reuse_conversation: false,
            reuse_rounds: 0,
            secret_hash,
        },
    )
    .await
    .unwrap();

    let req = Request::builder()
        .method(Method::POST)
        .uri(format!("/hooks/{secret}"))
        .body(common::endless_body())
        .unwrap();
    let resp = common::serve_promptly(&state, req).await;
    assert_eq!(resp.status(), StatusCode::ACCEPTED);

    let stored = webhooks::get(&state.db, "owner", &hook.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.last_payload.unwrap().len(), 256 * 1024);
}

async fn hook(state: &aiplane::rama_server::RamaState, synchronous: bool) -> String {
    common::seed_user_with_token(state, "owner").await;
    let (secret, secret_hash) = token::mint_webhook();
    webhooks::create(
        &state.db,
        NewWebhook {
            dot_id: None,
            linked_session_id: None,
            user_id: "owner".into(),
            name: "ci".into(),
            prompt: "summarise".into(),
            model: "model-a".into(),
            tools_enabled: false,
            synchronous,
            reuse_conversation: false,
            reuse_rounds: 0,
            secret_hash,
        },
    )
    .await
    .unwrap();
    secret
}

async fn fire(
    state: &aiplane::rama_server::RamaState,
    path: &str,
) -> (StatusCode, serde_json::Value) {
    let req = Request::builder()
        .method(Method::POST)
        .uri(path)
        .body(rama::http::Body::from("{}"))
        .unwrap();
    let resp = common::serve_promptly(state, req).await;
    let status = resp.status();
    let body = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    (status, body)
}

/// The public hooks refuse in the shared error envelope, like every other
/// route: an unknown secret or sync token is a 404 `not_found`.
#[tokio::test]
async fn unknown_hook_credentials_are_404_in_the_shared_envelope() {
    let state = common::state_no_skills().await;
    for path in ["/hooks/gwh_0000", "/hooks/rag/NoSuchToken"] {
        let (status, body) = fire(&state, path).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}: {body}");
        assert_eq!(body["error"]["code"], "not_found", "{path}: {body}");
    }
}

/// An async hook answers 202 naming the conversation it opened.
#[tokio::test]
async fn an_async_hook_accepts_with_its_session() {
    let state = common::state_no_skills().await;
    let secret = hook(&state, false).await;
    let (status, body) = fire(&state, &format!("/hooks/{secret}")).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert!(body["session_id"].is_string(), "{body}");
}

/// A synchronous run that fails is a 502 `run_failed` envelope carrying the
/// session and the run's status.
#[tokio::test]
async fn a_failed_sync_run_is_a_502_run_failed() {
    let state = common::state_no_skills().await;
    let secret = hook(&state, true).await;
    let (status, body) = fire(&state, &format!("/hooks/{secret}")).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert_eq!(body["error"]["code"], "run_failed", "{body}");
    assert!(body["error"]["session_id"].is_string(), "{body}");
    assert!(body["error"]["status"].is_string(), "{body}");
}
