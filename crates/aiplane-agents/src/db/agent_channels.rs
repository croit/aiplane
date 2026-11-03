// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! `agent_notify_channels`: the Slack and Discord incoming webhooks an agent's
//! waiting turns are announced on (`docs/agent-hil.md`).
//!
//! An incoming-webhook URL is a credential — whoever has it can post into the
//! channel — so it is sealed at rest and never read back out through the API.
//! Only its host is kept in clear, so the builder can say where a channel
//! posts. Creating and deleting a channel records an [`agent_audit`] row in
//! the same transaction, without the URL.

use jiff::Timestamp;
use serde_json::json;
use sqlx::Row;
use sqlx::sqlite::SqliteRow;
use uuid::Uuid;

use super::agent_audit::{self, AuditKind};
use super::{DbError, Pool, WriteTx};
use aiplane_core::server::crypto::Crypto;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelKind {
    Slack,
    Discord,
}

impl ChannelKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Slack => "slack",
            Self::Discord => "discord",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "slack" => Some(Self::Slack),
            "discord" => Some(Self::Discord),
            _ => None,
        }
    }
}

/// A channel as the builder shows it: never the URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Channel {
    pub id: String,
    pub kind: ChannelKind,
    pub name: String,
    pub url_host: String,
    /// Put the question or the tool name into the message, not only the
    /// agent, the kind and the inbox link.
    pub details: bool,
    /// The catalog language of the message.
    pub lang: String,
    pub created_by: String,
    pub created_at: Timestamp,
}

/// A channel with its opened URL, for sending.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub channel: Channel,
    pub url: String,
}

pub struct NewChannel<'a> {
    pub kind: ChannelKind,
    pub name: &'a str,
    pub url: &'a str,
    pub url_host: &'a str,
    pub details: bool,
    pub lang: &'a str,
}

const COLS: &str = "id, kind, name, url_host, details, lang, created_by, created_at";

fn map_channel(row: &SqliteRow) -> Result<Channel, DbError> {
    let kind: String = row.try_get("kind")?;
    Ok(Channel {
        id: row.try_get("id")?,
        kind: ChannelKind::parse(&kind).ok_or_else(|| DbError::Decode {
            column: "kind",
            source: anyhow::anyhow!("unknown channel kind `{kind}`"),
        })?,
        name: row.try_get("name")?,
        url_host: row.try_get("url_host")?,
        details: row.try_get::<i64, _>("details")? != 0,
        lang: row.try_get("lang")?,
        created_by: row.try_get("created_by")?,
        created_at: super::parse_ts(row.try_get("created_at")?, "created_at")?,
    })
}

/// Store a channel with its URL sealed. `Ok(None)` when the agent already
/// has a channel of that name.
pub async fn create(
    pool: &Pool,
    crypto: &Crypto,
    agent_id: &str,
    new: &NewChannel<'_>,
    actor_id: &str,
) -> Result<Option<Channel>, DbError> {
    let sealed = crypto.seal_str(new.url).map_err(|e| DbError::Decode {
        column: "url_ct",
        source: anyhow::anyhow!("sealing the webhook URL: {e}"),
    })?;
    let id = Uuid::new_v4().to_string();
    let mut tx = WriteTx::begin(pool).await?;
    let inserted = sqlx::query(
        "INSERT INTO agent_notify_channels
            (id, principal_id, kind, name, url_nonce, url_ct, url_host, details, lang,
             created_by, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(principal_id, name) DO NOTHING",
    )
    .bind(&id)
    .bind(agent_id)
    .bind(new.kind.as_str())
    .bind(new.name)
    .bind(&sealed.nonce)
    .bind(&sealed.ciphertext)
    .bind(new.url_host)
    .bind(i64::from(new.details))
    .bind(new.lang)
    .bind(actor_id)
    .bind(Timestamp::now().to_string())
    .execute(&mut *tx)
    .await?
    .rows_affected()
        > 0;
    if !inserted {
        return Ok(None);
    }
    agent_audit::append(
        &mut tx,
        agent_audit::NewEvent::new(
            AuditKind::ChannelCreated,
            agent_id,
            json!({ "channel_id": id, "kind": new.kind.as_str(), "name": new.name,
                "url_host": new.url_host, "details": new.details }),
        )
        .by(Some(actor_id)),
    )
    .await?;
    tx.commit().await?;
    let row = sqlx::query(&format!(
        "SELECT {COLS} FROM agent_notify_channels WHERE id = ?"
    ))
    .bind(&id)
    .fetch_one(pool)
    .await?;
    map_channel(&row).map(Some)
}

pub async fn list(pool: &Pool, agent_id: &str) -> Result<Vec<Channel>, DbError> {
    let rows = sqlx::query(&format!(
        "SELECT {COLS} FROM agent_notify_channels WHERE principal_id = ? ORDER BY name"
    ))
    .bind(agent_id)
    .fetch_all(pool)
    .await?;
    rows.iter().map(map_channel).collect()
}

/// Every channel of the agent with its URL opened. A URL that no longer
/// opens (a changed at-rest key) is skipped with a warning: the other
/// channels still get the message.
pub async fn targets(pool: &Pool, crypto: &Crypto, agent_id: &str) -> Result<Vec<Target>, DbError> {
    let rows = sqlx::query(&format!(
        "SELECT {COLS}, url_nonce, url_ct FROM agent_notify_channels
          WHERE principal_id = ? ORDER BY name"
    ))
    .bind(agent_id)
    .fetch_all(pool)
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let channel = map_channel(row)?;
        let nonce: Vec<u8> = row.try_get("url_nonce")?;
        let ct: Vec<u8> = row.try_get("url_ct")?;
        match crypto.open_str(&nonce, &ct) {
            Ok(url) => out.push(Target { channel, url }),
            Err(err) => tracing::warn!(
                channel = %channel.id, error = %err,
                "a notification channel's webhook URL does not open under the at-rest key; \
                 re-create the channel"
            ),
        }
    }
    Ok(out)
}

/// Delete a channel of the agent. `Ok(false)` when it has none by that id.
pub async fn delete(
    pool: &Pool,
    agent_id: &str,
    channel_id: &str,
    actor_id: &str,
) -> Result<bool, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let removed =
        sqlx::query("DELETE FROM agent_notify_channels WHERE id = ? AND principal_id = ?")
            .bind(channel_id)
            .bind(agent_id)
            .execute(&mut *tx)
            .await?
            .rows_affected()
            > 0;
    if removed {
        agent_audit::append(
            &mut tx,
            agent_audit::NewEvent::new(
                AuditKind::ChannelDeleted,
                agent_id,
                json!({ "channel_id": channel_id }),
            )
            .by(Some(actor_id)),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{agents, system_principals as sp};
    use std::path::Path;

    const URL: &str = "https://hooks.slack.com/services/T000/B000/secret-part";

    async fn pool_with_agent() -> (Pool, String) {
        let pool = aiplane_core::server::db::open(Path::new(":memory:"))
            .await
            .unwrap();
        let id = agents::create(
            &pool,
            &sp::NewPrincipal {
                name: "support",
                display: "Support",
                description: "",
            },
            "{}",
            "alice",
        )
        .await
        .unwrap()
        .unwrap()
        .principal
        .id;
        (pool, id)
    }

    fn new(name: &str) -> NewChannel<'_> {
        NewChannel {
            kind: ChannelKind::Slack,
            name,
            url: URL,
            url_host: "hooks.slack.com",
            details: false,
            lang: "en",
        }
    }

    #[tokio::test]
    async fn the_url_is_sealed_at_rest_and_opens_for_sending_only() {
        let (pool, agent) = pool_with_agent().await;
        // codeql[rust/hard-coded-cryptographic-value] — deterministic test key, not a credential.
        let crypto = Crypto::from_session(&[3u8; 32]);
        let channel = create(&pool, &crypto, &agent, &new("ops"), "alice")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(channel.url_host, "hooks.slack.com");

        let stored: Vec<u8> = sqlx::query_scalar("SELECT url_ct FROM agent_notify_channels")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(!stored.windows(11).any(|w| w == b"secret-part"));
        let audit = agent_audit::for_principal(&pool, &agent).await.unwrap();
        assert!(
            audit
                .iter()
                .all(|e| !e.detail.to_string().contains("secret-part"))
        );

        let targets = targets(&pool, &crypto, &agent).await.unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].url, URL);
    }

    #[tokio::test]
    async fn a_name_is_unique_per_agent_and_delete_is_scoped_to_it() {
        let (pool, agent) = pool_with_agent().await;
        // codeql[rust/hard-coded-cryptographic-value] — deterministic test key, not a credential.
        let crypto = Crypto::from_session(&[3u8; 32]);
        let first = create(&pool, &crypto, &agent, &new("ops"), "alice")
            .await
            .unwrap()
            .unwrap();
        assert!(
            create(&pool, &crypto, &agent, &new("ops"), "alice")
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            !delete(&pool, "another-agent", &first.id, "alice")
                .await
                .unwrap()
        );
        assert!(delete(&pool, &agent, &first.id, "alice").await.unwrap());
        assert!(list(&pool, &agent).await.unwrap().is_empty());
    }
}
