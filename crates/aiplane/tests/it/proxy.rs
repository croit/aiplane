// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Bearer-gated /v1/* proxy routes. Covers the auth boundary, header
//! policy, model resolution, and response relay against a wiremock
//! upstream.

use crate::common;

use aiplane_core::server::upstreams::PoolKind;
use common::Service as _;
use rama::http::{Body, Method, Request, StatusCode};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// RFC 7235: the auth scheme name is case-insensitive; the token is not.
#[tokio::test]
async fn v1_reads_the_bearer_scheme_in_any_case_and_the_token_exactly() {
    let state = common::state_with_chat_pool("http://unused.invalid").await;
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);
    let models = |authorization: String| {
        Request::builder()
            .method(Method::GET)
            .uri("/v1/models")
            .header("authorization", authorization)
            .body(Body::empty())
            .unwrap()
    };
    for scheme in ["bearer", "BEARER"] {
        let resp = app
            .serve(models(format!("{scheme} {bearer}")))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "{scheme}");
    }
    let resp = app
        .serve(models(format!("bearer {}", bearer.to_uppercase())))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn v1_models_without_bearer_is_401() {
    let state = common::state_with_chat_pool("http://unused.invalid").await;
    let app = common::app(state);
    let resp = app
        .serve(common::req(Method::GET, "/v1/models"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let www = resp
        .headers()
        .get("www-authenticate")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        www.contains("Bearer"),
        "missing WWW-Authenticate header: got `{www}`"
    );
}

#[tokio::test]
async fn v1_models_relays_upstream_list() {
    let upstream = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": "list",
            "data": [{"id": "model-a", "object": "model"}]
        })))
        .mount(&upstream)
        .await;

    let state = common::state_with_chat_pool(&upstream.uri()).await;
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);

    let req = Request::builder()
        .method(Method::GET)
        .uri("/v1/models")
        .header("authorization", format!("Bearer {bearer}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.serve(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = common::read_body(resp).await;
    let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(parsed["data"][0]["id"], "model-a");
}

#[tokio::test]
async fn v1_chat_completions_relays_through_upstream() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{"message": {"role": "assistant", "content": "hi from upstream"}}]
        })))
        .mount(&upstream)
        .await;

    let state = common::state_with_chat_pool(&upstream.uri()).await;
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);

    let body = json!({"model": "model-a", "messages": []}).to_string();
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {bearer}"))
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();
    let resp = app.serve(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = common::read_body(resp).await;
    // Streaming relay → bytes are upstream-shaped JSON, byte-for-byte.
    let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        parsed["choices"][0]["message"]["content"],
        "hi from upstream"
    );
}

#[tokio::test]
async fn v1_chat_content_guard_denies_before_dispatching_to_a_noncompliant_pool() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/systemone"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "answers": {
                "gdpr": {"type": "noul", "noul": 0.9},
                "nda": {"type": "noul", "noul": 0.9}
            }
        })))
        .mount(&upstream)
        .await;
    let state = common::state_with_content_guard_pools(&upstream.uri()).await;
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {bearer}"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"model": "model-a", "messages": [{"role": "user", "content": "secret"}]})
                .to_string(),
        ))
        .unwrap();
    let resp = app.serve(req).await.unwrap();
    let status = resp.status();
    let body: serde_json::Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"]["code"], "content_policy_denied");
    let requests = upstream.received_requests().await.unwrap();
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.url.path() == "/chat/completions")
            .count(),
        0,
        "the guarded request must not reach the selected chat pool"
    );
}

#[tokio::test]
async fn v1_chat_content_guard_monitor_logs_an_unavailable_guard_and_dispatches() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{"message": {"role": "assistant", "content": "hi from upstream"}}]
        })))
        .mount(&upstream)
        .await;
    let state = common::state_with_content_guard_pools_in_mode(
        &upstream.uri(),
        aiplane_core::server::config::ContentGuardMode::Monitor,
    )
    .await;
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {bearer}"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"model": "model-a", "messages": [{"role": "user", "content": "secret"}]})
                .to_string(),
        ))
        .unwrap();
    let resp = app.serve(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn v1_chat_blocked_by_quota_returns_429() {
    use aiplane_core::server::db::limits::{self, Dimension, SubjectType, Window};

    let upstream = MockServer::start().await;
    // The backend never needs to answer — enforcement fires before routing.
    let state = common::state_with_chat_pool(&upstream.uri()).await;
    let bearer = common::seed_user_with_token(&state, "alice").await;
    // A global 0-requests/hour rule puts every caller over budget immediately.
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
    let app = common::app(state);

    let body = json!({"model": "model-a", "messages": []}).to_string();
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {bearer}"))
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();
    let resp = app.serve(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(
        resp.headers().get("retry-after").is_some(),
        "429 must carry a Retry-After header"
    );
    let bytes = common::read_body(resp).await;
    let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(parsed["error"]["type"], "rate_limit_exceeded");
}

#[tokio::test]
async fn v1_chat_completion_records_a_usage_row() {
    use aiplane_core::server::db::usage::{Filter, Period, aggregate, period_bounds};
    use jiff::Timestamp;

    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{"message": {"role": "assistant", "content": "hi"}}],
            "usage": {"prompt_tokens": 11, "completion_tokens": 7, "total_tokens": 18}
        })))
        .mount(&upstream)
        .await;

    // Opt into a live metered usage sink (the harness default is disabled).
    let state = common::state_with_chat_pool(&upstream.uri()).await;
    let metered = aiplane_core::server::usage::spawn(state.db.clone(), 90);
    let state = state.with_usage(metered);
    let db = state.db.clone();
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);

    let body = json!({"model": "model-a", "messages": []}).to_string();
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {bearer}"))
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();
    let resp = app.serve(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    // Drain the response so the streaming relay task runs to completion and
    // emits its usage record.
    let _ = common::read_body(resp).await;

    // The batched writer flushes within ~500ms; poll a little past that.
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;

    let now = Timestamp::now();
    let bounds = period_bounds(Period::Today, "UTC", now);
    let agg = aggregate(&db, bounds, &Filter::default(), 90, now, true)
        .await
        .unwrap();
    assert_eq!(agg.summary.requests, 1, "one upstream call recorded");
    assert_eq!(agg.summary.total_tokens, 18, "usage block parsed from body");
    assert_eq!(agg.by_source[0].key, "v1_api");
    assert_eq!(agg.by_backend[0].key, "mock");
    assert_eq!(agg.by_model[0].key, "model-a");
    assert_eq!(agg.by_user[0].key, "alice");
    assert_eq!(agg.by_user[0].label, "alice@example.com");
}

/// A model that collapses into repeating itself on `/v1` is cut off with an
/// in-band `loop_detected` error, and the call is recorded as stopped — the
/// only trace a `/v1` loop leaves, since the client keeps no turn row.
#[tokio::test]
async fn a_v1_stream_that_loops_is_stopped_and_recorded_as_a_loop() {
    let looped: String = (0..800)
        .map(|_| "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"I'll send it. \"}}]}\n\n")
        .collect();
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(format!("{looped}data: [DONE]\n\n")),
        )
        .mount(&upstream)
        .await;
    let state = common::state_with_chat_pool(&upstream.uri()).await;
    let metered = aiplane_core::server::usage::spawn(state.db.clone(), 90);
    let state = state.with_usage(metered);
    let db = state.db.clone();
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);

    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {bearer}"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"model": "model-a", "messages": [], "stream": true}).to_string(),
        ))
        .unwrap();
    let resp = app.serve(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = String::from_utf8_lossy(&common::read_body(resp).await).to_string();
    assert!(body.contains("loop_detected"), "{body}");
    assert!(
        body.ends_with("data: [DONE]\n\n"),
        "the stream still terminates"
    );

    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    let reasons: Vec<Option<String>> = sqlx::query_scalar("SELECT stop_reason FROM usage_events")
        .fetch_all(&db)
        .await
        .unwrap();
    assert_eq!(reasons, vec![Some("loop".to_string())]);
}

#[tokio::test]
async fn v1_embeddings_relays_through_upstream() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/embeddings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": "list",
            "model": "embed-model",
            "data": [{"object": "embedding", "index": 0, "embedding": [0.1, 0.2, 0.3]}],
        })))
        .mount(&upstream)
        .await;

    let state = common::state_with_pool(&upstream.uri(), PoolKind::Embedding, "embed-model").await;
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);

    let body = json!({"model": "embed-model", "input": ["Schreibe einen Brief", "Write a letter"]})
        .to_string();
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/embeddings")
        .header("authorization", format!("Bearer {bearer}"))
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();
    let resp = app.serve(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = common::read_body(resp).await;
    let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(parsed["data"][0]["embedding"][0], 0.1);
}

#[tokio::test]
async fn v1_images_generations_relays_through_upstream() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/images/generations"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "created": 0,
            "data": [{"url": "https://cdn.example/img.png"}],
        })))
        .mount(&upstream)
        .await;

    let state = common::state_with_pool(&upstream.uri(), PoolKind::Image, "glm-image").await;
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);

    let body = json!({"model": "glm-image", "prompt": "a blue cloud"}).to_string();
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/images/generations")
        .header("authorization", format!("Bearer {bearer}"))
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();
    let resp = app.serve(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    // Byte-dumb relay: the provider's exact response reaches the client.
    let bytes = common::read_body(resp).await;
    let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(parsed["data"][0]["url"], "https://cdn.example/img.png");
}

#[tokio::test]
async fn v1_images_generations_without_bearer_is_401() {
    let state =
        common::state_with_pool("http://unused.invalid", PoolKind::Image, "glm-image").await;
    let app = common::app(state);
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/images/generations")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"model":"glm-image","prompt":"x"}"#))
        .unwrap();
    let resp = app.serve(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn v1_images_generations_records_image_usage_row() {
    use aiplane_core::server::db::usage::{Filter, Period, aggregate, period_bounds};
    use jiff::Timestamp;

    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/images/generations"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{"url": "https://cdn.example/img.png"}],
        })))
        .mount(&upstream)
        .await;

    let state = common::state_with_pool(&upstream.uri(), PoolKind::Image, "glm-image").await;
    aiplane_core::server::db::model_defaults::set_pricing_with_unit(
        &state.db,
        "glm-image",
        None,
        Some(0.75),
        aiplane_core::server::db::model_defaults::PricingUnit::Images,
    )
    .await
    .unwrap();
    let metered = aiplane_core::server::usage::spawn(state.db.clone(), 90);
    let state = state.with_usage(metered);
    let db = state.db.clone();
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);

    let body = json!({"model": "glm-image", "prompt": "a blue cloud"}).to_string();
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/images/generations")
        .header("authorization", format!("Bearer {bearer}"))
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();
    let resp = app.serve(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let _ = common::read_body(resp).await;

    tokio::time::sleep(std::time::Duration::from_millis(800)).await;

    let now = Timestamp::now();
    let bounds = period_bounds(Period::Today, "UTC", now);
    let agg = aggregate(&db, bounds, &Filter::default(), 90, now, true)
        .await
        .unwrap();
    assert_eq!(agg.summary.requests, 1, "one image call recorded");
    assert_eq!(agg.by_model[0].key, "glm-image");
    assert!((agg.summary.total_cost - 0.75).abs() < 1e-9);
    // Images carry no token counts.
    assert_eq!(agg.summary.total_tokens, 0);
}

#[tokio::test]
async fn v1_image_edits_count_all_input_images() {
    use aiplane_core::server::db::model_defaults::{self, PricingUnit};
    use aiplane_core::server::db::usage::{Filter, Period, aggregate, period_bounds};
    use jiff::Timestamp;

    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/images/edits"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{"url": "https://cdn.example/img.png"}],
        })))
        .mount(&upstream)
        .await;
    let state = common::state_with_pool(&upstream.uri(), PoolKind::Image, "image-edit").await;
    model_defaults::set_pricing_with_unit(
        &state.db,
        "image-edit",
        Some(2.0),
        Some(3.0),
        PricingUnit::Images,
    )
    .await
    .unwrap();
    let metered = aiplane_core::server::usage::spawn(state.db.clone(), 90);
    let state = state.with_usage(metered);
    let db = state.db.clone();
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);
    let boundary = "edit-boundary";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\nimage-edit\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"image\"; filename=\"a.png\"\r\nContent-Type: image/png\r\n\r\nA\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"image\"; filename=\"b.png\"\r\nContent-Type: image/png\r\n\r\nB\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"prompt\"\r\n\r\ncombine\r\n--{boundary}--\r\n"
    );
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/images/edits")
        .header("authorization", format!("Bearer {bearer}"))
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();
    let resp = app.serve(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let _ = common::read_body(resp).await;
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    let now = Timestamp::now();
    let agg = aggregate(
        &db,
        period_bounds(Period::Today, "UTC", now),
        &Filter::default(),
        90,
        now,
        true,
    )
    .await
    .unwrap();
    assert!((agg.summary.total_cost - 7.0).abs() < 1e-9);
}

#[tokio::test]
async fn v1_speech_records_character_usage_for_costs() {
    use aiplane_core::server::db::model_defaults::{self, PricingUnit};
    use aiplane_core::server::db::usage::{Filter, Period, aggregate, period_bounds};
    use jiff::Timestamp;

    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/audio/speech"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"audio".to_vec()))
        .mount(&upstream)
        .await;

    let state = common::state_with_pool(&upstream.uri(), PoolKind::Speech, "tts-model").await;
    model_defaults::set_pricing_with_unit(
        &state.db,
        "tts-model",
        Some(0.01),
        None,
        PricingUnit::Characters,
    )
    .await
    .unwrap();
    let metered = aiplane_core::server::usage::spawn(state.db.clone(), 90);
    let state = state.with_usage(metered);
    let db = state.db.clone();
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/audio/speech")
        .header("authorization", format!("Bearer {bearer}"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"model": "tts-model", "input": "hello"}).to_string(),
        ))
        .unwrap();
    let resp = app.serve(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let _ = common::read_body(resp).await;
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;

    let now = Timestamp::now();
    let bounds = period_bounds(Period::Today, "UTC", now);
    let agg = aggregate(&db, bounds, &Filter::default(), 90, now, true)
        .await
        .unwrap();
    assert!((agg.summary.total_cost - 0.05).abs() < 1e-9);
}

#[tokio::test]
async fn v1_images_edits_without_bearer_is_401() {
    let state =
        common::state_with_pool("http://unused.invalid", PoolKind::Image, "glm-image").await;
    let app = common::app(state);
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/images/edits")
        .header("content-type", "multipart/form-data; boundary=x")
        .body(Body::from("--x--\r\n"))
        .unwrap();
    let resp = app.serve(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn v1_embeddings_without_bearer_is_401() {
    let state =
        common::state_with_pool("http://unused.invalid", PoolKind::Embedding, "embed-model").await;
    let app = common::app(state);
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/embeddings")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"model":"embed-model","input":["x"]}"#))
        .unwrap();
    let resp = app.serve(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn v1_embeddings_missing_model_field_is_400() {
    let state =
        common::state_with_pool("http://unused.invalid", PoolKind::Embedding, "embed-model").await;
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/embeddings")
        .header("authorization", format!("Bearer {bearer}"))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"input":["x"]}"#))
        .unwrap();
    let resp = app.serve(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn v1_embeddings_unknown_model_is_404_model_not_found() {
    let state =
        common::state_with_pool("http://unused.invalid", PoolKind::Embedding, "embed-model").await;
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/embeddings")
        .header("authorization", format!("Bearer {bearer}"))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"model":"no-such-model","input":["x"]}"#))
        .unwrap();
    let resp = app.serve(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// --- pool-kind routing isolation -------------------------------------------
// A model is reachable ONLY through its own pool kind's endpoint. These pin
// that `acquire_for(model, kind)` filters by kind, so an embedding model can't
// be driven through /v1/chat/completions and vice-versa — even though both
// models are advertised in /v1/models.

#[tokio::test]
async fn chat_endpoint_rejects_embedding_model_with_404() {
    let state = common::state_with_chat_and_embed("chat-model", "embed-model").await;
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);

    // The embedding model IS known to the gateway (listed in /v1/models)…
    let list = app
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
    let models: serde_json::Value = serde_json::from_slice(&common::read_body(list).await).unwrap();
    let ids: Vec<&str> = models["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"embed-model") && ids.contains(&"chat-model"));

    // …but it must NOT be routable as a chat model.
    let resp = app
        .serve(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/chat/completions")
                .header("authorization", format!("Bearer {bearer}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"model":"embed-model","messages":[{"role":"user","content":"hi"}]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "embedding model must not be usable on /v1/chat/completions"
    );
}

#[tokio::test]
async fn embeddings_endpoint_rejects_chat_model_with_404() {
    let state = common::state_with_chat_and_embed("chat-model", "embed-model").await;
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);

    let resp = app
        .serve(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/embeddings")
                .header("authorization", format!("Bearer {bearer}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"model":"chat-model","input":["hi"]}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "chat model must not be usable on /v1/embeddings"
    );
}

#[tokio::test]
async fn v1_chat_completions_with_missing_model_field_is_400() {
    let state = common::state_with_chat_pool("http://unused.invalid").await;
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);

    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {bearer}"))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"messages":[]}"#))
        .unwrap();
    let resp = app.serve(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn v1_chat_completions_with_unknown_model_is_404_model_not_found() {
    // OpenAI parity: a model no backend serves is a client error (404
    // `model_not_found`), not a transient 503 — so clients surface a config
    // problem instead of silently retrying.
    let state = common::state_with_chat_pool("http://unused.invalid").await;
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);

    let body = json!({"model": "not-routed", "messages": []}).to_string();
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {bearer}"))
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();
    let resp = app.serve(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let parsed: serde_json::Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(parsed["error"]["code"], "model_not_found");
    assert_eq!(parsed["error"]["type"], "invalid_request_error");
    assert_eq!(parsed["error"]["param"], "model");
    assert!(
        parsed["error"]["message"]
            .as_str()
            .unwrap_or("")
            .contains("not-routed"),
        "expected error to name the unknown model: {parsed}"
    );
}

#[tokio::test]
async fn v1_chat_completions_known_model_all_replicas_down_is_503() {
    // The model IS known, but every replica is unhealthy → transient 503,
    // NOT 404. This is the distinction the OpenAI contract draws.
    let state = common::state_with_chat_pool("http://unused.invalid").await;
    for pool in state.upstreams.pools() {
        for backend in &pool.backends {
            backend.set_healthy(false);
        }
    }
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);

    let body = json!({"model": "model-a", "messages": []}).to_string();
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {bearer}"))
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();
    let resp = app.serve(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    let parsed: serde_json::Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(parsed["error"]["code"], "upstream_unreachable");
}

/// The graceful-pause contract, end to end: a request that arrives while every
/// replica is down is **held**, and served normally the moment one returns.
///
/// This is what keeps an agent session alive across an upstream restart. The
/// client has received nothing at that point — no status line, no byte — so a
/// request that waits 300 ms and then succeeds is indistinguishable from a slow
/// one, and the turn continues instead of dying on a 503 the client may not
/// retry.
#[tokio::test]
async fn v1_chat_completions_waits_for_a_backend_to_come_back() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "c1",
            "object": "chat.completion",
            "choices": [{"index": 0, "message": {"role": "assistant", "content": "waited-for-you"}}],
        })))
        .mount(&upstream)
        .await;
    let mut state = common::state_with_chat_pool(&upstream.uri()).await;
    // A real (short) wait budget: the harness disables parking by default so
    // failure assertions don't sit through two minutes.
    state = state.with_upstream_wait(std::time::Duration::from_secs(10));
    for pool in state.upstreams.pools() {
        for backend in &pool.backends {
            backend.set_healthy(false);
        }
    }
    let bearer = common::seed_user_with_token(&state, "alice").await;

    // Bring the pool back shortly after the request is parked.
    let recover = state.upstreams.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        for pool in recover.pools() {
            for backend in &pool.backends {
                backend.set_healthy(true);
            }
        }
    });

    let app = common::app(state);
    let body = json!({"model": "model-a", "messages": []}).to_string();
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {bearer}"))
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();
    let started = std::time::Instant::now();
    let resp = app.serve(req).await.unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "a request parked through a short outage must still be served"
    );
    assert!(
        started.elapsed() >= std::time::Duration::from_millis(250),
        "it answered before the backend recovered, so it cannot have waited"
    );
    let parsed: serde_json::Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(parsed["choices"][0]["message"]["content"], "waited-for-you");
}

/// The streamed path must survive a replica failing mid-turn, because that is
/// the path an agent client actually uses.
///
/// The response headers left long before the upstream was contacted, so this
/// cannot be answered with a status code — it has to be answered by asking a
/// different replica. A `send()` that fails has produced no frames, so there is
/// nothing to duplicate.
#[tokio::test]
async fn a_streamed_turn_survives_a_replica_that_fails_before_any_frame() {
    // Two replicas: one that refuses connections, one that streams a real
    // answer. Which one the picker tries first is not the point — either
    // ordering must end with the client getting the answer.
    let dead = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let dead_addr = dead.local_addr().unwrap();
    drop(dead);

    let alive = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            concat!(
                "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"survived\"}}]}\n\n",
                "data: [DONE]\n\n",
            ),
            "text/event-stream",
        ))
        .mount(&alive)
        .await;

    let state =
        common::state_with_two_chat_backends(&format!("http://{dead_addr}/v1"), &alive.uri()).await;
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);

    let body = json!({"model": "model-a", "stream": true, "messages": []}).to_string();
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {bearer}"))
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();
    let resp = app.serve(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let text = String::from_utf8_lossy(&common::read_body(resp).await).to_string();
    assert!(
        text.contains("survived"),
        "the turn should have been re-dispatched to the working replica: {text}"
    );
}

#[tokio::test]
async fn v1_models_lists_all_pools_deduped_with_full_objects() {
    // Lists EVERY pool/kind, de-duplicated by id, even when a backend (the
    // transcription one) never reported a `/models` probe — its id comes
    // from the pool's config fallback.
    let state = common::state_with_chat_and_config_transcription(
        "Qwen/Qwen3.6-35B-A3B-FP8",
        "mistralai/Voxtral-Mini-4B-Realtime-2602",
    )
    .await;
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);

    let req = Request::builder()
        .method(Method::GET)
        .uri("/v1/models")
        .header("authorization", format!("Bearer {bearer}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.serve(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let parsed: serde_json::Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(parsed["object"], "list");
    let data = parsed["data"].as_array().expect("data array");
    let ids: Vec<&str> = data
        .iter()
        .map(|m| m["id"].as_str().unwrap_or(""))
        .collect();
    assert!(
        ids.contains(&"Qwen/Qwen3.6-35B-A3B-FP8"),
        "chat model missing: {ids:?}"
    );
    assert!(
        ids.contains(&"mistralai/Voxtral-Mini-4B-Realtime-2602"),
        "transcription (config-fallback) model missing: {ids:?}"
    );
    // Two chat replicas serve the same id → exactly two distinct models.
    assert_eq!(data.len(), 2, "expected de-duped list of 2: {ids:?}");
    // Each entry is a full OpenAI model object incl. `created`.
    for m in data {
        assert_eq!(m["object"], "model");
        assert_eq!(m["owned_by"], "aiplane");
        assert!(
            m["created"].as_u64().is_some(),
            "created must be a unix-seconds integer: {m}"
        );
    }
}

#[tokio::test]
async fn v1_models_retrieve_returns_model_object_for_id_with_slash() {
    let state = common::state_with_chat_and_config_transcription(
        "Qwen/Qwen3.6-35B-A3B-FP8",
        "mistralai/Voxtral-Mini-4B-Realtime-2602",
    )
    .await;
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);

    // The id contains `/` — exercises the `{*id}` catch-all route.
    let req = Request::builder()
        .method(Method::GET)
        .uri("/v1/models/mistralai/Voxtral-Mini-4B-Realtime-2602")
        .header("authorization", format!("Bearer {bearer}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.serve(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let parsed: serde_json::Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(parsed["id"], "mistralai/Voxtral-Mini-4B-Realtime-2602");
    assert_eq!(parsed["object"], "model");
    assert!(parsed["created"].as_u64().is_some());
}

#[tokio::test]
async fn v1_models_retrieve_unknown_id_is_404() {
    let state = common::state_with_chat_pool("http://unused.invalid").await;
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);

    let req = Request::builder()
        .method(Method::GET)
        .uri("/v1/models/does-not-exist")
        .header("authorization", format!("Bearer {bearer}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.serve(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let parsed: serde_json::Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(parsed["error"]["code"], "model_not_found");
    assert_eq!(parsed["error"]["type"], "invalid_request_error");
}

#[tokio::test]
async fn v1_models_retrieve_without_bearer_is_401() {
    let state = common::state_with_chat_pool("http://unused.invalid").await;
    let app = common::app(state);
    let resp = app
        .serve(common::req(Method::GET, "/v1/models/model-a"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn v1_chat_completions_drops_client_authorization_and_injects_upstream_key() {
    // Mount that asserts the upstream Authorization header is exactly
    // what we configured on the BackendConfig, NOT the gateway-token
    // bearer the client sent.
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(wiremock::matchers::header(
            "authorization",
            "Bearer SK-UPSTREAM",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"choices":[]})))
        .mount(&upstream)
        .await;

    let upstream_uri = upstream.uri();
    let state = state_with_backend_api_key(&upstream_uri, "SK-UPSTREAM").await;
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);

    let body = json!({"model": "model-a", "messages": []}).to_string();
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {bearer}"))
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();
    let resp = app.serve(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "wiremock would 404 if the assertion failed"
    );
}

/// Variant of `state_with_chat_pool` that configures an `api_key_env`
/// pointing at a test-scoped env var. The integration test sets/clears
/// the env around the lookup so we don't leak state between tests.
async fn state_with_backend_api_key(
    upstream_url: &str,
    key: &str,
) -> aiplane::rama_server::RamaState {
    use std::collections::HashMap;
    use std::sync::Arc;

    use aiplane::rama_server::{RamaState, SessionStore};
    use aiplane_core::server::rbac::Resolver;
    use aiplane_core::server::upstreams::{
        self,
        config::{BackendConfig, PickerStrategy, PoolKind, UpstreamPoolConfig},
    };
    use aiplane_core::server::{Config, db};
    use aiplane_runtime::server::AppState;
    use aiplane_runtime::server::tools::ToolRegistry;

    const ENV_KEY: &str = "TEST_UPSTREAM_KEY";
    // SAFETY: integration tests run in the same process — this set
    // races with itself in parallel runs but the value we set is the
    // same across all callers so the race is benign.
    unsafe { std::env::set_var(ENV_KEY, key) };

    let pool = db::open(std::path::Path::new(":memory:")).await.unwrap();
    let mut pools = HashMap::new();
    pools.insert(
        "pool".to_string(),
        UpstreamPoolConfig {
            voices: Default::default(),
            offer_voices: Vec::new(),
            allowed_groups: Vec::new(),
            fallback_offline: None,
            compliance: Default::default(),
            enforce_limits: true,
            kind: PoolKind::Chat,
            strategy: PickerStrategy::RoundRobin,
            models: Vec::new(),
            backend: vec![BackendConfig {
                alias: None,
                supports_edit: false,
                enabled: true,
                name: "mock".into(),
                base_url: upstream_url.into(),
                api_key_env: Some(ENV_KEY.into()),
                api_key: None,
                weight: 1,
                max_inflight: 16,
                health_path: "/models".into(),
                models: Vec::new(),
            }],
        },
    );
    let registry = upstreams::UpstreamRegistry::new(&pools).unwrap();
    common::seed_pool_models(&registry, "pool", 0, &["model-a"]);
    let tools = Arc::new(ToolRegistry::new());
    let rbac = Arc::new(Resolver::empty());
    let app = AppState::new(Config::default(), pool.clone(), registry, tools, rbac);
    let sessions = SessionStore::new(pool, common::TEST_SECRET);
    RamaState::new(
        app,
        sessions,
        aiplane_core::server::usage::UsageHandle::disabled(),
    )
}

/// A `kind` pool whose single backend answers to the bare alias `qwen` and
/// serves the real id `model-a`.
async fn state_with_alias_pool(
    upstream_url: &str,
    kind: PoolKind,
) -> aiplane::rama_server::RamaState {
    use std::collections::HashMap;
    use std::sync::Arc;

    use aiplane::rama_server::{RamaState, SessionStore};
    use aiplane_core::server::rbac::Resolver;
    use aiplane_core::server::upstreams::{
        self,
        config::{AliasSpec, BackendConfig, PickerStrategy, UpstreamPoolConfig},
    };
    use aiplane_core::server::{Config, db};
    use aiplane_runtime::server::AppState;
    use aiplane_runtime::server::tools::ToolRegistry;

    let pool = db::open(std::path::Path::new(":memory:")).await.unwrap();
    let mut pools = HashMap::new();
    pools.insert(
        "pool".to_string(),
        UpstreamPoolConfig {
            voices: Default::default(),
            offer_voices: Vec::new(),
            allowed_groups: Vec::new(),
            fallback_offline: None,
            compliance: Default::default(),
            enforce_limits: true,
            kind,
            strategy: PickerStrategy::RoundRobin,
            models: Vec::new(),
            backend: vec![BackendConfig {
                alias: Some(AliasSpec::Names(vec!["qwen".into()])),
                supports_edit: false,
                enabled: true,
                name: "mock".into(),
                base_url: upstream_url.into(),
                api_key_env: None,
                api_key: None,
                weight: 1,
                max_inflight: 16,
                health_path: "/models".into(),
                models: Vec::new(),
            }],
        },
    );
    let registry = upstreams::UpstreamRegistry::new(&pools).unwrap();
    common::seed_pool_models(&registry, "pool", 0, &["model-a"]);
    let tools = Arc::new(ToolRegistry::new());
    let rbac = Arc::new(Resolver::empty());
    let app = AppState::new(Config::default(), pool.clone(), registry, tools, rbac);
    let sessions = SessionStore::new(pool, common::TEST_SECRET);
    RamaState::new(
        app,
        sessions,
        aiplane_core::server::usage::UsageHandle::disabled(),
    )
}

#[tokio::test]
async fn v1_chat_alias_rewrites_model_and_sets_resolved_header() {
    let upstream = MockServer::start().await;
    // Matches ONLY when the forwarded body carries the real id. If the gateway
    // forwarded the alias `qwen` unchanged, this mock wouldn't match and the
    // request would 404 — so the 200 assertion below proves the body rewrite.
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(wiremock::matchers::body_partial_json(
            json!({"model": "model-a"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{"message": {"role": "assistant", "content": "ok"}}]
        })))
        .mount(&upstream)
        .await;

    let state = state_with_alias_pool(&upstream.uri(), PoolKind::Chat).await;
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);

    let body = json!({"model": "qwen", "messages": []}).to_string();
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {bearer}"))
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();
    let resp = app.serve(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "alias must rewrite model→model-a so the upstream mock matches"
    );
    assert_eq!(
        resp.headers()
            .get("x-gateway-resolved-model")
            .and_then(|v| v.to_str().ok()),
        Some("model-a"),
        "response must advertise the resolved real model id"
    );
}

// --- /v1/rerank --------------------------------------------------------------

fn rerank_request(bearer: Option<&str>, body: serde_json::Value) -> Request<Body> {
    let mut builder = Request::builder()
        .method(Method::POST)
        .uri("/v1/rerank")
        .header("content-type", "application/json");
    if let Some(bearer) = bearer {
        builder = builder.header("authorization", format!("Bearer {bearer}"));
    }
    builder.body(Body::from(body.to_string())).unwrap()
}

#[tokio::test]
async fn v1_rerank_relays_through_upstream() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/rerank"))
        .and(wiremock::matchers::body_partial_json(json!({
            "model": "rerank-model",
            "query": "invoice 4711",
            "documents": ["invoice 4711", "invoice 4712"],
            "top_n": 1,
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{"index": 0, "relevance_score": 0.93}],
        })))
        .mount(&upstream)
        .await;

    let state = common::state_with_pool(&upstream.uri(), PoolKind::Rerank, "rerank-model").await;
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);

    let resp = app
        .serve(rerank_request(
            Some(&bearer),
            json!({
                "model": "rerank-model",
                "query": "invoice 4711",
                "documents": ["invoice 4711", "invoice 4712"],
                "top_n": 1,
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let parsed: serde_json::Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(parsed["results"][0]["index"], 0);
    assert_eq!(parsed["results"][0]["relevance_score"], 0.93);
}

/// Rerank models are listed and retrievable like embedding models: a client
/// discovers them the same way it discovers anything else it may route to.
#[tokio::test]
async fn v1_models_lists_and_retrieves_rerank_models() {
    let state =
        common::state_with_pool("http://unused.invalid", PoolKind::Rerank, "rerank-model").await;
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);
    let get = |uri: &str| {
        Request::builder()
            .method(Method::GET)
            .uri(uri)
            .header("authorization", format!("Bearer {bearer}"))
            .body(Body::empty())
            .unwrap()
    };

    let resp = app.serve(get("/v1/models")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let parsed: serde_json::Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(parsed["data"][0]["id"], "rerank-model", "{parsed}");

    let resp = app.serve(get("/v1/models/rerank-model")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn v1_rerank_without_bearer_is_401() {
    let state =
        common::state_with_pool("http://unused.invalid", PoolKind::Rerank, "rerank-model").await;
    let app = common::app(state);
    let resp = app
        .serve(rerank_request(
            None,
            json!({"model": "rerank-model", "query": "q", "documents": ["d"]}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn v1_rerank_missing_model_field_is_400() {
    let state =
        common::state_with_pool("http://unused.invalid", PoolKind::Rerank, "rerank-model").await;
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);
    let resp = app
        .serve(rerank_request(
            Some(&bearer),
            json!({"query": "q", "documents": ["d"]}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn v1_rerank_unknown_model_is_404_model_not_found() {
    let state =
        common::state_with_pool("http://unused.invalid", PoolKind::Rerank, "rerank-model").await;
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);
    let resp = app
        .serve(rerank_request(
            Some(&bearer),
            json!({"model": "no-such-model", "query": "q", "documents": ["d"]}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let parsed: serde_json::Value = serde_json::from_slice(&common::read_body(resp).await).unwrap();
    assert_eq!(parsed["error"]["code"], "model_not_found");
}

#[tokio::test]
async fn rerank_endpoint_rejects_embedding_model_with_404() {
    let state =
        common::state_with_pool("http://unused.invalid", PoolKind::Embedding, "embed-model").await;
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);
    let resp = app
        .serve(rerank_request(
            Some(&bearer),
            json!({"model": "embed-model", "query": "q", "documents": ["d"]}),
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "an embedding model must not be usable on /v1/rerank"
    );
}

#[tokio::test]
async fn v1_rerank_alias_rewrites_model_and_sets_resolved_header() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/rerank"))
        .and(wiremock::matchers::body_partial_json(
            json!({"model": "model-a"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"results": []})))
        .mount(&upstream)
        .await;

    let state = state_with_alias_pool(&upstream.uri(), PoolKind::Rerank).await;
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);

    let resp = app
        .serve(rerank_request(
            Some(&bearer),
            json!({"model": "qwen", "query": "q", "documents": ["d"]}),
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "alias must rewrite model→model-a so the upstream mock matches"
    );
    assert_eq!(
        resp.headers()
            .get("x-gateway-resolved-model")
            .and_then(|v| v.to_str().ok()),
        Some("model-a"),
    );
}

/// A rerank backend that reports only `usage.total_tokens` (vLLM): every
/// token it scored is input, so a per-token input price applies to all of
/// them and the call counts against spend limits like an embedding does.
#[tokio::test]
async fn v1_rerank_records_rerank_usage_priced_on_its_tokens() {
    use aiplane_core::server::db::model_defaults::{self, PricingUnit};
    use aiplane_core::server::db::usage::{Filter, Period, aggregate, period_bounds};
    use jiff::Timestamp;

    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/rerank"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{"index": 0, "relevance_score": 0.5}],
            "usage": {"total_tokens": 500_000},
        })))
        .mount(&upstream)
        .await;

    let state = common::state_with_pool(&upstream.uri(), PoolKind::Rerank, "rerank-model").await;
    model_defaults::set_pricing_with_unit(
        &state.db,
        "rerank-model",
        Some(2.0),
        None,
        PricingUnit::Tokens,
    )
    .await
    .unwrap();
    let metered = aiplane_core::server::usage::spawn(state.db.clone(), 90);
    let state = state.with_usage(metered);
    let db = state.db.clone();
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);

    let resp = app
        .serve(rerank_request(
            Some(&bearer),
            json!({"model": "rerank-model", "query": "q", "documents": ["d"]}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let _ = common::read_body(resp).await;
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;

    let kinds: Vec<String> = sqlx::query_scalar("SELECT kind FROM usage_events")
        .fetch_all(&db)
        .await
        .unwrap();
    assert_eq!(kinds, vec!["rerank".to_string()]);
    let now = Timestamp::now();
    let bounds = period_bounds(Period::Today, "UTC", now);
    let agg = aggregate(&db, bounds, &Filter::default(), 90, now, true)
        .await
        .unwrap();
    assert_eq!(agg.summary.total_tokens, 500_000);
    assert!(
        (agg.summary.total_cost - 1.0).abs() < 1e-9,
        "500k tokens at 2.0 per 1M input tokens, got {}",
        agg.summary.total_cost
    );
}

/// An embedding backend that reports only `usage.total_tokens`: an
/// embedding generates nothing, so every token is input and priced so.
#[tokio::test]
async fn v1_embeddings_prices_a_bare_total_as_input_tokens() {
    use aiplane_core::server::db::model_defaults::{self, PricingUnit};
    use aiplane_core::server::db::usage::{Filter, Period, aggregate, period_bounds};
    use jiff::Timestamp;

    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/embeddings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": "list",
            "data": [{"object": "embedding", "index": 0, "embedding": [0.1]}],
            "usage": {"total_tokens": 250_000},
        })))
        .mount(&upstream)
        .await;

    let state = common::state_with_pool(&upstream.uri(), PoolKind::Embedding, "embed-model").await;
    model_defaults::set_pricing_with_unit(
        &state.db,
        "embed-model",
        Some(4.0),
        None,
        PricingUnit::Tokens,
    )
    .await
    .unwrap();
    let metered = aiplane_core::server::usage::spawn(state.db.clone(), 90);
    let state = state.with_usage(metered);
    let db = state.db.clone();
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);

    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/embeddings")
        .header("authorization", format!("Bearer {bearer}"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"model": "embed-model", "input": "hi"}).to_string(),
        ))
        .unwrap();
    let resp = app.serve(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let _ = common::read_body(resp).await;
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;

    let now = Timestamp::now();
    let bounds = period_bounds(Period::Today, "UTC", now);
    let agg = aggregate(&db, bounds, &Filter::default(), 90, now, true)
        .await
        .unwrap();
    assert_eq!(agg.summary.total_tokens, 250_000);
    assert!(
        (agg.summary.total_cost - 1.0).abs() < 1e-9,
        "250k tokens at 4.0 per 1M input tokens, got {}",
        agg.summary.total_cost
    );
}

// --- limits on the single-round-trip /v1 relays ------------------------------

/// A `/v1` relay that reads a `model`, routes it and forwards one request.
struct ModelRelay {
    kind: PoolKind,
    uri: &'static str,
    model: &'static str,
    multipart: bool,
}

const MODEL_RELAYS: [ModelRelay; 7] = [
    ModelRelay {
        kind: PoolKind::Embedding,
        uri: "/v1/embeddings",
        model: "embed-model",
        multipart: false,
    },
    ModelRelay {
        kind: PoolKind::Rerank,
        uri: "/v1/rerank",
        model: "rerank-model",
        multipart: false,
    },
    ModelRelay {
        kind: PoolKind::SystemOne,
        uri: "/v1/systemone",
        model: "decide-model",
        multipart: false,
    },
    ModelRelay {
        kind: PoolKind::Image,
        uri: "/v1/images/generations",
        model: "glm-image",
        multipart: false,
    },
    ModelRelay {
        kind: PoolKind::Speech,
        uri: "/v1/audio/speech",
        model: "tts-model",
        multipart: false,
    },
    ModelRelay {
        kind: PoolKind::Image,
        uri: "/v1/images/edits",
        model: "edit-model",
        multipart: true,
    },
    ModelRelay {
        kind: PoolKind::Transcription,
        uri: "/v1/audio/transcriptions",
        model: "whisper-model",
        multipart: true,
    },
];

fn relay_request(bearer: &str, relay: &ModelRelay) -> Request<Body> {
    let model = relay.model;
    let (content_type, body) = if relay.multipart {
        let boundary = "relay-boundary";
        (
            format!("multipart/form-data; boundary={boundary}"),
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\n{model}\r\n\
                 --{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.mp3\"\r\nContent-Type: audio/mpeg\r\n\r\nAUDIO\r\n\
                 --{boundary}\r\nContent-Disposition: form-data; name=\"image\"; filename=\"a.png\"\r\nContent-Type: image/png\r\n\r\nA\r\n\
                 --{boundary}\r\nContent-Disposition: form-data; name=\"prompt\"\r\n\r\nedit\r\n\
                 --{boundary}--\r\n"
            ),
        )
    } else {
        (
            "application/json".to_string(),
            json!({"model": model, "input": "hi"}).to_string(),
        )
    };
    Request::builder()
        .method(Method::POST)
        .uri(relay.uri)
        .header("authorization", format!("Bearer {bearer}"))
        .header("content-type", content_type)
        .body(Body::from(body))
        .unwrap()
}

/// A lone pool for `relay` behind an upstream that answers every POST, with a
/// zero-request rule on `subject` that has already spent the budget. Returns
/// the state, the caller's bearer and the upstream.
async fn spent_budget(
    relay: &ModelRelay,
    enforce_limits: bool,
    subject: aiplane_core::server::db::limits::SubjectType,
) -> (aiplane::rama_server::RamaState, String, MockServer) {
    use aiplane_core::server::db::limits::{self, Dimension, SubjectType, Window};

    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .mount(&upstream)
        .await;
    let state =
        common::state_with_pool_enforcing(&upstream.uri(), relay.kind, relay.model, enforce_limits)
            .await;
    let (bearer, token_id) = common::seed_user_with_token_id(&state, "alice").await;
    let subject_id = match subject {
        SubjectType::Token => token_id.as_str(),
        _ => "",
    };
    limits::upsert(
        &state.db,
        subject,
        subject_id,
        None,
        Dimension::Requests,
        Window::Hour,
        0.0,
    )
    .await
    .unwrap();
    (state, bearer, upstream)
}

/// Sends one request through `relay` with a spent budget and returns the
/// status together with how many requests reached the upstream.
async fn status_with_spent_budget(
    relay: &ModelRelay,
    enforce_limits: bool,
    subject: aiplane_core::server::db::limits::SubjectType,
) -> (StatusCode, usize) {
    let (state, bearer, upstream) = spent_budget(relay, enforce_limits, subject).await;
    let resp = common::app(state)
        .serve(relay_request(&bearer, relay))
        .await
        .unwrap();
    let reached = upstream.received_requests().await.unwrap().len();
    (resp.status(), reached)
}

/// A pool exempt from enforcement does not consume a budget, so it stays
/// reachable after the caller's budget is spent.
#[tokio::test]
async fn exempt_pools_stay_available_after_the_budget_is_spent() {
    use aiplane_core::server::db::limits::SubjectType;

    for relay in &MODEL_RELAYS {
        let (status, reached) = status_with_spent_budget(relay, false, SubjectType::Global).await;
        assert_eq!(status, StatusCode::OK, "{} on an exempt pool", relay.uri);
        assert_eq!(reached, 1, "{} must reach the exempt pool", relay.uri);
    }
}

#[tokio::test]
async fn enforced_pools_refuse_a_spent_budget_with_429() {
    use aiplane_core::server::db::limits::SubjectType;

    for relay in &MODEL_RELAYS {
        let (status, reached) = status_with_spent_budget(relay, true, SubjectType::Global).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{}", relay.uri);
        assert_eq!(reached, 0, "{} must refuse before forwarding", relay.uri);
    }
}

/// The token's own rules are an additional ceiling on enforced pools.
#[tokio::test]
async fn enforced_pools_apply_the_tokens_own_rules() {
    use aiplane_core::server::db::limits::SubjectType;

    for relay in &MODEL_RELAYS {
        let (status, reached) = status_with_spent_budget(relay, true, SubjectType::Token).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{}", relay.uri);
        assert_eq!(reached, 0, "{} must refuse before forwarding", relay.uri);
    }
}

/// An exempt pool ignores the token's own rules as it ignores every other.
#[tokio::test]
async fn exempt_pools_ignore_the_tokens_own_rules() {
    use aiplane_core::server::db::limits::SubjectType;

    for relay in &MODEL_RELAYS {
        let (status, reached) = status_with_spent_budget(relay, false, SubjectType::Token).await;
        assert_eq!(status, StatusCode::OK, "{} on an exempt pool", relay.uri);
        assert_eq!(reached, 1, "{} must reach the exempt pool", relay.uri);
    }
}

/// The limit gate runs before a slot is taken: an over-budget caller on a
/// saturated pool is told it is over budget, and its refused call neither
/// holds capacity nor counts as a dispatch.
#[tokio::test]
async fn a_spent_budget_is_refused_before_a_slot_is_taken() {
    use aiplane_core::server::db::limits::SubjectType;

    for relay in &MODEL_RELAYS {
        let (state, bearer, upstream) = spent_budget(relay, true, SubjectType::Global).await;
        let registry = state.upstreams.clone();
        let pool = registry.pools().into_iter().next().unwrap();
        let backend = &pool.backends[0];
        let held: Vec<_> = (0..16)
            .map(|_| registry.route(relay.model, relay.kind).unwrap())
            .collect();
        let dispatched = backend.dispatched();

        let resp = common::app(state)
            .serve(relay_request(&bearer, relay))
            .await
            .unwrap();

        assert_eq!(
            resp.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "{}",
            relay.uri
        );
        assert_eq!(backend.dispatched(), dispatched, "{}", relay.uri);
        assert_eq!(backend.inflight(), 16, "{}", relay.uri);
        assert!(upstream.received_requests().await.unwrap().is_empty());
        drop(held);
    }
}

// --- limits on the chat UI's voice -------------------------------------------

/// A zero-request global rule: every caller is over budget at once.
async fn spend_every_budget(state: &aiplane::rama_server::RamaState) {
    use aiplane_core::server::db::limits::{self, Dimension, SubjectType, Window};
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
}

/// An upstream that answers every POST with `200 {}`; what the tests read is
/// how many requests arrive.
async fn answering_upstream() -> MockServer {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .mount(&upstream)
        .await;
    upstream
}

fn session_transcription(cookie: &str) -> Request<Body> {
    let boundary = "voice-boundary";
    Request::builder()
        .method(Method::POST)
        .uri("/api/v0/transcriptions")
        .header("cookie", format!("id={cookie}"))
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\nwhisper-model\r\n\
             --{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.mp3\"\r\nContent-Type: audio/mpeg\r\n\r\nAUDIO\r\n\
             --{boundary}--\r\n"
        )))
        .unwrap()
}

/// `text` keys the process-wide TTS cache, so every test speaks its own.
fn session_speech(cookie: &str, text: &str) -> Request<Body> {
    common::post_json(
        "/api/v0/speech",
        cookie,
        &json!({"text": text, "language": "en"}).to_string(),
    )
}

/// One voice call by a signed-in user: its status, the error code, the
/// `Retry-After` seconds and how many requests reached the upstream.
async fn voice_call(
    enforce_limits: bool,
    over_budget: bool,
    kind: PoolKind,
    model: &str,
    request: impl FnOnce(&str) -> Request<Body>,
) -> (StatusCode, Option<String>, Option<u64>, usize) {
    let upstream = answering_upstream().await;
    let state =
        common::state_with_pool_enforcing(&upstream.uri(), kind, model, enforce_limits).await;
    let cookie = common::seed_session(&state, "alice", "alice@example.com").await;
    if over_budget {
        spend_every_budget(&state).await;
    }
    let resp = common::app(state).serve(request(&cookie)).await.unwrap();
    let status = resp.status();
    let retry_after = resp
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok());
    let body: serde_json::Value =
        serde_json::from_slice(&common::read_body(resp).await).unwrap_or_default();
    let code = body["error"]["code"].as_str().map(str::to_string);
    (
        status,
        code,
        retry_after,
        upstream.received_requests().await.unwrap().len(),
    )
}

/// The refusal a signed-in user over budget gets on a voice call: the chat
/// submit's code, and how long until the breached window moves on.
fn assert_session_limit_refusal(status: StatusCode, code: Option<&str>, retry_after: Option<u64>) {
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(code, Some("rate_limited"));
    assert!(
        retry_after.is_some_and(|secs| secs > 0),
        "a limit refusal says when to retry, got {retry_after:?}"
    );
}

#[tokio::test]
async fn voice_transcription_is_refused_over_budget_on_an_enforced_pool() {
    let (status, code, retry_after, reached) = voice_call(
        true,
        true,
        PoolKind::Transcription,
        "whisper-model",
        session_transcription,
    )
    .await;
    assert_session_limit_refusal(status, code.as_deref(), retry_after);
    assert_eq!(
        reached, 0,
        "a refused transcription must not reach the upstream"
    );
}

#[tokio::test]
async fn voice_transcription_on_an_exempt_pool_ignores_a_spent_budget() {
    let (status, _, _, reached) = voice_call(
        false,
        true,
        PoolKind::Transcription,
        "whisper-model",
        session_transcription,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reached, 1);
}

#[tokio::test]
async fn voice_transcription_under_budget_is_relayed() {
    let (status, _, _, reached) = voice_call(
        true,
        false,
        PoolKind::Transcription,
        "whisper-model",
        session_transcription,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reached, 1);
}

#[tokio::test]
async fn voice_speech_is_refused_over_budget_on_an_enforced_pool() {
    let (status, code, retry_after, reached) =
        voice_call(true, true, PoolKind::Speech, "tts-model", |cookie| {
            session_speech(cookie, "Refused for being over budget.")
        })
        .await;
    assert_session_limit_refusal(status, code.as_deref(), retry_after);
    assert_eq!(
        reached, 0,
        "a refused synthesis must not reach the upstream"
    );
}

#[tokio::test]
async fn voice_speech_on_an_exempt_pool_ignores_a_spent_budget() {
    let (status, _, _, reached) =
        voice_call(false, true, PoolKind::Speech, "tts-model", |cookie| {
            session_speech(cookie, "Spoken by an exempt pool.")
        })
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reached, 1);
}

#[tokio::test]
async fn voice_speech_under_budget_is_synthesised() {
    let (status, _, _, reached) =
        voice_call(true, false, PoolKind::Speech, "tts-model", |cookie| {
            session_speech(cookie, "Spoken within the budget.")
        })
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reached, 1);
}

/// The user row carries the groups that decide which pools and which limits
/// apply. When it cannot be read, a voice call is refused rather than run as
/// a user without groups, whose limits and access may be looser.
#[tokio::test]
async fn a_voice_call_fails_closed_when_the_user_cannot_be_read() {
    for (kind, model, request) in [
        (
            PoolKind::Transcription,
            "whisper-model",
            session_transcription as fn(&str) -> Request<Body>,
        ),
        (PoolKind::Speech, "tts-model", |cookie: &str| {
            session_speech(cookie, "Never spoken without its user.")
        }),
    ] {
        let upstream = answering_upstream().await;
        let state = common::state_with_pool_enforcing(&upstream.uri(), kind, model, true).await;
        let cookie = common::seed_session(&state, "alice", "alice@example.com").await;
        sqlx::query("ALTER TABLE users RENAME TO users_unreadable")
            .execute(&state.db)
            .await
            .unwrap();

        let resp = common::app(state).serve(request(&cookie)).await.unwrap();
        let status = resp.status();
        let body: serde_json::Value =
            serde_json::from_slice(&common::read_body(resp).await).unwrap_or_default();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{model}: {body}");
        assert_eq!(body["error"]["code"], "internal_error", "{model}");
        assert!(upstream.received_requests().await.unwrap().is_empty());
    }
}

/// A cached sentence costs no upstream call and records no usage, so a spent
/// budget does not take it away; a sentence that needs the upstream is refused.
#[tokio::test]
async fn voice_speech_serves_a_cached_sentence_after_the_budget_is_spent() {
    let upstream = answering_upstream().await;
    let state =
        common::state_with_pool_enforcing(&upstream.uri(), PoolKind::Speech, "tts-model", true)
            .await;
    let cookie = common::seed_session(&state, "alice", "alice@example.com").await;
    let app = common::app(state.clone());
    let cached = "Cached before the budget ran out.";
    let first = app.serve(session_speech(&cookie, cached)).await.unwrap();
    assert_eq!(first.status(), StatusCode::OK);

    spend_every_budget(&state).await;
    let again = app.serve(session_speech(&cookie, cached)).await.unwrap();
    assert_eq!(again.status(), StatusCode::OK, "a cache hit is free");
    let fresh = app
        .serve(session_speech(
            &cookie,
            "Never spoken before the budget ran out.",
        ))
        .await
        .unwrap();
    assert_eq!(fresh.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(upstream.received_requests().await.unwrap().len(), 1);
}

// --- the target's limits come before the content guard -----------------------

/// A content-guard deployment whose guard allows everything and whose chat
/// model answers, so the only thing that can stop a request is a limit.
async fn guarded_upstream() -> MockServer {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/systemone"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "answers": {
                "gdpr": {"type": "noul", "noul": 0.1},
                "nda": {"type": "noul", "noul": 0.1}
            }
        })))
        .mount(&upstream)
        .await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "c1",
            "object": "chat.completion",
            "model": "model-a",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "hi"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
        })))
        .mount(&upstream)
        .await;
    upstream
}

/// One request per chat dialect that runs through the content guard.
fn guarded_requests(bearer: &str) -> Vec<(&'static str, Request<Body>)> {
    let post = |uri: &str, body: serde_json::Value| {
        Request::builder()
            .method(Method::POST)
            .uri(uri)
            .header("authorization", format!("Bearer {bearer}"))
            .header("content-type", "application/json")
            .header("anthropic-version", "2023-06-01")
            .body(Body::from(body.to_string()))
            .unwrap()
    };
    vec![
        (
            "/v1/chat/completions",
            post(
                "/v1/chat/completions",
                json!({"model": "model-a", "messages": [{"role": "user", "content": "secret"}]}),
            ),
        ),
        (
            "/v1/messages",
            post(
                "/v1/messages",
                json!({
                    "model": "model-a",
                    "max_tokens": 16,
                    "messages": [{"role": "user", "content": "secret"}]
                }),
            ),
        ),
        (
            "/v1/responses",
            post(
                "/v1/responses",
                json!({"model": "model-a", "input": "secret"}),
            ),
        ),
    ]
}

async fn guard_calls(upstream: &MockServer) -> usize {
    upstream
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|request| request.url.path() == "/systemone")
        .count()
}

#[tokio::test]
async fn an_over_budget_caller_is_refused_before_the_content_guard_is_asked() {
    let upstream = guarded_upstream().await;
    let state = common::state_with_content_guard_pools(&upstream.uri()).await;
    let bearer = common::seed_user_with_token(&state, "alice").await;
    spend_every_budget(&state).await;
    let app = common::app(state);
    for (uri, request) in guarded_requests(&bearer) {
        let resp = app.serve(request).await.unwrap();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS, "{uri}");
    }
    assert_eq!(
        guard_calls(&upstream).await,
        0,
        "a request refused by its limits must not cost a guard inference"
    );
}

#[tokio::test]
async fn a_caller_within_budget_is_still_checked_by_the_content_guard() {
    let upstream = guarded_upstream().await;
    let state = common::state_with_content_guard_pools(&upstream.uri()).await;
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);
    for (asked_before, (uri, request)) in guarded_requests(&bearer).into_iter().enumerate() {
        let resp = app.serve(request).await.unwrap();
        let status = resp.status();
        let body = String::from_utf8_lossy(&common::read_body(resp).await).into_owned();
        assert_eq!(status, StatusCode::OK, "{uri}: {body}");
        assert_eq!(guard_calls(&upstream).await, asked_before + 1, "{uri}");
    }
}
