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
use gateway_runtime::rama_server::state::RamaState;

use super::{json_error, json_ok, require_admin_json};

fn bad_request(message: impl Into<String>) -> Response {
    json_error(StatusCode::BAD_REQUEST, "invalid_request", &message.into())
}

fn internal(message: impl std::fmt::Display) -> Response {
    json_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal_error",
        &message.to_string(),
    )
}

// ---------------------------------------------------------------------------
// Groups

/// GET /api/v0/admin/groups — the RBAC groups with their OIDC mappings,
/// tool grants, and skill grants, plus the pickers' option sets.
pub async fn groups_list(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = match require_admin_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
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
    let (_session, _admin) = match require_admin_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
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
pub async fn groups_delete(
    Path(name): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, _admin) = match require_admin_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
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

/// GET /api/v0/admin/users — every known user with their roles.
pub async fn users_list(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = match require_admin_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let all = match db::users::list_all(&state.db).await {
        Ok(v) => v,
        Err(err) => return internal(err),
    };
    let users: Vec<_> = all
        .into_iter()
        .map(|u| {
            serde_json::json!({
                "id": u.id,
                "email": u.email,
                "name": u.name,
                "roles": u.roles,
                "created_at": u.created_at.to_string(),
            })
        })
        .collect();
    json_ok(StatusCode::OK, serde_json::json!({ "users": users }))
}

/// POST /api/v0/admin/users/{id}/impersonate — mint an impersonation
/// session for the target and hand it back as a Set-Cookie (the SPA
/// reloads to become the target).
pub async fn users_impersonate(
    Path(user_id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    use gateway_core::rama_server::session::secure_cookies;
    use rama::http::header;

    let (session, admin) = match require_admin_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
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
        return json_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "You can't impersonate yourself.",
        );
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

    let (session, _target) = match super::require_session_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
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

    let (_session, _admin) = match require_admin_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let offered: Vec<String> = state.upstreams.all_models();
    let mut models = Vec::new();
    for name in &offered {
        let defaults = db::model_defaults::get(&state.db, name)
            .await
            .ok()
            .flatten();
        models.push(serde_json::json!({
            "name": name,
            "configured": defaults.is_some(),
            "defaults": defaults.map(|d| model_defaults_json(&d)),
        }));
    }
    // Configured-but-no-longer-offered rows keep their editor visible.
    if let Ok(all_configured) = db::model_defaults::all_names(&state.db).await {
        for name in all_configured {
            if !offered.contains(&name) {
                let defaults = db::model_defaults::get(&state.db, &name)
                    .await
                    .ok()
                    .flatten();
                models.push(serde_json::json!({
                    "name": name,
                    "configured": defaults.is_some(),
                    "defaults": defaults.map(|d| model_defaults_json(&d)),
                }));
            }
        }
    }
    let mut feature_defaults = Vec::new();
    for feature in [
        Feature::Chat,
        Feature::Transcription,
        Feature::Image,
        Feature::Embedding,
    ] {
        let model = feature_defaults::get(&state.db, feature).await;
        feature_defaults.push(serde_json::json!({
            "feature": feature.as_str(),
            "model": model,
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
    let (_session, _admin) = match require_admin_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
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
pub async fn models_delete(
    Path(name): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, _admin) = match require_admin_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
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

/// PUT /api/v0/admin/models/defaults — set/clear a feature's default model.
pub async fn models_feature_default(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    use gateway_core::server::feature_defaults::{self, Feature};

    let (_session, _admin) = match require_admin_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
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

/// PUT /api/v0/admin/models/search — the web-search provider settings.
pub async fn models_search_save(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    use gateway_features::server::search_settings::{self, SearchProvider};

    let (_session, _admin) = match require_admin_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
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
    let (_session, _admin) = match require_admin_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let rules = match limits::list_all(&state.db).await {
        Ok(v) => v,
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
            "roles": state.config().roles.iter().map(|r| r.id.clone()).collect::<Vec<String>>(),
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
    let (_session, _admin) = match require_admin_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
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
    // Subject validation mirrors the form path: roles must exist in config,
    // tokens in the DB, users by id or email.
    let subject_id = match subject_type {
        limits::SubjectType::Role => {
            if !state
                .config()
                .roles
                .iter()
                .any(|r| r.id == parsed.subject_id)
            {
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
    let (_session, _admin) = match require_admin_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
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
    let (_session, _admin) = match require_admin_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
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
                    serde_json::json!({
                        "key": f.key,
                        // SPA labels: derive from the key's last segment.
                        "kind": settings_kind(f.kind),
                        "restart": f.restart,
                        "value": effective.shown(f.key),
                        "secret_set": matches!(f.kind, settings::Kind::Secret)
                            && effective.secret_is_set(f.key),
                    })
                })
                .collect();
            serde_json::json!({
                "name": section.name,
                "category": section.category.slug(),
                "fields": fields,
            })
        })
        .collect();
    json_ok(
        StatusCode::OK,
        serde_json::json!({
            "sections": sections,
            "restart_pending": restart_pending,
        }),
    )
}

fn settings_kind(kind: settings::Kind) -> &'static str {
    match kind {
        settings::Kind::Bool => "bool",
        settings::Kind::Int => "int",
        settings::Kind::Float => "float",
        settings::Kind::Text => "text",
        settings::Kind::Path => "path",
        settings::Kind::Model(_) => "model",
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
    let (_session, _admin) = match require_admin_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
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
    if let Err(err) = settings::mark_imported(&state.db).await {
        tracing::warn!(error = %err, "recording that settings are operator-owned");
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
    let (_session, _admin) = match require_admin_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
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
    let (session, _admin) = match require_admin_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
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
    let (_session, _admin) = match require_admin_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
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
    let (_session, _admin) = match require_admin_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
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
            let entry = serde_json::json!({
                "healthy": backend.is_healthy(),
                "enabled": backend.is_enabled(),
                "auth_failed": backend.auth_failed(),
                "inflight": backend.inflight(),
                "max_inflight": backend.max_inflight,
                "models": backend.models_snapshot().into_iter().collect::<Vec<_>>(),
                "withheld": backend.withheld_models().into_iter().collect::<Vec<_>>(),
                "pool": pool.name,
            });
            live.insert(backend.name.clone(), entry);
        }
    }

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
            "usage_last_hour": usage,
            "dirty": state.topology_dirty_count(),
        }),
    )
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
    /// Pool membership to set (None = leave as-is; Some(None) = unassign).
    pub pool: Option<Option<String>>,
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

/// PUT /api/v0/admin/backends — upsert a backend row (sealed key, preserved
/// drain state) and optionally set its pool membership. Marks the topology
/// dirty; the registry picks it up on the apply.
pub async fn backends_save(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = match require_admin_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
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
    if let Some(pool) = parsed.pool
        && let Err(err) =
            upstreams_config::set_backend_pool(&state.db, &name, pool.as_deref()).await
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
pub async fn backends_enabled(
    Path(name): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, _admin) = match require_admin_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
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
pub async fn backends_delete(
    Path(name): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, _admin) = match require_admin_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
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
    let (_session, _admin) = match require_admin_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
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
    const KINDS: &[&str] = &[
        "chat",
        "transcription",
        "embedding",
        "image",
        "speech",
        "ocr",
    ];
    const STRATEGIES: &[&str] = &["prefix_affinity", "least_inflight", "round_robin"];
    if !KINDS.contains(&parsed.kind.as_str()) {
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

/// DELETE /api/v0/admin/pools/{name}
pub async fn pools_delete(
    Path(name): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, _admin) = match require_admin_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
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
    let (_session, _admin) = match require_admin_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return bad_request(msg),
    };
    let parsed: FallbackBody = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => return bad_request(format!("parsing the fallback body: {err}")),
    };
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
    let (_session, _admin) = match require_admin_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let snapshot = match upstreams_config::load_snapshot(&state.db).await {
        Ok(s) => s,
        Err(err) => return internal(err),
    };
    if let Err(err) = state.upstreams.reload(&snapshot, &state.crypto) {
        return internal(err);
    }
    gateway_core::server::upstreams::health::spawn(state.upstreams.clone(), Some(state.db.clone()))
        .await;
    state.topology_dirty_reset();
    json_ok(
        StatusCode::OK,
        serde_json::json!({ "applied": true, "dirty": 0 }),
    )
}

/// GET /api/v0/admin/upstreams/events — the live health stream as JSON
/// events: every 2 s, one `status` event per backend whose state changed
/// since it was last sent (plus the dirty counter inside each event), a
/// comment keepalive every 20 s. The JSON twin of the legacy
/// `/admin/upstreams/live` HTML-patch stream.
pub async fn topology_events(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = match require_admin_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
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
        const TICK: Duration = Duration::from_secs(2);
        const KEEPALIVE: Duration = Duration::from_secs(20);

        loop {
            let snapshot = match upstreams_config::load_snapshot(&state.db).await {
                Ok(s) => s,
                Err(_) => {
                    tokio::time::sleep(TICK).await;
                    continue;
                }
            };
            let now = jiff::Timestamp::now();
            let usage = db::usage::recent_buckets_by_backend(&state.db, now, 5, 12)
                .await
                .unwrap_or_default();
            let dirty = state.topology_dirty_count();

            let mut sent_any = false;
            for (name, backend) in &snapshot.backends {
                let pools = state.upstreams.pools();
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
        }
    });
    Response::builder()
        .status(StatusCode::OK)
        .header(rama::http::header::CONTENT_TYPE, "text/event-stream")
        .header(rama::http::header::CACHE_CONTROL, "no-cache")
        .header("x-accel-buffering", "no")
        .body(rama::http::Body::from_stream(rx))
        .expect("static SSE response")
}
