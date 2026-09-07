// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Drift guard for `docs/openapi.json` — the hand-maintained OpenAPI spec
//! of the `/api/v0/*` session API (issue #22, phase 1).
//!
//! The spec is deliberately NOT generated from code annotations (see its
//! `info.description` for why), which only works if the contract is
//! enforced by something. This test is that something: it fails CI when the
//! spec and the routes registered in `router.rs` drift apart, in either
//! direction —
//!
//!   - a route is registered in `router.rs` but missing from the spec
//!     (a new `/api/v0` endpoint shipped undocumented — the generated TS
//!     client would not know it exists), or
//!   - the spec documents a `(method, path)` no route serves (a stale entry
//!     after a rename/removal — the client would call a 404).
//!
//! Both route syntaxes use the SAME parameter spelling (`{id}`, `{turn_id}`
//! — rama and OpenAPI chose the same brace syntax), so paths compare
//! verbatim after lowercasing the method.
//!
//! Scope: `/api/v0/*` only. `/v1/*` (OpenAI relay, documented in
//! docs/gateway-api.md), `/auth/*` (browser redirect flow), and the legacy
//! page routes are outside this spec by design — see its `info.description`.
//! The guard asserts the converse too: no `/api/v0` route may be absent
//! from the spec, so adding one forces a spec entry.

use std::path::Path;

use serde_json::Value;

/// HTTP verbs a route can register (`with_get` → `GET`) and the key the
/// spec nests them under (`get`).
const METHODS: &[(&str, &str)] = &[
    ("GET", "get"),
    ("POST", "post"),
    ("PUT", "put"),
    ("DELETE", "delete"),
    ("PATCH", "patch"),
];

/// `(METHOD, path)` routes registered in `router.rs` whose path starts with
/// `prefix` — scanned the same way `readme_routes.rs` scans (the path is
/// always the first string literal after `.with_<verb>(`).
fn actual_routes(src: &str, prefix: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (verb, _) in METHODS {
        let needle = format!(".with_{}(", verb.to_lowercase());
        let mut from = 0;
        while let Some(pos) = src[from..].find(&needle) {
            let after = from + pos + needle.len();
            from = after;
            let Some(q1) = src[after..].find('"') else {
                continue;
            };
            let s = after + q1 + 1;
            let Some(q2) = src[s..].find('"') else {
                continue;
            };
            let path = &src[s..s + q2];
            if path.starts_with(prefix) {
                out.push((verb.to_string(), path.to_string()));
            }
        }
    }
    out
}

/// `(method, path)` pairs the spec declares, from `paths.<path>.<method>`.
/// Keys under `paths` that are not method objects (`parameters`, `summary`)
/// are ignored — a method entry is an object that has a `responses` key.
fn spec_routes(spec: &Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let paths = spec
        .get("paths")
        .and_then(Value::as_object)
        .expect("spec has a `paths` object");
    for (path, item) in paths {
        let Some(obj) = item.as_object() else {
            continue;
        };
        for (verb, _) in METHODS {
            if let Some(op) = obj.get(verb.to_lowercase().as_str())
                && op.get("responses").is_some()
            {
                out.push((verb.to_string(), path.clone()));
            }
        }
    }
    out
}

#[test]
fn openapi_spec_matches_router_api_v0_routes() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let router_src = std::fs::read_to_string(manifest.join("src/rama_server/router.rs"))
        .expect("read router.rs");
    let spec_bytes =
        std::fs::read(manifest.join("../../docs/openapi.json")).expect("read docs/openapi.json");
    let spec: Value = serde_json::from_slice(&spec_bytes)
        .expect("docs/openapi.json is valid JSON (it is the file the TS client generates from)");

    assert_eq!(
        spec.get("openapi")
            .and_then(Value::as_str)
            .map(|v| v.split('.').next().unwrap_or("")),
        Some("3"),
        "spec must stay on OpenAPI 3.x (the version openapi-typescript supports)"
    );

    let actual = actual_routes(&router_src, "/api/v0");
    let documented = spec_routes(&spec);

    assert!(
        !actual.is_empty(),
        "parsed zero /api/v0 routes from router.rs"
    );
    assert!(
        !documented.is_empty(),
        "parsed zero operations from docs/openapi.json"
    );

    let mut errors = Vec::new();

    // 1. Every registered /api/v0 route has a spec operation.
    for (verb, path) in &actual {
        if !documented.iter().any(|(m, p)| m == verb && p == path) {
            errors.push(format!(
                "  route `{verb} {path}` is registered in router.rs but has no \
                 `{}` operation in docs/openapi.json",
                verb.to_lowercase()
            ));
        }
    }

    // 2. Every spec operation is served by a real route.
    for (verb, path) in &documented {
        if !actual.iter().any(|(m, p)| m == verb && p == path) {
            errors.push(format!(
                "  docs/openapi.json documents `{verb} {path}` but no such route \
                 exists in router.rs (stale spec entry)"
            ));
        }
    }

    assert!(
        errors.is_empty(),
        "docs/openapi.json is out of sync with the /api/v0 routes in router.rs:\n{}",
        errors.join("\n")
    );
}
