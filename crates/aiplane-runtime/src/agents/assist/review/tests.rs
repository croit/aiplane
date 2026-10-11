// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use std::collections::HashMap;

use aiplane_core::server::principal::{GrantKind, GrantSet};
use serde_json::{Value, json};

use super::*;
use crate::agents::assist::proposal::{suggest_input, suggest_schema};
use crate::agents::assist::{Ability, Candidates, Knowledge, Target};
use std::collections::BTreeMap;

const AGENT: &str = "self-agent";

fn tool(id: &str) -> Ability {
    Ability {
        id: id.into(),
        name: id.into(),
        description: None,
    }
}

fn candidates() -> Candidates {
    Candidates {
        abilities: vec![
            Ability {
                id: "get_current_timestamp".into(),
                name: "Current time".into(),
                description: Some("The time now".into()),
            },
            tool("rag_search"),
            tool("rag_list_collections"),
            tool("search_web"),
        ],
        knowledge: vec![
            Knowledge {
                id: "7".into(),
                name: "Ceph docs".into(),
                description: None,
            },
            Knowledge {
                id: "9".into(),
                name: "Price list".into(),
                description: None,
            },
        ],
        agents: vec![Target {
            id: "billing-agent".into(),
            name: "Billing".into(),
        }],
        models: vec!["main-model".into(), "fast-model".into()],
    }
}

struct World {
    action_catalogues: BTreeMap<String, BTreeSet<String>>,
    grants: GrantSet,
    agents: HashMap<String, bool>,
    live: HashMap<String, Value>,
    candidates: Candidates,
}

impl World {
    fn new() -> Self {
        Self {
            action_catalogues: BTreeMap::new(),
            grants: GrantSet::new([(GrantKind::Model, "main-model".to_string())]),
            agents: HashMap::from([
                (AGENT.to_string(), false),
                ("billing-agent".to_string(), true),
            ]),
            live: HashMap::from([(
                "billing-agent".to_string(),
                json!({ "finish": { "schema": { "type": "object" } } }),
            )]),
            candidates: candidates(),
        }
    }

    fn ctx(&self) -> ReviewContext<'_> {
        ReviewContext {
            action_catalogues: &self.action_catalogues,
            agent_id: AGENT,
            grants: &self.grants,
            agents: &self.agents,
            live_specs: &self.live,
            candidates: &self.candidates,
            allow_private: false,
        }
    }
}

fn good() -> Value {
    json!({
        "task": "You help customers of Acme with orders. First ask for the order number.",
        "tone": { "response": "Use the customer's name.",
                  "chips": ["friendly", "brief", ""], "language": "visitor" },
        "scope": { "topics": ["Acme orders", "Acme shipping"],
                   "refusal": "I can only help with Acme orders.", "strict": true },
        "abilities": [{ "id": "get_current_timestamp", "why": "to tell delivery times" }],
        "slots": [
            { "name": "Order Number", "label": "The order number", "type": "text", "choices": [] },
            { "name": "issue", "label": "What it is about", "type": "choice",
              "choices": ["billing", "delivery"] },
            { "name": "email", "label": "Email", "type": "email", "choices": [] }
        ],
        "identity": { "method": "email_code", "why": "orders are personal" },
        "handoffs": [
            { "name": "billing", "topic": "Invoice questions", "condition": "always",
              "target": "billing-agent" },
            { "name": "people", "topic": "Qualified lead", "condition": "details",
              "target": "human" }
        ],
        "tests": [
            { "name": "asks for an order", "kind": "in_scope",
              "messages": ["Where is my order?"], "answer_contains": ["order"],
              "answer_not_contains": [] },
            { "name": "off topic", "kind": "out_of_scope",
              "messages": ["How do I repair a diesel engine?"], "answer_contains": [],
              "answer_not_contains": [] },
            { "name": "greets", "kind": "in_scope", "messages": ["Hi"],
              "answer_contains": [], "answer_not_contains": [] }
        ]
    })
}

/// The draft the UI builds by applying every offered step to `base`.
fn apply(base: &Value, steps: &Steps) -> Value {
    let mut d = base.clone();
    if let Some(t) = &steps.task {
        set_at(
            &mut d,
            &["main", "instructions", "orchestration"],
            json!(t.orchestration),
        );
    }
    if let Some(t) = &steps.tone {
        set_at(
            &mut d,
            &["main", "instructions", "response"],
            json!(tone::response_text(
                &t.chips,
                t.language.as_deref(),
                &t.response
            )),
        );
    }
    if let Some(s) = &steps.scope {
        set_at(
            &mut d,
            &["scope"],
            json!({ "topics": s.topics, "refusal": s.refusal, "strict": s.strict }),
        );
    }
    let tools: Vec<&str> = steps.abilities.iter().map(|a| a.id.as_str()).collect();
    set_at(&mut d, &["main", "tools"], json!(tools));
    for s in &steps.slots {
        set_at(&mut d, &["state", &s.name], s.def.clone());
    }
    for h in &steps.handoffs {
        set_at(&mut d, &["routes", &h.name], h.route.clone());
    }
    if !steps.handoffs.is_empty() {
        let topics: Vec<&str> = steps.handoffs.iter().map(|h| h.topic.as_str()).collect();
        set_at(
            &mut d,
            &["state", "topic"],
            json!({ "type": "enum", "values": topics, "set_by": ["llm"] }),
        );
        set_at(
            &mut d,
            &["state", "request"],
            json!({ "type": "string", "max_length": 2000, "set_by": ["llm"] }),
        );
    }
    d
}

#[test]
fn a_good_proposal_maps_to_a_draft_that_passes_the_validator() {
    let w = World::new();
    let base = json!({ "main": { "model": "main-model" } });
    let out = review(&good(), &base, &w.ctx());
    assert!(out.dropped.is_empty(), "{:#?}", out.dropped);
    let s = &out.steps;
    let tone = s.tone.as_ref().unwrap();
    assert_eq!(tone.chips, ["friendly", "brief"]);
    assert_eq!(tone.language.as_deref(), Some("visitor"));
    assert_eq!(tone.response, "Use the customer's name.");
    assert_eq!(s.abilities[0].name, "Current time");
    assert_eq!(
        s.slots.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(),
        ["order_number", "issue", "email"]
    );
    assert_eq!(s.identity.as_ref().unwrap().method, "email_code");
    assert_eq!(s.handoffs[0].route["agent"], "billing-agent");
    assert_eq!(s.handoffs[0].target_name, "Billing");
    assert!(!s.handoffs[0].details && !s.handoffs[0].identity);
    assert_eq!(
        s.handoffs[1].route["when"]["all"],
        json!([
            { "slot": "topic", "eq": "Qualified lead" },
            { "slot": "request", "set": true },
            { "slot": "email", "set": true },
            { "slot": "issue", "set": true },
            { "slot": "order_number", "set": true },
        ]),
        "it waits for every detail the slots step offered"
    );
    assert!(s.handoffs[1].details);
    assert_eq!(s.handoffs[1].target_name, "a person");
    assert_eq!(s.tests.len(), 3);
    assert_eq!(
        s.tests[1].expect["answer"]["contains"],
        json!(["I can only help with Acme orders."])
    );
    for t in &s.tests {
        eval::parse_case(&t.script, &t.expect).unwrap();
    }

    let draft = apply(&base, s);
    let granted = GrantSet::new(
        w.grants
            .iter()
            .map(|(k, r)| (k, r.to_string()))
            .chain([(GrantKind::Tool, "get_current_timestamp".to_string())]),
    );
    let ctx = SpecContext {
        action_catalogues: &Default::default(),
        agent_id: AGENT,
        grants: &granted,
        agents: &w.agents,
        live_specs: &w.live,
        model_defaults: &Default::default(),
        speech_voices: &HashMap::new(),
        allow_private: false,
    };
    spec::check(&draft, &ctx, Stage::Draft).unwrap();
    spec::check(&draft, &ctx, Stage::Publish).unwrap();
    assert_eq!(
        s.offered(),
        [
            "task",
            "tone",
            "scope",
            "abilities",
            "slots",
            "identity",
            "handoffs",
            "tests"
        ]
    );
}

#[test]
fn tone_chips_are_the_setups_ids_and_anything_else_is_left_out() {
    let w = World::new();
    let mut answer = good();
    answer["tone"] = json!({ "chips": ["freundlich", "formal", "formal"], "language": "de",
                             "response": "" });
    let out = review(&answer, &json!({}), &w.ctx());
    let tone = out.steps.tone.unwrap();
    assert_eq!(tone.chips, ["formal"]);
    assert_eq!(tone.language.as_deref(), Some("de"));
    let dropped = out.dropped.iter().find(|d| d.step == "tone").unwrap();
    assert_eq!(dropped.item.as_deref(), Some("freundlich"));

    answer["tone"] = json!({ "chips": ["nett"], "language": "none", "response": " " });
    let out = review(&answer, &json!({}), &w.ctx());
    assert!(out.steps.tone.is_none());
    assert!(out.dropped.iter().any(|d| d.reason.contains("no tone")));
}

#[test]
fn an_architects_tone_is_written_as_the_setups_lines() {
    let w = World::new();
    let changes = json!({ "tone": { "chips": ["brief"], "language": "de",
                                    "response": "Sign as Lena." } });
    let out = apply_changes(&changes, &json!({}), &w.ctx());
    assert_eq!(
        out.draft["main"]["instructions"]["response"],
        "Keep answers short and to the point.\nAlways answer in German.\nSign as Lena."
    );
}

#[test]
fn knowledge_is_offered_by_knowledge_base_never_as_the_search_tools() {
    let w = World::new();
    let mut answer = good();
    answer["abilities"] = json!([
        { "id": "rag_search", "why": "to look things up" },
        { "id": "search_web", "why": "news" }
    ]);
    answer["knowledge"] = json!([
        { "name": "ceph docs", "why": "product questions" },
        { "name": "Pricing wiki", "why": "prices" }
    ]);
    answer["missing_knowledge"] = json!(["croit support contracts", " "]);
    let out = review(&answer, &json!({}), &w.ctx());
    let s = &out.steps;
    assert_eq!(
        s.abilities
            .iter()
            .map(|a| (a.id.as_str(), a.name.as_str()))
            .collect::<Vec<_>>(),
        [("search_web", "search_web")],
        "a tool is named as the candidates name it, never made up from its id"
    );
    assert_eq!(
        s.knowledge
            .iter()
            .map(|k| (k.id.as_str(), k.name.as_str()))
            .collect::<Vec<_>>(),
        [("7", "Ceph docs")]
    );
    assert_eq!(s.missing_knowledge, ["croit support contracts"]);
    let dropped: Vec<(&str, Option<&str>)> = out
        .dropped
        .iter()
        .map(|d| (d.step, d.item.as_deref()))
        .collect();
    assert_eq!(
        dropped,
        [
            ("abilities", Some("rag_search")),
            ("knowledge", Some("Pricing wiki"))
        ]
    );
    assert!(s.offered().contains(&"knowledge"));
}

#[test]
fn knowledge_is_wired_the_way_the_abilities_step_wires_it() {
    let w = World::new();
    let changes = json!({ "knowledge": [{ "name": "Ceph docs", "why": "" }] });
    let one = apply_changes(&changes, &json!({}), &w.ctx());
    assert_eq!(one.draft["main"]["tools"], json!(["rag_search"]));
    assert_eq!(
        one.draft["main"]["tool_resources"]["rag_search"]["bind"]["collection"],
        json!({ "const": "Ceph docs" })
    );
    assert_eq!(
        one.grants,
        [
            (GrantKind::RagCollection, "7".to_string()),
            (GrantKind::Tool, "rag_search".to_string()),
        ]
    );

    let changes = json!({ "knowledge": [{ "name": "Ceph docs", "why": "" },
                                        { "name": "Price list", "why": "" }] });
    let two = apply_changes(&changes, &one.draft, &w.ctx());
    assert_eq!(
        two.draft["main"]["tools"],
        json!(["rag_search", "rag_list_collections"])
    );
    assert!(
        two.draft["main"]["tool_resources"]["rag_search"]["bind"]
            .get("collection")
            .is_none()
    );
}

#[test]
fn an_ability_the_manager_may_not_grant_is_never_offered() {
    let w = World::new();
    let mut answer = good();
    answer["abilities"] = json!([
        { "id": "run_in_sandbox", "why": "to compute" },
        { "id": "get_current_timestamp", "why": "time" }
    ]);
    let out = review(&answer, &json!({}), &w.ctx());
    assert_eq!(
        out.steps
            .abilities
            .iter()
            .map(|a| a.id.as_str())
            .collect::<Vec<_>>(),
        ["get_current_timestamp"]
    );
    let dropped = out.dropped.iter().find(|d| d.step == "abilities").unwrap();
    assert_eq!(dropped.item.as_deref(), Some("run_in_sandbox"));
    assert!(dropped.reason.contains("may grant"), "{}", dropped.reason);
}

#[test]
fn an_invalid_piece_is_dropped_with_the_validators_reason_and_the_rest_kept() {
    let w = World::new();
    let mut answer = good();
    answer["slots"][1]["choices"] = json!([]);
    answer["slots"].as_array_mut().unwrap().push(json!(
        { "name": "mood", "label": "", "type": "colour", "choices": [] }
    ));
    let out = review(&answer, &json!({}), &w.ctx());
    let names: Vec<&str> = out.steps.slots.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["order_number", "email"]);
    let issue = out
        .dropped
        .iter()
        .find(|d| d.item.as_deref() == Some("issue"))
        .unwrap();
    assert!(
        issue.reason.contains("state.issue.values"),
        "{}",
        issue.reason
    );
    let mood = out
        .dropped
        .iter()
        .find(|d| d.item.as_deref() == Some("mood"))
        .unwrap();
    assert!(mood.reason.contains("colour"), "{}", mood.reason);
    assert_eq!(
        out.steps.handoffs[1].route["when"]["all"]
            .as_array()
            .unwrap()
            .len(),
        4,
        "the lead waits for the details that were kept, not for `issue`"
    );
}

#[test]
fn a_hand_off_only_reaches_an_agent_shared_with_the_manager_or_a_person() {
    let w = World::new();
    let mut answer = good();
    answer["handoffs"][0]["target"] = json!("someone-elses-agent");
    answer["handoffs"].as_array_mut().unwrap().push(json!(
        { "name": "me", "topic": "loop", "condition": "always", "target": AGENT }
    ));
    let out = review(&answer, &json!({}), &w.ctx());
    let targets: Vec<&str> = out
        .steps
        .handoffs
        .iter()
        .map(|h| h.target.as_str())
        .collect();
    assert_eq!(targets, ["human"]);
    assert_eq!(
        out.dropped.iter().filter(|d| d.step == "handoffs").count(),
        2,
        "{:#?}",
        out.dropped
    );
}

#[test]
fn a_step_that_does_not_read_is_dropped_whole() {
    let w = World::new();
    let mut answer = good();
    answer["tone"] = json!("friendly");
    answer["identity"] = json!({ "method": "palm_reading", "why": "" });
    let out = review(&answer, &json!({}), &w.ctx());
    assert!(out.steps.tone.is_none());
    assert!(out.steps.identity.is_none());
    assert!(out.steps.task.is_some());
    let steps: Vec<&str> = out.dropped.iter().map(|d| d.step).collect();
    assert_eq!(steps, ["tone", "identity"]);
}

#[test]
fn tests_need_a_message_a_unique_name_and_a_refusal_to_expect() {
    let w = World::new();
    let mut answer = good();
    answer["scope"] = Value::Null;
    answer["tests"].as_array_mut().unwrap().extend([
        json!({ "name": "greets", "kind": "in_scope", "messages": ["Hello"],
                "answer_contains": [], "answer_not_contains": [] }),
        json!({ "name": "silent", "kind": "in_scope", "messages": [" "],
                "answer_contains": [], "answer_not_contains": [] }),
    ]);
    let out = review(&answer, &json!({}), &w.ctx());
    let kept: Vec<&str> = out.steps.tests.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(kept, ["asks for an order", "greets"]);
    let dropped: Vec<&str> = out
        .dropped
        .iter()
        .filter(|d| d.step == "tests")
        .filter_map(|d| d.item.as_deref())
        .collect();
    assert_eq!(dropped, ["off topic", "greets", "silent"]);
}

#[test]
fn at_most_six_tests_are_offered() {
    let w = World::new();
    let mut answer = good();
    answer["tests"] = (0..8)
        .map(|i| {
            json!({ "name": format!("case {i}"), "kind": "in_scope", "messages": ["Hi"],
                    "answer_contains": [], "answer_not_contains": [] })
        })
        .collect();
    let out = review(&answer, &json!({}), &w.ctx());
    assert_eq!(out.steps.tests.len(), MAX_TESTS);
    assert_eq!(out.dropped.iter().filter(|d| d.step == "tests").count(), 2);
}

#[test]
fn an_existing_slot_or_route_is_kept_rather_than_replaced() {
    let w = World::new();
    let base = json!({ "state": { "email": { "type": "email", "set_by": ["llm"] } } });
    let out = review(&good(), &base, &w.ctx());
    assert!(out.steps.slots.iter().all(|s| s.name != "email"));
    assert!(
        out.dropped
            .iter()
            .any(|d| d.item.as_deref() == Some("email"))
    );
    let existing = json!({ "routes": { "people": { "when": { "slot": "email", "set": true },
                                                   "human": {} } },
                          "state": { "email": { "type": "email", "set_by": ["llm"] } } });
    let out = review(&good(), &existing, &w.ctx());
    assert_eq!(out.steps.handoffs.len(), 2, "{:#?}", out.dropped);
    let mut lead = existing.clone();
    lead["routes"] = json!({});
    super::super::handoffs::write(
        &mut lead,
        &super::super::handoffs::Handoffs {
            rules: vec![Rule {
                route: None,
                topic: "Qualified lead".into(),
                details: true,
                identity: false,
                target: super::super::handoffs::Target::Human,
                bind: Map::new(),
            }],
            ..Default::default()
        },
    );
    let out = review(&good(), &lead, &w.ctx());
    assert!(
        out.steps
            .handoffs
            .iter()
            .all(|h| h.topic != "Qualified lead")
    );
    assert!(
        out.dropped
            .iter()
            .any(|d| d.item.as_deref() == Some("people") && d.reason.contains("already")),
        "{:#?}",
        out.dropped
    );
}

#[test]
fn a_draft_that_is_already_invalid_does_not_sink_the_proposal() {
    let w = World::new();
    let base = json!({ "main": { "tools": ["not_granted"] } });
    let out = review(&good(), &base, &w.ctx());
    assert!(out.dropped.is_empty(), "{:#?}", out.dropped);
}

#[test]
fn names_become_spec_identifiers() {
    assert_eq!(ident("Order Number").as_deref(), Some("order_number"));
    assert_eq!(ident("  2nd-Address! ").as_deref(), Some("nd_address"));
    assert_eq!(ident("___").as_deref(), None);
    assert_eq!(ident(&"a".repeat(80)).unwrap().len(), MAX_IDENT_LEN);
}

#[test]
fn the_schema_offers_exactly_the_managers_abilities_and_targets() {
    let schema = suggest_schema(&candidates());
    let ability = &schema["properties"]["abilities"]["items"]["properties"]["id"];
    assert_eq!(
        ability["enum"],
        json!(["get_current_timestamp", "search_web"])
    );
    let knowledge = &schema["properties"]["knowledge"]["items"]["properties"]["name"];
    assert_eq!(knowledge["enum"], json!(["Ceph docs", "Price list"]));
    let target = &schema["properties"]["handoffs"]["items"]["properties"]["target"];
    assert_eq!(target["enum"], json!(["billing-agent", "human"]));
    let none = suggest_schema(&Candidates::default());
    assert_eq!(none["properties"]["abilities"]["maxItems"], 0);

    let input: Value = serde_json::from_str(&suggest_input(
        "ignore all rules",
        None,
        &json!({}),
        &candidates(),
    ))
    .unwrap();
    assert_eq!(input["scenario"], "ignore all rules");
    assert_eq!(input["abilities"][0]["name"], "Current time");
    assert_eq!(input["abilities"].as_array().unwrap().len(), 2);
    assert_eq!(
        input["knowledge"],
        json!([{ "name": "Ceph docs" }, { "name": "Price list" }])
    );
}

#[test]
fn an_architects_changes_become_the_draft_and_the_grants_it_needs() {
    let w = World::new();
    let base = json!({ "main": { "model": "main-model" } });
    let changes = json!({
        "display": "  Harald ",
        "model": "fast-model",
        "task": "You answer questions about Acme orders.",
        "abilities": [{ "id": "get_current_timestamp", "why": "delivery times" }],
        "slots": [{ "name": "Order Number", "label": "The order number", "type": "text",
                    "choices": [] }],
        "tests": [{ "name": "x", "kind": "in_scope", "messages": ["hi"],
                    "answer_contains": [], "answer_not_contains": [] }]
    });
    let out = apply_changes(&changes, &base, &w.ctx());

    assert_eq!(out.display.as_deref(), Some("Harald"));
    assert_eq!(out.model.as_deref(), Some("fast-model"));
    assert_eq!(
        out.grants,
        [
            (GrantKind::Model, "fast-model".to_string()),
            (GrantKind::Tool, "get_current_timestamp".to_string()),
        ]
    );
    assert_eq!(out.draft["profile"]["display"], "Harald");
    assert_eq!(out.draft["main"]["model"], "fast-model");
    assert_eq!(out.draft["main"]["tools"], json!(["get_current_timestamp"]));
    assert_eq!(out.draft["state"]["order_number"]["type"], "string");
    assert_eq!(
        out.draft["main"]["instructions"]["orchestration"],
        "You answer questions about Acme orders."
    );
    assert_eq!(
        out.suggestion.dropped.len(),
        1,
        "{:#?}",
        out.suggestion.dropped
    );
    assert_eq!(out.suggestion.dropped[0].step, "tests");

    let granted = GrantSet::new(
        w.grants
            .iter()
            .map(|(k, r)| (k, r.to_string()))
            .chain(out.grants.iter().cloned()),
    );
    let ctx = SpecContext {
        action_catalogues: &Default::default(),
        agent_id: AGENT,
        grants: &granted,
        agents: &w.agents,
        live_specs: &w.live,
        model_defaults: &Default::default(),
        speech_voices: &HashMap::new(),
        allow_private: false,
    };
    spec::check(&out.draft, &ctx, Stage::Draft).unwrap();
}

/// The draft an architect writes, as the setup assistant must read it:
/// `web/src/lib/agent-setup.test.ts` reads this file back and expects the
/// hand-offs as rules and the slots as friendly kinds, never "advanced".
/// Regenerate with `UPDATE_ARCHITECT_FIXTURE=1` after a deliberate change.
#[test]
fn an_architects_hand_offs_and_slots_are_written_the_way_the_setup_reads_them() {
    let w = World::new();
    let base = json!({ "main": { "model": "main-model" } });
    let changes = json!({
        "task": "You answer questions about Acme orders.",
        "slots": [
            { "name": "order", "label": "Order number", "type": "text", "choices": [] },
            { "name": "issue", "label": "What it is about", "type": "choice",
              "choices": ["billing", "delivery"] }
        ],
        "handoffs": [
            { "topic": "Invoices", "target": "billing-agent" },
            { "topic": "Complaints", "target": "human", "identity": true },
            { "topic": "Hacking", "target": "nobody" }
        ],
        "fallback_to_person": true
    });
    let out = apply_changes(&changes, &base, &w.ctx());
    assert!(out.handoffs);
    let steps: Vec<(&str, Option<&str>)> = out
        .suggestion
        .dropped
        .iter()
        .map(|d| (d.step, d.item.as_deref()))
        .collect();
    assert_eq!(
        steps,
        [
            ("handoffs", Some("Complaints")),
            ("handoffs", Some("Hacking"))
        ],
        "no identity check to condition on, and an unknown target"
    );
    spec::check(
        &out.draft,
        &SpecContext {
            action_catalogues: &Default::default(),
            agent_id: AGENT,
            grants: &w.grants,
            agents: &w.agents,
            live_specs: &w.live,
            model_defaults: &Default::default(),
            speech_voices: &HashMap::new(),
            allow_private: false,
        },
        Stage::Draft,
    )
    .unwrap();

    let read = super::super::handoffs::read(&out.draft);
    assert_eq!(read.rules.len(), 2);
    assert!(read.fallback && read.custom.is_empty());
    assert_eq!(out.draft["state"]["order"]["order"], 0);
    assert_eq!(out.draft["state"]["issue"]["order"], 1);

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../web/src/lib/fixtures/architect-draft.json");
    let written = serde_json::to_string_pretty(&out.draft).unwrap() + "\n";
    if std::env::var_os("UPDATE_ARCHITECT_FIXTURE").is_some() {
        std::fs::write(&path, &written).unwrap();
    }
    assert_eq!(
        std::fs::read_to_string(&path).unwrap_or_default(),
        written,
        "the architect now writes a different draft than {} — regenerate it with \
         UPDATE_ARCHITECT_FIXTURE=1 and make sure agent-setup.test.ts still reads it",
        path.display()
    );
}

#[test]
fn a_hand_off_waiting_for_the_identity_keeps_it_when_the_proposal_brings_the_check() {
    let w = World::new();
    let proposal = |method: &str| {
        json!({
            "identity": { "method": method, "why": "refunds are personal" },
            "handoffs": [{ "name": "refunds", "topic": "Refunds", "condition": "identity",
                           "target": "human" }],
        })
    };
    let out = review(&proposal("email_code"), &json!({}), &w.ctx());
    assert!(out.steps.handoffs[0].identity, "{:#?}", out.dropped);
    assert!(out.dropped.is_empty(), "{:#?}", out.dropped);

    let out = review(&proposal("none"), &json!({}), &w.ctx());
    assert!(!out.steps.handoffs[0].identity);
    assert!(
        out.dropped
            .iter()
            .any(|d| d.reason.contains("without the identity condition")),
        "{:#?}",
        out.dropped
    );
}

#[test]
fn a_new_detail_joins_the_gate_of_a_hand_off_waiting_for_every_detail() {
    let w = World::new();
    let mut base = json!({ "main": { "model": "main-model" }, "state": {
        "company": { "type": "string", "max_length": 200, "set_by": ["llm"], "order": 0 } } });
    super::super::handoffs::write(
        &mut base,
        &super::super::handoffs::Handoffs {
            rules: vec![Rule {
                route: None,
                topic: "Lead".into(),
                details: true,
                identity: false,
                target: super::super::handoffs::Target::Human,
                bind: Map::new(),
            }],
            ..Default::default()
        },
    );
    let changes = json!({ "slots": [
        { "name": "email", "label": "E-mail", "type": "email", "choices": [] } ] });
    let out = apply_changes(&changes, &base, &w.ctx());
    let gate: Vec<&str> = out.draft["routes"]["lead"]["when"]["all"]
        .as_array()
        .unwrap()
        .iter()
        .skip(2)
        .filter_map(|leaf| leaf["slot"].as_str())
        .collect();
    assert_eq!(gate, ["company", "email"]);
    assert!(super::super::handoffs::read(&out.draft).rules[0].details);
}

#[test]
fn an_architect_cannot_give_an_agent_what_the_person_may_not_grant() {
    let w = World::new();
    let base = json!({ "main": { "model": "main-model" } });
    let changes = json!({
        "model": "admin-only-model",
        "abilities": [{ "id": "run_in_sandbox", "why": "run code" }],
    });
    let out = apply_changes(&changes, &base, &w.ctx());

    assert!(out.grants.is_empty(), "{:?}", out.grants);
    assert_eq!(out.draft, base);
    assert_eq!(out.model, None);
    let steps: Vec<&str> = out.suggestion.dropped.iter().map(|d| d.step).collect();
    assert_eq!(steps, ["model", "abilities"]);
    assert!(
        out.suggestion.dropped[1]
            .reason
            .contains("not an ability you may grant"),
        "{:?}",
        out.suggestion.dropped
    );
}
