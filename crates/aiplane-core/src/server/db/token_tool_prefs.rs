// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Per-token Off / Auto / On preferences for the shared capability picker.
//!
//! A layer on top of the per-user grant: a row can only ever expose a
//! tool the owning user's roles already grant. An enabled row means Always On,
//! a disabled row means Off, and value 2 means explicit Auto. Missing rows
//! default to Auto for built-in tools and Off for MCP connectors and skills. `tool_key`
//! is the UI toggle key (the per-template `typst_<id>` tools collapse to
//! a single `typst` key — see `server::tools::catalog`).
//!
//! These rows only matter when the token's master `tools_enabled` flag
//! is on; while it's off the request path skips tool injection entirely
//! (see `RamaState::allowed_tools_for_token`).
//!
//! Schema lives in `migrations/0019_token_tool_prefs.sql`; migration 0075
//! clears old implicit enabled rows so existing built-in tools default to Auto.

use std::collections::{HashMap, HashSet};

use jiff::Timestamp;
use sqlx::Row;

use super::{DbError, Pool};

/// Set the on/off state for one tool key on one token. Idempotent upsert
/// — re-saving the same state just bumps `updated_at`.
pub async fn set(
    pool: &Pool,
    token_id: &str,
    tool_key: &str,
    enabled: bool,
) -> Result<(), DbError> {
    let now = Timestamp::now().to_string();
    sqlx::query(
        r#"INSERT INTO token_tool_prefs (token_id, tool_key, enabled, updated_at)
           VALUES (?, ?, ?, ?)
           ON CONFLICT(token_id, tool_key) DO UPDATE SET
             enabled    = excluded.enabled,
             updated_at = excluded.updated_at"#,
    )
    .bind(token_id)
    .bind(tool_key)
    .bind(i64::from(enabled))
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

/// Remove a token override so the capability returns to automatic discovery.
pub async fn clear(pool: &Pool, token_id: &str, tool_key: &str) -> Result<(), DbError> {
    sqlx::query("DELETE FROM token_tool_prefs WHERE token_id = ? AND tool_key = ?")
        .bind(token_id)
        .bind(tool_key)
        .execute(pool)
        .await?;
    Ok(())
}

/// Explicit per-token states: 0 = Off, 1 = On, 2 = Auto.
pub async fn states_for_token(
    pool: &Pool,
    token_id: &str,
) -> Result<HashMap<String, i64>, DbError> {
    let rows = sqlx::query("SELECT tool_key, enabled FROM token_tool_prefs WHERE token_id = ?")
        .bind(token_id)
        .fetch_all(pool)
        .await?;
    rows.into_iter()
        .map(|row| {
            Ok((
                row.try_get::<String, _>("tool_key")?,
                row.try_get::<i64, _>("enabled")?,
            ))
        })
        .collect()
}

/// Replace every explicit override atomically. Missing keys use the family default.
pub async fn replace(
    pool: &Pool,
    token_id: &str,
    states: &HashMap<String, i64>,
) -> Result<(), DbError> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM token_tool_prefs WHERE token_id = ?")
        .bind(token_id)
        .execute(&mut *tx)
        .await?;
    let now = Timestamp::now().to_string();
    for (key, enabled) in states {
        sqlx::query(
            "INSERT INTO token_tool_prefs (token_id, tool_key, enabled, updated_at) VALUES (?, ?, ?, ?)",
        )
        .bind(token_id)
        .bind(key)
        .bind(enabled)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// The set of tool keys this token has explicitly turned **off**.
/// Everything not in this set is enabled (the default). Callers subtract
/// this from the user's granted tool list at request time.
pub async fn disabled_for_token(pool: &Pool, token_id: &str) -> Result<HashSet<String>, DbError> {
    let rows = sqlx::query(
        r#"SELECT tool_key FROM token_tool_prefs
           WHERE token_id = ? AND enabled = 0"#,
    )
    .bind(token_id)
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|r| r.try_get::<String, _>("tool_key").map_err(DbError::from))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::db::{open, tokens, users};
    use std::path::Path;

    /// A pool with a couple of real token rows — the `token_tool_prefs`
    /// FK requires a parent token, so prefs tests seed them first.
    async fn fresh() -> Pool {
        let pool = open(Path::new(":memory:")).await.unwrap();
        let now = jiff::Timestamp::now();
        users::upsert(
            &pool,
            &users::User {
                id: "alice".into(),
                email: "alice@example.com".into(),
                name: None,
                roles: vec![],
                created_at: now,
                updated_at: now,
                timezone: None,
                speech_voice: None,
            },
        )
        .await
        .unwrap();
        for id in ["tok-1", "tok-2"] {
            tokens::insert(
                &pool,
                &tokens::Token {
                    id: id.into(),
                    user_id: "alice".into(),
                    name: id.into(),
                    hash: format!("hash-{id}"),
                    created_at: now,
                    last_used_at: None,
                    expires_at: now + jiff::SignedDuration::from_hours(24),
                    revoked_at: None,
                    tools_enabled: true,
                },
            )
            .await
            .unwrap();
        }
        pool
    }

    #[tokio::test]
    async fn default_is_enabled_for_every_token() {
        let pool = fresh().await;
        // No rows → nothing disabled.
        assert!(disabled_for_token(&pool, "tok-1").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn disabling_then_reading_back() {
        let pool = fresh().await;
        set(&pool, "tok-1", "rag_search", false).await.unwrap();
        let disabled = disabled_for_token(&pool, "tok-1").await.unwrap();
        assert!(disabled.contains("rag_search"));
        assert_eq!(disabled.len(), 1);
    }

    #[tokio::test]
    async fn missing_row_is_auto_and_explicit_rows_are_on_or_off() {
        let pool = fresh().await;
        set(&pool, "tok-1", "search_web", true).await.unwrap();
        set(&pool, "tok-1", "mcp__discord", false).await.unwrap();
        let states = states_for_token(&pool, "tok-1").await.unwrap();
        assert_eq!(states.get("search_web"), Some(&1));
        assert_eq!(states.get("mcp__discord"), Some(&0));
        assert_eq!(states.get("read_skill"), None);
        clear(&pool, "tok-1", "search_web").await.unwrap();
        assert!(
            !states_for_token(&pool, "tok-1")
                .await
                .unwrap()
                .contains_key("search_web")
        );
    }

    #[tokio::test]
    async fn replace_persists_explicit_auto_and_removes_stale_states() {
        let pool = fresh().await;
        set(&pool, "tok-1", "search_web", true).await.unwrap();
        replace(
            &pool,
            "tok-1",
            &HashMap::from([("mcp__discord".into(), 2), ("skill:brand".into(), 0)]),
        )
        .await
        .unwrap();
        assert_eq!(
            states_for_token(&pool, "tok-1").await.unwrap(),
            HashMap::from([("mcp__discord".into(), 2), ("skill:brand".into(), 0)])
        );
    }

    #[tokio::test]
    async fn re_enabling_drops_it_from_disabled_set() {
        let pool = fresh().await;
        set(&pool, "tok-1", "rag_search", false).await.unwrap();
        set(&pool, "tok-1", "rag_search", true).await.unwrap();
        assert!(disabled_for_token(&pool, "tok-1").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn prefs_are_scoped_per_token() {
        let pool = fresh().await;
        set(&pool, "tok-1", "search_web", false).await.unwrap();
        assert!(disabled_for_token(&pool, "tok-2").await.unwrap().is_empty());
    }
}
