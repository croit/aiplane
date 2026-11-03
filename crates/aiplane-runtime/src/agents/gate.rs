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
mod tests;
