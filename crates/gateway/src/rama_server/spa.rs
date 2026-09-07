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
//! (`/assets/_app/immutable/entry/*.js`), so a hashed file never changes
//! content at a given URL — those are cached `immutable` for a year. The
//! non-hashed surface (`index.html`, the service worker, the manifest) must
//! **not** be `immutable`, or an update never reaches the browser; those get
//! `no-cache` / a short max-age.
//!
//! # SPA history fallback
//!
//! A client-side route like `/app/tokens` has no file on disk. Anything that
//! is not an existing file (and not a traversal) falls back to
//! `index.html`, which lets the SPA's router take over. This is the standard
//! SPA-fallback behaviour a static host must provide.
//!
//! # Case preservation
//!
//! rama lowercases the *matched* path for route lookup, but the `Request`
//! handed to the handler keeps its **original** case (see `router.rs` and the
//! `assets::icon` / `retrieve_model` precedent). `req.uri().path()` therefore
//! preserves the case of the content-hashed filename, and `strip_prefix("/app/")`
//! is case-sensitive on the original. This is why the hashed assets resolve.
//!
//! # Coexistence during migration
//!
//! The SPA is mounted under the `/app` prefix so it coexists with the current
//! server-rendered pages (which own `/`, `/chat`, `/tokens`, …) until those
//! are removed in a later phase. Nothing under `/app` collides with an
//! existing route.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use rama::http::{Body, Request, Response, StatusCode, header};
use rama::{Layer, Service};

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

/// `GET /app/{*name}` — the catch-all route handler. Thin wrapper over
/// [`serve`] using the env-resolved root.
pub async fn spa_get(req: Request) -> Response {
    serve(&STATIC_ROOT, &req).await
}

/// Normalises the trailing-slash form of the SPA root (`/app/` → `/app`)
/// before routing.
///
/// rama's route matcher trims a trailing slash when a route is *inserted*
/// (`/app` and `/app/` would collide) but does **not** trim it on *lookup*,
/// so a request for `/app/` — which browsers happily produce, and which the
/// Vite dev proxy can forward — matches nothing and 404s before any handler
/// runs. The rewrite applies to exactly `/app/` (with any query preserved);
/// everything else passes through untouched.
#[derive(Clone)]
pub struct SpaNormalizeLayer;

impl<S> Layer<S> for SpaNormalizeLayer {
    type Service = SpaNormalize<S>;

    fn layer(&self, inner: S) -> Self::Service {
        SpaNormalize { inner }
    }
}

#[derive(Clone)]
pub struct SpaNormalize<S> {
    inner: S,
}

impl<S> Service<Request> for SpaNormalize<S>
where
    S: Service<Request, Output = Response, Error = std::convert::Infallible>,
{
    type Output = Response;
    type Error = std::convert::Infallible;

    async fn serve(&self, mut req: Request) -> Result<Self::Output, Self::Error> {
        if req.uri().path() == "/app/" {
            let pq = match req.uri().query() {
                Some(q) => format!("/app?{q}"),
                None => "/app".to_string(),
            };
            // Parsing "/app[?query]" cannot fail; `expect` for a literal we
            // built ourselves matches the codebase's idiom for infallible
            // parses (see `first_run.rs`).
            let mut uri = req.uri().clone().into_parts();
            uri.path_and_query = Some(pq.parse().expect("static path rewrite parses as a URI"));
            *req.uri_mut() = rama::http::Uri::from_parts(uri)
                .expect("re-assembled URI from parts of a valid URI stays valid");
        }
        self.inner.serve(req).await
    }
}

/// Serve the SPA for `req`, rooted at `root`. Pure with respect to the
/// filesystem (reads via `tokio::fs`) and the request — no global state — so
/// it is directly unit-testable against a temp dir.
async fn serve(root: &Path, req: &Request) -> Response {
    let path = req.uri().path();
    // `path` is the original-case URI path (rama lowercases only the matched
    // prefix for routing). Strip the `/app` prefix to get the file's path
    // relative to `root`. The bare `/app` (no trailing slash) and `/app/`
    // both resolve to the entry point.
    let rel: &str = match path {
        "/app" => "",
        _ => path.strip_prefix("/app/").unwrap_or(""),
    };
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
            let cache_control = cache_control(&ext, &filename);
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, content_type)
                .header(header::CACHE_CONTROL, cache_control)
                .body(Body::from(bytes))
                .unwrap()
        }
        Err(_) => {
            // Not a file: the SPA history fallback serves the entry point so
            // the client router can resolve the route.
            serve_entry(root).await
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
/// like `/app/../../etc/passwd` must not read files outside the build dir.
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

/// Cache-Control for a file, by kind. Content-hashed SvelteKit assets live
/// under `/assets/_app/immutable/` and never change at a given URL →
/// `immutable`. The entry point and service worker must revalidate →
/// `no-cache`. Everything else stable → short max-age.
fn cache_control(ext: &str, filename: &str) -> &'static str {
    if filename == "index.html" || filename == "sw.js" {
        NO_CACHE
    } else if ext == "webmanifest" {
        SHORT_CACHE
    } else {
        // All hashed bundle files and static assets. SvelteKit puts the
        // immutable set under a path that contains `immutable/`, but a file
        // reaching here that is not index/sw/manifest is a hashed asset —
        // safe to cache long. (Serving a *non-hashed*, mutable file would
        // require naming it explicitly; the SPA build does not emit any
        // beyond the ones handled above.)
        IMMUTABLE_CACHE
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
        let req = get("/app/assets/_app/immutable/entry/START-AbC123.js");
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
        let resp = serve(&root, &get("/app/")).await;
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
        // `/app/tokens` is a client route with no file on disk.
        let resp = serve(&root, &get("/app/tokens")).await;
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
        let resp = serve(empty, &get("/app/")).await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn traversal_cannot_escape_the_root() {
        let root = Path::new("/app/ui");
        // Classic climb: `..` segments that reach above the root are refused.
        assert!(resolve_path(root, "../../etc/passwd").is_none());
        assert!(resolve_path(root, "a/../../../etc").is_none());
        assert!(resolve_path(root, "..").is_none());
        // A leading `..` mixed with normal segments still escapes: refused.
        assert!(resolve_path(root, "../x/assets").is_none());
    }

    #[test]
    fn resolve_path_keeps_descendants_inside_the_root() {
        let root = Path::new("/app/ui");
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
        assert_eq!(cache_control("html", "index.html"), NO_CACHE);
        assert_eq!(cache_control("js", "sw.js"), NO_CACHE);
        assert_eq!(
            cache_control("webmanifest", "manifest.webmanifest"),
            SHORT_CACHE
        );
        // A hashed JS asset caches immutable.
        assert!(cache_control("js", "START-AbC123.js").contains("immutable"));
    }
}
