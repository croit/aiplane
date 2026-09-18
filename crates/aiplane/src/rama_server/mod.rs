// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Rama-based HTTP routing — the gateway's request-dispatch surface.
//!
//! Reuses the database, OIDC client, upstream registry, RBAC resolver, and tool
//! registry from `aiplane_core::server` unchanged — those modules are
//! framework-neutral — and mounts the HTML handlers from `aiplane_api::pages`.
//! Only the routes themselves live here.
//!
//! The pieces both this crate and `aiplane-api` need — [`RamaState`], the
//! signed-cookie session store, the `/v1` bearer middleware, and CORS — sit
//! below both in `aiplane_core::rama_server`, and are re-exported here so the
//! router and the integration tests keep a single import path.

pub mod api;
pub mod comfyui_api;
#[cfg(debug_assertions)]
pub mod dev_seed;
pub mod first_run;
pub mod messages;
pub mod oidc_handlers;
pub mod openapi;
pub mod proxy;
pub mod rag_api;
pub mod router;
pub mod sandbox_api;
pub mod setup_api;
pub mod spa;
pub mod vad;

pub use aiplane_api::pages;
pub use aiplane_core::rama_server::{SessionStore, cors, session};
pub use aiplane_runtime::rama_server::{RamaState, auth, state};
pub use router::router;
