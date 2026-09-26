// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Generic worker harness.
//!
//! Glue between session-core's `SessionDriver` trait and the on-disk
//! turn row that one assistant message corresponds to. The handler
//! that accepts a user message in HTTP is responsible for:
//!
//!   1. Persisting the user turn.
//!   2. Persisting the assistant turn (status `in_progress`).
//!   3. Reserving the per-user worker slot via `SessionWorkers`.
//!   4. Spawning `run_session_turn` on a tokio task with a driver +
//!      a `SessionContext` carrying the assistant turn id, the
//!      cancel flag, and the broadcast channel.
//!
//! `run_session_turn` then:
//!
//!   - Calls `driver.run_turn(ctx)`.
//!   - Translates the result + cancel flag into a final `TurnStatus`.
//!   - Stamps a reasoning-elapsed if the model reasoned but never
//!     emitted content (so the renderer shows a stable "Thought for
//!     Xs" instead of a frozen "Thinking…").
//!   - Calls `finalize_turn`.
//!   - Bumps `chat_sessions.updated_at` so the sidebar floats it to
//!     the top on the next render.
//!   - Broadcasts `TurnUpdate::Finalized` so attached HTTP
//!     subscribers send their final patch and close.

use std::sync::atomic::Ordering;
use std::{any::Any, panic::AssertUnwindSafe};

use rama::futures::FutureExt;

use crate::db::{self, Pool, TurnStatus};
use crate::driver::{SessionContext, SessionDriver};
use crate::i18n::{Lang, t};
use crate::workers::TurnUpdate;

/// Drive the lifecycle around one `SessionDriver::run_turn` call.
/// The caller wraps this in `tokio::spawn` so the HTTP handler that
/// accepted the user message doesn't have to wait. The `Pool` is
/// owned (clones are cheap — sqlx pools are `Arc` internally) so the
/// future can outlive the request scope.
pub async fn run_session_turn(pool: Pool, driver: Box<dyn SessionDriver>, ctx: SessionContext) {
    let result = AssertUnwindSafe(driver.run_turn(ctx.clone()))
        .catch_unwind()
        .await;

    let SessionContext {
        session_id,
        assistant_turn_id,
        cancel,
        broadcast,
        ..
    } = ctx;
    let result_is_panic = result.is_err();

    // Cancel-vs-natural-finish disambiguation. The driver's `Ok(())`
    // covers both natural finishes and clean cancels (the contract
    // is that drivers don't surface cancel as an error); the cancel
    // flag tells us which it was.
    let (status, error_message) = match result {
        Ok(Ok(_)) if cancel.load(Ordering::SeqCst) => (TurnStatus::Cancelled, None),
        // A finished turn may still carry a notice — see `TurnOutcome`. It goes
        // in the same column an error would, and the renderer tells them apart
        // by the row's status, so a turn that produced a real answer stays
        // `Completed` (replayable, webhook-ok, compactable) while still saying
        // out loud how it ended.
        Ok(Ok(outcome)) => (TurnStatus::Completed, outcome.notice),
        Ok(Err(err)) => {
            // The top-level `Display` often hides the real cause (e.g.
            // `DbError::Query`'s source sqlx error). Walk the full
            // `source()` chain into the log so a terse UI message like
            // "upstream: query" is always traceable server-side.
            let mut chain = err.to_string();
            let mut src = std::error::Error::source(&err);
            while let Some(e) = src {
                // Skip frames already embedded via thiserror's `{0}` so the
                // same cause isn't printed several times over.
                let s = e.to_string();
                if !chain.contains(&s) {
                    chain.push_str(": ");
                    chain.push_str(&s);
                }
                src = e.source();
            }
            tracing::error!(
                %session_id,
                %assistant_turn_id,
                error = %chain,
                "turn failed"
            );
            (TurnStatus::Errored, Some(err.to_string()))
        }
        Err(panic) => {
            let detail = panic_message(panic.as_ref());
            tracing::error!(%session_id, %assistant_turn_id, detail, "turn panicked");
            (
                TurnStatus::Errored,
                Some(t(Lang::En, "chat-error-turn-interrupted")),
            )
        }
    };

    // Reasoning timer cleanup. If the model emitted `reasoning_*`
    // chunks but never landed visible content (or the cancel
    // pre-empted the first content delta), the row's
    // `reasoning_elapsed_ms` is still NULL — the renderer would show a
    // forever-spinning "Thinking…" pseudo-state. Freeze it now so the
    // bubble reads "Thought for Xs" once the row finalises. Measured
    // from `reasoning_started_at` (the actual first reasoning chunk)
    // when present, falling back to `created_at` for legacy rows.
    if let Ok(Some(turn)) = db::list_turns(&pool, &session_id)
        .await
        .map(|turns| turns.into_iter().find(|t| t.turn.id == assistant_turn_id))
        && turn.turn.reasoning.is_some()
        && turn.turn.reasoning_elapsed_ms.is_none()
    {
        let anchor = turn
            .turn
            .reasoning_started_at
            .unwrap_or(turn.turn.created_at);
        let elapsed_ms = (jiff::Timestamp::now() - anchor).total(jiff::Unit::Millisecond);
        if let Ok(ms) = elapsed_ms {
            let _ = db::set_reasoning_elapsed(&pool, &assistant_turn_id, ms.max(0.0) as i64).await;
        }
    }

    let finalized = if result_is_panic {
        db::error_interrupted_turn(
            &pool,
            &assistant_turn_id,
            error_message.as_deref().unwrap_or_default(),
        )
        .await
        .map(|_| ())
    } else {
        db::finalize_turn(&pool, &assistant_turn_id, status, error_message.as_deref()).await
    };
    if let Err(err) = finalized {
        tracing::error!(%session_id, %assistant_turn_id, error = %err, "finalizing turn failed");
    }
    let _ = db::touch_session(&pool, &session_id).await;
    let _ = broadcast.send(TurnUpdate::Finalized);
}

fn panic_message(panic: &(dyn Any + Send)) -> &str {
    panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panic.downcast_ref::<&'static str>().copied())
        .unwrap_or("non-string panic payload")
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::driver::{TurnError, TurnOutcome};
    use crate::workers::{RegisterOutcome, SessionWorkers, SteerInbox};

    struct PanickingDriver {
        pool: Pool,
    }

    #[async_trait::async_trait]
    impl SessionDriver for PanickingDriver {
        async fn run_turn(&self, ctx: SessionContext) -> Result<TurnOutcome, TurnError> {
            db::insert_running_tool_call(
                &self.pool,
                &ctx.assistant_turn_id,
                "call-1",
                "fetch_url",
                "{}",
            )
            .await
            .unwrap();
            panic!("tool failed unexpectedly");
        }
    }

    #[tokio::test]
    async fn panicking_driver_finalizes_turn_and_releases_worker_slot() {
        let pool = db::tests::pool().await;
        let session = db::create_session(&pool, "u1").await.unwrap();
        db::create_assistant_turn_in_progress(&pool, &session.id, "turn-1", "model")
            .await
            .unwrap();
        let workers = Arc::new(SessionWorkers::default());
        let RegisterOutcome::Registered { worker } =
            workers.register("u1", "turn-1", &session.id, 1)
        else {
            panic!("expected worker registration");
        };
        let mut updates = worker.broadcast.subscribe();
        let ctx = SessionContext {
            user_id: Some("u1".into()),
            session_id: session.id.clone(),
            assistant_turn_id: "turn-1".into(),
            model: "model".into(),
            cancel: worker.cancel.clone(),
            broadcast: worker.broadcast.clone(),
            steers: SteerInbox::default(),
        };
        let worker_pool = pool.clone();
        let driver_pool = pool.clone();
        let worker_registry = workers.clone();
        tokio::spawn(async move {
            run_session_turn(
                worker_pool,
                Box::new(PanickingDriver { pool: driver_pool }),
                ctx,
            )
            .await;
            worker_registry.clear("u1", &worker);
        })
        .await
        .unwrap();

        assert_eq!(updates.try_recv().unwrap(), TurnUpdate::Finalized);
        let turns = db::list_turns(&pool, &session.id).await.unwrap();
        assert_eq!(turns[0].turn.status, TurnStatus::Errored);
        assert!(
            turns[0]
                .turn
                .error_message
                .as_deref()
                .unwrap()
                .contains("internal error")
        );
        assert_eq!(turns[0].tool_calls[0].status, db::ToolCallStatus::Errored);
        assert_eq!(workers.active_count(), 0);
        assert!(matches!(
            workers.register("u1", "turn-2", &session.id, 1),
            RegisterOutcome::Registered { .. }
        ));
    }
}
