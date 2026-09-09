// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Shared page chrome — Theme cookie, SSE-event helpers, Flash
//! toasts, cookie parsing, body collection, and the unauthenticated
//! `<html>` layout. Lives in `session-core` so a future second
//! consumer can paint the same styling and the same datastar SSE
//! patches without forking.
//!
//! What stays per-binary: the sidebar (nav items + auth model), the
//! authed-layout wrapper that wraps it, the auth gate, the login
//! page shape, and the page handlers themselves.

use rama::http::service::web::response::IntoResponse;
use rama::http::{Body, HeaderMap, HeaderValue, Response, StatusCode, header};

// ---------------------------------------------------------------------------
// Theme.

/// Cookie name carrying the user's theme preference. Read on every
/// page render; written by `theme_toggle` after a flip.
pub const THEME_COOKIE: &str = "theme";

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Theme {
    Dark,
    Light,
}

impl Theme {
    /// Reads `theme=` from the request's Cookie header. Defaults to
    /// Dark when missing or unrecognised — operators run these
    /// tools in tooling contexts and dark reads better.
    pub fn from_headers(headers: &HeaderMap) -> Self {
        match read_cookie(headers, THEME_COOKIE).as_deref() {
            Some("light") => Theme::Light,
            _ => Theme::Dark,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Theme::Dark => "dark",
            Theme::Light => "light",
        }
    }
    pub fn flip(self) -> Self {
        match self {
            Theme::Dark => Theme::Light,
            Theme::Light => Theme::Dark,
        }
    }
}

/// `Set-Cookie` value for the theme. 1-year max-age so the
/// preference rides reloads + fresh tabs.
pub fn set_theme_header(theme: Theme) -> HeaderValue {
    let value = format!(
        "theme={}; Path=/; SameSite=Lax; Max-Age={}",
        theme.as_str(),
        60 * 60 * 24 * 365
    );
    HeaderValue::try_from(value).expect("theme cookie value is ascii")
}

// ---------------------------------------------------------------------------
// Language switcher.

// ---------------------------------------------------------------------------
// Sidebar section collapse state.

// ---------------------------------------------------------------------------
// HTML escaping.

/// Escape the five HTML-significant characters (`& < > " '`) so a string
/// can be spliced into markup as inert text. Shared by every hand-built
/// HTML fragment that isn't going through plait's auto-escaping (e.g. the
/// gateway's OIDC form fields and the DB layer's search-snippet
/// highlighter) so the escape set can't drift between copies.
pub fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Datastar request detection.

// ---------------------------------------------------------------------------
// Cookies.

/// Pull a named cookie out of a `Cookie:` header. Tolerates whitespace
/// after `;`; no percent-decoding (current callers store URL-safe
/// values only).
pub fn read_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    let header = headers.get(header::COOKIE)?.to_str().ok()?;
    for piece in header.split(';') {
        let piece = piece.trim();
        if let Some((k, v)) = piece.split_once('=')
            && k == name
        {
            return Some(v.to_string());
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Flash + toast.

// ---------------------------------------------------------------------------
// SSE event helpers (datastar-patch-elements / -signals).

/// Build a `datastar-patch-elements` SSE event payload (terminated by
/// the blank line that ends an SSE event). `elements_html` may be
/// empty — `mode remove` doesn't need a body.
pub fn sse_patch(
    selector: Option<&str>,
    mode: Option<&str>,
    elements_html: &str,
) -> rama::bytes::Bytes {
    let mut out = String::from("event: datastar-patch-elements\n");
    if let Some(sel) = selector {
        out.push_str(&format!("data: selector {sel}\n"));
    }
    if let Some(m) = mode {
        out.push_str(&format!("data: mode {m}\n"));
    }
    if !elements_html.is_empty() {
        for line in elements_html.split('\n') {
            out.push_str("data: elements ");
            out.push_str(line);
            out.push('\n');
        }
    }
    out.push('\n');
    rama::bytes::Bytes::from(out.into_bytes())
}

/// Fire a one-shot snippet of JS on the client. Datastar 1.x dropped
/// the standalone `datastar-execute-script` event; we ride on the
/// element-patching pipeline (append a `<script>` to `<body>`, let the
/// browser execute, the script removes itself).
pub fn sse_script(js: &str) -> rama::bytes::Bytes {
    let payload =
        format!("<script>try{{ {js} }} finally {{ document.currentScript?.remove(); }}</script>");
    sse_patch(Some("body"), Some("append"), &payload)
}

/// `datastar-patch-signals` event. The body is a JSON object that
/// Datastar merges into the global signal store.
pub fn sse_signals(signals_json: &str) -> rama::bytes::Bytes {
    let mut out = String::from("event: datastar-patch-signals\n");
    for line in signals_json.split('\n') {
        out.push_str("data: signals ");
        out.push_str(line);
        out.push('\n');
    }
    out.push('\n');
    rama::bytes::Bytes::from(out.into_bytes())
}

/// Bundle a set of pre-built SSE event payloads into a single response.
pub fn sse_response(events: &[rama::bytes::Bytes]) -> Response {
    let mut payload = Vec::with_capacity(events.iter().map(|e| e.len()).sum());
    for ev in events {
        payload.extend_from_slice(ev);
    }
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header("x-accel-buffering", "no")
        .body(payload.into())
        .unwrap()
}

// ---------------------------------------------------------------------------
// Body collection.

pub async fn read_body_to_bytes(body: Body) -> Result<rama::bytes::Bytes, String> {
    use rama::http::body::util::BodyExt;
    body.collect()
        .await
        .map(|c| c.to_bytes())
        .map_err(|e| format!("reading body: {e}"))
}

// ---------------------------------------------------------------------------
// Plain (unauthed) HTML responses.

/// 303 redirect — Post/Redirect/Get so reloads don't re-submit.
pub fn see_other(to: &str) -> Response {
    Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header(header::LOCATION, to)
        .body("".into())
        .unwrap()
}

/// Wrap an arbitrary HTML body string in an `200 OK; text/html`
/// response with the usual `Permissions-Policy` we set for every page
/// (mic + geolocation same-origin only; camera disabled).
///
/// `geolocation=(self)` (not `()`!) is load-bearing: an empty allowlist
/// disables the feature entirely, so `navigator.geolocation` rejects
/// with `PERMISSION_DENIED` *without ever prompting* — which is exactly
/// what `get_user_location`'s in-chat "share your location?" prompt
/// needs to NOT happen. `(self)` lets the same-origin page request it,
/// at which point the browser shows its native allow/deny prompt.
pub fn html_response(body: String) -> Response {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (
                rama::http::HeaderName::from_static("permissions-policy"),
                "microphone=(self), camera=(), geolocation=(self)",
            ),
        ],
        body,
    )
        .into_response()
}
