// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The per-user `/tools` page — every signed-in user can turn the
//! individual AI tools the assistant may call on or off for their own
//! account.
//!
//! This is a personal layer on top of RBAC: the list only ever shows
//! tools the user's roles already grant (via `rbac::allowed_tools`),
//! and a toggle can only *subtract* one — never add a tool a role
//! didn't grant. Default is enabled; we persist only explicit choices
//! (see `db::user_tool_prefs`). Tools are grouped + de-noised by
//! `server::tools::catalog`.

use std::sync::Arc;

use rama::http::service::web::extract::State;
use rama::http::{Request, Response};

use super::tool_toggles;
use super::{internal, json_ok, not_found, read_json};

use aiplane_core::server::db::{user_tool_prefs, users};
use aiplane_runtime::rama_server::state::RamaState;

// ---------------------------------------------------------------------------
// GET /tools

// ---------------------------------------------------------------------------
// POST /tools/toggle

/// GET /api/v0/tools — the caller's tool toggles as data (issue #22 P3):
/// every tool their roles grant, with its per-user enabled state and the
/// same grouping the page renders.
pub async fn tools_list_json(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, user) = require_session_json!(state, req);
    let entries = tool_toggles::entries_for_roles(&state, &user.roles);
    let disabled = user_tool_prefs::disabled_for_user(&state.db, &user.id)
        .await
        .unwrap_or_default();
    let location_granted = entries.iter().any(|entry| entry.key == "get_user_location");
    let tools: Vec<_> = entries
        .into_iter()
        .map(|e| {
            serde_json::json!({
                "key": e.key,
                "title": e.title,
                "tech": e.tech,
                "description": e.description,
                "category": e.category.label(),
                "enabled": !disabled.contains(&e.key),
            })
        })
        .collect();
    let location = if location_granted {
        match users::find_location(&state.db, &user.id).await {
            Ok(stored) => serde_json::json!({
                "shared": stored.is_some(),
                "accuracy": stored.and_then(|location| location.accuracy),
            }),
            Err(err) => return internal(err),
        }
    } else {
        serde_json::Value::Null
    };
    json_ok(
        rama::http::StatusCode::OK,
        serde_json::json!({ "tools": tools, "location": location }),
    )
}

#[derive(serde::Deserialize)]
pub struct ToolsToggleBody {
    pub tool_key: String,
    pub enabled: bool,
}

/// POST /api/v0/tools/toggle — set one tool's on/off state for the caller
/// (explicit value; idempotent, unlike the checkbox-presence form).
pub async fn tools_toggle_json(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    use rama::http::StatusCode;

    let (_session, user) = require_session_json!(state, req);
    let (_, body) = req.into_parts();
    let parsed: ToolsToggleBody = match read_json(body, "the toggle body").await {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    // Refuse keys the caller's roles don't grant — toggling a tool you
    // cannot use would render a switch that lies.
    let entries = tool_toggles::entries_for_roles(&state, &user.roles);
    if !entries.iter().any(|e| e.key == parsed.tool_key) {
        return not_found("no such tool for your roles");
    }
    match user_tool_prefs::set(&state.db, &user.id, &parsed.tool_key, parsed.enabled).await {
        Ok(()) => json_ok(
            StatusCode::OK,
            serde_json::json!({ "tool_key": parsed.tool_key, "enabled": parsed.enabled }),
        ),
        Err(err) => internal(err),
    }
}

// ---------------------------------------------------------------------------
// Rendering
