// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The work queue behind a message that was accepted but not yet answered —
//! see `0070_chat_pending_turns.sql` for why it is rows rather than browser
//! state, and why it is a side table rather than a new turn status.
//!
//! The lifecycle is deliberately short:
//!
//! 1. A submit that cannot start right away persists the user turn as usual
//!    and records how to start it here.
//! 2. When a worker slot frees up — a turn finishing, a slot in another
//!    conversation being released, or the process coming back — the scheduler
//!    claims the oldest waiting turn with [`take_next_for_user`] and starts it.
//! 3. Claiming deletes the row. A turn is started exactly once because exactly
//!    one caller can delete a given row.

use super::*;

/// What a waiting turn needs in order to be started later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingTurn {
    /// The user turn waiting for an answer. Its text is in `chat_turns`.
    pub turn_id: String,
    pub session_id: String,
    pub user_id: String,
    /// The model to answer with, as chosen when the message was sent.
    pub model: String,
    /// Submitted from voice-conversation mode.
    pub voice: bool,
    /// The caller's IP as seen when the message was accepted, and whether that
    /// request was really TLS. The request is gone by the time the turn runs,
    /// and the request-context system message is built from these.
    pub client_ip: Option<String>,
    pub secure: bool,
    pub created_at: Timestamp,
}

fn map_pending(row: &SqliteRow) -> Result<PendingTurn, DbError> {
    Ok(PendingTurn {
        turn_id: row.try_get("turn_id")?,
        session_id: row.try_get("session_id")?,
        user_id: row.try_get("user_id")?,
        model: row.try_get("model")?,
        voice: row.try_get::<i64, _>("voice")? != 0,
        client_ip: row.try_get("client_ip")?,
        secure: row.try_get::<i64, _>("secure")? != 0,
        created_at: parse_ts(row.try_get("created_at")?, "created_at")?,
    })
}

/// Record that this user turn is waiting for an answer.
pub async fn insert_pending_turn(pool: &Pool, pending: &PendingTurn) -> Result<(), DbError> {
    sqlx::query(
        r#"INSERT INTO chat_pending_turns
              (turn_id, session_id, user_id, model, voice, client_ip, secure, created_at)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?)"#,
    )
    .bind(&pending.turn_id)
    .bind(&pending.session_id)
    .bind(&pending.user_id)
    .bind(&pending.model)
    .bind(i64::from(pending.voice))
    .bind(&pending.client_ip)
    .bind(i64::from(pending.secure))
    .bind(pending.created_at.to_string())
    .execute(pool)
    .await?;
    Ok(())
}

/// Claim this user's oldest waiting turn whose conversation is not already
/// being answered, or `None` when there is nothing to start.
///
/// Claiming and reading are one statement. Two schedulers can run at the same
/// moment — a turn finishing in one conversation while another releases a slot
/// — and a read-then-delete would let both start the same turn, which is two
/// workers writing one transcript. `DELETE … RETURNING` hands the row to
/// exactly one of them.
///
/// `busy_sessions` are the conversations that already have a worker; their
/// waiting turns stay put. The list is short (it is bounded by the parallel
/// ceiling), so it goes into the statement rather than being filtered after.
pub async fn take_next_for_user(
    pool: &Pool,
    user_id: &str,
    busy_sessions: &[String],
) -> Result<Option<PendingTurn>, DbError> {
    let placeholders = if busy_sessions.is_empty() {
        "''".to_string()
    } else {
        std::iter::repeat_n("?", busy_sessions.len())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let sql = format!(
        r#"DELETE FROM chat_pending_turns
           WHERE turn_id = (
               SELECT turn_id FROM chat_pending_turns
               WHERE user_id = ? AND session_id NOT IN ({placeholders})
               ORDER BY created_at ASC, rowid ASC
               LIMIT 1
           )
           RETURNING turn_id, session_id, user_id, model, voice, client_ip, secure, created_at"#
    );
    let mut query = sqlx::query(&sql).bind(user_id);
    for session_id in busy_sessions {
        query = query.bind(session_id);
    }
    let row = query.fetch_optional(pool).await?;
    row.as_ref().map(map_pending).transpose()
}

/// Every user with at least one waiting turn, oldest first.
///
/// What the startup sweep reads: waiting turns survive a restart, and nothing
/// else would ever start them — the browser that sent them may be long closed.
pub async fn users_with_pending_turns(pool: &Pool) -> Result<Vec<String>, DbError> {
    let rows = sqlx::query(
        r#"SELECT user_id, MIN(created_at) AS oldest
           FROM chat_pending_turns
           GROUP BY user_id
           ORDER BY oldest ASC"#,
    )
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|row| row.try_get::<String, _>("user_id").map_err(DbError::from))
        .collect()
}

/// The waiting turns of one conversation, oldest first. Read by the snapshot
/// so the transcript can mark them as waiting rather than unanswered.
pub async fn list_pending_for_session(
    pool: &Pool,
    session_id: &str,
) -> Result<Vec<PendingTurn>, DbError> {
    let rows = sqlx::query(
        r#"SELECT turn_id, session_id, user_id, model, voice, client_ip, secure, created_at
           FROM chat_pending_turns
           WHERE session_id = ?
           ORDER BY created_at ASC, rowid ASC"#,
    )
    .bind(session_id)
    .fetch_all(pool)
    .await?;
    rows.iter().map(map_pending).collect()
}

/// Drop a waiting turn without starting it — the user cancelled it.
///
/// The turn row itself is deleted by the caller through the ordinary
/// turn-removal path; this only takes it out of the work queue.
pub async fn delete_pending_turn(pool: &Pool, turn_id: &str) -> Result<bool, DbError> {
    let done = sqlx::query("DELETE FROM chat_pending_turns WHERE turn_id = ?")
        .bind(turn_id)
        .execute(pool)
        .await?;
    Ok(done.rows_affected() > 0)
}
