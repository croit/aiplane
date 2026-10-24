// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! One model round of an agent run in the activity log (`llm_exchange`):
//! the request body exactly as sent and the answer as the stream assembled
//! it. A person's turn records nothing here — their chat keeps its own
//! history and its logging is unchanged.

use std::sync::Mutex;
use std::time::Instant;

use aiplane_agents::db::agent_audit::{AuditKind, exchange};
use serde_json::{Value, json};
use session_core::driver::TurnError;

use super::OpenAiDriver;
use crate::agent_run::AgentRun;
use crate::server::side_call::SideExchange;
use crate::server::tools::{ToolContext, ToolSource};

/// Which backend took the request.
pub(super) struct Served<'a> {
    pub model: &'a str,
    pub real_model: &'a str,
    pub backend: Option<&'a str>,
}

/// What came back, assembled from the stream.
#[derive(Default)]
pub(super) struct Answer {
    pub status: Option<u16>,
    pub content: String,
    pub reasoning: String,
    pub tool_calls: Vec<Value>,
    pub finish_reason: Option<String>,
    pub usage: aiplane_core::server::db::usage::TokenCounts,
    /// The upstream's or the transport's failure, in full.
    pub error: Option<String>,
    pub cancelled: bool,
}

impl Answer {
    pub fn failed(status: Option<u16>, error: impl Into<String>) -> Self {
        Self {
            status,
            error: Some(error.into()),
            ..Self::default()
        }
    }

    pub fn from_error(err: &TurnError) -> Self {
        Self::failed(None, err.to_string())
    }
}

/// Record the vision fallback's calls for the result of tool call
/// `call_id`, in an agent run only.
pub(super) async fn record_vision_fallback(
    d: &OpenAiDriver,
    tool_ctx: &ToolContext,
    call_id: &str,
    calls: &[aiplane_core::server::capabilities::DescribeCall],
) {
    let ctx = ToolContext {
        call_id: Some(call_id.to_string()),
        ..tool_ctx.clone()
    };
    let Some(log) = crate::agents::audit::RunLog::of(&ctx) else {
        return;
    };
    for call in calls {
        let mut exchange = SideExchange::new("vision_fallback");
        exchange.model = Some(call.model.clone());
        exchange.backend = call.backend.clone();
        exchange.request = call.request.clone();
        exchange.status = call.status;
        exchange.response = call.response.clone().unwrap_or(Value::Null);
        exchange.error = call.error.clone();
        exchange.took(call.latency_ms);
        log.record(&d.state.db, &exchange).await;
    }
}

/// The rounds of one turn, as the log keeps them: the first whole, every
/// later one as a delta against the round before
/// (`agent_audit::exchange::request_delta`).
pub(super) struct ExchangeLog<'a> {
    /// The turn's tools: which of them declare their arguments sensitive.
    tools: &'a dyn ToolSource,
    /// The last round recorded: its event's id and its request as stored.
    previous: Mutex<Option<(String, Value)>>,
}

impl<'a> ExchangeLog<'a> {
    pub fn new(tools: &'a dyn ToolSource) -> Self {
        Self {
            tools,
            previous: Mutex::new(None),
        }
    }

    /// Record one round. `request` is the body that went (or would have
    /// gone) upstream; `started` is when it was sent.
    #[allow(clippy::too_many_arguments)]
    pub async fn record(
        &self,
        d: &OpenAiDriver,
        tool_ctx: &ToolContext,
        round: u32,
        request: &Value,
        served: Served<'_>,
        answer: Answer,
        started: Instant,
    ) {
        let Some(run) = d.agent() else {
            return;
        };
        let request = request.clone();
        self.note_sensitive(run, &request, &answer.tool_calls);
        let previous = self
            .previous
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        let stored = match previous.and_then(|(id, prev)| {
            exchange::request_delta(&prev, &request).map(|mut delta| {
                delta["prev"] = json!(id);
                delta
            })
        }) {
            Some(delta) => ("request_delta", delta),
            None => ("request", request.clone()),
        };
        let id = record(tool_ctx, round, stored, served, answer, started).await;
        *self.previous.lock().unwrap_or_else(|p| p.into_inner()) = id.map(|id| (id, request));
    }
}

impl ExchangeLog<'_> {
    /// Note on `run` every tool called in this round's request history or
    /// answer that declares its arguments sensitive, so the log redacts
    /// them in this exchange and every later event of the run.
    fn note_sensitive(&self, run: &AgentRun, request: &Value, tool_calls: &[Value]) {
        let history = request["messages"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| m["tool_calls"].as_array())
            .flatten();
        for call in history.chain(tool_calls) {
            if let Some(name) = call["function"]["name"].as_str()
                && self.tools.get(name).is_some_and(|t| t.sensitive_args())
            {
                run.note_sensitive(name);
            }
        }
    }
}

async fn record(
    tool_ctx: &ToolContext,
    round: u32,
    (request_key, request): (&str, Value),
    served: Served<'_>,
    answer: Answer,
    started: Instant,
) -> Option<String> {
    let usage = answer.usage;
    let latency = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let ctx = ToolContext {
        call_id: None,
        ..tool_ctx.clone()
    };
    let mut event = json!({
        "purpose": "round",
        "round": round,
        "model": served.model,
        "real_model": served.real_model,
        "backend": served.backend,
        request_key: request,
        "response": {
            "status": answer.status,
            "content": answer.content,
            "reasoning": answer.reasoning,
            "tool_calls": answer.tool_calls,
            "finish_reason": answer.finish_reason,
            "usage": {
                "prompt_tokens": usage.prompt,
                "completion_tokens": usage.completion,
                "total_tokens": usage.total,
                "cached_tokens": usage.cached,
            },
        },
        "latency_ms": latency,
    });
    if let Some(error) = answer.error {
        event["error"] = json!(error);
    }
    if answer.cancelled {
        event["cancelled"] = json!(true);
    }
    ctx.audit_event(AuditKind::LlmExchange, Some(latency), event)
        .await
}
