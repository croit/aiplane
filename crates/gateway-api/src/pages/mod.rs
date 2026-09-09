// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The `/api/v0` JSON handlers the SvelteKit SPA calls.
//!
//! Despite the module name, nothing here renders a page any more: issue #22
//! replaced the server-rendered UI with the SPA in `web/`, and what survived
//! the teardown are the JSON endpoints plus the handful of genuinely
//! server-rendered surfaces the SPA cannot own — the OAuth callback pages in
//! `rag_oauth` and `integrations`, which a provider redirects a browser to
//! before any SPA route exists.
//!
//! Shared helpers for that surface live here: the auth gates
//! ([`require_session_json`], [`require_admin_json`]), the error envelope
//! ([`json_error`]) and [`raw_path_segment`], which exists because rama's
//! `Path` extractor mangles case-sensitive ids.

use rama::http::{Method, Request, Response, StatusCode, header};

use session_core::chrome::see_other;

use gateway_core::rama_server::session::Session;
use gateway_core::server::db::users;
use gateway_runtime::rama_server::state::RamaState;

/// Resolve the caller's session or bail out of the handler. Expands to the
/// `require_session_or_redirect` match that early-`return`s the redirect
/// `Response` on failure — replacing the ~45 hand-written copies of that
/// 4-line block across the page handlers. The binding stays at the call site,
/// so every shape works:
/// `let (session, user) = require_session!(state, req);`
/// `let (_, user) = require_session!(state, req);`
///
/// Defined before the `mod …;` page declarations below so textual macro
/// scoping makes it available in every page submodule without an import.
/// `require_session_or_redirect` resolves in the caller's scope (each handler
/// already has it in scope), so no extra `use` is needed there either.
macro_rules! require_session {
    ($state:expr, $req:expr) => {
        match require_session_or_redirect(&$state, &$req).await {
            Ok(s) => s,
            Err(resp) => return resp,
        }
    };
}

/// The JSON twin of [`require_session!`]: resolve the caller or `return` the
/// 401 envelope. Binding stays at the call site, so both shapes work:
/// `let (session, user) = require_session_json!(state, req);`
/// `let (_, user) = require_session_json!(state, req);`
macro_rules! require_session_json {
    ($state:expr, $req:expr) => {
        match crate::pages::require_session_json(&$state, &$req).await {
            Ok(v) => v,
            Err(resp) => return resp,
        }
    };
}

/// Admin gate, same shape. A non-admin gets 403, not a redirect — an API
/// caller has nowhere to be redirected to.
macro_rules! require_admin_json {
    ($state:expr, $req:expr) => {
        match crate::pages::require_admin_json(&$state, &$req).await {
            Ok(v) => v,
            Err(resp) => return resp,
        }
    };
}

/// True when the user holds any role flagged `admin = true` in config.
/// Used to gate `/admin/*` routes and conditionally render the Admin
/// sidebar entry.
///
/// `user.roles` holds the raw OIDC group claims (e.g. `"engineering"`,
/// `"platform-admins"`). We translate through the RBAC resolver to the
/// internal role IDs, then ask the resolver whether any of them carries
/// the admin capability — the role name is irrelevant.
pub(super) fn is_admin(state: &RamaState, user: &users::User) -> bool {
    let role_ids = state.rbac.role_ids_for(&user.roles);
    state.rbac.is_admin(&role_ids)
}

/// Admin gate. Wraps `require_session_or_redirect` + checks the
/// `admin` role. Anonymous → /login redirect (standard
/// not-logged-in flow); logged-in-but-not-admin → 403 page (don't
/// bounce them to /login, they'd just loop). Returns the user on
/// success so the caller doesn't have to look it up again.
pub(super) async fn require_admin_or_403(
    state: &RamaState,
    req: &Request,
) -> Result<(Session, users::User), Response> {
    let (session, user) = require_session_or_redirect(state, req).await?;
    if !is_admin(state, &user) {
        return Err(flow_error_page(
            StatusCode::FORBIDDEN,
            "admin role required",
        ));
    }
    Ok((session, user))
}

/// One path segment, read off the **raw** request URI, counted from the end.
///
/// The `Path` extractor cannot be used wherever the value's exact bytes
/// matter. rama's router matches on `uri.path().to_lowercase()` and the
/// `UriParams` it hands the extractor come from that lowercased string, and it
/// never percent-decodes them. So a case-sensitive OIDC subject, a model id
/// like `Qwen/Qwen3-32B` (whose `/` the client sends as `%2F`), a skill name
/// or a pool name all arrive mangled — and because the lookups they feed
/// simply match nothing, the endpoints answer "not found" or, worse, report
/// success for a row they never touched.
///
/// `from_end` is 0 for the last segment, 1 for the one before it, and so on;
/// counting from the end keeps a caller independent of the route's prefix.
/// Returns `None` when there is no such segment or it is empty.
pub(crate) fn raw_path_segment(req: &Request, from_end: usize) -> Option<String> {
    let path = req.uri().path();
    let segment = path.rsplit('/').nth(from_end)?;
    if segment.is_empty() {
        return None;
    }
    Some(percent_decode_segment(segment))
}

/// Percent-decode one path segment. `+` stays a literal plus (a path is not a
/// form body), and a malformed escape passes through untouched rather than
/// eating the characters after it.
pub(crate) fn percent_decode_segment(s: &str) -> String {
    if !s.contains('%') {
        return s.to_string();
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            )
        {
            out.push((hi * 16 + lo) as u8);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Auth gate that redirects to /login on miss (vs the API gate which
/// returns 401 JSON). Returns either the resolved session or the
/// redirect Response that the caller should `return`.
pub(super) async fn require_session_or_redirect(
    state: &RamaState,
    req: &Request,
) -> Result<(Session, users::User), Response> {
    // No first-run check here: `rama_server::first_run` gates the whole HTML
    // surface before routing, so an unconfigured gateway never reaches a
    // handler that would call this. Repeating it per-handler is what let
    // `/auth/login` slip through.
    let session = match state.sessions.lookup_from_headers(req.headers()).await {
        Ok(Some(s)) => s,
        Ok(None) => return Err(login_redirect(req)),
        Err(err) => {
            tracing::warn!(error = %err, "session lookup");
            return Err(login_redirect(req));
        }
    };
    match users::find_by_id(&state.db, &session.user_id).await {
        Ok(Some(u)) => Ok((session, u)),
        Ok(None) | Err(_) => Err(login_redirect(req)),
    }
}

/// The JSON twin of [`require_session_or_redirect`]: same lookup, but a
/// miss is a 401 with the error envelope a fetch/EventSource client
/// understands, not a 303 to an HTML login page. Shape-matched with
/// `gateway::rama_server::api`'s envelope — one `/api/v0` contract.
pub(crate) async fn require_session_json(
    state: &RamaState,
    req: &Request,
) -> Result<(Session, users::User), Response> {
    let session = match state.sessions.lookup_from_headers(req.headers()).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            return Err(json_error(
                rama::http::StatusCode::UNAUTHORIZED,
                "unauthorized",
                "no active session — sign in at /auth/login",
            ));
        }
        Err(err) => {
            tracing::warn!(error = %err, "session lookup");
            return Err(json_error(
                rama::http::StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "session lookup failed",
            ));
        }
    };
    match users::find_by_id(&state.db, &session.user_id).await {
        Ok(Some(u)) => Ok((session, u)),
        Ok(None) => Err(json_error(
            rama::http::StatusCode::UNAUTHORIZED,
            "unauthorized",
            "no active session — sign in at /auth/login",
        )),
        Err(_) => Err(json_error(
            rama::http::StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "user lookup failed",
        )),
    }
}

/// A JSON error response in the `/api/v0` envelope
/// (`{"error":{"message","type","code"}}`). Mirrors
/// `gateway::rama_server::api::error_envelope` — the SPA sees one
/// contract, so the two must stay in sync.
/// The admin twin of [`require_session_json`]: session + admin role, with
/// JSON envelopes (401 / 403) instead of HTML.
pub(crate) async fn require_admin_json(
    state: &RamaState,
    req: &Request,
) -> Result<(Session, users::User), Response> {
    let (session, user) = require_session_json(state, req).await?;
    if !is_admin(state, &user) {
        return Err(json_error(
            rama::http::StatusCode::FORBIDDEN,
            "forbidden",
            "admin role required",
        ));
    }
    Ok((session, user))
}

pub(crate) fn json_ok(status: rama::http::StatusCode, body: serde_json::Value) -> Response {
    use rama::http::header;
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(body.to_string().into())
        .expect("static JSON response")
}

pub(crate) fn json_error(status: rama::http::StatusCode, code: &str, message: &str) -> Response {
    use rama::http::header;
    let body = serde_json::json!({
        "error": {
            "message": message,
            "type": code,
            "code": code,
        }
    });
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(body.to_string().into())
        .expect("static JSON response")
}

// ---------------------------------------------------------------------------
// Shared shapes for the `/api/v0` handlers.

/// 400 with the error envelope.
pub(crate) fn bad_request(message: impl Into<String>) -> Response {
    json_error(StatusCode::BAD_REQUEST, "invalid_request", &message.into())
}

/// 500 with the error envelope. Takes anything printable so a `Display` error
/// can go straight in.
pub(crate) fn internal(message: impl std::fmt::Display) -> Response {
    json_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal_error",
        &message.to_string(),
    )
}

/// 404 with the error envelope.
pub(crate) fn not_found(message: impl Into<String>) -> Response {
    json_error(StatusCode::NOT_FOUND, "not_found", &message.into())
}

/// 204, for a handler whose success carries no body.
pub(crate) fn no_content() -> Response {
    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(rama::http::Body::empty())
        .expect("static empty response")
}

/// Drain a request body and deserialize it, or hand back the 400 to return.
///
/// `what` names the payload for the error message — "the action body", "the
/// token body" — because "invalid JSON" on its own tells a caller nothing
/// about which of several bodies an endpoint accepts was wrong.
///
/// Takes the body rather than the `Request` because most handlers need the
/// request first (the auth gate borrows it, and anything reading a raw path
/// segment or a header must do so before `into_parts` consumes it).
pub(crate) async fn read_json<T: serde::de::DeserializeOwned>(
    body: rama::http::Body,
    what: &str,
) -> Result<T, Response> {
    let bytes = session_core::chrome::read_body_to_bytes(body)
        .await
        .map_err(bad_request)?;
    serde_json::from_slice(&bytes).map_err(|err| bad_request(format!("parsing {what}: {err}")))
}

/// Bounce an unauthenticated request to `/login`, preserving the originally
/// requested URL as `?return_to=…` so a deep link — e.g. a shared chat handed
/// to a colleague who isn't signed in yet — survives the OIDC round-trip
/// instead of dumping the user on the default surface (`/chat`, i.e. *their*
/// latest/new conversation). Only GETs to same-origin paths are carried; a
/// non-GET (no point replaying a POST after login) or an odd target falls back
/// to a bare `/login`. `/auth/login` + the callback re-validate `return_to` and
/// only honour same-origin `/`-paths, so this can't become an open redirect.
fn login_redirect(req: &Request) -> Response {
    if req.method() == Method::GET
        && let Some(path_and_query) = req.uri().path_and_query().map(|pq| pq.as_str())
        && gateway_core::rama_server::session::is_safe_return_to(path_and_query)
        && !path_and_query.starts_with("/login")
        && let Ok(query) = serde_urlencoded::to_string([("return_to", path_and_query)])
    {
        return see_other(&format!("/login?{query}"));
    }
    see_other("/login")
}

// `theme_toggle` lives in `session_core::chrome::theme_toggle`; the
// router mounts it directly.

// ---------------------------------------------------------------------------
// Chat
//
// Composer, message-send + tail SSE endpoints, tool-call loop, and the bubble
// renderers all live in `chat.rs`. We pub-re-export the four handler
// entry points so the router (which calls `pages::chat_index` etc.)
// doesn't have to know about the split.
pub mod chat;
pub use chat::chat_attachment;

// ---------------------------------------------------------------------------
// Tokens
//
// CRUD handlers, the list + row + minted-banner renderers all live in
// `tokens.rs`. Re-export the four handler entry points so the router
// continues to call `pages::tokens_index` etc. without any change.
// Reusable tool on/off toggle list shared by /tools and the /tokens
// per-token panel (`tool_toggles`). The resolver helpers are re-exported
// so the JSON token API can validate toggle keys against the same source.
mod tool_toggles;
pub use tool_toggles::{entries_for_roles, valid_keys};

// ---------------------------------------------------------------------------
// Tools
//
// Per-user tool on/off page (`/tools` + `/tools/toggle`). Available to
// every signed-in user; the list is scoped to the tools their roles
// grant. Re-export the two handler entry points for the router.
pub mod json_admin;
pub mod json_skills;
pub mod json_workspace;
pub mod tools;

// ---------------------------------------------------------------------------
// Memory
//
// Per-user memory management page (`/memory` + create/edit/delete).
// Available to every signed-in user; the assistant-facing side is the
// `remember` / `recall` tools (see `server::tools::memory`).

// ---------------------------------------------------------------------------
// Scheduled actions
//
// Per-user prompts that run on a cron schedule (`/scheduled` + create /
// update / toggle / delete / preview, plus the edit sub-page). Available
// to every signed-in user; scoped to the owner in the data layer. The
// firing loop lives in `server::scheduled::worker`.

// ---------------------------------------------------------------------------
// Webhooks
//
// Per-user prompts fired by an inbound HTTP call (`/webhooks` + create /
// update / toggle / rotate / delete, plus the edit sub-page). The public
// trigger `webhook_trigger` (on `/hooks/{secret}`) has no session — the
// secret in the URL is the credential. Available to every signed-in user;
// scoped to the owner in the data layer.
mod webhooks;
pub use webhooks::webhook_trigger;

// Per-user MCP connector store (`/integrations`). OAuth connect/callback +
// per-tool permissions. The admin-managed catalog lives in `connectors`.
mod integrations;
pub use integrations::{integrations_callback, integrations_connect, integrations_retry};

// Admin-managed MCP connector catalog (`/admin/connectors`).

// ---------------------------------------------------------------------------
// Admin (model defaults, future operator tooling). Gated on the
// `admin` role at the handler entry; non-admins never see the
// sidebar entry either.
pub(crate) mod admin;

// Merged upstream pools + backends page (`/admin/upstreams`). The GET page
// lives here; the old `/admin/pools` + `/admin/backends` GET routes 302 here.
// Same `admin`-role gate as the model-defaults page.

// Backend CRUD write handlers (paths `/admin/backends/*`, unchanged); the page
// they back is now `/admin/upstreams`. Same `admin`-role gate.

// Pool CRUD write handlers (paths `/admin/pools/*`, unchanged); the page they
// back is now `/admin/upstreams`. Same `admin`-role gate.

// Admin rate-limit / quota editor (`/admin/limits`). Same admin gate.

// Admin skills viewer + manager (`/admin/skills`, upload, delete, grants).
// Same admin gate.

// Per-user private skills page (`/skills`, upload, save, delete, download).
// Signed-in-user gate (not admin) — each user manages their own bundles.

// `/admin/comfyui` — operator viewer for the headless ComfyUI workflow
// catalog (live snapshot + reload trigger). Same admin gate as the other
// operator pages.

// Admin RAG-collections CRUD (`/rag`). Same admin gate.
mod rag;
// The source-kind picker + provider field sets, rendered from each
// provider's own declared config fields (see `rag_source`).
mod rag_oauth;
pub use rag::rag_sync_hook;
pub use rag_oauth::{rag_connect, rag_oauth_callback};

// The deployment setup wizard (`/setup`). Unlike every other page here it runs
// with no session and, on a first run, no configuration at all — its own access
// rules live in the module.

// The operator-settings editor (`/admin/settings`) — the twelve config blocks
// that moved into the database. Rendered from the spec table in
// `gateway_core::server::settings`, not hand-built per block.

// Admin gateway-groups editor (`/admin/groups`) — OIDC→group mappings + per-group
// tool/skill grants. Same admin gate.

// Admin user roster + impersonation (`/admin/users`, `/admin/users/impersonate`)
// plus the un-gated `/impersonate/stop`. Roster + start are admin-only; stop is
// reachable by the impersonated (possibly non-admin) session so it can get back.

// Deployment-wide API-token register (`/admin/tokens`) — every token with its
// owner, month-to-date spend, model allowlist and quota. Read-only: the
// plaintext is unrecoverable by construction, and the write paths stay on the
// owner's own `/tokens` page.

// Usage statistics: `/usage` for every signed-in user (scoped to their own
// requests), with an admin-only in-page "All users" toggle (`?scope=all`).

// Feedback widget: a floating button on every authed page that files a
// GitHub issue (with optional voice-to-fields + a viewport screenshot). The
// FAB + dialog are static chrome mounted in `layout_authed`; the three JSON
// endpoints are re-exported for the router.
mod feedback;
pub use feedback::{feedback_config, feedback_extract, feedback_submit};

// `read_body_to_bytes` lives in `session_core::chrome::read_body_to_bytes`.

/// A standalone error page for the OAuth round-trips.
///
/// These are the only responses left that a browser renders as a document:
/// the user arrives from a provider's redirect, mid-flow, with no app shell
/// loaded. So it is a whole page rather than a fragment — and deliberately a
/// self-contained one, with inline styling and a link back into the SPA,
/// instead of the app layout it used to borrow. The message says what failed
/// and where to resume; there is nothing actionable to render beyond that.
pub(super) fn flow_error_page(status: StatusCode, message: &str) -> Response {
    let escaped = message
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let body = format!(
        "<!doctype html>\n\
         <html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>Connection failed — LLM Gateway</title></head>\
         <body style=\"font-family:system-ui,sans-serif;margin:0;display:grid;\
         place-items:center;min-height:100dvh;background:#18181b;color:#fafafa\">\
         <main style=\"max-width:34rem;padding:2rem\">\
         <h1 style=\"font-size:1.25rem;margin:0 0 .5rem\">That connection did not complete</h1>\
         <p style=\"margin:0 0 1.5rem;color:#a1a1aa\">{escaped}</p>\
         <a href=\"/\" style=\"color:#fafafa\">Back to the app</a>\
         </main></body></html>"
    );
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .body(body.into())
        .expect("static error page")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(path: &str) -> Request {
        Request::builder()
            .uri(format!("http://gw.example.com{path}"))
            .body(rama::http::Body::empty())
            .expect("test request")
    }

    /// The whole reason this helper exists instead of the `Path` extractor.
    ///
    /// rama matches on `uri.path().to_lowercase()` and cuts the extractor's
    /// params out of that lowercased string, never percent-decoding them. So
    /// `Qwen/Qwen3-32B` reaches a handler as `qwen%2fqwen3-32b`, matches no
    /// row, and `model_defaults::delete` reports success for a row it never
    /// found — a 204 for an override that is still there.
    #[test]
    fn a_raw_segment_keeps_its_case_and_decodes_escapes() {
        assert_eq!(
            raw_path_segment(&get("/api/v0/admin/models/Qwen%2FQwen3-32B"), 0).as_deref(),
            Some("Qwen/Qwen3-32B"),
        );
        assert_eq!(
            raw_path_segment(&get("/api/v0/skills/Deck-Builder/archive"), 1).as_deref(),
            Some("Deck-Builder"),
            "counted from the end, so the trailing `/archive` is segment 0",
        );
        assert_eq!(
            raw_path_segment(&get("/api/v0/admin/users/AzureAD%7C42/impersonate"), 1).as_deref(),
            Some("AzureAD|42"),
            "an OIDC subject is case-sensitive and may carry escaped characters",
        );
    }

    /// An empty or absent segment is `None`, not an empty-string lookup that
    /// would silently address the wrong row.
    #[test]
    fn a_missing_segment_is_none() {
        assert_eq!(raw_path_segment(&get("/api/v0/admin/models/"), 0), None);
        assert_eq!(raw_path_segment(&get("/api/v0"), 5), None);
    }
}
