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
/// The catch-all still matches every unclaimed path — but with no SPA
/// deployed in this harness it answers 503 "not deployed", the operator
/// signal, rather than pretending the path exists.
#[tokio::test]
async fn an_unknown_path_reaches_the_spa_handler() {
    // An API 404 here would mean the catch-all had stopped matching this
    // shape. (Once a build IS deployed, `spa::serve` narrows further: only
    // the client router's own routes get the shell — see its unit tests.)
    let state = common::state_with_chat_pool("http://unused.invalid").await;
    let app = common::app(state);
    let resp = app
        .serve(common::req(Method::GET, "/no-such-route"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}
