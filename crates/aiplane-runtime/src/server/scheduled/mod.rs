// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! User- and principal-owned scheduled actions, their atomic due claims and
//! exact turn-correlated run history. The existing cron worker drives personal
//! conversations or an organization's pinned private principal conversation.
//! Storage owns immutable source ownership and configuration generations;
//! current human and execution authority are enforced by the API and worker.

pub mod cron;
pub mod worker;

use aiplane_agents::db::{WriteTx, agents, dot_sessions, run_sessions};
use jiff::Timestamp;
use sqlx::Row;
use sqlx::sqlite::SqliteRow;
use uuid::Uuid;

use aiplane_core::server::db::{DbError, Pool, parse_optional_ts, parse_ts};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleOwner {
    User(String),
    Principal(String),
}

impl ScheduleOwner {
    fn columns(&self) -> (Option<&str>, Option<&str>) {
        match self {
            Self::User(id) => (Some(id), None),
            Self::Principal(id) => (None, Some(id)),
        }
    }
}

/// A persisted scheduled action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledAction {
    pub id: String,
    pub owner: ScheduleOwner,
    pub created_by: Option<String>,
    pub configuration_generation: String,
    pub dot_id: Option<String>,
    pub name: String,
    pub prompt: String,
    pub model: Option<String>,
    pub agent_version: Option<i64>,
    /// 5-field cron expression (the source of truth; the UI builder and
    /// the advanced field both write this).
    pub cron: String,
    /// IANA timezone the cron expression is evaluated in.
    pub timezone: String,
    pub tools_enabled: bool,
    /// `true` = each fire reuses the previous run's chat session
    /// (`last_session_id`) so the model sees prior runs as history; `false`
    /// = each fire opens a fresh session. First run (or a deleted prior
    /// session) falls back to a fresh session regardless.
    pub reuse_conversation: bool,
    /// When reusing, how many recent rounds (one round = the run's prompt +
    /// reply = 2 turns) of history to replay — caps unbounded growth.
    pub reuse_rounds: i64,
    /// `false` = paused; the worker ignores it and `next_run_at` is NULL.
    pub enabled: bool,
    /// Precomputed next fire time (UTC). NULL when paused or when the
    /// expression has no future occurrence.
    pub next_run_at: Option<Timestamp>,
    pub last_run_at: Option<Timestamp>,
    /// `"ok"` or `"error"` — the outcome of the most recent run.
    pub last_status: Option<String>,
    /// Chat session opened by the most recent run, for the "open" link.
    pub last_session_id: Option<String>,
    pub last_error: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

/// The validated, ready-to-insert fields for a new action. `next_run_at`
/// is computed by the caller from `cron` + `timezone`.
pub struct NewAction {
    pub owner: ScheduleOwner,
    pub created_by: String,
    pub dot_id: Option<String>,
    pub name: String,
    pub prompt: String,
    pub model: Option<String>,
    pub agent_version: Option<i64>,
    pub cron: String,
    pub timezone: String,
    pub tools_enabled: bool,
    pub reuse_conversation: bool,
    pub reuse_rounds: i64,
    pub next_run_at: Option<Timestamp>,
    pub linked_session_id: Option<String>,
}

/// The mutable fields of an existing action, as submitted by the edit
/// form. `next_run_at` is recomputed by the caller from the new schedule.
pub struct EditAction {
    pub dot_id: Option<String>,
    pub name: String,
    pub prompt: String,
    pub model: Option<String>,
    pub agent_version: Option<i64>,
    pub cron: String,
    pub timezone: String,
    pub tools_enabled: bool,
    pub reuse_conversation: bool,
    pub reuse_rounds: i64,
    pub next_run_at: Option<Timestamp>,
    pub linked_session_id: Option<Option<String>>,
}

fn map_row(row: &SqliteRow) -> Result<ScheduledAction, DbError> {
    Ok(ScheduledAction {
        id: row.try_get("id")?,
        owner: match (
            row.try_get::<Option<String>, _>("user_id")?,
            row.try_get::<Option<String>, _>("principal_id")?,
        ) {
            (Some(id), None) => ScheduleOwner::User(id),
            (None, Some(id)) => ScheduleOwner::Principal(id),
            _ => return Err(invalid("The schedule has an invalid owner")),
        },
        created_by: row.try_get("created_by")?,
        configuration_generation: row.try_get("configuration_generation")?,
        dot_id: row.try_get("dot_id")?,
        name: row.try_get("name")?,
        prompt: row.try_get("prompt")?,
        model: row.try_get("model")?,
        agent_version: row.try_get("agent_version")?,
        cron: row.try_get("cron")?,
        timezone: row.try_get("timezone")?,
        tools_enabled: row.try_get::<i64, _>("tools_enabled")? != 0,
        reuse_conversation: row.try_get::<i64, _>("reuse_conversation")? != 0,
        reuse_rounds: row.try_get("reuse_rounds")?,
        enabled: row.try_get::<i64, _>("enabled")? != 0,
        next_run_at: parse_optional_ts(row.try_get("next_run_at")?, "next_run_at")?,
        last_run_at: parse_optional_ts(row.try_get("last_run_at")?, "last_run_at")?,
        last_status: row.try_get("last_status")?,
        last_session_id: row.try_get("last_session_id")?,
        last_error: row.try_get("last_error")?,
        created_at: parse_ts(row.try_get("created_at")?, "created_at")?,
        updated_at: parse_ts(row.try_get("updated_at")?, "updated_at")?,
    })
}

const COLS: &str = "id, user_id, principal_id, created_by, configuration_generation, agent_version, dot_id, name, prompt, model, cron, timezone, tools_enabled, \
     reuse_conversation, reuse_rounds, enabled, next_run_at, last_run_at, last_status, \
     last_session_id, last_error, created_at, updated_at";

/// Insert a new action. Returns the stored row.
fn invalid(message: &str) -> DbError {
    sqlx::Error::Protocol(message.into()).into()
}

struct ScheduleBinding<'a> {
    dot_id: Option<&'a str>,
    model: Option<&'a str>,
    version: Option<i64>,
    reuse: bool,
    linked: Option<&'a str>,
}

async fn validate_configuration_in(
    tx: &mut WriteTx,
    owner: &ScheduleOwner,
    previous_dot: Option<&str>,
    binding: ScheduleBinding<'_>,
) -> Result<(), DbError> {
    let ScheduleBinding {
        dot_id,
        model,
        version,
        reuse,
        linked,
    } = binding;
    match owner {
        ScheduleOwner::User(user) => {
            if model.is_none_or(|value| value.is_empty())
                || version.is_some()
                || !dot_sessions::binding_valid(tx, user, dot_id, previous_dot).await?
            {
                return Err(invalid("The schedule model or Dot binding is unavailable"));
            }
            if let Some(session) = linked
                && (!reuse || !dot_sessions::session_matches(tx, user, session, dot_id).await?)
            {
                return Err(invalid(
                    "The linked conversation does not match this automation",
                ));
            }
        }
        ScheduleOwner::Principal(principal) => {
            if model.is_some()
                || dot_id.is_some()
                || reuse
                || linked.is_some()
                || version.is_none_or(|value| value <= 0)
            {
                return Err(invalid(
                    "The principal schedule requires a published version and a fresh conversation",
                ));
            }
            if !matches!(
                agents::dot_owner_in(tx, principal).await?,
                Some(agents::DotOwner::Organization(_))
            ) || agents::version_in(tx, principal, version.expect("positive version checked"))
                .await?
                .is_none()
            {
                return Err(invalid(
                    "The principal schedule published version is unavailable",
                ));
            }
        }
    }
    Ok(())
}

pub async fn create(pool: &Pool, new: NewAction) -> Result<ScheduledAction, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let result = create_in(&mut tx, new).await?;
    tx.commit().await?;
    Ok(result)
}

pub async fn create_in(tx: &mut WriteTx, new: NewAction) -> Result<ScheduledAction, DbError> {
    validate_configuration_in(
        tx,
        &new.owner,
        None,
        ScheduleBinding {
            dot_id: new.dot_id.as_deref(),
            model: new.model.as_deref(),
            version: new.agent_version,
            reuse: new.reuse_conversation,
            linked: new.linked_session_id.as_deref(),
        },
    )
    .await?;
    let now = Timestamp::now();
    let row = ScheduledAction {
        id: Uuid::new_v4().to_string(),
        owner: new.owner,
        created_by: Some(new.created_by),
        configuration_generation: Uuid::new_v4().to_string(),
        dot_id: new.dot_id,
        name: new.name,
        prompt: new.prompt,
        model: new.model,
        agent_version: new.agent_version,
        cron: new.cron,
        timezone: new.timezone,
        tools_enabled: new.tools_enabled,
        reuse_conversation: new.reuse_conversation,
        reuse_rounds: new.reuse_rounds,
        enabled: true,
        next_run_at: new.next_run_at,
        last_run_at: None,
        last_status: None,
        last_session_id: new.linked_session_id,
        last_error: None,
        created_at: now,
        updated_at: now,
    };
    let (user, principal) = row.owner.columns();
    sqlx::query("INSERT INTO scheduled_actions(id,user_id,principal_id,created_by,configuration_generation,agent_version,dot_id,name,prompt,model,cron,timezone,tools_enabled,reuse_conversation,reuse_rounds,enabled,next_run_at,last_session_id,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,1,?,?,?,?)")
        .bind(&row.id).bind(user).bind(principal).bind(&row.created_by).bind(&row.configuration_generation).bind(row.agent_version)
        .bind(&row.dot_id).bind(&row.name).bind(&row.prompt).bind(&row.model).bind(&row.cron).bind(&row.timezone)
        .bind(row.tools_enabled).bind(row.reuse_conversation).bind(row.reuse_rounds)
        .bind(row.next_run_at.map(|v|v.to_string())).bind(&row.last_session_id).bind(now.to_string()).bind(now.to_string())
        .execute(&mut **tx).await?;
    Ok(row)
}

pub async fn list_for_user(pool: &Pool, user_id: &str) -> Result<Vec<ScheduledAction>, DbError> {
    list_for_owner_with(pool, &ScheduleOwner::User(user_id.into())).await
}

pub async fn list_for_owner_in(
    tx: &mut WriteTx,
    owner: &ScheduleOwner,
) -> Result<Vec<ScheduledAction>, DbError> {
    list_for_owner_with(&mut **tx, owner).await
}

async fn list_for_owner_with<'e, E: sqlx::Executor<'e, Database = sqlx::Sqlite>>(
    executor: E,
    owner: &ScheduleOwner,
) -> Result<Vec<ScheduledAction>, DbError> {
    let (user, principal) = owner.columns();
    let rows=sqlx::query(&format!("SELECT {COLS} FROM scheduled_actions WHERE user_id IS ? AND principal_id IS ? ORDER BY created_at DESC,id ASC"))
        .bind(user).bind(principal).fetch_all(executor).await?;
    rows.iter().map(map_row).collect()
}

pub async fn get(
    pool: &Pool,
    owner: &ScheduleOwner,
    id: &str,
) -> Result<Option<ScheduledAction>, DbError> {
    get_with(pool, owner, id).await
}

pub async fn get_in(
    tx: &mut WriteTx,
    owner: &ScheduleOwner,
    id: &str,
) -> Result<Option<ScheduledAction>, DbError> {
    get_with(&mut **tx, owner, id).await
}

async fn get_with<'e, E: sqlx::Executor<'e, Database = sqlx::Sqlite>>(
    executor: E,
    owner: &ScheduleOwner,
    id: &str,
) -> Result<Option<ScheduledAction>, DbError> {
    let (user, principal) = owner.columns();
    let row = sqlx::query(&format!(
        "SELECT {COLS} FROM scheduled_actions WHERE id=? AND user_id IS ? AND principal_id IS ?"
    ))
    .bind(id)
    .bind(user)
    .bind(principal)
    .fetch_optional(executor)
    .await?;
    row.as_ref().map(map_row).transpose()
}

pub async fn get_by_id_in(tx: &mut WriteTx, id: &str) -> Result<Option<ScheduledAction>, DbError> {
    let row = sqlx::query(&format!("SELECT {COLS} FROM scheduled_actions WHERE id=?"))
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?;
    row.as_ref().map(map_row).transpose()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditOutcome {
    Missing,
    Conflict,
    Saved(Box<ScheduledAction>),
}

pub async fn update(
    pool: &Pool,
    owner: &ScheduleOwner,
    id: &str,
    expected_generation: &str,
    edit: EditAction,
) -> Result<EditOutcome, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let result = update_in(&mut tx, owner, id, expected_generation, &edit).await?;
    tx.commit().await?;
    Ok(result)
}

pub async fn update_in(
    tx: &mut WriteTx,
    owner: &ScheduleOwner,
    id: &str,
    expected_generation: &str,
    edit: &EditAction,
) -> Result<EditOutcome, DbError> {
    let Some(previous) = get_in(tx, owner, id).await? else {
        return Ok(EditOutcome::Missing);
    };
    if previous.configuration_generation != expected_generation {
        return Ok(EditOutcome::Conflict);
    };
    let linked = match &edit.linked_session_id {
        Some(linked) => linked.clone(),
        None if previous.dot_id != edit.dot_id || !edit.reuse_conversation => None,
        None => previous.last_session_id.clone(),
    };
    validate_configuration_in(
        tx,
        owner,
        previous.dot_id.as_deref(),
        ScheduleBinding {
            dot_id: edit.dot_id.as_deref(),
            model: edit.model.as_deref(),
            version: edit.agent_version,
            reuse: edit.reuse_conversation,
            linked: linked.as_deref(),
        },
    )
    .await?;
    let changed = previous.dot_id != edit.dot_id
        || previous.prompt != edit.prompt
        || previous.model != edit.model
        || previous.agent_version != edit.agent_version
        || previous.cron != edit.cron
        || previous.timezone != edit.timezone
        || previous.tools_enabled != edit.tools_enabled
        || previous.reuse_conversation != edit.reuse_conversation
        || previous.reuse_rounds != edit.reuse_rounds
        || (edit.linked_session_id.is_some() && previous.last_session_id != linked);
    if !changed && previous.name == edit.name {
        return Ok(EditOutcome::Saved(Box::new(previous)));
    };
    let generation = if changed {
        Uuid::new_v4().to_string()
    } else {
        previous.configuration_generation.clone()
    };
    sqlx::query("UPDATE scheduled_actions SET dot_id=?,last_session_id=?,name=?,prompt=?,model=?,agent_version=?,cron=?,timezone=?,tools_enabled=?,reuse_conversation=?,reuse_rounds=?,next_run_at=?,configuration_generation=?,updated_at=? WHERE id=?")
        .bind(&edit.dot_id).bind(linked).bind(&edit.name).bind(&edit.prompt).bind(&edit.model).bind(edit.agent_version).bind(&edit.cron).bind(&edit.timezone)
        .bind(edit.tools_enabled).bind(edit.reuse_conversation).bind(edit.reuse_rounds).bind(if previous.enabled {(if changed {edit.next_run_at} else {previous.next_run_at}).map(|v|v.to_string())} else {None})
        .bind(generation).bind(Timestamp::now().to_string()).bind(id).execute(&mut **tx).await?;
    Ok(EditOutcome::Saved(Box::new(
        get_in(tx, owner, id)
            .await?
            .expect("updated schedule exists"),
    )))
}

pub async fn set_enabled(
    pool: &Pool,
    owner: &ScheduleOwner,
    id: &str,
    expected_generation: &str,
    enabled: bool,
    next: Option<Timestamp>,
) -> Result<EditOutcome, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let result = set_enabled_in(&mut tx, owner, id, expected_generation, enabled, next).await?;
    tx.commit().await?;
    Ok(result)
}

pub async fn set_enabled_in(
    tx: &mut WriteTx,
    owner: &ScheduleOwner,
    id: &str,
    expected_generation: &str,
    enabled: bool,
    next: Option<Timestamp>,
) -> Result<EditOutcome, DbError> {
    let Some(row) = get_in(tx, owner, id).await? else {
        return Ok(EditOutcome::Missing);
    };
    if row.configuration_generation != expected_generation {
        return Ok(EditOutcome::Conflict);
    };
    if row.enabled == enabled {
        return Ok(EditOutcome::Saved(Box::new(row)));
    };
    sqlx::query("UPDATE scheduled_actions SET enabled=?,next_run_at=?,updated_at=? WHERE id=?")
        .bind(enabled)
        .bind(if enabled {
            next.map(|v| v.to_string())
        } else {
            None
        })
        .bind(Timestamp::now().to_string())
        .bind(id)
        .execute(&mut **tx)
        .await?;
    Ok(EditOutcome::Saved(Box::new(
        get_in(tx, owner, id)
            .await?
            .expect("updated schedule exists"),
    )))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteOutcome {
    Deleted,
    Missing,
    Pending,
}

pub async fn delete(
    pool: &Pool,
    owner: &ScheduleOwner,
    id: &str,
) -> Result<DeleteOutcome, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let result = delete_in(&mut tx, owner, id).await?;
    tx.commit().await?;
    Ok(result)
}

pub async fn delete_in(
    tx: &mut WriteTx,
    owner: &ScheduleOwner,
    id: &str,
) -> Result<DeleteOutcome, DbError> {
    if get_in(tx, owner, id).await?.is_none() {
        return Ok(DeleteOutcome::Missing);
    };
    let pending:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM scheduled_runs r LEFT JOIN chat_turns t ON t.id=r.assistant_turn_id WHERE r.action_id=? AND (t.status IN ('in_progress','suspended') OR (r.status IS NULL AND r.assistant_turn_id IS NULL)))")
        .bind(id).fetch_one(&mut **tx).await?;
    if pending {
        return Ok(DeleteOutcome::Pending);
    };
    sqlx::query("DELETE FROM scheduled_actions WHERE id=?")
        .bind(id)
        .execute(&mut **tx)
        .await?;
    Ok(DeleteOutcome::Deleted)
}

/// Every enabled action whose `next_run_at` is at or before `now` —
/// i.e. due to fire. The `now` bind is floored to whole seconds so it
/// shares the canonical `…:SSZ` RFC3339 format the minute-aligned
/// `next_run_at` uses, keeping the string comparison chronologically
/// exact (no sub-second drift across the boundary).
pub async fn due_actions(pool: &Pool, now: Timestamp) -> Result<Vec<ScheduledAction>, DbError> {
    let now_floored = now.strftime("%Y-%m-%dT%H:%M:%SZ").to_string();
    let sql = format!(
        "SELECT {COLS} FROM scheduled_actions \
         WHERE enabled = 1 AND next_run_at IS NOT NULL AND next_run_at <= ? \
         ORDER BY next_run_at ASC"
    );
    let rows = sqlx::query(&sql).bind(now_floored).fetch_all(pool).await?;
    rows.iter().map(map_row).collect()
}

// ---------------------------------------------------------------------------
// Run history (scheduled_runs, migration 0064)

/// One recorded fire of a scheduled action.
///
/// `session_id` is the chat the run opened — the link the user comes to
/// `/scheduled` for. An action with `reuse_conversation` points every run at
/// the same session by design; the runs still list separately, because when
/// each fired and whether it worked differ even when the conversation does not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledRun {
    pub id: String,
    pub action_id: String,
    pub configuration_generation: Option<String>,
    pub agent_version: Option<i64>,
    pub tools_enabled: bool,
    pub history_limit: Option<usize>,
    pub fired_at: Timestamp,
    /// `None` only while the run is in flight; `"ok"` or `"error"` once done.
    pub status: Option<String>,
    pub session_id: Option<String>,
    pub assistant_turn_id: Option<String>,
    /// The run opened a chat that the user has since deleted. Kept apart from
    /// `session_id = None` (the run never opened one) so the history can say
    /// which of the two happened.
    pub chat_deleted: bool,
    pub error: Option<String>,
    pub created_at: Timestamp,
}

const RUN_COLS: &str = "r.id, r.action_id, r.configuration_generation, r.agent_version, r.tools_enabled, r.history_limit, r.fired_at, r.status, r.session_id, r.assistant_turn_id, r.error, r.created_at, \
     (r.session_id IS NOT NULL AND s.id IS NULL) AS chat_deleted";

/// What a run left pending by a dead process is closed with at startup.
pub const RUN_INTERRUPTED: &str = "interrupted — the server stopped before this run finished";

fn map_run(row: &SqliteRow) -> Result<ScheduledRun, DbError> {
    Ok(ScheduledRun {
        id: row.try_get("id")?,
        action_id: row.try_get("action_id")?,
        configuration_generation: row.try_get("configuration_generation")?,
        agent_version: row.try_get("agent_version")?,
        tools_enabled: row.try_get("tools_enabled")?,
        history_limit: row
            .try_get::<Option<i64>, _>("history_limit")?
            .map(usize::try_from)
            .transpose()
            .map_err(|_| invalid("The scheduled history limit is invalid"))?,
        fired_at: parse_ts(row.try_get("fired_at")?, "fired_at")?,
        status: row.try_get("status")?,
        session_id: row.try_get("session_id")?,
        assistant_turn_id: row.try_get("assistant_turn_id")?,
        chat_deleted: row.try_get("chat_deleted")?,
        error: row.try_get("error")?,
        created_at: parse_ts(row.try_get("created_at")?, "created_at")?,
    })
}

/// Record the start of a run (status left NULL until [`finish_run`]). Returns
/// the new run id. Not owner-scoped: the worker has already loaded the action.
pub async fn record_run_start(pool: &Pool, action_id: &str) -> Result<String, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let action = get_by_id_in(&mut tx, action_id)
        .await?
        .ok_or_else(|| invalid("The scheduled action is unavailable"))?;
    let run = insert_run_in(&mut tx, &action, Timestamp::now()).await?;
    tx.commit().await?;
    Ok(run.id)
}

async fn insert_run_in(
    tx: &mut WriteTx,
    action: &ScheduledAction,
    fired_at: Timestamp,
) -> Result<ScheduledRun, DbError> {
    let id = Uuid::new_v4().to_string();
    let history = if action.reuse_conversation {
        Some(
            action
                .reuse_rounds
                .checked_mul(2)
                .ok_or_else(|| invalid("The scheduled history limit is too large"))?,
        )
    } else {
        None
    };
    sqlx::query("INSERT INTO scheduled_runs(id,action_id,fired_at,created_at,configuration_generation,agent_version,tools_enabled,history_limit) VALUES(?,?,?,?,?,?,?,?)")
        .bind(&id).bind(&action.id).bind(fired_at.to_string()).bind(Timestamp::now().to_string()).bind(&action.configuration_generation).bind(action.agent_version)
        .bind(action.tools_enabled).bind(history).execute(&mut **tx).await?;
    Ok(get_for_run_in(tx, &id)
        .await?
        .expect("inserted scheduled run exists")
        .1)
}

pub async fn claim_due_in(
    tx: &mut WriteTx,
    action_id: &str,
    expected_next_run_at: Timestamp,
    expected_generation: &str,
    next_run_at: Option<Timestamp>,
    fired_at: Timestamp,
) -> Result<Option<ScheduledRun>, DbError> {
    let Some(action) = get_by_id_in(tx, action_id).await? else {
        return Ok(None);
    };
    if !action.enabled
        || action.configuration_generation != expected_generation
        || action.next_run_at != Some(expected_next_run_at)
        || expected_next_run_at > fired_at
    {
        return Ok(None);
    };
    sqlx::query("UPDATE scheduled_actions SET next_run_at=?,updated_at=? WHERE id=?")
        .bind(next_run_at.map(|v| v.to_string()))
        .bind(fired_at.to_string())
        .bind(action_id)
        .execute(&mut **tx)
        .await?;
    Ok(Some(insert_run_in(tx, &action, fired_at).await?))
}

pub async fn get_for_run_in(
    tx: &mut WriteTx,
    run_id: &str,
) -> Result<Option<(ScheduledAction, ScheduledRun)>, DbError> {
    let row=sqlx::query(&format!("SELECT {RUN_COLS} FROM scheduled_runs r LEFT JOIN chat_sessions s ON s.id=r.session_id WHERE r.id=?")).bind(run_id).fetch_optional(&mut **tx).await?;
    let Some(row) = row else { return Ok(None) };
    let run = map_run(&row)?;
    Ok(get_by_id_in(tx, &run.action_id)
        .await?
        .map(|action| (action, run)))
}

pub async fn attach_run_turn(
    pool: &Pool,
    run_id: &str,
    session_id: &str,
    assistant_turn_id: &str,
    tools_enabled: bool,
    history_limit: Option<usize>,
) -> Result<(), DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    attach_run_turn_in(
        &mut tx,
        run_id,
        session_id,
        assistant_turn_id,
        tools_enabled,
        history_limit,
    )
    .await?;
    tx.commit().await
}

pub async fn attach_run_turn_in(
    tx: &mut WriteTx,
    run_id: &str,
    session_id: &str,
    assistant_turn_id: &str,
    tools_enabled: bool,
    history_limit: Option<usize>,
) -> Result<(), DbError> {
    let (action, run) = get_for_run_in(tx, run_id)
        .await?
        .ok_or_else(|| invalid("The pending scheduled run is unavailable"))?;
    if run.status.is_some()
        || run.assistant_turn_id.is_some()
        || run.configuration_generation.as_deref() != Some(action.configuration_generation.as_str())
        || run.agent_version != action.agent_version
    {
        return Err(invalid("The scheduled run configuration has changed"));
    }
    let valid_session = match &action.owner {
        ScheduleOwner::User(user) => {
            dot_sessions::session_matches(tx, user, session_id, action.dot_id.as_deref()).await?
        }
        ScheduleOwner::Principal(principal) => run_sessions::get_private_principal_session_in(
            tx,
            principal,
            session_id,
            action
                .agent_version
                .ok_or_else(|| invalid("The scheduled version is unavailable"))?,
        )
        .await?
        .is_some(),
    };
    if !valid_session {
        return Err(invalid(
            "The scheduled run conversation has a different owner or version",
        ));
    };
    let valid_turn:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM chat_turns WHERE id=? AND session_id=? AND role='assistant' AND status='in_progress')")
        .bind(assistant_turn_id).bind(session_id).fetch_one(&mut **tx).await?;
    if !valid_turn {
        return Err(invalid("The scheduled run assistant turn is unavailable"));
    };
    let history = history_limit
        .map(i64::try_from)
        .transpose()
        .map_err(|_| invalid("The scheduled history limit is too large"))?;
    sqlx::query("UPDATE scheduled_runs SET session_id=?,assistant_turn_id=?,tools_enabled=?,history_limit=? WHERE id=?")
        .bind(session_id).bind(assistant_turn_id).bind(tools_enabled).bind(history).bind(run_id).execute(&mut **tx).await?;
    sqlx::query("UPDATE scheduled_actions SET last_session_id=? WHERE id=? AND (SELECT rowid FROM scheduled_runs WHERE id=?)=(SELECT rowid FROM scheduled_runs WHERE action_id=? ORDER BY rtrim(fired_at,'Z') DESC,rowid DESC LIMIT 1)")
        .bind(session_id).bind(&action.id).bind(run_id).bind(&action.id).execute(&mut **tx).await?;
    Ok(())
}

/// Close every run still pending at startup: no worker survives a restart, so
/// nothing will ever finish them, and left alone they read as running forever.
/// An action whose newest run is one of them gets the same outcome on its list
/// row, which would otherwise still describe the run before (newest as
/// [`list_runs`] orders). Returns how many runs were closed.
///
/// Only sound before the scheduler starts firing: a run pending *then* is
/// necessarily a previous process's.
pub async fn sweep_interrupted_runs(pool: &Pool) -> Result<u64, DbError> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        r#"UPDATE scheduled_actions
           SET last_status = 'error', last_error = ?1, last_run_at = newest.fired_at
           FROM (SELECT r.action_id, r.fired_at
                 FROM scheduled_runs r
                 WHERE r.status IS NULL
                   AND r.rowid = (SELECT rowid FROM scheduled_runs
                                  WHERE action_id = r.action_id
                                  ORDER BY rtrim(fired_at, 'Z') DESC, rowid DESC
                                  LIMIT 1)) AS newest
           WHERE scheduled_actions.id = newest.action_id"#,
    )
    .bind(RUN_INTERRUPTED)
    .execute(&mut *tx)
    .await?;
    let done =
        sqlx::query("UPDATE scheduled_runs SET status = 'error', error = ? WHERE status IS NULL")
            .bind(RUN_INTERRUPTED)
            .execute(&mut *tx)
            .await?;
    tx.commit().await?;
    let reconciled = crate::server::background::reconcile_interrupted_waiting(
        pool,
        aiplane_core::server::db::usage::UsageSource::Scheduled,
    )
    .await?;
    Ok(done.rows_affected() + reconciled)
}

/// Record the outcome of a run started by [`record_run_start`]. `session_id`
/// is kept even on error so the user can inspect the partial conversation;
/// `None` keeps the one [`attach_run_turn`] already linked.
pub async fn finish_run(
    pool: &Pool,
    run_id: &str,
    status: &str,
    session_id: Option<&str>,
    error: Option<&str>,
) -> Result<(), DbError> {
    sqlx::query(
        "UPDATE scheduled_runs SET status = ?, session_id = COALESCE(?, session_id), error = ? \
         WHERE id = ?",
    )
    .bind(status)
    .bind(session_id)
    .bind(error)
    .bind(run_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// An action's most recent runs, newest first, capped at `limit`.
///
/// Ordered on `rtrim(fired_at, 'Z')`: the stored RFC 3339 text has only as
/// many fractional digits as the instant needs and sorts wrongly within one
/// second (`…00.1Z` after `…00.123Z`); without its `Z` it sorts right
/// (`aiplane_core::server::db::window_key`). The tiebreaker is `rowid DESC`
/// (SQLite's monotonic insertion order), not `id`, a random UUID.
pub async fn list_runs(
    pool: &Pool,
    action_id: &str,
    limit: i64,
) -> Result<Vec<ScheduledRun>, DbError> {
    list_runs_with(pool, action_id, limit).await
}

pub async fn list_runs_in(
    tx: &mut WriteTx,
    action_id: &str,
    limit: i64,
) -> Result<Vec<ScheduledRun>, DbError> {
    list_runs_with(&mut **tx, action_id, limit).await
}

async fn list_runs_with<'e, E: sqlx::Executor<'e, Database = sqlx::Sqlite>>(
    executor: E,
    action_id: &str,
    limit: i64,
) -> Result<Vec<ScheduledRun>, DbError> {
    let sql = format!(
        "SELECT {RUN_COLS} FROM scheduled_runs r \
         LEFT JOIN chat_sessions s ON s.id = r.session_id \
         WHERE r.action_id = ? \
         ORDER BY rtrim(r.fired_at, 'Z') DESC, r.rowid DESC LIMIT ?"
    );
    let rows = sqlx::query(&sql)
        .bind(action_id)
        .bind(limit)
        .fetch_all(executor)
        .await?;
    rows.iter().map(map_run).collect()
}

/// How many runs, and how many distinct chats, each of a user's actions has.
///
/// Both numbers in one grouped query rather than one per row: the list page
/// renders every action, and the two differ for a `reuse_conversation`
/// schedule (many runs, one chat) — which is exactly the distinction the
/// row's link has to make.
pub async fn run_counts_for_user(
    pool: &Pool,
    user_id: &str,
) -> Result<std::collections::HashMap<String, (i64, i64)>, DbError> {
    run_counts_with(pool, &ScheduleOwner::User(user_id.into())).await
}

pub async fn run_counts_for_owner_in(
    tx: &mut WriteTx,
    owner: &ScheduleOwner,
) -> Result<std::collections::HashMap<String, (i64, i64)>, DbError> {
    run_counts_with(&mut **tx, owner).await
}

async fn run_counts_with<'e, E: sqlx::Executor<'e, Database = sqlx::Sqlite>>(
    executor: E,
    owner: &ScheduleOwner,
) -> Result<std::collections::HashMap<String, (i64, i64)>, DbError> {
    let (user, principal) = owner.columns();
    let rows=sqlx::query("SELECT r.action_id,COUNT(*) AS runs,COUNT(DISTINCT r.session_id) AS chats FROM scheduled_runs r JOIN scheduled_actions a ON a.id=r.action_id WHERE a.user_id IS ? AND a.principal_id IS ? GROUP BY r.action_id")
        .bind(user).bind(principal).fetch_all(executor).await?;
    rows.iter()
        .map(|row| {
            Ok((
                row.try_get("action_id")?,
                (row.try_get("runs")?, row.try_get("chats")?),
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn fresh_db() -> Pool {
        // `open` runs the migration set, which includes 0021 (the
        // scheduled_actions table).
        aiplane_core::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap()
    }

    async fn seed_user(pool: &Pool, id: &str) {
        // The FK to users(id) means we need a row to attach actions to.
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

    async fn fixture_attach_run_turn(
        pool: &Pool,
        run_id: &str,
        session_id: &str,
        turn_id: &str,
        tools_enabled: bool,
        history_limit: Option<usize>,
    ) -> Result<(), DbError> {
        let owner: String = sqlx::query_scalar("SELECT a.user_id FROM scheduled_actions a JOIN scheduled_runs r ON r.action_id = a.id WHERE r.id = ?").bind(run_id).fetch_one(pool).await?;
        let session = session_core::db::create_session(pool, &owner).await?;
        sqlx::query("UPDATE chat_sessions SET id = ? WHERE id = ?")
            .bind(session_id)
            .bind(&session.id)
            .execute(pool)
            .await?;
        session_core::db::create_assistant_turn_in_progress(pool, session_id, turn_id, "qwen")
            .await?;
        attach_run_turn(
            pool,
            run_id,
            session_id,
            turn_id,
            tools_enabled,
            history_limit,
        )
        .await?;
        session_core::db::delete_session(pool, &owner, session_id).await?;
        Ok(())
    }

    fn sample(user_id: &str, next: Option<Timestamp>) -> NewAction {
        NewAction {
            dot_id: None,
            linked_session_id: None,
            owner: ScheduleOwner::User(user_id.to_string()),
            created_by: user_id.into(),
            agent_version: None,
            name: "Daily digest".to_string(),
            prompt: "Summarize the news.".to_string(),
            model: Some("qwen".to_string()),
            cron: "0 9 * * *".to_string(),
            timezone: "Europe/Berlin".to_string(),
            tools_enabled: true,
            reuse_conversation: false,
            reuse_rounds: 5,
            next_run_at: next,
        }
    }

    async fn fixture_source_summary(
        pool: &Pool,
        id: &str,
        status: &str,
        session_id: Option<&str>,
        next_run_at: Option<Timestamp>,
        error: Option<&str>,
    ) -> Result<(), DbError> {
        sqlx::query(
            r#"UPDATE scheduled_actions
           SET last_run_at = ?, last_status = ?, last_session_id = COALESCE(?, last_session_id),
               last_error = ?, next_run_at = ?, updated_at = ?
           WHERE id = ?"#,
        )
        .bind(Timestamp::now().to_string())
        .bind(status)
        .bind(session_id)
        .bind(error)
        .bind(next_run_at.map(|t| t.to_string()))
        .bind(Timestamp::now().to_string())
        .bind(id)
        .execute(pool)
        .await?;
        Ok(())
    }

    #[tokio::test]
    async fn create_then_get_round_trips() {
        let pool = fresh_db().await;
        seed_user(&pool, "u1").await;
        let next = "2026-06-19T07:00:00Z".parse::<Timestamp>().unwrap();
        let created = create(&pool, sample("u1", Some(next))).await.unwrap();
        let got = get(&pool, &ScheduleOwner::User("u1".into()), &created.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got, created);
        assert_eq!(got.next_run_at, Some(next));
        assert!(got.enabled && got.tools_enabled);
    }

    #[tokio::test]
    async fn get_is_scoped_to_owner() {
        let pool = fresh_db().await;
        seed_user(&pool, "u1").await;
        seed_user(&pool, "u2").await;
        let created = create(&pool, sample("u1", None)).await.unwrap();
        assert!(
            get(&pool, &ScheduleOwner::User("u2".into()), &created.id)
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            delete(&pool, &ScheduleOwner::User("u2".into()), &created.id)
                .await
                .unwrap(),
            DeleteOutcome::Missing
        );
        assert!(
            get(&pool, &ScheduleOwner::User("u1".into()), &created.id)
                .await
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn due_actions_selects_only_enabled_and_due() {
        let pool = fresh_db().await;
        seed_user(&pool, "u1").await;
        let past = "2026-06-19T06:00:00Z".parse::<Timestamp>().unwrap();
        let future = "2999-01-01T00:00:00Z".parse::<Timestamp>().unwrap();
        let due = create(&pool, sample("u1", Some(past))).await.unwrap();
        let not_yet = create(&pool, sample("u1", Some(future))).await.unwrap();
        let paused = create(&pool, sample("u1", Some(past))).await.unwrap();
        set_enabled(
            &pool,
            &paused.owner,
            &paused.id,
            &paused.configuration_generation,
            false,
            None,
        )
        .await
        .unwrap();

        let now = "2026-06-19T06:30:00Z".parse::<Timestamp>().unwrap();
        let ids: Vec<String> = due_actions(&pool, now)
            .await
            .unwrap()
            .into_iter()
            .map(|a| a.id)
            .collect();
        assert_eq!(ids, vec![due.id.clone()]);
        assert!(!ids.contains(&not_yet.id));
        assert!(!ids.contains(&paused.id));
    }

    #[tokio::test]
    async fn runs_record_start_then_outcome_newest_first() {
        let pool = fresh_db().await;
        seed_user(&pool, "u1").await;
        let a = create(&pool, sample("u1", None)).await.unwrap();

        let first = record_run_start(&pool, &a.id).await.unwrap();
        // A run is visible the moment it starts, pending, so a long run
        // doesn't look like it never happened.
        let pending = list_runs(&pool, &a.id, 50).await.unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].status, None);
        assert_eq!(pending[0].session_id, None);

        finish_run(&pool, &first, "ok", Some("sess-1"), None)
            .await
            .unwrap();
        let second = record_run_start(&pool, &a.id).await.unwrap();
        finish_run(&pool, &second, "error", Some("sess-2"), Some("boom"))
            .await
            .unwrap();

        let runs = list_runs(&pool, &a.id, 50).await.unwrap();
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].id, second);
        assert_eq!(runs[0].status.as_deref(), Some("error"));
        assert_eq!(runs[0].error.as_deref(), Some("boom"));
        // The session is kept on a failed run: the partial conversation is
        // usually where the reason is.
        assert_eq!(runs[0].session_id.as_deref(), Some("sess-2"));
        assert_eq!(runs[1].id, first);
        assert_eq!(runs[1].status.as_deref(), Some("ok"));
        assert_eq!(list_runs(&pool, &a.id, 1).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn run_counts_separate_runs_from_chats() {
        let pool = fresh_db().await;
        seed_user(&pool, "u1").await;
        seed_user(&pool, "u2").await;
        let fresh_each_time = create(&pool, sample("u1", None)).await.unwrap();
        let reuses = create(&pool, sample("u1", None)).await.unwrap();
        let other_owner = create(&pool, sample("u2", None)).await.unwrap();

        for session in ["sess-a", "sess-b"] {
            let run = record_run_start(&pool, &fresh_each_time.id).await.unwrap();
            finish_run(&pool, &run, "ok", Some(session), None)
                .await
                .unwrap();
        }
        // A reusing schedule fires repeatedly into one conversation: three
        // runs, one chat. That is the distinction the list row's link makes.
        for _ in 0..3 {
            let run = record_run_start(&pool, &reuses.id).await.unwrap();
            finish_run(&pool, &run, "ok", Some("sess-shared"), None)
                .await
                .unwrap();
        }
        let run = record_run_start(&pool, &other_owner.id).await.unwrap();
        finish_run(&pool, &run, "ok", Some("sess-other"), None)
            .await
            .unwrap();

        let counts = run_counts_for_user(&pool, "u1").await.unwrap();
        assert_eq!(counts.get(&fresh_each_time.id), Some(&(2, 2)));
        assert_eq!(counts.get(&reuses.id), Some(&(3, 1)));
        // Another user's action never appears in this user's counts.
        assert_eq!(counts.get(&other_owner.id), None);
    }

    /// The run links its chat the moment the session is opened, not when it
    /// finishes: a run that never reaches `finish_run` (a panic, a restart)
    /// must still point at the conversation it was writing.
    #[tokio::test]
    async fn a_pending_run_links_its_chat_as_soon_as_the_session_opens() {
        let pool = fresh_db().await;
        seed_user(&pool, "u1").await;
        let a = create(&pool, sample("u1", None)).await.unwrap();
        let run = record_run_start(&pool, &a.id).await.unwrap();

        fixture_attach_run_turn(&pool, &run, "sess-1", "turn-sess-1", false, None)
            .await
            .unwrap();

        let runs = list_runs(&pool, &a.id, 50).await.unwrap();
        assert_eq!(runs[0].status, None);
        assert_eq!(runs[0].session_id.as_deref(), Some("sess-1"));
        // A reusing schedule continues in this chat even if the run dies here.
        let a = get(&pool, &ScheduleOwner::User("u1".into()), &a.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(a.last_session_id.as_deref(), Some("sess-1"));

        // Closing the run without a session (it crashed before it knew one)
        // keeps the link rather than erasing it.
        finish_run(&pool, &run, "error", None, Some("crashed"))
            .await
            .unwrap();
        fixture_source_summary(&pool, &a.id, "error", None, None, Some("crashed"))
            .await
            .unwrap();
        let runs = list_runs(&pool, &a.id, 50).await.unwrap();
        assert_eq!(runs[0].session_id.as_deref(), Some("sess-1"));
        let a = get(&pool, &ScheduleOwner::User("u1".into()), &a.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(a.last_session_id.as_deref(), Some("sess-1"));
    }

    #[tokio::test]
    async fn list_runs_reports_a_deleted_chat() {
        let pool = fresh_db().await;
        seed_user(&pool, "u1").await;
        let a = create(&pool, sample("u1", None)).await.unwrap();
        let kept = session_core::db::create_session(&pool, "u1").await.unwrap();
        let deleted = session_core::db::create_session(&pool, "u1").await.unwrap();
        for session in [&kept.id, &deleted.id] {
            let run = record_run_start(&pool, &a.id).await.unwrap();
            finish_run(&pool, &run, "ok", Some(session), None)
                .await
                .unwrap();
        }
        let no_chat = record_run_start(&pool, &a.id).await.unwrap();
        finish_run(&pool, &no_chat, "error", None, Some("usage limit"))
            .await
            .unwrap();
        assert!(
            session_core::db::delete_session(&pool, "u1", &deleted.id)
                .await
                .unwrap()
        );

        let runs = list_runs(&pool, &a.id, 50).await.unwrap();
        let by_session = |id: Option<&str>| {
            runs.iter()
                .find(|r| r.session_id.as_deref() == id)
                .unwrap()
                .chat_deleted
        };
        assert!(!by_session(Some(&kept.id)));
        assert!(by_session(Some(&deleted.id)));
        // A run that never opened a chat has nothing that could be deleted.
        assert!(!by_session(None));
    }

    #[tokio::test]
    async fn startup_sweep_closes_runs_a_dead_process_left_pending() {
        let pool = fresh_db().await;
        seed_user(&pool, "u1").await;
        let a = create(&pool, sample("u1", None)).await.unwrap();
        let finished = record_run_start(&pool, &a.id).await.unwrap();
        finish_run(&pool, &finished, "ok", Some("sess-1"), None)
            .await
            .unwrap();
        fixture_source_summary(&pool, &a.id, "ok", Some("sess-1"), None, None)
            .await
            .unwrap();
        let orphan = record_run_start(&pool, &a.id).await.unwrap();
        fixture_attach_run_turn(&pool, &orphan, "sess-2", "turn-sess-2", false, None)
            .await
            .unwrap();
        // An older orphan on an action whose later run finished: that list row
        // describes the later run and must stay as it is.
        let b = create(&pool, sample("u1", None)).await.unwrap();
        let old_orphan = record_run_start(&pool, &b.id).await.unwrap();
        let later = record_run_start(&pool, &b.id).await.unwrap();
        finish_run(&pool, &later, "ok", Some("sess-b"), None)
            .await
            .unwrap();
        fixture_source_summary(&pool, &b.id, "ok", Some("sess-b"), None, None)
            .await
            .unwrap();

        assert_eq!(sweep_interrupted_runs(&pool).await.unwrap(), 2);

        let runs = list_runs(&pool, &a.id, 50).await.unwrap();
        let orphan_run = runs.iter().find(|r| r.id == orphan).unwrap();
        assert_eq!(orphan_run.status.as_deref(), Some("error"));
        assert_eq!(orphan_run.error.as_deref(), Some(RUN_INTERRUPTED));
        assert_eq!(orphan_run.session_id.as_deref(), Some("sess-2"));
        let finished = runs.iter().find(|r| r.id == finished).unwrap();
        assert_eq!(finished.status.as_deref(), Some("ok"));
        assert_eq!(finished.error, None);
        let a_row = get(&pool, &ScheduleOwner::User("u1".into()), &a.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(a_row.last_status.as_deref(), Some("error"));
        assert_eq!(a_row.last_error.as_deref(), Some(RUN_INTERRUPTED));
        assert_eq!(a_row.last_session_id.as_deref(), Some("sess-2"));
        assert_eq!(a_row.last_run_at, Some(orphan_run.fired_at));

        let b_row = get(&pool, &ScheduleOwner::User("u1".into()), &b.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(b_row.last_status.as_deref(), Some("ok"));
        assert_eq!(b_row.last_error, None);
        let b_runs = list_runs(&pool, &b.id, 50).await.unwrap();
        let old_orphan = b_runs.iter().find(|r| r.id == old_orphan).unwrap();
        assert_eq!(old_orphan.status.as_deref(), Some("error"));

        assert_eq!(
            sweep_interrupted_runs(&pool).await.unwrap(),
            0,
            "a second pass finds nothing left to close"
        );
    }

    /// `fired_at` is RFC 3339 text with only as many fractional digits as the
    /// instant needs, so within one second `…00.1Z` sorts after `…00.123Z`
    /// although it is earlier. Two runs fired in the same second made
    /// `startup_sweep_closes_runs_a_dead_process_left_pending` flaky.
    #[tokio::test]
    async fn the_newest_run_is_the_one_fired_last_even_within_one_second() {
        let pool = fresh_db().await;
        seed_user(&pool, "u1").await;
        let a = create(&pool, sample("u1", None)).await.unwrap();
        let earlier = record_run_start(&pool, &a.id).await.unwrap();
        let later = record_run_start(&pool, &a.id).await.unwrap();
        for (run, at) in [
            (&earlier, "2026-10-04T10:00:00.1Z"),
            (&later, "2026-10-04T10:00:00.123Z"),
        ] {
            sqlx::query("UPDATE scheduled_runs SET fired_at = ? WHERE id = ?")
                .bind(at)
                .bind(run)
                .execute(&pool)
                .await
                .unwrap();
        }
        finish_run(&pool, &later, "ok", Some("sess"), None)
            .await
            .unwrap();
        fixture_source_summary(&pool, &a.id, "ok", Some("sess"), None, None)
            .await
            .unwrap();

        let runs = list_runs(&pool, &a.id, 50).await.unwrap();
        let order: Vec<&str> = runs.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(order, [later.as_str(), earlier.as_str()]);
        assert_eq!(sweep_interrupted_runs(&pool).await.unwrap(), 1);
        let row = get(&pool, &ScheduleOwner::User("u1".into()), &a.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            row.last_status.as_deref(),
            Some("ok"),
            "the row still describes the later run, which finished"
        );
    }

    #[tokio::test]
    async fn linked_chat_updates_are_atomic_and_owner_scoped() {
        let pool = fresh_db().await;
        seed_user(&pool, "u1").await;
        seed_user(&pool, "u2").await;
        let chat = session_core::db::create_session(&pool, "u1").await.unwrap();
        let mut new = sample("u1", None);
        new.reuse_conversation = true;
        new.linked_session_id = Some(chat.id.clone());
        let source = create(&pool, new).await.unwrap();
        assert_eq!(source.last_session_id.as_deref(), Some(chat.id.as_str()));
        let edit = || EditAction {
            dot_id: None,
            linked_session_id: Some(None),
            name: source.name.clone(),
            prompt: source.prompt.clone(),
            model: source.model.clone(),
            agent_version: source.agent_version,
            tools_enabled: source.tools_enabled,
            reuse_conversation: true,
            reuse_rounds: source.reuse_rounds,
            cron: source.cron.clone(),
            timezone: source.timezone.clone(),
            next_run_at: source.next_run_at,
        };
        assert_eq!(
            update(
                &pool,
                &ScheduleOwner::User("u2".into()),
                &source.id,
                &source.configuration_generation,
                edit()
            )
            .await
            .unwrap(),
            EditOutcome::Missing
        );
        assert_eq!(
            get(&pool, &ScheduleOwner::User("u1".into()), &source.id)
                .await
                .unwrap()
                .unwrap()
                .last_session_id,
            Some(chat.id)
        );
        assert!(matches!(
            update(
                &pool,
                &source.owner,
                &source.id,
                &source.configuration_generation,
                edit()
            )
            .await
            .unwrap(),
            EditOutcome::Saved(_)
        ));
        assert!(
            get(&pool, &ScheduleOwner::User("u1".into()), &source.id)
                .await
                .unwrap()
                .unwrap()
                .last_session_id
                .is_none()
        );
    }

    #[tokio::test]
    async fn deleting_an_action_deletes_its_runs() {
        let pool = fresh_db().await;
        seed_user(&pool, "u1").await;
        let a = create(&pool, sample("u1", None)).await.unwrap();
        let run = record_run_start(&pool, &a.id).await.unwrap();
        finish_run(&pool, &run, "ok", Some("sess-1"), None)
            .await
            .unwrap();
        assert_eq!(
            delete(&pool, &ScheduleOwner::User("u1".into()), &a.id)
                .await
                .unwrap(),
            DeleteOutcome::Deleted
        );
        assert!(list_runs(&pool, &a.id, 50).await.unwrap().is_empty());
    }
    #[tokio::test]
    async fn correlation_is_owned_atomic_and_keeps_snapshots_until_terminal_delete() {
        let pool = fresh_db().await;
        seed_user(&pool, "u1").await;
        seed_user(&pool, "u2").await;
        let action = create(&pool, sample("u1", None)).await.unwrap();
        let run = record_run_start(&pool, &action.id).await.unwrap();
        assert_eq!(
            delete(&pool, &ScheduleOwner::User("u1".into()), &action.id)
                .await
                .unwrap(),
            DeleteOutcome::Pending
        );
        let other = session_core::db::create_session(&pool, "u2").await.unwrap();
        session_core::db::create_assistant_turn_in_progress(
            &pool,
            &other.id,
            "foreign-turn",
            "qwen",
        )
        .await
        .unwrap();
        assert!(
            attach_run_turn(&pool, &run, &other.id, "foreign-turn", true, Some(4))
                .await
                .is_err()
        );
        assert!(
            get(&pool, &ScheduleOwner::User("u1".into()), &action.id)
                .await
                .unwrap()
                .unwrap()
                .last_session_id
                .is_none()
        );
        let chat = session_core::db::create_session(&pool, "u1").await.unwrap();
        session_core::db::create_assistant_turn_in_progress(&pool, &chat.id, "owned-turn", "qwen")
            .await
            .unwrap();
        assert!(
            attach_run_turn(&pool, "missing", &chat.id, "owned-turn", true, Some(4))
                .await
                .is_err()
        );
        assert!(
            attach_run_turn(&pool, &run, &chat.id, "foreign-turn", true, Some(4))
                .await
                .is_err()
        );
        attach_run_turn(&pool, &run, &chat.id, "owned-turn", true, Some(4))
            .await
            .unwrap();
        let snapshot: (String, i64, i64) = sqlx::query_as("SELECT assistant_turn_id, tools_enabled, history_limit FROM scheduled_runs WHERE id = ?")
            .bind(&run).fetch_one(&pool).await.unwrap();
        assert_eq!(snapshot, ("owned-turn".into(), 1, 4));
        assert!(
            attach_run_turn(&pool, &run, &chat.id, "owned-turn", true, None)
                .await
                .is_err()
        );
        for status in [
            session_core::db::TurnStatus::InProgress,
            session_core::db::TurnStatus::Suspended,
        ] {
            if status == session_core::db::TurnStatus::Suspended {
                session_core::db::finalize_turn(&pool, "owned-turn", status, None)
                    .await
                    .unwrap();
            }
            assert_eq!(
                delete(&pool, &ScheduleOwner::User("u1".into()), &action.id)
                    .await
                    .unwrap(),
                DeleteOutcome::Pending
            );
        }
        session_core::db::finalize_turn(
            &pool,
            "owned-turn",
            session_core::db::TurnStatus::Cancelled,
            None,
        )
        .await
        .unwrap();
        assert_eq!(
            delete(&pool, &ScheduleOwner::User("u1".into()), &action.id)
                .await
                .unwrap(),
            DeleteOutcome::Deleted
        );
    }

    #[tokio::test]
    async fn schedule_claim_is_atomic_generation_due_and_enabled_cas() {
        let pool = fresh_db().await;
        seed_user(&pool, "claim-user").await;
        let due: Timestamp = "2026-10-11T09:00:00Z".parse().unwrap();
        let next: Timestamp = "2026-10-12T09:00:00Z".parse().unwrap();
        let action = create(&pool, sample("claim-user", Some(due)))
            .await
            .unwrap();
        assert_eq!(action.owner, ScheduleOwner::User("claim-user".into()));
        let mut tx = WriteTx::begin(&pool).await.unwrap();
        assert!(
            claim_due_in(&mut tx, &action.id, due, "stale", Some(next), due)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            claim_due_in(
                &mut tx,
                &action.id,
                next,
                &action.configuration_generation,
                Some(next),
                due
            )
            .await
            .unwrap()
            .is_none()
        );
        let run = claim_due_in(
            &mut tx,
            &action.id,
            due,
            &action.configuration_generation,
            Some(next),
            due,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(
            run.configuration_generation.as_deref(),
            Some(action.configuration_generation.as_str())
        );
        assert_eq!(run.agent_version, None);
        let (claimed, stored) = get_for_run_in(&mut tx, &run.id).await.unwrap().unwrap();
        assert_eq!(claimed.next_run_at, Some(next));
        assert_eq!(stored.id, run.id);
        assert!(
            claim_due_in(
                &mut tx,
                &action.id,
                due,
                &action.configuration_generation,
                Some(next),
                due
            )
            .await
            .unwrap()
            .is_none()
        );
        tx.commit().await.unwrap();
        assert_eq!(list_runs(&pool, &action.id, 10).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn schedule_claim_drop_rolls_back_due_and_run_together() {
        let pool = fresh_db().await;
        seed_user(&pool, "rollback-user").await;
        let due: Timestamp = "2026-10-11T09:00:00Z".parse().unwrap();
        let action = create(&pool, sample("rollback-user", Some(due)))
            .await
            .unwrap();
        {
            let mut tx = WriteTx::begin(&pool).await.unwrap();
            assert!(
                claim_due_in(
                    &mut tx,
                    &action.id,
                    due,
                    &action.configuration_generation,
                    None,
                    due
                )
                .await
                .unwrap()
                .is_some()
            );
        }
        assert!(list_runs(&pool, &action.id, 10).await.unwrap().is_empty());
        let mut tx = WriteTx::begin(&pool).await.unwrap();
        assert!(
            claim_due_in(
                &mut tx,
                &action.id,
                due,
                &action.configuration_generation,
                None,
                due
            )
            .await
            .unwrap()
            .is_some()
        );
        tx.commit().await.unwrap();
    }

    async fn organization_schedule_fixture(pool: &Pool) -> (String, i64) {
        use aiplane_agents::db::{agents, organizations, system_principals::NewPrincipal};
        seed_user(pool, "schedule-creator").await;
        let organizations::OrganizationCreateResult::Created(org) = organizations::create(
            pool,
            &organizations::NewOrganization {
                name: "Operations",
                description: "Incident monitoring",
                member_groups: &[],
            },
            "schedule-creator",
        )
        .await
        .unwrap() else {
            panic!("organization fixture")
        };
        let spec = serde_json::json!({"main":{"instructions":"Monitor incidents.","model":"qwen","tools":[]}}).to_string();
        let mut tx = WriteTx::begin(pool).await.unwrap();
        let (agents::DotCreate::Created, Some(dot)) = agents::create_organization_dot_in(
            &mut tx,
            &org.organization.id,
            &NewPrincipal {
                name: "schedule-dot",
                display: "Harald",
                description: "Monitors incidents",
            },
            &spec,
            "schedule-creator",
            4,
        )
        .await
        .unwrap() else {
            panic!("Dot fixture")
        };
        let version = agents::publish_in(&mut tx, &dot.principal.id, &spec, "schedule-creator")
            .await
            .unwrap()
            .unwrap();
        tx.commit().await.unwrap();
        (dot.principal.id, version)
    }

    #[tokio::test]
    async fn organization_schedule_owner_pin_and_creator_lifetime_are_independent() {
        let pool = fresh_db().await;
        let (principal, version) = organization_schedule_fixture(&pool).await;
        let action = create(
            &pool,
            NewAction {
                owner: ScheduleOwner::Principal(principal.clone()),
                created_by: "schedule-creator".into(),
                dot_id: None,
                name: "Incident review".into(),
                prompt: "Review incidents".into(),
                model: None,
                agent_version: Some(version),
                cron: "0 9 * * *".into(),
                timezone: "Europe/Berlin".into(),
                tools_enabled: true,
                reuse_conversation: false,
                reuse_rounds: 5,
                next_run_at: None,
                linked_session_id: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(action.model, None);
        assert_eq!(action.agent_version, Some(version));
        assert_eq!(action.created_by.as_deref(), Some("schedule-creator"));
        sqlx::query("DELETE FROM users WHERE id=?")
            .bind("schedule-creator")
            .execute(&pool)
            .await
            .unwrap();
        let rows = due_actions(&pool, Timestamp::now()).await.unwrap();
        assert!(!rows.iter().any(|row| row.id == action.id));
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM scheduled_actions WHERE id=? AND principal_id=?",
        )
        .bind(&action.id)
        .bind(&principal)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count, 1);
        assert!(
            sqlx::query("DELETE FROM system_principals WHERE id=?")
                .bind(&principal)
                .execute(&pool)
                .await
                .is_err()
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM agents WHERE principal_id=?")
                .bind(&principal)
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
    }

    fn edit_action(action: &ScheduledAction) -> EditAction {
        EditAction {
            dot_id: action.dot_id.clone(),
            name: action.name.clone(),
            prompt: action.prompt.clone(),
            model: action.model.clone(),
            agent_version: action.agent_version,
            cron: action.cron.clone(),
            timezone: action.timezone.clone(),
            tools_enabled: action.tools_enabled,
            reuse_conversation: action.reuse_conversation,
            reuse_rounds: action.reuse_rounds,
            next_run_at: action.next_run_at,
            linked_session_id: None,
        }
    }

    #[tokio::test]
    async fn schedule_configuration_cas_noop_name_and_disable_preserve_generation() {
        let pool = fresh_db().await;
        seed_user(&pool, "config-user").await;
        let due: Timestamp = "2026-10-11T09:00:00Z".parse().unwrap();
        let original = create(&pool, sample("config-user", Some(due)))
            .await
            .unwrap();
        let mut tx = WriteTx::begin(&pool).await.unwrap();
        assert_eq!(
            update_in(
                &mut tx,
                &original.owner,
                &original.id,
                "stale",
                &edit_action(&original)
            )
            .await
            .unwrap(),
            EditOutcome::Conflict
        );
        assert_eq!(
            update_in(
                &mut tx,
                &original.owner,
                &original.id,
                &original.configuration_generation,
                &edit_action(&original)
            )
            .await
            .unwrap(),
            EditOutcome::Saved(Box::new(original.clone()))
        );
        let mut rename = edit_action(&original);
        rename.name = "Renamed".into();
        let EditOutcome::Saved(renamed) = update_in(
            &mut tx,
            &original.owner,
            &original.id,
            &original.configuration_generation,
            &rename,
        )
        .await
        .unwrap() else {
            panic!("rename")
        };
        assert_eq!(
            renamed.configuration_generation,
            original.configuration_generation
        );
        let EditOutcome::Saved(disabled) = set_enabled_in(
            &mut tx,
            &original.owner,
            &original.id,
            &original.configuration_generation,
            false,
            None,
        )
        .await
        .unwrap() else {
            panic!("disable")
        };
        assert_eq!(
            disabled.configuration_generation,
            original.configuration_generation
        );
        assert!(
            claim_due_in(
                &mut tx,
                &original.id,
                due,
                &original.configuration_generation,
                None,
                due
            )
            .await
            .unwrap()
            .is_none()
        );
        let mut changed = edit_action(&disabled);
        changed.prompt = "Authored execution change".into();
        let EditOutcome::Saved(changed) = update_in(
            &mut tx,
            &original.owner,
            &original.id,
            &original.configuration_generation,
            &changed,
        )
        .await
        .unwrap() else {
            panic!("edit")
        };
        assert_ne!(
            changed.configuration_generation,
            original.configuration_generation
        );
        assert!(!changed.enabled);
        assert_eq!(changed.next_run_at, None);
        assert_eq!(
            set_enabled_in(
                &mut tx,
                &original.owner,
                &original.id,
                &original.configuration_generation,
                true,
                Some(due)
            )
            .await
            .unwrap(),
            EditOutcome::Conflict
        );
        drop(tx);
        assert_eq!(
            get(&pool, &original.owner, &original.id).await.unwrap(),
            Some(original)
        );
    }
}
