// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Identity verifiers over HTTP (`docs/agent-visitors.md` → "Identity verifiers"): a
//! visitor proving their address with the code the agent's own ERP sends,
//! typed into the widget's secure field and posted to
//! `/api/v0/embed/resume`; and a website vouching for its visitor on
//! `/api/v0/embed/identity`.
//!
//! The production runner drives each turn on a scripted wiremock upstream;
//! the ERP is a wiremock MCP server granted as an `agent` connector.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use jsonwebtoken::{EncodingKey, Header, encode};
use rama::http::{Method, StatusCode};
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::suspend::{Logs, every_stored_text};
use super::{Embed, SITE, code, new_key};
use crate::agents::{self, TIME};

use aiplane_core::server::db::{mcp_audit, mcp_catalog};
use aiplane_runtime::agents::embed::LiveAgentRunner;
use aiplane_runtime::server::tools::ToolRegistry;
use aiplane_runtime::server::tools::time::CurrentTimestamp;
use session_core::db as chat;
use session_core::i18n::{Lang, t};

const CODE: &str = "481516";
const ALICE: &str = "alice@example.com";
const SECRET: &str = "the-website-and-the-agent-share-this-secret";
const AUDIENCE: &str = "support-agent";

struct Scripted {
    deltas: Vec<Value>,
    served: AtomicUsize,
}

impl wiremock::Respond for Scripted {
    fn respond(&self, _req: &wiremock::Request) -> ResponseTemplate {
        let i = self.served.fetch_add(1, Ordering::SeqCst);
        let delta = &self.deltas[i.min(self.deltas.len() - 1)];
        let sse = format!(
            "data: {}\n\ndata: [DONE]\n\n",
            json!({"choices": [{"index": 0, "delta": delta}]})
        );
        ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream")
    }
}

async fn upstream(deltas: Vec<Value>) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(Scripted {
            deltas,
            served: AtomicUsize::new(0),
        })
        .mount(&server)
        .await;
    server
}

fn call(id: &str, name: &str, args: Value) -> Value {
    json!({"tool_calls": [{"index": 0, "id": id, "type": "function",
        "function": {"name": name, "arguments": args.to_string()}}]})
}

fn text(s: &str) -> Value {
    json!({ "content": s })
}

async fn sent(server: &MockServer) -> Vec<Value> {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| serde_json::from_slice(&r.body).unwrap())
        .collect()
}

/// The customer's ERP: `send_code` and `check_code`, accepting [`CODE`] for
/// [`ALICE`]. Records every call.
async fn erp() -> (MockServer, Arc<Mutex<Vec<(String, Value)>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(move |request: &wiremock::Request| {
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            let answer = |v: Value| json!({"content": [{"type": "text", "text": v.to_string()}]});
            let result = match body["method"].as_str() {
                Some("initialize") => json!({
                    "protocolVersion": "2025-06-18",
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "erp", "version": "1"},
                }),
                Some("tools/list") => json!({"tools": [
                    {"name": "send_code", "inputSchema": {"type": "object"}},
                    {"name": "check_code", "inputSchema": {"type": "object"}},
                ]}),
                Some("tools/call") => {
                    let name = body["params"]["name"].as_str().unwrap().to_string();
                    let args = body["params"]["arguments"].clone();
                    log.lock().unwrap().push((name.clone(), args.clone()));
                    match name.as_str() {
                        "send_code" => answer(json!({"sent": true})),
                        _ if args["email"] == ALICE && args["code"] == CODE => {
                            answer(json!({"valid": true, "customer_id": "K-1"}))
                        }
                        _ => answer(json!({"valid": false})),
                    }
                }
                Some("notifications/initialized") => return ResponseTemplate::new(202),
                other => panic!("unexpected MCP method {other:?}"),
            };
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/json")
                .set_body_json(json!({"jsonrpc": "2.0", "id": body["id"], "result": result}))
        })
        .mount(&server)
        .await;
    (server, seen)
}

fn verified_spec() -> Value {
    json!({
        "main": {
            "model": "m",
            "instructions": { "orchestration": "Verify the visitor, then help." },
            "tools": [TIME]
        },
        "state": {
            "email": { "type": "email", "set_by": ["llm"] },
            "verified": { "type": "subject", "set_by": ["verifier:otp", "host"] }
        },
        "verifiers": {
            "otp": { "kind": "mcp_code", "connector": "erp", "email_slot": "email",
                     "writes": { "verified": "result" } },
            "site": { "kind": "host_jwt", "algorithm": "HS256", "secret": SECRET,
                      "issuer": SITE, "audience": AUDIENCE,
                      "claims": { "verified": { "customer_id": "sub" } } }
        },
        "routes": {
            "billing": {
                "when": { "slot": "verified", "provenance": "verifier:otp" },
                "agent": "billing-agent-id", "task": "Invoices of {verified.customer_id}"
            }
        }
    })
}

/// A published agent with the ERP as its audited `agent` connector, served
/// by the production runner on `llm`.
async fn verifying(llm: &MockServer, erp: &MockServer, spec: Value) -> Embed {
    let mut fx = agents::fixture_with(
        Some(&llm.uri()),
        ToolRegistry::new().with(CurrentTimestamp),
        &[TIME],
    )
    .await;
    fx.state = fx
        .state
        .clone()
        .with_agent_runner(Arc::new(LiveAgentRunner));
    let connector = mcp_catalog::ConnectorInput {
        key: "erp".into(),
        name: "ERP".into(),
        description: None,
        icon: None,
        category: None,
        url: erp.uri(),
        auth: mcp_catalog::AuthKind::None,
        scope: mcp_catalog::Scope::Agent,
        audit: true,
        use_dcr: false,
        client_id: None,
        client_secret_ct: None,
        client_secret_nonce: None,
        authorize_url: None,
        token_url: None,
        registration_url: None,
        scopes: vec![],
        allowed_groups: vec![],
    };
    mcp_catalog::create(&fx.state.db, connector).await.unwrap();
    mcp_catalog::set_enabled(&fx.state.db, "erp", true)
        .await
        .unwrap();
    let agent = fx.runnable("support").await;
    assert_eq!(
        fx.grant(&fx.alice, &agent, "connector", "erp").await,
        StatusCode::CREATED
    );
    let billing = fx.runnable("billing").await;
    let mut billing_spec = agents::spec("Explain the invoices.");
    billing_spec["finish"] = json!({ "schema": { "type": "object" } });
    let (status, body) = fx.put_draft(&fx.alice, &billing, billing_spec).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = fx.publish(&fx.alice, &billing).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let spec =
        serde_json::from_str(&spec.to_string().replace("billing-agent-id", &billing)).unwrap();
    let (status, body) = fx.put_draft(&fx.alice, &agent, spec).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = fx.publish(&fx.alice, &agent).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (key_id, key) = new_key(&fx, &agent, &[SITE]).await;
    Embed {
        fx,
        agent,
        key_id,
        key,
        runner: Arc::default(),
    }
}

#[tokio::test]
async fn a_visitor_verifies_with_the_code_in_the_secure_field_and_the_gate_opens() {
    let logs = Logs::default();
    let _guard = logs.capture();
    let llm = upstream(vec![
        call("k1", "set_email", json!({ "value": ALICE })),
        call("k2", "verify_otp_request_code", json!({})),
        text("Thanks, you are verified."),
    ])
    .await;
    let (erp, seen) = erp().await;
    let e = verifying(&llm, &erp, verified_spec()).await;
    let token = e.visitor().await;

    let r = e.say(&token, "My invoice is wrong.").await;
    assert_eq!(r.status, StatusCode::ACCEPTED, "{}", r.body);
    let paused = e.settled(&token).await;
    assert_eq!(paused.status, chat::TurnStatus::Suspended);
    let frames = e.event_frames(&token).await;
    let (name, frame) = frames.last().unwrap();
    assert_eq!(name, "suspended", "{frames:?}");
    assert_eq!(frame["kind"], "secure_input");
    assert_eq!(
        frame["message"],
        t(Lang::En, "agent-verifier-code-sent").as_str()
    );
    assert_eq!(frame["options"], json!(["value", "deny"]));
    assert!(frame.get("tool").is_none(), "{frame}");
    let request_id = frame["request_id"].as_str().unwrap().to_string();

    let r = e
        .answer(
            &token,
            json!({ "request_id": request_id, "decision": "value", "value": CODE }),
        )
        .await;
    assert_eq!(r.status, StatusCode::ACCEPTED, "{}", r.body);
    let done = e.settled(&token).await;
    assert_eq!(done.status, chat::TurnStatus::Completed, "{done:?}");
    assert_eq!(done.content.as_deref(), Some("Thanks, you are verified."));

    let session = e.conversation_of(&token).await;
    let slots = aiplane_agents::db::agent_state::for_session(&e.fx.state.db, &session)
        .await
        .unwrap();
    let verified = slots.iter().find(|s| s.slot == "verified").unwrap();
    assert_eq!(verified.provenance, "verifier:otp");
    assert_eq!(verified.value, json!({ "customer_id": "K-1" }));
    let calls: Vec<String> = seen.lock().unwrap().iter().map(|c| c.0.clone()).collect();
    assert_eq!(calls, ["send_code", "check_code"]);

    let requests = sent(&llm).await;
    let last = requests.last().unwrap();
    let system = last["messages"][0]["content"].as_str().unwrap();
    assert!(system.contains("- billing: open"), "{system}");
    for request in &requests {
        assert!(!request.to_string().contains(CODE), "never to the model");
    }
    assert!(
        !every_stored_text(&e.fx.state.db).await.contains(CODE),
        "the code is nowhere stored: transcript, tool rows, agent_audit, mcp_tool_audit"
    );
    let check = mcp_audit::recent(&e.fx.state.db, 10)
        .await
        .unwrap()
        .into_iter()
        .find(|ev| ev.tool_id.ends_with("check_code"))
        .expect("the connector is audited");
    assert_eq!(
        check.arguments,
        Some(aiplane_agents::db::agent_audit::redaction::redacted_arguments().to_string()),
        "the activity log's marker"
    );
    let logged = logs.text();
    assert!(
        logged.contains("resuming a suspended agent run"),
        "capturing"
    );
    let leaks: Vec<&str> = logged.lines().filter(|l| l.contains(CODE)).collect();
    assert!(leaks.is_empty(), "never in a log line: {leaks:#?}");
    assert!(
        e.fx.audit_kinds(&e.agent)
            .await
            .contains(&"verifier_outcome".to_string())
    );
}

fn claims(lifetime: i64) -> Value {
    let now = jiff::Timestamp::now().as_second();
    json!({ "iss": SITE, "aud": AUDIENCE, "sub": "K-7", "iat": now, "exp": now + lifetime })
}

fn signed(claims: &Value, secret: &str) -> String {
    encode(
        &Header::default(),
        claims,
        &EncodingKey::from_secret(secret.as_bytes()).unwrap(),
    )
    .unwrap()
}

impl Embed {
    async fn identify(&self, token: &str, identity: &str) -> super::Reply {
        self.send(
            Method::POST,
            "/api/v0/embed/identity",
            Some(SITE),
            Some(token),
            Some(json!({ "token": identity })),
        )
        .await
    }
}

#[tokio::test]
async fn a_website_vouches_for_its_visitor_with_a_signed_token() {
    let llm = upstream(vec![text("Hello.")]).await;
    let (erp, _) = erp().await;
    let e = verifying(&llm, &erp, verified_spec()).await;
    let token = e.visitor().await;

    let r = e.identify(&token, &signed(&claims(300), SECRET)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body, json!({ "slots": ["verified"] }));
    let session = e.conversation_of(&token).await;
    let slots = aiplane_agents::db::agent_state::for_session(&e.fx.state.db, &session)
        .await
        .unwrap();
    assert_eq!(slots[0].provenance, "host");
    assert_eq!(slots[0].value, json!({ "customer_id": "K-7" }));

    let mut expired = claims(300);
    expired["exp"] = json!(jiff::Timestamp::now().as_second() - 120);
    expired["iat"] = json!(jiff::Timestamp::now().as_second() - 300);
    let r = e.identify(&token, &signed(&expired, SECRET)).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED, "{}", r.body);
    assert_eq!(code(&r), "identity_token_invalid");
    assert!(
        r.body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("expired")
    );

    let mut elsewhere = claims(300);
    elsewhere["aud"] = json!("another-agent");
    let r = e.identify(&token, &signed(&elsewhere, SECRET)).await;
    assert_eq!(code(&r), "identity_token_invalid");
    assert!(r.body["error"]["message"].as_str().unwrap().contains("aud"));

    let forged = signed(&claims(300), "a-guess-at-the-secret-that-is-wrong!!");
    let r = e.identify(&token, &forged).await;
    assert_eq!(code(&r), "identity_token_invalid");
    assert!(
        r.body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("signature")
    );

    let mut once = claims(300);
    once["jti"] = json!("visit-1");
    let first = e.identify(&token, &signed(&once, SECRET)).await;
    assert_eq!(first.status, StatusCode::OK, "{}", first.body);
    let again = e.identify(&token, &signed(&once, SECRET)).await;
    assert_eq!(again.status, StatusCode::CONFLICT, "{}", again.body);
    assert_eq!(code(&again), "identity_token_replayed");

    let r = e
        .identify("gwv_nobody", &signed(&claims(300), SECRET))
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    assert_eq!(code(&r), "visitor_session_invalid");
}

#[tokio::test]
async fn the_shared_secret_is_sealed_before_it_is_stored_or_shown() {
    let llm = upstream(vec![text("Hello.")]).await;
    let (erp, _) = erp().await;
    let e = verifying(&llm, &erp, verified_spec()).await;
    let (status, body) =
        e.fx.get(&e.fx.alice, &format!("/api/v0/agents/{}", e.agent))
            .await;
    assert_eq!(status, StatusCode::OK);
    let site = &body["agent"]["live_spec"]["verifiers"]["site"];
    assert!(site.get("secret").is_none(), "{site}");
    assert!(site["secret_sealed"].is_string(), "{site}");
    assert!(!body.to_string().contains(SECRET));
    assert!(!every_stored_text(&e.fx.state.db).await.contains(SECRET));

    let (status, body) =
        e.fx.put_draft(&e.fx.alice, &e.agent, body["agent"]["draft_spec"].clone())
            .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a sealed secret round-trips: {body}"
    );
}

#[tokio::test]
async fn a_misconfigured_verifier_is_refused_on_save_with_what_to_fix() {
    let llm = upstream(vec![text("Hello.")]).await;
    let (erp, _) = erp().await;
    let e = verifying(&llm, &erp, verified_spec()).await;
    let mut spec = verified_spec();
    spec.as_object_mut().unwrap().remove("routes");
    spec["verifiers"]["otp"]["connector"] = json!("crm");
    spec["verifiers"]["otp"]["max_attempts"] = json!(99);
    spec["verifiers"]["site"]["secret"] = json!("short");
    let (status, body) = e.fx.put_draft(&e.fx.alice, &e.agent, spec).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["error"]["code"], "invalid_agent_spec");
    let paths: Vec<&str> = body["error"]["issues"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["path"].as_str().unwrap())
        .collect();
    assert_eq!(
        paths,
        [
            "verifiers.otp.connector",
            "verifiers.otp.max_attempts",
            "verifiers.site.secret"
        ]
    );

    let r = {
        let llm = upstream(vec![text("Hello.")]).await;
        let plain = super::embed_live(&llm, None).await;
        let token = plain.visitor().await;
        plain.identify(&token, &signed(&claims(300), SECRET)).await
    };
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{}", r.body);
    assert_eq!(code(&r), "identity_not_configured");
}
