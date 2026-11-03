// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! An agent's sliding-window rates (`docs/agent-visitors.md` → "Rates"
//! and "Identity verifiers"): a visitor's and a client IP's admitted requests,
//! and a verifier's sends and lookups. Exact to the second. Separate from the
//! spend limits in `aiplane_core::server::limits`, which meter tokens per
//! user, token and agent and know nothing about visitors.
//!
//! **One primitive.** [`record_within`] checks an event against every window
//! it counts in and records it, on a [`WriteTx`]: the windows are read under
//! the write lock the event is then written under, so parallel requests
//! queue instead of all passing the check before any of them is counted. A
//! refused event writes nothing and so never counts. The events are rows of
//! `rate_events`, one per window (`migrations/0077_agent_builder.sql`).

use jiff::{SignedDuration, Timestamp};
use sqlx::Row;

use crate::db::{DbError, Pool, WriteTx, window_key};

/// What one window counts: the subject of a rate, as stored in
/// `rate_events.counter`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Counter(String);

impl Counter {
    /// A key the caller composes (a verifier's sends to one address). The
    /// visitor windows have their own constructors.
    pub fn new(key: impl Into<String>) -> Self {
        Self(key.into())
    }

    /// The requests admitted into one conversation, on whichever channel.
    pub fn conversation(conversation: Inbound<'_>) -> Self {
        match conversation {
            Inbound::Visitor(id) => Self(format!("conversation:visitor:{id}")),
            Inbound::A2a(id) => Self(format!("conversation:a2a:{id}")),
        }
    }

    /// The requests admitted from one client IP, on every channel at once:
    /// a client that splits its traffic between the widget and A2A meets
    /// one limit.
    pub fn ip(ip: &str) -> Self {
        Self(format!("ip:{ip}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One window an event must fit into.
#[derive(Debug, Clone)]
pub struct Window {
    pub scope: RateScope,
    pub rate: Rate,
    pub counter: Counter,
}

/// An admitted event: how many events each window already held, in the
/// order the windows were given (this one not included).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Admitted {
    pub seen: Vec<u32>,
}

/// Record one event of agent `principal_id` at `at`, counted in every one of
/// `windows`, unless it does not fit one of them: the first window it does
/// not fit is the answer, and nothing is recorded. Only the newest `max`
/// events of a window are read: whether there are `max` of them, and when
/// the oldest of those leaves, is all [`sliding_window`] decides on. Drops
/// the agent's expired events on the way.
pub async fn record_within(
    tx: &mut WriteTx,
    principal_id: &str,
    windows: &[Window],
    at: Timestamp,
) -> Result<Result<Admitted, RateExceeded>, DbError> {
    let mut seen = Vec::with_capacity(windows.len());
    for w in windows {
        let rows = sqlx::query(
            "SELECT created_at FROM rate_events
              WHERE principal_id = ? AND counter = ? AND created_at >= ?
              ORDER BY created_at DESC LIMIT ?",
        )
        .bind(principal_id)
        .bind(w.counter.as_str())
        .bind(window_key(w.rate.since(at)))
        .bind(i64::from(w.rate.max))
        .fetch_all(&mut **tx)
        .await?;
        let mut times: Vec<Timestamp> = Vec::with_capacity(rows.len());
        for row in &rows {
            let raw: String = row.try_get("created_at")?;
            let parsed: Timestamp =
                format!("{raw}Z")
                    .parse()
                    .map_err(|e: jiff::Error| DbError::Decode {
                        column: "created_at",
                        source: e.into(),
                    })?;
            times.push(parsed);
        }
        if let Err(exceeded) = sliding_window(w.scope, w.rate, &times, at) {
            return Ok(Err(exceeded));
        }
        seen.push(u32::try_from(times.len()).unwrap_or(u32::MAX));
    }
    sqlx::query("DELETE FROM rate_events WHERE principal_id = ? AND expires_at < ?")
        .bind(principal_id)
        .bind(window_key(at))
        .execute(&mut **tx)
        .await?;
    for w in windows {
        sqlx::query(
            "INSERT INTO rate_events (principal_id, counter, created_at, expires_at)
             VALUES (?, ?, ?, ?)",
        )
        .bind(principal_id)
        .bind(w.counter.as_str())
        .bind(window_key(at))
        .bind(window_key(at.checked_add(w.rate.per).unwrap_or(at)))
        .execute(&mut **tx)
        .await?;
    }
    Ok(Ok(Admitted { seen }))
}

/// [`record_within`] in a write transaction of its own.
pub async fn record_now(
    pool: &Pool,
    principal_id: &str,
    windows: &[Window],
    at: Timestamp,
) -> Result<Result<Admitted, RateExceeded>, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let admitted = record_within(&mut tx, principal_id, windows, at).await?;
    if admitted.is_ok() {
        tx.commit().await?;
    }
    Ok(admitted)
}

/// Admit a visitor's request to agent `who.principal_id` against its
/// per-conversation and per-IP rates, counting it when it fits. What is
/// counted is the admission, whatever the request then does. A database
/// error admits without counting: like the spend limits, a rate must not
/// wedge live traffic.
pub async fn admit_visitor(
    pool: &Pool,
    rates: &VisitorRates,
    who: &VisitorKey<'_>,
    now: Timestamp,
) -> Result<(), RateExceeded> {
    let mut windows = Vec::with_capacity(2);
    if let Some(conversation) = who.conversation {
        windows.push(Window {
            scope: RateScope::Visitor,
            rate: rates.visitor,
            counter: Counter::conversation(conversation),
        });
    }
    if let Some(ip) = who.ip {
        windows.push(Window {
            scope: RateScope::Ip,
            rate: rates.ip,
            counter: Counter::ip(ip),
        });
    }
    if windows.is_empty() {
        return Ok(());
    }
    match record_now(pool, who.principal_id, &windows, now).await {
        Ok(admitted) => admitted.map(|_| ()),
        Err(err) => {
            tracing::warn!(error = %err, "rates: recording a visitor admission; allowing");
            Ok(())
        }
    }
}

/// Count a request admitted before the conversation it opened had an id
/// (an A2A message without a context) in that conversation's window, as
/// its first event. Nobody else can name the new conversation yet, so
/// nothing races it; a zero rate counts nothing and refuses the next one.
pub async fn count_opening(
    pool: &Pool,
    principal_id: &str,
    rate: Rate,
    conversation: Inbound<'_>,
    at: Timestamp,
) {
    let window = Window {
        scope: RateScope::Visitor,
        rate,
        counter: Counter::conversation(conversation),
    };
    if let Err(err) = record_now(pool, principal_id, &[window], at).await {
        tracing::warn!(error = %err, "rates: counting the request that opened a conversation");
    }
}

/// One conversation on an inbound channel, by the key that channel names it
/// with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Inbound<'a> {
    /// An embed visitor session (`visitor_sessions.id`).
    Visitor(&'a str),
    /// An A2A context (`a2a_contexts.session_id`, its conversation's id).
    A2a(&'a str),
}

/// At most `max` events in any `per`. Unlike a limit's
/// [`Window`](aiplane_core::server::db::limits::Window), which snaps to the
/// hour because it meters spend, a rate is exact to the second: a visitor
/// told to wait 40 seconds may send again after 40 seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rate {
    pub max: u32,
    pub per: SignedDuration,
}

impl Rate {
    fn since(self, now: Timestamp) -> Timestamp {
        now.checked_sub(self.per).unwrap_or(now)
    }
}

/// An agent's visitor rates: per visitor session and per client IP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisitorRates {
    pub visitor: Rate,
    pub ip: Rate,
}

/// Whose admissions a visitor rate counts. No conversation yet when one is
/// being started; no IP when the request carries none.
#[derive(Debug, Clone, Copy)]
pub struct VisitorKey<'a> {
    pub principal_id: &'a str,
    /// The conversation asking, on whichever channel: each counts against
    /// `rates.visitor`.
    pub conversation: Option<Inbound<'a>>,
    pub ip: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateScope {
    Visitor,
    Ip,
    /// A verifier's sends to one email address, across conversations.
    Email,
    /// A verifier's sends or lookups in one conversation.
    Session,
    /// A manager's prompt-assistant calls for one agent.
    Manager,
}

impl RateScope {
    pub fn as_str(self) -> &'static str {
        match self {
            RateScope::Visitor => "visitor",
            RateScope::Ip => "ip",
            RateScope::Email => "email",
            RateScope::Session => "session",
            RateScope::Manager => "manager",
        }
    }
}

/// A refused visitor request: `scope` already reached `max` in the last
/// `per`. `retry_after_secs` is when the oldest counted event leaves the
/// window, served as `Retry-After`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateExceeded {
    pub scope: RateScope,
    pub max: u32,
    pub per: SignedDuration,
    pub retry_after_secs: i64,
}

/// Whether one more event fits `rate` given the `events` already in its
/// window (any order; older ones are ignored).
pub fn sliding_window(
    scope: RateScope,
    rate: Rate,
    events: &[Timestamp],
    now: Timestamp,
) -> Result<(), RateExceeded> {
    let since = rate.since(now);
    let mut inside: Vec<Timestamp> = events.iter().copied().filter(|t| *t >= since).collect();
    if inside.len() < rate.max as usize {
        return Ok(());
    }
    inside.sort_unstable_by(|a, b| b.cmp(a));
    // A zero rate admits nothing, ever; no event's departure frees a slot, so
    // the honest retry is one window from now.
    let leaves = match (rate.max as usize).checked_sub(1) {
        Some(nth) => inside[nth].checked_add(rate.per).unwrap_or(now),
        None => now.checked_add(rate.per).unwrap_or(now),
    };
    Err(RateExceeded {
        scope,
        max: rate.max,
        per: rate.per,
        retry_after_secs: (leaves.as_second() - now.as_second()).max(1),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    const TEN_MIN: SignedDuration = SignedDuration::from_secs(600);

    async fn agent_db() -> Pool {
        let pool = aiplane_core::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        for sql in [
            "INSERT INTO users (id, email, created_at, updated_at)
             VALUES ('u1', 'u1@example.com', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            "INSERT INTO system_principals (id, name, display, created_by, created_at)
             VALUES ('a1', 'support', 'Support', 'u1', '2026-01-01T00:00:00Z')",
        ] {
            sqlx::query(sql).execute(&pool).await.unwrap();
        }
        pool
    }

    async fn counted(pool: &Pool, counter: &Counter) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM rate_events WHERE counter = ?")
            .bind(counter.as_str())
            .fetch_one(pool)
            .await
            .unwrap()
    }

    /// Parallel requests of one visitor from one IP queue on the write lock:
    /// exactly the limit is admitted, and only admitted ones are counted.
    #[tokio::test]
    async fn parallel_admissions_of_one_visitor_admit_exactly_the_limit() {
        let pool = agent_db().await;
        let rates = VisitorRates {
            visitor: Rate {
                max: 3,
                per: TEN_MIN,
            },
            ip: Rate {
                max: 5,
                per: TEN_MIN,
            },
        };
        let now = at("2026-10-01T12:00:00Z");
        let requests: Vec<_> = (0..20)
            .map(|_| {
                let pool = pool.clone();
                tokio::spawn(async move {
                    let who = VisitorKey {
                        principal_id: "a1",
                        conversation: Some(Inbound::Visitor("v1")),
                        ip: Some("192.0.2.1"),
                    };
                    admit_visitor(&pool, &rates, &who, now).await
                })
            })
            .collect();
        let mut admitted = 0;
        for r in requests {
            admitted += usize::from(r.await.unwrap().is_ok());
        }
        assert_eq!(admitted, 3);
        assert_eq!(
            counted(&pool, &Counter::conversation(Inbound::Visitor("v1"))).await,
            3
        );
        assert_eq!(counted(&pool, &Counter::ip("192.0.2.1")).await, 3);
    }

    /// The per-IP window is one count across conversations and channels: a
    /// client that opens a fresh conversation for each request, or splits
    /// them between the widget and A2A, meets the same limit.
    #[tokio::test]
    async fn an_ip_meets_one_limit_across_conversations_and_channels() {
        let pool = agent_db().await;
        let rates = VisitorRates {
            visitor: Rate {
                max: 10,
                per: TEN_MIN,
            },
            ip: Rate {
                max: 2,
                per: TEN_MIN,
            },
        };
        let now = at("2026-10-01T12:00:00Z");
        let from = |conversation| VisitorKey {
            principal_id: "a1",
            conversation,
            ip: Some("192.0.2.1"),
        };
        assert!(admit_visitor(&pool, &rates, &from(None), now).await.is_ok());
        assert!(
            admit_visitor(&pool, &rates, &from(Some(Inbound::A2a("c1"))), now)
                .await
                .is_ok()
        );
        let refused = admit_visitor(&pool, &rates, &from(Some(Inbound::Visitor("v9"))), now)
            .await
            .unwrap_err();
        assert_eq!(refused.scope, RateScope::Ip);
        assert_eq!(
            counted(&pool, &Counter::conversation(Inbound::Visitor("v9"))).await,
            0,
            "a refused request counts nowhere"
        );
    }

    #[tokio::test]
    async fn an_admission_reports_what_each_window_held_and_old_events_expire() {
        let pool = agent_db().await;
        let windows = [
            Window {
                scope: RateScope::Session,
                rate: Rate {
                    max: 3,
                    per: TEN_MIN,
                },
                counter: Counter::new("verifier:x:lookup:session:s1"),
            },
            Window {
                scope: RateScope::Ip,
                rate: Rate {
                    max: 9,
                    per: TEN_MIN,
                },
                counter: Counter::ip("192.0.2.1"),
            },
        ];
        let t0 = at("2026-10-01T12:00:00Z");
        for (n, expected) in [(0, [0, 0]), (1, [1, 1]), (2, [2, 2])] {
            let t = t0.checked_add(SignedDuration::from_secs(n)).unwrap();
            let admitted = record_now(&pool, "a1", &windows, t).await.unwrap().unwrap();
            assert_eq!(admitted.seen, expected);
        }
        assert!(
            record_now(&pool, "a1", &windows, t0)
                .await
                .unwrap()
                .is_err()
        );
        let later = t0.checked_add(TEN_MIN * 2).unwrap();
        let admitted = record_now(&pool, "a1", &windows, later)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(admitted.seen, [0, 0], "the window moved on");
        assert_eq!(
            counted(&pool, &Counter::ip("192.0.2.1")).await,
            1,
            "expired events are dropped when the next one is recorded"
        );
    }

    #[test]
    fn a_rate_admits_until_max_events_sit_inside_the_window() {
        let rate = Rate {
            max: 2,
            per: TEN_MIN,
        };
        let now = at("2026-10-01T12:10:00Z");
        let old = at("2026-10-01T11:59:00Z");
        let one = at("2026-10-01T12:02:00Z");
        let two = at("2026-10-01T12:05:00Z");
        assert!(sliding_window(RateScope::Visitor, rate, &[old, one], now).is_ok());
        let err = sliding_window(RateScope::Visitor, rate, &[two, old, one], now).unwrap_err();
        assert_eq!(err.scope, RateScope::Visitor);
        assert_eq!(err.max, 2);
        assert_eq!(
            err.retry_after_secs, 120,
            "12:02 leaves the window at 12:12, two minutes from now"
        );
    }

    #[test]
    fn a_zero_rate_refuses_without_panicking() {
        let rate = Rate {
            max: 0,
            per: TEN_MIN,
        };
        let now = at("2026-10-01T12:10:00Z");
        let err = sliding_window(RateScope::Visitor, rate, &[], now).unwrap_err();
        assert_eq!(err.max, 0);
        assert_eq!(
            err.retry_after_secs, 600,
            "nothing will ever fit; retry after a window"
        );
        assert!(sliding_window(RateScope::Ip, rate, &[at("2026-10-01T12:09:00Z")], now).is_err());
    }

    #[test]
    fn retry_after_is_when_enough_events_have_left_the_window() {
        let rate = Rate {
            max: 2,
            per: TEN_MIN,
        };
        let now = at("2026-10-01T12:10:00Z");
        let events = [
            at("2026-10-01T12:01:00Z"),
            at("2026-10-01T12:03:00Z"),
            at("2026-10-01T12:09:00Z"),
        ];
        let err = sliding_window(RateScope::Ip, rate, &events, now).unwrap_err();
        assert_eq!(
            err.retry_after_secs, 180,
            "below max once 12:03 is gone too, at 12:13"
        );
    }
}
