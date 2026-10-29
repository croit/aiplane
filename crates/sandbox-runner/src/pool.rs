// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Warm pool of sandbox containers.
//!
//! Security model: by default every job runs in a container that is used
//! **exactly once** and then destroyed — no state leaks between users. To
//! hide container cold-start latency we keep `pool_size` pristine, idle
//! containers pre-booted; a job pops one, runs, and the container is torn
//! down while a replacement boots in the background.
//!
//! Exception — **leases**: a `keep_alive` job (the gateway's `run_in_sandbox`
//! within one conversation turn) keeps its container alive in the `leased`
//! table so later calls in the same turn reuse it (persisting `/work`). It's
//! freed by an explicit `DELETE /container/{id}` at turn end, with a TTL
//! sweeper reaping any lease a crashed turn left behind. This never crosses a
//! user boundary: a turn is one authenticated caller.
//!
//! Networked calls (pip / browser) never reuse a pooled container: they
//! get a fresh on-demand container attached to the egress-proxy network,
//! so the default-deny warm pool stays default-deny.

use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;

use shared::sandbox::{AgentWorkspaceScope, OwnedAgentWorkspace, RunRequest, RunResponse};
use thiserror::Error;
use tokio::sync::Semaphore;

use crate::backend::{ContainerBackend, Network, WorkspaceMount};
use crate::config::Config;
use crate::workspace::Workspace;

#[derive(Debug, Error)]
pub enum RunnerError {
    #[error("sandbox backend error: {0}")]
    Backend(#[from] crate::backend::BackendError),
    #[error("sandbox at capacity")]
    Busy,
    #[error("network egress requested but not configured on this runner")]
    NetworkUnavailable,
    #[error(
        "persistent agent workspace storage is unavailable; configure and verify the runner's persistent storage"
    )]
    WorkspaceUnavailable,
    #[error("sandbox lease does not match this agent, turn or purpose")]
    WorkspaceMismatch,
    #[error("workspace filesystem operation failed: {0}")]
    Workspace(#[from] crate::workspace::WorkspaceError),
    #[error("the agent browser handover is expired or unavailable")]
    BrowserUnavailable,
}

/// A pre-booted, idle container plus the workload-image id it was created
/// from. Stamping the image lets the pool spot containers left over from a
/// previous image after a rebuild / re-tag and recycle them.
struct Warm {
    id: String,
    image: String,
}

/// A container kept alive across calls within one conversation turn (a
/// "lease"). Unlike a [`Warm`] container it is *not* pristine — the turn's
/// earlier calls may have written `/work`, set env, etc. — which is exactly
/// the point. Freed explicitly by the gateway at turn end
/// ([`Pool::release_container`]); the [`Pool::sweep_expired`] backstop reaps
/// one whose gateway died mid-turn.
struct Leased {
    /// Instant of the most recent exec's *completion* (not start), so a
    /// long-running exec doesn't look idle while it runs.
    last_used: Instant,
    /// A job is currently exec'ing in this container — the sweeper must not
    /// reap it even if `last_used` is old (a single exec can legitimately
    /// run up to `max_timeout_secs`).
    busy: bool,
    retiring: bool,
    scope: Option<AgentWorkspaceScope>,
    network: Network,
    workspace: Option<Arc<Workspace>>,
    handover_generation: u64,
    handover: Option<BrowserHandover>,
}

struct BrowserHandover {
    id: String,
    generation: u64,
    last_sequence: u64,
    started_at: Instant,
    last_input_at: Instant,
    expires_at: String,
    sealed: bool,
    controller_id: Option<String>,
}

const BROWSER_HANDOVER_IDLE: Duration = Duration::from_secs(5 * 60);
const BROWSER_HANDOVER_MAX: Duration = Duration::from_secs(15 * 60);

fn browser_remaining(started_at: Instant, last_input_at: Instant, now: Instant) -> Duration {
    let idle = BROWSER_HANDOVER_IDLE.saturating_sub(now.saturating_duration_since(last_input_at));
    let total = BROWSER_HANDOVER_MAX.saturating_sub(now.saturating_duration_since(started_at));
    idle.min(total)
}

fn browser_expiry_in(remaining: Duration) -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .saturating_add(remaining.as_secs());
    let timestamp: libc::time_t = seconds.try_into().unwrap_or(libc::time_t::MAX);
    let mut broken_down = unsafe { std::mem::zeroed::<libc::tm>() };
    if unsafe { libc::gmtime_r(&timestamp, &mut broken_down) }.is_null() {
        return seconds.to_string();
    }
    let mut output = [0u8; 32];
    let written = unsafe {
        libc::strftime(
            output.as_mut_ptr().cast(),
            output.len(),
            c"%Y-%m-%dT%H:%M:%SZ".as_ptr(),
            &broken_down,
        )
    };
    if written == 0 {
        return seconds.to_string();
    }
    String::from_utf8_lossy(&output[..written]).into_owned()
}

pub struct Pool {
    backend: Arc<dyn ContainerBackend>,
    cfg: Arc<Config>,
    /// Pre-booted, default-deny containers awaiting a job.
    ready: Mutex<VecDeque<Warm>>,
    /// Containers kept alive for reuse within a turn, keyed by container id.
    leased: Mutex<HashMap<String, Leased>>,
    /// The workload-image id the pool is currently warming to. Empty until
    /// the first [`Self::refresh_image`]; updated when the image changes.
    image: Mutex<String>,
    /// Caps concurrent in-flight jobs.
    sem: Arc<Semaphore>,
    /// Serializes refill so we never overshoot `pool_size`.
    refill_lock: tokio::sync::Mutex<()>,
}

struct TeardownAttempt<'a> {
    pool: &'a Pool,
    id: &'a str,
}

impl Drop for TeardownAttempt<'_> {
    fn drop(&mut self) {
        if let Some(lease) = self.pool.leased.lock().unwrap().get_mut(self.id) {
            lease.busy = false;
        }
    }
}

impl Pool {
    /// The runner's configuration, so the HTTP layer can report capabilities
    /// (`/healthz`) without holding a second `Arc<Config>`.
    pub fn config(&self) -> &Config {
        &self.cfg
    }

    pub fn new(backend: Arc<dyn ContainerBackend>, cfg: Arc<Config>) -> Arc<Self> {
        let permits = cfg.max_concurrent.max(1);
        Arc::new(Self {
            backend,
            cfg,
            ready: Mutex::new(VecDeque::new()),
            leased: Mutex::new(HashMap::new()),
            image: Mutex::new(String::new()),
            sem: Arc::new(Semaphore::new(permits)),
            refill_lock: tokio::sync::Mutex::new(()),
        })
    }

    /// Spawn the background lease sweeper: every 30 s, reap any leased
    /// container that has been idle (no in-flight job) longer than
    /// `lease_ttl_secs`. This is the crash/leak backstop — in the normal
    /// case the gateway frees leases explicitly at turn end. Called once at
    /// boot (from `main`); a no-op-friendly loop that runs for the process
    /// lifetime.
    pub fn spawn_sweeper(self: &Arc<Self>) {
        let this = self.clone();
        // Tick often enough that a small configured TTL is actually honored,
        // but never busier than once a second; capped at 30s so a large TTL
        // doesn't spin. (Reap latency ≈ TTL + up to one tick.)
        let tick = Duration::from_secs(self.cfg.lease_ttl_secs.clamp(1, 30));
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(tick).await;
                this.sweep_expired().await;
            }
        });
    }

    /// Reap leased containers idle longer than the TTL. Skips any with an
    /// in-flight job (`busy`), so a legitimately long exec is never torn out
    /// from under itself.
    async fn sweep_expired(self: &Arc<Self>) {
        let ttl = Duration::from_secs(self.cfg.lease_ttl_secs);
        let now = Instant::now();
        let expired: Vec<String> = {
            let leased = self.leased.lock().unwrap();
            leased
                .iter()
                .filter(|(_, l)| {
                    !l.busy
                        && (l.retiring
                            || l.handover.as_ref().is_some_and(|handover| {
                                now.saturating_duration_since(handover.last_input_at)
                                    >= BROWSER_HANDOVER_IDLE
                                    || now.saturating_duration_since(handover.started_at)
                                        >= BROWSER_HANDOVER_MAX
                            })
                            || (l.handover.is_none()
                                && now.saturating_duration_since(l.last_used) > ttl))
                })
                .map(|(id, _)| id.clone())
                .collect()
        };
        for id in expired {
            tracing::warn!(
                container = %id,
                "sandbox lease expired (gateway did not release it — turn crashed?); reaping"
            );
            self.reap_if_idle(&id).await;
        }
    }

    /// Reap a lease only if it's still idle, re-checking `busy` **under the
    /// same lock** as the removal. The gap between `sweep_expired` collecting
    /// an id and destroying it is a window in which a fresh `/run` can reuse
    /// that id and start an exec (marking it busy); this re-check ensures we
    /// don't destroy a container mid-exec. Nothing removed → nothing destroyed.
    async fn reap_if_idle(self: &Arc<Self>, id: &str) {
        if let Err(error) = self.release_container(id).await {
            tracing::warn!(container = %id, error = %error, "sandbox teardown not confirmed; retaining container for retry");
        }
    }

    pub async fn release_container(self: &Arc<Self>, id: &str) -> Result<(), RunnerError> {
        self.backend.validate_container_id(id)?;
        {
            let mut leased = self.leased.lock().unwrap();
            let lease = leased.entry(id.to_owned()).or_insert(Leased {
                last_used: Instant::now(),
                busy: false,
                retiring: true,
                scope: None,
                network: Network::None,
                workspace: None,
                handover_generation: 0,
                handover: None,
            });
            if lease.busy {
                return Err(RunnerError::Busy);
            }
            if let Some(handover) = &lease.handover {
                let now = Instant::now();
                let expired = now.saturating_duration_since(handover.last_input_at)
                    >= BROWSER_HANDOVER_IDLE
                    || now.saturating_duration_since(handover.started_at) >= BROWSER_HANDOVER_MAX;
                if !expired {
                    return Err(RunnerError::Busy);
                }
                lease.handover = None;
            }
            lease.retiring = true;
            lease.busy = true;
        }
        let _attempt = TeardownAttempt { pool: self, id };
        let result = self.backend.destroy(id).await;
        let mut leased = self.leased.lock().unwrap();
        match result {
            Ok(()) => {
                leased.remove(id);
                Ok(())
            }
            Err(error) => {
                if let Some(lease) = leased.get_mut(id) {
                    lease.busy = false;
                }
                Err(error.into())
            }
        }
    }

    /// Number of live leases — test/observability only.
    #[cfg(test)]
    pub fn leased_len(&self) -> usize {
        self.leased
            .lock()
            .unwrap()
            .values()
            .filter(|lease| !lease.retiring)
            .count()
    }

    /// Number of pre-booted containers currently idle. Test-only today;
    /// promote to `pub` (and surface via `/healthz`) if we add readiness
    /// reporting.
    #[cfg(test)]
    pub fn ready_len(&self) -> usize {
        self.ready.lock().unwrap().len()
    }

    /// The image ids stamped on the currently-idle warm containers.
    #[cfg(test)]
    pub fn ready_images(&self) -> Vec<String> {
        self.ready
            .lock()
            .unwrap()
            .iter()
            .map(|w| w.image.clone())
            .collect()
    }

    /// Boot containers until the ready queue reaches `pool_size`. Held
    /// behind `refill_lock` so concurrent callers can't overshoot. Each
    /// new container is stamped with the pool's current target image so a
    /// later image change can tell it apart from a fresh one.
    pub async fn refill(self: &Arc<Self>) {
        let _g = self.refill_lock.lock().await;
        let target = self.image.lock().unwrap().clone();
        loop {
            let have = self.ready.lock().unwrap().len();
            if have >= self.cfg.pool_size {
                break;
            }
            match self.backend.create(Network::None, None).await {
                Ok(id) => self.ready.lock().unwrap().push_back(Warm {
                    id,
                    image: target.clone(),
                }),
                Err(e) => {
                    tracing::warn!(error = %e, "warm-pool refill failed; will retry later");
                    break;
                }
            }
        }
    }

    /// Re-resolve the workload image's id and, if it changed since the pool
    /// was last warmed (a rebuild / re-tag), destroy the now-stale idle
    /// containers and re-warm on the new image. Seeds the target image on
    /// the first call (boot). Best-effort: a failed id lookup leaves the
    /// pool as-is rather than tearing it down on a transient error.
    pub async fn refresh_image(self: &Arc<Self>) {
        let now = match self.backend.image_id().await {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!(error = %e, "sandbox image-id check failed; keeping current pool");
                return;
            }
        };
        let prev = {
            let mut g = self.image.lock().unwrap();
            if *g == now {
                return; // unchanged — nothing to do
            }
            std::mem::replace(&mut *g, now.clone())
        };
        if prev.is_empty() {
            tracing::info!(image = %short(&now), "sandbox warm pool: seeding on image");
        } else {
            tracing::info!(
                prev = %short(&prev),
                new = %short(&now),
                "sandbox workload image changed — recycling warm pool"
            );
            // Drop every stale idle container; in-flight jobs are single-use
            // and torn down on completion regardless.
            let stale: Vec<Warm> = { self.ready.lock().unwrap().drain(..).collect() };
            for w in stale {
                self.spawn_destroy(w.id);
            }
        }
        self.refill().await;
    }

    /// Obtain a *fresh* container for a job: a warm pooled one for
    /// default-deny, an on-demand one for egress. Returns `(id, pooled)`
    /// where `pooled` means it came off the warm queue (so the pool should
    /// refill). The std `MutexGuard`s are scoped so they never straddle an
    /// `.await` (that would make the future non-Send; rama handlers must be
    /// Send).
    async fn acquire_fresh(
        self: &Arc<Self>,
        want_egress: bool,
    ) -> Result<(String, bool), RunnerError> {
        if want_egress {
            return Ok((self.backend.create(Network::Egress, None).await?, false));
        }
        let warm = { self.ready.lock().unwrap().pop_front() };
        let target = { self.image.lock().unwrap().clone() };
        match warm {
            // Defensive against the brief race where the image changed but
            // the periodic refresh hasn't drained yet: a popped container
            // stamped with a superseded image is destroyed, not served, and
            // we boot a fresh one (which uses the new image).
            Some(w) if !target.is_empty() && w.image != target => {
                self.spawn_destroy(w.id.clone());
                Ok((self.backend.create(Network::None, None).await?, false))
            }
            Some(w) => Ok((w.id, true)),
            None => Ok((self.backend.create(Network::None, None).await?, false)),
        }
    }

    /// Run one job. Acquires a concurrency permit, obtains a container
    /// (reusing a live lease when `req.container_id` names one, else a fresh
    /// pooled/on-demand one), executes, and disposes of the container per
    /// `req.keep_alive`:
    ///
    /// - **reused lease** → refreshed and kept; response echoes its id.
    /// - **fresh + `keep_alive`** → registered as a lease *if* under the
    ///   `max_leases` cap, response echoes its id; when the cap is full the
    ///   job still ran, the container is destroyed single-use, and the
    ///   response carries `container_id: None` (graceful fallback).
    /// - **fresh, no `keep_alive`** → destroyed single-use (today's default).
    ///
    /// A background refill tops the pool back up whenever a warm container
    /// was consumed (whether it became a lease or was torn down).
    pub async fn run(self: &Arc<Self>, req: &RunRequest) -> Result<RunResponse, RunnerError> {
        if req.workspace.is_some() {
            return self.run_scoped(req.clone()).await;
        }
        let _permit = self.sem.try_acquire().map_err(|_| RunnerError::Busy)?;

        let want_egress = req.network;
        // Egress posture is only decided when creating a container. On reuse
        // the leased container's fixed network stands, so don't reject a
        // reuse call just because this runner has no egress wired.
        let creating = req.container_id.is_none();
        if want_egress && creating && !self.cfg.egress_available() {
            return Err(RunnerError::NetworkUnavailable);
        }

        // Resolve the container. `reused` marks a live lease we exec into
        // again (mark it busy so the sweeper leaves it alone); `pooled` marks
        // a warm container consumed from the queue (so we refill).
        let mut reused = false;
        let mut pooled = false;
        let id = match req.container_id.as_deref() {
            Some(want) if self.mark_lease_busy(want)? => {
                reused = true;
                want.to_string()
            }
            // Either no reuse requested, or the named lease is gone (swept /
            // expired): obtain a fresh container. `want_egress` reflects the
            // caller's request for the fresh case.
            _ => {
                let (fresh, from_pool) = self.acquire_fresh(want_egress).await?;
                pooled = from_pool;
                fresh
            }
        };

        let timeout = self.cfg.effective_timeout(req.timeout_secs);
        let started = Instant::now();
        let result = self.backend.exec(&id, req, timeout).await;
        let elapsed_ms = started.elapsed().as_millis() as u64;

        // Decide the container's fate and what id (if any) to echo back. A
        // wall-clock timeout comes back as `Ok(timed_out:true)` but must be
        // treated as a failure here: the runaway process is still alive inside
        // (the exec was abandoned, not killed), so the container has to be torn
        // down, never kept as a lease.
        let exec_ok = matches!(&result, Ok(r) if !r.timed_out);
        let lease_id = self.dispose(&id, reused, req.keep_alive, exec_ok);

        // Top the warm pool back up whenever we consumed a warm container —
        // it left the ready queue regardless of whether it became a lease.
        if pooled {
            let this = self.clone();
            tokio::spawn(async move { this.refill().await });
        }

        let mut resp = result?;
        // Trust the runner's own wall-clock over the agent's self-report.
        if !resp.timed_out {
            resp.duration_ms = elapsed_ms;
        }
        resp.container_id = lease_id;
        clamp_output(&mut resp, self.cfg.max_output_bytes);
        Ok(resp)
    }

    pub fn workspace_available(&self) -> bool {
        self.cfg.work_root.is_some() && self.backend.workspace_available()
    }

    pub fn workspace_operation<T>(
        &self,
        identity: &OwnedAgentWorkspace,
        mutating: bool,
        operation: impl FnOnce(&Workspace, bool) -> Result<T, crate::workspace::WorkspaceError>,
    ) -> Result<T, RunnerError> {
        if !self.workspace_available() {
            return Err(RunnerError::WorkspaceUnavailable);
        }
        let leased = self.leased.lock().unwrap();
        let busy = leased.values().any(|lease| {
            lease
                .scope
                .as_ref()
                .is_some_and(|scope| &scope.workspace == identity)
        });
        if mutating && busy {
            return Err(RunnerError::Busy);
        }
        let files = Workspace::open(self.cfg.work_root.as_ref().unwrap(), identity)?;
        let result = operation(&files, busy)?;
        drop(leased);
        Ok(result)
    }

    pub async fn retire_workspace(
        self: &Arc<Self>,
        identity: &OwnedAgentWorkspace,
    ) -> Result<bool, RunnerError> {
        if !self.workspace_available() {
            return Err(RunnerError::WorkspaceUnavailable);
        }
        let root = self
            .cfg
            .work_root
            .as_ref()
            .ok_or(RunnerError::WorkspaceUnavailable)?;
        let containers = {
            let mut leased = self.leased.lock().unwrap();
            Workspace::seal_retirement(root, identity)?;
            let mut containers = Vec::new();
            for (id, lease) in leased.iter_mut() {
                if lease
                    .scope
                    .as_ref()
                    .is_some_and(|scope| &scope.workspace == identity)
                {
                    lease.retiring = true;
                    containers.push(id.clone());
                }
            }
            containers
        };
        let mut failure = None;
        for id in containers {
            if let Err(error) = self.release_container(&id).await {
                failure = Some(error);
            }
        }
        if let Some(error) = failure {
            return Err(error);
        }
        let leased = self.leased.lock().unwrap();
        if leased.values().any(|lease| {
            lease
                .scope
                .as_ref()
                .is_some_and(|scope| &scope.workspace == identity)
        }) {
            return Err(RunnerError::Busy);
        }
        let completed = Workspace::cleanup_retired(root, identity)?;
        drop(leased);
        Ok(completed)
    }

    async fn run_scoped(self: &Arc<Self>, req: RunRequest) -> Result<RunResponse, RunnerError> {
        if !self.workspace_available() {
            return Err(RunnerError::WorkspaceUnavailable);
        }
        let permit = self
            .sem
            .clone()
            .try_acquire_owned()
            .map_err(|_| RunnerError::Busy)?;
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let pool = self.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let result = pool.run_scoped_owned(&req).await;
            if sender.is_closed()
                && let Ok(response) = &result
                && let Some(id) = &response.container_id
                && let Err(error) = pool.release_container(id).await
            {
                tracing::warn!(container = %id, error = %error, "cancelled workspace request retained for teardown retry");
            }
            if let Err(result) = sender.send(result)
                && let Ok(response) = result
                && let Some(id) = response.container_id
                && let Err(error) = pool.release_container(&id).await
            {
                tracing::warn!(container = %id, error = %error, "undelivered workspace response retained for teardown retry");
            }
        });
        receiver
            .await
            .map_err(|_| RunnerError::WorkspaceUnavailable)?
    }

    async fn run_scoped_owned(
        self: &Arc<Self>,
        req: &RunRequest,
    ) -> Result<RunResponse, RunnerError> {
        let scope = req
            .workspace
            .as_ref()
            .ok_or(RunnerError::WorkspaceMismatch)?;
        if scope.turn_id.is_empty() {
            return Err(RunnerError::WorkspaceMismatch);
        }
        let network = if req.network {
            Network::Egress
        } else {
            Network::None
        };
        if req.network && !self.cfg.egress_available() {
            return Err(RunnerError::NetworkUnavailable);
        }
        let (id, creating, files, path, browser_profile_path) = {
            let mut leased = self.leased.lock().unwrap();
            let files = Arc::new(Workspace::open(
                self.cfg
                    .work_root
                    .as_ref()
                    .ok_or(RunnerError::WorkspaceUnavailable)?,
                &scope.workspace,
            )?);
            let path = files.mount_path()?;
            let browser_profile_path = match scope.purpose {
                shared::sandbox::WorkspacePurpose::Workspace => None,
                shared::sandbox::WorkspacePurpose::Browser => Some(files.browser_profile_path()?),
            };
            if let Some(id) = &req.container_id {
                let lease = leased.get_mut(id).ok_or(RunnerError::WorkspaceMismatch)?;
                if lease.scope.as_ref() != Some(scope) || lease.network != network {
                    return Err(RunnerError::WorkspaceMismatch);
                }
                if lease.handover.is_some() {
                    return Err(RunnerError::Busy);
                }
                if lease.busy || lease.retiring {
                    return Err(RunnerError::Busy);
                }
                lease
                    .workspace
                    .as_ref()
                    .ok_or(RunnerError::WorkspaceMismatch)?
                    .validate_mount_path(&path)?;
                lease.busy = true;
                (id.clone(), false, files, path, browser_profile_path)
            } else {
                if leased.values().any(|lease| {
                    lease.scope.as_ref().is_some_and(|held| {
                        held.workspace.agent_id == scope.workspace.agent_id
                            && (held.workspace != scope.workspace
                                || held.turn_id != scope.turn_id
                                || lease.retiring
                                || lease.handover.is_some())
                    })
                }) {
                    return Err(RunnerError::Busy);
                }
                if leased.len() >= self.cfg.max_leases {
                    return Err(RunnerError::Busy);
                }
                let id = crate::backend::new_container_name();
                leased.insert(
                    id.clone(),
                    Leased {
                        last_used: Instant::now(),
                        busy: true,
                        retiring: false,
                        scope: Some(scope.clone()),
                        network,
                        workspace: Some(files.clone()),
                        handover_generation: 0,
                        handover: None,
                    },
                );
                (id, true, files, path, browser_profile_path)
            }
        };
        if creating {
            let mount = WorkspaceMount {
                name: id.clone(),
                files,
                path,
                browser_profile_path,
            };
            match self.backend.create(network, Some(&mount)).await {
                Ok(created) if created == id => {}
                Ok(created) => {
                    self.spawn_destroy(created);
                    if let Some(lease) = self.leased.lock().unwrap().get_mut(&id) {
                        lease.busy = false;
                    }
                    self.spawn_destroy(id);
                    return Err(RunnerError::WorkspaceMismatch);
                }
                Err(error) => {
                    if let Some(lease) = self.leased.lock().unwrap().get_mut(&id) {
                        lease.busy = false;
                    }
                    self.spawn_destroy(id);
                    return Err(error.into());
                }
            }
        }
        let retired = {
            let mut leased = self.leased.lock().unwrap();
            match leased.get_mut(&id) {
                Some(lease) if !lease.retiring => false,
                Some(lease) => {
                    lease.busy = false;
                    true
                }
                None => true,
            }
        };
        if retired {
            self.spawn_destroy(id);
            return Err(RunnerError::WorkspaceMismatch);
        }
        let started = Instant::now();
        let result = self
            .backend
            .exec(&id, req, self.cfg.effective_timeout(req.timeout_secs))
            .await;
        let healthy = matches!(&result, Ok(response) if !response.timed_out);
        let retiring = {
            let mut leased = self.leased.lock().unwrap();
            if let Some(lease) = leased.get_mut(&id) {
                lease.busy = false;
                lease.last_used = Instant::now();
                lease.retiring |= !healthy || !req.keep_alive;
                lease.retiring
            } else {
                true
            }
        };
        if retiring {
            self.spawn_destroy(id.clone());
        }
        let mut response = result?;
        if !response.timed_out {
            response.duration_ms = started.elapsed().as_millis() as u64;
        }
        response.container_id = (!retiring).then_some(id);
        clamp_output(&mut response, self.cfg.max_output_bytes);
        Ok(response)
    }

    pub async fn browser_control(
        &self,
        request: &shared::sandbox::BrowserControlRequest,
    ) -> Result<shared::sandbox::BrowserControlResponse, RunnerError> {
        use shared::sandbox::{
            BrowserControlOperation as Operation, BrowserInput, WorkspacePurpose,
        };
        if request.turn_id.is_empty()
            || request.handover_id.is_empty()
            || request.handover_id.len() > 128
            || request.container_id.is_empty()
        {
            return Err(RunnerError::WorkspaceMismatch);
        }
        if matches!(request.operation, Operation::Begin) && request.controller_id.is_some()
            || !matches!(request.operation, Operation::Begin)
                && request.controller_id.as_deref().is_none_or(str::is_empty)
        {
            return Err(RunnerError::BrowserUnavailable);
        }
        let id = &request.container_id;
        let (generation, mut expires_at) = {
            let mut leases = self.leased.lock().unwrap();
            let lease = leases.get_mut(id).ok_or(RunnerError::BrowserUnavailable)?;
            let scope = lease.scope.as_ref().ok_or(RunnerError::WorkspaceMismatch)?;
            if scope.workspace != request.workspace
                || scope.turn_id != request.turn_id
                || scope.purpose != WorkspacePurpose::Browser
                || lease.retiring
                || lease.busy
            {
                return Err(RunnerError::WorkspaceMismatch);
            }
            let (generation, expiry) = match &request.operation {
                Operation::Begin => {
                    if lease.handover.is_some() {
                        return Err(RunnerError::Busy);
                    }
                    lease.handover_generation = lease.handover_generation.saturating_add(1);
                    let generation = lease.handover_generation;
                    let now = Instant::now();
                    let expiry = browser_expiry_in(BROWSER_HANDOVER_IDLE);
                    lease.handover = Some(BrowserHandover {
                        id: request.handover_id.clone(),
                        generation,
                        last_sequence: 0,
                        started_at: now,
                        last_input_at: now,
                        expires_at: expiry.clone(),
                        sealed: true,
                        controller_id: None,
                    });
                    lease.busy = true;
                    (generation, expiry)
                }
                Operation::Claim { generation: wanted }
                | Operation::Frame { generation: wanted }
                | Operation::Input {
                    generation: wanted, ..
                }
                | Operation::Finish { generation: wanted } => {
                    let handover = lease
                        .handover
                        .as_mut()
                        .filter(|handover| handover.id == request.handover_id)
                        .ok_or(RunnerError::BrowserUnavailable)?;
                    let now = Instant::now();
                    if handover.generation != *wanted
                        || handover.sealed
                        || now.saturating_duration_since(handover.last_input_at)
                            >= BROWSER_HANDOVER_IDLE
                        || now.saturating_duration_since(handover.started_at)
                            >= BROWSER_HANDOVER_MAX
                    {
                        return Err(RunnerError::BrowserUnavailable);
                    }
                    handover.expires_at = browser_expiry_in(browser_remaining(
                        handover.started_at,
                        handover.last_input_at,
                        now,
                    ));
                    match request.operation {
                        Operation::Claim { .. } => {
                            let Some(controller_id) = request.controller_id.as_deref() else {
                                return Err(RunnerError::BrowserUnavailable);
                            };
                            if controller_id.is_empty()
                                || controller_id.len() > 128
                                || handover
                                    .controller_id
                                    .as_deref()
                                    .is_some_and(|current| current != controller_id)
                            {
                                return Err(RunnerError::BrowserUnavailable);
                            }
                            handover
                                .controller_id
                                .get_or_insert_with(|| controller_id.to_owned());
                        }
                        Operation::Frame { .. }
                        | Operation::Input { .. }
                        | Operation::Finish { .. } => {
                            if handover.controller_id.as_deref() != request.controller_id.as_deref()
                                || handover.controller_id.is_none()
                            {
                                return Err(RunnerError::BrowserUnavailable);
                            }
                        }
                        Operation::Begin => unreachable!(),
                    }
                    if let Operation::Input {
                        sequence, input, ..
                    } = &request.operation
                    {
                        if *sequence <= handover.last_sequence {
                            return Err(RunnerError::BrowserUnavailable);
                        }
                        match input {
                            BrowserInput::Click { x, y } if *x < 1280 && *y < 900 => {}
                            BrowserInput::Key { key }
                                if matches!(
                                    key.as_str(),
                                    "Enter"
                                        | "Tab"
                                        | "Escape"
                                        | "ArrowUp"
                                        | "ArrowDown"
                                        | "ArrowLeft"
                                        | "ArrowRight"
                                        | "Backspace"
                                        | "Delete"
                                        | "Home"
                                        | "End"
                                        | "PageUp"
                                        | "PageDown"
                                        | "Space"
                                ) => {}
                            BrowserInput::Text { text } if text.len() <= 2048 => {}
                            BrowserInput::Scroll { delta_x, delta_y }
                                if delta_x.abs() <= 1000 && delta_y.abs() <= 1000 => {}
                            _ => return Err(RunnerError::BrowserUnavailable),
                        }
                    }
                    (handover.generation, handover.expires_at.clone())
                }
            };
            lease.busy = true;
            (generation, expiry)
        };

        let result = if matches!(request.operation, Operation::Claim { .. }) {
            Ok(shared::sandbox::BrowserControlResponse {
                ok: true,
                generation,
                expires_at: expires_at.clone(),
                frame_b64: None,
                width: None,
                height: None,
            })
        } else {
            self.backend.browser_control(id, &request.operation).await
        };
        let result = match result {
            Ok(response) => response,
            Err(error) => {
                let cleanup = if matches!(request.operation, Operation::Begin) {
                    self.backend
                        .browser_control(id, &Operation::Finish { generation })
                        .await
                        .is_ok()
                } else {
                    false
                };
                let mut leases = self.leased.lock().unwrap();
                if let Some(lease) = leases.get_mut(id) {
                    lease.busy = false;
                    if cleanup
                        && lease
                            .handover
                            .as_ref()
                            .is_some_and(|handover| handover.id == request.handover_id)
                    {
                        lease.handover = None;
                    }
                }
                return Err(error.into());
            }
        };
        let mut leases = self.leased.lock().unwrap();
        let lease = leases.get_mut(id).ok_or(RunnerError::BrowserUnavailable)?;
        lease.busy = false;
        let handover = lease
            .handover
            .as_mut()
            .filter(|handover| handover.id == request.handover_id)
            .ok_or(RunnerError::BrowserUnavailable)?;
        match request.operation {
            Operation::Begin => handover.sealed = false,
            Operation::Claim { .. } => {}
            Operation::Frame { .. } => {}
            Operation::Input { sequence, .. } => {
                handover.last_sequence = sequence;
                handover.last_input_at = Instant::now();
                handover.expires_at = browser_expiry_in(browser_remaining(
                    handover.started_at,
                    handover.last_input_at,
                    handover.last_input_at,
                ));
                expires_at = handover.expires_at.clone();
                lease.last_used = handover.last_input_at;
            }
            Operation::Finish { .. } => {
                lease.handover = None;
                lease.last_used = Instant::now();
            }
        }
        Ok(shared::sandbox::BrowserControlResponse {
            ok: true,
            generation,
            expires_at,
            frame_b64: result.frame_b64,
            width: result.width,
            height: result.height,
        })
    }

    fn mark_lease_busy(&self, id: &str) -> Result<bool, RunnerError> {
        let mut leased = self.leased.lock().unwrap();
        match leased.get_mut(id) {
            Some(lease) if lease.scope.is_some() => Err(RunnerError::WorkspaceMismatch),
            Some(lease) if lease.busy || lease.retiring => Err(RunnerError::Busy),
            Some(lease) => {
                lease.busy = true;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Post-exec disposition of a container. Returns the id to echo in the
    /// response iff the container is (still) leased, else `None`. Never
    /// awaits — teardown is spawned so a slow `podman rm` doesn't stall the
    /// response.
    fn dispose(
        self: &Arc<Self>,
        id: &str,
        reused: bool,
        keep_alive: bool,
        exec_ok: bool,
    ) -> Option<String> {
        if reused {
            if exec_ok {
                // Existing lease still good: clear busy + refresh idle timer.
                let mut leased = self.leased.lock().unwrap();
                if let Some(l) = leased.get_mut(id) {
                    l.busy = false;
                    l.last_used = Instant::now();
                }
                return Some(id.to_string());
            }
            if let Some(lease) = self.leased.lock().unwrap().get_mut(id) {
                lease.busy = false;
                lease.retiring = true;
            }
            self.spawn_destroy(id.to_string());
            return None;
        }
        // Only turn a *fresh* container into a lease when its job actually
        // ran: on a backend error the runner returns no body, so the gateway
        // would never learn this id to release it — don't strand a container
        // it can't address (the single-use teardown below handles it).
        if keep_alive && exec_ok {
            // New lease requested. Register only if under the cap; otherwise
            // fall back to single-use so idle leases can't over-commit RAM.
            let registered = {
                let mut leased = self.leased.lock().unwrap();
                if leased.len() < self.cfg.max_leases {
                    leased.insert(
                        id.to_string(),
                        Leased {
                            last_used: Instant::now(),
                            busy: false,
                            retiring: false,
                            scope: None,
                            network: Network::None,
                            workspace: None,
                            handover_generation: 0,
                            handover: None,
                        },
                    );
                    true
                } else {
                    false
                }
            };
            if registered {
                return Some(id.to_string());
            }
            tracing::warn!(
                max_leases = self.cfg.max_leases,
                "sandbox lease cap reached; running single-use (no persistence for this turn)"
            );
        }
        // Single-use teardown (default, or lease-cap fallback).
        self.spawn_destroy(id.to_string());
        None
    }

    fn spawn_destroy(self: &Arc<Self>, id: String) {
        {
            let mut leased = self.leased.lock().unwrap();
            let lease = leased.entry(id.clone()).or_insert(Leased {
                last_used: Instant::now(),
                busy: false,
                retiring: true,
                scope: None,
                network: Network::None,
                workspace: None,
                handover_generation: 0,
                handover: None,
            });
            lease.retiring = true;
        }
        let pool = self.clone();
        tokio::spawn(async move {
            if let Err(error) = pool.release_container(&id).await {
                tracing::warn!(container = %id, error = %error, "sandbox teardown not confirmed; retaining container for retry");
            }
        });
    }
}

/// First few chars of an image id, for readable logs (`sha256:abcd…`).
fn short(id: &str) -> &str {
    let end = id
        .char_indices()
        .nth(19)
        .map(|(i, _)| i)
        .unwrap_or(id.len());
    &id[..end]
}

/// Clip stdout/stderr to the configured cap so a runaway job can't return
/// gigabytes through the gateway (and blow the model's context). Keeps a
/// HEAD and a TAIL with an omission marker in the middle — for logs and
/// tracebacks the decisive part (the error, the exit) is at the end, so a
/// head-only truncation would hide exactly what matters. The agent
/// separately preserves the full stream as an attachment when it's large,
/// so nothing is actually lost. Marks `output_truncated` when it bites.
fn clamp_output(resp: &mut RunResponse, max: usize) {
    // Split the budget so one stream can't starve the other.
    let half = max.max(2) / 2;
    let a = clip_head_tail(&mut resp.stdout, half);
    let b = clip_head_tail(&mut resp.stderr, half);
    if a || b {
        resp.output_truncated = true;
    }
}

/// Keep ~60% head + ~40% tail of `s` within `budget` bytes, char-boundary
/// safe, with a marker naming how much was dropped. Returns whether it cut.
fn clip_head_tail(s: &mut String, budget: usize) -> bool {
    if s.len() <= budget {
        return false;
    }
    let total = s.len();
    let head_budget = (budget * 6 / 10).max(1);
    let tail_budget = budget.saturating_sub(head_budget);
    let mut h = head_budget.min(total);
    while h > 0 && !s.is_char_boundary(h) {
        h -= 1;
    }
    let mut t = total.saturating_sub(tail_budget);
    while t < total && !s.is_char_boundary(t) {
        t += 1;
    }
    if t < h {
        t = h;
    }
    let omitted = t - h;
    let head = s[..h].to_string();
    let tail = s[t..].to_string();
    *s = format!(
        "{head}\n…[{omitted} bytes omitted; full output saved as a stdout/stderr attachment]…\n{tail}"
    );
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_handover_deadline_tracks_idle_and_total_limits() {
        let start = Instant::now();
        assert_eq!(
            browser_remaining(start, start, start),
            BROWSER_HANDOVER_IDLE
        );
        let one_minute = start + Duration::from_secs(60);
        assert_eq!(
            browser_remaining(start, start, one_minute),
            Duration::from_secs(4 * 60)
        );
        let fourteen_minutes = start + Duration::from_secs(14 * 60);
        assert_eq!(
            browser_remaining(start, fourteen_minutes, fourteen_minutes),
            Duration::from_secs(60)
        );
        let deadline = start + BROWSER_HANDOVER_MAX;
        assert_eq!(browser_remaining(start, deadline, deadline), Duration::ZERO);
    }

    #[tokio::test]
    async fn workspace_request_never_falls_back_to_unscoped_container() {
        let backend = Arc::new(crate::backend::fake::FakeBackend::new());
        let pool = Pool::new(backend.clone(), Arc::new(Config::for_test()));
        let mut request = crate::backend::fake::req();
        request.workspace = Some(shared::sandbox::AgentWorkspaceScope {
            workspace: shared::sandbox::OwnedAgentWorkspace {
                owner_user_id: "person".into(),
                agent_id: "00000000-0000-4000-8000-000000000001".into(),
            },
            turn_id: "persisted-turn".into(),
            purpose: shared::sandbox::WorkspacePurpose::Workspace,
        });
        assert!(pool.run(&request).await.is_err());
    }
    use crate::backend::fake::{FakeBackend, req};

    struct WorkspaceFixture(std::path::PathBuf);

    impl WorkspaceFixture {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "aiplane-pool-workspace-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir(&root).unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
            Self(std::fs::canonicalize(root).unwrap())
        }

        fn config(&self) -> Arc<Config> {
            let mut config = (*cfg(1, 4, false)).clone();
            config.work_root = Some(self.0.clone());
            Arc::new(config)
        }
    }

    impl Drop for WorkspaceFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn scoped_request(turn: &str, purpose: shared::sandbox::WorkspacePurpose) -> RunRequest {
        let mut request = req();
        request.keep_alive = true;
        request.workspace = Some(shared::sandbox::AgentWorkspaceScope {
            workspace: shared::sandbox::OwnedAgentWorkspace {
                owner_user_id: "person".into(),
                agent_id: "00000000-0000-4000-8000-000000000001".into(),
            },
            turn_id: turn.into(),
            purpose,
        });
        request
    }

    #[tokio::test]
    async fn workspace_retirement_waits_for_confirmed_teardown_and_blocks_delayed_runs() {
        let root = WorkspaceFixture::new();
        let backend = Arc::new(FakeBackend::new());
        let pool = Pool::new(backend.clone(), root.config());
        let request = scoped_request("turn-a", shared::sandbox::WorkspacePurpose::Workspace);
        let identity = request.workspace.as_ref().unwrap().workspace.clone();
        pool.workspace_operation(&identity, true, |files, _| files.write("keep.txt", b"keep"))
            .unwrap();
        pool.run(&request).await.unwrap();
        backend
            .fail_destroy
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(pool.retire_workspace(&identity).await.is_err());
        assert!(
            root.0
                .join("agents")
                .join(&identity.agent_id)
                .join("files/keep.txt")
                .exists()
        );
        assert!(pool.run(&request).await.is_err());
        backend
            .fail_destroy
            .store(false, std::sync::atomic::Ordering::SeqCst);
        assert!(pool.retire_workspace(&identity).await.unwrap());
        assert!(!root.0.join("agents").join(&identity.agent_id).exists());
        assert!(pool.retire_workspace(&identity).await.unwrap());
        assert!(pool.run(&request).await.is_err());
        assert!(!backend.destroyed.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn workspace_scoped_run_bypasses_warm_container_and_rejects_foreign_lease() {
        let root = WorkspaceFixture::new();
        let backend = Arc::new(FakeBackend::new());
        let pool = Pool::new(backend.clone(), root.config());
        pool.refill().await;
        let warm = backend.created.lock().unwrap()[0].0.clone();
        let request = scoped_request("turn-a", shared::sandbox::WorkspacePurpose::Workspace);
        let id = pool.run(&request).await.unwrap().container_id.unwrap();
        assert_ne!(id, warm);
        let before = backend.execs.load(std::sync::atomic::Ordering::SeqCst);
        let mut foreign = request.clone();
        foreign.container_id = Some(id.clone());
        foreign.workspace.as_mut().unwrap().workspace.owner_user_id = "foreign".into();
        assert!(pool.run(&foreign).await.is_err());
        let mut ordinary = req();
        ordinary.container_id = Some(id.clone());
        assert!(pool.run(&ordinary).await.is_err());
        let mut unknown = request;
        unknown.container_id = Some("fake-999".into());
        assert!(pool.run(&unknown).await.is_err());
        assert_eq!(
            backend.execs.load(std::sync::atomic::Ordering::SeqCst),
            before
        );
        pool.release_container(&id).await.unwrap();
    }

    #[tokio::test]
    async fn workspace_same_turn_code_and_browser_share_occupancy_until_both_released() {
        let root = WorkspaceFixture::new();
        let backend = Arc::new(FakeBackend::new());
        let pool = Pool::new(backend, root.config());
        let code = pool
            .run(&scoped_request(
                "turn-a",
                shared::sandbox::WorkspacePurpose::Workspace,
            ))
            .await
            .unwrap()
            .container_id
            .unwrap();
        let browser = pool
            .run(&scoped_request(
                "turn-a",
                shared::sandbox::WorkspacePurpose::Browser,
            ))
            .await
            .unwrap()
            .container_id
            .unwrap();
        assert_ne!(code, browser);
        let other = scoped_request("turn-b", shared::sandbox::WorkspacePurpose::Workspace);
        assert!(matches!(pool.run(&other).await, Err(RunnerError::Busy)));
        pool.release_container(&code).await.unwrap();
        assert!(matches!(pool.run(&other).await, Err(RunnerError::Busy)));
        pool.release_container(&browser).await.unwrap();
        let next = pool.run(&other).await.unwrap().container_id.unwrap();
        pool.release_container(&next).await.unwrap();
    }

    #[tokio::test]
    async fn browser_handover_fences_agent_execution_and_rejects_stale_input() {
        use shared::sandbox::{
            BrowserControlOperation as Operation, BrowserControlRequest, BrowserInput,
            WorkspacePurpose,
        };

        let root = WorkspaceFixture::new();
        let backend = Arc::new(FakeBackend::new());
        let pool = Pool::new(backend.clone(), root.config());
        let browser_scope = scoped_request("turn-a", WorkspacePurpose::Browser)
            .workspace
            .unwrap();
        let browser_id = pool
            .run(&scoped_request("turn-a", WorkspacePurpose::Browser))
            .await
            .unwrap()
            .container_id
            .unwrap();
        let request = |operation, controller: Option<&str>| BrowserControlRequest {
            workspace: browser_scope.workspace.clone(),
            turn_id: browser_scope.turn_id.clone(),
            container_id: browser_id.clone(),
            handover_id: "handover-1".into(),
            controller_id: controller.map(str::to_owned),
            operation,
        };

        backend
            .fail_browser_begin_once
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(
            pool.browser_control(&request(Operation::Begin, None))
                .await
                .is_err()
        );
        let started = pool
            .browser_control(&request(Operation::Begin, None))
            .await
            .unwrap();
        assert_eq!(started.generation, 2);
        assert!(matches!(
            pool.run(&scoped_request("turn-a", WorkspacePurpose::Workspace))
                .await,
            Err(RunnerError::Busy)
        ));
        pool.browser_control(&request(
            Operation::Claim { generation: 2 },
            Some("control-1"),
        ))
        .await
        .unwrap();
        assert!(
            pool.browser_control(&request(
                Operation::Claim { generation: 2 },
                Some("control-1")
            ))
            .await
            .is_ok()
        );
        assert!(matches!(
            pool.browser_control(&request(
                Operation::Claim { generation: 2 },
                Some("control-2")
            ))
            .await,
            Err(RunnerError::BrowserUnavailable)
        ));
        assert!(matches!(
            pool.browser_control(&request(
                Operation::Input {
                    generation: 2,
                    sequence: 0,
                    input: BrowserInput::Key {
                        key: "Enter".into()
                    },
                },
                Some("control-1")
            ))
            .await,
            Err(RunnerError::BrowserUnavailable)
        ));
        pool.browser_control(&request(
            Operation::Input {
                generation: 2,
                sequence: 1,
                input: BrowserInput::Key {
                    key: "Enter".into(),
                },
            },
            Some("control-1"),
        ))
        .await
        .unwrap();
        pool.browser_control(&request(
            Operation::Input {
                generation: 2,
                sequence: 3,
                input: BrowserInput::Key { key: "Tab".into() },
            },
            Some("control-1"),
        ))
        .await
        .unwrap();
        pool.browser_control(&request(
            Operation::Finish { generation: 2 },
            Some("control-1"),
        ))
        .await
        .unwrap();

        let mut resumed = scoped_request("turn-a", WorkspacePurpose::Browser);
        resumed.container_id = Some(browser_id.clone());
        assert!(pool.run(&resumed).await.is_ok());
        assert_eq!(backend.browser_controls.lock().unwrap().len(), 6);
        pool.release_container(&browser_id).await.unwrap();
    }

    #[tokio::test]
    async fn workspace_failed_teardown_retains_agent_turn_occupancy_until_confirmed() {
        let root = WorkspaceFixture::new();
        let backend = Arc::new(FakeBackend::new());
        let pool = Pool::new(backend.clone(), root.config());
        let id = pool
            .run(&scoped_request(
                "turn-a",
                shared::sandbox::WorkspacePurpose::Workspace,
            ))
            .await
            .unwrap()
            .container_id
            .unwrap();
        backend
            .fail_destroy
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(pool.release_container(&id).await.is_err());
        let identity = scoped_request("turn-a", shared::sandbox::WorkspacePurpose::Workspace)
            .workspace
            .unwrap()
            .workspace;
        assert!(matches!(
            pool.workspace_operation(&identity, true, |files, _| files
                .write("blocked.txt", b"blocked")),
            Err(RunnerError::Busy)
        ));
        let listing = pool
            .workspace_operation(&identity, false, |files, busy| {
                assert!(busy);
                files.list("", None)
            })
            .unwrap();
        assert!(listing.entries.is_empty());
        assert!(matches!(
            pool.run(&scoped_request(
                "turn-b",
                shared::sandbox::WorkspacePurpose::Workspace
            ))
            .await,
            Err(RunnerError::Busy)
        ));
        backend
            .fail_destroy
            .store(false, std::sync::atomic::Ordering::SeqCst);
        pool.release_container(&id).await.unwrap();
        let next = pool
            .run(&scoped_request(
                "turn-b",
                shared::sandbox::WorkspacePurpose::Workspace,
            ))
            .await
            .unwrap()
            .container_id
            .unwrap();
        pool.release_container(&next).await.unwrap();
    }

    #[tokio::test]
    async fn workspace_same_turn_refused_while_prior_teardown_unconfirmed() {
        let root = WorkspaceFixture::new();
        let backend = Arc::new(FakeBackend::new());
        let pool = Pool::new(backend.clone(), root.config());
        let id = pool
            .run(&scoped_request(
                "turn-a",
                shared::sandbox::WorkspacePurpose::Workspace,
            ))
            .await
            .unwrap()
            .container_id
            .unwrap();
        backend
            .fail_destroy
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(pool.release_container(&id).await.is_err());
        // A fresh same-turn request must not mount the files directory while
        // the prior container's absence is unconfirmed.
        assert!(matches!(
            pool.run(&scoped_request(
                "turn-a",
                shared::sandbox::WorkspacePurpose::Workspace
            ))
            .await,
            Err(RunnerError::Busy)
        ));
        backend
            .fail_destroy
            .store(false, std::sync::atomic::Ordering::SeqCst);
        pool.release_container(&id).await.unwrap();
        let next = pool
            .run(&scoped_request(
                "turn-a",
                shared::sandbox::WorkspacePurpose::Workspace,
            ))
            .await
            .unwrap()
            .container_id
            .unwrap();
        pool.release_container(&next).await.unwrap();
    }

    #[tokio::test]
    async fn workspace_cancelled_creation_keeps_reservation_and_confirms_cleanup() {
        let root = WorkspaceFixture::new();
        let backend = Arc::new(FakeBackend::new());
        backend
            .hold_create
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let pool = Pool::new(backend.clone(), root.config());
        let task_pool = pool.clone();
        let task = tokio::spawn(async move {
            task_pool
                .run(&scoped_request(
                    "turn-a",
                    shared::sandbox::WorkspacePurpose::Workspace,
                ))
                .await
        });
        tokio::time::timeout(Duration::from_secs(2), backend.create_entered.notified())
            .await
            .unwrap();
        task.abort();
        let _ = task.await;
        let blocked = tokio::time::timeout(
            Duration::from_secs(2),
            pool.run(&scoped_request(
                "turn-b",
                shared::sandbox::WorkspacePurpose::Workspace,
            )),
        )
        .await;
        backend
            .hold_create
            .store(false, std::sync::atomic::Ordering::SeqCst);
        backend.create_release.notify_one();
        assert!(matches!(blocked, Ok(Err(RunnerError::Busy))));
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if !backend.destroyed.lock().unwrap().is_empty() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(backend.live_count(), 0);
        let next = pool
            .run(&scoped_request(
                "turn-b",
                shared::sandbox::WorkspacePurpose::Workspace,
            ))
            .await
            .unwrap()
            .container_id
            .unwrap();
        pool.release_container(&next).await.unwrap();
    }

    #[tokio::test]
    async fn workspace_retirement_during_creation_blocks_exec_and_preserves_files_until_absence() {
        let root = WorkspaceFixture::new();
        let backend = Arc::new(FakeBackend::new());
        let pool = Pool::new(backend.clone(), root.config());
        let request = scoped_request("turn-a", shared::sandbox::WorkspacePurpose::Workspace);
        let identity = request.workspace.as_ref().unwrap().workspace.clone();
        pool.workspace_operation(&identity, true, |files, _| files.write("keep.txt", b"keep"))
            .unwrap();
        let payload = root
            .0
            .join("agents")
            .join(&identity.agent_id)
            .join("files/keep.txt");
        backend
            .hold_create
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let task_pool = pool.clone();
        let task = tokio::spawn(async move { task_pool.run(&request).await });
        let entered =
            tokio::time::timeout(Duration::from_secs(2), backend.create_entered.notified()).await;
        let retired =
            tokio::time::timeout(Duration::from_secs(2), pool.retire_workspace(&identity)).await;
        let retained = std::fs::read(&payload);
        let delayed = tokio::time::timeout(
            Duration::from_secs(2),
            pool.run(&scoped_request(
                "turn-b",
                shared::sandbox::WorkspacePurpose::Browser,
            )),
        )
        .await;
        let tombstone = root
            .0
            .join(".agent-retired")
            .join(&identity.agent_id)
            .join("owner.json")
            .is_file();
        backend
            .hold_create
            .store(false, std::sync::atomic::Ordering::SeqCst);
        backend.create_release.notify_one();
        let result = tokio::time::timeout(Duration::from_secs(2), task).await;
        assert!(entered.is_ok());
        assert!(matches!(retired, Ok(Err(RunnerError::Busy))));
        assert_eq!(retained.unwrap(), b"keep");
        assert!(tombstone);
        assert!(matches!(delayed, Ok(Err(_))));
        assert!(result.unwrap().unwrap().is_err());
        assert_eq!(backend.execs.load(std::sync::atomic::Ordering::SeqCst), 0);
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                match pool.retire_workspace(&identity).await {
                    Ok(true) => break,
                    Ok(false) | Err(RunnerError::Busy) => tokio::task::yield_now().await,
                    Err(error) => panic!("retirement failed: {error}"),
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(backend.live_count(), 0);
        assert!(!payload.exists());
        assert!(
            tokio::time::timeout(
                Duration::from_secs(2),
                pool.run(&scoped_request(
                    "turn-c",
                    shared::sandbox::WorkspacePurpose::Workspace
                ))
            )
            .await
            .unwrap()
            .is_err()
        );
    }

    #[tokio::test]
    async fn workspace_retirement_during_execution_keeps_seal_and_confirms_teardown_before_cleanup()
    {
        let root = WorkspaceFixture::new();
        let backend = Arc::new(FakeBackend::new());
        let pool = Pool::new(backend.clone(), root.config());
        let mut request = scoped_request("turn-a", shared::sandbox::WorkspacePurpose::Workspace);
        let identity = request.workspace.as_ref().unwrap().workspace.clone();
        pool.workspace_operation(&identity, true, |files, _| files.write("keep.txt", b"keep"))
            .unwrap();
        let payload = root
            .0
            .join("agents")
            .join(&identity.agent_id)
            .join("files/keep.txt");
        request.code = "hold-exec".into();
        let task_pool = pool.clone();
        let task = tokio::spawn(async move { task_pool.run(&request).await });
        let entered =
            tokio::time::timeout(Duration::from_secs(2), backend.exec_entered.notified()).await;
        let retired =
            tokio::time::timeout(Duration::from_secs(2), pool.retire_workspace(&identity)).await;
        let retained = std::fs::read(&payload);
        let delayed = tokio::time::timeout(
            Duration::from_secs(2),
            pool.run(&scoped_request(
                "turn-b",
                shared::sandbox::WorkspacePurpose::Browser,
            )),
        )
        .await;
        let tombstone = root
            .0
            .join(".agent-retired")
            .join(&identity.agent_id)
            .join("owner.json")
            .is_file();
        backend.exec_release.notify_one();
        let result = tokio::time::timeout(Duration::from_secs(2), task).await;
        assert!(entered.is_ok());
        assert!(matches!(retired, Ok(Err(RunnerError::Busy))));
        assert_eq!(retained.unwrap(), b"keep");
        assert!(tombstone);
        assert!(matches!(delayed, Ok(Err(_))));
        assert!(result.unwrap().unwrap().unwrap().container_id.is_none());
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                match pool.retire_workspace(&identity).await {
                    Ok(true) => break,
                    Ok(false) | Err(RunnerError::Busy) => tokio::task::yield_now().await,
                    Err(error) => panic!("retirement failed: {error}"),
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(backend.execs.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(backend.live_count(), 0);
        assert!(!payload.exists());
        assert!(
            tokio::time::timeout(
                Duration::from_secs(2),
                pool.run(&scoped_request(
                    "turn-c",
                    shared::sandbox::WorkspacePurpose::Workspace
                ))
            )
            .await
            .unwrap()
            .is_err()
        );
    }

    fn cfg(pool_size: usize, max_concurrent: usize, egress: bool) -> Arc<Config> {
        Arc::new(Config {
            bind: "127.0.0.1:9000".into(),
            image: "img".into(),
            runtime: "runsc".into(),
            podman: "podman".into(),
            pool_size,
            max_concurrent,
            image_check_secs: 0,
            default_timeout_secs: 60,
            max_timeout_secs: 300,
            memory: "1024m".into(),
            cpus: "2".into(),
            pids_limit: 256,
            work_size: "512m".into(),
            tmp_size: "512m".into(),
            work_root: None,
            max_output_bytes: 1_048_576,
            egress_network: if egress {
                "egress".into()
            } else {
                String::new()
            },
            egress_proxy: if egress {
                "http://proxy:3128".into()
            } else {
                String::new()
            },
            lease_ttl_secs: 600,
            max_leases: 6,
        })
    }

    async fn settle_ready(pool: &Arc<Pool>, want: usize) {
        for _ in 0..2000 {
            if pool.ready_len() >= want {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        panic!(
            "pool never reached {want} ready (have {})",
            pool.ready_len()
        );
    }

    /// Wait for at least `want` destroys to land (teardown is spawned, so a
    /// synchronous assert right after would race it).
    async fn settle_destroyed(be: &Arc<FakeBackend>, want: usize) {
        for _ in 0..2000 {
            if be.destroyed.lock().unwrap().len() >= want {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        panic!(
            "never reached {want} destroyed (have {})",
            be.destroyed.lock().unwrap().len()
        );
    }

    #[tokio::test]
    async fn refill_warms_to_pool_size() {
        let be = Arc::new(FakeBackend::new());
        let pool = Pool::new(be.clone(), cfg(3, 6, false));
        pool.refill().await;
        assert_eq!(pool.ready_len(), 3);
        assert_eq!(be.live_count(), 3);
    }

    #[tokio::test]
    async fn run_consumes_a_pooled_container_then_refills() {
        let be = Arc::new(FakeBackend::new());
        let pool = Pool::new(be.clone(), cfg(2, 4, false));
        pool.refill().await;
        assert_eq!(pool.ready_len(), 2);

        let resp = pool.run(&req()).await.unwrap();
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("ran python"));

        // The consumed container is destroyed and the pool refilled.
        settle_ready(&pool, 2).await;
        assert_eq!(be.destroyed.lock().unwrap().len(), 1, "single-use teardown");
        // Created: 2 warm + 1 refill = 3; one destroyed → 2 live.
        assert_eq!(be.live_count(), 2);
    }

    #[tokio::test]
    async fn refresh_image_seeds_then_recycles_on_change() {
        let be = Arc::new(FakeBackend::new()); // image "img-v1"
        let pool = Pool::new(be.clone(), cfg(2, 4, false));
        // First refresh seeds the image and warms the pool.
        pool.refresh_image().await;
        settle_ready(&pool, 2).await;
        assert!(pool.ready_images().iter().all(|i| i == "img-v1"));
        let created_v1 = be.created.lock().unwrap().len();
        assert_eq!(created_v1, 2);

        // Image gets rebuilt/re-tagged → next refresh drains v1 and re-warms.
        be.set_image("img-v2");
        pool.refresh_image().await;
        settle_ready(&pool, 2).await;
        assert!(
            pool.ready_images().iter().all(|i| i == "img-v2"),
            "pool must be re-warmed on the new image: {:?}",
            pool.ready_images()
        );
        // The two v1 containers were destroyed and two v2 ones created.
        settle_destroyed(&be, 2).await;
        assert_eq!(be.destroyed.lock().unwrap().len(), 2);
        assert_eq!(be.created.lock().unwrap().len(), 4);
    }

    #[tokio::test]
    async fn refresh_image_noop_when_unchanged() {
        let be = Arc::new(FakeBackend::new());
        let pool = Pool::new(be.clone(), cfg(2, 4, false));
        pool.refresh_image().await;
        settle_ready(&pool, 2).await;
        let created = be.created.lock().unwrap().len();
        // Same image → no drain, no re-create.
        pool.refresh_image().await;
        assert_eq!(be.created.lock().unwrap().len(), created);
        assert_eq!(be.destroyed.lock().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn stale_warm_container_is_discarded_on_checkout() {
        // Simulate the race: pool warmed on v1, image flips to v2, but the
        // periodic drain hasn't run yet. A job must NOT be served the stale
        // v1 container — it's destroyed and a fresh one booted.
        let be = Arc::new(FakeBackend::new());
        let pool = Pool::new(be.clone(), cfg(1, 4, false));
        pool.refresh_image().await;
        settle_ready(&pool, 1).await;
        // Flip the pool's notion of the target image WITHOUT draining (mimic
        // the window before refresh_image fires).
        be.set_image("img-v2");
        *pool.image.lock().unwrap() = "img-v2".to_string();
        let before = be.created.lock().unwrap().len();
        pool.run(&req()).await.unwrap();
        // The stale v1 container was destroyed; a fresh on-demand one ran.
        assert!(be.created.lock().unwrap().len() > before);
        settle_destroyed(&be, 1).await;
        assert!(!be.destroyed.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn pooled_container_is_default_deny_not_egress() {
        let be = Arc::new(FakeBackend::new());
        let pool = Pool::new(be.clone(), cfg(1, 2, true));
        pool.refill().await;
        let created = be.created.lock().unwrap();
        assert!(created.iter().all(|(_, net)| *net == Network::None));
    }

    #[tokio::test]
    async fn egress_request_creates_on_demand_networked_container() {
        let be = Arc::new(FakeBackend::new());
        let pool = Pool::new(be.clone(), cfg(0, 2, true));
        let mut r = req();
        r.network = true;
        pool.run(&r).await.unwrap();
        let created = be.created.lock().unwrap();
        assert!(
            created.iter().any(|(_, net)| *net == Network::Egress),
            "a networked call must get an egress container: {created:?}"
        );
    }

    #[tokio::test]
    async fn egress_request_rejected_when_not_configured() {
        let be = Arc::new(FakeBackend::new());
        let pool = Pool::new(be.clone(), cfg(1, 2, false));
        let mut r = req();
        r.network = true;
        let err = pool.run(&r).await.unwrap_err();
        assert!(matches!(err, RunnerError::NetworkUnavailable));
    }

    /// A `Config` with lease knobs made explicit, for the lease tests.
    fn cfg_leases(pool_size: usize, max_leases: usize, lease_ttl_secs: u64) -> Arc<Config> {
        let mut c = (*cfg(pool_size, 8, false)).clone();
        c.max_leases = max_leases;
        c.lease_ttl_secs = lease_ttl_secs;
        Arc::new(c)
    }

    #[tokio::test]
    async fn keep_alive_leases_and_reuses_the_same_container() {
        let be = Arc::new(FakeBackend::new());
        let pool = Pool::new(be.clone(), cfg_leases(1, 6, 600));
        pool.refill().await;

        // First keep-alive call: the warm container becomes a lease and its
        // id is echoed back. Nothing destroyed yet.
        let mut r = req();
        r.keep_alive = true;
        let resp1 = pool.run(&r).await.unwrap();
        let id = resp1.container_id.clone().expect("lease id echoed");
        assert_eq!(pool.leased_len(), 1);
        settle_ready(&pool, 1).await; // pool refilled after consuming the warm one
        assert!(
            be.destroyed.lock().unwrap().is_empty(),
            "a leased container must not be torn down"
        );

        // Second call naming that lease reuses the SAME container (a second
        // exec into it) — no new job container is created for the exec.
        let mut r2 = req();
        r2.keep_alive = true;
        r2.container_id = Some(id.clone());
        let resp2 = pool.run(&r2).await.unwrap();
        assert_eq!(resp2.container_id.as_deref(), Some(id.as_str()));
        assert!(resp2.stdout.contains(&id), "ran in the leased container");
        assert_eq!(pool.leased_len(), 1);
        assert!(be.destroyed.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn keep_alive_false_is_still_single_use() {
        let be = Arc::new(FakeBackend::new());
        let pool = Pool::new(be.clone(), cfg_leases(1, 6, 600));
        pool.refill().await;
        let resp = pool.run(&req()).await.unwrap();
        assert_eq!(resp.container_id, None, "no lease id for a single-use job");
        assert_eq!(pool.leased_len(), 0);
        settle_destroyed(&be, 1).await;
    }

    #[tokio::test]
    async fn checked_teardown_invalid_release_never_enters_retirement_tracking() {
        let backend = Arc::new(FakeBackend::new());
        let pool = Pool::new(backend.clone(), cfg_leases(0, 6, 600));
        for id in ["-f", "", "fake-1/../other"] {
            assert!(pool.release_container(id).await.is_err());
            assert!(pool.leased.lock().unwrap().is_empty());
        }
        assert!(backend.destroyed.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn checked_teardown_poisoned_lease_is_retiring_before_async_destruction() {
        let backend = Arc::new(FakeBackend::new());
        let pool = Pool::new(backend.clone(), cfg_leases(0, 6, 600));
        pool.leased.lock().unwrap().insert(
            "fake-0".into(),
            Leased {
                last_used: Instant::now(),
                busy: true,
                retiring: false,
                scope: None,
                network: Network::None,
                workspace: None,
                handover_generation: 0,
                handover: None,
            },
        );
        assert_eq!(pool.dispose("fake-0", true, true, false), None);
        assert!(matches!(
            pool.mark_lease_busy("fake-0"),
            Err(RunnerError::Busy)
        ));
        assert!(pool.leased.lock().unwrap().get("fake-0").unwrap().retiring);
        settle_destroyed(&backend, 1).await;
    }

    #[tokio::test]
    async fn checked_teardown_release_failure_retains_tracking_until_retry_confirms_absence() {
        let be = Arc::new(FakeBackend::new());
        let pool = Pool::new(be.clone(), cfg_leases(0, 6, 600));
        let mut request = req();
        request.keep_alive = true;
        let id = pool.run(&request).await.unwrap().container_id.unwrap();
        be.fail_destroy
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(pool.release_container(&id).await.is_err());
        assert_eq!(be.live_count(), 1);
        assert!(
            pool.leased.lock().unwrap().contains_key(&id),
            "unconfirmed container absence must retain tracking"
        );
        request.container_id = Some(id.clone());
        assert!(matches!(pool.run(&request).await, Err(RunnerError::Busy)));
        assert_eq!(be.execs.load(std::sync::atomic::Ordering::SeqCst), 1);
        be.fail_destroy
            .store(false, std::sync::atomic::Ordering::SeqCst);
        pool.release_container(&id).await.unwrap();
        assert_eq!(be.live_count(), 0);
        assert_eq!(pool.leased_len(), 0);
    }

    #[tokio::test]
    async fn checked_teardown_ttl_failure_retains_tracking_until_sweeper_retry() {
        let be = Arc::new(FakeBackend::new());
        let pool = Pool::new(be.clone(), cfg_leases(0, 6, 600));
        let mut request = req();
        request.keep_alive = true;
        let id = pool.run(&request).await.unwrap().container_id.unwrap();
        pool.leased.lock().unwrap().get_mut(&id).unwrap().last_used =
            Instant::now() - Duration::from_secs(601);
        be.fail_destroy
            .store(true, std::sync::atomic::Ordering::SeqCst);
        pool.sweep_expired().await;
        assert_eq!(be.live_count(), 1);
        assert!(
            pool.leased.lock().unwrap().contains_key(&id),
            "failed TTL teardown must retain tracking"
        );
        be.fail_destroy
            .store(false, std::sync::atomic::Ordering::SeqCst);
        pool.sweep_expired().await;
        assert_eq!(be.live_count(), 0);
        assert_eq!(pool.leased_len(), 0);
    }

    #[tokio::test]
    async fn checked_teardown_same_container_concurrent_exec_is_refused() {
        let be = Arc::new(FakeBackend::new());
        let pool = Pool::new(be.clone(), cfg_leases(0, 6, 600));
        let mut request = req();
        request.keep_alive = true;
        let id = pool.run(&request).await.unwrap().container_id.unwrap();
        request.container_id = Some(id.clone());
        let mut blocked = request.clone();
        blocked.code = "hold-exec".into();
        let active = tokio::spawn({
            let pool = pool.clone();
            async move { pool.run(&blocked).await }
        });
        tokio::time::timeout(Duration::from_secs(2), be.exec_entered.notified())
            .await
            .unwrap();
        let concurrent = tokio::time::timeout(Duration::from_secs(2), pool.run(&request))
            .await
            .unwrap();
        be.exec_release.notify_one();
        tokio::time::timeout(Duration::from_secs(2), active)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(
            matches!(concurrent, Err(RunnerError::Busy)),
            "{concurrent:?}"
        );
        assert_eq!(be.execs.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert_eq!(be.live_count(), 1);
        assert_eq!(pool.leased_len(), 1);
        pool.release_container(&id).await.unwrap();
    }

    #[tokio::test]
    async fn release_container_destroys_and_untracks_and_is_idempotent() {
        let be = Arc::new(FakeBackend::new());
        let pool = Pool::new(be.clone(), cfg_leases(0, 6, 600));
        let mut r = req();
        r.keep_alive = true;
        let id = pool.run(&r).await.unwrap().container_id.unwrap();
        assert_eq!(pool.leased_len(), 1);

        pool.release_container(&id).await.unwrap();
        assert_eq!(pool.leased_len(), 0);
        assert!(be.destroyed.lock().unwrap().contains(&id));

        // A second release (e.g. sweeper raced the gateway) must not panic.
        pool.release_container(&id).await.unwrap();
        assert_eq!(pool.leased_len(), 0);
    }

    #[tokio::test]
    async fn lease_cap_falls_back_to_single_use() {
        let be = Arc::new(FakeBackend::new());
        // Cap at 1 lease.
        let pool = Pool::new(be.clone(), cfg_leases(0, 1, 600));

        let mut r = req();
        r.keep_alive = true;
        let first = pool.run(&r).await.unwrap();
        assert!(first.container_id.is_some(), "first lease granted");
        assert_eq!(pool.leased_len(), 1);

        // Second keep-alive request: cap is full, so the job runs single-use
        // and returns NO lease id — the caller gracefully loses persistence
        // rather than the runner over-committing.
        let second = pool.run(&r).await.unwrap();
        assert_eq!(second.container_id, None, "over-cap request not leased");
        assert_eq!(pool.leased_len(), 1);
        settle_destroyed(&be, 1).await;
    }

    #[tokio::test]
    async fn sweeper_reaps_idle_lease_but_skips_busy() {
        let be = Arc::new(FakeBackend::new());
        // TTL 0 → any idle lease is immediately expired once a moment passes.
        let pool = Pool::new(be.clone(), cfg_leases(0, 6, 0));

        // A real idle lease.
        let mut r = req();
        r.keep_alive = true;
        let idle = pool.run(&r).await.unwrap().container_id.unwrap();

        // A synthetic busy lease the sweeper must leave alone.
        pool.leased.lock().unwrap().insert(
            "busy-1".to_string(),
            Leased {
                last_used: Instant::now(),
                busy: true,
                retiring: false,
                scope: None,
                network: Network::None,
                workspace: None,
                handover_generation: 0,
                handover: None,
            },
        );

        // Let a moment pass so the idle lease's age exceeds the 0s TTL.
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        pool.sweep_expired().await;

        assert!(
            be.destroyed.lock().unwrap().contains(&idle),
            "idle lease reaped"
        );
        assert!(
            pool.leased.lock().unwrap().contains_key("busy-1"),
            "busy lease must survive the sweep"
        );
        assert!(!pool.leased.lock().unwrap().contains_key(&idle));
    }

    /// Backend whose `exec` always errors, to prove a failed job doesn't
    /// strand a lease. `create`/`destroy` are recorded so we can assert the
    /// single-use teardown happened.
    struct FailExecBackend {
        created: Mutex<usize>,
        destroyed: Mutex<Vec<String>>,
    }
    #[async_trait::async_trait]
    impl ContainerBackend for FailExecBackend {
        fn validate_container_id(&self, id: &str) -> Result<(), crate::backend::BackendError> {
            if !id.is_empty()
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            {
                Ok(())
            } else {
                Err(crate::backend::BackendError::Protocol(
                    "invalid test container ID".into(),
                ))
            }
        }
        async fn create(
            &self,
            _network: Network,
            _workspace: Option<&WorkspaceMount>,
        ) -> Result<String, crate::backend::BackendError> {
            let mut n = self.created.lock().unwrap();
            *n += 1;
            Ok(format!("fail-{n}"))
        }
        async fn exec(
            &self,
            _id: &str,
            _req: &RunRequest,
            _timeout: std::time::Duration,
        ) -> Result<RunResponse, crate::backend::BackendError> {
            Err(crate::backend::BackendError::Protocol("boom".into()))
        }
        async fn destroy(&self, id: &str) -> Result<(), crate::backend::BackendError> {
            self.destroyed.lock().unwrap().push(id.to_string());
            Ok(())
        }
    }

    #[tokio::test]
    async fn failed_exec_does_not_strand_a_lease() {
        let be = Arc::new(FailExecBackend {
            created: Mutex::new(0),
            destroyed: Mutex::new(Vec::new()),
        });
        let pool = Pool::new(be.clone(), cfg_leases(0, 6, 600));
        let mut r = req();
        r.keep_alive = true; // wants a lease…
        let err = pool.run(&r).await;
        assert!(err.is_err(), "exec error propagates");
        // …but a job that never ran must not become a lease the gateway can't
        // address; the fresh container is torn down single-use instead.
        assert_eq!(pool.leased_len(), 0, "no stranded lease on exec failure");
        settle_destroyed_generic(&be.destroyed, 1).await;
    }

    /// Like `settle_destroyed` but for a plain `Mutex<Vec<String>>`.
    async fn settle_destroyed_generic(destroyed: &Mutex<Vec<String>>, want: usize) {
        for _ in 0..2000 {
            if destroyed.lock().unwrap().len() >= want {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        panic!("never reached {want} destroyed");
    }

    #[tokio::test]
    async fn reused_exec_error_drops_the_poisoned_lease() {
        // A reused container whose exec fails must have its lease dropped and
        // the container destroyed, so the gateway's next call recreates a clean
        // one instead of re-exec'ing into a dead container all turn.
        let be = Arc::new(FailExecBackend {
            created: Mutex::new(0),
            destroyed: Mutex::new(Vec::new()),
        });
        let pool = Pool::new(be.clone(), cfg_leases(0, 6, 600));
        // Pre-register a live lease, then drive a reuse call at it.
        pool.leased.lock().unwrap().insert(
            "live-1".to_string(),
            Leased {
                last_used: Instant::now(),
                busy: false,
                retiring: false,
                scope: None,
                network: Network::None,
                workspace: None,
                handover_generation: 0,
                handover: None,
            },
        );
        let mut r = req();
        r.keep_alive = true;
        r.container_id = Some("live-1".to_string());
        assert!(pool.run(&r).await.is_err(), "exec error propagates");
        assert_eq!(pool.leased_len(), 0, "poisoned lease dropped");
        settle_destroyed_generic(&be.destroyed, 1).await;
        assert!(be.destroyed.lock().unwrap().contains(&"live-1".to_string()));
    }

    /// Backend whose exec always reports a wall-clock timeout (Ok, timed_out).
    struct TimeoutBackend {
        created: Mutex<usize>,
        destroyed: Mutex<Vec<String>>,
    }
    #[async_trait::async_trait]
    impl ContainerBackend for TimeoutBackend {
        fn validate_container_id(&self, id: &str) -> Result<(), crate::backend::BackendError> {
            if !id.is_empty()
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            {
                Ok(())
            } else {
                Err(crate::backend::BackendError::Protocol(
                    "invalid test container ID".into(),
                ))
            }
        }
        async fn create(
            &self,
            _network: Network,
            _workspace: Option<&WorkspaceMount>,
        ) -> Result<String, crate::backend::BackendError> {
            let mut n = self.created.lock().unwrap();
            *n += 1;
            Ok(format!("to-{n}"))
        }
        async fn exec(
            &self,
            _id: &str,
            _req: &RunRequest,
            _timeout: std::time::Duration,
        ) -> Result<RunResponse, crate::backend::BackendError> {
            Ok(RunResponse {
                exit_code: -1,
                stdout: String::new(),
                stderr: "timed out".into(),
                artifacts: vec![],
                duration_ms: 1,
                timed_out: true,
                output_truncated: false,
                artifacts_truncated: false,
                container_id: None,
            })
        }
        async fn destroy(&self, id: &str) -> Result<(), crate::backend::BackendError> {
            self.destroyed.lock().unwrap().push(id.to_string());
            Ok(())
        }
    }

    #[tokio::test]
    async fn timed_out_keepalive_job_is_not_leased() {
        // A timeout returns Ok(timed_out:true), but the runaway process is
        // still alive in the container, so it must be torn down single-use, not
        // kept as a lease (which would poison the turn's next call).
        let be = Arc::new(TimeoutBackend {
            created: Mutex::new(0),
            destroyed: Mutex::new(Vec::new()),
        });
        let pool = Pool::new(be.clone(), cfg_leases(0, 6, 600));
        let mut r = req();
        r.keep_alive = true;
        let resp = pool.run(&r).await.unwrap();
        assert!(resp.timed_out);
        assert_eq!(resp.container_id, None, "timed-out job not leased");
        assert_eq!(pool.leased_len(), 0);
        settle_destroyed_generic(&be.destroyed, 1).await;
    }

    #[tokio::test]
    async fn stale_lease_id_falls_back_to_a_fresh_container() {
        // The gateway sends an id the runner no longer tracks (swept mid-turn):
        // the job must still run — in a fresh container — and get a new lease
        // id back, not error.
        let be = Arc::new(FakeBackend::new());
        let pool = Pool::new(be.clone(), cfg_leases(0, 6, 600));
        let mut r = req();
        r.keep_alive = true;
        r.container_id = Some("gone-forever".to_string());
        let resp = pool.run(&r).await.unwrap();
        let id = resp.container_id.expect("a fresh lease id");
        assert_ne!(id, "gone-forever");
        assert_eq!(pool.leased_len(), 1);
    }

    #[test]
    fn clamp_output_keeps_head_and_tail() {
        // Distinct head/tail so we can prove BOTH ends survive.
        let body = format!("{}{}", "A".repeat(500), "Z".repeat(500));
        let mut resp = RunResponse {
            exit_code: 0,
            stdout: body,
            stderr: String::new(),
            artifacts: vec![],
            duration_ms: 0,
            timed_out: false,
            output_truncated: false,
            artifacts_truncated: false,
            container_id: None,
        };
        clamp_output(&mut resp, 200);
        assert!(resp.output_truncated);
        assert!(resp.stdout.len() < 1000);
        assert!(resp.stdout.contains("omitted"));
        assert!(resp.stdout.starts_with('A'), "head kept");
        assert!(resp.stdout.ends_with('Z'), "tail kept");
    }
}
