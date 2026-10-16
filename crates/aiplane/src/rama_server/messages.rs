// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! `POST /v1/messages` — the Anthropic Messages API, served by the same
//! pipeline as `/v1/chat/completions`.
//!
//! This is what lets Claude Code (and any other Anthropic-format client) run
//! against the gateway: point `ANTHROPIC_BASE_URL` here, hand it a `gwk_…`
//! token, and it talks to whatever model the operator has configured — with
//! the gateway's routing, per-token model allowlists, rate limits, usage
//! accounting and tool loop all applying unchanged, because *none of that is
//! reimplemented here*. This module is the format edge and nothing else:
//!
//! ```text
//!   Anthropic request  ──[anthropic::request]──▶  OpenAI request
//!                                                      │
//!            (the existing routing / limits / tool loop / usage path)
//!                                                      │
//!   Anthropic response ◀─[anthropic::response]──  OpenAI response
//!   Anthropic SSE      ◀─[AnthropicSink]────────  OpenAI SSE
//! ```
//!
//! ## Gateway tools
//!
//! An Anthropic-format client brings its own tools — Claude Code brings a
//! dozen — and the gateway's tool loop already knows how to split a turn
//! between the tools it owns and the ones the client owns. So the behaviour
//! here is the same as on `/v1/chat/completions`: gateway tools are merged in
//! only when the caller's token has tool use enabled (off by default), in
//! which case the gateway runs its own tools server-side, invisibly, and
//! hands client-owned calls back for the client to run. With tool use off,
//! this endpoint is pure translation.
//!
//! ## Token counting
//!
//! `POST /v1/messages/count_tokens` is answered by asking the *serving backend*
//! to tokenize the translated request — vLLM exposes `POST /tokenize`, which
//! runs the model's own chat template over the messages and tool definitions
//! and returns an exact count.
//!
//! A backend without that endpoint gets a `404` rather than a guess: the
//! client then counts context from the `usage` figures on real responses,
//! which is also exact. Estimating from character counts was the other option,
//! and a confidently wrong number driving a client's compaction decisions is
//! worse than no number at all.

use aiplane_core::server::capped_read;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use rama::bytes::Bytes;
use rama::http::service::web::extract::State;
use rama::http::service::web::response::IntoResponse;
use rama::http::{Request, Response, StatusCode};
use serde_json::{Value, json};

use aiplane_core::server::anthropic::{self, stream::StreamEncoder};
use aiplane_core::server::upstreams::PoolKind;
use aiplane_core::server::upstreams::registry::RouteError;
use aiplane_runtime::rama_server::auth::require_bearer;
use aiplane_runtime::rama_server::state::RamaState;
use aiplane_runtime::server::tools::runner::{LoopError, LoopOutput, ToolCallAcc};

use crate::rama_server::proxy::{self, ChunkMeta, StreamFailure, StreamSink, TokenUsage};
use crate::rama_server::translated::{self, Ran, Refusals, TranslatedTurn, Turn};

/// How often to send an SSE `ping` while a streamed turn is producing no
/// upstream bytes.
///
/// Claude Code aborts a stream that relays nothing for 300 seconds, counting
/// every byte the gateway sends — including pings. A self-hosted backend
/// sends no keep-alives of its own, and a round that runs a gateway tool (a
/// sandbox command, a web fetch) can be silent for minutes, so the gateway
/// has to produce them. Fifteen seconds is far inside the client's window and
/// costs four frames a minute.
const PING_INTERVAL: Duration = Duration::from_secs(15);

/// `POST /v1/messages`.
pub async fn messages(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    // Source IP for `get_user_location`, captured before the request is split
    // so the socket extension is still reachable. Same as the OpenAI path.
    let client_ip = state.client_ip(&req);
    let (parts, body) = req.into_parts();

    let user = match require_bearer(&state, &parts.headers).await {
        Ok(u) => u,
        // `require_bearer` hands back *why* it refused rather than a rendered
        // response, so this says the same thing in the Anthropic envelope —
        // including the internal reasons, which a status code alone can't
        // distinguish.
        Err(refusal) => return error_response(refusal.status, &refusal.message),
    };
    let translated = match translated_request(body).await {
        Ok(t) => t,
        Err(resp) => return resp,
    };
    let requested_model = translated.model.clone();
    let turn = TranslatedTurn {
        body: translated.body,
        model: translated.model,
        stream: translated.stream,
        effort: translated.effort,
    };
    let turn = match translated::run(
        state,
        user,
        parts.headers,
        client_ip,
        turn,
        &AnthropicRefusals,
        || Box::new(AnthropicSink::new(&requested_model)),
    )
    .await
    {
        Ok(turn) => turn,
        Err(response) => return response,
    };
    let Turn { ran, route } = turn;
    let response = match ran {
        Ran::Streamed(response) => response,
        Ran::Buffered(outcome) => buffered_response(outcome, &requested_model),
    };
    route.decorate(response)
}

/// Read, parse and translate a request body — the identical prologue both
/// handlers run before they can do anything endpoint-specific. Every failure
/// is the caller's, so they all render as `400` in the Anthropic shape.
async fn translated_request(
    body: rama::http::Body,
) -> Result<anthropic::request::TranslatedRequest, Response> {
    let bytes = session_core::chrome::read_body_to_bytes(body)
        .await
        .map_err(|msg| error_response(StatusCode::BAD_REQUEST, &msg))?;
    let request: Value = serde_json::from_slice(&bytes).map_err(|err| {
        error_response(
            StatusCode::BAD_REQUEST,
            &format!("body is not valid JSON: {err}"),
        )
    })?;
    anthropic::request::to_openai(&request)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, &err.0))
}

/// `POST /v1/messages/count_tokens`.
///
/// Translates the request exactly as [`messages`] would, then asks the backend
/// that would serve it to tokenize the result. That makes the count the real
/// one — the model's own chat template, its own tokenizer, the tool
/// definitions included — rather than an approximation of it.
///
/// The count covers what the *client* sent. Gateway tools the loop would inject
/// at inference time (only when the token has tool use enabled) are not in it:
/// resolving that set means talking to the caller's MCP connectors, which is
/// far too much work for a request whose whole job is to be cheap.
pub async fn count_tokens(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (parts, body) = req.into_parts();
    let user = match require_bearer(&state, &parts.headers).await {
        Ok(u) => u,
        Err(refusal) => return error_response(refusal.status, &refusal.message),
    };
    let translated = match translated_request(body).await {
        Ok(t) => t,
        Err(resp) => return resp,
    };
    let requested_model = translated.model.clone();

    let access = state
        .pool_access_for_token(&user)
        .for_request(&requested_model);
    let (routing_model, automatic_decision) = match proxy::resolve_automatic_chat_route(
        &state,
        &user,
        &translated.model,
        &translated.body,
        &access,
        &parts.headers,
        false,
        &AnthropicRefusals,
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let access = match &automatic_decision {
        Some(decision) => {
            access.for_route_targets(&decision.alias, [decision.effective_target.as_str()])
        }
        None => access,
    };
    let with_route =
        |response| proxy::with_automatic_route_headers(response, automatic_decision.as_ref());
    let acquired = match state
        .upstreams
        .route_access(&routing_model, PoolKind::Chat, &access)
    {
        Ok(a) => a,
        Err(e) => return with_route(route_error_response(e)),
    };
    let real_model = acquired.resolved_model().to_string();
    let backend = acquired.backend().name.clone();
    let url = tokenize_url(&acquired.backend().base_url);
    let api_key = acquired.backend().api_key.clone();
    let extra_headers = acquired.backend().extra_headers.clone();
    // Release the in-flight slot before the call: this is not inference, and
    // holding a chat slot for it would let token counting starve real turns.
    drop(acquired);

    if tokenize_unsupported(&url) {
        return with_route(tokenize_unavailable());
    }

    // Built field by field rather than by stripping the inference body: the
    // tokenizer endpoint validates its input strictly, and an allowlist can't
    // be outgrown by a field the translator learns to emit later. The two
    // fields are *moved* out of the translated body — on a Claude Code request
    // they are the whole transcript plus a dozen tool schemas, and this
    // endpoint's entire justification is being cheap.
    let mut body = translated.body;
    let mut payload = json!({"model": real_model, "messages": body["messages"].take()});
    if let Some(tools) = body.get_mut("tools").map(Value::take)
        && let Some(obj) = payload.as_object_mut()
    {
        obj.insert("tools".into(), tools);
    }

    let mut http = state.http.post(&url).json(&payload);
    if let Some(key) = api_key.as_deref() {
        http = http.bearer_auth(key);
    }
    http = aiplane_core::server::upstreams::config::apply_extra_headers(http, &extra_headers);
    let resp = match http.send().await {
        Ok(r) => r,
        Err(err) => {
            tracing::debug!(error = %err, %backend, "count_tokens: tokenize call failed");
            return with_route(tokenize_unavailable());
        }
    };
    if !resp.status().is_success() {
        let status = resp.status();
        // Only "this endpoint isn't here" is worth remembering. A 429 or a 503
        // says the backend is busy, and caching that would disable token
        // counting for the rest of the process over a momentary hiccup.
        if matches!(status.as_u16(), 404 | 405 | 501) {
            remember_tokenize_unsupported(&url);
        }
        tracing::debug!(%status, %backend, "count_tokens: tokenize unavailable");
        return with_route(tokenize_unavailable());
    }
    let counted = capped_read::read_capped_json::<Value>(resp, capped_read::API_ANSWER_BYTES)
        .await
        .ok()
        .and_then(|v| v.get("count").and_then(Value::as_i64));
    let response = match counted {
        Some(count) => json_response(StatusCode::OK, &json!({"input_tokens": count})),
        None => {
            tracing::debug!(%backend, "count_tokens: tokenize response carried no count");
            tokenize_unavailable()
        }
    };
    let response = proxy::with_resolved_model_header(response, &requested_model, &real_model);
    with_route(response)
}

/// The backend's tokenizer endpoint. vLLM serves `/tokenize` at the server
/// root, *not* under the OpenAI `/v1` prefix that `base_url` carries — asking
/// for `…/v1/tokenize` is a 404.
fn tokenize_url(base_url: &str) -> String {
    let root =
        aiplane_core::server::upstreams::profile::server_root(base_url.trim_end_matches('/'));
    format!("{root}/tokenize")
}

/// Tokenizer endpoints observed not to exist. A negative cache only: the worst
/// case of a stale entry is that a backend which later gained the endpoint keeps
/// being skipped until the gateway restarts, and the client keeps using the
/// fallback it was already using. Only a status that means *absence* gets an
/// entry — a busy backend is not a missing one.
///
/// Keyed by URL rather than backend name, because the URL is what actually
/// determines whether the endpoint is there: repointing a backend at a new
/// address must not inherit the old address's answer.
///
/// `Backend` in the upstream registry is the codebase's usual home for a
/// learned per-backend fact (it already carries `healthy`, the probed model
/// set, and disabled aliases), and this would sit there naturally but for one
/// thing: the answer only arrives *after* the in-flight slot is released
/// above, deliberately, so that counting tokens can't starve real inference.
/// Reaching the backend again to record it would mean re-acquiring a slot for
/// a bookkeeping write. A URL-keyed map costs one hash lookup and needs
/// nothing held open. Revisit if a second "does this backend support X" probe
/// appears — then the pattern, not this instance, is what wants fixing.
static NO_TOKENIZER: std::sync::LazyLock<std::sync::Mutex<std::collections::HashSet<String>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashSet::new()));

fn tokenize_unsupported(url: &str) -> bool {
    NO_TOKENIZER
        .lock()
        .map(|s| s.contains(url))
        .unwrap_or(false)
}

fn remember_tokenize_unsupported(url: &str) {
    if let Ok(mut s) = NO_TOKENIZER.lock() {
        s.insert(url.to_string());
    }
}

/// `404` — the documented way to tell an Anthropic-format client that this
/// optional endpoint isn't available, so it falls back to counting context
/// from response `usage` instead of trusting a number we can't produce.
fn tokenize_unavailable() -> Response {
    error_response(
        StatusCode::NOT_FOUND,
        concat!(
            "token counting is not available for this model — the serving backend ",
            "exposes no tokenizer. Count context from the `usage` field of a real ",
            "response instead.",
        ),
    )
}

/// `HEAD /api/hello` — the connection-warming probe an Anthropic-format
/// client sends at startup. Answering it costs nothing and keeps a puzzling
/// `404` out of the gateway's own logs; a client that gets one carries on
/// regardless.
pub async fn hello() -> Response {
    StatusCode::OK.into_response()
}

/// The non-streaming result, translated into an Anthropic `Message`.
fn buffered_response(outcome: Result<LoopOutput, LoopError>, requested_model: &str) -> Response {
    let outcome = match outcome {
        Ok(o) => o,
        Err(err) => return loop_error_response(err),
    };
    if outcome.status >= 400 {
        // The upstream's own wording survives: the client's
        // retry-without-the-capability recovery matches on it.
        return json_response(
            StatusCode::from_u16(outcome.status).unwrap_or(StatusCode::BAD_GATEWAY),
            &anthropic::error::from_upstream(outcome.status, &outcome.body),
        );
    }
    let completion: Value = match serde_json::from_slice(&outcome.body) {
        Ok(v) => v,
        Err(err) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("upstream returned unparseable JSON: {err}"),
            );
        }
    };
    let message = anthropic::response::from_openai(&completion, requested_model);
    translated::with_outcome_headers(json_response(StatusCode::OK, &message), &outcome)
}

/// Pre-turn refusals in the Anthropic shape.
struct AnthropicRefusals;

impl Refusals for AnthropicRefusals {
    fn route_error(&self, err: RouteError) -> Response {
        route_error_response(err)
    }

    fn rate_limited(&self, err: &aiplane_core::server::limits::LimitExceeded) -> Response {
        rate_limited(err)
    }

    fn refused(&self, status: StatusCode, _code: &str, message: &str) -> Response {
        error_response(status, message)
    }
}

/// The Anthropic-format [`StreamSink`]: re-encodes the loop's OpenAI chunks
/// as the Anthropic SSE event sequence.
struct AnthropicSink {
    encoder: StreamEncoder,
}

impl AnthropicSink {
    fn new(model: &str) -> Self {
        // The message id is minted here rather than derived from an upstream
        // id: `message_start` goes out before the first backend has answered.
        Self {
            encoder: StreamEncoder::new(
                anthropic::message_id(Some(&uuid::Uuid::new_v4().simple().to_string())),
                model,
            ),
        }
    }
}

/// SSE frames the encoder produced, as body chunks.
fn frames(out: Vec<String>) -> Vec<Bytes> {
    out.into_iter().map(Bytes::from).collect()
}

impl StreamSink for AnthropicSink {
    fn prologue(&mut self) -> Vec<Bytes> {
        frames(self.encoder.start())
    }

    fn visible_chunk(&mut self, chunk: Option<&Value>, _raw: Vec<u8>) -> Vec<Bytes> {
        // An event with no parseable `data:` payload (a comment, a provider's
        // own keep-alive) has nothing to re-encode; our own pings cover the
        // gap it was there to fill.
        chunk
            .map(|c| frames(self.encoder.chunk(c)))
            .unwrap_or_default()
    }

    fn round_usage(&mut self, tokens: TokenUsage) {
        self.encoder
            .absorb_round_usage(tokens.0.unwrap_or(0), tokens.1.unwrap_or(0));
    }

    fn client_tool_calls(
        &mut self,
        _meta: &ChunkMeta,
        acc: &BTreeMap<usize, ToolCallAcc>,
    ) -> Vec<Bytes> {
        acc.values()
            .filter(|call| !call.name.is_empty())
            .flat_map(|call| frames(self.encoder.tool_use(&call.id, &call.name, &call.arguments)))
            .collect()
    }

    fn finish(&mut self) -> Vec<Bytes> {
        frames(self.encoder.finish())
    }

    fn error(&mut self, failure: &StreamFailure) -> Vec<Bytes> {
        // The headers shipped with `message_start`, so the status can only
        // reach the client as the error's *type*. Keeping it is what lets a
        // client tell "the backend was busy" (retry) from "this request is
        // wrong" (don't) — the same mapping the buffered path applies.
        let body = match failure.status {
            Some(status) => anthropic::error::for_status(status, &failure.message),
            None => anthropic::error::envelope("api_error", &failure.message),
        };
        frames(self.encoder.error(&body))
    }

    fn heartbeat(&self) -> Option<(Duration, Bytes)> {
        Some((PING_INTERVAL, Bytes::from(StreamEncoder::ping())))
    }
}

/// A JSON response carrying an already-built Anthropic body.
fn json_response(status: StatusCode, body: &Value) -> Response {
    (
        status,
        [
            ("content-type", "application/json"),
            ("anthropic-version", anthropic::ANTHROPIC_VERSION),
        ],
        body.to_string(),
    )
        .into_response()
}

/// An Anthropic error envelope with the type implied by `status`.
pub(crate) fn error_response(status: StatusCode, message: &str) -> Response {
    json_response(
        status,
        &anthropic::error::for_status(status.as_u16(), message),
    )
}

/// `429` with the `Retry-After` the enforcer computed, in the Anthropic shape.
fn rate_limited(e: &aiplane_core::server::limits::LimitExceeded) -> Response {
    let body = anthropic::error::envelope("rate_limit_error", &e.to_string());
    let mut resp = json_response(StatusCode::TOO_MANY_REQUESTS, &body);
    if let Ok(secs) = rama::http::HeaderValue::from_str(&e.retry_after_secs.to_string()) {
        resp.headers_mut()
            .insert(rama::http::header::RETRY_AFTER, secs);
    }
    resp
}

/// Routing failures, in the Anthropic shape. The status and the sentence come
/// from the error itself ([`RouteError::status_and_message`]), so this and its
/// OpenAI counterpart cannot disagree about what a given failure means — only
/// about the envelope they put it in.
fn route_error_response(err: RouteError) -> Response {
    let (status, message) = err.status_and_message();
    // An outage that outlived the wait budget is reported as Anthropic's own
    // `529 overloaded_error` rather than a bare `503`. Both are retryable, but
    // 529 is the one Anthropic clients have a dedicated recovery path for
    // (the SDKs back off and retry it; Claude Code shows "retrying" instead of
    // ending the turn), and `Retry-After` tells them how long to wait — which is
    // the whole point of having parked the request first.
    if matches!(err, RouteError::Acquire(_)) {
        let body = anthropic::error::envelope("overloaded_error", &message);
        let mut resp = json_response(
            StatusCode::from_u16(529).unwrap_or(StatusCode::SERVICE_UNAVAILABLE),
            &body,
        );
        if let Ok(v) = rama::http::HeaderValue::from_str(&RETRY_AFTER_SECS.to_string()) {
            resp.headers_mut()
                .insert(rama::http::header::RETRY_AFTER, v);
        }
        return resp;
    }
    error_response(
        StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        &message,
    )
}

/// `Retry-After` for an upstream outage, in seconds. Short: the gateway already
/// waited out the budget, and the health probe re-checks a down backend every
/// second, so there is nothing to gain from parking the *client* for long.
const RETRY_AFTER_SECS: u32 = 5;

/// Tool-loop failures, in the Anthropic shape. Same split as above.
fn loop_error_response(err: LoopError) -> Response {
    let (status, message) = err.status_and_message();
    error_response(
        StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        &message,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// vLLM serves `/tokenize` at the server root while `base_url` carries the
    /// OpenAI `/v1` prefix — asking for `…/v1/tokenize` is a 404, which would
    /// silently disable token counting against every vLLM backend we run.
    #[test]
    fn the_tokenize_url_drops_the_openai_prefix() {
        assert_eq!(
            tokenize_url("http://backend:8005/v1"),
            "http://backend:8005/tokenize"
        );
        assert_eq!(
            tokenize_url("http://backend:8005/v1/"),
            "http://backend:8005/tokenize"
        );
        // A backend served without the prefix keeps its own root.
        assert_eq!(
            tokenize_url("http://backend:8005"),
            "http://backend:8005/tokenize"
        );
        // Only a trailing `/v1` is a prefix; one inside a path is part of the
        // address the operator configured.
        assert_eq!(
            tokenize_url("https://host/v1/openai/v1"),
            "https://host/v1/openai/tokenize"
        );
    }
}
