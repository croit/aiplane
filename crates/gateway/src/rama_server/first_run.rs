// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! First-run gate: until the setup wizard has completed, the gateway's HTML
//! surface is nothing but the wizard.
//!
//! # Why a layer and not a check in each handler
//!
//! This started as three hand-placed checks — in `require_session_or_redirect`,
//! in the `/login` page, and in `/auth/login`. Between the first two, one route
//! was missed, and `GET /auth/login` on a fresh gateway answered `500 OIDC is
//! not configured` instead of pointing at the wizard. That is the shape of the
//! bug this design has: a *new* route is unprotected by default, and nothing
//! reminds its author, because the symptom only appears on a machine nobody has
//! configured yet.
//!
//! Inverting it fixes the default. Everything is gated unless it appears in
//! [`serves_before_setup`], so forgetting to think about a new route now means
//! it redirects to the wizard on an unconfigured gateway — visible, harmless,
//! and fixed by adding one line. The old default was to serve.
//!
//! # What still answers
//!
//! See [`serves_before_setup`]. Three groups: the wizard and the page chrome it
//! is rendered with, the probe callback the wizard's test login lands on, and
//! the non-HTML surfaces that already refuse everything on a gateway with no
//! accounts.
//!
//! # This is first-run only
//!
//! The gate keys on [`AppState::setup_completed`], which stays `true` for a
//! configured gateway even while `restore-setup` has reopened `/setup`. A
//! recovery window must not redirect anyone: an admin who cannot sign in asking
//! for help must never take a working deployment offline for everybody else.
//!
//! [`AppState::setup_completed`]: gateway_runtime::server::state::AppState::setup_completed

use std::convert::Infallible;
use std::sync::Arc;

use rama::http::{Body, Request, Response, StatusCode, header};
use rama::{Layer, Service};

use crate::rama_server::RamaState;

/// Whether `path` keeps working on a gateway that has not been set up.
///
/// An allowlist, deliberately: see the [module docs](self).
fn serves_before_setup(path: &str) -> bool {
    // The wizard's client route. Its API (below) decides whether it may be
    // *used* (first run, recovery window, or gone) — see `setup_api`.
    if path == "/setup" || path.starts_with("/setup/") {
        return true;
    }
    // The wizard proves a provider by running a real authorization-code round
    // trip, and the IdP redirects back here. Gating this would leave the wizard
    // unable to finish the one thing it exists to do.
    if path == "/auth/callback" {
        return true;
    }
    // The SPA's static shell, served from disk at the root: the content-hashed
    // bundles SvelteKit emits under `/_app/`, plus the loose files in its build
    // directory. The wizard *is* the SPA now, so gating these would 303 a
    // JavaScript module request to an HTML page and leave the wizard blank.
    // They are unauthenticated static files; the SPA's own API calls
    // self-protect with 401/403.
    if path.starts_with("/_app/") || path.starts_with("/icons/") {
        return true;
    }
    if matches!(
        path,
        "/favicon.ico"
            | "/favicon.svg"
            | "/manifest.webmanifest"
            | "/sw.js"
            | "/robots.txt"
            | "/pcm-recorder.js"
    ) {
        return true;
    }
    // The setup wizard's own API — the one surface that must work while the
    // gateway is unconfigured (that is its entire job).
    if path == "/api/v0/setup/state"
        || path == "/api/v0/setup/test"
        || path == "/api/v0/setup/restart"
        || path == "/api/v0/setup/finish"
    {
        return true;
    }
    // Debug-only dev/e2e seeding (see `rama_server::dev_seed`). These are
    // how a fresh dev database *becomes* set up, so gating them would be
    // self-blocking. The routes only exist in debug builds; on a release
    // binary this exemption answers a route that was never registered,
    // which falls through to the ordinary 404.
    if cfg!(debug_assertions) && (path == "/__dev/seed-session" || path == "/__dev/session") {
        return true;
    }
    // Liveness must answer while the operator is still in the wizard;
    // readiness answers *and reports not-ready* (see the `/readyz` handler).
    if matches!(path, "/healthz" | "/readyz" | "/openapi.json") {
        return true;
    }
    // Non-HTML surfaces answer for themselves. A 303 to an HTML wizard is the
    // wrong reply to an API call, and on a gateway with no accounts no token,
    // session or webhook secret can exist yet — so they already refuse
    // everything, with a status their callers understand.
    path == "/v1"
        || path.starts_with("/v1/")
        || path.starts_with("/api/v0/")
        || path.starts_with("/hooks/")
}

/// [`Layer`] that wraps a service with [`FirstRun`]. Apply it outside the
/// router's error handler, so an unmatched path on an unconfigured gateway
/// lands on the wizard rather than on a 404 that explains nothing.
#[derive(Clone)]
pub struct FirstRunLayer {
    state: Arc<RamaState>,
}

impl FirstRunLayer {
    pub fn new(state: Arc<RamaState>) -> Self {
        Self { state }
    }
}

impl<S> Layer<S> for FirstRunLayer {
    type Service = FirstRun<S>;

    fn layer(&self, inner: S) -> Self::Service {
        FirstRun {
            inner,
            state: self.state.clone(),
        }
    }

    fn into_layer(self, inner: S) -> Self::Service {
        FirstRun {
            inner,
            state: self.state,
        }
    }
}

/// Redirects everything but [`serves_before_setup`] to `/setup` while the
/// gateway is unconfigured. See the [module docs](self).
#[derive(Clone)]
pub struct FirstRun<S> {
    inner: S,
    state: Arc<RamaState>,
}

impl<S> Service<Request> for FirstRun<S>
where
    S: Service<Request, Output = Response, Error = Infallible>,
{
    type Output = Response;
    type Error = Infallible;

    async fn serve(&self, req: Request) -> Result<Self::Output, Self::Error> {
        // One `ArcSwap` load on the configured path — cheaper than the routing
        // that follows it, and it is the only cost this layer adds once the
        // gateway is in service.
        if self.state.setup_completed() || serves_before_setup(req.uri().path()) {
            return self.inner.serve(req).await;
        }
        // 303 rather than 307: a POST that arrives before setup (a stale tab,
        // a bookmarked form) should be re-issued as a GET of the wizard, not
        // replayed against it.
        Ok(Response::builder()
            .status(StatusCode::SEE_OTHER)
            .header(header::LOCATION, "/setup")
            .body(Body::empty())
            .expect("static redirect response"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wizard_and_the_shell_it_needs_are_reachable() {
        // The wizard is a client route of the SPA, so everything the SPA shell
        // loads to render it has to answer too. If one of these starts
        // redirecting, the operator gets an HTML page where a script was
        // expected and the wizard never paints.
        for path in [
            "/setup",
            "/_app/immutable/entry/start.CkY1xNvY.js",
            "/_app/version.json",
            "/favicon.svg",
            "/favicon.ico",
            "/manifest.webmanifest",
            "/sw.js",
            "/icons/icon-192.png",
            "/api/v0/setup/state",
            "/api/v0/setup/test",
            "/api/v0/setup/finish",
            "/openapi.json",
        ] {
            assert!(serves_before_setup(path), "{path} must serve before setup");
        }
    }

    #[test]
    fn the_probe_callback_is_reachable_but_login_is_not() {
        // The asymmetry is the point. `/auth/callback` is where the wizard's
        // own test login comes back, so gating it would break setup itself.
        // `/auth/login` is a sign-in nobody can complete yet, and answering it
        // with a 500 was the bug that motivated this layer.
        assert!(serves_before_setup("/auth/callback"));
        assert!(!serves_before_setup("/auth/login"));
    }

    #[test]
    fn api_surfaces_answer_for_themselves() {
        for path in [
            "/v1",
            "/v1/chat/completions",
            "/v1/models",
            "/api/v0/me",
            "/hooks/abc123",
        ] {
            assert!(
                serves_before_setup(path),
                "{path} must get its own 401/404, not an HTML redirect"
            );
        }
    }

    #[test]
    fn dev_seeding_answers_on_a_fresh_database() {
        // The seed endpoints are what flips a fresh dev database to
        // setup-completed, so the first-run gate must let them through —
        // otherwise they could never do their one job. Debug builds only.
        if !cfg!(debug_assertions) {
            return;
        }
        for path in ["/__dev/seed-session", "/__dev/session"] {
            assert!(
                serves_before_setup(path),
                "{path} must reach its handler before setup completes"
            );
        }
    }

    #[test]
    fn every_spa_route_is_gated() {
        // Client routes of the SPA, which resolve to `index.html` once setup
        // is done. Before that they belong at the wizard.
        for path in [
            "/",
            "/chat",
            "/tokens",
            "/usage",
            "/login",
            "/admin/users",
            "/rag",
            "/memory",
            "/skills",
            "/webhooks",
            "/scheduled",
            "/integrations",
            "/tools",
            "/feedback",
        ] {
            assert!(!serves_before_setup(path), "{path} must redirect to /setup");
        }
    }

    #[test]
    fn a_prefix_match_cannot_be_walked_out_of() {
        // `starts_with` on a path that merely *begins* with an exempt name must
        // not open the gate — `/setupfoo` and `/v1foo` are not `/setup/…` or
        // `/v1/…`, and an unknown path is gated like any other page.
        for path in ["/setupfoo", "/v1foo", "/_appfoo", "/hooksfoo", "/nonsense"] {
            assert!(!serves_before_setup(path), "{path} must not be exempt");
        }
    }
}
