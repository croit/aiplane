// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! `/api/v0/agents/{id}/assist/*` — the prompt assistant
//! (`docs/agent-builder.md` → "Prompt assistant"): a proposed setup for every step,
//! and an improved text. Both need a `write` share, since they are for
//! editing the agent and spend the manager's usage; neither writes to the
//! agent. The work is `aiplane_runtime::agents::assist`; this resolves who
//! asks, what they may offer the agent, and what the draft is checked
//! against.

use std::sync::Arc;

use rama::http::header::{self, HeaderValue};
use rama::http::service::web::extract::State;
use rama::http::{Request, Response, StatusCode};
use serde::Deserialize;
use serde_json::Value;

use super::json_agent_resources::{grantable_chat_models, grantable_collections, grantable_tools};
use super::json_agents::{SpecWorld, agent_at, parse_spec, visible_agents};
use super::json_principals::require_agent_manager;
use super::{internal, json_error, json_ok};
use aiplane_agents::db::agents::{Access, AgentRow};
use aiplane_core::server::db::users::User;
use aiplane_runtime::agents::assist::{
    Asker, AssistError, Candidates, ImproveField, Knowledge, ReviewContext, SuggestRequest,
    Suggested, Target,
};
use aiplane_runtime::rama_server::state::RamaState;

/// A request body: a scenario, a draft and a little JSON around them.
const MAX_BODY_BYTES: usize = 512 * 1024;

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SuggestBody {
    pub scenario: String,
    #[serde(default)]
    pub template: Option<String>,
    /// The draft as the manager is editing it; the stored draft when
    /// absent.
    #[serde(default)]
    pub current_draft: Option<Value>,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImproveBody {
    pub field: ImproveField,
    pub text: String,
    #[serde(default)]
    pub model: Option<String>,
}

fn refused(err: AssistError) -> Response {
    let (status, code) = match &err {
        AssistError::Input(_) => (StatusCode::BAD_REQUEST, "invalid_assist_input"),
        AssistError::ModelNotAllowed(_) => (StatusCode::FORBIDDEN, "assist_model_not_allowed"),
        AssistError::NoModel => (StatusCode::SERVICE_UNAVAILABLE, "assist_no_model"),
        AssistError::RateLimited(_) => (StatusCode::TOO_MANY_REQUESTS, "assist_rate_limited"),
        AssistError::OverBudget(_) => (StatusCode::TOO_MANY_REQUESTS, "rate_limit_exceeded"),
        AssistError::Model(_) => (StatusCode::BAD_GATEWAY, "assist_model_failed"),
        AssistError::Log(_) => (StatusCode::SERVICE_UNAVAILABLE, "activity_log_unavailable"),
        AssistError::Db(e) => return internal(e),
    };
    let mut resp = json_error(status, code, &err.to_string());
    if let Some(secs) = err.retry_after_secs() {
        resp.headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from(secs.max(1)));
    }
    resp
}

/// What `user` may offer agent `agent_id`: the tools and knowledge bases
/// they may grant, the agents shared with them, and the chat models they may
/// grant.
pub(super) async fn candidates(
    state: &RamaState,
    user: &User,
    agent_id: &str,
) -> Result<Candidates, Response> {
    let role_ids = state.rbac.role_ids_for(&user.roles);
    let agents = visible_agents(state, user)
        .await?
        .into_iter()
        .filter(|(a, _): &(AgentRow, Access)| a.principal.id != agent_id)
        .map(|(a, _)| Target {
            id: a.principal.id,
            name: a.principal.display,
        })
        .collect();
    Ok(Candidates {
        abilities: grantable_tools(state, &role_ids),
        knowledge: grantable_collections(state, &role_ids)
            .await?
            .into_iter()
            .map(|c| Knowledge {
                id: c.id.to_string(),
                name: c.name,
                description: c.description,
            })
            .collect(),
        agents,
        models: grantable_chat_models(state, user).await,
    })
}

/// POST /api/v0/agents/{id}/assist/suggest — `{scenario, template?,
/// current_draft?, model?}`: a proposal per setup step, each piece checked
/// against the draft, and what was dropped and why.
pub async fn suggest(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let user = or_return!(require_agent_manager(&state, &req).await);
    let (agent, _) = or_return!(agent_at(&state, &req, &user, 2, Access::Write).await);
    let body: SuggestBody = or_return!(
        super::read_json_capped(req.into_body(), "the assistant request", MAX_BODY_BYTES).await
    );
    match suggest_for(&state, &user, &agent, body).await {
        Ok(suggested) => json_ok(StatusCode::OK, suggested),
        Err(resp) => resp,
    }
}

/// A proposal for `agent` (which `user` may write), as `…/assist/suggest`
/// answers it.
pub(super) async fn suggest_for(
    state: &RamaState,
    user: &User,
    agent: &AgentRow,
    body: SuggestBody,
) -> Result<Suggested, Response> {
    let id = &agent.principal.id;
    let base = body
        .current_draft
        .unwrap_or_else(|| parse_spec(&agent.draft_spec));
    let world = SpecWorld::load(state, id).await?;
    let candidates = candidates(state, user, id).await?;
    let ctx = ReviewContext {
        agent_id: id,
        grants: &world.grants,
        agents: &world.agents,
        live_specs: &world.live_specs,
        candidates: &candidates,
        action_catalogues: &world.action_catalogues,
        allow_private: world.allow_private,
    };
    let asker = Asker {
        state,
        user,
        agent_id: id,
    };
    let request = SuggestRequest {
        scenario: &body.scenario,
        template: body.template.as_deref(),
        base: &base,
        model: body.model.as_deref(),
    };
    asker.suggest(request, &ctx).await.map_err(refused)
}

/// POST /api/v0/agents/{id}/assist/improve — `{field: task|tone|refusal,
/// text, model?}`: `{field, suggestion, why, model, usage}`.
pub async fn improve(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let user = or_return!(require_agent_manager(&state, &req).await);
    let (agent, _) = or_return!(agent_at(&state, &req, &user, 2, Access::Write).await);
    let body: ImproveBody = or_return!(
        super::read_json_capped(req.into_body(), "the assistant request", MAX_BODY_BYTES).await
    );
    let asker = Asker {
        state: &state,
        user: &user,
        agent_id: &agent.principal.id,
    };
    let draft = parse_spec(&agent.draft_spec);
    match asker
        .improve(body.field, &body.text, body.model.as_deref(), &draft)
        .await
    {
        Ok(improved) => json_ok(StatusCode::OK, improved),
        Err(err) => refused(err),
    }
}
