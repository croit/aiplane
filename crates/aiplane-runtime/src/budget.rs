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
//! A sub-agent run is meant to be handed a `Budget` of its own rather than
//! share its parent's.
//!
//! Limits are checked **between rounds**, never mid-stream: cutting an
//! upstream reply off would throw away tokens already paid for and leave a
//! half-written message, while a round is bounded by the model's own output
//! ceiling. A run can therefore overshoot `seconds` or `tokens` by at most one
//! round. See docs/tools-rbac.md → "Run budgets".

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use aiplane_core::server::reasoning::{Effort, HARD_ROUND_CAP};

use crate::finish::IncompleteReason;

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
#[derive(Debug, Default)]
pub struct SpendMeter {
    tokens: AtomicU64,
}

impl SpendMeter {
    pub fn add(&self, tokens: u64) {
        self.tokens.fetch_add(tokens, Ordering::Relaxed);
    }

    pub fn tokens(&self) -> u64 {
        self.tokens.load(Ordering::Relaxed)
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
}
