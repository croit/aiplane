// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! `visitor_sessions`: one anonymous visitor's conversation with an agent
//! (`docs/agent-visitors.md`).
//!
//! A visitor is not a principal. The conversation is a `chat_sessions` row
//! owned by the agent's principal, linked back through
//! `chat_sessions.visitor_id`; the visitor holds only a token (`gwv_…`) whose
//! SHA-256 is stored here. Every request slides `expires_at` by the session's
//! idle TTL, but never past `max_expires_at`, so a visitor who keeps a tab
//! open forever still starts over eventually.
//!
//! Every function takes `now` so expiry is testable without waiting.

use crate::db::run_sessions;
use jiff::{SignedDuration, Timestamp};
use sqlx::Row;
use sqlx::sqlite::SqliteRow;
use uuid::Uuid;

use super::{DbError, Pool, WriteTx};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisitorSession {
    pub id: String,
    pub principal_id: String,
    pub embed_key_id: String,
    /// The `chat_sessions` row of this conversation.
    pub session_id: String,
    pub client_ip: Option<String>,
    pub idle_ttl: SignedDuration,
    pub created_at: Timestamp,
    pub last_seen_at: Timestamp,
    pub expires_at: Timestamp,
    pub max_expires_at: Timestamp,
}

pub struct NewVisitorSession<'a> {
    pub principal_id: &'a str,
    pub embed_key_id: &'a str,
    /// The live version the conversation starts on.
    pub agent_version: i64,
    pub token_hash: &'a str,
    pub client_ip: Option<&'a str>,
    pub idle_ttl: SignedDuration,
    /// The absolute cap: no request slides the session past `now + max_age`.
    pub max_age: SignedDuration,
    pub now: Timestamp,
}

/// What a visitor token resolves to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup {
    /// Live; its expiry has been slid by this lookup.
    Active(VisitorSession),
    /// Known, but idle too long or past its absolute cap.
    Expired,
    Unknown,
}

const COLS: &str = "id, principal_id, embed_key_id, session_id, client_ip, idle_ttl_secs, \
                    created_at, last_seen_at, expires_at, max_expires_at";

fn map_session(row: &SqliteRow) -> Result<VisitorSession, DbError> {
    Ok(VisitorSession {
        id: row.try_get("id")?,
        principal_id: row.try_get("principal_id")?,
        embed_key_id: row.try_get("embed_key_id")?,
        session_id: row.try_get("session_id")?,
        client_ip: row.try_get("client_ip")?,
        idle_ttl: SignedDuration::from_secs(row.try_get("idle_ttl_secs")?),
        created_at: super::parse_ts(row.try_get("created_at")?, "created_at")?,
        last_seen_at: super::parse_ts(row.try_get("last_seen_at")?, "last_seen_at")?,
        expires_at: super::parse_ts(row.try_get("expires_at")?, "expires_at")?,
        max_expires_at: super::parse_ts(row.try_get("max_expires_at")?, "max_expires_at")?,
    })
}

fn add(t: Timestamp, d: SignedDuration) -> Result<Timestamp, DbError> {
    t.checked_add(d).map_err(|e| DbError::Decode {
        column: "expires_at",
        source: e.into(),
    })
}

/// Open the visitor's conversation under the agent's principal and the
/// session that holds it, linked both ways, in one transaction.
pub async fn start(pool: &Pool, new: &NewVisitorSession<'_>) -> Result<VisitorSession, DbError> {
    let id = Uuid::new_v4().to_string();
    let max_expires_at = add(new.now, new.max_age)?;
    let expires_at = add(new.now, new.idle_ttl)?.min(max_expires_at);
    let mut tx = pool.begin().await?;
    let conversation = run_sessions::create_principal_session(
        &mut *tx,
        &run_sessions::NewRunSession {
            principal_id: new.principal_id,
            title: None,
            parent_turn_id: None,
            agent_version: Some(new.agent_version),
        },
    )
    .await?;
    sqlx::query(
        "INSERT INTO visitor_sessions (id, principal_id, embed_key_id, token_hash, session_id,
             client_ip, idle_ttl_secs, created_at, last_seen_at, expires_at, max_expires_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(new.principal_id)
    .bind(new.embed_key_id)
    .bind(new.token_hash)
    .bind(&conversation.id)
    .bind(new.client_ip)
    .bind(new.idle_ttl.as_secs())
    .bind(new.now.to_string())
    .bind(new.now.to_string())
    .bind(expires_at.to_string())
    .bind(max_expires_at.to_string())
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE chat_sessions SET visitor_id = ? WHERE id = ?")
        .bind(&id)
        .bind(&conversation.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    get(pool, &id)
        .await?
        .ok_or_else(|| DbError::Query(sqlx::Error::RowNotFound))
}

pub async fn get(pool: &Pool, id: &str) -> Result<Option<VisitorSession>, DbError> {
    let row = sqlx::query(&format!("SELECT {COLS} FROM visitor_sessions WHERE id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await?;
    row.as_ref().map(map_session).transpose()
}

/// Resolve a visitor token's hash at `now` without sliding it: a request
/// the endpoint then refuses (wrong origin, revoked key) must not keep the
/// session alive. [`slide`] once it is accepted.
pub async fn lookup(pool: &Pool, token_hash: &str, now: Timestamp) -> Result<Lookup, DbError> {
    let row = sqlx::query(&format!(
        "SELECT {COLS} FROM visitor_sessions WHERE token_hash = ?"
    ))
    .bind(token_hash)
    .fetch_optional(pool)
    .await?;
    let Some(session) = row.as_ref().map(map_session).transpose()? else {
        return Ok(Lookup::Unknown);
    };
    if now >= session.expires_at || now >= session.max_expires_at {
        return Ok(Lookup::Expired);
    }
    Ok(Lookup::Active(session))
}

/// Count an accepted request: move `expires_at` to `now + idle_ttl`, capped
/// at `max_expires_at`.
///
/// Read, decide and write under one write lock: two accepted requests whose
/// updates land in reverse order must not move `expires_at`/`last_seen_at`
/// backwards, and a request resolved just before expiry must not extend the
/// session after it has expired.
pub async fn slide(
    pool: &Pool,
    session: &VisitorSession,
    now: Timestamp,
) -> Result<VisitorSession, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let row = sqlx::query(&format!("SELECT {COLS} FROM visitor_sessions WHERE id = ?"))
        .bind(&session.id)
        .fetch_optional(&mut *tx)
        .await?;
    let Some(current) = row.as_ref().map(map_session).transpose()? else {
        return Ok(session.clone());
    };
    if now >= current.expires_at || now >= current.max_expires_at {
        return Ok(current);
    }
    let expires_at = add(now, current.idle_ttl)?
        .min(current.max_expires_at)
        .max(current.expires_at);
    let last_seen_at = now.max(current.last_seen_at);
    sqlx::query("UPDATE visitor_sessions SET last_seen_at = ?, expires_at = ? WHERE id = ?")
        .bind(last_seen_at.to_string())
        .bind(expires_at.to_string())
        .bind(&current.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(VisitorSession {
        last_seen_at,
        expires_at,
        ..current
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{agents, embed_keys, system_principals as sp};

    use std::path::Path;

    const MIN: SignedDuration = SignedDuration::from_secs(60);

    /// One accepted request at `now`, as the endpoint makes it.
    async fn touch(pool: &Pool, hash: &str, now: Timestamp) -> Result<Lookup, DbError> {
        Ok(match lookup(pool, hash, now).await? {
            Lookup::Active(v) => Lookup::Active(slide(pool, &v, now).await?),
            other => other,
        })
    }

    #[tokio::test]
    async fn a_lookup_alone_does_not_slide() {
        let fx = fixture().await;
        let v = fx.start("th", 30 * MIN, 24 * 60 * MIN).await;
        lookup(&fx.pool, "th", add(t0(), 20 * MIN).unwrap())
            .await
            .unwrap();
        assert_eq!(
            lookup(&fx.pool, "th", add(t0(), 30 * MIN).unwrap())
                .await
                .unwrap(),
            Lookup::Expired
        );
        assert_eq!(
            get(&fx.pool, &v.id).await.unwrap().unwrap().expires_at,
            v.expires_at
        );
    }

    fn t0() -> Timestamp {
        "2026-10-01T12:00:00Z".parse().unwrap()
    }

    struct Fx {
        pool: Pool,
        agent: String,
        key: String,
    }

    async fn fixture() -> Fx {
        let pool = aiplane_core::server::db::open(Path::new(":memory:"))
            .await
            .unwrap();
        let agent = agents::create(
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
        let key = embed_keys::create(
            &pool,
            &embed_keys::NewEmbedKey {
                principal_id: &agent,
                name: "site",
                origins: &["https://a.example".to_string()],
                key_hash: "kh",
            },
            "alice",
        )
        .await
        .unwrap()
        .id;
        Fx { pool, agent, key }
    }

    impl Fx {
        async fn start(
            &self,
            hash: &str,
            idle: SignedDuration,
            max: SignedDuration,
        ) -> VisitorSession {
            start(
                &self.pool,
                &NewVisitorSession {
                    principal_id: &self.agent,
                    embed_key_id: &self.key,
                    agent_version: 3,
                    token_hash: hash,
                    client_ip: Some("192.0.2.1"),
                    idle_ttl: idle,
                    max_age: max,
                    now: t0(),
                },
            )
            .await
            .unwrap()
        }
    }

    #[tokio::test]
    async fn starting_opens_a_principal_owned_conversation_linked_both_ways() {
        let fx = fixture().await;
        let v = fx.start("th", 30 * MIN, 24 * 60 * MIN).await;
        assert_eq!(v.expires_at, add(t0(), 30 * MIN).unwrap());
        assert_eq!(v.max_expires_at, add(t0(), 24 * 60 * MIN).unwrap());
        assert_eq!(v.client_ip.as_deref(), Some("192.0.2.1"));

        let run = run_sessions::get_principal_session(&fx.pool, &fx.agent, &v.session_id)
            .await
            .unwrap()
            .expect("the conversation belongs to the agent's principal");
        assert_eq!(run.agent_version, Some(3));
        let visitor_id: Option<String> =
            sqlx::query_scalar("SELECT visitor_id FROM chat_sessions WHERE id = ?")
                .bind(&v.session_id)
                .fetch_one(&fx.pool)
                .await
                .unwrap();
        assert_eq!(visitor_id.as_deref(), Some(v.id.as_str()));
    }

    #[tokio::test]
    async fn every_lookup_slides_the_idle_expiry() {
        let fx = fixture().await;
        fx.start("th", 30 * MIN, 24 * 60 * MIN).await;
        let at = add(t0(), 20 * MIN).unwrap();
        let Lookup::Active(v) = touch(&fx.pool, "th", at).await.unwrap() else {
            panic!("live within the idle TTL");
        };
        assert_eq!(v.last_seen_at, at);
        assert_eq!(v.expires_at, add(at, 30 * MIN).unwrap());
        let later = add(t0(), 45 * MIN).unwrap();
        assert!(
            matches!(
                touch(&fx.pool, "th", later).await.unwrap(),
                Lookup::Active(_)
            ),
            "45 minutes after start is only 25 after the last request"
        );
    }

    #[tokio::test]
    async fn an_idle_session_expires_and_stays_expired() {
        let fx = fixture().await;
        fx.start("th", 30 * MIN, 24 * 60 * MIN).await;
        let idle = add(t0(), 30 * MIN).unwrap();
        assert_eq!(touch(&fx.pool, "th", idle).await.unwrap(), Lookup::Expired);
        let v = touch(&fx.pool, "th", add(t0(), 31 * MIN).unwrap())
            .await
            .unwrap();
        assert_eq!(v, Lookup::Expired, "an expired lookup does not revive it");
        assert_eq!(
            touch(&fx.pool, "other", t0()).await.unwrap(),
            Lookup::Unknown
        );
    }

    #[tokio::test]
    async fn sliding_never_passes_the_absolute_cap() {
        let fx = fixture().await;
        fx.start("th", 30 * MIN, 60 * MIN).await;
        touch(&fx.pool, "th", add(t0(), 20 * MIN).unwrap())
            .await
            .unwrap();
        let at = add(t0(), 45 * MIN).unwrap();
        let Lookup::Active(v) = touch(&fx.pool, "th", at).await.unwrap() else {
            panic!("live");
        };
        assert_eq!(v.expires_at, add(t0(), 60 * MIN).unwrap());
        assert_eq!(
            touch(&fx.pool, "th", add(t0(), 60 * MIN).unwrap())
                .await
                .unwrap(),
            Lookup::Expired
        );
    }

    #[tokio::test]
    async fn an_idle_ttl_longer_than_the_cap_starts_capped() {
        let fx = fixture().await;
        let v = fx.start("th", 48 * 60 * MIN, 24 * 60 * MIN).await;
        assert_eq!(v.expires_at, v.max_expires_at);
    }

    #[tokio::test]
    async fn deleting_the_conversation_or_the_agent_ends_the_visitor_session() {
        let fx = fixture().await;
        let v = fx.start("th", 30 * MIN, 24 * 60 * MIN).await;
        sqlx::query("DELETE FROM chat_sessions WHERE id = ?")
            .bind(&v.session_id)
            .execute(&fx.pool)
            .await
            .unwrap();
        assert!(get(&fx.pool, &v.id).await.unwrap().is_none());

        let w = fx.start("th2", 30 * MIN, 24 * 60 * MIN).await;
        agents::delete(&fx.pool, &fx.agent, "alice").await.unwrap();
        assert!(get(&fx.pool, &w.id).await.unwrap().is_none());
        let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chat_sessions")
            .fetch_one(&fx.pool)
            .await
            .unwrap();
        assert_eq!(left, 0, "the agent's conversations go with it");
    }
}
