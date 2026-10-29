// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use sqlx::{Executor, Row};

async fn upgraded_from_0088(seed: &str) -> sqlx::SqlitePool {
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
    for migration in migrations.iter().filter(|migration| migration.version < 89) {
        pool.execute(sqlx::raw_sql(&migration.sql)).await.unwrap();
    }
    pool.execute(sqlx::raw_sql(seed)).await.unwrap();
    pool.execute(sqlx::raw_sql(
        &migrations.iter().find(|m| m.version == 89).unwrap().sql,
    ))
    .await
    .unwrap();
    pool.execute("PRAGMA foreign_keys = ON").await.unwrap();
    pool
}

/// Every row, setting and rule of a personal agent survives the rename into
/// the agent tables, and nothing keeps the old names.
#[tokio::test]
async fn upgrade_keeps_every_owned_agent_row_under_the_agent_names() {
    let pool = upgraded_from_0088(
        "INSERT INTO users (id, email, roles_json, created_at, updated_at) VALUES
             ('alice', 'alice@example.test', '[]', '2026-01-01', '2026-01-01');
         INSERT INTO system_principals (id, name, display, created_by, created_at) VALUES
             ('harald', 'harald', 'Harald', 'alice', '2026-01-01'),
             ('clara', 'clara', 'Clara', 'alice', '2026-01-01');
         INSERT INTO agents (principal_id, draft_spec, created_at, updated_at) VALUES
             ('harald', '{}', '2026-01-01', '2026-01-01'),
             ('clara', '{}', '2026-01-01', '2026-01-01');
         INSERT INTO dots (principal_id, owner_user_id, name, created_at, updated_at) VALUES
             ('harald', 'alice', 'Harald', '2026-01-01', '2026-01-01'),
             ('clara', 'alice', 'Clara', '2026-01-01', '2026-01-01');
         INSERT INTO dot_memories (id, dot_id, kind, content, created_at, updated_at) VALUES
             ('m1', 'harald', 'fact', 'Prefers short answers', '2026-01-01', '2026-01-01');
         INSERT INTO dot_delegation_edges (caller_dot_id, receiver_dot_id, outbound_enabled, inbound_enabled, generation, updated_at) VALUES
             ('harald', 'clara', 1, 1, 'g1', '2026-01-01');
         INSERT INTO gateway_groups (name, is_admin, is_default, created_at, updated_at, can_manage_dots) VALUES
             ('engineering', 0, 0, '2026-01-01', '2026-01-01', 1);
         INSERT INTO app_settings (key, value, updated_at) VALUES
             ('settings.dots.max_active_per_user', '6', '2026-01-01'),
             ('settings.dots.max_active_per_organization', '2', '2026-01-01');",
    )
    .await;

    let owners: Vec<String> =
        sqlx::query_scalar("SELECT principal_id FROM agent_owners ORDER BY principal_id")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(owners, ["clara", "harald"]);
    let memory = sqlx::query("SELECT agent_id, content FROM agent_memories")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(memory.get::<String, _>("agent_id"), "harald");
    assert_eq!(memory.get::<String, _>("content"), "Prefers short answers");
    let edge = sqlx::query("SELECT caller_agent_id, receiver_agent_id FROM agent_delegation_edges")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(edge.get::<String, _>("caller_agent_id"), "harald");
    assert_eq!(edge.get::<String, _>("receiver_agent_id"), "clara");
    let may_manage: bool = sqlx::query_scalar(
        "SELECT can_manage_personal_agents FROM gateway_groups WHERE name = 'engineering'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(may_manage);
    let settings: Vec<(String, String)> =
        sqlx::query_as("SELECT key, value FROM app_settings ORDER BY key")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        settings,
        [
            (
                "settings.agents.max_active_per_organization".to_owned(),
                "2".to_owned()
            ),
            (
                "settings.agents.max_active_per_user".to_owned(),
                "6".to_owned()
            ),
        ]
    );

    let stale: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_schema WHERE name LIKE '%dot%' OR sql LIKE '%dot%'",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(stale.is_empty(), "{stale:?}");
    assert!(
        sqlx::query("SELECT 1 FROM agent_owners")
            .fetch_all(&pool)
            .await
            .is_ok()
    );
    let violations = sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert!(violations.is_empty());
}

/// The renamed triggers still guard ownership and clean up after a deleted
/// owner.
#[tokio::test]
async fn upgraded_triggers_keep_ownership_immutable_and_queue_workspace_cleanup() {
    let pool = upgraded_from_0088(
        "INSERT INTO users (id, email, roles_json, created_at, updated_at) VALUES
             ('alice', 'alice@example.test', '[]', '2026-01-01', '2026-01-01'),
             ('bob', 'bob@example.test', '[]', '2026-01-01', '2026-01-01');
         INSERT INTO system_principals (id, name, display, created_by, created_at) VALUES
             ('harald', 'harald', 'Harald', 'alice', '2026-01-01');
         INSERT INTO agents (principal_id, draft_spec, created_at, updated_at) VALUES
             ('harald', '{}', '2026-01-01', '2026-01-01');
         INSERT INTO dots (principal_id, owner_user_id, name, created_at, updated_at) VALUES
             ('harald', 'alice', 'Harald', '2026-01-01', '2026-01-01');",
    )
    .await;

    let moved = pool
        .execute("UPDATE agent_owners SET owner_user_id = 'bob' WHERE principal_id = 'harald'")
        .await
        .unwrap_err();
    assert!(
        moved.to_string().contains("Agent ownership is immutable"),
        "{moved}"
    );
    pool.execute("DELETE FROM users WHERE id = 'alice'")
        .await
        .unwrap();
    let principals: i64 =
        sqlx::query_scalar("SELECT count(*) FROM system_principals WHERE id = 'harald'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(principals, 0);
    let queued: String = sqlx::query_scalar("SELECT agent_id FROM agent_workspace_cleanup")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(queued, "harald");
}
