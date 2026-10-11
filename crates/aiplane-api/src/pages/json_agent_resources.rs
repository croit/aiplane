// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! `GET /api/v0/agent-resources` — what the calling manager holds and so may
//! grant to an agent, for the builder's pickers (`docs/agent-builder.md` → "Test chat").
//!
//! It lists with the same predicates `POST /system-principals/{id}/grants`
//! checks one reference against (`json_principals::manager_holds`), so the
//! picker never offers what the grant would refuse. The grant stays the
//! authority: a resource that changed between listing and granting is still
//! refused there, with its reason.
//!
//! `models` lists, per kind (chat, transcription, speech), the models the
//! caller may use — the chat picker's list (`server::model_choices`), minus
//! any automatic route whose candidates or selector they may not use, since
//! granting the route would hand those on.
//!
//! A speech model also lists its `voices` (`UpstreamRegistry::speech_voices_of`):
//! the ones an agent's `publish.voice.voice` may name, as publishing checks.
//!
//! `defaults` names the gateway's default model of each kind (Models &
//! routing → Default models): what an agent's unset model key runs on.
//! Whether the caller may grant it is whether `models` lists it.
//!
//! `items` are the tools, connectors, skills and knowledge bases, each the
//! row the chat picker shows for it (`tool_toggles::CapabilityEntry`, so the
//! title and description are the resource's own), with the grant it takes,
//! the tool ids it puts into the spec, and its edit page when the caller may
//! maintain it there.

use std::sync::Arc;

use rama::http::service::web::extract::State;
use rama::http::{Request, Response, StatusCode};
use serde::Serialize;
use session_core::i18n::Lang;

use super::json_principals::require_agent_manager;
use super::tool_toggles::{CapabilityEntry, entries_for_tools, sort_entries};
use super::{internal, json_ok};
use aiplane_core::server::db::users::User;
use aiplane_core::server::db::{mcp_catalog, rag as rag_db};
use aiplane_core::server::feature_defaults::Feature;
use aiplane_core::server::upstreams::PoolKind;
use aiplane_runtime::agents::assist::Ability;
use aiplane_runtime::rama_server::state::RamaState;
use aiplane_runtime::server::model_choices::{self, ModelChoice, gateway_default};
use aiplane_runtime::server::tools::catalog;

const MCP_TOOL_PREFIX: &str = aiplane_runtime::server::tools::mcp::MCP_ID_PREFIX;

/// One resource the agent setup offers: the row the chat picker shows for
/// it, plus what switching it on grants and puts into the spec, and where
/// the viewer maintains it when they may.
#[derive(Serialize, schemars::JsonSchema)]
pub(super) struct GrantableItem {
    #[serde(flatten)]
    pub entry: CapabilityEntry,
    pub grant: GrantRefs,
    /// The tool ids switching it on puts into the spec's `main.tools`.
    pub tools: Vec<String>,
    pub editable: bool,
    pub config_url: Option<String>,
}

/// The grant switching a resource on takes.
#[derive(Serialize, schemars::JsonSchema)]
pub(super) struct GrantRefs {
    pub kind: &'static str,
    pub refs: Vec<String>,
}

impl GrantableItem {
    fn new(entry: CapabilityEntry, kind: &'static str, refs: Vec<String>) -> Self {
        Self {
            entry,
            grant: GrantRefs { kind, refs },
            tools: Vec::new(),
            editable: false,
            config_url: None,
        }
    }

    fn edited_at(mut self, url: Option<String>) -> Self {
        self.editable = url.is_some();
        self.config_url = url;
        self
    }
}

/// The registry tool ids (MCP tools aside: those come with their connector)
/// a manager in groups `role_ids` holds and may therefore grant.
fn grantable_tool_ids(state: &RamaState, role_ids: &[String]) -> Vec<String> {
    let grantable = state.grantable_tool_ids();
    let mut held = state.rbac.allowed_tools(role_ids, &state.tools());
    state.expand_comfyui_tools(&mut held, role_ids);
    held.into_iter()
        .filter(|id| !id.starts_with(MCP_TOOL_PREFIX) && grantable.contains(id))
        .collect()
}

/// The tools a manager in groups `role_ids` may grant, one row per catalog
/// entry, granting every tool id the entry stands for.
fn tool_items(state: &RamaState, role_ids: &[String], lang: Lang) -> Vec<GrantableItem> {
    let held = grantable_tool_ids(state, role_ids);
    entries_for_tools(state, &held, lang)
        .into_iter()
        .map(|entry| {
            let refs: Vec<String> = held
                .iter()
                .filter(|id| catalog::entry_key_for(id) == entry.key)
                .cloned()
                .collect();
            let mut item = GrantableItem::new(CapabilityEntry::tool(entry), "tool", refs.clone());
            item.tools = refs;
            item
        })
        .collect()
}

/// The tools a manager in groups `role_ids` may grant, by tool id, each
/// named as its catalog entry names it.
pub(super) fn grantable_tools(state: &RamaState, role_ids: &[String]) -> Vec<Ability> {
    tool_items(state, role_ids, Lang::En)
        .into_iter()
        .flat_map(|item| {
            let entry = item.entry;
            item.grant.refs.into_iter().map(move |id| Ability {
                id,
                name: entry.title.clone(),
                description: Some(entry.description.clone()).filter(|d| !d.is_empty()),
            })
        })
        .collect()
}

/// Everything `user` holds and may grant an agent — tools, connectors,
/// skills and knowledge bases — as rows built like the chat picker's.
pub(super) async fn grantable_items(
    state: &RamaState,
    user: &User,
    lang: Lang,
) -> Result<Vec<GrantableItem>, Response> {
    let role_ids = state.rbac.role_ids_for(&user.roles);
    let is_admin = state.rbac.is_admin(&role_ids);
    let admin_page = |url: String| is_admin.then_some(url);
    let mut items = tool_items(state, &role_ids, lang);

    let grantable = state.grantable_tool_ids();
    let mcp_grant = state.mcp_grant_for(&user.roles);
    for connector in mcp_catalog::list_enabled(&state.db)
        .await
        .map_err(internal)?
    {
        let offered = connector.has_shared_identity()
            && if connector.is_agent() {
                connector.grantable_by(&role_ids, is_admin)
            } else {
                let key = format!("{MCP_TOOL_PREFIX}{}", connector.key);
                connector.allows(&role_ids, is_admin) && mcp_grant.allows(&key, &key)
            };
        if !offered {
            continue;
        }
        let prefix = format!("{MCP_TOOL_PREFIX}{}__", connector.key);
        let key = connector.key.clone();
        let mut item = GrantableItem::new(
            CapabilityEntry::connector(connector),
            "connector",
            vec![key.clone()],
        )
        .edited_at(admin_page(format!("/admin/connectors/{key}/edit")));
        item.tools = grantable
            .iter()
            .filter(|id| id.starts_with(&prefix))
            .cloned()
            .collect();
        items.push(item);
    }

    if let Some(store) = state.skills() {
        let registry = store.current();
        for name in state.rbac.allowed_skills(&role_ids, &registry) {
            let Some(skill) = registry.get(&name) else {
                continue;
            };
            let entry = CapabilityEntry::skill(name.clone(), Some(skill));
            items.push(
                GrantableItem::new(entry, "skill", vec![name])
                    .edited_at(admin_page("/admin/skills".into())),
            );
        }
    }

    for collection in grantable_collections(state, &role_ids).await? {
        let id = collection.id.to_string();
        let url = admin_page(format!("/rag/{id}/edit"));
        items.push(
            GrantableItem::new(
                CapabilityEntry::collection(collection),
                "rag_collection",
                vec![id],
            )
            .edited_at(url),
        );
    }
    sort_entries(&mut items, |item| &item.entry);
    Ok(items)
}

/// The knowledge bases (RAG collections) a manager in groups `role_ids` may
/// read and may therefore grant.
pub(super) async fn grantable_collections(
    state: &RamaState,
    role_ids: &[String],
) -> Result<Vec<rag_db::Collection>, Response> {
    let collections = rag_db::list_collections(&state.db)
        .await
        .map_err(internal)?;
    Ok(collections
        .into_iter()
        .filter(|c| state.rbac.resource_allowed(role_ids, &c.allowed_groups))
        .collect())
}

/// The models of `kind` `user` may grant: the ones they may use, an
/// automatic route only when they may use every model it reaches.
pub(super) async fn grantable_models(
    state: &RamaState,
    user: &User,
    kind: PoolKind,
) -> Vec<ModelChoice> {
    let access = state.pool_access_for(&user.roles);
    let mut choices = model_choices::offered(state, kind, &access).await;
    choices.retain(ModelChoice::grantable);
    choices
}

/// The chat models `user` may grant, by name.
pub(super) async fn grantable_chat_models(state: &RamaState, user: &User) -> Vec<String> {
    grantable_models(state, user, PoolKind::Chat)
        .await
        .into_iter()
        .map(|c| c.id)
        .collect()
}

pub async fn resources(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let lang = Lang::from_request(req.headers());
    let user = match require_agent_manager(&state, &req).await {
        Ok(user) => user,
        Err(resp) => return resp,
    };
    match resources_for(&state, &user, lang).await {
        Ok(view) => json_ok(StatusCode::OK, view),
        Err(resp) => resp,
    }
}

/// What the caller holds and may grant an agent.
#[derive(Serialize, schemars::JsonSchema)]
pub struct AgentResources {
    models: GrantableModels,
    /// The gateway's default model of each kind: what an agent's unset model
    /// key runs on.
    defaults: DefaultModels,
    /// Tools, connectors, skills and knowledge bases.
    items: Vec<GrantableItem>,
}

/// The models the caller may grant, by kind.
#[derive(Serialize, schemars::JsonSchema)]
pub struct GrantableModels {
    pub chat: Vec<GrantableModel>,
    pub transcription: Vec<GrantableModel>,
    pub speech: Vec<GrantableModel>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct GrantableModel {
    pub id: String,
    /// Every pool serving it has GDPR safeguards.
    pub gdpr: bool,
    /// Every pool serving it is covered by a confidentiality agreement.
    pub nda: bool,
    /// A speech model's voices; absent on other kinds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voices: Option<Vec<String>>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct DefaultModels {
    pub chat: Option<String>,
    pub transcription: Option<String>,
    pub speech: Option<String>,
}

async fn model_list(state: &RamaState, user: &User, kind: PoolKind) -> Vec<GrantableModel> {
    grantable_models(state, user, kind)
        .await
        .into_iter()
        .map(|c| GrantableModel {
            voices: (kind == PoolKind::Speech).then(|| state.upstreams.speech_voices_of(&c.id)),
            id: c.id,
            gdpr: c.compliance.gdpr,
            nda: c.compliance.nda,
        })
        .collect()
}

/// What `user` holds and may grant, as `GET /api/v0/agent-resources`
/// answers it.
pub(super) async fn resources_for(
    state: &RamaState,
    user: &User,
    lang: Lang,
) -> Result<AgentResources, Response> {
    let models = GrantableModels {
        chat: model_list(state, user, PoolKind::Chat).await,
        transcription: model_list(state, user, PoolKind::Transcription).await,
        speech: model_list(state, user, PoolKind::Speech).await,
    };

    let items = grantable_items(state, user, lang).await?;

    let defaults = DefaultModels {
        chat: gateway_default(state, Feature::Chat).await,
        transcription: gateway_default(state, Feature::Transcription).await,
        speech: gateway_default(state, Feature::Speech).await,
    };

    Ok(AgentResources {
        models,
        defaults,
        items,
    })
}
