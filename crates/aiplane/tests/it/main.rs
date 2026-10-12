// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Single integration-test harness: every former `tests/<name>.rs` is a
//! module here, so the 36 test files link into ONE binary instead of 36.
//! (The live, env-gated `sandbox_e2e_live.rs` stays a separate binary.)
//! nextest still runs each #[test] in its own process, so tests that touch
//! process-global state (env vars) stay isolated.

// Tests build plain clients and drain bodies to talk to their in-process
// mocks; the outbound and body rules in clippy.toml are about production paths.
#![allow(clippy::disallowed_methods)]

mod a2a;
mod admin_json_api;
mod agent_activity;
mod agent_analytics;
mod agent_architect;
mod agent_assist;
mod agent_evaluation;
mod agent_test_chat;
mod agents;
mod anthropic_messages;
mod architecture;
mod ask_feedback;
mod automatic_routing;
mod body_limit;
mod browser_feedback;
mod chat_json_api;
mod comfyui_integration;
mod common;
mod cors;
#[cfg(debug_assertions)]
mod dev_seed;
mod docs_current;
mod dot_access;
mod dot_background_chat;
mod dot_communication;
mod dot_connections;
mod dot_oauth;
mod dot_workspace;
mod dots_background;
mod embed;
mod feedback_extract;
mod healthz;
mod metrics;
mod oidc_integration;
mod openai_responses;
mod openapi_drift;
mod organizations;
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
mod system_one;
mod system_principals;
mod token_scope;
mod tool_loop;
mod tools_inventory;
mod transcriptions;
mod turn_suspension;
mod typst_compile;
mod webhook_trigger;
