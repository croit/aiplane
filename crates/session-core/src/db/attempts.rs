// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Model calls of an assistant turn that looped and were retried.
//!
//! When the gateway stops a call that collapsed into repeating itself and
//! tries again at a lower effort, that call's partial output must neither stay
//! in the answer nor reach the model again — but it happened, and the reader
//! is shown it, collapsed, above the answer. [`stash_attempt`] moves the
//! call's text out of the turn row into `chat_turn_attempts` in one
//! transaction, so a viewer never sees it in both places or in neither.

use super::*;

/// One retried call, as stored.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
pub struct TurnAttempt {
    pub seq: i64,
    /// The effort level the call ran at.
    pub effort: String,
    /// The level the next try ran at; `None` on the last attempt of a turn
    /// that gave up.
    pub retry_effort: Option<String>,
    /// Why it was stopped: `loop` or `repeated_call`.
    pub stop_reason: String,
    pub reasoning: String,
    pub content: String,
    pub created_at: Timestamp,
}

/// What [`stash_attempt`] records about the call it moves.
#[derive(Debug, Clone, Copy)]
pub struct AttemptStop<'a> {
    pub effort: &'a str,
    pub retry_effort: Option<&'a str>,
    pub stop_reason: &'a str,
}

/// How much text a turn holds, in characters — the mark a call's output
/// starts at, for [`stash_attempt`] to cut back to.
pub async fn text_marks(pool: &Pool, turn_id: &str) -> Result<(i64, i64), DbError> {
    let row = sqlx::query(
        r#"SELECT COALESCE(length(content), 0) AS content_chars,
                  COALESCE(length(reasoning), 0) AS reasoning_chars
           FROM chat_turns WHERE id = ?"#,
    )
    .bind(turn_id)
    .fetch_optional(pool)
    .await?;
    Ok(match row {
        Some(r) => (r.try_get("content_chars")?, r.try_get("reasoning_chars")?),
        None => (0, 0),
    })
}

/// Move everything the turn gained past `marks` (from [`text_marks`]) into a
/// new attempt row, and cut the turn back to the marks.
///
/// Character offsets on both sides — SQLite's `length` and `substr` count
/// characters of TEXT — so a cut never lands inside a multi-byte character.
pub async fn stash_attempt(
    pool: &Pool,
    turn_id: &str,
    marks: (i64, i64),
    stop: AttemptStop<'_>,
) -> Result<TurnAttempt, DbError> {
    let (content_mark, reasoning_mark) = marks;
    let created_at = Timestamp::now();
    let mut tx = pool.begin().await?;
    let row = sqlx::query(
        r#"INSERT INTO chat_turn_attempts
               (turn_id, seq, effort, retry_effort, stop_reason, reasoning, content, created_at)
           SELECT t.id,
                  (SELECT COALESCE(MAX(a.seq), -1) + 1 FROM chat_turn_attempts a
                    WHERE a.turn_id = t.id),
                  ?, ?, ?,
                  COALESCE(substr(t.reasoning, ? + 1), ''),
                  COALESCE(substr(t.content, ? + 1), ''),
                  ?
           FROM chat_turns t WHERE t.id = ?
           RETURNING seq, reasoning, content"#,
    )
    .bind(stop.effort)
    .bind(stop.retry_effort)
    .bind(stop.stop_reason)
    .bind(reasoning_mark)
    .bind(content_mark)
    .bind(created_at.to_string())
    .bind(turn_id)
    .fetch_one(&mut *tx)
    .await?;
    // NULLIF keeps a turn that had no text before the call at NULL, which is
    // what "nothing streamed yet" looks like everywhere else.
    sqlx::query(
        r#"UPDATE chat_turns
           SET content = NULLIF(substr(content, 1, ?), ''),
               reasoning = NULLIF(substr(reasoning, 1, ?), '')
           WHERE id = ?"#,
    )
    .bind(content_mark)
    .bind(reasoning_mark)
    .bind(turn_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(TurnAttempt {
        seq: row.try_get("seq")?,
        effort: stop.effort.to_string(),
        retry_effort: stop.retry_effort.map(str::to_string),
        stop_reason: stop.stop_reason.to_string(),
        reasoning: row.try_get("reasoning")?,
        content: row.try_get("content")?,
        created_at,
    })
}

/// Stamp the machine-readable reason a turn is about to fail with
/// ([`Turn::error_code`]). The worker writes the message when it finalizes.
pub async fn set_error_code(pool: &Pool, turn_id: &str, code: &str) -> Result<(), DbError> {
    sqlx::query("UPDATE chat_turns SET error_code = ? WHERE id = ?")
        .bind(code)
        .bind(turn_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Copy an attempt under another turn (a fork).
pub(crate) async fn insert_attempt(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    turn_id: &str,
    attempt: &TurnAttempt,
) -> Result<(), DbError> {
    sqlx::query(
        r#"INSERT INTO chat_turn_attempts
               (turn_id, seq, effort, retry_effort, stop_reason, reasoning, content, created_at)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?)"#,
    )
    .bind(turn_id)
    .bind(attempt.seq)
    .bind(&attempt.effort)
    .bind(attempt.retry_effort.as_deref())
    .bind(&attempt.stop_reason)
    .bind(&attempt.reasoning)
    .bind(&attempt.content)
    .bind(attempt.created_at.to_string())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub(crate) fn map_attempt(row: &SqliteRow) -> Result<TurnAttempt, DbError> {
    Ok(TurnAttempt {
        seq: row.try_get("seq")?,
        effort: row.try_get("effort")?,
        retry_effort: row.try_get("retry_effort")?,
        stop_reason: row.try_get("stop_reason")?,
        reasoning: row.try_get("reasoning")?,
        content: row.try_get("content")?,
        created_at: parse_ts(row.try_get("created_at")?, "created_at")?,
    })
}

/// One turn's attempts, oldest first.
pub async fn list_attempts(pool: &Pool, turn_id: &str) -> Result<Vec<TurnAttempt>, DbError> {
    let rows = list_attempts_query(pool, turn_id).await?;
    rows.iter().map(map_attempt).collect()
}

/// The raw rows behind [`list_attempts`], so the per-tick turn read can run it
/// beside its other side-table queries.
pub(crate) async fn list_attempts_query(
    pool: &Pool,
    turn_id: &str,
) -> Result<Vec<SqliteRow>, sqlx::Error> {
    sqlx::query(
        r#"SELECT seq, effort, retry_effort, stop_reason, reasoning, content, created_at
           FROM chat_turn_attempts WHERE turn_id = ? ORDER BY seq"#,
    )
    .bind(turn_id)
    .fetch_all(pool)
    .await
}

/// Every attempt of a session, bucketed by turn — one query for a whole
/// transcript, like the tool calls and interjections.
pub(crate) async fn list_attempts_for_session(
    pool: &Pool,
    session_id: &str,
) -> Result<std::collections::HashMap<String, Vec<TurnAttempt>>, DbError> {
    let rows = sqlx::query(
        r#"SELECT a.turn_id, a.seq, a.effort, a.retry_effort, a.stop_reason,
                  a.reasoning, a.content, a.created_at
           FROM chat_turn_attempts a
           JOIN chat_turns t ON t.id = a.turn_id
           WHERE t.session_id = ?
           ORDER BY a.turn_id, a.seq"#,
    )
    .bind(session_id)
    .fetch_all(pool)
    .await?;
    let mut by_turn: std::collections::HashMap<String, Vec<TurnAttempt>> =
        std::collections::HashMap::new();
    for row in &rows {
        let turn_id: String = row.try_get("turn_id")?;
        by_turn.entry(turn_id).or_default().push(map_attempt(row)?);
    }
    Ok(by_turn)
}
