// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Top-level rama Router.
//!
//! Route groups, in the order they are registered — which is the order this
//! router matches in, so the SPA catch-all must stay last:
//!   - **Non-UI survivors**: the two public `/hooks` triggers (the URL is the
//!     credential) and the OAuth round-trips, whose redirect URIs are
//!     registered with external providers and so cannot move.
//!   - **Model API proxy**: `/v1/models`, `/v1/chat/completions`, `/v1/systemone`,
//!     `/v1/audio/*`, `/v1/embeddings`, `/v1/images/*` — bearer-authenticated,
//!     forwarded to the upstream pool selected by model.
//!   - **Anthropic-compatible proxy**: `/v1/messages` — the same pipeline
//!     behind the Messages API wire format, so Claude Code and other
//!     Anthropic-format clients can be pointed here.
//!   - **Auth + session API**: `/auth/*` (the OIDC browser flow) and
//!     `/api/v0/*`, the JSON surface the SvelteKit SPA is built on.
//!   - **The SPA itself**: every other path, served from disk by
//!     [`spa`], with `index.html` as the history fallback.

use std::sync::Arc;
use std::time::{Duration, Instant};

use rama::http::StatusCode;
use rama::http::layer::error_handling::ErrorHandlerLayer;
use rama::http::server::HttpServer;
use rama::http::service::web::Router;
use rama::http::service::web::extract::State;
use rama::http::service::web::response::Json;
use rama::layer::{ArcLayer, Layer};
use rama::net::address::SocketAddress;
use rama::rt::Executor;
use serde_json::json;

use crate::rama_server::RamaState;
#[cfg(debug_assertions)]
use crate::rama_server::dev_seed;
use crate::rama_server::first_run::FirstRunLayer;
use crate::rama_server::setup_api;
use crate::rama_server::{
    api, comfyui_api, messages, oidc_handlers, openapi, pages, proxy, rag_api, sandbox_api, spa,
};
use aiplane_core::rama_server::cors::V1CorsLayer;

/// Builds the rama router. State is shared via `Arc` since handlers
/// borrow it immutably.
pub fn router(state: Arc<RamaState>) -> Router<Arc<RamaState>> {
    let router = Router::new_with_state(state)
        .with_get("/healthz", async || Json(json!({"status": "ok"})))
        .with_get("/openapi.json", openapi::document)
        .with_get("/api/v0/build", aiplane_api::build_info::metadata)
        // `/healthz` is liveness — the process is up. `/readyz` is readiness,
        // and an unconfigured gateway is not ready: it cannot serve a single
        // authenticated request, so a load balancer must not route production
        // traffic to it while an operator is still walking through `/setup`.
        .with_get("/readyz", async |State(state): State<Arc<RamaState>>| {
            if state.setup_completed() {
                (StatusCode::OK, Json(json!({"status": "ok"})))
            } else {
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({"status": "setup_required"})),
                )
            }
        })
        // --- Non-UI routes that outlived the server-rendered pages --------
        //
        // The SPA replaced every page, but these four are not a UI: two are
        // public triggers whose credential is the URL itself, and two are
        // OAuth round-trips whose redirect URI is registered with an external
        // provider, so the path is not ours to change.
        // Public trigger for one RAG collection's re-sync. Point a file
        // host's webhook (Nextcloud's `webhook_listeners`, or a cron line) at
        // it. Unauthenticated by design: the token in the URL is the
        // credential.
        .with_post("/hooks/rag/{token}", pages::rag_sync_hook)
        // Public webhook trigger. GET and POST so simple senders and JSON
        // POSTers both work; the secret in the URL is the credential.
        .with_get("/hooks/{secret}", pages::webhook_trigger)
        .with_post("/hooks/{secret}", pages::webhook_trigger)
        // RAG source OAuth: `connect` sends the operator to the provider,
        // `callback` is the redirect URI they registered there.
        .with_get("/rag/{id}/connect", pages::rag_connect)
        .with_get("/rag/oauth/callback", pages::rag_oauth_callback)
        // Per-user MCP connector OAuth, same shape.
        .with_post("/integrations/{key}/connect", pages::integrations_connect)
        .with_post("/integrations/{key}/retry", pages::integrations_retry)
        .with_get("/integrations/callback", pages::integrations_callback)
        .with_get("/v1/models", proxy::list_models)
        // Catch-all param: model ids contain `/` (e.g.
        // `mistralai/Voxtral-Mini-4B-Realtime-2602`).
        .with_get("/v1/models/{*id}", proxy::retrieve_model)
        .with_post("/v1/chat/completions", proxy::chat_completions)
        .with_post("/v1/systemone", proxy::system_one)
        // Anthropic Messages format — what Claude Code speaks. Same pipeline
        // as `/v1/chat/completions` (routing, limits, tool loop, usage); only
        // the wire format differs. See `rama_server::messages`.
        // The more specific path first, matching how this router
        // disambiguates everywhere else.
        .with_post("/v1/messages/count_tokens", messages::count_tokens)
        .with_post("/v1/messages", messages::messages)
        .with_post("/v1/audio/transcriptions", proxy::transcribe)
        .with_post("/v1/audio/speech", proxy::speech)
        .with_post("/v1/embeddings", proxy::embeddings)
        .with_post("/v1/images/generations", proxy::images_generations)
        .with_post("/v1/images/edits", proxy::images_edits)
        // Bearer-authed download of a file a sandbox run produced for an
        // API caller (scoped to the caller's user; see `sandbox_api`).
        .with_get("/v1/sandbox/files/{run}/{filename}", sandbox_api::download)
        // Connection-warming probe an Anthropic-format client sends at
        // startup. Unauthenticated: it carries no request and reveals only
        // that something is listening.
        .with_head("/api/hello", messages::hello)
        .with_get("/auth/login", oidc_handlers::login)
        .with_get("/auth/callback", oidc_handlers::callback)
        .with_post("/auth/logout", oidc_handlers::logout)
        .with_get("/api/v0/me", api::me)
        .with_get("/api/v0/tokens", api::list_tokens)
        .with_post("/api/v0/tokens", api::create_token)
        .with_post("/api/v0/tokens/{id}/revoke", api::revoke_token)
        .with_post("/api/v0/tokens/{id}/rotate", api::rotate_token)
        .with_put("/api/v0/tokens/{id}/tools", api::update_token_tools)
        .with_delete("/api/v0/tokens/{id}", api::delete_token)
        .with_post("/api/v0/transcriptions", proxy::transcribe_session)
        .with_get("/api/v0/transcription_models", api::transcription_models)
        .with_post("/api/v0/speech", proxy::speech_session)
        .with_get("/api/v0/push/config", api::push_config)
        .with_post("/api/v0/push/subscribe", api::push_subscribe)
        .with_post("/api/v0/push/unsubscribe", api::push_unsubscribe)
        .with_post("/api/v0/me/timezone", api::set_timezone)
        .with_post("/api/v0/me/speech_voice", api::set_speech_voice)
        .with_post("/api/v0/me/location", api::set_location)
        .with_delete("/api/v0/me/location", api::clear_location)
        .with_post(
            "/api/v0/me/location/feedback/{turn_id}",
            api::location_feedback,
        )
        .with_post("/api/v0/me/ask/feedback/{turn_id}", api::ask_feedback)
        .with_post(
            "/api/v0/me/browser/feedback/{turn_id}",
            api::browser_feedback,
        )
        // Chat attachment bytes. Under /api/v0 like everything else the SPA
        // calls; the filename keeps its case (the handler reads the raw URI).
        .with_get(
            "/api/v0/chat/attachment/{turn_id}/{filename}",
            pages::chat_attachment,
        )
        // …and at the path the attachment markers actually carry. Those URLs
        // are written into turn content by `chat_attachments::proxy_url` and
        // parsed back by `session_core::attachments`, so they are a stored
        // data format: every marker already in a database points here, and
        // re-pointing them would mean a content migration. Registered BEFORE
        // the SPA catch-all, which would otherwise answer these with the app
        // shell at 200 text/html — a broken <img> and a "download" that saves
        // index.html under the attachment's name. Four segments, so the SPA's
        // own two-segment `/chat/{id}` route is untouched.
        .with_get(
            "/chat/attachment/{turn_id}/{filename}",
            pages::chat_attachment,
        )
        // Feedback widget: report a problem, and the config that tells the SPA
        // whether it is wired up at all.
        .with_get("/api/v0/feedback/config", pages::feedback_config)
        .with_post("/api/v0/feedback/extract", pages::feedback_extract)
        .with_post("/api/v0/feedback", pages::feedback_submit)
        .with_get("/api/v0/rag/providers", rag_api::list_providers)
        .with_post("/api/v0/rag/test-source", rag_api::test_source)
        .with_get("/api/v0/rag/profiles", rag_api::list_profiles)
        .with_post("/api/v0/rag/profiles", rag_api::create_profile)
        .with_put("/api/v0/rag/profiles/{name}", rag_api::update_profile)
        .with_delete("/api/v0/rag/profiles/{name}", rag_api::delete_profile)
        .with_get("/api/v0/rag/collections", rag_api::list_collections)
        .with_post("/api/v0/rag/collections", rag_api::create_collection)
        .with_get("/api/v0/rag/collections/{id}", rag_api::get_collection)
        .with_patch("/api/v0/rag/collections/{id}", rag_api::update_collection)
        .with_delete("/api/v0/rag/collections/{id}", rag_api::delete_collection)
        .with_post(
            "/api/v0/rag/collections/{id}/reindex",
            rag_api::reindex_collection,
        )
        .with_get("/api/v0/rag/collections/{id}/refs", rag_api::list_refs)
        .with_post("/api/v0/rag/collections/{id}/refs", rag_api::add_refs)
        .with_delete(
            "/api/v0/rag/collections/{id}/refs/{ref_id}",
            rag_api::delete_ref,
        )
        .with_patch(
            "/api/v0/rag/collections/{id}/refs/{ref_id}",
            rag_api::update_ref,
        )
        .with_get(
            "/api/v0/rag/collections/{id}/refs/{ref_id}/log",
            rag_api::ref_log,
        )
        .with_post(
            "/api/v0/rag/collections/{id}/refs/{ref_id}/rebuild",
            rag_api::rebuild_ref,
        )
        .with_post(
            "/api/v0/rag/collections/{id}/refs/{ref_id}/primary",
            rag_api::set_primary_ref,
        )
        .with_post(
            "/api/v0/rag/collections/{id}/sync-token",
            rag_api::rotate_sync_token,
        )
        .with_post(
            "/api/v0/rag/collections/{id}/sync-token/clear",
            rag_api::clear_sync_token,
        )
        .with_post("/api/v0/comfyui/reload", comfyui_api::reload)
        .with_get("/api/v0/comfyui/catalog", comfyui_api::catalog)
        .with_get("/api/v0/comfyui/health", comfyui_api::health)
        .with_get("/api/v0/models", api::chat_models)
        .with_get("/api/v0/usage", api::usage)
        // Admin JSON API for the SPA (issue #22 P4).
        .with_get("/api/v0/admin/groups", pages::json_admin::groups_list)
        .with_put("/api/v0/admin/groups", pages::json_admin::groups_save)
        .with_delete(
            "/api/v0/admin/groups/{name}",
            pages::json_admin::groups_delete,
        )
        .with_get("/api/v0/admin/users", pages::json_admin::users_list)
        .with_post(
            "/api/v0/admin/users/{id}/impersonate",
            pages::json_admin::users_impersonate,
        )
        .with_post(
            "/api/v0/admin/impersonate/stop",
            pages::json_admin::impersonate_stop,
        )
        .with_get("/api/v0/admin/models", pages::json_admin::models_list)
        .with_put("/api/v0/admin/models", pages::json_admin::models_save)
        .with_get(
            "/api/v0/admin/automatic-routes",
            pages::json_admin::automatic_routes_list,
        )
        .with_put(
            "/api/v0/admin/automatic-routes",
            pages::json_admin::automatic_routes_save,
        )
        .with_delete(
            "/api/v0/admin/automatic-routes/{*alias}",
            pages::json_admin::automatic_routes_delete,
        )
        .with_delete(
            "/api/v0/admin/models/{name}",
            pages::json_admin::models_delete,
        )
        .with_put(
            "/api/v0/admin/model-defaults",
            pages::json_admin::models_feature_default,
        )
        .with_put(
            "/api/v0/admin/search-settings",
            pages::json_admin::models_search_save,
        )
        .with_get("/api/v0/admin/limits", pages::json_admin::limits_list)
        .with_post("/api/v0/admin/limits", pages::json_admin::limits_save)
        .with_delete(
            "/api/v0/admin/limits/{id}",
            pages::json_admin::limits_delete,
        )
        .with_get("/api/v0/admin/settings", pages::json_admin::settings_list)
        .with_post("/api/v0/admin/settings", pages::json_admin::settings_save)
        .with_post(
            "/api/v0/admin/settings/clear",
            pages::json_admin::settings_clear,
        )
        .with_get("/api/v0/admin/tokens", pages::json_admin::tokens_list)
        .with_put(
            "/api/v0/admin/tokens/{id}/models",
            pages::json_admin::tokens_models,
        )
        .with_get("/api/v0/admin/upstreams", pages::json_admin::topology_list)
        .with_get(
            "/api/v0/admin/upstreams/events",
            pages::json_admin::topology_events,
        )
        .with_put("/api/v0/admin/backends", pages::json_admin::backends_save)
        .with_post(
            "/api/v0/admin/backends/test",
            pages::json_admin::backends_test,
        )
        .with_delete(
            "/api/v0/admin/backends/{name}",
            pages::json_admin::backends_delete,
        )
        .with_post(
            "/api/v0/admin/backends/{name}/enabled",
            pages::json_admin::backends_enabled,
        )
        .with_post(
            "/api/v0/admin/backends/{name}/rename",
            pages::json_admin::backends_rename,
        )
        .with_put("/api/v0/admin/pools", pages::json_admin::pools_save)
        .with_delete(
            "/api/v0/admin/pools/{name}",
            pages::json_admin::pools_delete,
        )
        .with_post(
            "/api/v0/admin/pools/{name}/rename",
            pages::json_admin::pools_rename,
        )
        .with_put(
            "/api/v0/admin/upstreams/fallback",
            pages::json_admin::topology_fallback,
        )
        .with_post(
            "/api/v0/admin/upstreams/reload",
            pages::json_admin::topology_reload,
        )
        // Workspace JSON API for the SPA (issue #22 P5).
        // Skills + connectors + integrations JSON (issue #22 P5).
        .with_get("/api/v0/skills", pages::json_skills::skills_list)
        .with_post("/api/v0/skills", pages::json_skills::skills_upload)
        .with_get("/api/v0/skills/{name}/body", pages::json_skills::skill_body)
        .with_get(
            "/api/v0/skills/{name}/archive",
            pages::json_skills::skill_archive,
        )
        .with_delete("/api/v0/skills/{name}", pages::json_skills::skills_delete)
        .with_get(
            "/api/v0/admin/skills",
            pages::json_skills::admin_skills_list,
        )
        .with_post(
            "/api/v0/admin/skills",
            pages::json_skills::admin_skills_upload,
        )
        .with_get(
            "/api/v0/admin/skills/{name}/archive",
            pages::json_skills::admin_skill_archive,
        )
        .with_delete(
            "/api/v0/admin/skills/{name}",
            pages::json_skills::admin_skills_delete,
        )
        .with_put(
            "/api/v0/admin/skills/grants",
            pages::json_skills::admin_skills_grants,
        )
        .with_get(
            "/api/v0/admin/connectors",
            pages::json_skills::admin_connectors_list,
        )
        .with_put(
            "/api/v0/admin/connectors",
            pages::json_skills::admin_connectors_save,
        )
        .with_post(
            "/api/v0/admin/connectors/restore-defaults",
            pages::json_skills::admin_connectors_restore_defaults,
        )
        .with_post(
            "/api/v0/admin/connectors/{key}/toggle",
            pages::json_skills::admin_connectors_toggle,
        )
        .with_get(
            "/api/v0/admin/connectors/{key}/audit",
            pages::json_skills::admin_connector_audit,
        )
        .with_delete(
            "/api/v0/admin/connectors/{key}",
            pages::json_skills::admin_connectors_delete,
        )
        .with_get(
            "/api/v0/integrations",
            pages::json_skills::integrations_list,
        )
        .with_post(
            "/api/v0/integrations/{key}/token",
            pages::json_skills::integrations_connect_token,
        )
        .with_post(
            "/api/v0/integrations/{key}/disconnect",
            pages::json_skills::integrations_disconnect,
        )
        .with_post(
            "/api/v0/integrations/{key}/retry",
            pages::json_skills::integrations_retry_json,
        )
        .with_post(
            "/api/v0/integrations/{key}/tools/mode",
            pages::json_skills::integrations_tool_mode,
        )
        .with_post(
            "/api/v0/integrations/{key}/tools/all",
            pages::json_skills::integrations_tools_all,
        )
        .with_get("/api/v0/setup/state", setup_api::setup_state)
        .with_post("/api/v0/setup/test", setup_api::setup_test)
        .with_post("/api/v0/setup/restart", setup_api::setup_restart)
        .with_post("/api/v0/setup/finish", setup_api::setup_finish)
        .with_get("/api/v0/memories", pages::json_workspace::memories_list)
        .with_post("/api/v0/memories", pages::json_workspace::memories_create)
        .with_put(
            "/api/v0/memories/{id}",
            pages::json_workspace::memories_update,
        )
        .with_delete(
            "/api/v0/memories/{id}",
            pages::json_workspace::memories_delete,
        )
        .with_get("/api/v0/scheduled", pages::json_workspace::scheduled_list)
        .with_post("/api/v0/scheduled", pages::json_workspace::scheduled_create)
        .with_post(
            "/api/v0/scheduled/preview",
            pages::json_workspace::scheduled_preview,
        )
        .with_put(
            "/api/v0/scheduled/{id}",
            pages::json_workspace::scheduled_update,
        )
        .with_post(
            "/api/v0/scheduled/{id}/toggle",
            pages::json_workspace::scheduled_toggle,
        )
        .with_delete(
            "/api/v0/scheduled/{id}",
            pages::json_workspace::scheduled_delete,
        )
        .with_get(
            "/api/v0/scheduled/{id}/runs",
            pages::json_workspace::scheduled_runs,
        )
        .with_get("/api/v0/webhooks", pages::json_workspace::webhooks_list)
        .with_post("/api/v0/webhooks", pages::json_workspace::webhooks_create)
        .with_put(
            "/api/v0/webhooks/{id}",
            pages::json_workspace::webhooks_update,
        )
        .with_post(
            "/api/v0/webhooks/{id}/toggle",
            pages::json_workspace::webhooks_toggle,
        )
        .with_post(
            "/api/v0/webhooks/{id}/rotate",
            pages::json_workspace::webhooks_rotate,
        )
        .with_delete(
            "/api/v0/webhooks/{id}",
            pages::json_workspace::webhooks_delete,
        )
        .with_get(
            "/api/v0/webhooks/{id}/runs",
            pages::json_workspace::webhooks_runs,
        )
        .with_post(
            "/api/v0/webhooks/{id}/rerun",
            pages::json_workspace::webhooks_rerun,
        )
        .with_get("/api/v0/tools", pages::tools::tools_list_json)
        .with_post("/api/v0/tools/toggle", pages::tools::tools_toggle_json)
        // Chat JSON API for the SvelteKit SPA (issue #22 phase 2). The
        // legacy form/SSE-HTML chat routes under `/chat/*` stay alive
        // beside these until phase 6.
        .with_get(
            "/api/v0/chat/landing",
            pages::chat::json_api::session_landing,
        )
        .with_get(
            "/api/v0/chat/sessions",
            pages::chat::json_api::sessions_list,
        )
        .with_post(
            "/api/v0/chat/sessions",
            pages::chat::json_api::session_create,
        )
        .with_get(
            "/api/v0/chat/sessions/{id}",
            pages::chat::json_api::session_get,
        )
        .with_delete(
            "/api/v0/chat/sessions/{id}",
            pages::chat::json_api::session_delete,
        )
        .with_post(
            "/api/v0/chat/sessions/{id}/pin",
            pages::chat::json_api::session_pin,
        )
        .with_post(
            "/api/v0/chat/sessions/{id}/messages",
            pages::chat::json_api::message_send,
        )
        .with_post(
            "/api/v0/chat/sessions/{id}/steer",
            pages::chat::json_api::session_steer,
        )
        .with_post(
            "/api/v0/chat/sessions/{id}/steer/{steer_id}/discard",
            pages::chat::json_api::steer_discard,
        )
        .with_post(
            "/api/v0/chat/sessions/{id}/cancel",
            pages::chat::json_api::session_cancel,
        )
        .with_get(
            "/api/v0/chat/sessions/{id}/events",
            pages::chat::json_api::session_events,
        )
        .with_post(
            "/api/v0/chat/sessions/{id}/share",
            pages::chat::json_api::session_share,
        )
        .with_post(
            "/api/v0/chat/sessions/{id}/effort",
            pages::chat::json_api::session_effort,
        )
        .with_get(
            "/api/v0/chat/sessions/{id}/capabilities",
            pages::chat::json_api::capabilities_list,
        )
        .with_post(
            "/api/v0/chat/sessions/{id}/capabilities",
            pages::chat::json_api::capabilities_set,
        )
        .with_put(
            "/api/v0/tokens/{id}/models",
            pages::chat::json_api::owner_token_models,
        )
        .with_get("/api/v0/tokens/details", pages::json_tokens::details)
        .with_post(
            "/api/v0/tokens/{id}/quota",
            pages::chat::json_api::owner_token_quota,
        )
        .with_put(
            "/api/v0/tokens/{id}/mcp-policy",
            pages::chat::json_api::owner_token_mcp_policy,
        )
        .with_delete(
            "/api/v0/tokens/{id}/quota/{rule_id}",
            pages::chat::json_api::owner_token_quota_delete,
        )
        .with_get(
            "/api/v0/chat/sessions/{id}/export.md",
            pages::chat::json_api::session_export_markdown,
        )
        .with_get(
            "/api/v0/chat/sessions/{id}/export.pdf",
            pages::chat::json_api::session_export_pdf,
        )
        .with_post(
            "/api/v0/chat/sessions/{id}/fork",
            pages::chat::json_api::session_fork,
        )
        // Canvas documents. The static `/documents` list must precede the
        // `{doc_id}` form — rama matches in registration order, so the
        // parameterised route would otherwise swallow it.
        .with_get(
            "/api/v0/chat/sessions/{id}/documents",
            pages::chat::json_api::documents_list,
        )
        .with_get(
            "/api/v0/chat/sessions/{id}/documents/{doc_id}",
            pages::chat::json_api::document_get,
        )
        .with_put(
            "/api/v0/chat/sessions/{id}/documents/{doc_id}",
            pages::chat::json_api::document_edit,
        )
        .with_delete(
            "/api/v0/chat/sessions/{id}/turns/{turn_id}/attachments/{filename}",
            pages::chat::json_api::attachment_remove,
        )
        .with_delete(
            "/api/v0/chat/sessions/{id}/turns/{turn_id}",
            pages::chat::json_api::turn_delete,
        )
        .with_post(
            "/api/v0/chat/sessions/{id}/turns/{turn_id}/retry",
            pages::chat::json_api::turn_retry,
        )
        .with_post(
            "/api/v0/chat/sessions/{id}/turns/{turn_id}/edit",
            pages::chat::json_api::turn_edit,
        );
    // Debug-only dev/e2e seeding. Registered before the SPA catch-all (which
    // must remain the last routes) and absent from release binaries entirely —
    // see `rama_server::dev_seed` for why there are two endpoints.
    #[cfg(debug_assertions)]
    let router = router
        .with_get("/__dev/seed-session", dev_seed::reset)
        .with_get("/__dev/session", dev_seed::sign_in);
    router
        // The SPA owns the root now that the server-rendered pages are gone.
        //
        // MUST be the LAST routes: `{*name}` is a catch-all, and this router
        // matches in registration order, so anything registered after it would
        // be unreachable. Two registrations because they are distinct shapes
        // to the matcher: `/` (the entry point) and everything below it. Every
        // path that is not a real file falls back to `index.html` so the
        // client router can resolve it — see `rama_server::spa`.
        .with_get("/", spa::spa_get)
        .with_get("/{*name}", spa::spa_get)
}

/// The complete HTTP service: the router plus the layers that make it
/// servable. rc1's `Router` is not `Clone` and surfaces `RouterError`,
/// while `HttpServer::listen` wants a `Clone` service whose error is
/// `Infallible` — `ArcLayer` makes the router shareable/cloneable and
/// `ErrorHandlerLayer` renders any `RouterError` (e.g. an unmatched path)
/// into a `Response`. Both `serve` and the tests build the service through
/// here so they exercise the same stack (notably the 404 handling).
///
/// `V1CorsLayer` sits *outside* the error handler so it decorates every
/// `/v1` response — including the 404/405 a `RouterError` renders (a
/// browser preflight `OPTIONS /v1/…` hits no registered route) and the
/// bearer-auth 401 — with the CORS headers browser SPAs require. It is
/// scoped to `/v1`, so the same-origin HTML UI and `/api/v0` are untouched.
///
/// `FirstRunLayer` sits outside the error handler for a related reason: on an
/// unconfigured gateway an unmatched path should land on the setup wizard, not
/// on a 404 that explains nothing. It is a no-op once setup has completed. See
/// [`first_run`](crate::rama_server::first_run) for why the gate lives here
/// rather than in the handlers.
pub fn service(
    state: Arc<RamaState>,
) -> impl rama::Service<
    rama::http::Request,
    Output = rama::http::Response,
    Error = std::convert::Infallible,
> + Clone {
    let first_run = FirstRunLayer::new(state.clone());
    let router = router(state);
    (
        V1CorsLayer,
        first_run,
        ArcLayer::new(),
        ErrorHandlerLayer::default(),
    )
        .into_layer(router)
}

/// How long a shutdown waits for open connections to drain before it stops
/// waiting on them. Streaming responses (the chat SSE tail, a `/v1` stream)
/// are the ones this is for.
const CONNECTION_DRAIN: Duration = Duration::from_secs(15);

/// How long, after connections are done, to wait for assistant turns still
/// running in background tasks.
///
/// A turn outlives its HTTP connection by design — that is what makes
/// resume-on-reconnect work — so draining connections does not drain work.
/// Without this wait, every deploy kills turns mid-write and the next boot
/// sweeps them to `errored` (see `sweep_in_progress_at_startup`, which exists
/// precisely because this used to be the only outcome).
///
/// Deliberately shorter than a typical orchestrator stop grace (systemd's
/// `TimeoutStopSec` defaults to 90s): exceeding that buys nothing, because the
/// SIGKILL lands anyway and we are back to the behaviour this replaces.
const TURN_DRAIN: Duration = Duration::from_secs(45);

/// Polling interval while waiting for turns to finish.
const TURN_POLL: Duration = Duration::from_millis(250);

/// Convenience: build the service and start serving on `addr`, shutting down
/// gracefully on SIGINT / SIGTERM.
pub async fn serve(state: Arc<RamaState>, addr: SocketAddress) -> anyhow::Result<()> {
    // `Shutdown::default()` installs the platform's usual signal handling
    // (SIGINT + SIGTERM); the guard handed to the task is what `listen`
    // threads into each connection so a signal stops accepting and lets
    // in-flight requests finish.
    let graceful = rama::graceful::Shutdown::default();
    let chats = state.chats.clone();
    let svc = service(state);

    // A listen failure has to reach the caller, not just the log. `listen` runs
    // inside a graceful task now, so its error no longer propagates out of
    // `serve` on its own — and `shutdown_with_limit` waits for a *signal*
    // before it waits for guards. Without this channel a bind failure (port
    // taken, bad bind address) leaves the process alive and waiting forever
    // with nothing listening, which an orchestrator reads as a healthy unit.
    let (listen_err_tx, listen_err_rx) = tokio::sync::oneshot::channel::<String>();
    graceful.spawn_task_fn(async move |guard| {
        let exec = Executor::graceful(guard);
        if let Err(err) = HttpServer::auto(exec).listen(addr, svc).await {
            tracing::error!(error = %err, "rama listen");
            let _ = listen_err_tx.send(err.to_string());
        }
    });

    tokio::select! {
        drained = graceful.shutdown_with_limit(CONNECTION_DRAIN) => {
            // A drain *timeout* is not a reason to skip the turn drain below —
            // it is the reason to run it. The chat SSE tail holds its
            // connection open for the whole turn, so an in-flight turn is
            // exactly the case that overruns this budget, and returning here
            // would kill it mid-write: the outcome `drain_turns` exists to
            // prevent.
            if let Err(err) = drained {
                tracing::warn!(
                    error = %err,
                    "connections still open at the drain deadline; draining turns anyway"
                );
            }
        }
        Ok(err) = listen_err_rx => {
            // Never served a request; nothing to drain.
            return Err(anyhow::anyhow!("rama listen: {err}"));
        }
    }

    drain_turns(&chats, TURN_DRAIN, TURN_POLL).await;
    Ok(())
}

/// Wait for in-flight assistant turns, then ask any stragglers to stop.
///
/// Two phases on purpose. A turn that finishes on its own writes a complete
/// row; one that is cancelled writes a terminal row saying so. Both beat being
/// killed mid-write, and the difference between them is only how long we were
/// willing to wait.
async fn drain_turns(chats: &session_core::SessionWorkers, budget: Duration, poll: Duration) {
    let active = chats.active_count();
    if active == 0 {
        return;
    }
    tracing::info!(
        active,
        drain_secs = budget.as_secs(),
        "waiting for in-flight assistant turns before exit"
    );
    let deadline = Instant::now() + budget;
    while Instant::now() < deadline {
        if chats.active_count() == 0 {
            tracing::info!("all in-flight turns finished; exiting cleanly");
            return;
        }
        tokio::time::sleep(poll).await;
    }
    // Out of budget: ask them to stop at their next checkpoint and give that
    // a brief moment to land, so the turn row is finalised by the turn itself
    // rather than swept as an orphan on the next boot.
    let remaining = chats.cancel_all();
    tracing::warn!(
        remaining,
        "turns still running at the drain deadline; cancelling them"
    );
    let grace = Instant::now() + (poll * 12);
    while Instant::now() < grace && chats.active_count() > 0 {
        tokio::time::sleep(poll).await;
    }
    let stuck = chats.active_count();
    if stuck > 0 {
        tracing::warn!(
            stuck,
            "exiting with turns still running; they will be swept on next boot"
        );
    }
}

#[cfg(test)]
mod shutdown_tests {
    use super::*;
    use session_core::{RegisterOutcome, SessionWorkers};

    /// The smallest `RamaState` that can be served: in-memory database, no
    /// upstreams, no tools. Enough for the tests here, which never route a
    /// request — they exercise `serve`'s own lifecycle.
    async fn minimal_state() -> RamaState {
        use aiplane_core::server::rbac::Resolver;
        use aiplane_core::server::{Config, db, upstreams};
        use aiplane_runtime::server::AppState;
        use aiplane_runtime::server::tools::ToolRegistry;

        let db_pool = db::open(std::path::Path::new(":memory:")).await.unwrap();
        let registry = upstreams::UpstreamRegistry::new(&std::collections::HashMap::new()).unwrap();
        let app = AppState::new(
            Config::default(),
            db_pool.clone(),
            registry,
            Arc::new(ToolRegistry::new()),
            Arc::new(Resolver::empty()),
        );
        RamaState::new(
            app,
            crate::rama_server::SessionStore::new(db_pool, [3u8; 32]),
            aiplane_core::server::usage::UsageHandle::disabled(),
        )
    }

    /// An idle gateway exits immediately — the common deploy must not sit out
    /// a drain budget waiting for work that isn't there.
    #[tokio::test]
    async fn draining_an_idle_gateway_returns_at_once() {
        let chats = SessionWorkers::default();
        let start = Instant::now();
        drain_turns(&chats, Duration::from_secs(30), Duration::from_millis(10)).await;
        assert!(
            start.elapsed() < Duration::from_millis(200),
            "idle drain took {:?}",
            start.elapsed()
        );
    }

    /// A turn that finishes on its own releases the shutdown as soon as it
    /// does — the wait is bounded *above* by the budget, not equal to it.
    #[tokio::test]
    async fn a_finishing_turn_releases_the_drain_early() {
        let chats = std::sync::Arc::new(SessionWorkers::default());
        let RegisterOutcome::Registered { worker } = chats.register("u1", "t1", "s1", 1) else {
            panic!("registered");
        };
        let bg = chats.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(80)).await;
            bg.clear("u1", &worker);
        });

        let start = Instant::now();
        drain_turns(&chats, Duration::from_secs(30), Duration::from_millis(10)).await;
        let waited = start.elapsed();
        assert!(
            waited >= Duration::from_millis(70),
            "returned too early: {waited:?}"
        );
        assert!(
            waited < Duration::from_secs(5),
            "waited the whole budget instead of noticing the turn finished: {waited:?}"
        );
    }

    /// A listener that never comes up must fail the process, not leave it
    /// waiting for a shutdown signal with nothing bound. An orchestrator reads
    /// a still-running unit as healthy, so this is the difference between a
    /// crash-loop it can act on and a silent outage.
    #[tokio::test]
    async fn a_bind_failure_returns_an_error_instead_of_hanging() {
        // Hold the port so the gateway's own bind is guaranteed to fail.
        let squatter = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = squatter.local_addr().unwrap();

        let state = Arc::new(minimal_state().await);
        let served = tokio::time::timeout(
            Duration::from_secs(10),
            serve(state, SocketAddress::from(addr)),
        )
        .await;

        match served {
            Err(_) => panic!("serve() hung on a failed bind instead of returning"),
            Ok(Ok(())) => panic!("serve() reported success without ever listening"),
            Ok(Err(err)) => assert!(
                err.to_string().contains("listen"),
                "unexpected error: {err}"
            ),
        }
    }

    /// A turn that overruns the budget is asked to stop rather than being
    /// killed mid-write — that is the difference between a finalised row and
    /// one swept as `errored` on the next boot.
    #[tokio::test]
    async fn an_overrunning_turn_is_cancelled_at_the_deadline() {
        let chats = SessionWorkers::default();
        let RegisterOutcome::Registered { worker } = chats.register("u1", "t1", "s1", 1) else {
            panic!("registered");
        };
        assert!(!worker.cancel.load(std::sync::atomic::Ordering::SeqCst));

        drain_turns(&chats, Duration::from_millis(50), Duration::from_millis(10)).await;

        assert!(
            worker.cancel.load(std::sync::atomic::Ordering::SeqCst),
            "a straggler must be signalled to stop at its next checkpoint"
        );
    }
}
