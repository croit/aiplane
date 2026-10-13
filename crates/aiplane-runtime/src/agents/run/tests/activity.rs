// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The activity log (`docs/agent-activity-log.md`) over whole runs: a main agent whose gate is
//! closed until an OTP verifier opens it, a sub-agent, an external A2A
//! agent with a sealed credential, a handoff to a person and the answer —
//! every model exchange, tool call, state write and decision in one hash
//! chain per conversation, enough to reconstruct the run, and no secret in
//! the database. Then the log under parallel turns, and a run whose log
//! cannot be written.

use aiplane_agents::db::agent_audit::{ActivityQuery, AuditKind, Order, StoredEvent};
use aiplane_agents::db::system_principals as sp;
use aiplane_core::server::config::NetworkConfig;
use session_core::db::{Decision, SuspensionKind};

use super::a2a::{Peer, TOKEN, completed, finish_schema};
use super::suspend::{answer, every_stored_text, staff, visitor_says};
use super::verifiers::{ALICE, CODE, erp};
use super::*;
use crate::agents::resume::{AgentResume, ResumedBy, claim, run_claimed};
use crate::agents::spec::secrets::{SPEC_SECRETS, seal_spec_secrets};

const VISITOR: &str = "My March invoice is wrong and I want to know about my warranty.";
const QUESTION: &str = "May we refund the duplicate charge on RE-1?";
const STAFF_ANSWER: &str = "Yes, refund it today.";
const FINAL: &str = "RE-1 was billed twice and is refunded today; you are covered until 2027.";

fn billing_spec() -> Value {
    json!({
        "main": {
            "model": "billing-model",
            "instructions": { "orchestration": "Explain the customer's invoice." },
            "budget": { "rounds": 3 }
        },
        "state": { "finding": { "type": "string", "set_by": ["llm"] } },
        "finish": { "schema": finish_schema() }
    })
}

fn support_spec(billing: &str, card_url: &str) -> Value {
    let verified = json!({ "slot": "verified", "provenance": "verifier:otp" });
    json!({
        "main": {
            "model": "support-model",
            "instructions": { "orchestration": "Verify the visitor, then route the request." },
            "budget": { "rounds": 12 }
        },
        "state": {
            "email": { "type": "email", "set_by": ["llm"] },
            "issue": { "type": "enum", "values": ["billing", "warranty"], "set_by": ["llm"] },
            "verified": { "type": "subject", "set_by": ["verifier:otp"] }
        },
        "verifiers": { "otp": { "kind": "mcp_code", "connector": "erp", "email_slot": "email",
                                "writes": { "verified": "result" } } },
        "router": { "kind": "rules", "order": ["billing", "partner"] },
        "routes": {
            "billing": {
                "when": { "all": [ { "slot": "issue", "eq": "billing" }, verified ] },
                "agent": billing,
                "task": "Invoice question of customer {verified.customer_id}"
            },
            "partner": {
                "when": { "all": [ { "slot": "issue", "eq": "warranty" }, verified ] },
                "task": "Warranty of customer {verified.customer_id}",
                "bind": { "customer": "state.verified.customer_id" },
                "a2a": {
                    "card_url": card_url,
                    "auth": { "kind": "bearer", "token": TOKEN },
                    "finish": { "schema": finish_schema() },
                    "budget": { "seconds": 20 }
                }
            },
            "desk": { "when": verified, "human": { "inbox": "billing desk" } }
        }
    })
}

struct Run {
    world: World,
    support: String,
    billing: String,
    main: Vec<Value>,
    sub: Vec<Value>,
    session: String,
    done: AgentReply,
}

/// The whole support story, from the visitor's first message to the answer
/// after a person decided.
async fn the_story() -> Run {
    let main = llm(vec![
        calls(&[
            ("k1", "set_email", json!({ "value": ALICE })),
            ("k2", "set_issue", json!({ "value": "billing" })),
        ]),
        call("fwd1", "forward_request", json!({})),
        call("v1", "verify_otp_request_code", json!({})),
        call("fwd2", "forward_request", json!({})),
        call("k3", "set_issue", json!({ "value": "warranty" })),
        call("fwd3", "forward_request", json!({})),
        call("h1", "request_human", json!({ "question": QUESTION })),
        text(FINAL),
    ])
    .await;
    let sub = llm(vec![
        call("n1", "set_finding", json!({ "value": "double charge" })),
        finish("b1", json!({ "answer": "RE-1 was billed twice." })),
    ])
    .await;
    let peer = Peer::bearer(vec![completed(json!({ "answer": "Covered until 2027." }))]).await;
    let (erp, _) = erp().await;
    let world = World::build_with(
        &[
            ("support-pool", "support-model", &main),
            ("billing-pool", "billing-model", &sub),
        ],
        None,
        false,
        base_tools(),
        None,
        NetworkConfig {
            allow_private_networks: true,
        },
    )
    .await;
    world.connect_erp(&erp).await;
    let billing = world
        .agent("billing", &[(GrantKind::Model, "billing-model")])
        .await;
    world.publish(&billing, &billing_spec()).await;
    let support = world
        .agent(
            "support",
            &[
                (GrantKind::Model, "support-model"),
                (GrantKind::Connector, "erp"),
                (GrantKind::A2aAgent, &peer.card_url()),
            ],
        )
        .await;
    let mut spec = support_spec(&billing, &peer.card_url());
    seal_spec_secrets(&mut spec, SPEC_SECRETS, &world.state.crypto).unwrap();
    assert_eq!(world.issues(&support, &spec).await, []);
    world.publish(&support, &spec).await;

    let paused = visitor_says(&world, &support, VISITOR).await;
    let waiting = paused.suspension.clone().expect("the code is asked for");
    assert_eq!(waiting.kind, SuspensionKind::SecureInput);
    let claimed = claim(
        &world.state,
        AgentResume {
            agent_id: &support,
            ..answer(
                &paused,
                Decision::Value { value: json!(CODE) },
                ResumedBy::Participant,
            )
        },
    )
    .await
    .unwrap();
    let handed_off = run_claimed(&world.state, claimed, RunOptions::default())
        .await
        .unwrap();
    let waiting = handed_off
        .suspension
        .clone()
        .expect("a person is asked next");
    assert_eq!(waiting.kind, SuspensionKind::HumanAnswer);
    let claimed = claim(
        &world.state,
        AgentResume {
            agent_id: &support,
            ..answer(
                &handed_off,
                Decision::Value {
                    value: json!(STAFF_ANSWER),
                },
                staff(),
            )
        },
    )
    .await
    .unwrap();
    let done = run_claimed(&world.state, claimed, RunOptions::default())
        .await
        .unwrap();
    assert_eq!(done.status, chat::TurnStatus::Completed);
    assert_eq!(done.answer.as_deref(), Some(FINAL));
    Run {
        main: requests(&main).await,
        sub: requests(&sub).await,
        session: paused.session_id.clone(),
        world,
        support,
        billing,
        done,
    }
}

/// Every event of the conversation, oldest first.
async fn conversation(run: &Run) -> Vec<StoredEvent> {
    agent_audit::page(
        run.world.db(),
        &ActivityQuery {
            agent_id: run.support.clone(),
            conversation_id: Some(run.session.clone()),
            order: Order::Asc,
            limit: 10_000,
            max_bytes: usize::MAX,
            ..ActivityQuery::default()
        },
    )
    .await
    .unwrap()
    .events
}

/// `request` with the arguments of every assistant call to one of `tools`
/// replaced by the log's marker, as the log keeps it.
fn with_sensitive_arguments_redacted(mut request: Value, tools: &[&str]) -> Value {
    for message in request["messages"].as_array_mut().unwrap() {
        for call in message
            .get_mut("tool_calls")
            .and_then(Value::as_array_mut)
            .into_iter()
            .flatten()
        {
            if tools.contains(&call["function"]["name"].as_str().unwrap_or_default()) {
                call["function"]["arguments"] = json!({ "redacted": true });
            }
        }
    }
    request
}

fn detail(e: &StoredEvent) -> Value {
    serde_json::from_str(&e.detail).unwrap()
}

fn position(events: &[StoredEvent], what: impl Fn(&StoredEvent, &Value) -> bool) -> usize {
    events
        .iter()
        .position(|e| what(e, &detail(e)))
        .expect("the event is in the log")
}

#[tokio::test]
async fn a_whole_run_is_one_hash_chain_that_reconstructs_it_and_holds_no_secret() {
    let run = the_story().await;
    let events = conversation(&run).await;

    let verified = agent_audit::verify(run.world.db(), &run.support)
        .await
        .unwrap();
    assert!(verified.ok(), "{verified:?}");
    assert_eq!(
        verified.unanchored, 0,
        "every turn's end anchored the conversation"
    );
    let head = verified.head.expect("the agent chain's head is reported");
    assert_eq!(head.chain_key, format!("agent:{}", run.support));
    let anchors: Vec<Value> = agent_audit::for_principal(run.world.db(), &run.support)
        .await
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == "chain_anchored")
        .map(|e| e.detail)
        .collect();
    assert_eq!(anchors.len(), 3, "one per turn: two pauses and the answer");
    assert!(
        anchors
            .iter()
            .all(|a| a["chain_key"] == format!("conversation:{}", run.session))
    );
    let seqs: Vec<i64> = events.iter().map(|e| e.seq.unwrap()).collect();
    assert_eq!(
        seqs,
        (1..=events.len() as i64).collect::<Vec<_>>(),
        "one chain, in the order things happened"
    );
    assert!(
        events
            .iter()
            .all(|e| e.chain_key.as_deref() == Some(&format!("conversation:{}", run.session)))
    );

    // Every model request, exactly as sent, and what came back. A round
    // after the first of its turn is stored as a delta against the one
    // before; the log reads it back whole.
    let stored: Vec<Value> = events
        .iter()
        .filter(|e| e.kind == "llm_exchange" && e.principal_id == run.support)
        .map(detail)
        .collect();
    assert!(
        stored[0].get("request").is_some(),
        "a turn's first round is whole"
    );
    let deltas = stored
        .iter()
        .filter(|d| d.get("request_delta").is_some())
        .count();
    assert!(
        deltas >= stored.len() - 3,
        "every round but each turn's first is a delta: {deltas} of {}",
        stored.len()
    );
    let mut reconstructor = agent_audit::Reconstructor::default();
    let mut read_back = Vec::new();
    for event in &events {
        read_back.push(
            reconstructor
                .event_json(run.world.db(), event)
                .await
                .unwrap(),
        );
    }
    let exchanges = |principal: &str| -> Vec<Value> {
        read_back
            .iter()
            .filter(|e| e["kind"] == "llm_exchange" && e["principal_id"] == principal)
            .map(|e| e["detail"].clone())
            .collect()
    };
    let main_exchanges = exchanges(&run.support);
    assert_eq!(
        main_exchanges
            .iter()
            .map(|e| e["request"].clone())
            .collect::<Vec<_>>(),
        run.main
            .iter()
            .cloned()
            .map(|r| with_sensitive_arguments_redacted(r, &["verify_otp_request_code"]))
            .collect::<Vec<_>>(),
        "the main agent's requests, byte for byte but for the OTP tool's arguments, which \
         it declares sensitive"
    );
    assert_eq!(
        exchanges(&run.billing)
            .iter()
            .map(|e| e["request"].clone())
            .collect::<Vec<_>>(),
        run.sub,
        "the sub-agent's, in the same chain"
    );
    assert_eq!(main_exchanges.last().unwrap()["response"]["content"], FINAL);
    assert_eq!(
        main_exchanges[0]["response"]["tool_calls"][0]["function"]["name"],
        "set_email"
    );

    // Every tool call the main model made has its full result.
    let results: HashMap<String, Value> = events
        .iter()
        .filter(|e| e.kind == "tool_result")
        .map(|e| (e.call_id.clone().unwrap(), detail(e)))
        .collect();
    for id in [
        "k1", "k2", "fwd1", "v1", "fwd2", "k3", "fwd3", "h1", "n1", "b1",
    ] {
        assert!(results.contains_key(id), "no tool_result for {id}");
    }
    assert_eq!(results["k1"]["arguments"], json!({ "value": ALICE }));
    assert_eq!(results["fwd1"]["result"]["reason"], "no_open_route");
    assert_eq!(results["fwd2"]["result"]["outcome"]["status"], "finished");
    assert_eq!(
        results["fwd3"]["result"]["outcome"]["result"]["answer"],
        "Covered until 2027."
    );

    // The turns: what the visitor said, and the answer.
    let started = position(&events, |e, d| {
        e.kind == "turn_started" && d["message"] == VISITOR
    });
    assert_eq!(started, 0);
    let last = events.last().unwrap();
    assert_eq!(last.kind, "turn_finished");
    assert_eq!(detail(last)["answer"], FINAL);
    assert_eq!(last.turn_id.as_deref(), Some(run.done.turn_id.as_str()));

    // The story in order: closed gate, code asked, code received, the
    // verifier's slot written and its outcome, sub-agent, external agent, a
    // person, the answer.
    let order = [
        position(&events, |e, d| {
            e.kind == "route_decision" && d["reason"] == "no_open_route"
        }),
        position(&events, |e, d| {
            e.kind == "run_suspended" && d["kind"] == "secure_input"
        }),
        position(&events, |e, d| {
            e.kind == "run_resumed" && d["secure_input_received"] == true
        }),
        position(&events, |e, d| {
            e.kind == "state_written" && d["slot"] == "verified"
        }),
        position(&events, |e, d| {
            e.kind == "verifier_outcome" && d["outcome"] == "verified"
        }),
        position(&events, |e, d| {
            e.kind == "sub_agent_dispatched" && d["route"] == "billing"
        }),
        position(&events, |e, _| {
            e.kind == "turn_started" && e.principal_id == run.billing
        }),
        position(&events, |e, d| {
            e.kind == "sub_agent_finished" && d["route"] == "billing"
        }),
        position(&events, |e, d| {
            e.kind == "sub_agent_dispatched" && d["target"] == "a2a"
        }),
        position(&events, |e, d| {
            e.kind == "human_handoff" && d["route"] == "desk"
        }),
        position(&events, |e, d| {
            e.kind == "run_resumed" && d["answer"] == STAFF_ANSWER
        }),
        events.len() - 1,
    ];
    assert!(
        order.windows(2).all(|w| w[0] < w[1]),
        "the events follow the run: {order:?}"
    );

    let routed: Vec<Value> = events
        .iter()
        .filter(|e| e.kind == "route_decision" && detail(e)["picked"].is_string())
        .map(detail)
        .collect();
    assert_eq!(routed.len(), 2);
    assert!(routed.iter().all(|d| d["method"] == "rules"), "{routed:?}");

    let issue_writes: Vec<Value> = events
        .iter()
        .filter(|e| e.kind == "state_written" && detail(e)["slot"] == "issue")
        .map(detail)
        .collect();
    assert_eq!(issue_writes.len(), 2);
    assert_eq!(issue_writes[1]["old"]["value"], "billing");
    assert_eq!(issue_writes[1]["new"], "warranty");
    assert_eq!(issue_writes[1]["provenance"], "llm");

    let finding = &events[position(&events, |e, d| {
        e.kind == "state_written" && d["slot"] == "finding"
    })];
    assert_eq!(
        finding.principal_id, run.billing,
        "the sub-agent's slot write is in the root conversation's chain"
    );
    assert_eq!(finding.agent_id.as_deref(), Some(run.support.as_str()));
    assert_ne!(finding.session_id.as_deref(), Some(run.session.as_str()));

    let sub_event = events
        .iter()
        .find(|e| e.principal_id == run.billing)
        .unwrap();
    assert_eq!(sub_event.agent_id.as_deref(), Some(run.support.as_str()));
    let chain: Value = serde_json::from_str(sub_event.chain.as_deref().unwrap()).unwrap();
    assert_eq!(chain["frames"].as_array().unwrap().len(), 2);

    let sent = detail(
        &events[position(&events, |e, d| {
            e.kind == "sub_agent_dispatched" && d["target"] == "a2a"
        })],
    );
    assert_eq!(
        sent["message"]["parts"][1]["data"],
        json!({ "customer": "K-1" }),
        "what left the gateway for the external agent"
    );

    let everything = every_stored_text(run.world.db()).await;
    assert!(!everything.contains(CODE), "the one-time code is nowhere");
    assert!(
        !everything.contains(TOKEN),
        "the external agent's credential is stored sealed only"
    );
    let resumed_secure = detail(&events[order[2]]);
    assert!(resumed_secure.get("answer").is_none());
}

#[tokio::test]
async fn a_changed_event_breaks_the_chain_at_that_event() {
    let run = the_story().await;
    let events = conversation(&run).await;
    let tampered = &events[events.len() / 2];
    sqlx::query("UPDATE agent_audit SET detail = '{}' WHERE id = ?")
        .bind(&tampered.id)
        .execute(run.world.db())
        .await
        .unwrap();
    let broken = agent_audit::verify(run.world.db(), &run.support)
        .await
        .unwrap()
        .broken
        .expect("the change is found");
    assert_eq!(broken.event_id.as_deref(), Some(tampered.id.as_str()));
    assert_eq!(Some(broken.seq), tampered.seq);
}

#[tokio::test]
async fn a_conversation_whose_last_turn_was_cut_from_the_log_fails_verification() {
    let run = the_story().await;
    let last = conversation(&run).await.last().unwrap().seq.unwrap();
    sqlx::query("DELETE FROM agent_audit WHERE conversation_id = ? AND seq > ?")
        .bind(&run.session)
        .bind(last - 3)
        .execute(run.world.db())
        .await
        .unwrap();
    let broken = agent_audit::verify(run.world.db(), &run.support)
        .await
        .unwrap()
        .broken
        .expect("the cut tail is found");
    assert_eq!(broken.chain_key, format!("conversation:{}", run.session));
    assert!(
        broken.reason.contains("newest events were removed"),
        "{broken:?}"
    );
}

/// A pool upstream answering a streamed round with `round` and a plain
/// (non-streamed) call with `answer` as its message content.
async fn rounds_and_calls(round: Value, answer: &str) -> MockServer {
    let answer = answer.to_string();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(move |req: &wiremock::Request| {
            let body: Value = serde_json::from_slice(&req.body).unwrap();
            if body["stream"] == true {
                let sse = format!(
                    "data: {}\n\ndata: [DONE]\n\n",
                    json!({"choices": [{"index": 0, "delta": round}]})
                );
                ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream")
            } else {
                ResponseTemplate::new(200).set_body_json(json!({
                    "choices": [{ "message": { "role": "assistant", "content": answer } }]
                }))
            }
        })
        .mount(&server)
        .await;
    server
}

async fn exchanges_of(world: &World, agent: &str, purpose: &str) -> Vec<StoredEvent> {
    agent_audit::page(
        world.db(),
        &ActivityQuery {
            agent_id: agent.to_string(),
            kinds: vec![AuditKind::LlmExchange],
            order: Order::Asc,
            limit: 1_000,
            max_bytes: usize::MAX,
            ..ActivityQuery::default()
        },
    )
    .await
    .unwrap()
    .events
    .into_iter()
    .filter(|e| detail(e)["purpose"] == purpose)
    .collect()
}

async fn plain_agent(world: &World, extra: &[(GrantKind, &str)], tools: &[&str]) -> String {
    let mut grants = vec![(GrantKind::Model, "support-model")];
    grants.extend_from_slice(extra);
    let agent = world.agent("support", &grants).await;
    world
        .publish(
            &agent,
            &json!({ "main": {
                "model": "support-model",
                "instructions": { "orchestration": "Answer." },
                "tools": tools,
                "budget": { "rounds": 4 }
            } }),
        )
        .await;
    agent
}

#[tokio::test]
async fn an_agent_conversations_compaction_summary_is_an_exchange_of_its_log() {
    let main = rounds_and_calls(text("noted"), "The visitor asked five things.").await;
    let world = World::new(&[("support-pool", "support-model", &main)], None).await;
    let agent = plain_agent(&world, &[], &[]).await;
    let first = visitor_says(&world, &agent, "question 0").await;
    for n in 1..5 {
        run_turn(
            &world.state,
            AgentTurn {
                agent_id: &agent,
                session_id: Some(&first.session_id),
                message: &format!("question {n}"),
                visitor_id: None,
                lang: None,
            },
        )
        .await
        .unwrap();
    }
    sqlx::query("UPDATE chat_turns SET context_tokens = 30000 WHERE session_id = ?")
        .bind(&first.session_id)
        .execute(world.db())
        .await
        .unwrap();
    let principal = sp::load_active(world.db(), &agent).await.unwrap().unwrap();
    let access = aiplane_core::server::upstreams::PoolAccess::for_system_models(
        &principal,
        ["support-model"],
    );
    let unlogged = crate::server::side_call::Payer::agent(&principal, None);

    crate::server::compaction::maybe_autocompact(
        &world.state,
        &first.session_id,
        "support-model",
        &access,
        &unlogged,
        None,
    )
    .await;
    assert!(
        exchanges_of(&world, &agent, "compaction_summary")
            .await
            .is_empty(),
        "without a run nothing is recorded — a person's chat"
    );
    sqlx::query("DELETE FROM chat_compactions")
        .execute(world.db())
        .await
        .unwrap();

    let log = crate::agents::audit::RunLog::conversation(&principal, 1, &first.session_id);
    let payer = crate::server::side_call::Payer::agent(&principal, Some(log.chain()));
    crate::server::compaction::maybe_autocompact(
        &world.state,
        &first.session_id,
        "support-model",
        &access,
        &payer,
        Some(log),
    )
    .await;
    let summaries = exchanges_of(&world, &agent, "compaction_summary").await;
    assert_eq!(summaries.len(), 1);
    let d = detail(&summaries[0]);
    let asked = d["request"]["messages"][1]["content"].as_str().unwrap();
    assert!(asked.contains("question 0"), "{asked}");
    assert!(asked.ends_with("/no_think"), "{asked}");
    assert_eq!(
        d["request"]["chat_template_kwargs"]["enable_thinking"], false,
        "the side call switches reasoning off"
    );
    assert_eq!(
        d["response"]["body"]["choices"][0]["message"]["content"],
        "The visitor asked five things."
    );
    assert_eq!(
        summaries[0].conversation_id.as_deref(),
        Some(first.session_id.as_str())
    );
    assert!(agent_audit::verify(world.db(), &agent).await.unwrap().ok());
}

#[tokio::test]
async fn the_rubric_judges_exchange_is_logged_in_the_case_conversation() {
    let main = rounds_and_calls(text("hi"), r#"{"passed": true, "reason": "it greeted"}"#).await;
    let world = World::new(&[("support-pool", "support-model", &main)], None).await;
    let agent = plain_agent(&world, &[], &[]).await;
    let spec = crate::agents::spec::AgentSpec::from_value(&json!({ "main": {
        "model": "support-model", "instructions": { "orchestration": "Answer." }
    } }))
    .unwrap();
    let judge =
        crate::agents::eval_judge::ModelJudge::for_agent(world.state.clone(), &agent, &spec)
            .await
            .expect("a judge on the agent's model");
    let verdict = crate::agents::eval::RubricJudge::judge(
        &judge,
        "greets",
        &[crate::agents::eval::Exchange {
            visitor: "hello".into(),
            agent: Some("hi".into()),
        }],
        "case-conversation",
    )
    .await
    .unwrap();
    assert!(verdict.passed);
    let judged = exchanges_of(&world, &agent, "rubric_judge").await;
    assert_eq!(judged.len(), 1);
    assert_eq!(
        judged[0].conversation_id.as_deref(),
        Some("case-conversation")
    );
    assert_eq!(judged[0].version, Some(0), "the evaluation's draft run");
    let request = &detail(&judged[0])["request"];
    assert!(request.to_string().contains("greets"));
    assert_eq!(
        request["chat_template_kwargs"]["enable_thinking"], false,
        "the judge switches reasoning off like every side call"
    );
}

/// A tool whose arguments carry what a visitor must not find in a log.
struct Secretive;

const SECRET_ARG: &str = "pin-4711-never-logged";

impl crate::server::tools::Tool for Secretive {
    fn id(&self) -> &str {
        "secretive"
    }

    fn schema(&self) -> shared::api::ToolDef {
        shared::api::ToolDef::function(
            "secretive",
            "Check a pin.",
            json!({ "type": "object", "properties": { "pin": { "type": "string" } } }),
        )
    }

    fn sensitive_args(&self) -> bool {
        true
    }

    fn run<'a>(
        &'a self,
        _ctx: crate::server::tools::ToolContext,
        _args: Value,
    ) -> crate::server::tools::ToolFuture<'a> {
        Box::pin(async move { Ok(json!({ "ok": true })) })
    }
}

#[tokio::test]
async fn a_sensitive_tools_arguments_stay_out_of_every_model_exchange() {
    let main = llm(vec![
        call("s1", "secretive", json!({ "pin": SECRET_ARG })),
        text("Checked."),
    ])
    .await;
    let world = World::build(
        &[("support-pool", "support-model", &main)],
        None,
        false,
        base_tools().with(Secretive),
        None,
    )
    .await;
    let agent = plain_agent(&world, &[(GrantKind::Tool, "secretive")], &["secretive"]).await;

    let reply = visitor_says(&world, &agent, "Check my pin.").await;
    assert_eq!(reply.answer.as_deref(), Some("Checked."));
    assert!(
        requests(&main).await[1].to_string().contains(SECRET_ARG),
        "the model itself was sent its own call back"
    );
    let rounds = exchanges_of(&world, &agent, "round").await;
    assert_eq!(rounds.len(), 2);
    assert_eq!(
        detail(&rounds[0])["response"]["tool_calls"][0]["function"]["arguments"],
        json!({ "redacted": true }),
        "the marker the tool_result keeps"
    );
    let stored: Vec<String> = sqlx::query_scalar("SELECT detail FROM agent_audit")
        .fetch_all(world.db())
        .await
        .unwrap();
    assert!(
        stored.iter().all(|d| !d.contains(SECRET_ARG)),
        "no event of the log holds the sensitive arguments"
    );
}

/// A tool that returns an image for the model to see.
struct Snapshot;

const IMAGE: &str = "data:image/png;base64,iVBORw0KGgo=";

impl crate::server::tools::Tool for Snapshot {
    fn id(&self) -> &str {
        "snapshot"
    }

    fn schema(&self) -> shared::api::ToolDef {
        shared::api::ToolDef::function(
            "snapshot",
            "Take a snapshot.",
            json!({ "type": "object", "properties": {} }),
        )
    }

    fn run<'a>(
        &'a self,
        _ctx: crate::server::tools::ToolContext,
        _args: Value,
    ) -> crate::server::tools::ToolFuture<'a> {
        Box::pin(async move {
            Ok(crate::server::tools::tool_content_parts(vec![json!({
                "type": "image_url", "image_url": { "url": IMAGE }
            })]))
        })
    }
}

#[tokio::test]
async fn the_vision_fallbacks_description_is_an_exchange_of_the_tool_call() {
    let main = llm(vec![
        call("p1", "snapshot", json!({})),
        text("A red square."),
    ])
    .await;
    let vision = rounds_and_calls(text("unused"), "a red square").await;
    let world = World::build(
        &[
            ("support-pool", "support-model", &main),
            ("vision-pool", "vision-model", &vision),
        ],
        None,
        false,
        base_tools().with(Snapshot),
        None,
    )
    .await;
    aiplane_core::server::db::model_defaults::set_capabilities(
        world.db(),
        "support-model",
        &aiplane_core::server::db::model_defaults::ModelCapabilities {
            vision: Some(false),
            fallback_vision: Some("vision-model".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let agent = plain_agent(&world, &[(GrantKind::Tool, "snapshot")], &["snapshot"]).await;

    let reply = visitor_says(&world, &agent, "What do you see?").await;
    assert_eq!(reply.answer.as_deref(), Some("A red square."));
    let described = exchanges_of(&world, &agent, "vision_fallback").await;
    assert_eq!(described.len(), 1);
    assert_eq!(described[0].call_id.as_deref(), Some("p1"));
    let d = detail(&described[0]);
    assert!(
        d["request"].to_string().contains(IMAGE),
        "the image as sent"
    );
    assert_eq!(
        d["response"]["body"]["choices"][0]["message"]["content"],
        "a red square"
    );
    assert!(agent_audit::verify(world.db(), &agent).await.unwrap().ok());
}

/// Answers a round that has a tool result with text, any other with a call
/// to the echo tool — the same for every conversation, whatever order their
/// rounds arrive in.
struct EchoThenAnswer;

impl wiremock::Respond for EchoThenAnswer {
    fn respond(&self, req: &wiremock::Request) -> ResponseTemplate {
        let body: Value = serde_json::from_slice(&req.body).unwrap();
        let answered = body["messages"]
            .as_array()
            .unwrap()
            .last()
            .is_some_and(|m| m["role"] == "tool");
        let delta = if answered {
            text("done")
        } else {
            call("e1", "company_echo", json!({ "message": "ping" }))
        };
        let sse = format!(
            "data: {}\n\ndata: [DONE]\n\n",
            json!({"choices": [{"index": 0, "delta": delta}]})
        );
        ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream")
    }
}

/// An agent that echoes once and answers, on a database file (parallel
/// writers need real locking), which lives as long as the returned dir.
async fn echoing_agent() -> (World, String, MockServer, tempfile::TempDir) {
    let main = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(EchoThenAnswer)
        .mount(&main)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let world = World::build(
        &[("support-pool", "support-model", &main)],
        None,
        false,
        base_tools(),
        Some(&dir.path().join("db.sqlite")),
    )
    .await;
    let agent = world
        .agent(
            "support",
            &[
                (GrantKind::Model, "support-model"),
                (GrantKind::Tool, "company_echo"),
            ],
        )
        .await;
    world
        .publish(
            &agent,
            &json!({ "main": {
                "model": "support-model",
                "instructions": { "orchestration": "Echo, then answer." },
                "tools": ["company_echo"],
                "budget": { "rounds": 4 }
            } }),
        )
        .await;
    (world, agent, main, dir)
}

#[tokio::test]
async fn parallel_turns_lose_no_event_and_keep_every_chain_whole() {
    let (world, agent, main, _dir) = echoing_agent().await;
    let world = Arc::new(world);
    let turns: Vec<_> = (0..6)
        .map(|n| {
            let (world, agent) = (world.clone(), agent.clone());
            tokio::spawn(async move {
                let message = format!("visitor {n}");
                visitor_says(&world, &agent, &message).await
            })
        })
        .collect();
    let mut sessions = Vec::new();
    for turn in turns {
        let reply = turn.await.unwrap();
        assert_eq!(reply.status, chat::TurnStatus::Completed);
        sessions.push(reply.session_id);
    }

    let verified = agent_audit::verify(world.db(), &agent).await.unwrap();
    assert!(verified.ok(), "{verified:?}");
    for session in &sessions {
        let kinds: Vec<String> = agent_audit::page(
            world.db(),
            &ActivityQuery {
                agent_id: agent.clone(),
                conversation_id: Some(session.clone()),
                order: Order::Asc,
                limit: 1_000,
                max_bytes: usize::MAX,
                ..ActivityQuery::default()
            },
        )
        .await
        .unwrap()
        .events
        .into_iter()
        .map(|e| e.kind)
        .collect();
        assert_eq!(
            kinds,
            [
                "turn_started",
                "llm_exchange",
                "tool_call",
                "tool_result",
                "llm_exchange",
                "turn_finished"
            ],
            "conversation {session}"
        );
    }
    assert_eq!(
        requests(&main).await.len(),
        12,
        "every exchange the upstream saw is in the log"
    );
}

/// The log refuses every write of `kind` (every kind for `None`), as a full
/// disk or a broken database would.
async fn refuse_writes(world: &World, kind: Option<&str>) {
    let when = kind.map_or(String::new(), |k| format!("WHEN NEW.kind = '{k}'"));
    sqlx::query(&format!(
        "CREATE TRIGGER refuse_activity BEFORE INSERT ON agent_audit {when}
         BEGIN SELECT RAISE(ABORT, 'disk I/O error'); END"
    ))
    .execute(world.db())
    .await
    .unwrap();
}

#[tokio::test]
async fn a_run_whose_log_cannot_be_written_stops_before_the_model_is_asked() {
    let (world, agent, main, _dir) = echoing_agent().await;
    refuse_writes(&world, None).await;

    let reply = visitor_says(&world, &agent, "hello").await;
    assert_eq!(reply.status, chat::TurnStatus::Errored);
    assert_eq!(
        reply.error.as_deref(),
        Some(crate::agents::audit::LOG_UNAVAILABLE)
    );
    assert!(
        requests(&main).await.is_empty(),
        "nothing happened that the log could not show"
    );
}

#[tokio::test]
async fn a_lost_tool_result_stops_the_run_before_its_next_round() {
    let (world, agent, main, _dir) = echoing_agent().await;
    refuse_writes(&world, Some("tool_result")).await;

    let reply = visitor_says(&world, &agent, "hello").await;
    assert_eq!(reply.status, chat::TurnStatus::Errored);
    assert_eq!(
        reply.error.as_deref(),
        Some(
            session_core::driver::TurnError::Upstream {
                message: crate::agents::audit::LOG_UNAVAILABLE.into(),
            }
            .to_string()
            .as_str()
        )
    );
    assert_eq!(
        requests(&main).await.len(),
        1,
        "the round after the unrecorded result never went out"
    );
}
