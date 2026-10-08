// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The typed agent spec: what runtime code reads instead of walking the JSON
//! (`docs/agent-spec.md` → "The typed spec").
//!
//! [`super::check`] builds it once a spec passed the path-reporting walk, and
//! [`crate::agents::spec_cache::CompiledSpec`] holds it for every reader of a
//! published version. Every type rejects unknown keys like the walk does, so
//! a misspelt key can never deserialize into a silent default.
//!
//! **Defaults live here.** A field the spec may leave out is an `Option` (or
//! an empty collection), and the one accessor that reads it applies its
//! default: [`Publish::idle_ttl`], [`Publish::visitor_rates`],
//! [`Publish::retention_days`], [`RunBudget::budget`],
//! [`ToolResource::approval_timeout`], [`HumanSpec::timeout`],
//! [`LoopSpec::max_iterations`], [`A2aRouteSpec::seconds`],
//! [`McpCodeSpec`]'s accessors and [`HostJwtSpec::max_lifetime`]. The
//! constants they apply stay with the subsystem that documents them.
//!
//! Two parts keep their JSON: a route's `when`, which [`crate::agents::gate::Cond`]
//! compiles, and `finish.schema` (and a `subject` slot's `schema`), which is a
//! JSON schema [`crate::finish::FinishContract`] enforces.

use std::collections::BTreeMap;
use std::num::NonZeroU64;
use std::sync::LazyLock;
use std::time::Duration;

use aiplane_agents::rates::{Rate, VisitorRates};
use jiff::SignedDuration;
use serde::Deserialize;
use serde::de::{self, Deserializer};
use serde_json::Value;

use super::parse_duration;
use crate::agents::a2a_client::{self, AuthKind};
use crate::agents::bind::BindSource;
use crate::agents::embed::{DEFAULT_IDLE_TTL, DEFAULT_IP_RATE, DEFAULT_VISITOR_RATE};
use crate::agents::human::DEFAULT_HUMAN_TIMEOUT;
use crate::agents::output_filter::Action;
use crate::agents::retention::{DEFAULT_AUDIT_RETENTION_DAYS, DEFAULT_RETENTION_DAYS};
use crate::agents::router::loop_route::{DEFAULT_ITERATIONS, MAX_ITERATIONS};
use crate::agents::verifier::host_jwt::ClaimMap;
use crate::agents::verifier::{
    self, CHECK_TOOL_DEFAULT, CODE_TTL_DEFAULT, EMAIL_SENDS_DEFAULT, IP_SENDS_DEFAULT, InputSource,
    JwtAlgorithm, LIFETIME_DEFAULT, MAX_ATTEMPTS_CAP, MAX_ATTEMPTS_DEFAULT, MAX_CODE_TTL,
    MAX_LIFETIME_CAP, SEND_TOOL_DEFAULT, SESSION_SENDS_DEFAULT, WriteSource,
};
use crate::budget::Budget;
use crate::server::tools::ask_first::DEFAULT_APPROVAL_TIMEOUT;

/// One agent's spec, as the validator accepted it.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentSpec {
    #[serde(default)]
    pub profile: Profile,
    pub scope: Option<Scope>,
    #[serde(default)]
    pub main: Main,
    #[serde(default)]
    pub state: BTreeMap<String, Slot>,
    #[serde(default)]
    pub verifiers: BTreeMap<String, Verifier>,
    pub router: Option<RouterConfig>,
    #[serde(default)]
    pub routes: BTreeMap<String, Route>,
    pub finish: Option<Finish>,
    #[serde(default)]
    pub publish: Publish,
}

impl AgentSpec {
    /// The typed form of `spec`. A spec [`super::check`] accepted always
    /// reads; the error is for one stored without passing it.
    pub fn from_value(spec: &Value) -> Result<Self, serde_json::Error> {
        Self::deserialize(spec)
    }

    /// The spec of an agent that sets nothing: every default.
    pub fn empty() -> &'static Self {
        static EMPTY: LazyLock<AgentSpec> = LazyLock::new(AgentSpec::default);
        &EMPTY
    }

    /// `main.model`, the model the main run uses; `None` runs on the
    /// gateway's default chat model.
    pub fn main_model(&self) -> Option<&str> {
        self.main.model.as_deref()
    }

    /// `finish.schema`, what a routed run of this agent returns.
    pub fn finish_schema(&self) -> Option<&Value> {
        self.finish.as_ref().map(|f| &f.schema)
    }

    /// The `host_jwt` verifier, when the spec declares one (at most one).
    pub fn host_jwt(&self) -> Option<(&str, &HostJwtSpec)> {
        self.verifiers.iter().find_map(|(id, v)| match v {
            Verifier::HostJwt(cfg) => Some((id.as_str(), cfg)),
            _ => None,
        })
    }
}

/// A duration as the spec writes it: `30s`, `15m`, `2h`, `30d`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpecDuration(pub SignedDuration);

impl SpecDuration {
    fn std(self) -> Option<Duration> {
        u64::try_from(self.0.as_secs())
            .ok()
            .map(Duration::from_secs)
    }
}

impl<'de> Deserialize<'de> for SpecDuration {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        parse_duration(&s)
            .map(Self)
            .ok_or_else(|| de::Error::custom(format!("`{s}` is not a duration")))
    }
}

/// A spec word read into the runtime type that has a parser for it.
fn word<'de, D: Deserializer<'de>, T>(
    d: D,
    parse: impl FnOnce(&str) -> Option<T>,
    what: &str,
) -> Result<T, D::Error> {
    let s = String::deserialize(d)?;
    parse(&s).ok_or_else(|| de::Error::custom(format!("`{s}` is not {what}")))
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub display: Option<String>,
    pub avatar: Option<String>,
    pub color: Option<String>,
}

impl Profile {
    /// `color` as `#rrggbb` in lowercase, unless it is blank or not one.
    pub fn color(&self) -> Option<String> {
        self.color
            .as_deref()
            .map(str::trim)
            .filter(|c| is_hex_color(c))
            .map(str::to_ascii_lowercase)
    }

    /// `display`, unless it is blank.
    pub fn display(&self) -> Option<&str> {
        self.display.as_deref().filter(|d| !d.trim().is_empty())
    }
}

/// `scope`: what the agent talks about. The topics and the refusal go into
/// the system message as guidance; `strict` adds the topic guard
/// ([`crate::agents::topic_guard`]), which refuses an out-of-scope message
/// before the main model sees it.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    #[serde(default)]
    pub topics: Vec<String>,
    pub refusal: Option<String>,
    #[serde(default)]
    pub strict: bool,
    /// The model the guard classifies with; the main run's model when unset.
    pub classifier_model: Option<String>,
}

impl Scope {
    /// `refusal`, unless it is blank.
    pub fn refusal(&self) -> Option<&str> {
        self.refusal
            .as_deref()
            .map(str::trim)
            .filter(|r| !r.is_empty())
    }

    /// The topics, trimmed, blank ones left out.
    pub fn topics(&self) -> Vec<&str> {
        self.topics
            .iter()
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
            .collect()
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Main {
    pub model: Option<String>,
    #[serde(default)]
    pub instructions: Instructions,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub tool_resources: BTreeMap<String, ToolResource>,
    #[serde(default)]
    pub budget: RunBudget,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Instructions {
    pub orchestration: Option<String>,
    pub response: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunBudget {
    pub rounds: Option<u32>,
    pub seconds: Option<u64>,
    pub tokens: Option<u64>,
}

impl RunBudget {
    /// The run's budget; rounds default to the default effort's.
    pub fn budget(&self) -> Budget {
        Budget::new(
            self.rounds
                .unwrap_or_else(|| aiplane_core::server::reasoning::Effort::default().max_rounds()),
            self.seconds,
            self.tokens,
        )
    }
}

/// `main.tool_resources.<tool>`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolResource {
    #[serde(default)]
    pub bind: BTreeMap<String, BindSource>,
    pub permission: Option<Permission>,
    pub approval_timeout: Option<SpecDuration>,
}

impl ToolResource {
    /// How long an approval waits; [`DEFAULT_APPROVAL_TIMEOUT`] when unset.
    pub fn approval_timeout(&self) -> Duration {
        self.approval_timeout
            .and_then(SpecDuration::std)
            .unwrap_or(DEFAULT_APPROVAL_TIMEOUT)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    AlwaysAllow,
    AlwaysAsk,
}

/// `state.<slot>`. Its run-time form is [`crate::agents::state::SlotDef`],
/// compiled from the same JSON by the code the validator shares.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Slot {
    #[serde(rename = "type")]
    pub kind: SlotKind,
    pub set_by: Vec<String>,
    pub description: Option<String>,
    pub values: Option<Vec<Value>>,
    pub min_length: Option<u64>,
    pub max_length: Option<u64>,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
    pub pattern: Option<String>,
    pub schema: Option<Value>,
    /// Where the slot sits in the setup's list of details (#116). A JSON
    /// object's key order does not survive a save, so the order the manager
    /// gave lives here; nothing at run time reads it.
    pub order: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlotKind {
    String,
    Email,
    Enum,
    Integer,
    Number,
    Boolean,
    Subject,
}

/// `verifiers.<id>`, by its `kind`.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Verifier {
    McpCode(McpCodeSpec),
    Lookup(LookupSpec),
    HostJwt(HostJwtSpec),
}

/// An `mcp_code` verifier. What it needs to run is required only on
/// publish, so a draft's may still lack it.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpCodeSpec {
    pub assurance: Option<String>,
    pub connector: Option<String>,
    pub send_tool: Option<String>,
    pub check_tool: Option<String>,
    pub input: Option<String>,
    pub email_slot: Option<String>,
    #[serde(default)]
    pub writes: BTreeMap<String, WriteSource>,
    pub max_attempts: Option<u32>,
    pub code_ttl: Option<SpecDuration>,
    #[serde(default)]
    pub send_limits: SendLimitsSpec,
}

impl McpCodeSpec {
    fn tool(name: Option<&str>, default: &'static str) -> String {
        name.filter(|s| !s.trim().is_empty())
            .unwrap_or(default)
            .to_string()
    }

    pub fn send_tool(&self) -> String {
        Self::tool(self.send_tool.as_deref(), SEND_TOOL_DEFAULT)
    }

    pub fn check_tool(&self) -> String {
        Self::tool(self.check_tool.as_deref(), CHECK_TOOL_DEFAULT)
    }

    pub fn max_attempts(&self) -> u32 {
        max_attempts(self.max_attempts)
    }

    /// How long a code is good for, at most [`MAX_CODE_TTL`].
    pub fn code_ttl(&self) -> SignedDuration {
        self.code_ttl
            .map_or(CODE_TTL_DEFAULT, |d| d.0)
            .min(MAX_CODE_TTL)
    }

    pub fn send_limits(&self) -> verifier::SendLimits {
        let l = &self.send_limits;
        verifier::SendLimits {
            email: l.email.map_or(EMAIL_SENDS_DEFAULT, RateSpec::rate),
            ip: l.ip.map_or(IP_SENDS_DEFAULT, RateSpec::rate),
            session: l.session.map_or(SESSION_SENDS_DEFAULT, RateSpec::rate),
        }
    }
}

/// Wrong guesses a verifier allows per code or lookup, `1..=MAX_ATTEMPTS_CAP`.
fn max_attempts(set: Option<u32>) -> u32 {
    set.unwrap_or(MAX_ATTEMPTS_DEFAULT)
        .clamp(1, MAX_ATTEMPTS_CAP)
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SendLimitsSpec {
    pub email: Option<RateSpec>,
    pub ip: Option<RateSpec>,
    pub session: Option<RateSpec>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LookupSpec {
    pub assurance: Option<String>,
    pub tool: Option<String>,
    #[serde(default)]
    pub inputs: BTreeMap<String, InputSource>,
    #[serde(default)]
    pub writes: BTreeMap<String, WriteSource>,
    pub max_attempts: Option<u32>,
}

impl LookupSpec {
    pub fn max_attempts(&self) -> u32 {
        max_attempts(self.max_attempts)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostJwtSpec {
    pub assurance: Option<String>,
    pub algorithm: Option<JwtAlgorithm>,
    /// The plaintext, only between a save's validation and its sealing.
    pub secret: Option<String>,
    pub secret_sealed: Option<String>,
    pub public_key: Option<String>,
    pub jwks_url: Option<String>,
    pub issuer: Option<String>,
    pub audience: Option<String>,
    pub max_lifetime: Option<SpecDuration>,
    #[serde(default)]
    pub claims: BTreeMap<String, ClaimMap>,
}

impl HostJwtSpec {
    /// The longest lifetime a token may claim, at most [`MAX_LIFETIME_CAP`].
    pub fn max_lifetime(&self) -> SignedDuration {
        self.max_lifetime
            .map_or(LIFETIME_DEFAULT, |d| d.0)
            .min(MAX_LIFETIME_CAP)
    }
}

impl<'de> Deserialize<'de> for JwtAlgorithm {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        word(d, JwtAlgorithm::parse, "a JWT algorithm")
    }
}

impl<'de> Deserialize<'de> for WriteSource {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        word(d, WriteSource::parse, "a verifier write source")
    }
}

impl<'de> Deserialize<'de> for InputSource {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        let source = match &v {
            Value::String(s) => s
                .strip_prefix("state.")
                .map(|slot| Self::Slot(slot.to_string())),
            Value::Object(lit) if lit.len() == 1 => lit.get("const").cloned().map(Self::Const),
            _ => None,
        };
        source.ok_or_else(|| de::Error::custom(format!("{v} is not a lookup input")))
    }
}

impl<'de> Deserialize<'de> for ClaimMap {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Claim(String),
            Object(BTreeMap<String, String>),
        }
        Ok(match Raw::deserialize(d)? {
            Raw::Claim(c) => Self::Claim(c),
            Raw::Object(fields) => Self::Object(fields.into_iter().collect()),
        })
    }
}

impl<'de> Deserialize<'de> for BindSource {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        Self::parse(&v).ok_or_else(|| de::Error::custom(format!("{v} is not a bind source")))
    }
}

impl<'de> Deserialize<'de> for AuthKind {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        word(d, AuthKind::parse, "an A2A auth kind")
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouterConfig {
    pub kind: RouterKind,
    pub model: Option<String>,
    #[serde(default)]
    pub order: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouterKind {
    Rules,
    Classifier,
}

/// `routes.<name>`: a gate, and the one target it opens.
#[derive(Debug, Clone, Deserialize)]
#[serde(try_from = "RawRoute")]
pub struct Route {
    pub description: Option<String>,
    /// The gate, compiled by [`crate::agents::gate::Cond`].
    pub when: Value,
    /// The task template; empty for a route to a person, which has none.
    pub task: String,
    pub bind: BTreeMap<String, BindSource>,
    pub target: RouteTarget,
}

#[derive(Debug, Clone)]
pub enum RouteTarget {
    /// A sub-agent, by agent id.
    Agent(String),
    Human(HumanSpec),
    A2a(A2aRouteSpec),
    Loop(LoopSpec),
}

impl Route {
    /// The agent ids this route runs: its sub-agent, or a loop's worker and
    /// critic.
    pub fn agents(&self) -> Vec<&str> {
        match &self.target {
            RouteTarget::Agent(id) => vec![id.as_str()],
            RouteTarget::Loop(l) => vec![l.worker.as_str(), l.critic.as_str()],
            RouteTarget::Human(_) | RouteTarget::A2a(_) => Vec::new(),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRoute {
    description: Option<String>,
    when: Value,
    agent: Option<String>,
    task: Option<String>,
    #[serde(default)]
    bind: BTreeMap<String, BindSource>,
    human: Option<HumanSpec>,
    a2a: Option<A2aRouteSpec>,
    #[serde(rename = "loop")]
    looped: Option<LoopSpec>,
}

impl TryFrom<RawRoute> for Route {
    type Error = String;

    fn try_from(r: RawRoute) -> Result<Self, String> {
        let mut targets = [
            r.agent.map(RouteTarget::Agent),
            r.human.map(RouteTarget::Human),
            r.a2a.map(RouteTarget::A2a),
            r.looped.map(RouteTarget::Loop),
        ]
        .into_iter()
        .flatten();
        let target = targets
            .next()
            .ok_or("a route needs a target: `agent`, `human`, `a2a` or `loop`")?;
        if targets.next().is_some() {
            return Err("a route has exactly one target".into());
        }
        Ok(Self {
            description: r.description,
            when: r.when,
            task: r.task.unwrap_or_default(),
            bind: r.bind,
            target,
        })
    }
}

/// A `human` route's target.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanSpec {
    pub notify: Option<Vec<String>>,
    pub inbox: Option<String>,
    pub timeout: Option<SpecDuration>,
    pub transcript: Option<bool>,
}

impl HumanSpec {
    /// How long a handoff waits; [`DEFAULT_HUMAN_TIMEOUT`] when unset.
    pub fn timeout(&self) -> Duration {
        self.timeout
            .and_then(SpecDuration::std)
            .unwrap_or(DEFAULT_HUMAN_TIMEOUT)
    }

    /// Whether the transcript goes along; off unless set.
    pub fn transcript(&self) -> bool {
        self.transcript.unwrap_or(false)
    }
}

/// An `a2a` route's target. `finish` is required only on publish.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct A2aRouteSpec {
    pub card_url: String,
    pub auth: Option<A2aAuthSpec>,
    pub finish: Option<Finish>,
    #[serde(default)]
    pub budget: A2aBudget,
}

impl A2aRouteSpec {
    /// How long the route waits for the remote agent, at most
    /// [`a2a_client::MAX_SECONDS`].
    pub fn seconds(&self) -> u64 {
        self.budget
            .seconds
            .unwrap_or(a2a_client::DEFAULT_SECONDS)
            .clamp(1, a2a_client::MAX_SECONDS)
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct A2aBudget {
    pub seconds: Option<u64>,
}

/// `a2a.auth`. The credential is stored sealed; its plaintext key exists
/// only between a save's validation and its sealing.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct A2aAuthSpec {
    pub kind: AuthKind,
    pub scheme: Option<String>,
    pub token: Option<String>,
    pub token_sealed: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub client_secret_sealed: Option<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
}

impl A2aAuthSpec {
    /// The sealed credential `kind` uses.
    pub fn secret_sealed(&self) -> Option<&str> {
        match self.kind {
            AuthKind::Bearer | AuthKind::ApiKey => self.token_sealed.as_deref(),
            AuthKind::ClientCredentials => self.client_secret_sealed.as_deref(),
        }
    }
}

/// A `loop` route's target.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoopSpec {
    pub worker: String,
    pub critic: String,
    pub max_iterations: Option<u64>,
    #[serde(default)]
    pub budget: LoopBudget,
}

impl LoopSpec {
    /// `1..=MAX_ITERATIONS`, [`DEFAULT_ITERATIONS`] when unset.
    pub fn max_iterations(&self) -> u64 {
        self.max_iterations
            .unwrap_or(DEFAULT_ITERATIONS)
            .clamp(1, MAX_ITERATIONS)
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoopBudget {
    pub seconds: Option<u64>,
    pub tokens: Option<u64>,
}

/// `finish` (and an A2A route's `a2a.finish`): the result's JSON schema.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Finish {
    pub schema: Value,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Publish {
    pub origins: Option<Vec<String>>,
    pub idle_ttl: Option<SpecDuration>,
    pub retention_days: Option<NonZeroU64>,
    pub audit_retention_days: Option<NonZeroU64>,
    #[serde(default)]
    pub rate_limits: RateLimits,
    #[serde(default)]
    pub budget: PublishBudget,
    pub output_filter: Option<OutputFilterSpec>,
    #[serde(default)]
    pub require_passing_tests: bool,
    pub a2a: Option<A2aPublish>,
    #[serde(default)]
    pub voice: VoiceSpec,
}

impl Publish {
    /// A visitor session's idle TTL; [`DEFAULT_IDLE_TTL`] when unset.
    pub fn idle_ttl(&self) -> SignedDuration {
        self.idle_ttl.map_or(DEFAULT_IDLE_TTL, |d| d.0)
    }

    /// Whether a browser on `origin` may use the agent: always when the spec
    /// sets no `origins`, otherwise only an origin listed there.
    pub fn allows_origin(&self, origin: &str) -> bool {
        self.origins
            .as_ref()
            .is_none_or(|list| list.iter().any(|o| o == origin))
    }

    /// `rate_limits`, each scope falling back to its default.
    pub fn visitor_rates(&self) -> VisitorRates {
        VisitorRates {
            visitor: self
                .rate_limits
                .visitor
                .map_or(DEFAULT_VISITOR_RATE, RateSpec::rate),
            ip: self.rate_limits.ip.map_or(DEFAULT_IP_RATE, RateSpec::rate),
        }
    }

    /// How many idle days a conversation is kept;
    /// [`DEFAULT_RETENTION_DAYS`] when unset.
    pub fn retention_days(&self) -> i64 {
        self.retention_days.map_or(DEFAULT_RETENTION_DAYS, |d| {
            i64::try_from(d.get()).unwrap_or(i64::MAX)
        })
    }

    /// How many days the activity log keeps a conversation's events after
    /// its last one; [`DEFAULT_AUDIT_RETENTION_DAYS`] when unset. The
    /// validator keeps it at least [`Self::retention_days`].
    pub fn audit_retention_days(&self) -> i64 {
        self.audit_retention_days
            .map_or(DEFAULT_AUDIT_RETENTION_DAYS, |d| {
                i64::try_from(d.get()).unwrap_or(i64::MAX)
            })
    }

    /// Whether the agent is served over A2A. Off unless the spec says so.
    pub fn a2a_enabled(&self) -> bool {
        self.a2a.as_ref().is_some_and(|a| a.enabled)
    }
}

/// `#rrggbb`: the one colour form the widget and the builder both read.
pub fn is_hex_color(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

/// Spoken input and output for the embed widget (`publish.voice`). A
/// direction that is on runs on the model named here, else on the gateway's
/// default model for it (`agents::defaults::voice_model`); either way the
/// agent must hold a grant on it, which the validator checks on publish.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceSpec {
    #[serde(default)]
    pub input: bool,
    #[serde(default)]
    pub output: bool,
    /// The TTS voice; the speech model's default for the language when unset.
    pub voice: Option<String>,
    pub speech_model: Option<String>,
    pub transcription_model: Option<String>,
}

impl VoiceSpec {
    /// The model named for transcribing a visitor's recording, when voice
    /// input is on.
    pub fn input_model(&self) -> Option<&str> {
        self.transcription_model.as_deref().filter(|_| self.input)
    }

    /// The model named for speaking answers, when voice output is on.
    pub fn output_model(&self) -> Option<&str> {
        self.speech_model.as_deref().filter(|_| self.output)
    }
}

/// `{max, per}`, both required.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RateSpec {
    pub max: NonZeroU64,
    pub per: SpecDuration,
}

impl RateSpec {
    pub fn rate(self) -> Rate {
        Rate {
            max: u32::try_from(self.max.get()).unwrap_or(u32::MAX),
            per: self.per.0,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RateLimits {
    pub visitor: Option<RateSpec>,
    pub ip: Option<RateSpec>,
}

/// The owner's monthly ceilings; none by default.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishBudget {
    pub monthly_cost: Option<f64>,
    pub monthly_tokens: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputFilterSpec {
    /// Pattern name → regex; the filter is off while this is empty.
    #[serde(default)]
    pub patterns: BTreeMap<String, String>,
    #[serde(default)]
    pub action: Action,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct A2aPublish {
    pub enabled: bool,
    pub skills: Option<Vec<A2aSkill>>,
}

/// One skill an agent card advertises.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct A2aSkill {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub examples: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn read(spec: Value) -> AgentSpec {
        AgentSpec::from_value(&spec).unwrap()
    }

    #[test]
    fn an_empty_spec_reads_as_every_default() {
        let spec = read(json!({}));
        assert_eq!(spec.publish.idle_ttl(), DEFAULT_IDLE_TTL);
        assert_eq!(spec.publish.retention_days(), DEFAULT_RETENTION_DAYS);
        assert_eq!(
            spec.publish.audit_retention_days(),
            DEFAULT_AUDIT_RETENTION_DAYS
        );
        assert_eq!(spec.publish.visitor_rates().visitor, DEFAULT_VISITOR_RATE);
        assert_eq!(spec.publish.visitor_rates().ip, DEFAULT_IP_RATE);
        assert!(spec.publish.allows_origin("https://any.example"));
        assert!(!spec.publish.a2a_enabled());
        assert!(!spec.publish.require_passing_tests);
        assert!(spec.main_model().is_none() && spec.routes.is_empty());
    }

    #[test]
    fn only_a_hex_colour_reads_as_the_agents_colour() {
        let colour = |c: Value| read(json!({ "profile": { "color": c } })).profile.color();
        assert_eq!(colour(json!("#0B6BCB")), Some("#0b6bcb".into()));
        assert_eq!(colour(json!(" ")), None);
        assert_eq!(colour(Value::Null), None);
    }

    #[test]
    fn voice_is_off_until_a_direction_is_switched_on() {
        let spec = read(json!({}));
        assert_eq!(spec.publish.voice.input_model(), None);
        assert_eq!(spec.publish.voice.output_model(), None);

        let spec = read(json!({ "publish": { "voice": {
            "input": true, "output": false, "voice": "alloy",
            "speech_model": "tts-1", "transcription_model": "whisper"
        } } }));
        assert!(spec.publish.voice.input && !spec.publish.voice.output);
        assert_eq!(spec.publish.voice.input_model(), Some("whisper"));
        assert_eq!(spec.publish.voice.output_model(), None, "output is off");
        assert_eq!(spec.publish.voice.voice.as_deref(), Some("alloy"));
        assert!(
            AgentSpec::from_value(&json!({ "publish": { "voice": { "loud": true } } })).is_err()
        );
        assert!(
            AgentSpec::from_value(&json!({ "publish": { "voice": { "speech_pool": "tts" } } }))
                .is_err(),
            "voice names models, not pools"
        );
    }

    #[test]
    fn an_unknown_key_anywhere_does_not_read() {
        for bad in [
            json!({ "mian": {} }),
            json!({ "main": { "tool_resource": {} } }),
            json!({ "publish": { "rate_limits": { "visitors": { "max": 1, "per": "1m" } } } }),
            json!({ "routes": { "r": { "when": {}, "agent": "a", "taks": "t" } } }),
            json!({ "verifiers": { "v": { "kind": "lookup", "connector": "erp" } } }),
            json!({ "routes": { "r": { "when": {}, "loop": {
                "worker": "w", "critic": "c", "budget": { "rounds": 2 } } } } }),
        ] {
            assert!(AgentSpec::from_value(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_route_reads_into_its_one_target() {
        let spec = read(json!({ "routes": {
            "sub": { "when": {}, "agent": "a1", "task": "t", "bind": { "c": "state.v.id" } },
            "people": { "when": {}, "human": { "timeout": "2h" } },
            "partner": { "when": {}, "task": "t", "a2a": { "card_url": "https://p.example" } },
            "offer": { "when": {}, "task": "t", "loop": { "worker": "w", "critic": "c" } },
        } }));
        assert!(matches!(&spec.routes["sub"].target, RouteTarget::Agent(id) if id == "a1"));
        assert_eq!(
            spec.routes["sub"].bind["c"],
            BindSource::State("v.id".into())
        );
        let RouteTarget::Human(h) = &spec.routes["people"].target else {
            panic!("a human target")
        };
        assert_eq!(h.timeout(), Duration::from_secs(7200));
        assert!(!h.transcript());
        let RouteTarget::A2a(a) = &spec.routes["partner"].target else {
            panic!("an a2a target")
        };
        assert_eq!(a.seconds(), a2a_client::DEFAULT_SECONDS);
        let RouteTarget::Loop(l) = &spec.routes["offer"].target else {
            panic!("a loop target")
        };
        assert_eq!(l.max_iterations(), DEFAULT_ITERATIONS);
        assert_eq!(spec.routes["offer"].agents(), ["w", "c"]);
        assert!(
            AgentSpec::from_value(
                &json!({ "routes": { "r": { "when": {}, "agent": "a", "human": {} } } })
            )
            .is_err(),
            "two targets"
        );
    }

    #[test]
    fn a_verifier_reads_by_its_kind() {
        let spec = read(json!({ "verifiers": {
            "otp": { "kind": "mcp_code", "connector": "erp", "writes": { "v": "result" },
                     "send_limits": { "ip": { "max": 2, "per": "1h" } } },
            "site": { "kind": "host_jwt", "algorithm": "HS256", "claims": {
                "v": { "customer_id": "sub" } } },
        } }));
        let Verifier::McpCode(otp) = &spec.verifiers["otp"] else {
            panic!("an mcp_code verifier")
        };
        assert_eq!(otp.send_tool(), SEND_TOOL_DEFAULT);
        assert_eq!(otp.send_limits().ip.max, 2);
        assert_eq!(otp.send_limits().email, EMAIL_SENDS_DEFAULT);
        let (id, jwt) = spec.host_jwt().expect("a host_jwt verifier");
        assert_eq!(id, "site");
        assert_eq!(jwt.max_lifetime(), LIFETIME_DEFAULT);
    }
}
