// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! One-off model calls made beside a conversation rather than in it: the
//! session title, the compaction summary, the feedback form's fields, an
//! agent's topic guard and route classifier, the evaluation judge and the
//! prompt assistant. Every one goes through [`ask_text`] or [`ask_json`],
//! which does what each used to do on its own:
//!
//! - resolves the model the way a chat turn does ([`route_target`]), under
//!   the access the caller gives;
//! - checks the payer's spend limits **before** the model is called — a
//!   person's ceilings ([`RamaState::limit_exceeded`]), an agent's owner
//!   budget and operator rules ([`Enforcer::check_agent`]);
//! - switches reasoning off (`chat_template_kwargs.enable_thinking: false`,
//!   and Qwen3's `/no_think` directive where the caller asks for it), because
//!   a reasoning model can spend a short answer's whole budget thinking;
//! - reads the answer capped ([`capped_read::MODEL_ANSWER_BYTES`]), bounded
//!   by one timeout;
//! - writes the payer's usage row, so the call counts against the same
//!   budget as their turns.
//!
//! What comes back is a [`SideExchange`] — the one record of such a call,
//! which an agent's activity log writes as an `llm_exchange`
//! ([`crate::agents::audit::RunLog::record`]) — and the answer.
//!
//! [`RamaState::limit_exceeded`]: crate::rama_server::state::RamaState::limit_exceeded
//! [`Enforcer::check_agent`]: aiplane_core::server::limits::Enforcer::check_agent

use std::sync::Arc;
use std::time::{Duration, Instant};

use aiplane_core::server::capped_read;
use aiplane_core::server::db::usage::{UsageKind, UsageRecord, UsageSource, usage_from_value};
use aiplane_core::server::limits::LimitExceeded;
use aiplane_core::server::principal::{Principal, SystemPrincipal};
use aiplane_core::server::run_chain::RunChain;
use aiplane_core::server::upstreams::{PoolAccess, PoolKind};
use serde_json::{Value, json};

use crate::rama_server::state::RamaState;
use crate::server::model_route::route_target;

/// Who a side call is for: whose spend limits it must pass and whose usage
/// row it is.
#[derive(Clone)]
pub struct Payer {
    principal: Principal,
    email: Option<String>,
    source: UsageSource,
    run: Option<Arc<RunChain>>,
}

impl Payer {
    /// A person, by their id and raw OIDC groups.
    pub fn person(id: &str, roles: &[String], email: Option<String>, source: UsageSource) -> Self {
        Self {
            principal: Principal::User {
                id: id.to_string(),
                roles: roles.to_vec(),
            },
            email,
            source,
            run: None,
        }
    }

    /// An agent's principal, in `run` when the call belongs to one: the
    /// usage row joins the run, and the budget checked is that of the agent
    /// at the run's root, which the row is booked to.
    pub fn agent(principal: &SystemPrincipal, run: Option<Arc<RunChain>>) -> Self {
        Self {
            email: Some(principal.name.clone()),
            principal: Principal::System(principal.clone()),
            source: UsageSource::Agent,
            run,
        }
    }

    /// Whoever a turn acts as: a person, or an agent in its run.
    pub fn of_turn(
        principal: &Principal,
        run: Option<Arc<RunChain>>,
        email: Option<String>,
        source: UsageSource,
    ) -> Self {
        Self {
            principal: principal.clone(),
            email,
            source,
            run,
        }
    }

    /// The first of the payer's ceilings `model` is already over, if any.
    async fn over_budget(&self, state: &RamaState, model: &str) -> Option<LimitExceeded> {
        let enforce = state
            .upstreams
            .enforce_limits_for_model(model, PoolKind::Chat);
        if !enforce {
            return None;
        }
        match &self.principal {
            Principal::User { id, roles } => {
                state
                    .limit_exceeded(id, &state.role_ids_for(roles), None, model, enforce)
                    .await
            }
            Principal::System(principal) => {
                let agent_id = self.run.as_deref().map_or(principal.id.as_str(), |run| {
                    run.agent().principal_id.as_str()
                });
                let budget = match state.agent_specs.live_recent(&state.db, agent_id).await {
                    Ok(Some(compiled)) => compiled
                        .agent()
                        .map(crate::agents::embed::owner_budget)
                        .unwrap_or_default(),
                    Ok(None) => Vec::new(),
                    Err(err) => {
                        tracing::warn!(
                            error = %err,
                            agent = agent_id,
                            "reading the agent's owner budget for a side call; checking the \
                             operator's limits only"
                        );
                        Vec::new()
                    }
                };
                state
                    .enforcer
                    .check_agent(agent_id, &budget, jiff::Timestamp::now())
                    .await
                    .err()
            }
        }
    }

    /// Emit the call's usage row, once it reached a backend.
    fn meter(&self, state: &RamaState, exchange: &SideExchange) {
        if !state.usage.is_enabled() {
            return;
        }
        let (Some(status), Some(model), Some(backend)) = (
            exchange.status,
            exchange.model.as_ref(),
            exchange.backend.as_ref(),
        ) else {
            return;
        };
        let (prompt_tokens, completion_tokens, total_tokens) = usage_from_value(&exchange.response);
        state.usage.emit(
            UsageRecord {
                created_at: jiff::Timestamp::now(),
                user_id: self.principal.subject_id().to_string(),
                user_email: self.email.clone(),
                token_id: None,
                token_name: None,
                source: self.source,
                kind: UsageKind::Chat,
                backend: backend.clone(),
                model: model.clone(),
                status,
                duration_ms: i64::try_from(exchange.latency_ms()).unwrap_or(i64::MAX),
                prompt_tokens,
                completion_tokens,
                total_tokens,
                input_units: None,
                output_units: None,
                enforce_limits: state
                    .upstreams
                    .enforce_limits_for_model(model, PoolKind::Chat),
                principal_kind: self.principal.kind(),
                agent_id: None,
                chain: None,
                stop_reason: None,
            }
            .in_run(self.run.as_deref()),
        );
    }
}

/// What to ask, on which model, and how.
pub struct SideCall<'a> {
    /// What the call is for; an agent's `llm_exchange` carries it as
    /// `purpose`.
    pub purpose: &'static str,
    /// The model as the caller names it: a real id, an alias or an
    /// automatic route.
    pub model: &'a str,
    pub access: &'a PoolAccess,
    pub instructions: &'a str,
    /// Data for the model, never instructions.
    pub input: &'a str,
    pub temperature: f64,
    pub max_tokens: Option<i64>,
    /// Append Qwen3's `/no_think` directive to the input. Other model
    /// families read it as text, so only a caller whose input tolerates the
    /// suffix sets it.
    pub no_think: bool,
    pub timeout: Duration,
}

/// The JSON object a [`ask_json`] call asks for: the schema's name in the
/// request, and the schema (`response_format: json_schema`, strict).
pub struct JsonShape<'a> {
    pub name: &'a str,
    pub schema: Value,
}

/// Why a side call has no answer.
#[derive(Debug, Clone)]
pub enum SideCallError {
    /// The payer's spend limit refused the call; the model was not asked.
    OverBudget(LimitExceeded),
    /// The call was made and failed, or its answer does not read.
    Failed(String),
}

impl std::fmt::Display for SideCallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OverBudget(exceeded) => exceeded.fmt(f),
            Self::Failed(why) => f.write_str(why),
        }
    }
}

fn failed(why: impl std::fmt::Display) -> SideCallError {
    SideCallError::Failed(why.to_string())
}

/// A side call's record and its answer.
pub struct Answered<T> {
    pub exchange: SideExchange,
    pub answer: Result<T, SideCallError>,
}

impl<T> Answered<T> {
    /// The tokens the call spent, as the upstream reported them.
    pub fn tokens(&self) -> u64 {
        usage_from_value(&self.exchange.response)
            .2
            .and_then(|t| u64::try_from(t).ok())
            .unwrap_or(0)
    }
}

/// One model call made outside a turn's round loop — a side call above, or
/// an agent's voice call — as far as it got: enough for a usage row and an
/// `llm_exchange` in an agent's activity log.
#[derive(Debug, Clone)]
pub struct SideExchange {
    pub purpose: &'static str,
    pub model: Option<String>,
    pub backend: Option<String>,
    /// The body exactly as sent.
    pub request: Value,
    pub status: Option<u16>,
    /// The answer as it came back, parsed when it was JSON; `null` when none
    /// was read.
    pub response: Value,
    /// What the caller read from the answer, when the log should say so.
    pub answer: Option<Value>,
    pub error: Option<String>,
    started: Instant,
    latency_ms: Option<u64>,
}

impl SideExchange {
    pub fn new(purpose: &'static str) -> Self {
        Self {
            purpose,
            model: None,
            backend: None,
            request: Value::Null,
            status: None,
            response: Value::Null,
            answer: None,
            error: None,
            started: Instant::now(),
            latency_ms: None,
        }
    }

    /// The answer's raw bytes, as JSON when they parse and as text otherwise.
    pub fn answered(&mut self, status: u16, body: &[u8]) {
        self.status = Some(status);
        self.response = serde_json::from_slice(body)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(body).into_owned()));
    }

    /// How long the call took: until [`Self::finish`], or until now.
    pub fn latency_ms(&self) -> u64 {
        self.latency_ms.unwrap_or_else(|| {
            u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
        })
    }

    /// The call took `ms`, measured by whoever made it.
    pub fn took(&mut self, ms: u64) {
        self.latency_ms = Some(ms);
    }

    fn finish(&mut self) {
        self.took(self.latency_ms());
    }
}

/// Ask for free text: the answer's `content`, empty when the model gave
/// none (a reasoning parser may have moved all of it to
/// `reasoning_content`, which stays readable in the exchange's response).
pub async fn ask_text(state: &RamaState, payer: &Payer, call: SideCall<'_>) -> Answered<String> {
    let (exchange, answer) = ask(state, payer, &call, None).await;
    let answer = answer.map(|response| {
        response
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    });
    Answered { exchange, answer }
}

/// Ask for a JSON object of `shape`. The schema is a request, not a
/// guarantee — a backend may ignore `response_format` — so the answer is
/// read leniently (code fences and stray prose around the object are
/// dropped) and the caller still checks the fields it reads.
pub async fn ask_json(
    state: &RamaState,
    payer: &Payer,
    call: SideCall<'_>,
    shape: JsonShape<'_>,
) -> Answered<Value> {
    let format = json!({
        "type": "json_schema",
        "json_schema": { "name": shape.name, "strict": true, "schema": shape.schema },
    });
    let (mut exchange, answer) = ask(state, payer, &call, Some(format)).await;
    let answer = answer.and_then(|response| {
        let content = response
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .ok_or_else(|| failed("the answer has no content"))?;
        json_object(content)
    });
    if let Err(err) = &answer {
        exchange.error = Some(err.to_string());
    }
    Answered { exchange, answer }
}

/// The JSON object `content` holds: the whole of it, or inside a code fence,
/// or the first balanced object in surrounding prose.
fn json_object(content: &str) -> Result<Value, SideCallError> {
    let unfenced = content
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let parsed = match serde_json::from_str::<Value>(unfenced) {
        Ok(value) => value,
        Err(err) => first_object(content)
            .ok_or_else(|| failed(format!("the answer is not JSON ({err})")))?,
    };
    if parsed.is_object() {
        Ok(parsed)
    } else {
        Err(failed("the answer is not a JSON object"))
    }
}

fn first_object(raw: &str) -> Option<Value> {
    let start = raw.find('{')?;
    let mut depth = 0usize;
    let mut in_str = false;
    let mut escaped = false;
    for (i, ch) in raw[start..].char_indices() {
        match ch {
            '"' if !escaped => in_str = !in_str,
            '\\' if in_str => {
                escaped = !escaped;
                continue;
            }
            '{' if !in_str => depth += 1,
            '}' if !in_str => {
                depth -= 1;
                if depth == 0 {
                    return serde_json::from_str(&raw[start..start + i + 1]).ok();
                }
            }
            _ => {}
        }
        escaped = false;
    }
    None
}

async fn ask(
    state: &RamaState,
    payer: &Payer,
    call: &SideCall<'_>,
    format: Option<Value>,
) -> (SideExchange, Result<Value, SideCallError>) {
    let mut exchange = SideExchange::new(call.purpose);
    let answer = match tokio::time::timeout(
        call.timeout,
        send(state, payer, call, format, &mut exchange),
    )
    .await
    {
        Ok(answer) => answer,
        Err(_) => Err(failed(format!(
            "the model did not answer within {}s",
            call.timeout.as_secs()
        ))),
    };
    exchange.finish();
    payer.meter(state, &exchange);
    if let Err(err) = &answer {
        exchange.error = Some(err.to_string());
    }
    (exchange, answer)
}

async fn send(
    state: &RamaState,
    payer: &Payer,
    call: &SideCall<'_>,
    format: Option<Value>,
    exchange: &mut SideExchange,
) -> Result<Value, SideCallError> {
    let input = if call.no_think {
        format!("{}\n\n/no_think", call.input)
    } else {
        call.input.to_string()
    };
    let messages = json!([
        { "role": "system", "content": call.instructions },
        { "role": "user", "content": input },
    ]);
    let target = route_target(
        state,
        call.model,
        &json!({ "messages": messages, "tools": [] }),
        call.access,
        None,
    )
    .await
    .map_err(failed)?;
    exchange.model = Some(target.model.clone());
    if let Some(exceeded) = payer.over_budget(state, &target.model).await {
        return Err(SideCallError::OverBudget(exceeded));
    }
    let acquired = state
        .upstreams
        .route_access(&target.model, PoolKind::Chat, &target.access)
        .map_err(failed)?;
    let backend = acquired.backend();
    let mut body = json!({
        "model": acquired.resolved_model(),
        "messages": messages,
        "temperature": call.temperature,
        "stream": false,
        "chat_template_kwargs": { "enable_thinking": false },
    });
    if let Some(max_tokens) = call.max_tokens {
        body["max_tokens"] = json!(max_tokens);
    }
    if let Some(format) = format {
        body["response_format"] = format;
    }
    exchange.backend = Some(backend.name.clone());
    exchange.request = body.clone();
    let mut req = state
        .http
        .post(format!("{}/chat/completions", backend.base_url))
        .json(&body);
    if let Some(key) = backend.api_key.as_deref() {
        req = req.bearer_auth(key);
    }
    let resp = req.send().await.map_err(failed)?;
    let status = resp.status();
    let bytes = capped_read::read_capped(resp, capped_read::MODEL_ANSWER_BYTES)
        .await
        .map_err(failed)?;
    drop(acquired);
    exchange.answered(status.as_u16(), &bytes);
    if !status.is_success() {
        return Err(failed(format!(
            "upstream {status}: {}",
            String::from_utf8_lossy(&bytes)
                .chars()
                .take(160)
                .collect::<String>()
        )));
    }
    if !exchange.response.is_object() {
        return Err(failed("the upstream's answer is not JSON"));
    }
    Ok(exchange.response.clone())
}

/// Drop one `<think>…</think>` block, case-insensitive, that a reasoning
/// parser leaked into the content despite the knobs. Conservative: only a
/// balanced pair is removed.
pub fn strip_think_block(s: &str) -> String {
    let lower = s.to_ascii_lowercase();
    let Some(start) = lower.find("<think>") else {
        return s.to_string();
    };
    let after_start = start + "<think>".len();
    let Some(rel_end) = lower[after_start..].find("</think>") else {
        return s.to_string();
    };
    let end = after_start + rel_end + "</think>".len();
    let mut out = String::with_capacity(s.len() - (end - start));
    out.push_str(&s[..start]);
    out.push_str(&s[end..]);
    out
}

#[cfg(test)]
mod tests;
