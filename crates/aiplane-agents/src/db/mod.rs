// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Accessors for the agent tables. The schema stays in the one embedded
//! migration set in `aiplane-core` (`migrations/`), so the sqlx history is
//! never split; only the Rust that reads and writes those tables lives here,
//! on `aiplane-core`'s pool, error type and timestamp helpers.

pub mod a2a_contexts;
pub mod agent_a2a_tasks;
pub mod agent_analytics;
pub mod agent_audit;
pub mod agent_channels;
pub mod agent_retention;
pub mod agent_state;
pub mod agent_tests;
pub mod agent_verifiers;
pub mod agents;
pub mod architect_sessions;
pub mod dot_delegation;
pub mod dot_memories;
pub mod dot_sessions;
pub mod embed_keys;
pub mod run_sessions;
pub mod system_principals;
pub mod visitor_sessions;
pub mod write_tx;

pub use aiplane_core::server::db::{DbError, Pool, parse_optional_ts, parse_ts, window_key};
pub use write_tx::WriteTx;
