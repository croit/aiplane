// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Resuming a suspended agent run (`docs/agent-hil.md` → "Suspend and resume").
//!
//! An agent conversation pauses at its main turn. When the pause is inside
//! a sub-agent run, every turn between the conversation and that run is
//! suspended too, each pointing at the one below through `child_turn`, and
//! the conversation's turn mirrors the innermost request. One decision
//! settles all of them, innermost first:
//!
//! 1. [`claim`] walks the chain from the conversation's turn down, checks the
//!    decision against the innermost request and who may give it, and claims
//!    every row (innermost first: that claim is the one two answers race
//!    for).
//! 2. [`run_claimed`] rebuilds each run from its session and its parent's
//!    pause, resumes the innermost with the decision, hands its outcome to
//!    its parent as the waiting `forward_request` result, and so on up to the
//!    conversation's turn, which ends as any turn does: answer, output
//!    filter, or a new pause.
//!
//! A message the visitor sent while the conversation waited runs once the
//! resumed turn is over.

use std::sync::Arc;

use aiplane_agents::db::agent_audit::{AuditKind, Correlation, NewEvent};
use aiplane_agents::db::run_sessions;
use aiplane_agents::db::run_sessions::RunSession;
use aiplane_agents::db::{a2a_contexts, agents as agents_db};
use aiplane_core::server::db::DbError;
use aiplane_core::server::run_chain::{CallSite, Frame, MAX_DEPTH, RunChain};
use serde_json::{Value, json};
use session_core::db::{
    self as chat, Answerer, Decision, DenyReason, SuspensionKind, TurnRole, TurnStatus,
    TurnSuspension,
};
use session_core::i18n::Lang;

use super::human::end_unanswered;
use super::profile::{
    AgentRunError, OrganizationResumeAuthority, Role, RunOptions, RunProfile, SpecSource,
};
use super::router::{Caller, dispatch_result};
use super::run::{AgentReply, OpenedTurn, drive_opened, drive_opened_from, root_chain};
use crate::rama_server::state::RamaState;
use crate::server::headless::drive_resumed;
use crate::suspend::{ResumeFrom, ResumeRefused};

/// Who gives the decision, for the rules on who may and for the audit row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResumedBy {
    /// The visitor of the conversation, with their visitor token.
    Participant,
    /// A manager with a share on the agent, or an admin.
    Staff { user_id: String },
    /// Nobody: the request expired and its fallback applies.
    Timeout,
}

impl ResumedBy {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Participant => "participant",
            Self::Staff { .. } => "staff",
            Self::Timeout => "timeout",
        }
    }
}

/// One decision for a suspended agent conversation.
pub struct AgentResume<'a> {
    pub agent_id: &'a str,
    pub session_id: &'a str,
    /// The conversation's suspended turn.
    pub turn_id: &'a str,
    /// The request being answered, as the `suspended` frame named it. When
    /// given, an answer to an earlier pause is refused.
    pub request_id: Option<&'a str>,
    pub decision: Decision,
    pub by: ResumedBy,
}

/// Why a resume was refused. Nothing was claimed in any of these cases.
#[derive(Debug, thiserror::Error)]
pub enum AgentResumeError {
    #[error("A child conversation already has a running worker. Retry after it finishes.")]
    Busy,
    #[error(
        "The personal Dot run is unavailable under its current ownership, consent or access. Review its settings before retrying."
    )]
    Unavailable,
    #[error(transparent)]
    Refused(#[from] ResumeRefused),
    #[error(
        "this request is for the agent's staff to answer (`{kind}`); a visitor can only wait for \
         their decision"
    )]
    StaffOnly { kind: &'static str },
    #[error(
        "only the visitor can answer a `{kind}` request: the value goes from them to the tool \
         that asked and to nobody else"
    )]
    ParticipantOnly { kind: &'static str },
    #[error("reading the suspended agent run failed: {0}")]
    Db(#[from] DbError),
}

impl From<chat::DbError> for AgentResumeError {
    fn from(e: chat::DbError) -> Self {
        Self::Db(e.into())
    }
}

/// One suspended run of the chain: its conversation and its pause.
#[derive(Debug, Clone)]
struct Level {
    session: RunSession,
    suspension: TurnSuspension,
}

/// A decision that won the claim: every row of the chain is gone and every
/// turn is `in_progress` again. Hand it to [`run_claimed`], which is the only
/// thing that may happen next; dropping it leaves the turns without a run.
#[derive(Debug)]
pub struct ClaimedResume {
    levels: Vec<Level>,
    decision: Decision,
    by: ResumedBy,
}

impl ClaimedResume {
    /// The conversation's turn, which the resume continues.
    pub fn turn_id(&self) -> &str {
        &self.levels[0].suspension.turn_id
    }

    pub fn session_id(&self) -> &str {
        &self.levels[0].session.id
    }

    /// The version the conversation runs; `DRAFT_VERSION` for the test chat.
    pub fn agent_version(&self) -> Option<i64> {
        self.levels[0].session.agent_version
    }
}

fn not_suspended() -> AgentResumeError {
    AgentResumeError::Refused(ResumeRefused::NotSuspended)
}

/// Check `resume` against the conversation's suspension and claim it.
pub async fn claim(
    state: &RamaState,
    resume: AgentResume<'_>,
) -> Result<ClaimedResume, AgentResumeError> {
    let db = &state.db;
    let root = run_sessions::get_principal_session(db, resume.agent_id, resume.session_id)
        .await?
        .filter(|s| s.parent_turn_id.is_none())
        .ok_or_else(not_suspended)?;
    if run_sessions::run_session_of_turn(db, resume.turn_id)
        .await?
        .is_none_or(|s| s.id != root.id)
    {
        return Err(not_suspended());
    }
    let top = chat::get_suspension(db, resume.turn_id)
        .await?
        .ok_or_else(not_suspended)?;
    if let Some(asked) = resume.request_id
        && asked != top.request_id
    {
        return Err(ResumeRefused::StaleRequest {
            current: top.request_id,
        }
        .into());
    }

    let mut levels = vec![Level {
        session: root,
        suspension: top,
    }];
    while let Some(child) = levels.last().and_then(|l| l.suspension.child_turn.clone()) {
        let parent_turn = levels.last().map(|l| l.suspension.turn_id.clone());
        let session = run_sessions::run_session_of_turn(db, &child)
            .await?
            .filter(|s| s.parent_turn_id == parent_turn)
            .ok_or_else(not_suspended)?;
        let suspension = chat::get_suspension(db, &child)
            .await?
            .ok_or_else(not_suspended)?;
        levels.push(Level {
            session,
            suspension,
        });
        if levels.len() > MAX_DEPTH {
            return Err(not_suspended());
        }
    }

    let kind = levels
        .last()
        .map(|l| l.suspension.kind)
        .ok_or_else(not_suspended)?;
    let testing = levels[0].session.agent_version == Some(agents_db::DRAFT_VERSION);
    let organization = matches!(
        agents_db::dot_owner(db, &levels[0].session.principal_id).await?,
        Some(agents_db::DotOwner::Organization(_))
    );
    match (&resume.by, kind.answered_by()) {
        (ResumedBy::Participant, Answerer::Staff) => {
            return Err(AgentResumeError::StaffOnly {
                kind: kind.as_str(),
            });
        }
        (ResumedBy::Staff { .. }, Answerer::Participant) if !testing && !organization => {
            return Err(AgentResumeError::ParticipantOnly {
                kind: kind.as_str(),
            });
        }
        _ => {}
    }

    let mut tx = aiplane_agents::db::WriteTx::begin(db).await?;
    if let ResumedBy::Staff { user_id } = &resume.by {
        let root = levels.first().ok_or_else(not_suspended)?;
        if super::access::effective_access_in(state, &mut tx, &root.session.principal_id, user_id)
            .await?
            .is_none()
        {
            return Err(not_suspended());
        }
    }
    for level in &levels {
        let owner = agents_db::dot_owner_in(&mut tx, &level.session.principal_id).await?;
        if let Some(agents_db::DotOwner::Organization(organization_id)) = owner {
            if levels.len() != 1
                || !level
                    .session
                    .agent_version
                    .is_some_and(|version| version >= 0)
                || level.session.visitor_id.is_some()
                || level.session.parent_turn_id.is_some()
            {
                return Err(not_suspended());
            }
            match &resume.by {
                ResumedBy::Staff { user_id } => {
                    if super::access::organization_access_in(
                        state,
                        &mut tx,
                        &level.session.principal_id,
                        user_id,
                    )
                    .await?
                        != Some(agents_db::Access::Write)
                    {
                        return Err(not_suspended());
                    }
                }
                ResumedBy::Timeout
                    if matches!(
                        resume.decision,
                        Decision::Deny {
                            reason: DenyReason::Timeout
                        }
                    ) => {}
                _ => return Err(not_suspended()),
            }
            let authority =
                OrganizationResumeAuthority::from_context(level.suspension.run_context.as_ref())
                    .ok_or_else(not_suspended)?;
            if authority.principal_id != level.session.principal_id
                || authority.organization_id != organization_id
                || authority.session_id != level.session.id
                || authority.assistant_turn_id != level.suspension.turn_id
                || authority.version() != level.session.agent_version
            {
                return Err(not_suspended());
            }
            let principal = super::profile::validate_organization_resume_in(&mut tx, &authority)
                .await
                .map_err(resume_authority_error)?;
            let model_access = aiplane_core::server::upstreams::PoolAccess::for_system_models(
                &principal,
                [authority.model.as_str()],
            );
            if !crate::server::model_choices::offered_in(
                state,
                &mut tx,
                aiplane_core::server::upstreams::PoolKind::Chat,
                &model_access,
            )
            .await?
            .iter()
            .any(|choice| choice.id == authority.model)
            {
                return Err(not_suspended());
            }
            validate_organization_pending_in(&mut tx, level, &authority, &principal).await?;
        }
    }
    for level in &levels {
        validate_session(&mut tx, level).await?;
        if chat::get_suspension_in(&mut tx, &level.suspension.turn_id)
            .await?
            .as_ref()
            != Some(&level.suspension)
        {
            return Err(not_suspended());
        }
    }
    let snapshots: Vec<_> = levels
        .iter()
        .map(|level| level.suspension.clone())
        .collect();
    let claimed =
        crate::suspend::claim_chain_in(&mut tx, &snapshots, resume.decision.clone()).await?;
    for (level, suspension) in levels.iter_mut().zip(claimed) {
        level.suspension = suspension;
    }
    for level in &levels {
        validate_session(&mut tx, level).await?;
    }
    tx.commit().await?;
    Ok(ClaimedResume {
        levels,
        decision: resume.decision,
        by: resume.by,
    })
}

fn resume_authority_error(error: AgentRunError) -> AgentResumeError {
    match error {
        AgentRunError::Db(error) => AgentResumeError::Db(error),
        _ => not_suspended(),
    }
}

async fn validate_organization_pending_in(
    tx: &mut aiplane_agents::db::WriteTx,
    level: &Level,
    authority: &OrganizationResumeAuthority,
    principal: &aiplane_core::server::principal::SystemPrincipal,
) -> Result<(), AgentResumeError> {
    use crate::server::tools::mcp::{
        MCP_ID_PREFIX,
        identities::{McpDispatchFacts, selected_principal_identity_in},
    };
    use aiplane_core::server::principal::GrantKind;
    let call = &level.suspension.tool_call;
    let mut dependencies = match authority.synthetic_tools.get(&call.name) {
        Some(dependencies) => dependencies.clone(),
        None => vec![call.name.clone()],
    };
    if call.name == super::router::FORWARD_TOOL_NAME {
        return Err(not_suspended());
    }
    let mut connectors = std::collections::BTreeSet::new();
    for dependency in dependencies.drain(..) {
        if let Some(rest) = dependency.strip_prefix(MCP_ID_PREFIX) {
            let (connector, _) = rest.split_once("__").ok_or_else(not_suspended)?;
            if !principal.grants.has(GrantKind::Connector, connector) {
                return Err(not_suspended());
            }
            connectors.insert(connector.to_owned());
        } else if !principal.grants.has(GrantKind::Tool, &dependency) {
            return Err(not_suspended());
        }
    }
    if !connectors.is_empty() {
        let facts = call.execution_facts.as_ref().ok_or_else(not_suspended)?;
        let McpDispatchFacts::Owned(expected) =
            serde_json::from_value::<McpDispatchFacts>(facts.clone())
                .map_err(|_| not_suspended())?
        else {
            return Err(not_suspended());
        };
        if connectors.len() != 1
            || !connectors.contains(&expected.connection.connector_key)
            || selected_principal_identity_in(
                tx,
                &agents_db::DotOwner::Organization(authority.organization_id.clone()),
                &authority.principal_id,
                &expected.connection.connector_key,
            )
            .await?
            .as_ref()
                != Some(&expected)
        {
            return Err(not_suspended());
        }
    }
    Ok(())
}

async fn validate_session(
    connection: &mut sqlx::SqliteConnection,
    level: &Level,
) -> Result<(), AgentResumeError> {
    let valid: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM chat_turns t
         JOIN chat_sessions s ON s.id = t.session_id
         WHERE t.id = ? AND t.role = 'assistant' AND s.id = ?
           AND s.principal_id = ? AND s.parent_turn_id IS ?
           AND s.agent_version IS ? AND s.visitor_id IS ?)",
    )
    .bind(&level.suspension.turn_id)
    .bind(&level.session.id)
    .bind(&level.session.principal_id)
    .bind(level.session.parent_turn_id.as_deref())
    .bind(level.session.agent_version)
    .bind(level.session.visitor_id.as_deref())
    .fetch_one(connection)
    .await
    .map_err(DbError::from)?;
    if !valid {
        return Err(not_suspended());
    }
    Ok(())
}

/// Error every claimed turn of `levels`, so none is left `in_progress`
/// without a run.
async fn fail(state: &RamaState, levels: &[Level], message: &str) {
    for level in levels {
        let turn = &level.suspension.turn_id;
        if let Err(err) =
            chat::finalize_turn(&state.db, turn, TurnStatus::Errored, Some(message)).await
        {
            tracing::warn!(error = %err, %turn, "erroring a resumed agent turn that cannot run");
        }
    }
}

/// The spec a level runs, as its conversation recorded it.
async fn source_of(state: &RamaState, session: &RunSession) -> Result<SpecSource, AgentRunError> {
    Ok(match session.agent_version {
        Some(agents_db::DRAFT_VERSION) => {
            let draft = agents_db::get(&state.db, &session.principal_id)
                .await?
                .map(|a| serde_json::from_str(&a.draft_spec).unwrap_or(Value::Null))
                .unwrap_or(Value::Null);
            SpecSource::Draft(draft)
        }
        Some(version) => SpecSource::Pinned(version),
        None => SpecSource::Live,
    })
}

fn route_binds(level: &Level) -> Value {
    level
        .suspension
        .run_context
        .as_ref()
        .and_then(|c| c.get("route_binds"))
        .cloned()
        .unwrap_or_else(|| json!({}))
}

/// Rebuild every run of the chain: its profile and its call chain.
async fn rebuild(
    state: &Arc<RamaState>,
    levels: &[Level],
    options: &RunOptions,
) -> Result<(Vec<RunProfile>, Vec<Arc<RunChain>>, OpenedTurn), AgentRunError> {
    let mut profiles: Vec<RunProfile> = Vec::with_capacity(levels.len());
    let mut chains: Vec<Arc<RunChain>> = Vec::with_capacity(levels.len());
    let root = &levels[0];
    let mut opened = None;
    for (i, level) in levels.iter().enumerate() {
        let role = if i == 0 {
            Role::Main
        } else {
            let binds = serde_json::from_value(route_binds(level)).unwrap_or_default();
            Role::SubAgent { route_binds: binds }
        };
        let profile = if matches!(
            agents_db::dot_owner(&state.db, &level.session.principal_id).await?,
            Some(agents_db::DotOwner::Organization(_))
        ) {
            let authority =
                OrganizationResumeAuthority::from_context(level.suspension.run_context.as_ref())
                    .ok_or_else(|| {
                        AgentRunError::OrganizationExecutionUnavailable(
                            level.session.principal_id.clone(),
                        )
                    })?;
            RunProfile::load_organization_resume(state, &authority, options).await?
        } else {
            let source = source_of(state, &level.session).await?;
            RunProfile::load_from(state, &level.session.principal_id, source, role, options).await?
        };
        let chain = match i {
            0 => {
                let turn = OpenedTurn {
                    agent_id: profile.principal.id.clone(),
                    version: profile.version,
                    session_id: root.session.id.clone(),
                    turn_id: root.suspension.turn_id.clone(),
                    visitor_id: root.session.visitor_id.clone(),
                    caller: a2a_contexts::get(&state.db, &root.session.id)
                        .await?
                        .map(|c| c.caller()),
                    lang: None,
                };
                let chain = root_chain(&profile, &turn);
                opened = Some(turn);
                chain
            }
            _ => {
                let parent = &levels[i - 1].suspension;
                let site = CallSite {
                    turn_id: parent.turn_id.clone(),
                    tool_call_id: parent.tool_call.id.clone(),
                };
                let frame = Frame::for_principal(&profile.principal, Some(profile.version))
                    .called_from(site);
                Arc::new(
                    chains[i - 1]
                        .enter(frame)
                        .map_err(|e| AgentRunError::BadSpec {
                            agent: profile.principal.name.clone(),
                            version: profile.version,
                            message: e.to_string(),
                        })?,
                )
            }
        };
        profiles.push(profile);
        chains.push(chain);
    }
    let opened = opened.expect("the chain has a root");
    Ok((profiles, chains, opened))
}

/// Continue a claimed resume to its end: every sub-agent run innermost
/// first, then the conversation's turn. Returns how that turn ended; a
/// message queued behind the decision runs after it.
///
/// The run speaks the language the conversation's turn was asked in, not
/// the one of whoever decided: staff answer from the inbox in theirs, and a
/// timeout has none.
pub async fn run_claimed(
    state: &Arc<RamaState>,
    claimed: ClaimedResume,
    options: RunOptions,
) -> Result<AgentReply, AgentRunError> {
    let ClaimedResume {
        levels,
        decision,
        by,
    } = claimed;
    let lang = levels[0].session.lang.unwrap_or(Lang::En);
    let (profiles, chains, opened) = match rebuild(state, &levels, &options).await {
        Ok(rebuilt) => rebuilt,
        Err(err) => {
            fail(state, &levels, &err.to_string()).await;
            return Err(err);
        }
    };
    let innermost = &levels[levels.len() - 1].suspension;
    let actor = match &by {
        ResumedBy::Staff { user_id } => Some(user_id.as_str()),
        _ => None,
    };
    let mut detail = json!({
        "session_id": opened.session_id,
        "turn_id": opened.turn_id,
        "request_id": levels[0].suspension.request_id,
        "kind": innermost.kind,
        "decision": decision.kind(),
        "answered_by": by.as_str(),
        "waiting_turn": innermost.turn_id,
    });
    if let Decision::Value { value } = &decision {
        // A staff answer to a handoff is what the agent passes on, and the
        // log keeps it; a secure input is only ever marked as received.
        match innermost.kind {
            SuspensionKind::HumanAnswer => detail["answer"] = value.clone(),
            _ => detail["secure_input_received"] = json!(true),
        }
    }
    super::audit::record_event(
        &state.db,
        NewEvent::new(AuditKind::RunResumed, &profiles[0].principal.id, detail)
            .by(actor)
            .in_run(Some(&chains[0]))
            .at(Correlation {
                session_id: Some(opened.session_id.clone()),
                turn_id: Some(opened.turn_id.clone()),
                ..Correlation::default()
            }),
    )
    .await
    .ok();
    tracing::info!(
        turn = %opened.turn_id,
        depth = levels.len(),
        kind = innermost.kind.as_str(),
        decision = ?decision.kind(),
        answered_by = by.as_str(),
        "resuming a suspended agent run"
    );

    if levels.len() == 1
        && innermost.kind == SuspensionKind::HumanAnswer
        && matches!(
            decision,
            Decision::Deny {
                reason: DenyReason::Timeout
            }
        )
    {
        let answer = end_unanswered(&state.db, innermost, lang)
            .await
            .map_err(DbError::from)?;
        run_queued(state, &profiles[0], &opened).await?;
        return Ok(AgentReply {
            session_id: opened.session_id.clone(),
            turn_id: opened.turn_id.clone(),
            status: TurnStatus::Completed,
            answer: Some(answer),
            error: None,
            suspension: None,
        });
    }

    let mut child_result = None;
    for i in (1..levels.len()).rev() {
        let level = &levels[i];
        let profile = &profiles[i];
        let resume = ResumeFrom {
            suspension: level.suspension.clone(),
            decision: decision.clone(),
            child_result: child_result.take(),
        };
        let params = profile.drive_params(
            &level.session.id,
            &level.suspension.turn_id,
            chains[i].clone(),
        )?;
        let outcome = drive_resumed(state, params, resume, None).await;
        let route = level
            .suspension
            .run_context
            .as_ref()
            .and_then(|c| c.get("route"))
            .cloned()
            .unwrap_or(Value::Null);
        let about = json!({
            "route": route,
            "sub_agent": profile.principal.name,
            "sub_agent_id": profile.principal.id,
            "version": profile.version,
            "session_id": level.session.id,
            "turn_id": level.suspension.turn_id,
        });
        let caller = Caller {
            principal_id: &profiles[i - 1].principal.id,
            chain: Some(&chains[i - 1]),
        };
        let result = dispatch_result(&state.db, caller, about, &route_binds(level), outcome)
            .await
            .unwrap_or_else(|e| json!({ "error": e.to_string() }));
        child_result = Some(result);
    }
    let resume = ResumeFrom {
        suspension: levels[0].suspension.clone(),
        decision,
        child_result,
    };
    let reply = drive_opened_from(state, &profiles[0], &opened, Some(resume)).await?;
    if reply.status != TurnStatus::Suspended {
        run_queued(state, &profiles[0], &opened).await?;
    }
    Ok(reply)
}

/// The visitor's message that waited behind the decision, if one did: a
/// user turn after the resumed turn with no answer yet. It runs now, as the
/// next turn of the conversation.
async fn run_queued(
    state: &Arc<RamaState>,
    profile: &RunProfile,
    resumed: &OpenedTurn,
) -> Result<(), AgentRunError> {
    let turns = chat::list_turns(&state.db, &resumed.session_id)
        .await
        .map_err(DbError::from)?;
    let Some(last) = turns.last() else {
        return Ok(());
    };
    if last.turn.role != TurnRole::User || last.turn.id == resumed.turn_id {
        return Ok(());
    }
    let turn_id = uuid::Uuid::new_v4().to_string();
    chat::create_assistant_turn_in_progress(
        &state.db,
        &resumed.session_id,
        &turn_id,
        &profile.model,
    )
    .await
    .map_err(DbError::from)?;
    state.chats.hand_over(
        &profile.principal.id,
        &resumed.session_id,
        &resumed.turn_id,
        &turn_id,
    );
    let next = OpenedTurn {
        turn_id,
        ..resumed.clone()
    };
    drive_opened(state, profile, &next).await.map(|_| ())
}

/// Settle every agent conversation whose request expired with the request's
/// fallback (deny, for every kind today). A conversation whose turn is held
/// right now is left for the next sweep.
pub async fn resume_expired(state: &Arc<RamaState>) {
    let expired =
        match run_sessions::expired_run_suspensions(&state.db, jiff::Timestamp::now()).await {
            Ok(expired) => expired,
            Err(err) => {
                tracing::warn!(error = %err, "reading expired agent suspensions");
                return;
            }
        };
    for e in expired {
        let Some(hold) =
            super::embed::claim(&state.chats, &e.principal_id, &e.session_id, &e.turn_id)
        else {
            continue;
        };
        let claimed = claim(
            state,
            AgentResume {
                agent_id: &e.principal_id,
                session_id: &e.session_id,
                turn_id: &e.turn_id,
                request_id: None,
                decision: e.on_timeout.decision(),
                by: ResumedBy::Timeout,
            },
        )
        .await;
        match claimed {
            Ok(claimed) => {
                let state = state.clone();
                tokio::spawn(async move {
                    let _hold = hold;
                    let turn = claimed.turn_id().to_string();
                    if let Err(err) = run_claimed(&state, claimed, RunOptions::default()).await {
                        tracing::warn!(error = %err, %turn, "an expired agent request could not resume");
                    }
                });
            }
            Err(err) => {
                tracing::debug!(turn = %e.turn_id, %err, "expired agent request already settled");
            }
        }
    }
}

struct PersonalLevel {
    binding: aiplane_agents::db::dot_sessions::PersonalDotSession,
    link: Option<aiplane_agents::db::dot_sessions::PersonalChildLink>,
    suspension: TurnSuspension,
    model: String,
}

pub struct ClaimedPersonalResume {
    owner: String,
    levels: Vec<PersonalLevel>,
    decision: Decision,
    workers: Vec<Option<crate::server::headless::BackgroundWorker>>,
}

impl ClaimedPersonalResume {
    pub fn turn_id(&self) -> &str {
        &self.levels[0].suspension.turn_id
    }
    pub fn session_id(&self) -> &str {
        &self.levels[0].binding.session_id
    }
}

async fn validate_personal_level_in(
    connection: &mut sqlx::SqliteConnection,
    owner: &str,
    level: &PersonalLevel,
) -> Result<(), AgentResumeError> {
    let valid: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM chat_turns t JOIN chat_sessions s ON s.id = t.session_id
         JOIN personal_dot_sessions p ON p.session_id = s.id JOIN dots d ON d.principal_id = p.dot_id
         JOIN system_principals principal ON principal.id = d.principal_id
         WHERE t.id = ? AND t.role = 'assistant' AND t.model = ? AND s.id = ? AND s.user_id = ?
           AND s.principal_id IS NULL AND s.shared = 0 AND p.owner_user_id = ? AND p.dot_id = ?
           AND d.owner_user_id = ? AND d.active = 1 AND principal.disabled_at IS NULL AND s.parent_turn_id IS ? AND p.parent_turn_id IS ?)",
    ).bind(&level.suspension.turn_id).bind(&level.model).bind(&level.binding.session_id).bind(owner)
        .bind(owner).bind(&level.binding.dot_id).bind(owner)
        .bind(level.link.as_ref().map(|link| link.parent_turn_id.as_str()))
        .bind(level.link.as_ref().map(|link| link.parent_turn_id.as_str()))
        .fetch_one(&mut *connection).await.map_err(DbError::from)?;
    if !valid {
        return Err(AgentResumeError::Unavailable);
    }
    if let Some(link) = &level.link
        && !aiplane_agents::db::child_sessions::validate_child_in(
            connection,
            owner,
            &level.suspension.turn_id,
            &link.parent_turn_id,
            &link.parent_tool_call_id,
            &link.edge_generation,
        )
        .await?
    {
        return Err(AgentResumeError::Unavailable);
    }
    Ok(())
}

pub async fn claim_personal(
    state: &Arc<RamaState>,
    owner: &str,
    root_turn: &str,
    request_id: Option<&str>,
    decision: Decision,
) -> Result<ClaimedPersonalResume, AgentResumeError> {
    use aiplane_agents::db::{child_sessions, dot_sessions};
    if chat::user_for_root_turn(&state.db, root_turn)
        .await?
        .as_deref()
        != Some(owner)
    {
        return Err(AgentResumeError::Unavailable);
    }
    let top = chat::get_suspension(&state.db, root_turn)
        .await?
        .ok_or_else(not_suspended)?;
    if let Some(request_id) = request_id
        && request_id != top.request_id
    {
        return Err(ResumeRefused::StaleRequest {
            current: top.request_id,
        }
        .into());
    }
    let root_session: String = sqlx::query_scalar("SELECT session_id FROM chat_turns WHERE id = ?")
        .bind(root_turn)
        .fetch_one(&state.db)
        .await
        .map_err(DbError::from)?;
    let binding = dot_sessions::for_session(&state.db, owner, &root_session)
        .await?
        .filter(|binding| binding.active)
        .ok_or(AgentResumeError::Unavailable)?;
    let root_model = chat::get_turn(&state.db, &root_session, root_turn)
        .await?
        .and_then(|turn| turn.model)
        .ok_or(AgentResumeError::Unavailable)?;
    let mut levels = vec![PersonalLevel {
        binding,
        link: None,
        suspension: top,
        model: root_model,
    }];
    while let Some(child_turn) = levels
        .last()
        .and_then(|level| level.suspension.child_turn.clone())
    {
        if levels.len() >= MAX_DEPTH {
            return Err(AgentResumeError::Unavailable);
        }
        let Some(child_sessions::LocalChild::Personal(child)) =
            child_sessions::child_of_turn(&state.db, &child_turn).await?
        else {
            return Err(AgentResumeError::Unavailable);
        };
        let parent = levels.last().ok_or(AgentResumeError::Unavailable)?;
        if child.session.owner_user_id != owner
            || child.link.parent_turn_id != parent.suspension.turn_id
            || child.link.parent_tool_call_id != parent.suspension.tool_call.id
            || child.link.caller_dot_id != parent.binding.dot_id
            || parent.suspension.tool_call.name != super::router::FORWARD_TOOL_NAME
            || levels
                .iter()
                .any(|level| level.binding.dot_id == child.session.dot_id)
        {
            return Err(AgentResumeError::Unavailable);
        }
        let suspension = chat::get_suspension(&state.db, &child_turn)
            .await?
            .ok_or_else(not_suspended)?;
        let model = chat::get_turn(&state.db, &child.session.session_id, &child_turn)
            .await?
            .and_then(|turn| turn.model)
            .ok_or(AgentResumeError::Unavailable)?;
        levels.push(PersonalLevel {
            binding: child.session,
            link: Some(child.link),
            suspension,
            model,
        });
    }
    let user = aiplane_core::server::db::users::find_by_id(&state.db, owner)
        .await?
        .ok_or(AgentResumeError::Unavailable)?;
    let access =
        state.pool_access_for_principal(&aiplane_core::server::principal::Principal::User {
            id: user.id.clone(),
            roles: user.roles.clone(),
        });
    let offered = crate::server::model_choices::offered(
        state,
        aiplane_core::server::upstreams::PoolKind::Chat,
        &access,
    )
    .await;
    for (index, level) in levels.iter().enumerate() {
        if !offered.iter().any(|model| model.id == level.model) {
            return Err(AgentResumeError::Unavailable);
        }
        let dot = agents_db::get_personal_dot(&state.db, owner, &level.binding.dot_id)
            .await?
            .filter(|dot| dot.active && dot.agent.principal.disabled_at.is_none())
            .ok_or(AgentResumeError::Unavailable)?;
        let spec: super::spec::AgentSpec = serde_json::from_str(&dot.agent.draft_spec)
            .map_err(|_| AgentResumeError::Unavailable)?;
        if index + 1 < levels.len()
            && !super::router::local::forward_available(state, &user, &spec)
                .await
                .map_err(|_| AgentResumeError::Unavailable)?
        {
            return Err(AgentResumeError::Unavailable);
        }
    }
    let mut workers = vec![None];
    for level in levels.iter().skip(1) {
        match state.chats.register_background(
            owner,
            &level.suspension.turn_id,
            &level.binding.session_id,
        ) {
            session_core::workers::RegisterOutcome::Registered { worker } => {
                workers.push(Some(crate::server::headless::BackgroundWorker::new(
                    state.clone(),
                    owner.to_owned(),
                    worker,
                )))
            }
            _ => return Err(AgentResumeError::Busy),
        }
    }
    let mut tx = aiplane_agents::db::WriteTx::begin(&state.db).await?;
    for level in &levels {
        validate_personal_level_in(&mut tx, owner, level).await?;
    }
    let snapshots: Vec<_> = levels
        .iter()
        .map(|level| level.suspension.clone())
        .collect();
    let claimed = crate::suspend::claim_chain_in(&mut tx, &snapshots, decision.clone()).await?;
    for level in &levels {
        validate_personal_level_in(&mut tx, owner, level).await?;
    }
    for (level, suspension) in levels.iter_mut().zip(claimed) {
        level.suspension = suspension;
    }
    tx.commit().await?;
    Ok(ClaimedPersonalResume {
        owner: owner.to_owned(),
        levels,
        decision,
        workers,
    })
}

pub async fn run_personal_children(
    state: &Arc<RamaState>,
    claimed: ClaimedPersonalResume,
) -> Result<ResumeFrom, AgentResumeError> {
    let root_turn = claimed.turn_id().to_owned();
    let owner = claimed.owner.clone();
    match run_personal_children_inner(state, claimed).await {
        Ok(resumed) => Ok(resumed),
        Err(error) => {
            if let Err(cleanup) = cancel_personal_tree(state, &owner, &root_turn).await {
                tracing::error!(%cleanup, %root_turn, "settling the claimed personal run failed");
            }
            Err(error)
        }
    }
}

async fn run_personal_children_inner(
    state: &Arc<RamaState>,
    mut claimed: ClaimedPersonalResume,
) -> Result<ResumeFrom, AgentResumeError> {
    use crate::agent_run::{
        Actor, PersonalDotExecution, PersonalDotIdentity, PersonalExecutionBinding,
    };
    use crate::persona::personal_dot::{
        PersonalDotContext, PersonalTurnContext, initialize_for_turn,
    };
    let user = aiplane_core::server::db::users::find_by_id(&state.db, &claimed.owner)
        .await?
        .ok_or(AgentResumeError::Unavailable)?;
    let root = &claimed.levels[0];
    let background =
        crate::server::background::for_turn(&state.db, &claimed.owner, &root.suspension.turn_id)
            .await?;
    let enabled = background
        .as_ref()
        .is_none_or(|context| context.tools_enabled);
    let source = background.as_ref().map_or(
        aiplane_core::server::db::usage::UsageSource::Chat,
        |context| context.source,
    );
    let history_limit = background.and_then(|context| context.history_limit);
    let Some(PersonalDotContext::Active { actor, .. }) = initialize_for_turn(
        state,
        &user,
        enabled,
        PersonalTurnContext {
            session_id: &root.binding.session_id,
            assistant_turn_id: &root.suspension.turn_id,
            model: &root.model,
            clock: crate::budget::system_clock(),
            resume_checkpoint: root.suspension.budget_used.tree.as_ref(),
            child_execution: None,
        },
    )
    .await?
    else {
        return Err(AgentResumeError::Unavailable);
    };
    let root_execution = actor
        .personal_execution()
        .cloned()
        .ok_or(AgentResumeError::Unavailable)?;
    let mut executions = vec![root_execution.clone()];
    for level in claimed.levels.iter().skip(1) {
        let parent = executions.last().ok_or(AgentResumeError::Unavailable)?;
        let link = level.link.as_ref().ok_or(AgentResumeError::Unavailable)?;
        let dot = agents_db::get_personal_dot(&state.db, &claimed.owner, &level.binding.dot_id)
            .await?
            .filter(|dot| dot.active && dot.agent.principal.disabled_at.is_none())
            .ok_or(AgentResumeError::Unavailable)?;
        let spec: super::spec::AgentSpec = serde_json::from_str(&dot.agent.draft_spec)
            .map_err(|_| AgentResumeError::Unavailable)?;
        let chain = Arc::new(
            parent
                .chain()
                .enter(Frame {
                    principal_id: dot.agent.principal.id.clone(),
                    name: dot.name,
                    version: None,
                    via: Some(CallSite {
                        turn_id: link.parent_turn_id.clone(),
                        tool_call_id: link.parent_tool_call_id.clone(),
                    }),
                })
                .map_err(|_| AgentResumeError::Unavailable)?,
        );
        let saved = level
            .suspension
            .budget_used
            .tree
            .as_ref()
            .ok_or(AgentResumeError::Unavailable)?;
        let local = spec
            .main
            .budget
            .budget()
            .intersect(crate::budget::Budget::from_limits(saved.local_limits));
        executions.push(Arc::new(
            PersonalDotExecution::new(
                state.clone(),
                PersonalExecutionBinding {
                    identity: PersonalDotIdentity {
                        owner_user_id: claimed.owner.clone(),
                        dot_id: level.binding.dot_id.clone(),
                    },
                    session_id: level.binding.session_id.clone(),
                    assistant_turn_id: level.suspension.turn_id.clone(),
                    model: level.model.clone(),
                },
                chain,
                local,
                root_execution.meter().clone(),
            )
            .map_err(|_| AgentResumeError::Unavailable)?,
        ));
    }
    let mut child_result = None;
    for index in (1..claimed.levels.len()).rev() {
        let level = &claimed.levels[index];
        let execution = executions[index].clone();
        let actor = Actor::personal_dot(&claimed.owner, user.roles.clone(), &level.binding.dot_id)
            .with_personal_execution(execution)
            .map_err(|_| AgentResumeError::Unavailable)?;
        let mut suspension = level.suspension.clone();
        suspension.budget_used.tree = Some(
            root_execution
                .meter()
                .snapshot(executions[index].local_budget()),
        );
        let resume = ResumeFrom {
            suspension,
            decision: claimed.decision.clone(),
            child_result: child_result.take(),
        };
        drive_resumed(
            state,
            crate::server::headless::DriveParams {
                actor,
                session_id: level.binding.session_id.clone(),
                assistant_turn_id: level.suspension.turn_id.clone(),
                model: level.model.clone(),
                source,
                history_limit,
            },
            resume,
            claimed.workers[index].take(),
        )
        .await;
        let parent = &claimed.levels[index - 1];
        let parent_actor =
            Actor::personal_dot(&claimed.owner, user.roles.clone(), &parent.binding.dot_id)
                .with_personal_execution(executions[index - 1].clone())
                .map_err(|_| AgentResumeError::Unavailable)?;
        let mut ctx = crate::openai_driver::build_tool_context(
            state,
            crate::openai_driver::TurnFacts {
                actor: parent_actor,
                session_id: parent.binding.session_id.clone(),
                assistant_turn_id: parent.suspension.turn_id.clone(),
                model: Some(parent.model.clone()),
                client_ip: None,
                chat_feedback: None,
                pool_access: None,
                suspendable: true,
            },
        );
        ctx.call_id = Some(parent.suspension.tool_call.id.clone());
        child_result = Some(
            super::router::local::personal_result(&ctx, &level.suspension.turn_id)
                .await
                .map_err(|_| AgentResumeError::Unavailable)?,
        );
    }
    let mut suspension = claimed.levels.remove(0).suspension;
    suspension.budget_used.tree = Some(
        root_execution
            .meter()
            .snapshot(root_execution.local_budget()),
    );
    Ok(ResumeFrom {
        suspension,
        decision: claimed.decision,
        child_result,
    })
}

pub async fn cancel_personal_tree(
    state: &Arc<RamaState>,
    owner: &str,
    root_turn: &str,
) -> Result<(), DbError> {
    let root_session: Option<String> = sqlx::query_scalar(
        "SELECT s.id FROM chat_sessions s JOIN chat_turns t ON t.session_id = s.id
         WHERE t.id = ? AND t.role = 'assistant' AND s.user_id = ? AND s.principal_id IS NULL",
    )
    .bind(root_turn)
    .bind(owner)
    .fetch_optional(&state.db)
    .await?;
    let Some(root_session) = root_session else {
        return Ok(());
    };
    let mut pending = vec![(root_session, root_turn.to_owned(), 0usize)];
    let mut turns = Vec::new();
    while let Some((session, turn, depth)) = pending.pop() {
        if depth < MAX_DEPTH {
            for child in
                aiplane_agents::db::child_sessions::child_turns(&state.db, owner, &turn).await?
            {
                pending.push((child.session.session_id, child.assistant_turn.id, depth + 1));
            }
        }
        turns.push((session, turn));
    }
    for (session, turn) in turns.into_iter().rev() {
        state.chats.cancel_turn(owner, &session, &turn);
        if !chat::cancel_suspended_turn(&state.db, &turn).await?
            && let Some(current) = chat::get_turn(&state.db, &session, &turn).await?
            && current.status == TurnStatus::InProgress
            && !state
                .chats
                .get(owner, &session)
                .is_some_and(|worker| worker.turn_id == turn)
        {
            chat::finalize_turn(&state.db, &turn, TurnStatus::Cancelled, None).await?;
        }
        crate::server::background::reconcile_turn(&state.db, owner, &turn).await?;
    }
    Ok(())
}

pub async fn cancel_personal_runs_for_dot(
    state: &Arc<RamaState>,
    owner: &str,
    dot_id: &str,
) -> Result<(), DbError> {
    for turn in
        aiplane_agents::db::child_sessions::active_roots_for_dot(&state.db, owner, dot_id).await?
    {
        cancel_personal_tree(state, owner, &turn).await?;
    }
    Ok(())
}

pub async fn cancel_personal_runs_for_edge(
    state: &Arc<RamaState>,
    owner: &str,
    caller: &str,
    receiver: &str,
    old_generation: &str,
) -> Result<(), DbError> {
    for turn in aiplane_agents::db::child_sessions::active_roots_for_edge(
        &state.db,
        owner,
        caller,
        receiver,
        old_generation,
    )
    .await?
    {
        cancel_personal_tree(state, owner, &turn).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiplane_agents::db::system_principals::NewPrincipal;
    use aiplane_core::server::upstreams::UpstreamRegistry;
    use session_core::db::{BudgetUsed, PendingCall, TimeoutFallback};

    async fn paused_chain() -> (RamaState, String, String) {
        let db = aiplane_core::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        let principal = agents_db::create(
            &db,
            &NewPrincipal {
                name: "claim-test",
                display: "Claim test",
                description: "",
            },
            "{}",
            "tester",
        )
        .await
        .unwrap()
        .unwrap()
        .principal;
        let root = run_sessions::create_principal_session(
            &db,
            &run_sessions::NewRunSession {
                principal_id: &principal.id,
                title: None,
                parent_turn_id: None,
                agent_version: None,
            },
        )
        .await
        .unwrap();
        chat::create_assistant_turn_in_progress(&db, &root.id, "parent", "m")
            .await
            .unwrap();
        let child = run_sessions::create_principal_session(
            &db,
            &run_sessions::NewRunSession {
                principal_id: &principal.id,
                title: None,
                parent_turn_id: Some("parent"),
                agent_version: None,
            },
        )
        .await
        .unwrap();
        chat::create_assistant_turn_in_progress(&db, &child.id, "child", "m")
            .await
            .unwrap();
        for (turn, child_turn) in [("child", None), ("parent", Some("child".into()))] {
            let now = jiff::Timestamp::now();
            chat::suspend_turn(
                &db,
                &TurnSuspension {
                    turn_id: turn.into(),
                    request_id: format!("req-{turn}"),
                    kind: session_core::db::SuspensionKind::Approval,
                    message: None,
                    tool_call: PendingCall {
                        execution_facts: None,
                        id: format!("call-{turn}"),
                        name: "company_echo".into(),
                        arguments: "{}".into(),
                        actions: None,
                    },
                    tail: Vec::new(),
                    budget_used: BudgetUsed::default(),
                    child_turn,
                    on_timeout: TimeoutFallback::Deny,
                    expires_at: now,
                    created_at: now,
                    run_context: None,
                },
            )
            .await
            .unwrap();
        }
        aiplane_core::server::db::gateway_groups::upsert_group(&db, "test-admin", "", true, true)
            .await
            .unwrap();
        let now = jiff::Timestamp::now();
        aiplane_core::server::db::users::upsert(
            &db,
            &aiplane_core::server::db::users::User {
                id: "tester".into(),
                email: "tester@example.com".into(),
                name: None,
                roles: vec![],
                created_at: now,
                updated_at: now,
                timezone: None,
                speech_voice: None,
            },
        )
        .await
        .unwrap();
        let app = crate::server::AppState::new(
            Default::default(),
            db.clone(),
            UpstreamRegistry::new(&Default::default()).unwrap(),
            Arc::new(crate::server::tools::ToolRegistry::new()),
            Arc::new(crate::server::tools::echo::granted_to_everyone()),
        );
        let state = RamaState::new(
            app,
            aiplane_core::rama_server::SessionStore::new(db, [7; 32]),
            aiplane_core::server::usage::UsageHandle::disabled(),
        );
        (state, principal.id, root.id)
    }

    async fn resume_chain(
        state: &RamaState,
        principal: &str,
        session: &str,
        decision: Decision,
    ) -> Result<ClaimedResume, AgentResumeError> {
        claim(
            state,
            AgentResume {
                agent_id: principal,
                session_id: session,
                turn_id: "parent",
                request_id: Some("req-parent"),
                decision,
                by: ResumedBy::Staff {
                    user_id: "tester".into(),
                },
            },
        )
        .await
    }

    #[tokio::test]
    async fn revoked_staff_policy_preserves_all_pauses_until_current_access_is_restored() {
        let (state, principal, session) = paused_chain().await;
        agents_db::set_share(
            &state.db,
            &principal,
            agents_db::SubjectKind::User,
            "tester",
            agents_db::Access::Write,
            "tester",
        )
        .await
        .unwrap();
        assert_eq!(
            crate::agents::access::effective_access(&state, &principal, "tester")
                .await
                .unwrap(),
            Some(agents_db::Access::Write)
        );
        let mut tx = aiplane_core::server::db::WriteTx::begin(&state.db)
            .await
            .unwrap();
        sqlx::query("UPDATE gateway_groups SET is_admin = 0, can_manage_agents = 0 WHERE name = 'test-admin'")
            .execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        assert!(
            resume_chain(&state, &principal, &session, Decision::AllowOnce)
                .await
                .is_err()
        );
        for turn in ["parent", "child"] {
            assert_eq!(
                chat::get_suspension(&state.db, turn)
                    .await
                    .unwrap()
                    .unwrap()
                    .request_id,
                format!("req-{turn}")
            );
        }
        aiplane_core::server::db::gateway_groups::set_can_manage_agents(
            &state.db,
            "test-admin",
            true,
        )
        .await
        .unwrap();
        let claimed = resume_chain(&state, &principal, &session, Decision::AllowOnce)
            .await
            .unwrap();
        assert_eq!(claimed.levels.len(), 2);
        for turn in ["parent", "child"] {
            assert!(
                chat::get_suspension(&state.db, turn)
                    .await
                    .unwrap()
                    .is_none()
            );
        }
    }

    #[tokio::test]
    async fn a_missing_ancestor_during_claim_preserves_the_entire_chain() {
        let (state, principal, session) = paused_chain().await;
        sqlx::query("CREATE TRIGGER remove_parent AFTER DELETE ON chat_turn_suspensions WHEN OLD.turn_id = 'child' BEGIN DELETE FROM chat_turn_suspensions WHERE turn_id = 'parent'; END")
            .execute(&state.db).await.unwrap();
        assert!(
            resume_chain(&state, &principal, &session, Decision::AllowOnce)
                .await
                .is_err()
        );
        for turn in ["parent", "child"] {
            assert!(
                chat::get_suspension(&state.db, turn)
                    .await
                    .unwrap()
                    .is_some()
            );
        }
    }

    #[tokio::test]
    async fn a_changed_ancestor_request_during_claim_preserves_the_entire_chain() {
        let (state, principal, session) = paused_chain().await;
        sqlx::query("CREATE TRIGGER change_parent AFTER DELETE ON chat_turn_suspensions WHEN OLD.turn_id = 'child' BEGIN UPDATE chat_turn_suspensions SET request_id = 'new-parent' WHERE turn_id = 'parent'; END")
            .execute(&state.db).await.unwrap();
        assert!(
            resume_chain(&state, &principal, &session, Decision::AllowOnce)
                .await
                .is_err()
        );
        for turn in ["parent", "child"] {
            assert_eq!(
                chat::get_suspension(&state.db, turn)
                    .await
                    .unwrap()
                    .unwrap()
                    .request_id,
                format!("req-{turn}")
            );
        }
    }

    #[tokio::test]
    async fn an_unoffered_deepest_decision_preserves_the_entire_chain() {
        let (state, principal, session) = paused_chain().await;
        assert!(matches!(
            resume_chain(
                &state,
                &principal,
                &session,
                Decision::Value { value: "x".into() }
            )
            .await,
            Err(AgentResumeError::Refused(ResumeRefused::NotOffered { .. }))
        ));
        for turn in ["parent", "child"] {
            assert!(
                chat::get_suspension(&state.db, turn)
                    .await
                    .unwrap()
                    .is_some()
            );
        }
    }

    #[tokio::test]
    async fn a_successful_chain_claim_consumes_all_exact_requests() {
        let (state, principal, session) = paused_chain().await;
        let claimed = resume_chain(&state, &principal, &session, Decision::AllowOnce)
            .await
            .unwrap();
        assert_eq!(claimed.levels.len(), 2);
        for level in claimed.levels {
            assert!(
                chat::get_suspension(&state.db, &level.suspension.turn_id)
                    .await
                    .unwrap()
                    .is_none()
            );
            assert_eq!(
                chat::get_turn(&state.db, &level.session.id, &level.suspension.turn_id)
                    .await
                    .unwrap()
                    .unwrap()
                    .status,
                TurnStatus::InProgress
            );
        }
    }
}
