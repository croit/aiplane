// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Mid-turn interjections — see `0069_chat_turn_steers.sql` for why they are
//! rows at all, and why their outcome has three values rather than two.
//!
//! The lifecycle:
//!
//! 1. The HTTP handler [`insert_steer`]s what the user typed and hands the
//!    running worker the same note in memory.
//! 2. The driver folds it into the prompt at a round boundary and settles it
//!    as [`SteerStatus::Delivered`].
//! 3. A note the turn ended before reaching stays `Pending`; the client
//!    submits it as an ordinary next message, which settles it as
//!    [`SteerStatus::Resent`]. That settle doubles as the claim check that
//!    stops two tabs from sending it twice.
//! 4. The transcript and the replayed history read them back with
//!    [`list_steers`] / `list_turns`.

use super::*;

/// What became of an interjection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SteerStatus {
    /// Typed, not yet accounted for — the turn is still running.
    Pending,
    /// Handed to the model mid-turn, at a round boundary.
    Delivered,
    /// The turn ended before a round could carry it, so it was submitted as
    /// an ordinary next message instead.
    Resent,
    /// The turn ended before reaching it and the user threw it away rather
    /// than re-sending it.
    ///
    /// A fourth value rather than deleting the row: the note was part of the
    /// conversation the moment it was typed, and a transcript that quietly
    /// loses a sentence someone wrote is the thing this table exists to
    /// prevent. It is also what stops the client re-queueing a note the user
    /// has already dismissed — `pending` is the only state that comes back.
    Discarded,
}

impl SteerStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            SteerStatus::Pending => "pending",
            SteerStatus::Delivered => "delivered",
            SteerStatus::Resent => "resent",
            SteerStatus::Discarded => "discarded",
        }
    }

    fn parse(raw: &str) -> Result<Self, DbError> {
        match raw {
            "pending" => Ok(SteerStatus::Pending),
            "delivered" => Ok(SteerStatus::Delivered),
            "resent" => Ok(SteerStatus::Resent),
            "discarded" => Ok(SteerStatus::Discarded),
            other => Err(DbError::Decode {
                column: "status",
                source: anyhow::anyhow!("unknown steer status {other:?}"),
            }),
        }
    }
}

/// One interjection as stored.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnSteer {
    pub id: String,
    pub turn_id: String,
    pub seq: i64,
    pub text: String,
    pub status: SteerStatus,
    pub created_at: Timestamp,
    /// When it stopped being `Pending`. `None` while it still is.
    pub settled_at: Option<Timestamp>,
}

pub(crate) fn map_steer(row: &SqliteRow) -> Result<TurnSteer, DbError> {
    Ok(TurnSteer {
        id: row.try_get("id")?,
        turn_id: row.try_get("turn_id")?,
        seq: row.try_get("seq")?,
        text: row.try_get("text")?,
        status: SteerStatus::parse(row.try_get::<String, _>("status")?.as_str())?,
        created_at: parse_ts(row.try_get("created_at")?, "created_at")?,
        settled_at: parse_optional_ts(row.try_get("settled_at")?, "settled_at")?,
    })
}

/// Record an interjection against the turn it is aimed at, if that turn is
/// still running. `None` when it is not.
///
/// The `HAVING COUNT(t.id) > 0` is load-bearing and easy to lose: an
/// aggregate without `GROUP BY` returns one row even when the `WHERE` matched
/// nothing, so without it a note against a finished turn inserted happily with
/// `seq = 0`. The test pins it.
///
/// The liveness check is part of the INSERT for the same reason the sequence
/// is: a turn ends on its own schedule, so anything checked before the write
/// can be false by the time the write lands. Reading the worker first and
/// deleting the row afterwards left a window in which a note was recorded
/// against a finished answer, handed to an inbox nobody drains, and reported
/// to the caller as accepted.
///
/// The id is generated here and returned, because the caller has to put the
/// same id on the in-memory note it hands the worker — that is what lets the
/// driver settle this exact row when the model is handed it.
pub async fn insert_steer(
    pool: &Pool,
    turn_id: &str,
    text: &str,
) -> Result<Option<TurnSteer>, DbError> {
    let id = Uuid::new_v4().to_string();
    let created_at = Timestamp::now();
    // The sequence is computed *inside* the INSERT. Reading the max first and
    // inserting second is two statements with a gap in between, and two
    // interjections racing on the same turn — two tabs, or a fast double
    // Ctrl+Enter — both read the same number and the loser dies on
    // `UNIQUE (turn_id, seq)`, surfacing as a 500 on a note the user just
    // typed.
    let row = sqlx::query(
        r#"INSERT INTO chat_turn_steers (id, turn_id, seq, text, status, created_at)
           SELECT ?, ?, COALESCE(MAX(s.seq), -1) + 1, ?, ?, ?
           FROM chat_turns t
           LEFT JOIN chat_turn_steers s ON s.turn_id = t.id
           WHERE t.id = ? AND t.status = 'in_progress'
           HAVING COUNT(t.id) > 0
           RETURNING seq"#,
    )
    .bind(&id)
    .bind(turn_id)
    .bind(text)
    .bind(SteerStatus::Pending.as_str())
    .bind(created_at.to_string())
    .bind(turn_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|row| TurnSteer {
        id,
        turn_id: turn_id.to_string(),
        seq: row.try_get("seq").unwrap_or(0),
        text: text.to_string(),
        status: SteerStatus::Pending,
        created_at,
        settled_at: None,
    }))
}

/// Settle a pending interjection, reporting whether *this* caller is the one
/// that settled it.
///
/// The `status = 'pending'` guard is load-bearing in two places: it keeps a
/// re-delivery from moving a timestamp the user is already looking at, and it
/// is what makes re-sending safe when two browser tabs are looking at the same
/// finished turn — the second one settles nothing, learns it lost, and drops
/// its submit instead of duplicating the message.
pub async fn settle_steer(pool: &Pool, id: &str, status: SteerStatus) -> Result<bool, DbError> {
    if status == SteerStatus::Pending {
        return Err(DbError::Decode {
            column: "status",
            source: anyhow::anyhow!("settle_steer called with status=pending"),
        });
    }
    let done = sqlx::query(
        r#"UPDATE chat_turn_steers
           SET status = ?, settled_at = ?
           WHERE id = ? AND status = 'pending'"#,
    )
    .bind(status.as_str())
    .bind(Timestamp::now().to_string())
    .bind(id)
    .execute(pool)
    .await?;
    Ok(done.rows_affected() > 0)
}

/// Hand a claim back: return a settled interjection to `pending`.
///
/// The re-send path claims a note *before* submitting the message it stands in
/// for, so that two tabs cannot both send it. When that submit is then refused
/// — the conversation is busy, the user is at their parallel-turn ceiling, a
/// write fails — the claim has to be given back, or the note is marked as
/// dealt with while the sentence it carried was never sent, and the client
/// throws its copy away on the next attempt ("already settled"). That is a
/// silent loss of something the user typed.
///
/// Guarded on the status it is undoing, so this can never resurrect a note the
/// model really was handed.
pub async fn release_steer(pool: &Pool, id: &str, from: SteerStatus) -> Result<bool, DbError> {
    let done = sqlx::query(
        r#"UPDATE chat_turn_steers
           SET status = 'pending', settled_at = NULL
           WHERE id = ? AND status = ?"#,
    )
    .bind(id)
    .bind(from.as_str())
    .execute(pool)
    .await?;
    Ok(done.rows_affected() > 0)
}

/// Every interjection in a conversation, bucketed by the turn it belongs to.
/// One query for the session, like `list_turns` does for tool calls.
pub(crate) async fn list_steers_for_session(
    pool: &Pool,
    session_id: &str,
) -> Result<std::collections::HashMap<String, Vec<TurnSteer>>, DbError> {
    let rows = sqlx::query(
        r#"SELECT s.id, s.turn_id, s.seq, s.text, s.status, s.created_at, s.settled_at
           FROM chat_turn_steers s
           JOIN chat_turns t ON t.id = s.turn_id
           WHERE t.session_id = ?
           ORDER BY s.turn_id, s.seq ASC"#,
    )
    .bind(session_id)
    .fetch_all(pool)
    .await?;
    let mut by_turn: std::collections::HashMap<String, Vec<TurnSteer>> =
        std::collections::HashMap::new();
    for row in &rows {
        let steer = map_steer(row)?;
        by_turn
            .entry(steer.turn_id.clone())
            .or_default()
            .push(steer);
    }
    Ok(by_turn)
}

/// The interjections of a single turn, in arrival order.
pub async fn list_steers(pool: &Pool, turn_id: &str) -> Result<Vec<TurnSteer>, DbError> {
    let rows = list_steers_query(pool, turn_id).await?;
    rows.iter().map(map_steer).collect()
}

/// The raw rows behind [`list_steers`], so a caller that is already fetching
/// another of the turn's side tables can run both at once. Returns rows rather
/// than values because `try_join!` wants one future, not a decode step.
pub(crate) fn list_steers_query<'a>(
    pool: &'a Pool,
    turn_id: &'a str,
) -> impl std::future::Future<Output = Result<Vec<SqliteRow>, sqlx::Error>> + 'a {
    sqlx::query(
        r#"SELECT id, turn_id, seq, text, status, created_at, settled_at
           FROM chat_turn_steers
           WHERE turn_id = ?
           ORDER BY seq ASC"#,
    )
    .bind(turn_id)
    .fetch_all(pool)
}

/// Read one interjection by id, scoped to the conversation it must belong to.
///
/// The scope is a parameter rather than a check the caller bolts on
/// afterwards, like every other accessor here (`get_turn`,
/// `list_steers_for_session`): the id arrives from the client, so "this note
/// belongs to that conversation" is part of the question, not a follow-up to
/// it. A caller cannot forget it.
pub async fn get_steer(
    pool: &Pool,
    session_id: &str,
    id: &str,
) -> Result<Option<TurnSteer>, DbError> {
    let row = sqlx::query(
        r#"SELECT s.id, s.turn_id, s.seq, s.text, s.status, s.created_at, s.settled_at
           FROM chat_turn_steers s
           JOIN chat_turns t ON t.id = s.turn_id
           WHERE s.id = ? AND t.session_id = ?"#,
    )
    .bind(id)
    .bind(session_id)
    .fetch_optional(pool)
    .await?;
    row.as_ref().map(map_steer).transpose()
}
