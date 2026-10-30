// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! LLM-driven session title generation.
//!
//! Triggered by `chat_message_send` the moment the first user message
//! lands on an untitled session. Runs as a separate `tokio::spawn`
//! task so it doesn't block the main chat response. The prompt asks
//! the model for a 3-6 word title with no preamble or reasoning;
//! we strip `<think>…</think>` blocks defensively in case the
//! upstream's reasoning parser leaks them through.
//!
//! Once the title lands in `chat_sessions.title`, we push a
//! `TurnUpdate::SidebarChanged` through the live worker's broadcast
//! (if any) so the sidebar row updates in place without waiting for
//! the user's next navigation.
//!
//! The call is a side call ([`side_call::ask_text`]): it passes the person's
//! spend limits first and is a usage row of theirs.

use std::sync::Arc;
use std::time::Duration;

use session_core::TurnUpdate;

use aiplane_core::server::db::usage::UsageSource;
use aiplane_core::server::db::users::User;
use aiplane_core::server::upstreams::PoolAccess;
use aiplane_runtime::rama_server::state::RamaState;
use aiplane_runtime::server::side_call::{self, Payer, SideCall, strip_think_block};
use serde_json::Value;
use session_core::db as chat;

/// Hard char cap on the generated title. The sidebar's 18rem column
/// fits roughly 32-36 characters at the 14px session-row font size
/// before ellipsizing, so we land just past that — the CSS still
/// truncates, but the DB row stays close to what's visible.
const MAX_TITLE_LEN: usize = 40;

/// Independent word cap. A 5-word title made of single-character
/// words is fine; a 5-word title where every word is `Functionality`
/// hits the char cap first. Whichever cap fires first wins.
const MAX_TITLE_WORDS: usize = 5;

/// Hard timeout so a sticky upstream can't keep this background task
/// around indefinitely. The chat worker itself doesn't have a
/// timeout (the user can stop it), but title-gen is fire-and-forget
/// with no stop button.
const TIMEOUT: Duration = Duration::from_secs(15);

/// System prompt: terse + concrete length bound + examples so
/// reasoning models that follow instructions land in a sensible
/// place. We *also* clamp server-side to be sure — chat models
/// regularly produce 8-word "perfectly accurate" titles.
/// The trailing `/no_think` is Qwen3's reasoning-off marker —
/// honored by Qwen3-family vLLM deployments, treated as harmless
/// junk by everything else.
const SYSTEM_PROMPT: &str = "You name chat conversations.\n\
Output ONLY a short headline (2-4 words, max 30 characters) for the user's message. \
No quotes, no punctuation, no preamble, no reasoning. \
Capitalize like a headline.\n\
Examples:\n\
Ceph vs SeaweedFS\n\
Postgres tuning help\n\
Rust async question\n\
/no_think";

/// Run a title-generation pass against the upstream and persist the
/// result. Best-effort — every failure path logs and returns without
/// touching the DB (so the session stays "Untitled chat" until the
/// next message lands or the user renames it). `model` defaults to
/// the same one the user picked for the conversation; the call is a usage
/// row of `user`'s, under their spend limits.
pub(super) async fn generate_session_title(
    state: Arc<RamaState>,
    user: User,
    session_id: String,
    user_msg: String,
    model: String,
) {
    let payer = Payer::person(
        &user.id,
        &user.roles,
        Some(user.email.clone()),
        UsageSource::Chat,
    );
    let user_id = user.id;
    tracing::info!(
        %session_id,
        %model,
        msg_len = user_msg.len(),
        "title generation: starting"
    );
    // The title call carries the user's message, so it must not reach a model
    // the conversation's level does not clear. The level is read once the
    // turn has been through its classification (an upload or the typed text
    // can raise it), not when the message was accepted.
    if state.scheme().is_active() {
        wait_for_turns_to_settle(&state, &session_id).await;
        if !state
            .model_cleared_for_session(&session_id, &model, &PoolAccess::all())
            .await
        {
            tracing::info!(%session_id, %model, "title generation: model not cleared for the conversation's level");
            return;
        }
    }
    let raw = match call_upstream(&state, &payer, &model, &user_msg).await {
        Ok(r) => r,
        Err(err) => {
            tracing::warn!(error = %err, %session_id, %model, "title generation: upstream failed");
            return;
        }
    };
    tracing::info!(
        %session_id,
        content_len = raw.content.len(),
        reasoning_len = raw.reasoning_content.len(),
        finish_reason = %raw.finish_reason,
        "title generation: response received"
    );
    let title = clean_title(&raw.content);
    if title.is_empty() {
        // Two common upstream shapes land here:
        //   * reasoning-parser pulled the model's entire output into
        //     `reasoning_content` and left `content` empty. The
        //     `chat_template_kwargs.enable_thinking=false` knob in
        //     the request body is meant to prevent that on vLLM
        //     Qwen3 — if you still see this branch, the upstream
        //     either ignored the knob or the chat template doesn't
        //     wire it up.
        //   * `finish_reason="length"` — the model ran out of token
        //     budget mid-reasoning. Bumping `max_tokens` in
        //     `call_upstream` is the answer.
        tracing::warn!(
            %session_id,
            finish_reason = %raw.finish_reason,
            content_len = raw.content.len(),
            reasoning_len = raw.reasoning_content.len(),
            content_prefix = %raw.content.chars().take(200).collect::<String>(),
            reasoning_prefix = %raw.reasoning_content.chars().take(200).collect::<String>(),
            "title generation: empty content after cleanup"
        );
        return;
    }
    tracing::info!(%session_id, title = %title, "title generation: persisting");
    if let Err(err) = chat::set_session_title(&state.db, &session_id, &title).await {
        tracing::warn!(error = %err, %session_id, "title generation: DB write failed");
        return;
    }

    // Push a live update so the sidebar reflects the rename without
    // waiting for the user's next nav. Only meaningful when this
    // session's worker is still being tailed; if it's already
    // finalised (slow title gen) the sidebar will pick up the new
    // title on the user's next page interaction.
    if let Some(worker) = state.chats.get(&user_id, &session_id) {
        let _ = worker.broadcast.send(TurnUpdate::SidebarChanged);
    }
}

const SETTLE_POLL: Duration = Duration::from_millis(500);
const SETTLE_LIMIT: Duration = Duration::from_secs(20 * 60);

/// Wait until the conversation has no unfinished work, bounded: a turn
/// streaming, a message queued behind another turn, or one claimed from the
/// queue that has not started yet. Each of them can still raise the level, and
/// the title call carries the message. Work still going after the bound is not
/// waited for further: the title is then checked against whatever level the
/// conversation holds.
async fn wait_for_turns_to_settle(state: &RamaState, session_id: &str) {
    let started = std::time::Instant::now();
    while started.elapsed() < SETTLE_LIMIT {
        let unfinished = chat::has_unfinished_work(&state.db, session_id)
            .await
            .unwrap_or(false);
        if !unfinished {
            return;
        }
        tokio::time::sleep(SETTLE_POLL).await;
    }
}

/// Just the fields the caller wants to inspect on the upstream's
/// `choices[0].message` — visible content, reasoning-parser-split
/// content (vLLM extension; empty everywhere else), and the
/// `finish_reason` so an operator can tell a `length` truncation
/// apart from a `stop` with empty output.
struct UpstreamReply {
    content: String,
    reasoning_content: String,
    finish_reason: String,
}

/// One side call. Hard-capped output tokens so even an unruly model can't
/// burn time generating an essay; no tools. The access is the gateway's
/// own, as for the chat model the person already runs their turn on.
///
/// Reasoning is switched off three ways, in order of how reliably they work
/// across upstreams: `chat_template_kwargs.enable_thinking: false` (the only
/// way to make a vLLM-served Qwen3 skip its `<think>` block), Qwen3's
/// `/no_think` directive appended to the user message, and the system
/// prompt itself saying "no reasoning". `max_tokens: 256` leaves room for a
/// small reasoning preamble that slips through anyway and a 3-6 word title.
async fn call_upstream(
    state: &RamaState,
    payer: &Payer,
    model: &str,
    user_msg: &str,
) -> Result<UpstreamReply, String> {
    let answered = side_call::ask_text(
        state,
        payer,
        SideCall {
            purpose: "session_title",
            model,
            access: &PoolAccess::all(),
            instructions: SYSTEM_PROMPT,
            input: user_msg,
            temperature: 0.0,
            max_tokens: Some(256),
            no_think: true,
            timeout: TIMEOUT,
        },
    )
    .await;
    let content = answered.answer.map_err(|e| e.to_string())?;
    let pluck_str = |ptr: &str| -> String {
        answered
            .exchange
            .response
            .pointer(ptr)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    Ok(UpstreamReply {
        content,
        reasoning_content: pluck_str("/choices/0/message/reasoning_content"),
        finish_reason: pluck_str("/choices/0/finish_reason"),
    })
}

/// Squeeze the model's response into a title-shaped string:
/// - strip a leading `<think>…</think>` block if present (some
///   reasoning-parser adapters leak it through despite the prompt)
/// - take the first non-empty line
/// - drop surrounding quotes / asterisks / colons / trailing dots
/// - clamp to `MAX_TITLE_WORDS` words, *then* `MAX_TITLE_LEN` chars,
///   so a chatty model still produces a sidebar-friendly result
/// - if the char clamp lands mid-word, drop the half-word so we
///   don't get `Generation Functionali…`
fn clean_title(raw: &str) -> String {
    let after_think = strip_think_block(raw);
    let first_line = after_think
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let trimmed = first_line.trim_matches(|c: char| {
        c.is_whitespace() || matches!(c, '"' | '\'' | '*' | '#' | '-' | '`')
    });
    let trimmed = trimmed.trim_end_matches(['.', ':', '!', '?']).trim();

    // Word cap first.
    let by_words: String = trimmed
        .split_whitespace()
        .take(MAX_TITLE_WORDS)
        .collect::<Vec<_>>()
        .join(" ");

    // Then char cap. If we land mid-word, drop back to the last word
    // boundary so we don't get an ugly half-word + ellipsis later.
    if by_words.chars().count() <= MAX_TITLE_LEN {
        return by_words;
    }
    let clipped: String = by_words.chars().take(MAX_TITLE_LEN).collect();
    match clipped.rfind(' ') {
        Some(idx) if idx > 0 => clipped[..idx].to_string(),
        _ => clipped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_title_strips_quotes_and_caps_length() {
        assert_eq!(clean_title(r#""Hello World""#), "Hello World");
        assert_eq!(clean_title("  *Foo Bar*  "), "Foo Bar");
        assert_eq!(clean_title("title: Some Name."), "title: Some Name"); // colon mid-string stays
        let long: String = "x".repeat(200);
        assert_eq!(clean_title(&long).len(), MAX_TITLE_LEN);
    }

    #[test]
    fn clean_title_clips_to_word_cap() {
        // 7 words → first MAX_TITLE_WORDS (5) survive.
        assert_eq!(
            clean_title("Alpha Beta Gamma Delta Epsilon Zeta Eta"),
            "Alpha Beta Gamma Delta Epsilon"
        );
    }

    #[test]
    fn clean_title_char_cap_falls_back_to_word_boundary() {
        // Real-world case from the user: 5 words but 46 chars. The
        // char cap fires mid-word ("Functionality") and we drop back
        // to the previous space rather than emitting "Functio".
        let raw = "Testing Session Title Generation Functionality";
        let out = clean_title(raw);
        assert!(
            out.len() <= MAX_TITLE_LEN,
            "len {} should be <= {}",
            out.len(),
            MAX_TITLE_LEN
        );
        assert!(
            !out.ends_with("Functio"),
            "should not slice mid-word: {out:?}"
        );
        assert_eq!(out, "Testing Session Title Generation");
    }

    #[test]
    fn clean_title_takes_first_non_empty_line() {
        assert_eq!(
            clean_title("\n\nFirst Real Line\n\nrest"),
            "First Real Line"
        );
    }

    #[test]
    fn clean_title_strips_think_block() {
        let raw = "<think>let me think about this</think>\n\nActual Title";
        assert_eq!(clean_title(raw), "Actual Title");
    }

    #[test]
    fn clean_title_case_insensitive_think_tags() {
        let raw = "<THINK>internal</THINK>\nThe Title";
        assert_eq!(clean_title(raw), "The Title");
    }

    #[test]
    fn clean_title_passes_through_when_only_open_tag() {
        // Conservative: we don't strip half-baked thinking blocks
        // because the rest of the response might be misleading.
        let raw = "<think>oops no close\nbut this is the title";
        assert!(clean_title(raw).contains("<think>"));
    }
}
