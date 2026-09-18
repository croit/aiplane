// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use crate::common;

use common::Service as _;
use rama::http::{Method, StatusCode};

#[tokio::test]
async fn backend_serves_generated_openapi_for_every_api_route() {
    let state = common::state_with_chat_pool("http://unused.invalid").await;
    let app = common::app(state);
    let response = app
        .serve(common::req(Method::GET, "/openapi.json"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "application/json"
    );
    let body = common::read_body(response).await;
    let document: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(document["openapi"], "3.1.0");

    let paths = document["paths"].as_object().unwrap();
    assert!(paths.len() > 100);
    assert!(paths["/api/v0/tokens/details"]["get"]["responses"]["200"].is_object());
    assert!(paths["/api/v0/rag/collections/{id}"]["patch"]["responses"]["200"].is_object());
    assert!(paths["/api/v0/chat/sessions/{id}/events"]["get"]["responses"]["200"].is_object());
}

#[test]
fn detached_openapi_document_does_not_exist() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    assert!(!manifest.join("../../docs/openapi.json").exists());
}
