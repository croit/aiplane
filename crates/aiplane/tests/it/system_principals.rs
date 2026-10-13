// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! System principals and `gws_` tokens, end to end
//! (`docs/agents.md` → "Principals").
//!
//! The fixture is deliberately generous to *people*: a default group grants
//! tools, skills and both MCP connectors to everyone, a global connector is
//! open to all, and two users hold their own OAuth connections. Every test
//! then checks that none of that reaches a system principal unless it was
//! granted one resource at a time, by someone who held it.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use rama::Service;
use rama::http::{Body, Method, Request, StatusCode};
use serde_json::{Value, json};
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::common::{self, TEST_SECRET};

use aiplane::rama_server::{RamaState, SessionStore};
use aiplane_core::server::db::{self, gateway_groups, mcp_catalog, skill_grants, users};
use aiplane_core::server::principal::{GrantKind, Principal};
use aiplane_core::server::rbac::Resolver;
use aiplane_core::server::upstreams::{
    self,
    config::{PickerStrategy, PoolKind, UpstreamPoolConfig},
};
use aiplane_features::server::skills::{Skill, SkillRegistry, SkillStore, UserSkillStore};
use aiplane_runtime::server::AppState;
use aiplane_runtime::server::tools::ToolRegistry;
use aiplane_runtime::server::tools::echo::Echo;
use aiplane_runtime::server::tools::mcp::manager::AskContext;
use aiplane_runtime::server::tools::time::CurrentTimestamp;
use aiplane_tools::read_skill::ReadSkill;

const TIME: &str = "get_current_timestamp";
const GLOBAL: &str = "globaltools";
const PER_USER: &str = "privdrive";

struct Fixture {
    state: RamaState,
    upstream: MockServer,
    /// The `vip` pool's own upstream, so a test sees whether a call landed there.
    vip_upstream: MockServer,
    _mcp: MockServer,
    /// MCP calls that carried a person's OAuth credential.
    personal_mcp_hits: Arc<AtomicUsize>,
    admin: String,
    manager: String,
    /// A second manager with the same rights as `manager`.
    other_manager: String,
    plain: String,
}

impl Fixture {
    fn app(
        &self,
    ) -> impl Service<Request, Output = rama::http::Response, Error = std::convert::Infallible>
    {
        common::app(self.state.clone())
    }

    async fn send(&self, req: Request) -> (StatusCode, Value) {
        let resp = self.app().serve(req).await.unwrap();
        let status = resp.status();
        let bytes = common::read_body(resp).await;
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn post(&self, cookie: &str, uri: &str, body: Value) -> (StatusCode, Value) {
        self.send(common::post_json(uri, cookie, &body.to_string()))
            .await
    }

    async fn get(&self, cookie: &str, uri: &str) -> (StatusCode, Value) {
        self.send(
            Request::builder()
                .method(Method::GET)
                .uri(uri)
                .header("cookie", format!("id={cookie}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
    }

    async fn create(&self, cookie: &str, name: &str) -> String {
        let (status, body) = self
            .post(cookie, "/api/v0/system-principals", json!({ "name": name }))
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        body["principal"]["id"].as_str().unwrap().to_string()
    }

    async fn grant(&self, cookie: &str, id: &str, kind: &str, r: &str) -> (StatusCode, Value) {
        self.post(
            cookie,
            &format!("/api/v0/system-principals/{id}/grants"),
            json!({ "kind": kind, "ref": r }),
        )
        .await
    }

    async fn token(&self, cookie: &str, id: &str) -> (String, String) {
        let (status, body) = self
            .post(
                cookie,
                &format!("/api/v0/system-principals/{id}/tokens"),
                json!({ "name": "pipeline" }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        (
            body["plaintext"].as_str().unwrap().to_string(),
            body["token"]["id"].as_str().unwrap().to_string(),
        )
    }

    /// A principal `manager` created that may call `model-a` and nothing
    /// else.
    async fn principal_with_model(&self, name: &str) -> (String, String) {
        let id = self.create(&self.manager, name).await;
        let (status, body) = self.grant(&self.manager, &id, "model", "model-a").await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        let (bearer, _) = self.token(&self.manager, &id).await;
        (id, bearer)
    }

    async fn chat(&self, bearer: &str) -> StatusCode {
        let req = Request::builder()
            .method(Method::POST)
            .uri("/v1/chat/completions")
            .header("authorization", format!("Bearer {bearer}"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"model": "model-a", "messages": [{"role": "user", "content": "hi"}]})
                    .to_string(),
            ))
            .unwrap();
        let resp = self.app().serve(req).await.unwrap();
        let status = resp.status();
        let _ = common::read_body(resp).await;
        status
    }

    async fn models_status(&self, bearer: &str) -> StatusCode {
        let req = Request::builder()
            .method(Method::GET)
            .uri("/v1/models")
            .header("authorization", format!("Bearer {bearer}"))
            .body(Body::empty())
            .unwrap();
        self.app().serve(req).await.unwrap().status()
    }

    /// The body of the last request the chat upstream received.
    async fn last_upstream_body(&self) -> Value {
        let requests = self.upstream.received_requests().await.unwrap();
        let last = requests.last().expect("the upstream was called");
        serde_json::from_slice(&last.body).unwrap()
    }

    async fn offered_tools(&self) -> Vec<String> {
        let body = self.last_upstream_body().await;
        body["tools"]
            .as_array()
            .map(|tools| {
                tools
                    .iter()
                    .map(|t| t["function"]["name"].as_str().unwrap().to_string())
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// An MCP server whose tool list depends on the credential: no credential is
/// the global connector, `alice-secret` / `bob-secret` are two people's own
/// OAuth connections.
async fn mcp_server(personal_hits: Arc<AtomicUsize>) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(move |request: &wiremock::Request| {
            let bearer = request
                .headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            let tool = match bearer {
                "" => "shared_lookup",
                "Bearer alice-secret" => "alice_files",
                "Bearer bob-secret" => "bob_files",
                "Bearer erp-secret" => "erp_lookup",
                other => panic!("unexpected MCP credential: {other}"),
            };
            if bearer.contains("alice") || bearer.contains("bob") {
                personal_hits.fetch_add(1, Ordering::SeqCst);
            }
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            let result = match body["method"].as_str() {
                Some("initialize") => json!({
                    "protocolVersion": "2025-06-18",
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "fixture", "version": "1"},
                }),
                Some("tools/list") => json!({
                    "tools": [{"name": tool, "description": tool,
                        "inputSchema": {"type": "object"},
                        "annotations": {"readOnlyHint": true}}]
                }),
                Some("notifications/initialized") => return ResponseTemplate::new(202),
                other => panic!("unexpected MCP method: {other:?}"),
            };
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/json")
                .set_body_json(json!({"jsonrpc": "2.0", "id": body["id"], "result": result}))
        })
        .mount(&server)
        .await;
    server
}

fn chat_pool(url: &str, allowed_groups: Vec<String>) -> UpstreamPoolConfig {
    UpstreamPoolConfig {
        voices: Default::default(),
        offer_voices: Vec::new(),
        allowed_groups,
        fallback_offline: None,
        compliance: Default::default(),
        enforce_limits: true,
        kind: PoolKind::Chat,
        strategy: PickerStrategy::RoundRobin,
        models: Vec::new(),
        backend: vec![common::mock_backend("mock", url)],
    }
}

fn connector(key: &str, url: &str, scope: mcp_catalog::Scope) -> mcp_catalog::ConnectorInput {
    mcp_catalog::ConnectorInput {
        key: key.into(),
        name: key.into(),
        description: None,
        icon: None,
        category: None,
        url: url.into(),
        auth: match scope {
            mcp_catalog::Scope::Global | mcp_catalog::Scope::Agent => mcp_catalog::AuthKind::None,
            mcp_catalog::Scope::PerUser => mcp_catalog::AuthKind::OAuth2,
        },
        scope,
        audit: false,
        use_dcr: false,
        client_id: None,
        client_secret_ct: None,
        client_secret_nonce: None,
        authorize_url: None,
        token_url: None,
        registration_url: None,
        scopes: vec![],
        allowed_groups: vec![],
    }
}

async fn person(state: &RamaState, id: &str, roles: &[&str]) -> String {
    let now = jiff::Timestamp::now();
    users::upsert(
        &state.db,
        &users::User {
            id: id.into(),
            email: format!("{id}@example.com"),
            name: None,
            roles: roles.iter().map(|r| r.to_string()).collect(),
            created_at: now,
            updated_at: now,
            timezone: None,
            speech_voice: None,
        },
    )
    .await
    .unwrap();
    let session = state.sessions.create(id).await.unwrap();
    state.sessions.sign(&session.id)
}

async fn fixture() -> Fixture {
    fixture_with_usage(false).await
}

async fn fixture_with_usage(metered: bool) -> Fixture {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{"message": {"role": "assistant", "content": "ok"}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 3, "completion_tokens": 2, "total_tokens": 5}
        })))
        .mount(&upstream)
        .await;
    Mock::given(method("POST"))
        .and(path("/embeddings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": "list",
            "data": [{"object": "embedding", "index": 0, "embedding": [0.1, 0.2]}],
            "model": "embed-1",
            "usage": {"prompt_tokens": 1, "total_tokens": 1}
        })))
        .mount(&upstream)
        .await;
    Mock::given(method("POST"))
        .and(path("/rerank"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{"index": 0, "relevance_score": 0.5}],
            "usage": {"total_tokens": 2}
        })))
        .mount(&upstream)
        .await;
    Mock::given(method("POST"))
        .and(path("/images/generations"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "created": 1,
            "data": [{"b64_json": "aGk="}]
        })))
        .mount(&upstream)
        .await;
    let vip_upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{"message": {"role": "assistant", "content": "vip"}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 3, "completion_tokens": 2, "total_tokens": 5}
        })))
        .mount(&vip_upstream)
        .await;
    let personal_mcp_hits = Arc::new(AtomicUsize::new(0));
    let mcp = mcp_server(personal_mcp_hits.clone()).await;

    let pool = db::open(std::path::Path::new(":memory:")).await.unwrap();
    let mut pools = HashMap::new();
    let mut open = chat_pool(&upstream.uri(), vec![]);
    open.backend[0].alias = Some(upstreams::config::AliasSpec::Targets(HashMap::from([(
        "fast".to_string(),
        "model-a".to_string(),
    )])));
    pools.insert("pool".to_string(), open);
    pools.insert(
        "vip".to_string(),
        chat_pool(&vip_upstream.uri(), vec!["vipgroup".into()]),
    );
    pools.insert(
        "selector".to_string(),
        UpstreamPoolConfig {
            kind: PoolKind::SystemOne,
            ..chat_pool(&upstream.uri(), vec![])
        },
    );
    // Not probed yet: it serves nothing until a test seeds it.
    pools.insert("late".to_string(), chat_pool(&upstream.uri(), vec![]));
    for (name, kind) in [
        ("embed", PoolKind::Embedding),
        ("image", PoolKind::Image),
        ("rerank", PoolKind::Rerank),
    ] {
        pools.insert(
            name.to_string(),
            UpstreamPoolConfig {
                kind,
                ..chat_pool(&upstream.uri(), vec![])
            },
        );
    }
    let registry = upstreams::UpstreamRegistry::new(&pools).unwrap();
    common::seed_pool_models(&registry, "embed", 0, &["embed-1"]);
    common::seed_pool_models(&registry, "image", 0, &["img-1"]);
    common::seed_pool_models(&registry, "rerank", 0, &["rerank-1"]);
    common::seed_pool_models(&registry, "pool", 0, &["model-a", "model-b"]);
    common::seed_pool_models(&registry, "vip", 0, &["model-vip", "model-a"]);
    common::seed_pool_models(&registry, "selector", 0, &["picker"]);

    let rbac = Arc::new(Resolver::empty());
    let skills = Arc::new(SkillStore::with_registry(
        std::path::PathBuf::from("/nonexistent"),
        SkillRegistry::new([Skill {
            name: "brand".into(),
            title: "Brand".into(),
            description: "Enforce the brand.".into(),
            root: std::path::PathBuf::from("/nonexistent"),
        }]),
    ));
    let user_skills = Arc::new(UserSkillStore::new(std::path::PathBuf::from(
        "/nonexistent/.users",
    )));
    let tools = ToolRegistry::new()
        .with(CurrentTimestamp)
        .with(Echo)
        .with(ReadSkill::new(
            skills.clone(),
            user_skills.clone(),
            rbac.clone(),
        ));
    let app = AppState::new(
        common::test_config(),
        pool.clone(),
        registry,
        Arc::new(tools),
        rbac,
    )
    .with_skills(skills)
    .with_user_skills(user_skills);
    let usage = if metered {
        aiplane_core::server::usage::spawn(pool.clone(), 90)
    } else {
        aiplane_core::server::usage::UsageHandle::disabled()
    };
    let state = RamaState::new(app, SessionStore::new(pool.clone(), TEST_SECRET), usage);

    // Everyone: the time tool, the skill loader, both connectors and every
    // skill. Admins: everything. Managers: agent management, nothing more.
    gateway_groups::upsert_group(&pool, "everyone", "", false, true)
        .await
        .unwrap();
    gateway_groups::set_tools_for_group(
        &pool,
        "everyone",
        &[
            TIME.into(),
            "read_skill".into(),
            format!("mcp__{GLOBAL}"),
            format!("mcp__{PER_USER}"),
        ],
    )
    .await
    .unwrap();
    skill_grants::set_skills_for_role(&pool, "everyone", &["*".into()])
        .await
        .unwrap();
    gateway_groups::upsert_group(&pool, "admins", "", true, false)
        .await
        .unwrap();
    gateway_groups::set_mappings_for_group(&pool, "admins", &["admins".into()])
        .await
        .unwrap();
    gateway_groups::upsert_group(&pool, "managers", "", false, false)
        .await
        .unwrap();
    gateway_groups::set_can_manage_agents(&pool, "managers", true)
        .await
        .unwrap();
    gateway_groups::set_mappings_for_group(&pool, "managers", &["managers".into()])
        .await
        .unwrap();
    state.reload_rbac().await;

    mcp_catalog::create(
        &pool,
        connector(GLOBAL, &mcp.uri(), mcp_catalog::Scope::Global),
    )
    .await
    .unwrap();
    mcp_catalog::create(
        &pool,
        connector(PER_USER, &mcp.uri(), mcp_catalog::Scope::PerUser),
    )
    .await
    .unwrap();

    for key in [GLOBAL, PER_USER] {
        mcp_catalog::set_enabled(&pool, key, true).await.unwrap();
    }

    let admin = person(&state, "admin", &["admins"]).await;
    let manager = person(&state, "manager", &["managers"]).await;
    let other_manager = person(&state, "other-manager", &["managers"]).await;
    let plain = person(&state, "plain", &[]).await;
    for (user, secret) in [("alice", "alice-secret"), ("bob", "bob-secret")] {
        person(&state, user, &[]).await;
        let sealed = state.crypto.seal_str(secret).unwrap();
        aiplane_core::server::db::mcp_connections::upsert_connection(
            &pool,
            aiplane_core::server::db::mcp_connections::NewConnection {
                owner: aiplane_core::server::db::mcp_connections::ConnectionOwner::User(
                    user.into(),
                ),
                connector_key: PER_USER.into(),
                catalog_generation: mcp_catalog::get(&pool, PER_USER)
                    .await
                    .unwrap()
                    .unwrap()
                    .dispatch_generation,
                access_token_ct: sealed.ciphertext,
                access_token_nonce: sealed.nonce,
                refresh_token_ct: None,
                refresh_token_nonce: None,
                token_expires_at: None,
                scopes: vec![],
                dcr_client_id: None,
                dcr_client_secret_ct: None,
                dcr_client_secret_nonce: None,
                token_url: None,
            },
        )
        .await
        .unwrap()
        .unwrap();
    }

    Fixture {
        state,
        upstream,
        vip_upstream,
        _mcp: mcp,
        personal_mcp_hits,
        admin,
        manager,
        other_manager,
        plain,
    }
}

fn person_principal(id: &str) -> Principal {
    Principal::User {
        id: id.into(),
        roles: vec![],
    }
}

/// The control: on this exact configuration a person *is* offered tools,
/// skills, the global connector and their own connection. Without it the
/// default-deny tests below could pass on an empty gateway.
#[tokio::test]
async fn the_fixture_offers_people_tools_skills_and_connectors() {
    let fx = fixture().await;
    let tools = fx.state.allowed_tools_for_user(&[], "alice").await;
    assert!(tools.contains(&TIME.to_string()), "{tools:?}");
    assert_eq!(fx.state.allowed_skills_for(&[], "alice"), vec!["brand"]);
    let mut mcp = fx
        .state
        .mcp_layer_for(&person_principal("alice"), AskContext::Chat)
        .await
        .tool_ids();
    mcp.sort();
    assert_eq!(
        mcp,
        [
            format!("mcp__{GLOBAL}__shared_lookup"),
            format!("mcp__{PER_USER}__alice_files"),
        ]
    );
}

#[tokio::test]
async fn a_fresh_principal_is_offered_no_tools_connectors_or_skills() {
    let fx = fixture().await;
    let (_, bearer) = fx.principal_with_model("ci").await;
    let sent = json!([{"role": "user", "content": "hi"}]);

    assert_eq!(fx.chat(&bearer).await, StatusCode::OK);
    let body = fx.last_upstream_body().await;
    assert!(body.get("tools").is_none(), "tools were offered: {body}");
    assert_eq!(
        body["messages"], sent,
        "the request context gained something: {body}"
    );
    assert_eq!(fx.personal_mcp_hits.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_principal_without_a_model_grant_cannot_reach_an_open_pool() {
    let fx = fixture().await;
    let id = fx.create(&fx.admin, "ci").await;
    let (bearer, _) = fx.token(&fx.admin, &id).await;
    assert_eq!(fx.chat(&bearer).await, StatusCode::NOT_FOUND);
    assert!(fx.upstream.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn after_granting_one_tool_exactly_that_tool_is_offered() {
    let fx = fixture().await;
    let (id, bearer) = fx.principal_with_model("ci").await;
    let (status, body) = fx.grant(&fx.manager, &id, "tool", TIME).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    assert_eq!(fx.chat(&bearer).await, StatusCode::OK);
    assert_eq!(fx.offered_tools().await, [TIME]);
}

#[tokio::test]
async fn a_granted_global_connector_never_exposes_a_persons_connection() {
    let fx = fixture().await;
    let (id, bearer) = fx.principal_with_model("ci").await;
    let (status, body) = fx.grant(&fx.admin, &id, "connector", GLOBAL).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    assert_eq!(fx.chat(&bearer).await, StatusCode::OK);
    assert_eq!(
        fx.offered_tools().await,
        [format!("mcp__{GLOBAL}__shared_lookup")]
    );
    assert_eq!(
        fx.personal_mcp_hits.load(Ordering::SeqCst),
        0,
        "a person's OAuth credential was used for a system principal"
    );
}

#[tokio::test]
async fn a_per_user_connector_cannot_be_granted_and_is_ignored_if_present() {
    let fx = fixture().await;
    let id = fx.create(&fx.admin, "ci").await;
    let (status, body) = fx.grant(&fx.admin, &id, "connector", PER_USER).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains(PER_USER),
        "{body}"
    );

    // Even a grant row that got in some other way reads nobody's connection.
    aiplane_agents::db::system_principals::add_grant(
        &fx.state.db,
        &id,
        GrantKind::Connector,
        PER_USER,
        "x",
    )
    .await
    .unwrap();
    let principal = aiplane_agents::db::system_principals::load_active(&fx.state.db, &id)
        .await
        .unwrap()
        .unwrap();
    let layer = fx
        .state
        .mcp_layer_for(&Principal::System(principal), AskContext::Chat)
        .await;
    assert!(layer.is_empty(), "{:?}", layer.tool_ids());
    assert_eq!(fx.personal_mcp_hits.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_grant_beyond_the_managers_own_rights_is_refused_naming_the_resource() {
    let fx = fixture().await;
    let id = fx.create(&fx.manager, "ci").await;
    for (kind, reference) in [("tool", "company_echo"), ("model", "model-vip")] {
        let (status, body) = fx.grant(&fx.manager, &id, kind, reference).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{kind} {reference}: {body}");
        assert_eq!(body["error"]["code"], "grant_exceeds_manager");
        let message = body["error"]["message"].as_str().unwrap();
        assert!(message.contains(&format!("`{reference}`")), "{message}");
        assert!(message.contains("Ask an admin"), "{message}");
    }
    let (status, _) = fx.grant(&fx.manager, &id, "tool", "no_such_tool").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = fx.grant(&fx.manager, &id, "tool", "*").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // What the manager does hold goes through.
    let (status, _) = fx.grant(&fx.manager, &id, "skill", "brand").await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = fx.grant(&fx.manager, &id, "model", "model-a").await;
    assert_eq!(status, StatusCode::CREATED);
    let (_, detail) = fx
        .get(&fx.manager, &format!("/api/v0/system-principals/{id}"))
        .await;
    assert_eq!(detail["principal"]["grants"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn a_person_without_agent_management_cannot_create_or_configure() {
    let fx = fixture().await;
    let id = fx.create(&fx.admin, "ci").await;
    let (status, _) = fx
        .post(
            &fx.plain,
            "/api/v0/system-principals",
            json!({"name": "mine"}),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, body) = fx.grant(&fx.plain, &id, "model", "model-a").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("enable agent management"),
        "{body}"
    );
    for uri in [
        "/api/v0/system-principals".to_string(),
        format!("/api/v0/system-principals/{id}"),
    ] {
        assert_eq!(fx.get(&fx.plain, &uri).await.0, StatusCode::FORBIDDEN);
    }
    for (uri, body) in [
        (
            format!("/api/v0/system-principals/{id}/tokens"),
            json!({"name": "x"}),
        ),
        (format!("/api/v0/system-principals/{id}/disable"), json!({})),
        (
            format!("/api/v0/system-principals/{id}/grants/revoke"),
            json!({"kind": "model", "ref": "model-a"}),
        ),
    ] {
        assert_eq!(
            fx.post(&fx.plain, &uri, body).await.0,
            StatusCode::FORBIDDEN
        );
    }
    let (_, detail) = fx
        .get(&fx.admin, &format!("/api/v0/system-principals/{id}"))
        .await;
    assert!(detail["principal"]["grants"].as_array().unwrap().is_empty());
    assert!(detail["principal"]["tokens"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn every_route_needs_a_session() {
    let fx = fixture().await;
    let routes = [
        (Method::GET, "/api/v0/system-principals"),
        (Method::POST, "/api/v0/system-principals"),
        (Method::GET, "/api/v0/system-principals/p"),
        (Method::POST, "/api/v0/system-principals/p/disable"),
        (Method::POST, "/api/v0/system-principals/p/grants"),
        (Method::POST, "/api/v0/system-principals/p/grants/revoke"),
        (Method::POST, "/api/v0/system-principals/p/tokens"),
        (Method::POST, "/api/v0/system-principals/p/tokens/t/revoke"),
    ];
    for (verb, uri) in routes {
        let (status, _) = fx.send(common::req(verb.clone(), uri)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{verb} {uri}");
    }
}

/// The principal keeps its grants when the manager who made them loses their
/// rights or leaves; a token that manager minted does not, because it is
/// capped at what its minter holds now. An admin's token is not.
#[tokio::test]
async fn grants_survive_the_granting_manager_losing_rights_and_leaving() {
    let fx = fixture().await;
    let id = fx.create(&fx.manager, "ci").await;
    assert_eq!(
        fx.grant(&fx.manager, &id, "model", "model-a").await.0,
        StatusCode::CREATED
    );
    assert_eq!(
        fx.grant(&fx.manager, &id, "tool", TIME).await.0,
        StatusCode::CREATED
    );
    let (managers, _) = fx.token(&fx.manager, &id).await;
    let (bearer, _) = fx.token(&fx.admin, &id).await;

    gateway_groups::set_can_manage_agents(&fx.state.db, "managers", false)
        .await
        .unwrap();
    gateway_groups::set_tools_for_group(&fx.state.db, "everyone", &[])
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE id = 'manager'")
        .execute(&fx.state.db)
        .await
        .unwrap();
    fx.state.reload_rbac().await;

    assert_eq!(fx.chat(&bearer).await, StatusCode::OK);
    assert_eq!(fx.offered_tools().await, [TIME]);
    assert_ne!(
        fx.chat(&managers).await,
        StatusCode::OK,
        "a token of a manager who left reaches nothing"
    );
}

/// A token a manager minted is capped at authentication time at what the
/// manager holds then: a grant an admin adds later that the manager lacks
/// is not usable through it, but is through a token an admin minted.
#[tokio::test]
async fn a_managers_token_is_capped_at_the_managers_current_rights() {
    let fx = fixture().await;
    let id = fx.create(&fx.manager, "ci").await;
    assert_eq!(
        fx.grant(&fx.manager, &id, "model", "model-a").await.0,
        StatusCode::CREATED
    );
    let (managers, _) = fx.token(&fx.manager, &id).await;
    let (admins, _) = fx.token(&fx.admin, &id).await;
    assert_eq!(
        fx.grant(&fx.admin, &id, "tool", TIME).await.0,
        StatusCode::CREATED
    );
    gateway_groups::set_tools_for_group(&fx.state.db, "everyone", &["read_skill".into()])
        .await
        .unwrap();
    fx.state.reload_rbac().await;

    assert_eq!(fx.chat(&managers).await, StatusCode::OK);
    assert!(
        !fx.offered_tools().await.contains(&TIME.to_string()),
        "the manager does not hold the tool, so their token does not carry it"
    );
    assert_eq!(fx.chat(&admins).await, StatusCode::OK);
    assert_eq!(fx.offered_tools().await, [TIME]);
}

/// The cap is reused between requests, but never past a change: a revoked
/// grant is gone from the next request, and so is a right the minter lost.
#[tokio::test]
async fn a_reused_cap_never_outlives_a_revoked_grant_or_a_minters_lost_right() {
    let fx = fixture().await;
    let id = fx.create(&fx.manager, "ci").await;
    fx.grant(&fx.manager, &id, "model", "model-a").await;
    fx.grant(&fx.manager, &id, "tool", TIME).await;
    let (managers, _) = fx.token(&fx.manager, &id).await;
    assert_eq!(fx.chat(&managers).await, StatusCode::OK);
    assert_eq!(fx.offered_tools().await, [TIME], "the cap is now warm");

    let (status, _) = fx
        .post(
            &fx.admin,
            &format!("/api/v0/system-principals/{id}/grants/revoke"),
            json!({"kind": "tool", "ref": TIME}),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(fx.chat(&managers).await, StatusCode::OK);
    assert!(
        fx.offered_tools().await.is_empty(),
        "a revoked grant is not served from the cached cap"
    );

    fx.grant(&fx.admin, &id, "tool", TIME).await;
    assert_eq!(fx.chat(&managers).await, StatusCode::OK);
    assert_eq!(fx.offered_tools().await, [TIME]);
    gateway_groups::set_tools_for_group(&fx.state.db, "everyone", &[])
        .await
        .unwrap();
    fx.state.reload_rbac().await;
    assert_eq!(fx.chat(&managers).await, StatusCode::OK);
    assert!(
        fx.offered_tools().await.is_empty(),
        "the minter lost the tool, and the reload let go of the cap"
    );
}

#[tokio::test]
async fn every_grant_change_is_audited_with_who_what_and_when() {
    let fx = fixture().await;
    let id = fx.create(&fx.manager, "ci").await;
    fx.grant(&fx.manager, &id, "tool", TIME).await;
    let (status, _) = fx
        .post(
            &fx.admin,
            &format!("/api/v0/system-principals/{id}/grants/revoke"),
            json!({"kind": "tool", "ref": TIME}),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = fx
        .post(
            &fx.admin,
            &format!("/api/v0/system-principals/{id}/grants/revoke"),
            json!({"kind": "tool", "ref": TIME}),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (_, detail) = fx
        .get(&fx.admin, &format!("/api/v0/system-principals/{id}"))
        .await;
    let audit: Vec<(String, String, Value)> = detail["principal"]["audit"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .map(|e| {
            assert!(e["created_at"].is_string(), "{e}");
            (
                e["kind"].as_str().unwrap().to_string(),
                e["actor_id"].as_str().unwrap().to_string(),
                e["detail"].clone(),
            )
        })
        .collect();
    assert_eq!(
        audit,
        [
            (
                "principal_created".into(),
                "manager".into(),
                json!({"name": "ci"})
            ),
            (
                "grant_added".into(),
                "manager".into(),
                json!({"kind": "tool", "ref": TIME})
            ),
            (
                "grant_removed".into(),
                "admin".into(),
                json!({"kind": "tool", "ref": TIME})
            ),
        ]
    );
}

/// A headless run, as the scheduler or (later) an agent dispatch starts one.
async fn headless_run(
    fx: &Fixture,
    owner: aiplane_runtime::server::headless::Owner<'_>,
    actor: aiplane_runtime::agent_run::Actor,
) -> Value {
    use aiplane_runtime::server::headless::{self, DriveParams, OpenParams};
    let source = if actor.agent().is_some() {
        aiplane_core::server::db::usage::UsageSource::Agent
    } else {
        aiplane_core::server::db::usage::UsageSource::Chat
    };
    let before = fx.upstream.received_requests().await.unwrap().len();
    let (session_id, turn_id) = headless::open_session(
        &fx.state.db,
        OpenParams {
            owner,
            title: "run",
            prompt: "what do you know about me?",
            model: "model-a",
            existing_session: None,
            dot_id: None,
        },
    )
    .await
    .unwrap();
    headless::drive(
        &Arc::new(fx.state.clone()),
        DriveParams {
            actor,
            session_id: session_id.clone(),
            assistant_turn_id: turn_id.clone(),
            model: "model-a".into(),
            source,
            history_limit: None,
        },
    )
    .await;
    let turn = session_core::db::get_turn(&fx.state.db, &session_id, &turn_id)
        .await
        .unwrap();
    assert!(
        fx.upstream.received_requests().await.unwrap().len() > before,
        "headless upstream was not called; owner={owner:?}, persisted_turn={turn:?}"
    );
    fx.last_upstream_body().await
}

#[tokio::test]
async fn an_agent_run_gets_none_of_its_owners_connectors_memory_or_skills() {
    use aiplane_core::server::run_chain::{Frame, RunChain};
    use aiplane_runtime::server::headless::Owner;
    let fx = fixture().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"ok\"}}]}\n\ndata: [DONE]\n\n",
            "text/event-stream",
        ))
        .with_priority(1)
        .mount(&fx.upstream)
        .await;
    db::user_memories::insert(
        &fx.state.db,
        "alice",
        db::user_memories::MemoryKind::Preference,
        "always answer in pirate speak",
    )
    .await
    .unwrap();

    let persons = headless_run(
        &fx,
        Owner::User("alice"),
        aiplane_runtime::agent_run::Actor::person("alice", vec![]),
    )
    .await
    .to_string();
    assert!(persons.contains("alice@example.com"), "{persons}");
    assert!(persons.contains("brand"), "{persons}");
    let personal_hits_before = fx.personal_mcp_hits.load(Ordering::SeqCst);

    let (id, _) = fx.principal_with_model("support-website").await;
    let (status, body) = fx.grant(&fx.admin, &id, "tool", TIME).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let principal = aiplane_agents::db::system_principals::load_active(&fx.state.db, &id)
        .await
        .unwrap()
        .unwrap();
    let chain = Arc::new(RunChain::root(
        "s-visitor",
        None,
        Frame::for_principal(&principal, Some(1)),
    ));
    let agents = headless_run(
        &fx,
        Owner::Run {
            principal_id: &id,
            parent_turn_id: None,
            agent_version: Some(1),
        },
        aiplane_runtime::agent_run::Actor::Agent(Arc::new(
            aiplane_runtime::agent_run::AgentRun::new(principal, chain).unwrap(),
        )),
    )
    .await;

    assert_eq!(fx.offered_tools().await, [TIME]);
    let agents = agents.to_string();
    for owners in ["alice", "pirate", "brand", PER_USER] {
        assert!(
            !agents.contains(owners),
            "`{owners}` reached the agent run: {agents}"
        );
    }
    assert_eq!(
        fx.personal_mcp_hits.load(Ordering::SeqCst),
        personal_hits_before,
        "the owner's own MCP connection was used for the agent run"
    );
}

/// An agent run's system message comes from the same builder as a person's:
/// it carries the turn-discipline rule and lists the operator skills its
/// principal is granted, with the loader to read them. Its owner's identity
/// and preferences stay out — the builder is told this is an agent's turn.
#[tokio::test]
async fn an_agent_run_is_told_the_turn_rule_and_its_granted_skills() {
    use aiplane_core::server::run_chain::{Frame, RunChain};
    use aiplane_runtime::server::headless::Owner;
    let fx = fixture().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"ok\"}}]}\n\ndata: [DONE]\n\n",
            "text/event-stream",
        ))
        .with_priority(1)
        .mount(&fx.upstream)
        .await;
    db::user_memories::insert(
        &fx.state.db,
        "alice",
        db::user_memories::MemoryKind::Preference,
        "always answer in pirate speak",
    )
    .await
    .unwrap();
    let (id, _) = fx.principal_with_model("support-website").await;
    for (kind, reference) in [("skill", "brand"), ("tool", "read_skill")] {
        let (status, body) = fx.grant(&fx.admin, &id, kind, reference).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }
    let principal = aiplane_agents::db::system_principals::load_active(&fx.state.db, &id)
        .await
        .unwrap()
        .unwrap();
    let chain = Arc::new(RunChain::root(
        "s-visitor",
        None,
        Frame::for_principal(&principal, Some(1)),
    ));

    let sent = headless_run(
        &fx,
        Owner::Run {
            principal_id: &id,
            parent_turn_id: None,
            agent_version: Some(1),
        },
        aiplane_runtime::agent_run::Actor::Agent(Arc::new(
            aiplane_runtime::agent_run::AgentRun::new(principal, chain).unwrap(),
        )),
    )
    .await;

    let system = sent["messages"][0]["content"].as_str().unwrap();
    assert!(system.starts_with("How a turn works here:"), "{system}");
    assert!(system.contains("- brand: Enforce the brand."), "{system}");
    assert!(system.contains("read_skill(name)"), "{system}");
    assert!(fx.offered_tools().await.contains(&"read_skill".to_string()));
    for personal in ["alice", "pirate", "Automatically provided context"] {
        assert!(
            !system.contains(personal),
            "`{personal}` reached the agent: {system}"
        );
    }
}

/// The router is told the turn has tools from the very list the turn offers:
/// an agent whose spec lists no tools but whose principal holds the skill
/// loader and a skill is offered `read_skill`, so it has tools, and an
/// automatic route never hands it to a candidate that cannot call one. Here
/// the route's fallback cannot, so the turn has no model to run on rather
/// than sending `read_skill` to one that would ignore it.
#[tokio::test]
async fn an_agent_with_only_the_skill_loader_is_routed_as_one_with_tools() {
    use aiplane_agents::db::{agents as agents_db, system_principals as sp};
    use aiplane_runtime::agents::run::{AgentTurn, run_turn};
    let fx = fixture().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"ok\"}}]}\n\ndata: [DONE]\n\n",
            "text/event-stream",
        ))
        .with_priority(1)
        .mount(&fx.upstream)
        .await;
    upsert_route(&fx, "auto-skills", &["model-b", "model-a"]).await;
    db::model_defaults::set_capabilities(
        &fx.state.db,
        "model-b",
        &db::model_defaults::ModelCapabilities {
            tools: Some(false),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let id = agents_db::create(
        &fx.state.db,
        &sp::NewPrincipal {
            name: "brand-voice",
            display: "Brand voice",
            description: "",
        },
        "{}",
        "admin",
    )
    .await
    .unwrap()
    .unwrap()
    .principal
    .id;
    for (kind, reference) in [
        ("model", "auto-skills"),
        ("skill", "brand"),
        ("tool", "read_skill"),
    ] {
        let (status, body) = fx.grant(&fx.admin, &id, kind, reference).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }
    let spec = json!({ "main": {
        "model": "auto-skills",
        "instructions": { "orchestration": "Write in the brand voice." },
        "tools": [],
    } });
    agents_db::publish(&fx.state.db, &id, &spec.to_string(), "admin")
        .await
        .unwrap()
        .unwrap();

    let reply = run_turn(
        &Arc::new(fx.state.clone()),
        AgentTurn {
            agent_id: &id,
            session_id: None,
            message: "write it in our brand voice",
            visitor_id: None,
            lang: None,
        },
    )
    .await;

    let sent: Vec<Value> = fx
        .upstream
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter_map(|r| serde_json::from_slice(&r.body).ok())
        .collect();
    assert!(
        sent.iter().all(|body| body["model"] != "model-b"),
        "the tool-less candidate got the turn: {sent:?}"
    );
    let outcome = format!("{reply:?}");
    assert!(outcome.contains("not currently eligible"), "{outcome}");
}

#[tokio::test]
async fn the_audit_trail_shows_a_run_events_call_chain() {
    use aiplane_agents::db::agent_audit::{self, AuditKind};
    use aiplane_core::server::run_chain::{Frame, RunChain};
    let fx = fixture().await;
    let id = fx.create(&fx.admin, "support-website").await;
    let principal = aiplane_agents::db::system_principals::load_active(&fx.state.db, &id)
        .await
        .unwrap()
        .unwrap();
    let chain = RunChain::root(
        "s-visitor",
        Some("v-1".into()),
        Frame::for_principal(&principal, Some(3)),
    );
    agent_audit::append_now(
        &fx.state.db,
        agent_audit::NewEvent::new(
            AuditKind::ToolCall,
            &id,
            json!({"tool": TIME, "decision": "denied", "policy": "not_granted"}),
        )
        .in_run(Some(&chain)),
    )
    .await
    .unwrap();

    let (_, detail) = fx
        .get(&fx.admin, &format!("/api/v0/system-principals/{id}"))
        .await;
    let audit = detail["principal"]["audit"].as_array().unwrap();
    let run = audit
        .iter()
        .find(|e| e["kind"] == "tool_call")
        .unwrap_or_else(|| panic!("no run event in {audit:?}"));
    assert_eq!(run["chain"], chain.to_json());
    assert_eq!(run["actor_id"], Value::Null);
    assert_eq!(run["detail"]["policy"], "not_granted");
    let created = audit
        .iter()
        .find(|e| e["kind"] == "principal_created")
        .unwrap();
    assert_eq!(created["chain"], Value::Null);
}

#[tokio::test]
async fn revoking_a_token_or_disabling_the_principal_cuts_it_off() {
    let fx = fixture().await;
    let id = fx.create(&fx.admin, "ci").await;
    let (first, first_id) = fx.token(&fx.admin, &id).await;
    let (second, _) = fx.token(&fx.admin, &id).await;
    assert!(first.starts_with("gws_"));
    assert_eq!(fx.models_status(&first).await, StatusCode::OK);

    let (status, _) = fx
        .post(
            &fx.admin,
            &format!("/api/v0/system-principals/{id}/tokens/{first_id}/revoke"),
            json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(fx.models_status(&first).await, StatusCode::UNAUTHORIZED);
    assert_eq!(fx.models_status(&second).await, StatusCode::OK);

    let (status, body) = fx
        .post(
            &fx.admin,
            &format!("/api/v0/system-principals/{id}/disable"),
            json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["principal"]["disabled_at"].is_string(), "{body}");
    assert_eq!(fx.models_status(&second).await, StatusCode::UNAUTHORIZED);

    let (status, _) = fx
        .post(
            &fx.admin,
            &format!("/api/v0/system-principals/{id}/tokens"),
            json!({"name": "again"}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (_, detail) = fx
        .get(&fx.admin, &format!("/api/v0/system-principals/{id}"))
        .await;
    let tokens = detail["principal"]["tokens"].as_array().unwrap();
    assert!(tokens.iter().all(|t| t["revoked"] == true), "{detail}");
    assert!(
        tokens
            .iter()
            .all(|t| t.get("hash").is_none() && t.get("plaintext").is_none()),
        "{detail}"
    );
}

#[tokio::test]
async fn a_system_token_is_never_accepted_as_a_user_token_or_the_other_way() {
    let fx = fixture().await;
    let id = fx.create(&fx.admin, "ci").await;
    let (system, _) = fx.token(&fx.admin, &id).await;
    let user = common::seed_user_with_token(&fx.state, "alice").await;
    let forged_system = system.replacen("gws_", "gwk_", 1);
    let forged_user = user.replacen("gwk_", "gws_", 1);
    assert_eq!(
        fx.models_status(&forged_system).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        fx.models_status(&forged_user).await,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn usage_rows_name_the_principal_not_a_user() {
    let fx = fixture_with_usage(true).await;
    let (id, bearer) = fx.principal_with_model("ci").await;
    assert_eq!(fx.chat(&bearer).await, StatusCode::OK);
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;

    let rows: Vec<(String, String, Option<String>)> =
        sqlx::query_as("SELECT user_id, principal_kind, user_email FROM usage_events")
            .fetch_all(&fx.state.db)
            .await
            .unwrap();
    assert_eq!(rows, [(id, "system".to_string(), Some("ci".to_string()))]);
}

#[tokio::test]
async fn names_are_slugs_and_unique() {
    let fx = fixture().await;
    fx.create(&fx.admin, "support-website").await;
    let (status, body) = fx
        .post(
            &fx.admin,
            "/api/v0/system-principals",
            json!({"name": "support-website"}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    let (status, _) = fx
        .post(
            &fx.admin,
            "/api/v0/system-principals",
            json!({"name": "Not A Slug"}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, body) = fx.get(&fx.admin, "/api/v0/system-principals").await;
    assert_eq!(status, StatusCode::OK);
    let names: Vec<&str> = body["principals"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["support-website"]);
}

async fn listed_models(fx: &Fixture, bearer: &str) -> Vec<String> {
    let req = Request::builder()
        .method(Method::GET)
        .uri("/v1/models")
        .header("authorization", format!("Bearer {bearer}"))
        .body(Body::empty())
        .unwrap();
    let (status, body) = fx.send(req).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap().to_string())
        .collect()
}

/// A model grant is the whole rule: the principal lists and reaches the
/// models it was granted — one in a pool only `vipgroup` may use, too —
/// and no other model, however open its pool.
#[tokio::test]
async fn a_system_principal_lists_only_the_models_of_its_granted_models() {
    let fx = fixture().await;
    let (id, bearer) = fx.principal_with_model("ci").await;
    assert_eq!(listed_models(&fx, &bearer).await, ["model-a"]);

    assert_eq!(
        fx.grant(&fx.admin, &id, "model", "model-vip").await.0,
        StatusCode::CREATED
    );
    let (admins, _) = fx.token(&fx.admin, &id).await;
    assert_eq!(listed_models(&fx, &admins).await, ["model-a", "model-vip"]);
}

async fn upsert_route(fx: &Fixture, alias: &str, candidates: &[&str]) {
    use aiplane_core::server::db::automatic_routes::{
        self, AutomaticRoute, AutomaticRouteCandidate,
    };
    automatic_routes::upsert(
        &fx.state.db,
        &AutomaticRoute {
            alias: alias.into(),
            selector_model: "picker".into(),
            objective: "balanced".into(),
            instructions: String::new(),
            minimum_confidence: 0.5,
            selector_timeout_ms: 1_000,
            fallback_target: candidates[0].into(),
            session_affinity: false,
            session_ttl_seconds: 3_600,
            rollout: "active".into(),
            version: 0,
            candidates: candidates
                .iter()
                .map(|c| AutomaticRouteCandidate {
                    key: c.to_string(),
                    target: c.to_string(),
                    description: c.to_string(),
                })
                .collect(),
        },
    )
    .await
    .unwrap();
}

/// A manager grants a model only when they may use it: a model id or an
/// automatic route whose fallback, candidates and selector they may all
/// use. A route that would hand on a model of a pool they cannot reach is
/// refused like that model; a name the gateway does not serve is missing.
#[tokio::test]
async fn a_model_grant_is_capped_at_the_models_and_routes_the_manager_may_use() {
    let fx = fixture().await;
    upsert_route(&fx, "auto-open", &["model-a", "model-b"]).await;
    upsert_route(&fx, "auto-vip", &["model-a", "model-vip"]).await;
    let id = fx.create(&fx.manager, "ci").await;

    for held in ["model-a", "auto-open"] {
        let (status, body) = fx.grant(&fx.manager, &id, "model", held).await;
        assert_eq!(status, StatusCode::CREATED, "{held}: {body}");
    }
    for beyond in ["model-vip", "auto-vip"] {
        let (status, body) = fx.grant(&fx.manager, &id, "model", beyond).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{beyond}: {body}");
        assert_eq!(body["error"]["code"], "grant_exceeds_manager");
        let message = body["error"]["message"].as_str().unwrap();
        assert!(message.contains(&format!("`{beyond}`")), "{message}");
    }
    let (status, body) = fx.grant(&fx.manager, &id, "model", "no-such-model").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    let (status, body) = fx.grant(&fx.admin, &id, "model", "auto-vip").await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "an admin may use every member: {body}"
    );
}

/// Embedding and image models are granted like chat models, and a
/// principal's token then reaches them over `/v1` — as an agent's
/// `generate_image` call does, under the same access.
#[tokio::test]
async fn embedding_and_image_models_can_be_granted_and_reached() {
    let fx = fixture().await;
    let id = fx.create(&fx.manager, "pipeline").await;
    for model in ["embed-1", "img-1"] {
        let (status, body) = fx.grant(&fx.manager, &id, "model", model).await;
        assert_eq!(status, StatusCode::CREATED, "{model}: {body}");
    }
    let (bearer, _) = fx.token(&fx.manager, &id).await;
    for (uri, body) in [
        ("/v1/embeddings", json!({"model": "embed-1", "input": "hi"})),
        (
            "/v1/images/generations",
            json!({"model": "img-1", "prompt": "a cat", "response_format": "b64_json"}),
        ),
    ] {
        let req = Request::builder()
            .method(Method::POST)
            .uri(uri)
            .header("authorization", format!("Bearer {bearer}"))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let resp = fx.app().serve(req).await.unwrap();
        let status = resp.status();
        let text = String::from_utf8_lossy(&common::read_body(resp).await).to_string();
        assert_eq!(status, StatusCode::OK, "{uri}: {text}");
    }
}

/// A rerank model is granted like an embedding model: a principal holding
/// the grant reaches it over `/v1/rerank`, one without it does not.
#[tokio::test]
async fn a_rerank_model_reaches_only_a_principal_granted_it() {
    let fx = fixture().await;
    let rerank = |bearer: &str| {
        Request::builder()
            .method(Method::POST)
            .uri("/v1/rerank")
            .header("authorization", format!("Bearer {bearer}"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"model": "rerank-1", "query": "q", "documents": ["d"]}).to_string(),
            ))
            .unwrap()
    };

    let granted = fx.create(&fx.manager, "search").await;
    let (status, body) = fx.grant(&fx.manager, &granted, "model", "rerank-1").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (bearer, _) = fx.token(&fx.manager, &granted).await;
    let resp = fx.app().serve(rerank(&bearer)).await.unwrap();
    let status = resp.status();
    let text = String::from_utf8_lossy(&common::read_body(resp).await).to_string();
    assert_eq!(status, StatusCode::OK, "{text}");

    let other = fx.create(&fx.manager, "pipeline").await;
    let (status, body) = fx.grant(&fx.manager, &other, "model", "embed-1").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (bearer, _) = fx.token(&fx.manager, &other).await;
    let resp = fx.app().serve(rerank(&bearer)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// Whether principal `id`'s grants reach `model-a` on pool `pool_name`.
async fn reaches(fx: &Fixture, id: &str, pool_name: &str) -> bool {
    let principal = aiplane_agents::db::system_principals::load_active(&fx.state.db, id)
        .await
        .unwrap()
        .unwrap();
    let pool = fx
        .state
        .upstreams
        .pools()
        .into_iter()
        .find(|p| p.name == pool_name)
        .unwrap();
    upstreams::PoolAccess::for_system(&principal).reaches(&pool, "model-a")
}

/// `model-a` is served by the open pool and by `vip`, which the manager is
/// not in: the manager's grant records the open pool only, and the principal
/// never reaches `vip` with it; an admin's grant names no pools and reaches
/// both.
#[tokio::test]
async fn a_model_grant_routes_only_through_the_pools_its_manager_could_use() {
    let fx = fixture().await;
    let id = fx.create(&fx.manager, "ci").await;
    let (status, body) = fx.grant(&fx.manager, &id, "model", "model-a").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (_, shown) = fx
        .get(&fx.manager, &format!("/api/v0/system-principals/{id}"))
        .await;
    let grant = shown["principal"]["grants"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["ref"] == "model-a")
        .cloned()
        .unwrap();
    assert_eq!(
        grant["pools"],
        json!(["late", "pool"]),
        "every chat pool the manager may use, serving the model yet or not: {shown}"
    );

    assert!(reaches(&fx, &id, "pool").await);
    assert!(
        !reaches(&fx, &id, "vip").await,
        "the manager could not use `vip`"
    );
    common::seed_pool_models(&fx.state.upstreams, "late", 0, &["model-a"]);
    assert!(
        reaches(&fx, &id, "late").await,
        "a pool the manager could use serves it later: the grant reaches it"
    );

    let (status, body) = fx.grant(&fx.admin, &id, "model", "model-a").await;
    assert_eq!(status, StatusCode::OK, "held already: {body}");
    assert_eq!(
        body["widened"], true,
        "an admin's regrant reaches every pool"
    );
    let (status, body) = fx.grant(&fx.manager, &id, "model", "model-a").await;
    assert_eq!(
        (status, body["widened"].clone()),
        (StatusCode::OK, json!(false))
    );
    assert!(
        reaches(&fx, &id, "vip").await && reaches(&fx, &id, "pool").await,
        "an admin's grant reaches every pool"
    );
}

/// An admin's grant on `model-a` reaches every pool, `vip` included; a
/// token the (non-vip) manager mints carries it only through the pools the
/// manager may use, so `/v1/chat/completions` never lands on `vip`. A token
/// an admin mints keeps the grant's reach.
#[tokio::test]
async fn a_minted_token_narrows_a_model_grant_to_the_minters_pools() {
    let fx = fixture().await;
    let id = fx.create(&fx.manager, "ci").await;
    let (status, body) = fx.grant(&fx.admin, &id, "model", "model-a").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let principal = aiplane_agents::db::system_principals::load_active(&fx.state.db, &id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(principal.grants.model_pools("model-a"), None);

    let capped = aiplane_runtime::server::grant_holding::capped_to_minter(
        &fx.state,
        principal.clone(),
        "manager",
    )
    .await
    .unwrap();
    let reach: Vec<&str> = capped
        .grants
        .model_pools("model-a")
        .expect("narrowed")
        .iter()
        .map(String::as_str)
        .collect();
    assert_eq!(
        reach,
        ["late", "pool"],
        "the manager's chat pools, not `vip`"
    );
    let by_admin =
        aiplane_runtime::server::grant_holding::capped_to_minter(&fx.state, principal, "admin")
            .await
            .unwrap();
    assert_eq!(
        by_admin.grants.model_pools("model-a"),
        None,
        "an admin's token keeps it"
    );

    let (bearer, _) = fx.token(&fx.manager, &id).await;
    for _ in 0..6 {
        assert_eq!(fx.chat(&bearer).await, StatusCode::OK);
    }
    assert!(
        fx.vip_upstream
            .received_requests()
            .await
            .unwrap()
            .is_empty(),
        "a manager's token never lands on a pool the manager may not use"
    );
}

/// A grant on the alias `fast` is a grant on the name `fast`: a caller
/// naming its target `model-a` directly holds nothing — 404 on both chat
/// dialects and on the model lookup, as for any model it was not granted.
#[tokio::test]
async fn a_granted_alias_does_not_grant_its_target_by_name() {
    let fx = fixture().await;
    let id = fx.create(&fx.manager, "ci").await;
    let (status, body) = fx.grant(&fx.manager, &id, "model", "fast").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (bearer, _) = fx.token(&fx.manager, &id).await;
    let call = |uri: &'static str, model: &'static str| {
        let bearer = bearer.clone();
        let body = if uri == "/v1/messages" {
            json!({"model": model, "max_tokens": 16,
                   "messages": [{"role": "user", "content": "hi"}]})
        } else {
            json!({"model": model, "messages": [{"role": "user", "content": "hi"}]})
        };
        Request::builder()
            .method(Method::POST)
            .uri(uri)
            .header("authorization", format!("Bearer {bearer}"))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    };
    let status = |req| async { fx.app().serve(req).await.unwrap().status() };
    assert_eq!(
        status(call("/v1/chat/completions", "fast")).await,
        StatusCode::OK
    );
    assert_eq!(
        status(call("/v1/chat/completions", "model-a")).await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        status(call("/v1/messages", "model-a")).await,
        StatusCode::NOT_FOUND
    );
    let lookup = Request::builder()
        .method(Method::GET)
        .uri("/v1/models/model-a")
        .header("authorization", format!("Bearer {bearer}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(status(lookup).await, StatusCode::NOT_FOUND);
}

/// A principal with a tool grant streams its tool loop on a name it holds
/// only as an alias (`fast`) or as an automatic route whose fallback is
/// that alias: every streamed round routes the resolved id, on the OpenAI
/// and on the Anthropic endpoint.
#[tokio::test]
async fn a_streamed_tool_round_routes_a_granted_alias_and_route() {
    let fx = fixture().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_partial_json(json!({"stream": true})))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            format!(
                "data: {}\n\ndata: [DONE]\n\n",
                json!({"choices": [{"index": 0, "delta": {"content": "streamed"}}]})
            ),
            "text/event-stream",
        ))
        .with_priority(1)
        .mount(&fx.upstream)
        .await;
    upsert_route(&fx, "auto-fast", &["fast", "model-b"]).await;
    let id = fx.create(&fx.admin, "ci").await;
    for (kind, reference) in [("tool", TIME), ("model", "fast"), ("model", "auto-fast")] {
        let (status, body) = fx.grant(&fx.admin, &id, kind, reference).await;
        assert_eq!(status, StatusCode::CREATED, "{reference}: {body}");
    }
    let (bearer, _) = fx.token(&fx.admin, &id).await;
    for model in ["fast", "auto-fast"] {
        for (uri, body) in [
            (
                "/v1/chat/completions",
                json!({"model": model, "stream": true,
                       "messages": [{"role": "user", "content": "hi"}]}),
            ),
            (
                "/v1/messages",
                json!({"model": model, "stream": true, "max_tokens": 16,
                       "messages": [{"role": "user", "content": "hi"}]}),
            ),
        ] {
            let req = Request::builder()
                .method(Method::POST)
                .uri(uri)
                .header("authorization", format!("Bearer {bearer}"))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap();
            let resp = fx.app().serve(req).await.unwrap();
            let status = resp.status();
            let text = String::from_utf8_lossy(&common::read_body(resp).await).to_string();
            assert_eq!(status, StatusCode::OK, "{model} {uri}: {text}");
            assert!(text.contains("streamed"), "{model} {uri}: {text}");
        }
    }
    let streamed = fx
        .upstream
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter_map(|r| serde_json::from_slice::<Value>(&r.body).ok())
        .filter(|b| b["stream"] == true)
        .count();
    assert_eq!(streamed, 4, "every streamed round reached the upstream");
}

const AGENT: &str = "erp";

/// An `agent` connector created the way an operator would: through the admin
/// API, with a static bearer the gateway sends for every principal.
async fn agent_connector(fx: &Fixture, allowed_groups: &[&str]) {
    let body = json!({
        "key": AGENT,
        "title": "ERP",
        "base_url": fx._mcp.uri(),
        "scope": "agent",
        "auth_type": "static_bearer",
        "client_secret": "erp-secret",
        "groups": allowed_groups,
    });
    let req = Request::builder()
        .method(Method::PUT)
        .uri("/api/v0/admin/connectors")
        .header("cookie", format!("id={}", fx.admin))
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let (status, body) = fx.send(req).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = fx
        .post(
            &fx.admin,
            &format!("/api/v0/admin/connectors/{AGENT}/toggle"),
            json!({"enabled": true}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, listed) = fx.get(&fx.admin, "/api/v0/admin/connectors").await;
    let scope = listed["connectors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["key"] == AGENT)
        .map(|c| c["scope"].clone());
    assert_eq!(scope, Some(json!("agent")), "{listed}");
}

fn agent_tool() -> String {
    format!("mcp__{AGENT}__erp_lookup")
}

#[tokio::test]
async fn an_agent_connector_cannot_use_per_user_oauth() {
    let fx = fixture().await;
    let body = json!({
        "key": "erp2", "base_url": "http://erp/mcp", "scope": "agent", "auth_type": "oauth2",
    });
    let req = Request::builder()
        .method(Method::PUT)
        .uri("/api/v0/admin/connectors")
        .header("cookie", format!("id={}", fx.admin))
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let (status, body) = fx.send(req).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body["error"]["message"].as_str().unwrap().contains("agent"),
        "{body}"
    );
}

#[tokio::test]
async fn no_person_ever_sees_an_agent_connector_admin_included() {
    let fx = fixture().await;
    agent_connector(&fx, &[]).await;
    gateway_groups::set_tools_for_group(&fx.state.db, "everyone", &[format!("mcp__{AGENT}")])
        .await
        .unwrap();
    fx.state.reload_rbac().await;

    for (who, roles) in [("alice", vec![]), ("admin", vec!["admins".to_string()])] {
        let layer = fx
            .state
            .mcp_layer_for(
                &Principal::User {
                    id: who.into(),
                    roles,
                },
                AskContext::Chat,
            )
            .await;
        assert!(
            !layer.tool_ids().iter().any(|id| id.contains(AGENT)),
            "{who} was offered {:?}",
            layer.tool_ids()
        );
    }
    for cookie in [&fx.plain, &fx.admin] {
        let (status, body) = fx.get(cookie, "/api/v0/integrations").await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            body["connectors"]
                .as_array()
                .unwrap()
                .iter()
                .all(|c| c["key"] != AGENT),
            "{body}"
        );
    }
    let (_, groups) = fx.get(&fx.admin, "/api/v0/admin/groups").await;
    assert!(
        groups["tool_families"]
            .as_array()
            .unwrap()
            .iter()
            .all(|f| f["id"] != format!("mcp__{AGENT}")),
        "{groups}"
    );
}

#[tokio::test]
async fn a_granted_principal_uses_an_agent_connector_an_ungranted_one_does_not() {
    let fx = fixture().await;
    agent_connector(&fx, &[]).await;
    let (granted, granted_bearer) = fx.principal_with_model("granted").await;
    let (_, other_bearer) = fx.principal_with_model("other").await;
    let (status, body) = fx.grant(&fx.manager, &granted, "connector", AGENT).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    assert_eq!(fx.chat(&granted_bearer).await, StatusCode::OK);
    assert_eq!(fx.offered_tools().await, [agent_tool()]);

    assert_eq!(fx.chat(&other_bearer).await, StatusCode::OK);
    assert!(fx.offered_tools().await.is_empty());
}

#[tokio::test]
async fn an_agent_connectors_groups_decide_who_may_grant_it() {
    let fx = fixture().await;
    agent_connector(&fx, &["admins"]).await;
    let id = fx.create(&fx.manager, "ci").await;
    let (status, body) = fx.grant(&fx.manager, &id, "connector", AGENT).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"]["code"], "grant_exceeds_manager");
    let (status, _) = fx.grant(&fx.admin, &id, "connector", AGENT).await;
    assert_eq!(status, StatusCode::CREATED);
}

#[tokio::test]
async fn only_an_admin_grants_an_external_a2a_agent_and_only_by_a_valid_card_url() {
    const CARD: &str = "https://partner.example.com/.well-known/agent-card.json";
    let fx = fixture().await;
    let id = fx.create(&fx.manager, "ci").await;
    let (status, body) = fx.grant(&fx.manager, &id, "a2a_agent", CARD).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"]["code"], "grant_exceeds_manager");
    assert!(
        body["error"]["message"].as_str().unwrap().contains("admin"),
        "{body}"
    );

    let (status, body) = fx
        .grant(
            &fx.admin,
            &id,
            "a2a_agent",
            "http://partner.example.com/card",
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body["error"]["message"].as_str().unwrap().contains("https"),
        "{body}"
    );

    let (status, body) = fx.grant(&fx.admin, &id, "a2a_agent", CARD).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let principal = aiplane_agents::db::system_principals::load_active(&fx.state.db, &id)
        .await
        .unwrap()
        .unwrap();
    assert!(principal.grants.has(GrantKind::A2aAgent, CARD));
}

/// A principal that is not an agent belongs to whoever created it: another
/// manager can neither see it nor change it, so the grant-time cap cannot be
/// sidestepped by working on a principal someone with more rights set up.
#[tokio::test]
async fn only_the_creator_or_an_admin_manages_a_principal_that_is_not_an_agent() {
    let fx = fixture().await;
    let id = fx.create(&fx.manager, "ci").await;
    assert_eq!(
        fx.grant(&fx.manager, &id, "model", "model-a").await.0,
        StatusCode::CREATED
    );
    let (_, token_id) = fx.token(&fx.manager, &id).await;

    let other = &fx.other_manager;
    let (status, body) = fx
        .get(other, &format!("/api/v0/system-principals/{id}"))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let message = body["error"]["message"].as_str().unwrap();
    assert!(message.contains("creator or an admin"), "{message}");
    let (_, list) = fx.get(other, "/api/v0/system-principals").await;
    assert!(list["principals"].as_array().unwrap().is_empty(), "{list}");
    for (uri, body) in [
        (
            format!("/api/v0/system-principals/{id}/tokens"),
            json!({"name": "x"}),
        ),
        (
            format!("/api/v0/system-principals/{id}/grants"),
            json!({"kind": "tool", "ref": TIME}),
        ),
        (
            format!("/api/v0/system-principals/{id}/grants/revoke"),
            json!({"kind": "model", "ref": "model-a"}),
        ),
        (
            format!("/api/v0/system-principals/{id}/tokens/{token_id}/revoke"),
            json!({}),
        ),
        (format!("/api/v0/system-principals/{id}/disable"), json!({})),
    ] {
        let (status, resp) = fx.post(other, &uri, body).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}: {resp}");
    }
    let (_, detail) = fx
        .get(&fx.manager, &format!("/api/v0/system-principals/{id}"))
        .await;
    let p = &detail["principal"];
    assert_eq!(p["grants"].as_array().unwrap().len(), 1, "{p}");
    assert_eq!(p["tokens"].as_array().unwrap().len(), 1, "{p}");
    assert_eq!(p["tokens"][0]["revoked"], false, "{p}");
    assert!(p["disabled_at"].is_null(), "{p}");

    // The creator and an admin can.
    assert_eq!(
        fx.grant(&fx.manager, &id, "tool", TIME).await.0,
        StatusCode::CREATED
    );
    assert_eq!(
        fx.grant(&fx.admin, &id, "skill", "brand").await.0,
        StatusCode::CREATED
    );
    fx.token(&fx.admin, &id).await;
    let (status, _) = fx
        .post(
            &fx.admin,
            &format!("/api/v0/system-principals/{id}/tokens/{token_id}/revoke"),
            json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, list) = fx.get(&fx.manager, "/api/v0/system-principals").await;
    assert_eq!(list["principals"].as_array().unwrap().len(), 1, "{list}");
}

/// A token hands every grant of its principal to whoever holds it, so issuing
/// one is granting all of them again, and takes the same cap.
#[tokio::test]
async fn issuing_a_token_needs_every_grant_the_principal_holds() {
    let fx = fixture().await;
    let id = fx.create(&fx.manager, "ci").await;
    for (kind, reference) in [("model", "model-a"), ("tool", TIME)] {
        assert_eq!(
            fx.grant(&fx.manager, &id, kind, reference).await.0,
            StatusCode::CREATED
        );
    }
    gateway_groups::set_tools_for_group(&fx.state.db, "everyone", &["read_skill".into()])
        .await
        .unwrap();
    fx.state.reload_rbac().await;

    let (status, body) = fx
        .post(
            &fx.manager,
            &format!("/api/v0/system-principals/{id}/tokens"),
            json!({"name": "pipeline"}),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"]["code"], "token_exceeds_manager");
    let message = body["error"]["message"].as_str().unwrap();
    assert!(message.contains(&format!("`{TIME}`")), "{message}");
    assert!(message.contains("Ask an admin"), "{message}");

    fx.token(&fx.admin, &id).await;
    let (status, _) = fx
        .post(
            &fx.manager,
            &format!("/api/v0/system-principals/{id}/grants/revoke"),
            json!({"kind": "tool", "ref": TIME}),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    fx.token(&fx.manager, &id).await;
}

async fn collection(fx: &Fixture, name: &str, description: Option<&str>) -> i64 {
    use aiplane_core::server::db::rag;
    rag::create_collection(
        &fx.state.db,
        &rag::NewCollection {
            name: name.into(),
            description: description.map(Into::into),
            git_url: String::new(),
            git_ref: "main".into(),
            pat: None,
            source: Default::default(),
            profile_id: None,
            extraction_model: None,
            embedding_model: "embed-1".into(),
            include_globs: Vec::new(),
            exclude_globs: Vec::new(),
            chunk_size: 400,
            chunk_overlap: 40,
            search_mode: rag::SearchMode::Versioned,
            refresh_interval_mins: 0,
        },
    )
    .await
    .unwrap()
    .id
}

fn items(body: &Value) -> Vec<Value> {
    body["items"].as_array().cloned().unwrap_or_default()
}

fn item(body: &Value, kind: &str, key: &str) -> Value {
    items(body)
        .into_iter()
        .find(|i| i["kind"] == kind && i["key"] == key)
        .unwrap_or_else(|| panic!("{kind} {key} is not offered: {body}"))
}

fn shown(item: &Value) -> (String, String, String, String) {
    let text = |field: &str| item[field].as_str().unwrap_or_default().to_string();
    (
        text("kind"),
        text("key"),
        text("title"),
        text("description"),
    )
}

async fn chat_picker(fx: &Fixture, cookie: &str, user: &str) -> Value {
    let session = session_core::db::create_session(&fx.state.db, user)
        .await
        .unwrap();
    let (status, picker) = fx
        .get(
            cookie,
            &format!("/api/v0/chat/sessions/{}/capabilities", session.id),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{picker}");
    picker
}

/// The agent setup and the chat picker describe a resource from one source:
/// what both offer, they show with the same title and description.
#[tokio::test]
async fn agent_resources_describe_what_they_share_with_the_chat_picker_identically() {
    let fx = fixture().await;
    mcp_catalog::update(
        &fx.state.db,
        GLOBAL,
        mcp_catalog::ConnectorInput {
            name: "Global tools".into(),
            description: Some("Looks things up in the shared directory.".into()),
            ..connector(GLOBAL, &fx._mcp.uri(), mcp_catalog::Scope::Global)
        },
    )
    .await
    .unwrap();

    let picker = chat_picker(&fx, &fx.manager, "manager").await;
    let (status, resources) = fx.get(&fx.manager, "/api/v0/agent-resources").await;
    assert_eq!(status, StatusCode::OK, "{resources}");

    let offered: Vec<_> = items(&resources).iter().map(shown).collect();
    let in_chat: Vec<_> = picker["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(shown)
        .collect();
    for (kind, key) in [
        ("tool", TIME.to_string()),
        ("tool", format!("mcp__{GLOBAL}")),
        ("skill", "brand".to_string()),
    ] {
        let chat = in_chat.iter().find(|c| c.0 == kind && c.1 == key);
        let agents = offered.iter().find(|c| c.0 == kind && c.1 == key);
        assert!(chat.is_some(), "{kind} {key} missing in chat: {in_chat:?}");
        assert_eq!(chat, agents, "{kind} {key} is shown differently");
    }

    let connector = item(&resources, "tool", &format!("mcp__{GLOBAL}"));
    assert_eq!(connector["title"], "Global tools");
    assert_eq!(
        connector["description"],
        "Looks things up in the shared directory."
    );
    assert_eq!(
        connector["grant"],
        json!({ "kind": "connector", "refs": [GLOBAL] })
    );
    let skill = item(&resources, "skill", "brand");
    assert_eq!(skill["title"], "Brand");
    assert_eq!(skill["description"], "Enforce the brand.");
    assert_eq!(
        skill["grant"],
        json!({ "kind": "skill", "refs": ["brand"] })
    );
    let time = item(&resources, "tool", TIME);
    assert_eq!(time["grant"], json!({ "kind": "tool", "refs": [TIME] }));
    assert_eq!(time["tools"], json!([TIME]));
    assert_ne!(time["title"], TIME, "the catalog names the tool");
}

/// A knowledge base is offered with the description its admin maintains, or
/// with none at all; only who may edit it is told where to add one. The chat
/// picker, which has its own way to search knowledge, offers none.
#[tokio::test]
async fn agent_resources_offer_knowledge_bases_with_their_own_description_or_none() {
    let fx = fixture().await;
    let described = collection(&fx, "Ceph docs", Some("The Ceph administration manual.")).await;
    let bare = collection(&fx, "Contracts", None).await;

    let (_, resources) = fx.get(&fx.manager, "/api/v0/agent-resources").await;
    let shown = item(&resources, "rag_collection", &described.to_string());
    assert_eq!(shown["title"], "Ceph docs");
    assert_eq!(shown["description"], "The Ceph administration manual.");
    assert_eq!(
        shown["grant"],
        json!({ "kind": "rag_collection", "refs": [described.to_string()] })
    );
    let shown = item(&resources, "rag_collection", &bare.to_string());
    assert_eq!(shown["title"], "Contracts");
    assert_eq!(shown["description"], "");
    assert_eq!(shown["editable"], false, "only an admin edits a collection");
    assert_eq!(shown["config_url"], Value::Null);

    let (_, resources) = fx.get(&fx.admin, "/api/v0/agent-resources").await;
    let shown = item(&resources, "rag_collection", &bare.to_string());
    assert_eq!(shown["editable"], true);
    assert_eq!(shown["config_url"], format!("/rag/{bare}/edit"));

    let picker = chat_picker(&fx, &fx.manager, "manager").await;
    assert!(
        picker["tools"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["kind"] != "rag_collection"),
        "{picker}"
    );
}

/// A manager is offered only what they hold, and may edit none of the
/// admin-maintained resources; an admin is pointed at each one's edit page.
#[tokio::test]
async fn agent_resources_are_capped_by_what_the_manager_holds() {
    let fx = fixture().await;

    let (_, managed) = fx.get(&fx.manager, "/api/v0/agent-resources").await;
    let refs: Vec<String> = items(&managed)
        .iter()
        .flat_map(|i| i["grant"]["refs"].as_array().cloned().unwrap_or_default())
        .filter_map(|r| r.as_str().map(String::from))
        .collect();
    assert!(refs.contains(&TIME.to_string()), "{managed}");
    assert!(!refs.contains(&"company_echo".to_string()), "{managed}");
    assert!(!refs.contains(&PER_USER.to_string()), "{managed}");
    assert!(
        items(&managed)
            .iter()
            .all(|i| i["editable"] == false && i["config_url"].is_null()),
        "{managed}"
    );

    let (_, admin) = fx.get(&fx.admin, "/api/v0/agent-resources").await;
    let time = item(&admin, "tool", TIME);
    assert_eq!(
        (&time["editable"], &time["config_url"]),
        (&json!(false), &Value::Null),
        "nothing about a tool is maintained anywhere, so nothing links"
    );
    let skill = item(&admin, "skill", "brand");
    assert_eq!(skill["editable"], true);
    assert_eq!(skill["config_url"], "/admin/skills");
    let connector = item(&admin, "tool", &format!("mcp__{GLOBAL}"));
    assert_eq!(
        connector["config_url"],
        format!("/admin/connectors/{GLOBAL}/edit")
    );
}
