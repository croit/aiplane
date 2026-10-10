// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The chat driver's half of [`crate::suspend`]: pausing a turn at the call
//! that asked for it, and picking the turn up again from the stored tail.
//!
//! Kept out of `run_one_turn` on purpose. The loop calls three things — find
//! a pause in a round's results, write it, and resume from one — and owns
//! none of their detail.

use std::time::Instant;

use aiplane_agents::db::agent_audit::AuditKind;
use aiplane_agents::db::run_sessions;
use serde_json::{Value, json};
use session_core::db::{
    self as chat, BudgetUsed, Decision, DenyReason, PendingCall, ToolCallStatus, TurnSuspension,
};
use session_core::driver::{SessionContext, TurnError};
use session_core::workers::TurnUpdate;

use super::{OpenAiDriver, ToolResultBudget, cap_tool_result, persist_err};
use crate::agent_run::AgentRun;
use crate::server::tools::runner::{self, ToolCallRef, ToolResultRecord};
use crate::server::tools::{ToolContext, ToolSource, extract_content_parts};
use crate::suspend::{ChildPause, ResumeFrom, Suspend, SuspendRequest, extract_suspend};

/// The answer to a second suspend request in the same round. A turn waits
/// for one decision at a time; the model can ask again once it has the first.
const ONE_DECISION_AT_A_TIME: &str = "This call needs a decision, and another call in the same \
                                      round is already waiting for one. Call it again after that \
                                      one is settled.";

/// What a suspend request becomes where the run cannot pause. Normally the
/// tool refuses on its own; this covers one that asks anyway.
const CANNOT_PAUSE: &str = "This call needs a decision from the user, and this run cannot pause \
                            to ask for it, so it did not run.";

/// When a resumed run counts as having started: `now`, moved back by the
/// seconds spent before the pause, so the budget's `seconds` limit covers the
/// whole run and not just the part after the decision.
pub(super) fn started_at(now: Instant, resume: Option<&ResumeFrom>) -> Instant {
    let before = resume.map_or(0, |r| r.suspension.budget_used.seconds);
    now.checked_sub(std::time::Duration::from_secs(before))
        .unwrap_or(now)
}

/// The round's pause, if one of its calls asked for a decision. Every other
/// suspend request in the round is answered with an error in place, so the
/// rest of the round records and replays as usual.
pub(super) fn take_pause(
    d: &OpenAiDriver,
    calls: &[ToolCallRef],
    results: &mut [ToolResultRecord],
) -> Option<(ToolCallRef, SuspendRequest)> {
    let mut pause = None;
    for (call, result) in calls.iter().zip(results.iter_mut()) {
        let Some(request) = extract_suspend(&result.body) else {
            continue;
        };
        let refusal = if d.tool_ctx.suspend != Suspend::Available {
            CANNOT_PAUSE
        } else if pause.is_some() {
            ONE_DECISION_AT_A_TIME
        } else {
            pause = Some((call.clone(), request));
            continue;
        };
        result.body = json!({ "error": refusal });
        result.failed = true;
    }
    pause
}

/// Write the pause and flip the turn to `suspended`. The caller returns from
/// the turn afterwards; the worker harness sees the paused row and leaves it.
///
/// A turn stopped while its tools ran is not paused: the waiting call is
/// settled as never run, and the harness records the cancel.
pub(super) async fn pause(
    d: &OpenAiDriver,
    ctx: &SessionContext,
    call: &ToolCallRef,
    request: SuspendRequest,
    tail: &[Value],
    budget_used: BudgetUsed,
) -> Result<(), TurnError> {
    if ctx.cancel.load(std::sync::atomic::Ordering::SeqCst) {
        settle_call(
            d,
            ctx,
            &call.id,
            "The turn was stopped before this call's decision was asked for.",
            ToolCallStatus::Errored,
        )
        .await?;
        return Ok(());
    }
    let now = jiff::Timestamp::now();
    let expires_at = match &request.child {
        Some(child) => {
            check_child(d, ctx, child).await?;
            child.expires_at
        }
        None => now + jiff::SignedDuration::from_secs(request.timeout_secs as i64),
    };
    let suspension = TurnSuspension {
        turn_id: ctx.assistant_turn_id.clone(),
        request_id: uuid::Uuid::new_v4().to_string(),
        kind: request.kind,
        message: request.message,
        tool_call: PendingCall {
            id: call.id.clone(),
            name: call.name.clone(),
            arguments: call.arguments_raw.clone(),
        },
        tail: tail.to_vec(),
        budget_used,
        child_turn: request.child.map(|c| c.turn_id),
        on_timeout: request.kind.timeout_fallback(request.on_timeout),
        expires_at,
        created_at: now,
        run_context: request.context,
    };
    let paused = chat::suspend_turn(&d.state.db, &suspension)
        .await
        .map_err(persist_err("suspend_turn", &ctx.assistant_turn_id))?;
    if paused {
        tracing::info!(
            turn = %ctx.assistant_turn_id,
            tool = %call.name,
            kind = request.kind.as_str(),
            rounds = budget_used.rounds,
            "turn suspended for a decision"
        );
        if d.tool_ctx.agent_active() {
            d.tool_ctx
                .audit(
                    AuditKind::RunSuspended,
                    json!({
                        "session_id": ctx.session_id,
                        "turn_id": suspension.turn_id,
                        "request_id": suspension.request_id,
                        "kind": suspension.kind,
                        "tool": suspension.tool_call.name,
                        "child_turn": suspension.child_turn,
                        "expires_at": suspension.expires_at,
                    }),
                )
                .await;
        }
    }
    let _ = ctx.broadcast.send(TurnUpdate::Tick);
    Ok(())
}

/// A call may only wait on a sub-agent run this turn dispatched, and only
/// while that run is paused. Anything else is not a pause the resume could
/// settle, so the turn fails instead of waiting forever.
async fn check_child(
    d: &OpenAiDriver,
    ctx: &SessionContext,
    child: &ChildPause,
) -> Result<(), TurnError> {
    let session = run_sessions::run_session_of_turn(&d.state.db, &child.turn_id)
        .await
        .map_err(persist_err("run_session_of_turn", &ctx.assistant_turn_id))?;
    let ours = session
        .is_some_and(|s| s.parent_turn_id.as_deref() == Some(ctx.assistant_turn_id.as_str()));
    let paused = chat::get_suspension(&d.state.db, &child.turn_id)
        .await
        .map_err(persist_err("get_suspension", &ctx.assistant_turn_id))?
        .is_some();
    if ours && paused {
        return Ok(());
    }
    tracing::error!(
        turn = %ctx.assistant_turn_id,
        child = %child.turn_id,
        "a tool asked to wait on a sub-agent run that is not this turn's paused child"
    );
    Err(TurnError::Aborted {
        message: "a tool asked this turn to wait on a sub-agent run that this turn did not \
                  start or that is not waiting; the turn was stopped"
            .into(),
    })
}

/// How a resume left the turn.
pub(super) enum Resumed {
    /// The waiting call is answered; continue the round loop at this round.
    Continue { start_round: u32 },
    /// Running the call again asked for another decision. The turn is paused
    /// again and the caller returns.
    Paused,
}

/// Rebuild the turn's state from its suspension and answer the waiting call.
///
/// `messages` holds the freshly rebuilt history and system message; the
/// stored tail goes after them, then the call's result.
pub(super) async fn resume_into(
    d: &OpenAiDriver,
    ctx: &SessionContext,
    tools: &dyn ToolSource,
    tool_ctx: &ToolContext,
    resume: &ResumeFrom,
    messages: &mut Vec<Value>,
    budget: &mut ToolResultBudget,
) -> Result<Resumed, TurnError> {
    let suspension = &resume.suspension;
    messages.extend(suspension.tail.iter().cloned());
    let call = ToolCallRef {
        id: suspension.tool_call.id.clone(),
        name: suspension.tool_call.name.clone(),
        arguments_raw: suspension.tool_call.arguments.clone(),
    };

    if let Some(run) = d.agent()
        && tools.get(&call.name).is_some_and(|t| t.sensitive_args())
    {
        run.note_sensitive(&call.name);
    }
    let call_ctx = ToolContext {
        call_id: Some(call.id.clone()),
        ..tool_ctx.clone()
    };
    let content = match (&resume.child_result, &resume.decision) {
        (None, Decision::Deny { reason }) => {
            let refusal = denial(*reason, &call.name);
            settle_call(d, ctx, &call.id, &refusal, ToolCallStatus::Errored).await?;
            settled_without_running(&call_ctx, &call, "denied", json!({ "error": refusal })).await;
            Value::String(refusal)
        }
        (child_result, decided) => {
            let (body, status) = match child_result {
                Some(body) => {
                    settled_without_running(
                        &call_ctx,
                        &call,
                        "answered_by_sub_agent",
                        body.clone(),
                    )
                    .await;
                    (body.clone(), ToolCallStatus::Completed)
                }
                None => {
                    let tool_ctx = ToolContext {
                        suspend: Suspend::Decided(suspension.kind, decided.clone()),
                        ..tool_ctx.clone()
                    };
                    let (status, body) = runner::execute_tool_calls(
                        tools,
                        &tool_ctx,
                        std::slice::from_ref(&call),
                        &d.agent()
                            .map(AgentRun::injection)
                            .cloned()
                            .unwrap_or_default(),
                    )
                    .await
                    .pop()
                    .map(|record| (record.status(), record.body))
                    .unwrap_or_else(|| {
                        (
                            ToolCallStatus::Errored,
                            json!({ "error": "the tool produced no result" }),
                        )
                    });
                    let body = tool_ctx.redaction().withhold(body);
                    (body, status)
                }
            };
            if let Some(request) = extract_suspend(&body) {
                pause(
                    d,
                    ctx,
                    &call,
                    request,
                    &suspension.tail,
                    suspension.budget_used,
                )
                .await?;
                return Ok(Resumed::Paused);
            }
            let output = serde_json::to_string_pretty(&body).unwrap_or_else(|_| "{}".into());
            settle_call(d, ctx, &call.id, &output, status).await?;
            match extract_content_parts(&body) {
                Some(parts) => Value::Array(parts.clone()),
                None => {
                    let (cap, reason) = budget.cap();
                    Value::String(cap_tool_result(output, cap, reason))
                }
            }
        }
    };
    messages.push(json!({
        "role": "tool",
        "tool_call_id": &call.id,
        "content": content,
    }));
    budget.resync(runner::tool_output_bytes(messages));
    Ok(Resumed::Continue {
        start_round: suspension.budget_used.rounds,
    })
}

/// The `tool_result` event of a waiting call a resume settles without
/// running its tool: a denial, or the resumed sub-agent's result.
async fn settled_without_running(
    ctx: &ToolContext,
    call: &ToolCallRef,
    status: &str,
    result: Value,
) {
    if !ctx.agent_active() && ctx.personal_dot.is_none() {
        return;
    }
    let detail = if ctx.personal_dot.is_some() {
        json!({
            "tool": call.name,
            "call_id": call.id,
            "status": status,
            "resumed": true,
        })
    } else {
        json!({
            "tool": call.name,
            "arguments": serde_json::from_str::<Value>(&call.arguments_raw)
                .unwrap_or_else(|_| Value::String(call.arguments_raw.clone())),
            "status": status,
            "result": result,
            "resumed": true,
        })
    };
    ctx.audit_event(AuditKind::ToolResult, None, detail).await;
}

/// Every tool-call id the replayed `messages` already used, so a resumed
/// turn's later rounds cannot reuse one.
pub(super) fn tool_call_ids(messages: &[Value]) -> impl Iterator<Item = String> + '_ {
    messages
        .iter()
        .filter_map(|m| m["tool_calls"].as_array())
        .flatten()
        .filter_map(|call| call["id"].as_str().map(str::to_string))
}

/// What the model reads in place of a denied call's result.
fn denial(reason: DenyReason, tool: &str) -> String {
    match reason {
        DenyReason::User => format!(
            "The user declined this call, so `{tool}` did not run. Do not call it again for the \
             same purpose unless the user asks you to; continue without it, or say what you \
             could not do."
        ),
        DenyReason::Timeout => format!(
            "Nobody decided on this call before the request expired, so `{tool}` did not run. \
             Tell the user what was left waiting for their decision."
        ),
    }
}

async fn settle_call(
    d: &OpenAiDriver,
    ctx: &SessionContext,
    call_id: &str,
    output: &str,
    status: ToolCallStatus,
) -> Result<(), TurnError> {
    chat::complete_tool_call(&d.state.db, &ctx.assistant_turn_id, call_id, output, status)
        .await
        .map_err(persist_err("complete_tool_call", &ctx.assistant_turn_id))?;
    let _ = ctx.broadcast.send(TurnUpdate::Tick);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::Duration;

    use serde_json::{Value, json};
    use session_core::db::{self as chat, Decision, DenyReason, ToolCallStatus, TurnStatus};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::openai_driver::{OpenAiDriver, TurnFacts, build_tool_context};
    use crate::rama_server::state::RamaState;
    use crate::server::tools::ToolRegistry;
    use crate::server::tools::ask_first::AskFirst;
    use crate::server::tools::echo::Echo;
    use crate::suspend::{ResumeFrom, claim_for_resume};
    use aiplane_core::server::upstreams::{
        self,
        config::{BackendConfig, PickerStrategy, PoolKind, UpstreamPoolConfig},
    };

    const MODEL: &str = "model-a";

    /// A chat upstream answering each round with the next scripted delta. The
    /// counter lives on the mock, not on a state, so it keeps counting across
    /// a simulated restart.
    struct Scripted {
        deltas: Vec<Value>,
        served: AtomicUsize,
    }

    impl wiremock::Respond for Scripted {
        fn respond(&self, _req: &wiremock::Request) -> ResponseTemplate {
            let i = self.served.fetch_add(1, Ordering::SeqCst);
            let delta = &self.deltas[i.min(self.deltas.len() - 1)];
            let sse = format!(
                "data: {}\n\ndata: [DONE]\n\n",
                json!({"choices": [{"index": 0, "delta": delta}]})
            );
            ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream")
        }
    }

    fn echo_call() -> Value {
        json!({"tool_calls": [{"index": 0, "id": "call-1", "type": "function",
            "function": {"name": "company_echo", "arguments": "{\"message\":\"ship it\"}"}}]})
    }

    async fn upstream(deltas: Vec<Value>) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(Scripted {
                deltas,
                served: AtomicUsize::new(0),
            })
            .mount(&server)
            .await;
        server
    }

    async fn requests(server: &MockServer) -> Vec<Value> {
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .map(|r| serde_json::from_slice(&r.body).unwrap())
            .collect()
    }

    /// The state a process boots with: the database at `db_path` (migrated
    /// and swept, as at startup) and a fresh registry of `tools`.
    async fn boot(db_path: &Path, upstream: &str, tools: ToolRegistry) -> Arc<RamaState> {
        let db = aiplane_core::server::db::open(db_path).await.unwrap();
        let mut pools = HashMap::new();
        pools.insert(
            "pool".to_string(),
            UpstreamPoolConfig {
                voices: Default::default(),
                offer_voices: Vec::new(),
                allowed_groups: Vec::new(),
                fallback_offline: None,
                compliance: Default::default(),
                enforce_limits: true,
                kind: PoolKind::Chat,
                strategy: PickerStrategy::RoundRobin,
                models: Vec::new(),
                backend: vec![BackendConfig {
                    alias: None,
                    supports_edit: false,
                    enabled: true,
                    name: "mock".into(),
                    base_url: upstream.into(),
                    api_key_env: None,
                    api_key: None,
                    weight: 1,
                    max_inflight: 16,
                    health_path: "/models".into(),
                    models: Vec::new(),
                }],
            },
        );
        let registry = upstreams::UpstreamRegistry::new(&pools).unwrap();
        registry.pools()[0].backends[0].set_models([MODEL.to_string()].into());
        let config = aiplane_core::server::Config {
            gateway: aiplane_core::server::config::GatewayConfig {
                upstream_wait_secs: 0,
                ..Default::default()
            },
            ..Default::default()
        };
        let app = crate::server::AppState::new(
            config,
            db.clone(),
            registry,
            Arc::new(tools),
            Arc::new(crate::server::tools::echo::granted_to_everyone()),
        );
        let sessions = aiplane_core::rama_server::SessionStore::new(db, [7u8; 32]);
        Arc::new(RamaState::new(
            app,
            sessions,
            aiplane_core::server::usage::UsageHandle::disabled(),
        ))
    }

    fn gated() -> ToolRegistry {
        ToolRegistry::new().with(AskFirst::new(Echo, Duration::from_secs(3600)))
    }

    async fn open_turn(state: &Arc<RamaState>) -> (String, String) {
        let now = jiff::Timestamp::now();
        aiplane_core::server::db::users::upsert(
            &state.db,
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
        let session = chat::create_session(&state.db, "u1").await.unwrap();
        chat::create_user_turn(&state.db, &session.id, "u-turn", "echo something")
            .await
            .unwrap();
        chat::create_assistant_turn_in_progress(&state.db, &session.id, "a-turn", MODEL)
            .await
            .unwrap();
        (session.id, "a-turn".into())
    }

    /// Drive the turn once through the chat driver, as the chat path wires it.
    async fn drive(state: &Arc<RamaState>, session_id: &str, resume: Option<ResumeFrom>) {
        drive_as(state, session_id, resume, None).await;
    }

    async fn drive_as(
        state: &Arc<RamaState>,
        session_id: &str,
        resume: Option<ResumeFrom>,
        persona: Option<Arc<crate::persona::ChatPersona>>,
    ) {
        let tool_ctx = build_tool_context(
            state,
            TurnFacts {
                actor: crate::agent_run::Actor::person("u1", vec![]),
                session_id: session_id.into(),
                assistant_turn_id: "a-turn".into(),
                client_ip: None,
                chat_feedback: None,
                model: Some(MODEL.into()),
                pool_access: None,
                suspendable: true,
            },
        );
        let driver = Box::new(OpenAiDriver {
            state: state.clone(),
            tool_ctx,
            source: aiplane_core::server::db::usage::UsageSource::Chat,
            history_limit: None,
            voice_mode: false,
            clock: crate::budget::system_clock(),
            resume,
            persona,
        });
        let (broadcast, _rx) = tokio::sync::broadcast::channel(64);
        let ctx = session_core::driver::SessionContext {
            user_id: Some("u1".into()),
            session_id: session_id.into(),
            assistant_turn_id: "a-turn".into(),
            model: MODEL.into(),
            cancel: Arc::new(AtomicBool::new(false)),
            broadcast,
            steers: session_core::workers::SteerInbox::default(),
        };
        session_core::worker::run_session_turn(state.db.clone(), driver, ctx).await;
    }

    async fn the_turn(state: &Arc<RamaState>, session_id: &str) -> chat::TurnWithTools {
        chat::get_turn_with_tools(&state.db, session_id, "a-turn")
            .await
            .unwrap()
            .unwrap()
    }

    fn last_message(request: &Value) -> &Value {
        request["messages"].as_array().unwrap().last().unwrap()
    }

    /// Pause, lose the process, come back, resume: the same turn finishes, the
    /// approved call ran exactly once, and the model saw the round it had
    /// already done followed by the call's real result.
    #[tokio::test]
    async fn a_paused_turn_survives_a_restart_and_resumes_to_completion() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("gateway.db");
        let server = upstream(vec![echo_call(), json!({"content": "Echoed: ship it"})]).await;

        let before = boot(&db_path, &server.uri(), gated()).await;
        let (session_id, turn_id) = open_turn(&before).await;
        drive(&before, &session_id, None).await;
        let paused = the_turn(&before, &session_id).await;
        assert_eq!(paused.turn.status, TurnStatus::Suspended);
        assert_eq!(paused.tool_calls[0].status, ToolCallStatus::Running);
        let suspension = chat::get_suspension(&before.db, &turn_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(suspension.budget_used.rounds, 1);
        assert_eq!(suspension.tool_call.name, "company_echo");
        assert_eq!(
            suspension.tail[0]["tool_calls"][0]["id"], "call-1",
            "the tail starts at the round that made the call"
        );
        before.db.close().await;
        drop(before);

        let after = boot(&db_path, &server.uri(), gated()).await;
        let resume = claim_for_resume(&after.db, &turn_id, None, Decision::AllowOnce)
            .await
            .unwrap();
        drive(&after, &session_id, Some(resume)).await;

        let done = the_turn(&after, &session_id).await;
        assert_eq!(done.turn.status, TurnStatus::Completed);
        assert_eq!(done.turn.content.as_deref(), Some("Echoed: ship it"));
        assert_eq!(done.suspension, None);
        assert_eq!(done.tool_calls.len(), 1);
        assert_eq!(done.tool_calls[0].status, ToolCallStatus::Completed);
        assert!(
            done.tool_calls[0]
                .output_json
                .as_deref()
                .unwrap()
                .contains("ship it")
        );

        let sent = requests(&server).await;
        assert_eq!(sent.len(), 2, "one round before the pause, one after");
        let messages = sent[1]["messages"].as_array().unwrap();
        let assistant = messages
            .iter()
            .position(|m| m["tool_calls"][0]["id"] == "call-1")
            .expect("the replayed round");
        assert_eq!(messages[assistant + 1]["role"], "tool");
        assert_eq!(messages[assistant + 1]["tool_call_id"], "call-1");
        assert!(
            messages[assistant + 1]["content"]
                .as_str()
                .unwrap()
                .contains("ship it")
        );
        assert_eq!(assistant + 2, messages.len());
        assert_eq!(messages[0]["role"], "system");
    }

    #[tokio::test]
    async fn a_denied_call_never_runs_and_the_model_reads_why() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("gateway.db");
        let server = upstream(vec![echo_call(), json!({"content": "Understood."})]).await;
        let state = boot(&db_path, &server.uri(), gated()).await;
        let (session_id, turn_id) = open_turn(&state).await;
        drive(&state, &session_id, None).await;

        let resume = claim_for_resume(
            &state.db,
            &turn_id,
            None,
            Decision::Deny {
                reason: DenyReason::User,
            },
        )
        .await
        .unwrap();
        drive(&state, &session_id, Some(resume)).await;

        let done = the_turn(&state, &session_id).await;
        assert_eq!(done.turn.status, TurnStatus::Completed);
        assert_eq!(done.tool_calls[0].status, ToolCallStatus::Errored);
        let output = done.tool_calls[0].output_json.as_deref().unwrap();
        assert!(output.contains("declined"), "{output}");
        let tool_message = last_message(&requests(&server).await[1]).clone();
        assert_eq!(tool_message["role"], "tool");
        assert!(
            tool_message["content"]
                .as_str()
                .unwrap()
                .contains("declined")
        );
        assert!(
            !tool_message["content"]
                .as_str()
                .unwrap()
                .contains("ship it")
        );
    }

    #[tokio::test]
    async fn a_timed_out_call_takes_the_deny_fallback_and_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("gateway.db");
        let server = upstream(vec![echo_call(), json!({"content": "Still waiting."})]).await;
        let state = boot(&db_path, &server.uri(), gated()).await;
        let (session_id, turn_id) = open_turn(&state).await;
        drive(&state, &session_id, None).await;

        let suspension = chat::get_suspension(&state.db, &turn_id)
            .await
            .unwrap()
            .unwrap();
        let resume = claim_for_resume(&state.db, &turn_id, None, suspension.on_timeout.decision())
            .await
            .unwrap();
        drive(&state, &session_id, Some(resume)).await;

        let done = the_turn(&state, &session_id).await;
        assert_eq!(done.turn.status, TurnStatus::Completed);
        assert_eq!(done.tool_calls[0].status, ToolCallStatus::Errored);
        let tool_message = last_message(&requests(&server).await[1]).clone();
        assert!(
            tool_message["content"]
                .as_str()
                .unwrap()
                .contains("expired")
        );
    }

    #[test]
    fn a_resumed_run_counts_the_seconds_spent_before_the_pause() {
        let now = std::time::Instant::now();
        assert_eq!(super::started_at(now, None), now);
        let resume = ResumeFrom {
            suspension: chat::TurnSuspension {
                turn_id: "t".into(),
                request_id: "r".into(),
                kind: chat::SuspensionKind::Approval,
                message: None,
                tool_call: chat::PendingCall {
                    id: "c".into(),
                    name: "n".into(),
                    arguments: "{}".into(),
                },
                tail: Vec::new(),
                budget_used: chat::BudgetUsed {
                    rounds: 2,
                    seconds: 40,
                    tokens: 900,
                },
                child_turn: None,
                on_timeout: chat::TimeoutFallback::Deny,
                expires_at: jiff::Timestamp::now(),
                created_at: jiff::Timestamp::now(),
                run_context: None,
            },
            decision: Decision::AllowOnce,
            child_result: None,
        };
        assert_eq!(
            now.duration_since(super::started_at(now, Some(&resume))),
            Duration::from_secs(40)
        );
    }

    /// The same round with an ungated tool: nothing is written, nothing waits,
    /// and the turn runs to its answer in one go — exactly as before.
    #[tokio::test]
    async fn a_turn_whose_tools_ask_for_nothing_runs_straight_through() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("gateway.db");
        let server = upstream(vec![echo_call(), json!({"content": "Echoed."})]).await;
        let state = boot(&db_path, &server.uri(), ToolRegistry::new().with(Echo)).await;
        let (session_id, turn_id) = open_turn(&state).await;
        drive(&state, &session_id, None).await;

        let done = the_turn(&state, &session_id).await;
        assert_eq!(done.turn.status, TurnStatus::Completed);
        assert_eq!(done.turn.content.as_deref(), Some("Echoed."));
        assert_eq!(done.tool_calls[0].status, ToolCallStatus::Completed);
        assert_eq!(
            chat::get_suspension(&state.db, &turn_id).await.unwrap(),
            None
        );
        assert_eq!(requests(&server).await.len(), 2);
    }

    /// A persona's turn (the agent architect's) is the person's turn with the
    /// persona's prompt and only its tools: the person's own tools are neither
    /// offered nor run.
    #[tokio::test]
    async fn a_personas_turn_offers_only_its_tools_and_refuses_the_persons() {
        let dir = tempfile::tempdir().unwrap();
        let server = upstream(vec![
            echo_call(),
            json!({"tool_calls": [{"index": 0, "id": "call-2", "type": "function",
                "function": {"name": "get_current_timestamp", "arguments": "{}"}}]}),
            json!({"content": "Planned."}),
        ])
        .await;
        let state = boot(
            &dir.path().join("gateway.db"),
            &server.uri(),
            ToolRegistry::new().with(Echo),
        )
        .await;
        let (session_id, _) = open_turn(&state).await;
        let persona = crate::persona::ChatPersona::new(
            "You are the agent architect.",
            Arc::new(ToolRegistry::new().with(crate::server::tools::time::CurrentTimestamp)),
        );
        drive_as(&state, &session_id, None, Some(Arc::new(persona))).await;

        let done = the_turn(&state, &session_id).await;
        assert_eq!(done.turn.status, TurnStatus::Completed);
        assert_eq!(done.turn.content.as_deref(), Some("Planned."));
        let calls: Vec<(&str, ToolCallStatus)> = done
            .tool_calls
            .iter()
            .map(|c| (c.name.as_str(), c.status))
            .collect();
        assert_eq!(
            calls,
            [
                ("company_echo", ToolCallStatus::Errored),
                ("get_current_timestamp", ToolCallStatus::Completed),
            ]
        );

        let sent = requests(&server).await;
        let offered: Vec<&str> = sent[0]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["function"]["name"].as_str().unwrap())
            .collect();
        assert_eq!(offered, ["get_current_timestamp"]);
        let system = sent[0]["messages"][0]["content"].as_str().unwrap();
        assert!(system.contains("You are the agent architect."), "{system}");
    }
}
