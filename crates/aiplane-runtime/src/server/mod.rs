// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Tool API, shared state, and the state-dependent background workers. Module
//! paths keep the historical `server::` prefix; see the crate docs.

pub mod background;
pub mod comfyui_tool;
pub mod compaction;
pub mod grant_holding;
pub mod headless;
pub mod model_choices;
pub mod model_route;
pub mod scheduled;
pub mod side_call;
pub mod state;
pub mod tools;
pub mod webhooks;
pub mod workspace_cleanup;

pub use state::AppState;
