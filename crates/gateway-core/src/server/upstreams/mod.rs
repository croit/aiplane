// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Multi-provider routing.
//!
//! Stack:
//!
//! - **Config** (`config.rs`): the typed `[upstream_pools]` map. No
//!   `[[models]]` routing table — routes are discovered at runtime from
//!   each backend's own `/models` response (see Health below).
//! - **Runtime** (`registry.rs`): the `UpstreamRegistry` owns `Pool`s; each
//!   backend tracks the set of model IDs it currently advertises.
//!   `acquire_for(model, kind)` walks pools matching the kind, finds the
//!   first one whose backends serve `model`, picks one of those backends
//!   via the pool's strategy, and returns an `Acquired` RAII guard that
//!   releases the in-flight slot on drop.
//! - **Affinity** (`affinity.rs`): the routing key that keeps one conversation
//!   on the replica already holding its KV prefix. Load balancing alone sends a
//!   session's consecutive turns to alternating GPUs, where every turn pays a
//!   full prefill instead of a prefix-cache hit.
//! - **Prefix index** (`prefix_index.rs`): an approximate, per-pool map of
//!   prompt prefix → the replicas that were recently sent it, so routing can
//!   ask "who has the longest matching prefix" instead of "whose conversation
//!   is this". Same shape as llm-d's approximate prefix-cache scorer.
//! - **Waiting** (`wait.rs`): `route_or_wait` parks a request whose pool has no
//!   healthy backend rather than failing it on the spot, so a short upstream
//!   outage (a restarting GPU box, a model swap) is a pause in the client's
//!   stream instead of a broken turn.
//! - **Health** (`health.rs`): one background task per backend, hitting
//!   `<base_url>/models`. On every successful probe the response is
//!   parsed as the OpenAI envelope (`{"data": [{"id": ...}]}`) and the
//!   backend's advertised-model set is replaced. Three consecutive
//!   failures mark unhealthy; one success flips back. `spawn` blocks on
//!   an initial parallel probe round so the first request finds populated
//!   sets.
//!
//! See `docs/upstreams.md` for the wire/config shape and rationale.

pub mod affinity;
pub mod config;
pub mod db_bridge;
pub mod error_classify;
pub mod health;
pub mod prefix_index;
pub mod registry;
pub mod wait;

pub use config::{
    AliasSpec, BackendConfig, Compliance, FallbackConfig, PickerStrategy, PoolKind,
    UpstreamPoolConfig,
};
pub use prefix_index::PrefixIndex;
pub use registry::{
    AcquireError, Acquired, AliasStatus, Backend, LiveBackend, LivePool, LiveTopology, Pool,
    PoolAccess, RouteError, UpstreamRegistry,
};
pub use wait::route_or_wait;
