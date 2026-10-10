// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Whether one tool call the model made may run, and why — decided in code,
//! per call, before anything executes (`docs/agents.md`, trust rule 5).
//!
//! Inside an agent run every decision is also written to `agent_audit` with
//! the run's call chain, so "which agent called what, through whom, for which
//! visitor, and what let it" is answerable afterwards.

use aiplane_agents::db::agent_audit::AuditKind;

use crate::agent_run::AgentRun;
use crate::server::tools::ToolContext;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CallPolicy {
    /// The tool was offered this round: inside the caller's grant.
    Granted,
    /// A person's chat called a tool of their grant the conversation had not
    /// turned on yet; the call runs and enables it (miss recovery).
    AutoEnabled,
    /// An agent run called a tool outside its principal's grants. Never
    /// runs: a principal holds exactly what was granted, and nothing is
    /// enabled on the fly.
    NotGranted,
    /// The person switched the tool off for this conversation.
    DisabledInConversation,
    /// No tool of that name exists for this caller. A person calling a tool
    /// outside their grant lands here too: to them it does not exist.
    UnknownTool,
}

impl CallPolicy {
    /// `agent`: the turn's agent run, `None` for a person's turn.
    /// `known`: some source of the turn has the tool, granted or not.
    /// `granted`: the principal's grant-narrowed source has it, the only
    /// source a call ever runs from.
    pub(super) fn decide(
        agent: Option<&AgentRun>,
        known: bool,
        granted: bool,
        disabled_in_conversation: bool,
        offered: bool,
    ) -> Self {
        if !known {
            Self::UnknownTool
        } else if !granted {
            if agent.is_some() {
                Self::NotGranted
            } else {
                Self::UnknownTool
            }
        } else if disabled_in_conversation {
            Self::DisabledInConversation
        } else if offered {
            Self::Granted
        } else if agent.is_some() {
            Self::NotGranted
        } else {
            Self::AutoEnabled
        }
    }

    pub(super) fn allows(self) -> bool {
        matches!(self, Self::Granted | Self::AutoEnabled)
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Granted => "granted",
            Self::AutoEnabled => "auto_enabled",
            Self::NotGranted => "not_granted",
            Self::DisabledInConversation => "disabled_in_conversation",
            Self::UnknownTool => "unknown_tool",
        }
    }
}

/// What the model reads instead of a result when an agent run calls a tool its
/// principal does not hold.
pub(super) fn not_granted_message(tool: &str, agent: Option<&AgentRun>) -> String {
    let name = agent.map_or("", |run| run.system_principal().name.as_str());
    format!(
        "Tool `{tool}` is not granted to `{name}`, so it cannot be used in this run. Only call a \
         tool whose schema was provided to you; if the task needs this one, say so in your answer."
    )
}

/// Record one decision when the call is part of an agent run. Best-effort: the
/// decision stands whether or not the row lands.
pub(super) async fn audit(
    ctx: &ToolContext,
    call_id: &str,
    tool: &str,
    policy: CallPolicy,
) -> bool {
    if !ctx.agent_active() && ctx.personal_dot.is_none() {
        return true;
    }
    let detail = serde_json::json!({
        "tool": tool,
        "call_id": call_id,
        "decision": if policy.allows() { "allowed" } else { "denied" },
        "policy": policy.as_str(),
    });
    if ctx.personal_dot.is_some() {
        ctx.audit_event(AuditKind::ToolCall, None, detail)
            .await
            .is_some()
    } else {
        ctx.audit(AuditKind::ToolCall, detail).await;
        true
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use aiplane_core::server::principal::{GrantSet, SystemPrincipal};
    use aiplane_core::server::run_chain::{Frame, RunChain};

    fn person() -> Option<&'static AgentRun> {
        None
    }

    fn agent() -> Option<&'static AgentRun> {
        static RUN: std::sync::OnceLock<AgentRun> = std::sync::OnceLock::new();
        Some(RUN.get_or_init(|| {
            let principal = SystemPrincipal {
                id: "p1".into(),
                name: "support-website".into(),
                grants: Arc::new(GrantSet::default()),
            };
            let chain = Arc::new(RunChain::root(
                "s1",
                None,
                Frame::for_principal(&principal, None),
            ));
            AgentRun::new(principal, chain).unwrap()
        }))
    }

    #[test]
    fn an_offered_tool_is_granted_for_anyone() {
        for who in [person(), agent()] {
            assert_eq!(
                CallPolicy::decide(who, true, true, false, true),
                CallPolicy::Granted
            );
        }
    }

    #[test]
    fn only_a_person_gets_an_unoffered_tool_auto_enabled() {
        assert_eq!(
            CallPolicy::decide(person(), true, true, false, false),
            CallPolicy::AutoEnabled
        );
        let refused = CallPolicy::decide(agent(), true, true, false, false);
        assert_eq!(refused, CallPolicy::NotGranted);
        assert!(!refused.allows());
    }

    #[test]
    fn unknown_and_disabled_tools_never_run() {
        for who in [person(), agent()] {
            assert_eq!(
                CallPolicy::decide(who, false, false, false, true),
                CallPolicy::UnknownTool
            );
            assert_eq!(
                CallPolicy::decide(who, true, true, true, true),
                CallPolicy::DisabledInConversation
            );
        }
        assert!(!CallPolicy::UnknownTool.allows());
        assert!(!CallPolicy::DisabledInConversation.allows());
    }

    #[test]
    fn a_tool_outside_the_grant_never_runs_even_when_offered() {
        assert_eq!(
            CallPolicy::decide(agent(), true, false, false, true),
            CallPolicy::NotGranted
        );
        assert_eq!(
            CallPolicy::decide(person(), true, false, false, true),
            CallPolicy::UnknownTool
        );
    }

    #[test]
    fn the_refusal_names_the_tool_and_the_principal() {
        let message = not_granted_message("run_in_sandbox", agent());
        assert!(message.contains("`run_in_sandbox`"), "{message}");
        assert!(message.contains("`support-website`"), "{message}");
    }
}
