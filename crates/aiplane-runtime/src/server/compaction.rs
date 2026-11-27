// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Automatic conversation compaction.
//!
//! A chat session replays its whole turn history to the model on every turn
//! (see [`crate::openai_driver::run_one_turn`]), so the upstream prompt grows
//! without bound and eventually crowds the model's context window. Compaction
//! replaces the history up to a cutoff with one LLM-written checkpoint and
//! keeps the newest conversation — about [`CompactionConfig::keep_tokens`] of
//! it — verbatim beside that checkpoint.
//!
//! The trigger is automatic and after-the-fact: once an assistant turn
//! finalises, the driver spawns [`maybe_autocompact`], which compares the
//! turn's measured context size ([`session_core::db::latest_context_tokens`])
//! against the model's ceiling ([`compaction_ceiling`]: the window minus the
//! room an answer and its checkpoint need). If it's over, it summarises in the
//! background — off the turn's critical path, exactly like the
//! title-generation task — so the *next* turn replays a smaller prompt.
//!
//! The checkpoint is structured, not a paragraph: the headings in
//! [`SUMMARY_SYSTEM_PROMPT`] are checked against the answer, and one that
//! skipped the template is re-asked once before the fold is given up. The
//! section that carries the person's own words is the one that keeps a
//! conversation from drifting off early instructions.
//!
//! Re-compaction folds the previous checkpoint plus the newly-aged turns (and
//! every message the person wrote before them, verbatim) into a fresh
//! checkpoint and bumps the cutoff, so a long-running conversation stays
//! bounded across many compactions.
//!
//! The summariser is a best-effort side call ([`side_call::ask_text`]): it
//! passes the turn's payer's spend limits first and is a usage row of theirs.
//! The folded turns are never deleted — they stay in `chat_turns` and remain visible in the
//! transcript; they are simply not sent upstream.

use crate::agents::audit::RunLog;
use crate::server::side_call::{self, Payer, SideCall, strip_think_block};
use session_core::db::{self as chat, SteerStatus, TurnRole, TurnStatus, TurnWithTools};

use crate::rama_server::state::RamaState;
use aiplane_core::server::config::CompactionConfig;
use aiplane_core::server::db::{chat_compactions, model_defaults};
use aiplane_core::server::upstreams::{PoolAccess, PoolKind};

/// Hard timeout on the summariser call — a sticky upstream can't keep the
/// background task alive indefinitely.
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// The least room kept free below the model's window before compaction fires.
///
/// An answer and its checkpoint both have to fit beside the history, so a
/// conversation may not be filled to the window's last token. Below this, the
/// floor wins over the configured share: a 32k model compacts at 16k rather
/// than at 27k. Matches the floor the mature harnesses use.
const RESERVE_MIN: i64 = 16_000;

/// A re-compaction has to be worth its own model call: the turns that newly
/// aged must carry at least this share of the keep budget. Otherwise the
/// checkpoint is rewritten for a few hundred tokens of gain, which costs more
/// than it saves and rewrites the same facts over and over.
const RE_FOLD_MIN_SHARE: f64 = 0.1;

/// The prompt size at which an answer's older history is folded into a
/// checkpoint: the model's window minus the room kept free for the answer
/// (the configured share, at least [`RESERVE_MIN`]). `None` when that leaves
/// no usable room — a window too small to compact anything into.
pub fn compaction_ceiling(window: i64, buffer_ratio: f64) -> Option<i64> {
    if window <= 0 {
        return None;
    }
    let share = (window as f64 * buffer_ratio.clamp(0.0, 1.0)) as i64;
    let ceiling = window - share.max(RESERVE_MIN);
    (ceiling > 0).then_some(ceiling)
}

/// How much of the conversation stays verbatim beside a checkpoint. The
/// configured budget, but never more than half the ceiling: a keep window
/// that is itself as large as the ceiling leaves the checkpoint no room and
/// makes every following turn fold again.
pub fn effective_keep_tokens(configured: i64, ceiling: i64) -> i64 {
    configured.min(ceiling / 2).max(1_000)
}

pub(crate) async fn wait_for_completed_turn(
    state: &RamaState,
    session_id: &str,
    turn_id: &str,
    mut updates: tokio::sync::broadcast::Receiver<session_core::workers::TurnUpdate>,
) -> bool {
    let completed = tokio::time::timeout(TIMEOUT, async {
        loop {
            match updates.recv().await {
                Ok(session_core::workers::TurnUpdate::Finalized) => break,
                Ok(session_core::workers::TurnUpdate::Released) | Err(_) => return false,
                Ok(_) => {}
            }
        }
        matches!(chat::get_turn(&state.db, session_id, turn_id).await,
            Ok(Some(turn)) if turn.status == TurnStatus::Completed)
    })
    .await;
    completed.unwrap_or(false)
}

/// Per-tool-call truncation caps in the summariser input. Tool arguments and
/// outputs can be large (a fetched page, a document); we hand the summariser a
/// bounded slice — enough to know what happened, not the whole payload it's
/// meant to compress away.
const MAX_TOOL_ARGS_CHARS: usize = 200;
const MAX_TOOL_OUTPUT_CHARS: usize = 600;

/// The summariser's instructions: the operator's own prompt while it has
/// text, the built-in one otherwise.
fn summary_instructions(cfg: &aiplane_core::server::config::CompactionConfig) -> &str {
    use aiplane_core::server::prompts::SUMMARY_SYSTEM_PROMPT;

    if cfg.summary_prompt.is_empty() {
        SUMMARY_SYSTEM_PROMPT
    } else {
        &cfg.summary_prompt
    }
}

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

    let window = model_context_window(state, model)
        .await
        .unwrap_or(cfg.default_context_window);
    let Some(ceiling) = compaction_ceiling(window, cfg.buffer_ratio) else {
        return;
    };

    let current = match chat::latest_context_tokens(&state.db, session_id).await {
        Ok(Some(n)) => n,
        Ok(None) => return, // no measurement yet — nothing to decide on
        Err(err) => {
            tracing::warn!(error = %err, %session_id, "compaction: reading context size failed");
            return;
        }
    };
    if current < ceiling {
        return;
    }

    tracing::info!(
        %session_id, %model, current, ceiling, window,
        "compaction: context over the ceiling, summarising"
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
    let row = model_defaults::get(&state.db, model).await.ok().flatten();
    known_context_window(row.as_ref(), &state.upstreams, model)
}

/// The context window the gateway knows for `model`: the admin's value, else
/// the one the serving backend reports. `None` when neither exists.
pub fn known_context_window(
    defaults: Option<&model_defaults::ModelDefaults>,
    upstreams: &aiplane_core::server::upstreams::UpstreamRegistry,
    model: &str,
) -> Option<i64> {
    defaults
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
        .or_else(|| upstreams.probed_context_window(model))
}

/// One summariser call — the operator's prompt while it has text, the built-in
/// one otherwise — recorded in the caller's activity log when there is one.
async fn ask_summary(
    state: &RamaState,
    payer: &Payer,
    model: &str,
    access: &PoolAccess,
    cfg: &CompactionConfig,
    input: &str,
    log: Option<RunLog>,
) -> Result<String, String> {
    let answered = side_call::ask_text(
        state,
        payer,
        SideCall {
            purpose: "compaction_summary",
            model,
            access,
            instructions: summary_instructions(cfg),
            input,
            max_tokens: Some(cfg.summary_max_tokens),
            no_think: true,
            timeout: TIMEOUT,
        },
    )
    .await;
    if let Some(log) = &log {
        log.record(&state.db, &answered.exchange).await;
    }
    answered.answer.map_err(|e| e.to_string())
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
    // The summary carries the whole conversation: never to a model the
    // conversation's level does not clear (a retrieval can raise it after
    // the turn's model was chosen).
    if !state
        .model_cleared_for_session(session_id, model, access)
        .await
    {
        return Err(format!(
            "model `{model}` is not cleared for this conversation's classification level, so the conversation was not summarised"
        ));
    }
    let turns = chat::list_turns(&state.db, session_id)
        .await
        .map_err(|e| e.to_string())?;
    let existing = chat_compactions::get(&state.db, session_id)
        .await
        .map_err(|e| e.to_string())?;
    let cfg = &state.config().chat.compaction;

    // The keep window is bounded by the ceiling, not just by the setting: on a
    // small model the configured 15k would leave the checkpoint no room and
    // make every following turn fold again.
    let window = model_context_window(state, model)
        .await
        .unwrap_or(cfg.default_context_window);
    let ceiling = compaction_ceiling(window, cfg.buffer_ratio)
        .ok_or_else(|| format!("model `{model}` has no room to compact into ({window} tokens)"))?;
    let keep_tokens = effective_keep_tokens(cfg.keep_tokens, ceiling);

    let Some(plan) = plan_compaction(&turns, existing.as_ref(), keep_tokens) else {
        return Ok(false);
    };

    // An operator's own prompt may ask for a different shape, so only the
    // built-in template is enforced.
    let template_enforced = cfg.summary_prompt.is_empty();
    let mut answer = ask_summary(
        state,
        payer,
        model,
        access,
        cfg,
        &plan.input_text,
        log.clone(),
    )
    .await?;
    if template_enforced && !follows_summary_template(&answer) {
        let nudged = format!(
            "{}\n\n---\n\n{}",
            plan.input_text,
            aiplane_core::server::prompts::SUMMARY_NUDGE
        );
        tracing::info!(%session_id, "compaction: summary skipped the template, asking once more");
        answer = ask_summary(state, payer, model, access, cfg, &nudged, log).await?;
    }

    let summary = clean_summary(&answer);
    if summary.is_empty() {
        return Err("summariser returned empty content".to_string());
    }
    if template_enforced && !follows_summary_template(&summary) {
        return Err(
            "summariser answered twice without the required checkpoint headings".to_string(),
        );
    }
    // Rough token estimate for bookkeeping only (~4 chars/token); never
    // load-bearing.
    let tokens_after = Some((summary.chars().count() / 4) as i64);
    if payer.is_organization_compaction() {
        let mut tx = aiplane_core::server::db::WriteTx::begin(&state.db)
            .await
            .map_err(|error| error.to_string())?;
        payer
            .compaction_access_in(state, &mut tx, access, model)
            .await?;
        chat_compactions::upsert(
            &mut *tx,
            session_id,
            plan.new_up_to_seq,
            &summary,
            tokens_before,
            tokens_after,
        )
        .await
        .map_err(|error| error.to_string())?;
        tx.commit().await.map_err(|error| error.to_string())?;
    } else {
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
    }
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
    /// New cutoff: the highest turn `seq` the fresh checkpoint will cover.
    new_up_to_seq: i64,
    /// The summariser input — the previous checkpoint (if any), every message
    /// the person wrote before it, and the newly-aged turns as plain text.
    input_text: String,
}

/// Decide whether (and what) to compact. Pure so it can be unit-tested without
/// a model call.
///
/// - The newest turns whose replayed text fits `keep_tokens` stay verbatim
///   (always at least the newest one), and everything older is folded.
/// - Only folds turns that have aged past the previous checkpoint: it already
///   covers the rest.
/// - Returns `None` when nothing new has aged, or when the turns that did age
///   are too small for a re-fold to be worth its own model call.
fn plan_compaction(
    turns: &[TurnWithTools],
    existing: Option<&chat_compactions::Compaction>,
    keep_tokens: i64,
) -> Option<CompactionPlan> {
    // Eligible = the turns that actually go upstream on replay: every user
    // turn, plus completed assistant turns with visible content. In-progress /
    // errored / empty assistant turns are skipped (they never replay), so the
    // cutoff lines up with what the checkpoint is standing in for.
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
    if eligible.len() < 2 {
        return None; // nothing older to fold, and the newest turn always stays
    }

    // Walk back from the newest turn until the keep budget is spent. The newest
    // turn is kept unconditionally: the exchange the reader is looking at is
    // the one that must never be lossily restated.
    let mut kept: i64 = 0;
    let mut keep_from = eligible.len();
    for index in (0..eligible.len()).rev() {
        let cost = replay_tokens(eligible[index]);
        if kept > 0 && kept + cost > keep_tokens {
            break;
        }
        kept += cost;
        keep_from = index;
    }
    let folded = &eligible[..keep_from];
    let new_up_to_seq = folded.last()?.turn.seq;

    let old_up_to = existing.map_or(-1, |checkpoint| checkpoint.up_to_seq);
    if new_up_to_seq <= old_up_to {
        return None; // nothing new has aged past the previous checkpoint
    }
    let newly_folded: Vec<&&TurnWithTools> =
        folded.iter().filter(|t| t.turn.seq > old_up_to).collect();

    let mut input_text = String::new();
    if let Some(checkpoint) = existing {
        // A re-fold for a few hundred tokens costs more than it saves, and
        // rewrites the same facts over and over.
        let aged: i64 = newly_folded.iter().map(|t| replay_tokens(t)).sum();
        if (aged as f64) < (keep_tokens as f64 * RE_FOLD_MIN_SHARE) {
            return None;
        }
        input_text.push_str(
            "The checkpoint written by an earlier compaction of this conversation follows. \
             Fold the newer messages below into it; the newer messages take precedence \
             wherever they disagree.\n\n",
        );
        input_text.push_str(checkpoint.summary.trim());
        input_text.push_str("\n\n");

        // The person's own words are what a checkpoint must not lose, and a
        // previous checkpoint may already have paraphrased them. They are sent
        // again verbatim, so every fold starts from the original wording
        // rather than from the last summary of it.
        let earlier: Vec<&&TurnWithTools> = eligible
            .iter()
            .filter(|t| t.turn.seq <= old_up_to && t.turn.role == TurnRole::User)
            .collect();
        if !earlier.is_empty() {
            input_text.push_str(
                "Every message the person wrote before the messages that follow, oldest first, \
                 verbatim. Reproduce each one word for word in the \"All user messages\" \
                 section:\n\n",
            );
            for t in earlier {
                append_user_message(&mut input_text, t);
            }
            input_text.push('\n');
        }
        input_text.push_str("Conversation messages to fold in now follow.\n\n");
    }
    for t in newly_folded {
        append_turn(&mut input_text, t);
    }

    Some(CompactionPlan {
        new_up_to_seq,
        input_text,
    })
}

/// Rough token estimate (~4 characters per token) of what one turn replays to
/// the model. The keep window is a size budget, so it is measured the same way
/// the replay is: tool traces never go upstream and do not count.
fn replay_tokens(t: &TurnWithTools) -> i64 {
    let text = match t.turn.role {
        TurnRole::User => t.turn.user_content.as_deref().unwrap_or_default(),
        TurnRole::Assistant => t.turn.content.as_deref().unwrap_or_default(),
    };
    let steers: usize = t
        .steers
        .iter()
        .filter(|steer| steer.status == SteerStatus::Delivered)
        .map(|steer| steer.text.len())
        .sum();
    ((text.len() + steers) / 4) as i64
}

/// The person's message as the summariser sees it: attachment markers stripped
/// the same way the live replay strips them, and labelled so the section it
/// belongs in is unambiguous.
fn append_user_message(out: &mut String, t: &TurnWithTools) {
    let raw = t.turn.user_content.clone().unwrap_or_default();
    let content =
        aiplane_features::server::chat_attachments::strip_markers_for_replay(&raw, &t.turn.id);
    if content.trim().is_empty() {
        return;
    }
    out.push_str("- ");
    out.push_str(content.trim());
    out.push('\n');
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

/// Whether `summary` carries every heading the built-in template requires.
/// One missing section means a whole class of context (the person's own words,
/// the standing rules) is silently gone, so it is worth one more model call.
fn follows_summary_template(summary: &str) -> bool {
    let lowered = summary.to_ascii_lowercase();
    aiplane_core::server::prompts::SUMMARY_HEADINGS
        .iter()
        .all(|heading| lowered.contains(&heading.to_ascii_lowercase()))
}

/// Trim the summariser output into a clean body: strip a leaked
/// `<think>…</think>` block and the `<analysis>` pass the prompt asks for (some
/// reasoning-parser adapters leak the first despite the knobs), then trim.
fn clean_summary(raw: &str) -> String {
    strip_analysis_block(&strip_think_block(raw))
        .trim()
        .to_string()
}

/// Remove `<analysis>…</analysis>` from a summariser answer — the checkpoint
/// itself is what gets stored and replayed, not the working-through of it.
/// Repeated until none is left; an unterminated block is left alone rather than
/// eating the summary behind it.
fn strip_analysis_block(s: &str) -> String {
    let mut out = s.to_string();
    loop {
        let lower = out.to_ascii_lowercase();
        let Some(start) = lower.find("<analysis>") else {
            return out;
        };
        let after_start = start + "<analysis>".len();
        let Some(relative_end) = lower[after_start..].find("</analysis>") else {
            return out;
        };
        let end = after_start + relative_end + "</analysis>".len();
        out.replace_range(start..end, "");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiplane_core::server::prompts::SUMMARY_SYSTEM_PROMPT;
    use jiff::Timestamp;
    use session_core::db::{Turn, TurnRole, TurnStatus};

    #[tokio::test]
    async fn organization_compaction_waits_for_actual_completed_row_and_skips_suspended() {
        use crate::server::headless::tests::{organization_draft_fixture, state_for};
        for status in [
            TurnStatus::Completed,
            TurnStatus::Suspended,
            TurnStatus::Cancelled,
            TurnStatus::Errored,
        ] {
            let upstream = wiremock::MockServer::start().await;
            let state = state_for(&upstream.uri()).await;
            let (_, params) = organization_draft_fixture(&state).await;
            let (events, receiver) = tokio::sync::broadcast::channel(4);
            let observed_state = state.clone();
            let session = params.session_id.clone();
            let turn = params.assistant_turn_id.clone();
            let waiter = tokio::spawn(async move {
                wait_for_completed_turn(&observed_state, &session, &turn, receiver).await
            });
            assert!(!waiter.is_finished());
            chat::finalize_turn(&state.db, &params.assistant_turn_id, status, None)
                .await
                .unwrap();
            events
                .send(session_core::workers::TurnUpdate::Finalized)
                .unwrap();
            assert_eq!(
                tokio::time::timeout(std::time::Duration::from_secs(2), waiter)
                    .await
                    .unwrap()
                    .unwrap(),
                status == TurnStatus::Completed
            );
            assert!(upstream.received_requests().await.unwrap().is_empty());
        }
    }

    fn cfg() -> CompactionConfig {
        CompactionConfig {
            enabled: true,
            default_context_window: 1000,
            buffer_ratio: 0.15,
            keep_tokens: 200,
            summary_max_tokens: 512,
            summary_prompt: String::new(),
        }
    }

    #[test]
    fn the_summariser_uses_the_operators_prompt_only_while_it_has_text() {
        let default = cfg();
        assert_eq!(summary_instructions(&default), SUMMARY_SYSTEM_PROMPT);
        let mut custom = default;
        custom.summary_prompt = "Summarise in one short paragraph.".into();
        assert_eq!(
            summary_instructions(&custom),
            "Summarise in one short paragraph."
        );
    }

    /// The room kept free for the answer decides the ceiling, never the window
    /// itself — and on a small window the 16k floor wins over the share.
    #[test]
    fn the_ceiling_keeps_room_for_the_answer() {
        assert_eq!(compaction_ceiling(32_768, 0.15), Some(16_768));
        assert_eq!(compaction_ceiling(131_072, 0.15), Some(111_412));
        // Below the floor's reach the floor wins: 15% of 20k is 3k, so the
        // ceiling is 4k rather than 17k.
        assert_eq!(compaction_ceiling(20_000, 0.15), Some(4_000));
        // A window the reserve swallows whole has no room to compact into.
        assert_eq!(compaction_ceiling(16_000, 0.15), None);
        assert_eq!(compaction_ceiling(0, 0.15), None);
        // Turning the share up to 1 leaves no room at all.
        assert_eq!(compaction_ceiling(32_768, 1.0), None);
    }

    /// A keep window as large as the ceiling would leave the checkpoint no
    /// room and make every following turn fold again.
    #[test]
    fn the_keep_window_stays_below_half_the_ceiling() {
        assert_eq!(effective_keep_tokens(15_000, 111_411), 15_000);
        assert_eq!(effective_keep_tokens(15_000, 16_768), 8_384);
        assert_eq!(effective_keep_tokens(0, 16_768), 1_000);
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

    /// A turn's body of about `tokens` tokens (~4 characters each), carrying a
    /// recognisable tag so a test can say which turn it found.
    fn line(tag: &str, tokens: usize) -> String {
        format!("{tag} {}", "x".repeat(tokens * 4))
    }

    fn checkpoint(up_to_seq: i64, summary: &str) -> chat_compactions::Compaction {
        chat_compactions::Compaction {
            up_to_seq,
            summary: summary.to_string(),
            tokens_before: Some(9_000),
            tokens_after: Some(200),
        }
    }

    /// Two turns are one exchange: there is no older conversation to fold.
    #[test]
    fn nothing_is_folded_without_older_conversation() {
        let turns = vec![
            turn(0, TurnRole::User, &line("q1", 100)),
            turn(1, TurnRole::Assistant, &line("a1", 100)),
        ];
        assert!(plan_compaction(&turns, None, 200).is_none());
    }

    /// Everything outside the keep budget is folded; the newest turns stay
    /// verbatim.
    #[test]
    fn the_keep_budget_decides_where_folding_starts() {
        let turns = vec![
            turn(0, TurnRole::User, &line("q1", 100)),
            turn(1, TurnRole::Assistant, &line("a1", 100)),
            turn(2, TurnRole::User, &line("q2", 100)),
            turn(3, TurnRole::Assistant, &line("a2", 100)),
            turn(4, TurnRole::User, &line("q3", 100)),
            turn(5, TurnRole::Assistant, &line("a3", 100)),
        ];
        // keep=200 tokens → seq 4 and 5 stay, seq 0..=3 are folded.
        let plan = plan_compaction(&turns, None, 200).expect("should plan");
        assert_eq!(plan.new_up_to_seq, 3);
        assert!(plan.input_text.contains("q1"));
        assert!(plan.input_text.contains("a2"));
        assert!(!plan.input_text.contains("q3"), "tail must stay verbatim");
    }

    /// The exchange the reader is looking at is never summarised, even when it
    /// alone is larger than the whole keep budget.
    #[test]
    fn one_oversized_newest_turn_stays_verbatim() {
        let turns = vec![
            turn(0, TurnRole::User, &line("q1", 50)),
            turn(1, TurnRole::Assistant, &line("a1", 50)),
            turn(2, TurnRole::User, &line("q2", 2_500)),
        ];
        let plan = plan_compaction(&turns, None, 1_000).expect("should plan");
        assert_eq!(plan.new_up_to_seq, 1);
        assert!(!plan.input_text.contains("q2"));
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
            turn(0, TurnRole::User, &line("q1", 100)),
            turn(
                1,
                TurnRole::Assistant,
                &format!("here it is\n\n{marker}\n{}", line("a1", 100)),
            ),
            turn(2, TurnRole::User, &line("q2", 100)),
            turn(3, TurnRole::Assistant, &line("a2", 100)),
            turn(4, TurnRole::User, &line("q3", 100)),
            turn(5, TurnRole::Assistant, &line("a3", 100)),
        ];
        let plan = plan_compaction(&turns, None, 200).expect("should plan");
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

    /// Nothing new has aged past the previous checkpoint → no plan.
    #[test]
    fn no_replan_when_nothing_new_aged() {
        let turns = vec![
            turn(0, TurnRole::User, &line("q1", 100)),
            turn(1, TurnRole::Assistant, &line("a1", 100)),
            turn(2, TurnRole::User, &line("q2", 100)),
            turn(3, TurnRole::Assistant, &line("a2", 100)),
            turn(4, TurnRole::User, &line("q3", 100)),
            turn(5, TurnRole::Assistant, &line("a3", 100)),
        ];
        let previous = checkpoint(3, "Earlier checkpoint");
        assert!(plan_compaction(&turns, Some(&previous), 200).is_none());
    }

    /// A re-fold starts from the previous checkpoint and re-sends the person's
    /// earlier messages verbatim — a checkpoint that paraphrased a rule must
    /// not be the only copy of it.
    #[test]
    fn recompaction_carries_the_checkpoint_and_the_persons_own_words() {
        let turns = vec![
            turn(0, TurnRole::User, &line("q1", 100)),
            turn(1, TurnRole::Assistant, &line("a1", 100)),
            turn(2, TurnRole::User, &line("q2", 100)),
            turn(3, TurnRole::Assistant, &line("a2", 100)),
            turn(4, TurnRole::User, &line("q3", 100)),
            turn(5, TurnRole::Assistant, &line("a3", 100)),
            turn(6, TurnRole::User, &line("q4", 100)),
            turn(7, TurnRole::Assistant, &line("a4", 100)),
        ];
        let previous = checkpoint(1, "q1 wants the report in metric units");
        let plan = plan_compaction(&turns, Some(&previous), 200).expect("should replan");
        assert_eq!(plan.new_up_to_seq, 5);
        assert!(plan.input_text.contains("metric units"), "checkpoint lost");
        assert!(
            plan.input_text.contains("q1"),
            "the person's first message must be re-sent verbatim"
        );
        assert!(plan.input_text.contains("q2"), "newly aged turn missing");
        assert!(plan.input_text.contains("a3"), "newly aged turn missing");
        assert!(!plan.input_text.contains("q4"), "tail stays verbatim");
    }

    /// Re-writing the checkpoint for a few hundred tokens costs more than it
    /// saves and repeats the same facts over and over.
    #[test]
    fn a_re_fold_too_small_to_matter_is_skipped() {
        let mut turns = vec![
            turn(0, TurnRole::User, &line("q1", 100)),
            turn(1, TurnRole::Assistant, &line("a1", 100)),
            turn(2, TurnRole::User, &line("q2", 100)),
            turn(3, TurnRole::Assistant, &line("a2", 100)),
            turn(4, TurnRole::User, &line("q3", 100)),
            turn(5, TurnRole::Assistant, &line("a3", 100)),
        ];
        turns.push(turn(6, TurnRole::User, &line("q4", 5)));
        turns.push(turn(7, TurnRole::Assistant, &line("a4", 5)));
        let previous = checkpoint(5, "Earlier checkpoint");
        // keep=2000 → a re-fold needs at least 200 newly-aged tokens; the two
        // new turns carry ten.
        assert!(plan_compaction(&turns, Some(&previous), 2_000).is_none());
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
    fn clean_summary_strips_think_and_analysis() {
        assert_eq!(
            clean_summary("<think>pondering</think>\n\nThe summary."),
            "The summary."
        );
        assert_eq!(
            clean_summary("<analysis>walking it through</analysis>\n\n## Objective\n- done"),
            "## Objective\n- done"
        );
        // An unterminated block is left alone rather than eating the summary.
        assert_eq!(
            clean_summary("<analysis>never closed\n\n## Objective"),
            "<analysis>never closed\n\n## Objective"
        );
    }

    /// A checkpoint that skipped a section has silently dropped a whole class
    /// of context — the person's own words, the standing rules.
    #[test]
    fn the_template_check_requires_every_heading() {
        let complete = aiplane_core::server::prompts::SUMMARY_HEADINGS
            .iter()
            .map(|heading| format!("{heading}\n- (none)"))
            .collect::<Vec<_>>()
            .join("\n\n");
        assert!(follows_summary_template(&complete));
        for missing in aiplane_core::server::prompts::SUMMARY_HEADINGS {
            let without = complete.replace(missing, "## Something else");
            assert!(
                !follows_summary_template(&without),
                "{missing} was not required"
            );
        }
        assert!(follows_summary_template(&complete.to_ascii_uppercase()));
    }

    #[tokio::test]
    async fn organization_compaction_revoked_during_summary_http_never_persists_summary() {
        use crate::agent_run::Actor;
        use crate::server::headless::tests::{organization_draft_fixture, state_for};
        use std::sync::Arc;
        use wiremock::{
            Mock, MockServer, ResponseTemplate,
            matchers::{method, path},
        };
        for revoke_model in [false, true] {
            let provider = MockServer::start().await;
            let reached = Arc::new(tokio::sync::Notify::new());
            let checkpoint = reached.clone();
            Mock::given(method("POST")).and(path("/chat/completions"))
        .respond_with(move |_: &wiremock::Request| {
            checkpoint.notify_one();
            ResponseTemplate::new(200).set_delay(std::time::Duration::from_millis(300))
                .set_body_json(serde_json::json!({"choices":[{"message":{"content":"Actual summary"}}]}))
        }).mount(&provider).await;
            let state = state_for(&provider.uri()).await;
            let (profile, params) = organization_draft_fixture(&state).await;
            chat::finalize_turn(
                &state.db,
                &params.assistant_turn_id,
                TurnStatus::Completed,
                None,
            )
            .await
            .unwrap();
            sqlx::query("UPDATE chat_turns SET content='Completed organization answer' WHERE id=?")
                .bind(&params.assistant_turn_id)
                .execute(&state.db)
                .await
                .unwrap();
            for index in 0..4 {
                chat::create_user_turn(
                    &state.db,
                    &params.session_id,
                    &format!("compaction-user-{index}"),
                    // Roughly 1 500 tokens each, so the keep budget below
                    // folds them instead of keeping the lot verbatim.
                    &format!("Actual retained organization task {}", "x".repeat(6_000)),
                )
                .await
                .unwrap();
            }
            aiplane_core::server::settings::store(
                &state.db,
                &state.crypto,
                &[("chat.compaction.keep_tokens".into(), "1000".into())],
            )
            .await
            .unwrap();
            state.reload_settings().await;
            let Actor::Agent(run) = params.actor else {
                panic!("agent expected")
            };
            let access = PoolAccess::for_system_models(&profile.principal, [&*params.model]);
            let payer = Payer::agent(&profile.principal, Some(run.chain().clone()))
                .for_compaction(run, params.model.clone());
            let observed_state = state.clone();
            let session = params.session_id.clone();
            let model = params.model.clone();
            let worker = tokio::spawn(async move {
                run_compaction(
                    &observed_state,
                    &session,
                    &model,
                    &access,
                    &payer,
                    None,
                    None,
                )
                .await
            });
            tokio::time::timeout(std::time::Duration::from_secs(3), reached.notified())
                .await
                .unwrap();
            if revoke_model {
                aiplane_agents::db::system_principals::remove_grant(
                    &state.db,
                    &profile.principal.id,
                    aiplane_core::server::principal::GrantKind::Model,
                    &params.model,
                    "creator",
                )
                .await
                .unwrap();
            } else {
                sqlx::query("UPDATE agent_owners SET active=0 WHERE principal_id=?")
                    .bind(&profile.principal.id)
                    .execute(&state.db)
                    .await
                    .unwrap();
            }
            assert!(
                tokio::time::timeout(std::time::Duration::from_secs(5), worker)
                    .await
                    .unwrap()
                    .unwrap()
                    .is_err()
            );
            assert!(
                chat_compactions::get(&state.db, &params.session_id)
                    .await
                    .unwrap()
                    .is_none()
            );
            assert_eq!(provider.received_requests().await.unwrap().len(), 1);
        }
    }

    /// A summariser that answers the question instead of filling the template
    /// is asked once more with the nudge; only the shaped answer is stored, so
    /// a reply that dropped a section cannot silently become the checkpoint.
    #[tokio::test]
    async fn a_summary_without_the_template_is_asked_once_more_and_then_stored() {
        use crate::agent_run::Actor;
        use crate::server::headless::tests::{organization_draft_fixture, state_for};
        use std::sync::{Arc, Mutex};
        use wiremock::{
            Mock, MockServer, ResponseTemplate,
            matchers::{method, path},
        };

        const SHAPED: &str = "## Objective\n- ask five things\n\n## Requirements and rules\n- (none)\n\n\
             ## Decisions\n- (none)\n\n## Work state\n### Done\n- (none)\n### Active\n- (none)\n\
             ### Blocked\n- (none)\n\n## All user messages\n- Actual retained organization task\n\n\
             ## Important context\n- (none)\n\n## Next steps\n- (none)";
        let provider = MockServer::start().await;
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let recorder = seen.clone();
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(move |request: &wiremock::Request| {
                let body = String::from_utf8_lossy(&request.body).to_string();
                let mut bodies = recorder.lock().unwrap();
                // The first reply is prose; everything after it is the shape.
                let answer = if bodies.is_empty() {
                    "The visitor asked five things."
                } else {
                    SHAPED
                };
                bodies.push(body);
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"choices":[{"message":{"content":answer}}]}))
            })
            .mount(&provider)
            .await;

        let state = state_for(&provider.uri()).await;
        let (profile, params) = organization_draft_fixture(&state).await;
        chat::finalize_turn(
            &state.db,
            &params.assistant_turn_id,
            TurnStatus::Completed,
            None,
        )
        .await
        .unwrap();
        sqlx::query("UPDATE chat_turns SET content='Completed organization answer' WHERE id=?")
            .bind(&params.assistant_turn_id)
            .execute(&state.db)
            .await
            .unwrap();
        for index in 0..4 {
            chat::create_user_turn(
                &state.db,
                &params.session_id,
                &format!("compaction-user-{index}"),
                &format!("Actual retained organization task {}", "x".repeat(6_000)),
            )
            .await
            .unwrap();
        }
        aiplane_core::server::settings::store(
            &state.db,
            &state.crypto,
            &[("chat.compaction.keep_tokens".into(), "1000".into())],
        )
        .await
        .unwrap();
        state.reload_settings().await;

        let Actor::Agent(run) = params.actor else {
            panic!("agent expected")
        };
        let access = PoolAccess::for_system_models(&profile.principal, [&*params.model]);
        let payer = Payer::agent(&profile.principal, Some(run.chain().clone()))
            .for_compaction(run, params.model.clone());
        assert!(
            run_compaction(
                &state,
                &params.session_id,
                &params.model,
                &access,
                &payer,
                Some(30_000),
                None,
            )
            .await
            .unwrap()
        );

        let (asked, nudged) = {
            let bodies = seen.lock().unwrap();
            assert_eq!(bodies.len(), 2, "the nudged answer is the one that counts");
            (bodies.len(), bodies[1].clone())
        };
        assert_eq!(asked, 2);
        assert!(
            nudged.contains("did not use the required headings"),
            "the second call carries the nudge: {nudged}"
        );
        let stored = chat_compactions::get(&state.db, &params.session_id)
            .await
            .unwrap()
            .expect("the shaped checkpoint is stored");
        assert!(stored.summary.contains("## All user messages"));
        assert!(stored.summary.contains("Actual retained organization task"));
    }
}
