// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use aiplane_core::server::db::open;
use sqlx::{Executor, Row};
use std::path::Path;

const SCHEMA: &str = include_str!("fixtures/schema_after_backend_extra_headers.txt");

/// The column arrives as an empty object on every existing backend: a
/// deployment upgrading with a configured topology must keep probing and
/// routing exactly as before, with no header behaviour changed.
#[tokio::test]
async fn upgrade_defaults_extra_headers_to_an_empty_object() {
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
    for migration in migrations.iter().filter(|migration| migration.version < 83) {
        pool.execute(sqlx::raw_sql(&migration.sql)).await.unwrap();
    }
    pool.execute(
        "INSERT INTO backends(name, base_url, weight, max_inflight, health_path, supports_edit, enabled, priority, created_at, updated_at)
         VALUES('ollama', 'http://ollama:11434/v1', 1, 16, '/models', 0, 1, '{}', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'),
               ('anthropic', 'https://api.anthropic.com/v1', 2, 32, '/models', 0, 1, '{}', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
    )
    .await
    .unwrap();
    pool.execute(sqlx::raw_sql(
        &migrations.iter().find(|m| m.version == 83).unwrap().sql,
    ))
    .await
    .unwrap();
    for name in ["ollama", "anthropic"] {
        let row = sqlx::query(&format!("SELECT * FROM backends WHERE name='{name}'"))
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(row.get::<String, _>("extra_headers"), "{}");
        assert_eq!(row.get::<String, _>("base_url"), {
            if name == "ollama" {
                "http://ollama:11434/v1"
            } else {
                "https://api.anthropic.com/v1"
            }
        });
    }
}

/// The stored JSON object round-trips through the database layer: what the
/// admin form saved is what the registry reads back, byte for byte.
#[tokio::test]
async fn the_header_map_round_trips_through_the_database_layer() {
    let pool = open(Path::new(":memory:")).await.unwrap();
    let mut headers = std::collections::BTreeMap::new();
    headers.insert("anthropic-version".to_string(), "2023-06-01".to_string());
    let row = aiplane_core::server::db::upstreams_config::BackendRow {
        priority: Default::default(),
        name: "anthropic".to_string(),
        base_url: "https://api.anthropic.com/v1".to_string(),
        api_key_env: None,
        api_key_ct: None,
        api_key_nonce: None,
        weight: 1,
        max_inflight: 16,
        health_path: "/models".to_string(),
        supports_edit: false,
        enabled: true,
        extra_headers: headers,
        models: Vec::new(),
        aliases: Vec::new(),
        created_at: jiff::Timestamp::now(),
        updated_at: jiff::Timestamp::now(),
    };
    aiplane_core::server::db::upstreams_config::upsert_backend(&pool, &row)
        .await
        .unwrap();
    let stored: String =
        sqlx::query_scalar("SELECT extra_headers FROM backends WHERE name='anthropic'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stored, "{\"anthropic-version\":\"2023-06-01\"}");
    let loaded = aiplane_core::server::db::upstreams_config::get_backend(&pool, "anthropic")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        loaded.extra_headers.get("anthropic-version"),
        Some(&"2023-06-01".to_string())
    );
}

#[tokio::test]
async fn backends_schema_matches_its_fixture() {
    let pool = open(Path::new(":memory:")).await.unwrap();
    let rows = sqlx::query(
        "SELECT type, name, sql FROM sqlite_schema
         WHERE name IN ('backends')
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
                .join("tests/fixtures/schema_after_backend_extra_headers.txt"),
            &actual,
        )
        .unwrap();
    } else {
        assert_eq!(actual, SCHEMA);
    }
}
