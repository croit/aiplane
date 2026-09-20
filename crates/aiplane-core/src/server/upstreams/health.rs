// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Per-backend health checker + model discovery.
//!
//! Each backend gets one background task that pings `<base_url><health_path>`
//! (default `/models`) every 5 s with a 2 s timeout. The probe does two
//! jobs from one round-trip:
//!
//!   1. **Liveness** — three consecutive failures flip the backend to
//!      unhealthy; one success flips it back. The picker in `registry.rs`
//!      skips unhealthy backends.
//!   2. **Model discovery** — on every success the response body is parsed
//!      as the OpenAI `/models` envelope (`{"data": [{"id": "..."}, ...]}`)
//!      and the backend's advertised-model set is replaced wholesale. The
//!      router in `acquire_for` reads that set to decide which pool handles
//!      a given model. No static route table.
//!
//! Bootstrap: `spawn` is `async` because it does one initial parallel probe
//! round and awaits it before returning. That way the gateway doesn't start
//! serving traffic with empty model sets — the first `POST /v1/chat/
//! completions` lands on a registry that already knows what's where.

use std::sync::Arc;
use std::time::Duration;

use tokio::time::sleep;

use super::profile;
use super::registry::{Backend, UpstreamRegistry};
use crate::server::db::{Pool, upstreams_config};

const PROBE_INTERVAL: Duration = Duration::from_secs(5);
/// Probe cadence while a backend is *known down*. Tighter than
/// [`PROBE_INTERVAL`] because this is the interval that decides how long a
/// client parked in `wait_for_route` keeps waiting after the upstream is
/// actually back: at 5 s a recovered backend stayed invisible for up to five
/// seconds of dead air on every request. Cheap — one GET against an upstream
/// that is, by definition, not serving traffic.
const DOWN_PROBE_INTERVAL: Duration = Duration::from_secs(1);
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const FAILURE_THRESHOLD: u32 = 3;
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
/// How many backends are identified at once. Each round is five concurrent
/// GETs, so this is really a cap of `5 × N` in-flight requests on a client
/// that deliberately pools no connections — and the backend count is entirely
/// operator-controlled.
const DETECT_CONCURRENCY: usize = 8;

/// reqwest client for health probes: NO idle connection pooling. Probes fire
/// every [`PROBE_INTERVAL`], so a pooled keep-alive would sit idle between
/// them and get closed by the upstream/NAT — the next probe would reuse a dead
/// connection and fail with a spurious "connection reset" while the backend is
/// actually serving. A fresh connection per probe costs nothing at this
/// cadence and removes that whole class of false alarms. (Real traffic uses
/// the pooled `state.http` client.)
fn probe_client() -> reqwest::Client {
    reqwest::Client::builder()
        .pool_max_idle_per_host(0)
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

/// Spawns one background task per backend. Awaits an initial parallel
/// probe round before returning, so the registry has at least one
/// model-set update per reachable backend before traffic starts.
pub async fn spawn(registry: Arc<UpstreamRegistry>, db: Option<Pool>) {
    let http = probe_client();
    // The generation this call is *for*, read before anything is awaited.
    //
    // Reading it afterwards let two overlapping applies both arm loops for the
    // newest topology: A parks for up to two seconds, B bumps the generation
    // and arms its loops, then A resumes, reads B's generation and B's pools,
    // and arms a *second* loop for every backend. Both then pass the
    // retirement check forever, so each backend is probed twice as often by
    // two concurrent writers — permanently, and a double-click on Apply is
    // enough to cause it.
    let generation = registry.generation();
    let pools = registry.pools();
    if let Some(db) = db.as_ref() {
        seed_remembered_models(&registry, db).await;
        seed_remembered_detection(&registry, db).await;
    }
    // Identification runs *alongside* the first probe round rather than before
    // it. The two ask different servers different questions and neither needs
    // the other's answer, so serialising them would have added a second
    // timeout's worth of delay to every boot and every topology apply that
    // happens to include an unreachable backend — a cost paid on exactly the
    // occasions an operator is already waiting and already unhappy.
    let detection = tokio::spawn({
        let http = http.clone();
        let registry = Arc::clone(&registry);
        let db = db.clone();
        async move { detect_all(&http, &registry, db.as_ref()).await }
    });
    let mut initial = Vec::new();
    for pool in &pools {
        for backend in &pool.backends {
            let http = http.clone();
            let pool_name = pool.name.clone();
            let backend = Arc::clone(backend);
            let db = db.clone();
            initial.push(tokio::spawn(async move {
                let outcome = probe_once(&http, &pool_name, &backend, db.as_ref()).await;
                // A backend that is already unreachable at startup must not
                // start out `healthy` (the field's initial value): the router
                // would send it real traffic for the first three probe rounds
                // and every one of those requests would fail at the socket.
                // We just proved it is down, so say so — the loop below flips
                // it back on the first success, ~1 s later.
                if matches!(outcome, ProbeOutcome::Failed(_)) {
                    backend.set_healthy(false);
                }
            }));
        }
    }
    // Block startup until every backend has been probed at least once
    // (or its probe has timed out at 2 s). Failures here aren't fatal —
    // the looping probe will retry, and an unreachable backend just
    // stays out of routing decisions until it recovers.
    for handle in initial {
        let _ = handle.await;
    }
    // Detection is awaited too, so the admin page reflects the apply that just
    // finished rather than filling in a moment later, and so the first request
    // through a freshly-applied topology already knows how to phrase itself.
    let _ = detection.await;

    // Now arm the looping probe per backend. Each loop owns its own
    // failure counter — the bootstrap probe above doesn't pre-seed it
    // because mid-startup flaps shouldn't permanently mark a backend
    // unhealthy. Each loop is tagged with the topology generation it was
    // spawned for; a `reload()` bumps the generation and the loop retires
    // itself, so re-spawning here after a reload doesn't leak the previous
    // generation's loops (which hold detached `Backend` Arcs).
    //
    // A newer apply landing while this one was awaiting means these loops
    // would retire on their first tick anyway — and that apply arms its own.
    // Skipping them keeps a burst of applies from briefly doubling the probe
    // rate.
    if registry.generation() != generation {
        tracing::debug!(
            generation,
            current = registry.generation(),
            "a newer topology was applied while this one was starting up; \
             leaving its probe loops to it"
        );
        return;
    }
    for pool in &pools {
        for backend in &pool.backends {
            let backend = Arc::clone(backend);
            let pool_name = pool.name.clone();
            let http = http.clone();
            let registry = Arc::clone(&registry);
            let db = db.clone();
            tokio::spawn(async move {
                run_probe(http, pool_name, backend, registry, generation, db).await;
            });
        }
    }
}

/// Periodic liveness heartbeat. Per-probe failures are silent while a backend
/// stays healthy (only transitions log), so "quiet logs" no longer prove the
/// gateway is alive vs. hung. This emits one line every
/// [`HEARTBEAT_INTERVAL`] that affirmatively says the process is running and
/// summarises upstream health at a glance — INFO when all backends are
/// healthy, WARN when degraded so a filtered log still surfaces it. The first
/// line is emitted immediately at startup. Fire-and-forget; the task itself
/// running is the liveness proof.
pub fn spawn_heartbeat(registry: Arc<UpstreamRegistry>) {
    tokio::spawn(async move {
        loop {
            let mut total = 0usize;
            let mut healthy = 0usize;
            let mut per_pool: Vec<String> = Vec::new();
            for pool in registry.pools() {
                let p_total = pool.backends.len();
                let p_healthy = pool.backends.iter().filter(|b| b.is_healthy()).count();
                total += p_total;
                healthy += p_healthy;
                per_pool.push(format!("{}={p_healthy}/{p_total}", pool.name));
            }
            let pools = per_pool.join(" ");
            if total == 0 {
                tracing::info!("gateway alive — no upstream backends configured");
            } else if healthy == total {
                tracing::info!(
                    backends = format!("{healthy}/{total} healthy"),
                    pools = %pools,
                    "gateway alive — all upstreams healthy"
                );
            } else {
                tracing::warn!(
                    backends = format!("{healthy}/{total} healthy"),
                    pools = %pools,
                    "gateway alive — DEGRADED, some upstreams down"
                );
            }
            sleep(HEARTBEAT_INTERVAL).await;
        }
    });
}

/// Seed every backend that has no live model set yet from what it was last seen
/// serving (`backend_probed_models`).
///
/// Runs before the first probe round, so a gateway booting while an upstream is
/// down still *knows* which models that upstream serves. That is the difference
/// between `503 no healthy backend` (an outage; clients retry) and `404
/// model_not_found` (a typo; clients give up and tell the user the model does
/// not exist). It grants no health: routing still requires a successful probe,
/// which is the only thing that can set `healthy`.
///
/// Only fills *empty* sets, so it never overrides a set carried across a
/// topology reload, and the first successful probe replaces it wholesale.
async fn seed_remembered_models(registry: &UpstreamRegistry, db: &Pool) {
    let remembered = match upstreams_config::load_probed_models(db).await {
        Ok(m) if !m.is_empty() => m,
        Ok(_) => return,
        Err(err) => {
            tracing::debug!(error = %err, "could not load remembered model sets");
            return;
        }
    };
    for pool in registry.pools() {
        for backend in &pool.backends {
            if !backend.probe_models_enabled() {
                continue;
            }
            if !backend.live_models().is_empty() {
                continue;
            }
            let Some(models) = remembered.get(&backend.name) else {
                continue;
            };
            tracing::info!(
                pool = %pool.name, backend = %backend.name, models = models.len(),
                "seeded last-known model set (not yet probed; backend stays unhealthy until it answers)"
            );
            backend.set_models(models.clone());
        }
    }
}

/// Identify every backend once, in parallel, and record what it said.
///
/// Runs here rather than in the 5 s probe loop because this is exactly the
/// moment the operator means by "save": the admin UI applies a topology by
/// calling `topology_reload`, which rebuilds the registry and calls [`spawn`].
/// What a server *is* changes when someone reconfigures it, which is to say
/// almost never — five requests per backend per tick would be five requests to
/// learn nothing.
///
/// What a server currently *has loaded* is a different question on a different
/// clock, and [`probe_once`] reads that every tick. So a context setting
/// changed on the upstream is picked up within five seconds without anyone
/// applying anything; it is the *profile* that waits for the next apply.
///
async fn detect_all(http: &reqwest::Client, registry: &UpstreamRegistry, db: Option<&Pool>) {
    // Bounded fan-out. Each round is five concurrent GETs and each backend
    // gets its own round, so an unbounded loop turns a fifty-backend apply
    // into a ~250-socket burst against a client that pools nothing.
    let permits = Arc::new(tokio::sync::Semaphore::new(DETECT_CONCURRENCY));
    let mut rounds = Vec::new();
    for pool in registry.pools() {
        for backend in &pool.backends {
            // A drained backend takes no traffic, so nothing depends on
            // knowing what it is — and identification is five requests, not
            // the probe's one. It keeps whatever was seeded from the database
            // and is identified on the apply that brings it back.
            if !backend.is_enabled() {
                continue;
            }
            let http = http.clone();
            let backend = Arc::clone(backend);
            let pool_name = pool.name.clone();
            let permits = Arc::clone(&permits);
            rounds.push(tokio::spawn(async move {
                let _permit = permits.acquire().await.ok()?;
                let detected =
                    profile::detect(&http, &backend.base_url, backend.api_key.as_deref())
                        .await
                        // Stamped here rather than by the database write, so the
                        // live registry carries the same time the row does — the
                        // admin page reads the registry, and was showing `null`
                        // for every backend it had actually identified.
                        .map(|d| profile::Detected {
                            detected_at: Some(jiff::Timestamp::now().to_string()),
                            ..d
                        });

                // Nothing answered. Keep what we already knew — an unreachable
                // server is not a plain OpenAI server, and writing that
                // conclusion would undo `seed_remembered_detection` and the
                // reload carry-forward milliseconds after they ran, then
                // persist the loss. A gateway restarted while the Ollama box
                // was rebooting would forget it was Ollama, permanently.
                let Some(detected) = detected else {
                    tracing::info!(
                        pool = %pool_name, backend = %backend.name,
                        "backend did not answer identification; keeping the profile it had"
                    );
                    return None;
                };

                tracing::info!(
                    pool = %pool_name, backend = %backend.name,
                    profile = detected.profile.as_str(),
                    version = detected.version.as_deref().unwrap_or("-"),
                    context_windows = detected.context_windows.len(),
                    max_parallel = ?detected.max_parallel,
                    "identified backend"
                );
                if detected.profile == profile::BackendProfile::Generic {
                    // Not an error, and — unlike an unanswered round — a real
                    // answer, so it is allowed to replace a stale profile.
                    // That is what lets a backend repointed from an Ollama box
                    // to a hosted API stop being treated as Ollama.
                    tracing::info!(
                        pool = %pool_name, backend = %backend.name,
                        "backend answered but is none of the server kinds we know; treating it \
                         as plain OpenAI-compatible (no separate context endpoint, reasoning \
                         spelling guessed from the model name)"
                    );
                }
                backend.set_detected(&detected);
                Some((backend.name.clone(), backend.base_url.clone(), detected))
            }));
        }
    }
    for round in rounds {
        match round.await {
            Ok(Some((name, base_url, detected))) => {
                if let Some(db) = db
                    && let Err(err) =
                        upstreams_config::save_detected(db, &name, &base_url, &detected).await
                {
                    // A failure here means the profile does not survive a
                    // restart, and every model on that backend drops to the
                    // assumed window on the next boot. Not debug-level.
                    tracing::warn!(
                        backend = %name, error = %err,
                        "could not remember what identification found; it will have to be \
                         re-identified after a restart"
                    );
                }
            }
            Ok(None) => {}
            Err(err) => {
                tracing::warn!(error = %err, "a backend identification round did not finish");
            }
        }
    }
}

/// Restore the last detection result before anything is re-detected.
///
/// Without it a boot against an unreachable backend reads back as `Generic`
/// with no context windows, so every model on it silently falls back to the
/// global 32768 guess — the exact failure profiles exist to remove. Same
/// reasoning as [`seed_remembered_models`], and deliberately the same shape.
async fn seed_remembered_detection(registry: &UpstreamRegistry, db: &Pool) {
    let remembered = match upstreams_config::load_detected(db).await {
        Ok(d) if !d.is_empty() => d,
        Ok(_) => return,
        Err(err) => {
            tracing::debug!(error = %err, "could not load remembered backend detection");
            return;
        }
    };
    for pool in registry.pools() {
        for backend in &pool.backends {
            // Only seed what is still blank, exactly as `seed_remembered_models`
            // guards on an empty live set. `reload` has just carried a profile
            // forward for every backend whose name *and* address are unchanged,
            // and deliberately refused to for the rest — writing the stored row
            // over the top would reinstate precisely the association it
            // refused, and replace a `context_cap` the probe refreshes each
            // tick with a staler one.
            if backend.detected() != profile::Detected::default() {
                continue;
            }
            if let Some(detected) = remembered.get(&backend.name) {
                backend.set_detected(detected);
            }
        }
    }
}

/// Single round of probing — used by both the bootstrap path and the
/// looping path. Updates liveness + advertised-model set on success; on
/// failure, only returns the outcome (the caller decides whether one
/// failure flips health or only the third).
async fn probe_once(
    http: &reqwest::Client,
    pool_name: &str,
    backend: &Backend,
    db: Option<&Pool>,
) -> ProbeOutcome {
    let url = format!("{}{}", backend.base_url, backend.health_path);
    // The profile's context endpoint, read on the same tick and concurrently
    // with `/models` — they are independent, and serialising them would double
    // the worst case of a probe that gates routing.
    //
    // Every tick rather than once at identification, because this is runtime
    // state: Ollama loads models on first use, so at apply time `/api/ps`
    // reports nothing and a freshly applied topology would know no window for
    // any model. Now one arrives within five seconds of the model being used.
    //
    // Skipped when discovery is off: that path returns before the writes
    // below, so asking would have been one request every five seconds whose
    // answer is thrown away.
    let context = async {
        if !backend.probe_models_enabled() {
            return None;
        }
        profile::read_context(
            http,
            &backend.base_url,
            backend.api_key.as_deref(),
            backend.profile(),
        )
        .await
    };
    // Send the backend's API key on the probe — same `Authorization:
    // Bearer …` header `proxy.rs` adds to real requests. Without it the
    // upstream's access log fills with anonymous-401s from the gateway
    // every 5 s.
    let mut req = http.get(&url).header(
        "user-agent",
        concat!("aiplane/", env!("CARGO_PKG_VERSION"), " healthcheck"),
    );
    if let Some(key) = backend.api_key.as_deref() {
        req = req.bearer_auth(key);
    }
    let (result, context) = tokio::join!(tokio::time::timeout(PROBE_TIMEOUT, req.send()), context);

    let resp = match result {
        Ok(Ok(resp)) => resp,
        // Transport-level failure — no HTTP response came back. reqwest's
        // own Display is the opaque "error sending request for url (…)"; we
        // dig the concrete cause out of the source chain so the caller can
        // log something an admin can act on.
        Ok(Err(err)) => return ProbeOutcome::Failed(describe_transport_error(&err)),
        Err(_) => {
            return ProbeOutcome::Failed(format!(
                "no response within the {PROBE_TIMEOUT:?} probe timeout"
            ));
        }
    };

    let status = resp.status();

    // 401 counts as "alive" — even with the api_key header, some
    // upstreams scope `/models` differently from `/chat/completions`.
    // We can't discover models from a 401 body, but the backend is
    // reachable, so leave its model set alone and just mark it healthy.
    // If real requests get 401 too, they'll surface the failure end to
    // end; if the previous probe round populated the model set, that
    // state survives until a successful probe replaces it.
    if matches!(status.as_u16(), 401 | 403) {
        // Flag it so the admin page can say "the key was rejected" rather than
        // showing a green backend that quietly advertises nothing. Not a health
        // failure: some upstreams scope `/models` differently from
        // `/chat/completions`, so real traffic may still work.
        if !backend.auth_failed() {
            tracing::warn!(
                pool = %pool_name, backend = %backend.name, status = status.as_u16(),
                "health probe was rejected by upstream auth — model discovery is off for this \
                 backend, so nothing new will become routable through it. Check its API key \
                 (and, if it uses `api_key_env`, that the variable is actually set)."
            );
        }
        backend.set_auth_failed(true);
        return ProbeOutcome::AliveNoData;
    }
    if !status.is_success() {
        return ProbeOutcome::Failed(format!(
            "upstream returned HTTP {} from {}",
            status.as_u16(),
            backend.health_path
        ));
    }

    // Model discovery disabled for this backend: the probe is a pure liveness
    // check, and the configured model set is authoritative. We reached a 2xx,
    // so the backend is up — but we deliberately do NOT read/parse `/models`,
    // because on an image backend (e.g. z.AI's general endpoint) that response
    // is the *chat* catalog and would clobber the configured image model ids.
    if !backend.probe_models_enabled() {
        return ProbeOutcome::AliveNoData;
    }

    // Parse the OpenAI `/models` envelope. A backend that returns 200
    // with a different shape (or non-JSON entirely — e.g. plain
    // whisper.cpp) is alive but unparseable: we mark it healthy and
    // leave the model set unchanged, so the operator can either keep
    // a previously-populated set or accept that the backend won't be
    // routable.
    let body = match resp.bytes().await {
        Ok(b) => b,
        Err(err) => {
            tracing::debug!(
                pool = %pool_name, backend = %backend.name, error = %err,
                "reading /models body failed"
            );
            return ProbeOutcome::AliveNoData;
        }
    };
    let parsed: Result<serde_json::Value, _> = serde_json::from_slice(&body);
    let value = match parsed {
        Ok(v) => v,
        Err(err) => {
            tracing::debug!(
                pool = %pool_name, backend = %backend.name, error = %err,
                "parsing /models body failed; leaving model set unchanged"
            );
            return ProbeOutcome::AliveNoData;
        }
    };
    // One parse, both halves — ids to route by, windows to budget by. Shared
    // with detection (`profile::read_models`) so the two cannot drift: when
    // they did, the probe understood only vLLM's spelling of the window and
    // every llama.cpp model fell through to the global 32768 guess with the
    // real figure sitting in the body it had just read.
    // `None` = not an OpenAI model envelope at all. Alive, but nothing to
    // learn, so the previous model set survives — a backend whose `/models`
    // answers in some other shape stays routable on what it advertised before
    // rather than being emptied into unroutability.
    let Some((new_set, windows)) = profile::read_models(&value) else {
        tracing::debug!(
            pool = %pool_name, backend = %backend.name,
            "/models body was not an OpenAI model envelope; leaving model set unchanged"
        );
        return ProbeOutcome::AliveNoData;
    };

    let previous = backend.probe_models();
    if previous != new_set {
        let added: Vec<&String> = new_set.difference(&previous).collect();
        let removed: Vec<&String> = previous.difference(&new_set).collect();
        tracing::info!(
            pool = %pool_name, backend = %backend.name,
            added = ?added, removed = ?removed,
            total = new_set.len(),
            "advertised models updated"
        );
        // Remember it, so the *next* boot knows this backend's models even if
        // it is unreachable then (see `seed_remembered_models`). Only on a
        // change — the steady state writes nothing. Best-effort: a write failure
        // costs the seed, not the probe.
        if let Some(db) = db
            && let Err(err) =
                upstreams_config::save_probed_models(db, &backend.name, &new_set).await
        {
            tracing::debug!(
                pool = %pool_name, backend = %backend.name, error = %err,
                "could not remember the advertised model set"
            );
        }
    }
    backend.set_models(new_set);
    // `/models` first, then whatever the profile's own endpoint said — the
    // latter is the figure actually allocated, so it wins where both speak.
    //
    // Only written when that endpoint actually answered. A timeout or a 404
    // after an upstream upgrade is not "this server has no context": clearing
    // the figure on one bad tick drops every model on the backend onto the
    // assumed window, which is the silent truncation the reading exists to
    // prevent. `None` here leaves the last good answer standing, exactly as an
    // unparseable `/models` body leaves the model set standing.
    if let Some((endpoint_windows, context_cap)) = context {
        let mut windows = windows;
        windows.extend(endpoint_windows);
        backend.set_context_windows(windows);
        backend.set_context_cap(context_cap);
    } else if backend.profile().context_endpoint().is_some() {
        tracing::debug!(
            pool = %pool_name, backend = %backend.name,
            "the context endpoint did not answer; keeping the windows already known"
        );
        backend.set_context_windows(windows);
    } else {
        backend.set_context_windows(windows);
    }
    if backend.auth_failed() {
        tracing::info!(
            pool = %pool_name, backend = %backend.name,
            "upstream auth accepted again — model discovery restored"
        );
        backend.set_auth_failed(false);
    }

    ProbeOutcome::AliveWithModels
}

#[derive(Debug, Clone)]
enum ProbeOutcome {
    /// 200 + parseable body, models updated.
    AliveWithModels,
    /// 200 (non-parseable) or 401 — reachable but no new model data.
    AliveNoData,
    /// Network error, timeout, or non-2xx — counts toward FAILURE_THRESHOLD.
    /// Carries a precise, admin-readable reason for the log.
    Failed(String),
}

/// Turn an opaque reqwest transport error into a precise, admin-readable
/// reason. reqwest's `Display` is only "error sending request for url (…)";
/// the concrete cause (refused / reset / DNS / unreachable) lives down the
/// `source()` chain. Surface a human headline plus the full chain so a probe
/// log line is never ambiguous.
fn describe_transport_error(err: &reqwest::Error) -> String {
    let mut chain: Vec<String> = Vec::new();
    let mut io_kind: Option<std::io::ErrorKind> = None;
    let mut cur: Option<&(dyn std::error::Error + 'static)> = Some(err);
    while let Some(e) = cur {
        chain.push(e.to_string());
        if io_kind.is_none()
            && let Some(io) = e.downcast_ref::<std::io::Error>()
        {
            io_kind = Some(io.kind());
        }
        cur = std::error::Error::source(e);
    }

    let headline = if err.is_timeout() {
        "no response before the timeout".to_string()
    } else if let Some(kind) = io_kind {
        use std::io::ErrorKind;
        match kind {
            ErrorKind::ConnectionRefused => {
                "connection refused — nothing is listening at that host:port".into()
            }
            ErrorKind::ConnectionReset => {
                "connection reset by the upstream — it dropped the connection \
                 mid-request (commonly an idle keep-alive closed by the server \
                 or a firewall)"
                    .into()
            }
            ErrorKind::ConnectionAborted => "connection aborted by the upstream".into(),
            ErrorKind::TimedOut => "TCP timed out — host:port unreachable".into(),
            other => format!("I/O error: {other:?}"),
        }
    } else if err.is_connect() {
        "could not establish a TCP connection".into()
    } else {
        "transport failure before any HTTP response".into()
    };

    format!("{headline} (chain: {})", chain.join(" ⇐ "))
}

async fn run_probe(
    http: reqwest::Client,
    pool_name: String,
    backend: Arc<Backend>,
    registry: Arc<UpstreamRegistry>,
    generation: u64,
    db: Option<Pool>,
) {
    tracing::debug!(
        pool = %pool_name,
        backend = %backend.name,
        url = format!("{}{}", backend.base_url, backend.health_path),
        "starting health probe loop"
    );
    let mut consecutive_failures = 0u32;
    loop {
        // Retire once a `reload()` has superseded this generation, so a stale
        // loop doesn't keep probing a detached (possibly removed/re-pointed)
        // backend forever.
        if registry.generation() != generation {
            tracing::debug!(
                pool = %pool_name, backend = %backend.name,
                "health probe loop retiring — topology reloaded"
            );
            return;
        }
        match probe_once(&http, &pool_name, &backend, db.as_ref()).await {
            ProbeOutcome::AliveWithModels | ProbeOutcome::AliveNoData => {
                if !backend.is_healthy() {
                    tracing::info!(pool = %pool_name, backend = %backend.name, "backend recovered — healthy again");
                }
                backend.set_healthy(true);
                consecutive_failures = 0;
            }
            ProbeOutcome::Failed(reason) => {
                consecutive_failures = consecutive_failures.saturating_add(1);
                match (
                    backend.is_healthy(),
                    consecutive_failures >= FAILURE_THRESHOLD,
                ) {
                    // Crossed the threshold → a real outage. Exactly one WARN
                    // with the precise cause — the only probe-failure line an
                    // admin running at INFO ever sees.
                    (true, true) => {
                        tracing::warn!(
                            pool = %pool_name, backend = %backend.name,
                            failures = consecutive_failures,
                            "backend DOWN: {reason}"
                        );
                        backend.set_healthy(false);
                    }
                    // Still serving traffic — a single blip, not an outage.
                    // Quiet (DEBUG) and explicitly labelled so it can't be
                    // mistaken for a failure.
                    (true, false) => tracing::debug!(
                        pool = %pool_name, backend = %backend.name,
                        attempt = consecutive_failures, threshold = FAILURE_THRESHOLD,
                        "transient probe blip (backend still healthy): {reason}"
                    ),
                    // Already known-down; keep ongoing failures at DEBUG.
                    (false, _) => tracing::debug!(
                        pool = %pool_name, backend = %backend.name,
                        "backend still down: {reason}"
                    ),
                }
            }
        }
        // Poll a down backend more often than a healthy one: this interval is
        // the recovery latency every parked request pays.
        sleep(if backend.is_healthy() {
            PROBE_INTERVAL
        } else {
            DOWN_PROBE_INTERVAL
        })
        .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn describe_transport_error_surfaces_concrete_cause() {
        // Bind then drop → a port guaranteed to refuse connections, so the
        // probe's reqwest call fails with a real transport error.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let url = format!("http://{addr}/v1/models");
        let err = reqwest::Client::new().get(&url).send().await.unwrap_err();

        let desc = describe_transport_error(&err);
        let lower = desc.to_lowercase();
        // Must name a concrete cause, not just the opaque reqwest top line.
        assert!(
            lower.contains("refused") || lower.contains("connect"),
            "expected a concrete connection cause, got: {desc}"
        );
        // And it must include the full source chain.
        assert!(desc.contains("chain:"), "missing source chain: {desc}");
    }

    // --- probe_models gate ---------------------------------------------------

    use std::collections::{HashMap, HashSet};

    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::server::upstreams::UpstreamRegistry;
    use crate::server::upstreams::config::{
        BackendConfig, PickerStrategy, PoolKind, UpstreamPoolConfig,
    };

    fn image_backend(base_url: &str, probe_models: bool) -> BackendConfig {
        BackendConfig {
            name: "img".into(),
            base_url: base_url.into(),
            api_key_env: None,
            api_key: None,
            weight: 1,
            max_inflight: 16,
            health_path: "/models".into(),
            models: vec!["glm-image".into()],
            alias: None,
            probe_models,
            supports_edit: false,
            enabled: true,
        }
    }

    /// A `/models` endpoint that (like z.AI's general endpoint) answers 200 with
    /// a *chat* catalog — the exact shape that would clobber a static image
    /// model set if the probe were allowed to discover it.
    async fn chat_catalog_server() -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "object": "list",
                "data": [{ "id": "glm-4.6" }]
            })))
            .mount(&server)
            .await;
        server
    }

    fn backend_arc(base_url: &str, probe_models: bool) -> std::sync::Arc<Backend> {
        let mut pools = HashMap::new();
        pools.insert(
            "images".to_string(),
            UpstreamPoolConfig {
                voices: Default::default(),
                offer_voices: Vec::new(),
                allowed_groups: Vec::new(),
                compliance: Default::default(),
                enforce_limits: true,
                kind: PoolKind::Image,
                strategy: PickerStrategy::RoundRobin,
                models: Vec::new(),
                fallback_offline: None,
                backend: vec![image_backend(base_url, probe_models)],
            },
        );
        let reg = UpstreamRegistry::new(&pools).unwrap();
        reg.pools()
            .into_iter()
            .find(|p| p.name == "images")
            .unwrap()
            .backends[0]
            .clone()
    }

    /// A vLLM `/models` response carries each model's real context window in
    /// `max_model_len`. The probe used to read this response and throw that
    /// field away, so every model fell back to the global 32768 default — a
    /// model actually serving 262144 got an eighth of the tool-output
    /// allowance it should have, which is what starved long retrieval turns.
    #[tokio::test]
    async fn the_probe_learns_each_model_s_real_context_window() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "object": "list",
                "data": [
                    {"id": "glm-image", "max_model_len": 262_144},
                    // Hosted APIs omit the field; that model simply has none.
                    {"id": "hosted-model"},
                    // A nonsense value must not become a budget.
                    {"id": "broken", "max_model_len": 0},
                ]
            })))
            .mount(&server)
            .await;
        let backend = backend_arc(&server.uri(), true);

        let outcome = probe_once(&reqwest::Client::new(), "images", &backend, None).await;
        assert!(
            matches!(outcome, ProbeOutcome::AliveWithModels),
            "expected AliveWithModels, got {outcome:?}"
        );
        assert_eq!(backend.context_window("glm-image"), Some(262_144));
        assert_eq!(backend.context_window("hosted-model"), None);
        assert_eq!(backend.context_window("broken"), None, "0 is not a window");
        assert_eq!(backend.context_window("never-heard-of-it"), None);
    }

    #[tokio::test]
    async fn probe_models_false_keeps_config_models_over_chat_catalog() {
        let server = chat_catalog_server().await;
        let backend = backend_arc(&server.uri(), false);

        let outcome = probe_once(&reqwest::Client::new(), "images", &backend, None).await;
        // Reachable, but no discovery: config model set is untouched.
        assert!(
            matches!(outcome, ProbeOutcome::AliveNoData),
            "expected AliveNoData, got {outcome:?}"
        );
        assert!(
            backend.probe_models().is_empty(),
            "probe must not populate the live set when probe_models = false"
        );
        assert_eq!(
            backend.models_snapshot(),
            std::collections::HashSet::from(["glm-image".to_string()]),
            "configured image model must remain authoritative"
        );
    }

    #[tokio::test]
    async fn shared_backend_keeps_system_one_catalog_isolated_across_its_lifecycle() {
        use crate::server::db;
        use crate::server::db::upstreams_config::{BackendRow, PoolRow};
        use jiff::Timestamp;

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "object": "list",
                "data": [{ "id": "fresh-chat" }]
            })))
            .mount(&server)
            .await;

        let db = db::open(std::path::Path::new(":memory:")).await.unwrap();
        let now = Timestamp::now();
        upstreams_config::upsert_backend(
            &db,
            &BackendRow {
                name: "openrouter".into(),
                base_url: server.uri(),
                api_key_env: None,
                api_key_ct: None,
                api_key_nonce: None,
                weight: 1,
                max_inflight: 16,
                health_path: "/models".into(),
                probe_models: true,
                supports_edit: false,
                enabled: true,
                models: Vec::new(),
                aliases: Vec::new(),
                created_at: now,
                updated_at: now,
            },
        )
        .await
        .unwrap();
        for (name, kind, sort_order, models) in [
            ("chat", "chat", 0, Vec::new()),
            (
                "system-one",
                "system_one",
                1,
                vec!["typesafe/jev-1.13".to_string()],
            ),
        ] {
            upstreams_config::upsert_pool(
                &db,
                &PoolRow {
                    name: name.into(),
                    kind: kind.into(),
                    strategy: "least_inflight".into(),
                    fallback_offline: None,
                    compliance_gdpr: true,
                    compliance_nda: true,
                    enforce_limits: true,
                    sort_order,
                    allowed_groups: Vec::new(),
                    backends: vec!["openrouter".into()],
                    models,
                    voices: Vec::new(),
                    offer_voices: Vec::new(),
                    created_at: now,
                    updated_at: now,
                },
            )
            .await
            .unwrap();
        }
        upstreams_config::save_probed_models(
            &db,
            "openrouter",
            &HashSet::from(["cached-chat".to_string()]),
        )
        .await
        .unwrap();

        let snapshot = upstreams_config::load_snapshot(&db).await.unwrap();
        let crypto = crate::server::crypto::Crypto::ephemeral();
        let registry = UpstreamRegistry::from_snapshot(&snapshot, &crypto).unwrap();
        seed_remembered_models(&registry, &db).await;

        let chat = registry
            .pools()
            .into_iter()
            .find(|pool| pool.kind == PoolKind::Chat)
            .unwrap()
            .backends[0]
            .clone();
        let system_one = registry
            .pools()
            .into_iter()
            .find(|pool| pool.kind == PoolKind::SystemOne)
            .unwrap()
            .backends[0]
            .clone();
        assert_eq!(chat.live_models(), HashSet::from(["cached-chat".into()]));
        assert_eq!(
            chat.models_snapshot(),
            HashSet::from(["cached-chat".into()])
        );
        assert!(chat.withheld_models().is_empty());
        assert!(system_one.live_models().is_empty());
        assert_eq!(
            system_one.models_snapshot(),
            HashSet::from(["typesafe/jev-1.13".into()])
        );
        assert!(system_one.withheld_models().is_empty());

        system_one.set_models(HashSet::from(["cached-chat".into()]));
        assert!(
            system_one.live_models().is_empty(),
            "the model-state boundary must reject discovery for a pinned catalog"
        );

        assert!(registry.route("cached-chat", PoolKind::Chat).is_ok());
        assert!(
            registry
                .route("typesafe/jev-1.13", PoolKind::SystemOne)
                .is_ok()
        );
        assert!(registry.route("cached-chat", PoolKind::SystemOne).is_err());

        assert!(matches!(
            probe_once(&reqwest::Client::new(), "chat", &chat, Some(&db)).await,
            ProbeOutcome::AliveWithModels
        ));
        assert!(matches!(
            probe_once(
                &reqwest::Client::new(),
                "system-one",
                &system_one,
                Some(&db),
            )
            .await,
            ProbeOutcome::AliveNoData
        ));
        assert_eq!(chat.models_snapshot(), HashSet::from(["fresh-chat".into()]));
        assert!(chat.withheld_models().is_empty());
        assert!(system_one.live_models().is_empty());
        assert_eq!(
            system_one.models_snapshot(),
            HashSet::from(["typesafe/jev-1.13".into()])
        );
        assert!(system_one.withheld_models().is_empty());
        assert!(registry.route("fresh-chat", PoolKind::Chat).is_ok());
        assert!(registry.route("fresh-chat", PoolKind::SystemOne).is_err());

        registry.reload(&snapshot, &crypto).unwrap();
        let reloaded_chat = registry
            .pools()
            .into_iter()
            .find(|pool| pool.kind == PoolKind::Chat)
            .unwrap()
            .backends[0]
            .clone();
        let reloaded_system_one = registry
            .pools()
            .into_iter()
            .find(|pool| pool.kind == PoolKind::SystemOne)
            .unwrap()
            .backends[0]
            .clone();
        assert_eq!(
            reloaded_chat.models_snapshot(),
            HashSet::from(["fresh-chat".into()]),
            "reload must carry the live chat catalog across"
        );
        assert!(reloaded_chat.withheld_models().is_empty());
        assert!(reloaded_system_one.live_models().is_empty());
        assert_eq!(
            reloaded_system_one.models_snapshot(),
            HashSet::from(["typesafe/jev-1.13".into()]),
            "reload must leave the System One catalog pinned to config"
        );
        assert!(reloaded_system_one.withheld_models().is_empty());
        assert!(registry.route("fresh-chat", PoolKind::Chat).is_ok());
        assert!(
            registry
                .route("typesafe/jev-1.13", PoolKind::SystemOne)
                .is_ok()
        );
        assert!(registry.route("fresh-chat", PoolKind::SystemOne).is_err());
    }

    #[tokio::test]
    async fn probe_models_true_allowlist_withholds_chat_catalog() {
        // The contrast case: with discovery on, the server's chat catalog
        // (`glm-4.6`) is discovered — but the backend's configured `models`
        // list (`glm-image`) is now an allowlist, so the unlisted chat model
        // is *withheld* rather than clobbering the intended set. The
        // intersection is empty here (the backend doesn't actually serve
        // `glm-image`), so nothing is served and the discovered id shows up in
        // the withheld set for the struck-through UI chip.
        let server = chat_catalog_server().await;
        let backend = backend_arc(&server.uri(), true);

        let outcome = probe_once(&reqwest::Client::new(), "images", &backend, None).await;
        assert!(
            matches!(outcome, ProbeOutcome::AliveWithModels),
            "expected AliveWithModels, got {outcome:?}"
        );
        assert!(
            backend.models_snapshot().is_empty(),
            "allowlist ∩ probe is empty → nothing served, got {:?}",
            backend.models_snapshot()
        );
        assert_eq!(
            backend.withheld_models(),
            std::collections::HashSet::from(["glm-4.6".to_string()]),
            "the discovered-but-unlisted chat model must be withheld"
        );
    }
}

/// Detection wired end to end: a mock server that answers like Ollama, run
/// through the real `spawn` path, and then asked the question the request path
/// asks.
///
/// The pure pieces are tested next door in `profile`; this covers the seam
/// between them — that what identification learned reaches the registry, that
/// the probe keeps the context reading current, and that both come back as the
/// decisions a request depends on. A wiring bug there is invisible to every
/// unit test and produces exactly the silent failure the feature exists to
/// remove.
#[cfg(test)]
mod detection_wiring {
    use super::*;
    use crate::server::upstreams::config::{
        BackendConfig, PickerStrategy, PoolKind, UpstreamPoolConfig,
    };
    use crate::server::upstreams::profile::BackendProfile;
    use crate::server::upstreams::registry::PoolAccess;
    use std::collections::HashMap;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn route(server: &MockServer, route: &str, body: serde_json::Value) {
        Mock::given(method("GET"))
            .and(path(route))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(server)
            .await;
    }

    /// A registry holding one plain chat backend pointed at `server` — no
    /// configured model list, which is what an operator adding an Ollama
    /// actually has, and what lets the probe's discovery be authoritative.
    fn registry_for(server: &MockServer) -> Arc<UpstreamRegistry> {
        let pools = HashMap::from([(
            "chat".to_string(),
            UpstreamPoolConfig {
                kind: PoolKind::Chat,
                strategy: PickerStrategy::RoundRobin,
                fallback_offline: None,
                models: Vec::new(),
                compliance: Default::default(),
                enforce_limits: true,
                voices: Default::default(),
                offer_voices: Vec::new(),
                allowed_groups: Vec::new(),
                backend: vec![BackendConfig {
                    name: "ollama".into(),
                    // As an operator writes it: the OpenAI base.
                    base_url: format!("{}/v1", server.uri()),
                    api_key_env: None,
                    api_key: None,
                    weight: 1,
                    max_inflight: 16,
                    health_path: "/models".into(),
                    models: Vec::new(),
                    alias: None,
                    probe_models: true,
                    enabled: true,
                    supports_edit: false,
                }],
            },
        )]);
        UpstreamRegistry::new(&pools).unwrap()
    }

    /// A stand-in Ollama, answering the shapes a live 0.34.0 answers.
    ///
    /// `loaded` is what `/api/ps` reports: `false` is a model the server knows
    /// of but has not loaded, which is how Ollama behaves before first use.
    async fn ollama_server(loaded: bool) -> MockServer {
        ollama_serving(loaded, "qwen3:0.6b").await
    }

    /// As above, but serving a named model — so a test can use one whose name
    /// carries no reasoning family.
    async fn ollama_serving(loaded: bool, model: &str) -> MockServer {
        let server = MockServer::start().await;
        route(
            &server,
            "/api/version",
            serde_json::json!({"version": "0.34.0"}),
        )
        .await;
        let ps = if loaded {
            serde_json::json!({"models": [
                {"name": model, "model": model, "context_length": 4096}
            ]})
        } else {
            serde_json::json!({"models": []})
        };
        route(&server, "/api/ps", ps).await;
        route(
            &server,
            "/v1/models",
            serde_json::json!({"object": "list", "data": [{"id": model}]}),
        )
        .await;
        server
    }

    #[tokio::test]
    async fn spawn_identifies_a_backend_and_the_request_path_sees_it() {
        let server = ollama_server(true).await;
        let registry = registry_for(&server);

        spawn(Arc::clone(&registry), None).await;

        let pools = registry.pools();
        let backend = &pools[0].backends[0];
        assert_eq!(backend.profile(), BackendProfile::Ollama);
        assert_eq!(backend.detected().version.as_deref(), Some("0.34.0"));

        // The context the server actually allocated — the number a prompt has
        // to fit, and nowhere on the OpenAI surface. Without it every model
        // here falls back to the global 32768 guess and gets truncated.
        assert_eq!(registry.probed_context_window("qwen3:0.6b"), Some(4_096));

        // And the two decisions a request derives from the profile.
        let serving = registry.serving_profile("qwen3:0.6b", PoolKind::Chat, &PoolAccess::all());
        assert_eq!(
            serving.dialect,
            Some(crate::server::reasoning::ReasoningStyle::Ollama),
            "the effort control must be spelled the way this server understands"
        );
        assert!(
            !serving.honors_tool_choice,
            "a final tool round here has to withhold the tools, not trust tool_choice"
        );
    }

    /// The request body a turn against each kind of backend actually carries.
    ///
    /// Every other test here stops at identification — "the registry learned
    /// it is Ollama" — and that is how a backend profile came to switch
    /// reasoning *on* for models that have none. Ollama does not ignore a
    /// thinking parameter it cannot honour, it refuses the request:
    /// `HTTP 400 "gemma3:270m" does not support thinking`, verified against a
    /// live 0.34.0. So the wiring has to be checked where it lands, in the
    /// body, and not only where it is decided.
    #[tokio::test]
    async fn the_request_body_matches_the_backend_a_turn_would_reach() {
        use crate::server::reasoning::{Effort, ReasoningOverrides, ReasoningStyle, apply_effort};

        // A thinking model on Ollama: the effort control reaches it, spelled
        // the way that server understands rather than the way its name implies.
        let thinking = ollama_serving(true, "qwen3:0.6b").await;
        let registry = registry_for(&thinking);
        spawn(Arc::clone(&registry), None).await;
        let dialect = registry
            .serving_profile("qwen3:0.6b", PoolKind::Chat, &PoolAccess::all())
            .dialect;
        let style = ReasoningStyle::resolve(None, dialect, "qwen3:0.6b");
        let mut body = serde_json::json!({"model": "qwen3:0.6b", "messages": []});
        apply_effort(
            style,
            Effort::Max,
            &ReasoningOverrides::default(),
            &mut body,
        );
        assert_eq!(body["reasoning_effort"], serde_json::json!("max"));
        assert!(
            body.get("chat_template_kwargs").is_none(),
            "the model name says Qwen, but this server discards that spelling"
        );

        // An ordinary model on the same kind of backend: nothing is sent, and
        // that is the whole point — a thinking parameter here is a 400.
        let plain = ollama_serving(true, "gemma3:270m").await;
        let registry = registry_for(&plain);
        spawn(Arc::clone(&registry), None).await;
        let dialect = registry
            .serving_profile("gemma3:270m", PoolKind::Chat, &PoolAccess::all())
            .dialect;
        let style = ReasoningStyle::resolve(None, dialect, "gemma3:270m");
        let mut body = serde_json::json!({"model": "gemma3:270m", "messages": []});
        apply_effort(
            style,
            Effort::Max,
            &ReasoningOverrides::default(),
            &mut body,
        );
        assert_eq!(
            body,
            serde_json::json!({"model": "gemma3:270m", "messages": []}),
            "a model with no reasoning must leave the backend untouched"
        );
    }

    /// The case that decides whether any of this works in practice.
    ///
    /// Ollama loads models on first use, so at apply time `/api/ps` reports
    /// nothing. Reading the context only at identification would leave every
    /// model with no window and drop it onto the global 32768 guess against a
    /// 4096 allocation — the failure this feature exists to remove, arriving
    /// through its own refresh policy. The window has to come from the probe,
    /// which runs every five seconds, not from the apply.
    #[tokio::test]
    async fn a_model_loaded_after_the_apply_still_gets_its_window() {
        // The apply happens while nothing is loaded.
        let idle = ollama_server(false).await;
        let registry = registry_for(&idle);
        spawn(Arc::clone(&registry), None).await;
        let pools = registry.pools();
        let backend = &pools[0].backends[0];
        assert_eq!(backend.profile(), BackendProfile::Ollama);
        assert_eq!(
            backend.context_window("qwen3:0.6b"),
            None,
            "nothing is loaded yet, so there is honestly nothing to report"
        );

        // The user chats, Ollama loads the model, and the next probe tick
        // picks it up — no second apply, no operator action.
        let busy = ollama_server(true).await;
        let loaded = registry_for(&busy);
        spawn(Arc::clone(&loaded), None).await;
        let pools = loaded.pools();
        let backend = &pools[0].backends[0];
        probe_once(&reqwest::Client::new(), "chat", backend, None).await;
        assert_eq!(backend.context_window("qwen3:0.6b"), Some(4_096));
    }
}
