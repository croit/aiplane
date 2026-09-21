// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The gateway's request runtime: the tool API, the shared state handles, and
//! the workers that drive a turn.
//!
//! - [`server::tools`] — the `Tool` trait, `ToolContext`, the registry, the
//!   catalog, the round-loop runner, the MCP connection manager, the sandbox
//!   client. Implementations live in `aiplane-tools`, above this crate.
//! - [`server::state`] / [`rama_server::state`] — `AppState` and the `RamaState`
//!   that wraps it. This is the layer that ties the whole world together, which
//!   is why it sits above both `aiplane-core` and `aiplane-features`.
//! - [`openai_driver`] — the streaming chat-completion driver, plus the
//!   background workers that need state: `scheduled`, `webhooks`, `compaction`,
//!   `headless`.
//!
//! `aiplane-tools` and `aiplane-api` both depend on this and on nothing of each
//! other, so a tool edit and a page edit stay independent.

pub mod content_guard;
pub mod loop_guard;
pub mod openai_driver;
pub mod rama_server;
pub mod server;

pub use rama_server::state::RamaState;
pub use server::state::AppState;
