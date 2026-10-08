// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Rate-limit / quota enforcement.
//!
//! The [`Enforcer`] is consulted once per user-initiated call — the `/v1`
//! proxy, the chat UI send, and the scheduler all gate on it. It resolves the
//! caller's in-force limits (`db::limits::effective_limits`, the
//! global → role → user hierarchy) and compares each against the caller's
//! recent usage in that limit's sliding window (`db::usage::usage_in_window`,
//! which counts only metered rows).
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
        self.statuses_for(usage::Subject::User(user_id), effective, Timestamp::now())
            .await
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
        self.statuses_for(usage::Subject::Token(token_id), effective, Timestamp::now())
            .await
    }

    /// Pair each in-force rule with the matching slice of `subject`'s usage.
    /// Shared by the user and token views so the two can never drift in how
    /// they read a window.
    async fn statuses_for(
        &self,
        subject: usage::Subject<'_>,
        effective: Vec<limits::EffectiveLimit>,
        now: Timestamp,
    ) -> Vec<LimitStatus> {
        // One usage read per distinct (model-scope, window); the three
        // dimensions share it.
        let mut cache: HashMap<(Option<String>, Window), WindowUsage> = HashMap::new();
        let mut out = Vec::with_capacity(effective.len());
        for lim in effective {
            let key = (lim.model.clone(), lim.window);
            let usage = match cache.get(&key) {
                Some(u) => *u,
                None => {
                    let since = lim.window.since(now);
                    let u = usage::usage_in_window(&self.db, subject, since, lim.model.as_deref())
                        .await
                        .unwrap_or_default();
                    cache.insert(key, u);
                    u
                }
            };
            let used = match lim.dimension {
                Dimension::Requests => usage.requests as f64,
                Dimension::Tokens => usage.tokens as f64,
                Dimension::Cost => usage.cost,
            };
            out.push(LimitStatus {
                model: lim.model,
                dimension: lim.dimension,
                window: lim.window,
                limit: lim.value,
                used,
                source: lim.source,
                refreshes_at: lim.window.next_refresh(now),
            });
        }
        out
    }

    /// Gate a call for one resolved model. `Ok(())` to proceed; `Err` with the
    /// first breached limit (post-hoc debt: a limit already at/over its
    /// ceiling blocks the *next* call). Unlimited callers and disabled
    /// enforcement pass instantly. Pools exempt from enforcement do
    /// not consume a budget and must remain available after it is spent.
    /// Model-scoped rules apply only when their scope names this model; an
    /// unscoped rule remains the aggregate budget across all metered models.
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
        first_breach(
            self.statuses(user_id, role_ids)
                .await
                .into_iter()
                .filter(|status| status.model.as_deref().is_none_or(|scope| scope == model))
                .collect(),
        )
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
        first_breach(
            self.token_statuses(token_id)
                .await
                .into_iter()
                .filter(|status| status.model.as_deref().is_none_or(|scope| scope == model))
                .collect(),
        )
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
                Ok(rules) => effective.extend(limits::effective_limits(&rules)),
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
