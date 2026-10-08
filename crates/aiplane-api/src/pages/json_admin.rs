// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The `/api/v0/admin/*` JSON surface for the SvelteKit SPA: groups, users, model defaults/prices, limits, settings, the
//! admin token register, and the upstream topology. One module because the
//! legacy form handlers' DB calls live one crate down in `aiplane-core` —
//! these handlers are thin JSON translations of them, and the legacy
//! pages stay alive beside everything until phase 6 removes them.
//!
//! Contract notes:
//! * Every handler is admin-gated ([`require_admin_json`] → 401/403 JSON).
//! * Writes that have live side effects replicate them exactly:
//!   `reload_rbac` after group changes, `reload_settings` + session-policy
//!   push after settings saves, the registry reload + health respawn +
//!   dirty reset for topology applies.

use aiplane_core::server::capped_read;
use std::sync::Arc;

use rama::http::service::web::extract::{Path, State};
use rama::http::{Request, Response, StatusCode};

use aiplane_core::server::db;
use aiplane_core::server::db::limits;
use aiplane_core::server::settings;
use aiplane_core::server::upstreams::{self, PoolKind};
use aiplane_runtime::rama_server::state::RamaState;
use session_core::i18n::{Lang, t};

use super::{
    bad_request, internal, json_error, json_error_with, json_ok, not_found, raw_path_segment,
};

// ---------------------------------------------------------------------------
// Groups

/// GET /api/v0/admin/groups — the RBAC groups with their OIDC mappings,
/// tool grants, and skill grants, plus the pickers' option sets.
pub async fn groups_list(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    use aiplane_runtime::server::tools::catalog::{Category, category_for};

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
        groups.push(GroupView {
            name: g.name,
            description: g.description,
            is_admin: g.is_admin,
            is_default: g.is_default,
            can_manage_agents: g.can_manage_agents,
            oidc_values,
            tools,
            skills,
        });
    }
    let observed_oidc_values = db::gateway_groups::observed_oidc_values(&state.db)
        .await
        .unwrap_or_default();
    // Each grantable id with the section it belongs in, so the grant matrix can
    // group a long list the way `/tools` and `/tokens` already group theirs —
    // same `Category`, same ordering, no second taxonomy.
    let tools = state
        .grantable_tool_ids()
        .into_iter()
        .map(|id| {
            let category = category_for(&id);
            GrantableTool {
                id,
                category: category.key(),
                order: category.order(),
            }
        })
        .collect();
    // Family grants and the MCP tools a connector was seen to expose. The tool
    // cache is best-effort — a connector nobody has connected yet contributes
    // nothing, which the editor reports rather than showing an empty list.
    let tool_families = state
        .grantable_tool_families()
        .await
        .into_iter()
        .map(|(id, subject)| ToolFamily { id, subject })
        .collect();
    let mcp_tools = db::mcp_catalog::all_tools(&state.db)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|(connector, tool)| GrantableMcpTool {
            id: tool.tool_id,
            connector,
            description: tool.description,
            category: Category::Integrations.key(),
            order: Category::Integrations.order(),
        })
        .collect();
    let skill_names: Vec<String> = state
        .skills()
        .as_ref()
        .map(|s| s.current().names().map(|n| n.to_string()).collect())
        .unwrap_or_default();
    json_ok(
        StatusCode::OK,
        GroupsView {
            groups,
            observed_oidc_values,
            tools,
            tool_families,
            mcp_tools,
            skill_names,
        },
    )
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct GroupsView {
    pub groups: Vec<GroupView>,
    /// Every OIDC group claim value seen on a user's sign-in so far.
    pub observed_oidc_values: Vec<String>,
    /// Every tool id a group can be granted.
    pub tools: Vec<GrantableTool>,
    /// Tool families granted as a whole, with the subject the grant covers.
    pub tool_families: Vec<ToolFamily>,
    /// The MCP tools connectors were seen to expose.
    pub mcp_tools: Vec<GrantableMcpTool>,
    /// Every installed skill a group can be granted.
    pub skill_names: Vec<String>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct GroupView {
    pub name: String,
    pub description: String,
    pub is_admin: bool,
    pub is_default: bool,
    pub can_manage_agents: bool,
    /// The OIDC group claim values mapped to this group.
    pub oidc_values: Vec<String>,
    /// The granted tool ids (`*` grants every tool).
    pub tools: Vec<String>,
    pub skills: Vec<String>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct GrantableTool {
    pub id: String,
    /// The catalog section the tool is listed under.
    pub category: &'static str,
    /// The section's position in the catalog.
    pub order: u8,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ToolFamily {
    pub id: String,
    pub subject: String,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct GrantableMcpTool {
    pub id: String,
    pub connector: String,
    pub description: String,
    pub category: &'static str,
    pub order: u8,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct GroupSaveBody {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub is_admin: bool,
    #[serde(default)]
    pub is_default: bool,
    /// `None` leaves the flag as it is, so an editor that predates it cannot
    /// clear it by saving.
    #[serde(default)]
    pub can_manage_agents: Option<bool>,
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
    if let Some(enabled) = parsed.can_manage_agents
        && let Err(err) = db::gateway_groups::set_can_manage_agents(&state.db, &name, enabled).await
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
    json_ok(StatusCode::OK, GroupSaved { name })
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct GroupSaved {
    /// The saved group's name, trimmed.
    pub name: String,
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
    match db::gateway_groups::delete_group(&state.db, &name).await {
        Ok(true) => {}
        Ok(false) => return not_found(format!("no group `{name}`")),
        Err(err) => return internal(err),
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
    let users = all
        .into_iter()
        .map(|u| UserView {
            gateway_roles: state.rbac.role_ids_for(&u.roles),
            id: u.id,
            email: u.email,
            name: u.name,
            oidc_groups: u.roles,
            created_at: u.created_at.to_string(),
        })
        .collect();
    let audit = audit
        .into_iter()
        .map(|event| ImpersonationAuditEntry {
            id: event.id,
            action: event.action,
            actor_email: event.actor_email,
            target_email: event.target_email,
            created_at: event.created_at.to_string(),
        })
        .collect();
    json_ok(
        StatusCode::OK,
        UsersView {
            users,
            audit,
            current_user_id: admin.id,
            allow_impersonation: state.config().gateway.allow_impersonation,
        },
    )
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct UsersView {
    pub users: Vec<UserView>,
    /// The most recent impersonation starts and stops, newest first.
    pub audit: Vec<ImpersonationAuditEntry>,
    pub current_user_id: String,
    pub allow_impersonation: bool,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct UserView {
    pub id: String,
    pub email: String,
    pub name: Option<String>,
    /// The group claim values from the user's last sign-in.
    pub oidc_groups: Vec<String>,
    /// The gateway groups those claim values map to.
    pub gateway_roles: Vec<String>,
    pub created_at: String,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ImpersonationAuditEntry {
    pub id: String,
    pub action: String,
    pub actor_email: String,
    pub target_email: String,
    pub created_at: String,
}

/// POST /api/v0/admin/users/{id}/impersonate — mint an impersonation
/// session for the target and hand it back as a Set-Cookie (the SPA
/// reloads to become the target).
pub async fn users_impersonate(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    use aiplane_core::rama_server::session::secure_cookies;
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
            serde_json::to_string(&Impersonating {
                ok: true,
                user_id: target.id,
                email: target.email,
            })
            .expect("wire types serialize to JSON")
            .into(),
        )
        .expect("static JSON response")
}

/// The impersonation session is set as the session cookie.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct Impersonating {
    pub ok: bool,
    /// The impersonated user.
    pub user_id: String,
    pub email: String,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ImpersonationStopped {
    pub ok: bool,
    /// `false` when the session was not impersonating anyone.
    pub stopped: bool,
}

/// POST /api/v0/admin/impersonate/stop — end an impersonation, restore the
/// admin session (Set-Cookie). Not admin-gated: the live identity is the
/// target. A no-op for an ordinary session.
pub async fn impersonate_stop(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    use aiplane_core::rama_server::session::secure_cookies;
    use rama::http::header;

    let (session, _target) = require_session_json!(state, req);
    let Some(admin_id) = session.impersonator_id.clone() else {
        return json_ok(
            StatusCode::OK,
            ImpersonationStopped {
                ok: true,
                stopped: false,
            },
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
            serde_json::to_string(&ImpersonationStopped {
                ok: true,
                stopped: true,
            })
            .expect("wire types serialize to JSON")
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
    use aiplane_core::server::feature_defaults::{self, Feature};

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
            let serving = state.upstreams.serving_profile(
                &name,
                kind,
                &aiplane_core::server::upstreams::PoolAccess::all(),
            );
            let reasoning = aiplane_core::server::reasoning::ReasoningStyle::resolve(
                defaults
                    .as_ref()
                    .and_then(|value| value.reasoning_style.as_deref()),
                serving.dialect,
                &name,
            );
            models.push(ModelView {
                // What the serving backend says this model's context is, if it
                // says anything. The page puts it beside the operator's own
                // value: equal or larger is the ordinary case, *smaller* means
                // the configured figure will not be compacted but silently
                // truncated upstream, and that is worth saying out loud.
                detected_context_window: state.upstreams.probed_context_window(&name),
                name,
                kind: kind_label,
                alias_target,
                configured: defaults.is_some(),
                resolved_reasoning_style: reasoning.as_str(),
                uses_token_budget: reasoning.budget_enforced(serving.thinking_budget),
                effort_levels: reasoning.effort_levels(),
                defaults: defaults.map(|d| model_defaults_view(&d)),
            });
        }
    }
    let mut leftover: Vec<_> = configured.into_values().collect();
    leftover.sort_by(|a, b| a.model_name.cmp(&b.model_name));
    for defaults in leftover {
        // These are rows for models nothing currently serves, so there is no
        // backend to ask — the model name decides, as it always did.
        let reasoning = aiplane_core::server::reasoning::ReasoningStyle::resolve(
            defaults.reasoning_style.as_deref(),
            None,
            &defaults.model_name,
        );
        models.push(ModelView {
            name: defaults.model_name.clone(),
            kind: "chat",
            alias_target: None,
            configured: true,
            resolved_reasoning_style: reasoning.as_str(),
            // Nothing serves this model, so only a budget in the model's own
            // wire format can hold.
            uses_token_budget: reasoning.budget_enforced(None),
            effort_levels: reasoning.effort_levels(),
            // Nothing serves this model, so nothing has reported a window.
            detected_context_window: None,
            defaults: Some(model_defaults_view(&defaults)),
        });
    }
    let mut feature_defaults = Vec::new();
    for feature in Feature::ALL {
        let model = feature_defaults::get(&state.db, feature).await;
        let mut available = state.upstreams.models_for_kind(feature.pool_kind());
        available.sort();
        if available.is_empty() {
            continue;
        }
        feature_defaults.push(FeatureDefaultView {
            feature: feature.as_str(),
            model,
            available,
        });
    }
    let search =
        match aiplane_features::server::search_settings::view(&state.db, &state.crypto).await {
            Ok(v) => v,
            Err(err) => return internal(err),
        };
    json_ok(
        StatusCode::OK,
        ModelsView {
            models,
            all_models: state.upstreams.all_models(),
            currency: state.config().usage.currency.clone(),
            feature_defaults,
            search: SearchSettingsView {
                provider: search.provider.as_str(),
                searxng_url: search.searxng_url,
                brave_key_set: search.brave_key_set,
                tavily_key_set: search.tavily_key_set,
                tavily_enabled: search.tavily_enabled,
                tavily_active: search.tavily_active,
            },
        },
    )
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ModelsView {
    /// Every served model, then the stored overrides of models nothing serves.
    pub models: Vec<ModelView>,
    pub all_models: Vec<String>,
    /// The currency usage costs are counted in.
    pub currency: String,
    /// The features a default model can be chosen for, where any model can serve them.
    pub feature_defaults: Vec<FeatureDefaultView>,
    pub search: SearchSettingsView,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ModelView {
    pub name: String,
    /// The pool kind serving it, e.g. `chat`, `embedding`.
    pub kind: &'static str,
    /// The model this name is an alias of.
    pub alias_target: Option<String>,
    /// The model has stored overrides.
    pub configured: bool,
    pub resolved_reasoning_style: &'static str,
    /// Whether a thinking-token budget for this model would be enforced: the
    /// model's wire format carries one (Anthropic), or every serving backend
    /// enforces one (vLLM, SGLang with `--enable-strict-thinking`).
    pub uses_token_budget: bool,
    pub effort_levels: &'static [&'static str],
    /// The context window the serving backend reports.
    pub detected_context_window: Option<i64>,
    pub defaults: Option<ModelDefaultsView>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ModelDefaultsView {
    /// The request defaults as the operator typed them, in TOML.
    pub defaults_toml: String,
    pub reasoning_style: Option<String>,
    pub context_window: Option<i64>,
    pub input_price: Option<f64>,
    pub output_price: Option<f64>,
    pub pricing_unit: &'static str,
    pub budget_low: Option<i64>,
    pub budget_medium: Option<i64>,
    pub budget_high: Option<i64>,
    pub budget_xhigh: Option<i64>,
    pub effort_low: Option<String>,
    pub effort_medium: Option<String>,
    pub effort_high: Option<String>,
    pub effort_xhigh: Option<String>,
    pub capabilities: ModelCapabilitiesView,
}

/// `null` = unknown, `true`/`false` = confirmed. A `fallback_*` names the
/// model that stands in when this one lacks the capability.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ModelCapabilitiesView {
    pub vision: Option<bool>,
    pub audio_input: Option<bool>,
    pub pdf_input: Option<bool>,
    pub tools: Option<bool>,
    pub parallel_tools: Option<bool>,
    pub structured_output: Option<bool>,
    pub fallback_vision: Option<String>,
    pub fallback_tools: Option<String>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct FeatureDefaultView {
    pub feature: &'static str,
    /// The chosen default; `null` when none is set.
    pub model: Option<String>,
    pub available: Vec<String>,
}

/// The web-search settings. Stored keys are never returned, only whether one is set.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct SearchSettingsView {
    pub provider: &'static str,
    pub searxng_url: Option<String>,
    pub brave_key_set: bool,
    pub tavily_key_set: bool,
    pub tavily_enabled: bool,
    pub tavily_active: bool,
}

fn model_defaults_view(d: &db::model_defaults::ModelDefaults) -> ModelDefaultsView {
    ModelDefaultsView {
        defaults_toml: d.defaults_toml.clone(),
        reasoning_style: d.reasoning_style.clone(),
        context_window: d.context_window,
        input_price: d.input_price,
        output_price: d.output_price,
        pricing_unit: d.pricing_unit.as_str(),
        budget_low: d.thinking_budget_low,
        budget_medium: d.thinking_budget_medium,
        budget_high: d.thinking_budget_high,
        budget_xhigh: d.thinking_budget_xhigh,
        effort_low: d.reasoning_effort_low.clone(),
        effort_medium: d.reasoning_effort_medium.clone(),
        effort_high: d.reasoning_effort_high.clone(),
        effort_xhigh: d.reasoning_effort_xhigh.clone(),

        capabilities: ModelCapabilitiesView {
            vision: d.capabilities.vision,
            audio_input: d.capabilities.audio_input,
            pdf_input: d.capabilities.pdf_input,
            tools: d.capabilities.tools,
            parallel_tools: d.capabilities.parallel_tools,
            structured_output: d.capabilities.structured_output,
            fallback_vision: d.capabilities.fallback_vision.clone(),
            fallback_tools: d.capabilities.fallback_tools.clone(),
        },
    }
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
    // Through a map first, as the form always was: an object is required, and
    // a repeated key's last value wins.
    let raw = match serde_json::from_slice::<serde_json::Map<String, serde_json::Value>>(&bytes)
        .map(serde_json::Value::Object)
        .and_then(serde_json::from_value::<ModelSaveBody>)
    {
        Ok(v) => v,
        Err(err) => return bad_request(format!("parsing the model body: {err}")),
    };
    let form = super::admin::SaveForm {
        model_name: form_field(raw.model_name),
        input_price: form_field(raw.input_price),
        output_price: form_field(raw.output_price),
        pricing_unit: form_field(raw.pricing_unit),
        price_only: form_field(raw.price_only),
        context_window: form_field(raw.context_window),
        reasoning_style: form_field(raw.reasoning_style),
        budget_low: form_field(raw.budget_low),
        budget_medium: form_field(raw.budget_medium),
        budget_high: form_field(raw.budget_high),
        budget_xhigh: form_field(raw.budget_xhigh),
        effort_low: form_field(raw.effort_low),
        effort_medium: form_field(raw.effort_medium),
        effort_high: form_field(raw.effort_high),
        effort_xhigh: form_field(raw.effort_xhigh),

        cap_vision: form_field(raw.cap_vision),
        cap_audio_input: form_field(raw.cap_audio_input),
        cap_pdf_input: form_field(raw.cap_pdf_input),
        cap_tools: form_field(raw.cap_tools),
        cap_parallel_tools: form_field(raw.cap_parallel_tools),
        cap_structured_output: form_field(raw.cap_structured_output),
        fallback_vision: form_field(raw.fallback_vision),
        fallback_tools: form_field(raw.fallback_tools),
        defaults_toml: form_field(raw.defaults_toml),
    };
    match super::admin::apply_model_form(&state, &form).await {
        Ok(()) => json_ok(
            StatusCode::OK,
            ModelSaved {
                model: form.model_name,
            },
        ),
        Err(super::admin::ModelFormError::Invalid(message)) => bad_request(message),
        Err(super::admin::ModelFormError::Store(err)) => internal(err),
    }
}

/// One model's overrides, field for field the admin form's. Every field is a
/// string and blank clears it; another JSON value is read as its JSON text,
/// and `null` or an absent field as blank.
#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct ModelSaveBody {
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    model_name: Option<serde_json::Value>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    input_price: Option<serde_json::Value>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    output_price: Option<serde_json::Value>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pricing_unit: Option<serde_json::Value>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    price_only: Option<serde_json::Value>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    context_window: Option<serde_json::Value>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    reasoning_style: Option<serde_json::Value>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    budget_low: Option<serde_json::Value>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    budget_medium: Option<serde_json::Value>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    budget_high: Option<serde_json::Value>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    budget_xhigh: Option<serde_json::Value>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    effort_low: Option<serde_json::Value>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    effort_medium: Option<serde_json::Value>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    effort_high: Option<serde_json::Value>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    effort_xhigh: Option<serde_json::Value>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    cap_vision: Option<serde_json::Value>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    cap_audio_input: Option<serde_json::Value>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    cap_pdf_input: Option<serde_json::Value>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    cap_tools: Option<serde_json::Value>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    cap_parallel_tools: Option<serde_json::Value>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    cap_structured_output: Option<serde_json::Value>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    fallback_vision: Option<serde_json::Value>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    fallback_tools: Option<serde_json::Value>,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    defaults_toml: Option<serde_json::Value>,
}

fn form_field(value: Option<serde_json::Value>) -> String {
    match value {
        Some(serde_json::Value::String(s)) => s,
        Some(v) if !v.is_null() => v.to_string(),
        _ => String::new(),
    }
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ModelSaved {
    pub model: String,
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
        Ok(true) => Response::builder()
            .status(StatusCode::NO_CONTENT)
            .body(rama::http::Body::empty())
            .expect("static empty response"),
        Ok(false) => not_found(format!("`{name}` has no stored overrides")),
        Err(err) => internal(err),
    }
}

pub async fn automatic_routes_list(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = require_admin_json!(state, req);
    let lang = Lang::from_request(req.headers());
    let routes = match db::automatic_routes::all(&state.db).await {
        Ok(routes) => routes,
        Err(error) => {
            tracing::error!(error = %error, "listing automatic routes failed");
            return internal(t(lang, "auto-route-error-internal"));
        }
    };
    let decisions = match db::automatic_routes::recent_decisions(&state.db, 100).await {
        Ok(decisions) => decisions,
        Err(error) => {
            tracing::error!(error = %error, "listing automatic route decisions failed");
            return internal(t(lang, "auto-route-error-internal"));
        }
    };
    json_ok(
        StatusCode::OK,
        AutomaticRoutesView {
            routes,
            candidate_models: state.upstreams.models_for_kind(PoolKind::Chat),
            selector_models: state.upstreams.models_for_kind(PoolKind::SystemOne),
            decisions,
        },
    )
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct AutomaticRoutesView {
    pub routes: Vec<db::automatic_routes::AutomaticRoute>,
    /// The chat models a route may choose between.
    pub candidate_models: Vec<String>,
    /// The models that can make the choice.
    pub selector_models: Vec<String>,
    /// The most recent routing decisions, newest first.
    pub decisions: Vec<db::automatic_routes::AutomaticRouteDecisionRow>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct AutomaticRouteSaved {
    pub alias: String,
    /// The route's version after this save.
    pub version: i64,
}

pub async fn automatic_routes_save(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = require_admin_json!(state, req);
    let (parts, body) = req.into_parts();
    let lang = Lang::from_request(&parts.headers);
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(bytes) => bytes,
        Err(_) => return bad_request(t(lang, "auto-route-error-invalid-body")),
    };
    let route: db::automatic_routes::AutomaticRoute = match serde_json::from_slice(&bytes) {
        Ok(route) => route,
        Err(_) => return bad_request(t(lang, "auto-route-error-invalid-body")),
    };
    if route.validate().is_err() {
        return bad_request(t(lang, "auto-route-error-invalid-config"));
    }
    for candidate in &route.candidates {
        match db::automatic_routes::get(&state.db, &candidate.target).await {
            Ok(Some(_)) => {
                return bad_request(t(lang, "auto-route-error-nested"));
            }
            Ok(None) => {}
            Err(error) => {
                tracing::error!(error = %error, "validating automatic route candidates failed");
                return internal(t(lang, "auto-route-error-internal"));
            }
        }
    }
    match db::automatic_routes::upsert(&state.db, &route).await {
        Ok(version) => {
            state.automatic_router.invalidate_alias(&route.alias);
            json_ok(
                StatusCode::OK,
                AutomaticRouteSaved {
                    alias: route.alias,
                    version,
                },
            )
        }
        Err(error) => {
            tracing::error!(error = %error, "saving automatic route failed");
            let key = if error.to_string().contains("nested automatic routes") {
                "auto-route-error-nested"
            } else {
                "auto-route-error-internal"
            };
            if key == "auto-route-error-nested" {
                bad_request(t(lang, key))
            } else {
                internal(t(lang, key))
            }
        }
    }
}

pub async fn automatic_routes_delete(
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, _admin) = require_admin_json!(state, req);
    let lang = Lang::from_request(req.headers());
    let Some(alias) = raw_path_segment(&req, 0) else {
        return bad_request(t(lang, "auto-route-error-missing-alias"));
    };
    match db::automatic_routes::delete(&state.db, &alias).await {
        Ok(true) => {
            state.automatic_router.invalidate_alias(&alias);
            Response::builder()
                .status(StatusCode::NO_CONTENT)
                .body(rama::http::Body::empty())
                .expect("static empty response")
        }
        Ok(false) => json_error(
            StatusCode::NOT_FOUND,
            "not_found",
            &t(lang, "auto-route-error-not-found"),
        ),
        Err(error) => {
            tracing::error!(error = %error, "deleting automatic route failed");
            internal(t(lang, "auto-route-error-internal"))
        }
    }
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct FeatureDefaultBody {
    pub feature: String,
    /// Empty string clears the override.
    pub model: String,
}

/// PUT /api/v0/admin/model-defaults — set/clear a feature's default model.
pub async fn models_feature_default(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    use aiplane_core::server::feature_defaults::{self, Feature};

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
            FeatureDefaultSaved {
                model: value.map(str::to_string),
                feature: parsed.feature,
            },
        ),
        Err(err) => internal(err),
    }
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct FeatureDefaultSaved {
    pub feature: String,
    /// `null` when the default was cleared.
    pub model: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct SearchSettingsBody {
    pub provider: String,
    #[serde(default)]
    pub searxng_url: String,
    /// Blank = keep the stored key (write-only secret).
    #[serde(default)]
    pub brave_api_key: String,
    #[serde(default)]
    pub clear_brave_key: bool,
    #[serde(default)]
    pub tavily_api_key: String,
    #[serde(default)]
    pub clear_tavily_key: bool,
    pub tavily_enabled: Option<bool>,
}

/// PUT /api/v0/admin/search-settings — the web-search provider settings.
pub async fn models_search_save(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    use aiplane_features::server::search_settings::{self, SearchProvider};

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
    let key = parsed.tavily_api_key.trim();
    if key.is_empty() {
        if parsed.clear_tavily_key
            && let Err(err) = search_settings::set_tavily_key(&state.db, &state.crypto, "").await
        {
            return internal(err);
        }
    } else if let Err(err) = search_settings::set_tavily_key(&state.db, &state.crypto, key).await {
        return internal(err);
    }
    if let Some(enabled) = parsed.tavily_enabled
        && let Err(err) = search_settings::set_tavily_enabled(&state.db, enabled).await
    {
        return internal(err);
    }
    json_ok(StatusCode::OK, super::Done::OK)
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
    let roles: Vec<String> = match db::gateway_groups::list_group_names(&state.db).await {
        Ok(names) => names,
        Err(err) => return internal(err),
    };
    let users = db::users::list_all(&state.db)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|u| LimitUserOption {
            id: u.id,
            email: u.email,
        })
        .collect();
    let tokens = db::tokens::list_all_with_owner(&state.db)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|t| LimitTokenOption {
            id: t.id,
            name: t.name,
            owner: t.user_email,
        })
        .collect();
    let agents = match aiplane_agents::db::agents::list_all(&state.db).await {
        Ok(rows) => rows
            .into_iter()
            .map(|a| {
                let name = if a.principal.display.is_empty() {
                    a.principal.name
                } else {
                    a.principal.display
                };
                LimitAgentOption {
                    id: a.principal.id,
                    name,
                }
            })
            .collect(),
        Err(err) => return internal(err),
    };
    json_ok(
        StatusCode::OK,
        LimitsView {
            limits: rules.iter().map(limit_view).collect(),
            users,
            tokens,
            agents,
            roles,
            models: state.upstreams.all_models(),
            currency: state.config().usage.currency.clone(),
        },
    )
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct LimitsView {
    pub limits: Vec<LimitView>,
    pub users: Vec<LimitUserOption>,
    pub tokens: Vec<LimitTokenOption>,
    pub agents: Vec<LimitAgentOption>,
    /// The gateway group names a rule can name.
    pub roles: Vec<String>,
    pub models: Vec<String>,
    pub currency: String,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct LimitUserOption {
    pub id: String,
    pub email: String,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct LimitTokenOption {
    pub id: String,
    pub name: String,
    /// The owner's email, or their id when the user no longer exists.
    pub owner: String,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct LimitAgentOption {
    pub id: String,
    pub name: String,
}

#[derive(Clone, serde::Serialize, schemars::JsonSchema)]
pub struct LimitView {
    pub id: String,
    pub subject_type: &'static str,
    pub subject_id: String,
    /// `null` = all metered models together.
    pub model: Option<String>,
    pub dimension: &'static str,
    pub window: &'static str,
    pub value: f64,
    /// Who set the rule, which decides who may change it.
    pub managed_by: &'static str,
}

fn limit_view(rule: &limits::LimitRule) -> LimitView {
    LimitView {
        id: rule.id.clone(),
        subject_type: rule.subject_type.as_str(),
        subject_id: rule.subject_id.clone(),
        model: rule.model.clone(),
        dimension: rule.dimension.as_str(),
        window: rule.window.as_str(),
        value: rule.value,
        managed_by: rule.managed_by.as_str(),
    }
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct LimitBody {
    /// Omitted when creating a new rule; present when replacing a row from
    /// the admin editor.
    #[serde(default)]
    pub id: Option<String>,
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
            let known = db::gateway_groups::list_group_names(&state.db)
                .await
                .unwrap_or_default();
            if !known.contains(&parsed.subject_id) {
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
        limits::SubjectType::System => {
            match aiplane_agents::db::agents::get(&state.db, &parsed.subject_id).await {
                Ok(Some(a)) => a.principal.id,
                Ok(None) => {
                    return bad_request(format!(
                        "unknown agent: {} — give the agent's id as shown on its page",
                        parsed.subject_id
                    ));
                }
                Err(err) => return internal(err),
            }
        }
        limits::SubjectType::Global => String::new(),
        limits::SubjectType::AgentSpec => {
            return bad_request(
                "an agent's own budget is set in its spec (`publish.budget`), not here — use \
                 subject type `system` to cap an agent",
            );
        }
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
    let saved = match parsed.id.as_deref() {
        Some(id) => match limits::update(
            &state.db,
            id,
            limits::LimitUpdate {
                subject_type,
                subject_id: &subject_id,
                model,
                dimension,
                window,
                value: parsed.value,
            },
        )
        .await
        {
            Ok(true) => Ok(()),
            Ok(false) => Err(not_found("the limit no longer exists")),
            Err(err) => Err(internal(err)),
        },
        None => limits::upsert(
            &state.db,
            subject_type,
            &subject_id,
            model,
            dimension,
            window,
            parsed.value,
        )
        .await
        .map_err(internal),
    };
    match saved {
        Ok(()) => json_ok(StatusCode::OK, super::Done::OK),
        Err(response) => response,
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
        Ok(true) => Response::builder()
            .status(StatusCode::NO_CONTENT)
            .body(rama::http::Body::empty())
            .expect("static empty response"),
        Ok(false) => not_found("no such limit"),
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
    let sections = settings::SECTIONS
        .iter()
        .map(|section| {
            let fields = section
                .fields
                .iter()
                .map(|f| {
                    let (models, model_options) = match f.kind {
                        settings::Kind::Model(kind) => {
                            let models = state.upstreams.models_for_kind(kind);
                            let model_options =
                                if kind == aiplane_core::server::upstreams::PoolKind::SystemOne {
                                    state
                                        .upstreams
                                        .models_with_compliance_for_kind(kind)
                                        .into_iter()
                                        .map(|(id, compliance)| SettingsModelOption {
                                            id,
                                            gdpr: compliance.gdpr,
                                            nda: compliance.nda,
                                        })
                                        .collect::<Vec<_>>()
                                } else {
                                    Vec::new()
                                };
                            (models, model_options)
                        }
                        _ => (Vec::new(), Vec::new()),
                    };
                    SettingsFieldView {
                        key: f.key,
                        kind: settings_kind(f.kind),
                        span: settings_span(f.span),
                        restart: f.restart,
                        value: effective.shown(f.key).map(str::to_string),
                        secret_set: matches!(f.kind, settings::Kind::Secret)
                            && effective.secret_is_set(f.key),
                        models,
                        model_options,
                        // Closed option set for a `choice` field; empty for
                        // every other kind. The SPA labels each option from
                        // its own catalog, so only the identifiers travel.
                        choices: f.choices(),
                    }
                })
                .collect();
            SettingsSectionView {
                name: section.name,
                category: section.category.slug(),
                enabled: settings::section_is_enabled(&state.config(), section),
                fields,
            }
        })
        .collect();
    json_ok(
        StatusCode::OK,
        SettingsView {
            sections,
            restart_pending,
            needs_backend: state.upstreams.all_models().is_empty(),
        },
    )
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct SettingsView {
    pub sections: Vec<SettingsSectionView>,
    /// The fields saved since the last restart whose change takes effect only on one.
    pub restart_pending: Vec<String>,
    /// No backend serves any model yet.
    pub needs_backend: bool,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct SettingsSectionView {
    pub name: &'static str,
    pub category: &'static str,
    /// Whether the section's feature is switched on; `null` for a section without a switch.
    pub enabled: Option<bool>,
    pub fields: Vec<SettingsFieldView>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct SettingsFieldView {
    pub key: &'static str,
    pub kind: &'static str,
    /// The field's width in the form: `full` or `half`.
    pub span: &'static str,
    /// A change takes effect only after a restart.
    pub restart: bool,
    /// The value in force; `null` for a secret or when none is set.
    pub value: Option<String>,
    pub secret_set: bool,
    /// The models a `model` field can name.
    pub models: Vec<String>,
    /// The same models with their compliance flags, for a field naming a
    /// `system_one` model; empty otherwise.
    pub model_options: Vec<SettingsModelOption>,
    /// The allowed values of a `choice` field; empty otherwise.
    pub choices: &'static [&'static str],
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct SettingsModelOption {
    pub id: String,
    pub gdpr: bool,
    pub nda: bool,
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
        // The same control as a plain list; the save validates each entry.
        settings::Kind::List | settings::Kind::NetworkList => "list",
    }
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
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
    let lang = session_core::i18n::Lang::from_request(req.headers());
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
    let mut issues: Vec<serde_json::Value> = Vec::new();
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
            _ => match field.check(&submitted) {
                Ok(value) => pairs.push((field.key.to_owned(), value)),
                Err(invalid) => issues.push(serde_json::json!({
                    "path": field.key,
                    "message": invalid.message(lang),
                })),
            },
        }
        if field.restart && parsed.values.contains_key(field.key) {
            restart_fields.push(field.key.to_owned());
        }
    }
    // All or nothing: storing the valid half would leave the section in a
    // state the operator never submitted.
    if !issues.is_empty() {
        return json_error_with(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_settings",
            &session_core::i18n::t(lang, "settings-invalid"),
            serde_json::Map::from_iter([("issues".to_string(), serde_json::json!(issues))]),
        );
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
    json_ok(StatusCode::OK, super::Done::OK)
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
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
    json_ok(
        StatusCode::OK,
        SettingCleared {
            cleared: parsed.key,
        },
    )
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct SettingCleared {
    /// The field whose stored value was dropped.
    pub cleared: String,
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
    let models = match super::json_tokens::model_catalog(
        &state,
        &aiplane_core::server::upstreams::PoolAccess::all(),
    )
    .await
    {
        Ok(models) => models,
        Err(err) => return internal(err),
    };
    let limits: std::collections::HashMap<String, Vec<LimitView>> = db::limits::list_all(&state.db)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|r| matches!(r.subject_type, limits::SubjectType::Token))
        .fold(std::collections::HashMap::new(), |mut acc, rule| {
            acc.entry(rule.subject_id.clone())
                .or_default()
                .push(limit_view(&rule));
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
            TokenUsage {
                requests: g.requests,
                total_tokens: g.total_tokens,
                cost: g.cost,
            },
        );
    }
    let out = tokens
        .into_iter()
        .map(|t| {
            let (owner_models, admin_models) = lists
                .get(&t.id)
                .map(|l| (l.owner.clone(), l.admin.clone()))
                .unwrap_or_default();
            AdminTokenView {
                limits: limits.get(&t.id).cloned().unwrap_or_default(),
                usage_this_month: usage_by_token.get(&t.id).cloned().unwrap_or_default(),
                id: t.id,
                name: t.name,
                owner_id: t.user_id,
                owner_email: t.user_email,
                created_at: t.created_at.to_string(),
                last_used_at: t.last_used_at.map(|value| value.to_string()),
                expires_at: t.expires_at.to_string(),
                revoked: t.revoked_at.is_some(),
                tools_enabled: t.tools_enabled,
                owner_models,
                admin_models,
            }
        })
        .collect();
    json_ok(
        StatusCode::OK,
        AdminTokensView {
            tokens: out,
            models,
            usage_enabled: state.usage.is_enabled(),
            currency: state.config().usage.currency.clone(),
            timezone: tz,
        },
    )
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct AdminTokensView {
    pub tokens: Vec<AdminTokenView>,
    /// Every model a token's allowlist can name.
    pub models: Vec<super::json_tokens::CatalogModelView>,
    pub usage_enabled: bool,
    pub currency: String,
    /// The timezone the month of `usage_this_month` is counted in.
    pub timezone: String,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct AdminTokenView {
    pub id: String,
    pub name: String,
    pub owner_id: String,
    /// The owner's email, or their id when the user no longer exists.
    pub owner_email: String,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub expires_at: String,
    pub revoked: bool,
    pub tools_enabled: bool,
    /// The owner's model allowlist; `null` = unrestricted.
    pub owner_models: Option<Vec<String>>,
    /// The admin's model allowlist; `null` = unrestricted. A token may use
    /// only the models both lists allow.
    pub admin_models: Option<Vec<String>>,
    pub limits: Vec<LimitView>,
    pub usage_this_month: TokenUsage,
}

#[derive(Clone, Default, serde::Serialize, schemars::JsonSchema)]
#[schemars(rename = "AdminTokenUsage")]
pub struct TokenUsage {
    pub requests: i64,
    pub total_tokens: i64,
    pub cost: f64,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct AdminTokenModels {
    /// The admin allowlist as stored; empty = unrestricted.
    pub models: Vec<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
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
        Ok(()) => json_ok(StatusCode::OK, AdminTokenModels { models: to_store }),
        Err(err) => internal(err),
    }
}

// ---------------------------------------------------------------------------
// Upstream topology (backends + pools + fallbacks + apply + live health)

use aiplane_core::server::db::upstreams_config::{self, AliasRow, BackendRow, PoolRow, VoiceRow};

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
    let mut live: std::collections::HashMap<String, LiveBackendView> =
        std::collections::HashMap::new();
    let mut live_by_pool: std::collections::HashMap<
        String,
        std::collections::HashMap<String, LiveBackendView>,
    > = std::collections::HashMap::new();
    for pool in state.upstreams.pools() {
        for backend in &pool.backends {
            // One snapshot each: `models_snapshot` materialises the effective
            // set, and `detected` clones the identification record — both were
            // being taken more than once per backend.
            let models = backend.models_snapshot();
            let detected = backend.detected();
            let entry = LiveBackendView {
                healthy: backend.is_healthy(),
                enabled: backend.is_enabled(),
                auth_failed: backend.auth_failed(),
                inflight: backend.inflight(),
                max_inflight: backend.max_inflight,
                // Sorted for the same reason the live stream sorts them: a
                // `HashSet` would render the first paint in a random order.
                models: sorted(models),
                pool: pool.name.clone(),
                // What identification made of this backend: shown as a badge,
                // never as a setting — nothing here is an operator's choice.
                // `detected_max_parallel` is the one an operator can act on,
                // when the configured in-flight ceiling promises more than the
                // server said it runs.
                profile: detected.profile.as_str(),
                detected_version: detected.version,
                detected_max_parallel: detected.max_parallel,
                // Identification runs only on apply, so how long ago it ran is
                // the difference between a profile describing this server and
                // one describing the server it used to be.
                detected_at: detected.detected_at,
            };
            live.insert(backend.name.clone(), entry.clone());
            live_by_pool
                .entry(pool.name.clone())
                .or_default()
                .insert(backend.name.clone(), entry);
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
                .map(|(name, serving, total)| ModelCoverage {
                    name,
                    serving,
                    total,
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
    let pools = snapshot
        .pools
        .iter()
        .map(|p| PoolView {
            name: p.name.clone(),
            kind: p.kind.clone(),
            strategy: p.strategy.clone(),
            fallback_offline: p.fallback_offline.clone(),
            compliance_gdpr: p.compliance_gdpr,
            compliance_nda: p.compliance_nda,
            enforce_limits: p.enforce_limits,
            sort_order: p.sort_order,
            allowed_groups: p.allowed_groups.clone(),
            backends: p.backends.clone(),
            live_backends: live_by_pool.get(&p.name).cloned(),
            models: p.models.clone(),
            voices: p
                .voices
                .iter()
                .map(|v| PoolVoiceView {
                    lang: v.lang_code.clone(),
                    voice: v.voice_id.clone(),
                })
                .collect(),
            offer_voices: p.offer_voices.clone(),
        })
        .collect();
    let backends = snapshot
        .backends
        .values()
        .map(|b| BackendView {
            name: b.name.clone(),
            base_url: b.base_url.clone(),
            api_key_env: b.api_key_env.clone(),
            api_key_env_set: b
                .api_key_env
                .as_ref()
                .is_some_and(|name| std::env::var(name).is_ok_and(|value| !value.is_empty())),
            has_stored_key: b.api_key_ct.is_some(),
            weight: b.weight,
            max_inflight: b.max_inflight,
            health_path: b.health_path.clone(),
            supports_edit: b.supports_edit,
            enabled: b.enabled,
            models: b.models.clone(),
            aliases: b
                .aliases
                .iter()
                .map(|a| BackendAliasView {
                    alias: a.alias.clone(),
                    target: a.target.clone(),
                })
                .collect(),
            live: live.get(&b.name).cloned(),
        })
        .collect();
    json_ok(
        StatusCode::OK,
        TopologyView {
            pools,
            backends,
            fallbacks: snapshot.fallbacks,
            all_models: state.upstreams.all_models(),
            coverage,
            pending_changes,
            usage_last_hour: usage,
            dirty: state.topology_dirty_count(),
            // The vocabulary, so the SPA renders its picker from data rather
            // than from a hardcoded <option> list that can fall behind the
            // enum (as it had — `rerank` was missing from every copy).
            pool_kinds: upstreams::config::PoolKind::ALL
                .iter()
                .map(|k| k.as_str())
                .collect(),
            pool_strategies: ["prefix_affinity", "least_inflight", "round_robin"],
            // The group vocabulary, for the same reason: the pool editor's
            // access picker renders from this rather than asking the operator
            // to retype a name that only matches when spelled exactly.
            groups: db::gateway_groups::list_group_names(&state.db)
                .await
                .unwrap_or_default(),
            fallback_kinds: ["chat", "transcription", "embedding", "image"],
        },
    )
}

/// The upstream topology as stored, beside what the running registry serves.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct TopologyView {
    pub pools: Vec<PoolView>,
    pub backends: Vec<BackendView>,
    /// Pool kind → the model an unknown model name falls back to.
    pub fallbacks: std::collections::HashMap<String, String>,
    pub all_models: Vec<String>,
    /// Pool name → how many of its backends serve each of its models.
    pub coverage: std::collections::HashMap<String, Vec<ModelCoverage>>,
    /// How the stored topology differs from the running one; applied by
    /// `POST /api/v0/admin/upstreams/reload`.
    pub pending_changes: Vec<PendingChange>,
    /// Backend name → its request counts over the last hour, in twelve
    /// five-minute buckets.
    pub usage_last_hour: std::collections::HashMap<String, Vec<i64>>,
    /// The number of topology changes saved since the last apply.
    pub dirty: u32,
    pub pool_kinds: Vec<&'static str>,
    pub pool_strategies: [&'static str; 3],
    /// The gateway group names a pool's access can name.
    pub groups: Vec<String>,
    /// The pool kinds a fallback model can be set for.
    pub fallback_kinds: [&'static str; 4],
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct PoolView {
    pub name: String,
    pub kind: String,
    pub strategy: String,
    pub fallback_offline: Option<String>,
    pub compliance_gdpr: bool,
    pub compliance_nda: bool,
    pub enforce_limits: bool,
    pub sort_order: i64,
    /// The groups that may use the pool; empty = everyone.
    pub allowed_groups: Vec<String>,
    pub backends: Vec<String>,
    /// The pool's backends in the running registry, by name; `null` when the
    /// pool is not running.
    pub live_backends: Option<std::collections::HashMap<String, LiveBackendView>>,
    pub models: Vec<String>,
    pub voices: Vec<PoolVoiceView>,
    pub offer_voices: Vec<String>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct PoolVoiceView {
    pub lang: String,
    pub voice: String,
}

/// A stored backend. Its API key is never returned.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct BackendView {
    pub name: String,
    pub base_url: String,
    /// The environment variable the API key is read from.
    pub api_key_env: Option<String>,
    /// That variable is set and not empty.
    pub api_key_env_set: bool,
    pub has_stored_key: bool,
    pub weight: u32,
    pub max_inflight: u32,
    pub health_path: String,
    pub supports_edit: bool,
    /// `false` while the backend is drained.
    pub enabled: bool,
    pub models: Vec<String>,
    pub aliases: Vec<BackendAliasView>,
    /// The backend in the running registry; `null` when it is not running.
    pub live: Option<LiveBackendView>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct BackendAliasView {
    pub alias: String,
    pub target: Option<String>,
}

#[derive(Clone, serde::Serialize, schemars::JsonSchema)]
pub struct LiveBackendView {
    pub healthy: bool,
    pub enabled: bool,
    pub auth_failed: bool,
    pub inflight: u32,
    pub max_inflight: u32,
    /// The models it serves now, sorted.
    pub models: Vec<String>,
    pub pool: String,
    /// The server software identification recognised.
    pub profile: &'static str,
    pub detected_version: Option<String>,
    /// How many requests the server says it runs at once.
    pub detected_max_parallel: Option<u32>,
    /// When identification last ran.
    pub detected_at: Option<String>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ModelCoverage {
    pub name: String,
    /// The pool's backends serving the model now.
    pub serving: usize,
    /// The pool's backends.
    pub total: usize,
}

/// One difference between the stored topology and the running one.
#[derive(serde::Serialize, schemars::JsonSchema)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum PendingChange {
    PoolAdded {
        pool: String,
    },
    PoolKind {
        pool: String,
        from: &'static str,
        to: String,
    },
    PoolStrategy {
        pool: String,
        from: &'static str,
        to: String,
    },
    BackendJoins {
        backend: String,
        pool: String,
    },
    BackendLeaves {
        backend: String,
        pool: String,
    },
    BackendUrl {
        backend: String,
        from: String,
        to: String,
    },
    BackendLimits {
        backend: String,
        weight: u32,
        inflight: u32,
    },
    BackendHealthPath {
        backend: String,
        to: String,
    },
    PoolRemoved {
        pool: String,
    },
}

fn topology_pending_changes(
    pools: &[PoolRow],
    backends: &std::collections::HashMap<String, BackendRow>,
    live: &aiplane_core::server::upstreams::LiveTopology,
) -> Vec<PendingChange> {
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
            changes.push(PendingChange::PoolAdded {
                pool: name.to_string(),
            });
            continue;
        };
        if pool.kind != live_pool.kind.as_str() {
            changes.push(PendingChange::PoolKind {
                pool: name.to_string(),
                from: live_pool.kind.as_str(),
                to: pool.kind.clone(),
            });
        }
        let live_strategy = picker_strategy_key(live_pool.strategy);
        if pool.strategy != live_strategy {
            changes.push(PendingChange::PoolStrategy {
                pool: name.to_string(),
                from: live_strategy,
                to: pool.strategy.clone(),
            });
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
            changes.push(PendingChange::BackendJoins {
                backend: backend.to_string(),
                pool: name.to_string(),
            });
        }
        for backend in live_members
            .iter()
            .filter(|name| !db_members.contains(name))
        {
            changes.push(PendingChange::BackendLeaves {
                backend: backend.to_string(),
                pool: name.to_string(),
            });
        }
        for live_backend in &live_pool.backends {
            let Some(backend) = backends.get(&live_backend.name) else {
                continue;
            };
            if backend.base_url.trim_end_matches('/') != live_backend.base_url {
                changes.push(PendingChange::BackendUrl {
                    backend: live_backend.name.clone(),
                    from: live_backend.base_url.clone(),
                    to: backend.base_url.clone(),
                });
            }
            if backend.weight.max(1) != live_backend.weight
                || backend.max_inflight.max(1) != live_backend.max_inflight
            {
                changes.push(PendingChange::BackendLimits {
                    backend: live_backend.name.clone(),
                    weight: backend.weight.max(1),
                    inflight: backend.max_inflight.max(1),
                });
            }
            if backend.health_path != live_backend.health_path {
                changes.push(PendingChange::BackendHealthPath {
                    backend: live_backend.name.clone(),
                    to: backend.health_path.clone(),
                });
            }
        }
    }
    for name in live_pools
        .keys()
        .filter(|name| !db_pools.contains_key(*name))
    {
        changes.push(PendingChange::PoolRemoved {
            pool: name.to_string(),
        });
    }
    changes
}

fn picker_strategy_key(strategy: aiplane_core::server::upstreams::PickerStrategy) -> &'static str {
    use aiplane_core::server::upstreams::PickerStrategy;
    match strategy {
        PickerStrategy::RoundRobin => "round_robin",
        PickerStrategy::LeastInflight => "least_inflight",
        PickerStrategy::PrefixAffinity => "prefix_affinity",
    }
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
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

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct AliasBody {
    pub alias: String,
    #[serde(default)]
    pub target: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
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
        return backend_test_failed(
            StatusCode::BAD_REQUEST,
            "base_url_required",
            "the connection test needs a base URL: enter the backend's address and test again",
            BackendTestFailure::default(),
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
        concat!("aiplane/", env!("CARGO_PKG_VERSION"), " connection-test"),
    );
    if let Some(key) = key.as_deref() {
        request = request.bearer_auth(key);
    }
    let response = match tokio::time::timeout(BACKEND_TEST_TIMEOUT, request.send()).await {
        Ok(Ok(response)) => response,
        Ok(Err(err)) => {
            let detail = deepest_error(&err);
            return backend_test_failed(
                StatusCode::BAD_GATEWAY,
                "unreachable",
                &format!(
                    "could not reach {url}: {detail}; check the address and that the backend is running"
                ),
                BackendTestFailure {
                    url: Some(url),
                    detail: Some(detail),
                    key_source: Some(key_source),
                    ..Default::default()
                },
            );
        }
        Err(_) => {
            return backend_test_failed(
                StatusCode::GATEWAY_TIMEOUT,
                "timeout",
                &format!(
                    "{url} did not answer within {} s; check the address and the backend's load",
                    BACKEND_TEST_TIMEOUT.as_secs()
                ),
                BackendTestFailure {
                    url: Some(url),
                    timeout_seconds: Some(BACKEND_TEST_TIMEOUT.as_secs()),
                    key_source: Some(key_source),
                    ..Default::default()
                },
            );
        }
    };
    let status = response.status().as_u16();
    let body = capped_read::read_capped(response, capped_read::MODEL_ANSWER_BYTES)
        .await
        .unwrap_or_default();
    // The backend's own 401 is not the caller's: answering it as such would
    // read as "your gateway session expired". It is a failed upstream, 502.
    if matches!(status, 401 | 403) {
        return backend_test_failed(
            StatusCode::BAD_GATEWAY,
            "auth_failed",
            &format!(
                "the backend refused the API key with {status}; check the key or its environment variable"
            ),
            BackendTestFailure {
                status: Some(status),
                key_source: Some(key_source),
                ..Default::default()
            },
        );
    }
    if !(200..300).contains(&status) {
        return backend_test_failed(
            StatusCode::BAD_GATEWAY,
            "http_error",
            &format!("{url} answered {status}; check the address and the health path"),
            BackendTestFailure {
                status: Some(status),
                url: Some(url),
                key_source: Some(key_source),
                ..Default::default()
            },
        );
    }
    let models = backend_test_model_ids(&body);
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
        aiplane_core::server::upstreams::profile::detect(&state.http, base_url, key.as_deref())
            .await
            .unwrap_or_default();
    let none_listed = models.is_empty();
    let found = BackendFound {
        outcome: if none_listed {
            TestOutcome::Warning
        } else {
            TestOutcome::Success
        },
        model_count: models.len(),
        models: models.into_iter().take(BACKEND_TEST_MAX_MODELS).collect(),
        key_source,
        profile: detected.profile.as_str(),
        // The tightest window this server reports, or nothing when it
        // reports none. One number, because the only thing the panel ever
        // did with the list was take its minimum.
        detected_context_window: detected
            .context_windows
            .values()
            .copied()
            .chain(detected.context_cap)
            .filter(|w| *w > 0)
            .min(),
        detected_version: detected.version,
        detected_max_parallel: detected.max_parallel,
    };
    json_ok(
        StatusCode::OK,
        if none_listed {
            BackendTest::OkNoModels(found)
        } else {
            BackendTest::Ok(found)
        },
    )
}

/// What a connection test of a backend that answered found; `code` says
/// whether it listed models. A test that failed is a refusal instead (see
/// [`backend_test_failed`]).
#[derive(serde::Serialize, schemars::JsonSchema)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum BackendTest {
    /// The backend answered and listed models.
    Ok(BackendFound),
    /// The backend answered but listed no model.
    OkNoModels(BackendFound),
}

/// A failed connection test: the shared envelope with the test's `code`
/// (`base_url_required`, `unreachable`, `timeout`, `auth_failed`,
/// `http_error`) and whichever details it has beside it.
fn backend_test_failed(
    status: StatusCode,
    code: &str,
    message: &str,
    failure: BackendTestFailure,
) -> Response {
    let extra = match serde_json::to_value(failure) {
        Ok(serde_json::Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    };
    json_error_with(status, code, message, extra)
}

/// The details a failed connection test carries in its error envelope.
#[derive(Default, serde::Serialize, schemars::JsonSchema)]
pub struct BackendTestFailure {
    /// The URL the test requested.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// The backend's HTTP status, when it answered.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    /// The innermost transport error, when it did not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_source: Option<KeySource>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TestOutcome {
    Warning,
    Success,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct BackendFound {
    pub outcome: TestOutcome,
    /// How many models the backend listed.
    pub model_count: usize,
    /// The first of them, sorted.
    pub models: Vec<String>,
    pub key_source: KeySource,
    /// The server software identification recognised.
    pub profile: &'static str,
    pub detected_version: Option<String>,
    pub detected_max_parallel: Option<u32>,
    /// The smallest context window the server reports.
    pub detected_context_window: Option<i64>,
}

/// Where the API key the test sent came from.
#[derive(serde::Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum KeySource {
    /// Typed into the request.
    Typed,
    /// The stored key of the backend named in the request.
    Stored,
    /// The named environment variable.
    Env { name: String },
    /// The named environment variable is unset or empty; no key was sent.
    EnvUnset { name: String },
    /// No key was sent.
    None,
}

async fn backend_test_key(
    state: &RamaState,
    body: &BackendTestBody,
) -> (Option<String>, KeySource) {
    if !body.api_key.trim().is_empty() {
        return (Some(body.api_key.trim().to_string()), KeySource::Typed);
    }
    if !body.name.trim().is_empty()
        && let Ok(Some(existing)) = upstreams_config::get_backend(&state.db, body.name.trim()).await
        && let (Some(ciphertext), Some(nonce)) = (existing.api_key_ct, existing.api_key_nonce)
        && let Ok(key) = state.crypto.open_str(&nonce, &ciphertext)
    {
        return (Some(key), KeySource::Stored);
    }
    let env_name = body.api_key_env.trim();
    if !env_name.is_empty() {
        return match std::env::var(env_name) {
            Ok(key) if !key.is_empty() => (
                Some(key),
                KeySource::Env {
                    name: env_name.to_string(),
                },
            ),
            _ => (
                None,
                KeySource::EnvUnset {
                    name: env_name.to_string(),
                },
            ),
        };
    }
    (None, KeySource::None)
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
    let Some((ids, _)) = aiplane_core::server::upstreams::profile::read_models(&value) else {
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
    json_ok(StatusCode::OK, TopologySaved { name, dirty })
}

/// The saved topology row; the change is live after the next apply.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct TopologySaved {
    pub name: String,
    /// The number of topology changes saved since the last apply.
    pub dirty: u32,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct BackendEnabled {
    pub name: String,
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
    let parsed: super::json_workspace::EnabledBody = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => return bad_request(format!("parsing the enabled body: {err}")),
    };
    match upstreams_config::set_backend_enabled(&state.db, &name, parsed.enabled).await {
        Ok(true) => {}
        Ok(false) => return not_found(format!("no backend `{name}`")),
        Err(err) => return internal(err),
    }
    state.upstreams.set_backend_enabled(&name, parsed.enabled);
    json_ok(
        StatusCode::OK,
        BackendEnabled {
            name,
            enabled: parsed.enabled,
        },
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
    match upstreams_config::delete_backend(&state.db, &name).await {
        Ok(true) => {}
        Ok(false) => return not_found(format!("no backend `{name}`")),
        Err(err) => return internal(err),
    }
    let dirty = state.topology_dirty_bump();
    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .header("x-topology-dirty", dirty.to_string())
        .body(rama::http::Body::empty())
        .expect("static empty response")
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
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
            json_ok(StatusCode::OK, TopologySaved { name: new, dirty })
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

#[derive(serde::Deserialize, schemars::JsonSchema)]
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

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct VoiceBody {
    pub lang: String,
    pub voice: String,
}

/// PUT /api/v0/admin/pools — upsert a pool row. Marks the topology dirty.
pub async fn pools_save(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = require_admin_json!(state, req);
    let (parts, body) = req.into_parts();
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
    // A group name that matches nothing makes the pool non-empty-but-unmatchable:
    // invisible and unroutable to every non-admin, with no clue as to why. Only
    // names this save introduces are checked — see `unknown_added_groups`.
    let stored_groups = if parsed.allowed_groups.is_empty() {
        Vec::new()
    } else {
        match upstreams_config::pool_allowed_groups(&state.db, &name).await {
            Ok(groups) => groups,
            Err(err) => return internal(err),
        }
    };
    match db::gateway_groups::unknown_added_groups_message(
        &state.db,
        Lang::from_request(&parts.headers),
        &parsed.allowed_groups,
        &stored_groups,
    )
    .await
    {
        Ok(Some(message)) => return bad_request(message),
        Ok(None) => {}
        Err(err) => return internal(err),
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
    json_ok(StatusCode::OK, TopologySaved { name, dirty })
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
    match upstreams_config::delete_pool(&state.db, &name).await {
        Ok(true) => {}
        Ok(false) => return not_found(format!("no pool `{name}`")),
        Err(err) => return internal(err),
    }
    let dirty = state.topology_dirty_bump();
    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .header("x-topology-dirty", dirty.to_string())
        .body(rama::http::Body::empty())
        .expect("static empty response")
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
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
        FallbackSaved {
            model: value.map(str::to_string),
            kind: parsed.kind,
        },
    )
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct FallbackSaved {
    pub kind: String,
    /// `null` when the fallback was cleared.
    pub model: Option<String>,
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
    let armed = tokio::spawn(aiplane_core::server::upstreams::health::spawn(
        state.upstreams.clone(),
        Some(state.db.clone()),
    ));
    if let Err(err) = armed.await {
        tracing::error!(error = %err, "arming the health probes after a topology apply panicked");
    }
    state.topology_dirty_reset();
    json_ok(
        StatusCode::OK,
        TopologyApplied {
            applied: true,
            dirty: 0,
        },
    )
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct TopologyApplied {
    pub applied: bool,
    pub dirty: u32,
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
/// keepalive when nothing has changed for a while. See `TICK` below and
/// `SSE_KEEPALIVE` for the cadence. The JSON twin of the legacy
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
        // `since_send` advances in whole ticks, so `SSE_KEEPALIVE` is reached
        // at the first tick at or past it — keep `TICK` a divisor of it, or
        // the effective gap is the next multiple up.
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
                let payload = BackendStatus {
                    name: name.clone(),
                    pool: live.as_ref().map(|(p, _)| p.clone()),
                    healthy,
                    enabled,
                    auth_failed,
                    inflight,
                    max_inflight,
                    configured: true,
                    dirty,
                    // Sorted, because these come out of a `HashSet` whose
                    // iteration order is randomized per process and reshuffles
                    // as probes rebuild it. Unsorted, the same models
                    // re-serialize differently on every tick: the list visibly
                    // reorders under the operator's cursor, and — worse — the
                    // `last` comparison below never matches, so the stream
                    // pushes an event per backend per tick forever. Sorting is
                    // what makes the change detection actually detect change.
                    models: live
                        .as_ref()
                        .map(|(_, backend)| sorted(backend.models_snapshot()))
                        .unwrap_or_default(),
                    usage: usage.get(name).cloned().unwrap_or_else(|| vec![0; 12]),
                    requests_last_hour: usage.get(name).map(|v| v.iter().sum::<i64>()).unwrap_or(0),
                };
                let encoded =
                    serde_json::to_string(&payload).expect("wire types serialize to JSON");
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
            if since_send >= super::SSE_KEEPALIVE {
                if tx.send(Ok(super::sse_keepalive())).await.is_err() {
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

/// One `status` event of the live topology stream: a stored backend's state
/// in the running registry.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct BackendStatus {
    pub name: String,
    /// The pool the backend runs in; `null` when it is not running.
    pub pool: Option<String>,
    pub healthy: bool,
    pub enabled: bool,
    pub auth_failed: bool,
    pub inflight: u32,
    pub max_inflight: u32,
    pub configured: bool,
    /// The number of topology changes saved since the last apply.
    pub dirty: u32,
    /// The models it serves now, sorted.
    pub models: Vec<String>,
    /// Its request counts over the last hour, in twelve five-minute buckets.
    pub usage: Vec<i64>,
    pub requests_last_hour: i64,
}
