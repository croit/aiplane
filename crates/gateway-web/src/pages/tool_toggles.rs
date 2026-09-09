// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Reusable tool on/off toggle list — the grouped, daisyUI-styled
//! capability switches shared by the per-user `/tools` page and the
//! per-token panel on `/tokens`.
//!
//! Both pages render the *same* catalog of capabilities (grouped by
//! [`Category`], one row per toggle key) and persist a negative
//! allowlist (default-on; a row records an explicit disable). They differ
//! only in where a toggle POSTs and how its row's DOM id is namespaced —
//! captured by [`ToggleCtx`] — so the markup, the checkbox-presence
//! convergence trick, and the category grouping all live here once.

use std::collections::HashSet;

use gateway_runtime::rama_server::state::RamaState;
use gateway_runtime::server::tools::catalog::{self, ToolEntry};

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
