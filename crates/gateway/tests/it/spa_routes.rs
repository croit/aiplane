// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Router wiring for the SvelteKit SPA, which serves the root.
//!
//! The serve behaviour itself (traversal guard, content types, cache headers,
//! history fallback) is unit-tested against a temp dir in `rama_server::spa`.
//! What this file pins is the *wiring*: that the catch-all is registered, that
//! it is registered LAST so it does not shadow the API, and that a client
//! route with no file behind it still reaches the SPA rather than 404ing.
//!
//! Proven through the undeployed-503 path: with no `GATEWAY_STATIC_DIR` the
//! handler answers 503 "not deployed", which is distinguishable from the
//! router's own 404. We deliberately do NOT set that env var here — it is read
//! once into a process-global `LazyLock`, and `cargo nextest` runs this crate's
//! `it` module in one process, so setting it would race across tests.

use crate::common;

use common::Service as _;
use rama::http::{Method, StatusCode, header};

/// Drive a GET through the full layered service.
async fn get(uri: &str) -> (StatusCode, String, String) {
    let state = common::state_with_chat_pool("http://unused.invalid").await;
    let app = common::app(state);
    let resp = app.serve(common::req(Method::GET, uri)).await.unwrap();
    let status = resp.status();
    let ct = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let body = String::from_utf8_lossy(&common::read_body(resp).await).to_string();
    (status, ct, body)
}

/// Every shape of request the SPA owns reaches its handler: the bare root, a
/// client route with no file behind it, and a content-hashed asset path
/// (whose case must survive the router, since the filename is the cache key).
#[tokio::test]
async fn the_spa_owns_every_unclaimed_path() {
    for uri in [
        "/",
        "/chat",
        "/admin/settings",
        "/_app/immutable/entry/START-AbC123.js",
    ] {
        let (status, _ct, _body) = get(uri).await;
        assert_eq!(
            status,
            StatusCode::SERVICE_UNAVAILABLE,
            "`{uri}` must reach the SPA handler (503 undeployed); a 404 means the catch-all is not \
             matching this shape"
        );
    }
}

/// …but it must not shadow the surfaces registered before it. The catch-all is
/// last precisely so these still answer; if it ever moved up, the whole API
/// would start returning the SPA shell with a 200 and every client would break
/// in a way no single test would obviously explain.
#[tokio::test]
async fn the_catch_all_does_not_shadow_the_api() {
    let (status, ct, _) = get("/healthz").await;
    assert_eq!(status, StatusCode::OK);
    assert!(ct.contains("json"), "healthz must still be JSON, got {ct}");

    // Session-gated, so anonymous is a 401 envelope — not the SPA's 503.
    let (status, _, _) = get("/api/v0/me").await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "the JSON API must answer for itself, not fall through to the SPA"
    );

    let (status, ct, body) = get("/api/v0/build").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "build metadata must be public for the login page"
    );
    assert!(ct.contains("json"));
    let build: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(
        build["source_url"]
            .as_str()
            .unwrap()
            .starts_with("https://")
    );
    assert!(build["version"].as_str().unwrap().starts_with('v'));
}
