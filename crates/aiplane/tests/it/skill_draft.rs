// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! `POST /api/v0/skills/draft` — user-entered workflow steps become an
//! unsaved personal skill manifest through one metered model call.

use crate::common;

use std::sync::Arc;

use aiplane::rama_server::router::router;
use common::Service as _;
use rama::http::StatusCode;
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const DRAFT: &str = "/api/v0/skills/draft";

async fn upstream() -> MockServer {
    let server = MockServer::start().await;
    let result = json!({
        "title": "Review support tickets",
        "description": "Review incoming support tickets and flag urgent issues.",
        "instructions": "## Steps\n1. Review incoming tickets.\n2. Flag urgent issues.\n\nAsk before changing a ticket."
    });
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{ "message": {
                "role": "assistant",
                "content": format!("```json\n{result}\n```"),
            } }],
        })))
        .mount(&server)
        .await;
    server
}

async fn draft(
    state: &Arc<aiplane::rama_server::RamaState>,
    cookie: &str,
    model: &str,
) -> (StatusCode, Value) {
    let body = json!({
        "name": "Support ticket review",
        "goal": "Flag urgent issues for a human",
        "steps": ["Read each new ticket", "Check whether it is urgent"],
        "stop_conditions": "Ask before editing or replying to a ticket",
        "model": model
    });
    let response = router(state.clone())
        .serve(common::post_json(DRAFT, cookie, &body.to_string()))
        .await
        .unwrap();
    let status = response.status();
    let body = serde_json::from_slice(&common::read_body(response).await).unwrap();
    (status, body)
}

#[tokio::test]
async fn an_owned_workflow_becomes_an_unsaved_manifest_using_the_selected_model() {
    let server = upstream().await;
    let temp = tempfile::tempdir().unwrap();
    let state = Arc::new(
        common::state_with_chat_pool_and_user_skills(&server.uri(), temp.path().to_path_buf())
            .await,
    );
    let cookie = common::seed_session(&state, "alice", "alice@example.com").await;

    let (status, body) = draft(&state, &cookie, "model-a").await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let manifest = body["manifest"].as_str().expect("manifest");
    assert!(manifest.starts_with("---\nname: support-ticket-review\n"));
    assert!(manifest.contains("title: 'Review support tickets'"));
    assert!(
        manifest.contains("description: 'Review incoming support tickets and flag urgent issues.'")
    );
    assert!(manifest.contains("Ask before changing a ticket."));
    assert_eq!(body["model"], "model-a");
    assert_eq!(body["saved"], false);

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let request: Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(request["model"], "model-a");
    let input = request["messages"][1]["content"].as_str().unwrap();
    assert!(input.contains("Read each new ticket"));
    assert!(input.contains("Ask before editing or replying to a ticket"));
    assert!(input.ends_with("\n\n/no_think"));
}

#[tokio::test]
async fn a_model_the_user_cannot_access_is_refused_before_the_model_call() {
    let server = upstream().await;
    let temp = tempfile::tempdir().unwrap();
    let state = Arc::new(
        common::state_with_chat_pool_and_user_skills(&server.uri(), temp.path().to_path_buf())
            .await,
    );
    let cookie = common::seed_session(&state, "alice", "alice@example.com").await;

    let (status, body) = draft(&state, &cookie, "not-available").await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_draft_request_requires_a_signed_in_user() {
    let server = upstream().await;
    let state = Arc::new(common::state_with_chat_pool(&server.uri()).await);

    let response = router(state)
        .serve(common::post_json(
            DRAFT,
            "",
            r#"{"name":"Check","goal":"Review","steps":["Read"],"stop_conditions":"Ask","model":"model-a"}"#,
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(server.received_requests().await.unwrap().is_empty());
}
