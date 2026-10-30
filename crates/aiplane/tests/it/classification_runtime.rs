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
    state_with(upstream, s3, active, "", |_| {}).await
}

async fn state_with(
    upstream: &str,
    s3: &str,
    active: bool,
    extra_sql: &str,
    configure: impl FnOnce(&mut aiplane_core::server::config::Config),
) -> RamaState {
    state_full(
        upstream,
        s3,
        active,
        extra_sql,
        configure,
        Arc::new(aiplane_runtime::server::tools::ToolRegistry::new()),
        Arc::new(aiplane_core::server::rbac::Resolver::empty()),
    )
    .await
}

async fn state_full(
    upstream: &str,
    s3: &str,
    active: bool,
    extra_sql: &str,
    configure: impl FnOnce(&mut aiplane_core::server::config::Config),
    tools: Arc<aiplane_runtime::server::tools::ToolRegistry>,
    rbac: Arc<aiplane_core::server::rbac::Resolver>,
) -> RamaState {
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
    configure(&mut config);
    if active {
        sqlx::raw_sql(&format!(
            "INSERT INTO classification_levels (id, rank, name, color, filename_tag, mip_label_id) VALUES
                 ('internal', 1, 'Internal', 'info', 'internal', ''),
                 ('confidential', 2, 'Confidential', 'warning', 'confidential', '{CONFIDENTIAL_GUID}'),
                 ('strictly_confidential', 3, 'Strictly confidential', 'error', 'strict', '');
             INSERT INTO classification_markings (kind, value, level_id) VALUES
                 ('mip', '{CONFIDENTIAL_GUID}', 'confidential'),
                 ('mip', '{INTERNAL_GUID}', 'internal');
             {extra_sql}"
        ))
        .execute(&db_pool)
        .await
        .unwrap();
    }
    let state = common::state_from_parts(db_pool, registry, config, tools, rbac);
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
        ("iban.txt", "text/plain"),
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
    submit_message(session_id, cookie, model, "please read this", file)
}

fn submit_message(
    session_id: &str,
    cookie: &str,
    model: &str,
    message: &str,
    file: Option<(&str, &str)>,
) -> Request {
    let boundary = "----classification";
    let mut body = Vec::new();
    for (name, value) in [("model", model), ("message", message)] {
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

const GATE_SQL: &str = "
    INSERT INTO classification_data_classes (id, name, level_id, rule_question, rule_positive, rule_negative, rule_threshold, action)
        VALUES ('secret', 'Secret project', 'confidential', '', '', '', 0.5, 'raise'),
               ('pii', 'Personal data', 'confidential', 'Does the text contain personal data?', 'a name and an address', 'a recipe', 0.5, 'raise');
    INSERT INTO classification_terms (class_id, term) VALUES ('secret', 'project-falcon');
    INSERT INTO classification_assignments (kind, id, level_id, over) VALUES
        ('tool', 'company_echo', 'internal', 'gate'),
        ('tool', 'get_current_timestamp', 'internal', 'hide'),
        ('mcp', 'wiki', 'internal', 'hide'),
        ('connector', 'wiki', 'confidential', 'hide'),
        ('tool', 'edit_image', 'internal', 'hide'),
        ('collection', '7', 'confidential', 'hide');
";

async fn system_one(answer: Option<f64>) -> MockServer {
    let server = MockServer::start().await;
    let response = match answer {
        Some(score) => ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "answers": {"pii": {"type": "noul", "noul": score}}
        })),
        None => ResponseTemplate::new(503),
    };
    Mock::given(method("POST"))
        .and(path("/systemone"))
        .respond_with(response)
        .mount(&server)
        .await;
    server
}

async fn system_one_calls(server: &MockServer) -> usize {
    server.received_requests().await.unwrap().len()
}

/// A tool that can send data out of AIplane, which the real echo and
/// timestamp tools do not.
struct Outbound(&'static str);

impl aiplane_runtime::server::tools::Tool for Outbound {
    fn id(&self) -> &str {
        self.0
    }
    fn schema(&self) -> shared::api::ToolDef {
        probe_schema(self.0)
    }
    fn run<'a>(
        &'a self,
        _ctx: ToolContext,
        args: serde_json::Value,
    ) -> aiplane_runtime::server::tools::ToolFuture<'a> {
        Box::pin(async move { Ok(args) })
    }
}

fn probe_schema(id: &str) -> shared::api::ToolDef {
    shared::api::ToolDef::function(
        id,
        "a test probe",
        serde_json::json!({"type": "object", "properties": {"message": {"type": "string"}}}),
    )
}

struct Gate {
    source_state: Arc<aiplane_runtime::server::AppState>,
    context: ToolContext,
}

async fn gate(
    upstream: &MockServer,
    level: &str,
    configure: impl FnOnce(&mut aiplane_core::server::config::Config),
) -> Gate {
    let s3 = object_store().await;
    let state = state_with(&upstream.uri(), &s3.uri(), true, GATE_SQL, configure).await;
    common::seed_session(&state, "alice", "alice@example.com").await;
    let session = chat::create_session(&state.db, "alice").await.unwrap();
    classification::set_level(&state.db, &session.id, level)
        .await
        .unwrap();
    Gate {
        context: ToolContext {
            session_id: Some(session.id),
            classification: Some(TurnClassification {
                scheme: state.scheme(),
                config: Arc::new(state.config().classification.clone()),
            }),
            suspend: aiplane_runtime::suspend::Suspend::Available,
            ..ToolContext::for_test(state.db.clone())
        },
        source_state: Arc::new(aiplane_runtime::server::AppState::clone(&state)),
    }
}

async fn call(
    gate: &Gate,
    tool: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    call_with(gate, &gate.context, tool, args).await
}

async fn call_with(
    gate: &Gate,
    context: &ToolContext,
    tool: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use aiplane_runtime::server::tools::{ToolRegistry, ToolSource as _};
    let registry = ToolRegistry::new().with(Outbound("company_echo"));
    let source = aiplane_runtime::classification_gate::ClassifiedToolSource::new(
        &registry,
        gate.source_state.clone(),
    );
    let tool = source.get(tool).expect("the tool is registered");
    tool.run(context.clone(), args)
        .await
        .map_err(|error| error.to_string())
}

fn pause(body: &serde_json::Value) -> aiplane_runtime::suspend::SuspendRequest {
    aiplane_runtime::suspend::extract_suspend(body).expect("the call paused for an approval")
}

#[tokio::test]
async fn a_gated_tool_runs_when_its_arguments_are_clean() {
    let guard = system_one(Some(0.0)).await;
    let gate = gate(&guard, "confidential", |_| {}).await;
    let out = call(
        &gate,
        "company_echo",
        serde_json::json!({"message": "hello"}),
    )
    .await
    .unwrap();
    assert!(
        aiplane_runtime::suspend::extract_suspend(&out).is_none(),
        "{out}"
    );
    assert_eq!(system_one_calls(&guard).await, 1);
}

#[tokio::test]
async fn a_gated_tool_asks_with_the_matched_class_names_and_runs_once_approved() {
    let guard = system_one(Some(0.0)).await;
    let gate = gate(&guard, "confidential", |_| {}).await;
    let args = serde_json::json!({"message": "the plan of project-falcon"});
    let paused = call(&gate, "company_echo", args.clone()).await.unwrap();
    let request = pause(&paused);
    assert_eq!(request.message.as_deref(), Some("Secret project"));
    assert_eq!(
        request.kind,
        session_core::db::SuspensionKind::Approval,
        "the existing approval protocol, not a new one"
    );

    let approved = ToolContext {
        suspend: aiplane_runtime::suspend::Suspend::Decided(
            session_core::db::SuspensionKind::Approval,
            session_core::db::Decision::AllowOnce,
        ),
        ..gate.context.clone()
    };
    let out = call_with(&gate, &approved, "company_echo", args)
        .await
        .unwrap();
    assert!(
        aiplane_runtime::suspend::extract_suspend(&out).is_none(),
        "{out}"
    );
}

#[tokio::test]
async fn a_system_one_match_in_the_arguments_asks_too() {
    let guard = system_one(Some(0.9)).await;
    let gate = gate(&guard, "confidential", |_| {}).await;
    let paused = call(
        &gate,
        "company_echo",
        serde_json::json!({"message": "Ada, 1 Main St"}),
    )
    .await
    .unwrap();
    assert_eq!(pause(&paused).message.as_deref(), Some("Personal data"));
}

#[tokio::test]
async fn with_the_block_policy_a_match_is_refused_without_asking() {
    use aiplane_core::server::classification::GateOnMatch;
    let guard = system_one(Some(0.0)).await;
    let gate = gate(&guard, "confidential", |config| {
        config.classification.gate_on_match = GateOnMatch::Block;
    })
    .await;
    let error = call(
        &gate,
        "company_echo",
        serde_json::json!({"message": "project-falcon"}),
    )
    .await
    .unwrap_err();
    assert!(
        error.contains("Secret project") && error.contains("was not called"),
        "{error}"
    );
}

#[tokio::test]
async fn an_unreachable_gate_model_blocks_the_call_and_says_so() {
    let guard = system_one(None).await;
    let gate = gate(&guard, "confidential", |_| {}).await;
    let error = call(
        &gate,
        "company_echo",
        serde_json::json!({"message": "hello"}),
    )
    .await
    .unwrap_err();
    assert!(error.contains("could not be reached"), "{error}");
}

#[tokio::test]
async fn a_tool_cleared_for_the_conversation_runs_without_any_check() {
    let guard = system_one(Some(1.0)).await;
    let gate = gate(&guard, "internal", |_| {}).await;
    let out = call(
        &gate,
        "company_echo",
        serde_json::json!({"message": "project-falcon"}),
    )
    .await
    .unwrap();
    assert!(
        aiplane_runtime::suspend::extract_suspend(&out).is_none(),
        "{out}"
    );
    assert_eq!(system_one_calls(&guard).await, 0);
}

#[tokio::test]
async fn in_monitor_mode_a_gated_match_is_only_logged() {
    use aiplane_core::server::classification::ClassificationMode;
    let guard = system_one(Some(0.0)).await;
    let gate = gate(&guard, "confidential", |config| {
        config.classification.mode = ClassificationMode::Monitor;
    })
    .await;
    let out = call(
        &gate,
        "company_echo",
        serde_json::json!({"message": "project-falcon"}),
    )
    .await
    .unwrap();
    assert!(
        aiplane_runtime::suspend::extract_suspend(&out).is_none(),
        "{out}"
    );
}

#[tokio::test]
async fn a_hidden_tool_above_its_level_is_refused_when_called() {
    use aiplane_runtime::server::tools::{ToolRegistry, ToolSource as _};
    let guard = system_one(Some(0.0)).await;
    let gate = gate(&guard, "confidential", |_| {}).await;
    let registry = ToolRegistry::new().with(Outbound("get_current_timestamp"));
    let source = aiplane_runtime::classification_gate::ClassifiedToolSource::new(
        &registry,
        gate.source_state.clone(),
    );
    let error = source
        .get("get_current_timestamp")
        .unwrap()
        .run(gate.context.clone(), serde_json::json!({}))
        .await
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("Confidential") && error.contains("not cleared"),
        "{error}"
    );
}

async fn raised_events(gate: &Gate) -> Vec<(String, String, String)> {
    let state = &gate.source_state;
    let session = gate.context.session_id.as_deref().unwrap();
    sqlx::query_as::<_, (String, String, String)>(
        "SELECT from_level, to_level, cause FROM chat_classification_events WHERE session_id = ? ORDER BY at, rowid",
    )
    .bind(session)
    .fetch_all(&state.db)
    .await
    .unwrap()
}

#[tokio::test]
async fn reading_a_classified_collection_raises_the_conversation_with_the_retrieval_cause() {
    use aiplane_runtime::classification_retrieval::raise_for_collection;
    let guard = system_one(Some(0.0)).await;
    let gate = gate(&guard, "internal", |_| {}).await;

    raise_for_collection(&gate.context, 7).await.unwrap();

    let session = gate.context.session_id.as_deref().unwrap();
    assert_eq!(
        classification::level_id(&gate.source_state.db, session)
            .await
            .unwrap()
            .as_deref(),
        Some("confidential")
    );
    let events = raised_events(&gate).await;
    assert_eq!(events.last().unwrap().2, "retrieval", "{events:?}");
}

#[tokio::test]
async fn an_unrated_collection_and_monitor_mode_leave_the_level_alone() {
    use aiplane_core::server::classification::ClassificationMode;
    use aiplane_runtime::classification_retrieval::raise_for_collection;
    let guard = system_one(Some(0.0)).await;
    let gate = gate(&guard, "internal", |_| {}).await;
    raise_for_collection(&gate.context, 99).await.unwrap();
    assert!(raised_events(&gate).await.is_empty());

    let monitored = self::gate(&guard, "internal", |config| {
        config.classification.mode = ClassificationMode::Monitor;
    })
    .await;
    raise_for_collection(&monitored.context, 7).await.unwrap();
    assert!(raised_events(&monitored).await.is_empty());
}

const IBAN: &str = "DE89370400440532013000";

fn iban_class(action: &str) -> String {
    format!(
        "INSERT INTO classification_data_classes (id, name, level_id, patterns, action)
         VALUES ('iban', 'Bank account', 'confidential', '[\"iban\"]', '{action}');"
    )
}

/// Send a message (with an optional file) to the uncleared model and settle.
async fn send_to_low_model(
    action: &str,
    mode: aiplane_core::server::classification::ClassificationMode,
    message: &str,
    file: Option<(&str, &str)>,
) -> (
    MockServer,
    Arc<RamaState>,
    String,
    (chat::TurnStatus, String),
) {
    let upstream = chat_upstream().await;
    let s3 = object_store().await;
    let state = state_with(
        &upstream.uri(),
        &s3.uri(),
        true,
        &iban_class(action),
        move |config| {
            config.classification.mode = mode;
        },
    )
    .await;
    let cookie = common::seed_session(&state, "alice", "alice@example.com").await;
    let state = Arc::new(state);
    let session = chat::create_session(&state.db, "alice").await.unwrap();
    let resp = router(state.clone())
        .serve(submit_message(
            &session.id,
            &cookie,
            "low-model",
            message,
            file,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    wait_settled(&state, &session.id).await;
    let status = last_turn_status(&state, &session.id).await;
    (upstream, state, session.id, status)
}

#[tokio::test]
async fn a_typed_iban_raises_the_conversation_and_the_uncleared_model_is_refused() {
    use aiplane_core::server::classification::ClassificationMode::Enforce;
    let (upstream, state, session, (status, error)) =
        send_to_low_model("raise", Enforce, &format!("pay {IBAN}"), None).await;
    assert_eq!(status, chat::TurnStatus::Errored);
    assert!(error.contains("Confidential"), "{error}");
    assert_eq!(
        events(&state, &session).await,
        vec![(
            "public".into(),
            "confidential".into(),
            "pattern:iban".into()
        )]
    );
    assert_eq!(chat_requests(&upstream).await, 0);
}

#[tokio::test]
async fn a_confirm_class_without_an_approval_stops_typed_text_and_uploads_alike() {
    use aiplane_core::server::classification::ClassificationMode::Enforce;
    for (message, file) in [
        (format!("pay {IBAN}"), None),
        ("read this".to_owned(), Some(("iban.txt", "text/plain"))),
    ] {
        let (upstream, _state, _session, (status, error)) =
            send_to_low_model("confirm", Enforce, &message, file).await;
        assert_eq!(status, chat::TurnStatus::Errored);
        assert!(error.contains("not approved"), "{error}");
        assert_eq!(chat_requests(&upstream).await, 0);
    }
}

#[tokio::test]
async fn a_block_class_refuses_typed_text_and_uploads_alike() {
    use aiplane_core::server::classification::ClassificationMode::Enforce;
    for (message, file) in [
        (format!("pay {IBAN}"), None),
        ("read this".to_owned(), Some(("iban.txt", "text/plain"))),
    ] {
        let (upstream, _state, _session, (status, error)) =
            send_to_low_model("block", Enforce, &message, file).await;
        assert_eq!(status, chat::TurnStatus::Errored);
        assert!(error.contains("prevents sending"), "{error}");
        assert_eq!(chat_requests(&upstream).await, 0);
    }
}

#[tokio::test]
async fn in_monitor_mode_typed_text_and_uploads_are_recorded_only() {
    use aiplane_core::server::classification::ClassificationMode::Monitor;
    for (message, file) in [
        (format!("pay {IBAN}"), None),
        ("read this".to_owned(), Some(("iban.txt", "text/plain"))),
    ] {
        let (upstream, state, session, (status, _)) =
            send_to_low_model("block", Monitor, &message, file).await;
        assert_eq!(status, chat::TurnStatus::Completed);
        assert!(events(&state, &session).await.is_empty());
        assert_eq!(chat_requests(&upstream).await, 1);
    }
}

/// A granted tool whose call raises the conversation as a retrieval does.
struct Retrieval;

impl aiplane_runtime::server::tools::Tool for Retrieval {
    fn id(&self) -> &str {
        "rag_search"
    }
    fn schema(&self) -> shared::api::ToolDef {
        probe_schema("rag_search")
    }
    fn egress(&self) -> bool {
        false
    }
    fn run<'a>(
        &'a self,
        ctx: ToolContext,
        _args: serde_json::Value,
    ) -> aiplane_runtime::server::tools::ToolFuture<'a> {
        Box::pin(async move {
            aiplane_runtime::classification_retrieval::raise_for_collection(&ctx, 7)
                .await
                .map_err(aiplane_runtime::server::tools::ToolError::Failed)?;
            Ok(serde_json::json!({"hits": ["a confidential passage"]}))
        })
    }
}

/// Round one asks for `rag_search`; every later round answers.
struct ToolThenAnswer(std::sync::atomic::AtomicUsize);

impl wiremock::Respond for ToolThenAnswer {
    fn respond(&self, request: &wiremock::Request) -> ResponseTemplate {
        if String::from_utf8_lossy(&request.body).contains("You name chat conversations") {
            return ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"role": "assistant", "content": "Title"}}]
            }));
        }
        let round = self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let frames = if round == 0 {
            vec![
                serde_json::json!({"choices": [{"index": 0, "delta": {"role": "assistant", "tool_calls": [{"index": 0, "id": "call-0", "type": "function", "function": {"name": "rag_search", "arguments": "{}"}}]}, "finish_reason": null}]}),
                serde_json::json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}]}),
            ]
        } else {
            vec![
                serde_json::json!({"choices": [{"index": 0, "delta": {"content": "answer"}, "finish_reason": "stop"}]}),
            ]
        };
        let mut body = String::new();
        for frame in frames {
            body.push_str(&format!("data: {frame}\n\n"));
        }
        body.push_str("data: [DONE]\n\n");
        ResponseTemplate::new(200).set_body_raw(body, "text/event-stream")
    }
}

const TOOLS_SQL: &str = "
    INSERT INTO classification_assignments (kind, id, level_id, over) VALUES
        ('collection', '7', 'confidential', 'hide');";

/// A person with granted tools, all enabled for the conversation.
async fn chat_with_tools(upstream: &MockServer) -> (Arc<RamaState>, String, String) {
    use aiplane_core::server::rbac::Resolver;
    use aiplane_core::server::rbac::config::{RbacConfig, RoleConfig, RoleMapping};
    use aiplane_runtime::server::tools::{ToolRegistry, time::CurrentTimestamp};
    let rbac = Resolver::build(
        RbacConfig {
            default_role: None,
            mappings: vec![RoleMapping {
                oidc_value: "engineering".into(),
                role: "engineer".into(),
            }],
        },
        vec![RoleConfig {
            id: "engineer".into(),
            admin: false,
            tools: vec![
                "fetch_url".into(),
                "get_current_timestamp".into(),
                "rag_search".into(),
            ],
            skills: vec![],
        }],
    )
    .unwrap();
    let tools = ToolRegistry::new()
        .with(Outbound("fetch_url"))
        .with(CurrentTimestamp)
        .with(Retrieval);
    let s3 = object_store().await;
    let state = state_full(
        &upstream.uri(),
        &s3.uri(),
        true,
        TOOLS_SQL,
        |_| {},
        Arc::new(tools),
        Arc::new(rbac),
    )
    .await;
    let now = jiff::Timestamp::now();
    aiplane_core::server::db::users::upsert(
        &state.db,
        &aiplane_core::server::db::users::User {
            id: "alice".into(),
            email: "alice@example.com".into(),
            name: None,
            roles: vec!["engineering".into()],
            created_at: now,
            updated_at: now,
            timezone: None,
            speech_voice: None,
        },
    )
    .await
    .unwrap();
    let cookie = state
        .sessions
        .sign(&state.sessions.create("alice").await.unwrap().id);
    let session = chat::create_session(&state.db, "alice").await.unwrap();
    for key in ["fetch_url", "get_current_timestamp", "rag_search"] {
        aiplane_core::server::db::chat_session_tools::set(
            &state.db,
            &session.id,
            key,
            true,
            "manual",
        )
        .await
        .unwrap();
    }
    (Arc::new(state), cookie, session.id)
}

fn offered_tools(request: &wiremock::Request) -> Vec<String> {
    let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
    body["tools"]
        .as_array()
        .map(|tools| {
            tools
                .iter()
                .filter_map(|tool| tool["function"]["name"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

async fn turn_requests(upstream: &MockServer) -> Vec<wiremock::Request> {
    upstream
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|request| request.url.path() == "/chat/completions")
        .filter(|request| {
            !String::from_utf8_lossy(&request.body).contains("You name chat conversations")
        })
        .collect()
}

#[tokio::test]
async fn an_egress_tool_above_its_level_is_not_offered_but_an_internal_tool_is() {
    for level in [None, Some("confidential")] {
        let upstream_for_level = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\ndata: [DONE]\n\n",
                "text/event-stream",
            ))
            .mount(&upstream_for_level)
            .await;
        let (state, cookie, session) = chat_with_tools(&upstream_for_level).await;
        if let Some(level) = level {
            classification::set_level(&state.db, &session, level)
                .await
                .unwrap();
        }
        let resp = router(state.clone())
            .serve(submit(&session, &cookie, "high-model", None))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::ACCEPTED);
        wait_settled(&state, &session).await;
        let requests = turn_requests(&upstream_for_level).await;
        let offered = offered_tools(&requests[0]);
        assert!(
            offered.contains(&"get_current_timestamp".to_owned()),
            "{offered:?}"
        );
        assert_eq!(
            offered.contains(&"fetch_url".to_owned()),
            level.is_none(),
            "level {level:?}: {offered:?}"
        );
    }
}

#[tokio::test]
async fn a_level_raised_by_a_retrieval_ends_the_turn_before_the_next_round_reaches_an_uncleared_model()
 {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ToolThenAnswer(Default::default()))
        .mount(&upstream)
        .await;
    let (state, cookie, session) = chat_with_tools(&upstream).await;
    let resp = router(state.clone())
        .serve(submit(&session, &cookie, "low-model", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    wait_settled(&state, &session).await;

    let (status, error) = last_turn_status(&state, &session).await;
    assert_eq!(status, chat::TurnStatus::Errored);
    assert!(error.contains("Confidential"), "{error}");
    assert_eq!(
        turn_requests(&upstream).await.len(),
        1,
        "round two never left"
    );
    assert_eq!(
        events(&state, &session)
            .await
            .last()
            .map(|event| event.2.clone()),
        Some("retrieval".to_owned())
    );
}

async fn title_requests(upstream: &MockServer) -> usize {
    upstream
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|request| {
            String::from_utf8_lossy(&request.body).contains("You name chat conversations")
        })
        .count()
}

#[tokio::test]
async fn a_conversation_raised_by_its_first_upload_gets_no_title_from_an_uncleared_model() {
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
            "low-model",
            Some(("mip_confidential.docx", "application/octet-stream")),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    wait_settled(&state, &session.id).await;
    tokio::time::sleep(Duration::from_millis(600)).await;

    assert_eq!(title_requests(&upstream).await, 0);
}

#[tokio::test]
async fn an_unraised_conversation_is_still_titled_once_its_turn_has_settled() {
    let upstream = chat_upstream().await;
    let s3 = object_store().await;
    let state = state(&upstream.uri(), &s3.uri(), true).await;
    let cookie = common::seed_session(&state, "alice", "alice@example.com").await;
    let state = Arc::new(state);
    let session = chat::create_session(&state.db, "alice").await.unwrap();

    let resp = router(state.clone())
        .serve(submit(&session.id, &cookie, "low-model", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    wait_settled(&state, &session.id).await;
    for _ in 0..50 {
        if title_requests(&upstream).await == 1 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("no title request reached the cleared model");
}

#[tokio::test]
async fn every_model_is_cleared_for_a_conversation_at_the_lowest_level() {
    let upstream = chat_upstream().await;
    let s3 = object_store().await;
    let state = state(&upstream.uri(), &s3.uri(), true).await;
    let access = aiplane_core::server::upstreams::PoolAccess::all();
    common::seed_session(&state, "alice", "alice@example.com").await;
    let session = chat::create_session(&state.db, "alice").await.unwrap();

    // A model the registry does not list yet (its backend still probing at
    // boot) or reaches through a route: nothing to protect at the lowest level.
    assert!(
        state
            .model_cleared_for_session(&session.id, "not-listed-model", &access)
            .await
    );

    classification::set_level(&state.db, &session.id, "internal")
        .await
        .unwrap();
    assert!(
        !state
            .model_cleared_for_session(&session.id, "not-listed-model", &access)
            .await
    );
}
