// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use std::{collections::HashMap, sync::Arc};

use aiplane_agents::db::{agents, architect_sessions, system_principals::NewPrincipal};
use aiplane_core::server::{
    db::{WriteTx, users::User},
    principal::GrantSet,
};
use aiplane_runtime::server::tools::ToolSource;
use aiplane_runtime::{
    agents::{
        assist::{ReviewContext, apply_changes},
        spec::AgentSpec,
    },
    rama_server::state::RamaState,
    server::tools::catalog,
};
use rama::http::{Request, Response, StatusCode, service::web::extract::State};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use session_core::i18n::{Lang, t};

use super::super::{internal, json_error, json_ok, not_found, raw_path_segment};

const SUPPORTED_CHANGES: &[&str] = &["task", "tone", "scope", "abilities", "knowledge"];

pub(super) fn personal_changes_schema() -> Value {
    let mut schema = aiplane_runtime::agents::assist::changes_schema();
    schema["properties"]
        .as_object_mut()
        .expect("changes schema has properties")
        .retain(|name, _| SUPPORTED_CHANGES.contains(&name.as_str()));
    schema["description"] = json!(
        "An actual JSON object containing supported configuration changes. Do not encode this object as a string."
    );
    schema
}

#[derive(Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct DotPlanTool {
    pub id: String,
    pub capability_key: String,
    pub label: Option<String>,
    pub description: Option<String>,
    pub permission: String,
}

#[derive(Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct DotPlanKnowledge {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct DotPlan {
    pub revision: String,
    pub source_turn_id: String,
    pub name: String,
    pub display: String,
    pub description: String,
    pub draft_spec: Value,
    pub tools: Vec<DotPlanTool>,
    pub knowledge: Option<DotPlanKnowledge>,
    pub dropped: Vec<Value>,
    pub unresolved: Vec<String>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct DotPlanReply {
    pub proposal: Option<DotPlan>,
    pub dot_id: Option<String>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct AppliedDotPlan {
    pub dot: super::super::json_dots::DotView,
    pub session_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProposalArgs {
    name: String,
    #[serde(default)]
    display: Option<String>,
    #[serde(default)]
    description: String,
    changes: serde_json::Map<String, Value>,
    #[serde(default)]
    unresolved: Vec<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApplyDotPlanBody {
    pub revision: String,
}

fn conflict(lang: Lang) -> Response {
    json_error(
        StatusCode::CONFLICT,
        "conflict",
        &t(lang, "dot-setup-error-conflict"),
    )
}

pub(super) async fn current_manager(
    state: &RamaState,
    tx: &mut WriteTx,
    user_id: &str,
    lang: Lang,
) -> Result<User, Response> {
    let Some((user, policy)) =
        aiplane_runtime::agents::access::current_user_policy_in(state, tx, user_id)
            .await
            .map_err(internal)?
    else {
        return Err(not_found(t(lang, "dot-error-not-found")));
    };
    if !policy.can_manage_dots(&policy.role_ids_for(&user.roles)) {
        return Err(json_error(
            StatusCode::FORBIDDEN,
            "forbidden",
            &t(lang, "dot-error-permission-required"),
        ));
    }
    Ok(user)
}

async fn owned_session(
    state: &RamaState,
    tx: &mut WriteTx,
    user_id: &str,
    session_id: &str,
    lang: Lang,
) -> Result<architect_sessions::ArchitectSession, Response> {
    let session = architect_sessions::get_in(tx, session_id)
        .await
        .map_err(internal)?
        .filter(|session| {
            session.user_id == user_id && session.dot_owner.as_deref() == Some("personal")
        })
        .ok_or_else(|| not_found(t(lang, "dot-error-not-found")))?;
    current_manager(state, tx, user_id, lang).await?;
    Ok(session)
}

async fn current_tools_in(
    state: &RamaState,
    tx: &mut WriteTx,
    user: &User,
) -> Result<Vec<String>, Response> {
    let (_, policy) = aiplane_runtime::agents::access::current_user_policy_in(state, tx, &user.id)
        .await
        .map_err(internal)?
        .ok_or_else(|| not_found(t(Lang::En, "dot-error-not-found")))?;
    let groups = policy.role_ids_for(&user.roles);
    let mut tools = policy.allowed_tools(&groups, &state.tools());
    state.expand_comfyui_tools(&mut tools, &groups);
    let disabled = aiplane_core::server::db::user_tool_prefs::disabled_for_user_in(tx, &user.id)
        .await
        .map_err(internal)?;
    catalog::retain_enabled(&mut tools, &disabled);
    tools.retain(|id| !catalog::is_hidden(id));
    Ok(tools)
}

fn localized_plan(mut plan: DotPlan, lang: Lang) -> DotPlan {
    let localized = |value: &str| {
        if [
            "dot-plan-unsupported",
            "dot-plan-strict-scope-unavailable",
            "dot-plan-one-knowledge",
            "dot-plan-task-required",
            "dot-plan-connection-required",
            "dot-access-error-unavailable",
            "dot-setup-error-unavailable",
        ]
        .contains(&value)
        {
            t(lang, value)
        } else {
            value.to_string()
        }
    };
    for unresolved in &mut plan.unresolved {
        *unresolved = localized(unresolved);
    }
    for dropped in &mut plan.dropped {
        if let Some(reason) = dropped.get("reason").and_then(Value::as_str) {
            dropped["reason"] = json!(localized(reason));
        }
    }
    plan
}

pub(crate) fn supported_tool(
    state: &RamaState,
    id: &str,
) -> Option<Arc<dyn aiplane_runtime::server::tools::Tool>> {
    let registry = state.tools();
    let mcp = aiplane_runtime::server::tools::mcp::manager::UserMcpLayer::default();
    let comfyui = state.comfyui().as_ref().map(|handle| {
        aiplane_runtime::server::comfyui_tool::ComfyuiToolSource::new((**handle).clone())
    });
    aiplane_runtime::server::tools::mcp::manager::CompositeToolSource::new(registry.as_ref(), &mcp)
        .with_comfyui(comfyui.as_ref())
        .get(id)
}

/// The rule a newly selected tool starts with: Allow for the default memory
/// tools, for read-only tools and for Ask another agent, Ask before each call
/// for everything else. *Chosen:* Ask another agent reaches no agent until
/// both consents of the pair are set — that is the owner's permission — and
/// the receiver keeps its own rules, so asking again on every call would only
/// stop delegation from being useful. The owner can still set it to Ask.
pub(crate) fn default_permission(
    defaults: &AgentSpec,
    id: &str,
    tool: Option<&dyn aiplane_runtime::server::tools::Tool>,
) -> &'static str {
    if defaults
        .main
        .tool_resources
        .get(id)
        .and_then(|resource| resource.permission)
        == Some(aiplane_runtime::agents::spec::model::Permission::AlwaysAllow)
        || tool.is_some_and(|tool| tool.read_only())
        || id == aiplane_runtime::agents::router::FORWARD_TOOL_NAME
    {
        "always_allow"
    } else {
        "always_ask"
    }
}

async fn personal_candidates(
    state: &RamaState,
    user: &User,
    allowed: &[String],
) -> Result<aiplane_runtime::agents::assist::Candidates, Response> {
    use aiplane_runtime::agents::assist::{Ability, Candidates, Knowledge};
    let resources =
        super::super::json_agent_resources::resources_for(state, user, Lang::En).await?;
    let mut abilities = Vec::new();
    let mut knowledge = Vec::new();
    for item in resources.items {
        if item.grant.kind == "tool" {
            for id in item
                .grant
                .refs
                .into_iter()
                .filter(|id| allowed.contains(id))
            {
                abilities.push(Ability {
                    id,
                    name: item.entry.title.clone(),
                    description: (!item.entry.description.is_empty())
                        .then(|| item.entry.description.clone()),
                });
            }
        } else if item.grant.kind == "rag_collection" && allowed.iter().any(|id| id == "rag_search")
        {
            for id in item.grant.refs {
                knowledge.push(Knowledge {
                    id,
                    name: item.entry.title.clone(),
                    description: (!item.entry.description.is_empty())
                        .then(|| item.entry.description.clone()),
                });
            }
        }
    }
    Ok(Candidates {
        abilities,
        knowledge,
        agents: Vec::new(),
        models: Vec::new(),
    })
}

pub(super) async fn resources(state: &RamaState, user: &User) -> Result<Value, Response> {
    let mut tx = WriteTx::begin(&state.db).await.map_err(internal)?;
    let current = current_manager(state, &mut tx, &user.id, Lang::En).await?;
    let allowed = current_tools_in(state, &mut tx, &current).await?;
    tx.commit().await.map_err(internal)?;
    let candidates = personal_candidates(state, &current, &allowed).await?;
    let defaults = AgentSpec::from_value(&super::super::json_dots::default_personal_spec(&allowed))
        .map_err(internal)?;
    let abilities=candidates.abilities.iter().map(|ability| {
        let tool=supported_tool(state,&ability.id);
        json!({"id":ability.id,"capability_key":catalog::entry_key_for(&ability.id),"name":ability.name,"description":ability.description,"read_only":tool.as_ref().is_some_and(|tool|tool.read_only()),"default_permission":default_permission(&defaults,&ability.id,tool.as_deref())})
    }).collect::<Vec<_>>();
    Ok(
        json!({"abilities":abilities,"knowledge":candidates.knowledge.iter().map(|collection| json!({"id":collection.id,"name":collection.name,"description":collection.description})).collect::<Vec<_>>(),"identity":"user","model":"owner_chat_selection","supported_changes":SUPPORTED_CHANGES,"default_memory_tools":defaults.main.tools,"requires_manual_setup":["connector_accounts","action_rules","schedules","webhooks","dot_communication","strict_scope"]}),
    )
}

fn valid_name(name: &str) -> bool {
    !name.trim().is_empty()
        && name.trim().chars().count() <= 80
        && !name.chars().any(char::is_control)
}

pub(super) async fn propose(
    state: &RamaState,
    user: &User,
    session_id: &str,
    source_turn_id: &str,
    args: ProposalArgs,
) -> Result<Value, Response> {
    if !valid_name(&args.name)
        || args.description.len() > 16384
        || serde_json::to_string(&args.changes)
            .map_err(internal)?
            .len()
            > 128 * 1024
        || args.unresolved.len() > 20
        || args.unresolved.iter().any(|value| value.len() > 2000)
    {
        return Err(super::super::bad_request(t(
            Lang::En,
            "dot-error-name-invalid",
        )));
    }
    let display = args
        .display
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(args.name.trim());
    if !valid_name(display) {
        return Err(super::super::bad_request(t(
            Lang::En,
            "dot-error-name-invalid",
        )));
    }
    let mut tx = WriteTx::begin(&state.db).await.map_err(internal)?;
    let session = owned_session(state, &mut tx, &user.id, session_id, Lang::En).await?;
    if session.agent_id.is_some() {
        return Err(conflict(Lang::En));
    }
    let current = current_manager(state, &mut tx, &user.id, Lang::En).await?;
    let allowed = current_tools_in(state, &mut tx, &current).await?;
    tx.commit().await.map_err(internal)?;
    let candidates = personal_candidates(state, &current, &allowed).await?;
    let mut changes = args.changes;
    let mut dropped = Vec::new();
    let mut unresolved = args.unresolved;
    for key in changes.keys().cloned().collect::<Vec<_>>() {
        if !SUPPORTED_CHANGES.contains(&key.as_str()) {
            changes.remove(&key);
            dropped.push(json!({"step":key,"reason":"dot-plan-unsupported".to_string()}));
            unresolved.push("dot-plan-unsupported".to_string());
        }
    }
    if changes
        .get("scope")
        .and_then(|scope| scope.get("strict"))
        .and_then(Value::as_bool)
        == Some(true)
    {
        changes.remove("scope");
        dropped
            .push(json!({"step":"scope","reason":"dot-plan-strict-scope-unavailable".to_string()}));
        unresolved.push("dot-plan-strict-scope-unavailable".to_string());
    }
    if changes
        .get("knowledge")
        .and_then(Value::as_array)
        .is_some_and(|knowledge| knowledge.len() > 1)
    {
        changes.remove("knowledge");
        dropped.push(json!({"step":"knowledge","reason":"dot-plan-one-knowledge".to_string()}));
        unresolved.push("dot-plan-one-knowledge".to_string());
    }
    let base = super::super::json_dots::default_personal_spec(&allowed);
    let grants = GrantSet::new(
        catalog::MEMORY_TOOL_IDS
            .iter()
            .filter(|id| allowed.iter().any(|allowed| allowed == **id))
            .map(|id| {
                (
                    aiplane_core::server::principal::GrantKind::Tool,
                    (*id).to_string(),
                )
            }),
    );
    let agents = HashMap::new();
    let live_specs = HashMap::new();
    let actions =
        aiplane_runtime::server::tools::registry::action_catalogues(state.tools().as_ref());
    let context = ReviewContext {
        agent_id: "",
        grants: &grants,
        agents: &agents,
        live_specs: &live_specs,
        candidates: &candidates,
        action_catalogues: &actions,
        allow_private: state.config().network.allow_private_networks,
    };
    let applied = apply_changes(&Value::Object(changes), &base, &context);
    for entry in applied.suggestion.dropped {
        unresolved.push(entry.reason.clone());
        dropped.push(json!(entry));
    }
    let mut draft = applied.draft;
    let typed = AgentSpec::from_value(&draft).map_err(internal)?;
    let defaults = AgentSpec::from_value(&base).map_err(internal)?;
    if typed
        .main
        .instructions
        .orchestration
        .as_deref()
        .is_none_or(|task| task.trim().is_empty())
    {
        unresolved.push("dot-plan-task-required".to_string());
    }
    let ids = typed.main.tools.clone();
    let entries = super::super::tool_toggles::entries_for_tools(state, &ids, Lang::En);
    let mut tools = Vec::new();
    if typed.main.tool_resources.is_empty() && !ids.is_empty() {
        draft["main"]["tool_resources"] = json!({});
    }
    for id in ids {
        if !allowed.contains(&id) {
            unresolved.push("dot-access-error-unavailable".to_string());
        }
        let tool = supported_tool(state, &id);
        if tool.is_none() {
            unresolved.push("dot-plan-connection-required".to_string());
        }
        let permission = default_permission(&defaults, &id, tool.as_deref());
        if !typed.main.tool_resources.contains_key(&id) {
            draft["main"]["tool_resources"][&id] = json!({});
        }
        draft["main"]["tool_resources"][&id]["permission"] = json!(permission);
        let entry = entries
            .iter()
            .find(|entry| entry.key == catalog::entry_key_for(&id));
        tools.push(DotPlanTool {
            capability_key: catalog::entry_key_for(&id).to_string(),
            id,
            label: entry.map(|entry| entry.title.clone()),
            description: entry.and_then(|entry| {
                (!entry.description.is_empty()).then(|| entry.description.clone())
            }),
            permission: permission.into(),
        });
    }
    AgentSpec::from_value(&draft).map_err(internal)?;
    let knowledge = typed
        .main
        .tool_resources
        .get("rag_search")
        .and_then(|resource| resource.bind.get("collection"))
        .and_then(|binding| match binding {
            aiplane_runtime::agents::bind::BindSource::Const(value) => value.as_str(),
            _ => None,
        })
        .and_then(|name| {
            candidates
                .knowledge
                .iter()
                .find(|collection| collection.name == name)
        })
        .map(|collection| DotPlanKnowledge {
            id: collection.id.clone(),
            name: collection.name.clone(),
            description: collection.description.clone(),
        });
    if knowledge.is_some() && state.indexer.is_none() {
        unresolved.push("dot-setup-error-unavailable".into());
    }
    let plan = DotPlan {
        revision: uuid::Uuid::new_v4().to_string(),
        source_turn_id: source_turn_id.into(),
        name: args.name.trim().into(),
        display: display.into(),
        description: args.description.trim().into(),
        draft_spec: draft,
        tools,
        knowledge,
        dropped,
        unresolved,
    };
    let mut tx = WriteTx::begin(&state.db).await.map_err(internal)?;
    owned_session(state, &mut tx, &user.id, session_id, Lang::En).await?;
    if !architect_sessions::plan_is_current_in(&mut tx, session_id, source_turn_id)
        .await
        .map_err(internal)?
    {
        return Err(conflict(Lang::En));
    }
    if !architect_sessions::set_personal_plan_in(
        &mut tx,
        session_id,
        &user.id,
        &serde_json::to_string(&plan).map_err(internal)?,
    )
    .await
    .map_err(internal)?
    {
        return Err(conflict(Lang::En));
    }
    tx.commit().await.map_err(internal)?;
    Ok(json!(localized_plan(plan, Lang::En)))
}

async fn knowledge_available_in(
    tx: &mut WriteTx,
    policy: &aiplane_core::server::rbac::Resolver,
    groups: &[String],
    knowledge: &DotPlanKnowledge,
) -> Result<bool, Response> {
    let collection_id = knowledge.id.parse::<i64>().map_err(internal)?;
    Ok(
        aiplane_core::server::db::rag::find_collection_by_id_in(tx, collection_id)
            .await
            .map_err(internal)?
            .is_some_and(|collection| {
                collection.name == knowledge.name
                    && policy.resource_allowed(groups, &collection.allowed_groups)
            }),
    )
}

pub async fn get_plan(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_, user) = require_session_json!(state, req);
    let lang = Lang::from_request(req.headers());
    let Some(id) = raw_path_segment(&req, 1) else {
        return not_found(t(lang, "dot-error-not-found"));
    };
    async {
        let mut tx = WriteTx::begin(&state.db).await.map_err(internal)?;
        let session = owned_session(&state, &mut tx, &user.id, &id, lang).await?;
        let proposal = session
            .dot_plan
            .as_deref()
            .map(serde_json::from_str::<DotPlan>)
            .transpose()
            .map_err(internal)?;
        let proposal = match proposal {
            Some(plan)
                if session.agent_id.is_some()
                    || architect_sessions::plan_is_current_in(
                        &mut tx,
                        &id,
                        &plan.source_turn_id,
                    )
                    .await
                    .map_err(internal)? =>
            {
                Some(localized_plan(plan, lang))
            }
            _ => None,
        };
        let proposal = proposal.map(|mut plan| {
            let ids = plan
                .tools
                .iter()
                .map(|tool| tool.id.clone())
                .collect::<Vec<_>>();
            let entries = super::super::tool_toggles::entries_for_tools(&state, &ids, lang);
            for tool in &mut plan.tools {
                if let Some(entry) = entries
                    .iter()
                    .find(|entry| entry.key == catalog::entry_key_for(&tool.id))
                {
                    tool.label = Some(entry.title.clone());
                    tool.description =
                        (!entry.description.is_empty()).then(|| entry.description.clone());
                }
            }
            plan
        });
        tx.commit().await.map_err(internal)?;
        Ok::<_, Response>(json_ok(
            StatusCode::OK,
            DotPlanReply {
                proposal,
                dot_id: session.agent_id,
            },
        ))
    }
    .await
    .unwrap_or_else(|response| response)
}

pub async fn apply_plan(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_, user) = require_session_json!(state, req);
    let lang = Lang::from_request(req.headers());
    let Some(id) = raw_path_segment(&req, 1) else {
        return not_found(t(lang, "dot-error-not-found"));
    };
    let body: ApplyDotPlanBody =
        or_return!(super::super::read_json(req.into_body(), "the reviewed agent plan").await);
    async {
        let mut tx = WriteTx::begin(&state.db).await.map_err(internal)?;
        let session = owned_session(&state, &mut tx, &user.id, &id, lang).await?;
        let plan: DotPlan = session
            .dot_plan
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(internal)?
            .ok_or_else(|| conflict(lang))?;
        if plan.revision != body.revision {
            return Err(conflict(lang));
        }
        if let Some(dot_id) = session.agent_id {
            tx.commit().await.map_err(internal)?;
            let dot = agents::get_personal_dot(&state.db, &user.id, &dot_id)
                .await
                .map_err(internal)?
                .ok_or_else(|| not_found(t(lang, "dot-error-not-found")))?;
            return Ok(json_ok(
                StatusCode::OK,
                AppliedDotPlan {
                    dot: super::super::json_dots::view(&dot),
                    session_id: id,
                },
            ));
        }
        if !architect_sessions::plan_is_current_in(&mut tx, &id, &plan.source_turn_id)
            .await
            .map_err(internal)?
        {
            return Err(conflict(lang));
        }
        if !plan.unresolved.is_empty() {
            return Err(json_error(
                StatusCode::CONFLICT,
                "dot_plan_unresolved",
                &t(lang, "dot-setup-error-incomplete"),
            ));
        }
        let busy = architect_sessions::busy_in(&mut tx, &id)
            .await
            .map_err(internal)?;
        if busy || state.chats.get(&user.id, &id).is_some() {
            return Err(json_error(
                StatusCode::CONFLICT,
                "dot_plan_busy",
                &t(lang, "dot-setup-error-running"),
            ));
        }
        let current = current_manager(&state, &mut tx, &user.id, lang).await?;
        let allowed = current_tools_in(&state, &mut tx, &current).await?;
        let typed = AgentSpec::from_value(&plan.draft_spec).map_err(internal)?;
        if typed.main.tools.iter().any(|tool| !allowed.contains(tool)) {
            return Err(json_error(
                StatusCode::CONFLICT,
                "dot_plan_resources_changed",
                &t(lang, "dot-access-error-unavailable"),
            ));
        }
        if plan.tools.iter().any(|selected| {
            supported_tool(&state, &selected.id).is_none_or(|tool| {
                selected.permission == "always_allow"
                    && !tool.read_only()
                    && !catalog::MEMORY_TOOL_IDS.contains(&selected.id.as_str())
            })
        }) {
            return Err(json_error(
                StatusCode::CONFLICT,
                "dot_plan_resources_changed",
                &t(lang, "dot-setup-error-unavailable"),
            ));
        }
        if let Some(knowledge) = &plan.knowledge {
            let (_, policy) = aiplane_runtime::agents::access::current_user_policy_in(
                &state,
                &mut tx,
                &current.id,
            )
            .await
            .map_err(internal)?
            .ok_or_else(|| conflict(lang))?;
            let groups = policy.role_ids_for(&current.roles);
            let available = state.indexer.is_some()
                && knowledge_available_in(&mut tx, &policy, &groups, knowledge).await?;
            if !available {
                return Err(json_error(
                    StatusCode::CONFLICT,
                    "dot_plan_resources_changed",
                    &t(lang, "dot-setup-error-unavailable"),
                ));
            }
        }
        let (outcome, agent) = agents::create_personal_dot_in(
            &mut tx,
            &current.id,
            &NewPrincipal {
                name: &plan.name,
                display: &plan.display,
                description: &plan.description,
            },
            &plan.draft_spec.to_string(),
            state.config().dots.max_active_per_user,
        )
        .await
        .map_err(internal)?;
        let agent = match (outcome, agent) {
            (agents::DotCreate::Created, Some(agent)) => agent,
            (agents::DotCreate::NameTaken, _) => {
                return Err(json_error(
                    StatusCode::CONFLICT,
                    "conflict",
                    &t(lang, "dot-error-name-taken"),
                ));
            }
            (agents::DotCreate::ActiveLimit, _) => {
                return Err(json_error(
                    StatusCode::CONFLICT,
                    "active_dot_limit",
                    &t(lang, "dot-error-active-limit"),
                ));
            }
            _ => return Err(internal("created agent could not be loaded")),
        };
        architect_sessions::set_agent_in(&mut tx, &id, &agent.principal.id)
            .await
            .map_err(internal)?;
        tx.commit().await.map_err(internal)?;
        let dot = agents::get_personal_dot(&state.db, &current.id, &agent.principal.id)
            .await
            .map_err(internal)?
            .ok_or_else(|| internal("created agent could not be loaded"))?;
        Ok(json_ok(
            StatusCode::CREATED,
            AppliedDotPlan {
                dot: super::super::json_dots::view(&dot),
                session_id: id,
            },
        ))
    }
    .await
    .unwrap_or_else(|response| response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actual_read_only_and_existing_memory_defaults_determine_proposal_permissions() {
        use aiplane_runtime::server::tools::{echo::Echo, time::CurrentTimestamp};
        let defaults =
            AgentSpec::from_value(&super::super::super::json_dots::default_personal_spec(&[
                "remember".into(),
            ]))
            .unwrap();
        assert_eq!(
            default_permission(&defaults, "get_current_timestamp", Some(&CurrentTimestamp)),
            "always_allow"
        );
        assert_eq!(
            default_permission(&defaults, "remember", Some(&Echo)),
            "always_allow"
        );
        assert_eq!(
            default_permission(&defaults, "company_echo", Some(&Echo)),
            "always_ask"
        );
        assert_eq!(default_permission(&defaults, "missing", None), "always_ask");
        assert_eq!(
            default_permission(&defaults, "forward_request", Some(&Echo)),
            "always_allow",
            "consent of both agents is the permission to delegate"
        );
        let disabled_memory =
            AgentSpec::from_value(&super::super::super::json_dots::default_personal_spec(&[]))
                .unwrap();
        assert_eq!(
            default_permission(&disabled_memory, "remember", Some(&Echo)),
            "always_ask"
        );
    }

    #[test]
    fn encoded_changes_are_rejected_as_an_object_type_error() {
        let result = serde_json::from_value::<ProposalArgs>(
            json!({"name":"Clock","changes":"{\"task\":\"Report the time\"}"}),
        );
        let error = result
            .err()
            .expect("encoded changes must not be accepted")
            .to_string();
        assert!(error.contains("expected a map"), "{error}");
        assert!(!error.to_lowercase().contains("approval"), "{error}");
    }

    #[test]
    fn personal_schema_offers_only_the_supported_changes_as_an_object() {
        let schema = personal_changes_schema();
        assert_eq!(schema["type"], "object");
        let properties = schema["properties"].as_object().unwrap();
        assert_eq!(properties.len(), 5);
        for key in ["task", "tone", "scope", "abilities", "knowledge"] {
            assert!(properties.contains_key(key));
        }
        for key in [
            "slots",
            "model",
            "handoffs",
            "display",
            "fallback_to_person",
        ] {
            assert!(!properties.contains_key(key));
        }
    }

    #[tokio::test]
    async fn reviewed_knowledge_rejects_revoked_access_and_replacement_with_the_same_name() {
        use aiplane_core::server::db::{self, rag};
        let pool = db::open(std::path::Path::new(":memory:")).await.unwrap();
        let new = rag::NewCollection {
            name: "Storage manual".into(),
            description: Some("Storage administration".into()),
            git_url: String::new(),
            git_ref: "main".into(),
            pat: None,
            source: Default::default(),
            profile_id: None,
            extraction_model: None,
            embedding_model: "embed-test".into(),
            include_globs: Vec::new(),
            exclude_globs: Vec::new(),
            chunk_size: 400,
            chunk_overlap: 40,
            search_mode: rag::SearchMode::Versioned,
            refresh_interval_mins: 0,
        };
        let original = rag::create_collection(&pool, &new).await.unwrap();
        let selected = DotPlanKnowledge {
            id: original.id.to_string(),
            name: original.name.clone(),
            description: original.description,
        };
        let policy = aiplane_core::server::rbac::Resolver::empty();
        let mut tx = WriteTx::begin(&pool).await.unwrap();
        assert!(
            knowledge_available_in(&mut tx, &policy, &[], &selected)
                .await
                .unwrap()
        );
        tx.commit().await.unwrap();
        rag::set_allowed_groups(&pool, original.id, &["restricted".into()])
            .await
            .unwrap();
        let mut tx = WriteTx::begin(&pool).await.unwrap();
        assert!(
            !knowledge_available_in(&mut tx, &policy, &[], &selected)
                .await
                .unwrap()
        );
        tx.commit().await.unwrap();
        rag::delete_collection(&pool, original.id).await.unwrap();
        let replacement = rag::create_collection(&pool, &new).await.unwrap();
        assert_ne!(replacement.id, original.id);
        let mut tx = WriteTx::begin(&pool).await.unwrap();
        assert!(
            !knowledge_available_in(&mut tx, &policy, &[], &selected)
                .await
                .unwrap()
        );
        tx.commit().await.unwrap();
    }

    #[test]
    fn personal_plan_names_reject_empty_long_and_control_values() {
        assert!(valid_name("A named agent"));
        assert!(!valid_name("  "));
        assert!(!valid_name(&"x".repeat(81)));
        assert!(!valid_name("Clock\n"));
    }

    #[test]
    fn own_blocker_keys_are_localized_without_changing_model_written_requirements() {
        let plan = DotPlan {
            revision: "one".into(),
            source_turn_id: "turn-one".into(),
            name: "Clock".into(),
            display: "Clock".into(),
            description: String::new(),
            draft_spec: json!({}),
            tools: vec![],
            knowledge: None,
            dropped: vec![json!({"step":"scope","reason":"dot-plan-strict-scope-unavailable"})],
            unresolved: vec![
                "dot-plan-task-required".into(),
                "Ask which account the owner wants.".into(),
            ],
        };
        let rendered = localized_plan(plan, Lang::De);
        assert_eq!(
            rendered.unresolved[0],
            t(Lang::De, "dot-plan-task-required")
        );
        assert_eq!(rendered.unresolved[1], "Ask which account the owner wants.");
        assert_eq!(
            rendered.dropped[0]["reason"],
            t(Lang::De, "dot-plan-strict-scope-unavailable")
        );
    }
}
