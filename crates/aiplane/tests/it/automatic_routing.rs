// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use crate::common;

use aiplane_core::server::db::automatic_routes::{self, AutomaticRoute, AutomaticRouteCandidate};
use aiplane_core::server::db::limits::ManagedBy;
use aiplane_core::server::db::token_models;
use common::Service as _;
use rama::http::{Body, Method, Request, StatusCode};
use serde_json::{Value, json};
use std::time::Duration;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn route(rollout: &str, minimum_confidence: f64) -> AutomaticRoute {
    AutomaticRoute {
        alias: "default".into(),
        selector_model: "jev-model".into(),
        objective: "balanced".into(),
        instructions: "Use expert for complex software engineering.".into(),
        minimum_confidence,
        selector_timeout_ms: 1_000,
        fallback_target: "fast-model".into(),
        session_affinity: false,
        session_ttl_seconds: 3_600,
        rollout: rollout.into(),
        version: 0,
        candidates: vec![
            AutomaticRouteCandidate {
                key: "fast".into(),
                target: "fast-model".into(),
                description: "Fast inexpensive general model".into(),
            },
            AutomaticRouteCandidate {
                key: "expert".into(),
                target: "expert-model".into(),
                description: "Strong software engineering model".into(),
            },
        ],
    }
}

async fn request(state: aiplane::rama_server::RamaState) -> rama::http::Response {
    let bearer = common::seed_user_with_token(&state, "alice").await;
    common::app(state)
        .serve(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/chat/completions")
                .header("authorization", format!("Bearer {bearer}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "model": "default",
                        "messages": [{"role": "user", "content": "Fix this Rust lifetime"}]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn mount_selector(upstream: &MockServer, confidence: f64) {
    Mock::given(method("POST"))
        .and(path("/systemone"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "model": "jev-model",
            "answers": {
                "route": {
                    "type": "choice",
                    "choice": "expert",
                    "confidence": confidence,
                    "probabilities": {"fast": 1.0 - confidence, "expert": confidence}
                }
            },
            "usage": {"input_tokens": 20, "output_tokens": 2}
        })))
        .mount(upstream)
        .await;
}

async fn wait_for_latest_decision_reason(db: &aiplane_core::server::db::Pool) -> String {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Some(reason) = sqlx::query_scalar(
                "SELECT reason FROM automatic_route_decisions ORDER BY id DESC LIMIT 1",
            )
            .fetch_optional(db)
            .await
            .unwrap()
            {
                return reason;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("automatic routing decision was not persisted within 2s")
}

#[tokio::test]
async fn active_route_selects_a_candidate_and_exposes_the_decision() {
    let upstream = MockServer::start().await;
    mount_selector(&upstream, 0.92).await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_json(json!({
            "model": "expert-model",
            "messages": [{"role": "user", "content": "Fix this Rust lifetime"}]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "model": "expert-model",
            "choices": [{"message": {"role": "assistant", "content": "done"}}]
        })))
        .mount(&upstream)
        .await;
    let state = common::state_with_automatic_route_pools(&upstream.uri()).await;
    automatic_routes::upsert(&state.db, &route("active", 0.7))
        .await
        .unwrap();

    let response = request(state).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("x-gateway-route-target")
            .and_then(|value| value.to_str().ok()),
        Some("expert-model")
    );
    assert_eq!(
        response
            .headers()
            .get("x-gateway-route-reason")
            .and_then(|value| value.to_str().ok()),
        Some("selected")
    );
    assert_eq!(
        response
            .headers()
            .get("x-gateway-resolved-model")
            .and_then(|value| value.to_str().ok()),
        Some("expert-model")
    );
}

#[tokio::test]
async fn shadow_route_records_the_suggestion_but_sends_the_fallback() {
    let upstream = MockServer::start().await;
    mount_selector(&upstream, 0.97).await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_json(json!({
            "model": "fast-model",
            "messages": [{"role": "user", "content": "Fix this Rust lifetime"}]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "model": "fast-model",
            "choices": [{"message": {"role": "assistant", "content": "fallback"}}]
        })))
        .mount(&upstream)
        .await;
    let state = common::state_with_automatic_route_pools(&upstream.uri()).await;
    let db = state.db.clone();
    automatic_routes::upsert(&db, &route("shadow", 0.7))
        .await
        .unwrap();

    let response = request(state).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("x-gateway-route-target")
            .and_then(|value| value.to_str().ok()),
        Some("fast-model")
    );
    assert_eq!(
        response
            .headers()
            .get("x-gateway-route-suggested-target")
            .and_then(|value| value.to_str().ok()),
        Some("expert-model")
    );
    let reason = wait_for_latest_decision_reason(&db).await;
    assert_eq!(reason, "shadow");
}

#[tokio::test]
async fn low_confidence_uses_the_configured_fallback() {
    let upstream = MockServer::start().await;
    mount_selector(&upstream, 0.4).await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "model": "fast-model",
            "choices": [{"message": {"role": "assistant", "content": "fallback"}}]
        })))
        .mount(&upstream)
        .await;
    let state = common::state_with_automatic_route_pools(&upstream.uri()).await;
    automatic_routes::upsert(&state.db, &route("active", 0.8))
        .await
        .unwrap();

    let response = request(state).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("x-gateway-route-reason")
            .and_then(|value| value.to_str().ok()),
        Some("low_confidence")
    );
    let body: Value = serde_json::from_slice(&common::read_body(response).await).unwrap();
    assert_eq!(body["model"], "fast-model");
}

#[tokio::test]
async fn malformed_selector_response_uses_the_configured_fallback() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/systemone"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"answers": {}})))
        .mount(&upstream)
        .await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "model": "fast-model",
            "choices": [{"message": {"role": "assistant", "content": "fallback"}}]
        })))
        .mount(&upstream)
        .await;
    let state = common::state_with_automatic_route_pools(&upstream.uri()).await;
    automatic_routes::upsert(&state.db, &route("active", 0.7))
        .await
        .unwrap();

    let response = request(state).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("x-gateway-route-reason")
            .and_then(|value| value.to_str().ok()),
        Some("selector_error")
    );
}

#[tokio::test]
async fn selector_timeout_uses_the_configured_fallback() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/systemone"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(std::time::Duration::from_millis(250))
                .set_body_json(json!({})),
        )
        .mount(&upstream)
        .await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "model": "fast-model",
            "choices": [{"message": {"role": "assistant", "content": "fallback"}}]
        })))
        .mount(&upstream)
        .await;
    let state = common::state_with_automatic_route_pools(&upstream.uri()).await;
    let mut policy = route("active", 0.7);
    policy.selector_timeout_ms = 100;
    automatic_routes::upsert(&state.db, &policy).await.unwrap();

    let response = request(state).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("x-gateway-route-reason")
            .and_then(|value| value.to_str().ok()),
        Some("selector_error")
    );
}

#[tokio::test]
async fn anthropic_messages_use_the_same_automatic_route() {
    let upstream = MockServer::start().await;
    mount_selector(&upstream, 0.95).await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-auto",
            "model": "expert-model",
            "choices": [{
                "index": 0,
                "finish_reason": "stop",
                "message": {"role": "assistant", "content": "selected"}
            }],
            "usage": {"prompt_tokens": 4, "completion_tokens": 1, "total_tokens": 5}
        })))
        .mount(&upstream)
        .await;
    let state = common::state_with_automatic_route_pools(&upstream.uri()).await;
    automatic_routes::upsert(&state.db, &route("active", 0.7))
        .await
        .unwrap();
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let response = common::app(state)
        .serve(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/messages")
                .header("authorization", format!("Bearer {bearer}"))
                .header("content-type", "application/json")
                .header("anthropic-version", "2023-06-01")
                .body(Body::from(
                    json!({
                        "model": "default",
                        "max_tokens": 256,
                        "messages": [{"role": "user", "content": "Fix this Rust lifetime"}]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("x-gateway-route-target")
            .and_then(|value| value.to_str().ok()),
        Some("expert-model")
    );
    let body: Value = serde_json::from_slice(&common::read_body(response).await).unwrap();
    assert_eq!(body["model"], "default");
    assert_eq!(body["content"][0]["text"], "selected");
}

#[tokio::test]
async fn anthropic_token_count_uses_the_route_and_exposes_the_decision() {
    let upstream = MockServer::start().await;
    mount_selector(&upstream, 0.95).await;
    Mock::given(method("POST"))
        .and(path("/tokenize"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"count": 42})))
        .mount(&upstream)
        .await;
    let state = common::state_with_automatic_route_pools(&upstream.uri()).await;
    automatic_routes::upsert(&state.db, &route("active", 0.7))
        .await
        .unwrap();
    let bearer = common::seed_user_with_token(&state, "alice").await;

    let response = common::app(state)
        .serve(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/messages/count_tokens")
                .header("authorization", format!("Bearer {bearer}"))
                .header("content-type", "application/json")
                .header("anthropic-version", "2023-06-01")
                .body(Body::from(
                    json!({
                        "model": "default",
                        "messages": [{"role": "user", "content": "Fix this Rust lifetime"}]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("x-gateway-route-target")
            .and_then(|value| value.to_str().ok()),
        Some("expert-model")
    );
    let body: Value = serde_json::from_slice(&common::read_body(response).await).unwrap();
    assert_eq!(body["input_tokens"], 42);
}

#[tokio::test]
async fn anthropic_token_count_does_not_bypass_quota_for_selector_inference() {
    use aiplane_core::server::db::limits::{self, Dimension, SubjectType, Window};

    let upstream = MockServer::start().await;
    mount_selector(&upstream, 0.95).await;
    let state = common::state_with_automatic_route_pools(&upstream.uri()).await;
    automatic_routes::upsert(&state.db, &route("active", 0.7))
        .await
        .unwrap();
    let bearer = common::seed_user_with_token(&state, "alice").await;
    limits::upsert(
        &state.db,
        SubjectType::Global,
        "",
        None,
        Dimension::Requests,
        Window::Hour,
        0.0,
    )
    .await
    .unwrap();

    let response = common::app(state)
        .serve(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/messages/count_tokens")
                .header("authorization", format!("Bearer {bearer}"))
                .header("content-type", "application/json")
                .header("anthropic-version", "2023-06-01")
                .body(Body::from(
                    json!({
                        "model": "default",
                        "messages": [{"role": "user", "content": "hello"}]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    let requests = upstream.received_requests().await.unwrap();
    assert!(
        requests.is_empty(),
        "quota rejection must happen before selector inference"
    );
}

#[tokio::test]
async fn a_token_can_grant_the_virtual_alias_without_granting_its_internals() {
    let upstream = MockServer::start().await;
    mount_selector(&upstream, 0.95).await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "model": "expert-model",
            "choices": [{"message": {"role": "assistant", "content": "selected"}}]
        })))
        .mount(&upstream)
        .await;
    let state = common::state_with_automatic_route_pools(&upstream.uri()).await;
    automatic_routes::upsert(&state.db, &route("active", 0.7))
        .await
        .unwrap();
    let (bearer, token_id) = common::seed_user_with_token_id(&state, "alice").await;
    token_models::set_for_token(&state.db, &token_id, &["default".into()], ManagedBy::Owner)
        .await
        .unwrap();
    let app = common::app(state);

    let listed = app
        .serve(
            Request::builder()
                .method(Method::GET)
                .uri("/v1/models")
                .header("authorization", format!("Bearer {bearer}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let listed: Value = serde_json::from_slice(&common::read_body(listed).await).unwrap();
    let ids: Vec<&str> = listed["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|model| model["id"].as_str())
        .collect();
    assert_eq!(ids, vec!["default"]);

    let retrieved = app
        .serve(
            Request::builder()
                .method(Method::GET)
                .uri("/v1/models/default")
                .header("authorization", format!("Bearer {bearer}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(retrieved.status(), StatusCode::OK);

    let response = app
        .serve(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/chat/completions")
                .header("authorization", format!("Bearer {bearer}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"model": "default", "messages": [{"role": "user", "content": "code"}]})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}
