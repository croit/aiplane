// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The architect's tools. Each one acts as the person through the function
//! the matching route uses, so the share, the validator and the grant cap
//! decide exactly as they would for a click in the setup UI; a refusal comes
//! back to the model (and shows in the chat) as the route's own message.

use std::sync::Arc;
use std::time::Duration;

use rama::http::Response;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use shared::api::ToolDef;

use super::super::json_agent_assist::{SuggestBody, candidates, suggest_for};
use super::super::json_agent_resources::{grantable_chat_models, resources_for};
use super::super::json_agent_test::draft_test_turn;
use super::super::json_agents::{
    CreateBody, SpecWorld, agent_by_id, agent_json, create_agent, parse_spec, publish_issues,
    save_draft, visible_agents,
};
use super::super::json_principals::add_capped_grant;
use aiplane_agents::db::agents::{Access, AgentRow, DraftChange};
use aiplane_agents::db::architect_sessions;
use aiplane_core::server::db::users::User;
use aiplane_core::server::feature_defaults::Feature;
use aiplane_core::server::principal::GrantKind;
use aiplane_runtime::agents::assist::{ReviewContext, apply_changes, changes_schema};
use aiplane_runtime::rama_server::state::RamaState;
use aiplane_runtime::server::model_choices::gateway_default;
use aiplane_runtime::server::tools::{Tool, ToolContext, ToolError, ToolFuture};
use session_core::i18n::Lang;

/// The largest refusal body read back into a tool error.
const MAX_REFUSAL_BYTES: usize = 64 * 1024;

/// Who the tools act for, in which conversation.
pub(super) struct Ctx {
    pub state: Arc<RamaState>,
    pub user: User,
    pub session_id: String,
    pub personal: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    ListAgents,
    ReadAgent,
    ListGrantable,
    ProposeSetup,
    CreateAgentDraft,
    UpdateAgentDraft,
    RunTestTurn,
    ProposeAgentSetup,
}

impl Kind {
    fn id(self) -> &'static str {
        match self {
            Kind::ProposeAgentSetup => "propose_agent_setup",
            Kind::ListAgents => "list_agents",
            Kind::ReadAgent => "read_agent",
            Kind::ListGrantable => "list_grantable",
            Kind::ProposeSetup => "propose_setup",
            Kind::CreateAgentDraft => "create_agent_draft",
            Kind::UpdateAgentDraft => "update_agent_draft",
            Kind::RunTestTurn => "run_test_turn",
        }
    }
}

/// Every tool the architect has. Publishing is deliberately not one.
pub(super) const PERSONAL_KINDS: &[Kind] = &[Kind::ListGrantable, Kind::ProposeAgentSetup];

pub(super) const KINDS: &[Kind] = &[
    Kind::ListAgents,
    Kind::ReadAgent,
    Kind::ListGrantable,
    Kind::ProposeSetup,
    Kind::CreateAgentDraft,
    Kind::UpdateAgentDraft,
    Kind::RunTestTurn,
];

pub(super) struct ArchitectTool {
    kind: Kind,
    ctx: Arc<Ctx>,
}

impl ArchitectTool {
    pub(super) fn new(kind: Kind, ctx: Arc<Ctx>) -> Self {
        Self { kind, ctx }
    }
}

fn agent_id_param() -> Value {
    json!({ "type": "string", "description": "the agent's id or name, as list_agents names it" })
}

fn setup_url(agent_id: &str) -> String {
    format!("/agents/{agent_id}")
}

impl Tool for ArchitectTool {
    fn id(&self) -> &str {
        self.kind.id()
    }

    fn schema(&self) -> ToolDef {
        let (description, parameters) = match self.kind {
            Kind::ProposeAgentSetup => (
                "Store a complete reviewed personal agent proposal, without creating an agent. Only task, tone, non-strict scope, abilities and one knowledge collection are supported. The changes field must be an actual JSON object, never a string containing JSON. Every requested prerequisite that cannot be configured must be listed in unresolved. Replaces the previous proposal.",
                json!({"type":"object","additionalProperties":false,"required":["name","changes"],"properties":{"name":{"type":"string"},"display":{"type":"string"},"description":{"type":"string"},"changes":super::owned_agent::personal_changes_schema(),"unresolved":{"type":"array","items":{"type":"string"}}}}),
            ),
            Kind::ListGrantable if self.ctx.personal => (
                "Inspect the personal agent's actual resources. Each ability reports read_only and default_permission: known read-only tools allow, external or unknown writes ask, permitted default_memory_tools allow only agent-scoped memory writes. Use these existing defaults; valid defaults do not need an unresolved approval-rule prerequisite. This is resource information, not approval of a tool call.",
                json!({"type":"object","properties":{},"additionalProperties":false}),
            ),
            Kind::ListAgents => (
                "List the agents shared with the person: id, name, whether one is live.",
                json!({ "type": "object", "properties": {}, "additionalProperties": false }),
            ),
            Kind::ReadAgent => (
                "Read one agent: its draft, its grants and what still blocks publishing it.",
                json!({ "type": "object", "additionalProperties": false,
                        "required": ["agent_id"],
                        "properties": { "agent_id": agent_id_param() } }),
            ),
            Kind::ListGrantable => (
                "What the person may give an agent: models by kind (`models.chat`, \
                 `models.transcription`, `models.speech`), the gateway's default model of each \
                 kind (`defaults`, what an agent runs on when its draft names none), and \
                 `items`: the tools, connectors, skills and knowledge collections, each with its \
                 own `title` and `description`, the `grant` it takes (`kind` and `refs`) and the \
                 tool ids it adds to the draft (`tools`).",
                json!({ "type": "object", "properties": {}, "additionalProperties": false }),
            ),
            Kind::ProposeSetup => (
                "Ask for a complete proposed setup of an existing agent from a scenario in the \
                 person's words: every step plus test conversations, each checked against the \
                 draft. Writes nothing; apply what the person agrees to with \
                 update_agent_draft.",
                json!({ "type": "object", "additionalProperties": false,
                        "required": ["agent_id", "scenario"],
                        "properties": {
                            "agent_id": agent_id_param(),
                            "scenario": { "type": "string" },
                            "template": { "type": "string" } } }),
            ),
            Kind::CreateAgentDraft => (
                "Create a new agent with an empty draft. Not published, so no visitor sees it.",
                json!({ "type": "object", "additionalProperties": false,
                        "required": ["display"],
                        "properties": {
                            "display": { "type": "string",
                                         "description": "the agent's name as visitors see it" },
                            "id": { "type": "string",
                                    "description": "lowercase letters, digits and `-`; \
                                                    derived from the name when left out" },
                            "description": { "type": "string" } } }),
            ),
            Kind::UpdateAgentDraft => (
                "Change an agent's draft, only the steps given in `changes`: name (`display`), \
                 model (`model`, one of `models.chat`), task, tone, scope, abilities (granted to the agent), slots \
                 (information to collect), handoffs (topic → agent or person) and \
                 fallback_to_person. Each piece is checked and kept or \
                 dropped with a reason; the previous draft is kept so the person can undo.",
                json!({ "type": "object", "additionalProperties": false,
                        "required": ["agent_id", "changes"],
                        "properties": {
                            "agent_id": agent_id_param(),
                            "changes": changes_schema() } }),
            ),
            Kind::RunTestTurn => (
                "Send one test message to the agent's draft and get its answer and what it \
                 collected. Pass `conversation_id` from an earlier answer to continue.",
                json!({ "type": "object", "additionalProperties": false,
                        "required": ["agent_id", "message"],
                        "properties": {
                            "agent_id": agent_id_param(),
                            "message": { "type": "string" },
                            "conversation_id": { "type": "string" } } }),
            ),
        };
        ToolDef::function(self.id(), description, parameters)
    }

    fn max_duration(&self) -> Option<Duration> {
        match self.kind {
            Kind::ProposeSetup | Kind::RunTestTurn => Some(Duration::from_secs(180)),
            _ => None,
        }
    }

    fn run<'a>(&'a self, tool_ctx: ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let ctx = &self.ctx;
            let role_ids = ctx.state.rbac.role_ids_for(&ctx.user.roles);
            if !ctx.personal && !ctx.state.rbac.can_manage_agents(&role_ids) {
                return Err(ToolError::Failed(
                    "the person no longer holds the agent-management permission, so the \
                     architect cannot act for them — an admin can enable `can_manage_agents` \
                     on one of their groups"
                        .into(),
                ));
            }
            if ctx.personal && !PERSONAL_KINDS.contains(&self.kind) {
                return Err(ToolError::Failed(
                    "this tool is unavailable in personal agent planning".into(),
                ));
            }
            let out = match self.kind {
                Kind::ProposeAgentSetup => super::owned_agent::propose(
                    &ctx.state,
                    &ctx.user,
                    &ctx.session_id,
                    tool_ctx.assistant_turn_id.as_deref().ok_or_else(|| {
                        ToolError::Failed(
                            "personal planning needs its current conversation turn".into(),
                        )
                    })?,
                    parse(args)?,
                )
                .await
                .map_err(Refusal::from),
                Kind::ListGrantable if ctx.personal => {
                    super::owned_agent::resources(&ctx.state, &ctx.user)
                        .await
                        .map_err(Refusal::from)
                }
                Kind::ListAgents => list_agents(ctx).await,
                Kind::ReadAgent => read_agent(ctx, parse(args)?).await,
                Kind::ListGrantable => resources_for(&ctx.state, &ctx.user, Lang::En)
                    .await
                    .map(|resources| json!(resources))
                    .map_err(Refusal::from),
                Kind::ProposeSetup => propose(ctx, parse(args)?).await,
                Kind::CreateAgentDraft => create(ctx, parse(args)?).await,
                Kind::UpdateAgentDraft => update(ctx, parse(args)?).await,
                Kind::RunTestTurn => test_turn(ctx, parse(args)?).await,
            };
            match out {
                Ok(value) => Ok(value),
                Err(Refusal::Response(resp)) => Err(refusal(resp).await),
                Err(Refusal::Said(message)) => Err(ToolError::Failed(message)),
            }
        })
    }
}

fn parse<T: DeserializeOwned>(args: Value) -> Result<T, ToolError> {
    serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs(e.to_string()))
}

/// Why a tool did not do what it was asked.
enum Refusal {
    /// The route's own answer.
    Response(Response),
    /// The tool's own words.
    Said(String),
}

impl From<Response> for Refusal {
    fn from(resp: Response) -> Self {
        Self::Response(resp)
    }
}

/// A route's refusal as the model reads it: its message and code.
async fn refusal(resp: Response) -> ToolError {
    let status = resp.status();
    let body: Value =
        super::super::read_json_capped(resp.into_body(), "a refusal", MAX_REFUSAL_BYTES)
            .await
            .unwrap_or(Value::Null);
    let error = &body["error"];
    let message = error["message"]
        .as_str()
        .unwrap_or("the request was refused");
    match error["code"].as_str() {
        Some(code) => ToolError::Failed(format!("{message} ({code}, HTTP {})", status.as_u16())),
        None => ToolError::Failed(format!("{message} (HTTP {})", status.as_u16())),
    }
}

async fn list_agents(ctx: &Ctx) -> Result<Value, Refusal> {
    let visible = visible_agents(&ctx.state, &ctx.user).await?;
    let mut agents = Vec::with_capacity(visible.len());
    for (agent, access) in visible {
        let mut view = agent_json(&ctx.state, &agent, access).await?;
        view["setup_url"] = json!(setup_url(&agent.principal.id));
        agents.push(view);
    }
    Ok(json!({ "agents": agents }))
}

/// The agent `key` names — its id, or its name, which a model remembers
/// better — if the person holds `need` on it.
async fn agent(ctx: &Ctx, key: &str, need: Access) -> Result<(AgentRow, Access), Refusal> {
    let key = key.trim();
    let id = visible_agents(&ctx.state, &ctx.user)
        .await?
        .into_iter()
        .find(|(a, _)| a.principal.name == key)
        .map_or_else(|| key.to_string(), |(a, _)| a.principal.id);
    Ok(agent_by_id(&ctx.state, &ctx.user, &id, need).await?)
}

#[derive(Deserialize)]
struct AgentArgs {
    agent_id: String,
}

async fn read_agent(ctx: &Ctx, args: AgentArgs) -> Result<Value, Refusal> {
    let (agent, access) = agent(ctx, &args.agent_id, Access::Read).await?;
    let id = &agent.principal.id;
    let world = SpecWorld::load(&ctx.state, id).await?;
    let draft = parse_spec(&agent.draft_spec);
    let issues = publish_issues(&ctx.state, id, &draft).await?;
    let mut v = agent_json(&ctx.state, &agent, access).await?;
    v["draft_spec"] = draft;
    v["grants"] = world
        .grants
        .iter()
        .map(|(kind, reference)| json!({ "kind": kind.as_str(), "ref": reference }))
        .collect();
    v["publish_issues"] = json!(issues);
    v["setup_url"] = json!(setup_url(id));
    Ok(v)
}

#[derive(Deserialize)]
struct ProposeArgs {
    agent_id: String,
    scenario: String,
    #[serde(default)]
    template: Option<String>,
}

async fn propose(ctx: &Ctx, args: ProposeArgs) -> Result<Value, Refusal> {
    let (agent, _) = agent(ctx, &args.agent_id, Access::Write).await?;
    let body = SuggestBody {
        scenario: args.scenario,
        template: args.template,
        current_draft: None,
        model: None,
    };
    Ok(json!(
        suggest_for(&ctx.state, &ctx.user, &agent, body).await?
    ))
}

#[derive(Deserialize)]
struct CreateArgs {
    display: String,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

async fn create(ctx: &Ctx, args: CreateArgs) -> Result<Value, Refusal> {
    let display = args.display.trim().to_string();
    let name = args
        .id
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| id_from_name(&display));
    let spec = json!({ "profile": { "display": display } });
    let agent = create_agent(
        &ctx.state,
        &ctx.user,
        CreateBody {
            kind: super::super::json_agents::CreateKind::Public,
            name: name.clone(),
            display: Some(display),
            description: args.description.unwrap_or_default(),
            spec: Some(spec),
        },
    )
    .await?;
    let id = agent.agent.id.clone();
    let model = grant_default_model(ctx, &id, &agent.agent.display).await?;
    let planned = architect_sessions::get(&ctx.state.db, &ctx.session_id)
        .await
        .ok()
        .flatten()
        .is_some_and(|s| s.agent_id.is_some());
    if !planned
        && let Err(err) = architect_sessions::set_agent(&ctx.state.db, &ctx.session_id, &id).await
    {
        tracing::warn!(error = %err, agent = %id, "pointing the architect conversation at its agent");
    }
    Ok(json!({
        "agent_id": id,
        "name": name,
        "display": agent.agent.display,
        "model": model,
        "setup_url": setup_url(&id),
    }))
}

/// Grant a new agent the model it starts on, as in the setup: the gateway's
/// default chat model, which its unset `main.model` runs on — through the
/// capped route, when the person may grant it. `None` when there is none;
/// the setup then asks for a model.
async fn grant_default_model(
    ctx: &Ctx,
    agent_id: &str,
    display: &str,
) -> Result<Option<String>, Refusal> {
    let Some(default) = gateway_default(&ctx.state, Feature::Chat).await else {
        return Ok(None);
    };
    if !grantable_chat_models(&ctx.state, &ctx.user)
        .await
        .contains(&default)
    {
        return Ok(None);
    }
    let (agent, _) = agent_by_id(&ctx.state, &ctx.user, agent_id, Access::Write).await?;
    add_capped_grant(
        &ctx.state,
        &ctx.user,
        agent_id,
        GrantKind::Model,
        &default,
        Lang::En,
    )
    .await?;
    let spec = json!({ "profile": { "display": display } });
    let granted = [(GrantKind::Model, default.clone())];
    let change = DraftChange {
        granted: &granted,
        ..DraftChange::default()
    };
    save_draft(&ctx.state, &ctx.user, &agent, spec, change, Lang::En).await?;
    Ok(Some(default))
}

/// An agent id from the name a person gives it: what the create dialog
/// derives (`agentIdFromName` in `web/src/lib/agents.ts`).
fn id_from_name(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        let folded = match c {
            'ä' => "ae",
            'ö' => "oe",
            'ü' => "ue",
            'ß' => "ss",
            'à' | 'á' | 'â' | 'ã' | 'å' => "a",
            'è' | 'é' | 'ê' | 'ë' => "e",
            'ì' | 'í' | 'î' | 'ï' => "i",
            'ò' | 'ó' | 'ô' | 'õ' => "o",
            'ù' | 'ú' | 'û' => "u",
            'ç' => "c",
            'ñ' => "n",
            c if c.is_ascii_alphanumeric() => {
                out.push(c);
                continue;
            }
            _ => "-",
        };
        out.push_str(folded);
    }
    let mut id = String::new();
    for c in out.chars() {
        if c == '-' && (id.is_empty() || id.ends_with('-')) {
            continue;
        }
        id.push(c);
    }
    let id: String = id.chars().take(48).collect();
    let id = id.trim_end_matches('-').to_string();
    if id.is_empty() { "agent".into() } else { id }
}

#[derive(Deserialize)]
struct UpdateArgs {
    agent_id: String,
    changes: Value,
}

async fn update(ctx: &Ctx, args: UpdateArgs) -> Result<Value, Refusal> {
    let (agent, _) = agent(ctx, &args.agent_id, Access::Write).await?;
    let id = &agent.principal.id;
    let world = SpecWorld::load(&ctx.state, id).await?;
    let candidates = candidates(&ctx.state, &ctx.user, id).await?;
    let review = ReviewContext {
        agent_id: id,
        grants: &world.grants,
        agents: &world.agents,
        live_specs: &world.live_specs,
        candidates: &candidates,
        action_catalogues: &world.action_catalogues,
        allow_private: world.allow_private,
    };
    let base = parse_spec(&agent.draft_spec);
    let applied = apply_changes(&args.changes, &base, &review);
    let dropped = json!(applied.suggestion.dropped);
    if applied.draft == base && applied.grants.is_empty() {
        let reasons: Vec<String> = applied
            .suggestion
            .dropped
            .iter()
            .map(|d| match &d.item {
                Some(item) => format!("{} `{item}`: {}", d.step, d.reason),
                None => format!("{}: {}", d.step, d.reason),
            })
            .collect();
        return Err(Refusal::Said(if reasons.is_empty() {
            "nothing to change — `changes` names no step, or every step already is as given".into()
        } else {
            format!("nothing was changed — {}", reasons.join("; "))
        }));
    }
    let mut made = Vec::new();
    for (kind, reference) in &applied.grants {
        if !world.grants.has(*kind, reference) {
            add_capped_grant(&ctx.state, &ctx.user, id, *kind, reference, Lang::En).await?;
            made.push((*kind, reference.clone()));
        }
    }
    let change = DraftChange {
        granted: &made,
        ..DraftChange::default()
    };
    let saved = save_draft(
        &ctx.state,
        &ctx.user,
        &agent,
        applied.draft,
        change,
        Lang::En,
    )
    .await?;
    let granted: Vec<Value> = made
        .iter()
        .map(|(kind, reference)| json!({ "kind": kind.as_str(), "ref": reference }))
        .collect();
    let mut changed: Vec<&str> = Vec::new();
    if applied.display.is_some() {
        changed.push("display");
    }
    if applied.model.is_some() {
        changed.push("model");
    }
    changed.extend(applied.suggestion.steps.offered());
    if applied.handoffs {
        changed.push("handoffs");
    }
    Ok(json!({
        "agent_id": id,
        "revision": saved.revision,
        "changed": changed,
        "granted": granted,
        "dropped": dropped,
        "setup_url": setup_url(id),
    }))
}

#[derive(Deserialize)]
struct TestArgs {
    agent_id: String,
    message: String,
    #[serde(default)]
    conversation_id: Option<String>,
}

async fn test_turn(ctx: &Ctx, args: TestArgs) -> Result<Value, Refusal> {
    let (agent, _) = agent(ctx, &args.agent_id, Access::Write).await?;
    let out = draft_test_turn(
        &ctx.state,
        &agent,
        &args.message,
        args.conversation_id.as_deref(),
    )
    .await?;
    Ok(json!({
        "conversation_id": out["session_id"],
        "status": out["status"],
        "answer": out["answer"],
        "error": out["error"],
        "debug": out["debug"],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_agent_id_is_derived_from_its_name_like_the_create_dialog_does() {
        assert_eq!(id_from_name("Harald"), "harald");
        assert_eq!(
            id_from_name("  Kunden-Support für Ceph! "),
            "kunden-support-fuer-ceph"
        );
        assert_eq!(id_from_name("Café Größe"), "cafe-groesse");
        assert_eq!(id_from_name("!!!"), "agent");
        assert_eq!(id_from_name(&"x ".repeat(60)).len(), 47);
    }

    #[test]
    fn the_architect_has_no_publish_tool() {
        let names: Vec<String> = KINDS.iter().map(|k| k.id().to_string()).collect();
        assert!(names.iter().all(|n| !n.contains("publish")), "{names:?}");
        assert_eq!(KINDS.len(), 7);
    }
}
