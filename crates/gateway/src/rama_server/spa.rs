// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Static SPA hosting — serves the SvelteKit client-side app built by `web/`
//! (`adapter-static`) from a directory on disk.
//!
//! # Why a hand-rolled handler and not a `tower-http::ServeDir`
//!
//! The dependency policy (`docs/dependencies.md`) keeps the server stack
//! rama-only; `tower-http` is explicitly not-allowed. Rather than bend that
//! rule we serve the files with plain rama + `tokio::fs`. The logic here is
//! small and fully unit-tested (`resolve_path` for the traversal guard,
//! `content_type` for the extension map, `serve` for the whole request path),
//! which keeps it safe.
//!
//! # Layout and caching
//!
//! SvelteKit's `adapter-static` emits **content-hashed** asset filenames
//! (`/_app/immutable/…`), so a hashed file never changes
//! content at a given URL — those are cached `immutable` for a year. The
//! non-hashed surface (`index.html`, the service worker, the manifest) must
//! **not** be `immutable`, or an update never reaches the browser; those get
//! `no-cache` / a short max-age.
//!
//! # SPA history fallback
//!
//! A client-side route like `/tokens` has no file on disk. Anything that
//! is not an existing file (and not a traversal) falls back to
//! `index.html`, which lets the SPA's router take over. This is the standard
//! SPA-fallback behaviour a static host must provide.
//!
//! # Case preservation
//!
//! rama lowercases the *matched* path for route lookup, but the `Request`
//! handed to the handler keeps its **original** case (see `router.rs` and the
//! `retrieve_model` precedent). `req.uri().path()` therefore preserves the
//! case of the content-hashed filename, which is why hashed assets resolve.
//!
//! # Mount point
//!
//! The SPA owns the root: it is the whole UI now that the server-rendered
//! pages are gone. Its catch-all is registered last so the API, proxy and
//! auth routes still match first.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use rama::http::{Body, Request, Response, StatusCode, header};

/// Env var pointing at the SvelteKit `build/` output directory. When unset
/// (or the dir is missing) the SPA is not deployed and requests 503 — the
/// rest of the gateway is unaffected. Set in the container image and by the
/// `dev-ui` example; see `Dockerfile`.
pub const STATIC_DIR_ENV: &str = "GATEWAY_STATIC_DIR";

/// One year, `immutable`: for content-hashed files whose bytes never change
/// at a given URL.
const IMMUTABLE_CACHE: &str = "public, max-age=31536000, immutable";
/// Never serve from cache: the SPA entry point and service worker must
/// revalidate so updates roll out.
const NO_CACHE: &str = "no-cache";
/// Short revalidating cache: the manifest and other stable non-hashed assets.
const SHORT_CACHE: &str = "public, max-age=300";

/// The static root, resolved once from the environment. Empty when
/// `GATEWAY_STATIC_DIR` is unset — in which case [`serve`] answers 503.
static STATIC_ROOT: LazyLock<PathBuf> = LazyLock::new(|| {
    std::env::var(STATIC_DIR_ENV)
        .map(PathBuf::from)
        .unwrap_or_default()
});

/// `GET /{*name}` — the catch-all route handler. Thin wrapper over
/// [`serve`] using the env-resolved root.
pub async fn spa_get(req: Request) -> Response {
    serve(&STATIC_ROOT, &req).await
}

/// Namespaces that belong to the API, not to the client router.
///
/// The catch-all is registered last, so anything the router did not match
/// lands here. Answering all of it with the app shell means a typo'd or
/// removed endpoint returns HTML with a 200 where the caller expected JSON
/// and a 404 — a genuinely confusing thing to debug.
///
/// This is an ALLOWLIST of the client router's own top-level routes, not a
/// denylist of the server's. A denylist has to be kept in lockstep with a
/// router it cannot see, and the previous one (`/api/`, `/v1/`, `/auth/`)
/// already wasn't: `/healthz`, `/readyz`, `/hooks/*`, `/rag/*`,
/// `/integrations/*` and `/__dev/*` are all server-owned and matched none of
/// those prefixes, so `GET /healthzz` cheerfully returned the app shell.
///
/// Kept in step with `web/src/routes/` by
/// [`tests::the_client_routes_match_the_spa_source`].
const SPA_ROUTES: [&str; 12] = [
    "admin",
    "chat",
    "integrations",
    "login",
    "memory",
    "scheduled",
    "setup",
    "skills",
    "tokens",
    "tools",
    "usage",
    "webhooks",
];

/// Does the SPA's client router own this path?
///
/// True for `/`, for a real file in the build directory (handled by the
/// caller), and for anything under one of [`SPA_ROUTES`]. Everything else
/// reaching the catch-all is a request for something nobody serves.
fn is_client_route(path: &str) -> bool {
    let rel = path.strip_prefix('/').unwrap_or(path);
    if rel.is_empty() {
        return true;
    }
    let root = rel.split('/').next().unwrap_or("");
    SPA_ROUTES.contains(&root)
}

/// Serve the SPA for `req`, rooted at `root`. Pure with respect to the
/// filesystem (reads via `tokio::fs`) and the request — no global state — so
/// it is directly unit-testable against a temp dir.
async fn serve(root: &Path, req: &Request) -> Response {
    let path = req.uri().path();

    // `path` is the original-case URI path (rama lowercases only the matched
    // prefix for routing), so a content-hashed filename keeps its case here.
    // Everything below the root is relative to the build directory.
    let rel: &str = path.strip_prefix('/').unwrap_or("");
    // Empty relative path → the SPA entry point.
    let rel = if rel.is_empty() { "index.html" } else { rel };

    // Security: resolve within the root, refusing any traversal.
    let file_path = match resolve_path(root, rel) {
        Some(p) => p,
        None => {
            return Response::builder()
                .status(StatusCode::FORBIDDEN)
                .body(Body::from("forbidden path"))
                .unwrap();
        }
    };

    // A real file wins.
    match tokio::fs::read(&file_path).await {
        Ok(bytes) => {
            let ext = file_path
                .extension()
                .and_then(|e| e.to_str().map(str::to_lowercase))
                .unwrap_or_default();
            let filename = file_path
                .file_name()
                .and_then(|n| n.to_str().map(str::to_string))
                .unwrap_or_default();
            let content_type = content_type(&ext);
            let cache_control = cache_control(&ext, &filename, rel);
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, content_type)
                .header(header::CACHE_CONTROL, cache_control)
                .body(Body::from(bytes))
                .unwrap()
        }
        Err(_) => {
            // Not a file on disk. Ask for the entry point first, because a
            // missing entry point means the SPA was never deployed — a 503
            // the operator needs to see whatever path they happened to
            // request. Only once we know the app IS deployed does it make
            // sense to talk about whether this particular path exists.
            let mut resp = serve_entry(root).await;
            if resp.status() != StatusCode::OK || is_client_route(path) {
                return resp;
            }
            // Deployed, but the client router does not own this path either,
            // so nothing serves it. 404 rather than the 200 the shell would
            // otherwise carry — but the body still follows what the caller
            // asked for: a browser gets the shell and renders its own styled
            // 404, anything else gets the error envelope instead of a page of
            // HTML it cannot parse.
            let wants_html = req
                .headers()
                .get(header::ACCEPT)
                .and_then(|v| v.to_str().ok())
                .is_some_and(|a| a.contains("text/html"));
            if wants_html {
                *resp.status_mut() = StatusCode::NOT_FOUND;
                return resp;
            }
            Response::builder()
                .status(StatusCode::NOT_FOUND)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"error":{"message":"no such endpoint","type":"not_found","code":"not_found"}}"#,
                ))
                .expect("static JSON 404")
        }
    }
}

/// Serve `index.html` (the SPA entry point) with a non-`immutable` cache
/// policy. Falls back to 503 if it does not exist (SPA not built/deployed).
async fn serve_entry(root: &Path) -> Response {
    let index = root.join("index.html");
    match tokio::fs::read(&index).await {
        Ok(bytes) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .header(header::CACHE_CONTROL, NO_CACHE)
            .body(Body::from(bytes))
            .unwrap(),
        Err(_) => not_deployed(),
    }
}

/// 503 — the SPA build was not deployed at `GATEWAY_STATIC_DIR`. Deliberately
/// distinct from a 404 (a known endpoint that needs its build) and from the
/// router's `RouterError` 404 (an unknown path), so operators can tell
/// "SPA not deployed" apart from "that path doesn't exist".
fn not_deployed() -> Response {
    Response::builder()
        .status(StatusCode::SERVICE_UNAVAILABLE)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(
            format!(
                "static SPA not deployed — set {STATIC_DIR_ENV} to the SvelteKit build/ directory"
            )
            .into(),
        )
        .unwrap()
}

/// Resolve `rel` (a path relative to the SPA root) to a real path, returning
/// `None` if it would escape `root`. This is the traversal guard: a request
/// like `/../../etc/passwd` must not read files outside the build dir.
///
/// We normalise `rel` **on its own** (not after joining `root`): a leading
/// `..` with no prior component to consume means the path climbs above the
/// relative root, which is exactly the escape we must refuse. Normalising
/// after joining `root` would let the `..` be silently absorbed by `root`'s
/// own components and land back inside it — safe, but it would make the
/// guard a no-op and mask a real traversal attempt.
fn resolve_path(root: &Path, rel: &str) -> Option<PathBuf> {
    // Reject absolute paths outright — a `rel` from a URI path is always
    // relative, but be defensive.
    if Path::new(rel).is_absolute() {
        return None;
    }

    // Normalise `rel` lexically: drop `.` and `..` that have a matching
    // prior component; a `..` with nothing left to consume escapes the root.
    let mut norm: Vec<std::ffi::OsString> = Vec::new();
    for comp in Path::new(rel).components() {
        match comp {
            std::path::Component::Normal(c) => norm.push(c.to_os_string()),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                // A `..` consumes the last surviving component; one with
                // nothing left to consume (`pop` → `None`) climbed above the
                // relative root and must be refused.
                norm.pop()?;
            }
            // `RootDir` / `Prefix` — a relative `rel` should have neither;
            // refuse if one slipped in (we only serve under `root`).
            _ => return None,
        }
    }

    let normed = norm.iter().fold(std::path::PathBuf::new(), |mut acc, c| {
        acc.push(c);
        acc
    });
    Some(root.join(normed))
}

/// Content type for a file extension. Falls back to `application/octet-stream`
/// for anything unknown so we never mislabel a binary.
fn content_type(ext: &str) -> &'static str {
    match ext {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "webmanifest" => "application/manifest+json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "wasm" => "application/wasm",
        "map" => "application/json; charset=utf-8",
        "txt" | "md" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Cache-Control for a file, by kind.
///
/// Only content-hashed assets may be `immutable`, and the one reliable signal
/// for that is the path: SvelteKit puts them under `_app/immutable/`, where
/// the filename carries a content hash so the URL genuinely never changes
/// meaning.
///
/// Everything in `web/static/` is copied through verbatim — `pcm-recorder.js`,
/// `favicon.svg`, `robots.txt`, `icons/*`, `_app/version.json`. Those keep
/// their names across deploys, so a year of `immutable` would mean a browser
/// that cached one can never be handed a fixed version: a bug in the audio
/// worklet would be permanent for that user. They get a short max-age.
///
/// (The previous rule was "anything that is not index/sw/manifest is hashed",
/// which was simply not true of that directory.)
fn cache_control(ext: &str, filename: &str, rel_path: &str) -> &'static str {
    let _ = ext;
    if filename == "index.html" || filename == "sw.js" {
        NO_CACHE
    } else if rel_path.contains("_app/immutable/") {
        IMMUTABLE_CACHE
    } else {
        // The manifest and every verbatim-copied static file.
        SHORT_CACHE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A temp dir with a minimal SPA layout: a hashed JS asset, a CSS file,
    /// the entry point, and a nested file.
    fn spa_tempdir() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();

        std::fs::write(
            root.join("index.html"),
            "<!doctype html><html><head></head><body></body></html>",
        )
        .unwrap();
        std::fs::write(root.join("sw.js"), "self.skipWaiting()").unwrap();
        std::fs::write(root.join("manifest.webmanifest"), r#"{"name":"x"}"#).unwrap();

        let assets = root
            .join("assets")
            .join("_app")
            .join("immutable")
            .join("entry");
        std::fs::create_dir_all(&assets).unwrap();
        // A content-hashed filename with UPPERCASE letters — SvelteKit emits
        // these; the router must not lowercase them.
        let hashed = "START-AbC123.js";
        std::fs::write(assets.join(hashed), "export const x=1").unwrap();
        let css = "styles-XYZ789.css";
        std::fs::write(assets.join(css), "body{}").unwrap();

        let nested = root.join("sub").join("page.html");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(&nested, "<p>deep</p>").unwrap();

        (dir, root)
    }

    fn get(path: &str) -> Request {
        Request::builder()
            .method(rama::http::Method::GET)
            .uri(path)
            .body(rama::http::Body::empty())
            .unwrap()
    }

    async fn drain(resp: Response) -> Vec<u8> {
        use rama::http::body::util::BodyExt;
        resp.into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec()
    }

    #[tokio::test]
    async fn serves_a_hashed_asset_with_immutable_cache_and_correct_type() {
        let (_d, root) = spa_tempdir();
        let req = get("/assets/_app/immutable/entry/START-AbC123.js");
        let resp = serve(&root, &req).await;

        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        let cc = resp
            .headers()
            .get(header::CACHE_CONTROL)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert!(ct.starts_with("text/javascript"), "got {ct}");
        assert!(
            cc.contains("immutable"),
            "hashed asset must be immutable, got {cc}"
        );

        let body = drain(resp).await;
        assert_eq!(body, b"export const x=1");
    }

    #[tokio::test]
    async fn serves_the_entry_point_for_bare_app_prefix() {
        let (_d, root) = spa_tempdir();
        let resp = serve(&root, &get("/")).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let cc = resp
            .headers()
            .get(header::CACHE_CONTROL)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert_eq!(cc, NO_CACHE, "index.html must revalidate, got {cc}");
    }

    #[tokio::test]
    async fn spa_history_route_falls_back_to_index_html() {
        let (_d, root) = spa_tempdir();
        // `/tokens` is a client route with no file on disk.
        let resp = serve(&root, &get("/tokens")).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = drain(resp).await;
        let s = String::from_utf8_lossy(&body);
        assert!(
            s.contains("<!doctype html"),
            "should fall back to index.html: {s}"
        );
    }

    #[tokio::test]
    async fn missing_static_dir_is_a_503_not_a_404() {
        let empty = std::path::Path::new("/nonexistent-gateway-static-dir-xyz");
        let resp = serve(empty, &get("/")).await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    /// An unknown API path must not be answered with the app shell.
    ///
    /// The catch-all is registered last, so every mistyped or removed endpoint
    /// lands here. Serving index.html with a 200 would hand an API caller HTML
    /// where it expected JSON, and success where it expected 404 — the sort of
    /// answer that sends someone debugging their own client for an hour.
    #[tokio::test]
    async fn an_unknown_api_path_is_a_json_404_not_the_app_shell() {
        let (_d, root) = spa_tempdir();
        for uri in [
            "/api/v0/no-such-endpoint",
            "/v1/no-such-endpoint",
            "/auth/no-such-endpoint",
        ] {
            let resp = serve(&root, &get(uri)).await;
            assert_eq!(resp.status(), StatusCode::NOT_FOUND, "{uri}");
            let ct = resp
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_string();
            assert!(ct.contains("json"), "{uri} answered {ct}, not JSON");
        }

        // A real client route still gets the shell, including a deep one the
        // client router resolves itself.
        for uri in ["/", "/chat", "/chat/abc-123", "/admin/settings"] {
            let resp = serve(&root, &get(uri)).await;
            assert_eq!(resp.status(), StatusCode::OK, "{uri} is a client route");
        }
    }

    /// The gap the old denylist left: server-owned paths that start with none
    /// of `/api/`, `/v1/`, `/auth/`.
    ///
    /// `GET /healthzz` used to answer 200 text/html with the whole app. Every
    /// one of these is served by the router when spelled correctly, so a
    /// near-miss must 404 rather than pretend to be a page.
    #[tokio::test]
    async fn a_typo_on_a_server_route_is_a_404_not_the_app_shell() {
        let (_d, root) = spa_tempdir();
        for uri in [
            "/healthzz",
            "/readyzz",
            "/hooksfoo",
            "/rag/typo",
            "/__dev/nope",
            "/apiary",
            "/nonsense",
        ] {
            let resp = serve(&root, &get(uri)).await;
            assert_eq!(
                resp.status(),
                StatusCode::NOT_FOUND,
                "{uri} is owned by nobody and must not answer with the app shell"
            );
        }
    }

    /// `SPA_ROUTES` matches the SvelteKit source.
    ///
    /// The allowlist decides what gets the history fallback, so a new page in
    /// `web/src/routes/` that nobody adds here would 404 on a hard load while
    /// working fine via client-side navigation — the kind of bug that only
    /// shows up for someone who pastes a link.
    #[test]
    fn the_client_routes_match_the_spa_source() {
        let routes_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/src/routes");
        let mut found: Vec<String> = std::fs::read_dir(&routes_dir)
            .expect("read web/src/routes")
            .filter_map(|e| {
                let entry = e.ok()?;
                if !entry.file_type().ok()?.is_dir() {
                    return None;
                }
                let name = entry.file_name().to_str()?.to_string();
                // `+layout`/`+page` files are not directories; a `[param]`
                // directory is a child route, never a top-level one.
                (!name.starts_with('+') && !name.starts_with('[')).then_some(name)
            })
            .collect();
        found.sort();
        let mut declared: Vec<String> = SPA_ROUTES.iter().map(|s| s.to_string()).collect();
        declared.sort();
        assert_eq!(
            declared, found,
            "SPA_ROUTES is out of step with web/src/routes — a route missing here \
             404s on a hard load but works via client navigation"
        );
    }

    #[test]
    fn traversal_cannot_escape_the_root() {
        let root = Path::new("/srv/ui");
        // Classic climb: `..` segments that reach above the root are refused.
        assert!(resolve_path(root, "../../etc/passwd").is_none());
        assert!(resolve_path(root, "a/../../../etc").is_none());
        assert!(resolve_path(root, "..").is_none());
        // A leading `..` mixed with normal segments still escapes: refused.
        assert!(resolve_path(root, "../x/assets").is_none());
    }

    #[test]
    fn resolve_path_keeps_descendants_inside_the_root() {
        let root = Path::new("/srv/ui");
        let ok = resolve_path(root, "assets/_app/immutable/entry/START-AbC123.js").unwrap();
        assert_eq!(ok, root.join("assets/_app/immutable/entry/START-AbC123.js"));

        let nested = resolve_path(root, "sub/page.html").unwrap();
        assert_eq!(nested, root.join("sub/page.html"));
    }

    #[test]
    fn content_types_cover_the_common_cases() {
        assert_eq!(content_type("js"), "text/javascript; charset=utf-8");
        assert_eq!(content_type("css"), "text/css; charset=utf-8");
        assert_eq!(
            content_type("webmanifest"),
            "application/manifest+json; charset=utf-8"
        );
        assert_eq!(content_type("woff2"), "font/woff2");
        assert_eq!(content_type("wasm"), "application/wasm");
        // Unknown extension must not be mislabelled as text.
        assert_eq!(content_type("bin"), "application/octet-stream");
    }

    #[test]
    fn entry_and_sw_never_use_immutable_cache() {
        assert_eq!(cache_control("html", "index.html", "index.html"), NO_CACHE);
        assert_eq!(cache_control("js", "sw.js", "sw.js"), NO_CACHE);
        assert_eq!(
            cache_control(
                "webmanifest",
                "manifest.webmanifest",
                "manifest.webmanifest"
            ),
            SHORT_CACHE
        );
        // A hashed asset — and only because of where it lives.
        assert!(
            cache_control(
                "js",
                "START-AbC123.js",
                "_app/immutable/entry/START-AbC123.js"
            )
            .contains("immutable")
        );
    }

    /// Files copied verbatim out of `web/static/` keep their names forever,
    /// so `immutable` would make a bad one unfixable for anyone who cached
    /// it — a broken audio worklet that no deploy can replace.
    #[test]
    fn verbatim_static_files_are_not_immutable() {
        for (ext, name, rel) in [
            ("js", "pcm-recorder.js", "pcm-recorder.js"),
            ("svg", "favicon.svg", "favicon.svg"),
            ("txt", "robots.txt", "robots.txt"),
            ("png", "icon-192.png", "icons/icon-192.png"),
            ("json", "version.json", "_app/version.json"),
        ] {
            assert_eq!(
                cache_control(ext, name, rel),
                SHORT_CACHE,
                "{rel} is not content-hashed, so it must stay replaceable"
            );
        }
    }
}
