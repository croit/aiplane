// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use super::*;

/// Bump `updated_at` so the session floats to the top of the sidebar.
/// Called after a new turn lands.
pub async fn touch_session(pool: &Pool, session_id: &str) -> Result<(), DbError> {
    sqlx::query(r#"UPDATE chat_sessions SET updated_at = ? WHERE id = ?"#)
        .bind(Timestamp::now().to_string())
        .bind(session_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Set the session title. Called once when the first user message lands
/// (auto-title = first user message truncated); the user may overwrite
/// later from the UI.
pub async fn set_session_title(pool: &Pool, session_id: &str, title: &str) -> Result<(), DbError> {
    sqlx::query(r#"UPDATE chat_sessions SET title = ? WHERE id = ?"#)
        .bind(title)
        .bind(session_id)
        .execute(pool)
        .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Turns

pub(crate) async fn next_turn_seq(pool: &Pool, session_id: &str) -> Result<i64, DbError> {
    let row = sqlx::query(
        r#"SELECT COALESCE(MAX(seq), -1) + 1 AS next_seq
           FROM chat_turns
           WHERE session_id = ?"#,
    )
    .bind(session_id)
    .fetch_one(pool)
    .await?;
    Ok(row.try_get("next_seq")?)
}

/// Insert a user-role turn (already 'completed' — the user's message is
/// final the moment we receive it). Returns the new turn.
///
/// `turn_id` is caller-supplied so attachments uploaded under that id's
/// S3 prefix have a stable key the chat-page render-refresh can find
/// later. Pre-generate at the handler entry and pass both here and to
/// the upload step.
pub async fn create_user_turn(
    pool: &Pool,
    session_id: &str,
    turn_id: &str,
    content: &str,
) -> Result<Turn, DbError> {
    let seq = next_turn_seq(pool, session_id).await?;
    let now = Timestamp::now();
    let turn = Turn {
        id: turn_id.to_string(),
        session_id: session_id.to_string(),
        seq,
        role: TurnRole::User,
        user_content: Some(content.to_string()),
        model: None,
        content: None,
        reasoning: None,
        reasoning_elapsed_ms: None,
        reasoning_started_at: None,
        status: TurnStatus::Completed,
        error_message: None,
        error_code: None,
        created_at: now,
        completed_at: Some(now),
    };
    sqlx::query(
        r#"INSERT INTO chat_turns
              (id, session_id, seq, role, user_content, status, created_at, completed_at)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?)"#,
    )
    .bind(&turn.id)
    .bind(&turn.session_id)
    .bind(turn.seq)
    .bind(turn.role.as_str())
    .bind(strip_fts_sentinels(content).as_ref())
    .bind(turn.status.as_str())
    .bind(turn.created_at.to_string())
    .bind(turn.completed_at.map(|t| t.to_string()))
    .execute(pool)
    .await?;
    Ok(turn)
}

/// Insert an assistant-role turn in `in_progress` state. The worker
/// fills in content/reasoning incrementally and calls `finalize_turn`
/// at the end.
///
/// The turn id is supplied by the caller so it can reserve the
/// per-user worker slot under that id *before* persisting anything —
/// see `chat_message_send`. If reservation fails, no DB row is
/// created and the id is simply discarded.
pub async fn create_assistant_turn_in_progress(
    pool: &Pool,
    session_id: &str,
    turn_id: &str,
    model: &str,
) -> Result<Turn, DbError> {
    let seq = next_turn_seq(pool, session_id).await?;
    let now = Timestamp::now();
    let turn = Turn {
        id: turn_id.to_string(),
        session_id: session_id.to_string(),
        seq,
        role: TurnRole::Assistant,
        user_content: None,
        model: Some(model.to_string()),
        content: None,
        reasoning: None,
        reasoning_elapsed_ms: None,
        reasoning_started_at: None,
        status: TurnStatus::InProgress,
        error_message: None,
        error_code: None,
        created_at: now,
        completed_at: None,
    };
    sqlx::query(
        r#"INSERT INTO chat_turns
              (id, session_id, seq, role, model, status, created_at)
           VALUES (?, ?, ?, ?, ?, ?, ?)"#,
    )
    .bind(&turn.id)
    .bind(&turn.session_id)
    .bind(turn.seq)
    .bind(turn.role.as_str())
    .bind(model)
    .bind(turn.status.as_str())
    .bind(turn.created_at.to_string())
    .execute(pool)
    .await?;
    Ok(turn)
}

pub async fn create_turn_pair_if_idle(
    pool: &Pool,
    session_id: &str,
    user_turn_id: &str,
    assistant_turn_id: &str,
    prompt: &str,
    model: &str,
    reject_pending: bool,
) -> Result<Option<(Turn, Turn)>, DbError> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let pair = create_turn_pair_if_idle_in(
        &mut tx,
        session_id,
        user_turn_id,
        assistant_turn_id,
        prompt,
        model,
        reject_pending,
    )
    .await?;
    tx.commit().await?;
    Ok(pair)
}

pub async fn create_turn_pair_if_idle_in(
    connection: &mut sqlx::SqliteConnection,
    session_id: &str,
    user_turn_id: &str,
    assistant_turn_id: &str,
    prompt: &str,
    model: &str,
    reject_pending: bool,
) -> Result<Option<(Turn, Turn)>, DbError> {
    let Some(seq) = idle_turn_sequence(connection, session_id, reject_pending).await? else {
        return Ok(None);
    };
    let now = Timestamp::now().to_string();
    let rows = sqlx::query(
        "INSERT INTO chat_turns
         (id, session_id, seq, role, user_content, model, status, created_at, completed_at)
         VALUES (?, ?, ?, 'user', ?, NULL, 'completed', ?, ?),
                (?, ?, ?, 'assistant', NULL, ?, 'in_progress', ?, NULL)
         RETURNING *",
    )
    .bind(user_turn_id)
    .bind(session_id)
    .bind(seq)
    .bind(strip_fts_sentinels(prompt).as_ref())
    .bind(&now)
    .bind(&now)
    .bind(assistant_turn_id)
    .bind(session_id)
    .bind(seq + 1)
    .bind(model)
    .bind(&now)
    .fetch_all(&mut *connection)
    .await?;
    sqlx::query("UPDATE chat_sessions SET updated_at = ? WHERE id = ?")
        .bind(&now)
        .bind(session_id)
        .execute(connection)
        .await?;
    let mut turns = rows.iter().map(map_turn).collect::<Result<Vec<_>, _>>()?;
    turns.sort_by_key(|turn| turn.seq);
    let assistant = turns.pop().ok_or(sqlx::Error::RowNotFound)?;
    let user = turns.pop().ok_or(sqlx::Error::RowNotFound)?;
    Ok(Some((user, assistant)))
}

pub async fn create_assistant_turn_if_idle(
    pool: &Pool,
    session_id: &str,
    turn_id: &str,
    model: &str,
) -> Result<Option<Turn>, DbError> {
    let Some((mut tx, seq)) = idle_turn_transaction(pool, session_id, false).await? else {
        return Ok(None);
    };
    let now = Timestamp::now().to_string();
    let turn = insert_assistant_turn(&mut tx, session_id, turn_id, model, seq, &now).await?;
    tx.commit().await?;
    Ok(Some(turn))
}

pub async fn regenerate_turn_if_idle(
    pool: &Pool,
    session_id: &str,
    target_turn_id: &str,
    edited_content: Option<&str>,
    assistant_turn_id: &str,
    model: &str,
) -> Result<Option<Turn>, DbError> {
    let Some((mut tx, _)) = idle_turn_transaction(pool, session_id, false).await? else {
        return Ok(None);
    };
    let expected_role = if edited_content.is_some() {
        "user"
    } else {
        "assistant"
    };
    let target_seq: i64 = sqlx::query_scalar(
        "SELECT seq FROM chat_turns WHERE session_id = ? AND id = ? AND role = ?",
    )
    .bind(session_id)
    .bind(target_turn_id)
    .bind(expected_role)
    .fetch_one(&mut *tx)
    .await?;
    let from_seq = target_seq + i64::from(edited_content.is_some());
    if let Some(content) = edited_content {
        sqlx::query("UPDATE chat_turns SET user_content = ? WHERE session_id = ? AND id = ?")
            .bind(strip_fts_sentinels(content).as_ref())
            .bind(session_id)
            .bind(target_turn_id)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("DELETE FROM chat_turns WHERE session_id = ? AND seq >= ?")
        .bind(session_id)
        .bind(from_seq)
        .execute(&mut *tx)
        .await?;
    let seq: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(seq), -1) + 1 FROM chat_turns WHERE session_id = ?",
    )
    .bind(session_id)
    .fetch_one(&mut *tx)
    .await?;
    let now = Timestamp::now().to_string();
    let turn =
        insert_assistant_turn(&mut tx, session_id, assistant_turn_id, model, seq, &now).await?;
    tx.commit().await?;
    Ok(Some(turn))
}

async fn insert_assistant_turn(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    session_id: &str,
    turn_id: &str,
    model: &str,
    seq: i64,
    now: &str,
) -> Result<Turn, DbError> {
    let row = sqlx::query(
        "INSERT INTO chat_turns (id, session_id, seq, role, model, status, created_at)
         VALUES (?, ?, ?, 'assistant', ?, 'in_progress', ?) RETURNING *",
    )
    .bind(turn_id)
    .bind(session_id)
    .bind(seq)
    .bind(model)
    .bind(now)
    .fetch_one(&mut **tx)
    .await?;
    sqlx::query("UPDATE chat_sessions SET updated_at = ? WHERE id = ?")
        .bind(now)
        .bind(session_id)
        .execute(&mut **tx)
        .await?;
    map_turn(&row)
}

async fn idle_turn_transaction(
    pool: &Pool,
    session_id: &str,
    reject_pending: bool,
) -> Result<Option<(sqlx::Transaction<'static, sqlx::Sqlite>, i64)>, DbError> {
    // Reserve the write lock before checking admission or assigning sequence numbers.
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let Some(seq) = idle_turn_sequence(&mut tx, session_id, reject_pending).await? else {
        return Ok(None);
    };
    Ok(Some((tx, seq)))
}

async fn idle_turn_sequence(
    connection: &mut sqlx::SqliteConnection,
    session_id: &str,
    reject_pending: bool,
) -> Result<Option<i64>, DbError> {
    let busy: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM chat_turns WHERE session_id = ?
         AND role = 'assistant' AND status IN ('in_progress', 'suspended'))
         OR (? AND EXISTS(SELECT 1 FROM chat_pending_turns WHERE session_id = ?))",
    )
    .bind(session_id)
    .bind(reject_pending)
    .bind(session_id)
    .fetch_one(&mut *connection)
    .await?;
    if busy {
        return Ok(None);
    }
    let seq: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(seq), -1) + 1 FROM chat_turns WHERE session_id = ?",
    )
    .bind(session_id)
    .fetch_one(connection)
    .await?;
    Ok(Some(seq))
}

/// Append to an in-progress assistant turn's `content`. Worker batches
/// these every ~100ms; SQLite handles the small-string concat fine but
/// we don't want one write per token.
pub async fn append_content(pool: &Pool, turn_id: &str, chunk: &str) -> Result<(), DbError> {
    sqlx::query(
        r#"UPDATE chat_turns
           SET content = COALESCE(content, '') || ?
           WHERE id = ?"#,
    )
    .bind(strip_fts_sentinels(chunk).as_ref())
    .bind(turn_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Overwrite an in-progress assistant turn's `content` wholesale.
/// Unlike [`append_content`], this *replaces* the column — used when a
/// tool needs to rewrite prior markers rather than only add to them
/// (e.g. a typst re-render superseding the earlier render's chip within
/// the same turn). The live view re-renders full turn content from the
/// DB on every tick, so the rewrite is reflected without a delta-accrual
/// mismatch. Pairs with [`get_content`].
pub async fn set_content(pool: &Pool, turn_id: &str, content: &str) -> Result<(), DbError> {
    sqlx::query(
        r#"UPDATE chat_turns
           SET content = ?
           WHERE id = ?"#,
    )
    .bind(strip_fts_sentinels(content).as_ref())
    .bind(turn_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Read the current `content` of a turn — used by tools that need
/// to inspect already-spliced attachment markers (e.g. to pick a
/// non-colliding filename for a same-turn re-upload). Returns `None`
/// for a missing row, `Some(String::new())` for a row whose content
/// is still SQL-NULL.
pub async fn get_content(pool: &Pool, turn_id: &str) -> Result<Option<String>, DbError> {
    let row = sqlx::query(r#"SELECT content FROM chat_turns WHERE id = ?"#)
        .bind(turn_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| {
        r.try_get::<Option<String>, _>("content")
            .ok()
            .flatten()
            .unwrap_or_default()
    }))
}

/// Append to an in-progress assistant turn's `reasoning`. Same
/// batching pattern as `append_content`.
pub async fn append_reasoning(pool: &Pool, turn_id: &str, chunk: &str) -> Result<(), DbError> {
    sqlx::query(
        r#"UPDATE chat_turns
           SET reasoning = COALESCE(reasoning, '') || ?
           WHERE id = ?"#,
    )
    .bind(chunk)
    .bind(turn_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Anchor the reasoning timer. Called once, when the model emits its
/// first reasoning chunk. The client-side `<thinking-timer>` counts up
/// from this instant; a mid-stream reload / late SSE subscriber reads it
/// to resume the count instead of restarting at 0. The `IS NULL` guard
/// keeps it set-once even if the driver calls it more than once.
pub async fn set_reasoning_started(
    pool: &Pool,
    turn_id: &str,
    started_at: Timestamp,
) -> Result<(), DbError> {
    sqlx::query(
        r#"UPDATE chat_turns SET reasoning_started_at = ?
           WHERE id = ? AND reasoning_started_at IS NULL"#,
    )
    .bind(started_at.to_string())
    .bind(turn_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Freeze the reasoning timer at its final duration. Called once, the
/// moment the model emits its first visible content delta (= it has
/// stopped reasoning), and at finalization for a reasoning-only turn.
/// This is the authoritative value the settled "Thought for X.Ys" label
/// renders from.
pub async fn set_reasoning_elapsed(
    pool: &Pool,
    turn_id: &str,
    elapsed_ms: i64,
) -> Result<(), DbError> {
    sqlx::query(r#"UPDATE chat_turns SET reasoning_elapsed_ms = ? WHERE id = ?"#)
        .bind(elapsed_ms)
        .bind(turn_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Record the largest `prompt_tokens` the upstream reported across an
/// assistant turn's rounds — a model-tokenizer-accurate measure of how
/// big the session's replayed context had grown by the end of the turn.
/// The gateway's compaction trigger reads it back via
/// [`latest_context_tokens`]. Column added in migration 0032.
pub async fn set_context_tokens(
    pool: &Pool,
    turn_id: &str,
    context_tokens: i64,
) -> Result<(), DbError> {
    sqlx::query(r#"UPDATE chat_turns SET context_tokens = ? WHERE id = ?"#)
        .bind(context_tokens)
        .bind(turn_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// The most recently measured context size for a session — the
/// `context_tokens` of the latest turn that carried one. `None` when no
/// turn in the session has a measurement yet (e.g. the upstream never
/// reported usage). Used by the gateway's auto-compaction threshold check.
pub async fn latest_context_tokens(pool: &Pool, session_id: &str) -> Result<Option<i64>, DbError> {
    let row = sqlx::query(
        r#"SELECT context_tokens
           FROM chat_turns
           WHERE session_id = ? AND context_tokens IS NOT NULL
           ORDER BY seq DESC
           LIMIT 1"#,
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await?;
    row.map(|r| r.try_get::<i64, _>("context_tokens"))
        .transpose()
        .map_err(Into::into)
}

/// End-of-stream: flip the turn's status and stamp `completed_at`. The
/// worker calls this exactly once per assistant turn whether the turn
/// ended naturally, via cancel, or with an error.
pub async fn finalize_turn(
    pool: &Pool,
    turn_id: &str,
    status: TurnStatus,
    error_message: Option<&str>,
) -> Result<(), DbError> {
    let mut tx = pool.begin().await?;
    finalize_turn_in(&mut tx, turn_id, status, error_message).await?;
    tx.commit().await?;
    Ok(())
}

pub async fn finalize_turn_in(
    connection: &mut sqlx::SqliteConnection,
    turn_id: &str,
    status: TurnStatus,
    error_message: Option<&str>,
) -> Result<(), DbError> {
    if status == TurnStatus::InProgress {
        return Err(DbError::Decode {
            column: "status",
            source: anyhow::anyhow!("finalize_turn called with status=in_progress"),
        });
    }
    sqlx::query(
        r#"UPDATE chat_turns
           SET status = ?, error_message = ?, completed_at = ?
           WHERE id = ?"#,
    )
    .bind(status.as_str())
    .bind(error_message)
    .bind(Timestamp::now().to_string())
    .bind(turn_id)
    .execute(&mut *connection)
    .await?;
    if status == TurnStatus::Cancelled {
        error_running_tool_calls_in(
            connection,
            turn_id,
            &crate::i18n::t(crate::i18n::Lang::En, "chat-error-run-cancelled"),
        )
        .await?;
    }
    Ok(())
}

/// Recover one abandoned assistant turn and any tool calls it left running.
/// Returns whether the turn was still in progress and was changed.
pub async fn error_interrupted_turn(
    pool: &Pool,
    turn_id: &str,
    message: &str,
) -> Result<bool, DbError> {
    let now = Timestamp::now().to_string();
    let mut tx = pool.begin().await?;
    let changed = sqlx::query(
        r#"UPDATE chat_turns
           SET status = 'errored', error_message = ?, completed_at = ?
           WHERE id = ? AND role = 'assistant' AND status = 'in_progress'"#,
    )
    .bind(message)
    .bind(&now)
    .bind(turn_id)
    .execute(&mut *tx)
    .await?
    .rows_affected()
        > 0;
    if changed {
        error_running_tool_calls_in(&mut tx, turn_id, message).await?;
    }
    tx.commit().await?;
    Ok(changed)
}

pub(super) async fn error_running_tool_calls_in(
    connection: &mut sqlx::SqliteConnection,
    turn_id: &str,
    message: &str,
) -> Result<(), DbError> {
    sqlx::query(
        r#"UPDATE chat_tool_calls
           SET status = 'errored', output_json = COALESCE(output_json, ?), completed_at = ?
           WHERE turn_id = ? AND status = 'running'"#,
    )
    .bind(serde_json::Value::String(message.to_owned()).to_string())
    .bind(Timestamp::now().to_string())
    .bind(turn_id)
    .execute(connection)
    .await?;
    Ok(())
}

/// All turns in a session, oldest first, each carrying its tool calls
/// (also oldest first). Used by the renderer for both initial page
/// load and the reconnect-tail path.
pub async fn list_turns(pool: &Pool, session_id: &str) -> Result<Vec<TurnWithTools>, DbError> {
    let turn_rows = sqlx::query(
        r#"SELECT id, session_id, seq, role, user_content, model, content,
                  reasoning, reasoning_elapsed_ms, reasoning_started_at,
                  status, error_message, error_code,
                  created_at, completed_at
           FROM chat_turns
           WHERE session_id = ?
           ORDER BY seq ASC"#,
    )
    .bind(session_id)
    .fetch_all(pool)
    .await?;
    let turns: Vec<Turn> = turn_rows.iter().map(map_turn).collect::<Result<_, _>>()?;
    if turns.is_empty() {
        return Ok(Vec::new());
    }

    // One query for all tool calls in the session, then bucketed by
    // turn_id. Cheaper than N+1.
    let tool_rows = sqlx::query(
        r#"SELECT tc.id, tc.turn_id, tc.seq, tc.name, tc.arguments_json,
                  tc.output_json, tc.status, tc.created_at, tc.completed_at
           FROM chat_tool_calls tc
           JOIN chat_turns t ON t.id = tc.turn_id
           WHERE t.session_id = ?
           ORDER BY tc.turn_id, tc.seq ASC"#,
    )
    .bind(session_id)
    .fetch_all(pool)
    .await?;
    let mut by_turn: std::collections::HashMap<String, Vec<ToolCall>> =
        std::collections::HashMap::new();
    for r in &tool_rows {
        let tc = map_tool_call(r)?;
        by_turn.entry(tc.turn_id.clone()).or_default().push(tc);
    }

    // Same one-query-then-bucket treatment for mid-turn interjections.
    let mut steers_by_turn = crate::db::steers::list_steers_for_session(pool, session_id).await?;
    let mut attempts_by_turn =
        crate::db::attempts::list_attempts_for_session(pool, session_id).await?;
    let mut suspensions =
        crate::db::suspensions::suspension_views_for_session(pool, session_id).await?;

    Ok(turns
        .into_iter()
        .map(|turn| TurnWithTools {
            tool_calls: by_turn.remove(&turn.id).unwrap_or_default(),
            steers: steers_by_turn.remove(&turn.id).unwrap_or_default(),
            attempts: attempts_by_turn.remove(&turn.id).unwrap_or_default(),
            suspension: suspensions.remove(&turn.id),
            turn,
        })
        .collect())
}

/// The single in-flight assistant turn for a session, if any. Used by
/// the tail-subscription handler to decide whether to attach to a
/// running worker.
pub async fn in_flight_turn(pool: &Pool, session_id: &str) -> Result<Option<Turn>, DbError> {
    let row = sqlx::query(
        r#"SELECT id, session_id, seq, role, user_content, model, content,
                  reasoning, reasoning_elapsed_ms, reasoning_started_at,
                  status, error_message, error_code,
                  created_at, completed_at
           FROM chat_turns
           WHERE session_id = ? AND status = 'in_progress'
           ORDER BY seq DESC
           LIMIT 1"#,
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await?;
    row.as_ref().map(map_turn).transpose()
}

/// Fetch one turn by id, scoped to its session. `None` if it doesn't
/// exist or belongs to another session. Used by retry/edit to look up
/// the target turn's `seq` and role before truncating.
pub async fn get_turn(
    pool: &Pool,
    session_id: &str,
    turn_id: &str,
) -> Result<Option<Turn>, DbError> {
    let row = sqlx::query(
        r#"SELECT id, session_id, seq, role, user_content, model, content,
                  reasoning, reasoning_elapsed_ms, reasoning_started_at,
                  status, error_message, error_code,
                  created_at, completed_at
           FROM chat_turns
           WHERE session_id = ? AND id = ?"#,
    )
    .bind(session_id)
    .bind(turn_id)
    .fetch_optional(pool)
    .await?;
    row.as_ref().map(map_turn).transpose()
}

/// The turn right before `seq` in its session — a turn's prompt, without
/// reading the rest of the conversation.
pub async fn turn_before(pool: &Pool, session_id: &str, seq: i64) -> Result<Option<Turn>, DbError> {
    let row = sqlx::query(
        r#"SELECT id, session_id, seq, role, user_content, model, content,
                  reasoning, reasoning_elapsed_ms, reasoning_started_at,
                  status, error_message, error_code,
                  created_at, completed_at
           FROM chat_turns
           WHERE session_id = ? AND seq < ?
           ORDER BY seq DESC
           LIMIT 1"#,
    )
    .bind(session_id)
    .bind(seq)
    .fetch_optional(pool)
    .await?;
    row.as_ref().map(map_turn).transpose()
}

/// Fetch one turn by id together with its tool calls, scoped to its
/// session. This is the per-tick read for the streaming SSE loop: the
/// loop only ever mutates one assistant turn, so re-reading the whole
/// conversation (`list_turns`) per delta was pure overhead — and grew
/// with conversation length on every tick.
pub async fn get_turn_with_tools(
    pool: &Pool,
    session_id: &str,
    turn_id: &str,
) -> Result<Option<TurnWithTools>, DbError> {
    let Some(turn) = get_turn(pool, session_id, turn_id).await? else {
        return Ok(None);
    };
    // The turn's two side tables, read at the same time. This function runs on
    // every coalesced streaming flush, for every attached viewer, so the
    // second read costs a round trip it does not have to: the queries are
    // independent and neither depends on the other's result.
    let (tool_rows, steers, attempts) = tokio::try_join!(
        sqlx::query(
            r#"SELECT id, turn_id, seq, name, arguments_json, output_json,
                      status, created_at, completed_at
               FROM chat_tool_calls
               WHERE turn_id = ?
               ORDER BY seq ASC"#,
        )
        .bind(turn_id)
        .fetch_all(pool),
        crate::db::steers::list_steers_query(pool, turn_id),
        crate::db::attempts::list_attempts_query(pool, turn_id),
    )?;
    let attempts: Vec<TurnAttempt> = attempts
        .iter()
        .map(crate::db::attempts::map_attempt)
        .collect::<Result<_, _>>()?;
    let tool_calls: Vec<ToolCall> = tool_rows
        .iter()
        .map(map_tool_call)
        .collect::<Result<_, _>>()?;
    let steers: Vec<TurnSteer> = steers.iter().map(map_steer).collect::<Result<_, _>>()?;
    // Only a paused turn has a row to read, so the hot streaming path skips it.
    let suspension = if turn.status == TurnStatus::Suspended {
        crate::db::suspensions::get_suspension(pool, turn_id)
            .await?
            .map(|s| s.view())
    } else {
        None
    };
    Ok(Some(TurnWithTools {
        turn,
        tool_calls,
        steers,
        attempts,
        suspension,
    }))
}

/// Replace a user turn's text (the "edit" action). Scoped to the
/// session + `role = 'user'` so it can never rewrite an assistant turn.
/// Returns whether a row was updated.
pub async fn update_user_turn_content(
    pool: &Pool,
    session_id: &str,
    turn_id: &str,
    content: &str,
) -> Result<bool, DbError> {
    let affected = sqlx::query(
        r#"UPDATE chat_turns SET user_content = ?
           WHERE id = ? AND session_id = ? AND role = 'user'"#,
    )
    .bind(strip_fts_sentinels(content).as_ref())
    .bind(turn_id)
    .bind(session_id)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(affected > 0)
}

/// Delete every turn in the session from `from_seq` onwards (inclusive).
/// Backs retry/edit: drop the target turn and everything below it before
/// regenerating. Tool-call rows cascade via the FK. Returns the number
/// of turns removed.
pub async fn delete_turns_from_seq(
    pool: &Pool,
    session_id: &str,
    from_seq: i64,
) -> Result<u64, DbError> {
    let affected = sqlx::query("DELETE FROM chat_turns WHERE session_id = ? AND seq >= ?")
        .bind(session_id)
        .bind(from_seq)
        .execute(pool)
        .await?
        .rows_affected();
    Ok(affected)
}

/// Flip every still-`in_progress` assistant turn to `errored`. Used
/// at startup to evict orphans left behind by a crash / SIGKILL — no
/// worker is going to come back and finish them.
///
/// Returns the number of rows actually touched (useful for a one-shot
/// log line and for the startup test). Idempotent on a clean DB.
pub async fn sweep_in_progress_at_startup(pool: &Pool) -> Result<u64, DbError> {
    let now = Timestamp::now().to_string();
    let affected = sqlx::query(
        r#"UPDATE chat_turns
           SET status = 'errored',
               error_message = COALESCE(error_message,
                                        'Stream interrupted — the server restarted before this response finished.'),
               completed_at = ?
           WHERE status = 'in_progress' AND role = 'assistant'"#,
    )
    .bind(&now)
    .execute(pool)
    .await?
    .rows_affected();
    // The turn sweep above doesn't touch `chat_tool_calls`. A row left at
    // 'running' renders as "Calling" forever even once its turn is errored —
    // the orphaned-tool-call bug. At startup no worker can come back to finish
    // any of them, so flip every still-running row to errored too.
    sqlx::query(
        r#"UPDATE chat_tool_calls
           SET status = 'errored',
               output_json = COALESCE(output_json,
                                      'Tool call interrupted — the server restarted before it finished.'),
               completed_at = ?
           WHERE status = 'running'
             AND turn_id NOT IN (SELECT turn_id FROM chat_turn_suspensions)"#,
    )
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(affected)
}

/// Mark in-progress assistant turns in `session_id` as errored,
/// except for `exempt_turn_id` (the one a live worker is still
/// driving). Called from the render path so a stale orphan from a
/// previous crash / `Busy`-path leak doesn't keep showing the
/// thinking spinner.
///
/// Returns the number of rows flipped.
pub async fn mark_orphaned_in_progress_as_errored(
    pool: &Pool,
    session_id: &str,
    exempt_turn_id: Option<&str>,
) -> Result<u64, DbError> {
    let now = Timestamp::now().to_string();
    let message = "Stream interrupted — no worker is producing this response.";
    let affected = if let Some(exempt) = exempt_turn_id {
        sqlx::query(
            r#"UPDATE chat_turns
               SET status = 'errored',
                   error_message = COALESCE(error_message, ?),
                   completed_at = ?
               WHERE session_id = ?
                 AND status = 'in_progress'
                 AND role = 'assistant'
                 AND id != ?"#,
        )
        .bind(message)
        .bind(&now)
        .bind(session_id)
        .bind(exempt)
        .execute(pool)
        .await?
        .rows_affected()
    } else {
        sqlx::query(
            r#"UPDATE chat_turns
               SET status = 'errored',
                   error_message = COALESCE(error_message, ?),
                   completed_at = ?
               WHERE session_id = ?
                 AND status = 'in_progress'
                 AND role = 'assistant'"#,
        )
        .bind(message)
        .bind(&now)
        .bind(session_id)
        .execute(pool)
        .await?
        .rows_affected()
    };
    // Flip running tool-call rows belonging to the turns we just errored —
    // otherwise they render as "Calling" forever (same orphaned-tool-call bug
    // the startup sweep fixes, but scoped to this session). The live worker's
    // own turn (`exempt`) is preserved so a genuinely in-flight call keeps
    // showing "Calling".
    let tool_msg = "Tool call interrupted — no worker is producing this response.";
    if let Some(exempt) = exempt_turn_id {
        sqlx::query(
            r#"UPDATE chat_tool_calls
               SET status = 'errored',
                   output_json = COALESCE(output_json, ?),
                   completed_at = ?
               WHERE status = 'running'
                 AND turn_id IN (
                     SELECT id FROM chat_turns
                     WHERE session_id = ? AND role = 'assistant' AND id != ?
                       AND status != 'suspended'
                 )"#,
        )
        .bind(tool_msg)
        .bind(&now)
        .bind(session_id)
        .bind(exempt)
        .execute(pool)
        .await?;
    } else {
        sqlx::query(
            r#"UPDATE chat_tool_calls
               SET status = 'errored',
                   output_json = COALESCE(output_json, ?),
                   completed_at = ?
               WHERE status = 'running'
                 AND turn_id IN (
                     SELECT id FROM chat_turns
                     WHERE session_id = ? AND role = 'assistant'
                       AND status != 'suspended'
                 )"#,
        )
        .bind(tool_msg)
        .bind(&now)
        .bind(session_id)
        .execute(pool)
        .await?;
    }
    Ok(affected)
}

// ---------------------------------------------------------------------------
// Tool calls

#[cfg(test)]
mod pair_tests {
    use super::*;

    #[tokio::test]
    async fn idle_pair_persists_exact_ids_content_model_and_adjacent_sequence() {
        let pool = super::super::tests::pool().await;
        let session = create_session(&pool, "u1").await.unwrap();
        create_user_turn(&pool, &session.id, "earlier", "history")
            .await
            .unwrap();
        let (user, assistant) = create_turn_pair_if_idle(
            &pool,
            &session.id,
            "prompt",
            "reply",
            "Review updates",
            "model",
            false,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!((user.seq, assistant.seq), (1, 2));
        assert_eq!(user.user_content.as_deref(), Some("Review updates"));
        assert_eq!(user.status, TurnStatus::Completed);
        assert_eq!(assistant.model.as_deref(), Some("model"));
        assert_eq!(assistant.status, TurnStatus::InProgress);
        let stored = list_turns(&pool, &session.id).await.unwrap();
        assert_eq!(stored.len(), 3);
        assert_eq!(stored[1].turn.id, "prompt");
        assert_eq!(stored[2].turn.id, "reply");
    }

    #[tokio::test]
    async fn regeneration_refuses_active_and_suspended_turns_without_changing_history() {
        let pool = super::super::tests::pool().await;
        let session = create_session(&pool, "u1").await.unwrap();
        create_turn_pair_if_idle(
            &pool,
            &session.id,
            "prompt",
            "reply",
            "Original",
            "model",
            false,
        )
        .await
        .unwrap()
        .unwrap();
        for status in [TurnStatus::InProgress, TurnStatus::Suspended] {
            sqlx::query("UPDATE chat_turns SET status = ? WHERE id = 'reply'")
                .bind(status.as_str())
                .execute(&pool)
                .await
                .unwrap();
            for (target, content) in [("prompt", Some("Changed")), ("reply", None)] {
                assert!(
                    regenerate_turn_if_idle(&pool, &session.id, target, content, "new", "other")
                        .await
                        .unwrap()
                        .is_none()
                );
            }
            let turns = list_turns(&pool, &session.id).await.unwrap();
            assert_eq!(turns.len(), 2);
            assert_eq!(turns[0].turn.user_content.as_deref(), Some("Original"));
            assert_eq!(turns[1].turn.status, status);
        }
    }

    #[tokio::test]
    async fn regeneration_atomically_edits_or_retries_the_persisted_target() {
        let pool = super::super::tests::pool().await;
        let session = create_session(&pool, "u1").await.unwrap();
        create_turn_pair_if_idle(
            &pool,
            &session.id,
            "prompt",
            "reply",
            "Original",
            "model",
            false,
        )
        .await
        .unwrap()
        .unwrap();
        finalize_turn(&pool, "reply", TurnStatus::Completed, None)
            .await
            .unwrap();
        let edited = regenerate_turn_if_idle(
            &pool,
            &session.id,
            "prompt",
            Some("Changed"),
            "edited",
            "other",
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(edited.seq, 1);
        assert_eq!(edited.model.as_deref(), Some("other"));
        assert_eq!(edited.status, TurnStatus::InProgress);
        assert!(
            get_turn(&pool, &session.id, "reply")
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            get_turn(&pool, &session.id, "prompt")
                .await
                .unwrap()
                .unwrap()
                .user_content
                .as_deref(),
            Some("Changed")
        );
        finalize_turn(&pool, "edited", TurnStatus::Completed, None)
            .await
            .unwrap();
        let retried =
            regenerate_turn_if_idle(&pool, &session.id, "edited", None, "retried", "model")
                .await
                .unwrap()
                .unwrap();
        assert_eq!(retried.seq, 1);
        assert_eq!(list_turns(&pool, &session.id).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn regeneration_insert_failure_rolls_back_edits_and_removed_answers() {
        let pool = super::super::tests::pool().await;
        let session = create_session(&pool, "u1").await.unwrap();
        create_turn_pair_if_idle(
            &pool,
            &session.id,
            "prompt",
            "reply",
            "Original",
            "model",
            false,
        )
        .await
        .unwrap()
        .unwrap();
        finalize_turn(&pool, "reply", TurnStatus::Completed, None)
            .await
            .unwrap();
        assert!(
            regenerate_turn_if_idle(
                &pool,
                &session.id,
                "prompt",
                Some("Changed"),
                "prompt",
                "model"
            )
            .await
            .is_err()
        );
        let turns = list_turns(&pool, &session.id).await.unwrap();
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].turn.user_content.as_deref(), Some("Original"));
        assert_eq!(turns[1].turn.id, "reply");
        assert_eq!(turns[1].turn.status, TurnStatus::Completed);
        assert!(
            regenerate_turn_if_idle(
                &pool,
                &session.id,
                "reply",
                Some("Wrong role"),
                "new",
                "model"
            )
            .await
            .is_err()
        );
        assert!(
            regenerate_turn_if_idle(&pool, &session.id, "missing", None, "new", "model")
                .await
                .is_err()
        );
        assert_eq!(list_turns(&pool, &session.id).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn regeneration_and_background_opening_cannot_overwrite_each_other() {
        let pool = super::super::tests::pool().await;
        let session = create_session(&pool, "u1").await.unwrap();
        create_turn_pair_if_idle(
            &pool,
            &session.id,
            "prompt",
            "reply",
            "Original",
            "model",
            false,
        )
        .await
        .unwrap()
        .unwrap();
        finalize_turn(&pool, "reply", TurnStatus::Completed, None)
            .await
            .unwrap();
        let (edited, background) = tokio::join!(
            regenerate_turn_if_idle(
                &pool,
                &session.id,
                "prompt",
                Some("Changed"),
                "edited",
                "model"
            ),
            create_turn_pair_if_idle(
                &pool,
                &session.id,
                "work",
                "background",
                "Background",
                "model",
                true
            ),
        );
        let edited = edited.unwrap().is_some();
        let background = background.unwrap().is_some();
        assert_eq!(usize::from(edited) + usize::from(background), 1);
        let turns = list_turns(&pool, &session.id).await.unwrap();
        assert_eq!(turns.len(), if edited { 2 } else { 4 });
        assert_eq!(
            turns[0].turn.user_content.as_deref(),
            Some(if edited { "Changed" } else { "Original" })
        );
        assert_eq!(
            turns
                .iter()
                .filter(|turn| turn.turn.status == TurnStatus::InProgress)
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn background_pair_does_not_overtake_waiting_user_messages() {
        let pool = super::super::tests::pool().await;
        let session = create_session(&pool, "u1").await.unwrap();
        create_user_turn(&pool, &session.id, "queued", "Earlier request")
            .await
            .unwrap();
        super::super::insert_pending_turn(
            &pool,
            &super::super::PendingTurn {
                turn_id: "queued".into(),
                session_id: session.id.clone(),
                user_id: "u1".into(),
                model: "model".into(),
                voice: false,
                client_ip: None,
                secure: false,
                created_at: Timestamp::now(),
            },
        )
        .await
        .unwrap();
        assert!(
            create_turn_pair_if_idle(
                &pool,
                &session.id,
                "prompt",
                "reply",
                "Background work",
                "model",
                true
            )
            .await
            .unwrap()
            .is_none()
        );
        assert_eq!(list_turns(&pool, &session.id).await.unwrap().len(), 1);
        assert!(
            create_assistant_turn_if_idle(&pool, &session.id, "queued-reply", "model")
                .await
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn running_and_suspended_sessions_refuse_a_pair_without_orphan_prompt() {
        let pool = super::super::tests::pool().await;
        let session = create_session(&pool, "u1").await.unwrap();
        create_assistant_turn_in_progress(&pool, &session.id, "busy", "model")
            .await
            .unwrap();
        for status in [TurnStatus::InProgress, TurnStatus::Suspended] {
            sqlx::query("UPDATE chat_turns SET status = ? WHERE id = 'busy'")
                .bind(status.as_str())
                .execute(&pool)
                .await
                .unwrap();
            assert!(
                create_turn_pair_if_idle(
                    &pool,
                    &session.id,
                    "prompt",
                    "reply",
                    "work",
                    "model",
                    false
                )
                .await
                .unwrap()
                .is_none()
            );
            assert_eq!(list_turns(&pool, &session.id).await.unwrap().len(), 1);
        }
        finalize_turn(&pool, "busy", TurnStatus::Completed, None)
            .await
            .unwrap();
        assert!(
            create_turn_pair_if_idle(
                &pool,
                &session.id,
                "prompt",
                "reply",
                "work",
                "model",
                false
            )
            .await
            .unwrap()
            .is_some()
        );
    }

    #[tokio::test]
    async fn failed_second_insert_rolls_back_the_prompt_and_sequence() {
        let pool = super::super::tests::pool().await;
        let session = create_session(&pool, "u1").await.unwrap();
        assert!(
            create_turn_pair_if_idle(&pool, &session.id, "same", "same", "work", "model", false)
                .await
                .is_err()
        );
        assert!(list_turns(&pool, &session.id).await.unwrap().is_empty());
        let (user, assistant) = create_turn_pair_if_idle(
            &pool,
            &session.id,
            "prompt",
            "reply",
            "work",
            "model",
            false,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!((user.seq, assistant.seq), (0, 1));
    }

    #[tokio::test]
    async fn competing_openers_admit_exactly_one_pair_in_a_session() {
        let pool = super::super::tests::pool().await;
        let session = create_session(&pool, "u1").await.unwrap();
        let (first, second) = tokio::join!(
            create_turn_pair_if_idle(&pool, &session.id, "p1", "a1", "one", "model", false),
            create_turn_pair_if_idle(&pool, &session.id, "p2", "a2", "two", "model", false),
        );
        assert_eq!(
            usize::from(first.unwrap().is_some()) + usize::from(second.unwrap().is_some()),
            1
        );
        assert_eq!(list_turns(&pool, &session.id).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn assistant_admission_and_pair_admission_share_the_same_busy_boundary() {
        let pool = super::super::tests::pool().await;
        let session = create_session(&pool, "u1").await.unwrap();
        create_user_turn(&pool, &session.id, "existing", "work")
            .await
            .unwrap();
        let assistant = create_assistant_turn_if_idle(&pool, &session.id, "reply", "model")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(assistant.seq, 1);
        assert_eq!(assistant.status, TurnStatus::InProgress);
        assert!(
            create_assistant_turn_if_idle(&pool, &session.id, "other", "model")
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            create_turn_pair_if_idle(
                &pool,
                &session.id,
                "prompt",
                "response",
                "work",
                "model",
                false
            )
            .await
            .unwrap()
            .is_none()
        );
        finalize_turn(&pool, "reply", TurnStatus::Suspended, None)
            .await
            .unwrap();
        assert!(
            create_assistant_turn_if_idle(&pool, &session.id, "other", "model")
                .await
                .unwrap()
                .is_none()
        );
        finalize_turn(&pool, "reply", TurnStatus::Completed, None)
            .await
            .unwrap();
        assert!(
            create_assistant_turn_if_idle(&pool, &session.id, "other", "model")
                .await
                .unwrap()
                .is_some()
        );
    }
}
