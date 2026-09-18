// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! `/api/v0/*` — session-authenticated endpoints used by the web UI.
//!
//! Ported from `aiplane_core::server::api::tokens` and `chat`. Same wire shapes
//! (the wire types live in `shared::api`); the only difference is the
//! auth boundary: tower-sessions → our hand-rolled `SessionStore`, and
//! axum extractors → rama `Request` + path/query extractors.
//!
//! Endpoints:
//!   - GET /api/v0/me                       → user identity + role grants
//!   - GET /api/v0/tokens                   → list caller's tokens
//!   - POST /api/v0/tokens                  → mint a new token
//!   - DELETE /api/v0/tokens/{id}           → hard-delete a revoked token
//!   - POST /api/v0/tokens/{id}/revoke      → revoke an active token
//!   - POST /api/v0/tokens/{id}/rotate      → re-mint an active token's secret
//!
//! Chat / transcription / models session mirrors will land alongside
//! the UI port — they only matter once the rama-side UI is hitting them.

use std::sync::Arc;

use jiff::{SignedDuration, Timestamp};
use rama::http::service::web::extract::{Path, Query, State};
use rama::http::service::web::response::IntoResponse;
use rama::http::{Request, Response, StatusCode, header};
use serde_json::json;
use shared::api::{
    CreateTokenRequest, CreateTokenResponse, DeleteResponse, Me, RevokeResponse, TokenSummary,
    UpdateTokenToolsRequest,
};
use uuid::Uuid;

use aiplane_api::pages::{entries_for_roles, valid_keys};
use aiplane_core::rama_server::session::Session;
use aiplane_core::server::auth::token;
use aiplane_core::server::db::{token_tool_prefs, tokens, users};
use aiplane_runtime::rama_server::state::RamaState;

// ---------------------------------------------------------------------------
// Session gate

/// Pull the signed session cookie off the request and resolve the user.
/// `Err(Response)` is the 401-with-OpenAI-envelope a missing/expired
/// session produces; callers `return` it directly.
async fn require_session(state: &RamaState, req: &Request) -> Result<Session, Response> {
    match state.sessions.lookup_from_headers(req.headers()).await {
        Ok(Some(session)) => Ok(session),
        Ok(None) => Err(unauthorized("no active session — sign in at /auth/login")),
        Err(err) => {
            tracing::warn!(error = %err, "session lookup");
            Err(internal_error("session lookup failed"))
        }
    }
}

// ---------------------------------------------------------------------------
// Handlers

/// GET /api/v0/me — caller identity, role IDs after RBAC mapping, and the
/// set of tools their roles grant. The web UI uses the `allowed_tools`
/// field to render a "what can I do" panel.
pub async fn me(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let session = match require_session(&state, &req).await {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    me_response(&state, &session.user_id).await
}

/// Build the identity response (`Me` JSON, or a 5xx envelope if the user row
/// has vanished) for an already-authenticated `user_id`. Backs the
/// session-gated `GET /api/v0/me` (web UI).
pub(crate) async fn me_response(state: &RamaState, user_id: &str) -> Response {
    let user = match users::find_by_id(&state.db, user_id).await {
        Ok(Some(u)) => u,
        Ok(None) => {
            tracing::warn!(user_id = %user_id, "identity references missing user");
            return internal_error("identity references missing user");
        }
        Err(err) => {
            tracing::warn!(error = %err, "user lookup");
            return internal_error("user lookup failed");
        }
    };
    let role_ids = state.rbac.role_ids_for(&user.roles);
    let mut allowed_tool_ids = state.rbac.allowed_tools(&role_ids, &state.tools());
    state.expand_comfyui_tools(&mut allowed_tool_ids, &role_ids);
    let mut allowed_tools = state.tools().summaries_for(&allowed_tool_ids);
    // summaries_for only covers the static ToolRegistry; comfyui_* tools
    // live in the hot-reloadable ComfyuiToolSource. Build summaries from
    // the live catalog snapshot for any ids that summaries_for missed.
    if let Some(handle) = state.comfyui() {
        let snapshot = handle.store.current();
        for id in &allowed_tool_ids {
            if id.starts_with(aiplane_runtime::server::tools::catalog::COMFYUI_PREFIX)
                && !allowed_tools.iter().any(|t| &t.id == id)
                && let Some(manifest_id) =
                    id.strip_prefix(aiplane_runtime::server::tools::catalog::COMFYUI_PREFIX)
                && let Some(m) = snapshot.lookup(manifest_id)
            {
                allowed_tools.push(shared::api::ToolSummary {
                    id: id.clone(),
                    name: id.clone(),
                    description: m.description.clone(),
                });
            }
        }
    }
    json_ok(&Me {
        id: user.id,
        email: user.email,
        name: user.name,
        roles: user.roles,
        role_ids,
        allowed_tools,
        features: aiplane_core::server::settings::enabled_sections(&state.config()),
    })
}

/// GET /api/v0/tokens — list the caller's tokens (no hashes, no plaintext).
pub async fn list_tokens(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let session = match require_session(&state, &req).await {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let list = match tokens::list_for_user(&state.db, &session.user_id).await {
        Ok(l) => l,
        Err(err) => {
            tracing::warn!(error = %err, "listing tokens");
            return internal_error("listing tokens failed");
        }
    };
    let mut out: Vec<TokenSummary> = Vec::with_capacity(list.len());
    for t in list {
        let disabled = disabled_tools_for(&state, &t.id).await;
        out.push(to_summary(t, disabled));
    }
    json_ok(&out)
}

/// POST /api/v0/tokens — mint a new bearer. Plaintext returned **once**.
pub async fn create_token(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let session = match require_session(&state, &req).await {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let (_, body) = req.into_parts();
    let body_bytes = match read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return invalid_request(&msg),
    };
    let body: CreateTokenRequest = match serde_json::from_slice(&body_bytes) {
        Ok(b) => b,
        Err(err) => return invalid_request(&format!("body is not a CreateTokenRequest: {err}")),
    };

    let name = body.name.trim();
    if name.is_empty() || name.len() > 128 {
        return invalid_request("token name must be 1..=128 characters");
    }
    let ttl_days = body
        .ttl_days
        .unwrap_or(state.config().gateway.token_ttl_days)
        .clamp(1, 365 * 5);

    // Tool config (defaults: off, nothing disabled). Validate the
    // requested disable keys against the caller's own grant so we never
    // persist a key their roles don't expose.
    let tools_enabled = body.tools_enabled.unwrap_or(false);
    let user = match users::find_by_id(&state.db, &session.user_id).await {
        Ok(Some(u)) => u,
        Ok(None) => return internal_error("session references missing user"),
        Err(err) => {
            tracing::warn!(error = %err, "user lookup");
            return internal_error("user lookup failed");
        }
    };
    let allowed_keys = valid_keys(&entries_for_roles(&state, &user.roles));
    let mut disabled: Vec<String> = body.disabled_tools.clone();
    disabled.sort();
    disabled.dedup();
    if let Some(bad) = disabled.iter().find(|k| !allowed_keys.contains(*k)) {
        return invalid_request(&format!("unknown tool key `{bad}`"));
    }

    let now = Timestamp::now();
    let expires_at = now + SignedDuration::from_hours(24 * ttl_days);
    let (plaintext, hash) = token::mint();
    let row = tokens::Token {
        id: Uuid::new_v4().to_string(),
        user_id: session.user_id.clone(),
        name: name.to_string(),
        hash,
        created_at: now,
        last_used_at: None,
        expires_at,
        revoked_at: None,
        tools_enabled,
    };
    if let Err(err) = tokens::insert(&state.db, &row).await {
        tracing::warn!(error = %err, "storing token");
        return internal_error("storing token failed");
    }
    for key in &disabled {
        if let Err(err) = token_tool_prefs::set(&state.db, &row.id, key, false).await {
            tracing::warn!(error = %err, token_id = %row.id, tool_key = %key, "token tool pref save");
            return internal_error("storing token tool prefs failed");
        }
    }
    let summary = to_summary(row, disabled);
    json_ok(&CreateTokenResponse {
        token: summary,
        plaintext,
    })
}

/// PUT /api/v0/tokens/{id}/tools — replace a token's tool configuration:
/// the master switch plus the full set of disabled toggle keys. Owner-only
/// (a non-owned id 404s); disable keys are validated against the caller's
/// own grant. Replaces any previous per-token tool prefs.
pub async fn update_token_tools(
    State(state): State<Arc<RamaState>>,
    Path(token_id): Path<String>,
    req: Request,
) -> Response {
    let session = match require_session(&state, &req).await {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let (_, body) = req.into_parts();
    let body_bytes = match read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return invalid_request(&msg),
    };
    let body: UpdateTokenToolsRequest = match serde_json::from_slice(&body_bytes) {
        Ok(b) => b,
        Err(err) => {
            return invalid_request(&format!("body is not an UpdateTokenToolsRequest: {err}"));
        }
    };

    let user = match users::find_by_id(&state.db, &session.user_id).await {
        Ok(Some(u)) => u,
        Ok(None) => return internal_error("session references missing user"),
        Err(err) => {
            tracing::warn!(error = %err, "user lookup");
            return internal_error("user lookup failed");
        }
    };
    let allowed_keys = valid_keys(&entries_for_roles(&state, &user.roles));
    let mut disabled: Vec<String> = body.disabled_tools.clone();
    disabled.sort();
    disabled.dedup();
    if let Some(bad) = disabled.iter().find(|k| !allowed_keys.contains(*k)) {
        return invalid_request(&format!("unknown tool key `{bad}`"));
    }

    // Master switch, scoped to the owner — a non-owned/missing id is a 404.
    match tokens::set_tools_enabled(&state.db, &session.user_id, &token_id, body.tools_enabled)
        .await
    {
        Ok(true) => {}
        Ok(false) => return not_found("token not found"),
        Err(err) => {
            tracing::warn!(error = %err, %token_id, "set tools_enabled");
            return internal_error("updating token failed");
        }
    }

    // Replace prefs: write an explicit on/off for every key the user can
    // see, so a key dropped from `disabled_tools` flips back on.
    let disabled_set: std::collections::HashSet<&String> = disabled.iter().collect();
    for key in &allowed_keys {
        let enabled = !disabled_set.contains(key);
        if let Err(err) = token_tool_prefs::set(&state.db, &token_id, key, enabled).await {
            tracing::warn!(error = %err, %token_id, tool_key = %key, "token tool pref save");
            return internal_error("storing token tool prefs failed");
        }
    }

    json_ok(&json!({ "ok": true, "tools_enabled": body.tools_enabled, "disabled_tools": disabled }))
}

/// POST /api/v0/tokens/{id}/revoke — flip `revoked_at` on an owned active row.
pub async fn revoke_token(
    State(state): State<Arc<RamaState>>,
    Path(token_id): Path<String>,
    req: Request,
) -> Response {
    let session = match require_session(&state, &req).await {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let revoked = match tokens::revoke(&state.db, &session.user_id, &token_id).await {
        Ok(b) => b,
        Err(err) => {
            tracing::warn!(error = %err, %token_id, "revoke token");
            return internal_error("revoke failed");
        }
    };
    json_ok(&RevokeResponse { revoked })
}

/// POST /api/v0/tokens/{id}/rotate — re-mint an active token's secret in
/// place. The new plaintext is returned **once** (same shape as create);
/// the token's name, tool config and the configured TTL span are preserved
/// (the new lifetime runs the same duration from now). The old plaintext
/// stops authenticating immediately. 404s for a missing / non-owned / already
/// revoked token (a revoked token must be created anew, not resurrected).
pub async fn rotate_token(
    State(state): State<Arc<RamaState>>,
    Path(token_id): Path<String>,
    req: Request,
) -> Response {
    let session = match require_session(&state, &req).await {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let list = match tokens::list_for_user(&state.db, &session.user_id).await {
        Ok(l) => l,
        Err(err) => {
            tracing::warn!(error = %err, "listing tokens");
            return internal_error("listing tokens failed");
        }
    };
    let Some(existing) = list
        .iter()
        .find(|t| t.id == token_id && t.revoked_at.is_none())
    else {
        return not_found("no active token with that id");
    };

    // Preserve the originally-configured lifetime: re-issue with the same
    // span measured from now.
    let ttl = existing.expires_at - existing.created_at;
    let now = Timestamp::now();
    let expires_at = now + ttl;
    let (plaintext, hash) = token::mint();

    match tokens::rotate(
        &state.db,
        &session.user_id,
        &token_id,
        &hash,
        now,
        expires_at,
    )
    .await
    {
        Ok(true) => {}
        Ok(false) => return not_found("no active token with that id"),
        Err(err) => {
            tracing::warn!(error = %err, %token_id, "rotate token");
            return internal_error("rotate failed");
        }
    }

    // Re-read so the returned summary reflects the rotated row exactly.
    let disabled = disabled_tools_for(&state, &token_id).await;
    let summary = match tokens::list_for_user(&state.db, &session.user_id).await {
        Ok(l) => match l.into_iter().find(|t| t.id == token_id) {
            Some(t) => to_summary(t, disabled),
            None => return internal_error("rotated token vanished"),
        },
        Err(err) => {
            tracing::warn!(error = %err, "listing tokens");
            return internal_error("listing tokens failed");
        }
    };
    json_ok(&CreateTokenResponse {
        token: summary,
        plaintext,
    })
}

/// Query params of `GET /api/v0/usage` (public: rama's `Query` extractor
/// requires the type to match the handler's visibility).
#[derive(serde::Deserialize, Default)]
pub struct UsageQuery {
    period: Option<String>,
    scope: Option<String>,
    source: Option<String>,
    backend: Option<String>,
    token: Option<String>,
}

/// GET /api/v0/usage — the usage dashboard as data (issue #22 P3): the same
/// aggregates, pickers, in-force limits, and unpriced-model hints the
/// server-rendered page computes, for the SPA's usage view.
pub async fn usage(
    State(state): State<Arc<RamaState>>,
    Query(q): Query<UsageQuery>,
    req: Request,
) -> Response {
    use aiplane_core::server::db::usage as usage_db;
    use aiplane_core::server::db::usage::{Filter, Period};

    let session = match require_session(&state, &req).await {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let user = match aiplane_core::server::db::users::find_by_id(&state.db, &session.user_id).await
    {
        Ok(Some(u)) => u,
        _ => return unauthorized("no active session — sign in at /auth/login"),
    };
    // "All users" is admin-only; a non-admin passing ?scope=all is ignored —
    // the same clamp the page applies.
    let role_ids = state.role_ids_for(&user.roles);
    let can_view_all = state.rbac.is_admin(&role_ids);
    let show_all = can_view_all && q.scope.as_deref() == Some("all");
    let period = Period::parse(q.period.as_deref());
    let tz = session
        .timezone
        .clone()
        .or_else(|| user.timezone.clone())
        .unwrap_or_else(|| "UTC".to_string());
    let now = jiff::Timestamp::now();
    let bounds = usage_db::period_bounds(period, &tz, now);
    let filter = Filter {
        source: q.source.clone().filter(|s| !s.is_empty()),
        backend: q.backend.clone().filter(|s| !s.is_empty()),
        user_id: (!show_all).then(|| user.id.clone()),
        token_id: match q.token.as_deref() {
            None | Some("") => None,
            // `none` selects the rows that carry no token at all; empty means
            // "every token", so the two cannot share a spelling.
            Some("none") => Some(String::new()),
            Some(id) => Some(id.to_string()),
        },
    };
    let retention = state.config().usage.retention_days;
    let agg = usage_db::aggregate(&state.db, bounds, &filter, retention, now, show_all)
        .await
        .unwrap_or_default();
    let backends = usage_db::distinct_backends(&state.db, bounds)
        .await
        .unwrap_or_default();
    let tokens: Vec<_> =
        usage_db::distinct_tokens(&state.db, bounds, (!show_all).then_some(user.id.as_str()))
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|(id, _)| !id.is_empty())
            .map(|(id, label)| json!({ "id": id, "label": label }))
            .collect();
    let limit_status = state.enforcer.statuses(&user.id, &role_ids).await;
    // Models with traffic but no configured price → spend under-counted.
    let priced = aiplane_core::server::db::model_defaults::all_prices(&state.db)
        .await
        .unwrap_or_default();
    let unpriced: Vec<String> = agg
        .by_model
        .iter()
        .filter(|g| {
            !priced.contains_key(&g.key)
                && (g.total_tokens > 0 || g.input_units > 0.0 || g.output_units > 0.0)
        })
        .map(|g| g.key.clone())
        .collect();

    let limits: Vec<_> = limit_status
        .iter()
        .map(|l| {
            json!({
                "model": l.model,
                "dimension": l.dimension.as_str(),
                "window": l.window.as_str(),
                "limit": l.limit,
                "used": l.used,
                "refreshes_at": l.refreshes_at.to_string(),
            })
        })
        .collect();

    json_ok(&json!({
        "period": period.as_str(),
        "scope": if show_all { "all" } else { "self" },
        "can_view_all": can_view_all,
        "usage_enabled": state.usage.is_enabled(),
        "timezone": tz,
        "currency": state.config().usage.currency,
        "summary": agg.summary,
        "by_user": agg.by_user,
        "by_token": agg.by_token,
        "by_backend": agg.by_backend,
        "by_source": agg.by_source,
        "by_model": agg.by_model,
        "backends": backends,
        "tokens": tokens,
        "limits": limits,
        "unpriced_models": unpriced,
    }))
}

/// GET /api/v0/models — the caller's selectable chat models, with the
/// data-handling flags the compliance banner shows and the configured
/// default promoted. The SPA's model picker; the legacy page builds the
/// same list server-side (`pages::chat::list_chat_models`).
pub async fn chat_models(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let session = match require_session(&state, &req).await {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let user = match aiplane_core::server::db::users::find_by_id(&state.db, &session.user_id).await
    {
        Ok(Some(u)) => u,
        _ => return unauthorized("no active session — sign in at /auth/login"),
    };
    // Session path: access is exactly the user's group grant.
    let access = state.pool_access_for(&user.roles);
    let mut models: Vec<(String, aiplane_core::server::upstreams::Compliance)> =
        state.upstreams.models_with_compliance_for_kind_for(
            aiplane_core::server::upstreams::PoolKind::Chat,
            &access,
        );
    let mut automatic_targets: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    if let Ok(routes) = aiplane_core::server::db::automatic_routes::all(&state.db).await {
        let chat_compliance: std::collections::HashMap<_, _> = models.iter().cloned().collect();
        let selector_compliance: std::collections::HashMap<_, _> = state
            .upstreams
            .models_with_compliance_for_kind_for(
                aiplane_core::server::upstreams::PoolKind::SystemOne,
                &access,
            )
            .into_iter()
            .collect();
        for route in routes {
            let Some(fallback) = chat_compliance.get(&route.fallback_target) else {
                continue;
            };
            let mut compliance = *fallback;
            if let Some(selector) = selector_compliance.get(&route.selector_model) {
                compliance.gdpr &= selector.gdpr;
                compliance.nda &= selector.nda;
            } else {
                compliance.gdpr = false;
                compliance.nda = false;
            }
            for candidate in &route.candidates {
                if let Some(candidate_compliance) = chat_compliance.get(&candidate.target) {
                    compliance.gdpr &= candidate_compliance.gdpr;
                    compliance.nda &= candidate_compliance.nda;
                }
            }
            if !models.iter().any(|(name, _)| name == &route.alias) {
                automatic_targets.insert(
                    route.alias.clone(),
                    route
                        .candidates
                        .iter()
                        .map(|candidate| candidate.target.clone())
                        .collect(),
                );
                models.push((route.alias, compliance));
            }
        }
        models.sort_by(|left, right| left.0.cmp(&right.0));
    }
    use aiplane_core::server::feature_defaults::{self, Feature};
    let configured = feature_defaults::get(&state.db, Feature::Chat).await;
    feature_defaults::promote(configured.as_deref(), &mut models, |m| m.0.as_str());
    // Whether the composer's effort control does anything for each model.
    //
    // It is a select that has always been rendered the same for every model,
    // including the ones where it resolves to no parameter at all and changes
    // nothing. That is a control that lies, and it is the same class of
    // problem as the effort spelling being silently dropped: the user turns a
    // knob, nothing happens, and the model looks bad. Resolved exactly as the
    // request path resolves it — admin choice, then the serving backend's
    // dialect, then the model name — so the UI and the wire cannot disagree.
    //
    // One query for every stored row, not one per model. `resolve_for_model`
    // would be a `SELECT` each time round, and this runs on every chat page
    // load — a deployment with fifty models would have made fifty round trips
    // to answer a question about a dropdown. Most models have no row at all,
    // so the map is small and the lookup usually misses.
    let stored: std::collections::HashMap<String, Option<String>> =
        aiplane_core::server::db::model_defaults::all(&state.db)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|row| (row.model_name, row.reasoning_style))
            .collect();
    let listed: Vec<_> = models
        .into_iter()
        .map(|(id, compliance)| {
            // Aliases are listed as models of their own (`default`, `fast`,
            // …), and an alias name says nothing about the family behind it.
            // The turn resolves the alias to its real upstream id *before*
            // asking about reasoning, so every per-model lookup here has to
            // key on the same resolved id — otherwise the picker greys itself
            // out on a name like `default` while the request path happily
            // sends Qwen's `enable_thinking`, which is the same disagreement
            // between UI and wire this whole field exists to prevent.
            let targets = automatic_targets.get(&id).cloned().unwrap_or_else(|| {
                vec![
                    state
                        .upstreams
                        .resolve_model_for(
                            &id,
                            aiplane_core::server::upstreams::PoolKind::Chat,
                            &access,
                        )
                        .unwrap_or_else(|| id.clone()),
                ]
            });
            let reasoning = targets.iter().any(|target| {
                let dialect = state
                    .upstreams
                    .serving_profile(
                        target,
                        aiplane_core::server::upstreams::PoolKind::Chat,
                        &access,
                    )
                    .dialect;
                aiplane_core::server::reasoning::ReasoningStyle::resolve(
                    stored.get(target).and_then(Option::as_deref),
                    dialect,
                    target,
                ) != aiplane_core::server::reasoning::ReasoningStyle::None
            });
            json!({
                "id": id,
                "gdpr": compliance.gdpr,
                "nda": compliance.nda,
                "reasoning": reasoning,
            })
        })
        .collect();
    json_ok(&json!({ "models": listed }))
}

pub async fn transcription_models(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let session = match require_session(&state, &req).await {
        Ok(session) => session,
        Err(resp) => return resp,
    };
    let user = match users::find_by_id(&state.db, &session.user_id).await {
        Ok(Some(user)) => user,
        Ok(None) => return unauthorized("no active session — sign in at /auth/login"),
        Err(err) => {
            tracing::warn!(error = %err, "loading voice configuration user");
            return internal_error("could not load voice configuration");
        }
    };
    let access = state.pool_access_for(&user.roles);
    let mut models = state.upstreams.models_for_kind_for(
        aiplane_core::server::upstreams::PoolKind::Transcription,
        &access,
    );
    use aiplane_core::server::feature_defaults::{self, Feature};
    let configured = feature_defaults::get(&state.db, Feature::Transcription).await;
    feature_defaults::promote(configured.as_deref(), &mut models, |model| model.as_str());
    let speech_available = !state
        .upstreams
        .models_for_kind_for(aiplane_core::server::upstreams::PoolKind::Speech, &access)
        .is_empty();
    let speech_voices = state.upstreams.speech_voices_for(&access);
    json_ok(&json!({
        "data": models,
        "speech_available": speech_available,
        "speech_voices": speech_voices,
        "speech_voice": user.speech_voice,
    }))
}

/// POST /api/v0/me/timezone — store the caller's IANA timezone on
/// their session + user row. Posted from `app.js` once per page load
/// after reading `Intl.DateTimeFormat().resolvedOptions().timeZone`.
/// Body: `{ "timezone": "Europe/Berlin" }`.
///
/// Validates the IANA name via `jiff::tz::TimeZone::get` so we don't
/// persist garbage. We update *both* the session row (per-device
/// scope) and the user row (fallback for bearer-authed callers that
/// never have a session) — tools that care about wall-clock time
/// read from the user row.
pub async fn set_timezone(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    use jiff::tz::TimeZone;

    let session = match require_session(&state, &req).await {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let (_, body) = req.into_parts();
    let body = match read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return invalid_request(&msg),
    };
    #[derive(serde::Deserialize)]
    struct Body {
        timezone: String,
    }
    let parsed: Body = match serde_json::from_slice(&body) {
        Ok(p) => p,
        Err(err) => return invalid_request(&format!("expected {{\"timezone\":\"…\"}}: {err}")),
    };
    if TimeZone::get(&parsed.timezone).is_err() {
        return invalid_request(&format!(
            "`{}` is not a known IANA timezone",
            parsed.timezone
        ));
    }
    if let Err(err) = state
        .sessions
        .set_timezone(&session.id, &parsed.timezone)
        .await
    {
        tracing::warn!(error = %err, "session set_timezone");
        return internal_error("could not save timezone");
    }
    if let Err(err) = users::set_timezone(&state.db, &session.user_id, &parsed.timezone).await {
        tracing::warn!(error = %err, "users set_timezone");
        return internal_error("could not save timezone");
    }
    json_ok(&json!({ "ok": true, "timezone": parsed.timezone }))
}

/// POST /api/v0/me/speech_voice — store (or clear) the voice the caller wants
/// spoken replies in. Posted by the chat header's voice picker.
/// Body: `{ "voice": "onyx" }`, or `{ "voice": null }` to go back to the
/// operator's language→voice default.
///
/// The id is checked against the voices the caller's own speech pools
/// advertise. The speech path re-checks on every call (an operator can retire a
/// voice after it was picked), so this validation is not what makes the feature
/// safe — it's what gives the picker an honest 400 instead of storing a value
/// that would silently fall back forever.
pub async fn set_speech_voice(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let session = match require_session(&state, &req).await {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let (_, body) = req.into_parts();
    let body = match read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return invalid_request(&msg),
    };
    #[derive(serde::Deserialize)]
    struct Body {
        #[serde(default)]
        voice: Option<String>,
    }
    let parsed: Body = match serde_json::from_slice(&body) {
        Ok(p) => p,
        Err(err) => return invalid_request(&format!("expected {{\"voice\":\"…\"}}: {err}")),
    };
    // An empty string is the picker's "no preference" option, same as null.
    let voice = parsed
        .voice
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    if let Some(v) = &voice {
        let roles = users::find_by_id(&state.db, &session.user_id)
            .await
            .ok()
            .flatten()
            .map(|u| u.roles)
            .unwrap_or_default();
        let allowed = state
            .upstreams
            .speech_voices_for(&state.pool_access_for(&roles));
        if !allowed.contains(v) {
            return invalid_request(&format!(
                "`{v}` is not one of this deployment's speech voices"
            ));
        }
    }
    if let Err(err) = users::set_speech_voice(&state.db, &session.user_id, voice.as_deref()).await {
        tracing::warn!(error = %err, "users set_speech_voice");
        return internal_error("could not save the speech voice");
    }
    json_ok(&json!({ "ok": true, "voice": voice }))
}

/// POST /api/v0/me/location — store the caller's browser-reported
/// position on their user row. Posted from `geo.ts` once
/// `navigator.geolocation.getCurrentPosition` resolves (the `/tools`
/// "share location" button, or the chat feedback-loop prompt). Body:
/// `{ "lat": 52.52, "lon": 13.405, "accuracy": 25.0 }` — `accuracy`
/// (metres) optional. The `get_user_location` tool reads it back,
/// preferring a fresh fix over coarse GeoIP.
pub async fn set_location(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let session = match require_session(&state, &req).await {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let (_, body) = req.into_parts();
    let body = match read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return invalid_request(&msg),
    };
    #[derive(serde::Deserialize)]
    struct Body {
        lat: f64,
        lon: f64,
        #[serde(default)]
        accuracy: Option<f64>,
    }
    let parsed: Body = match serde_json::from_slice(&body) {
        Ok(p) => p,
        Err(err) => return invalid_request(&format!("expected {{\"lat\":…,\"lon\":…}}: {err}")),
    };
    let accuracy = match validate_lat_lon(parsed.lat, parsed.lon, parsed.accuracy) {
        Ok(a) => a,
        Err(msg) => return invalid_request(msg),
    };
    if let Err(err) = users::set_location(
        &state.db,
        &session.user_id,
        parsed.lat,
        parsed.lon,
        accuracy,
    )
    .await
    {
        tracing::warn!(error = %err, "users set_location");
        return internal_error("could not save location");
    }
    json_ok(&json!({ "ok": true }))
}

/// DELETE /api/v0/me/location — forget the caller's stored position (the
/// "stop sharing" affordance on `/tools`).
pub async fn clear_location(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let session = match require_session(&state, &req).await {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    if let Err(err) = users::clear_location(&state.db, &session.user_id).await {
        tracing::warn!(error = %err, "users clear_location");
        return internal_error("could not clear location");
    }
    json_ok(&json!({ "ok": true }))
}

/// Shared preamble of the three mid-turn feedback endpoints: authenticate the
/// caller, read the body, parse it, and prove the turn is theirs.
///
/// Extracted because the ownership check is the security-critical half of all
/// three and was written out three times. An unknown turn and somebody else's
/// turn answer identically, so the endpoint cannot be used to discover which
/// turn ids exist — a property that has to hold in every copy, which is the
/// argument for there being only one.
///
/// `shape` is the hint appended to a parse failure; `missing` is the message
/// for a turn the caller does not own. The session comes back with the body —
/// `location_feedback` also writes to the caller's own user row.
async fn turn_feedback_body<T: serde::de::DeserializeOwned>(
    state: &RamaState,
    req: Request,
    turn_id: &str,
    shape: &str,
    missing: &str,
) -> Result<(Session, T), Response> {
    let session = match require_session(state, &req).await {
        Ok(s) => s,
        Err(resp) => return Err(resp),
    };
    let (_, body) = req.into_parts();
    let body = match read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return Err(invalid_request(&msg)),
    };
    let parsed: T = match serde_json::from_slice(&body) {
        Ok(p) => p,
        Err(err) => return Err(invalid_request(&format!("expected {shape}: {err}"))),
    };

    match session_core::db::user_for_turn(&state.db, turn_id).await {
        Ok(Some(owner)) if owner == session.user_id => Ok((session, parsed)),
        Ok(_) => Err(invalid_request(missing)),
        Err(err) => {
            tracing::warn!(error = %err, turn_id, "turn feedback user_for_turn");
            Err(internal_error("could not verify the request"))
        }
    }
}

/// POST /api/v0/me/location/feedback/{turn_id} — reply to an in-flight
/// `get_user_location` prompt for assistant turn `turn_id`. Posted by
/// `geo.ts` when the user clicks "share" (body `{lat, lon, accuracy}`)
/// or "not now" (body `{ "denied": true }`) on the prompt the tool
/// injected. Resolves the parked tool via the feedback hub; a shared
/// position is also persisted so the next turn skips the prompt.
///
/// Verifies the turn belongs to the caller — see [`ask_feedback`] for why a
/// session cookie alone isn't enough. Here it matters twice over: an answer
/// also *persists a position on the user row*, so without the check a
/// logged-in user could write a location onto whoever owns the turn.
pub async fn location_feedback(
    State(state): State<Arc<RamaState>>,
    Path(turn_id): Path<String>,
    req: Request,
) -> Response {
    use aiplane_runtime::server::tools::feedback::BrowserFix;

    #[derive(serde::Deserialize)]
    struct Body {
        #[serde(default)]
        lat: Option<f64>,
        #[serde(default)]
        lon: Option<f64>,
        #[serde(default)]
        accuracy: Option<f64>,
        #[serde(default)]
        denied: bool,
    }
    let (session, parsed): (Session, Body) = match turn_feedback_body(
        &state,
        req,
        &turn_id,
        "a position or {\"denied\":true}",
        "no such pending prompt",
    )
    .await
    {
        Ok(p) => p,
        Err(resp) => return resp,
    };

    let fix = if parsed.denied {
        BrowserFix::Declined
    } else {
        let (Some(lat), Some(lon)) = (parsed.lat, parsed.lon) else {
            return invalid_request("need {lat, lon} or {\"denied\": true}");
        };
        let accuracy = match validate_lat_lon(lat, lon, parsed.accuracy) {
            Ok(a) => a,
            Err(msg) => return invalid_request(msg),
        };
        // Persist so a follow-up turn within the freshness window reuses
        // it without re-prompting.
        if let Err(err) = users::set_location(&state.db, &session.user_id, lat, lon, accuracy).await
        {
            tracing::warn!(error = %err, "location_feedback set_location");
        }
        BrowserFix::Position { lat, lon, accuracy }
    };
    // Whoever's parked on this turn (if anyone — the tool may have timed
    // out) gets the reply. We don't treat "no one waiting" as an error.
    state.location_feedback.resolve(&turn_id, fix);
    json_ok(&json!({ "ok": true }))
}

/// POST /api/v0/me/ask/feedback/{turn_id} — answer an in-flight `ask_user`
/// question for assistant turn `turn_id`.
///
/// Posted by `ask.ts` when the user picks an option, types an answer, or skips.
/// Body is `{choices: [..], text: "..."}` for an answer or `{"dismissed": true}`
/// to skip. Resolves the parked tool via the ask feedback hub.
///
/// Unlike [`location_feedback`], this **verifies the turn belongs to the
/// caller**. A session cookie alone is not enough: without the check any
/// logged-in user who learned a turn id could answer someone else's question,
/// injecting text straight into another user's model context. Turn ids are
/// UUIDs so it isn't trivially exploitable, but "hard to guess" is not an
/// authorisation model.
pub async fn ask_feedback(
    State(state): State<Arc<RamaState>>,
    Path(turn_id): Path<String>,
    req: Request,
) -> Response {
    use aiplane_runtime::server::tools::feedback::AskReply;

    #[derive(serde::Deserialize)]
    struct Body {
        #[serde(default)]
        choices: Vec<String>,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        dismissed: bool,
    }
    let (_session, parsed): (Session, Body) = match turn_feedback_body(
        &state,
        req,
        &turn_id,
        "{choices, text} or {\"dismissed\":true}",
        "no such pending question",
    )
    .await
    {
        Ok(p) => p,
        Err(resp) => return resp,
    };

    let reply = if parsed.dismissed {
        AskReply::Dismissed
    } else {
        let choices: Vec<String> = parsed
            .choices
            .into_iter()
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty())
            .collect();
        let text = parsed
            .text
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty());
        // An empty answer is a dismissal in disguise; treating it as an answer
        // would hand the model `answered: true` with nothing in it.
        if choices.is_empty() && text.is_none() {
            AskReply::Dismissed
        } else {
            AskReply::Answered { choices, text }
        }
    };
    // Whoever's parked on this turn (if anyone — the tool may have timed out)
    // gets the reply. "No one waiting" is not an error.
    state.ask_feedback.resolve(&turn_id, reply);
    json_ok(&json!({ "ok": true }))
}

/// POST /api/v0/me/browser/feedback/{turn_id} — report what the paired browser
/// extension did with an in-flight `browser_control` batch.
///
/// Posted by `browser-bridge.ts` after the extension has worked through the
/// actions, refused them, or reported that it isn't there. Body is one of:
///
/// ```json
/// {"request_id": "…", "results": [ ... ]}                 // every action ran
/// {"request_id": "…", "error": "…", "results": [ ... ]}   // stopped partway
/// {"request_id": "…", "refused": "the user declined the click"}
/// {"request_id": "…", "no_extension": true}
/// ```
///
/// The two ids do different jobs. The **path** carries the turn, which is what
/// authorisation is checked against; the **body** carries the request id the
/// tool is actually parked on, because one turn can have several batches in
/// flight (the runner executes a round's tool calls concurrently). Resolving by
/// turn would let one batch's reply wake another batch's tool.
///
/// Ownership is verified exactly as in [`ask_feedback`], and for a sharper
/// reason: this reply becomes the model's picture of a page in *someone's*
/// browser. A stranger able to answer another user's turn could hand the model
/// a page that never existed, which is prompt injection with a return address.
pub async fn browser_feedback(
    State(state): State<Arc<RamaState>>,
    Path(turn_id): Path<String>,
    req: Request,
) -> Response {
    use aiplane_runtime::server::tools::feedback::BrowserReply;

    #[derive(serde::Deserialize)]
    struct Body {
        request_id: String,
        #[serde(default)]
        results: Vec<serde_json::Value>,
        #[serde(default)]
        error: Option<String>,
        #[serde(default)]
        refused: Option<String>,
        #[serde(default)]
        no_extension: bool,
    }
    let (_session, parsed): (Session, Body) = match turn_feedback_body(
        &state,
        req,
        &turn_id,
        "{request_id, results}, {request_id, error, results}, {request_id, refused} \
         or {request_id, \"no_extension\":true}",
        "no such pending browser request",
    )
    .await
    {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    if parsed.request_id.trim().is_empty() {
        return invalid_request("request_id must not be empty");
    }

    // Order matters: a refusal outranks a partial result (the user said no, so
    // whatever ran before that is not the headline), and "no extension"
    // outranks an empty success (which would otherwise read as "it worked and
    // did nothing").
    let reply = if let Some(reason) = parsed.refused.filter(|r| !r.trim().is_empty()) {
        BrowserReply::Refused { reason }
    } else if parsed.no_extension {
        BrowserReply::NoExtension
    } else if let Some(error) = parsed.error.filter(|e| !e.trim().is_empty()) {
        BrowserReply::Failed {
            error,
            results: parsed.results,
        }
    } else {
        BrowserReply::Done {
            results: parsed.results,
        }
    };

    // Resolved by request id: several batches can be parked for one turn, and
    // an unknown id simply finds nobody waiting (the tool may have timed out).
    state.browser_feedback.resolve(&parsed.request_id, reply);
    json_ok(&json!({ "ok": true }))
}

/// DELETE /api/v0/tokens/{id} — hard-delete an already-revoked row.
/// Active tokens have to be revoked first (DB layer enforces).
pub async fn delete_token(
    State(state): State<Arc<RamaState>>,
    Path(token_id): Path<String>,
    req: Request,
) -> Response {
    let session = match require_session(&state, &req).await {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let deleted = match tokens::delete_if_revoked(&state.db, &session.user_id, &token_id).await {
        Ok(b) => b,
        Err(err) => {
            tracing::warn!(error = %err, %token_id, "delete token");
            return internal_error("delete failed");
        }
    };
    json_ok(&DeleteResponse { deleted })
}

// ---------------------------------------------------------------------------
// Web Push (turn-complete notifications)

/// GET /api/v0/push/config — what the client needs to decide whether to offer
/// the "enable notifications" control and, if so, to subscribe:
/// `{ "enabled": bool, "publicKey": <VAPID key base64url>|null }`.
pub async fn push_config(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    if let Err(resp) = require_session(&state, &req).await {
        return resp;
    }
    match state.push.as_ref() {
        Some(push) => json_ok(&json!({ "enabled": true, "publicKey": push.public_key() })),
        None => json_ok(&json!({ "enabled": false, "publicKey": null })),
    }
}

/// POST /api/v0/push/subscribe — register this browser's push subscription for
/// the signed-in user. Body is the browser's `PushSubscription.toJSON()`:
/// `{ "endpoint": "...", "keys": { "p256dh": "...", "auth": "..." } }`.
pub async fn push_subscribe(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let session = match require_session(&state, &req).await {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    if state.push.is_none() {
        return error_envelope(
            StatusCode::SERVICE_UNAVAILABLE,
            "push_disabled",
            "push notifications are disabled on this gateway",
        );
    }
    let user_agent = req
        .headers()
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    // Capture the browser's UI language now (from the `lang` cookie) so the
    // detached turn-complete send can localize the notification per device.
    let lang = session_core::i18n::Lang::from_request(req.headers())
        .code()
        .to_string();
    let (_, body) = req.into_parts();
    let body = match read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return invalid_request(&msg),
    };
    #[derive(serde::Deserialize)]
    struct Keys {
        p256dh: String,
        auth: String,
    }
    #[derive(serde::Deserialize)]
    struct Body {
        endpoint: String,
        keys: Keys,
    }
    let parsed: Body = match serde_json::from_slice(&body) {
        Ok(p) => p,
        Err(err) => {
            return invalid_request(&format!(
                "expected {{\"endpoint\":…,\"keys\":{{\"p256dh\":…,\"auth\":…}}}}: {err}"
            ));
        }
    };
    // Validate before storing: the gateway later POSTs to `endpoint` on turn
    // completion, so reject non-https / private / loopback targets (blind-SSRF
    // guard) and key material that could only ever fail to encrypt.
    if let Err(msg) = aiplane_features::server::push::validate_subscription(
        &parsed.endpoint,
        &parsed.keys.p256dh,
        &parsed.keys.auth,
    ) {
        return invalid_request(&msg);
    }
    if let Err(err) = aiplane_core::server::db::push_subscriptions::upsert(
        &state.db,
        &session.user_id,
        &parsed.endpoint,
        &parsed.keys.p256dh,
        &parsed.keys.auth,
        Some(lang.as_str()),
        user_agent.as_deref(),
    )
    .await
    {
        tracing::warn!(error = %err, "storing push subscription");
        return internal_error("could not store subscription");
    }
    json_ok(&json!({ "ok": true }))
}

/// POST /api/v0/push/unsubscribe — forget a browser subscription (the user
/// turned notifications off, or the browser rotated it). Body:
/// `{ "endpoint": "..." }`.
pub async fn push_unsubscribe(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let session = match require_session(&state, &req).await {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let (_, body) = req.into_parts();
    let body = match read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return invalid_request(&msg),
    };
    #[derive(serde::Deserialize)]
    struct Body {
        endpoint: String,
    }
    let parsed: Body = match serde_json::from_slice(&body) {
        Ok(p) => p,
        Err(err) => return invalid_request(&format!("expected {{\"endpoint\":…}}: {err}")),
    };
    // Scoped to the caller: a user can only forget their OWN subscription, so
    // knowing another user's (opaque) endpoint can't be used to unsubscribe them.
    if let Err(err) = aiplane_core::server::db::push_subscriptions::delete_by_endpoint(
        &state.db,
        &session.user_id,
        &parsed.endpoint,
    )
    .await
    {
        tracing::warn!(error = %err, "deleting push subscription");
        return internal_error("could not remove subscription");
    }
    json_ok(&json!({ "ok": true }))
}

// ---------------------------------------------------------------------------
// Shared helpers (response builders + utilities)

fn to_summary(t: tokens::Token, disabled_tools: Vec<String>) -> TokenSummary {
    TokenSummary {
        id: t.id,
        name: t.name,
        created_at: t.created_at,
        last_used_at: t.last_used_at,
        expires_at: t.expires_at,
        revoked: t.revoked_at.is_some(),
        tools_enabled: t.tools_enabled,
        disabled_tools,
    }
}

/// The token's disabled toggle keys, sorted for a stable response.
async fn disabled_tools_for(state: &RamaState, token_id: &str) -> Vec<String> {
    let mut keys: Vec<String> = token_tool_prefs::disabled_for_token(&state.db, token_id)
        .await
        .unwrap_or_default()
        .into_iter()
        .collect();
    keys.sort();
    keys
}

/// Validate a browser-reported lat/lon pair. On success returns a
/// sanitised `accuracy` (a NaN/negative one is dropped — the position is
/// still usable without it); on failure, the message naming the bad
/// field. Shared by `set_location` and `location_feedback`.
fn validate_lat_lon(
    lat: f64,
    lon: f64,
    accuracy: Option<f64>,
) -> Result<Option<f64>, &'static str> {
    if !lat.is_finite() || !(-90.0..=90.0).contains(&lat) {
        return Err("lat must be a number between -90 and 90");
    }
    if !lon.is_finite() || !(-180.0..=180.0).contains(&lon) {
        return Err("lon must be a number between -180 and 180");
    }
    Ok(accuracy.filter(|a| a.is_finite() && *a >= 0.0))
}

fn json_ok<T: serde::Serialize>(value: &T) -> Response {
    let body = match serde_json::to_string(value) {
        Ok(s) => s,
        Err(err) => return internal_error(&format!("serialising response: {err}")),
    };
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        body,
    )
        .into_response()
}

fn invalid_request(message: &str) -> Response {
    error_envelope(StatusCode::BAD_REQUEST, "invalid_request", message)
}

fn unauthorized(message: &str) -> Response {
    error_envelope(StatusCode::UNAUTHORIZED, "unauthorized", message)
}

fn internal_error(message: &str) -> Response {
    error_envelope(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", message)
}

fn not_found(message: &str) -> Response {
    error_envelope(StatusCode::NOT_FOUND, "not_found", message)
}

fn error_envelope(status: StatusCode, code: &str, message: &str) -> Response {
    let body = json!({
        "error": {
            "message": message,
            "type": code,
            "code": code,
        }
    });
    (
        status,
        [(header::CONTENT_TYPE, "application/json")],
        body.to_string(),
    )
        .into_response()
}

async fn read_body_to_bytes(body: rama::http::Body) -> Result<rama::bytes::Bytes, String> {
    use rama::http::body::util::BodyExt;
    body.collect()
        .await
        .map(|c| c.to_bytes())
        .map_err(|e| format!("reading request body: {e}"))
}
