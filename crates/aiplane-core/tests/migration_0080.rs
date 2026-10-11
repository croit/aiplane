// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use aiplane_core::server::db::open;
use sqlx::{Executor, Row};
use std::path::Path;

#[tokio::test]
async fn personal_dot_management_is_false_for_existing_groups() {
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
    for migration in migrations.iter().filter(|migration| migration.version < 80) {
        pool.execute(sqlx::raw_sql(&migration.sql)).await.unwrap();
    }
    pool.execute("INSERT INTO gateway_groups (name, description, is_admin, is_default, can_manage_agents, created_at, updated_at) VALUES ('existing', 'Existing group', 0, 1, 1, '2026-01-01', '2026-01-01')")
        .await.unwrap();
    let migration = migrations
        .iter()
        .find(|migration| migration.version == 80)
        .unwrap();
    pool.execute(sqlx::raw_sql(&migration.sql)).await.unwrap();
    let row = sqlx::query("SELECT is_default, can_manage_agents, can_manage_dots FROM gateway_groups WHERE name = 'existing'")
        .fetch_one(&pool).await.unwrap();
    assert!(row.get::<bool, _>("is_default"));
    assert!(row.get::<bool, _>("can_manage_agents"));
    assert!(!row.get::<bool, _>("can_manage_dots"));
    assert!(
        pool.execute("UPDATE gateway_groups SET can_manage_dots = 2 WHERE name = 'existing'")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn dot_group_permission_schema_matches_its_fixture() {
    let pool = open(Path::new(":memory:")).await.unwrap();
    let sql: String = sqlx::query_scalar(
        "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = 'gateway_groups'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let actual = sql.split_whitespace().collect::<Vec<_>>().join(" ") + "\n";
    if std::env::var_os("UPDATE_SCHEMA_FIXTURE").is_some() {
        std::fs::write(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/schema_after_dot_permissions.txt"),
            &actual,
        )
        .unwrap();
    } else {
        assert_eq!(
            actual,
            include_str!("fixtures/schema_after_dot_permissions.txt")
        );
    }
}
