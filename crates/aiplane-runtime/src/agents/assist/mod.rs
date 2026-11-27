// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The prompt assistant (`docs/agent-builder.md` → "Prompt assistant"): from a
//! manager's scenario, one model call proposes a value for every setup step
//! of an agent plus test cases; another improves one text.
//!
//! **It writes nothing to the agent.** The proposal is checked against the
//! draft ([`review`]) and returned; the UI applies what the manager accepts,
//! step by step, through the ordinary draft, grant and test routes, which
//! check it again. So a scenario that talks the model into proposing
//! anything at all can at most produce an offer the manager sees.
//!
//! **A manager's call, not an agent run.** The call is a side call
//! ([`side_call::ask_json`]) on a model the manager may use, under the
//! manager's own spend limits, and is a usage row of the manager's
//! (`UsageSource::Chat`, like the rest of the session UI). It is in no conversation chain; the agent's own chain gets one
//! `assist_suggested` event per call with who asked, the scenario (the
//! manager's own words, kept for the audit), the model, token counts
//! and which steps were offered or dropped.

use std::time::Duration;

use crate::server::model_choices;
use aiplane_agents::db::agent_audit::{AuditKind, NewEvent};
use aiplane_agents::rates::{self, Counter, Rate, RateExceeded, RateScope, Window};
use aiplane_core::server::db::DbError;
use aiplane_core::server::db::usage::UsageSource;
use aiplane_core::server::db::users::User;
use aiplane_core::server::limits::LimitExceeded;
use aiplane_core::server::upstreams::{PoolAccess, PoolKind};
use jiff::{SignedDuration, Timestamp};
use serde::Serialize;
use serde_json::{Value, json};

use super::audit::{self, LogError};
use crate::rama_server::state::RamaState;
use crate::server::side_call::{
    self, Answered, JsonShape, Payer, SideCall, SideCallError, SideExchange,
};

pub mod handoffs;
mod proposal;
pub mod review;
pub mod tone;

pub use proposal::changes_schema;
pub use review::{Applied, ReviewContext, Suggestion, apply_changes};

/// The longest scenario the assistant reads.
pub const MAX_SCENARIO_CHARS: usize = 8_000;
/// The longest text "improve" reads.
pub const MAX_IMPROVE_CHARS: usize = 8_000;
pub const MAX_TEMPLATE_CHARS: usize = 200;
/// Calls per manager and agent, suggest and improve together.
pub const ASSIST_RATE: Rate = Rate {
    max: 30,
    per: SignedDuration::from_secs(60 * 60),
};
pub const MAX_TESTS: usize = 6;
const SUGGEST_TIMEOUT: Duration = Duration::from_secs(120);
const IMPROVE_TIMEOUT: Duration = Duration::from_secs(60);

/// The friendly kinds of information a slot step offers.
pub const SLOT_TYPES: &[&str] = &[
    "text",
    "long_text",
    "email",
    "number",
    "whole_number",
    "yes_no",
    "choice",
];
/// The identity checks the identity step knows.
pub const IDENTITY_METHODS: &[&str] = &["none", "website_login", "email_code", "lookup"];
/// The hand-off target that is a person rather than an agent.
pub const HUMAN_TARGET: &str = "human";

/// A tool this manager may grant.
#[derive(Debug, Clone)]
pub struct Ability {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

/// Knowledge search, offered by knowledge base ([`Knowledge`]) and never as
/// an ability of its own: without a collection it finds nothing.
pub const RAG_SEARCH: &str = "rag_search";
pub const RAG_LIST: &str = "rag_list_collections";

pub fn is_knowledge_tool(id: &str) -> bool {
    id == RAG_SEARCH || id == RAG_LIST
}

/// A knowledge base (RAG collection) this manager may grant.
#[derive(Debug, Clone)]
pub struct Knowledge {
    pub id: String,
    pub name: String,
    /// What the base holds, as the admin described it; the model matches on it.
    pub description: Option<String>,
}

/// An agent this manager may hand conversations to.
#[derive(Debug, Clone)]
pub struct Target {
    pub id: String,
    pub name: String,
}

/// What the model may choose among, resolved for the calling manager.
#[derive(Debug, Clone, Default)]
pub struct Candidates {
    pub abilities: Vec<Ability>,
    pub knowledge: Vec<Knowledge>,
    pub agents: Vec<Target>,
    /// The chat models this manager may use and grant, for an architect's
    /// model choice. The suggestion's schema does not offer a model.
    pub models: Vec<String>,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ImproveField {
    Task,
    Tone,
    Refusal,
}

impl ImproveField {
    fn as_str(self) -> &'static str {
        match self {
            Self::Task => "task",
            Self::Tone => "tone",
            Self::Refusal => "refusal",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AssistError {
    #[error("{0}")]
    Input(String),
    #[error(
        "model `{0}` is not a chat model you may use — leave `model` out to let the assistant \
         pick one, or name one listed under `models.chat` in GET /api/v0/agent-resources"
    )]
    ModelNotAllowed(String),
    #[error(
        "none of the chat models you may use is served right now, so the assistant cannot run \
         — try again shortly, or ask an admin which models your groups may use"
    )]
    NoModel,
    #[error(
        "you asked the assistant {} times within {} for this agent — wait {} seconds and try \
         again",
        .0.max, rate_window(.0.per), .0.retry_after_secs
    )]
    RateLimited(RateExceeded),
    #[error(
        "you are over your usage limit, so the assistant cannot call a model for you — see \
         /usage for your limits; it frees up in {} seconds",
        .0.retry_after_secs
    )]
    OverBudget(LimitExceeded),
    #[error("the assistant's model call failed: {0} — try again, or pick another `model`")]
    Model(String),
    #[error(transparent)]
    Log(#[from] LogError),
    #[error("the assistant could not reach the database: {0}")]
    Db(#[from] DbError),
}

impl AssistError {
    /// Seconds to wait before asking again, for a `Retry-After`.
    pub fn retry_after_secs(&self) -> Option<i64> {
        match self {
            Self::RateLimited(r) => Some(r.retry_after_secs),
            Self::OverBudget(l) => Some(l.retry_after_secs),
            _ => None,
        }
    }
}

fn rate_window(per: SignedDuration) -> String {
    match per.as_secs() {
        s if s % 3600 == 0 => format!("{} h", s / 3600),
        s if s % 60 == 0 => format!("{} min", s / 60),
        s => format!("{s} s"),
    }
}

/// A suggestion for agent `agent_id`, as the endpoint returns it.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct Suggested {
    #[serde(flatten)]
    pub suggestion: Suggestion,
    pub model: String,
    pub usage: Value,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct ImprovedText {
    pub field: ImproveField,
    pub suggestion: String,
    pub why: String,
    pub model: String,
    pub usage: Value,
}

/// What `POST …/assist/suggest` asks.
pub struct SuggestRequest<'a> {
    pub scenario: &'a str,
    pub template: Option<&'a str>,
    /// The draft the manager is editing: theirs, or the stored one.
    pub base: &'a Value,
    pub model: Option<&'a str>,
}

/// The manager asking, about which agent.
pub struct Asker<'a> {
    pub state: &'a RamaState,
    pub user: &'a User,
    pub agent_id: &'a str,
}

fn require_text(text: &str, what: &str, max: usize) -> Result<(), AssistError> {
    if text.trim().is_empty() {
        return Err(AssistError::Input(format!(
            "the {what} is empty — describe what the agent should do"
        )));
    }
    let chars = text.chars().count();
    if chars > max {
        return Err(AssistError::Input(format!(
            "the {what} is {chars} characters long; the assistant reads at most {max} — shorten \
             it to what the agent needs to know"
        )));
    }
    Ok(())
}

impl Asker<'_> {
    /// Propose a setup for `request.scenario`, checked against
    /// `request.base` in `ctx`.
    pub async fn suggest(
        &self,
        request: SuggestRequest<'_>,
        ctx: &ReviewContext<'_>,
    ) -> Result<Suggested, AssistError> {
        require_text(request.scenario, "scenario", MAX_SCENARIO_CHARS)?;
        if let Some(t) = request.template
            && t.chars().count() > MAX_TEMPLATE_CHARS
        {
            return Err(AssistError::Input(format!(
                "the template name is longer than {MAX_TEMPLATE_CHARS} characters — pass the \
                 template's id"
            )));
        }
        let draft_model = request.base.pointer("/main/model").and_then(Value::as_str);
        let (model, access) = self.admit(request.model, draft_model).await?;
        let input = proposal::suggest_input(
            request.scenario,
            request.template,
            request.base,
            ctx.candidates,
        );
        let answered = self
            .ask(
                SideCall {
                    purpose: "assist_suggest",
                    model: &model,
                    access: &access,
                    instructions: proposal::SUGGEST_INSTRUCTIONS,
                    input: &input,
                    max_tokens: None,
                    no_think: false,
                    timeout: SUGGEST_TIMEOUT,
                },
                JsonShape {
                    name: "agent_setup",
                    schema: proposal::suggest_schema(ctx.candidates),
                },
            )
            .await?;
        let exchange = &answered.exchange;
        let suggestion = answered
            .answer
            .as_ref()
            .ok()
            .map(|answer| review::review(answer, request.base, ctx));
        let mut detail = json!({
            "action": "suggest",
            "scenario": request.scenario,
            "template": request.template,
        });
        if let Some(s) = &suggestion {
            detail["offered"] = json!(s.steps.offered());
            detail["dropped"] = json!(s.dropped);
        }
        self.record(exchange, detail).await?;
        let suggestion = suggestion.ok_or_else(|| model_error(exchange))?;
        Ok(Suggested {
            suggestion,
            model: exchange.model.clone().unwrap_or(model),
            usage: usage_of(exchange),
        })
    }

    /// A better version of `text` for `field`.
    pub async fn improve(
        &self,
        field: ImproveField,
        text: &str,
        requested_model: Option<&str>,
        draft: &Value,
    ) -> Result<ImprovedText, AssistError> {
        require_text(text, "text", MAX_IMPROVE_CHARS)?;
        let draft_model = draft.pointer("/main/model").and_then(Value::as_str);
        let (model, access) = self.admit(requested_model, draft_model).await?;
        let input = json!({ "field": proposal::improve_purpose(field), "text": text }).to_string();
        let answered = self
            .ask(
                SideCall {
                    purpose: "assist_improve",
                    model: &model,
                    access: &access,
                    instructions: proposal::IMPROVE_INSTRUCTIONS,
                    input: &input,
                    max_tokens: None,
                    no_think: false,
                    timeout: IMPROVE_TIMEOUT,
                },
                JsonShape {
                    name: "improved_text",
                    schema: proposal::improve_schema(),
                },
            )
            .await?;
        let exchange = &answered.exchange;
        let improved = answered
            .answer
            .as_ref()
            .map_err(ToString::to_string)
            .and_then(|a| {
                serde_json::from_value::<proposal::Improved>(a.clone())
                    .map_err(|e| format!("the answer does not read as an improved text ({e})"))
                    .and_then(|i| {
                        if i.suggestion.trim().is_empty() {
                            Err("the model returned an empty text".to_string())
                        } else {
                            Ok(i)
                        }
                    })
            });
        self.record(
            exchange,
            json!({
                "action": "improve",
                "field": field.as_str(),
                "text": text,
                "error": improved.as_ref().err(),
            }),
        )
        .await?;
        let improved = improved.map_err(AssistError::Model)?;
        Ok(ImprovedText {
            field,
            suggestion: improved.suggestion.trim().to_string(),
            why: improved.why.trim().to_string(),
            model: exchange.model.clone().unwrap_or(model),
            usage: usage_of(exchange),
        })
    }

    /// Ask the model as the manager; a spent limit refuses before the model
    /// is called, and the refused call leaves nothing in the agent's log.
    async fn ask(
        &self,
        call: SideCall<'_>,
        shape: JsonShape<'_>,
    ) -> Result<Answered<Value>, AssistError> {
        let payer = Payer::person(
            &self.user.id,
            &self.user.roles,
            Some(self.user.email.clone()),
            UsageSource::Chat,
        );
        let answered = side_call::ask_json(self.state, &payer, call, shape).await;
        if let Err(SideCallError::OverBudget(exceeded)) = &answered.answer {
            return Err(AssistError::OverBudget(exceeded.clone()));
        }
        Ok(answered)
    }

    /// The model to ask, once the manager's rate allows a call: `requested`
    /// if they may use it, else the draft's `main.model` if they may, else
    /// their default chat model ([`choose_model`]).
    async fn admit(
        &self,
        requested: Option<&str>,
        draft_model: Option<&str>,
    ) -> Result<(String, PoolAccess), AssistError> {
        let access = self.state.pool_access_for(&self.user.roles);
        let model = choose_model(self.state, &access, requested, draft_model).await?;
        let window = Window {
            scope: RateScope::Manager,
            rate: ASSIST_RATE,
            counter: Counter::new(format!("assist:{}", self.user.id)),
        };
        rates::record_now(&self.state.db, self.agent_id, &[window], Timestamp::now())
            .await?
            .map_err(AssistError::RateLimited)?;
        Ok((model, access))
    }

    async fn record(&self, exchange: &SideExchange, mut detail: Value) -> Result<(), AssistError> {
        detail["model"] = json!(exchange.model);
        detail["usage"] = usage_of(exchange);
        detail["latency_ms"] = json!(exchange.latency_ms());
        if let Some(error) = &exchange.error {
            detail["error"] = json!(error);
        }
        audit::record_event(
            &self.state.db,
            NewEvent::new(AuditKind::AssistSuggested, self.agent_id, detail)
                .by(Some(&self.user.id)),
        )
        .await?;
        Ok(())
    }
}

fn model_error(exchange: &SideExchange) -> AssistError {
    AssistError::Model(exchange.error.clone().unwrap_or_else(|| "no answer".into()))
}

fn usage_of(exchange: &SideExchange) -> Value {
    let u = exchange.response.get("usage");
    json!({
        "prompt_tokens": u.and_then(|u| u.get("prompt_tokens")),
        "completion_tokens": u.and_then(|u| u.get("completion_tokens")),
        "total_tokens": u.and_then(|u| u.get("total_tokens")),
    })
}

/// The chat model a manager with `access` asks: `requested` if it is one of
/// the models they may use (the chat picker's list), else `draft_model` if
/// it is, else the first of that list — the gateway's default chat model
/// when they may use it.
pub async fn choose_model(
    state: &RamaState,
    access: &PoolAccess,
    requested: Option<&str>,
    draft_model: Option<&str>,
) -> Result<String, AssistError> {
    let offered: Vec<String> = model_choices::offered(state, PoolKind::Chat, access)
        .await
        .into_iter()
        .map(|c| c.id)
        .collect();
    let usable = |model: &str| offered.iter().any(|m| m == model);
    if let Some(model) = requested {
        return if usable(model) {
            Ok(model.to_string())
        } else {
            Err(AssistError::ModelNotAllowed(model.to_string()))
        };
    }
    if let Some(model) = draft_model.filter(|m| usable(m)) {
        return Ok(model.to_string());
    }
    offered.into_iter().next().ok_or(AssistError::NoModel)
}
