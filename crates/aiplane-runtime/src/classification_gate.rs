//! What a conversation's level means for the tools it can call.
//!
//! A tool that can send data out of AIplane ([`Tool::egress`]) is a destination
//! with a maximum level, set per tool, per MCP server or per connector. Tools
//! whose data stays inside the plane are never filtered or gated. Once the conversation is above it, the tool is either
//! hidden or gated, as its assignment says. A gated tool stays callable: only
//! its arguments leave the conversation, so they are checked first (patterns,
//! word lists and the System One model, for the classes above the tool's level)
//! and a match asks the person or refuses, as the policy says.

use std::sync::Arc;
use std::time::Duration;

use aiplane_core::server::classification::{
    self, ClassificationMode, DestinationKind, GateOnMatch, Over, Scheme,
};
use serde_json::Value;
use session_core::i18n::{Lang, args, t_args};
use shared::api::ToolDef;

use crate::classification_detector::{Finding, text_findings};
use crate::server::AppState;
use crate::server::tools::ask_first::{Approval, DEFAULT_APPROVAL_TIMEOUT, approval};
use crate::server::tools::mcp::MCP_ID_PREFIX;
use crate::server::tools::{
    Tool, ToolAction, ToolActionFuture, ToolContext, ToolError, ToolFuture, ToolPhase, ToolSource,
};
use crate::suspend::{Suspend, SuspendRequest};
use session_core::db::SuspensionKind;

/// The level a tool is cleared for and what happens above it. An MCP tool
/// takes its connector's assignment, else its server's; a tool without an
/// assignment is cleared for the lowest level and hidden above it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolClearance {
    pub rank: u32,
    pub over: Over,
}

pub fn clearance(scheme: &Scheme, tool_id: &str) -> ToolClearance {
    let assigned = match tool_id
        .strip_prefix(MCP_ID_PREFIX)
        .and_then(|rest| rest.split("__").next())
    {
        Some(server) => scheme
            .assignment(DestinationKind::Connector, server)
            .or_else(|| scheme.assignment(DestinationKind::Mcp, server)),
        None => scheme.assignment(DestinationKind::Tool, tool_id),
    };
    ToolClearance {
        rank: scheme
            .level_or_lowest(assigned.map(|assignment| assignment.level_id.as_str()))
            .rank,
        over: assigned.map_or(Over::Hide, |assignment| assignment.over),
    }
}

/// The conversation's rank. A level that cannot be read counts as the
/// highest: a tool is never opened up by a database error.
pub async fn conversation_rank(ctx: &ToolContext, scheme: &Scheme, session_id: &str) -> u32 {
    match classification::level_id(&ctx.db, session_id).await {
        Ok(stored) => scheme.level_or_lowest(stored.as_deref()).rank,
        Err(error) => {
            tracing::warn!(session_id, error = %error, "could not read the conversation level; treating it as the highest");
            scheme.highest().rank
        }
    }
}

/// Drop the tools a conversation at `rank` may not see: tools that can send
/// data out of AIplane, above their clearance and set to be hidden. Gated
/// tools stay, and so does every tool whose data stays inside the plane.
pub fn retain_visible(
    scheme: &Scheme,
    rank: u32,
    offered: &mut Vec<String>,
    source: &dyn ToolSource,
) {
    offered.retain(|id| {
        let clearance = clearance(scheme, id);
        rank <= clearance.rank
            || clearance.over == Over::Gate
            || source.get(id).is_some_and(|tool| !tool.egress())
    });
}

/// A person's turn source with the classification applied to every tool it
/// resolves. Without an active scheme it is the source it wraps.
pub struct ClassifiedToolSource<'a> {
    inner: &'a dyn ToolSource,
    state: Arc<AppState>,
}

impl<'a> ClassifiedToolSource<'a> {
    pub fn new(inner: &'a dyn ToolSource, state: Arc<AppState>) -> Self {
        Self { inner, state }
    }
}

impl ToolSource for ClassifiedToolSource<'_> {
    fn personal_call_audit_is_deferred(&self) -> bool {
        self.inner.personal_call_audit_is_deferred()
    }

    fn get(&self, id: &str) -> Option<Arc<dyn Tool>> {
        let tool = self.inner.get(id)?;
        let scheme = self.state.scheme();
        if !scheme.is_active() || !tool.egress() {
            return Some(tool);
        }
        Some(Arc::new(ClassifiedTool {
            clearance: clearance(&scheme, id),
            scheme,
            state: self.state.clone(),
            inner: tool,
        }))
    }

    fn defs_for(&self, allowed: &[String]) -> Vec<ToolDef> {
        self.inner.defs_for(allowed)
    }

    fn ids(&self) -> Vec<String> {
        self.inner.ids()
    }

    fn contains(&self, id: &str) -> bool {
        self.inner.contains(id)
    }

    fn phase(&self, id: &str) -> ToolPhase {
        self.inner.phase(id)
    }

    fn state_written(&self) {
        self.inner.state_written();
    }
}

struct ClassifiedTool {
    inner: Arc<dyn Tool>,
    state: Arc<AppState>,
    scheme: Arc<Scheme>,
    clearance: ToolClearance,
}

impl ClassifiedTool {
    fn message(&self, key: &str, pairs: Vec<(&'static str, String)>) -> String {
        t_args(
            Lang::En,
            key,
            &args(pairs.into_iter().map(|(name, value)| (name, value.into()))),
        )
    }

    async fn checked(&self, ctx: &ToolContext, call: &Value) -> Result<Gate, ToolError> {
        let id = self.inner.id();
        let findings = text_findings(&self.state, &call.to_string(), self.clearance.rank)
            .await
            .map_err(|error| {
                tracing::warn!(tool = id, error = %error, "the gate could not check a tool call; blocking it");
                ToolError::Failed(self.message(
                    "cls-gate-unavailable",
                    vec![("tool", id.to_owned())],
                ))
            })?;
        let classes = self.class_names(&findings);
        if classes.is_empty() {
            return Ok(Gate::Clear);
        }
        tracing::info!(tool = id, classes = %classes, session_id = ?ctx.session_id, "a gated tool call matched data classes above the tool's level");
        Ok(Gate::Matched(classes))
    }

    fn level_name(&self, rank: u32) -> String {
        self.scheme
            .levels()
            .iter()
            .find(|level| level.rank == rank)
            .map_or_else(String::new, |level| level.name.clone())
    }

    fn class_names(&self, findings: &[Finding]) -> String {
        let mut names: Vec<&str> = Vec::new();
        for finding in findings {
            let Some(class) = finding
                .class_id
                .as_deref()
                .and_then(|id| self.scheme.classes().iter().find(|class| class.id == id))
            else {
                continue;
            };
            if !names.contains(&class.name.as_str()) {
                names.push(&class.name);
            }
        }
        names.join(", ")
    }
}

enum Gate {
    Clear,
    Matched(String),
}

impl Tool for ClassifiedTool {
    fn execution_facts(&self) -> Option<Value> {
        self.inner.execution_facts()
    }

    fn phase(&self) -> ToolPhase {
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

    fn run<'a>(&'a self, ctx: ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let Some(session_id) = ctx.session_id.clone() else {
                return self.inner.run(ctx, args).await;
            };
            let rank = conversation_rank(&ctx, &self.scheme, &session_id).await;
            if rank <= self.clearance.rank {
                return self.inner.run(ctx, args).await;
            }
            let id = self.inner.id().to_owned();
            if self.clearance.over == Over::Hide {
                return Err(ToolError::Failed(self.message(
                    "cls-tool-hidden",
                    vec![("tool", id), ("level", self.level_name(rank))],
                )));
            }
            let config = self.state.config().classification.clone();
            let gate = match self.checked(&ctx, &args).await {
                Err(_) if config.mode == ClassificationMode::Monitor => Gate::Clear,
                other => other?,
            };
            let Gate::Matched(classes) = gate else {
                return self.inner.run(ctx, args).await;
            };
            if config.mode == ClassificationMode::Monitor {
                return self.inner.run(ctx, args).await;
            }
            if config.gate_on_match == GateOnMatch::Block {
                return Err(ToolError::Failed(self.message(
                    "cls-gate-blocked",
                    vec![("tool", id), ("classes", classes)],
                )));
            }
            if matches!(
                ctx.suspend,
                Suspend::Decided(kind, _) if kind != SuspensionKind::Approval
            ) {
                // The call resumes after a pause of the inner tool, which only
                // ran once this gate had granted the same arguments.
                return self.inner.run(ctx, args).await;
            }
            let mut request = SuspendRequest::approval(DEFAULT_APPROVAL_TIMEOUT);
            request.message = (!classes.is_empty()).then_some(classes);
            match approval(&ctx, &id, request)? {
                Approval::Granted => self.inner.run(ctx, args).await,
                Approval::Ask(pause) => Ok(pause),
            }
        })
    }

    fn egress(&self) -> bool {
        self.inner.egress()
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
    use aiplane_core::server::classification::{Assignment, Level};

    fn level(id: &str, rank: u32) -> Level {
        Level {
            id: id.into(),
            rank,
            name: id.into(),
            description: String::new(),
            color: "neutral".into(),
            marking_text: String::new(),
            mip_label_id: String::new(),
            filename_tag: String::new(),
        }
    }

    fn assignment(kind: DestinationKind, id: &str, level: &str, over: Over) -> Assignment {
        Assignment {
            kind,
            id: id.into(),
            level_id: level.into(),
            over,
        }
    }

    fn scheme() -> Scheme {
        Scheme::new(
            vec![level("public", 0), level("internal", 1), level("secret", 2)],
            Vec::new(),
        )
        .with_assignments(vec![
            assignment(DestinationKind::Tool, "edit_image", "internal", Over::Hide),
            assignment(DestinationKind::Tool, "search_web", "public", Over::Gate),
            assignment(DestinationKind::Mcp, "wiki", "internal", Over::Hide),
            assignment(DestinationKind::Connector, "wiki", "secret", Over::Gate),
        ])
    }

    struct Probe {
        id: &'static str,
        egress: bool,
    }

    impl Tool for Probe {
        fn id(&self) -> &str {
            self.id
        }
        fn schema(&self) -> ToolDef {
            crate::server::tools::echo::Echo.schema()
        }
        fn egress(&self) -> bool {
            self.egress
        }
        fn run<'a>(&'a self, _ctx: ToolContext, _args: Value) -> ToolFuture<'a> {
            Box::pin(async { Ok(Value::Null) })
        }
    }

    fn offered(scheme: &Scheme, rank: u32, ids: &[&str]) -> Vec<String> {
        let registry = crate::server::tools::ToolRegistry::new()
            .with(Probe {
                id: "edit_image",
                egress: true,
            })
            .with(Probe {
                id: "search_web",
                egress: true,
            })
            .with(Probe {
                id: "list_attachments",
                egress: false,
            });
        let mut offered: Vec<String> = ids.iter().map(|id| (*id).to_owned()).collect();
        retain_visible(scheme, rank, &mut offered, &registry);
        offered
    }

    #[test]
    fn a_tool_whose_data_stays_inside_the_plane_is_never_hidden() {
        assert_eq!(
            offered(&scheme(), 2, &["list_attachments"]),
            ["list_attachments"]
        );
    }

    #[test]
    fn a_tool_without_an_assignment_is_cleared_for_the_lowest_level_and_hidden_above_it() {
        assert_eq!(
            clearance(&scheme(), "qr_code"),
            ToolClearance {
                rank: 0,
                over: Over::Hide
            }
        );
    }

    #[test]
    fn image_editing_is_filtered_like_any_other_tool() {
        let scheme = scheme();
        assert_eq!(offered(&scheme, 1, &["edit_image"]), ["edit_image"]);
        assert!(offered(&scheme, 2, &["edit_image"]).is_empty());
    }

    #[test]
    fn a_gated_tool_stays_offered_above_its_level() {
        assert_eq!(offered(&scheme(), 2, &["search_web"]), ["search_web"]);
    }

    #[test]
    fn an_mcp_tool_takes_its_connectors_assignment_before_its_servers() {
        let scheme = scheme();
        assert_eq!(
            clearance(&scheme, "mcp__wiki__search"),
            ToolClearance {
                rank: 2,
                over: Over::Gate
            }
        );
        let server_only = Scheme::new(vec![level("public", 0), level("internal", 1)], Vec::new())
            .with_assignments(vec![assignment(
                DestinationKind::Mcp,
                "wiki",
                "internal",
                Over::Hide,
            )]);
        assert_eq!(clearance(&server_only, "mcp__wiki__search").rank, 1);
    }

    #[test]
    fn what_is_offered_follows_the_level_of_each_round() {
        let scheme = scheme();
        let ids = ["edit_image", "search_web"];
        assert_eq!(offered(&scheme, 0, &ids), ids);
        assert_eq!(offered(&scheme, 2, &ids), ["search_web"]);
    }
}
