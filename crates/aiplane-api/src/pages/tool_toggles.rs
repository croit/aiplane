// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Capability catalog shared by chat and token settings. Each visible row
//! represents one toggle key; runtime grants remain the outer bound.

use std::collections::HashSet;

use aiplane_runtime::rama_server::state::RamaState;
use aiplane_runtime::server::tools::catalog::Category;
use aiplane_runtime::server::tools::catalog::{self, ToolEntry};

#[derive(Clone, serde::Serialize)]
pub struct CapabilityEntry {
    pub key: String,
    pub kind: &'static str,
    pub title: String,
    pub description: String,
    pub group: String,
    pub order: u8,
    pub can_disable: bool,
    pub icon: Option<String>,
}

/// The account's granted capabilities, shared by chat and token pickers.
pub async fn capabilities_for_user(
    state: &RamaState,
    roles: &[String],
    user_id: &str,
) -> Vec<CapabilityEntry> {
    let mut views = entries_for_roles(state, roles)
        .into_iter()
        .filter(|entry| {
            entry.category != Category::Integrations && entry.key != catalog::READ_SKILL_ID
        })
        .map(|entry| CapabilityEntry {
            key: entry.key,
            kind: "tool",
            title: entry.title,
            description: entry.description,
            group: entry.category.key().to_string(),
            order: entry.category.order(),
            can_disable: true,
            icon: None,
        })
        .collect::<Vec<_>>();
    let role_ids = state.role_ids_for(roles);
    let admin = state.rbac.is_admin(&role_ids);
    let mcp_grant = state.mcp_grant_for(roles);
    let mut connector_keys = aiplane_core::server::db::user_mcp::connected_keys(&state.db, user_id)
        .await
        .unwrap_or_default();
    for connector in aiplane_core::server::db::mcp_catalog::list_enabled(&state.db)
        .await
        .unwrap_or_default()
    {
        if connector.is_global() && !connector_keys.contains(&connector.key) {
            connector_keys.push(connector.key);
        }
    }
    for connector_key in connector_keys {
        let Ok(Some(connector)) =
            aiplane_core::server::db::mcp_catalog::get(&state.db, &connector_key).await
        else {
            continue;
        };
        if !connector.enabled || !connector.allows(&role_ids, admin) {
            continue;
        }
        let key = format!(
            "{}{}",
            aiplane_runtime::server::tools::mcp::MCP_ID_PREFIX,
            connector_key
        );
        let permitted = match &mcp_grant {
            aiplane_core::server::rbac::resolver::McpGrant::Unscoped => true,
            aiplane_core::server::rbac::resolver::McpGrant::Scoped(grants) => grants
                .iter()
                .any(|grant| grant == &key || grant.starts_with(&format!("{key}__"))),
        };
        if !permitted {
            continue;
        }
        views.push(CapabilityEntry {
            key,
            kind: "tool",
            title: connector.name,
            description: connector.description.unwrap_or_default(),
            group: Category::Integrations.key().to_string(),
            order: Category::Integrations.order(),
            can_disable: true,
            icon: connector.icon,
        });
    }
    let registry = state.combined_skills_for(user_id);
    let skill_names = if state
        .allowed_tools_for_user(roles, user_id)
        .await
        .iter()
        .any(|id| id == catalog::READ_SKILL_ID)
    {
        state.allowed_skills_for(roles, user_id)
    } else {
        Vec::new()
    };
    for name in skill_names {
        let (title, description) = registry
            .as_ref()
            .and_then(|skills| skills.get(&name))
            .map(|skill| (skill.title.clone(), skill.description.clone()))
            .unwrap_or_else(|| (name.clone(), String::new()));
        views.push(CapabilityEntry {
            key: name,
            kind: "skill",
            title,
            description,
            group: "skills".to_string(),
            order: u8::MAX,
            can_disable: true,
            icon: None,
        });
    }
    views.sort_by(|left, right| {
        left.order
            .cmp(&right.order)
            .then_with(|| left.title.cmp(&right.title))
    });
    views
}

/// The capability toggle entries a user's roles grant, grouped + de-noised
/// for display. The single home both `/tools` and the `/tokens` per-token
/// panel resolve the row list through, so they never drift.
pub fn entries_for_roles(state: &RamaState, roles: &[String]) -> Vec<ToolEntry> {
    let role_ids = state.rbac.role_ids_for(roles);
    let mut allowed = state.rbac.allowed_tools(&role_ids, &state.tools());
    state.expand_comfyui_tools(&mut allowed, &role_ids);
    let comfyui_metas = state
        .comfyui()
        .as_ref()
        .map(|h| {
            h.store
                .current()
                .workflows()
                .into_iter()
                .map(|m| catalog::ComfyuiMeta {
                    tool_id: format!("comfyui_{}", m.id),
                    title: m.title.clone(),
                    description: m.description.clone(),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    catalog::entries(
        &state.tools(),
        &allowed,
        &state.typst_templates(),
        &comfyui_metas,
    )
}

/// The set of valid toggle keys for these entries — used to reject bogus
/// keys before persisting a choice.
pub fn valid_keys(entries: &[ToolEntry]) -> HashSet<String> {
    entries.iter().map(|e| e.key.clone()).collect()
}
