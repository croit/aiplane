// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Migration 0095 (`0095_openrouter_optional_ceilings.sql`) rebuilds the
//! OpenRouter policy table so a ceiling can be absent, and gives each policy
//! the unit its prices are billed in (per 1M tokens or per audio second).
//!
//! The rebuild copies every row, so the tests here seed a database as the
//! previous release left it, boot it through the real
//! [`aiplane_core::server::db::open`] path, and check that the saved policy
//! survived unchanged and that empty ceilings are now storable.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use aiplane_core::server::db::open;
use aiplane_core::server::db::upstreams_config::{self, BackendRow, OpenRouterPolicyRow};
use aiplane_core::server::upstreams::config::ProviderKind;
use aiplane_core::server::upstreams::openrouter::PriceUnit;
use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};

const LAST_VERSION_BEFORE: i64 = 94;

struct TempDb(PathBuf);

impl TempDb {
    fn new() -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the clock is after the epoch")
            .as_nanos();
        Self(std::env::temp_dir().join(format!(
            "aiplane-0095-{}-{stamp}.sqlite",
            std::process::id()
        )))
    }
}

impl Drop for TempDb {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", self.0.display()));
        }
    }
}

fn migrations_up_to(version: i64) -> Migrator {
    let mut migrator = sqlx::migrate!("./migrations");
    migrator.migrations = Cow::Owned(
        migrator
            .migrations
            .iter()
            .filter(|m| m.version <= version)
            .cloned()
            .collect(),
    );
    migrator
}

async fn file_pool(path: &Path) -> SqlitePool {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .foreign_keys(false);
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .expect("the temporary database opens")
}

fn backend(name: &str) -> BackendRow {
    BackendRow {
        provider: ProviderKind::OpenRouter,
        priority: Default::default(),
        name: name.into(),
        base_url: "https://openrouter.ai/api/v1".into(),
        api_key_env: None,
        api_key_ct: None,
        api_key_nonce: None,
        weight: 1,
        max_inflight: 16,
        health_path: "/models".into(),
        supports_edit: false,
        enabled: true,
        extra_headers: Default::default(),
        models: Vec::new(),
        aliases: Vec::new(),
        created_at: jiff::Timestamp::now(),
        updated_at: jiff::Timestamp::now(),
    }
}

#[tokio::test]
async fn a_saved_policy_survives_the_rebuild_and_keeps_its_ceiling() {
    let file = TempDb::new();
    let before = file_pool(&file.0).await;
    migrations_up_to(LAST_VERSION_BEFORE)
        .run(&before)
        .await
        .expect("the pre-0095 schema applies");
    upstreams_config::upsert_backend(&before, &backend("or"))
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO openrouter_model_policies
           (backend_id, model_id, max_prompt_price, max_completion_price, strategy,
            available, created_at, updated_at)
         VALUES ((SELECT id FROM backends WHERE name = 'or'), 'deepseek/deepseek-v4.1',
                 0.30, 1.20, 'cheapest', 1, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
    )
    .execute(&before)
    .await
    .unwrap();
    before.close().await;

    let pool = open(&file.0).await.expect("the migration boots");
    let policies = upstreams_config::load_openrouter_policies(&pool, "or")
        .await
        .unwrap();
    let policy = &policies["deepseek/deepseek-v4.1"];
    assert_eq!(policy.max_prompt_price, Some(0.30));
    assert_eq!(policy.max_completion_price, Some(1.20));
    assert_eq!(policy.strategy, "cheapest");
    assert_eq!(
        policy.price_unit,
        PriceUnit::Token,
        "an existing row was token-priced"
    );

    let columns: Vec<String> =
        sqlx::query("SELECT name FROM pragma_table_info('openrouter_model_policies')")
            .fetch_all(&pool)
            .await
            .unwrap()
            .into_iter()
            .map(|row| row.get("name"))
            .collect();
    assert!(columns.iter().any(|column| column == "price_unit"));
    let not_null: i64 = sqlx::query_scalar(
        "SELECT \"notnull\" FROM pragma_table_info('openrouter_model_policies') WHERE name = 'max_prompt_price'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(not_null, 0, "an empty ceiling is allowed");
}

#[tokio::test]
async fn a_duration_priced_policy_can_have_no_ceiling_at_all() {
    let file = TempDb::new();
    let pool = open(&file.0).await.unwrap();
    upstreams_config::upsert_backend(&pool, &backend("or"))
        .await
        .unwrap();

    let policy = OpenRouterPolicyRow {
        model_id: "openai/whisper-large-v3-turbo".into(),
        max_prompt_price: None,
        max_completion_price: None,
        price_unit: PriceUnit::Second,
        strategy: "cheapest".into(),
        min_throughput: None,
        available: true,
        cheapest_prompt_price: Some(0.00000333),
        cheapest_completion_price: Some(0.0),
        cheapest_provider: Some("DeepInfra".into()),
        observed_at: None,
    };
    upstreams_config::upsert_openrouter_policy(&pool, "or", &policy)
        .await
        .unwrap();

    let policies = upstreams_config::load_openrouter_policies(&pool, "or")
        .await
        .unwrap();
    assert_eq!(policies["openai/whisper-large-v3-turbo"], policy);
}
