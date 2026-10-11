// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Inbound webhooks: per-user prompts that run when an external caller POSTs
//! to a secret trigger URL. The event-driven twin of [`scheduled`] — instead
//! of a cron tick, an inbound request to `/hooks/{secret}` fires the run.
//!
//! This module owns the persistence (`webhooks` table + CRUD, created by
//! migration `0037_webhooks.sql`). The trigger endpoint and the management UI
//! live in `rama_server::pages::webhooks`; the actual model run is driven by
//! the shared [`crate::server::headless`] helper (the same engine scheduled
//! actions and `/chat` use).
//!
//! The trigger secret is a `gwh_<64 hex>` string minted by
//! [`aiplane_core::server::auth::token::mint_webhook`]; only its SHA-256 hash is
//! stored here, so the plaintext URL exists only in the owner's hands.
//!
//! [`scheduled`]: crate::server::scheduled

use super::scheduled::DeleteOutcome;
use aiplane_agents::db::{WriteTx, dot_sessions};
use jiff::Timestamp;
use sqlx::Row;
use sqlx::sqlite::SqliteRow;
use uuid::Uuid;

use aiplane_core::server::db::{DbError, Pool, parse_optional_ts, parse_ts};

/// A persisted webhook.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Webhook {
    pub id: String,
    pub user_id: String,
    pub dot_id: Option<String>,
    pub name: String,
    pub prompt: String,
    pub model: String,
    /// `true` = the fire runs with the owner's normal tools; `false` (the
    /// default) = no tools, since an anonymous caller triggers the run.
    pub tools_enabled: bool,
    /// `true` = the caller waits and gets the model output in the HTTP
    /// response; `false` = fire-and-forget (respond 202, run in background).
    pub synchronous: bool,
    /// SHA-256 hex of the `gwh_` trigger secret. The plaintext is shown to the
    /// owner only on create/rotate.
    pub secret_hash: String,
    /// `false` = paused; the trigger 404s and never fires.
    pub enabled: bool,
    pub last_fired_at: Option<Timestamp>,
    /// `"ok"` or `"error"` — the outcome of the most recent fire.
    pub last_status: Option<String>,
    /// Chat session opened by the most recent fire, for the "open" link.
    pub last_session_id: Option<String>,
    pub last_error: Option<String>,
    /// Raw body of the most recent fire, kept so the owner can rerun it with a
    /// different prompt (see [`set_last_payload`] and the rerun handlers).
    /// `None` until the webhook has fired at least once.
    pub last_payload: Option<String>,
    /// `true` = each live fire appends into the previous fire's chat (so the
    /// model sees prior fires as history); `false` (default) = each fire opens
    /// a fresh chat. Reruns are always fresh regardless.
    pub reuse_conversation: bool,
    /// When reusing, how many recent rounds (one round = prompt + reply = 2
    /// turns) of history to replay — caps unbounded growth.
    pub reuse_rounds: i64,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

/// The validated, ready-to-insert fields for a new webhook. The caller mints
/// the secret and passes only its hash.
pub struct NewWebhook {
    pub user_id: String,
    pub dot_id: Option<String>,
    pub name: String,
    pub prompt: String,
    pub model: String,
    pub tools_enabled: bool,
    pub synchronous: bool,
    pub reuse_conversation: bool,
    pub reuse_rounds: i64,
    pub secret_hash: String,
    pub linked_session_id: Option<String>,
}

/// The mutable fields of an existing webhook, as submitted by the edit form.
/// The secret is rotated separately via [`rotate_secret`].
pub struct EditWebhook {
    pub dot_id: Option<String>,
    pub name: String,
    pub prompt: String,
    pub model: String,
    pub tools_enabled: bool,
    pub synchronous: bool,
    pub reuse_conversation: bool,
    pub reuse_rounds: i64,
    pub linked_session_id: Option<Option<String>>,
}

fn map_row(row: &SqliteRow) -> Result<Webhook, DbError> {
    Ok(Webhook {
        id: row.try_get("id")?,
        user_id: row.try_get("user_id")?,
        dot_id: row.try_get("dot_id")?,
        name: row.try_get("name")?,
        prompt: row.try_get("prompt")?,
        model: row.try_get("model")?,
        tools_enabled: row.try_get::<i64, _>("tools_enabled")? != 0,
        synchronous: row.try_get::<i64, _>("synchronous")? != 0,
        secret_hash: row.try_get("secret_hash")?,
        enabled: row.try_get::<i64, _>("enabled")? != 0,
        last_fired_at: parse_optional_ts(row.try_get("last_fired_at")?, "last_fired_at")?,
        last_status: row.try_get("last_status")?,
        last_session_id: row.try_get("last_session_id")?,
        last_error: row.try_get("last_error")?,
        last_payload: row.try_get("last_payload")?,
        reuse_conversation: row.try_get::<i64, _>("reuse_conversation")? != 0,
        reuse_rounds: row.try_get("reuse_rounds")?,
        created_at: parse_ts(row.try_get("created_at")?, "created_at")?,
        updated_at: parse_ts(row.try_get("updated_at")?, "updated_at")?,
    })
}

const COLS: &str = "id, user_id, dot_id, name, prompt, model, tools_enabled, synchronous, secret_hash, \
     enabled, last_fired_at, last_status, last_session_id, last_error, last_payload, \
     reuse_conversation, reuse_rounds, created_at, updated_at";

/// Insert a new webhook. Returns the stored row.
pub async fn create(pool: &Pool, new: NewWebhook) -> Result<Webhook, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    if !dot_sessions::binding_valid(&mut tx, &new.user_id, new.dot_id.as_deref(), None).await? {
        return Err(
            sqlx::Error::Protocol("The selected Dot is not owned and active".into()).into(),
        );
    }
    if let Some(session_id) = new.linked_session_id.as_deref()
        && (!new.reuse_conversation
            || !dot_sessions::session_matches(
                &mut tx,
                &new.user_id,
                session_id,
                new.dot_id.as_deref(),
            )
            .await?)
    {
        return Err(sqlx::Error::Protocol(
            "The linked conversation does not match this automation".into(),
        )
        .into());
    }
    let now = Timestamp::now();
    let row = Webhook {
        id: Uuid::new_v4().to_string(),
        user_id: new.user_id,
        dot_id: new.dot_id,
        name: new.name,
        prompt: new.prompt,
        model: new.model,
        tools_enabled: new.tools_enabled,
        synchronous: new.synchronous,
        secret_hash: new.secret_hash,
        enabled: true,
        last_fired_at: None,
        last_status: None,
        last_session_id: new.linked_session_id,
        last_error: None,
        last_payload: None,
        reuse_conversation: new.reuse_conversation,
        reuse_rounds: new.reuse_rounds,
        created_at: now,
        updated_at: now,
    };
    sqlx::query(
        r#"INSERT INTO webhooks
              (id, user_id, dot_id, name, prompt, model, tools_enabled, synchronous, secret_hash,
               enabled, reuse_conversation, reuse_rounds, created_at, updated_at)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, ?)"#,
    )
    .bind(&row.id)
    .bind(&row.user_id)
    .bind(&row.dot_id)
    .bind(&row.name)
    .bind(&row.prompt)
    .bind(&row.model)
    .bind(row.tools_enabled as i64)
    .bind(row.synchronous as i64)
    .bind(&row.secret_hash)
    .bind(row.reuse_conversation as i64)
    .bind(row.reuse_rounds)
    .bind(row.created_at.to_string())
    .bind(row.updated_at.to_string())
    .execute(&mut *tx)
    .await?;
    if row.last_session_id.is_some() {
        sqlx::query("UPDATE webhooks SET last_session_id = ? WHERE id = ?")
            .bind(&row.last_session_id)
            .bind(&row.id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(row)
}

/// All of a user's webhooks, newest first.
pub async fn list_for_user(pool: &Pool, user_id: &str) -> Result<Vec<Webhook>, DbError> {
    let sql =
        format!("SELECT {COLS} FROM webhooks WHERE user_id = ? ORDER BY created_at DESC, id ASC");
    let rows = sqlx::query(&sql).bind(user_id).fetch_all(pool).await?;
    rows.iter().map(map_row).collect()
}

/// One webhook, scoped to its owner (so a user can't read another's).
pub async fn get(pool: &Pool, user_id: &str, id: &str) -> Result<Option<Webhook>, DbError> {
    let sql = format!("SELECT {COLS} FROM webhooks WHERE id = ? AND user_id = ?");
    let row = sqlx::query(&sql)
        .bind(id)
        .bind(user_id)
        .fetch_optional(pool)
        .await?;
    row.as_ref().map(map_row).transpose()
}

/// Apply an edit, scoped to the owner. Returns `true` if a row matched.
pub async fn update(
    pool: &Pool,
    user_id: &str,
    id: &str,
    edit: EditWebhook,
) -> Result<bool, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let previous: Option<(Option<String>, Option<String>)> =
        sqlx::query_as("SELECT dot_id, last_session_id FROM webhooks WHERE id = ? AND user_id = ?")
            .bind(id)
            .bind(user_id)
            .fetch_optional(&mut *tx)
            .await?;
    let Some((previous_dot, previous_session)) = previous else {
        return Ok(false);
    };
    if !dot_sessions::binding_valid(
        &mut tx,
        user_id,
        edit.dot_id.as_deref(),
        previous_dot.as_deref(),
    )
    .await?
    {
        return Err(
            sqlx::Error::Protocol("The selected Dot is not owned and active".into()).into(),
        );
    }
    let linked = match edit.linked_session_id {
        Some(linked) => linked,
        None if previous_dot != edit.dot_id || !edit.reuse_conversation => None,
        None => previous_session,
    };
    if let Some(session_id) = linked.as_deref()
        && (!edit.reuse_conversation
            || !dot_sessions::session_matches(&mut tx, user_id, session_id, edit.dot_id.as_deref())
                .await?)
    {
        return Err(sqlx::Error::Protocol(
            "The linked conversation does not match this automation".into(),
        )
        .into());
    }
    let affected = sqlx::query(
        r#"UPDATE webhooks
           SET dot_id = ?, last_session_id = ?, name = ?, prompt = ?, model = ?, tools_enabled = ?, synchronous = ?,
               reuse_conversation = ?, reuse_rounds = ?, updated_at = ?
           WHERE id = ? AND user_id = ?"#,
    )
    .bind(&edit.dot_id)
    .bind(&linked)
    .bind(&edit.name)
    .bind(&edit.prompt)
    .bind(&edit.model)
    .bind(edit.tools_enabled as i64)
    .bind(edit.synchronous as i64)
    .bind(edit.reuse_conversation as i64)
    .bind(edit.reuse_rounds)
    .bind(Timestamp::now().to_string())
    .bind(id)
    .bind(user_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    tx.commit().await?;
    Ok(affected > 0)
}

/// Pause or resume a webhook, scoped to the owner. A paused webhook's trigger
/// 404s. Returns `true` if a row matched.
pub async fn set_enabled(
    pool: &Pool,
    user_id: &str,
    id: &str,
    enabled: bool,
) -> Result<bool, DbError> {
    let affected =
        sqlx::query("UPDATE webhooks SET enabled = ?, updated_at = ? WHERE id = ? AND user_id = ?")
            .bind(enabled as i64)
            .bind(Timestamp::now().to_string())
            .bind(id)
            .bind(user_id)
            .execute(pool)
            .await?
            .rows_affected();
    Ok(affected > 0)
}

/// Swap in a fresh secret hash, scoped to the owner. The old trigger URL stops
/// working immediately. Returns `true` if a row matched.
pub async fn rotate_secret(
    pool: &Pool,
    user_id: &str,
    id: &str,
    new_hash: &str,
) -> Result<bool, DbError> {
    let affected = sqlx::query(
        "UPDATE webhooks SET secret_hash = ?, updated_at = ? WHERE id = ? AND user_id = ?",
    )
    .bind(new_hash)
    .bind(Timestamp::now().to_string())
    .bind(id)
    .bind(user_id)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(affected > 0)
}

/// Delete an idle webhook, scoped to the owner.
pub async fn delete(pool: &Pool, user_id: &str, id: &str) -> Result<DeleteOutcome, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM webhooks WHERE id = ? AND user_id = ?)")
            .bind(id)
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await?;
    if !exists {
        return Ok(DeleteOutcome::Missing);
    }
    let pending: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM webhook_runs r LEFT JOIN chat_turns t ON t.id = r.assistant_turn_id
         WHERE r.webhook_id = ? AND (t.status IN ('in_progress', 'suspended') OR (r.status IS NULL AND r.assistant_turn_id IS NULL)))",
    ).bind(id).fetch_one(&mut *tx).await?;
    if pending {
        return Ok(DeleteOutcome::Pending);
    }
    sqlx::query("DELETE FROM webhooks WHERE id = ? AND user_id = ?")
        .bind(id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(DeleteOutcome::Deleted)
}

/// The webhook a trigger secret's hash belongs to — but only when it's
/// enabled. The trigger's hot path; a paused (or missing) webhook returns
/// `None`, which the handler turns into a 404. Not owner-scoped: the secret
/// itself is the credential.
pub async fn find_active_by_secret_hash(
    pool: &Pool,
    secret_hash: &str,
) -> Result<Option<Webhook>, DbError> {
    let sql = format!("SELECT {COLS} FROM webhooks WHERE secret_hash = ? AND enabled = 1");
    let row = sqlx::query(&sql)
        .bind(secret_hash)
        .fetch_optional(pool)
        .await?;
    row.as_ref().map(map_row).transpose()
}

/// Record the outcome of a fire. `status` is `"ok"` or `"error"`;
/// `session_id` is the chat the fire opened (kept even on error so the owner
/// can inspect the partial conversation).
pub async fn mark_fired(
    pool: &Pool,
    id: &str,
    status: &str,
    session_id: Option<&str>,
    error: Option<&str>,
) -> Result<(), DbError> {
    sqlx::query(
        r#"UPDATE webhooks
           SET last_fired_at = ?, last_status = ?, last_session_id = ?, last_error = ?,
               updated_at = ?
           WHERE id = ?"#,
    )
    .bind(Timestamp::now().to_string())
    .bind(status)
    .bind(session_id)
    .bind(error)
    .bind(Timestamp::now().to_string())
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Stash the raw body of a fire so the owner can later rerun it with a
/// different prompt. Called at fire time (before the run), so the payload is
/// retained even if the run itself errors. Not owner-scoped — the trigger has
/// already resolved the webhook by its secret.
pub async fn set_last_payload(pool: &Pool, id: &str, payload: &str) -> Result<(), DbError> {
    sqlx::query("UPDATE webhooks SET last_payload = ?, updated_at = ? WHERE id = ?")
        .bind(payload)
        .bind(Timestamp::now().to_string())
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Run history (webhook_runs, migration 0038)

/// One recorded fire of a webhook — the run-history unit. `prompt` + `payload`
/// are captured per run so any past run is fully reproducible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebhookRun {
    pub id: String,
    pub webhook_id: String,
    pub fired_at: Timestamp,
    /// `None` while the run is in flight (async fires respond before finishing);
    /// `"ok"` or `"error"` once complete.
    pub status: Option<String>,
    /// Chat session this run opened.
    pub session_id: Option<String>,
    pub assistant_turn_id: Option<String>,
    /// The run's chat has since been deleted by its owner.
    pub chat_deleted: bool,
    pub error: Option<String>,
    pub prompt: String,
    pub payload: String,
    /// `"fire"` (external trigger) or `"rerun"` (manual replay).
    pub source: String,
    pub created_at: Timestamp,
}

const RUN_COLS: &str = "r.id, r.webhook_id, r.fired_at, r.status, r.session_id, r.assistant_turn_id, r.error, \
     r.prompt, r.payload, r.source, r.created_at, \
     (r.session_id IS NOT NULL AND s.id IS NULL) AS chat_deleted";

/// Every run read goes through this join, so `chat_deleted` is always known.
const RUN_FROM: &str = "webhook_runs r LEFT JOIN chat_sessions s ON s.id = r.session_id";

/// What a run left pending by a dead process is closed with at startup.
pub const RUN_INTERRUPTED: &str = "interrupted — the server stopped before this run finished";

fn map_run(row: &SqliteRow) -> Result<WebhookRun, DbError> {
    Ok(WebhookRun {
        id: row.try_get("id")?,
        webhook_id: row.try_get("webhook_id")?,
        fired_at: parse_ts(row.try_get("fired_at")?, "fired_at")?,
        status: row.try_get("status")?,
        session_id: row.try_get("session_id")?,
        assistant_turn_id: row.try_get("assistant_turn_id")?,
        chat_deleted: row.try_get("chat_deleted")?,
        error: row.try_get("error")?,
        prompt: row.try_get("prompt")?,
        payload: row.try_get("payload")?,
        source: row.try_get("source")?,
        created_at: parse_ts(row.try_get("created_at")?, "created_at")?,
    })
}

/// Record the start of a run (status left NULL until [`finish_run`]). Returns
/// the new run id. Not owner-scoped — the caller has already resolved the
/// webhook (by secret for a fire, by owner for a rerun).
pub struct NewWebhookRun<'a> {
    pub webhook_id: &'a str,
    pub session_id: &'a str,
    pub prompt: &'a str,
    pub payload: &'a str,
    pub source: &'a str,
    pub assistant_turn_id: &'a str,
    pub tools_enabled: bool,
    pub history_limit: Option<usize>,
}

pub async fn record_run_start(pool: &Pool, new: NewWebhookRun<'_>) -> Result<String, DbError> {
    let NewWebhookRun {
        webhook_id,
        session_id,
        prompt,
        payload,
        source,
        assistant_turn_id,
        tools_enabled,
        history_limit,
    } = new;
    let mut tx = WriteTx::begin(pool).await?;
    let source_row: Option<(String, Option<String>)> =
        sqlx::query_as("SELECT user_id, dot_id FROM webhooks WHERE id = ?")
            .bind(webhook_id)
            .fetch_optional(&mut *tx)
            .await?;
    let Some((owner_id, dot_id)) = source_row else {
        return Err(sqlx::Error::Protocol("The webhook is unavailable".into()).into());
    };
    if !dot_sessions::session_matches(&mut tx, &owner_id, session_id, dot_id.as_deref()).await? {
        return Err(sqlx::Error::Protocol(
            "The webhook run conversation has a different owner or Dot".into(),
        )
        .into());
    }
    let valid_turn: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM chat_turns WHERE id = ? AND session_id = ? AND role = 'assistant' AND status = 'in_progress')")
        .bind(assistant_turn_id).bind(session_id).fetch_one(&mut *tx).await?;
    if !valid_turn {
        return Err(
            sqlx::Error::Protocol("The webhook assistant turn is unavailable".into()).into(),
        );
    }
    let history_limit = history_limit
        .map(i64::try_from)
        .transpose()
        .map_err(|_| sqlx::Error::Protocol("The webhook history limit is too large".into()))?;
    let id = Uuid::new_v4().to_string();
    let now = Timestamp::now().to_string();
    sqlx::query(
        r#"INSERT INTO webhook_runs
              (id, webhook_id, fired_at, session_id, prompt, payload, source, created_at, assistant_turn_id, tools_enabled, history_limit)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
    )
    .bind(&id)
    .bind(webhook_id)
    .bind(&now)
    .bind(session_id)
    .bind(prompt)
    .bind(payload)
    .bind(source)
    .bind(&now)
    .bind(assistant_turn_id)
    .bind(tools_enabled as i64)
    .bind(history_limit)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(id)
}

/// Record the outcome of a run started by [`record_run_start`].
pub async fn finish_run(
    pool: &Pool,
    run_id: &str,
    status: &str,
    error: Option<&str>,
) -> Result<(), DbError> {
    sqlx::query("UPDATE webhook_runs SET status = ?, error = ? WHERE id = ?")
        .bind(status)
        .bind(error)
        .bind(run_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Close every run still pending at startup. An async fire runs detached from
/// its request, so a restart mid-run leaves nothing that will ever finish it.
/// A webhook whose newest run (as [`list_runs`] orders) is one of them gets
/// the same outcome on its list row. Returns how many runs were closed.
///
/// Only sound before the server accepts fires: a run pending *then* is
/// necessarily a previous process's.
pub async fn sweep_interrupted_runs(pool: &Pool) -> Result<u64, DbError> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        r#"UPDATE webhooks
           SET last_status = 'error', last_error = ?1, last_fired_at = newest.fired_at
           FROM (SELECT r.webhook_id, r.fired_at
                 FROM webhook_runs r
                 WHERE r.status IS NULL
                   AND r.rowid = (SELECT rowid FROM webhook_runs
                                  WHERE webhook_id = r.webhook_id
                                  ORDER BY rtrim(fired_at, 'Z') DESC, rowid DESC
                                  LIMIT 1)) AS newest
           WHERE webhooks.id = newest.webhook_id"#,
    )
    .bind(RUN_INTERRUPTED)
    .execute(&mut *tx)
    .await?;
    let done =
        sqlx::query("UPDATE webhook_runs SET status = 'error', error = ? WHERE status IS NULL")
            .bind(RUN_INTERRUPTED)
            .execute(&mut *tx)
            .await?;
    tx.commit().await?;
    let reconciled = crate::server::background::reconcile_interrupted_waiting(
        pool,
        aiplane_core::server::db::usage::UsageSource::Webhook,
    )
    .await?;
    Ok(done.rows_affected() + reconciled)
}

/// A webhook's most recent runs, newest first, capped at `limit`.
pub async fn list_runs(
    pool: &Pool,
    webhook_id: &str,
    limit: i64,
) -> Result<Vec<WebhookRun>, DbError> {
    // Newest-first on `rtrim(fired_at, 'Z')`: the stored RFC 3339 text sorts
    // wrongly within one second (`…00.1Z` after `…00.123Z`), without its `Z`
    // it sorts right (`db::window_key`). The tiebreaker is `rowid DESC`
    // (SQLite's monotonic insertion order), not `id`, a random UUID.
    let sql = format!(
        "SELECT {RUN_COLS} FROM {RUN_FROM} WHERE r.webhook_id = ? \
         ORDER BY rtrim(r.fired_at, 'Z') DESC, r.rowid DESC LIMIT ?"
    );
    let rows = sqlx::query(&sql)
        .bind(webhook_id)
        .bind(limit)
        .fetch_all(pool)
        .await?;
    rows.iter().map(map_run).collect()
}

/// How many runs, and how many distinct chats, each of a user's webhooks has.
///
/// Both numbers in one grouped query rather than one per row: the list page
/// renders every webhook, and the two differ for a `reuse_conversation` hook
/// — many fires, one conversation — which is exactly the distinction the
/// row's link has to make.
pub async fn run_counts_for_user(
    pool: &Pool,
    user_id: &str,
) -> Result<std::collections::HashMap<String, (i64, i64)>, DbError> {
    let rows = sqlx::query(
        r#"SELECT r.webhook_id AS webhook_id,
                  COUNT(*) AS runs,
                  COUNT(DISTINCT r.session_id) AS chats
           FROM webhook_runs r
           JOIN webhooks w ON w.id = r.webhook_id
           WHERE w.user_id = ?
           GROUP BY r.webhook_id"#,
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|row| {
            Ok((
                row.try_get::<String, _>("webhook_id")?,
                (row.try_get("runs")?, row.try_get("chats")?),
            ))
        })
        .collect()
}

/// One run, scoped to its webhook (the caller has verified webhook ownership).
pub async fn get_run(
    pool: &Pool,
    webhook_id: &str,
    run_id: &str,
) -> Result<Option<WebhookRun>, DbError> {
    let sql = format!("SELECT {RUN_COLS} FROM {RUN_FROM} WHERE r.webhook_id = ? AND r.id = ?");
    let row = sqlx::query(&sql)
        .bind(webhook_id)
        .bind(run_id)
        .fetch_optional(pool)
        .await?;
    row.as_ref().map(map_run).transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn fresh_db() -> Pool {
        // `open` runs the migration set, which includes 0037 (the webhooks table).
        aiplane_core::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap()
    }

    async fn seed_user(pool: &Pool, id: &str) {
        // The FK to users(id) means we need a row to attach webhooks to.
        let now = Timestamp::now();
        aiplane_core::server::db::users::upsert(
            pool,
            &aiplane_core::server::db::users::User {
                id: id.to_string(),
                email: format!("{id}@example.com"),
                name: None,
                roles: vec![],
                created_at: now,
                updated_at: now,
                timezone: Some("Europe/Berlin".to_string()),
                speech_voice: None,
            },
        )
        .await
        .unwrap();
    }

    async fn fixture_run_start(pool: &Pool, new: NewWebhookRun<'_>) -> Result<String, DbError> {
        let NewWebhookRun {
            webhook_id: hook_id,
            session_id,
            prompt,
            payload,
            source,
            assistant_turn_id: turn_id,
            tools_enabled,
            history_limit,
        } = new;
        let owner: String = sqlx::query_scalar("SELECT user_id FROM webhooks WHERE id = ?")
            .bind(hook_id)
            .fetch_one(pool)
            .await?;
        let temporary = session_core::db::get_session(pool, &owner, session_id)
            .await?
            .is_none();
        if temporary {
            let session = session_core::db::create_session(pool, &owner).await?;
            sqlx::query("UPDATE chat_sessions SET id = ? WHERE id = ?")
                .bind(session_id)
                .bind(&session.id)
                .execute(pool)
                .await?;
        }
        session_core::db::create_assistant_turn_in_progress(pool, session_id, turn_id, "qwen")
            .await?;
        let id = record_run_start(
            pool,
            NewWebhookRun {
                webhook_id: hook_id,
                session_id,
                prompt,
                payload,
                source,
                assistant_turn_id: turn_id,
                tools_enabled,
                history_limit,
            },
        )
        .await?;
        session_core::db::finalize_turn(
            pool,
            turn_id,
            session_core::db::TurnStatus::Completed,
            None,
        )
        .await?;
        if temporary {
            session_core::db::delete_session(pool, &owner, session_id).await?;
        }
        Ok(id)
    }

    fn sample(user_id: &str, secret_hash: &str) -> NewWebhook {
        NewWebhook {
            dot_id: None,
            linked_session_id: None,
            user_id: user_id.to_string(),
            name: "Deploy digest".to_string(),
            prompt: "Summarize the incoming payload.".to_string(),
            model: "qwen".to_string(),
            tools_enabled: false,
            synchronous: true,
            reuse_conversation: false,
            reuse_rounds: 5,
            secret_hash: secret_hash.to_string(),
        }
    }

    #[tokio::test]
    async fn create_then_get_round_trips() {
        let pool = fresh_db().await;
        seed_user(&pool, "u1").await;
        let created = create(&pool, sample("u1", "hash-aaa")).await.unwrap();
        let got = get(&pool, "u1", &created.id).await.unwrap().unwrap();
        assert_eq!(got, created);
        assert!(got.enabled && got.synchronous && !got.tools_enabled);
        assert_eq!(got.secret_hash, "hash-aaa");
    }

    #[tokio::test]
    async fn get_and_delete_are_scoped_to_owner() {
        let pool = fresh_db().await;
        seed_user(&pool, "u1").await;
        seed_user(&pool, "u2").await;
        let created = create(&pool, sample("u1", "hash-bbb")).await.unwrap();
        assert!(get(&pool, "u2", &created.id).await.unwrap().is_none());
        assert_eq!(
            delete(&pool, "u2", &created.id).await.unwrap(),
            DeleteOutcome::Missing
        );
        assert!(get(&pool, "u1", &created.id).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn find_active_by_secret_hash_only_hits_enabled() {
        let pool = fresh_db().await;
        seed_user(&pool, "u1").await;
        let created = create(&pool, sample("u1", "hash-ccc")).await.unwrap();
        assert_eq!(
            find_active_by_secret_hash(&pool, "hash-ccc")
                .await
                .unwrap()
                .map(|w| w.id),
            Some(created.id.clone())
        );
        // Pausing takes it out of the trigger's reach.
        set_enabled(&pool, "u1", &created.id, false).await.unwrap();
        assert!(
            find_active_by_secret_hash(&pool, "hash-ccc")
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            find_active_by_secret_hash(&pool, "hash-nope")
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn startup_sweep_closes_pending_runs_and_the_row_that_shows_them() {
        let pool = fresh_db().await;
        seed_user(&pool, "u1").await;
        let hook = create(&pool, sample("u1", "hash-sweep")).await.unwrap();
        let chat = session_core::db::create_session(&pool, "u1").await.unwrap();
        let done = fixture_run_start(
            &pool,
            NewWebhookRun {
                webhook_id: &hook.id,
                session_id: &chat.id,
                prompt: "p",
                payload: "{}",
                source: "fire",
                assistant_turn_id: &uuid::Uuid::new_v4().to_string(),
                tools_enabled: false,
                history_limit: None,
            },
        )
        .await
        .unwrap();
        finish_run(&pool, &done, "ok", None).await.unwrap();
        mark_fired(&pool, &hook.id, "ok", Some(&chat.id), None)
            .await
            .unwrap();
        let orphan = fixture_run_start(
            &pool,
            NewWebhookRun {
                webhook_id: &hook.id,
                session_id: &chat.id,
                prompt: "p",
                payload: "{}",
                source: "fire",
                assistant_turn_id: &uuid::Uuid::new_v4().to_string(),
                tools_enabled: false,
                history_limit: None,
            },
        )
        .await
        .unwrap();

        assert_eq!(sweep_interrupted_runs(&pool).await.unwrap(), 1);

        let run = get_run(&pool, &hook.id, &orphan).await.unwrap().unwrap();
        assert_eq!(run.status.as_deref(), Some("error"));
        assert_eq!(run.error.as_deref(), Some(RUN_INTERRUPTED));
        assert_eq!(run.session_id.as_deref(), Some(chat.id.as_str()));
        let row = get(&pool, "u1", &hook.id).await.unwrap().unwrap();
        assert_eq!(row.last_status.as_deref(), Some("error"));
        assert_eq!(row.last_error.as_deref(), Some(RUN_INTERRUPTED));
        assert_eq!(
            get_run(&pool, &hook.id, &done)
                .await
                .unwrap()
                .unwrap()
                .status
                .as_deref(),
            Some("ok")
        );
    }

    /// `fired_at` sorts as text, and within one second `…00.1Z` sorts after
    /// the later `…00.123Z`.
    #[tokio::test]
    async fn the_newest_run_is_the_one_fired_last_even_within_one_second() {
        let pool = fresh_db().await;
        seed_user(&pool, "u1").await;
        let hook = create(&pool, sample("u1", "hash-order")).await.unwrap();
        let chat = session_core::db::create_session(&pool, "u1").await.unwrap();
        let earlier = fixture_run_start(
            &pool,
            NewWebhookRun {
                webhook_id: &hook.id,
                session_id: &chat.id,
                prompt: "p",
                payload: "{}",
                source: "fire",
                assistant_turn_id: &uuid::Uuid::new_v4().to_string(),
                tools_enabled: false,
                history_limit: None,
            },
        )
        .await
        .unwrap();
        let later = fixture_run_start(
            &pool,
            NewWebhookRun {
                webhook_id: &hook.id,
                session_id: &chat.id,
                prompt: "p",
                payload: "{}",
                source: "fire",
                assistant_turn_id: &uuid::Uuid::new_v4().to_string(),
                tools_enabled: false,
                history_limit: None,
            },
        )
        .await
        .unwrap();
        for (run, at) in [
            (&earlier, "2026-10-04T10:00:00.1Z"),
            (&later, "2026-10-04T10:00:00.123Z"),
        ] {
            sqlx::query("UPDATE webhook_runs SET fired_at = ? WHERE id = ?")
                .bind(at)
                .bind(run)
                .execute(&pool)
                .await
                .unwrap();
        }
        finish_run(&pool, &later, "ok", None).await.unwrap();
        mark_fired(&pool, &hook.id, "ok", Some(&chat.id), None)
            .await
            .unwrap();

        let runs = list_runs(&pool, &hook.id, 50).await.unwrap();
        let order: Vec<&str> = runs.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(order, [later.as_str(), earlier.as_str()]);
        assert_eq!(sweep_interrupted_runs(&pool).await.unwrap(), 1);
        let row = get(&pool, "u1", &hook.id).await.unwrap().unwrap();
        assert_eq!(row.last_status.as_deref(), Some("ok"));
    }

    #[tokio::test]
    async fn runs_report_a_deleted_chat() {
        let pool = fresh_db().await;
        seed_user(&pool, "u1").await;
        let hook = create(&pool, sample("u1", "hash-deleted")).await.unwrap();
        let chat = session_core::db::create_session(&pool, "u1").await.unwrap();
        let run = fixture_run_start(
            &pool,
            NewWebhookRun {
                webhook_id: &hook.id,
                session_id: &chat.id,
                prompt: "p",
                payload: "{}",
                source: "fire",
                assistant_turn_id: &uuid::Uuid::new_v4().to_string(),
                tools_enabled: false,
                history_limit: None,
            },
        )
        .await
        .unwrap();
        assert!(
            !get_run(&pool, &hook.id, &run)
                .await
                .unwrap()
                .unwrap()
                .chat_deleted
        );

        session_core::db::delete_session(&pool, "u1", &chat.id)
            .await
            .unwrap();

        assert!(
            get_run(&pool, &hook.id, &run)
                .await
                .unwrap()
                .unwrap()
                .chat_deleted
        );
        assert!(list_runs(&pool, &hook.id, 50).await.unwrap()[0].chat_deleted);
    }

    #[tokio::test]
    async fn linked_chat_updates_are_atomic_and_owner_scoped() {
        let pool = fresh_db().await;
        seed_user(&pool, "u1").await;
        seed_user(&pool, "u2").await;
        let chat = session_core::db::create_session(&pool, "u1").await.unwrap();
        let mut new = sample("u1", "hash-link");
        new.reuse_conversation = true;
        new.linked_session_id = Some(chat.id.clone());
        let source = create(&pool, new).await.unwrap();
        assert_eq!(source.last_session_id.as_deref(), Some(chat.id.as_str()));
        let edit = || EditWebhook {
            dot_id: None,
            linked_session_id: Some(None),
            name: source.name.clone(),
            prompt: source.prompt.clone(),
            model: source.model.clone(),
            tools_enabled: source.tools_enabled,
            reuse_conversation: true,
            reuse_rounds: source.reuse_rounds,
            synchronous: source.synchronous,
        };
        assert!(!update(&pool, "u2", &source.id, edit()).await.unwrap());
        assert_eq!(
            get(&pool, "u1", &source.id)
                .await
                .unwrap()
                .unwrap()
                .last_session_id,
            Some(chat.id)
        );
        assert!(update(&pool, "u1", &source.id, edit()).await.unwrap());
        assert!(
            get(&pool, "u1", &source.id)
                .await
                .unwrap()
                .unwrap()
                .last_session_id
                .is_none()
        );
    }

    #[tokio::test]
    async fn rotate_secret_swaps_the_hash() {
        let pool = fresh_db().await;
        seed_user(&pool, "u1").await;
        let created = create(&pool, sample("u1", "hash-old")).await.unwrap();
        assert!(
            rotate_secret(&pool, "u1", &created.id, "hash-new")
                .await
                .unwrap()
        );
        // Old hash no longer resolves; new one does.
        assert!(
            find_active_by_secret_hash(&pool, "hash-old")
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            find_active_by_secret_hash(&pool, "hash-new")
                .await
                .unwrap()
                .is_some()
        );
        // Rotation is owner-scoped.
        seed_user(&pool, "u2").await;
        assert!(
            !rotate_secret(&pool, "u2", &created.id, "hash-hijack")
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn mark_fired_records_status_and_session() {
        let pool = fresh_db().await;
        seed_user(&pool, "u1").await;
        let created = create(&pool, sample("u1", "hash-ddd")).await.unwrap();
        mark_fired(&pool, &created.id, "ok", Some("sess-1"), None)
            .await
            .unwrap();
        let got = get(&pool, "u1", &created.id).await.unwrap().unwrap();
        assert_eq!(got.last_status.as_deref(), Some("ok"));
        assert_eq!(got.last_session_id.as_deref(), Some("sess-1"));
        assert!(got.last_fired_at.is_some());
        assert!(got.last_error.is_none());
    }

    #[tokio::test]
    async fn set_last_payload_round_trips_for_rerun() {
        let pool = fresh_db().await;
        seed_user(&pool, "u1").await;
        let created = create(&pool, sample("u1", "hash-eee")).await.unwrap();
        assert!(
            created.last_payload.is_none(),
            "no payload until first fire"
        );
        set_last_payload(&pool, &created.id, r#"{"event":"deploy"}"#)
            .await
            .unwrap();
        let got = get(&pool, "u1", &created.id).await.unwrap().unwrap();
        assert_eq!(got.last_payload.as_deref(), Some(r#"{"event":"deploy"}"#));
    }

    #[tokio::test]
    async fn run_history_records_lists_and_reproduces() {
        let pool = fresh_db().await;
        seed_user(&pool, "u1").await;
        let hook = create(&pool, sample("u1", "hash-run")).await.unwrap();

        let r1 = fixture_run_start(
            &pool,
            NewWebhookRun {
                webhook_id: &hook.id,
                session_id: "sess-1",
                prompt: "prompt one",
                payload: r#"{"a":1}"#,
                source: "fire",
                assistant_turn_id: &uuid::Uuid::new_v4().to_string(),
                tools_enabled: false,
                history_limit: None,
            },
        )
        .await
        .unwrap();
        finish_run(&pool, &r1, "ok", None).await.unwrap();
        let r2 = fixture_run_start(
            &pool,
            NewWebhookRun {
                webhook_id: &hook.id,
                session_id: "sess-2",
                prompt: "prompt two",
                payload: r#"{"b":2}"#,
                source: "rerun",
                assistant_turn_id: &uuid::Uuid::new_v4().to_string(),
                tools_enabled: false,
                history_limit: None,
            },
        )
        .await
        .unwrap();
        finish_run(&pool, &r2, "error", Some("boom")).await.unwrap();

        // Newest first.
        let runs = list_runs(&pool, &hook.id, 50).await.unwrap();
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].id, r2);
        assert_eq!(runs[0].source, "rerun");
        assert_eq!(runs[0].status.as_deref(), Some("error"));
        assert_eq!(runs[0].error.as_deref(), Some("boom"));
        assert_eq!(runs[0].session_id.as_deref(), Some("sess-2"));
        assert_eq!(runs[1].id, r1);
        assert_eq!(runs[1].status.as_deref(), Some("ok"));

        // A run carries its own prompt + payload so it can be replayed exactly.
        let got = get_run(&pool, &hook.id, &r1).await.unwrap().unwrap();
        assert_eq!(got.prompt, "prompt one");
        assert_eq!(got.payload, r#"{"a":1}"#);

        // Scoped to the webhook.
        assert!(get_run(&pool, "other-hook", &r1).await.unwrap().is_none());

        // The limit caps the list.
        assert_eq!(list_runs(&pool, &hook.id, 1).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn run_counts_separate_runs_from_chats() {
        let pool = fresh_db().await;
        seed_user(&pool, "u1").await;
        seed_user(&pool, "u2").await;
        let fresh_each_time = create(&pool, sample("u1", "hash-a")).await.unwrap();
        let reuses = create(&pool, sample("u1", "hash-b")).await.unwrap();
        let other_owner = create(&pool, sample("u2", "hash-c")).await.unwrap();

        for session in ["sess-a", "sess-b"] {
            let run = fixture_run_start(
                &pool,
                NewWebhookRun {
                    webhook_id: &fresh_each_time.id,
                    session_id: session,
                    prompt: "p",
                    payload: "{}",
                    source: "fire",
                    assistant_turn_id: &uuid::Uuid::new_v4().to_string(),
                    tools_enabled: false,
                    history_limit: None,
                },
            )
            .await
            .unwrap();
            finish_run(&pool, &run, "ok", None).await.unwrap();
        }
        // A reusing webhook fires repeatedly into one conversation: three
        // runs, one chat. That is the distinction the list row's link makes.
        for _ in 0..3 {
            let run = fixture_run_start(
                &pool,
                NewWebhookRun {
                    webhook_id: &reuses.id,
                    session_id: "sess-shared",
                    prompt: "p",
                    payload: "{}",
                    source: "fire",
                    assistant_turn_id: &uuid::Uuid::new_v4().to_string(),
                    tools_enabled: false,
                    history_limit: None,
                },
            )
            .await
            .unwrap();
            finish_run(&pool, &run, "ok", None).await.unwrap();
        }
        let run = fixture_run_start(
            &pool,
            NewWebhookRun {
                webhook_id: &other_owner.id,
                session_id: "sess-other",
                prompt: "p",
                payload: "{}",
                source: "fire",
                assistant_turn_id: &uuid::Uuid::new_v4().to_string(),
                tools_enabled: false,
                history_limit: None,
            },
        )
        .await
        .unwrap();
        finish_run(&pool, &run, "ok", None).await.unwrap();

        let counts = run_counts_for_user(&pool, "u1").await.unwrap();
        assert_eq!(counts.get(&fresh_each_time.id), Some(&(2, 2)));
        assert_eq!(counts.get(&reuses.id), Some(&(3, 1)));
        // Another user's webhook never appears in this user's counts.
        assert_eq!(counts.get(&other_owner.id), None);
    }
    #[tokio::test]
    async fn correlation_validates_owner_and_turn_and_blocks_nonterminal_deletion() {
        let pool = fresh_db().await;
        seed_user(&pool, "u1").await;
        seed_user(&pool, "u2").await;
        let hook = create(&pool, sample("u1", "hash-correlation"))
            .await
            .unwrap();
        let foreign = session_core::db::create_session(&pool, "u2").await.unwrap();
        session_core::db::create_assistant_turn_in_progress(
            &pool,
            &foreign.id,
            "foreign-turn",
            "qwen",
        )
        .await
        .unwrap();
        assert!(
            record_run_start(
                &pool,
                NewWebhookRun {
                    webhook_id: &hook.id,
                    session_id: &foreign.id,
                    prompt: "p",
                    payload: "{}",
                    source: "fire",
                    assistant_turn_id: "foreign-turn",
                    tools_enabled: true,
                    history_limit: Some(4)
                }
            )
            .await
            .is_err()
        );
        assert!(list_runs(&pool, &hook.id, 10).await.unwrap().is_empty());
        let chat = session_core::db::create_session(&pool, "u1").await.unwrap();
        session_core::db::create_assistant_turn_in_progress(&pool, &chat.id, "owned-turn", "qwen")
            .await
            .unwrap();
        assert!(
            record_run_start(
                &pool,
                NewWebhookRun {
                    webhook_id: &hook.id,
                    session_id: &chat.id,
                    prompt: "p",
                    payload: "{}",
                    source: "fire",
                    assistant_turn_id: "foreign-turn",
                    tools_enabled: true,
                    history_limit: Some(4)
                }
            )
            .await
            .is_err()
        );
        let run = record_run_start(
            &pool,
            NewWebhookRun {
                webhook_id: &hook.id,
                session_id: &chat.id,
                prompt: "p",
                payload: "{}",
                source: "fire",
                assistant_turn_id: "owned-turn",
                tools_enabled: true,
                history_limit: Some(4),
            },
        )
        .await
        .unwrap();
        let snapshot: (String, i64, i64) = sqlx::query_as(
            "SELECT assistant_turn_id, tools_enabled, history_limit FROM webhook_runs WHERE id = ?",
        )
        .bind(&run)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(snapshot, ("owned-turn".into(), 1, 4));
        assert_eq!(
            delete(&pool, "u1", &hook.id).await.unwrap(),
            DeleteOutcome::Pending
        );
        session_core::db::finalize_turn(
            &pool,
            "owned-turn",
            session_core::db::TurnStatus::Suspended,
            None,
        )
        .await
        .unwrap();
        assert_eq!(
            delete(&pool, "u1", &hook.id).await.unwrap(),
            DeleteOutcome::Pending
        );
        session_core::db::finalize_turn(
            &pool,
            "owned-turn",
            session_core::db::TurnStatus::Cancelled,
            None,
        )
        .await
        .unwrap();
        assert_eq!(
            delete(&pool, "u1", &hook.id).await.unwrap(),
            DeleteOutcome::Deleted
        );
    }
}
