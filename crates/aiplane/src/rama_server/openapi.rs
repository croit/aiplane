// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use std::collections::BTreeMap;

use rama::http::service::web::response::Json;
use serde_json::{Map, Value, json};

const ROUTER_SOURCE: &str = include_str!("router.rs");
const METHODS: &[(&str, &str)] = &[
    ("get", "with_get"),
    ("post", "with_post"),
    ("put", "with_put"),
    ("delete", "with_delete"),
    ("patch", "with_patch"),
];

pub async fn document() -> Json<Value> {
    Json(generate(ROUTER_SOURCE))
}

fn generate(router_source: &str) -> Value {
    let mut operations = BTreeMap::<String, Map<String, Value>>::new();
    for &(method, registration) in METHODS {
        for path in registered_paths(router_source, registration) {
            let operation = operation(method, path);
            operations
                .entry(path.to_string())
                .or_default()
                .insert(method.to_string(), operation);
        }
    }
    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "croit AIplane session API",
            "version": env!("CARGO_PKG_VERSION"),
            "description": "Generated from the gateway's registered /api/v0 routes."
        },
        "paths": operations,
    })
}

fn registered_paths<'a>(source: &'a str, registration: &str) -> Vec<&'a str> {
    let needle = format!(".{registration}(");
    let mut paths = Vec::new();
    let mut remaining = source;
    while let Some(position) = remaining.find(&needle) {
        remaining = &remaining[position + needle.len()..];
        let Some(opening_quote) = remaining.find('"') else {
            break;
        };
        let candidate = &remaining[opening_quote + 1..];
        let Some(closing_quote) = candidate.find('"') else {
            break;
        };
        let path = &candidate[..closing_quote];
        if path.starts_with("/api/v0/") {
            paths.push(path);
        }
        remaining = &candidate[closing_quote + 1..];
    }
    paths
}

fn operation(method: &str, path: &str) -> Value {
    let parameters = path_parameters(path);
    let responses = if path == "/api/v0/build" {
        json!({
            "200": { "description": "Successful response" }
        })
    } else {
        json!({
            "200": { "description": "Successful response" },
            "401": { "description": "A valid gateway session is required" }
        })
    };
    let mut value = json!({
        "operationId": operation_id(method, path),
        "responses": responses
    });
    let object = value.as_object_mut().unwrap();
    if !parameters.is_empty() {
        object.insert("parameters".into(), Value::Array(parameters));
    }
    if matches!(method, "post" | "put" | "patch") {
        object.insert(
            "requestBody".into(),
            json!({
                "content": {
                    "application/json": {
                        "schema": {}
                    }
                }
            }),
        );
    }
    value
}

fn path_parameters(path: &str) -> Vec<Value> {
    path.split('/')
        .filter_map(|part| part.strip_prefix('{')?.strip_suffix('}'))
        .map(|name| {
            json!({
                "name": name.trim_start_matches('*'),
                "in": "path",
                "required": true,
                "schema": { "type": "string" }
            })
        })
        .collect()
}

fn operation_id(method: &str, path: &str) -> String {
    let mut id = method.to_string();
    for part in path.split('/').filter(|part| !part.is_empty()) {
        for word in part.trim_matches(['{', '}', '*']).split(['-', '_', '.']) {
            let mut chars = word.chars();
            if let Some(first) = chars.next() {
                id.extend(first.to_uppercase());
                id.extend(chars);
            }
        }
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_routes_and_path_parameters_from_router_declarations() {
        let source = r#"
            .with_get("/api/v0/items/{id}", item)
            .with_get("/api/v0/build", build)
            .with_post(
                "/api/v0/items/{id}/runs/{*tail}",
                run,
            )
            .with_get("/healthz", health)
        "#;
        let document = generate(source);
        assert!(document["paths"]["/api/v0/items/{id}"]["get"].is_object());
        assert_eq!(
            document["paths"]["/api/v0/items/{id}/runs/{*tail}"]["post"]["parameters"][1]["name"],
            "tail"
        );
        assert!(document["paths"]["/healthz"].is_null());
        assert!(
            document["paths"]["/api/v0/build"]["get"]["responses"]["401"].is_null(),
            "the public build metadata route must not advertise a session requirement"
        );
    }

    #[test]
    fn operation_ids_are_stable_and_identifier_safe() {
        assert_eq!(
            operation_id("get", "/api/v0/chat/sessions/{id}/export.pdf"),
            "getApiV0ChatSessionsIdExportPdf"
        );
    }
}
