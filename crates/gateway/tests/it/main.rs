// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Single integration-test harness: every former `tests/<name>.rs` is a
//! module here, so the 36 test files link into ONE binary instead of 36.
//! (The live, env-gated `sandbox_e2e_live.rs` stays a separate binary.)
//! nextest still runs each #[test] in its own process, so tests that touch
//! process-global state (env vars) stay isolated.

mod admin_json_api;
mod anthropic_messages;
mod ask_feedback;
mod chat_json_api;
mod comfyui_integration;
mod common;
mod cors;
#[cfg(debug_assertions)]
mod dev_seed;
mod healthz;
mod oidc_integration;
mod openapi_drift;
mod proxy;
mod push_routes;
mod rag;
mod rag_api;
mod rag_eval;
mod rag_extract;
mod rag_gdrive;
mod rag_hyperkitty;
mod rag_incremental;
mod rag_profile;
mod rag_webdav;
mod rbac;
mod readme_routes;
mod session_routes;
mod setup_wizard;
mod spa_routes;
mod speech_voice;
mod token_scope;
mod tool_loop;
mod tools_inventory;
mod transcriptions;
mod typst_compile;
