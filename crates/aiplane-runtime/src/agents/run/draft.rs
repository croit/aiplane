// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The internal test chat's half of the run path (`docs/agent-builder.md` → "Test chat"): one turn of an
//! agent's **draft**, and what a manager may see of it that a visitor never
//! does.
//!
//! The test chat starts a turn with [`start_draft_turn`], which claims the
//! conversation as the public endpoint does, so the turn runs in the
//! background and its frames stream like any conversation's. The architect's
//! `run_test_turn` tool, itself inside a turn, runs one to its end with
//! [`run_draft_turn`]. Either way [`collect_turn_debug`] reads what the turn
//! decided from the agent's activity log; nothing else stores it.
//!
//! The draft is passed explicitly as [`SpecSource::Draft`]; nothing about
//! "live" is overridden anywhere, so no other path can reach a draft. The turn
//! then goes through the same `open_session` and `drive_opened` as a
//! visitor's, so gates, slots, tools, binds and budgets behave as they will
//! once published. The conversation is recorded as version [`DRAFT_VERSION`],
//! which no published version has: a visitor session can never continue it,
//! and a test session can never be continued as one.

use std::sync::Arc;

use aiplane_agents::db::run_sessions;
use aiplane_agents::db::{agent_audit, agents as agents_db};
use aiplane_core::server::db::DbError;
use jiff::Timestamp;
use serde::Serialize;
use serde_json::{Value, json};

use super::{AgentReply, AgentTurn, OpenedTurn, drive_opened};
use crate::agents::embed::{TurnClaim, claim};
use crate::agents::gate::{GateInput, GateStatus, Unmet};
use crate::agents::profile::{AgentRunError, Role, RunOptions, RunProfile, SpecSource};
use crate::agents::spec_cache::CompiledSpec;
use crate::agents::state::{AgentState, Provenance, SlotState};
use crate::rama_server::state::RamaState;
use crate::server::headless::{OpenParams, Owner, open_session};
use session_core::db as chat;

pub use agents_db::DRAFT_VERSION;

/// Run one visitor message against `draft` as the agent's principal, to its
/// end.
pub async fn run_draft_turn(
    state: &Arc<RamaState>,
    turn: AgentTurn<'_>,
    draft: &Value,
    options: RunOptions,
) -> Result<AgentReply, AgentRunError> {
    let (profile, opened) = open_draft_turn(state, turn, draft, &options, None).await?;
    drive_opened(state, &profile, &opened).await
}

/// A test-chat turn whose rows exist and whose conversation it holds: drive
/// it in the background, keeping `claim` until it is settled.
pub struct StartedDraftTurn {
    pub claim: TurnClaim,
    pub profile: RunProfile,
    pub turn: OpenedTurn,
}

/// Open one visitor message against `draft` and claim its conversation, so a
/// stream of it follows the turn from its first frame. Refused when the
/// conversation is running a turn already.
pub async fn start_draft_turn(
    state: &Arc<RamaState>,
    turn: AgentTurn<'_>,
    draft: &Value,
    options: &RunOptions,
    initiator_user_id: &str,
) -> Result<StartedDraftTurn, AgentRunError> {
    let busy = |session: &str| AgentRunError::Busy {
        session: session.to_string(),
    };
    if let Some(session) = turn.session_id
        && state.chats.get(turn.agent_id, session).is_some()
    {
        return Err(busy(session));
    }
    let (profile, opened) =
        open_draft_turn(state, turn, draft, options, Some(initiator_user_id)).await?;
    match claim(
        &state.chats,
        &opened.agent_id,
        &opened.session_id,
        &opened.turn_id,
    ) {
        Some(claim) => Ok(StartedDraftTurn {
            claim,
            profile,
            turn: opened,
        }),
        None => {
            let err = busy(&opened.session_id);
            chat::finalize_turn(
                &state.db,
                &opened.turn_id,
                chat::TurnStatus::Errored,
                Some(&err.to_string()),
            )
            .await
            .map_err(DbError::from)?;
            Err(err)
        }
    }
}

/// Load `draft` as the agent's run and open the message's rows in its test
/// conversation, a new one unless `turn` continues one.
async fn open_draft_turn(
    state: &Arc<RamaState>,
    turn: AgentTurn<'_>,
    draft: &Value,
    options: &RunOptions,
    initiator_user_id: Option<&str>,
) -> Result<(RunProfile, OpenedTurn), AgentRunError> {
    let organization = match agents_db::dot_owner(&state.db, turn.agent_id).await? {
        Some(agents_db::DotOwner::Organization(id)) => {
            if initiator_user_id.is_none() || turn.visitor_id.is_some() {
                return Err(AgentRunError::OrganizationExecutionUnavailable(
                    turn.agent_id.to_owned(),
                ));
            }
            Some(id)
        }
        _ => None,
    };
    if let Some(session) = turn.session_id {
        let continues = run_sessions::get_principal_session(&state.db, turn.agent_id, session)
            .await?
            .is_some_and(|run| run.agent_version == Some(DRAFT_VERSION));
        if !continues {
            let agent = aiplane_agents::db::system_principals::get(&state.db, turn.agent_id)
                .await?
                .map_or_else(|| turn.agent_id.to_string(), |p| p.name);
            return Err(AgentRunError::UnknownSession {
                agent,
                session: session.to_string(),
            });
        }
        super::refuse_if_waiting(state, session).await?;
    }
    let profile = RunProfile::load_from(
        state,
        turn.agent_id,
        SpecSource::Draft(draft.clone()),
        Role::Main,
        options,
    )
    .await?;
    let params = OpenParams {
        owner: Owner::Run {
            principal_id: &profile.principal.id,
            parent_turn_id: None,
            agent_version: Some(DRAFT_VERSION),
        },
        title: &profile.principal.name,
        prompt: turn.message,
        model: &profile.model,
        existing_session: turn.session_id.map(str::to_string),
        dot_id: None,
    };
    let opened = if let Some(organization_id) = organization {
        let mut tx = aiplane_core::server::db::WriteTx::begin(&state.db).await?;
        let current_owner = agents_db::get_dot_metadata_in(&mut tx, turn.agent_id).await?;
        if current_owner.is_none_or(|dot| {
            !dot.active || dot.owner != agents_db::DotOwner::Organization(organization_id)
        }) || crate::agents::access::organization_access_in(
            state,
            &mut tx,
            turn.agent_id,
            initiator_user_id.unwrap_or_default(),
        )
        .await?
            != Some(agents_db::Access::Write)
        {
            return Err(AgentRunError::OrganizationExecutionUnavailable(
                turn.agent_id.to_owned(),
            ));
        }
        let opened = crate::server::headless::open_principal_session_in(&mut tx, params).await;
        match opened {
            Ok((session_id, turn_id)) => {
                let chain = Arc::new(aiplane_core::server::run_chain::RunChain::root(
                    &session_id,
                    None,
                    aiplane_core::server::run_chain::Frame::for_principal(&profile.principal, None),
                ));
                let drive = profile.drive_params(&session_id, &turn_id, chain)?;
                let crate::agent_run::Actor::Agent(run) = drive.actor else {
                    return Err(AgentRunError::OrganizationExecutionUnavailable(
                        turn.agent_id.to_owned(),
                    ));
                };
                let current =
                    crate::agents::profile::validate_organization_turn_in(&mut tx, &run).await?;
                if !current.grants.has(
                    aiplane_core::server::principal::GrantKind::Model,
                    &profile.model,
                ) {
                    return Err(AgentRunError::ModelNotGranted {
                        agent: profile.principal.id.clone(),
                        model: profile.model.clone(),
                    });
                }
                tx.commit().await?;
                Ok((session_id, turn_id))
            }
            Err(error) => Err(error),
        }
    } else {
        open_session(&state.db, params).await
    };
    let (session_id, turn_id) = opened.map_err(|error| match error {
        crate::server::headless::OpenError::Storage(error) => AgentRunError::Db(error),
        crate::server::headless::OpenError::Busy => AgentRunError::Busy {
            session: turn.session_id.unwrap_or_default().to_string(),
        },
        crate::server::headless::OpenError::Unavailable => {
            AgentRunError::Unavailable(profile.principal.id.clone())
        }
    })?;
    let opened = OpenedTurn {
        agent_id: profile.principal.id.clone(),
        version: DRAFT_VERSION,
        session_id,
        turn_id,
        visitor_id: None,
        caller: None,
        lang: turn.lang,
    };
    Ok((profile, opened))
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct SlotDebug {
    pub slot: String,
    pub status: &'static str,
    /// Why the slot is `invalid`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The stored value, whoever wrote it: a manager may see what a verifier
    /// stored, the model may not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub set_at: Option<String>,
    pub set_by: Vec<String>,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct RouteDebug {
    pub route: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub open: bool,
    pub missing: Vec<Unmet>,
}

/// What the run left behind, for the manager testing the draft.
#[derive(Debug, Clone, Default, Serialize, schemars::JsonSchema)]
pub struct DraftDebug {
    pub slots: Vec<SlotDebug>,
    pub routes: Vec<RouteDebug>,
    /// Each `forward_request` decision of this turn: every route's gate and
    /// the route picked, if any.
    pub routing: Vec<Value>,
    /// Each sub-agent (or external agent) dispatch of this turn, with its
    /// outcome once finished.
    pub sub_agents: Vec<Value>,
    /// Each tool call of this turn and the grant decision on it.
    pub tool_calls: Vec<Value>,
    /// Each `loop` route iteration of this turn (`loop_iteration`: whether
    /// the critic accepted, and its feedback) and each loop's end
    /// (`loop_finished`: iterations, why it stopped), in order, each with its
    /// `event`. The child runs themselves are in `sub_agents`.
    pub loops: Vec<Value>,
    /// The topic guard's decision on this turn's message, under a strict
    /// scope: `{verdict, topics}`, and `error` when it could not decide.
    pub scope: Option<Value>,
}

/// The conversation's state now, and the decisions turn `turn_id` made: the
/// activity rows of the conversation written from the turn's start until the
/// next turn's, a pause and its resume included.
pub async fn collect_turn_debug(
    state: &RamaState,
    agent_id: &str,
    draft: &Value,
    session_id: &str,
    turn_id: &str,
    options: &RunOptions,
) -> Result<DraftDebug, DbError> {
    let turns = chat::list_turns(&state.db, session_id).await?;
    let Some(at) = turns.iter().position(|t| t.turn.id == turn_id) else {
        return Ok(DraftDebug::default());
    };
    let since = turns[at].turn.created_at;
    let until = turns
        .iter()
        .skip(at + 1)
        .find(|t| t.turn.role == chat::TurnRole::Assistant)
        .map(|t| t.turn.created_at);
    collect_window(state, agent_id, draft, session_id, since, until, options).await
}

/// The state after a run of several turns, and the audit rows they wrote
/// (`since` bounds the conversation's earlier turns out): an evaluation case.
pub async fn collect_debug(
    state: &RamaState,
    agent_id: &str,
    draft: &Value,
    session_id: &str,
    since: Timestamp,
    options: &RunOptions,
) -> Result<DraftDebug, DbError> {
    collect_window(state, agent_id, draft, session_id, since, None, options).await
}

/// The state now, and the audit rows written in `[since, until)`.
async fn collect_window(
    state: &RamaState,
    agent_id: &str,
    draft: &Value,
    session_id: &str,
    since: Timestamp,
    until: Option<Timestamp>,
    options: &RunOptions,
) -> Result<DraftDebug, DbError> {
    let compiled = CompiledSpec::compile(DRAFT_VERSION, draft.clone());
    let Ok(parts) = compiled.parts() else {
        return Ok(DraftDebug::default());
    };
    let (schema, gates) = (&*parts.schema, &*parts.gates);
    let stored = AgentState::load(&state.db, schema, session_id).await?;
    let slots = schema
        .slots()
        .map(|def| slot_debug(def.name.clone(), stored.get(&def.name), &def.set_by))
        .collect();
    let routes = gates
        .statuses(GateInput {
            schema,
            state: &stored,
            now: (options.now)(),
        })
        .into_iter()
        .map(|(route, status)| {
            let (open, missing) = match status {
                GateStatus::Open => (true, Vec::new()),
                GateStatus::Closed { missing } => (false, missing),
            };
            RouteDebug {
                description: parts
                    .agent
                    .routes
                    .get(&route)
                    .and_then(|r| r.description.clone()),
                route,
                open,
                missing,
            }
        })
        .collect();

    let mut debug = DraftDebug {
        slots,
        routes,
        ..DraftDebug::default()
    };
    let mut events = agent_audit::for_principal(&state.db, agent_id).await?;
    events.reverse();
    for event in events.into_iter().filter(|e| {
        e.created_at >= since
            && until.is_none_or(|until| e.created_at < until)
            && e.chain
                .as_ref()
                .and_then(|c| c.get("root_session"))
                .and_then(Value::as_str)
                == Some(session_id)
    }) {
        match event.kind.as_str() {
            "route_decision" => debug.routing.push(event.detail),
            "scope_decision" => {
                let mut scope = json!({
                    "verdict": event.detail["verdict"],
                    "topics": event.detail["topics"],
                });
                if let Some(error) = event.detail.get("error") {
                    scope["error"] = error.clone();
                }
                debug.scope = Some(scope);
            }
            "sub_agent_dispatched" => debug.sub_agents.push(event.detail),
            "sub_agent_finished" => {
                // A sub-agent run is its child turn; an external agent's has
                // none, so its dispatch carries an id of its own.
                let key = |d: &Value| d.get("turn_id").or_else(|| d.get("dispatch_id")).cloned();
                let finished = key(&event.detail);
                match debug
                    .sub_agents
                    .iter_mut()
                    .find(|d| finished.is_some() && key(d) == finished)
                {
                    Some(dispatched) => *dispatched = event.detail,
                    None => debug.sub_agents.push(event.detail),
                }
            }
            "tool_call" => debug.tool_calls.push(event.detail),
            "loop_iteration" | "loop_finished" => {
                let mut detail = event.detail;
                detail["event"] = json!(event.kind);
                debug.loops.push(detail);
            }
            _ => {}
        }
    }
    Ok(debug)
}

/// What turn `turn_id` hands to a person, as the inbox would show it to
/// whoever answers, while it waits for them.
pub async fn waiting_handoff(
    db: &aiplane_core::server::db::Pool,
    turn_id: &str,
) -> Result<Option<Value>, DbError> {
    Ok(chat::get_suspension(db, turn_id)
        .await?
        .filter(|s| s.kind == chat::SuspensionKind::HumanAnswer)
        .and_then(|s| crate::agents::human::handoff_of(s.run_context.as_ref()).cloned()))
}

fn slot_debug(slot: String, state: &SlotState, set_by: &[Provenance]) -> SlotDebug {
    let set_by = set_by.iter().map(ToString::to_string).collect();
    let (status, reason, entry) = match state {
        SlotState::Missing => ("missing", None, None),
        SlotState::Set(entry) => ("set", None, Some(entry)),
        SlotState::Invalid { entry, reason } => ("invalid", Some(reason.clone()), Some(entry)),
    };
    SlotDebug {
        slot,
        status,
        reason,
        value: entry.map(|e| e.value.clone()),
        provenance: entry.map(|e| e.provenance.to_string()),
        set_at: entry.map(|e| e.set_at.to_string()),
        set_by,
    }
}
