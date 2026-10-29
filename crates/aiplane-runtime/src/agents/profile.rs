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

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use aiplane_agents::db::{agents as agents_db, system_principals as sp};
use aiplane_core::server::db::{DbError, Pool};
use aiplane_core::server::principal::{GrantKind, GrantSet, SystemPrincipal};
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
use crate::agent_run::{Actor, AgentRun, MismatchedRun, OrganizationTurnContext};
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
    organization: Option<(String, Arc<RamaState>)>,
    original_spec: Option<Value>,
    admission: OrganizationAdmission,
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

#[derive(Clone, Copy)]
enum ProfilePurpose {
    Generic,
    Private,
    /// A personal caller delegating to an organization agent: the receiver runs
    /// its published spec under its own principal's authority.
    Delegated,
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum OrganizationSpecSource {
    Draft,
    Published { version: i64 },
}

pub async fn organization_automation_version_in(
    state: &crate::server::AppState,
    tx: &mut aiplane_core::server::db::WriteTx,
    principal_id: &str,
    version: i64,
) -> Result<(Value, String), AgentRunError> {
    let unavailable = || AgentRunError::OrganizationExecutionUnavailable(principal_id.to_owned());
    if version <= 0
        || !matches!(
            agents_db::agent_owner_in(tx, principal_id).await?,
            Some(agents_db::AgentOwner::Organization(_))
        )
    {
        return Err(unavailable());
    }
    let row = sp::get_in(tx, principal_id)
        .await?
        .ok_or_else(unavailable)?;
    let grants = sp::grants_in(tx, principal_id).await?;
    let pools: Vec<_> = grants
        .iter()
        .filter(|grant| grant.kind == GrantKind::Model)
        .filter_map(|grant| {
            grant.pools.as_ref().map(|pools| {
                (
                    grant.reference.clone(),
                    pools.iter().cloned().collect::<BTreeSet<_>>(),
                )
            })
        })
        .collect();
    let principal = SystemPrincipal {
        id: row.id,
        name: row.name,
        grants: Arc::new(
            GrantSet::new(
                grants
                    .into_iter()
                    .map(|grant| (grant.kind, grant.reference)),
            )
            .with_model_pools(pools),
        ),
    };
    let version_row = agents_db::version_in(tx, principal_id, version)
        .await?
        .ok_or_else(unavailable)?;
    let spec: Value = serde_json::from_str(&version_row.spec).map_err(|_| unavailable())?;
    let compiled = CompiledSpec::compile(version, spec.clone());
    let parts = compiled.parts().map_err(|_| unavailable())?;
    let model = match parts.agent.main_model() {
        Some(model) => model.to_owned(),
        None => crate::server::model_choices::gateway_default_in(
            state,
            tx,
            aiplane_core::server::feature_defaults::Feature::Chat,
        )
        .await?
        .ok_or_else(unavailable)?,
    };
    if !principal.grants.has(GrantKind::Model, &model) {
        return Err(unavailable());
    }
    let access = PoolAccess::for_system_models(&principal, [model.as_str()]);
    if !crate::server::model_choices::offered_in(
        state,
        tx,
        aiplane_core::server::upstreams::PoolKind::Chat,
        &access,
    )
    .await?
    .iter()
    .any(|choice| choice.id == model)
    {
        return Err(unavailable());
    }
    Ok((spec, model))
}

struct ProfileMaterial {
    compiled: Arc<CompiledSpec>,
    principal: SystemPrincipal,
    organization: Option<(String, Arc<RamaState>)>,
    original_spec: Option<Value>,
    model: String,
    display: String,
    admission: OrganizationAdmission,
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
        Self::load_source(state, agent_id, source, role, options, None).await
    }

    /// [`Self::load`] for a personal caller delegating to an organization agent:
    /// the organization execution contract on the published spec, the
    /// receiver's own principal's grants, no person's authority.
    pub async fn load_delegated(
        state: &Arc<RamaState>,
        agent_id: &str,
        role: Role,
        options: &RunOptions,
    ) -> Result<Self, AgentRunError> {
        Self::load_source_for(
            state,
            agent_id,
            SpecSource::Live,
            role,
            options,
            None,
            ProfilePurpose::Delegated,
        )
        .await
    }

    pub(crate) async fn load_organization_resume(
        state: &Arc<RamaState>,
        authority: &OrganizationResumeAuthority,
        options: &RunOptions,
    ) -> Result<Self, AgentRunError> {
        Self::load_source(
            state,
            &authority.principal_id,
            authority.source(),
            Role::Main,
            options,
            Some(authority),
        )
        .await
    }

    pub(crate) async fn load_private(
        state: &Arc<RamaState>,
        agent_id: &str,
        source: SpecSource,
        options: &RunOptions,
    ) -> Result<Self, AgentRunError> {
        Self::load_source_for(
            state,
            agent_id,
            source,
            Role::Main,
            options,
            None,
            ProfilePurpose::Private,
        )
        .await
    }

    pub(crate) async fn load_scheduled(
        state: &Arc<RamaState>,
        run_id: &str,
        options: &RunOptions,
    ) -> Result<Self, AgentRunError> {
        let mut tx = aiplane_core::server::db::WriteTx::begin(&state.db).await?;
        let (action, run) = crate::server::scheduled::get_for_run_in(&mut tx, run_id)
            .await?
            .ok_or_else(|| AgentRunError::OrganizationExecutionUnavailable(run_id.to_owned()))?;
        if !action.enabled
            || action.reuse_conversation
            || action.personal_agent_id.is_some()
            || action.model.is_some()
            || run.configuration_generation.as_deref()
                != Some(action.configuration_generation.as_str())
            || run.agent_version != action.agent_version
            || run.status.is_some()
            || run.assistant_turn_id.is_some()
        {
            return Err(AgentRunError::OrganizationExecutionUnavailable(
                run_id.to_owned(),
            ));
        }
        let profile = Self::load_automation_in(
            state,
            &mut tx,
            &action.owner,
            action.agent_version,
            OrganizationAdmission::Scheduled {
                action_id: action.id,
                run_id: run.id,
                configuration_generation: action.configuration_generation,
            },
            run.tools_enabled && action.tools_enabled,
            options,
        )
        .await?;
        tx.commit().await?;
        Ok(profile)
    }

    pub async fn load_webhook_in(
        state: &Arc<RamaState>,
        tx: &mut aiplane_core::server::db::WriteTx,
        webhook_id: &str,
        run_id: &str,
    ) -> Result<Self, AgentRunError> {
        let unavailable = || AgentRunError::OrganizationExecutionUnavailable(webhook_id.to_owned());
        let (hook, run) = crate::server::webhooks::get_for_run_in(tx, run_id)
            .await?
            .ok_or_else(unavailable)?;
        if hook.id != webhook_id
            || hook.reuse_conversation
            || hook.synchronous
            || hook.personal_agent_id.is_some()
            || hook.model.is_some()
            || run.configuration_generation.as_deref()
                != Some(hook.configuration_generation.as_str())
            || run.agent_version != hook.agent_version
            || run.status.is_some()
            || run.assistant_turn_id.is_some()
        {
            return Err(unavailable());
        }
        Self::load_automation_in(
            state,
            tx,
            &hook.owner,
            hook.agent_version,
            OrganizationAdmission::Webhook {
                webhook_id: hook.id,
                run_id: run.id,
                configuration_generation: hook.configuration_generation,
            },
            run.tools_enabled && hook.tools_enabled,
            &RunOptions::default(),
        )
        .await
    }

    async fn load_automation_in(
        state: &Arc<RamaState>,
        tx: &mut aiplane_core::server::db::WriteTx,
        owner: &crate::server::background::AutomationOwner,
        version: Option<i64>,
        admission: OrganizationAdmission,
        tools_enabled: bool,
        options: &RunOptions,
    ) -> Result<Self, AgentRunError> {
        let crate::server::background::AutomationOwner::Principal(id) = owner else {
            return Err(AgentRunError::OrganizationExecutionUnavailable(
                String::new(),
            ));
        };
        let unavailable = || AgentRunError::OrganizationExecutionUnavailable(id.clone());
        let owned_agent = agents_db::get_agent_metadata_in(tx, id)
            .await?
            .filter(|owned_agent| owned_agent.active)
            .ok_or_else(unavailable)?;
        let agents_db::AgentOwner::Organization(organization_id) = owned_agent.owner else {
            return Err(unavailable());
        };
        let principal = sp::load_active_in(tx, id).await?.ok_or_else(unavailable)?;
        let version = version.filter(|v| *v > 0).ok_or_else(unavailable)?;
        let (spec, model) = organization_automation_version_in(state, tx, id, version).await?;
        let compiled = Arc::new(CompiledSpec::compile(version, spec.clone()));
        let parts = compiled.parts().map_err(|_| unavailable())?;
        let display = match parts.agent.profile.display() {
            Some(display) => display.to_owned(),
            None => sp::get_in(tx, id)
                .await?
                .map(|row| row.display)
                .filter(|display| !display.trim().is_empty())
                .unwrap_or_else(|| principal.name.clone()),
        };
        let mut profile = Self::assemble(
            state,
            Role::Main,
            options,
            ProfileMaterial {
                compiled,
                principal,
                organization: Some((organization_id, state.clone())),
                original_spec: Some(spec),
                model,
                display,
                admission,
            },
        )?;
        if !tools_enabled {
            let surface = Arc::get_mut(&mut profile.surface).expect("new profile owns surface");
            surface.tools.clear();
            surface.synthetic.clear();
        }
        Ok(profile)
    }

    async fn load_source(
        state: &Arc<RamaState>,
        agent_id: &str,
        source: SpecSource,
        role: Role,
        options: &RunOptions,
        authority: Option<&OrganizationResumeAuthority>,
    ) -> Result<Self, AgentRunError> {
        let purpose = if authority.is_some() {
            ProfilePurpose::Private
        } else {
            ProfilePurpose::Generic
        };
        Self::load_source_for(state, agent_id, source, role, options, authority, purpose).await
    }

    async fn load_source_for(
        state: &Arc<RamaState>,
        agent_id: &str,
        source: SpecSource,
        role: Role,
        options: &RunOptions,
        authority: Option<&OrganizationResumeAuthority>,
        purpose: ProfilePurpose,
    ) -> Result<Self, AgentRunError> {
        let mut original_spec = match &source {
            SpecSource::Draft(value) => Some(value.clone()),
            _ => None,
        };
        let organization = match agents_db::agent_owner(&state.db, agent_id).await? {
            Some(agents_db::AgentOwner::Organization(id))
                if matches!(&source, SpecSource::Draft(_))
                    || matches!(purpose, ProfilePurpose::Private | ProfilePurpose::Delegated) =>
            {
                Some((id, state.clone()))
            }
            Some(agents_db::AgentOwner::Organization(_)) => {
                return Err(AgentRunError::OrganizationExecutionUnavailable(
                    agent_id.to_owned(),
                ));
            }
            _ => None,
        };
        let current = sp::load_active(&state.db, agent_id)
            .await?
            .ok_or_else(|| AgentRunError::Unavailable(agent_id.to_string()))?;
        let principal = match authority {
            Some(saved) => {
                if organization.as_ref().map(|(id, _)| id.as_str())
                    != Some(saved.organization_id.as_str())
                    || saved.principal_id != agent_id
                {
                    return Err(saved.unavailable());
                }
                saved.initial_principal(current.name)?
            }
            None => current,
        };
        let compiled = if organization.is_some() {
            let (version, spec) = match &source {
                SpecSource::Draft(spec) => (agents_db::DRAFT_VERSION, spec.clone()),
                SpecSource::Live => {
                    let (version, text) = agents_db::live(&state.db, agent_id)
                        .await?
                        .ok_or_else(|| AgentRunError::NotLive(principal.name.clone()))?;
                    let spec = serde_json::from_str(&text).map_err(|_| {
                        AgentRunError::OrganizationExecutionUnavailable(agent_id.to_owned())
                    })?;
                    (version, spec)
                }
                SpecSource::Pinned(version) => {
                    let row = agents_db::version(&state.db, agent_id, *version)
                        .await?
                        .ok_or_else(|| {
                            AgentRunError::OrganizationExecutionUnavailable(agent_id.to_owned())
                        })?;
                    let spec = serde_json::from_str(&row.spec).map_err(|_| {
                        AgentRunError::OrganizationExecutionUnavailable(agent_id.to_owned())
                    })?;
                    (row.version, spec)
                }
            };
            if version < 0 || (!matches!(&source, SpecSource::Draft(_)) && version == 0) {
                return Err(AgentRunError::OrganizationExecutionUnavailable(
                    agent_id.to_owned(),
                ));
            }
            if let Some(saved) = authority
                && (saved.version() != Some(version) || saved.spec != spec)
            {
                return Err(saved.unavailable());
            }
            original_spec = Some(spec.clone());
            Arc::new(CompiledSpec::compile(version, spec))
        } else {
            match source {
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
            }
        };
        let parts = compiled.parts().map_err(|message| AgentRunError::BadSpec {
            agent: principal.name.clone(),
            version: compiled.version,
            message: message.to_string(),
        })?;
        let spec = &parts.agent;
        let model = match authority {
            Some(saved) => saved.model.clone(),
            None => super::defaults::main_model(state, spec)
                .await
                .ok_or_else(|| AgentRunError::NoModel {
                    agent: principal.name.clone(),
                })?,
        };
        let display = match spec.profile.display() {
            Some(display) => display.to_string(),
            None => sp::get(&state.db, agent_id)
                .await?
                .map(|row| row.display)
                .filter(|d| !d.trim().is_empty())
                .unwrap_or_else(|| principal.name.clone()),
        };
        let mut profile = Self::assemble(
            state,
            role,
            options,
            ProfileMaterial {
                compiled,
                principal,
                organization,
                original_spec,
                model,
                display,
                admission: authority
                    .map(|saved| saved.admission.clone())
                    .unwrap_or(OrganizationAdmission::PrivateChat),
            },
        )?;
        if let Some(saved) = authority {
            let enabled = match &saved.admission {
                OrganizationAdmission::PrivateChat => true,
                OrganizationAdmission::Scheduled { run_id, .. } => {
                    let mut tx = aiplane_core::server::db::WriteTx::begin(&state.db).await?;
                    let (action, run) = crate::server::scheduled::get_for_run_in(&mut tx, run_id)
                        .await?
                        .ok_or_else(|| saved.unavailable())?;
                    run.tools_enabled && action.tools_enabled
                }
                OrganizationAdmission::Webhook { run_id, .. } => {
                    let mut tx = aiplane_core::server::db::WriteTx::begin(&state.db).await?;
                    let (hook, run) = crate::server::webhooks::get_for_run_in(&mut tx, run_id)
                        .await?
                        .ok_or_else(|| saved.unavailable())?;
                    run.tools_enabled && hook.tools_enabled
                }
            };
            if !enabled {
                let surface = Arc::get_mut(&mut profile.surface).expect("new profile owns surface");
                surface.tools.clear();
                surface.synthetic.clear();
            }
        }
        if let Some(saved) = authority
            && saved.synthetic_tools != synthetic_dependencies(&profile.surface.synthetic)
        {
            return Err(saved.unavailable());
        }
        Ok(profile)
    }

    fn assemble(
        state: &Arc<RamaState>,
        role: Role,
        options: &RunOptions,
        material: ProfileMaterial,
    ) -> Result<Self, AgentRunError> {
        let ProfileMaterial {
            compiled,
            principal,
            organization,
            original_spec,
            model,
            display,
            admission,
        } = material;
        let version = compiled.version;
        let bad = |message: String| AgentRunError::BadSpec {
            agent: principal.name.clone(),
            version,
            message,
        };
        let parts = compiled.parts().map_err(|m| bad(m.to_string()))?;
        let spec = &parts.agent;
        let (schema, gates) = (parts.schema.clone(), parts.gates.clone());
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
        let mut add = |tool: Arc<dyn Tool>, phase: ToolPhase, dependency_tool_ids: Vec<String>| {
            synthetic.insert(
                tool.id().to_string(),
                Synthetic {
                    tool,
                    phase,
                    dependency_tool_ids,
                },
            );
        };
        let slot_tools = SlotTools::with_clock(schema.clone(), options.now.clone());
        for id in slot_tools.ids() {
            if let Some(tool) = slot_tools.get(&id) {
                add(tool, ToolPhase::WritesState, Vec::new());
            }
        }
        if matches!(role, Role::Main) {
            let run = VerifierRun {
                state: state.clone(),
                principal: principal.clone(),
                schema: schema.clone(),
                options: options.clone(),
            };
            for descriptor in verifier::tools(&Verifiers::from_spec(spec), &run) {
                add(
                    descriptor.tool,
                    ToolPhase::WritesState,
                    descriptor.dependency_tool_ids,
                );
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
                add(Arc::new(human), ToolPhase::ActsOnState, Vec::new());
            }
            let forward = ForwardRequest::new(state.clone(), router, options.clone());
            add(Arc::new(forward), ToolPhase::ActsOnState, Vec::new());
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
            organization,
            original_spec,
            admission,
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
        let mut run = self.agent_run(chain)?;
        if let Some((organization_id, state)) = &self.organization {
            run = run.with_organization(OrganizationTurnContext {
                organization_id: organization_id.clone(),
                session_id: session_id.to_owned(),
                assistant_turn_id: assistant_turn_id.to_owned(),
                state: state.clone(),
            });
            if let Some(draft) = &self.original_spec {
                let authority = OrganizationResumeAuthority::for_profile(
                    self,
                    draft.clone(),
                    organization_id.clone(),
                    session_id.to_owned(),
                    assistant_turn_id.to_owned(),
                );
                run = run.with_organization_resume(authority);
            }
        }
        Ok(DriveParams {
            actor: Actor::Agent(Arc::new(run)),
            session_id: session_id.to_string(),
            assistant_turn_id: assistant_turn_id.to_string(),
            model: self.model.clone(),
            source: match self.admission {
                OrganizationAdmission::PrivateChat => UsageSource::Agent,
                OrganizationAdmission::Scheduled { .. } => UsageSource::Scheduled,
                OrganizationAdmission::Webhook { .. } => UsageSource::Webhook,
            },
            history_limit: None,
        })
    }
}

const ORGANIZATION_RESUME_KEY: &str = "aiplane_organization_resume";

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct OrganizationGrant {
    kind: String,
    reference: String,
    pools: Option<BTreeSet<String>>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum OrganizationAdmission {
    PrivateChat,
    Scheduled {
        action_id: String,
        run_id: String,
        configuration_generation: String,
    },
    Webhook {
        webhook_id: String,
        run_id: String,
        configuration_generation: String,
    },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OrganizationResumeAuthority {
    pub(crate) principal_id: String,
    pub(crate) organization_id: String,
    pub(crate) session_id: String,
    pub(crate) assistant_turn_id: String,
    source: OrganizationSpecSource,
    admission: OrganizationAdmission,
    spec: Value,
    pub(crate) model: String,
    grants: Vec<OrganizationGrant>,
    pub(crate) synthetic_tools: BTreeMap<String, Vec<String>>,
}

fn synthetic_dependencies(
    synthetic: &BTreeMap<String, Synthetic>,
) -> BTreeMap<String, Vec<String>> {
    synthetic
        .iter()
        .map(|(id, tool)| (id.clone(), tool.dependency_tool_ids.clone()))
        .collect()
}

impl OrganizationResumeAuthority {
    fn for_profile(
        profile: &RunProfile,
        draft: Value,
        organization_id: String,
        session_id: String,
        assistant_turn_id: String,
    ) -> Self {
        Self {
            principal_id: profile.principal.id.clone(),
            organization_id,
            session_id,
            assistant_turn_id,
            source: if profile.version == agents_db::DRAFT_VERSION {
                OrganizationSpecSource::Draft
            } else {
                OrganizationSpecSource::Published {
                    version: profile.version,
                }
            },
            admission: profile.admission.clone(),
            spec: draft,
            model: profile.model.clone(),
            grants: profile
                .principal
                .grants
                .iter()
                .map(|(kind, reference)| OrganizationGrant {
                    kind: kind.as_str().to_owned(),
                    reference: reference.to_owned(),
                    pools: profile
                        .principal
                        .grants
                        .model_pools(reference)
                        .cloned()
                        .filter(|_| kind == GrantKind::Model),
                })
                .collect(),
            synthetic_tools: synthetic_dependencies(&profile.surface.synthetic),
        }
    }

    pub(crate) fn capture(run: &AgentRun) -> Option<Self> {
        run.organization_resume().cloned()
    }

    pub(crate) fn from_context(context: Option<&Value>) -> Option<Self> {
        let authority: Self =
            serde_json::from_value(context?.get(ORGANIZATION_RESUME_KEY)?.clone()).ok()?;
        authority.version()?;
        Some(authority)
    }

    pub(crate) fn attach_to(&self, context: Option<Value>) -> Result<Value, AgentRunError> {
        let mut context = match context {
            None => serde_json::Map::new(),
            Some(Value::Object(context)) if !context.contains_key(ORGANIZATION_RESUME_KEY) => {
                context
            }
            _ => return Err(self.unavailable()),
        };
        context.insert(
            ORGANIZATION_RESUME_KEY.to_owned(),
            serde_json::to_value(self).map_err(|_| self.unavailable())?,
        );
        Ok(Value::Object(context))
    }

    pub(crate) fn source(&self) -> SpecSource {
        match self.source {
            OrganizationSpecSource::Draft => SpecSource::Draft(self.spec.clone()),
            OrganizationSpecSource::Published { version } => SpecSource::Pinned(version),
        }
    }

    pub(crate) fn version(&self) -> Option<i64> {
        match self.source {
            OrganizationSpecSource::Draft => Some(0),
            OrganizationSpecSource::Published { version } if version > 0 => Some(version),
            _ => None,
        }
    }

    pub(crate) async fn validate_source_in(
        &self,
        tx: &mut aiplane_core::server::db::WriteTx,
    ) -> Result<i64, AgentRunError> {
        let version = self.version().ok_or_else(|| self.unavailable())?;
        if version > 0 {
            let row = agents_db::version_in(tx, &self.principal_id, version)
                .await?
                .ok_or_else(|| self.unavailable())?;
            let spec: Value = serde_json::from_str(&row.spec).map_err(|_| self.unavailable())?;
            if spec != self.spec {
                return Err(self.unavailable());
            }
        }
        if let OrganizationAdmission::Scheduled {
            action_id,
            run_id,
            configuration_generation,
        } = &self.admission
        {
            let Some((action, run)) = crate::server::scheduled::get_for_run_in(tx, run_id).await?
            else {
                return Err(self.unavailable());
            };
            if action.id != *action_id
                || action.owner
                    != crate::server::background::AutomationOwner::Principal(
                        self.principal_id.clone(),
                    )
                || action.agent_version != Some(version)
                || action.configuration_generation != *configuration_generation
                || run.configuration_generation.as_ref() != Some(configuration_generation)
                || run.agent_version != Some(version)
                || run.session_id.as_deref() != Some(self.session_id.as_str())
                || run.assistant_turn_id.as_deref() != Some(self.assistant_turn_id.as_str())
                || version <= 0
            {
                return Err(self.unavailable());
            }
        }
        if let OrganizationAdmission::Webhook {
            webhook_id,
            run_id,
            configuration_generation,
        } = &self.admission
        {
            let (hook, run) = crate::server::webhooks::get_for_run_in(tx, run_id)
                .await?
                .ok_or_else(|| self.unavailable())?;
            if hook.id != *webhook_id
                || hook.owner
                    != crate::server::background::AutomationOwner::Principal(
                        self.principal_id.clone(),
                    )
                || hook.agent_version != Some(version)
                || hook.configuration_generation != *configuration_generation
                || run.configuration_generation.as_ref() != Some(configuration_generation)
                || run.agent_version != Some(version)
                || run.session_id.as_deref() != Some(self.session_id.as_str())
                || run.assistant_turn_id.as_deref() != Some(self.assistant_turn_id.as_str())
                || version <= 0
            {
                return Err(self.unavailable());
            }
        }
        Ok(version)
    }

    fn unavailable(&self) -> AgentRunError {
        AgentRunError::OrganizationExecutionUnavailable(self.principal_id.clone())
    }

    fn initial_principal(&self, name: String) -> Result<SystemPrincipal, AgentRunError> {
        let mut grants = BTreeSet::new();
        let mut pools = Vec::new();
        if self.principal_id.is_empty()
            || self.organization_id.is_empty()
            || self.session_id.is_empty()
            || self.assistant_turn_id.is_empty()
            || self.model.is_empty()
        {
            return Err(self.unavailable());
        }
        for grant in &self.grants {
            let kind = GrantKind::parse(&grant.kind).ok_or_else(|| self.unavailable())?;
            if grant.reference.is_empty()
                || !grants.insert((kind, grant.reference.clone()))
                || (kind != GrantKind::Model && grant.pools.is_some())
            {
                return Err(self.unavailable());
            }
            if let Some(allowed) = &grant.pools {
                pools.push((grant.reference.clone(), allowed.clone()));
            }
        }
        Ok(SystemPrincipal {
            id: self.principal_id.clone(),
            name,
            grants: Arc::new(GrantSet::new(grants).with_model_pools(pools)),
        })
    }
}

pub(crate) async fn validate_organization_resume_in(
    tx: &mut aiplane_core::server::db::WriteTx,
    authority: &OrganizationResumeAuthority,
) -> Result<SystemPrincipal, AgentRunError> {
    let version = authority.validate_source_in(tx).await?;
    let compiled = CompiledSpec::compile(version, authority.spec.clone());
    compiled.parts().map_err(|_| authority.unavailable())?;
    let initial = authority.initial_principal(String::new())?;
    let current = validate_organization_authority_in(
        tx,
        &initial,
        &authority.organization_id,
        &authority.session_id,
        &authority.assistant_turn_id,
        version,
        OrganizationTurnState::Live,
    )
    .await?;
    if !current.grants.has(GrantKind::Model, &authority.model) {
        return Err(authority.unavailable());
    }
    Ok(current)
}

pub async fn private_session_spec_in(
    tx: &mut aiplane_core::server::db::WriteTx,
    principal_id: &str,
    session_id: &str,
    version: i64,
) -> Result<Option<(super::run::draft::PrivateChatSource, Value)>, AgentRunError> {
    use super::run::draft::PrivateChatSource;
    if aiplane_agents::db::run_sessions::get_private_principal_session_in(
        tx,
        principal_id,
        session_id,
        version,
    )
    .await?
    .is_none()
    {
        return Ok(None);
    }
    let mut captured_spec = None;
    let source = if version == 0 {
        PrivateChatSource::Draft
    } else {
        let Some(row) = agents_db::version_in(tx, principal_id, version).await? else {
            return Ok(None);
        };
        let compiled = CompiledSpec::parse(version, &row.spec);
        if compiled.parts().is_err() {
            return Ok(None);
        }
        let Ok(spec) = serde_json::from_str::<Value>(&row.spec) else {
            return Ok(None);
        };
        captured_spec = Some(spec);
        PrivateChatSource::Published
    };
    let turns: Vec<String> = sqlx::query_scalar("SELECT t.id FROM chat_turns t JOIN chat_turn_suspensions p ON p.turn_id=t.id WHERE t.session_id=? AND t.role='assistant'")
        .bind(session_id).fetch_all(&mut **tx).await.map_err(DbError::from)?;
    for turn in turns {
        let Some(pause) = session_core::db::get_suspension_in(tx, &turn)
            .await
            .map_err(DbError::from)?
        else {
            return Ok(None);
        };
        let Some(authority) = OrganizationResumeAuthority::from_context(pause.run_context.as_ref())
        else {
            return Ok(None);
        };
        let owner = agents_db::agent_owner_in(tx, principal_id).await?;
        if authority.principal_id != principal_id
            || authority.session_id != session_id
            || authority.assistant_turn_id != turn
            || authority.version() != Some(version)
            || owner
                != Some(agents_db::AgentOwner::Organization(
                    authority.organization_id.clone(),
                ))
        {
            return Ok(None);
        }
        match authority.validate_source_in(tx).await {
            Ok(_) => {}
            Err(AgentRunError::Db(error)) => return Err(AgentRunError::Db(error)),
            Err(_) => return Ok(None),
        }
        if captured_spec
            .as_ref()
            .is_some_and(|spec| spec != &authority.spec)
        {
            return Ok(None);
        }
        captured_spec = Some(authority.spec);
    }
    let spec = match captured_spec {
        Some(spec) => spec,
        None => {
            let Some(row) = agents_db::get_in(tx, principal_id).await? else {
                return Ok(None);
            };
            let Ok(spec) = serde_json::from_str::<Value>(&row.draft_spec) else {
                return Ok(None);
            };
            spec
        }
    };
    Ok(Some((source, spec)))
}

pub async fn validate_organization_turn_in(
    tx: &mut aiplane_core::server::db::WriteTx,
    run: &AgentRun,
) -> Result<SystemPrincipal, AgentRunError> {
    validate_organization_context_in(tx, run, OrganizationTurnState::Live).await
}

pub async fn validate_organization_compaction_in(
    tx: &mut aiplane_core::server::db::WriteTx,
    run: &AgentRun,
) -> Result<SystemPrincipal, AgentRunError> {
    validate_organization_context_in(tx, run, OrganizationTurnState::Compaction).await
}

enum OrganizationTurnState {
    Live,
    Compaction,
}

async fn validate_organization_context_in(
    tx: &mut aiplane_core::server::db::WriteTx,
    run: &AgentRun,
    admission: OrganizationTurnState,
) -> Result<SystemPrincipal, AgentRunError> {
    let initial = run.system_principal();
    let unavailable = || AgentRunError::OrganizationExecutionUnavailable(initial.id.clone());
    let context = run.organization().ok_or_else(unavailable)?;
    let authority = run.organization_resume().ok_or_else(unavailable)?;
    if authority.principal_id != initial.id
        || authority.organization_id != context.organization_id
        || authority.session_id != context.session_id
        || authority.assistant_turn_id != context.assistant_turn_id
    {
        return Err(unavailable());
    }
    let version = authority.validate_source_in(tx).await?;
    validate_organization_authority_in(
        tx,
        initial,
        &context.organization_id,
        &context.session_id,
        &context.assistant_turn_id,
        version,
        admission,
    )
    .await
}

async fn validate_organization_authority_in(
    tx: &mut aiplane_core::server::db::WriteTx,
    initial: &SystemPrincipal,
    organization_id: &str,
    session_id: &str,
    assistant_turn_id: &str,
    version: i64,
    admission: OrganizationTurnState,
) -> Result<SystemPrincipal, AgentRunError> {
    let unavailable = || AgentRunError::OrganizationExecutionUnavailable(initial.id.clone());
    let owned_agent = agents_db::get_agent_metadata_in(tx, &initial.id)
        .await?
        .ok_or_else(unavailable)?;
    if !owned_agent.active
        || owned_agent.owner != agents_db::AgentOwner::Organization(organization_id.to_owned())
        || aiplane_agents::db::organizations::get_admin_in(tx, organization_id)
            .await?
            .is_none()
    {
        return Err(unavailable());
    }
    let mut current = sp::load_active_in(tx, &initial.id)
        .await?
        .ok_or_else(unavailable)?;
    let valid: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM chat_sessions s JOIN chat_turns t ON t.session_id=s.id \
         WHERE s.id=? AND s.principal_id=? AND s.user_id IS NULL AND s.shared=0 \
         AND s.agent_version=? AND s.visitor_id IS NULL \
         AND (s.parent_turn_id IS NULL OR EXISTS (
             SELECT 1 FROM cross_boundary_delegations cb WHERE cb.child_session_id = s.id)) \
         AND t.id=? AND t.role='assistant' AND (t.status=? OR t.status=?)",
    )
    .bind(session_id)
    .bind(&initial.id)
    .bind(version)
    .bind(assistant_turn_id)
    .bind(match admission {
        OrganizationTurnState::Live => "in_progress",
        OrganizationTurnState::Compaction => "completed",
    })
    .bind(match admission {
        OrganizationTurnState::Live => "suspended",
        OrganizationTurnState::Compaction => "completed",
    })
    .fetch_one(&mut **tx)
    .await
    .map_err(DbError::from)?;
    if valid != 1 {
        return Err(unavailable());
    }
    let grants = GrantSet::new(
        initial
            .grants
            .iter()
            .filter(|(kind, reference)| current.grants.has(*kind, reference))
            .map(|(kind, reference)| (kind, reference.to_owned())),
    );
    let pools = grants
        .refs(GrantKind::Model)
        .filter_map(|model| {
            let initial_pools = initial.grants.model_pools(model);
            let current_pools = current.grants.model_pools(model);
            let pools = match (initial_pools, current_pools) {
                (Some(a), Some(b)) => a.intersection(b).cloned().collect(),
                (Some(a), None) => a.clone(),
                (None, Some(b)) => b.clone(),
                (None, None) => return None,
            };
            Some((model.to_owned(), pools))
        })
        .collect::<Vec<_>>();
    current.grants = Arc::new(grants.with_model_pools(pools));
    Ok(current)
}

pub async fn validate_organization_turn(
    state: &crate::server::AppState,
    run: &AgentRun,
) -> Result<SystemPrincipal, AgentRunError> {
    let mut tx = aiplane_core::server::db::WriteTx::begin(&state.db).await?;
    let principal = validate_organization_turn_in(&mut tx, run).await?;
    tx.commit().await?;
    Ok(principal)
}

pub async fn validate_organization_compaction(
    state: &crate::server::AppState,
    run: &AgentRun,
) -> Result<SystemPrincipal, AgentRunError> {
    let mut tx = aiplane_core::server::db::WriteTx::begin(&state.db).await?;
    let principal = validate_organization_compaction_in(&mut tx, run).await?;
    tx.commit().await?;
    Ok(principal)
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
    dependency_tool_ids: Vec<String>,
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
            agent: None,
        }
    }
}

pub fn render_owner_brief(display: &str, spec: &AgentSpec) -> String {
    Brief {
        display: display.into(),
        instructions: spec.main.instructions.clone(),
        scope: spec.scope.clone(),
    }
    .render()
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
    agent: Option<&'a AgentRun>,
}

impl<'a> RunToolSource<'a> {
    pub fn new(inner: &'a dyn ToolSource, run: Option<&'a AgentRun>) -> Self {
        Self {
            inner,
            run: run.and_then(AgentRun::surface),
            terminal: run.and_then(AgentRun::terminal_tool),
            agent: run,
        }
    }

    fn terminal(&self, id: &str) -> Option<&Arc<dyn Tool>> {
        self.terminal.as_ref().filter(|t| t.id() == id)
    }

    fn admitted(
        &self,
        tool: Arc<dyn Tool>,
        dependencies: &[String],
        grant_required: bool,
    ) -> Arc<dyn Tool> {
        let Some(run) = self.agent else {
            return tool;
        };
        let Some(context) = run.organization() else {
            return tool;
        };
        let unavailable = || {
            AgentRunError::OrganizationExecutionUnavailable(run.system_principal().id.clone())
                .to_string()
        };
        let facts =
            dependencies
                .iter()
                .filter(|id| id.starts_with(crate::server::tools::mcp::MCP_ID_PREFIX))
                .try_fold(None, |selected: Option<Value>, id| {
                    let facts = self
                        .inner
                        .get(id)
                        .and_then(|tool| tool.execution_facts())
                        .ok_or_else(unavailable)?;
                    if !matches!(
                        serde_json::from_value::<
                            crate::server::tools::mcp::identities::McpDispatchFacts,
                        >(facts.clone()),
                        Ok(crate::server::tools::mcp::identities::McpDispatchFacts::Owned(_))
                    ) {
                        return Err(unavailable());
                    }
                    if selected.as_ref().is_some_and(|previous| previous != &facts) {
                        return Err(unavailable());
                    }
                    Ok(Some(facts))
                })
                .map(|facts| facts.or_else(|| tool.execution_facts()));
        Arc::new(OrganizationTool {
            inner: tool,
            principal: run.system_principal().clone(),
            session_id: context.session_id.clone(),
            assistant_turn_id: context.assistant_turn_id.clone(),
            organization_id: context.organization_id.clone(),
            facts,
            grant_required,
            dependency_tool_ids: dependencies.to_vec(),
        })
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
        let gate = |tool| {
            if self
                .agent
                .is_some_and(|agent| agent.organization().is_some())
            {
                run.permissions.gate_agent(tool)
            } else {
                run.permissions.gate(tool)
            }
        };
        let bound: Arc<dyn Tool> = match run.binds.for_tool(tool.id(), &tool.schema()) {
            Ok(binds) if binds.is_empty() => gate(tool),
            Ok(binds) => Arc::new(BoundTool::new(
                gate(tool),
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
            return Some(self.admitted(terminal.clone(), &[], false));
        }
        let Some(run) = self.run else {
            return self.inner.get(id);
        };
        if let Some(synthetic) = run.synthetic.get(id) {
            return Some(self.admitted(
                synthetic.tool.clone(),
                &synthetic.dependency_tool_ids,
                false,
            ));
        }
        self.inner
            .get(id)
            .map(|t| self.admitted(self.bound(run, t), &[], true))
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

struct OrganizationTool {
    inner: Arc<dyn Tool>,
    principal: SystemPrincipal,
    organization_id: String,
    session_id: String,
    assistant_turn_id: String,
    facts: Result<Option<Value>, String>,
    grant_required: bool,
    dependency_tool_ids: Vec<String>,
}

impl OrganizationTool {
    async fn admit(
        &self,
        ctx: &crate::server::tools::ToolContext,
    ) -> Result<Option<Value>, crate::server::tools::ToolError> {
        use crate::server::tools::{
            ToolError,
            mcp::identities::{McpDispatchFacts, validate_organization_identity_in},
        };
        let refused = || {
            ToolError::Failed(
                AgentRunError::OrganizationExecutionUnavailable(self.principal.id.clone())
                    .to_string(),
            )
        };
        let run = ctx.agent.as_ref().ok_or_else(refused)?;
        let context = run.organization().ok_or_else(refused)?;
        if ctx.principal.subject_id() != self.principal.id
            || run.system_principal().id != self.principal.id
            || !Arc::ptr_eq(&run.system_principal().grants, &self.principal.grants)
            || context.organization_id != self.organization_id
            || context.session_id != self.session_id
            || context.assistant_turn_id != self.assistant_turn_id
            || ctx.session_id.as_deref() != Some(self.session_id.as_str())
            || ctx.assistant_turn_id.as_deref() != Some(self.assistant_turn_id.as_str())
        {
            return Err(refused());
        }
        let facts = self.facts.clone().map_err(ToolError::Failed)?;
        if let Some(pending) = &ctx.pending_execution_facts
            && pending.execution_facts != facts
        {
            return Err(refused());
        }
        let mut tx = aiplane_core::server::db::WriteTx::begin(&ctx.db)
            .await
            .map_err(|error| ToolError::Failed(AgentRunError::Db(error).to_string()))?;
        let current = validate_organization_turn_in(&mut tx, run)
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        for dependency in &self.dependency_tool_ids {
            if let Some(rest) = dependency.strip_prefix(crate::server::tools::mcp::MCP_ID_PREFIX) {
                let (connector, _) = rest.split_once("__").ok_or_else(refused)?;
                if !current.grants.has(GrantKind::Connector, connector) {
                    return Err(refused());
                }
            } else if !current.grants.has(GrantKind::Tool, dependency) {
                return Err(refused());
            }
        }
        match facts.as_ref() {
            Some(facts) => match serde_json::from_value::<McpDispatchFacts>(facts.clone()) {
                Ok(McpDispatchFacts::Owned(identity)) => {
                    validate_organization_identity_in(&mut tx, ctx, &identity).await?
                }
                Ok(McpDispatchFacts::Shared { .. }) => return Err(refused()),
                Err(_)
                    if self.grant_required
                        && !current.grants.has(GrantKind::Tool, self.inner.id()) =>
                {
                    return Err(refused());
                }
                Err(_) => {}
            },
            None if self.grant_required
                && !current.grants.has(GrantKind::Tool, self.inner.id()) =>
            {
                return Err(refused());
            }
            None => {}
        }
        tx.commit()
            .await
            .map_err(|error| ToolError::Failed(AgentRunError::Db(error).to_string()))?;
        Ok(facts)
    }
}

impl Tool for OrganizationTool {
    fn id(&self) -> &str {
        self.inner.id()
    }
    fn schema(&self) -> ToolDef {
        self.inner.schema()
    }
    fn execution_facts(&self) -> Option<Value> {
        self.facts.as_ref().ok().cloned().flatten()
    }
    fn phase(&self) -> ToolPhase {
        self.inner.phase()
    }
    fn action_catalogue(&self) -> Option<&'static [crate::server::tools::ToolAction]> {
        self.inner.action_catalogue()
    }
    fn classify_actions<'a>(
        &'a self,
        ctx: crate::server::tools::ToolContext,
        args: Value,
    ) -> crate::server::tools::ToolActionFuture<'a> {
        Box::pin(async move {
            self.admit(&ctx).await?;
            self.inner.classify_actions(ctx, args).await
        })
    }
    fn run<'a>(
        &'a self,
        mut ctx: crate::server::tools::ToolContext,
        args: Value,
    ) -> crate::server::tools::ToolFuture<'a> {
        Box::pin(async move {
            let facts = self.admit(&ctx).await?;
            if facts.is_some() && ctx.pending_execution_facts.is_none() {
                ctx.pending_execution_facts = Some(crate::server::tools::PendingExecutionFacts {
                    execution_facts: facts.clone(),
                });
            }
            let body = self.inner.run(ctx, args).await?;
            if let Some(mut suspension) = crate::suspend::extract_suspend(&body) {
                if suspension
                    .execution_facts
                    .as_ref()
                    .is_some_and(|existing| Some(existing) != facts.as_ref())
                {
                    return Err(crate::server::tools::ToolError::Failed(
                        AgentRunError::OrganizationExecutionUnavailable(self.principal.id.clone())
                            .to_string(),
                    ));
                }
                suspension.execution_facts = facts;
                return Ok(crate::suspend::tool_suspend(suspension));
            }
            Ok(body)
        })
    }
    fn max_duration(&self) -> Option<std::time::Duration> {
        self.inner.max_duration()
    }
    fn sensitive_args(&self) -> bool {
        self.inner.sensitive_args()
    }
    fn changes_state(&self) -> bool {
        self.inner.changes_state()
    }
    fn read_only(&self) -> bool {
        self.inner.read_only()
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
                dependency_tool_ids: Vec::new(),
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
        render_owner_brief("croit Support", &spec)
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
    #[tokio::test]
    async fn organization_resume_authority_round_trips_original_profile_and_preserves_other_context()
     {
        let server = wiremock::MockServer::start().await;
        let state = crate::server::headless::tests::state_for(&server.uri()).await;
        let (profile, params) =
            crate::server::headless::tests::organization_draft_fixture(&state).await;
        let crate::agent_run::Actor::Agent(run) = params.actor else {
            panic!("agent expected")
        };
        let authority = super::OrganizationResumeAuthority::capture(&run).unwrap();
        let context = authority.attach_to(Some(serde_json::json!({"route_binds":{"customer":"actual"},"handoff":{"question":"Actual question"}}))).unwrap();
        assert_eq!(context["route_binds"]["customer"], "actual");
        assert_eq!(context["handoff"]["question"], "Actual question");
        let restored = super::OrganizationResumeAuthority::from_context(Some(&context)).unwrap();
        assert_eq!(restored.model, profile.model);
        assert_eq!(restored.spec, profile.original_spec.clone().unwrap());
        assert_eq!(
            restored.synthetic_tools,
            super::synthetic_dependencies(&profile.surface.synthetic)
        );
        let principal = restored
            .initial_principal(profile.principal.name.clone())
            .unwrap();
        assert_eq!(principal.grants, profile.principal.grants);
        assert!(authority.attach_to(Some(context.clone())).is_err());
        assert!(
            authority
                .attach_to(Some(serde_json::json!("untrusted context")))
                .is_err()
        );
        let mut malformed = context;
        malformed[super::ORGANIZATION_RESUME_KEY]["unknown_field"] = serde_json::json!(true);
        assert!(super::OrganizationResumeAuthority::from_context(Some(&malformed)).is_none());
        assert!(super::OrganizationResumeAuthority::from_context(None).is_none());
    }

    #[tokio::test]
    async fn organization_resume_authority_retains_initial_model_pool_and_grant_ceiling() {
        use aiplane_core::server::principal::{GrantKind, GrantSet};
        let server = wiremock::MockServer::start().await;
        let state = crate::server::headless::tests::state_for(&server.uri()).await;
        let (mut profile, params) =
            crate::server::headless::tests::organization_draft_fixture(&state).await;
        let model = profile.model.clone();
        profile.principal.grants = std::sync::Arc::new(
            GrantSet::new(
                profile
                    .principal
                    .grants
                    .iter()
                    .map(|(kind, reference)| (kind, reference.to_owned())),
            )
            .with_model_pools([(
                model.clone(),
                ["initial-pool".to_owned()].into_iter().collect(),
            )]),
        );
        let crate::agent_run::Actor::Agent(original) = params.actor else {
            panic!("agent expected")
        };
        let context = original.organization().unwrap();
        let rebuilt = profile
            .drive_params(
                &context.session_id,
                &context.assistant_turn_id,
                original.chain().clone(),
            )
            .unwrap();
        let crate::agent_run::Actor::Agent(run) = rebuilt.actor else {
            panic!("agent expected")
        };
        let authority = super::OrganizationResumeAuthority::capture(&run).unwrap();
        aiplane_agents::db::system_principals::add_grant(
            &state.db,
            &profile.principal.id,
            GrantKind::Tool,
            "get_current_timestamp",
            "creator",
        )
        .await
        .unwrap();
        let mut tx = aiplane_core::server::db::WriteTx::begin(&state.db)
            .await
            .unwrap();
        let current = super::validate_organization_resume_in(&mut tx, &authority)
            .await
            .unwrap();
        assert!(!current.grants.has(GrantKind::Tool, "get_current_timestamp"));
        assert_eq!(
            current.grants.model_pools(&model),
            Some(&["initial-pool".to_owned()].into_iter().collect())
        );
        drop(tx);
        let restored =
            super::RunProfile::load_organization_resume(&state, &authority, &Default::default())
                .await
                .unwrap();
        assert_eq!(restored.principal.grants, profile.principal.grants);
        assert_eq!(restored.model, model);
    }

    #[tokio::test]
    async fn organization_resume_loader_keeps_the_original_resolved_default_model() {
        use aiplane_core::server::feature_defaults::{self, Feature};
        let server = wiremock::MockServer::start().await;
        let state = crate::server::headless::tests::state_for(&server.uri()).await;
        let (profile, params) =
            crate::server::headless::tests::organization_draft_fixture(&state).await;
        let model = profile.model.clone();
        feature_defaults::set(&state.db, Feature::Chat, Some(&model))
            .await
            .unwrap();
        let mut draft = profile.original_spec.clone().unwrap();
        draft["main"].as_object_mut().unwrap().remove("model");
        let profile = super::RunProfile::load_from(
            &state,
            &profile.principal.id,
            super::SpecSource::Draft(draft),
            super::Role::Main,
            &Default::default(),
        )
        .await
        .unwrap();
        let crate::agent_run::Actor::Agent(original) = params.actor else {
            panic!("agent expected")
        };
        let context = original.organization().unwrap();
        let rebuilt = profile
            .drive_params(
                &context.session_id,
                &context.assistant_turn_id,
                original.chain().clone(),
            )
            .unwrap();
        let crate::agent_run::Actor::Agent(run) = rebuilt.actor else {
            panic!("agent expected")
        };
        let authority = super::OrganizationResumeAuthority::capture(&run).unwrap();
        for pool in state.upstreams.pools() {
            pool.backends[0].set_models([model.clone(), "replacement-model".to_owned()].into());
        }
        feature_defaults::set(&state.db, Feature::Chat, Some("replacement-model"))
            .await
            .unwrap();
        let spec = crate::agents::spec::AgentSpec::from_value(&authority.spec).unwrap();
        assert_eq!(
            crate::agents::defaults::main_model(&state, &spec)
                .await
                .as_deref(),
            Some("replacement-model")
        );
        let restored =
            super::RunProfile::load_organization_resume(&state, &authority, &Default::default())
                .await
                .unwrap();
        assert_eq!(restored.model, model);
        assert_eq!(restored.principal.grants, profile.principal.grants);
    }
    #[tokio::test]
    async fn organization_authority_requires_explicit_source_and_authored_snapshot() {
        let server = wiremock::MockServer::start().await;
        let state = crate::server::headless::tests::state_for(&server.uri()).await;
        let (_, params) = crate::server::headless::tests::organization_draft_fixture(&state).await;
        let crate::agent_run::Actor::Agent(run) = params.actor else {
            panic!("agent expected")
        };
        let authority = super::OrganizationResumeAuthority::capture(&run).unwrap();
        let context = authority.attach_to(None).unwrap();
        assert_eq!(
            context[super::ORGANIZATION_RESUME_KEY]["source"],
            serde_json::json!({"kind":"draft"})
        );
        assert!(context[super::ORGANIZATION_RESUME_KEY]["spec"].is_object());
        for field in ["source", "spec"] {
            let mut missing = context.clone();
            missing[super::ORGANIZATION_RESUME_KEY]
                .as_object_mut()
                .unwrap()
                .remove(field);
            assert!(
                super::OrganizationResumeAuthority::from_context(Some(&missing)).is_none(),
                "{field}"
            );
        }
        let mut malformed = context;
        malformed[super::ORGANIZATION_RESUME_KEY]["source"] =
            serde_json::json!({"kind":"published","version":0});
        assert!(super::OrganizationResumeAuthority::from_context(Some(&malformed)).is_none());
    }
    #[tokio::test]
    async fn organization_authority_requires_explicit_private_or_scheduled_admission() {
        let server = wiremock::MockServer::start().await;
        let state = crate::server::headless::tests::state_for(&server.uri()).await;
        let (_, params) = crate::server::headless::tests::organization_draft_fixture(&state).await;
        let crate::agent_run::Actor::Agent(run) = params.actor else {
            panic!("principal expected");
        };
        let authority = super::OrganizationResumeAuthority::capture(&run).unwrap();
        let context = authority.attach_to(None).unwrap();
        assert_eq!(
            context[super::ORGANIZATION_RESUME_KEY]["admission"],
            serde_json::json!({"kind":"private_chat"})
        );
        let mut missing = context.clone();
        missing[super::ORGANIZATION_RESUME_KEY]
            .as_object_mut()
            .unwrap()
            .remove("admission");
        assert!(super::OrganizationResumeAuthority::from_context(Some(&missing)).is_none());
        let mut unknown = context.clone();
        unknown[super::ORGANIZATION_RESUME_KEY]["admission"] =
            serde_json::json!({"kind":"employee"});
        assert!(super::OrganizationResumeAuthority::from_context(Some(&unknown)).is_none());
        let mut fabricated = context;
        fabricated[super::ORGANIZATION_RESUME_KEY]["admission"] = serde_json::json!({"kind":"scheduled","action_id":"missing-action","run_id":"missing-run","configuration_generation":"missing-generation"});
        let saved = super::OrganizationResumeAuthority::from_context(Some(&fabricated)).unwrap();
        let mut tx = aiplane_core::server::db::WriteTx::begin(&state.db)
            .await
            .unwrap();
        assert!(
            super::validate_organization_resume_in(&mut tx, &saved)
                .await
                .is_err()
        );
        drop(tx);
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn organization_webhook_profile_opens_and_attaches_uncommitted_run_without_another_connection()
     {
        let server = wiremock::MockServer::start().await;
        let state = crate::server::headless::tests::state_for(&server.uri()).await;
        let (original, _) =
            crate::server::headless::tests::organization_draft_fixture(&state).await;
        let mut tx = aiplane_core::server::db::WriteTx::begin(&state.db)
            .await
            .unwrap();
        let version = agents_db::publish_in(
            &mut tx,
            &original.principal.id,
            &original.original_spec.as_ref().unwrap().to_string(),
            "creator",
        )
        .await
        .unwrap()
        .unwrap();
        let now = jiff::Timestamp::now().to_string();
        sqlx::query("INSERT INTO webhooks(id,principal_id,agent_version,configuration_generation,name,prompt,model,tools_enabled,secret_hash,created_at,updated_at,enabled) VALUES('atomic-hook',?,?,'original','event','prompt',NULL,1,'atomic-secret',?,?,0)")
            .bind(&original.principal.id).bind(version).bind(&now).bind(&now).execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO webhook_runs(id,webhook_id,fired_at,prompt,payload,source,configuration_generation,agent_version,tools_enabled,created_at) VALUES('atomic-run','atomic-hook',?,'prompt','payload','rerun','original',?,1,?)")
            .bind(&now).bind(version).bind(&now).execute(&mut *tx).await.unwrap();
        let mut held = Vec::new();
        let max = state.db.options().get_max_connections();
        assert!(max <= 8);
        for _ in 1..max {
            held.push(state.db.acquire().await.unwrap());
        }
        let profile = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            RunProfile::load_webhook_in(&state, &mut tx, "atomic-hook", "atomic-run"),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(profile.version, version);
        assert_eq!(profile.model, original.model);
        let (session, turn) = crate::server::headless::open_principal_session_in(
            &mut tx,
            crate::server::headless::OpenParams {
                owner: crate::server::headless::Owner::Run {
                    principal_id: &profile.principal.id,
                    parent_turn_id: None,
                    agent_version: Some(version),
                },
                title: "event",
                prompt: "prompt",
                model: &profile.model,
                existing_session: None,
                personal_agent_id: None,
            },
        )
        .await
        .unwrap();
        crate::server::webhooks::attach_run_turn_in(
            &mut tx,
            "atomic-run",
            &session,
            &turn,
            true,
            None,
        )
        .await
        .unwrap();
        let chain = Arc::new(RunChain::root(
            &session,
            None,
            aiplane_core::server::run_chain::Frame::for_principal(&profile.principal, None),
        ));
        let params = profile.drive_params(&session, &turn, chain).unwrap();
        assert_eq!(params.source, UsageSource::Webhook);
        let Actor::Agent(run) = params.actor else {
            panic!("principal expected")
        };
        super::validate_organization_turn_in(&mut tx, &run)
            .await
            .unwrap();
        let saved = OrganizationResumeAuthority::capture(&run).unwrap();
        sqlx::query("UPDATE chat_turns SET status='suspended' WHERE id=?")
            .bind(&turn)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("UPDATE webhooks SET secret_hash='rotated',enabled=0 WHERE id='atomic-hook'")
            .execute(&mut *tx)
            .await
            .unwrap();
        saved.validate_source_in(&mut tx).await.unwrap();
        super::validate_organization_resume_in(&mut tx, &saved)
            .await
            .unwrap();
        let mut replacement = original.original_spec.clone().unwrap();
        replacement["main"]["instructions"] =
            serde_json::json!({"orchestration":"replacement authored instructions"});
        let newer = agents_db::publish_in(
            &mut tx,
            &original.principal.id,
            &replacement.to_string(),
            "creator",
        )
        .await
        .unwrap()
        .unwrap();
        assert!(newer > version);
        assert_eq!(saved.validate_source_in(&mut tx).await.unwrap(), version);
        assert_eq!(saved.spec, original.original_spec.clone().unwrap());
        sqlx::query("UPDATE webhooks SET configuration_generation='edited' WHERE id='atomic-hook'")
            .execute(&mut *tx)
            .await
            .unwrap();
        assert!(
            super::validate_organization_resume_in(&mut tx, &saved)
                .await
                .is_err()
        );
        sqlx::query(
            "UPDATE webhooks SET configuration_generation='original' WHERE id='atomic-hook'",
        )
        .execute(&mut *tx)
        .await
        .unwrap();
        for mutation in [
            "UPDATE agent_owners SET active=0 WHERE principal_id=?",
            "UPDATE system_principals SET disabled_at='2026-01-01T00:00:00Z' WHERE id=?",
        ] {
            sqlx::query(mutation)
                .bind(&original.principal.id)
                .execute(&mut *tx)
                .await
                .unwrap();
            assert!(
                super::validate_organization_resume_in(&mut tx, &saved)
                    .await
                    .is_err(),
                "{mutation}"
            );
            sqlx::query("UPDATE agent_owners SET active=1 WHERE principal_id=?")
                .bind(&original.principal.id)
                .execute(&mut *tx)
                .await
                .unwrap();
            sqlx::query("UPDATE system_principals SET disabled_at=NULL WHERE id=?")
                .bind(&original.principal.id)
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        sp::remove_grant_in(
            &mut tx,
            &original.principal.id,
            GrantKind::Model,
            &original.model,
            "creator",
        )
        .await
        .unwrap();
        assert!(
            super::validate_organization_resume_in(&mut tx, &saved)
                .await
                .is_err()
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT status FROM chat_turns WHERE id=?")
                .bind(&turn)
                .fetch_one(&mut *tx)
                .await
                .unwrap(),
            "suspended"
        );
        drop(tx);
        drop(held);
        assert!(server.received_requests().await.unwrap().is_empty());
        assert!(
            !sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM webhook_runs WHERE id='atomic-run')"
            )
            .fetch_one(&state.db)
            .await
            .unwrap()
        );
    }

    #[tokio::test]
    async fn organization_schedule_configuration_uses_current_grants_without_execution_activation()
    {
        let server = wiremock::MockServer::start().await;
        let state = crate::server::headless::tests::state_for(&server.uri()).await;
        let (profile, _) = crate::server::headless::tests::organization_draft_fixture(&state).await;
        let spec = profile.original_spec.clone().unwrap();
        let mut tx = aiplane_core::server::db::WriteTx::begin(&state.db)
            .await
            .unwrap();
        let version =
            agents_db::publish_in(&mut tx, &profile.principal.id, &spec.to_string(), "creator")
                .await
                .unwrap()
                .unwrap();
        sqlx::query("UPDATE agent_owners SET active=0 WHERE principal_id=?")
            .bind(&profile.principal.id)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("UPDATE system_principals SET disabled_at=? WHERE id=?")
            .bind(jiff::Timestamp::now().to_string())
            .bind(&profile.principal.id)
            .execute(&mut *tx)
            .await
            .unwrap();
        let (actual, model) = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            super::organization_automation_version_in(
                &state,
                &mut tx,
                &profile.principal.id,
                version,
            ),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(actual, spec);
        assert_eq!(model, profile.model);
        for missing in [0, -1, version + 1] {
            assert!(
                super::organization_automation_version_in(
                    &state,
                    &mut tx,
                    &profile.principal.id,
                    missing
                )
                .await
                .is_err()
            );
        }
        let mut default_spec = spec.clone();
        default_spec["main"]
            .as_object_mut()
            .unwrap()
            .remove("model");
        let default_version = agents_db::publish_in(
            &mut tx,
            &profile.principal.id,
            &default_spec.to_string(),
            "creator",
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(
            super::organization_automation_version_in(
                &state,
                &mut tx,
                &profile.principal.id,
                default_version
            )
            .await
            .unwrap(),
            (default_spec, profile.model.clone())
        );
        sqlx::query("UPDATE principal_grants SET pools='[]' WHERE principal_id=? AND kind='model' AND ref=?").bind(&profile.principal.id).bind(&profile.model).execute(&mut *tx).await.unwrap();
        assert!(
            super::organization_automation_version_in(
                &state,
                &mut tx,
                &profile.principal.id,
                version
            )
            .await
            .is_err()
        );
        sqlx::query("DELETE FROM principal_grants WHERE principal_id=? AND kind='model'")
            .bind(&profile.principal.id)
            .execute(&mut *tx)
            .await
            .unwrap();
        assert!(
            super::organization_automation_version_in(
                &state,
                &mut tx,
                &profile.principal.id,
                version
            )
            .await
            .is_err()
        );
        drop(tx);
        assert!(server.received_requests().await.unwrap().is_empty());
    }
}
