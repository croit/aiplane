// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Wire-format types for the gateway's session- and bearer-authenticated APIs.
//!
//! Keeping these in `shared` (and using only stdlib + serde + jiff) means the
//! server, the CLI, and the WASM web UI all deserialise them identically.

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct Me {
    pub id: String,
    pub email: String,
    pub name: Option<String>,
    /// Raw OIDC claim values for this user (e.g. groups: ["engineering"]).
    pub roles: Vec<String>,
    /// Internal RBAC role IDs the user resolves to after `[rbac.mapping]`
    /// (and `default_role`) is applied.
    #[serde(default)]
    pub role_ids: Vec<String>,
    /// Tools granted to this user (union over their roles). UI/CLI uses this
    /// to render the "what can I do" surface.
    #[serde(default)]
    pub allowed_tools: Vec<ToolSummary>,
    /// Optional features the operator has switched on, named after their
    /// `/admin/settings` sections (`comfyui`, `rag`, `skills`, `usage`,
    /// `limits`, …). The web UI hides the nav entry for anything absent here
    /// and answers its URL with "not enabled" rather than an error page.
    #[serde(default)]
    pub features: Vec<String>,
    /// Whether the user may build agents (`docs/agents.md`). The web UI shows
    /// the Agents section only when this is true.
    #[serde(default)]
    pub can_manage_agents: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
pub struct TokenSummary {
    pub id: String,
    pub name: String,
    pub created_at: Timestamp,
    pub last_used_at: Option<Timestamp>,
    pub expires_at: Timestamp,
    pub revoked: bool,
    /// Master "tool use" switch — `false` (the default) means this token
    /// gets no gateway tools (pure passthrough).
    #[serde(default)]
    pub tools_enabled: bool,
    /// Explicit per-capability states. Missing built-ins use Auto;
    /// missing MCP connectors and skills use Off.
    #[serde(default)]
    pub tool_states: std::collections::BTreeMap<String, String>,
}

/// Set a token's tool configuration wholesale — the master switch plus
/// the full set of explicit capability states. Replaces any previous per-token
/// tool prefs. Backs `PUT /api/v0/tokens/{id}/tools`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct UpdateTokenToolsRequest {
    pub tools_enabled: bool,
    #[serde(default)]
    pub tool_states: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CreateTokenResponse {
    pub token: TokenSummary,
    /// Plaintext token. Shown to the user **exactly once.**
    pub plaintext: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RevokeResponse {
    /// Always `true`: a missing or foreign token is a 404, an already revoked
    /// one a 409.
    pub revoked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DeleteResponse {
    /// Always `true`: a missing or foreign token is a 404, a still active one
    /// a 409 (revoke it first).
    pub deleted: bool,
}

/// OpenAI tool definition — the JSON shape the model sees in the `tools`
/// array of a chat completion request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolDef {
    #[serde(rename = "type")]
    pub kind: String,
    pub function: ToolFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolFunction {
    pub name: String,
    pub description: String,
    /// JSON Schema for the function arguments.
    pub parameters: serde_json::Value,
}

impl ToolDef {
    pub fn function(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: serde_json::Value,
    ) -> Self {
        Self {
            kind: "function".into(),
            function: ToolFunction {
                name: name.into(),
                description: description.into(),
                parameters,
            },
        }
    }
}

/// What a UI / CLI sees about a single tool registered on the gateway.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct ToolSummary {
    pub id: String,
    pub name: String,
    pub description: String,
}

/// A single chat message in the request the WASM client sends to
/// `/api/v0/chat`. Matches the OpenAI wire shape — `role` ∈ {system, user,
/// assistant, tool} and `content` is plain text. The browser composer only
/// emits `user`; the gateway adds `assistant` on the way back.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// `POST /api/v0/chat` request body. The handler forwards a JSON shape
/// compatible with `/v1/chat/completions` — keeping the wire format
/// explicit makes the contract easier for the browser to reason about than
/// "send whatever OpenAI takes".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
}

/// The error envelope every gateway endpoint emits when something goes wrong
/// on the gateway side. Mirrors OpenAI's `{"error": {…}}` shape.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ErrorEnvelope {
    pub error: ErrorBody,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ErrorBody {
    /// What went wrong and what to do about it, for a person to read.
    pub message: String,
    /// The machine-readable reason, e.g. `invalid_request`, `not_found`.
    #[serde(rename = "type")]
    pub error_type: String,
    /// The same value as `type`.
    pub code: String,
    /// Fields a refusal carries beside the three above, such as a
    /// validator's `issues`.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}
