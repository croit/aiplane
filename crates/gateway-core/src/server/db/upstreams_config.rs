// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Database-backed upstream topology — pools, backends, and their relationships.
//!
//! Replaces the `[upstream_pools]` TOML sections with DB rows managed through
//! the admin UI. [`load_snapshot`] reads the full topology in a handful of
//! queries for registry rebuilds; the CRUD functions power `/admin/backends`
//! and `/admin/pools`.
//!
//! Schema lives in `migrations/0042_upstream_config_db.sql`.

use std::collections::{HashMap, HashSet};

use jiff::Timestamp;
use sqlx::Row;
use sqlx::sqlite::SqliteRow;

use super::{DbError, Pool};

// ---------------------------------------------------------------------------
// Row structs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct BackendRow {
    pub name: String,
    pub base_url: String,
    /// Optional env-var NAME holding the API key — a fallback resolved from the
    /// environment only when no sealed [`api_key_ct`](Self::api_key_ct) is set.
    pub api_key_env: Option<String>,
    /// The API key value itself, AES-256-GCM sealed (`ciphertext`, `nonce`). The
    /// DB layer treats these as opaque blobs; only a holder of
    /// [`crate::server::crypto::Crypto`] can `open` them (see `db_bridge`).
    /// `None` when the backend has no stored key (uses `api_key_env` or none).
    pub api_key_ct: Option<Vec<u8>>,
    pub api_key_nonce: Option<Vec<u8>>,
    pub weight: u32,
    pub max_inflight: u32,
    pub health_path: String,
    pub probe_models: bool,
    pub supports_edit: bool,
    /// `false` = taken out of rotation for maintenance (migration 0063).
    pub enabled: bool,
    pub models: Vec<String>,
    pub aliases: Vec<AliasRow>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AliasRow {
    pub alias: String,
    pub target: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PoolRow {
    pub name: String,
    pub kind: String,
    pub strategy: String,
    pub fallback_offline: Option<String>,
    pub compliance_gdpr: bool,
    pub compliance_nda: bool,
    pub enforce_limits: bool,
    pub sort_order: i64,
    /// Gateway-group names allowed to see + route to this pool (JSON array in
    /// the `allowed_groups` column). Empty = unrestricted. See
    /// `migrations/0045_pool_allowed_groups.sql`.
    pub allowed_groups: Vec<String>,
    pub backends: Vec<String>,
    pub models: Vec<String>,
    pub voices: Vec<VoiceRow>,
    /// Voices this pool offers users to pick from, in menu order. Distinct from
    /// [`Self::voices`], which maps a language to the voice to *use*: that table
    /// holds one voice per language and so cannot express "three German voices
    /// to choose between". Empty = no menu; see `migrations/0057`.
    pub offer_voices: Vec<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VoiceRow {
    pub lang_code: String,
    pub voice_id: String,
}

/// Complete topology snapshot for an [`crate::server::upstreams::UpstreamRegistry`]
/// rebuild. Loaded in a small fixed number of queries.
#[derive(Debug, Clone, Default)]
pub struct UpstreamConfigSnapshot {
    pub pools: Vec<PoolRow>,
    pub backends: HashMap<String, BackendRow>,
    pub fallbacks: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// Timestamp helper
// ---------------------------------------------------------------------------

fn parse_ts(col: &'static str, row: &SqliteRow) -> Result<Timestamp, DbError> {
    let s: String = row.try_get(col)?;
    s.parse().map_err(|e: jiff::Error| DbError::Decode {
        column: col,
        source: e.into(),
    })
}

fn now_rfc3339() -> String {
    Timestamp::now().to_string()
}

// ---------------------------------------------------------------------------
// Snapshot loader — used by registry rebuild on startup and on "Apply changes"
// ---------------------------------------------------------------------------

/// Loads the complete upstream topology from the database.
///
/// Returns an empty snapshot when no pools/backends are configured yet (first
/// boot). Drives the registry build on startup and on "Apply changes".
pub async fn load_snapshot(db: &Pool) -> Result<UpstreamConfigSnapshot, DbError> {
    let backends = load_all_backends(db).await?;
    let pools = load_all_pools(db).await?;
    let fallbacks = load_all_fallbacks(db).await?;
    Ok(UpstreamConfigSnapshot {
        pools,
        backends,
        fallbacks,
    })
}

/// True when the DB has no pools (and therefore no usable topology). Note the
/// first-boot seed in `main.rs` gates on a persistent `topology.seeded` marker,
/// not on this — deleting every pool via the UI must not trigger a reseed.
pub async fn is_empty(db: &Pool) -> Result<bool, DbError> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pools")
        .fetch_one(db)
        .await?;
    Ok(count == 0)
}

async fn load_all_backends(db: &Pool) -> Result<HashMap<String, BackendRow>, DbError> {
    let rows = sqlx::query(
        r#"SELECT name, base_url, api_key_env, api_key_ct, api_key_nonce, weight, max_inflight,
                  health_path, probe_models, supports_edit, enabled, created_at, updated_at
             FROM backends ORDER BY name"#,
    )
    .fetch_all(db)
    .await?;

    let mut backends: HashMap<String, BackendRow> = HashMap::new();
    for row in &rows {
        let name: String = row.try_get("name")?;
        let created_at = parse_ts("created_at", row)?;
        let updated_at = parse_ts("updated_at", row)?;
        backends.insert(
            name.clone(),
            BackendRow {
                name: name.clone(),
                base_url: row.try_get("base_url")?,
                api_key_env: row.try_get("api_key_env")?,
                api_key_ct: row.try_get("api_key_ct")?,
                api_key_nonce: row.try_get("api_key_nonce")?,
                weight: row.try_get::<u32, _>("weight")?,
                max_inflight: row.try_get::<u32, _>("max_inflight")?,
                health_path: row.try_get("health_path")?,
                probe_models: row.try_get::<i64, _>("probe_models")? != 0,
                supports_edit: row.try_get::<i64, _>("supports_edit")? != 0,
                enabled: row.try_get::<i64, _>("enabled")? != 0,
                models: Vec::new(),
                aliases: Vec::new(),
                created_at,
                updated_at,
            },
        );
    }

    // Load models for all backends in one query.
    let model_rows = sqlx::query(
        r#"SELECT b.name AS backend_name, m.model_id
              FROM backend_models m JOIN backends b ON b.id = m.backend_id
             ORDER BY b.name, m.sort_order"#,
    )
    .fetch_all(db)
    .await?;
    for row in &model_rows {
        let backend_name: String = row.try_get("backend_name")?;
        let model_id: String = row.try_get("model_id")?;
        if let Some(b) = backends.get_mut(&backend_name) {
            b.models.push(model_id);
        }
    }

    // Load aliases for all backends in one query.
    let alias_rows = sqlx::query(
        r#"SELECT b.name AS backend_name, a.alias, a.target
              FROM backend_aliases a JOIN backends b ON b.id = a.backend_id
             ORDER BY b.name, a.alias"#,
    )
    .fetch_all(db)
    .await?;
    for row in &alias_rows {
        let backend_name: String = row.try_get("backend_name")?;
        let alias_row = AliasRow {
            alias: row.try_get("alias")?,
            target: row.try_get("target")?,
        };
        if let Some(b) = backends.get_mut(&backend_name) {
            b.aliases.push(alias_row);
        }
    }

    Ok(backends)
}

async fn load_all_pools(db: &Pool) -> Result<Vec<PoolRow>, DbError> {
    let rows = sqlx::query(
        r#"SELECT name, kind, strategy, fallback_offline, compliance_gdpr,
                  compliance_nda, enforce_limits, sort_order, allowed_groups,
                  created_at, updated_at
             FROM pools ORDER BY sort_order, name"#,
    )
    .fetch_all(db)
    .await?;

    let mut pools: Vec<PoolRow> = Vec::new();
    for row in &rows {
        let created_at = parse_ts("created_at", row)?;
        let updated_at = parse_ts("updated_at", row)?;
        let allowed_groups_json: String = row.try_get("allowed_groups")?;
        let allowed_groups: Vec<String> =
            serde_json::from_str(&allowed_groups_json).map_err(|e| DbError::Decode {
                column: "allowed_groups",
                source: e.into(),
            })?;
        pools.push(PoolRow {
            name: row.try_get("name")?,
            kind: row.try_get("kind")?,
            strategy: row.try_get("strategy")?,
            fallback_offline: row.try_get("fallback_offline")?,
            compliance_gdpr: row.try_get::<i64, _>("compliance_gdpr")? != 0,
            compliance_nda: row.try_get::<i64, _>("compliance_nda")? != 0,
            enforce_limits: row.try_get::<i64, _>("enforce_limits")? != 0,
            sort_order: row.try_get("sort_order")?,
            allowed_groups,
            backends: Vec::new(),
            models: Vec::new(),
            voices: Vec::new(),
            offer_voices: Vec::new(),
            created_at,
            updated_at,
        });
    }

    // Load pool-backend assignments.
    let pb_rows = sqlx::query(
        r#"SELECT p.name AS pool_name, b.name AS backend_name
              FROM pool_backends pb
              JOIN pools p ON p.id = pb.pool_id
              JOIN backends b ON b.id = pb.backend_id
             ORDER BY p.name, pb.sort_order"#,
    )
    .fetch_all(db)
    .await?;
    for row in &pb_rows {
        let pool_name: String = row.try_get("pool_name")?;
        let backend_name: String = row.try_get("backend_name")?;
        if let Some(p) = pools.iter_mut().find(|p| p.name == pool_name) {
            p.backends.push(backend_name);
        }
    }

    // Load pool models.
    let pm_rows = sqlx::query(
        r#"SELECT p.name AS pool_name, m.model_id
              FROM pool_models m JOIN pools p ON p.id = m.pool_id
             ORDER BY p.name, m.sort_order"#,
    )
    .fetch_all(db)
    .await?;
    for row in &pm_rows {
        let pool_name: String = row.try_get("pool_name")?;
        let model_id: String = row.try_get("model_id")?;
        if let Some(p) = pools.iter_mut().find(|p| p.name == pool_name) {
            p.models.push(model_id);
        }
    }

    // Load pool voices.
    let pv_rows = sqlx::query(
        r#"SELECT p.name AS pool_name, v.lang_code, v.voice_id
              FROM pool_voices v JOIN pools p ON p.id = v.pool_id
             ORDER BY p.name, v.lang_code"#,
    )
    .fetch_all(db)
    .await?;
    for row in &pv_rows {
        let pool_name: String = row.try_get("pool_name")?;
        let voice = VoiceRow {
            lang_code: row.try_get("lang_code")?,
            voice_id: row.try_get("voice_id")?,
        };
        if let Some(p) = pools.iter_mut().find(|p| p.name == pool_name) {
            p.voices.push(voice);
        }
    }

    // Load the offerable-voice menus.
    let ov_rows = sqlx::query(
        r#"SELECT p.name AS pool_name, o.voice_id
              FROM pool_offer_voices o JOIN pools p ON p.id = o.pool_id
             ORDER BY p.name, o.sort_order, o.voice_id"#,
    )
    .fetch_all(db)
    .await?;
    for row in &ov_rows {
        let pool_name: String = row.try_get("pool_name")?;
        let voice_id: String = row.try_get("voice_id")?;
        if let Some(p) = pools.iter_mut().find(|p| p.name == pool_name) {
            p.offer_voices.push(voice_id);
        }
    }

    Ok(pools)
}

async fn load_all_fallbacks(db: &Pool) -> Result<HashMap<String, String>, DbError> {
    let rows = sqlx::query("SELECT kind, model_id FROM fallback_models")
        .fetch_all(db)
        .await?;
    let mut map = HashMap::new();
    for row in &rows {
        map.insert(row.try_get("kind")?, row.try_get("model_id")?);
    }
    Ok(map)
}

// ---------------------------------------------------------------------------
// Backend CRUD
// ---------------------------------------------------------------------------

/// Fetch a single backend by name.
pub async fn get_backend(db: &Pool, name: &str) -> Result<Option<BackendRow>, DbError> {
    let mut backends = load_all_backends(db).await?;
    Ok(backends.remove(name))
}

/// Remember the model set a backend was last seen serving (see migration
/// `0062_backend_probed_models`). Replaces the whole set for that backend.
///
/// Called from the health probe whenever a `/models` response differs from what
/// the backend previously advertised, so the rows track the live loadout. The
/// point is the *next* boot: without them, a gateway that starts while a backend
/// is down knows of no models at all and answers `404 model_not_found` — a
/// non-retryable "that model does not exist" — instead of the `503` an outage
/// deserves.
pub async fn save_probed_models(
    db: &Pool,
    backend_name: &str,
    models: &HashSet<String>,
) -> Result<(), DbError> {
    let now = now_rfc3339();
    let mut tx = db.begin().await?;
    sqlx::query("DELETE FROM backend_probed_models WHERE backend_id = (SELECT id FROM backends WHERE name = ?)")
        .bind(backend_name)
        .execute(&mut *tx)
        .await?;
    for model_id in models {
        sqlx::query(
            r#"INSERT INTO backend_probed_models (backend_id, model_id, seen_at)
               SELECT id, ?, ? FROM backends WHERE name = ?"#,
        )
        .bind(model_id)
        .bind(&now)
        .bind(backend_name)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// The remembered model sets, keyed by backend name. Read once at startup to
/// seed the registry before the first probe round — see [`save_probed_models`].
pub async fn load_probed_models(db: &Pool) -> Result<HashMap<String, HashSet<String>>, DbError> {
    let rows = sqlx::query(
        r#"SELECT b.name AS backend_name, p.model_id
              FROM backend_probed_models p JOIN backends b ON b.id = p.backend_id"#,
    )
    .fetch_all(db)
    .await?;
    let mut out: HashMap<String, HashSet<String>> = HashMap::new();
    for row in &rows {
        let backend_name: String = row.try_get("backend_name")?;
        let model_id: String = row.try_get("model_id")?;
        out.entry(backend_name).or_default().insert(model_id);
    }
    Ok(out)
}

/// Store what a detection round learned about one backend (migration
/// `0067_backend_detected_profile`). Replaces the whole record for that
/// backend, context windows included.
///
/// Called when the topology is applied and from the admin test button — never
/// from the recurring health probe, which stays a liveness + model-set check.
/// The point is the same as [`save_probed_models`]'s: a gateway that boots
/// while a backend is down would otherwise show "generic" for it and read its
/// context from nowhere, so every model on it would silently fall back to the
/// global 32768 guess.
pub async fn save_detected(
    db: &Pool,
    backend_name: &str,
    detected: &crate::server::upstreams::profile::Detected,
) -> Result<(), DbError> {
    let now = now_rfc3339();
    // Resolve the id once. Every statement below used to carry its own
    // `SELECT id FROM backends WHERE name = ?` subquery — including the
    // per-window insert, so a fifty-model backend re-ran that lookup fifty
    // times inside one transaction. It also makes "no such backend" an
    // explicit no-op instead of a silent zero-row insert.
    let Some(backend_id): Option<i64> =
        sqlx::query_scalar("SELECT id FROM backends WHERE name = ?")
            .bind(backend_name)
            .fetch_optional(db)
            .await?
    else {
        return Ok(());
    };
    let mut tx = db.begin().await?;
    sqlx::query(
        r#"INSERT INTO backend_detected
               (backend_id, profile, context_cap, max_parallel, version, detected_at)
           VALUES (?, ?, ?, ?, ?, ?)
           ON CONFLICT(backend_id) DO UPDATE SET
               profile = excluded.profile,
               context_cap = excluded.context_cap,
               max_parallel = excluded.max_parallel,
               version = excluded.version,
               detected_at = excluded.detected_at"#,
    )
    .bind(backend_id)
    .bind(detected.profile.as_str())
    .bind(detected.context_cap)
    .bind(detected.max_parallel.map(i64::from))
    .bind(detected.version.as_deref())
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM backend_detected_context WHERE backend_id = ?")
        .bind(backend_id)
        .execute(&mut *tx)
        .await?;
    for (model_id, window) in &detected.context_windows {
        sqlx::query(
            r#"INSERT INTO backend_detected_context (backend_id, model_id, context_window)
               VALUES (?, ?, ?)"#,
        )
        .bind(backend_id)
        .bind(model_id)
        .bind(window)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Everything detection last learned, keyed by backend name. Read at startup
/// to seed the registry before anything is re-detected — see [`save_detected`].
pub async fn load_detected(
    db: &Pool,
) -> Result<HashMap<String, crate::server::upstreams::profile::Detected>, DbError> {
    use crate::server::upstreams::profile::{BackendProfile, Detected};

    let rows = sqlx::query(
        r#"SELECT b.name AS backend_name, d.profile, d.context_cap, d.max_parallel,
                     d.version, d.detected_at
              FROM backend_detected d JOIN backends b ON b.id = d.backend_id"#,
    )
    .fetch_all(db)
    .await?;
    let mut out: HashMap<String, Detected> = HashMap::new();
    for row in &rows {
        let backend_name: String = row.try_get("backend_name")?;
        let profile: String = row.try_get("profile")?;
        let context_cap: Option<i64> = row.try_get("context_cap")?;
        let max_parallel: Option<i64> = row.try_get("max_parallel")?;
        let version: Option<String> = row.try_get("version")?;
        let detected_at: Option<String> = row.try_get("detected_at")?;
        out.insert(
            backend_name,
            Detected {
                profile: BackendProfile::parse(Some(&profile)),
                context_windows: HashMap::new(),
                context_cap,
                max_parallel: max_parallel.and_then(|n| u32::try_from(n).ok()),
                version,
                detected_at,
            },
        );
    }

    let windows = sqlx::query(
        r#"SELECT b.name AS backend_name, c.model_id, c.context_window
              FROM backend_detected_context c JOIN backends b ON b.id = c.backend_id"#,
    )
    .fetch_all(db)
    .await?;
    for row in &windows {
        let backend_name: String = row.try_get("backend_name")?;
        let model_id: String = row.try_get("model_id")?;
        let window: i64 = row.try_get("context_window")?;
        if let Some(entry) = out.get_mut(&backend_name) {
            entry.context_windows.insert(model_id, window);
        }
    }
    Ok(out)
}

/// Flip a backend's maintenance switch in the database.
///
/// The **live** registry is flipped separately and immediately
/// (`UpstreamRegistry::set_backend_enabled`) — this is only the part that has
/// to survive a restart. A maintenance switch you have to "apply" is not a
/// maintenance switch, so the two are deliberately not coupled to the
/// topology-reload flow.
pub async fn set_backend_enabled(db: &Pool, name: &str, enabled: bool) -> Result<(), DbError> {
    sqlx::query("UPDATE backends SET enabled = ?, updated_at = ? WHERE name = ?")
        .bind(enabled as i64)
        .bind(now_rfc3339())
        .bind(name)
        .execute(db)
        .await?;
    Ok(())
}

/// Whether a backend with this name already exists.
///
/// Both `upsert_backend` and `upsert_pool` are upserts keyed on `name`, so the
/// admin UI needs to tell "create" from "replace" *before* it writes — an add
/// form that silently overwrote an existing backend cost an operator a working
/// upstream (the second backend they added to a pool replaced the first).
pub async fn backend_exists(db: &Pool, name: &str) -> Result<bool, DbError> {
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM backends WHERE name = ?")
        .bind(name)
        .fetch_one(db)
        .await?;
    Ok(n > 0)
}

/// Whether a pool with this name already exists. See [`backend_exists`].
pub async fn pool_exists(db: &Pool, name: &str) -> Result<bool, DbError> {
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pools WHERE name = ?")
        .bind(name)
        .fetch_one(db)
        .await?;
    Ok(n > 0)
}

/// Insert or update a backend, replacing its models and aliases atomically.
pub async fn upsert_backend(db: &Pool, row: &BackendRow) -> Result<(), DbError> {
    let now = now_rfc3339();
    sqlx::query(
        r#"INSERT INTO backends
               (name, base_url, api_key_env, api_key_ct, api_key_nonce, weight, max_inflight,
                health_path, probe_models, supports_edit, enabled, created_at, updated_at)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
           ON CONFLICT(name) DO UPDATE SET
               base_url      = excluded.base_url,
               api_key_env   = excluded.api_key_env,
               api_key_ct    = excluded.api_key_ct,
               api_key_nonce = excluded.api_key_nonce,
               weight        = excluded.weight,
               max_inflight  = excluded.max_inflight,
               health_path   = excluded.health_path,
               probe_models  = excluded.probe_models,
               supports_edit = excluded.supports_edit,
               enabled       = excluded.enabled,
               updated_at    = excluded.updated_at"#,
    )
    .bind(&row.name)
    .bind(&row.base_url)
    .bind(&row.api_key_env)
    .bind(&row.api_key_ct)
    .bind(&row.api_key_nonce)
    .bind(row.weight)
    .bind(row.max_inflight)
    .bind(&row.health_path)
    .bind(row.probe_models as i64)
    .bind(row.supports_edit as i64)
    .bind(row.enabled as i64)
    .bind(&now)
    .bind(&now)
    .execute(db)
    .await?;

    replace_backend_models(db, &row.name, &row.models).await?;
    replace_backend_aliases(db, &row.name, &row.aliases).await?;
    Ok(())
}

/// Delete a backend and all its dependent rows (models, aliases, pool links).
/// Outcome of a rename attempt, so callers can answer precisely rather than
/// turning every refusal into a 500.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenameOutcome {
    Renamed,
    /// No row by that name.
    NotFound,
    /// The new name is already taken.
    Taken,
}

/// Rename a backend.
///
/// One `UPDATE`, because the name is a label and `id` is the identity: no child
/// row mentions the name, so nothing else has to move and nothing can be
/// forgotten. Before the surrogate key this was a transaction across eight
/// tables plus a hand-maintained list of them.
///
/// `usage_daily` / `usage_events` are the exception, and deliberately so: they
/// record the name with no foreign key, as a denormalized historical label. They
/// move with the rename, because an operator renaming a backend when the same
/// host started serving a different model would otherwise find their own
/// traffic split across two names on /usage.
pub async fn rename_backend(db: &Pool, old: &str, new: &str) -> Result<RenameOutcome, DbError> {
    let mut tx = db.begin().await?;
    let outcome = rename_in(&mut tx, "backends", old, new).await?;
    if outcome == RenameOutcome::Renamed {
        // `usage_events` is keyed by a uuid, so its label moves with an UPDATE.
        sqlx::query("UPDATE usage_events SET backend = ? WHERE backend = ?")
            .bind(new)
            .bind(old)
            .execute(&mut *tx)
            .await?;
        // `usage_daily` is not: `backend` is part of its primary key, so an
        // UPDATE onto a name that already has a row for the same day, user,
        // token, source, kind and model violates it — and because usage rows
        // deliberately outlive the backend they name, reusing a name that once
        // had traffic is exactly when a rename would hit this. Merge instead:
        // sum the counters into the target row, then drop the source rows.
        sqlx::query(
            r#"INSERT INTO usage_daily
                   (day, user_id, user_email, token_id, token_name, source, kind, backend, model,
                    req_count, error_count, prompt_tokens, completion_tokens, total_tokens,
                    input_units, output_units, cost)
               SELECT day, user_id, user_email, token_id, token_name, source, kind, ?, model,
                      req_count, error_count, prompt_tokens, completion_tokens, total_tokens,
                      input_units, output_units, cost
                 FROM usage_daily WHERE backend = ?
               ON CONFLICT(day, user_id, token_id, source, kind, backend, model) DO UPDATE SET
                   req_count         = req_count + excluded.req_count,
                   error_count       = error_count + excluded.error_count,
                   prompt_tokens     = prompt_tokens + excluded.prompt_tokens,
                   completion_tokens = completion_tokens + excluded.completion_tokens,
                   total_tokens      = total_tokens + excluded.total_tokens,
                   input_units       = input_units + excluded.input_units,
                   output_units      = output_units + excluded.output_units,
                   cost              = cost + excluded.cost"#,
        )
        .bind(new)
        .bind(old)
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM usage_daily WHERE backend = ?")
            .bind(old)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(outcome)
}

/// Rename a pool. Nothing outside the `pools` row names a pool, so this is the
/// whole operation.
pub async fn rename_pool(db: &Pool, old: &str, new: &str) -> Result<RenameOutcome, DbError> {
    let mut tx = db.begin().await?;
    let outcome = rename_in(&mut tx, "pools", old, new).await?;
    tx.commit().await?;
    Ok(outcome)
}

async fn rename_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    table: &str,
    old: &str,
    new: &str,
) -> Result<RenameOutcome, DbError> {
    if old != new {
        let taken: i64 = sqlx::query(&format!("SELECT COUNT(*) AS n FROM {table} WHERE name = ?"))
            .bind(new)
            .fetch_one(&mut **tx)
            .await?
            .try_get("n")?;
        if taken > 0 {
            return Ok(RenameOutcome::Taken);
        }
    }
    let moved = sqlx::query(&format!(
        "UPDATE {table} SET name = ?, updated_at = ? WHERE name = ?"
    ))
    .bind(new)
    .bind(now_rfc3339())
    .bind(old)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    Ok(if moved > 0 {
        RenameOutcome::Renamed
    } else {
        RenameOutcome::NotFound
    })
}

pub async fn delete_backend(db: &Pool, name: &str) -> Result<(), DbError> {
    sqlx::query("DELETE FROM backends WHERE name = ?")
        .bind(name)
        .execute(db)
        .await?;
    Ok(())
}

async fn replace_backend_models(
    db: &Pool,
    backend_name: &str,
    models: &[String],
) -> Result<(), DbError> {
    sqlx::query(
        "DELETE FROM backend_models WHERE backend_id = (SELECT id FROM backends WHERE name = ?)",
    )
    .bind(backend_name)
    .execute(db)
    .await?;
    for (i, model_id) in models.iter().enumerate() {
        sqlx::query(
            r#"INSERT INTO backend_models (backend_id, model_id, sort_order)
               SELECT id, ?, ? FROM backends WHERE name = ?"#,
        )
        .bind(model_id)
        .bind(i as i64)
        .bind(backend_name)
        .execute(db)
        .await?;
    }
    Ok(())
}

async fn replace_backend_aliases(
    db: &Pool,
    backend_name: &str,
    aliases: &[AliasRow],
) -> Result<(), DbError> {
    sqlx::query(
        "DELETE FROM backend_aliases WHERE backend_id = (SELECT id FROM backends WHERE name = ?)",
    )
    .bind(backend_name)
    .execute(db)
    .await?;
    for a in aliases {
        sqlx::query(
            r#"INSERT INTO backend_aliases (backend_id, alias, target)
               SELECT id, ?, ? FROM backends WHERE name = ?"#,
        )
        .bind(&a.alias)
        .bind(&a.target)
        .bind(backend_name)
        .execute(db)
        .await?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Pool CRUD
// ---------------------------------------------------------------------------

/// Insert or update a pool, replacing backends/models/voices atomically.
pub async fn upsert_pool(db: &Pool, row: &PoolRow) -> Result<(), DbError> {
    let now = now_rfc3339();
    sqlx::query(
        r#"INSERT INTO pools
               (name, kind, strategy, fallback_offline, compliance_gdpr,
                compliance_nda, enforce_limits, sort_order, allowed_groups,
                created_at, updated_at)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
           ON CONFLICT(name) DO UPDATE SET
               kind             = excluded.kind,
               strategy         = excluded.strategy,
               fallback_offline = excluded.fallback_offline,
               compliance_gdpr  = excluded.compliance_gdpr,
               compliance_nda   = excluded.compliance_nda,
               enforce_limits   = excluded.enforce_limits,
               sort_order       = excluded.sort_order,
               allowed_groups   = excluded.allowed_groups,
               updated_at       = excluded.updated_at"#,
    )
    .bind(&row.name)
    .bind(&row.kind)
    .bind(&row.strategy)
    .bind(&row.fallback_offline)
    .bind(row.compliance_gdpr as i64)
    .bind(row.compliance_nda as i64)
    .bind(row.enforce_limits as i64)
    .bind(row.sort_order)
    .bind(serde_json::to_string(&row.allowed_groups).unwrap_or_else(|_| "[]".to_string()))
    .bind(&now)
    .bind(&now)
    .execute(db)
    .await?;

    replace_pool_backends(db, &row.name, &row.backends).await?;
    replace_pool_models(db, &row.name, &row.models).await?;
    replace_pool_voices(db, &row.name, &row.voices).await?;
    replace_pool_offer_voices(db, &row.name, &row.offer_voices).await?;
    Ok(())
}

/// Delete a pool and all its dependent rows.
pub async fn delete_pool(db: &Pool, name: &str) -> Result<(), DbError> {
    sqlx::query("DELETE FROM pools WHERE name = ?")
        .bind(name)
        .execute(db)
        .await?;
    Ok(())
}

/// Set a backend's pool membership to *exactly* `pool` (or none). Backs the
/// single "Pool" select on the backend editor, which trades the DB's
/// many-to-many capability for a simpler UI: a backend in several pools
/// collapses to the one selected here. Pool-side editing (the pool's backend
/// checkboxes) still supports multi-pool membership.
pub async fn set_backend_pool(
    db: &Pool,
    backend_name: &str,
    pool: Option<&str>,
) -> Result<(), DbError> {
    if let Some(pool_name) = pool {
        let pool_id = sqlx::query_scalar::<_, i64>("SELECT id FROM pools WHERE name = ?")
            .bind(pool_name)
            .fetch_optional(db)
            .await?
            .ok_or(DbError::Query(sqlx::Error::RowNotFound))?;
        let linked = sqlx::query_scalar::<_, bool>(
            r#"SELECT EXISTS(
                   SELECT 1 FROM pool_backends
                    WHERE pool_id = ?
                      AND backend_id = (SELECT id FROM backends WHERE name = ?)
               )"#,
        )
        .bind(pool_id)
        .bind(backend_name)
        .fetch_one(db)
        .await?;
        sqlx::query(
            "DELETE FROM pool_backends WHERE backend_id = (SELECT id FROM backends WHERE name = ?) AND pool_id != ?",
        )
        .bind(backend_name)
        .bind(pool_id)
        .execute(db)
        .await?;
        if !linked {
            let inserted = sqlx::query(
                r#"INSERT INTO pool_backends (pool_id, backend_id, sort_order)
               SELECT p.id, b.id, COALESCE(
                   (SELECT MAX(sort_order) + 1 FROM pool_backends WHERE pool_id = p.id), 0)
                 FROM pools p, backends b
                WHERE p.name = ? AND b.name = ?"#,
            )
            .bind(pool_name)
            .bind(backend_name)
            .execute(db)
            .await?
            .rows_affected();
            if inserted == 0 {
                return Err(DbError::Query(sqlx::Error::RowNotFound));
            }
        }
    } else {
        sqlx::query(
            "DELETE FROM pool_backends WHERE backend_id = (SELECT id FROM backends WHERE name = ?)",
        )
        .bind(backend_name)
        .execute(db)
        .await?;
    }
    Ok(())
}

async fn replace_pool_backends(
    db: &Pool,
    pool_name: &str,
    backend_names: &[String],
) -> Result<(), DbError> {
    sqlx::query("DELETE FROM pool_backends WHERE pool_id = (SELECT id FROM pools WHERE name = ?)")
        .bind(pool_name)
        .execute(db)
        .await?;
    for (i, name) in backend_names.iter().enumerate() {
        sqlx::query(
            r#"INSERT INTO pool_backends (pool_id, backend_id, sort_order)
               SELECT p.id, b.id, ? FROM pools p, backends b
                WHERE p.name = ? AND b.name = ?"#,
        )
        .bind(i as i64)
        .bind(pool_name)
        .bind(name)
        .execute(db)
        .await?;
    }
    Ok(())
}

async fn replace_pool_models(db: &Pool, pool_name: &str, models: &[String]) -> Result<(), DbError> {
    sqlx::query("DELETE FROM pool_models WHERE pool_id = (SELECT id FROM pools WHERE name = ?)")
        .bind(pool_name)
        .execute(db)
        .await?;
    for (i, model_id) in models.iter().enumerate() {
        sqlx::query(
            r#"INSERT INTO pool_models (pool_id, model_id, sort_order)
               SELECT id, ?, ? FROM pools WHERE name = ?"#,
        )
        .bind(model_id)
        .bind(i as i64)
        .bind(pool_name)
        .execute(db)
        .await?;
    }
    Ok(())
}

/// Replace a pool's language→voice map.
///
/// `ON CONFLICT … DO NOTHING` rather than a bare INSERT: the table is keyed by
/// `(pool_id, lang_code)`, and a caller handing over two entries for the same
/// language used to abort the entire pool save with a raw SQLite 1555 — a
/// constraint error surfaced at the admin form as an internal failure, for input
/// whose meaning is unambiguous (the first entry is the one resolution would
/// ever have used). Callers that care about the difference de-duplicate before
/// getting here; this makes it impossible to fail on.
async fn replace_pool_voices(
    db: &Pool,
    pool_name: &str,
    voices: &[VoiceRow],
) -> Result<(), DbError> {
    sqlx::query("DELETE FROM pool_voices WHERE pool_id = (SELECT id FROM pools WHERE name = ?)")
        .bind(pool_name)
        .execute(db)
        .await?;
    for v in voices {
        sqlx::query(
            r#"INSERT INTO pool_voices (pool_id, lang_code, voice_id)
               SELECT id, ?, ? FROM pools WHERE name = ?
               ON CONFLICT(pool_id, lang_code) DO NOTHING"#,
        )
        .bind(&v.lang_code)
        .bind(&v.voice_id)
        .bind(pool_name)
        .execute(db)
        .await?;
    }
    Ok(())
}

/// Replace a pool's selectable-voice menu. Same reasoning as
/// [`replace_pool_voices`]: keyed by `(pool_id, voice_id)`, so a repeated
/// voice keeps its first position instead of failing the save.
async fn replace_pool_offer_voices(
    db: &Pool,
    pool_name: &str,
    voices: &[String],
) -> Result<(), DbError> {
    sqlx::query(
        "DELETE FROM pool_offer_voices WHERE pool_id = (SELECT id FROM pools WHERE name = ?)",
    )
    .bind(pool_name)
    .execute(db)
    .await?;
    // Position carries the menu order, so the operator's first line stays first.
    for (i, voice_id) in voices.iter().enumerate() {
        sqlx::query(
            r#"INSERT INTO pool_offer_voices (pool_id, voice_id, sort_order)
               SELECT id, ?, ? FROM pools WHERE name = ?
               ON CONFLICT(pool_id, voice_id) DO NOTHING"#,
        )
        .bind(voice_id)
        .bind(i as i64)
        .bind(pool_name)
        .execute(db)
        .await?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Fallback models CRUD
// ---------------------------------------------------------------------------

/// Set or clear (with `None`) the unknown-model fallback for a kind.
pub async fn set_fallback(db: &Pool, kind: &str, model_id: Option<&str>) -> Result<(), DbError> {
    match model_id {
        Some(id) => {
            sqlx::query(
                r#"INSERT INTO fallback_models (kind, model_id)
                   VALUES (?, ?)
                   ON CONFLICT(kind) DO UPDATE SET model_id = excluded.model_id"#,
            )
            .bind(kind)
            .bind(id)
            .execute(db)
            .await?;
        }
        None => {
            sqlx::query("DELETE FROM fallback_models WHERE kind = ?")
                .bind(kind)
                .execute(db)
                .await?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::db;

    async fn test_pool() -> Pool {
        db::open(std::path::Path::new(":memory:")).await.unwrap()
    }

    #[tokio::test]
    async fn empty_snapshot_when_no_rows() {
        let pool = test_pool().await;
        let snap = load_snapshot(&pool).await.unwrap();
        assert!(snap.pools.is_empty());
        assert!(snap.backends.is_empty());
        assert!(snap.fallbacks.is_empty());
        assert!(is_empty(&pool).await.unwrap());
    }

    /// A backend with nothing else set, for the detection tests.
    fn bare_backend(name: &str) -> BackendRow {
        BackendRow {
            name: name.into(),
            base_url: "http://host:11434/v1".into(),
            api_key_env: None,
            api_key_ct: None,
            api_key_nonce: None,
            weight: 1,
            max_inflight: 16,
            health_path: "/models".into(),
            probe_models: true,
            supports_edit: false,
            enabled: true,
            models: Vec::new(),
            aliases: Vec::new(),
            created_at: Timestamp::now(),
            updated_at: Timestamp::now(),
        }
    }

    /// Detection results have to survive a restart: a gateway that boots while
    /// a backend is down would otherwise read it back as `generic` with no
    /// context, and every model on it would silently fall to the global 32768
    /// guess — the failure profiles exist to remove.
    #[tokio::test]
    async fn detected_profile_round_trips() {
        use crate::server::upstreams::profile::{BackendProfile, Detected};

        let pool = test_pool().await;
        upsert_backend(&pool, &bare_backend("llama")).await.unwrap();

        let detected = Detected {
            profile: BackendProfile::LlamaCpp,
            context_windows: HashMap::from([
                ("gemma".to_string(), 131_072),
                ("qwen".to_string(), 32_768),
            ]),
            context_cap: Some(8_192),
            max_parallel: Some(4),
            version: Some("b1234".into()),
            detected_at: None,
        };
        save_detected(&pool, "llama", &detected).await.unwrap();

        // `detected_at` is stamped on the way in, so compare the rest and
        // assert only that a stamp came back.
        let loaded = load_detected(&pool).await.unwrap();
        let back = loaded.get("llama").expect("row was written");
        assert!(back.detected_at.is_some(), "the write must stamp a time");
        assert_eq!(
            Detected {
                detected_at: None,
                ..back.clone()
            },
            detected
        );
    }

    /// Re-detecting replaces the record rather than merging into it: a server
    /// that was reconfigured has to be able to *shrink* a window, and a
    /// backend repointed at a different kind of server must not keep the old
    /// one's models.
    #[tokio::test]
    async fn re_detecting_replaces_the_previous_record() {
        use crate::server::upstreams::profile::{BackendProfile, Detected};

        let pool = test_pool().await;
        upsert_backend(&pool, &bare_backend("b")).await.unwrap();

        save_detected(
            &pool,
            "b",
            &Detected {
                profile: BackendProfile::LlamaCpp,
                context_windows: HashMap::from([("old".to_string(), 131_072)]),
                context_cap: Some(65_536),
                max_parallel: Some(8),
                version: Some("b1".into()),
                detected_at: None,
            },
        )
        .await
        .unwrap();

        let now = Detected {
            profile: BackendProfile::Ollama,
            context_windows: HashMap::new(),
            context_cap: None,
            max_parallel: None,
            version: Some("0.13.3".into()),
            detected_at: None,
        };
        save_detected(&pool, "b", &now).await.unwrap();

        let loaded = load_detected(&pool).await.unwrap();
        let back = loaded.get("b").expect("row was written");
        assert_eq!(
            Detected {
                detected_at: None,
                ..back.clone()
            },
            now
        );
    }

    #[tokio::test]
    async fn backend_round_trip() {
        let pool = test_pool().await;
        let backend = BackendRow {
            name: "gpu-01".into(),
            base_url: "http://gpu-01:8000/v1".into(),
            api_key_env: Some("GPU01_KEY".into()),
            api_key_ct: None,
            api_key_nonce: None,
            weight: 2,
            max_inflight: 32,
            health_path: "/v1/models".into(),
            probe_models: true,
            supports_edit: false,
            enabled: true,
            models: vec!["qwen-32b".into(), "qwen-7b".into()],
            aliases: vec![
                AliasRow {
                    alias: "fast".into(),
                    target: Some("qwen-7b".into()),
                },
                AliasRow {
                    alias: "smart".into(),
                    target: Some("qwen-32b".into()),
                },
            ],
            created_at: Timestamp::now(),
            updated_at: Timestamp::now(),
        };
        upsert_backend(&pool, &backend).await.unwrap();

        let loaded = get_backend(&pool, "gpu-01").await.unwrap().unwrap();
        assert_eq!(loaded.base_url, "http://gpu-01:8000/v1");
        assert_eq!(loaded.weight, 2);
        assert_eq!(loaded.max_inflight, 32);
        assert_eq!(loaded.models, vec!["qwen-32b", "qwen-7b"]);
        assert_eq!(loaded.aliases.len(), 2);
        // is_empty tracks pools, not backends: inserting a backend alone leaves
        // the topology "empty" (no pool → nothing routable yet).
        assert!(is_empty(&pool).await.unwrap());

        // Update — change weight, drop a model.
        let mut updated = backend.clone();
        updated.weight = 5;
        updated.models = vec!["qwen-32b".into()];
        upsert_backend(&pool, &updated).await.unwrap();
        let reloaded = get_backend(&pool, "gpu-01").await.unwrap().unwrap();
        assert_eq!(reloaded.weight, 5);
        assert_eq!(reloaded.models, vec!["qwen-32b"]);
    }

    /// No foreign key points at a *name*.
    ///
    /// This is the invariant that makes a rename one `UPDATE`: if a child table
    /// referenced `backends(name)` or `pools(name)`, renaming would have to move
    /// it too, and the old code kept a hand-maintained list of exactly that for
    /// a reader to forget. Read out of the live schema, so a child table added
    /// against the name is a failing test rather than a bug found on a rename.
    #[tokio::test]
    async fn nothing_references_a_topology_row_by_name() {
        let pool = test_pool().await;
        let rows = sqlx::query(
            r#"SELECT m.name AS child, f."from" AS col, f."table" AS parent, f."to" AS target
               FROM sqlite_master m JOIN pragma_foreign_key_list(m.name) f
               WHERE m.type = 'table' AND f."table" IN ('backends', 'pools')"#,
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert!(!rows.is_empty(), "expected foreign keys into the topology");
        let by_name: Vec<String> = rows
            .iter()
            .filter(|r| r.try_get::<Option<String>, _>("target").unwrap().as_deref() != Some("id"))
            .map(|r| {
                format!(
                    "{}.{} -> {}",
                    r.try_get::<String, _>("child").unwrap(),
                    r.try_get::<String, _>("col").unwrap(),
                    r.try_get::<String, _>("parent").unwrap(),
                )
            })
            .collect();
        assert!(
            by_name.is_empty(),
            "these reference a topology row by something other than its id, so a rename \
             would have to move them: {by_name:?}"
        );
    }

    #[tokio::test]
    async fn renaming_a_backend_moves_every_row_that_names_it() {
        let pool = test_pool().await;
        let backend = BackendRow {
            name: "voxtral".into(),
            base_url: "http://llm01:8002/v1".into(),
            api_key_env: None,
            api_key_ct: None,
            api_key_nonce: None,
            weight: 1,
            max_inflight: 16,
            health_path: "/models".into(),
            probe_models: true,
            supports_edit: false,
            enabled: true,
            models: vec!["voxtral-small".into()],
            aliases: vec![AliasRow {
                alias: "asr".into(),
                target: Some("voxtral-small".into()),
            }],
            created_at: Timestamp::now(),
            updated_at: Timestamp::now(),
        };
        upsert_backend(&pool, &backend).await.unwrap();
        upsert_pool(
            &pool,
            &PoolRow {
                name: "transcribe".into(),
                kind: "transcription".into(),
                strategy: "least_inflight".into(),
                fallback_offline: None,
                compliance_gdpr: true,
                compliance_nda: true,
                enforce_limits: true,
                sort_order: 0,
                backends: vec!["voxtral".into()],
                models: vec![],
                voices: vec![],
                offer_voices: vec![],
                allowed_groups: vec![],
                created_at: Timestamp::now(),
                updated_at: Timestamp::now(),
            },
        )
        .await
        .unwrap();
        save_probed_models(
            &pool,
            "voxtral",
            &HashSet::from(["voxtral-small".to_string()]),
        )
        .await
        .unwrap();

        assert_eq!(
            rename_backend(&pool, "voxtral", "qwen-asr").await.unwrap(),
            RenameOutcome::Renamed
        );

        // The row itself, and every child that named it.
        assert!(get_backend(&pool, "voxtral").await.unwrap().is_none());
        let moved = get_backend(&pool, "qwen-asr").await.unwrap().unwrap();
        assert_eq!(moved.base_url, "http://llm01:8002/v1");
        assert_eq!(moved.models, vec!["voxtral-small"]);
        assert_eq!(moved.aliases.len(), 1);
        let snap = load_snapshot(&pool).await.unwrap();
        let renamed_pool = snap.pools.iter().find(|p| p.name == "transcribe").unwrap();
        assert_eq!(renamed_pool.backends, vec!["qwen-asr".to_string()]);
        assert!(
            load_probed_models(&pool)
                .await
                .unwrap()
                .contains_key("qwen-asr")
        );

        // No orphans anywhere: the deferred check would have refused the
        // commit, but assert it directly so a future `PRAGMA` slip is loud.
        let violations = sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert!(violations.is_empty(), "rename left dangling references");
    }

    #[tokio::test]
    async fn renaming_onto_a_name_with_usage_history_merges_rather_than_collides() {
        // `usage_daily`'s primary key includes `backend`, and its rows outlive
        // the backend they name. Rename onto a name that once had traffic on
        // the same day/model and a blind UPDATE violates the key, aborting the
        // whole rename with a 500.
        let pool = test_pool().await;
        for name in ["new-gpu"] {
            upsert_backend(
                &pool,
                &BackendRow {
                    name: name.into(),
                    base_url: "http://x/v1".into(),
                    api_key_env: None,
                    api_key_ct: None,
                    api_key_nonce: None,
                    weight: 1,
                    max_inflight: 16,
                    health_path: "/models".into(),
                    probe_models: true,
                    supports_edit: false,
                    enabled: true,
                    models: vec![],
                    aliases: vec![],
                    created_at: Timestamp::now(),
                    updated_at: Timestamp::now(),
                },
            )
            .await
            .unwrap();
        }
        // `old-gpu` is gone as a backend but its accounting rows remain — that
        // is the whole point of leaving `usage_daily` without a foreign key,
        // and it is what makes the name reusable while the history is not.
        for (backend, reqs) in [("old-gpu", 3_i64), ("new-gpu", 4_i64)] {
            sqlx::query(
                r#"INSERT INTO usage_daily
                       (day, user_id, token_id, source, kind, backend, model, req_count, total_tokens)
                   VALUES ('2026-09-14', 'u1', '', 'chat', 'chat', ?, 'qwen', ?, ?)"#,
            )
            .bind(backend)
            .bind(reqs)
            .bind(reqs * 10)
            .execute(&pool)
            .await
            .unwrap();
        }

        assert_eq!(
            rename_backend(&pool, "new-gpu", "old-gpu").await.unwrap(),
            RenameOutcome::Renamed
        );

        let rows: Vec<(String, i64, i64)> = sqlx::query(
            "SELECT backend, req_count, total_tokens FROM usage_daily ORDER BY backend",
        )
        .fetch_all(&pool)
        .await
        .unwrap()
        .iter()
        .map(|r| {
            (
                r.try_get::<String, _>("backend").unwrap(),
                r.try_get("req_count").unwrap(),
                r.try_get("total_tokens").unwrap(),
            )
        })
        .collect();
        assert_eq!(
            rows,
            vec![("old-gpu".to_string(), 7, 70)],
            "the two days' counters must be summed into one row, not lost or duplicated"
        );
    }

    #[tokio::test]
    async fn a_rename_refuses_a_name_already_in_use_and_an_unknown_row() {
        let pool = test_pool().await;
        for name in ["a", "b"] {
            upsert_backend(
                &pool,
                &BackendRow {
                    name: name.into(),
                    base_url: "http://x/v1".into(),
                    api_key_env: None,
                    api_key_ct: None,
                    api_key_nonce: None,
                    weight: 1,
                    max_inflight: 16,
                    health_path: "/models".into(),
                    probe_models: true,
                    supports_edit: false,
                    enabled: true,
                    models: vec![],
                    aliases: vec![],
                    created_at: Timestamp::now(),
                    updated_at: Timestamp::now(),
                },
            )
            .await
            .unwrap();
        }
        // Taking an existing name would silently merge two backends.
        assert_eq!(
            rename_backend(&pool, "a", "b").await.unwrap(),
            RenameOutcome::Taken
        );
        assert!(get_backend(&pool, "a").await.unwrap().is_some());
        assert_eq!(
            rename_backend(&pool, "ghost", "c").await.unwrap(),
            RenameOutcome::NotFound
        );
        // Renaming to itself is a no-op that still reports the row exists.
        assert_eq!(
            rename_backend(&pool, "a", "a").await.unwrap(),
            RenameOutcome::Renamed
        );
    }

    #[tokio::test]
    async fn backend_delete_cascades() {
        let pool = test_pool().await;
        let backend = BackendRow {
            name: "tmp".into(),
            base_url: "http://tmp".into(),
            api_key_env: None,
            api_key_ct: None,
            api_key_nonce: None,
            weight: 1,
            max_inflight: 16,
            health_path: "/models".into(),
            probe_models: true,
            supports_edit: false,
            enabled: true,
            models: vec!["m1".into()],
            aliases: vec![AliasRow {
                alias: "a1".into(),
                target: None,
            }],
            created_at: Timestamp::now(),
            updated_at: Timestamp::now(),
        };
        upsert_backend(&pool, &backend).await.unwrap();
        assert!(get_backend(&pool, "tmp").await.unwrap().is_some());

        delete_backend(&pool, "tmp").await.unwrap();
        assert!(get_backend(&pool, "tmp").await.unwrap().is_none());

        // Dependent rows should be gone.
        let model_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM backend_models")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(model_count, 0);
    }

    #[tokio::test]
    async fn pool_round_trip() {
        let pool = test_pool().await;

        let backend = BackendRow {
            name: "b1".into(),
            base_url: "http://b1".into(),
            api_key_env: None,
            api_key_ct: None,
            api_key_nonce: None,
            weight: 1,
            max_inflight: 16,
            health_path: "/models".into(),
            probe_models: true,
            supports_edit: false,
            enabled: true,
            models: vec![],
            aliases: vec![],
            created_at: Timestamp::now(),
            updated_at: Timestamp::now(),
        };
        upsert_backend(&pool, &backend).await.unwrap();
        let mut second_backend = backend.clone();
        second_backend.name = "b2".into();
        second_backend.base_url = "http://b2".into();
        upsert_backend(&pool, &second_backend).await.unwrap();

        let pool_row = PoolRow {
            name: "chat-pool".into(),
            kind: "chat".into(),
            strategy: "round_robin".into(),
            fallback_offline: Some("backup-model".into()),
            compliance_gdpr: false,
            compliance_nda: true,
            enforce_limits: true,
            sort_order: 0,
            allowed_groups: Vec::new(),
            backends: vec!["b1".into()],
            models: vec!["pool-fallback-model".into()],
            voices: vec![],
            offer_voices: Vec::new(),
            created_at: Timestamp::now(),
            updated_at: Timestamp::now(),
        };
        upsert_pool(&pool, &pool_row).await.unwrap();

        let snap = load_snapshot(&pool).await.unwrap();
        assert_eq!(snap.pools.len(), 1);
        let p = &snap.pools[0];
        assert_eq!(p.name, "chat-pool");
        assert_eq!(p.kind, "chat");
        assert_eq!(p.strategy, "round_robin");
        assert!(!p.compliance_gdpr);
        assert!(p.compliance_nda);
        assert_eq!(p.backends, vec!["b1"]);
        assert_eq!(p.models, vec!["pool-fallback-model"]);
        assert!(!is_empty(&pool).await.unwrap());

        set_backend_pool(&pool, "b2", Some("chat-pool"))
            .await
            .unwrap();
        set_backend_pool(&pool, "b1", Some("chat-pool"))
            .await
            .unwrap();
        let snap = load_snapshot(&pool).await.unwrap();
        assert_eq!(snap.pools[0].backends, ["b1", "b2"]);
    }

    #[tokio::test]
    async fn pool_with_voices_round_trip() {
        let pool = test_pool().await;
        upsert_backend(
            &pool,
            &BackendRow {
                name: "tts".into(),
                base_url: "http://tts".into(),
                api_key_env: None,
                api_key_ct: None,
                api_key_nonce: None,
                weight: 1,
                max_inflight: 16,
                health_path: "/models".into(),
                probe_models: true,
                supports_edit: false,
                enabled: true,
                models: vec![],
                aliases: vec![],
                created_at: Timestamp::now(),
                updated_at: Timestamp::now(),
            },
        )
        .await
        .unwrap();

        let pool_row = PoolRow {
            name: "voice".into(),
            kind: "speech".into(),
            strategy: "least_inflight".into(),
            fallback_offline: None,
            compliance_gdpr: true,
            compliance_nda: true,
            enforce_limits: true,
            sort_order: 0,
            allowed_groups: Vec::new(),
            backends: vec!["tts".into()],
            models: vec![],
            voices: vec![
                VoiceRow {
                    lang_code: "de".into(),
                    voice_id: "de-voice".into(),
                },
                VoiceRow {
                    lang_code: "".into(),
                    voice_id: "default-voice".into(),
                },
            ],
            // The menu is its own list and keeps the operator's order — three
            // German voices are expressible here and not in `voices` above.
            offer_voices: vec!["default-voice".into(), "de-voice".into(), "extra".into()],
            created_at: Timestamp::now(),
            updated_at: Timestamp::now(),
        };
        upsert_pool(&pool, &pool_row).await.unwrap();

        let snap = load_snapshot(&pool).await.unwrap();
        let p = &snap.pools[0];
        assert_eq!(p.voices.len(), 2);
        assert_eq!(
            p.voices
                .iter()
                .find(|v| v.lang_code == "de")
                .unwrap()
                .voice_id,
            "de-voice"
        );
        assert_eq!(
            p.voices
                .iter()
                .find(|v| v.lang_code.is_empty())
                .unwrap()
                .voice_id,
            "default-voice"
        );
        // The offer list survives in the operator's order (it is read back by
        // `sort_order`, not alphabetically) — that order is the picker's.
        assert_eq!(p.offer_voices, ["default-voice", "de-voice", "extra"]);

        // Regression: a duplicate in either list must not abort the save with a
        // raw SQLite 1555. Both tables are keyed by their own second column, and
        // upsert_pool is the admin form's write path — a constraint error there
        // surfaces as "could not save the pool" for input whose meaning is
        // obvious. First entry wins, rest ignored.
        let mut dup = pool_row.clone();
        dup.voices = vec![
            VoiceRow {
                lang_code: String::new(),
                voice_id: "alloy".into(),
            },
            VoiceRow {
                lang_code: String::new(),
                voice_id: "marin".into(),
            },
        ];
        dup.offer_voices = vec!["marin".into(), "marin".into(), "cedar".into()];
        upsert_pool(&pool, &dup)
            .await
            .expect("duplicates tolerated");

        let snap = load_snapshot(&pool).await.unwrap();
        let p = &snap.pools[0];
        assert_eq!(p.voices.len(), 1);
        assert_eq!(p.voices[0].voice_id, "alloy");
        assert_eq!(p.offer_voices, ["marin", "cedar"]);
    }

    #[tokio::test]
    async fn fallback_set_and_clear() {
        let pool = test_pool().await;
        set_fallback(&pool, "chat", Some("qwen")).await.unwrap();
        set_fallback(&pool, "embedding", Some("text-embed"))
            .await
            .unwrap();

        let snap = load_snapshot(&pool).await.unwrap();
        assert_eq!(snap.fallbacks.get("chat").unwrap(), "qwen");
        assert_eq!(snap.fallbacks.get("embedding").unwrap(), "text-embed");

        // Clear one.
        set_fallback(&pool, "chat", None).await.unwrap();
        let snap = load_snapshot(&pool).await.unwrap();
        assert!(!snap.fallbacks.contains_key("chat"));
        assert!(snap.fallbacks.contains_key("embedding"));
    }

    #[tokio::test]
    async fn pool_delete_cascades() {
        let pool = test_pool().await;
        upsert_backend(
            &pool,
            &BackendRow {
                name: "b".into(),
                base_url: "http://b".into(),
                api_key_env: None,
                api_key_ct: None,
                api_key_nonce: None,
                weight: 1,
                max_inflight: 16,
                health_path: "/models".into(),
                probe_models: true,
                supports_edit: false,
                enabled: true,
                models: vec![],
                aliases: vec![],
                created_at: Timestamp::now(),
                updated_at: Timestamp::now(),
            },
        )
        .await
        .unwrap();
        upsert_pool(
            &pool,
            &PoolRow {
                name: "p".into(),
                kind: "chat".into(),
                strategy: "least_inflight".into(),
                fallback_offline: None,
                compliance_gdpr: true,
                compliance_nda: true,
                enforce_limits: true,
                sort_order: 0,
                allowed_groups: Vec::new(),
                backends: vec!["b".into()],
                models: vec!["m".into()],
                voices: vec![VoiceRow {
                    lang_code: "en".into(),
                    voice_id: "v".into(),
                }],
                offer_voices: Vec::new(),
                created_at: Timestamp::now(),
                updated_at: Timestamp::now(),
            },
        )
        .await
        .unwrap();

        delete_pool(&pool, "p").await.unwrap();
        assert!(is_empty(&pool).await.unwrap());

        // Backend should still exist (delete pool ≠ delete backend).
        assert!(get_backend(&pool, "b").await.unwrap().is_some());

        // Pool-dependent rows gone.
        let pb_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pool_backends")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(pb_count, 0);
    }
}
