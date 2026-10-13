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
use serde::{Deserialize, Serialize};
use session_core::i18n::Lang;

use super::json_principals::require_agent_manager;
use super::tool_toggles::{CapabilityEntry, entries_for_tools, sort_entries};
use super::{internal, json_ok};
use aiplane_core::server::db::users::User;
use aiplane_core::server::db::{WriteTx, mcp_catalog, rag as rag_db};
use aiplane_core::server::feature_defaults::{self, Feature};
use aiplane_core::server::upstreams::{PoolAccess, PoolKind};
use aiplane_runtime::agents::assist::Ability;
use aiplane_runtime::rama_server::state::RamaState;
use aiplane_runtime::server::grant_holding;
use aiplane_runtime::server::model_choices::{self, ModelChoice, gateway_default};
use aiplane_runtime::server::{AppState, tools::catalog};

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
fn grantable_tool_ids(state: &AppState, role_ids: &[String]) -> Vec<String> {
    let grantable = state.grantable_tool_ids();
    let mut held = state.rbac.allowed_tools(role_ids, &state.tools());
    state.expand_comfyui_tools(&mut held, role_ids);
    held.into_iter()
        .filter(|id| !id.starts_with(MCP_TOOL_PREFIX) && grantable.contains(id))
        .collect()
}

/// The tools a manager in groups `role_ids` may grant, one row per catalog
/// entry, granting every tool id the entry stands for.
fn tool_items(
    display: &RamaState,
    state: &AppState,
    role_ids: &[String],
    lang: Lang,
) -> Vec<GrantableItem> {
    let held = grantable_tool_ids(state, role_ids);
    entries_for_tools(display, &held, lang)
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
    tool_items(state, state, role_ids, Lang::En)
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
    let mut tx = WriteTx::begin(&state.db).await.map_err(internal)?;
    let Some((current, policy)) =
        aiplane_runtime::agents::access::current_user_policy_in(state, &mut tx, &user.id)
            .await
            .map_err(internal)?
    else {
        return Ok(Vec::new());
    };
    let keys = grant_holding::grantable_connectors_in(state, &mut tx, &current.id, None)
        .await
        .map_err(internal)?;
    let connectors = mcp_catalog::list_enabled_in(&mut tx)
        .await
        .map_err(internal)?
        .into_iter()
        .filter(|connector| keys.contains(&connector.key))
        .collect();
    let collections = rag_db::list_collections_in(&mut tx)
        .await
        .map_err(internal)?;
    let mut app = (**state).clone();
    app.rbac = Arc::new(policy);
    let items = assemble_items(state, &app, &current, lang, connectors, collections);
    tx.commit().await.map_err(internal)?;
    Ok(items)
}

fn assemble_items(
    display: &RamaState,
    state: &AppState,
    user: &User,
    lang: Lang,
    connectors: Vec<mcp_catalog::Connector>,
    collections: Vec<rag_db::Collection>,
) -> Vec<GrantableItem> {
    let role_ids = state.rbac.role_ids_for(&user.roles);
    let is_admin = state.rbac.is_admin(&role_ids);
    let admin_page = |url: String| is_admin.then_some(url);
    let mut items = tool_items(display, state, &role_ids, lang);

    let grantable = state.grantable_tool_ids();
    for connector in connectors {
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

    for collection in collections.into_iter().filter(|collection| {
        state
            .rbac
            .resource_allowed(&role_ids, &collection.allowed_groups)
    }) {
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
    items
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

#[derive(Deserialize, schemars::JsonSchema)]
pub struct AgentResourcesQuery {
    pub agent_id: Option<String>,
}

pub async fn resources(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let lang = Lang::from_request(req.headers());
    let user = match require_agent_manager(&state, &req).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    let query = AgentResourcesQuery {
        agent_id: super::query_map(&req).get("agent_id").cloned(),
    };
    async {
        let mut tx = WriteTx::begin(&state.db).await.map_err(internal)?;
        let (current, policy) =
            super::json_organizations::current_policy_in(&state, &mut tx, &user.id, lang).await?;
        if !policy.can_manage_agents(&policy.role_ids_for(&current.roles)) {
            return Err(super::json_error(
                StatusCode::FORBIDDEN,
                "forbidden",
                &session_core::i18n::t(lang, "agent-management-error-permission-required"),
            ));
        }
        if let Some(id) = query.agent_id.as_deref() {
            let held = aiplane_runtime::agents::access::effective_access_in(
                &state,
                &mut tx,
                id,
                &current.id,
            )
            .await
            .map_err(internal)?;
            super::json_agents::require_held(held, id, aiplane_agents::db::agents::Access::Read)?;
        }
        let mut app = (**state).clone();
        app.rbac = Arc::new(policy);
        let view = resources_in(
            &state,
            &app,
            &mut tx,
            &current,
            lang,
            query.agent_id.as_deref(),
        )
        .await?;
        tx.commit().await.map_err(internal)?;
        Ok::<_, Response>(json_ok(StatusCode::OK, view))
    }
    .await
    .unwrap_or_else(|response| response)
}

/// What the caller holds and may grant an agent.
#[derive(Serialize, schemars::JsonSchema)]
pub struct AgentResources {
    models: GrantableModels,
    /// The gateway's default model of each kind: what an agent's unset model
    /// key runs on.
    defaults: DefaultModels,
    /// Tools, connectors, skills and knowledge bases.
    pub(super) items: Vec<GrantableItem>,
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
    model_views(state, kind, grantable_models(state, user, kind).await)
}

fn model_views(state: &AppState, kind: PoolKind, choices: Vec<ModelChoice>) -> Vec<GrantableModel> {
    choices
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

async fn model_list_in(
    state: &AppState,
    tx: &mut WriteTx,
    user: &User,
    kind: PoolKind,
) -> Result<Vec<GrantableModel>, Response> {
    let mut choices =
        model_choices::offered_in(state, tx, kind, &state.pool_access_for(&user.roles))
            .await
            .map_err(internal)?;
    choices.retain(ModelChoice::grantable);
    Ok(model_views(state, kind, choices))
}

async fn default_in(
    state: &AppState,
    tx: &mut WriteTx,
    feature: Feature,
) -> Result<Option<String>, Response> {
    let offered = model_choices::offered_in(state, tx, feature.pool_kind(), &PoolAccess::all())
        .await
        .map_err(internal)?
        .into_iter()
        .map(|choice| choice.id)
        .collect::<Vec<_>>();
    let configured = feature_defaults::get_in(tx, feature)
        .await
        .map_err(internal)?;
    Ok(feature_defaults::resolve(configured.as_deref(), &offered))
}

async fn resources_in(
    display: &RamaState,
    state: &AppState,
    tx: &mut WriteTx,
    user: &User,
    lang: Lang,
    target_principal_id: Option<&str>,
) -> Result<AgentResources, Response> {
    let models = GrantableModels {
        chat: model_list_in(state, tx, user, PoolKind::Chat).await?,
        transcription: model_list_in(state, tx, user, PoolKind::Transcription).await?,
        speech: model_list_in(state, tx, user, PoolKind::Speech).await?,
    };
    let keys = grant_holding::grantable_connectors_in(display, tx, &user.id, target_principal_id)
        .await
        .map_err(internal)?;
    let connectors = mcp_catalog::list_enabled_in(tx)
        .await
        .map_err(internal)?
        .into_iter()
        .filter(|connector| keys.contains(&connector.key))
        .collect();
    let collections = rag_db::list_collections_in(tx).await.map_err(internal)?;
    let items = assemble_items(display, state, user, lang, connectors, collections);
    let defaults = DefaultModels {
        chat: default_in(state, tx, Feature::Chat).await?,
        transcription: default_in(state, tx, Feature::Transcription).await?,
        speech: default_in(state, tx, Feature::Speech).await?,
    };
    Ok(AgentResources {
        models,
        defaults,
        items,
    })
}
