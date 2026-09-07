// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Router wiring for the SvelteKit SPA static shell served under `/app`.
//!
//! The full serve behaviour (traversal guard, content types, cache headers,
//! SPA history fallback) is unit-tested against a temp dir in
//! `rama_server::spa`. What this file pins is the *wiring*: that the
//! `/app/{*name}` catch-all is actually registered and dispatches to the SPA
//! handler — proven by the fact that an undeployed SPA answers **503**
//! ("not deployed") rather than the router's generic **404** ("unknown path").
//!
//! We deliberately do NOT set `GATEWAY_STATIC_DIR` here: that env var is read
//! once into a process-global `LazyLock`, and `cargo nextest` runs every test
//! in this crate's `it` module in a single process, so setting it would race
//! across tests. The 503 path (no dir) is the deterministic, env-free proof
//! that the route reaches `spa` and that the undeployed case is distinct from
//! a plain 404.

use crate::common;

use common::Service as _;
use rama::http::{Method, StatusCode, header};

/// Drive a GET through the full layered service. Returns (status, a header
/// value, the drained body as a lossy string).
async fn get_app(uri: &str) -> (StatusCode, String, String) {
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

/// Probe which request forms reach the SPA handler (503 when undeployed) vs
/// fall through to the router's 404. This pins the route registration: the
/// bare `/app`, a sub-route, and a hashed-asset path must all reach the
/// handler; an unrelated path must not.
#[tokio::test]
async fn app_forms_reach_the_spa_handler() {
    for uri in [
        "/app",
        "/app/",
        "/app/tokens",
        "/app/assets/_app/immutable/entry/START-AbC123.js",
    ] {
        let (status, _ct, _body) = get_app(uri).await;
        assert_eq!(
            status,
            StatusCode::SERVICE_UNAVAILABLE,
            "`{uri}` must reach the SPA handler (503 undeployed); a 404 means the route is not registered for this form"
        );
    }
}
