// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use super::{DbError, Pool};
use jiff::Timestamp;
use sqlx::Row;
use std::collections::HashMap;

/// Tri-state per-tool permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolMode {
    /// Run without asking.
    Always,
    /// Ask the user first (chat UI); over /v1, governed by the token policy.
    Ask,
    /// Never expose this tool.
    Off,
}

impl ToolMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ToolMode::Always => "always",
            ToolMode::Ask => "ask",
            ToolMode::Off => "off",
        }
    }
    pub fn parse(s: &str) -> Option<ToolMode> {
        match s {
            "always" => Some(ToolMode::Always),
            "ask" => Some(ToolMode::Ask),
            "off" => Some(ToolMode::Off),
            _ => None,
        }
    }
}

/// How `ask`-mode tools behave over the /v1 API (which can't pause for an
/// interactive approval).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AskOverApi {
    /// Don't run; return a tool error telling the model approval is required.
    Block,
    /// Treat `ask` like `always`.
    Allow,
}

impl AskOverApi {
    pub fn as_str(self) -> &'static str {
        match self {
            AskOverApi::Block => "block",
            AskOverApi::Allow => "allow",
        }
    }
    pub fn parse(s: &str) -> AskOverApi {
        match s {
            "allow" => AskOverApi::Allow,
            _ => AskOverApi::Block,
        }
    }
}

// ---- per-tool permission modes --------------------------------------------

/// Explicit per-tool modes for a user+connector. Tools without a row fall back
/// to the connector default (resolved by the caller from tool annotations).
pub async fn tool_modes(
    pool: &Pool,
    user_id: &str,
    connector_key: &str,
) -> Result<HashMap<String, ToolMode>, DbError> {
    let rows = sqlx::query(
        "SELECT tool_name, mode FROM user_mcp_tool_prefs WHERE user_id = ? AND connector_key = ?",
    )
    .bind(user_id)
    .bind(connector_key)
    .fetch_all(pool)
    .await?;
    let mut out = HashMap::new();
    for r in &rows {
        let name: String = r.try_get("tool_name")?;
        let mode: String = r.try_get("mode")?;
        if let Some(m) = ToolMode::parse(&mode) {
            out.insert(name, m);
        }
    }
    Ok(out)
}

pub async fn set_tool_mode(
    pool: &Pool,
    user_id: &str,
    connector_key: &str,
    tool_name: &str,
    mode: ToolMode,
) -> Result<(), DbError> {
    sqlx::query(
        r#"INSERT INTO user_mcp_tool_prefs (user_id, connector_key, tool_name, mode, updated_at)
           VALUES (?, ?, ?, ?, ?)
           ON CONFLICT(user_id, connector_key, tool_name) DO UPDATE SET
               mode = excluded.mode, updated_at = excluded.updated_at"#,
    )
    .bind(user_id)
    .bind(connector_key)
    .bind(tool_name)
    .bind(mode.as_str())
    .bind(Timestamp::now().to_string())
    .execute(pool)
    .await?;
    Ok(())
}

// ---- per-token /v1 ask policy ---------------------------------------------

/// Resolve how `ask` tools behave over the /v1 API for `token_id` +
/// `connector_key`: the connector-specific row wins, else the `'*'` default,
/// else [`AskOverApi::Block`].
pub async fn token_ask_policy(
    pool: &Pool,
    token_id: &str,
    connector_key: &str,
) -> Result<AskOverApi, DbError> {
    let row = sqlx::query(
        "SELECT connector_key, ask_over_api FROM token_mcp_policy \
         WHERE token_id = ? AND connector_key IN (?, '*')",
    )
    .bind(token_id)
    .bind(connector_key)
    .fetch_all(pool)
    .await?;
    // Prefer the exact connector match over the wildcard.
    let mut wildcard: Option<AskOverApi> = None;
    let mut exact: Option<AskOverApi> = None;
    for r in &row {
        let key: String = r.try_get("connector_key")?;
        let val = AskOverApi::parse(&r.try_get::<String, _>("ask_over_api")?);
        if key == "*" {
            wildcard = Some(val);
        } else {
            exact = Some(val);
        }
    }
    Ok(exact.or(wildcard).unwrap_or(AskOverApi::Block))
}

pub async fn set_token_policy(
    pool: &Pool,
    token_id: &str,
    connector_key: &str,
    policy: AskOverApi,
) -> Result<(), DbError> {
    sqlx::query(
        r#"INSERT INTO token_mcp_policy (token_id, connector_key, ask_over_api, updated_at)
           VALUES (?, ?, ?, ?)
           ON CONFLICT(token_id, connector_key) DO UPDATE SET
               ask_over_api = excluded.ask_over_api, updated_at = excluded.updated_at"#,
    )
    .bind(token_id)
    .bind(connector_key)
    .bind(policy.as_str())
    .bind(Timestamp::now().to_string())
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::db;

    async fn pool() -> Pool {
        let p = db::open(std::path::Path::new(":memory:")).await.unwrap();
        // A user row for the FK.
        sqlx::query("INSERT INTO users (id, email, roles_json, created_at, updated_at) VALUES ('u1','u@e','[]','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')")
            .execute(&p)
            .await
            .unwrap();
        p
    }

    #[tokio::test]
    async fn tool_modes_roundtrip() {
        let pool = pool().await;
        assert!(tool_modes(&pool, "u1", "gmail").await.unwrap().is_empty());
        set_tool_mode(&pool, "u1", "gmail", "create_draft", ToolMode::Ask)
            .await
            .unwrap();
        set_tool_mode(&pool, "u1", "gmail", "search_threads", ToolMode::Always)
            .await
            .unwrap();
        // overwrite
        set_tool_mode(&pool, "u1", "gmail", "create_draft", ToolMode::Off)
            .await
            .unwrap();
        let modes = tool_modes(&pool, "u1", "gmail").await.unwrap();
        assert_eq!(modes.get("create_draft"), Some(&ToolMode::Off));
        assert_eq!(modes.get("search_threads"), Some(&ToolMode::Always));
    }

    #[tokio::test]
    async fn token_policy_exact_beats_wildcard_and_defaults_block() {
        let pool = pool().await;
        sqlx::query("INSERT INTO tokens (id, user_id, name, hash, created_at, expires_at) VALUES ('t1','u1','tok','h','2026-01-01T00:00:00Z','2030-01-01T00:00:00Z')")
            .execute(&pool)
            .await
            .unwrap();
        // Default with no rows = block.
        assert_eq!(
            token_ask_policy(&pool, "t1", "gmail").await.unwrap(),
            AskOverApi::Block
        );
        set_token_policy(&pool, "t1", "*", AskOverApi::Allow)
            .await
            .unwrap();
        assert_eq!(
            token_ask_policy(&pool, "t1", "gmail").await.unwrap(),
            AskOverApi::Allow
        );
        // Exact connector row overrides the wildcard.
        set_token_policy(&pool, "t1", "gmail", AskOverApi::Block)
            .await
            .unwrap();
        assert_eq!(
            token_ask_policy(&pool, "t1", "gmail").await.unwrap(),
            AskOverApi::Block
        );
    }
}
