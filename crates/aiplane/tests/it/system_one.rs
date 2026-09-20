// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! TypeSafe System One compatibility at `POST /v1/systemone`.

use crate::common;

use std::collections::HashMap;
use std::io::Write as _;
use std::sync::Arc;

use aiplane::rama_server::{RamaState, SessionStore};
use aiplane_core::server::rbac::Resolver;
use aiplane_core::server::rbac::config::{RbacConfig, RoleConfig, RoleMapping};
use aiplane_core::server::upstreams::{
    self,
    config::{AliasSpec, PickerStrategy, PoolKind, UpstreamPoolConfig},
};
use aiplane_core::server::{Config, db};
use aiplane_runtime::server::AppState;
use common::Service as _;
use rama::http::{Body, Method, Request, StatusCode};
use serde_json::{Value, json};
use wiremock::matchers::{body_json, body_string, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const LATEST_ALIAS: &str = "jev-latest";
const LATEST_ID: &str = "~typesafe/jev-latest";
const PINNED_ID: &str = "typesafe/jev-1.13";
const CHAT_ID: &str = "openai/chat-model";

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

fn request_raw(bearer: &str, body: &str) -> Request {
    Request::builder()
        .method(Method::POST)
        .uri("/v1/systemone")
        .header("authorization", format!("Bearer {bearer}"))
        .header("content-type", "application/json")
        .header("x-typesafe-feature", "opaque-v2")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn models_request(bearer: &str) -> Request {
    Request::builder()
        .method(Method::GET)
        .uri("/v1/models")
        .header("authorization", format!("Bearer {bearer}"))
        .body(Body::empty())
        .unwrap()
}

fn chat_request(bearer: &str, model: &str) -> Request {
    Request::builder()
        .method(Method::POST)
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {bearer}"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"model": model, "messages": [{"role": "user", "content": "hello"}]}).to_string(),
        ))
        .unwrap()
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

fn system_one_pool(upstream_url: &str, allowed_groups: &[&str]) -> UpstreamPoolConfig {
    let mut backend = common::mock_backend("openrouter", upstream_url);
    backend.models = vec![LATEST_ID.into(), PINNED_ID.into()];
    backend.alias = Some(AliasSpec::Targets(HashMap::from([(
        LATEST_ALIAS.into(),
        LATEST_ID.into(),
    )])));
    UpstreamPoolConfig {
        voices: Default::default(),
        offer_voices: Vec::new(),
        allowed_groups: allowed_groups
            .iter()
            .map(|group| (*group).to_string())
            .collect(),
        fallback_offline: None,
        compliance: Default::default(),
        enforce_limits: true,
        kind: PoolKind::SystemOne,
        strategy: PickerStrategy::RoundRobin,
        models: Vec::new(),
        backend: vec![backend],
    }
}

fn chat_pool(upstream_url: &str) -> UpstreamPoolConfig {
    let mut backend = common::mock_backend("openrouter", upstream_url);
    backend.models = vec![CHAT_ID.into()];
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
        backend: vec![backend],
    }
}

async fn state_with_pools(
    pools: HashMap<String, UpstreamPoolConfig>,
    resolver: Arc<Resolver>,
) -> RamaState {
    let db_pool = db::open(std::path::Path::new(":memory:")).await.unwrap();
    let registry = upstreams::UpstreamRegistry::new(&pools).unwrap();
    let tools = Arc::new(aiplane_runtime::server::tools::ToolRegistry::new());
    let app = AppState::new(
        Config::default(),
        db_pool.clone(),
        registry,
        tools,
        resolver,
    );
    let sessions = SessionStore::new(db_pool, common::TEST_SECRET);
    RamaState::new(
        app,
        sessions,
        aiplane_core::server::usage::UsageHandle::disabled(),
    )
}

async fn state_with_system_one(upstream_url: &str) -> RamaState {
    state_with_pools(
        HashMap::from([("system-one".into(), system_one_pool(upstream_url, &[]))]),
        Arc::new(Resolver::empty()),
    )
    .await
}

fn system_one_rbac() -> Arc<Resolver> {
    Arc::new(
        Resolver::build(
            RbacConfig {
                default_role: Some("ordinary".into()),
                mappings: vec![RoleMapping {
                    oidc_claim: "groups".into(),
                    oidc_value: "system-one-users".into(),
                    role: "system-one-user".into(),
                }],
            },
            vec![
                RoleConfig {
                    id: "ordinary".into(),
                    admin: false,
                    models: vec!["*".into()],
                    tools: Vec::new(),
                    skills: Vec::new(),
                },
                RoleConfig {
                    id: "system-one-user".into(),
                    admin: false,
                    models: vec!["*".into()],
                    tools: Vec::new(),
                    skills: Vec::new(),
                },
            ],
        )
        .unwrap(),
    )
}

async fn seed_user_with_roles(state: &RamaState, user_id: &str, roles: &[&str]) -> String {
    use aiplane_core::server::auth::token;
    use aiplane_core::server::db::{tokens, users};
    use jiff::{SignedDuration, Timestamp};

    let now = Timestamp::now();
    users::upsert(
        &state.db,
        &users::User {
            id: user_id.into(),
            email: format!("{user_id}@example.com"),
            name: None,
            roles: roles.iter().map(|role| (*role).to_string()).collect(),
            created_at: now,
            updated_at: now,
            timezone: None,
            speech_voice: None,
        },
    )
    .await
    .unwrap();
    let (bearer, hash) = token::mint();
    tokens::insert(
        &state.db,
        &tokens::Token {
            id: uuid::Uuid::new_v4().to_string(),
            user_id: user_id.into(),
            name: "test".into(),
            hash,
            created_at: now,
            last_used_at: None,
            expires_at: now + SignedDuration::from_hours(1),
            revoked_at: None,
            tools_enabled: false,
        },
    )
    .await
    .unwrap();
    bearer
}

async fn model_ids<S>(app: &S, bearer: &str) -> Vec<String>
where
    S: rama::Service<Request, Output = rama::http::Response, Error = std::convert::Infallible>,
{
    let response = app.serve(models_request(bearer)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&common::read_body(response).await).unwrap();
    body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|model| model["id"].as_str().unwrap().to_string())
        .collect()
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
async fn ordinary_users_get_an_opaque_relay_for_every_system_one_primitive() {
    let upstream = MockServer::start().await;
    let upstream_request = format!(
        r#"{{"model":"{PINNED_ID}","state":{{"ticket":"My card was charged twice."}},"questions":{{"urgent":{{"type":"noul","instructions":"Does this need an immediate response?","criteria":{{"true":"Urgent","false":"Can wait"}}}},"team":{{"type":"choice","instructions":"Which team owns this?","criteria":{{"billing":null,"technical":null}}}},"priority":{{"type":"score","instructions":"How urgent is this?","criteria":{{"min":0,"max":10}}}},"future":{{"type":"future-primitive","shape":{{"kept":true}}}}}},"future_option":{{"kept":true}}}}"#
    );
    let upstream_response = format!(
        r#"{{ "model": "{PINNED_ID}", "answers": {{ "urgent": {{"type":"noul","noul":0.93}}, "team": {{"type":"choice","choice":"billing","confidence":0.88,"probabilities":{{"billing":0.94,"technical":0.06}}}}, "priority": {{"type":"score","score":8.5,"confidence":0.81}}, "future": {{"type":"future-primitive","opaque":[1,2,3]}} }}, "usage": {{"input_tokens":41,"output_tokens":7}}, "future_response": true }}"#
    );
    Mock::given(method("POST"))
        .and(path("/systemone"))
        .and(header("x-typesafe-feature", "opaque-v2"))
        .and(body_string(upstream_request.clone()))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-typesafe-trace", "trace-123")
                .set_body_raw(upstream_response.clone(), "application/json"),
        )
        .expect(1)
        .mount(&upstream)
        .await;

    let state = state_with_system_one(&upstream.uri()).await;
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);
    let resp = app
        .serve(request_raw(&bearer, &upstream_request))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get("x-typesafe-trace")
            .and_then(|value| value.to_str().ok()),
        Some("trace-123")
    );
    assert_eq!(common::read_body(resp).await, upstream_response.as_bytes());
}

#[tokio::test]
async fn aliases_rewrite_to_latest_while_pinned_ids_stay_pinned_and_all_are_listed() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/systemone"))
        .and(body_json(json!({
            "model": LATEST_ID,
            "state": "hello",
            "questions": {"positive": {"type": "noul"}}
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "model": LATEST_ID,
            "answers": {"positive": {"type": "noul", "noul": 0.5}},
            "usage": {"input_tokens": 3, "output_tokens": 1}
        })))
        .expect(1)
        .mount(&upstream)
        .await;
    Mock::given(method("POST"))
        .and(path("/systemone"))
        .and(body_json(json!({
            "model": PINNED_ID,
            "state": "hello",
            "questions": {"positive": {"type": "noul"}}
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "model": PINNED_ID,
            "answers": {"positive": {"type": "noul", "noul": 0.6}},
            "usage": {"input_tokens": 3, "output_tokens": 1}
        })))
        .expect(1)
        .mount(&upstream)
        .await;

    let state = state_with_system_one(&upstream.uri()).await;
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);
    let alias_response = app
        .serve(request(
            Some(&bearer),
            json!({
                "model": LATEST_ALIAS,
                "state": "hello",
                "questions": {"positive": {"type": "noul"}}
            }),
        ))
        .await
        .unwrap();
    assert_eq!(alias_response.status(), StatusCode::OK);
    assert_eq!(
        alias_response
            .headers()
            .get("x-gateway-resolved-model")
            .and_then(|value| value.to_str().ok()),
        Some(LATEST_ID)
    );

    let pinned_response = app
        .serve(request(
            Some(&bearer),
            json!({
                "model": PINNED_ID,
                "state": "hello",
                "questions": {"positive": {"type": "noul"}}
            }),
        ))
        .await
        .unwrap();
    assert_eq!(pinned_response.status(), StatusCode::OK);
    assert!(
        !pinned_response
            .headers()
            .contains_key("x-gateway-resolved-model")
    );

    assert_eq!(
        model_ids(&app, &bearer).await,
        vec![
            LATEST_ALIAS.to_string(),
            PINNED_ID.to_string(),
            LATEST_ID.to_string(),
        ]
    );
    upstream.verify().await;
}

#[tokio::test]
async fn pool_rbac_hides_system_one_from_non_members_and_allows_members() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/systemone"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "model": PINNED_ID,
            "answers": {"positive": {"type": "noul", "noul": 0.5}}
        })))
        .expect(1)
        .mount(&upstream)
        .await;
    let state = state_with_pools(
        HashMap::from([(
            "system-one".into(),
            system_one_pool(&upstream.uri(), &["system-one-user"]),
        )]),
        system_one_rbac(),
    )
    .await;
    let ordinary = common::seed_user_with_token(&state, "ordinary").await;
    let member = seed_user_with_roles(&state, "member", &["system-one-users"]).await;
    let app = common::app(state);

    let denied = app
        .serve(request(
            Some(&ordinary),
            json!({"model": PINNED_ID, "state": "hello", "questions": {}}),
        ))
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::NOT_FOUND);
    assert!(model_ids(&app, &ordinary).await.is_empty());

    let allowed = app
        .serve(request(
            Some(&member),
            json!({"model": PINNED_ID, "state": "hello", "questions": {}}),
        ))
        .await
        .unwrap();
    assert_eq!(allowed.status(), StatusCode::OK);
    assert_eq!(
        model_ids(&app, &member).await,
        vec![
            LATEST_ALIAS.to_string(),
            PINNED_ID.to_string(),
            LATEST_ID.to_string(),
        ]
    );
    upstream.verify().await;
}

#[tokio::test]
async fn token_allowlists_apply_to_system_one_routing_aliases_and_listing() {
    use aiplane_core::server::db::limits::ManagedBy;
    use aiplane_core::server::db::token_models;

    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/systemone"))
        .and(body_json(json!({
            "model": LATEST_ID,
            "state": "hello",
            "questions": {}
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "model": LATEST_ID,
            "answers": {}
        })))
        .expect(1)
        .mount(&upstream)
        .await;
    let state = state_with_system_one(&upstream.uri()).await;
    let (bearer, token_id) = common::seed_user_with_token_id(&state, "alice").await;
    token_models::set_for_token(
        &state.db,
        &token_id,
        &[LATEST_ALIAS.into()],
        ManagedBy::Owner,
    )
    .await
    .unwrap();
    let app = common::app(state);

    let allowed = app
        .serve(request(
            Some(&bearer),
            json!({"model": LATEST_ALIAS, "state": "hello", "questions": {}}),
        ))
        .await
        .unwrap();
    assert_eq!(allowed.status(), StatusCode::OK);
    assert_eq!(model_ids(&app, &bearer).await, vec![LATEST_ALIAS]);

    let denied = app
        .serve(request(
            Some(&bearer),
            json!({"model": PINNED_ID, "state": "hello", "questions": {}}),
        ))
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    let body: Value = serde_json::from_slice(&common::read_body(denied).await).unwrap();
    assert_eq!(body["error"]["code"], "model_not_allowed");
    assert_eq!(body["error"]["param"], "model");
    upstream.verify().await;
}

#[tokio::test]
async fn shared_backend_url_keeps_chat_and_system_one_models_on_their_own_protocols() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/systemone"))
        .and(body_json(json!({
            "model": PINNED_ID,
            "state": "hello",
            "questions": {}
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "model": PINNED_ID,
            "answers": {}
        })))
        .expect(1)
        .mount(&upstream)
        .await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_json(json!({
            "model": CHAT_ID,
            "messages": [{"role": "user", "content": "hello"}]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "model": CHAT_ID,
            "choices": [{"message": {"role": "assistant", "content": "hello"}}]
        })))
        .expect(1)
        .mount(&upstream)
        .await;
    let state = state_with_pools(
        HashMap::from([
            ("chat".into(), chat_pool(&upstream.uri())),
            ("system-one".into(), system_one_pool(&upstream.uri(), &[])),
        ]),
        Arc::new(Resolver::empty()),
    )
    .await;
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);

    let system_one_response = app
        .serve(request(
            Some(&bearer),
            json!({"model": PINNED_ID, "state": "hello", "questions": {}}),
        ))
        .await
        .unwrap();
    assert_eq!(system_one_response.status(), StatusCode::OK);
    let chat_response = app.serve(chat_request(&bearer, CHAT_ID)).await.unwrap();
    assert_eq!(chat_response.status(), StatusCode::OK);

    let chat_model_on_system_one = app
        .serve(request(
            Some(&bearer),
            json!({"model": CHAT_ID, "state": "hello", "questions": {}}),
        ))
        .await
        .unwrap();
    assert_eq!(chat_model_on_system_one.status(), StatusCode::NOT_FOUND);
    let system_one_model_on_chat = app.serve(chat_request(&bearer, PINNED_ID)).await.unwrap();
    assert_eq!(system_one_model_on_chat.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        model_ids(&app, &bearer).await,
        vec![
            LATEST_ALIAS.to_string(),
            CHAT_ID.to_string(),
            PINNED_ID.to_string(),
            LATEST_ID.to_string(),
        ]
    );
    upstream.verify().await;
}

#[tokio::test]
async fn gzip_capable_clients_do_not_hide_system_one_usage_from_accounting() {
    let upstream = MockServer::start().await;
    let upstream_response = json!({
        "id": "gen-dec-01K5K8M9KQ2JY7QH2STXAV4N4R",
        "provider": "TypeSafe",
        "model": "typesafe/jev-1.13-20260917",
        "answers": {
            "urgent": {"type": "noul", "noul": 0.93},
            "team": {
                "type": "choice",
                "choice": "billing",
                "confidence": 0.88,
                "probabilities": {"billing": 0.94, "technical": 0.06}
            },
            "priority": {"type": "score", "score": 8.5, "confidence": 0.81}
        },
        "usage": {
            "input_tokens": 388,
            "output_tokens": 69,
            "cost": 0.000016296
        }
    });
    let plain = upstream_response.to_string();
    let compressed = gzip(plain.as_bytes());
    Mock::given(method("POST"))
        .and(path("/systemone"))
        .respond_with(move |request: &wiremock::Request| {
            let accepts_gzip = request
                .headers
                .get("accept-encoding")
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.contains("gzip"));
            if accepts_gzip {
                ResponseTemplate::new(200)
                    .insert_header("content-encoding", "gzip")
                    .set_body_bytes(compressed.clone())
            } else {
                ResponseTemplate::new(200).set_body_raw(plain.clone(), "application/json")
            }
        })
        .expect(1)
        .mount(&upstream)
        .await;
    let state = state_with_system_one(&upstream.uri()).await;
    let metered = aiplane_core::server::usage::spawn(state.db.clone(), 90);
    let state = state.with_usage(metered);
    let db = state.db.clone();
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);
    let mut req = request(
        Some(&bearer),
        json!({
            "model": PINNED_ID,
            "state": {"ticket": "My card was charged twice."},
            "questions": {
                "urgent": {"type": "noul"},
                "team": {"type": "choice"},
                "priority": {"type": "score"}
            }
        }),
    );
    req.headers_mut()
        .insert("accept-encoding", "gzip, deflate".parse().unwrap());

    let response = app.serve(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!response.headers().contains_key("content-encoding"));
    let body: Value = serde_json::from_slice(&common::read_body(response).await).unwrap();
    assert_eq!(body, upstream_response);
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;

    let usage: (i64, i64, i64) = sqlx::query_as(
        "SELECT prompt_tokens, completion_tokens, total_tokens FROM usage_events LIMIT 1",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(usage, (388, 69, 457));
    let requests = upstream.received_requests().await.unwrap();
    assert_eq!(
        requests[0]
            .headers
            .get("accept-encoding")
            .and_then(|value| value.to_str().ok()),
        Some("identity")
    );
}

#[tokio::test]
async fn records_system_one_usage() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/systemone"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "model": PINNED_ID,
            "answers": {"positive": {"type": "noul", "noul": 0.8}},
            "usage": {"input_tokens": 17, "output_tokens": 2}
        })))
        .mount(&upstream)
        .await;
    let state = state_with_system_one(&upstream.uri()).await;
    let metered = aiplane_core::server::usage::spawn(state.db.clone(), 90);
    let state = state.with_usage(metered);
    let db = state.db.clone();
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);
    let resp = app
        .serve(request(
            Some(&bearer),
            json!({
                "model": PINNED_ID,
                "state": "hello",
                "questions": {"positive": {"type": "noul"}}
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let _ = common::read_body(resp).await;
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;

    let row: (String, String, String, String, i64, i64, i64) = sqlx::query_as(
        "SELECT kind, backend, model, user_id, prompt_tokens, completion_tokens, total_tokens \
         FROM usage_events LIMIT 1",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(
        row,
        (
            "system_one".into(),
            "openrouter".into(),
            PINNED_ID.into(),
            "alice".into(),
            17,
            2,
            19,
        )
    );
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
    let state = state_with_system_one(&upstream.uri()).await;
    let bearer = common::seed_user_with_token(&state, "alice").await;
    let app = common::app(state);
    let resp = app
        .serve(request(
            Some(&bearer),
            json!({
                "model": PINNED_ID,
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
