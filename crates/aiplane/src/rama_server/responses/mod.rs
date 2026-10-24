// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! `/v1/responses` — the OpenAI Responses API, served by the same pipeline as
//! `/v1/chat/completions`.
//!
//! This is what lets Codex CLI, the OpenAI Agents SDK and every other
//! Responses-only client run against the gateway, with its routing, limits,
//! usage accounting and tool loop applying unchanged, because none of that is
//! reimplemented here (see [`super::translated`]). This module is the format
//! edge, plus the one thing the Responses API has that Chat Completions does
//! not: stored responses.
//!
//! ```text
//!   Responses request ──[request]──▶ chat request ──▶ (the shared pipeline)
//!   response object   ◀─[output]───  chat completion
//!   Responses SSE     ◀─[stream]───  chat chunks
//!                         │
//!                      [store]  `store: true` → previous_response_id, GET, DELETE
//! ```
//!
//! A stored response is saved before the client is told it is complete — on a
//! stream, before `response.completed` — because a client may send the next
//! request with `previous_response_id` the moment it sees the end of this one.

pub mod output;
pub mod request;
pub mod store;
pub mod stream;

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use rama::bytes::Bytes;
use rama::http::service::web::extract::State;
use rama::http::service::web::response::IntoResponse;
use rama::http::{Request, Response, StatusCode};
use serde_json::{Value, json};

use aiplane_core::server::db::Pool;
use aiplane_runtime::rama_server::auth::require_bearer;
use aiplane_runtime::rama_server::state::RamaState;
use aiplane_runtime::server::tools::runner::{LoopError, LoopOutput, ToolCallAcc};

use crate::rama_server::proxy::{self, ChunkMeta, Outbound, StreamFailure, StreamSink};
use crate::rama_server::translated::{self, Ran, TranslatedTurn, Turn};

use output::Shell;
use request::{ResponsesRequest, TranslateError};
use store::{ChainError, Owner};
use stream::{SequenceNumbers, StreamEncoder};

/// How often a quiet stream re-sends `response.in_progress`. Codex aborts a
/// stream after five minutes without an event; a round that runs a slow
/// gateway tool sends nothing upstream-driven for its whole duration.
const KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(15);

const PREFIX: &str = "/v1/responses/";

/// `POST /v1/responses`.
pub async fn create(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let client_ip = state.client_ip(&req);
    let (parts, body) = req.into_parts();
    let user = match require_bearer(&state, &parts.headers).await {
        Ok(u) => u,
        Err(refusal) => return refusal.into_response(),
    };
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return invalid_request(None, &msg),
    };
    let request: Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(err) => return invalid_request(None, &format!("body is not valid JSON: {err}")),
    };
    drop(bytes);
    let mut request = match ResponsesRequest::parse(request) {
        Ok(r) => r,
        Err(err) => return translate_error(err),
    };

    let owner = Owner::of(&user.principal);
    let history = match &request.previous_response_id {
        Some(id) => match store::history(&state.db, &owner, id).await {
            Ok(items) => items,
            Err(err) => return chain_error(err),
        },
        None => Vec::new(),
    };
    let body = match request.to_chat(&history) {
        Ok(b) => b,
        Err(err) => return translate_error(err),
    };
    drop(history);

    let shell = Shell::new(&request.model, std::mem::take(&mut request.echo));
    let custom_tools = std::mem::take(&mut request.custom_tools);
    // Moved, not copied: the input can be tens of megabytes of inline images.
    let saving = request.store.then(|| {
        Arc::new(Saving {
            pool: state.db.clone(),
            owner,
            previous_response_id: request.previous_response_id.take(),
            input_items: std::mem::take(&mut request.input_items),
        })
    });
    let turn = TranslatedTurn {
        body,
        model: request.model.clone(),
        stream: request.stream,
        effort: request.effort,
    };
    drop(request);
    let sink_shell = shell.clone();
    let sink_custom = custom_tools.clone();
    let sink_saving = saving.clone();
    let turn = match translated::run(
        state,
        user,
        parts.headers,
        client_ip,
        turn,
        &proxy::OpenAiRefusals,
        move || Box::new(ResponsesSink::new(sink_shell, sink_custom, sink_saving)),
    )
    .await
    {
        Ok(turn) => turn,
        Err(response) => return response,
    };
    let Turn { ran, route } = turn;
    let response = match ran {
        Ran::Streamed(response) => response,
        Ran::Buffered(outcome) => buffered_response(outcome, &shell, &custom_tools, saving).await,
    };
    route.decorate(response)
}

/// `GET /v1/responses/{id}` and `GET /v1/responses/{id}/input_items`.
pub async fn retrieve(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (parts, _body) = req.into_parts();
    let user = match require_bearer(&state, &parts.headers).await {
        Ok(u) => u,
        Err(refusal) => return refusal.into_response(),
    };
    // Read from the raw URI: rama's router lowercases matched segments.
    let tail = parts.uri.path().strip_prefix(PREFIX).unwrap_or_default();
    let (id, listing) = match tail.strip_suffix("/input_items") {
        Some(id) => (id, true),
        None => (tail, false),
    };
    let stored = match store::find(&state.db, &Owner::of(&user.principal), id).await {
        Ok(Some(stored)) => stored,
        Ok(None) => return not_found(id),
        Err(err) => return storage_error(&err),
    };
    if !listing {
        return json_ok(&stored.response);
    }
    let query = parts.uri.query().unwrap_or_default();
    match ListQuery::parse(query) {
        Ok(q) => json_ok(&q.page(&stored.input_items)),
        Err(message) => invalid_request(None, &message),
    }
}

/// `DELETE /v1/responses/{id}`.
pub async fn delete(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (parts, _body) = req.into_parts();
    let user = match require_bearer(&state, &parts.headers).await {
        Ok(u) => u,
        Err(refusal) => return refusal.into_response(),
    };
    let id = parts.uri.path().strip_prefix(PREFIX).unwrap_or_default();
    match store::delete(&state.db, &Owner::of(&user.principal), id).await {
        Ok(true) => json_ok(&json!({"id": id, "object": "response.deleted", "deleted": true})),
        Ok(false) => not_found(id),
        Err(err) => storage_error(&err),
    }
}

/// What storing a response needs, captured before the turn runs.
struct Saving {
    pool: Pool,
    owner: Owner,
    previous_response_id: Option<String>,
    input_items: Vec<Value>,
}

impl Saving {
    /// Store `response`. A failure is logged, not returned: the turn already
    /// happened and was billed, and the client is better served by its answer
    /// than by an error about bookkeeping.
    async fn save(&self, response: &Value) {
        if let Err(err) = store::save(
            &self.pool,
            &self.owner,
            self.previous_response_id.as_deref(),
            &self.input_items,
            response,
        )
        .await
        {
            tracing::error!(error = %err, id = %response["id"], "storing a response failed");
        }
    }
}

async fn buffered_response(
    outcome: Result<LoopOutput, LoopError>,
    shell: &Shell,
    custom_tools: &BTreeSet<String>,
    saving: Option<Arc<Saving>>,
) -> Response {
    let outcome = match outcome {
        Ok(o) => o,
        Err(err) => return proxy::loop_error_response(err),
    };
    if outcome.status >= 400 {
        // The upstream already speaks OpenAI's error envelope; relay it with
        // its wording intact, as `/v1/chat/completions` does.
        return (
            StatusCode::from_u16(outcome.status).unwrap_or(StatusCode::BAD_GATEWAY),
            [("content-type", "application/json")],
            outcome.body,
        )
            .into_response();
    }
    let completion: Value = match serde_json::from_slice(&outcome.body) {
        Ok(v) => v,
        Err(err) => {
            return proxy::error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                &format!("upstream returned unparseable JSON: {err}"),
            );
        }
    };
    let response = output::from_completion(shell, &completion, custom_tools);
    if let Some(saving) = saving {
        saving.save(&response).await;
    }
    translated::with_outcome_headers(json_ok(&response), &outcome)
}

/// The Responses [`StreamSink`]: re-encodes the loop's chat chunks as the
/// Responses event sequence, and stores the result before announcing it.
struct ResponsesSink {
    encoder: StreamEncoder,
    saving: Option<Arc<Saving>>,
    closing: Vec<String>,
}

impl ResponsesSink {
    fn new(shell: Shell, custom_tools: BTreeSet<String>, saving: Option<Arc<Saving>>) -> Self {
        Self {
            encoder: StreamEncoder::new(shell, custom_tools),
            saving,
            closing: Vec::new(),
        }
    }
}

fn frames(out: Vec<String>) -> Vec<Bytes> {
    out.into_iter().map(Bytes::from).collect()
}

impl StreamSink for ResponsesSink {
    fn prologue(&mut self) -> Vec<Bytes> {
        frames(self.encoder.start())
    }

    fn visible_chunk(&mut self, chunk: Option<&Value>, _raw: Vec<u8>) -> Vec<Bytes> {
        chunk
            .map(|c| frames(self.encoder.chunk(c)))
            .unwrap_or_default()
    }

    fn round_usage(&mut self, tokens: aiplane_core::server::db::usage::TokenCounts) {
        self.encoder
            .absorb_round_usage(tokens.prompt.unwrap_or(0), tokens.completion.unwrap_or(0));
    }

    fn client_tool_calls(
        &mut self,
        _meta: &ChunkMeta,
        acc: &std::collections::BTreeMap<usize, ToolCallAcc>,
    ) -> Vec<Bytes> {
        acc.values()
            .filter(|call| !call.name.is_empty())
            .flat_map(|call| {
                frames(
                    self.encoder
                        .tool_call(&call.id, &call.name, &call.arguments),
                )
            })
            .collect()
    }

    fn before_finish(
        &mut self,
    ) -> Option<std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>> {
        let saving = self.saving.take()?;
        let (closing, response) = self.encoder.complete();
        self.closing = closing;
        Some(Box::pin(async move { saving.save(&response).await }))
    }

    fn finish(&mut self) -> Vec<Bytes> {
        let mut out = std::mem::take(&mut self.closing);
        out.extend(self.encoder.finish());
        frames(out)
    }

    fn error(&mut self, failure: &StreamFailure) -> Vec<Bytes> {
        let code = failure.code.unwrap_or(match failure.status {
            Some(429) => "rate_limit_exceeded",
            Some(400..=499) => "invalid_request",
            _ => "server_error",
        });
        frames(self.encoder.error(code, &failure.message))
    }

    fn heartbeat(&self) -> Option<(Duration, Bytes)> {
        Some((KEEP_ALIVE_INTERVAL, Bytes::from(self.encoder.keep_alive())))
    }

    fn outbound(&self) -> Option<Outbound> {
        let mut numbers = SequenceNumbers::default();
        Some(Box::new(move |frame| numbers.number(frame)))
    }
}

/// `?limit=&order=&after=` for `GET /v1/responses/{id}/input_items`.
#[derive(Debug, PartialEq, Eq)]
struct ListQuery {
    limit: usize,
    ascending: bool,
    after: Option<String>,
}

impl ListQuery {
    fn parse(query: &str) -> Result<Self, String> {
        let mut out = Self {
            limit: 20,
            ascending: false,
            after: None,
        };
        let pairs: Vec<(String, String)> = serde_urlencoded::from_str(query)
            .map_err(|err| format!("the query string is malformed: {err}"))?;
        for (key, value) in pairs {
            match key.as_str() {
                "limit" => {
                    out.limit = value
                        .parse()
                        .ok()
                        .filter(|n| (1..=100).contains(n))
                        .ok_or("`limit` must be a number from 1 to 100")?;
                }
                "order" => {
                    out.ascending = match value.as_str() {
                        "asc" => true,
                        "desc" => false,
                        _ => return Err("`order` must be `asc` or `desc`".into()),
                    };
                }
                "after" => out.after = Some(value),
                _ => {}
            }
        }
        Ok(out)
    }

    fn page(&self, items: &[Value]) -> Value {
        let mut ordered: Vec<&Value> = items.iter().collect();
        if !self.ascending {
            ordered.reverse();
        }
        let start = match &self.after {
            Some(after) => ordered
                .iter()
                .position(|item| item["id"] == after.as_str())
                .map_or(ordered.len(), |i| i + 1),
            None => 0,
        };
        let rest = &ordered[start.min(ordered.len())..];
        let data: Vec<Value> = rest.iter().take(self.limit).map(|v| (*v).clone()).collect();
        json!({
            "object": "list",
            "first_id": data.first().map(|i| i["id"].clone()),
            "last_id": data.last().map(|i| i["id"].clone()),
            "has_more": rest.len() > self.limit,
            "data": data,
        })
    }
}

fn json_ok(body: &Value) -> Response {
    (
        StatusCode::OK,
        [("content-type", "application/json")],
        body.to_string(),
    )
        .into_response()
}

fn invalid_request(param: Option<&str>, message: &str) -> Response {
    proxy::request_error_response(StatusCode::BAD_REQUEST, "invalid_request", param, message)
}

fn translate_error(err: TranslateError) -> Response {
    invalid_request(err.param, &err.message)
}

fn chain_error(err: ChainError) -> Response {
    match err {
        ChainError::NotFound(_) => proxy::request_error_response(
            StatusCode::BAD_REQUEST,
            "previous_response_not_found",
            Some("previous_response_id"),
            &err.to_string(),
        ),
        ChainError::TooLong(_) => invalid_request(Some("previous_response_id"), &err.to_string()),
        ChainError::Db(_) => proxy::error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            &err.to_string(),
        ),
    }
}

fn not_found(id: &str) -> Response {
    proxy::request_error_response(
        StatusCode::NOT_FOUND,
        "not_found",
        None,
        &format!(
            "Response with id '{id}' not found. Only responses created with `store` on, by the \
             same person or principal, within the last 30 days can be read."
        ),
    )
}

fn storage_error(err: &sqlx::Error) -> Response {
    proxy::error_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal_error",
        &format!("reading stored responses failed: {err}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(n: usize) -> Vec<Value> {
        (1..=n).map(|i| json!({"id": format!("msg_{i}")})).collect()
    }

    fn ids(page: &Value) -> Vec<String> {
        page["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["id"].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn input_items_are_listed_newest_first_by_default() {
        let page = ListQuery::parse("").unwrap().page(&items(3));
        assert_eq!(ids(&page), ["msg_3", "msg_2", "msg_1"]);
        assert_eq!(page["first_id"], "msg_3");
        assert_eq!(page["last_id"], "msg_1");
        assert_eq!(page["has_more"], false);
        assert_eq!(page["object"], "list");
    }

    #[test]
    fn input_items_page_with_limit_order_and_after() {
        let q = ListQuery::parse("limit=2&order=asc").unwrap();
        let page = q.page(&items(5));
        assert_eq!(ids(&page), ["msg_1", "msg_2"]);
        assert_eq!(page["has_more"], true);
        let next = ListQuery::parse("limit=2&order=asc&after=msg_2")
            .unwrap()
            .page(&items(5));
        assert_eq!(ids(&next), ["msg_3", "msg_4"]);
        let unknown = ListQuery::parse("after=msg_9").unwrap().page(&items(2));
        assert_eq!(ids(&unknown), Vec::<String>::new());
    }

    #[test]
    fn a_bad_list_query_is_explained() {
        assert!(
            ListQuery::parse("limit=0")
                .unwrap_err()
                .contains("1 to 100")
        );
        assert!(ListQuery::parse("order=up").unwrap_err().contains("asc"));
    }
}
