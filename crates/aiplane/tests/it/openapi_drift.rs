// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use crate::common;

use common::Service as _;
use rama::http::{Method, StatusCode};

#[tokio::test]
async fn backend_serves_generated_openapi_for_every_api_route() {
    let state = common::state_with_chat_pool("http://unused.invalid").await;
    let app = common::app(state);
    let response = app
        .serve(common::req(Method::GET, "/openapi.json"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "application/json"
    );
    let body = common::read_body(response).await;
    let document: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(document["openapi"], "3.1.0");

    let paths = document["paths"].as_object().unwrap();
    assert!(paths.len() > 100);
    assert!(paths["/api/v0/tokens/details"]["get"]["responses"]["200"].is_object());
    assert!(paths["/api/v0/rag/collections/{id}"]["patch"]["responses"]["200"].is_object());
    assert!(paths["/api/v0/chat/sessions/{id}/events"]["get"]["responses"]["200"].is_object());
}

async fn served_document() -> serde_json::Value {
    let state = common::state_with_chat_pool("http://unused.invalid").await;
    let response = common::app(state)
        .serve(common::req(Method::GET, "/openapi.json"))
        .await
        .unwrap();
    serde_json::from_slice(&common::read_body(response).await).unwrap()
}

#[tokio::test]
async fn organization_schedule_and_private_stop_contracts_require_exact_configuration_and_turn() {
    let document = served_document().await;
    let paths = &document["paths"];
    let create = json_body(
        &document,
        &paths["/api/v0/scheduled"]["post"]["requestBody"]["content"],
    );
    assert!(create["properties"]["principal_id"].is_object());
    assert!(create["properties"]["agent_version"].is_object());
    assert!(
        !create["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|field| field == "model")
    );
    for (path, method) in [
        ("/api/v0/scheduled/{id}", "put"),
        ("/api/v0/scheduled/{id}/toggle", "post"),
    ] {
        let operation = &paths[path][method];
        let body = json_body(&document, &operation["requestBody"]["content"]);
        assert!(
            body["required"]
                .as_array()
                .unwrap()
                .iter()
                .any(|field| field == "expected_configuration_generation")
        );
        assert!(operation["responses"]["409"].is_object());
    }
    let stop = &paths["/api/v0/agents/{id}/test/{session}/cancel"]["post"];
    assert_eq!(stop["x-aiplane-access"], "session");
    let body = json_body(&document, &stop["requestBody"]["content"]);
    assert_eq!(body["required"], serde_json::json!(["turn_id"]));
    let reply = json_body(&document, &stop["responses"]["200"]["content"]);
    assert!(reply["properties"]["ok"].is_object());
}

/// The schema a `$ref` names, out of `components/schemas`.
fn resolved<'a>(
    document: &'a serde_json::Value,
    schema: &'a serde_json::Value,
) -> &'a serde_json::Value {
    match schema["$ref"].as_str() {
        Some(reference) => {
            let name = reference.trim_start_matches("#/components/schemas/");
            &document["components"]["schemas"][name]
        }
        None => schema,
    }
}

fn json_body<'a>(
    document: &'a serde_json::Value,
    content: &'a serde_json::Value,
) -> &'a serde_json::Value {
    resolved(document, &content["application/json"]["schema"])
}

#[tokio::test]
async fn every_operation_is_declared_with_its_access_and_responses() {
    let document = served_document().await;
    for (path, item) in document["paths"].as_object().unwrap() {
        for (method, operation) in item.as_object().unwrap() {
            assert!(
                operation["x-aiplane-undeclared"].is_null(),
                "{method} {path} has no declaration"
            );
            assert!(operation["x-aiplane-access"].is_string(), "{method} {path}");
            assert!(operation["security"].is_array(), "{method} {path}");
            let responses = operation["responses"].as_object().unwrap();
            assert!(
                responses.keys().any(|status| status.starts_with('2')),
                "{method} {path} declares no success response"
            );
        }
    }
}

#[tokio::test]
async fn organization_operations_describe_current_typed_configuration_contracts() {
    let document = served_document().await;
    let paths = &document["paths"];
    for (path, method, access, success) in [
        ("/api/v0/admin/organizations", "get", "admin", "200"),
        ("/api/v0/admin/organizations", "post", "admin", "201"),
        ("/api/v0/admin/organizations/{id}", "put", "admin", "200"),
        ("/api/v0/organizations", "get", "session", "200"),
        ("/api/v0/organizations/{id}", "get", "session", "200"),
        ("/api/v0/organizations/{id}/dots", "post", "session", "201"),
        (
            "/api/v0/organizations/{id}/dots/{dot}/active",
            "post",
            "session",
            "200",
        ),
    ] {
        let operation = &paths[path][method];
        assert_eq!(operation["x-aiplane-access"], access, "{method} {path}");
        assert!(
            operation["x-aiplane-undeclared"].is_null(),
            "{method} {path}"
        );
        let reply = json_body(&document, &operation["responses"][success]["content"]);
        assert!(reply["properties"].is_object(), "{method} {path}");
    }

    let edit = &paths["/api/v0/admin/organizations/{id}"]["put"];
    let body = json_body(&document, &edit["requestBody"]["content"]);
    assert_eq!(body["properties"]["expected_revision"]["type"], "string");
    assert_eq!(body["properties"]["member_groups"]["type"], "array");
    assert!(edit["responses"]["409"].is_object());

    let detail = json_body(
        &document,
        &paths["/api/v0/organizations/{id}"]["get"]["responses"]["200"]["content"],
    );
    for field in [
        "organization",
        "dots",
        "active_count",
        "active_limit",
        "can_create_dot",
    ] {
        assert!(detail["properties"].get(field).is_some(), "missing {field}");
    }
    let agent = resolved(&document, &detail["properties"]["dots"]["items"]);
    assert!(agent["properties"].get("dot").is_some());
    let subjects = json_body(
        &document,
        &paths["/api/v0/agents/{id}/share-subjects"]["get"]["responses"]["200"]["content"],
    );
    assert_eq!(subjects["properties"]["truncated"]["type"], "boolean");

    let resources = &paths["/api/v0/agent-resources"]["get"];
    assert!(
        resources["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .any(|parameter| { parameter["name"] == "agent_id" && parameter["in"] == "query" })
    );
    assert!(resources["responses"]["404"].is_object());
}

#[tokio::test]
async fn dot_connection_inventory_declares_authoritative_selection_and_optional_personal_identity()
{
    let document = served_document().await;
    let operation = &document["paths"]["/api/v0/dots/{id}/connections"]["get"];
    let inventory = json_body(&document, &operation["responses"]["200"]["content"]);
    let row = resolved(&document, &inventory["properties"]["connections"]["items"]);
    assert_eq!(row["properties"]["can_select"]["type"], "boolean");
    assert!(
        row["required"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("can_select"))
    );
    let identity = &row["properties"]["user_connection"];
    assert!(
        identity["anyOf"]
            .as_array()
            .unwrap()
            .iter()
            .any(|choice| choice["type"] == "null")
    );
    assert_eq!(operation["x-aiplane-access"], "session");
}

#[tokio::test]
async fn the_groups_save_body_is_the_handlers_own_type() {
    let document = served_document().await;
    let save = &document["paths"]["/api/v0/admin/groups"]["put"];
    let body = json_body(&document, &save["requestBody"]["content"]);
    let properties = body["properties"].as_object().unwrap();
    for field in [
        "name",
        "description",
        "is_admin",
        "is_default",
        "can_manage_agents",
        "oidc_values",
        "tools",
        "skills",
    ] {
        assert!(
            properties.contains_key(field),
            "groups save body lacks `{field}`"
        );
    }
    assert_eq!(body["required"], serde_json::json!(["name"]));
    assert_eq!(save["security"], serde_json::json!([{ "session": [] }]));
    assert_eq!(save["x-aiplane-access"], "admin");
    for status in ["400", "401", "403", "413", "500"] {
        let error = json_body(&document, &save["responses"][status]["content"]);
        assert_eq!(
            error["properties"]["error"]["$ref"], "#/components/schemas/ErrorBody",
            "{status}"
        );
    }
    let error_body = &document["components"]["schemas"]["ErrorBody"];
    assert_eq!(
        error_body["required"],
        serde_json::json!(["message", "type", "code"])
    );

    let list = &document["paths"]["/api/v0/admin/groups"]["get"];
    let view = json_body(&document, &list["responses"]["200"]["content"]);
    let group = resolved(&document, &view["properties"]["groups"]["items"]);
    assert_eq!(group["properties"]["can_manage_agents"]["type"], "boolean");
}

#[tokio::test]
async fn a_chat_submit_takes_json_or_multipart_and_says_where_the_message_went() {
    let document = served_document().await;
    let submit = &document["paths"]["/api/v0/chat/sessions/{id}/messages"]["post"];
    let content = &submit["requestBody"]["content"];
    let body = json_body(&document, content);
    assert_eq!(body["required"], serde_json::json!(["model", "message"]));
    assert_eq!(body["properties"]["voice"]["type"], "boolean");
    let multipart = &content["multipart/form-data"]["schema"];
    assert_eq!(multipart["required"], serde_json::json!(["model"]));
    assert_eq!(
        multipart["properties"]["attachment"]["items"]["format"],
        "binary"
    );

    let placed = json_body(&document, &submit["responses"]["202"]["content"]);
    let placements: Vec<&str> = placed["oneOf"]
        .as_array()
        .unwrap()
        .iter()
        .map(|variant| {
            variant["properties"]["placement"]["const"]
                .as_str()
                .unwrap()
        })
        .collect();
    assert_eq!(placements, ["started", "folded", "queued"]);
    for status in ["400", "401", "404", "409", "429"] {
        assert!(submit["responses"][status].is_object(), "{status}");
    }
}

#[tokio::test]
async fn public_routes_need_no_credential_and_embed_routes_name_theirs() {
    let document = served_document().await;
    let paths = &document["paths"];
    let build = &paths["/api/v0/build"]["get"];
    assert_eq!(build["security"], serde_json::json!([]));
    assert!(build["responses"]["401"].is_null());
    let metadata = json_body(&document, &build["responses"]["200"]["content"]);
    assert_eq!(
        metadata["required"],
        serde_json::json!(["source_url", "version"])
    );

    assert_eq!(
        paths["/api/v0/embed/sessions"]["post"]["x-aiplane-access"],
        "embed_key"
    );
    assert_eq!(
        paths["/api/v0/embed/messages"]["post"]["security"],
        serde_json::json!([{ "visitorToken": [] }])
    );
    assert_eq!(
        paths["/api/v0/setup/state"]["get"]["x-aiplane-access"],
        "setup"
    );
    let schemes = &document["components"]["securitySchemes"];
    assert_eq!(schemes["session"]["in"], "cookie");
    assert_eq!(schemes["visitorToken"]["scheme"], "bearer");
}

#[tokio::test]
async fn the_chat_event_stream_describes_its_frames() {
    let document = served_document().await;
    let events = &document["paths"]["/api/v0/chat/sessions/{id}/events"]["get"];
    let stream = &events["responses"]["200"]["content"]["text/event-stream"];
    let frame = resolved(&document, &stream["x-aiplane-event-data"]);
    let types: Vec<&str> = frame["oneOf"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|variant| variant["properties"]["type"]["const"].as_str())
        .collect();
    for event in ["snapshot", "turn_delta", "turn_finalized", "idle"] {
        assert!(
            types.contains(&event),
            "chat events lack `{event}`: {types:?}"
        );
    }
}

#[test]
fn detached_openapi_document_does_not_exist() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    assert!(!manifest.join("../../docs/openapi.json").exists());
}

#[tokio::test]
async fn private_agent_chat_declares_required_source_and_actual_version_metadata() {
    let document = served_document().await;
    let paths = &document["paths"];
    let send = &paths["/api/v0/agents/{id}/test/messages"]["post"];
    let body = json_body(&document, &send["requestBody"]["content"]);
    assert!(
        body["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|field| field == "source")
    );
    let source = resolved(&document, &body["properties"]["source"]);
    assert_eq!(source["enum"], serde_json::json!(["draft", "published"]));
    for content in [
        &send["responses"]["202"]["content"],
        &paths["/api/v0/agents/{id}/test/{session}/turns/{turn}/debug"]["get"]["responses"]["200"]
            ["content"],
    ] {
        let response = json_body(&document, content);
        assert!(response["properties"]["agent_version"].is_object());
        assert!(response["properties"]["source"].is_object());
        assert!(response["properties"].get("draft_version").is_none());
    }
}
