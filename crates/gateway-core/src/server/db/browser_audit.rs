// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Append-only audit of `browser_control` batches (migration 0069).
//!
//! One row per batch the tool handed to a user's browser: who asked, which
//! steps, how it ended. It exists because this is the one capability whose
//! effects land somewhere the gateway cannot see — a click in someone's own
//! logged-in session, on a site that keeps no record the operator can read.
//!
//! **What is stored is the shape of an action, never its content.** Step names
//! and an outcome; no URLs, no typed text, no page snapshots, no screenshots. A
//! trail that also captured what was typed into a login form would create a
//! worse exposure than the one it documents, and the questions this table has
//! to answer ("did the assistant submit something as me, and when") do not need
//! the payload.
//!
//! Best-effort at the call site, like [`super::mcp_audit`]: a failed audit
//! write is logged and the tool call still returns.

use jiff::Timestamp;
use sqlx::Row;
use sqlx::sqlite::SqliteRow;
use uuid::Uuid;

use super::{DbError, Pool};

/// One recorded batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserActionEvent {
    pub id: String,
    pub user_id: String,
    pub user_email: String,
    pub turn_id: String,
    pub session_id: Option<String>,
    /// Comma-separated step names, in order.
    pub actions: String,
    /// How many of them change a page rather than observe it.
    pub writes: i64,
    pub outcome: String,
    pub detail: Option<String>,
    pub created_at: Timestamp,
}

/// Longest refusal / error detail stored. Long enough for a real message,
/// short enough that a page cannot write an essay into the audit table.
/// Counted in chars, not bytes — see [`session_core::text::truncate_chars`].
const MAX_DETAIL_LEN: usize = 500;

fn map_row(row: &SqliteRow) -> Result<BrowserActionEvent, DbError> {
    let created_at: String = row.try_get("created_at")?;
    let created_at: Timestamp = created_at
        .parse()
        .map_err(|e: jiff::Error| DbError::Decode {
            column: "created_at",
            source: e.into(),
        })?;
    Ok(BrowserActionEvent {
        id: row.try_get("id")?,
        user_id: row.try_get("user_id")?,
        user_email: row.try_get("user_email")?,
        turn_id: row.try_get("turn_id")?,
        session_id: row.try_get("session_id")?,
        actions: row.try_get("actions")?,
        writes: row.try_get("writes")?,
        outcome: row.try_get("outcome")?,
        detail: row.try_get("detail")?,
        created_at,
    })
}

/// Record one batch.
///
/// The acting user's email is denormalised so the row survives user deletion;
/// an unknown user records an empty email rather than failing the write.
// Flat columns of one audit row, same shape (and same lint) as `mcp_audit`.
// A params struct here would only move the argument list one line up.
#[allow(clippy::too_many_arguments)]
pub async fn record(
    pool: &Pool,
    user_id: &str,
    turn_id: &str,
    session_id: Option<&str>,
    actions: &[&str],
    writes: usize,
    outcome: &str,
    detail: Option<&str>,
) -> Result<(), DbError> {
    // One statement, not a SELECT followed by an INSERT: the email is
    // denormalised inline, and a user who no longer exists still produces a row
    // (with an empty email) rather than a hole in the trail.
    sqlx::query(
        "INSERT INTO browser_action_audit
           (id, user_id, user_email, turn_id, session_id, actions, writes, outcome, detail, created_at)
         SELECT ?, ?, COALESCE((SELECT email FROM users WHERE id = ?), ''), ?, ?, ?, ?, ?, ?, ?",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(user_id)
    .bind(user_id)
    .bind(turn_id)
    .bind(session_id)
    .bind(actions.join(","))
    .bind(writes as i64)
    .bind(outcome)
    .bind(detail.map(|d| session_core::text::truncate_chars(d, MAX_DETAIL_LEN)))
    .bind(Timestamp::now().to_string())
    .execute(pool)
    .await?;
    Ok(())
}

/// The most recent `limit` batches, newest first.
pub async fn recent(pool: &Pool, limit: i64) -> Result<Vec<BrowserActionEvent>, DbError> {
    let rows =
        sqlx::query("SELECT * FROM browser_action_audit ORDER BY created_at DESC, id LIMIT ?")
            .bind(limit)
            .fetch_all(pool)
            .await?;
    rows.iter().map(map_row).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn pool() -> Pool {
        crate::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn a_batch_is_recorded_with_its_shape_only() {
        let pool = pool().await;
        record(
            &pool,
            "u1",
            "t1",
            Some("s1"),
            &["navigate", "read_page", "click"],
            2,
            "ok",
            None,
        )
        .await
        .unwrap();

        let rows = recent(&pool, 10).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].actions, "navigate,read_page,click");
        assert_eq!(rows[0].writes, 2);
        assert_eq!(rows[0].outcome, "ok");
    }

    #[tokio::test]
    async fn an_unknown_user_still_produces_a_row() {
        // The trail must not develop holes because a user was deleted between
        // the action and the audit write.
        let pool = pool().await;
        record(&pool, "ghost", "t1", None, &["read_page"], 0, "ok", None)
            .await
            .unwrap();
        let rows = recent(&pool, 10).await.unwrap();
        assert_eq!(rows[0].user_email, "");
    }

    #[tokio::test]
    async fn a_long_detail_is_truncated() {
        // The refusal reason can originate from a page we visited.
        let pool = pool().await;
        let huge = "x".repeat(MAX_DETAIL_LEN * 3);
        record(
            &pool,
            "u1",
            "t1",
            None,
            &["click"],
            1,
            "refused",
            Some(&huge),
        )
        .await
        .unwrap();
        let rows = recent(&pool, 10).await.unwrap();
        let detail = rows[0].detail.as_deref().unwrap();
        assert!(detail.chars().count() <= MAX_DETAIL_LEN + 1, "{detail}");
        assert!(
            detail.ends_with('\u{2026}'),
            "cut strings carry the ellipsis"
        );
    }
}
