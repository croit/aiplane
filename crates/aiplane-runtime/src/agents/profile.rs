// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The `RunProfile` of an agent run (`docs/agent-runs.md`): everything that
//! makes a headless turn the run of one agent's live version.
//!
//! - **System message**: the owner's brief in sections — `## Role` (the
//!   agent's display name), `## Task` (`orchestration`), `## Scope` (the
//!   topics and the refusal) and `## Tone` (`response`) — then the slot view
//!   and each route's gate status. No chat rules, no person's memory,
//!   location or connectors. Rebuilt every round, so a slot the model just
//!   set shows up on the next one.
//! - **Topic guard** (main agent, strict scope only): the
//!   [`TopicGuard`] the turn consults before its first model call.
//! - **Tools**: the principal's grants that the spec lists, plus the run's
//!   synthetic tools (`set_<slot>`, `forward_request`, `request_human`, the
//!   verifiers' `verify_<id>…`). The synthetic ones are layered over the
//!   grant-narrowed source, never inside it: they are no grant, and exist
//!   only for this run.
//! - **Bound arguments** wrap the granted tools they apply to, so the model
//!   never sees nor sets them; a tool's `permission` puts an approval in
//!   front of it ([`super::approval`]).
//! - **Budget**, **finish contract** (sub-agents only) and an injection policy
//!   of `Flag`: an agent's tool results are untrusted data by default.

use std::collections::BTreeMap;
use std::sync::Arc;

use aiplane_agents::db::{agents as agents_db, system_principals as sp};
use aiplane_core::server::db::{DbError, Pool};
use aiplane_core::server::principal::{GrantKind, SystemPrincipal};
use aiplane_core::server::run_chain::RunChain;
use aiplane_core::server::upstreams::PoolAccess;
use serde_json::Value;
use shared::api::ToolDef;

use super::approval::Permissions;
use super::bind::{BoundTool, ToolBinds, WithheldTool, without_bound};
use super::gate::{GateInput, GateStatus, RouteGates};
use super::human::{RequestHuman, human_routes};
use super::output_filter::OutputFilter;
use super::router::{ForwardRequest, RouteClassifier, RouterSpec};
use super::slot_tools::SlotTools;
use super::spec::AgentSpec;
use super::spec::model::{Instructions, Route, Scope};
use super::spec_cache::CompiledSpec;
use super::state::{self, StateSchema, StateSnapshot, render_view};
use super::topic_guard::TopicGuard;
use super::verifier::{self, VerifierRun, Verifiers};
use crate::agent_run::{Actor, AgentRun, MismatchedRun};
use crate::budget::{Budget, SpendMeter};
use crate::finish::FinishContract;
use crate::rama_server::state::RamaState;
use crate::server::headless::DriveParams;
use crate::server::tools::injection::{InjectionPolicy, InjectionScan};
use crate::server::tools::{Tool, ToolPhase, ToolSource};
use aiplane_core::server::db::usage::UsageSource;

/// Why an agent could not be run.
#[derive(Debug, thiserror::Error)]
pub enum AgentRunError {
    #[error("{message}", message = session_core::i18n::t(session_core::i18n::Lang::En, "organization-error-execution-unavailable"))]
    OrganizationExecutionUnavailable(String),
    #[error(
        "there is no enabled agent `{0}`: it was deleted, or its principal is disabled. Re-enable \
         it or route to another agent"
    )]
    Unavailable(String),
    #[error("agent `{0}` has never been published, so it has no live version to run; publish it")]
    NotLive(String),
    #[error(
        "agent `{agent}` has no version {version}, which this conversation runs on; start a new \
         conversation to use the live version"
    )]
    MissingVersion { agent: String, version: i64 },
    #[error(
        "agent `{agent}` cannot run its live version {version}: {message}. Publish a corrected \
         version"
    )]
    BadSpec {
        agent: String,
        version: i64,
        message: String,
    },
    #[error(
        "agent `{agent}` names no model and the gateway has no default chat model to run it \
         on; set its model, or set a default chat model under Models & routing"
    )]
    NoModel { agent: String },
    #[error(
        "agent `{agent}` runs on model `{model}`, but holds no grant on it; grant it the model \
         (kind `model`) or pick one it holds"
    )]
    ModelNotGranted { agent: String, model: String },
    #[error("agent `{agent}` has no conversation `{session}`; start a new one instead")]
    UnknownSession { agent: String, session: String },
    #[error(
        "conversation `{session}` is waiting for a decision on turn `{turn}`; answer it through \
         that turn's resume route (or let it expire) before sending the next message"
    )]
    DecisionPending { session: String, turn: String },
    #[error(
        "conversation `{session}` is running a turn right now; wait for its answer, then send \
         the next message"
    )]
    Busy { session: String },
    #[error(transparent)]
    Mismatched(#[from] MismatchedRun),
    #[error("reading or writing the agent run failed: {0}")]
    Db(#[from] DbError),
}

/// Seams a run can be given: the clock gates and slot writes read, and the
/// route classifier. Both default to the real thing.
#[derive(Clone)]
pub struct RunOptions {
    pub now: state::Clock,
    /// `None` classifies with the agent's model (`router.model`, else the
    /// main run's). A seam because the classifier is the one model call the
    /// router makes on its own: a caller can swap it for a test double, or a
    /// deterministic stand-in, without faking an upstream.
    pub classifier: Option<Arc<dyn RouteClassifier>>,
    /// Where every run started with these options adds the tokens it spends:
    /// set by a `loop` route, whose budget covers all of its child runs (and
    /// anything they dispatch). `None` counts nowhere.
    pub spend: Option<Arc<SpendMeter>>,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            now: state::system_clock(),
            classifier: None,
            spend: None,
        }
    }
}

/// Which part an agent plays in this run.
#[derive(Debug, Clone)]
pub enum Role {
    /// The conversational agent the visitor talks to. Ends its turn with text.
    Main,
    /// Dispatched by a route: ends only through `finish`. `route_binds` are
    /// the route's `bind` values, resolved from the caller's state.
    SubAgent {
        route_binds: BTreeMap<String, Value>,
    },
}

/// One agent's live version, ready to drive.
pub struct RunProfile {
    pub principal: SystemPrincipal,
    pub version: i64,
    pub model: String,
    pub budget: Budget,
    pub finish: Option<FinishContract>,
    pub injection: InjectionScan,
    pub surface: Arc<AgentSurface>,
    /// `None` when the spec configures no identifier patterns.
    pub output_filter: Option<OutputFilter>,
}

/// Which spec of an agent a run loads.
#[derive(Debug, Clone)]
pub enum SpecSource {
    /// The published version the agent points at: what visitors get.
    Live,
    /// A published version, so a conversation keeps the one it started on.
    Pinned(i64),
    /// An unpublished spec, run as version [`agents_db::DRAFT_VERSION`]. Only
    /// `agents::run::draft` loads one: the test chat, the architect's test
    /// turn and an evaluation.
    Draft(Value),
}

impl RunProfile {
    /// Load agent `agent_id`'s live version and its principal's grants.
    pub async fn load(
        state: &Arc<RamaState>,
        agent_id: &str,
        role: Role,
        options: &RunOptions,
    ) -> Result<Self, AgentRunError> {
        Self::load_version(state, agent_id, None, role, options).await
    }

    /// [`Self::load`], on version `pinned` instead of the live one when it is
    /// `Some`: a conversation keeps the version it started on.
    pub async fn load_version(
        state: &Arc<RamaState>,
        agent_id: &str,
        pinned: Option<i64>,
        role: Role,
        options: &RunOptions,
    ) -> Result<Self, AgentRunError> {
        let source = pinned.map_or(SpecSource::Live, SpecSource::Pinned);
        Self::load_from(state, agent_id, source, role, options).await
    }

    /// Load agent `agent_id` from an explicit [`SpecSource`]. Only the
    /// internal test chat passes [`SpecSource::Draft`]; every other caller
    /// reaches a draft by no path at all.
    pub async fn load_from(
        state: &Arc<RamaState>,
        agent_id: &str,
        source: SpecSource,
        role: Role,
        options: &RunOptions,
    ) -> Result<Self, AgentRunError> {
        if matches!(
            agents_db::dot_owner(&state.db, agent_id).await?,
            Some(agents_db::DotOwner::Organization(_))
        ) {
            return Err(AgentRunError::OrganizationExecutionUnavailable(
                agent_id.to_owned(),
            ));
        }
        let principal = sp::load_active(&state.db, agent_id)
            .await?
            .ok_or_else(|| AgentRunError::Unavailable(agent_id.to_string()))?;
        let compiled = match source {
            SpecSource::Live => state
                .agent_specs
                .live(&state.db, agent_id)
                .await?
                .ok_or_else(|| AgentRunError::NotLive(principal.name.clone()))?,
            SpecSource::Pinned(v) => state
                .agent_specs
                .version(&state.db, agent_id, v)
                .await?
                .ok_or_else(|| AgentRunError::MissingVersion {
                    agent: principal.name.clone(),
                    version: v,
                })?,
            SpecSource::Draft(spec) => {
                Arc::new(CompiledSpec::compile(agents_db::DRAFT_VERSION, spec))
            }
        };
        let version = compiled.version;
        let bad = |message: String| AgentRunError::BadSpec {
            agent: principal.name.clone(),
            version,
            message,
        };
        let parts = compiled.parts().map_err(|m| bad(m.to_string()))?;
        let spec = &parts.agent;
        let (schema, gates) = (parts.schema.clone(), parts.gates.clone());
        let model = super::defaults::main_model(state, spec)
            .await
            .ok_or_else(|| AgentRunError::NoModel {
                agent: principal.name.clone(),
            })?;
        if !principal.grants.has(GrantKind::Model, &model) {
            return Err(AgentRunError::ModelNotGranted {
                agent: principal.name.clone(),
                model,
            });
        }
        let guard = match role {
            Role::Main => TopicGuard::from_spec(spec, &model),
            Role::SubAgent { .. } => None,
        };
        let display = match spec.profile.display() {
            Some(display) => display.to_string(),
            None => sp::get(&state.db, agent_id)
                .await?
                .map(|row| row.display)
                .filter(|d| !d.trim().is_empty())
                .unwrap_or_else(|| principal.name.clone()),
        };
        let models = PoolAccess::for_system_models(&principal, [model.as_str()]);
        let finish = match &role {
            Role::Main => None,
            Role::SubAgent { .. } => {
                let schema = spec.finish_schema().cloned().ok_or_else(|| {
                    bad("it declares no `finish.schema`, which a routed sub-agent needs".into())
                })?;
                Some(FinishContract::new(schema).map_err(|e| bad(e.to_string()))?)
            }
        };

        let output_filter = parts.output_filter.clone().map_err(bad)?;
        let mut synthetic: BTreeMap<String, Synthetic> = BTreeMap::new();
        let mut add = |tool: Arc<dyn Tool>, phase: ToolPhase| {
            synthetic.insert(tool.id().to_string(), Synthetic { tool, phase });
        };
        let slot_tools = SlotTools::with_clock(schema.clone(), options.now.clone());
        for id in slot_tools.ids() {
            if let Some(tool) = slot_tools.get(&id) {
                add(tool, ToolPhase::WritesState);
            }
        }
        if matches!(role, Role::Main) {
            let run = VerifierRun {
                state: state.clone(),
                principal: principal.clone(),
                schema: schema.clone(),
                options: options.clone(),
            };
            for tool in verifier::tools(&Verifiers::from_spec(spec), &run) {
                add(tool, ToolPhase::WritesState);
            }
        }
        let snapshot = Arc::new(StateSnapshot::default());
        let conversation = (!schema.is_empty() || !gates.is_empty()).then(|| Conversation {
            schema: schema.clone(),
            gates: gates.clone(),
            agent: spec.clone(),
            now: options.now.clone(),
        });
        if !gates.is_empty() {
            let router = Arc::new(RouterSpec {
                principal: principal.clone(),
                agent: spec.clone(),
                schema: schema.clone(),
                gates,
                main_model: model.clone(),
                snapshot: snapshot.clone(),
            });
            if matches!(role, Role::Main)
                && let Some(human) = RequestHuman::new(router.clone(), options.clone())
            {
                add(Arc::new(human), ToolPhase::ActsOnState);
            }
            let forward = ForwardRequest::new(state.clone(), router, options.clone());
            add(Arc::new(forward), ToolPhase::ActsOnState);
        }
        let binds = match role {
            Role::Main => ToolBinds::from_spec(spec),
            Role::SubAgent { route_binds } => ToolBinds::from_spec(spec).with_route(route_binds),
        };
        let surface = AgentSurface {
            brief: Brief {
                display,
                instructions: spec.main.instructions.clone(),
                scope: spec.scope.clone(),
            },
            guard,
            conversation,
            tools: spec.main.tools.clone(),
            synthetic,
            binds,
            permissions: Permissions::from_spec(spec),
            schema: (!schema.is_empty()).then_some(schema),
            snapshot,
            models,
            spend: options.spend.clone(),
        };
        Ok(Self {
            budget: spec.main.budget.budget(),
            principal,
            version,
            model,
            finish,
            injection: InjectionScan::new(InjectionPolicy::Flag),
            surface: Arc::new(surface),
            output_filter,
        })
    }

    /// The run of this profile in `chain`, whose running frame must be this
    /// profile's principal.
    pub fn agent_run(&self, chain: Arc<RunChain>) -> Result<AgentRun, MismatchedRun> {
        let run = AgentRun::new(self.principal.clone(), chain)?
            .with_surface(self.surface.clone())
            .with_budget(self.budget)
            .with_injection(self.injection.clone());
        Ok(match &self.finish {
            Some(contract) => run.with_contract(contract.clone()),
            None => run,
        })
    }

    /// The headless parameters that run this profile's turn in `session_id`.
    pub fn drive_params(
        &self,
        session_id: &str,
        assistant_turn_id: &str,
        chain: Arc<RunChain>,
    ) -> Result<DriveParams, MismatchedRun> {
        Ok(DriveParams {
            actor: Actor::Agent(Arc::new(self.agent_run(chain)?)),
            session_id: session_id.to_string(),
            assistant_turn_id: assistant_turn_id.to_string(),
            model: self.model.clone(),
            source: UsageSource::Agent,
            history_limit: None,
        })
    }
}

/// The main agent's conversation pieces: what its system message reports.
struct Conversation {
    schema: Arc<StateSchema>,
    gates: Arc<RouteGates>,
    agent: Arc<AgentSpec>,
    now: state::Clock,
}

/// A run-scoped tool of the spec, and when it runs among the calls of its
/// round. With the run's `finish` tool ([`ToolPhase::Terminal`], from
/// [`AgentRun::terminal_tool`]) the only tools tagged with a phase other than
/// the default.
struct Synthetic {
    tool: Arc<dyn Tool>,
    phase: ToolPhase,
}

/// What an agent's spec puts in front of the model on every round of its run:
/// the system message, the offered and synthetic tools, bound arguments and
/// permissions, the conversation state, and the models its calls may use.
/// Part of the run's [`AgentRun`].
pub struct AgentSurface {
    brief: Brief,
    guard: Option<TopicGuard>,
    conversation: Option<Conversation>,
    tools: Vec<String>,
    synthetic: BTreeMap<String, Synthetic>,
    binds: ToolBinds,
    permissions: Permissions,
    schema: Option<Arc<StateSchema>>,
    /// This turn's read of the conversation state; see [`StateSnapshot`].
    snapshot: Arc<StateSnapshot>,
    models: PoolAccess,
    spend: Option<Arc<SpendMeter>>,
}

impl AgentSurface {
    /// Count `tokens` this run's round spent against the allowance it runs
    /// inside, if any.
    pub fn record_spend(&self, tokens: u64) {
        if let Some(meter) = &self.spend {
            meter.add(tokens);
        }
    }

    /// What this run's own model calls may use: its main model (and, for
    /// an automatic route, what the route picks). The turn's rounds and the
    /// compaction of its conversation both route through it.
    pub fn models(&self) -> &PoolAccess {
        &self.models
    }

    /// The tools offered this round: the spec's tools the principal is
    /// granted (`granted`), then the synthetic ones.
    pub fn state_schema(&self) -> Option<&Arc<StateSchema>> {
        self.schema.as_ref()
    }

    pub fn state_snapshot(&self) -> &StateSnapshot {
        &self.snapshot
    }

    pub fn offered(&self, granted: &[String]) -> Vec<String> {
        self.tools
            .iter()
            .filter(|t| granted.contains(t))
            .cloned()
            .chain(self.synthetic.keys().cloned())
            .collect()
    }

    /// The topic guard of a main agent with a strict scope.
    pub fn topic_guard(&self) -> Option<&TopicGuard> {
        self.guard.as_ref()
    }

    /// Whether the system message changes within a turn (it carries state).
    pub fn has_conversation_state(&self) -> bool {
        self.conversation.is_some()
    }

    /// The run's own sections of its leading system message
    /// (`openai_driver::leading_system_message`): the owner's brief, then
    /// the conversation's slots and open routes.
    pub async fn system_sections(&self, db: &Pool, session_id: &str) -> Vec<String> {
        let mut parts = vec![self.brief.render()];
        if let Some(c) = &self.conversation {
            let state = self
                .snapshot
                .get(db, &c.schema, session_id)
                .await
                .unwrap_or_else(|err| {
                    tracing::warn!(error = %err, session_id, "agent state unreadable; shown as empty");
                    Arc::default()
                });
            let view = render_view(&state.view(&c.schema));
            if !view.is_empty() {
                parts.push(view);
            }
            let input = GateInput {
                schema: &c.schema,
                state: &state,
                now: (c.now)(),
            };
            let routes = route_summary(&c.gates, &c.agent.routes, input);
            if !routes.is_empty() {
                parts.push(routes);
            }
        }
        parts
    }

    /// `inner` with this run's synthetic tools over it and its bound
    /// arguments applied.
    pub fn source<'a>(&'a self, inner: &'a dyn ToolSource) -> RunToolSource<'a> {
        RunToolSource {
            inner,
            run: Some(self),
            terminal: None,
        }
    }
}

/// What the owner wrote for the model, as the sections that open the
/// system message. They never change within a run.
struct Brief {
    /// The agent's display name: what it is called in front of visitors,
    /// never its id or slug.
    display: String,
    instructions: Instructions,
    scope: Option<Scope>,
}

impl Brief {
    fn render(&self) -> String {
        let mut sections = vec![format!(
            "## Role\n\nYou are {}. The sections below are your owner's instructions.",
            self.display
        )];
        let written = |s: &Option<String>| {
            s.as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        };
        if let Some(task) = written(&self.instructions.orchestration) {
            sections.push(format!("## Task\n\n{task}"));
        }
        if let Some(scope) = self.scope.as_ref().and_then(scope_section) {
            sections.push(format!("## Scope\n\n{scope}"));
        }
        if let Some(tone) = written(&self.instructions.response) {
            sections.push(format!("## Tone\n\n{tone}"));
        }
        sections.join("\n\n")
    }
}

/// The body of `## Scope`, written whether or not the scope is strict: for
/// a strict one it says what the guard enforces anyway.
fn scope_section(scope: &Scope) -> Option<String> {
    let topics = scope.topics();
    let mut lines = Vec::new();
    if !topics.is_empty() {
        lines.push("You cover only these topics:".to_string());
        lines.extend(topics.iter().map(|t| format!("- {t}")));
    }
    match scope.refusal() {
        Some(refusal) => lines.push(format!(
            "If asked about anything else, reply exactly: {refusal}"
        )),
        None if !topics.is_empty() => {
            lines.push("Politely decline anything outside them.".to_string())
        }
        None => {}
    }
    (!lines.is_empty()).then(|| lines.join("\n"))
}

fn route_summary(
    gates: &RouteGates,
    routes: &BTreeMap<String, Route>,
    input: GateInput<'_>,
) -> String {
    let statuses = gates.statuses(input);
    if statuses.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        "Routes. When the request is complete, call forward_request: the gateway picks an open \
         route and hands the request over. You choose neither the route nor whom it concerns.",
    );
    if !human_routes(routes).is_empty() {
        out.push_str(
            " To hand the conversation to a member of staff, call request_human with the \
             question they should answer; a route to a person must be open for it.",
        );
    }
    for (name, status) in statuses {
        let about = routes
            .get(&name)
            .and_then(|r| r.description.as_deref())
            .map(|d| format!(" ({d})"))
            .unwrap_or_default();
        let line = match status {
            GateStatus::Open => format!("- {name}{about}: open"),
            GateStatus::Closed { missing } => {
                let why: Vec<&str> = missing.iter().map(|u| u.message.as_str()).collect();
                format!("- {name}{about}: closed — {}", why.join("; "))
            }
        };
        out.push('\n');
        out.push_str(&line);
    }
    out
}

/// A turn's tool source with an agent run layered over it: the spec's
/// surface, and the run's `finish` tool under a contract. With no run it is
/// `inner` unchanged.
pub struct RunToolSource<'a> {
    inner: &'a dyn ToolSource,
    run: Option<&'a AgentSurface>,
    terminal: Option<Arc<dyn Tool>>,
}

impl<'a> RunToolSource<'a> {
    pub fn new(inner: &'a dyn ToolSource, run: Option<&'a AgentRun>) -> Self {
        Self {
            inner,
            run: run.and_then(AgentRun::surface),
            terminal: run.and_then(AgentRun::terminal_tool),
        }
    }

    fn terminal(&self, id: &str) -> Option<&Arc<dyn Tool>> {
        self.terminal.as_ref().filter(|t| t.id() == id)
    }

    fn def_for(&self, id: &str) -> Option<ToolDef> {
        if let Some(terminal) = self.terminal(id) {
            return Some(terminal.schema());
        }
        let ids = [id.to_string()];
        let Some(run) = self.run else {
            return self.inner.defs_for(&ids).into_iter().next();
        };
        match run.synthetic.get(id) {
            Some(synthetic) => Some(synthetic.tool.schema()),
            None => self
                .inner
                .defs_for(&ids)
                .into_iter()
                .next()
                .and_then(|def| {
                    let binds = run.binds.for_tool(id, &def).ok()?;
                    Some(without_bound(def, &binds))
                }),
        }
    }

    /// A granted tool as this run offers it: its bound arguments filled in,
    /// behind an approval when its permission asks for one. A withheld tool
    /// is refused outright, so nobody is asked to approve a call that could
    /// not run.
    fn bound(&self, run: &AgentSurface, tool: Arc<dyn Tool>) -> Arc<dyn Tool> {
        let bound: Arc<dyn Tool> = match run.binds.for_tool(tool.id(), &tool.schema()) {
            Ok(binds) if binds.is_empty() => run.permissions.gate(tool),
            Ok(binds) => Arc::new(BoundTool::new(
                run.permissions.gate(tool),
                binds,
                run.schema.clone(),
                run.snapshot.clone(),
            )),
            Err(unbound) => return Arc::new(WithheldTool::new(tool, unbound)),
        };
        bound
    }
}

impl ToolSource for RunToolSource<'_> {
    fn get(&self, id: &str) -> Option<Arc<dyn Tool>> {
        if let Some(terminal) = self.terminal(id) {
            return Some(terminal.clone());
        }
        let Some(run) = self.run else {
            return self.inner.get(id);
        };
        if let Some(synthetic) = run.synthetic.get(id) {
            return Some(synthetic.tool.clone());
        }
        self.inner.get(id).map(|t| self.bound(run, t))
    }

    fn defs_for(&self, allowed: &[String]) -> Vec<ToolDef> {
        if self.run.is_none() && self.terminal.is_none() {
            return self.inner.defs_for(allowed);
        }
        allowed.iter().filter_map(|id| self.def_for(id)).collect()
    }

    fn ids(&self) -> Vec<String> {
        let mut ids = self.inner.ids();
        if let Some(run) = self.run {
            ids.extend(run.synthetic.keys().cloned());
        }
        ids.extend(self.terminal.iter().map(|t| t.id().to_string()));
        ids
    }

    fn contains(&self, id: &str) -> bool {
        self.terminal(id).is_some()
            || self.run.is_some_and(|r| r.synthetic.contains_key(id))
            || self.inner.contains(id)
    }

    fn phase(&self, id: &str) -> ToolPhase {
        if self.terminal(id).is_some() {
            return ToolPhase::Terminal;
        }
        self.run
            .and_then(|r| r.synthetic.get(id))
            .map_or_else(|| self.inner.phase(id), |s| s.phase)
    }

    fn state_written(&self) {
        if let Some(run) = self.run {
            run.snapshot.written();
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn run_sources_preserve_inner_synthetic_and_terminal_phases_through_grants_and_discovery() {
        use crate::server::tools::{
            GrantedToolSource, ToolRegistry, discovery::DiscoverableToolSource, tests::PhasedEcho,
        };
        use aiplane_core::server::principal::GrantSet;
        use aiplane_core::server::run_chain::Frame;
        let registry = ToolRegistry::new().with(PhasedEcho);
        assert_eq!(
            RunToolSource::new(&registry, None).phase("company_echo"),
            ToolPhase::ActsOnState
        );
        let principal = SystemPrincipal {
            id: "phase-agent".into(),
            name: "phase-agent".into(),
            grants: Arc::new(GrantSet::default()),
        };
        let chain = Arc::new(RunChain::root(
            "root",
            None,
            Frame::for_principal(&principal, None),
        ));
        let mut surface = run();
        surface.synthetic.insert(
            "company_echo".into(),
            Synthetic {
                tool: Arc::new(PhasedEcho),
                phase: ToolPhase::WritesState,
            },
        );
        let agent = AgentRun::new(principal, chain)
            .unwrap()
            .with_surface(Arc::new(surface))
            .with_contract(
                FinishContract::new(serde_json::json!({"type":"object","properties":{}})).unwrap(),
            );
        let source = RunToolSource::new(&registry, Some(&agent));
        let ids = ["company_echo".to_string(), "finish".to_string()];
        let granted = GrantedToolSource::new(&source, ids.clone());
        let discovery = DiscoverableToolSource::new(&granted, &ids, &[]);
        for tool_source in [&source as &dyn ToolSource, &granted, &discovery] {
            assert_eq!(tool_source.phase("company_echo"), ToolPhase::WritesState);
            assert_eq!(tool_source.phase("finish"), ToolPhase::Terminal);
            assert_eq!(tool_source.phase("missing"), ToolPhase::Concurrent);
        }
    }
    use super::*;
    use crate::server::tools::echo::Echo;
    use crate::server::tools::{ToolContext, ToolRegistry};
    use serde_json::json;

    /// A run whose `lookup` tool binds `message` from the route: `message`
    /// is then a subject parameter, and `company_echo` declares it unbound.
    fn run() -> AgentSurface {
        let spec = json!({ "main": { "tool_resources": {
            "lookup": { "bind": { "message": "route.customer" } }
        } } });
        AgentSurface {
            brief: Brief {
                display: "Billing".into(),
                instructions: Instructions::default(),
                scope: None,
            },
            guard: None,
            conversation: None,
            tools: vec!["company_echo".into()],
            synthetic: BTreeMap::new(),
            binds: ToolBinds::from_spec(&AgentSpec::from_value(&spec).unwrap())
                .with_route(BTreeMap::from([("customer".into(), json!("K-1"))])),
            permissions: Permissions::default(),
            schema: None,
            snapshot: Arc::default(),
            models: PoolAccess::all(),
            spend: None,
        }
    }

    fn brief(spec: Value) -> String {
        let spec = AgentSpec::from_value(&spec).unwrap();
        Brief {
            display: "croit Support".into(),
            instructions: spec.main.instructions,
            scope: spec.scope,
        }
        .render()
    }

    #[test]
    fn the_brief_keeps_role_task_scope_and_tone_apart() {
        let rendered = brief(json!({
            "scope": { "topics": ["Ceph storage", "croit products"],
                       "refusal": "I can only help with Ceph and croit." },
            "main": { "instructions": { "orchestration": "Collect the issue, then forward it.",
                                        "response": "Short and friendly." } }
        }));
        assert_eq!(
            rendered,
            "## Role\n\nYou are croit Support. The sections below are your owner's instructions.\n\n\
             ## Task\n\nCollect the issue, then forward it.\n\n\
             ## Scope\n\nYou cover only these topics:\n- Ceph storage\n- croit products\n\
             If asked about anything else, reply exactly: I can only help with Ceph and croit.\n\n\
             ## Tone\n\nShort and friendly."
        );
    }

    #[test]
    fn a_section_the_owner_left_empty_is_left_out() {
        assert_eq!(
            brief(json!({ "main": { "instructions": { "response": "  " } } })),
            "## Role\n\nYou are croit Support. The sections below are your owner's instructions."
        );
        let topics_only = brief(json!({ "scope": { "topics": ["Ceph"] } }));
        assert!(
            topics_only.ends_with(
                "## Scope\n\nYou cover only these topics:\n- Ceph\n\
                 Politely decline anything outside them."
            ),
            "{topics_only}"
        );
        let refusal_only = brief(json!({ "scope": { "refusal": "Not my area." } }));
        assert!(
            refusal_only
                .ends_with("## Scope\n\nIf asked about anything else, reply exactly: Not my area."),
            "{refusal_only}"
        );
    }

    struct ActionEcho;
    impl Tool for ActionEcho {
        fn id(&self) -> &str {
            "company_echo"
        }
        fn schema(&self) -> ToolDef {
            Echo.schema()
        }
        fn run<'a>(
            &'a self,
            _ctx: ToolContext,
            args: Value,
        ) -> crate::server::tools::ToolFuture<'a> {
            Box::pin(async move { Ok(args) })
        }
        fn action_catalogue(&self) -> Option<&'static [crate::server::tools::ToolAction]> {
            Some(&[
                crate::server::tools::ToolAction {
                    id: "read",
                    description: "Read",
                },
                crate::server::tools::ToolAction {
                    id: "write",
                    description: "Write",
                },
            ])
        }
        fn classify_actions<'a>(
            &'a self,
            _ctx: ToolContext,
            args: Value,
        ) -> crate::server::tools::ToolActionFuture<'a> {
            Box::pin(async move {
                Ok(Some(vec![
                    args["message"]
                        .as_str()
                        .ok_or_else(|| {
                            crate::server::tools::ToolError::InvalidArgs(
                                "missing bound message".into(),
                            )
                        })?
                        .to_string(),
                ]))
            })
        }
    }

    #[tokio::test]
    async fn published_action_gate_uses_effective_bound_arguments_before_approval() {
        let registry = ToolRegistry::new().with(ActionEcho);
        let mut run = run();
        let spec = AgentSpec::from_value(&json!({"main":{"tool_resources":{"company_echo":{
            "bind":{"message":{"const":"write"}},"actions":{"read":"allow","write":"ask"}
        }}}}))
        .unwrap();
        run.binds = ToolBinds::from_spec(&spec);
        run.permissions = Permissions::from_spec(&spec);
        let db = aiplane_core::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        let ctx = ToolContext {
            suspend: crate::suspend::Suspend::Available,
            ..ToolContext::for_test(db)
        };
        let tool = run.source(&registry).get("company_echo").unwrap();
        let body = tool
            .run(ctx.clone(), json!({"message":"read"}))
            .await
            .unwrap();
        let pause = crate::suspend::extract_suspend(&body).unwrap();
        assert_eq!(pause.actions, Some(vec!["write".to_string()]));
        let mut resumed = ctx;
        resumed.suspend = crate::suspend::Suspend::Decided(
            session_core::db::SuspensionKind::Approval,
            session_core::db::Decision::AllowOnce,
        );
        resumed.pending_actions = Some(crate::server::tools::PendingActionFacts {
            actions: pause.actions,
        });
        assert_eq!(
            tool.run(resumed.clone(), json!({"message":"read"}))
                .await
                .unwrap(),
            json!({"message":"write"})
        );
        let changed = AgentSpec::from_value(&json!({"main":{"tool_resources":{"company_echo":{"bind":{"message":{"const":"read"}}}}}})).unwrap();
        run.binds = ToolBinds::from_spec(&changed);
        assert!(
            run.source(&registry)
                .get("company_echo")
                .unwrap()
                .run(resumed, json!({}))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn a_tool_leaving_a_subject_parameter_unbound_is_neither_offered_nor_run() {
        let registry = ToolRegistry::new().with(Echo);
        let run = run();
        let source = run.source(&registry);
        assert!(source.defs_for(&["company_echo".into()]).is_empty());

        let db = aiplane_core::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        let err = source
            .get("company_echo")
            .expect("still resolvable, so the call is answered")
            .run(ToolContext::for_test(db), json!({"message": "K-99999"}))
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("`message`"), "{err}");
        assert!(err.contains("not available"), "{err}");
    }
}
