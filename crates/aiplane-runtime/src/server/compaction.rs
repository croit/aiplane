// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Automatic conversation compaction.
//!
//! A chat session replays its whole turn history to the model on every turn
//! (see [`crate::openai_driver::run_one_turn`]), so the upstream prompt grows
//! without bound and eventually crowds the model's context window. Compaction
//! folds the oldest prefix of a conversation into one LLM-generated summary and
//! replays that summary in place of the folded turns, keeping the most recent
//! turns verbatim (the "hybrid" scheme).
//!
//! The trigger is automatic and after-the-fact: once an assistant turn
//! finalises, the driver spawns [`maybe_autocompact`], which compares the
//! turn's measured context size ([`session_core::db::latest_context_tokens`])
//! against a fraction of the model's context window. If it's over, it
//! summarises in the background — off the turn's critical path, exactly like
//! the title-generation task — so the *next* turn replays a smaller prompt.
//!
//! Re-compaction folds the previous summary plus the newly-aged turns into a
//! fresh summary and bumps the cutoff, so a long-running conversation stays
//! bounded across many compactions.
//!
//! The summariser is a best-effort side call ([`side_call::ask_text`]): it
//! passes the turn's payer's spend limits first and is a usage row of theirs.
//! The folded turns are never deleted — they stay in `chat_turns` and remain visible in the
//! transcript; they are simply not sent upstream.

use crate::agents::audit::RunLog;
use crate::server::side_call::{self, Payer, SideCall, strip_think_block};
use session_core::db::{self as chat, TurnRole, TurnStatus, TurnWithTools};

use crate::rama_server::state::RamaState;
use aiplane_core::server::config::CompactionConfig;
use aiplane_core::server::db::{chat_compactions, model_defaults};
use aiplane_core::server::upstreams::{PoolAccess, PoolKind};

/// Hard timeout on the summariser call — a sticky upstream can't keep the
/// background task alive indefinitely.
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// Per-tool-call truncation caps in the summariser input. Tool arguments and
/// outputs can be large (a fetched page, a document); we hand the summariser a
/// bounded slice — enough to know what happened, not the whole payload it's
/// meant to compress away.
const MAX_TOOL_ARGS_CHARS: usize = 200;
const MAX_TOOL_OUTPUT_CHARS: usize = 600;

const SUMMARY_SYSTEM_PROMPT: &str = "You compress the earlier part of a chat conversation into a \
dense summary that will REPLACE those messages so the assistant can keep going without them.\n\
Preserve: concrete facts, decisions made, the user's goals and constraints, file/function/entity \
names, numbers, code or commands that matter, tool results the conversation relied on, and any \
open questions or unfinished tasks.\n\
Drop: pleasantries, redundancy, and step-by-step narration that no longer matters.\n\
Write in compact prose or bullet points. No preamble, no meta-commentary, no \"here is the \
summary\" — output only the summary itself.\n\
/no_think";

/// Check the session's current context size against the compaction threshold
/// and, if over, summarise the oldest turns in the background. Best-effort:
/// every failure path logs and returns without touching the conversation.
///
/// Called (spawned) by the driver after an assistant turn finalises. `model`
/// is the **resolved real model** (the driver maps any alias first) — used both
/// to resolve the context window and to route the summariser call, over the
/// pools `access` reaches: the summary carries the conversation, so an agent's
/// may only go to the pools its own turns may. `log` is the agent run the
/// conversation belongs to, whose activity log records the summariser's
/// exchange; `None` for a person's chat. `payer` is whoever the turns are
/// for: the summary is a usage row of theirs and needs their budget.
pub async fn maybe_autocompact(
    state: &RamaState,
    session_id: &str,
    model: &str,
    access: &PoolAccess,
    payer: &Payer,
    log: Option<RunLog>,
) {
    let cfg = &state.config().chat.compaction;
    if !cfg.enabled {
        return;
    }
    if cfg.trigger_ratio <= 0.0 {
        return;
    }

    let window = model_context_window(state, model)
        .await
        .unwrap_or(cfg.default_context_window);
    if window <= 0 {
        return;
    }
    let threshold = (window as f64 * cfg.trigger_ratio) as i64;

    let current = match chat::latest_context_tokens(&state.db, session_id).await {
        Ok(Some(n)) => n,
        Ok(None) => return, // no measurement yet — nothing to decide on
        Err(err) => {
            tracing::warn!(error = %err, %session_id, "compaction: reading context size failed");
            return;
        }
    };
    if current < threshold {
        return;
    }

    tracing::info!(
        %session_id, %model, current, threshold, window,
        "compaction: context over threshold, summarising"
    );
    match run_compaction(state, session_id, model, access, payer, Some(current), log).await {
        Ok(true) => {}
        Ok(false) => {
            tracing::debug!(%session_id, "compaction: nothing to fold (guarded)");
        }
        Err(err) => {
            tracing::warn!(error = %err, %session_id, "compaction: failed");
        }
    }
}

/// The real model a conversation on `requested` compacts with, and the
/// access to route it under. An alias resolves to its target — the context
/// window keys on the model that ran, an alias carries no settings of its
/// own. An automatic route compacts on the target it pinned for the session,
/// else its fallback, under `access` widened by the route's members exactly
/// as its turns are.
pub async fn compaction_target(
    state: &RamaState,
    requested: &str,
    subject: &str,
    session_id: &str,
    access: PoolAccess,
) -> (String, PoolAccess) {
    let (routing_model, access) = match state.automatic_router.route(requested).await {
        Ok(Some(route)) => {
            let target = state
                .automatic_router
                .session_target(requested, subject, session_id)
                .unwrap_or_else(|| route.fallback_target.clone());
            (
                target,
                access.for_route_targets(&route.alias, route.members()),
            )
        }
        _ => (requested.to_string(), access.resolving()),
    };
    let model = state
        .upstreams
        .resolve_model_for(&routing_model, PoolKind::Chat, &access)
        .unwrap_or(routing_model);
    (model, access)
}

/// Resolve the model's context window from `model_defaults`. `None` when the
/// model has no row or no `context_window` set — the caller falls back to the
/// global default. Keyed on the resolved real model id (the caller maps any
/// alias first), matching how reasoning config and cost accounting key on it —
/// an alias carries no settings of its own.
pub(crate) async fn model_context_window(state: &RamaState, model: &str) -> Option<i64> {
    model_defaults::get(&state.db, model)
        .await
        .ok()
        .flatten()
        .and_then(|row| row.context_window)
        .filter(|w| *w > 0)
        // Nothing configured: ask the backend. The health probe already reads
        // `/models`, where vLLM reports each model's `max_model_len`, so the
        // real window is known without an operator having to type it in.
        //
        // It matters more than it looks. Falling through to the global
        // `default_context_window` (32768) for a model actually serving 262144
        // shrank the turn's tool-output allowance to an eighth of its size —
        // which is what starved a multi-step retrieval turn into 2 KB tool
        // results and made it report that it could not read a file.
        .or_else(|| state.upstreams.probed_context_window(model))
}

/// Load the session, plan the fold, call the summariser, and persist the
/// compaction row. Returns `Ok(true)` if a summary was written, `Ok(false)` if
/// the plan decided there was nothing (new) to fold.
async fn run_compaction(
    state: &RamaState,
    session_id: &str,
    model: &str,
    access: &PoolAccess,
    payer: &Payer,
    tokens_before: Option<i64>,
    log: Option<RunLog>,
) -> Result<bool, String> {
    let turns = chat::list_turns(&state.db, session_id)
        .await
        .map_err(|e| e.to_string())?;
    let existing = chat_compactions::get(&state.db, session_id)
        .await
        .map_err(|e| e.to_string())?;
    let cfg = &state.config().chat.compaction;

    let Some(plan) = plan_compaction(&turns, existing.as_ref().map(|c| c.up_to_seq), cfg) else {
        return Ok(false);
    };

    let answered = side_call::ask_text(
        state,
        payer,
        SideCall {
            purpose: "compaction_summary",
            model,
            access,
            instructions: SUMMARY_SYSTEM_PROMPT,
            input: &plan.input_text,
            temperature: 0.0,
            max_tokens: Some(cfg.summary_max_tokens),
            no_think: true,
            timeout: TIMEOUT,
        },
    )
    .await;
    if let Some(log) = &log {
        log.record(&state.db, &answered.exchange).await;
    }
    let raw = answered.answer.map_err(|e| e.to_string())?;

    let summary = clean_summary(&raw);
    if summary.is_empty() {
        return Err("summariser returned empty content".to_string());
    }
    // Rough token estimate for bookkeeping only (~4 chars/token); never
    // load-bearing.
    let tokens_after = Some((summary.chars().count() / 4) as i64);
    chat_compactions::upsert(
        &state.db,
        session_id,
        plan.new_up_to_seq,
        &summary,
        tokens_before,
        tokens_after,
    )
    .await
    .map_err(|e| e.to_string())?;
    tracing::info!(
        %session_id,
        up_to_seq = plan.new_up_to_seq,
        summary_len = summary.len(),
        "compaction: summary persisted"
    );
    Ok(true)
}

/// The decision + the text to summarise. Pure output of [`plan_compaction`].
#[derive(Debug, PartialEq, Eq)]
struct CompactionPlan {
    /// New cutoff: the highest turn `seq` the fresh summary will cover.
    new_up_to_seq: i64,
    /// The summariser input — the previous summary (if any) followed by the
    /// newly-aged turns rendered as plain text.
    input_text: String,
}

/// Decide whether (and what) to compact. Pure so it can be unit-tested without
/// a model call.
///
/// - Keeps the last `keep_recent_turns` eligible turns verbatim.
/// - Only folds turns that have aged past `old_up_to_seq` (the previous
///   summary already covers the rest).
/// - Returns `None` when there aren't enough newly-aged turns to be worth a
///   re-summarise (`min_turns_to_compact`), or when nothing new has aged.
fn plan_compaction(
    turns: &[TurnWithTools],
    old_up_to_seq: Option<i64>,
    cfg: &CompactionConfig,
) -> Option<CompactionPlan> {
    // Eligible = the turns that actually go upstream on replay: every user
    // turn, plus completed assistant turns with visible content. In-progress /
    // errored / empty assistant turns are skipped (they never replay), so the
    // cutoff lines up with what the summary is standing in for.
    let eligible: Vec<&TurnWithTools> = turns
        .iter()
        .filter(|t| match t.turn.role {
            TurnRole::User => true,
            TurnRole::Assistant => {
                t.turn.status == TurnStatus::Completed
                    && t.turn.content.as_deref().is_some_and(|c| !c.is_empty())
            }
        })
        .collect();

    if eligible.len() <= cfg.keep_recent_turns {
        return None;
    }
    let cutoff_index = eligible.len() - cfg.keep_recent_turns;
    let folded = &eligible[..cutoff_index];
    let new_up_to_seq = folded.last()?.turn.seq;

    let old_up_to = old_up_to_seq.unwrap_or(-1);
    if new_up_to_seq <= old_up_to {
        return None; // nothing new has aged past the previous cutoff
    }
    let newly_folded: Vec<&&TurnWithTools> =
        folded.iter().filter(|t| t.turn.seq > old_up_to).collect();
    if newly_folded.len() < cfg.min_turns_to_compact {
        return None; // anti-thrash: not enough new material to re-summarise
    }

    let mut input_text = String::new();
    if old_up_to_seq.is_some() {
        input_text.push_str(
            "Additional conversation messages to fold into the summary above follow. \
             Merge them into a single updated summary.\n\n",
        );
    }
    for t in newly_folded {
        append_turn(&mut input_text, t);
    }

    Some(CompactionPlan {
        new_up_to_seq,
        input_text,
    })
}

/// Render one turn into the summariser input: the user's prompt, or the
/// assistant's content plus a one-line trace of each tool call (name + bounded
/// args + bounded output). Tool traces matter because they're never replayed
/// as normal history, yet are often the load-bearing context.
fn append_turn(out: &mut String, t: &TurnWithTools) {
    match t.turn.role {
        TurnRole::User => {
            let raw = t.turn.user_content.clone().unwrap_or_default();
            let content = aiplane_features::server::chat_attachments::strip_markers_for_replay(
                &raw, &t.turn.id,
            );
            if !content.trim().is_empty() {
                out.push_str("User: ");
                out.push_str(content.trim());
                out.push_str("\n\n");
            }
        }
        TurnRole::Assistant => {
            if let Some(content) = t.turn.content.as_deref().filter(|c| !c.is_empty()) {
                // Same stripping as the live history replay: the summary
                // rides in the system prompt, so a raw marker in here
                // would re-teach the model the syntax it must not write.
                let content = aiplane_features::server::chat_attachments::strip_markers_for_replay(
                    content, &t.turn.id,
                );
                out.push_str("Assistant: ");
                out.push_str(content.trim());
                out.push('\n');
            }
            for tc in &t.tool_calls {
                out.push_str("  [tool ");
                out.push_str(&tc.name);
                out.push('(');
                out.push_str(&session_core::text::truncate_chars(
                    tc.arguments_json.trim(),
                    MAX_TOOL_ARGS_CHARS,
                ));
                out.push(')');
                if let Some(output) = tc.output_json.as_deref() {
                    out.push_str(" -> ");
                    out.push_str(&session_core::text::truncate_chars(
                        output.trim(),
                        MAX_TOOL_OUTPUT_CHARS,
                    ));
                }
                out.push_str("]\n");
            }
            out.push('\n');
        }
    }
}

/// Trim the summariser output into a clean body: strip a leaked
/// `<think>…</think>` block (some reasoning-parser adapters leak it despite the
/// knobs) and surrounding whitespace.
fn clean_summary(raw: &str) -> String {
    strip_think_block(raw).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::Timestamp;
    use session_core::db::{Turn, TurnRole, TurnStatus};

    fn cfg() -> CompactionConfig {
        CompactionConfig {
            enabled: true,
            default_context_window: 1000,
            trigger_ratio: 0.7,
            keep_recent_turns: 2,
            min_turns_to_compact: 2,
            summary_max_tokens: 512,
        }
    }

    fn turn(seq: i64, role: TurnRole, text: &str) -> TurnWithTools {
        let now: Timestamp = "2026-01-01T00:00:00Z".parse().unwrap();
        let (user_content, content, status) = match role {
            TurnRole::User => (Some(text.to_string()), None, TurnStatus::Completed),
            TurnRole::Assistant => (None, Some(text.to_string()), TurnStatus::Completed),
        };
        TurnWithTools {
            turn: Turn {
                id: format!("t{seq}"),
                session_id: "s1".into(),
                seq,
                role,
                user_content,
                model: None,
                content,
                reasoning: None,
                reasoning_elapsed_ms: None,
                reasoning_started_at: None,
                status,
                error_message: None,
                error_code: None,
                created_at: now,
                completed_at: Some(now),
            },
            tool_calls: vec![],
            steers: vec![],
            attempts: Vec::new(),
            suspension: None,
        }
    }

    /// A short conversation (<= keep_recent_turns worth) is never compacted.
    #[test]
    fn no_plan_when_short() {
        let turns = vec![
            turn(0, TurnRole::User, "hi"),
            turn(1, TurnRole::Assistant, "hello"),
        ];
        assert!(plan_compaction(&turns, None, &cfg()).is_none());
    }

    /// First compaction folds everything but the last `keep_recent_turns`.
    #[test]
    fn first_compaction_folds_prefix_keeps_tail() {
        let turns = vec![
            turn(0, TurnRole::User, "q1"),
            turn(1, TurnRole::Assistant, "a1"),
            turn(2, TurnRole::User, "q2"),
            turn(3, TurnRole::Assistant, "a2"),
            turn(4, TurnRole::User, "q3"),
            turn(5, TurnRole::Assistant, "a3"),
        ];
        // keep_recent_turns = 2 → fold seq 0..3, keep seq 4,5.
        let plan = plan_compaction(&turns, None, &cfg()).expect("should plan");
        assert_eq!(plan.new_up_to_seq, 3);
        assert!(plan.input_text.contains("q1"));
        assert!(plan.input_text.contains("a2"));
        assert!(!plan.input_text.contains("q3"), "tail must stay verbatim");
    }

    /// The summary rides in the system prompt. If an assistant turn's
    /// `[gw-attachment …]` marker went into it raw, the model would read
    /// the marker syntax as a way to "attach" a file and start writing
    /// marker lines itself instead of calling the render tool.
    #[test]
    fn folded_assistant_markers_never_reach_the_summariser_raw() {
        let marker = session_core::attachments::marker_line(
            "deck.pdf",
            "application/pdf",
            "/chat/attachment/t1/deck.pdf",
            42,
        );
        let turns = vec![
            turn(0, TurnRole::User, "build me a deck"),
            turn(1, TurnRole::Assistant, &format!("here it is\n\n{marker}")),
            turn(2, TurnRole::User, "q2"),
            turn(3, TurnRole::Assistant, "a2"),
            turn(4, TurnRole::User, "q3"),
            turn(5, TurnRole::Assistant, "a3"),
        ];
        let plan = plan_compaction(&turns, None, &cfg()).expect("should plan");
        assert!(
            !plan.input_text.contains("gw-attachment"),
            "raw marker leaked into the summariser input: {}",
            plan.input_text
        );
        // The file is still named — the summary must not lose that a deck
        // exists, only how to forge a chip for it.
        assert!(plan.input_text.contains("deck.pdf"), "{}", plan.input_text);
        assert!(
            plan.input_text.contains("t1/deck.pdf"),
            "{}",
            plan.input_text
        );
    }

    /// Anti-thrash: nothing new has aged past the previous cutoff → no plan.
    #[test]
    fn no_replan_when_nothing_new_aged() {
        let turns = vec![
            turn(0, TurnRole::User, "q1"),
            turn(1, TurnRole::Assistant, "a1"),
            turn(2, TurnRole::User, "q2"),
            turn(3, TurnRole::Assistant, "a2"),
            turn(4, TurnRole::User, "q3"),
            turn(5, TurnRole::Assistant, "a3"),
        ];
        // Already compacted up to seq 3; cutoff would still be 3 → None.
        assert!(plan_compaction(&turns, Some(3), &cfg()).is_none());
    }

    /// Re-compaction only folds the turns beyond the previous cutoff.
    #[test]
    fn recompaction_folds_only_new_turns() {
        let turns = vec![
            turn(0, TurnRole::User, "q1"),
            turn(1, TurnRole::Assistant, "a1"),
            turn(2, TurnRole::User, "q2"),
            turn(3, TurnRole::Assistant, "a2"),
            turn(4, TurnRole::User, "q3"),
            turn(5, TurnRole::Assistant, "a3"),
            turn(6, TurnRole::User, "q4"),
            turn(7, TurnRole::Assistant, "a4"),
        ];
        // Previously compacted to seq 1; keep_recent 2 → new cutoff seq 5.
        let plan = plan_compaction(&turns, Some(1), &cfg()).expect("should replan");
        assert_eq!(plan.new_up_to_seq, 5);
        assert!(
            !plan.input_text.contains("q1"),
            "already-summarised turn excluded"
        );
        assert!(plan.input_text.contains("q2"));
        assert!(plan.input_text.contains("a3"));
        assert!(!plan.input_text.contains("q4"), "tail stays verbatim");
    }

    #[test]
    fn truncate_is_char_safe() {
        assert_eq!(session_core::text::truncate_chars("hello", 10), "hello");
        assert_eq!(session_core::text::truncate_chars("hello", 3), "hel…");
        // Multi-byte chars aren't split.
        assert_eq!(
            session_core::text::truncate_chars("héllo wörld", 4),
            "héll…"
        );
    }

    #[test]
    fn clean_summary_strips_think() {
        assert_eq!(
            clean_summary("<think>pondering</think>\n\nThe summary."),
            "The summary."
        );
    }
}
