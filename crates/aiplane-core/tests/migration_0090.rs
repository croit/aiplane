// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use aiplane_core::server::db::open;
use sqlx::{Executor, Row, SqlitePool};
use std::path::Path;

const SCHEMA: &str = include_str!("fixtures/schema_after_data_classification.txt");

const USER_AND_SESSIONS: &str = "
    INSERT INTO users (id, email, roles_json, created_at, updated_at)
        VALUES ('alice', 'alice@example.test', '[]', '2026-01-01', '2026-01-01');
    INSERT INTO chat_sessions (id, user_id, created_at, updated_at) VALUES
        ('s-secret', 'alice', '2026-01-01', '2026-01-01'),
        ('s-plain', 'alice', '2026-01-01', '2026-01-01');
    INSERT INTO chat_session_settings (session_id, effort, confidential, updated_at) VALUES
        ('s-secret', 'high', 1, '2026-01-01'),
        ('s-plain', 'high', 0, '2026-01-01');";

async fn upgraded_from_0089(seed: &str) -> SqlitePool {
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
    for migration in migrations.iter().filter(|m| m.version < 90) {
        pool.execute(sqlx::raw_sql(&migration.sql)).await.unwrap();
    }
    pool.execute(sqlx::raw_sql(seed)).await.unwrap();
    pool.execute(sqlx::raw_sql(
        &migrations.iter().find(|m| m.version == 90).unwrap().sql,
    ))
    .await
    .unwrap();
    pool.execute("PRAGMA foreign_keys = ON").await.unwrap();
    pool
}

fn pool_row(name: &str, gdpr: u8, nda: u8) -> String {
    format!(
        "INSERT INTO pools (name, kind, compliance_gdpr, compliance_nda, created_at, updated_at)
         VALUES ('{name}', 'chat', {gdpr}, {nda}, '2026-01-01', '2026-01-01');"
    )
}

async fn levels(pool: &SqlitePool) -> Vec<(String, i64)> {
    sqlx::query("SELECT id, rank FROM classification_levels ORDER BY rank")
        .fetch_all(pool)
        .await
        .unwrap()
        .iter()
        .map(|r| (r.get("id"), r.get("rank")))
        .collect()
}

async fn setting(pool: &SqlitePool, key: &str) -> Option<String> {
    sqlx::query_scalar("SELECT value FROM app_settings WHERE key = ?")
        .bind(format!("settings.{key}"))
        .fetch_optional(pool)
        .await
        .unwrap()
}

async fn assignments(pool: &SqlitePool) -> Vec<(String, String)> {
    sqlx::query(
        "SELECT id, level_id FROM classification_assignments WHERE kind = 'pool' ORDER BY id",
    )
    .fetch_all(pool)
    .await
    .unwrap()
    .iter()
    .map(|r| (r.get("id"), r.get("level_id")))
    .collect()
}

#[tokio::test]
async fn an_untouched_installation_gets_the_single_public_level() {
    let pool = upgraded_from_0089(&format!(
        "{} {} {}",
        pool_row("a", 1, 1),
        pool_row("b", 1, 1),
        USER_AND_SESSIONS.replace("'s-secret', 'high', 1", "'s-secret', 'high', 0")
    ))
    .await;
    let seeded = [("public".to_owned(), 0)];
    assert_eq!(levels(&pool).await, seeded);
    let classes: i64 = sqlx::query_scalar("SELECT count(*) FROM classification_data_classes")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(classes, 0);
    assert!(assignments(&pool).await.is_empty());
    let level: Option<String> = sqlx::query_scalar(
        "SELECT level_id FROM chat_session_settings WHERE session_id = 's-secret'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(level, None);
}

/// Mixed flags, an enabled guard and a confidential conversation become the
/// ISO-typical scheme; nothing the old mechanism stored is lost or kept.
#[tokio::test]
async fn an_installation_that_used_the_old_mechanism_is_mapped_into_the_iso_scheme() {
    let pool = upgraded_from_0089(&format!(
        "{} {} {} {}
         {USER_AND_SESSIONS}
         INSERT INTO app_settings (key, value, updated_at) VALUES
            ('settings.content_guard.enabled', 'true', '2026-01-01'),
            ('settings.content_guard.model', 'guard-1', '2026-01-01'),
            ('settings.content_guard.mode', 'enforce', '2026-01-01'),
            ('settings.content_guard.gdpr_action', 'confirm', '2026-01-01'),
            ('settings.content_guard.nda_action', 'allow', '2026-01-01'),
            ('settings.chat.confidential.default', 'true', '2026-01-01'),
            ('settings.chat.confidential.requires', 'both', '2026-01-01'),
            ('settings.chat.prompt.enabled', 'true', '2026-01-01');",
        pool_row("cleared", 1, 1),
        pool_row("eu-only", 1, 0),
        pool_row("cloud", 0, 1),
        pool_row("nothing", 0, 0),
    ))
    .await;

    assert_eq!(
        levels(&pool).await,
        [
            ("public".to_owned(), 0),
            ("internal".to_owned(), 1),
            ("confidential".to_owned(), 2),
            ("strictly_confidential".to_owned(), 3)
        ]
    );
    assert_eq!(
        assignments(&pool).await,
        [
            ("cleared".to_owned(), "strictly_confidential".to_owned()),
            ("cloud".to_owned(), "public".to_owned()),
            ("eu-only".to_owned(), "internal".to_owned()),
            ("nothing".to_owned(), "public".to_owned()),
        ]
    );

    let classes = sqlx::query(
        "SELECT id, level_id, rule_question, action FROM classification_data_classes ORDER BY id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    let got: Vec<(String, String, bool, String)> = classes
        .iter()
        .map(|r| {
            (
                r.get("id"),
                r.get("level_id"),
                !r.get::<String, _>("rule_question").is_empty(),
                r.get("action"),
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            (
                "nda".to_owned(),
                "confidential".to_owned(),
                false,
                "raise".to_owned()
            ),
            (
                "personal_data".to_owned(),
                "internal".to_owned(),
                true,
                "confirm".to_owned()
            ),
        ]
    );

    let secret: Option<String> = sqlx::query_scalar(
        "SELECT level_id FROM chat_session_settings WHERE session_id = 's-secret'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(secret.as_deref(), Some("confidential"));
    let plain: Option<String> = sqlx::query_scalar(
        "SELECT level_id FROM chat_session_settings WHERE session_id = 's-plain'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(plain, None);
    let events =
        sqlx::query("SELECT session_id, to_level, cause, actor FROM chat_classification_events")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].get::<String, _>("session_id"), "s-secret");
    assert_eq!(events[0].get::<String, _>("cause"), "migration");

    assert_eq!(
        setting(&pool, "classification.detector_model")
            .await
            .as_deref(),
        Some("guard-1")
    );
    assert_eq!(
        setting(&pool, "classification.mode").await.as_deref(),
        Some("enforce")
    );
    assert_eq!(
        setting(&pool, "classification.default_level")
            .await
            .as_deref(),
        Some("confidential")
    );
    assert_eq!(
        setting(&pool, "classification.link_share_max_level")
            .await
            .as_deref(),
        Some("confidential"),
        "links that worked before keep working: nothing was above confidential"
    );
    let leftover: Vec<String> = sqlx::query_scalar(
        "SELECT key FROM app_settings WHERE key LIKE 'settings.content_guard.%' OR key LIKE 'settings.chat.confidential.%'",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(leftover.is_empty(), "{leftover:?}");
    assert_eq!(
        setting(&pool, "chat.prompt.enabled").await.as_deref(),
        Some("true")
    );

    let violations = sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert!(violations.is_empty());
}

#[tokio::test]
async fn a_pool_that_met_the_confidential_requirement_becomes_confidential() {
    for (requires, expected) in [
        (
            Some("nda"),
            [
                ("both", "strictly_confidential"),
                ("gdpr-only", "internal"),
                ("nda-only", "confidential"),
                ("none", "public"),
            ],
        ),
        (
            Some("gdpr"),
            [
                ("both", "strictly_confidential"),
                ("gdpr-only", "confidential"),
                ("nda-only", "public"),
                ("none", "public"),
            ],
        ),
        (
            None,
            [
                ("both", "strictly_confidential"),
                ("gdpr-only", "internal"),
                ("nda-only", "public"),
                ("none", "public"),
            ],
        ),
    ] {
        let requires_row = requires.map_or_else(String::new, |value| {
            format!(
                "INSERT INTO app_settings (key, value, updated_at)
                 VALUES ('settings.chat.confidential.requires', '{value}', '2026-01-01');"
            )
        });
        let pool = upgraded_from_0089(&format!(
            "{} {} {} {} {requires_row}
             INSERT INTO app_settings (key, value, updated_at)
             VALUES ('settings.content_guard.enabled', 'true', '2026-01-01');",
            pool_row("both", 1, 1),
            pool_row("gdpr-only", 1, 0),
            pool_row("nda-only", 0, 1),
            pool_row("none", 0, 0),
        ))
        .await;
        let got = assignments(&pool).await;
        let mut expected: Vec<(String, String)> = expected
            .iter()
            .map(|(name, level)| ((*name).to_owned(), (*level).to_owned()))
            .collect();
        expected.sort();
        assert_eq!(got, expected, "requires = {requires:?}");
    }
}

#[tokio::test]
async fn an_untouched_installation_keeps_no_link_ceiling() {
    let pool = upgraded_from_0089("").await;
    assert_eq!(
        setting(&pool, "classification.link_share_max_level").await,
        None
    );
}

#[tokio::test]
async fn a_guard_that_was_off_leaves_the_classes_without_a_detector() {
    let pool = upgraded_from_0089(&format!(
        "{} INSERT INTO app_settings (key, value, updated_at) VALUES
            ('settings.content_guard.enabled', 'false', '2026-01-01');",
        pool_row("eu-only", 1, 0)
    ))
    .await;
    let detecting: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM classification_data_classes WHERE rule_question <> ''",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(detecting, 0);
    assert_eq!(setting(&pool, "classification.mode").await, None);
}

#[tokio::test]
async fn an_enabled_guard_keeps_its_default_monitor_mode() {
    let pool = upgraded_from_0089(
        "INSERT INTO app_settings (key, value, updated_at) VALUES
            ('settings.content_guard.enabled', 'true', '2026-01-01');",
    )
    .await;
    assert_eq!(
        setting(&pool, "classification.mode").await.as_deref(),
        Some("monitor")
    );
    let blocking: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM classification_data_classes WHERE rule_question <> '' AND action = 'block'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(blocking, 2);
}

#[tokio::test]
async fn a_pool_assignment_follows_the_pool_through_rename_and_delete() {
    let pool = upgraded_from_0089(&pool_row("eu-only", 1, 0)).await;
    assert_eq!(
        assignments(&pool).await,
        [("eu-only".to_owned(), "internal".to_owned())]
    );
    pool.execute("UPDATE pools SET name = 'eu' WHERE name = 'eu-only'")
        .await
        .unwrap();
    assert_eq!(
        assignments(&pool).await,
        [("eu".to_owned(), "internal".to_owned())]
    );
    pool.execute("DELETE FROM pools WHERE name = 'eu'")
        .await
        .unwrap();
    assert!(assignments(&pool).await.is_empty());
}

#[tokio::test]
async fn data_classification_schema_matches_its_fixture() {
    let pool = open(Path::new(":memory:")).await.unwrap();
    let rows = sqlx::query(
        "SELECT type, name, sql FROM sqlite_schema
         WHERE (name LIKE 'classification_%' OR name LIKE 'chat_classification_%'
                OR name LIKE 'idx_chat_classification_%' OR name = 'chat_session_settings'
                OR name = 'chat_session_shares' OR name = 'idx_chat_session_shares_subject'
                OR name = 'pools')
           AND sql IS NOT NULL
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
                .join("tests/fixtures/schema_after_data_classification.txt"),
            &actual,
        )
        .unwrap();
    } else {
        assert_eq!(actual, SCHEMA);
    }
}

#[tokio::test]
async fn a_conversations_shares_go_with_it_and_name_a_user_or_a_group() {
    let pool = open(Path::new(":memory:")).await.unwrap();
    pool.execute(sqlx::raw_sql(
        "INSERT INTO users (id, email, roles_json, created_at, updated_at)
             VALUES ('alice', 'alice@example.test', '[]', '2026-01-01', '2026-01-01');
         INSERT INTO chat_sessions (id, user_id, created_at, updated_at)
             VALUES ('s1', 'alice', '2026-01-01', '2026-01-01');
         INSERT INTO chat_session_shares (session_id, subject_kind, subject_id)
             VALUES ('s1', 'user', 'bob'), ('s1', 'group', 'engineering');",
    ))
    .await
    .unwrap();

    let other_kind = sqlx::query(
        "INSERT INTO chat_session_shares (session_id, subject_kind, subject_id)
         VALUES ('s1', 'team', 'x')",
    )
    .execute(&pool)
    .await;
    assert!(
        other_kind.is_err(),
        "only a user or a group can be shared with"
    );

    sqlx::query("DELETE FROM chat_sessions WHERE id = 's1'")
        .execute(&pool)
        .await
        .unwrap();
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM chat_session_shares")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(left, 0);
}
