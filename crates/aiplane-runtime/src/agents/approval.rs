// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Per-tool approval (`tool_resources.<tool>.permission`, `docs/agent-hil.md`).
//!
//! `always_ask` wraps the tool in [`AskFirst`]: every call pauses the turn
//! until a member of staff approves it in the inbox, and an approval nobody
//! gives in time is a denial (`docs/agent-hil.md`; [`SuspensionKind::timeout_fallback`]
//! enforces it whatever the tool asked). `always_allow` runs it as granted.
//! Without a `permission` a tool asks first exactly when it is known to
//! change something ([`Tool::changes_state`]): an MCP tool its server marks
//! destructive and not read-only.
//!
//! [`SuspensionKind::timeout_fallback`]: session_core::db::SuspensionKind::timeout_fallback

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use super::spec::AgentSpec;
use super::spec::model::{ActionRule, Permission as Setting};
use crate::server::tools::ask_first::{AskFirst, DEFAULT_APPROVAL_TIMEOUT};
use crate::server::tools::{
    Tool, ToolAction, ToolActionFuture, ToolContext, ToolError, ToolFuture,
};
use serde_json::Value;
use shared::api::ToolDef;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Permission {
    /// `Some(true)` for `always_ask`, `Some(false)` for `always_allow`,
    /// `None` when the spec says nothing.
    ask: Option<bool>,
    timeout: Duration,
    actions: BTreeMap<String, ActionRule>,
}

/// Every tool's permission, as the spec's `main.tool_resources` sets it.
#[derive(Debug, Clone, Default)]
pub struct Permissions {
    tools: BTreeMap<String, Permission>,
}

impl Permissions {
    pub fn from_spec(spec: &AgentSpec) -> Self {
        let tools = spec
            .main
            .tool_resources
            .iter()
            .map(|(tool, resource)| {
                let permission = Permission {
                    ask: resource.permission.map(|p| p == Setting::AlwaysAsk),
                    timeout: resource.approval_timeout(),
                    actions: resource.actions.clone(),
                };
                (tool.clone(), permission)
            })
            .collect();
        Self { tools }
    }

    /// `tool`, behind an approval when its permission (or, without one, its
    /// own say-so) asks for one.
    pub fn gate(&self, tool: Arc<dyn Tool>) -> Arc<dyn Tool> {
        let ask = tool.changes_state();
        self.gate_with_default(tool, ask)
    }

    pub fn gate_personal_dot(&self, tool: Arc<dyn Tool>) -> Arc<dyn Tool> {
        let ask = !tool.read_only();
        self.gate_with_default(tool, ask)
    }

    pub fn validate_tool_rules(&self, tool: &dyn Tool) -> Result<(), ToolError> {
        if let Some(permission) = self.tools.get(tool.id()) {
            validate_action_rules(tool, &permission.actions)?;
        }
        Ok(())
    }

    fn gate_with_default(&self, tool: Arc<dyn Tool>, default_ask: bool) -> Arc<dyn Tool> {
        let set = self.tools.get(tool.id());
        let ask = set.and_then(|p| p.ask).unwrap_or(default_ask);
        let timeout = set.map_or(DEFAULT_APPROVAL_TIMEOUT, |p| p.timeout);
        Arc::new(ActionPolicy {
            inner: tool,
            ask,
            timeout,
            actions: set.map(|p| p.actions.clone()).unwrap_or_default(),
        })
    }
}

fn validate_action_rules(
    tool: &dyn Tool,
    actions: &BTreeMap<String, ActionRule>,
) -> Result<(), ToolError> {
    for action in actions.keys() {
        if !tool
            .action_catalogue()
            .is_some_and(|known| known.iter().any(|entry| entry.id == action))
        {
            return Err(ToolError::Failed(format!(
                "`{}` cannot use action rule `{action}` because this tool does not declare it. Update the tool rules before retrying.",
                tool.id()
            )));
        }
    }
    Ok(())
}

struct ActionPolicy {
    inner: Arc<dyn Tool>,
    ask: bool,
    timeout: Duration,
    actions: BTreeMap<String, ActionRule>,
}

impl Tool for ActionPolicy {
    fn phase(&self) -> crate::server::tools::ToolPhase {
        self.inner.phase()
    }

    fn id(&self) -> &str {
        self.inner.id()
    }
    fn schema(&self) -> ToolDef {
        self.inner.schema()
    }
    fn action_catalogue(&self) -> Option<&'static [ToolAction]> {
        self.inner.action_catalogue()
    }
    fn classify_actions<'a>(&'a self, ctx: ToolContext, args: Value) -> ToolActionFuture<'a> {
        self.inner.classify_actions(ctx, args)
    }
    fn max_duration(&self) -> Option<Duration> {
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
    fn run<'a>(&'a self, ctx: ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let catalogue = self.inner.action_catalogue();
            validate_action_rules(self.inner.as_ref(), &self.actions)?;
            let classified = self
                .inner
                .classify_actions(ctx.clone(), args.clone())
                .await?;
            crate::server::tools::ask_first::validate_actions(
                &ctx,
                self.id(),
                catalogue,
                &classified,
            )?;
            let mut ask = self.ask;
            if let Some(actions) = &classified {
                ask = false;
                for action in actions {
                    match self.actions.get(action) {
                        Some(ActionRule::Deny) => {
                            return Err(ToolError::Failed(format!(
                                "`{}` action `{action}` is denied by the current tool rules. Change the rule before retrying.",
                                self.id()
                            )));
                        }
                        Some(ActionRule::Ask) => ask = true,
                        Some(ActionRule::Allow) => {}
                        None => ask |= self.ask,
                    }
                }
            }
            if ask {
                AskFirst::wrap(self.inner.clone(), self.timeout)
                    .run(ctx, args)
                    .await
            } else {
                self.inner.run(ctx, args).await
            }
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn action_policy_preserves_the_tools_phase() {
        let tool = Permissions::default().gate(Arc::new(crate::server::tools::tests::PhasedEcho));
        assert_eq!(tool.phase(), crate::server::tools::ToolPhase::ActsOnState);
    }
    use super::*;
    use crate::server::tools::echo::Echo;
    use crate::server::tools::{ToolContext, ToolFuture};
    use crate::suspend::{Suspend, extract_suspend};
    use serde_json::{Value, json};
    use shared::api::ToolDef;

    /// The echo, saying it changes something as a destructive MCP tool
    /// would. It borrows the echo's id so no fixture id joins the catalog.
    struct Destructive;

    impl Tool for Destructive {
        fn id(&self) -> &str {
            "company_echo"
        }
        fn schema(&self) -> ToolDef {
            Echo.schema()
        }
        fn run<'a>(&'a self, ctx: ToolContext, args: Value) -> ToolFuture<'a> {
            Box::pin(async move { Echo.run(ctx, args).await })
        }
        fn changes_state(&self) -> bool {
            true
        }
    }

    async fn asks(permissions: &Permissions, tool: Arc<dyn Tool>) -> Option<u64> {
        let db = aiplane_core::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        let ctx = ToolContext {
            suspend: Suspend::Available,
            ..ToolContext::for_test(db)
        };
        let body = permissions
            .gate(tool)
            .run(ctx, json!({"message": "hi"}))
            .await
            .unwrap();
        extract_suspend(&body).map(|r| r.timeout_secs)
    }

    fn permissions(spec: Value) -> Permissions {
        Permissions::from_spec(&AgentSpec::from_value(&spec).unwrap())
    }

    struct Actions;
    impl Tool for Actions {
        fn id(&self) -> &str {
            "company_echo"
        }
        fn schema(&self) -> ToolDef {
            Echo.schema()
        }
        fn run<'a>(&'a self, _ctx: ToolContext, args: Value) -> ToolFuture<'a> {
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
                let action = args["action"]
                    .as_str()
                    .filter(|a| ["read", "write"].contains(a))
                    .ok_or_else(|| {
                        crate::server::tools::ToolError::InvalidArgs("unknown action".into())
                    })?;
                Ok(Some(vec![action.to_string()]))
            })
        }
    }

    #[test]
    fn tool_rule_availability_uses_the_actual_action_catalogue() {
        let selected = permissions(
            json!({"main":{"tool_resources":{"company_echo":{"actions":{"read":"allow"}}}}}),
        );
        assert!(selected.validate_tool_rules(&Actions).is_ok());
        assert!(selected.validate_tool_rules(&Echo).is_err());
        let unknown = permissions(
            json!({"main":{"tool_resources":{"company_echo":{"actions":{"missing":"ask"}}}}}),
        );
        assert!(unknown.validate_tool_rules(&Actions).is_err());
    }

    #[tokio::test]
    async fn action_rules_allow_ask_deny_and_validate_before_asking() {
        let db = aiplane_core::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        let ctx = ToolContext {
            suspend: Suspend::Available,
            ..ToolContext::for_test(db)
        };
        for (rule, outcome) in [("allow", 0), ("ask", 1), ("deny", 2)] {
            let p = permissions(json!({"main":{"tool_resources":{"company_echo":{
                "permission":"always_ask", "actions":{"read":rule}
            }}}}));
            let result = p
                .gate(Arc::new(Actions))
                .run(ctx.clone(), json!({"action":"read"}))
                .await;
            match outcome {
                0 => assert_eq!(result.unwrap(), json!({"action":"read"})),
                1 => assert_eq!(
                    extract_suspend(&result.unwrap()).unwrap().actions,
                    Some(vec!["read".to_string()])
                ),
                _ => assert!(result.is_err()),
            }
            assert!(
                p.gate(Arc::new(Actions))
                    .run(ctx.clone(), json!({"action":"invalid"}))
                    .await
                    .is_err()
            );
        }
    }

    #[tokio::test]
    async fn resumed_action_mismatch_and_unknown_saved_rules_refuse_execution() {
        let db = aiplane_core::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        let ctx = ToolContext {
            suspend: Suspend::Decided(
                session_core::db::SuspensionKind::Approval,
                session_core::db::Decision::AllowOnce,
            ),
            pending_actions: Some(crate::server::tools::PendingActionFacts {
                actions: Some(vec!["write".to_string()]),
            }),
            ..ToolContext::for_test(db)
        };
        let p = permissions(
            json!({"main":{"tool_resources":{"company_echo":{"permission":"always_allow"}}}}),
        );
        assert!(
            p.gate(Arc::new(Actions))
                .run(ctx.clone(), json!({"action":"read"}))
                .await
                .is_err()
        );
        let p = permissions(
            json!({"main":{"tool_resources":{"company_echo":{"actions":{"removed":"allow"}}}}}),
        );
        assert!(
            p.gate(Arc::new(Actions))
                .run(ctx, json!({"action":"write"}))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn connector_ask_is_a_ceiling_over_action_allow() {
        let db = aiplane_core::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        let ctx = ToolContext {
            suspend: Suspend::Available,
            ..ToolContext::for_test(db)
        };
        let p = permissions(json!({"main":{"tool_resources":{"company_echo":{
            "permission":"always_allow", "actions":{"read":"allow"}
        }}}}));
        let tool = Arc::new(AskFirst::new(Actions, Duration::from_secs(30)));
        let body = p
            .gate(tool)
            .run(ctx, json!({"action":"read"}))
            .await
            .unwrap();
        assert_eq!(
            extract_suspend(&body).unwrap().actions,
            Some(vec!["read".into()])
        );
    }

    #[tokio::test]
    async fn approved_action_runs_and_unsupported_contract_change_is_refused() {
        let db = aiplane_core::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        let mut ctx = ToolContext {
            suspend: Suspend::Decided(
                session_core::db::SuspensionKind::Approval,
                session_core::db::Decision::AllowOnce,
            ),
            pending_actions: Some(crate::server::tools::PendingActionFacts {
                actions: Some(vec!["read".into()]),
            }),
            ..ToolContext::for_test(db)
        };
        let p = permissions(
            json!({"main":{"tool_resources":{"company_echo":{"actions":{"read":"ask"}}}}}),
        );
        assert_eq!(
            p.gate(Arc::new(Actions))
                .run(ctx.clone(), json!({"action":"read"}))
                .await
                .unwrap(),
            json!({"action":"read"})
        );
        ctx.pending_actions = Some(crate::server::tools::PendingActionFacts { actions: None });
        assert!(
            p.gate(Arc::new(Actions))
                .run(ctx.clone(), json!({"action":"read"}))
                .await
                .is_err()
        );
        ctx.pending_actions = Some(crate::server::tools::PendingActionFacts {
            actions: Some(vec!["read".into()]),
        });
        assert!(
            p.gate(Arc::new(Echo))
                .run(ctx, json!({"message":"hi"}))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn always_ask_pauses_and_always_allow_runs() {
        let ask = permissions(json!({ "main": { "tool_resources": {
            "company_echo": { "permission": "always_ask", "approval_timeout": "15m" }
        } } }));
        assert_eq!(asks(&ask, Arc::new(Echo)).await, Some(15 * 60));
        let allow = permissions(json!({ "main": { "tool_resources": {
            "company_echo": { "permission": "always_allow" }
        } } }));
        assert_eq!(asks(&allow, Arc::new(Destructive)).await, None);
    }

    #[tokio::test]
    async fn without_a_permission_only_a_state_changing_tool_asks() {
        let p = Permissions::from_spec(AgentSpec::empty());
        assert_eq!(asks(&p, Arc::new(Echo)).await, None);
        assert_eq!(
            asks(&p, Arc::new(Destructive)).await,
            Some(DEFAULT_APPROVAL_TIMEOUT.as_secs())
        );
    }

    #[tokio::test]
    async fn personal_dot_defaults_require_positive_read_only_metadata() {
        let db = aiplane_core::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        let ctx = ToolContext {
            suspend: Suspend::Available,
            ..ToolContext::for_test(db)
        };
        let permissions = Permissions::from_spec(AgentSpec::empty());
        for (tool, should_ask) in [
            (Arc::new(Echo) as Arc<dyn Tool>, true),
            (
                Arc::new(crate::server::tools::time::CurrentTimestamp) as Arc<dyn Tool>,
                false,
            ),
        ] {
            let body = permissions
                .gate_personal_dot(tool)
                .run(ctx.clone(), json!({}))
                .await
                .unwrap();
            assert_eq!(extract_suspend(&body).is_some(), should_ask);
        }
    }
}
