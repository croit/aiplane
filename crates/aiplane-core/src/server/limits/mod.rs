// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Rate-limit / quota enforcement.
//!
//! The [`Enforcer`] is consulted once per user-initiated call — the `/v1`
//! proxy, the chat UI send, and the scheduler all gate on it. It resolves the
//! caller's in-force limits (`db::limits::effective_limits`, the
//! global → role → user hierarchy, then most-specific-wins per dimension for
//! the called model) and compares each against the caller's recent usage in
//! that limit's sliding window (`db::usage::usage_by_model_in_window`, which
//! counts only metered rows).
//!
//! Enforcement is **post-hoc / debt-based**: a request is allowed while the
//! caller is *under* the limit; its own usage is settled afterwards, so the
//! request that crosses the line is served and the *next* one is refused. This
//! keeps the check a single cheap read and works with streaming (where the
//! token count isn't known until the end). A blocked call is a hard refusal
//! (HTTP 429 on the API; a visible error in the chat UI; a skipped, recorded
//! run for the scheduler).
//!
//! The usage read is the committed `usage_events` table, which the batched
//! writer flushes within ~500 ms — so a burst can overshoot by at most that
//! window's worth of traffic before it starts counting. That is well within
//! the bounded-overshoot the debt model already tolerates. The `Enforcer` is a
//! concrete struct today (in-process, one DB per deployment); it's the single
//! choke point to swap for a shared/atomic store if the gateway ever runs
//! multi-instance.

use std::collections::HashMap;

use jiff::Timestamp;

use super::db::Pool;
use super::db::limits::{self, Dimension, SubjectType, Window};
use super::db::model_defaults;
use super::db::usage::{self, WindowUsage};

/// Per-request limit gate. Cheap to clone (holds a pool handle + a flag).
#[derive(Clone)]
pub struct Enforcer {
    db: Pool,
    enabled: bool,
}

/// One in-force limit paired with the caller's current usage — the unit the
/// user's self-view renders as a progress bar, and what [`Enforcer::check_for_model`]
/// scans for a breach.
#[derive(Debug, Clone, PartialEq)]
pub struct LimitStatus {
    pub model: Option<String>,
    pub dimension: Dimension,
    pub window: Window,
    pub limit: f64,
    pub used: f64,
    /// Which hierarchy level supplied the winning limit (for UI labelling).
    pub source: SubjectType,
    /// A total is always an additional ceiling; overrides resolve
    /// most-specific-wins per dimension for the called model.
    pub is_total: bool,
    /// When the sliding window next advances (the next top of the hour).
    pub refreshes_at: Timestamp,
}

impl LimitStatus {
    /// Usage as a fraction of the limit, clamped to `[0, 1]` for the bar. A
    /// zero/negative limit reads as full (it can never be satisfied).
    pub fn fraction(&self) -> f64 {
        if self.limit <= 0.0 {
            return 1.0;
        }
        (self.used / self.limit).clamp(0.0, 1.0)
    }

    /// Whole-percent used, for the bar label.
    pub fn percent(&self) -> u32 {
        (self.fraction() * 100.0).round() as u32
    }

    /// True once the caller has hit or passed the limit (debt model: the next
    /// request is refused).
    pub fn exceeded(&self) -> bool {
        self.used >= self.limit
    }
}

/// A refused call: the first limit found already at/over its ceiling.
#[derive(Debug, Clone)]
pub struct LimitExceeded {
    pub model: Option<String>,
    pub dimension: Dimension,
    pub window: Window,
    pub limit: f64,
    pub used: f64,
    /// Seconds until the window next advances — served as `Retry-After`.
    pub retry_after_secs: i64,
    /// Which ceiling refused the call: `Token` for the calling token's own
    /// rule, anything else for the owner's global/role/user budget. The 429
    /// says which, because "you are over quota" and "this token is over its
    /// quota" call for different fixes.
    pub subject: SubjectType,
}

/// The human-readable "you are over a limit" sentence, shared by every wire
/// format and every side call, so a caller reads the same explanation
/// wherever the limit refused them.
impl std::fmt::Display for LimitExceeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let scope = self
            .model
            .as_deref()
            .map(|m| format!(" for model `{m}`"))
            .unwrap_or_default();
        // Say *whose* ceiling this was. "You are over quota" and "this token is
        // over its quota" have different fixes — the second is solved by using a
        // different token, or by raising that token's own rule, and a caller who
        // cannot tell them apart will chase the wrong one.
        let subject = match self.subject {
            SubjectType::Token => " for this API token",
            _ => "",
        };
        write!(
            f,
            "{} limit reached{subject}{scope}: {} per {} (used {}). Try again later.",
            self.dimension.as_str(),
            fmt_limit_num(self.limit),
            self.window.as_str(),
            fmt_limit_num(self.used),
        )
    }
}

/// Compact number for limit messages: whole values without a trailing `.0`,
/// otherwise two decimals (cost).
fn fmt_limit_num(n: f64) -> String {
    if n.fract() == 0.0 {
        format!("{}", n as i64)
    } else {
        format!("{n:.2}")
    }
}

impl Enforcer {
    pub fn new(db: Pool, enabled: bool) -> Self {
        Self { db, enabled }
    }

    /// Resolve the caller's in-force limits and pair each with current usage.
    /// Empty when enforcement is disabled or the caller has no applicable
    /// rules (the unlimited default). Fails open on a DB error — a metrics/
    /// limits read must never wedge live traffic.
    pub async fn statuses(&self, user_id: &str, role_ids: &[String]) -> Vec<LimitStatus> {
        if !self.enabled {
            return Vec::new();
        }
        let rules = match limits::applicable(&self.db, user_id, role_ids).await {
            Ok(r) => r,
            Err(err) => {
                tracing::warn!(error = %err, "limits: applicable() failed; allowing");
                return Vec::new();
            }
        };
        if rules.is_empty() {
            return Vec::new();
        }
        let effective = limits::effective_limits(&rules);
        drop_shadowed(
            self.statuses_for(usage::Subject::User(user_id), effective, Timestamp::now())
                .await,
        )
    }

    /// The rules attached to one API token, paired with that token's own
    /// usage. Independent of [`Self::statuses`]: this is the second ceiling,
    /// not another tier of the owner's hierarchy.
    pub async fn token_statuses(&self, token_id: &str) -> Vec<LimitStatus> {
        if !self.enabled || token_id.is_empty() {
            return Vec::new();
        }
        let rules = match limits::applicable_for_token(&self.db, token_id).await {
            Ok(r) => r,
            Err(err) => {
                tracing::warn!(error = %err, "limits: applicable_for_token() failed; allowing");
                return Vec::new();
            }
        };
        if rules.is_empty() {
            return Vec::new();
        }
        let effective = limits::effective_limits(&rules);
        drop_shadowed(
            self.statuses_for(usage::Subject::Token(token_id), effective, Timestamp::now())
                .await,
        )
    }

    /// Pair each in-force rule with the usage it is charged with: a total
    /// with every call its scope covers, an override only with the calls on
    /// models it is the most specific rule for, so spend under a carved-out
    /// allowance never eats the allowance it was carved out of. Shared by the
    /// user, token and agent views so they can never drift in how they read
    /// a window.
    async fn statuses_for(
        &self,
        subject: usage::Subject<'_>,
        effective: Vec<limits::EffectiveLimit>,
        now: Timestamp,
    ) -> Vec<LimitStatus> {
        let mut by_window: HashMap<Window, Vec<(String, WindowUsage)>> = HashMap::new();
        for lim in &effective {
            if by_window.contains_key(&lim.window) {
                continue;
            }
            let rows = usage::usage_by_model_in_window(&self.db, subject, lim.window.since(now))
                .await
                .unwrap_or_else(|err| {
                    tracing::warn!(error = %err, "limits: usage read failed; counting nothing");
                    Vec::new()
                });
            by_window.insert(lim.window, rows);
        }
        let mut winners: HashMap<(Dimension, &str), Option<Rank>> = HashMap::new();
        for lim in &effective {
            for (model, _) in &by_window[&lim.window] {
                winners
                    .entry((lim.dimension, model.as_str()))
                    .or_insert_with(|| {
                        winning_rank(
                            effective
                                .iter()
                                .map(|e| (e.dimension, e.source, e.model.as_deref())),
                            lim.dimension,
                            model,
                        )
                    });
            }
        }
        let mut out = Vec::with_capacity(effective.len());
        for lim in &effective {
            let rank = rank(lim.source, lim.model.as_deref());
            let used = by_window[&lim.window]
                .iter()
                .filter(|(model, _)| {
                    limits::model_matches(lim.model.as_deref(), model)
                        && (lim.is_total || winners[&(lim.dimension, model.as_str())] == Some(rank))
                })
                .map(|(_, usage)| spent(usage, lim.dimension))
                .sum();
            out.push(LimitStatus {
                model: lim.model.clone(),
                dimension: lim.dimension,
                window: lim.window,
                limit: lim.value,
                used,
                source: lim.source,
                is_total: lim.is_total,
                refreshes_at: lim.window.next_refresh(now),
            });
        }
        out
    }

    /// Whether a call on `model` can cost anything. A model without a price
    /// above zero settles every call to 0, so a cost limit has nothing to
    /// guard there. Unknown on a DB error, which reads as charging: the
    /// cost limits then apply as before.
    async fn charges(&self, model: &str) -> bool {
        match model_defaults::get(&self.db, model).await {
            Ok(defaults) => {
                defaults.is_some_and(|d| model_defaults::charges(d.input_price, d.output_price))
            }
            Err(err) => {
                tracing::warn!(error = %err, "limits: price read failed; treating the model as charged");
                true
            }
        }
    }

    /// The statuses that gate a call on `model`, cost limits dropped for a
    /// model that cannot cost anything.
    async fn gate(&self, statuses: Vec<LimitStatus>, model: &str) -> Result<(), LimitExceeded> {
        let mut in_force = select_in_force(statuses, model);
        if in_force.iter().any(|s| s.dimension == Dimension::Cost) && !self.charges(model).await {
            in_force.retain(|s| s.dimension != Dimension::Cost);
        }
        first_breach(in_force)
    }

    /// Gate a call for one resolved model. `Ok(())` to proceed; `Err` with the
    /// first breached limit (post-hoc debt: a limit already at/over its
    /// ceiling blocks the *next* call). Unlimited callers and disabled
    /// enforcement pass instantly. Pools exempt from enforcement do
    /// not consume a budget and must remain available after it is spent.
    /// Rules resolve most-specific-wins per dimension (see
    /// [`select_in_force`]); totals are checked additionally. A model that
    /// cannot cost anything is never refused by a cost limit.
    pub async fn check_for_model(
        &self,
        user_id: &str,
        role_ids: &[String],
        model: &str,
        enforce_limits: bool,
    ) -> Result<(), LimitExceeded> {
        if !enforce_limits {
            return Ok(());
        }
        self.gate(self.statuses(user_id, role_ids).await, model)
            .await
    }

    /// Gate a call against the calling token's own rules — the *additional*
    /// ceiling. A caller under their personal budget can still be refused
    /// here, and a token rule can never grant more than the owner's budget
    /// allows, because both gates must pass. No rules on the token (the
    /// default for every token ever issued) passes instantly. The token-owned
    /// counterpart of [`Self::check_for_model`].
    pub async fn check_token_for_model(
        &self,
        token_id: &str,
        model: &str,
        enforce_limits: bool,
    ) -> Result<(), LimitExceeded> {
        if !enforce_limits {
            return Ok(());
        }
        self.gate(self.token_statuses(token_id).await, model).await
    }

    /// What one agent's conversations have spent against each ceiling on it:
    /// `owner_budget`, the limits from the agent's own spec, and any
    /// operator rule with subject `system` on the agent. Every one is a
    /// ceiling of its own, so the tightest decides; none widens another.
    ///
    /// The owner's budget is part of the agent's definition and applies even
    /// when `[limits] enabled` is off, like its idle TTL; that switch governs
    /// the operator's rules. Both read `usage_events.agent_id`, so with usage
    /// metrics off nothing is ever spent.
    pub async fn agent_statuses(
        &self,
        agent_id: &str,
        owner_budget: &[limits::EffectiveLimit],
        now: Timestamp,
    ) -> Vec<LimitStatus> {
        let mut effective = owner_budget.to_vec();
        if self.enabled {
            match limits::applicable_for_agent(&self.db, agent_id).await {
                // Every ceiling on an agent caps on its own, so each counts
                // its whole scope, whatever kind the operator saved it as.
                Ok(rules) => {
                    effective.extend(limits::effective_limits(&rules).into_iter().map(|lim| {
                        limits::EffectiveLimit {
                            is_total: true,
                            ..lim
                        }
                    }))
                }
                Err(err) => {
                    tracing::warn!(error = %err, "limits: applicable_for_agent() failed; ignoring")
                }
            }
        }
        if effective.is_empty() {
            return Vec::new();
        }
        self.statuses_for(usage::Subject::Agent(agent_id), effective, now)
            .await
    }

    /// Gate one visitor message to an agent on its budgets (debt model, like
    /// every other check here: the message that crosses the line is served).
    /// `now` is a parameter so a test can stand at a window's edge.
    pub async fn check_agent(
        &self,
        agent_id: &str,
        owner_budget: &[limits::EffectiveLimit],
        now: Timestamp,
    ) -> Result<(), LimitExceeded> {
        first_breach_at(self.agent_statuses(agent_id, owner_budget, now).await, now)
    }
}

/// The rules gating a call on `model`. Per dimension the most specific
/// matching rule wins — the highest subject (own rule, then role, then
/// global), then the most specific model scope (exact, then the longest
/// `prefix-*`, then all models) — together with every window set at that
/// same level: the window is not part of specificity, so `alice 75/day`
/// replaces `everybody 25/week` outright. Totals always stay on top, each
/// a ceiling of its own, and read first, so a refusal names the overall
/// ceiling before a carved-out allowance.
fn select_in_force(statuses: Vec<LimitStatus>, model: &str) -> Vec<LimitStatus> {
    let matching: Vec<LimitStatus> = statuses
        .into_iter()
        .filter(|s| limits::model_matches(s.model.as_deref(), model))
        .collect();
    let winner = |dimension| {
        winning_rank(
            matching
                .iter()
                .map(|s| (s.dimension, s.source, s.model.as_deref())),
            dimension,
            model,
        )
    };
    let keep: Vec<bool> = matching
        .iter()
        .map(|s| s.is_total || winner(s.dimension) == Some(rank(s.source, s.model.as_deref())))
        .collect();
    let (mut totals, overrides): (Vec<_>, Vec<_>) = matching
        .into_iter()
        .zip(keep)
        .filter_map(|(s, keep)| keep.then_some(s))
        .partition(|s| s.is_total);
    totals.extend(overrides);
    totals
}

/// Without the overrides that can never be in force: one is shadowed when
/// a more specific rule in its dimension covers every model it covers, as
/// `alice 75/day` does `everybody 25/week`. What is left is what the caller
/// can actually run into.
fn drop_shadowed(statuses: Vec<LimitStatus>) -> Vec<LimitStatus> {
    let shadowed = |s: &LimitStatus| {
        !s.is_total
            && statuses.iter().any(|other| {
                other.dimension == s.dimension
                    && rank(other.source, other.model.as_deref())
                        > rank(s.source, s.model.as_deref())
                    && limits::scope_covers(other.model.as_deref(), s.model.as_deref())
            })
    };
    let keep: Vec<bool> = statuses.iter().map(|s| !shadowed(s)).collect();
    statuses
        .into_iter()
        .zip(keep)
        .filter_map(|(s, keep)| keep.then_some(s))
        .collect()
}

/// How specific a rule is: subject first, then model scope.
type Rank = (u8, (u8, usize));

fn rank(source: SubjectType, scope: Option<&str>) -> Rank {
    (subject_rank(source), limits::scope_specificity(scope))
}

/// The rank of the most specific rule in `dimension` that covers `model`.
fn winning_rank<'a>(
    rules: impl Iterator<Item = (Dimension, SubjectType, Option<&'a str>)>,
    dimension: Dimension,
    model: &str,
) -> Option<Rank> {
    rules
        .filter(|(d, _, scope)| *d == dimension && limits::model_matches(*scope, model))
        .map(|(_, source, scope)| rank(source, scope))
        .max()
}

fn spent(usage: &WindowUsage, dimension: Dimension) -> f64 {
    match dimension {
        Dimension::Requests => usage.requests as f64,
        Dimension::Tokens => usage.tokens as f64,
        Dimension::Cost => usage.cost,
    }
}

/// Subject precedence for most-specific-wins, mirroring the hierarchy in
/// `db::limits::effective_limits`: your own rule beats a role's, which beats
/// the default.
fn subject_rank(s: SubjectType) -> u8 {
    match s {
        SubjectType::Token | SubjectType::System | SubjectType::AgentSpec => 3,
        SubjectType::User => 2,
        SubjectType::Role => 1,
        SubjectType::Global => 0,
    }
}

/// The first status already at/over its ceiling, as the refusal to send.
fn first_breach(statuses: Vec<LimitStatus>) -> Result<(), LimitExceeded> {
    first_breach_at(statuses, Timestamp::now())
}

fn first_breach_at(statuses: Vec<LimitStatus>, now: Timestamp) -> Result<(), LimitExceeded> {
    for s in statuses {
        if s.exceeded() {
            let retry = (s.refreshes_at.as_second() - now.as_second()).max(1);
            return Err(LimitExceeded {
                model: s.model,
                dimension: s.dimension,
                window: s.window,
                limit: s.limit,
                used: s.used,
                retry_after_secs: retry,
                subject: s.source,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::db::usage::{UsageKind, UsageRecord, UsageSource};

    async fn pool() -> Pool {
        crate::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap()
    }

    fn event(
        user: &str,
        model: &str,
        total: i64,
        enforce_limits: bool,
        at: Timestamp,
    ) -> UsageRecord {
        UsageRecord {
            created_at: at,
            user_id: user.into(),
            user_email: None,
            token_id: None,
            token_name: None,
            source: UsageSource::V1Api,
            kind: UsageKind::Chat,
            backend: "b".into(),
            model: model.into(),
            status: 200,
            duration_ms: 1,
            prompt_tokens: Some(total),
            completion_tokens: Some(0),
            total_tokens: Some(total),
            cached_tokens: None,
            input_units: None,
            output_units: None,
            enforce_limits,
            principal_kind: crate::server::principal::PrincipalKind::User,
            agent_id: None,
            chain: None,
            stop_reason: None,
        }
    }

    #[tokio::test]
    async fn no_rules_means_unlimited() {
        let pool = pool().await;
        let enf = Enforcer::new(pool, true);
        assert!(enf.check_for_model("alice", &[], "gpt", true).await.is_ok());
        assert!(enf.statuses("alice", &[]).await.is_empty());
    }

    #[tokio::test]
    async fn disabled_enforcer_never_blocks() {
        let pool = pool().await;
        limits::upsert(
            &pool,
            SubjectType::Global,
            "",
            None,
            Dimension::Requests,
            Window::Hour,
            0.0,
        )
        .await
        .unwrap();
        let enf = Enforcer::new(pool, false);
        assert!(enf.check_for_model("alice", &[], "gpt", true).await.is_ok());
    }

    /// An event attributed to one API token.
    fn token_event(user: &str, token: &str, total: i64, at: Timestamp) -> UsageRecord {
        UsageRecord {
            token_id: Some(token.to_string()),
            token_name: Some(token.to_string()),
            ..event(user, "gpt", total, true, at)
        }
    }

    /// The default: a token with no rules of its own is never blocked by the
    /// token gate. Every token issued before per-token quotas existed is in
    /// this state, so a mistake here would 429 the entire installed base.
    #[tokio::test]
    async fn a_token_without_rules_is_never_blocked() {
        let pool = pool().await;
        let enf = Enforcer::new(pool, true);
        assert!(enf.check_token_for_model("tok-a", "m", true).await.is_ok());
        assert!(
            enf.check_token_for_model("", "m", true).await.is_ok(),
            "no token id at all"
        );
        assert!(enf.token_statuses("tok-a").await.is_empty());
    }

    /// A token rule counts only that token's own traffic. If it read the
    /// owner's usage instead, a busy second token would exhaust a quiet
    /// token's quota.
    #[tokio::test]
    async fn a_token_quota_measures_only_that_tokens_usage() {
        let pool = pool().await;
        limits::upsert(
            &pool,
            SubjectType::Token,
            "tok-a",
            None,
            Dimension::Requests,
            Window::Hour,
            2.0,
        )
        .await
        .unwrap();
        let enf = Enforcer::new(pool.clone(), true);
        let now = Timestamp::now();

        // Three requests on a *different* token must not touch tok-a's quota.
        usage::insert_batch(
            &pool,
            &[
                token_event("alice", "tok-b", 1, now),
                token_event("alice", "tok-b", 1, now),
                token_event("alice", "tok-b", 1, now),
            ],
        )
        .await
        .unwrap();
        assert!(enf.check_token_for_model("tok-a", "m", true).await.is_ok());

        // Its own two requests do.
        usage::insert_batch(
            &pool,
            &[
                token_event("alice", "tok-a", 1, now),
                token_event("alice", "tok-a", 1, now),
            ],
        )
        .await
        .unwrap();
        let err = enf
            .check_token_for_model("tok-a", "m", true)
            .await
            .unwrap_err();
        assert_eq!(err.limit, 2.0);
        assert_eq!(
            err.subject,
            SubjectType::Token,
            "the 429 has to say which ceiling tripped"
        );
        // …and tok-b, which has no rule, still passes.
        assert!(enf.check_token_for_model("tok-b", "m", true).await.is_ok());
    }

    /// A token rule is an *additional* ceiling, never a replacement. The
    /// failure this rules out: treating `token` as another tier of the
    /// hierarchy, where a generous token rule would override — and so widen —
    /// its owner's budget. Minting a token must not be a way out of a quota.
    #[tokio::test]
    async fn a_generous_token_rule_cannot_widen_the_owners_budget() {
        let pool = pool().await;
        // The user may make 1 request/hour; the token says 100.
        limits::upsert(
            &pool,
            SubjectType::User,
            "alice",
            None,
            Dimension::Requests,
            Window::Hour,
            1.0,
        )
        .await
        .unwrap();
        limits::upsert(
            &pool,
            SubjectType::Token,
            "tok-a",
            None,
            Dimension::Requests,
            Window::Hour,
            100.0,
        )
        .await
        .unwrap();
        let enf = Enforcer::new(pool.clone(), true);
        let now = Timestamp::now();
        usage::insert_batch(&pool, &[token_event("alice", "tok-a", 1, now)])
            .await
            .unwrap();

        // The token's own gate is happy (1 of 100)…
        assert!(enf.check_token_for_model("tok-a", "m", true).await.is_ok());
        // …but the owner's budget is spent, and that gate is checked too.
        let err = enf
            .check_for_model("alice", &[], "gpt", true)
            .await
            .unwrap_err();
        assert_eq!(err.limit, 1.0);
        assert_eq!(err.subject, SubjectType::User);
    }

    /// The user rule must not leak into the token's own resolution either:
    /// `applicable_for_token` returns token rules only.
    #[tokio::test]
    async fn token_statuses_ignore_the_owners_rules() {
        let pool = pool().await;
        limits::upsert(
            &pool,
            SubjectType::Global,
            "",
            None,
            Dimension::Requests,
            Window::Hour,
            5.0,
        )
        .await
        .unwrap();
        let enf = Enforcer::new(pool, true);
        assert!(
            enf.token_statuses("tok-a").await.is_empty(),
            "a global rule is the owner's budget, not the token's"
        );
    }

    #[tokio::test]
    async fn blocks_once_usage_reaches_the_limit() {
        let pool = pool().await;
        // 2 requests / hour, global.
        limits::upsert(
            &pool,
            SubjectType::Global,
            "",
            None,
            Dimension::Requests,
            Window::Hour,
            2.0,
        )
        .await
        .unwrap();
        let enf = Enforcer::new(pool.clone(), true);
        let now = Timestamp::now();

        // No usage yet → allowed.
        assert!(enf.check_for_model("alice", &[], "gpt", true).await.is_ok());

        // Two metered requests recorded → at the ceiling → next is refused.
        usage::insert_batch(
            &pool,
            &[
                event("alice", "gpt", 1, true, now),
                event("alice", "gpt", 1, true, now),
            ],
        )
        .await
        .unwrap();
        let err = enf
            .check_for_model("alice", &[], "gpt", true)
            .await
            .unwrap_err();
        assert_eq!(err.dimension, Dimension::Requests);
        assert_eq!(err.limit, 2.0);
        assert!(err.used >= 2.0);
        assert!(err.retry_after_secs >= 1);
    }

    #[tokio::test]
    async fn usage_outside_the_window_ages_out_and_resets() {
        let pool = pool().await;
        // 2 requests / hour, global.
        limits::upsert(
            &pool,
            SubjectType::Global,
            "",
            None,
            Dimension::Requests,
            Window::Hour,
            2.0,
        )
        .await
        .unwrap();
        let enf = Enforcer::new(pool.clone(), true);
        let now = Timestamp::now();
        let three_hours_ago = now
            .checked_sub(jiff::SignedDuration::from_hours(3))
            .unwrap();

        // Three requests, but all from 3h ago — outside the 1-hour sliding
        // window, so they've aged out and don't count. The window has, in
        // effect, reset.
        usage::insert_batch(
            &pool,
            &[
                event("alice", "gpt", 1, true, three_hours_ago),
                event("alice", "gpt", 1, true, three_hours_ago),
                event("alice", "gpt", 1, true, three_hours_ago),
            ],
        )
        .await
        .unwrap();
        assert!(
            enf.check_for_model("alice", &[], "gpt", true).await.is_ok(),
            "usage older than the window must not count (the window resets)"
        );
        let status = enf.statuses("alice", &[]).await;
        assert_eq!(status.len(), 1);
        assert_eq!(
            status[0].used, 0.0,
            "aged-out usage reads as 0 in the window"
        );

        // Fresh usage inside the window does count, and re-blocks.
        usage::insert_batch(
            &pool,
            &[
                event("alice", "gpt", 1, true, now),
                event("alice", "gpt", 1, true, now),
            ],
        )
        .await
        .unwrap();
        assert!(
            enf.check_for_model("alice", &[], "gpt", true)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn exempt_usage_does_not_count() {
        let pool = pool().await;
        limits::upsert(
            &pool,
            SubjectType::User,
            "alice",
            None,
            Dimension::Tokens,
            Window::Day,
            100.0,
        )
        .await
        .unwrap();
        let enf = Enforcer::new(pool.clone(), true);
        let now = Timestamp::now();
        // 500 tokens but on an EXEMPT (metered=false) pool → ignored.
        usage::insert_batch(&pool, &[event("alice", "local", 500, false, now)])
            .await
            .unwrap();
        assert!(enf.check_for_model("alice", &[], "gpt", true).await.is_ok());
        let st = enf.statuses("alice", &[]).await;
        assert_eq!(st.len(), 1);
        assert_eq!(st[0].used, 0.0);
    }

    #[tokio::test]
    async fn exempt_models_are_allowed_after_a_budget_is_spent() {
        let pool = pool().await;
        limits::upsert(
            &pool,
            SubjectType::User,
            "alice",
            None,
            Dimension::Requests,
            Window::Day,
            1.0,
        )
        .await
        .unwrap();
        let now = Timestamp::now();
        let charged = event("alice", "paid", 1, true, now);
        usage::insert_batch(&pool, &[charged]).await.unwrap();

        let enf = Enforcer::new(pool, true);
        assert!(
            enf.check_for_model("alice", &[], "free", false)
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn per_model_scope_counts_only_that_model() {
        let pool = pool().await;
        limits::upsert(
            &pool,
            SubjectType::User,
            "alice",
            Some("pricey"),
            Dimension::Tokens,
            Window::Day,
            100.0,
        )
        .await
        .unwrap();
        let enf = Enforcer::new(pool.clone(), true);
        let now = Timestamp::now();
        usage::insert_batch(
            &pool,
            &[
                event("alice", "cheap", 999, true, now),
                event("alice", "pricey", 100, true, now),
            ],
        )
        .await
        .unwrap();
        assert!(
            enf.check_for_model("alice", &[], "cheap", true)
                .await
                .is_ok()
        );
        // Only "pricey" usage (100) counts against the pricey-scoped limit → at ceiling.
        assert!(
            enf.check_for_model("alice", &[], "pricey", true)
                .await
                .is_err()
        );
    }

    fn at(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    /// An override helper: an admin rule that takes part in
    /// most-specific-wins rather than capping additionally.
    async fn upsert_override(
        pool: &Pool,
        subject: limits::SubjectType,
        id: &str,
        model: Option<&str>,
        dim: limits::Dimension,
        win: limits::Window,
        value: f64,
    ) {
        limits::upsert_checked(
            pool,
            subject,
            id,
            model,
            dim,
            win,
            value,
            limits::ManagedBy::Admin,
            false,
        )
        .await
        .unwrap();
    }

    /// Most-specific-wins: a model-scoped override replaces the aggregate in
    /// its dimension — the aggregate no longer caps that model.
    #[tokio::test]
    async fn a_model_override_replaces_the_aggregate_for_that_model() {
        let pool = pool().await;
        upsert_override(
            &pool,
            limits::SubjectType::User,
            "alice",
            None,
            Dimension::Tokens,
            Window::Month,
            25.0,
        )
        .await;
        upsert_override(
            &pool,
            limits::SubjectType::User,
            "alice",
            Some("pricey"),
            Dimension::Tokens,
            Window::Month,
            250.0,
        )
        .await;
        let enf = Enforcer::new(pool.clone(), true);
        let now = Timestamp::now();
        usage::insert_batch(&pool, &[event("alice", "pricey", 100, true, now)])
            .await
            .unwrap();

        // 100 tokens on pricey: under the specific 250 → allowed, although
        // they are more than the aggregate's 25.
        assert!(
            enf.check_for_model("alice", &[], "pricey", true)
                .await
                .is_ok()
        );
        // The carved-out spend is not the aggregate's: other models still
        // have their whole 25.
        assert!(
            enf.check_for_model("alice", &[], "cheap", true)
                .await
                .is_ok()
        );
        usage::insert_batch(&pool, &[event("alice", "cheap", 25, true, now)])
            .await
            .unwrap();
        assert!(
            enf.check_for_model("alice", &[], "cheap", true)
                .await
                .is_err()
        );
        assert!(
            enf.check_for_model("alice", &[], "pricey", true)
                .await
                .is_ok(),
            "spend on other models does not eat the model's own allowance"
        );
    }

    async fn upsert_total(
        pool: &Pool,
        subject: limits::SubjectType,
        id: &str,
        model: Option<&str>,
        value: f64,
    ) {
        limits::upsert_checked(
            pool,
            subject,
            id,
            model,
            Dimension::Tokens,
            Window::Month,
            value,
            limits::ManagedBy::Admin,
            true,
        )
        .await
        .unwrap();
    }

    async fn allowed(enf: &Enforcer, user: &str, model: &str) -> bool {
        enf.check_for_model(user, &[], model, true).await.is_ok()
    }

    /// Everybody 25, alice 75, alice on `claude-*` 250: each layer replaces
    /// the one beneath it for the calls it covers, and spends only from its
    /// own allowance.
    #[tokio::test]
    async fn overrides_nest_from_everybody_to_a_user_to_a_users_model_family() {
        let pool = pool().await;
        let tokens_month = |subject, id: &'static str, model, value| {
            let pool = pool.clone();
            async move {
                upsert_override(
                    &pool,
                    subject,
                    id,
                    model,
                    Dimension::Tokens,
                    Window::Month,
                    value,
                )
                .await
            }
        };
        tokens_month(limits::SubjectType::Global, "", None, 25.0).await;
        tokens_month(limits::SubjectType::User, "alice", None, 75.0).await;
        tokens_month(limits::SubjectType::User, "alice", Some("claude-*"), 250.0).await;
        let enf = Enforcer::new(pool.clone(), true);
        let now = Timestamp::now();

        usage::insert_batch(&pool, &[event("bob", "gpt", 25, true, now)])
            .await
            .unwrap();
        assert!(!allowed(&enf, "bob", "gpt").await, "bob has everybody's 25");
        assert!(!allowed(&enf, "bob", "claude-x").await);

        usage::insert_batch(&pool, &[event("alice", "claude-x", 200, true, now)])
            .await
            .unwrap();
        assert!(allowed(&enf, "alice", "claude-x").await, "200 of 250");
        assert!(
            allowed(&enf, "alice", "gpt").await,
            "claude spend is not the 75's"
        );

        usage::insert_batch(&pool, &[event("alice", "gpt", 75, true, now)])
            .await
            .unwrap();
        assert!(!allowed(&enf, "alice", "gpt").await, "75 of 75");
        assert!(
            allowed(&enf, "alice", "claude-y").await,
            "the family keeps its 250"
        );

        usage::insert_batch(&pool, &[event("alice", "claude-y", 50, true, now)])
            .await
            .unwrap();
        assert!(
            !allowed(&enf, "alice", "claude-x").await,
            "250 of 250 across the family"
        );
    }

    /// A 100 budget with 75 for each of two models: each model stops at its
    /// 75, and together they never pass the 100.
    #[tokio::test]
    async fn model_allowances_inside_a_total_never_pass_the_total() {
        let pool = pool().await;
        upsert_total(&pool, limits::SubjectType::User, "alice", None, 100.0).await;
        for model in ["claude1", "claude2"] {
            upsert_override(
                &pool,
                limits::SubjectType::User,
                "alice",
                Some(model),
                Dimension::Tokens,
                Window::Month,
                75.0,
            )
            .await;
        }
        upsert_override(
            &pool,
            limits::SubjectType::Global,
            "",
            None,
            Dimension::Tokens,
            Window::Month,
            25.0,
        )
        .await;
        let enf = Enforcer::new(pool.clone(), true);
        let now = Timestamp::now();

        usage::insert_batch(&pool, &[event("alice", "claude1", 75, true, now)])
            .await
            .unwrap();
        assert!(
            !allowed(&enf, "alice", "claude1").await,
            "75 of claude1's 75"
        );
        assert!(allowed(&enf, "alice", "claude2").await);
        assert!(
            allowed(&enf, "alice", "gpt").await,
            "alice's own 100 outranks everybody's 25"
        );

        usage::insert_batch(&pool, &[event("alice", "claude2", 25, true, now)])
            .await
            .unwrap();
        let err = enf
            .check_for_model("alice", &[], "claude2", true)
            .await
            .unwrap_err();
        assert_eq!(err.limit, 100.0, "25 of claude2's 75, but 100 of the 100");
        assert!(!allowed(&enf, "alice", "gpt").await);
    }

    /// A spent cost budget refuses priced models only: a model without a
    /// price above zero cannot move it, so it stays usable. Request and
    /// token limits still hold there.
    #[tokio::test]
    async fn a_spent_cost_budget_never_refuses_a_free_model() {
        let pool = pool().await;
        model_defaults::set_pricing(&pool, "paid", Some(1_000_000.0), Some(0.0), None)
            .await
            .unwrap();
        model_defaults::set_pricing(&pool, "zero", Some(0.0), Some(0.0), None)
            .await
            .unwrap();
        limits::upsert(
            &pool,
            SubjectType::User,
            "alice",
            None,
            Dimension::Cost,
            Window::Day,
            1.0,
        )
        .await
        .unwrap();
        let enf = Enforcer::new(pool.clone(), true);
        usage::insert_batch(&pool, &[event("alice", "paid", 1, true, Timestamp::now())])
            .await
            .unwrap();
        assert!(!allowed(&enf, "alice", "paid").await, "1 USD of 1");
        assert!(allowed(&enf, "alice", "zero").await, "priced at 0");
        assert!(allowed(&enf, "alice", "unpriced").await, "never priced");

        limits::upsert(
            &pool,
            SubjectType::User,
            "alice",
            None,
            Dimension::Requests,
            Window::Day,
            1.0,
        )
        .await
        .unwrap();
        assert!(
            !allowed(&enf, "alice", "zero").await,
            "a request limit still counts free calls"
        );
    }

    /// The window is not part of specificity: alice's own daily rule
    /// replaces everybody's weekly one for her.
    #[tokio::test]
    async fn a_more_specific_rule_replaces_every_window_of_a_less_specific_one() {
        let pool = pool().await;
        upsert_override(
            &pool,
            limits::SubjectType::Global,
            "",
            None,
            Dimension::Tokens,
            Window::Week,
            25.0,
        )
        .await;
        upsert_override(
            &pool,
            limits::SubjectType::User,
            "alice",
            None,
            Dimension::Tokens,
            Window::Day,
            75.0,
        )
        .await;
        let enf = Enforcer::new(pool.clone(), true);
        usage::insert_batch(&pool, &[event("alice", "gpt", 50, true, Timestamp::now())])
            .await
            .unwrap();
        assert!(allowed(&enf, "alice", "gpt").await, "50 of alice's 75/day");
        let shown: Vec<f64> = enf
            .statuses("alice", &[])
            .await
            .iter()
            .map(|s| s.limit)
            .collect();
        assert_eq!(shown, [75.0], "everybody's weekly rule is not hers to see");
        usage::insert_batch(&pool, &[event("bob", "gpt", 25, true, Timestamp::now())])
            .await
            .unwrap();
        assert!(
            !allowed(&enf, "bob", "gpt").await,
            "25 of everybody's 25/week"
        );
    }

    /// A total is also the most specific rule for what it covers: alice's
    /// own family budget beats everybody's aggregate for her claude calls.
    #[tokio::test]
    async fn a_total_takes_part_in_most_specific_wins() {
        let pool = pool().await;
        upsert_total(
            &pool,
            limits::SubjectType::User,
            "alice",
            Some("claude-*"),
            100.0,
        )
        .await;
        upsert_override(
            &pool,
            limits::SubjectType::Global,
            "",
            None,
            Dimension::Tokens,
            Window::Month,
            25.0,
        )
        .await;
        let enf = Enforcer::new(pool.clone(), true);
        usage::insert_batch(
            &pool,
            &[event("alice", "claude-x", 50, true, Timestamp::now())],
        )
        .await
        .unwrap();
        assert!(allowed(&enf, "alice", "claude-x").await);
        assert!(
            allowed(&enf, "alice", "gpt").await,
            "claude spend is the family's"
        );
    }

    /// A total keeps the old behaviour: it caps additionally, so the
    /// specific allowance cannot exceed it.
    #[tokio::test]
    async fn a_total_aggregate_still_caps_the_specific_rule() {
        let pool = pool().await;
        // Aggregate as a real overall ceiling (is_total = true).
        limits::upsert(
            &pool,
            limits::SubjectType::User,
            "alice",
            None,
            Dimension::Tokens,
            Window::Month,
            25.0,
        )
        .await
        .unwrap();
        upsert_override(
            &pool,
            limits::SubjectType::User,
            "alice",
            Some("pricey"),
            Dimension::Tokens,
            Window::Month,
            250.0,
        )
        .await;
        let enf = Enforcer::new(pool.clone(), true);
        let now = Timestamp::now();
        usage::insert_batch(&pool, &[event("alice", "pricey", 100, true, now)])
            .await
            .unwrap();

        let err = enf
            .check_for_model("alice", &[], "pricey", true)
            .await
            .unwrap_err();
        assert_eq!(err.limit, 25.0);
    }

    /// A `prefix-*` scope counts every matching model, and an exact scope
    /// outranks it for its own model.
    #[tokio::test]
    async fn a_prefix_scope_covers_its_family_and_loses_to_exact() {
        let pool = pool().await;
        upsert_override(
            &pool,
            limits::SubjectType::User,
            "alice",
            Some("claude-*"),
            Dimension::Tokens,
            Window::Month,
            500.0,
        )
        .await;
        upsert_override(
            &pool,
            limits::SubjectType::User,
            "alice",
            Some("claude-sonnet-5-5"),
            Dimension::Tokens,
            Window::Month,
            50.0,
        )
        .await;
        let enf = Enforcer::new(pool.clone(), true);
        let now = Timestamp::now();
        usage::insert_batch(
            &pool,
            &[
                event("alice", "claude-sonnet-5-5", 60, true, now),
                event("alice", "claude-haiku-5-5", 60, true, now),
            ],
        )
        .await
        .unwrap();

        // Sonnet spent 60 of its exact 50 → blocked (exact wins over prefix).
        assert!(
            enf.check_for_model("alice", &[], "claude-sonnet-5-5", true)
                .await
                .is_err()
        );
        // Haiku spent 60 of the family's 500 → allowed; unrelated models are
        // untouched by the family scope.
        assert!(
            enf.check_for_model("alice", &[], "claude-haiku-5-5", true)
                .await
                .is_ok()
        );
        assert!(
            enf.check_for_model("alice", &[], "gpt", true).await.is_ok(),
            "no aggregate rule exists, so other models are unlimited"
        );
    }

    /// Subject specificity outranks model specificity: your own aggregate
    /// beats a global rule scoped to the model.
    #[tokio::test]
    async fn a_users_own_aggregate_beats_a_global_model_rule() {
        let pool = pool().await;
        upsert_override(
            &pool,
            limits::SubjectType::Global,
            "",
            Some("pricey"),
            Dimension::Tokens,
            Window::Month,
            10.0,
        )
        .await;
        upsert_override(
            &pool,
            limits::SubjectType::User,
            "alice",
            None,
            Dimension::Tokens,
            Window::Month,
            1_000.0,
        )
        .await;
        let enf = Enforcer::new(pool.clone(), true);
        let now = Timestamp::now();
        usage::insert_batch(&pool, &[event("alice", "pricey", 100, true, now)])
            .await
            .unwrap();

        // Alice's own aggregate (1000) wins over the global model rule (10).
        assert!(
            enf.check_for_model("alice", &[], "pricey", true)
                .await
                .is_ok()
        );
    }

    fn agent_event(principal: &str, agent: &str, tokens: i64, at: Timestamp) -> UsageRecord {
        UsageRecord {
            principal_kind: crate::server::principal::PrincipalKind::System,
            agent_id: Some(agent.into()),
            ..event(principal, "gpt", tokens, true, at)
        }
    }

    fn monthly_tokens(value: f64) -> limits::EffectiveLimit {
        limits::EffectiveLimit {
            model: None,
            dimension: Dimension::Tokens,
            window: Window::Month,
            value,
            source: SubjectType::AgentSpec,
            is_total: true,
        }
    }

    /// The budget is the conversation's, not the principal's: a sub-agent
    /// call names the sub-agent in `user_id` and still spends the main
    /// agent's budget, while the sub-agent's own conversations do not.
    #[tokio::test]
    async fn an_agents_budget_counts_its_sub_agents_but_not_their_own_conversations() {
        let pool = pool().await;
        let now = at("2026-10-01T12:30:00Z");
        let earlier = at("2026-10-01T10:00:00Z");
        usage::insert_batch(
            &pool,
            &[
                agent_event("support", "support", 40, earlier),
                agent_event("billing", "support", 50, earlier),
                agent_event("billing", "billing", 500, earlier),
            ],
        )
        .await
        .unwrap();
        let enf = Enforcer::new(pool, true);
        let budget = [monthly_tokens(100.0)];
        assert!(enf.check_agent("support", &budget, now).await.is_ok());
        let status = enf.agent_statuses("support", &budget, now).await;
        assert_eq!(status[0].used, 90.0);

        let err = enf
            .check_agent("support", &[monthly_tokens(90.0)], now)
            .await
            .unwrap_err();
        assert_eq!(err.subject, SubjectType::AgentSpec);
        assert_eq!(err.window, Window::Month);
        assert!(err.retry_after_secs >= 1);
    }

    /// An operator's `system` rule narrows the owner's budget; it cannot be
    /// lifted by a generous owner, and `[limits] enabled = false` switches
    /// only the operator's rules off, never the owner's own budget.
    #[tokio::test]
    async fn an_operator_rule_on_an_agent_is_one_more_ceiling() {
        let pool = pool().await;
        let now = at("2026-10-01T12:30:00Z");
        usage::insert_batch(
            &pool,
            &[agent_event(
                "support",
                "support",
                60,
                at("2026-10-01T11:00:00Z"),
            )],
        )
        .await
        .unwrap();
        limits::upsert(
            &pool,
            SubjectType::System,
            "support",
            None,
            Dimension::Tokens,
            Window::Month,
            50.0,
        )
        .await
        .unwrap();
        let generous = [monthly_tokens(1_000_000.0)];
        let on = Enforcer::new(pool.clone(), true);
        let err = on.check_agent("support", &generous, now).await.unwrap_err();
        assert_eq!(err.limit, 50.0);

        let off = Enforcer::new(pool, false);
        assert!(off.check_agent("support", &generous, now).await.is_ok());
        assert!(
            off.check_agent("support", &[monthly_tokens(60.0)], now)
                .await
                .is_err(),
            "the owner's budget holds with enforcement switched off"
        );
        assert!(
            on.check_agent("support", &[], now).await.is_err(),
            "an agent without a budget of its own is still capped by the operator"
        );
    }

    #[tokio::test]
    async fn an_agent_without_any_budget_is_unlimited() {
        let pool = pool().await;
        let enf = Enforcer::new(pool, true);
        assert!(
            enf.agent_statuses("support", &[], Timestamp::now())
                .await
                .is_empty()
        );
        assert!(
            enf.check_agent("support", &[], Timestamp::now())
                .await
                .is_ok()
        );
    }
}
