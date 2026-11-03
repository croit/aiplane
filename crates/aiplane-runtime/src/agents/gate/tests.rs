use super::*;
use crate::agents::slot_tools::SlotTools;
use crate::agents::state::tests::{at, pool_with_session, schema, support_spec};
use crate::agents::state::{TrustedWriter, write_trusted};
use crate::server::tools::{ToolContext, ToolSource};
use aiplane_agents::db::agent_state::StoredSlot;
use serde_json::json;
use std::sync::Arc;

const NOW: &str = "2026-10-02T12:00:00Z";

fn row(slot: &str, value: Value, provenance: &str, set_at: &str) -> StoredSlot {
    StoredSlot {
        slot: slot.into(),
        value,
        provenance: provenance.into(),
        set_at: at(set_at),
    }
}

/// issue (llm, 2 min old), verified (verifier:otp, 10 min), score (host,
/// exactly 1 h), name (llm, invalid: too short), email missing.
fn state() -> AgentState {
    AgentState::from_rows(
        &schema(),
        vec![
            row("issue", json!("billing"), "llm", "2026-10-02T11:58:00Z"),
            row(
                "verified",
                json!({"customer_id": "K-12345"}),
                "verifier:otp",
                "2026-10-02T11:50:00Z",
            ),
            row("score", json!(0.5), "host", "2026-10-02T11:00:00Z"),
            row("name", json!("A"), "llm", "2026-10-02T11:59:00Z"),
        ],
    )
}

fn kinds(unmet: &[Unmet]) -> Vec<String> {
    unmet
        .iter()
        .map(|u| {
            serde_json::to_value(&u.problem).unwrap()["kind"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect()
}

fn eval_at(cond: &Value, state: &AgentState, now: &str) -> Vec<Unmet> {
    let s = schema();
    let cond = Cond::parse(cond).unwrap_or_else(|e| panic!("{cond}: {e}"));
    evaluate(
        &cond,
        GateInput {
            schema: &s,
            state,
            now: at(now),
        },
    )
}

/// The table: a gate, and the kinds of its unmet leaves (empty = open).
#[test]
fn gate_expressions_evaluate_as_documented() {
    let billing = json!({ "slot": "issue", "eq": "billing" });
    let fresh_otp = json!({ "slot": "verified", "provenance": "verifier:otp", "max_age": "15m" });
    let technical = json!({ "slot": "issue", "eq": "technical" });
    let email_set = json!({ "slot": "email", "set": true });
    let cases: Vec<(&str, Value, Vec<&str>)> = vec![
        ("eq holds", billing.clone(), vec![]),
        ("eq fails", technical.clone(), vec!["not_equal"]),
        (
            "in holds",
            json!({ "slot": "issue", "in": ["technical", "billing"] }),
            vec![],
        ),
        (
            "in fails",
            json!({ "slot": "issue", "in": ["technical"] }),
            vec!["not_in"],
        ),
        ("set on a missing slot", email_set.clone(), vec!["missing"]),
        (
            "unset on a missing slot",
            json!({ "slot": "email", "set": false }),
            vec![],
        ),
        (
            "unset on a set slot",
            json!({ "slot": "issue", "set": false }),
            vec!["must_be_unset"],
        ),
        (
            "set on an invalid slot",
            json!({ "slot": "name", "set": true }),
            vec!["invalid"],
        ),
        (
            "unset on an invalid slot",
            json!({ "slot": "name", "set": false }),
            vec![],
        ),
        (
            "eq on an invalid slot",
            json!({ "slot": "name", "eq": "A" }),
            vec!["invalid"],
        ),
        ("provenance and age hold", fresh_otp.clone(), vec![]),
        (
            "too old",
            json!({ "slot": "verified", "provenance": "verifier:otp", "max_age": "5m" }),
            vec!["too_old"],
        ),
        (
            "wrong provenance",
            json!({ "slot": "verified", "provenance": "host" }),
            vec!["wrong_provenance"],
        ),
        (
            "llm provenance does not satisfy a trusted one",
            json!({ "slot": "issue", "provenance": "host" }),
            vec!["wrong_provenance"],
        ),
        (
            "an age exactly at the limit holds",
            json!({ "slot": "score", "provenance": "host", "max_age": "1h" }),
            vec![],
        ),
        (
            "a second over the limit",
            json!({ "slot": "score", "max_age": "59m" }),
            vec!["too_old"],
        ),
        (
            "every failing check of one leaf is reported",
            json!({ "slot": "verified", "provenance": "host", "max_age": "1m",
                        "eq": { "customer_id": "K-1" } }),
            vec!["not_equal", "wrong_provenance", "too_old"],
        ),
        (
            "eq compares a whole subject",
            json!({ "slot": "verified", "eq": { "customer_id": "K-12345" } }),
            vec![],
        ),
        (
            "provenance on a missing slot",
            json!({ "slot": "email", "provenance": "llm" }),
            vec!["missing"],
        ),
        (
            "an undeclared slot is missing",
            json!({ "slot": "ghost", "set": true }),
            vec!["missing"],
        ),
        ("all holds", json!({ "all": [billing, fresh_otp] }), vec![]),
        (
            "all reports every failing child",
            json!({ "all": [technical, email_set] }),
            vec!["not_equal", "missing"],
        ),
        (
            "any holds if one child does",
            json!({ "any": [technical, billing] }),
            vec![],
        ),
        (
            "any reports every child when none holds",
            json!({ "any": [technical, email_set] }),
            vec!["not_equal", "missing"],
        ),
        (
            "not of a failing leaf holds",
            json!({ "not": email_set }),
            vec![],
        ),
        (
            "not of a holding leaf",
            json!({ "not": billing }),
            vec!["excluded"],
        ),
        (
            "not of an undeclared slot holds",
            json!({ "not": { "slot": "ghost", "eq": 1 } }),
            vec![],
        ),
        (
            "the epic's billing gate",
            json!({ "all": [
                    { "slot": "issue", "eq": "billing" },
                    { "slot": "verified", "provenance": "verifier:otp", "max_age": "15m" }
                ] }),
            vec![],
        ),
        (
            "nesting",
            json!({ "all": [
                    { "any": [ { "slot": "email", "set": true }, { "slot": "score", "provenance": "host" } ] },
                    { "not": { "slot": "issue", "in": ["sales"] } }
                ] }),
            vec![],
        ),
    ];
    let s = state();
    for (name, cond, expected) in cases {
        let unmet = eval_at(&cond, &s, NOW);
        assert_eq!(kinds(&unmet), expected, "{name}: {cond}\n{unmet:#?}");
    }
}

#[test]
fn every_unmet_leaf_says_where_it_is_and_which_slot() {
    let unmet = eval_at(
        &json!({ "all": [
                { "slot": "issue", "eq": "billing" },
                { "any": [ { "slot": "email", "set": true }, { "not": { "slot": "issue", "set": true } } ] }
            ] }),
        &state(),
        NOW,
    );
    let at: Vec<(&str, Option<&str>)> = unmet
        .iter()
        .map(|u| (u.path.as_str(), u.slot.as_deref()))
        .collect();
    assert_eq!(
        at,
        [
            ("all[1].any[0]", Some("email")),
            ("all[1].any[1].not", None)
        ]
    );
}

#[test]
fn the_same_state_and_clock_always_give_the_same_answer_and_age_follows_the_clock() {
    let gate = json!({ "slot": "verified", "provenance": "verifier:otp", "max_age": "15m" });
    let s = state();
    assert_eq!(eval_at(&gate, &s, NOW), eval_at(&gate, &s, NOW));
    assert!(eval_at(&gate, &s, "2026-10-02T12:05:00Z").is_empty());
    assert_eq!(
        kinds(&eval_at(&gate, &s, "2026-10-02T12:05:01Z")),
        ["too_old"]
    );
    // A value stamped after `now` (clock skew) is not older than anything.
    assert!(eval_at(&gate, &s, "2026-10-02T11:00:00Z").is_empty());
}

#[test]
fn messages_say_what_to_do_and_never_leak_a_trusted_value() {
    let unmet = eval_at(
        &json!({ "all": [
                { "slot": "email", "set": true },
                { "slot": "plan", "set": true },
                { "slot": "verified", "provenance": "verifier:otp", "max_age": "1m" },
                { "slot": "verified", "eq": { "customer_id": "K-1" } },
                { "slot": "issue", "eq": "technical" }
            ] }),
        &state(),
        NOW,
    );
    let messages: Vec<&str> = unmet.iter().map(|u| u.message.as_str()).collect();
    assert!(messages[0].contains("call set_email"), "{}", messages[0]);
    assert!(
        messages[1].contains("set by host, not by you"),
        "{}",
        messages[1]
    );
    assert!(messages[2].contains("more than 1m ago"), "{}", messages[2]);
    assert!(messages[4].contains("\"technical\""), "{}", messages[4]);
    for m in &messages {
        assert!(!m.contains("K-12345"), "{m}");
    }
}

#[test]
fn a_malformed_tree_is_refused_when_read() {
    for (bad, needle) in [
        (json!([]), "an object"),
        (json!({ "all": [] }), "non-empty"),
        (json!({ "slot": "x" }), "checks nothing"),
        (json!({ "slot": "x", "max_age": "soon" }), "duration"),
        (json!({ "slot": "x", "provenance": "model" }), "provenance"),
        (json!({ "slot": "x", "set": "yes" }), "`set`"),
        (json!({ "slot": "x", "in": "a" }), "`in`"),
        (json!({ "slot": 3, "set": true }), "`slot`"),
        (json!({ "slot": "x", "set": true, "equals": 1 }), "`equals`"),
        (
            json!({ "not": { "slot": "x", "set": true }, "slot": "x" }),
            "only key",
        ),
    ] {
        let err = Cond::parse(&bad).unwrap_err();
        assert!(err.contains(needle), "{bad}: {err}");
    }
}

#[test]
fn a_gate_requires_trusted_provenance_only_if_every_way_to_open_it_does() {
    let trusted = json!({ "slot": "verified", "provenance": "verifier:otp" });
    let llm = json!({ "slot": "issue", "eq": "billing" });
    for (cond, expected) in [
        (trusted.clone(), true),
        (json!({ "slot": "issue", "provenance": "llm" }), false),
        (llm.clone(), false),
        (json!({ "all": [llm.clone(), trusted.clone()] }), true),
        (json!({ "any": [llm.clone(), trusted.clone()] }), false),
        (
            json!({ "any": [trusted.clone(), { "slot": "score", "provenance": "host" }] }),
            true,
        ),
        (json!({ "not": { "not": trusted.clone() } }), false),
    ] {
        assert_eq!(
            Cond::parse(&cond).unwrap().requires_trusted_provenance(),
            expected,
            "{cond}"
        );
    }
}

#[test]
fn a_gate_requires_one_slot_trusted_only_if_every_way_to_open_it_checks_that_slot() {
    let verified = json!({ "slot": "verified", "provenance": "verifier:otp" });
    let score = json!({ "slot": "score", "provenance": "host" });
    for (cond, expected) in [
        (verified.clone(), true),
        (score.clone(), false),
        (json!({ "all": [score.clone(), verified.clone()] }), true),
        (json!({ "any": [score.clone(), verified.clone()] }), false),
        (json!({ "slot": "verified", "provenance": "llm" }), false),
        (json!({ "not": verified.clone() }), false),
    ] {
        assert_eq!(
            Cond::parse(&cond)
                .unwrap()
                .requires_trusted_provenance_of("verified"),
            expected,
            "{cond}"
        );
    }
}

#[test]
fn type_check_finds_leaves_that_can_never_hold() {
    let cond = Cond::parse(&json!({ "all": [
            { "slot": "issue", "eq": "sales" },
            { "slot": "issue", "in": ["billing", 7] },
            { "slot": "issue", "provenance": "host" },
            { "slot": "verified", "provenance": "verifier:otp", "eq": { "customer_id": 1 } },
            { "not": { "slot": "seats", "eq": "three" } },
            { "slot": "score", "provenance": "host", "eq": 0.5 }
        ] }))
    .unwrap();
    let problems = cond.type_check(&schema());
    let paths: Vec<&str> = problems.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(
        paths,
        [
            "all[0].eq",
            "all[1].in[1]",
            "all[2].provenance",
            "all[3].eq",
            "all[4].not.eq"
        ]
    );
    assert!(problems[0].1.contains("one of"), "{}", problems[0].1);
    assert!(
        problems[2].1.contains("set only by llm"),
        "{}",
        problems[2].1
    );
}

fn gates() -> RouteGates {
    let mut spec = support_spec();
    spec["routes"] = json!({
        "billing": {
            "when": { "all": [
                { "slot": "issue", "eq": "billing" },
                { "slot": "verified", "provenance": "verifier:otp", "max_age": "15m" }
            ] },
            "agent": "billing-id", "task": "t"
        },
        "technical": { "when": { "slot": "issue", "eq": "technical" }, "agent": "t-id", "task": "t" }
    });
    RouteGates::from_spec(&AgentSpec::from_value(&spec).unwrap()).unwrap()
}

#[test]
fn gate_status_is_open_or_closed_with_the_structured_list() {
    let s = schema();
    let st = state();
    let input = GateInput {
        schema: &s,
        state: &st,
        now: at(NOW),
    };
    assert_eq!(gates().gate_status("billing", input), GateStatus::Open);
    let closed = gates().gate_status("technical", input);
    let wire = serde_json::to_value(&closed).unwrap();
    assert_eq!(wire["status"], "closed");
    assert_eq!(wire["missing"][0]["kind"], "not_equal");
    assert_eq!(wire["missing"][0]["slot"], "issue");
    assert_eq!(wire["missing"][0]["expected"], "technical");
    assert!(wire["missing"][0]["message"].is_string());

    let unknown = gates().gate_status("refunds", input);
    let GateStatus::Closed { missing } = unknown else {
        panic!("{unknown:?}");
    };
    assert_eq!(missing[0].problem, Problem::UnknownRoute);
    assert!(missing[0].message.contains("billing, technical"));

    let names: Vec<String> = gates()
        .statuses(input)
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    assert_eq!(names, ["billing", "technical"]);
}

#[test]
fn a_spec_without_routes_has_no_gates_and_a_malformed_one_says_where() {
    assert!(
        RouteGates::from_spec(AgentSpec::empty())
            .unwrap()
            .routes
            .is_empty()
    );
    let malformed = json!({ "routes": { "r": {
            "when": { "all": 1 }, "agent": "a", "task": "t"
        } } });
    let err = RouteGates::from_spec(&AgentSpec::from_value(&malformed).unwrap()).unwrap_err();
    assert_eq!(err.path, "routes.r.when");
}

/// The gate's acceptance test: a route cannot be opened while its gate is
/// closed, whatever the model sends through the tools it has.
#[tokio::test]
async fn a_route_cannot_be_opened_while_closed_whatever_the_model_sends() {
    let pool = pool_with_session("s1").await;
    let s = Arc::new(schema());
    let tools = SlotTools::with_clock(Arc::clone(&s), Arc::new(|| at(NOW)));
    let ctx = ToolContext {
        session_id: Some("s1".into()),
        ..ToolContext::for_test(pool.clone())
    };
    let forged = [
        ("set_issue", json!({ "value": "billing" })),
        ("set_verified", json!({ "value": { "customer_id": "K-1" } })),
        (
            "set_issue",
            json!({ "value": "billing", "provenance": "verifier:otp" }),
        ),
        ("set_name", json!({ "value": "Ada", "slot": "verified" })),
    ];
    for (tool, args) in forged {
        if let Some(t) = tools.get(tool) {
            let _ = t.run(ctx.clone(), args).await;
        }
    }
    let open = |now: &str| {
        let s = Arc::clone(&s);
        let pool = pool.clone();
        let now = at(now);
        async move {
            let st = AgentState::load(&pool, &s, "s1").await.unwrap();
            gates()
                .open(
                    "billing",
                    GateInput {
                        schema: &s,
                        state: &st,
                        now,
                    },
                )
                .map(|r| r.name().to_string())
        }
    };
    let refused = open(NOW).await.unwrap_err();
    assert_eq!(kinds(&refused), ["missing"]);
    assert_eq!(refused[0].slot.as_deref(), Some("verified"));

    write_trusted(
        &pool,
        &s,
        "s1",
        "verified",
        json!({ "customer_id": "K-1" }),
        TrustedWriter::Verifier("otp".into()),
        at(NOW),
    )
    .await
    .unwrap();
    assert_eq!(open(NOW).await.unwrap(), "billing");
    assert_eq!(
        kinds(&open("2026-10-02T12:15:01Z").await.unwrap_err()),
        ["too_old"]
    );
}
