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
mod tests;
