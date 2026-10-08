// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The chat pipeline behind a translated dialect.
//!
//! `/v1/messages` and `/v1/responses` each translate their request into a
//! chat-completions body and their result back. Everything in between —
//! tool layer, automatic routing, content guard, alias resolution with the
//! outage wait, limits, model defaults, reasoning effort, and the buffered or
//! streaming tool loop — is one sequence, and it lives here so the two
//! dialects cannot drift apart on the next fix. A dialect supplies only what
//! differs: how a refusal is worded on its wire, and the [`StreamSink`] that
//! encodes its stream.

use std::sync::Arc;

use rama::http::{HeaderMap, Response, StatusCode};
use serde_json::Value;

use aiplane_core::server::auth::UserCtx;
use aiplane_core::server::limits::LimitExceeded;
use aiplane_core::server::reasoning::Effort;
use aiplane_core::server::upstreams::registry::RouteError;
use aiplane_core::server::upstreams::{PoolAccess, PoolKind};
use aiplane_runtime::rama_server::state::RamaState;
use aiplane_runtime::server::tools::runner::{LoopError, LoopOutput};

use super::proxy::{self, StreamSink};

/// A request already translated into a chat-completions body.
pub(crate) struct TranslatedTurn {
    pub body: Value,
    /// The model id exactly as the client asked for it (may be an alias).
    pub model: String,
    pub stream: bool,
    /// `None` leaves the backend's reasoning parameters alone.
    pub effort: Option<Effort>,
}

/// How a dialect words the refusals that happen before a turn starts.
pub(crate) trait Refusals {
    fn route_error(&self, err: RouteError) -> Response;
    fn rate_limited(&self, err: &LimitExceeded) -> Response;
    /// Any other refusal. `code` is the OpenAI error code; a dialect without
    /// one words the refusal by `status` alone.
    fn refused(&self, status: StatusCode, code: &str, message: &str) -> Response;
}

/// What the loop produced.
pub(crate) enum Ran {
    /// The SSE response, already encoded by the dialect's sink.
    Streamed(Response),
    /// The final chat completion, for the dialect to translate.
    Buffered(Result<LoopOutput, LoopError>),
}

/// A turn that ran, with the routing facts its response headers report.
pub(crate) struct Turn {
    pub ran: Ran,
    pub route: Route,
}

/// Which model the request named, which one served it, and the automatic
/// route that chose it, if one did.
pub(crate) struct Route {
    pub requested_model: String,
    pub real_model: String,
    automatic_decision: Option<aiplane_core::server::automatic_routing::AutomaticRouteDecision>,
}

impl Route {
    /// `X-Gateway-Resolved-Model` and the automatic-route headers.
    pub fn decorate(&self, response: Response) -> Response {
        let response =
            proxy::with_resolved_model_header(response, &self.requested_model, &self.real_model);
        proxy::with_automatic_route_headers(response, self.automatic_decision.as_ref())
    }
}

/// Run a translated turn through the chat pipeline. `Err` is a refusal that
/// is already the whole response.
pub(crate) async fn run(
    state: Arc<RamaState>,
    user: UserCtx,
    headers: HeaderMap,
    client_ip: Option<String>,
    turn: TranslatedTurn,
    refusals: &(dyn Refusals + Sync),
    sink: impl FnOnce() -> Box<dyn StreamSink> + Send,
) -> Result<Turn, Response> {
    // The same tool surface `/v1/chat/completions` resolves: gateway tools
    // ride along only when the token has tool use enabled, and the turn is
    // pure translation when it doesn't.
    let (allowed_tools, auto_tools, user_mcp) = state.api_tool_layer(&user).await;

    let access = state.pool_access_for_token(&user).for_request(&turn.model);
    let (routing_model, automatic_decision) = proxy::resolve_automatic_chat_route(
        &state,
        &user,
        &turn.model,
        &turn.body,
        &access,
        &headers,
        !allowed_tools.is_empty() || !auto_tools.is_empty(),
        refusals,
    )
    .await?;
    let access = match &automatic_decision {
        Some(decision) => {
            access.for_route_targets(&decision.alias, [decision.effective_target.as_str()])
        }
        None => access,
    };
    // Resolve **without** taking a slot: the tool loop makes its own routing
    // decision per round, so a guard acquired here would be dropped unused —
    // holding capacity the request never spends and counting as a dispatch it
    // never made. Waiting still happens: a pool with no available replica
    // parks the request (a restarting GPU box, a model being swapped) rather
    // than failing it, and since nothing has been written to the client yet, a
    // request that waits and then succeeds looks like a slow one.
    let real_model = proxy::resolve_or_wait(&state, &routing_model, &access)
        .await
        .map_err(|e| {
            proxy::with_automatic_route_headers(
                refusals.route_error(e),
                automatic_decision.as_ref(),
            )
        })?;
    // The limits come before the content guard, so a caller over budget does
    // not cost a guard inference on the way to its 429.
    if let Some(exceeded) =
        proxy::limit_exceeded_for_model(&state, &user, &real_model, PoolKind::Chat).await
    {
        return Err(proxy::with_automatic_route_headers(
            refusals.rate_limited(&exceeded),
            automatic_decision.as_ref(),
        ));
    }
    if let Some(response) =
        proxy::enforce_content_guard(&state, &turn.body, &routing_model, &access).await
    {
        return Err(proxy::with_automatic_route_headers(
            response,
            automatic_decision.as_ref(),
        ));
    }

    // The `model` field becomes the resolved id first, because the admin
    // defaults are keyed on it — an alias inherits its target's settings. The
    // `Value` form of the merge: this body was just built as a `Value`, so
    // serialising it only to parse it back would be pure waste.
    let mut request_body = turn.body;
    proxy::set_model_in_value(&mut request_body, &real_model);
    if let Err(err) =
        aiplane_core::server::model_defaults::apply_defaults(&state.db, &mut request_body).await
    {
        // A broken stored default is not a reason to fail the caller's request.
        tracing::warn!(error = %err, model = %real_model, "model_defaults: skipping merge");
    }
    if let Some(effort) = turn.effort {
        apply_effort(&state, &real_model, &access, effort, &mut request_body).await;
    }

    let ran = if turn.stream {
        Ran::Streamed(
            proxy::stream_with_tools(
                state,
                user,
                real_model.clone(),
                access,
                headers,
                client_ip,
                request_body,
                allowed_tools,
                auto_tools,
                user_mcp,
                sink(),
            )
            .await,
        )
    } else {
        Ran::Buffered(
            proxy::buffered_with_tools(
                &state,
                &user,
                &real_model,
                access,
                headers,
                client_ip,
                request_body,
                &allowed_tools,
                &auto_tools,
                &user_mcp,
            )
            .await,
        )
    };
    Ok(Turn {
        ran,
        route: Route {
            requested_model: turn.model,
            real_model,
            automatic_decision,
        },
    })
}

/// `X-Gateway-Tool-Rounds`, `X-Gateway-Tool-Budget-Exhausted` and
/// `X-Gateway-Backend` for a buffered turn's response. The backend header is
/// the one way a client can check from outside whether prefix affinity is
/// doing what it claims.
pub(crate) fn with_outcome_headers(response: Response, outcome: &LoopOutput) -> Response {
    let mut response = proxy::with_budget_header(response, outcome.budget_exhausted);
    if let Ok(rounds) = rama::http::HeaderValue::from_str(&outcome.rounds.to_string()) {
        response
            .headers_mut()
            .insert("x-gateway-tool-rounds", rounds);
    }
    match outcome.backend.as_deref() {
        Some(backend) => proxy::with_backend_header(response, backend),
        None => response,
    }
}

/// Carry the request's reasoning intent across as the serving model's own
/// reasoning parameter.
///
/// The dialect's field never reaches the upstream — a vLLM backend rejects
/// Anthropic's `thinking` — so the gateway's effort levels, which already know
/// how each backend family spells "think harder"
/// (`chat_template_kwargs.enable_thinking`, `reasoning_effort`,
/// `thinking.budget_tokens`), do it instead, and an admin can retune the
/// per-level budgets per model on `/admin/models` without touching this path.
async fn apply_effort(
    state: &RamaState,
    real_model: &str,
    access: &PoolAccess,
    effort: Effort,
    body: &mut Value,
) {
    // Same three-source resolution as the chat driver: a client asking for
    // reasoning against an Ollama backend has to get the spelling that server
    // understands, or its request translates into a parameter that is silently
    // dropped.
    let serving = state
        .upstreams
        .serving_profile(real_model, PoolKind::Chat, access);
    let (style, overrides) =
        aiplane_core::server::reasoning::resolve_for_model(&state.db, real_model, serving.dialect)
            .await;
    aiplane_core::server::reasoning::apply_effort(
        style,
        effort,
        &overrides,
        serving.thinking_budget,
        body,
    );
}
