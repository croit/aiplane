// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Router wiring for the debug-only `/__dev/*` seeding endpoints
//! (`rama_server::dev_seed`).
//!
//! The fixture logic itself (canonical three tokens, fixture user) is
//! unit-tested in the module. What this file pins is the *wiring*: the
//! routes answer through the full layered service, the seeded cookie is a
//! real signed session that unlocks `/tokens`, and — the reason the
//! endpoints exist at all — a fresh database is ready afterwards
//! (`/readyz` flips from `setup_required` to `ok`).
//!
//! These routes are `cfg(debug_assertions)`, and so are these tests: the
//! `it` module compiles under the dev profile, where they exist.

#![cfg(debug_assertions)]

use crate::common;

use common::Service as _;
use rama::http::{Method, StatusCode, header};

#[tokio::test]
async fn seed_session_signs_in_and_serves_the_fixture_through_the_router() {
    let state = common::state_with_chat_pool("http://unused.invalid").await;
    let app = common::app(state);

    let resp = app
        .serve(common::req(Method::GET, "/__dev/seed-session"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        resp.headers()
            .get(header::LOCATION)
            .and_then(|v| v.to_str().ok()),
        Some("/tokens"),
        "the seeded session should land where the fixture is visible"
    );
    let cookie = resp
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .expect("a Set-Cookie header");
    let (name, _) = cookie.split_once('=').expect("cookie has a value");
    assert_eq!(name, "id", "the ordinary session cookie, nothing bespoke");

    let mut authed = common::req(Method::GET, "/tokens");
    authed.headers_mut().insert(
        header::COOKIE,
        header::HeaderValue::from_str(&format!("id={}", &cookie[3..])).unwrap(),
    );
    let resp = app.serve(authed).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = String::from_utf8_lossy(&common::read_body(resp).await).to_string();
    for expected in [
        "Signed in as alice@example.com",
        "engineering, admin",
        "Local laptop",
        "CI pipeline",
        "Production API",
    ] {
        assert!(
            body.contains(expected),
            "expected {expected:?} in the rendered page"
        );
    }

    // Anonymous /tokens still bounces to sign-in: seeding added a session,
    // it did not open the page up.
    let resp = app
        .serve(common::req(Method::GET, "/tokens"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn the_login_only_endpoint_mints_a_session_without_touching_tokens() {
    let state = common::state_with_chat_pool("http://unused.invalid").await;
    let app = common::app(state.clone());

    // Something a previous test run might have left behind: an extra token
    // must survive /__dev/session — only /__dev/seed-session may delete.
    let pool = state.db.clone();
    let now = jiff::Timestamp::now();
    gateway_core::server::db::users::upsert(
        &pool,
        &gateway_core::server::db::users::User {
            id: "alice".into(),
            email: "alice@example.com".into(),
            name: None,
            roles: vec![],
            created_at: now,
            updated_at: now,
            timezone: None,
            speech_voice: None,
        },
    )
    .await
    .unwrap();
    gateway_core::server::db::tokens::insert(
        &pool,
        &gateway_core::server::db::tokens::Token {
            id: "preexisting".into(),
            user_id: "alice".into(),
            name: "Pre-existing".into(),
            hash: "h".into(),
            created_at: now,
            last_used_at: None,
            expires_at: now + jiff::SignedDuration::from_hours(24),
            revoked_at: None,
            tools_enabled: false,
        },
    )
    .await
    .unwrap();

    let resp = app
        .serve(common::req(Method::GET, "/__dev/session"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    assert!(resp.headers().contains_key(header::SET_COOKIE));

    let names: Vec<String> = gateway_core::server::db::tokens::list_for_user(&pool, "alice")
        .await
        .unwrap()
        .into_iter()
        .map(|t| t.name)
        .collect();
    assert_eq!(names, vec!["Pre-existing".to_string()]);
}
