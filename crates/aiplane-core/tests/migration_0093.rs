// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use aiplane_core::server::db::open;
use aiplane_core::server::db::upstreams_config::{self, BackendRow, OpenRouterPolicyRow};
use aiplane_core::server::upstreams::config::ProviderKind;
use sqlx::Row;
use std::path::Path;

fn backend(name: &str, provider: ProviderKind) -> BackendRow {
    BackendRow {
        provider,
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
async fn existing_backends_default_to_the_openai_compatible_provider() {
    let pool = open(Path::new(":memory:")).await.unwrap();
    upstreams_config::upsert_backend(&pool, &backend("plain", ProviderKind::OpenAiCompatible))
        .await
        .unwrap();
    let provider: String = sqlx::query_scalar("SELECT provider FROM backends WHERE name = 'plain'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(provider, "openai_compatible");
}

#[tokio::test]
async fn the_provider_round_trips_and_drives_the_openrouter_catalog() {
    let pool = open(Path::new(":memory:")).await.unwrap();
    upstreams_config::upsert_backend(&pool, &backend("or", ProviderKind::OpenRouter))
        .await
        .unwrap();
    let loaded = upstreams_config::get_backend(&pool, "or")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.provider, ProviderKind::OpenRouter);

    let policy = OpenRouterPolicyRow {
        model_id: "deepseek/deepseek-v4.1".into(),
        max_prompt_price: Some(0.30),
        max_completion_price: Some(1.20),
        price_unit: aiplane_core::server::upstreams::openrouter::PriceUnit::Token,
        strategy: "cheapest".into(),
        min_throughput: Some(40.0),
        available: true,
        cheapest_prompt_price: Some(0.27),
        cheapest_completion_price: Some(1.10),
        cheapest_provider: Some("deepseek".into()),
        observed_at: None,
    };
    upstreams_config::upsert_openrouter_policy(&pool, "or", &policy)
        .await
        .unwrap();
    let policies = upstreams_config::load_openrouter_policies(&pool, "or")
        .await
        .unwrap();
    assert_eq!(policies["deepseek/deepseek-v4.1"], policy);

    // The request path resolves it by model id alone, which is unique on
    // OpenRouter, and only for a backend whose provider is OpenRouter.
    let resolved =
        upstreams_config::get_openrouter_policy_for_model(&pool, "deepseek/deepseek-v4.1")
            .await
            .unwrap()
            .unwrap();
    assert_eq!(resolved.max_prompt_price, Some(0.30));
}

#[tokio::test]
async fn a_non_openrouter_backend_never_resolves_an_openrouter_policy() {
    let pool = open(Path::new(":memory:")).await.unwrap();
    upstreams_config::upsert_backend(&pool, &backend("plain", ProviderKind::OpenAiCompatible))
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO openrouter_model_policies
           (backend_id, model_id, max_prompt_price, max_completion_price, strategy,
            available, created_at, updated_at)
         VALUES ((SELECT id FROM backends WHERE name = 'plain'), 'deepseek/deepseek-v4.1',
                 0.30, 1.20, 'balanced', 1, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert!(
        upstreams_config::get_openrouter_policy_for_model(&pool, "deepseek/deepseek-v4.1")
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn the_new_tables_carry_the_policy_columns() {
    let pool = open(Path::new(":memory:")).await.unwrap();
    let columns: Vec<String> =
        sqlx::query("SELECT name FROM pragma_table_info('openrouter_model_policies') ORDER BY cid")
            .fetch_all(&pool)
            .await
            .unwrap()
            .into_iter()
            .map(|row| row.get("name"))
            .collect();
    for expected in [
        "backend_id",
        "model_id",
        "max_prompt_price",
        "max_completion_price",
        "price_unit",
        "strategy",
        "min_throughput",
        "available",
    ] {
        assert!(
            columns.iter().any(|column| column == expected),
            "missing {expected}"
        );
    }
}
