// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The client integrations the gateway hands out: `/clients/opencode/aiplane.js`.

use crate::common;
use common::Service as _;
use rama::http::{Method, StatusCode};

#[tokio::test]
async fn the_opencode_plugin_is_served_pointing_at_this_gateway() {
    let state = common::state_with_chat_pool("http://unused.invalid").await;
    let public_url = state.public_url();
    let app = common::app(state);

    let resp = app
        .serve(common::req(Method::GET, "/clients/opencode/aiplane.js"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()["content-type"], "text/javascript");
    let body = String::from_utf8(common::read_body(resp).await.to_vec()).unwrap();
    assert!(
        body.contains(&format!("const GATEWAY = \"{public_url}\"")),
        "the plugin must carry this gateway's address"
    );
    assert!(!body.contains("__AIPLANE_GATEWAY__"), "placeholder left in");
    assert!(body.contains("export default plugin(GATEWAY)"));
}
