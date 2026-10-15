// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Client integrations the gateway hands out, so a client's setup always
//! matches the gateway it talks to.

use std::sync::Arc;

use aiplane_runtime::rama_server::state::RamaState;
use rama::http::service::web::extract::State;
use rama::http::{Response, StatusCode, header};

use super::internal;

const OPENCODE_PLUGIN: &str = include_str!("../../../../clients/opencode/aiplane.js");
const GATEWAY_PLACEHOLDER: &str = "\"__AIPLANE_GATEWAY__\"";

/// GET /clients/opencode/aiplane.js — the OpenCode v2 provider plugin, with
/// this gateway's public URL in place of its placeholder.
pub async fn opencode_plugin(State(state): State<Arc<RamaState>>) -> Response {
    let gateway = serde_json::Value::String(state.public_url()).to_string();
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/javascript")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(
            OPENCODE_PLUGIN
                .replacen(GATEWAY_PLACEHOLDER, &gateway, 1)
                .into(),
        )
        .unwrap_or_else(internal)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plugin_source_carries_exactly_one_gateway_placeholder() {
        assert_eq!(OPENCODE_PLUGIN.matches(GATEWAY_PLACEHOLDER).count(), 1);
    }
}
