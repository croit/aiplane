// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use aiplane_core::server::db::open;
use sqlx::{Executor, Row};
use std::path::Path;

const SCHEMA: &str = include_str!("fixtures/schema_after_dot_sessions.txt");

#[tokio::test]
async fn connector_upgrade_preserves_sealed_credentials_and_newest_authorization() {
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .in_memory(true)
        .foreign_keys(false);
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();
    let migrations = sqlx::migrate!("./migrations");
    for migration in migrations.iter().filter(|migration| migration.version < 81) {
        pool.execute(sqlx::raw_sql(&migration.sql)).await.unwrap();
    }
    pool.execute(sqlx::raw_sql(
        "INSERT INTO users(id,email,created_at,updated_at) VALUES('owner','owner@example.test','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');
         INSERT INTO user_mcp_connections(id,user_id,connector_key,status,access_token_ct,access_token_nonce,refresh_token_ct,refresh_token_nonce,dcr_client_secret_ct,dcr_client_secret_nonce,created_at,updated_at)
         VALUES('original','owner','workspace','connected',x'0001ff',x'0203',x'0405',x'0607',x'0809',x'0a0b','2026-01-01T00:00:00Z','2026-02-01T00:00:00Z');"
    )).await.unwrap();
    for (state, created_at) in [
        ("whole-second", "2026-03-01T00:00:00Z"),
        ("fraction-a", "2026-03-01T00:00:00.1Z"),
        ("fraction-z", "2026-03-01T00:00:00.1Z"),
    ] {
        sqlx::query("INSERT INTO pending_mcp_oauth(state,user_id,connector_key,pkce_verifier,redirect_uri,token_url,created_at,expires_at) VALUES(?,'owner','workspace','verifier','https://plane.example/integrations/callback','https://provider.example/token',?,'2026-03-01T00:10:00Z')")
            .bind(state).bind(created_at).execute(&pool).await.unwrap();
    }
    pool.execute(sqlx::raw_sql(
        &migrations
            .iter()
            .find(|migration| migration.version == 81)
            .unwrap()
            .sql,
    ))
    .await
    .unwrap();
    let row = sqlx::query("SELECT * FROM mcp_connections WHERE id='original'")
        .fetch_one(&pool)
        .await
        .unwrap();
    for (column, bytes) in [
        ("access_token_ct", vec![0, 1, 255]),
        ("access_token_nonce", vec![2, 3]),
        ("refresh_token_ct", vec![4, 5]),
        ("refresh_token_nonce", vec![6, 7]),
        ("dcr_client_secret_ct", vec![8, 9]),
        ("dcr_client_secret_nonce", vec![10, 11]),
    ] {
        assert_eq!(row.get::<Vec<u8>, _>(column), bytes);
    }
    assert_eq!(row.get::<String, _>("status"), "connected");
    assert_eq!(row.get::<String, _>("created_at"), "2026-01-01T00:00:00Z");
    assert_eq!(row.get::<String, _>("updated_at"), "2026-02-01T00:00:00Z");
    assert!(!row.get::<String, _>("generation").is_empty());
    let pending = sqlx::query("SELECT state,connection_id,expected_connection_generation,authorization_generation FROM pending_mcp_oauth")
        .fetch_all(&pool).await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].get::<String, _>("state"), "fraction-z");
    assert_eq!(pending[0].get::<String, _>("connection_id"), "original");
    assert_eq!(
        pending[0].get::<String, _>("expected_connection_generation"),
        row.get::<String, _>("generation")
    );
    assert_eq!(
        pending[0].get::<String, _>("authorization_generation"),
        row.get::<String, _>("authorization_generation")
    );
    pool.execute("PRAGMA foreign_keys = ON").await.unwrap();
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&pool)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn connector_owner_constraints_and_independent_principal_lifetime_are_enforced() {
    let pool = open(Path::new(":memory:")).await.unwrap();
    pool.execute(sqlx::raw_sql(
        "INSERT INTO users(id,email,created_at,updated_at) VALUES('owner','owner@example.test','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');
         INSERT INTO system_principals(id,name,display,description,created_by,created_at) VALUES('service','Service','Service','','owner','2026-01-01T00:00:00Z');"
    )).await.unwrap();
    for (id, user, principal) in [
        ("neither", None, None),
        ("both", Some("owner"), Some("service")),
    ] {
        assert!(sqlx::query("INSERT INTO mcp_connections(id,user_id,principal_id,connector_key,generation,status,created_at,updated_at) VALUES(?,?,?,'workspace','generation','connected','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')")
            .bind(id).bind(user).bind(principal).execute(&pool).await.is_err());
    }
    pool.execute(sqlx::raw_sql(
        "INSERT INTO mcp_connections(id,user_id,principal_id,connector_key,generation,status,created_at,updated_at)
         VALUES('user','owner',NULL,'workspace','user-generation','connected','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z'),
         ('principal',NULL,'service','workspace','principal-generation','connected','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');"
    )).await.unwrap();
    assert!(pool.execute("INSERT INTO mcp_connections(id,user_id,connector_key,generation,status,created_at,updated_at) VALUES('duplicate','owner','workspace','other','connected','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')").await.is_err());
    pool.execute("DELETE FROM users WHERE id='owner'")
        .await
        .unwrap();
    let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM mcp_connections ORDER BY id")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(ids, ["principal"]);
    pool.execute("DELETE FROM system_principals WHERE id='service'")
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM mcp_connections")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn dot_background_schema_matches_its_fixture() {
    let pool = open(Path::new(":memory:")).await.unwrap();
    let rows = sqlx::query(
        "SELECT type, name, sql FROM sqlite_schema
         WHERE name IN ('personal_dot_sessions', 'idx_personal_dot_sessions_owner_dot',
                        'scheduled_actions', 'webhooks', 'scheduled_runs', 'webhook_runs',
                        'scheduled_runs_assistant_turn', 'webhook_runs_assistant_turn',
                        'mcp_connections', 'mcp_connections_user_connector',
                        'mcp_connections_principal_connector', 'pending_mcp_oauth',
                        'idx_pending_mcp_oauth_expires', 'dot_connector_bindings')
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
