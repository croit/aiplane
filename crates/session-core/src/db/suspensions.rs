// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Turns paused at a tool call that waits for a decision from outside the
//! model (`chat_turn_suspensions`, migration `0077_agent_builder.sql`).
//!
//! A suspension is the durable half of what `FeedbackHub` does in memory: the
//! run state at the pause point is written down, the turn's status becomes
//! [`TurnStatus::Suspended`], and the worker is freed. Resuming claims the row
//! (deleting it and flipping the turn back to `in_progress` in one
//! transaction) and hands it to the driver, which continues the same turn.

use super::*;

/// What a suspended call is waiting for. Decides which decisions are offered.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SuspensionKind {
    /// Run this call, or not.
    Approval,
    /// A value only the user may type, which never passes through the model.
    SecureInput,
    /// An answer from a human other than the one chatting.
    HumanAnswer,
}

impl SuspensionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Approval => "approval",
            Self::SecureInput => "secure_input",
            Self::HumanAnswer => "human_answer",
        }
    }

    pub fn parse(s: &str) -> Result<Self, DbError> {
        match s {
            "approval" => Ok(Self::Approval),
            "secure_input" => Ok(Self::SecureInput),
            "human_answer" => Ok(Self::HumanAnswer),
            _ => Err(DbError::Decode {
                column: "kind",
                source: anyhow::anyhow!("unknown suspension kind `{s}`"),
            }),
        }
    }

    /// The decisions a resume may carry for this kind. Deny is always one of
    /// them: whoever is asked can always refuse.
    pub fn options(self) -> &'static [DecisionKind] {
        match self {
            Self::Approval => &[DecisionKind::AllowOnce, DecisionKind::Deny],
            Self::SecureInput | Self::HumanAnswer => &[DecisionKind::Value, DecisionKind::Deny],
        }
    }

    /// Who may answer this kind in a conversation the participant does not
    /// own, where someone acting for the owner answers what is not the
    /// participant's to decide. In a person's own chat the owner is both.
    pub fn answered_by(self) -> Answerer {
        match self {
            Self::SecureInput => Answerer::Participant,
            Self::Approval | Self::HumanAnswer => Answerer::Staff,
        }
    }

    /// The fallback an expired request of this kind takes, given the one the
    /// tool asked for. Only a decision this kind offers can stand in for
    /// silence, and an approval always falls back to deny: nobody approving
    /// is not an approval.
    pub fn timeout_fallback(self, asked: TimeoutFallback) -> TimeoutFallback {
        let offered = self.options().contains(&asked.decision().kind());
        if self == Self::Approval || !offered {
            TimeoutFallback::Deny
        } else {
            asked
        }
    }
}

/// Who answers a suspension in a conversation the participant does not own.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Answerer {
    /// The one chatting. A secure input is theirs, and goes nowhere but the
    /// tool that asked for it.
    Participant,
    /// Someone acting for the conversation's owner. Approvals and human
    /// answers are theirs, never the participant's.
    Staff,
}

/// The shape of a decision, without its payload — what a client is offered.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DecisionKind {
    AllowOnce,
    Deny,
    Value,
}

/// Why a call was denied. The model is told which, because "the user said no"
/// and "nobody answered in time" call for different next steps.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DenyReason {
    User,
    Timeout,
}

/// What a resume carries into the waiting call.
///
/// `Debug` never prints a value: a `secure_input` value must not reach a log
/// line through `{:?}`.
#[derive(Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum Decision {
    AllowOnce,
    Deny { reason: DenyReason },
    Value { value: serde_json::Value },
}

impl std::fmt::Debug for Decision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AllowOnce => f.write_str("AllowOnce"),
            Self::Deny { reason } => f.debug_struct("Deny").field("reason", reason).finish(),
            Self::Value { .. } => f.write_str("Value { value: <redacted> }"),
        }
    }
}

impl Decision {
    pub fn kind(&self) -> DecisionKind {
        match self {
            Self::AllowOnce => DecisionKind::AllowOnce,
            Self::Deny { .. } => DecisionKind::Deny,
            Self::Value { .. } => DecisionKind::Value,
        }
    }
}

/// What an expired suspension resolves to. Deny unless the requesting tool
/// said otherwise: silence is not consent.
#[derive(
    Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum TimeoutFallback {
    #[default]
    Deny,
    AllowOnce,
}

impl TimeoutFallback {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Deny => "deny",
            Self::AllowOnce => "allow_once",
        }
    }

    pub fn parse(s: &str) -> Result<Self, DbError> {
        match s {
            "deny" => Ok(Self::Deny),
            "allow_once" => Ok(Self::AllowOnce),
            _ => Err(DbError::Decode {
                column: "on_timeout",
                source: anyhow::anyhow!("unknown timeout fallback `{s}`"),
            }),
        }
    }

    /// The decision a timeout stands in for.
    pub fn decision(self) -> Decision {
        match self {
            Self::Deny => Decision::Deny {
                reason: DenyReason::Timeout,
            },
            Self::AllowOnce => Decision::AllowOnce,
        }
    }
}

/// The tool call a suspended turn is waiting on, as the model issued it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PendingCall {
    pub id: String,
    pub name: String,
    /// The model's raw JSON arguments string.
    pub arguments: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actions: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_facts: Option<serde_json::Value>,
}

#[derive(
    Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
pub struct BudgetLimits {
    pub rounds: u32,
    pub seconds: Option<u64>,
    pub tokens: Option<u64>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TreeBudgetCheckpoint {
    pub limits: BudgetLimits,
    pub local_limits: BudgetLimits,
    pub rounds: u32,
    pub seconds: u64,
    pub tokens: u64,
    pub usage_known: bool,
}

/// What the turn had spent when it paused, so a resumed run continues against
/// the same budget instead of starting a fresh one.
#[derive(
    Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
pub struct BudgetUsed {
    pub rounds: u32,
    pub seconds: u64,
    pub tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tree: Option<TreeBudgetCheckpoint>,
}

/// One paused turn: everything a resume needs to continue it.
#[derive(Clone, Debug, PartialEq)]
pub struct TurnSuspension {
    pub turn_id: String,
    /// What a client answers. A fresh one per pause, so an answer meant for
    /// an earlier pause of the same turn cannot settle a later one.
    pub request_id: String,
    pub kind: SuspensionKind,
    /// Shown next to the decision, when the tool has something to say.
    pub message: Option<String>,
    pub tool_call: PendingCall,
    /// This turn's round messages so far, without the waiting call's result.
    /// The history and system message before them are rebuilt on resume.
    pub tail: Vec<serde_json::Value>,
    pub budget_used: BudgetUsed,
    /// Set when the pause is inside a nested run: the child turn that
    /// actually waits. Resuming continues there first, then here.
    pub child_turn: Option<String>,
    pub on_timeout: TimeoutFallback,
    pub expires_at: Timestamp,
    pub created_at: Timestamp,
    /// What the run needs to be rebuilt on resume beyond its session and call
    /// chain, opaque here (the driver's own, e.g. values a route bound).
    pub run_context: Option<serde_json::Value>,
}

impl TurnSuspension {
    /// The part a client may see: enough to render the decision, none of the
    /// run state.
    pub fn view(&self) -> SuspensionView {
        SuspensionView {
            request_id: self.request_id.clone(),
            kind: self.kind,
            message: self.message.clone(),
            tool_call_id: Some(self.tool_call.id.clone()),
            tool: Some(self.tool_call.name.clone()),
            options: self.kind.options().to_vec(),
            expires_at: self.expires_at,
        }
    }
}

/// A suspension as the client sees it, on the turn and in the `suspended`
/// event.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SuspensionView {
    pub request_id: String,
    pub kind: SuspensionKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// The waiting call. Absent from a participant's view.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// The decisions the viewer may send; empty when someone else answers.
    pub options: Vec<DecisionKind>,
    pub expires_at: Timestamp,
}

impl SuspensionView {
    /// What the participant of a conversation they do not own may see: no
    /// tool, and only the decisions they may make. A request staff answer
    /// offers them none; they can only wait for it.
    pub fn for_participant(&self) -> SuspensionView {
        let theirs = self.kind.answered_by() == Answerer::Participant;
        SuspensionView {
            tool_call_id: None,
            tool: None,
            options: if theirs {
                self.options.clone()
            } else {
                Vec::new()
            },
            ..self.clone()
        }
    }
}

/// A suspension past its deadline, with the owner a resume runs as.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpiredSuspension {
    pub turn_id: String,
    pub session_id: String,
    pub user_id: String,
    pub on_timeout: TimeoutFallback,
}

const SUSPENSION_COLUMNS: &str = "turn_id, request_id, kind, message, tool_call, tail, \
                                  budget_used, child_turn, on_timeout, expires_at, created_at, \
                                  run_context";

fn json_column<T: serde::de::DeserializeOwned>(
    row: &SqliteRow,
    column: &'static str,
) -> Result<T, DbError> {
    let raw: String = row.try_get(column)?;
    serde_json::from_str(&raw).map_err(|e| DbError::Decode {
        column,
        source: e.into(),
    })
}

/// Read a suspension out of `row`, which must carry its columns under their
/// own names. Public so a caller that joins a suspension onto its own tables
/// can map the row without a second read.
pub fn map_suspension(row: &SqliteRow) -> Result<TurnSuspension, DbError> {
    let kind: String = row.try_get("kind")?;
    let on_timeout: String = row.try_get("on_timeout")?;
    Ok(TurnSuspension {
        turn_id: row.try_get("turn_id")?,
        request_id: row.try_get("request_id")?,
        kind: SuspensionKind::parse(&kind)?,
        message: row.try_get("message")?,
        tool_call: json_column(row, "tool_call")?,
        tail: json_column(row, "tail")?,
        budget_used: json_column(row, "budget_used")?,
        child_turn: row.try_get("child_turn")?,
        on_timeout: TimeoutFallback::parse(&on_timeout)?,
        expires_at: parse_ts(row.try_get("expires_at")?, "expires_at")?,
        created_at: parse_ts(row.try_get("created_at")?, "created_at")?,
        run_context: row
            .try_get::<Option<String>, _>("run_context")?
            .map(|raw| {
                serde_json::from_str(&raw).map_err(|e| DbError::Decode {
                    column: "run_context",
                    source: e.into(),
                })
            })
            .transpose()?,
    })
}

fn to_json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("plain data serializes")
}

/// Pause a running turn. Writes the row and flips the turn to `suspended` in
/// one transaction. Returns `false`, and writes nothing, when the turn is not
/// `in_progress` any more (cancelled or swept underneath the driver).
pub async fn suspend_turn(pool: &Pool, suspension: &TurnSuspension) -> Result<bool, DbError> {
    let mut tx = pool.begin().await?;
    let flipped = sqlx::query(
        r#"UPDATE chat_turns SET status = 'suspended'
           WHERE id = ? AND role = 'assistant' AND status = 'in_progress'"#,
    )
    .bind(&suspension.turn_id)
    .execute(&mut *tx)
    .await?
    .rows_affected()
        > 0;
    if !flipped {
        return Ok(false);
    }
    sqlx::query(
        r#"INSERT INTO chat_turn_suspensions
              (turn_id, request_id, kind, message, tool_call, tail, budget_used,
               child_turn, on_timeout, expires_at, created_at, run_context)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
    )
    .bind(&suspension.turn_id)
    .bind(&suspension.request_id)
    .bind(suspension.kind.as_str())
    .bind(&suspension.message)
    .bind(to_json(&suspension.tool_call))
    .bind(to_json(&suspension.tail))
    .bind(to_json(&suspension.budget_used))
    .bind(&suspension.child_turn)
    .bind(suspension.on_timeout.as_str())
    .bind(suspension.expires_at.to_string())
    .bind(suspension.created_at.to_string())
    .bind(suspension.run_context.as_ref().map(to_json))
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(true)
}

/// The suspension a turn is paused on, if any.
pub async fn get_suspension(pool: &Pool, turn_id: &str) -> Result<Option<TurnSuspension>, DbError> {
    let mut connection = pool.acquire().await?;
    get_suspension_in(&mut connection, turn_id).await
}

pub async fn get_suspension_in(
    connection: &mut sqlx::SqliteConnection,
    turn_id: &str,
) -> Result<Option<TurnSuspension>, DbError> {
    let row = sqlx::query(&format!(
        "SELECT {SUSPENSION_COLUMNS} FROM chat_turn_suspensions WHERE turn_id = ?"
    ))
    .bind(turn_id)
    .fetch_optional(connection)
    .await?;
    row.as_ref().map(map_suspension).transpose()
}

/// Attach `context` to a turn's current suspension. Returns whether the turn
/// was suspended. A driver records here what only it knows and the resume
/// needs (a nested run's route-bound values, say).
pub async fn set_suspension_run_context(
    pool: &Pool,
    turn_id: &str,
    context: &serde_json::Value,
) -> Result<bool, DbError> {
    let updated = sqlx::query("UPDATE chat_turn_suspensions SET run_context = ? WHERE turn_id = ?")
        .bind(to_json(context))
        .bind(turn_id)
        .execute(pool)
        .await?
        .rows_affected();
    Ok(updated > 0)
}

/// Take a turn's suspension for resuming: delete the row and flip the turn
/// back to `in_progress`, in one transaction. `None` when there is nothing to
/// take — never suspended, or another resume got there first.
pub async fn claim_suspension(
    pool: &Pool,
    turn_id: &str,
    expected_request_id: &str,
) -> Result<Option<TurnSuspension>, DbError> {
    let mut tx = pool.begin().await?;
    let suspension = claim_suspension_in(&mut tx, turn_id, expected_request_id).await?;
    tx.commit().await?;
    Ok(suspension)
}

pub async fn claim_suspension_in(
    connection: &mut sqlx::SqliteConnection,
    turn_id: &str,
    expected_request_id: &str,
) -> Result<Option<TurnSuspension>, DbError> {
    let row = sqlx::query(&format!(
        "DELETE FROM chat_turn_suspensions WHERE turn_id = ? AND request_id = ?
         AND EXISTS (SELECT 1 FROM chat_turns WHERE id = chat_turn_suspensions.turn_id
                     AND status = 'suspended') RETURNING {SUSPENSION_COLUMNS}"
    ))
    .bind(turn_id)
    .bind(expected_request_id)
    .fetch_optional(&mut *connection)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let suspension = map_suspension(&row)?;
    sqlx::query(
        r#"UPDATE chat_turns SET status = 'in_progress' WHERE id = ? AND status = 'suspended' RETURNING id"#,
    )
    .bind(turn_id)
    .fetch_one(connection)
    .await?;
    Ok(Some(suspension))
}

/// Give up on a paused turn: it ends `cancelled`, like a stopped one, and its
/// waiting call is recorded as never run. Returns whether anything was paused.
pub async fn cancel_suspended_turn(pool: &Pool, turn_id: &str) -> Result<bool, DbError> {
    let now = Timestamp::now().to_string();
    let mut tx = pool.begin().await?;
    let removed = sqlx::query("DELETE FROM chat_turn_suspensions WHERE turn_id = ?")
        .bind(turn_id)
        .execute(&mut *tx)
        .await?
        .rows_affected()
        > 0;
    if !removed {
        return Ok(false);
    }
    sqlx::query(
        r#"UPDATE chat_turns SET status = 'cancelled', completed_at = ?
           WHERE id = ? AND status = 'suspended'"#,
    )
    .bind(&now)
    .bind(turn_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        r#"UPDATE chat_tool_calls
           SET status = 'errored',
               output_json = COALESCE(output_json, ?),
               completed_at = ?
           WHERE turn_id = ? AND status = 'running'"#,
    )
    .bind(serde_json::Value::String("Cancelled while waiting for a decision.".into()).to_string())
    .bind(&now)
    .bind(turn_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(true)
}

/// The paused turn of a conversation, if it has one. A conversation has at
/// most one: a suspended turn holds it the way a running worker does.
pub async fn suspended_turn_in_session(
    pool: &Pool,
    session_id: &str,
) -> Result<Option<String>, DbError> {
    let row = sqlx::query(
        r#"SELECT s.turn_id FROM chat_turn_suspensions s
           JOIN chat_turns t ON t.id = s.turn_id
           WHERE t.session_id = ?
           LIMIT 1"#,
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await?;
    row.map(|r| r.try_get("turn_id"))
        .transpose()
        .map_err(Into::into)
}

/// The string the SQL prefilter on `expires_at` compares against: the second
/// after `now`, truncated to whole seconds. RFC 3339 strings with and without
/// fractional seconds do not sort as the instants they name, but they do sort
/// by whole second, so every deadline at or before `now` is below this bound;
/// the exact comparison is done after parsing.
pub fn deadline_bound(now: Timestamp) -> String {
    let next_second = Timestamp::from_second(now.as_second() + 1).unwrap_or(now);
    next_second.to_string()
}

/// Every suspension whose deadline has passed at `now`, oldest deadline first.
///
/// Only a person's conversations: the chat path resumes as the owner. A
/// conversation with another owner (`user_id` NULL) is that owner's to sweep.
pub async fn expired_suspensions(
    pool: &Pool,
    now: Timestamp,
) -> Result<Vec<ExpiredSuspension>, DbError> {
    let rows = sqlx::query(
        r#"SELECT s.turn_id, s.on_timeout, s.expires_at, t.session_id, cs.user_id
           FROM chat_turn_suspensions s
           JOIN chat_turns t ON t.id = s.turn_id
           JOIN chat_sessions cs ON cs.id = t.session_id
           WHERE cs.user_id IS NOT NULL AND cs.parent_turn_id IS NULL AND s.expires_at < ?"#,
    )
    .bind(deadline_bound(now))
    .fetch_all(pool)
    .await?;
    let mut expired = Vec::new();
    for row in &rows {
        let expires_at = parse_ts(row.try_get("expires_at")?, "expires_at")?;
        if expires_at > now {
            continue;
        }
        let on_timeout: String = row.try_get("on_timeout")?;
        expired.push((
            expires_at,
            ExpiredSuspension {
                turn_id: row.try_get("turn_id")?,
                session_id: row.try_get("session_id")?,
                user_id: row.try_get("user_id")?,
                on_timeout: TimeoutFallback::parse(&on_timeout)?,
            },
        ));
    }
    expired.sort_by_key(|(at, _)| *at);
    Ok(expired.into_iter().map(|(_, s)| s).collect())
}

/// Record that the pause `request_id` was announced. `true` only for the
/// first call per pause, so whoever gets `true` sends the notification and
/// nobody else does.
pub async fn mark_suspension_notified(
    pool: &Pool,
    request_id: &str,
    now: Timestamp,
) -> Result<bool, DbError> {
    let updated = sqlx::query(
        "UPDATE chat_turn_suspensions SET notified_at = ? \
          WHERE request_id = ? AND notified_at IS NULL",
    )
    .bind(now.to_string())
    .bind(request_id)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(updated > 0)
}

/// The client views of every suspension in a session, by turn id — the
/// one-query-then-bucket read `list_turns` does for its side tables.
pub(crate) async fn suspension_views_for_session(
    pool: &Pool,
    session_id: &str,
) -> Result<std::collections::HashMap<String, SuspensionView>, DbError> {
    let rows = sqlx::query(&format!(
        "SELECT {SUSPENSION_COLUMNS} FROM chat_turn_suspensions \
         WHERE turn_id IN (SELECT id FROM chat_turns WHERE session_id = ?)"
    ))
    .bind(session_id)
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|row| map_suspension(row).map(|s| (s.turn_id.clone(), s.view())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::pool;

    #[test]
    fn pending_call_execution_facts_round_trip_without_entering_presentation_views() {
        let facts = serde_json::json!({"connection": {"owner": {"Principal": "opaque-principal"}, "connector_key": "workspace", "connection_id": "opaque-connection", "generation": "opaque-generation"}, "binding_generation": "opaque-binding"});
        let pending: PendingCall = serde_json::from_value(serde_json::json!({
            "id": "call", "name": "workspace_write", "arguments": "{}", "execution_facts": facts
        }))
        .unwrap();
        assert_eq!(
            serde_json::to_value(&pending).unwrap()["execution_facts"],
            facts
        );
        let mut waiting = suspension("turn", Timestamp::now());
        waiting.tool_call = pending;
        let view = waiting.view();
        let encoded = serde_json::to_string(&view).unwrap();
        assert!(!encoded.contains("opaque-principal"));
        assert!(!encoded.contains("opaque-connection"));
        assert!(!encoded.contains("execution_facts"));
        let absent: PendingCall = serde_json::from_value(serde_json::json!({
            "id": "ordinary", "name": "company_echo", "arguments": "{}"
        }))
        .unwrap();
        assert!(
            serde_json::to_value(absent)
                .unwrap()
                .get("execution_facts")
                .is_none()
        );
    }

    #[test]
    fn pending_call_action_facts_round_trip_and_absence_is_explicit() {
        let pending = PendingCall {
            execution_facts: None,
            id: "call".into(),
            name: "browser".into(),
            arguments: "{}".into(),
            actions: Some(vec!["fill".into()]),
        };
        let encoded = serde_json::to_value(&pending).unwrap();
        assert_eq!(encoded["actions"], serde_json::json!(["fill"]));
        assert_eq!(
            serde_json::from_value::<PendingCall>(encoded).unwrap(),
            pending
        );
        let absent: PendingCall = serde_json::from_value(
            serde_json::json!({"id":"call","name":"browser","arguments":"{}"}),
        )
        .unwrap();
        assert_eq!(absent.actions, None);
    }

    fn suspension(turn_id: &str, expires_at: Timestamp) -> TurnSuspension {
        TurnSuspension {
            turn_id: turn_id.into(),
            request_id: format!("req-{turn_id}"),
            kind: SuspensionKind::Approval,
            message: None,
            tool_call: PendingCall {
                execution_facts: None,
                actions: None,
                id: "call-1".into(),
                name: "company_echo".into(),
                arguments: r#"{"message":"hi"}"#.into(),
            },
            tail: vec![serde_json::json!({"role": "assistant", "content": null})],
            budget_used: BudgetUsed {
                rounds: 1,
                seconds: 3,
                tokens: 120,
                tree: None,
            },
            child_turn: None,
            on_timeout: TimeoutFallback::Deny,
            expires_at,
            created_at: Timestamp::now(),
            run_context: None,
        }
    }

    #[test]
    fn a_participant_answers_only_a_secure_input_and_an_approval_only_ever_times_out_to_deny() {
        assert_eq!(
            SuspensionKind::SecureInput.answered_by(),
            Answerer::Participant
        );
        assert_eq!(SuspensionKind::Approval.answered_by(), Answerer::Staff);
        assert_eq!(SuspensionKind::HumanAnswer.answered_by(), Answerer::Staff);
        for kind in [
            SuspensionKind::Approval,
            SuspensionKind::SecureInput,
            SuspensionKind::HumanAnswer,
        ] {
            assert_eq!(
                kind.timeout_fallback(TimeoutFallback::AllowOnce),
                TimeoutFallback::Deny,
                "{kind:?}: silence never stands in for a yes or a value"
            );
            assert_eq!(
                kind.timeout_fallback(TimeoutFallback::Deny),
                TimeoutFallback::Deny
            );
        }
    }

    #[test]
    fn a_participant_sees_no_tool_and_no_decision_that_is_not_theirs() {
        let mut row = suspension("a1", in_an_hour());
        row.message = Some("Enter the code".into());
        let approval = row.view().for_participant();
        assert_eq!(approval.tool, None);
        assert_eq!(approval.tool_call_id, None);
        assert!(approval.options.is_empty(), "staff answer an approval");
        let json = serde_json::to_value(&approval).unwrap();
        assert!(json.get("tool").is_none() && json.get("tool_call_id").is_none());
        assert_eq!(json["kind"], "approval");
        assert_eq!(json["request_id"], "req-a1");

        row.kind = SuspensionKind::SecureInput;
        let secure = row.view().for_participant();
        assert_eq!(secure.options, [DecisionKind::Value, DecisionKind::Deny]);
        assert_eq!(secure.message.as_deref(), Some("Enter the code"));
        assert_eq!(secure.expires_at, row.expires_at);
    }

    #[test]
    fn a_value_never_shows_in_debug_output() {
        let decision = Decision::Value {
            value: serde_json::json!("481516"),
        };
        let printed = format!("{decision:?}");
        assert!(!printed.contains("481516"), "{printed}");
        assert!(printed.contains("redacted"), "{printed}");
    }

    #[tokio::test]
    async fn a_run_context_rides_with_the_pause_it_belongs_to() {
        let pool = pool().await;
        running_turn(&pool, "a1").await;
        assert!(
            !set_suspension_run_context(&pool, "a1", &serde_json::json!({}))
                .await
                .unwrap(),
            "nothing to attach it to before the pause"
        );
        suspend_turn(&pool, &suspension("a1", in_an_hour()))
            .await
            .unwrap();
        let context = serde_json::json!({"route": "billing", "route_binds": {"customer": "K-1"}});
        assert!(
            set_suspension_run_context(&pool, "a1", &context)
                .await
                .unwrap()
        );
        assert_eq!(
            get_suspension(&pool, "a1")
                .await
                .unwrap()
                .unwrap()
                .run_context,
            Some(context.clone())
        );
        assert_eq!(
            claim_suspension(&pool, "a1", "req-a1")
                .await
                .unwrap()
                .unwrap()
                .run_context,
            Some(context)
        );
    }

    fn in_an_hour() -> Timestamp {
        Timestamp::now() + jiff::SignedDuration::from_hours(1)
    }

    async fn running_turn(pool: &Pool, turn_id: &str) -> String {
        let s = create_session(pool, "u1").await.unwrap();
        create_user_turn(pool, &s.id, &format!("{turn_id}-u"), "hi")
            .await
            .unwrap();
        create_assistant_turn_in_progress(pool, &s.id, turn_id, "m")
            .await
            .unwrap();
        insert_running_tool_call(pool, turn_id, "call-1", "company_echo", "{}")
            .await
            .unwrap();
        s.id
    }

    #[tokio::test]
    async fn a_suspended_turn_carries_its_decision_to_every_reader() {
        let pool = pool().await;
        let session_id = running_turn(&pool, "a1").await;
        let row = suspension("a1", in_an_hour());
        assert!(suspend_turn(&pool, &row).await.unwrap());

        let listed = list_turns(&pool, &session_id).await.unwrap();
        let turn = listed.iter().find(|t| t.turn.id == "a1").unwrap();
        assert_eq!(turn.turn.status, TurnStatus::Suspended);
        assert_eq!(turn.suspension, Some(row.view()));
        assert_eq!(
            turn.suspension.as_ref().unwrap().options,
            [DecisionKind::AllowOnce, DecisionKind::Deny]
        );
        let one = get_turn_with_tools(&pool, &session_id, "a1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(one.suspension, Some(row.view()));
        assert_eq!(get_suspension(&pool, "a1").await.unwrap(), Some(row));
    }

    #[tokio::test]
    async fn only_a_running_turn_can_be_suspended() {
        let pool = pool().await;
        running_turn(&pool, "a1").await;
        finalize_turn(&pool, "a1", TurnStatus::Cancelled, None)
            .await
            .unwrap();
        assert!(
            !suspend_turn(&pool, &suspension("a1", in_an_hour()))
                .await
                .unwrap()
        );
        assert_eq!(get_suspension(&pool, "a1").await.unwrap(), None);
    }

    #[tokio::test]
    async fn a_suspension_is_claimed_exactly_once() {
        let pool = pool().await;
        let session_id = running_turn(&pool, "a1").await;
        let row = suspension("a1", in_an_hour());
        suspend_turn(&pool, &row).await.unwrap();

        assert_eq!(
            claim_suspension(&pool, "a1", "req-a1").await.unwrap(),
            Some(row)
        );
        assert_eq!(claim_suspension(&pool, "a1", "req-a1").await.unwrap(), None);
        let turn = get_turn(&pool, &session_id, "a1").await.unwrap().unwrap();
        assert_eq!(turn.status, TurnStatus::InProgress);
    }

    #[tokio::test]
    async fn a_stale_request_cannot_claim_the_current_pause() {
        let pool = pool().await;
        let session = running_turn(&pool, "a1").await;
        let row = suspension("a1", in_an_hour());
        suspend_turn(&pool, &row).await.unwrap();
        assert!(
            claim_suspension(&pool, "a1", "previous-request")
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(get_suspension(&pool, "a1").await.unwrap(), Some(row));
        assert_eq!(
            get_turn(&pool, &session, "a1")
                .await
                .unwrap()
                .unwrap()
                .status,
            TurnStatus::Suspended
        );
    }

    #[tokio::test]
    async fn connection_claims_commit_together_and_rollback_together() {
        let pool = pool().await;
        for turn in ["parent", "child"] {
            running_turn(&pool, turn).await;
            suspend_turn(&pool, &suspension(turn, in_an_hour()))
                .await
                .unwrap();
        }
        let mut tx = pool.begin().await.unwrap();
        assert!(
            claim_suspension_in(&mut tx, "child", "req-child")
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            claim_suspension_in(&mut tx, "parent", "stale-parent")
                .await
                .unwrap()
                .is_none()
        );
        tx.rollback().await.unwrap();
        for turn in ["parent", "child"] {
            assert!(get_suspension(&pool, turn).await.unwrap().is_some());
        }
        let mut tx = pool.begin().await.unwrap();
        for turn in ["child", "parent"] {
            let row = get_suspension_in(&mut tx, turn).await.unwrap().unwrap();
            assert_eq!(
                claim_suspension_in(&mut tx, turn, &row.request_id)
                    .await
                    .unwrap(),
                Some(row)
            );
        }
        tx.commit().await.unwrap();
        for turn in ["parent", "child"] {
            assert!(get_suspension(&pool, turn).await.unwrap().is_none());
            let status: String = sqlx::query_scalar("SELECT status FROM chat_turns WHERE id = ?")
                .bind(turn)
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(status, "in_progress");
        }
    }

    #[tokio::test]
    async fn a_terminal_turn_cannot_be_claimed_from_a_leftover_pause() {
        let pool = pool().await;
        running_turn(&pool, "a1").await;
        let row = suspension("a1", in_an_hour());
        suspend_turn(&pool, &row).await.unwrap();
        sqlx::query("UPDATE chat_turns SET status = 'cancelled' WHERE id = 'a1'")
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            claim_suspension(&pool, "a1", &row.request_id)
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(get_suspension(&pool, "a1").await.unwrap(), Some(row));
    }

    #[tokio::test]
    async fn a_failed_status_transition_rolls_back_the_claimed_pause() {
        let pool = pool().await;
        let session = running_turn(&pool, "a1").await;
        let row = suspension("a1", in_an_hour());
        suspend_turn(&pool, &row).await.unwrap();
        sqlx::query("CREATE TRIGGER cancel_during_claim AFTER DELETE ON chat_turn_suspensions BEGIN UPDATE chat_turns SET status = 'cancelled' WHERE id = OLD.turn_id; END")
            .execute(&pool).await.unwrap();
        assert!(
            claim_suspension(&pool, "a1", &row.request_id)
                .await
                .is_err()
        );
        assert_eq!(get_suspension(&pool, "a1").await.unwrap(), Some(row));
        assert_eq!(
            get_turn(&pool, &session, "a1")
                .await
                .unwrap()
                .unwrap()
                .status,
            TurnStatus::Suspended
        );
    }

    /// A restart must not take a paused turn for a crashed one: its waiting
    /// call stays `running` and the turn stays suspended.
    #[tokio::test]
    async fn a_suspended_turn_survives_the_startup_sweep_and_the_orphan_recovery() {
        let pool = pool().await;
        let session_id = running_turn(&pool, "a1").await;
        suspend_turn(&pool, &suspension("a1", in_an_hour()))
            .await
            .unwrap();

        assert_eq!(sweep_in_progress_at_startup(&pool).await.unwrap(), 0);
        mark_orphaned_in_progress_as_errored(&pool, &session_id, None)
            .await
            .unwrap();

        let turn = get_turn_with_tools(&pool, &session_id, "a1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(turn.turn.status, TurnStatus::Suspended);
        assert_eq!(turn.tool_calls[0].status, ToolCallStatus::Running);
    }

    #[test]
    fn tree_checkpoint_json_preserves_original_limits_and_unknown_usage() {
        let checkpoint = TreeBudgetCheckpoint {
            limits: BudgetLimits {
                rounds: 8,
                seconds: Some(60),
                tokens: Some(100),
            },
            local_limits: BudgetLimits {
                rounds: 3,
                seconds: Some(20),
                tokens: Some(50),
            },
            rounds: 2,
            seconds: 4,
            tokens: 25,
            usage_known: false,
        };
        let used = BudgetUsed {
            rounds: 1,
            seconds: 2,
            tokens: 10,
            tree: Some(checkpoint),
        };
        let restored: BudgetUsed =
            serde_json::from_str(&serde_json::to_string(&used).unwrap()).unwrap();
        assert_eq!(restored, used);
        let ordinary = BudgetUsed {
            rounds: 1,
            seconds: 2,
            tokens: 10,
            tree: None,
        };
        assert!(
            serde_json::to_value(ordinary)
                .unwrap()
                .get("tree")
                .is_none()
        );
    }

    #[tokio::test]
    async fn delegated_child_does_not_expire_independently() {
        let pool = pool().await;
        running_turn(&pool, "child").await;
        sqlx::query("UPDATE chat_sessions SET parent_turn_id = 'parent' WHERE id = (SELECT session_id FROM chat_turns WHERE id = 'child')").execute(&pool).await.unwrap();
        let now = Timestamp::now();
        suspend_turn(
            &pool,
            &suspension("child", now - jiff::SignedDuration::from_secs(5)),
        )
        .await
        .unwrap();
        assert!(expired_suspensions(&pool, now).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn only_suspensions_past_their_deadline_are_expired() {
        let pool = pool().await;
        running_turn(&pool, "late").await;
        running_turn(&pool, "early").await;
        running_turn(&pool, "waiting").await;
        let now = Timestamp::now();
        let mut late = suspension("late", now - jiff::SignedDuration::from_secs(5));
        late.on_timeout = TimeoutFallback::AllowOnce;
        suspend_turn(&pool, &late).await.unwrap();
        suspend_turn(
            &pool,
            &suspension("early", now - jiff::SignedDuration::from_mins(5)),
        )
        .await
        .unwrap();
        suspend_turn(&pool, &suspension("waiting", in_an_hour()))
            .await
            .unwrap();

        let expired = expired_suspensions(&pool, now).await.unwrap();
        let ids: Vec<_> = expired.iter().map(|e| e.turn_id.as_str()).collect();
        assert_eq!(ids, ["early", "late"]);
        assert_eq!(expired[1].user_id, "u1");
        assert_eq!(expired[1].on_timeout, TimeoutFallback::AllowOnce);
    }

    #[tokio::test]
    async fn a_deadline_within_the_current_second_counts_however_it_is_written() {
        let pool = pool().await;
        for id in ["whole", "fraction", "later"] {
            running_turn(&pool, id).await;
        }
        let now = Timestamp::from_millisecond(1_800_000_000_500).unwrap();
        let at = |ms| Timestamp::from_millisecond(ms).unwrap();
        suspend_turn(&pool, &suspension("whole", at(1_800_000_000_000)))
            .await
            .unwrap();
        suspend_turn(&pool, &suspension("fraction", at(1_800_000_000_250)))
            .await
            .unwrap();
        suspend_turn(&pool, &suspension("later", at(1_800_000_000_900)))
            .await
            .unwrap();

        let ids: Vec<_> = expired_suspensions(&pool, now)
            .await
            .unwrap()
            .into_iter()
            .map(|e| e.turn_id)
            .collect();
        assert_eq!(ids, ["whole", "fraction"]);
    }

    /// A conversation a person does not own has no person to resume as; its
    /// owner, above this crate, settles it, never the chat sweeper.
    #[tokio::test]
    async fn the_chat_sweeper_never_sees_a_conversation_without_a_person() {
        let pool = pool().await;
        let session_id = crate::db::tests::unowned_session(&pool).await;
        create_user_turn(&pool, &session_id, "other-u", "hi")
            .await
            .unwrap();
        create_assistant_turn_in_progress(&pool, &session_id, "other-turn", "m")
            .await
            .unwrap();
        insert_running_tool_call(&pool, "other-turn", "call-1", "company_echo", "{}")
            .await
            .unwrap();
        let now = Timestamp::now();
        suspend_turn(
            &pool,
            &suspension("other-turn", now - jiff::SignedDuration::from_secs(5)),
        )
        .await
        .unwrap();

        assert!(expired_suspensions(&pool, now).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn cancelling_a_paused_turn_ends_it_and_its_waiting_call() {
        let pool = pool().await;
        let session_id = running_turn(&pool, "a1").await;
        suspend_turn(&pool, &suspension("a1", in_an_hour()))
            .await
            .unwrap();

        assert!(cancel_suspended_turn(&pool, "a1").await.unwrap());
        assert!(!cancel_suspended_turn(&pool, "a1").await.unwrap());
        let turn = get_turn_with_tools(&pool, &session_id, "a1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(turn.turn.status, TurnStatus::Cancelled);
        assert_eq!(turn.suspension, None);
        assert_eq!(turn.tool_calls[0].status, ToolCallStatus::Errored);
        assert_eq!(
            suspended_turn_in_session(&pool, &session_id).await.unwrap(),
            None
        );
    }

    /// A paused turn holds its conversation the way a running worker does, so
    /// a message queued behind it waits for the decision.
    #[tokio::test]
    async fn a_waiting_message_does_not_start_in_a_paused_conversation() {
        let pool = pool().await;
        let session_id = running_turn(&pool, "a1").await;
        suspend_turn(&pool, &suspension("a1", in_an_hour()))
            .await
            .unwrap();
        assert_eq!(
            suspended_turn_in_session(&pool, &session_id).await.unwrap(),
            Some("a1".into())
        );
        create_user_turn(&pool, &session_id, "next", "and then?")
            .await
            .unwrap();
        insert_pending_turn(
            &pool,
            &PendingTurn {
                turn_id: "next".into(),
                session_id: session_id.clone(),
                user_id: "u1".into(),
                model: "m".into(),
                voice: false,
                client_ip: None,
                secure: false,
                created_at: Timestamp::now(),
            },
        )
        .await
        .unwrap();

        assert_eq!(take_next_for_user(&pool, "u1", &[]).await.unwrap(), None);
        claim_suspension(&pool, "a1", "req-a1").await.unwrap();
        assert_eq!(
            take_next_for_user(&pool, "u1", &[])
                .await
                .unwrap()
                .map(|p| p.turn_id),
            Some("next".into())
        );
    }

    #[tokio::test]
    async fn a_pause_is_announced_once() {
        let pool = pool().await;
        running_turn(&pool, "a1").await;
        assert!(
            suspend_turn(&pool, &suspension("a1", in_an_hour()))
                .await
                .unwrap()
        );
        let now = Timestamp::now();
        assert!(
            mark_suspension_notified(&pool, "req-a1", now)
                .await
                .unwrap()
        );
        assert!(
            !mark_suspension_notified(&pool, "req-a1", now)
                .await
                .unwrap()
        );
        assert!(
            !mark_suspension_notified(&pool, "req-gone", now)
                .await
                .unwrap()
        );
        let notified: Option<String> = sqlx::query_scalar(
            "SELECT notified_at FROM chat_turn_suspensions WHERE request_id = 'req-a1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(notified.is_some());
    }

    #[test]
    fn a_decision_reads_as_the_resume_body_names_it() {
        let deny: Decision =
            serde_json::from_value(serde_json::json!({"decision": "deny", "reason": "user"}))
                .unwrap();
        assert_eq!(deny.kind(), DecisionKind::Deny);
        let value: Decision =
            serde_json::from_value(serde_json::json!({"decision": "value", "value": "s3cret"}))
                .unwrap();
        assert_eq!(
            value,
            Decision::Value {
                value: serde_json::json!("s3cret")
            }
        );
        assert_eq!(
            TimeoutFallback::default().decision(),
            Decision::Deny {
                reason: DenyReason::Timeout
            }
        );
    }
}
