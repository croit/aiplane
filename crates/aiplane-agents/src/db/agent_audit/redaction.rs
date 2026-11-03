// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! What the activity log never stores (`docs/agent-activity-log.md`, "Secrets never enter it"), applied in one place: [`append`]
//! hands every event's detail to [`Redaction::apply`] before it is hashed
//! and written, so no writer can forget it.
//!
//! Two things are left out, the same wherever they appear:
//! - the arguments of a call to a tool that declares them sensitive
//!   (`sensitive_args`), replaced by [`redacted_arguments`] — in a
//!   `tool_result` or `tool_call`, and in every tool call an `llm_exchange`
//!   carries (the answer's, and the history's in each later request);
//! - the secure input a turn was resumed with, replaced by
//!   [`SECURE_INPUT_WITHHELD`] in every event of that turn.
//!
//! [`append`]: super::append

use std::collections::BTreeSet;

use serde_json::{Value, json};
use session_core::db::{Decision, SuspensionKind};

use super::AuditKind;

/// What stands in a tool's result wherever it repeated a secure input: the
/// value goes to the tool that asked, never to the model, the stored
/// transcript or a log.
pub const SECURE_INPUT_WITHHELD: &str = "[secure input withheld]";

/// What stands for the arguments of a tool that declares them sensitive,
/// wherever a call is recorded: the activity log and the MCP audit alike.
pub fn redacted_arguments() -> Value {
    json!({ "redacted": true })
}

/// What one event leaves out.
#[derive(Clone, Default, PartialEq)]
pub struct Redaction {
    /// The secure input the turn was resumed with.
    pub secret: Option<Value>,
    /// The run's tools that declare their arguments sensitive.
    pub sensitive_tools: BTreeSet<String>,
}

impl std::fmt::Debug for Redaction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Redaction")
            .field("secret", &self.secret.as_ref().map(|_| "<withheld>"))
            .field("sensitive_tools", &self.sensitive_tools)
            .finish()
    }
}

impl Redaction {
    /// The one rule for what a decision leaves out of everything but the
    /// tool that asked: the value of a secure input. An approval carries no
    /// value, and a human's answer is the run's own content (`run_resumed`
    /// records it), so neither is withheld.
    pub fn decided(kind: SuspensionKind, decision: &Decision) -> Self {
        Self {
            secret: match (kind, decision) {
                (SuspensionKind::SecureInput, Decision::Value { value }) => Some(value.clone()),
                _ => None,
            },
            sensitive_tools: BTreeSet::new(),
        }
    }

    /// Both redactions at once.
    pub fn and(mut self, other: &Redaction) -> Self {
        if self.secret.is_none() {
            self.secret = other.secret.clone();
        }
        self.sensitive_tools
            .extend(other.sensitive_tools.iter().cloned());
        self
    }

    /// `body` with the secure input withheld, for what goes elsewhere than
    /// the log (the model, the transcript).
    pub fn withhold(&self, body: Value) -> Value {
        match &self.secret {
            Some(secret) => withhold_secret(body, secret),
            None => body,
        }
    }

    /// Redact the `detail` of a `kind` event in place.
    pub fn apply(&self, kind: AuditKind, detail: &mut Value) {
        if !self.sensitive_tools.is_empty() {
            match kind {
                AuditKind::ToolResult | AuditKind::ToolCall => {
                    let sensitive = detail["tool"]
                        .as_str()
                        .is_some_and(|t| self.sensitive_tools.contains(t));
                    if sensitive && let Some(arguments) = detail.get_mut("arguments") {
                        *arguments = redacted_arguments();
                    }
                }
                AuditKind::LlmExchange => self.redact_calls(detail),
                _ => {}
            }
        }
        if self.secret.is_some() {
            *detail = self.withhold(std::mem::take(detail));
        }
    }

    /// Every OpenAI-shaped tool call in `v` — `{function: {name,
    /// arguments}}`, wherever it sits — to a sensitive tool, its arguments
    /// replaced.
    fn redact_calls(&self, v: &mut Value) {
        match v {
            Value::Object(map) => {
                if let Some(Value::Object(function)) = map.get_mut("function")
                    && function
                        .get("name")
                        .and_then(Value::as_str)
                        .is_some_and(|n| self.sensitive_tools.contains(n))
                    && function.contains_key("arguments")
                {
                    function.insert("arguments".into(), redacted_arguments());
                }
                map.values_mut().for_each(|v| self.redact_calls(v));
            }
            Value::Array(items) => items.iter_mut().for_each(|v| self.redact_calls(v)),
            _ => {}
        }
    }
}

/// `body` with every repetition of the secure input `value` replaced by
/// [`SECURE_INPUT_WITHHELD`], the value trimmed of surrounding whitespace as
/// the tool that asked reads it. A well-behaved tool never repeats the
/// value; this makes sure a careless one cannot hand it to the model, the
/// transcript or the log either.
pub fn withhold_secret(body: Value, value: &Value) -> Value {
    let secret = match value {
        Value::String(s) => s.trim().to_string(),
        other => other.to_string(),
    };
    if secret.is_empty() {
        return body;
    }
    fn walk(v: Value, secret: &str) -> Value {
        match v {
            Value::String(s) if s.contains(secret) => {
                Value::String(s.replace(secret, SECURE_INPUT_WITHHELD))
            }
            Value::Number(n) if n.to_string() == secret => {
                Value::String(SECURE_INPUT_WITHHELD.into())
            }
            Value::Array(items) => {
                Value::Array(items.into_iter().map(|i| walk(i, secret)).collect())
            }
            Value::Object(map) => Value::Object(
                map.into_iter()
                    .map(|(k, v)| (k.replace(secret, SECURE_INPUT_WITHHELD), walk(v, secret)))
                    .collect(),
            ),
            other => other,
        }
    }
    walk(body, &secret)
}

#[cfg(test)]
mod tests;
