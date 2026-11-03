// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Route gates: the JSON condition tree and its evaluator (`docs/agent-spec.md` → "Gates").
//!
//! ```text
//! Cond := { all: [Cond] } | { any: [Cond] } | { not: Cond }
//!       | { slot, set?: bool, eq?: Value, in?: [Value],
//!           provenance?: "llm" | "verifier:<id>" | "host", max_age?: Duration }
//! ```
//!
//! A leaf's checks are ANDed. Evaluation is:
//! - **total** — it never fails or panics. A slot that is missing, invalid or
//!   not declared at all makes every check on it false, except `set: false`.
//! - **deterministic** — the only inputs are the state and the `now` the
//!   caller passes in.
//! - **explainable** — a closed gate comes with every unmet leaf, its path in
//!   the tree and a message the model can act on. A message never contains a
//!   value the model did not write.
//!
//! A route is invoked only through an [`OpenRoute`], which nothing but this
//! module constructs, and only for a gate that holds.

use std::collections::BTreeMap;

use jiff::{SignedDuration, Timestamp};
use serde::Serialize;
use serde_json::Value;

use super::slot_tools::set_tool_name;
use super::spec::{AgentSpec, LEAF_KEYS, SpecIssue, format_duration, index, join, parse_duration};
use super::state::{self, AgentState, Provenance, SlotState, StateSchema, writers};

#[derive(Debug, Clone, PartialEq)]
pub enum Cond {
    All(Vec<Cond>),
    Any(Vec<Cond>),
    Not(Box<Cond>),
    Leaf(Leaf),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Leaf {
    pub slot: String,
    pub set: Option<bool>,
    pub eq: Option<Value>,
    pub any_of: Option<Vec<Value>>,
    pub provenance: Option<Provenance>,
    pub max_age: Option<SignedDuration>,
}

impl Cond {
    /// Read a condition the spec validator accepted. The error names the
    /// first problem, for a tree that skipped validation.
    pub fn parse(v: &Value) -> Result<Self, String> {
        let Value::Object(map) = v else {
            return Err(format!("a condition must be an object, not {v}"));
        };
        for combinator in ["all", "any"] {
            let Some(children) = map.get(combinator) else {
                continue;
            };
            if map.len() != 1 {
                return Err(format!(
                    "`{combinator}` must be the only key of its condition"
                ));
            }
            let Some(items) = children.as_array().filter(|a| !a.is_empty()) else {
                return Err(format!(
                    "`{combinator}` must be a non-empty array of conditions"
                ));
            };
            let parsed = items.iter().map(Cond::parse).collect::<Result<_, _>>()?;
            return Ok(if combinator == "all" {
                Cond::All(parsed)
            } else {
                Cond::Any(parsed)
            });
        }
        if let Some(child) = map.get("not") {
            if map.len() != 1 {
                return Err("`not` must be the only key of its condition".into());
            }
            return Ok(Cond::Not(Box::new(Cond::parse(child)?)));
        }
        if let Some(key) = map.keys().find(|k| !LEAF_KEYS.contains(&k.as_str())) {
            return Err(format!(
                "unknown key `{key}` — a condition is `all`, `any`, `not`, or a leaf with: {}",
                LEAF_KEYS.join(", ")
            ));
        }
        let slot = map
            .get("slot")
            .and_then(Value::as_str)
            .ok_or("a leaf condition needs a `slot` naming a state slot")?;
        let set = map
            .get("set")
            .map(|v| v.as_bool().ok_or("`set` must be true or false"))
            .transpose()?;
        let any_of = map
            .get("in")
            .map(|v| {
                v.as_array()
                    .cloned()
                    .ok_or("`in` must be an array of values")
            })
            .transpose()?;
        let provenance = map
            .get("provenance")
            .map(|v| {
                v.as_str().and_then(Provenance::parse).ok_or_else(|| {
                    format!("`provenance` must be `llm`, `host` or `verifier:<id>`, not {v}")
                })
            })
            .transpose()?;
        let max_age = map
            .get("max_age")
            .map(|v| {
                v.as_str()
                    .and_then(parse_duration)
                    .ok_or_else(|| format!("`max_age` must be a duration such as `15m`, not {v}"))
            })
            .transpose()?;
        let leaf = Leaf {
            slot: slot.to_string(),
            set,
            eq: map.get("eq").cloned(),
            any_of,
            provenance,
            max_age,
        };
        if leaf.set.is_some() || leaf.has_value_checks() {
            Ok(Cond::Leaf(leaf))
        } else {
            Err(
                "a leaf condition checks nothing — add `set`, `eq`, `in`, `provenance` or \
                 `max_age`"
                    .into(),
            )
        }
    }

    /// Whether the condition can only hold when some slot was written by a
    /// verifier or the host — what a route that binds a subject must require.
    pub fn requires_trusted_provenance(&self) -> bool {
        self.requires_trusted(&|_| true)
    }

    /// [`Self::requires_trusted_provenance`] for one slot: the condition can
    /// only hold when `slot` was written by a verifier or the host — what a
    /// route that binds an argument from `slot` must require.
    pub fn requires_trusted_provenance_of(&self, slot: &str) -> bool {
        self.requires_trusted(&|leaf| leaf.slot == slot)
    }

    fn requires_trusted(&self, counts: &dyn Fn(&Leaf) -> bool) -> bool {
        match self {
            Cond::Leaf(leaf) => {
                counts(leaf)
                    && leaf
                        .provenance
                        .as_ref()
                        .is_some_and(|p| *p != Provenance::Llm)
            }
            Cond::All(children) => children.iter().any(|c| c.requires_trusted(counts)),
            Cond::Any(children) => children.iter().all(|c| c.requires_trusted(counts)),
            // `not` can hold on a missing slot, so it proves nothing.
            Cond::Not(_) => false,
        }
    }

    /// Every leaf check that can never hold for the declared slots, as
    /// `(path below the gate, message)`. Undeclared slots are left to the
    /// spec's shape check, which already reports them.
    pub fn type_check(&self, schema: &StateSchema) -> Vec<(String, String)> {
        let mut out = Vec::new();
        self.type_check_at(schema, "", &mut out);
        out
    }

    fn type_check_at(&self, schema: &StateSchema, path: &str, out: &mut Vec<(String, String)>) {
        let leaf = match self {
            Cond::All(children) | Cond::Any(children) => {
                let key = if matches!(self, Cond::All(_)) {
                    "all"
                } else {
                    "any"
                };
                for (i, child) in children.iter().enumerate() {
                    child.type_check_at(schema, &index(&join(path, key), i), out);
                }
                return;
            }
            Cond::Not(child) => return child.type_check_at(schema, &join(path, "not"), out),
            Cond::Leaf(leaf) => leaf,
        };
        let Some(def) = schema.slot(&leaf.slot) else {
            return;
        };
        let slot = &leaf.slot;
        if let Some(eq) = &leaf.eq
            && let Err(why) = def.check(eq)
        {
            out.push((
                join(path, "eq"),
                format!("can never hold: a value of `{slot}` {why}"),
            ));
        }
        for (i, v) in leaf.any_of.iter().flatten().enumerate() {
            if let Err(why) = def.check(v) {
                out.push((
                    index(&join(path, "in"), i),
                    format!("can never match: a value of `{slot}` {why}"),
                ));
            }
        }
        if let Some(p) = &leaf.provenance
            && !def.writable_by(p)
        {
            out.push((
                join(path, "provenance"),
                format!(
                    "can never hold: `{slot}` is set only by {}, so it never has provenance `{p}`",
                    writers(&def.set_by)
                ),
            ));
        }
    }
}

impl Leaf {
    /// `{slot, set: false}` and nothing else: the one leaf a missing slot
    /// satisfies.
    fn only_unset(&self) -> bool {
        self.set == Some(false) && !self.has_value_checks()
    }

    fn has_value_checks(&self) -> bool {
        self.eq.is_some()
            || self.any_of.is_some()
            || self.provenance.is_some()
            || self.max_age.is_some()
    }
}

/// Why a gate is closed, for one leaf.
#[derive(Debug, Clone, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Problem {
    Missing,
    Invalid,
    MustBeUnset,
    NotEqual {
        expected: Value,
    },
    NotIn {
        expected: Vec<Value>,
    },
    WrongProvenance {
        required: Provenance,
        actual: Provenance,
    },
    TooOld {
        max_age: String,
    },
    /// The condition under a `not` holds.
    Excluded,
    UnknownRoute,
}

/// One unmet condition.
#[derive(Debug, Clone, PartialEq, Serialize, schemars::JsonSchema)]
pub struct Unmet {
    /// Where in the tree, e.g. `all[1]`; empty for the root.
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot: Option<String>,
    #[serde(flatten)]
    pub problem: Problem,
    /// What the model can do about it.
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum GateStatus {
    Open,
    Closed { missing: Vec<Unmet> },
}

/// What a gate is judged on.
#[derive(Clone, Copy)]
pub struct GateInput<'a> {
    pub schema: &'a StateSchema,
    pub state: &'a AgentState,
    /// Injected rather than read here so a `max_age` check is reproducible.
    pub now: Timestamp,
}

/// Every unmet leaf of `cond`; empty when it holds.
pub fn evaluate(cond: &Cond, input: GateInput<'_>) -> Vec<Unmet> {
    let mut out = Vec::new();
    eval_at(cond, input, "", &mut out);
    out
}

fn eval_at(cond: &Cond, input: GateInput<'_>, path: &str, out: &mut Vec<Unmet>) {
    match cond {
        Cond::All(children) => {
            for (i, child) in children.iter().enumerate() {
                eval_at(child, input, &index(&join(path, "all"), i), out);
            }
        }
        Cond::Any(children) => {
            let mut unmet = Vec::new();
            for (i, child) in children.iter().enumerate() {
                let before = unmet.len();
                eval_at(child, input, &index(&join(path, "any"), i), &mut unmet);
                if unmet.len() == before {
                    return;
                }
            }
            out.extend(unmet);
        }
        Cond::Not(child) => {
            if evaluate(child, input).is_empty() {
                let mut slots = Vec::new();
                child.slots(&mut slots);
                out.push(Unmet {
                    path: join(path, "not"),
                    slot: None,
                    problem: Problem::Excluded,
                    message: format!(
                        "the gate excludes the current value of {}, so this route is not for this \
                         conversation",
                        slots.join(", ")
                    ),
                });
            }
        }
        Cond::Leaf(leaf) => eval_leaf(leaf, input, path, out),
    }
}

impl Cond {
    fn slots(&self, out: &mut Vec<String>) {
        match self {
            Cond::All(children) | Cond::Any(children) => {
                children.iter().for_each(|c| c.slots(out));
            }
            Cond::Not(child) => child.slots(out),
            Cond::Leaf(leaf) => {
                let named = format!("`{}`", leaf.slot);
                if !out.contains(&named) {
                    out.push(named);
                }
            }
        }
    }
}

fn eval_leaf(leaf: &Leaf, input: GateInput<'_>, path: &str, out: &mut Vec<Unmet>) {
    let slot = &leaf.slot;
    let mut push = |problem: Problem, message: String| {
        out.push(Unmet {
            path: path.to_string(),
            slot: Some(slot.clone()),
            problem,
            message,
        });
    };
    let how = how_to_set(input.schema, slot);
    let entry = match input.state.get(slot) {
        SlotState::Set(entry) => entry,
        _ if leaf.only_unset() => return,
        SlotState::Invalid { reason, .. } => {
            return push(
                Problem::Invalid,
                format!("`{slot}` is invalid: {reason}{how}"),
            );
        }
        SlotState::Missing => return push(Problem::Missing, format!("`{slot}` is missing{how}")),
    };
    if leaf.set == Some(false) {
        push(
            Problem::MustBeUnset,
            format!("this route is only for a conversation where `{slot}` is not set"),
        );
    }
    if let Some(expected) = &leaf.eq
        && entry.value != *expected
    {
        push(
            Problem::NotEqual {
                expected: expected.clone(),
            },
            format!("`{slot}` must be {expected}{how}"),
        );
    }
    if let Some(expected) = &leaf.any_of
        && !expected.contains(&entry.value)
    {
        let listed: Vec<String> = expected.iter().map(Value::to_string).collect();
        push(
            Problem::NotIn {
                expected: expected.clone(),
            },
            format!("`{slot}` must be one of {}{how}", listed.join(", ")),
        );
    }
    if let Some(required) = &leaf.provenance
        && entry.provenance != *required
    {
        push(
            Problem::WrongProvenance {
                required: required.clone(),
                actual: entry.provenance.clone(),
            },
            format!(
                "`{slot}` must be set by {required}, and it was set by {}",
                entry.provenance
            ),
        );
    }
    if let Some(max_age) = leaf.max_age
        && input.now.duration_since(entry.set_at) > max_age
    {
        let limit = format_duration(max_age);
        push(
            Problem::TooOld {
                max_age: limit.clone(),
            },
            format!("`{slot}` was set more than {limit} ago and must be set again{how}"),
        );
    }
}

/// The tail of a message about `slot`: who can fix it.
fn how_to_set(schema: &StateSchema, slot: &str) -> String {
    match schema.slot(slot) {
        Some(def) => state::how_to_set(
            def.model_writable().then(|| set_tool_name(slot)).as_deref(),
            &def.set_by,
        ),
        None => " — it is not declared in this agent's state".into(),
    }
}

/// Proof that a route's gate was open when it was checked. Only
/// [`RouteGates::open`] makes one, so code that dispatches a route cannot be
/// reached without passing its gate.
#[derive(Debug)]
pub struct OpenRoute {
    name: String,
}

impl OpenRoute {
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Every route's gate of one spec.
#[derive(Debug, Clone, Default)]
pub struct RouteGates {
    routes: BTreeMap<String, Cond>,
}

impl RouteGates {
    pub fn from_spec(spec: &AgentSpec) -> Result<Self, SpecIssue> {
        let mut gates = BTreeMap::new();
        for (name, route) in &spec.routes {
            let cond = Cond::parse(&route.when).map_err(|message| SpecIssue {
                path: format!("routes.{name}.when"),
                message,
            })?;
            gates.insert(name.clone(), cond);
        }
        Ok(Self { routes: gates })
    }

    pub fn gate_status(&self, route: &str, input: GateInput<'_>) -> GateStatus {
        let Some(cond) = self.routes.get(route) else {
            let names: Vec<&str> = self.routes.keys().map(String::as_str).collect();
            return GateStatus::Closed {
                missing: vec![Unmet {
                    path: String::new(),
                    slot: None,
                    problem: Problem::UnknownRoute,
                    message: format!(
                        "there is no route `{route}`; the routes are: {}",
                        names.join(", ")
                    ),
                }],
            };
        };
        let missing = evaluate(cond, input);
        if missing.is_empty() {
            GateStatus::Open
        } else {
            GateStatus::Closed { missing }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.routes.is_empty()
    }

    /// Every route in name order with its status.
    pub fn statuses(&self, input: GateInput<'_>) -> Vec<(String, GateStatus)> {
        self.routes
            .keys()
            .map(|name| (name.clone(), self.gate_status(name, input)))
            .collect()
    }

    /// The route, if its gate holds; otherwise what is unmet.
    pub fn open(&self, route: &str, input: GateInput<'_>) -> Result<OpenRoute, Vec<Unmet>> {
        match self.gate_status(route, input) {
            GateStatus::Open => Ok(OpenRoute {
                name: route.to_string(),
            }),
            GateStatus::Closed { missing } => Err(missing),
        }
    }
}

#[cfg(test)]
mod tests {
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
        let fresh_otp =
            json!({ "slot": "verified", "provenance": "verifier:otp", "max_age": "15m" });
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
                // codeql[rust/cleartext-logging] — the message prints the JSON condition, not a secret.
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
}
