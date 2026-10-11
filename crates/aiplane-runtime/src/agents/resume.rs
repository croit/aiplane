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
use super::profile::{AgentRunError, Role, RunOptions, RunProfile, SpecSource};
use super::router::{Caller, dispatch_result};
use super::run::{AgentReply, OpenedTurn, drive_opened, drive_opened_from, root_chain};
use crate::rama_server::state::RamaState;
use crate::server::headless::drive_resumed;
use crate::suspend::{ResumeFrom, ResumeRefused, claim_for_resume_in};

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
    match (&resume.by, kind.answered_by()) {
        (ResumedBy::Participant, Answerer::Staff) => {
            return Err(AgentResumeError::StaffOnly {
                kind: kind.as_str(),
            });
        }
        (ResumedBy::Staff { .. }, Answerer::Participant) if !testing => {
            return Err(AgentResumeError::ParticipantOnly {
                kind: kind.as_str(),
            });
        }
        _ => {}
    }

    let mut tx = aiplane_agents::db::WriteTx::begin(db).await?;
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
    let innermost = levels.len() - 1;
    let claimed = claim_for_resume_in(
        &mut tx,
        &levels[innermost].suspension.turn_id,
        Some(&levels[innermost].suspension.request_id),
        resume.decision.clone(),
    )
    .await?;
    levels[innermost].suspension = claimed.suspension;
    for i in (0..innermost).rev() {
        let row = chat::claim_suspension_in(
            &mut tx,
            &levels[i].suspension.turn_id,
            &levels[i].suspension.request_id,
        )
        .await?
        .filter(|row| row == &levels[i].suspension)
        .ok_or_else(not_suspended)?;
        levels[i].suspension = row;
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
        let source = source_of(state, &level.session).await?;
        let profile =
            RunProfile::load_from(state, &level.session.principal_id, source, role, options)
                .await?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use aiplane_agents::db::system_principals::{self, NewPrincipal};
    use aiplane_core::server::upstreams::UpstreamRegistry;
    use session_core::db::{BudgetUsed, PendingCall, TimeoutFallback};

    async fn paused_chain() -> (RamaState, String, String) {
        let db = aiplane_core::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        let principal = system_principals::create(
            &db,
            &NewPrincipal {
                name: "claim-test",
                display: "Claim test",
                description: "",
            },
            "tester",
        )
        .await
        .unwrap()
        .unwrap();
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
