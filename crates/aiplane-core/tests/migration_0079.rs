// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Migration 0079 moves every stored effort level and per-level reasoning
//! override onto the `off · low · medium · high · xhigh` scale. A conversation
//! or a model tuned on the old scale must think exactly as much afterwards as
//! it did before; these tests seed a database as the previous release left it,
//! apply 0079, and read what came out.

use std::str::FromStr;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Executor, Row, SqlitePool};

/// Every migration before 0079, run the way `db::open` runs them: with
/// foreign keys off, which the table rebuilds in 0077 require.
async fn db_before_0079() -> SqlitePool {
    let options = SqliteConnectOptions::from_str("sqlite::memory:")
        .expect("in-memory sqlite url")
        .foreign_keys(false);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .expect("in-memory sqlite");
    for m in sqlx::migrate!("./migrations").iter() {
        if m.version >= 79 {
            continue;
        }
        pool.execute(sqlx::raw_sql(&m.sql))
            .await
            .unwrap_or_else(|e| panic!("migration {} failed: {e}", m.version));
    }
    pool
}

async fn apply_0079(pool: &SqlitePool) {
    let m = sqlx::migrate!("./migrations")
        .iter()
        .find(|m| m.version == 79)
        .expect("migration 0079 exists")
        .clone();
    pool.execute(sqlx::raw_sql(&m.sql))
        .await
        .expect("0079 applies");
}

async fn seed_session(pool: &SqlitePool, id: &str, effort: Option<&str>) {
    sqlx::query(
        "INSERT OR IGNORE INTO users (id, email, created_at, updated_at)
         VALUES ('u', 'u@example.com', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO chat_sessions (id, user_id, created_at, updated_at)
         VALUES (?, 'u', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
    )
    .bind(id)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO chat_session_settings (session_id, effort, updated_at)
         VALUES (?, ?, '2026-01-01T00:00:00Z')",
    )
    .bind(id)
    .bind(effort)
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn every_stored_effort_moves_to_the_level_that_thinks_as_much() {
    let pool = db_before_0079().await;
    for (id, effort) in [
        ("fast", Some("fast")),
        ("standard", Some("standard")),
        ("deep", Some("deep")),
        ("max", Some("max")),
        ("unset", None),
    ] {
        seed_session(&pool, id, effort).await;
    }
    apply_0079(&pool).await;

    let rows = sqlx::query("SELECT session_id, effort FROM chat_session_settings")
        .fetch_all(&pool)
        .await
        .unwrap();
    let effort = |id: &str| -> Option<String> {
        rows.iter()
            .find(|r| r.get::<String, _>("session_id") == id)
            .and_then(|r| r.get("effort"))
    };
    assert_eq!(effort("fast").as_deref(), Some("off"));
    assert_eq!(effort("standard").as_deref(), Some("medium"));
    assert_eq!(effort("deep").as_deref(), Some("high"));
    assert_eq!(effort("max").as_deref(), Some("xhigh"));
    assert_eq!(
        effort("unset"),
        None,
        "no choice stays no choice: the default applies"
    );
}

#[tokio::test]
async fn per_level_overrides_keep_their_values_under_the_new_names() {
    let pool = db_before_0079().await;
    sqlx::query(
        "INSERT INTO model_defaults
           (model_name, defaults_toml, updated_at,
            thinking_budget_standard, thinking_budget_deep, thinking_budget_max,
            reasoning_effort_standard, reasoning_effort_deep, reasoning_effort_max)
         VALUES ('m', '', '2026-01-01T00:00:00Z', 1024, 4096, 8192, 'low', 'high', 'max')",
    )
    .execute(&pool)
    .await
    .unwrap();
    apply_0079(&pool).await;

    let row = sqlx::query(
        "SELECT thinking_budget_low, thinking_budget_medium, thinking_budget_high,
                thinking_budget_xhigh, reasoning_effort_low, reasoning_effort_medium,
                reasoning_effort_high, reasoning_effort_xhigh
           FROM model_defaults WHERE model_name = 'm'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.get::<Option<i64>, _>("thinking_budget_low"), None);
    assert_eq!(
        row.get::<Option<i64>, _>("thinking_budget_medium"),
        Some(1024)
    );
    assert_eq!(
        row.get::<Option<i64>, _>("thinking_budget_high"),
        Some(4096)
    );
    assert_eq!(
        row.get::<Option<i64>, _>("thinking_budget_xhigh"),
        Some(8192)
    );
    assert_eq!(row.get::<Option<String>, _>("reasoning_effort_low"), None);
    assert_eq!(
        row.get::<Option<String>, _>("reasoning_effort_medium")
            .as_deref(),
        Some("low")
    );
    assert_eq!(
        row.get::<Option<String>, _>("reasoning_effort_high")
            .as_deref(),
        Some("high")
    );
    assert_eq!(
        row.get::<Option<String>, _>("reasoning_effort_xhigh")
            .as_deref(),
        Some("max")
    );
}
