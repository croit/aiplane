// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use aiplane_core::server::db::usage::UsageSource;
use aiplane_core::server::db::{DbError, Pool};
use sqlx::Row;

#[derive(Debug)]
pub struct BackgroundTurn {
    pub source: UsageSource,
    pub history_limit: Option<usize>,
    pub tools_enabled: bool,
}

const MATCHED_RUNS: &str = "SELECT 'scheduled' AS source, r.id AS run_id, r.session_id, r.tools_enabled AS snapshot_tools, r.history_limit, a.user_id AS owner, a.tools_enabled AS current_tools, a.dot_id, s.user_id AS session_owner, s.principal_id, s.shared, p.dot_id AS session_dot FROM scheduled_runs r LEFT JOIN scheduled_actions a ON a.id=r.action_id LEFT JOIN chat_sessions s ON s.id=r.session_id LEFT JOIN personal_dot_sessions p ON p.session_id=s.id WHERE r.assistant_turn_id=? UNION ALL SELECT 'webhook', r.id, r.session_id, r.tools_enabled, r.history_limit, a.user_id, a.tools_enabled, a.dot_id, s.user_id, s.principal_id, s.shared, p.dot_id FROM webhook_runs r LEFT JOIN webhooks a ON a.id=r.webhook_id LEFT JOIN chat_sessions s ON s.id=r.session_id LEFT JOIN personal_dot_sessions p ON p.session_id=s.id WHERE r.assistant_turn_id=?";

fn invalid(message: &str) -> DbError {
    DbError::Decode {
        column: "background_turn",
        source: std::io::Error::other(message.to_string()).into(),
    }
}

pub async fn for_turn(
    db: &Pool,
    user_id: &str,
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
    let owner: Option<String> = row.try_get("owner")?;
    let session_owner: Option<String> = row.try_get("session_owner")?;
    let principal: Option<String> = row.try_get("principal_id")?;
    let dot: Option<String> = row.try_get("dot_id")?;
    let session_dot: Option<String> = row.try_get("session_dot")?;
    if owner.as_deref() != Some(user_id)
        || session_owner.as_deref() != Some(user_id)
        || principal.is_some()
        || row.try_get::<bool, _>("shared")?
        || dot != session_dot
    {
        return Err(invalid(
            "The background run no longer matches its owner and conversation binding. Restore its original binding before continuing.",
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

pub async fn reconcile_turn(
    db: &Pool,
    user_id: &str,
    assistant_turn_id: &str,
) -> Result<(), DbError> {
    let mut tx = aiplane_agents::db::WriteTx::begin(db).await?;
    let rows = sqlx::query(MATCHED_RUNS)
        .bind(assistant_turn_id)
        .bind(assistant_turn_id)
        .fetch_all(&mut *tx)
        .await?;
    if rows.is_empty() {
        return Ok(());
    }
    if rows.len() != 1 {
        return Err(invalid(
            "The background turn has multiple run bindings. Correct its run history before reconciling.",
        ));
    }
    let row = &rows[0];
    if row.try_get::<Option<String>, _>("owner")?.as_deref() != Some(user_id)
        || row
            .try_get::<Option<String>, _>("session_owner")?
            .as_deref()
            != Some(user_id)
    {
        return Err(invalid(
            "The background result does not belong to this user.",
        ));
    }
    let turn = sqlx::query(
        "SELECT status,error_message,session_id FROM chat_turns WHERE id=? AND role='assistant'",
    )
    .bind(assistant_turn_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| invalid("The background assistant turn is unavailable."))?;
    if turn.try_get::<String, _>("session_id")? != row.try_get::<String, _>("session_id")? {
        return Err(invalid(
            "The background assistant turn does not match its run's conversation.",
        ));
    }
    let status: String = turn.try_get("status")?;
    let mut error: Option<String> = turn.try_get("error_message")?;
    let status = match status.as_str() {
        "in_progress" => return Ok(()),
        "completed" => "ok",
        "suspended" => "waiting",
        "cancelled" => {
            error.get_or_insert_with(|| {
                "The run was cancelled while waiting for a decision.".into()
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
    sqlx::query(&format!("UPDATE {triggers} SET last_status=?,last_error=?,{last_at}=(SELECT fired_at FROM {runs} WHERE id=?),last_session_id=(SELECT session_id FROM {runs} WHERE id=?) WHERE id=(SELECT {foreign} FROM {runs} WHERE id=?) AND (SELECT id FROM {runs} WHERE {foreign}={triggers}.id ORDER BY rtrim(fired_at,'Z') DESC,rowid DESC LIMIT 1)=?"))
    .bind(status).bind(&error).bind(&run_id).bind(&run_id).bind(&run_id).bind(&run_id).execute(&mut *tx).await?;
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
        let rows=sqlx::query(&format!("SELECT r.rowid AS cursor,a.user_id,r.assistant_turn_id FROM {runs} r JOIN {triggers} a ON a.id=r.{foreign} JOIN chat_turns t ON t.id=r.assistant_turn_id WHERE r.rowid>? AND r.rowid<=? AND r.status='waiting' AND t.status IN ('completed','cancelled','errored') ORDER BY r.rowid LIMIT 128"))
            .bind(cursor).bind(max_row).fetch_all(db).await?;
        if rows.is_empty() {
            break;
        }
        for row in rows {
            cursor = row.try_get("cursor")?;
            let owner: String = row.try_get("user_id")?;
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
        sqlx::query("INSERT INTO scheduled_runs(id,action_id,fired_at,session_id,assistant_turn_id,tools_enabled,history_limit,created_at) VALUES('run','action',?,?, 'turn',1,6,?)").bind(now.to_string()).bind(&session.id).bind(now.to_string()).execute(&db).await.unwrap();
        db
    }

    #[tokio::test]
    async fn background_turn_keeps_source_history_and_intersects_current_tools() {
        let db = fixture().await;
        let ctx = for_turn(&db, "owner", "turn").await.unwrap().unwrap();
        assert_eq!(ctx.source, UsageSource::Scheduled);
        assert_eq!(ctx.history_limit, Some(6));
        assert!(ctx.tools_enabled);
        assert!(for_turn(&db, "other", "turn").await.is_err());
        assert!(for_turn(&db, "owner", "unknown").await.unwrap().is_none());
        sqlx::query("UPDATE scheduled_actions SET tools_enabled=0,enabled=0")
            .execute(&db)
            .await
            .unwrap();
        assert!(
            !for_turn(&db, "owner", "turn")
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
            !for_turn(&db, "owner", "turn")
                .await
                .unwrap()
                .unwrap()
                .tools_enabled
        );
    }

    #[tokio::test]
    async fn changed_trigger_binding_and_missing_state_refuse_continuation() {
        let db = fixture().await;
        sqlx::query("UPDATE scheduled_actions SET dot_id='another-dot'")
            .execute(&db)
            .await
            .unwrap();
        assert!(for_turn(&db, "owner", "turn").await.is_err());
        db.close().await;
        assert!(for_turn(&db, "owner", "turn").await.is_err());
    }

    #[tokio::test]
    async fn reconcile_waiting_completed_and_cancelled_exact_run() {
        let db = fixture().await;
        reconcile_turn(&db, "owner", "turn").await.unwrap();
        let status: Option<String> =
            sqlx::query_scalar("SELECT status FROM scheduled_runs WHERE id='run'")
                .fetch_one(&db)
                .await
                .unwrap();
        assert!(status.is_none());
        chat::finalize_turn(&db, "turn", chat::TurnStatus::Suspended, None)
            .await
            .unwrap();
        reconcile_turn(&db, "owner", "turn").await.unwrap();
        let status: String = sqlx::query_scalar("SELECT status FROM scheduled_runs WHERE id='run'")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(status, "waiting");
        chat::finalize_turn(&db, "turn", chat::TurnStatus::Completed, None)
            .await
            .unwrap();
        reconcile_turn(&db, "owner", "turn").await.unwrap();
        let status: String = sqlx::query_scalar("SELECT last_status FROM scheduled_actions")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(status, "ok");
        chat::finalize_turn(&db, "turn", chat::TurnStatus::Cancelled, None)
            .await
            .unwrap();
        reconcile_turn(&db, "owner", "turn").await.unwrap();
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
        reconcile_turn(&db, "owner", "turn").await.unwrap();
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
        assert!(for_turn(&db, "owner", "turn").await.is_err());
        assert!(reconcile_turn(&db, "owner", "turn").await.is_err());
        sqlx::query("DELETE FROM scheduled_runs WHERE id='run'")
            .execute(&db)
            .await
            .unwrap();
        let context = for_turn(&db, "owner", "turn").await.unwrap().unwrap();
        assert_eq!(context.source, UsageSource::Webhook);
        assert_eq!(context.history_limit, Some(4));
        chat::finalize_turn(&db, "turn", chat::TurnStatus::Completed, None)
            .await
            .unwrap();
        reconcile_turn(&db, "owner", "turn").await.unwrap();
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
        assert!(for_turn(&db, "owner", "turn").await.is_err());
        chat::finalize_turn(&db, "turn", chat::TurnStatus::Cancelled, None)
            .await
            .unwrap();
        reconcile_turn(&db, "owner", "turn").await.unwrap();
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
