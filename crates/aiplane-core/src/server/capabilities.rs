// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Vision-fallback: when a tool result contains image content but the primary
//! model has no vision support, transparently route the image to a configured
//! fallback model and inject the text description instead.
//!
//! Activated only when the admin has set `vision = false` + a `fallback_vision`
//! model on the primary model's capability row. The fallback model must be
//! routable via the normal upstream registry.

use crate::server::capped_read;
use serde_json::Value;

use crate::server::db::model_defaults;
use crate::server::upstreams::{PoolKind, UpstreamRegistry};

/// The prompt sent to the fallback vision model. Asks for a thorough
/// description so the primary model can reason about the image's content
/// (text, layout, colors, UI elements) without seeing it directly.
const DESCRIBE_PROMPT: &str = "Describe this image in detail. Include any visible text, the layout, colors, objects, people, UI elements, and anything else notable. Be thorough — your description will be used by another model that cannot see the image.";

/// One call to the fallback vision model, for a caller that keeps a record
/// of model calls (an agent run's activity log).
#[derive(Debug, Clone, PartialEq)]
pub struct DescribeCall {
    pub model: String,
    pub backend: Option<String>,
    /// The body exactly as sent, the image included.
    pub request: Value,
    pub status: Option<u16>,
    pub response: Option<Value>,
    pub error: Option<String>,
    pub latency_ms: u64,
}

/// What [`maybe_replace_image_content`] made of the parts.
#[derive(Debug, Default)]
pub struct Replaced {
    pub parts: Vec<Value>,
    /// Shown to the user when a fallback was used.
    pub notification: Option<String>,
    /// Every call made to the fallback model, in order.
    pub calls: Vec<DescribeCall>,
}

impl Replaced {
    fn unchanged(parts: &[Value]) -> Self {
        Self {
            parts: parts.to_vec(),
            ..Self::default()
        }
    }
}

/// If the tool-result content parts contain `image_url` entries and the
/// primary model lacks vision support, replace them with a text description
/// produced by the fallback vision model.
pub async fn maybe_replace_image_content(
    parts: &[Value],
    primary_model: &str,
    db: &sqlx::SqlitePool,
    http: &reqwest::Client,
    registry: &UpstreamRegistry,
) -> Replaced {
    let has_image = parts
        .iter()
        .any(|p| p.get("type").and_then(|t| t.as_str()) == Some("image_url"));
    if !has_image {
        return Replaced::unchanged(parts);
    }

    let caps = match model_defaults::get(db, primary_model).await {
        Ok(Some(row)) => row.capabilities,
        _ => return Replaced::unchanged(parts),
    };

    // Unknown is not "no": only a declared `vision = false` makes the notice
    // below true, so an undeclared model gets the image as sent.
    if caps.vision != Some(false) {
        return Replaced::unchanged(parts);
    }

    let Some(fallback_model) = caps.fallback_vision.as_deref() else {
        return Replaced::unchanged(parts);
    };
    let mut calls = Vec::new();

    let mut new_parts: Vec<Value> = Vec::new();
    let mut descriptions: Vec<String> = Vec::new();

    for part in parts {
        match part.get("type").and_then(|t| t.as_str()) {
            Some("image_url") => {
                let url = part
                    .get("image_url")
                    .and_then(|iu| iu.get("url"))
                    .and_then(|u| u.as_str())
                    .unwrap_or("");
                if url.is_empty() {
                    continue;
                }
                let (described, call) = describe_image(http, registry, fallback_model, url).await;
                calls.push(call);
                match described {
                    Ok(desc) => {
                        descriptions.push(format!("({fallback_model}): {desc}"));
                        new_parts.push(serde_json::json!({
                            "type": "text",
                            "text": format!("[Image described by {fallback_model}: {desc}]"),
                        }));
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, fallback_model, "vision fallback: describe failed");
                        new_parts.push(part.clone());
                    }
                }
            }
            Some("text") => {
                new_parts.push(part.clone());
            }
            _ => {
                new_parts.push(part.clone());
            }
        }
    }

    let notification = if descriptions.is_empty() {
        None
    } else {
        Some(format!(
            "The selected model ({primary_model}) has no vision support. Using {fallback_model} to describe the image and attaching the text description instead."
        ))
    };

    Replaced {
        parts: new_parts,
        notification,
        calls,
    }
}

async fn describe_image(
    http: &reqwest::Client,
    registry: &UpstreamRegistry,
    model: &str,
    image_url: &str,
) -> (Result<String, anyhow::Error>, DescribeCall) {
    let started = std::time::Instant::now();
    let mut call = DescribeCall {
        model: model.to_string(),
        backend: None,
        request: Value::Null,
        status: None,
        response: None,
        error: None,
        latency_ms: 0,
    };
    let described = describe_with(http, registry, model, image_url, &mut call).await;
    call.error = described.as_ref().err().map(|e| format!("{e:#}"));
    call.latency_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    (described, call)
}

async fn describe_with(
    http: &reqwest::Client,
    registry: &UpstreamRegistry,
    model: &str,
    image_url: &str,
    call: &mut DescribeCall,
) -> Result<String, anyhow::Error> {
    let acquired = registry
        .acquire_for(model, PoolKind::Chat)
        .map_err(|e| anyhow::anyhow!("routing fallback model: {e}"))?;
    call.backend = Some(acquired.backend().name.clone());
    let url = format!("{}/chat/completions", acquired.backend().base_url);
    let body = serde_json::json!({
        "model": acquired.resolved_model(),
        "messages": [{
            "role": "user",
            "content": [
                {"type": "text", "text": DESCRIBE_PROMPT},
                {"type": "image_url", "image_url": {"url": image_url}}
            ]
        }],
        "max_tokens": 500,
        "stream": false,
    });
    call.request = body.clone();

    let mut req = http.post(&url).json(&body);
    if let Some(key) = acquired.backend().api_key.as_deref() {
        req = req.bearer_auth(key);
    }
    req = crate::server::upstreams::config::apply_extra_headers(
        req,
        &acquired.backend().extra_headers,
    );
    let resp = req.send().await?;
    let status = resp.status();
    call.status = Some(status.as_u16());
    let resp = resp.error_for_status()?;
    let json: Value = capped_read::read_capped_json(resp, capped_read::MODEL_ANSWER_BYTES).await?;
    call.response = Some(json.clone());
    let text = json
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("(no description)");
    Ok(text.to_string())
}

#[cfg(test)]
// The client never reaches the network: the registry routes nowhere, so
// the describe fails before any request. The outbound rule is about
// production paths.
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use crate::server::db::model_defaults::{self, ModelCapabilities};

    async fn replace_with(vision: Option<bool>) -> Replaced {
        let db = crate::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        model_defaults::set_capabilities(
            &db,
            "primary",
            &ModelCapabilities {
                vision,
                fallback_vision: Some("describer".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let registry = UpstreamRegistry::new(&std::collections::HashMap::new()).unwrap();
        let parts = [serde_json::json!({
            "type": "image_url",
            "image_url": { "url": "data:image/png;base64,AAAA" },
        })];
        maybe_replace_image_content(&parts, "primary", &db, &reqwest::Client::new(), &registry)
            .await
    }

    #[tokio::test]
    async fn unknown_vision_passes_the_image_through_untouched() {
        let replaced = replace_with(None).await;
        assert!(
            replaced.calls.is_empty(),
            "no fallback call without explicit vision = false"
        );
        assert_eq!(replaced.parts[0]["type"], "image_url");
        assert_eq!(replaced.notification, None);
    }

    #[tokio::test]
    async fn declared_vision_passes_the_image_through_untouched() {
        assert!(replace_with(Some(true)).await.calls.is_empty());
    }

    #[tokio::test]
    async fn explicit_no_vision_asks_the_fallback_model() {
        let replaced = replace_with(Some(false)).await;
        assert_eq!(replaced.calls.len(), 1);
        assert_eq!(replaced.calls[0].model, "describer");
    }
}
