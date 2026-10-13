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

use session_core::i18n::Lang;

use rama::http::service::web::extract::State;
use rama::http::{Request, Response};

use super::tool_toggles;
use super::{internal, json_ok, not_found, read_json};

use aiplane_core::server::db::{user_tool_prefs, users};
use aiplane_runtime::rama_server::state::RamaState;
use aiplane_runtime::server::tools::catalog;

// ---------------------------------------------------------------------------
// GET /tools

// ---------------------------------------------------------------------------
// POST /tools/toggle

/// GET /api/v0/tools — the caller's tool toggles as data:
/// every tool their roles grant, with its per-user enabled state and the
/// same grouping the page renders.
pub async fn tools_list_json(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let lang = Lang::from_request(req.headers());
    let (_session, user) = require_session_json!(state, req);
    let mut entries = tool_toggles::entries_for_roles(&state, &user.roles, lang);
    let admin = super::is_admin(&state, &user);
    let registered = state.tools();
    if admin {
        entries.extend(catalog::unregistered_configurable_entries(&registered));
        entries.sort_by(|left, right| {
            left.category
                .order()
                .cmp(&right.category.order())
                .then_with(|| left.key.cmp(&right.key))
        });
    }
    let config = state.config();
    let disabled = user_tool_prefs::disabled_for_user(&state.db, &user.id)
        .await
        .unwrap_or_default();
    let location_granted = entries.iter().any(|entry| entry.key == "get_user_location");
    let tools = entries
        .into_iter()
        .map(|e| {
            let availability = tool_configuration(
                &e.key,
                &config,
                registered.contains(&e.key),
                state.push.is_some(),
                state.indexer.is_some(),
            );
            ToolToggleView {
                enabled: availability.is_none() && !disabled.contains(&e.key),
                key: e.key,
                title: e.title,
                tech: e.tech,
                description: e.description,
                category: e.category.key(),
                configuration: availability.map(|(reason, url)| ToolConfigurationNeeded {
                    reason,
                    url: admin.then_some(url),
                }),
            }
        })
        .collect();
    let location = if location_granted {
        match users::find_location(&state.db, &user.id).await {
            Ok(stored) => Some(LocationSharing {
                shared: stored.is_some(),
                accuracy: stored.and_then(|location| location.accuracy),
            }),
            Err(err) => return internal(err),
        }
    } else {
        None
    };
    json_ok(rama::http::StatusCode::OK, ToolsView { tools, location })
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ToolsView {
    pub tools: Vec<ToolToggleView>,
    /// Whether the caller shares a browser position; `null` when no role
    /// grants `get_user_location`.
    pub location: Option<LocationSharing>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ToolToggleView {
    pub key: String,
    pub title: String,
    pub tech: String,
    pub description: String,
    pub category: &'static str,
    pub enabled: bool,
    /// Set when the tool cannot run until the deployment configures something.
    pub configuration: Option<ToolConfigurationNeeded>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ToolConfigurationNeeded {
    /// The translation key of the reason.
    pub reason: &'static str,
    /// Where an admin configures it; `null` for everyone else.
    pub url: Option<&'static str>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct LocationSharing {
    pub shared: bool,
    /// Accuracy radius in metres of the stored position.
    pub accuracy: Option<f64>,
}

fn tool_configuration(
    key: &str,
    config: &aiplane_core::server::Config,
    registered: bool,
    push_ready: bool,
    rag_ready: bool,
) -> Option<(&'static str, &'static str)> {
    if !registered
        && matches!(
            key,
            "lookup_ip"
                | "generate_image"
                | "edit_image"
                | "run_in_sandbox"
                | "generate_document"
                | "convert_document"
                | "edit_presentation"
                | "capture_webpage"
                | "browse_page"
                | "read_sandbox_output"
                | "render_excalidraw"
                | "render_typst"
                | "render_video"
        )
    {
        return Some(match key {
            "lookup_ip" => ("tools-needs-geoip", "/admin/settings"),
            "generate_image" | "edit_image" => {
                ("tools-needs-image-backend", "/admin/models?tab=upstreams")
            }
            "capture_webpage" | "browse_page" if config.sandbox.is_some() => {
                ("tools-needs-sandbox-network", "/admin/settings")
            }
            _ => ("tools-needs-sandbox", "/admin/settings"),
        });
    }
    if key == "notify_user" && (!config.push.enabled || !push_ready) {
        return Some(("tools-needs-push", "/admin/settings"));
    }
    if key.starts_with("rag_") && (config.rag.is_none() || !rag_ready) {
        return Some(("tools-needs-rag", "/admin/settings"));
    }
    if (matches!(
        key,
        "fetch_attachment"
            | "upload_attachment"
            | "generate_qr_code"
            | "load_image_url"
            | "generate_image"
            | "edit_image"
            | "comfyui"
    ) || key.starts_with("typst_"))
        && config.chat.s3.is_none()
    {
        return Some(("tools-needs-storage", "/admin/settings"));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unconfigured_tools_have_specific_setup_destinations() {
        let config = aiplane_core::server::Config::default();
        assert_eq!(
            tool_configuration("generate_image", &config, false, false, false),
            Some(("tools-needs-image-backend", "/admin/models?tab=upstreams"))
        );
        assert_eq!(
            tool_configuration("run_in_sandbox", &config, false, false, false),
            Some(("tools-needs-sandbox", "/admin/settings"))
        );
        assert_eq!(
            tool_configuration("rag_search", &config, true, false, false),
            Some(("tools-needs-rag", "/admin/settings"))
        );
        assert_eq!(
            tool_configuration("read_skill", &config, true, false, false),
            None
        );
    }
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct ToolsToggleBody {
    pub tool_key: String,
    pub enabled: bool,
}

/// POST /api/v0/tools/toggle — set one tool's on/off state for the caller
/// (explicit value; idempotent, unlike the checkbox-presence form).
pub async fn tools_toggle_json(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    use rama::http::StatusCode;

    let lang = Lang::from_request(req.headers());
    let (_session, user) = require_session_json!(state, req);
    let (_, body) = req.into_parts();
    let parsed: ToolsToggleBody = match read_json(body, "the toggle body").await {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    // Refuse keys the caller's roles don't grant — toggling a tool you
    // cannot use would render a switch that lies.
    let entries = tool_toggles::entries_for_roles(&state, &user.roles, lang);
    if !entries.iter().any(|e| e.key == parsed.tool_key) {
        return not_found("no such tool for your roles");
    }
    match user_tool_prefs::set(&state.db, &user.id, &parsed.tool_key, parsed.enabled).await {
        Ok(()) => json_ok(
            StatusCode::OK,
            ToolToggled {
                tool_key: parsed.tool_key,
                enabled: parsed.enabled,
            },
        ),
        Err(err) => internal(err),
    }
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ToolToggled {
    pub tool_key: String,
    pub enabled: bool,
}

// ---------------------------------------------------------------------------
// Rendering
