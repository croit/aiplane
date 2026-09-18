// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use thiserror::Error;

use crate::server::db::Pool;
use crate::server::db::automatic_routes::{self, AutomaticRoute, AutomaticRouteCandidate};
use crate::server::db::model_defaults::PricingUnit;
use crate::server::upstreams::{PoolAccess, PoolKind, UpstreamRegistry};

#[derive(Debug, Clone, PartialEq)]
pub struct AutomaticRouteDecision {
    pub alias: String,
    pub policy_version: i64,
    pub selected_key: Option<String>,
    pub selected_target: Option<String>,
    pub effective_target: String,
    pub confidence: Option<f64>,
    pub reason: String,
    pub selector_model: String,
    pub selector_backend: Option<String>,
    pub selector_usage: Option<SelectorUsage>,
    pub selector_duration_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorUsage {
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
}

#[derive(Debug, Error)]
pub enum AutomaticRoutingError {
    #[error("loading automatic route `{alias}`: {source}")]
    Load {
        alias: String,
        #[source]
        source: crate::server::db::DbError,
    },
    #[error("model `{0}` is not allowed by this token")]
    AliasNotAllowed(String),
    #[error("automatic route `{alias}` has no currently eligible candidates")]
    NoEligibleCandidates { alias: String },
    #[error("automatic route `{alias}` fallback `{fallback}` is not currently eligible")]
    IneligibleFallback { alias: String, fallback: String },
}

#[derive(Clone)]
struct AffinityDecision {
    policy_version: i64,
    target: String,
    key: String,
    confidence: Option<f64>,
    expires_at: Instant,
    sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct AffinityKey {
    alias: String,
    principal: String,
    session: String,
}

#[derive(Default)]
struct AffinityCache {
    entries: HashMap<AffinityKey, AffinityDecision>,
    next_sequence: u64,
}

impl AffinityCache {
    fn insert(&mut self, key: AffinityKey, mut decision: AffinityDecision) {
        self.entries.remove(&key);
        if self.entries.len() >= MAX_AFFINITY_ENTRIES {
            let now = Instant::now();
            self.entries.retain(|_, decision| decision.expires_at > now);
        }
        if self.entries.len() >= MAX_AFFINITY_ENTRIES
            && let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, decision)| decision.sequence)
                .map(|(key, _)| key.clone())
        {
            self.entries.remove(&oldest);
        }
        decision.sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1);
        self.entries.insert(key, decision);
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AutomaticRouteAffinity<'a> {
    pub principal: &'a str,
    pub session: &'a str,
}

const MAX_AFFINITY_ENTRIES: usize = 10_000;

pub struct AutomaticRouter {
    db: Pool,
    upstreams: Arc<UpstreamRegistry>,
    http: reqwest::Client,
    routes: Mutex<HashMap<String, Option<Arc<AutomaticRoute>>>>,
    affinity: Mutex<AffinityCache>,
    decisions: tokio::sync::mpsc::Sender<automatic_routes::DecisionInsert>,
}

impl AutomaticRouter {
    pub fn new(db: Pool, upstreams: Arc<UpstreamRegistry>, http: reqwest::Client) -> Self {
        let (decisions, receiver) = tokio::sync::mpsc::channel(1_024);
        tokio::spawn(decision_writer(db.clone(), receiver));
        Self {
            db,
            upstreams,
            http,
            routes: Mutex::new(HashMap::new()),
            affinity: Mutex::new(AffinityCache::default()),
            decisions,
        }
    }

    pub async fn is_route(&self, alias: &str) -> Result<bool, AutomaticRoutingError> {
        Ok(self.load_route(alias).await?.is_some())
    }

    pub fn invalidate_alias(&self, alias: &str) {
        if let Ok(mut routes) = self.routes.lock() {
            routes.remove(alias);
        }
        if let Ok(mut affinity) = self.affinity.lock() {
            affinity.entries.retain(|key, _| key.alias != alias);
        }
    }

    async fn load_route(
        &self,
        alias: &str,
    ) -> Result<Option<Arc<AutomaticRoute>>, AutomaticRoutingError> {
        if let Ok(routes) = self.routes.lock()
            && let Some(route) = routes.get(alias)
        {
            return Ok(route.clone());
        }
        let route = automatic_routes::get(&self.db, alias)
            .await
            .map_err(|source| AutomaticRoutingError::Load {
                alias: alias.to_string(),
                source,
            })?
            .map(Arc::new);
        if let Ok(mut routes) = self.routes.lock() {
            routes.insert(alias.to_string(), route.clone());
        }
        Ok(route)
    }

    pub async fn select(
        &self,
        alias: &str,
        state: &Value,
        access: &PoolAccess,
        affinity: Option<AutomaticRouteAffinity<'_>>,
    ) -> Result<Option<AutomaticRouteDecision>, AutomaticRoutingError> {
        let Some(route) = self.load_route(alias).await? else {
            return Ok(None);
        };
        if !access.allows_model(alias) {
            return Err(AutomaticRoutingError::AliasNotAllowed(alias.to_string()));
        }

        let target_access = PoolAccess {
            allowed_models: None,
            ..access.clone()
        };
        let requirements = RequestRequirements::from_state(state);
        let defaults_by_model: HashMap<String, _> =
            crate::server::db::model_defaults::all(&self.db)
                .await
                .map_err(|source| AutomaticRoutingError::Load {
                    alias: alias.to_string(),
                    source,
                })?
                .into_iter()
                .map(|defaults| (defaults.model_name.clone(), defaults))
                .collect();
        let mut eligible: Vec<&AutomaticRouteCandidate> = Vec::new();
        let mut candidate_facts: HashMap<String, String> = HashMap::new();
        for candidate in &route.candidates {
            let real_models = self.upstreams.resolved_models_for(
                &candidate.target,
                PoolKind::Chat,
                &target_access,
            );
            let [real_model] = real_models.as_slice() else {
                if real_models.len() > 1 {
                    tracing::warn!(route = %route.alias, candidate = %candidate.target, resolved_models = ?real_models, "automatic route candidate resolves to heterogeneous real models");
                }
                continue;
            };
            let defaults = defaults_by_model.get(real_model).cloned();
            let capabilities = defaults
                .as_ref()
                .map(|value| value.capabilities.clone())
                .unwrap_or_default();
            if requirements.accepts(&capabilities) {
                if let Some(defaults) = defaults {
                    let mut facts = Vec::new();
                    if let Some(price) = defaults.input_price {
                        facts.push(format!(
                            "input price {price} {}",
                            price_unit(defaults.pricing_unit)
                        ));
                    }
                    if let Some(price) = defaults.output_price {
                        facts.push(format!(
                            "output price {price} {}",
                            price_unit(defaults.pricing_unit)
                        ));
                    }
                    if let Some(window) = defaults.context_window {
                        facts.push(format!("context window {window} tokens"));
                    }
                    if !facts.is_empty() {
                        candidate_facts.insert(candidate.key.clone(), facts.join(", "));
                    }
                }
                eligible.push(candidate);
            }
        }
        if eligible.is_empty() {
            return Err(AutomaticRoutingError::NoEligibleCandidates {
                alias: alias.to_string(),
            });
        }
        if !eligible
            .iter()
            .any(|candidate| candidate.target == route.fallback_target)
        {
            return Err(AutomaticRoutingError::IneligibleFallback {
                alias: alias.to_string(),
                fallback: route.fallback_target.clone(),
            });
        }

        if route.session_affinity
            && let Some(affinity) = affinity
            && let Some(decision) = self.affinity_hit(&route, affinity, &eligible)
        {
            self.record_decision(
                &route,
                &decision,
                eligible
                    .iter()
                    .map(|candidate| candidate.target.clone())
                    .collect(),
                None,
                0,
            );
            return Ok(Some(decision));
        }

        let started = Instant::now();
        let selection = self
            .ask_selector(
                &route,
                &selector_state(state),
                &eligible,
                &candidate_facts,
                &target_access,
            )
            .await;
        let elapsed = started.elapsed().as_millis() as i64;
        let (selected, confidence, probabilities, selector_model, selector_backend, selector_usage) =
            match selection {
                Ok(answer) => (
                    eligible
                        .iter()
                        .copied()
                        .find(|candidate| candidate.key == answer.key),
                    answer.confidence,
                    answer.probabilities,
                    answer.selector_model,
                    Some(answer.selector_backend),
                    answer.usage,
                ),
                Err(error) => {
                    tracing::warn!(route = %route.alias, error = %error, "automatic routing selector failed; using fallback");
                    (None, None, None, route.selector_model.clone(), None, None)
                }
            };
        let selected_target = selected.map(|candidate| candidate.target.clone());
        let selected_key = selected.map(|candidate| candidate.key.clone());
        let reason = if selected.is_none() {
            "selector_error"
        } else if confidence.is_none_or(|value| value < route.minimum_confidence) {
            "low_confidence"
        } else if route.rollout == "shadow" {
            "shadow"
        } else {
            "selected"
        };
        let effective_target = if reason == "selected" {
            selected_target
                .clone()
                .unwrap_or_else(|| route.fallback_target.clone())
        } else {
            route.fallback_target.clone()
        };
        let decision = AutomaticRouteDecision {
            alias: route.alias.clone(),
            policy_version: route.version,
            selected_key: selected_key.clone(),
            selected_target: selected_target.clone(),
            effective_target: effective_target.clone(),
            confidence,
            reason: reason.into(),
            selector_model,
            selector_backend,
            selector_usage,
            selector_duration_ms: elapsed,
        };
        if route.session_affinity
            && let Some(affinity) = affinity
        {
            self.remember_affinity(&route, affinity, &decision);
        }
        self.record_decision(
            &route,
            &decision,
            eligible
                .iter()
                .map(|candidate| candidate.target.clone())
                .collect(),
            probabilities.as_ref(),
            elapsed,
        );
        tracing::info!(
            route = %route.alias,
            policy_version = route.version,
            selected = ?selected_target,
            effective = %effective_target,
            confidence,
            reason,
            "automatic routing decision"
        );
        Ok(Some(decision))
    }

    pub fn session_target(&self, alias: &str, principal: &str, session: &str) -> Option<String> {
        let key = affinity_key(alias, principal, session);
        let mut cache = self.affinity.lock().ok()?;
        let expired = cache
            .entries
            .get(&key)
            .is_some_and(|decision| decision.expires_at <= Instant::now());
        if expired {
            cache.entries.remove(&key);
            return None;
        }
        cache
            .entries
            .get(&key)
            .map(|decision| decision.target.clone())
    }

    fn affinity_hit(
        &self,
        route: &AutomaticRoute,
        affinity: AutomaticRouteAffinity<'_>,
        eligible: &[&AutomaticRouteCandidate],
    ) -> Option<AutomaticRouteDecision> {
        let key = affinity_key(&route.alias, affinity.principal, affinity.session);
        let mut cache = self.affinity.lock().ok()?;
        let expired = cache
            .entries
            .get(&key)
            .is_some_and(|decision| decision.expires_at <= Instant::now());
        if expired {
            cache.entries.remove(&key);
            return None;
        }
        let cached = cache.entries.get(&key)?;
        if cached.policy_version != route.version
            || !eligible
                .iter()
                .any(|candidate| candidate.target == cached.target)
        {
            cache.entries.remove(&key);
            return None;
        }
        Some(AutomaticRouteDecision {
            alias: route.alias.clone(),
            policy_version: route.version,
            selected_key: Some(cached.key.clone()),
            selected_target: Some(cached.target.clone()),
            effective_target: cached.target.clone(),
            confidence: cached.confidence,
            reason: "session_affinity".into(),
            selector_model: route.selector_model.clone(),
            selector_backend: None,
            selector_usage: None,
            selector_duration_ms: 0,
        })
    }

    fn remember_affinity(
        &self,
        route: &AutomaticRoute,
        affinity: AutomaticRouteAffinity<'_>,
        decision: &AutomaticRouteDecision,
    ) {
        let Some(key) = route
            .candidates
            .iter()
            .find(|candidate| candidate.target == decision.effective_target)
            .map(|candidate| candidate.key.clone())
        else {
            return;
        };
        if let Ok(mut cache) = self.affinity.lock() {
            cache.insert(
                affinity_key(&route.alias, affinity.principal, affinity.session),
                AffinityDecision {
                    policy_version: route.version,
                    target: decision.effective_target.clone(),
                    key,
                    confidence: decision.confidence,
                    expires_at: Instant::now() + Duration::from_secs(route.session_ttl_seconds),
                    sequence: 0,
                },
            );
        }
    }

    async fn ask_selector(
        &self,
        route: &AutomaticRoute,
        state: &Value,
        eligible: &[&AutomaticRouteCandidate],
        candidate_facts: &HashMap<String, String>,
        access: &PoolAccess,
    ) -> Result<SelectorAnswer, String> {
        let acquired = self
            .upstreams
            .route_access(&route.selector_model, PoolKind::SystemOne, access)
            .map_err(|error| format!("routing selector model: {error}"))?;
        let selector_model = acquired.resolved_model().to_string();
        let selector_backend = acquired.backend().name.clone();
        let url = format!(
            "{}/systemone",
            acquired.backend().base_url.trim_end_matches('/')
        );
        let criteria: serde_json::Map<String, Value> = eligible
            .iter()
            .map(|candidate| (candidate.key.clone(), Value::Null))
            .collect();
        let candidates = eligible
            .iter()
            .map(|candidate| {
                let facts = candidate_facts
                    .get(&candidate.key)
                    .map(|value| format!(" ({value})"))
                    .unwrap_or_default();
                format!("{}: {}{}", candidate.key, candidate.description, facts)
            })
            .collect::<Vec<_>>()
            .join("\n");
        let instructions = format!(
            "Choose the best candidate for the request. Optimization objective: {}. {}\nCandidates:\n{}",
            route.objective, route.instructions, candidates
        );
        let body = json!({
            "model": selector_model,
            "state": state,
            "questions": {
                "route": {
                    "type": "choice",
                    "instructions": instructions,
                    "criteria": criteria
                }
            }
        });
        let mut request = self.http.post(url).json(&body);
        if let Some(api_key) = acquired.backend().api_key.as_deref() {
            request = request.bearer_auth(api_key);
        }
        let response = request
            .timeout(Duration::from_millis(route.selector_timeout_ms))
            .send()
            .await
            .map_err(|error| format!("sending selector request: {error}"))?;
        let status = response.status();
        let value: Value = response
            .json()
            .await
            .map_err(|error| format!("reading selector response: {error}"))?;
        if !status.is_success() {
            return Err(format!("selector returned HTTP {status}: {value}"));
        }
        let answer = value
            .pointer("/answers/route")
            .ok_or_else(|| "selector response is missing answers.route".to_string())?;
        let key = answer
            .get("choice")
            .and_then(Value::as_str)
            .ok_or_else(|| "selector response is missing answers.route.choice".to_string())?
            .to_string();
        let confidence = match answer.get("confidence") {
            None => None,
            Some(value) => {
                let confidence = value
                    .as_f64()
                    .filter(|confidence| (0.0..=1.0).contains(confidence))
                    .ok_or_else(|| "selector confidence must be between 0 and 1".to_string())?;
                Some(confidence)
            }
        };
        let probabilities = answer.get("probabilities").cloned();
        let usage = value.get("usage").map(|usage| SelectorUsage {
            input_tokens: usage.get("input_tokens").and_then(Value::as_i64),
            output_tokens: usage.get("output_tokens").and_then(Value::as_i64),
        });
        Ok(SelectorAnswer {
            key,
            confidence,
            probabilities,
            selector_model,
            selector_backend,
            usage,
        })
    }

    fn record_decision(
        &self,
        route: &AutomaticRoute,
        decision: &AutomaticRouteDecision,
        eligible: Vec<String>,
        probabilities: Option<&Value>,
        duration_ms: i64,
    ) {
        if let Err(error) = self.decisions.try_send(automatic_routes::DecisionInsert {
            route_alias: route.alias.clone(),
            route_version: route.version,
            selector_model: route.selector_model.clone(),
            selected_key: decision.selected_key.clone(),
            selected_target: decision.selected_target.clone(),
            effective_target: decision.effective_target.clone(),
            confidence: decision.confidence,
            reason: decision.reason.clone(),
            eligible_targets: eligible,
            probabilities: probabilities.cloned(),
            duration_ms,
        }) {
            tracing::warn!(route = %route.alias, error = %error, "automatic routing decision buffer is full; dropping audit row");
        }
    }
}

async fn decision_writer(
    db: Pool,
    mut receiver: tokio::sync::mpsc::Receiver<automatic_routes::DecisionInsert>,
) {
    while let Some(first) = receiver.recv().await {
        let mut batch = Vec::with_capacity(128);
        batch.push(first);
        while batch.len() < 128 {
            match receiver.try_recv() {
                Ok(decision) => batch.push(decision),
                Err(_) => break,
            }
        }
        if let Err(error) = automatic_routes::record_decisions(&db, &batch).await {
            tracing::warn!(count = batch.len(), error = %error, "recording automatic routing decisions failed");
        }
    }
}

fn price_unit(unit: PricingUnit) -> &'static str {
    match unit {
        PricingUnit::Tokens => "per 1,000,000 tokens",
        PricingUnit::Images => "per image",
        PricingUnit::Characters => "per character",
        PricingUnit::Seconds => "per second",
    }
}

fn affinity_key(alias: &str, principal: &str, session: &str) -> AffinityKey {
    AffinityKey {
        alias: alias.to_string(),
        principal: principal.to_string(),
        session: session.to_string(),
    }
}

fn selector_state(state: &Value) -> Value {
    fn compact(value: &Value, key: Option<&str>) -> Option<Value> {
        match value {
            Value::String(text)
                if text.starts_with("data:")
                    || matches!(key, Some("data" | "b64_json" | "audio")) && text.len() > 256 =>
            {
                Some(Value::String("[inline data omitted]".into()))
            }
            Value::Array(values) => Some(Value::Array(
                values
                    .iter()
                    .filter_map(|value| compact(value, None))
                    .collect(),
            )),
            Value::Object(object) => Some(Value::Object(
                object
                    .iter()
                    .filter(|(name, _)| {
                        !(matches!(key, Some("function")) && name.as_str() == "parameters")
                    })
                    .filter_map(|(name, value)| {
                        compact(value, Some(name)).map(|value| (name.clone(), value))
                    })
                    .collect(),
            )),
            _ => Some(value.clone()),
        }
    }

    compact(state, None).unwrap_or(Value::Null)
}

struct SelectorAnswer {
    key: String,
    confidence: Option<f64>,
    probabilities: Option<Value>,
    selector_model: String,
    selector_backend: String,
    usage: Option<SelectorUsage>,
}

#[derive(Default)]
struct RequestRequirements {
    vision: bool,
    audio: bool,
    tools: bool,
    structured_output: bool,
}

impl RequestRequirements {
    fn from_state(state: &Value) -> Self {
        let mut requirements = Self {
            tools: state
                .get("tools")
                .and_then(Value::as_array)
                .is_some_and(|tools| !tools.is_empty()),
            structured_output: state.get("response_format").is_some()
                || state.get("output_config").is_some(),
            ..Self::default()
        };
        visit_content_types(state, &mut requirements);
        requirements
    }

    fn accepts(&self, capabilities: &crate::server::db::model_defaults::ModelCapabilities) -> bool {
        (!self.vision || capabilities.vision != Some(false))
            && (!self.audio || capabilities.audio_input != Some(false))
            && (!self.tools || capabilities.tools != Some(false))
            && (!self.structured_output || capabilities.structured_output != Some(false))
    }
}

fn visit_content_types(value: &Value, requirements: &mut RequestRequirements) {
    match value {
        Value::Array(values) => {
            for value in values {
                visit_content_types(value, requirements);
            }
        }
        Value::Object(object) => {
            match object.get("type").and_then(Value::as_str) {
                Some("image" | "image_url" | "input_image") => requirements.vision = true,
                Some("input_audio" | "audio") => requirements.audio = true,
                _ => {}
            }
            for value in object.values() {
                visit_content_types(value, requirements);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_types_preserve_selector_usage() {
        let decision = AutomaticRouteDecision {
            alias: "default".into(),
            policy_version: 2,
            selected_key: Some("expert".into()),
            selected_target: Some("reasoning".into()),
            effective_target: "reasoning".into(),
            confidence: Some(0.9),
            reason: "selected".into(),
            selector_model: "jev".into(),
            selector_backend: Some("openrouter".into()),
            selector_usage: Some(SelectorUsage {
                input_tokens: Some(4),
                output_tokens: Some(1),
            }),
            selector_duration_ms: 9,
        };
        assert_eq!(decision.selector_usage.unwrap().input_tokens, Some(4));
    }

    #[test]
    fn request_requirements_reject_known_incompatible_models() {
        let requirements = RequestRequirements::from_state(&json!({
            "messages": [{"role": "user", "content": [{"type": "image_url"}]}],
            "tools": [{"type": "function"}],
            "response_format": {"type": "json_schema"}
        }));
        let incompatible = crate::server::db::model_defaults::ModelCapabilities {
            vision: Some(false),
            tools: Some(true),
            structured_output: Some(true),
            ..Default::default()
        };
        assert!(!requirements.accepts(&incompatible));
        assert!(
            requirements.accepts(&crate::server::db::model_defaults::ModelCapabilities::default())
        );
    }

    #[test]
    fn selector_price_facts_use_the_stored_billing_scale() {
        assert_eq!(price_unit(PricingUnit::Tokens), "per 1,000,000 tokens");
        assert_eq!(price_unit(PricingUnit::Seconds), "per second");
    }

    #[test]
    fn affinity_keys_are_scoped_to_the_authenticated_principal() {
        assert_ne!(
            affinity_key("default", "token-a", "session-1"),
            affinity_key("default", "token-b", "session-1")
        );
    }

    #[test]
    fn affinity_cache_is_bounded() {
        let mut cache = AffinityCache::default();
        for index in 0..=MAX_AFFINITY_ENTRIES {
            cache.insert(
                affinity_key("default", "token", &index.to_string()),
                AffinityDecision {
                    policy_version: 1,
                    target: "model".into(),
                    key: "choice".into(),
                    confidence: Some(1.0),
                    expires_at: Instant::now() + Duration::from_secs(60),
                    sequence: 0,
                },
            );
        }
        assert_eq!(cache.entries.len(), MAX_AFFINITY_ENTRIES);
        assert!(
            !cache
                .entries
                .contains_key(&affinity_key("default", "token", "0"))
        );
    }

    #[test]
    fn selector_state_removes_inline_binary_payloads() {
        let compact = selector_state(&json!({
            "messages": [{
                "role": "user",
                "content": [{"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}}]
            }],
            "tools": [{"type": "function", "function": {"name": "lookup", "description": "Find data", "parameters": {"type": "object", "properties": {"query": {"type": "string"}}}}}]
        }));
        assert_eq!(
            compact
                .pointer("/messages/0/content/0/image_url/url")
                .and_then(Value::as_str),
            Some("[inline data omitted]")
        );
        assert!(compact.pointer("/tools/0/function/parameters").is_none());
        assert_eq!(
            compact
                .pointer("/tools/0/function/name")
                .and_then(Value::as_str),
            Some("lookup")
        );
    }
}
