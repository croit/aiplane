// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The runtime's door to the activity log (`aiplane_agents::db::agent_audit`,
//! `docs/agent-activity-log.md`). Every run event goes through
//! [`record_event`]: one event, written synchronously in its own write
//! transaction before the run moves on, so the log's order is the run's order
//! and nothing waits in memory to be lost.
//!
//! **Bound.** A write may take at most [`WRITE_BOUND`]: the lock wait (SQLite
//! serialises writers) and the write itself. Past it the event counts as not
//! written.
//!
//! **Fail closed.** An event of an agent run that could not be written marks
//! the run ([`AgentRun::mark_log_failed`]); the driver stops it at the next
//! step — before the next model round, before tools run, and when the turn
//! would end — and errors the turn, so nothing happens in a run that its log
//! does not show. Outside a run (a management change is written on the
//! change's own transaction instead; a refused visitor; a sweep) the failure
//! is logged at `error` with the event's correlation ids.

use std::sync::Arc;
use std::time::Duration;

use aiplane_agents::db::agent_audit::{self, AuditKind, Correlation, NewEvent, Redaction};
use aiplane_core::server::db::Pool;
use aiplane_core::server::principal::SystemPrincipal;
use aiplane_core::server::run_chain::{Frame, RunChain};
use serde_json::{Value, json};

use crate::agent_run::AgentRun;
use crate::server::side_call::SideExchange;
use crate::server::tools::ToolContext;
use crate::suspend::Suspend;

/// The longest a run waits for one event to be written.
pub const WRITE_BOUND: Duration = Duration::from_secs(5);

/// Why an event did not reach the log.
#[derive(Debug, thiserror::Error)]
pub enum LogError {
    #[error(
        "writing to the agent activity log took longer than {}s; the database is busy or the \
         disk is slow — check the server's disk and database lock contention",
        WRITE_BOUND.as_secs()
    )]
    TimedOut,
    #[error("writing to the agent activity log failed: {0}")]
    Db(#[from] aiplane_core::server::db::DbError),
}

/// What a turn stopped by [`LogError`] tells its participant: the run is
/// stopped rather than continued unrecorded.
pub const LOG_UNAVAILABLE: &str = "The agent's activity log could not be written, so the run was \
                                   stopped before doing anything it could not record. Try again \
                                   shortly; if it keeps happening, the gateway's database needs \
                                   attention.";

/// Append one event, bounded by [`WRITE_BOUND`], and return its id. A
/// failure is logged here, once, with the event's correlation ids.
pub async fn record_event(db: &Pool, event: NewEvent<'_>) -> Result<String, LogError> {
    let kind = event.kind.as_str();
    let principal = event.principal_id.to_string();
    let session = event.at.session_id.clone();
    let turn = event.at.turn_id.clone();
    let written = match tokio::time::timeout(WRITE_BOUND, agent_audit::append_now(db, event)).await
    {
        Ok(Ok(appended)) => return Ok(appended.id),
        Ok(Err(err)) => LogError::Db(err),
        Err(_) => LogError::TimedOut,
    };
    tracing::error!(
        error = %written,
        kind,
        principal = %principal,
        session = ?session,
        turn = ?turn,
        "an agent activity event could not be written"
    );
    Err(written)
}

/// One event of `principal_id` outside a tool context; `actor_id` names the
/// person who caused it, when one did. A failure is logged and marks `run`
/// when there is one.
pub async fn record(
    db: &Pool,
    kind: AuditKind,
    principal_id: &str,
    actor_id: Option<&str>,
    chain: Option<&RunChain>,
    detail: Value,
) {
    let _ = record_event(
        db,
        NewEvent::new(kind, principal_id, detail)
            .by(actor_id)
            .in_run(chain),
    )
    .await;
}

/// [`record_event`] for an event of `run`, which is stopped when the event
/// cannot be written; the event's id when it was.
pub(crate) async fn record_for_run(
    db: &Pool,
    run: Option<&AgentRun>,
    event: NewEvent<'_>,
) -> Option<String> {
    let recorded = record_event(db, event).await;
    if recorded.is_err()
        && let Some(run) = run
    {
        run.mark_log_failed();
    }
    recorded.ok()
}

/// Anchor conversation `conversation_id`'s chain head in agent `agent_id`'s
/// own chain, bounded like any event. Called when a turn of the conversation
/// ends; a failure is logged, and the conversation's newest events stay
/// unanchored until the next turn's anchor.
pub async fn anchor(db: &Pool, agent_id: &str, conversation_id: &str) {
    let anchored = tokio::time::timeout(
        WRITE_BOUND,
        agent_audit::anchor_conversation(db, agent_id, conversation_id),
    )
    .await;
    let error = match anchored {
        Ok(Ok(_)) => return,
        Ok(Err(err)) => LogError::Db(err),
        Err(_) => LogError::TimedOut,
    };
    tracing::error!(
        error = %error,
        agent = agent_id,
        conversation = conversation_id,
        "a conversation's activity chain could not be anchored"
    );
}

/// The run a [`SideExchange`] — a model call made on an agent's behalf
/// outside the round loop: the compaction summary, the topic guard, the
/// route classifier, the rubric judge, a voice call — is recorded in, as an
/// `llm_exchange` whose `purpose` says which: its principal, call chain and
/// where in it the call happened. Only an agent's conversation has one, so
/// a person's compaction records nothing.
#[derive(Clone)]
pub struct RunLog {
    principal_id: String,
    chain: Arc<RunChain>,
    at: Correlation,
    redaction: Redaction,
    /// The live run, stopped when its exchange cannot be written.
    run: Option<Arc<AgentRun>>,
}

impl RunLog {
    /// The run of the call `ctx` belongs to; `None` for a person's turn.
    pub fn of(ctx: &ToolContext) -> Option<Self> {
        let run = ctx.agent.as_ref()?;
        Some(Self {
            principal_id: run.system_principal().id.clone(),
            chain: run.chain().clone(),
            at: ctx.correlation(),
            redaction: ctx.redaction(),
            run: Some(run.clone()),
        })
    }

    /// A conversation of `principal` at `version` that no run is driving
    /// right now — an evaluation case's, once its script ran.
    pub fn conversation(principal: &SystemPrincipal, version: i64, conversation: &str) -> Self {
        Self {
            principal_id: principal.id.clone(),
            chain: Arc::new(RunChain::root(
                conversation,
                None,
                Frame::for_principal(principal, Some(version)),
            )),
            at: Correlation {
                session_id: Some(conversation.to_string()),
                ..Correlation::default()
            },
            redaction: Redaction::default(),
            run: None,
        }
    }

    /// Visitor `visitor_id`'s conversation of `principal` at `version`,
    /// between turns: a voice call it made, about `turn_id` when it names one.
    pub fn visitor(
        principal: &SystemPrincipal,
        version: i64,
        conversation: &str,
        visitor_id: &str,
        turn_id: Option<&str>,
    ) -> Self {
        Self {
            chain: Arc::new(RunChain::root(
                conversation,
                Some(visitor_id.to_string()),
                Frame::for_principal(principal, Some(version)),
            )),
            at: Correlation {
                turn_id: turn_id.map(str::to_string),
                ..Self::conversation(principal, version, conversation).at
            },
            ..Self::conversation(principal, version, conversation)
        }
    }

    /// The call chain an exchange of this log belongs to, which its usage
    /// row joins.
    pub fn chain(&self) -> Arc<RunChain> {
        self.chain.clone()
    }

    pub async fn record(&self, db: &Pool, exchange: &SideExchange) {
        let latency = exchange.latency_ms();
        let mut detail = json!({
            "purpose": exchange.purpose,
            "model": exchange.model,
            "backend": exchange.backend,
            "request": exchange.request,
            "response": { "status": exchange.status, "body": exchange.response },
            "latency_ms": latency,
        });
        if let Some(answer) = &exchange.answer {
            detail["answer"] = answer.clone();
        }
        if let Some(error) = &exchange.error {
            detail["error"] = json!(error);
        }
        let mut event = NewEvent::new(AuditKind::LlmExchange, &self.principal_id, detail)
            .in_run(Some(&self.chain))
            .at(self.at.clone())
            .redacted(self.redaction.clone());
        event.duration_ms = Some(latency);
        record_for_run(db, self.run.as_deref(), event).await;
    }
}

impl ToolContext {
    /// What this call's events leave out: its run's redaction, and the
    /// secure input the call is running again with, if any
    /// ([`Redaction::decided`]).
    pub(crate) fn redaction(&self) -> Redaction {
        let decided = match &self.suspend {
            Suspend::Decided(kind, decision) => Redaction::decided(*kind, decision),
            Suspend::Available | Suspend::Unavailable => Redaction::default(),
        };
        match self.agent.as_deref() {
            Some(run) => decided.and(&run.redaction()),
            None => decided,
        }
    }

    /// Where this call sits in its run: its conversation, turn, the round
    /// the driver is in, and the tool call.
    pub(crate) fn correlation(&self) -> Correlation {
        Correlation {
            session_id: self.session_id.clone(),
            turn_id: self.assistant_turn_id.clone(),
            round: self.agent.as_deref().map(AgentRun::round),
            call_id: self.call_id.clone(),
        }
    }

    /// One run event of this call's principal, with its call chain and
    /// correlation ids, and its conversation and turn added to `detail`
    /// unless it names them already. The run stops when it cannot be
    /// written.
    pub(crate) async fn audit(&self, kind: AuditKind, mut detail: Value) {
        if let Value::Object(map) = &mut detail {
            map.entry("session_id")
                .or_insert_with(|| json!(self.session_id));
            map.entry("turn_id")
                .or_insert_with(|| json!(self.assistant_turn_id));
        }
        self.audit_event(kind, None, detail).await;
    }

    /// [`Self::audit`] without touching `detail`, with how long the thing
    /// recorded took; the event's id once written.
    pub(crate) async fn audit_event(
        &self,
        kind: AuditKind,
        duration_ms: Option<u64>,
        detail: Value,
    ) -> Option<String> {
        let mut event = NewEvent::new(kind, self.principal.subject_id(), detail)
            .in_run(self.chain())
            .at(self.correlation())
            .redacted(self.redaction());
        if let Some(dot) = self.personal_dot.as_ref() {
            event = event.by(Some(&dot.owner_user_id)).for_agent(&dot.dot_id);
        }
        event.duration_ms = duration_ms;
        record_for_run(&self.db, self.agent.as_deref(), event).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use session_core::db::{Decision, DenyReason, SuspensionKind};

    /// The driver, the resumed call, the verifier and the runner all take
    /// what to withhold from one rule (`Redaction::decided`): a secure
    /// input's value, and nothing of any other decision.
    #[tokio::test]
    async fn every_path_withholds_a_secure_input_and_nothing_else() {
        let pool = aiplane_core::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        let value = json!("4711");
        for kind in [
            SuspensionKind::Approval,
            SuspensionKind::SecureInput,
            SuspensionKind::HumanAnswer,
        ] {
            for decision in [
                Decision::AllowOnce,
                Decision::Deny {
                    reason: DenyReason::User,
                },
                Decision::Value {
                    value: value.clone(),
                },
            ] {
                let expected = matches!(
                    (kind, &decision),
                    (SuspensionKind::SecureInput, Decision::Value { .. })
                )
                .then_some(&value);
                assert_eq!(
                    Redaction::decided(kind, &decision).secret.as_ref(),
                    expected
                );
                let ctx = ToolContext {
                    suspend: Suspend::Decided(kind, decision.clone()),
                    ..ToolContext::for_test(pool.clone())
                };
                assert_eq!(ctx.redaction().secret.as_ref(), expected, "{kind:?}");
            }
        }
        let ctx = ToolContext::for_test(pool);
        assert_eq!(ctx.redaction().secret, None);
    }

    /// A writer hands the log the raw call: the run's redaction rides along
    /// on every event its context writes, and the log applies it, so a
    /// `tool_result` written outside the runner (a resume's denial) keeps a
    /// sensitive tool's arguments out as well as the runner's own does.
    #[tokio::test]
    async fn every_event_of_a_run_carries_its_redaction_to_the_log() {
        use aiplane_agents::db::agent_audit::redaction::redacted_arguments;
        use aiplane_core::server::principal::{GrantSet, SystemPrincipal};
        let pool = aiplane_core::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        let principal = SystemPrincipal {
            id: "p1".into(),
            name: "support".into(),
            grants: Arc::new(GrantSet::default()),
        };
        let chain = Arc::new(RunChain::root(
            "s1",
            None,
            Frame::for_principal(&principal, None),
        ));
        let run = Arc::new(AgentRun::new(principal.clone(), chain).unwrap());
        run.note_sensitive("secretive");
        run.redact(&Redaction::decided(
            SuspensionKind::SecureInput,
            &Decision::Value {
                value: json!("code-4711"),
            },
        ));
        let ctx = ToolContext {
            principal: run.principal(),
            agent: Some(run),
            session_id: Some("s1".into()),
            ..ToolContext::for_test(pool.clone())
        };
        ctx.audit_event(
            AuditKind::ToolResult,
            None,
            json!({
                "tool": "secretive",
                "arguments": { "pin": "1234" },
                "status": "denied",
                "result": { "said": "code-4711" },
            }),
        )
        .await
        .unwrap();
        let detail: String = sqlx::query_scalar("SELECT detail FROM agent_audit")
            .fetch_one(&pool)
            .await
            .unwrap();
        let detail: Value = serde_json::from_str(&detail).unwrap();
        assert_eq!(detail["arguments"], redacted_arguments());
        assert!(!detail.to_string().contains("code-4711"), "{detail}");
    }
}
