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

use super::{Tool, ToolContext, ToolError, ToolFuture};
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

impl Tool for AskFirst {
    fn id(&self) -> &str {
        self.inner.id()
    }

    fn schema(&self) -> ToolDef {
        self.inner.schema()
    }

    fn run<'a>(&'a self, ctx: ToolContext, args: Value) -> ToolFuture<'a> {
        match approval(&ctx, self.id(), SuspendRequest::approval(self.timeout)) {
            Ok(Approval::Granted) => self.inner.run(ctx, args),
            Ok(Approval::Ask(pause)) => Box::pin(async move { Ok(pause) }),
            Err(refusal) => Box::pin(async move { Err(refusal) }),
        }
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
