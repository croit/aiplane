// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! OpenRouter as a provider: the account catalog, per-provider prices, and the
//! purchase-price policy the gateway enforces.
//!
//! OpenRouter's model list is the only place that knows what the account may
//! use and what each provider charges. The health probe is deliberately not
//! that reader: it runs every five seconds and asks one question (is this
//! upstream alive, and what does it serve). Prices and endpoint performance
//! change on a slower clock, and `GET /models/{author}/{slug}/endpoints` costs
//! one request per model. [`refresh_backend`] is the slower loop.
//!
//! The hard guard is not this module: it is the `provider.max_price` block the
//! request path injects (see `server::model_defaults::apply_openrouter_policy`).
//! This module decides what the catalog *shows*; that block decides what
//! OpenRouter may actually charge, including between two refreshes.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use serde::Deserialize;

use crate::server::capped_read;
use crate::server::db::{DbError, Pool, upstreams_config};
use crate::server::upstreams::config::{PoolKind, apply_extra_headers};
use crate::server::upstreams::registry::UpstreamRegistry;

const CATALOG_TIMEOUT: Duration = Duration::from_secs(20);

/// The OpenRouter `output_modalities` value that carries a pool kind's models.
/// `None` for kinds OpenRouter does not serve (OCR is the gateway's own
/// document sidecar, not a public model family).
pub fn modality_for_kind(kind: PoolKind) -> Option<&'static str> {
    match kind {
        PoolKind::Chat => Some("text"),
        PoolKind::Embedding => Some("embeddings"),
        PoolKind::Transcription => Some("transcription"),
        PoolKind::Speech => Some("speech"),
        PoolKind::Image => Some("image"),
        PoolKind::Rerank => Some("rerank"),
        PoolKind::SystemOne => Some("decisions"),
        PoolKind::Ocr => None,
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Architecture {
    #[serde(default, deserialize_with = "list_or_empty")]
    pub input_modalities: Vec<String>,
    #[serde(default, deserialize_with = "list_or_empty")]
    pub output_modalities: Vec<String>,
}

/// Read an explicit `null` as an empty list.
///
/// `#[serde(default)]` covers a *missing* field only. OpenRouter sends
/// `"supported_voices": null` on every model without voices (and has sent
/// `null` for the other list fields), which a bare `Vec` rejects with
/// "invalid type: null, expected a sequence" — and that failed the whole
/// account catalogue, not just the entry.
fn list_or_empty<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Pricing {
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub completion: Option<String>,
    #[serde(default)]
    pub request: Option<String>,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub input_cache_read: Option<String>,
    #[serde(default)]
    pub input_cache_write: Option<String>,
}

/// What a model's input/output price is billed per.
///
/// OpenRouter publishes one number for each side, but the unit depends on the
/// model: chat and token-priced transcription are USD per token (the panel
/// shows per 1M), while whisper-class transcription is USD per audio second.
/// A transcription model that prices its output at zero is duration-priced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriceUnit {
    Token,
    Second,
}

impl PriceUnit {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Token => "token",
            Self::Second => "second",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "token" => Some(Self::Token),
            "second" => Some(Self::Second),
            _ => None,
        }
    }

    /// Re-express a ceiling when a model changes its pricing scheme.
    pub fn convert(self, value: f64, to: Self) -> f64 {
        match (self, to) {
            (Self::Token, Self::Second) => value / 1_000_000.0,
            (Self::Second, Self::Token) => value * 1_000_000.0,
            _ => value,
        }
    }
}

impl Pricing {
    pub fn prompt_per_million(&self) -> f64 {
        per_million(self.prompt.as_deref())
    }

    pub fn completion_per_million(&self) -> f64 {
        per_million(self.completion.as_deref())
    }

    /// The input price in `unit`: USD per 1M tokens, or USD per audio second.
    pub fn prompt_price(&self, unit: PriceUnit, model_id: &str) -> f64 {
        match unit {
            PriceUnit::Token => self.prompt_per_million(),
            PriceUnit::Second => duration_price(self.prompt.as_deref(), model_id),
        }
    }

    /// The output price in `unit`. Duration-priced transcription has none.
    pub fn completion_price(&self, unit: PriceUnit) -> f64 {
        match unit {
            PriceUnit::Token => self.completion_per_million(),
            PriceUnit::Second => self
                .completion
                .as_deref()
                .and_then(parse_price)
                .unwrap_or(0.0),
        }
    }

    pub fn cache_read_price(&self, unit: PriceUnit) -> Option<f64> {
        match unit {
            PriceUnit::Token => self.cache_read_per_million(),
            PriceUnit::Second => None,
        }
    }

    pub fn request_price(&self) -> Option<f64> {
        self.request.as_deref().and_then(parse_price)
    }

    pub fn image_price(&self) -> Option<f64> {
        self.image.as_deref().and_then(parse_price)
    }

    pub fn cache_read_per_million(&self) -> Option<f64> {
        self.input_cache_read
            .as_deref()
            .and_then(parse_price)
            .map(|p| p * 1_000_000.0)
    }

    pub fn cache_write_per_million(&self) -> Option<f64> {
        self.input_cache_write
            .as_deref()
            .and_then(parse_price)
            .map(|p| p * 1_000_000.0)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TopProvider {
    #[serde(default)]
    pub context_length: Option<i64>,
    #[serde(default)]
    pub max_completion_tokens: Option<i64>,
    #[serde(default)]
    pub is_moderated: bool,
}

/// One model from `GET /models/user` — the account-filtered catalog.
#[derive(Debug, Clone, Deserialize)]
pub struct CatalogEntry {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub context_length: Option<i64>,
    #[serde(default)]
    pub architecture: Option<Architecture>,
    #[serde(default)]
    pub pricing: Pricing,
    #[serde(default, deserialize_with = "list_or_empty")]
    pub supported_parameters: Vec<String>,
    #[serde(default)]
    pub top_provider: Option<TopProvider>,
    /// The API paths that serve this model, e.g. `/api/v1/chat/completions`.
    /// The admin UI uses them to group a model under the right pool kind.
    #[serde(default, deserialize_with = "list_or_empty")]
    pub endpoints: Vec<String>,
    /// The voices this TTS model speaks. Empty for non-speech models.
    #[serde(default, deserialize_with = "list_or_empty")]
    pub supported_voices: Vec<String>,
}

impl CatalogEntry {
    pub fn supports(&self, parameter: &str) -> bool {
        self.supported_parameters.iter().any(|p| p == parameter)
    }

    pub fn input_modalities(&self) -> &[String] {
        self.architecture
            .as_ref()
            .map(|a| a.input_modalities.as_slice())
            .unwrap_or(&[])
    }

    pub fn output_modalities(&self) -> &[String] {
        self.architecture
            .as_ref()
            .map(|a| a.output_modalities.as_slice())
            .unwrap_or(&[])
    }

    pub fn max_completion_tokens(&self) -> Option<i64> {
        self.top_provider
            .as_ref()
            .and_then(|t| t.max_completion_tokens)
    }

    /// What this model's prices are billed per. See [`PriceUnit`].
    pub fn price_unit(&self) -> PriceUnit {
        let duration_priced = self
            .output_modalities()
            .iter()
            .any(|modality| modality == "transcription")
            && self
                .pricing
                .completion
                .as_deref()
                .and_then(parse_price)
                .unwrap_or(0.0)
                == 0.0;
        if duration_priced {
            PriceUnit::Second
        } else {
            PriceUnit::Token
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Percentiles {
    #[serde(default)]
    pub p50: Option<f64>,
    #[serde(default)]
    pub p90: Option<f64>,
}

/// One provider endpoint from `GET /models/{author}/{slug}/endpoints`.
#[derive(Debug, Clone, Deserialize)]
pub struct EndpointEntry {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub provider_name: String,
    #[serde(default)]
    pub tag: String,
    #[serde(default)]
    pub pricing: Pricing,
    #[serde(default)]
    pub context_length: Option<i64>,
    #[serde(default)]
    pub max_completion_tokens: Option<i64>,
    #[serde(default)]
    pub throughput_last_30m: Option<Percentiles>,
    #[serde(default)]
    pub latency_last_30m: Option<Percentiles>,
    #[serde(default)]
    pub uptime_last_30m: Option<f64>,
    #[serde(default)]
    pub quantization: Option<String>,
    #[serde(default, deserialize_with = "list_or_empty")]
    pub supported_parameters: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct KeyInfo {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub usage: Option<f64>,
    #[serde(default)]
    pub limit: Option<f64>,
    #[serde(default)]
    pub limit_remaining: Option<f64>,
    #[serde(default)]
    pub is_free_tier: bool,
}

#[derive(Debug, Clone, Default)]
pub struct RefreshReport {
    pub backend: String,
    pub pools: usize,
    pub selected: usize,
    pub available: usize,
    pub errors: Vec<String>,
}

fn per_million(value: Option<&str>) -> f64 {
    value.and_then(parse_price).unwrap_or(0.0) * 1_000_000.0
}

/// USD per second of audio. Microsoft's MAI transcription models are the one
/// exception in OpenRouter's catalog: they quote an hourly rate in `prompt`
/// (`0.10`, `0.36`), every other duration-priced model is already per second.
fn duration_price(value: Option<&str>, model_id: &str) -> f64 {
    let raw = value.and_then(parse_price).unwrap_or(0.0);
    if model_id.starts_with("microsoft/mai-transcribe") {
        raw / 3_600.0
    } else {
        raw
    }
}

fn parse_price(value: &str) -> Option<f64> {
    value
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|p| p.is_finite() && *p >= 0.0)
}

fn endpoint_url(base_url: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

async fn get_json<T: for<'de> Deserialize<'de>>(
    http: &reqwest::Client,
    url: &str,
    api_key: Option<&str>,
    extra_headers: &std::collections::BTreeMap<String, String>,
) -> anyhow::Result<T> {
    let mut request = http.get(url).header(
        "user-agent",
        concat!("aiplane/", env!("CARGO_PKG_VERSION"), " openrouter-catalog"),
    );
    if let Some(key) = api_key {
        request = request.bearer_auth(key);
    }
    request = apply_extra_headers(request, extra_headers);
    let response = request.timeout(CATALOG_TIMEOUT).send().await?;
    let status = response.status();
    let body = capped_read::read_capped(response, capped_read::MODEL_ANSWER_BYTES).await?;
    if !status.is_success() {
        let detail = String::from_utf8_lossy(&body);
        anyhow::bail!("OpenRouter returned HTTP {status} from {url}: {detail}");
    }
    Ok(serde_json::from_slice::<T>(&body)?)
}

#[derive(Deserialize)]
struct CatalogEnvelope {
    #[serde(default)]
    data: Vec<CatalogEntry>,
}

#[derive(Deserialize)]
struct EndpointsEnvelope {
    #[serde(default)]
    data: Option<EndpointsData>,
}

#[derive(Deserialize)]
struct EndpointsData {
    #[serde(default)]
    endpoints: Vec<EndpointEntry>,
}

#[derive(Deserialize)]
struct KeyEnvelope {
    #[serde(default)]
    data: KeyInfo,
}

/// The account-filtered model catalog for one capability.
pub async fn fetch_catalog(
    http: &reqwest::Client,
    base_url: &str,
    api_key: Option<&str>,
    extra_headers: &std::collections::BTreeMap<String, String>,
    modality: &str,
) -> anyhow::Result<Vec<CatalogEntry>> {
    let url = endpoint_url(
        base_url,
        &format!("models/user?output_modalities={modality}"),
    );
    let envelope: CatalogEnvelope = get_json(http, &url, api_key, extra_headers).await?;
    Ok(envelope.data)
}

/// Why `model_id` cannot be used as an OpenRouter `author/slug` path pair.
///
/// The id becomes part of the upstream path, so a `..` segment or a query
/// marker would let a crafted selection walk somewhere else on the provider's
/// host. Shared by the on-demand provider read (which answers 400) and the
/// catalogue refresh (which refuses to request).
pub fn validate_model_id(model_id: &str) -> Result<(), String> {
    let Some((author, slug)) = model_id.split_once('/') else {
        return Err(format!(
            "`{model_id}` is not an OpenRouter `author/slug` id"
        ));
    };
    if [author, slug].iter().any(|part| {
        part.is_empty()
            || *part == "."
            || *part == ".."
            || part.contains('/')
            || part.contains('?')
            || part.contains('#')
    }) {
        return Err(format!("`{model_id}` is not a usable OpenRouter model id"));
    }
    Ok(())
}

/// Per-provider prices and performance for one model.
pub async fn fetch_endpoints(
    http: &reqwest::Client,
    base_url: &str,
    api_key: Option<&str>,
    extra_headers: &std::collections::BTreeMap<String, String>,
    model_id: &str,
) -> anyhow::Result<Vec<EndpointEntry>> {
    if let Err(problem) = validate_model_id(model_id) {
        anyhow::bail!(problem);
    }
    let (author, slug) = model_id
        .split_once('/')
        .expect("validated as an author/slug pair above");
    let url = endpoint_url(base_url, &format!("models/{author}/{slug}/endpoints"));
    let envelope: EndpointsEnvelope = get_json(http, &url, api_key, extra_headers).await?;
    Ok(envelope.data.map(|d| d.endpoints).unwrap_or_default())
}

pub async fn fetch_key(
    http: &reqwest::Client,
    base_url: &str,
    api_key: Option<&str>,
    extra_headers: &std::collections::BTreeMap<String, String>,
) -> anyhow::Result<KeyInfo> {
    let url = endpoint_url(base_url, "key");
    let envelope: KeyEnvelope = get_json(http, &url, api_key, extra_headers).await?;
    Ok(envelope.data)
}

/// A provider endpoint that satisfies a model's purchase-price ceiling.
#[derive(Debug, Clone, Copy)]
pub struct Allowed {
    pub prompt: f64,
    pub completion: f64,
}

/// Whether one endpoint satisfies both ceilings. A missing ceiling does not
/// filter that side.
pub fn endpoint_allowed(
    endpoint: &EndpointEntry,
    max_prompt: Option<f64>,
    max_completion: Option<f64>,
    unit: PriceUnit,
    model_id: &str,
) -> bool {
    max_prompt.is_none_or(|max| endpoint.pricing.prompt_price(unit, model_id) <= max)
        && max_completion.is_none_or(|max| endpoint.pricing.completion_price(unit) <= max)
}

/// The cheapest allowed endpoint, by prompt price then completion price.
pub fn cheapest_allowed<'a>(
    endpoints: &'a [EndpointEntry],
    max_prompt: Option<f64>,
    max_completion: Option<f64>,
    unit: PriceUnit,
    model_id: &str,
) -> Option<(&'a EndpointEntry, Allowed)> {
    endpoints
        .iter()
        .filter(|e| endpoint_allowed(e, max_prompt, max_completion, unit, model_id))
        .map(|e| {
            (
                e,
                Allowed {
                    prompt: e.pricing.prompt_price(unit, model_id),
                    completion: e.pricing.completion_price(unit),
                },
            )
        })
        .min_by(|a, b| {
            (a.1.prompt, a.1.completion)
                .partial_cmp(&(b.1.prompt, b.1.completion))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

/// The voice menu a speech pool should offer: the union of the `selected`
/// models' `supported_voices`, de-duplicated in catalog order.
pub fn selection_voices(catalog: &[CatalogEntry], selected: &HashSet<String>) -> Vec<String> {
    let mut voices: Vec<String> = Vec::new();
    for model in catalog {
        if !selected.contains(&model.id) {
            continue;
        }
        for voice in &model.supported_voices {
            if !voices.contains(voice) {
                voices.push(voice.clone());
            }
        }
    }
    voices
}

/// Fetch the catalog and endpoints for every selected model, persist what was
/// seen, and update each pool view's availability overlay. A model with no
/// provider under its ceiling is hidden from routing and `/v1/models`; the
/// selection itself is untouched, so it returns when a price drops.
pub async fn refresh_backend(
    db: &Pool,
    registry: &UpstreamRegistry,
    http: &reqwest::Client,
    backend_name: &str,
) -> Result<RefreshReport, DbError> {
    let policies = upstreams_config::load_openrouter_policies(db, backend_name).await?;
    let mut report = RefreshReport {
        backend: backend_name.to_string(),
        ..Default::default()
    };
    let mut catalog_cache: HashMap<&'static str, Vec<CatalogEntry>> = HashMap::new();
    let mut selected_seen: HashSet<String> = HashSet::new();

    for pool in registry.pools() {
        let Some(backend) = pool.backends.iter().find(|b| b.name == backend_name) else {
            continue;
        };
        if !backend.is_openrouter() {
            continue;
        }
        let Some(modality) = modality_for_kind(pool.kind) else {
            continue;
        };
        report.pools += 1;
        let selected = backend.configured_models();
        if selected.is_empty() {
            backend.set_available_models(Some(HashSet::new()));
            continue;
        }
        selected_seen.extend(selected.iter().cloned());

        if !catalog_cache.contains_key(modality) {
            match fetch_catalog(
                http,
                &backend.base_url,
                backend.api_key.as_deref(),
                &backend.extra_headers,
                modality,
            )
            .await
            {
                Ok(catalog) => {
                    catalog_cache.insert(modality, catalog);
                }
                Err(err) => {
                    let message = format!(
                        "catalog for {} ({modality}) could not be read: {err}",
                        backend.name
                    );
                    tracing::warn!(backend = %backend.name, %err, "OpenRouter catalog read failed");
                    report.errors.push(message);
                    // Fail closed on a fresh registry: the overlay is empty
                    // until a refresh proves a provider fits. A later tick
                    // restores the models when the catalog answers again.
                    backend.set_available_models(Some(HashSet::new()));
                    continue;
                }
            }
        }
        let catalog = catalog_cache.get(modality).cloned().unwrap_or_default();
        let known: HashMap<&str, &CatalogEntry> =
            catalog.iter().map(|m| (m.id.as_str(), m)).collect();

        let mut available = HashSet::new();
        for model_id in &selected {
            if !known.contains_key(model_id.as_str()) {
                // The account can no longer see it, or the catalog moved on.
                // Hide it rather than sending a request OpenRouter will 404.
                if let Some(policy) = policies.get(model_id) {
                    upstreams_config::set_openrouter_availability(
                        db,
                        backend_name,
                        model_id,
                        false,
                        policy.cheapest_prompt_price,
                        policy.cheapest_completion_price,
                        policy.cheapest_provider.as_deref(),
                    )
                    .await?;
                }
                continue;
            }
            let endpoints = match fetch_endpoints(
                http,
                &backend.base_url,
                backend.api_key.as_deref(),
                &backend.extra_headers,
                model_id,
            )
            .await
            {
                Ok(endpoints) => endpoints,
                Err(err) => {
                    tracing::warn!(
                        backend = %backend.name, model = %model_id, %err,
                        "OpenRouter endpoints read failed; keeping the previous availability"
                    );
                    if policies
                        .get(model_id)
                        .is_some_and(|policy| policy.available)
                    {
                        available.insert(model_id.clone());
                    }
                    continue;
                }
            };

            let unit = known[model_id.as_str()].price_unit();
            let rows: Vec<upstreams_config::OpenRouterEndpointRow> = endpoints
                .iter()
                .map(|endpoint| upstreams_config::OpenRouterEndpointRow {
                    provider_tag: if endpoint.tag.is_empty() {
                        endpoint.provider_name.clone()
                    } else {
                        endpoint.tag.clone()
                    },
                    provider_name: endpoint.provider_name.clone(),
                    prompt_price: endpoint.pricing.prompt_price(unit, model_id),
                    completion_price: endpoint.pricing.completion_price(unit),
                    request_price: endpoint.pricing.request_price(),
                    image_price: endpoint.pricing.image_price(),
                    cache_read_price: endpoint.pricing.cache_read_price(unit),
                    cache_write_price: endpoint
                        .pricing
                        .cache_write_per_million()
                        .filter(|_| unit == PriceUnit::Token),
                    context_length: endpoint.context_length,
                    max_completion_tokens: endpoint.max_completion_tokens,
                    throughput_p50: endpoint.throughput_last_30m.as_ref().and_then(|p| p.p50),
                    latency_p50: endpoint.latency_last_30m.as_ref().and_then(|p| p.p50),
                    uptime_30m: endpoint.uptime_last_30m,
                    quantization: endpoint.quantization.clone(),
                    supported_parameters: endpoint.supported_parameters.clone(),
                })
                .collect();
            upstreams_config::replace_openrouter_endpoints(db, backend_name, model_id, &rows)
                .await?;

            let Some(policy) = policies.get(model_id) else {
                // A selected model with no policy is not offered. The admin UI
                // writes the policy with the selection, so this is a stale row
                // from an older selection.
                continue;
            };
            let (max_prompt, max_completion) = policy.ceilings_in(unit);
            let cheapest = cheapest_allowed(&endpoints, max_prompt, max_completion, unit, model_id);
            let is_available = cheapest.is_some();
            if is_available {
                available.insert(model_id.clone());
            }
            let (prompt, completion, provider) = match cheapest {
                Some((endpoint, allowed)) => (
                    Some(allowed.prompt),
                    Some(allowed.completion),
                    Some(if endpoint.tag.is_empty() {
                        endpoint.provider_name.clone()
                    } else {
                        endpoint.tag.clone()
                    }),
                ),
                None => (None, None, None),
            };
            upstreams_config::set_openrouter_availability(
                db,
                backend_name,
                model_id,
                is_available,
                prompt,
                completion,
                provider.as_deref(),
            )
            .await?;
        }
        report.available = report.available.max(available.len());
        backend.set_available_models(Some(available));

        // The speech pool's voice menu is the union of what the selected TTS
        // models can speak: the picker must offer voices the account can
        // actually synthesise, and nothing it cannot.
        if pool.kind == PoolKind::Speech {
            let voices = selection_voices(&catalog, &selected);
            upstreams_config::set_pool_offer_voices(db, &pool.name, &voices).await?;
            pool.set_offer_voices(voices);
        }
    }
    report.selected = selected_seen.len();
    Ok(report)
}

/// Refresh every OpenRouter backend in the registry. One failing backend never
/// stops the others.
pub async fn refresh_all(
    db: &Pool,
    registry: &UpstreamRegistry,
    http: &reqwest::Client,
) -> Vec<RefreshReport> {
    let mut names: Vec<String> = registry
        .pools()
        .iter()
        .flat_map(|pool| pool.backends.iter())
        .filter(|backend| backend.is_openrouter())
        .map(|backend| backend.name.clone())
        .collect();
    names.sort();
    names.dedup();
    let mut reports = Vec::with_capacity(names.len());
    for name in names {
        match refresh_backend(db, registry, http, &name).await {
            Ok(report) => reports.push(report),
            Err(err) => {
                tracing::warn!(backend = %name, %err, "OpenRouter catalog refresh failed");
                reports.push(RefreshReport {
                    backend: name,
                    errors: vec![err.to_string()],
                    ..Default::default()
                });
            }
        }
    }
    reports
}

#[cfg(test)]
// Tests build plain clients and drain bodies to talk to their in-process
// mocks; the outbound and body rules are about production paths.
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;

    fn endpoint(prompt: &str, completion: &str, tag: &str) -> EndpointEntry {
        EndpointEntry {
            name: tag.into(),
            provider_name: tag.into(),
            tag: tag.into(),
            pricing: Pricing {
                prompt: Some(prompt.into()),
                completion: Some(completion.into()),
                ..Default::default()
            },
            context_length: Some(128_000),
            max_completion_tokens: Some(8_192),
            throughput_last_30m: Some(Percentiles {
                p50: Some(80.0),
                p90: Some(40.0),
            }),
            latency_last_30m: None,
            uptime_last_30m: Some(99.9),
            quantization: Some("fp8".into()),
            supported_parameters: vec!["tools".into()],
        }
    }

    fn catalog_model(id: &str, output: &str, prompt: &str, completion: &str) -> CatalogEntry {
        CatalogEntry {
            id: id.into(),
            name: id.into(),
            description: String::new(),
            context_length: Some(128_000),
            architecture: Some(Architecture {
                input_modalities: vec!["audio".into()],
                output_modalities: vec![output.into()],
            }),
            pricing: Pricing {
                prompt: Some(prompt.into()),
                completion: Some(completion.into()),
                ..Default::default()
            },
            supported_parameters: Vec::new(),
            top_provider: None,
            endpoints: Vec::new(),
            supported_voices: Vec::new(),
        }
    }

    #[test]
    fn pricing_converts_per_token_strings_to_per_million() {
        let pricing = Pricing {
            prompt: Some("0.0000003".into()),
            completion: Some("0.0000012".into()),
            ..Default::default()
        };
        assert!((pricing.prompt_per_million() - 0.30).abs() < 1e-9);
        assert!((pricing.completion_per_million() - 1.20).abs() < 1e-9);
    }

    #[test]
    fn a_duration_priced_transcription_model_prices_in_audio_seconds() {
        let whisper = catalog_model(
            "openai/whisper-large-v3-turbo",
            "transcription",
            "0.00000333",
            "0",
        );
        assert_eq!(whisper.price_unit(), PriceUnit::Second);
        assert!(
            (whisper.pricing.prompt_price(PriceUnit::Second, &whisper.id) - 0.00000333).abs()
                < 1e-12
        );
        assert_eq!(whisper.pricing.completion_price(PriceUnit::Second), 0.0);
        assert_eq!(
            whisper.pricing.cache_read_price(PriceUnit::Second),
            None,
            "a duration-priced model has no token cache"
        );
    }

    #[test]
    fn a_token_priced_transcription_model_keeps_per_million_tokens() {
        let gpt = catalog_model(
            "openai/gpt-4o-transcribe",
            "transcription",
            "0.0000025",
            "0.00001",
        );
        assert_eq!(gpt.price_unit(), PriceUnit::Token);
        assert!((gpt.pricing.prompt_price(PriceUnit::Token, &gpt.id) - 2.50).abs() < 1e-9);
        assert!((gpt.pricing.completion_price(PriceUnit::Token) - 10.0).abs() < 1e-9);
    }

    #[test]
    fn microsoft_mai_duration_models_divide_their_hourly_rate() {
        let mai = catalog_model("microsoft/mai-transcribe-2", "transcription", "0.10", "0");
        assert_eq!(mai.price_unit(), PriceUnit::Second);
        assert!(
            (mai.pricing.prompt_price(PriceUnit::Second, &mai.id) - 0.10 / 3_600.0).abs() < 1e-12,
            "an hourly rate becomes a per-second price"
        );
    }

    #[test]
    fn a_chat_model_with_free_completions_stays_token_priced() {
        let free = catalog_model("acme/free", "text", "0", "0");
        assert_eq!(free.price_unit(), PriceUnit::Token);
    }

    #[test]
    fn a_model_is_available_only_when_a_provider_fits_both_ceilings() {
        let endpoints = vec![
            endpoint("0.0000010", "0.0000020", "expensive"),
            endpoint("0.0000002", "0.0000009", "cheap"),
        ];
        // 0.30 / 1.20 per 1M: only the cheap provider fits both.
        let cheapest = cheapest_allowed(
            &endpoints,
            Some(0.30),
            Some(1.20),
            PriceUnit::Token,
            "acme/chat",
        )
        .unwrap();
        assert_eq!(cheapest.0.tag, "cheap");
        assert!(
            cheapest_allowed(
                &endpoints,
                Some(0.10),
                Some(0.10),
                PriceUnit::Token,
                "acme/chat"
            )
            .is_none()
        );
    }

    #[test]
    fn input_and_output_ceilings_are_independent() {
        let endpoints = [endpoint("0.0000001", "0.0000100", "cheap-in")];
        assert!(
            cheapest_allowed(
                &endpoints,
                Some(0.30),
                Some(0.50),
                PriceUnit::Token,
                "acme/chat"
            )
            .is_none()
        );
        assert!(
            cheapest_allowed(
                &endpoints,
                Some(0.30),
                Some(20.0),
                PriceUnit::Token,
                "acme/chat"
            )
            .is_some()
        );
    }

    #[test]
    fn no_ceiling_accepts_every_provider_and_picks_the_cheapest() {
        let endpoints = vec![
            endpoint("0.0000100", "0.0000500", "expensive"),
            endpoint("0.0000002", "0.0000009", "cheap"),
        ];
        let cheapest =
            cheapest_allowed(&endpoints, None, None, PriceUnit::Token, "acme/chat").unwrap();
        assert_eq!(cheapest.0.tag, "cheap");
        assert!(endpoint_allowed(
            &endpoints[0],
            None,
            None,
            PriceUnit::Token,
            "acme/chat"
        ));
    }

    #[test]
    fn one_ceiling_filters_only_its_own_side() {
        let endpoints = [endpoint("0.0000001", "0.0000100", "cheap-in")];
        assert!(endpoint_allowed(
            &endpoints[0],
            Some(0.20),
            None,
            PriceUnit::Token,
            "acme/chat"
        ));
        assert!(!endpoint_allowed(
            &endpoints[0],
            Some(0.05),
            None,
            PriceUnit::Token,
            "acme/chat"
        ));
        assert!(endpoint_allowed(
            &endpoints[0],
            None,
            Some(20.0),
            PriceUnit::Token,
            "acme/chat"
        ));
    }

    #[test]
    fn an_audio_second_ceiling_filters_in_seconds() {
        let whisper = catalog_model("openai/whisper-large-v3", "transcription", "0.0000075", "0");
        let endpoints = vec![
            endpoint("0.0000075", "0", "deepinfra"),
            endpoint("0.000031", "0", "groq"),
        ];
        let cheapest = cheapest_allowed(
            &endpoints,
            Some(0.00001),
            None,
            whisper.price_unit(),
            &whisper.id,
        )
        .unwrap();
        assert_eq!(cheapest.0.tag, "deepinfra");
        assert!((cheapest.1.prompt - 0.0000075).abs() < 1e-12);
    }

    #[tokio::test]
    async fn a_model_id_with_a_path_segment_is_refused_before_the_request() {
        // The id is interpolated into the upstream path; refusing it here
        // keeps a crafted selection from walking the provider's host.
        let client = reqwest::Client::new();
        for model in ["../admin", "a/../../b", "a/b?x=1", "a/b#frag", "a/", "/b"] {
            let error = fetch_endpoints(
                &client,
                "http://127.0.0.1:9/v1",
                None,
                &std::collections::BTreeMap::new(),
                model,
            )
            .await
            .expect_err("a malformed id is refused");
            assert!(error.to_string().contains("model id"), "{model}: {error}");
        }
    }

    #[test]
    fn an_explicit_null_catalogue_field_reads_as_empty() {
        // OpenRouter answers `"supported_voices": null` for every model without
        // voices (462 of 465 when this was written). A bare `#[serde(default)]`
        // only covers a missing field, so one null failed the whole catalogue
        // with "invalid type: null, expected a sequence".
        let entry: CatalogEntry = serde_json::from_value(serde_json::json!({
            "id": "openai/gpt-5",
            "name": "GPT-5",
            "supported_parameters": null,
            "supported_voices": null,
            "endpoints": null,
            "architecture": { "input_modalities": null, "output_modalities": null }
        }))
        .expect("an explicit null list is an empty list");
        assert!(entry.supported_voices.is_empty());
        assert!(entry.supported_parameters.is_empty());
        assert!(entry.endpoints.is_empty());
        assert!(!entry.supports("tools"));
        assert!(entry.input_modalities().is_empty());
        assert!(entry.output_modalities().is_empty());
    }

    fn tts_model(id: &str, voices: &[&str]) -> CatalogEntry {
        CatalogEntry {
            id: id.into(),
            name: id.into(),
            description: String::new(),
            context_length: None,
            architecture: None,
            pricing: Pricing::default(),
            supported_parameters: Vec::new(),
            top_provider: None,
            endpoints: Vec::new(),
            supported_voices: voices.iter().map(|v| v.to_string()).collect(),
        }
    }

    #[test]
    fn the_voice_menu_is_the_deduplicated_union_of_the_selected_models_voices() {
        let catalog = vec![
            tts_model("openai/gpt-4o-mini-tts", &["alloy", "onyx", "alloy"]),
            tts_model("google/gemini-flash-tts", &["nova", "alloy"]),
            tts_model("not-selected/tts", &["shimmer"]),
            tts_model("plain/chat-model", &[]),
        ];
        let selected = HashSet::from([
            "openai/gpt-4o-mini-tts".to_string(),
            "google/gemini-flash-tts".to_string(),
            "plain/chat-model".to_string(),
        ]);
        assert_eq!(
            selection_voices(&catalog, &selected),
            vec!["alloy", "onyx", "nova"]
        );
    }

    #[test]
    fn an_empty_selection_offers_no_voices() {
        let catalog = vec![tts_model("openai/gpt-4o-mini-tts", &["alloy"])];
        assert!(selection_voices(&catalog, &HashSet::new()).is_empty());
    }

    #[tokio::test]
    async fn a_refresh_syncs_the_speech_pool_voice_menu_from_the_catalog() {
        use crate::server::db;
        use crate::server::upstreams::config::{
            BackendConfig, PickerStrategy, PoolKind, ProviderKind, UpstreamPoolConfig,
        };
        use crate::server::upstreams::registry::UpstreamRegistry;
        use std::collections::HashMap;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/models/user"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [
                    {
                        "id": "openai/gpt-4o-mini-tts",
                        "name": "GPT-4o Mini TTS",
                        "pricing": { "prompt": "0.000000015", "completion": "0.00000006" },
                        "supported_voices": ["alloy", "onyx", "alloy"]
                    },
                    {
                        "id": "not-selected/tts",
                        "name": "Not selected",
                        "pricing": { "prompt": "0.000000015", "completion": "0.00000006" },
                        "supported_voices": ["shimmer"]
                    }
                ]
            })))
            .mount(&mock)
            .await;
        Mock::given(method("GET"))
            .and(path("/models/openai/gpt-4o-mini-tts/endpoints"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": { "endpoints": [
                    {
                        "name": "OpenAI", "provider_name": "OpenAI", "tag": "openai",
                        "pricing": { "prompt": "0.000000015", "completion": "0.00000006" }
                    }
                ] }
            })))
            .mount(&mock)
            .await;

        let pool = db::open(std::path::Path::new(":memory:")).await.unwrap();
        upstreams_config::upsert_backend(
            &pool,
            &upstreams_config::BackendRow {
                provider: ProviderKind::OpenRouter,
                priority: Default::default(),
                name: "or".into(),
                base_url: mock.uri(),
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
            },
        )
        .await
        .unwrap();
        upstreams_config::upsert_pool(
            &pool,
            &upstreams_config::PoolRow {
                name: "speech".into(),
                kind: "speech".into(),
                strategy: "round_robin".into(),
                fallback_offline: None,
                enforce_limits: true,
                sort_order: 0,
                allowed_groups: Vec::new(),
                backends: vec!["or".into()],
                models: vec!["openai/gpt-4o-mini-tts".into()],
                voices: Vec::new(),
                // A stale menu from before the model was selected: the refresh
                // must replace it, not append to it.
                offer_voices: vec!["stale-voice".into()],
                created_at: jiff::Timestamp::now(),
                updated_at: jiff::Timestamp::now(),
            },
        )
        .await
        .unwrap();

        let pools: HashMap<String, UpstreamPoolConfig> = HashMap::from([(
            "speech".to_string(),
            UpstreamPoolConfig {
                voices: Default::default(),
                offer_voices: vec!["stale-voice".into()],
                allowed_groups: Vec::new(),
                fallback_offline: None,
                level_rank: 0,
                enforce_limits: true,
                kind: PoolKind::Speech,
                strategy: PickerStrategy::RoundRobin,
                models: vec!["openai/gpt-4o-mini-tts".into()],
                backend: vec![BackendConfig {
                    provider: ProviderKind::OpenRouter,
                    priority: Default::default(),
                    alias: Default::default(),
                    supports_edit: false,
                    extra_headers: Default::default(),
                    enabled: true,
                    name: "or".into(),
                    base_url: mock.uri(),
                    api_key_env: None,
                    api_key: None,
                    weight: 1,
                    max_inflight: 16,
                    health_path: "/models".into(),
                    models: Vec::new(),
                }],
            },
        )]);
        let registry = UpstreamRegistry::new(&pools).unwrap();

        let report = refresh_backend(&pool, &registry, &reqwest::Client::new(), "or")
            .await
            .unwrap();
        assert!(report.errors.is_empty());

        let snapshot = upstreams_config::load_snapshot(&pool).await.unwrap();
        assert_eq!(
            snapshot.pools[0].offer_voices,
            vec!["alloy".to_string(), "onyx".to_string()],
            "the menu is the selected model's voices, de-duplicated, replacing the stale list"
        );
        // The running registry follows without a topology reload.
        use crate::server::upstreams::registry::PoolAccess;
        assert_eq!(
            registry.speech_voices_for(&PoolAccess::all()),
            vec!["alloy".to_string(), "onyx".to_string()]
        );
    }

    #[test]
    fn every_pool_kind_maps_to_an_openrouter_modality_except_ocr() {
        assert_eq!(modality_for_kind(PoolKind::Chat), Some("text"));
        assert_eq!(modality_for_kind(PoolKind::Embedding), Some("embeddings"));
        assert_eq!(
            modality_for_kind(PoolKind::Transcription),
            Some("transcription")
        );
        assert_eq!(modality_for_kind(PoolKind::Speech), Some("speech"));
        assert_eq!(modality_for_kind(PoolKind::Image), Some("image"));
        assert_eq!(modality_for_kind(PoolKind::Rerank), Some("rerank"));
        assert_eq!(modality_for_kind(PoolKind::SystemOne), Some("decisions"));
        assert_eq!(modality_for_kind(PoolKind::Ocr), None);
    }
}
