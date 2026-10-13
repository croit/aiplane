// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Identity verifiers (`docs/agent-visitors.md` → "Identity verifiers").
//!
//! A verifier is the only thing besides the host that may write a slot the
//! model cannot: it writes through [`write_trusted`](super::state::write_trusted) as
//! `TrustedWriter::Verifier(<id>)`, after it decided in code that the visitor
//! proved something. Three kinds:
//!
//! - **`mcp_code`** ([`otp`]): the agent's own connector sends a one-time code
//!   to the address in the email slot (`send_code`) and checks it
//!   (`check_code`). The visitor types the code into a secure field (a
//!   `secure_input` suspension), so it never reaches the model, the
//!   transcript, a log line or an audit row. The gateway counts what the
//!   connector cannot be trusted to: attempts per code, expiry, and sends per
//!   address, client IP and conversation. Every answer about an address is the
//!   same whether it is registered or not.
//! - **`lookup`** ([`lookup`]): a granted tool confirms details the visitor
//!   gave (a name and a customer number). Weaker — it proves knowledge, not
//!   control — so the spec labels it with an `assurance`.
//! - **`host_jwt`** ([`host_jwt`]): the embedding website signs who the
//!   visitor is. Its slots are written as `host`, not as a verifier.
//!
//! [`Verifiers::from_spec`] reads the typed spec ([`AgentSpec::verifiers`]),
//! which applies each setting's default. A verifier missing what it needs to
//! run is left out: a draft may be incomplete, and a published version is
//! not.

pub mod host_jwt;
pub mod lookup;
pub mod otp;

use std::collections::BTreeMap;
use std::sync::Arc;

use aiplane_agents::db::agent_audit::AuditKind;
use aiplane_agents::rates::Rate;
use aiplane_core::server::crypto::sha256_hex;
use aiplane_core::server::principal::SystemPrincipal;
use jiff::SignedDuration;
use serde_json::{Map, Value, json};

use super::profile::RunOptions;
use super::spec::AgentSpec;
use super::spec::model::{LookupSpec, McpCodeSpec, Verifier};
use super::state::{StateSchema, StateWriteError, TrustedWriter, write_trusted_all};
use crate::rama_server::state::RamaState;
use crate::server::tools::{Tool, ToolContext, ToolError, extract_content_parts};
use aiplane_agents::db::WriteTx;

pub const SEND_TOOL_DEFAULT: &str = "send_code";
pub const CHECK_TOOL_DEFAULT: &str = "check_code";
pub const MAX_ATTEMPTS_DEFAULT: u32 = 5;
pub const MAX_ATTEMPTS_CAP: u32 = 10;
pub const CODE_TTL_DEFAULT: SignedDuration = SignedDuration::from_mins(10);
pub const MAX_CODE_TTL: SignedDuration = SignedDuration::from_hours(1);
pub const LIFETIME_DEFAULT: SignedDuration = SignedDuration::from_mins(10);
pub const MAX_LIFETIME_CAP: SignedDuration = SignedDuration::from_hours(24);

/// Codes sent to one address, across every conversation of the agent.
pub const EMAIL_SENDS_DEFAULT: Rate = Rate {
    max: 5,
    per: SignedDuration::from_hours(1),
};
/// Codes sent from one client IP, whatever the address.
pub const IP_SENDS_DEFAULT: Rate = Rate {
    max: 20,
    per: SignedDuration::from_hours(1),
};
/// Codes sent in one conversation.
pub const SESSION_SENDS_DEFAULT: Rate = Rate {
    max: 3,
    per: SignedDuration::from_mins(15),
};

/// Where a verifier takes the value of a slot it writes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteSource {
    /// The tool's whole answer, without its `valid` flag.
    Result,
    /// One field of the tool's answer.
    ResultField(String),
    /// A value the verifier sent the tool: `email` for `mcp_code`, a lookup's
    /// input argument. Never the code.
    Input(String),
}

impl WriteSource {
    pub fn parse(s: &str) -> Option<Self> {
        if s == "result" {
            return Some(Self::Result);
        }
        if let Some(field) = s.strip_prefix("result.").filter(|f| !f.is_empty()) {
            return Some(Self::ResultField(field.to_string()));
        }
        s.strip_prefix("input.")
            .filter(|a| !a.is_empty())
            .map(|a| Self::Input(a.to_string()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JwtAlgorithm {
    Hs256,
    Rs256,
    Es256,
}

impl JwtAlgorithm {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "HS256" => Some(Self::Hs256),
            "RS256" => Some(Self::Rs256),
            "ES256" => Some(Self::Es256),
            _ => None,
        }
    }
}

/// `{slot: source}` of a verifier, in slot order.
pub type Writes = Vec<(String, WriteSource)>;

/// The rate windows on sending codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SendLimits {
    pub email: Rate,
    pub ip: Rate,
    pub session: Rate,
}

/// An `mcp_code` verifier, ready to run.
#[derive(Debug, Clone)]
pub struct McpCode {
    pub id: String,
    pub connector: String,
    pub send_tool: String,
    pub check_tool: String,
    pub email_slot: String,
    pub writes: Writes,
    pub max_attempts: u32,
    pub code_ttl: SignedDuration,
    pub limits: SendLimits,
    pub assurance: Option<String>,
}

/// What a lookup sends the tool for one argument.
#[derive(Debug, Clone, PartialEq)]
pub enum InputSource {
    Slot(String),
    Const(Value),
}

/// A `lookup` verifier, ready to run.
#[derive(Debug, Clone)]
pub struct Lookup {
    pub id: String,
    pub tool: String,
    pub inputs: Vec<(String, InputSource)>,
    pub writes: Writes,
    pub max_attempts: u32,
    pub assurance: String,
}

/// Every runnable verifier of a spec, by kind.
#[derive(Debug, Clone, Default)]
pub struct Verifiers {
    pub codes: Vec<McpCode>,
    pub lookups: Vec<Lookup>,
}

impl Verifiers {
    pub fn from_spec(spec: &AgentSpec) -> Self {
        let mut out = Self::default();
        for (id, v) in &spec.verifiers {
            match v {
                Verifier::McpCode(cfg) => out.codes.extend(mcp_code(id, cfg)),
                Verifier::Lookup(cfg) => out.lookups.extend(lookup(id, cfg)),
                Verifier::HostJwt(_) => {}
            }
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.codes.is_empty() && self.lookups.is_empty()
    }
}

/// `writes` in slot order; `None` when it writes nothing.
fn writes(map: &BTreeMap<String, WriteSource>) -> Option<Writes> {
    let writes: Writes = map.iter().map(|(s, w)| (s.clone(), w.clone())).collect();
    (!writes.is_empty()).then_some(writes)
}

fn mcp_code(id: &str, cfg: &McpCodeSpec) -> Option<McpCode> {
    Some(McpCode {
        id: id.to_string(),
        connector: cfg.connector.clone()?,
        send_tool: cfg.send_tool(),
        check_tool: cfg.check_tool(),
        email_slot: cfg.email_slot.clone()?,
        writes: writes(&cfg.writes)?,
        max_attempts: cfg.max_attempts(),
        code_ttl: cfg.code_ttl(),
        limits: cfg.send_limits(),
        assurance: cfg.assurance.clone(),
    })
}

fn lookup(id: &str, cfg: &LookupSpec) -> Option<Lookup> {
    let inputs: Vec<(String, InputSource)> = cfg
        .inputs
        .iter()
        .map(|(arg, src)| (arg.clone(), src.clone()))
        .collect();
    Some(Lookup {
        id: id.to_string(),
        tool: cfg.tool.clone()?,
        inputs: (!inputs.is_empty()).then_some(inputs)?,
        writes: writes(&cfg.writes)?,
        max_attempts: cfg.max_attempts(),
        assurance: cfg.assurance.clone()?,
    })
}

/// Everything a verifier tool needs from the run that offers it.
#[derive(Clone)]
pub struct VerifierRun {
    pub state: Arc<RamaState>,
    pub principal: SystemPrincipal,
    pub schema: Arc<StateSchema>,
    pub options: RunOptions,
}

/// The synthetic tools of every runnable verifier: `verify_<id>_request_code`
/// and `verify_<id>_submit_code` per `mcp_code`, `verify_<id>` per `lookup`.
pub struct VerifierTool {
    pub tool: Arc<dyn Tool>,
    pub dependency_tool_ids: Vec<String>,
}

pub fn tools(verifiers: &Verifiers, run: &VerifierRun) -> Vec<VerifierTool> {
    let mut out = Vec::new();
    for code in &verifiers.codes {
        let code = Arc::new(code.clone());
        let send = crate::server::tools::mcp::tool_id(&code.connector, &code.send_tool);
        let check = crate::server::tools::mcp::tool_id(&code.connector, &code.check_tool);
        out.push(VerifierTool {
            tool: Arc::new(otp::RequestCode::new(code.clone(), run.clone())),
            dependency_tool_ids: vec![send, check.clone()],
        });
        out.push(VerifierTool {
            tool: Arc::new(otp::SubmitCode::new(code, run.clone())),
            dependency_tool_ids: vec![check],
        });
    }
    for lookup in &verifiers.lookups {
        out.push(VerifierTool {
            tool: Arc::new(lookup::LookupTool::new(
                Arc::new(lookup.clone()),
                run.clone(),
            )),
            dependency_tool_ids: vec![lookup.tool.clone()],
        });
    }
    out
}

/// SHA-256 of an address as the windows compare it: trimmed, lowercase.
pub fn email_hash(email: &str) -> String {
    sha256_hex(email.trim().to_lowercase().as_bytes())
}

/// A verifier tool takes nothing from the model: what it checks comes from
/// state, and a code comes from the visitor's secure field.
pub(crate) fn no_args(tool: &str, args: &Value, about: &str) -> Result<(), ToolError> {
    let extra: Vec<String> = match args {
        Value::Object(map) => map.keys().map(|k| format!("`{k}`")).collect(),
        Value::Null => Vec::new(),
        _ => vec!["a non-object".into()],
    };
    if extra.is_empty() {
        return Ok(());
    }
    Err(ToolError::InvalidArgs(format!(
        "{tool} takes no arguments; {} is not one. {about} Call it again with {{}}.",
        extra.join(", ")
    )))
}

/// Call `tool` of `connector` as the agent's principal. `sensitive` keeps
/// its arguments out of `mcp_tool_audit` and the journal.
pub(crate) async fn call_connector(
    run: &VerifierRun,
    ctx: &ToolContext,
    connector: &str,
    tool: &str,
    args: Value,
    sensitive: bool,
) -> Result<Value, ToolError> {
    let organization_unavailable = || {
        ToolError::Failed(
            super::profile::AgentRunError::OrganizationExecutionUnavailable(
                run.principal.id.clone(),
            )
            .to_string(),
        )
    };
    let unavailable = || {
        if ctx
            .agent
            .as_ref()
            .is_some_and(|agent| agent.organization().is_some())
        {
            organization_unavailable()
        } else {
            ToolError::Failed(format!(
                "connector `{connector}` offers no tool `{tool}` to this agent right now: it is not \
                 granted, not connected, or has no such tool. The agent's owner has to fix the \
                 verifier's connector"
            ))
        }
    };
    let layer = if let Some(agent) = &ctx.agent {
        if agent.system_principal().id != run.principal.id
            || !matches!(&ctx.principal, aiplane_core::server::principal::Principal::System(principal)
            if principal.id == agent.system_principal().id)
        {
            return Err(unavailable());
        }
        run.state.mcp_layer_for_run(agent).await?
    } else {
        if matches!(
            aiplane_agents::db::agents::dot_owner(&run.state.db, &run.principal.id)
                .await
                .map_err(|error| ToolError::Failed(error.to_string()))?,
            Some(aiplane_agents::db::agents::DotOwner::Organization(_))
        ) {
            return Err(organization_unavailable());
        }
        run.state.mcp.layer_for_principal(&run.principal).await
    };
    let id = crate::server::tools::mcp::tool_id(connector, tool);
    if layer.mode_of(&id) != Some(aiplane_core::server::db::user_mcp::ToolMode::Always) {
        return Err(unavailable());
    }
    let found = if sensitive {
        layer.get_with_sensitive_args(&id)
    } else {
        crate::server::tools::ToolSource::get(&layer, &id)
    };
    let Some(found) = found else {
        return Err(unavailable());
    };
    let mut nested_ctx = ctx.clone();
    if !ctx
        .agent
        .as_ref()
        .is_some_and(|agent| agent.organization().is_some())
        && nested_ctx
            .pending_execution_facts
            .as_ref()
            .is_some_and(|pending| pending.execution_facts.is_none())
    {
        nested_ctx.pending_execution_facts = None;
    }
    found.run(nested_ctx, args).await
}

pub(crate) async fn validate_verifier_authority_in(
    tx: &mut WriteTx,
    run: &VerifierRun,
    ctx: &ToolContext,
    dependency: &str,
) -> Result<(), ToolError> {
    let owner = aiplane_agents::db::agents::dot_owner_in(tx, &run.principal.id)
        .await
        .map_err(|error| ToolError::Failed(error.to_string()))?;
    let organization = matches!(
        owner,
        Some(aiplane_agents::db::agents::DotOwner::Organization(_))
    ) || ctx
        .agent
        .as_ref()
        .is_some_and(|agent| agent.organization().is_some());
    if !organization {
        return Ok(());
    }
    let unavailable = || {
        ToolError::Failed(
            super::profile::AgentRunError::OrganizationExecutionUnavailable(
                run.principal.id.clone(),
            )
            .to_string(),
        )
    };
    let agent = ctx.agent.as_ref().ok_or_else(unavailable)?;
    let context = agent.organization().ok_or_else(unavailable)?;
    if agent.system_principal().id != run.principal.id
        || !matches!(&ctx.principal, aiplane_core::server::principal::Principal::System(principal)
            if principal.id == agent.system_principal().id)
        || ctx.session_id.as_deref() != Some(context.session_id.as_str())
        || ctx.assistant_turn_id.as_deref() != Some(context.assistant_turn_id.as_str())
    {
        return Err(unavailable());
    }
    if let Some(rest) = dependency.strip_prefix(crate::server::tools::mcp::MCP_ID_PREFIX) {
        let (connector, _) = rest.split_once("__").ok_or_else(unavailable)?;
        let facts = ctx
            .pending_execution_facts
            .as_ref()
            .and_then(|facts| facts.execution_facts.as_ref())
            .ok_or_else(unavailable)?;
        let crate::server::tools::mcp::identities::McpDispatchFacts::Owned(identity) =
            serde_json::from_value::<crate::server::tools::mcp::identities::McpDispatchFacts>(
                facts.clone(),
            )
            .map_err(|_| unavailable())?
        else {
            return Err(unavailable());
        };
        if identity.connection.connector_key != connector {
            return Err(unavailable());
        }
        crate::server::tools::mcp::identities::validate_organization_identity_in(tx, ctx, &identity)
            .await
    } else {
        let principal = super::profile::validate_organization_turn_in(tx, agent)
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        if !principal
            .grants
            .has(aiplane_core::server::principal::GrantKind::Tool, dependency)
        {
            return Err(unavailable());
        }
        Ok(())
    }
}

/// A tool's answer as one JSON object: structured content, or a text answer
/// that is a JSON object.
pub(crate) fn answer_object(body: &Value) -> Option<Map<String, Value>> {
    if let Value::Object(map) = body
        && extract_content_parts(body).is_none()
    {
        return Some(map.clone());
    }
    let text = match extract_content_parts(body) {
        Some(parts) => parts
            .iter()
            .filter_map(|p| p["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        None => body.as_str()?.to_string(),
    };
    serde_json::from_str::<Value>(text.trim())
        .ok()?
        .as_object()
        .cloned()
}

/// Whether an answer confirms: `valid: true` and nothing else counts.
pub(crate) fn confirms(answer: Option<&Map<String, Value>>) -> bool {
    answer.and_then(|a| a.get("valid")) == Some(&Value::Bool(true))
}

/// Why a verifier could not store what it verified. The owner's
/// configuration is wrong; nothing the visitor or the model does fixes it.
fn write_failed(id: &str, slot: &str, why: &str) -> ToolError {
    ToolError::Failed(format!(
        "verifier `{id}` confirmed the visitor but could not set slot `{slot}`: {why}. The \
         agent's owner has to fix the verifier's `writes` or the connector's answer; tell the \
         visitor verification is unavailable right now"
    ))
}

/// Resolve every write first and store them only if all fit, so a gate never
/// sees half a verification. The slots are `ctx`'s conversation's, written
/// in its run.
// The admitted call context and dependency proof must remain separate from verifier answer data.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn apply_writes(
    run: &VerifierRun,
    ctx: &ToolContext,
    id: &str,
    dependency: &str,
    writes: &Writes,
    answer: &Map<String, Value>,
    inputs: &Map<String, Value>,
    writer: TrustedWriter,
) -> Result<Vec<String>, ToolError> {
    let session_id = session_of(ctx, id)?;
    let mut resolved = Vec::new();
    for (slot, source) in writes {
        let value = match source {
            WriteSource::Result => {
                let mut whole = answer.clone();
                whole.remove("valid");
                Some(Value::Object(whole))
            }
            WriteSource::ResultField(field) => answer.get(field).cloned(),
            WriteSource::Input(arg) => inputs.get(arg).cloned(),
        };
        let Some(value) = value.filter(|v| !v.is_null()) else {
            return Err(write_failed(id, slot, "the answer has no value for it"));
        };
        resolved.push((slot.clone(), value));
    }
    let now = (run.options.now)();
    let refused = |err: StateWriteError| match err {
        StateWriteError::Db { .. } => ToolError::Failed(err.to_string()),
        StateWriteError::UnknownSlot { ref slot, .. }
        | StateWriteError::NotWritable { ref slot, .. }
        | StateWriteError::Invalid { ref slot, .. } => write_failed(id, slot, &err.to_string()),
    };
    let mut tx = WriteTx::begin(&run.state.db)
        .await
        .map_err(|e| ToolError::Failed(format!("storing the verifier's slots: {e}")))?;
    validate_verifier_authority_in(&mut tx, run, ctx, dependency).await?;
    write_trusted_all(&mut tx, &run.schema, session_id, &resolved, writer, now)
        .await
        .map_err(refused)?;
    tx.commit()
        .await
        .map_err(|e| ToolError::Failed(format!("storing the verifier's slots: {e}")))?;
    Ok(resolved.into_iter().map(|(slot, _)| slot).collect())
}

/// One `verifier_outcome` row on the running principal, with its chain.
/// Best-effort, like every run event: the decision stands without it.
pub(crate) async fn audit(run: &VerifierRun, ctx: &ToolContext, detail: Value) {
    let mut detail = detail;
    if let Value::Object(map) = &mut detail {
        map.insert("session_id".into(), json!(ctx.session_id));
        map.insert("turn_id".into(), json!(ctx.assistant_turn_id));
    }
    crate::agents::audit::record(
        &run.state.db,
        AuditKind::VerifierOutcome,
        &run.principal.id,
        None,
        ctx.chain(),
        detail,
    )
    .await;
}

/// The conversation of a call, or the refusal to give the model.
pub(crate) fn session_of<'a>(ctx: &'a ToolContext, tool: &str) -> Result<&'a str, ToolError> {
    ctx.session_id.as_deref().ok_or_else(|| {
        ToolError::Failed(format!(
            "{tool} only works inside an agent conversation, and this call has none. Do not retry."
        ))
    })
}

#[cfg(test)]
mod tests;
