// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use aiplane_core::server::db::open;
use sqlx::{Executor, Row};
use std::path::Path;

const SCHEMA: &str = include_str!("fixtures/schema_after_dot_sessions.txt");

#[tokio::test]
async fn dot_background_schema_matches_its_fixture() {
    let pool = open(Path::new(":memory:")).await.unwrap();
    let rows = sqlx::query(
        "SELECT type, name, sql FROM sqlite_schema
         WHERE name IN ('personal_dot_sessions', 'idx_personal_dot_sessions_owner_dot',
                        'scheduled_actions', 'webhooks', 'scheduled_runs', 'webhook_runs',
                        'scheduled_runs_assistant_turn', 'webhook_runs_assistant_turn')
         ORDER BY type, name",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    let mut actual = String::new();
    for row in rows {
        let kind: String = row.get("type");
        let name: String = row.get("name");
        let sql: String = row.get("sql");
        actual.push_str(&format!(
            "{kind} {name}: {}\n",
            sql.split_whitespace().collect::<Vec<_>>().join(" ")
        ));
    }
    if std::env::var_os("UPDATE_SCHEMA_FIXTURE").is_some() {
        std::fs::write(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/schema_after_dot_sessions.txt"),
            &actual,
        )
        .unwrap();
    } else {
        assert_eq!(actual, SCHEMA);
    }
}

#[tokio::test]
async fn dot_background_upgrade_preserves_existing_triggers_and_terminal_history() {
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .in_memory(true)
        .foreign_keys(false);
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();
    for migration in sqlx::migrate!("./migrations")
        .iter()
        .filter(|migration| migration.version < 81)
    {
        pool.execute(sqlx::raw_sql(&migration.sql)).await.unwrap();
    }
    pool.execute(sqlx::raw_sql(
        "INSERT INTO users(id,email,created_at,updated_at) VALUES('owner','owner@example.test','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');
         INSERT INTO scheduled_actions(id,user_id,name,prompt,model,cron,timezone,created_at,updated_at)
         VALUES('action','owner','Existing','prompt','model','0 * * * *','UTC','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');
         INSERT INTO scheduled_runs(id,action_id,fired_at,status,created_at)
         VALUES('run','action','2026-01-01T00:00:00Z','ok','2026-01-01T00:00:00Z');
         INSERT INTO webhooks(id,user_id,name,prompt,model,secret_hash,created_at,updated_at)
         VALUES('hook','owner','Existing hook','prompt','model','testhash','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');
         INSERT INTO webhook_runs(id,webhook_id,fired_at,status,prompt,payload,source,created_at)
         VALUES('hook-run','hook','2026-01-01T00:00:00Z','ok','prompt','{}','fire','2026-01-01T00:00:00Z');"
    )).await.unwrap();
    let migrations = sqlx::migrate!("./migrations");
    let migration = migrations
        .iter()
        .find(|migration| migration.version == 81)
        .unwrap();
    pool.execute(sqlx::raw_sql(&migration.sql)).await.unwrap();
    for table in ["scheduled_actions", "webhooks"] {
        let dot: Option<String> = sqlx::query_scalar(&format!("SELECT dot_id FROM {table}"))
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(dot.is_none());
    }
    for table in ["scheduled_runs", "webhook_runs"] {
        let row = sqlx::query(&format!(
            "SELECT status, assistant_turn_id, tools_enabled, history_limit FROM {table}"
        ))
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.get::<String, _>("status"), "ok");
        assert!(row.get::<Option<String>, _>("assistant_turn_id").is_none());
        assert!(!row.get::<bool, _>("tools_enabled"));
        assert!(row.get::<Option<i64>, _>("history_limit").is_none());
    }
    pool.execute("PRAGMA foreign_keys = ON").await.unwrap();
    let violations = sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert!(violations.is_empty());
}

#[tokio::test]
async fn dot_session_tombstone_keeps_binding_without_retaining_deleted_owner_data() {
    let pool = open(Path::new(":memory:")).await.unwrap();
    pool.execute(sqlx::raw_sql(
        "INSERT INTO users(id,email,created_at,updated_at) VALUES('owner','owner@example.test','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');
         INSERT INTO chat_sessions(id,user_id,created_at,updated_at) VALUES('session','owner','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');"
    )).await.unwrap();
    sqlx::query("INSERT INTO personal_dot_sessions(session_id,owner_user_id,dot_id,created_at) VALUES('session','owner','deleted-dot','2026-01-01T00:00:00Z')")
        .execute(&pool).await.unwrap();
    let dot: String = sqlx::query_scalar("SELECT dot_id FROM personal_dot_sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(dot, "deleted-dot");
    pool.execute("DELETE FROM users WHERE id='owner'")
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM personal_dot_sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn child_binding_requires_complete_unique_relationship() {
    let pool = open(Path::new(":memory:")).await.unwrap();
    pool.execute(sqlx::raw_sql("INSERT INTO users(id,email,created_at,updated_at) VALUES('owner','owner@example.test','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z'); INSERT INTO chat_sessions(id,user_id,created_at,updated_at) VALUES('child','owner','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z'),('other','owner','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');")).await.unwrap();
    assert!(sqlx::query("INSERT INTO personal_dot_sessions(session_id,owner_user_id,dot_id,created_at,parent_turn_id) VALUES('child','owner','receiver','2026-01-01T00:00:00Z','parent')").execute(&pool).await.is_err());
    sqlx::query("INSERT INTO personal_dot_sessions(session_id,owner_user_id,dot_id,created_at,parent_turn_id,parent_tool_call_id,caller_dot_id,edge_generation) VALUES('child','owner','receiver','2026-01-01T00:00:00Z','parent','call','caller','generation')").execute(&pool).await.unwrap();
    assert!(sqlx::query("INSERT INTO personal_dot_sessions(session_id,owner_user_id,dot_id,created_at,parent_turn_id,parent_tool_call_id,caller_dot_id,edge_generation) VALUES('other','owner','receiver','2026-01-01T00:00:00Z','parent','call','caller','generation')").execute(&pool).await.is_err());
}
