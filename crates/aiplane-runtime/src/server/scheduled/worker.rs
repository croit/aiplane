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

use super::{ScheduledAction, cron::Cron};
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
        if let Err(err) = super::set_next_run(&state.db, &action.id, next).await {
            tracing::warn!(action = %action.id, error = %err, "claiming scheduled action");
            continue;
        }
        let state = state.clone();
        tokio::spawn(async move {
            run_action(state, action, next).await;
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
async fn run_action(state: Arc<RamaState>, action: ScheduledAction, next: Option<Timestamp>) {
    // Open the history row first, so a run that is slow, or that dies with
    // the process, is still visible as *something that happened* rather than
    // as a gap. The startup sweep closes the row if the process dies.
    let run_id = match super::record_run_start(&state.db, &action.id).await {
        Ok(id) => Some(id),
        Err(err) => {
            tracing::warn!(action = %action.id, error = %err, "opening scheduled run history row");
            return;
        }
    };
    let (status, session_id, error) =
        guard_run(&action.id, execute_run(&state, &action, run_id.as_deref())).await;
    record(
        &state,
        &action,
        run_id.as_deref(),
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
    // The owner's RBAC roles gate the run's tools. A `None` here means the
    // user row vanished between selection and run (FK cascade should have
    // deleted the action too — defensive).
    let user = match aiplane_core::server::db::users::find_by_id(&state.db, &action.user_id).await {
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
            &action.user_id,
            &state.role_ids_for(&user.roles),
            None,
            &action.model,
            state.upstreams.enforce_limits_for_model(
                &action.model,
                aiplane_core::server::upstreams::PoolKind::Chat,
            ),
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
                    crate::server::background::reconcile_turn(&state.db, &action.user_id, &turn_id)
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
    if let Err(err) = super::mark_ran(&state.db, &action.id, status, session_id, next, error).await
    {
        tracing::warn!(action = %action.id, error = %err, "recording scheduled run");
    }
    if let Some(run_id) = run_id
        && let Err(err) = super::finish_run(&state.db, run_id, status, session_id, error).await
    {
        tracing::warn!(action = %action.id, error = %err, "closing scheduled run history row");
    }
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
    let existing_session = reusable_session(db, action).await?;
    crate::server::headless::open_session(
        db,
        crate::server::headless::OpenParams {
            owner: crate::server::headless::Owner::User(&action.user_id),
            title: &action.name,
            prompt: &action.prompt,
            model: &action.model,
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
    Ok(chat::get_session(db, &action.user_id, last)
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
            actor: crate::agent_run::Actor::person(action.user_id.clone(), user.roles.clone()),
            session_id: session_id.clone(),
            assistant_turn_id: assistant_turn_id.clone(),
            model: action.model.clone(),
            source: aiplane_core::server::db::usage::UsageSource::Scheduled,
            history_limit,
        },
    )
    .await;
    Ok((session_id, assistant_turn_id))
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
            user_id: "u1".to_string(),
            name: "Every minute".to_string(),
            prompt: "hi".to_string(),
            model: "qwen".to_string(),
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
}
