// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The `lookup` verifier: `verify_<id>()` sends details the visitor gave
//! (read from state, never from the model's arguments) to a granted tool,
//! and writes the configured slots when the tool answers `valid: true`.
//!
//! It proves the visitor knows something, not that they control an address,
//! so the spec gives it an `assurance` label, recorded with every outcome.
//! Attempts are capped per conversation, and a miss reads the same whether
//! nothing matched or something half matched.

use std::sync::Arc;

use aiplane_agents::db::agent_verifiers::{self as rows, Counted, EventKind, MAX_WINDOW};
use aiplane_agents::rates::{self, Rate, RateScope};
use aiplane_core::server::principal::GrantKind;
use serde_json::{Map, Value, json};
use shared::api::ToolDef;

use super::{
    InputSource, Lookup, VerifierRun, answer_object, apply_writes, audit, call_connector, confirms,
    no_args, session_of,
};
use crate::agents::state::{AgentState, TrustedWriter};
use crate::server::tools::mcp::MCP_ID_PREFIX;
use crate::server::tools::{Tool, ToolContext, ToolError, ToolFuture};

pub fn tool_name(id: &str) -> String {
    format!("verify_{id}")
}

pub struct LookupTool {
    name: String,
    cfg: Arc<Lookup>,
    run: VerifierRun,
}

impl LookupTool {
    pub fn new(cfg: Arc<Lookup>, run: VerifierRun) -> Self {
        Self {
            name: tool_name(&cfg.id),
            cfg,
            run,
        }
    }

    fn outcome(&self, outcome: &str) -> Value {
        json!({
            "verifier": self.cfg.id,
            "kind": "lookup",
            "step": "lookup",
            "outcome": outcome,
            "assurance": self.cfg.assurance,
        })
    }

    /// The tool's arguments from state, or the slot to set first.
    fn inputs(&self, state: &AgentState) -> Result<Map<String, Value>, ToolError> {
        let mut args = Map::new();
        for (arg, source) in &self.cfg.inputs {
            let value = match source {
                InputSource::Const(v) => v.clone(),
                InputSource::Slot(slot) => match state.valid(slot) {
                    Some(entry) => entry.value.clone(),
                    None => {
                        return Err(ToolError::InvalidArgs(format!(
                            "slot `{slot}` is not set yet. Ask the visitor for it, record it, \
                             then call {} again.",
                            self.name
                        )));
                    }
                },
            };
            args.insert(arg.clone(), value);
        }
        Ok(args)
    }

    async fn call(&self, ctx: &ToolContext, args: Value) -> Result<Value, ToolError> {
        if let Some(rest) = self.cfg.tool.strip_prefix(MCP_ID_PREFIX)
            && let Some((connector, tool)) = rest.split_once("__")
        {
            return call_connector(&self.run, ctx, connector, tool, args, false).await;
        }
        let unavailable = || {
            ToolError::Failed(format!(
                "verifier `{}` uses tool `{}`, which this agent is not granted or which does not \
                 exist. The agent's owner has to fix the verifier",
                self.cfg.id, self.cfg.tool
            ))
        };
        if !self
            .run
            .principal
            .grants
            .has(GrantKind::Tool, &self.cfg.tool)
        {
            return Err(unavailable());
        }
        let mut tx = aiplane_agents::db::WriteTx::begin(&self.run.state.db)
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        super::validate_verifier_authority_in(&mut tx, &self.run, ctx, &self.cfg.tool).await?;
        tx.commit()
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        let registry = self.run.state.tools();
        let tool = registry.get(&self.cfg.tool).ok_or_else(unavailable)?;
        tool.run(ctx.clone(), args).await
    }
}

impl Tool for LookupTool {
    fn id(&self) -> &str {
        &self.name
    }

    fn schema(&self) -> ToolDef {
        let slots: Vec<&str> = self
            .cfg
            .inputs
            .iter()
            .filter_map(|(_, s)| match s {
                InputSource::Slot(slot) => Some(slot.as_str()),
                InputSource::Const(_) => None,
            })
            .collect();
        ToolDef::function(
            &self.name,
            format!(
                "Check the details the visitor gave against the records: {}. Record them first. \
                 Takes no arguments. Its result says whether they were confirmed.",
                slots.join(", ")
            ),
            json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        )
    }

    fn run<'a>(&'a self, ctx: ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            no_args(
                &self.name,
                &args,
                "The details it checks come from the conversation's state.",
            )?;
            let session = session_of(&ctx, &self.name)?;
            let db = &self.run.state.db;
            let state = AgentState::load(db, &self.run.schema, session)
                .await
                .map_err(|e| ToolError::Failed(e.to_string()))?;
            let inputs = self.inputs(&state)?;
            let now = (self.run.options.now)();
            let attempt = rows::window(
                &self.cfg.id,
                EventKind::Lookup,
                RateScope::Session,
                Rate {
                    max: self.cfg.max_attempts,
                    per: MAX_WINDOW,
                },
                Counted::Session(session),
            );
            let admitted = rates::record_now(db, &self.run.principal.id, &[attempt], now)
                .await
                .map_err(|e| ToolError::Failed(e.to_string()))?;
            let Ok(admitted) = admitted else {
                audit(&self.run, &ctx, self.outcome("too_many_attempts")).await;
                return Ok(json!({
                    "verified": false,
                    "reason": "too_many_attempts",
                    "next": "The details could not be confirmed and no more attempts are \
                             allowed in this conversation. Offer the visitor another way to \
                             reach support.",
                }));
            };
            let answer = self
                .call(&ctx, Value::Object(inputs.clone()))
                .await
                .ok()
                .as_ref()
                .and_then(answer_object);
            if !confirms(answer.as_ref()) {
                audit(&self.run, &ctx, self.outcome("not_confirmed")).await;
                return Ok(json!({
                    "verified": false,
                    "reason": "not_confirmed",
                    "attempts_left": self.cfg.max_attempts.saturating_sub(admitted.seen[0] + 1),
                    "next": "The details were not confirmed. Ask the visitor to check them; \
                             correct the slots and call this again.",
                }));
            }
            let written = apply_writes(
                &self.run,
                &ctx,
                &self.cfg.id,
                &self.cfg.tool,
                &self.cfg.writes,
                answer.as_ref().expect("a confirming answer is an object"),
                &inputs,
                TrustedWriter::Verifier(self.cfg.id.clone()),
            )
            .await?;
            let mut detail = self.outcome("verified");
            detail["slots"] = json!(written);
            audit(&self.run, &ctx, detail).await;
            Ok(json!({
                "verified": true,
                "slots": written,
                "assurance": self.cfg.assurance,
            }))
        })
    }
}
