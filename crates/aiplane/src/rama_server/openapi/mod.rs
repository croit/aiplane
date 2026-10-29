// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! `GET /openapi.json` — the OpenAPI 3.1 contract of the `/api/v0` session API.
//!
//! Two sources, checked against each other. The routes come from scanning
//! `router.rs` itself, so the document lists exactly what is mounted. What
//! each route takes and answers comes from its [`Op`] declaration in the
//! area modules below, whose schemas are derived (`schemars`) from the very
//! types the handler deserializes and serializes. A route without a
//! declaration, or a declaration without a route, fails
//! `every_registered_route_is_declared_exactly_once`.
//!
//! An operation whose payload has no wire type to derive from is declared
//! [`Op::unsupported`] with the reason; the document carries the reason as
//! `x-aiplane-schema-unsupported`, and `docs/ui.md` lists every such route.

use std::collections::{BTreeMap, BTreeSet};

use rama::http::service::web::response::Json;
use schemars::generate::{SchemaGenerator, SchemaSettings};
use schemars::{JsonSchema, Schema};
use serde_json::{Map, Value, json};

mod account;
mod admin;
mod agents;
mod chat;
mod rag;
mod workspace;

const ROUTER_SOURCE: &str = include_str!("../router.rs");
const METHODS: &[(&str, &str)] = &[
    ("get", "with_get"),
    ("post", "with_post"),
    ("put", "with_put"),
    ("delete", "with_delete"),
    ("patch", "with_patch"),
];
const SCHEMAS: &str = "#/components/schemas/";
/// Where the response generator files its definitions until they are merged
/// into [`SCHEMAS`]; see [`merge_definitions`].
const REPLY_SCHEMAS: &str = "#/components/replySchemas/";

pub async fn document() -> Json<Value> {
    Json(generate(ROUTER_SOURCE, &operations()))
}

fn operations() -> Vec<Op> {
    [
        account::operations(),
        admin::operations(),
        agents::operations(),
        chat::operations(),
        rag::operations(),
        workspace::operations(),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Who may call an operation, and with which credential.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Access {
    /// No credential.
    Public,
    /// The signed-in session cookie.
    Session,
    /// The session cookie of a user holding an admin role.
    Admin,
    /// No credential, but the body names an embed key (`gwe_…`) and the
    /// request must come from an `Origin` that key lists.
    EmbedKey,
    /// The visitor token (`gwv_…`) an embed session handed out, as
    /// `Authorization: Bearer`, from an `Origin` the embed key lists.
    Visitor,
    /// Open on a first run; during a recovery window, the one-time claim
    /// `restore-setup` printed, as `?claim=` or the setup cookie.
    Setup,
}

impl Access {
    fn name(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Session => "session",
            Self::Admin => "admin",
            Self::EmbedKey => "embed_key",
            Self::Visitor => "visitor",
            Self::Setup => "setup",
        }
    }

    fn security(self) -> Value {
        match self {
            Self::Public | Self::EmbedKey => json!([]),
            Self::Session | Self::Admin => json!([{ "session": [] }]),
            Self::Visitor => json!([{ "visitorToken": [] }]),
            Self::Setup => json!([{}, { "setupClaim": [] }, { "setupClaimCookie": [] }]),
        }
    }

    /// The refusals the credential check itself produces, before the handler
    /// does anything of its own.
    fn errors(self) -> &'static [u16] {
        match self {
            Self::Public | Self::EmbedKey | Self::Setup => &[],
            Self::Session => &[401, 500],
            Self::Admin => &[401, 403, 500],
            Self::Visitor => &[401],
        }
    }
}

type SchemaFn = fn(&mut SchemaGenerator) -> Schema;

fn schema_of<T: JsonSchema>(generator: &mut SchemaGenerator) -> Schema {
    generator.subschema_for::<T>()
}

/// One media type of a request or response body.
pub(crate) enum Content {
    /// `application/json`, shaped like `T`.
    Json(SchemaFn),
    /// `text/event-stream` whose every `data:` line is one JSON `T`.
    Sse(SchemaFn),
    /// `multipart/form-data` with these parts.
    Multipart(&'static [Part]),
    /// Opaque bytes of this media type (a PDF, an image, an archive).
    Bytes(&'static str),
    /// Text of this media type (Markdown, JavaScript).
    Text(&'static str),
}

pub(crate) fn json<T: JsonSchema>() -> Content {
    Content::Json(schema_of::<T>)
}

pub(crate) fn sse<T: JsonSchema>() -> Content {
    Content::Sse(schema_of::<T>)
}

/// One part of a `multipart/form-data` body, as the handler's parser reads it.
pub(crate) struct Part {
    pub name: &'static str,
    pub kind: PartKind,
    pub required: bool,
}

#[derive(Clone, Copy)]
pub(crate) enum PartKind {
    Text,
    File,
    /// A file part that may repeat.
    Files,
}

/// The type of a query parameter a handler reads by hand.
#[derive(Clone, Copy)]
pub(crate) enum Scalar {
    String,
    Integer,
    Boolean,
}

enum Query {
    /// Every field of `T` is one parameter (a `Query<T>` extractor).
    Typed(SchemaFn),
    Param {
        name: &'static str,
        kind: Scalar,
        required: bool,
    },
}

/// The declaration of one `/api/v0` operation.
pub(crate) struct Op {
    method: &'static str,
    path: &'static str,
    access: Access,
    query: Vec<Query>,
    request: Vec<Content>,
    replies: Vec<(u16, Option<Content>)>,
    errors: Vec<u16>,
    unsupported: Option<&'static str>,
}

fn op(method: &'static str, path: &'static str, access: Access) -> Op {
    Op {
        method,
        path,
        access,
        query: Vec::new(),
        request: Vec::new(),
        replies: Vec::new(),
        errors: Vec::new(),
        unsupported: None,
    }
}

pub(crate) fn get(path: &'static str, access: Access) -> Op {
    op("get", path, access)
}

pub(crate) fn post(path: &'static str, access: Access) -> Op {
    op("post", path, access)
}

pub(crate) fn put(path: &'static str, access: Access) -> Op {
    op("put", path, access)
}

pub(crate) fn patch(path: &'static str, access: Access) -> Op {
    op("patch", path, access)
}

pub(crate) fn delete(path: &'static str, access: Access) -> Op {
    op("delete", path, access)
}

impl Op {
    /// The fields of `T` are the query parameters (the handler extracts
    /// `Query<T>` or parses its query string into `T`).
    pub(crate) fn query<T: JsonSchema>(mut self) -> Self {
        self.query.push(Query::Typed(schema_of::<T>));
        self
    }

    /// One query parameter the handler reads by name.
    pub(crate) fn param(mut self, name: &'static str, kind: Scalar, required: bool) -> Self {
        self.query.push(Query::Param {
            name,
            kind,
            required,
        });
        self
    }

    /// A request body media type. Call once per type the handler accepts.
    pub(crate) fn body(mut self, content: Content) -> Self {
        self.request.push(content);
        self
    }

    pub(crate) fn ok(self, content: Content) -> Self {
        self.reply(200, content)
    }

    pub(crate) fn created(self, content: Content) -> Self {
        self.reply(201, content)
    }

    pub(crate) fn accepted(self, content: Content) -> Self {
        self.reply(202, content)
    }

    pub(crate) fn no_content(self) -> Self {
        self.empty(204)
    }

    /// A success status whose response has no body (a redirect, a 204).
    pub(crate) fn empty(mut self, status: u16) -> Self {
        self.replies.push((status, None));
        self
    }

    /// A success status other than the usual ones, or a second media type
    /// for one already declared.
    pub(crate) fn reply(mut self, status: u16, content: Content) -> Self {
        self.replies.push((status, Some(content)));
        self
    }

    /// The handler's own refusals, each answered with the error envelope.
    /// The credential check's refusals come from [`Access`] and need not be
    /// repeated.
    pub(crate) fn errors(mut self, statuses: &[u16]) -> Self {
        self.errors.extend_from_slice(statuses);
        self
    }

    /// This operation's payload has no wire type the document could be
    /// derived from; `reason` says why.
    pub(crate) fn unsupported(mut self, reason: &'static str) -> Self {
        self.unsupported = Some(reason);
        self
    }
}

fn status_description(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted; the work continues in the background",
        204 => "Done; no body",
        302 | 303 => "Redirect; see the `Location` header",
        400 => "The request is malformed or failed validation; `error.message` says which part",
        401 => "No valid credential for this route",
        403 => "The caller is identified but not allowed to do this",
        404 => "The addressed resource does not exist or is not the caller's",
        409 => "The request conflicts with the resource's current state",
        413 => "The request body is larger than this route reads",
        415 => "The request body's media type is not one this route reads",
        422 => "The request is well-formed but cannot be carried out",
        429 => "A rate limit or quota refused the request",
        500 => "The gateway failed while handling the request",
        502 => "An upstream service the request depends on failed",
        503 => "A service the request depends on is not configured or not available",
        504 => "An upstream service the request depends on timed out",
        _ => "See `error.message`",
    }
}

struct Generators {
    request: SchemaGenerator,
    reply: SchemaGenerator,
}

impl Generators {
    fn new() -> Self {
        let settings = |path: &str| {
            SchemaSettings::draft2020_12().with(|s| {
                s.definitions_path = path.trim_start_matches('#').to_string().into();
                s.meta_schema = None;
            })
        };
        Self {
            request: settings(SCHEMAS).for_deserialize().into_generator(),
            reply: settings(REPLY_SCHEMAS).for_serialize().into_generator(),
        }
    }
}

fn generate(router_source: &str, declarations: &[Op]) -> Value {
    let by_route: BTreeMap<(&str, &str), &Op> = declarations
        .iter()
        .map(|op| ((op.method, op.path), op))
        .collect();
    let mut generators = Generators::new();
    let error = generators
        .reply
        .subschema_for::<shared::api::ErrorEnvelope>();
    let mut paths = BTreeMap::<String, Map<String, Value>>::new();
    for (method, path) in registered_routes(router_source) {
        let operation = match by_route.get(&(method, path)) {
            Some(declared) => operation(declared, &mut generators, &error),
            None => undeclared(method, path),
        };
        paths
            .entry(path.to_string())
            .or_default()
            .insert(method.to_string(), operation);
    }
    let schemas = merge_definitions(&mut generators);
    let mut document = json!({
        "openapi": "3.1.0",
        "info": {
            "title": "croit AIplane session API",
            "version": env!("CARGO_PKG_VERSION"),
            "description": "The /api/v0 JSON API the AIplane web app is built on. \
                Routes are read from the gateway's router; request and response \
                schemas are derived from the handlers' own wire types. \
                `x-aiplane-access` names the credential each operation needs."
        },
        "paths": paths,
        "components": {
            "schemas": schemas.definitions,
            "securitySchemes": security_schemes(),
        },
    });
    rewrite_refs(&mut document, &schemas.renamed);
    document
}

fn security_schemes() -> Value {
    json!({
        "session": {
            "type": "apiKey",
            "in": "cookie",
            "name": aiplane_core::rama_server::session::COOKIE_NAME,
            "description": "The signed session cookie the browser sign-in (`/auth/login`) sets."
        },
        "visitorToken": {
            "type": "http",
            "scheme": "bearer",
            "description": "The visitor token (`gwv_…`) `POST /api/v0/embed/sessions` returns. \
                Accepted on `/api/v0/embed/*` only."
        },
        "setupClaim": {
            "type": "apiKey",
            "in": "query",
            "name": "claim",
            "description": "The one-time token `restore-setup` printed; needed only while a \
                recovery window is open."
        },
        "setupClaimCookie": {
            "type": "apiKey",
            "in": "cookie",
            "name": crate::rama_server::setup_api::SETUP_CLAIM_COOKIE,
            "description": "Set by the setup API after a valid `?claim=`, so the claim is \
                presented once."
        },
    })
}

fn operation(declared: &Op, generators: &mut Generators, error: &Schema) -> Value {
    let mut object = Map::new();
    object.insert(
        "operationId".into(),
        operation_id(declared.method, declared.path).into(),
    );
    object.insert("x-aiplane-access".into(), declared.access.name().into());
    object.insert("security".into(), declared.access.security());
    let mut parameters = path_parameters(declared.path);
    for query in &declared.query {
        parameters.extend(query_parameters(query, &mut generators.request));
    }
    if !parameters.is_empty() {
        object.insert("parameters".into(), Value::Array(parameters));
    }
    if !declared.request.is_empty() {
        let content = content_map(&declared.request, &mut generators.request);
        object.insert(
            "requestBody".into(),
            json!({ "required": true, "content": content }),
        );
    }
    let mut responses = Map::new();
    let mut by_status = BTreeMap::<u16, Vec<&Content>>::new();
    for (status, content) in &declared.replies {
        let entry = by_status.entry(*status).or_default();
        if let Some(content) = content {
            entry.push(content);
        }
    }
    for (status, contents) in by_status {
        let mut response = json!({ "description": status_description(status) });
        if !contents.is_empty() {
            response["content"] = content_map(contents, &mut generators.reply);
        }
        responses.insert(status.to_string(), response);
    }
    // Every body is read through a cap: `BodyLimitLayer`, or the handler's
    // own on the public routes. Either refuses with 413.
    let body_cap: &[u16] = if declared.request.is_empty() {
        &[]
    } else {
        &[413]
    };
    let errors: BTreeSet<u16> = declared
        .access
        .errors()
        .iter()
        .chain(&declared.errors)
        .chain(body_cap)
        .copied()
        .collect();
    let error_content = json!({ "application/json": { "schema": error } });
    for status in errors {
        responses.insert(
            status.to_string(),
            json!({
                "description": status_description(status),
                "content": error_content,
            }),
        );
    }
    object.insert("responses".into(), Value::Object(responses));
    if let Some(reason) = declared.unsupported {
        object.insert("x-aiplane-schema-unsupported".into(), reason.into());
    }
    Value::Object(object)
}

fn undeclared(method: &str, path: &str) -> Value {
    let mut object = Map::new();
    object.insert("operationId".into(), operation_id(method, path).into());
    let parameters = path_parameters(path);
    if !parameters.is_empty() {
        object.insert("parameters".into(), Value::Array(parameters));
    }
    object.insert("responses".into(), json!({}));
    object.insert("x-aiplane-undeclared".into(), true.into());
    Value::Object(object)
}

fn content_map<'a>(
    contents: impl IntoIterator<Item = &'a Content>,
    generator: &mut SchemaGenerator,
) -> Value {
    let mut map = Map::new();
    for content in contents {
        let (media_type, value) = match content {
            Content::Json(schema) => ("application/json", json!({ "schema": schema(generator) })),
            Content::Sse(schema) => (
                "text/event-stream",
                json!({
                    "schema": {
                        "type": "string",
                        "description": "Server-sent events. Each event's `data:` line is one \
                            JSON document matching `x-aiplane-event-data`."
                    },
                    "x-aiplane-event-data": schema(generator),
                }),
            ),
            Content::Multipart(parts) => ("multipart/form-data", multipart_schema(parts)),
            Content::Bytes(media_type) => (
                *media_type,
                json!({ "schema": { "type": "string", "format": "binary" } }),
            ),
            Content::Text(media_type) => (*media_type, json!({ "schema": { "type": "string" } })),
        };
        map.insert(media_type.to_string(), value);
    }
    Value::Object(map)
}

fn multipart_schema(parts: &[Part]) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for part in parts {
        let schema = match part.kind {
            PartKind::Text => json!({ "type": "string" }),
            PartKind::File => json!({ "type": "string", "format": "binary" }),
            PartKind::Files => json!({
                "type": "array",
                "items": { "type": "string", "format": "binary" }
            }),
        };
        properties.insert(part.name.to_string(), schema);
        if part.required {
            required.push(part.name);
        }
    }
    json!({
        "schema": {
            "type": "object",
            "properties": properties,
            "required": required,
        }
    })
}

fn query_parameters(query: &Query, generator: &mut SchemaGenerator) -> Vec<Value> {
    match query {
        Query::Param {
            name,
            kind,
            required,
        } => {
            let kind = match kind {
                Scalar::String => "string",
                Scalar::Integer => "integer",
                Scalar::Boolean => "boolean",
            };
            vec![json!({
                "name": name,
                "in": "query",
                "required": required,
                "schema": { "type": kind }
            })]
        }
        Query::Typed(schema) => {
            let reference = schema(generator).to_value();
            let resolved = resolve(&reference, generator);
            let required: BTreeSet<&str> = resolved["required"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            resolved["properties"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(name, schema)| {
                    let mut parameter = json!({
                        "name": name,
                        "in": "query",
                        "required": required.contains(name.as_str()),
                        "schema": schema,
                    });
                    if let Some(description) = schema.get("description") {
                        parameter["description"] = description.clone();
                    }
                    parameter
                })
                .collect()
        }
    }
}

/// The schema a `$ref` (into the generator's definitions) points at.
fn resolve(schema: &Value, generator: &SchemaGenerator) -> Value {
    match schema["$ref"].as_str() {
        Some(reference) => {
            let name = reference.rsplit('/').next().unwrap_or_default();
            generator
                .definitions()
                .get(name)
                .cloned()
                .unwrap_or(Value::Null)
        }
        None => schema.clone(),
    }
}

struct Merged {
    definitions: Map<String, Value>,
    /// Reply-side definition name → its name under `components/schemas`.
    renamed: BTreeMap<String, String>,
}

/// Fold the response generator's definitions into the request generator's.
///
/// A type a handler both reads and writes can have two schemas — a
/// `#[serde(default)]` field is optional to read but always written — so the
/// two contracts are generated separately. Where they agree they share one
/// name; where they differ the reply side gets a `Reply` suffix.
fn merge_definitions(generators: &mut Generators) -> Merged {
    merge(
        generators.request.take_definitions(true),
        generators.reply.take_definitions(true),
    )
}

/// Each side's schemas point at their own namespace, so two definitions are
/// compared only after the reply's references are rewritten with the names
/// settled so far. A rename can make a definition that refers to it differ in
/// turn, so the pass repeats until no name changes; names only ever move from
/// shared to suffixed, which bounds it.
fn merge(mut definitions: Map<String, Value>, replies: Map<String, Value>) -> Merged {
    let mut renamed: BTreeMap<String, String> = replies
        .keys()
        .map(|name| (name.clone(), name.clone()))
        .collect();
    loop {
        let mut changed = false;
        for (name, schema) in &replies {
            if renamed[name] != *name {
                continue;
            }
            let mut rewritten = schema.clone();
            rewrite_refs(&mut rewritten, &renamed);
            if definitions
                .get(name)
                .is_some_and(|existing| *existing != rewritten)
            {
                let mut final_name = format!("{name}Reply");
                let mut n = 2;
                while definitions.contains_key(&final_name) || replies.contains_key(&final_name) {
                    final_name = format!("{name}Reply{n}");
                    n += 1;
                }
                renamed.insert(name.clone(), final_name);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    for (name, mut schema) in replies {
        rewrite_refs(&mut schema, &renamed);
        let final_name = renamed[&name].clone();
        definitions.entry(final_name).or_insert(schema);
    }
    Merged {
        definitions,
        renamed,
    }
}

fn rewrite_refs(value: &mut Value, renamed: &BTreeMap<String, String>) {
    match value {
        Value::Object(object) => {
            if let Some(Value::String(reference)) = object.get_mut("$ref")
                && let Some(name) = reference.strip_prefix(REPLY_SCHEMAS)
            {
                let name = renamed.get(name).map(String::as_str).unwrap_or(name);
                *reference = format!("{SCHEMAS}{name}");
            }
            for child in object.values_mut() {
                rewrite_refs(child, renamed);
            }
        }
        Value::Array(items) => {
            for child in items {
                rewrite_refs(child, renamed);
            }
        }
        _ => {}
    }
}

fn registered_routes(source: &str) -> Vec<(&'static str, &str)> {
    METHODS
        .iter()
        .flat_map(|&(method, registration)| {
            registered_paths(source, registration)
                .into_iter()
                .map(move |path| (method, path))
        })
        .collect()
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

    #[derive(serde::Deserialize, serde::Serialize, JsonSchema)]
    struct Item {
        name: String,
        #[serde(default)]
        tags: Vec<String>,
    }

    #[derive(JsonSchema)]
    #[expect(dead_code, reason = "only its schema is used")]
    struct Filter {
        /// Only items whose name contains this.
        q: Option<String>,
    }

    fn sample_router() -> &'static str {
        r#"
            .with_get("/api/v0/items/{id}", item)
            .with_put("/api/v0/items/{id}", save)
            .with_get("/api/v0/build", build)
            .with_post(
                "/api/v0/items/{id}/runs/{*tail}",
                run,
            )
            .with_get("/api/v0/stray", stray)
            .with_get("/healthz", health)
        "#
    }

    fn sample_declarations() -> Vec<Op> {
        vec![
            get("/api/v0/items/{id}", Access::Session)
                .query::<Filter>()
                .ok(json::<Item>())
                .errors(&[404]),
            put("/api/v0/items/{id}", Access::Admin)
                .body(json::<Item>())
                .no_content()
                .errors(&[400]),
            get("/api/v0/build", Access::Public).ok(json::<Item>()),
            post("/api/v0/items/{id}/runs/{*tail}", Access::Visitor)
                .body(Content::Multipart(&[Part {
                    name: "file",
                    kind: PartKind::File,
                    required: true,
                }]))
                .ok(sse::<Item>())
                .unsupported("the run's frames are built by hand"),
        ]
    }

    #[test]
    fn routes_come_from_the_router_and_payloads_from_the_declarations() {
        let document = generate(sample_router(), &sample_declarations());
        let paths = &document["paths"];
        assert!(paths["/healthz"].is_null());
        assert_eq!(
            paths["/api/v0/items/{id}/runs/{*tail}"]["post"]["parameters"][1]["name"],
            "tail"
        );
        assert_eq!(paths["/api/v0/stray"]["get"]["x-aiplane-undeclared"], true);

        let read = &paths["/api/v0/items/{id}"]["get"];
        assert_eq!(read["security"], json!([{ "session": [] }]));
        assert_eq!(read["parameters"][1]["name"], "q");
        assert_eq!(read["parameters"][1]["required"], false);
        assert_eq!(
            read["parameters"][1]["description"],
            "Only items whose name contains this."
        );
        for status in ["401", "404", "500"] {
            assert_eq!(
                read["responses"][status]["content"]["application/json"]["schema"]["$ref"],
                "#/components/schemas/ErrorEnvelope",
                "{status}"
            );
        }
        assert!(read["responses"]["403"].is_null());

        let build = &paths["/api/v0/build"]["get"];
        assert_eq!(build["security"], json!([]));
        assert!(build["responses"]["401"].is_null());

        let save = &paths["/api/v0/items/{id}"]["put"];
        assert!(save["responses"]["403"].is_object());
        assert!(save["responses"]["204"]["content"].is_null());

        let run = &paths["/api/v0/items/{id}/runs/{*tail}"]["post"];
        assert_eq!(run["security"], json!([{ "visitorToken": [] }]));
        assert_eq!(
            run["x-aiplane-schema-unsupported"],
            "the run's frames are built by hand"
        );
        assert_eq!(
            run["requestBody"]["content"]["multipart/form-data"]["schema"]["required"],
            json!(["file"])
        );
        assert!(
            run["responses"]["200"]["content"]["text/event-stream"]["x-aiplane-event-data"]
                .is_object()
        );
    }

    /// `Item` is read with `tags` optional and written with `tags` always
    /// present, so it has two schemas, and each side points at its own.
    #[test]
    fn a_type_read_and_written_differently_gets_one_schema_per_direction() {
        let document = generate(sample_router(), &sample_declarations());
        let schemas = &document["components"]["schemas"];
        assert_eq!(schemas["Item"]["required"], json!(["name"]));
        assert_eq!(schemas["ItemReply"]["required"], json!(["name", "tags"]));
        let paths = &document["paths"];
        assert_eq!(
            paths["/api/v0/items/{id}"]["put"]["requestBody"]["content"]["application/json"]["schema"]
                ["$ref"],
            "#/components/schemas/Item"
        );
        assert_eq!(
            paths["/api/v0/items/{id}"]["get"]["responses"]["200"]["content"]["application/json"]["schema"]
                ["$ref"],
            "#/components/schemas/ItemReply"
        );
        assert!(schemas["ErrorEnvelope"]["properties"]["error"].is_object());
    }

    /// A definition holding a reference is the same on both sides once the
    /// reply's reference is rewritten; it differs only when what it points
    /// at was itself renamed.
    #[test]
    fn nested_references_share_a_name_unless_their_target_differs() {
        let reply_ref = |name: &str| json!({ "$ref": format!("{REPLY_SCHEMAS}{name}") });
        let request_ref = |name: &str| json!({ "$ref": format!("{SCHEMAS}{name}") });
        let leaf = json!({ "type": "string" });
        let definitions = Map::from_iter([
            ("Leaf".to_string(), leaf.clone()),
            (
                "Holder".to_string(),
                json!({ "properties": { "leaf": request_ref("Leaf") } }),
            ),
            ("Changed".to_string(), json!({ "type": "integer" })),
            (
                "Outer".to_string(),
                json!({ "properties": { "inner": request_ref("Changed") } }),
            ),
        ]);
        let replies = Map::from_iter([
            ("Leaf".to_string(), leaf),
            (
                "Holder".to_string(),
                json!({ "properties": { "leaf": reply_ref("Leaf") } }),
            ),
            ("Changed".to_string(), json!({ "type": "number" })),
            (
                "Outer".to_string(),
                json!({ "properties": { "inner": reply_ref("Changed") } }),
            ),
        ]);
        let merged = merge(definitions, replies);
        assert_eq!(merged.renamed["Holder"], "Holder");
        assert!(!merged.definitions.contains_key("HolderReply"));
        assert_eq!(merged.renamed["Changed"], "ChangedReply");
        assert_eq!(merged.renamed["Outer"], "OuterReply");
        assert_eq!(
            merged.definitions["OuterReply"]["properties"]["inner"]["$ref"],
            format!("{SCHEMAS}ChangedReply")
        );
    }

    #[test]
    fn operation_ids_are_stable_and_identifier_safe() {
        assert_eq!(
            operation_id("get", "/api/v0/chat/sessions/{id}/export.pdf"),
            "getApiV0ChatSessionsIdExportPdf"
        );
    }

    #[test]
    fn every_registered_route_is_declared_exactly_once() {
        let registered: BTreeSet<(&str, &str)> =
            registered_routes(ROUTER_SOURCE).into_iter().collect();
        let declarations = operations();
        let mut declared = BTreeSet::new();
        for op in &declarations {
            assert!(
                declared.insert((op.method, op.path)),
                "{} {} is declared twice",
                op.method,
                op.path
            );
        }
        let undeclared: Vec<_> = registered.difference(&declared).collect();
        assert!(
            undeclared.is_empty(),
            "routes in router.rs with no declaration in rama_server/openapi — declare their \
             request, response and errors: {undeclared:?}"
        );
        let unrouted: Vec<_> = declared.difference(&registered).collect();
        assert!(
            unrouted.is_empty(),
            "declarations for routes router.rs does not register: {unrouted:?}"
        );
    }

    #[test]
    fn every_declaration_answers_with_something() {
        for op in operations() {
            assert!(
                !op.replies.is_empty(),
                "{} {} declares no success response",
                op.method,
                op.path
            );
            // The agent connection's DELETE is the one body-carrying deletion:
            // it carries the version body for the same conflict check as its
            // PUT and connect.
            if matches!(op.method, "get" | "delete")
                && op.path != "/api/v0/agents/{id}/connections/{key}"
            {
                assert!(
                    op.request.is_empty(),
                    "{} {} declares a request body",
                    op.method,
                    op.path
                );
            }
        }
    }

    #[test]
    fn every_schema_reference_resolves() {
        fn refs<'a>(value: &'a Value, out: &mut Vec<&'a str>) {
            match value {
                Value::Object(object) => {
                    if let Some(Value::String(r)) = object.get("$ref") {
                        out.push(r);
                    }
                    object.values().for_each(|v| refs(v, out));
                }
                Value::Array(items) => items.iter().for_each(|v| refs(v, out)),
                _ => {}
            }
        }
        let document = generate(ROUTER_SOURCE, &operations());
        let mut found = Vec::new();
        refs(&document, &mut found);
        let schemas = document["components"]["schemas"].as_object().unwrap();
        for reference in found {
            let name = reference
                .strip_prefix(SCHEMAS)
                .unwrap_or_else(|| panic!("{reference} points outside components/schemas"));
            assert!(schemas.contains_key(name), "{reference} does not resolve");
        }
    }

    /// An operation without a derived schema is a documented exception, not
    /// a silent gap: it says why, and `docs/ui.md` lists it.
    #[test]
    fn unsupported_operations_give_a_reason_and_are_listed_in_the_docs() {
        let docs = include_str!("../../../../../docs/ui.md");
        for op in operations() {
            let Some(reason) = op.unsupported else {
                continue;
            };
            assert!(
                reason.len() > 20,
                "{} {} needs a real reason",
                op.method,
                op.path
            );
            let entry = format!("`{} {}`", op.method.to_uppercase(), op.path);
            assert!(
                docs.contains(&entry),
                "docs/ui.md does not list {entry} among the operations without a schema"
            );
        }
    }
}
