// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use sqlx::Executor;

/// A connection a transient refresh failure parked is back in service; one the
/// provider declared dead still asks for a new sign-in.
#[tokio::test]
async fn connections_parked_by_a_transient_failure_return_to_service() {
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
    for migration in migrations.iter().filter(|m| m.version < 92) {
        pool.execute(sqlx::raw_sql(&migration.sql)).await.unwrap();
    }
    pool.execute(sqlx::raw_sql(
        "INSERT INTO mcp_connections (id, user_id, connector_key, status, generation,
              catalog_generation, created_at, updated_at)
         VALUES ('parked', 'u1', 'google', 'error', 'g1', 'c1', '2026-01-01', '2026-01-01'),
                ('dead', 'u2', 'google', 'reauth', 'g2', 'c1', '2026-01-01', '2026-01-01');",
    ))
    .await
    .unwrap();
    pool.execute(sqlx::raw_sql(
        &migrations.iter().find(|m| m.version == 92).unwrap().sql,
    ))
    .await
    .unwrap();

    let statuses: Vec<(String, String)> =
        sqlx::query_as("SELECT id, status FROM mcp_connections ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        statuses,
        [
            ("dead".to_string(), "reauth".to_string()),
            ("parked".to_string(), "connected".to_string())
        ]
    );
}
