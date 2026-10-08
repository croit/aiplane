// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The "Denkaufwand" (German for "thinking effort") control: one user-chosen
//! level that drives both the upstream reasoning budget *and* the per-turn
//! tool-round cap.
//!
//! One scale everywhere — `off · low · medium · high · xhigh` — the same words
//! the OpenAI Responses API and Codex use, so a `/v1/responses` client's
//! `reasoning.effort` arrives as the level of the same name. [`apply_effort`]
//! translates the level into the parameter the serving backend understands,
//! because the backends we target express "think harder" differently:
//!
//!   - `Qwen` → `chat_template_kwargs.enable_thinking` (bool) plus the
//!     template's own `reasoning_effort` (`low`|`medium`|`xhigh`); Qwen3.8's
//!     template defaults to `xhigh` when it is missing, so leaving it out is
//!     what made every Qwen turn think at the maximum. An optional token cap
//!     rides in the spelling the *server* enforces ([`ThinkingBudget`]).
//!   - `OpenAI` → `reasoning_effort` ("low"|"medium"|"high"); it cannot switch
//!     reasoning off, so it offers no `off` level.
//!   - `Ollama` → `reasoning_effort` ("none"…"max").
//!   - `GLM`/z.AI → `thinking.type` ("enabled"|"disabled") +
//!     `reasoning_effort` ("none"…"max") intensity
//!   - `Anthropic` → `thinking.{type,budget_tokens}`
//!   - everything else → nothing
//!
//!   The per-level budgets/levels have built-in defaults but can be tuned per
//!   model on `/admin/models` via [`ReasoningOverrides`]. A tool-round cap
//!   ([`Effort::max_rounds`]) rides on the same level, so an agentic task that
//!   needs many tool calls can be given more headroom without a second knob.
//!
//! Like `model_defaults`, the merge is *client-wins*: a parameter the request
//! already carries is never overwritten. The chat composer never sets these,
//! so on the chat path the effort always applies; a `/v1` client that sets its
//! own reasoning param keeps it.

use serde_json::{Value, json};

/// Hard ceiling on the per-turn tool-round cap, regardless of effort. Matches
/// the most-headroom effort level ([`Effort::Xhigh`]) and bounds the blast radius
/// of a runaway tool loop.
pub const HARD_ROUND_CAP: u32 = 64;

/// Qwen exposes reasoning through `chat_template_kwargs`; these name the exact
/// wire keys so the request-shaping code has a single source of truth and
/// can't drift on a typo.
const QWEN_CHAT_TEMPLATE_KWARGS: &str = "chat_template_kwargs";
const QWEN_ENABLE_THINKING: &str = "enable_thinking";
const QWEN_REASONING_EFFORT: &str = "reasoning_effort";

/// Anthropic thinking-token budgets per level (see [`anthropic_budget`]). Kept
/// modest so `max_tokens` (which must exceed the budget) stays reasonable.
const ANTHROPIC_BUDGET_LOW: u32 = 2_048;
const ANTHROPIC_BUDGET_MEDIUM: u32 = 4_096;
const ANTHROPIC_BUDGET_HIGH: u32 = 16_384;
const ANTHROPIC_BUDGET_XHIGH: u32 = 32_768;

/// The smallest thinking-token budget an admin may set. Anthropic refuses
/// anything under 1024; an open-weight model cut off much earlier carries on
/// thinking in its *answer* instead (Qwen3.8 on SGLang, capped at 300 tokens,
/// wrote 90 KB of visible reasoning, `</think>` and all), while 1000 and up
/// end cleanly.
pub const MIN_THINKING_BUDGET: u32 = 1_024;

/// The user-chosen effort level for a conversation. Persisted as the lowercase
/// string in `chat_session_settings.effort`; [`Effort::Low`] is the default
/// for a missing row / unknown value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Effort {
    /// Reasoning off where the backend can switch it off.
    Off,
    /// Brief reasoning — enough for most chat questions, and the default.
    #[default]
    Low,
    Medium,
    High,
    /// Maximum reasoning + tool headroom — the hardest multi-step tasks.
    Xhigh,
}

impl Effort {
    /// Every level, least to most thinking.
    pub const ALL: [Self; 5] = [Self::Off, Self::Low, Self::Medium, Self::High, Self::Xhigh];

    /// Parse a stored or posted string; `None` for anything that is not a
    /// level.
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|e| e.as_str() == s.trim())
    }

    /// Parse the stored string. `None` / unknown → [`Effort::Low`], so a
    /// missing row degrades to the default.
    pub fn from_db(s: Option<&str>) -> Self {
        s.and_then(Self::parse).unwrap_or_default()
    }

    /// The canonical lowercase string persisted in the DB and posted by the UI.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Xhigh => "xhigh",
        }
    }

    /// Per-turn tool-round cap for this level. Bounded by [`HARD_ROUND_CAP`].
    /// `low` keeps the 16 rounds the old default had, so making it the default
    /// thinks less without cutting agentic chats short.
    pub fn max_rounds(self) -> u32 {
        match self {
            Self::Off => 8,
            Self::Low | Self::Medium => 16,
            Self::High => 32,
            Self::Xhigh => HARD_ROUND_CAP,
        }
    }

    /// The next level down, for a turn retried after a loop; `None` at
    /// [`Effort::Off`].
    pub fn lower(self) -> Option<Self> {
        match self {
            Self::Off => None,
            Self::Low => Some(Self::Off),
            Self::Medium => Some(Self::Low),
            Self::High => Some(Self::Medium),
            Self::Xhigh => Some(Self::High),
        }
    }

    /// The level a round that looped at `self` is retried at: the next one
    /// down that `style` offers *and* that puts a different request on the
    /// wire. OpenAI sends `high` for both `high` and `xhigh`, and Qwen sends
    /// `xhigh` for both unless a budget tells them apart; a retry at such a
    /// level would resend the request that just looped. `None` when nothing
    /// lower changes anything.
    pub fn retry_level(
        self,
        style: ReasoningStyle,
        overrides: &ReasoningOverrides,
        budget: Option<ThinkingBudget>,
    ) -> Option<Self> {
        let wire = |effort| {
            let mut body = json!({});
            apply_effort(style, effort, overrides, budget, &mut body);
            body
        };
        let current = wire(self);
        let mut next = self.lower();
        while let Some(level) = next {
            if style.efforts().contains(&level) && wire(level) != current {
                return Some(level);
            }
            next = level.lower();
        }
        None
    }

    /// Whether reasoning is enabled at all at this level.
    fn reasoning_on(self) -> bool {
        !matches!(self, Self::Off)
    }
}

/// How a server enforces a per-request cap on thinking tokens. A server fact,
/// read when the backend is identified (`upstreams::profile`): a server that
/// does not know a spelling drops it without an error, so sending one it
/// cannot enforce would leave the admin's budget silently doing nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThinkingBudget {
    /// vLLM: top-level `thinking_token_budget`.
    TokenBudget,
    /// SGLang started with `--enable-strict-thinking`: the reasoning grammar
    /// reads `custom_params.thinking_budget`.
    CustomParams,
}

impl ThinkingBudget {
    /// The canonical string, as stored in `backend_detected.thinking_budget`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TokenBudget => "token_budget",
            Self::CustomParams => "custom_params",
        }
    }

    /// Parse a stored value; unknown / missing → `None`, which sends no budget.
    pub fn parse(s: Option<&str>) -> Option<Self> {
        match s.map(str::trim) {
            Some("token_budget") => Some(Self::TokenBudget),
            Some("custom_params") => Some(Self::CustomParams),
            _ => None,
        }
    }
}

/// How a model expresses its reasoning budget on the wire. Configured per model
/// on `/admin/models` (`model_defaults.reasoning_style`); `None`/unset
/// auto-detects from the model name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ReasoningStyle {
    /// No reasoning support — the effort knob is a no-op (e.g. Voxtral).
    #[default]
    None,
    /// vLLM chat-template flag (`chat_template_kwargs.enable_thinking`), the
    /// Qwen3 convention.
    Qwen,
    /// OpenAI `reasoning_effort` ("low"|"medium"|"high").
    OpenAi,
    /// z.AI / GLM `thinking.type` ("enabled"|"disabled").
    Glm,
    /// Anthropic `thinking.{type, budget_tokens}`.
    Anthropic,
    /// Ollama `reasoning_effort`, which takes the wider scale
    /// `"none"|"low"|"medium"|"high"|"max"`.
    ///
    /// Distinct from [`OpenAi`](Self::OpenAi) for one reason that matters to
    /// the user: it has an *off*. OpenAI's reasoning models always reason, so
    /// that style maps Fast to the cheapest `"low"` — send that to Ollama and
    /// "Fast" still thinks, which is a control that does not do what its label
    /// says. Ollama accepts `"none"`, so Fast can mean fast.
    ///
    /// Never auto-detected from a model name; only a backend identified as
    /// Ollama resolves to it (see `upstreams::profile`).
    Ollama,
}

impl ReasoningStyle {
    /// Resolve the effective style, most specific source first:
    ///
    ///   1. an explicit admin choice on `/admin/models`, including `"none"`
    ///      (which lets an admin silence a model name-detection would enable);
    ///   2. the spelling the *serving backend* dictates, when it dictates one
    ///      **and** the model reasons at all;
    ///   3. auto-detection from the model name.
    ///
    /// Step 2 is what makes the effort control work on a server that
    /// re-encodes requests through its own API. Ollama discards
    /// `chat_template_kwargs` — it has no such field — so a model called
    /// `qwen3:8b` would get Qwen's spelling from step 3, have it dropped in
    /// silence, and leave the user with a knob that does nothing. Servers that
    /// pass model parameters through (vLLM, llama.cpp, SGLang) dictate nothing
    /// and fall to step 3, where the model family really is the right signal.
    ///
    /// The admin choice still wins over the backend: an operator who has
    /// established what a particular deployment wants should not be overruled
    /// by a fingerprint.
    pub fn resolve(explicit: Option<&str>, dialect: Option<Self>, model: &str) -> Self {
        match explicit.map(str::trim) {
            Some("qwen") => Self::Qwen,
            Some("openai") => Self::OpenAi,
            Some("glm") => Self::Glm,
            Some("anthropic") => Self::Anthropic,
            Some("ollama") => Self::Ollama,
            Some("none") => Self::None,
            // Empty string / "auto" / unknown / missing → the backend, then
            // the model name.
            //
            // The backend may only *re-spell* reasoning, never switch it on.
            // `detect` returns `None` for a model it does not recognise, and
            // that answer is load-bearing: it is what stops a reasoning
            // parameter being sent to a model that has no reasoning. Letting
            // the dialect win outright made every model on an Ollama backend
            // "thinking" — and Ollama does not ignore that, it refuses it:
            //
            //     HTTP 400  "gemma3:270m" does not support thinking
            //
            // So an Ollama serving one ordinary model went from "the effort
            // knob does nothing" to "every request fails". Which half decides
            // what is exactly the split this module is built on: whether a
            // model thinks is the model's answer, how to say so is the
            // server's.
            _ => match Self::detect(model) {
                Self::None => Self::None,
                detected => dialect.unwrap_or(detected),
            },
        }
    }

    /// Best-effort guess from a model id. Conservative: an unrecognised model
    /// maps to `None` (the effort knob simply does nothing) rather than risk
    /// injecting a parameter the backend rejects.
    pub fn detect(model: &str) -> Self {
        let m = model.to_ascii_lowercase();
        // Order matters only where substrings could overlap; these don't.
        if m.contains("qwen") {
            Self::Qwen
        } else if m.contains("gpt")
            || m.starts_with("o1")
            || m.starts_with("o3")
            || m.starts_with("o4")
        {
            Self::OpenAi
        } else if m.contains("glm") || m.contains("z-ai") || m.contains("zhipu") {
            Self::Glm
        } else if m.contains("claude") || m.contains("anthropic") {
            Self::Anthropic
        } else {
            // Voxtral and any unrecognised model: no reasoning parameter.
            Self::None
        }
    }

    /// Canonical string (round-trips with [`Self::resolve`]).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Qwen => "qwen",
            Self::OpenAi => "openai",
            Self::Glm => "glm",
            Self::Anthropic => "anthropic",
            Self::Ollama => "ollama",
        }
    }

    /// Whether this style is tuned by a numeric *token budget* per effort
    /// (`thinking_token_budget` / `thinking.budget_tokens`). The admin UI shows
    /// integer token fields for these.
    pub fn uses_token_budget(self) -> bool {
        matches!(self, Self::Qwen | Self::Anthropic)
    }

    /// Whether a token budget set for this style is actually enforced when
    /// `budget` is how the serving backend caps thinking. Anthropic's budget
    /// is part of its own wire format; Qwen's needs a server that enforces
    /// one. The admin page offers budget fields only where this holds.
    pub fn budget_enforced(self, budget: Option<ThinkingBudget>) -> bool {
        match self {
            Self::Anthropic => true,
            Self::Qwen => budget.is_some(),
            Self::None | Self::OpenAi | Self::Glm | Self::Ollama => false,
        }
    }

    /// Whether this style is tuned by a categorical `reasoning_effort` level
    /// per effort (OpenAI, GLM/z.AI — neither exposes a token cap). The admin
    /// UI shows a level dropdown for these.
    pub fn uses_effort_level(self) -> bool {
        matches!(self, Self::OpenAi | Self::Glm | Self::Ollama)
    }

    /// The effort levels the composer offers for this style: none when the
    /// model has no reasoning control, every level but `off` when reasoning
    /// cannot be switched off (OpenAI's reasoning models always reason), all
    /// of them otherwise.
    pub fn efforts(self) -> &'static [Effort] {
        match self {
            Self::None => &[],
            Self::OpenAi => &Effort::ALL[1..],
            Self::Qwen | Self::Glm | Self::Anthropic | Self::Ollama => &Effort::ALL,
        }
    }

    /// Allowed `reasoning_effort` values for this style, most→least thinking.
    /// Empty for token-budget / no-reasoning styles. Single source of truth for
    /// both the admin dropdown and save-time validation.
    pub fn effort_levels(self) -> &'static [&'static str] {
        match self {
            // OpenAI reasoning models accept only these three.
            Self::OpenAi => &["high", "medium", "low"],
            // z.AI / GLM accepts the full intensity scale.
            Self::Glm => &["max", "xhigh", "high", "medium", "low", "minimal", "none"],
            // Ollama's scale, including the "none" that makes Fast mean off.
            Self::Ollama => &["max", "high", "medium", "low", "none"],
            _ => &[],
        }
    }
}

/// Anthropic thinking-token budget per level; `None` (off) disables thinking.
fn anthropic_budget(effort: Effort) -> Option<u32> {
    match effort {
        Effort::Off => None,
        Effort::Low => Some(ANTHROPIC_BUDGET_LOW),
        Effort::Medium => Some(ANTHROPIC_BUDGET_MEDIUM),
        Effort::High => Some(ANTHROPIC_BUDGET_HIGH),
        Effort::Xhigh => Some(ANTHROPIC_BUDGET_XHIGH),
    }
}

/// Qwen3.8's template knows `low`, `medium` and `xhigh` and raises on anything
/// else; `high` takes `xhigh` and is told apart by its budget and round cap.
/// Templates of older Qwen generations ignore the variable.
fn qwen_effort(effort: Effort) -> &'static str {
    match effort {
        Effort::Off | Effort::Low => "low",
        Effort::Medium => "medium",
        Effort::High | Effort::Xhigh => "xhigh",
    }
}

/// OpenAI `reasoning_effort` value per level. OpenAI reasoning models always
/// reason, so `off` (never offered for this style, but reachable from a `/v1`
/// client) takes the cheapest level.
fn openai_effort(effort: Effort) -> &'static str {
    match effort {
        Effort::Off | Effort::Low => "low",
        Effort::Medium => "medium",
        Effort::High | Effort::Xhigh => "high",
    }
}

/// Ollama `reasoning_effort` value per level. Unlike OpenAI's three-value
/// scale this one has an off switch, so `off` genuinely stops the model
/// thinking rather than making it think cheaply.
fn ollama_effort(effort: Effort) -> &'static str {
    match effort {
        Effort::Off => "none",
        Effort::Low => "low",
        Effort::Medium => "medium",
        Effort::High => "high",
        Effort::Xhigh => "max",
    }
}

/// GLM / z.AI `reasoning_effort` value per *thinking* level; `off` disables
/// thinking instead (see [`apply_effort`]). z.AI's own default is `"max"`, so
/// mapping the level here is what reins GLM in.
fn glm_effort(effort: Effort) -> &'static str {
    match effort {
        Effort::Off | Effort::Low => "low",
        Effort::Medium => "medium",
        Effort::High => "high",
        Effort::Xhigh => "max",
    }
}

/// Per-model, per-level overrides for the reasoning budget, configured on
/// `/admin/models` and stored in `model_defaults`. Two parallel
/// representations because backends differ (see [`ReasoningStyle`]):
///
///   * `budget_*` — integer token caps, used by token-budget styles
///     ([`ReasoningStyle::uses_token_budget`]).
///   * `effort_*` — categorical `reasoning_effort` levels, used by
///     effort-level styles ([`ReasoningStyle::uses_effort_level`]).
///
/// A `None` field means "use the built-in default for that style+level". There
/// is no `off` field: `off` means reasoning off and has nothing to tune.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReasoningOverrides {
    pub budget_low: Option<u32>,
    pub budget_medium: Option<u32>,
    pub budget_high: Option<u32>,
    pub budget_xhigh: Option<u32>,
    pub effort_low: Option<String>,
    pub effort_medium: Option<String>,
    pub effort_high: Option<String>,
    pub effort_xhigh: Option<String>,
}

/// The model's reasoning configuration: how it spells "think harder", and the
/// per-level budgets an admin tuned for it on `/admin/models`.
///
/// Both halves come from the same `model_defaults` row, and every caller that
/// wants one wants the other — the chat driver and the `/v1/messages`
/// compatibility layer had the identical lookup before this existed. A model
/// with no row takes the built-in budgets and resolves its style from
/// `dialect` (the serving backend, see `upstreams::ServingProfile`) or, failing
/// that, its own name — which is the common case.
pub async fn resolve_for_model(
    pool: &crate::server::db::Pool,
    model: &str,
    dialect: Option<ReasoningStyle>,
) -> (ReasoningStyle, ReasoningOverrides) {
    let row = crate::server::db::model_defaults::get(pool, model)
        .await
        .ok()
        .flatten();
    let style = ReasoningStyle::resolve(
        row.as_ref().and_then(|r| r.reasoning_style.as_deref()),
        dialect,
        model,
    );
    let overrides = row
        .as_ref()
        .map(ReasoningOverrides::from_row)
        .unwrap_or_default();
    (style, overrides)
}

impl ReasoningOverrides {
    /// The per-model overrides an admin set on `/admin/models`.
    ///
    /// Lives here rather than at a call site because every path that resolves
    /// a reasoning parameter — the chat driver, the `/v1/messages`
    /// compatibility layer — needs the same translation from the stored row.
    pub fn from_row(row: &crate::server::db::model_defaults::ModelDefaults) -> Self {
        let budget = |v: Option<i64>| v.and_then(|n| u32::try_from(n).ok());
        Self {
            budget_low: budget(row.thinking_budget_low),
            budget_medium: budget(row.thinking_budget_medium),
            budget_high: budget(row.thinking_budget_high),
            budget_xhigh: budget(row.thinking_budget_xhigh),
            effort_low: row.reasoning_effort_low.clone(),
            effort_medium: row.reasoning_effort_medium.clone(),
            effort_high: row.reasoning_effort_high.clone(),
            effort_xhigh: row.reasoning_effort_xhigh.clone(),
        }
    }

    /// The token-budget override for `effort`, if any. `off` never has one.
    fn budget(&self, effort: Effort) -> Option<u32> {
        match effort {
            Effort::Off => None,
            Effort::Low => self.budget_low,
            Effort::Medium => self.budget_medium,
            Effort::High => self.budget_high,
            Effort::Xhigh => self.budget_xhigh,
        }
    }

    /// The `reasoning_effort` override for `effort`, if any. `off` never has
    /// one.
    fn effort_level(&self, effort: Effort) -> Option<&str> {
        match effort {
            Effort::Off => None,
            Effort::Low => self.effort_low.as_deref(),
            Effort::Medium => self.effort_medium.as_deref(),
            Effort::High => self.effort_high.as_deref(),
            Effort::Xhigh => self.effort_xhigh.as_deref(),
        }
    }
}

/// Translate `effort` into the backend-specific reasoning parameter for
/// `style` and merge it into `body`. `overrides` carries the per-model,
/// per-level tuning from `/admin/models`; pass [`ReasoningOverrides::default`]
/// for the built-in behaviour. `budget` is how the serving backend enforces a
/// thinking-token cap, if it can: a Qwen budget is only sent in a spelling the
/// server will act on. Client-wins: a key the request already set is left
/// untouched. No-op for [`ReasoningStyle::None`].
pub fn apply_effort(
    style: ReasoningStyle,
    effort: Effort,
    overrides: &ReasoningOverrides,
    budget: Option<ThinkingBudget>,
    body: &mut Value,
) {
    let Some(obj) = body.as_object_mut() else {
        return;
    };
    match style {
        ReasoningStyle::None => {}
        ReasoningStyle::Qwen => {
            // A top-level `reasoning_effort` is the client's choice too: SGLang
            // folds it into the template variables itself.
            let client_effort = obj.contains_key(QWEN_REASONING_EFFORT);
            // chat_template_kwargs is a nested object; merge the flags without
            // clobbering other kwargs the client may have set.
            let kwargs = obj
                .entry(QWEN_CHAT_TEMPLATE_KWARGS)
                .or_insert_with(|| json!({}));
            if let Some(k) = kwargs.as_object_mut() {
                if !k.contains_key(QWEN_ENABLE_THINKING) {
                    k.insert(
                        QWEN_ENABLE_THINKING.into(),
                        Value::Bool(effort.reasoning_on()),
                    );
                }
                if effort.reasoning_on() && !client_effort && !k.contains_key(QWEN_REASONING_EFFORT)
                {
                    k.insert(
                        QWEN_REASONING_EFFORT.into(),
                        Value::String(qwen_effort(effort).into()),
                    );
                }
            }
            if effort.reasoning_on()
                && let Some(spelling) = budget
                && let Some(tokens) = overrides.budget(effort)
            {
                insert_budget(obj, spelling, tokens);
            }
        }
        // Both spell the parameter the same way and differ only in what each
        // level means: OpenAI's reasoning models always reason, while Ollama's
        // scale has an off. One arm, so the client-wins rule cannot drift
        // between two copies of it.
        ReasoningStyle::OpenAi | ReasoningStyle::Ollama => {
            if !obj.contains_key("reasoning_effort") {
                let level = overrides
                    .effort_level(effort)
                    .unwrap_or_else(|| match style {
                        ReasoningStyle::Ollama => ollama_effort(effort),
                        _ => openai_effort(effort),
                    });
                obj.insert("reasoning_effort".into(), Value::String(level.into()));
            }
        }
        ReasoningStyle::Glm => {
            if !obj.contains_key("thinking") {
                let kind = if effort.reasoning_on() {
                    "enabled"
                } else {
                    "disabled"
                };
                obj.insert("thinking".into(), json!({ "type": kind }));
            }
            // z.AI has no token cap; intensity is `reasoning_effort` and only
            // takes effect while thinking is enabled. Its native default is
            // "max", so mapping the effort knob here is what limits GLM.
            if effort.reasoning_on() && !obj.contains_key("reasoning_effort") {
                let level = overrides.effort_level(effort).unwrap_or(glm_effort(effort));
                obj.insert("reasoning_effort".into(), Value::String(level.into()));
            }
        }
        ReasoningStyle::Anthropic => {
            if !obj.contains_key("thinking") {
                // An explicit per-model budget wins over the built-in default.
                match overrides
                    .budget(effort)
                    .or_else(|| anthropic_budget(effort))
                {
                    Some(budget) => {
                        obj.insert(
                            "thinking".into(),
                            json!({ "type": "enabled", "budget_tokens": budget }),
                        );
                    }
                    None => {
                        obj.insert("thinking".into(), json!({ "type": "disabled" }));
                    }
                }
            }
        }
    }
}

/// Merge a thinking-token cap into `obj` in the server's spelling, unless the
/// client already set one.
fn insert_budget(obj: &mut serde_json::Map<String, Value>, spelling: ThinkingBudget, tokens: u32) {
    match spelling {
        ThinkingBudget::TokenBudget => {
            obj.entry("thinking_token_budget").or_insert(json!(tokens));
        }
        ThinkingBudget::CustomParams => {
            let params = obj.entry("custom_params").or_insert_with(|| json!({}));
            if let Some(p) = params.as_object_mut() {
                p.entry("thinking_budget").or_insert(json!(tokens));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effort_round_trips_and_defaults_to_low() {
        for e in Effort::ALL {
            assert_eq!(Effort::from_db(Some(e.as_str())), e);
            assert_eq!(Effort::parse(e.as_str()), Some(e));
        }
        assert_eq!(Effort::from_db(None), Effort::Low);
        assert_eq!(Effort::from_db(Some("bogus")), Effort::Low);
        // The old scale's names are not levels any more; the migration
        // rewrote every stored one.
        assert_eq!(Effort::parse("standard"), None);
    }

    #[test]
    fn rounds_scale_with_effort_and_are_capped() {
        assert_eq!(Effort::Off.max_rounds(), 8);
        assert_eq!(Effort::Low.max_rounds(), 16);
        assert_eq!(Effort::Medium.max_rounds(), 16);
        assert_eq!(Effort::High.max_rounds(), 32);
        assert_eq!(Effort::Xhigh.max_rounds(), HARD_ROUND_CAP);
    }

    #[test]
    fn lower_steps_down_one_level_and_stops_at_off() {
        assert_eq!(Effort::Xhigh.lower(), Some(Effort::High));
        assert_eq!(Effort::High.lower(), Some(Effort::Medium));
        assert_eq!(Effort::Medium.lower(), Some(Effort::Low));
        assert_eq!(Effort::Low.lower(), Some(Effort::Off));
        assert_eq!(Effort::Off.lower(), None);
    }

    #[test]
    fn a_retry_steps_down_only_to_a_level_that_changes_the_request() {
        let none = ReasoningOverrides::default();
        let retry = |effort: Effort, style, overrides: &ReasoningOverrides| {
            effort.retry_level(style, overrides, Some(ThinkingBudget::CustomParams))
        };
        assert_eq!(
            retry(Effort::Low, ReasoningStyle::Qwen, &none),
            Some(Effort::Off)
        );
        // Qwen sends `xhigh` for both: `high` would resend the same request.
        assert_eq!(
            retry(Effort::Xhigh, ReasoningStyle::Qwen, &none),
            Some(Effort::Medium)
        );
        // A budget on `high` tells them apart.
        let capped = ReasoningOverrides {
            budget_high: Some(4_096),
            ..Default::default()
        };
        assert_eq!(
            retry(Effort::Xhigh, ReasoningStyle::Qwen, &capped),
            Some(Effort::High)
        );
        // OpenAI cannot stop reasoning, and sends `high` for `xhigh` too.
        assert_eq!(retry(Effort::Low, ReasoningStyle::OpenAi, &none), None);
        assert_eq!(
            retry(Effort::Xhigh, ReasoningStyle::OpenAi, &none),
            Some(Effort::Medium)
        );
        // A model without a reasoning control has no lower level at all.
        assert_eq!(retry(Effort::Xhigh, ReasoningStyle::None, &none), None);
    }

    #[test]
    fn a_style_that_cannot_stop_reasoning_offers_no_off() {
        assert_eq!(ReasoningStyle::OpenAi.efforts().first(), Some(&Effort::Low));
        assert_eq!(ReasoningStyle::Qwen.efforts(), &Effort::ALL);
        assert!(ReasoningStyle::None.efforts().is_empty());
    }

    #[test]
    fn style_detect_by_name() {
        assert_eq!(
            ReasoningStyle::detect("Qwen/Qwen3-32B"),
            ReasoningStyle::Qwen
        );
        assert_eq!(ReasoningStyle::detect("gpt-5"), ReasoningStyle::OpenAi);
        assert_eq!(ReasoningStyle::detect("o3-mini"), ReasoningStyle::OpenAi);
        assert_eq!(ReasoningStyle::detect("glm-4.6"), ReasoningStyle::Glm);
        assert_eq!(
            ReasoningStyle::detect("claude-opus-4-8"),
            ReasoningStyle::Anthropic
        );
        // Voxtral and unknowns → no reasoning.
        assert_eq!(
            ReasoningStyle::detect("Voxtral-Small"),
            ReasoningStyle::None
        );
        assert_eq!(
            ReasoningStyle::detect("mystery-model"),
            ReasoningStyle::None
        );
    }

    #[test]
    fn style_explicit_overrides_detection() {
        // An admin can force a style the name wouldn't detect…
        assert_eq!(
            ReasoningStyle::resolve(Some("anthropic"), None, "mystery"),
            ReasoningStyle::Anthropic
        );
        // …or silence one the name would enable.
        assert_eq!(
            ReasoningStyle::resolve(Some("none"), None, "Qwen/Qwen3"),
            ReasoningStyle::None
        );
        // Empty/auto → fall back to detection.
        assert_eq!(
            ReasoningStyle::resolve(Some(""), None, "gpt-4o"),
            ReasoningStyle::OpenAi
        );
        assert_eq!(
            ReasoningStyle::resolve(None, None, "gpt-4o"),
            ReasoningStyle::OpenAi
        );
    }

    /// The failure this exists for: Ollama serving `qwen3:8b`. The model name
    /// says Qwen, so the gateway sent `chat_template_kwargs.enable_thinking` —
    /// a field Ollama has no member for, discarded without an error, leaving
    /// the user with an effort control that did nothing. The serving backend
    /// has to win over the name.
    #[test]
    fn a_backend_dialect_beats_the_model_name() {
        assert_eq!(
            ReasoningStyle::resolve(None, Some(ReasoningStyle::Ollama), "qwen3:8b"),
            ReasoningStyle::Ollama
        );
        // Without a dialect the name still decides — which is right on every
        // server that passes model parameters through untouched.
        assert_eq!(
            ReasoningStyle::resolve(None, None, "qwen3:8b"),
            ReasoningStyle::Qwen
        );
    }

    /// A backend may re-spell reasoning; it may not switch it on.
    ///
    /// Ollama refuses a thinking parameter for a model without thinking —
    /// `HTTP 400 "gemma3:270m" does not support thinking`, verified against
    /// 0.34.0 — so letting the dialect win for every model turned "the effort
    /// knob does nothing" into "every request fails".
    #[test]
    fn a_dialect_does_not_give_reasoning_to_a_model_that_has_none() {
        // The model name says nothing about reasoning: stays off, whatever the
        // server would like to call it.
        assert_eq!(
            ReasoningStyle::resolve(None, Some(ReasoningStyle::Ollama), "gemma3:270m"),
            ReasoningStyle::None
        );
        assert_eq!(
            ReasoningStyle::resolve(None, Some(ReasoningStyle::Ollama), "llama3.2"),
            ReasoningStyle::None
        );
        // A model that does reason still gets the server's spelling.
        assert_eq!(
            ReasoningStyle::resolve(None, Some(ReasoningStyle::Ollama), "qwen3:0.6b"),
            ReasoningStyle::Ollama
        );
        // And an admin who knows better is still not overruled.
        assert_eq!(
            ReasoningStyle::resolve(Some("ollama"), None, "gemma3:270m"),
            ReasoningStyle::Ollama
        );
    }

    /// An operator who has worked out what a particular deployment wants must
    /// not be overruled by a fingerprint.
    #[test]
    fn an_admin_choice_beats_the_backend_dialect() {
        assert_eq!(
            ReasoningStyle::resolve(Some("qwen"), Some(ReasoningStyle::Ollama), "qwen3:8b"),
            ReasoningStyle::Qwen
        );
        assert_eq!(
            ReasoningStyle::resolve(Some("none"), Some(ReasoningStyle::Ollama), "qwen3:8b"),
            ReasoningStyle::None
        );
    }

    fn applied(style: ReasoningStyle, effort: Effort, body: Value) -> Value {
        applied_with(style, effort, &ReasoningOverrides::default(), None, body)
    }

    fn applied_with(
        style: ReasoningStyle,
        effort: Effort,
        overrides: &ReasoningOverrides,
        budget: Option<ThinkingBudget>,
        mut body: Value,
    ) -> Value {
        apply_effort(style, effort, overrides, budget, &mut body);
        body
    }

    /// The point of a separate Ollama style: its scale has an off switch, so
    /// `off` stops the model thinking instead of making it think cheaply.
    #[test]
    fn ollama_off_actually_turns_thinking_off() {
        for (effort, expected) in [
            (Effort::Off, "none"),
            (Effort::Low, "low"),
            (Effort::Medium, "medium"),
            (Effort::High, "high"),
            (Effort::Xhigh, "max"),
        ] {
            let body = applied(ReasoningStyle::Ollama, effort, json!({"model": "qwen3:8b"}));
            assert_eq!(body["reasoning_effort"], json!(expected), "{effort:?}");
        }
    }

    /// A client that set its own value keeps it, like every other style.
    #[test]
    fn ollama_style_does_not_overwrite_a_client_value() {
        let body = applied(
            ReasoningStyle::Ollama,
            Effort::Xhigh,
            json!({"model": "m", "reasoning_effort": "low"}),
        );
        assert_eq!(body["reasoning_effort"], json!("low"));
    }

    #[test]
    fn none_style_is_a_noop() {
        let body = applied(ReasoningStyle::None, Effort::Xhigh, json!({"model": "x"}));
        assert_eq!(body, json!({"model": "x"}));
    }

    #[test]
    fn qwen_off_turns_thinking_off_without_an_effort() {
        let body = applied(ReasoningStyle::Qwen, Effort::Off, json!({"model": "Qwen3"}));
        assert_eq!(
            body["chat_template_kwargs"],
            json!({"enable_thinking": false})
        );
    }

    /// Qwen3.8's template thinks at `xhigh` unless told otherwise, so every
    /// thinking level has to name its effort — the omission is what made every
    /// Qwen turn think at the maximum.
    #[test]
    fn qwen_names_the_template_effort_on_every_thinking_level() {
        for (effort, expected) in [
            (Effort::Low, "low"),
            (Effort::Medium, "medium"),
            (Effort::High, "xhigh"),
            (Effort::Xhigh, "xhigh"),
        ] {
            let body = applied(ReasoningStyle::Qwen, effort, json!({"model": "Qwen3"}));
            assert_eq!(
                body["chat_template_kwargs"],
                json!({"enable_thinking": true, "reasoning_effort": expected}),
                "{effort:?}"
            );
        }
    }

    #[test]
    fn qwen_preserves_other_chat_template_kwargs() {
        let body = applied(
            ReasoningStyle::Qwen,
            Effort::Medium,
            json!({"chat_template_kwargs": {"foo": 1}}),
        );
        assert_eq!(body["chat_template_kwargs"]["foo"], json!(1));
        assert_eq!(body["chat_template_kwargs"]["enable_thinking"], json!(true));
    }

    #[test]
    fn qwen_keeps_a_client_effort_in_either_place() {
        let body = applied(
            ReasoningStyle::Qwen,
            Effort::Low,
            json!({"chat_template_kwargs": {"reasoning_effort": "xhigh"}}),
        );
        assert_eq!(
            body["chat_template_kwargs"]["reasoning_effort"],
            json!("xhigh")
        );

        let body = applied(
            ReasoningStyle::Qwen,
            Effort::Low,
            json!({"reasoning_effort": "medium"}),
        );
        assert_eq!(body["reasoning_effort"], json!("medium"));
        assert!(
            body["chat_template_kwargs"]
                .get("reasoning_effort")
                .is_none()
        );
    }

    #[test]
    fn openai_sets_reasoning_effort() {
        for (effort, expected) in [
            (Effort::Off, "low"),
            (Effort::Low, "low"),
            (Effort::Medium, "medium"),
            (Effort::High, "high"),
            (Effort::Xhigh, "high"),
        ] {
            let body = applied(ReasoningStyle::OpenAi, effort, json!({}));
            assert_eq!(body["reasoning_effort"], json!(expected), "{effort:?}");
        }
    }

    #[test]
    fn glm_sets_thinking_type_and_effort() {
        let body = applied(ReasoningStyle::Glm, Effort::Off, json!({}));
        assert_eq!(body["thinking"]["type"], json!("disabled"));
        assert!(body.get("reasoning_effort").is_none());

        let body = applied(ReasoningStyle::Glm, Effort::High, json!({}));
        assert_eq!(body["thinking"]["type"], json!("enabled"));
        assert_eq!(body["reasoning_effort"], json!("high"));

        let body = applied(ReasoningStyle::Glm, Effort::Xhigh, json!({}));
        assert_eq!(body["reasoning_effort"], json!("max"));
    }

    #[test]
    fn anthropic_sets_budget() {
        let body = applied(ReasoningStyle::Anthropic, Effort::Off, json!({}));
        assert_eq!(body["thinking"]["type"], json!("disabled"));
        let body = applied(ReasoningStyle::Anthropic, Effort::Low, json!({}));
        assert_eq!(body["thinking"]["budget_tokens"], json!(2_048));
        let body = applied(ReasoningStyle::Anthropic, Effort::Xhigh, json!({}));
        assert_eq!(body["thinking"]["type"], json!("enabled"));
        assert_eq!(body["thinking"]["budget_tokens"], json!(32_768));
    }

    #[test]
    fn client_value_wins() {
        let body = applied(
            ReasoningStyle::OpenAi,
            Effort::Off,
            json!({"reasoning_effort": "high"}),
        );
        assert_eq!(body["reasoning_effort"], json!("high"));

        let body = applied(
            ReasoningStyle::Anthropic,
            Effort::Xhigh,
            json!({"thinking": {"type": "disabled"}}),
        );
        assert_eq!(body["thinking"], json!({"type": "disabled"}));
    }

    /// A Qwen budget goes out in the spelling the serving backend enforces,
    /// only on thinking levels, and only for levels that have one.
    #[test]
    fn qwen_budget_takes_the_servers_spelling() {
        let ov = ReasoningOverrides {
            budget_medium: Some(2_048),
            budget_high: Some(8_192),
            ..Default::default()
        };
        let q = |effort, budget| {
            applied_with(
                ReasoningStyle::Qwen,
                effort,
                &ov,
                budget,
                json!({"model": "Qwen3"}),
            )
        };

        let vllm = q(Effort::High, Some(ThinkingBudget::TokenBudget));
        assert_eq!(vllm["thinking_token_budget"], json!(8_192));
        assert!(vllm.get("custom_params").is_none());

        let sglang = q(Effort::High, Some(ThinkingBudget::CustomParams));
        assert_eq!(sglang["custom_params"], json!({"thinking_budget": 8_192}));
        assert!(sglang.get("thinking_token_budget").is_none());

        // A server that enforces no budget gets none: it would drop it in
        // silence and the admin's cap would do nothing.
        let generic = q(Effort::High, None);
        assert!(generic.get("thinking_token_budget").is_none());
        assert!(generic.get("custom_params").is_none());

        let off = q(Effort::Off, Some(ThinkingBudget::CustomParams));
        assert!(off.get("custom_params").is_none());

        let unset = q(Effort::Xhigh, Some(ThinkingBudget::CustomParams));
        assert!(unset.get("custom_params").is_none());
    }

    /// A client-supplied budget is never overwritten, in either spelling.
    #[test]
    fn qwen_token_budget_client_wins() {
        let ov = ReasoningOverrides {
            budget_high: Some(4_096),
            ..Default::default()
        };
        let body = applied_with(
            ReasoningStyle::Qwen,
            Effort::High,
            &ov,
            Some(ThinkingBudget::TokenBudget),
            json!({"thinking_token_budget": 99}),
        );
        assert_eq!(body["thinking_token_budget"], json!(99));

        let body = applied_with(
            ReasoningStyle::Qwen,
            Effort::High,
            &ov,
            Some(ThinkingBudget::CustomParams),
            json!({"custom_params": {"thinking_budget": 99, "other": 1}}),
        );
        assert_eq!(
            body["custom_params"],
            json!({"thinking_budget": 99, "other": 1})
        );
    }

    /// OpenAI / GLM effort-level overrides replace the built-in mapping.
    #[test]
    fn effort_level_overrides() {
        let ov = ReasoningOverrides {
            effort_medium: Some("high".into()),
            effort_high: Some("minimal".into()),
            ..Default::default()
        };
        let body = applied_with(ReasoningStyle::OpenAi, Effort::Medium, &ov, None, json!({}));
        assert_eq!(body["reasoning_effort"], json!("high"));

        let body = applied_with(ReasoningStyle::Glm, Effort::High, &ov, None, json!({}));
        assert_eq!(body["reasoning_effort"], json!("minimal"));
        assert_eq!(body["thinking"]["type"], json!("enabled"));
    }

    /// A per-model Anthropic budget overrides the built-in default, whatever
    /// the serving backend: the budget is part of Anthropic's own wire format.
    #[test]
    fn anthropic_budget_override() {
        let ov = ReasoningOverrides {
            budget_xhigh: Some(50_000),
            ..Default::default()
        };
        let body = applied_with(
            ReasoningStyle::Anthropic,
            Effort::Xhigh,
            &ov,
            None,
            json!({}),
        );
        assert_eq!(body["thinking"]["budget_tokens"], json!(50_000));
    }

    #[test]
    fn a_qwen_budget_is_enforced_only_where_the_server_enforces_it() {
        assert!(ReasoningStyle::Qwen.budget_enforced(Some(ThinkingBudget::CustomParams)));
        assert!(!ReasoningStyle::Qwen.budget_enforced(None));
        assert!(ReasoningStyle::Anthropic.budget_enforced(None));
        assert!(!ReasoningStyle::OpenAi.budget_enforced(Some(ThinkingBudget::TokenBudget)));
    }

    #[test]
    fn effort_levels_and_kind_per_style() {
        assert!(ReasoningStyle::Qwen.uses_token_budget());
        assert!(ReasoningStyle::Anthropic.uses_token_budget());
        assert!(ReasoningStyle::OpenAi.uses_effort_level());
        assert!(ReasoningStyle::Glm.uses_effort_level());
        assert_eq!(ReasoningStyle::OpenAi.effort_levels().len(), 3);
        assert_eq!(ReasoningStyle::Glm.effort_levels().len(), 7);
        assert!(ReasoningStyle::Qwen.effort_levels().is_empty());
        assert!(ReasoningStyle::None.effort_levels().is_empty());
    }
}
