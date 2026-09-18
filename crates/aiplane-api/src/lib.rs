// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The gateway's `/api/v0` JSON layer — what the SvelteKit SPA is built on.
//!
//! Handlers take a
//! [`aiplane_runtime::RamaState`](aiplane_runtime::rama_server::state::RamaState)
//! and return rama responses carrying JSON: chat (sessions, turns, the submit
//! core and the event stream), the admin surfaces, the workspace surfaces
//! (memories, scheduled actions, webhooks), and skills/connectors.
//!
//! It also keeps the handful of routes that are not an API and not a page
//! either: the two public `/hooks` triggers, whose credential is the URL, and
//! the OAuth round-trips, whose redirect URIs are registered with external
//! providers. They answer a browser directly, so they are the one place a
//! whole HTML page is still rendered — see `flow_error_page`.
//!
//! This crate is a pure sink: nothing in `aiplane-core` references it, and only
//! the router in the `gateway` binary crate mounts it. That's deliberate — it
//! keeps a handler edit to a leaf-crate rebuild. See `docs/architecture.md`.
//!
//! [`build_info`] (and the `build.rs` that feeds it the git SHA) lives here
//! rather than in `aiplane-core` for the same reason: the page chrome is its
//! only consumer, so a new commit invalidates this crate and the binary but
//! leaves the much larger `aiplane-core` cached.

pub mod build_info;
pub mod pages;
