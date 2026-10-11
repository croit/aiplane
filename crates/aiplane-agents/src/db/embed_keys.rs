// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! `agent_embed_keys`: which websites may embed an agent (`docs/agent-visitors.md`).
//!
//! The key itself is public — it ships in the embedding page — so the origin
//! allowlist, not the key, is what keeps other sites from embedding the agent.
//! Neither is protection against abuse: a non-browser client forges `Origin`
//! at will. That is the job of the rate limits, the budget (`docs/agent-visitors.md` → "Owner budget"), the agent's
//! default-deny grants and its gates.
//!
//! Only the SHA-256 of a key is stored. Creating and revoking one records an
//! [`agent_audit`] row in the same transaction.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use jiff::Timestamp;
use serde_json::json;
use sqlx::Row;
use sqlx::sqlite::SqliteRow;
use uuid::Uuid;

use super::agent_audit::{self, AuditKind};
use super::{DbError, Pool, WriteTx};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbedKey {
    pub id: String,
    pub principal_id: String,
    pub name: String,
    pub origins: Vec<String>,
    pub created_by: String,
    pub created_at: Timestamp,
    pub revoked_at: Option<Timestamp>,
}

impl EmbedKey {
    /// Whether a browser on `origin` may use this key. Exact match only: the
    /// stored origins are validated as `scheme://host[:port]`, which is the
    /// exact form a browser sends.
    pub fn allows(&self, origin: &str) -> bool {
        self.origins.iter().any(|o| o == origin)
    }
}

const COLS: &str = "id, principal_id, name, origins, created_by, created_at, revoked_at";

fn map_key(row: &SqliteRow) -> Result<EmbedKey, DbError> {
    let origins: String = row.try_get("origins")?;
    Ok(EmbedKey {
        id: row.try_get("id")?,
        principal_id: row.try_get("principal_id")?,
        name: row.try_get("name")?,
        origins: serde_json::from_str(&origins).map_err(|e| DbError::Decode {
            column: "origins",
            source: e.into(),
        })?,
        created_by: row.try_get("created_by")?,
        created_at: super::parse_ts(row.try_get("created_at")?, "created_at")?,
        revoked_at: super::parse_optional_ts(row.try_get("revoked_at")?, "revoked_at")?,
    })
}

pub struct NewEmbedKey<'a> {
    pub principal_id: &'a str,
    pub name: &'a str,
    pub origins: &'a [String],
    pub key_hash: &'a str,
}

/// Store a key for an agent. Fails when `principal_id` is not an agent
/// (foreign key).
pub async fn create(
    pool: &Pool,
    new: &NewEmbedKey<'_>,
    actor_id: &str,
) -> Result<EmbedKey, DbError> {
    let id = Uuid::new_v4().to_string();
    let origins = serde_json::Value::from(new.origins.to_vec()).to_string();
    let mut tx = WriteTx::begin(pool).await?;
    sqlx::query(
        "INSERT INTO agent_embed_keys (id, principal_id, name, key_hash, origins, created_by, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(new.principal_id)
    .bind(new.name)
    .bind(new.key_hash)
    .bind(&origins)
    .bind(actor_id)
    .bind(Timestamp::now().to_string())
    .execute(&mut *tx)
    .await?;
    agent_audit::append(
        &mut tx,
        agent_audit::NewEvent::new(
            AuditKind::EmbedKeyCreated,
            new.principal_id,
            json!({ "embed_key_id": id, "name": new.name, "origins": new.origins }),
        )
        .by(Some(actor_id)),
    )
    .await?;
    tx.commit().await?;
    origins_changed();
    get(pool, &id)
        .await?
        .ok_or_else(|| DbError::Query(sqlx::Error::RowNotFound))
}

pub async fn get(pool: &Pool, id: &str) -> Result<Option<EmbedKey>, DbError> {
    let row = sqlx::query(&format!("SELECT {COLS} FROM agent_embed_keys WHERE id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await?;
    row.as_ref().map(map_key).transpose()
}

/// The key behind a hash, revoked or not: the caller tells a revoked key
/// apart from an unknown one, because the fix differs.
pub async fn find_by_hash(pool: &Pool, key_hash: &str) -> Result<Option<EmbedKey>, DbError> {
    let row = sqlx::query(&format!(
        "SELECT {COLS} FROM agent_embed_keys WHERE key_hash = ?"
    ))
    .bind(key_hash)
    .fetch_optional(pool)
    .await?;
    row.as_ref().map(map_key).transpose()
}

pub async fn list(pool: &Pool, principal_id: &str) -> Result<Vec<EmbedKey>, DbError> {
    let rows = sqlx::query(&format!(
        "SELECT {COLS} FROM agent_embed_keys WHERE principal_id = ? ORDER BY created_at, id"
    ))
    .bind(principal_id)
    .fetch_all(pool)
    .await?;
    rows.iter().map(map_key).collect()
}

/// Revoke one of the agent's keys. `Ok(false)` when it has no live key with
/// that id.
pub async fn revoke(
    pool: &Pool,
    principal_id: &str,
    key_id: &str,
    actor_id: &str,
) -> Result<bool, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let changed = sqlx::query(
        "UPDATE agent_embed_keys SET revoked_at = ?
          WHERE id = ? AND principal_id = ? AND revoked_at IS NULL",
    )
    .bind(Timestamp::now().to_string())
    .bind(key_id)
    .bind(principal_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if changed == 0 {
        return Ok(false);
    }
    agent_audit::append(
        &mut tx,
        agent_audit::NewEvent::new(
            AuditKind::EmbedKeyRevoked,
            principal_id,
            json!({ "embed_key_id": key_id }),
        )
        .by(Some(actor_id)),
    )
    .await?;
    tx.commit().await?;
    origins_changed();
    Ok(true)
}

/// Every origin some live key of an enabled agent lists. This is what a CORS
/// preflight can be answered from: it carries neither the key nor the
/// visitor token, so it cannot be scoped to one key. Each request is then
/// checked against its own key's origins (and the spec's `publish.origins`).
pub async fn embeddable_origins(pool: &Pool) -> Result<HashSet<String>, DbError> {
    let rows = sqlx::query(
        "SELECT k.origins FROM agent_embed_keys k
           JOIN system_principals p ON p.id = k.principal_id
          WHERE k.revoked_at IS NULL AND p.disabled_at IS NULL",
    )
    .fetch_all(pool)
    .await?;
    let mut all = HashSet::new();
    for row in rows {
        let origins: String = row.try_get("origins")?;
        let origins: Vec<String> = serde_json::from_str(&origins).map_err(|e| DbError::Decode {
            column: "origins",
            source: e.into(),
        })?;
        all.extend(origins);
    }
    Ok(all)
}

/// Bumped after every committed write that can change
/// [`embeddable_origins`]: a key created or revoked, an agent disabled or
/// deleted. Process-wide rather than per pool — a bump for another pool only
/// costs a cache a reload, never a stale answer.
static ORIGINS_GENERATION: AtomicU64 = AtomicU64::new(0);

pub fn origins_changed() {
    ORIGINS_GENERATION.fetch_add(1, Ordering::SeqCst);
}

/// Backstop for writes this process cannot see (another process on the same
/// database file, a hand-run SQL fix): within this long they apply anyway.
const ORIGINS_TTL: Duration = Duration::from_secs(30);

/// [`embeddable_origins`], cached for the CORS layer, which asks on every
/// `/api/v0/embed/*` request. Reloaded when this module's writes changed
/// the set (at once) or after [`ORIGINS_TTL`].
#[derive(Debug)]
pub struct EmbeddableOrigins {
    ttl: Duration,
    cached: Mutex<Option<Snapshot>>,
}

#[derive(Debug)]
struct Snapshot {
    generation: u64,
    loaded_at: Instant,
    origins: Arc<HashSet<String>>,
}

impl Default for EmbeddableOrigins {
    fn default() -> Self {
        Self::with_ttl(ORIGINS_TTL)
    }
}

impl EmbeddableOrigins {
    pub fn with_ttl(ttl: Duration) -> Self {
        Self {
            ttl,
            cached: Mutex::new(None),
        }
    }

    /// Whether some live key of an enabled agent lists `origin`.
    pub async fn allows(&self, pool: &Pool, origin: &str) -> Result<bool, DbError> {
        let generation = ORIGINS_GENERATION.load(Ordering::SeqCst);
        if let Some(origins) = self.fresh(generation) {
            return Ok(origins.contains(origin));
        }
        let origins = Arc::new(embeddable_origins(pool).await?);
        *self.cached.lock().unwrap_or_else(|p| p.into_inner()) = Some(Snapshot {
            generation,
            loaded_at: Instant::now(),
            origins: origins.clone(),
        });
        Ok(origins.contains(origin))
    }

    fn fresh(&self, generation: u64) -> Option<Arc<HashSet<String>>> {
        let cached = self.cached.lock().unwrap_or_else(|p| p.into_inner());
        cached
            .as_ref()
            .filter(|s| s.generation == generation && s.loaded_at.elapsed() < self.ttl)
            .map(|s| s.origins.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{agents, system_principals as sp};
    use std::path::Path;

    async fn pool() -> Pool {
        aiplane_core::server::db::open(Path::new(":memory:"))
            .await
            .unwrap()
    }

    async fn agent(pool: &Pool, name: &str) -> String {
        agents::create(
            pool,
            &sp::NewPrincipal {
                name,
                display: name,
                description: "",
            },
            "{}",
            "alice",
        )
        .await
        .unwrap()
        .unwrap()
        .principal
        .id
    }

    async fn key(pool: &Pool, agent: &str, hash: &str, origins: &[&str]) -> EmbedKey {
        let origins: Vec<String> = origins.iter().map(|o| o.to_string()).collect();
        create(
            pool,
            &NewEmbedKey {
                principal_id: agent,
                name: "website",
                origins: &origins,
                key_hash: hash,
            },
            "alice",
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn a_key_is_found_by_its_hash_and_allows_exactly_its_origins() {
        let pool = pool().await;
        let a = agent(&pool, "support").await;
        let k = key(
            &pool,
            &a,
            "h1",
            &["https://www.example.com", "http://localhost:5173"],
        )
        .await;
        let found = find_by_hash(&pool, "h1").await.unwrap().unwrap();
        assert_eq!(found, k);
        assert!(found.allows("https://www.example.com"));
        assert!(found.allows("http://localhost:5173"));
        assert!(!found.allows("https://www.example.com/"));
        assert!(!found.allows("http://www.example.com"));
        assert!(!found.allows("https://evil.example"));
        assert!(find_by_hash(&pool, "nope").await.unwrap().is_none());
        assert_eq!(list(&pool, &a).await.unwrap(), [k]);
    }

    #[tokio::test]
    async fn a_key_can_only_belong_to_an_agent() {
        let pool = pool().await;
        let plain = sp::create(
            &pool,
            &sp::NewPrincipal {
                name: "ci",
                display: "CI",
                description: "",
            },
            "alice",
        )
        .await
        .unwrap()
        .unwrap();
        let res = create(
            &pool,
            &NewEmbedKey {
                principal_id: &plain.id,
                name: "x",
                origins: &[],
                key_hash: "h",
            },
            "alice",
        )
        .await;
        assert!(res.is_err(), "a key for a non-agent principal was stored");
    }

    #[tokio::test]
    async fn revoking_stamps_the_key_once_and_audits_it() {
        let pool = pool().await;
        let a = agent(&pool, "support").await;
        let other = agent(&pool, "sales").await;
        let k = key(&pool, &a, "h1", &["https://a.example"]).await;
        assert!(!revoke(&pool, &other, &k.id, "bob").await.unwrap());
        assert!(revoke(&pool, &a, &k.id, "bob").await.unwrap());
        assert!(!revoke(&pool, &a, &k.id, "bob").await.unwrap());
        assert!(
            get(&pool, &k.id)
                .await
                .unwrap()
                .unwrap()
                .revoked_at
                .is_some()
        );
        let audit = agent_audit::for_principal(&pool, &a).await.unwrap();
        assert_eq!(audit[0].kind, "embed_key_revoked");
        assert_eq!(audit[0].actor_id.as_deref(), Some("bob"));
        assert_eq!(audit[1].kind, "embed_key_created");
        assert_eq!(audit[1].detail["origins"][0], "https://a.example");
    }

    #[tokio::test]
    async fn an_origin_is_embeddable_only_through_a_live_key_of_an_enabled_agent() {
        let pool = pool().await;
        let origins = EmbeddableOrigins::default();
        let allows = |o: &'static str| {
            let (pool, origins) = (pool.clone(), &origins);
            async move { origins.allows(&pool, o).await.unwrap() }
        };
        let a = agent(&pool, "support").await;
        let b = agent(&pool, "sales").await;
        let c = agent(&pool, "billing").await;
        let ka = key(&pool, &a, "ha", &["https://a.example"]).await;
        assert!(allows("https://a.example").await);
        assert!(!allows("https://b.example").await, "the cache is warm now");
        key(&pool, &b, "hb", &["https://b.example"]).await;
        assert!(allows("https://b.example").await, "a new key opens at once");
        key(&pool, &c, "hc", &["https://c.example"]).await;
        assert!(!allows("https://d.example").await);

        revoke(&pool, &a, &ka.id, "alice").await.unwrap();
        assert!(!allows("https://a.example").await, "a revoked key at once");
        sp::disable(&pool, &b, "alice").await.unwrap();
        assert!(
            !allows("https://b.example").await,
            "a disabled agent at once"
        );
        agents::delete(&pool, &c, "alice").await.unwrap();
        assert!(
            !allows("https://c.example").await,
            "a deleted agent at once"
        );
    }

    /// The point of the cache: a CORS answer is not a table scan. A write
    /// that bypasses this module is not seen until the backstop TTL.
    #[tokio::test]
    async fn a_warm_cache_answers_without_reading_the_keys_again() {
        let pool = pool().await;
        let origins = EmbeddableOrigins::default();
        let a = agent(&pool, "support").await;
        key(&pool, &a, "ha", &["https://a.example"]).await;
        assert!(origins.allows(&pool, "https://a.example").await.unwrap());
        sqlx::query("DELETE FROM agent_embed_keys")
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            origins.allows(&pool, "https://a.example").await.unwrap(),
            "answered from the cache"
        );
        let expired = EmbeddableOrigins::with_ttl(std::time::Duration::ZERO);
        assert!(!expired.allows(&pool, "https://a.example").await.unwrap());
    }

    #[tokio::test]
    async fn deleting_the_agent_deletes_its_keys() {
        let pool = pool().await;
        let a = agent(&pool, "support").await;
        let k = key(&pool, &a, "h1", &["https://a.example"]).await;
        agents::delete(&pool, &a, "alice").await.unwrap();
        assert!(get(&pool, &k.id).await.unwrap().is_none());
    }
}
