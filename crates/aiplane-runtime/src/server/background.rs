// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use aiplane_core::server::db::usage::UsageSource;
use aiplane_core::server::db::{DbError, Pool};
use sqlx::Row;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationOwner {
    User(String),
    Principal(String),
}

impl AutomationOwner {
    pub(crate) fn columns(&self) -> (Option<&str>, Option<&str>) {
        match self {
            Self::User(id) => (Some(id), None),
            Self::Principal(id) => (None, Some(id)),
        }
    }
}

#[derive(Debug)]
pub struct BackgroundTurn {
    pub source: UsageSource,
    pub history_limit: Option<usize>,
    pub tools_enabled: bool,
}

const MATCHED_RUNS: &str = "SELECT 'scheduled' AS source, r.id AS run_id, r.session_id,
    r.tools_enabled AS snapshot_tools, r.history_limit, a.user_id AS owner,
    a.principal_id AS owner_principal, a.tools_enabled AS current_tools, a.dot_id,
    s.user_id AS session_owner, s.principal_id, s.shared, s.agent_version,
    s.visitor_id, s.parent_turn_id, p.dot_id AS session_dot,
    r.agent_version AS run_agent_version, a.agent_version AS current_agent_version,
    r.configuration_generation AS run_generation, a.configuration_generation AS current_generation
    FROM scheduled_runs r LEFT JOIN scheduled_actions a ON a.id=r.action_id
    LEFT JOIN chat_sessions s ON s.id=r.session_id
    LEFT JOIN personal_dot_sessions p ON p.session_id=s.id WHERE r.assistant_turn_id=?
    UNION ALL SELECT 'webhook', r.id, r.session_id, r.tools_enabled, r.history_limit,
    a.user_id, a.principal_id, a.tools_enabled, a.dot_id, s.user_id, s.principal_id, s.shared,
    s.agent_version, s.visitor_id, s.parent_turn_id, p.dot_id, r.agent_version, a.agent_version, r.configuration_generation, a.configuration_generation
    FROM webhook_runs r LEFT JOIN webhooks a ON a.id=r.webhook_id
    LEFT JOIN chat_sessions s ON s.id=r.session_id
    LEFT JOIN personal_dot_sessions p ON p.session_id=s.id WHERE r.assistant_turn_id=?";

fn has_original_run_pin(row: &sqlx::sqlite::SqliteRow) -> Result<bool, DbError> {
    let run_version: Option<i64> = row.try_get("run_agent_version")?;
    let session_version: Option<i64> = row.try_get("agent_version")?;
    Ok(run_version.is_some_and(|version| version > 0) && run_version == session_version)
}

fn owner_matches(
    row: &sqlx::sqlite::SqliteRow,
    expected: &AutomationOwner,
) -> Result<bool, DbError> {
    let user: Option<String> = row.try_get("owner")?;
    let owner_principal: Option<String> = row.try_get("owner_principal")?;
    let session_user: Option<String> = row.try_get("session_owner")?;
    let session_principal: Option<String> = row.try_get("principal_id")?;
    Ok(match expected {
        AutomationOwner::User(id) => {
            user.as_ref() == Some(id)
                && session_user.as_ref() == Some(id)
                && owner_principal.is_none()
                && session_principal.is_none()
        }
        AutomationOwner::Principal(id) => {
            user.is_none()
                && session_user.is_none()
                && owner_principal.as_ref() == Some(id)
                && session_principal.as_ref() == Some(id)
                && !row.try_get::<bool, _>("shared")?
                && has_original_run_pin(row)?
                && row.try_get::<Option<String>, _>("visitor_id")?.is_none()
                && row
                    .try_get::<Option<String>, _>("parent_turn_id")?
                    .is_none()
        }
    })
}

fn current_configuration_matches(row: &sqlx::sqlite::SqliteRow) -> Result<bool, DbError> {
    if row
        .try_get::<Option<String>, _>("run_generation")?
        .is_none()
        && row
            .try_get::<Option<String>, _>("owner_principal")?
            .is_none()
        && row.try_get::<String, _>("source")? == "webhook"
    {
        return Ok(true);
    }
    let run_generation: Option<String> = row.try_get("run_generation")?;
    let current_generation: Option<String> = row.try_get("current_generation")?;
    let run_version: Option<i64> = row.try_get("run_agent_version")?;
    let current_version: Option<i64> = row.try_get("current_agent_version")?;
    Ok(run_generation
        .as_ref()
        .is_some_and(|generation| !generation.is_empty())
        && run_generation == current_generation
        && run_version == current_version)
}

fn invalid(message: &str) -> DbError {
    DbError::Decode {
        column: "background_turn",
        source: std::io::Error::other(message.to_string()).into(),
    }
}

pub async fn for_turn(
    db: &Pool,
    owner: &AutomationOwner,
    assistant_turn_id: &str,
) -> Result<Option<BackgroundTurn>, DbError> {
    let rows = sqlx::query(MATCHED_RUNS)
        .bind(assistant_turn_id)
        .bind(assistant_turn_id)
        .fetch_all(db)
        .await?;
    if rows.is_empty() {
        return Ok(None);
    }
    if rows.len() != 1 {
        return Err(invalid(
            "The background turn has multiple run bindings. Correct its run history before continuing.",
        ));
    }
    let row = &rows[0];
    let dot: Option<String> = row.try_get("dot_id")?;
    let session_dot: Option<String> = row.try_get("session_dot")?;
    // An unbound (person) session is a valid home for a Dot-configured run —
    // the Dot is optional — but a session bound to a *different* Dot is not.
    let dot_mismatch = !(session_dot.is_none() || session_dot == dot);
    if !owner_matches(row, owner)? || row.try_get::<bool, _>("shared")? || dot_mismatch {
        return Err(invalid(
            "The background run no longer matches its owner and conversation binding. Restore its original binding before continuing.",
        ));
    }
    if !current_configuration_matches(row)? {
        return Err(invalid(
            "The background run's configuration changed. Start a new run with the current configuration before continuing.",
        ));
    }
    let session_id: String = row.try_get("session_id")?;
    let valid: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM chat_turns WHERE id=? AND session_id=? AND role='assistant')",
    )
    .bind(assistant_turn_id)
    .bind(&session_id)
    .fetch_one(db)
    .await?;
    if !valid {
        return Err(invalid(
            "The background run's assistant turn is unavailable. Open its run history to inspect the missing conversation.",
        ));
    }
    let history: Option<i64> = row.try_get("history_limit")?;
    let history_limit=history.map(usize::try_from).transpose().map_err(|_|invalid("The background run has an invalid history limit. Correct its run history before continuing."))?;
    let source: String = row.try_get("source")?;
    Ok(Some(BackgroundTurn {
        source: if source == "scheduled" {
            UsageSource::Scheduled
        } else {
            UsageSource::Webhook
        },
        history_limit,
        tools_enabled: row.try_get::<bool, _>("snapshot_tools")?
            && row.try_get::<bool, _>("current_tools")?,
    }))
}

async fn structural_run_in(
    tx: &mut aiplane_core::server::db::WriteTx,
    owner: &AutomationOwner,
    assistant_turn_id: &str,
) -> Result<Option<(sqlx::sqlite::SqliteRow, sqlx::sqlite::SqliteRow)>, DbError> {
    let rows = sqlx::query(MATCHED_RUNS)
        .bind(assistant_turn_id)
        .bind(assistant_turn_id)
        .fetch_all(&mut **tx)
        .await?;
    if rows.is_empty() {
        return Ok(None);
    }
    if rows.len() != 1 {
        return Err(invalid(
            "The background turn has multiple run bindings. Correct its run history before reconciling.",
        ));
    }
    let row = rows
        .into_iter()
        .next()
        .expect("exactly one binding checked");
    if !owner_matches(&row, owner)? {
        return Err(invalid(
            "The background result does not belong to this owner.",
        ));
    }
    let turn = sqlx::query(
        "SELECT status,error_message,session_id FROM chat_turns WHERE id=? AND role='assistant'",
    )
    .bind(assistant_turn_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| invalid("The background assistant turn is unavailable."))?;
    if turn.try_get::<String, _>("session_id")? != row.try_get::<String, _>("session_id")? {
        return Err(invalid(
            "The background assistant turn does not match its run's conversation.",
        ));
    }
    Ok(Some((row, turn)))
}

pub(crate) async fn validate_turn_in(
    tx: &mut aiplane_core::server::db::WriteTx,
    owner: &AutomationOwner,
    assistant_turn_id: &str,
) -> Result<(), DbError> {
    structural_run_in(tx, owner, assistant_turn_id)
        .await
        .map(|_| ())
}

pub async fn reconcile_turn(
    db: &Pool,
    owner: &AutomationOwner,
    assistant_turn_id: &str,
) -> Result<(), DbError> {
    let mut tx = aiplane_agents::db::WriteTx::begin(db).await?;
    let Some((row, turn)) = structural_run_in(&mut tx, owner, assistant_turn_id).await? else {
        return Ok(());
    };
    let status: String = turn.try_get("status")?;
    let mut error: Option<String> = turn.try_get("error_message")?;
    let status = match status.as_str() {
        "in_progress" => return Ok(()),
        "completed" => "ok",
        "suspended" => "waiting",
        "cancelled" => {
            error.get_or_insert_with(|| {
                session_core::i18n::t(session_core::i18n::Lang::En, "chat-error-run-cancelled")
            });
            "error"
        }
        "errored" => "error",
        _ => return Err(invalid("The background turn has an unrecognized status.")),
    };
    let source: String = row.try_get("source")?;
    let (runs, triggers, foreign, last_at) = if source == "scheduled" {
        (
            "scheduled_runs",
            "scheduled_actions",
            "action_id",
            "last_run_at",
        )
    } else {
        ("webhook_runs", "webhooks", "webhook_id", "last_fired_at")
    };
    let run_id: String = row.try_get("run_id")?;
    sqlx::query(&format!("UPDATE {runs} SET status=?,error=? WHERE id=?"))
        .bind(status)
        .bind(&error)
        .bind(&run_id)
        .execute(&mut *tx)
        .await?;
    let generation_guard = if source == "scheduled"
        || row
            .try_get::<Option<String>, _>("run_generation")?
            .is_some()
    {
        format!(
            " AND configuration_generation=(SELECT configuration_generation FROM {runs} WHERE id=?)"
        )
    } else {
        String::new()
    };
    let summary = format!(
        "UPDATE {triggers} SET last_status=?,last_error=?,{last_at}=(SELECT fired_at FROM {runs} WHERE id=?),last_session_id=(SELECT session_id FROM {runs} WHERE id=?) WHERE id=(SELECT {foreign} FROM {runs} WHERE id=?) AND (SELECT id FROM {runs} WHERE {foreign}={triggers}.id ORDER BY rtrim(fired_at,'Z') DESC,rowid DESC LIMIT 1)=?{generation_guard}"
    );
    let summary = sqlx::query(&summary)
        .bind(status)
        .bind(&error)
        .bind(&run_id)
        .bind(&run_id)
        .bind(&run_id)
        .bind(&run_id);
    let summary = if source == "scheduled"
        || row
            .try_get::<Option<String>, _>("run_generation")?
            .is_some()
    {
        summary.bind(&run_id)
    } else {
        summary
    };
    summary.execute(&mut *tx).await?;
    tx.commit().await
}

pub async fn reconcile_interrupted_waiting(db: &Pool, source: UsageSource) -> Result<u64, DbError> {
    let (runs, triggers, foreign) = match source {
        UsageSource::Scheduled => ("scheduled_runs", "scheduled_actions", "action_id"),
        UsageSource::Webhook => ("webhook_runs", "webhooks", "webhook_id"),
        _ => {
            return Err(invalid(
                "Only scheduled and webhook histories have background continuations.",
            ));
        }
    };
    let max_row: Option<i64> = sqlx::query_scalar(&format!("SELECT MAX(rowid) FROM {runs}"))
        .fetch_one(db)
        .await?;
    let Some(max_row) = max_row else { return Ok(0) };
    let mut cursor = i64::MIN;
    let mut changed = 0u64;
    while cursor < max_row {
        let principal_column = if matches!(source, UsageSource::Scheduled) {
            "a.principal_id"
        } else {
            "NULL"
        };
        let rows=sqlx::query(&format!("SELECT r.rowid AS cursor,a.user_id,{principal_column} AS owner_principal,r.assistant_turn_id FROM {runs} r JOIN {triggers} a ON a.id=r.{foreign} JOIN chat_turns t ON t.id=r.assistant_turn_id WHERE r.rowid>? AND r.rowid<=? AND r.status='waiting' AND t.status IN ('completed','cancelled','errored') ORDER BY r.rowid LIMIT 128"))
            .bind(cursor).bind(max_row).fetch_all(db).await?;
        if rows.is_empty() {
            break;
        }
        for row in rows {
            cursor = row.try_get("cursor")?;
            let owner = match (
                row.try_get::<Option<String>, _>("user_id")?,
                row.try_get::<Option<String>, _>("owner_principal")?,
            ) {
                (Some(user), None) => AutomationOwner::User(user),
                (None, Some(principal)) => AutomationOwner::Principal(principal),
                _ => return Err(invalid("The background owner is unavailable.")),
            };
            let turn: String = row.try_get("assistant_turn_id")?;
            reconcile_turn(db, &owner, &turn).await?;
            changed += 1;
        }
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiplane_core::server::db::{open, users};
    use jiff::Timestamp;
    use session_core::db as chat;

    async fn fixture() -> Pool {
        let db = open(std::path::Path::new(":memory:")).await.unwrap();
        let now = Timestamp::now();
        users::upsert(
            &db,
            &users::User {
                id: "owner".into(),
                email: "owner@example.test".into(),
                name: None,
                roles: vec![],
                created_at: now,
                updated_at: now,
                timezone: None,
                speech_voice: None,
            },
        )
        .await
        .unwrap();
        let session = chat::create_session(&db, "owner").await.unwrap();
        chat::create_assistant_turn_in_progress(&db, &session.id, "turn", "model")
            .await
            .unwrap();
        sqlx::query("INSERT INTO scheduled_actions(id,user_id,name,prompt,model,cron,timezone,tools_enabled,created_at,updated_at) VALUES('action','owner','job','prompt','model','* * * * *','UTC',1,?,?)").bind(now.to_string()).bind(now.to_string()).execute(&db).await.unwrap();
        sqlx::query("INSERT INTO scheduled_runs(id,action_id,fired_at,session_id,assistant_turn_id,tools_enabled,history_limit,configuration_generation,created_at) SELECT 'run',id,?,?, 'turn',1,6,configuration_generation,? FROM scheduled_actions WHERE id='action'").bind(now.to_string()).bind(&session.id).bind(now.to_string()).execute(&db).await.unwrap();
        db
    }

    async fn principal_fixture() -> (Pool, String, String) {
        use aiplane_agents::db::{run_sessions, system_principals};
        let db = fixture().await;
        let principal = system_principals::create(
            &db,
            &system_principals::NewPrincipal {
                name: "scheduled-principal",
                display: "Harald",
                description: "Scheduled owner proof",
            },
            "owner",
        )
        .await
        .unwrap()
        .unwrap();
        let session = run_sessions::create_principal_session(
            &db,
            &run_sessions::NewRunSession {
                principal_id: &principal.id,
                title: None,
                parent_turn_id: None,
                agent_version: Some(7),
            },
        )
        .await
        .unwrap();
        chat::create_assistant_turn_in_progress(&db, &session.id, "principal-turn", "model")
            .await
            .unwrap();
        let now = Timestamp::now().to_string();
        sqlx::query("INSERT INTO scheduled_actions(id,principal_id,created_by,agent_version,configuration_generation,name,prompt,model,cron,timezone,tools_enabled,next_run_at,created_at,updated_at) VALUES('principal-action',?,'owner',7,'original','job','prompt',NULL,'* * * * *','UTC',1,'2999-01-01T00:00:00Z',?,?)")
            .bind(&principal.id).bind(&now).bind(&now).execute(&db).await.unwrap();
        sqlx::query("INSERT INTO scheduled_runs(id,action_id,fired_at,session_id,assistant_turn_id,tools_enabled,history_limit,agent_version,configuration_generation,created_at) VALUES('principal-run','principal-action',?,?,'principal-turn',1,6,7,'original',?)")
            .bind(&now).bind(&session.id).bind(&now).execute(&db).await.unwrap();
        (db, principal.id, session.id)
    }

    #[tokio::test]
    async fn principal_webhook_background_retains_exact_owner_pin_and_structural_cleanup() {
        let (db, principal, session) = principal_fixture().await;
        let now = Timestamp::now().to_string();
        sqlx::query("DELETE FROM scheduled_runs WHERE id='principal-run'")
            .execute(&db)
            .await
            .unwrap();
        sqlx::query("INSERT INTO webhooks(id,principal_id,created_by,agent_version,configuration_generation,name,prompt,model,tools_enabled,secret_hash,created_at,updated_at) VALUES('principal-hook',?,'owner',7,'original','events','prompt',NULL,1,'principal-secret',?,?)")
            .bind(&principal).bind(&now).bind(&now).execute(&db).await.unwrap();
        sqlx::query("INSERT INTO webhook_runs(id,webhook_id,fired_at,session_id,assistant_turn_id,tools_enabled,history_limit,agent_version,configuration_generation,prompt,payload,source,created_at) VALUES('principal-hook-run','principal-hook',?,?,'principal-turn',1,4,7,'original','event','payload','fire',?)")
            .bind(&now).bind(&session).bind(&now).execute(&db).await.unwrap();
        let owner = AutomationOwner::Principal(principal);
        let context = for_turn(&db, &owner, "principal-turn")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(context.source, UsageSource::Webhook);
        assert_eq!(context.history_limit, Some(4));
        assert!(context.tools_enabled);
        assert!(
            for_turn(
                &db,
                &AutomationOwner::User("owner".into()),
                "principal-turn"
            )
            .await
            .is_err()
        );
        chat::finalize_turn(&db, "principal-turn", chat::TurnStatus::Suspended, None)
            .await
            .unwrap();
        reconcile_turn(&db, &owner, "principal-turn").await.unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT status FROM webhook_runs WHERE id='principal-hook-run'"
            )
            .fetch_one(&db)
            .await
            .unwrap(),
            "waiting"
        );
        sqlx::query(
            "UPDATE webhooks SET enabled=0,secret_hash='rotated' WHERE id='principal-hook'",
        )
        .execute(&db)
        .await
        .unwrap();
        assert!(
            for_turn(&db, &owner, "principal-turn")
                .await
                .unwrap()
                .is_some()
        );
        sqlx::query("UPDATE webhooks SET agent_version=8,configuration_generation='replacement',last_status='new-configuration' WHERE id='principal-hook'").execute(&db).await.unwrap();
        assert!(for_turn(&db, &owner, "principal-turn").await.is_err());
        chat::finalize_turn(&db, "principal-turn", chat::TurnStatus::Cancelled, None)
            .await
            .unwrap();
        reconcile_turn(&db, &owner, "principal-turn").await.unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT status FROM webhook_runs WHERE id='principal-hook-run'"
            )
            .fetch_one(&db)
            .await
            .unwrap(),
            "error"
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT last_status FROM webhooks WHERE id='principal-hook'"
            )
            .fetch_one(&db)
            .await
            .unwrap(),
            "new-configuration"
        );
    }

    #[tokio::test]
    async fn principal_background_dispatch_requires_exact_current_generation_and_published_pin() {
        for mutation in [
            "UPDATE scheduled_actions SET configuration_generation='replacement' WHERE id='principal-action'",
            "UPDATE scheduled_runs SET configuration_generation='replacement' WHERE id='principal-run'",
            "UPDATE scheduled_runs SET agent_version=8 WHERE id='principal-run'",
            "UPDATE scheduled_actions SET agent_version=8 WHERE id='principal-action'",
            "UPDATE chat_sessions SET agent_version=8 WHERE id=(SELECT session_id FROM scheduled_runs WHERE id='principal-run')",
        ] {
            let (db, principal, _) = principal_fixture().await;
            let owner = AutomationOwner::Principal(principal);
            let context = for_turn(&db, &owner, "principal-turn")
                .await
                .unwrap()
                .unwrap();
            assert_eq!(context.source, UsageSource::Scheduled);
            assert_eq!(context.history_limit, Some(6));
            assert!(context.tools_enabled);
            assert!(
                for_turn(
                    &db,
                    &AutomationOwner::User("owner".into()),
                    "principal-turn"
                )
                .await
                .is_err()
            );
            sqlx::query(mutation).execute(&db).await.unwrap();
            assert!(
                for_turn(&db, &owner, "principal-turn").await.is_err(),
                "{mutation}"
            );
        }
    }

    #[tokio::test]
    async fn principal_structural_cancel_reconciles_stale_disabled_run_without_touching_new_configuration()
     {
        let (db, principal, _) = principal_fixture().await;
        let owner = AutomationOwner::Principal(principal.clone());
        chat::finalize_turn(&db, "principal-turn", chat::TurnStatus::Suspended, None)
            .await
            .unwrap();
        reconcile_turn(&db, &owner, "principal-turn").await.unwrap();
        sqlx::query("UPDATE scheduled_actions SET configuration_generation='replacement',agent_version=8,enabled=0,last_status='new-configuration',last_error='retained' WHERE id='principal-action'").execute(&db).await.unwrap();
        sqlx::query("UPDATE system_principals SET disabled_at=? WHERE id=?")
            .bind(Timestamp::now().to_string())
            .bind(&principal)
            .execute(&db)
            .await
            .unwrap();
        chat::finalize_turn(&db, "principal-turn", chat::TurnStatus::Cancelled, None)
            .await
            .unwrap();
        reconcile_turn(&db, &owner, "principal-turn").await.unwrap();
        let run = sqlx::query("SELECT status,error,configuration_generation,agent_version FROM scheduled_runs WHERE id='principal-run'").fetch_one(&db).await.unwrap();
        assert_eq!(run.get::<String, _>("status"), "error");
        assert!(run.get::<String, _>("error").contains("cancelled"));
        assert_eq!(run.get::<String, _>("configuration_generation"), "original");
        assert_eq!(run.get::<i64, _>("agent_version"), 7);
        let action = sqlx::query("SELECT enabled,last_status,last_error,next_run_at,configuration_generation FROM scheduled_actions WHERE id='principal-action'").fetch_one(&db).await.unwrap();
        assert!(!action.get::<bool, _>("enabled"));
        assert_eq!(action.get::<String, _>("last_status"), "new-configuration");
        assert_eq!(action.get::<String, _>("last_error"), "retained");
        assert_eq!(
            action.get::<String, _>("next_run_at"),
            "2999-01-01T00:00:00Z"
        );
        assert_eq!(
            action.get::<String, _>("configuration_generation"),
            "replacement"
        );
    }

    #[tokio::test]
    async fn principal_structural_reconciliation_requires_original_run_session_pin() {
        for mutation in [
            "UPDATE scheduled_runs SET agent_version=8 WHERE id='principal-run'",
            "UPDATE chat_sessions SET agent_version=8 WHERE id=(SELECT session_id FROM scheduled_runs WHERE id='principal-run')",
        ] {
            let (db, principal, _) = principal_fixture().await;
            chat::finalize_turn(&db, "principal-turn", chat::TurnStatus::Cancelled, None)
                .await
                .unwrap();
            sqlx::query(mutation).execute(&db).await.unwrap();
            assert!(
                reconcile_turn(
                    &db,
                    &AutomationOwner::Principal(principal),
                    "principal-turn"
                )
                .await
                .is_err(),
                "{mutation}"
            );
            let status: Option<String> =
                sqlx::query_scalar("SELECT status FROM scheduled_runs WHERE id='principal-run'")
                    .fetch_one(&db)
                    .await
                    .unwrap();
            assert!(status.is_none());
        }
    }

    #[tokio::test]
    async fn structural_validation_in_uses_current_transaction_without_execution_admission() {
        let (db, principal, _) = principal_fixture().await;
        let owner = AutomationOwner::Principal(principal);
        let mut tx = aiplane_core::server::db::WriteTx::begin(&db).await.unwrap();
        sqlx::query("UPDATE scheduled_actions SET configuration_generation='replacement',agent_version=8,enabled=0 WHERE id='principal-action'").execute(&mut *tx).await.unwrap();
        validate_turn_in(&mut tx, &owner, "principal-turn")
            .await
            .unwrap();
        validate_turn_in(&mut tx, &owner, "unbound-turn")
            .await
            .unwrap();
        sqlx::query("UPDATE scheduled_runs SET agent_version=8 WHERE id='principal-run'")
            .execute(&mut *tx)
            .await
            .unwrap();
        assert!(
            validate_turn_in(&mut tx, &owner, "principal-turn")
                .await
                .is_err()
        );
        drop(tx);
        let version: i64 =
            sqlx::query_scalar("SELECT agent_version FROM scheduled_runs WHERE id='principal-run'")
                .fetch_one(&db)
                .await
                .unwrap();
        assert_eq!(version, 7);
    }

    #[tokio::test]
    async fn startup_reconciles_principal_terminal_history_after_generation_change() {
        let (db, _, _) = principal_fixture().await;
        sqlx::query("UPDATE scheduled_runs SET status='waiting' WHERE id='principal-run'")
            .execute(&db)
            .await
            .unwrap();
        sqlx::query("UPDATE scheduled_actions SET enabled=0,configuration_generation='replacement',last_status='new-configuration' WHERE id='principal-action'").execute(&db).await.unwrap();
        chat::finalize_turn(&db, "principal-turn", chat::TurnStatus::Suspended, None)
            .await
            .unwrap();
        assert_eq!(
            reconcile_interrupted_waiting(&db, UsageSource::Scheduled)
                .await
                .unwrap(),
            0
        );
        chat::finalize_turn(
            &db,
            "principal-turn",
            chat::TurnStatus::Errored,
            Some("interrupted"),
        )
        .await
        .unwrap();
        assert_eq!(
            reconcile_interrupted_waiting(&db, UsageSource::Scheduled)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            reconcile_interrupted_waiting(&db, UsageSource::Scheduled)
                .await
                .unwrap(),
            0
        );
        let run: String =
            sqlx::query_scalar("SELECT status FROM scheduled_runs WHERE id='principal-run'")
                .fetch_one(&db)
                .await
                .unwrap();
        assert_eq!(run, "error");
        let summary: String = sqlx::query_scalar(
            "SELECT last_status FROM scheduled_actions WHERE id='principal-action'",
        )
        .fetch_one(&db)
        .await
        .unwrap();
        assert_eq!(summary, "new-configuration");
    }

    #[tokio::test]
    async fn background_turn_keeps_source_history_and_intersects_current_tools() {
        let db = fixture().await;
        let ctx = for_turn(&db, &AutomationOwner::User("owner".into()), "turn")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ctx.source, UsageSource::Scheduled);
        assert_eq!(ctx.history_limit, Some(6));
        assert!(ctx.tools_enabled);
        assert!(
            for_turn(&db, &AutomationOwner::User("other".into()), "turn")
                .await
                .is_err()
        );
        assert!(
            for_turn(&db, &AutomationOwner::User("owner".into()), "unknown")
                .await
                .unwrap()
                .is_none()
        );
        sqlx::query("UPDATE scheduled_actions SET tools_enabled=0,enabled=0")
            .execute(&db)
            .await
            .unwrap();
        assert!(
            !for_turn(&db, &AutomationOwner::User("owner".into()), "turn")
                .await
                .unwrap()
                .unwrap()
                .tools_enabled
        );
        sqlx::query("UPDATE scheduled_runs SET tools_enabled=0")
            .execute(&db)
            .await
            .unwrap();
        sqlx::query("UPDATE scheduled_actions SET tools_enabled=1")
            .execute(&db)
            .await
            .unwrap();
        assert!(
            !for_turn(&db, &AutomationOwner::User("owner".into()), "turn")
                .await
                .unwrap()
                .unwrap()
                .tools_enabled
        );
    }

    #[tokio::test]
    async fn changed_trigger_binding_and_missing_state_refuse_continuation() {
        let db = fixture().await;
        // A session bound to a *different* Dot than the run's configured Dot is
        // a mismatch and refuses continuation.
        sqlx::query(
            "INSERT INTO personal_dot_sessions(session_id,owner_user_id,dot_id,created_at)
             SELECT id,'owner','bound-dot',? FROM chat_sessions WHERE user_id='owner'",
        )
        .bind(jiff::Timestamp::now().to_string())
        .execute(&db)
        .await
        .unwrap();
        sqlx::query("UPDATE scheduled_actions SET dot_id='another-dot'")
            .execute(&db)
            .await
            .unwrap();
        assert!(
            for_turn(&db, &AutomationOwner::User("owner".into()), "turn")
                .await
                .is_err()
        );
        db.close().await;
        assert!(
            for_turn(&db, &AutomationOwner::User("owner".into()), "turn")
                .await
                .is_err()
        );
    }

    /// A Dot-configured run whose session is unbound (person) is a valid home —
    /// the Dot is optional — so `for_turn` accepts it instead of refusing.
    #[tokio::test]
    async fn dot_configured_run_accepts_an_unbound_session() {
        let db = fixture().await;
        sqlx::query("UPDATE scheduled_actions SET dot_id='some-dot'")
            .execute(&db)
            .await
            .unwrap();
        let ctx = for_turn(&db, &AutomationOwner::User("owner".into()), "turn")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ctx.source, UsageSource::Scheduled);
    }

    #[tokio::test]
    async fn reconcile_waiting_completed_and_cancelled_exact_run() {
        let db = fixture().await;
        reconcile_turn(&db, &AutomationOwner::User("owner".into()), "turn")
            .await
            .unwrap();
        let status: Option<String> =
            sqlx::query_scalar("SELECT status FROM scheduled_runs WHERE id='run'")
                .fetch_one(&db)
                .await
                .unwrap();
        assert!(status.is_none());
        chat::finalize_turn(&db, "turn", chat::TurnStatus::Suspended, None)
            .await
            .unwrap();
        reconcile_turn(&db, &AutomationOwner::User("owner".into()), "turn")
            .await
            .unwrap();
        let status: String = sqlx::query_scalar("SELECT status FROM scheduled_runs WHERE id='run'")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(status, "waiting");
        chat::finalize_turn(&db, "turn", chat::TurnStatus::Completed, None)
            .await
            .unwrap();
        reconcile_turn(&db, &AutomationOwner::User("owner".into()), "turn")
            .await
            .unwrap();
        let status: String = sqlx::query_scalar("SELECT last_status FROM scheduled_actions")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(status, "ok");
        chat::finalize_turn(&db, "turn", chat::TurnStatus::Cancelled, None)
            .await
            .unwrap();
        reconcile_turn(&db, &AutomationOwner::User("owner".into()), "turn")
            .await
            .unwrap();
        let status: String = sqlx::query_scalar("SELECT status FROM scheduled_runs WHERE id='run'")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(status, "error");
    }

    #[tokio::test]
    async fn older_continuation_does_not_replace_newer_summary() {
        let db = fixture().await;
        sqlx::query("INSERT INTO scheduled_runs(id,action_id,fired_at,status,created_at) VALUES('newer','action','2999-01-01T00:00:00Z','waiting','2999-01-01T00:00:00Z')").execute(&db).await.unwrap();
        sqlx::query("UPDATE scheduled_actions SET last_status='waiting',last_error='newer'")
            .execute(&db)
            .await
            .unwrap();
        chat::finalize_turn(&db, "turn", chat::TurnStatus::Completed, None)
            .await
            .unwrap();
        reconcile_turn(&db, &AutomationOwner::User("owner".into()), "turn")
            .await
            .unwrap();
        let status: String = sqlx::query_scalar("SELECT last_status FROM scheduled_actions")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(status, "waiting");
        let error: String = sqlx::query_scalar("SELECT last_error FROM scheduled_actions")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(error, "newer");
    }

    #[tokio::test]
    async fn webhook_context_reconciles_and_duplicate_cross_source_bindings_refuse() {
        let db = fixture().await;
        let now = Timestamp::now().to_string();
        sqlx::query("INSERT INTO webhooks(id,user_id,name,prompt,model,tools_enabled,secret_hash,created_at,updated_at) VALUES('hook','owner','events','prompt','model',1,'unique',?,?)").bind(&now).bind(&now).execute(&db).await.unwrap();
        sqlx::query("INSERT INTO webhook_runs(id,webhook_id,fired_at,session_id,assistant_turn_id,tools_enabled,history_limit,prompt,payload,source,created_at) SELECT 'hook-run','hook',?,session_id,'turn',1,4,'prompt','payload','fire',? FROM scheduled_runs WHERE id='run'").bind(&now).bind(&now).execute(&db).await.unwrap();
        assert!(
            for_turn(&db, &AutomationOwner::User("owner".into()), "turn")
                .await
                .is_err()
        );
        assert!(
            reconcile_turn(&db, &AutomationOwner::User("owner".into()), "turn")
                .await
                .is_err()
        );
        sqlx::query("DELETE FROM scheduled_runs WHERE id='run'")
            .execute(&db)
            .await
            .unwrap();
        let context = for_turn(&db, &AutomationOwner::User("owner".into()), "turn")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(context.source, UsageSource::Webhook);
        assert_eq!(context.history_limit, Some(4));
        chat::finalize_turn(&db, "turn", chat::TurnStatus::Completed, None)
            .await
            .unwrap();
        reconcile_turn(&db, &AutomationOwner::User("owner".into()), "turn")
            .await
            .unwrap();
        let status: String = sqlx::query_scalar("SELECT last_status FROM webhooks WHERE id='hook'")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(status, "ok");
    }

    #[tokio::test]
    async fn shared_correlated_conversation_is_not_a_private_background_context() {
        let db = fixture().await;
        sqlx::query("UPDATE chat_sessions SET shared=1")
            .execute(&db)
            .await
            .unwrap();
        assert!(
            for_turn(&db, &AutomationOwner::User("owner".into()), "turn")
                .await
                .is_err()
        );
        chat::finalize_turn(&db, "turn", chat::TurnStatus::Cancelled, None)
            .await
            .unwrap();
        reconcile_turn(&db, &AutomationOwner::User("owner".into()), "turn")
            .await
            .unwrap();
        let status: String = sqlx::query_scalar("SELECT status FROM scheduled_runs WHERE id='run'")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(status, "error");
    }

    #[tokio::test]
    async fn startup_reconciles_terminal_continuations_and_preserves_real_waiting_turns() {
        let db = fixture().await;
        sqlx::query("UPDATE scheduled_runs SET status='waiting'")
            .execute(&db)
            .await
            .unwrap();
        chat::finalize_turn(&db, "turn", chat::TurnStatus::Suspended, None)
            .await
            .unwrap();
        assert_eq!(
            reconcile_interrupted_waiting(&db, UsageSource::Scheduled)
                .await
                .unwrap(),
            0
        );
        chat::finalize_turn(&db, "turn", chat::TurnStatus::Errored, Some("interrupted"))
            .await
            .unwrap();
        assert_eq!(
            reconcile_interrupted_waiting(&db, UsageSource::Scheduled)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            reconcile_interrupted_waiting(&db, UsageSource::Scheduled)
                .await
                .unwrap(),
            0
        );
        let status: String =
            sqlx::query_scalar("SELECT last_status FROM scheduled_actions WHERE id='action'")
                .fetch_one(&db)
                .await
                .unwrap();
        assert_eq!(status, "error");
    }
}
