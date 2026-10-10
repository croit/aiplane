// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Executor, Row};
use std::str::FromStr;

#[tokio::test]
async fn existing_groups_tokens_and_backends_inherit_priority_without_data_loss() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::from_str("sqlite::memory:")
                .unwrap()
                .foreign_keys(false),
        )
        .await
        .unwrap();
    for migration in sqlx::migrate!("./migrations")
        .iter()
        .filter(|m| m.version < 80)
    {
        pool.execute(sqlx::raw_sql(&migration.sql)).await.unwrap();
    }
    pool.execute(sqlx::raw_sql(r#"
        INSERT INTO users(id,email,created_at,updated_at) VALUES ('u','u@example.com','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');
        INSERT INTO tokens(id,user_id,name,hash,created_at,expires_at) VALUES ('erp','u','ERP','synthetic-hash','2026-01-01T00:00:00Z','2027-01-01T00:00:00Z');
        INSERT INTO gateway_groups(name,description,is_admin,is_default,created_at,updated_at) VALUES ('staff','Staff',0,1,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');
        INSERT INTO backends(name,base_url,created_at,updated_at) VALUES ('gpu','http://unused/v1','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');
    "#)).await.unwrap();
    let migration = sqlx::migrate!("./migrations")
        .iter()
        .find(|m| m.version == 80)
        .unwrap()
        .clone();
    pool.execute(sqlx::raw_sql(&migration.sql)).await.unwrap();
    for (table, identity) in [("tokens", "name"), ("gateway_groups", "description")] {
        let row = sqlx::query(&format!("SELECT {identity}, priority FROM {table}"))
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(row.get::<Option<String>, _>("priority").is_none());
        assert!(!row.get::<String, _>(identity).is_empty());
        assert!(
            pool.execute(sqlx::raw_sql(&format!(
                "UPDATE {table} SET priority = 'urgent'"
            )))
            .await
            .is_err()
        );
    }
    let row = sqlx::query("SELECT base_url, priority FROM backends")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row.get::<String, _>("base_url"), "http://unused/v1");
    assert_eq!(row.get::<String, _>("priority"), r#"{"mode":"auto"}"#);
}
