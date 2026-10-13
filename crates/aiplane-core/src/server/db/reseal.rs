// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! One-time re-sealing of secrets written under the previous at-rest key.
//!
//! # Why this exists
//!
//! The at-rest key is derived from the session secret with a domain-separation
//! label (see [`crate::server::crypto`]). That label was renamed once — from
//! `mcp-token-encryption/v1` to `at-rest-encryption/v1`, when sealing grew
//! beyond MCP tokens — and the rename shipped with **no migration**. Every
//! value sealed before it stopped decrypting, and the release notes told
//! operators to re-enter them.
//!
//! For most of these that instruction was not even possible to follow: a
//! backend API key, a connector client secret and a file-share credential are
//! all **write-only** in the UI, so "just save it again" means retyping a
//! credential the operator may not have. Losing an upstream key silently is not
//! an upgrade path.
//!
//! [`Crypto::open`] now falls back to the old key, so nothing is lost either
//! way. This pass closes the loop: it rewrites those values under the current
//! key so the fallback stops being load-bearing, and so a future release can
//! drop it.
//!
//! # Shape
//!
//! Idempotent and cheap: one `SELECT` per table, and an `UPDATE` only for rows
//! that actually need it — [`Crypto::is_legacy_sealed`] is false for anything
//! the current key already opens. A normal boot does the selects, finds
//! nothing, and moves on. Failures are logged and skipped rather than fatal: a
//! row nobody can decrypt is not made worse by leaving it alone, and refusing
//! to boot over it would take a working gateway down for a value it may never
//! read.

use crate::server::crypto::Crypto;
use crate::server::db::{DbError, Pool};

/// A sealed `(nonce, ciphertext)` column pair and the table it lives in.
struct Column {
    table: &'static str,
    /// Single-column primary key, used to address the row on update.
    id: &'static str,
    nonce: &'static str,
    ct: &'static str,
}

/// Every long-lived sealed value in the schema.
///
/// `pending_logins` and `pending_mcp_oauth` are deliberately absent: both hold
/// in-flight state that expires within minutes, so they heal on their own and a
/// migration would only race with them.
const COLUMNS: &[Column] = &[
    Column {
        table: "agent_notify_channels",
        id: "id",
        nonce: "url_nonce",
        ct: "url_ct",
    },
    Column {
        table: "backends",
        id: "name",
        nonce: "api_key_nonce",
        ct: "api_key_ct",
    },
    Column {
        table: "mcp_catalog_connectors",
        id: "key",
        nonce: "client_secret_nonce",
        ct: "client_secret_ct",
    },
    Column {
        table: "rag_collections",
        id: "id",
        nonce: "source_secrets_nonce",
        ct: "source_secrets_ct",
    },
    Column {
        table: "mcp_connections",
        id: "id",
        nonce: "access_token_nonce",
        ct: "access_token_ct",
    },
    Column {
        table: "mcp_connections",
        id: "id",
        nonce: "refresh_token_nonce",
        ct: "refresh_token_ct",
    },
    Column {
        table: "mcp_connections",
        id: "id",
        nonce: "dcr_client_secret_nonce",
        ct: "dcr_client_secret_ct",
    },
];

/// The suffix of a JSON key whose string value is sealed (`"<nonce>.<ct>"`,
/// [`Crypto::seal_to_string`]). Agent specs keep their credentials this way
/// (`token_sealed`, `secret_sealed`, …), so this pass finds every one of
/// them without knowing the spec's layout.
pub const SEALED_SUFFIX: &str = "_sealed";

/// A JSON text column that may carry sealed strings under
/// [`SEALED_SUFFIX`] keys, addressed by `rowid`.
struct JsonColumn {
    table: &'static str,
    column: &'static str,
}

/// Every agent spec as stored: the draft, each autosaved revision of it and
/// each published version. The audit trail's copies are left alone: a
/// hash-chained row is never rewritten, and nothing runs from it.
const JSON_COLUMNS: &[JsonColumn] = &[
    JsonColumn {
        table: "agents",
        column: "draft_spec",
    },
    JsonColumn {
        table: "agent_draft_revisions",
        column: "spec",
    },
    JsonColumn {
        table: "agent_versions",
        column: "spec",
    },
];

/// Re-seal everything still under the previous key. Returns how many values
/// were rewritten, for logging.
pub async fn legacy_sealed_values(pool: &Pool, crypto: &Crypto) -> Result<usize, DbError> {
    let mut rewritten = 0usize;
    for col in COLUMNS {
        rewritten += one_column(pool, crypto, col).await?;
    }
    // Secrets kept as a single `"<nonce>.<ciphertext>"` string rather than a
    // BLOB pair: the VAPID key, the web-search key, the OIDC client secret and
    // every `Kind::Secret` settings field. One table, so one pass covers all.
    rewritten += app_settings_strings(pool, crypto).await?;
    for col in JSON_COLUMNS {
        rewritten += json_column(pool, crypto, col).await?;
    }
    Ok(rewritten)
}

async fn json_column(pool: &Pool, crypto: &Crypto, col: &JsonColumn) -> Result<usize, DbError> {
    let sql = format!(
        "SELECT rowid, {column} FROM {table} WHERE {column} LIKE '%{SEALED_SUFFIX}%'",
        column = col.column,
        table = col.table
    );
    let rows: Vec<(i64, String)> = sqlx::query_as(&sql).fetch_all(pool).await?;
    let mut rewritten = 0usize;
    for (rowid, text) in rows {
        let Ok(mut doc) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let count = reseal_sealed_strings(&mut doc, crypto);
        if count == 0 {
            continue;
        }
        let update = format!(
            "UPDATE {table} SET {column} = ? WHERE rowid = ?",
            table = col.table,
            column = col.column
        );
        sqlx::query(&update)
            .bind(doc.to_string())
            .bind(rowid)
            .execute(pool)
            .await?;
        rewritten += count;
    }
    Ok(rewritten)
}

/// Re-seal, under the current key, every string under a [`SEALED_SUFFIX`]
/// key anywhere in `doc` that only a retired key opens. How many it
/// rewrote.
fn reseal_sealed_strings(doc: &mut serde_json::Value, crypto: &Crypto) -> usize {
    use serde_json::Value;
    match doc {
        Value::Object(map) => map
            .iter_mut()
            .map(|(key, value)| match value {
                Value::String(stored) if key.ends_with(SEALED_SUFFIX) => {
                    if !crypto.is_legacy_sealed_string(stored) {
                        return 0;
                    }
                    let resealed = crypto
                        .open_bytes_from_string(stored)
                        .and_then(|plain| crypto.seal_bytes_to_string(&plain).ok());
                    match resealed {
                        Some(fresh) => {
                            *stored = fresh;
                            1
                        }
                        None => {
                            tracing::warn!(
                                %key,
                                "re-sealing a legacy-encrypted spec credential failed; leaving it"
                            );
                            0
                        }
                    }
                }
                other => reseal_sealed_strings(other, crypto),
            })
            .sum(),
        Value::Array(items) => items
            .iter_mut()
            .map(|v| reseal_sealed_strings(v, crypto))
            .sum(),
        _ => 0,
    }
}

async fn one_column(pool: &Pool, crypto: &Crypto, col: &Column) -> Result<usize, DbError> {
    let sql = format!(
        "SELECT {id}, {nonce}, {ct} FROM {table} WHERE {ct} IS NOT NULL AND {nonce} IS NOT NULL",
        id = col.id,
        nonce = col.nonce,
        ct = col.ct,
        table = col.table
    );
    let rows: Vec<(String, Vec<u8>, Vec<u8>)> = sqlx::query_as(&sql).fetch_all(pool).await?;

    let mut rewritten = 0usize;
    for (id, nonce, ct) in rows {
        if !crypto.is_legacy_sealed(&nonce, &ct) {
            continue;
        }
        let Ok(plain) = crypto.open(&nonce, &ct) else {
            continue;
        };
        let Ok(sealed) = crypto.seal(&plain) else {
            tracing::warn!(
                table = col.table, column = col.ct, %id,
                "re-sealing a legacy-encrypted value failed; leaving it as it is (it still \
                 decrypts through the compatibility path)"
            );
            continue;
        };
        let update = format!(
            "UPDATE {table} SET {nonce} = ?, {ct} = ? WHERE {id} = ?",
            table = col.table,
            nonce = col.nonce,
            ct = col.ct,
            id = col.id
        );
        sqlx::query(&update)
            .bind(&sealed.nonce)
            .bind(&sealed.ciphertext)
            .bind(&id)
            .execute(pool)
            .await?;
        rewritten += 1;
    }
    Ok(rewritten)
}

async fn app_settings_strings(pool: &Pool, crypto: &Crypto) -> Result<usize, DbError> {
    // Only values that *look* sealed — the table is mostly plain rows (a model
    // id, a retention count), and those must be left exactly as they are.
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT key, value FROM app_settings WHERE value LIKE '%.%'")
            .fetch_all(pool)
            .await?;

    let mut rewritten = 0usize;
    for (key, value) in rows {
        if !crypto.is_legacy_sealed_string(&value) {
            continue;
        }
        let Some(plain) = crypto.open_bytes_from_string(&value) else {
            continue;
        };
        let Ok(sealed) = crypto.seal_bytes_to_string(&plain) else {
            tracing::warn!(%key, "re-sealing a legacy-encrypted setting failed; leaving it");
            continue;
        };
        sqlx::query("UPDATE app_settings SET value = ? WHERE key = ?")
            .bind(&sealed)
            .bind(&key)
            .execute(pool)
            .await?;
        rewritten += 1;
    }
    Ok(rewritten)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::crypto::{LABEL, RETIRED_LABELS, derive};
    use crate::server::db::open;
    use std::path::Path;

    async fn fresh() -> Pool {
        open(Path::new(":memory:")).await.unwrap()
    }

    /// What a build of the most recently retired label wrote, and what this
    /// build uses. `RETIRED_LABELS[0]` rather than a literal, so a future
    /// rotation moves these tests with it instead of silently exercising a
    /// label nothing writes any more.
    fn keys() -> (Crypto, Crypto, Crypto) {
        let session = rand::random();
        (
            Crypto::from_key(derive(&session, RETIRED_LABELS[0])),
            Crypto::from_session(&session),
            Crypto::from_key(derive(&session, LABEL)),
        )
    }

    #[tokio::test]
    async fn a_legacy_sealed_backend_key_is_rewritten_under_the_current_key() {
        let pool = fresh().await;
        let (old, now, no_fallback) = keys();
        let sealed = old.seal(b"sk-upstream").unwrap();
        sqlx::query(
            "INSERT INTO backends \
             (name, base_url, created_at, updated_at, api_key_nonce, api_key_ct) \
             VALUES ('qwen', 'http://x', '2026-01-01T00:00:00Z', \
                     '2026-01-01T00:00:00Z', ?, ?)",
        )
        .bind(&sealed.nonce)
        .bind(&sealed.ciphertext)
        .execute(&pool)
        .await
        .unwrap();

        assert_eq!(legacy_sealed_values(&pool, &now).await.unwrap(), 1);

        // Stored under the current key now: readable by a Crypto with no
        // legacy fallback at all, which is the whole point.
        let (nonce, ct): (Vec<u8>, Vec<u8>) =
            sqlx::query_as("SELECT api_key_nonce, api_key_ct FROM backends WHERE name = 'qwen'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(no_fallback.open(&nonce, &ct).unwrap(), b"sk-upstream");

        // And it is idempotent.
        assert_eq!(legacy_sealed_values(&pool, &now).await.unwrap(), 0);
    }

    /// A spec in the shape `seal_spec_secrets` writes: an A2A route's token
    /// and a host_jwt secret, sealed under `key`.
    fn sealed_spec(key: &Crypto) -> serde_json::Value {
        serde_json::json!({
            "routes": { "partner": { "a2a": { "auth": {
                "kind": "bearer",
                "token_sealed": key.seal_to_string("a2a-token").unwrap()
            } } } },
            "verifiers": { "site": {
                "kind": "host_jwt",
                "secret_sealed": key.seal_to_string("jwt-secret").unwrap(),
                "issuer": "https://www.example.com"
            } }
        })
    }

    async fn spec_texts(pool: &Pool) -> Vec<String> {
        let mut out = Vec::new();
        for sql in [
            "SELECT draft_spec FROM agents",
            "SELECT spec FROM agent_draft_revisions",
            "SELECT spec FROM agent_versions",
        ] {
            out.extend(
                sqlx::query_scalar::<_, String>(sql)
                    .fetch_all(pool)
                    .await
                    .unwrap(),
            );
        }
        out
    }

    #[tokio::test]
    async fn spec_credentials_in_drafts_revisions_and_versions_follow_the_key() {
        let pool = fresh().await;
        let (old, now, no_fallback) = keys();
        let spec = sealed_spec(&old).to_string();
        let ts = "2026-01-01T00:00:00Z";
        sqlx::query(
            "INSERT INTO system_principals (id, name, display, description, created_by, created_at)
             VALUES ('a1', 'support', 'Support', '', 'u1', ?)",
        )
        .bind(ts)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO agents (principal_id, draft_spec, live_version, created_at, updated_at)
             VALUES ('a1', ?, 1, ?, ?)",
        )
        .bind(&spec)
        .bind(ts)
        .bind(ts)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO agent_versions (principal_id, version, spec, published_by, published_at)
             VALUES ('a1', 1, ?, 'u1', ?)",
        )
        .bind(&spec)
        .bind(ts)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO agent_draft_revisions (principal_id, spec, saved_by, saved_at)
             VALUES ('a1', ?, 'u1', ?)",
        )
        .bind(&spec)
        .bind(ts)
        .execute(&pool)
        .await
        .unwrap();

        assert_eq!(legacy_sealed_values(&pool, &now).await.unwrap(), 6);

        let texts = spec_texts(&pool).await;
        assert_eq!(texts.len(), 3);
        for text in texts {
            let doc: serde_json::Value = serde_json::from_str(&text).unwrap();
            let open = |v: &serde_json::Value| no_fallback.open_from_string(v.as_str().unwrap());
            assert_eq!(
                open(&doc["routes"]["partner"]["a2a"]["auth"]["token_sealed"]).as_deref(),
                Some("a2a-token"),
                "the retired key is no longer needed: {text}"
            );
            assert_eq!(
                open(&doc["verifiers"]["site"]["secret_sealed"]).as_deref(),
                Some("jwt-secret")
            );
            assert_eq!(
                doc["verifiers"]["site"]["issuer"],
                "https://www.example.com"
            );
        }
        assert_eq!(legacy_sealed_values(&pool, &now).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn a_plain_app_setting_is_left_alone() {
        let pool = fresh().await;
        let (_, now, _) = keys();
        // Contains a dot, so it matches the LIKE filter, but is not ciphertext.
        crate::server::db::app_settings::set(&pool, "default_model.chat", "qwen.72b")
            .await
            .unwrap();

        assert_eq!(legacy_sealed_values(&pool, &now).await.unwrap(), 0);
        assert_eq!(
            crate::server::db::app_settings::get(&pool, "default_model.chat")
                .await
                .unwrap()
                .as_deref(),
            Some("qwen.72b"),
            "a plain value must survive the pass untouched"
        );
    }

    #[tokio::test]
    async fn a_legacy_sealed_setting_is_rewritten() {
        let pool = fresh().await;
        let (old, now, no_fallback) = keys();
        let stored = old.seal_to_string("vapid-private").unwrap();
        crate::server::db::app_settings::set(&pool, "push.vapid.private", &stored)
            .await
            .unwrap();

        assert_eq!(legacy_sealed_values(&pool, &now).await.unwrap(), 1);
        let after = crate::server::db::app_settings::get(&pool, "push.vapid.private")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            no_fallback.open_from_string(&after).as_deref(),
            Some("vapid-private")
        );
    }
    #[tokio::test]
    async fn generalized_user_and_principal_credentials_reseal_all_secret_pairs() {
        use crate::server::db::mcp_connections::{self, ConnectionOwner, NewConnection};
        let pool = fresh().await;
        let (old, now, no_fallback) = keys();
        sqlx::query("INSERT INTO users(id,email,roles_json,created_at,updated_at) VALUES('u','u@example.test','[]','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO system_principals(id,name,display,created_by,created_at) VALUES('p','principal','Principal','u','2026-01-01T00:00:00Z')").execute(&pool).await.unwrap();
        let catalog_secret = old.seal(b"catalog-client-secret").unwrap();
        sqlx::query("INSERT INTO mcp_catalog_connectors(key,name,url,auth,scope,client_secret_ct,client_secret_nonce,created_at,updated_at) VALUES('gmail','Actual Gmail','https://gmail.example.test/mcp','oauth2','per_user',?,?,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')")
            .bind(&catalog_secret.ciphertext).bind(&catalog_secret.nonce).execute(&pool).await.unwrap();
        let catalog_generation: String = sqlx::query_scalar(
            "SELECT dispatch_generation FROM mcp_catalog_connectors WHERE key='gmail'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        for owner in [
            ConnectionOwner::User("u".into()),
            ConnectionOwner::Principal("p".into()),
        ] {
            let access = old.seal(b"access").unwrap();
            let refresh = old.seal(b"refresh").unwrap();
            let dcr = old.seal(b"client-secret").unwrap();
            mcp_connections::upsert_connection(
                &pool,
                NewConnection {
                    owner,
                    connector_key: "gmail".into(),
                    catalog_generation: catalog_generation.clone(),
                    access_token_ct: access.ciphertext,
                    access_token_nonce: access.nonce,
                    refresh_token_ct: Some(refresh.ciphertext),
                    refresh_token_nonce: Some(refresh.nonce),
                    token_expires_at: None,
                    scopes: vec![],
                    dcr_client_id: Some("client".into()),
                    dcr_client_secret_ct: Some(dcr.ciphertext),
                    dcr_client_secret_nonce: Some(dcr.nonce),
                    token_url: None,
                },
            )
            .await
            .unwrap()
            .unwrap();
        }
        let credentials_before: Vec<(String, Option<String>)> =
            sqlx::query_as("SELECT generation,catalog_generation FROM mcp_connections ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(legacy_sealed_values(&pool, &now).await.unwrap(), 7);
        assert_eq!(
            sqlx::query_as::<_, (String, Option<String>)>(
                "SELECT generation,catalog_generation FROM mcp_connections ORDER BY id"
            )
            .fetch_all(&pool)
            .await
            .unwrap(),
            credentials_before
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT dispatch_generation FROM mcp_catalog_connectors WHERE key='gmail'"
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            catalog_generation
        );
        let (nonce, ciphertext): (Vec<u8>, Vec<u8>) = sqlx::query_as("SELECT client_secret_nonce,client_secret_ct FROM mcp_catalog_connectors WHERE key='gmail'").fetch_one(&pool).await.unwrap();
        assert_eq!(
            no_fallback.open(&nonce, &ciphertext).unwrap(),
            b"catalog-client-secret"
        );
        for owner in [
            ConnectionOwner::User("u".into()),
            ConnectionOwner::Principal("p".into()),
        ] {
            let row = mcp_connections::get_connection(&pool, &owner, "gmail")
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                no_fallback
                    .open(
                        row.access_token_nonce.as_ref().unwrap(),
                        row.access_token_ct.as_ref().unwrap()
                    )
                    .unwrap(),
                b"access"
            );
            assert_eq!(
                no_fallback
                    .open(
                        row.refresh_token_nonce.as_ref().unwrap(),
                        row.refresh_token_ct.as_ref().unwrap()
                    )
                    .unwrap(),
                b"refresh"
            );
            assert_eq!(
                no_fallback
                    .open(
                        row.dcr_client_secret_nonce.as_ref().unwrap(),
                        row.dcr_client_secret_ct.as_ref().unwrap()
                    )
                    .unwrap(),
                b"client-secret"
            );
        }
        assert_eq!(legacy_sealed_values(&pool, &now).await.unwrap(), 0);
    }
}
