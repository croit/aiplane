// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The `mcp_code` verifier: a one-time code the agent's connector sends and
//! checks, typed by the visitor into a secure field.
//!
//! - `verify_<id>_request_code()` sends a code to the address in the email
//!   slot, then pauses the turn for the visitor to type it.
//! - `verify_<id>_submit_code()` asks again for the code already sent, after
//!   a wrong one.
//!
//! Both take no arguments, and both check the typed code when the turn
//! resumes with it. What the gateway decides itself, because the connector
//! is the customer's and the visitor is untrusted:
//!
//! - **Expiry**: a code is accepted until `code_ttl` after it was sent; the
//!   field itself waits no longer either.
//! - **Attempts**: at most `max_attempts` checks per code, counted in one
//!   statement so parallel guesses cannot share an attempt.
//! - **Sends**: sliding windows per address, client IP and conversation.
//! - **One address**: a code is checked only against the address it went
//!   to; changing the email slot in between voids it.
//! - **No enumeration**: the visitor and the model hear the same thing for a
//!   registered and an unknown address — the connector's answer to
//!   `send_code` is recorded for the owner, never relayed.
//!
//! The code reaches only the connector's `check_code`, whose arguments are
//! declared sensitive: `mcp_tool_audit` records the redaction marker.

use std::sync::Arc;

use aiplane_agents::db::agent_verifiers::{self as rows, Counted, EventKind, PendingCode};
use aiplane_agents::db::visitor_sessions;
use aiplane_agents::rates::{self, RateExceeded, RateScope};
use aiplane_core::server::crypto::sha256_hex;
use jiff::Timestamp;
use serde_json::{Map, Value, json};
use session_core::db::Decision;
use session_core::i18n::t;
use shared::api::ToolDef;

use super::{
    McpCode, VerifierRun, answer_object, apply_writes, audit, call_connector, confirms, email_hash,
    no_args, session_of,
};
use crate::agents::state::{AgentState, TrustedWriter};
use crate::server::tools::{Tool, ToolContext, ToolError, ToolFuture};
use crate::suspend::{Suspend, SuspendRequest, tool_suspend};

const NO_ARGS: &str = "The address comes from the email slot, and the visitor types the code \
                       into a field only they see.";

pub fn request_tool_name(id: &str) -> String {
    format!("verify_{id}_request_code")
}

pub fn submit_tool_name(id: &str) -> String {
    format!("verify_{id}_submit_code")
}

/// `verify_<id>_request_code`.
pub struct RequestCode {
    name: String,
    cfg: Arc<McpCode>,
    run: VerifierRun,
}

impl RequestCode {
    pub fn new(cfg: Arc<McpCode>, run: VerifierRun) -> Self {
        Self {
            name: request_tool_name(&cfg.id),
            cfg,
            run,
        }
    }
}

/// `verify_<id>_submit_code`.
pub struct SubmitCode {
    name: String,
    cfg: Arc<McpCode>,
    run: VerifierRun,
}

impl SubmitCode {
    pub fn new(cfg: Arc<McpCode>, run: VerifierRun) -> Self {
        Self {
            name: submit_tool_name(&cfg.id),
            cfg,
            run,
        }
    }
}

fn no_params() -> Value {
    json!({ "type": "object", "properties": {}, "additionalProperties": false })
}

impl Tool for RequestCode {
    fn id(&self) -> &str {
        &self.name
    }

    fn schema(&self) -> ToolDef {
        ToolDef::function(
            &self.name,
            format!(
                "Verify the visitor's email address: send a one-time code to the address in \
                 slot `{slot}` and ask the visitor to type it into a secure field. Set \
                 `{slot}` first. Takes no arguments; you never see the code. Its result says \
                 whether the visitor is verified.",
                slot = self.cfg.email_slot
            ),
            no_params(),
        )
    }

    fn sensitive_args(&self) -> bool {
        true
    }

    fn run<'a>(&'a self, ctx: ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            no_args(&self.name, &args, NO_ARGS)?;
            let flow = Flow {
                cfg: &self.cfg,
                run: &self.run,
                ctx: &ctx,
                tool: &self.name,
            };
            match &ctx.suspend {
                Suspend::Available => flow.send().await,
                Suspend::Decided(_, decision) => flow.check(decision).await,
                Suspend::Unavailable => Err(unavailable(&self.name)),
            }
        })
    }
}

impl Tool for SubmitCode {
    fn id(&self) -> &str {
        &self.name
    }

    fn schema(&self) -> ToolDef {
        ToolDef::function(
            &self.name,
            format!(
                "Ask the visitor again for the code already sent by {}, after they typed a \
                 wrong one. Takes no arguments; you never see the code.",
                request_tool_name(&self.cfg.id)
            ),
            no_params(),
        )
    }

    fn sensitive_args(&self) -> bool {
        true
    }

    fn run<'a>(&'a self, ctx: ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            no_args(&self.name, &args, NO_ARGS)?;
            let flow = Flow {
                cfg: &self.cfg,
                run: &self.run,
                ctx: &ctx,
                tool: &self.name,
            };
            match &ctx.suspend {
                Suspend::Available => flow.ask_again().await,
                Suspend::Decided(_, decision) => flow.check(decision).await,
                Suspend::Unavailable => Err(unavailable(&self.name)),
            }
        })
    }
}

fn unavailable(tool: &str) -> ToolError {
    ToolError::Failed(format!(
        "{tool} needs the visitor to type a code, and this run cannot ask them. Do not retry it \
         here."
    ))
}

fn db_failed(err: impl std::fmt::Display) -> ToolError {
    ToolError::Failed(format!(
        "the gateway could not read or record the verification ({err}); tell the visitor to try \
         again in a moment"
    ))
}

struct Flow<'a> {
    cfg: &'a McpCode,
    run: &'a VerifierRun,
    ctx: &'a ToolContext,
    tool: &'a str,
}

impl Flow<'_> {
    fn now(&self) -> Timestamp {
        (self.run.options.now)()
    }

    fn request_name(&self) -> String {
        request_tool_name(&self.cfg.id)
    }

    /// The address in the email slot, if the slot holds a valid one.
    async fn email(&self, session: &str) -> Result<Option<String>, ToolError> {
        let state = AgentState::load(&self.run.state.db, &self.run.schema, session)
            .await
            .map_err(db_failed)?;
        Ok(state
            .valid(&self.cfg.email_slot)
            .and_then(|e| e.value.as_str())
            .map(str::to_string))
    }

    async fn ip_hash(&self) -> Option<String> {
        let visitor = self.ctx.chain()?.visitor_id.clone()?;
        let session = visitor_sessions::get(&self.run.state.db, &visitor)
            .await
            .ok()
            .flatten()?;
        session.client_ip.map(|ip| sha256_hex(ip.as_bytes()))
    }

    fn outcome(&self, step: &str, outcome: &str) -> Value {
        json!({
            "verifier": self.cfg.id,
            "kind": "mcp_code",
            "step": step,
            "outcome": outcome,
            "assurance": self.cfg.assurance,
        })
    }

    /// Count this send against every window and record it, in one step: the
    /// first window it does not fit, if any.
    async fn record_send(
        &self,
        session: &str,
        email: &str,
        ip: Option<&str>,
        now: Timestamp,
    ) -> Result<Option<RateExceeded>, ToolError> {
        let limits = self.cfg.limits;
        let id = &self.cfg.id;
        let mut windows = vec![
            rows::window(
                id,
                EventKind::Send,
                RateScope::Email,
                limits.email,
                Counted::Email(email),
            ),
            rows::window(
                id,
                EventKind::Send,
                RateScope::Session,
                limits.session,
                Counted::Session(session),
            ),
        ];
        if let Some(ip) = ip {
            windows.push(rows::window(
                id,
                EventKind::Send,
                RateScope::Ip,
                limits.ip,
                Counted::Ip(ip),
            ));
        }
        rates::record_now(&self.run.state.db, &self.run.principal.id, &windows, now)
            .await
            .map(Result::err)
            .map_err(db_failed)
    }

    async fn send(&self) -> Result<Value, ToolError> {
        let session = session_of(self.ctx, self.tool)?;
        let Some(email) = self.email(session).await? else {
            return Err(ToolError::InvalidArgs(format!(
                "slot `{slot}` holds no valid email address yet. Ask the visitor for it, call \
                 set_{slot}, then call {} again.",
                self.tool,
                slot = self.cfg.email_slot
            )));
        };
        let now = self.now();
        let hashed = email_hash(&email);
        let ip = self.ip_hash().await;
        if let Some(exceeded) = self
            .record_send(session, &hashed, ip.as_deref(), now)
            .await?
        {
            let mut detail = self.outcome("send", "rate_limited");
            detail["scope"] = json!(exceeded.scope.as_str());
            detail["retry_after_secs"] = json!(exceeded.retry_after_secs);
            audit(self.run, self.ctx, detail).await;
            return Ok(json!({
                "sent": false,
                "reason": "rate_limited",
                "retry_after_secs": exceeded.retry_after_secs,
                "next": "Too many codes were requested. Tell the visitor to try again later; do \
                         not call this again in this turn."
            }));
        }
        // Registered or not, the visitor and the model are told the same:
        // the connector's answer only reaches the owner's audit trail.
        let delivery = match call_connector(
            self.run,
            self.ctx,
            &self.cfg.connector,
            &self.cfg.send_tool,
            json!({ "email": email }),
            false,
        )
        .await
        {
            Ok(_) => "accepted",
            Err(err) => {
                tracing::info!(agent = %self.run.principal.name, verifier = %self.cfg.id,
                    error = %err, "verifier send_code refused or failed");
                "refused"
            }
        };
        let expires = now.checked_add(self.cfg.code_ttl).unwrap_or(now);
        rows::put_code(
            &self.run.state.db,
            session,
            &self.cfg.id,
            &hashed,
            now,
            expires,
        )
        .await
        .map_err(db_failed)?;
        let mut detail = self.outcome("send", "code_sent");
        detail["delivery"] = json!(delivery);
        detail["expires_at"] = json!(expires);
        audit(self.run, self.ctx, detail).await;
        Ok(tool_suspend(SuspendRequest::secure_input(
            t(
                self.ctx.conversation_lang().await,
                "agent-verifier-code-sent",
            ),
            self.cfg.code_ttl.unsigned_abs(),
        )))
    }

    /// The code outstanding for this conversation, if it can still be
    /// checked; otherwise the result that tells the model what to do.
    async fn usable_code(&self, session: &str) -> Result<Result<PendingCode, Value>, ToolError> {
        let pending = rows::pending_code(&self.run.state.db, session, &self.cfg.id)
            .await
            .map_err(db_failed)?;
        let request = self.request_name();
        let Some(pending) = pending else {
            return Ok(Err(json!({
                "verified": false,
                "reason": "no_code",
                "next": format!("No code was sent in this conversation. Call {request}."),
            })));
        };
        if self.now() > pending.expires_at {
            return Ok(Err(json!({
                "verified": false,
                "reason": "expired",
                "next": format!("The code expired. Call {request} to send a new one."),
            })));
        }
        if pending.attempts >= self.cfg.max_attempts {
            return Ok(Err(locked(&request)));
        }
        Ok(Ok(pending))
    }

    async fn ask_again(&self) -> Result<Value, ToolError> {
        let session = session_of(self.ctx, self.tool)?;
        let pending = match self.usable_code(session).await? {
            Ok(p) => p,
            Err(result) => return Ok(result),
        };
        let left = (pending.expires_at.as_second() - self.now().as_second()).max(1);
        Ok(tool_suspend(SuspendRequest::secure_input(
            t(
                self.ctx.conversation_lang().await,
                "agent-verifier-code-again",
            ),
            std::time::Duration::from_secs(left.unsigned_abs()),
        )))
    }

    async fn check(&self, decision: &Decision) -> Result<Value, ToolError> {
        let Decision::Value { value } = decision else {
            return Err(ToolError::Failed(
                "no code was entered, so nothing was checked".into(),
            ));
        };
        let session = session_of(self.ctx, self.tool)?;
        let code = match value {
            Value::String(s) => s.trim().to_string(),
            Value::Number(n) => n.to_string(),
            _ => String::new(),
        };
        let request = self.request_name();
        let submit = submit_tool_name(&self.cfg.id);
        if code.is_empty() {
            return Ok(json!({
                "verified": false,
                "reason": "empty",
                "next": format!("The visitor entered nothing. Call {submit} to ask again."),
            }));
        }
        let pending = match self.usable_code(session).await? {
            Ok(p) => p,
            Err(result) => {
                audit(
                    self.run,
                    self.ctx,
                    self.outcome("check", result["reason"].as_str().unwrap_or_default()),
                )
                .await;
                return Ok(result);
            }
        };
        let email = self.email(session).await?;
        let Some(email) = email.filter(|e| email_hash(e) == pending.email_hash) else {
            audit(self.run, self.ctx, self.outcome("check", "email_changed")).await;
            return Ok(json!({
                "verified": false,
                "reason": "email_changed",
                "next": format!(
                    "The email address changed after the code was sent. Call {request} to send \
                     a code to the current one."
                ),
            }));
        };
        let Some(attempt) = rows::take_attempt(
            &self.run.state.db,
            session,
            &self.cfg.id,
            self.cfg.max_attempts,
        )
        .await
        .map_err(db_failed)?
        else {
            audit(self.run, self.ctx, self.outcome("check", "locked")).await;
            return Ok(locked(&request));
        };
        let answer = call_connector(
            self.run,
            self.ctx,
            &self.cfg.connector,
            &self.cfg.check_tool,
            json!({ "email": email, "code": code }),
            true,
        )
        .await;
        let answer = answer
            .ok()
            .map(|body| self.ctx.redaction().withhold(body))
            .as_ref()
            .and_then(answer_object);
        if !confirms(answer.as_ref()) {
            let left = self.cfg.max_attempts.saturating_sub(attempt);
            let outcome = if left == 0 { "locked" } else { "wrong_code" };
            let mut detail = self.outcome("check", outcome);
            detail["attempt"] = json!(attempt);
            audit(self.run, self.ctx, detail).await;
            if left == 0 {
                return Ok(locked(&request));
            }
            return Ok(json!({
                "verified": false,
                "reason": "wrong_code",
                "attempts_left": left,
                "next": format!(
                    "The code did not match. Tell the visitor, then call {submit} so they can \
                     enter it again."
                ),
            }));
        }
        let inputs = Map::from_iter([("email".to_string(), Value::String(email))]);
        let written = apply_writes(
            self.run,
            self.ctx,
            &self.cfg.id,
            &crate::server::tools::mcp::tool_id(&self.cfg.connector, &self.cfg.check_tool),
            &self.cfg.writes,
            answer.as_ref().expect("a confirming answer is an object"),
            &inputs,
            TrustedWriter::Verifier(self.cfg.id.clone()),
        )
        .await;
        let written = match written {
            Ok(w) => w,
            Err(err) => {
                audit(self.run, self.ctx, self.outcome("check", "write_failed")).await;
                return Err(err);
            }
        };
        rows::clear_code(&self.run.state.db, session, &self.cfg.id)
            .await
            .map_err(db_failed)?;
        let mut detail = self.outcome("check", "verified");
        detail["attempt"] = json!(attempt);
        detail["slots"] = json!(written);
        audit(self.run, self.ctx, detail).await;
        Ok(json!({ "verified": true, "slots": written }))
    }
}

fn locked(request: &str) -> Value {
    json!({
        "verified": false,
        "reason": "locked",
        "next": format!(
            "This code had too many wrong attempts and no longer works. Call {request} to send \
             a new code."
        ),
    })
}
