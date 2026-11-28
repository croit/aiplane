// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! What an agent did over a time range, counted from rows that already exist
//! (`docs/agent-activity-log.md` → "Analytics").
//!
//! There is no second event store: conversations and turns come from the chat
//! tables, outcomes from `agent_audit`, cost and tokens from `usage_events`.
//! Nothing a visitor typed or an agent answered is read — only counts, route
//! and slot names, and reason kinds the spec's author chose.
//!
//! Test conversations (the builder's draft chat, recorded as version
//! [`DRAFT_VERSION`]) are left out everywhere, so a manager trying an agent
//! never moves its numbers. SQL does the counting, per UTC day
//! (`substr(created_at, 1, 10)`): the version filter reads the serialized
//! call chain with `json_extract` ([`counts`]), and the exact range compares
//! on `rtrim(created_at, 'Z')` ([`in_range`]). Only the audit rows whose
//! `detail` carries the numbers are fetched, and nothing else is.

use std::collections::BTreeMap;

use jiff::{Timestamp, ToSpan, tz::TimeZone};
use serde::Serialize;
use serde_json::Value;

use super::agent_audit::{AuditKind, sql_kinds};
use super::agents::DRAFT_VERSION;
use super::{DbError, Pool, window_key};

/// The audit kinds whose numbers are in `detail`; the rest are only counted.
const DETAILED_KINDS: [AuditKind; 4] = [
    AuditKind::RouteDecision,
    AuditKind::SubAgentFinished,
    AuditKind::OutputBlocked,
    AuditKind::LimitRefused,
];
const COUNTED_KINDS: [AuditKind; 2] = [AuditKind::SubAgentDispatched, AuditKind::HumanHandoff];

/// A half-open range `[from, to)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    pub from: Timestamp,
    pub to: Timestamp,
}

impl Range {
    fn day_prefixes(&self) -> (String, String) {
        let day = |t: Timestamp| t.to_string()[..10].to_string();
        (day(self.from), day(self.to + 24.hours()))
    }
}

#[derive(Debug, Default, Serialize, PartialEq, schemars::JsonSchema)]
pub struct SubAgents {
    pub dispatched: u64,
    pub finished: u64,
    pub incomplete: u64,
    pub incomplete_by_reason: BTreeMap<String, u64>,
}

#[derive(Debug, Default, Serialize, PartialEq, schemars::JsonSchema)]
pub struct MissingSlot {
    pub route: String,
    pub slot: String,
    pub count: u64,
}

#[derive(Debug, Default, Serialize, PartialEq, schemars::JsonSchema)]
pub struct GateRefusals {
    pub total: u64,
    pub by_route: BTreeMap<String, u64>,
    pub by_missing_slot: Vec<MissingSlot>,
}

#[derive(Debug, Default, Serialize, PartialEq, schemars::JsonSchema)]
pub struct Blocks {
    pub total: u64,
    pub by_action: BTreeMap<String, u64>,
}

#[derive(Debug, Default, Serialize, PartialEq, schemars::JsonSchema)]
pub struct LimitRefusals {
    pub total: u64,
    pub by_kind: BTreeMap<String, u64>,
}

#[derive(Debug, Default, Serialize, PartialEq, schemars::JsonSchema)]
pub struct Spend {
    pub requests: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub tokens: u64,
    pub cost: f64,
}

#[derive(Debug, Serialize, PartialEq, schemars::JsonSchema)]
pub struct Day {
    pub day: String,
    pub conversations: u64,
    pub turns: u64,
    pub tokens: u64,
    pub cost: f64,
    pub refusals: u64,
}

#[derive(Debug, Serialize, PartialEq, schemars::JsonSchema)]
pub struct Analytics {
    pub from: String,
    pub to: String,
    pub version: Option<i64>,
    pub conversations: u64,
    pub turns: u64,
    pub sub_agents: SubAgents,
    pub gate_refusals: GateRefusals,
    pub routes_chosen: BTreeMap<String, u64>,
    pub output_blocks: Blocks,
    pub limit_refusals: LimitRefusals,
    pub human_handoffs: u64,
    pub usage: Spend,
    pub daily: Vec<Day>,
}

/// Whether a row belongs in the numbers, as SQL over the row's serialized
/// call chain in `chain`. `?1` is the main agent the numbers are for, `?2`
/// [`DRAFT_VERSION`], `?3` the version asked for, if any.
///
/// A chain whose main agent is someone else is that agent's run, and a chain
/// at the draft version is a builder test. A row with no chain (a visitor
/// refused before any run) has no version, so it counts only when the caller
/// did not ask for one.
fn counts(chain: &str) -> String {
    format!(
        "CASE WHEN {chain} IS NULL OR NOT json_valid({chain}) THEN ?3 IS NULL
         ELSE json_extract({chain}, '$.frames[0].principal_id') IS ?1
          AND json_extract({chain}, '$.frames[0].version') IS NOT ?2
          AND (?3 IS NULL OR json_extract({chain}, '$.frames[0].version') IS ?3) END"
    )
}

/// Whether `created_at` lies in the range. `?4` and `?5` are the exact
/// bounds as [`window_key`]s; `?6` and `?7` the whole days around them, which
/// is what an index on the column can narrow by.
fn in_range(created_at: &str) -> String {
    format!(
        "{created_at} >= ?6 AND {created_at} < ?7
         AND rtrim({created_at}, 'Z') >= ?4 AND rtrim({created_at}, 'Z') < ?5"
    )
}

/// One query's parameters, in the order [`counts`] and [`in_range`] number
/// them.
macro_rules! bind_all {
    ($query:expr, $agent:expr, $version:expr, $range:expr) => {{
        let (lo, hi) = $range.day_prefixes();
        $query
            .bind($agent)
            .bind(DRAFT_VERSION)
            .bind($version)
            .bind(window_key($range.from))
            .bind(window_key($range.to))
            .bind(lo)
            .bind(hi)
    }};
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_string)
}

fn day_of(at: Timestamp) -> String {
    at.to_zoned_datetime(TimeZone::UTC).date().to_string()
}

fn empty_days(range: Range) -> BTreeMap<String, Day> {
    let mut days = BTreeMap::new();
    let mut at = range
        .from
        .to_zoned_datetime(TimeZone::UTC)
        .start_of_day()
        .map_or(range.from, |z| z.timestamp());
    while at < range.to {
        let day = day_of(at);
        days.insert(
            day.clone(),
            Day {
                day,
                conversations: 0,
                turns: 0,
                tokens: 0,
                cost: 0.0,
                refusals: 0,
            },
        );
        at += 24.hours();
    }
    days
}

/// The bucket of a `YYYY-MM-DD` the database grouped by. Every day in the
/// range has one; a row whose day is outside it cannot have passed the range
/// filter, so `None` is only defensive.
fn bucket<'a>(days: &'a mut BTreeMap<String, Day>, day: &str) -> Option<&'a mut Day> {
    days.get_mut(day)
}

pub async fn compute(
    pool: &Pool,
    agent_id: &str,
    range: Range,
    version: Option<i64>,
) -> Result<Analytics, DbError> {
    let mut out = Analytics {
        from: range.from.to_string(),
        to: range.to.to_string(),
        version,
        conversations: 0,
        turns: 0,
        sub_agents: SubAgents::default(),
        gate_refusals: GateRefusals::default(),
        routes_chosen: BTreeMap::new(),
        output_blocks: Blocks::default(),
        limit_refusals: LimitRefusals::default(),
        human_handoffs: 0,
        usage: Spend::default(),
        daily: Vec::new(),
    };
    let mut days = empty_days(range);

    let sql = format!(
        "SELECT substr(created_at, 1, 10), COUNT(*) FROM chat_sessions
          WHERE principal_id = ?1 AND user_id IS NULL AND parent_turn_id IS NULL
            AND agent_version IS NOT ?2 AND (?3 IS NULL OR agent_version = ?3)
            AND {}
          GROUP BY 1",
        in_range("created_at")
    );
    let sessions: Vec<(String, i64)> = bind_all!(sqlx::query_as(&sql), agent_id, version, range)
        .fetch_all(pool)
        .await?;
    for (day, n) in sessions {
        out.conversations += n as u64;
        if let Some(d) = bucket(&mut days, &day) {
            d.conversations += n as u64;
        }
    }

    let sql = format!(
        "SELECT substr(t.created_at, 1, 10), COUNT(*) FROM chat_turns t
           JOIN chat_sessions s ON s.id = t.session_id
          WHERE s.principal_id = ?1 AND s.user_id IS NULL AND s.parent_turn_id IS NULL
            AND s.agent_version IS NOT ?2 AND (?3 IS NULL OR s.agent_version = ?3)
            AND t.role = 'user' AND {}
          GROUP BY 1",
        in_range("t.created_at")
    );
    let turns: Vec<(String, i64)> = bind_all!(sqlx::query_as(&sql), agent_id, version, range)
        .fetch_all(pool)
        .await?;
    for (day, n) in turns {
        out.turns += n as u64;
        if let Some(d) = bucket(&mut days, &day) {
            d.turns += n as u64;
        }
    }

    let sql = format!(
        "SELECT kind, COUNT(*) FROM agent_audit
          WHERE principal_id = ?1 AND kind IN ({}) AND {} AND {}
          GROUP BY kind",
        sql_kinds(COUNTED_KINDS),
        counts("chain"),
        in_range("created_at")
    );
    let counted: Vec<(String, i64)> = bind_all!(sqlx::query_as(&sql), agent_id, version, range)
        .fetch_all(pool)
        .await?;
    for (kind, n) in counted {
        match AuditKind::parse(&kind) {
            Some(AuditKind::SubAgentDispatched) => out.sub_agents.dispatched += n as u64,
            Some(AuditKind::HumanHandoff) => out.human_handoffs += n as u64,
            _ => {}
        }
    }

    let sql = format!(
        "SELECT kind, detail, substr(created_at, 1, 10) FROM agent_audit
          WHERE principal_id = ?1 AND kind IN ({}) AND {} AND {}",
        sql_kinds(DETAILED_KINDS),
        counts("chain"),
        in_range("created_at")
    );
    let detailed: Vec<(String, String, String)> =
        bind_all!(sqlx::query_as(&sql), agent_id, version, range)
            .fetch_all(pool)
            .await?;
    let mut missing: BTreeMap<(String, String), u64> = BTreeMap::new();
    for (kind, detail, day) in detailed {
        let detail: Value = serde_json::from_str(&detail).unwrap_or(Value::Null);
        match AuditKind::parse(&kind) {
            Some(AuditKind::RouteDecision) => match text(&detail, "picked") {
                Some(route) => *out.routes_chosen.entry(route).or_default() += 1,
                None if detail["reason"] == "no_open_route" => {
                    out.gate_refusals.total += 1;
                    if let Some(d) = bucket(&mut days, &day) {
                        d.refusals += 1;
                    }
                    for gate in detail["routes"].as_array().into_iter().flatten() {
                        let Some(route) = text(gate, "route") else {
                            continue;
                        };
                        if gate["gate"]["status"] != "closed" {
                            continue;
                        }
                        *out.gate_refusals.by_route.entry(route.clone()).or_default() += 1;
                        for unmet in gate["gate"]["missing"].as_array().into_iter().flatten() {
                            if let Some(slot) = text(unmet, "slot") {
                                *missing.entry((route.clone(), slot)).or_default() += 1;
                            }
                        }
                    }
                }
                None => {}
            },
            Some(AuditKind::SubAgentFinished) => match detail["outcome"]["status"].as_str() {
                Some("finished") => out.sub_agents.finished += 1,
                Some("incomplete") => {
                    out.sub_agents.incomplete += 1;
                    let reason = text(&detail["outcome"]["reason"], "kind")
                        .unwrap_or_else(|| "unknown".into());
                    *out.sub_agents
                        .incomplete_by_reason
                        .entry(reason)
                        .or_default() += 1;
                }
                _ => {}
            },
            Some(AuditKind::OutputBlocked) => {
                out.output_blocks.total += 1;
                let action = text(&detail, "action").unwrap_or_else(|| "unknown".into());
                *out.output_blocks.by_action.entry(action).or_default() += 1;
            }
            Some(AuditKind::LimitRefused) => {
                let refused = detail["count"].as_u64().unwrap_or(1);
                out.limit_refusals.total += refused;
                if let Some(d) = bucket(&mut days, &day) {
                    d.refusals += refused;
                }
                let limit = text(&detail, "limit").unwrap_or_else(|| "unknown".into());
                *out.limit_refusals.by_kind.entry(limit).or_default() += refused;
            }
            _ => {}
        }
    }
    out.gate_refusals.by_missing_slot = missing
        .into_iter()
        .map(|((route, slot), count)| MissingSlot { route, slot, count })
        .collect();

    type UsageDay = (String, i64, i64, i64, i64, f64);
    let sql = format!(
        "SELECT substr(created_at, 1, 10), COUNT(*),
                SUM(COALESCE(prompt_tokens, 0)), SUM(COALESCE(completion_tokens, 0)),
                SUM(COALESCE(total_tokens,
                               COALESCE(prompt_tokens, 0) + COALESCE(completion_tokens, 0))),
                TOTAL(cost)
           FROM usage_events
          WHERE agent_id = ?1 AND {} AND {}
          GROUP BY 1",
        counts("chain"),
        in_range("created_at")
    );
    let usage: Vec<UsageDay> = bind_all!(sqlx::query_as(&sql), agent_id, version, range)
        .fetch_all(pool)
        .await?;
    for (day, requests, prompt, completion, tokens, cost) in usage {
        out.usage.requests += requests as u64;
        out.usage.prompt_tokens += prompt as u64;
        out.usage.completion_tokens += completion as u64;
        out.usage.tokens += tokens as u64;
        out.usage.cost += cost;
        if let Some(d) = bucket(&mut days, &day) {
            d.tokens += tokens as u64;
            d.cost += cost;
        }
    }
    out.daily = days.into_values().collect();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn chain(main: &str, version: Option<i64>) -> Option<String> {
        Some(json!({ "frames": [{ "principal_id": main, "version": version }] }).to_string())
    }

    async fn counted(chain: Option<String>, agent: &str, version: Option<i64>) -> bool {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query_scalar(&format!(
            "SELECT {} FROM (SELECT ?4 AS chain)",
            counts("chain")
        ))
        .bind(agent)
        .bind(DRAFT_VERSION)
        .bind(version)
        .bind(chain)
        .fetch_one(&pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn a_draft_run_never_counts() {
        assert!(!counted(chain("a", Some(0)), "a", None).await);
    }

    #[tokio::test]
    async fn another_agents_run_never_counts() {
        assert!(!counted(chain("b", Some(1)), "a", None).await);
    }

    #[tokio::test]
    async fn the_version_filter_keeps_only_that_version_and_drops_chainless_rows() {
        assert!(counted(chain("a", Some(2)), "a", Some(2)).await);
        assert!(!counted(chain("a", Some(1)), "a", Some(2)).await);
        assert!(!counted(None, "a", Some(2)).await);
        assert!(counted(None, "a", None).await);
        assert!(counted(chain("a", None), "a", None).await);
        assert!(
            counted(Some("not json".into()), "a", None).await,
            "an unreadable chain is a chainless row"
        );
    }

    #[test]
    fn a_range_asks_the_database_only_for_whole_days() {
        let range = Range {
            from: "2026-10-01T13:00:00Z".parse().unwrap(),
            to: "2026-10-03T01:00:00Z".parse().unwrap(),
        };
        assert_eq!(
            range.day_prefixes(),
            ("2026-10-01".to_string(), "2026-10-04".to_string())
        );
    }

    #[test]
    fn every_day_in_the_range_has_a_bucket_even_with_no_activity() {
        let range = Range {
            from: "2026-10-01T13:00:00Z".parse().unwrap(),
            to: "2026-10-03T01:00:00Z".parse().unwrap(),
        };
        let days: Vec<_> = empty_days(range).into_keys().collect();
        assert_eq!(days, ["2026-10-01", "2026-10-02", "2026-10-03"]);
    }
}
