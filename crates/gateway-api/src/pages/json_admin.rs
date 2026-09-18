// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The `/api/v0/admin/*` JSON surface for the SvelteKit SPA (issue #22,
//! phase 4): groups, users, model defaults/prices, limits, settings, the
//! admin token register, and the upstream topology. One module because the
//! legacy form handlers' DB calls live one crate down in `gateway-core` —
//! these handlers are thin JSON translations of them, and the legacy
//! pages stay alive beside everything until phase 6 removes them.
//!
//! Contract notes:
//! * Every handler is admin-gated ([`require_admin_json`] → 401/403 JSON).
//! * Writes that have live side effects replicate them exactly:
//!   `reload_rbac` after group changes, `reload_settings` + session-policy
//!   push after settings saves, the registry reload + health respawn +
//!   dirty reset for topology applies.

use std::sync::Arc;

use rama::http::service::web::extract::{Path, State};
use rama::http::{Request, Response, StatusCode};

use gateway_core::server::db;
use gateway_core::server::db::limits;
use gateway_core::server::settings;
use gateway_core::server::upstreams::{self, PoolKind};
use gateway_runtime::rama_server::state::RamaState;

use super::{bad_request, internal, json_error, json_ok, raw_path_segment};

// ---------------------------------------------------------------------------
// Groups

/// GET /api/v0/admin/groups — the RBAC groups with their OIDC mappings,
/// tool grants, and skill grants, plus the pickers' option sets.
pub async fn groups_list(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = require_admin_json!(state, req);
    let mut groups = Vec::new();
    for g in db::gateway_groups::list_groups(&state.db)
        .await
        .unwrap_or_default()
    {
        let oidc_values = db::gateway_groups::mapped_values_for_group(&state.db, &g.name)
            .await
            .unwrap_or_default();
        let tools = db::gateway_groups::tools_for_group(&state.db, &g.name)
            .await
            .unwrap_or_default();
        let skills = db::skill_grants::skills_for_role(&state.db, &g.name)
            .await
            .unwrap_or_default();
        groups.push(serde_json::json!({
            "name": g.name,
            "description": g.description,
            "is_admin": g.is_admin,
            "is_default": g.is_default,
            "oidc_values": oidc_values,
            "tools": tools,
            "skills": skills,
        }));
    }
    let observed = db::gateway_groups::observed_oidc_values(&state.db)
        .await
        .unwrap_or_default();
    let mut tool_ids: Vec<String> = state.tools().ids().map(|s| s.to_string()).collect();
    tool_ids.sort();
    let skill_names: Vec<String> = state
        .skills()
        .as_ref()
        .map(|s| s.current().names().map(|n| n.to_string()).collect())
        .unwrap_or_default();
    json_ok(
        StatusCode::OK,
        serde_json::json!({
            "groups": groups,
            "observed_oidc_values": observed,
            "tool_ids": tool_ids,
            "skill_names": skill_names,
        }),
    )
}

#[derive(serde::Deserialize)]
pub struct GroupSaveBody {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub is_admin: bool,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub oidc_values: Vec<String>,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub skills: Vec<String>,
}

/// PUT /api/v0/admin/groups — upsert a group and replace its mappings and
/// grants, then reload the RBAC resolver so the change is live.
pub async fn groups_save(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = require_admin_json!(state, req);
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return bad_request(msg),
    };
    let parsed: GroupSaveBody = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => return bad_request(format!("parsing the group body: {err}")),
    };
    let name = parsed.name.trim().to_string();
    if name.is_empty() {
        return bad_request("a group needs a name");
    }
    if let Err(err) = db::gateway_groups::upsert_group(
        &state.db,
        &name,
        parsed.description.trim(),
        parsed.is_admin,
        parsed.is_default,
    )
    .await
    {
        return internal(err);
    }
    if let Err(err) =
        db::gateway_groups::set_mappings_for_group(&state.db, &name, &parsed.oidc_values).await
    {
        return internal(err);
    }
    if let Err(err) = db::gateway_groups::set_tools_for_group(&state.db, &name, &parsed.tools).await
    {
        return internal(err);
    }
    if let Err(err) = db::skill_grants::set_skills_for_role(&state.db, &name, &parsed.skills).await
    {
        return internal(err);
    }
    state.reload_rbac().await;
    json_ok(StatusCode::OK, serde_json::json!({ "name": name }))
}

/// DELETE /api/v0/admin/groups/{name} — remove a group (cascades its
/// mappings + grants), then reload the resolver.
pub async fn groups_delete(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = require_admin_json!(state, req);
    // Raw URI, not the `Path` extractor: rama lowercases path segments and
    // never percent-decodes them, which would turn a group name into something
    // that matches no row — reported as a successful delete of nothing.
    let Some(name) = raw_path_segment(&req, 0) else {
        return bad_request("the URL is missing its group name");
    };
    // Skill grants live in their own table without an FK — clear explicitly.
    let _ = db::skill_grants::set_skills_for_role(&state.db, &name, &[]).await;
    if let Err(err) = db::gateway_groups::delete_group(&state.db, &name).await {
        return internal(err);
    }
    state.reload_rbac().await;
    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(rama::http::Body::empty())
        .expect("static empty response")
}

// ---------------------------------------------------------------------------
// Users (admin roster + impersonation)

/// GET /api/v0/admin/users — every known user and the impersonation audit trail.
pub async fn users_list(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, admin) = require_admin_json!(state, req);
    let all = match db::users::list_all(&state.db).await {
        Ok(v) => v,
        Err(err) => return internal(err),
    };
    let audit = match db::audit::recent(&state.db, 20).await {
        Ok(events) => events,
        Err(err) => return internal(err),
    };
    let users: Vec<_> = all
        .into_iter()
        .map(|u| {
            let gateway_roles = state.rbac.role_ids_for(&u.roles);
            serde_json::json!({
                "id": u.id,
                "email": u.email,
                "name": u.name,
                "oidc_groups": u.roles,
                "gateway_roles": gateway_roles,
                "created_at": u.created_at.to_string(),
            })
        })
        .collect();
    let audit: Vec<_> = audit
        .into_iter()
        .map(|event| {
            serde_json::json!({
                "id": event.id,
                "action": event.action,
                "actor_email": event.actor_email,
                "target_email": event.target_email,
                "created_at": event.created_at.to_string(),
            })
        })
        .collect();
    json_ok(
        StatusCode::OK,
        serde_json::json!({
            "users": users,
            "audit": audit,
            "current_user_id": admin.id,
            "allow_impersonation": state.config().gateway.allow_impersonation,
        }),
    )
}

/// POST /api/v0/admin/users/{id}/impersonate — mint an impersonation
/// session for the target and hand it back as a Set-Cookie (the SPA
/// reloads to become the target).
pub async fn users_impersonate(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    use gateway_core::rama_server::session::secure_cookies;
    use rama::http::header;

    let (session, admin) = require_admin_json!(state, req);
    // Raw URI, not the `Path` extractor: rama lowercases path segments and
    // never percent-decodes them, which would turn a case-sensitive OIDC subject into something
    // that matches no row — reported as "No such user" for a user who exists.
    let Some(user_id) = raw_path_segment(&req, 1) else {
        return bad_request("the URL is missing its user id");
    };
    if !state.config().gateway.allow_impersonation {
        return json_error(
            StatusCode::FORBIDDEN,
            "forbidden",
            "Impersonation is disabled on this gateway.",
        );
    }
    if session.impersonator_id.is_some() {
        return json_error(
            StatusCode::FORBIDDEN,
            "forbidden",
            "Already impersonating — return to your account before starting another.",
        );
    }
    if user_id == admin.id {
        return bad_request("You can't impersonate yourself.");
    }
    let target = match db::users::find_by_id(&state.db, &user_id).await {
        Ok(Some(u)) => u,
        Ok(None) => return json_error(StatusCode::NOT_FOUND, "not_found", "No such user."),
        Err(err) => return internal(err),
    };
    let new_session = match state
        .sessions
        .create_impersonation(&target.id, &admin.id)
        .await
    {
        Ok(s) => s,
        Err(err) => return internal(format!("could not start impersonation: {err}")),
    };
    if let Err(err) = db::audit::record(
        &state.db,
        &admin.id,
        &admin.email,
        &target.id,
        &target.email,
        db::audit::Action::Start,
    )
    .await
    {
        tracing::warn!(error = %err, "impersonate: audit start");
    }
    let cookie = state
        .sessions
        .cookie(&new_session.id, secure_cookies(&state.public_url()));
    Response::builder()
        .status(StatusCode::OK)
        .header(header::SET_COOKIE, cookie)
        .body(
            serde_json::json!({ "ok": true, "user_id": target.id, "email": target.email })
                .to_string()
                .into(),
        )
        .expect("static JSON response")
}

/// POST /api/v0/admin/impersonate/stop — end an impersonation, restore the
/// admin session (Set-Cookie). Not admin-gated: the live identity is the
/// target. A no-op for an ordinary session.
pub async fn impersonate_stop(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    use gateway_core::rama_server::session::secure_cookies;
    use rama::http::header;

    let (session, _target) = require_session_json!(state, req);
    let Some(admin_id) = session.impersonator_id.clone() else {
        return json_ok(
            StatusCode::OK,
            serde_json::json!({ "ok": true, "stopped": false }),
        );
    };
    let admin_session = match state.sessions.create(&admin_id).await {
        Ok(s) => s,
        Err(err) => return internal(format!("could not restore your account: {err}")),
    };
    let _ = state.sessions.delete(&session.id).await;
    let admin_email = db::users::find_by_id(&state.db, &admin_id)
        .await
        .ok()
        .flatten()
        .map(|u| u.email)
        .unwrap_or_else(|| admin_id.clone());
    let target_email = db::users::find_by_id(&state.db, &session.user_id)
        .await
        .ok()
        .flatten()
        .map(|u| u.email)
        .unwrap_or_default();
    let _ = db::audit::record(
        &state.db,
        &admin_id,
        &admin_email,
        &session.user_id,
        &target_email,
        db::audit::Action::Stop,
    )
    .await;
    let cookie = state
        .sessions
        .cookie(&admin_session.id, secure_cookies(&state.public_url()));
    Response::builder()
        .status(StatusCode::OK)
        .header(header::SET_COOKIE, cookie)
        .body(
            serde_json::json!({ "ok": true, "stopped": true })
                .to_string()
                .into(),
        )
        .expect("static JSON response")
}

// ---------------------------------------------------------------------------
// Model defaults / prices / feature defaults / search settings

/// GET /api/v0/admin/models — every model the registry can serve with its
/// stored overrides, the offered-but-unconfigured set, feature defaults,
/// search settings, and the usage currency.
pub async fn models_list(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    use gateway_core::server::feature_defaults::{self, Feature};

    let (_session, _admin) = require_admin_json!(state, req);
    let mut configured: std::collections::HashMap<String, _> = db::model_defaults::all(&state.db)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|d| (d.model_name.clone(), d))
        .collect();
    let mut models = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (kind, kind_label) in [
        (PoolKind::Chat, "chat"),
        (PoolKind::Embedding, "embedding"),
        (PoolKind::Image, "image"),
        (PoolKind::Speech, "speech"),
        (PoolKind::Transcription, "transcription"),
        (PoolKind::Ocr, "ocr"),
        (PoolKind::Rerank, "rerank"),
        (PoolKind::SystemOne, "system_one"),
    ] {
        for (name, alias_target) in state.upstreams.models_with_alias_target(kind) {
            if !seen.insert(name.clone()) {
                continue;
            }
            if alias_target.is_some() {
                configured.remove(&name);
            }
            let defaults = alias_target
                .is_none()
                .then(|| configured.remove(&name))
                .flatten();
            // The same three-source resolution the request path uses, so the
            // page shows what will actually be sent rather than what the model
            // name suggests. On an Ollama backend that is the visible
            // difference between "qwen" (dropped in silence) and "openai"
            // (understood).
            let reasoning = gateway_core::server::reasoning::ReasoningStyle::resolve(
                defaults
                    .as_ref()
                    .and_then(|value| value.reasoning_style.as_deref()),
                state
                    .upstreams
                    .serving_profile(
                        &name,
                        kind,
                        &gateway_core::server::upstreams::PoolAccess::all(),
                    )
                    .dialect,
                &name,
            );
            models.push(serde_json::json!({
                "name": name,
                "kind": kind_label,
                "alias_target": alias_target,
                "configured": defaults.is_some(),
                "resolved_reasoning_style": reasoning.as_str(),
                "uses_token_budget": reasoning.uses_token_budget(),
                "effort_levels": reasoning.effort_levels(),
                // What the serving backend says this model's context is, if it
                // says anything. The page puts it beside the operator's own
                // value: equal or larger is the ordinary case, *smaller* means
                // the configured figure will not be compacted but silently
                // truncated upstream, and that is worth saying out loud.
                "detected_context_window": state.upstreams.probed_context_window(&name),
                "defaults": defaults.map(|d| model_defaults_json(&d)),
            }));
        }
    }
    let mut leftover: Vec<_> = configured.into_values().collect();
    leftover.sort_by(|a, b| a.model_name.cmp(&b.model_name));
    for defaults in leftover {
        // These are rows for models nothing currently serves, so there is no
        // backend to ask — the model name decides, as it always did.
        let reasoning = gateway_core::server::reasoning::ReasoningStyle::resolve(
            defaults.reasoning_style.as_deref(),
            None,
            &defaults.model_name,
        );
        models.push(serde_json::json!({
            "name": defaults.model_name,
            "kind": "chat",
            "alias_target": null,
            "configured": true,
            "resolved_reasoning_style": reasoning.as_str(),
            "uses_token_budget": reasoning.uses_token_budget(),
            "effort_levels": reasoning.effort_levels(),
            // Nothing serves this model, so nothing has reported a window.
            "detected_context_window": null,
            "defaults": model_defaults_json(&defaults),
        }));
    }
    let mut feature_defaults = Vec::new();
    for feature in [
        Feature::Chat,
        Feature::Transcription,
        Feature::Image,
        Feature::Embedding,
    ] {
        let model = feature_defaults::get(&state.db, feature).await;
        let mut available = state.upstreams.models_for_kind(feature.pool_kind());
        available.sort();
        if available.is_empty() {
            continue;
        }
        feature_defaults.push(serde_json::json!({
            "feature": feature.as_str(),
            "model": model,
            "available": available,
        }));
    }
    let search = match gateway_features::server::search_settings::view(&state.db).await {
        Ok(v) => v,
        Err(err) => return internal(err),
    };
    json_ok(
        StatusCode::OK,
        serde_json::json!({
            "models": models,
            "all_models": state.upstreams.all_models(),
            "currency": state.config().usage.currency,
            "feature_defaults": feature_defaults,
            "search": {
                "provider": search.provider.as_str(),
                "searxng_url": search.searxng_url,
                "brave_key_set": search.brave_key_set,
            },
        }),
    )
}

fn model_defaults_json(d: &db::model_defaults::ModelDefaults) -> serde_json::Value {
    serde_json::json!({
        "defaults_toml": d.defaults_toml,
        "reasoning_style": d.reasoning_style,
        "context_window": d.context_window,
        "input_price": d.input_price,
        "output_price": d.output_price,
        "pricing_unit": d.pricing_unit.as_str(),
        "budget_standard": d.thinking_budget_standard,
        "budget_deep": d.thinking_budget_deep,
        "budget_max": d.thinking_budget_max,
        "effort_standard": d.reasoning_effort_standard,
        "effort_deep": d.reasoning_effort_deep,
        "effort_max": d.reasoning_effort_max,
        "capabilities": {
            "vision": d.capabilities.vision,
            "audio_input": d.capabilities.audio_input,
            "pdf_input": d.capabilities.pdf_input,
            "tools": d.capabilities.tools,
            "parallel_tools": d.capabilities.parallel_tools,
            "structured_output": d.capabilities.structured_output,
            "fallback_vision": d.capabilities.fallback_vision,
            "fallback_tools": d.capabilities.fallback_tools,
        },
    })
}

/// PUT /api/v0/admin/models — validate + write one model's overrides. The
/// body mirrors the legacy form's string fields verbatim (blank = clear),
/// so the shared validation core sees identical input from both surfaces.
pub async fn models_save(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = require_admin_json!(state, req);
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return bad_request(msg),
    };
    // All fields arrive as strings (or null → ""), matching SaveForm.
    let raw: serde_json::Map<String, serde_json::Value> = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(err) => return bad_request(format!("parsing the model body: {err}")),
    };
    let field = |key: &str| -> String {
        match raw.get(key) {
            Some(serde_json::Value::String(s)) => s.clone(),
            Some(v) if !v.is_null() => v.to_string(),
            _ => String::new(),
        }
    };
    let form = super::admin::SaveForm {
        model_name: field("model_name"),
        input_price: field("input_price"),
        output_price: field("output_price"),
        pricing_unit: field("pricing_unit"),
        price_only: field("price_only"),
        context_window: field("context_window"),
        reasoning_style: field("reasoning_style"),
        budget_standard: field("budget_standard"),
        budget_deep: field("budget_deep"),
        budget_max: field("budget_max"),
        effort_standard: field("effort_standard"),
        effort_deep: field("effort_deep"),
        effort_max: field("effort_max"),
        cap_vision: field("cap_vision"),
        cap_audio_input: field("cap_audio_input"),
        cap_pdf_input: field("cap_pdf_input"),
        cap_tools: field("cap_tools"),
        cap_parallel_tools: field("cap_parallel_tools"),
        cap_structured_output: field("cap_structured_output"),
        fallback_vision: field("fallback_vision"),
        fallback_tools: field("fallback_tools"),
        defaults_toml: field("defaults_toml"),
    };
    match super::admin::apply_model_form(&state, &form).await {
        Ok(()) => json_ok(
            StatusCode::OK,
            serde_json::json!({ "model": form.model_name }),
        ),
        Err(e) => bad_request(e),
    }
}

/// DELETE /api/v0/admin/models/{name} — drop a model's stored overrides.
pub async fn models_delete(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = require_admin_json!(state, req);
    // Raw URI, not the `Path` extractor: rama lowercases path segments and
    // never percent-decodes them, which would turn a model id like `Qwen/Qwen3-32B` into something
    // that matches no row — reported as 204 No Content for a row that is still there.
    let Some(name) = raw_path_segment(&req, 0) else {
        return bad_request("the URL is missing its model id");
    };
    match db::model_defaults::delete(&state.db, &name).await {
        Ok(()) => Response::builder()
            .status(StatusCode::NO_CONTENT)
            .body(rama::http::Body::empty())
            .expect("static empty response"),
        Err(err) => internal(err),
    }
}

#[derive(serde::Deserialize)]
pub struct FeatureDefaultBody {
    pub feature: String,
    /// Empty string clears the override.
    pub model: String,
}

/// PUT /api/v0/admin/model-defaults — set/clear a feature's default model.
pub async fn models_feature_default(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    use gateway_core::server::feature_defaults::{self, Feature};

    let (_session, _admin) = require_admin_json!(state, req);
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return bad_request(msg),
    };
    let parsed: FeatureDefaultBody = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => return bad_request(format!("parsing the default body: {err}")),
    };
    let Some(feature) = Feature::from_wire(&parsed.feature) else {
        return bad_request(format!("unknown feature: {}", parsed.feature));
    };
    let model = parsed.model.trim();
    let value = if model.is_empty() { None } else { Some(model) };
    match feature_defaults::set(&state.db, feature, value).await {
        Ok(()) => json_ok(
            StatusCode::OK,
            serde_json::json!({ "feature": parsed.feature, "model": value }),
        ),
        Err(err) => internal(err),
    }
}

#[derive(serde::Deserialize)]
pub struct SearchSettingsBody {
    pub provider: String,
    #[serde(default)]
    pub searxng_url: String,
    /// Blank = keep the stored key (write-only secret).
    #[serde(default)]
    pub brave_api_key: String,
    #[serde(default)]
    pub clear_brave_key: bool,
}

/// PUT /api/v0/admin/search-settings — the web-search provider settings.
pub async fn models_search_save(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    use gateway_features::server::search_settings::{self, SearchProvider};

    let (_session, _admin) = require_admin_json!(state, req);
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return bad_request(msg),
    };
    let parsed: SearchSettingsBody = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => return bad_request(format!("parsing the search body: {err}")),
    };
    let Some(provider) = SearchProvider::from_wire(&parsed.provider) else {
        return bad_request(format!("unknown search provider: {}", parsed.provider));
    };
    if let Err(err) = search_settings::set_provider(&state.db, provider).await {
        return internal(err);
    }
    if let Err(err) = search_settings::set_searxng_url(&state.db, &parsed.searxng_url).await {
        return internal(err);
    }
    let key = parsed.brave_api_key.trim();
    if key.is_empty() {
        if parsed.clear_brave_key
            && let Err(err) = search_settings::set_brave_key(&state.db, &state.crypto, "").await
        {
            return internal(err);
        }
    } else if let Err(err) = search_settings::set_brave_key(&state.db, &state.crypto, key).await {
        return internal(err);
    }
    json_ok(StatusCode::OK, serde_json::json!({ "ok": true }))
}

// ---------------------------------------------------------------------------
// Limits

/// GET /api/v0/admin/limits — every rule with the pickers' option sets.
pub async fn limits_list(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = require_admin_json!(state, req);
    let rules = match limits::list_all(&state.db).await {
        Ok(v) => v,
        Err(err) => return internal(err),
    };
    // Not `unwrap_or_default()`: an empty list renders as "this gateway has no
    // groups", which is indistinguishable from a read that failed.
    let roles: Vec<String> = match db::gateway_groups::list_groups(&state.db).await {
        Ok(groups) => groups.into_iter().map(|g| g.name).collect(),
        Err(err) => return internal(err),
    };
    let users = db::users::list_all(&state.db)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|u| serde_json::json!({ "id": u.id, "email": u.email }))
        .collect::<Vec<_>>();
    let tokens = db::tokens::list_all_with_owner(&state.db)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|t| serde_json::json!({ "id": t.id, "name": t.name, "owner": t.user_email }))
        .collect::<Vec<_>>();
    json_ok(
        StatusCode::OK,
        serde_json::json!({
            "limits": rules.iter().map(limit_json).collect::<Vec<_>>(),
            "users": users,
            "tokens": tokens,
            // Groups are database rows now, not `[[roles]]` in a config file.
            "roles": roles,
            "models": state.upstreams.all_models(),
            "currency": state.config().usage.currency,
        }),
    )
}

fn limit_json(rule: &limits::LimitRule) -> serde_json::Value {
    serde_json::json!({
        "id": rule.id,
        "subject_type": rule.subject_type.as_str(),
        "subject_id": rule.subject_id,
        "model": rule.model,
        "dimension": rule.dimension.as_str(),
        "window": rule.window.as_str(),
        "value": rule.value,
        "managed_by": rule.managed_by.as_str(),
    })
}

#[derive(serde::Deserialize)]
pub struct LimitBody {
    pub subject_type: String,
    pub subject_id: String,
    /// Empty = all models.
    #[serde(default)]
    pub model: String,
    pub dimension: String,
    pub window: String,
    pub value: f64,
}

/// POST /api/v0/admin/limits — upsert one rule (admin rules outrank owner
/// rules; same validations as the form path).
pub async fn limits_save(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = require_admin_json!(state, req);
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return bad_request(msg),
    };
    let parsed: LimitBody = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => return bad_request(format!("parsing the limit body: {err}")),
    };
    let Some(subject_type) = limits::SubjectType::parse(&parsed.subject_type) else {
        return bad_request(format!("unknown subject type: {}", parsed.subject_type));
    };
    // Subject validation mirrors the form path: roles must exist as gateway
    // groups, tokens in the DB, users by id or email.
    let subject_id = match subject_type {
        limits::SubjectType::Role => {
            let known = db::gateway_groups::list_groups(&state.db)
                .await
                .unwrap_or_default();
            if !known.iter().any(|g| g.name == parsed.subject_id) {
                return bad_request(format!("unknown role: {}", parsed.subject_id));
            }
            parsed.subject_id
        }
        limits::SubjectType::Token => {
            match db::tokens::find_by_id(&state.db, &parsed.subject_id).await {
                Ok(Some(t)) => t.id,
                Ok(None) => return bad_request(format!("unknown token: {}", parsed.subject_id)),
                Err(err) => return internal(err),
            }
        }
        limits::SubjectType::User => {
            let users = db::users::list_all(&state.db).await.unwrap_or_default();
            match users.iter().find(|u| {
                u.id == parsed.subject_id
                    || u.email.to_lowercase() == parsed.subject_id.to_lowercase()
            }) {
                Some(u) => u.id.clone(),
                None => return bad_request(format!("unknown user: {}", parsed.subject_id)),
            }
        }
        limits::SubjectType::Global => String::new(),
    };
    let Some(dimension) = limits::Dimension::parse(&parsed.dimension) else {
        return bad_request(format!("unknown dimension: {}", parsed.dimension));
    };
    let Some(window) = limits::Window::parse(&parsed.window) else {
        return bad_request(format!("unknown window: {}", parsed.window));
    };
    if !parsed.value.is_finite() || parsed.value < 0.0 {
        return bad_request("the value must be a number ≥ 0");
    }
    let model = parsed.model.trim();
    let model = if model.is_empty() { None } else { Some(model) };
    match limits::upsert(
        &state.db,
        subject_type,
        &subject_id,
        model,
        dimension,
        window,
        parsed.value,
    )
    .await
    {
        Ok(()) => json_ok(StatusCode::OK, serde_json::json!({ "ok": true })),
        Err(err) => internal(err),
    }
}

/// DELETE /api/v0/admin/limits/{id}
pub async fn limits_delete(
    Path(id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, _admin) = require_admin_json!(state, req);
    match limits::delete(&state.db, &id).await {
        Ok(()) => Response::builder()
            .status(StatusCode::NO_CONTENT)
            .body(rama::http::Body::empty())
            .expect("static empty response"),
        Err(err) => internal(err),
    }
}

// ---------------------------------------------------------------------------
// Settings

/// GET /api/v0/admin/settings — the declarative section/field spec with the
/// in-force values (secrets never leave; only whether they are set), plus
/// the restart-pending list. The SPA renders its own labels; the spec's
/// i18n keys are for the legacy page and omitted here.
pub async fn settings_list(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = require_admin_json!(state, req);
    let effective = settings::effective(&state.config());
    let restart_pending = settings::restart_pending(&state.db)
        .await
        .unwrap_or_default();
    let sections: Vec<_> = settings::SECTIONS
        .iter()
        .map(|section| {
            let fields: Vec<_> = section
                .fields
                .iter()
                .map(|f| {
                    let models = match f.kind {
                        settings::Kind::Model(kind) => state.upstreams.models_for_kind(kind),
                        _ => Vec::new(),
                    };
                    serde_json::json!({
                        "key": f.key,
                        "kind": settings_kind(f.kind),
                        "span": settings_span(f.span),
                        "restart": f.restart,
                        "value": effective.shown(f.key),
                        "secret_set": matches!(f.kind, settings::Kind::Secret)
                            && effective.secret_is_set(f.key),
                        "models": models,
                        // Closed option set for a `choice` field; empty for
                        // every other kind. The SPA labels each option from
                        // its own catalog, so only the identifiers travel.
                        "choices": f.choices(),
                    })
                })
                .collect();
            serde_json::json!({
                "name": section.name,
                "category": section.category.slug(),
                "enabled": settings::section_is_enabled(&state.config(), section),
                "fields": fields,
            })
        })
        .collect();
    json_ok(
        StatusCode::OK,
        serde_json::json!({
            "sections": sections,
            "restart_pending": restart_pending,
            "needs_backend": state.upstreams.all_models().is_empty(),
        }),
    )
}

fn settings_span(span: settings::Span) -> &'static str {
    match span {
        settings::Span::Full => "full",
        settings::Span::Half => "half",
    }
}

fn settings_kind(kind: settings::Kind) -> &'static str {
    match kind {
        settings::Kind::Bool => "bool",
        settings::Kind::Int => "int",
        settings::Kind::Float => "float",
        settings::Kind::Text => "text",
        settings::Kind::Path => "path",
        settings::Kind::Model(_) => "model",
        settings::Kind::Choice(_) => "choice",
        settings::Kind::Secret => "secret",
        settings::Kind::List => "list",
    }
}

#[derive(serde::Deserialize)]
pub struct SettingsSaveBody {
    pub section: String,
    /// key → submitted string value. Same semantics per kind as the legacy
    /// form: absent bool = false, blank secret = keep stored, list =
    /// comma-separated.
    pub values: std::collections::HashMap<String, String>,
}

/// POST /api/v0/admin/settings — persist one section with the legacy
/// coercion rules, mark settings operator-owned, push the hot reload
/// (config snapshot + session policy), and track restart-pending fields.
pub async fn settings_save(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = require_admin_json!(state, req);
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return bad_request(msg),
    };
    let parsed: SettingsSaveBody = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => return bad_request(format!("parsing the settings body: {err}")),
    };
    let Some(section) = settings::SECTIONS.iter().find(|s| s.name == parsed.section) else {
        return bad_request(format!("unknown settings section: {}", parsed.section));
    };

    let mut pairs: Vec<(String, String)> = Vec::new();
    let mut restart_fields: Vec<String> = Vec::new();
    for field in section.fields {
        let Some(submitted) = parsed.values.get(field.key) else {
            // Bool: absence is false; every other kind: absence = leave
            // alone (the SPA always submits all fields, but stay lenient).
            if matches!(field.kind, settings::Kind::Bool) {
                pairs.push((field.key.to_owned(), "false".into()));
            }
            continue;
        };
        let submitted = submitted.trim().to_owned();
        match field.kind {
            settings::Kind::Bool => pairs.push((
                field.key.to_owned(),
                (submitted == "on" || submitted == "true").to_string(),
            )),
            settings::Kind::Secret => {
                if !submitted.is_empty() {
                    pairs.push((field.key.to_owned(), submitted));
                }
            }
            settings::Kind::List => {
                let items: Vec<String> = submitted
                    .split(',')
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(str::to_owned)
                    .collect();
                pairs.push((
                    field.key.to_owned(),
                    serde_json::to_string(&items).unwrap_or_else(|_| "[]".into()),
                ));
            }
            _ => pairs.push((field.key.to_owned(), submitted)),
        }
        if field.restart && parsed.values.contains_key(field.key) {
            restart_fields.push(field.key.to_owned());
        }
    }

    if let Err(err) = settings::store(&state.db, &state.crypto, &pairs).await {
        return internal(err);
    }
    state.reload_settings().await;
    // The session policy lives on the store, outside the config snapshot —
    // pushing it here is what makes gateway.session_* live without restart.
    {
        let config = state.config();
        let ttl = config.gateway.session_ttl_days.clamp(1, 400);
        let max = config.gateway.session_absolute_max_days.clamp(1, 400);
        state.sessions.set_policy(
            std::time::Duration::from_secs((ttl * 86400) as u64),
            std::time::Duration::from_secs((max * 86400) as u64),
        );
    }
    if !restart_fields.is_empty() {
        let mut pending = settings::restart_pending(&state.db)
            .await
            .unwrap_or_default();
        for key in restart_fields {
            if !pending.contains(&key) {
                pending.push(key);
            }
        }
        if let Err(err) = settings::mark_restart_pending(&state.db, &pending).await {
            tracing::warn!(error = %err, "recording restart-pending fields");
        }
    }
    json_ok(StatusCode::OK, serde_json::json!({ "ok": true }))
}

#[derive(serde::Deserialize)]
pub struct SettingsClearBody {
    pub key: String,
}

/// POST /api/v0/admin/settings/clear — drop one stored value (the only way
/// to remove a secret); the built-in default applies again.
pub async fn settings_clear(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = require_admin_json!(state, req);
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return bad_request(msg),
    };
    let parsed: SettingsClearBody = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => return bad_request(format!("parsing the clear body: {err}")),
    };
    if settings::field(&parsed.key).is_none() {
        return bad_request(format!("unknown settings field: {}", parsed.key));
    }
    if let Err(err) = settings::clear(&state.db, &parsed.key).await {
        return internal(err);
    }
    state.reload_settings().await;
    json_ok(StatusCode::OK, serde_json::json!({ "cleared": parsed.key }))
}

// ---------------------------------------------------------------------------
// Admin token register

/// GET /api/v0/admin/tokens — every token with owner, resolved allowlists
/// (owner vs admin lists), per-token limits, month-to-date usage, and the
/// pickable model set.
pub async fn tokens_list(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (session, _admin) = require_admin_json!(state, req);
    let tokens = match db::tokens::list_all_with_owner(&state.db).await {
        Ok(v) => v,
        Err(err) => return internal(err),
    };
    let lists = db::token_models::lists_all(&state.db)
        .await
        .unwrap_or_default();
    let limits: std::collections::HashMap<String, Vec<serde_json::Value>> =
        db::limits::list_all(&state.db)
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|r| matches!(r.subject_type, limits::SubjectType::Token))
            .fold(std::collections::HashMap::new(), |mut acc, rule| {
                acc.entry(rule.subject_id.clone())
                    .or_default()
                    .push(limit_json(&rule));
                acc
            });
    // Month-to-date per-token usage, same window the page uses.
    let tz = session
        .timezone
        .clone()
        .or_else(|| _admin.timezone.clone())
        .unwrap_or_else(|| "UTC".to_string());
    let now = jiff::Timestamp::now();
    let bounds = db::usage::period_bounds(db::usage::Period::ThisMonth, &tz, now);
    let agg = db::usage::aggregate(
        &state.db,
        bounds,
        &db::usage::Filter::default(),
        state.config().usage.retention_days,
        now,
        false,
    )
    .await
    .unwrap_or_default();
    let mut usage_by_token = std::collections::HashMap::new();
    for g in agg.by_token {
        usage_by_token.insert(
            g.key.clone(),
            serde_json::json!({
                "requests": g.requests,
                "total_tokens": g.total_tokens,
                "cost": g.cost,
            }),
        );
    }
    let out: Vec<_> = tokens
        .into_iter()
        .map(|t| {
            let (owner_models, admin_models) = lists
                .get(&t.id)
                .map(|l| (l.owner.clone(), l.admin.clone()))
                .unwrap_or_default();
            serde_json::json!({
                "id": t.id,
                "name": t.name,
                "owner_id": t.user_id,
                "owner_email": t.user_email,
                "created_at": t.created_at.to_string(),
                "last_used_at": t.last_used_at.map(|value| value.to_string()),
                "expires_at": t.expires_at.to_string(),
                "revoked": t.revoked_at.is_some(),
                "tools_enabled": t.tools_enabled,
                "owner_models": owner_models,
                "admin_models": admin_models,
                "limits": limits.get(&t.id).cloned().unwrap_or_default(),
                "usage_this_month": usage_by_token.get(&t.id).cloned().unwrap_or(serde_json::json!({
                    "requests": 0, "total_tokens": 0, "cost": 0.0,
                })),
            })
        })
        .collect();
    json_ok(
        StatusCode::OK,
        serde_json::json!({
            "tokens": out,
            "models": state.upstreams.all_models_for(&gateway_core::server::upstreams::PoolAccess::all()),
            "usage_enabled": state.usage.is_enabled(),
            "currency": state.config().usage.currency,
            "timezone": tz,
        }),
    )
}

#[derive(serde::Deserialize)]
pub struct AdminTokenModelsBody {
    pub restrict: bool,
    #[serde(default)]
    pub models: Vec<String>,
}

/// PUT /api/v0/admin/tokens/{id}/models — replace the ADMIN-side model
/// allowlist (owner rows untouched; the two intersect at routing time).
pub async fn tokens_models(
    Path(id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, _admin) = require_admin_json!(state, req);
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return bad_request(msg),
    };
    let parsed: AdminTokenModelsBody = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => return bad_request(format!("parsing the models body: {err}")),
    };
    match db::tokens::find_by_id(&state.db, &id).await {
        Ok(Some(_)) => {}
        Ok(None) => return json_error(StatusCode::NOT_FOUND, "not_found", "no such token"),
        Err(err) => return internal(err),
    }
    if parsed.restrict && parsed.models.is_empty() {
        return bad_request("restricting to an empty list would block every model");
    }
    let to_store: Vec<String> = if parsed.restrict {
        parsed.models
    } else {
        Vec::new()
    };
    match db::token_models::set_for_token(&state.db, &id, &to_store, limits::ManagedBy::Admin).await
    {
        Ok(()) => json_ok(StatusCode::OK, serde_json::json!({ "models": to_store })),
        Err(err) => internal(err),
    }
}

// ---------------------------------------------------------------------------
// Upstream topology (backends + pools + fallbacks + apply + live health)

use gateway_core::server::db::upstreams_config::{self, AliasRow, BackendRow, PoolRow, VoiceRow};

/// GET /api/v0/admin/upstreams — the DB topology, the live registry health,
/// the last hour of per-backend request counts, and the dirty counter.
pub async fn topology_list(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = require_admin_json!(state, req);
    let snapshot = match upstreams_config::load_snapshot(&state.db).await {
        Ok(s) => s,
        Err(err) => return internal(err),
    };
    let now = jiff::Timestamp::now();
    let usage = db::usage::recent_buckets_by_backend(&state.db, now, 5, 12)
        .await
        .unwrap_or_default();

    // Live registry state per backend name.
    let mut live: std::collections::HashMap<String, serde_json::Value> =
        std::collections::HashMap::new();
    for pool in state.upstreams.pools() {
        for backend in &pool.backends {
            // One snapshot each: `models_snapshot` materialises the effective
            // set, and `detected` clones the identification record — both were
            // being taken more than once per backend.
            let models = backend.models_snapshot();
            let detected = backend.detected();
            let entry = serde_json::json!({
                "healthy": backend.is_healthy(),
                "enabled": backend.is_enabled(),
                "auth_failed": backend.auth_failed(),
                "inflight": backend.inflight(),
                "max_inflight": backend.max_inflight,
                // Sorted for the same reason the live stream sorts them: a
                // `HashSet` would render the first paint in a random order.
                "models": sorted(models),
                "withheld": sorted(backend.withheld_models()),
                "pool": pool.name,
                // What identification made of this backend: shown as a badge,
                // never as a setting — nothing here is an operator's choice.
                // `detected_max_parallel` is the one an operator can act on,
                // when the configured in-flight ceiling promises more than the
                // server said it runs.
                "profile": detected.profile.as_str(),
                "detected_version": detected.version,
                "detected_max_parallel": detected.max_parallel,
                // Identification runs only on apply, so how long ago it ran is
                // the difference between a profile describing this server and
                // one describing the server it used to be.
                "detected_at": detected.detected_at,
            });
            live.insert(backend.name.clone(), entry);
        }
    }

    let coverage: std::collections::HashMap<_, _> = snapshot
        .pools
        .iter()
        .map(|pool| {
            let models = state
                .upstreams
                .pool_model_coverage(&pool.name)
                .into_iter()
                .map(|(name, serving, total)| {
                    serde_json::json!({
                        "name": name,
                        "serving": serving,
                        "total": total,
                    })
                })
                .collect::<Vec<_>>();
            (pool.name.clone(), models)
        })
        .collect();
    let pending_changes = topology_pending_changes(
        &snapshot.pools,
        &snapshot.backends,
        &state.upstreams.live_topology(),
    );
    let pools: Vec<_> = snapshot
        .pools
        .iter()
        .map(|p| {
            serde_json::json!({
                "name": p.name,
                "kind": p.kind,
                "strategy": p.strategy,
                "fallback_offline": p.fallback_offline,
                "compliance_gdpr": p.compliance_gdpr,
                "compliance_nda": p.compliance_nda,
                "enforce_limits": p.enforce_limits,
                "sort_order": p.sort_order,
                "allowed_groups": p.allowed_groups,
                "backends": p.backends,
                "models": p.models,
                "voices": p.voices.iter().map(|v| serde_json::json!({"lang": v.lang_code, "voice": v.voice_id})).collect::<Vec<_>>(),
                "offer_voices": p.offer_voices,
            })
        })
        .collect();
    let backends: Vec<_> = snapshot
        .backends
        .values()
        .map(|b| {
            serde_json::json!({
                "name": b.name,
                "base_url": b.base_url,
                "api_key_env": b.api_key_env,
                "api_key_env_set": b.api_key_env.as_ref().is_some_and(|name| {
                    std::env::var(name).is_ok_and(|value| !value.is_empty())
                }),
                "has_stored_key": b.api_key_ct.is_some(),
                "weight": b.weight,
                "max_inflight": b.max_inflight,
                "health_path": b.health_path,
                "probe_models": b.probe_models,
                "supports_edit": b.supports_edit,
                "enabled": b.enabled,
                "models": b.models,
                "aliases": b.aliases.iter().map(|a| serde_json::json!({"alias": a.alias, "target": a.target})).collect::<Vec<_>>(),
                "live": live.get(&b.name),
            })
        })
        .collect();
    json_ok(
        StatusCode::OK,
        serde_json::json!({
            "pools": pools,
            "backends": backends,
            "fallbacks": snapshot.fallbacks,
            "all_models": state.upstreams.all_models(),
            "coverage": coverage,
            "pending_changes": pending_changes,
            "usage_last_hour": usage,
            "dirty": state.topology_dirty_count(),
            // The vocabulary, so the SPA renders its picker from data rather
            // than from a hardcoded <option> list that can fall behind the
            // enum (as it had — `rerank` was missing from every copy).
            "pool_kinds": upstreams::config::PoolKind::ALL
                .iter()
                .map(|k| k.as_str())
                .collect::<Vec<_>>(),
            "pool_strategies": ["prefix_affinity", "least_inflight", "round_robin"],
            "fallback_kinds": ["chat", "transcription", "embedding", "image"],
        }),
    )
}

fn topology_pending_changes(
    pools: &[PoolRow],
    backends: &std::collections::HashMap<String, BackendRow>,
    live: &gateway_core::server::upstreams::LiveTopology,
) -> Vec<serde_json::Value> {
    use std::collections::BTreeMap;

    let live_pools: BTreeMap<_, _> = live
        .pools
        .iter()
        .map(|pool| (pool.name.as_str(), pool))
        .collect();
    let db_pools: BTreeMap<_, _> = pools
        .iter()
        .map(|pool| (pool.name.as_str(), pool))
        .collect();
    let mut changes = Vec::new();
    for (name, pool) in &db_pools {
        let Some(live_pool) = live_pools.get(name) else {
            changes.push(serde_json::json!({"code": "pool_added", "pool": name}));
            continue;
        };
        if pool.kind != live_pool.kind.as_str() {
            changes.push(serde_json::json!({
                "code": "pool_kind",
                "pool": name,
                "from": live_pool.kind.as_str(),
                "to": pool.kind,
            }));
        }
        let live_strategy = picker_strategy_key(live_pool.strategy);
        if pool.strategy != live_strategy {
            changes.push(serde_json::json!({
                "code": "pool_strategy",
                "pool": name,
                "from": live_strategy,
                "to": pool.strategy,
            }));
        }
        let mut db_members = pool.backends.iter().map(String::as_str).collect::<Vec<_>>();
        db_members.sort_unstable();
        let live_members = live_pool
            .backends
            .iter()
            .map(|backend| backend.name.as_str())
            .collect::<Vec<_>>();
        for backend in db_members
            .iter()
            .filter(|name| !live_members.contains(name))
        {
            changes.push(
                serde_json::json!({"code": "backend_joins", "backend": backend, "pool": name}),
            );
        }
        for backend in live_members
            .iter()
            .filter(|name| !db_members.contains(name))
        {
            changes.push(
                serde_json::json!({"code": "backend_leaves", "backend": backend, "pool": name}),
            );
        }
        for live_backend in &live_pool.backends {
            let Some(backend) = backends.get(&live_backend.name) else {
                continue;
            };
            if backend.base_url.trim_end_matches('/') != live_backend.base_url {
                changes.push(serde_json::json!({
                    "code": "backend_url",
                    "backend": live_backend.name,
                    "from": live_backend.base_url,
                    "to": backend.base_url,
                }));
            }
            if backend.weight.max(1) != live_backend.weight
                || backend.max_inflight.max(1) != live_backend.max_inflight
            {
                changes.push(serde_json::json!({
                    "code": "backend_limits",
                    "backend": live_backend.name,
                    "weight": backend.weight.max(1),
                    "inflight": backend.max_inflight.max(1),
                }));
            }
            if backend.health_path != live_backend.health_path {
                changes.push(serde_json::json!({
                    "code": "backend_health_path",
                    "backend": live_backend.name,
                    "to": backend.health_path,
                }));
            }
        }
    }
    for name in live_pools
        .keys()
        .filter(|name| !db_pools.contains_key(*name))
    {
        changes.push(serde_json::json!({"code": "pool_removed", "pool": name}));
    }
    changes
}

fn picker_strategy_key(strategy: gateway_core::server::upstreams::PickerStrategy) -> &'static str {
    use gateway_core::server::upstreams::PickerStrategy;
    match strategy {
        PickerStrategy::RoundRobin => "round_robin",
        PickerStrategy::LeastInflight => "least_inflight",
        PickerStrategy::PrefixAffinity => "prefix_affinity",
    }
}

#[derive(serde::Deserialize)]
pub struct BackendSaveBody {
    pub name: String,
    pub base_url: String,
    #[serde(default)]
    pub api_key_env: String,
    /// A freshly entered key is sealed + stored; blank keeps the stored one
    /// (the API never echoes secrets).
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub weight: u32,
    #[serde(default)]
    pub max_inflight: u32,
    #[serde(default)]
    pub health_path: String,
    #[serde(default)]
    pub probe_models: bool,
    #[serde(default)]
    pub supports_edit: bool,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub aliases: Vec<AliasBody>,
    /// Pool membership to set. `null` unassigns the backend.
    #[serde(default)]
    pub pool: Option<String>,
    /// Guard against the accidental-overwrite the form path guards: the
    /// caller confirms when the name already exists.
    #[serde(default)]
    pub overwrite: bool,
}

#[derive(serde::Deserialize)]
pub struct AliasBody {
    pub alias: String,
    #[serde(default)]
    pub target: Option<String>,
}

#[derive(serde::Deserialize)]
pub struct BackendTestBody {
    #[serde(default)]
    pub name: String,
    pub base_url: String,
    #[serde(default)]
    pub api_key_env: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub health_path: String,
}

const BACKEND_TEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(8);
const BACKEND_TEST_MAX_MODELS: usize = 40;

pub async fn backends_test(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = require_admin_json!(state, req);
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(bytes) => bytes,
        Err(message) => return bad_request(message),
    };
    let parsed: BackendTestBody = match serde_json::from_slice(&bytes) {
        Ok(parsed) => parsed,
        Err(err) => return bad_request(format!("parsing the backend test body: {err}")),
    };
    let base_url = parsed.base_url.trim().trim_end_matches('/');
    if base_url.is_empty() {
        return json_ok(
            StatusCode::OK,
            serde_json::json!({"outcome": "error", "code": "base_url_required", "models": []}),
        );
    }
    let health_path = match parsed.health_path.trim() {
        "" => "/models",
        path => path,
    };
    let url = format!("{base_url}{health_path}");
    let (key, key_source) = backend_test_key(&state, &parsed).await;
    let mut request = state.http.get(&url).header(
        "user-agent",
        concat!(
            "llm-gateway/",
            env!("CARGO_PKG_VERSION"),
            " connection-test"
        ),
    );
    if let Some(key) = key.as_deref() {
        request = request.bearer_auth(key);
    }
    let response = match tokio::time::timeout(BACKEND_TEST_TIMEOUT, request.send()).await {
        Ok(Ok(response)) => response,
        Ok(Err(err)) => {
            return json_ok(
                StatusCode::OK,
                serde_json::json!({
                    "outcome": "error",
                    "code": "unreachable",
                    "url": url,
                    "detail": deepest_error(&err),
                    "key_source": key_source,
                    "models": [],
                }),
            );
        }
        Err(_) => {
            return json_ok(
                StatusCode::OK,
                serde_json::json!({
                    "outcome": "error",
                    "code": "timeout",
                    "url": url,
                    "timeout_seconds": BACKEND_TEST_TIMEOUT.as_secs(),
                    "key_source": key_source,
                    "models": [],
                }),
            );
        }
    };
    let status = response.status().as_u16();
    let body = response.bytes().await.unwrap_or_default();
    if matches!(status, 401 | 403) {
        return json_ok(
            StatusCode::OK,
            serde_json::json!({
                "outcome": "error",
                "code": "auth_failed",
                "status": status,
                "key_source": key_source,
                "models": [],
            }),
        );
    }
    if !(200..300).contains(&status) {
        return json_ok(
            StatusCode::OK,
            serde_json::json!({
                "outcome": "error",
                "code": "http_error",
                "status": status,
                "url": url,
                "key_source": key_source,
                "models": [],
            }),
        );
    }
    let models = backend_test_model_ids(&body);
    let code = if models.is_empty() {
        "ok_no_models"
    } else {
        "ok"
    };
    let outcome = if models.is_empty() {
        "warning"
    } else {
        "success"
    };
    // Identify the server while we have the operator's attention, so the panel
    // can say what it found and flag an in-flight ceiling the server cannot
    // honour.
    //
    // Display only, deliberately. The windows themselves need no applying: the
    // health probe reads the profile's context endpoint every tick, so a
    // server reconfigured a moment ago is already current in the registry by
    // the time anyone reads this. What the button adds is an answer *before*
    // saving, against the address being typed rather than the one stored.
    // `None` means nothing answered — which the reachability test above has
    // already ruled out, but the panel should not invent a profile if it ever
    // did.
    let detected =
        gateway_core::server::upstreams::profile::detect(&state.http, base_url, key.as_deref())
            .await
            .unwrap_or_default();
    json_ok(
        StatusCode::OK,
        serde_json::json!({
            "outcome": outcome,
            "code": code,
            "model_count": models.len(),
            "models": models.into_iter().take(BACKEND_TEST_MAX_MODELS).collect::<Vec<_>>(),
            "key_source": key_source,
            "profile": detected.profile.as_str(),
            "detected_version": detected.version,
            "detected_max_parallel": detected.max_parallel,
            // The tightest window this server reports, or nothing when it
            // reports none. One number, because the only thing the panel ever
            // did with the list was take its minimum.
            "detected_context_window": detected
                .context_windows
                .values()
                .copied()
                .chain(detected.context_cap)
                .filter(|w| *w > 0)
                .min(),
        }),
    )
}

async fn backend_test_key(
    state: &RamaState,
    body: &BackendTestBody,
) -> (Option<String>, serde_json::Value) {
    if !body.api_key.trim().is_empty() {
        return (
            Some(body.api_key.trim().to_string()),
            serde_json::json!({"kind": "typed"}),
        );
    }
    if !body.name.trim().is_empty()
        && let Ok(Some(existing)) = upstreams_config::get_backend(&state.db, body.name.trim()).await
        && let (Some(ciphertext), Some(nonce)) = (existing.api_key_ct, existing.api_key_nonce)
        && let Ok(key) = state.crypto.open_str(&nonce, &ciphertext)
    {
        return (Some(key), serde_json::json!({"kind": "stored"}));
    }
    let env_name = body.api_key_env.trim();
    if !env_name.is_empty() {
        return match std::env::var(env_name) {
            Ok(key) if !key.is_empty() => (
                Some(key),
                serde_json::json!({"kind": "env", "name": env_name}),
            ),
            _ => (
                None,
                serde_json::json!({"kind": "env_unset", "name": env_name}),
            ),
        };
    }
    (None, serde_json::json!({"kind": "none"}))
}

/// The model ids a `/models` body advertises, by the same parser the health
/// probe uses.
///
/// Hand-rolling this a third time is how the Test panel came to list rows the
/// probe rejects — the panel would show a model the router would never route,
/// which is precisely the confusion the panel exists to prevent.
fn backend_test_model_ids(body: &[u8]) -> Vec<String> {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return Vec::new();
    };
    let Some((ids, _)) = gateway_core::server::upstreams::profile::read_models(&value) else {
        return Vec::new();
    };
    let mut models: Vec<String> = ids.into_iter().collect();
    models.sort();
    models
}

fn deepest_error(err: &(dyn std::error::Error + 'static)) -> String {
    let mut message = err.to_string();
    let mut source = err.source();
    while let Some(err) = source {
        message = err.to_string();
        source = err.source();
    }
    message
}

/// PUT /api/v0/admin/backends — upsert a backend row (sealed key, preserved
/// drain state) and set its pool membership. Marks the topology
/// dirty; the registry picks it up on the apply.
pub async fn backends_save(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = require_admin_json!(state, req);
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return bad_request(msg),
    };
    let parsed: BackendSaveBody = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => return bad_request(format!("parsing the backend body: {err}")),
    };
    let name = parsed.name.trim().to_string();
    if name.is_empty() {
        return bad_request("a backend needs a name");
    }
    if !parsed.overwrite
        && matches!(
            upstreams_config::backend_exists(&state.db, &name).await,
            Ok(true)
        )
    {
        return json_error(
            StatusCode::CONFLICT,
            "name_exists",
            &format!("a backend named {name} already exists — resend with overwrite to replace it"),
        );
    }
    if parsed.base_url.trim().is_empty() {
        return bad_request("a backend needs a base URL");
    }
    let (api_key_ct, api_key_nonce) = {
        let entered = parsed.api_key.trim();
        if !entered.is_empty() {
            match state.crypto.seal_str(entered) {
                Ok(s) => (Some(s.ciphertext), Some(s.nonce)),
                Err(err) => return internal(err),
            }
        } else {
            match upstreams_config::get_backend(&state.db, &name).await {
                Ok(Some(existing)) => (existing.api_key_ct, existing.api_key_nonce),
                _ => (None, None),
            }
        }
    };
    // Preserve the maintenance switch: a save must not undrain.
    let enabled = match upstreams_config::get_backend(&state.db, &name).await {
        Ok(Some(existing)) => existing.enabled,
        _ => true,
    };
    let row = BackendRow {
        name: name.clone(),
        base_url: parsed.base_url.trim().to_string(),
        api_key_env: (!parsed.api_key_env.trim().is_empty())
            .then(|| parsed.api_key_env.trim().to_string()),
        api_key_ct,
        api_key_nonce,
        weight: parsed.weight.max(1),
        max_inflight: parsed.max_inflight.max(1),
        health_path: if parsed.health_path.trim().is_empty() {
            "/models".into()
        } else {
            parsed.health_path.trim().to_string()
        },
        probe_models: parsed.probe_models,
        supports_edit: parsed.supports_edit,
        enabled,
        models: parsed.models,
        aliases: parsed
            .aliases
            .into_iter()
            .map(|a| AliasRow {
                alias: a.alias,
                target: a.target.filter(|t| !t.trim().is_empty()),
            })
            .collect(),
        created_at: jiff::Timestamp::now(),
        updated_at: jiff::Timestamp::now(),
    };
    if let Err(err) = upstreams_config::upsert_backend(&state.db, &row).await {
        return internal(err);
    }
    if let Err(err) =
        upstreams_config::set_backend_pool(&state.db, &name, parsed.pool.as_deref()).await
    {
        return internal(err);
    }
    let dirty = state.topology_dirty_bump();
    json_ok(
        StatusCode::OK,
        serde_json::json!({ "name": name, "dirty": dirty }),
    )
}

#[derive(serde::Deserialize)]
pub struct EnabledBody {
    pub enabled: bool,
}

/// POST /api/v0/admin/backends/{name}/enabled — drain/undrain. Mirrors the
/// form path: this one IS live (registry flip, no reload needed, no dirty).
pub async fn backends_enabled(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = require_admin_json!(state, req);
    // Raw URI, not the `Path` extractor: rama lowercases path segments and
    // never percent-decodes them, which would turn a backend name into something
    // that matches no row — reported as a drain that silently did nothing.
    let Some(name) = raw_path_segment(&req, 1) else {
        return bad_request("the URL is missing its backend name");
    };
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return bad_request(msg),
    };
    let parsed: EnabledBody = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => return bad_request(format!("parsing the enabled body: {err}")),
    };
    if let Err(err) = upstreams_config::set_backend_enabled(&state.db, &name, parsed.enabled).await
    {
        return internal(err);
    }
    state.upstreams.set_backend_enabled(&name, parsed.enabled);
    json_ok(
        StatusCode::OK,
        serde_json::json!({ "name": name, "enabled": parsed.enabled }),
    )
}

/// DELETE /api/v0/admin/backends/{name} — remove from the DB topology.
pub async fn backends_delete(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = require_admin_json!(state, req);
    // Raw URI, not the `Path` extractor: rama lowercases path segments and
    // never percent-decodes them, which would turn a backend name into something
    // that matches no row — reported as 204 No Content for a row that is still there.
    let Some(name) = raw_path_segment(&req, 0) else {
        return bad_request("the URL is missing its backend name");
    };
    if let Err(err) = upstreams_config::delete_backend(&state.db, &name).await {
        return internal(err);
    }
    let dirty = state.topology_dirty_bump();
    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .header("x-topology-dirty", dirty.to_string())
        .body(rama::http::Body::empty())
        .expect("static empty response")
}

#[derive(serde::Deserialize)]
pub struct RenameBody {
    /// The new name.
    pub name: String,
}

/// POST /api/v0/admin/backends/{name}/rename
///
/// A rename, not a save-under-a-new-name: `backends.name` is the primary key
/// and every child table names it, so saving the form with the name field
/// changed would leave the original behind and start a second, empty backend.
/// This moves the row and everything that points at it — including the usage
/// history, so an operator who renames because the same host now serves a
/// different model does not find their traffic split across two names.
pub async fn backends_rename(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    rename_topology_row(state, req, TopologyRow::Backend).await
}

/// POST /api/v0/admin/pools/{name}/rename — see [`backends_rename`].
pub async fn pools_rename(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    rename_topology_row(state, req, TopologyRow::Pool).await
}

#[derive(Clone, Copy)]
enum TopologyRow {
    Backend,
    Pool,
}

async fn rename_topology_row(state: Arc<RamaState>, req: Request, kind: TopologyRow) -> Response {
    let (_session, _admin) = require_admin_json!(state, req);
    // Segment 1, not 0: the path ends `/{name}/rename`. Read from the raw URI
    // for the reason `backends_delete` gives — rama lowercases and never
    // percent-decodes the matched segments.
    let Some(old) = raw_path_segment(&req, 1) else {
        return bad_request("the URL is missing its name");
    };
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return bad_request(msg),
    };
    let parsed: RenameBody = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => return bad_request(format!("parsing the rename body: {err}")),
    };
    let new = parsed.name.trim().to_string();
    if new.is_empty() {
        return bad_request("a name must not be empty");
    }
    let renamed = match kind {
        TopologyRow::Backend => upstreams_config::rename_backend(&state.db, &old, &new).await,
        TopologyRow::Pool => upstreams_config::rename_pool(&state.db, &old, &new).await,
    };
    let noun = match kind {
        TopologyRow::Backend => "backend",
        TopologyRow::Pool => "pool",
    };
    match renamed {
        Ok(upstreams_config::RenameOutcome::Renamed) => {
            let dirty = state.topology_dirty_bump();
            json_ok(
                StatusCode::OK,
                serde_json::json!({ "name": new, "dirty": dirty }),
            )
        }
        Ok(upstreams_config::RenameOutcome::NotFound) => json_error(
            StatusCode::NOT_FOUND,
            "not_found",
            &format!("no {noun} named {old}"),
        ),
        Ok(upstreams_config::RenameOutcome::Taken) => json_error(
            StatusCode::CONFLICT,
            "name_exists",
            &format!("a {noun} named {new} already exists"),
        ),
        Err(err) => internal(err),
    }
}

#[derive(serde::Deserialize)]
pub struct PoolSaveBody {
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub strategy: String,
    #[serde(default)]
    pub fallback_offline: Option<String>,
    #[serde(default)]
    pub compliance_gdpr: bool,
    #[serde(default)]
    pub compliance_nda: bool,
    #[serde(default)]
    pub enforce_limits: bool,
    #[serde(default)]
    pub sort_order: i64,
    #[serde(default)]
    pub allowed_groups: Vec<String>,
    #[serde(default)]
    pub backends: Vec<String>,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub voices: Vec<VoiceBody>,
    #[serde(default)]
    pub offer_voices: Vec<String>,
    #[serde(default)]
    pub overwrite: bool,
}

#[derive(serde::Deserialize)]
pub struct VoiceBody {
    pub lang: String,
    pub voice: String,
}

/// PUT /api/v0/admin/pools — upsert a pool row. Marks the topology dirty.
pub async fn pools_save(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = require_admin_json!(state, req);
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return bad_request(msg),
    };
    let parsed: PoolSaveBody = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => return bad_request(format!("parsing the pool body: {err}")),
    };
    let name = parsed.name.trim().to_string();
    if name.is_empty() {
        return bad_request("a pool needs a name");
    }
    if !parsed.overwrite
        && matches!(
            upstreams_config::pool_exists(&state.db, &name).await,
            Ok(true)
        )
    {
        return json_error(
            StatusCode::CONFLICT,
            "name_exists",
            &format!("a pool named {name} already exists — resend with overwrite to replace it"),
        );
    }
    const STRATEGIES: &[&str] = &["prefix_affinity", "least_inflight", "round_robin"];
    if !pool_kind_exists(&parsed.kind) {
        return bad_request(format!("unknown pool kind: {}", parsed.kind));
    }
    let strategy = if STRATEGIES.contains(&parsed.strategy.as_str()) {
        parsed.strategy
    } else {
        "least_inflight".into()
    };
    let row = PoolRow {
        name: name.clone(),
        kind: parsed.kind,
        strategy,
        fallback_offline: parsed.fallback_offline,
        compliance_gdpr: parsed.compliance_gdpr,
        compliance_nda: parsed.compliance_nda,
        enforce_limits: parsed.enforce_limits,
        sort_order: parsed.sort_order,
        allowed_groups: parsed.allowed_groups,
        backends: parsed.backends,
        models: parsed.models,
        voices: parsed
            .voices
            .into_iter()
            .map(|v| VoiceRow {
                lang_code: v.lang,
                voice_id: v.voice,
            })
            .collect(),
        offer_voices: parsed.offer_voices,
        created_at: jiff::Timestamp::now(),
        updated_at: jiff::Timestamp::now(),
    };
    if let Err(err) = upstreams_config::upsert_pool(&state.db, &row).await {
        return internal(err);
    }
    let dirty = state.topology_dirty_bump();
    json_ok(
        StatusCode::OK,
        serde_json::json!({ "name": name, "dirty": dirty }),
    )
}

/// Is this a pool kind the config loader accepts?
///
/// Asks the enum rather than a hand-kept list. A local list is how `rerank`
/// came to be rejected here while `PoolKind` had accepted it all along.
fn pool_kind_exists(kind: &str) -> bool {
    upstreams::config::PoolKind::ALL
        .iter()
        .any(|k| k.as_str() == kind)
}

/// DELETE /api/v0/admin/pools/{name}
pub async fn pools_delete(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = require_admin_json!(state, req);
    // Raw URI, not the `Path` extractor: rama lowercases path segments and
    // never percent-decodes them, which would turn a pool name into something
    // that matches no row — reported as 204 No Content for a row that is still there.
    let Some(name) = raw_path_segment(&req, 0) else {
        return bad_request("the URL is missing its pool name");
    };
    if let Err(err) = upstreams_config::delete_pool(&state.db, &name).await {
        return internal(err);
    }
    let dirty = state.topology_dirty_bump();
    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .header("x-topology-dirty", dirty.to_string())
        .body(rama::http::Body::empty())
        .expect("static empty response")
}

#[derive(serde::Deserialize)]
pub struct FallbackBody {
    pub kind: String,
    /// Empty clears the fallback.
    #[serde(default)]
    pub model: String,
}

/// PUT /api/v0/admin/upstreams/fallback — set/clear a kind's unknown-model
/// fallback. Live immediately: fallbacks are re-read per model miss.
pub async fn topology_fallback(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = require_admin_json!(state, req);
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return bad_request(msg),
    };
    let parsed: FallbackBody = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => return bad_request(format!("parsing the fallback body: {err}")),
    };
    if !pool_kind_exists(&parsed.kind) {
        return bad_request(format!("unknown pool kind: {}", parsed.kind));
    }
    let model = parsed.model.trim();
    let value = if model.is_empty() { None } else { Some(model) };
    if let Err(err) = upstreams_config::set_fallback(&state.db, &parsed.kind, value).await {
        return internal(err);
    }
    json_ok(
        StatusCode::OK,
        serde_json::json!({ "kind": parsed.kind, "model": value }),
    )
}

/// POST /api/v0/admin/upstreams/reload — apply the DB topology: rebuild the
/// registry (unsealing keys, carrying live model sets), respawn the health
/// probes, reset the dirty counter.
pub async fn topology_reload(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = require_admin_json!(state, req);
    let snapshot = match upstreams_config::load_snapshot(&state.db).await {
        Ok(s) => s,
        Err(err) => return internal(err),
    };
    if let Err(err) = state.upstreams.reload(&snapshot, &state.crypto) {
        return internal(err);
    }
    // Spawned, then awaited, so a client that disconnects mid-apply cannot
    // cancel it. `reload` has already published the new topology and retired
    // the old probe loops; dropping this future before it arms the new ones
    // would leave a live registry with no health probing at all — every
    // backend frozen at `healthy` and never re-checked, for the life of the
    // process.
    let armed = tokio::spawn(gateway_core::server::upstreams::health::spawn(
        state.upstreams.clone(),
        Some(state.db.clone()),
    ));
    if let Err(err) = armed.await {
        tracing::error!(error = %err, "arming the health probes after a topology apply panicked");
    }
    state.topology_dirty_reset();
    json_ok(
        StatusCode::OK,
        serde_json::json!({ "applied": true, "dirty": 0 }),
    )
}

/// A `HashSet` of model ids as a stable, sorted `Vec`.
fn sorted(set: std::collections::HashSet<String>) -> Vec<String> {
    let mut out: Vec<String> = set.into_iter().collect();
    out.sort();
    out
}

/// GET /api/v0/admin/upstreams/events — the live health stream as JSON
/// events: one `status` event per backend whose state changed since it was
/// last sent (plus the dirty counter inside each event), and a comment
/// keepalive when nothing has changed for a while. See `TICK` and `KEEPALIVE`
/// below for the cadence. The JSON twin of the legacy
/// `/admin/upstreams/live` HTML-patch stream.
pub async fn topology_events(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = require_admin_json!(state, req);
    let (tx, rx) =
        rama::futures::channel::mpsc::unbounded::<Result<rama::bytes::Bytes, std::io::Error>>();
    let state = state.clone();
    tokio::spawn(async move {
        use rama::futures::sink::SinkExt;
        use std::collections::HashMap;
        use std::time::Duration;

        let mut tx = tx;
        let mut last: HashMap<String, String> = HashMap::new();
        let mut since_send = Duration::ZERO;
        /// How often the in-memory registry is re-read for health flips. With
        /// the payload stable (see the sort below) a tick that changes nothing
        /// sends nothing, so this only bounds how stale a health flip can look
        /// — it is not what drives traffic.
        const TICK: Duration = Duration::from_secs(15);
        /// Idle gap after which a comment frame goes out, so a proxy with an
        /// idle timeout in front of the stream does not cut it. `since_send`
        /// advances in whole ticks, so this is reached at the first tick at or
        /// past it — keep it a multiple of `TICK`, or the effective gap is the
        /// next multiple up (a 20 s threshold on a 15 s tick fires at 30 s).
        const KEEPALIVE: Duration = Duration::from_secs(15);
        /// How often the rolling hour counts are re-aggregated. Slower than
        /// the tick, because an hour bucket does not move that fast.
        const USAGE_REFRESH: Duration = Duration::from_secs(60);

        // Both of these are re-read only when they can actually have changed:
        // the topology when the dirty counter moves, the usage aggregate on
        // its own slow cadence. A tick then touches only in-memory registry
        // state, and sends nothing unless something actually changed.
        let mut snapshot = match upstreams_config::load_snapshot(&state.db).await {
            Ok(s) => s,
            Err(_) => return,
        };
        let mut snapshot_dirty = state.topology_dirty_count();
        let mut usage =
            db::usage::recent_buckets_by_backend(&state.db, jiff::Timestamp::now(), 5, 12)
                .await
                .unwrap_or_default();
        let mut since_usage = Duration::ZERO;

        loop {
            let dirty = state.topology_dirty_count();
            if dirty != snapshot_dirty {
                if let Ok(s) = upstreams_config::load_snapshot(&state.db).await {
                    snapshot = s;
                }
                snapshot_dirty = dirty;
            }
            if since_usage >= USAGE_REFRESH {
                usage =
                    db::usage::recent_buckets_by_backend(&state.db, jiff::Timestamp::now(), 5, 12)
                        .await
                        .unwrap_or_default();
                since_usage = Duration::ZERO;
            }

            // Once per tick, not once per backend: `pools()` takes the
            // registry lock and allocates.
            let pools = state.upstreams.pools();
            let mut sent_any = false;
            for (name, backend) in &snapshot.backends {
                let live = pools.iter().find_map(|pool| {
                    pool.backends
                        .iter()
                        .find(|b| &b.name == name)
                        .map(|b| (pool.name.clone(), b))
                });
                let (healthy, enabled, auth_failed, inflight, max_inflight) = match &live {
                    Some((_, b)) => (
                        b.is_healthy(),
                        b.is_enabled(),
                        b.auth_failed(),
                        b.inflight(),
                        b.max_inflight,
                    ),
                    None => (false, backend.enabled, false, 0, backend.max_inflight),
                };
                let payload = serde_json::json!({
                    "name": name,
                    "pool": live.as_ref().map(|(p, _)| p),
                    "healthy": healthy,
                    "enabled": enabled,
                    "auth_failed": auth_failed,
                    "inflight": inflight,
                    "max_inflight": max_inflight,
                    "configured": true,
                    "dirty": dirty,
                    // Sorted, because these come out of a `HashSet` whose
                    // iteration order is randomized per process and reshuffles
                    // as probes rebuild it. Unsorted, the same models
                    // re-serialize differently on every tick: the list visibly
                    // reorders under the operator's cursor, and — worse — the
                    // `last` comparison below never matches, so the stream
                    // pushes an event per backend per tick forever. Sorting is
                    // what makes the change detection actually detect change.
                    "models": live.as_ref().map(|(_, backend)| sorted(backend.models_snapshot()))
                        .unwrap_or_default(),
                    "withheld": live.as_ref().map(|(_, backend)| sorted(backend.withheld_models()))
                        .unwrap_or_default(),
                    "usage": usage.get(name).cloned().unwrap_or_else(|| vec![0; 12]),
                    "requests_last_hour": usage.get(name).map(|v| v.iter().sum::<i64>()).unwrap_or(0),
                });
                let encoded = payload.to_string();
                if last.get(name) != Some(&encoded) {
                    last.insert(name.clone(), encoded.clone());
                    let frame = format!("event: status\ndata: {encoded}\n\n");
                    if tx.send(Ok(rama::bytes::Bytes::from(frame))).await.is_err() {
                        return;
                    }
                    sent_any = true;
                }
            }
            since_send = if sent_any {
                Duration::ZERO
            } else {
                since_send + TICK
            };
            if since_send >= KEEPALIVE {
                if tx
                    .send(Ok(rama::bytes::Bytes::from(": keepalive\n\n")))
                    .await
                    .is_err()
                {
                    return;
                }
                since_send = Duration::ZERO;
            }
            tokio::time::sleep(TICK).await;
            since_usage += TICK;
        }
    });
    session_core::chat_json::json_stream_response(rx)
}
