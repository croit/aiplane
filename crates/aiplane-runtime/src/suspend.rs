// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Durable suspend and resume of a turn (`docs/agent-hil.md` → "Suspend and resume").
//!
//! A tool that needs a decision from outside the model — an approval, a value
//! only the user may type, a human's answer — returns [`tool_suspend`] instead
//! of a result. The chat driver then writes the run state at that point to
//! `chat_turn_suspensions`, flips the turn to `suspended`, and frees the
//! worker. Nothing waits in memory, so the pause survives a restart.
//!
//! A resume claims the row ([`claim_for_resume`]) and drives the same turn
//! again with an [`OpenAiDriver`](crate::openai_driver::OpenAiDriver) whose
//! `resume` is set. The driver rebuilds the replayed history as for any turn,
//! appends the stored tail, and settles the waiting call:
//!
//! - **deny** (by the user, or by a timeout whose fallback is deny) answers
//!   the call with a tool error the model reads; the tool never runs;
//! - **allow once** and **value** run the same call again, with
//!   [`ToolContext::suspend`](crate::server::tools::ToolContext) set to
//!   [`Suspend::Decided`], and its result is the call's result.
//!
//! Then the round loop continues where it stopped, against the rounds already
//! spent.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use session_core::db::{
    self as chat, Decision, DecisionKind, Pool, SuspensionKind, TimeoutFallback, TurnSuspension,
};

/// Sentinel key of the suspend envelope. Same mechanism as
/// [`TOOL_CONTENT_PARTS_KEY`](crate::server::tools::TOOL_CONTENT_PARTS_KEY):
/// the driver recognises a body whose only key is this one.
pub const SUSPEND_KEY: &str = "__gateway_suspend";

/// What a tool asks for when it suspends its call.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SuspendRequest {
    pub kind: SuspensionKind,
    /// Shown next to the decision. Data the tool supplies, not an app string:
    /// an approval leaves it empty and the client words the question itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// How long the decision may take before [`Self::on_timeout`] applies.
    pub timeout_secs: u64,
    #[serde(default)]
    pub on_timeout: TimeoutFallback,
    /// Set by `forward_request` when the sub-agent run it dispatched paused:
    /// this call waits on that run's pause, which a resume settles first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child: Option<ChildPause>,
    /// What the inbox shows next to the request, stored with the pause
    /// (`run_context`): a handoff's question, the visitor's last message and
    /// the slots the model may see. Never anything a resume relies on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actions: Option<Vec<String>>,
}

/// The paused sub-agent run a call is waiting on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChildPause {
    /// The child run's assistant turn, whose session names the waiting turn
    /// as its parent.
    pub turn_id: String,
    /// The child's deadline; the parent waits exactly as long.
    pub expires_at: jiff::Timestamp,
}

impl SuspendRequest {
    /// Ask whether this call may run, denying it if nobody answers in time.
    pub fn approval(timeout: Duration) -> Self {
        Self {
            kind: SuspensionKind::Approval,
            message: None,
            timeout_secs: timeout.as_secs(),
            on_timeout: TimeoutFallback::Deny,
            actions: None,
            child: None,
            context: None,
        }
    }

    /// Ask a person other than the one chatting to answer `question`, with
    /// `context` shown next to it in the inbox. Denied if nobody answers in
    /// time.
    pub fn human_answer(question: impl Into<String>, timeout: Duration, context: Value) -> Self {
        Self {
            kind: SuspensionKind::HumanAnswer,
            message: Some(question.into()),
            timeout_secs: timeout.as_secs(),
            on_timeout: TimeoutFallback::Deny,
            actions: None,
            child: None,
            context: Some(context),
        }
    }

    /// Ask the one chatting for a value the model must never see, such as a
    /// verification code, with `message` shown next to the field. Denied if
    /// nobody answers in time.
    pub fn secure_input(message: impl Into<String>, timeout: Duration) -> Self {
        Self {
            kind: SuspensionKind::SecureInput,
            message: Some(message.into()),
            timeout_secs: timeout.as_secs(),
            on_timeout: TimeoutFallback::Deny,
            actions: None,
            child: None,
            context: None,
        }
    }
}

/// The tool result that suspends the call instead of answering it.
pub fn tool_suspend(request: SuspendRequest) -> Value {
    serde_json::json!({ SUSPEND_KEY: request })
}

/// The suspend request inside a tool result, if the tool returned one.
pub fn extract_suspend(body: &Value) -> Option<SuspendRequest> {
    body.as_object()
        .filter(|m| m.len() == 1)
        .and_then(|m| m.get(SUSPEND_KEY))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
}

/// Whether a tool may suspend its call, and what was decided when it is being
/// run again after one.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Suspend {
    /// The caller cannot resume a pause. A tool that needs a decision must
    /// refuse rather than proceed.
    #[default]
    Unavailable,
    /// The chat path and agent runs: returning [`tool_suspend`] pauses the
    /// turn.
    Available,
    /// The call is running again because of this decision on a pause of
    /// this kind. Never `Deny`: a denied call is answered by the driver
    /// without running the tool. The kind decides what the log withholds
    /// (`agents::audit::Redaction`).
    Decided(SuspensionKind, Decision),
}

/// A claimed suspension and the decision that settles it — what an
/// [`OpenAiDriver`](crate::openai_driver::OpenAiDriver) resumes from.
#[derive(Clone, Debug)]
pub struct ResumeFrom {
    pub suspension: TurnSuspension,
    pub decision: Decision,
    /// Set when the waiting call waited on a sub-agent run that was resumed
    /// first: its result is the call's result, and the call does not run
    /// again.
    pub child_result: Option<Value>,
}

/// Why a resume was refused. Nothing was claimed in any of these cases.
#[derive(Debug, thiserror::Error)]
pub enum ResumeRefused {
    #[error("this turn is not waiting for a decision")]
    NotSuspended,
    #[error(
        "this decision answers an earlier request; the turn is now waiting for request \
         `{current}` — reload the conversation and answer that one"
    )]
    StaleRequest { current: String },
    #[error(
        "a `{kind}` request cannot be answered with `{decision:?}`; it accepts {offered:?}",
        kind = kind.as_str()
    )]
    NotOffered {
        kind: SuspensionKind,
        decision: DecisionKind,
        offered: &'static [DecisionKind],
    },
    #[error("reading or claiming the suspension: {0}")]
    Storage(#[from] chat::DbError),
}

/// Validate `decision` against the turn's suspension and claim it. On `Ok`
/// the row is gone and the turn is `in_progress` again: the caller must now
/// drive it, or the turn is left with no worker.
///
/// `request_id`, when given, must name the current suspension, so an answer
/// to a pause the turn has since moved past cannot settle the next one.
pub async fn claim_for_resume(
    db: &Pool,
    turn_id: &str,
    request_id: Option<&str>,
    decision: Decision,
) -> Result<ResumeFrom, ResumeRefused> {
    let mut tx = db
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(chat::DbError::from)?;
    let resumed = claim_for_resume_in(&mut tx, turn_id, request_id, decision).await?;
    tx.commit().await.map_err(chat::DbError::from)?;
    Ok(resumed)
}

pub async fn claim_for_resume_in(
    connection: &mut sqlx::SqliteConnection,
    turn_id: &str,
    request_id: Option<&str>,
    decision: Decision,
) -> Result<ResumeFrom, ResumeRefused> {
    let current = chat::suspensions::get_suspension_in(connection, turn_id)
        .await?
        .ok_or(ResumeRefused::NotSuspended)?;
    if let Some(asked) = request_id
        && asked != current.request_id
    {
        return Err(ResumeRefused::StaleRequest {
            current: current.request_id,
        });
    }
    let offered = current.kind.options();
    if !offered.contains(&decision.kind()) {
        return Err(ResumeRefused::NotOffered {
            kind: current.kind,
            decision: decision.kind(),
            offered,
        });
    }
    let suspension =
        chat::suspensions::claim_suspension_in(connection, turn_id, &current.request_id)
            .await?
            .filter(|claimed| claimed.request_id == current.request_id)
            .ok_or(ResumeRefused::NotSuspended)?;
    Ok(ResumeFrom {
        suspension,
        decision,
        child_result: None,
    })
}

pub(crate) async fn claim_chain_in(
    connection: &mut sqlx::SqliteConnection,
    snapshots: &[TurnSuspension],
    decision: Decision,
) -> Result<Vec<TurnSuspension>, ResumeRefused> {
    let Some(leaf) = snapshots.last() else {
        return Err(ResumeRefused::NotSuspended);
    };
    for snapshot in snapshots {
        if chat::get_suspension_in(connection, &snapshot.turn_id)
            .await?
            .as_ref()
            != Some(snapshot)
        {
            return Err(ResumeRefused::NotSuspended);
        }
    }
    let deepest =
        claim_for_resume_in(connection, &leaf.turn_id, Some(&leaf.request_id), decision).await?;
    if deepest.suspension != *leaf {
        return Err(ResumeRefused::NotSuspended);
    }
    let mut claimed = vec![deepest.suspension];
    for snapshot in snapshots[..snapshots.len() - 1].iter().rev() {
        let row = chat::claim_suspension_in(connection, &snapshot.turn_id, &snapshot.request_id)
            .await?
            .filter(|row| row == snapshot)
            .ok_or(ResumeRefused::NotSuspended)?;
        claimed.push(row);
    }
    claimed.reverse();
    Ok(claimed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use session_core::db::{BudgetUsed, DenyReason, PendingCall, TurnStatus};

    #[test]
    fn the_envelope_round_trips_and_only_alone() {
        let request = SuspendRequest::approval(Duration::from_secs(600));
        let body = tool_suspend(request.clone());
        assert_eq!(extract_suspend(&body), Some(request));
        assert_eq!(extract_suspend(&serde_json::json!({"message": "hi"})), None);
        let mut crowded = body.as_object().unwrap().clone();
        crowded.insert("other".into(), Value::Null);
        assert_eq!(extract_suspend(&Value::Object(crowded)), None);
    }

    async fn suspended_turn() -> (Pool, String) {
        let db = aiplane_core::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        let now = jiff::Timestamp::now();
        aiplane_core::server::db::users::upsert(
            &db,
            &aiplane_core::server::db::users::User {
                id: "u1".into(),
                email: "u1@example.com".into(),
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
        let session = chat::create_session(&db, "u1").await.unwrap();
        chat::create_assistant_turn_in_progress(&db, &session.id, "t1", "m")
            .await
            .unwrap();
        chat::suspend_turn(
            &db,
            &TurnSuspension {
                turn_id: "t1".into(),
                request_id: "req-1".into(),
                kind: SuspensionKind::Approval,
                message: None,
                tool_call: PendingCall {
                    actions: None,
                    id: "call-1".into(),
                    name: "company_echo".into(),
                    arguments: "{}".into(),
                },
                tail: Vec::new(),
                budget_used: BudgetUsed::default(),
                child_turn: None,
                on_timeout: TimeoutFallback::Deny,
                expires_at: now,
                created_at: now,
                run_context: None,
            },
        )
        .await
        .unwrap();
        (db, session.id)
    }

    async fn still_suspended(db: &Pool) -> bool {
        chat::get_suspension(db, "t1").await.unwrap().is_some()
    }

    #[tokio::test]
    async fn a_decision_the_request_does_not_offer_is_refused_without_claiming() {
        let (db, _) = suspended_turn().await;
        let refused =
            claim_for_resume(&db, "t1", None, Decision::Value { value: "x".into() }).await;
        assert!(matches!(refused, Err(ResumeRefused::NotOffered { .. })));
        assert!(still_suspended(&db).await);
    }

    #[tokio::test]
    async fn an_answer_to_another_request_is_refused_without_claiming() {
        let (db, _) = suspended_turn().await;
        let refused = claim_for_resume(&db, "t1", Some("req-0"), Decision::AllowOnce).await;
        assert!(
            matches!(refused, Err(ResumeRefused::StaleRequest { ref current }) if current == "req-1")
        );
        assert!(still_suspended(&db).await);
    }

    #[tokio::test]
    async fn a_suspension_resumes_once() {
        let (db, session_id) = suspended_turn().await;
        let decision = Decision::Deny {
            reason: DenyReason::User,
        };
        let resumed = claim_for_resume(&db, "t1", Some("req-1"), decision.clone())
            .await
            .unwrap();
        assert_eq!(resumed.decision, decision);
        assert_eq!(resumed.suspension.tool_call.id, "call-1");
        assert!(!still_suspended(&db).await);
        let turn = chat::get_turn(&db, &session_id, "t1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(turn.status, TurnStatus::InProgress);
        assert!(matches!(
            claim_for_resume(&db, "t1", None, Decision::AllowOnce).await,
            Err(ResumeRefused::NotSuspended)
        ));
    }

    #[tokio::test]
    async fn a_connection_claim_rolls_back_with_its_callers_transaction() {
        let (db, session_id) = suspended_turn().await;
        let mut tx = db.begin_with("BEGIN IMMEDIATE").await.unwrap();
        let resumed = claim_for_resume_in(&mut tx, "t1", Some("req-1"), Decision::AllowOnce)
            .await
            .unwrap();
        assert_eq!(resumed.suspension.request_id, "req-1");
        tx.rollback().await.unwrap();
        assert!(still_suspended(&db).await);
        assert_eq!(
            chat::get_turn(&db, &session_id, "t1")
                .await
                .unwrap()
                .unwrap()
                .status,
            TurnStatus::Suspended
        );
    }

    #[tokio::test]
    async fn a_connection_claim_refuses_stale_and_unoffered_decisions_without_consuming() {
        let (db, _) = suspended_turn().await;
        let mut tx = db.begin_with("BEGIN IMMEDIATE").await.unwrap();
        assert!(matches!(
            claim_for_resume_in(&mut tx, "t1", Some("old"), Decision::AllowOnce).await,
            Err(ResumeRefused::StaleRequest { .. })
        ));
        assert!(matches!(
            claim_for_resume_in(
                &mut tx,
                "t1",
                Some("req-1"),
                Decision::Value { value: "x".into() }
            )
            .await,
            Err(ResumeRefused::NotOffered { .. })
        ));
        tx.commit().await.unwrap();
        assert!(still_suspended(&db).await);
    }
}
