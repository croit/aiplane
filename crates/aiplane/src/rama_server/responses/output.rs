// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The response object and its output items, built from chat-completion
//! results.
//!
//! A completion's `reasoning_content` becomes a `reasoning` item, its text a
//! `message` item, and each tool call a `function_call` item — or a
//! `custom_tool_call` item when it calls one of the request's freeform
//! `custom` tools, whose single `input` argument is unwrapped again.
//!
//! Reasoning is reported as `reasoning_text` content, the shape open-weight
//! servers use: it is the model's own reasoning, not a summary of it, so the
//! `summary` stays empty rather than claiming to be one.

use std::collections::BTreeSet;

use serde_json::{Map, Value, json};

use aiplane_core::server::tool_args::tool_arguments_object;

/// A fresh id with OpenAI's prefix for its kind (`resp`, `msg`, `fc`, …).
pub fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::new_v4().simple())
}

/// The parts of a response object fixed before the model has answered.
#[derive(Debug, Clone)]
pub struct Shell {
    pub id: String,
    pub created_at: i64,
    /// The model as the client asked for it; the resolved id rides the
    /// `X-Gateway-Resolved-Model` header, as on the other `/v1` endpoints.
    pub model: String,
    pub echo: Map<String, Value>,
}

impl Shell {
    pub fn new(model: &str, echo: Map<String, Value>) -> Self {
        Self {
            id: new_id("resp"),
            created_at: now_unix(),
            model: model.to_string(),
            echo,
        }
    }

    /// The whole response object.
    pub fn response(&self, status: &str, output: &[Value], end: &End) -> Value {
        let mut obj = Map::new();
        obj.insert("id".into(), json!(self.id));
        obj.insert("object".into(), json!("response"));
        obj.insert("created_at".into(), json!(self.created_at));
        obj.insert("status".into(), json!(status));
        obj.insert(
            "completed_at".into(),
            if status == "in_progress" {
                Value::Null
            } else {
                json!(now_unix())
            },
        );
        obj.insert("error".into(), end.error.clone().unwrap_or(Value::Null));
        obj.insert(
            "incomplete_details".into(),
            end.incomplete_reason
                .map(|reason| json!({"reason": reason}))
                .unwrap_or(Value::Null),
        );
        obj.insert("model".into(), json!(self.model));
        obj.insert("output".into(), Value::Array(output.to_vec()));
        obj.insert("output_text".into(), json!(output_text(output)));
        for (key, value) in &self.echo {
            obj.insert(key.clone(), value.clone());
        }
        obj.insert(
            "usage".into(),
            if status == "in_progress" {
                Value::Null
            } else {
                end.usage.object()
            },
        );
        Value::Object(obj)
    }
}

/// How a response ended: its usage, and why it stopped short if it did.
#[derive(Debug, Clone, Default)]
pub struct End {
    pub usage: Usage,
    pub incomplete_reason: Option<&'static str>,
    pub error: Option<Value>,
}

impl End {
    /// `completed`, or `incomplete` when the model was cut off.
    pub fn status(&self) -> &'static str {
        if self.error.is_some() {
            "failed"
        } else if self.incomplete_reason.is_some() {
            "incomplete"
        } else {
            "completed"
        }
    }
}

/// Why a chat `finish_reason` means the response is incomplete, if it does.
pub fn incomplete_reason(finish_reason: Option<&str>) -> Option<&'static str> {
    match finish_reason {
        Some("length") => Some("max_output_tokens"),
        Some("content_filter") => Some("content_filter"),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_tokens: i64,
    pub cached_tokens: i64,
}

impl Usage {
    /// A completion's `usage`, read with the same reader billing uses.
    pub fn from_completion(completion: &Value) -> Self {
        let counts = aiplane_core::server::db::usage::usage_from_value(completion);
        let detail = |pointer: &str| {
            completion
                .pointer(pointer)
                .and_then(Value::as_i64)
                .unwrap_or(0)
        };
        Self {
            input_tokens: counts.prompt.unwrap_or(0),
            output_tokens: counts.completion.unwrap_or(0),
            reasoning_tokens: detail("/usage/completion_tokens_details/reasoning_tokens"),
            cached_tokens: counts.cached.unwrap_or(0),
        }
    }

    pub fn object(&self) -> Value {
        json!({
            "input_tokens": self.input_tokens,
            "input_tokens_details": {"cached_tokens": self.cached_tokens},
            "output_tokens": self.output_tokens,
            "output_tokens_details": {"reasoning_tokens": self.reasoning_tokens},
            "total_tokens": self.input_tokens + self.output_tokens,
        })
    }
}

/// The convenience `output_text` the SDKs expose: every message item's text.
fn output_text(output: &[Value]) -> String {
    output
        .iter()
        .filter(|item| item["type"] == "message")
        .flat_map(|item| item["content"].as_array().into_iter().flatten())
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect()
}

pub fn reasoning_item(id: &str, text: &str, status: &str) -> Value {
    json!({
        "id": id,
        "type": "reasoning",
        "status": status,
        "summary": [],
        "content": [{"type": "reasoning_text", "text": text}],
    })
}

pub fn message_item(id: &str, text: &str, status: &str) -> Value {
    json!({
        "id": id,
        "type": "message",
        "status": status,
        "role": "assistant",
        "content": [output_text_part(text)],
    })
}

pub fn output_text_part(text: &str) -> Value {
    json!({"type": "output_text", "text": text, "annotations": [], "logprobs": []})
}

/// One tool call as an output item: `custom_tool_call` for a freeform tool,
/// `function_call` otherwise.
pub fn tool_call_item(
    call_id: &str,
    name: &str,
    arguments: &str,
    custom_tools: &BTreeSet<String>,
) -> Value {
    if custom_tools.contains(name) {
        return json!({
            "id": new_id("ctc"),
            "type": "custom_tool_call",
            "status": "completed",
            "call_id": call_id,
            "name": name,
            "input": custom_tool_input(arguments),
        });
    }
    json!({
        "id": new_id("fc"),
        "type": "function_call",
        "status": "completed",
        "call_id": call_id,
        "name": name,
        "arguments": aiplane_core::server::tool_args::normalize_tool_arguments(arguments),
    })
}

/// A freeform tool's text from the `{"input": …}` arguments the model was
/// asked for. A model that answered with something else gets its raw
/// arguments passed on rather than an empty input.
fn custom_tool_input(arguments: &str) -> String {
    match tool_arguments_object(arguments).get("input") {
        Some(Value::String(input)) => input.clone(),
        _ => arguments.to_string(),
    }
}

/// The output items of a buffered completion.
pub fn items_from_completion(completion: &Value, custom_tools: &BTreeSet<String>) -> Vec<Value> {
    let mut items = Vec::new();
    let Some(message) = completion.pointer("/choices/0/message") else {
        return items;
    };
    let reasoning = message
        .get("reasoning_content")
        .and_then(Value::as_str)
        .or_else(|| message.get("reasoning").and_then(Value::as_str))
        .filter(|s| !s.is_empty());
    if let Some(reasoning) = reasoning {
        items.push(reasoning_item(&new_id("rs"), reasoning, "completed"));
    }
    if let Some(text) = message
        .get("content")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        items.push(message_item(&new_id("msg"), text, "completed"));
    }
    for call in message
        .get("tool_calls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let field = |pointer: &str| {
            call.pointer(pointer)
                .and_then(Value::as_str)
                .unwrap_or_default()
        };
        items.push(tool_call_item(
            field("/id"),
            field("/function/name"),
            field("/function/arguments"),
            custom_tools,
        ));
    }
    items
}

/// The response object for a buffered completion.
pub fn from_completion(
    shell: &Shell,
    completion: &Value,
    custom_tools: &BTreeSet<String>,
) -> Value {
    let finish = completion
        .pointer("/choices/0/finish_reason")
        .and_then(Value::as_str);
    let end = End {
        usage: Usage::from_completion(completion),
        incomplete_reason: incomplete_reason(finish),
        error: None,
    };
    shell.response(
        end.status(),
        &items_from_completion(completion, custom_tools),
        &end,
    )
}

pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell() -> Shell {
        let mut echo = Map::new();
        echo.insert("store".into(), json!(true));
        Shell::new("alias-model", echo)
    }

    #[test]
    fn a_completion_becomes_reasoning_message_and_call_items() {
        let completion = json!({
            "choices": [{"finish_reason": "tool_calls", "message": {
                "role": "assistant",
                "reasoning_content": "thinking",
                "content": "Running it.",
                "tool_calls": [
                    {"id": "call_1", "type": "function", "function": {"name": "shell", "arguments": "{\"cmd\":\"ls\"}"}},
                    {"id": "call_2", "type": "function", "function": {"name": "apply_patch", "arguments": "{\"input\":\"*** Begin Patch\"}"}},
                ],
            }}],
            "usage": {"prompt_tokens": 10, "completion_tokens": 4,
                      "completion_tokens_details": {"reasoning_tokens": 2}},
        });
        let custom = BTreeSet::from(["apply_patch".to_string()]);
        let response = from_completion(&shell(), &completion, &custom);
        assert_eq!(response["object"], "response");
        assert_eq!(response["status"], "completed");
        assert_eq!(response["model"], "alias-model");
        assert_eq!(response["store"], true);
        assert!(response["id"].as_str().unwrap().starts_with("resp_"));
        let output = response["output"].as_array().unwrap();
        assert_eq!(output[0]["type"], "reasoning");
        assert_eq!(
            output[0]["content"][0],
            json!({"type": "reasoning_text", "text": "thinking"})
        );
        assert_eq!(output[1]["type"], "message");
        assert_eq!(output[1]["content"][0]["text"], "Running it.");
        assert_eq!(output[2]["type"], "function_call");
        assert_eq!(output[2]["call_id"], "call_1");
        assert_eq!(output[2]["arguments"], "{\"cmd\":\"ls\"}");
        assert_eq!(output[3]["type"], "custom_tool_call");
        assert_eq!(output[3]["input"], "*** Begin Patch");
        assert_eq!(response["output_text"], "Running it.");
        assert_eq!(
            response["usage"],
            json!({
                "input_tokens": 10,
                "input_tokens_details": {"cached_tokens": 0},
                "output_tokens": 4,
                "output_tokens_details": {"reasoning_tokens": 2},
                "total_tokens": 14,
            })
        );
    }

    #[test]
    fn a_length_stop_is_an_incomplete_response() {
        let completion =
            json!({"choices": [{"finish_reason": "length", "message": {"content": "cut"}}]});
        let response = from_completion(&shell(), &completion, &BTreeSet::new());
        assert_eq!(response["status"], "incomplete");
        assert_eq!(
            response["incomplete_details"],
            json!({"reason": "max_output_tokens"})
        );
    }

    #[test]
    fn a_custom_call_with_unexpected_arguments_keeps_them_raw() {
        let custom = BTreeSet::from(["apply_patch".to_string()]);
        let item = tool_call_item("c", "apply_patch", "*** Begin Patch", &custom);
        assert_eq!(item["input"], "*** Begin Patch");
    }

    #[test]
    fn an_in_progress_response_has_no_usage_or_completion_time() {
        let response = shell().response("in_progress", &[], &End::default());
        assert_eq!(response["usage"], Value::Null);
        assert_eq!(response["completed_at"], Value::Null);
        assert_eq!(response["output"], json!([]));
    }
}
