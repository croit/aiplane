// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The generated `set_<slot>(value)` tools (`docs/agent-runs.md` → "Synthetic tools").
//!
//! One tool per slot whose `set_by` lists `llm`, and none for any other: a
//! verifier-only slot has no tool to call, so no argument the model invents
//! can reach it. Each tool takes exactly `value`; the slot comes from the tool,
//! the conversation from [`ToolContext::session_id`] and the provenance is
//! always `llm`. All three are the gateway's, never the model's.
//!
//! [`SlotTools`] is a [`ToolSource`], so the run that offers them composes it
//! with the principal's granted tools the way MCP and ComfyUI tools compose.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{Value, json};
use shared::api::ToolDef;

use super::state::{Clock, StateSchema, StateWriteError, system_clock, write_from_model};
use crate::server::tools::{Tool, ToolContext, ToolError, ToolFuture, ToolSource};

const SET_TOOL_PREFIX: &str = "set_";

pub fn set_tool_name(slot: &str) -> String {
    format!("{SET_TOOL_PREFIX}{slot}")
}

/// `set_<slot>` for one model-writable slot.
pub struct SetSlotTool {
    id: String,
    slot: String,
    schema: Arc<StateSchema>,
    clock: Clock,
}

impl Tool for SetSlotTool {
    fn id(&self) -> &str {
        &self.id
    }

    fn schema(&self) -> ToolDef {
        let def = self.slot_def();
        let about = def
            .and_then(|d| d.description.as_deref())
            .map(|d| format!(" ({d})"))
            .unwrap_or_default();
        ToolDef::function(
            &self.id,
            format!(
                "Record `{slot}`{about} in this conversation's state. The gateway checks the \
                 value; an invalid one is refused with the reason, so fix it and call again. \
                 Call it again to correct a value.",
                slot = self.slot
            ),
            json!({
                "type": "object",
                "properties": {
                    "value": def.map(|d| d.value_schema()).unwrap_or(json!({})),
                },
                "required": ["value"],
                "additionalProperties": false,
            }),
        )
    }

    fn run<'a>(&'a self, ctx: ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let Some(session_id) = ctx.session_id.as_deref() else {
                return Err(ToolError::Failed(format!(
                    "{} only works inside an agent conversation, and this call has none. Do not \
                     retry.",
                    self.id
                )));
            };
            let value = self.value_arg(args)?;
            let entry = write_from_model(
                &ctx.db,
                &self.schema,
                session_id,
                &self.slot,
                value,
                (self.clock)(),
            )
            .await
            .map_err(|err| match err {
                StateWriteError::Invalid { .. } => ToolError::InvalidArgs(format!(
                    "{err}. Fix the value and call {} again.",
                    self.id
                )),
                StateWriteError::UnknownSlot { .. } | StateWriteError::NotWritable { .. } => {
                    ToolError::InvalidArgs(err.to_string())
                }
                StateWriteError::Db { .. } => ToolError::Failed(err.to_string()),
            })?;
            Ok(json!({ "slot": self.slot, "status": "set", "value": entry.value }))
        })
    }
}

impl SetSlotTool {
    fn slot_def(&self) -> Option<&super::state::SlotDef> {
        self.schema.slot(&self.slot)
    }

    /// Exactly `{"value": …}`. Anything else is refused rather than ignored,
    /// so a model that tries to pass a slot, a session or a provenance learns
    /// that those are not its to choose.
    fn value_arg(&self, args: Value) -> Result<Value, ToolError> {
        let Value::Object(mut map) = args else {
            return Err(ToolError::InvalidArgs(format!(
                "{} takes an object {{\"value\": …}}",
                self.id
            )));
        };
        let extra: Vec<String> = map
            .keys()
            .filter(|k| k.as_str() != "value")
            .map(|k| format!("`{k}`"))
            .collect();
        if !extra.is_empty() {
            return Err(ToolError::InvalidArgs(format!(
                "{} takes only `value`; {} is not an argument. The slot, the conversation and who \
                 wrote the value are set by the gateway. Call it again with just {{\"value\": …}}.",
                self.id,
                extra.join(", ")
            )));
        }
        map.remove("value").ok_or_else(|| {
            ToolError::InvalidArgs(format!(
                "{} needs `value`. Call it again with {{\"value\": …}}.",
                self.id
            ))
        })
    }
}

/// Every `set_<slot>` tool of one agent's state.
pub struct SlotTools {
    tools: BTreeMap<String, Arc<SetSlotTool>>,
}

impl SlotTools {
    pub fn new(schema: Arc<StateSchema>) -> Self {
        Self::with_clock(schema, system_clock())
    }

    pub fn with_clock(schema: Arc<StateSchema>, clock: Clock) -> Self {
        let tools = schema
            .slots()
            .filter(|d| d.model_writable())
            .map(|d| {
                let id = set_tool_name(&d.name);
                let tool = SetSlotTool {
                    id: id.clone(),
                    slot: d.name.clone(),
                    schema: Arc::clone(&schema),
                    clock: Arc::clone(&clock),
                };
                (id, Arc::new(tool))
            })
            .collect();
        Self { tools }
    }

    /// Every definition, in slot order.
    pub fn definitions(&self) -> Vec<ToolDef> {
        self.tools.values().map(|t| t.schema()).collect()
    }
}

impl ToolSource for SlotTools {
    fn get(&self, id: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(id).map(|t| Arc::clone(t) as Arc<dyn Tool>)
    }

    fn defs_for(&self, allowed: &[String]) -> Vec<ToolDef> {
        allowed
            .iter()
            .filter_map(|id| self.tools.get(id))
            .map(|t| t.schema())
            .collect()
    }

    fn ids(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::state::tests::{at, pool_with_session, schema};
    use crate::agents::state::{AgentState, Provenance};
    use aiplane_agents::db::agent_state;

    fn tools() -> SlotTools {
        SlotTools::with_clock(Arc::new(schema()), Arc::new(|| at("2026-10-02T12:00:00Z")))
    }

    fn ctx(pool: &aiplane_core::server::db::Pool) -> ToolContext {
        ToolContext {
            session_id: Some("s1".into()),
            ..ToolContext::for_test(pool.clone())
        }
    }

    async fn call(
        pool: &aiplane_core::server::db::Pool,
        tool: &str,
        args: Value,
    ) -> Result<Value, ToolError> {
        let Some(t) = tools().get(tool) else {
            return Err(ToolError::InvalidArgs(format!("no tool {tool}")));
        };
        t.run(ctx(pool), args).await
    }

    #[test]
    fn a_tool_exists_only_for_slots_the_model_may_write() {
        let t = tools();
        assert_eq!(
            t.ids(),
            [
                "set_email",
                "set_issue",
                "set_name",
                "set_order",
                "set_score",
                "set_seats",
                "set_urgent"
            ]
        );
        for trusted in ["set_verified", "set_plan"] {
            // codeql[rust/cleartext-logging] — `trusted` is a tool id, not a secret.
            assert!(!t.contains(trusted), "{trusted}");
        }
        assert_eq!(
            t.defs_for(&[
                "set_name".into(),
                "set_verified".into(),
                "rag_search".into()
            ])
            .len(),
            1
        );
    }

    #[test]
    fn a_definition_takes_exactly_a_value_of_the_slots_shape() {
        let def = tools().get("set_issue").unwrap().schema();
        assert_eq!(def.function.name, "set_issue");
        assert_eq!(
            def.function.parameters,
            json!({
                "type": "object",
                "properties": { "value": { "enum": ["billing", "technical"] } },
                "required": ["value"],
                "additionalProperties": false
            })
        );
        let name = tools().get("set_name").unwrap().schema();
        assert!(
            name.function.description.contains("the visitor's name"),
            "{}",
            name.function.description
        );
    }

    #[tokio::test]
    async fn a_valid_call_stores_the_value_as_written_by_the_model() {
        let pool = pool_with_session("s1").await;
        let out = call(&pool, "set_issue", json!({ "value": "billing" }))
            .await
            .unwrap();
        assert_eq!(out["status"], "set");

        let state = AgentState::load(&pool, &schema(), "s1").await.unwrap();
        let entry = state.valid("issue").unwrap();
        assert_eq!(entry.value, json!("billing"));
        assert_eq!(entry.provenance, Provenance::Llm);
        assert_eq!(entry.set_at, at("2026-10-02T12:00:00Z"));
    }

    #[tokio::test]
    async fn an_invalid_value_is_refused_with_the_validators_message() {
        let pool = pool_with_session("s1").await;
        let err = call(&pool, "set_email", json!({ "value": "not-an-address" }))
            .await
            .unwrap_err();
        let ToolError::InvalidArgs(msg) = &err else {
            panic!("{err:?}");
        };
        assert!(msg.contains("not an email address"), "{msg}");
        assert!(msg.contains("call set_email again"), "{msg}");
        assert!(
            agent_state::for_session(&pool, "s1")
                .await
                .unwrap()
                .is_empty()
        );
    }

    /// Whatever the model sends, a verifier-only slot stays unwritten: there is
    /// no tool for it, and no argument of another tool reaches it.
    #[tokio::test]
    async fn the_model_cannot_write_a_verifier_only_slot_whatever_it_sends() {
        let pool = pool_with_session("s1").await;
        let attempts: &[(&str, Value)] = &[
            ("set_verified", json!({ "value": { "customer_id": "K-1" } })),
            ("set_plan", json!({ "value": "gold" })),
            ("set_name", json!({ "value": "Ada", "slot": "verified" })),
            (
                "set_name",
                json!({ "value": "Ada", "provenance": "verifier:otp" }),
            ),
            ("set_name", json!({ "value": "Ada", "provenance": "host" })),
            (
                "set_name",
                json!({ "slot": "verified", "value": { "customer_id": "K-1" } }),
            ),
            ("set_name", json!({ "session_id": "s2", "value": "Ada" })),
            ("set_name", json!("Ada")),
            ("set_name", json!({})),
        ];
        for (tool, args) in attempts {
            assert!(
                call(&pool, tool, args.clone()).await.is_err(),
                "{tool}({args}) was accepted"
            );
        }
        assert!(
            agent_state::for_session(&pool, "s1")
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn an_extra_argument_is_named_in_the_refusal() {
        let pool = pool_with_session("s1").await;
        let err = call(
            &pool,
            "set_name",
            json!({ "value": "Ada", "provenance": "host" }),
        )
        .await
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("`provenance`"), "{msg}");
        assert!(msg.contains("only `value`"), "{msg}");
    }

    #[tokio::test]
    async fn outside_an_agent_conversation_the_tool_refuses() {
        let pool = pool_with_session("s1").await;
        let tool = tools().get("set_name").unwrap();
        let err = tool
            .run(
                ToolContext::for_test(pool.clone()),
                json!({ "value": "Ada" }),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("agent conversation"), "{err}");
    }
}
