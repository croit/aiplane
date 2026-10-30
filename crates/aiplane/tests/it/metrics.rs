// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! `GET /metrics` — the guards (off, unguarded, token, IP list, both), the
//! client address behind a trusted proxy, and what the body may contain.
//! The settings are written the way the admin save writes them and then
//! reloaded, which is all a save does to make them live.

use std::collections::HashMap;

use crate::common;

use aiplane::rama_server::RamaState;
use aiplane_core::server::db;
use aiplane_core::server::settings;
use aiplane_core::server::trusted_proxies::TrustedProxies;
use aiplane_core::server::upstreams::{
    PickerStrategy, PoolKind, UpstreamPoolConfig, UpstreamRegistry,
};
use common::Service as _;
use rama::extensions::ExtensionsRef as _;
use rama::http::{Body, Method, Request, StatusCode, header};

const UPSTREAM_URL: &str = "http://gpu-host.internal.invalid:8000/v1";
const UPSTREAM_KEY: &str = "sk-upstream-secret-key";
const TOKEN: &str = "scrape-token-1";

async fn state() -> RamaState {
    let db_pool = db::open(std::path::Path::new(":memory:")).await.unwrap();
    let mut backend = common::mock_backend("gpu0", UPSTREAM_URL);
    backend.api_key = Some(UPSTREAM_KEY.into());
    let pools = HashMap::from([(
        "chat-pool".to_string(),
        UpstreamPoolConfig {
            voices: Default::default(),
            offer_voices: Vec::new(),
            allowed_groups: Vec::new(),
            fallback_offline: None,
            level_rank: 0,
            enforce_limits: true,
            kind: PoolKind::Chat,
            strategy: PickerStrategy::RoundRobin,
            models: vec!["model-a".into()],
            backend: vec![backend],
        },
    )]);
    let registry = UpstreamRegistry::new(&pools).unwrap();
    common::state_from_registry(db_pool, registry)
}

async fn configure(state: &RamaState, pairs: &[(&str, &str)]) {
    let pairs: Vec<(String, String)> = pairs
        .iter()
        .map(|(k, v)| {
            let value = settings::field(k).unwrap().check(v).unwrap();
            ((*k).to_string(), value)
        })
        .collect();
    settings::store(&state.db, &state.crypto, &pairs)
        .await
        .unwrap();
    state.reload_settings().await;
}

fn scrape(peer: &str, headers: &[(&str, &str)]) -> Request {
    let mut builder = Request::builder().method(Method::GET).uri("/metrics");
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    let req = builder.body(Body::empty()).unwrap();
    req.extensions().insert(rama::net::stream::SocketInfo::new(
        None,
        rama::net::address::SocketAddress::new(peer.parse::<std::net::IpAddr>().unwrap(), 4000),
    ));
    req
}

async fn status_of(state: &RamaState, req: Request) -> StatusCode {
    common::app(state.clone())
        .serve(req)
        .await
        .unwrap()
        .status()
}

const BEARER: (&str, &str) = ("authorization", "Bearer scrape-token-1");

/// Status, content type and body: what tells one answer from another.
async fn answer(state: &RamaState, req: Request) -> (StatusCode, Option<String>, Vec<u8>) {
    let resp = common::app(state.clone()).serve(req).await.unwrap();
    let content_type = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .map(|v| v.to_str().unwrap().to_string());
    (
        resp.status(),
        content_type,
        common::read_body(resp).await.to_vec(),
    )
}

/// The answer a path nobody serves gets from the router's catch-all.
async fn unknown_route(state: &RamaState) -> (StatusCode, Option<String>, Vec<u8>) {
    let mut req = scrape("10.0.0.1", &[BEARER]);
    *req.uri_mut() = "/metricz".parse().unwrap();
    answer(state, req).await
}

#[tokio::test]
async fn off_by_default_and_off_again_after_a_save() {
    let state = state().await;
    assert_eq!(
        answer(&state, scrape("10.0.0.1", &[BEARER])).await,
        unknown_route(&state).await,
        "switched off, /metrics answers like a path that does not exist"
    );

    configure(
        &state,
        &[("metrics.enabled", "true"), ("metrics.token", TOKEN)],
    )
    .await;
    assert_eq!(
        status_of(&state, scrape("10.0.0.1", &[BEARER])).await,
        StatusCode::OK,
        "a saved setting applies to the next scrape, without a restart"
    );

    configure(&state, &[("metrics.enabled", "false")]).await;
    assert_eq!(
        answer(&state, scrape("10.0.0.1", &[BEARER])).await,
        unknown_route(&state).await
    );
}

#[tokio::test]
async fn switched_on_without_a_guard_answers_like_an_unknown_route() {
    let state = state().await;
    configure(&state, &[("metrics.enabled", "true")]).await;
    assert_eq!(
        answer(&state, scrape("127.0.0.1", &[])).await,
        unknown_route(&state).await
    );
}

#[tokio::test]
async fn an_unreadable_stored_ip_list_refuses_every_scrape() {
    let state = state().await;
    configure(
        &state,
        &[("metrics.enabled", "true"), ("metrics.token", TOKEN)],
    )
    .await;
    // A hand-edited row: a bare value where the save writes a JSON list.
    db::app_settings::set(&state.db, "settings.metrics.allowed_ips", "10.0.0.0/8")
        .await
        .unwrap();
    state.reload_settings().await;
    assert_eq!(
        status_of(&state, scrape("10.0.0.1", &[BEARER])).await,
        StatusCode::FORBIDDEN,
        "an IP guard the gateway cannot read must not turn into no IP guard"
    );
}

#[tokio::test]
async fn a_token_guard_wants_the_exact_bearer() {
    let state = state().await;
    configure(
        &state,
        &[("metrics.enabled", "true"), ("metrics.token", TOKEN)],
    )
    .await;

    let resp = common::app(state.clone())
        .serve(scrape("192.0.2.1", &[]))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(resp.headers()[header::WWW_AUTHENTICATE], "Bearer");
    let body: serde_json::Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(body["error"]["code"], "unauthorized", "{body}");

    assert_eq!(
        status_of(
            &state,
            scrape("192.0.2.1", &[("authorization", "Bearer wrong")])
        )
        .await,
        StatusCode::UNAUTHORIZED
    );
    for header in ["bearer scrape-token-1", "BEARER scrape-token-1"] {
        assert_eq!(
            status_of(&state, scrape("192.0.2.1", &[("authorization", header)])).await,
            StatusCode::OK,
            "the scheme is case-insensitive: {header}"
        );
    }
    assert_eq!(
        status_of(
            &state,
            scrape("192.0.2.1", &[("authorization", "bearer SCRAPE-TOKEN-1")])
        )
        .await,
        StatusCode::UNAUTHORIZED,
        "the token is not"
    );

    let resp = common::app(state.clone())
        .serve(scrape("192.0.2.1", &[BEARER]))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()[header::CONTENT_TYPE],
        "text/plain; version=0.0.4; charset=utf-8"
    );
    let body = String::from_utf8(common::read_body(resp).await.to_vec()).unwrap();
    assert!(
        body.contains(r#"aiplane_backend_up{pool="chat-pool",backend="gpu0"} 1"#),
        "{body}"
    );
    assert!(body.contains("# TYPE aiplane_backend_dispatched_total counter"));
    assert!(body.contains("aiplane_build_info{version=\""));
    assert!(!body.contains("gpu-host.internal.invalid"), "{body}");
    assert!(!body.contains(UPSTREAM_KEY), "{body}");
    assert!(!body.contains(TOKEN), "{body}");
}

#[tokio::test]
async fn an_ip_guard_admits_only_its_networks() {
    let state = state().await;
    configure(
        &state,
        &[
            ("metrics.enabled", "true"),
            ("metrics.allowed_ips", "10.0.0.0/8, 2001:db8::/32"),
        ],
    )
    .await;
    let resp = common::app(state.clone())
        .serve(scrape("192.0.2.1", &[]))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body: serde_json::Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(body["error"]["code"], "forbidden", "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("AIPLANE_TRUSTED_PROXIES"),
        "{body}"
    );

    assert_eq!(
        status_of(&state, scrape("10.20.30.40", &[])).await,
        StatusCode::OK
    );
    assert_eq!(
        status_of(&state, scrape("2001:db8::9", &[])).await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn with_both_guards_a_scrape_must_pass_both() {
    let state = state().await;
    configure(
        &state,
        &[
            ("metrics.enabled", "true"),
            ("metrics.token", TOKEN),
            ("metrics.allowed_ips", "10.0.0.0/8"),
        ],
    )
    .await;
    assert_eq!(
        status_of(&state, scrape("192.0.2.1", &[BEARER])).await,
        StatusCode::FORBIDDEN,
        "the right token from the wrong address"
    );
    assert_eq!(
        status_of(
            &state,
            scrape("10.0.0.7", &[("authorization", "Bearer wrong")])
        )
        .await,
        StatusCode::UNAUTHORIZED,
        "the wrong token from the right address"
    );
    assert_eq!(
        status_of(&state, scrape("10.0.0.7", &[BEARER])).await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn behind_a_trusted_proxy_the_forwarded_client_is_checked() {
    let state = state()
        .await
        .with_trusted_proxies(TrustedProxies::parse("172.16.0.0/12").unwrap());
    configure(
        &state,
        &[
            ("metrics.enabled", "true"),
            ("metrics.allowed_ips", "10.0.0.0/8"),
        ],
    )
    .await;
    assert_eq!(
        status_of(
            &state,
            scrape("172.16.0.2", &[("x-forwarded-for", "10.1.1.1")])
        )
        .await,
        StatusCode::OK,
        "the proxy vouches for an allowed client"
    );
    assert_eq!(
        status_of(
            &state,
            scrape("172.16.0.2", &[("x-forwarded-for", "192.0.2.1")])
        )
        .await,
        StatusCode::FORBIDDEN,
        "the proxy's own address is not what is checked"
    );
    assert_eq!(
        status_of(
            &state,
            scrape("192.0.2.1", &[("x-forwarded-for", "10.1.1.1")])
        )
        .await,
        StatusCode::FORBIDDEN,
        "an untrusted peer cannot claim an allowed address"
    );
}

#[tokio::test]
async fn the_endpoint_is_not_a_cors_surface() {
    let state = state().await;
    configure(
        &state,
        &[("metrics.enabled", "true"), ("metrics.token", TOKEN)],
    )
    .await;
    let resp = common::app(state)
        .serve(scrape(
            "192.0.2.1",
            &[BEARER, ("origin", "https://elsewhere.example")],
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        resp.headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none()
    );
}
