// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The agent spec: its JSON layout and the validator that runs on every save
//! and again on publish (`docs/agent-spec.md` → "Layout").
//!
//! The validator walks the JSON itself rather than deserializing into typed
//! structs, so it can report *every* problem at once, each with the path the
//! builder UI points at (`routes.billing.when.all[0].slot`), instead of
//! serde's first error at a line and column nobody typed.
//!
//! What it checks:
//! - **Shape.** Unknown keys anywhere are errors, never ignored — a misspelt
//!   `tool_resource` must not silently leave a tool unbound. Types, enums,
//!   durations, origins and regexes are checked; the `finish` schema goes
//!   through [`FinishContract::new`], the run-time validator itself.
//! - **Grants.** Every model, tool, connector and skill the spec names must be
//!   granted to this agent's principal — and on publish, so must the
//!   gateway's default model an unset model key runs on. The spec cannot widen what the
//!   principal holds; it can only pick from it.
//! - **References.** Sub-agents are named by agent id and must exist (and, on
//!   publish, be live). Gate leaves, templates and `set_by` entries must name
//!   slots and verifiers the spec declares.
//! - **Binds.** A bound argument is hidden from the model, so its source is a
//!   state slot the model cannot write (no `llm` in its `set_by`), or a
//!   `{"const": …}` literal.
//!
//! - **Slots.** Each constraint must apply to the slot's type, a `subject`
//!   slot's `schema` must be one the run-time validator enforces, and the
//!   model may not write a `subject`. Run-time meaning is [`super::state`].
//!
//! - **Gates.** Beyond their shape, every leaf must be able to hold: an `eq`
//!   or `in` value the slot would accept, a `provenance` its `set_by` lists
//!   ([`Cond::type_check`]). A route that binds arguments from state needs a
//!   gate that cannot open without a verifier- or host-written slot
//!   ([`Cond::requires_trusted_provenance`]), on every slot it binds from. So
//!   does a route whose sub-agent, or any agent below it, binds a tool.
//!
//! - **The sub-agent graph**, over the live specs of the agents it routes to:
//!   acyclic and at most [`MAX_DEPTH`] agents deep, this one included.
//!
//! **Validation and typing.** [`check`] runs the walk and, only when it found
//! nothing, deserializes the same JSON into the typed [`AgentSpec`]
//! (`spec/model.rs`), which is what runtime code reads. The walk owns the
//! `{path, message}` errors; the types own the defaults. Their key sets are
//! the same, so a spec the walk accepts always deserializes — every accepting
//! test of the walk also proves that, since [`validate`] reports a spec that
//! does not as an issue.

use std::collections::{BTreeSet, HashMap};

use aiplane_core::server::principal::{GrantKind, GrantSet};
use aiplane_core::server::reasoning::HARD_ROUND_CAP;
use aiplane_core::server::run_chain::MAX_DEPTH;
use serde::Serialize;
use serde_json::{Map, Value};

use super::gate::Cond;
use verifiers::HOST_JWT;

mod a2a;
pub mod model;
mod route_kinds;
pub mod secrets;
pub mod tool_access;
mod verifiers;
use super::state::StateSchema;
use crate::finish::FinishContract;
use crate::server::tools::mcp::MCP_ID_PREFIX;
pub(crate) use a2a::{MAX_SKILLS as MAX_A2A_SKILLS, is_skill_id};
pub use model::AgentSpec;

/// One problem with a spec: where, and what to do about it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct SpecIssue {
    pub path: String,
    pub message: String,
}

/// A draft may be incomplete; a published version must be runnable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Draft,
    Publish,
}

/// What a spec is validated against.
pub struct SpecContext<'a> {
    /// The agent whose spec this is — its principal id.
    pub agent_id: &'a str,
    /// That principal's grants, as stored now.
    pub grants: &'a GrantSet,
    /// Every agent id, mapped to whether it has a live version.
    pub agents: &'a HashMap<String, bool>,
    /// Every published agent's live spec, by id: the sub-agent graph the
    /// cycle, depth and bind-reach checks walk.
    pub live_specs: &'a HashMap<String, Value>,
    /// What an unset model key runs on.
    pub model_defaults: &'a ModelDefaults,
    /// The voices each speech model offers, by model
    /// (`UpstreamRegistry::speech_voices_of`): what `publish.voice.voice`
    /// may name on publish. A model missing here offers none.
    pub speech_voices: &'a HashMap<String, Vec<String>>,
    /// `$AIPLANE_ALLOW_PRIVATE_NETWORKS`: whether a URL the spec names (an
    /// A2A card, a JWKS) may be plain http or a private host, as at run time.
    pub allow_private: bool,
}

/// The gateway's default model of each kind
/// (`server::model_choices::gateway_default`): what `main.model` and a
/// `publish.voice` direction run on when the spec names none. `None` when
/// the gateway serves no model of that kind.
#[derive(Debug, Clone, Default)]
pub struct ModelDefaults {
    pub chat: Option<String>,
    pub transcription: Option<String>,
    pub speech: Option<String>,
}

const TOP_KEYS: &[&str] = &[
    "profile",
    "main",
    "scope",
    "state",
    "verifiers",
    "router",
    "routes",
    "finish",
    "publish",
];
const PROFILE_KEYS: &[&str] = &["display", "avatar", "color"];
const SCOPE_KEYS: &[&str] = &["topics", "refusal", "strict", "classifier_model"];
const MAIN_KEYS: &[&str] = &[
    "model",
    "instructions",
    "tools",
    "skills",
    "tool_resources",
    "budget",
];
const INSTRUCTION_KEYS: &[&str] = &["orchestration", "response"];
const TOOL_RESOURCE_KEYS: &[&str] = &["bind", "permission", "approval_timeout"];
const PERMISSIONS: &[&str] = &["always_allow", "always_ask"];
const BUDGET_KEYS: &[&str] = &["rounds", "seconds", "tokens"];
const SLOT_KEYS: &[&str] = &[
    "type",
    "set_by",
    "description",
    "values",
    "min_length",
    "max_length",
    "minimum",
    "maximum",
    "pattern",
    "schema",
    "order",
];
/// Constraint keys and the slot types they apply to. A constraint on a type it
/// cannot apply to would be silently unenforced, so it is an error instead.
const SLOT_CONSTRAINTS: &[(&str, &[&str])] = &[
    ("min_length", &["string"]),
    ("max_length", &["string", "email"]),
    ("pattern", &["string"]),
    ("minimum", &["integer", "number"]),
    ("maximum", &["integer", "number"]),
    ("schema", &["subject"]),
];
const SLOT_TYPES: &[&str] = &[
    "string", "email", "enum", "integer", "number", "boolean", "subject",
];

const ROUTER_KEYS: &[&str] = &["kind", "model", "order"];
const ROUTER_KINDS: &[&str] = &["rules", "classifier"];
const ROUTE_KEYS: &[&str] = &[
    "description",
    "when",
    "agent",
    "task",
    "bind",
    "human",
    "a2a",
    "loop",
];
/// The keys that name a route's target; a route has exactly one.
const ROUTE_TARGETS: &[&str] = &["agent", "human", "a2a", "loop"];
const HUMAN_KEYS: &[&str] = &["notify", "inbox", "timeout", "transcript"];
/// Where a handoff may be announced: Web Push to whoever may answer it, and
/// the agent's Slack and Discord channels.
pub const NOTIFY_CHANNELS: &[&str] = &["push", "slack", "discord"];
const FINISH_KEYS: &[&str] = &["schema"];
const PUBLISH_KEYS: &[&str] = &[
    "origins",
    "idle_ttl",
    "retention_days",
    "audit_retention_days",
    "rate_limits",
    "budget",
    "output_filter",
    "require_passing_tests",
    "a2a",
    "voice",
];
const VOICE_KEYS: &[&str] = &[
    "input",
    "output",
    "voice",
    "speech_model",
    "transcription_model",
];
const RATE_SCOPES: &[&str] = &["visitor", "ip"];
const RATE_KEYS: &[&str] = &["max", "per"];
const BUDGET_PUBLISH_KEYS: &[&str] = &["monthly_cost", "monthly_tokens"];
const OUTPUT_FILTER_KEYS: &[&str] = &["patterns", "action"];
pub(super) const LEAF_KEYS: &[&str] = &["slot", "set", "eq", "in", "provenance", "max_age"];
const MAX_IDENT_LEN: usize = 48;

/// What a route's gate has to guarantee because of what the route binds.
struct BindDemand<'a> {
    /// The slots the route's own `bind` reads, when it has one.
    bound_slots: Option<Vec<String>>,
    /// The routed sub-agent, when it reaches a tool with a `bind`.
    sub_agent: Option<&'a str>,
}

/// The slot each state-sourced bind reads (`verified.customer_id` → `verified`).
fn bound_slots(bind: &Value) -> Vec<String> {
    let mut slots: Vec<String> = bind
        .as_object()
        .into_iter()
        .flat_map(|m| m.values())
        .filter_map(Value::as_str)
        .filter_map(|src| src.strip_prefix("state."))
        .filter_map(|path| path.split('.').next())
        .map(str::to_string)
        .collect();
    slots.sort();
    slots.dedup();
    slots
}

/// Every `route.<name>` a spec's tools bind.
fn route_values_taken(spec: &Value) -> BTreeSet<String> {
    spec.pointer("/main/tool_resources")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|tools| tools.values())
        .filter_map(|r| r.get("bind").and_then(Value::as_object))
        .flat_map(|bind| bind.values())
        .filter_map(Value::as_str)
        .filter_map(|src| src.strip_prefix("route."))
        .map(str::to_string)
        .collect()
}

/// The agent ids a spec's routes dispatch to.
pub fn routed_agents(spec: &Value) -> Vec<&str> {
    spec.get("routes")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|routes| routes.values())
        .flat_map(route_agents)
        .collect()
}

/// The agent ids one route runs: its sub-agent, or a loop's worker and critic.
fn route_agents(route: &Value) -> Vec<&str> {
    ["/agent", "/loop/worker", "/loop/critic"]
        .iter()
        .filter_map(|at| route.pointer(at).and_then(Value::as_str))
        .collect()
}

fn binds_anything(spec: &Value) -> bool {
    let tool_binds = spec
        .pointer("/main/tool_resources")
        .and_then(Value::as_object)
        .is_some_and(|tools| tools.values().any(|r| r.get("bind").is_some()));
    let route_binds = spec
        .get("routes")
        .and_then(Value::as_object)
        .is_some_and(|routes| routes.values().any(|r| r.get("bind").is_some()));
    tool_binds || route_binds
}

/// Whether agent `id`, or any agent it routes to, runs a tool with a `bind`.
fn reaches_bind(live: &HashMap<String, Value>, id: &str, seen: &mut Vec<String>) -> bool {
    if seen.iter().any(|s| s == id) {
        return false;
    }
    seen.push(id.to_string());
    let Some(spec) = live.get(id) else {
        return false;
    };
    binds_anything(spec)
        || routed_agents(spec)
            .into_iter()
            .any(|child| reaches_bind(live, child, seen))
}

/// How many agents deep the graph goes from `id` down, `id` included; the
/// looping path when it reaches an agent already on `trail`.
fn depth_below(
    live: &HashMap<String, Value>,
    id: &str,
    trail: &mut Vec<String>,
) -> Result<usize, Vec<String>> {
    if let Some(at) = trail.iter().position(|t| t == id) {
        let mut cycle = trail[at..].to_vec();
        cycle.push(id.to_string());
        return Err(cycle);
    }
    trail.push(id.to_string());
    let mut deepest = 0;
    if let Some(spec) = live.get(id) {
        for child in routed_agents(spec) {
            deepest = deepest.max(depth_below(live, child, trail)?);
        }
    }
    trail.pop();
    Ok(deepest + 1)
}

/// Every problem with `spec`, in document order. Empty means valid.
pub fn validate(spec: &Value, ctx: &SpecContext<'_>, stage: Stage) -> Vec<SpecIssue> {
    check(spec, ctx, stage).err().unwrap_or_default()
}

/// `spec` as the typed [`AgentSpec`], or every problem with it.
pub fn check(
    spec: &Value,
    ctx: &SpecContext<'_>,
    stage: Stage,
) -> Result<AgentSpec, Vec<SpecIssue>> {
    let mut check = Check {
        ctx,
        stage,
        issues: Vec::new(),
        slots: HashMap::new(),
        verifiers: HashMap::new(),
        schema: None,
    };
    check.spec(spec);
    if !check.issues.is_empty() {
        return Err(check.issues);
    }
    AgentSpec::from_value(spec).map_err(|err| {
        vec![SpecIssue {
            path: String::new(),
            message: format!(
                "passed every check but does not read as an agent spec ({err}) — the validator \
                 and the spec types disagree, which is a bug to report"
            ),
        }]
    })
}

struct Check<'a> {
    ctx: &'a SpecContext<'a>,
    stage: Stage,
    issues: Vec<SpecIssue>,
    /// Slot name → whether the model may write it.
    slots: HashMap<String, bool>,
    /// Verifier id → its `kind`, when it is a known one (`spec/verifiers.rs`).
    verifiers: HashMap<String, Option<String>>,
    /// The typed slots, for checking gates against them; `None` when `state`
    /// is too malformed to read, which is reported already.
    schema: Option<StateSchema>,
}

pub(super) fn join(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_string()
    } else {
        format!("{path}.{key}")
    }
}

pub(super) fn index(path: &str, i: usize) -> String {
    format!("{path}[{i}]")
}

fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && s.len() <= MAX_IDENT_LEN
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// `30s`, `15m`, `2h`, `30d` — a positive count and one unit. A day is 24
/// hours: these are ages and timeouts, not calendar dates.
pub fn parse_duration(s: &str) -> Option<jiff::SignedDuration> {
    let unit = s.chars().last()?;
    let digits = &s[..s.len() - unit.len_utf8()];
    let per_unit: i64 = match unit {
        's' => 1,
        'm' => 60,
        'h' => 3_600,
        'd' => 86_400,
        _ => return None,
    };
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let count: i64 = digits.parse().ok().filter(|n| *n > 0)?;
    count
        .checked_mul(per_unit)
        .map(jiff::SignedDuration::from_secs)
}

/// The spec's own notation for `d`, in its largest whole unit.
pub fn format_duration(d: jiff::SignedDuration) -> String {
    let secs = d.as_secs();
    for (unit, per) in [('d', 86_400), ('h', 3_600), ('m', 60)] {
        if secs % per == 0 && secs >= per {
            return format!("{}{unit}", secs / per);
        }
    }
    format!("{secs}s")
}

fn is_duration(s: &str) -> bool {
    parse_duration(s).is_some()
}

/// Exactly `scheme://host[:port]`, the form a browser sends in `Origin`.
pub fn is_origin(s: &str) -> bool {
    let Some((scheme, rest)) = s.split_once("://") else {
        return false;
    };
    let (host, port) = match rest.rsplit_once(':') {
        Some((h, p)) => (h, Some(p)),
        None => (rest, None),
    };
    matches!(scheme, "http" | "https")
        && !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        && port.is_none_or(|p| p.parse::<u16>().is_ok_and(|n| n > 0))
}

/// What kind of JSON value `v` is, as a message names it: `a string`.
pub(crate) fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

impl<'a> Check<'a> {
    fn issue(&mut self, path: &str, message: impl Into<String>) {
        self.issues.push(SpecIssue {
            path: path.to_string(),
            message: message.into(),
        });
    }

    fn grant_hint(&self, kind: &str, reference: &str) -> String {
        format!(
            "grant it with POST /api/v0/system-principals/{}/grants \
             {{\"kind\": \"{kind}\", \"ref\": \"{reference}\"}}",
            self.ctx.agent_id
        )
    }

    /// The object at `path` with only `allowed` keys, or `None` after
    /// recording why not.
    fn object<'v>(
        &mut self,
        v: &'v Value,
        path: &str,
        allowed: &[&str],
    ) -> Option<&'v Map<String, Value>> {
        let Value::Object(map) = v else {
            self.issue(path, format!("must be an object, not {}", type_name(v)));
            return None;
        };
        for key in map.keys() {
            if !allowed.contains(&key.as_str()) {
                self.issue(
                    &join(path, key),
                    format!(
                        "unknown key `{key}` — expected one of: {}",
                        allowed.join(", ")
                    ),
                );
            }
        }
        Some(map)
    }

    /// A map keyed by identifiers (`state`, `routes`, `verifiers`).
    fn named_map<'v>(&mut self, v: &'v Value, path: &str) -> Vec<(&'v str, &'v Value)> {
        let Value::Object(map) = v else {
            self.issue(
                path,
                format!("must be an object keyed by name, not {}", type_name(v)),
            );
            return Vec::new();
        };
        let mut out = Vec::new();
        for (name, value) in map {
            if is_ident(name) {
                out.push((name.as_str(), value));
            } else {
                self.issue(
                    &join(path, name),
                    format!(
                        "`{name}` is not a valid name — use lowercase letters, digits and `_`, \
                         starting with a letter, at most {MAX_IDENT_LEN} characters"
                    ),
                );
            }
        }
        out
    }

    fn string<'v>(&mut self, v: &'v Value, path: &str) -> Option<&'v str> {
        match v {
            Value::String(s) => Some(s),
            other => {
                self.issue(path, format!("must be a string, not {}", type_name(other)));
                None
            }
        }
    }

    fn nullable_string(&mut self, v: &Value, path: &str) {
        if !v.is_null() {
            self.string(v, path);
        }
    }

    fn one_of(&mut self, v: &Value, path: &str, allowed: &[&str]) -> Option<String> {
        let s = self.string(v, path)?;
        if allowed.contains(&s) {
            Some(s.to_string())
        } else {
            self.issue(
                path,
                format!(
                    "`{s}` is not allowed here — use one of: {}",
                    allowed.join(", ")
                ),
            );
            None
        }
    }

    fn positive_int(&mut self, v: &Value, path: &str, max: Option<u64>) {
        match v.as_u64() {
            Some(n) if n >= 1 && max.is_none_or(|m| n <= m) => {}
            _ => {
                let range = match max {
                    Some(m) => format!("between 1 and {m}"),
                    None => "of at least 1".into(),
                };
                self.issue(path, format!("must be a whole number {range}"));
            }
        }
    }

    fn duration(&mut self, v: &Value, path: &str) {
        if let Some(s) = self.string(v, path)
            && !is_duration(s)
        {
            self.issue(
                path,
                format!("`{s}` is not a duration — write a count and a unit, e.g. `30s`, `15m`, `2h`, `30d`"),
            );
        }
    }

    /// `{max, per}`, both required; `per` at most `longest` when given.
    fn rate(&mut self, v: &Value, path: &str, longest: Option<jiff::SignedDuration>) {
        let Some(rate) = self.object(v, path, RATE_KEYS) else {
            return;
        };
        for key in RATE_KEYS {
            let at = join(path, key);
            match (rate.get(*key), *key) {
                (Some(x), "max") => self.positive_int(x, &at, None),
                (Some(x), _) => self.bounded_duration(x, &at, longest),
                (None, _) => self.issue(
                    &at,
                    "a rate needs both `max` (how many) and `per` (over how long, e.g. `10m`)"
                        .to_string(),
                ),
            }
        }
    }

    /// A duration no longer than `longest`, when given.
    fn bounded_duration(&mut self, v: &Value, path: &str, longest: Option<jiff::SignedDuration>) {
        self.duration(v, path);
        if let (Some(longest), Some(d)) = (longest, v.as_str().and_then(parse_duration))
            && d > longest
        {
            self.issue(
                path,
                format!(
                    "must be at most {} — a longer window is not kept",
                    format_duration(longest)
                ),
            );
        }
    }

    fn regex(&mut self, v: &Value, path: &str) {
        if let Some(s) = self.string(v, path)
            && let Err(err) = regex::Regex::new(s)
        {
            self.issue(path, format!("is not a valid regular expression: {err}"));
        }
    }

    fn string_list<'v>(&mut self, v: &'v Value, path: &str) -> Vec<(String, &'v str)> {
        let Value::Array(items) = v else {
            self.issue(
                path,
                format!("must be an array of strings, not {}", type_name(v)),
            );
            return Vec::new();
        };
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        for (i, item) in items.iter().enumerate() {
            let p = index(path, i);
            if let Some(s) = self.string(item, &p) {
                if seen.insert(s) {
                    out.push((p, s));
                } else {
                    self.issue(&p, format!("`{s}` is listed twice"));
                }
            }
        }
        out
    }

    fn require_grant(&mut self, path: &str, kind: GrantKind, reference: &str, what: &str) {
        if !self.ctx.grants.has(kind, reference) {
            let hint = self.grant_hint(kind.as_str(), reference);
            self.issue(
                path,
                format!(
                    "{what} `{reference}` is not granted to this agent — {hint}, or remove it \
                     from the spec"
                ),
            );
        }
    }

    // --- the layout --------------------------------------------------------

    fn spec(&mut self, spec: &Value) {
        let Some(top) = self.object(spec, "", TOP_KEYS) else {
            return;
        };
        // Declarations first, so references to them can be checked wherever
        // they appear.
        if let Some(v) = top.get("verifiers") {
            self.verifiers_decl(v);
        }
        if let Some(v) = top.get("state") {
            self.state(v);
        }
        self.schema = StateSchema::from_spec(spec).ok();
        if let Some(v) = top.get("verifiers") {
            self.verifier_refs(v);
        }
        if let Some(v) = top.get("profile") {
            self.profile(v);
        }
        if let Some(v) = top.get("scope") {
            self.scope(v);
        }
        match top.get("main") {
            Some(v) => self.main(v),
            None if self.stage == Stage::Publish => self.issue(
                "main",
                "a published agent needs `main` with its instructions",
            ),
            None => {}
        }
        if let Some(v) = top.get("router") {
            self.router(v, top.get("routes"));
        }
        if let Some(v) = top.get("routes") {
            for (name, route) in self.named_map(v, "routes") {
                self.route(route, &join("routes", name));
            }
        }
        if let Some(v) = top.get("finish") {
            self.finish(v);
        }
        if let Some(v) = top.get("publish") {
            self.publish(v);
        }
    }

    fn profile(&mut self, v: &Value) {
        let Some(map) = self.object(v, "profile", PROFILE_KEYS) else {
            return;
        };
        if let Some(d) = map.get("display") {
            self.string(d, "profile.display");
        }
        if let Some(x) = map.get("avatar") {
            self.nullable_string(x, "profile.avatar");
        }
        if let Some(Value::String(c)) = map.get("color")
            && !c.trim().is_empty()
            && !model::is_hex_color(c.trim())
        {
            self.issue(
                "profile.color",
                format!("`{c}` is not a colour — write it as `#rrggbb`, for example `#2563eb`"),
            );
        } else if let Some(x) = map.get("color") {
            self.nullable_string(x, "profile.color");
        }
    }

    /// Checked on every save, not only on publish: the test chat runs a
    /// draft, and a strict scope without topics or a refusal cannot run.
    fn scope(&mut self, v: &Value) {
        let Some(map) = self.object(v, "scope", SCOPE_KEYS) else {
            return;
        };
        let mut topics = 0;
        if let Some(list) = map.get("topics") {
            for (p, topic) in self.string_list(list, "scope.topics") {
                if topic.trim().is_empty() {
                    self.issue(
                        &p,
                        "a topic must not be blank — name it or remove the entry",
                    );
                } else {
                    topics += 1;
                }
            }
        }
        let refusal = map
            .get("refusal")
            .and_then(|r| self.string(r, "scope.refusal"))
            .is_some_and(|r| !r.trim().is_empty());
        let strict = match map.get("strict") {
            Some(Value::Bool(b)) => *b,
            Some(_) => {
                self.issue(
                    "scope.strict",
                    "must be true or false: true lets a classifier refuse every message outside \
                     the topics before the agent's model sees it",
                );
                false
            }
            None => false,
        };
        if let Some(model) = map.get("classifier_model")
            && let Some(name) = self.string(model, "scope.classifier_model")
        {
            self.require_grant("scope.classifier_model", GrantKind::Model, name, "model");
        }
        if strict && topics == 0 {
            self.issue(
                "scope.topics",
                "a strict scope must list the topics the agent covers — a message is judged \
                 against them; add at least one, or set `strict` to false",
            );
        }
        if strict && !refusal {
            self.issue(
                "scope.refusal",
                "a strict scope needs the `refusal` the visitor gets as the reply to a message \
                 outside the topics — write it, or set `strict` to false",
            );
        }
    }

    fn main(&mut self, v: &Value) {
        let Some(map) = self.object(v, "main", MAIN_KEYS) else {
            return;
        };
        let chat_default = self.ctx.model_defaults.chat.as_deref();
        self.model_or_default(
            map.get("model"),
            "main.model",
            chat_default,
            ("the agent", "chat"),
            "in the setup's \"Task & tone\" step choose its model (that grants it)",
        );
        self.instructions(map.get("instructions"));

        let mut tools = BTreeSet::new();
        if let Some(list) = map.get("tools") {
            for (path, tool) in self.string_list(list, "main.tools") {
                self.tool_reference(&path, tool);
                tools.insert(tool.to_string());
            }
        }
        if let Some(list) = map.get("skills") {
            for (path, skill) in self.string_list(list, "main.skills") {
                self.require_grant(&path, GrantKind::Skill, skill, "skill");
            }
        }
        if let Some(resources) = map.get("tool_resources") {
            self.tool_resources(resources, &tools);
        }
        if let Some(budget) = map.get("budget")
            && let Some(b) = self.object(budget, "main.budget", BUDGET_KEYS)
        {
            if let Some(r) = b.get("rounds") {
                self.positive_int(r, "main.budget.rounds", Some(u64::from(HARD_ROUND_CAP)));
            }
            for key in ["seconds", "tokens"] {
                if let Some(x) = b.get(key) {
                    self.positive_int(x, &join("main.budget", key), None);
                }
            }
        }
    }

    fn instructions(&mut self, v: Option<&Value>) {
        let path = "main.instructions";
        let mut written = false;
        if let Some(v) = v
            && let Some(map) = self.object(v, path, INSTRUCTION_KEYS)
        {
            for key in INSTRUCTION_KEYS {
                if let Some(x) = map.get(*key)
                    && let Some(s) = self.string(x, &join(path, key))
                {
                    written |= !s.trim().is_empty();
                }
            }
        }
        if self.stage == Stage::Publish && !written {
            self.issue(
                path,
                "a published agent needs instructions — write `orchestration` (what to do), \
                 `response` (how to answer), or both",
            );
        }
    }

    /// A registry tool needs a `tool` grant; an MCP tool
    /// (`mcp__<connector>__<tool>`) needs its connector granted.
    fn tool_reference(&mut self, path: &str, tool: &str) {
        let Some(rest) = tool.strip_prefix(MCP_ID_PREFIX) else {
            self.require_grant(path, GrantKind::Tool, tool, "tool");
            return;
        };
        match rest.split_once("__") {
            Some((connector, name)) if !connector.is_empty() && !name.is_empty() => {
                if !self.ctx.grants.has(GrantKind::Connector, connector) {
                    let hint = self.grant_hint("connector", connector);
                    self.issue(
                        path,
                        format!(
                            "tool `{tool}` comes from connector `{connector}`, which is not \
                             granted to this agent — {hint}, or remove the tool from the spec"
                        ),
                    );
                }
            }
            _ => self.issue(
                path,
                format!(
                    "`{tool}` is not an MCP tool id — they read `{MCP_ID_PREFIX}<connector>__<tool>`"
                ),
            ),
        }
    }

    fn tool_resources(&mut self, v: &Value, tools: &BTreeSet<String>) {
        let path = "main.tool_resources";
        let Value::Object(map) = v else {
            self.issue(
                path,
                format!("must be an object keyed by tool, not {}", type_name(v)),
            );
            return;
        };
        for (tool, resource) in map {
            let p = join(path, tool);
            if !tools.contains(tool) {
                self.issue(
                    &p,
                    format!(
                        "configures `{tool}`, which is not in `main.tools` — add it there or \
                         remove this entry"
                    ),
                );
            }
            let Some(r) = self.object(resource, &p, TOOL_RESOURCE_KEYS) else {
                continue;
            };
            if let Some(perm) = r.get("permission") {
                self.one_of(perm, &join(&p, "permission"), PERMISSIONS);
            }
            if let Some(timeout) = r.get("approval_timeout") {
                self.duration(timeout, &join(&p, "approval_timeout"));
            }
            if let Some(bind) = r.get("bind") {
                self.bind(bind, &join(&p, "bind"), true);
            }
        }
    }

    /// `{param: source}`. A source is `"state.<slot>[.<field>]"` (a slot the
    /// model cannot write), `{"const": v}`, or — in `tool_resources` only —
    /// `"route.<name>"`, a value the dispatching route passes.
    fn bind(&mut self, v: &Value, path: &str, allow_route: bool) {
        let Value::Object(map) = v else {
            self.issue(
                path,
                format!(
                    "must be an object of argument → source, not {}",
                    type_name(v)
                ),
            );
            return;
        };
        for (arg, source) in map {
            let p = join(path, arg);
            let forms = if allow_route {
                "`state.<slot>` (`\"state.verified.customer_id\"`), `route.<name>` (a value \
                 the dispatching route passes) or a fixed value (`{\"const\": …}`)"
            } else {
                "`state.<slot>` (`\"state.verified.customer_id\"`) or a fixed value \
                 (`{\"const\": …}`) — a route passes values from its caller's state"
            };
            match source {
                Value::String(src) if src.starts_with("state.") => {
                    self.bind_slot(&p, &src["state.".len()..])
                }
                Value::String(src)
                    if allow_route && src.strip_prefix("route.").is_some_and(is_ident) => {}
                Value::Object(lit) if lit.len() == 1 && lit.contains_key("const") => {}
                Value::String(src) => {
                    self.issue(&p, format!("`{src}` is not a bind source — write {forms}"))
                }
                other => self.issue(
                    &p,
                    format!("a bind source is {forms}, not {}", type_name(other)),
                ),
            }
        }
    }

    fn bind_slot(&mut self, path: &str, slot_path: &str) {
        let slot = slot_path.split('.').next().unwrap_or_default();
        match self.slots.get(slot) {
            None => self.issue(
                path,
                format!(
                    "binds from `{slot_path}`, but `{slot}` is not a slot in `state` — declare \
                     it, or bind a fixed value with {{\"const\": …}}"
                ),
            ),
            Some(true) => self.issue(
                path,
                format!(
                    "binds from slot `{slot}`, which the model can set (`llm` in its `set_by`). \
                     A bound argument decides whose data a tool touches, so it must come from a \
                     verifier or the host — remove `llm` from `state.{slot}.set_by` or bind \
                     another slot"
                ),
            ),
            Some(false) => {}
        }
    }

    fn state(&mut self, v: &Value) {
        for (name, slot) in self.named_map(v, "state") {
            let p = join("state", name);
            let Some(map) = self.object(slot, &p, SLOT_KEYS) else {
                continue;
            };
            let ty = match map.get("type") {
                Some(t) => self.one_of(t, &join(&p, "type"), SLOT_TYPES),
                None => {
                    self.issue(
                        &join(&p, "type"),
                        format!("a slot needs a `type` — one of: {}", SLOT_TYPES.join(", ")),
                    );
                    None
                }
            };
            let llm = self.set_by(map.get("set_by"), &join(&p, "set_by"));
            self.slots.insert(name.to_string(), llm);
            if llm && ty.as_deref() == Some("subject") {
                self.issue(
                    &join(&p, "set_by"),
                    "lists `llm`, but a `subject` slot says whose data the agent acts on — only a \
                     verifier or the host may set it. Remove `llm`, or use a `string` slot for \
                     what the visitor merely claims",
                );
            }
            if let Some(ty) = ty.as_deref() {
                for (key, types) in SLOT_CONSTRAINTS {
                    if map.contains_key(*key) && !types.contains(&ty) {
                        self.issue(
                            &join(&p, key),
                            format!(
                                "`{key}` does not apply to a slot of type `{ty}` — it applies to: {}",
                                types.join(", ")
                            ),
                        );
                    }
                }
            }
            if let Some(schema) = map.get("schema")
                && ty.as_deref() == Some("subject")
            {
                self.subject_schema(schema, &join(&p, "schema"));
            }

            match (ty.as_deref(), map.get("values")) {
                (Some("enum"), Some(Value::Array(values))) if !values.is_empty() => {}
                (Some("enum"), _) => self.issue(
                    &join(&p, "values"),
                    "an `enum` slot needs a non-empty `values` list",
                ),
                (_, Some(_)) => self.issue(
                    &join(&p, "values"),
                    "`values` only applies to a slot of type `enum`",
                ),
                _ => {}
            }
            for key in ["min_length", "max_length", "order"] {
                if let Some(x) = map.get(key)
                    && x.as_u64().is_none()
                {
                    self.issue(&join(&p, key), "must be a whole number of at least 0");
                }
            }
            for key in ["minimum", "maximum"] {
                if let Some(x) = map.get(key)
                    && !x.is_number()
                {
                    self.issue(&join(&p, key), "must be a number");
                }
            }
            if let Some(x) = map.get("description") {
                self.string(x, &join(&p, "description"));
            }
            if let Some(x) = map.get("pattern") {
                self.regex(x, &join(&p, "pattern"));
            }
        }
    }

    /// A subject is an object; its schema goes through the same validator
    /// that enforces it at run time.
    fn subject_schema(&mut self, schema: &Value, path: &str) {
        if let Err(err) = FinishContract::new(schema.clone()) {
            self.issue(
                path,
                format!(
                    "at `{}`, {} — a subject schema supports only type, properties, required, \
                     enum, items and a boolean additionalProperties",
                    err.path, err.problem
                ),
            );
        } else if schema.get("type").is_some_and(|t| t != "object") {
            self.issue(
                path,
                "a subject is an object — set `type` to `object` and describe its fields in \
                 `properties`",
            );
        }
    }

    /// Whether `llm` is among the writers. A slot nobody can write is
    /// refused: its gate could never open.
    fn set_by(&mut self, v: Option<&Value>, path: &str) -> bool {
        let Some(v) = v else {
            self.issue(
                path,
                "a slot needs `set_by` — who may write it: `llm`, `host` or `verifier:<id>`",
            );
            return false;
        };
        let writers = self.string_list(v, path);
        if writers.is_empty() && v.is_array() {
            self.issue(
                path,
                "lists nobody — a slot nobody can write never opens a gate",
            );
        }
        let mut llm = false;
        for (p, writer) in writers {
            llm |= writer == "llm";
            self.provenance(&p, writer);
        }
        llm
    }

    fn provenance(&mut self, path: &str, value: &str) {
        match value {
            "llm" | "host" => {}
            other => match other.strip_prefix("verifier:") {
                Some(id) if self.verifier_kind(id) == Some(HOST_JWT) => self.issue(
                    path,
                    format!(
                        "verifier `{id}` is a `host_jwt` verifier, whose slots are written as \
                         `host` — list `host` instead of `{other}`"
                    ),
                ),
                Some(id) if self.verifiers.contains_key(id) => {}
                Some(id) => self.issue(
                    path,
                    format!("names verifier `{id}`, which is not declared in `verifiers`"),
                ),
                None => self.issue(
                    path,
                    format!("`{other}` is not a writer — use `llm`, `host` or `verifier:<id>`"),
                ),
            },
        }
    }

    fn router(&mut self, v: &Value, routes: Option<&Value>) {
        let Some(map) = self.object(v, "router", ROUTER_KEYS) else {
            return;
        };
        if let Some(order) = map.get("order") {
            if map.get("kind").and_then(Value::as_str) == Some("classifier") {
                self.issue(
                    "router.order",
                    "`order` ranks open routes for a `rules` router; a `classifier` router picks \
                     by itself — remove `order` or switch `kind` to `rules`",
                );
            } else {
                for (p, name) in self.string_list(order, "router.order") {
                    if routes.and_then(|r| r.get(name)).is_none() {
                        self.issue(
                            &p,
                            format!("`{name}` is not a route — `order` lists names from `routes`"),
                        );
                    }
                }
            }
        }
        let kind = match map.get("kind") {
            Some(k) => self.one_of(k, "router.kind", ROUTER_KINDS),
            None => {
                self.issue(
                    "router.kind",
                    "the router needs a `kind`: `rules` or `classifier`",
                );
                None
            }
        };
        match map.get("model") {
            Some(model) => {
                if let Some(name) = self.string(model, "router.model") {
                    self.require_grant("router.model", GrantKind::Model, name, "model");
                }
            }
            None if kind.as_deref() == Some("classifier") => self.issue(
                "router.model",
                "a `classifier` router needs a `model` to classify with — name one of the \
                 agent's model grants",
            ),
            None => {}
        }
    }

    fn route(&mut self, v: &Value, path: &str) {
        let Some(map) = self.object(v, path, ROUTE_KEYS) else {
            return;
        };
        if let Some(d) = map.get("description") {
            self.string(d, &join(path, "description"));
        }
        match map.get("when") {
            Some(when) => {
                let at = join(path, "when");
                let before = self.issues.len();
                self.cond(when, &at);
                if self.issues.len() == before {
                    let demand = BindDemand {
                        bound_slots: map.get("bind").map(bound_slots),
                        sub_agent: route_agents(v)
                            .into_iter()
                            .find(|id| reaches_bind(self.ctx.live_specs, id, &mut Vec::new())),
                    };
                    self.gate_semantics(when, &at, demand);
                }
            }
            None => self.issue(
                &join(path, "when"),
                "a route needs a `when` gate — use `{\"slot\": …, \"set\": true}` for the \
                 simplest one",
            ),
        }
        let targets: Vec<&str> = ROUTE_TARGETS
            .iter()
            .copied()
            .filter(|k| map.contains_key(*k))
            .collect();
        match targets.as_slice() {
            ["agent"] => {
                let agent = &map["agent"];
                self.sub_agent(agent, &join(path, "agent"));
                self.task_and_bind(map, path, "a sub-agent");
                if let Some(id) = agent.as_str() {
                    self.route_contract(id, map.get("bind"), path);
                }
            }
            ["human"] => {
                let human = &map["human"];
                if let Some(h) = self.object(human, &join(path, "human"), HUMAN_KEYS) {
                    if let Some(n) = h.get("notify") {
                        let at = join(path, "human.notify");
                        for (item_path, item) in self.string_list(n, &at) {
                            if !NOTIFY_CHANNELS.contains(&item) {
                                self.issue(
                                    &item_path,
                                    format!(
                                        "`{item}` is not a notification channel — use {}",
                                        NOTIFY_CHANNELS
                                            .iter()
                                            .map(|c| format!("`{c}`"))
                                            .collect::<Vec<_>>()
                                            .join(", ")
                                    ),
                                );
                            }
                        }
                    }
                    if let Some(i) = h.get("inbox") {
                        self.string(i, &join(path, "human.inbox"));
                    }
                    if let Some(t) = h.get("timeout") {
                        self.duration(t, &join(path, "human.timeout"));
                    }
                    if let Some(t) = h.get("transcript")
                        && !t.is_boolean()
                    {
                        self.issue(
                            &join(path, "human.transcript"),
                            "`transcript` is `true` or `false`",
                        );
                    }
                }
                for key in ["task", "bind"] {
                    if map.contains_key(key) {
                        self.issue(
                            &join(path, key),
                            format!("`{key}` does not apply to a route to a person"),
                        );
                    }
                }
            }
            ["a2a"] => self.a2a_route(map, path),
            ["loop"] => self.loop_route(map, path),
            [] => self.issue(
                path,
                "a route needs a target — `agent` (a sub-agent's id), `human`, `a2a` (an \
                 external agent) or `loop` (a worker and a critic)",
            ),
            several => self.issue(
                path,
                format!(
                    "a route has exactly one target, and this one names {} — keep one",
                    several
                        .iter()
                        .map(|k| format!("`{k}`"))
                        .collect::<Vec<_>>()
                        .join(" and ")
                ),
            ),
        }
    }

    /// What a route and the live sub-agent it dispatches must agree on: the
    /// route passes exactly the `route.<name>` values the sub-agent's tools
    /// bind, and, to be published, the sub-agent says what it returns.
    fn route_contract(&mut self, id: &str, bind: Option<&Value>, path: &str) {
        let Some(sub) = self.ctx.live_specs.get(id) else {
            return;
        };
        let passed: BTreeSet<&str> = bind
            .and_then(Value::as_object)
            .into_iter()
            .flat_map(|m| m.keys().map(String::as_str))
            .collect();
        let taken = route_values_taken(sub);
        let lacking: Vec<String> = taken
            .iter()
            .filter(|n| !passed.contains(n.as_str()))
            .map(|n| format!("`route.{n}`"))
            .collect();
        if !lacking.is_empty() {
            self.issue(
                &join(path, "bind"),
                format!(
                    "sub-agent `{id}` binds {}, which this route does not pass — add each name \
                     to this route's `bind` with a `state.<slot>` source",
                    lacking.join(", ")
                ),
            );
        }
        for name in passed.iter().filter(|n| !taken.contains(**n)) {
            self.issue(
                &join(&join(path, "bind"), name),
                format!(
                    "sub-agent `{id}` never binds `route.{name}`, so this value would reach no \
                     tool — bind it in the sub-agent's `tool_resources` or remove it here"
                ),
            );
        }
        if self.stage == Stage::Publish && sub.pointer("/finish/schema").is_none() {
            self.issue(
                &join(path, "agent"),
                format!(
                    "sub-agent `{id}` declares no `finish` schema, and a routed sub-agent must \
                     say what it returns — add `finish.schema` to it and publish it again"
                ),
            );
        }
    }

    fn sub_agent(&mut self, v: &Value, path: &str) {
        let Some(id) = self.string(v, path) else {
            return;
        };
        if id == self.ctx.agent_id {
            self.issue(path, "an agent cannot route to itself");
            return;
        }
        match self.ctx.agents.get(id) {
            None => self.issue(
                path,
                format!(
                    "there is no agent with id `{id}` — sub-agents are referenced by agent id, \
                     as listed by GET /api/v0/agents"
                ),
            ),
            Some(false) if self.stage == Stage::Publish => self.issue(
                path,
                format!(
                    "sub-agent `{id}` has never been published, so this route could not run — \
                     publish it first"
                ),
            ),
            Some(_) => self.sub_agent_graph(id, path),
        }
    }

    /// Every `{slot}` / `{slot.field}` placeholder must name a declared slot.
    fn template(&mut self, v: &Value, path: &str) {
        let Some(text) = self.string(v, path) else {
            return;
        };
        let mut rest = text;
        while let Some(open) = rest.find('{') {
            let after = &rest[open + 1..];
            let Some(close) = after.find('}') else {
                self.issue(path, "has a `{` that is never closed");
                return;
            };
            let placeholder = after[..close].trim();
            let slot = placeholder.split('.').next().unwrap_or_default();
            if !self.slots.contains_key(slot) {
                self.issue(
                    path,
                    format!("uses `{{{placeholder}}}`, but `{slot}` is not a slot in `state`"),
                );
            }
            rest = &after[close + 1..];
        }
    }

    /// The gate condition tree (`docs/agent-spec.md` → "Gates"). Only its shape and the
    /// slots it names; the gate checks the types (`docs/agent-spec.md` → "Gates").
    fn cond(&mut self, v: &Value, path: &str) {
        let Value::Object(map) = v else {
            self.issue(
                path,
                format!("a condition must be an object, not {}", type_name(v)),
            );
            return;
        };
        for combinator in ["all", "any"] {
            if let Some(children) = map.get(combinator) {
                if map.len() != 1 {
                    self.issue(
                        path,
                        format!("`{combinator}` must be the only key of its condition"),
                    );
                }
                match children {
                    Value::Array(items) if !items.is_empty() => {
                        for (i, child) in items.iter().enumerate() {
                            self.cond(child, &index(&join(path, combinator), i));
                        }
                    }
                    _ => self.issue(
                        &join(path, combinator),
                        "must be a non-empty array of conditions",
                    ),
                }
                return;
            }
        }
        if let Some(child) = map.get("not") {
            if map.len() != 1 {
                self.issue(path, "`not` must be the only key of its condition");
            }
            self.cond(child, &join(path, "not"));
            return;
        }
        for key in map.keys() {
            if !LEAF_KEYS.contains(&key.as_str()) {
                self.issue(
                    &join(path, key),
                    format!(
                        "unknown key `{key}` — a condition is `all`, `any`, `not`, or a leaf \
                         with: {}",
                        LEAF_KEYS.join(", ")
                    ),
                );
            }
        }
        match map.get("slot") {
            Some(slot) => {
                if let Some(name) = self.string(slot, &join(path, "slot"))
                    && !self.slots.contains_key(name)
                {
                    self.issue(
                        &join(path, "slot"),
                        format!("`{name}` is not a slot in `state`"),
                    );
                }
            }
            None => self.issue(&join(path, "slot"), "a leaf condition needs a `slot`"),
        }
        if !LEAF_KEYS[1..].iter().any(|k| map.contains_key(*k)) {
            self.issue(
                path,
                "a leaf condition checks nothing — add `set`, `eq`, `in`, `provenance` or \
                 `max_age`",
            );
        }
        if let Some(x) = map.get("set")
            && !x.is_boolean()
        {
            self.issue(&join(path, "set"), "must be true or false");
        }
        if let Some(x) = map.get("in")
            && !x.is_array()
        {
            self.issue(&join(path, "in"), "must be an array of values");
        }
        if let Some(x) = map.get("provenance") {
            let p = join(path, "provenance");
            if let Some(s) = self.string(x, &p) {
                self.provenance(&p, s);
            }
        }
        if let Some(x) = map.get("max_age") {
            self.duration(x, &join(path, "max_age"));
        }
    }

    /// A well-shaped gate, checked against the slots: no leaf that can never
    /// hold, and — on a route that binds arguments from state — a gate that
    /// cannot open on model-written slots alone (§4 "Type-checked").
    fn gate_semantics(&mut self, when: &Value, path: &str, demand: BindDemand<'_>) {
        let Ok(cond) = Cond::parse(when) else {
            return;
        };
        let problems = self
            .schema
            .as_ref()
            .map(|schema| cond.type_check(schema))
            .unwrap_or_default();
        for (at, message) in problems {
            self.issue(&join(path, &at), message);
        }
        const LEAF_HINT: &str = "add a leaf such as {\"slot\": \"verified\", \"provenance\": \
                                 \"verifier:<id>\"} that every way through the gate has to pass";
        if !cond.requires_trusted_provenance() {
            if demand.bound_slots.is_some() {
                self.issue(
                    path,
                    format!(
                        "this route binds arguments from state, so its gate must require a slot \
                         set by a verifier or the host — {LEAF_HINT}"
                    ),
                );
            } else if let Some(agent) = demand.sub_agent {
                self.issue(
                    path,
                    format!(
                        "sub-agent `{agent}` reaches a tool with bound arguments, so this route's \
                         gate must require a slot set by a verifier or the host — {LEAF_HINT}"
                    ),
                );
            }
            return;
        }
        for slot in demand.bound_slots.unwrap_or_default() {
            if !cond.requires_trusted_provenance_of(&slot) {
                self.issue(
                    path,
                    format!(
                        "this route binds an argument from `{slot}`, so its gate must require \
                         `{slot}` itself to be set by a verifier or the host — add \
                         {{\"slot\": \"{slot}\", \"provenance\": …}} on every way through the gate"
                    ),
                );
            }
        }
    }

    /// The sub-agent graph below a route: no agent reached twice on one path,
    /// and at most [`MAX_DEPTH`] agents from this one down.
    fn sub_agent_graph(&mut self, id: &str, path: &str) {
        let mut trail = vec![self.ctx.agent_id.to_string()];
        match depth_below(self.ctx.live_specs, id, &mut trail) {
            Err(cycle) => self.issue(
                path,
                format!(
                    "routing to `{id}` loops back: {} — an agent cannot reach itself through its \
                     sub-agents; change the routes so they do not loop",
                    cycle.join(" → ")
                ),
            ),
            Ok(below) if below + 1 > MAX_DEPTH => self.issue(
                path,
                format!(
                    "routing to `{id}` nests {} agents deep, and at most {MAX_DEPTH} are allowed \
                     — route to a sub-agent with fewer levels below it",
                    below + 1
                ),
            ),
            Ok(_) => {}
        }
    }

    fn finish(&mut self, v: &Value) {
        let Some(map) = self.object(v, "finish", FINISH_KEYS) else {
            return;
        };
        let Some(schema) = map.get("schema") else {
            self.issue("finish.schema", "`finish` needs a `schema` for the result");
            return;
        };
        if let Err(err) = FinishContract::new(schema.clone()) {
            let at = if err.path.is_empty() || err.path == "/" {
                "finish.schema".to_string()
            } else {
                format!("finish.schema{}", err.path)
            };
            self.issue(&at, err.to_string());
        }
    }

    fn publish(&mut self, v: &Value) {
        let Some(map) = self.object(v, "publish", PUBLISH_KEYS) else {
            return;
        };
        if let Some(origins) = map.get("origins") {
            for (p, origin) in self.string_list(origins, "publish.origins") {
                if !is_origin(origin) {
                    self.issue(
                        &p,
                        format!(
                            "`{origin}` is not an origin — write exactly `https://host` or \
                             `https://host:port`, without a path or trailing slash"
                        ),
                    );
                }
            }
        }
        if let Some(x) = map.get("idle_ttl") {
            self.duration(x, "publish.idle_ttl");
        }
        if let Some(x) = map.get("retention_days") {
            self.positive_int(x, "publish.retention_days", None);
        }
        if let Some(x) = map.get("audit_retention_days") {
            self.positive_int(x, "publish.audit_retention_days", None);
            let conversations = map
                .get("retention_days")
                .and_then(Value::as_u64)
                .unwrap_or(crate::agents::retention::DEFAULT_RETENTION_DAYS as u64);
            if let Some(days) = x.as_u64()
                && days >= 1
                && days < conversations
            {
                self.issue(
                    "publish.audit_retention_days",
                    format!(
                        "is {days} days, shorter than the {conversations} days conversations are \
                         kept (`publish.retention_days`); the activity log must outlive the \
                         conversations it records — raise it to at least {conversations}"
                    ),
                );
            }
        }
        if let Some(x) = map.get("a2a") {
            self.a2a(x);
        }
        if let Some(x) = map.get("require_passing_tests")
            && !x.is_boolean()
        {
            self.issue(
                "publish.require_passing_tests",
                "must be true or false: true blocks publishing until the latest test run of this                  draft is green"
                    .to_string(),
            );
        }
        if let Some(rates) = map.get("rate_limits")
            && let Some(rates) = self.object(rates, "publish.rate_limits", RATE_SCOPES)
        {
            for scope in RATE_SCOPES {
                if let Some(rate) = rates.get(*scope) {
                    self.rate(rate, &join("publish.rate_limits", scope), None);
                }
            }
        }
        if let Some(budget) = map.get("budget")
            && let Some(budget) = self.object(budget, "publish.budget", BUDGET_PUBLISH_KEYS)
        {
            if let Some(x) = budget.get("monthly_cost")
                && !x.as_f64().is_some_and(|c| c > 0.0)
            {
                self.issue(
                    "publish.budget.monthly_cost",
                    "must be an amount above 0, in the currency the models are priced in"
                        .to_string(),
                );
            }
            if let Some(x) = budget.get("monthly_tokens") {
                self.positive_int(x, "publish.budget.monthly_tokens", None);
            }
        }
        if let Some(voice) = map.get("voice") {
            self.voice(voice);
        }
        if let Some(filter) = map.get("output_filter")
            && let Some(f) = self.object(filter, "publish.output_filter", OUTPUT_FILTER_KEYS)
            && let Some(patterns) = f.get("patterns")
        {
            for (name, pattern) in self.named_map(patterns, "publish.output_filter.patterns") {
                self.regex(pattern, &join("publish.output_filter.patterns", name));
            }
            if let Some(action) = f.get("action") {
                self.one_of(
                    action,
                    "publish.output_filter.action",
                    super::output_filter::Action::NAMES,
                );
            }
        }
    }

    /// `publish.voice`: each direction that is on runs on the model named
    /// here, else on the gateway's default model for it — one the agent
    /// must hold a grant on before it is published.
    fn voice(&mut self, v: &Value) {
        let Some(map) = self.object(v, "publish.voice", VOICE_KEYS) else {
            return;
        };
        let mut on = |key: &str| match map.get(key) {
            Some(Value::Bool(b)) => *b,
            Some(_) => {
                self.issue(
                    &join("publish.voice", key),
                    "must be true or false: true offers the visitor this direction of voice",
                );
                false
            }
            None => false,
        };
        let (input, output) = (on("input"), on("output"));
        let defaults = self.ctx.model_defaults;
        if let Some(x) = map.get("voice")
            && let Some(voice) = self.string(x, "publish.voice.voice")
        {
            if voice.trim().is_empty() {
                self.issue(
                    "publish.voice.voice",
                    "must not be blank — name one of the speech model's voices, or remove it to \
                     use its default for the visitor's language",
                );
            } else if output && self.stage == Stage::Publish {
                let model = map
                    .get("speech_model")
                    .and_then(Value::as_str)
                    .or(defaults.speech.as_deref());
                if let Some(model) = model {
                    self.offered_voice(voice, model);
                }
            }
        }
        for (key, wanted, fallback, (direction, kind)) in [
            (
                "transcription_model",
                input,
                defaults.transcription.as_deref(),
                ("voice input", "speech-recognition"),
            ),
            (
                "speech_model",
                output,
                defaults.speech.as_deref(),
                ("voice output", "speech-output"),
            ),
        ] {
            let path = join("publish.voice", key);
            if wanted {
                self.model_or_default(
                    map.get(key),
                    &path,
                    fallback,
                    (direction, kind),
                    &format!(
                        "in the setup's \"Website\" step choose its model (that grants it), or \
                         switch {direction} off"
                    ),
                );
            } else if let Some(x) = map.get(key)
                && let Some(model) = self.string(x, &path)
            {
                self.require_grant(&path, GrantKind::Model, model, "model");
            }
        }
    }

    /// `publish.voice.voice` names one of the voices speech model `model`
    /// offers, the ones the setup's voice picker lists.
    fn offered_voice(&mut self, voice: &str, model: &str) {
        let offered = self
            .ctx
            .speech_voices
            .get(model)
            .map_or(&[][..], Vec::as_slice);
        if offered.iter().any(|v| v == voice) {
            return;
        }
        let message = if offered.is_empty() {
            format!(
                "`{voice}` cannot be checked: the speech model `{model}` offers no voices to \
                 choose from — remove the voice to use the model's default, or ask an admin to \
                 list its voices on the speech pool"
            )
        } else {
            format!(
                "`{voice}` is not a voice of the speech model `{model}` — pick one of {}, or \
                 remove it to use the model's default for the visitor's language",
                offered.join(", ")
            )
        };
        self.issue("publish.voice.voice", message);
    }

    /// A model key (`main.model`, a voice direction's): the model it names
    /// must be granted; unset, it runs on the gateway's default model of its
    /// `kind`, which on publish must exist and be granted. `what` is what
    /// runs on it, `fix` how the owner makes it run.
    fn model_or_default(
        &mut self,
        named: Option<&Value>,
        path: &str,
        default: Option<&str>,
        (what, kind): (&str, &str),
        fix: &str,
    ) {
        if let Some(x) = named {
            if let Some(model) = self.string(x, path) {
                self.require_grant(path, GrantKind::Model, model, "model");
            }
            return;
        }
        if self.stage != Stage::Publish {
            return;
        }
        match default {
            None => self.issue(
                path,
                format!(
                    "{what} names no model and the gateway has no default {kind} model to run \
                     {what} on — {fix}"
                ),
            ),
            Some(model) if !self.ctx.grants.has(GrantKind::Model, model) => {
                let hint = self.grant_hint(GrantKind::Model.as_str(), model);
                self.issue(
                    path,
                    format!(
                        "{what} names no model, so it runs on the gateway's default {kind} \
                         model `{model}`, which is not granted to this agent — {fix}, or {hint}"
                    ),
                );
            }
            Some(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SELF: &str = "self-id";
    const BILLING: &str = "billing-id";
    const UNPUBLISHED: &str = "draft-id";

    fn grants() -> GrantSet {
        GrantSet::new([
            (GrantKind::Model, "chat".to_string()),
            (GrantKind::Model, "small".to_string()),
            (GrantKind::Tool, "rag_search".to_string()),
            (GrantKind::Connector, "erp".to_string()),
            (GrantKind::Skill, "brand".to_string()),
        ])
    }

    fn agents() -> HashMap<String, bool> {
        HashMap::from([
            (SELF.to_string(), false),
            (BILLING.to_string(), true),
            (UNPUBLISHED.to_string(), false),
        ])
    }

    fn check(spec: Value, stage: Stage) -> Vec<SpecIssue> {
        let grants = grants();
        let agents = agents();
        validate(
            &spec,
            &SpecContext {
                agent_id: SELF,
                grants: &grants,
                agents: &agents,
                live_specs: &HashMap::new(),
                model_defaults: &Default::default(),
                speech_voices: &HashMap::new(),
                allow_private: false,
            },
            stage,
        )
    }

    fn paths(issues: &[SpecIssue]) -> Vec<&str> {
        issues.iter().map(|i| i.path.as_str()).collect()
    }

    /// The layout from docs/agent-spec.md → "Layout", in JSON, with the sub-agent
    /// referenced by id.
    fn full() -> Value {
        json!({
            "profile": { "display": "croit Support", "avatar": null, "color": null },
            "scope": {
                "topics": ["croit products", "Ceph storage"],
                "refusal": "I can only help with croit products and Ceph storage.",
                "strict": true,
                "classifier_model": "small"
            },
            "main": {
                "model": "chat",
                "instructions": {
                    "orchestration": "Collect name, email and issue before forwarding.",
                    "response": "Friendly, short, in the visitor's language."
                },
                "tools": ["rag_search", "mcp__erp__lookup"],
                "skills": ["brand"],
                "tool_resources": {
                    "rag_search": { "bind": { "collection": { "const": "produktdoku" } } },
                    "mcp__erp__lookup": {
                        "bind": { "customer_id": "state.verified.customer_id" },
                        "permission": "always_ask"
                    }
                },
                "budget": { "rounds": 12, "seconds": 60, "tokens": 40000 }
            },
            "state": {
                "name": { "type": "string", "max_length": 120, "set_by": ["llm"] },
                "email": { "type": "email", "set_by": ["llm"] },
                "issue": { "type": "enum", "values": ["billing", "technical"], "set_by": ["llm"] },
                "verified": { "type": "subject", "set_by": ["verifier:otp", "host"] },
                "issue_summary": { "type": "string", "max_length": 2000, "set_by": ["llm"] }
            },
            "verifiers": {
                "otp": { "kind": "mcp_code", "connector": "erp", "send_tool": "send_code",
                         "check_tool": "check_code", "input": "secure_field",
                         "email_slot": "email", "writes": { "verified": "result" },
                         "max_attempts": 5, "code_ttl": "10m" }
            },
            "router": { "kind": "classifier", "model": "small" },
            "routes": {
                "billing": {
                    "when": { "all": [
                        { "slot": "issue", "eq": "billing" },
                        { "slot": "verified", "provenance": "verifier:otp", "max_age": "15m" }
                    ] },
                    "agent": BILLING,
                    "task": "Invoice question from {verified.customer_id}: {issue_summary}",
                    "bind": { "customer_id": "state.verified.customer_id" }
                },
                "human": {
                    "when": { "not": { "slot": "issue", "set": false } },
                    "human": { "notify": ["push"], "inbox": "support" }
                }
            },
            "finish": { "schema": { "type": "object", "required": ["answer"],
                "properties": { "answer": { "type": "string" } } } },
            "publish": {
                "origins": ["https://www.example.com", "http://localhost:5173"],
                "idle_ttl": "30m",
                "retention_days": 30,
                "output_filter": { "patterns": { "invoice": "RE-\\d{6}" } }
            }
        })
    }

    #[test]
    fn the_documented_layout_is_valid_for_publishing() {
        assert_eq!(check(full(), Stage::Publish), []);
    }

    #[test]
    fn a_spec_that_passes_comes_back_typed_with_its_defaults_applied() {
        let (grants, agents, live) = (grants(), agents(), HashMap::new());
        let ctx = SpecContext {
            agent_id: SELF,
            grants: &grants,
            agents: &agents,
            live_specs: &live,
            model_defaults: &Default::default(),
            speech_voices: &HashMap::new(),
            allow_private: false,
        };
        let typed = super::check(&full(), &ctx, Stage::Publish).expect("the layout is valid");
        assert_eq!(typed.main_model(), Some("chat"));
        let scope = typed.scope.as_ref().expect("the layout declares a scope");
        assert!(scope.strict);
        assert_eq!(scope.topics, ["croit products", "Ceph storage"]);
        assert_eq!(scope.classifier_model.as_deref(), Some("small"));
        assert_eq!(typed.main.budget.budget().rounds(), 12);
        assert!(matches!(
            &typed.routes["billing"].target,
            model::RouteTarget::Agent(id) if id == BILLING
        ));
        assert_eq!(
            typed.publish.idle_ttl(),
            jiff::SignedDuration::from_mins(30)
        );
        assert_eq!(
            typed.publish.visitor_rates().visitor,
            crate::agents::embed::DEFAULT_VISITOR_RATE,
            "an unset rate takes its default from the type"
        );
        let issues = super::check(&json!({ "mian": {} }), &ctx, Stage::Draft).unwrap_err();
        assert_eq!(issues, check(json!({ "mian": {} }), Stage::Draft));
    }

    #[test]
    fn a_strict_scope_needs_topics_and_a_refusal_even_in_a_draft() {
        let issues = check(json!({ "scope": { "strict": true } }), Stage::Draft);
        assert_eq!(paths(&issues), ["scope.topics", "scope.refusal"]);
        assert!(issues[0].message.contains("list the topics"), "{issues:?}");
        assert!(issues[1].message.contains("reply"), "{issues:?}");
        assert_eq!(
            paths(&check(
                json!({ "scope": { "strict": true, "topics": [], "refusal": "  " } }),
                Stage::Draft
            )),
            ["scope.topics", "scope.refusal"]
        );
        assert_eq!(
            check(json!({ "scope": { "topics": ["Ceph"] } }), Stage::Publish)
                .iter()
                .filter(|i| i.path.starts_with("scope"))
                .count(),
            0,
            "a scope that is not strict is guidance and needs no refusal"
        );
    }

    #[test]
    fn a_scope_is_checked_for_its_shape_and_its_classifier_model_grant() {
        let issues = check(
            json!({ "scope": {
                "topic": [],
                "topics": ["Ceph", "Ceph", " "],
                "refusal": 3,
                "strict": "yes",
                "classifier_model": "gpu-big"
            } }),
            Stage::Draft,
        );
        assert_eq!(
            paths(&issues),
            [
                "scope.topic",
                "scope.topics[1]",
                "scope.topics[2]",
                "scope.refusal",
                "scope.strict",
                "scope.classifier_model"
            ]
        );
        assert!(issues[5].message.contains("not granted"), "{issues:?}");
    }

    #[test]
    fn an_empty_draft_is_valid_but_cannot_be_published() {
        assert_eq!(check(json!({}), Stage::Draft), []);
        assert_eq!(check(json!({ "main": {} }), Stage::Draft), []);
        assert_eq!(paths(&check(json!({}), Stage::Publish)), ["main"]);
        assert_eq!(
            paths(&check(json!({ "main": {} }), Stage::Publish)),
            ["main.model", "main.instructions"]
        );
    }

    #[test]
    fn unknown_keys_are_reported_at_every_level_with_the_expected_ones() {
        let issues = check(
            json!({
                "mian": {},
                "main": { "tool_resource": {} },
                "state": { "x": { "type": "string", "set_by": ["llm"], "maxlen": 3 } },
                "routes": { "r": { "when": { "slot": "x", "set": true, "equals": 1 },
                                   "agent": BILLING, "task": "t" } }
            }),
            Stage::Draft,
        );
        assert_eq!(
            paths(&issues),
            [
                "mian",
                "state.x.maxlen",
                "main.tool_resource",
                "routes.r.when.equals"
            ]
        );
        assert!(
            issues[0].message.contains("expected one of: profile, main"),
            "{issues:?}"
        );
    }

    #[test]
    fn an_ungranted_model_tool_skill_or_connector_is_rejected_with_the_grant_to_make() {
        let issues = check(
            json!({ "main": {
                "model": "gpu-big",
                "tools": ["send_email", "mcp__crm__find", "rag_search"],
                "skills": ["legal"]
            } }),
            Stage::Draft,
        );
        assert_eq!(
            paths(&issues),
            [
                "main.model",
                "main.tools[0]",
                "main.tools[1]",
                "main.skills[0]"
            ]
        );
        assert!(
            issues[1].message.contains(
                "POST /api/v0/system-principals/self-id/grants {\"kind\": \"tool\", \"ref\": \"send_email\"}"
            ),
            "{}",
            issues[1].message
        );
        assert!(
            issues[2].message.contains("connector `crm`"),
            "{}",
            issues[2].message
        );
    }

    #[test]
    fn a_tool_resource_must_belong_to_a_listed_tool() {
        let issues = check(
            json!({ "main": { "tools": [], "tool_resources": {
                "rag_search": { "permission": "sometimes" } } } }),
            Stage::Draft,
        );
        assert_eq!(
            paths(&issues),
            [
                "main.tool_resources.rag_search",
                "main.tool_resources.rag_search.permission"
            ]
        );
    }

    #[test]
    fn an_approval_timeout_and_a_human_routes_settings_are_checked() {
        let issues = check(
            json!({
                "main": { "tools": ["rag_search"], "tool_resources": {
                    "rag_search": { "permission": "always_ask", "approval_timeout": "soon" } } },
                "state": { "issue": { "type": "string", "set_by": ["llm"] } },
                "routes": { "people": {
                    "when": { "slot": "issue", "set": true },
                    "human": { "notify": ["push", "pager"], "timeout": "1x", "transcript": "yes" }
                } }
            }),
            Stage::Draft,
        );
        let found = paths(&issues);
        for expected in [
            "main.tool_resources.rag_search.approval_timeout",
            "routes.people.human.notify[1]",
            "routes.people.human.timeout",
            "routes.people.human.transcript",
        ] {
            assert!(found.contains(&expected), "{expected} in {found:?}");
        }
        let fine = check(
            json!({
                "main": { "tools": ["rag_search"], "tool_resources": {
                    "rag_search": { "permission": "always_ask", "approval_timeout": "2h" } } },
                "state": { "issue": { "type": "string", "set_by": ["llm"] } },
                "routes": { "people": {
                    "when": { "slot": "issue", "set": true },
                    "human": { "notify": ["push", "slack"], "timeout": "30m", "transcript": true }
                } }
            }),
            Stage::Draft,
        );
        assert!(
            !paths(&fine)
                .iter()
                .any(|p| p.contains("approval_timeout") || p.contains("human")),
            "{fine:?}"
        );
    }

    #[test]
    fn a_bind_source_must_be_a_slot_the_model_cannot_write_or_a_const() {
        let spec = |bind: Value| {
            json!({
                "state": {
                    "name": { "type": "string", "set_by": ["llm"] },
                    "verified": { "type": "subject", "set_by": ["host"] }
                },
                "main": { "tools": ["rag_search"],
                          "tool_resources": { "rag_search": { "bind": bind } } }
            })
        };
        assert_eq!(
            check(
                spec(json!({ "a": "state.verified.customer_id" })),
                Stage::Draft
            ),
            []
        );
        assert_eq!(
            check(spec(json!({ "a": { "const": 7 } })), Stage::Draft),
            []
        );

        assert_eq!(
            check(spec(json!({ "a": "route.customer_id" })), Stage::Draft),
            [],
            "a tool may take a value its dispatching route passes"
        );

        let bare = check(spec(json!({ "a": "verified.customer_id" })), Stage::Draft);
        assert_eq!(paths(&bare), ["main.tool_resources.rag_search.bind.a"]);
        assert!(
            bare[0].message.contains("`state.<slot>`"),
            "{}",
            bare[0].message
        );

        let unknown = check(spec(json!({ "a": "state.customer" })), Stage::Draft);
        assert_eq!(paths(&unknown), ["main.tool_resources.rag_search.bind.a"]);
        assert!(unknown[0].message.contains("not a slot in `state`"));

        let llm = check(spec(json!({ "a": "state.name" })), Stage::Draft);
        assert!(
            llm[0].message.contains("the model can set"),
            "{}",
            llm[0].message
        );

        let shape = check(
            spec(json!({ "a": 3, "b": { "const": 1, "x": 2 } })),
            Stage::Draft,
        );
        assert_eq!(shape.len(), 2);
    }

    #[test]
    fn sub_agents_are_referenced_by_an_existing_agent_id_and_must_be_live_to_publish() {
        let route = |agent: &str| {
            let mut spec = full();
            spec["routes"]["billing"]["agent"] = json!(agent);
            spec
        };
        let missing = check(route("billing-subagent"), Stage::Draft);
        assert_eq!(paths(&missing), ["routes.billing.agent"]);
        assert!(missing[0].message.contains("no agent with id"));

        assert_eq!(check(route(UNPUBLISHED), Stage::Draft), []);
        let unpublished = check(route(UNPUBLISHED), Stage::Publish);
        assert!(unpublished[0].message.contains("never been published"));

        let itself = check(route(SELF), Stage::Draft);
        assert!(itself[0].message.contains("itself"));
    }

    #[test]
    fn a_route_needs_one_target_a_gate_and_for_a_sub_agent_a_task() {
        let issues = check(
            json!({ "routes": {
                "a": { "agent": BILLING },
                "b": { "when": { "all": [] } },
                "c": { "when": { "slot": "x" }, "agent": BILLING, "human": {}, "task": "t" },
                "Bad Name": {}
            } }),
            Stage::Draft,
        );
        assert_eq!(
            paths(&issues),
            [
                "routes.Bad Name",
                "routes.a.when",
                "routes.a.task",
                "routes.b.when.all",
                "routes.b",
                "routes.c.when.slot",
                "routes.c.when",
                "routes.c"
            ]
        );
    }

    #[test]
    fn gate_leaves_and_templates_must_name_declared_slots_and_verifiers() {
        let issues = check(
            json!({
                "state": { "issue": { "type": "string", "set_by": ["llm", "verifier:sms"] } },
                "routes": { "r": {
                    "when": { "any": [
                        { "slot": "issue", "provenance": "verifier:otp", "max_age": "soon" },
                        { "slot": "ghost", "in": "x" }
                    ] },
                    "agent": BILLING,
                    "task": "About {issue} for {customer.id} {"
                } }
            }),
            Stage::Draft,
        );
        assert_eq!(
            paths(&issues),
            [
                "state.issue.set_by[1]",
                "routes.r.when.any[0].provenance",
                "routes.r.when.any[0].max_age",
                "routes.r.when.any[1].slot",
                "routes.r.when.any[1].in",
                "routes.r.task",
                "routes.r.task"
            ]
        );
    }

    /// A spec with every kind of verifier and the slots they need.
    fn verified(verifiers: Value) -> Value {
        json!({
            "state": {
                "email": { "type": "email", "set_by": ["llm"] },
                "name": { "type": "string", "set_by": ["llm"] },
                "verified": { "type": "subject",
                              "set_by": ["verifier:otp", "verifier:kyc", "host"] },
                "plan": { "type": "string", "set_by": ["host"] }
            },
            "verifiers": verifiers
        })
    }

    const PEM_GARBAGE: &str = "-----BEGIN PUBLIC KEY-----\nnot a key\n-----END PUBLIC KEY-----";

    #[test]
    fn complete_verifiers_of_every_kind_are_valid_for_publishing() {
        let spec = verified(json!({
            "otp": { "kind": "mcp_code", "connector": "erp", "email_slot": "email",
                     "writes": { "verified": "result.customer" },
                     "send_limits": { "email": { "max": 3, "per": "1h" } } },
            "kyc": { "kind": "lookup", "tool": "mcp__erp__find", "assurance": "low",
                     "inputs": { "name": "state.name", "region": { "const": "eu" } },
                     "writes": { "verified": "input.name" } },
            "site": { "kind": "host_jwt", "algorithm": "HS256",
                      "secret": "0123456789abcdef0123456789abcdef",
                      "issuer": "https://www.example.com", "audience": "support",
                      "max_lifetime": "5m",
                      "claims": { "verified": { "customer_id": "sub" }, "plan": "plan" } }
        }));
        assert_eq!(on_verifiers(check(spec, Stage::Publish)), []);
    }

    /// The issues under `verifiers`: publishing also needs `main`, and a
    /// spec declaring only some verifiers leaves `set_by` entries dangling.
    fn on_verifiers(issues: Vec<SpecIssue>) -> Vec<SpecIssue> {
        issues
            .into_iter()
            .filter(|i| i.path.starts_with("verifiers."))
            .collect()
    }

    #[test]
    fn verifier_settings_are_checked_on_save_with_what_to_change() {
        let issues = check(
            verified(json!({
                "otp": { "kind": "mcp_code", "connector": "crm", "input": "sms",
                         "max_attempts": 50, "code_ttl": "2h",
                         "send_limits": { "email": { "max": 3 }, "ip": { "max": 1, "per": "2d" },
                                          "fax": {} } },
                "kyc": { "kind": "lookup", "tool": "lookup_customer", "assurance": "Very Low" },
                "site": { "kind": "host_jwt", "algorithm": "none" },
                "sms": { "kind": "sms" }
            })),
            Stage::Draft,
        );
        assert_eq!(
            paths(&issues),
            [
                "verifiers.kyc.assurance",
                "verifiers.kyc.tool",
                "verifiers.otp.connector",
                "verifiers.otp.input",
                "verifiers.otp.max_attempts",
                "verifiers.otp.code_ttl",
                "verifiers.otp.send_limits.fax",
                "verifiers.otp.send_limits.email.per",
                "verifiers.otp.send_limits.ip.per",
                "verifiers.site.algorithm",
                "verifiers.sms.kind"
            ]
        );
        let grant = issues
            .iter()
            .find(|i| i.path == "verifiers.otp.connector")
            .unwrap();
        assert!(
            grant
                .message
                .contains("\"kind\": \"connector\", \"ref\": \"crm\""),
            "{}",
            grant.message
        );
        let ttl = issues
            .iter()
            .find(|i| i.path == "verifiers.otp.code_ttl")
            .unwrap();
        assert!(ttl.message.contains("at most 1h"), "{}", ttl.message);
    }

    #[test]
    fn host_jwt_keys_are_checked_for_the_algorithm_they_serve() {
        let host = |cfg: Value| {
            let mut v = json!({ "kind": "host_jwt", "issuer": "i", "audience": "a",
                                "claims": { "plan": "plan" } });
            v.as_object_mut()
                .unwrap()
                .extend(cfg.as_object().unwrap().clone());
            on_verifiers(check(verified(json!({ "site": v })), Stage::Publish))
        };
        let short = host(json!({ "algorithm": "HS256", "secret": "short" }));
        assert_eq!(paths(&short), ["verifiers.site.secret"]);
        assert!(
            short[0].message.contains("at least 32"),
            "{}",
            short[0].message
        );

        let mixed = host(json!({ "algorithm": "HS256", "secret_sealed": "x",
                                 "jwks_url": "https://www.example.com/jwks" }));
        assert_eq!(paths(&mixed), ["verifiers.site.algorithm"]);

        let keyless = host(json!({ "algorithm": "RS256" }));
        assert_eq!(paths(&keyless), ["verifiers.site.public_key"]);

        let bad = host(json!({ "algorithm": "ES256", "public_key": PEM_GARBAGE }));
        assert_eq!(paths(&bad), ["verifiers.site.public_key"]);
        assert!(
            bad[0].message.contains("BEGIN PUBLIC KEY"),
            "{}",
            bad[0].message
        );

        let plain = host(json!({ "algorithm": "RS256",
                                 "jwks_url": "http://www.example.com/jwks" }));
        assert_eq!(paths(&plain), ["verifiers.site.jwks_url"]);

        let secretful = host(json!({ "algorithm": "RS256", "secret_sealed": "x" }));
        assert_eq!(paths(&secretful), ["verifiers.site.algorithm"]);
    }

    #[test]
    fn verifiers_must_read_and_write_slots_that_allow_them() {
        let issues = check(
            json!({
                "state": {
                    "name": { "type": "string", "set_by": ["llm"] },
                    "verified": { "type": "subject", "set_by": ["verifier:kyc"] },
                    "plan": { "type": "string", "set_by": ["verifier:site"] }
                },
                "verifiers": {
                    "otp": { "kind": "mcp_code", "connector": "erp", "email_slot": "name",
                             "writes": { "verified": "input.code", "ghost": "result",
                                         "name": "whatever" } },
                    "kyc": { "kind": "lookup", "tool": "rag_search", "assurance": "low",
                             "inputs": { "who": "state.nobody", "n": 3 },
                             "writes": { "verified": "input.who" } },
                    "site": { "kind": "host_jwt", "claims": { "name": { "id": "sub" } } },
                    "other": { "kind": "host_jwt" }
                }
            }),
            Stage::Draft,
        );
        assert_eq!(
            paths(&issues),
            [
                "verifiers.site.kind",
                "state.plan.set_by[0]",
                "verifiers.kyc.inputs.n",
                "verifiers.kyc.inputs.who",
                "verifiers.otp.email_slot",
                "verifiers.otp.writes.ghost",
                "verifiers.otp.writes.name",
                "verifiers.otp.writes.name",
                "verifiers.otp.writes.verified",
                "verifiers.otp.writes.verified",
                "verifiers.site.claims.name",
                "verifiers.site.claims.name"
            ]
        );
        let host_writer = &issues[1];
        assert!(
            host_writer.message.contains("list `host`"),
            "{}",
            host_writer.message
        );
        let code = issues
            .iter()
            .filter(|i| i.path == "verifiers.otp.writes.verified")
            .map(|i| i.message.as_str())
            .collect::<Vec<_>>();
        assert!(
            code.iter().any(|m| m.contains("add `verifier:otp`")),
            "{code:?}"
        );
        assert!(code.iter().any(|m| m.contains("`input.email`")), "{code:?}");
    }

    #[test]
    fn a_published_verifier_must_say_everything_it_needs_to_run() {
        let issues = on_verifiers(check(
            verified(json!({
                "otp": { "kind": "mcp_code" },
                "kyc": { "kind": "lookup" },
                "site": { "kind": "host_jwt" }
            })),
            Stage::Publish,
        ));
        assert_eq!(
            paths(&issues),
            [
                "verifiers.kyc.tool",
                "verifiers.kyc.inputs",
                "verifiers.kyc.writes",
                "verifiers.kyc.assurance",
                "verifiers.otp.connector",
                "verifiers.otp.email_slot",
                "verifiers.otp.writes",
                "verifiers.site.algorithm",
                "verifiers.site.issuer",
                "verifiers.site.audience",
                "verifiers.site.claims"
            ]
        );
        assert_eq!(
            on_verifiers(check(
                verified(json!({ "otp": { "kind": "mcp_code" } })),
                Stage::Draft
            )),
            [],
            "a draft may be incomplete"
        );
    }

    #[test]
    fn slot_definitions_are_checked_for_type_writers_and_values() {
        let issues = check(
            json!({ "state": {
                "a": { "set_by": ["llm"] },
                "b": { "type": "enum", "set_by": ["llm"] },
                "c": { "type": "string", "set_by": [], "values": ["x"], "pattern": "(" },
                "d": { "type": "date", "set_by": ["robot"] },
                "e": { "type": "string", "set_by": ["llm"], "order": -1 },
                "f": { "type": "string", "set_by": ["llm"], "order": 3 }
            } }),
            Stage::Draft,
        );
        assert_eq!(
            paths(&issues),
            [
                "state.a.type",
                "state.b.values",
                "state.c.set_by",
                "state.c.values",
                "state.c.pattern",
                "state.d.type",
                "state.d.set_by[0]",
                "state.e.order"
            ]
        );
    }

    fn gated(when: Value, bind: Option<Value>) -> Value {
        let mut route = json!({ "when": when, "agent": BILLING, "task": "t" });
        if let Some(bind) = bind {
            route["bind"] = bind;
        }
        json!({
            "verifiers": { "otp": { "kind": "mcp_code" } },
            "state": {
                "issue": { "type": "enum", "values": ["billing", "technical"], "set_by": ["llm"] },
                "seats": { "type": "integer", "set_by": ["llm"] },
                "verified": { "type": "subject", "set_by": ["verifier:otp"] }
            },
            "routes": { "r": route }
        })
    }

    #[test]
    fn gates_are_type_checked_against_the_slots_they_name() {
        let issues = check(
            gated(
                json!({ "all": [
                    { "slot": "issue", "eq": "sales" },
                    { "slot": "seats", "in": [1, "two"] },
                    { "slot": "issue", "provenance": "host" },
                    { "slot": "verified", "provenance": "verifier:otp", "max_age": "15m" },
                    { "not": { "slot": "issue", "eq": "billing" } }
                ] }),
                None,
            ),
            Stage::Draft,
        );
        assert_eq!(
            paths(&issues),
            [
                "routes.r.when.all[0].eq",
                "routes.r.when.all[1].in[1]",
                "routes.r.when.all[2].provenance"
            ]
        );
        assert!(
            issues[0].message.contains("can never hold"),
            "{}",
            issues[0].message
        );
    }

    #[test]
    fn a_route_that_binds_from_state_must_require_a_trusted_provenance() {
        let bind = Some(json!({ "customer_id": "state.verified.customer_id" }));
        let trusted = json!({ "slot": "verified", "provenance": "verifier:otp" });
        let llm_only = json!({ "slot": "issue", "eq": "billing" });

        assert_eq!(
            check(
                gated(
                    json!({ "all": [llm_only.clone(), trusted.clone()] }),
                    bind.clone()
                ),
                Stage::Draft
            ),
            []
        );
        for weak in [
            llm_only.clone(),
            json!({ "any": [llm_only.clone(), trusted.clone()] }),
            json!({ "not": { "slot": "verified", "set": false } }),
            json!({ "slot": "issue", "provenance": "llm" }),
        ] {
            let issues = check(gated(weak.clone(), bind.clone()), Stage::Draft);
            assert_eq!(paths(&issues), ["routes.r.when"], "{weak}");
            assert!(
                issues[0].message.contains("verifier or the host"),
                "{}",
                issues[0].message
            );
        }
        assert_eq!(check(gated(llm_only, None), Stage::Draft), []);
    }

    #[test]
    fn a_route_passes_values_from_its_callers_state_never_from_another_route() {
        let issues = check(
            gated(
                json!({ "slot": "verified", "provenance": "verifier:otp" }),
                Some(json!({ "customer_id": "route.customer_id" })),
            ),
            Stage::Draft,
        );
        assert_eq!(paths(&issues), ["routes.r.bind.customer_id"]);
        assert!(
            issues[0].message.contains("`state.<slot>`"),
            "{}",
            issues[0].message
        );
    }

    fn invoices_agent(bind: Value) -> Value {
        json!({ "main": {
            "tools": ["mcp__erp__invoices", "mcp__erp__tickets"],
            "tool_resources": { "mcp__erp__invoices": { "bind": bind } }
        }, "finish": { "schema": { "type": "object" } } })
    }

    fn subject_route(bind: Value) -> Value {
        let mut spec = gated(
            json!({ "slot": "verified", "provenance": "verifier:otp" }),
            Some(bind),
        );
        spec["routes"]["r"]["agent"] = json!(BILLING);
        spec
    }

    #[test]
    fn a_route_provides_exactly_the_values_its_sub_agent_binds() {
        let billing = invoices_agent(json!({ "customer_id": "route.customer_id" }));
        assert_eq!(
            check_live(
                subject_route(json!({ "customer_id": "state.verified.customer_id" })),
                &[(BILLING, billing.clone())]
            ),
            []
        );

        let missing = check_live(subject_route(json!({})), &[(BILLING, billing.clone())]);
        assert_eq!(paths(&missing), ["routes.r.bind"]);
        assert!(
            missing[0].message.contains("`route.customer_id`"),
            "{}",
            missing[0].message
        );

        let unused = check_live(
            subject_route(json!({
                "customer_id": "state.verified.customer_id",
                "tier": "state.verified.tier"
            })),
            &[(BILLING, billing)],
        );
        assert_eq!(paths(&unused), ["routes.r.bind.tier"]);
        assert!(
            unused[0].message.contains("never binds"),
            "{}",
            unused[0].message
        );
    }

    #[test]
    fn a_routed_sub_agent_must_declare_its_finish_schema_to_be_published() {
        let mut no_finish = invoices_agent(json!({ "customer_id": "route.customer_id" }));
        no_finish.as_object_mut().unwrap().remove("finish");
        let route = subject_route(json!({ "customer_id": "state.verified.customer_id" }));
        assert_eq!(
            check_live(route.clone(), &[(BILLING, no_finish.clone())]),
            []
        );
        let issues = check_live_at(route, &[(BILLING, no_finish)], Stage::Publish);
        assert!(
            issues
                .iter()
                .any(|i| i.path == "routes.r.agent" && i.message.contains("`finish`")),
            "{issues:?}"
        );
    }

    #[test]
    fn every_slot_a_route_binds_from_must_itself_be_gated_on_a_trusted_provenance() {
        let mut spec = gated(
            json!({ "slot": "verified", "provenance": "verifier:otp" }),
            Some(
                json!({ "customer_id": "state.verified.customer_id", "account": "state.account" }),
            ),
        );
        spec["state"]["account"] = json!({ "type": "subject", "set_by": ["host"] });
        let issues = check(spec.clone(), Stage::Draft);
        assert_eq!(paths(&issues), ["routes.r.when"]);
        assert!(
            issues[0].message.contains("`account`"),
            "{}",
            issues[0].message
        );

        spec["routes"]["r"]["when"] = json!({ "all": [
            { "slot": "verified", "provenance": "verifier:otp" },
            { "slot": "account", "provenance": "host" }
        ] });
        assert_eq!(check(spec, Stage::Draft), []);
    }

    fn check_live(spec: Value, live: &[(&str, Value)]) -> Vec<SpecIssue> {
        check_live_at(spec, live, Stage::Draft)
    }

    fn check_live_at(spec: Value, live: &[(&str, Value)], stage: Stage) -> Vec<SpecIssue> {
        let grants = grants();
        let mut agents = agents();
        for (id, _) in live {
            agents.insert(id.to_string(), true);
        }
        let live_specs: HashMap<String, Value> = live
            .iter()
            .map(|(id, spec)| (id.to_string(), spec.clone()))
            .collect();
        validate(
            &spec,
            &SpecContext {
                agent_id: SELF,
                grants: &grants,
                agents: &agents,
                live_specs: &live_specs,
                model_defaults: &Default::default(),
                speech_voices: &HashMap::new(),
                allow_private: false,
            },
            stage,
        )
    }

    fn routes_to(agent: &str) -> Value {
        json!({ "routes": { "r": {
            "when": { "not": { "slot": "issue", "set": true } },
            "agent": agent, "task": "t"
        } }, "state": {
            "issue": { "type": "string", "set_by": ["llm"] }
        } })
    }

    #[test]
    fn a_route_to_a_sub_agent_that_binds_a_tool_needs_a_trusted_gate_too() {
        let binding_billing = json!({ "main": {
            "tools": ["mcp__erp__invoices"],
            "tool_resources": { "mcp__erp__invoices": { "bind": { "tenant": { "const": "t1" } } } }
        } });
        let issues = check_live(routes_to(BILLING), &[(BILLING, binding_billing)]);
        assert_eq!(paths(&issues), ["routes.r.when"]);
        assert!(
            issues[0]
                .message
                .contains("reaches a tool with bound arguments"),
            "{}",
            issues[0].message
        );
        let plain_billing = json!({ "main": { "tools": ["rag_search"] } });
        assert_eq!(
            check_live(routes_to(BILLING), &[(BILLING, plain_billing)]),
            []
        );
    }

    #[test]
    fn the_sub_agent_graph_must_be_acyclic() {
        let back = check_live(routes_to(BILLING), &[(BILLING, routes_to(SELF))]);
        assert_eq!(paths(&back), ["routes.r.agent"]);
        assert!(
            back[0].message.contains("loops back"),
            "{}",
            back[0].message
        );

        let around = check_live(
            routes_to(BILLING),
            &[
                (BILLING, routes_to("refunds-id")),
                ("refunds-id", routes_to(SELF)),
            ],
        );
        assert_eq!(paths(&around), ["routes.r.agent"]);
        assert!(
            around[0]
                .message
                .contains("billing-id → refunds-id → self-id"),
            "{}",
            around[0].message
        );
    }

    #[test]
    fn the_sub_agent_graph_is_at_most_three_agents_deep() {
        let three = check_live(
            routes_to(BILLING),
            &[
                (BILLING, routes_to("refunds-id")),
                ("refunds-id", json!({})),
            ],
        );
        assert_eq!(three, []);
        let four = check_live(
            routes_to(BILLING),
            &[
                (BILLING, routes_to("refunds-id")),
                ("refunds-id", routes_to("ledger-id")),
                ("ledger-id", json!({})),
            ],
        );
        assert_eq!(paths(&four), ["routes.r.agent"]);
        assert!(
            four[0].message.contains("4 agents deep"),
            "{}",
            four[0].message
        );
    }

    #[test]
    fn a_rules_router_may_order_its_routes_by_name() {
        let mut spec = routes_to(BILLING);
        spec["router"] = json!({ "kind": "rules", "order": ["r"] });
        assert_eq!(check(spec.clone(), Stage::Draft), []);
        spec["router"] = json!({ "kind": "rules", "order": ["r", "nope", "r"] });
        assert_eq!(
            paths(&check(spec.clone(), Stage::Draft)),
            ["router.order[2]", "router.order[1]"]
        );
        spec["router"] = json!({ "kind": "classifier", "model": "small", "order": ["r"] });
        assert_eq!(paths(&check(spec, Stage::Draft)), ["router.order"]);
    }

    #[test]
    fn a_constraint_is_refused_on_a_slot_type_it_does_not_apply_to() {
        let issues = check(
            json!({ "state": {
                "age": { "type": "integer", "set_by": ["llm"], "max_length": 3, "minimum": 0 },
                "name": { "type": "string", "set_by": ["llm"], "maximum": 9, "pattern": "^A" },
                "ok": { "type": "boolean", "set_by": ["llm"], "pattern": "x" },
                "email": { "type": "email", "set_by": ["llm"], "max_length": 80 }
            } }),
            Stage::Draft,
        );
        assert_eq!(
            paths(&issues),
            [
                "state.age.max_length",
                "state.name.maximum",
                "state.ok.pattern"
            ]
        );
        assert!(
            issues[0]
                .message
                .contains("does not apply to a slot of type `integer`"),
            "{}",
            issues[0].message
        );
    }

    #[test]
    fn a_subject_slot_takes_a_schema_the_run_time_validator_can_enforce() {
        let spec = |schema: Value| {
            json!({ "state": { "verified": {
                "type": "subject", "set_by": ["host"], "schema": schema
            } } })
        };
        assert_eq!(
            check(
                spec(json!({ "type": "object", "required": ["customer_id"],
                             "properties": { "customer_id": { "type": "string" } } })),
                Stage::Draft
            ),
            []
        );
        let unsupported = check(spec(json!({ "type": "object", "oneOf": [] })), Stage::Draft);
        assert_eq!(paths(&unsupported), ["state.verified.schema"]);
        assert!(
            unsupported[0].message.contains("`oneOf`"),
            "{unsupported:?}"
        );

        let not_object = check(spec(json!({ "type": "string" })), Stage::Draft);
        assert_eq!(paths(&not_object), ["state.verified.schema"]);

        let on_string = check(
            json!({ "state": { "n": { "type": "string", "set_by": ["llm"],
                                      "schema": { "type": "object" } } } }),
            Stage::Draft,
        );
        assert_eq!(paths(&on_string), ["state.n.schema"]);
    }

    #[test]
    fn the_model_may_not_write_a_subject_slot() {
        let issues = check(
            json!({ "state": { "verified": { "type": "subject", "set_by": ["llm", "host"] } } }),
            Stage::Draft,
        );
        assert_eq!(paths(&issues), ["state.verified.set_by"]);
        assert!(
            issues[0].message.contains("whose data"),
            "{}",
            issues[0].message
        );
    }

    #[test]
    fn the_activity_log_must_outlive_the_conversations_it_records() {
        let audit = |publish: Value| check(json!({ "publish": publish }), Stage::Draft);
        let short = audit(json!({ "retention_days": 90, "audit_retention_days": 60 }));
        assert_eq!(paths(&short), ["publish.audit_retention_days"]);
        assert!(
            short[0].message.contains("at least 90"),
            "{}",
            short[0].message
        );
        assert_eq!(
            paths(&audit(json!({ "audit_retention_days": 20 }))),
            ["publish.audit_retention_days"],
            "against the 30-day default of conversations too"
        );
        assert!(audit(json!({ "retention_days": 90, "audit_retention_days": 90 })).is_empty());
        assert!(audit(json!({ "audit_retention_days": 3650 })).is_empty());
    }

    /// Nothing at run time read it, so it is not part of the spec: a spec
    /// naming it is told the key is unknown.
    #[test]
    fn on_tool_unavailable_is_not_a_spec_key() {
        let issues = check(json!({ "on_tool_unavailable": "reject" }), Stage::Draft);
        assert!(
            issues
                .iter()
                .any(|i| i.message.contains("unknown key `on_tool_unavailable`")),
            "{issues:?}"
        );
    }

    #[test]
    fn budgets_finish_schemas_and_publish_settings_are_checked() {
        let issues = check(
            json!({
                "main": { "budget": { "rounds": 0, "seconds": -1, "tokens": "many" } },
                "finish": { "schema": { "type": "object", "oneOf": [] } },
                "publish": {
                    "origins": ["https://example.com/", "example.com", "https://ok.example:8443"],
                    "idle_ttl": "30",
                    "retention_days": 0,
                    "audit_retention_days": 0,
                    "output_filter": { "patterns": { "bad": "[" }, "action": "shrug" }
                },
                "router": { "kind": "classifier" }
            }),
            Stage::Draft,
        );
        assert_eq!(
            paths(&issues),
            [
                "main.budget.rounds",
                "main.budget.seconds",
                "main.budget.tokens",
                "router.model",
                "finish.schema",
                "publish.origins[0]",
                "publish.origins[1]",
                "publish.idle_ttl",
                "publish.retention_days",
                "publish.audit_retention_days",
                "publish.output_filter.patterns.bad",
                "publish.output_filter.action"
            ]
        );
        let over = check(
            json!({ "main": { "budget": { "rounds": 65 } } }),
            Stage::Draft,
        );
        assert!(
            over[0].message.contains("between 1 and 64"),
            "{}",
            over[0].message
        );
    }

    #[test]
    fn the_profile_colour_is_a_hex_colour() {
        for ok in [json!("#2563eb"), json!(""), Value::Null] {
            assert!(check(json!({ "profile": { "color": ok } }), Stage::Draft).is_empty());
        }
        for bad in [json!("blue"), json!("#fff"), json!(3)] {
            let issues = check(json!({ "profile": { "color": bad } }), Stage::Draft);
            assert_eq!(paths(&issues), ["profile.color"], "{bad}");
        }
    }

    #[test]
    fn voice_settings_are_checked_and_their_models_must_be_granted() {
        let ok = check(
            json!({ "publish": { "voice": {
                "input": true, "output": true, "voice": "alloy",
                "speech_model": "chat", "transcription_model": "small"
            } } }),
            Stage::Draft,
        );
        assert!(ok.is_empty(), "{ok:?}");

        let issues = check(
            json!({ "publish": { "voice": {
                "input": "yes", "output": true, "voice": " ",
                "speech_model": "cloud-tts", "transcription_model": 3, "loud": true
            } } }),
            Stage::Draft,
        );
        assert_eq!(
            paths(&issues),
            [
                "publish.voice.loud",
                "publish.voice.input",
                "publish.voice.voice",
                "publish.voice.transcription_model",
                "publish.voice.speech_model"
            ]
        );
        assert!(
            issues[4].message.contains("not granted"),
            "{}",
            issues[4].message
        );
    }

    fn check_voice(voice: &str, offered: &[&str], stage: Stage) -> Vec<SpecIssue> {
        let mut spec = full();
        spec["publish"] =
            json!({ "voice": { "output": true, "speech_model": "chat", "voice": voice } });
        let voices = HashMap::from([(
            "chat".to_string(),
            offered.iter().map(|v| v.to_string()).collect::<Vec<_>>(),
        )]);
        let grants = grants();
        let agents = agents();
        validate(
            &spec,
            &SpecContext {
                agent_id: SELF,
                grants: &grants,
                agents: &agents,
                live_specs: &HashMap::new(),
                model_defaults: &Default::default(),
                speech_voices: &voices,
                allow_private: false,
            },
            stage,
        )
    }

    #[test]
    fn a_published_voice_is_one_its_speech_model_offers() {
        assert_eq!(check_voice("onyx", &["alloy", "onyx"], Stage::Publish), []);

        let unknown = check_voice("verse", &["alloy", "onyx"], Stage::Publish);
        assert_eq!(paths(&unknown), ["publish.voice.voice"]);
        assert!(
            unknown[0].message.contains("`verse`")
                && unknown[0].message.contains("`chat`")
                && unknown[0].message.contains("alloy, onyx"),
            "{}",
            unknown[0].message
        );

        let none = check_voice("alloy", &[], Stage::Publish);
        assert_eq!(paths(&none), ["publish.voice.voice"]);
        assert!(
            none[0].message.contains("offers no voices"),
            "{}",
            none[0].message
        );

        assert_eq!(
            check_voice("verse", &["alloy"], Stage::Draft),
            [],
            "a draft may hold a voice the model does not offer (yet)"
        );
    }

    fn check_with(spec: &Value, defaults: &ModelDefaults) -> Vec<SpecIssue> {
        let grants = grants();
        let agents = agents();
        validate(
            spec,
            &SpecContext {
                agent_id: SELF,
                grants: &grants,
                agents: &agents,
                live_specs: &HashMap::new(),
                model_defaults: defaults,
                speech_voices: &HashMap::new(),
                allow_private: false,
            },
            Stage::Publish,
        )
    }

    #[test]
    fn an_unset_model_runs_on_the_gateway_default_which_must_be_granted() {
        let mut spec = full();
        spec["main"].as_object_mut().unwrap().remove("model");
        assert!(check(spec.clone(), Stage::Draft).is_empty());

        let none = check_with(&spec, &ModelDefaults::default());
        assert_eq!(paths(&none), ["main.model"]);
        assert!(
            none[0].message.contains("no default chat model"),
            "{none:?}"
        );

        let ungranted = check_with(
            &spec,
            &ModelDefaults {
                chat: Some("gpu-big".into()),
                ..ModelDefaults::default()
            },
        );
        assert_eq!(paths(&ungranted), ["main.model"]);
        assert!(
            ungranted[0]
                .message
                .contains("default chat model `gpu-big`")
                && ungranted[0].message.contains("\"kind\": \"model\""),
            "{}",
            ungranted[0].message
        );

        let granted = ModelDefaults {
            chat: Some("chat".into()),
            ..ModelDefaults::default()
        };
        assert_eq!(check_with(&spec, &granted), []);
    }

    #[test]
    fn a_published_voice_direction_runs_on_a_granted_model() {
        let mut spec = full();
        spec["publish"]["voice"] = json!({ "input": true, "output": true });
        assert!(check(spec.clone(), Stage::Draft).is_empty());
        let issues = check(spec.clone(), Stage::Publish);
        assert_eq!(
            paths(&issues),
            [
                "publish.voice.transcription_model",
                "publish.voice.speech_model"
            ]
        );
        assert!(
            issues[0].message.contains("\"Website\" step"),
            "{}",
            issues[0].message
        );

        let defaults = ModelDefaults {
            chat: None,
            transcription: Some("small".into()),
            speech: Some("cloud-tts".into()),
        };
        let issues = check_with(&spec, &defaults);
        assert_eq!(paths(&issues), ["publish.voice.speech_model"]);
        assert!(issues[0].message.contains("`cloud-tts`"), "{issues:?}");

        spec["publish"]["voice"]["speech_model"] = json!("chat");
        assert_eq!(check_with(&spec, &defaults), []);

        let mut off = full();
        off["publish"]["voice"] = json!({ "input": false, "output": false });
        assert_eq!(check(off, Stage::Publish), []);
    }

    #[test]
    fn require_passing_tests_is_a_boolean() {
        let ok = check(
            json!({ "publish": { "require_passing_tests": true } }),
            Stage::Draft,
        );
        assert!(ok.is_empty(), "{ok:?}");
        let bad = check(
            json!({ "publish": { "require_passing_tests": "yes" } }),
            Stage::Draft,
        );
        assert_eq!(paths(&bad), ["publish.require_passing_tests"]);
    }

    #[test]
    fn visitor_rates_and_the_owner_budget_are_checked() {
        let ok = check(
            json!({ "publish": {
                "rate_limits": {
                    "visitor": { "max": 20, "per": "10m" },
                    "ip": { "max": 60, "per": "1h" }
                },
                "budget": { "monthly_cost": 25.5, "monthly_tokens": 2000000 }
            } }),
            Stage::Draft,
        );
        assert!(ok.is_empty(), "{ok:?}");

        let issues = check(
            json!({ "publish": {
                "rate_limits": {
                    "visitor": { "max": 0, "per": "10" },
                    "ip": { "per": "1m", "burst": 3 },
                    "token": {}
                },
                "budget": { "monthly_cost": -1, "monthly_tokens": 1.5, "daily_cost": 1 }
            } }),
            Stage::Draft,
        );
        assert_eq!(
            paths(&issues),
            [
                "publish.rate_limits.token",
                "publish.rate_limits.visitor.max",
                "publish.rate_limits.visitor.per",
                "publish.rate_limits.ip.burst",
                "publish.rate_limits.ip.max",
                "publish.budget.daily_cost",
                "publish.budget.monthly_cost",
                "publish.budget.monthly_tokens",
            ]
        );
    }

    #[test]
    fn a_spec_that_is_not_an_object_says_so() {
        let issues = check(json!([1, 2]), Stage::Draft);
        assert_eq!(paths(&issues), [""]);
        assert!(issues[0].message.contains("not an array"));
    }

    #[test]
    fn durations_and_origins_follow_their_documented_forms() {
        for ok in ["30s", "15m", "2h", "30d"] {
            assert!(is_duration(ok), "{ok}");
        }
        for bad in ["", "m", "0m", "15", "15 m", "1w", "-1m"] {
            assert!(!is_duration(bad), "{bad}");
        }
        for ok in ["https://a.example", "http://localhost:8080"] {
            assert!(is_origin(ok), "{ok}");
        }
        for bad in [
            "https://a.example/",
            "ftp://a",
            "https://",
            "https://a:0",
            "https://a b",
        ] {
            assert!(!is_origin(bad), "{bad}");
        }
    }
}
