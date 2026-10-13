// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use aiplane_core::server::db::open;
use sqlx::{Executor, Row};
use std::path::Path;

const SCHEMA: &str = include_str!("fixtures/schema_after_dot_sessions.txt");

#[tokio::test]
async fn connector_catalogue_upgrade_preserves_tool_cache_and_pins_known_provenance() {
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
    for migration in migrations.iter().filter(|migration| migration.version < 81) {
        pool.execute(sqlx::raw_sql(&migration.sql)).await.unwrap();
    }
    pool.execute("PRAGMA foreign_keys = ON").await.unwrap();
    pool.execute(sqlx::raw_sql(
        "INSERT INTO users(id,email,created_at,updated_at) VALUES('owner','owner@example.test','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');
         INSERT INTO mcp_catalog_connectors(key,name,url,client_secret_ct,client_secret_nonce,created_at,updated_at) VALUES('workspace','Actual Workspace','https://workspace.example.test/mcp',x'0001ff',x'0203','2026-01-01T00:00:00Z','2026-02-01T00:00:00Z');
         INSERT INTO mcp_connector_tools(connector_key,tool_id,description,seen_at) VALUES('workspace','mcp__workspace__read','Actual tool description','2026-02-01T00:00:00Z');
         INSERT INTO user_mcp_connections(id,user_id,connector_key,status,access_token_ct,access_token_nonce,created_at,updated_at) VALUES('known','owner','workspace','connected',x'0405',x'0607','2026-01-01T00:00:00Z','2026-02-01T00:00:00Z'),('orphan','owner','retired','connected',x'0809',x'0a0b','2026-01-01T00:00:00Z','2026-02-01T00:00:00Z');
         INSERT INTO pending_mcp_oauth(state,user_id,connector_key,pkce_verifier,redirect_uri,token_url,created_at,expires_at) VALUES('known-state','owner','workspace','verifier','https://plane.example/integrations/callback','https://provider.example/token','2026-03-01T00:00:00Z','2026-03-01T00:10:00Z'),('orphan-state','owner','retired','verifier','https://plane.example/integrations/callback','https://retired.example/token','2026-03-01T00:00:00Z','2026-03-01T00:10:00Z');"
    )).await.unwrap();
    pool.execute(sqlx::raw_sql(
        &migrations.iter().find(|m| m.version == 81).unwrap().sql,
    ))
    .await
    .unwrap();
    let catalog = sqlx::query("SELECT * FROM mcp_catalog_connectors WHERE key='workspace'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let generation: String = catalog.try_get("dispatch_generation").unwrap();
    assert!(!generation.is_empty());
    assert_eq!(
        catalog.get::<Vec<u8>, _>("client_secret_ct"),
        vec![0, 1, 255]
    );
    assert_eq!(catalog.get::<Vec<u8>, _>("client_secret_nonce"), vec![2, 3]);
    assert_eq!(
        catalog.get::<String, _>("updated_at"),
        "2026-02-01T00:00:00Z"
    );
    let tool = sqlx::query("SELECT * FROM mcp_connector_tools WHERE connector_key='workspace'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(tool.get::<String, _>("tool_id"), "mcp__workspace__read");
    assert_eq!(
        tool.get::<String, _>("description"),
        "Actual tool description"
    );
    for (table, identifier, known, orphan) in [
        ("mcp_connections", "id", "known", "orphan"),
        ("pending_mcp_oauth", "state", "known-state", "orphan-state"),
    ] {
        let sql = format!("SELECT catalog_generation FROM {table} WHERE {identifier}=?");
        assert_eq!(
            sqlx::query_scalar::<_, Option<String>>(&sql)
                .bind(known)
                .fetch_one(&pool)
                .await
                .unwrap()
                .as_deref(),
            Some(generation.as_str())
        );
        assert_eq!(
            sqlx::query_scalar::<_, Option<String>>(&sql)
                .bind(orphan)
                .fetch_one(&pool)
                .await
                .unwrap(),
            None
        );
    }
    assert!(
        sqlx::query(
            "UPDATE mcp_catalog_connectors SET dispatch_generation='' WHERE key='workspace'"
        )
        .execute(&pool)
        .await
        .is_err()
    );
    assert!(
        sqlx::query(
            "UPDATE mcp_catalog_connectors SET dispatch_generation=NULL WHERE key='workspace'"
        )
        .execute(&pool)
        .await
        .is_err()
    );
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&pool)
            .await
            .unwrap()
            .is_empty()
    );
    sqlx::query("DELETE FROM mcp_catalog_connectors WHERE key='workspace'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM mcp_connector_tools")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM mcp_connections")
            .fetch_one(&pool)
            .await
            .unwrap(),
        2
    );
}

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
                        'scheduled_actions_principal', 'scheduled_actions_owner_immutable',
                        'webhooks_principal', 'webhooks_owner_immutable',
                        'mcp_connections', 'mcp_connections_user_connector',
                        'mcp_connections_principal_connector', 'pending_mcp_oauth',
                        'idx_pending_mcp_oauth_expires', 'mcp_oauth_cancel_before_initiator_delete', 'dot_connector_bindings',
                        'cross_boundary_delegations', 'idx_cross_boundary_delegations_caller',
                        'mcp_catalog_connectors', 'mcp_connector_tools', 'agent_architect_sessions')
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
async fn organization_schedule_owner_is_exclusive_and_survives_creator_deletion() {
    let pool = open(Path::new(":memory:")).await.unwrap();
    pool.execute(sqlx::raw_sql(
        "INSERT INTO users(id,email,created_at,updated_at) VALUES('creator','creator@example.test','2026-01-01','2026-01-01');
         INSERT INTO organizations(id,name,description,revision,created_by,created_at,updated_at) VALUES('acme','Acme','','initial','creator','2026-01-01','2026-01-01');
         INSERT INTO system_principals(id,name,display,created_by,created_at) VALUES('organization','organization','Harald','creator','2026-01-01');
         INSERT INTO agents(principal_id,draft_spec,created_at,updated_at) VALUES('organization','{}','2026-01-01','2026-01-01');
         INSERT INTO dots(principal_id,owner_user_id,organization_id,name,created_at,updated_at) VALUES('organization',NULL,'acme','Harald','2026-01-01','2026-01-01');
         INSERT INTO scheduled_actions(id,principal_id,agent_version,configuration_generation,name,prompt,cron,timezone,created_at,updated_at)
         VALUES('org-action','organization',1,'original','Org action','Watch tickets','0 * * * *','Europe/Berlin','2026-01-01','2026-01-01');
         INSERT INTO scheduled_actions(id,user_id,name,prompt,model,cron,timezone,created_at,updated_at)
         VALUES('user-action','creator','User action','Digest','model','0 * * * *','UTC','2026-01-01','2026-01-01');
         INSERT INTO scheduled_runs(id,action_id,fired_at,status,configuration_generation,agent_version,created_at)
         VALUES('org-run','org-action','2026-01-01','ok','original',1,'2026-01-01'),('user-run','user-action','2026-01-01','ok',NULL,NULL,'2026-01-01');"
    )).await.unwrap();
    for mutation in [
        "UPDATE scheduled_actions SET user_id='creator' WHERE id='org-action'",
        "UPDATE scheduled_actions SET principal_id=NULL,user_id='creator',model='model',agent_version=NULL WHERE id='org-action'",
        "UPDATE scheduled_actions SET agent_version=0 WHERE id='org-action'",
        "UPDATE scheduled_actions SET model='employee-model' WHERE id='org-action'",
        "UPDATE scheduled_actions SET dot_id='organization' WHERE id='org-action'",
        "UPDATE scheduled_actions SET configuration_generation='' WHERE id='org-action'",
        "DELETE FROM system_principals WHERE id='organization'",
    ] {
        assert!(pool.execute(mutation).await.is_err(), "{mutation}");
    }
    pool.execute("DELETE FROM users WHERE id='creator'")
        .await
        .unwrap();
    let source = sqlx::query("SELECT user_id,principal_id,agent_version,configuration_generation,model FROM scheduled_actions")
        .fetch_one(&pool).await.unwrap();
    assert!(source.get::<Option<String>, _>("user_id").is_none());
    assert_eq!(source.get::<String, _>("principal_id"), "organization");
    assert_eq!(source.get::<i64, _>("agent_version"), 1);
    assert_eq!(
        source.get::<String, _>("configuration_generation"),
        "original"
    );
    assert!(source.get::<Option<String>, _>("model").is_none());
    let runs: Vec<String> = sqlx::query_scalar("SELECT id FROM scheduled_runs ORDER BY id")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(runs, ["org-run"]);
    pool.execute("DELETE FROM scheduled_actions WHERE id='org-action'")
        .await
        .unwrap();
    pool.execute("DELETE FROM system_principals WHERE id='organization'")
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM scheduled_runs")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&pool)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn organization_schedule_upgrade_preserves_user_history_with_foreign_keys_enabled() {
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
    for migration in migrations.iter().filter(|migration| migration.version < 81) {
        pool.execute(sqlx::raw_sql(&migration.sql)).await.unwrap();
    }
    pool.execute("PRAGMA foreign_keys = ON").await.unwrap();
    pool.execute(sqlx::raw_sql(
        "INSERT INTO users(id,email,created_at,updated_at) VALUES('owner','owner@example.test','2026-01-01','2026-01-01');
         INSERT INTO scheduled_actions(id,user_id,name,prompt,model,cron,timezone,reuse_conversation,reuse_rounds,enabled,next_run_at,last_run_at,last_status,last_error,created_at,updated_at)
         VALUES('old','owner','Actual name','Actual prompt','actual-model','0 9 * * *','Europe/Berlin',1,7,0,NULL,'2026-02-01','error','Actual error','2026-01-01','2026-02-01');
         INSERT INTO scheduled_runs(id,action_id,fired_at,status,error,created_at)
         VALUES('old-run','old','2026-02-01','error','Actual error','2026-02-01');"
    )).await.unwrap();
    pool.execute(sqlx::raw_sql(
        &migrations
            .iter()
            .find(|migration| migration.version == 81)
            .unwrap()
            .sql,
    ))
    .await
    .unwrap();
    let source = sqlx::query("SELECT * FROM scheduled_actions WHERE id='old'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(source.get::<String, _>("user_id"), "owner");
    assert!(source.get::<Option<String>, _>("principal_id").is_none());
    assert!(source.get::<Option<i64>, _>("agent_version").is_none());
    assert!(source.get::<Option<String>, _>("created_by").is_none());
    assert!(
        !source
            .get::<String, _>("configuration_generation")
            .is_empty()
    );
    assert_eq!(source.get::<String, _>("name"), "Actual name");
    assert_eq!(source.get::<String, _>("prompt"), "Actual prompt");
    assert_eq!(source.get::<String, _>("model"), "actual-model");
    assert_eq!(source.get::<String, _>("timezone"), "Europe/Berlin");
    assert_eq!(source.get::<i64, _>("reuse_rounds"), 7);
    assert_eq!(source.get::<i64, _>("reuse_conversation"), 1);
    assert_eq!(source.get::<i64, _>("enabled"), 0);
    assert!(source.get::<Option<String>, _>("next_run_at").is_none());
    assert_eq!(source.get::<String, _>("last_error"), "Actual error");
    let run = sqlx::query("SELECT * FROM scheduled_runs WHERE id='old-run'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(run.get::<String, _>("action_id"), "old");
    assert_eq!(run.get::<String, _>("error"), "Actual error");
    assert!(
        run.get::<Option<String>, _>("configuration_generation")
            .is_none()
    );
    assert!(run.get::<Option<i64>, _>("agent_version").is_none());
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&pool)
            .await
            .unwrap()
            .is_empty()
    );
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
         INSERT INTO webhooks(id,user_id,name,prompt,model,tools_enabled,synchronous,secret_hash,enabled,last_fired_at,last_status,last_session_id,last_error,last_payload,created_at,updated_at,reuse_conversation,reuse_rounds)
         VALUES('hook','owner','Existing hook','Actual instruction','actual-model',1,1,'testhash',0,'2026-02-01T12:34:56Z','error','recorded-session','Actual summary error','{\"event\":\"last payload\"}','2026-01-01T00:00:00Z','2026-02-02T00:00:00Z',1,17);
         INSERT INTO webhook_runs(id,webhook_id,fired_at,status,session_id,error,prompt,payload,source,created_at)
         VALUES('hook-run','hook','2026-02-01T12:34:56Z','ok','recorded-session','Actual history detail','Actual historical instruction','{\"event\":\"historical payload\"}','rerun','2026-02-01T12:34:57Z');"
    )).await.unwrap();
    pool.execute("PRAGMA foreign_keys = ON").await.unwrap();
    let mut historical_snapshots = Vec::new();
    for table in ["webhooks", "webhook_runs"] {
        let columns: Vec<String> = sqlx::query(&format!("PRAGMA table_info({table})"))
            .fetch_all(&pool)
            .await
            .unwrap()
            .iter()
            .map(|row| row.get("name"))
            .collect();
        let fields = columns
            .iter()
            .map(|column| format!("'{column}',{column}"))
            .collect::<Vec<_>>()
            .join(",");
        let query = format!("SELECT json_object({fields}) FROM {table}");
        let before: String = sqlx::query_scalar(&query).fetch_one(&pool).await.unwrap();
        historical_snapshots.push((query, before));
    }
    let migrations = sqlx::migrate!("./migrations");
    let migration = migrations
        .iter()
        .find(|migration| migration.version == 81)
        .unwrap();
    pool.execute(sqlx::raw_sql(&migration.sql)).await.unwrap();
    for (query, before) in historical_snapshots {
        let after: String = sqlx::query_scalar(&query).fetch_one(&pool).await.unwrap();
        assert_eq!(after, before);
    }
    let hook = sqlx::query(
        "SELECT created_by,principal_id,agent_version,configuration_generation FROM webhooks",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    for column in ["created_by", "principal_id"] {
        assert!(hook.get::<Option<String>, _>(column).is_none());
    }
    assert!(hook.get::<Option<i64>, _>("agent_version").is_none());
    assert!(!hook.get::<String, _>("configuration_generation").is_empty());
    let run = sqlx::query("SELECT agent_version,configuration_generation FROM webhook_runs")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(run.get::<Option<i64>, _>("agent_version").is_none());
    assert!(
        run.get::<Option<String>, _>("configuration_generation")
            .is_none()
    );
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

#[tokio::test]
async fn organization_webhook_owner_pin_and_identity_survive_creator_deletion() {
    let pool = open(Path::new(":memory:")).await.unwrap();
    pool.execute(sqlx::raw_sql(
        "INSERT INTO users(id,email,created_at,updated_at) VALUES('creator','creator@example.test','2026-01-01','2026-01-01');
         INSERT INTO organizations(id,name,description,revision,created_by,created_at,updated_at) VALUES('acme','Acme','','initial','creator','2026-01-01','2026-01-01');
         INSERT INTO system_principals(id,name,display,created_by,created_at) VALUES('organization','organization','Harald','creator','2026-01-01');
         INSERT INTO agents(principal_id,draft_spec,created_at,updated_at) VALUES('organization','{}','2026-01-01','2026-01-01');
         INSERT INTO dots(principal_id,owner_user_id,organization_id,name,created_at,updated_at) VALUES('organization',NULL,'acme','Harald','2026-01-01','2026-01-01');
         INSERT INTO webhooks(id,principal_id,created_by,agent_version,configuration_generation,name,prompt,secret_hash,created_at,updated_at)
         VALUES('org-hook','organization','creator',1,'original','Org event','Watch tickets','org-hash','2026-01-01','2026-01-01');
         INSERT INTO webhooks(id,user_id,name,prompt,model,secret_hash,created_at,updated_at)
         VALUES('user-hook','creator','User event','Digest','model','user-hash','2026-01-01','2026-01-01');
         INSERT INTO webhook_runs(id,webhook_id,fired_at,status,prompt,payload,source,configuration_generation,agent_version,created_at)
         VALUES('org-run','org-hook','2026-01-01','ok','Watch tickets','{}','fire','original',1,'2026-01-01'),('user-run','user-hook','2026-01-01','ok','Digest','{}','fire',NULL,NULL,'2026-01-01');"
    )).await.unwrap();
    for mutation in [
        "UPDATE webhooks SET user_id='creator' WHERE id='org-hook'",
        "UPDATE webhooks SET principal_id=NULL,user_id='creator',model='model',agent_version=NULL WHERE id='org-hook'",
        "UPDATE webhooks SET agent_version=0 WHERE id='org-hook'",
        "UPDATE webhooks SET model='employee-model' WHERE id='org-hook'",
        "UPDATE webhooks SET dot_id='organization' WHERE id='org-hook'",
        "UPDATE webhooks SET reuse_conversation=1 WHERE id='org-hook'",
        "UPDATE webhooks SET synchronous=1 WHERE id='org-hook'",
        "UPDATE webhooks SET configuration_generation='' WHERE id='org-hook'",
        "DELETE FROM system_principals WHERE id='organization'",
    ] {
        assert!(pool.execute(mutation).await.is_err(), "{mutation}");
    }
    pool.execute("DELETE FROM users WHERE id='creator'")
        .await
        .unwrap();
    let hook = sqlx::query("SELECT * FROM webhooks")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(hook.get::<Option<String>, _>("user_id").is_none());
    assert_eq!(hook.get::<String, _>("principal_id"), "organization");
    assert_eq!(hook.get::<String, _>("created_by"), "creator");
    assert_eq!(hook.get::<i64, _>("agent_version"), 1);
    assert_eq!(
        hook.get::<String, _>("configuration_generation"),
        "original"
    );
    assert!(hook.get::<Option<String>, _>("model").is_none());
    let runs: Vec<String> = sqlx::query_scalar("SELECT id FROM webhook_runs ORDER BY id")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(runs, ["org-run"]);
    pool.execute("DELETE FROM webhooks WHERE id='org-hook'")
        .await
        .unwrap();
    pool.execute("DELETE FROM system_principals WHERE id='organization'")
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM webhook_runs")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&pool)
            .await
            .unwrap()
            .is_empty()
    );
}
