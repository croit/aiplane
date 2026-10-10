// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! One agent run, as a single value.
//!
//! A turn either acts for a person or is an agent's run. An agent run carries
//! everything that sets it apart — the system principal it acts as, the call
//! chain it runs in, its finish contract, budget and injection scan, and the
//! spec's [`AgentSurface`] — in one [`AgentRun`], built once and handed down
//! as `Option<Arc<AgentRun>>`: through [`Actor`] into the headless drive, and
//! from there into the driver and every [`ToolContext`]. "Is this an agent
//! run" is then one question with one answer everywhere: is there an
//! `AgentRun`.
//!
//! [`AgentRun::new`] is the only way to build one, and it refuses a chain
//! whose running frame is not the principal, so a run can never act as one
//! agent while auditing as another. See docs/agent-runs.md → "One run, one value".
//!
//! [`ToolContext`]: crate::server::tools::ToolContext

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use aiplane_agents::db::agent_audit::Redaction;
use aiplane_core::server::principal::{Principal, SystemPrincipal};
use aiplane_core::server::run_chain::RunChain;

use crate::agents::profile::AgentSurface;
use crate::budget::Budget;
use crate::finish::{FinishContract, FinishTool, IncompleteReason, RunOutcome};
use crate::server::tools::Tool;
use crate::server::tools::injection::InjectionScan;

/// A call chain and a principal that name two different agents.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "the run's call chain names `{chain_agent}` as the running agent, but the run was started as \
     `{principal}`; refusing to run it as either. Build the chain's running frame from the same \
     principal the run acts as"
)]
pub struct MismatchedRun {
    pub chain_agent: String,
    pub principal: String,
}

/// Everything that makes a turn an agent's run.
pub struct AgentRun {
    principal: SystemPrincipal,
    chain: Arc<RunChain>,
    finish: Option<Arc<FinishTool>>,
    budget: Option<Budget>,
    injection: InjectionScan,
    surface: Option<Arc<AgentSurface>>,
    outcome: Mutex<Option<RunOutcome>>,
    /// The model round the driver is in, so an event a tool writes names it.
    round: AtomicU32,
    /// Set once an event of this run could not be written to the activity
    /// log; the driver then stops the run (`agents::audit`).
    log_failed: AtomicBool,
    /// What every event of this run leaves out: the secure input the turn
    /// resumed with, and the tools seen so far that declare their arguments
    /// sensitive. Attached to each event (`ToolContext::redaction`) and
    /// applied by the log itself.
    redaction: Mutex<Redaction>,
}

impl std::fmt::Debug for AgentRun {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentRun")
            .field("principal", &self.principal.name)
            .field("chain", &self.chain)
            .field("contract", &self.contract().is_some())
            .field("budget", &self.budget)
            .field("injection", &self.injection.policy)
            .finish_non_exhaustive()
    }
}

impl AgentRun {
    /// A run of `principal` in `chain`, with no contract, the conversation's
    /// effort-level budget, no injection screening and no spec surface; the
    /// `with_*` methods add those.
    pub fn new(principal: SystemPrincipal, chain: Arc<RunChain>) -> Result<Self, MismatchedRun> {
        if chain.current().principal_id != principal.id {
            return Err(MismatchedRun {
                chain_agent: chain.current().name.clone(),
                principal: principal.name.clone(),
            });
        }
        Ok(Self {
            principal,
            chain,
            finish: None,
            budget: None,
            injection: InjectionScan::default(),
            surface: None,
            outcome: Mutex::new(None),
            round: AtomicU32::new(0),
            log_failed: AtomicBool::new(false),
            redaction: Mutex::new(Redaction::default()),
        })
    }

    /// The run ends only through a schema-valid `finish`, or incomplete.
    pub fn with_contract(mut self, contract: FinishContract) -> Self {
        self.finish = Some(Arc::new(FinishTool::new(contract)));
        self
    }

    pub fn with_budget(mut self, budget: Budget) -> Self {
        self.budget = Some(budget);
        self
    }

    pub fn with_injection(mut self, injection: InjectionScan) -> Self {
        self.injection = injection;
        self
    }

    /// The spec's system message, synthetic tools and bound arguments.
    pub fn with_surface(mut self, surface: Arc<AgentSurface>) -> Self {
        self.surface = Some(surface);
        self
    }

    /// What this run's events leave out so far.
    pub fn redaction(&self) -> Redaction {
        self.redaction
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// Leave `redaction` out of this run's events from now on, on top of
    /// what is left out already.
    pub fn redact(&self, redaction: &Redaction) {
        let mut held = self.redaction.lock().unwrap_or_else(|p| p.into_inner());
        *held = std::mem::take(&mut *held).and(redaction);
    }

    /// Leave the arguments of every call to tool `name` out of this run's
    /// events.
    pub fn note_sensitive(&self, name: &str) {
        self.redaction
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .sensitive_tools
            .insert(name.to_string());
    }

    pub fn system_principal(&self) -> &SystemPrincipal {
        &self.principal
    }

    /// The principal the run acts as, in the form a tool context holds.
    pub fn principal(&self) -> Principal {
        Principal::System(self.principal.clone())
    }

    pub fn chain(&self) -> &Arc<RunChain> {
        &self.chain
    }

    pub fn contract(&self) -> Option<&FinishContract> {
        self.finish.as_deref().map(FinishTool::contract)
    }

    pub fn finish(&self) -> Option<&FinishTool> {
        self.finish.as_deref()
    }

    /// The run-scoped tool that ends this run, under a contract: its
    /// `finish`, in [`ToolPhase::Terminal`](crate::server::tools::ToolPhase).
    pub fn terminal_tool(&self) -> Option<Arc<dyn Tool>> {
        self.finish.clone().map(|tool| tool as Arc<dyn Tool>)
    }

    /// `None` takes the round cap of the conversation's effort level.
    pub fn budget(&self) -> Option<Budget> {
        self.budget
    }

    pub fn injection(&self) -> &InjectionScan {
        &self.injection
    }

    pub fn surface(&self) -> Option<&AgentSurface> {
        self.surface.as_deref()
    }

    pub fn round(&self) -> u32 {
        self.round.load(Ordering::Relaxed)
    }

    /// Only the driver calls it, at the top of each round.
    pub fn enter_round(&self, round: u32) {
        self.round.store(round, Ordering::Relaxed);
    }

    /// Whether an event of this run failed to reach the activity log. The
    /// run fails closed on it: nothing more may happen that the log would
    /// not show.
    pub fn log_failed(&self) -> bool {
        self.log_failed.load(Ordering::Acquire)
    }

    pub fn mark_log_failed(&self) {
        self.log_failed.store(true, Ordering::Release);
    }

    /// Record how a contracted run ended. Only the driver's `run_turn` calls
    /// it, once per turn, from how the turn ended.
    pub fn settle(&self, outcome: RunOutcome) {
        *self.outcome.lock().unwrap_or_else(|p| p.into_inner()) = Some(outcome);
    }

    /// How the run ended, if the driver settled it yet, without taking it.
    pub fn outcome(&self) -> Option<RunOutcome> {
        self.outcome
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// How the run ended. A run the driver never settled — its turn panicked
    /// before `run_turn` returned — was interrupted, and is incomplete.
    pub fn take_outcome(&self) -> RunOutcome {
        self.outcome
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
            .unwrap_or_else(|| RunOutcome::Incomplete {
                reason: IncompleteReason::Failed {
                    message: "the run was interrupted before it ended".into(),
                },
                summary: String::new(),
            })
    }
}

/// Who a driven turn acts as: a person, or an agent's run. One value, so a
/// person's turn cannot carry an agent's chain and an agent's run cannot act
/// as anyone but its own principal.
#[derive(Debug, Clone)]
pub enum Actor {
    /// A person, gated by their roles: their real roles offer their normal
    /// tools, an empty list offers none (the scheduler's "tools off").
    Person { id: String, roles: Vec<String> },
    /// A personal Dot is named separately, but exercises the owner's user
    /// principal. The Dot identity travels beside that principal for scoped
    /// data and audit attribution.
    PersonalDot {
        id: String,
        roles: Vec<String>,
        dot: PersonalDotIdentity,
    },
    /// An agent's run, offered exactly its principal's grants.
    Agent(Arc<AgentRun>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonalDotIdentity {
    pub owner_user_id: String,
    pub dot_id: String,
}

impl Actor {
    pub fn person(id: impl Into<String>, roles: Vec<String>) -> Self {
        Self::Person {
            id: id.into(),
            roles,
        }
    }

    pub fn personal_dot(
        owner_user_id: impl Into<String>,
        roles: Vec<String>,
        dot_id: impl Into<String>,
    ) -> Self {
        let owner_user_id = owner_user_id.into();
        Self::PersonalDot {
            id: owner_user_id.clone(),
            roles,
            dot: PersonalDotIdentity {
                owner_user_id,
                dot_id: dot_id.into(),
            },
        }
    }

    pub fn principal(&self) -> Principal {
        match self {
            Self::Person { id, roles } => Principal::User {
                id: id.clone(),
                roles: roles.clone(),
            },
            Self::PersonalDot { id, roles, .. } => Principal::User {
                id: id.clone(),
                roles: roles.clone(),
            },
            Self::Agent(run) => run.principal(),
        }
    }

    pub fn agent(&self) -> Option<&Arc<AgentRun>> {
        match self {
            Self::Person { .. } | Self::PersonalDot { .. } => None,
            Self::Agent(run) => Some(run),
        }
    }

    /// The person behind the turn; `None` for an agent's run.
    pub fn person_id(&self) -> Option<&str> {
        match self {
            Self::Person { id, .. } | Self::PersonalDot { id, .. } => Some(id),
            Self::Agent(_) => None,
        }
    }

    pub fn personal_dot_identity(&self) -> Option<&PersonalDotIdentity> {
        match self {
            Self::PersonalDot { dot, .. } => Some(dot),
            Self::Person { .. } | Self::Agent(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiplane_core::server::principal::GrantSet;
    use aiplane_core::server::run_chain::Frame;

    fn principal(id: &str, name: &str) -> SystemPrincipal {
        SystemPrincipal {
            id: id.into(),
            name: name.into(),
            grants: Arc::new(GrantSet::default()),
        }
    }

    fn chain_of(running: &SystemPrincipal) -> Arc<RunChain> {
        Arc::new(RunChain::root(
            "s1",
            None,
            Frame::for_principal(running, None),
        ))
    }

    #[test]
    fn a_run_acts_as_its_chains_running_principal() {
        let billing = principal("p2", "billing");
        let run = AgentRun::new(billing.clone(), chain_of(&billing)).unwrap();
        assert_eq!(run.principal(), Principal::System(billing));
        assert_eq!(run.chain().current().principal_id, "p2");
    }

    #[test]
    fn a_personal_dot_keeps_the_users_principal_and_the_dots_attribution() {
        let actor = Actor::personal_dot("u1", vec!["staff".into()], "dot-7");
        assert_eq!(
            actor.principal(),
            Principal::User {
                id: "u1".into(),
                roles: vec!["staff".into()],
            }
        );
        assert_eq!(actor.person_id(), Some("u1"));
        assert_eq!(
            actor.personal_dot_identity().map(|dot| dot.dot_id.as_str()),
            Some("dot-7")
        );
        assert!(actor.agent().is_none());
    }

    #[test]
    fn a_run_whose_chain_names_another_agent_cannot_be_built() {
        let main = principal("p1", "support-website");
        let billing = principal("p2", "billing");
        let err = AgentRun::new(billing, chain_of(&main)).unwrap_err();
        assert_eq!(
            err,
            MismatchedRun {
                chain_agent: "support-website".into(),
                principal: "billing".into(),
            }
        );
        assert!(err.to_string().contains("refusing to run it"), "{err}");
    }

    #[test]
    fn a_run_that_never_settled_reads_as_interrupted() {
        let billing = principal("p2", "billing");
        let run = AgentRun::new(billing.clone(), chain_of(&billing)).unwrap();
        let RunOutcome::Incomplete {
            reason: IncompleteReason::Failed { message },
            ..
        } = run.take_outcome()
        else {
            panic!("expected an interrupted run");
        };
        assert!(message.contains("interrupted"), "{message}");

        let finished = RunOutcome::Finished {
            result: serde_json::json!({"ok": true}),
        };
        run.settle(finished.clone());
        assert_eq!(run.take_outcome(), finished);
    }

    #[test]
    fn only_an_agent_actor_carries_a_run() {
        let person = Actor::person("u1", vec!["everyone".into()]);
        assert!(person.agent().is_none());
        assert_eq!(person.person_id(), Some("u1"));
        assert_eq!(person.principal().subject_id(), "u1");

        let billing = principal("p2", "billing");
        let run = AgentRun::new(billing, chain_of(&principal("p2", "billing"))).unwrap();
        let agent = Actor::Agent(Arc::new(run));
        assert!(agent.agent().is_some());
        assert_eq!(agent.person_id(), None);
        assert_eq!(agent.principal().subject_id(), "p2");
    }
}
