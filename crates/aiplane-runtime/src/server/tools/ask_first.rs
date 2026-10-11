// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! `AskFirst`: a tool that runs only after the user approves the call.
//!
//! The first consumer of [`crate::suspend`] and the shape `permission:
//! always_ask` (`docs/agent-hil.md`) takes: wrap any tool, keep its id and schema, and on each
//! call pause the turn for an approval instead of running it. The driver
//! answers a denial itself; an approval runs the call again with
//! [`Suspend::Decided`], and only then does the wrapped tool run.
//!
//! Where pausing is impossible (`/v1`) the call is refused: a tool that
//! needs approval never runs without one.
//!
//! A tool that must check its arguments before it asks (so the person is
//! never asked about a call that cannot run) follows the same protocol
//! itself, through [`approval`].

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use session_core::db::Decision;
use shared::api::ToolDef;

use super::{Tool, ToolAction, ToolActionFuture, ToolContext, ToolError, ToolFuture};
use crate::suspend::{Suspend, SuspendRequest, tool_suspend};

/// How long an approval may take when nobody chose a deadline: an agent spec
/// without `approval_timeout`, a connector tool in `ask` mode.
pub const DEFAULT_APPROVAL_TIMEOUT: Duration = Duration::from_secs(60 * 60);

pub struct AskFirst {
    inner: Arc<dyn Tool>,
    timeout: Duration,
}

impl AskFirst {
    /// Gate `inner` behind an approval that expires after `timeout`, as a
    /// denial: an approval nobody gives is never one.
    pub fn new(inner: impl Tool, timeout: Duration) -> Self {
        Self::wrap(Arc::new(inner), timeout)
    }

    /// [`Self::new`] for a tool already behind an `Arc`, as a tool source
    /// hands it out.
    pub fn wrap(inner: Arc<dyn Tool>, timeout: Duration) -> Self {
        Self { inner, timeout }
    }
}

/// Where a call that needs an approval stands.
#[derive(Debug)]
pub enum Approval {
    /// The person approved this call: run it.
    Granted,
    /// Return this body: it pauses the turn until the person decides.
    Ask(Value),
}

/// The approval protocol of [`AskFirst`] for tool `id`'s call: pause for
/// `request` the first time, run once approved, refuse a denial and refuse
/// where nothing can pause.
pub fn approval(
    ctx: &ToolContext,
    id: &str,
    request: SuspendRequest,
) -> Result<Approval, ToolError> {
    match &ctx.suspend {
        Suspend::Decided(_, Decision::AllowOnce) => Ok(Approval::Granted),
        Suspend::Decided(_, other) => Err(ToolError::Failed(format!(
            "`{id}` was not approved (decision: {:?}), so it did not run.",
            other.kind()
        ))),
        Suspend::Available => Ok(Approval::Ask(tool_suspend(request))),
        Suspend::Unavailable => Err(ToolError::Failed(format!(
            "`{id}` runs only after the user approves the call, and this run cannot \
             pause to ask. Do not retry it here."
        ))),
    }
}

pub(crate) fn validate_actions(
    ctx: &ToolContext,
    id: &str,
    catalogue: Option<&[ToolAction]>,
    actions: &Option<Vec<String>>,
) -> Result<(), ToolError> {
    match (catalogue, actions) {
        (None, None) => {}
        (Some(known), Some(actual))
            if !actual.is_empty()
                && actual
                    .iter()
                    .all(|id| known.iter().any(|entry| entry.id == id)) => {}
        _ => {
            return Err(ToolError::Failed(format!(
                "`{id}` returned action facts that do not match its declared actions, so it did not run."
            )));
        }
    }
    if let Some(pending) = &ctx.pending_actions
        && &pending.actions != actions
    {
        return Err(ToolError::Failed(format!(
            "`{id}` actions changed since this call requested approval, so it did not run. Request a new approval."
        )));
    }
    Ok(())
}

pub(crate) fn validate_execution_facts(
    ctx: &ToolContext,
    id: &str,
    facts: &Option<Value>,
) -> Result<(), ToolError> {
    if let Some(pending) = &ctx.pending_execution_facts
        && &pending.execution_facts != facts
    {
        return Err(ToolError::Failed(format!(
            "`{id}` connection identity changed since this call requested approval, so it did not run. Request a new approval."
        )));
    }
    Ok(())
}

impl Tool for AskFirst {
    fn execution_facts(&self) -> Option<Value> {
        self.inner.execution_facts()
    }

    fn phase(&self) -> crate::server::tools::ToolPhase {
        self.inner.phase()
    }

    fn id(&self) -> &str {
        self.inner.id()
    }

    fn schema(&self) -> ToolDef {
        self.inner.schema()
    }

    fn run<'a>(&'a self, ctx: ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let actions = self
                .inner
                .classify_actions(ctx.clone(), args.clone())
                .await?;
            validate_actions(&ctx, self.id(), self.inner.action_catalogue(), &actions)?;
            let facts = self.inner.execution_facts();
            validate_execution_facts(&ctx, self.id(), &facts)?;
            let mut request = SuspendRequest::approval(self.timeout);
            request.execution_facts = facts;
            request.actions = actions;
            match approval(&ctx, self.id(), request)? {
                Approval::Granted => self.inner.run(ctx, args).await,
                Approval::Ask(pause) => Ok(pause),
            }
        })
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
}

#[cfg(test)]
mod tests {
    #[test]
    fn approval_wrapper_preserves_the_tools_phase() {
        let tool = AskFirst::new(
            crate::server::tools::tests::PhasedEcho,
            Duration::from_secs(90),
        );
        assert_eq!(tool.phase(), crate::server::tools::ToolPhase::ActsOnState);
        assert_eq!(
            tool.execution_facts(),
            Some(serde_json::json!({"resource":"fixture"}))
        );
    }
    use super::*;
    use crate::server::tools::echo::Echo;
    use crate::suspend::extract_suspend;
    use serde_json::json;
    use session_core::db::{DenyReason, SuspensionKind, TimeoutFallback};

    async fn ctx(suspend: Suspend) -> ToolContext {
        let db = aiplane_core::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        ToolContext {
            suspend,
            ..ToolContext::for_test(db)
        }
    }

    fn gated() -> AskFirst {
        AskFirst::new(Echo, Duration::from_secs(90))
    }

    struct IdentityWrite(Value);

    impl Tool for IdentityWrite {
        fn id(&self) -> &str {
            "company_echo"
        }
        fn schema(&self) -> shared::api::ToolDef {
            Echo.schema()
        }
        fn execution_facts(&self) -> Option<Value> {
            Some(self.0.clone())
        }
        fn run<'a>(&'a self, ctx: ToolContext, _args: Value) -> ToolFuture<'a> {
            Box::pin(async move {
                sqlx::query("INSERT INTO app_settings(key,value,updated_at) VALUES('identity_write','1','2026-01-01T00:00:00Z') ON CONFLICT(key) DO UPDATE SET value=CAST(value AS INTEGER)+1")
                    .execute(&ctx.db).await.unwrap();
                Ok(json!({"written":true}))
            })
        }
    }

    fn identity_facts(generation: &str, binding_generation: Option<&str>) -> Value {
        serde_json::to_value(crate::server::tools::mcp::StoredMcpIdentity {
            connection: aiplane_core::server::db::mcp_connections::ConnectionIdentity {
                owner: aiplane_core::server::db::mcp_connections::ConnectionOwner::User("u".into()),
                connector_key: "fixture".into(),
                connection_id: "connection-one".into(),
                generation: generation.into(),
            },
            binding_generation: binding_generation.map(str::to_owned),
        })
        .unwrap()
    }

    #[tokio::test]
    async fn mcp_approval_captures_identity_and_refuses_changed_facts_without_writing() {
        let facts = identity_facts("credential-one", None);
        let tool = AskFirst::new(IdentityWrite(facts.clone()), Duration::from_secs(90));
        assert_eq!(tool.execution_facts(), Some(facts.clone()));
        let fresh = ctx(Suspend::Available).await;
        let paused = extract_suspend(&tool.run(fresh.clone(), json!({})).await.unwrap()).unwrap();
        assert_eq!(paused.execution_facts, Some(facts.clone()));
        assert!(
            sqlx::query_scalar::<_, String>(
                "SELECT value FROM app_settings WHERE key='identity_write'"
            )
            .fetch_optional(&fresh.db)
            .await
            .unwrap()
            .is_none()
        );
        let pending = ToolContext {
            suspend: Suspend::Decided(
                SuspensionKind::Approval,
                session_core::db::Decision::AllowOnce,
            ),
            pending_execution_facts: Some(crate::server::tools::PendingExecutionFacts {
                execution_facts: Some(facts.clone()),
            }),
            ..fresh.clone()
        };
        assert!(tool.run(pending.clone(), json!({})).await.is_ok());
        for changed in [
            identity_facts("credential-two", None),
            identity_facts("credential-one", Some("binding-two")),
        ] {
            let replacement = AskFirst::new(IdentityWrite(changed), Duration::from_secs(90));
            assert!(replacement.run(pending.clone(), json!({})).await.is_err());
        }
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT value FROM app_settings WHERE key='identity_write'"
            )
            .fetch_one(&fresh.db)
            .await
            .unwrap(),
            "1"
        );
    }

    #[tokio::test]
    async fn resumed_execution_facts_are_checked_without_an_approval_wrapper_and_do_not_reach_fresh_calls()
     {
        use crate::server::tools::{ToolRegistry, runner};
        let facts = identity_facts("credential-one", None);
        let fresh = ctx(Suspend::Available).await;
        let call = runner::ToolCallRef {
            id: "call-one".into(),
            name: "company_echo".into(),
            arguments_raw: "{}".into(),
        };
        let registry = ToolRegistry::new().with(IdentityWrite(facts.clone()));
        for original in [
            None,
            Some(identity_facts("credential-two", None)),
            Some(json!({"unknown":"facts"})),
        ] {
            let pending = ToolContext {
                suspend: Suspend::Decided(
                    SuspensionKind::Approval,
                    session_core::db::Decision::AllowOnce,
                ),
                pending_execution_facts: Some(crate::server::tools::PendingExecutionFacts {
                    execution_facts: original,
                }),
                ..fresh.clone()
            };
            let result = runner::execute_tool_calls(
                &registry,
                &pending,
                std::slice::from_ref(&call),
                &crate::server::tools::injection::InjectionScan::default(),
            )
            .await;
            assert!(result[0].failed);
        }
        assert!(
            sqlx::query_scalar::<_, String>(
                "SELECT value FROM app_settings WHERE key='identity_write'"
            )
            .fetch_optional(&fresh.db)
            .await
            .unwrap()
            .is_none()
        );
        let pending = ToolContext {
            pending_execution_facts: Some(crate::server::tools::PendingExecutionFacts {
                execution_facts: Some(facts),
            }),
            ..fresh.clone()
        };
        assert!(
            !runner::execute_tool_calls(
                &registry,
                &pending,
                std::slice::from_ref(&call),
                &crate::server::tools::injection::InjectionScan::default()
            )
            .await[0]
                .failed
        );
        assert!(
            !runner::execute_tool_calls(
                &registry,
                &fresh,
                std::slice::from_ref(&call),
                &crate::server::tools::injection::InjectionScan::default()
            )
            .await[0]
                .failed
        );
        assert!(fresh.pending_execution_facts.is_none());
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT value FROM app_settings WHERE key='identity_write'"
            )
            .fetch_one(&fresh.db)
            .await
            .unwrap(),
            "2"
        );
    }

    #[test]
    fn it_presents_as_the_tool_it_wraps() {
        assert_eq!(gated().id(), Echo.id());
        assert_eq!(gated().schema(), Echo.schema());
    }

    #[tokio::test]
    async fn on_the_chat_path_a_call_suspends_for_approval() {
        let body = gated()
            .run(ctx(Suspend::Available).await, json!({"message": "hi"}))
            .await
            .unwrap();
        let request = extract_suspend(&body).expect("a suspend request");
        assert_eq!(request.kind, SuspensionKind::Approval);
        assert_eq!(request.timeout_secs, 90);
        assert_eq!(request.on_timeout, TimeoutFallback::Deny);
    }

    #[tokio::test]
    async fn an_approved_call_runs_the_wrapped_tool() {
        let body = gated()
            .run(
                ctx(Suspend::Decided(
                    SuspensionKind::Approval,
                    Decision::AllowOnce,
                ))
                .await,
                json!({"message": "hi"}),
            )
            .await
            .unwrap();
        assert_eq!(body, json!({"message": "hi"}));
    }

    #[tokio::test]
    async fn without_an_approval_it_never_runs() {
        for suspend in [
            Suspend::Unavailable,
            Suspend::Decided(
                SuspensionKind::Approval,
                Decision::Deny {
                    reason: DenyReason::User,
                },
            ),
        ] {
            let result = gated()
                .run(ctx(suspend).await, json!({"message": "hi"}))
                .await;
            assert!(matches!(result, Err(ToolError::Failed(_))), "{result:?}");
        }
    }
}
