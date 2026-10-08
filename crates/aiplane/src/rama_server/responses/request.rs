// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! OpenAI `/v1/responses` request → OpenAI `/v1/chat/completions` request.
//!
//! | Responses | Chat Completions | Note |
//! |---|---|---|
//! | `instructions` | a leading `role:"system"` message | |
//! | `system` / `developer` message items | folded into that same leading system message | a chat template rejects a system message anywhere but first |
//! | `message` items (`input_text`, `input_image`, `output_text`) | `user` / `assistant` messages | |
//! | `function_call` / `custom_tool_call` items | `tool_calls[]` on the assistant message they follow | |
//! | `function_call_output` / `custom_tool_call_output` items | one `role:"tool"` message each | images in an output follow as a user message |
//! | `reasoning` items | *(dropped)* | they were synthesised from the backend's own reasoning |
//! | flat `function` tools | nested `function` tools | |
//! | `custom` (freeform) tools | a function taking one string `input` | the grammar, if any, rides in the description |
//! | hosted tools (`web_search`, `file_search`, `code_interpreter`, …) | *(dropped)* | they only run on OpenAI's infrastructure |
//! | `max_output_tokens` | `max_tokens` | |
//! | `text.format` | `response_format` | |
//! | `reasoning.effort` | the serving model's reasoning parameter | via `aiplane_core::server::reasoning`, never forwarded verbatim |
//!
//! **Unknown fields are dropped, not rejected**, for the same reason as on
//! `/v1/messages`: clients grow request fields with every release. What *is*
//! rejected is a field whose meaning the gateway cannot honour without lying:
//! `background`, `conversation`, a stored `prompt` template, and references to
//! uploaded files or items — each would otherwise be silently ignored and the
//! model answer from a context the caller never meant to send.

use std::collections::BTreeSet;

use serde_json::{Map, Value, json};

use aiplane_core::server::reasoning::Effort;

/// A request we could not translate. Surfaces as `400 invalid_request_error`,
/// naming the offending parameter where there is one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslateError {
    pub message: String,
    pub param: Option<&'static str>,
}

impl TranslateError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            param: None,
        }
    }

    fn param(param: &'static str, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            param: Some(param),
        }
    }
}

/// A parsed `/v1/responses` request: everything the handler needs before it
/// can load stored context and route.
#[derive(Debug, Clone)]
pub struct ResponsesRequest {
    /// The model id exactly as the client asked for it (may be an alias).
    pub model: String,
    pub stream: bool,
    /// Whether the response is kept for `previous_response_id` and
    /// `GET /v1/responses/{id}`. OpenAI's default is `true`.
    pub store: bool,
    pub previous_response_id: Option<String>,
    /// This request's own input, normalised to item form: a string `input`
    /// becomes one user `message` item, and string message content becomes an
    /// `input_text` / `output_text` part. This is what is stored and what
    /// `GET /v1/responses/{id}/input_items` lists.
    pub input_items: Vec<Value>,
    pub effort: Option<Effort>,
    /// Names of the request's `custom` tools, whose calls go back to the
    /// client as `custom_tool_call` items rather than `function_call` ones.
    pub custom_tools: BTreeSet<String>,
    /// The request fields a response object echoes back.
    pub echo: Map<String, Value>,
    request: Map<String, Value>,
}

impl ResponsesRequest {
    /// Takes the request by value: its `input` is moved into
    /// [`Self::input_items`] rather than copied, since it can be tens of
    /// megabytes of inline images.
    pub fn parse(req: Value) -> Result<Self, TranslateError> {
        let Value::Object(mut obj) = req else {
            return Err(TranslateError::new("request body must be a JSON object"));
        };
        let model = obj
            .get("model")
            .and_then(Value::as_str)
            .filter(|m| !m.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| {
                TranslateError::param("model", "request body is missing a string `model` field")
            })?;
        if obj.get("background").and_then(Value::as_bool) == Some(true) {
            return Err(TranslateError::param(
                "background",
                "background responses are not supported by this gateway; send the request \
                 without `background` and keep the connection open (or stream it)",
            ));
        }
        if obj.get("conversation").is_some_and(|c| !c.is_null()) {
            return Err(TranslateError::param(
                "conversation",
                "the Conversations API is not supported by this gateway; chain turns with \
                 `previous_response_id` or send the whole conversation as `input`",
            ));
        }
        if obj.get("prompt").is_some_and(|p| !p.is_null()) {
            return Err(TranslateError::param(
                "prompt",
                "stored prompt templates are not supported by this gateway; send the prompt as \
                 `instructions` and `input`",
            ));
        }
        let previous_response_id = match obj.get("previous_response_id") {
            None | Some(Value::Null) => None,
            Some(Value::String(id)) if !id.is_empty() => Some(id.clone()),
            Some(_) => {
                return Err(TranslateError::param(
                    "previous_response_id",
                    "`previous_response_id` must be a non-empty string",
                ));
            }
        };
        let input_items = normalize_input(obj.remove("input").as_ref())?;
        let custom_tools = obj
            .get("tools")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|t| t.get("type").and_then(Value::as_str) == Some("custom"))
            .filter_map(|t| t.get("name").and_then(Value::as_str).map(str::to_owned))
            .collect();

        Ok(Self {
            stream: obj.get("stream").and_then(Value::as_bool).unwrap_or(false),
            store: obj.get("store").and_then(Value::as_bool).unwrap_or(true),
            effort: effort_for(&obj),
            echo: echo_fields(&obj, previous_response_id.as_deref()),
            model,
            previous_response_id,
            input_items,
            custom_tools,
            request: obj,
        })
    }

    /// The chat-completions body for this request, with `history` — the
    /// items of every stored response this one continues, oldest first —
    /// ahead of its own input. Only this request's `instructions` apply:
    /// a previous response's instructions are not carried over, as on
    /// OpenAI.
    pub fn to_chat(&self, history: &[Value]) -> Result<Value, TranslateError> {
        let obj = &self.request;
        let mut system_parts: Vec<String> = Vec::new();
        if let Some(instructions) = obj
            .get("instructions")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            system_parts.push(instructions.to_string());
        }
        let mut messages = Messages::default();
        for (i, item) in history.iter().enumerate() {
            messages.push_item(item, &mut system_parts).map_err(|e| {
                TranslateError::param("previous_response_id", format!("stored item {i}: {e}"))
            })?;
        }
        for (i, item) in self.input_items.iter().enumerate() {
            messages
                .push_item(item, &mut system_parts)
                .map_err(|e| TranslateError::param("input", format!("input[{i}]: {e}")))?;
        }
        let mut messages = messages.finish();
        if !system_parts.is_empty() {
            messages.insert(
                0,
                json!({"role": "system", "content": system_parts.join("\n\n")}),
            );
        }

        let mut out = Map::new();
        out.insert("model".into(), Value::String(self.model.clone()));
        out.insert("messages".into(), Value::Array(messages));
        out.insert("stream".into(), Value::Bool(self.stream));
        if let Some(max) = obj.get("max_output_tokens").and_then(Value::as_i64)
            && max > 0
        {
            out.insert("max_tokens".into(), json!(max));
        }
        for key in ["temperature", "top_p"] {
            if let Some(v) = obj.get(key).filter(|v| v.is_number()) {
                out.insert(key.into(), v.clone());
            }
        }
        if let Some(parallel) = obj.get("parallel_tool_calls").and_then(Value::as_bool) {
            out.insert("parallel_tool_calls".into(), Value::Bool(parallel));
        }
        if let Some(user) = ["safety_identifier", "user"]
            .iter()
            .find_map(|k| obj.get(*k).and_then(Value::as_str))
            .filter(|s| !s.is_empty())
        {
            out.insert("user".into(), Value::String(user.to_string()));
        }
        if let Some(format) = response_format(obj.get("text")) {
            out.insert("response_format".into(), format);
        }
        let tools = translate_tools(obj.get("tools"))?;
        if !tools.is_empty() {
            out.insert("tools".into(), Value::Array(tools));
            if let Some(choice) = translate_tool_choice(obj.get("tool_choice")) {
                out.insert("tool_choice".into(), choice);
            }
        }
        Ok(Value::Object(out))
    }
}

/// `input` as a list of items. A string is one user message.
fn normalize_input(input: Option<&Value>) -> Result<Vec<Value>, TranslateError> {
    match input {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::String(text)) => Ok(vec![json!({
            "id": super::output::new_id("msg"),
            "type": "message",
            "role": "user",
            "content": [{"type": "input_text", "text": text}],
        })]),
        Some(Value::Array(items)) => items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                normalize_item(item)
                    .map_err(|e| TranslateError::param("input", format!("input[{i}]: {e}")))
            })
            .collect(),
        Some(_) => Err(TranslateError::param(
            "input",
            "`input` must be a string or an array of items",
        )),
    }
}

/// One input item with its implicit parts made explicit: a message without
/// `type` is a `message`, string content becomes a one-part array, and an
/// item without an `id` gets one, so a stored item can be listed and paged.
fn normalize_item(item: &Value) -> Result<Value, String> {
    let Some(obj) = item.as_object() else {
        return Err("an input item must be an object".into());
    };
    let kind = obj.get("type").and_then(Value::as_str);
    if kind == Some("item_reference") {
        return Err(
            "`item_reference` is not supported by this gateway; send the item itself, or \
             continue a stored response with `previous_response_id`"
                .into(),
        );
    }
    let mut out = obj.clone();
    if !out.get("id").is_some_and(Value::is_string) {
        let prefix = if kind.is_none_or(|k| k == "message") {
            "msg"
        } else {
            "item"
        };
        out.insert("id".into(), json!(super::output::new_id(prefix)));
    }
    if kind.is_some() && kind != Some("message") {
        return Ok(Value::Object(out));
    }
    let role = obj
        .get("role")
        .and_then(Value::as_str)
        .ok_or("a message item is missing `role`")?;
    out.insert("type".into(), json!("message"));
    if let Some(Value::String(text)) = obj.get("content") {
        let part = if role == "assistant" {
            "output_text"
        } else {
            "input_text"
        };
        out.insert("content".into(), json!([{"type": part, "text": text}]));
    }
    if let Some(parts) = out.get("content").and_then(Value::as_array) {
        for part in parts {
            if part.get("type").and_then(Value::as_str) == Some("input_image")
                && part.get("image_url").and_then(Value::as_str).is_none()
            {
                return Err(
                    "an `input_image` by `file_id` is not supported: this gateway has no Files \
                     API; send the image as `image_url` (a URL or a data URI)"
                        .into(),
                );
            }
        }
    }
    Ok(Value::Object(out))
}

/// Builds the chat `messages` array from items, merging what the chat schema
/// keeps together: a tool call joins the assistant message it follows, and
/// images from tool outputs wait until the run of tool messages is over.
#[derive(Default)]
struct Messages {
    out: Vec<Value>,
    pending_images: Vec<Value>,
}

impl Messages {
    fn push_item(&mut self, item: &Value, system_parts: &mut Vec<String>) -> Result<(), String> {
        let kind = item
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("message");
        match kind {
            "function_call_output" | "custom_tool_call_output" => {
                let call_id = item
                    .get("call_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let (text, images) = output_content(item.get("output"));
                self.out
                    .push(json!({"role": "tool", "tool_call_id": call_id, "content": text}));
                self.pending_images.extend(images);
                return Ok(());
            }
            "reasoning" => return Ok(()),
            _ => {}
        }
        self.flush_images();
        match kind {
            "message" => self.push_message(item, system_parts),
            "function_call" => {
                let arguments = item.get("arguments").and_then(Value::as_str).unwrap_or("");
                self.push_tool_call(
                    item,
                    aiplane_core::server::tool_args::normalize_tool_arguments(arguments),
                );
                Ok(())
            }
            "custom_tool_call" => {
                let input = item.get("input").and_then(Value::as_str).unwrap_or("");
                self.push_tool_call(item, json!({"input": input}).to_string());
                Ok(())
            }
            // Hosted-tool calls (`web_search_call`, …) and anything newer:
            // nothing an OpenAI-compatible backend could be shown.
            _ => Ok(()),
        }
    }

    fn push_message(&mut self, item: &Value, system_parts: &mut Vec<String>) -> Result<(), String> {
        let role = item.get("role").and_then(Value::as_str).unwrap_or_default();
        let parts = item
            .get("content")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        match role {
            "system" | "developer" => {
                let text = joined_text(&parts);
                if !text.is_empty() {
                    system_parts.push(text);
                }
            }
            "assistant" => {
                self.out
                    .push(json!({"role": "assistant", "content": joined_text(&parts)}));
            }
            "user" => {
                let content = user_parts(&parts);
                if !content.is_empty() {
                    self.out
                        .push(json!({"role": "user", "content": collapse(content)}));
                }
            }
            other => return Err(format!("unsupported role `{other}`")),
        }
        Ok(())
    }

    fn push_tool_call(&mut self, item: &Value, arguments: String) {
        let call = json!({
            "id": item.get("call_id").and_then(Value::as_str).unwrap_or_default(),
            "type": "function",
            "function": {
                "name": item.get("name").and_then(Value::as_str).unwrap_or_default(),
                "arguments": arguments,
            },
        });
        if let Some(last) = self.out.last_mut()
            && last["role"] == "assistant"
        {
            if last["content"].as_str() == Some("") {
                last["content"] = Value::Null;
            }
            match last.get_mut("tool_calls").and_then(Value::as_array_mut) {
                Some(calls) => calls.push(call),
                None => last["tool_calls"] = json!([call]),
            }
            return;
        }
        // `content: null` alongside tool_calls: an empty string is rejected by
        // some backends when tool_calls are present.
        self.out
            .push(json!({"role": "assistant", "content": null, "tool_calls": [call]}));
    }

    fn flush_images(&mut self) {
        if !self.pending_images.is_empty() {
            let images = std::mem::take(&mut self.pending_images);
            self.out.push(json!({"role": "user", "content": images}));
        }
    }

    fn finish(mut self) -> Vec<Value> {
        self.flush_images();
        self.out
    }
}

/// The text of a message's parts: `input_text`, `output_text`, `text` and
/// `refusal`, joined by newlines.
fn joined_text(parts: &[Value]) -> String {
    parts
        .iter()
        .filter_map(|p| match p.get("type").and_then(Value::as_str) {
            Some("input_text" | "output_text" | "text") => p.get("text").and_then(Value::as_str),
            Some("refusal") => p.get("refusal").and_then(Value::as_str),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A user message's parts in chat form. A file the backend cannot be shown is
/// announced rather than dropped, so the model doesn't answer about a document
/// it never saw.
fn user_parts(parts: &[Value]) -> Vec<Value> {
    parts
        .iter()
        .filter_map(|p| match p.get("type").and_then(Value::as_str) {
            Some("input_text" | "output_text" | "text") => p
                .get("text")
                .and_then(Value::as_str)
                .map(|t| json!({"type": "text", "text": t})),
            Some("input_image") => image_part(p),
            Some("input_file") => Some(json!({"type": "text", "text": omitted_file(p)})),
            _ => None,
        })
        .collect()
}

fn image_part(part: &Value) -> Option<Value> {
    let url = part.get("image_url").and_then(Value::as_str)?;
    let mut image = json!({"url": url});
    if let Some(detail) = part.get("detail").and_then(Value::as_str) {
        image["detail"] = json!(detail);
    }
    Some(json!({"type": "image_url", "image_url": image}))
}

fn omitted_file(part: &Value) -> String {
    match part
        .get("filename")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        Some(name) => {
            format!("[file omitted: `{name}` — this backend accepts text and images only]")
        }
        None => "[file omitted — this backend accepts text and images only]".to_string(),
    }
}

/// A single text part collapses to a plain string: some backends' chat
/// templates only handle the array form for multimodal input.
fn collapse(parts: Vec<Value>) -> Value {
    if let [only] = parts.as_slice()
        && let Some(text) = only.get("text").and_then(Value::as_str)
    {
        return json!(text);
    }
    Value::Array(parts)
}

/// A tool output's text, plus any images it carried. `output` is a string or
/// an array of `input_text` / `input_image` parts.
fn output_content(output: Option<&Value>) -> (String, Vec<Value>) {
    match output {
        Some(Value::String(s)) => (s.clone(), Vec::new()),
        Some(Value::Array(parts)) => {
            let images = parts
                .iter()
                .filter(|p| p.get("type").and_then(Value::as_str) == Some("input_image"))
                .filter_map(image_part)
                .collect();
            let mut text = joined_text(parts);
            for part in parts {
                if part.get("type").and_then(Value::as_str) == Some("input_file") {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(&omitted_file(part));
                }
            }
            (text, images)
        }
        _ => (String::new(), Vec::new()),
    }
}

/// Responses tools → chat-completions function tools. Hosted tools are
/// skipped: they only run on OpenAI's infrastructure, so the model simply
/// never sees them.
fn translate_tools(tools: Option<&Value>) -> Result<Vec<Value>, TranslateError> {
    let Some(tools) = tools.filter(|t| !t.is_null()) else {
        return Ok(Vec::new());
    };
    let tools = tools
        .as_array()
        .ok_or_else(|| TranslateError::param("tools", "`tools` must be an array"))?;
    let mut out = Vec::with_capacity(tools.len());
    for tool in tools {
        let Some(name) = tool.get("name").and_then(Value::as_str) else {
            continue;
        };
        let description = tool.get("description").and_then(Value::as_str);
        let function = match tool.get("type").and_then(Value::as_str) {
            Some("function") => {
                let mut function = json!({
                    "name": name,
                    "parameters": tool
                        .get("parameters")
                        .filter(|p| p.is_object())
                        .cloned()
                        .unwrap_or_else(|| json!({"type": "object", "properties": {}})),
                });
                if let Some(description) = description {
                    function["description"] = json!(description);
                }
                function
            }
            Some("custom") => custom_tool_function(name, description, tool.get("format")),
            _ => continue,
        };
        out.push(json!({"type": "function", "function": function}));
    }
    Ok(out)
}

/// A freeform `custom` tool as a function taking its text as one string
/// argument, `input`. A grammar-constrained tool's grammar goes into the
/// description: a chat backend cannot enforce it, but the model can follow it.
fn custom_tool_function(name: &str, description: Option<&str>, format: Option<&Value>) -> Value {
    let mut text = description.unwrap_or_default().to_string();
    if let Some(format) = format
        && format.get("type").and_then(Value::as_str) == Some("grammar")
        && let Some(definition) = format.get("definition").and_then(Value::as_str)
    {
        let syntax = format
            .get("syntax")
            .and_then(Value::as_str)
            .unwrap_or("lark");
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(&format!(
            "The `input` must follow this {syntax} grammar:\n{definition}"
        ));
    }
    let mut function = json!({
        "name": name,
        "parameters": {
            "type": "object",
            "properties": {"input": {"type": "string"}},
            "required": ["input"],
        },
    });
    if !text.is_empty() {
        function["description"] = json!(text);
    }
    function
}

fn translate_tool_choice(choice: Option<&Value>) -> Option<Value> {
    match choice? {
        Value::String(mode) if matches!(mode.as_str(), "auto" | "none" | "required") => {
            Some(json!(mode))
        }
        Value::Object(obj) => match obj.get("type").and_then(Value::as_str)? {
            "function" | "custom" => {
                let name = obj.get("name").and_then(Value::as_str)?;
                Some(json!({"type": "function", "function": {"name": name}}))
            }
            "allowed_tools" => obj
                .get("mode")
                .and_then(Value::as_str)
                .filter(|m| matches!(*m, "auto" | "required"))
                .map(|m| json!(m)),
            _ => None,
        },
        _ => None,
    }
}

/// `text.format` → `response_format`. Plain text needs no field.
fn response_format(text: Option<&Value>) -> Option<Value> {
    let format = text?.get("format")?;
    match format.get("type").and_then(Value::as_str)? {
        "json_object" => Some(json!({"type": "json_object"})),
        "json_schema" => {
            let mut schema = Map::new();
            for key in ["name", "description", "schema", "strict"] {
                if let Some(v) = format.get(key) {
                    schema.insert(key.into(), v.clone());
                }
            }
            Some(json!({"type": "json_schema", "json_schema": schema}))
        }
        _ => None,
    }
}

/// `reasoning.effort` onto the gateway's effort level of the same name. A
/// request that says nothing about reasoning leaves the backend's reasoning
/// parameters alone, as `/v1/chat/completions` does.
fn effort_for(obj: &Map<String, Value>) -> Option<Effort> {
    let level = obj.get("reasoning")?.get("effort")?.as_str()?;
    Some(match level {
        "none" | "minimal" => Effort::Off,
        "max" => Effort::Xhigh,
        // "medium" and anything newer.
        other => Effort::parse(other).unwrap_or(Effort::Medium),
    })
}

/// The request fields a response object reports back. A field the client did
/// not send is reported as what the gateway actually did with it.
fn echo_fields(obj: &Map<String, Value>, previous_response_id: Option<&str>) -> Map<String, Value> {
    let get = |key: &str| obj.get(key).cloned().unwrap_or(Value::Null);
    let mut echo = Map::new();
    echo.insert("instructions".into(), get("instructions"));
    echo.insert("max_output_tokens".into(), get("max_output_tokens"));
    echo.insert(
        "metadata".into(),
        obj.get("metadata")
            .filter(|m| m.is_object())
            .cloned()
            .unwrap_or_else(|| json!({})),
    );
    echo.insert(
        "parallel_tool_calls".into(),
        json!(
            obj.get("parallel_tool_calls")
                .and_then(Value::as_bool)
                .unwrap_or(true)
        ),
    );
    echo.insert("previous_response_id".into(), json!(previous_response_id));
    echo.insert(
        "reasoning".into(),
        json!({
            "effort": obj.get("reasoning").and_then(|r| r.get("effort")).cloned().unwrap_or(Value::Null),
            "summary": Value::Null,
        }),
    );
    echo.insert(
        "store".into(),
        json!(obj.get("store").and_then(Value::as_bool).unwrap_or(true)),
    );
    echo.insert("temperature".into(), get("temperature"));
    echo.insert(
        "text".into(),
        json!({"format": obj
            .get("text")
            .and_then(|t| t.get("format"))
            .cloned()
            .unwrap_or_else(|| json!({"type": "text"}))}),
    );
    echo.insert(
        "tool_choice".into(),
        obj.get("tool_choice")
            .cloned()
            .unwrap_or_else(|| json!("auto")),
    );
    echo.insert(
        "tools".into(),
        obj.get("tools")
            .filter(|t| t.is_array())
            .cloned()
            .unwrap_or_else(|| json!([])),
    );
    echo.insert("top_p".into(), get("top_p"));
    echo.insert("truncation".into(), json!("disabled"));
    echo.insert("user".into(), get("user"));
    echo.insert("background".into(), json!(false));
    echo
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chat(req: Value) -> Value {
        ResponsesRequest::parse(req)
            .expect("parses")
            .to_chat(&[])
            .expect("translates")
    }

    fn rejected(req: Value) -> TranslateError {
        ResponsesRequest::parse(req).expect_err("is rejected")
    }

    #[test]
    fn a_string_input_becomes_one_user_message() {
        let out = chat(json!({"model": "m", "input": "hi"}));
        assert_eq!(out["model"], "m");
        assert_eq!(out["stream"], false);
        assert_eq!(out["messages"], json!([{"role": "user", "content": "hi"}]));
    }

    #[test]
    fn instructions_and_developer_messages_lead_as_one_system_message() {
        let out = chat(json!({
            "model": "m",
            "instructions": "be brief",
            "input": [
                {"role": "user", "content": "hi"},
                {"type": "message", "role": "developer", "content": [{"type": "input_text", "text": "sandbox: read-only"}]},
            ],
        }));
        assert_eq!(
            out["messages"],
            json!([
                {"role": "system", "content": "be brief\n\nsandbox: read-only"},
                {"role": "user", "content": "hi"},
            ])
        );
    }

    #[test]
    fn a_function_call_joins_the_assistant_message_it_follows() {
        let out = chat(json!({
            "model": "m",
            "input": [
                {"role": "user", "content": "list files"},
                {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Let me look."}]},
                {"type": "function_call", "call_id": "call_1", "name": "shell", "arguments": "{\"cmd\":\"ls\"}"},
                {"type": "function_call", "call_id": "call_2", "name": "shell", "arguments": ""},
                {"type": "function_call_output", "call_id": "call_1", "output": "a.txt"},
                {"type": "function_call_output", "call_id": "call_2", "output": [{"type": "input_text", "text": "b.txt"}]},
            ],
        }));
        assert_eq!(
            out["messages"],
            json!([
                {"role": "user", "content": "list files"},
                {"role": "assistant", "content": "Let me look.", "tool_calls": [
                    {"id": "call_1", "type": "function", "function": {"name": "shell", "arguments": "{\"cmd\":\"ls\"}"}},
                    {"id": "call_2", "type": "function", "function": {"name": "shell", "arguments": "{}"}},
                ]},
                {"role": "tool", "tool_call_id": "call_1", "content": "a.txt"},
                {"role": "tool", "tool_call_id": "call_2", "content": "b.txt"},
            ])
        );
    }

    #[test]
    fn a_tool_call_without_preceding_text_has_null_content() {
        let out = chat(json!({
            "model": "m",
            "input": [
                {"role": "user", "content": "go"},
                {"type": "reasoning", "id": "rs_1", "summary": [], "content": [{"type": "reasoning_text", "text": "hmm"}]},
                {"type": "function_call", "call_id": "c", "name": "f", "arguments": "{}"},
            ],
        }));
        assert_eq!(out["messages"][1]["content"], Value::Null);
        assert_eq!(out["messages"][1]["tool_calls"][0]["id"], "c");
        assert_eq!(
            out["messages"].as_array().unwrap().len(),
            2,
            "the reasoning item is dropped"
        );
    }

    #[test]
    fn images_in_a_tool_output_follow_the_tool_messages_as_a_user_message() {
        let out = chat(json!({
            "model": "m",
            "input": [
                {"type": "function_call", "call_id": "a", "name": "view_image", "arguments": "{}"},
                {"type": "function_call", "call_id": "b", "name": "f", "arguments": "{}"},
                {"type": "function_call_output", "call_id": "a", "output": [
                    {"type": "input_image", "image_url": "data:image/png;base64,AAAA"},
                ]},
                {"type": "function_call_output", "call_id": "b", "output": "ok"},
            ],
        }));
        let messages = out["messages"].as_array().unwrap();
        assert_eq!(messages[1]["role"], "tool");
        assert_eq!(messages[2]["role"], "tool");
        assert_eq!(
            messages[3],
            json!({"role": "user", "content": [
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}},
            ]})
        );
    }

    #[test]
    fn user_images_become_image_url_parts_and_files_are_announced() {
        let out = chat(json!({
            "model": "m",
            "input": [{"role": "user", "content": [
                {"type": "input_text", "text": "what is this?"},
                {"type": "input_image", "image_url": "https://x/y.png", "detail": "low"},
                {"type": "input_file", "filename": "report.pdf", "file_data": "…"},
            ]}],
        }));
        assert_eq!(
            out["messages"][0]["content"],
            json!([
                {"type": "text", "text": "what is this?"},
                {"type": "image_url", "image_url": {"url": "https://x/y.png", "detail": "low"}},
                {"type": "text", "text": "[file omitted: `report.pdf` — this backend accepts text and images only]"},
            ])
        );
    }

    #[test]
    fn custom_tools_become_single_string_functions_and_round_trip() {
        let req = ResponsesRequest::parse(json!({
            "model": "m",
            "tools": [{
                "type": "custom",
                "name": "apply_patch",
                "description": "Edit files.",
                "format": {"type": "grammar", "syntax": "lark", "definition": "start: \"*** Begin Patch\""},
            }],
            "input": [
                {"type": "custom_tool_call", "call_id": "p1", "name": "apply_patch", "input": "*** Begin Patch"},
                {"type": "custom_tool_call_output", "call_id": "p1", "output": "Done"},
            ],
        }))
        .unwrap();
        assert!(req.custom_tools.contains("apply_patch"));
        let out = req.to_chat(&[]).unwrap();
        let function = &out["tools"][0]["function"];
        assert_eq!(function["name"], "apply_patch");
        assert_eq!(function["parameters"]["required"], json!(["input"]));
        assert_eq!(
            function["description"],
            "Edit files.\n\nThe `input` must follow this lark grammar:\nstart: \"*** Begin Patch\""
        );
        assert_eq!(
            out["messages"][0]["tool_calls"][0]["function"]["arguments"],
            "{\"input\":\"*** Begin Patch\"}"
        );
        assert_eq!(
            out["messages"][1],
            json!({"role": "tool", "tool_call_id": "p1", "content": "Done"})
        );
    }

    #[test]
    fn function_tools_are_nested_and_hosted_tools_are_skipped() {
        let out = chat(json!({
            "model": "m",
            "input": "hi",
            "tools": [
                {"type": "function", "name": "shell", "description": "Run.", "parameters": {"type": "object", "properties": {"cmd": {"type": "string"}}}, "strict": false},
                {"type": "web_search"},
                {"type": "file_search", "vector_store_ids": ["vs_1"]},
            ],
            "tool_choice": {"type": "function", "name": "shell"},
        }));
        assert_eq!(
            out["tools"],
            json!([{"type": "function", "function": {
                "name": "shell",
                "description": "Run.",
                "parameters": {"type": "object", "properties": {"cmd": {"type": "string"}}},
            }}])
        );
        assert_eq!(
            out["tool_choice"],
            json!({"type": "function", "function": {"name": "shell"}})
        );
    }

    #[test]
    fn a_tool_choice_without_tools_is_not_sent() {
        let out = chat(
            json!({"model": "m", "input": "hi", "tools": [{"type": "web_search"}], "tool_choice": "auto"}),
        );
        assert!(out.get("tools").is_none());
        assert!(out.get("tool_choice").is_none());
    }

    #[test]
    fn sampling_limits_and_format_are_mapped() {
        let out = chat(json!({
            "model": "m",
            "input": "hi",
            "max_output_tokens": 300,
            "temperature": 0.2,
            "top_p": 0.9,
            "parallel_tool_calls": false,
            "safety_identifier": "u-1",
            "text": {"format": {"type": "json_schema", "name": "r", "schema": {"type": "object"}, "strict": true}},
            "include": ["reasoning.encrypted_content"],
            "prompt_cache_key": "k",
        }));
        assert_eq!(out["max_tokens"], 300);
        assert_eq!(out["temperature"], 0.2);
        assert_eq!(out["top_p"], 0.9);
        assert_eq!(out["parallel_tool_calls"], false);
        assert_eq!(out["user"], "u-1");
        assert_eq!(
            out["response_format"],
            json!({"type": "json_schema", "json_schema": {"name": "r", "schema": {"type": "object"}, "strict": true}})
        );
        assert!(out.get("include").is_none());
        assert!(out.get("prompt_cache_key").is_none());
    }

    #[test]
    fn reasoning_effort_maps_onto_the_gateway_levels() {
        let effort = |level: &str| {
            ResponsesRequest::parse(json!({"model": "m", "reasoning": {"effort": level}}))
                .unwrap()
                .effort
        };
        assert_eq!(effort("none"), Some(Effort::Off));
        assert_eq!(effort("minimal"), Some(Effort::Off));
        assert_eq!(effort("low"), Some(Effort::Low));
        assert_eq!(effort("medium"), Some(Effort::Medium));
        assert_eq!(effort("high"), Some(Effort::High));
        assert_eq!(effort("xhigh"), Some(Effort::Xhigh));
        assert_eq!(effort("ludicrous"), Some(Effort::Medium));
        assert_eq!(
            ResponsesRequest::parse(json!({"model": "m"}))
                .unwrap()
                .effort,
            None
        );
    }

    #[test]
    fn history_precedes_the_new_input_and_only_current_instructions_apply() {
        let req = ResponsesRequest::parse(json!({
            "model": "m",
            "instructions": "now",
            "previous_response_id": "resp_1",
            "input": "and then?",
        }))
        .unwrap();
        assert_eq!(req.previous_response_id.as_deref(), Some("resp_1"));
        let history = [
            json!({"type": "message", "role": "user", "content": [{"type": "input_text", "text": "first"}]}),
            json!({"type": "message", "role": "assistant", "id": "msg_1", "status": "completed",
                   "content": [{"type": "output_text", "text": "answer", "annotations": []}]}),
        ];
        let out = req.to_chat(&history).unwrap();
        assert_eq!(
            out["messages"],
            json!([
                {"role": "system", "content": "now"},
                {"role": "user", "content": "first"},
                {"role": "assistant", "content": "answer"},
                {"role": "user", "content": "and then?"},
            ])
        );
    }

    #[test]
    fn input_is_normalised_to_items_for_storage() {
        let req = ResponsesRequest::parse(json!({
            "model": "m",
            "input": [{"role": "assistant", "content": "earlier"}, {"role": "user", "content": "now"}],
        }))
        .unwrap();
        let mut items = req.input_items.clone();
        for item in &mut items {
            let id = item.as_object_mut().unwrap().remove("id").unwrap();
            assert!(
                id.as_str().unwrap().starts_with("msg_"),
                "every item gets an id"
            );
        }
        assert_eq!(
            items,
            vec![
                json!({"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "earlier"}]}),
                json!({"type": "message", "role": "user", "content": [{"type": "input_text", "text": "now"}]}),
            ]
        );
        assert!(req.store, "store defaults to true, as on OpenAI");
    }

    #[test]
    fn requests_the_gateway_cannot_honour_are_rejected_naming_the_parameter() {
        assert_eq!(rejected(json!({"input": "hi"})).param, Some("model"));
        assert_eq!(
            rejected(json!({"model": "m", "background": true})).param,
            Some("background")
        );
        assert_eq!(
            rejected(json!({"model": "m", "conversation": "conv_1"})).param,
            Some("conversation")
        );
        assert_eq!(
            rejected(json!({"model": "m", "prompt": {"id": "pmpt_1"}})).param,
            Some("prompt")
        );
        assert_eq!(
            rejected(json!({"model": "m", "previous_response_id": 7})).param,
            Some("previous_response_id")
        );
        let reference =
            rejected(json!({"model": "m", "input": [{"type": "item_reference", "id": "msg_1"}]}));
        assert_eq!(reference.param, Some("input"));
        assert!(reference.message.starts_with("input[0]: `item_reference`"));
        let file = rejected(
            json!({"model": "m", "input": [{"role": "user", "content": [{"type": "input_image", "file_id": "file_1"}]}]}),
        );
        assert!(file.message.contains("no Files API"));
        assert_eq!(
            rejected(json!({"model": "m", "input": 3})).param,
            Some("input")
        );
    }

    #[test]
    fn background_false_and_null_fields_are_accepted() {
        ResponsesRequest::parse(json!({
            "model": "m",
            "background": false,
            "conversation": null,
            "prompt": null,
            "previous_response_id": null,
            "input": "hi",
        }))
        .expect("explicit defaults are fine");
    }

    #[test]
    fn the_echo_reports_what_the_gateway_did() {
        let req =
            ResponsesRequest::parse(json!({"model": "m", "input": "hi", "store": false})).unwrap();
        assert_eq!(req.echo["store"], false);
        assert_eq!(req.echo["tool_choice"], "auto");
        assert_eq!(req.echo["tools"], json!([]));
        assert_eq!(req.echo["text"], json!({"format": {"type": "text"}}));
        assert_eq!(req.echo["parallel_tool_calls"], true);
        assert_eq!(req.echo["temperature"], Value::Null);
    }
}
