// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! What one run may spend: tool rounds, wall-clock seconds, and tokens.
//!
//! Every run carries its own [`Budget`]. A chat turn derives one from the
//! conversation's effort level ([`Budget::from_effort`]), so interactive
//! behaviour is the round cap it always had; a headless run may pass an
//! explicit one. Running out of *any* limit takes the same path as running out
//! of rounds: the next request is the final round, with tools withheld (or only
//! `finish` offered, under a [`FinishContract`](crate::finish::FinishContract)).
//! Delegated runs have local limits and share the root's aggregate meter.
//!
//! Limits are checked **between rounds**, never mid-stream: cutting an
//! upstream reply off would throw away tokens already paid for and leave a
//! half-written message, while a round is bounded by the model's own output
//! ceiling. A run can therefore overshoot `seconds` or `tokens` by at most one
//! round. See docs/tools-rbac.md → "Run budgets".

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use aiplane_core::server::reasoning::{Effort, HARD_ROUND_CAP};

use crate::finish::IncompleteReason;
use session_core::db::suspensions::{BudgetLimits, TreeBudgetCheckpoint};

/// A run's limits. `rounds` is always bounded by [`HARD_ROUND_CAP`]; `None`
/// for seconds or tokens means that dimension is unlimited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    rounds: u32,
    seconds: Option<u64>,
    tokens: Option<u64>,
}

/// Which limit of a [`Budget`] ran out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Limit {
    Rounds,
    Seconds,
    Tokens,
}

/// Source of "now". A seam rather than `Instant::now()` called directly:
/// wall-clock time cannot be faked any other way, and a test that slept
/// through a real deadline would be slow and flaky. Production uses
/// [`system_clock`].
pub type Clock = Arc<dyn Fn() -> Instant + Send + Sync>;

pub fn system_clock() -> Clock {
    Arc::new(Instant::now)
}

impl Budget {
    /// An explicit budget. `rounds` is clamped into `1..=HARD_ROUND_CAP`: a run
    /// always gets a final round, and no caller can lift the platform ceiling.
    pub fn new(rounds: u32, seconds: Option<u64>, tokens: Option<u64>) -> Self {
        Self {
            rounds: rounds.clamp(1, HARD_ROUND_CAP),
            seconds,
            tokens,
        }
    }

    /// The budget an interactive chat turn runs under: the effort level's
    /// round cap, no time or token limit.
    pub fn from_effort(effort: Effort) -> Self {
        Self::new(effort.max_rounds(), None, None)
    }

    pub fn rounds(&self) -> u32 {
        self.rounds
    }

    pub fn seconds(&self) -> Option<u64> {
        self.seconds
    }

    pub fn tokens(&self) -> Option<u64> {
        self.tokens
    }

    /// This budget, tightened to `seconds` and `tokens` where those are
    /// lower: a run inside a larger allowance gets no more than what is left
    /// of it.
    pub fn capped(self, seconds: Option<u64>, tokens: Option<u64>) -> Self {
        let min = |own: Option<u64>, cap: Option<u64>| match (own, cap) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        Self {
            rounds: self.rounds,
            seconds: min(self.seconds, seconds),
            tokens: min(self.tokens, tokens),
        }
    }

    /// The time or token limit that has run out, if any. Rounds are counted by
    /// the loop itself.
    pub fn intersect(self, other: Self) -> Self {
        Self {
            rounds: self.rounds.min(other.rounds),
            ..self.capped(other.seconds, other.tokens)
        }
    }

    pub fn limits(self) -> BudgetLimits {
        BudgetLimits {
            rounds: self.rounds,
            seconds: self.seconds,
            tokens: self.tokens,
        }
    }

    pub fn from_limits(limits: BudgetLimits) -> Self {
        Self::new(limits.rounds, limits.seconds, limits.tokens)
    }

    pub fn exhausted(&self, elapsed: Duration, tokens_used: u64) -> Option<Limit> {
        if self
            .seconds
            .is_some_and(|s| elapsed >= Duration::from_secs(s))
        {
            return Some(Limit::Seconds);
        }
        if self.tokens.is_some_and(|t| tokens_used >= t) {
            return Some(Limit::Tokens);
        }
        None
    }

    /// The reason a contracted run settles with when `limit` stopped it after
    /// `rounds_used` rounds.
    pub fn incomplete_reason(&self, limit: Limit, rounds_used: u32) -> IncompleteReason {
        match limit {
            Limit::Rounds => IncompleteReason::RoundBudgetExhausted {
                rounds: rounds_used,
            },
            Limit::Seconds => IncompleteReason::SecondsExhausted {
                seconds: self.seconds.unwrap_or_default(),
            },
            Limit::Tokens => IncompleteReason::TokensExhausted {
                tokens: self.tokens.unwrap_or_default(),
            },
        }
    }
}

/// Tokens spent by several runs together, so an allowance can span them: a
/// `loop` route's budget covers every worker and critic run it starts. The
/// driver adds each round's tokens to the meter its agent run carries.
pub struct SpendMeter {
    tokens: AtomicU64,
    rounds: AtomicU32,
    usage_known: AtomicBool,
    budget: Option<Budget>,
    clock: Clock,
    started_at: Instant,
    restored_seconds: u64,
}

impl std::fmt::Debug for SpendMeter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpendMeter")
            .field("tokens", &self.tokens())
            .field("budget", &self.budget)
            .finish_non_exhaustive()
    }
}

impl Default for SpendMeter {
    fn default() -> Self {
        let clock = system_clock();
        Self {
            tokens: AtomicU64::new(0),
            rounds: AtomicU32::new(0),
            usage_known: AtomicBool::new(true),
            budget: None,
            started_at: clock(),
            clock,
            restored_seconds: 0,
        }
    }
}

impl SpendMeter {
    pub fn for_budget(
        limit: Budget,
        clock: Clock,
        checkpoint: Option<&TreeBudgetCheckpoint>,
    ) -> Self {
        let budget = checkpoint.map_or(limit, |saved| {
            Budget::from_limits(saved.limits).intersect(limit)
        });
        Self {
            tokens: AtomicU64::new(checkpoint.map_or(0, |saved| saved.tokens)),
            rounds: AtomicU32::new(checkpoint.map_or(0, |saved| saved.rounds)),
            usage_known: AtomicBool::new(checkpoint.is_none_or(|saved| saved.usage_known)),
            budget: Some(budget),
            started_at: clock(),
            clock,
            restored_seconds: checkpoint.map_or(0, |saved| saved.seconds),
        }
    }

    pub fn add(&self, tokens: u64) {
        let _ = self
            .tokens
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |old| {
                Some(old.saturating_add(tokens))
            });
    }

    pub fn record_tokens(&self, tokens: Option<u64>) {
        match tokens {
            Some(tokens) => self.add(tokens),
            None => self.usage_known.store(false, Ordering::Release),
        }
    }

    pub fn tokens(&self) -> u64 {
        self.tokens.load(Ordering::Acquire)
    }

    fn elapsed_seconds(&self) -> u64 {
        self.restored_seconds.saturating_add(
            (self.clock)()
                .saturating_duration_since(self.started_at)
                .as_secs(),
        )
    }

    pub fn exhausted(&self) -> Option<Limit> {
        let budget = self.budget?;
        if budget
            .seconds
            .is_some_and(|limit| self.elapsed_seconds() >= limit)
        {
            return Some(Limit::Seconds);
        }
        if budget.tokens.is_some_and(|limit| {
            !self.usage_known.load(Ordering::Acquire) || self.tokens() >= limit
        }) {
            return Some(Limit::Tokens);
        }
        if self.rounds.load(Ordering::Acquire) >= budget.rounds {
            return Some(Limit::Rounds);
        }
        None
    }

    pub fn remaining(&self) -> Option<Budget> {
        let budget = self.budget?;
        if self.exhausted().is_some() {
            return None;
        }
        Some(Budget {
            rounds: budget
                .rounds
                .saturating_sub(self.rounds.load(Ordering::Acquire)),
            seconds: budget
                .seconds
                .map(|limit| limit.saturating_sub(self.elapsed_seconds())),
            tokens: budget
                .tokens
                .map(|limit| limit.saturating_sub(self.tokens())),
        })
    }

    pub fn begin_round(&self) -> Result<(), Limit> {
        if let Some(limit) = self.exhausted() {
            return Err(limit);
        }
        let Some(budget) = self.budget else {
            return Ok(());
        };
        self.rounds
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |old| {
                (old < budget.rounds).then(|| old + 1)
            })
            .map(|_| ())
            .map_err(|_| Limit::Rounds)
    }

    pub fn begin_final_round(&self) {
        let _ = self
            .rounds
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |old| {
                Some(old.saturating_add(1))
            });
    }

    pub fn snapshot(&self, local_limits: Budget) -> TreeBudgetCheckpoint {
        TreeBudgetCheckpoint {
            limits: self.budget.unwrap_or(local_limits).limits(),
            local_limits: local_limits.limits(),
            rounds: self.rounds.load(Ordering::Acquire),
            seconds: self.elapsed_seconds(),
            tokens: self.tokens(),
            usage_known: self.usage_known.load(Ordering::Acquire),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounds_are_capped_by_the_platform_ceiling() {
        assert_eq!(Budget::new(10_000, None, None).rounds(), HARD_ROUND_CAP);
    }

    #[test]
    fn a_run_always_gets_at_least_one_round() {
        assert_eq!(Budget::new(0, None, None).rounds(), 1);
    }

    #[test]
    fn chat_derives_its_budget_from_effort() {
        for effort in Effort::ALL {
            let b = Budget::from_effort(effort);
            assert_eq!(b.rounds(), effort.max_rounds());
            assert_eq!((b.seconds(), b.tokens()), (None, None));
        }
    }

    #[test]
    fn a_cap_only_ever_tightens_a_budget() {
        let own = Budget::new(8, Some(60), None);
        let capped = own.capped(Some(20), Some(500));
        assert_eq!(
            (capped.rounds(), capped.seconds(), capped.tokens()),
            (8, Some(20), Some(500))
        );
        let loose = own.capped(Some(600), None);
        assert_eq!((loose.seconds(), loose.tokens()), (Some(60), None));
    }

    #[test]
    fn a_meter_sums_what_several_runs_spend() {
        let meter = SpendMeter::default();
        meter.add(600);
        meter.add(150);
        assert_eq!(meter.tokens(), 750);
    }

    #[test]
    fn unlimited_dimensions_never_run_out() {
        let b = Budget::new(8, None, None);
        assert_eq!(b.exhausted(Duration::from_secs(1 << 30), u64::MAX), None);
    }

    #[test]
    fn seconds_and_tokens_run_out_at_their_limit() {
        let b = Budget::new(8, Some(30), Some(1000));
        assert_eq!(b.exhausted(Duration::from_secs(29), 999), None);
        assert_eq!(
            b.exhausted(Duration::from_secs(30), 0),
            Some(Limit::Seconds)
        );
        assert_eq!(b.exhausted(Duration::ZERO, 1000), Some(Limit::Tokens));
    }

    #[test]
    fn shared_round_reservations_stop_at_the_exact_limit() {
        let meter = SpendMeter::for_budget(Budget::new(2, None, None), system_clock(), None);
        assert_eq!(meter.begin_round(), Ok(()));
        assert_eq!(meter.remaining().unwrap().rounds(), 1);
        assert_eq!(meter.begin_round(), Ok(()));
        assert_eq!(meter.begin_round(), Err(Limit::Rounds));
        assert_eq!(meter.remaining(), None);
    }

    #[test]
    fn unknown_usage_prevents_further_finite_token_work() {
        let meter = SpendMeter::for_budget(Budget::new(8, None, Some(100)), system_clock(), None);
        meter.record_tokens(Some(20));
        assert_eq!(meter.remaining().unwrap().tokens(), Some(80));
        meter.record_tokens(None);
        assert_eq!(meter.remaining(), None);
        assert_eq!(meter.begin_round(), Err(Limit::Tokens));
        assert!(!meter.snapshot(Budget::new(4, None, Some(50))).usage_known);
    }

    #[test]
    fn missing_usage_does_not_invent_a_token_limit() {
        let meter = SpendMeter::for_budget(Budget::new(2, None, None), system_clock(), None);
        meter.record_tokens(None);
        assert!(meter.remaining().is_some());
        assert_eq!(meter.begin_round(), Ok(()));
    }

    #[test]
    fn tokens_saturate_without_wrapping_into_a_fresh_allowance() {
        let meter = SpendMeter::default();
        meter.add(u64::MAX);
        meter.add(1);
        assert_eq!(meter.tokens(), u64::MAX);
    }

    #[test]
    fn checkpoint_preserves_limits_usage_and_local_caps() {
        let root = Budget::new(8, Some(60), Some(100));
        let local = Budget::new(3, Some(20), Some(50));
        let meter = SpendMeter::for_budget(root, system_clock(), None);
        meter.begin_round().unwrap();
        meter.record_tokens(Some(25));
        let saved = meter.snapshot(local);
        let restored =
            SpendMeter::for_budget(Budget::new(99, None, None), system_clock(), Some(&saved));
        assert_eq!(restored.remaining().unwrap().rounds(), 7);
        assert_eq!(restored.remaining().unwrap().tokens(), Some(75));
        assert_eq!(restored.snapshot(local).local_limits, saved.local_limits);
        assert_eq!(restored.tokens(), 25);
    }

    #[test]
    fn shared_clock_counts_elapsed_time_once() {
        let start = Instant::now();
        let now = Arc::new(std::sync::Mutex::new(start));
        let clock: Clock = {
            let now = now.clone();
            Arc::new(move || *now.lock().unwrap())
        };
        let meter = SpendMeter::for_budget(Budget::new(8, Some(10), None), clock.clone(), None);
        *now.lock().unwrap() = start + Duration::from_secs(4);
        assert_eq!(meter.remaining().unwrap().seconds(), Some(6));
        let checkpoint = meter.snapshot(Budget::new(3, Some(5), None));
        let restored =
            SpendMeter::for_budget(Budget::new(8, Some(10), None), clock, Some(&checkpoint));
        *now.lock().unwrap() = start + Duration::from_secs(10);
        assert_eq!(restored.begin_round(), Err(Limit::Seconds));
    }

    #[test]
    fn intersection_tightens_every_dimension() {
        assert_eq!(
            Budget::new(8, Some(20), None).intersect(Budget::new(3, None, Some(50))),
            Budget::new(3, Some(20), Some(50))
        );
    }
    #[test]
    fn bounded_final_request_records_observed_rounds_after_exhaustion() {
        let meter = SpendMeter::for_budget(Budget::new(1, None, None), system_clock(), None);
        meter.begin_round().unwrap();
        assert_eq!(meter.begin_round(), Err(Limit::Rounds));
        meter.begin_final_round();
        assert_eq!(meter.snapshot(Budget::new(1, None, None)).rounds, 2);
        assert_eq!(meter.remaining(), None);
    }
}
