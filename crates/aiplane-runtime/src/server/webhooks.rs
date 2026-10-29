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

use super::background::AutomationOwner;
use super::scheduled::DeleteOutcome;
use aiplane_agents::db::{WriteTx, agent_sessions};
use aiplane_agents::db::{agents, run_sessions};
use jiff::Timestamp;
use sqlx::Row;
use sqlx::sqlite::SqliteRow;
use uuid::Uuid;

use aiplane_core::server::db::{DbError, Pool, parse_optional_ts, parse_ts};

/// A persisted webhook.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Webhook {
    pub id: String,
    pub owner: AutomationOwner,
    pub created_by: Option<String>,
    pub configuration_generation: String,
    pub personal_agent_id: Option<String>,
    pub name: String,
    pub prompt: String,
    pub model: Option<String>,
    pub agent_version: Option<i64>,
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
    /// different prompt through the rerun handlers.
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
    pub owner: AutomationOwner,
    pub created_by: String,
    pub personal_agent_id: Option<String>,
    pub name: String,
    pub prompt: String,
    pub model: Option<String>,
    pub agent_version: Option<i64>,
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
    pub personal_agent_id: Option<String>,
    pub name: String,
    pub prompt: String,
    pub model: Option<String>,
    pub agent_version: Option<i64>,
    pub tools_enabled: bool,
    pub synchronous: bool,
    pub reuse_conversation: bool,
    pub reuse_rounds: i64,
    pub linked_session_id: Option<Option<String>>,
}

fn map_row(row: &SqliteRow) -> Result<Webhook, DbError> {
    Ok(Webhook {
        id: row.try_get("id")?,
        owner: match (
            row.try_get::<Option<String>, _>("user_id")?,
            row.try_get::<Option<String>, _>("principal_id")?,
        ) {
            (Some(id), None) => AutomationOwner::User(id),
            (None, Some(id)) => AutomationOwner::Principal(id),
            _ => return Err(invalid("The webhook has an invalid owner")),
        },
        created_by: row.try_get("created_by")?,
        configuration_generation: row.try_get("configuration_generation")?,
        personal_agent_id: row.try_get("personal_agent_id")?,
        name: row.try_get("name")?,
        prompt: row.try_get("prompt")?,
        model: row.try_get("model")?,
        agent_version: row.try_get("agent_version")?,
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

const COLS: &str = "id,user_id,principal_id,created_by,configuration_generation,agent_version,personal_agent_id,name,prompt,model,tools_enabled,synchronous,secret_hash,enabled,last_fired_at,last_status,last_session_id,last_error,last_payload,reuse_conversation,reuse_rounds,created_at,updated_at";

fn invalid(message: &str) -> DbError {
    sqlx::Error::Protocol(message.into()).into()
}

async fn validate_in(
    tx: &mut WriteTx,
    owner: &AutomationOwner,
    previous_agent: Option<&str>,
    new: &EditWebhook,
    linked: Option<&str>,
) -> Result<(), DbError> {
    match owner {
        AutomationOwner::User(user) => {
            if new
                .model
                .as_deref()
                .is_none_or(|model| model.trim().is_empty())
                || new.agent_version.is_some()
            {
                return Err(invalid("The user webhook requires a model"));
            };
            if !agent_sessions::binding_valid(
                tx,
                user,
                new.personal_agent_id.as_deref(),
                previous_agent,
            )
            .await?
            {
                return Err(invalid("The selected agent is not owned and active"));
            };
            if let Some(session) = linked
                && (!new.reuse_conversation
                    || !agent_sessions::session_matches(
                        tx,
                        user,
                        session,
                        new.personal_agent_id.as_deref(),
                    )
                    .await?)
            {
                return Err(invalid(
                    "The linked conversation does not match this automation",
                ));
            };
        }
        AutomationOwner::Principal(principal) => {
            if new.model.is_some()
                || new.agent_version.is_none_or(|version| version <= 0)
                || new.personal_agent_id.is_some()
                || new.synchronous
                || new.reuse_conversation
                || linked.is_some()
            {
                return Err(invalid(
                    "The principal webhook requires an asynchronous fresh published conversation",
                ));
            };
            if !matches!(
                agents::agent_owner_in(tx, principal).await?,
                Some(agents::AgentOwner::Organization(_))
            ) || agents::version_in(
                tx,
                principal,
                new.agent_version.expect("positive version checked"),
            )
            .await?
            .is_none()
            {
                return Err(invalid("The webhook published version is unavailable"));
            };
        }
    }
    Ok(())
}

pub async fn create(pool: &Pool, new: NewWebhook) -> Result<Webhook, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let row = create_in(&mut tx, new).await?;
    tx.commit().await?;
    Ok(row)
}
pub async fn create_in(tx: &mut WriteTx, new: NewWebhook) -> Result<Webhook, DbError> {
    let edit = EditWebhook {
        personal_agent_id: new.personal_agent_id.clone(),
        name: new.name.clone(),
        prompt: new.prompt.clone(),
        model: new.model.clone(),
        agent_version: new.agent_version,
        tools_enabled: new.tools_enabled,
        synchronous: new.synchronous,
        reuse_conversation: new.reuse_conversation,
        reuse_rounds: new.reuse_rounds,
        linked_session_id: None,
    };
    validate_in(
        tx,
        &new.owner,
        None,
        &edit,
        new.linked_session_id.as_deref(),
    )
    .await?;
    let now = Timestamp::now();
    let id = Uuid::new_v4().to_string();
    let generation = Uuid::new_v4().to_string();
    let (user, principal) = new.owner.columns();
    sqlx::query("INSERT INTO webhooks(id,user_id,principal_id,created_by,configuration_generation,agent_version,personal_agent_id,name,prompt,model,tools_enabled,synchronous,secret_hash,enabled,last_session_id,reuse_conversation,reuse_rounds,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,1,?,?,?,?,?)")
        .bind(&id).bind(user).bind(principal).bind(&new.created_by).bind(generation).bind(new.agent_version).bind(&new.personal_agent_id).bind(&new.name).bind(&new.prompt).bind(&new.model).bind(new.tools_enabled).bind(new.synchronous).bind(&new.secret_hash).bind(&new.linked_session_id).bind(new.reuse_conversation).bind(new.reuse_rounds).bind(now.to_string()).bind(now.to_string()).execute(&mut **tx).await?;
    get_by_id_in(tx, &id)
        .await?
        .ok_or_else(|| invalid("The newly created webhook is unavailable"))
}

async fn list_with<'e, E: sqlx::Executor<'e, Database = sqlx::Sqlite>>(
    executor: E,
    owner: &AutomationOwner,
) -> Result<Vec<Webhook>, DbError> {
    let (user, principal) = owner.columns();
    let sql = format!(
        "SELECT {COLS} FROM webhooks WHERE user_id IS ? AND principal_id IS ? ORDER BY created_at DESC,id ASC"
    );
    let rows = sqlx::query(&sql)
        .bind(user)
        .bind(principal)
        .fetch_all(executor)
        .await?;
    rows.iter().map(map_row).collect()
}
pub async fn list_for_owner_in(
    tx: &mut WriteTx,
    owner: &AutomationOwner,
) -> Result<Vec<Webhook>, DbError> {
    list_with(&mut **tx, owner).await
}
pub async fn list_for_user(pool: &Pool, user: &str) -> Result<Vec<Webhook>, DbError> {
    list_with(pool, &AutomationOwner::User(user.into())).await
}
async fn get_with<'e, E: sqlx::Executor<'e, Database = sqlx::Sqlite>>(
    executor: E,
    owner: &AutomationOwner,
    id: &str,
) -> Result<Option<Webhook>, DbError> {
    let (user, principal) = owner.columns();
    let sql =
        format!("SELECT {COLS} FROM webhooks WHERE id=? AND user_id IS ? AND principal_id IS ?");
    let row = sqlx::query(&sql)
        .bind(id)
        .bind(user)
        .bind(principal)
        .fetch_optional(executor)
        .await?;
    row.as_ref().map(map_row).transpose()
}
pub async fn get(
    pool: &Pool,
    owner: &AutomationOwner,
    id: &str,
) -> Result<Option<Webhook>, DbError> {
    get_with(pool, owner, id).await
}
pub async fn get_in(
    tx: &mut WriteTx,
    owner: &AutomationOwner,
    id: &str,
) -> Result<Option<Webhook>, DbError> {
    get_with(&mut **tx, owner, id).await
}
pub async fn get_by_id_in(tx: &mut WriteTx, id: &str) -> Result<Option<Webhook>, DbError> {
    let sql = format!("SELECT {COLS} FROM webhooks WHERE id=?");
    let row = sqlx::query(&sql).bind(id).fetch_optional(&mut **tx).await?;
    row.as_ref().map(map_row).transpose()
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditOutcome {
    Missing,
    Conflict,
    Saved(Box<Webhook>),
}
pub async fn update(
    pool: &Pool,
    owner: &AutomationOwner,
    id: &str,
    expected: &str,
    edit: EditWebhook,
) -> Result<EditOutcome, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let outcome = update_in(&mut tx, owner, id, expected, &edit).await?;
    tx.commit().await?;
    Ok(outcome)
}
pub async fn update_in(
    tx: &mut WriteTx,
    owner: &AutomationOwner,
    id: &str,
    expected: &str,
    edit: &EditWebhook,
) -> Result<EditOutcome, DbError> {
    let Some(previous) = get_in(tx, owner, id).await? else {
        return Ok(EditOutcome::Missing);
    };
    if previous.configuration_generation != expected {
        return Ok(EditOutcome::Conflict);
    };
    let linked = match &edit.linked_session_id {
        Some(link) => link.clone(),
        None if previous.personal_agent_id != edit.personal_agent_id
            || !edit.reuse_conversation =>
        {
            None
        }
        None => previous.last_session_id.clone(),
    };
    validate_in(
        tx,
        owner,
        previous.personal_agent_id.as_deref(),
        edit,
        linked.as_deref(),
    )
    .await?;
    let changed = previous.personal_agent_id != edit.personal_agent_id
        || previous.prompt != edit.prompt
        || previous.model != edit.model
        || previous.agent_version != edit.agent_version
        || previous.tools_enabled != edit.tools_enabled
        || previous.synchronous != edit.synchronous
        || previous.reuse_conversation != edit.reuse_conversation
        || previous.reuse_rounds != edit.reuse_rounds
        || (edit.linked_session_id.is_some() && previous.last_session_id != linked);
    if !changed && previous.name == edit.name {
        return Ok(EditOutcome::Saved(Box::new(previous)));
    };
    let generation = if changed {
        Uuid::new_v4().to_string()
    } else {
        previous.configuration_generation
    };
    sqlx::query("UPDATE webhooks SET personal_agent_id=?,last_session_id=?,name=?,prompt=?,model=?,agent_version=?,tools_enabled=?,synchronous=?,reuse_conversation=?,reuse_rounds=?,configuration_generation=?,updated_at=? WHERE id=?").bind(&edit.personal_agent_id).bind(linked).bind(&edit.name).bind(&edit.prompt).bind(&edit.model).bind(edit.agent_version).bind(edit.tools_enabled).bind(edit.synchronous).bind(edit.reuse_conversation).bind(edit.reuse_rounds).bind(generation).bind(Timestamp::now().to_string()).bind(id).execute(&mut **tx).await?;
    Ok(EditOutcome::Saved(Box::new(
        get_in(tx, owner, id)
            .await?
            .expect("updated webhook exists"),
    )))
}
pub async fn set_enabled(
    pool: &Pool,
    owner: &AutomationOwner,
    id: &str,
    expected: &str,
    enabled: bool,
) -> Result<EditOutcome, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let outcome = set_enabled_in(&mut tx, owner, id, expected, enabled).await?;
    tx.commit().await?;
    Ok(outcome)
}
pub async fn set_enabled_in(
    tx: &mut WriteTx,
    owner: &AutomationOwner,
    id: &str,
    expected: &str,
    enabled: bool,
) -> Result<EditOutcome, DbError> {
    let Some(row) = get_in(tx, owner, id).await? else {
        return Ok(EditOutcome::Missing);
    };
    if row.configuration_generation != expected {
        return Ok(EditOutcome::Conflict);
    };
    if row.enabled != enabled {
        sqlx::query("UPDATE webhooks SET enabled=?,updated_at=? WHERE id=?")
            .bind(enabled)
            .bind(Timestamp::now().to_string())
            .bind(id)
            .execute(&mut **tx)
            .await?;
    }
    Ok(EditOutcome::Saved(Box::new(
        get_in(tx, owner, id)
            .await?
            .expect("updated webhook exists"),
    )))
}
pub async fn rotate_secret(
    pool: &Pool,
    owner: &AutomationOwner,
    id: &str,
    expected: &str,
    hash: &str,
) -> Result<EditOutcome, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let outcome = rotate_secret_in(&mut tx, owner, id, expected, hash).await?;
    tx.commit().await?;
    Ok(outcome)
}
pub async fn rotate_secret_in(
    tx: &mut WriteTx,
    owner: &AutomationOwner,
    id: &str,
    expected: &str,
    hash: &str,
) -> Result<EditOutcome, DbError> {
    let Some(row) = get_in(tx, owner, id).await? else {
        return Ok(EditOutcome::Missing);
    };
    if row.configuration_generation != expected {
        return Ok(EditOutcome::Conflict);
    };
    sqlx::query("UPDATE webhooks SET secret_hash=?,updated_at=? WHERE id=?")
        .bind(hash)
        .bind(Timestamp::now().to_string())
        .bind(id)
        .execute(&mut **tx)
        .await?;
    Ok(EditOutcome::Saved(Box::new(
        get_in(tx, owner, id)
            .await?
            .expect("updated webhook exists"),
    )))
}
pub async fn delete(
    pool: &Pool,
    owner: &AutomationOwner,
    id: &str,
) -> Result<DeleteOutcome, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let outcome = delete_in(&mut tx, owner, id).await?;
    tx.commit().await?;
    Ok(outcome)
}
pub async fn delete_in(
    tx: &mut WriteTx,
    owner: &AutomationOwner,
    id: &str,
) -> Result<DeleteOutcome, DbError> {
    if get_in(tx, owner, id).await?.is_none() {
        return Ok(DeleteOutcome::Missing);
    };
    let pending:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM webhook_runs r LEFT JOIN chat_turns t ON t.id=r.assistant_turn_id WHERE r.webhook_id=? AND (t.status IN ('in_progress','suspended') OR (r.status IS NULL AND r.assistant_turn_id IS NULL)))").bind(id).fetch_one(&mut **tx).await?;
    if pending {
        return Ok(DeleteOutcome::Pending);
    };
    sqlx::query("DELETE FROM webhooks WHERE id=?")
        .bind(id)
        .execute(&mut **tx)
        .await?;
    Ok(DeleteOutcome::Deleted)
}
pub async fn find_active_by_secret_hash(
    pool: &Pool,
    hash: &str,
) -> Result<Option<Webhook>, DbError> {
    let sql = format!("SELECT {COLS} FROM webhooks WHERE secret_hash=? AND enabled=1");
    let row = sqlx::query(&sql).bind(hash).fetch_optional(pool).await?;
    row.as_ref().map(map_row).transpose()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebhookRun {
    pub id: String,
    pub webhook_id: String,
    pub configuration_generation: Option<String>,
    pub agent_version: Option<i64>,
    pub tools_enabled: bool,
    pub history_limit: Option<usize>,
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

const RUN_COLS: &str = "r.configuration_generation,r.agent_version,r.tools_enabled,r.history_limit,r.id, r.webhook_id, r.fired_at, r.status, r.session_id, r.assistant_turn_id, r.error, \
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
        configuration_generation: row.try_get("configuration_generation")?,
        agent_version: row.try_get("agent_version")?,
        tools_enabled: row.try_get::<i64, _>("tools_enabled")? != 0,
        history_limit: row
            .try_get::<Option<i64>, _>("history_limit")?
            .map(usize::try_from)
            .transpose()
            .map_err(|_| invalid("The webhook history limit is invalid"))?,
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
#[derive(Clone, Copy)]
pub enum AdmissionKind<'a> {
    Fire { secret_hash: &'a str },
    Replay,
}
pub struct WebhookAdmission<'a> {
    pub webhook_id: &'a str,
    pub expected_configuration_generation: &'a str,
    pub kind: AdmissionKind<'a>,
    pub prompt: &'a str,
    pub payload: &'a str,
}

pub async fn admit_run_in(
    tx: &mut WriteTx,
    new: &WebhookAdmission<'_>,
) -> Result<Option<WebhookRun>, DbError> {
    let Some(hook) = get_by_id_in(tx, new.webhook_id).await? else {
        return Ok(None);
    };
    if hook.configuration_generation != new.expected_configuration_generation {
        return Ok(None);
    };
    let source = match new.kind {
        AdmissionKind::Fire { secret_hash } => {
            if !hook.enabled || hook.secret_hash != secret_hash {
                return Ok(None);
            };
            "fire"
        }
        AdmissionKind::Replay => "rerun",
    };
    let id = Uuid::new_v4().to_string();
    let now = Timestamp::now().to_string();
    let history = hook
        .reuse_conversation
        .then(|| hook.reuse_rounds.max(0).saturating_mul(2));
    sqlx::query("INSERT INTO webhook_runs(id,webhook_id,fired_at,prompt,payload,source,created_at,configuration_generation,agent_version,tools_enabled,history_limit) VALUES(?,?,?,?,?,?,?,?,?,?,?)")
        .bind(&id).bind(&hook.id).bind(&now).bind(new.prompt).bind(new.payload).bind(source).bind(&now).bind(&hook.configuration_generation).bind(hook.agent_version).bind(hook.tools_enabled).bind(history).execute(&mut **tx).await?;
    sqlx::query("UPDATE webhooks SET last_payload=? WHERE id=?")
        .bind(new.payload)
        .bind(&hook.id)
        .execute(&mut **tx)
        .await?;
    get_run_in(tx, &hook.id, &id).await
}

pub async fn get_for_run_in(
    tx: &mut WriteTx,
    run_id: &str,
) -> Result<Option<(Webhook, WebhookRun)>, DbError> {
    let sql = format!("SELECT {RUN_COLS} FROM {RUN_FROM} WHERE r.id=?");
    let row = sqlx::query(&sql)
        .bind(run_id)
        .fetch_optional(&mut **tx)
        .await?;
    let Some(row) = row else { return Ok(None) };
    let run = map_run(&row)?;
    let Some(hook) = get_by_id_in(tx, &run.webhook_id).await? else {
        return Ok(None);
    };
    Ok(Some((hook, run)))
}
pub async fn attach_run_turn_in(
    tx: &mut WriteTx,
    run_id: &str,
    session: &str,
    turn: &str,
    tools: bool,
    history: Option<usize>,
) -> Result<(), DbError> {
    let (hook, run) = get_for_run_in(tx, run_id)
        .await?
        .ok_or_else(|| invalid("The webhook run is unavailable"))?;
    if run.status.is_some()
        || run.assistant_turn_id.is_some()
        || run.session_id.is_some()
        || run.configuration_generation.as_deref() != Some(hook.configuration_generation.as_str())
        || run.agent_version != hook.agent_version
    {
        return Err(invalid("The webhook run configuration changed"));
    };
    let valid = match &hook.owner {
        AutomationOwner::User(user) => {
            agent_sessions::session_matches(tx, user, session, hook.personal_agent_id.as_deref())
                .await?
        }
        AutomationOwner::Principal(principal) => match run.agent_version {
            Some(version) if version > 0 => {
                run_sessions::get_private_principal_session_in(tx, principal, session, version)
                    .await?
                    .is_some()
            }
            _ => false,
        },
    };
    let valid_turn:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM chat_turns WHERE id=? AND session_id=? AND role='assistant' AND status='in_progress')").bind(turn).bind(session).fetch_one(&mut **tx).await?;
    if !valid || !valid_turn {
        return Err(invalid(
            "The webhook turn has a different owner or publication",
        ));
    };
    let history = history
        .map(i64::try_from)
        .transpose()
        .map_err(|_| invalid("The webhook history limit is too large"))?;
    let affected=sqlx::query("UPDATE webhook_runs SET session_id=?,assistant_turn_id=?,tools_enabled=?,history_limit=? WHERE id=? AND status IS NULL AND assistant_turn_id IS NULL AND session_id IS NULL").bind(session).bind(turn).bind(tools).bind(history).bind(run_id).execute(&mut **tx).await?.rows_affected();
    if affected != 1 {
        return Err(invalid("The webhook run was already attached"));
    };
    sqlx::query("UPDATE webhooks SET last_session_id=? WHERE id=? AND configuration_generation=? AND (SELECT id FROM webhook_runs WHERE webhook_id=? ORDER BY rtrim(fired_at,'Z') DESC,rowid DESC LIMIT 1)=?").bind(session).bind(&hook.id).bind(&hook.configuration_generation).bind(&hook.id).bind(run_id).execute(&mut **tx).await?;
    Ok(())
}

pub struct NewWebhookRun<'a> {
    pub webhook_id: &'a str,
    pub expected_configuration_generation: &'a str,
    pub session_id: &'a str,
    pub prompt: &'a str,
    pub payload: &'a str,
    pub source: &'a str,
    pub assistant_turn_id: &'a str,
    pub tools_enabled: bool,
    pub history_limit: Option<usize>,
}
pub async fn record_run_start(pool: &Pool, new: NewWebhookRun<'_>) -> Result<String, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let hook = get_by_id_in(&mut tx, new.webhook_id)
        .await?
        .ok_or_else(|| invalid("The webhook is unavailable"))?;
    let kind = match new.source {
        "fire" => AdmissionKind::Fire {
            secret_hash: &hook.secret_hash,
        },
        "rerun" => AdmissionKind::Replay,
        _ => return Err(invalid("The webhook run source is invalid")),
    };
    let run = admit_run_in(
        &mut tx,
        &WebhookAdmission {
            webhook_id: new.webhook_id,
            expected_configuration_generation: new.expected_configuration_generation,
            kind,
            prompt: new.prompt,
            payload: new.payload,
        },
    )
    .await?
    .ok_or_else(|| invalid("The webhook configuration changed"))?;
    attach_run_turn_in(
        &mut tx,
        &run.id,
        new.session_id,
        new.assistant_turn_id,
        new.tools_enabled,
        new.history_limit,
    )
    .await?;
    tx.commit().await?;
    Ok(run.id)
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
           FROM (SELECT r.webhook_id, r.fired_at, r.configuration_generation
                 FROM webhook_runs r
                 WHERE r.status IS NULL
                   AND r.rowid = (SELECT rowid FROM webhook_runs
                                  WHERE webhook_id = r.webhook_id
                                  ORDER BY rtrim(fired_at, 'Z') DESC, rowid DESC
                                  LIMIT 1)) AS newest
           WHERE webhooks.id = newest.webhook_id
             AND webhooks.configuration_generation = newest.configuration_generation"#,
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
    list_runs_with(pool, webhook_id, limit).await
}
pub async fn list_runs_in(
    tx: &mut WriteTx,
    webhook_id: &str,
    limit: i64,
) -> Result<Vec<WebhookRun>, DbError> {
    list_runs_with(&mut **tx, webhook_id, limit).await
}
async fn list_runs_with<'e, E: sqlx::Executor<'e, Database = sqlx::Sqlite>>(
    executor: E,
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
        .fetch_all(executor)
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
    user: &str,
) -> Result<std::collections::HashMap<String, (i64, i64)>, DbError> {
    counts_with(pool, &AutomationOwner::User(user.into())).await
}
pub async fn run_counts_for_owner_in(
    tx: &mut WriteTx,
    owner: &AutomationOwner,
) -> Result<std::collections::HashMap<String, (i64, i64)>, DbError> {
    counts_with(&mut **tx, owner).await
}
async fn counts_with<'e, E: sqlx::Executor<'e, Database = sqlx::Sqlite>>(
    executor: E,
    owner: &AutomationOwner,
) -> Result<std::collections::HashMap<String, (i64, i64)>, DbError> {
    let (user, principal) = owner.columns();
    let rows=sqlx::query("SELECT r.webhook_id,COUNT(*) AS runs,COUNT(DISTINCT r.session_id) AS chats FROM webhook_runs r JOIN webhooks w ON w.id=r.webhook_id WHERE w.user_id IS ? AND w.principal_id IS ? GROUP BY r.webhook_id").bind(user).bind(principal).fetch_all(executor).await?;
    rows.iter()
        .map(|row| {
            Ok((
                row.try_get("webhook_id")?,
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
    get_run_with(pool, webhook_id, run_id).await
}
pub async fn get_run_in(
    tx: &mut WriteTx,
    webhook_id: &str,
    run_id: &str,
) -> Result<Option<WebhookRun>, DbError> {
    get_run_with(&mut **tx, webhook_id, run_id).await
}
async fn get_run_with<'e, E: sqlx::Executor<'e, Database = sqlx::Sqlite>>(
    executor: E,
    webhook_id: &str,
    run_id: &str,
) -> Result<Option<WebhookRun>, DbError> {
    let sql = format!("SELECT {RUN_COLS} FROM {RUN_FROM} WHERE r.webhook_id = ? AND r.id = ?");
    let row = sqlx::query(&sql)
        .bind(webhook_id)
        .bind(run_id)
        .fetch_optional(executor)
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
            expected_configuration_generation,
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
                expected_configuration_generation,
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
            personal_agent_id: None,
            linked_session_id: None,
            owner: AutomationOwner::User(user_id.to_string()),
            created_by: user_id.to_string(),
            agent_version: None,
            name: "Deploy digest".to_string(),
            prompt: "Summarize the incoming payload.".to_string(),
            model: Some("qwen".to_string()),
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
        let got = get(&pool, &AutomationOwner::User("u1".into()), &created.id)
            .await
            .unwrap()
            .unwrap();
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
        assert!(
            get(&pool, &AutomationOwner::User("u2".into()), &created.id)
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            delete(&pool, &AutomationOwner::User("u2".into()), &created.id)
                .await
                .unwrap(),
            DeleteOutcome::Missing
        );
        assert!(
            get(&pool, &AutomationOwner::User("u1".into()), &created.id)
                .await
                .unwrap()
                .is_some()
        );
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
        set_enabled(
            &pool,
            &created.owner,
            &created.id,
            &created.configuration_generation,
            false,
        )
        .await
        .unwrap();
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
                expected_configuration_generation: &hook.configuration_generation,
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
        crate::server::background::reconcile_turn(
            &pool,
            &hook.owner,
            get_run(&pool, &hook.id, &done)
                .await
                .unwrap()
                .unwrap()
                .assistant_turn_id
                .as_deref()
                .unwrap(),
        )
        .await
        .unwrap();
        let orphan = fixture_run_start(
            &pool,
            NewWebhookRun {
                webhook_id: &hook.id,
                expected_configuration_generation: &hook.configuration_generation,
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
        let row = get(&pool, &AutomationOwner::User("u1".into()), &hook.id)
            .await
            .unwrap()
            .unwrap();
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
                expected_configuration_generation: &hook.configuration_generation,
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
                expected_configuration_generation: &hook.configuration_generation,
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
        crate::server::background::reconcile_turn(
            &pool,
            &hook.owner,
            get_run(&pool, &hook.id, &later)
                .await
                .unwrap()
                .unwrap()
                .assistant_turn_id
                .as_deref()
                .unwrap(),
        )
        .await
        .unwrap();

        let runs = list_runs(&pool, &hook.id, 50).await.unwrap();
        let order: Vec<&str> = runs.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(order, [later.as_str(), earlier.as_str()]);
        assert_eq!(sweep_interrupted_runs(&pool).await.unwrap(), 1);
        let row = get(&pool, &AutomationOwner::User("u1".into()), &hook.id)
            .await
            .unwrap()
            .unwrap();
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
                expected_configuration_generation: &hook.configuration_generation,
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
            personal_agent_id: None,
            linked_session_id: Some(None),
            name: source.name.clone(),
            prompt: source.prompt.clone(),
            model: source.model.clone(),
            agent_version: source.agent_version,
            tools_enabled: source.tools_enabled,
            reuse_conversation: true,
            reuse_rounds: source.reuse_rounds,
            synchronous: source.synchronous,
        };
        assert!(matches!(
            update(
                &pool,
                &AutomationOwner::User("u2".into()),
                &source.id,
                &source.configuration_generation,
                edit()
            )
            .await
            .unwrap(),
            EditOutcome::Missing
        ));
        assert_eq!(
            get(&pool, &AutomationOwner::User("u1".into()), &source.id)
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
            get(&pool, &AutomationOwner::User("u1".into()), &source.id)
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
        assert!(matches!(
            rotate_secret(
                &pool,
                &created.owner,
                &created.id,
                &created.configuration_generation,
                "hash-new"
            )
            .await
            .unwrap(),
            EditOutcome::Saved(_)
        ));
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
        assert!(matches!(
            rotate_secret(
                &pool,
                &AutomationOwner::User("u2".into()),
                &created.id,
                &created.configuration_generation,
                "hash-hijack"
            )
            .await
            .unwrap(),
            EditOutcome::Missing
        ));
    }

    #[tokio::test]
    async fn admission_records_payload_and_reconciliation_records_real_outcome() {
        let pool = fresh_db().await;
        seed_user(&pool, "u1").await;
        let hook = create(&pool, sample("u1", "hash-outcome")).await.unwrap();
        let chat = session_core::db::create_session(&pool, "u1").await.unwrap();
        let run = fixture_run_start(
            &pool,
            NewWebhookRun {
                webhook_id: &hook.id,
                expected_configuration_generation: &hook.configuration_generation,
                session_id: &chat.id,
                prompt: "p",
                payload: "{}",
                source: "fire",
                assistant_turn_id: "outcome-turn",
                tools_enabled: false,
                history_limit: None,
            },
        )
        .await
        .unwrap();
        crate::server::background::reconcile_turn(&pool, &hook.owner, "outcome-turn")
            .await
            .unwrap();
        let got = get(&pool, &hook.owner, &hook.id).await.unwrap().unwrap();
        assert_eq!(got.last_payload.as_deref(), Some("{}"));
        assert_eq!(got.last_status.as_deref(), Some("ok"));
        assert_eq!(got.last_session_id.as_deref(), Some(chat.id.as_str()));
        assert!(got.last_fired_at.is_some());
        assert_eq!(
            get_run(&pool, &hook.id, &run)
                .await
                .unwrap()
                .unwrap()
                .status
                .as_deref(),
            Some("ok")
        );
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
                expected_configuration_generation: &hook.configuration_generation,
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
                expected_configuration_generation: &hook.configuration_generation,
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
                    expected_configuration_generation: &fresh_each_time.configuration_generation,
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
                    expected_configuration_generation: &reuses.configuration_generation,
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
                expected_configuration_generation: &other_owner.configuration_generation,
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
                    expected_configuration_generation: &hook.configuration_generation,
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
                    expected_configuration_generation: &hook.configuration_generation,
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
                expected_configuration_generation: &hook.configuration_generation,
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
            delete(&pool, &AutomationOwner::User("u1".into()), &hook.id)
                .await
                .unwrap(),
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
            delete(&pool, &AutomationOwner::User("u1".into()), &hook.id)
                .await
                .unwrap(),
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
            delete(&pool, &AutomationOwner::User("u1".into()), &hook.id)
                .await
                .unwrap(),
            DeleteOutcome::Deleted
        );
    }
}
