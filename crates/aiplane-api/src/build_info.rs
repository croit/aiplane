// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Build/version metadata surfaced in the UI.
//!
//! The gateway is licensed under the GNU AGPL-3.0, whose §13 requires anyone
//! running a *modified* version as a network service to offer that service's
//! users the corresponding source. To make that practical the UI carries a
//! persistent "Source" link pointing at the repository for the running build.

use std::sync::LazyLock;

use rama::http::service::web::response::Json;
use serde_json::{Value, json};

/// Canonical public source repository.
const DEFAULT_SOURCE_URL: &str = "https://github.com/croit/aiplane";

/// Crate version (`CARGO_PKG_VERSION`) — the fallback when nothing stamped a
/// release version into the environment (a `cargo run`, or an image built by
/// hand). It is deliberately NOT the release identity: releases are named
/// `YYMM.RELEASE.BUILD` and come from the git tag, so Cargo.toml's number
/// stays where it is and nobody spends a commit bumping it. See
/// `docs/releases.md`.
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Short git commit the binary was built from, or `"unknown"`. Set by `build.rs`.
pub const GIT_SHA: &str = env!("AIPLANE_GIT_SHA");

/// The source URL the running build is offered from.
///
/// AGPL §13: if you fork and deploy modifications as a network service, set
/// `AIPLANE_SOURCE_URL` in the gateway's environment to *your* repository so
/// the in-app link points at the source actually running.
static SOURCE_URL: LazyLock<String> = LazyLock::new(|| {
    aiplane_core::server::env::var("AIPLANE_SOURCE_URL")
        .unwrap_or_else(|_| DEFAULT_SOURCE_URL.to_string())
});

/// The source URL to advertise (env override for forks, else the default).
pub fn source_url() -> &'static str {
    &SOURCE_URL
}

/// The version this build reports, e.g. `2609.1.0`.
///
/// Read from `$AIPLANE_VERSION` at *runtime*, not compiled in, so the image
/// promoted to a release is the same bytes that were tested — stamping the
/// number is an `ENV` in the image, not a recompile. `scripts/derive-version.sh`
/// produces it and CI passes it in as a build argument.
static VERSION: LazyLock<String> =
    LazyLock::new(|| version_from(aiplane_core::server::env::var("AIPLANE_VERSION").ok()));

/// The resolution itself, split out so it is testable: a `LazyLock` reads the
/// environment once per process, which a test cannot vary.
fn version_from(raw: Option<String>) -> String {
    raw.map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| CRATE_VERSION.to_string())
}

/// The version string to advertise.
pub fn version() -> &'static str {
    &VERSION
}

/// Human-readable build label, e.g. `v2609.1.0 (a1b2c3d4e5f6)`.
pub fn version_label() -> String {
    format!("v{} ({GIT_SHA})", version())
}

pub async fn metadata() -> Json<Value> {
    Json(json!({
        "source_url": source_url(),
        "version": version_label(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stamped_version_wins() {
        assert_eq!(version_from(Some("2609.1.0".into())), "2609.1.0");
    }

    #[test]
    fn surrounding_whitespace_is_trimmed() {
        // A build arg that arrived with a trailing newline must not produce
        // `v2609.1.0\n (sha)` in the UI.
        assert_eq!(version_from(Some("  2609.1.0\n".into())), "2609.1.0");
    }

    #[test]
    fn unset_or_empty_falls_back_to_the_crate_version() {
        assert_eq!(version_from(None), CRATE_VERSION);
        assert_eq!(version_from(Some(String::new())), CRATE_VERSION);
        assert_eq!(version_from(Some("   ".into())), CRATE_VERSION);
    }
}
