// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! `/api/v0/admin/classification` — the scheme editor's API: the three scheme
//! states (one level, ISO, BSI), validation, level removal, term lists,
//! marking inspection, the dry-run test and the audit events.

use std::sync::Arc;

use crate::common;
use aiplane::rama_server::RamaState;
use aiplane::rama_server::router::router;
use aiplane_core::server::classification as core;
use common::Service as _;
use rama::http::body::util::BodyExt;
use rama::http::{Body, Method, Request, StatusCode, header};
use serde_json::{Value, json};
use session_core::db as chat;

const BOUNDARY: &str = "----classification-test";

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../aiplane-features/src/server/testdata/classification")
            .join(name),
    )
    .unwrap()
}

async fn setup() -> (Arc<RamaState>, String) {
    let state = common::state_with_admin_rbac("http://unused.invalid").await;
    let cookie = common::seed_session(&state, "boss", "boss@example.com").await;
    aiplane_core::server::db::users::upsert(
        &state.db,
        &aiplane_core::server::db::users::User {
            id: "boss".into(),
            email: "boss@example.com".into(),
            name: None,
            roles: vec!["admin".into()],
            created_at: jiff::Timestamp::now(),
            updated_at: jiff::Timestamp::now(),
            timezone: None,
            speech_voice: None,
        },
    )
    .await
    .unwrap();
    (Arc::new(state), cookie)
}

async fn send(
    state: &Arc<RamaState>,
    cookie: &str,
    method: Method,
    uri: &str,
    content_type: Option<&str>,
    body: Vec<u8>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("cookie", format!("id={cookie}"));
    if let Some(content_type) = content_type {
        builder = builder.header(header::CONTENT_TYPE, content_type);
    }
    let resp = router(state.clone())
        .serve(builder.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

async fn get(state: &Arc<RamaState>, cookie: &str, uri: &str) -> (StatusCode, Value) {
    send(state, cookie, Method::GET, uri, None, Vec::new()).await
}

async fn put(state: &Arc<RamaState>, cookie: &str, body: &Value) -> (StatusCode, Value) {
    send(
        state,
        cookie,
        Method::PUT,
        "/api/v0/admin/classification",
        Some("application/json"),
        serde_json::to_vec(body).unwrap(),
    )
    .await
}

fn multipart_body(parts: &[(&str, Option<&str>, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, filename, data) in parts {
        out.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
        match filename {
            Some(filename) => out.extend_from_slice(
                format!(
                    "Content-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
                )
                .as_bytes(),
            ),
            None => out.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
            ),
        }
        out.extend_from_slice(data);
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    out
}

async fn upload(
    state: &Arc<RamaState>,
    cookie: &str,
    uri: &str,
    parts: &[(&str, Option<&str>, &[u8])],
) -> (StatusCode, Value) {
    send(
        state,
        cookie,
        Method::POST,
        uri,
        Some(&format!("multipart/form-data; boundary={BOUNDARY}")),
        multipart_body(parts),
    )
    .await
}

/// The save body that applies one of the shipped templates on top of the
/// current policy, the way the editor does.
fn template_body(current: &Value, template_id: &str) -> Value {
    let template = current["templates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == template_id)
        .unwrap_or_else(|| panic!("no template {template_id}"))
        .clone();
    let levels = template["levels"].as_array().unwrap();
    let first = levels[0]["id"].clone();
    let last = levels.last().unwrap()["id"].clone();
    let mut policy = current["policy"].clone();
    policy["default_level"] = first.clone();
    policy["link_share_max_level"] = first;
    policy["unknown_label_level"] = last.clone();
    policy["unreadable_level"] = last;
    policy["system_one_model"] = json!("");
    for (key, value) in template["policy"].as_object().unwrap() {
        policy[key] = value.clone();
    }
    json!({
        "levels": template["levels"],
        "classes": template["classes"],
        "markings": template["markings"],
        "assignments": [],
        "policy": policy,
    })
}

async fn apply_template(state: &Arc<RamaState>, cookie: &str, template_id: &str) -> Value {
    let (_, current) = get(state, cookie, "/api/v0/admin/classification").await;
    let (status, saved) = put(state, cookie, &template_body(&current, template_id)).await;
    assert_eq!(status, StatusCode::OK, "{template_id}: {saved}");
    saved
}

#[tokio::test]
async fn a_fresh_install_has_one_level_and_the_editor_can_still_read_it() {
    let (state, cookie) = setup().await;

    let (status, view) = get(&state, &cookie, "/api/v0/admin/classification").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["active"], false);
    assert_eq!(view["levels"].as_array().unwrap().len(), 1);
    let ids: Vec<&str> = view["templates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["iso", "tisax", "bsi", "tlp"]);
    let destinations = view["destinations"].as_array().unwrap();
    let pool = destinations
        .iter()
        .find(|d| d["kind"] == "pool" && d["id"] == "pool")
        .expect("every pool is a destination");
    assert_eq!(pool["egress"], false);
    assert_eq!(pool["over"], "hide");
}

#[tokio::test]
async fn the_endpoints_are_admin_only() {
    let (state, _) = setup().await;
    let anonymous = router(state.clone())
        .serve(
            Request::builder()
                .uri("/api/v0/admin/classification")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);

    let user_cookie = common::seed_session(&state, "alice", "alice@example.com").await;
    let (status, _) = get(&state, &user_cookie, "/api/v0/admin/classification").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn saving_the_iso_template_activates_a_four_level_scheme() {
    let (state, cookie) = setup().await;

    let saved = apply_template(&state, &cookie, "iso").await;

    assert_eq!(saved["active"], true);
    let ids: Vec<&str> = saved["levels"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        [
            "public",
            "internal",
            "confidential",
            "strictly_confidential"
        ]
    );
    assert!(state.scheme().is_active());
    let (_, again) = get(&state, &cookie, "/api/v0/admin/classification").await;
    assert_eq!(again["levels"], saved["levels"]);
    assert_eq!(again["classes"].as_array().unwrap().len(), 5);
}

#[tokio::test]
async fn the_templates_arrive_in_the_language_of_the_request() {
    let (state, cookie) = setup().await;
    let request = Request::builder()
        .uri("/api/v0/admin/classification")
        .header("cookie", format!("id={cookie}"))
        .header("accept-language", "de")
        .body(Body::empty())
        .unwrap();

    let resp = router(state.clone()).serve(request).await.unwrap();

    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let view: Value = serde_json::from_slice(&bytes).unwrap();
    let bsi = view["templates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == "bsi")
        .unwrap();
    assert_eq!(bsi["name"], "BSI IT-Grundschutz Schutzbedarf");
    assert_eq!(bsi["levels"][1]["name"], "hoch");
    assert_eq!(
        bsi["classes"][0]["rule_question"],
        "Is a specific, identifiable person described in this text?"
    );
}

#[tokio::test]
async fn saving_the_bsi_template_gives_normal_high_very_high() {
    let (state, cookie) = setup().await;

    let saved = apply_template(&state, &cookie, "bsi").await;

    let ids: Vec<&str> = saved["levels"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["normal", "high", "very_high"]);
    assert_eq!(saved["policy"]["default_level"], "normal");
}

#[tokio::test]
async fn users_may_override_the_model_clearance_by_default_and_the_choice_round_trips() {
    let (state, cookie) = setup().await;
    let (_, current) = get(&state, &cookie, "/api/v0/admin/classification").await;
    assert_eq!(current["policy"]["user_override"], true);

    let mut body = template_body(&current, "iso");
    body["policy"]["user_override"] = json!(false);
    let (status, _) = put(&state, &cookie, &body).await;
    assert_eq!(status, StatusCode::OK);

    let (_, saved) = get(&state, &cookie, "/api/v0/admin/classification").await;
    assert_eq!(saved["policy"]["user_override"], false);
}

#[tokio::test]
async fn a_refused_save_names_the_field_and_changes_nothing() {
    let (state, cookie) = setup().await;
    let (_, current) = get(&state, &cookie, "/api/v0/admin/classification").await;

    let mut body = template_body(&current, "iso");
    body["levels"][2]["rank"] = json!(9);
    let (status, error) = put(&state, &cookie, &body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["error"]["field"], "levels");

    let mut body = template_body(&current, "iso");
    body["classes"][0]["level_id"] = json!("nonexistent");
    let (status, error) = put(&state, &cookie, &body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        error["error"]["field"]
            .as_str()
            .unwrap()
            .starts_with("classes")
    );

    let mut body = template_body(&current, "iso");
    body["policy"]["link_share_max_level"] = json!("nonexistent");
    let (status, _) = put(&state, &cookie, &body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let mut body = template_body(&current, "iso");
    body["assignments"] = json!([
        {"kind": "pool", "id": "no-such-pool", "level_id": "internal", "over": "hide"}
    ]);
    let (status, error) = put(&state, &cookie, &body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        error["error"]["field"]
            .as_str()
            .unwrap()
            .starts_with("assignments")
    );

    let (_, after) = get(&state, &cookie, "/api/v0/admin/classification").await;
    assert_eq!(
        after["active"], false,
        "a refused save leaves the scheme alone"
    );
}

#[tokio::test]
async fn a_pool_assignment_rates_the_pool_at_once() {
    let (state, cookie) = setup().await;
    let (_, current) = get(&state, &cookie, "/api/v0/admin/classification").await;
    let mut body = template_body(&current, "iso");
    body["assignments"] = json!([
        {"kind": "pool", "id": "pool", "level_id": "confidential", "over": "hide"}
    ]);

    let (status, saved) = put(&state, &cookie, &body).await;

    assert_eq!(status, StatusCode::OK, "{saved}");
    let pool = saved["destinations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["kind"] == "pool" && d["id"] == "pool")
        .unwrap();
    assert_eq!(pool["level_id"], "confidential");
    let live = state.upstreams.pools();
    let live = live.iter().find(|p| p.name == "pool").unwrap();
    assert_eq!(live.level_rank(), 2);
}

#[tokio::test]
async fn removing_a_level_moves_its_conversations_up_and_audits_it() {
    let (state, cookie) = setup().await;
    apply_template(&state, &cookie, "iso").await;
    let internal = chat::create_session(&state.db, "boss").await.unwrap();
    let confidential = chat::create_session(&state.db, "boss").await.unwrap();
    core::set_level(&state.db, &internal.id, "internal")
        .await
        .unwrap();
    core::set_level(&state.db, &confidential.id, "confidential")
        .await
        .unwrap();

    let (_, current) = get(&state, &cookie, "/api/v0/admin/classification").await;
    let mut body = json!({
        "levels": current["levels"],
        "classes": current["classes"],
        "markings": current["markings"],
        "assignments": [],
        "policy": current["policy"],
    });
    for key in ["default_level", "link_share_max_level"] {
        if body["policy"][key] == "internal" {
            body["policy"][key] = json!("public");
        }
    }
    body["levels"]
        .as_array_mut()
        .unwrap()
        .retain(|l| l["id"] != "internal");
    for (rank, level) in body["levels"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        level["rank"] = json!(rank);
    }
    for class in body["classes"].as_array_mut().unwrap() {
        if class["level_id"] == "internal" {
            class["level_id"] = json!("confidential");
        }
    }
    body["markings"]
        .as_array_mut()
        .unwrap()
        .retain(|m| m["level_id"] != "internal");
    let (status, saved) = put(&state, &cookie, &body).await;
    assert_eq!(status, StatusCode::OK, "{saved}");

    assert_eq!(
        core::level_id(&state.db, &internal.id)
            .await
            .unwrap()
            .as_deref(),
        Some("confidential"),
        "the conversation moves to the next level up"
    );
    assert_eq!(
        core::level_id(&state.db, &confidential.id)
            .await
            .unwrap()
            .as_deref(),
        Some("confidential")
    );
    let (_, events) = get(&state, &cookie, "/api/v0/admin/classification/events").await;
    let moved: Vec<&Value> = events["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["cause"] == "scheme_change")
        .collect();
    assert_eq!(moved.len(), 1);
    assert_eq!(moved[0]["session_id"], internal.id);
    assert_eq!(moved[0]["from_level"], "internal");
    assert_eq!(moved[0]["to_level"], "confidential");
}

#[tokio::test]
async fn a_term_list_is_previewed_replaced_and_takes_effect_at_once() {
    let (state, cookie) = setup().await;
    apply_template(&state, &cookie, "iso").await;
    let uri = "/api/v0/admin/classification/classes/trade_secrets/terms";

    let (status, first) = upload(
        &state,
        &cookie,
        uri,
        &[(
            "file",
            Some("terms.txt"),
            b"Project Falcon\nProject Falcon\nab\nOperation Heron\n",
        )],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["count"], 2);
    assert_eq!(first["duplicates"], 1);
    assert_eq!(first["too_short"], json!(["ab"]));

    let (_, view) = get(&state, &cookie, "/api/v0/admin/classification").await;
    let class = view["classes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "trade_secrets")
        .unwrap();
    assert_eq!(class["terms"]["count"], 2);

    let (_, tested) = upload(
        &state,
        &cookie,
        "/api/v0/admin/classification/test",
        &[("text", None, b"The status of project falcon is green.")],
    )
    .await;
    assert_eq!(tested["classes"], json!(["trade_secrets"]));

    let (status, _) = upload(
        &state,
        &cookie,
        uri,
        &[("file", Some("terms.txt"), b"Operation Heron\n")],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, tested) = upload(
        &state,
        &cookie,
        "/api/v0/admin/classification/test",
        &[("text", None, b"The status of project falcon is green.")],
    )
    .await;
    assert_eq!(
        tested["classes"],
        json!([]),
        "the replaced list no longer matches"
    );

    let (status, _) = send(&state, &cookie, Method::DELETE, uri, None, Vec::new()).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = upload(
        &state,
        &cookie,
        "/api/v0/admin/classification/classes/nonexistent/terms",
        &[("file", Some("terms.txt"), b"Operation Heron\n")],
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_terms_of_a_kept_class_survive_a_save() {
    let (state, cookie) = setup().await;
    apply_template(&state, &cookie, "iso").await;
    upload(
        &state,
        &cookie,
        "/api/v0/admin/classification/classes/trade_secrets/terms",
        &[("file", Some("terms.txt"), b"Project Falcon\n")],
    )
    .await;

    apply_template(&state, &cookie, "iso").await;

    let (_, view) = get(&state, &cookie, "/api/v0/admin/classification").await;
    let class = view["classes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "trade_secrets")
        .unwrap();
    assert_eq!(class["terms"]["count"], 1);
}

#[tokio::test]
async fn inspect_reads_the_markings_of_a_labelled_file() {
    let (state, cookie) = setup().await;
    for (name, file) in [
        ("mip_confidential.docx", fixture("mip_confidential.docx")),
        ("msip_confidential.eml", fixture("msip_confidential.eml")),
    ] {
        let (status, found) = upload(
            &state,
            &cookie,
            "/api/v0/admin/classification/inspect",
            &[("file", Some(name), &file)],
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{name}: {found}");
        assert_eq!(found["unreadable"], false);
        let markings = found["markings"].as_array().unwrap();
        assert_eq!(markings.len(), 1, "{name}: {found}");
        assert_eq!(markings[0]["kind"], "mip");
        assert_eq!(markings[0]["value"], "9fbde396-1a24-4c79-8edf-9254a0f35055");
        assert_eq!(markings[0]["name"], "Confidential");
    }
}

#[tokio::test]
async fn test_classifies_a_stamped_file_without_writing_anything() {
    let (state, cookie) = setup().await;
    apply_template(&state, &cookie, "tisax").await;
    let before = chat::list_sessions(&state.db, "boss").await.unwrap().len();

    let (status, result) = upload(
        &state,
        &cookie,
        "/api/v0/admin/classification/test",
        &[(
            "file",
            Some("vertraulich_stamp.txt"),
            &fixture("vertraulich_stamp.txt"),
        )],
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["level_id"], "confidential");
    assert!(
        result["causes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c.as_str().unwrap().starts_with("marking:stamp")),
        "{result}"
    );
    let (_, events) = get(&state, &cookie, "/api/v0/admin/classification/events").await;
    assert_eq!(events["events"], json!([]));
    assert_eq!(
        chat::list_sessions(&state.db, "boss").await.unwrap().len(),
        before
    );
}

#[tokio::test]
async fn the_settings_editor_no_longer_lists_the_classification_section() {
    let (state, cookie) = setup().await;
    let (status, settings) = get(&state, &cookie, "/api/v0/admin/settings").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        !settings.to_string().contains("classification."),
        "{settings}"
    );
}

#[tokio::test]
async fn a_pool_on_a_loopback_backend_is_in_the_own_network() {
    let upstream = wiremock::MockServer::start().await;
    let state = common::state_with_admin_rbac(&upstream.uri()).await;
    let cookie = common::seed_session(&state, "boss", "boss@example.com").await;
    aiplane_core::server::db::users::upsert(
        &state.db,
        &aiplane_core::server::db::users::User {
            id: "boss".into(),
            email: "boss@example.com".into(),
            name: None,
            roles: vec!["admin".into()],
            created_at: jiff::Timestamp::now(),
            updated_at: jiff::Timestamp::now(),
            timezone: None,
            speech_voice: None,
        },
    )
    .await
    .unwrap();
    let state = Arc::new(state);

    let (status, view) = get(&state, &cookie, "/api/v0/admin/classification").await;

    assert_eq!(status, StatusCode::OK);
    let pool = view["destinations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["kind"] == "pool" && d["id"] == "pool")
        .unwrap();
    assert_eq!(pool["hosting"], "own_network");
    assert_eq!(pool["hosts"], json!(["127.0.0.1"]));
}
