// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Capability catalog shared by chat, token settings and the agent setup.
//! Each visible row represents one toggle key; runtime grants remain the
//! outer bound. Every surface builds a row through the constructors on
//! [`CapabilityEntry`], so a resource reads the same wherever it is offered:
//! a tool by its catalog copy, a connector, skill or knowledge base by what
//! its admin wrote on it, and nothing in its place when that is empty.

use std::collections::HashSet;

use session_core::i18n::{Lang, t};

use aiplane_core::server::db::{mcp_catalog, rag};
use aiplane_features::server::skills::Skill;
use aiplane_runtime::rama_server::state::RamaState;
use aiplane_runtime::server::tools::catalog::Category;
use aiplane_runtime::server::tools::catalog::{self, ToolEntry};
use aiplane_runtime::server::tools::mcp::MCP_ID_PREFIX;

#[derive(Clone, serde::Serialize, schemars::JsonSchema)]
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

/// The `kind` of a knowledge base row, which only the agent setup offers.
pub const COLLECTION_KIND: &str = "rag_collection";

pub fn builtin_display_copy(tool_id: &str, lang: Lang) -> Option<(String, String)> {
    match tool_id {
        "forward_request" => Some((
            t(lang, "tool-forward-request-title"),
            t(lang, "tool-forward-request-description"),
        )),
        _ => None,
    }
}

fn localized_entry(mut entry: ToolEntry, lang: Lang) -> ToolEntry {
    if let Some((title, description)) = builtin_display_copy(&entry.key, lang) {
        entry.title = title;
        entry.description = description;
    }
    entry
}

impl CapabilityEntry {
    pub fn tool(entry: ToolEntry) -> Self {
        Self {
            key: entry.key,
            kind: "tool",
            title: entry.title,
            description: entry.description,
            group: entry.category.key().to_string(),
            order: entry.category.order(),
            can_disable: true,
            icon: None,
        }
    }

    pub fn connector(connector: mcp_catalog::Connector) -> Self {
        Self {
            key: format!("{MCP_ID_PREFIX}{}", connector.key),
            kind: "tool",
            title: connector.name,
            description: connector.description.unwrap_or_default(),
            group: Category::Integrations.key().to_string(),
            order: Category::Integrations.order(),
            can_disable: true,
            icon: connector.icon,
        }
    }

    /// A skill the registry no longer knows keeps its name and no description.
    pub fn skill(name: String, skill: Option<&Skill>) -> Self {
        let (title, description) = skill.map_or_else(
            || (name.clone(), String::new()),
            |s| (s.title.clone(), s.description.clone()),
        );
        Self {
            key: name,
            kind: "skill",
            title,
            description,
            group: "skills".to_string(),
            order: u8::MAX,
            can_disable: true,
            icon: None,
        }
    }

    pub fn collection(collection: rag::Collection) -> Self {
        Self {
            key: collection.id.to_string(),
            kind: COLLECTION_KIND,
            title: collection.name,
            description: collection.description.unwrap_or_default(),
            group: Category::Knowledge.key().to_string(),
            order: Category::Knowledge.order(),
            can_disable: true,
            icon: None,
        }
    }
}

/// Category order first, then title: how every capability list reads.
pub fn sort_entries<T>(rows: &mut [T], entry: impl Fn(&T) -> &CapabilityEntry) {
    rows.sort_by(|left, right| {
        let (left, right) = (entry(left), entry(right));
        left.order
            .cmp(&right.order)
            .then_with(|| left.title.cmp(&right.title))
    });
}

/// The capabilities an API token of this account can use: everything
/// [`capabilities_for_user`] lists, minus the tools that only run inside a chat
/// session. The token panel shows exactly these and the token-tools endpoint
/// accepts exactly these, so neither offers nor stores a switch `/v1` ignores.
pub async fn token_capabilities_for_user(
    state: &RamaState,
    roles: &[String],
    user_id: &str,
    lang: Lang,
) -> Vec<CapabilityEntry> {
    let tools = state.tools();
    let usable = catalog::api_keys(tools.ids());
    capabilities_for_user(state, roles, user_id, lang)
        .await
        .into_iter()
        .filter(|entry| {
            entry.kind != "tool"
                || entry
                    .key
                    .starts_with(aiplane_runtime::server::tools::mcp::MCP_ID_PREFIX)
                || usable.contains(&entry.key)
        })
        .collect()
}

/// The account's granted capabilities, shared by chat and token pickers.
pub async fn capabilities_for_user(
    state: &RamaState,
    roles: &[String],
    user_id: &str,
    lang: Lang,
) -> Vec<CapabilityEntry> {
    let mut views = entries_for_roles(state, roles, lang)
        .into_iter()
        .filter(|entry| {
            entry.category != Category::Integrations && entry.key != catalog::READ_SKILL_ID
        })
        .map(CapabilityEntry::tool)
        .collect::<Vec<_>>();
    let role_ids = state.role_ids_for(roles);
    let admin = state.rbac.is_admin(&role_ids);
    let mcp_grant = state.mcp_grant_for(roles);
    let mut connector_keys = aiplane_core::server::db::mcp_connections::connected_keys(
        &state.db,
        &aiplane_core::server::db::mcp_connections::ConnectionOwner::User(user_id.to_owned()),
    )
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
        let key = format!("{MCP_ID_PREFIX}{connector_key}");
        let permitted = match &mcp_grant {
            aiplane_core::server::rbac::resolver::McpGrant::Unscoped => true,
            aiplane_core::server::rbac::resolver::McpGrant::Scoped(grants) => grants
                .iter()
                .any(|grant| grant == &key || grant.starts_with(&format!("{key}__"))),
        };
        if !permitted {
            continue;
        }
        views.push(CapabilityEntry::connector(connector));
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
        let skill = registry.as_ref().and_then(|skills| skills.get(&name));
        views.push(CapabilityEntry::skill(name, skill));
    }
    sort_entries(&mut views, |entry| entry);
    views
}

/// The capability toggle entries a user's roles grant, grouped + de-noised
/// for display. The single home both `/tools` and the `/tokens` per-token
/// panel resolve the row list through, so they never drift.
pub fn entries_for_roles(state: &RamaState, roles: &[String], lang: Lang) -> Vec<ToolEntry> {
    let role_ids = state.rbac.role_ids_for(roles);
    let mut allowed = state.rbac.allowed_tools(&role_ids, &state.tools());
    state.expand_comfyui_tools(&mut allowed, &role_ids);
    entries_for_tools(state, &allowed, lang)
}

/// The catalog rows for exactly the tool ids `allowed`: one per toggle key,
/// so a family (memory, a typst template, ComfyUI) is one row.
pub fn entries_for_tools(state: &RamaState, allowed: &[String], lang: Lang) -> Vec<ToolEntry> {
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
        allowed,
        &state.typst_templates(),
        &comfyui_metas,
    )
    .into_iter()
    .map(|entry| localized_entry(entry, lang))
    .collect()
}

/// The set of valid toggle keys for these entries — used to reject bogus
/// keys before persisting a choice.
pub fn valid_keys(entries: &[ToolEntry]) -> HashSet<String> {
    entries.iter().map(|e| e.key.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resource_entry(key: &str) -> ToolEntry {
        ToolEntry {
            key: key.into(),
            title: "Operator-provided title".into(),
            tech: key.into(),
            description: "Operator-provided description".into(),
            category: Category::Utility,
        }
    }

    #[test]
    fn builtin_display_copy_translates_forward_request_in_every_language() {
        let english = builtin_display_copy("forward_request", Lang::En).unwrap();
        assert_eq!(english.0, "Ask another Dot");
        for lang in Lang::ALL {
            let copy = builtin_display_copy("forward_request", lang).unwrap();
            assert!(!copy.0.is_empty());
            assert!(!copy.1.is_empty());
            assert_ne!(copy.0, "tool-forward-request-title");
            assert_ne!(copy.1, "tool-forward-request-description");
            if lang != Lang::En {
                assert_ne!(copy, english);
            }
        }
    }

    #[test]
    fn localized_entry_preserves_actual_resource_data_and_technical_identity() {
        for key in [
            "mcp__workspace",
            "typst_invoice",
            "comfyui",
            "operator_tool",
        ] {
            let actual = resource_entry(key);
            assert_eq!(localized_entry(actual.clone(), Lang::De), actual);
            assert!(builtin_display_copy(key, Lang::De).is_none());
        }
        let actual = resource_entry("forward_request");
        let translated = localized_entry(actual.clone(), Lang::De);
        assert_ne!(translated.title, actual.title);
        assert_ne!(translated.description, actual.description);
        assert_eq!(translated.key, actual.key);
        assert_eq!(translated.tech, actual.tech);
        assert_eq!(translated.category, actual.category);
        let capability = CapabilityEntry::tool(translated.clone());
        assert_eq!(capability.title, translated.title);
        assert_eq!(capability.description, translated.description);
    }

    #[test]
    fn localized_entry_keeps_missing_dynamic_description_missing() {
        let mut actual = resource_entry("operator_tool");
        actual.description.clear();
        assert_eq!(localized_entry(actual.clone(), Lang::Zh), actual);
    }
}
