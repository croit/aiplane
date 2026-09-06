// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Hold a request while its pool is down, instead of failing it instantly.
//!
//! A GPU box that is restarting, an upstream that OOMs and comes back, a model
//! being swapped — these are seconds-to-minutes outages, and the request that
//! arrives during one is almost always still worth serving thirty seconds later.
//! Failing it immediately pushes the whole problem onto the client, and for an
//! agent client that is expensive: an interactive Claude Code turn that loses
//! its request loses the tool loop it was in the middle of.
//!
//! So [`route_or_wait`] parks: it retries the normal routing decision every
//! [`POLL_INTERVAL`] until a backend is healthy again or the budget runs out.
//! Nothing has been sent to the client at that point — not a byte, not a status
//! line — so a request that waits and then succeeds is indistinguishable from a
//! slow one, and the client's stream starts normally.
//!
//! What it deliberately does **not** do:
//!
//!   - **Wait on an unknown model.** A name no pool serves is a typo or a
//!     misconfiguration; waiting for it would turn a clear `404` into a long
//!     hang. Only [`RouteError::Acquire`] — "known model, no capacity right
//!     now" — parks.
//!   - **Wait forever.** The budget is bounded (see
//!     [`crate::server::config::GatewayConfig::upstream_wait_secs`]) and, when
//!     it expires, the caller still answers with a *retryable* status plus
//!     `Retry-After`, so the client's own backoff takes over from there. Parking
//!     is a shock absorber for short outages, not a substitute for the client
//!     giving up on a long one.
//!   - **Hold an in-flight slot.** No slot is acquired until routing succeeds,
//!     so parked requests cost a task and a timer, nothing on the backend.
//!
//! The recovery latency a parked request actually sees is the health probe's,
//! not this poll's: routing only turns healthy again once a probe says so. That
//! is why `health::DOWN_PROBE_INTERVAL` is tighter than the healthy cadence.

use std::time::Duration;

use tokio::time::{Instant, sleep};

use super::config::PoolKind;
use super::registry::{AcquireError, Acquired, PoolAccess, RouteError, UpstreamRegistry};

/// How often to re-attempt routing while parked. Short enough that recovery is
/// bounded by the probe cadence rather than by this, cheap enough to run per
/// parked request (one lock-free registry read).
const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// [`UpstreamRegistry::route_access`], but a pool-outage failure parks and
/// retries until `budget` is spent instead of failing the request.
///
/// Returns exactly what `route_access` would: `Ok` with an acquired slot, or
/// the last `RouteError`. A zero `budget` makes this a plain `route_access`.
/// `affinity` is passed straight through (see
/// [`crate::server::upstreams::affinity`]) — a request released after a wait
/// should still land on the replica holding its cache.
pub async fn route_or_wait(
    registry: &UpstreamRegistry,
    model: &str,
    kind: PoolKind,
    access: &PoolAccess,
    budget: Duration,
    affinity: Option<&super::affinity::AffinityHint>,
) -> Result<Acquired, RouteError> {
    let deadline = Instant::now() + budget;
    let mut parked_since: Option<Instant> = None;
    loop {
        let err = match registry.route_access_affine(model, kind, access, affinity) {
            Ok(acquired) => {
                if let Some(since) = parked_since {
                    tracing::info!(
                        model,
                        waited_ms = since.elapsed().as_millis() as u64,
                        "upstream recovered — releasing the parked request"
                    );
                }
                return Ok(acquired);
            }
            Err(e) => e,
        };
        // Only a capacity/health failure is worth waiting on; an unknown or
        // forbidden model will not become known by waiting.
        if !matches!(err, RouteError::Acquire(_)) || Instant::now() >= deadline {
            if let Some(since) = parked_since {
                tracing::warn!(
                    model,
                    waited_ms = since.elapsed().as_millis() as u64,
                    "upstream did not recover within the wait budget — failing the request"
                );
            }
            return Err(err);
        }
        if parked_since.is_none() {
            match &err {
                RouteError::Acquire(AcquireError::NoHealthyBackend { pool }) => {
                    tracing::warn!(
                        model,
                        pool,
                        budget_secs = budget.as_secs(),
                        "no healthy backend — parking the request until one returns"
                    );
                }
                _ => tracing::info!(
                    model,
                    budget_secs = budget.as_secs(),
                    "pool saturated — parking the request until a slot frees up"
                ),
            }
            parked_since = Some(Instant::now());
        }
        sleep(POLL_INTERVAL).await;
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use super::*;
    use crate::server::upstreams::config::{BackendConfig, PickerStrategy, UpstreamPoolConfig};

    fn pool_with_one_backend() -> HashMap<String, UpstreamPoolConfig> {
        let backend = BackendConfig {
            name: "b1".into(),
            base_url: "http://b1:8000/v1".into(),
            api_key_env: None,
            api_key: None,
            weight: 1,
            max_inflight: 1,
            health_path: "/models".into(),
            models: vec!["m1".into()],
            alias: None,
            probe_models: false,
            supports_edit: false,
            enabled: true,
        };
        HashMap::from([(
            "p".to_string(),
            UpstreamPoolConfig {
                kind: PoolKind::Chat,
                strategy: PickerStrategy::LeastInflight,
                fallback_offline: None,
                models: Vec::new(),
                compliance: Default::default(),
                enforce_limits: true,
                voices: HashMap::new(),
                offer_voices: Vec::new(),
                allowed_groups: Vec::new(),
                backend: vec![backend],
            },
        )])
    }

    /// An unknown model must fail immediately — waiting cannot make a name
    /// exist, and a client that sees a slow 404 has been told nothing useful.
    #[tokio::test]
    async fn an_unknown_model_does_not_park() {
        let reg = UpstreamRegistry::new(&pool_with_one_backend()).unwrap();
        let started = Instant::now();
        let err = route_or_wait(
            &reg,
            "nope",
            PoolKind::Chat,
            &PoolAccess::all(),
            Duration::from_secs(5),
            None,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, RouteError::UnknownModel(_)), "{err:?}");
        assert!(started.elapsed() < Duration::from_secs(1), "it waited");
    }

    /// A known model whose only backend is down parks for the whole budget and
    /// then reports the outage — the 503-shaped error, never a 404.
    #[tokio::test]
    async fn a_down_backend_parks_then_reports_the_outage() {
        let reg = UpstreamRegistry::new(&pool_with_one_backend()).unwrap();
        for pool in reg.pools() {
            for b in &pool.backends {
                b.set_healthy(false);
            }
        }
        let started = Instant::now();
        let err = route_or_wait(
            &reg,
            "m1",
            PoolKind::Chat,
            &PoolAccess::all(),
            Duration::from_millis(600),
            None,
        )
        .await
        .unwrap_err();
        assert!(
            matches!(
                err,
                RouteError::Acquire(AcquireError::NoHealthyBackend { .. })
            ),
            "{err:?}"
        );
        assert!(
            started.elapsed() >= Duration::from_millis(500),
            "it did not wait"
        );
    }

    /// The point of the whole module: a backend that comes back mid-wait serves
    /// the request, and the client never learns anything went wrong.
    #[tokio::test]
    async fn a_recovering_backend_releases_the_parked_request() {
        let reg = UpstreamRegistry::new(&pool_with_one_backend()).unwrap();
        for pool in reg.pools() {
            for b in &pool.backends {
                b.set_healthy(false);
            }
        }
        let recover = Arc::clone(&reg);
        tokio::spawn(async move {
            sleep(Duration::from_millis(300)).await;
            for pool in recover.pools() {
                for b in &pool.backends {
                    b.set_healthy(true);
                }
            }
        });
        let acquired = route_or_wait(
            &reg,
            "m1",
            PoolKind::Chat,
            &PoolAccess::all(),
            Duration::from_secs(10),
            None,
        )
        .await
        .expect("should have been released once the backend recovered");
        assert_eq!(acquired.resolved_model(), "m1");
    }
}
