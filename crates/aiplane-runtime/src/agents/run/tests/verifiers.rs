// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Identity verifiers in real agent runs: the customer's ERP is a wiremock
//! MCP server granted as an `agent` connector, the model a scripted
//! upstream, the database real. The visitor's code arrives the way the
//! widget sends it, as the decision of a `secure_input` pause.

use jiff::SignedDuration;

use aiplane_agents::db::run_sessions;
use aiplane_core::server::db::{mcp_audit, mcp_catalog};
use jsonwebtoken::{EncodingKey, Header, encode};
use session_core::db::{Decision, DecisionKind, SuspensionKind};
use session_core::i18n::t;

use super::suspend::every_stored_text;
use super::*;
use crate::agents::resume::{AgentResume, ResumedBy, claim, run_claimed};
use crate::agents::spec::secrets::{SPEC_SECRETS, seal_spec_secrets};
use crate::agents::verifier::host_jwt::{self, IdentityError, Refusal};

pub(super) const CODE: &str = "481516";
pub(super) const ALICE: &str = "alice@example.com";
const STRANGER: &str = "nobody@example.com";
const REQUEST: &str = "verify_otp_request_code";
const SUBMIT: &str = "verify_otp_submit_code";

/// The customer's ERP over MCP: `send_code` knows only [`ALICE`],
/// `check_code` accepts [`CODE`] for her, `find_customer` knows Alice
/// Smith, K-1. Every call is recorded as `(tool, arguments)`.
pub(super) async fn erp() -> (MockServer, Arc<Mutex<Vec<(String, Value)>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let server = MockServer::start().await;
    let tool = |name: &str, props: Value| {
        json!({ "name": name, "description": name,
                "inputSchema": { "type": "object", "properties": props } })
    };
    let tools = json!([
        tool("send_code", json!({ "email": {"type": "string"} })),
        tool(
            "check_code",
            json!({ "email": {"type": "string"}, "code": {"type": "string"} })
        ),
        tool(
            "find_customer",
            json!({ "name": {"type": "string"}, "number": {"type": "string"} })
        ),
    ]);
    Mock::given(method("POST"))
        .respond_with(move |request: &wiremock::Request| {
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            let text = |t: Value, error: bool| {
                json!({ "content": [{"type": "text", "text": t.to_string()}], "isError": error })
            };
            let result = match body["method"].as_str() {
                Some("initialize") => json!({
                    "protocolVersion": "2025-06-18",
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "erp", "version": "1"},
                }),
                Some("tools/list") => json!({ "tools": tools }),
                Some("tools/call") => {
                    let name = body["params"]["name"].as_str().unwrap().to_string();
                    let args = body["params"]["arguments"].clone();
                    log.lock().unwrap().push((name.clone(), args.clone()));
                    let alice = args["email"] == ALICE;
                    match name.as_str() {
                        "send_code" if alice => text(json!({"sent": true}), false),
                        "send_code" => text(json!({"error": "no such customer"}), true),
                        "check_code" if alice && args["code"] == CODE => text(
                            json!({"valid": true, "customer_id": "K-1", "name": "Alice"}),
                            false,
                        ),
                        "check_code" => text(json!({"valid": false}), false),
                        "find_customer"
                            if args["name"] == "Alice Smith" && args["number"] == "K-1" =>
                        {
                            text(json!({"valid": true, "customer_id": "K-1"}), false)
                        }
                        "find_customer" => text(json!({"valid": false}), false),
                        other => panic!("unexpected tool {other}"),
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

impl World {
    /// The ERP as an enabled, audited `agent` connector `erp`.
    pub(super) async fn connect_erp(&self, erp: &MockServer) {
        mcp_catalog::create(
            self.db(),
            mcp_catalog::ConnectorInput {
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
            },
        )
        .await
        .unwrap();
        mcp_catalog::set_enabled(self.db(), "erp", true)
            .await
            .unwrap();
    }
}

fn erp_calls(seen: &Mutex<Vec<(String, Value)>>, tool: &str) -> Vec<Value> {
    seen.lock()
        .unwrap()
        .iter()
        .filter(|(t, _)| t == tool)
        .map(|(_, a)| a.clone())
        .collect()
}

fn otp_spec(otp: Value) -> Value {
    let mut verifier = json!({
        "kind": "mcp_code", "connector": "erp", "email_slot": "email",
        "writes": { "verified": "result", "verified_email": "input.email" },
        "max_attempts": 5, "code_ttl": "10m"
    });
    if let (Value::Object(v), Value::Object(extra)) = (&mut verifier, otp) {
        v.extend(extra);
    }
    json!({
        "main": {
            "model": "support-model",
            "instructions": { "orchestration": "Verify the visitor, then help." },
            "budget": { "rounds": 12 }
        },
        "state": {
            "email": { "type": "email", "set_by": ["llm"] },
            "name": { "type": "string", "set_by": ["llm"] },
            "customer_number": { "type": "string", "set_by": ["llm"] },
            "verified": { "type": "subject", "set_by": ["verifier:otp", "verifier:kyc", "host"] },
            "verified_email": { "type": "email", "set_by": ["verifier:otp"] }
        },
        "verifiers": {
            "otp": verifier,
            "kyc": {
                "kind": "lookup", "tool": "mcp__erp__find_customer", "assurance": "low",
                "inputs": { "name": "state.name", "number": "state.customer_number" },
                "writes": { "verified": "result" }, "max_attempts": 2
            }
        },
        "routes": {
            "billing": {
                "when": { "slot": "verified", "provenance": "verifier:otp", "max_age": "15m" },
                "agent": "billing-agent",
                "task": "Invoices of {verified.customer_id}"
            }
        }
    })
}

/// The support agent on `spec`, granted the pool and the ERP connector.
async fn support(world: &World, spec: &Value) -> String {
    let id = world
        .agent(
            "support",
            &[
                (GrantKind::Model, "support-model"),
                (GrantKind::Connector, "erp"),
            ],
        )
        .await;
    world.publish(&id, spec).await;
    id
}

/// A clock the test moves.
fn clock() -> (Arc<Mutex<jiff::Timestamp>>, RunOptions) {
    let now = Arc::new(Mutex::new(jiff::Timestamp::now()));
    let read = now.clone();
    let options = RunOptions {
        now: Arc::new(move || *read.lock().unwrap()),
        ..RunOptions::default()
    };
    (now, options)
}

async fn says(world: &World, agent: &str, options: &RunOptions, message: &str) -> AgentReply {
    run_turn_with(
        &world.state,
        AgentTurn {
            agent_id: agent,
            session_id: None,
            message,
            visitor_id: None,
            lang: None,
        },
        options.clone(),
    )
    .await
    .unwrap()
}

/// The visitor types `code` into the field the paused turn shows.
async fn types(
    world: &World,
    agent: &str,
    paused: &AgentReply,
    options: &RunOptions,
    code: &str,
) -> AgentReply {
    let waiting = paused
        .suspension
        .as_ref()
        .expect("the turn waits for input");
    assert_eq!(waiting.kind, SuspensionKind::SecureInput);
    let claimed = claim(
        &world.state,
        AgentResume {
            agent_id: agent,
            session_id: &paused.session_id,
            turn_id: &paused.turn_id,
            request_id: Some(&waiting.request_id),
            decision: Decision::Value { value: json!(code) },
            by: ResumedBy::Participant,
        },
    )
    .await
    .unwrap();
    run_claimed(&world.state, claimed, options.clone())
        .await
        .unwrap()
}

async fn slot_rows(world: &World, session: &str) -> Vec<(String, Value, String)> {
    aiplane_agents::db::agent_state::for_session(world.db(), session)
        .await
        .unwrap()
        .into_iter()
        .map(|s| (s.slot, s.value, s.provenance))
        .collect()
}

fn verifier_outcomes(events: &[agent_audit::AuditEvent]) -> Vec<String> {
    events
        .iter()
        .filter(|e| e.kind == "verifier_outcome")
        .map(|e| e.detail["outcome"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn a_code_the_visitor_types_writes_the_verifier_slots_and_opens_the_gate() {
    let main = llm(vec![
        call("k1", "set_email", json!({ "value": ALICE })),
        call("k2", REQUEST, json!({})),
        text("Thanks, you are verified."),
    ])
    .await;
    let (erp, seen) = erp().await;
    let world = World::new(&[("support-pool", "support-model", &main)], None).await;
    world.connect_erp(&erp).await;
    let agent = support(&world, &otp_spec(json!({}))).await;
    let (_, options) = clock();

    let paused = says(&world, &agent, &options, "I have a billing question.").await;
    assert_eq!(paused.status, chat::TurnStatus::Suspended);
    let waiting = paused.suspension.clone().unwrap();
    let stored = chat::get_suspension(world.db(), &paused.turn_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.tool_call.execution_facts, None);

    assert_eq!(
        waiting.message.as_deref(),
        Some(t(Lang::En, "agent-verifier-code-sent").as_str())
    );
    assert_eq!(waiting.options, [DecisionKind::Value, DecisionKind::Deny]);
    assert_eq!(erp_calls(&seen, "send_code"), [json!({ "email": ALICE })]);

    let done = types(&world, &agent, &paused, &options, CODE).await;
    assert_eq!(done.status, chat::TurnStatus::Completed);
    assert_eq!(done.answer.as_deref(), Some("Thanks, you are verified."));
    assert_eq!(
        erp_calls(&seen, "check_code"),
        [json!({ "email": ALICE, "code": CODE })]
    );

    assert_eq!(
        slot_rows(&world, &paused.session_id).await,
        [
            ("email".into(), json!(ALICE), "llm".into()),
            (
                "verified".into(),
                json!({ "customer_id": "K-1", "name": "Alice" }),
                "verifier:otp".into()
            ),
            ("verified_email".into(), json!(ALICE), "verifier:otp".into()),
        ]
    );
    let sent = requests(&main).await;
    assert_eq!(sent.len(), 3);
    assert!(
        system(&sent[1]).contains("- billing: closed"),
        "{}",
        system(&sent[1])
    );
    assert!(
        system(&sent[2]).contains("- billing: open"),
        "{}",
        system(&sent[2])
    );
    assert!(
        system(&sent[2]).contains("verified: set by verifier:otp"),
        "the model sees who vouched, not the value"
    );
    let verified = tool_answer(&sent, "k2");
    assert!(verified.contains("\"verified\": true"), "{verified}");
    for request in &sent {
        let request = request.to_string();
        assert!(!request.contains(CODE), "the code never reaches the model");
        assert!(
            !request.contains("K-1"),
            "nor what the verifier vouched for"
        );
    }

    assert!(
        !every_stored_text(world.db()).await.contains(CODE),
        "the code is nowhere in the database"
    );
    let audited = mcp_audit::recent(world.db(), 10).await.unwrap();
    let check = audited
        .iter()
        .find(|e| e.tool_id.ends_with("check_code"))
        .unwrap();
    assert_eq!(
        check.arguments,
        Some(aiplane_agents::db::agent_audit::redaction::redacted_arguments().to_string()),
        "the activity log's marker"
    );
    let send = audited
        .iter()
        .find(|e| e.tool_id.ends_with("send_code"))
        .unwrap();
    assert!(send.arguments.as_deref().unwrap().contains(ALICE));
    assert_eq!(
        verifier_outcomes(&world.audit(&agent).await),
        ["code_sent", "verified"]
    );
}

#[tokio::test]
async fn five_wrong_codes_use_the_code_up() {
    let main = llm(vec![
        call("k1", "set_email", json!({ "value": ALICE })),
        call("k2", REQUEST, json!({})),
        call("k3", SUBMIT, json!({})),
        call("k4", SUBMIT, json!({})),
        call("k5", SUBMIT, json!({})),
        call("k6", SUBMIT, json!({})),
        call("k7", SUBMIT, json!({})),
        text("Too many wrong codes."),
    ])
    .await;
    let (erp, seen) = erp().await;
    let world = World::new(&[("support-pool", "support-model", &main)], None).await;
    world.connect_erp(&erp).await;
    let agent = support(&world, &otp_spec(json!({}))).await;
    let (_, options) = clock();

    let mut turn = says(&world, &agent, &options, "Hi").await;
    for guess in ["111111", "222222", "333333", "444444", "555555"] {
        assert_eq!(turn.status, chat::TurnStatus::Suspended, "{turn:?}");
        turn = types(&world, &agent, &turn, &options, guess).await;
    }
    assert_eq!(turn.status, chat::TurnStatus::Completed, "{turn:?}");
    assert_eq!(erp_calls(&seen, "check_code").len(), 5);

    let sent = requests(&main).await;
    assert!(tool_answer(&sent, "k2").contains("\"attempts_left\": 4"));
    assert!(tool_answer(&sent, "k5").contains("\"attempts_left\": 1"));
    for used_up in ["k6", "k7"] {
        let answer = tool_answer(&sent, used_up);
        assert!(
            answer.contains("\"reason\": \"locked\""),
            "{used_up}: {answer}"
        );
        assert!(
            answer.contains(REQUEST),
            "says how to get a new code: {answer}"
        );
    }
    assert!(
        slot_rows(&world, &turn.session_id)
            .await
            .iter()
            .all(|(slot, ..)| slot == "email"),
        "nothing verified"
    );
    assert_eq!(
        verifier_outcomes(&world.audit(&agent).await),
        [
            "code_sent",
            "wrong_code",
            "wrong_code",
            "wrong_code",
            "wrong_code",
            "locked"
        ]
    );
}

#[tokio::test]
async fn an_expired_code_is_refused_without_asking_the_connector() {
    let main = llm(vec![
        call("k1", "set_email", json!({ "value": ALICE })),
        call("k2", REQUEST, json!({})),
        text("It expired."),
    ])
    .await;
    let (erp, seen) = erp().await;
    let world = World::new(&[("support-pool", "support-model", &main)], None).await;
    world.connect_erp(&erp).await;
    let agent = support(&world, &otp_spec(json!({ "code_ttl": "10m" }))).await;
    let (now, options) = clock();

    let paused = says(&world, &agent, &options, "Hi").await;
    let later = now
        .lock()
        .unwrap()
        .checked_add(SignedDuration::from_mins(11))
        .unwrap();
    *now.lock().unwrap() = later;
    let done = types(&world, &agent, &paused, &options, CODE).await;

    assert_eq!(done.status, chat::TurnStatus::Completed);
    assert!(erp_calls(&seen, "check_code").is_empty());
    let answer = tool_answer(&requests(&main).await, "k2").to_string();
    assert!(answer.contains("\"reason\": \"expired\""), "{answer}");
    assert!(
        slot_rows(&world, &paused.session_id)
            .await
            .iter()
            .all(|(slot, ..)| slot == "email")
    );
}

#[tokio::test]
async fn an_unknown_address_hears_exactly_what_a_registered_one_hears() {
    let (erp, seen) = erp().await;
    let mut answers = Vec::new();
    let mut prompts = Vec::new();
    for email in [ALICE, STRANGER] {
        let main = llm(vec![
            call("k1", "set_email", json!({ "value": email })),
            call("k2", REQUEST, json!({})),
            text("Done."),
        ])
        .await;
        let world = World::new(&[("support-pool", "support-model", &main)], None).await;
        world.connect_erp(&erp).await;
        let agent = support(&world, &otp_spec(json!({}))).await;
        let (_, options) = clock();
        let paused = says(&world, &agent, &options, "Hi").await;
        let waiting = paused.suspension.clone().unwrap();
        prompts.push((waiting.kind, waiting.message, waiting.options));
        types(&world, &agent, &paused, &options, "000000").await;
        answers.push(tool_answer(&requests(&main).await, "k2").to_string());
        let delivery: Vec<Value> = world
            .audit(&agent)
            .await
            .iter()
            .filter(|e| e.detail["outcome"] == "code_sent")
            .map(|e| e.detail["delivery"].clone())
            .collect();
        let expected = if email == ALICE {
            "accepted"
        } else {
            "refused"
        };
        assert_eq!(delivery, [json!(expected)], "the owner's audit still knows");
    }
    assert_eq!(prompts[0], prompts[1]);
    assert_eq!(answers[0], answers[1]);
    assert!(answers[0].contains("wrong_code"), "{}", answers[0]);
    assert_eq!(erp_calls(&seen, "send_code").len(), 2);
}

#[tokio::test]
async fn a_conversation_cannot_send_more_codes_than_its_limit() {
    let main = llm(vec![
        call("k1", "set_email", json!({ "value": ALICE })),
        call("k2", REQUEST, json!({})),
        call("k3", REQUEST, json!({})),
        text("Try later."),
    ])
    .await;
    let (erp, seen) = erp().await;
    let world = World::new(&[("support-pool", "support-model", &main)], None).await;
    world.connect_erp(&erp).await;
    let spec = otp_spec(json!({ "send_limits": { "session": { "max": 1, "per": "15m" } } }));
    let agent = support(&world, &spec).await;
    let (_, options) = clock();

    let paused = says(&world, &agent, &options, "Hi").await;
    let waiting = paused.suspension.clone().unwrap();
    let claimed = claim(
        &world.state,
        AgentResume {
            agent_id: &agent,
            session_id: &paused.session_id,
            turn_id: &paused.turn_id,
            request_id: Some(&waiting.request_id),
            decision: Decision::Deny {
                reason: session_core::db::DenyReason::User,
            },
            by: ResumedBy::Participant,
        },
    )
    .await
    .unwrap();
    let done = run_claimed(&world.state, claimed, options.clone())
        .await
        .unwrap();
    assert_eq!(done.status, chat::TurnStatus::Completed, "{done:?}");
    assert_eq!(erp_calls(&seen, "send_code").len(), 1);
    let answer = tool_answer(&requests(&main).await, "k3").to_string();
    assert!(answer.contains("rate_limited"), "{answer}");
    assert!(answer.contains("retry_after_secs"), "{answer}");
}

#[tokio::test]
async fn the_model_can_neither_hand_over_a_code_nor_write_a_verifier_slot() {
    let main = llm(vec![
        call("k1", SUBMIT, json!({ "code": CODE })),
        call(
            "k2",
            "set_verified",
            json!({ "value": { "customer_id": "K-1" } }),
        ),
        text("I could not."),
    ])
    .await;
    let (erp, seen) = erp().await;
    let world = World::new(&[("support-pool", "support-model", &main)], None).await;
    world.connect_erp(&erp).await;
    let agent = support(&world, &otp_spec(json!({}))).await;
    let (_, options) = clock();

    let done = says(&world, &agent, &options, "My code is 481516").await;
    assert_eq!(done.status, chat::TurnStatus::Completed);
    let sent = requests(&main).await;
    let tools = offered(&sent[0]);
    assert!(
        tools.contains(&REQUEST) && tools.contains(&SUBMIT),
        "{tools:?}"
    );
    assert!(tools.contains(&"verify_kyc"), "{tools:?}");
    assert!(
        !tools
            .iter()
            .any(|t| *t == "set_verified" || *t == "set_verified_email")
    );
    assert_eq!(
        tool_def(&sent[0], SUBMIT)["function"]["parameters"],
        json!({ "type": "object", "properties": {}, "additionalProperties": false })
    );
    assert!(tool_answer(&sent, "k1").contains("takes no arguments"));
    assert!(tool_answer(&sent, "k1").contains("`code`"));
    assert!(!tool_answer(&sent, "k2").contains("\"status\": \"set\""));
    assert!(slot_rows(&world, &done.session_id).await.is_empty());
    assert!(seen.lock().unwrap().is_empty(), "nothing reached the ERP");
}

#[tokio::test]
async fn a_lookup_vouches_with_its_own_provenance_and_caps_its_attempts() {
    let main = llm(vec![
        call("k1", "set_name", json!({ "value": "Alice Smith" })),
        call("k2", "set_customer_number", json!({ "value": "K-2" })),
        call("k3", "verify_kyc", json!({})),
        call("k4", "set_customer_number", json!({ "value": "K-1" })),
        call("k5", "verify_kyc", json!({})),
        call("k6", "verify_kyc", json!({})),
        text("Done."),
    ])
    .await;
    let (erp, seen) = erp().await;
    let world = World::new(&[("support-pool", "support-model", &main)], None).await;
    world.connect_erp(&erp).await;
    let agent = support(&world, &otp_spec(json!({}))).await;
    let (_, options) = clock();

    let done = says(&world, &agent, &options, "I am Alice Smith").await;
    assert_eq!(done.status, chat::TurnStatus::Completed, "{done:?}");
    let sent = requests(&main).await;
    let miss = tool_answer(&sent, "k3");
    assert!(miss.contains("not_confirmed"), "{miss}");
    assert!(
        miss.contains("\"attempts_left\": 1"),
        "the count the attempt was admitted against: {miss}"
    );
    let hit = tool_answer(&sent, "k5");
    assert!(
        hit.contains("\"verified\": true") && hit.contains("\"low\""),
        "{hit}"
    );
    let capped = tool_answer(&sent, "k6");
    assert!(capped.contains("too_many_attempts"), "{capped}");
    assert_eq!(erp_calls(&seen, "find_customer").len(), 2);

    let verified = slot_rows(&world, &done.session_id)
        .await
        .into_iter()
        .find(|(s, ..)| s == "verified")
        .unwrap();
    assert_eq!(verified.1, json!({ "customer_id": "K-1" }));
    assert_eq!(verified.2, "verifier:kyc");
    let assurance: Vec<Value> = world
        .audit(&agent)
        .await
        .iter()
        .filter(|e| e.kind == "verifier_outcome")
        .map(|e| e.detail["assurance"].clone())
        .collect();
    assert!(assurance.iter().all(|a| a == "low"), "{assurance:?}");
    assert!(
        system(&sent[6]).contains("- billing: closed"),
        "a lookup does not open a gate that requires the code"
    );
}

/// A lookup whose second write is refused stores neither: a `verified` slot
/// without its companion would open a gate for half an identity.
#[tokio::test]
async fn a_lookup_whose_second_write_is_refused_stores_nothing() {
    let main = llm(vec![
        call("k1", "set_name", json!({ "value": "Alice Smith" })),
        call("k2", "set_customer_number", json!({ "value": "K-1" })),
        call("k3", "verify_kyc", json!({})),
        text("Done."),
    ])
    .await;
    let (erp, _) = erp().await;
    let world = World::new(&[("support-pool", "support-model", &main)], None).await;
    world.connect_erp(&erp).await;
    let mut spec = otp_spec(json!({}));
    spec["state"]["zz_note"] = json!({ "type": "string", "set_by": ["verifier:otp"] });
    spec["verifiers"]["kyc"]["writes"] = json!({ "verified": "result", "zz_note": "input.name" });
    let agent = support(&world, &spec).await;
    let (_, options) = clock();

    let done = says(&world, &agent, &options, "I am Alice Smith").await;
    assert_eq!(done.status, chat::TurnStatus::Completed, "{done:?}");
    let sent = requests(&main).await;
    let answer = tool_answer(&sent, "k3");
    assert!(
        answer.contains("zz_note"),
        "the refusal names the slot: {answer}"
    );
    let trusted: Vec<String> = slot_rows(&world, &done.session_id)
        .await
        .into_iter()
        .filter(|(_, _, provenance)| provenance != "llm")
        .map(|(slot, ..)| slot)
        .collect();
    assert!(trusted.is_empty(), "nothing vouched for: {trusted:?}");
}

/// The slots a lookup reads, the lookup itself and the forward it opens, all
/// in one round: writers run first in call order, so the forward sees what
/// the lookup vouched for every time.
#[tokio::test]
async fn a_forward_in_the_round_of_the_lookup_that_opens_its_route_is_dispatched() {
    for attempt in 0..30 {
        let main = llm(vec![
            calls(&[
                ("fwd", "forward_request", json!({})),
                ("k1", "set_name", json!({ "value": "Alice Smith" })),
                ("k2", "set_customer_number", json!({ "value": "K-1" })),
                ("k3", "verify_kyc", json!({})),
            ]),
            text("Done."),
        ])
        .await;
        let helper = llm(vec![finish("h1", json!({"answer": "ok"}))]).await;
        let (erp, _) = erp().await;
        let world = World::new(
            &[
                ("support-pool", "support-model", &main),
                ("helper-pool", "helper-model", &helper),
            ],
            None,
        )
        .await;
        world.connect_erp(&erp).await;
        let helper_id = world
            .agent("helper", &[(GrantKind::Model, "helper-model")])
            .await;
        world.publish(&helper_id, &helper_spec(4)).await;
        let mut spec = otp_spec(json!({}));
        spec["router"] = json!({ "kind": "rules" });
        spec["routes"] = json!({
            "account": {
                "when": { "slot": "verified", "provenance": "verifier:kyc" },
                "agent": helper_id,
                "task": "Account of {verified.customer_id}"
            }
        });
        let agent = support(&world, &spec).await;
        let (_, options) = clock();

        let done = says(&world, &agent, &options, "I am Alice Smith, K-1").await;
        assert_eq!(done.status, chat::TurnStatus::Completed, "{done:?}");
        let sent = requests(&main).await;
        let verified = tool_answer(&sent, "k3");
        assert!(
            verified.contains("\"verified\": true"),
            "attempt {attempt}: {verified}"
        );
        let dispatched = tool_answer(&sent, "fwd");
        assert!(
            dispatched.contains("\"forwarded\": true"),
            "attempt {attempt}: {dispatched}"
        );
        let helper = requests(&helper).await;
        assert_eq!(helper.len(), 1, "attempt {attempt}");
        assert_eq!(helper[0]["messages"][1]["content"], "Account of K-1");
    }
}

// --- host identity tokens ----------------------------------------------------

const SECRET: &str = "a-shared-secret-of-at-least-32-bytes!";
const ISSUER: &str = "https://www.example.com";
const AUDIENCE: &str = "support-agent";
/// A throw-away P-256 key for the JWKS test: the private half signs, the
/// public half is served from the mock JWKS below. Generated for this test,
/// never used anywhere else.
const EC_PRIVATE: &str = "-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgyor9Z19yS0e42TBC
SjOt4WNm3QabfIgqoG5wO4gv6euhRANCAATD20OG9gBw3nfEgVqWewZvgC33X7YD
RcNm4EJrOD+IHmSMZ9soKSVOFAURZOFu88JqiUma30te8xHfHPhIssmx
-----END PRIVATE KEY-----";
const EC_X: &str = "w9tDhvYAcN53xIFalnsGb4At91-2A0XDZuBCazg_iB4";
const EC_Y: &str = "ZIxn2ygpJU4UBRFk4W7zwmqJSZrfS17zEd8c-EiyybE";

fn host_spec(world: &World, key: Value) -> Value {
    let mut verifier = json!({
        "kind": "host_jwt", "issuer": ISSUER, "audience": AUDIENCE,
        "claims": { "verified": { "customer_id": "sub", "plan": "plan" } }
    });
    verifier
        .as_object_mut()
        .unwrap()
        .extend(key.as_object().unwrap().clone());
    let mut spec = json!({
        "state": { "verified": { "type": "subject", "set_by": ["host"] } },
        "verifiers": { "site": verifier }
    });
    seal_spec_secrets(&mut spec, SPEC_SECRETS, &world.state.crypto).unwrap();
    spec
}

fn hs256() -> Value {
    json!({ "algorithm": "HS256", "secret": SECRET })
}

fn claims(lifetime: i64) -> Value {
    let now = jiff::Timestamp::now().as_second();
    json!({ "iss": ISSUER, "aud": AUDIENCE, "sub": "K-1", "plan": "pro",
            "iat": now, "exp": now + lifetime })
}

fn signed(claims: &Value, secret: &str) -> String {
    encode(
        &Header::default(),
        claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .unwrap()
}

/// A conversation of a fresh agent, for writing slots into.
async fn conversation(world: &World) -> (String, String) {
    static N: AtomicUsize = AtomicUsize::new(0);
    let name = format!("site-{}", N.fetch_add(1, Ordering::SeqCst));
    let agent = world.agent(&name, &[]).await;
    let session = run_sessions::create_principal_session(
        world.db(),
        &run_sessions::NewRunSession {
            principal_id: &agent,
            title: None,
            parent_turn_id: None,
            agent_version: Some(1),
        },
    )
    .await
    .unwrap();
    (agent, session.id)
}

/// [`host_jwt::accept`] on `spec` as a published version holds it.
async fn accept(
    state: &RamaState,
    agent: &str,
    session: &str,
    spec: &Value,
    token: &str,
    now: jiff::Timestamp,
) -> Result<Vec<String>, IdentityError> {
    let compiled = crate::agents::spec_cache::CompiledSpec::compile(1, spec.clone());
    host_jwt::accept(state, agent, session, &compiled, token, now).await
}

async fn present(world: &World, spec: &Value, token: &str) -> Result<Vec<String>, IdentityError> {
    let (agent, session) = conversation(world).await;
    accept(
        &world.state,
        &agent,
        &session,
        spec,
        token,
        jiff::Timestamp::now(),
    )
    .await
}

fn refusal(result: Result<Vec<String>, IdentityError>) -> Refusal {
    match result {
        Err(IdentityError::Invalid(r)) => r,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[tokio::test]
async fn a_valid_host_token_writes_its_claims_as_host() {
    let world = World::new(&[], None).await;
    let spec = host_spec(&world, hs256());
    let (agent, session) = conversation(&world).await;
    let written = accept(
        &world.state,
        &agent,
        &session,
        &spec,
        &signed(&claims(300), SECRET),
        jiff::Timestamp::now(),
    )
    .await
    .unwrap();
    assert_eq!(written, ["verified"]);
    assert_eq!(
        slot_rows(&world, &session).await,
        [(
            "verified".into(),
            json!({ "customer_id": "K-1", "plan": "pro" }),
            "host".into()
        )]
    );
    let events = world.audit(&agent).await;
    let accepted = events.iter().find(|e| e.kind == "host_identity").unwrap();
    assert_eq!(accepted.detail["outcome"], "accepted");
    let (chain_key, conversation): (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT chain_key, conversation_id FROM agent_audit WHERE kind = 'host_identity'",
    )
    .fetch_one(world.db())
    .await
    .unwrap();
    assert_eq!(
        (chain_key, conversation),
        (
            Some(format!("conversation:{session}")),
            Some(session.clone())
        ),
        "the identity is part of the conversation's chain"
    );
    assert!(
        !accepted.detail.to_string().contains("K-1"),
        "no claim value"
    );
    assert!(!every_stored_text(world.db()).await.contains(SECRET));
}

#[tokio::test]
async fn expired_misaddressed_forged_and_long_lived_tokens_are_refused() {
    let world = World::new(&[], None).await;
    let spec = host_spec(&world, hs256());

    let mut expired = claims(300);
    expired["exp"] = json!(jiff::Timestamp::now().as_second() - 120);
    expired["iat"] = json!(jiff::Timestamp::now().as_second() - 400);
    assert_eq!(
        refusal(present(&world, &spec, &signed(&expired, SECRET)).await),
        Refusal::Expired
    );

    let mut elsewhere = claims(300);
    elsewhere["aud"] = json!("another-agent");
    assert_eq!(
        refusal(present(&world, &spec, &signed(&elsewhere, SECRET)).await),
        Refusal::WrongAudience
    );

    let mut issuer = claims(300);
    issuer["iss"] = json!("https://evil.example");
    assert_eq!(
        refusal(present(&world, &spec, &signed(&issuer, SECRET)).await),
        Refusal::WrongIssuer
    );

    let forged = signed(&claims(300), "another-secret-of-at-least-32-bytes!!");
    assert_eq!(
        refusal(present(&world, &spec, &forged).await),
        Refusal::BadSignature
    );

    assert!(matches!(
        refusal(present(&world, &spec, &signed(&claims(3 * 3600), SECRET)).await),
        Refusal::TooLong { .. }
    ));

    let mut no_plan = claims(300);
    no_plan.as_object_mut().unwrap().remove("plan");
    assert_eq!(
        refusal(present(&world, &spec, &signed(&no_plan, SECRET)).await),
        Refusal::MissingClaim("plan".into())
    );

    assert_eq!(
        refusal(present(&world, &spec, "not.a.jwt").await),
        Refusal::Malformed
    );
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_state")
        .fetch_one(world.db())
        .await
        .unwrap();
    assert_eq!(rows, 0, "a refused token writes nothing");
}

#[tokio::test]
async fn a_token_signed_with_another_algorithm_is_refused_before_its_signature() {
    let world = World::new(&[], None).await;
    let spec = host_spec(
        &world,
        json!({ "algorithm": "ES256", "jwks_url": "http://127.0.0.1:9/jwks" }),
    );
    // The classic confusion: an HMAC token for a public-key verifier.
    assert_eq!(
        refusal(present(&world, &spec, &signed(&claims(300), SECRET)).await),
        Refusal::WrongAlgorithm {
            expected: "ES256".into(),
            found: "HS256".into()
        }
    );
}

#[tokio::test]
async fn a_flood_of_refused_identity_tokens_writes_one_event_per_window() {
    let world = World::new(&[], None).await;
    let spec = host_spec(&world, hs256());
    let (agent, session) = conversation(&world).await;
    let forged = signed(&claims(300), "another-secret-of-at-least-32-bytes!!");
    for token in
        std::iter::repeat_n("not.a.jwt", 100).chain(std::iter::repeat_n(forged.as_str(), 100))
    {
        let refused = accept(
            &world.state,
            &agent,
            &session,
            &spec,
            token,
            jiff::Timestamp::now(),
        )
        .await;
        assert!(matches!(refused, Err(IdentityError::Invalid(_))));
    }
    let rows: Vec<String> =
        sqlx::query_scalar("SELECT detail FROM agent_audit WHERE kind = 'host_identity'")
            .fetch_all(world.db())
            .await
            .unwrap();
    assert_eq!(rows.len(), 2, "one per reason, not per request");
}

#[tokio::test]
async fn a_token_with_a_jti_is_accepted_once() {
    let world = World::new(&[], None).await;
    let spec = host_spec(&world, hs256());
    let mut once = claims(300);
    once["jti"] = json!("token-1");
    let token = signed(&once, SECRET);
    let (agent, first) = conversation(&world).await;
    let now = jiff::Timestamp::now();
    assert!(
        accept(&world.state, &agent, &first, &spec, &token, now)
            .await
            .is_ok()
    );
    let (_, elsewhere) = conversation(&world).await;
    let replayed = accept(&world.state, &agent, &elsewhere, &spec, &token, now).await;
    assert!(
        matches!(replayed, Err(IdentityError::Replayed)),
        "{replayed:?}"
    );
    assert!(slot_rows(&world, &elsewhere).await.is_empty());
}

#[tokio::test]
async fn a_public_key_token_verifies_against_the_websites_jwks() {
    let jwks = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/.well-known/jwks.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "keys": [{
            "kty": "EC", "crv": "P-256", "kid": "site-1", "alg": "ES256", "use": "sig",
            "x": EC_X, "y": EC_Y
        }] })))
        .mount(&jwks)
        .await;
    let world = private_world().await;
    let url = format!("{}/.well-known/jwks.json", jwks.uri());
    let spec = host_spec(&world, json!({ "algorithm": "ES256", "jwks_url": url }));
    let key = EncodingKey::from_ec_pem(EC_PRIVATE.as_bytes()).unwrap();
    let mut header = Header::new(jsonwebtoken::Algorithm::ES256);
    header.kid = Some("site-1".into());
    let token = encode(&header, &claims(300), &key).unwrap();
    assert_eq!(present(&world, &spec, &token).await.unwrap(), ["verified"]);

    header.kid = Some("rotated-away".into());
    let stale = encode(&header, &claims(300), &key).unwrap();
    for _ in 0..3 {
        assert_eq!(
            refusal(present(&world, &spec, &stale).await),
            Refusal::UnknownKey
        );
    }
    assert_eq!(
        jwks.received_requests().await.unwrap().len(),
        1,
        "an unknown kid does not refetch the keys on every request"
    );
}

/// A world whose operator lets agents reach private networks, so a
/// loopback wiremock can stand in for the website.
async fn private_world() -> World {
    World::build_with(
        &[],
        None,
        false,
        base_tools(),
        None,
        aiplane_core::server::config::NetworkConfig {
            allow_private_networks: true,
        },
    )
    .await
}

fn es256_token() -> String {
    let key = EncodingKey::from_ec_pem(EC_PRIVATE.as_bytes()).unwrap();
    let mut header = Header::new(jsonwebtoken::Algorithm::ES256);
    header.kid = Some("site-1".into());
    encode(&header, &claims(300), &key).unwrap()
}

/// What a visitor is told when the keys cannot be had: nothing about the
/// address, its answer or why it failed, so the endpoint is no probe into
/// the gateway's network.
fn assert_generic(err: &IdentityError, url: &str, leaks: &[&str]) {
    assert!(
        matches!(err, IdentityError::KeysUnavailable { .. }),
        "{err:?}"
    );
    assert_eq!(err.code(), "identity_keys_unavailable");
    let shown = err.to_string();
    assert!(!shown.contains(url), "{shown}");
    for leak in leaks {
        assert!(!shown.contains(leak), "`{leak}` leaked: {shown}");
    }
}

#[tokio::test]
async fn a_jwks_redirect_is_not_followed() {
    let inside = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "keys": [{
            "kty": "EC", "crv": "P-256", "kid": "site-1", "alg": "ES256", "use": "sig",
            "x": EC_X, "y": EC_Y
        }] })))
        .mount(&inside)
        .await;
    let outside = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(302).insert_header("location", format!("{}/jwks", inside.uri())),
        )
        .mount(&outside)
        .await;
    let world = private_world().await;
    let url = format!("{}/jwks", outside.uri());
    let spec = host_spec(&world, json!({ "algorithm": "ES256", "jwks_url": url }));

    let err = present(&world, &spec, &es256_token()).await.unwrap_err();
    assert_generic(&err, &url, &["302", "Found"]);
    assert!(
        inside.received_requests().await.unwrap().is_empty(),
        "the redirect was followed"
    );
}

#[tokio::test]
async fn a_jwks_url_on_a_private_address_is_refused_without_connecting() {
    let world = World::new(&[], None).await;
    let url = "https://10.20.30.40/.well-known/jwks.json";
    let spec = host_spec(&world, json!({ "algorithm": "ES256", "jwks_url": url }));
    let err = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        present(&world, &spec, &es256_token()),
    )
    .await
    .expect("refused before any connection attempt")
    .unwrap_err();
    assert_generic(&err, url, &["10.20.30.40", "private"]);
}

#[tokio::test]
async fn a_failing_jwks_endpoint_tells_the_visitor_nothing_about_it() {
    let world = private_world().await;

    let broken = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(500).set_body_string("db password is hunter2"))
        .mount(&broken)
        .await;
    let url = format!("{}/broken", broken.uri());
    let spec = host_spec(&world, json!({ "algorithm": "ES256", "jwks_url": url }));
    let err = present(&world, &spec, &es256_token()).await.unwrap_err();
    assert_generic(&err, &url, &["500", "hunter2", "Internal"]);

    let page = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>hello</html>"))
        .mount(&page)
        .await;
    let url = format!("{}/page", page.uri());
    let spec = host_spec(&world, json!({ "algorithm": "ES256", "jwks_url": url }));
    let err = present(&world, &spec, &es256_token()).await.unwrap_err();
    assert_generic(&err, &url, &["JWKS document", "expected", "line"]);
}

/// Two host slots, written in name order: `a_verified`, then `b_plan`.
fn two_slot_spec(world: &World, b_set_by: &str) -> Value {
    let mut spec = json!({
        "state": {
            "a_verified": { "type": "subject", "set_by": ["host"] },
            "b_plan": { "type": "string", "max_length": 20, "set_by": [b_set_by] }
        },
        "verifiers": { "site": {
            "kind": "host_jwt", "issuer": ISSUER, "audience": AUDIENCE,
            "algorithm": "HS256", "secret": SECRET,
            "claims": { "a_verified": { "customer_id": "sub" }, "b_plan": "plan" }
        } }
    });
    seal_spec_secrets(&mut spec, SPEC_SECRETS, &world.state.crypto).unwrap();
    spec
}

fn once_token() -> String {
    let mut once = claims(300);
    once["jti"] = json!("token-once");
    signed(&once, SECRET)
}

/// Accepting a token is all or nothing: when the second slot cannot be
/// stored, the first is not either and the `jti` is not spent, so the
/// website's retry with the same token goes through.
#[tokio::test]
async fn a_failed_slot_write_stores_nothing_and_leaves_the_jti_unspent() {
    let world = World::new(&[], None).await;
    let spec = two_slot_spec(&world, "host");
    let token = once_token();
    let (agent, session) = conversation(&world).await;
    sqlx::query(
        "CREATE TRIGGER refuse_b_plan BEFORE INSERT ON agent_state WHEN NEW.slot = 'b_plan'
         BEGIN SELECT RAISE(ABORT, 'disk full'); END",
    )
    .execute(world.db())
    .await
    .unwrap();
    let now = jiff::Timestamp::now();

    let failed = accept(&world.state, &agent, &session, &spec, &token, now).await;
    assert!(
        matches!(failed, Err(IdentityError::Storage(_))),
        "{failed:?}"
    );
    assert!(
        slot_rows(&world, &session).await.is_empty(),
        "a partial write"
    );

    sqlx::query("DROP TRIGGER refuse_b_plan")
        .execute(world.db())
        .await
        .unwrap();
    let retried = accept(&world.state, &agent, &session, &spec, &token, now).await;
    assert_eq!(retried.unwrap(), ["a_verified", "b_plan"]);
    assert_eq!(slot_rows(&world, &session).await.len(), 2);
}

/// A slot the host may not write is found before anything is stored.
#[tokio::test]
async fn a_slot_the_host_may_not_write_stores_nothing_and_leaves_the_jti_unspent() {
    let world = World::new(&[], None).await;
    let token = once_token();
    let (agent, session) = conversation(&world).await;
    let now = jiff::Timestamp::now();

    let refused = accept(
        &world.state,
        &agent,
        &session,
        &two_slot_spec(&world, "llm"),
        &token,
        now,
    )
    .await;
    assert!(refused.is_err(), "{refused:?}");
    assert!(
        slot_rows(&world, &session).await.is_empty(),
        "a partial write"
    );

    let retried = accept(
        &world.state,
        &agent,
        &session,
        &two_slot_spec(&world, "host"),
        &token,
        now,
    )
    .await;
    assert_eq!(retried.unwrap(), ["a_verified", "b_plan"]);
}

async fn organization_otp_support(world: &World, erp: &MockServer, spec: &Value) -> String {
    use aiplane_agents::db::{agent_connections, organizations};
    use aiplane_core::server::db::{WriteTx, gateway_groups, mcp_connections};
    gateway_groups::upsert_group(world.db(), "organization-staff", "", false, true)
        .await
        .unwrap();
    gateway_groups::set_can_manage_agents(world.db(), "organization-staff", true)
        .await
        .unwrap();
    let organizations::OrganizationCreateResult::Created(organization) = organizations::create(
        world.db(),
        &organizations::NewOrganization {
            name: "Support organization",
            description: "",
            member_groups: &["organization-staff".to_owned()],
        },
        "u1",
    )
    .await
    .unwrap() else {
        panic!("actual organization expected");
    };
    let mut tx = WriteTx::begin(world.db()).await.unwrap();
    let (_, agent) = agents_db::create_organization_agent_in(
        &mut tx,
        &organization.organization.id,
        &sp::NewPrincipal {
            name: "Organization support",
            display: "Organization support",
            description: "",
        },
        &spec.to_string(),
        "u1",
        4,
    )
    .await
    .unwrap();
    let agent = agent.unwrap().principal.id;
    tx.commit().await.unwrap();
    for (kind, reference) in [
        (GrantKind::Model, "support-model"),
        (GrantKind::Connector, "erp"),
    ] {
        sp::add_grant(world.db(), &agent, kind, reference, "u1")
            .await
            .unwrap();
    }
    world.connect_erp(erp).await;
    sqlx::query("UPDATE mcp_catalog_connectors SET scope='per_user',auth='oauth2' WHERE key='erp'")
        .execute(world.db())
        .await
        .unwrap();
    let connector = mcp_catalog::get(world.db(), "erp").await.unwrap().unwrap();
    let sealed = world
        .state
        .crypto
        .seal_str("actual-organization-credential")
        .unwrap();
    let identity = mcp_connections::upsert_connection(
        world.db(),
        mcp_connections::NewConnection {
            catalog_generation: connector.dispatch_generation,
            owner: mcp_connections::ConnectionOwner::Principal(agent.clone()),
            connector_key: "erp".into(),
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
    let mut tx = WriteTx::begin(world.db()).await.unwrap();
    assert!(matches!(
        agent_connections::save_in(
            &mut tx,
            agent_connections::Selection {
                owner: &agents_db::AgentOwner::Organization(organization.organization.id),
                agent_id: &agent,
                connector_key: "erp",
                identity_mode: agent_connections::IdentityMode::Principal,
                connection_generation: &identity.generation,
                expected_binding_generation: None,
            }
        )
        .await
        .unwrap(),
        agent_connections::SaveOutcome::Saved(Some(_))
    ));
    tx.commit().await.unwrap();
    agent
}

async fn organization_otp_pause(world: &World, agent: &str, spec: &Value) -> AgentReply {
    organization_otp_pause_source(
        world,
        agent,
        spec,
        crate::agents::run::draft::PrivateChatSource::Draft,
    )
    .await
}

async fn organization_otp_pause_source(
    world: &World,
    agent: &str,
    spec: &Value,
    source: crate::agents::run::draft::PrivateChatSource,
) -> AgentReply {
    let started = crate::agents::run::draft::start_private_turn(
        &world.state,
        AgentTurn {
            agent_id: agent,
            session_id: None,
            message: "Please verify my details",
            visitor_id: None,
            lang: Some(Lang::En),
        },
        spec,
        source,
        &RunOptions::default(),
        "u1",
    )
    .await
    .unwrap();
    let reply = drive_opened(&world.state, &started.profile, &started.turn)
        .await
        .unwrap();
    assert_eq!(reply.status, chat::TurnStatus::Suspended, "{reply:?}");
    drop(started.claim);
    reply
}

fn restarted_organization_world(world: &World) -> World {
    let app = (**world.state)
        .clone()
        .with_crypto(world.state.crypto.clone());
    let sessions = aiplane_core::rama_server::SessionStore::new(world.db().clone(), [7u8; 32]);
    World {
        state: Arc::new(RamaState::new(
            app,
            sessions,
            aiplane_core::server::usage::UsageHandle::disabled(),
        )),
    }
}

#[tokio::test]
async fn organization_otp_resume_keeps_original_selected_account_and_draft_after_fresh_runtime() {
    let main = llm(vec![
        call("email", "set_email", json!({"value":ALICE})),
        call("otp", REQUEST, json!({})),
        text("Verification complete"),
    ])
    .await;
    let (erp, seen) = erp().await;
    let world = World::new(&[("support-pool", "support-model", &main)], None).await;
    let mut spec = otp_spec(json!({}));
    spec.as_object_mut().unwrap().remove("routes");
    let agent = organization_otp_support(&world, &erp, &spec).await;
    let paused = organization_otp_pause(&world, &agent, &spec).await;
    let stored = chat::get_suspension(world.db(), &paused.turn_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.kind, SuspensionKind::SecureInput);
    let facts = stored
        .tool_call
        .execution_facts
        .clone()
        .expect("original selected account facts");
    let crate::server::tools::mcp::identities::McpDispatchFacts::Owned(identity) =
        serde_json::from_value(facts.clone()).unwrap()
    else {
        panic!("own identity expected")
    };
    assert_eq!(
        identity.connection.owner,
        aiplane_core::server::db::mcp_connections::ConnectionOwner::Principal(agent.clone())
    );
    assert!(identity.binding_generation.is_some());
    assert_eq!(erp_calls(&seen, "send_code"), [json!({"email":ALICE})]);
    let mut edited = spec.clone();
    edited["main"]["instructions"]["orchestration"] =
        json!("Edited draft outside original paused run");
    edited["main"]["tools"] = json!(["company_echo"]);
    agents_db::update_draft(
        world.db(),
        &agent,
        &edited.to_string(),
        "u1",
        Default::default(),
    )
    .await
    .unwrap();
    let mut source_tx = aiplane_core::server::db::WriteTx::begin(world.db())
        .await
        .unwrap();
    assert_eq!(
        crate::agents::profile::private_session_spec_in(
            &mut source_tx,
            &agent,
            &paused.session_id,
            0
        )
        .await
        .unwrap(),
        Some((
            crate::agents::run::draft::PrivateChatSource::Draft,
            spec.clone()
        ))
    );
    drop(source_tx);
    sp::add_grant(world.db(), &agent, GrantKind::Tool, "company_echo", "u1")
        .await
        .unwrap();
    let restarted = restarted_organization_world(&world);
    let claimed = claim(
        &restarted.state,
        AgentResume {
            agent_id: &agent,
            session_id: &paused.session_id,
            turn_id: &paused.turn_id,
            request_id: Some(&stored.request_id),
            decision: Decision::Value { value: json!(CODE) },
            by: ResumedBy::Staff {
                user_id: "u1".into(),
            },
        },
    )
    .await
    .unwrap();
    let done = run_claimed(&restarted.state, claimed, RunOptions::default())
        .await
        .unwrap();
    assert_eq!(done.status, chat::TurnStatus::Completed, "{done:?}");
    assert_eq!(
        erp_calls(&seen, "check_code"),
        [json!({"email":ALICE,"code":CODE})]
    );
    assert!(
        slot_rows(&restarted, &paused.session_id)
            .await
            .iter()
            .any(|(slot, _, writer)| slot == "verified" && writer == "verifier:otp")
    );
    let sent = requests(&main).await;
    assert_eq!(sent.len(), 3);
    assert!(!offered(&sent[2]).contains(&"company_echo"));
    assert!(!system(&sent[2]).contains("Edited draft outside original paused run"));
    assert!(!every_stored_text(restarted.db()).await.contains(CODE));
}

#[tokio::test]
async fn organization_otp_changed_binding_refuses_claim_without_consuming_original_pause() {
    let main = llm(vec![
        call("email", "set_email", json!({"value":ALICE})),
        call("otp", REQUEST, json!({})),
        text("Unexpected continuation"),
    ])
    .await;
    let (erp, seen) = erp().await;
    let world = World::new(&[("support-pool", "support-model", &main)], None).await;
    let mut spec = otp_spec(json!({}));
    spec.as_object_mut().unwrap().remove("routes");
    let agent = organization_otp_support(&world, &erp, &spec).await;
    let paused = organization_otp_pause(&world, &agent, &spec).await;
    let before = chat::get_suspension(world.db(), &paused.turn_id)
        .await
        .unwrap()
        .unwrap();
    sqlx::query("UPDATE agent_connector_bindings SET binding_generation='changed-after-pause' WHERE agent_id=? AND connector_key='erp'")
        .bind(&agent).execute(world.db()).await.unwrap();
    let restarted = restarted_organization_world(&world);
    assert!(
        claim(
            &restarted.state,
            AgentResume {
                agent_id: &agent,
                session_id: &paused.session_id,
                turn_id: &paused.turn_id,
                request_id: Some(&before.request_id),
                decision: Decision::Value { value: json!(CODE) },
                by: ResumedBy::Staff {
                    user_id: "u1".into()
                },
            }
        )
        .await
        .is_err()
    );
    assert_eq!(
        chat::get_suspension(world.db(), &paused.turn_id)
            .await
            .unwrap(),
        Some(before)
    );
    assert!(erp_calls(&seen, "check_code").is_empty());
    assert_eq!(requests(&main).await.len(), 2);
    assert!(
        slot_rows(&restarted, &paused.session_id)
            .await
            .iter()
            .all(|(slot, _, _)| slot != "verified")
    );
}

#[tokio::test]
async fn organization_otp_resume_current_authority_failures_preserve_original_pause() {
    let changes = [
        "DELETE FROM organization_groups",
        "UPDATE agent_owners SET active=0 WHERE principal_id=?",
        "UPDATE system_principals SET disabled_at='2026-10-11T00:00:00Z' WHERE id=?",
        "DELETE FROM principal_grants WHERE principal_id=? AND kind='connector' AND ref='erp'",
        "DELETE FROM principal_grants WHERE principal_id=? AND kind='model' AND ref='support-model'",
        "UPDATE principal_grants SET pools='[]' WHERE principal_id=? AND kind='model' AND ref='support-model'",
        "UPDATE mcp_connections SET generation='replaced-credential' WHERE principal_id=?",
        "UPDATE mcp_connections SET status='error' WHERE principal_id=?",
        "UPDATE mcp_catalog_connectors SET dispatch_generation='replaced-catalogue' WHERE key='erp'",
        "UPDATE chat_sessions SET parent_turn_id=(SELECT id FROM chat_turns WHERE role='user' LIMIT 1) WHERE principal_id=?",
        "UPDATE chat_sessions SET agent_version=1 WHERE principal_id=?",
        "UPDATE chat_turn_suspensions SET run_context=NULL",
        "UPDATE chat_turn_suspensions SET run_context='{}'",
    ];
    for change in changes {
        let main = llm(vec![
            call("email", "set_email", json!({"value": ALICE})),
            call("otp", REQUEST, json!({})),
            text("Unexpected continuation"),
        ])
        .await;
        let (erp, seen) = erp().await;
        let world = World::new(&[("support-pool", "support-model", &main)], None).await;
        let mut spec = otp_spec(json!({}));
        spec.as_object_mut().unwrap().remove("routes");
        let agent = organization_otp_support(&world, &erp, &spec).await;
        let paused = organization_otp_pause(&world, &agent, &spec).await;
        let mut query = sqlx::query(change);
        if change.contains('?') {
            query = query.bind(&agent);
        }
        assert!(
            query.execute(world.db()).await.unwrap().rows_affected() > 0,
            "{change}"
        );
        let before = chat::get_suspension(world.db(), &paused.turn_id)
            .await
            .unwrap()
            .unwrap();
        let restarted = restarted_organization_world(&world);
        let result = claim(
            &restarted.state,
            AgentResume {
                agent_id: &agent,
                session_id: &paused.session_id,
                turn_id: &paused.turn_id,
                request_id: Some(&before.request_id),
                decision: Decision::Value { value: json!(CODE) },
                by: ResumedBy::Staff {
                    user_id: "u1".into(),
                },
            },
        )
        .await;
        assert!(result.is_err(), "{change}");
        assert_eq!(
            chat::get_suspension(world.db(), &paused.turn_id)
                .await
                .unwrap(),
            Some(before),
            "{change}"
        );
        assert!(erp_calls(&seen, "check_code").is_empty(), "{change}");
        assert_eq!(requests(&main).await.len(), 2, "{change}");
        assert!(
            slot_rows(&restarted, &paused.session_id)
                .await
                .iter()
                .all(|(slot, _, _)| slot != "verified"),
            "{change}"
        );
    }
}

#[tokio::test]
async fn organization_otp_private_pause_refuses_participant_but_allows_current_writer() {
    let main = llm(vec![
        call("email", "set_email", json!({"value": ALICE})),
        call("otp", REQUEST, json!({})),
        text("Verification complete"),
    ])
    .await;
    let (erp, seen) = erp().await;
    let world = World::new(&[("support-pool", "support-model", &main)], None).await;
    let mut spec = otp_spec(json!({}));
    spec.as_object_mut().unwrap().remove("routes");
    let agent = organization_otp_support(&world, &erp, &spec).await;
    let paused = organization_otp_pause(&world, &agent, &spec).await;
    let before = chat::get_suspension(world.db(), &paused.turn_id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        sqlx::query("UPDATE chat_sessions SET shared=1 WHERE id=?")
            .bind(&paused.session_id)
            .execute(world.db())
            .await
            .is_err()
    );
    assert_eq!(
        chat::get_suspension(world.db(), &paused.turn_id)
            .await
            .unwrap(),
        Some(before.clone())
    );
    assert!(
        claim(
            &world.state,
            AgentResume {
                agent_id: &agent,
                session_id: &paused.session_id,
                turn_id: &paused.turn_id,
                request_id: Some(&before.request_id),
                decision: Decision::Value { value: json!(CODE) },
                by: ResumedBy::Participant,
            }
        )
        .await
        .is_err()
    );
    assert_eq!(
        chat::get_suspension(world.db(), &paused.turn_id)
            .await
            .unwrap(),
        Some(before.clone())
    );
    assert!(erp_calls(&seen, "check_code").is_empty());
    let claimed = claim(
        &world.state,
        AgentResume {
            agent_id: &agent,
            session_id: &paused.session_id,
            turn_id: &paused.turn_id,
            request_id: Some(&before.request_id),
            decision: Decision::Value { value: json!(CODE) },
            by: ResumedBy::Staff {
                user_id: "u1".into(),
            },
        },
    )
    .await
    .unwrap();
    let completed = run_claimed(&world.state, claimed, RunOptions::default())
        .await
        .unwrap();
    assert_eq!(completed.status, chat::TurnStatus::Completed);
    assert_eq!(
        erp_calls(&seen, "check_code"),
        [json!({"email": ALICE, "code": CODE})]
    );
}

#[tokio::test]
async fn organization_otp_timeout_denies_without_checking_code_or_fabricating_staff() {
    let main = llm(vec![
        call("email", "set_email", json!({"value": ALICE})),
        call("otp", REQUEST, json!({})),
        text("Verification expired"),
    ])
    .await;
    let (erp, seen) = erp().await;
    let world = World::new(&[("support-pool", "support-model", &main)], None).await;
    let mut spec = otp_spec(json!({}));
    spec.as_object_mut().unwrap().remove("routes");
    let agent = organization_otp_support(&world, &erp, &spec).await;
    let paused = organization_otp_pause(&world, &agent, &spec).await;
    sqlx::query(
        "UPDATE chat_turn_suspensions SET expires_at='2000-01-01T00:00:00Z' WHERE turn_id=?",
    )
    .bind(&paused.turn_id)
    .execute(world.db())
    .await
    .unwrap();
    let before = chat::get_suspension(world.db(), &paused.turn_id)
        .await
        .unwrap()
        .unwrap();
    let restarted = restarted_organization_world(&world);
    assert!(
        claim(
            &restarted.state,
            AgentResume {
                agent_id: &agent,
                session_id: &paused.session_id,
                turn_id: &paused.turn_id,
                request_id: Some(&before.request_id),
                decision: Decision::Value { value: json!(CODE) },
                by: ResumedBy::Timeout,
            }
        )
        .await
        .is_err()
    );
    assert_eq!(
        chat::get_suspension(world.db(), &paused.turn_id)
            .await
            .unwrap(),
        Some(before.clone())
    );
    let claimed = claim(
        &restarted.state,
        AgentResume {
            agent_id: &agent,
            session_id: &paused.session_id,
            turn_id: &paused.turn_id,
            request_id: Some(&before.request_id),
            decision: Decision::Deny {
                reason: chat::DenyReason::Timeout,
            },
            by: ResumedBy::Timeout,
        },
    )
    .await
    .unwrap();
    let completed = run_claimed(&restarted.state, claimed, RunOptions::default())
        .await
        .unwrap();
    assert_eq!(completed.status, chat::TurnStatus::Completed);
    assert!(erp_calls(&seen, "check_code").is_empty());
    assert!(
        slot_rows(&restarted, &paused.session_id)
            .await
            .iter()
            .all(|(slot, _, _)| slot != "verified")
    );
    assert!(
        chat::get_suspension(world.db(), &paused.turn_id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(!every_stored_text(world.db()).await.contains(CODE));
}

#[tokio::test]
async fn organization_published_otp_resume_keeps_original_version_model_grants_and_account_after_restart()
 {
    let main = llm(vec![
        call("email", "set_email", json!({"value":ALICE})),
        call("otp", REQUEST, json!({})),
        text("Verification complete"),
    ])
    .await;
    let (erp, seen) = erp().await;
    let world = World::new(&[("support-pool", "support-model", &main)], None).await;
    let mut spec = otp_spec(json!({}));
    spec.as_object_mut().unwrap().remove("routes");
    let agent = organization_otp_support(&world, &erp, &spec).await;
    assert_eq!(
        agents_db::publish(world.db(), &agent, &spec.to_string(), "u1")
            .await
            .unwrap(),
        Some(1)
    );
    let paused = organization_otp_pause_source(
        &world,
        &agent,
        &spec,
        crate::agents::run::draft::PrivateChatSource::Published,
    )
    .await;
    let stored = chat::get_suspension(world.db(), &paused.turn_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.kind, SuspensionKind::SecureInput);
    let facts = stored
        .tool_call
        .execution_facts
        .clone()
        .expect("original selected account facts");
    let crate::server::tools::mcp::identities::McpDispatchFacts::Owned(identity) =
        serde_json::from_value(facts.clone()).unwrap()
    else {
        panic!("own identity expected")
    };
    assert_eq!(
        identity.connection.owner,
        aiplane_core::server::db::mcp_connections::ConnectionOwner::Principal(agent.clone())
    );
    assert!(identity.binding_generation.is_some());
    assert_eq!(erp_calls(&seen, "send_code"), [json!({"email":ALICE})]);
    let mut edited = spec.clone();
    edited["main"]["instructions"]["orchestration"] =
        json!("Edited draft outside original paused run");
    edited["main"]["tools"] = json!(["company_echo"]);
    edited["main"]["model"] = json!("replacement-model");
    assert_eq!(
        agents_db::publish(world.db(), &agent, &edited.to_string(), "u1")
            .await
            .unwrap(),
        Some(2)
    );
    sp::add_grant(
        world.db(),
        &agent,
        GrantKind::Model,
        "replacement-model",
        "u1",
    )
    .await
    .unwrap();
    sp::add_grant(world.db(), &agent, GrantKind::Tool, "company_echo", "u1")
        .await
        .unwrap();
    let restarted = restarted_organization_world(&world);
    let claimed = claim(
        &restarted.state,
        AgentResume {
            agent_id: &agent,
            session_id: &paused.session_id,
            turn_id: &paused.turn_id,
            request_id: Some(&stored.request_id),
            decision: Decision::Value { value: json!(CODE) },
            by: ResumedBy::Staff {
                user_id: "u1".into(),
            },
        },
    )
    .await
    .unwrap();
    let done = run_claimed(&restarted.state, claimed, RunOptions::default())
        .await
        .unwrap();
    assert_eq!(done.status, chat::TurnStatus::Completed, "{done:?}");
    assert_eq!(
        erp_calls(&seen, "check_code"),
        [json!({"email":ALICE,"code":CODE})]
    );
    assert!(
        slot_rows(&restarted, &paused.session_id)
            .await
            .iter()
            .any(|(slot, _, writer)| slot == "verified" && writer == "verifier:otp")
    );
    let sent = requests(&main).await;
    assert_eq!(sent.len(), 3);
    assert_eq!(sent[2]["model"], "support-model");
    assert!(!offered(&sent[2]).contains(&"company_echo"));
    assert!(!system(&sent[2]).contains("Edited draft outside original paused run"));
    assert!(!every_stored_text(restarted.db()).await.contains(CODE));
}

#[tokio::test]
async fn organization_published_otp_changed_or_missing_original_source_keeps_pause_unclaimed() {
    for delete in [false, true] {
        let main = llm(vec![
            call("email", "set_email", json!({"value":ALICE})),
            call("otp", REQUEST, json!({})),
            text("Unexpected continuation"),
        ])
        .await;
        let (erp, seen) = erp().await;
        let world = World::new(&[("support-pool", "support-model", &main)], None).await;
        let mut spec = otp_spec(json!({}));
        spec.as_object_mut().unwrap().remove("routes");
        let agent = organization_otp_support(&world, &erp, &spec).await;
        agents_db::publish(world.db(), &agent, &spec.to_string(), "u1")
            .await
            .unwrap();
        assert!(
            world
                .state
                .agent_specs
                .version(world.db(), &agent, 1)
                .await
                .unwrap()
                .is_some()
        );
        let paused = organization_otp_pause_source(
            &world,
            &agent,
            &spec,
            crate::agents::run::draft::PrivateChatSource::Published,
        )
        .await;
        let before = chat::get_suspension(world.db(), &paused.turn_id)
            .await
            .unwrap()
            .unwrap();
        if delete {
            sqlx::query("DELETE FROM agent_versions WHERE principal_id=? AND version=1")
                .bind(&agent)
                .execute(world.db())
                .await
                .unwrap();
        } else {
            let mut changed = spec;
            changed["main"]["instructions"]["orchestration"] = json!("CHANGED ORIGINAL VERSION");
            sqlx::query("UPDATE agent_versions SET spec=? WHERE principal_id=? AND version=1")
                .bind(changed.to_string())
                .bind(&agent)
                .execute(world.db())
                .await
                .unwrap();
        }
        let restarted = restarted_organization_world(&world);
        assert!(
            claim(
                &restarted.state,
                AgentResume {
                    agent_id: &agent,
                    session_id: &paused.session_id,
                    turn_id: &paused.turn_id,
                    request_id: Some(&before.request_id),
                    decision: Decision::Value { value: json!(CODE) },
                    by: ResumedBy::Staff {
                        user_id: "u1".into()
                    }
                }
            )
            .await
            .is_err()
        );
        assert_eq!(
            chat::get_suspension(world.db(), &paused.turn_id)
                .await
                .unwrap(),
            Some(before)
        );
        assert!(erp_calls(&seen, "check_code").is_empty());
        assert_eq!(requests(&main).await.len(), 2);
    }
}

#[tokio::test]
async fn organization_published_inbox_requires_current_member_write_for_all_private_pause_kinds() {
    use crate::agents::inbox::{self, Viewer};
    use aiplane_core::server::db::{WriteTx, gateway_groups, users};
    let main = llm(vec![
        call("email", "set_email", json!({"value":ALICE})),
        call("otp", REQUEST, json!({})),
    ])
    .await;
    let (erp, _) = erp().await;
    let world = World::new(&[("support-pool", "support-model", &main)], None).await;
    let mut spec = otp_spec(json!({}));
    spec.as_object_mut().unwrap().remove("routes");
    let agent = organization_otp_support(&world, &erp, &spec).await;
    agents_db::publish(world.db(), &agent, &spec.to_string(), "u1")
        .await
        .unwrap();
    let paused = organization_otp_pause_source(
        &world,
        &agent,
        &spec,
        crate::agents::run::draft::PrivateChatSource::Published,
    )
    .await;
    let original = chat::get_suspension(world.db(), &paused.turn_id)
        .await
        .unwrap()
        .unwrap();
    gateway_groups::upsert_group(world.db(), "organization-staff", "", false, false)
        .await
        .unwrap();
    gateway_groups::set_mappings_for_group(world.db(), "organization-staff", &["member".into()])
        .await
        .unwrap();
    gateway_groups::upsert_group(world.db(), "recovery-admin", "", true, false)
        .await
        .unwrap();
    gateway_groups::set_mappings_for_group(world.db(), "recovery-admin", &["admin".into()])
        .await
        .unwrap();
    let now = jiff::Timestamp::now();
    for (id, role) in [
        ("u1", "member"),
        ("reader", "member"),
        ("responder", "member"),
        ("recovery", "admin"),
    ] {
        users::upsert(
            world.db(),
            &users::User {
                id: id.into(),
                email: format!("{id}@example.test"),
                name: None,
                roles: vec![role.into()],
                created_at: now,
                updated_at: now,
                timezone: None,
                speech_voice: None,
            },
        )
        .await
        .unwrap();
    }
    for (id, access) in [
        ("reader", agents_db::Access::Read),
        ("responder", agents_db::Access::Respond),
    ] {
        agents_db::set_share(
            world.db(),
            &agent,
            agents_db::SubjectKind::User,
            id,
            access,
            "u1",
        )
        .await
        .unwrap();
    }
    let viewer = |id: &str| Viewer {
        user_id: id.into(),
        groups: vec!["organization-staff".into()],
    };
    for kind in [
        SuspensionKind::Approval,
        SuspensionKind::HumanAnswer,
        SuspensionKind::SecureInput,
    ] {
        sqlx::query("UPDATE chat_turn_suspensions SET kind=? WHERE turn_id=?")
            .bind(kind.as_str())
            .bind(&paused.turn_id)
            .execute(world.db())
            .await
            .unwrap();
        assert!(
            inbox::find(&world.state, &viewer("u1"), &original.request_id)
                .await
                .unwrap()
                .is_some()
        );
        assert_eq!(
            inbox::list(&world.state, &viewer("u1"))
                .await
                .unwrap()
                .len(),
            1
        );
        for denied in ["reader", "responder", "recovery"] {
            assert!(
                inbox::find(&world.state, &viewer(denied), &original.request_id)
                    .await
                    .unwrap()
                    .is_none(),
                "{kind:?} {denied}"
            );
            assert!(
                inbox::list(&world.state, &viewer(denied))
                    .await
                    .unwrap()
                    .is_empty()
            );
        }
        let mut tx = WriteTx::begin(world.db()).await.unwrap();
        assert_eq!(
            crate::agents::profile::private_session_spec_in(&mut tx, &agent, &paused.session_id, 1)
                .await
                .unwrap(),
            Some((
                crate::agents::run::draft::PrivateChatSource::Published,
                spec.clone()
            ))
        );
        assert!(
            crate::agents::profile::private_session_spec_in(&mut tx, &agent, &paused.session_id, 0)
                .await
                .unwrap()
                .is_none()
        );
        drop(tx);
    }
    gateway_groups::set_mappings_for_group(world.db(), "organization-staff", &[])
        .await
        .unwrap();
    assert!(
        inbox::find(&world.state, &viewer("u1"), &original.request_id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        inbox::list(&world.state, &viewer("u1"))
            .await
            .unwrap()
            .is_empty()
    );
    let mut tx = WriteTx::begin(world.db()).await.unwrap();
    sqlx::query("UPDATE chat_turn_suspensions SET run_context=NULL WHERE turn_id=?")
        .bind(&paused.turn_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    assert!(
        crate::agents::profile::private_session_spec_in(&mut tx, &agent, &paused.session_id, 1)
            .await
            .unwrap()
            .is_none()
    );
    drop(tx);
    assert_eq!(requests(&main).await.len(), 2);
}
