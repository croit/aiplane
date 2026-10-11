// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The background loop that fires due scheduled actions.
//!
//! Mirrors the RAG indexer's shape (`spawn` → forever loop → `drain_once`
//! per tick). Each tick selects every action whose `next_run_at` has
//! passed, *claims* it (advances `next_run_at` to the next occurrence
//! before running, so a slow run or a crash can't double-fire), then
//! spawns the run. A run opens a fresh chat session and drives it
//! headlessly through the same `OpenAiDriver` the interactive `/chat`
//! path uses (see [`crate::openai_driver::build_tool_context`]) — so the
//! result is an ordinary conversation the user can open afterwards.
//!
//! Catch-up policy: if the gateway was down across one or more fire
//! times, `next_after(now)` jumps straight to the next *future*
//! occurrence, so the missed slots collapse into a single catch-up run on
//! the first tick after startup rather than a backlog burst.

use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::Duration;

use jiff::Timestamp;
use jiff::tz::TimeZone;
use rama::futures::FutureExt;
use session_core::db as chat;
use session_core::db::TurnStatus;

use super::{AutomationOwner, ScheduledAction, ScheduledRun, cron::Cron};
use crate::rama_server::state::RamaState;

/// How often to poll for due actions. Cron granularity is one minute, so
/// a 30s tick guarantees we never miss a minute boundary by more than the
/// tick itself.
const POLL_INTERVAL: Duration = Duration::from_secs(30);

/// Spawn the scheduler loop. Runs until the process exits. Never panics:
/// a failed pass is logged and retried on the next tick.
pub fn spawn(state: Arc<RamaState>) {
    tokio::spawn(async move {
        match super::sweep_interrupted_runs(&state.db).await {
            Ok(0) => {}
            Ok(swept) => tracing::info!(
                swept,
                "scheduled: closed runs a previous process left pending"
            ),
            Err(err) => {
                tracing::warn!(error = %err, "scheduled: closing interrupted runs at startup")
            }
        }
        loop {
            if let Err(err) = drain_once(&state).await {
                tracing::warn!(error = %err, "scheduled-actions pass failed");
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    });
}

/// One poll pass: claim every due action and spawn its run.
async fn drain_once(state: &Arc<RamaState>) -> Result<(), super::DbError> {
    let now = Timestamp::now();
    let due = super::due_actions(&state.db, now).await?;
    for action in due {
        // Compute the next occurrence and claim the slot *now*, before
        // running — concurrent ticks (or a restart) then see a future
        // `next_run_at` and won't re-select this action.
        let next = next_occurrence(&action, now);
        let Some(due_at) = action.next_run_at else {
            continue;
        };
        let mut tx = aiplane_core::server::db::WriteTx::begin(&state.db).await?;
        let claimed = super::claim_due_in(
            &mut tx,
            &action.id,
            due_at,
            &action.configuration_generation,
            next,
            now,
        )
        .await?;
        tx.commit().await?;
        let Some(run) = claimed else {
            continue;
        };
        let state = state.clone();
        tokio::spawn(async move {
            run_action(state, action, run, next).await;
        });
    }
    Ok(())
}

/// Parse the action's cron + timezone and return the first occurrence
/// strictly after `now`. `None` if the expression is invalid (defensive —
/// the UI validates on save) or unsatisfiable, which pauses future fires.
fn next_occurrence(action: &ScheduledAction, now: Timestamp) -> Option<Timestamp> {
    let cron = Cron::parse(&action.cron).ok()?;
    let tz = TimeZone::get(&action.timezone).unwrap_or(TimeZone::UTC);
    cron.next_after(now, &tz)
}

/// How one fire ended: `("ok" | "error", the chat it opened, error detail)`.
type RunOutcome = (&'static str, Option<String>, Option<String>);

/// What a run that panicked is closed with. The chat, if it got that far, is
/// already linked by then, and its turn carries the detail.
const RUN_CRASHED: &str = "the run stopped unexpectedly";

/// Run one scheduled action end-to-end: open a chat session, persist the
/// prompt + an in-progress assistant turn, drive it to completion
/// headlessly, then record the outcome.
async fn run_action(
    state: Arc<RamaState>,
    action: ScheduledAction,
    run: ScheduledRun,
    next: Option<Timestamp>,
) {
    let (status, session_id, error) =
        guard_run(&action.id, execute_run(&state, &action, Some(&run.id))).await;
    record(
        &state,
        &action,
        Some(&run.id),
        status,
        session_id.as_deref(),
        next,
        error.as_deref(),
    )
    .await;
}

/// Turn a panic anywhere in a run into an `error` outcome. Without it the
/// spawned task unwinds past `record`, and the run reads as running forever —
/// which is how a tool panic once left a run with no chat linked.
async fn guard_run(
    action_id: &str,
    run: impl std::future::Future<Output = RunOutcome>,
) -> RunOutcome {
    match AssertUnwindSafe(run).catch_unwind().await {
        Ok(outcome) => outcome,
        Err(panic) => {
            let detail = panic
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| panic.downcast_ref::<&'static str>().copied())
                .unwrap_or("non-string panic payload");
            tracing::error!(action = %action_id, detail, "scheduled run panicked");
            ("error", None, Some(RUN_CRASHED.to_string()))
        }
    }
}

async fn execute_run(
    state: &Arc<RamaState>,
    action: &ScheduledAction,
    run_id: Option<&str>,
) -> RunOutcome {
    let fail = |msg: &str| ("error", None, Some(msg.to_string()));
    if let AutomationOwner::Principal(_) = &action.owner {
        let Some(run_id) = run_id else {
            return fail("The scheduled run history is unavailable");
        };
        return match try_run_organization(state, run_id).await {
            Ok((session, turn)) => {
                let (status, error) = outcome_for(state, &session, &turn).await;
                (status, Some(session), error)
            }
            Err(error) => fail(&error.to_string()),
        };
    }
    let AutomationOwner::User(user_id) = &action.owner else {
        unreachable!()
    };
    let Some(model) = action.model.as_deref() else {
        return fail("The scheduled model is unavailable");
    };
    // The owner's RBAC roles gate the run's tools. A `None` here means the
    // user row vanished between selection and run (FK cascade should have
    // deleted the action too — defensive).
    let user = match aiplane_core::server::db::users::find_by_id(&state.db, user_id).await {
        Ok(Some(u)) => u,
        Ok(None) => return fail("schedule owner no longer exists"),
        Err(err) => {
            tracing::warn!(action = %action.id, error = %err, "loading schedule owner");
            return fail("could not load the schedule owner");
        }
    };
    // A scheduled fire draws from the owner's budget like any other call. If
    // they're over a limit, skip this fire and record it as an error (visible
    // in the schedule's run history) rather than running it for free.
    if state
        .limit_exceeded(
            user_id,
            &state.role_ids_for(&user.roles),
            None,
            model,
            state
                .upstreams
                .enforce_limits_for_model(model, aiplane_core::server::upstreams::PoolKind::Chat),
        )
        .await
        .is_some()
    {
        return fail("usage limit reached — run skipped");
    }
    match try_run_action(state, action, &user, run_id).await {
        Ok((session_id, assistant_turn_id)) => {
            let (status, error) = outcome_for(state, &session_id, &assistant_turn_id).await;
            (status, Some(session_id), error)
        }
        Err(err) => {
            let msg = err.to_string();
            tracing::warn!(action = %action.id, error = %msg, "scheduled run failed to start");
            fail(&msg)
        }
    }
}

/// Close out one fire: the action's denormalized `last_*` summary (what the
/// list row reads) and the run's history row (what the runs page reads) say
/// the same thing, so they are written together and never drift.
async fn record(
    state: &Arc<RamaState>,
    action: &ScheduledAction,
    run_id: Option<&str>,
    status: &str,
    session_id: Option<&str>,
    next: Option<Timestamp>,
    error: Option<&str>,
) {
    if let Some(run_id) = run_id {
        let turn: Result<Option<String>, sqlx::Error> =
            sqlx::query_scalar("SELECT assistant_turn_id FROM scheduled_runs WHERE id = ?")
                .bind(run_id)
                .fetch_optional(&state.db)
                .await
                .map(Option::flatten);
        match turn {
            Ok(Some(turn_id)) => {
                if status == "error"
                    && let Some(session_id) = session_id
                    && let Ok(Some(turn)) = chat::get_turn(&state.db, session_id, &turn_id).await
                    && turn.status == TurnStatus::InProgress
                {
                    let _ =
                        chat::finalize_turn(&state.db, &turn_id, TurnStatus::Errored, error).await;
                }
                if let Err(err) =
                    crate::server::background::reconcile_turn(&state.db, &action.owner, &turn_id)
                        .await
                {
                    tracing::warn!(action = %action.id, error = %err, "reconciling scheduled run history");
                }
                return;
            }
            Ok(None) => {}
            Err(err) => {
                tracing::warn!(action = %action.id, error = %err, "reading scheduled run correlation");
                return;
            }
        }
    }
    if let Some(run_id) = run_id {
        if let Err(err) = super::finish_run(&state.db, run_id, status, session_id, error).await {
            tracing::warn!(action=%action.id,%err,"closing scheduled run");
            return;
        }
        if let Err(err)=sqlx::query("UPDATE scheduled_actions SET last_run_at=(SELECT fired_at FROM scheduled_runs WHERE id=?),last_status=?,last_error=?,last_session_id=COALESCE(?,last_session_id) WHERE id=? AND configuration_generation=? AND (SELECT id FROM scheduled_runs WHERE action_id=scheduled_actions.id ORDER BY rtrim(fired_at,'Z') DESC,rowid DESC LIMIT 1)=?")
            .bind(run_id).bind(status).bind(error).bind(session_id).bind(&action.id).bind(&action.configuration_generation).bind(run_id).execute(&state.db).await {
            tracing::warn!(action=%action.id,%err,"recording scheduled startup result");
        }
    }
    let _ = next;
}

/// Open the chat session for one run and append the prompt + an in-progress
/// assistant turn. Returns `(session_id, assistant_turn_id)`. Delegates the
/// generic session-opening to [`crate::server::headless::open_session`]; this
/// wrapper only decides *which* session (fresh vs. reused).
///
/// In the default mode every call mints a brand-new session, so repeated
/// runs of the *same* action never collide — reusing the action id as the
/// turn id would PRIMARY KEY-clash on the second fire.
///
/// When `action.reuse_conversation` is set, the run instead appends into the
/// session the previous run opened (`last_session_id`), so prior runs become
/// conversation history. The first run (no prior session) and the
/// deleted-session case both fall back to minting a fresh session. The turn
/// ids are always freshly minted, so reuse never PRIMARY KEY-clashes either.
async fn open_run_session(
    db: &aiplane_core::server::db::Pool,
    action: &ScheduledAction,
) -> Result<(String, String), crate::server::headless::OpenError> {
    let AutomationOwner::User(user_id) = &action.owner else {
        return Err(crate::server::headless::OpenError::Unavailable);
    };
    let model = action
        .model
        .as_deref()
        .ok_or(crate::server::headless::OpenError::Unavailable)?;
    let existing_session = reusable_session(db, action).await?;
    crate::server::headless::open_session(
        db,
        crate::server::headless::OpenParams {
            owner: crate::server::headless::Owner::User(user_id),
            title: &action.name,
            prompt: &action.prompt,
            model,
            existing_session,
            dot_id: action.dot_id.as_deref(),
        },
    )
    .await
}

/// The previous run's session id, but only when reuse is enabled *and* that
/// session still exists (the user may have deleted the chat). `None` means
/// the caller should mint a fresh session.
async fn reusable_session(
    db: &aiplane_core::server::db::Pool,
    action: &ScheduledAction,
) -> Result<Option<String>, super::DbError> {
    if !action.reuse_conversation {
        return Ok(None);
    }
    let Some(last) = action.last_session_id.as_deref() else {
        return Ok(None);
    };
    let AutomationOwner::User(user_id) = &action.owner else {
        return Ok(None);
    };
    Ok(chat::get_session(db, user_id, last)
        .await?
        .map(|_| last.to_string()))
}

/// The fallible core of a run: everything up to and including driving the
/// turn to completion. Returns `(session_id, assistant_turn_id)` so the
/// outcome can be recorded against the run (and linked from the UI).
async fn try_run_action(
    state: &Arc<RamaState>,
    action: &ScheduledAction,
    user: &aiplane_core::server::db::users::User,
    run_id: Option<&str>,
) -> Result<(String, String), crate::server::headless::OpenError> {
    let (session_id, assistant_turn_id) = open_run_session(&state.db, action).await?;
    let history_limit = action
        .reuse_conversation
        .then(|| (action.reuse_rounds.max(0) as usize).saturating_mul(2));
    let Some(run_id) = run_id else {
        let error =
            "The scheduled run has no persisted history. Retry after run history can be stored.";
        chat::finalize_turn(
            &state.db,
            &assistant_turn_id,
            TurnStatus::Errored,
            Some(error),
        )
        .await
        .map_err(super::DbError::from)?;
        return Err(crate::server::headless::OpenError::Storage(
            super::DbError::Decode {
                column: "scheduled_run",
                source: std::io::Error::other(error.to_string()).into(),
            },
        ));
    };
    if let Err(error) = super::attach_run_turn(
        &state.db,
        run_id,
        &session_id,
        &assistant_turn_id,
        action.tools_enabled,
        history_limit,
    )
    .await
    {
        let _ = chat::finalize_turn(
            &state.db,
            &assistant_turn_id,
            TurnStatus::Errored,
            Some(&error.to_string()),
        )
        .await;
        return Err(error.into());
    }
    crate::server::headless::drive(
        state,
        crate::server::headless::DriveParams {
            actor: crate::agent_run::Actor::person(user.id.clone(), user.roles.clone()),
            session_id: session_id.clone(),
            assistant_turn_id: assistant_turn_id.clone(),
            model: action
                .model
                .clone()
                .ok_or(crate::server::headless::OpenError::Unavailable)?,
            source: aiplane_core::server::db::usage::UsageSource::Scheduled,
            history_limit,
        },
    )
    .await;
    Ok((session_id, assistant_turn_id))
}

async fn try_run_organization(
    state: &Arc<RamaState>,
    run_id: &str,
) -> Result<(String, String), crate::agents::profile::AgentRunError> {
    use crate::agents::profile::{AgentRunError, RunProfile};
    use crate::server::headless::{self, OpenError, OpenParams, Owner};
    use aiplane_core::server::run_chain::{Frame, RunChain};
    let profile = RunProfile::load_scheduled(state, run_id, &Default::default()).await?;
    let unavailable =
        || AgentRunError::OrganizationExecutionUnavailable(profile.principal.id.clone());
    let mut tx = aiplane_core::server::db::WriteTx::begin(&state.db).await?;
    let (action, run) = super::get_for_run_in(&mut tx, run_id)
        .await?
        .ok_or_else(unavailable)?;
    if !action.enabled
        || action.owner != AutomationOwner::Principal(profile.principal.id.clone())
        || run.configuration_generation.as_deref() != Some(action.configuration_generation.as_str())
        || run.agent_version != Some(profile.version)
        || action.agent_version != Some(profile.version)
    {
        return Err(unavailable());
    }
    let (session, turn) = headless::open_principal_session_in(
        &mut tx,
        OpenParams {
            owner: Owner::Run {
                principal_id: &profile.principal.id,
                parent_turn_id: None,
                agent_version: Some(profile.version),
            },
            title: &action.name,
            prompt: &action.prompt,
            model: &profile.model,
            existing_session: None,
            dot_id: None,
        },
    )
    .await
    .map_err(|error| match error {
        OpenError::Storage(db) => AgentRunError::Db(db),
        _ => unavailable(),
    })?;
    super::attach_run_turn_in(&mut tx, run_id, &session, &turn, run.tools_enabled, None).await?;
    let chain = Arc::new(RunChain::root(
        &session,
        None,
        Frame::for_principal(&profile.principal, None),
    ));
    let params = profile.drive_params(&session, &turn, chain)?;
    let crate::agent_run::Actor::Agent(agent) = &params.actor else {
        unreachable!()
    };
    crate::agents::profile::validate_organization_turn_in(&mut tx, agent).await?;
    tx.commit().await?;
    headless::drive(state, params).await;
    Ok((session, turn))
}

/// Classify a finished run by reading its assistant turn's final status.
/// Returns `("ok" | "waiting" | "error", Option<error message>)`; `waiting`
/// is a run that paused for a decision its owner gives in the inbox.
async fn outcome_for(
    state: &RamaState,
    session_id: &str,
    turn_id: &str,
) -> (&'static str, Option<String>) {
    match chat::get_turn(&state.db, session_id, turn_id).await {
        Ok(Some(turn)) => match turn.status {
            TurnStatus::Completed => ("ok", None),
            TurnStatus::Suspended => ("waiting", None),
            _ => (
                "error",
                turn.error_message
                    .or(Some("run did not complete".to_string())),
            ),
        },
        Ok(None) => ("error", Some("no assistant turn produced".to_string())),
        Err(_) => ("error", Some("could not read run result".to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::scheduled::{NewAction, create};
    use aiplane_core::server::db::users::User;

    async fn fresh_db() -> aiplane_core::server::db::Pool {
        aiplane_core::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap()
    }

    async fn seed_user(pool: &aiplane_core::server::db::Pool, id: &str) {
        let now = Timestamp::now();
        aiplane_core::server::db::users::upsert(
            pool,
            &User {
                id: id.to_string(),
                email: format!("{id}@example.com"),
                name: None,
                roles: vec![],
                created_at: now,
                updated_at: now,
                timezone: Some("Europe/Berlin".to_string()),
                speech_voice: None,
            },
        )
        .await
        .unwrap();
    }

    fn new_action(reuse_conversation: bool) -> NewAction {
        NewAction {
            owner: AutomationOwner::User("u1".to_string()),
            created_by: "u1".to_string(),
            agent_version: None,
            name: "Every minute".to_string(),
            prompt: "hi".to_string(),
            model: Some("qwen".to_string()),
            cron: "* * * * *".to_string(),
            timezone: "Europe/Berlin".to_string(),
            tools_enabled: false,
            reuse_conversation,
            reuse_rounds: 5,
            next_run_at: None,
            dot_id: None,
            linked_session_id: None,
        }
    }

    #[tokio::test]
    async fn a_panicking_run_still_yields_an_error_outcome() {
        let outcome = guard_run("a1", async { panic!("byte index is not a char boundary") }).await;
        assert_eq!(outcome, ("error", None, Some(RUN_CRASHED.to_string())));
    }

    #[tokio::test]
    async fn a_run_that_returns_passes_its_outcome_through() {
        let outcome = guard_run("a1", async { ("ok", Some("sess-1".to_string()), None) }).await;
        assert_eq!(outcome, ("ok", Some("sess-1".to_string()), None));
    }

    /// Default mode: each fire of the same action must open a fresh session +
    /// turns. The original bug reused the action id as the assistant turn id,
    /// so the second run PRIMARY KEY-clashed on `chat_turns.id` and never ran.
    #[tokio::test]
    async fn repeated_runs_open_distinct_sessions_and_turns() {
        let pool = fresh_db().await;
        seed_user(&pool, "u1").await;
        let action = create(&pool, new_action(false)).await.unwrap();

        let (s1, t1) = open_run_session(&pool, &action).await.unwrap();
        // Second fire of the SAME action — must not collide.
        let (s2, t2) = open_run_session(&pool, &action).await.unwrap();

        assert_ne!(s1, s2, "each run gets its own session");
        assert_ne!(t1, t2, "each run gets its own assistant turn");
        assert_ne!(
            t1, action.id,
            "assistant turn id must not reuse the action id"
        );
        assert_ne!(t2, action.id);
    }

    /// Reuse mode: the second fire must land in the *same* session the first
    /// run opened (so prior runs become history), but still mint a fresh
    /// assistant turn. The action's `last_session_id` is what the worker
    /// stamps via `mark_ran` between runs, so we simulate that here.
    #[tokio::test]
    async fn reuse_mode_second_run_reuses_session_distinct_turns() {
        let pool = fresh_db().await;
        seed_user(&pool, "u1").await;
        let mut action = create(&pool, new_action(true)).await.unwrap();

        let (s1, t1) = open_run_session(&pool, &action).await.unwrap();
        // The worker records the opened session on the action between fires.
        action.last_session_id = Some(s1.clone());
        chat::finalize_turn(&pool, &t1, TurnStatus::Completed, None)
            .await
            .unwrap();
        let (s2, t2) = open_run_session(&pool, &action).await.unwrap();

        assert_eq!(s1, s2, "reuse mode keeps the same session across runs");
        assert_ne!(t1, t2, "each run still gets its own assistant turn");
    }

    /// Reuse mode falls back to a fresh session when the prior chat was
    /// deleted (a stale `last_session_id` must not error the run).
    #[tokio::test]
    async fn reuse_mode_falls_back_when_prior_session_gone() {
        let pool = fresh_db().await;
        seed_user(&pool, "u1").await;
        let mut action = create(&pool, new_action(true)).await.unwrap();
        action.last_session_id = Some("does-not-exist".to_string());

        let (s, _t) = open_run_session(&pool, &action).await.unwrap();
        assert_ne!(s, "does-not-exist", "stale pointer must not be reused");
        // The freshly minted session exists and is owned by the user.
        assert!(chat::get_session(&pool, "u1", &s).await.unwrap().is_some());
    }
    #[tokio::test]
    async fn reuse_refuses_a_running_or_waiting_turn_without_adding_prompt() {
        let pool = fresh_db().await;
        seed_user(&pool, "u1").await;
        let mut action = create(&pool, new_action(true)).await.unwrap();
        let (session, turn) = open_run_session(&pool, &action).await.unwrap();
        action.last_session_id = Some(session.clone());
        assert!(matches!(
            open_run_session(&pool, &action).await,
            Err(crate::server::headless::OpenError::Busy)
        ));
        chat::finalize_turn(&pool, &turn, TurnStatus::Suspended, None)
            .await
            .unwrap();
        assert!(matches!(
            open_run_session(&pool, &action).await,
            Err(crate::server::headless::OpenError::Busy)
        ));
        assert_eq!(chat::list_turns(&pool, &session).await.unwrap().len(), 2);
    }
    async fn organization_schedule_fixture(
        state: &Arc<RamaState>,
    ) -> (super::super::ScheduledAction, super::super::ScheduledRun) {
        use aiplane_agents::db::agents;
        use aiplane_core::server::db::WriteTx;
        let (profile, _) = crate::server::headless::tests::organization_draft_fixture(state).await;
        let spec = serde_json::json!({"main":{"model":profile.model,"instructions":{"orchestration":"PINNED SCHEDULE INSTRUCTIONS"}}});
        assert_eq!(
            agents::publish(
                &state.db,
                &profile.principal.id,
                &spec.to_string(),
                "creator"
            )
            .await
            .unwrap(),
            Some(1)
        );
        let due = Timestamp::now();
        let action = create(
            &state.db,
            NewAction {
                owner: super::super::AutomationOwner::Principal(profile.principal.id),
                created_by: "creator".into(),
                dot_id: None,
                agent_version: Some(1),
                name: "Organization schedule".into(),
                prompt: "Scheduled task".into(),
                model: None,
                cron: "* * * * *".into(),
                timezone: "UTC".into(),
                tools_enabled: true,
                reuse_conversation: false,
                reuse_rounds: 0,
                next_run_at: Some(due),
                linked_session_id: None,
            },
        )
        .await
        .unwrap();
        let mut tx = WriteTx::begin(&state.db).await.unwrap();
        let run = super::super::claim_due_in(
            &mut tx,
            &action.id,
            due,
            &action.configuration_generation,
            None,
            due,
        )
        .await
        .unwrap()
        .unwrap();
        tx.commit().await.unwrap();
        (action, run)
    }

    #[tokio::test]
    async fn organization_scheduled_fire_uses_the_pinned_principal_model_after_creator_deletion() {
        use wiremock::{
            Mock, MockServer, ResponseTemplate,
            matchers::{method, path},
        };
        let upstream = MockServer::start().await;
        Mock::given(method("POST")).and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_raw("data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Scheduled reply\"}}]}\n\ndata: [DONE]\n\n","text/event-stream"))
            .mount(&upstream).await;
        let base = crate::server::headless::tests::state_for(&upstream.uri()).await;
        let state = Arc::new(RamaState::new(
            (**base).clone(),
            aiplane_core::rama_server::SessionStore::new(base.db.clone(), [7u8; 32]),
            aiplane_core::server::usage::spawn(base.db.clone(), 90),
        ));
        let (action, run) = organization_schedule_fixture(&state).await;
        sqlx::query("DELETE FROM users WHERE id='creator'")
            .execute(&state.db)
            .await
            .unwrap();
        let (status, session, error) = execute_run(&state, &action, Some(&run.id)).await;
        assert_eq!(status, "ok", "{error:?}");
        let session = session.unwrap();
        let super::super::AutomationOwner::Principal(principal) = &action.owner else {
            panic!("principal expected");
        };
        let stored =
            aiplane_agents::db::run_sessions::get_principal_session(&state.db, principal, &session)
                .await
                .unwrap()
                .unwrap();
        assert_eq!(stored.agent_version, Some(1));
        let rows: (Option<String>, bool) =
            sqlx::query_as("SELECT user_id,shared FROM chat_sessions WHERE id=?")
                .bind(&session)
                .fetch_one(&state.db)
                .await
                .unwrap();
        assert_eq!(rows, (None, false));
        let requests = upstream.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        let request: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(request["model"], "model-a");
        assert!(
            request["messages"]
                .to_string()
                .contains("PINNED SCHEDULE INSTRUCTIONS")
        );
        assert!(
            !request["messages"]
                .to_string()
                .contains("creator@example.com")
        );
        tokio::time::sleep(std::time::Duration::from_millis(700)).await;
        let usage: Vec<(String, String, String, Option<String>)> =
            sqlx::query_as("SELECT user_id,principal_kind,source,agent_id FROM usage_events")
                .fetch_all(&state.db)
                .await
                .unwrap();
        assert!(!usage.is_empty());
        for (payer, kind, source, agent) in usage {
            assert_eq!(payer, *principal);
            assert_eq!(kind, "system");
            assert_eq!(source, "scheduled");
            assert_eq!(agent.as_deref(), Some(principal.as_str()));
        }
    }

    #[tokio::test]
    async fn organization_scheduled_fire_rechecks_enabled_and_generation_before_opening() {
        let upstream = wiremock::MockServer::start().await;
        for mutation in ["disable", "generation"] {
            let state = crate::server::headless::tests::state_for(&upstream.uri()).await;
            let (action, run) = organization_schedule_fixture(&state).await;
            let sql = if mutation == "disable" {
                "UPDATE scheduled_actions SET enabled=0 WHERE id=?"
            } else {
                "UPDATE scheduled_actions SET configuration_generation='new-authored-generation' WHERE id=?"
            };
            sqlx::query(sql)
                .bind(&action.id)
                .execute(&state.db)
                .await
                .unwrap();
            let (status, session, _) = execute_run(&state, &action, Some(&run.id)).await;
            assert_eq!(status, "error", "{mutation}");
            assert!(session.is_none());
            let correlation: Option<String> =
                sqlx::query_scalar("SELECT assistant_turn_id FROM scheduled_runs WHERE id=?")
                    .bind(&run.id)
                    .fetch_one(&state.db)
                    .await
                    .unwrap();
            assert!(correlation.is_none());
        }
        assert!(upstream.received_requests().await.unwrap().is_empty());
    }
}
