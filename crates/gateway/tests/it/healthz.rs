// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! /healthz / /readyz are unauthenticated and always 200. The simplest
//! test in the suite — also doubles as a smoke check that the test
//! scaffolding (state, router, serve) hangs together.

use crate::common;

use common::Service as _;
use rama::http::{Method, StatusCode};

#[tokio::test]
async fn healthz_returns_ok() {
    let state = common::state_with_chat_pool("http://unused.invalid").await;
    let app = common::app(state);
    let resp = app
        .serve(common::req(Method::GET, "/healthz"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = common::read_body(resp).await;
    assert!(
        body.starts_with(b"{\"status\":\"ok\""),
        "unexpected body: {body:?}"
    );
}

#[tokio::test]
async fn readyz_returns_ok() {
    let state = common::state_with_chat_pool("http://unused.invalid").await;
    let app = common::app(state);
    let resp = app
        .serve(common::req(Method::GET, "/readyz"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

/// An unknown path is the SPA's, not a 404.
///
/// The SvelteKit app owns the root and resolves its own routes, so the server
/// cannot know whether `/no-such-route` is a client route or a typo — it hands
/// back the SPA entry point and lets the client router decide. In this
/// harness no build is deployed (`GATEWAY_STATIC_DIR` is unset), so that path
/// answers 503 "not deployed" rather than the index. Either way it is the SPA
/// handler answering, which is what this pins: an API 404 would mean the
/// catch-all had stopped matching.
#[tokio::test]
async fn an_unknown_path_falls_through_to_the_spa() {
    let state = common::state_with_chat_pool("http://unused.invalid").await;
    let app = common::app(state);
    let resp = app
        .serve(common::req(Method::GET, "/no-such-route"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}
