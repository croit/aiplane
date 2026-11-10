// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use sqlx::Executor;

/// Dropping the tools-fallback column keeps every other stored fact of a
/// configured model: the column was never read at runtime, so its values die
/// with it and nothing else moves.
#[tokio::test]
async fn dropping_fallback_tools_keeps_the_rest_of_the_row() {
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
    for migration in migrations.iter().filter(|m| m.version < 94) {
        pool.execute(sqlx::raw_sql(&migration.sql)).await.unwrap();
    }
    pool.execute(sqlx::raw_sql(
        "INSERT INTO model_defaults
              (model_name, defaults_toml, cap_tools, fallback_tools, fallback_vision, updated_at)
         VALUES ('m', 'temperature = 0.5', 0, 'other-model', 'vl-model', '2026-01-01');",
    ))
    .await
    .unwrap();
    pool.execute(sqlx::raw_sql(
        &migrations.iter().find(|m| m.version == 94).unwrap().sql,
    ))
    .await
    .unwrap();

    let columns: Vec<String> =
        sqlx::query_scalar("SELECT name FROM pragma_table_xinfo('model_defaults') ORDER BY cid")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert!(
        !columns.contains(&"fallback_tools".to_string()),
        "{columns:?}"
    );
    assert!(
        columns.contains(&"fallback_vision".to_string()),
        "{columns:?}"
    );

    let row: (String, Option<i64>, Option<String>) = sqlx::query_as(
        "SELECT defaults_toml, cap_tools, fallback_vision FROM model_defaults WHERE model_name = 'm'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        row,
        (
            "temperature = 0.5".to_string(),
            Some(0),
            Some("vl-model".to_string())
        )
    );
}
