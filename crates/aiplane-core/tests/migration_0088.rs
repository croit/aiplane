// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use aiplane_core::server::db::open;
use sqlx::{Executor, Row};
use std::path::Path;

const SCHEMA: &str = include_str!("fixtures/schema_after_agent_share_origin.txt");

/// Existing shares learn who gave them from the activity log: the latest
/// `agent_share_set` for the same subject, else the agent's creation for the
/// creator's own share. A share the log does not explain stays unnamed
/// rather than being credited to someone who did not give it.
#[tokio::test]
async fn upgrade_takes_each_share_origin_from_the_activity_log() {
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
    for migration in migrations.iter().filter(|migration| migration.version < 88) {
        pool.execute(sqlx::raw_sql(&migration.sql)).await.unwrap();
    }
    pool.execute(sqlx::raw_sql(
        "INSERT INTO agent_shares (principal_id, subject_kind, subject_id, access) VALUES
             ('a1', 'user', 'alice', 'write'),
             ('a1', 'user', 'bob', 'read'),
             ('a1', 'group', 'support', 'respond'),
             ('a1', 'user', 'carol', 'read');
         INSERT INTO agent_audit (id, kind, principal_id, actor_id, detail, created_at) VALUES
             ('e1', 'agent_created', 'a1', 'alice', '{\"name\":\"support\"}', '2026-09-01T08:00:00Z'),
             ('e2', 'agent_share_set', 'a1', 'alice', '{\"subject_kind\":\"user\",\"subject_id\":\"bob\",\"access\":\"write\"}', '2026-09-02T08:00:00Z'),
             ('e3', 'agent_share_set', 'a1', 'dave', '{\"subject_kind\":\"user\",\"subject_id\":\"bob\",\"access\":\"read\"}', '2026-09-03T08:00:00Z'),
             ('e4', 'agent_share_set', 'a1', 'alice', '{\"subject_kind\":\"group\",\"subject_id\":\"support\",\"access\":\"respond\"}', '2026-09-04T08:00:00Z'),
             ('e5', 'agent_share_set', 'a2', 'alice', '{\"subject_kind\":\"user\",\"subject_id\":\"carol\",\"access\":\"read\"}', '2026-09-05T08:00:00Z');",
    ))
    .await
    .unwrap();
    pool.execute(sqlx::raw_sql(
        &migrations.iter().find(|m| m.version == 88).unwrap().sql,
    ))
    .await
    .unwrap();

    let rows = sqlx::query(
        "SELECT subject_id, access, granted_by, granted_at FROM agent_shares ORDER BY subject_id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    let got = rows
        .iter()
        .map(|row| {
            (
                row.get::<String, _>("subject_id"),
                row.get::<String, _>("access"),
                row.get::<Option<String>, _>("granted_by"),
                row.get::<Option<String>, _>("granted_at"),
            )
        })
        .collect::<Vec<_>>();
    let some = |s: &str| Some(s.to_string());
    assert_eq!(
        got,
        [
            (
                "alice".into(),
                "write".into(),
                some("alice"),
                some("2026-09-01T08:00:00Z")
            ),
            (
                "bob".into(),
                "read".into(),
                some("dave"),
                some("2026-09-03T08:00:00Z")
            ),
            ("carol".into(), "read".into(), None, None),
            (
                "support".into(),
                "respond".into(),
                some("alice"),
                some("2026-09-04T08:00:00Z")
            ),
        ]
    );
}

#[tokio::test]
async fn agent_shares_schema_matches_its_fixture() {
    let pool = open(Path::new(":memory:")).await.unwrap();
    let rows = sqlx::query(
        "SELECT type, name, sql FROM sqlite_schema
         WHERE tbl_name = 'agent_shares' AND sql IS NOT NULL
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
                .join("tests/fixtures/schema_after_agent_share_origin.txt"),
            &actual,
        )
        .unwrap();
    } else {
        assert_eq!(actual, SCHEMA);
    }
}
