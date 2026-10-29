// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Run one turn of an agent conversation as the agent's principal, on its
//! live version. The Rust entry point the internal test chat and the public
//! endpoint put HTTP in front of (`docs/agent-builder.md` → "Test chat", `docs/agent-visitors.md`).

use std::sync::Arc;

use aiplane_agents::db::run_sessions;
use aiplane_agents::db::system_principals as sp;
use aiplane_core::server::db::DbError;
use aiplane_core::server::run_chain::{Frame, RemoteCaller, RunChain};
use session_core::db as chat;
use session_core::i18n::Lang;

use super::output_filter::{Delivery, guard_answer};
use super::profile::{AgentRunError, Role, RunOptions, RunProfile};
use crate::rama_server::state::RamaState;
use crate::server::headless::{OpenParams, Owner, drive, drive_resumed, open_session};
use crate::suspend::ResumeFrom;

/// One visitor message to an agent.
pub struct AgentTurn<'a> {
    /// The agent's id (its principal's).
    pub agent_id: &'a str,
    /// Continue this conversation; `None` starts one.
    pub session_id: Option<&'a str>,
    pub message: &'a str,
    /// The visitor session behind the conversation, for the call chain.
    pub visitor_id: Option<&'a str>,
    /// The language the message was written in; see [`OpenedTurn::lang`].
    pub lang: Option<Lang>,
}

/// How the turn ended.
#[derive(Debug, Clone)]
pub struct AgentReply {
    pub session_id: String,
    pub turn_id: String,
    pub status: chat::TurnStatus,
    /// `None` while the turn is suspended: nothing is final yet.
    pub answer: Option<String>,
    pub error: Option<String>,
    /// What the turn waits for, when it is suspended. The full view; a
    /// visitor gets [`chat::SuspensionView::for_participant`] of it.
    pub suspension: Option<chat::SuspensionView>,
}

/// Refuse a new message into a conversation that waits for a decision: it
/// would run before the paused call, in a context the decision was not asked
/// about. The test chat and a person's own chat refuse; the public endpoint
/// queues one message behind the decision instead (`docs/agent-hil.md`).
pub async fn refuse_if_waiting(state: &RamaState, session_id: &str) -> Result<(), AgentRunError> {
    match chat::suspended_turn_in_session(&state.db, session_id)
        .await
        .map_err(DbError::from)?
    {
        Some(turn) => Err(AgentRunError::DecisionPending {
            session: session_id.to_string(),
            turn,
        }),
        None => Ok(()),
    }
}

pub async fn run_turn(
    state: &Arc<RamaState>,
    turn: AgentTurn<'_>,
) -> Result<AgentReply, AgentRunError> {
    run_turn_with(state, turn, RunOptions::default()).await
}

/// [`run_turn`] with its seams supplied.
///
/// A new conversation runs the live version and records it in
/// `chat_sessions.agent_version`; a continued one keeps running the version
/// it started on, so publishing or rolling back never changes an open
/// conversation mid-way.
pub async fn run_turn_with(
    state: &Arc<RamaState>,
    turn: AgentTurn<'_>,
    options: RunOptions,
) -> Result<AgentReply, AgentRunError> {
    let pinned = match turn.session_id {
        None => None,
        Some(session) => {
            let Some(run) =
                run_sessions::get_principal_session(&state.db, turn.agent_id, session).await?
            else {
                let agent = sp::get(&state.db, turn.agent_id)
                    .await?
                    .map_or_else(|| turn.agent_id.to_string(), |p| p.name);
                return Err(AgentRunError::UnknownSession {
                    agent,
                    session: session.to_string(),
                });
            };
            refuse_if_waiting(state, session).await?;
            run.agent_version
        }
    };
    let profile =
        RunProfile::load_version(state, turn.agent_id, pinned, Role::Main, &options).await?;
    let (session_id, turn_id) = open_session(
        &state.db,
        OpenParams {
            owner: Owner::Run {
                principal_id: &profile.principal.id,
                parent_turn_id: None,
                agent_version: Some(profile.version),
            },
            title: &profile.principal.name,
            prompt: turn.message,
            model: &profile.model,
            existing_session: turn.session_id.map(str::to_string),
            personal_agent_id: None,
        },
    )
    .await
    .map_err(|error| match error {
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
        version: profile.version,
        session_id,
        turn_id,
        visitor_id: turn.visitor_id.map(str::to_string),
        caller: None,
        lang: turn.lang,
    };
    drive_opened(state, &profile, &opened).await
}

/// A turn whose rows already exist: the visitor's user turn and an
/// `in_progress` assistant turn `turn_id` in the principal-owned conversation
/// `session_id`. The public endpoint opens these itself, because it must
/// answer before the turn has run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenedTurn {
    pub agent_id: String,
    /// The version the conversation runs on.
    pub version: i64,
    pub session_id: String,
    pub turn_id: String,
    pub visitor_id: Option<String>,
    /// The remote caller behind the conversation, on an A2A task.
    pub caller: Option<RemoteCaller>,
    /// The language this turn was asked in, when the caller knows it: the
    /// public endpoint and the A2A task take it from the request, `run_turn`
    /// from [`AgentTurn::lang`]. Driving the turn
    /// records it on the conversation (`chat_sessions.lang`), the one place
    /// a run reads its language from ([`conversation_lang`]); `None` keeps
    /// the recorded one, as a resume does, whoever gives the decision.
    pub lang: Option<Lang>,
}

/// The language principal `principal_id`'s conversation `session_id` was
/// last asked in: what the gateway's own text in it is written in. English
/// when it recorded none or cannot be read.
pub(crate) async fn conversation_lang(
    db: &aiplane_core::server::db::Pool,
    principal_id: &str,
    session_id: &str,
) -> Lang {
    match run_sessions::get_principal_session(db, principal_id, session_id).await {
        Ok(session) => session.and_then(|s| s.lang).unwrap_or(Lang::En),
        Err(err) => {
            tracing::warn!(error = %err, session_id, "reading the conversation's language; using English");
            Lang::En
        }
    }
}

impl crate::server::tools::ToolContext {
    /// [`conversation_lang`] of the visitor's conversation this call's run
    /// tree started in: a routed sub-agent speaks the language of the
    /// conversation that dispatched it.
    pub(crate) async fn conversation_lang(&self) -> Lang {
        match self.chain() {
            Some(chain) => {
                conversation_lang(&self.db, &chain.agent().principal_id, &chain.root_session).await
            }
            None => Lang::En,
        }
    }
}

/// The call chain of a main agent's turn: the conversation is its root.
pub fn root_chain(profile: &RunProfile, turn: &OpenedTurn) -> Arc<RunChain> {
    Arc::new(
        RunChain::root(
            &turn.session_id,
            turn.visitor_id.clone(),
            Frame::for_principal(&profile.principal, Some(profile.version)),
        )
        .called_by(turn.caller.clone()),
    )
}

/// Drive an opened turn of `profile` until it is terminal or suspended, and
/// read it back.
pub async fn drive_opened(
    state: &Arc<RamaState>,
    profile: &RunProfile,
    turn: &OpenedTurn,
) -> Result<AgentReply, AgentRunError> {
    drive_opened_from(state, profile, turn, None).await
}

/// [`drive_opened`], continuing the turn from `resume` when it was suspended.
/// Both paths end the same way: a suspended turn reports what it waits for,
/// a terminal one has its answer checked by the output filter.
pub async fn drive_opened_from(
    state: &Arc<RamaState>,
    profile: &RunProfile,
    turn: &OpenedTurn,
    resume: Option<ResumeFrom>,
) -> Result<AgentReply, AgentRunError> {
    let chain = root_chain(profile, turn);
    let reply = drive_and_read(state, profile, turn, resume, &chain).await;
    super::audit::anchor(&state.db, &profile.principal.id, &turn.session_id).await;
    reply
}

/// [`drive_opened_from`] without the anchor every way out of it ends with.
async fn drive_and_read(
    state: &Arc<RamaState>,
    profile: &RunProfile,
    turn: &OpenedTurn,
    resume: Option<ResumeFrom>,
    chain: &Arc<RunChain>,
) -> Result<AgentReply, AgentRunError> {
    let params = profile.drive_params(&turn.session_id, &turn.turn_id, chain.clone())?;
    match resume {
        Some(resume) => drive_resumed(state, params, resume, None).await,
        None => {
            if let Some(lang) = turn.lang {
                run_sessions::set_run_lang(&state.db, &turn.session_id, lang).await?;
            }
            drive(state, params).await
        }
    };
    let done = chat::get_turn(&state.db, &turn.session_id, &turn.turn_id)
        .await
        .map_err(DbError::from)?;
    if done
        .as_ref()
        .is_some_and(|t| t.status == chat::TurnStatus::Suspended)
    {
        let suspension = chat::get_suspension(&state.db, &turn.turn_id)
            .await
            .map_err(DbError::from)?
            .map(|s| s.view());
        if let Some(waiting) = &suspension {
            super::inbox::announce_in_background(state.clone(), waiting.request_id.clone());
        }
        return Ok(AgentReply {
            session_id: turn.session_id.clone(),
            turn_id: turn.turn_id.clone(),
            status: chat::TurnStatus::Suspended,
            answer: None,
            error: None,
            suspension,
        });
    }
    let mut answer = done.as_ref().and_then(|t| t.content.clone());
    if let Some(filter) = &profile.output_filter
        && let Some(text) = answer.take()
    {
        let at = Delivery {
            principal_id: &profile.principal.id,
            session_id: &turn.session_id,
            turn_id: &turn.turn_id,
            chain,
            lang: conversation_lang(&state.db, &profile.principal.id, &turn.session_id).await,
        };
        answer = Some(guard_answer(state, filter, &profile.surface, at, text).await);
    }
    Ok(AgentReply {
        status: done
            .as_ref()
            .map_or(chat::TurnStatus::Errored, |t| t.status),
        answer,
        error: done.and_then(|t| t.error_message),
        session_id: turn.session_id.clone(),
        turn_id: turn.turn_id.clone(),
        suspension: None,
    })
}

pub mod draft;

#[cfg(test)]
mod tests;
