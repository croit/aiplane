// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Gateway-executed tools.
//!
//! Tools are async functions the gateway exposes to the LLM through the OpenAI
//! `tools` array on a chat completion. When the model emits `tool_calls`, the
//! gateway runs the corresponding tool, appends the result as a `role: "tool"`
//! message, and re-prompts the model. The loop runs until the model returns a
//! plain assistant message (or we hit the round bound — see `runner`).
//!
//! Adding a tool means writing code, registering it in `ToolRegistry`, and
//! granting it to one or more roles in `[rbac]`. We do **not** discover tools
//! at runtime.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde_json::Value;
use shared::api::ToolDef;
use thiserror::Error;

pub mod ask_first;
pub mod catalog;
pub mod check_code;
pub mod discovery;
pub mod echo;
pub mod feedback;
pub mod injection;
pub mod mcp;
pub mod memory;
pub mod registry;
pub mod runner;
pub mod sandbox;
pub mod time;

pub use registry::{GrantedToolSource, ToolPhase, ToolRegistry, ToolSource};

/// Carried into each `Tool::run` invocation. Lets a tool read the
/// caller's identity + roles and reach the gateway's datastore
/// without us threading specific scalars (timezone, locale,
/// preferences) through the trait signature each time we add one.
#[derive(Clone)]
pub struct ToolContext {
    pub personal_execution: Option<Arc<crate::agent_run::PersonalDotExecution>>,
    /// Who the tool acts for. A person on the chat page and on a `gwk_`
    /// bearer; a system principal on a `gws_` bearer. Scope rows by
    /// [`Principal::subject_id`]; a tool that acts *for a person* must go
    /// through [`Self::person`] and so refuses for a system principal.
    pub principal: aiplane_core::server::principal::Principal,
    /// Bearer-token id on `/v1`; absent for browser chat and internal turns.
    pub token_id: Option<String>,
    /// The caller's resolved upstream access — pool `allowed_groups` plus, on
    /// a bearer request, the calling API token's model allowlist.
    ///
    /// Tools that route to an upstream must go through this. `roles` above is
    /// not enough: it cannot express a token's allowlist at all, and every
    /// tool would have to re-resolve the groups itself. A tool that resolves a
    /// *caller-named* model without it is a hole straight through both
    /// controls — the model asks for an expensive image pool mid-completion
    /// and gets it, whatever the token was scoped to.
    ///
    /// Defaults to [`PoolAccess::all`] for internal callers with no user
    /// behind them (background workers, tests); request paths overwrite it.
    pub pool_access: aiplane_core::server::upstreams::PoolAccess,
    /// Handle to the gateway's SQLite pool. Tools that need anything
    /// from `users` / `sessions` / etc. query it here keyed by
    /// the subject id (e.g. the time tool reads `users.timezone`).
    pub db: aiplane_core::server::db::Pool,
    /// S3 config for chat attachments. Threaded through so the
    /// `fetch_attachment` tool can resolve opaque attachment ids
    /// (`<turn_id>/<filename>`) back to the live bucket. `None` when
    /// the deployment hasn't configured `[chat.s3]` — the tool errs
    /// with `NotConfigured` in that case.
    pub s3: Option<std::sync::Arc<aiplane_core::server::config::S3Config>>,
    /// Id of the assistant turn the gateway is currently driving.
    /// `Some` only on the chat-page path (where a persistent
    /// `chat_turns` row exists); `None` on the proxy paths, which
    /// have no chat session to attach to. Tools that produce
    /// in-conversation side effects (`upload_attachment`) refuse to
    /// run when this is `None`.
    pub assistant_turn_id: Option<String>,
    /// Id of the chat session the current turn belongs to. `Some` only on
    /// the chat-page path; `None` on the proxy paths (no session there).
    /// Required by `enable_tools` to write per-conversation rows; tools
    /// that don't need it ignore the field.
    pub session_id: Option<String>,
    /// The caller's source IP (the TCP peer, or the forwarded client when the peer is a trusted proxy),
    /// when the request carried one. Used by `get_user_location` to
    /// resolve a coarse location via [`geoip`]. `None` for callers we
    /// can't attribute an IP to.
    pub client_ip: Option<String>,
    /// Client-IP → location resolver, cloned from `AppState`. `None`
    /// when `[geoip]` isn't configured. Carried here (rather than reached
    /// via global state) so tools stay a pure function of their context.
    pub geoip: Option<aiplane_features::server::geoip::GeoIp>,
    /// Handles for an interactive mid-turn browser prompt. `Some` only on
    /// the chat-page path (a live SSE turn the user is watching); `None`
    /// on proxy / bearer paths. `get_user_location` uses it to ask the
    /// browser for a precise position and wait for the reply.
    pub chat_feedback: Option<ChatFeedback>,
    /// Per-turn reservation set of filenames already claimed by an
    /// in-flight upload (typst, `upload_attachment`). Tool calls in
    /// one round run concurrently via `join_all`, so two parallel
    /// callers would both read the same pre-upload `content` and
    /// pick the same filename — the second `put_object` then
    /// overwrites the first. Holding the set behind a mutex inside
    /// `ToolContext` lets every uploader take the lock, see the
    /// committed *and* in-flight names together, and pick a unique
    /// suffix before releasing. `Some` only on the chat-page path
    /// (the only path with an `assistant_turn_id` to attach to);
    /// `None` on proxy / bearer paths.
    pub attachment_reservations:
        Option<std::sync::Arc<tokio::sync::Mutex<std::collections::HashSet<String>>>>,
    /// The RAG indexer handle, cloned from `AppState`. Tools that hit
    /// the vector index (`rag_search`, `rag_list_collections`) reach it
    /// here. `None` on paths that never wired one up (notably the
    /// in-memory test scaffolding); the tools degrade to a clear
    /// `Failed("RAG not configured")` error in that case.
    pub indexer: Option<aiplane_features::server::rag::worker::Indexer>,
    /// Image-generation handle, built from `AppState`'s upstream registry +
    /// HTTP client + usage sink. `generate_image` reaches it here; `None` on
    /// paths (tests) that never wired one up, where the tool degrades to a
    /// clear `Failed("image generation not configured")`.
    pub image_gen: Option<aiplane_features::server::image_gen::ImageGenerator>,
    /// Document-OCR handle, cloned from the server state (one shared instance,
    /// so its concurrency gate actually bounds concurrent runs).
    /// `fetch_attachment` reaches it for `mode="auto"` / `mode="ocr"`. `None`
    /// on paths (tests) that never wired one up, where those modes report that
    /// OCR isn't configured instead of failing opaquely.
    pub ocr: Option<aiplane_features::server::ocr::OcrService>,
    /// The turn's sandbox-container lease, so successive `run_in_sandbox`
    /// calls reuse one container (persisting `/work` across a turn's rounds)
    /// instead of each running single-use. `Some` on the chat + proxy paths
    /// when `[sandbox]` is configured; `None` elsewhere (tests, sandbox-less
    /// deployments), where sandbox calls stay single-use. The driver releases
    /// it at turn end. See [`sandbox::SandboxLease`].
    pub sandbox_lease: Option<std::sync::Arc<sandbox::SandboxLease>>,
    /// A *second* container lease, held for `browse_page`'s long-lived
    /// browser. Separate from [`Self::sandbox_lease`] on purpose: the runner
    /// fixes a container's network posture at creation, so the code lease
    /// recreates its container whenever a call flips `network` — which would
    /// kill the browser every time the model ran an ordinary offline
    /// `run_in_sandbox` between two browse calls. `Some` only where the
    /// sandbox is configured *and* its runner reports egress (a browser with
    /// no network is pointless). Released at turn end like the other lease.
    pub browser_lease: Option<std::sync::Arc<sandbox::SandboxLease>>,
    /// The at-rest encryption key, cloned from `AppState`. Tools that read a
    /// sealed operator setting out of `app_settings` need it — currently
    /// `search_web`, for the Brave API key. `None` on paths that never wired
    /// one up (unit tests), where such a setting reads as "not configured"
    /// and the tool reports that rather than failing opaquely.
    pub crypto: Option<std::sync::Arc<aiplane_core::server::crypto::Crypto>>,
    /// Web Push handle plus this turn's one-notification budget, so
    /// `notify_user` can reach the user's subscribed browsers. `None` when
    /// push isn't configured (no VAPID key) — the tool then reports that
    /// rather than failing opaquely. See [`PushNotifier`].
    pub push: Option<PushNotifier>,
    /// The model this turn is running on, when the path knows it. Carried so
    /// a tool can act *as* the current model without reaching into global
    /// state: `schedule_action` gives the action it creates the same model the
    /// user is talking to, which is the only choice that doesn't surprise
    /// them. `None` on paths that don't resolve one (unit tests), where a
    /// tool needing it must ask for it explicitly or refuse.
    pub model: Option<String>,
    /// Whether this call may pause its turn for a decision from outside the
    /// model, or what was decided if it is running again after one. Persisted
    /// interactive and headless turns can pause. See [`crate::suspend`].
    pub suspend: crate::suspend::Suspend,
    pub pending_actions: Option<PendingActionFacts>,
    pub pending_execution_facts: Option<PendingExecutionFacts>,
    /// The tool ids the acting principal is granted, resolved once per chat
    /// turn by the driver. `enable_tools` refuses a key that none of them
    /// belongs to, so a conversation never records an ungranted tool as on.
    /// `None` where nothing resolved it (the `/v1` paths, tests) and reads as
    /// nothing granted.
    pub granted_tools: Option<std::sync::Arc<std::collections::HashSet<String>>>,
    /// The agent run this call is part of, `None` for a person's chat, a
    /// scheduled action or a `/v1` request. `principal` is then its
    /// principal; see [`Self::agent_active`].
    pub agent: Option<std::sync::Arc<crate::agent_run::AgentRun>>,
    /// Personal Dot identity carried beside the user's principal. It scopes
    /// Dot-owned data without changing the permissions of the user.
    pub personal_dot: Option<crate::agent_run::PersonalDotIdentity>,
    /// The id of the tool call this run answers. The runner sets it on the
    /// copy of the context it hands each call; `None` outside a call. A
    /// sub-agent dispatch records it as the call site of the run it starts.
    pub call_id: Option<String>,
}

/// Test-support constructor. Not `#[cfg(test)]`-gated because the tool
/// implementations that use it live in `aiplane-tools`, a *different* crate —
/// a `cfg(test)` build of this crate is a separate crate instance whose types
/// wouldn't unify with theirs. Unused in production builds (the linker drops
/// it); it is not part of the runtime API.
impl ToolContext {
    /// Minimal [`ToolContext`] for unit tests: the given DB pool, a dummy
    /// user (`"u"`, no roles), and every optional handle `None` (no S3,
    /// geoip, indexer, image-gen, sandbox, chat turn/session). Replaces the
    /// 13-field literal that used to be copied into ~25 tool test modules.
    /// Override individual fields with struct-update syntax:
    /// `ToolContext { session_id: Some(id), ..ToolContext::for_test(pool) }`.
    pub fn for_test(db: aiplane_core::server::db::Pool) -> Self {
        Self {
            principal: Self::test_user("u"),
            token_id: None,
            // Unrestricted: a test that cares about scoping sets it.
            pool_access: aiplane_core::server::upstreams::PoolAccess::all(),
            db,
            s3: None,
            assistant_turn_id: None,
            session_id: None,
            client_ip: None,
            geoip: None,
            chat_feedback: None,
            attachment_reservations: None,
            indexer: None,
            image_gen: None,
            ocr: None,
            sandbox_lease: None,
            browser_lease: None,
            crypto: None,
            push: None,
            model: None,
            suspend: crate::suspend::Suspend::Unavailable,
            pending_actions: None,
            pending_execution_facts: None,
            granted_tools: None,
            agent: None,
            personal_dot: None,
            personal_execution: None,
            call_id: None,
        }
    }

    /// A person with no group claims, for tests. Crate-private: the other
    /// crates' tests build their principal themselves.
    pub(crate) fn test_user(id: &str) -> aiplane_core::server::principal::Principal {
        aiplane_core::server::principal::Principal::User {
            id: id.into(),
            roles: vec![],
        }
    }
}

impl ToolContext {
    /// Whether this call runs inside an agent run. An agent run acts as the
    /// agent's system principal and nobody else: no person's memory,
    /// connectors or skills, and every call is audited with the chain.
    pub fn agent_active(&self) -> bool {
        self.agent.is_some()
    }

    /// The agent call chain this call runs in, inside an agent run.
    pub fn chain(&self) -> Option<&aiplane_core::server::run_chain::RunChain> {
        self.agent
            .as_deref()
            .map(|run| run.chain().as_ref())
            .or_else(|| {
                self.personal_execution
                    .as_ref()
                    .map(|run| run.chain().as_ref())
            })
    }

    /// The person this tool acts for, or the refusal to hand the model when
    /// the caller is a system principal. Memory, notifications, scheduled
    /// actions, location and browser control only make sense for a person —
    /// there is no one to remember, notify or locate behind a CI token.
    pub fn person(&self, tool: &str) -> Result<&str, ToolError> {
        match &self.principal {
            aiplane_core::server::principal::Principal::User { id, .. } => Ok(id),
            aiplane_core::server::principal::Principal::System(sp) => {
                Err(ToolError::Failed(format!(
                    "`{tool}` acts on behalf of a signed-in person, and this request runs as \
                     the system principal `{}`, which has no person behind it. Do not retry; \
                     answer without it.",
                    sp.name
                )))
            }
        }
    }
}

/// Web Push handle for `notify_user`, with the turn's notification budget
/// attached.
///
/// The budget is the reason this is a struct rather than a bare
/// `Arc<PushSender>`: a model in a tool loop that can push freely is spam on
/// someone's phone, and repeated deliveries are how a VAPID key earns a bad
/// reputation with the push services. One notification per turn is enforced
/// here — not in `server::limits`, which counts tokens and cost per user over
/// time and has no notion of a turn — because the latch has exactly the
/// lifetime of the [`ToolContext`] the driver builds per turn.
#[derive(Clone)]
pub struct PushNotifier {
    sender: std::sync::Arc<aiplane_features::server::push::PushSender>,
    /// Cleared for every fresh context, i.e. once per turn.
    spent: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl PushNotifier {
    pub fn new(sender: std::sync::Arc<aiplane_features::server::push::PushSender>) -> Self {
        Self {
            sender,
            spent: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Claim this turn's single notification. `true` for the first caller,
    /// `false` for every later one.
    ///
    /// `swap` rather than load-then-store: tool calls in one round run
    /// concurrently via `join_all`, so two `notify_user` calls could both
    /// observe "unspent" and both send.
    pub fn claim(&self) -> bool {
        !self.spent.swap(true, std::sync::atomic::Ordering::SeqCst)
    }

    pub fn sender(&self) -> &aiplane_features::server::push::PushSender {
        &self.sender
    }
}

/// Chat-only handles a tool needs to run an interactive prompt while a
/// turn is streaming: push UI onto the live SSE stream (`broadcast`,
/// via `TurnUpdate::Inject`) and await the browser's reply (one hub per
/// reply shape). Only populated on the chat-page path — proxy / bearer
/// callers have no browser and no live turn to prompt against.
#[derive(Clone)]
pub struct ChatFeedback {
    pub broadcast: tokio::sync::broadcast::Sender<session_core::workers::TurnUpdate>,
    /// Where `get_user_location` parks waiting for a position.
    pub hub: std::sync::Arc<feedback::FeedbackHub<feedback::BrowserFix>>,
    /// Where `ask_user` parks waiting for an answer to a question.
    pub ask_hub: std::sync::Arc<feedback::FeedbackHub<feedback::AskReply>>,
    /// Where `browser_control` parks waiting for the paired browser extension
    /// to run a batch of actions.
    pub browser_hub: std::sync::Arc<feedback::FeedbackHub<feedback::BrowserReply>>,
    /// Whether the browser is on a secure context (so `navigator.
    /// geolocation` is actually allowed). When false, the tool skips the
    /// futile precise-location prompt, warns inline, and falls back to
    /// GeoIP. Computed per-request from `X-Forwarded-Proto` / `Host` —
    /// see `geoip::transport_is_secure`.
    pub secure: bool,
}

impl ChatFeedback {
    /// A chat context wired to fresh, empty hubs, for tests that only care
    /// about one of them.
    ///
    /// Without it, every tool test that fakes a live turn has to name all three
    /// hubs — so adding a fourth callback edits test modules that have nothing
    /// to do with it. Callers override the hub they assert on:
    /// `ChatFeedback { ask_hub, ..ChatFeedback::for_test(broadcast) }`.
    ///
    /// Ungated, like [`ToolContext::for_test`]: the callers are tests in other
    /// crates, which `cfg(test)` does not reach.
    pub fn for_test(
        broadcast: tokio::sync::broadcast::Sender<session_core::workers::TurnUpdate>,
    ) -> Self {
        Self {
            broadcast,
            hub: std::sync::Arc::new(feedback::FeedbackHub::default()),
            ask_hub: std::sync::Arc::new(feedback::FeedbackHub::default()),
            browser_hub: std::sync::Arc::new(feedback::FeedbackHub::default()),
            secure: true,
        }
    }
}

impl std::fmt::Debug for ToolContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Pool isn't `Debug`. Print the principal and elide the rest
        // so test failures stay readable.
        f.debug_struct("ToolContext")
            .field("principal", &self.principal)
            .field("db", &"<Pool>")
            .field("s3", &self.s3.as_ref().map(|_| "<S3Config>"))
            .field("assistant_turn_id", &self.assistant_turn_id)
            .field("session_id", &self.session_id)
            .field("client_ip", &self.client_ip)
            .field("geoip", &self.geoip.as_ref().map(|_| "<GeoIp>"))
            .field(
                "chat_feedback",
                &self.chat_feedback.as_ref().map(|_| "<ChatFeedback>"),
            )
            .field(
                "attachment_reservations",
                &self
                    .attachment_reservations
                    .as_ref()
                    .map(|_| "<Reservations>"),
            )
            .field("indexer", &self.indexer.as_ref().map(|_| "<Indexer>"))
            .field(
                "image_gen",
                &self.image_gen.as_ref().map(|_| "<ImageGenerator>"),
            )
            .field("ocr", &self.ocr.as_ref().map(|_| "<OcrService>"))
            .field(
                "sandbox_lease",
                &self.sandbox_lease.as_ref().map(|_| "<SandboxLease>"),
            )
            // Never print the key material itself.
            .field("crypto", &self.crypto.as_ref().map(|_| "<Crypto>"))
            .finish()
    }
}

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("{message}", message = crate::agents::audit::LOG_UNAVAILABLE)]
    AuditUnavailable,
    #[error("tool arguments did not match the declared schema: {0}")]
    InvalidArgs(String),
    #[error("tool execution failed: {0}")]
    Failed(String),
}

pub type ToolResult = Result<Value, ToolError>;

/// A reference that didn't resolve is the model's mistake to fix — a wrong id,
/// a deleted document, an off-chat call — so it maps to `InvalidArgs` and the
/// `RefError` text (which names `list_attachments` / `list_documents` /
/// `undelete_document`) reaches the model verbatim. A genuine read failure is
/// ours, and stays `Failed`.
///
/// Lives here so every file-taking tool converts with `?` instead of writing
/// its own phrasing — the reason a wrong id used to produce a different
/// message depending on which tool you called.
impl From<aiplane_features::server::file_refs::RefError> for ToolError {
    fn from(err: aiplane_features::server::file_refs::RefError) -> Self {
        use aiplane_features::server::file_refs::RefError;
        match &err {
            RefError::Failed(msg) => ToolError::Failed(msg.clone()),
            RefError::NotFound(_) | RefError::Deleted(_) | RefError::NoSession => {
                ToolError::InvalidArgs(err.to_string())
            }
        }
    }
}

/// `BoxFuture` alias — keeps each tool's `run` signature legible while staying
/// object-safe. Hand-rolled so we don't pull in `async_trait` for one trait.
pub type ToolFuture<'a> = Pin<Box<dyn Future<Output = ToolResult> + Send + 'a>>;

/// Sentinel key that signals "the body of this tool result is not a
/// JSON object to stringify into `content`, but an array of OpenAI
/// content parts to splice in as-is". Used by `fetch_attachment` so
/// it can hand the model back an `image_url` part for binary image
/// attachments — OpenAI's Chat Completions API has supported tool
/// message `content` as an array of parts for a while now, and that's
/// the only path that actually gets image bytes back into the
/// conversation (tool results have no other channel for them).
///
/// Tools opt in by returning `Value::Object` of shape
/// `{ TOOL_CONTENT_PARTS_KEY: [ ...parts ] }`. The driver detects this
/// shape and emits `content: [..parts..]` instead of `content: "<json>"`.
pub const TOOL_CONTENT_PARTS_KEY: &str = "__gateway_tool_content_parts";

/// Helper for tools that want to return mixed text + image content
/// back to the model. The driver renders the parts as the tool
/// message's `content` array; the parts themselves are also what the
/// operator UI sees in the tool-call log (we persist the envelope
/// verbatim).
pub fn tool_content_parts(parts: Vec<Value>) -> Value {
    serde_json::json!({ TOOL_CONTENT_PARTS_KEY: parts })
}

/// Truncate `s` to at most `max_bytes`, snapping back to the nearest
/// UTF-8 char boundary so we never split a multibyte codepoint.
/// Returns the slice plus whether anything was dropped.
///
/// Shared by every tool that caps text it hands back to the model
/// (`fetch_attachment`'s PDF text tier, `fetch_url`'s HTML extraction),
/// so the byte-cap semantics can't drift between them.
pub fn truncate_on_char_boundary(s: &str, max_bytes: usize) -> (&str, bool) {
    if s.len() <= max_bytes {
        return (s, false);
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    (&s[..end], true)
}

/// Read the parts back out of a tool result body if the tool used
/// `tool_content_parts(...)`. Returns `None` for the JSON-stringified
/// path so the existing behavior stays unchanged.
pub fn extract_content_parts(body: &Value) -> Option<&Vec<Value>> {
    body.as_object()
        .filter(|m| m.len() == 1)
        .and_then(|m| m.get(TOOL_CONTENT_PARTS_KEY))
        .and_then(|v| v.as_array())
}

/// A registered, server-side tool.
///
/// Implementations are typically zero-sized or hold an `Arc<Service>`; cloning
/// them must be cheap because they're held in `Arc` by the registry.
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct ToolAction {
    pub id: &'static str,
    pub description: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingActionFacts {
    pub actions: Option<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PendingExecutionFacts {
    pub execution_facts: Option<Value>,
}

pub type ToolActionFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Option<Vec<String>>, ToolError>> + Send + 'a>>;

pub trait Tool: Send + Sync + 'static {
    fn execution_facts(&self) -> Option<Value> {
        None
    }

    fn phase(&self) -> ToolPhase {
        ToolPhase::Concurrent
    }

    /// Stable identifier. Used as the OpenAI function name and as the lookup
    /// key in `ToolRegistry`. Must be unique across all registered tools.
    /// Convention: `"company.<area>.<verb>"`, e.g. `"company_wiki_search"`.
    fn id(&self) -> &str;

    /// The OpenAI-shaped definition the model sees. Generated fresh each call
    /// so the schema can interpolate runtime context if needed (most don't).
    fn schema(&self) -> ToolDef;

    fn action_catalogue(&self) -> Option<&'static [ToolAction]> {
        None
    }

    fn classify_actions<'a>(&'a self, _ctx: ToolContext, _args: Value) -> ToolActionFuture<'a> {
        Box::pin(async { Ok(None) })
    }

    /// Executes the tool. Returns a JSON value that's stringified into the
    /// `role: "tool"` message we send back to the model in the next round.
    fn run<'a>(&'a self, ctx: ToolContext, args: Value) -> ToolFuture<'a>;

    /// Optional per-tool wall-clock ceiling the runner enforces around
    /// [`Tool::run`]. `None` (the default) uses the runner's standard
    /// per-tool timeout. Override it for tools that legitimately run long
    /// — e.g. the sandbox tools, whose jobs (document rendering, headless
    /// browser, cold-start) can exceed the default. Returning a value
    /// shorter than the default is fine too.
    fn max_duration(&self) -> Option<std::time::Duration> {
        None
    }

    /// Whether this tool's call arguments must be kept out of the journal.
    ///
    /// The runner logs every call's arguments at info level so operators can
    /// see what a hanging tool was asked to do. For almost every tool that is
    /// exactly right. For a few it is a disclosure: `browser_control` carries
    /// the URLs a user's own browser is being sent to and the text being typed
    /// into their forms — the very things its audit table goes out of its way
    /// *not* to store. A redaction that the process logger quietly undoes is
    /// not a redaction, so the tool declares it here and the runner logs a
    /// placeholder instead.
    fn sensitive_args(&self) -> bool {
        false
    }

    /// Whether the tool is known to change something outside the
    /// conversation. Only an MCP server says so today (a destructive tool
    /// that is not read-only); everything else is `false` because nothing
    /// is known about it. An agent asks for approval before such a call
    /// unless its spec sets the tool's `permission` (`docs/agent-hil.md` → "Per-tool approval").
    fn changes_state(&self) -> bool {
        false
    }

    /// Positive read-only metadata for autonomous personal Dot calls. Unknown
    /// tools require approval unless the owner's spec permits them explicitly.
    fn read_only(&self) -> bool {
        false
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;

    pub(crate) struct PhasedEcho;

    impl Tool for PhasedEcho {
        fn execution_facts(&self) -> Option<Value> {
            Some(json!({"resource":"fixture"}))
        }
        fn id(&self) -> &str {
            "company_echo"
        }
        fn schema(&self) -> shared::api::ToolDef {
            echo::Echo.schema()
        }
        fn phase(&self) -> ToolPhase {
            ToolPhase::ActsOnState
        }
        fn run<'a>(&'a self, ctx: ToolContext, args: Value) -> ToolFuture<'a> {
            echo::Echo.run(ctx, args)
        }
    }

    #[test]
    fn parts_envelope_round_trips_through_extract() {
        let body = tool_content_parts(vec![
            json!({"type": "text", "text": "hi"}),
            json!({"type": "image_url", "image_url": {"url": "https://e/x.png"}}),
        ]);
        let parts = extract_content_parts(&body).expect("parts envelope detected");
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0]["type"], "text");
        assert_eq!(parts[1]["type"], "image_url");
    }

    #[test]
    fn extract_returns_none_for_plain_json_body() {
        // A regular tool result must not be misinterpreted as parts —
        // the existing JSON-string path stays the default.
        let body = json!({"foo": "bar", "count": 3});
        assert!(extract_content_parts(&body).is_none());
    }

    #[test]
    fn extract_returns_none_when_sentinel_key_is_one_of_many() {
        // Only an envelope where the sentinel is the SOLE key counts;
        // protects against a tool accidentally namespace-colliding by
        // returning a struct that happens to include the same field.
        let mut obj = serde_json::Map::new();
        obj.insert(
            TOOL_CONTENT_PARTS_KEY.into(),
            json!([{"type": "text", "text": "x"}]),
        );
        obj.insert("other_field".into(), json!("hello"));
        let body = Value::Object(obj);
        assert!(extract_content_parts(&body).is_none());
    }

    #[tokio::test]
    async fn person_only_tools_refuse_a_system_principal_by_name() {
        use aiplane_core::server::principal::{GrantSet, Principal, SystemPrincipal};
        let db = aiplane_core::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        let person = ToolContext::for_test(db.clone());
        assert_eq!(person.person("remember").unwrap(), "u");

        let system = ToolContext {
            principal: Principal::System(SystemPrincipal {
                id: "p1".into(),
                name: "ci-bot".into(),
                grants: std::sync::Arc::new(GrantSet::default()),
            }),
            ..ToolContext::for_test(db)
        };
        let err = system.person("remember").unwrap_err().to_string();
        assert!(err.contains("`remember`"), "{err}");
        assert!(err.contains("`ci-bot`"), "{err}");
    }
}
