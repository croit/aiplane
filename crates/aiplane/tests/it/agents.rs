// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Agent definitions end to end (`docs/agents.md` → "Agent definition"): drafts,
//! versions with a live pointer, shares, and spec validation against the
//! agent principal's grants.
//!
//! Two managers (`alice`, `bob`) hold `can_manage_agents` through the
//! `managers` group; `plain` holds nothing but the default group, which grants
//! the time tool and leaves the pool serving model `m` open to everyone — so every
//! manager holds both and can grant them to an agent. `root` is an admin.

use std::collections::HashMap;
use std::sync::Arc;

use rama::Service;
use rama::http::{Body, Method, Request, StatusCode};
use serde_json::{Value, json};

use crate::common::{self, TEST_SECRET};

use aiplane::rama_server::{RamaState, SessionStore};
use aiplane_agents::db::agent_audit;
use aiplane_core::server::db::{self, gateway_groups, users};
use aiplane_core::server::rbac::{BOOTSTRAP_ADMIN_GROUP, Resolver};
use aiplane_core::server::upstreams::{
    self,
    config::{PickerStrategy, PoolKind, UpstreamPoolConfig},
};
use aiplane_runtime::server::AppState;
use aiplane_runtime::server::tools::ToolRegistry;
use aiplane_runtime::server::tools::time::CurrentTimestamp;

pub(crate) const TIME: &str = "get_current_timestamp";

/// The break-glass admin claim value of the fixture.
const BREAK_GLASS: &str = "break-glass";

pub(crate) struct Fx {
    pub(crate) state: RamaState,
    pub(crate) root: String,
    pub(crate) alice: String,
    pub(crate) bob: String,
    pub(crate) plain: String,
}

pub(crate) async fn person(state: &RamaState, id: &str, roles: &[&str]) -> String {
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

pub(crate) async fn fixture() -> Fx {
    fixture_on(None).await
}

/// The fixture with `pool` served by `upstream`, a chat backend that
/// serves model `m`, when one is given.
pub(crate) async fn fixture_on(upstream: Option<&str>) -> Fx {
    fixture_with(
        upstream,
        ToolRegistry::new().with(CurrentTimestamp),
        &[TIME],
    )
    .await
}

/// The fixture with its own tool registry, every tool of `everyone` granted to
/// the default group, so each manager holds them and can grant them on.
pub(crate) async fn fixture_with(
    upstream: Option<&str>,
    tools: ToolRegistry,
    everyone: &[&str],
) -> Fx {
    fixture_with_pools(upstream, tools, everyone, Vec::new()).await
}

/// [`fixture_with`] plus `extra` pools beside `pool`, open to everyone; each
/// backend of an extra pool serves the models it is given.
pub(crate) async fn fixture_with_pools(
    upstream: Option<&str>,
    tools: ToolRegistry,
    everyone: &[&str],
    extra: Vec<(String, UpstreamPoolConfig, Vec<String>)>,
) -> Fx {
    let pool = db::open(std::path::Path::new(":memory:")).await.unwrap();
    let mut pools = HashMap::new();
    let mut served = HashMap::new();
    for (name, config, models) in extra {
        served.insert(name.clone(), models);
        pools.insert(name, config);
    }
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
            backend: vec![common::mock_backend(
                "mock",
                upstream.unwrap_or("http://127.0.0.1:9"),
            )],
        },
    );
    let registry = upstreams::UpstreamRegistry::new(&pools).unwrap();
    for p in registry.pools() {
        if let Some(models) = served.get(&p.name) {
            p.backends[0].set_models(models.iter().cloned().collect());
        } else {
            p.backends[0].set_models(["m".to_string()].into());
        }
    }
    let app = AppState::new(
        common::test_config(),
        pool.clone(),
        registry,
        Arc::new(tools),
        Arc::new(Resolver::from_snapshot(
            Default::default(),
            vec![BREAK_GLASS.into()],
        )),
    );
    let state = RamaState::new(
        app,
        SessionStore::new(pool.clone(), TEST_SECRET),
        aiplane_core::server::usage::UsageHandle::disabled(),
    );

    gateway_groups::upsert_group(&pool, "everyone", "", false, true)
        .await
        .unwrap();
    let everyone: Vec<String> = everyone.iter().map(|t| t.to_string()).collect();
    gateway_groups::set_tools_for_group(&pool, "everyone", &everyone)
        .await
        .unwrap();
    gateway_groups::upsert_group(&pool, "managers", "", false, false)
        .await
        .unwrap();
    gateway_groups::set_can_manage_agents(&pool, "managers", true)
        .await
        .unwrap();
    gateway_groups::set_can_manage_dots(&pool, "managers", true)
        .await
        .unwrap();
    gateway_groups::set_mappings_for_group(&pool, "managers", &["managers".into()])
        .await
        .unwrap();
    gateway_groups::upsert_group(&pool, "admins", "", true, false)
        .await
        .unwrap();
    gateway_groups::set_mappings_for_group(&pool, "admins", &["admins".into()])
        .await
        .unwrap();
    gateway_groups::upsert_group(&pool, "support", "", false, false)
        .await
        .unwrap();
    state.reload_rbac().await;

    let root = person(&state, "root", &["admins"]).await;
    let alice = person(&state, "alice", &["managers"]).await;
    let bob = person(&state, "bob", &["managers"]).await;
    let plain = person(&state, "plain", &[]).await;
    Fx {
        state,
        root,
        alice,
        bob,
        plain,
    }
}

impl Fx {
    pub(crate) async fn send(
        &self,
        cookie: Option<&str>,
        method: Method,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut req = Request::builder().method(method).uri(uri);
        if let Some(c) = cookie {
            req = req.header("cookie", format!("id={c}"));
        }
        let req = match body {
            Some(b) => req
                .header("content-type", "application/json")
                .body(Body::from(b.to_string())),
            None => req.body(Body::empty()),
        }
        .unwrap();
        let resp = common::app(self.state.clone()).serve(req).await.unwrap();
        let status = resp.status();
        let bytes = common::read_body(resp).await;
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    pub(crate) async fn get(&self, cookie: &str, uri: &str) -> (StatusCode, Value) {
        self.send(Some(cookie), Method::GET, uri, None).await
    }

    pub(crate) async fn post(&self, cookie: &str, uri: &str, body: Value) -> (StatusCode, Value) {
        self.send(Some(cookie), Method::POST, uri, Some(body)).await
    }

    pub(crate) async fn put(&self, cookie: &str, uri: &str, body: Value) -> (StatusCode, Value) {
        self.send(Some(cookie), Method::PUT, uri, Some(body)).await
    }

    pub(crate) async fn put_draft(
        &self,
        cookie: &str,
        id: &str,
        spec: Value,
    ) -> (StatusCode, Value) {
        self.send(
            Some(cookie),
            Method::PUT,
            &format!("/api/v0/agents/{id}/draft"),
            Some(json!({ "spec": spec })),
        )
        .await
    }

    pub(crate) async fn create(&self, cookie: &str, name: &str) -> String {
        let (status, body) = self
            .post(cookie, "/api/v0/agents", json!({ "name": name }))
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        body["agent"]["id"].as_str().unwrap().to_string()
    }

    pub(crate) async fn grant(&self, cookie: &str, id: &str, kind: &str, r: &str) -> StatusCode {
        self.post(
            cookie,
            &format!("/api/v0/system-principals/{id}/grants"),
            json!({ "kind": kind, "ref": r }),
        )
        .await
        .0
    }

    pub(crate) async fn publish(&self, cookie: &str, id: &str) -> (StatusCode, Value) {
        self.post(cookie, &format!("/api/v0/agents/{id}/publish"), json!({}))
            .await
    }

    pub(crate) async fn share(
        &self,
        cookie: &str,
        id: &str,
        kind: &str,
        subject: &str,
        access: &str,
    ) -> (StatusCode, Value) {
        self.post(
            cookie,
            &format!("/api/v0/agents/{id}/shares"),
            json!({ "subject_kind": kind, "subject_id": subject, "access": access }),
        )
        .await
    }

    /// An agent with the model and the time tool granted and a publishable
    /// draft that uses both.
    pub(crate) async fn runnable(&self, name: &str) -> String {
        let id = self.create(&self.alice, name).await;
        assert_eq!(
            self.grant(&self.alice, &id, "model", "m").await,
            StatusCode::CREATED
        );
        assert_eq!(
            self.grant(&self.alice, &id, "tool", TIME).await,
            StatusCode::CREATED
        );
        let (status, body) = self.put_draft(&self.alice, &id, spec("v1")).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        id
    }

    pub(crate) async fn audit_kinds(&self, id: &str) -> Vec<String> {
        agent_audit::for_principal(&self.state.db, id)
            .await
            .unwrap()
            .into_iter()
            .rev()
            .map(|e| e.kind)
            .collect()
    }
}

pub(crate) fn spec(orchestration: &str) -> Value {
    json!({
        "main": {
            "model": "m",
            "instructions": { "orchestration": orchestration },
            "tools": [TIME]
        }
    })
}

#[tokio::test]
async fn every_agent_route_needs_a_session() {
    let fx = fixture().await;
    for (method, uri) in [
        (Method::GET, "/api/v0/agents"),
        (Method::POST, "/api/v0/agents"),
        (Method::GET, "/api/v0/agents/x"),
        (Method::DELETE, "/api/v0/agents/x"),
        (Method::PUT, "/api/v0/agents/x/draft"),
        (Method::POST, "/api/v0/agents/x/publish"),
        (Method::GET, "/api/v0/agents/x/versions"),
        (Method::POST, "/api/v0/agents/x/live"),
        (Method::GET, "/api/v0/agents/x/shares"),
        (Method::POST, "/api/v0/agents/x/shares"),
        (Method::POST, "/api/v0/agents/x/shares/revoke"),
    ] {
        let (status, _) = fx.send(None, method.clone(), uri, Some(json!({}))).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {uri}");
    }
}

#[tokio::test]
async fn a_non_manager_can_neither_create_list_read_nor_share() {
    let fx = fixture().await;
    let id = fx.create(&fx.alice, "support").await;
    let (status, body) = fx
        .post(&fx.plain, "/api/v0/agents", json!({ "name": "mine" }))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("enable agent management"),
        "{body}"
    );
    assert_eq!(
        fx.get(&fx.plain, "/api/v0/agents").await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        fx.get(&fx.plain, &format!("/api/v0/agents/{id}")).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        fx.share(&fx.plain, &id, "user", "plain", "read").await.0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn creating_an_agent_creates_a_principal_without_rights_and_a_write_share() {
    let fx = fixture().await;
    let (status, body) = fx
        .post(
            &fx.alice,
            "/api/v0/agents",
            json!({ "name": "support", "display": "Support", "spec": { "profile": { "display": "Hi" } } }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id = body["agent"]["id"].as_str().unwrap();
    assert_eq!(body["agent"]["access"], "write");
    assert_eq!(body["agent"]["live_version"], Value::Null);

    let (_, detail) = fx.get(&fx.alice, &format!("/api/v0/agents/{id}")).await;
    let agent = &detail["agent"];
    assert_eq!(agent["draft_spec"]["profile"]["display"], "Hi");
    assert_eq!(agent["grants"], json!([]));
    let mut shares = agent["shares"].clone();
    assert!(shares[0]["granted"]["at"].is_string(), "{shares}");
    shares[0]["granted"]["at"] = Value::Null;
    assert_eq!(
        shares,
        json!([{ "subject_kind": "user", "subject_id": "alice", "access": "write", "name": null,
                 "granted": { "by": { "kind": "user", "id": "alice", "name": null, "email": "alice@example.com" }, "at": null } }])
    );
    assert_eq!(agent["publish_issues"][0]["path"], "main");

    let (_, principal) = fx
        .get(&fx.alice, &format!("/api/v0/system-principals/{id}"))
        .await;
    assert_eq!(principal["principal"]["name"], "support");

    let (_, list) = fx.get(&fx.alice, "/api/v0/agents").await;
    assert_eq!(list["agents"].as_array().unwrap().len(), 1);
    assert_eq!(list["agents"][0]["name"], "support");

    let (status, body) = fx
        .post(&fx.alice, "/api/v0/agents", json!({ "name": "support" }))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    let (status, _) = fx
        .post(&fx.alice, "/api/v0/agents", json!({ "name": "Not A Slug" }))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn an_agent_is_invisible_to_a_manager_without_a_share() {
    let fx = fixture().await;
    let id = fx.create(&fx.alice, "support").await;
    let (_, list) = fx.get(&fx.bob, "/api/v0/agents").await;
    assert_eq!(list["agents"], json!([]));
    for uri in [
        format!("/api/v0/agents/{id}"),
        format!("/api/v0/agents/{id}/versions"),
        format!("/api/v0/agents/{id}/shares"),
        format!("/api/v0/system-principals/{id}"),
    ] {
        assert_eq!(
            fx.get(&fx.bob, &uri).await.0,
            StatusCode::NOT_FOUND,
            "{uri}"
        );
    }
    assert_eq!(
        fx.grant(&fx.bob, &id, "model", "m").await,
        StatusCode::NOT_FOUND
    );
    let (_, principals) = fx.get(&fx.bob, "/api/v0/system-principals").await;
    assert_eq!(principals["principals"], json!([]));
}

#[tokio::test]
async fn a_spec_naming_an_ungranted_tool_is_rejected_with_the_grant_to_make() {
    let fx = fixture().await;
    let id = fx.create(&fx.alice, "support").await;
    let (status, body) = fx.put_draft(&fx.alice, &id, spec("hi")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["error"]["code"], "invalid_agent_spec");
    let issues = body["error"]["issues"].as_array().unwrap();
    let paths: Vec<&str> = issues.iter().map(|i| i["path"].as_str().unwrap()).collect();
    assert_eq!(paths, ["main.model", "main.tools[0]"]);
    let message = body["error"]["message"].as_str().unwrap();
    assert!(message.contains("model `m` is not granted"), "{message}");
    assert!(
        issues[1]["message"].as_str().unwrap().contains(&format!(
            "POST /api/v0/system-principals/{id}/grants {{\"kind\": \"tool\", \"ref\": \"{TIME}\"}}"
        )),
        "{body}"
    );

    let (status, body) = fx
        .put_draft(&fx.alice, &id, json!({ "main": { "tool_resorces": {} } }))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["issues"][0]["path"], "main.tool_resorces");

    assert_eq!(
        fx.grant(&fx.alice, &id, "model", "m").await,
        StatusCode::CREATED
    );
    assert_eq!(
        fx.grant(&fx.alice, &id, "tool", TIME).await,
        StatusCode::CREATED
    );
    let (status, body) = fx.put_draft(&fx.alice, &id, spec("hi")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[tokio::test]
async fn a_grant_revoked_after_saving_blocks_publishing() {
    let fx = fixture().await;
    let id = fx.runnable("support").await;
    let (status, _) = fx
        .post(
            &fx.alice,
            &format!("/api/v0/system-principals/{id}/grants/revoke"),
            json!({ "kind": "tool", "ref": TIME }),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, body) = fx.publish(&fx.alice, &id).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["error"]["issues"][0]["path"], "main.tools[0]");
}

#[tokio::test]
async fn an_incomplete_draft_saves_but_does_not_publish() {
    let fx = fixture().await;
    let id = fx.create(&fx.alice, "support").await;
    let (status, body) = fx.publish(&fx.alice, &id).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .starts_with("cannot publish the agent: at `main`"),
        "{body}"
    );
}

#[tokio::test]
async fn editing_the_draft_never_changes_the_live_version() {
    let fx = fixture().await;
    let id = fx.runnable("support").await;
    let (status, body) = fx.publish(&fx.alice, &id).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["version"], 1);

    let (status, body) = fx.put_draft(&fx.alice, &id, spec("v2")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["live_version"], 1);

    let (_, detail) = fx.get(&fx.alice, &format!("/api/v0/agents/{id}")).await;
    assert_eq!(detail["agent"]["live_version"], 1);
    assert_eq!(
        detail["agent"]["live_spec"]["main"]["instructions"]["orchestration"],
        "v1"
    );
    assert_eq!(
        detail["agent"]["draft_spec"]["main"]["instructions"]["orchestration"],
        "v2"
    );
    let (_, versions) = fx
        .get(&fx.alice, &format!("/api/v0/agents/{id}/versions"))
        .await;
    assert_eq!(versions["versions"].as_array().unwrap().len(), 1);
    assert_eq!(
        versions["versions"][0]["spec"]["main"]["instructions"]["orchestration"],
        "v1"
    );
}

#[tokio::test]
async fn publishing_adds_versions_and_rollback_moves_the_live_pointer() {
    let fx = fixture().await;
    let id = fx.runnable("support").await;
    fx.publish(&fx.alice, &id).await;
    fx.put_draft(&fx.alice, &id, spec("v2")).await;
    let (_, body) = fx.publish(&fx.alice, &id).await;
    assert_eq!(body["version"], 2);

    let (status, body) = fx
        .post(
            &fx.alice,
            &format!("/api/v0/agents/{id}/live"),
            json!({ "version": 1 }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, detail) = fx.get(&fx.alice, &format!("/api/v0/agents/{id}")).await;
    assert_eq!(detail["agent"]["live_version"], 1);
    assert_eq!(
        detail["agent"]["live_spec"]["main"]["instructions"]["orchestration"],
        "v1"
    );

    let (_, versions) = fx
        .get(&fx.alice, &format!("/api/v0/agents/{id}/versions"))
        .await;
    assert_eq!(versions["live_version"], 1);
    let numbers: Vec<i64> = versions["versions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["version"].as_i64().unwrap())
        .collect();
    assert_eq!(numbers, [2, 1]);
    assert_eq!(versions["versions"][0]["published_by"], "alice");

    let (status, body) = fx
        .post(
            &fx.alice,
            &format!("/api/v0/agents/{id}/live"),
            json!({ "version": 9 }),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

#[tokio::test]
async fn a_share_is_refused_for_anyone_without_the_agent_management_permission() {
    let fx = fixture().await;
    let id = fx.create(&fx.alice, "support").await;
    let (status, body) = fx.share(&fx.alice, &id, "user", "plain", "read").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["error"]["code"], "share_needs_agent_manager");
    let (status, body) = fx.share(&fx.alice, &id, "group", "support", "read").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        fx.share(&fx.alice, &id, "user", "nobody", "read").await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        fx.share(&fx.alice, &id, "group", "nogroup", "read").await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        fx.share(&fx.alice, &id, "user", "bob", "admin").await.0,
        StatusCode::BAD_REQUEST
    );

    let (_, shares) = fx
        .get(&fx.alice, &format!("/api/v0/agents/{id}/shares"))
        .await;
    assert_eq!(shares["shares"].as_array().unwrap().len(), 1);
}

/// The share subject check asks the same resolver every access check asks,
/// so a group that exists only there — the bootstrap admin group — is a
/// valid subject, as a group from the database is.
#[tokio::test]
async fn a_share_with_the_bootstrap_admin_group_is_accepted() {
    let fx = fixture().await;
    let id = fx.create(&fx.alice, "support").await;
    let (status, body) = fx
        .share(&fx.alice, &id, "group", BOOTSTRAP_ADMIN_GROUP, "read")
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (status, body) = fx.share(&fx.alice, &id, "group", "admins", "read").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

async fn named(fx: &Fx, id: &str, name: &str) {
    let now = jiff::Timestamp::now();
    users::upsert(
        &fx.state.db,
        &users::User {
            id: id.into(),
            email: format!("{id}@example.com"),
            name: Some(name.into()),
            roles: vec![],
            created_at: now,
            updated_at: now,
            timezone: None,
            speech_voice: None,
        },
    )
    .await
    .unwrap();
}

/// Who a share may name is searched, never listed: a manager learns no more
/// of the people and groups here than what they type already matches.
#[tokio::test]
async fn share_subjects_are_searched_not_listed() {
    let fx = fixture().await;
    let id = fx.create(&fx.alice, "support").await;
    for n in 0..12 {
        named(&fx, &format!("sam{n}"), &format!("Sam {n}")).await;
    }
    let search = |who: &str, q: &str| {
        let uri = format!("/api/v0/agents/{id}/share-subjects?q={q}");
        let who = who.to_string();
        let fx = &fx;
        async move { fx.get(&who, &uri).await }
    };

    let (status, body) = fx.get(&fx.alice, "/api/v0/agent-resources").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.get("subjects").is_none(), "no roster in the resources");
    assert!(!body.to_string().contains("@example.com"), "{body}");

    let (status, short) = search(&fx.alice, "s").await;
    assert_eq!(status, StatusCode::OK, "{short}");
    assert_eq!(
        short,
        json!({ "users": [], "groups": [], "truncated": false }),
        "one character finds nothing"
    );

    let (_, found) = search(&fx.alice, "sam").await;
    let users = found["users"].as_array().unwrap();
    assert_eq!(users.len(), 8, "capped: {found}");
    assert!(users.iter().all(|u| u.get("email").is_none()), "{found}");
    assert_eq!(users[0]["name"], "Sam 0");
    assert!(!found.to_string().contains("manager"), "{found}");

    let (_, exact) = search(&fx.alice, "sam7@example.com").await;
    assert_eq!(
        exact["users"],
        json!([{ "id": "sam7", "name": "Sam 7", "email": "sam7@example.com" }])
    );

    let (_, groups) = search(&fx.alice, "man").await;
    assert_eq!(groups["groups"], json!(["managers"]));
    let (_, groups) = search(&fx.alice, "m").await;
    assert_eq!(groups["groups"], json!([]));

    let (status, _) = search(&fx.root, "sam").await;
    assert_eq!(status, StatusCode::OK, "an admin searches the same way");
    assert_eq!(
        search(&fx.bob, "sam").await.0,
        StatusCode::NOT_FOUND,
        "no share"
    );
    assert_eq!(
        search(&fx.plain, "sam").await.0,
        StatusCode::FORBIDDEN,
        "no manager"
    );
    fx.share(&fx.alice, &id, "user", "bob", "read").await;
    assert_eq!(
        search(&fx.bob, "sam").await.0,
        StatusCode::FORBIDDEN,
        "searching is for who may change the shares"
    );
}

/// A share subject is picked without knowing whether it may hold `read` or
/// `write`; the share route says so when it may not.
#[tokio::test]
async fn an_ineligible_read_share_is_refused_in_words_and_respond_is_offered() {
    let fx = fixture().await;
    let id = fx.create(&fx.alice, "support").await;
    named(&fx, "sam", "Sam").await;
    let (status, body) = fx.share(&fx.alice, &id, "user", "sam", "write").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["error"]["code"], "share_needs_agent_manager");
    let message = body["error"]["message"].as_str().unwrap();
    assert!(
        message.contains("agent-management permission") && message.contains("`respond`"),
        "{message}"
    );
    let (status, _) = fx.share(&fx.alice, &id, "user", "sam", "respond").await;
    assert_eq!(status, StatusCode::CREATED);
    let (_, shares) = fx
        .get(&fx.alice, &format!("/api/v0/agents/{id}/shares"))
        .await;
    let sam = shares["shares"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["subject_id"] == "sam")
        .unwrap()
        .clone();
    assert_eq!(sam["name"], "Sam", "a share names its person: {shares}");
}

/// A share outlives the permission it needs, but grants nothing without it.
#[tokio::test]
async fn a_manager_who_loses_the_permission_loses_the_agent_despite_the_share() {
    let fx = fixture().await;
    let id = fx.create(&fx.alice, "support").await;
    gateway_groups::set_can_manage_agents(&fx.state.db, "managers", false)
        .await
        .unwrap();
    fx.state.reload_rbac().await;
    assert_eq!(
        aiplane_runtime::agents::access::effective_access(&fx.state, &id, "alice")
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        fx.get(&fx.alice, &format!("/api/v0/agents/{id}")).await.0,
        StatusCode::FORBIDDEN
    );

    let (status, body) = fx
        .share(&fx.root, &id, "group", "managers", "respond")
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(
        aiplane_runtime::agents::access::effective_access(&fx.state, &id, "alice")
            .await
            .unwrap(),
        Some(aiplane_agents::db::agents::Access::Respond),
        "only an explicit `respond` share outlives the permission, not the `write` one"
    );
}

#[tokio::test]
async fn a_read_share_shows_the_agent_but_does_not_allow_changing_it() {
    let fx = fixture().await;
    let id = fx.runnable("support").await;
    let (status, body) = fx.share(&fx.alice, &id, "user", "bob", "read").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let (status, detail) = fx.get(&fx.bob, &format!("/api/v0/agents/{id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["agent"]["access"], "read");
    assert_eq!(
        fx.get(&fx.bob, &format!("/api/v0/system-principals/{id}"))
            .await
            .0,
        StatusCode::OK
    );

    let (status, body) = fx.put_draft(&fx.bob, &id, spec("bob was here")).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "agent_write_required");
    assert_eq!(fx.publish(&fx.bob, &id).await.0, StatusCode::FORBIDDEN);
    assert_eq!(
        fx.share(&fx.bob, &id, "user", "bob", "write").await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        fx.grant(&fx.bob, &id, "model", "m").await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        fx.send(
            Some(&fx.bob),
            Method::DELETE,
            &format!("/api/v0/agents/{id}"),
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );

    let (status, _) = fx
        .post(
            &fx.alice,
            &format!("/api/v0/agents/{id}/shares/revoke"),
            json!({ "subject_kind": "user", "subject_id": "bob" }),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        fx.get(&fx.bob, &format!("/api/v0/agents/{id}")).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn a_group_share_reaches_every_manager_in_the_group() {
    let fx = fixture().await;
    let id = fx.create(&fx.alice, "support").await;
    let (status, body) = fx.share(&fx.alice, &id, "group", "managers", "write").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (_, list) = fx.get(&fx.bob, "/api/v0/agents").await;
    assert_eq!(list["agents"][0]["access"], "write");
    let (status, body) = fx
        .put_draft(&fx.bob, &id, json!({ "profile": { "display": "Bob" } }))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[tokio::test]
async fn the_last_write_share_cannot_be_removed() {
    let fx = fixture().await;
    let id = fx.create(&fx.alice, "support").await;
    let (status, body) = fx
        .post(
            &fx.alice,
            &format!("/api/v0/agents/{id}/shares/revoke"),
            json!({ "subject_kind": "user", "subject_id": "alice" }),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"]["code"], "last_writer");
    let (status, _) = fx.share(&fx.alice, &id, "user", "alice", "read").await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn a_route_to_a_missing_sub_agent_is_rejected_and_an_existing_one_accepted() {
    let fx = fixture().await;
    let main = fx.create(&fx.alice, "support").await;
    let sub = fx.create(&fx.alice, "billing").await;
    let draft = |agent: &str| {
        json!({
            "state": { "issue": { "type": "string", "set_by": ["llm"] } },
            "routes": { "billing": {
                "when": { "slot": "issue", "eq": "billing" },
                "agent": agent,
                "task": "{issue}"
            } }
        })
    };
    let (status, body) = fx.put_draft(&fx.alice, &main, draft("billing")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["error"]["issues"][0]["path"], "routes.billing.agent");
    let (status, body) = fx.put_draft(&fx.alice, &main, draft(&sub)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[tokio::test]
async fn deleting_an_agent_removes_it_and_its_principal_but_not_the_audit_trail() {
    let fx = fixture().await;
    let id = fx.runnable("support").await;
    let (status, _) = fx
        .send(
            Some(&fx.alice),
            Method::DELETE,
            &format!("/api/v0/agents/{id}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        fx.get(&fx.alice, &format!("/api/v0/agents/{id}")).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        fx.get(&fx.alice, &format!("/api/v0/system-principals/{id}"))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(fx.audit_kinds(&id).await.last().unwrap(), "agent_deleted");
}

#[tokio::test]
async fn every_mutation_writes_an_audit_row_with_its_actor() {
    let fx = fixture().await;
    let id = fx.runnable("support").await;
    fx.publish(&fx.alice, &id).await;
    fx.post(
        &fx.alice,
        &format!("/api/v0/agents/{id}/live"),
        json!({ "version": 1 }),
    )
    .await;
    fx.share(&fx.alice, &id, "user", "bob", "write").await;
    fx.post(
        &fx.bob,
        &format!("/api/v0/agents/{id}/shares/revoke"),
        json!({ "subject_kind": "user", "subject_id": "alice" }),
    )
    .await;
    assert_eq!(
        fx.audit_kinds(&id).await,
        [
            "principal_created",
            "agent_created",
            "grant_added",
            "grant_added",
            "agent_draft_updated",
            "agent_published",
            "agent_live_version_set",
            "agent_share_set",
            "agent_share_removed",
        ]
    );
    let (_, detail) = fx.get(&fx.bob, &format!("/api/v0/agents/{id}")).await;
    let audit = detail["agent"]["audit"].as_array().unwrap();
    assert_eq!(audit[0]["kind"], "agent_share_removed");
    assert_eq!(audit[0]["actor_id"], "bob");
    assert_eq!(audit[0]["detail"]["subject_id"], "alice");
}

#[tokio::test]
async fn an_admin_holds_write_on_every_agent_without_a_share() {
    let fx = fixture().await;
    let id = fx.runnable("support").await;

    let (_, list) = fx.get(&fx.root, "/api/v0/agents?scope=all").await;
    assert_eq!(list["agents"][0]["id"], id.as_str());
    assert_eq!(list["agents"][0]["access"], "write");
    let (status, detail) = fx.get(&fx.root, &format!("/api/v0/agents/{id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["agent"]["access"], "write");

    let (status, body) = fx.put_draft(&fx.root, &id, spec("rescued")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(fx.publish(&fx.root, &id).await.0, StatusCode::CREATED);
    let (status, body) = fx.share(&fx.root, &id, "user", "bob", "write").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(
        fx.grant(&fx.root, &id, "model", "m").await,
        StatusCode::OK,
        "held already; an admin's regrant widens it to every pool"
    );
    let (status, _) = fx
        .send(
            Some(&fx.root),
            Method::DELETE,
            &format!("/api/v0/agents/{id}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

fn listed<'a>(list: &'a Value, id: &str) -> Option<&'a Value> {
    list["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|agent| agent["id"] == id)
}

#[tokio::test]
async fn an_admins_workspace_lists_their_own_agents_and_every_users_only_on_request() {
    let fx = fixture().await;
    let public = fx.create(&fx.alice, "support").await;
    let (status, body) = fx
        .post(&fx.alice, "/api/v0/dots", json!({ "name": "Harald" }))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let personal = body["dot"]["id"].as_str().unwrap().to_string();
    let own = fx.create(&fx.root, "root-helper").await;

    let (status, list) = fx.get(&fx.root, "/api/v0/agents").await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(list["scope"], "self");
    assert_eq!(list["can_view_all"], true);
    let ids = list["agents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|agent| agent["id"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        [own.as_str()],
        "another user's agents stay out of the workspace"
    );

    let (status, all) = fx.get(&fx.root, "/api/v0/agents?scope=all").await;
    assert_eq!(status, StatusCode::OK, "{all}");
    assert_eq!(all["scope"], "all");
    let dot = listed(&all, &personal).expect("every user's personal agent");
    assert_eq!(dot["kind"], "personal");
    assert_eq!(dot["access"], "write");
    assert_eq!(
        dot["owner"],
        json!({ "kind": "user", "id": "alice", "name": null, "email": "alice@example.com" })
    );
    assert_eq!(dot["dot"], json!({ "name": "Harald", "active": true }));
    let agent = listed(&all, &public).expect("every user's public agent");
    assert_eq!(agent["kind"], "public");
    assert_eq!(agent["owner"]["id"], "alice");
}

#[tokio::test]
async fn a_non_admin_asking_for_every_users_agents_gets_their_own() {
    let fx = fixture().await;
    fx.create(&fx.alice, "support").await;
    let own = fx.create(&fx.bob, "bobs").await;

    let (status, list) = fx.get(&fx.bob, "/api/v0/agents?scope=all").await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(list["scope"], "self");
    assert_eq!(list["can_view_all"], false);
    assert_eq!(list["agents"].as_array().unwrap().len(), 1);
    assert_eq!(list["agents"][0]["id"], own.as_str());
    assert_eq!(list["agents"][0]["kind"], "public");
}

#[tokio::test]
async fn an_agent_shared_with_you_names_who_shared_it_and_when() {
    let fx = fixture().await;
    let id = fx.create(&fx.alice, "support").await;
    let before = jiff::Timestamp::now();
    let (status, body) = fx.share(&fx.alice, &id, "user", "bob", "read").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let (_, bobs) = fx.get(&fx.bob, "/api/v0/agents").await;
    let agent = listed(&bobs, &id).expect("the shared agent");
    assert_eq!(
        agent["shared_by"]["by"],
        json!({ "kind": "user", "id": "alice", "name": null, "email": "alice@example.com" })
    );
    let at: jiff::Timestamp = agent["shared_by"]["at"].as_str().unwrap().parse().unwrap();
    assert!(at >= before, "{at} is when the share was given");

    let (_, alices) = fx.get(&fx.alice, "/api/v0/agents").await;
    assert_eq!(
        listed(&alices, &id).unwrap()["shared_by"],
        Value::Null,
        "the creator's own share names no one"
    );

    let (status, shares) = fx
        .get(&fx.alice, &format!("/api/v0/agents/{id}/shares"))
        .await;
    assert_eq!(status, StatusCode::OK, "{shares}");
    let bob = shares["shares"]
        .as_array()
        .unwrap()
        .iter()
        .find(|share| share["subject_id"] == "bob")
        .unwrap();
    assert_eq!(bob["granted"]["by"]["id"], "alice");
    let creator = shares["shares"]
        .as_array()
        .unwrap()
        .iter()
        .find(|share| share["subject_id"] == "alice")
        .unwrap();
    assert_eq!(creator["granted"]["by"]["id"], "alice");

    let (status, body) = fx.share(&fx.root, &id, "user", "bob", "write").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (_, bobs) = fx.get(&fx.bob, "/api/v0/agents").await;
    assert_eq!(
        listed(&bobs, &id).unwrap()["shared_by"]["by"]["id"],
        "root",
        "changing a share makes its changer the one who gave it"
    );
}

#[tokio::test]
async fn a_personal_agent_answers_only_its_owner_and_an_admin() {
    let fx = fixture().await;
    let (status, body) = fx
        .post(&fx.alice, "/api/v0/dots", json!({ "name": "Harald" }))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let personal = body["dot"]["id"].as_str().unwrap().to_string();

    let (status, body) = fx.share(&fx.root, &personal, "user", "bob", "write").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["error"]["code"], "personal_agent_not_shareable");

    // A share written before personal agents became unshareable must not
    // open the agent to its holder.
    aiplane_agents::db::agents::set_share(
        &fx.state.db,
        &personal,
        aiplane_agents::db::agents::SubjectKind::User,
        "bob",
        aiplane_agents::db::agents::Access::Write,
        "root",
    )
    .await
    .unwrap();
    let (status, body) = fx.get(&fx.bob, &format!("/api/v0/agents/{personal}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert!(listed(&fx.get(&fx.bob, "/api/v0/agents").await.1, &personal).is_none());

    let (status, body) = fx
        .get(&fx.root, &format!("/api/v0/agents/{personal}"))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["agent"]["access"], "write");
    assert_eq!(body["agent"]["kind"], "personal");
}

#[tokio::test]
async fn admin_access_does_not_extend_to_a_manager_without_a_share() {
    let fx = fixture().await;
    let id = fx.runnable("support").await;
    fx.get(&fx.root, &format!("/api/v0/agents/{id}")).await;
    assert_eq!(
        fx.get(&fx.bob, "/api/v0/agents").await.1["agents"],
        json!([])
    );
    assert_eq!(
        fx.get(&fx.bob, &format!("/api/v0/agents/{id}")).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        fx.put_draft(&fx.bob, &id, spec("nope")).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn an_a2a_route_saves_once_granted_with_its_credential_sealed() {
    const CARD: &str = "https://partner.example.com/.well-known/agent-card.json";
    const TOKEN: &str = "partner-token-that-must-not-be-stored";
    let fx = fixture().await;
    let id = fx.create(&fx.alice, "support").await;
    let spec = json!({
        "state": { "issue": { "type": "string", "set_by": ["llm"] } },
        "routes": { "partner": {
            "when": { "slot": "issue", "set": true },
            "task": "Warranty question: {issue}",
            "a2a": {
                "card_url": CARD,
                "auth": { "kind": "bearer", "token": TOKEN },
                "finish": { "schema": { "type": "object", "required": ["answer"],
                                        "properties": { "answer": { "type": "string" } } } }
            }
        } }
    });
    let (status, body) = fx.put_draft(&fx.alice, &id, spec.clone()).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        body["error"]["issues"][0]["path"],
        "routes.partner.a2a.card_url"
    );
    assert_eq!(
        fx.grant(&fx.alice, &id, "a2a_agent", CARD).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        fx.grant(&fx.root, &id, "a2a_agent", CARD).await,
        StatusCode::CREATED
    );

    let (status, body) = fx.put_draft(&fx.alice, &id, spec).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let auth = &body["draft_spec"]["routes"]["partner"]["a2a"]["auth"];
    assert!(auth.get("token").is_none(), "{auth}");
    assert!(auth["token_sealed"].is_string(), "{auth}");
    let (_, read) = fx.get(&fx.alice, &format!("/api/v0/agents/{id}")).await;
    assert!(!read.to_string().contains(TOKEN), "{read}");

    let (status, body) = fx
        .put_draft(&fx.alice, &id, read["agent"]["draft_spec"].clone())
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a GET → PUT round trip keeps the sealed token: {body}"
    );
}

#[tokio::test]
async fn a_loop_route_saves_from_the_json_tab_and_names_its_problems() {
    let fx = fixture().await;
    let main = fx.create(&fx.alice, "support").await;
    let writer = fx.create(&fx.alice, "writer").await;
    let reviewer = fx.create(&fx.alice, "reviewer").await;
    let spec = |worker: &str, critic: &str, max: u64| {
        json!({
            "state": { "issue": { "type": "string", "set_by": ["llm"] } },
            "routes": { "offer": {
                "when": { "slot": "issue", "set": true },
                "task": "Write an offer for: {issue}",
                "loop": { "worker": worker, "critic": critic, "max_iterations": max }
            } }
        })
    };
    let (status, body) = fx
        .put_draft(&fx.alice, &main, spec(&writer, &reviewer, 3))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["draft_spec"]["routes"]["offer"]["loop"]["worker"],
        writer
    );

    let (status, body) = fx.put_draft(&fx.alice, &main, spec(&main, &main, 11)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let paths: Vec<&str> = body["error"]["issues"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["path"].as_str().unwrap())
        .collect();
    assert_eq!(
        paths,
        [
            "routes.offer.loop.worker",
            "routes.offer.loop.critic",
            "routes.offer.loop.critic",
            "routes.offer.loop.max_iterations"
        ]
    );
}

/// The sidebar shows the inbox only to whoever something can arrive for: an
/// item waits, or they answer for a published agent. `answers` is that
/// second half, from the same standing rule the inbox lists items by.
#[tokio::test]
async fn the_inbox_says_whether_its_viewer_answers_for_a_published_agent() {
    let fx = fixture().await;
    let answers = |body: &Value| body["answers"].as_bool();
    let id = fx.runnable("helper").await;
    for who in [&fx.root, &fx.alice, &fx.plain] {
        let (status, body) = fx.get(who, "/api/v0/agents/inbox").await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            answers(&body),
            Some(false),
            "nothing is published yet: {body}"
        );
    }

    let (status, body) = fx.publish(&fx.alice, &id).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(
        answers(&fx.get(&fx.root, "/api/v0/agents/inbox").await.1),
        Some(true)
    );
    assert_eq!(
        answers(&fx.get(&fx.alice, "/api/v0/agents/inbox").await.1),
        Some(true)
    );
    assert_eq!(
        answers(&fx.get(&fx.plain, "/api/v0/agents/inbox").await.1),
        Some(false)
    );

    let (status, body) = fx.share(&fx.alice, &id, "user", "plain", "respond").await;
    assert!(status.is_success(), "{body}");
    assert_eq!(
        answers(&fx.get(&fx.plain, "/api/v0/agents/inbox").await.1),
        Some(true)
    );
}

#[tokio::test]
async fn personal_dots_are_owner_scoped_and_deactivation_frees_capacity() {
    let fx = fixture().await;
    let (status, body) = fx.get(&fx.plain, "/api/v0/dots").await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    let mut ids = Vec::new();
    for n in 0..4 {
        let (status, body) = fx
            .post(
                &fx.alice,
                "/api/v0/dots",
                json!({ "name": format!("alice-dot-{n}") }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        ids.push(body["dot"]["id"].as_str().unwrap().to_string());
    }
    let (status, body) = fx.get(&fx.alice, "/api/v0/dots").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["active_limit"], 4);
    assert_eq!(body["dots"].as_array().unwrap().len(), 4);
    assert!(
        fx.get(&fx.bob, "/api/v0/dots").await.1["dots"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let (status, body) = fx
        .post(&fx.bob, "/api/v0/dots", json!({ "name": "alice-dot-0" }))
        .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "same name is scoped to its owner: {body}"
    );

    let (status, body) = fx
        .post(
            &fx.alice,
            "/api/v0/dots",
            json!({ "name": "alice-dot-over-limit" }),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"]["code"], "active_dot_limit");

    let (status, body) = fx
        .send(
            Some(&fx.alice),
            Method::PUT,
            &format!("/api/v0/dots/{}/active", ids[0]),
            Some(json!({ "active": false })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = fx
        .post(
            &fx.alice,
            "/api/v0/dots",
            json!({ "name": "alice-dot-replacement" }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let (status, body) = fx
        .send(
            Some(&fx.bob),
            Method::PUT,
            &format!("/api/v0/dots/{}/active", ids[0]),
            Some(json!({ "active": false })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

#[tokio::test]
async fn personal_dot_public_name_is_not_limited_to_a_principal_slug() {
    let fx = fixture().await;
    let (status, body) = fx
        .post(
            &fx.alice,
            "/api/v0/dots",
            json!({ "name": "Harald", "description": "My personal assistant" }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["dot"]["name"], "Harald");
    assert_eq!(body["dot"]["display"], "Harald");
    assert_eq!(body["dot"]["description"], "My personal assistant");

    let dot_id = body["dot"]["id"].as_str().unwrap();
    let (status, agent_list) = fx.get(&fx.alice, "/api/v0/agents").await;
    assert_eq!(status, StatusCode::OK, "{agent_list}");
    assert!(
        !agent_list["agents"]
            .as_array()
            .unwrap()
            .iter()
            .any(|agent| agent["id"] == dot_id)
    );
    let (status, body) = fx.get(&fx.alice, &format!("/api/v0/agents/{dot_id}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

#[tokio::test]
async fn personal_dot_memories_are_persistent_and_owner_scoped() {
    let fx = fixture().await;
    let (status, body) = fx
        .post(&fx.alice, "/api/v0/dots", json!({ "name": "memory-dot" }))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let dot_id = body["dot"]["id"].as_str().unwrap();
    let memory_path = format!("/api/v0/dots/{dot_id}/memories");
    let (status, body) = fx
        .post(
            &fx.alice,
            &memory_path,
            json!({ "scope":"dot", "kind": "fact", "content": "Avoid this recurring pitfall" }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["kind"], "fact");
    assert_eq!(body["scope"], "dot");
    let memory_id = body["id"].as_str().unwrap();

    let (status, body) = fx.get(&fx.bob, &memory_path).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (status, body) = fx.get(&fx.alice, &memory_path).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["scopes"],
        json!([{ "scope": "dot", "name": "memory-dot", "writable": true }])
    );
    assert_eq!(body["memories"][0]["scope"], "dot");
    assert_eq!(
        body["memories"][0]["content"],
        "Avoid this recurring pitfall"
    );

    let memory_item_path = format!("{memory_path}/{memory_id}");
    let (status, body) = fx
        .send(
            Some(&fx.bob),
            Method::PUT,
            &memory_item_path,
            Some(json!({ "scope":"dot", "kind": "fact", "content": "unauthorized" })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (status, body) = fx
        .send(
            Some(&fx.alice),
            Method::PUT,
            &memory_item_path,
            Some(json!({ "scope":"dot", "kind": "project", "content": "Keep this lesson" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["content"], "Keep this lesson");
    let (status, body) = fx
        .send(
            Some(&fx.alice),
            Method::DELETE,
            &format!("{memory_item_path}?scope=dot"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
}

#[tokio::test]
async fn personal_dot_chat_sessions_are_private_and_remain_dot_owned_when_deactivated() {
    let fx = fixture().await;
    let (status, body) = fx
        .post(&fx.alice, "/api/v0/dots", json!({ "name": "chat-dot" }))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let dot_id = body["dot"]["id"].as_str().unwrap();

    let (status, body) = fx
        .post(
            &fx.alice,
            &format!("/api/v0/dots/{dot_id}/sessions"),
            json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let session_id = body["session"]["id"].as_str().unwrap();
    assert_eq!(body["session"]["user_id"], "alice");
    assert_eq!(body["session"]["title"], "chat-dot");
    let (status, activity) = fx
        .get(&fx.alice, &format!("/api/v0/dots/{dot_id}/activity"))
        .await;
    assert_eq!(status, StatusCode::OK, "{activity}");
    let session_event = activity["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["kind"] == "personal_dot_session_started")
        .expect("starting a Dot chat is recorded in Dot activity");
    assert_eq!(session_event["principal_id"], dot_id);
    assert_eq!(session_event["actor_id"], "alice");
    assert_eq!(session_event["session_id"], session_id);
    assert_eq!(
        aiplane_agents::db::dot_sessions::for_session(&fx.state.db, "alice", session_id)
            .await
            .unwrap()
            .unwrap()
            .dot_id,
        dot_id
    );
    assert_eq!(
        fx.get(&fx.bob, &format!("/api/v0/chat/sessions/{session_id}"))
            .await
            .0,
        StatusCode::NOT_FOUND
    );

    fx.send(
        Some(&fx.alice),
        Method::PUT,
        &format!("/api/v0/dots/{dot_id}/active"),
        Some(json!({ "active": false })),
    )
    .await;
    assert!(
        !aiplane_agents::db::dot_sessions::for_session(&fx.state.db, "alice", session_id)
            .await
            .unwrap()
            .unwrap()
            .active
    );
    assert_eq!(
        fx.post(
            &fx.alice,
            &format!("/api/v0/dots/{dot_id}/sessions"),
            json!({})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let (status, body) = fx
        .post(
            &fx.alice,
            &format!("/api/v0/chat/sessions/{session_id}/messages"),
            json!({ "model": "model-a", "message": "Continue" }),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}

#[tokio::test]
async fn personal_dot_activity_is_available_only_to_its_owner() {
    let fx = fixture().await;
    let (status, created) = fx
        .post(&fx.alice, "/api/v0/dots", json!({ "name": "activity-dot" }))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let dot_id = created["dot"]["id"].as_str().unwrap();
    let path = format!("/api/v0/dots/{dot_id}/activity");

    let (status, denied) = fx.get(&fx.bob, &path).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{denied}");
    let (status, body) = fx.get(&fx.alice, &path).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let events = body["events"].as_array().unwrap();
    assert!(!events.is_empty());
    assert!(events.iter().any(|event| event["kind"] == "agent_created"));
}

#[tokio::test]
async fn dot_management_permission_is_independent_and_live_group_updates_control_lifecycle() {
    let fx = fixture().await;
    let (status, me) = fx.get(&fx.plain, "/api/v0/me").await;
    assert_eq!(status, StatusCode::OK, "{me}");
    assert_eq!(me["can_manage_dots"], false);
    assert_eq!(me["can_manage_agents"], false);
    let (status, denied) = fx
        .post(&fx.plain, "/api/v0/dots", json!({"name":"Denied"}))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{denied}");
    assert_eq!(
        denied["error"]["message"],
        session_core::i18n::t(
            session_core::i18n::Lang::En,
            "dot-error-permission-required"
        )
    );
    for allowed in [true, false] {
        let (status,saved)=fx.send(Some(&fx.root),Method::PUT,"/api/v0/admin/groups",Some(json!({"name":"everyone","is_default":true,"can_manage_dots":allowed,"tools":[TIME]}))).await;
        assert_eq!(status, StatusCode::OK, "{saved}");
        let (_, me) = fx.get(&fx.plain, "/api/v0/me").await;
        assert_eq!(me["can_manage_dots"], allowed);
        assert_eq!(me["can_manage_agents"], false);
        let expected = if allowed {
            StatusCode::OK
        } else {
            StatusCode::FORBIDDEN
        };
        assert_eq!(fx.get(&fx.plain, "/api/v0/dots").await.0, expected);
        assert_eq!(
            fx.post(
                &fx.plain,
                "/api/v0/agents",
                json!({"name":"forbidden-principal"})
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
        if allowed {
            let (status, created) = fx
                .post(&fx.plain, "/api/v0/dots", json!({"name":"Personal only"}))
                .await;
            assert_eq!(status, StatusCode::CREATED, "{created}");
            let id = created["dot"]["id"].as_str().unwrap();
            let url = format!("/api/v0/dots/{id}/active");
            assert_eq!(
                fx.send(
                    Some(&fx.plain),
                    Method::PUT,
                    &url,
                    Some(json!({"active":false}))
                )
                .await
                .0,
                StatusCode::OK
            );
            assert_eq!(
                fx.send(
                    Some(&fx.plain),
                    Method::PUT,
                    &url,
                    Some(json!({"active":true}))
                )
                .await
                .0,
                StatusCode::OK
            );
            assert_eq!(
                fx.send(
                    Some(&fx.bob),
                    Method::PUT,
                    &url,
                    Some(json!({"active":false}))
                )
                .await
                .0,
                StatusCode::NOT_FOUND
            );
        } else {
            let owned = aiplane_agents::db::agents::list_personal_dots(&fx.state.db, "plain")
                .await
                .unwrap();
            let id = &owned[0].agent.principal.id;
            assert!(owned[0].active);
            let access = format!("/api/v0/dots/{id}/access");
            assert_eq!(fx.get(&fx.plain, &access).await.0, StatusCode::OK);
            assert_eq!(
                fx.send(
                    Some(&fx.plain),
                    Method::PUT,
                    &format!("/api/v0/dots/{id}/active"),
                    Some(json!({"active":false}))
                )
                .await
                .0,
                StatusCode::FORBIDDEN
            );
        }
    }
    let (_, me) = fx.get(&fx.root, "/api/v0/me").await;
    assert_eq!(me["can_manage_dots"], true);
    assert_eq!(me["can_manage_agents"], true);
}

#[tokio::test]
async fn agent_management_does_not_imply_dot_management() {
    let fx = fixture().await;
    let (status,_)=fx.send(Some(&fx.root),Method::PUT,"/api/v0/admin/groups",Some(json!({"name":"managers","oidc_values":["managers"],"can_manage_agents":true,"can_manage_dots":false}))).await;
    assert_eq!(status, StatusCode::OK);
    let (_, me) = fx.get(&fx.alice, "/api/v0/me").await;
    assert_eq!(me["can_manage_agents"], true);
    assert_eq!(me["can_manage_dots"], false);
    assert_eq!(
        fx.post(
            &fx.alice,
            "/api/v0/dots",
            json!({"name":"No implicit permission"})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
}
