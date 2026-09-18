// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Debug-only session + fixture seeding for local development and the e2e
//! suite. Compiled exclusively under `cfg(debug_assertions)` — a release
//! binary registers neither route.
//!
//! Two endpoints, deliberately:
//!
//! * `GET /__dev/seed-session` — resets the canonical e2e fixture (the
//!   `alice` user and her three tokens), completes setup, and signs in as
//!   `alice`. It *deletes* tokens, so it is unsafe to call while other
//!   tests are mid-assertion: only the file that owns the canonical row
//!   counts (`e2e/authed.test.mjs`) may call it, and it reseeds before
//!   every test.
//! * `GET /__dev/session` — signs in as `alice` (creating her if absent)
//!   and completes setup, touching nothing else. Idempotent and
//!   delete-free, so any test file can call it in `before()` without
//!   racing the file that counts rows — which matters because
//!   `node --test` runs test files in parallel.
//!
//! Both are exempt from the first-run gate (they are how a fresh dev
//! database *becomes* set up) and both mint ordinary sessions through the
//! real `SessionStore`, so every subsequent request flows through the
//! exact production auth path.

use std::sync::Arc;

use jiff::Timestamp;
use rama::http::service::web::extract::State;
use rama::http::{Response, StatusCode, header};

use aiplane_core::rama_server::session::secure_cookies;
use aiplane_core::server::db::Pool;
use aiplane_core::server::db::{tokens, users};
use aiplane_core::server::setup;
use aiplane_runtime::rama_server::state::RamaState;

/// The fixture identity every seeded session signs in as.
pub const USER_ID: &str = "alice";
pub const EMAIL: &str = "alice@example.com";

/// OIDC roles the fixture user carries — arbitrary but non-empty, so
/// role-derived UI (the account card, admin surfaces) has something to
/// render.
const ROLES: &[&str] = &["engineering", "admin"];

/// The canonical token fixture: three rows, two active + one revoked,
/// newest first — the exact state `e2e/authed.test.mjs` counts. Ages are
/// backdated so the rendered metadata lines look lived-in; the ids are
/// fixed so a test can target `#token-row-{id}` across reseeds.
/// (name, id, created N days ago, mark used, revoke)
const TOKEN_FIXTURE: &[(&str, &str, i64, bool, bool)] = &[
    ("Local laptop", "devseed-laptop", 4, false, false),
    ("CI pipeline", "devseed-ci", 15, false, true),
    ("Production API", "devseed-prod", 42, true, false),
];

/// Upsert the fixture user. Returns nothing; the caller signs in as
/// [`USER_ID`] afterwards.
pub async fn ensure_user(pool: &Pool) -> anyhow::Result<()> {
    let now = Timestamp::now();
    users::upsert(
        pool,
        &users::User {
            id: USER_ID.into(),
            email: EMAIL.into(),
            name: None,
            roles: ROLES.iter().map(|r| (*r).to_string()).collect(),
            created_at: now,
            updated_at: now,
            timezone: None,
            speech_voice: None,
        },
    )
    .await?;
    Ok(())
}

/// Replace the fixture user's tokens with the canonical three (creating the
/// user if absent — the tokens table's foreign key requires her). Only the
/// `/__dev/seed-session` endpoint calls this — see the module docs for why
/// everything else must stay delete-free.
pub async fn reset_tokens(pool: &Pool) -> anyhow::Result<()> {
    ensure_user(pool).await?;
    tokens::delete_for_user(pool, USER_ID).await?;
    let now = Timestamp::now();
    for (name, id, age_days, used, revoked) in TOKEN_FIXTURE {
        tokens::insert(
            pool,
            &tokens::Token {
                id: (*id).to_string(),
                user_id: USER_ID.into(),
                name: (*name).to_string(),
                // A throwaway that matches no mintable plaintext: the
                // fixture is for rendering, never for authenticating.
                hash: format!("devseed-hash-{id}"),
                created_at: now - jiff::SignedDuration::from_hours(24 * age_days),
                last_used_at: None,
                expires_at: now + jiff::SignedDuration::from_hours(24 * 90),
                revoked_at: None,
                tools_enabled: true,
            },
        )
        .await?;
        if *used {
            tokens::touch(pool, id).await?;
        }
        if *revoked {
            let revoked = tokens::revoke(pool, USER_ID, id).await?;
            anyhow::ensure!(revoked, "revoking the just-inserted fixture token {id}");
        }
    }
    Ok(())
}

/// 303 that signs the caller in as the fixture user and lands them on
/// `target`.
async fn signed_in(state: &RamaState, target: &str) -> anyhow::Result<Response> {
    let session = state
        .sessions
        .create(USER_ID)
        .await
        .map_err(|err| anyhow::anyhow!("minting the dev session: {err}"))?;
    let cookie = state
        .sessions
        .cookie(&session.id, secure_cookies(&state.public_url()));
    Ok(Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header(header::LOCATION, target)
        .header(header::SET_COOKIE, cookie)
        .body("".into())
        .expect("static redirect response"))
}

/// A 500 that says what the seeding was doing when it failed — this
/// endpoint's users are developers staring at a test-runner failure. Logged
/// too, because the response alone is easy to miss when the caller is a
/// `before()` hook.
fn failed(what: &str, err: anyhow::Error) -> Response {
    tracing::warn!(what, error = %err, "dev seeding failed");
    Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(format!("dev seeding failed while {what}: {err:#}").into())
        .expect("static text response")
}

/// GET `/__dev/session` — sign in as the fixture user without resetting
/// anything. Idempotent; safe to call concurrently with any test.
pub async fn sign_in(State(state): State<Arc<RamaState>>) -> Response {
    if let Err(err) = ensure_user(&state.db).await {
        return failed("upserting the fixture user", err);
    }
    if !state.setup_completed() {
        if let Err(err) = setup::mark_completed(&state.db).await {
            return failed("marking setup complete", err.into());
        }
        state.reload_runtime().await;
    }
    match signed_in(&state, "/").await {
        Ok(resp) => resp,
        Err(err) => failed("minting the session", err),
    }
}

/// GET `/__dev/seed-session` — reset the canonical fixture, then sign in.
pub async fn reset(State(state): State<Arc<RamaState>>) -> Response {
    if let Err(err) = reset_tokens(&state.db).await {
        return failed("resetting the token fixture", err);
    }
    if !state.setup_completed() {
        if let Err(err) = setup::mark_completed(&state.db).await {
            return failed("marking setup complete", err.into());
        }
        state.reload_runtime().await;
    }
    match signed_in(&state, "/").await {
        Ok(resp) => resp,
        Err(err) => failed("minting the session", err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiplane_core::server::db::open;
    use std::path::Path;

    async fn fresh() -> Pool {
        open(Path::new(":memory:")).await.unwrap()
    }

    #[tokio::test]
    async fn the_token_reset_is_canonical_and_idempotent() {
        let pool = fresh().await;
        for _ in 0..2 {
            reset_tokens(&pool).await.unwrap();
            let rows = tokens::list_for_user(&pool, USER_ID).await.unwrap();
            assert_eq!(
                rows.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
                ["Local laptop", "CI pipeline", "Production API"],
                "newest first, exactly the canonical three — no leftovers, no dupes"
            );
            assert_eq!(
                rows.iter().filter(|t| t.revoked_at.is_none()).count(),
                2,
                "two active"
            );
            assert_eq!(
                rows.iter().filter(|t| t.revoked_at.is_some()).count(),
                1,
                "one revoked"
            );
            assert!(
                rows.iter().all(|t| t.expires_at > Timestamp::now()),
                "the fixture must not ship pre-expired"
            );
        }
    }

    #[tokio::test]
    async fn the_reset_only_takes_the_fixture_users_tokens() {
        let pool = fresh().await;
        ensure_user(&pool).await.unwrap();
        let now = Timestamp::now();
        users::upsert(
            &pool,
            &users::User {
                id: "bob".into(),
                email: "bob@example.com".into(),
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
        tokens::insert(
            &pool,
            &tokens::Token {
                id: "bobs".into(),
                user_id: "bob".into(),
                name: "Bobs token".into(),
                hash: "h".into(),
                created_at: now,
                last_used_at: None,
                expires_at: now + jiff::SignedDuration::from_hours(24),
                revoked_at: None,
                tools_enabled: false,
            },
        )
        .await
        .unwrap();

        reset_tokens(&pool).await.unwrap();

        assert_eq!(tokens::list_for_user(&pool, "bob").await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn the_fixture_user_carries_roles_the_ui_renders() {
        let pool = fresh().await;
        ensure_user(&pool).await.unwrap();
        let user = users::find_by_id(&pool, USER_ID).await.unwrap().unwrap();
        assert_eq!(user.email, EMAIL);
        assert_eq!(
            user.roles,
            vec!["engineering".to_string(), "admin".to_string()]
        );
    }
}
