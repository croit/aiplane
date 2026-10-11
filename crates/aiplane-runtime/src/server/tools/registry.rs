// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use std::collections::HashMap;
use std::sync::Arc;

use shared::api::{ToolDef, ToolSummary};

use super::Tool;

/// Holds every registered tool in an Arc so handlers can fan out to N tools
/// concurrently without cloning each impl.
#[derive(Default)]
pub struct ToolRegistry {
    // Keyed on an owned `String` rather than `&'static str`: built-in tools
    // return string literals from `id()`, but MCP-bridged tools (see
    // `tools::mcp`) carry runtime-built ids (`mcp__<server>__<tool>`), so the
    // key can't be `'static`.
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a tool. Panics if a tool with the same id has already been
    /// registered — IDs are a stable contract; collisions are a bug,
    /// not a runtime decision.
    ///
    /// Also asserts the id matches OpenAI's function-name regex
    /// (`^[a-zA-Z0-9_-]{1,64}$`). Tools whose id violates it (most
    /// commonly: a `.` in a dotted-namespace id like `company.echo`)
    /// silently break against strict tool-call parsers — qwen3-coder
    /// is the one that bit us — because the parser either drops the
    /// call entirely or rewrites the name before emitting `tool_calls`.
    /// Catching this at registration means we can't ship a tool that
    /// randomly fails on some upstreams. Use `_` for namespacing
    /// (`company_echo`).
    pub fn with<T: Tool>(mut self, tool: T) -> Self {
        let id = tool.id().to_string();
        assert!(
            is_openai_function_name(&id),
            "tool id `{id}` is not a valid OpenAI function name \
             (`^[a-zA-Z0-9_-]{{1,64}}$`); replace `.` with `_`"
        );
        assert!(
            !self.tools.contains_key(&id),
            "duplicate tool id `{id}` in ToolRegistry"
        );
        self.tools.insert(id, Arc::new(tool));
        self
    }

    /// A copy of this registry with every tool whose id starts with `prefix`
    /// removed, then `replacements` inserted.
    ///
    /// The one operation that lets a *settings-dependent tool family* be
    /// rebuilt without rebuilding the whole registry. Typst registers one
    /// concrete tool per discovered template, so pointing
    /// `typst.templates_dir` somewhere else changes which tools should exist —
    /// and a per-request filter cannot invent a tool that was never
    /// registered. Swapping the family is the narrow alternative to
    /// reconstructing all ~40 tools from the boot path.
    ///
    /// Unlike [`Self::with`] this does not panic on a duplicate id: the whole
    /// point is to replace ids that already exist.
    pub fn with_family_replaced(&self, prefix: &str, replacements: Vec<Arc<dyn Tool>>) -> Self {
        let mut tools: HashMap<String, Arc<dyn Tool>> = self
            .tools
            .iter()
            .filter(|(id, _)| !id.starts_with(prefix))
            .map(|(id, tool)| (id.clone(), tool.clone()))
            .collect();
        for tool in replacements {
            let id = tool.id().to_string();
            debug_assert!(
                id.starts_with(prefix),
                "replacement tool `{id}` does not belong to the `{prefix}` family"
            );
            tools.insert(id, tool);
        }
        Self { tools }
    }

    /// A registry containing only the requested tools that are registered.
    pub fn with_only(&self, ids: &[&str]) -> Self {
        Self {
            tools: ids
                .iter()
                .filter_map(|id| {
                    self.tools
                        .get(*id)
                        .map(|tool| ((*id).to_string(), tool.clone()))
                })
                .collect(),
        }
    }

    pub fn get(&self, id: &str) -> Option<&Arc<dyn Tool>> {
        self.tools.get(id)
    }

    pub fn contains(&self, id: &str) -> bool {
        self.tools.contains_key(id)
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.tools.keys().map(String::as_str)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &Arc<dyn Tool>)> {
        self.tools.iter().map(|(k, v)| (k.as_str(), v))
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// Collects the OpenAI-shaped definitions for every tool whose id appears
    /// in `allowed`. Order matches the iteration order of `allowed` so callers
    /// can present them deterministically.
    pub fn defs_for(&self, allowed: &[String]) -> Vec<ToolDef> {
        allowed
            .iter()
            .filter_map(|id| self.tools.get(id.as_str()))
            .map(|t| t.schema())
            .collect()
    }

    /// UI-friendly view: id + display name + description for every tool whose
    /// id is in `allowed`. The name comes from the OpenAI schema; for the
    /// majority of tools `id` and `schema().function.name` are the same
    /// string.
    pub fn summaries_for(&self, allowed: &[String]) -> Vec<ToolSummary> {
        allowed
            .iter()
            .filter_map(|id| self.tools.get(id.as_str()).map(|t| (id, t.schema())))
            .map(|(id, def)| ToolSummary {
                id: id.clone(),
                name: def.function.name,
                description: def.function.description,
            })
            .collect()
    }
}

/// A source of tool definitions and implementations the tool-call runner can
/// resolve against. Implemented by the static [`ToolRegistry`] and — once
/// per-user MCP connectors land — by per-request composites that overlay a
/// user's connected-connector tools on top of the registry. The runner
/// ([`super::runner`]) takes `&dyn ToolSource` so the buffered `/v1`,
/// streaming `/v1`, and chat-UI loops all gain per-user tools at one seam.
pub trait ToolSource: Send + Sync {
    fn personal_call_audit_is_deferred(&self) -> bool {
        false
    }

    /// Resolve a tool by id, returning an owned handle. The registry holds
    /// `Arc`s; an overlay may mint them per request.
    fn get(&self, id: &str) -> Option<Arc<dyn Tool>>;

    /// OpenAI-shaped definitions for every id in `allowed` this source knows,
    /// in `allowed` order.
    fn defs_for(&self, allowed: &[String]) -> Vec<ToolDef>;

    /// Every tool id known to this source. Used for diagnostics only.
    fn ids(&self) -> Vec<String>;

    /// Whether `id` resolves in this source.
    fn contains(&self, id: &str) -> bool {
        self.get(id).is_some()
    }

    /// When a call to `id` runs relative to the other calls of its round.
    fn phase(&self, id: &str) -> ToolPhase {
        self.get(id)
            .map_or(ToolPhase::Concurrent, |tool| tool.phase())
    }

    /// A call of [`ToolPhase::WritesState`] just ran, so whatever this source
    /// holds of the run's state is stale.
    fn state_written(&self) {}
}

pub fn action_catalogues(
    source: &dyn ToolSource,
) -> std::collections::BTreeMap<String, std::collections::BTreeSet<String>> {
    source
        .ids()
        .into_iter()
        .filter_map(|id| {
            let tool = source.get(&id)?;
            let catalogue = tool.action_catalogue()?;
            Some((
                id,
                catalogue
                    .iter()
                    .map(|action| action.id.to_string())
                    .collect(),
            ))
        })
        .collect()
}

/// The order a round's calls run in. Phases run one after the other, each
/// finished before the next starts; every result still answers its own call,
/// in the order the model made the calls.
///
/// Only an agent run's run-scoped tools leave [`ToolPhase::Concurrent`]: they
/// write the run's state, act on it or end the run, and a model batches them
/// freely, so a forward made in the round that fills its slots must see them
/// filled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum ToolPhase {
    /// Writes the run's state (`set_<slot>`, a verifier). Runs first, one call
    /// at a time in call order, since a verifier reads what a `set_<slot>`
    /// before it wrote.
    WritesState,
    /// Every other tool, concurrently, once the state is written: a bound
    /// argument may come from state too.
    #[default]
    Concurrent,
    /// Acts on the state as the round left it (`forward_request`,
    /// `request_human`). Runs one call at a time in call order.
    ActsOnState,
    /// Ends the run (`finish`). The driver runs such a call only as the one
    /// call of its round — next to others it is refused, since ending the
    /// run would throw their results away unread — and offers nothing else on
    /// a contracted run's final round. A call that succeeds ends the turn; one
    /// that fails is answered like any failed call, and the run goes on.
    Terminal,
}

impl ToolPhase {
    pub const ORDER: [ToolPhase; 4] = [
        Self::WritesState,
        Self::Concurrent,
        Self::ActsOnState,
        Self::Terminal,
    ];

    /// Whether the calls of this phase run one at a time.
    pub fn is_sequential(self) -> bool {
        self != Self::Concurrent
    }
}

impl ToolSource for ToolRegistry {
    fn get(&self, id: &str) -> Option<Arc<dyn Tool>> {
        ToolRegistry::get(self, id).cloned()
    }

    fn defs_for(&self, allowed: &[String]) -> Vec<ToolDef> {
        ToolRegistry::defs_for(self, allowed)
    }

    fn ids(&self) -> Vec<String> {
        ToolRegistry::ids(self).map(str::to_owned).collect()
    }

    fn contains(&self, id: &str) -> bool {
        ToolRegistry::contains(self, id)
    }
}

/// A source narrowed to the tools the acting principal is granted. The chat
/// driver resolves and runs every call through one, so a tool outside the
/// grant does not exist for the turn — not to offer, not to auto-enable, not to
/// run — whatever name the model calls.
///
/// It narrows the *grantable* tools only. A run-scoped synthetic tool an agent
/// run brings along (`finish`, an `agents::slot_tools::SlotTools` `set_<slot>`)
/// is nobody's grant: its existence is the run's permission, so it is composed
/// over this source, never wrapped by it.
pub struct GrantedToolSource<'a> {
    source: &'a dyn ToolSource,
    granted: std::collections::HashSet<String>,
}

impl<'a> GrantedToolSource<'a> {
    pub fn new(source: &'a dyn ToolSource, granted: impl IntoIterator<Item = String>) -> Self {
        Self {
            source,
            granted: granted.into_iter().collect(),
        }
    }
}

impl ToolSource for GrantedToolSource<'_> {
    fn personal_call_audit_is_deferred(&self) -> bool {
        self.source.personal_call_audit_is_deferred()
    }

    fn phase(&self, id: &str) -> ToolPhase {
        if self.get(id).is_some() {
            self.source.phase(id)
        } else {
            ToolPhase::Concurrent
        }
    }

    fn get(&self, id: &str) -> Option<Arc<dyn Tool>> {
        if self.granted.contains(id) {
            self.source.get(id)
        } else {
            None
        }
    }

    fn defs_for(&self, allowed: &[String]) -> Vec<ToolDef> {
        let allowed: Vec<String> = allowed
            .iter()
            .filter(|id| self.granted.contains(*id))
            .cloned()
            .collect();
        self.source.defs_for(&allowed)
    }

    fn ids(&self) -> Vec<String> {
        self.source
            .ids()
            .into_iter()
            .filter(|id| self.granted.contains(id))
            .collect()
    }
}

/// The character class OpenAI allows in a function name (and thus a tool
/// id): ASCII alphanumerics, `_`, `-`. Shared with the MCP id sanitizer
/// (`tools::mcp::sanitize_tool_id`) so a sanitized id can never fail the
/// validation below.
pub(crate) fn is_openai_function_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

/// True if `id` matches OpenAI's `^[a-zA-Z0-9_-]{1,64}$` for tool /
/// function names. Hand-rolled instead of pulling `regex` — the rule
/// is one line and the comparison runs at registration only.
fn is_openai_function_name(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(is_openai_function_name_char)
}

/// Lets the RBAC resolver filter grants against the registry without depending
/// on it — see [`GrantableSet`](aiplane_core::server::rbac::resolver::GrantableSet).
impl aiplane_core::server::rbac::resolver::GrantableSet for ToolRegistry {
    fn ids(&self) -> impl Iterator<Item = &str> {
        self.ids()
    }

    fn has(&self, id: &str) -> bool {
        self.contains(id)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn tool_phases_survive_registry_and_grants_without_exposing_denied_tools() {
        let registry = ToolRegistry::new().with(super::super::tests::PhasedEcho);
        assert_eq!(
            ToolSource::phase(&registry, "company_echo"),
            ToolPhase::ActsOnState
        );
        let granted = GrantedToolSource::new(&registry, ["company_echo".to_string()]);
        assert_eq!(granted.phase("company_echo"), ToolPhase::ActsOnState);
        let denied = GrantedToolSource::new(&registry, std::iter::empty());
        assert!(denied.get("company_echo").is_none());
        assert_eq!(denied.phase("company_echo"), ToolPhase::Concurrent);
        assert_eq!(granted.phase("missing"), ToolPhase::Concurrent);
    }
    use super::*;
    use crate::server::tools::echo::Echo;
    use crate::server::tools::time::CurrentTimestamp;

    #[test]
    fn a_granted_source_hides_every_tool_outside_the_grant() {
        let r = ToolRegistry::new().with(Echo).with(CurrentTimestamp);
        let granted = GrantedToolSource::new(&r, ["get_current_timestamp".to_string()]);

        assert!(granted.contains("get_current_timestamp"));
        assert!(!granted.contains("company_echo"));
        assert!(granted.get("company_echo").is_none());
        assert_eq!(granted.ids(), vec!["get_current_timestamp".to_string()]);
        let defs = granted.defs_for(&["company_echo".into(), "get_current_timestamp".into()]);
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].function.name, "get_current_timestamp");
    }

    #[test]
    fn action_catalogues_include_only_source_declared_and_granted_actions() {
        let client = crate::server::tools::sandbox::SandboxClient::new(
            Arc::new(aiplane_core::server::config::SandboxConfig {
                enabled: true,
                runner_url: "http://127.0.0.1:9".into(),
                timeout_secs: 1,
                max_artifact_bytes: 1024,
            }),
            crate::server::state::RuntimeSettings::new_handle("https://gw.example"),
        );
        let source = ToolRegistry::new()
            .with(Echo)
            .with(crate::server::tools::sandbox::BrowsePage(client));
        let catalogue = action_catalogues(&source);
        assert_eq!(catalogue.len(), 1);
        assert!(catalogue["browse_page"].contains("fill"));
        assert!(
            action_catalogues(&GrantedToolSource::new(&source, ["company_echo".into()])).is_empty()
        );
    }

    #[test]
    fn empty_registry() {
        let r = ToolRegistry::new();
        assert!(r.is_empty());
        assert_eq!(r.len(), 0);
        assert!(r.get("anything").is_none());
    }

    #[test]
    fn with_registers_and_lookup_works() {
        let r = ToolRegistry::new().with(Echo).with(CurrentTimestamp);
        assert_eq!(r.len(), 2);
        assert!(r.contains(Echo.id()));
        assert!(r.contains(CurrentTimestamp.id()));
        assert!(!r.contains("nope"));
    }

    #[test]
    fn with_only_copies_only_registered_requested_tools() {
        let r = ToolRegistry::new().with(Echo).with(CurrentTimestamp);
        let selected = r.with_only(&["get_current_timestamp", "missing"]);
        assert_eq!(
            selected.ids().collect::<Vec<_>>(),
            ["get_current_timestamp"]
        );
        assert!(!selected.contains("company_echo"));
        assert!(r.contains("company_echo"));
    }

    #[test]
    #[should_panic(expected = "duplicate tool id")]
    fn duplicate_id_panics() {
        let _ = ToolRegistry::new().with(Echo).with(Echo);
    }

    #[test]
    fn defs_for_returns_only_allowed_in_order() {
        let r = ToolRegistry::new().with(Echo).with(CurrentTimestamp);
        let defs = r.defs_for(&["get_current_timestamp".into(), "company_echo".into()]);
        assert_eq!(defs.len(), 2);
        assert_eq!(defs[0].function.name, "get_current_timestamp");
        assert_eq!(defs[1].function.name, "company_echo");
    }

    #[test]
    fn defs_for_silently_skips_unknown_ids() {
        let r = ToolRegistry::new().with(Echo);
        let defs = r.defs_for(&["company_echo".into(), "nope".into()]);
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].function.name, "company_echo");
    }

    #[test]
    fn summaries_carry_description() {
        let r = ToolRegistry::new().with(Echo);
        let summaries = r.summaries_for(&["company_echo".into()]);
        assert_eq!(summaries.len(), 1);
        assert!(!summaries[0].description.is_empty());
    }
}
