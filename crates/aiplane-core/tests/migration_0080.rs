// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use aiplane_core::server::db::open;
use sqlx::{Executor, Row};
use std::path::Path;

#[tokio::test]
async fn organization_dot_ownership_is_exclusive_immutable_and_independent_of_creator() {
    let pool = open(Path::new(":memory:")).await.unwrap();
    pool.execute("INSERT INTO users(id,email,roles_json,created_at,updated_at) VALUES('creator','creator@example.test','[]','2026-01-01','2026-01-01')").await.unwrap();
    for id in ["acme", "other"] {
        sqlx::query("INSERT INTO organizations(id,name,description,revision,created_by,created_at,updated_at) VALUES(?,?,'','initial','creator','2026-01-01','2026-01-01')")
            .bind(id).bind(id).execute(&pool).await.unwrap();
    }
    for id in ["personal", "organization", "invalid"] {
        sqlx::query("INSERT INTO system_principals(id,name,display,created_by,created_at) VALUES(?,?,?,'creator','2026-01-01')")
            .bind(id).bind(id).bind(id).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO agents(principal_id,draft_spec,created_at,updated_at) VALUES(?,'{}','2026-01-01','2026-01-01')")
            .bind(id).execute(&pool).await.unwrap();
    }
    pool.execute("INSERT INTO dots(principal_id,owner_user_id,organization_id,name,created_at,updated_at) VALUES('personal','creator',NULL,'Harald','2026-01-01','2026-01-01'),('organization',NULL,'acme','Harald','2026-01-01','2026-01-01')").await.unwrap();
    for (user, organization) in [(None, None), (Some("creator"), Some("acme"))] {
        assert!(sqlx::query("INSERT INTO dots(principal_id,owner_user_id,organization_id,name,created_at,updated_at) VALUES('invalid',?,?,'Invalid','2026-01-01','2026-01-01')")
            .bind(user).bind(organization).execute(&pool).await.is_err());
    }
    assert!(
        pool.execute("UPDATE dots SET principal_id='invalid' WHERE principal_id='organization'")
            .await
            .is_err()
    );
    assert!(
        pool.execute("UPDATE dots SET organization_id='other' WHERE principal_id='organization'")
            .await
            .is_err()
    );
    assert!(pool.execute("UPDATE dots SET owner_user_id=NULL,organization_id='acme' WHERE principal_id='personal'").await.is_err());
    pool.execute("DELETE FROM users WHERE id='creator'")
        .await
        .unwrap();
    for table in ["dots", "agents", "system_principals"] {
        let key = if table == "system_principals" {
            "id"
        } else {
            "principal_id"
        };
        let ids: Vec<String> =
            sqlx::query_scalar(&format!("SELECT {key} FROM {table} ORDER BY {key}"))
                .fetch_all(&pool)
                .await
                .unwrap();
        assert!(!ids.contains(&"personal".to_owned()), "{table}");
        assert!(ids.contains(&"organization".to_owned()), "{table}");
    }
    let organizations: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM organizations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(organizations, 2);
    assert!(
        pool.execute("DELETE FROM organizations WHERE id='acme'")
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
}

#[tokio::test]
async fn dot_memory_scopes_are_exclusive_and_reject_missing_owners() {
    let pool = open(Path::new(":memory:")).await.unwrap();
    pool.execute("INSERT INTO organizations(id,name,description,revision,created_by,created_at,updated_at) VALUES('acme','Acme','','initial','creator','2026-01-01','2026-01-01')").await.unwrap();
    pool.execute("INSERT INTO system_principals(id,name,display,created_by,created_at) VALUES('dot','dot','Dot','creator','2026-01-01')").await.unwrap();
    pool.execute("INSERT INTO agents(principal_id,draft_spec,created_at,updated_at) VALUES('dot','{}','2026-01-01','2026-01-01')").await.unwrap();
    pool.execute("INSERT INTO dots(principal_id,owner_user_id,organization_id,name,created_at,updated_at) VALUES('dot',NULL,'acme','Dot','2026-01-01','2026-01-01')").await.unwrap();

    for (dot, organization) in [
        (None, None),
        (Some("dot"), Some("acme")),
        (Some("missing"), None),
        (None, Some("missing")),
    ] {
        assert!(sqlx::query("INSERT INTO dot_memories(id,dot_id,organization_id,kind,content,created_at,updated_at) VALUES('invalid',?,?,'fact','Note','2026-01-01','2026-01-01')")
            .bind(dot).bind(organization).execute(&pool).await.is_err());
    }
    for (id, dot, organization) in [("own", Some("dot"), None), ("shared", None, Some("acme"))] {
        sqlx::query("INSERT INTO dot_memories(id,dot_id,organization_id,kind,content,created_at,updated_at) VALUES(?,?,?,'fact','Same content','2026-01-01','2026-01-01')")
            .bind(id).bind(dot).bind(organization).execute(&pool).await.unwrap();
    }
    assert!(
        pool.execute("UPDATE dot_memories SET organization_id='acme' WHERE id='own'")
            .await
            .is_err()
    );
    assert!(
        pool.execute("UPDATE dot_memories SET organization_id=NULL WHERE id='shared'")
            .await
            .is_err()
    );
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM dot_memories")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 2);
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&pool)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn organization_dot_schema_matches_its_fixture() {
    let pool = open(Path::new(":memory:")).await.unwrap();
    let rows = sqlx::query(
        "SELECT type, name, sql FROM sqlite_schema
         WHERE name IN ('organizations', 'organization_groups', 'dots',
             'idx_organization_groups_group', 'idx_dots_user_name',
             'idx_dots_organization_name', 'idx_dots_user_active',
             'idx_dots_organization_active', 'dots_reject_owner_update',
             'organization_groups_rotate_revision_after_insert',
             'organization_groups_rotate_revision_after_delete',
             'organization_groups_reject_update',
             'personal_dots_delete_principals_before_owner_delete',
             'dot_memories', 'idx_dot_memories_dot_content',
             'idx_dot_memories_organization_content', 'idx_dot_memories_dot_created',
             'idx_dot_memories_organization_created', 'dot_workspace_cleanup',
             'idx_dot_workspace_cleanup_due', 'personal_dot_workspace_cleanup_after_delete')
         ORDER BY type, name",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 21);
    let actual = rows
        .iter()
        .map(|row| {
            let kind: String = row.get("type");
            let name: String = row.get("name");
            let sql: String = row.get("sql");
            format!(
                "{kind} {name}: {}\n",
                sql.split_whitespace().collect::<Vec<_>>().join(" ")
            )
        })
        .collect::<String>();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/schema_after_dot_organizations.txt");
    if std::env::var_os("UPDATE_SCHEMA_FIXTURE").is_some() {
        std::fs::write(fixture, actual).unwrap();
    } else {
        assert_eq!(
            actual,
            include_str!("fixtures/schema_after_dot_organizations.txt")
        );
    }
}

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

#[tokio::test]
async fn dot_delegation_schema_matches_its_fixture() {
    let pool = open(Path::new(":memory:")).await.unwrap();
    let rows = sqlx::query(
        "SELECT type, name, sql FROM sqlite_schema
         WHERE name IN ('dot_delegation_edges', 'idx_dot_delegation_edges_receiver')
         ORDER BY type, name",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 2);
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
                .join("tests/fixtures/schema_after_dot_delegation.txt"),
            &actual,
        )
        .unwrap();
    } else {
        assert_eq!(
            actual,
            include_str!("fixtures/schema_after_dot_delegation.txt")
        );
    }
}
