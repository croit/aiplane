// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! `/admin/models` — per-model settings.
//!
//! A "Default models" card (auto-saving per-feature pickers) plus one
//! filterable list of every model the gateway advertises — chat models,
//! aliases, and other-kind models (embedding / image / speech / transcription).
//! Each real model is a collapsed row (name, kind, price, context, resolved
//! reasoning style, and which facets are configured); expanding it reveals a
//! single editor form that persists **all** settings at once via
//! `POST /admin/models/save`:
//!
//!   - per-1M-token prices (cost accounting),
//!   - context window (drives auto-compaction),
//!   - reasoning style + adaptive per-effort budgets (Qwen/Anthropic token
//!     budgets) or effort levels (OpenAI/GLM),
//!   - capability tri-states + vision/tools fallbacks,
//!   - sampling defaults (TOML, merged into requests that don't set the key).
//!
//! "Clear all overrides" (`POST /admin/models/clear`) deletes the row. Aliases
//! are dimmed, non-expandable rows (they inherit their target's settings);
//! other-kind models get a price-only editor posting to the same save endpoint.
//!
//! All routes are gated on the `admin` role via
//! [`super::require_admin_or_403`] — non-admins see a 403 page and never the
//! form. The sidebar entry is also conditional on that role.

use gateway_core::server::db::model_defaults as db;
use gateway_core::server::model_defaults as merge;
use gateway_core::server::reasoning::ReasoningStyle;
use gateway_runtime::rama_server::state::RamaState;

// ---------------------------------------------------------------------------
// POST handlers
// ---------------------------------------------------------------------------

/// The shared core of the model-overrides save (form route + JSON API):
/// validate every field, then write either the price-only slice or the
/// whole row. `Err` carries the human-readable failure.
pub(crate) async fn apply_model_form(state: &RamaState, form: &SaveForm) -> Result<(), String> {
    if form.model_name.is_empty() {
        return Err("a model name is required".into());
    }

    // ---- validate every field before touching the DB ----
    let parse_price = |s: &str| -> Result<Option<f64>, String> {
        match s.trim() {
            "" => Ok(None),
            v => match v.parse::<f64>() {
                Ok(n) if n.is_finite() && n >= 0.0 => Ok(Some(n)),
                _ => Err(v.to_string()),
            },
        }
    };
    let input_price = match parse_price(&form.input_price) {
        Ok(p) => p,
        Err(v) => {
            return Err(format!("not a valid price: {v}"));
        }
    };
    let output_price = match parse_price(&form.output_price) {
        Ok(p) => p,
        Err(v) => {
            return Err(format!("not a valid price: {v}"));
        }
    };
    let pricing_unit = db::PricingUnit::parse(&form.pricing_unit);

    if form.price_only == "1" {
        return db::set_pricing_with_unit(
            &state.db,
            &form.model_name,
            input_price,
            output_price,
            pricing_unit,
        )
        .await
        .map_err(|e| e.to_string());
    }
    let context_window = match form.context_window.trim() {
        "" => None,
        s => match s.parse::<i64>() {
            Ok(n) if n >= 1 => Some(n),
            _ => {
                return Err(format!("context window must be a whole number ≥ 1: {s}"));
            }
        },
    };
    let reasoning_style = match form.reasoning_style.trim() {
        "" | "auto" => None,
        s @ ("none" | "qwen" | "openai" | "glm" | "anthropic" | "ollama") => Some(s.to_string()),
        other => {
            return Err(format!("unknown reasoning style: {other}"));
        }
    };
    let budget = |s: &str| -> Result<Option<i64>, String> {
        let s = s.trim();
        if s.is_empty() {
            return Ok(None);
        }
        match s.parse::<i64>() {
            Ok(n) if n >= 1 => Ok(Some(n)),
            _ => Err(format!("budget must be a whole number ≥ 1: {s}")),
        }
    };
    let effort = |s: &str| -> Result<Option<String>, String> {
        let s = s.trim();
        if s.is_empty() {
            return Ok(None);
        }
        if ReasoningStyle::Glm.effort_levels().contains(&s) {
            Ok(Some(s.to_string()))
        } else {
            Err(format!("unknown reasoning effort: {s}"))
        }
    };
    let overrides = match (|| -> Result<db::ReasoningOverrideCols, String> {
        Ok(db::ReasoningOverrideCols {
            budget_standard: budget(&form.budget_standard)?,
            budget_deep: budget(&form.budget_deep)?,
            budget_max: budget(&form.budget_max)?,
            effort_standard: effort(&form.effort_standard)?,
            effort_deep: effort(&form.effort_deep)?,
            effort_max: effort(&form.effort_max)?,
        })
    })() {
        Ok(c) => c,
        Err(e) => return Err(e),
    };
    let toml = form.defaults_toml.trim();
    if !toml.is_empty()
        && let Err(err) = merge::parse_defaults(&form.defaults_toml)
    {
        return Err(format!("invalid defaults TOML: {err}"));
    }

    let tri = |s: &str| -> Option<bool> {
        match s.trim() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        }
    };
    let fb = |s: &str| -> Option<String> {
        let t = s.trim();
        (!t.is_empty()).then(|| t.to_string())
    };
    let fields = db::AllFields {
        defaults_toml: form.defaults_toml.clone(),
        reasoning_style,
        overrides,
        context_window,
        input_price,
        output_price,
        pricing_unit,
        capabilities: db::ModelCapabilities {
            vision: tri(&form.cap_vision),
            audio_input: tri(&form.cap_audio_input),
            pdf_input: tri(&form.cap_pdf_input),
            tools: tri(&form.cap_tools),
            parallel_tools: tri(&form.cap_parallel_tools),
            structured_output: tri(&form.cap_structured_output),
            fallback_vision: fb(&form.fallback_vision),
            fallback_tools: fb(&form.fallback_tools),
        },
    };
    db::set_all(&state.db, &form.model_name, &fields)
        .await
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Form structs
// ---------------------------------------------------------------------------

/// The consolidated per-model save form. Every field is optional / blank =
/// "clear this facet"; the row itself is kept (delete it via `models_clear`).
#[derive(Default, serde::Deserialize)]
pub(crate) struct SaveForm {
    pub(crate) model_name: String,
    #[serde(default)]
    pub(crate) input_price: String,
    #[serde(default)]
    pub(crate) output_price: String,
    #[serde(default)]
    pub(crate) pricing_unit: String,
    #[serde(default)]
    pub(crate) price_only: String,
    #[serde(default)]
    pub(crate) context_window: String,
    #[serde(default)]
    pub(crate) reasoning_style: String,
    #[serde(default)]
    pub(crate) budget_standard: String,
    #[serde(default)]
    pub(crate) budget_deep: String,
    #[serde(default)]
    pub(crate) budget_max: String,
    #[serde(default)]
    pub(crate) effort_standard: String,
    #[serde(default)]
    pub(crate) effort_deep: String,
    #[serde(default)]
    pub(crate) effort_max: String,
    #[serde(default)]
    pub(crate) cap_vision: String,
    #[serde(default)]
    pub(crate) cap_audio_input: String,
    #[serde(default)]
    pub(crate) cap_pdf_input: String,
    #[serde(default)]
    pub(crate) cap_tools: String,
    #[serde(default)]
    pub(crate) cap_parallel_tools: String,
    #[serde(default)]
    pub(crate) cap_structured_output: String,
    #[serde(default)]
    pub(crate) fallback_vision: String,
    #[serde(default)]
    pub(crate) fallback_tools: String,
    #[serde(default)]
    pub(crate) defaults_toml: String,
}

// ---------------------------------------------------------------------------
// Row view models
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Default-models card (unchanged behaviour)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Page body + model list
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Cell formatters
// ---------------------------------------------------------------------------
