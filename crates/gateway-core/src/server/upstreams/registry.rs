// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Runtime routing.
//!
//! Each backend tracks the set of models it currently advertises (populated
//! by the health probe in `health.rs`, which parses the OpenAI-shape
//! `/models` response on every successful probe). A request comes in with a
//! `model` string + a `PoolKind`; we walk pools matching the kind, pick the
//! first one that has at least one healthy backend advertising the model,
//! and acquire an inflight slot on a matching backend via the pool's
//! picker strategy.
//!
//! No static `model_routes` table — the gateway derives routes primarily
//! from what each upstream reports. A backend whose `/models` probe returns
//! nothing (no such endpoint, `401`, unparseable body) falls back to its
//! configured model ids (backend `models`, else the pool's) so it stays
//! routable; the live probe wins whenever it reports anything. If two pools
//! of the same kind both advertise the same model name, the first one in
//! config-order wins (`HashMap` iteration is unordered, so for deterministic
//! priority callers should keep one pool per kind in practice).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};

use arc_swap::ArcSwap;
use thiserror::Error;

use super::config::{
    BackendConfig, Compliance, FallbackConfig, PickerStrategy, PoolKind, UpstreamPoolConfig,
};
use super::profile::{BackendProfile, Detected};

/// A configured alias and its current state, for the read-only admin view.
#[derive(Debug, Clone)]
pub struct AliasStatus {
    /// The client-facing name.
    pub name: String,
    /// Explicit target real id (map form), or `None` for a bare list alias
    /// that binds to the backend's sole model.
    pub target: Option<String>,
    /// True when a bare alias is currently disabled because the backend serves
    /// more than one model (ambiguous) — see [`Backend::reevaluate_aliases`].
    pub disabled: bool,
    /// Whether the alias actually resolves right now, i.e. whether a request
    /// naming it would route.
    ///
    /// Separate from [`Self::disabled`], which only ever describes the *bare*
    /// form. A map alias is never "disabled" — it simply resolves or doesn't,
    /// depending on whether its target is currently served — and an operator who
    /// mistypes that target (easy: a vLLM reports its full repo path, not the
    /// short name anyone would guess) gets an alias that is configured, listed
    /// in the editor, and silently unroutable. This is the flag that lets the
    /// admin page say so.
    pub resolves: bool,
}

/// A single upstream backend with the runtime state we need to schedule it.
pub struct Backend {
    pub name: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub weight: u32,
    pub max_inflight: u32,
    pub health_path: String,
    /// Whether the probe may overwrite [`models`](Self::models) from a
    /// `/models` response. `false` pins the model set to `config_models`
    /// (see [`BackendConfig::probe_models`]).
    probe_models: bool,
    /// Whether this backend can edit images (image-to-image), not just
    /// generate. Only meaningful on image pools; see
    /// [`BackendConfig::supports_edit`].
    supports_edit: bool,
    inflight: AtomicU32,
    /// Total requests ever dispatched to this backend by this process.
    ///
    /// The load signal `inflight` cannot provide. An agent client waits for each
    /// answer before sending the next, so at the moment a request is routed
    /// *every* replica reads zero in flight — instantaneous load is blind to
    /// sequential traffic, which is most agent traffic. Compared as a **ratio**
    /// (never an absolute), so a monotonic counter converges on proportional
    /// sharing instead of eventually declaring everything overloaded.
    dispatched: AtomicU64,
    healthy: AtomicBool,
    /// Set when the health probe's last attempt was rejected by the upstream's
    /// authentication (`401`/`403`), cleared by the first probe that comes back
    /// with model data.
    ///
    /// A rejected probe is deliberately **not** "unhealthy": some upstreams
    /// protect `/models` differently from `/chat/completions`, so real traffic
    /// may work fine. But it does mean model discovery is off, and a backend
    /// whose model set never fills is a backend nothing routes to — the exact
    /// state that once presented as a green "up" badge serving no models, with
    /// an alias that silently bound to nothing. This flag exists so the admin
    /// page can say "the key was rejected" instead of showing health and
    /// leaving the operator to guess.
    auth_failed: AtomicBool,
    /// Maintenance switch (see [`BackendConfig::enabled`]). `false` = the picker
    /// skips this backend; everything else about it keeps working, health probe
    /// included, so the admin page still shows whether the box is back.
    ///
    /// Atomic rather than a plain `bool` because it is flipped **live**, without
    /// a topology reload — a maintenance switch you have to "apply" isn't one.
    enabled: AtomicBool,
    /// The set of model IDs this backend currently advertises, as reported
    /// by its most recent successful `/models` probe. Empty until the first
    /// probe completes (`health::spawn` does an initial blocking round so
    /// the first request finds something). Updated by the probe loop
    /// whenever the upstream's loadout changes.
    models: RwLock<HashSet<String>>,
    /// Static fallback model IDs from config (backend `models`, else the
    /// pool's `models`). Used only while `models` (the live probe set) is
    /// empty — see [`Backend::with_effective_models`] for the precedence.
    /// Lets a backend without a working `/models` endpoint (e.g. Voxtral
    /// realtime) still be routable and advertised.
    config_models: HashSet<String>,
    /// Client-facing aliases this backend answers to, from config: alias name →
    /// optional explicit target real id. `Some(id)` (map form) pins a specific
    /// model; `None` (bare list form) resolves to the backend's sole model at
    /// request time. Static — aliases never come from the probe.
    aliases: HashMap<String, Option<String>>,
    /// Bare aliases currently disabled because the backend serves ≠1 model, so
    /// "the sole model" is ambiguous. Recomputed by [`Backend::reevaluate_aliases`]
    /// whenever the effective model set changes (probe update or construction);
    /// a disabled alias stops resolving until the ambiguity clears. Map-form
    /// aliases are never disabled (they name their target explicitly).
    disabled_aliases: RwLock<HashSet<String>>,
    /// Context window per model, as the last `/models` probe reported it
    /// (vLLM and friends put `max_model_len` on each entry). Empty for
    /// backends whose `/models` omits it, which is most hosted APIs.
    ///
    /// The probe already reads this response; discarding the window meant
    /// every model fell back to the global `default_context_window` — 32768,
    /// eight times smaller than what a Qwen3 deployment actually serves —
    /// which silently shrank both the compaction trigger and the turn's
    /// tool-output allowance. Discovered rather than configured, because an
    /// operator has to notice the field exists before they can set it, and
    /// the backend already knows the answer.
    context_windows: RwLock<HashMap<String, i64>>,
    /// What the last identification round made of this server, and what it
    /// said about itself — see `upstreams::profile`. Observed, never
    /// configured.
    ///
    /// One lock rather than a field each, because every part of it is written
    /// in the same breath and read in the same breath. Five independent locks
    /// could be observed half-updated, and each new fact a server might report
    /// cost five edits to add.
    ///
    /// `context_windows` inside it is *not* used: windows are the probe's,
    /// refreshed every tick (they are runtime state — see
    /// `BackendProfile::context_endpoint`). They ride along here only so a
    /// boot against an unreachable backend can seed the probe's map from the
    /// database before any probe succeeds.
    detected: RwLock<Detected>,
}

impl Backend {
    /// `pool_models` is the pool-level fallback, applied when this backend
    /// declares no `models` of its own (backend config wins over pool).
    fn new(cfg: &BackendConfig, pool_models: &[String]) -> Self {
        let fallback = if cfg.models.is_empty() {
            pool_models
        } else {
            &cfg.models
        };
        let config_models: HashSet<String> =
            fallback.iter().filter(|s| !s.is_empty()).cloned().collect();
        let aliases = cfg.alias.as_ref().map(|a| a.into_map()).unwrap_or_default();
        let backend = Self {
            name: cfg.name.clone(),
            base_url: cfg.base_url.trim_end_matches('/').to_string(),
            api_key: cfg.api_key(),
            weight: cfg.weight.max(1),
            max_inflight: cfg.max_inflight.max(1),
            health_path: cfg.health_path.clone(),
            probe_models: cfg.probe_models,
            supports_edit: cfg.supports_edit,
            inflight: AtomicU32::new(0),
            dispatched: AtomicU64::new(0),
            healthy: AtomicBool::new(true),
            auth_failed: AtomicBool::new(false),
            enabled: AtomicBool::new(cfg.enabled),
            models: RwLock::new(HashSet::new()),
            config_models,
            aliases,
            disabled_aliases: RwLock::new(HashSet::new()),
            context_windows: RwLock::new(HashMap::new()),
            detected: RwLock::new(Detected::default()),
        };
        // Evaluate against the config-model set now, so a bare alias declared
        // alongside multiple static models is disabled from the start; the
        // first probe re-evaluates against the live set.
        //
        // Silent about the *zero*-model case: at construction the probe has
        // definitionally not run, so every bare alias on every probe-discovered
        // backend would warn on every boot and then immediately re-enable
        // itself a moment later. A warning that fires every time it is not a
        // problem is how an operator learns to ignore warnings.
        backend.reevaluate_aliases_quiet_when_empty(true);
        backend
    }

    pub fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::Relaxed)
    }

    pub fn set_healthy(&self, h: bool) {
        self.healthy.store(h, Ordering::Relaxed);
    }

    /// Whether this backend may take traffic (the maintenance switch). Health
    /// is a separate question — see [`Self::is_available`].
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Flip the maintenance switch on the live backend. Persisting it is the
    /// caller's job (`db::upstreams_config::set_backend_enabled`).
    pub fn set_enabled(&self, on: bool) {
        self.enabled.store(on, Ordering::Relaxed);
    }

    /// The one question the picker asks: may this backend take a request right
    /// now? Enabled **and** healthy.
    ///
    /// Every routing path goes through this rather than `is_healthy`, so a
    /// drained backend cannot be reached by any of them — while `knows_model`
    /// stays deliberately unaware of it, which is what keeps a drained pool
    /// answering "temporarily unavailable" instead of "no such model".
    pub fn is_available(&self) -> bool {
        self.is_enabled() && self.is_healthy()
    }

    /// Whether the last probe was rejected by upstream auth — see
    /// [`Self::auth_failed`](#structfield.auth_failed).
    pub fn auth_failed(&self) -> bool {
        self.auth_failed.load(Ordering::Relaxed)
    }

    /// Record (or clear) an upstream auth rejection on the health path.
    pub fn set_auth_failed(&self, failed: bool) {
        self.auth_failed.store(failed, Ordering::Relaxed);
    }

    pub fn inflight(&self) -> u32 {
        self.inflight.load(Ordering::Relaxed)
    }

    /// Total requests dispatched here since this process started — see
    /// [`Self::dispatched`](#structfield.dispatched).
    pub fn dispatched(&self) -> u64 {
        self.dispatched.load(Ordering::Relaxed)
    }

    /// Runs `f` against this backend's *effective* model set — the set the
    /// backend actually serves and advertises. The single place the
    /// probe/config precedence lives; the read lock is held for the duration
    /// of `f`. Precedence:
    ///   - **live probe + a configured `models` list** → the *intersection*:
    ///     the list is an allowlist, so a probed model it doesn't name is
    ///     discovered-but-withheld (not served, not advertised);
    ///   - **live probe + empty list** → the whole probe set (offer everything
    ///     the backend reports);
    ///   - **no live probe** → the configured `models` verbatim (the static
    ///     fallback for backends that don't self-report via `/models`).
    fn with_effective_models<R>(&self, f: impl FnOnce(&HashSet<String>) -> R) -> R {
        if let Ok(probe) = self.models.read()
            && !probe.is_empty()
        {
            if self.config_models.is_empty() {
                return f(&probe);
            }
            let allowed: HashSet<String> =
                probe.intersection(&self.config_models).cloned().collect();
            return f(&allowed);
        }
        f(&self.config_models)
    }

    /// The models the backend reports via `/models` but its allowlist withholds
    /// — i.e. `live probe \ effective`. Empty unless a configured `models` list
    /// is actively filtering a live probe. Drives the struck-through
    /// "discovered but not served" chips in the admin health view; never
    /// consulted on the routing hot path.
    pub fn withheld_models(&self) -> HashSet<String> {
        let Ok(probe) = self.models.read() else {
            return HashSet::new();
        };
        if probe.is_empty() || self.config_models.is_empty() {
            return HashSet::new();
        }
        probe.difference(&self.config_models).cloned().collect()
    }

    /// Real-model membership only (no aliases): the backend's effective set
    /// (live probe, else config fallback) contains `model`.
    ///
    /// Spelled out rather than going through
    /// [`with_effective_models`](Self::with_effective_models), because that
    /// *materialises* the intersection — a fresh `HashSet` with a cloned
    /// `String` per model — whenever a configured allowlist is filtering a live
    /// probe. This is a membership test on the routing path, and it is also
    /// reached once per model from the admin pages; the same answer comes out
    /// of two `contains` calls with no allocation at all.
    fn serves_real(&self, model: &str) -> bool {
        if let Ok(probe) = self.models.read()
            && !probe.is_empty()
        {
            return probe.contains(model)
                && (self.config_models.is_empty() || self.config_models.contains(model));
        }
        self.config_models.contains(model)
    }

    /// The backend's sole effective model, if it serves exactly one. Backs
    /// bare (list-form) alias resolution — an alias with no explicit target
    /// binds to this. `None` when the backend serves zero or several models.
    fn sole_model(&self) -> Option<String> {
        self.with_effective_models(|set| {
            if set.len() == 1 {
                set.iter().next().cloned()
            } else {
                None
            }
        })
    }

    /// True if a bare alias is currently disabled (the backend serves ≠1
    /// model, so "the sole model" is ambiguous — see `reevaluate_aliases`).
    fn alias_disabled(&self, name: &str) -> bool {
        self.disabled_aliases
            .read()
            .map(|g| g.contains(name))
            .unwrap_or(false)
    }

    /// Resolve a requested name to the **real model id** this backend would
    /// forward to the upstream, or `None` if it doesn't serve it. A real id
    /// always wins over an alias of the same spelling (identity). A map-form
    /// alias resolves to its target only while that target is actually served;
    /// a bare alias resolves to the backend's sole model while it isn't
    /// disabled. Health is not considered here; callers gate on `is_healthy`.
    pub fn resolve(&self, requested: &str) -> Option<String> {
        if self.serves_real(requested) {
            return Some(requested.to_string());
        }
        match self.aliases.get(requested) {
            Some(Some(target)) => self.serves_real(target).then(|| target.clone()),
            Some(None) => {
                if self.alias_disabled(requested) {
                    None
                } else {
                    self.sole_model()
                }
            }
            None => None,
        }
    }

    /// Returns true if this backend currently serves `model` — as a real
    /// advertised id, or as a resolvable alias. Health is *not* considered
    /// here; callers gate on `is_healthy`. Cheap on the common real-id path
    /// (no allocation); only an alias hit does the extra lookup.
    pub fn serves_model(&self, model: &str) -> bool {
        if self.serves_real(model) {
            return true;
        }
        match self.aliases.get(model) {
            Some(Some(target)) => self.serves_real(target),
            Some(None) => !self.alias_disabled(model) && self.sole_model().is_some(),
            None => false,
        }
    }

    /// Whether the probe is allowed to discover this backend's model set from
    /// `/models`. `false` pins the set to `config_models` — see
    /// [`BackendConfig::probe_models`].
    pub fn probe_models_enabled(&self) -> bool {
        self.probe_models
    }

    /// Whether this backend advertises image-editing support. Only meaningful
    /// on image pools.
    pub fn supports_edit(&self) -> bool {
        self.supports_edit
    }

    /// Replace the advertised-model set wholesale. Probe-only path —
    /// called from `health.rs` after a successful `/models` parse so the
    /// next routing lookup reflects the upstream's current loadout. Also
    /// re-evaluates bare-alias ambiguity against the new set.
    pub fn set_models(&self, models: HashSet<String>) {
        if let Ok(mut guard) = self.models.write() {
            *guard = models;
        }
        self.reevaluate_aliases();
    }

    /// Record the context windows the last `/models` probe reported, keyed by
    /// model id. Entries for models this backend no longer serves are dropped
    /// with the rest — the map is replaced, not merged.
    pub fn set_context_windows(&self, windows: HashMap<String, i64>) {
        if let Ok(mut guard) = self.context_windows.write() {
            *guard = windows;
        }
    }

    /// Record what a detection round learned, wholesale.
    ///
    /// The window map it carries seeds [`Self::context_windows`] only while
    /// that is still empty — at boot, from the database, before any probe has
    /// succeeded. After that the probe owns windows and refreshes them every
    /// tick, so letting an identification round write them would replace live
    /// readings with whatever was true when the topology was applied.
    pub fn set_detected(&self, detected: &Detected) {
        if let Ok(mut guard) = self.detected.write() {
            *guard = detected.clone();
        }
        if !detected.context_windows.is_empty()
            && let Ok(mut guard) = self.context_windows.write()
            && guard.is_empty()
        {
            *guard = detected.context_windows.clone();
        }
    }

    /// Everything identification last learned. One snapshot rather than an
    /// accessor per field, so a caller that wants several gets a consistent
    /// set and the struct can grow without growing this impl.
    pub fn detected(&self) -> Detected {
        self.detected.read().map(|g| g.clone()).unwrap_or_default()
    }

    /// What kind of server this is. [`BackendProfile::Generic`] until detected.
    ///
    /// Kept as its own accessor because the request path asks only this, on
    /// every turn, and should not clone the rest to find out.
    pub fn profile(&self) -> BackendProfile {
        self.detected
            .read()
            .map(|g| g.profile)
            .unwrap_or(BackendProfile::Generic)
    }

    /// The context this server says it allocated, if it reports one. Refreshed
    /// by the probe — see [`Detected::context_cap`].
    pub fn context_cap(&self) -> Option<i64> {
        self.detected.read().ok().and_then(|g| g.context_cap)
    }

    /// Record the server-wide context allocation the probe just read.
    pub fn set_context_cap(&self, cap: Option<i64>) {
        if let Ok(mut guard) = self.detected.write() {
            guard.context_cap = cap;
        }
    }

    /// The context window one model actually has here: what the `/models`
    /// probe read, capped by what the server says it allocated.
    ///
    /// The cap is the whole point on llama.cpp, where `/models` reports the
    /// model's *trained* context (`n_ctx_train`) and `/props` reports the far
    /// smaller figure it was actually started with. Believing the trained
    /// number puts prompts past what the server will hold, which it then
    /// truncates without telling anyone — the failure this is here to prevent.
    ///
    /// With no probed window the cap stands alone: a server that reports only
    /// its allocation still tells us more than nothing.
    pub fn context_window(&self, model: &str) -> Option<i64> {
        // A backend has nothing to say about a model it does not serve, and
        // saying something anyway is not harmless: the allocated cap and any
        // window left over from a previous loadout would otherwise answer for
        // *every* model id, and `UpstreamRegistry::probed_context_window` takes
        // the minimum across every backend in the deployment. One llama.cpp
        // instance started with `-c 4096` in an unrelated pool would have
        // budgeted a 262k vLLM model at 4096 tokens.
        if !self.serves_model(model) {
            return None;
        }
        // The tightest of what this server reports, for the same reason the
        // tightest backend wins across a pool: a prompt has to fit whatever it
        // meets. Both sources come from the probe, one tick apart at most.
        let probed = self
            .context_windows
            .read()
            .ok()
            .and_then(|g| g.get(model).copied())
            .filter(|w| *w > 0);
        [probed, self.context_cap().filter(|c| *c > 0)]
            .into_iter()
            .flatten()
            .min()
    }

    /// Effective advertised-model set: the live probe set if it reported
    /// anything, otherwise the configured fallback. Allocates; intended for
    /// listing/UI paths (`/v1/models`, the transcription dropdown), not the
    /// request hot path (which uses `serves_model`).
    pub fn models_snapshot(&self) -> HashSet<String> {
        self.with_effective_models(|set| set.clone())
    }

    /// The *live* probe set only (never the config fallback), empty until the
    /// first successful probe. Used by [`UpstreamRegistry::reload`] to carry a
    /// backend's discovered models across a topology swap so routing doesn't gap.
    pub fn live_models(&self) -> HashSet<String> {
        self.models.read().map(|g| g.clone()).unwrap_or_default()
    }

    /// Names a listing surface should advertise: the effective real set plus
    /// every alias that currently resolves (so clients can pick either the
    /// real id or the alias). An alias that can't route right now — disabled,
    /// or a map target that isn't loaded — is omitted so the list never
    /// advertises a name that would 404/503.
    pub fn listed_models(&self) -> HashSet<String> {
        let mut set = self.models_snapshot();
        for name in self.aliases.keys() {
            if self.resolve(name).is_some() {
                set.insert(name.clone());
            }
        }
        set
    }

    /// Recompute which bare (list-form) aliases can currently bind, and disable
    /// the ones that can't. Called at construction and after every probe update.
    /// Logs only on the transition (disable / re-enable), so a steady state
    /// stays silent. Map-form aliases are never disabled: they name their target.
    ///
    /// A bare alias needs **exactly one** effective model. Both other counts
    /// disable it, for different reasons and with different log lines:
    ///
    ///   - `>1` — genuinely ambiguous, the operator has to pick a target. ERROR.
    ///   - `0` — the backend advertises nothing, usually a probe that never
    ///     returned data (an unset `api_key_env` makes `/models` answer 401,
    ///     which counts as reachable). This case used to leave `disabled` at
    ///     `false`: the alias resolved to nothing while the admin page drew it
    ///     as healthy, on a backend badged "up". WARN, and honestly flagged.
    fn reevaluate_aliases(&self) {
        self.reevaluate_aliases_quiet_when_empty(false);
    }

    /// [`Self::reevaluate_aliases`], optionally suppressing the log line for
    /// the "backend advertises nothing" case — see the call in `new`.
    fn reevaluate_aliases_quiet_when_empty(&self, quiet_when_empty: bool) {
        let effective_len = self.with_effective_models(|set| set.len());
        let mut now_disabled: HashSet<String> = HashSet::new();
        if effective_len != 1 {
            for (name, target) in &self.aliases {
                if target.is_none() {
                    now_disabled.insert(name.clone());
                }
            }
        }
        let Ok(mut guard) = self.disabled_aliases.write() else {
            return;
        };
        for name in now_disabled.difference(&guard) {
            if effective_len == 0 {
                if quiet_when_empty {
                    continue;
                }
                tracing::warn!(
                    backend = %self.name,
                    alias = %name,
                    "bare alias `{name}` cannot bind — this backend advertises no models at \
                     all, so requests for `{name}` will not route. Usually a health probe that \
                     never returned data: check the backend's API key (a 401 on /models counts \
                     as reachable but disables model discovery) and that its base URL is right."
                );
            } else {
                tracing::error!(
                    backend = %self.name,
                    alias = %name,
                    models = effective_len,
                    "bare alias `{name}` is ambiguous — this backend now serves multiple models, \
                     so it can't pick one; disabling it. Give it an explicit target with the map \
                     form, e.g. `alias = {{ \"{name}\" = \"<real-model-id>\" }}`."
                );
            }
        }
        for name in guard.difference(&now_disabled) {
            tracing::info!(
                backend = %self.name,
                alias = %name,
                "bare alias `{name}` is no longer ambiguous — re-enabled"
            );
        }
        *guard = now_disabled;
    }

    /// Raw probe-reported set only (no config fallback). For `health.rs`'s
    /// change-detection so the "advertised models updated" diff reflects
    /// what the upstream actually reported, not the static fallback.
    pub fn probe_models(&self) -> HashSet<String> {
        self.models.read().map(|g| g.clone()).unwrap_or_default()
    }

    /// Configured aliases and their current state, sorted by name. For the
    /// read-only `/admin/backends` view.
    pub fn alias_status(&self) -> Vec<AliasStatus> {
        let disabled = self
            .disabled_aliases
            .read()
            .map(|g| g.clone())
            .unwrap_or_default();
        let mut out: Vec<AliasStatus> = self
            .aliases
            .iter()
            .map(|(name, target)| AliasStatus {
                name: name.clone(),
                target: target.clone(),
                disabled: disabled.contains(name),
                // The same call the router makes, so the badge can never claim
                // an alias works while requests for it 404.
                resolves: self.resolve(name).is_some(),
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }
}

/// A pool of backends sharing a strategy and a kind.
pub struct Pool {
    pub name: String,
    pub kind: PoolKind,
    pub strategy: PickerStrategy,
    pub backends: Vec<Arc<Backend>>,
    /// Data-handling attributes for every model this pool serves (default
    /// all-clear). Drives advisory chat-UI warnings; never affects routing.
    pub compliance: Compliance,
    /// Whether calls served by this pool count toward rate limits / quotas
    /// (default `true`; self-hosted pools set `false`). See
    /// [`UpstreamPoolConfig::enforce_limits`] and [`UpstreamRegistry::enforce_limits_for_model`].
    pub enforce_limits: bool,
    /// Language → voice-id map (speech pools only). See
    /// [`UpstreamPoolConfig::voices`] / [`UpstreamPoolConfig::voice_for_language`].
    pub voices: std::collections::HashMap<String, String>,
    /// Voices this pool offers users to choose from, in the operator's order
    /// (speech pools only). See [`UpstreamPoolConfig::offer_voices`] — this is
    /// the menu, `voices` is the resolution.
    pub offer_voices: Vec<String>,
    /// Pool-level configured model ids (the `models` TOML list). Retained so
    /// the speech default-model pick can prefer the operator's declared TTS
    /// model over the `/models` probe — a cloud provider like OpenAI reports
    /// its whole catalogue on `/models`, which would otherwise swamp the pick.
    pub configured_models: Vec<String>,
    /// Backup model when a model this pool *knows* has no healthy backend
    /// (every replica down). `UpstreamRegistry::route` re-resolves the request
    /// to this instead of returning `503`. See [`UpstreamPoolConfig::fallback_offline`].
    fallback_offline: Option<String>,
    /// Gateway-group names allowed to see + route to this pool. Empty =
    /// unrestricted (every user). See [`PoolAccess`] and
    /// [`crate::server::rbac::Resolver::resource_allowed`].
    pub allowed_groups: Vec<String>,
    /// Cursor for round-robin.
    rr_cursor: AtomicUsize,
    /// Which of this pool's replicas was recently sent which prompt prefix.
    /// Only read by [`PickerStrategy::PrefixAffinity`]; see
    /// [`crate::server::upstreams::prefix_index`].
    prefix_index: super::prefix_index::PrefixIndex,
}

/// A resolved caller's pool-access decision, built once per request from the
/// RBAC resolver and threaded into the group-aware listing/routing methods.
/// Encapsulates the opt-in + admin-bypass rule so the registry needs no
/// dependency on `rbac`.
#[derive(Debug, Clone, Default)]
pub struct PoolAccess {
    /// The caller's resolved gateway-group ids (`Resolver::role_ids_for`).
    pub role_ids: Vec<String>,
    /// True to bypass every restriction (`Resolver::is_admin`).
    pub is_admin: bool,
    /// The calling API token's model allowlist, or `None` when the caller is
    /// unrestricted (no token, or a token with no allowlist — the default).
    ///
    /// This rides on `PoolAccess` rather than being checked at each handler
    /// because `PoolAccess` is already threaded through every listing and
    /// routing entry point. Putting it here is what makes the restriction
    /// total: a new `/v1` surface that resolves a model at all cannot forget
    /// to apply it, and — as with pool groups — a model a token may not use
    /// is also invisible in its `/v1/models`, so the list stays a true
    /// capability view rather than a cosmetic filter.
    ///
    /// **Admins do not bypass this one.** `is_admin` waives the operator's
    /// group policy; an allowlist is a property of the credential itself,
    /// chosen by whoever issued it, and an admin's token that says "only
    /// these models" means it.
    pub allowed_models: Option<Arc<HashSet<String>>>,
}

impl PoolAccess {
    /// Full access — used by internal callers that must see the whole topology
    /// (health probes, admin topology views, non-user-facing resolution).
    pub fn all() -> Self {
        Self {
            role_ids: Vec::new(),
            is_admin: true,
            allowed_models: None,
        }
    }

    /// Whether the calling token may use `model`. `true` for every caller
    /// without an allowlist, which is the default for every token.
    pub fn allows_model(&self, model: &str) -> bool {
        match &self.allowed_models {
            None => true,
            Some(set) => set.contains(model),
        }
    }

    /// True when the caller carries a model allowlist at all — lets a handler
    /// tell "this model does not exist" from "this token may not use it"
    /// without leaking which models exist to a caller that has no business
    /// knowing.
    pub fn is_model_restricted(&self) -> bool {
        self.allowed_models.is_some()
    }

    /// Whether the caller may see/route to `pool`: unrestricted pools are open
    /// to all; admins bypass; otherwise the caller must hold a listed group.
    pub fn allows(&self, pool: &Pool) -> bool {
        if self.is_admin || pool.allowed_groups.is_empty() {
            return true;
        }
        pool.allowed_groups
            .iter()
            .any(|g| self.role_ids.iter().any(|r| r == g))
    }
}

/// What the running registry is serving right now — the "before" side of the
/// apply diff. See [`UpstreamRegistry::live_topology`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveTopology {
    /// Sorted by pool name.
    pub pools: Vec<LivePool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LivePool {
    pub name: String,
    pub kind: PoolKind,
    pub strategy: PickerStrategy,
    /// Sorted by backend name.
    pub backends: Vec<LiveBackend>,
}

/// The backend fields worth diffing: the ones a reload actually changes about
/// how requests are dispatched. Secrets are deliberately absent — a diff must
/// never be a place a key can leak, and "the key changed" is not something an
/// operator needs spelled out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveBackend {
    pub name: String,
    pub base_url: String,
    pub weight: u32,
    pub max_inflight: u32,
    pub health_path: String,
}

/// Minimum share of a prompt's blocks that must match before cache locality
/// outranks load — SGLang's `cache_threshold`, same default.
///
/// Below it there is little cache to preserve, so spreading the request is
/// worth more than pinning it.
const PREFIX_MATCH_THRESHOLD: f64 = 0.3;

/// Absolute in-flight difference (per unit of `weight`) above which load
/// outranks affinity. Modelled on SGLang's `balance_abs_threshold`, in requests
/// rather than queued tokens because that is what this gateway meters.
const BALANCE_ABS_THRESHOLD: u32 = 4;

/// Relative load ratio above which load outranks affinity — SGLang's
/// `balance_rel_threshold`, same default.
///
/// The absolute test alone is wrong under uniformly heavy load: with every
/// replica at 40 in-flight, a difference of 5 is noise, and spilling on it
/// would abandon warm caches for nothing. Both tests must fire.
const BALANCE_REL_THRESHOLD: f64 = 1.5;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AcquireError {
    #[error("no healthy backend in pool `{pool}`")]
    NoHealthyBackend { pool: String },
    #[error("all backends in pool `{pool}` are at max inflight")]
    Saturated { pool: String },
}

impl Pool {
    fn new(name: String, cfg: &UpstreamPoolConfig) -> Self {
        let backends = cfg
            .backend
            .iter()
            .map(|b| Arc::new(Backend::new(b, &cfg.models)))
            .collect();
        Self {
            name,
            kind: cfg.kind,
            strategy: cfg.strategy,
            backends,
            compliance: cfg.compliance,
            enforce_limits: cfg.enforce_limits,
            voices: cfg.voices.clone(),
            offer_voices: cfg.offer_voices.clone(),
            configured_models: cfg.models.clone(),
            fallback_offline: cfg.fallback_offline.clone(),
            allowed_groups: cfg.allowed_groups.clone(),
            rr_cursor: AtomicUsize::new(0),
            prefix_index: super::prefix_index::PrefixIndex::default(),
        }
    }

    /// True if at least one healthy backend in the pool advertises `model`.
    /// Used by `UpstreamRegistry::acquire_for` to pick the right pool.
    pub fn serves_model(&self, model: &str) -> bool {
        self.backends
            .iter()
            .any(|b| b.is_available() && b.serves_model(model))
    }

    /// True if *any* backend in the pool serves `model`, regardless of
    /// health. Lets `acquire_for` tell "this model exists here but every
    /// replica is down" (→ 503) from "no backend serves it at all" (→ 404).
    pub fn knows_model(&self, model: &str) -> bool {
        self.backends.iter().any(|b| b.serves_model(model))
    }

    /// The pool's configured offline backup model, if any. Read-only admin view.
    pub fn fallback_offline(&self) -> Option<&str> {
        self.fallback_offline.as_deref()
    }

    /// The real id the first healthy backend resolves `model` to, if any.
    /// Backs the registry's non-acquiring `resolve_model`.
    fn resolve_healthy(&self, model: &str) -> Option<String> {
        self.backends
            .iter()
            .filter(|b| b.is_available())
            .find_map(|b| b.resolve(model))
    }

    /// Picks a healthy backend that advertises `model`, atomically claims an
    /// inflight slot, and returns an `Acquired` guard. Drop releases the
    /// slot. The pool's `strategy` orders the candidate list; saturation
    /// falls through to the next candidate.
    pub fn acquire_for_model(&self, model: &str) -> Result<Acquired, AcquireError> {
        self.acquire_for_model_affine(model, None)
    }

    /// The pool's prefix index, for diagnostics.
    pub fn prefix_index(&self) -> &super::prefix_index::PrefixIndex {
        &self.prefix_index
    }

    /// [`Self::acquire_for_model`] with an optional prefix-affinity key (see
    /// [`crate::server::upstreams::affinity`]). Only
    /// [`PickerStrategy::PrefixAffinity`] reads it; `None` there behaves as
    /// `least_inflight`.
    pub fn acquire_for_model_affine(
        &self,
        model: &str,
        affinity: Option<&super::affinity::AffinityHint>,
    ) -> Result<Acquired, AcquireError> {
        let candidates: Vec<&Arc<Backend>> = self
            .backends
            .iter()
            .filter(|b| b.is_available() && b.serves_model(model))
            .collect();
        if candidates.is_empty() {
            return Err(AcquireError::NoHealthyBackend {
                pool: self.name.clone(),
            });
        }

        let hint = affinity.filter(|h| !h.is_empty());
        let ordered = match (self.strategy, hint) {
            (PickerStrategy::RoundRobin, _) => self.pick_round_robin(&candidates),
            (PickerStrategy::PrefixAffinity, Some(hint)) => {
                self.pick_prefix_affinity(&candidates, hint)
            }
            // Nothing to be affine to (embeddings, OCR, a body with no prompt):
            // fall back to load, which is the honest answer — pinning such
            // traffic to one backend would be affinity in name only.
            (PickerStrategy::PrefixAffinity, None) | (PickerStrategy::LeastInflight, _) => {
                self.pick_least_inflight(&candidates)
            }
        };

        for backend in ordered {
            if try_acquire_slot(backend) {
                // The real model id to forward: `model` itself for a real id,
                // or the alias's target on *this* backend. Candidates were
                // filtered by `serves_model`, so `resolve` is normally `Some`;
                // fall back to the requested string on a probe-update race.
                let resolved_model = backend.resolve(model).unwrap_or_else(|| model.to_string());
                // Remember where this prefix went, so the conversation's next
                // turn can find it. Recorded on dispatch rather than on success
                // because that is what the index claims to know: what was sent.
                if let Some(hint) = hint
                    && !hint.blocks.is_empty()
                {
                    self.prefix_index.record(&hint.blocks, &backend.name);
                }
                return Ok(Acquired {
                    backend: Arc::clone(backend),
                    resolved_model,
                });
            }
        }
        Err(AcquireError::Saturated {
            pool: self.name.clone(),
        })
    }

    /// Round-robin, **weighted**: a backend with `weight = 3` appears three
    /// times in the rotation, so it takes three turns for every one its
    /// `weight = 1` neighbour takes.
    ///
    /// `weight` was a dead field until this — stored, rendered in the admin
    /// editor, read by nothing. An operator running unequal hardware (one GPU
    /// twice the size of the other) turned the dial and got exactly nothing.
    /// A control that does nothing is worse than no control, so it now does what
    /// it says.
    fn pick_round_robin<'a>(&self, healthy: &[&'a Arc<Backend>]) -> Vec<&'a Arc<Backend>> {
        // The rotation slot list: each backend repeated `weight` times. Ordered
        // by backend so the expansion is deterministic, and cheap — pools are a
        // handful of entries and weights are small integers.
        let slots: Vec<&'a Arc<Backend>> = healthy
            .iter()
            .flat_map(|b| std::iter::repeat_n(*b, b.weight.max(1) as usize))
            .collect();
        let start = self.rr_cursor.fetch_add(1, Ordering::Relaxed) % slots.len();
        // Walk the rotation from `start`, keeping each backend's *first*
        // appearance: the caller falls through this list on saturation, so it
        // must name every candidate exactly once, in weighted-rotation order.
        let mut out: Vec<&'a Arc<Backend>> = Vec::with_capacity(healthy.len());
        for i in 0..slots.len() {
            let b = slots[(start + i) % slots.len()];
            if !out.iter().any(|seen| Arc::ptr_eq(seen, b)) {
                out.push(b);
            }
        }
        out
    }

    /// Least-loaded first, **ties broken round-robin**.
    ///
    /// The tie-break is not a refinement, it is the difference between using the
    /// fleet and using one machine of it. `inflight` counts requests in flight
    /// *right now*, so a client that waits for each answer before sending the
    /// next one finds every backend at zero every single time. A plain sort is
    /// stable, so every one of those ties resolved to the same backend and one
    /// GPU ran at 100 % while its twin idled at 15 W — visible in production,
    /// with no error anywhere, because the requests all succeeded.
    ///
    /// Rotating first and then stable-sorting gets both properties from one
    /// pass: the sort keeps the rotated order inside each group of equal load,
    /// so genuinely-idle backends alternate while a genuinely-busier one still
    /// sorts behind an idle one. (`pick_round_robin` advances the shared cursor,
    /// which is what makes consecutive ties land differently.)
    /// Order the candidates by cache locality, then let load override it when
    /// the imbalance is real.
    ///
    /// Locality comes from whichever mechanism the request supplied:
    ///
    ///   - an **exact key** (the client's `x-gateway-affinity`, or the chat UI's
    ///     session id) ranks by weighted rendezvous hash. Rendezvous rather than
    ///     a modulo or a ring position because draining one replica must move
    ///     only *its* share — anything else reshuffles the pool and cold-starts
    ///     every conversation at once.
    ///   - otherwise the **prefix chain** ranks by how many leading blocks each
    ///     replica was recently sent, longest match first. This is what lets a
    ///     new session start on the replica that already holds the shared system
    ///     prompt, and what lets a compacted conversation keep the part of its
    ///     history that still matches.
    ///
    /// Then the **load valve**, which is what keeps this from being a worse
    /// round-robin under pressure. Both of SGLang's tests must fire before load
    /// wins: an absolute in-flight difference *and* a relative ratio. The
    /// absolute test alone spills on noise once every replica is busy; the
    /// relative test alone spills far too eagerly when the pool is nearly idle
    /// (0 vs 1 in flight is a ratio of infinity and means nothing).
    fn pick_prefix_affinity<'a>(
        &self,
        candidates: &[&'a Arc<Backend>],
        hint: &super::affinity::AffinityHint,
    ) -> Vec<&'a Arc<Backend>> {
        let mut ordered: Vec<&'a Arc<Backend>> = candidates.to_vec();

        if let Some(key) = hint.key {
            // Highest rendezvous score first. `total_cmp` gives a total order
            // over floats; ties (astronomically unlikely) break on name so the
            // result stays deterministic.
            ordered.sort_by(|a, b| {
                let sa = super::affinity::score(key, &a.name, a.weight);
                let sb = super::affinity::score(key, &b.name, b.weight);
                sb.total_cmp(&sa).then_with(|| a.name.cmp(&b.name))
            });
        } else {
            let total = hint.blocks.len();
            let matched: HashMap<&str, usize> = ordered
                .iter()
                .map(|b| {
                    (
                        b.name.as_str(),
                        self.prefix_index.match_len(&hint.blocks, &b.name),
                    )
                })
                .collect();
            let best = matched.values().copied().max().unwrap_or(0);
            // Too little of the prompt is cached anywhere for locality to be
            // worth biasing on — spread instead, which also seeds the index
            // more evenly for the conversations that follow.
            if total == 0 || (best as f64 / total as f64) < PREFIX_MATCH_THRESHOLD {
                // Deliberately DEBUG, and deliberately present: "which replica
                // did my session land on, and why" is otherwise unanswerable
                // from outside the process, and it is the first question anyone
                // asks when a GPU looks idle.
                tracing::debug!(
                    blocks = total,
                    best,
                    "prefix-affinity: too little of this prompt is cached anywhere — balancing"
                );
                return self.pick_least_inflight(candidates);
            }
            // Rank on the **conversation-specific** part of the match, not the
            // total.
            //
            // This is the difference between a working router and one that
            // funnels a whole fleet onto a single GPU. Every session of one
            // client sends the same enormous system prompt, so once any replica
            // holds those blocks it wins the longest-match test for *every* new
            // conversation. Measured against two live replicas: eight
            // independent sessions, eight times the same replica, its twin
            // completely idle. The shared part of a prompt says nothing about
            // where a conversation belongs precisely *because* it is shared.
            //
            // `discriminating_match_len` strips it, by asking which matched
            // blocks several different conversations have extended differently.
            // A brand-new conversation therefore scores 0 on every replica —
            // however much boilerplate they hold — and falls through to load,
            // which spreads it; its second turn scores above 0 on exactly the
            // replica that served the first.
            //
            // Total match stays as the tie-break, so a replica that already
            // holds the shared preamble still beats a cold one for a brand-new
            // conversation — the cross-session warm start, kept without letting
            // it dominate.
            let exclusive = |name: &str| {
                self.prefix_index
                    .discriminating_match_len(&hint.blocks, name)
            };
            if ordered.iter().all(|b| exclusive(&b.name) == 0) {
                tracing::debug!(
                    blocks = total,
                    best,
                    "prefix-affinity: the match is shared boilerplate, not this \
                     conversation — balancing"
                );
                // Nothing distinguishes the candidates by cache: every one of
                // them holds exactly the same prefix. Balance, and let the
                // index learn where this conversation went.
                return self.pick_least_inflight(candidates);
            }
            tracing::debug!(
                blocks = total,
                best,
                scores = ?ordered
                    .iter()
                    .map(|b| (b.name.clone(), matched.get(b.name.as_str()).copied().unwrap_or(0), exclusive(&b.name)))
                    .collect::<Vec<_>>(),
                "prefix-affinity scoring"
            );
            ordered.sort_by(|a, b| {
                exclusive(&b.name)
                    .cmp(&exclusive(&a.name))
                    .then_with(|| {
                        matched
                            .get(b.name.as_str())
                            .cmp(&matched.get(a.name.as_str()))
                    })
                    .then_with(|| a.name.cmp(&b.name))
            });
        }

        // The load valve. A stable sort by "is this backend overloaded relative
        // to the least-loaded one" keeps the locality order intact among
        // backends that are equally busy.
        let min_load = ordered
            .iter()
            .map(|b| Self::normalised_load(b))
            .min()
            .unwrap_or(0);
        ordered.sort_by_key(|b| u8::from(Self::is_overloaded(b, min_load)));
        ordered
    }

    /// In-flight requests per unit of `weight` — load measured against
    /// capacity, so `weight` means the same thing here as in the rotation.
    fn normalised_load(b: &Backend) -> u32 {
        b.inflight() / b.weight.max(1)
    }

    /// Whether this backend is busy enough that throughput should outrank its
    /// warm cache. See [`Self::pick_prefix_affinity`].
    ///
    /// Two independent signals, either of which can fire:
    ///
    ///   - **Concurrent load**, gated on the absolute *and* relative tests
    ///     together. The absolute one alone spills on noise once every replica
    ///     is busy; the relative one alone spills far too eagerly when the pool
    ///     is nearly idle, where 0 vs 1 in flight is a ratio of infinity.
    ///
    /// Cumulative share is deliberately *not* consulted here. It is a fairness
    /// signal for **new** work, and new work does not reach this point — a
    /// conversation with no discriminating match falls through to
    /// [`Self::pick_least_inflight`], which weighs it. Applying it to an
    /// established conversation would move a lone session off its warm replica
    /// to a replica that then still idles: a cold prefill bought for nothing.
    fn is_overloaded(b: &Backend, min_load: u32) -> bool {
        let load = Self::normalised_load(b);
        let over_abs = load.saturating_sub(min_load) >= BALANCE_ABS_THRESHOLD;
        let over_rel = f64::from(load) > f64::from(min_load.max(1)) * BALANCE_REL_THRESHOLD;
        over_abs && over_rel
    }

    fn pick_least_inflight<'a>(&self, healthy: &[&'a Arc<Backend>]) -> Vec<&'a Arc<Backend>> {
        // Rotate first so that backends tied on *both* measures below still
        // alternate rather than always resolving to the same one.
        let mut sorted: Vec<&'a Arc<Backend>> = self.pick_round_robin(healthy);
        // Two keys, both normalised by `weight` so it means the same thing here
        // as in the rotation:
        //
        //   1. in-flight — who is busy *now*;
        //   2. total dispatched — who has been given more work overall.
        //
        // The second is what makes this work for sequential traffic. A client
        // that waits for each answer leaves every replica at zero in flight, so
        // key 1 ties on every single decision and key 1 alone would spread work
        // only as well as the rotation happens to. Measured: eight independent
        // conversations, all eight to one replica, its twin idle.
        //
        // Scaled integers rather than floats so the keys stay exactly `Ord`.
        sorted.sort_by_key(|b| {
            let w = u64::from(b.weight.max(1));
            (
                (u64::from(b.inflight()) * 1000) / w,
                (b.dispatched() * 1000) / w,
            )
        });
        sorted
    }
}

fn try_acquire_slot(backend: &Backend) -> bool {
    let max = backend.max_inflight;
    let mut current = backend.inflight.load(Ordering::Relaxed);
    loop {
        if current >= max {
            return false;
        }
        match backend.inflight.compare_exchange(
            current,
            current + 1,
            Ordering::AcqRel,
            Ordering::Relaxed,
        ) {
            Ok(_) => {
                backend.dispatched.fetch_add(1, Ordering::Relaxed);
                return true;
            }
            Err(observed) => current = observed,
        }
    }
}

/// RAII guard: while held, the backend has one slot reserved for this caller.
/// Dropping releases it. Cheap to clone — we move it through the proxy
/// pipeline so the slot is held for the full streaming response.
pub struct Acquired {
    backend: Arc<Backend>,
    /// The real model id the request resolved to on this backend — the id to
    /// write into the forwarded body's `model` field. Equal to the requested
    /// model for a direct hit; the alias's target when routed via an alias.
    resolved_model: String,
}

impl std::fmt::Debug for Acquired {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Acquired({}, model={})",
            self.backend.name, self.resolved_model
        )
    }
}

impl Acquired {
    pub fn backend(&self) -> &Backend {
        &self.backend
    }

    /// The real model id to forward upstream (see the field docs). Callers
    /// rewrite the request body's `model` to this and key usage/defaults on it.
    pub fn resolved_model(&self) -> &str {
        &self.resolved_model
    }
}

impl Drop for Acquired {
    fn drop(&mut self) {
        self.backend.inflight.fetch_sub(1, Ordering::Release);
    }
}

/// Top-level pool registry. Routes are computed on demand from each
/// backend's advertised-model set; no compiled route table.
///
/// The pool/fallback data lives behind an [`ArcSwap`] so the admin UI can
/// hot-swap the entire topology via [`Self::reload`] without restarting.
/// Every method call is a single lock-free atomic load.
pub struct UpstreamRegistry {
    inner: ArcSwap<RegistryData>,
    /// Bumped on every [`Self::reload`]. Each health-probe loop is tagged with
    /// the generation it was spawned for and retires itself once this moves past
    /// it, so a reload doesn't leak the previous generation's infinite probe
    /// loops (which hold detached `Backend` Arcs and would keep hammering the old
    /// URLs forever). See `health::spawn` / `health::run_probe`.
    generation: AtomicU64,
}

struct RegistryData {
    pools: HashMap<String, Arc<Pool>>,
    /// Unknown-model fallback per kind (`[fallback]`). Applied by `route` when
    /// a requested name is neither a real id nor an alias.
    fallback: FallbackConfig,
}

impl std::fmt::Debug for UpstreamRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UpstreamRegistry")
            .field("pools", &self.data().pools.keys().collect::<Vec<_>>())
            .finish()
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum BuildError {
    #[error("duplicate pool name `{0}`")]
    DuplicatePool(String),
    #[error(
        "alias `{alias}` on backend `{backend}` in pool `{pool}` collides with a real model id declared in config — an alias must not shadow a model name"
    )]
    AliasCollidesWithModel {
        alias: String,
        pool: String,
        backend: String,
    },
    #[error(
        "alias `{alias}` on backend `{backend}` in pool `{pool}` targets `{target}`, which isn't in that backend's configured `models` {declared:?} — fix the target or add it to `models`"
    )]
    AliasTargetUnknown {
        alias: String,
        target: String,
        pool: String,
        backend: String,
        declared: Vec<String>,
    },
}

/// Construct the internal data from config structs. Shared by the
/// constructors and [`UpstreamRegistry::reload`].
fn build_data(
    pool_configs: &HashMap<String, UpstreamPoolConfig>,
    fallback: FallbackConfig,
) -> Result<RegistryData, BuildError> {
    validate_aliases(pool_configs)?;
    let mut pools: HashMap<String, Arc<Pool>> = HashMap::new();
    for (name, cfg) in pool_configs {
        if pools.contains_key(name) {
            return Err(BuildError::DuplicatePool(name.clone()));
        }
        pools.insert(name.clone(), Arc::new(Pool::new(name.clone(), cfg)));
    }
    Ok(RegistryData { pools, fallback })
}

/// How the backends serving one model want a request phrased. See
/// [`UpstreamRegistry::serving_profile`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServingProfile {
    /// The reasoning spelling the *server* dictates, if it dictates one and
    /// every candidate agrees. `None` means the model name decides, which is
    /// right for every server that passes model parameters through untouched.
    pub dialect: Option<crate::server::reasoning::ReasoningStyle>,
    /// Whether `tool_choice: "none"` can be trusted to end a tool loop here.
    /// `false` means the caller must withhold the tool definitions instead.
    pub honors_tool_choice: bool,
}

impl Default for ServingProfile {
    /// What an unknown or unrouted model gets: the pre-profile behaviour.
    fn default() -> Self {
        Self {
            dialect: None,
            honors_tool_choice: true,
        }
    }
}

impl UpstreamRegistry {
    /// Build with no unknown-model fallback. Used by tests and any caller that
    /// doesn't route `[fallback]` (RAG embeddings, etc.).
    pub fn new(
        pool_configs: &HashMap<String, UpstreamPoolConfig>,
    ) -> Result<Arc<Self>, BuildError> {
        Self::build(pool_configs, FallbackConfig::default())
    }

    /// Build with the `[fallback]` map wired in, so `route` can substitute an
    /// unknown requested model with a configured per-kind default.
    pub fn with_fallback(
        pool_configs: &HashMap<String, UpstreamPoolConfig>,
        fallback: FallbackConfig,
    ) -> Result<Arc<Self>, BuildError> {
        Self::build(pool_configs, fallback)
    }

    /// Build from a DB topology snapshot (loaded by
    /// [`crate::server::db::upstreams_config::load_snapshot`]). Converts the
    /// snapshot into the same config structs that TOML parsing produces, then
    /// delegates to [`build`]. Used on startup and on every "Apply changes"
    /// reload from the admin UI.
    pub fn from_snapshot(
        snap: &crate::server::db::upstreams_config::UpstreamConfigSnapshot,
        crypto: &crate::server::crypto::Crypto,
    ) -> Result<Arc<Self>, BuildError> {
        let (pool_configs, fallback) =
            crate::server::upstreams::db_bridge::snapshot_to_configs(snap, crypto);
        Self::build(&pool_configs, fallback)
    }

    fn build(
        pool_configs: &HashMap<String, UpstreamPoolConfig>,
        fallback: FallbackConfig,
    ) -> Result<Arc<Self>, BuildError> {
        let data = build_data(pool_configs, fallback)?;
        Ok(Arc::new(Self {
            inner: ArcSwap::new(Arc::new(data)),
            generation: AtomicU64::new(0),
        }))
    }

    /// The current topology generation — bumped on every [`Self::reload`]. A
    /// health-probe loop reads this to know when it has been superseded.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }

    /// Hot-swap the entire topology from a DB snapshot. Validates aliases
    /// before swapping; on success the old pools are replaced atomically and the
    /// generation is bumped so stale probe loops retire. Fresh probes must be
    /// re-spawned after reload (see `health::spawn`).
    ///
    /// To avoid a routing gap during the swap, the *live* probed model set of
    /// each unchanged backend (same name + base_url) is carried over onto its
    /// freshly-built replacement. Without this, the new backends start with an
    /// empty live set and every request 404s until the first re-probe completes.
    pub fn reload(
        &self,
        snap: &crate::server::db::upstreams_config::UpstreamConfigSnapshot,
        crypto: &crate::server::crypto::Crypto,
    ) -> Result<(), BuildError> {
        let (pool_configs, fallback) =
            crate::server::upstreams::db_bridge::snapshot_to_configs(snap, crypto);
        let data = build_data(&pool_configs, fallback)?;

        // Snapshot the outgoing live model sets, keyed by identity (name +
        // base_url). Only carry over when both match — an edited base_url points
        // at a possibly-different upstream, so its set must be re-probed.
        // `old` (an `Arc<RegistryData>`) is held for this whole block, so the
        // map can borrow the outgoing backends' names/urls as keys — no clones
        // on either insert or lookup.
        //
        // The detected profile rides along on the same identity test and for a
        // sharper reason: re-detection happens *after* this swap, so between
        // the two every backend would otherwise read back as `Generic` — and a
        // request served in that window would spell its effort control wrong
        // and trust `tool_choice` on a server that ignores it. An edited
        // base_url drops both, which is correct: it may be a different server.
        let old = self.data();
        let mut prior: HashMap<(&str, &str), (HashSet<String>, Detected)> = HashMap::new();
        for pool in old.pools.values() {
            for b in &pool.backends {
                let live = b.live_models();
                let detected = b.detected();
                if !live.is_empty() || detected != Detected::default() {
                    prior.insert((b.name.as_str(), b.base_url.as_str()), (live, detected));
                }
            }
        }
        for pool in data.pools.values() {
            for b in &pool.backends {
                if let Some((live, detected)) = prior.get(&(b.name.as_str(), b.base_url.as_str())) {
                    if !live.is_empty() {
                        b.set_models(live.clone());
                    }
                    b.set_detected(detected);
                }
            }
        }
        // The maintenance switch is live state, but it is also persisted, and
        // the freshly-built backends already carry the DB's value — so there is
        // nothing to carry over here. Left explicit because the temptation is to
        // copy it from the outgoing registry, which would resurrect a backend an
        // admin drained *and* saved between the two.

        self.inner.store(Arc::new(data));
        self.generation.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// A comparable summary of what the registry is *currently serving*, for
    /// diffing against the edited DB topology.
    ///
    /// Deliberately a plain data snapshot rather than a diff method: the DB side
    /// lives in `db::upstreams_config` and the web layer owns the wording, so
    /// the registry's job is only to say what it has.
    pub fn live_topology(&self) -> LiveTopology {
        let d = self.data();
        let mut pools: Vec<LivePool> = d
            .pools
            .values()
            .map(|p| {
                let mut backends: Vec<LiveBackend> = p
                    .backends
                    .iter()
                    .map(|b| LiveBackend {
                        name: b.name.clone(),
                        base_url: b.base_url.clone(),
                        weight: b.weight,
                        max_inflight: b.max_inflight,
                        health_path: b.health_path.clone(),
                    })
                    .collect();
                backends.sort_by(|a, b| a.name.cmp(&b.name));
                LivePool {
                    name: p.name.clone(),
                    kind: p.kind,
                    strategy: p.strategy,
                    backends,
                }
            })
            .collect();
        pools.sort_by(|a, b| a.name.cmp(&b.name));
        LiveTopology { pools }
    }

    /// For one pool: every name it currently advertises to clients, each with
    /// how many of the pool's backends can actually serve it and how many exist.
    ///
    /// This is the number that makes a silent half-outage visible. A pool of two
    /// replicas where one backend's alias points at a model it does not serve
    /// keeps working — every request succeeds, on one GPU, while the other
    /// idles — and nothing anywhere says so. `1/2` does.
    ///
    /// Counts *availability*, so a drained backend reads as unavailable too:
    /// that is what the operator wants to see while draining. Sorted by name;
    /// empty for a pool the registry doesn't know.
    pub fn pool_model_coverage(&self, pool_name: &str) -> Vec<(String, usize, usize)> {
        let d = self.data();
        let Some(pool) = d.pools.get(pool_name) else {
            return Vec::new();
        };
        let total = pool.backends.len();
        let mut names: Vec<String> = pool
            .backends
            .iter()
            .flat_map(|b| b.listed_models())
            .collect();
        names.sort();
        names.dedup();
        names
            .into_iter()
            .map(|name| {
                let serving = pool
                    .backends
                    .iter()
                    .filter(|b| b.is_available() && b.serves_model(&name))
                    .count();
                (name, serving, total)
            })
            .collect()
    }

    /// Flip one backend's maintenance switch on the **running** registry, by
    /// name. Returns `false` if no backend by that name is registered.
    ///
    /// Deliberately outside the topology-reload flow: draining a box for
    /// maintenance has to take effect on the next request, not after an admin
    /// remembers to press "Apply changes". The DB write that makes it survive a
    /// restart is a separate, independent call.
    ///
    /// A backend can appear in several pools; every instance is flipped.
    pub fn set_backend_enabled(&self, backend_name: &str, on: bool) -> bool {
        let d = self.data();
        let mut found = false;
        for pool in d.pools.values() {
            for b in &pool.backends {
                if b.name == backend_name {
                    b.set_enabled(on);
                    found = true;
                }
            }
        }
        if found {
            tracing::info!(
                backend = %backend_name, enabled = on,
                "backend maintenance switch flipped"
            );
        }
        found
    }

    /// Load the current data snapshot. Lock-free read + one atomic Arc clone.
    fn data(&self) -> Arc<RegistryData> {
        self.inner.load_full()
    }

    /// The context window one model actually has, as reported by whichever
    /// backends serve it. When several disagree the smallest wins: a request
    /// may land on any of them, so the budget has to fit the tightest.
    /// `None` when no backend reported a window for it.
    pub fn probed_context_window(&self, model: &str) -> Option<i64> {
        self.data()
            .pools
            .values()
            .flat_map(|p| p.backends.iter())
            .filter_map(|b| b.context_window(model))
            .min()
    }

    pub fn pools(&self) -> Vec<Arc<Pool>> {
        self.data().pools.values().cloned().collect()
    }

    /// What the backends that could serve `model` imply about how to phrase a
    /// request for it.
    ///
    /// Resolved before routing rather than after, because the request body is
    /// built and serialised before a backend is picked — and these two facts
    /// decide what goes *into* the body.
    ///
    /// The two halves combine differently on purpose when candidates disagree:
    ///
    ///   * **Dialect** needs unanimity. Sending Ollama's spelling to a vLLM
    ///     means sending a parameter that server never asked for, so a mixed
    ///     set falls back to `None` — "ask the model name", exactly what the
    ///     gateway did before profiles.
    ///   * **`tool_choice`** takes the cautious side. Withholding the tools on
    ///     the final round is correct on every server; trusting `tool_choice`
    ///     is correct on all but one. So one Ollama in the candidate set is
    ///     enough to stop trusting it.
    ///
    /// In practice a pool is one kind of server and the question does not
    /// arise; this decides what happens when it does.
    pub fn serving_profile(
        &self,
        model: &str,
        kind: PoolKind,
        access: &PoolAccess,
    ) -> ServingProfile {
        // One pass, one `profile()` read per candidate, no intermediate `Vec` —
        // this runs on every turn and once per model on two admin pages.
        let data = self.data();
        let mut agreed: Option<BackendProfile> = None;
        let mut unanimous = true;
        let mut honors_tool_choice = true;
        for backend in data
            .pools
            .values()
            .filter(|p| p.kind == kind && access.allows(p))
            .flat_map(|p| p.backends.iter())
            .filter(|b| b.serves_model(model))
        {
            let profile = backend.profile();
            honors_tool_choice &= profile.honors_tool_choice();
            match agreed {
                None => agreed = Some(profile),
                Some(seen) if seen != profile => unanimous = false,
                Some(_) => {}
            }
        }
        ServingProfile {
            dialect: unanimous
                .then_some(agreed)
                .flatten()
                .and_then(BackendProfile::reasoning_dialect),
            honors_tool_choice,
        }
    }

    /// Sorted, de-duplicated union of the effective model sets of every
    /// backend in the pools matching `pred`, **including resolvable aliases**
    /// (so `/v1/models` and the pickers advertise alias names too). Shared by
    /// `models_for_kind` and `all_models`.
    fn collect_models(&self, pred: impl Fn(&Pool) -> bool) -> Vec<String> {
        let d = self.data();
        let mut all: HashSet<String> = HashSet::new();
        for pool in d.pools.values().filter(|p| pred(p)) {
            for backend in &pool.backends {
                all.extend(backend.listed_models());
            }
        }
        let mut out: Vec<String> = all.into_iter().collect();
        out.sort();
        out
    }

    /// [`Self::collect_models`], then narrowed to what the calling token may
    /// use. Every access-scoped listing goes through here so a restricted
    /// token's `/v1/models` matches exactly what it can route to.
    fn collect_models_for(&self, access: &PoolAccess, pred: impl Fn(&Pool) -> bool) -> Vec<String> {
        let mut out = self.collect_models(pred);
        out.retain(|m| access.allows_model(m));
        out
    }

    /// Union of every advertised model name across all pools of the given
    /// kind. Used by the chat UI to populate the voice-model dropdown and
    /// by `/api/v0/transcription_models`.
    pub fn models_for_kind(&self, kind: PoolKind) -> Vec<String> {
        self.collect_models(|p| p.kind == kind)
    }

    /// Every listed model of `kind`, each paired with the real id it resolves to
    /// when the name is an **alias** (`None` = a real model that owns its own
    /// settings). Real ids win over an alias of the same spelling. Sorted by
    /// name. Backs the admin pricing/defaults page, which renders aliases
    /// read-only: an alias carries no price or defaults of its own — requests are
    /// configured and metered as the model it resolves to.
    pub fn models_with_alias_target(&self, kind: PoolKind) -> Vec<(String, Option<String>)> {
        let d = self.data();
        let mut real: HashSet<String> = HashSet::new();
        for pool in d.pools.values().filter(|p| p.kind == kind) {
            for backend in &pool.backends {
                real.extend(backend.models_snapshot());
            }
        }
        let mut out: HashMap<String, Option<String>> = HashMap::new();
        for name in &real {
            out.insert(name.clone(), None);
        }
        for pool in d.pools.values().filter(|p| p.kind == kind) {
            for backend in &pool.backends {
                for name in backend.listed_models() {
                    if out.contains_key(&name) {
                        continue; // already a real id, or an alias we've mapped
                    }
                    if let Some(target) = backend.resolve(&name) {
                        out.insert(name, Some(target));
                    }
                }
            }
        }
        let mut rows: Vec<(String, Option<String>)> = out.into_iter().collect();
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        rows
    }

    /// True when at least one `speech` pool is configured — the switch that
    /// makes voice mode (and `POST /api/v0/speech`) available in the UI.
    pub fn has_speech(&self) -> bool {
        self.data()
            .pools
            .values()
            .any(|p| p.kind == PoolKind::Speech)
    }

    /// Resolve the voice-mode TTS target for a spoken `language` (lowercase
    /// ISO-639-1): the default speech model (first advertised across speech
    /// pools) plus the voice from the speech pool's language→voice map (exact
    /// match, then the `""` default entry, then `None` = backend default voice).
    /// `None` when no speech pool advertises a model. Used by the session
    /// `POST /api/v0/speech`; the raw `/v1/audio/speech` proxy takes an explicit
    /// model/voice from the caller instead.
    pub fn speech_target(&self, language: &str) -> Option<(String, Option<String>)> {
        let d = self.data();
        let pool = d.pools.values().find(|p| p.kind == PoolKind::Speech)?;
        // Prefer the operator's declared model (`models = ["tts-1"]`): a cloud
        // provider's `/models` probe lists its whole catalogue, so the probe
        // union can't identify *the* TTS model. Fall back to the probe union
        // for self-hosted servers that correctly advertise only their voice.
        let model = pool
            .configured_models
            .first()
            .cloned()
            .or_else(|| self.models_for_kind(PoolKind::Speech).into_iter().next())?;
        let voice = pool
            .voices
            .get(language)
            .or_else(|| pool.voices.get(""))
            .cloned();
        Some((model, voice))
    }

    /// Every distinct voice advertised by the speech pools `access` permits,
    /// sorted. Backs the per-user voice picker and, on the speech path, the
    /// check that a stored preference is still on offer.
    ///
    /// The menu is exactly what the operator declared in the pool's
    /// language→voice map, so a user can neither pick a voice the deployment
    /// doesn't offer nor keep one after the operator removes it — the stored
    /// preference falls back to [`Self::speech_target`]'s voice instead of
    /// reaching the upstream as an unknown id.
    pub fn speech_voices_for(&self, access: &PoolAccess) -> Vec<String> {
        let d = self.data();
        let pools = || {
            d.pools
                .values()
                .filter(|p| p.kind == PoolKind::Speech && access.allows(p))
        };
        // The operator's explicit menu comes first and in their order — the
        // house voice belongs at the top, not wherever the alphabet puts it.
        let mut voices: Vec<String> = Vec::new();
        for p in pools() {
            for v in &p.offer_voices {
                if !voices.contains(v) {
                    voices.push(v.clone());
                }
            }
        }
        // Then whatever the language map resolves to, so a deployment that
        // never fills the menu still offers its configured voices rather than
        // nothing at all. Sorted, since a HashMap has no order to honour.
        let mut resolved: Vec<String> = pools()
            .flat_map(|p| p.voices.values().cloned())
            .filter(|v| !voices.contains(v))
            .collect();
        resolved.sort();
        resolved.dedup();
        voices.extend(resolved);
        voices
    }

    /// Internal capability pools — never listed as chat models.
    ///
    /// OCR takes a document and reranking scores (query, passage) pairs;
    /// neither answers a chat completion, so a client picking a model from
    /// `/v1/models` must never see them.
    fn is_internal_kind(kind: PoolKind) -> bool {
        matches!(kind, PoolKind::Ocr | PoolKind::Rerank)
    }

    /// Every advertised model across *all* pools and kinds, de-duplicated by
    /// id (replicas serving the same id collapse to one) and sorted. Backs
    /// the OpenAI-parity `GET /v1/models`, which lists every usable model
    /// regardless of capability — clients pick by id.
    pub fn all_models(&self) -> Vec<String> {
        self.collect_models(|p| !Self::is_internal_kind(p.kind))
    }

    /// Like [`Self::all_models`], but only over pools `access` permits — the
    /// per-user `GET /v1/models`. A model withheld here is also unroutable for
    /// the same caller (see [`Self::route_for`]), so the list can't be bypassed.
    pub fn all_models_for(&self, access: &PoolAccess) -> Vec<String> {
        self.collect_models_for(access, |p| {
            !Self::is_internal_kind(p.kind) && access.allows(p)
        })
    }

    /// Like [`Self::models_for_kind`], but only over pools `access` permits —
    /// the per-user chat / transcription / speech model dropdowns.
    pub fn models_for_kind_for(&self, kind: PoolKind, access: &PoolAccess) -> Vec<String> {
        self.collect_models_for(access, |p| p.kind == kind && access.allows(p))
    }

    /// True if a pool of *any* kind that `access` permits knows `model`. Backs
    /// the per-user `GET /v1/models/{id}`.
    pub fn knows_any_for(&self, model: &str, access: &PoolAccess) -> bool {
        access.allows_model(model)
            && self.data().pools.values().any(|p| {
                !Self::is_internal_kind(p.kind) && access.allows(p) && p.knows_model(model)
            })
    }

    /// Sorted list of `(model_id, merged_compliance)` for every model served
    /// by a pool of `kind`. When the same id is served by multiple pools the
    /// flags are merged **most-restrictively** (a flag is clear only if it's
    /// clear on *every* serving pool) — so a model that's GDPR-safe on one
    /// upstream but not another is treated as not-safe. Backs the chat-UI
    /// model dropdown labels and the per-conversation warning banner.
    pub fn models_with_compliance_for_kind(&self, kind: PoolKind) -> Vec<(String, Compliance)> {
        self.models_with_compliance_for_kind_for(kind, &PoolAccess::all())
    }

    /// Like [`Self::models_with_compliance_for_kind`], but only over pools
    /// `access` permits — the per-user chat model dropdown.
    pub fn models_with_compliance_for_kind_for(
        &self,
        kind: PoolKind,
        access: &PoolAccess,
    ) -> Vec<(String, Compliance)> {
        let d = self.data();
        let mut merged: HashMap<String, Compliance> = HashMap::new();
        for pool in d
            .pools
            .values()
            .filter(|p| p.kind == kind && access.allows(p))
        {
            for backend in &pool.backends {
                // Alias names inherit the pool's compliance flags, same as the
                // real ids — clients pick either, so both must carry the warning.
                for id in backend.listed_models() {
                    let entry = merged.entry(id).or_default();
                    // AND the flags: clear only where every serving pool is clear.
                    entry.gdpr &= pool.compliance.gdpr;
                    entry.nda &= pool.compliance.nda;
                }
            }
        }
        let mut out: Vec<(String, Compliance)> = merged.into_iter().collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// True if any pool of `kind` knows `model` (probe- or config-derived),
    /// regardless of backend health. Used to decide 404 (`model_not_found`)
    /// vs 503 before routing — see `acquire_for`.
    pub fn knows_model(&self, model: &str, kind: PoolKind) -> bool {
        self.data()
            .pools
            .values()
            .any(|p| p.kind == kind && p.knows_model(model))
    }

    /// True if any pool of *any* kind knows `model`. Backs `GET
    /// /v1/models/{id}`, which (like the list) is capability-agnostic.
    pub fn knows_any(&self, model: &str) -> bool {
        self.data().pools.values().any(|p| p.knows_model(model))
    }

    /// Whether calls to `model` (of `kind`) count toward rate limits / quotas.
    /// Limits apply if *any* pool of that kind that knows it has
    /// `enforce_limits = true` (so a model available on a paid cloud pool is
    /// always counted, even if also mirrored on a free self-hosted one).
    /// Exempt only when every serving pool sets `enforce_limits = false` — i.e.
    /// a purely self-hosted model. An unknown model defaults to enforced (fail
    /// toward counting). See `server::limits`.
    pub fn enforce_limits_for_model(&self, model: &str, kind: PoolKind) -> bool {
        let d = self.data();
        let mut known = false;
        let mut any_enforced = false;
        for pool in d
            .pools
            .values()
            .filter(|p| p.kind == kind && p.knows_model(model))
        {
            known = true;
            any_enforced |= pool.enforce_limits;
        }
        !known || any_enforced
    }

    /// Find a pool of the given kind whose backends advertise `model` and
    /// acquire a slot on one of those backends. If two pools of the same
    /// kind both advertise the model, the first one we iterate wins —
    /// `HashMap` iteration is unordered, so callers shouldn't depend on
    /// which one (real-world deployments keep one pool per kind).
    ///
    /// Error semantics distinguish two cases the OpenAI contract treats
    /// differently:
    ///   - no pool of this kind knows `model` at all → [`RouteError::Unknown
    ///     Model`] (the caller maps this to `404 model_not_found`);
    ///   - the model *is* known but no healthy backend can serve it right
    ///     now → [`AcquireError::NoHealthyBackend`] / `Saturated` (`503`).
    pub fn acquire_for(&self, model: &str, kind: PoolKind) -> Result<Acquired, RouteError> {
        self.acquire_for_access(model, kind, &PoolAccess::all())
    }

    /// Like [`Self::acquire_for`], but only considers pools `access` permits. A
    /// model served solely by pools the caller can't access is reported as
    /// [`RouteError::UnknownModel`] (→ `404`), identical to a model that
    /// doesn't exist — so a restricted model can't be probed for existence, and
    /// filtering the `/v1/models` list can't be bypassed by calling the id.
    pub fn acquire_for_access(
        &self,
        model: &str,
        kind: PoolKind,
        access: &PoolAccess,
    ) -> Result<Acquired, RouteError> {
        self.acquire_for_access_affine(model, kind, access, None)
    }

    /// [`Self::acquire_for_access`] carrying an optional prefix-affinity key
    /// (see [`crate::server::upstreams::affinity`]). The key only influences
    /// *which backend inside a pool* is chosen, never which pool or which model
    /// — so it cannot change what a request resolves to, only where it lands.
    pub fn acquire_for_access_affine(
        &self,
        model: &str,
        kind: PoolKind,
        access: &PoolAccess,
        affinity: Option<&super::affinity::AffinityHint>,
    ) -> Result<Acquired, RouteError> {
        let d = self.data();
        // The token's allowlist is checked before anything is resolved, so a
        // denied model can never reach a backend — and never silently lands
        // on the kind's fallback model either (`route_access` only falls back
        // on `UnknownModel`, and the retry runs through here again, so the
        // fallback must clear the allowlist on its own account).
        //
        // A model the caller could never have used either way is *not*
        // reported as denied: an id no accessible pool serves is a typo, and
        // it has to keep behaving exactly as it does for an unrestricted
        // caller — 404, with the kind's fallback still applying — rather than
        // becoming an allowlist error that suppresses the fallback.
        if !access.allows_model(model) {
            let known = d
                .pools
                .values()
                .any(|p| p.kind == kind && access.allows(p) && p.knows_model(model));
            return Err(if known {
                RouteError::ModelNotAllowed(model.to_string())
            } else {
                RouteError::UnknownModel(model.to_string())
            });
        }
        // First, a pool with a healthy backend that serves the model.
        if let Some(pool) = d
            .pools
            .values()
            .find(|p| p.kind == kind && access.allows(p) && p.serves_model(model))
        {
            return pool
                .acquire_for_model_affine(model, affinity)
                .map_err(RouteError::Acquire);
        }
        // No healthy serving backend. If the model is nonetheless known to a
        // pool of this kind the caller may access, it's a transient outage (all
        // replicas down) — surface 503, not 404.
        if let Some(pool) = d
            .pools
            .values()
            .find(|p| p.kind == kind && access.allows(p) && p.knows_model(model))
        {
            return Err(RouteError::Acquire(AcquireError::NoHealthyBackend {
                pool: pool.name.clone(),
            }));
        }
        Err(RouteError::UnknownModel(model.to_string()))
    }

    /// Resolve + acquire with the two fallbacks layered on top of
    /// [`acquire_for`] (§ Fallback models in `docs/upstreams.md`):
    ///   - **unknown model** (`UnknownModel`) → retry with `[fallback].<kind>`;
    ///   - **known but all replicas down** (`NoHealthyBackend`) → retry with
    ///     that pool's `fallback_offline`;
    ///   - **saturated** (all healthy backends at `max_inflight`) → *no*
    ///     fallback, return `503` (don't silently downgrade under load).
    ///
    /// Fallback is a **single hop**: the retry calls `acquire_for` (not
    /// `route`), so a fallback target can't itself trigger another fallback —
    /// if it's also unavailable, the *original* error is returned. This is the
    /// method the dispatch paths call; the returned [`Acquired::resolved_model`]
    /// is the real id to forward (after alias/fallback resolution).
    pub fn route(&self, model: &str, kind: PoolKind) -> Result<Acquired, RouteError> {
        self.route_access(model, kind, &PoolAccess::all())
    }

    /// Like [`Self::route`], but only over pools `access` permits — the fallback
    /// targets are gated the same way, so a fallback can never route a caller to
    /// a pool they're not allowed to use.
    pub fn route_access(
        &self,
        model: &str,
        kind: PoolKind,
        access: &PoolAccess,
    ) -> Result<Acquired, RouteError> {
        self.route_access_affine(model, kind, access, None)
    }

    /// [`Self::route_access`] carrying an optional prefix-affinity key. The
    /// fallback retries carry it too: a request that lands on a pool's offline
    /// backup should still stick to one replica of it.
    pub fn route_access_affine(
        &self,
        model: &str,
        kind: PoolKind,
        access: &PoolAccess,
        affinity: Option<&super::affinity::AffinityHint>,
    ) -> Result<Acquired, RouteError> {
        match self.acquire_for_access_affine(model, kind, access, affinity) {
            Ok(acquired) => Ok(acquired),
            Err(RouteError::UnknownModel(orig)) => {
                let fb = self.data().fallback.for_kind(kind).map(str::to_owned);
                match fb {
                    Some(fallback) => self
                        .acquire_for_access_affine(&fallback, kind, access, affinity)
                        .map_err(|_| RouteError::UnknownModel(orig)),
                    None => Err(RouteError::UnknownModel(orig)),
                }
            }
            Err(RouteError::Acquire(AcquireError::NoHealthyBackend { pool })) => {
                match self.pool_fallback_offline(&pool) {
                    Some(fallback) => self
                        .acquire_for_access_affine(&fallback, kind, access, affinity)
                        .map_err(|_| RouteError::Acquire(AcquireError::NoHealthyBackend { pool })),
                    None => Err(RouteError::Acquire(AcquireError::NoHealthyBackend { pool })),
                }
            }
            // Saturated (or any other) → surface as-is; no fallback under load.
            Err(other) => Err(other),
        }
    }

    /// The `fallback_offline` model configured on the named pool, if any.
    fn pool_fallback_offline(&self, pool_name: &str) -> Option<String> {
        self.data()
            .pools
            .get(pool_name)
            .and_then(|p| p.fallback_offline.clone())
    }

    /// The configured unknown-model fallback for `kind` (`[fallback]`), if any.
    /// Read-only admin view.
    pub fn fallback_model(&self, kind: PoolKind) -> Option<String> {
        self.data().fallback.for_kind(kind).map(str::to_owned)
    }

    /// The model id [`Self::route_access`] would forward, **without taking an
    /// in-flight slot**.
    ///
    /// For callers that need the resolved name before they dispatch — the
    /// Anthropic path rewrites the body's `model` and keys admin defaults on it
    /// long before the tool loop makes its own routing decision per round.
    /// Those callers used to call `route_access` and drop the guard, which took
    /// a slot the request never used and, worse, counted as a dispatch: every
    /// request then registered *two*, and with a dispatch-balanced picker the
    /// two acquisitions locked into strict alternation — the resolve call always
    /// picking one replica and the real dispatch always the other. Measured
    /// live as 24 consecutive requests to one GPU.
    ///
    /// Same decisions as `route_access`, including both fallbacks, so the error
    /// a caller sees here is the error it would have got: unknown model, model
    /// not allowed, or pool unavailable.
    pub fn resolve_route_access(
        &self,
        model: &str,
        kind: PoolKind,
        access: &PoolAccess,
    ) -> Result<String, RouteError> {
        match self.resolve_access_no_slot(model, kind, access) {
            Ok(id) => Ok(id),
            Err(RouteError::UnknownModel(orig)) => {
                let fb = self.data().fallback.for_kind(kind).map(str::to_owned);
                match fb {
                    Some(fallback) => self
                        .resolve_access_no_slot(&fallback, kind, access)
                        .map_err(|_| RouteError::UnknownModel(orig)),
                    None => Err(RouteError::UnknownModel(orig)),
                }
            }
            Err(RouteError::Acquire(AcquireError::NoHealthyBackend { pool })) => {
                match self.pool_fallback_offline(&pool) {
                    Some(fallback) => self
                        .resolve_access_no_slot(&fallback, kind, access)
                        .map_err(|_| RouteError::Acquire(AcquireError::NoHealthyBackend { pool })),
                    None => Err(RouteError::Acquire(AcquireError::NoHealthyBackend { pool })),
                }
            }
            Err(other) => Err(other),
        }
    }

    /// The pool-selection half of [`Self::acquire_for_access_affine`], resolving
    /// the model id without claiming capacity. Mirrors its access checks and its
    /// 404-vs-503 distinction exactly.
    fn resolve_access_no_slot(
        &self,
        model: &str,
        kind: PoolKind,
        access: &PoolAccess,
    ) -> Result<String, RouteError> {
        let d = self.data();
        if !access.allows_model(model) {
            let known = d
                .pools
                .values()
                .any(|p| p.kind == kind && access.allows(p) && p.knows_model(model));
            return Err(if known {
                RouteError::ModelNotAllowed(model.to_string())
            } else {
                RouteError::UnknownModel(model.to_string())
            });
        }
        if let Some(id) = d
            .pools
            .values()
            .filter(|p| p.kind == kind && access.allows(p))
            .find_map(|p| p.resolve_healthy(model))
        {
            return Ok(id);
        }
        if let Some(pool) = d
            .pools
            .values()
            .find(|p| p.kind == kind && access.allows(p) && p.knows_model(model))
        {
            return Err(RouteError::Acquire(AcquireError::NoHealthyBackend {
                pool: pool.name.clone(),
            }));
        }
        Err(RouteError::UnknownModel(model.to_string()))
    }

    /// Resolve a requested name to the real model id a healthy backend of
    /// `kind` would serve it as, **without acquiring a slot**. Alias-aware;
    /// `None` when no healthy backend of that kind currently serves it. For
    /// callers that must rewrite a request body's `model` before a slot is
    /// taken (the chat-UI driver serialises before acquiring). Does not apply
    /// fallback — that's `route`'s job at acquire time.
    pub fn resolve_model(&self, model: &str, kind: PoolKind) -> Option<String> {
        self.resolve_model_for(model, kind, &PoolAccess::all())
    }

    /// Like [`Self::resolve_model`], but only over pools `access` permits. Used
    /// by the chat-UI driver so it resolves a body's `model` only against pools
    /// the signed-in user may use.
    pub fn resolve_model_for(
        &self,
        model: &str,
        kind: PoolKind,
        access: &PoolAccess,
    ) -> Option<String> {
        self.data()
            .pools
            .values()
            .filter(|p| p.kind == kind && access.allows(p))
            .find_map(|p| p.resolve_healthy(model))
    }
}

/// Boot-time alias validation (§ Alias validation in `docs/upstreams.md`).
/// Only conflicts knowable from *config* are checked here — the runtime,
/// probe-discovered kind (a bare alias on a multi-model backend) is handled by
/// [`Backend::reevaluate_aliases`]. Two failures refuse startup:
///   - an alias name that shadows a real model id declared in config;
///   - a map-form target that isn't in that backend's configured `models`
///     (only checkable when the backend declares `models`; otherwise deferred
///     — the alias just won't resolve until the probe reports the target).
fn validate_aliases(pool_configs: &HashMap<String, UpstreamPoolConfig>) -> Result<(), BuildError> {
    // Every real model id config actually names. Probe-discovered ids aren't
    // known at build time, so a collision with one of those can't be caught
    // here — but a real id wins over an alias at resolve time regardless.
    let mut config_ids: HashSet<&str> = HashSet::new();
    for cfg in pool_configs.values() {
        config_ids.extend(cfg.models.iter().map(String::as_str));
        for b in &cfg.backend {
            config_ids.extend(b.models.iter().map(String::as_str));
        }
    }
    for (pool_name, cfg) in pool_configs {
        for b in &cfg.backend {
            let Some(spec) = &b.alias else { continue };
            // This backend's effective config models (backend wins over pool).
            let declared: &[String] = if b.models.is_empty() {
                &cfg.models
            } else {
                &b.models
            };
            for (alias, target) in spec.into_map() {
                if config_ids.contains(alias.as_str()) {
                    return Err(BuildError::AliasCollidesWithModel {
                        alias,
                        pool: pool_name.clone(),
                        backend: b.name.clone(),
                    });
                }
                if let Some(target) = target
                    && !declared.is_empty()
                    && !declared.contains(&target)
                {
                    return Err(BuildError::AliasTargetUnknown {
                        alias,
                        target,
                        pool: pool_name.clone(),
                        backend: b.name.clone(),
                        declared: declared.to_vec(),
                    });
                }
            }
        }
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum RouteError {
    #[error(
        "no upstream advertises model `{0}` — check that the model is loaded on a backend of the right kind"
    )]
    UnknownModel(String),
    #[error("the API token used for this request is not allowed to use model `{0}`")]
    ModelNotAllowed(String),
    #[error(transparent)]
    Acquire(AcquireError),
}

impl RouteError {
    /// The HTTP status this failure means, and the sentence to show for it.
    ///
    /// The *decision* — an unserved model is a `404`, a token-forbidden one a
    /// `403`, an unreachable pool a `503` — is gateway policy, not wire
    /// format. It lives here, next to the variants that produce it, so the
    /// OpenAI and Anthropic renderers can only disagree about the envelope
    /// they wrap it in, never about what the failure was.
    pub fn status_and_message(&self) -> (u16, String) {
        match self {
            // No backend serves this id at all — not a transient 5xx.
            Self::UnknownModel(m) => (
                404,
                format!("The model `{m}` does not exist or you do not have access to it."),
            ),
            // The model exists and the owner may use it; this token may not.
            // A 403 that names the model, rather than the 404 a group-withheld
            // model gets: the caller holds the credential whose allowlist did
            // this and can see that allowlist on /tokens, so spelling it out is
            // help, not disclosure.
            Self::ModelNotAllowed(m) => (
                403,
                format!(
                    "The API token used for this request is not allowed to use the model \
                     `{m}`. Its allowed models are listed on the /tokens page."
                ),
            ),
            Self::Acquire(AcquireError::NoHealthyBackend { pool }) => {
                (503, format!("no healthy backend in `{pool}`"))
            }
            Self::Acquire(AcquireError::Saturated { pool }) => {
                (503, format!("`{pool}` is saturated"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An exact-key hint, as the `x-gateway-affinity` header or the chat UI's
    /// session id produces.
    fn keyed(key: u64) -> super::super::affinity::AffinityHint {
        super::super::affinity::AffinityHint {
            key: Some(key),
            blocks: Vec::new(),
        }
    }

    /// The whole point of `prefix_affinity`: one conversation keeps landing on
    /// one replica, so its KV prefix stays warm, while *different*
    /// conversations still spread across the pool.
    #[test]
    fn prefix_affinity_pins_a_conversation_and_still_spreads_sessions() {
        let pools = HashMap::from([(
            "p".to_string(),
            pool_config(
                PoolKind::Chat,
                PickerStrategy::PrefixAffinity,
                vec![
                    backend_with_models("gpu0", &["m"]),
                    backend_with_models("gpu1", &["m"]),
                ],
            ),
        )]);
        let reg = UpstreamRegistry::new(&pools).unwrap();
        let pool = reg.pools().into_iter().next().unwrap();

        // Same key, many turns (each released before the next, as a session
        // does): always the same backend.
        let key = 0xdead_beefu64;
        let first = pool
            .acquire_for_model_affine("m", Some(&keyed(key)))
            .unwrap();
        let pinned = first.backend().name.clone();
        drop(first);
        for _ in 0..20 {
            let a = pool
                .acquire_for_model_affine("m", Some(&keyed(key)))
                .unwrap();
            assert_eq!(a.backend().name, pinned, "session left its warm replica");
        }

        // Many different keys: both backends get used, so two sessions are not
        // stuck on one GPU.
        let mut seen: HashSet<String> = HashSet::new();
        for k in 0..64u64 {
            let a = pool.acquire_for_model_affine("m", Some(&keyed(k))).unwrap();
            seen.insert(a.backend().name.clone());
        }
        assert_eq!(
            seen.len(),
            2,
            "affinity collapsed every session onto {seen:?}"
        );
    }

    /// Affinity must not become a worse round-robin under load: once a replica
    /// is a full load bucket busier than its peer, the request spills.
    #[test]
    fn prefix_affinity_spills_when_its_backend_is_genuinely_busier() {
        let pools = HashMap::from([(
            "p".to_string(),
            pool_config(
                PoolKind::Chat,
                PickerStrategy::PrefixAffinity,
                vec![
                    backend_with_models("gpu0", &["m"]),
                    backend_with_models("gpu1", &["m"]),
                ],
            ),
        )]);
        let reg = UpstreamRegistry::new(&pools).unwrap();
        let pool = reg.pools().into_iter().next().unwrap();

        let key = 7u64;
        let probe = pool
            .acquire_for_model_affine("m", Some(&keyed(key)))
            .unwrap();
        let pinned = probe.backend().name.clone();
        // Pile requests on the pinned backend, holding the guards so the
        // in-flight count actually rises, until it spills. Both thresholds have
        // to fire, so this needs an absolute gap *and* a 1.5x ratio.
        let mut held = vec![probe];
        let mut spilled_at = None;
        for _ in 0..32 {
            let a = pool
                .acquire_for_model_affine("m", Some(&keyed(key)))
                .unwrap();
            if a.backend().name != pinned {
                spilled_at = Some(held.len());
                break;
            }
            held.push(a);
        }
        assert!(
            spilled_at.is_some(),
            "affinity never gave way to load: {} requests all went to {pinned}",
            held.len()
        );
        // ...but not immediately: the first few concurrent requests of one
        // conversation must stay on its warm replica.
        assert!(
            spilled_at.unwrap() >= BALANCE_ABS_THRESHOLD as usize,
            "spilled after only {} requests — a couple of parallel tool calls \
             must not push a session off its cache",
            spilled_at.unwrap()
        );
        drop(held);
    }

    /// Prefix matching with **no explicit key at all** — the case that matters
    /// for a client the gateway cannot instrument.
    ///
    /// Turn 2 of a conversation must land where turn 1 did, purely because that
    /// replica was the one sent this prefix. And a *different* conversation
    /// sharing only the system prompt must still be free to go elsewhere, which
    /// is the thing a sticky per-conversation key cannot do.
    #[test]
    fn prefix_matching_pins_a_conversation_without_any_explicit_key() {
        use super::super::affinity::AffinityHint;
        use super::super::prefix_index::prefix_chain;

        let pools = HashMap::from([(
            "p".to_string(),
            pool_config(
                PoolKind::Chat,
                PickerStrategy::PrefixAffinity,
                vec![
                    backend_with_models("gpu0", &["m"]),
                    backend_with_models("gpu1", &["m"]),
                ],
            ),
        )]);
        let reg = UpstreamRegistry::new(&pools).unwrap();
        let pool = reg.pools().into_iter().next().unwrap();

        let system = "SYSTEM PREAMBLE ".repeat(64);
        let hint_of = |text: &str| AffinityHint {
            key: None,
            blocks: prefix_chain(text),
        };

        // Turn 1 of conversation A: nothing is known yet, so this is a
        // load-balanced choice — and it teaches the index.
        let t1 = hint_of(&format!("{system}user: fix the flaky test"));
        let first = pool.acquire_for_model_affine("m", Some(&t1)).unwrap();
        let pinned = first.backend().name.clone();
        drop(first);

        // Turn 2 and onwards extend that prefix, so they must go back.
        for turn in 2..8 {
            let text = format!(
                "{system}user: fix the flaky test{}",
                "assistant: working on it ".repeat(turn * 8)
            );
            let a = pool
                .acquire_for_model_affine("m", Some(&hint_of(&text)))
                .unwrap();
            assert_eq!(
                a.backend().name,
                pinned,
                "turn {turn} left the replica holding its prefix"
            );
        }

        // The index actually learned something, rather than the test passing by
        // accident on a coin flip.
        assert!(!pool.prefix_index().is_empty());
    }

    /// Under *uniformly* heavy load, affinity must hold. This is what the
    /// relative threshold buys: with both replicas deep in requests, a small
    /// absolute difference is noise, and spilling on it would abandon a warm
    /// cache for nothing.
    #[test]
    fn prefix_affinity_holds_when_both_replicas_are_equally_busy() {
        let pools = HashMap::from([(
            "p".to_string(),
            pool_config(
                PoolKind::Chat,
                PickerStrategy::PrefixAffinity,
                vec![
                    backend_with_models("gpu0", &["m"]),
                    backend_with_models("gpu1", &["m"]),
                ],
            ),
        )]);
        let reg = UpstreamRegistry::new(&pools).unwrap();
        let pool = reg.pools().into_iter().next().unwrap();

        // Load both replicas deeply but short of `max_inflight` (16 each here) —
        // a saturated pool is a different behaviour, tested elsewhere.
        let mut held = Vec::new();
        for k in 0..22u64 {
            held.push(pool.acquire_for_model_affine("m", Some(&keyed(k))).unwrap());
        }
        let (a, b) = (pool.backends[0].inflight(), pool.backends[1].inflight());
        assert!(a >= 8 && b >= 8, "precondition: both busy, got {a} and {b}");

        // A conversation asking again lands on its own replica, whichever that
        // is, rather than being bounced by a few requests of difference.
        let key = 99u64;
        let first = pool
            .acquire_for_model_affine("m", Some(&keyed(key)))
            .unwrap();
        let pinned = first.backend().name.clone();
        drop(first);
        let again = pool
            .acquire_for_model_affine("m", Some(&keyed(key)))
            .unwrap();
        assert_eq!(again.backend().name, pinned);
        drop(again);
        drop(held);
    }

    /// Draining one backend must move only the keys that belonged to it. A
    /// modulo-style mapping would reshuffle everything and wipe every warm cache
    /// in the pool at once — which is why the picker uses rendezvous hashing.
    #[test]
    fn draining_one_backend_does_not_reshuffle_the_others_keys() {
        let pools = HashMap::from([(
            "p".to_string(),
            pool_config(
                PoolKind::Chat,
                PickerStrategy::PrefixAffinity,
                vec![
                    backend_with_models("gpu0", &["m"]),
                    backend_with_models("gpu1", &["m"]),
                    backend_with_models("gpu2", &["m"]),
                ],
            ),
        )]);
        let reg = UpstreamRegistry::new(&pools).unwrap();
        let pool = reg.pools().into_iter().next().unwrap();

        let before: Vec<(u64, String)> = (0..300u64)
            .map(|k| {
                let a = pool.acquire_for_model_affine("m", Some(&keyed(k))).unwrap();
                (k, a.backend().name.clone())
            })
            .collect();

        reg.set_backend_enabled("gpu2", false);

        for (k, was) in &before {
            let a = pool
                .acquire_for_model_affine("m", Some(&keyed(*k)))
                .unwrap();
            if was != "gpu2" {
                assert_eq!(
                    &a.backend().name,
                    was,
                    "key {k} moved off {was} even though it was not drained"
                );
            } else {
                assert_ne!(a.backend().name, "gpu2");
            }
        }
    }

    /// A request with no key (embeddings, OCR, a body with no prompt) must not
    /// all pile onto one backend — the strategy falls back to load.
    #[test]
    fn prefix_affinity_without_a_key_balances_by_load() {
        let pools = HashMap::from([(
            "p".to_string(),
            pool_config(
                PoolKind::Chat,
                PickerStrategy::PrefixAffinity,
                vec![
                    backend_with_models("gpu0", &["m"]),
                    backend_with_models("gpu1", &["m"]),
                ],
            ),
        )]);
        let reg = UpstreamRegistry::new(&pools).unwrap();
        let pool = reg.pools().into_iter().next().unwrap();

        let mut seen: HashSet<String> = HashSet::new();
        for _ in 0..6 {
            let a = pool.acquire_for_model_affine("m", None).unwrap();
            seen.insert(a.backend().name.clone());
        }
        assert_eq!(seen.len(), 2, "keyless traffic pinned to {seen:?}");
    }

    /// The maintenance switch: drained backends take no traffic, siblings pick
    /// up the load, and — the part that matters for clients — the models stay
    /// *known*, so draining the last backend in a pool is a temporary outage
    /// (503) and never "no such model" (404).
    #[test]
    fn draining_a_backend_reroutes_without_making_its_models_unknown() {
        let pools = HashMap::from([(
            "p".to_string(),
            pool_config(
                PoolKind::Chat,
                PickerStrategy::LeastInflight,
                vec![
                    backend_with_models("gpu0", &["m"]),
                    backend_with_models("gpu1", &["m"]),
                ],
            ),
        )]);
        let reg = UpstreamRegistry::new(&pools).unwrap();

        assert!(reg.set_backend_enabled("gpu0", false), "backend not found");
        assert!(
            !reg.set_backend_enabled("nope", false),
            "unknown name reported found"
        );

        // Everything goes to the sibling now, however many requests we make.
        for _ in 0..6 {
            let a = reg.route("m", PoolKind::Chat).unwrap();
            assert_eq!(a.backend().name, "gpu1");
        }

        // Drain the sibling too: the model is still known, so this is an outage,
        // not a typo. A 404 here is what makes a client give up entirely.
        reg.set_backend_enabled("gpu1", false);
        let err = reg.route("m", PoolKind::Chat).unwrap_err();
        assert!(
            matches!(
                err,
                RouteError::Acquire(AcquireError::NoHealthyBackend { .. })
            ),
            "drained pool must report an outage, got {err:?}"
        );
        assert_eq!(err.status_and_message().0, 503);
        // And it stays listed, so clients don't see the model disappear.
        assert!(reg.all_models().contains(&"m".to_string()));

        // Switch one back on and it serves again immediately — no reload.
        reg.set_backend_enabled("gpu0", true);
        assert_eq!(
            reg.route("m", PoolKind::Chat).unwrap().backend().name,
            "gpu0"
        );
    }

    /// `weight` has to actually weigh something. It was stored and rendered in
    /// the editor while no picker read it, so an operator running one GPU twice
    /// the size of the other could turn the dial and change nothing.
    #[test]
    fn weight_shifts_the_share_of_traffic() {
        let mut heavy = backend_with_models("big", &["m"]);
        heavy.weight = 3;
        let light = backend_with_models("small", &["m"]);

        for strategy in [PickerStrategy::RoundRobin, PickerStrategy::LeastInflight] {
            let pools = HashMap::from([(
                "p".to_string(),
                pool_config(PoolKind::Chat, strategy, vec![heavy.clone(), light.clone()]),
            )]);
            let reg = UpstreamRegistry::new(&pools).unwrap();

            let mut big = 0;
            let mut small = 0;
            for _ in 0..40 {
                let a = reg.route("m", PoolKind::Chat).unwrap();
                match a.backend().name.as_str() {
                    "big" => big += 1,
                    _ => small += 1,
                }
            }
            // 3:1 by weight. Assert the direction and that both are used, not an
            // exact split — `least_inflight` also reacts to real load, and
            // pinning the ratio would make this a test of the arithmetic.
            assert!(small > 0, "{strategy:?} starved the light backend");
            assert!(
                big > small,
                "{strategy:?} ignored weight: big={big} small={small}"
            );
        }
    }

    /// Every candidate must still appear exactly once in the order the picker
    /// falls through on saturation — the weighted rotation repeats backends
    /// internally, and a duplicate would waste a slot check (or, with an
    /// exhausted pool, report saturation against the same backend twice).
    #[test]
    fn the_weighted_rotation_names_each_backend_once() {
        let mut heavy = backend_with_models("big", &["m"]);
        heavy.weight = 4;
        let pools = HashMap::from([(
            "p".to_string(),
            pool_config(
                PoolKind::Chat,
                PickerStrategy::RoundRobin,
                vec![heavy, backend_with_models("small", &["m"])],
            ),
        )]);
        let reg = UpstreamRegistry::new(&pools).unwrap();
        let pool = reg.pools().into_iter().next().unwrap();
        let candidates: Vec<&Arc<Backend>> = pool.backends.iter().collect();
        for _ in 0..8 {
            let order = pool.pick_round_robin(&candidates);
            assert_eq!(
                order.len(),
                2,
                "each backend exactly once, got {}",
                order.len()
            );
            assert_ne!(order[0].name, order[1].name);
        }
    }

    /// The state the production incident was actually in: a backend that
    /// advertises **zero** models. A bare alias then binds to nothing, so it
    /// resolves to `None` and drops out of the listing — but `disabled` stayed
    /// `false` (that flag only ever described the >1 "ambiguous" case), so the
    /// admin page drew it as a healthy blue chip on a backend badged "up".
    ///
    /// Zero models is the normal consequence of a probe that never returned
    /// data: an unset `api_key_env` makes `/models` answer 401, which counts as
    /// "reachable" and leaves the model set empty. One missing environment
    /// variable therefore produced a green backend, a blue alias, and a name no
    /// request could route to.
    #[test]
    fn a_bare_alias_on_a_backend_with_no_models_reports_itself_broken() {
        let mut b = backend("qwen-gpu0", 16);
        b.alias = Some(AliasSpec::Names(vec!["default".into()]));
        let be = Backend::new(&b, &[]);

        assert!(
            be.models_snapshot().is_empty(),
            "precondition: nothing advertised"
        );
        assert!(be.resolve("default").is_none(), "nothing to bind to");
        assert!(!be.listed_models().contains("default"));

        let status = be.alias_status();
        assert_eq!(status.len(), 1);
        assert!(
            !status[0].resolves,
            "the admin view must be able to see that this alias routes nowhere"
        );

        // One model shows up → the same alias works again, untouched.
        be.set_models(HashSet::from(["unsloth/Qwen3.8-27B-NVFP4".to_string()]));
        assert_eq!(
            be.resolve("default").as_deref(),
            Some("unsloth/Qwen3.8-27B-NVFP4")
        );
        assert!(be.alias_status()[0].resolves);
    }

    /// Sequential traffic — one request at a time, which is what a single agent
    /// session looks like — must still spread across the pool.
    ///
    /// Every backend reads as "0 in flight" at the moment a lone request routes,
    /// so `least_inflight` is deciding between equals on every call. Without a
    /// tie-break it handed all of them to the same backend; this is the test that
    /// says it doesn't. Round-robin was never affected, which is exactly how the
    /// asymmetry showed up in production.
    #[test]
    fn least_inflight_spreads_sequential_traffic_across_the_pool() {
        for strategy in [PickerStrategy::LeastInflight, PickerStrategy::RoundRobin] {
            let pools = HashMap::from([(
                "p".to_string(),
                pool_config(
                    PoolKind::Chat,
                    strategy,
                    vec![
                        backend_with_models("gpu0", &["m"]),
                        backend_with_models("gpu1", &["m"]),
                    ],
                ),
            )]);
            let reg = UpstreamRegistry::new(&pools).unwrap();

            // Acquire *and release* each time: the next request sees an idle pool,
            // exactly like a client that waits for its answer before asking again.
            let mut seen: HashSet<String> = HashSet::new();
            for _ in 0..6 {
                let a = reg.route("m", PoolKind::Chat).expect("should route");
                seen.insert(a.backend().name.clone());
            }
            assert_eq!(
                seen.len(),
                2,
                "{strategy:?} sent every sequential request to {seen:?}"
            );
        }
    }

    /// The tie-break must not cost the actual load-awareness: a backend with
    /// requests in flight loses to an idle one however the cursor happens to sit.
    #[test]
    fn least_inflight_still_prefers_the_idle_backend() {
        let pools = HashMap::from([(
            "p".to_string(),
            pool_config(
                PoolKind::Chat,
                PickerStrategy::LeastInflight,
                vec![
                    backend_with_models("gpu0", &["m"]),
                    backend_with_models("gpu1", &["m"]),
                ],
            ),
        )]);
        let reg = UpstreamRegistry::new(&pools).unwrap();

        // Pin four requests on whichever backend the first call picks, then check
        // that the next two both go to the other one.
        let first = reg.route("m", PoolKind::Chat).unwrap();
        let busy = first.backend().name.clone();
        let mut held = vec![first];
        for _ in 0..3 {
            let a = reg.route("m", PoolKind::Chat).unwrap();
            if a.backend().name == busy {
                held.push(a);
            }
        }
        for _ in 0..2 {
            let a = reg.route("m", PoolKind::Chat).unwrap();
            assert_ne!(
                a.backend().name,
                busy,
                "the loaded backend should not win over an idle one"
            );
        }
        drop(held);
    }

    /// A broken alias doesn't just fail its own requests — it quietly takes a
    /// whole replica out of the load balancer.
    ///
    /// Observed in production as "one GPU is pinned at 100%, the other sits at
    /// 0% with plenty of requests in flight". The pool had two backends serving
    /// the same model; on one of them the alias clients actually ask for
    /// (`default`) pointed at a model id that backend doesn't serve, so
    /// `serves_model("default")` was false and the picker never considered it.
    /// Nothing reported an error: requests kept succeeding, on half the hardware.
    #[test]
    fn a_backend_whose_alias_is_broken_drops_out_of_the_balancer() {
        let served = "unsloth/Qwen3.8-27B-NVFP4";
        let mut good = backend("qwen-gpu1", 16);
        good.alias = Some(AliasSpec::Targets(HashMap::from([(
            "default".to_string(),
            served.to_string(),
        )])));
        let mut broken = backend("qwen-gpu0", 16);
        broken.alias = Some(AliasSpec::Targets(HashMap::from([(
            "default".to_string(),
            "qwen-32b".to_string(), // never loaded on this server
        )])));

        let pools = HashMap::from([(
            "qwen".to_string(),
            pool_config(
                PoolKind::Chat,
                PickerStrategy::LeastInflight,
                vec![good, broken],
            ),
        )]);
        let reg = UpstreamRegistry::new(&pools).unwrap();
        for pool in reg.pools() {
            for b in &pool.backends {
                b.set_models(HashSet::from([served.to_string()]));
            }
        }

        // Every request lands on the one backend whose alias resolves, however
        // loaded it already is — the other is invisible to the picker.
        let mut held = Vec::new();
        for _ in 0..8 {
            let a = reg.route("default", PoolKind::Chat).expect("should route");
            assert_eq!(a.backend().name, "qwen-gpu1");
            held.push(a);
        }
        drop(held);

        // Asking for the real id spreads across both, which is what proves the
        // imbalance is the alias and not the picker.
        let a = reg.route(served, PoolKind::Chat).unwrap();
        let b = reg.route(served, PoolKind::Chat).unwrap();
        assert_ne!(
            a.backend().name,
            b.backend().name,
            "least_inflight should have used the idle replica"
        );
    }

    /// Both ways an alias can silently stop resolving — the two shapes of the
    /// production incident where `default` and `qwen` disappeared from
    /// `/v1/models` and started answering 404.
    ///
    /// A **bare** alias binds to "the backend's sole model", so it dies the
    /// moment the effective set is not exactly one — including when a probe
    /// stops answering (a 401 from an unset `api_key_env` leaves the live set
    /// empty and falls back to the configured list).
    ///
    /// A **map** alias names its target, and resolves only while that target is
    /// actually served. Point one at a model id the server does not load — easy
    /// to do, since the id a vLLM reports is its full repo path, not the short
    /// name — and the alias is just as gone, with no error anywhere.
    #[test]
    fn an_alias_resolves_only_while_its_binding_holds() {
        let served = "unsloth/Qwen3.8-27B-NVFP4";

        // Bare alias + exactly one served model: resolves and is advertised.
        let mut b = backend("qwen-gpu0", 4);
        b.alias = Some(AliasSpec::Names(vec!["default".into(), "qwen".into()]));
        let bare = Backend::new(&b, &[]);
        bare.set_models(HashSet::from([served.to_string()]));
        assert_eq!(bare.resolve("default").as_deref(), Some(served));
        assert!(bare.listed_models().contains("default"));

        // Same backend once the probe reports a second model: "the sole model"
        // is undefined, so the bare aliases are disabled and drop out of the
        // listing rather than routing somewhere arbitrary.
        bare.set_models(HashSet::from([served.to_string(), "qwen-7b".to_string()]));
        assert!(bare.resolve("default").is_none());
        assert!(!bare.listed_models().contains("default"));

        // Map alias pointed at a model id this backend does not serve: also
        // unresolvable, also unlisted — the failure mode that looks like a
        // correctly configured alias.
        let mut m = backend("qwen-gpu1", 4);
        m.alias = Some(AliasSpec::Targets(HashMap::from([(
            "default".to_string(),
            "qwen-32b".to_string(),
        )])));
        let mapped = Backend::new(&m, &[]);
        mapped.set_models(HashSet::from([served.to_string()]));
        assert!(
            mapped.resolve("default").is_none(),
            "an alias whose target is not served must not resolve"
        );
        assert!(!mapped.listed_models().contains("default"));

        // Pointed at what the server actually reports, it resolves.
        let mut m2 = backend("qwen-gpu2", 4);
        m2.alias = Some(AliasSpec::Targets(HashMap::from([(
            "default".to_string(),
            served.to_string(),
        )])));
        let ok = Backend::new(&m2, &[]);
        ok.set_models(HashSet::from([served.to_string()]));
        assert_eq!(ok.resolve("default").as_deref(), Some(served));
    }

    use crate::server::upstreams::config::{
        AliasSpec, BackendConfig, PickerStrategy, PoolKind, UpstreamPoolConfig,
    };

    fn backend(name: &str, max_inflight: u32) -> BackendConfig {
        BackendConfig {
            name: name.into(),
            base_url: format!("http://{name}:8000/v1"),
            api_key_env: None,
            api_key: None,
            weight: 1,
            max_inflight,
            health_path: "/models".into(),
            models: Vec::new(),
            alias: None,
            probe_models: true,
            supports_edit: false,
            enabled: true,
        }
    }

    /// A registry serving `model` from backends carrying the given profiles.
    fn registry_with_profiles(model: &str, profiles: &[BackendProfile]) -> Arc<UpstreamRegistry> {
        let backends: Vec<BackendConfig> = profiles
            .iter()
            .enumerate()
            .map(|(i, _)| backend_with_models(&format!("b{i}"), &[model]))
            .collect();
        let pools = HashMap::from([(
            "p".to_string(),
            pool_config(PoolKind::Chat, PickerStrategy::RoundRobin, backends),
        )]);
        let registry = UpstreamRegistry::new(&pools).unwrap();
        for pool in registry.pools() {
            for (backend, profile) in pool.backends.iter().zip(profiles) {
                backend.set_detected(&Detected {
                    profile: *profile,
                    ..Detected::default()
                });
            }
        }
        registry
    }

    /// The whole point: a model served by Ollama gets Ollama's spelling of
    /// "think harder", regardless of what the model is called.
    #[test]
    fn serving_profile_reports_the_backends_dialect() {
        let registry = registry_with_profiles("qwen3:8b", &[BackendProfile::Ollama]);
        let serving = registry.serving_profile("qwen3:8b", PoolKind::Chat, &PoolAccess::all());
        assert_eq!(
            serving.dialect,
            Some(crate::server::reasoning::ReasoningStyle::Ollama)
        );
        assert!(!serving.honors_tool_choice);
    }

    /// Servers that pass model parameters through dictate nothing, so the
    /// model name keeps deciding — the pre-profile behaviour, preserved.
    #[test]
    fn serving_profile_defers_to_the_model_name_on_passthrough_servers() {
        for profile in [
            BackendProfile::VLlm,
            BackendProfile::LlamaCpp,
            BackendProfile::SgLang,
            BackendProfile::Generic,
        ] {
            let registry = registry_with_profiles("m", &[profile]);
            let serving = registry.serving_profile("m", PoolKind::Chat, &PoolAccess::all());
            assert_eq!(serving.dialect, None, "{profile:?}");
            assert!(serving.honors_tool_choice, "{profile:?}");
        }
    }

    /// Candidates that disagree: guessing a dialect would send a parameter one
    /// of them never asked for, so nothing is guessed. `tool_choice` goes the
    /// other way — withholding the tools is safe everywhere, so one backend
    /// that ignores the field is enough to stop relying on it.
    #[test]
    fn mixed_backends_drop_the_dialect_but_keep_the_cautious_tool_choice() {
        let registry = registry_with_profiles("m", &[BackendProfile::Ollama, BackendProfile::VLlm]);
        let serving = registry.serving_profile("m", PoolKind::Chat, &PoolAccess::all());
        assert_eq!(
            serving.dialect, None,
            "a mixed set must not guess a spelling"
        );
        assert!(
            !serving.honors_tool_choice,
            "one backend that ignores tool_choice is enough to stop trusting it"
        );
    }

    /// A model nothing serves resolves to the pre-profile defaults rather than
    /// to anything invented.
    #[test]
    fn an_unserved_model_gets_the_neutral_profile() {
        let registry = registry_with_profiles("m", &[BackendProfile::Ollama]);
        assert_eq!(
            registry.serving_profile("other", PoolKind::Chat, &PoolAccess::all()),
            ServingProfile::default()
        );
    }

    /// llama.cpp reports a model's *trained* context on `/models` and the far
    /// smaller figure it was actually started with on `/props`. Believing the
    /// trained number is how prompts end up past what the server will hold,
    /// which it truncates in silence.
    #[test]
    fn the_allocated_context_caps_the_reported_one() {
        let registry = registry_with_profiles("gemma", &[BackendProfile::LlamaCpp]);
        let pools = registry.pools();
        let backend = &pools[0].backends[0];
        backend.set_detected(&Detected {
            profile: BackendProfile::LlamaCpp,
            context_windows: HashMap::from([("gemma".to_string(), 131_072)]),
            context_cap: Some(8_192),
            ..Detected::default()
        });
        assert_eq!(backend.context_window("gemma"), Some(8_192));
        assert_eq!(registry.probed_context_window("gemma"), Some(8_192));
    }

    /// A cap with nothing probed still beats knowing nothing.
    #[test]
    fn the_allocated_context_stands_alone_when_models_reported_none() {
        let registry = registry_with_profiles("gemma", &[BackendProfile::LlamaCpp]);
        let pools = registry.pools();
        let backend = &pools[0].backends[0];
        backend.set_detected(&Detected {
            profile: BackendProfile::LlamaCpp,
            context_cap: Some(4_096),
            ..Detected::default()
        });
        assert_eq!(backend.context_window("gemma"), Some(4_096));
    }

    /// Re-detecting against a server that reports no windows (Ollama, hosted)
    /// must not erase what an earlier `/models` probe managed to read.
    #[test]
    fn detection_without_windows_leaves_probed_ones_alone() {
        let registry = registry_with_profiles("m", &[BackendProfile::VLlm]);
        let pools = registry.pools();
        let backend = &pools[0].backends[0];
        backend.set_context_windows(HashMap::from([("m".to_string(), 262_144)]));
        backend.set_detected(&Detected {
            profile: BackendProfile::Generic,
            ..Detected::default()
        });
        assert_eq!(backend.context_window("m"), Some(262_144));
    }

    /// A backend answers for the models it serves and for nothing else.
    ///
    /// `probed_context_window` takes the minimum across every backend in the
    /// deployment, so a backend that answered for a model it does not serve
    /// would impose its ceiling on the whole gateway: one llama.cpp started
    /// with `-c 4096` in an unrelated pool budgeting a 262k vLLM model at 4096.
    #[test]
    fn a_backend_has_nothing_to_say_about_a_model_it_does_not_serve() {
        let registry = registry_with_profiles("mine", &[BackendProfile::LlamaCpp]);
        let pools = registry.pools();
        let backend = &pools[0].backends[0];
        backend.set_detected(&Detected {
            profile: BackendProfile::LlamaCpp,
            context_windows: HashMap::from([("stale".to_string(), 2_048)]),
            context_cap: Some(4_096),
            ..Detected::default()
        });

        // Serves it: the cap applies.
        assert_eq!(backend.context_window("mine"), Some(4_096));
        // Does not serve it: silence, cap and stale entry alike.
        assert_eq!(backend.context_window("someone-elses"), None);
        assert_eq!(backend.context_window("stale"), None);
        assert_eq!(registry.probed_context_window("someone-elses"), None);
    }

    /// Ollama's `/api/ps` lists only *loaded* models, so a detection round
    /// taken while one is idle says nothing about it. Replacing the map on
    /// every round would erase a window learned minutes earlier and drop that
    /// model back onto the global guess.
    #[test]
    fn a_partial_detection_round_keeps_windows_it_did_not_mention() {
        let registry = registry_with_profiles("loaded", &[BackendProfile::Ollama]);
        let pools = registry.pools();
        let backend = &pools[0].backends[0];

        backend.set_detected(&Detected {
            profile: BackendProfile::Ollama,
            context_windows: HashMap::from([("loaded".to_string(), 4_096)]),
            ..Detected::default()
        });
        assert_eq!(backend.context_window("loaded"), Some(4_096));

        // A later round with the model unloaded: same server, nothing to say.
        backend.set_detected(&Detected {
            profile: BackendProfile::Ollama,
            context_windows: HashMap::new(),
            ..Detected::default()
        });
        assert_eq!(
            backend.context_window("loaded"),
            Some(4_096),
            "an idle model must not lose the window we already learned"
        );
    }

    /// Backend with a static fallback model list (no probe needed to route).
    fn backend_with_models(name: &str, models: &[&str]) -> BackendConfig {
        BackendConfig {
            models: models.iter().map(|s| (*s).to_string()).collect(),
            ..backend(name, 16)
        }
    }

    fn pool_config(
        kind: PoolKind,
        strategy: PickerStrategy,
        backends: Vec<BackendConfig>,
    ) -> UpstreamPoolConfig {
        UpstreamPoolConfig {
            voices: Default::default(),
            offer_voices: Vec::new(),
            allowed_groups: Vec::new(),
            compliance: Default::default(),
            enforce_limits: true,
            kind,
            strategy,
            models: Vec::new(),
            fallback_offline: None,
            backend: backends,
        }
    }

    /// Pool carrying explicit compliance flags.
    fn pool_config_with_compliance(
        kind: PoolKind,
        compliance: Compliance,
        backends: Vec<BackendConfig>,
    ) -> UpstreamPoolConfig {
        UpstreamPoolConfig {
            voices: Default::default(),
            offer_voices: Vec::new(),
            allowed_groups: Vec::new(),
            compliance,
            kind,
            strategy: PickerStrategy::RoundRobin,
            models: Vec::new(),
            fallback_offline: None,
            enforce_limits: true,
            backend: backends,
        }
    }

    /// Pool with a pool-level fallback model list.
    fn pool_config_with_models(
        kind: PoolKind,
        models: &[&str],
        backends: Vec<BackendConfig>,
    ) -> UpstreamPoolConfig {
        UpstreamPoolConfig {
            voices: Default::default(),
            offer_voices: Vec::new(),
            allowed_groups: Vec::new(),
            compliance: Default::default(),
            enforce_limits: true,
            kind,
            strategy: PickerStrategy::RoundRobin,
            models: models.iter().map(|s| (*s).to_string()).collect(),
            fallback_offline: None,
            backend: backends,
        }
    }

    fn build(pools: Vec<(&str, UpstreamPoolConfig)>) -> Arc<UpstreamRegistry> {
        let map: HashMap<String, UpstreamPoolConfig> =
            pools.into_iter().map(|(k, v)| (k.into(), v)).collect();
        UpstreamRegistry::new(&map).unwrap()
    }

    /// Test helper — synthesise what a `/models` probe would have written
    /// for a single backend. Real code calls `Backend::set_models` from
    /// the health probe; tests use this to bypass the network entirely.
    fn seed_models(reg: &UpstreamRegistry, pool: &str, backend_idx: usize, models: &[&str]) {
        let d = reg.data();
        let pool = d.pools.get(pool).expect("pool exists");
        let set: HashSet<String> = models.iter().map(|s| (*s).to_string()).collect();
        pool.backends[backend_idx].set_models(set);
    }

    #[test]
    fn speech_target_prefers_configured_model_over_probe_flood() {
        // Regression: a cloud provider's /models probe reports its whole
        // catalogue. speech_target must return the operator's declared TTS
        // model, not the alphabetically-first of the flood.
        let cfg: UpstreamPoolConfig = toml::from_str(
            r#"
            kind = "speech"
            models = ["tts-1"]
            [voices]
            "" = "alloy"
            de = "nova"
            [[backend]]
            name = "openai"
            base_url = "https://api.openai.com/v1"
        "#,
        )
        .unwrap();
        let reg = build(vec![("openai_tts", cfg)]);
        // Simulate the OpenAI /models flood.
        seed_models(
            &reg,
            "openai_tts",
            0,
            &["babbage-002", "gpt-4o", "tts-1", "whisper-1"],
        );

        assert!(reg.has_speech());
        // Configured model wins over the flood; voice resolves by language.
        assert_eq!(
            reg.speech_target("de"),
            Some(("tts-1".to_string(), Some("nova".to_string())))
        );
        // Unknown language → the "" default voice.
        assert_eq!(
            reg.speech_target("fr"),
            Some(("tts-1".to_string(), Some("alloy".to_string())))
        );
    }

    #[test]
    fn speech_voices_are_the_operators_declared_set_deduped() {
        // Two speech pools, one voice shared between them and one restricted to
        // a group. The picker must offer each distinct voice once, and only
        // from pools the caller may route to.
        let open: UpstreamPoolConfig = toml::from_str(
            r#"
            kind = "speech"
            models = ["tts-1"]
            [voices]
            "" = "alloy"
            de = "onyx"
            [[backend]]
            name = "openai"
            base_url = "https://api.openai.com/v1"
        "#,
        )
        .unwrap();
        let restricted: UpstreamPoolConfig = toml::from_str(
            r#"
            kind = "speech"
            models = ["tts-1"]
            allowed_groups = ["studio"]
            [voices]
            "" = "onyx"
            en = "sage"
            [[backend]]
            name = "local"
            base_url = "http://tts.example.com"
        "#,
        )
        .unwrap();
        let reg = build(vec![("cloud", open), ("studio", restricted)]);

        // A member of no group sees only the unrestricted pool's voices.
        let plain = PoolAccess::default();
        assert_eq!(reg.speech_voices_for(&plain), vec!["alloy", "onyx"]);
        // With access to both, `onyx` still appears exactly once.
        assert_eq!(
            reg.speech_voices_for(&PoolAccess::all()),
            vec!["alloy", "onyx", "sage"]
        );
    }

    #[test]
    fn an_explicit_voice_menu_leads_and_the_language_map_follows() {
        // The reason `offer_voices` exists: three voices for ONE language,
        // which the (pool, lang) keyed map cannot hold. The menu keeps the
        // operator's order — the house voice belongs first, not wherever the
        // alphabet puts it — and the map's own voices fill in behind it.
        let cfg: UpstreamPoolConfig = toml::from_str(
            r#"
            kind = "speech"
            models = ["tts-1"]
            offer_voices = ["marin", "cedar", "alloy"]
            [voices]
            "" = "alloy"
            de = "onyx"
            [[backend]]
            name = "openai"
            base_url = "https://api.openai.com/v1"
        "#,
        )
        .unwrap();
        let reg = build(vec![("cloud", cfg)]);
        assert_eq!(
            reg.speech_voices_for(&PoolAccess::default()),
            vec!["marin", "cedar", "alloy", "onyx"]
        );
        // Resolution is untouched by the menu: `de` still gets its own voice.
        assert_eq!(
            reg.speech_target("de"),
            Some(("tts-1".to_string(), Some("onyx".to_string())))
        );
    }

    #[test]
    fn speech_target_none_without_speech_pool() {
        let reg = build(vec![(
            "chat",
            pool_config(
                PoolKind::Chat,
                PickerStrategy::RoundRobin,
                vec![backend("a", 16)],
            ),
        )]);
        assert!(!reg.has_speech());
        assert_eq!(reg.speech_target("en"), None);
    }

    #[test]
    fn acquire_for_routes_by_advertised_model() {
        let reg = build(vec![
            (
                "chat",
                pool_config(
                    PoolKind::Chat,
                    PickerStrategy::RoundRobin,
                    vec![backend("a", 16)],
                ),
            ),
            (
                "voice",
                pool_config(
                    PoolKind::Transcription,
                    PickerStrategy::RoundRobin,
                    vec![backend("b", 16)],
                ),
            ),
        ]);
        seed_models(&reg, "chat", 0, &["llama-3.1-70b", "llama-3.1-8b"]);
        seed_models(&reg, "voice", 0, &["whisper-1"]);

        let g = reg.acquire_for("llama-3.1-70b", PoolKind::Chat).unwrap();
        assert_eq!(g.backend().name, "a");
        let g = reg
            .acquire_for("whisper-1", PoolKind::Transcription)
            .unwrap();
        assert_eq!(g.backend().name, "b");
    }

    #[test]
    fn pool_allowed_groups_gate_listing_and_routing() {
        // Two chat pools: "open" (unrestricted) and "vip" (restricted to the
        // `dev` gateway group). A user without the group sees only the open
        // pool's model and can't route to the vip one; a member and an admin
        // both see everything.
        let mut vip = pool_config(
            PoolKind::Chat,
            PickerStrategy::RoundRobin,
            vec![backend("vip-b", 16)],
        );
        vip.allowed_groups = vec!["dev".into()];
        let reg = build(vec![
            (
                "open",
                pool_config(
                    PoolKind::Chat,
                    PickerStrategy::RoundRobin,
                    vec![backend("open-b", 16)],
                ),
            ),
            ("vip", vip),
        ]);
        seed_models(&reg, "open", 0, &["open-model"]);
        seed_models(&reg, "vip", 0, &["vip-model"]);

        let outsider = PoolAccess {
            role_ids: vec!["other".into()],
            is_admin: false,
            ..Default::default()
        };
        let member = PoolAccess {
            role_ids: vec!["dev".into()],
            is_admin: false,
            ..Default::default()
        };
        let admin = PoolAccess::all();

        // Listing: the restricted model is withheld from the outsider only.
        let outsider_models = reg.all_models_for(&outsider);
        assert!(outsider_models.contains(&"open-model".to_string()));
        assert!(!outsider_models.contains(&"vip-model".to_string()));
        assert!(
            reg.all_models_for(&member)
                .contains(&"vip-model".to_string())
        );
        assert!(
            reg.all_models_for(&admin)
                .contains(&"vip-model".to_string())
        );

        // knows_any_for mirrors the listing (backs GET /v1/models/{id}).
        assert!(!reg.knows_any_for("vip-model", &outsider));
        assert!(reg.knows_any_for("vip-model", &member));

        // Routing: an outsider can't reach the restricted model — it's reported
        // as UnknownModel (404), identical to a nonexistent one, so the listing
        // filter can't be bypassed by calling the id directly. The open model
        // still routes for them.
        assert!(matches!(
            reg.acquire_for_access("vip-model", PoolKind::Chat, &outsider),
            Err(RouteError::UnknownModel(_))
        ));
        assert!(
            reg.acquire_for_access("open-model", PoolKind::Chat, &outsider)
                .is_ok()
        );
        // A member (and an admin) can route to the restricted model.
        assert!(
            reg.acquire_for_access("vip-model", PoolKind::Chat, &member)
                .is_ok()
        );
        assert!(
            reg.acquire_for_access("vip-model", PoolKind::Chat, &admin)
                .is_ok()
        );
    }

    /// The allowlist has to bind at the same seam pool groups bind at:
    /// listing, single-model lookup, and routing all read the same set, so a
    /// denied model is invisible *and* unroutable rather than merely hidden.
    #[test]
    fn a_token_allowlist_narrows_listing_and_routing_together() {
        let reg = build(vec![(
            "chat",
            pool_config(
                PoolKind::Chat,
                PickerStrategy::RoundRobin,
                vec![backend("a", 16)],
            ),
        )]);
        seed_models(&reg, "chat", 0, &["allowed-model", "denied-model"]);

        let unrestricted = PoolAccess::all();
        let restricted = PoolAccess {
            allowed_models: Some(Arc::new(HashSet::from(["allowed-model".to_string()]))),
            ..PoolAccess::all()
        };

        // Listing: only the allowed id, even though the caller is an admin —
        // an allowlist is a property of the credential, not of the operator's
        // group policy, so `is_admin` must not waive it.
        assert_eq!(reg.all_models_for(&restricted), vec!["allowed-model"]);
        assert_eq!(
            reg.all_models_for(&unrestricted),
            vec!["allowed-model", "denied-model"]
        );

        // GET /v1/models/{id} mirrors the listing.
        assert!(reg.knows_any_for("allowed-model", &restricted));
        assert!(!reg.knows_any_for("denied-model", &restricted));
        assert!(reg.knows_any_for("denied-model", &unrestricted));

        // Routing: denied is refused as its own error, distinct from a model
        // that does not exist — the token's owner chose this restriction and
        // is entitled to a message that says so.
        assert!(matches!(
            reg.acquire_for_access("denied-model", PoolKind::Chat, &restricted),
            Err(RouteError::ModelNotAllowed(_))
        ));
        assert!(
            reg.acquire_for_access("allowed-model", PoolKind::Chat, &restricted)
                .is_ok()
        );
    }

    /// A token with no allowlist is unrestricted. This is the default every
    /// token in the field already has, so getting it wrong would revoke
    /// access from every existing integration at once.
    #[test]
    fn no_allowlist_means_every_model_stays_reachable() {
        let reg = build(vec![(
            "chat",
            pool_config(
                PoolKind::Chat,
                PickerStrategy::RoundRobin,
                vec![backend("a", 16)],
            ),
        )]);
        seed_models(&reg, "chat", 0, &["m1", "m2"]);

        let access = PoolAccess {
            allowed_models: None,
            ..Default::default()
        };
        assert_eq!(reg.all_models_for(&access), vec!["m1", "m2"]);
        assert!(
            reg.acquire_for_access("m2", PoolKind::Chat, &access)
                .is_ok()
        );
    }

    /// The trap this pins: `route_access` answers `UnknownModel` by retrying
    /// the kind's configured fallback model. If a denied model reported
    /// `UnknownModel`, a token restricted to a cheap model would silently be
    /// served by the fallback instead of being refused — the restriction
    /// would read as enforced while quietly doing nothing.
    #[test]
    fn a_denied_model_is_refused_rather_than_sent_to_the_fallback() {
        let reg = build_with_fallback(
            vec![(
                "chat",
                pool_config(
                    PoolKind::Chat,
                    PickerStrategy::RoundRobin,
                    vec![backend("a", 16)],
                ),
            )],
            FallbackConfig {
                chat: Some("fallback-model".into()),
                ..Default::default()
            },
        );
        seed_models(&reg, "chat", 0, &["fallback-model", "expensive-model"]);

        let restricted = PoolAccess {
            allowed_models: Some(Arc::new(HashSet::from(["fallback-model".to_string()]))),
            ..Default::default()
        };
        assert!(
            matches!(
                reg.route_access("expensive-model", PoolKind::Chat, &restricted),
                Err(RouteError::ModelNotAllowed(_))
            ),
            "a denied model must never be quietly rerouted to the fallback"
        );

        // An unknown model still falls back for the same caller, so the guard
        // above is specific to the allowlist rather than disabling fallback.
        assert!(
            reg.route_access("no-such-model", PoolKind::Chat, &restricted)
                .is_ok()
        );

        // And when the fallback itself is off the allowlist, the unknown
        // model is a plain 404 rather than a quiet upgrade to a model the
        // token was never allowed.
        let fallback_denied = PoolAccess {
            allowed_models: Some(Arc::new(HashSet::from(["expensive-model".to_string()]))),
            ..Default::default()
        };
        assert!(matches!(
            reg.route_access("no-such-model", PoolKind::Chat, &fallback_denied),
            Err(RouteError::UnknownModel(_))
        ));
    }

    /// Aliases are matched as written. The allowlist is applied to the id the
    /// caller asked for, before alias resolution, so picking `qwen` allows the
    /// alias and not the underlying real id. Both appear in the picker (they
    /// both appear in `/v1/models`), so this is a choice the operator makes
    /// rather than a surprise — but it needs to stay deliberate.
    #[test]
    fn an_allowlist_matches_the_requested_id_not_the_resolved_one() {
        let reg = build(vec![(
            "chat",
            pool_config(
                PoolKind::Chat,
                PickerStrategy::RoundRobin,
                vec![backend_alias("a", names(&["qwen"]))],
            ),
        )]);
        seed_models(&reg, "chat", 0, &["Qwen/Qwen3-235B"]);

        let alias_only = PoolAccess {
            allowed_models: Some(Arc::new(HashSet::from(["qwen".to_string()]))),
            ..Default::default()
        };
        let g = reg
            .acquire_for_access("qwen", PoolKind::Chat, &alias_only)
            .expect("the alias itself is allowed");
        assert_eq!(g.resolved_model(), "Qwen/Qwen3-235B");
        assert!(matches!(
            reg.acquire_for_access("Qwen/Qwen3-235B", PoolKind::Chat, &alias_only),
            Err(RouteError::ModelNotAllowed(_))
        ));
    }

    #[test]
    fn acquire_for_unknown_model_returns_route_error() {
        let reg = build(vec![(
            "chat",
            pool_config(
                PoolKind::Chat,
                PickerStrategy::RoundRobin,
                vec![backend("a", 16)],
            ),
        )]);
        seed_models(&reg, "chat", 0, &["llama-3.1-70b"]);
        let err = reg.acquire_for("gpt-4o", PoolKind::Chat).unwrap_err();
        assert!(matches!(err, RouteError::UnknownModel(_)), "{err:?}");
    }

    #[test]
    fn acquire_for_wrong_kind_is_unknown_model() {
        // Voice pool advertises whisper-1; asking for it under Chat
        // doesn't surface a "wrong kind" error — it just doesn't match a
        // chat-kind pool, so the caller sees UnknownModel. Same UX as if
        // the model wasn't loaded anywhere.
        let reg = build(vec![(
            "voice",
            pool_config(
                PoolKind::Transcription,
                PickerStrategy::RoundRobin,
                vec![backend("a", 16)],
            ),
        )]);
        seed_models(&reg, "voice", 0, &["whisper-1"]);
        let err = reg.acquire_for("whisper-1", PoolKind::Chat).unwrap_err();
        assert!(matches!(err, RouteError::UnknownModel(_)), "{err:?}");
    }

    #[test]
    fn picks_backend_that_serves_the_model_when_pool_is_heterogeneous() {
        let reg = build(vec![(
            "chat",
            pool_config(
                PoolKind::Chat,
                PickerStrategy::RoundRobin,
                vec![backend("a", 16), backend("b", 16)],
            ),
        )]);
        seed_models(&reg, "chat", 0, &["llama-3.1-70b"]);
        seed_models(&reg, "chat", 1, &["llama-3.1-8b"]);

        // 70b lives on backend `a` only — picker shouldn't land on `b`.
        for _ in 0..4 {
            let g = reg.acquire_for("llama-3.1-70b", PoolKind::Chat).unwrap();
            assert_eq!(g.backend().name, "a");
        }
        // …and vice versa.
        for _ in 0..4 {
            let g = reg.acquire_for("llama-3.1-8b", PoolKind::Chat).unwrap();
            assert_eq!(g.backend().name, "b");
        }
    }

    #[test]
    fn models_for_kind_unions_across_pool_backends() {
        let reg = build(vec![
            (
                "chat",
                pool_config(
                    PoolKind::Chat,
                    PickerStrategy::RoundRobin,
                    vec![backend("a", 16), backend("b", 16)],
                ),
            ),
            (
                "voice",
                pool_config(
                    PoolKind::Transcription,
                    PickerStrategy::RoundRobin,
                    vec![backend("c", 16)],
                ),
            ),
        ]);
        seed_models(&reg, "chat", 0, &["llama-3.1-70b"]);
        seed_models(&reg, "chat", 1, &["llama-3.1-8b"]);
        seed_models(&reg, "voice", 0, &["whisper-1"]);

        let mut chat = reg.models_for_kind(PoolKind::Chat);
        chat.sort();
        assert_eq!(chat, vec!["llama-3.1-70b", "llama-3.1-8b"]);
        assert_eq!(
            reg.models_for_kind(PoolKind::Transcription),
            vec!["whisper-1"]
        );
        assert!(reg.models_for_kind(PoolKind::Embedding).is_empty());
    }

    #[test]
    fn models_with_alias_target_marks_aliases_and_real_ids() {
        let reg = build(vec![(
            "chat",
            pool_config(
                PoolKind::Chat,
                PickerStrategy::RoundRobin,
                vec![backend_alias("a", targets(&[("smart", "glm-4.6")]))],
            ),
        )]);
        seed_models(&reg, "chat", 0, &["glm-4.6", "glm-4.5-air"]);

        let map: std::collections::HashMap<String, Option<String>> = reg
            .models_with_alias_target(PoolKind::Chat)
            .into_iter()
            .collect();
        // Real ids own their settings — no alias target.
        assert_eq!(map["glm-4.6"], None);
        assert_eq!(map["glm-4.5-air"], None);
        // The alias resolves to its real target and carries no row of its own.
        assert_eq!(map["smart"], Some("glm-4.6".to_string()));
        // Exactly the two reals plus the one alias, nothing else.
        assert_eq!(map.len(), 3);
    }

    #[test]
    fn compliance_flags_attach_per_model_and_default_clear() {
        let reg = build(vec![
            (
                "zai",
                pool_config_with_compliance(
                    PoolKind::Chat,
                    Compliance {
                        gdpr: false,
                        nda: false,
                    },
                    vec![backend("zai", 16)],
                ),
            ),
            (
                "qwen",
                pool_config(
                    PoolKind::Chat,
                    PickerStrategy::RoundRobin,
                    vec![backend("qwen", 16)],
                ),
            ),
        ]);
        seed_models(&reg, "zai", 0, &["glm-4.6"]);
        seed_models(&reg, "qwen", 0, &["qwen-3"]);

        let map: std::collections::HashMap<String, Compliance> = reg
            .models_with_compliance_for_kind(PoolKind::Chat)
            .into_iter()
            .collect();
        // Flagged pool propagates to its model…
        assert_eq!(
            map["glm-4.6"],
            Compliance {
                gdpr: false,
                nda: false
            }
        );
        // …and a pool with no compliance block stays all-clear.
        assert!(map["qwen-3"].is_all_clear());
    }

    #[test]
    fn compliance_merges_most_restrictively_across_pools() {
        // Same model id served by two pools: one GDPR-safe, one not. The
        // merge must take the restrictive view (not safe).
        let reg = build(vec![
            (
                "safe",
                pool_config(
                    PoolKind::Chat,
                    PickerStrategy::RoundRobin,
                    vec![backend("safe", 16)],
                ),
            ),
            (
                "unsafe",
                pool_config_with_compliance(
                    PoolKind::Chat,
                    Compliance {
                        gdpr: false,
                        nda: true,
                    },
                    vec![backend("unsafe", 16)],
                ),
            ),
        ]);
        seed_models(&reg, "safe", 0, &["shared"]);
        seed_models(&reg, "unsafe", 0, &["shared"]);

        let map: std::collections::HashMap<String, Compliance> = reg
            .models_with_compliance_for_kind(PoolKind::Chat)
            .into_iter()
            .collect();
        // gdpr false on one pool wins; nda clear on both stays clear.
        assert_eq!(
            map["shared"],
            Compliance {
                gdpr: false,
                nda: true
            }
        );
    }

    #[test]
    fn round_robin_cycles_among_matching_backends() {
        let reg = build(vec![(
            "chat",
            pool_config(
                PoolKind::Chat,
                PickerStrategy::RoundRobin,
                vec![backend("a", 16), backend("b", 16), backend("c", 16)],
            ),
        )]);
        seed_models(&reg, "chat", 0, &["m"]);
        seed_models(&reg, "chat", 1, &["m"]);
        seed_models(&reg, "chat", 2, &["m"]);
        let mut picks = Vec::new();
        for _ in 0..6 {
            let g = reg.acquire_for("m", PoolKind::Chat).unwrap();
            picks.push(g.backend().name.clone());
        }
        for n in ["a", "b", "c"] {
            assert!(picks.contains(&n.to_string()), "no pick of {n}: {picks:?}");
        }
    }

    #[test]
    fn skips_unhealthy_backends_in_route_lookup() {
        let reg = build(vec![(
            "chat",
            pool_config(
                PoolKind::Chat,
                PickerStrategy::RoundRobin,
                vec![backend("a", 16), backend("b", 16)],
            ),
        )]);
        seed_models(&reg, "chat", 0, &["m"]);
        seed_models(&reg, "chat", 1, &["m"]);
        // Mark `a` unhealthy — every acquire should land on `b`.
        reg.data().pools.get("chat").unwrap().backends[0].set_healthy(false);
        for _ in 0..5 {
            let g = reg.acquire_for("m", PoolKind::Chat).unwrap();
            assert_eq!(g.backend().name, "b");
        }
    }

    #[test]
    fn least_inflight_prefers_idle_backend() {
        let reg = build(vec![(
            "chat",
            pool_config(
                PoolKind::Chat,
                PickerStrategy::LeastInflight,
                vec![backend("a", 16), backend("b", 16)],
            ),
        )]);
        seed_models(&reg, "chat", 0, &["m"]);
        seed_models(&reg, "chat", 1, &["m"]);
        let d = reg.data();
        let pool = d.pools.get("chat").unwrap();
        // Hold one slot via Pool API directly — exercising the inflight counter.
        let _a1 = pool.acquire_for_model("m").unwrap();
        // Force a's inflight up so the picker prefers b.
        pool.backends[0].inflight.store(5, Ordering::Relaxed);
        let g = reg.acquire_for("m", PoolKind::Chat).unwrap();
        assert_eq!(g.backend().name, "b");
    }

    #[test]
    fn saturated_when_all_matching_backends_at_max() {
        let reg = build(vec![(
            "chat",
            pool_config(
                PoolKind::Chat,
                PickerStrategy::LeastInflight,
                vec![backend("a", 1), backend("b", 1)],
            ),
        )]);
        seed_models(&reg, "chat", 0, &["m"]);
        seed_models(&reg, "chat", 1, &["m"]);
        let _g1 = reg.acquire_for("m", PoolKind::Chat).unwrap();
        let _g2 = reg.acquire_for("m", PoolKind::Chat).unwrap();
        let err = reg.acquire_for("m", PoolKind::Chat).unwrap_err();
        assert!(
            matches!(
                err,
                RouteError::Acquire(AcquireError::Saturated { ref pool }) if pool == "chat"
            ),
            "{err:?}"
        );
    }

    #[test]
    fn empty_model_set_means_no_route() {
        // First-request-before-first-probe scenario. `health::spawn` blocks
        // on the initial probe in production so this only happens if the
        // upstream is unreachable at boot — in which case UnknownModel is
        // the right surface error (the user wouldn't even know what model
        // to ask for yet).
        let reg = build(vec![(
            "chat",
            pool_config(
                PoolKind::Chat,
                PickerStrategy::RoundRobin,
                vec![backend("a", 16)],
            ),
        )]);
        let err = reg.acquire_for("anything", PoolKind::Chat).unwrap_err();
        assert!(matches!(err, RouteError::UnknownModel(_)), "{err:?}");
    }

    #[test]
    fn config_models_route_without_a_probe() {
        // A transcription backend with no working `/models` endpoint: the
        // probe set stays empty, but the pool-level `models` fallback makes
        // it routable and listable anyway.
        let reg = build(vec![(
            "voice",
            pool_config_with_models(
                PoolKind::Transcription,
                &["voxtral-realtime"],
                vec![backend("a", 16)],
            ),
        )]);
        // No seed_models — the probe never reported anything.
        let g = reg
            .acquire_for("voxtral-realtime", PoolKind::Transcription)
            .unwrap();
        assert_eq!(g.backend().name, "a");
        assert_eq!(reg.all_models(), vec!["voxtral-realtime"]);
        assert!(reg.knows_model("voxtral-realtime", PoolKind::Transcription));
    }

    #[test]
    fn image_pool_routes_by_config_models() {
        // An image backend whose `/models` isn't discovered (probe off, or no
        // such endpoint) is still routable via its static model ids — the same
        // mechanism transcription relies on, now for PoolKind::Image.
        let reg = build(vec![(
            "images",
            pool_config_with_models(PoolKind::Image, &["glm-image"], vec![backend("a", 16)]),
        )]);
        let g = reg.acquire_for("glm-image", PoolKind::Image).unwrap();
        assert_eq!(g.backend().name, "a");
        assert!(reg.knows_model("glm-image", PoolKind::Image));
        // Wrong kind must not match an image model.
        assert!(reg.acquire_for("glm-image", PoolKind::Chat).is_err());
    }

    #[test]
    fn backend_config_models_win_over_pool_config_models() {
        let reg = build(vec![(
            "voice",
            pool_config_with_models(
                PoolKind::Transcription,
                &["pool-model"],
                vec![backend_with_models("a", &["backend-model"])],
            ),
        )]);
        // Backend declared its own models, so the pool fallback is ignored
        // for that backend.
        assert!(
            reg.acquire_for("backend-model", PoolKind::Transcription)
                .is_ok()
        );
        let err = reg
            .acquire_for("pool-model", PoolKind::Transcription)
            .unwrap_err();
        assert!(matches!(err, RouteError::UnknownModel(_)), "{err:?}");
        assert_eq!(reg.all_models(), vec!["backend-model"]);
    }

    #[test]
    fn config_models_allowlist_restricts_live_probe() {
        // A configured `models` list is an allowlist over the live probe: only
        // the ids it names are served/advertised; a probed id it omits is
        // discovered-but-withheld (404 on request, absent from `/v1/models`).
        let reg = build(vec![(
            "voice",
            pool_config_with_models(
                PoolKind::Transcription,
                &["keep-a", "keep-b"],
                vec![backend("a", 16)],
            ),
        )]);
        seed_models(&reg, "voice", 0, &["keep-a", "keep-b", "drop-c"]);
        assert!(
            reg.acquire_for("keep-a", PoolKind::Transcription).is_ok(),
            "allowlisted id must route"
        );
        assert!(reg.acquire_for("keep-b", PoolKind::Transcription).is_ok());
        let err = reg
            .acquire_for("drop-c", PoolKind::Transcription)
            .unwrap_err();
        assert!(
            matches!(err, RouteError::UnknownModel(_)),
            "withheld id must 404 even though the backend reports it: {err:?}"
        );
        // Advertised set is the allowlist ∩ probe, not the whole probe.
        assert_eq!(reg.all_models(), vec!["keep-a", "keep-b"]);
        // The withheld id surfaces for the struck-through UI chip.
        let d = reg.data();
        let b = &d.pools.get("voice").unwrap().backends[0];
        assert_eq!(b.withheld_models(), HashSet::from(["drop-c".to_string()]));
    }

    #[test]
    fn empty_model_list_serves_whole_probe_and_withholds_nothing() {
        // With no configured list, the probe set is served verbatim — the
        // allowlist is opt-in, so unconfigured pools are unaffected.
        let reg = build(vec![(
            "chat",
            pool_config(
                PoolKind::Chat,
                PickerStrategy::RoundRobin,
                vec![backend("a", 16)],
            ),
        )]);
        seed_models(&reg, "chat", 0, &["m1", "m2"]);
        assert!(reg.acquire_for("m1", PoolKind::Chat).is_ok());
        assert!(reg.acquire_for("m2", PoolKind::Chat).is_ok());
        assert_eq!(reg.all_models(), vec!["m1", "m2"]);
        let d = reg.data();
        let b = &d.pools.get("chat").unwrap().backends[0];
        assert!(
            b.withheld_models().is_empty(),
            "no allowlist → nothing withheld"
        );
    }

    #[test]
    fn all_models_dedups_across_replicas_and_unions_across_kinds() {
        let reg = build(vec![
            (
                "chat",
                pool_config(
                    PoolKind::Chat,
                    PickerStrategy::RoundRobin,
                    vec![backend("a", 16), backend("b", 16)],
                ),
            ),
            (
                "voice",
                pool_config_with_models(
                    PoolKind::Transcription,
                    &["whisper-1"],
                    vec![backend("c", 16)],
                ),
            ),
        ]);
        // Both chat replicas serve the same id — must collapse to one entry.
        seed_models(&reg, "chat", 0, &["qwen"]);
        seed_models(&reg, "chat", 1, &["qwen"]);
        // Transcription model comes purely from config (no probe).
        assert_eq!(reg.all_models(), vec!["qwen", "whisper-1"]);
    }

    #[test]
    fn known_model_with_all_replicas_unhealthy_is_503_not_404() {
        // Distinguishes "model exists but every replica is down" (transient,
        // 503) from "no backend serves this id" (client error, 404).
        let reg = build(vec![(
            "chat",
            pool_config(
                PoolKind::Chat,
                PickerStrategy::RoundRobin,
                vec![backend("a", 16)],
            ),
        )]);
        seed_models(&reg, "chat", 0, &["m"]);
        reg.data().pools.get("chat").unwrap().backends[0].set_healthy(false);

        // Still "known" (health-agnostic)…
        assert!(reg.knows_model("m", PoolKind::Chat));
        // …so acquire surfaces NoHealthyBackend, not UnknownModel.
        let err = reg.acquire_for("m", PoolKind::Chat).unwrap_err();
        assert!(
            matches!(
                err,
                RouteError::Acquire(AcquireError::NoHealthyBackend { ref pool }) if pool == "chat"
            ),
            "{err:?}"
        );

        // A genuinely unknown id is still UnknownModel.
        let err = reg.acquire_for("nope", PoolKind::Chat).unwrap_err();
        assert!(matches!(err, RouteError::UnknownModel(_)), "{err:?}");
    }

    #[test]
    fn knows_any_spans_all_kinds() {
        let reg = build(vec![(
            "voice",
            pool_config_with_models(
                PoolKind::Transcription,
                &["whisper-1"],
                vec![backend("a", 16)],
            ),
        )]);
        assert!(reg.knows_any("whisper-1"));
        assert!(!reg.knows_any("unknown"));
    }

    // ------- aliases + fallback -------

    fn backend_alias(name: &str, spec: AliasSpec) -> BackendConfig {
        BackendConfig {
            alias: Some(spec),
            ..backend(name, 16)
        }
    }

    fn names(v: &[&str]) -> AliasSpec {
        AliasSpec::Names(v.iter().map(|s| (*s).to_string()).collect())
    }

    fn targets(pairs: &[(&str, &str)]) -> AliasSpec {
        AliasSpec::Targets(
            pairs
                .iter()
                .map(|(k, t)| ((*k).to_string(), (*t).to_string()))
                .collect(),
        )
    }

    fn pool_offline(
        kind: PoolKind,
        offline: &str,
        backends: Vec<BackendConfig>,
    ) -> UpstreamPoolConfig {
        UpstreamPoolConfig {
            voices: Default::default(),
            offer_voices: Vec::new(),
            allowed_groups: Vec::new(),
            compliance: Default::default(),
            enforce_limits: true,
            kind,
            strategy: PickerStrategy::RoundRobin,
            models: Vec::new(),
            fallback_offline: Some(offline.to_string()),
            backend: backends,
        }
    }

    fn build_with_fallback(
        pools: Vec<(&str, UpstreamPoolConfig)>,
        fallback: FallbackConfig,
    ) -> Arc<UpstreamRegistry> {
        let map: HashMap<String, UpstreamPoolConfig> =
            pools.into_iter().map(|(k, v)| (k.into(), v)).collect();
        UpstreamRegistry::with_fallback(&map, fallback).unwrap()
    }

    fn try_build(
        pools: Vec<(&str, UpstreamPoolConfig)>,
    ) -> Result<Arc<UpstreamRegistry>, BuildError> {
        let map: HashMap<String, UpstreamPoolConfig> =
            pools.into_iter().map(|(k, v)| (k.into(), v)).collect();
        UpstreamRegistry::new(&map)
    }

    fn set_health(reg: &UpstreamRegistry, pool: &str, idx: usize, healthy: bool) {
        reg.data().pools.get(pool).unwrap().backends[idx].set_healthy(healthy);
    }

    #[test]
    fn bare_alias_resolves_to_backends_sole_model() {
        let reg = build(vec![(
            "chat",
            pool_config(
                PoolKind::Chat,
                PickerStrategy::RoundRobin,
                vec![backend_alias("a", names(&["qwen", "fast"]))],
            ),
        )]);
        seed_models(&reg, "chat", 0, &["Qwen/Qwen3-235B"]);

        // Both alias names route, and resolve to the backend's real id.
        let g = reg.route("qwen", PoolKind::Chat).unwrap();
        assert_eq!(g.backend().name, "a");
        assert_eq!(g.resolved_model(), "Qwen/Qwen3-235B");
        assert_eq!(
            reg.route("fast", PoolKind::Chat).unwrap().resolved_model(),
            "Qwen/Qwen3-235B"
        );
        // The real id still routes and resolves to itself.
        assert_eq!(
            reg.route("Qwen/Qwen3-235B", PoolKind::Chat)
                .unwrap()
                .resolved_model(),
            "Qwen/Qwen3-235B"
        );
        // Both the alias and the real id are listed.
        let listed = reg.all_models();
        assert!(listed.contains(&"qwen".to_string()));
        assert!(listed.contains(&"fast".to_string()));
        assert!(listed.contains(&"Qwen/Qwen3-235B".to_string()));
    }

    #[test]
    fn shared_alias_forms_a_group_resolving_to_each_backends_real_id() {
        let reg = build(vec![(
            "chat",
            pool_config(
                PoolKind::Chat,
                PickerStrategy::RoundRobin,
                vec![
                    backend_alias("a", names(&["qwen"])),
                    backend_alias("b", names(&["qwen"])),
                ],
            ),
        )]);
        seed_models(&reg, "chat", 0, &["Qwen/Qwen2.5-72B"]);
        seed_models(&reg, "chat", 1, &["Qwen/Qwen3-30B-A3B"]);

        // Round-robin across the group; each hop rewrites to that backend's id.
        let g1 = reg.route("qwen", PoolKind::Chat).unwrap();
        let g2 = reg.route("qwen", PoolKind::Chat).unwrap();
        let mut resolved = [
            g1.resolved_model().to_string(),
            g2.resolved_model().to_string(),
        ];
        resolved.sort();
        assert_eq!(resolved, ["Qwen/Qwen2.5-72B", "Qwen/Qwen3-30B-A3B"]);
        // Pinning a real id hits exactly that backend.
        assert_eq!(
            reg.route("Qwen/Qwen3-30B-A3B", PoolKind::Chat)
                .unwrap()
                .backend()
                .name,
            "b"
        );
    }

    #[test]
    fn bare_alias_disabled_when_backend_serves_multiple_models() {
        let reg = build(vec![(
            "chat",
            pool_config(
                PoolKind::Chat,
                PickerStrategy::RoundRobin,
                vec![backend_alias("a", names(&["qwen"]))],
            ),
        )]);
        // Two models → "the sole model" is ambiguous → alias disabled + logged.
        seed_models(&reg, "chat", 0, &["m-1", "m-2"]);
        assert!(matches!(
            reg.route("qwen", PoolKind::Chat).unwrap_err(),
            RouteError::UnknownModel(_)
        ));
        assert!(!reg.all_models().contains(&"qwen".to_string()));
        // The backend's real ids still route fine.
        assert!(reg.route("m-1", PoolKind::Chat).is_ok());
        // Drop back to one model → alias re-enables.
        seed_models(&reg, "chat", 0, &["m-1"]);
        assert_eq!(
            reg.route("qwen", PoolKind::Chat).unwrap().resolved_model(),
            "m-1"
        );
    }

    #[test]
    fn map_alias_targets_specific_model_even_on_multi_model_backend() {
        let reg = build(vec![(
            "chat",
            pool_config(
                PoolKind::Chat,
                PickerStrategy::RoundRobin,
                vec![backend_alias(
                    "zai",
                    targets(&[("smart", "glm-4.6"), ("cheap", "glm-4.5-air")]),
                )],
            ),
        )]);
        seed_models(&reg, "chat", 0, &["glm-4.6", "glm-4.5-air"]);
        assert_eq!(
            reg.route("smart", PoolKind::Chat).unwrap().resolved_model(),
            "glm-4.6"
        );
        assert_eq!(
            reg.route("cheap", PoolKind::Chat).unwrap().resolved_model(),
            "glm-4.5-air"
        );
    }

    #[test]
    fn map_alias_does_not_resolve_while_target_unserved() {
        let reg = build(vec![(
            "chat",
            pool_config(
                PoolKind::Chat,
                PickerStrategy::RoundRobin,
                vec![backend_alias("zai", targets(&[("smart", "glm-4.6")]))],
            ),
        )]);
        seed_models(&reg, "chat", 0, &["something-else"]);
        assert!(matches!(
            reg.route("smart", PoolKind::Chat).unwrap_err(),
            RouteError::UnknownModel(_)
        ));
    }

    #[test]
    fn alias_colliding_with_config_model_refuses_build() {
        // Backend statically declares model "qwen" AND an alias "qwen".
        let mut b = backend_alias("a", names(&["qwen"]));
        b.models = vec!["qwen".into()];
        let err = try_build(vec![(
            "chat",
            pool_config(PoolKind::Chat, PickerStrategy::RoundRobin, vec![b]),
        )])
        .unwrap_err();
        assert!(
            matches!(err, BuildError::AliasCollidesWithModel { .. }),
            "{err:?}"
        );
    }

    #[test]
    fn map_target_not_in_declared_models_refuses_build() {
        let mut b = backend_alias("a", targets(&[("smart", "not-served")]));
        b.models = vec!["glm-4.6".into()];
        let err = try_build(vec![(
            "chat",
            pool_config(PoolKind::Chat, PickerStrategy::RoundRobin, vec![b]),
        )])
        .unwrap_err();
        assert!(
            matches!(err, BuildError::AliasTargetUnknown { .. }),
            "{err:?}"
        );
    }

    #[test]
    fn fallback_offline_fires_only_when_whole_group_is_down() {
        let reg = build(vec![
            (
                "local",
                pool_offline(
                    PoolKind::Chat,
                    "cloud-model",
                    vec![backend("a", 16), backend("b", 16)],
                ),
            ),
            (
                "cloud",
                pool_config(
                    PoolKind::Chat,
                    PickerStrategy::RoundRobin,
                    vec![backend("c", 16)],
                ),
            ),
        ]);
        seed_models(&reg, "local", 0, &["m"]);
        seed_models(&reg, "local", 1, &["m"]);
        seed_models(&reg, "cloud", 0, &["cloud-model"]);

        // One replica down → still served by the other, NOT the offline backup.
        set_health(&reg, "local", 0, false);
        let g = reg.route("m", PoolKind::Chat).unwrap();
        assert_eq!(g.resolved_model(), "m");
        assert_eq!(g.backend().name, "b");

        // Whole group down → spill to fallback_offline.
        set_health(&reg, "local", 1, false);
        let g = reg.route("m", PoolKind::Chat).unwrap();
        assert_eq!(g.resolved_model(), "cloud-model");
        assert_eq!(g.backend().name, "c");
    }

    #[test]
    fn fallback_offline_single_hop_returns_original_503_when_backup_also_down() {
        let reg = build(vec![(
            "local",
            pool_offline(PoolKind::Chat, "cloud-model", vec![backend("a", 16)]),
        )]);
        seed_models(&reg, "local", 0, &["m"]);
        set_health(&reg, "local", 0, false); // known but down; backup "cloud-model" served by nobody
        assert!(matches!(
            reg.route("m", PoolKind::Chat).unwrap_err(),
            RouteError::Acquire(AcquireError::NoHealthyBackend { .. })
        ));
    }

    #[test]
    fn unknown_model_falls_back_per_kind() {
        let reg = build_with_fallback(
            vec![(
                "chat",
                pool_config(
                    PoolKind::Chat,
                    PickerStrategy::RoundRobin,
                    vec![backend("a", 16)],
                ),
            )],
            FallbackConfig {
                chat: Some("house-model".into()),
                ..Default::default()
            },
        );
        seed_models(&reg, "chat", 0, &["house-model"]);
        // Never-heard-of model → substitute the house model.
        let g = reg.route("gpt-4-turbo", PoolKind::Chat).unwrap();
        assert_eq!(g.resolved_model(), "house-model");
        // Unset kind isn't rescued by the chat fallback.
        assert!(matches!(
            reg.route("gpt-4-turbo", PoolKind::Embedding).unwrap_err(),
            RouteError::UnknownModel(_)
        ));
    }

    #[test]
    fn unknown_fallback_is_single_hop_and_404_without_config() {
        // No fallback configured → plain 404.
        let reg = build(vec![(
            "chat",
            pool_config(
                PoolKind::Chat,
                PickerStrategy::RoundRobin,
                vec![backend("a", 16)],
            ),
        )]);
        seed_models(&reg, "chat", 0, &["m"]);
        assert!(matches!(
            reg.route("nope", PoolKind::Chat).unwrap_err(),
            RouteError::UnknownModel(_)
        ));

        // Fallback points at a model nobody serves → original 404, no loop.
        let reg = build_with_fallback(
            vec![(
                "chat",
                pool_config(
                    PoolKind::Chat,
                    PickerStrategy::RoundRobin,
                    vec![backend("a", 16)],
                ),
            )],
            FallbackConfig {
                chat: Some("also-missing".into()),
                ..Default::default()
            },
        );
        seed_models(&reg, "chat", 0, &["m"]);
        assert!(matches!(
            reg.route("nope", PoolKind::Chat).unwrap_err(),
            RouteError::UnknownModel(_)
        ));
    }

    #[test]
    fn saturation_does_not_fall_back() {
        let reg = build(vec![(
            "local",
            pool_offline(PoolKind::Chat, "cloud-model", vec![backend("a", 1)]),
        )]);
        seed_models(&reg, "local", 0, &["m"]);
        // Take the only slot; the model is loaded+healthy but saturated.
        let _held = reg.route("m", PoolKind::Chat).unwrap();
        assert!(
            matches!(
                reg.route("m", PoolKind::Chat).unwrap_err(),
                RouteError::Acquire(AcquireError::Saturated { .. })
            ),
            "saturation must 503, never spill to fallback_offline"
        );
    }

    #[test]
    fn resolve_model_is_alias_aware_and_takes_no_slot() {
        let reg = build(vec![(
            "chat",
            pool_config(
                PoolKind::Chat,
                PickerStrategy::RoundRobin,
                vec![backend_alias("a", names(&["qwen"]))],
            ),
        )]);
        seed_models(&reg, "chat", 0, &["Qwen/Qwen3-235B"]);
        assert_eq!(
            reg.resolve_model("qwen", PoolKind::Chat).as_deref(),
            Some("Qwen/Qwen3-235B")
        );
        assert_eq!(reg.resolve_model("nope", PoolKind::Chat), None);
        // No inflight slot consumed — the backend is still at 0.
        assert_eq!(
            reg.data().pools.get("chat").unwrap().backends[0].inflight(),
            0
        );
    }

    /// A reload bumps the generation and carries an unchanged backend's live
    /// (probed) model set onto its freshly-built replacement, so routing never
    /// 404s during the re-probe window.
    #[test]
    fn reload_carries_over_live_models_and_bumps_generation() {
        use crate::server::db::upstreams_config::{BackendRow, PoolRow, UpstreamConfigSnapshot};
        use jiff::Timestamp;

        let mk_snap = || {
            let mut snap = UpstreamConfigSnapshot::default();
            snap.backends.insert(
                "b".into(),
                BackendRow {
                    name: "b".into(),
                    base_url: "http://b".into(),
                    api_key_env: None,
                    api_key_ct: None,
                    api_key_nonce: None,
                    weight: 1,
                    max_inflight: 16,
                    health_path: "/models".into(),
                    probe_models: true,
                    supports_edit: false,
                    enabled: true,
                    models: vec![],
                    aliases: vec![],
                    created_at: Timestamp::now(),
                    updated_at: Timestamp::now(),
                },
            );
            snap.pools.push(PoolRow {
                name: "chat".into(),
                kind: "chat".into(),
                strategy: "least_inflight".into(),
                fallback_offline: None,
                compliance_gdpr: true,
                compliance_nda: true,
                enforce_limits: true,
                sort_order: 0,
                allowed_groups: Vec::new(),
                backends: vec!["b".into()],
                models: vec![],
                voices: vec![],
                offer_voices: Vec::new(),
                created_at: Timestamp::now(),
                updated_at: Timestamp::now(),
            });
            snap
        };

        let reg = UpstreamRegistry::from_snapshot(
            &mk_snap(),
            &crate::server::crypto::Crypto::ephemeral(),
        )
        .unwrap();
        assert_eq!(reg.generation(), 0);

        // Simulate a successful probe populating the live model set.
        reg.data().pools.get("chat").unwrap().backends[0]
            .set_models(HashSet::from(["live-model".to_string()]));
        assert!(reg.knows_model("live-model", PoolKind::Chat));

        // Reload the same topology: the rebuilt backend starts with an empty
        // live set, but the carry-over must keep "live-model" routable.
        reg.reload(&mk_snap(), &crate::server::crypto::Crypto::ephemeral())
            .unwrap();
        assert_eq!(reg.generation(), 1);
        assert!(
            reg.knows_model("live-model", PoolKind::Chat),
            "reload must carry over the live model set for an unchanged backend"
        );
    }
}
