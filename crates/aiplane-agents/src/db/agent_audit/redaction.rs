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
mod tests {
    use super::*;

    fn sensitive(tools: &[&str]) -> Redaction {
        Redaction {
            secret: None,
            sensitive_tools: tools.iter().map(|t| t.to_string()).collect(),
        }
    }

    #[test]
    fn a_sensitive_tools_arguments_are_redacted_in_its_result_and_its_gate() {
        let r = sensitive(&["otp_submit"]);
        for kind in [AuditKind::ToolResult, AuditKind::ToolCall] {
            let mut detail = json!({ "tool": "otp_submit", "arguments": { "code": "4711" } });
            r.apply(kind, &mut detail);
            assert_eq!(detail["arguments"], redacted_arguments(), "{kind:?}");
            let mut other = json!({ "tool": "echo", "arguments": { "m": "hi" } });
            r.apply(kind, &mut other);
            assert_eq!(other["arguments"], json!({ "m": "hi" }));
        }
    }

    #[test]
    fn an_exchange_has_every_call_to_a_sensitive_tool_redacted_wherever_it_sits() {
        let call = |name: &str| {
            json!({ "id": "c", "type": "function",
                    "function": { "name": name, "arguments": "{\"code\":\"4711\"}" } })
        };
        let mut detail = json!({
            "request": { "messages": [
                { "role": "assistant", "tool_calls": [call("otp_submit"), call("echo")] }
            ] },
            "request_delta": { "messages": [
                { "role": "assistant", "tool_calls": [call("otp_submit")] }
            ] },
            "response": { "tool_calls": [call("otp_submit")] },
        });
        sensitive(&["otp_submit"]).apply(AuditKind::LlmExchange, &mut detail);
        assert_eq!(
            detail["request"]["messages"][0]["tool_calls"][0]["function"]["arguments"],
            redacted_arguments()
        );
        assert_eq!(
            detail["request"]["messages"][0]["tool_calls"][1]["function"]["arguments"],
            json!("{\"code\":\"4711\"}"),
            "another tool's call is kept"
        );
        assert_eq!(
            detail["request_delta"]["messages"][0]["tool_calls"][0]["function"]["arguments"],
            redacted_arguments()
        );
        assert_eq!(
            detail["response"]["tool_calls"][0]["function"]["arguments"],
            redacted_arguments()
        );
    }

    #[test]
    fn a_secure_input_is_withheld_from_any_kind_of_event() {
        let r = Redaction::decided(
            SuspensionKind::SecureInput,
            &Decision::Value {
                value: json!(" 481516 "),
            },
        );
        for kind in AuditKind::ALL {
            let mut detail = json!({ "said": "the code is 481516", "n": 481516 });
            r.apply(*kind, &mut detail);
            // codeql[rust/cleartext-logging] — `detail` is the redacted copy; the test asserts the secret is gone.
            assert!(!detail.to_string().contains("481516"), "{kind:?}: {detail}");
        }
        assert!(format!("{r:?}").contains("<withheld>"));
        assert!(!format!("{r:?}").contains("481516"));
    }

    #[test]
    fn only_a_secure_input_is_a_secret() {
        let value = json!("yes");
        for kind in [SuspensionKind::Approval, SuspensionKind::HumanAnswer] {
            let decision = Decision::Value {
                value: value.clone(),
            };
            assert_eq!(Redaction::decided(kind, &decision).secret, None);
        }
        assert_eq!(
            Redaction::decided(SuspensionKind::SecureInput, &Decision::AllowOnce).secret,
            None
        );
    }

    #[test]
    fn a_tool_that_repeats_a_secure_input_has_it_withheld() {
        use SECURE_INPUT_WITHHELD as W;
        let body = json!({"echo": "you typed 481516", "n": 481516, "ok": true});
        let withheld = json!({"echo": format!("you typed {W}"), "n": W, "ok": true});
        assert_eq!(withhold_secret(body.clone(), &json!("481516")), withheld);
        assert_eq!(
            withhold_secret(body.clone(), &json!(" 481516 ")),
            withheld,
            "typed with spaces around it, as the verifier trims it"
        );
        assert_eq!(
            withhold_secret(body.clone(), &json!("")),
            body,
            "an empty value matches nothing"
        );
    }
}
