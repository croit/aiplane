// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Data classification at runtime: what attached files raise, and how
//! generated files are marked. Offline: wiremock stands in for the object
//! store and the model upstreams.

use crate::common;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use aiplane::rama_server::{RamaState, router::router};
use aiplane_core::server::classification::{self, ClassificationConfig, OutputMarking};
use aiplane_core::server::config::S3Config;
use aiplane_core::server::db;
use aiplane_core::server::upstreams::{
    self,
    config::{PickerStrategy, PoolKind, UpstreamPoolConfig},
};
use aiplane_features::server::classification::read_markings;
use aiplane_runtime::classification_output::{TurnClassification, upload_stamped};
use aiplane_runtime::server::tools::ToolContext;
use common::Service as _;
use rama::http::{Body, Method, Request, StatusCode, header};
use session_core::db as chat;
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

const CONFIDENTIAL_GUID: &str = "9fbde396-1a24-4c79-8edf-9254a0f35055";
const INTERNAL_GUID: &str = "87ba5c36-b7cf-4793-bbc2-bd5b3a9f95ca";

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../aiplane-features/src/server/testdata/classification")
            .join(name),
    )
    .unwrap()
}

fn s3_config(endpoint: &str) -> S3Config {
    S3Config {
        endpoint: endpoint.to_string(),
        region: "us-east-1".into(),
        bucket: common::TEST_S3_BUCKET.into(),
        access_key: Some("test-access-key".into()),
        secret_key: Some("test-secret-key".into()),
        key_prefix: common::TEST_S3_PREFIX.into(),
    }
}

/// An ISO-style scheme (public < internal < confidential < strictly
/// confidential) with the fixtures' Purview labels mapped, a `low-model` pool
/// at the lowest level and a `high-model` pool cleared for everything. Chat
/// models, the System One model and the object store all answer from mocks.
async fn state(upstream: &str, s3: &str, active: bool) -> RamaState {
    let db_pool = db::open(std::path::Path::new(":memory:")).await.unwrap();
    let mut pools = HashMap::new();
    for (name, kind, rank, models) in [
        ("low", PoolKind::Chat, 0, vec![]),
        ("high", PoolKind::Chat, 3, vec![]),
        (
            "guard",
            PoolKind::SystemOne,
            0,
            vec!["guard-model".to_string()],
        ),
    ] {
        pools.insert(
            name.to_string(),
            UpstreamPoolConfig {
                voices: Default::default(),
                offer_voices: Vec::new(),
                allowed_groups: Vec::new(),
                fallback_offline: None,
                level_rank: rank,
                enforce_limits: true,
                kind,
                strategy: PickerStrategy::RoundRobin,
                models,
                backend: vec![common::mock_backend(name, upstream)],
            },
        );
    }
    let registry = upstreams::UpstreamRegistry::new(&pools).unwrap();
    common::seed_pool_models(&registry, "low", 0, &["low-model"]);
    common::seed_pool_models(&registry, "high", 0, &["high-model"]);
    let mut config = common::test_config();
    config.chat.s3 = Some(s3_config(s3));
    config.classification.system_one_model = "guard-model".into();
    if active {
        sqlx::raw_sql(&format!(
            "INSERT INTO classification_levels (id, rank, name, color, filename_tag, mip_label_id) VALUES
                 ('internal', 1, 'Internal', 'info', 'internal', ''),
                 ('confidential', 2, 'Confidential', 'warning', 'confidential', '{CONFIDENTIAL_GUID}'),
                 ('strictly_confidential', 3, 'Strictly confidential', 'error', 'strict', '');
             INSERT INTO classification_markings (kind, value, level_id) VALUES
                 ('mip', '{CONFIDENTIAL_GUID}', 'confidential'),
                 ('mip', '{INTERNAL_GUID}', 'internal');"
        ))
        .execute(&db_pool)
        .await
        .unwrap();
    }
    let state = common::state_from_registry_with_config(db_pool, registry, config);
    state.reload_scheme().await;
    state
}

async fn object_store() -> MockServer {
    let s3 = MockServer::start().await;
    Mock::given(method("PUT"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&s3)
        .await;
    for (name, mime) in [
        ("mip_confidential.docx", "application/octet-stream"),
        ("mip_internal.pdf", "application/pdf"),
    ] {
        Mock::given(method("GET"))
            .and(path_regex(format!(".*/{}$", regex_escape(name))))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", mime)
                    .set_body_bytes(fixture(name)),
            )
            .mount(&s3)
            .await;
    }
    s3
}

fn regex_escape(name: &str) -> String {
    name.replace('.', "\\.")
}

async fn chat_upstream() -> MockServer {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\ndata: [DONE]\n\n",
            "text/event-stream",
        ))
        .mount(&upstream)
        .await;
    upstream
}

/// Turn requests; the title side call is not one.
async fn chat_requests(upstream: &MockServer) -> usize {
    upstream
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|request| request.url.path() == "/chat/completions")
        .filter(|request| {
            !String::from_utf8_lossy(&request.body).contains("You name chat conversations")
        })
        .count()
}

fn submit(session_id: &str, cookie: &str, model: &str, file: Option<(&str, &str)>) -> Request {
    let boundary = "----classification";
    let mut body = Vec::new();
    for (name, value) in [("model", model), ("message", "please read this")] {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n")
                .as_bytes(),
        );
    }
    if let Some((filename, mime)) = file {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            format!(
                "Content-Disposition: form-data; name=\"attachment\"; filename=\"{filename}\"\r\n\
                 Content-Type: {mime}\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(&fixture(filename));
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    Request::builder()
        .method(Method::POST)
        .uri(format!("/api/v0/chat/sessions/{session_id}/messages"))
        .header(header::COOKIE, format!("id={cookie}"))
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap()
}

async fn wait_settled(state: &RamaState, session_id: &str) {
    for _ in 0..200 {
        let busy = chat::list_turns(&state.db, session_id)
            .await
            .unwrap()
            .iter()
            .any(|t| t.turn.status == chat::TurnStatus::InProgress);
        if !busy {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("turn never settled");
}

async fn events(state: &RamaState, session_id: &str) -> Vec<(String, String, String)> {
    sqlx::query_as(
        "SELECT from_level, to_level, cause FROM chat_classification_events
          WHERE session_id = ? ORDER BY at, rowid",
    )
    .bind(session_id)
    .fetch_all(&state.db)
    .await
    .unwrap()
}

async fn last_turn_status(state: &RamaState, session_id: &str) -> (chat::TurnStatus, String) {
    let turns = chat::list_turns(&state.db, session_id).await.unwrap();
    let last = turns.last().unwrap();
    (
        last.turn.status,
        last.turn.error_message.clone().unwrap_or_default(),
    )
}

#[tokio::test]
async fn a_labelled_upload_raises_the_conversation_and_an_uncleared_model_is_refused() {
    let upstream = chat_upstream().await;
    let s3 = object_store().await;
    let state = state(&upstream.uri(), &s3.uri(), true).await;
    let cookie = common::seed_session(&state, "alice", "alice@example.com").await;
    let state = Arc::new(state);
    let session = chat::create_session(&state.db, "alice").await.unwrap();
    let app = router(state.clone());

    let resp = app
        .serve(submit(
            &session.id,
            &cookie,
            "low-model",
            Some(("mip_confidential.docx", "application/octet-stream")),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    wait_settled(&state, &session.id).await;

    assert_eq!(
        events(&state, &session.id).await,
        vec![(
            "public".into(),
            "confidential".into(),
            format!("marking:mip:{CONFIDENTIAL_GUID}")
        )]
    );
    let (status, error) = last_turn_status(&state, &session.id).await;
    assert_eq!(status, chat::TurnStatus::Errored);
    assert!(error.contains("Confidential"), "{error}");
    assert_eq!(chat_requests(&upstream).await, 0, "no model saw the file");

    let resp = app
        .serve(submit(&session.id, &cookie, "high-model", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    wait_settled(&state, &session.id).await;
    assert_eq!(
        last_turn_status(&state, &session.id).await.0,
        chat::TurnStatus::Completed
    );
    assert_eq!(chat_requests(&upstream).await, 1);
}

#[tokio::test]
async fn a_labelled_pdf_raises_to_its_level_and_a_cleared_model_answers() {
    let upstream = chat_upstream().await;
    let s3 = object_store().await;
    let state = state(&upstream.uri(), &s3.uri(), true).await;
    let cookie = common::seed_session(&state, "alice", "alice@example.com").await;
    let state = Arc::new(state);
    let session = chat::create_session(&state.db, "alice").await.unwrap();

    let resp = router(state.clone())
        .serve(submit(
            &session.id,
            &cookie,
            "high-model",
            Some(("mip_internal.pdf", "application/pdf")),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    wait_settled(&state, &session.id).await;

    assert_eq!(
        events(&state, &session.id).await,
        vec![(
            "public".into(),
            "internal".into(),
            format!("marking:mip:{INTERNAL_GUID}")
        )]
    );
    assert_eq!(
        last_turn_status(&state, &session.id).await.0,
        chat::TurnStatus::Completed
    );
}

#[tokio::test]
async fn with_one_level_an_upload_is_neither_fetched_nor_parsed_nor_asked_about() {
    let upstream = chat_upstream().await;
    let s3 = object_store().await;
    let state = state(&upstream.uri(), &s3.uri(), false).await;
    let cookie = common::seed_session(&state, "alice", "alice@example.com").await;
    let state = Arc::new(state);
    let session = chat::create_session(&state.db, "alice").await.unwrap();

    let resp = router(state.clone())
        .serve(submit(
            &session.id,
            &cookie,
            "low-model",
            Some(("mip_confidential.docx", "application/octet-stream")),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    wait_settled(&state, &session.id).await;

    assert!(events(&state, &session.id).await.is_empty());
    assert_eq!(
        last_turn_status(&state, &session.id).await.0,
        chat::TurnStatus::Completed
    );
    let reads = s3
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|request| request.method == wiremock::http::Method::GET)
        .count();
    assert_eq!(reads, 0, "the file was not downloaded to be inspected");
    assert!(
        upstream
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|request| request.url.path() != "/systemone"),
        "the detector asked nothing"
    );
}

async fn generated_context(
    state: &RamaState,
    s3: &MockServer,
    level: &str,
) -> (ToolContext, S3Config) {
    common::seed_session(state, "alice", "alice@example.com").await;
    let session = chat::create_session(&state.db, "alice").await.unwrap();
    classification::set_level(&state.db, &session.id, level)
        .await
        .unwrap();
    let mut config = state.config().classification.clone();
    config.output = OutputMarking {
        visible: true,
        metadata: true,
        filename: true,
    };
    config.mip_tenant_id = "72f988bf-86f1-41af-91ab-2d7cd011db47".into();
    let context = ToolContext {
        session_id: Some(session.id),
        classification: Some(TurnClassification {
            scheme: state.scheme(),
            config: Arc::new(ClassificationConfig { ..config }),
        }),
        ..ToolContext::for_test(state.db.clone())
    };
    (context, s3_config(&s3.uri()))
}

async fn stored(s3: &MockServer) -> Vec<(String, Vec<u8>)> {
    s3.received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|request| request.method == wiremock::http::Method::PUT)
        .map(|request| (request.url.path().to_string(), request.body.clone()))
        .collect()
}

#[tokio::test]
async fn a_generated_json_gets_the_level_tag_in_its_name() {
    let upstream = chat_upstream().await;
    let s3 = object_store().await;
    let state = state(&upstream.uri(), &s3.uri(), true).await;
    let (context, cfg) = generated_context(&state, &s3, "confidential").await;

    let outcome = upload_stamped(
        &context,
        &cfg,
        "turn-1",
        "offer.json",
        "application/json",
        br#"{"price":1}"#.to_vec(),
    )
    .await
    .unwrap();

    assert_eq!(outcome.filename, "offer-confidential.json");
    let puts = stored(&s3).await;
    assert_eq!(puts.len(), 1);
    assert!(
        puts[0].0.ends_with("/turn-1/offer-confidential.json"),
        "{}",
        puts[0].0
    );
    assert_eq!(
        puts[0].1, br#"{"price":1}"#,
        "a json has no marking to write"
    );
}

#[tokio::test]
async fn a_generated_docx_carries_the_levels_label() {
    let upstream = chat_upstream().await;
    let s3 = object_store().await;
    let state = state(&upstream.uri(), &s3.uri(), true).await;
    let (context, cfg) = generated_context(&state, &s3, "confidential").await;

    let outcome = upload_stamped(
        &context,
        &cfg,
        "turn-1",
        "report.docx",
        "application/octet-stream",
        fixture("unlabelled.docx"),
    )
    .await
    .unwrap();

    assert_eq!(outcome.filename, "report-confidential.docx");
    let puts = stored(&s3).await;
    let markings = read_markings(
        "report-confidential.docx",
        "application/octet-stream",
        &puts[0].1,
    );
    assert_eq!(markings.found.len(), 1, "{markings:?}");
    assert_eq!(markings.found[0].value, CONFIDENTIAL_GUID);
}

#[tokio::test]
async fn generated_files_are_stored_unchanged_at_the_lowest_level_and_with_one_level() {
    let upstream = chat_upstream().await;
    let s3 = object_store().await;
    let active = state(&upstream.uri(), &s3.uri(), true).await;
    let (context, cfg) = generated_context(&active, &s3, "public").await;
    let docx = fixture("unlabelled.docx");
    let outcome = upload_stamped(
        &context,
        &cfg,
        "t",
        "a.docx",
        "application/octet-stream",
        docx.clone(),
    )
    .await
    .unwrap();
    assert_eq!(outcome.filename, "a.docx");

    let single = state(&upstream.uri(), &s3.uri(), false).await;
    let (context, cfg) = generated_context(&single, &s3, "public").await;
    let outcome = upload_stamped(
        &context,
        &cfg,
        "t",
        "b.docx",
        "application/octet-stream",
        docx.clone(),
    )
    .await
    .unwrap();
    assert_eq!(outcome.filename, "b.docx");
    for (_, body) in stored(&s3).await {
        assert_eq!(body, docx);
    }
}
