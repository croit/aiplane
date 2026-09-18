// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! TypeSafe System One compatibility at `POST /v1/systemone`.

use crate::common;

use aiplane_core::server::upstreams::PoolKind;
use common::Service as _;
use rama::http::{Body, Method, Request, StatusCode};
use serde_json::{Value, json};
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn request(bearer: Option<&str>, body: Value) -> Request {
    let mut builder = Request::builder()
        .method(Method::POST)
        .uri("/v1/systemone")
        .header("content-type", "application/json");
    if let Some(bearer) = bearer {
        builder = builder.header("authorization", format!("Bearer {bearer}"));
    }
    builder.body(Body::from(body.to_string())).unwrap()
}

#[tokio::test]
async fn anonymous_request_is_401() {
    let state =
        common::state_with_pool("http://unused.invalid", PoolKind::SystemOne, "jev-latest").await;
    let app = common::app(state);
    let resp = app
        .serve(request(
            None,
            json!({"model": "jev-latest", "state": "hello", "questions": {}}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn relays_the_system_one_contract_without_translating_it() {
    let upstream = MockServer::start().await;
    let upstream_request = json!({
        "model": "jev-1.13.0",
        "state": {"ticket": "My card was charged twice."},
        "questions": {
            "urgent": {
                "type": "noul",
                "instructions": "Does this need an immediate response?",
                "criteria": {"true": "Urgent", "false": "Can wait"}
            },
            "team": {
                "type": "choice",
                "instructions": "Which team owns this?",
                "criteria": {"billing": null, "technical": null}
            }
        },
        "future_option": {"kept": true}
    });
    let upstream_response = json!({
        "model": "jev-1.13.0",
        "answers": {
            "urgent": {"type": "noul", "noul": 0.93},
            "team": {
                "type": "choice",
                "choice": "billing",
                "confidence": 0.88,
                "probabilities": {"billing": 0.94, "technical": 0.06}
            }
        },
        "usage": {"input_tokens": 41, "output_tokens": 7}
    });
    Mock::given(method("POST"))
        .and(path("/systemone"))
        .and(body_json(upstream_request.clone()))
        .respond_with(ResponseTemplate::new(200).set_body_json(upstream_response.clone()))
        .mount(&upstream)
        .await;

    let state = common::state_with_pool(&upstream.uri(), PoolKind::SystemOne, "jev-1.13.0").await;
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);
    let resp = app
        .serve(request(Some(&bearer), upstream_request))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(body, upstream_response);
}

#[tokio::test]
async fn rewrites_a_gateway_alias_to_the_openrouter_model_id() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/systemone"))
        .and(body_json(json!({
            "model": "typesafe/jev-1.13",
            "state": "hello",
            "questions": {"positive": {"type": "noul"}}
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "model": "typesafe/jev-1.13",
            "answers": {"positive": {"type": "noul", "noul": 0.5}},
            "usage": {"input_tokens": 3, "output_tokens": 1}
        })))
        .mount(&upstream)
        .await;

    let state = common::state_with_alias_for_kind(
        &upstream.uri(),
        PoolKind::SystemOne,
        "jev-latest",
        "typesafe/jev-1.13",
    )
    .await;
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);
    let resp = app
        .serve(request(
            Some(&bearer),
            json!({
                "model": "jev-latest",
                "state": "hello",
                "questions": {"positive": {"type": "noul"}}
            }),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get("x-gateway-resolved-model")
            .and_then(|value| value.to_str().ok()),
        Some("typesafe/jev-1.13")
    );
}

#[tokio::test]
async fn records_system_one_usage() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/systemone"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "model": "jev-1.13.0",
            "answers": {"positive": {"type": "noul", "noul": 0.8}},
            "usage": {"input_tokens": 17, "output_tokens": 2}
        })))
        .mount(&upstream)
        .await;
    let state = common::state_with_pool(&upstream.uri(), PoolKind::SystemOne, "jev-1.13.0").await;
    let metered = aiplane_core::server::usage::spawn(state.db.clone(), 90);
    let state = state.with_usage(metered);
    let db = state.db.clone();
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);
    let resp = app
        .serve(request(
            Some(&bearer),
            json!({
                "model": "jev-1.13.0",
                "state": "hello",
                "questions": {"positive": {"type": "noul"}}
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let _ = common::read_body(resp).await;
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;

    let kind: String = sqlx::query_scalar("SELECT kind FROM usage_events LIMIT 1")
        .fetch_one(&db)
        .await
        .unwrap();
    let total_tokens: i64 = sqlx::query_scalar("SELECT total_tokens FROM usage_events LIMIT 1")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(kind, "system_one");
    assert_eq!(total_tokens, 19);
}

#[tokio::test]
async fn relays_upstream_errors_and_retry_after() {
    let upstream = MockServer::start().await;
    let error = json!({
        "error": {
            "message": "rate limit exceeded",
            "type": "rate_limit_error"
        }
    });
    Mock::given(method("POST"))
        .and(path("/systemone"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "7")
                .set_body_json(error.clone()),
        )
        .mount(&upstream)
        .await;
    let state = common::state_with_pool(&upstream.uri(), PoolKind::SystemOne, "jev-1.13.0").await;
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);
    let resp = app
        .serve(request(
            Some(&bearer),
            json!({
                "model": "jev-1.13.0",
                "state": "hello",
                "questions": {"positive": {"type": "noul"}}
            }),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        resp.headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok()),
        Some("7")
    );
    let body: Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(body, error);
}
