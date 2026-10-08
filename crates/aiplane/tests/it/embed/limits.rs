// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Leaving a public agent running safely (`docs/agent-visitors.md` → "Rates"): per-visitor and per-IP rate limits, the owner's
//! monthly budget, and the retention sweep, on the embed fixture.

use std::time::Duration;

use rama::Service;
use rama::extensions::ExtensionsRef;
use rama::http::{Body, Method, Request, StatusCode, header};
use serde_json::{Value, json};

use super::{Embed, Reply, SITE, ScriptedRunner, code, embed_with};
use crate::common;

use aiplane_agents::db::agent_audit;
use aiplane_agents::db::run_sessions;
use aiplane_core::server::db::usage::{self, UsageKind, UsageRecord, UsageSource};
use aiplane_core::server::principal::PrincipalKind;
use aiplane_runtime::agents::retention;
use session_core::db as chat;
use session_core::i18n::{self, Lang};

/// The reverse proxy the embed fixture sits behind; `embed_with` trusts it.
const PROXY: &str = "10.0.0.1";

impl Embed {
    /// A POST to `uri` from [`SITE`], with the visitor token if any, a
    /// forwarded client IP and an `Accept-Language`.
    async fn post_as(
        &self,
        uri: &str,
        bearer: Option<&str>,
        ip: &str,
        lang: &str,
        body: Value,
    ) -> Reply {
        self.post_via(PROXY, uri, bearer, ip, lang, body).await
    }

    /// The same request as seen arriving from TCP peer `peer`, with `ip` in
    /// `X-Forwarded-For`. The fixture trusts [`PROXY`] only.
    async fn post_via(
        &self,
        peer: &str,
        uri: &str,
        bearer: Option<&str>,
        ip: &str,
        lang: &str,
        body: Value,
    ) -> Reply {
        let mut req = Request::builder()
            .method(Method::POST)
            .uri(uri)
            .header(header::ORIGIN, SITE)
            .header("x-forwarded-for", ip)
            .header(header::ACCEPT_LANGUAGE, lang)
            .header("content-type", "application/json");
        if let Some(b) = bearer {
            req = req.header(header::AUTHORIZATION, format!("Bearer {b}"));
        }
        let req = req.body(Body::from(body.to_string())).unwrap();
        req.extensions().insert(rama::net::stream::SocketInfo::new(
            None,
            rama::net::address::SocketAddress::new(peer.parse::<std::net::IpAddr>().unwrap(), 4000),
        ));
        let resp = common::app(self.fx.state.clone()).serve(req).await.unwrap();
        let status = resp.status();
        let headers = resp.headers().clone();
        let bytes = common::read_body(resp).await;
        Reply {
            status,
            headers,
            body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        }
    }

    async fn start_from(&self, ip: &str) -> Reply {
        self.post_as(
            "/api/v0/embed/sessions",
            None,
            ip,
            "en",
            json!({ "key": self.key }),
        )
        .await
    }

    /// Send a message and wait until its turn is done and released, so the
    /// next message is judged by the limits alone.
    async fn say_from(&self, token: &str, ip: &str, lang: &str) -> Reply {
        let r = self
            .post_as(
                "/api/v0/embed/messages",
                Some(token),
                ip,
                lang,
                json!({ "text": "hello" }),
            )
            .await;
        if r.status == StatusCode::ACCEPTED {
            let session = self.conversation_of(token).await;
            self.wait_terminal(&session, r.body["turn_id"].as_str().unwrap())
                .await;
            while self.fx.state.chats.get(&self.agent, &session).is_some() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
        r
    }

    async fn refusals(&self) -> Vec<Value> {
        agent_audit::for_principal(&self.fx.state.db, &self.agent)
            .await
            .unwrap()
            .into_iter()
            .filter(|e| e.kind == "limit_refused")
            .map(|e| e.detail)
            .collect()
    }
}

async fn limited(publish: Value) -> Embed {
    embed_with(
        Some(ScriptedRunner::answering()),
        Some(json!({ "publish": publish })),
    )
    .await
}

fn token_of(r: &Reply) -> String {
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    r.body["token"].as_str().unwrap().to_string()
}

fn retry_after(r: &Reply) -> i64 {
    r.headers
        .get(header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| panic!("no Retry-After on {}", r.body))
}

#[tokio::test]
async fn a_visitor_over_its_message_rate_is_told_to_wait_in_its_language() {
    let e = limited(json!({ "rate_limits": { "visitor": { "max": 2, "per": "10m" } } })).await;
    let token = token_of(&e.start_from("203.0.113.5").await);
    for _ in 0..2 {
        let r = e.say_from(&token, "203.0.113.5", "en").await;
        assert_eq!(r.status, StatusCode::ACCEPTED, "{}", r.body);
    }

    let r = e.say_from(&token, "203.0.113.5", "de-DE,de;q=0.9").await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS, "{}", r.body);
    assert_eq!(code(&r), "visitor_rate_limited");
    let wait = retry_after(&r);
    assert!((1..=600).contains(&wait), "{wait}");
    let expected = i18n::t_args(
        Lang::De,
        "agent-embed-rate-limited",
        &i18n::args([("seconds", wait.into())]),
    );
    assert_eq!(r.body["error"]["message"], expected.as_str());

    let session = e.conversation_of(&token).await;
    let turns = chat::list_turns(&e.fx.state.db, &session).await.unwrap();
    assert_eq!(turns.len(), 4, "the refused message was not stored");

    let other = token_of(&e.start_from("203.0.113.5").await);
    assert_eq!(
        e.say_from(&other, "203.0.113.5", "en").await.status,
        StatusCode::ACCEPTED,
        "the limit is per visitor session"
    );

    let refusals = e.refusals().await;
    assert_eq!(refusals.len(), 1, "{refusals:?}");
    assert_eq!(refusals[0]["limit"], "visitor_rate");
    assert_eq!(refusals[0]["max"], 2);
    assert_eq!(refusals[0]["per_secs"], 600);
}

#[tokio::test]
async fn one_ip_cannot_dodge_the_visitor_limit_by_starting_new_conversations() {
    let e = limited(json!({ "rate_limits": { "ip": { "max": 3, "per": "1h" } } })).await;
    let first = token_of(&e.start_from("198.51.100.20").await);
    assert_eq!(
        e.say_from(&first, "198.51.100.20", "en").await.status,
        StatusCode::ACCEPTED
    );
    token_of(&e.start_from("198.51.100.20").await);

    let r = e.start_from("198.51.100.20").await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS, "{}", r.body);
    assert_eq!(code(&r), "visitor_rate_limited");
    assert!(
        retry_after(&r) > 3_000,
        "the first event leaves after an hour"
    );
    let r = e.say_from(&first, "198.51.100.20", "en").await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS, "{}", r.body);

    token_of(&e.start_from("198.51.100.21").await);
    let refusals = e.refusals().await;
    assert_eq!(refusals.len(), 2);
    assert!(
        refusals.iter().all(|d| d["limit"] == "ip_rate"),
        "{refusals:?}"
    );
    assert!(
        !refusals[0].to_string().contains("198.51.100"),
        "the audit row does not keep the IP: {}",
        refusals[0]
    );
}

#[tokio::test]
async fn a_spoofed_forwarded_header_cannot_dodge_the_ip_limit() {
    let e = limited(json!({ "rate_limits": { "ip": { "max": 3, "per": "1h" } } })).await;
    let attacker = "198.51.100.50";
    let e = &e;
    let start = |forged: String| async move {
        e.post_via(
            attacker,
            "/api/v0/embed/sessions",
            None,
            &forged,
            "en",
            json!({ "key": e.key }),
        )
        .await
    };
    for n in 0..3 {
        assert_eq!(
            start(format!("203.0.113.{n}")).await.status,
            StatusCode::CREATED
        );
    }
    let r = start("203.0.113.99".into()).await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS, "{}", r.body);
    assert_eq!(code(&r), "visitor_rate_limited");

    // A genuine visitor reaching the trusted proxy is counted on their own.
    token_of(&e.start_from("192.0.2.77").await);
}

#[tokio::test]
async fn visitors_behind_the_trusted_proxy_are_limited_by_their_forwarded_address() {
    let e = limited(json!({ "rate_limits": { "ip": { "max": 1, "per": "1h" } } })).await;
    token_of(&e.start_from("192.0.2.1").await);
    let r = e.start_from("192.0.2.1").await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS, "{}", r.body);
    token_of(&e.start_from("192.0.2.2").await);
}

#[tokio::test]
async fn without_rate_settings_the_defaults_apply() {
    let e = limited(json!({})).await;
    let token = token_of(&e.start_from("192.0.2.44").await);
    for _ in 0..20 {
        let r = e.say_from(&token, "192.0.2.44", "en").await;
        assert_eq!(r.status, StatusCode::ACCEPTED, "{}", r.body);
    }
    let r = e.say_from(&token, "192.0.2.44", "en").await;
    assert_eq!(
        r.status,
        StatusCode::TOO_MANY_REQUESTS,
        "20 messages per 10 minutes"
    );
}

/// What the agent's conversations spent: one sub-agent call, which names
/// the sub-agent as the caller and still counts for the main agent.
async fn spend(e: &Embed, tokens: i64) {
    usage::insert_batch(
        &e.fx.state.db,
        &[UsageRecord {
            created_at: jiff::Timestamp::now(),
            user_id: "some-sub-agent".into(),
            user_email: Some("billing".into()),
            token_id: None,
            token_name: None,
            source: UsageSource::Agent,
            kind: UsageKind::Chat,
            backend: "mock".into(),
            model: "m".into(),
            status: 200,
            duration_ms: 1,
            prompt_tokens: Some(tokens),
            completion_tokens: Some(0),
            total_tokens: Some(tokens),
            input_units: None,
            output_units: None,
            enforce_limits: true,
            principal_kind: PrincipalKind::System,
            agent_id: Some(e.agent.clone()),
            chain: None,
            stop_reason: None,
        }],
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn a_spent_budget_makes_the_agent_unavailable_and_tells_its_managers_why() {
    let e = limited(json!({ "budget": { "monthly_tokens": 1000 } })).await;
    let token = token_of(&e.start_from("203.0.113.9").await);
    spend(&e, 400).await;
    assert_eq!(
        e.say_from(&token, "203.0.113.9", "en").await.status,
        StatusCode::ACCEPTED,
        "under budget"
    );
    let (status, detail) =
        e.fx.get(&e.fx.alice, &format!("/api/v0/agents/{}", e.agent))
            .await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["agent"]["limits"]["available"], true);
    assert_eq!(detail["agent"]["limits"]["budget"][0]["used"], 400.0);
    assert_eq!(detail["agent"]["limits"]["budget"][0]["max"], 1000.0);
    assert_eq!(
        detail["agent"]["limits"]["rate_limits"]["visitor"]["max"],
        20
    );
    assert_eq!(detail["agent"]["limits"]["retention_days"], 30);

    spend(&e, 700).await;
    let r = e.say_from(&token, "203.0.113.9", "fr").await;
    assert_eq!(r.status, StatusCode::SERVICE_UNAVAILABLE, "{}", r.body);
    assert_eq!(code(&r), "agent_unavailable");
    assert_eq!(
        r.body["error"]["message"],
        i18n::t(Lang::Fr, "agent-embed-unavailable").as_str()
    );
    assert!(
        !r.body.to_string().contains("budget"),
        "a visitor is not told about the owner's budget: {}",
        r.body
    );
    assert!(retry_after(&r) >= 1);
    let r = e.start_from("203.0.113.10").await;
    assert_eq!(r.status, StatusCode::SERVICE_UNAVAILABLE, "{}", r.body);

    let (_, detail) =
        e.fx.get(&e.fx.alice, &format!("/api/v0/agents/{}", e.agent))
            .await;
    let limits = &detail["agent"]["limits"];
    assert_eq!(limits["available"], false, "{limits}");
    assert_eq!(limits["unavailable_reason"]["limit"], "budget");
    assert_eq!(limits["unavailable_reason"]["set_by"], "agent");
    assert_eq!(limits["unavailable_reason"]["dimension"], "tokens");
    assert_eq!(limits["budget"][0]["exceeded"], true);
    let refusals = e.refusals().await;
    assert_eq!(refusals.len(), 2);
    assert_eq!(refusals[0]["limit"], "budget");
    assert_eq!(refusals[0]["used"], 1100.0);
}

#[tokio::test]
async fn retention_deletes_only_the_agents_idle_conversations() {
    let e = limited(json!({ "retention_days": 7 })).await;
    let old = token_of(&e.start_from("203.0.113.30").await);
    e.say_from(&old, "203.0.113.30", "en").await;
    let fresh = token_of(&e.start_from("203.0.113.31").await);
    let old_session = e.conversation_of(&old).await;
    let fresh_session = e.conversation_of(&fresh).await;
    let persons = chat::create_session(&e.fx.state.db, "alice").await.unwrap();
    let test_chat = run_sessions::create_principal_session(
        &e.fx.state.db,
        &run_sessions::NewRunSession {
            principal_id: &e.agent,
            title: None,
            parent_turn_id: None,
            agent_version: Some(aiplane_agents::db::agents::DRAFT_VERSION),
        },
    )
    .await
    .unwrap()
    .id;
    let long_ago = (jiff::Timestamp::now() - jiff::SignedDuration::from_hours(24 * 8)).to_string();
    for id in [&old_session, &persons.id, &test_chat] {
        sqlx::query("UPDATE chat_sessions SET updated_at = ? WHERE id = ?")
            .bind(&long_ago)
            .bind(id)
            .execute(&e.fx.state.db)
            .await
            .unwrap();
    }

    let swept = retention::sweep(&e.fx.state.db, None, jiff::Timestamp::now())
        .await
        .unwrap();
    assert_eq!(
        swept[&e.agent].conversations, 2,
        "the idle visitor conversation and the idle test-chat one"
    );
    let left: Vec<String> = sqlx::query_scalar("SELECT id FROM chat_sessions")
        .fetch_all(&e.fx.state.db)
        .await
        .unwrap();
    assert!(!left.contains(&old_session));
    assert!(!left.contains(&test_chat));
    assert!(left.contains(&fresh_session));
    assert!(
        left.contains(&persons.id),
        "a person's chat is never an agent conversation"
    );

    let r = e.resume(&old).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED, "{}", r.body);
    assert_eq!(code(&r), "visitor_session_invalid");
    assert_eq!(e.resume(&fresh).await.status, StatusCode::OK);

    let swept_rows: Vec<Value> = agent_audit::for_principal(&e.fx.state.db, &e.agent)
        .await
        .unwrap()
        .into_iter()
        .filter(|ev| ev.kind == "conversations_swept")
        .map(|ev| ev.detail)
        .collect();
    assert_eq!(
        swept_rows,
        [json!({ "retention_days": 7, "conversations": 2, "sub_agent_runs": 0 })],
        "counts only"
    );
}

/// An operator caps an agent at `/admin/limits` with subject `system`: one
/// more ceiling next to the owner's budget, reported as set by the operator.
#[tokio::test]
async fn an_operator_can_cap_an_agent_with_a_system_limit() {
    let e = limited(json!({ "budget": { "monthly_tokens": 1_000_000 } })).await;
    let rule = |subject: &str| {
        json!({ "subject_type": "system", "subject_id": subject, "model": "",
                "dimension": "tokens", "window": "month", "value": 100 })
    };
    let (status, body) =
        e.fx.post(&e.fx.root, "/api/v0/admin/limits", rule("no-such-agent"))
            .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let (status, body) =
        e.fx.post(&e.fx.root, "/api/v0/admin/limits", rule(&e.agent))
            .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let token = token_of(&e.start_from("203.0.113.50").await);
    spend(&e, 150).await;
    let r = e.say_from(&token, "203.0.113.50", "en").await;
    assert_eq!(r.status, StatusCode::SERVICE_UNAVAILABLE, "{}", r.body);
    let (_, detail) =
        e.fx.get(&e.fx.alice, &format!("/api/v0/agents/{}", e.agent))
            .await;
    assert_eq!(
        detail["agent"]["limits"]["unavailable_reason"]["set_by"],
        "operator"
    );
}
