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
use super::spec::model::Permission as Setting;
use crate::server::tools::Tool;
use crate::server::tools::ask_first::{AskFirst, DEFAULT_APPROVAL_TIMEOUT};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Permission {
    /// `Some(true)` for `always_ask`, `Some(false)` for `always_allow`,
    /// `None` when the spec says nothing.
    ask: Option<bool>,
    timeout: Duration,
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

    fn gate_with_default(&self, tool: Arc<dyn Tool>, default_ask: bool) -> Arc<dyn Tool> {
        let set = self.tools.get(tool.id()).copied();
        let ask = set.and_then(|p| p.ask).unwrap_or(default_ask);
        if !ask {
            return tool;
        }
        let timeout = set.map_or(DEFAULT_APPROVAL_TIMEOUT, |p| p.timeout);
        Arc::new(AskFirst::wrap(tool, timeout))
    }
}

#[cfg(test)]
mod tests {
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
