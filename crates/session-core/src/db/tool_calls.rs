// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use super::*;

/// Insert a running tool call row. The model emits `id`,
/// `function.name`, and `function.arguments` (potentially across
/// multiple stream deltas) — the worker assembles those server-side
/// and inserts when ready to dispatch the tool.
///
/// The sequence is computed *inside* the INSERT: reading `MAX(seq)` first
/// and inserting second leaves a gap in which a concurrent call for the same
/// turn reads the same number and dies on `UNIQUE (turn_id, seq)`. The same
/// shape as `steers::insert_steer`.
pub async fn insert_running_tool_call(
    pool: &Pool,
    turn_id: &str,
    id: &str,
    name: &str,
    arguments_json: &str,
) -> Result<ToolCall, DbError> {
    let now = Timestamp::now();
    let row = sqlx::query(
        r#"INSERT INTO chat_tool_calls
              (id, turn_id, seq, name, arguments_json, status, created_at)
           SELECT ?, ?, COALESCE(MAX(seq), -1) + 1, ?, ?, ?, ?
             FROM chat_tool_calls WHERE turn_id = ?
           RETURNING seq"#,
    )
    .bind(id)
    .bind(turn_id)
    .bind(name)
    .bind(arguments_json)
    .bind(ToolCallStatus::Running.as_str())
    .bind(now.to_string())
    .bind(turn_id)
    .fetch_one(pool)
    .await?;
    let seq: i64 = row.try_get("seq")?;
    Ok(ToolCall {
        id: id.to_string(),
        turn_id: turn_id.to_string(),
        seq,
        name: name.to_string(),
        arguments_json: arguments_json.to_string(),
        output_json: None,
        status: ToolCallStatus::Running,
        created_at: now,
        completed_at: None,
    })
}

/// Max bytes of `output_json` we persist per tool call. `fetch_url`
/// can hand us a 4 MB text body; storing that verbatim in SQLite
/// per call bloats the DB fast and balloons the rows pulled back on
/// every chat-history list. The MODEL already saw the full payload
/// in the turn loop and synthesised its response off it — and the
/// openai_driver history replay rebuilds the upstream message list
/// from `chat_turns.content` (the assistant's prose), NOT from past
/// tool-call rows. So a cap here is invisible to upstream LLM
/// correctness; only the UI + audit log are affected, and both are
/// fine with a head + a "truncated" note.
///
/// 16 KB keeps a row cheap to pull back and enough to read a result's shape.
/// A JSON object keeps all its fields and only its largest string value is
/// shortened, so the facts a reader needs survive; plain text keeps its head.
pub(crate) const PERSISTED_TOOL_OUTPUT_CAP: usize = 16 * 1024;

/// Cap `raw` to the persistence ceiling without splitting a UTF-8
/// codepoint. Strings under the cap pass through unchanged.
///
/// A JSON object is capped by shortening its largest string value (see
/// [`cap_json_object`]); anything else gets its head kept. The difference
/// matters because object keys serialize in alphabetical order: `content`
/// sorts before `url` and nearly every other field, so cutting the text keeps
/// a page's body and throws away where it came from.
pub(crate) fn cap_tool_output(raw: &str) -> std::borrow::Cow<'_, str> {
    if raw.len() <= PERSISTED_TOOL_OUTPUT_CAP {
        return std::borrow::Cow::Borrowed(raw);
    }
    if let Some(capped) = cap_json_object(raw) {
        return std::borrow::Cow::Owned(capped);
    }
    let head_end = raw
        .char_indices()
        .take_while(|(i, _)| *i <= PERSISTED_TOOL_OUTPUT_CAP)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(PERSISTED_TOOL_OUTPUT_CAP);
    let mut out = String::with_capacity(head_end + 128);
    out.push_str(&raw[..head_end]);
    out.push_str(&truncation_note(raw.len(), head_end));
    std::borrow::Cow::Owned(out)
}

/// What a shortened field says about itself, so neither a person nor a model
/// reads a slice as the whole answer.
fn truncation_note(original: usize, shown: usize) -> String {
    format!(
        "\n\n…\n(truncated by gateway at persist time: original {} bytes, \
         displayed first {} chars; the model saw the full payload before this \
         row was written)\n",
        original, shown,
    )
}

/// Cap a JSON object by shortening its largest string values, keeping every
/// other field.
///
/// A tool result is a record: `fetch_url` returns the body *plus* the URL it
/// came from, the status, the content type and whether it was truncated. All
/// of those are a few dozen bytes and all of them are lost when the serialized
/// text is cut at 16 KB behind a multi-megabyte `content` — the expanded tool
/// call in the chat then shows a page with no address. Shortening the value
/// instead keeps the result valid JSON with its facts intact; the model saw
/// the full payload when the call ran, and this row is the record of it.
///
/// `None` when the payload is not an object, when no string field is worth
/// shortening, or when the object is over the cap for reasons a string cannot
/// fix — the caller then keeps the head, as before.
fn cap_json_object(raw: &str) -> Option<String> {
    let mut value: serde_json::Value = serde_json::from_str(raw).ok()?;
    let fields = value.as_object()?.len();
    let mut shortened = false;
    // One check more than there are fields: every pass shortens one field, and
    // the last pass only has to confirm the result fits.
    for _ in 0..=fields {
        let serialized = serde_json::to_string(&value).ok()?;
        if serialized.len() <= PERSISTED_TOOL_OUTPUT_CAP {
            return shortened.then_some(serialized);
        }
        let over = serialized.len() - PERSISTED_TOOL_OUTPUT_CAP;
        let (key, len) = value
            .as_object()?
            .iter()
            .filter_map(|(key, value)| value.as_str().map(|text| (key.clone(), text.len())))
            .max_by_key(|(_, len)| *len)?;
        // Leave room for the note itself plus what serializing it adds (the
        // note's newlines become two bytes each), or the next pass would trim
        // the same field again and lose the size it reported.
        let keep = len.checked_sub(over + truncation_note(len, len).len() + 32)?;
        if keep == 0 {
            return None;
        }
        let text = value.as_object()?.get(&key)?.as_str()?;
        let mut end = keep;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let field = format!("{}{}", &text[..end], truncation_note(len, end));
        value
            .as_object_mut()?
            .insert(key, serde_json::Value::String(field));
        shortened = true;
    }
    None
}

/// Stamp a tool call's output + flip status. Idempotent on the same
/// (id, output) pair: worker calls it exactly once per tool result.
pub async fn complete_tool_call(
    pool: &Pool,
    turn_id: &str,
    id: &str,
    output_json: &str,
    status: ToolCallStatus,
) -> Result<(), DbError> {
    if status == ToolCallStatus::Running {
        return Err(DbError::Decode {
            column: "status",
            source: anyhow::anyhow!("complete_tool_call called with status=running"),
        });
    }
    let capped = cap_tool_output(output_json);
    // Key on the full (turn_id, id) identity — `id` alone is only unique
    // within its turn.
    sqlx::query(
        r#"UPDATE chat_tool_calls
           SET output_json = ?, status = ?, completed_at = ?
           WHERE turn_id = ? AND id = ?"#,
    )
    .bind(capped.as_ref())
    .bind(status.as_str())
    .bind(Timestamp::now().to_string())
    .bind(turn_id)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
