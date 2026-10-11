// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The internal test chat (`docs/agent-builder.md` → "Test chat"): a manager talks
//! to the agent's **draft** in a conversation that streams like any other,
//! and reads per turn what a visitor never sees — slot values and
//! provenance, each route's gate, the routing decision, sub-agent calls and
//! tool-call decisions.
//!
//! - `POST /api/v0/agents/{id}/test/messages` opens the turn, claims the
//!   conversation and answers `202 {session_id, turn_id}` at once; the turn
//!   runs in the background on the real run path ([`start_draft_turn`]): the
//!   draft's grants, gates, binds and budgets apply, and its tools really run
//!   as the agent's principal, so it needs a `write` share. Omitting
//!   `session_id` starts a conversation; passing one continues it.
//! - `GET /api/v0/agents/{id}/test/{session}/events` is the conversation's
//!   `chat_json` stream, the same frames a person's chat gets.
//! - `GET …/test/{session}/turns/{turn}/debug` is that turn's debug view,
//!   read from the agent's activity log once the turn has settled, with the
//!   hand-off it waits on when it waits for a person.
//!
//! `POST /api/v0/agents/{id}/conversations/{session}/turns/{turn}/resume` is
//! the staff side of a suspended agent run: an approval (or a human answer)
//! for any of the agent's conversations, and any decision in a test
//! conversation, where the manager is the visitor too. `202` once the turn
//! runs again; the rest arrives on the conversation's stream.
//!
//! A test conversation is recorded as version [`DRAFT_VERSION`], so its pauses
//! never reach the Inbox; the manager answers them in the test chat.

use session_core::i18n::{Lang, t};
use std::sync::Arc;
use std::time::Duration;

use rama::http::service::web::extract::State;
use rama::http::{Request, Response, StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};

use super::json_agents::{agent_at, parse_spec};
use super::json_principals::require_agent_manager;
use super::{bad_request, internal, json_error, json_ok, not_found, raw_path_segment};
use aiplane_agents::db::agents::{self as agents_db, Access, AgentRow};
use aiplane_agents::db::run_sessions;
use aiplane_runtime::agents::embed::{self as embed_rt, TurnWork};
use aiplane_runtime::agents::profile::{AgentRunError, RunOptions};
use aiplane_runtime::agents::run::AgentTurn;
use aiplane_runtime::agents::run::draft::{
    DRAFT_VERSION, DraftDebug, collect_turn_debug, run_draft_turn, start_draft_turn,
    waiting_handoff,
};
use aiplane_runtime::rama_server::state::RamaState;
use session_core::db as chat;

/// How long the debug view waits for a turn that has ended but is not yet
/// settled (the output filter rules after the turn's row is final).
const DEBUG_SETTLE: Duration = Duration::from_secs(15);

#[derive(Deserialize, schemars::JsonSchema)]
pub struct TestMessageBody {
    pub message: String,
    /// The test conversation to continue; a new one when absent.
    #[serde(default)]
    pub session_id: Option<String>,
}

/// The test turn that now runs; its frames arrive on the conversation's
/// event stream.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct TestTurnStarted {
    pub session_id: String,
    pub turn_id: String,
    /// The version a test conversation is recorded as.
    pub draft_version: i64,
}

/// A test turn's debug view.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct TurnDebugView {
    pub turn_id: String,
    pub draft_version: i64,
    pub debug: DraftDebug,
    /// The hand-off the turn waits on, when it waits for a person.
    pub handoff: Option<Value>,
}

fn run_error(err: AgentRunError, lang: Lang) -> Response {
    if matches!(&err, AgentRunError::OrganizationExecutionUnavailable(_)) {
        return json_error(
            StatusCode::FORBIDDEN,
            "organization_execution_unavailable",
            &t(lang, "organization-error-execution-unavailable"),
        );
    }
    let (status, code) = match &err {
        AgentRunError::OrganizationExecutionUnavailable(_) => {
            unreachable!("organization refusal returned before classification")
        }
        AgentRunError::Unavailable(_) => (StatusCode::NOT_FOUND, "agent_unavailable"),
        AgentRunError::UnknownSession { .. } => (StatusCode::NOT_FOUND, "unknown_session"),
        AgentRunError::DecisionPending { .. } => (StatusCode::CONFLICT, "decision_pending"),
        AgentRunError::Busy { .. } => (StatusCode::CONFLICT, "turn_in_progress"),
        AgentRunError::NotLive(_)
        | AgentRunError::BadSpec { .. }
        | AgentRunError::MissingVersion { .. } => {
            (StatusCode::UNPROCESSABLE_ENTITY, "agent_not_runnable")
        }
        AgentRunError::NoModel { .. } => (StatusCode::SERVICE_UNAVAILABLE, "agent_no_model"),
        AgentRunError::ModelNotGranted { .. } => {
            (StatusCode::UNPROCESSABLE_ENTITY, "agent_model_not_granted")
        }
        AgentRunError::Db(_) | AgentRunError::Mismatched(_) => return internal(err),
    };
    json_error(status, code, &err.to_string())
}

fn runtime_unavailable() -> Response {
    json_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "agent_runtime_unavailable",
        "this gateway cannot run agent conversations yet — try again after the gateway has been \
         updated",
    )
}

fn empty_message() -> Response {
    bad_request("a test message needs a non-empty `message`")
}

/// POST /api/v0/agents/{id}/test/messages
pub async fn send_message(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let lang = Lang::from_request(req.headers());
    let user = or_return!(require_agent_manager(&state, &req).await);
    let (agent, _) = or_return!(agent_at(&state, &req, &user, 2, Access::Write).await);
    let owner = or_return!(
        agents_db::dot_owner(&state.db, &agent.principal.id)
            .await
            .map_err(internal)
    );
    if matches!(owner, Some(agents_db::DotOwner::Organization(_))) {
        return run_error(
            AgentRunError::OrganizationExecutionUnavailable(agent.principal.id.clone()),
            lang,
        );
    }
    let body: TestMessageBody =
        or_return!(super::read_json(req.into_body(), "the test message").await);
    if body.message.trim().is_empty() {
        return empty_message();
    }
    let Some(runner) = state.agent_runner.clone() else {
        return runtime_unavailable();
    };
    let draft = parse_spec(&agent.draft_spec);
    let started = match start_draft_turn(
        &state,
        test_turn(&agent, &body.message, body.session_id.as_deref()),
        &draft,
        &RunOptions::default(),
    )
    .await
    {
        Ok(started) => started,
        Err(err) => return run_error(err, lang),
    };
    let out = TestTurnStarted {
        session_id: started.turn.session_id.clone(),
        turn_id: started.turn.turn_id.clone(),
        draft_version: DRAFT_VERSION,
    };
    embed_rt::spawn_guarded(
        state,
        runner,
        started.claim,
        TurnWork::Draft(Box::new(started.profile), started.turn),
    );
    json_ok(StatusCode::ACCEPTED, out)
}

fn test_turn<'a>(agent: &'a AgentRow, message: &'a str, session: Option<&'a str>) -> AgentTurn<'a> {
    AgentTurn {
        agent_id: &agent.principal.id,
        session_id: session,
        message,
        visitor_id: None,
        lang: None,
    }
}

/// `agent`'s test conversation `session_id`, or the response saying there is
/// none: a visitor's conversation is never a test one.
async fn test_conversation(
    state: &RamaState,
    agent: &AgentRow,
    session_id: &str,
) -> Result<(), Response> {
    let run = run_sessions::get_principal_session(&state.db, &agent.principal.id, session_id)
        .await
        .map_err(internal)?;
    if run.is_some_and(|r| r.agent_version == Some(DRAFT_VERSION)) {
        return Ok(());
    }
    Err(not_found(format!(
        "agent `{}` has no test conversation `{session_id}` — start a new one",
        agent.principal.name
    )))
}

/// GET /api/v0/agents/{id}/test/{session}/events
pub async fn events(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let user = or_return!(require_agent_manager(&state, &req).await);
    let (agent, _) = or_return!(agent_at(&state, &req, &user, 3, Access::Write).await);
    let Some(session_id) = raw_path_segment(&req, 1) else {
        return bad_request("the URL is missing the conversation id");
    };
    or_return!(test_conversation(&state, &agent, &session_id).await);
    match state.chats.get(&agent.principal.id, &session_id) {
        Some(worker) => super::chat::json_api::live_stream(&state, &worker, session_id).await,
        None => match chat::list_turns(&state.db, &session_id).await {
            Ok(turns) => super::chat::json_api::quiet_stream(turns),
            Err(err) => internal(err),
        },
    }
}

/// GET /api/v0/agents/{id}/test/{session}/turns/{turn}/debug
pub async fn turn_debug(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let user = or_return!(require_agent_manager(&state, &req).await);
    let (agent, _) = or_return!(agent_at(&state, &req, &user, 5, Access::Write).await);
    let (Some(session_id), Some(turn_id)) = (raw_path_segment(&req, 3), raw_path_segment(&req, 1))
    else {
        return bad_request("the URL is missing the conversation or the turn id");
    };
    or_return!(test_conversation(&state, &agent, &session_id).await);
    let settled = tokio::time::timeout(
        DEBUG_SETTLE,
        embed_rt::released(&state.chats, &agent.principal.id, &session_id, &turn_id),
    )
    .await;
    if settled.is_err() {
        return json_error(
            StatusCode::CONFLICT,
            "turn_in_progress",
            "this turn is still running — read its debug view once the conversation's stream \
             says it ended",
        );
    }
    match chat::get_turn(&state.db, &session_id, &turn_id).await {
        Ok(Some(turn)) if turn.role == chat::TurnRole::Assistant => {}
        Ok(_) => {
            return not_found(format!(
                "conversation `{session_id}` has no answer `{turn_id}`"
            ));
        }
        Err(err) => return internal(err),
    }
    match turn_view(&state, &agent, &session_id, &turn_id).await {
        Ok(view) => json_ok(StatusCode::OK, view),
        Err(resp) => resp,
    }
}

/// Turn `turn_id`'s debug view, and the hand-off it waits on, if any.
async fn turn_view(
    state: &RamaState,
    agent: &AgentRow,
    session_id: &str,
    turn_id: &str,
) -> Result<TurnDebugView, Response> {
    let draft = parse_spec(&agent.draft_spec);
    let debug = collect_turn_debug(
        state,
        &agent.principal.id,
        &draft,
        session_id,
        turn_id,
        &RunOptions::default(),
    )
    .await
    .map_err(internal)?;
    let handoff = waiting_handoff(&state.db, turn_id)
        .await
        .map_err(internal)?;
    Ok(TurnDebugView {
        turn_id: turn_id.to_string(),
        draft_version: DRAFT_VERSION,
        debug,
        handoff,
    })
}

/// One test turn against `agent`'s draft, run to its end, with its debug
/// view: the architect's `run_test_turn`, which is itself a step of a turn.
pub(super) async fn draft_test_turn(
    state: &Arc<RamaState>,
    agent: &AgentRow,
    message: &str,
    session_id: Option<&str>,
) -> Result<Value, Response> {
    if message.trim().is_empty() {
        return Err(empty_message());
    }
    let draft = parse_spec(&agent.draft_spec);
    let reply = run_draft_turn(
        state,
        test_turn(agent, message, session_id),
        &draft,
        RunOptions::default(),
    )
    .await
    .map_err(|error| run_error(error, Lang::En))?;
    let view = turn_view(state, agent, &reply.session_id, &reply.turn_id).await?;
    Ok(json!({
        "session_id": reply.session_id,
        "turn_id": reply.turn_id,
        "status": reply.status,
        "answer": reply.answer,
        "error": reply.error,
        "suspension": reply.suspension,
        "debug": view.debug,
        "handoff": view.handoff,
    }))
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StaffResumeBody {
    pub decision: chat::DecisionKind,
    /// The decision's value, where its kind takes one.
    #[serde(default)]
    pub value: Option<Value>,
    /// The `request_id` of the `suspended` frame being answered. Optional;
    /// when given, an answer to an earlier pause is refused.
    #[serde(default)]
    pub request_id: Option<String>,
}

/// POST /api/v0/agents/{id}/conversations/{session}/turns/{turn}/resume —
/// answer what a suspended agent turn waits for, as staff, and continue it.
///
/// `write` share (or admin). `202 {turn_id}` once the turn runs again, as the
/// inbox's answer does: the turn continues on the conversation's stream, the
/// visitor's and the test chat's alike.
pub async fn resume_turn(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let lang = session_core::i18n::Lang::from_request(req.headers());
    let user = or_return!(require_agent_manager(&state, &req).await);
    let (agent, _) = or_return!(agent_at(&state, &req, &user, 5, Access::Write).await);
    let (Some(session_id), Some(turn_id)) = (raw_path_segment(&req, 3), raw_path_segment(&req, 1))
    else {
        return bad_request("the URL is missing the conversation or the turn id");
    };
    let body: StaffResumeBody =
        or_return!(super::read_json(req.into_body(), "the resume body").await);
    let decision = match super::chat::json_api::decision_from(body.decision, body.value) {
        Ok(decision) => decision,
        Err(msg) => return bad_request(msg),
    };
    super::json_inbox::resume_as_staff(
        state,
        super::json_inbox::StaffDecision {
            lang,
            agent_id: &agent.principal.id,
            session_id: &session_id,
            turn_id: &turn_id,
            request_id: body.request_id.as_deref(),
            decision,
            user_id: &user.id,
        },
    )
    .await
}
