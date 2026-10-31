// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! What sets a person's turn apart from an agent's run, in one place.
//!
//! The round loop in [`super::run_one_turn`] is the same for both: it asks the
//! [`TurnPolicy`] for the leading system message, the round's tool offer, the
//! pools, budget and injection scan, and how the turn ends, and never asks
//! whether it is running an agent. See docs/architecture.md
//! (`openai_driver/turn_policy.rs`) and docs/agent-runs.md → "Entry points".
//!
//! An enum rather than a trait object: the set is closed (a turn acts for a
//! person or is an agent's run, exactly as [`crate::agent_run::Actor`] says),
//! the agent variant only borrows the [`AgentRun`] the driver already holds,
//! and the methods are async — a `dyn` policy would need boxing and
//! `async_trait` lifetimes for no extension anyone can make. A third kind of
//! turn is a new variant here, not an edit to the loop.

use std::collections::{BTreeMap, HashSet};

use aiplane_core::server::prompts::CHAT_PROMPT;
use serde_json::Value;

use super::{
    Audience, Leading, OpenAiDriver, TurnEnd, VOICE_DIRECTIVE, build_request_context, run_outcome,
};
use crate::agent_run::AgentRun;
use crate::agents::profile::{AgentSurface, RunToolSource};
use crate::agents::topic_guard::Decision;
use crate::budget::Budget;
use crate::finish::{FINISH_NUDGE, FinishTool};
use crate::persona::{ChatPersona, PersonaToolSource};
use crate::server::tools::catalog::READ_SKILL_ID;
use crate::server::tools::injection::InjectionScan;
use crate::server::tools::mcp::manager::UserMcpLayer;
use crate::server::tools::runner::{self, ToolCallAcc};
use crate::server::tools::{ToolContext, ToolPhase, ToolSource};
use aiplane_core::server::db::chat_session_tools;
use aiplane_core::server::reasoning::Effort;
use aiplane_core::server::upstreams::PoolAccess;
use session_core::driver::TurnError;

/// Who the turn is for, and so what it is offered and how it ends.
#[derive(Clone, Copy)]
pub(super) enum TurnPolicy<'a> {
    /// A person's turn: the chat rules, their own context, and the tools the
    /// conversation has turned on.
    Chat,
    /// An agent's run: the spec's system message, its principal's grants
    /// (narrowed to the spec's tools) with the run's synthetic tools over
    /// them, and, under a contract, an end only through `finish`.
    Agent(&'a AgentRun),
    /// A person's turn in a built-in persona's conversation: the person's
    /// pools, budget and usage, but the persona's system prompt and only its
    /// tools, all offered every round.
    Persona(&'a ChatPersona),
}

/// What the round that must end the turn does with the calls the model made
/// on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FinalRound {
    /// None of them runs: the reply written alongside is the answer.
    Answers,
    /// The terminal calls run, and nothing else; whether one is accepted
    /// decides how the run ends.
    RunsTerminal,
    /// No terminal call came, so the run ends incomplete.
    Incomplete,
}

pub(super) enum TurnToolSource<'a> {
    Run(RunToolSource<'a>),
    Persona(PersonaToolSource<'a>),
}

impl TurnToolSource<'_> {
    fn source(&self) -> &dyn ToolSource {
        match self {
            Self::Run(source) => source,
            Self::Persona(source) => source,
        }
    }
}

impl ToolSource for TurnToolSource<'_> {
    fn personal_call_audit_is_deferred(&self) -> bool {
        self.source().personal_call_audit_is_deferred()
    }

    fn get(&self, id: &str) -> Option<std::sync::Arc<dyn crate::server::tools::Tool>> {
        self.source().get(id)
    }

    fn defs_for(&self, allowed: &[String]) -> Vec<shared::api::ToolDef> {
        self.source().defs_for(allowed)
    }

    fn ids(&self) -> Vec<String> {
        self.source().ids()
    }

    fn contains(&self, id: &str) -> bool {
        self.source().contains(id)
    }

    fn phase(&self, id: &str) -> ToolPhase {
        self.source().phase(id)
    }

    fn state_written(&self) {
        self.source().state_written();
    }
}

impl<'a> TurnPolicy<'a> {
    pub(super) fn of(d: &'a OpenAiDriver) -> Self {
        match (d.agent(), d.persona.as_deref()) {
            (Some(run), _) => Self::Agent(run),
            (None, Some(persona)) => Self::Persona(persona),
            (None, None) => Self::Chat,
        }
    }

    fn surface(self) -> Option<&'a AgentSurface> {
        self.agent().and_then(AgentRun::surface)
    }

    fn finish(self) -> Option<&'a FinishTool> {
        self.agent().and_then(AgentRun::finish)
    }

    fn agent(self) -> Option<&'a AgentRun> {
        match self {
            Self::Agent(run) => Some(run),
            Self::Chat | Self::Persona(_) => None,
        }
    }

    /// `granted` with the run's synthetic and terminal tools layered over it.
    /// A person's turn gets `granted` unchanged; a persona's turn gets the
    /// persona's tools instead.
    pub(super) fn tool_source<'s>(
        self,
        granted: &'s dyn ToolSource,
        state: &std::sync::Arc<crate::server::AppState>,
    ) -> TurnToolSource<'s>
    where
        'a: 's,
    {
        match self {
            Self::Persona(persona) => TurnToolSource::Persona(persona.source(granted, Some(state))),
            _ => TurnToolSource::Run(RunToolSource::new(granted, self.agent())),
        }
    }

    /// What this turn's rounds may route to.
    pub(super) fn access(self, d: &OpenAiDriver) -> PoolAccess {
        match self.surface() {
            Some(surface) => surface.models().clone(),
            None => d.state.pool_access_for_principal(&d.tool_ctx.principal),
        }
    }

    /// What compacting this conversation may use.
    pub(super) fn compaction_access(self) -> PoolAccess {
        self.surface()
            .map_or_else(PoolAccess::all, |surface| surface.models().clone())
    }

    pub(super) fn budget(self, effort: Effort) -> Budget {
        self.agent()
            .and_then(AgentRun::budget)
            .unwrap_or_else(|| Budget::from_effort(effort))
    }

    pub(super) fn injection(self) -> InjectionScan {
        self.agent()
            .map(AgentRun::injection)
            .cloned()
            .unwrap_or_default()
    }

    /// The name a usage row carries: the person's email, the agent's name.
    pub(super) async fn usage_name(self, d: &OpenAiDriver) -> String {
        match self {
            Self::Agent(run) => run.system_principal().name.clone(),
            Self::Chat | Self::Persona(_) => aiplane_core::server::db::users::find_by_id(
                &d.state.db,
                d.tool_ctx.principal.subject_id(),
            )
            .await
            .ok()
            .flatten()
            .map(|u| u.email)
            .unwrap_or_default(),
        }
    }

    /// Who a model call made beside this turn is billed to and limited as.
    pub(super) async fn payer(self, d: &OpenAiDriver) -> crate::server::side_call::Payer {
        let email = if d.state.usage.is_enabled() {
            Some(self.usage_name(d).await).filter(|name| !name.is_empty())
        } else {
            None
        };
        crate::server::side_call::Payer::of_turn(
            &d.tool_ctx.principal,
            d.agent().map(|run| run.chain().clone()),
            email,
            d.source,
        )
        .with_model_context(&d.tool_ctx)
    }

    /// Count `tokens` against the allowance the run spends inside, if any.
    pub(super) fn record_spend(self, tokens: u64) {
        if let Some(surface) = self.surface() {
            surface.record_spend(tokens);
        }
    }

    /// The single leading system message. Built here and nowhere else, so the
    /// turn's first round and every refresh agree on its shape.
    ///
    /// For a person it combines the turn-discipline rule, the voice directive,
    /// the request context and the compaction summary; for an agent run it is
    /// the spec's message (or just the turn-discipline rule, for a run with no
    /// spec), with the finish contract's instructions under a contract. Kept
    /// to one message: some backends reject more than one leading `system`
    /// turn.
    pub(super) async fn system_message(
        self,
        d: &OpenAiDriver,
        session_id: &str,
        user_mcp: &UserMcpLayer,
        summary: Option<&str>,
    ) -> Value {
        let config = d.state.config();
        let leading = match self {
            Self::Chat => Leading {
                voice_directive: d.voice_mode.then_some(voice_directive(&config)),
                own: chat_own_sections(&config),
                context: build_request_context(d, user_mcp, Audience::Person).await,
                summary,
            },
            Self::Agent(run) => Leading {
                own: match run.surface() {
                    Some(surface) => {
                        let mut sections = surface.system_sections(&d.state.db, session_id).await;
                        match crate::server::tools::memory::organization_standing_context(run).await
                        {
                            Ok(Some(memory)) => sections.push(memory),
                            Ok(None) => {}
                            Err(error) => {
                                tracing::warn!(%error, "Organization standing memory unavailable")
                            }
                        }
                        sections
                    }
                    None => Vec::new(),
                },
                context: build_request_context(d, user_mcp, Audience::Agent).await,
                summary,
                ..Leading::default()
            },
            Self::Persona(persona) => Leading {
                own: vec![persona.instructions().to_string()],
                context: if persona.includes_user_context() {
                    build_request_context(d, user_mcp, Audience::Person).await
                } else {
                    None
                },
                summary,
                ..Leading::default()
            },
        };
        let base = super::leading_system_message(leading);
        let mut leading = vec![base];
        if let Some(finish) = self.finish() {
            runner::merge_into_leading_system_message(
                &mut leading,
                finish.contract().instructions(),
            );
        }
        leading.swap_remove(0)
    }

    /// The topic guard's decision on the visitor's message: for a main
    /// agent with a strict scope only, `None` for every other turn. Asked
    /// once, before the turn's first model call, and never on a resume —
    /// the message was judged when the turn began.
    pub(super) async fn guard_topic(
        self,
        d: &OpenAiDriver,
        tool_ctx: &ToolContext,
        messages: &[Value],
    ) -> Option<Decision> {
        if d.resume.is_some() {
            return None;
        }
        let run = self.agent()?;
        let guard = run.surface()?.topic_guard()?;
        Some(
            guard
                .judge(d.state.clone(), run.system_principal(), tool_ctx, messages)
                .await,
        )
    }

    /// Bring `messages[0]` up to date at the top of a round that is not the
    /// turn's first: a slot the model set last round must show, or it would
    /// ask for it again. Only an agent's conversation state changes within a
    /// turn.
    pub(super) async fn refresh_system_message(
        self,
        d: &OpenAiDriver,
        session_id: &str,
        user_mcp: &UserMcpLayer,
        summary: Option<&str>,
        messages: &mut [Value],
    ) {
        if self
            .surface()
            .is_some_and(AgentSurface::has_conversation_state)
        {
            messages[0] = self.system_message(d, session_id, user_mcp, summary).await;
        }
    }

    /// Whether the turn has any tools, as the automatic router weighs it.
    pub(super) async fn routing_has_tools(
        self,
        d: &OpenAiDriver,
        session_id: &str,
        granted: &[String],
    ) -> bool {
        match self {
            Self::Chat => {
                count_chat_only_read();
                !d.state
                    .allowed_tools_for_session(&d.tool_ctx.principal, session_id)
                    .await
                    .is_empty()
                    || !chat_session_tools::enabled_keys_for_session(&d.state.db, session_id)
                        .await
                        .unwrap_or_default()
                        .is_empty()
            }
            Self::Agent(run) => !agent_offer(d, run, granted).is_empty(),
            Self::Persona(persona) => !persona.offered(granted).is_empty(),
        }
    }

    /// The tool ids this round offers the model.
    ///
    /// A person's turn re-resolves the conversation's overlay every round, so
    /// a mid-turn `enable_tools` call surfaces the newly-enabled schemas on
    /// the next one. An agent run is offered its grants (the spec's tools
    /// among them, when it has a spec), the run's synthetic tools and, under
    /// a contract, `finish` — the same every round.
    pub(super) async fn offer(
        self,
        d: &OpenAiDriver,
        session_id: &str,
        user_mcp: &UserMcpLayer,
        granted: &[String],
    ) -> Vec<String> {
        match self {
            Self::Chat => {
                let mut offered = chat_offer(d, session_id, user_mcp).await;
                offered.retain(|id| granted.contains(id));
                offered
            }
            Self::Agent(run) => agent_offer(d, run, granted),
            Self::Persona(persona) => persona.offered(granted),
        }
    }

    /// Tool families the person switched off for this conversation. Their
    /// schemas are never offered, but a model can still call one from its
    /// training; those calls are refused. An agent run has no such switches.
    pub(super) async fn disabled_keys(self, d: &OpenAiDriver) -> HashSet<String> {
        let (Self::Chat, Some(sid)) = (self, d.tool_ctx.session_id.as_deref()) else {
            return HashSet::new();
        };
        count_chat_only_read();
        // A DB hiccup degrades open.
        chat_session_tools::disabled_keys_for_session(&d.state.db, sid)
            .await
            .unwrap_or_default()
    }

    /// Narrow the final round's offer: a contracted run's last round offers
    /// only what can end it.
    pub(super) fn narrow_final_offer(self, offer: &mut Vec<String>, source: &dyn ToolSource) {
        if self.finish().is_some() {
            offer.retain(|id| source.phase(id) == ToolPhase::Terminal);
        }
    }

    /// Shape the request for the last round the budget allows.
    pub(super) fn prepare_final_round(
        self,
        body: &mut Value,
        honors_tool_choice: bool,
        max_rounds: u32,
    ) {
        match self.finish() {
            Some(finish) => {
                finish.contract().prepare_final_round(body);
                tracing::info!(
                    max_rounds,
                    "tool-round budget reached; offering only finish for the final round"
                );
            }
            None => {
                // Whether the definitions may stay depends on whether this
                // backend can be trusted with `tool_choice` at all.
                runner::prepare_final_round(body, honors_tool_choice);
                tracing::info!(
                    max_rounds,
                    tools_withheld = !honors_tool_choice,
                    "tool-round budget reached; requesting final answer with tool choice none"
                );
            }
        }
    }

    /// What happens to the calls of the round that must end the turn. On a
    /// contracted run only a terminal call can still run; anything else the
    /// model called there never does.
    pub(super) fn final_round_calls(
        self,
        calls: &mut BTreeMap<usize, ToolCallAcc>,
        source: &dyn ToolSource,
    ) -> FinalRound {
        if self.finish().is_none() {
            return FinalRound::Answers;
        }
        calls.retain(|_, acc| source.phase(&acc.name) == ToolPhase::Terminal);
        if calls.is_empty() {
            FinalRound::Incomplete
        } else {
            FinalRound::RunsTerminal
        }
    }

    /// A round that came back without tool calls. Under a contract the run is
    /// not over because the model stopped calling tools: its text goes back
    /// with a nudge toward `finish` and this returns `true` (the round is
    /// spent). Otherwise the reply ends the turn and nothing changes.
    pub(super) fn nudge_toward_finish(
        self,
        round_content: &str,
        messages: &mut Vec<Value>,
    ) -> bool {
        if self.finish().is_none() {
            return false;
        }
        messages.push(serde_json::json!({"role": "assistant", "content": round_content}));
        messages.push(serde_json::json!({"role": "user", "content": FINISH_NUDGE}));
        true
    }

    /// Record a contracted run's [`RunOutcome`](crate::finish::RunOutcome)
    /// from how its turn ended. Every exit of the round loop passes through
    /// here once, so none of them settles the run itself.
    pub(super) fn settle(self, end: &Result<TurnEnd, TurnError>, cancelled: bool) {
        if let Self::Agent(run) = self
            && let Some(finish) = run.finish()
        {
            run.settle(run_outcome(finish.result(), end, cancelled));
        }
    }
}

/// The voice directive for a voice-mode turn: the operator's own wording
/// while it has text, the built-in directive otherwise. Only a person's chat
/// carries it — a persona's turn is its persona's, and an agent run never
/// gets voice.
fn voice_directive(config: &aiplane_core::server::config::Config) -> &str {
    match config.chat.voice.directive.trim() {
        "" => VOICE_DIRECTIVE,
        _ => &config.chat.voice.directive,
    }
}

/// The operator's standing instructions for a person's chat, as the leading
/// message's own section: one entry while the prompt is enabled — the
/// operator's own text while it has text, the built-in prompt otherwise —
/// none while it is off. An agent run never reads it — its system message
/// comes from its spec — and a `/v1` request carries the client's own.
fn chat_own_sections(config: &aiplane_core::server::config::Config) -> Vec<String> {
    let prompt = &config.chat.prompt;
    if !prompt.enabled {
        return Vec::new();
    }
    let text = match prompt.text.trim() {
        "" => CHAT_PROMPT,
        _ => &prompt.text,
    };
    vec![text.to_string()]
}

/// An agent run's offer: its granted spec tools and synthetic tools (every
/// grant, for a run with no spec), then the run's terminal tool.
/// Whether an agent run is offered the skill loader without its spec
/// listing it: the system message lists the skills its principal is granted,
/// so the loader must be callable whenever it is granted too — the rule a
/// person's chat follows (`allowed_tools_for_session`).
fn skill_loader_on(d: &OpenAiDriver, granted: &[String]) -> bool {
    granted.iter().any(|id| id == READ_SKILL_ID)
        && !d
            .state
            .allowed_skills_for_principal(&d.tool_ctx.principal)
            .is_empty()
}

/// An agent run's offer, the one list both the round and the automatic
/// router read: its grants (the spec's tools among them, when it has a
/// spec), the skill loader when its skills are listed, the run's synthetic
/// tools and, under a contract, `finish`.
fn agent_offer(d: &OpenAiDriver, run: &AgentRun, granted: &[String]) -> Vec<String> {
    let mut offer = match run.surface() {
        Some(surface) => surface.offered(granted),
        None => granted.to_vec(),
    };
    if skill_loader_on(d, granted) && !offer.iter().any(|id| id == READ_SKILL_ID) {
        offer.push(READ_SKILL_ID.to_string());
    }
    offer.extend(run.terminal_tool().iter().map(|t| t.id().to_string()));
    offer
}

/// A person's offer: the per-conversation overlay of their grant, plus the
/// connected MCP tools whose connector the conversation turned on (via
/// `enable_tools` or the composer's "+" menu). The MCP ids come from the SAME
/// layer the executor uses, so an advertised tool is always dispatchable.
async fn chat_offer(d: &OpenAiDriver, session_id: &str, user_mcp: &UserMcpLayer) -> Vec<String> {
    count_chat_only_read();
    let mut allowed = d
        .state
        .allowed_tools_for_session(&d.tool_ctx.principal, session_id)
        .await;
    let enabled_keys = chat_session_tools::enabled_keys_for_session(&d.state.db, session_id)
        .await
        .unwrap_or_default();
    d.state.union_enabled_mcp_tool_ids(
        &mut allowed,
        user_mcp,
        &enabled_keys,
        &d.state.mcp_grant_for_principal(&d.tool_ctx.principal),
    );
    allowed
}

#[cfg(test)]
thread_local! {
    /// How many chat-only reads this thread's turns made. A test seam, not a
    /// fake: on an agent run their results would be thrown away, so no state
    /// could show whether they ran — only a count can.
    pub(crate) static CHAT_ONLY_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn count_chat_only_read() {
    #[cfg(test)]
    CHAT_ONLY_READS.with(|reads| reads.set(reads.get() + 1));
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiplane_core::server::config::{Config, PromptConfig};

    fn config_with(prompt: PromptConfig) -> Config {
        let mut config = Config::default();
        config.chat.prompt = prompt;
        config
    }

    #[test]
    fn the_admin_prompt_is_a_chat_section_while_enabled_with_the_builtin_as_fallback() {
        assert_eq!(
            chat_own_sections(&Config::default()),
            vec![CHAT_PROMPT.to_owned()]
        );
        assert_eq!(
            chat_own_sections(&config_with(PromptConfig {
                enabled: true,
                text: "   ".into()
            })),
            vec![CHAT_PROMPT.to_owned()]
        );
        assert!(
            chat_own_sections(&config_with(PromptConfig {
                enabled: false,
                text: "Standby.".into()
            }))
            .is_empty()
        );
        assert_eq!(
            chat_own_sections(&config_with(PromptConfig {
                enabled: true,
                text: "Answer in plain German.".into()
            })),
            vec!["Answer in plain German.".to_owned()]
        );
    }

    #[test]
    fn the_voice_directive_falls_back_to_the_builtin_one_while_blank() {
        let mut config = Config::default();
        assert_eq!(voice_directive(&config), VOICE_DIRECTIVE);
        config.chat.voice.directive = "   ".into();
        assert_eq!(voice_directive(&config), VOICE_DIRECTIVE);
        config.chat.voice.directive = "Keep answers to one sentence.".into();
        assert_eq!(voice_directive(&config), "Keep answers to one sentence.");
    }
}
