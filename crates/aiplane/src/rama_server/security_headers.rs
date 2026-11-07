// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Baseline browser-hardening headers on every response.
//!
//! The gateway serves its own SPA, the operator's logo and the documentation
//! site from one origin, so a single response that an attacker can make the
//! browser interpret as active content is enough to run in that origin. The
//! headers here are the cheap, non-breaking half: no MIME sniffing, no
//! framing by other sites, and a CSP for HTML and SVG.
//!
//! The SPA's `app.html` carries one inline bootstrap script (theme and
//! language before first paint) and SvelteKit injects inline hydration data,
//! so `script-src` has to allow inline scripts. That keeps this from being a
//! complete XSS control on its own; it still stops a response from loading a
//! script, stylesheet or frame from another origin. The documentation site
//! keeps its own policy because MkDocs' bundle loads Google Fonts, so the CSP
//! is scoped to the application shell and SVG.

use std::convert::Infallible;

use rama::http::{Request, Response, header};
use rama::{Layer, Service};

/// The application shell's policy. `connect-src 'self'` keeps the JSON API
/// same-origin; `form-action` admits `https:` because browsers apply it to the
/// redirects a form submission follows, and connecting an integration posts to
/// `/integrations/{key}/connect`, which redirects to the provider's consent page; `img-src`/`media-src` allow the blob and data URLs the canvas,
/// voice recorder and generated images use.
const APP_CSP: &str = "default-src 'self'; base-uri 'self'; form-action 'self' https:; \
     frame-ancestors 'self'; object-src 'none'; \
     img-src 'self' data: blob: https:; media-src 'self' blob:; font-src 'self' data:; \
     style-src 'self' 'unsafe-inline'; script-src 'self' 'unsafe-inline'; \
     connect-src 'self'";

/// An SVG is a document the browser will run script in when it is opened
/// directly. The operator's logo is uploaded, so it is not a trusted script
/// source even though only an admin can put it there.
const SVG_CSP: &str = "default-src 'none'; style-src 'unsafe-inline'; img-src data:";

const HEADERS: &[(&str, &str)] = &[
    ("x-content-type-options", "nosniff"),
    ("x-frame-options", "SAMEORIGIN"),
    ("referrer-policy", "strict-origin-when-cross-origin"),
];

#[derive(Clone, Copy, Debug, Default)]
pub struct SecurityHeadersLayer;

impl<S> Layer<S> for SecurityHeadersLayer {
    type Service = SecurityHeaders<S>;

    fn layer(&self, inner: S) -> Self::Service {
        SecurityHeaders { inner }
    }

    fn into_layer(self, inner: S) -> Self::Service {
        SecurityHeaders { inner }
    }
}

#[derive(Clone, Debug)]
pub struct SecurityHeaders<S> {
    inner: S,
}

impl<S> Service<Request> for SecurityHeaders<S>
where
    S: Service<Request, Output = Response, Error = Infallible>,
{
    type Output = Response;
    type Error = Infallible;

    async fn serve(&self, req: Request) -> Result<Self::Output, Self::Error> {
        let docs = req.uri().path() == "/docs" || req.uri().path().starts_with("/docs/");
        let mut resp = self.inner.serve(req).await?;
        let headers = resp.headers_mut();
        for (name, value) in HEADERS {
            headers
                .entry(*name)
                .or_insert_with(|| header::HeaderValue::from_static(value));
        }
        let content_type = headers
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        if content_type.starts_with("image/svg+xml") {
            headers.insert(
                header::CONTENT_SECURITY_POLICY,
                header::HeaderValue::from_static(SVG_CSP),
            );
        } else if content_type.starts_with("text/html") && !docs {
            headers.insert(
                header::CONTENT_SECURITY_POLICY,
                header::HeaderValue::from_static(APP_CSP),
            );
        }
        Ok(resp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rama::http::Body;

    #[derive(Clone)]
    struct Reply(&'static str);

    impl Service<Request> for Reply {
        type Output = Response;
        type Error = Infallible;

        async fn serve(&self, _req: Request) -> Result<Self::Output, Self::Error> {
            Ok(Response::builder()
                .header(header::CONTENT_TYPE, self.0)
                .body(Body::empty())
                .unwrap())
        }
    }

    async fn headers_for(content_type: &'static str, path: &str) -> Response {
        SecurityHeaders {
            inner: Reply(content_type),
        }
        .serve(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn every_response_gets_the_baseline_headers() {
        let resp = headers_for("application/json", "/api/v0/me").await;
        for (name, value) in HEADERS {
            assert_eq!(resp.headers()[*name], *value);
        }
    }

    #[tokio::test]
    async fn the_shell_gets_a_csp_and_the_docs_do_not() {
        let shell = headers_for("text/html; charset=utf-8", "/chat").await;
        assert_eq!(shell.headers()[header::CONTENT_SECURITY_POLICY], APP_CSP);
        let docs = headers_for("text/html; charset=utf-8", "/docs/guide/chat/").await;
        assert!(
            docs.headers()
                .get(header::CONTENT_SECURITY_POLICY)
                .is_none()
        );
    }

    #[test]
    fn forms_may_follow_a_redirect_to_an_https_provider() {
        let form_action = APP_CSP
            .split(';')
            .map(str::trim)
            .find(|directive| directive.starts_with("form-action"))
            .unwrap();
        assert_eq!(form_action, "form-action 'self' https:");
    }

    #[tokio::test]
    async fn svg_gets_a_policy_that_cannot_run_script() {
        let resp = headers_for("image/svg+xml", "/brand/logo").await;
        assert_eq!(resp.headers()[header::CONTENT_SECURITY_POLICY], SVG_CSP);
    }
}
