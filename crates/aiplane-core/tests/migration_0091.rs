// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use aiplane_core::server::db::open;
use sqlx::{Executor, Row};
use std::path::Path;

const SCHEMA: &str = include_str!("fixtures/schema_after_bundled_approvals.txt");

/// A turn paused before the upgrade waits on its one call, as it did.
#[tokio::test]
async fn a_suspension_from_before_the_upgrade_bundles_nothing() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .in_memory(true)
                .foreign_keys(false),
        )
        .await
        .unwrap();
    let migrations = sqlx::migrate!("./migrations");
    for migration in migrations.iter().filter(|m| m.version < 91) {
        pool.execute(sqlx::raw_sql(&migration.sql)).await.unwrap();
    }
    pool.execute(sqlx::raw_sql(
        "INSERT INTO chat_turn_suspensions
             (turn_id, request_id, kind, tool_call, tail, budget_used, on_timeout,
              expires_at, created_at)
         VALUES ('t1', 'r1', 'approval', '{}', '[]', '{}', 'deny', '2026-01-01', '2026-01-01');",
    ))
    .await
    .unwrap();
    pool.execute(sqlx::raw_sql(
        &migrations.iter().find(|m| m.version == 91).unwrap().sql,
    ))
    .await
    .unwrap();

    let bundled: String =
        sqlx::query_scalar("SELECT bundled_calls FROM chat_turn_suspensions WHERE turn_id = 't1'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(bundled, "[]");
}

#[tokio::test]
async fn bundled_approvals_schema_matches_its_fixture() {
    let pool = open(Path::new(":memory:")).await.unwrap();
    let rows = sqlx::query(
        "SELECT type, name, sql FROM sqlite_schema
         WHERE tbl_name IN ('chat_turn_suspensions', 'chat_session_tool_approvals')
           AND sql IS NOT NULL
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
                .join("tests/fixtures/schema_after_bundled_approvals.txt"),
            &actual,
        )
        .unwrap();
    } else {
        assert_eq!(actual, SCHEMA);
    }
}

#[tokio::test]
async fn a_conversations_tool_approvals_go_with_it() {
    let pool = open(Path::new(":memory:")).await.unwrap();
    pool.execute(sqlx::raw_sql(
        "INSERT INTO users (id, email, roles_json, created_at, updated_at)
             VALUES ('alice', 'alice@example.test', '[]', '2026-01-01', '2026-01-01');
         INSERT INTO chat_sessions (id, user_id, created_at, updated_at)
             VALUES ('s1', 'alice', '2026-01-01', '2026-01-01');
         INSERT INTO chat_session_tool_approvals (session_id, tool, actions, approved_at)
             VALUES ('s1', 'erp_invoke', '', '2026-01-01');
         DELETE FROM chat_sessions WHERE id = 's1';",
    ))
    .await
    .unwrap();
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM chat_session_tool_approvals")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(left, 0);
}
