// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Per-user MCP connection manager.
//!
//! Holds live connections to the remote MCP servers each user has connected
//! (keyed `(user_id, connector_key)`), lazily (re)establishing them with the
//! user's own OAuth access token — refreshing the token first when it's
//! expired. Connections are cached for [`CACHE_TTL`] so an active conversation
//! reuses one rather than re-handshaking every turn.
//!
//! Per request, [`McpConnectionManager::layer_for_user`] produces a
//! [`UserMcpLayer`] — a [`ToolSource`] overlay of the user's connected-connector
//! tools (minus the ones they set to `off`), which [`CompositeToolSource`]
//! unions on top of the static [`ToolRegistry`] for the tool-call runner.
//!
//! A tool in `ask` mode is offered in chat behind [`AskFirst`]: each call
//! pauses the turn until the person approves it, through the same durable
//! suspension every approval uses (`docs/agent-hil.md` → "Suspend and resume").
//! Over `/v1` nobody can answer a pause, so there the token's policy decides
//! instead: a token allowed `ask` tools runs them unasked, any other never
//! sees them.

use aiplane_agents::db::agent_audit::redaction::redacted_arguments;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use jiff::Timestamp;
use shared::api::ToolDef;
use tokio::sync::Mutex;

use serde_json::Value;

use super::identities::StoredMcpIdentity;
use super::{ConnectedServer, McpTool, connect_http_server};
use crate::server::tools::ask_first::{AskFirst, DEFAULT_APPROVAL_TIMEOUT};
use crate::server::tools::{Tool, ToolContext, ToolFuture, ToolRegistry, ToolSource};
use aiplane_core::server::auth::mcp_oauth;
use aiplane_core::server::crypto::Crypto;
use aiplane_core::server::db::Pool;
use aiplane_core::server::db::mcp_audit;
use aiplane_core::server::db::mcp_catalog::{self, Connector};
use aiplane_core::server::db::mcp_connections::{
    self, Connection, ConnectionIdentity, ConnectionOwner, RefreshTokens,
};
use aiplane_core::server::db::user_mcp::{self, ToolMode};
use aiplane_core::server::rbac::Resolver;

/// How long a live connection (and its tool listing) is reused before a
/// refresh. Keeps active conversations warm without holding sockets forever.
const CACHE_TTL: Duration = Duration::from_secs(10 * 60);

/// Refresh the access token when it expires within this window (or already
/// has), so a call never races the expiry.
const REFRESH_SKEW_SECS: i64 = 60;

/// Hard cap on live cached connections. Past this, idle/stale entries are
/// evicted (closing their sockets) before inserting a new one — bounds memory
/// + open MCP sessions on a long-running daemon.
const MAX_CACHE_ENTRIES: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum CacheKey {
    Owned {
        identity: ConnectionIdentity,
        catalog_generation: String,
    },
    Shared {
        connector_key: String,
        catalog_generation: String,
    },
}

struct Cached {
    tools: Vec<Arc<McpTool>>,
    fetched_at: Instant,
}

/// Why a connection couldn't produce a usable access token, plus the one bit
/// the caller acts on: whether the credential is dead (only the user can fix
/// it, by authorizing again) or the failure might simply pass.
#[derive(Debug, Clone)]
pub struct TokenError {
    pub message: String,
    pub needs_reauth: bool,
}

impl TokenError {
    /// A failure we may recover from on our own.
    fn transient(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            needs_reauth: false,
        }
    }
}

impl std::fmt::Display for TokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl From<TokenError> for String {
    fn from(e: TokenError) -> String {
        e.message
    }
}

/// Where a user's overlay is built for, which decides what `ask` means.
#[derive(Clone, Copy)]
pub enum AskContext<'a> {
    /// A conversation that can pause: `ask` tools are offered behind
    /// [`AskFirst`], and the person approves each call in the chat or inbox.
    Chat,
    /// `/v1` API for a specific token, where nothing can pause: `ask` tools
    /// are offered, unasked, iff that token's policy allows them
    /// (`token_mcp_policy`), a standing pre-authorisation by the person.
    Api { token_id: &'a str },
}

/// What an `ask`-mode tool becomes in one overlay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AskOffer {
    /// Not offered.
    Hide,
    /// Offered, and each call waits for an approval.
    AskFirst,
    /// Offered and run without asking: the token's owner allowed it.
    Unasked,
}

type RefreshLocks = Mutex<HashMap<(ConnectionOwner, String), Arc<Mutex<()>>>>;

/// Process-wide manager. Cheap to clone the `Arc` in `AppState`.
pub struct McpConnectionManager {
    db: Pool,
    crypto: Arc<Crypto>,
    rbac: Arc<Resolver>,
    cache: Mutex<HashMap<CacheKey, Cached>>,
    /// Per-`(user,connector)` refresh locks: serialize token refreshes so the
    /// background worker and a live request can't both spend the same refresh
    /// token (which, with rotation, would invalidate one of them).
    refresh_locks: RefreshLocks,
}

impl McpConnectionManager {
    pub fn new(db: Pool, crypto: Arc<Crypto>, rbac: Arc<Resolver>) -> Arc<Self> {
        Arc::new(Self {
            db,
            crypto,
            rbac,
            cache: Mutex::new(HashMap::new()),
            refresh_locks: Mutex::new(HashMap::new()),
        })
    }

    /// Get-or-create the refresh lock for a `(user, connector)` pair.
    async fn refresh_lock(&self, owner: &ConnectionOwner, connector_key: &str) -> Arc<Mutex<()>> {
        let mut locks = self.refresh_locks.lock().await;
        locks
            .entry((owner.clone(), connector_key.to_owned()))
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }
    fn global_cache_key(connector: &Connector) -> CacheKey {
        CacheKey::Shared {
            connector_key: connector.key.clone(),
            catalog_generation: connector.dispatch_generation.clone(),
        }
    }
    pub async fn invalidate(&self, owner: &ConnectionOwner, connector_key: &str) {
        self.cache.lock().await.retain(|key,_| !matches!(key,CacheKey::Owned { identity, .. } if &identity.owner==owner && identity.connector_key==connector_key));
    }

    async fn current_connector(&self, expected: &Connector) -> Result<Connector, String> {
        mcp_catalog::get(&self.db, &expected.key)
            .await
            .map_err(|error| format!("loading connector: {error}"))?
            .filter(|current| {
                current.enabled && current.dispatch_generation == expected.dispatch_generation
            })
            .ok_or_else(|| "connector configuration is unavailable or changed".to_string())
    }

    /// Record what this connector exposes, so the admin group editor can offer
    /// its tools as grants.
    ///
    /// A connector's tool list lives on the server and is only ever seen while
    /// connected on some user's behalf, which an admin editing a group cannot
    /// do. A connector nobody has connected has nothing to offer yet.
    async fn record_catalog_tools(
        &self,
        connector_key: &str,
        catalog_generation: &str,
        tools: &[Arc<McpTool>],
    ) -> Result<bool, String> {
        use crate::server::tools::Tool;
        let cached: Vec<mcp_catalog::CachedTool> = tools
            .iter()
            .map(|t| mcp_catalog::CachedTool {
                tool_id: t.id().to_string(),
                description: t.schema().function.description,
            })
            .collect();
        mcp_catalog::replace_tools(&self.db, connector_key, catalog_generation, &cached)
            .await
            .map_err(|error| format!("recording connector tools: {error}"))
    }

    /// Return the cached tools for `ck` if still within [`CACHE_TTL`].
    async fn cache_lookup(&self, ck: &CacheKey) -> Option<Vec<Arc<McpTool>>> {
        let cache = self.cache.lock().await;
        cache
            .get(ck)
            .filter(|c| c.fetched_at.elapsed() < CACHE_TTL)
            .map(|c| c.tools.clone())
    }

    /// Insert `tools` under `ck`, bounding the cache: when full and this is a
    /// new key, evict stale entries first (dropping them closes their MCP
    /// sockets), then the oldest.
    async fn cache_store(&self, ck: CacheKey, tools: Vec<Arc<McpTool>>) {
        let mut cache = self.cache.lock().await;
        if cache.len() >= MAX_CACHE_ENTRIES && !cache.contains_key(&ck) {
            cache.retain(|_, c| c.fetched_at.elapsed() < CACHE_TTL);
            if cache.len() >= MAX_CACHE_ENTRIES
                && let Some(oldest) = cache
                    .iter()
                    .min_by_key(|(_, c)| c.fetched_at)
                    .map(|(k, _)| k.clone())
            {
                cache.remove(&oldest);
            }
        }
        cache.insert(
            ck,
            Cached {
                tools,
                fetched_at: Instant::now(),
            },
        );
    }

    /// Build the per-request tool overlay for `user_id`: every connected,
    /// enabled connector's tools. `off` tools are always excluded; how `ask`
    /// tools are treated depends on [`AskContext`] (hidden in chat — where
    /// there's no per-call approval UI yet — and gated by the per-token policy
    /// over the API). A connector that fails to connect/refresh is logged and
    /// skipped — it never fails the whole turn.
    pub async fn layer_for_user(
        &self,
        user_id: &str,
        role_ids: &[String],
        is_admin: bool,
        ask: AskContext<'_>,
    ) -> UserMcpLayer {
        let keys =
            mcp_connections::connected_keys(&self.db, &ConnectionOwner::User(user_id.to_owned()))
                .await
                .unwrap_or_default();
        // Per-user connectors the user has connected. Resolve concurrently —
        // one slow/unreachable server can't serialise the whole turn behind it.
        let futs = keys.into_iter().map(|key| async move {
            let connector = match mcp_catalog::get(&self.db, &key).await {
                Ok(Some(c)) if c.enabled && c.is_per_user() => c,
                // Connector gone, disabled by the admin, or flipped to global
                // (no per-user connection applies) → hide its tools.
                _ => return None,
            };
            if !self.role_allows(&connector, role_ids, is_admin) {
                return None;
            }
            let (asks, modes) = self.ask_and_modes(user_id, &key, ask).await;
            match self
                .ensure_connection(&ConnectionOwner::User(user_id.to_owned()), &connector)
                .await
            {
                Ok(tools) => Some((key, tools, modes, asks, connector.audit)),
                Err(err) => {
                    tracing::warn!(user = %user_id, connector = %key, error = %err,
                        "MCP connector unavailable this turn");
                    None
                }
            }
        });

        // Global connectors: one shared identity for the whole gateway, exposed
        // to everyone RBAC allows with no per-user connection step. Their tools
        // still respect the user's own always/ask/off prefs and the per-token
        // ask policy, so they slot into the same overlay.
        let globals: Vec<Connector> = mcp_catalog::list_enabled(&self.db)
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|c| c.is_global())
            .collect();
        let global_futs = globals.into_iter().map(|connector| async move {
            let key = connector.key.clone();
            if !self.role_allows(&connector, role_ids, is_admin) {
                return None;
            }
            let (asks, modes) = self.ask_and_modes(user_id, &key, ask).await;
            match self.ensure_global(&connector).await {
                Ok(tools) => Some((key, tools, modes, asks, connector.audit)),
                Err(err) => {
                    tracing::warn!(connector = %key, error = %err,
                        "global MCP connector unavailable this turn");
                    None
                }
            }
        });

        let (resolved, global_resolved) = rama::futures::future::join(
            rama::futures::future::join_all(futs),
            rama::futures::future::join_all(global_futs),
        )
        .await;
        let mut layer = UserMcpLayer::default();
        for (key, tools, modes, asks, audit) in resolved
            .into_iter()
            .flatten()
            .chain(global_resolved.into_iter().flatten())
        {
            let audit_db = audit.then(|| self.db.clone());
            layer.add(&key, &tools, &modes, asks, audit_db.as_ref());
        }
        layer
    }

    /// The overlay for a system principal: exactly the connectors it was
    /// granted, and only those with one shared gateway identity (`global` or
    /// `agent`).
    /// `user_mcp` — every person's own connections and tool preferences — is
    /// never read, so a principal cannot reach anyone's OAuth tokens even when
    /// granted a per-user connector's key. Connector `allowed_groups` does not
    /// apply: the grant is the whole decision. `ask`-mode tools stay hidden:
    /// an agent's approvals are its spec's (`tool_resources.<tool>.permission`),
    /// never a person's store setting.
    pub async fn layer_for_principal(
        &self,
        principal: &aiplane_core::server::principal::SystemPrincipal,
    ) -> UserMcpLayer {
        use aiplane_core::server::principal::GrantKind;
        let granted: Vec<&str> = principal.grants.refs(GrantKind::Connector).collect();
        if granted.is_empty() {
            return UserMcpLayer::default();
        }
        let connectors: Vec<Connector> = mcp_catalog::list_enabled(&self.db)
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|c| c.has_shared_identity() && granted.contains(&c.key.as_str()))
            .collect();
        let futs = connectors.into_iter().map(|connector| async move {
            match self.ensure_global(&connector).await {
                Ok(tools) => Some((connector.key.clone(), tools, connector.audit)),
                Err(err) => {
                    tracing::warn!(principal = %principal.name, connector = %connector.key,
                        error = %err, "granted MCP connector unavailable this request");
                    None
                }
            }
        });
        let mut layer = UserMcpLayer::default();
        for (key, tools, audit) in rama::futures::future::join_all(futs)
            .await
            .into_iter()
            .flatten()
        {
            let audit_db = audit.then(|| self.db.clone());
            layer.add(
                &key,
                &tools,
                &HashMap::new(),
                AskOffer::Hide,
                audit_db.as_ref(),
            );
        }
        layer
    }

    /// RBAC gate at exposure time: a connector restricted to `allowed_groups`
    /// is only exposed to users holding one of those gateway groups (admins
    /// bypass). Re-checked every turn, so revoking a group or restricting a
    /// connector drops the tools immediately. Delegates to
    /// [`Connector::allows`] so pools, RAG, and MCP gate identically.
    fn role_allows(&self, connector: &Connector, role_ids: &[String], is_admin: bool) -> bool {
        connector.allows(role_ids, is_admin)
    }

    /// Resolve, for one connector, what its `ask`-mode tools become in this
    /// context and the user's per-tool mode overrides.
    pub(super) async fn ask_and_modes(
        &self,
        user_id: &str,
        key: &str,
        ask: AskContext<'_>,
    ) -> (AskOffer, HashMap<String, ToolMode>) {
        let asks = match ask {
            AskContext::Chat => AskOffer::AskFirst,
            AskContext::Api { token_id } => {
                match user_mcp::token_ask_policy(&self.db, token_id, key)
                    .await
                    .unwrap_or(user_mcp::AskOverApi::Block)
                {
                    user_mcp::AskOverApi::Allow => AskOffer::Unasked,
                    user_mcp::AskOverApi::Block => AskOffer::Hide,
                }
            }
        };
        let modes = user_mcp::tool_modes(&self.db, user_id, key)
            .await
            .unwrap_or_default();
        (asks, modes)
    }

    /// Ensure a live connection for `(user, connector)`, returning its tools.
    /// Reuses the cache within [`CACHE_TTL`]; otherwise refreshes the token if
    /// needed and re-handshakes.
    pub async fn ensure_connection(
        &self,
        owner: &ConnectionOwner,
        connector: &Connector,
    ) -> Result<Vec<Arc<McpTool>>, String> {
        let current_connector = self.current_connector(connector).await?;
        let connector = &current_connector;
        let conn = mcp_connections::get_connection(&self.db, owner, &connector.key)
            .await
            .map_err(|e| format!("loading connection: {e}"))?
            .filter(|c| c.status == mcp_connections::STATUS_CONNECTED)
            .ok_or_else(|| "not connected".to_string())?;
        if conn.catalog_generation.as_deref() != Some(connector.dispatch_generation.as_str()) {
            return Err(
                "connection belongs to an unavailable connector configuration; reconnect".into(),
            );
        }
        let identity = conn.identity();
        let ck = CacheKey::Owned {
            identity: identity.clone(),
            catalog_generation: connector.dispatch_generation.clone(),
        };
        if let Some(tools) = self.cache_lookup(&ck).await {
            return Ok(tools);
        }

        // Open connectors carry no credentials at all — the user's "connected"
        // row exists purely so they can opt the tools in/out, not for auth.
        let connected = if connector.auth == mcp_catalog::AuthKind::None {
            connect_http_server(&connector.key, &connector.url, None).await?
        } else {
            let (access, refreshed) = self
                .access_token(owner, connector, &conn)
                .await
                .map_err(String::from)?;
            match connect_http_server(&connector.key, &connector.url, Some(&access)).await {
                Ok(s) => s,
                // The server rejected a token we didn't think was expired
                // (revoked upstream, clock skew, rotated elsewhere). If we haven't
                // already refreshed this pass and a refresh token exists, force one
                // refresh + a single reconnect before giving up.
                Err(e) if !refreshed && conn.refresh_token_ct.is_some() => {
                    tracing::info!(owner = ?owner, connector = %connector.key,
                    "MCP connect failed; forcing token refresh + one retry");
                    let new_access = match self.refresh(owner, connector, true).await {
                        Ok(access) => access,
                        Err(re) => {
                            // Park the connection with the right reason, same as
                            // the expiry-driven refresh path below does.
                            let msg = format!("{e}; forced refresh also failed: {re}");
                            return Err(msg);
                        }
                    };
                    connect_http_server(&connector.key, &connector.url, Some(&new_access))
                        .await
                        .map_err(|e2| format!("reconnect after refresh failed: {e2}"))?
                }
                Err(e) => return Err(e),
            }
        };
        let ConnectedServer { conn: _live, tools } = connected;
        let current = mcp_connections::get_connection(&self.db, owner, &connector.key)
            .await
            .map_err(|e| format!("checking connection: {e}"))?;
        if !current.is_some_and(|c| {
            c.status == mcp_connections::STATUS_CONNECTED
                && c.identity() == identity
                && c.catalog_generation.as_deref() == Some(connector.dispatch_generation.as_str())
        }) {
            return Err("connection changed while connecting".into());
        }
        let tools: Vec<Arc<McpTool>> = tools
            .into_iter()
            .map(|tool| {
                Arc::new(tool.with_identity(
                    StoredMcpIdentity {
                        connection: identity.clone(),
                        binding_generation: None,
                        catalog_generation: connector.dispatch_generation.clone(),
                    },
                    self.rbac.clone(),
                ))
            })
            .collect();
        if !self
            .record_catalog_tools(&connector.key, &connector.dispatch_generation, &tools)
            .await?
        {
            return Err("connector changed while connecting".into());
        }
        self.cache_store(ck, tools.clone()).await;
        Ok(tools)
    }

    /// Ensure the single shared connection for a `global` connector, returning
    /// its tools. Unlike [`Self::ensure`] there is no per-user connection row
    /// and the connection is cached per-connector (shared across users). Auth
    /// comes from the connector row: `None` sends no credentials (e.g. Discord,
    /// whose bot token is baked into the sidecar behind a loopback endpoint);
    /// `StaticBearer` sends the shared token stored (encrypted) on the row.
    /// `OAuth2` is inherently per-user and rejected here (and at admin save).
    pub(super) async fn ensure_global(
        &self,
        connector: &Connector,
    ) -> Result<Vec<Arc<McpTool>>, String> {
        let current_connector = self.current_connector(connector).await?;
        let connector = &current_connector;
        let ck = Self::global_cache_key(connector);
        if let Some(tools) = self.cache_lookup(&ck).await {
            return Ok(tools);
        }
        let connected = match connector.auth {
            mcp_catalog::AuthKind::None => {
                connect_http_server(&connector.key, &connector.url, None).await?
            }
            mcp_catalog::AuthKind::StaticBearer => {
                let token = self.decrypt_connector_secret(connector)?.ok_or_else(|| {
                    "global static_bearer connector has no bearer token configured".to_string()
                })?;
                connect_http_server(&connector.key, &connector.url, Some(&token)).await?
            }
            mcp_catalog::AuthKind::OAuth2 => {
                return Err("OAuth2 is per-user and not valid for a global connector".to_string());
            }
        };
        let ConnectedServer { conn: _live, tools } = connected;
        let tools: Vec<Arc<McpTool>> = tools
            .into_iter()
            .map(|tool| {
                Arc::new(tool.with_dispatch_facts(
                    super::McpDispatchFacts::Shared {
                        connector_key: connector.key.clone(),
                        catalog_generation: connector.dispatch_generation.clone(),
                    },
                    self.rbac.clone(),
                ))
            })
            .collect();
        if !self
            .record_catalog_tools(&connector.key, &connector.dispatch_generation, &tools)
            .await?
        {
            return Err("connector changed while connecting".into());
        }
        self.cache_store(ck, tools.clone()).await;
        Ok(tools)
    }

    /// Decrypt the stored access token, refreshing it first when it's expired
    /// (or about to be). Returns `(token, refreshed)` — `refreshed` is true
    /// when a refresh ran, so the caller can avoid a redundant forced refresh
    /// on a subsequent connect failure. On refresh failure the connection is
    /// marked errored.
    async fn access_token(
        &self,
        owner: &ConnectionOwner,
        connector: &Connector,
        conn: &Connection,
    ) -> Result<(String, bool), TokenError> {
        self.current_connector(connector)
            .await
            .map_err(TokenError::transient)?;
        if conn.catalog_generation.as_deref() != Some(connector.dispatch_generation.as_str()) {
            return Err(TokenError {
                message: "connector configuration changed; reconnect".into(),
                needs_reauth: true,
            });
        }
        let fresh_enough = conn
            .token_expires_at
            .map(|exp| exp > Timestamp::now() + jiff::Span::new().seconds(REFRESH_SKEW_SECS))
            .unwrap_or(true); // no expiry recorded → assume usable
        if fresh_enough {
            return match self.decrypt_access(conn) {
                Ok(token) => Ok((token, false)),
                // A decrypt failure won't self-heal — the encryption key changed
                // or the value was sealed under a different key. The stored
                // credential is unreadable, so this is a reauth case: the
                // connection drops out of `connected_keys`, stops silently
                // re-failing every turn, and the store asks for a new sign-in
                // instead of showing a stale "connected".
                Err(err) => {
                    let _ = mcp_connections::mark_error(
                        &self.db,
                        &conn.identity(),
                        &connector.dispatch_generation,
                        &err,
                        true,
                    )
                    .await;
                    Err(TokenError {
                        message: err,
                        needs_reauth: true,
                    })
                }
            };
        }
        // Needs refresh.
        match self.refresh(owner, connector, false).await {
            Ok(token) => Ok((token, true)),
            Err(err) => Err(err),
        }
    }

    /// Proactively refresh one connection (background worker entry point):
    /// runs the refresh under the per-connection lock and drops any cached live
    /// connection so the next use picks up the new token. Marks the connection
    /// errored on failure so the store surfaces "needs reconnect".
    pub async fn refresh_connection(
        &self,
        owner: &ConnectionOwner,
        connector_key: &str,
    ) -> Result<(), TokenError> {
        let connector = mcp_catalog::get(&self.db, connector_key)
            .await
            .map_err(|e| TokenError::transient(format!("loading connector: {e}")))?
            .ok_or_else(|| TokenError::transient("connector no longer in catalog"))?;
        match self.refresh(owner, &connector, true).await {
            Ok(_) => {
                self.invalidate(owner, connector_key).await;
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    fn decrypt_access(&self, conn: &Connection) -> Result<String, String> {
        match (&conn.access_token_ct, &conn.access_token_nonce) {
            (Some(ct), Some(nonce)) => self
                .crypto
                .open_str(nonce, ct)
                .map_err(|e| format!("decrypting access token: {e}")),
            _ => Err("no access token stored".into()),
        }
    }

    /// Run the OAuth refresh flow and persist the new tokens. Serialized per
    /// `(user, connector)` so a concurrent refresh (background worker vs a live
    /// request) can't double-spend the refresh token. Reloads the connection
    /// under the lock; unless `force`, returns the existing token if another
    /// task already refreshed it while we waited.
    async fn refresh(
        &self,
        owner: &ConnectionOwner,
        connector: &Connector,
        force: bool,
    ) -> Result<String, TokenError> {
        let lock = self.refresh_lock(owner, &connector.key).await;
        let _held = lock.lock().await;
        // Reload under the lock so we see any refresh a concurrent task just did.
        let conn = mcp_connections::get_connection(&self.db, owner, &connector.key)
            .await
            .map_err(|e| TokenError::transient(format!("loading connection: {e}")))?
            .filter(|c| c.status == mcp_connections::STATUS_CONNECTED)
            .ok_or_else(|| TokenError::transient("not connected"))?;
        self.current_connector(connector)
            .await
            .map_err(TokenError::transient)?;
        if conn.catalog_generation.as_deref() != Some(connector.dispatch_generation.as_str()) {
            return Err(TokenError {
                message: "connector configuration changed; reconnect".into(),
                needs_reauth: true,
            });
        }
        let identity = conn.identity();
        let result: Result<String, TokenError> = async {
            if !force
                && conn
                    .token_expires_at
                    .map(|exp| {
                        exp > Timestamp::now() + jiff::Span::new().seconds(REFRESH_SKEW_SECS)
                    })
                    .unwrap_or(false)
            {
                // A concurrent refresh already produced a fresh token — reuse it.
                return self.decrypt_access(&conn).map_err(TokenError::transient);
            }
            let (rt_ct, rt_nonce) = match (&conn.refresh_token_ct, &conn.refresh_token_nonce) {
                (Some(ct), Some(nonce)) => (ct, nonce),
                // Nothing to refresh *with*: only a new authorization can help,
                // so this is a reauth case rather than a retryable error.
                _ => {
                    return Err(TokenError {
                        message: "access token expired and no refresh token stored — reconnect"
                            .into(),
                        needs_reauth: true,
                    });
                }
            };
            let refresh_token = self
                .crypto
                .open_str(rt_nonce, rt_ct)
                .map_err(|e| TokenError::transient(format!("decrypting refresh token: {e}")))?;

            // Reuse the token endpoint resolved + persisted at connect time; only
            // re-run discovery for older connections that predate persistence.
            // Avoids re-fetching (and re-trusting) the MCP server's discovery doc
            // on the long-lived refresh path.
            let token_url = match conn.token_url.clone() {
                Some(u) => u,
                None => {
                    let ov = mcp_oauth::Overrides {
                        authorize_url: connector.authorize_url.clone(),
                        token_url: connector.token_url.clone(),
                        registration_url: connector.registration_url.clone(),
                    };
                    mcp_oauth::discover(&connector.url, &ov)
                        .await
                        .map_err(|e| TokenError::transient(format!("discovery for refresh: {e}")))?
                        .token_url
                }
            };

            let (client_id, client_secret) = self
                .client_credentials(connector, &conn)
                .map_err(TokenError::transient)?;
            // `needs_reauth` rides along: an `invalid_grant` / `invalid_client` here
            // is the authorization server telling us this credential is finished —
            // a dead refresh token, or (for a DCR client) a server that no longer
            // knows the client we registered, e.g. after it lost its OAuth store.
            let tokens = mcp_oauth::refresh(
                &token_url,
                &refresh_token,
                &client_id,
                client_secret.as_deref(),
            )
            .await
            .map_err(|e| TokenError {
                message: format!("token refresh: {e}"),
                needs_reauth: e.needs_reauth(),
            })?;

            let access_sealed = self
                .crypto
                .seal_str(&tokens.access_token)
                .map_err(|e| TokenError::transient(format!("sealing access token: {e}")))?;
            let refresh_sealed =
                match tokens.refresh_token.as_deref() {
                    Some(rt) => Some(self.crypto.seal_str(rt).map_err(|e| {
                        TokenError::transient(format!("sealing refresh token: {e}"))
                    })?),
                    None => None,
                };
            let saved = mcp_connections::update_tokens(
                &self.db,
                &identity,
                &connector.dispatch_generation,
                RefreshTokens {
                    access_token_ct: &access_sealed.ciphertext,
                    access_token_nonce: &access_sealed.nonce,
                    refresh_token_ct: refresh_sealed.as_ref().map(|s| s.ciphertext.as_slice()),
                    refresh_token_nonce: refresh_sealed.as_ref().map(|s| s.nonce.as_slice()),
                    token_expires_at: tokens.expires_at,
                },
            )
            .await
            .map_err(|e| TokenError::transient(format!("persisting refreshed tokens: {e}")))?;
            if !saved {
                return Err(TokenError::transient(
                    "connection changed during token refresh",
                ));
            }

            Ok(tokens.access_token)
        }
        .await;
        if let Err(err) = &result {
            let _ = mcp_connections::mark_error(
                &self.db,
                &identity,
                &connector.dispatch_generation,
                &err.message,
                err.needs_reauth,
            )
            .await;
        }
        result
    }

    /// Resolve `(client_id, client_secret?)` for token requests: the
    /// per-connection DCR client when present, else the catalog's static
    /// client (with its secret decrypted).
    fn client_credentials(
        &self,
        connector: &Connector,
        conn: &Connection,
    ) -> Result<(String, Option<String>), String> {
        if let Some(dcr_id) = &conn.dcr_client_id {
            let secret = match (&conn.dcr_client_secret_ct, &conn.dcr_client_secret_nonce) {
                (Some(ct), Some(nonce)) => Some(
                    self.crypto
                        .open_str(nonce, ct)
                        .map_err(|e| format!("decrypting DCR client secret: {e}"))?,
                ),
                _ => None,
            };
            return Ok((dcr_id.clone(), secret));
        }
        let client_id = connector
            .client_id
            .clone()
            .ok_or_else(|| "connector has no client_id configured".to_string())?;
        let secret = self.decrypt_connector_secret(connector)?;
        Ok((client_id, secret))
    }

    /// Decrypt the catalog connector's static client secret, if any.
    pub fn decrypt_connector_secret(
        &self,
        connector: &Connector,
    ) -> Result<Option<String>, String> {
        match (&connector.client_secret_ct, &connector.client_secret_nonce) {
            (Some(ct), Some(nonce)) => self
                .crypto
                .open_str(nonce, ct)
                .map(Some)
                .map_err(|e| format!("decrypting connector client secret: {e}")),
            _ => Ok(None),
        }
    }

    /// Tool metadata for the connector store UI: every tool the connector
    /// exposes (regardless of the user's `off` choices), with the server's
    /// read-only hint and the user's current effective mode. Connects if
    /// needed (cache-warm otherwise).
    pub async fn connector_tool_infos(
        &self,
        user_id: &str,
        connector: &Connector,
    ) -> Result<Vec<ToolInfo>, String> {
        let tools = if connector.has_shared_identity() {
            self.ensure_global(connector).await?
        } else {
            self.ensure_connection(&ConnectionOwner::User(user_id.to_owned()), connector)
                .await?
        };
        let modes = user_mcp::tool_modes(&self.db, user_id, &connector.key)
            .await
            .unwrap_or_default();
        Ok(tools
            .iter()
            .map(|t| {
                let default = default_mode(t.read_only(), t.destructive());
                let mode = modes.get(t.remote_name()).copied().unwrap_or(default);
                ToolInfo {
                    remote_name: t.remote_name().to_string(),
                    description: t.def().function.description.clone(),
                    read_only: t.read_only(),
                    mode,
                }
            })
            .collect())
    }

    /// Access to the shared crypto (for the OAuth handlers that seal tokens).
    pub fn crypto(&self) -> &Crypto {
        &self.crypto
    }
}

/// Tool metadata for the connector store UI.
pub struct ToolInfo {
    pub remote_name: String,
    pub description: String,
    pub read_only: bool,
    pub mode: ToolMode,
}

/// Default permission tier for a tool when the user hasn't set one: only a
/// destructive non-read tool defaults to `ask`; everything else (reads,
/// queries, and un-annotated tools) defaults to `always`, so a connector the
/// user explicitly connected actually works in chat without pre-authorizing
/// every tool. Destructive tools stay gated: each call asks first in chat.
fn default_mode(read_only: bool, destructive: bool) -> ToolMode {
    if destructive && !read_only {
        ToolMode::Ask
    } else {
        ToolMode::Always
    }
}

/// Whether a tool with effective `mode` is offered, and if so whether each
/// call asks first. `off` is never offered, `always` always and unasked.
fn offer(mode: ToolMode, asks: AskOffer) -> Option<bool> {
    match (mode, asks) {
        (ToolMode::Off, _) | (ToolMode::Ask, AskOffer::Hide) => None,
        (ToolMode::Always, _) | (ToolMode::Ask, AskOffer::Unasked) => Some(false),
        (ToolMode::Ask, AskOffer::AskFirst) => Some(true),
    }
}

/// A [`Tool`] wrapper that records every call to [`mcp_audit`]. Used for
/// connectors whose `audit` flag is on — the model-facing schema and behaviour
/// are unchanged; a best-effort audit row (acting user, connector, tool,
/// truncated args, outcome) is written after the inner tool runs. A failed
/// audit write is logged, never propagated — auditing must not break the call.
struct AuditedTool {
    inner: Arc<dyn Tool>,
    connector_key: String,
    db: Pool,
    /// Record the redaction marker (`redacted_arguments`, the activity
    /// log's own) instead of the arguments: set for a call whose
    /// arguments carry a secret, such as a verifier's `check_code` with the
    /// visitor's code.
    sensitive: bool,
}

impl Tool for AuditedTool {
    fn phase(&self) -> crate::server::tools::ToolPhase {
        self.inner.phase()
    }

    fn id(&self) -> &str {
        self.inner.id()
    }

    fn execution_facts(&self) -> Option<Value> {
        self.inner.execution_facts()
    }

    fn action_catalogue(&self) -> Option<&'static [crate::server::tools::ToolAction]> {
        self.inner.action_catalogue()
    }
    fn classify_actions<'a>(
        &'a self,
        ctx: ToolContext,
        args: Value,
    ) -> crate::server::tools::ToolActionFuture<'a> {
        self.inner.classify_actions(ctx, args)
    }

    fn schema(&self) -> ToolDef {
        self.inner.schema()
    }

    fn max_duration(&self) -> Option<Duration> {
        self.inner.max_duration()
    }

    fn sensitive_args(&self) -> bool {
        self.sensitive || self.inner.sensitive_args()
    }

    fn changes_state(&self) -> bool {
        self.inner.changes_state()
    }

    fn read_only(&self) -> bool {
        self.inner.read_only()
    }

    fn run<'a>(&'a self, ctx: ToolContext, args: Value) -> ToolFuture<'a> {
        let db = self.db.clone();
        let connector = self.connector_key.clone();
        let tool_id = self.inner.id().to_string();
        let principal = ctx.principal.clone();
        let session = ctx.session_id.clone();
        let chain = ctx.agent.as_ref().map(|a| a.chain().clone());
        let args_summary = if self.sensitive_args() {
            Some(redacted_arguments().to_string())
        } else {
            serde_json::to_string(&args).ok()
        };
        let inner = self.inner.clone();
        let sensitive = self.sensitive_args();
        Box::pin(async move {
            let res = inner.run(ctx, args).await;
            // A remote error may quote the arguments back ("code 123456 is
            // wrong"), so a sensitive call keeps only that it failed.
            let (outcome, error) = match &res {
                Ok(_) => ("ok", None),
                Err(_) if sensitive => ("error", Some(redacted_arguments().to_string())),
                Err(e) => ("error", Some(e.to_string())),
            };
            if let Err(e) = mcp_audit::record(
                &db,
                &principal,
                &connector,
                &tool_id,
                args_summary.as_deref(),
                outcome,
                error.as_deref(),
                session.as_deref(),
                chain.as_deref(),
            )
            .await
            {
                tracing::warn!(error = %e, connector = %connector, tool = %tool_id,
                    "MCP tool audit write failed");
            }
            res
        })
    }
}

/// A tool whose arguments are declared sensitive, for an unaudited connector.
struct SensitiveArgs(Arc<dyn Tool>);

impl Tool for SensitiveArgs {
    fn phase(&self) -> crate::server::tools::ToolPhase {
        self.0.phase()
    }

    fn id(&self) -> &str {
        self.0.id()
    }

    fn execution_facts(&self) -> Option<Value> {
        self.0.execution_facts()
    }

    fn action_catalogue(&self) -> Option<&'static [crate::server::tools::ToolAction]> {
        self.0.action_catalogue()
    }
    fn classify_actions<'a>(
        &'a self,
        ctx: ToolContext,
        args: Value,
    ) -> crate::server::tools::ToolActionFuture<'a> {
        self.0.classify_actions(ctx, args)
    }

    fn schema(&self) -> ToolDef {
        self.0.schema()
    }

    fn max_duration(&self) -> Option<Duration> {
        self.0.max_duration()
    }

    fn sensitive_args(&self) -> bool {
        true
    }

    fn changes_state(&self) -> bool {
        self.0.changes_state()
    }

    fn read_only(&self) -> bool {
        self.0.read_only()
    }

    fn run<'a>(&'a self, ctx: ToolContext, args: Value) -> ToolFuture<'a> {
        self.0.run(ctx, args)
    }
}

/// Per-request overlay of a user's connected-connector MCP tools.
#[derive(Default)]
pub struct UserMcpLayer {
    tools: HashMap<String, Arc<dyn Tool>>,
    /// The raw tool under each audited id, so a sensitive call can be
    /// audited without its arguments.
    audited: HashMap<String, (Arc<dyn Tool>, String, Pool)>,
    defs: Vec<ToolDef>,
    /// id → effective permission mode (`always` / `ask`; `off` tools are
    /// excluded entirely).
    modes: HashMap<String, ToolMode>,
    /// id → connector key (for the per-token /v1 ask policy).
    connector_of: HashMap<String, String>,
    /// The ids whose every call waits for an approval.
    ask_first: std::collections::HashSet<String>,
}

impl UserMcpLayer {
    pub(super) fn add(
        &mut self,
        connector_key: &str,
        tools: &[Arc<McpTool>],
        modes: &HashMap<String, ToolMode>,
        asks: AskOffer,
        audit_db: Option<&Pool>,
    ) {
        for tool in tools {
            let mode = modes
                .get(tool.remote_name())
                .copied()
                .unwrap_or_else(|| default_mode(tool.read_only(), tool.destructive()));
            let Some(ask_first) = offer(mode, asks) else {
                continue;
            };
            let id = tool.def().function.name.clone();
            if ask_first {
                self.ask_first.insert(id.clone());
            }
            // When the connector is audited, store an audit-wrapping tool in
            // place of the raw one; it delegates everything and records the call.
            let stored: Arc<dyn Tool> = match audit_db {
                Some(db) => {
                    self.audited.insert(
                        id.clone(),
                        (
                            tool.clone() as Arc<dyn Tool>,
                            connector_key.to_string(),
                            db.clone(),
                        ),
                    );
                    Arc::new(AuditedTool {
                        inner: tool.clone() as Arc<dyn Tool>,
                        connector_key: connector_key.to_string(),
                        db: db.clone(),
                        sensitive: false,
                    })
                }
                None => tool.clone() as Arc<dyn Tool>,
            };
            self.tools.insert(id.clone(), stored);
            self.defs.push(tool.def().clone());
            self.modes.insert(id.clone(), mode);
            self.connector_of.insert(id, connector_key.to_string());
        }
    }

    /// Tool `id`, declaring its arguments sensitive: an audited connector
    /// records the redaction marker for them, and the runner never logs
    /// them.
    pub fn get_with_sensitive_args(&self, id: &str) -> Option<Arc<dyn Tool>> {
        let sensitive: Arc<dyn Tool> = match self.audited.get(id) {
            Some((inner, connector_key, db)) => Arc::new(AuditedTool {
                inner: inner.clone(),
                connector_key: connector_key.clone(),
                db: db.clone(),
                sensitive: true,
            }),
            None => Arc::new(SensitiveArgs(self.tools.get(id)?.clone())),
        };
        Some(self.gated(id, sensitive))
    }

    /// `tool`, behind an approval when `id` asks first.
    fn gated(&self, id: &str, tool: Arc<dyn Tool>) -> Arc<dyn Tool> {
        if self.ask_first.contains(id) {
            Arc::new(AskFirst::wrap(tool, DEFAULT_APPROVAL_TIMEOUT))
        } else {
            tool
        }
    }

    /// Effective permission mode for a tool id, if this layer owns it.
    pub fn mode_of(&self, id: &str) -> Option<ToolMode> {
        self.modes.get(id).copied()
    }

    /// Connector key a tool id belongs to (for the per-token /v1 policy).
    pub fn connector_of(&self, id: &str) -> Option<&str> {
        self.connector_of.get(id).map(String::as_str)
    }

    /// Tool ids this overlay provides.
    pub fn tool_ids(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }

    /// The overlay tool ids whose connector toggle key is in `enabled_keys` —
    /// i.e. the tools the conversation has actually turned on. This is the
    /// progressive-disclosure gate for per-user MCP on the chat path: a
    /// connected-but-not-enabled connector contributes nothing here (only its
    /// system-context advertisement), and enabling `mcp__<connector>` surfaces
    /// every tool that connector bridges (since `entry_key_for(mcp__x__*)` all
    /// collapse to `mcp__x`).
    pub fn enabled_tool_ids(
        &self,
        enabled_keys: &std::collections::HashSet<String>,
    ) -> Vec<String> {
        use crate::server::tools::catalog::entry_key_for;
        self.tools
            .keys()
            .filter(|id| enabled_keys.contains(entry_key_for(id)))
            .cloned()
            .collect()
    }

    /// The distinct connector keys this overlay has tools for, sorted. Used by
    /// the chat driver to advertise connectable integrations in the system
    /// context (progressive disclosure for per-user MCP).
    pub fn connector_keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self
            .connector_of
            .values()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        keys.sort();
        keys
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

impl ToolSource for UserMcpLayer {
    fn get(&self, id: &str) -> Option<Arc<dyn Tool>> {
        Some(self.gated(id, self.tools.get(id)?.clone()))
    }

    fn defs_for(&self, allowed: &[String]) -> Vec<ToolDef> {
        allowed.iter().filter_map(|id| self.find_def(id)).collect()
    }

    fn ids(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }

    fn contains(&self, id: &str) -> bool {
        self.tools.contains_key(id)
    }
}

impl UserMcpLayer {
    fn find_def(&self, id: &str) -> Option<ToolDef> {
        self.defs.iter().find(|d| d.function.name == id).cloned()
    }
}

/// The static registry unioned with the gateway-side ComfyUI overlay and a
/// per-request user MCP overlay. Lookups run in declaration order —
/// registry wins on id collisions (built-ins are authoritative), ComfyUI
/// fills in any `comfyui_*` ids the registry doesn't have, MCP fills in
/// everything else. The ComfyUI overlay is a per-request construction
/// (cheaply: one `ComfyuiHandle` clone) so the live catalog snapshot is
/// always fresh — a `/api/v0/comfyui/reload` is visible on the very next
/// chat call without rebuilding the registry.
pub struct CompositeToolSource<'a> {
    registry: &'a ToolRegistry,
    comfyui: Option<&'a crate::server::comfyui_tool::ComfyuiToolSource>,
    user: &'a UserMcpLayer,
}

impl<'a> CompositeToolSource<'a> {
    pub fn new(registry: &'a ToolRegistry, user: &'a UserMcpLayer) -> Self {
        Self {
            registry,
            comfyui: None,
            user,
        }
    }

    /// ComfyUI overlay extension. Builder-style; chain after `new()` when
    /// the caller has a live [`ComfyuiToolSource`] to layer in.
    pub fn with_comfyui(
        mut self,
        comfyui: Option<&'a crate::server::comfyui_tool::ComfyuiToolSource>,
    ) -> Self {
        self.comfyui = comfyui;
        self
    }
}

impl ToolSource for CompositeToolSource<'_> {
    fn get(&self, id: &str) -> Option<Arc<dyn Tool>> {
        ToolSource::get(self.registry, id)
            .or_else(|| self.comfyui.and_then(|c| c.get(id)))
            .or_else(|| self.user.get(id))
    }

    fn defs_for(&self, allowed: &[String]) -> Vec<ToolDef> {
        let mut defs = ToolSource::defs_for(self.registry, allowed);
        // Only add overlay defs for ids the registry didn't already
        // provide, preserving `allowed` order for the overlay tail. ComfyUI
        // first (it's gateway-global, so its ids are stable), then the
        // per-user MCP layer.
        if let Some(comfyui) = self.comfyui {
            for id in allowed {
                if !self.registry.contains(id)
                    && let Some(def) = comfyui
                        .defs_for(std::slice::from_ref(id))
                        .into_iter()
                        .next()
                {
                    defs.push(def);
                }
            }
        }
        for id in allowed {
            if !self.registry.contains(id)
                && !self.comfyui.is_some_and(|c| c.contains(id))
                && let Some(def) = self.user.find_def(id)
            {
                defs.push(def);
            }
        }
        defs
    }

    fn ids(&self) -> Vec<String> {
        let mut ids = ToolSource::ids(self.registry);
        if let Some(comfyui) = self.comfyui {
            ids.extend(comfyui.ids());
        }
        ids.extend(self.user.ids());
        ids
    }

    fn contains(&self, id: &str) -> bool {
        self.registry.contains(id)
            || self.comfyui.is_some_and(|c| c.contains(id))
            || self.user.contains(id)
    }
}

#[cfg(test)]
impl UserMcpLayer {
    /// Test-only: inject a tool overlay entry directly, bypassing a live MCP
    /// connection. Lets the composite/union contracts be tested without a
    /// server.
    fn insert_for_test(&mut self, tool: Arc<dyn Tool>, connector_key: &str, mode: ToolMode) {
        let def = tool.schema();
        let id = def.function.name.clone();
        self.defs.push(def);
        self.tools.insert(id.clone(), tool);
        self.modes.insert(id.clone(), mode);
        self.connector_of.insert(id, connector_key.to_string());
    }
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn audit_and_sensitive_wrappers_preserve_the_tools_phase() {
        let db = aiplane_core::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        let inner: Arc<dyn Tool> = Arc::new(crate::server::tools::tests::PhasedEcho);
        let audited = AuditedTool {
            inner: inner.clone(),
            connector_key: "demo".into(),
            db,
            sensitive: false,
        };
        let sensitive = SensitiveArgs(inner);
        assert_eq!(
            audited.phase(),
            crate::server::tools::ToolPhase::ActsOnState
        );
        assert_eq!(
            sensitive.phase(),
            crate::server::tools::ToolPhase::ActsOnState
        );
        assert_eq!(
            audited.execution_facts(),
            Some(serde_json::json!({"resource":"fixture"}))
        );
        assert_eq!(
            sensitive.execution_facts(),
            Some(serde_json::json!({"resource":"fixture"}))
        );
    }
    use super::*;
    use crate::server::tools::echo::Echo;
    use aiplane_core::server::crypto::Crypto;
    use aiplane_core::server::db;
    use aiplane_core::server::db::mcp_catalog::{AuthKind, Scope};
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn fixture_rbac() -> Arc<Resolver> {
        use aiplane_core::server::rbac::config::{RbacConfig, RoleConfig};
        Arc::new(
            Resolver::build(
                RbacConfig {
                    default_role: Some("member".into()),
                    mappings: vec![],
                },
                vec![RoleConfig {
                    id: "member".into(),
                    admin: false,
                    tools: vec!["*".into()],
                    skills: vec![],
                }],
            )
            .unwrap(),
        )
    }
    async fn manager() -> Arc<McpConnectionManager> {
        let pool = db::open(std::path::Path::new(":memory:")).await.unwrap();
        McpConnectionManager::new(pool, Arc::new(Crypto::ephemeral()), fixture_rbac())
    }

    fn global_connector(auth: AuthKind) -> Connector {
        let now = Timestamp::now();
        Connector {
            key: "discord".into(),
            dispatch_generation: uuid::Uuid::new_v4().to_string(),
            name: "Discord".into(),
            description: None,
            icon: None,
            category: None,
            url: "http://127.0.0.1:1/mcp".into(),
            auth,
            scope: Scope::Global,
            audit: false,
            use_dcr: false,
            client_id: None,
            client_secret_ct: None,
            client_secret_nonce: None,
            authorize_url: None,
            token_url: None,
            registration_url: None,
            scopes: vec![],
            allowed_groups: Vec::new(),
            enabled: true,
            seeded: true,
            created_at: now,
            updated_at: now,
        }
    }

    #[tokio::test]
    async fn same_connector_tool_uses_each_users_own_oauth_connection() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(|request: &wiremock::Request| {
                let bearer = request
                    .headers
                    .get("authorization")
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or("");
                let principal = match bearer {
                    "Bearer alice-secret" => "alice",
                    "Bearer bob-secret" => "bob",
                    other => panic!("unexpected MCP credential: {other}"),
                };
                let body: Value = serde_json::from_slice(&request.body).unwrap();
                let result = match body["method"].as_str() {
                    Some("initialize") => serde_json::json!({
                        "protocolVersion": "2025-06-18",
                        "capabilities": {"tools": {}},
                        "serverInfo": {"name": "fixture", "version": "1"},
                    }),
                    Some("tools/list") => serde_json::json!({
                        "tools": [{"name": "lookup", "description": principal,
                            "inputSchema": {"type": "object"}}]
                    }),
                    Some("tools/call") => serde_json::json!({
                        "content": [{"type": "text", "text": principal}], "isError": false
                    }),
                    Some("notifications/initialized") => return ResponseTemplate::new(202),
                    other => panic!("unexpected MCP method: {other:?}"),
                };
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/json")
                    .set_body_json(serde_json::json!({
                        "jsonrpc": "2.0", "id": body["id"], "result": result
                    }))
            })
            .mount(&server)
            .await;
        let pool = db::open(std::path::Path::new(":memory:")).await.unwrap();
        let crypto = Arc::new(Crypto::ephemeral());
        let mgr = McpConnectionManager::new(pool.clone(), crypto.clone(), fixture_rbac());
        let mut connector = global_connector(AuthKind::OAuth2);
        connector.scope = Scope::PerUser;
        connector.url = server.uri();
        sqlx::query("INSERT INTO mcp_catalog_connectors(key,name,url,auth,scope,enabled,created_at,updated_at,dispatch_generation) VALUES(?,?,?,'oauth2','per_user',1,?,?,?)")
            .bind(&connector.key).bind(&connector.name).bind(&connector.url).bind(connector.created_at.to_string()).bind(connector.updated_at.to_string()).bind(&connector.dispatch_generation).execute(&pool).await.unwrap();
        for (user, secret) in [("alice", "alice-secret"), ("bob", "bob-secret")] {
            sqlx::query("INSERT INTO users (id, email, roles_json, created_at, updated_at) VALUES (?, ?, '[]', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')")
                .bind(user)
                .bind(format!("{user}@example.com"))
                .execute(&pool)
                .await
                .unwrap();
            let sealed = crypto.seal_str(secret).unwrap();
            mcp_connections::upsert_connection(
                &pool,
                mcp_connections::NewConnection {
                    owner: ConnectionOwner::User(user.into()),
                    connector_key: connector.key.clone(),
                    catalog_generation: connector.dispatch_generation.clone(),
                    access_token_ct: sealed.ciphertext,
                    access_token_nonce: sealed.nonce,
                    refresh_token_ct: None,
                    refresh_token_nonce: None,
                    token_expires_at: None,
                    scopes: vec![],
                    dcr_client_id: None,
                    dcr_client_secret_ct: None,
                    dcr_client_secret_nonce: None,
                    token_url: None,
                },
            )
            .await
            .unwrap();
        }
        let alice = mgr
            .ensure_connection(&ConnectionOwner::User("alice".into()), &connector)
            .await
            .unwrap();
        let bob = mgr
            .ensure_connection(&ConnectionOwner::User("bob".into()), &connector)
            .await
            .unwrap();
        assert_eq!(alice[0].schema().function.description, "alice");
        assert_eq!(bob[0].schema().function.description, "bob");
        assert_eq!(
            mgr.ensure_connection(&ConnectionOwner::User("alice".into()), &connector)
                .await
                .unwrap()[0]
                .schema()
                .function
                .description,
            "alice"
        );
        let mut alice_ctx = ToolContext::for_test(pool.clone());
        alice_ctx.principal = ToolContext::test_user("alice");
        let mut bob_ctx = ToolContext::for_test(pool);
        bob_ctx.principal = ToolContext::test_user("bob");
        let alice_result = alice[0]
            .run(alice_ctx, serde_json::json!({}))
            .await
            .unwrap();
        let bob_result = bob[0].run(bob_ctx, serde_json::json!({})).await.unwrap();
        assert!(alice_result.to_string().contains("alice"), "{alice_result}");
        assert!(bob_result.to_string().contains("bob"), "{bob_result}");
        mcp_connections::delete_connection(
            &mgr.db,
            &ConnectionOwner::User("alice".into()),
            &connector.key,
        )
        .await
        .unwrap();
        assert!(
            mgr.ensure_connection(&ConnectionOwner::User("alice".into()), &connector)
                .await
                .is_err()
        );
        assert!(
            mgr.ensure_connection(&ConnectionOwner::User("bob".into()), &connector)
                .await
                .is_ok()
        );
    }

    /// A shared-identity connector whose one tool its server marks
    /// destructive, so it defaults to `ask`. Counts the calls that reach it.
    async fn destructive_connector(
        pool: &Pool,
        calls: Arc<std::sync::atomic::AtomicUsize>,
    ) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(move |request: &wiremock::Request| {
                let body: Value = serde_json::from_slice(&request.body).unwrap();
                let result = match body["method"].as_str() {
                    Some("initialize") => serde_json::json!({
                        "protocolVersion": "2025-06-18",
                        "capabilities": {"tools": {}},
                        "serverInfo": {"name": "fixture", "version": "1"},
                    }),
                    Some("tools/list") => serde_json::json!({
                        "tools": [{"name": "delete_contact", "description": "Delete a contact",
                            "inputSchema": {"type": "object"},
                            "annotations": {"destructiveHint": true, "readOnlyHint": false}}]
                    }),
                    Some("tools/call") => {
                        calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        serde_json::json!({
                            "content": [{"type": "text", "text": "deleted"}], "isError": false
                        })
                    }
                    Some("notifications/initialized") => return ResponseTemplate::new(202),
                    other => panic!("unexpected MCP method: {other:?}"),
                };
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/json")
                    .set_body_json(serde_json::json!({
                        "jsonrpc": "2.0", "id": body["id"], "result": result
                    }))
            })
            .mount(&server)
            .await;
        mcp_catalog::create(
            pool,
            mcp_catalog::ConnectorInput {
                key: "crm".into(),
                name: "CRM".into(),
                description: None,
                icon: None,
                category: None,
                url: server.uri(),
                auth: AuthKind::None,
                scope: Scope::Global,
                audit: false,
                use_dcr: false,
                client_id: None,
                client_secret_ct: None,
                client_secret_nonce: None,
                authorize_url: None,
                token_url: None,
                registration_url: None,
                scopes: vec![],
                allowed_groups: vec![],
            },
        )
        .await
        .unwrap();
        mcp_catalog::set_enabled(pool, "crm", true).await.unwrap();
        for user in ["u", "u1"] {
            sqlx::query("INSERT OR IGNORE INTO users(id,email,roles_json,created_at,updated_at) VALUES (?,?,'[]','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')")
                .bind(user).bind(format!("{user}@example.com")).execute(pool).await.unwrap();
        }
        server
    }

    fn catalog_input(connector: &Connector, url: String) -> mcp_catalog::ConnectorInput {
        mcp_catalog::ConnectorInput {
            key: connector.key.clone(),
            name: connector.name.clone(),
            description: connector.description.clone(),
            icon: connector.icon.clone(),
            category: connector.category.clone(),
            url,
            auth: connector.auth,
            scope: connector.scope,
            audit: connector.audit,
            use_dcr: connector.use_dcr,
            client_id: connector.client_id.clone(),
            client_secret_ct: None,
            client_secret_nonce: None,
            authorize_url: connector.authorize_url.clone(),
            token_url: connector.token_url.clone(),
            registration_url: connector.registration_url.clone(),
            scopes: connector.scopes.clone(),
            allowed_groups: connector.allowed_groups.clone(),
        }
    }

    #[tokio::test]
    async fn catalog_generation_fences_held_shared_tool_after_url_round_trip() {
        let pool = db::open(std::path::Path::new(":memory:")).await.unwrap();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let original_server = destructive_connector(&pool, calls.clone()).await;
        let replacement_server = MockServer::start().await;
        let mgr =
            McpConnectionManager::new(pool.clone(), Arc::new(Crypto::ephemeral()), fixture_rbac());
        let original = mcp_catalog::get(&pool, "crm").await.unwrap().unwrap();
        let held = mgr.ensure_global(&original).await.unwrap().remove(0);
        held.run(ToolContext::for_test(pool.clone()), serde_json::json!({}))
            .await
            .unwrap();
        assert_eq!(calls.swap(0, std::sync::atomic::Ordering::SeqCst), 1);
        for url in [replacement_server.uri(), original_server.uri()] {
            assert!(
                mcp_catalog::update(&pool, "crm", catalog_input(&original, url))
                    .await
                    .unwrap()
            );
        }
        let result = held
            .run(ToolContext::for_test(pool.clone()), serde_json::json!({}))
            .await;
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert!(
            result.is_err(),
            "a restored URL must not revive the original held tool"
        );
        assert!(
            replacement_server
                .received_requests()
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn catalog_generation_never_forwards_owned_old_credentials_to_new_url() {
        let pool = db::open(std::path::Path::new(":memory:")).await.unwrap();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let _original_server = destructive_connector(&pool, calls.clone()).await;
        let replacement_server = MockServer::start().await;
        let mut connector = mcp_catalog::get(&pool, "crm").await.unwrap().unwrap();
        connector.auth = AuthKind::OAuth2;
        connector.scope = Scope::PerUser;
        assert!(
            mcp_catalog::update(
                &pool,
                "crm",
                catalog_input(&connector, connector.url.clone())
            )
            .await
            .unwrap()
        );
        let connector = mcp_catalog::get(&pool, "crm").await.unwrap().unwrap();
        sqlx::query("INSERT INTO users (id,email,roles_json,created_at,updated_at) VALUES ('owner','owner@example.com','[]','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')")
            .execute(&pool).await.unwrap();
        let crypto = Arc::new(Crypto::ephemeral());
        let sealed = crypto.seal_str("owned-generation-fixture-token").unwrap();
        mcp_connections::upsert_connection(
            &pool,
            mcp_connections::NewConnection {
                owner: ConnectionOwner::User("owner".into()),
                connector_key: connector.key.clone(),
                catalog_generation: connector.dispatch_generation.clone(),
                access_token_ct: sealed.ciphertext,
                access_token_nonce: sealed.nonce,
                refresh_token_ct: None,
                refresh_token_nonce: None,
                token_expires_at: None,
                scopes: vec![],
                dcr_client_id: None,
                dcr_client_secret_ct: None,
                dcr_client_secret_nonce: None,
                token_url: None,
            },
        )
        .await
        .unwrap();
        let mgr = McpConnectionManager::new(pool.clone(), crypto, fixture_rbac());
        let owner = ConnectionOwner::User("owner".into());
        let held = mgr
            .ensure_connection(&owner, &connector)
            .await
            .unwrap()
            .remove(0);
        let mut initial_ctx = ToolContext::for_test(pool.clone());
        initial_ctx.principal = ToolContext::test_user("owner");
        held.run(initial_ctx, serde_json::json!({})).await.unwrap();
        assert_eq!(calls.swap(0, std::sync::atomic::Ordering::SeqCst), 1);
        assert!(
            mcp_catalog::update(
                &pool,
                "crm",
                catalog_input(&connector, replacement_server.uri())
            )
            .await
            .unwrap()
        );
        let current = mcp_catalog::get(&pool, "crm").await.unwrap().unwrap();
        mgr.invalidate(&owner, "crm").await;
        let newly_offered = mgr.ensure_connection(&owner, &current).await;
        let mut ctx = ToolContext::for_test(pool);
        ctx.principal = ToolContext::test_user("owner");
        let held_result = held.run(ctx, serde_json::json!({})).await;
        assert!(
            replacement_server
                .received_requests()
                .await
                .unwrap()
                .is_empty(),
            "old credential must not be sent to a replacement endpoint"
        );
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert!(newly_offered.is_err());
        assert!(held_result.is_err());
    }

    #[tokio::test]
    async fn catalog_generation_shared_resume_retains_original_facts_and_live_principal_grants() {
        use aiplane_agents::db::system_principals;
        use aiplane_core::server::principal::{GrantKind, Principal};
        let db = db::open(std::path::Path::new(":memory:")).await.unwrap();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let _server = destructive_connector(&db, calls.clone()).await;
        let mgr =
            McpConnectionManager::new(db.clone(), Arc::new(Crypto::ephemeral()), fixture_rbac());
        let connector = mcp_catalog::get(&db, "crm").await.unwrap().unwrap();
        let tool = mgr.ensure_global(&connector).await.unwrap().remove(0);
        let original_facts = tool.execution_facts();
        let mut ctx = ToolContext::for_test(db.clone());
        ctx.pending_execution_facts = Some(crate::server::tools::PendingExecutionFacts {
            execution_facts: original_facts.clone(),
        });
        tool.run(ctx.clone(), serde_json::json!({})).await.unwrap();
        assert_eq!(calls.swap(0, std::sync::atomic::Ordering::SeqCst), 1);
        let row = system_principals::create(
            &db,
            &system_principals::NewPrincipal {
                name: "catalog-principal",
                display: "Catalog principal",
                description: "",
            },
            "admin",
        )
        .await
        .unwrap()
        .unwrap();
        system_principals::add_grant(&db, &row.id, GrantKind::Connector, "crm", "admin")
            .await
            .unwrap();
        ctx.principal = Principal::System(
            system_principals::load_active(&db, &row.id)
                .await
                .unwrap()
                .unwrap(),
        );
        tool.run(ctx.clone(), serde_json::json!({})).await.unwrap();
        assert_eq!(calls.swap(0, std::sync::atomic::Ordering::SeqCst), 1);
        system_principals::remove_grant(&db, &row.id, GrantKind::Connector, "crm", "admin")
            .await
            .unwrap();
        assert!(tool.run(ctx.clone(), serde_json::json!({})).await.is_err());
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        system_principals::add_grant(&db, &row.id, GrantKind::Connector, "crm", "admin")
            .await
            .unwrap();
        system_principals::disable(&db, &row.id, "admin")
            .await
            .unwrap();
        assert!(tool.run(ctx.clone(), serde_json::json!({})).await.is_err());
        ctx.principal = ToolContext::test_user("u");
        let mut input = catalog_input(&connector, connector.url.clone());
        input.name = "Actual renamed connector".into();
        mcp_catalog::update(&db, "crm", input).await.unwrap();
        tool.run(ctx.clone(), serde_json::json!({})).await.unwrap();
        assert_eq!(calls.swap(0, std::sync::atomic::Ordering::SeqCst), 1);
        mcp_catalog::update(
            &db,
            "crm",
            catalog_input(&connector, "http://127.0.0.1:1/replaced".into()),
        )
        .await
        .unwrap();
        assert!(tool.run(ctx, serde_json::json!({})).await.is_err());
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(tool.execution_facts(), original_facts);
    }

    #[tokio::test]
    async fn catalog_generation_late_handshake_cannot_publish_stale_cache_or_catalog_tools() {
        let db = db::open(std::path::Path::new(":memory:")).await.unwrap();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let _initial = destructive_connector(&db, calls).await;
        let server = MockServer::start().await;
        let listed = Arc::new(tokio::sync::Notify::new());
        let observed = listed.clone();
        Mock::given(method("POST")).respond_with(move |request: &wiremock::Request| {
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            let result = match body["method"].as_str() {
                Some("initialize") => serde_json::json!({"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"late","version":"1"}}),
                Some("notifications/initialized") => return ResponseTemplate::new(202),
                Some("tools/list") => {
                    observed.notify_one();
                    return ResponseTemplate::new(200).insert_header("content-type","application/json")
                        .set_body_json(serde_json::json!({"jsonrpc":"2.0","id":body["id"],"result":{"tools":[{"name":"stale","description":"Stale handshake","inputSchema":{"type":"object"}}]}}))
                        .set_delay(Duration::from_millis(200));
                }
                other => panic!("unexpected method: {other:?}"),
            };
            ResponseTemplate::new(200).insert_header("content-type","application/json")
                .set_body_json(serde_json::json!({"jsonrpc":"2.0","id":body["id"],"result":result}))
        }).mount(&server).await;
        let original = mcp_catalog::get(&db, "crm").await.unwrap().unwrap();
        mcp_catalog::update(&db, "crm", catalog_input(&original, server.uri()))
            .await
            .unwrap();
        let snapshot = mcp_catalog::get(&db, "crm").await.unwrap().unwrap();
        let mgr =
            McpConnectionManager::new(db.clone(), Arc::new(Crypto::ephemeral()), fixture_rbac());
        let worker = mgr.clone();
        let connecting = tokio::spawn(async move { worker.ensure_global(&snapshot).await });
        tokio::time::timeout(Duration::from_secs(2), listed.notified())
            .await
            .unwrap();
        mcp_catalog::update(&db, "crm", catalog_input(&original, original.url.clone()))
            .await
            .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_secs(2), connecting)
                .await
                .unwrap()
                .unwrap()
                .is_err()
        );
        assert!(mgr.cache.lock().await.is_empty());
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM mcp_connector_tools WHERE connector_key='crm'"
            )
            .fetch_one(&db)
            .await
            .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn in_chat_an_ask_tool_is_offered_and_runs_only_after_an_approval() {
        use crate::suspend::{Suspend, extract_suspend};
        use session_core::db::{Decision, SuspensionKind};
        let pool = db::open(std::path::Path::new(":memory:")).await.unwrap();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let _server = destructive_connector(&pool, calls.clone()).await;
        let mgr =
            McpConnectionManager::new(pool.clone(), Arc::new(Crypto::ephemeral()), fixture_rbac());

        let layer = mgr.layer_for_user("u1", &[], true, AskContext::Chat).await;
        assert_eq!(layer.tool_ids(), ["mcp__crm__delete_contact"]);
        assert_eq!(
            layer.mode_of("mcp__crm__delete_contact"),
            Some(ToolMode::Ask)
        );
        let tool = layer.get("mcp__crm__delete_contact").unwrap();
        let ctx = |suspend| ToolContext {
            suspend,
            ..ToolContext::for_test(pool.clone())
        };

        let asked = tool
            .run(ctx(Suspend::Available), serde_json::json!({}))
            .await
            .unwrap();
        let request = extract_suspend(&asked).expect("the call waits for an approval");
        assert_eq!(request.kind, SuspensionKind::Approval);
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert!(
            tool.run(ctx(Suspend::Unavailable), serde_json::json!({}))
                .await
                .is_err(),
            "where nobody can be asked, it never runs"
        );
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);

        let ran = tool
            .run(
                ctx(Suspend::Decided(
                    SuspensionKind::Approval,
                    Decision::AllowOnce,
                )),
                serde_json::json!({}),
            )
            .await
            .unwrap();
        assert!(ran.to_string().contains("deleted"), "{ran}");
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn over_the_api_an_ask_tool_is_hidden_unless_the_token_allows_it() {
        let pool = db::open(std::path::Path::new(":memory:")).await.unwrap();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let _server = destructive_connector(&pool, calls).await;
        let mgr = McpConnectionManager::new(pool, Arc::new(Crypto::ephemeral()), fixture_rbac());
        let layer = mgr
            .layer_for_user("u1", &[], true, AskContext::Api { token_id: "t1" })
            .await;
        assert!(layer.is_empty(), "{:?}", layer.tool_ids());
    }

    #[test]
    fn what_an_ask_tool_becomes_in_each_context() {
        assert_eq!(offer(ToolMode::Off, AskOffer::AskFirst), None);
        assert_eq!(offer(ToolMode::Always, AskOffer::Hide), Some(false));
        assert_eq!(offer(ToolMode::Ask, AskOffer::AskFirst), Some(true));
        assert_eq!(offer(ToolMode::Ask, AskOffer::Unasked), Some(false));
        assert_eq!(offer(ToolMode::Ask, AskOffer::Hide), None);
    }

    #[tokio::test]
    async fn global_oauth2_is_rejected_without_touching_network() {
        // OAuth2 is per-user; a global connector can't use it. ensure_global
        // must reject it up front (defensive — admin save also blocks it).
        let mgr = manager().await;
        let configured = global_connector(AuthKind::OAuth2);
        mcp_catalog::create(&mgr.db, catalog_input(&configured, configured.url.clone()))
            .await
            .unwrap();
        mcp_catalog::set_enabled(&mgr.db, &configured.key, true)
            .await
            .unwrap();
        let configured = mcp_catalog::get(&mgr.db, &configured.key)
            .await
            .unwrap()
            .unwrap();
        let err = match mgr.ensure_global(&configured).await {
            Ok(_) => panic!("expected OAuth2 global to be rejected"),
            Err(e) => e,
        };
        assert!(err.contains("OAuth2"), "{err}");
    }

    #[tokio::test]
    async fn global_static_bearer_without_token_errors() {
        // StaticBearer global connector needs its shared token on the row.
        let mgr = manager().await;
        let configured = global_connector(AuthKind::StaticBearer);
        mcp_catalog::create(&mgr.db, catalog_input(&configured, configured.url.clone()))
            .await
            .unwrap();
        mcp_catalog::set_enabled(&mgr.db, &configured.key, true)
            .await
            .unwrap();
        let configured = mcp_catalog::get(&mgr.db, &configured.key)
            .await
            .unwrap()
            .unwrap();
        let err = match mgr.ensure_global(&configured).await {
            Ok(_) => panic!("expected static_bearer-without-token to error"),
            Err(e) => e,
        };
        assert!(err.contains("bearer token"), "{err}");
    }

    #[tokio::test]
    async fn role_allows_gates_on_allowed_groups() {
        let mgr = manager().await;
        let mut c = global_connector(AuthKind::None);
        assert!(
            mgr.role_allows(&c, &[], false),
            "empty allowed_groups → everyone"
        );
        c.allowed_groups = vec!["staff".into()];
        assert!(!mgr.role_allows(&c, &["user".into()], false));
        assert!(mgr.role_allows(&c, &["user".into(), "staff".into()], false));
        // Admins bypass the restriction, like pools + RAG.
        assert!(mgr.role_allows(&c, &["user".into()], true));
    }

    #[tokio::test]
    async fn audited_tool_records_ok_and_error() {
        let pool = db::open(std::path::Path::new(":memory:")).await.unwrap();
        let ctx = |db: Pool| ToolContext {
            token_id: None,
            principal: aiplane_core::server::principal::Principal::User {
                id: "u".into(),
                roles: vec![],
            },
            pool_access: aiplane_core::server::upstreams::PoolAccess::all(),
            db,
            s3: None,
            assistant_turn_id: None,
            session_id: Some("sess".into()),
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
            suspend: Default::default(),
            pending_actions: None,
            pending_execution_facts: None,
            granted_tools: None,
            agent: None,
            personal_dot: None,
            personal_execution: None,
            call_id: None,
        };
        let audited = AuditedTool {
            inner: Arc::new(Echo) as Arc<dyn Tool>,
            connector_key: "discord".into(),
            db: pool.clone(),
            sensitive: false,
        };
        // Success path (Echo returns the message).
        audited
            .run(ctx(pool.clone()), serde_json::json!({"message": "hi"}))
            .await
            .unwrap();
        // Error path (Echo rejects a missing message) — still audited.
        let _ = audited.run(ctx(pool.clone()), serde_json::json!({})).await;

        let ev = mcp_audit::recent(&pool, 10).await.unwrap();
        assert_eq!(ev.len(), 2, "both calls recorded");
        assert!(ev.iter().any(|e| e.outcome == "ok"));
        assert!(ev.iter().any(|e| e.outcome == "error" && e.error.is_some()));
        assert!(ev.iter().all(|e| e.connector_key == "discord"
            && e.user_id == "u"
            && e.tool_id == "company_echo"
            && e.session_id.as_deref() == Some("sess")));
    }

    #[tokio::test]
    async fn a_sensitive_call_is_audited_without_its_arguments() {
        let pool = db::open(std::path::Path::new(":memory:")).await.unwrap();
        let mut layer = UserMcpLayer::default();
        layer.tools.insert("company_echo".into(), Arc::new(Echo));
        layer.audited.insert(
            "company_echo".into(),
            (Arc::new(Echo) as Arc<dyn Tool>, "erp".into(), pool.clone()),
        );
        let tool = layer.get_with_sensitive_args("company_echo").unwrap();
        assert!(tool.sensitive_args(), "the runner never logs them either");
        tool.run(
            ToolContext::for_test(pool.clone()),
            serde_json::json!({"message": "code 481516"}),
        )
        .await
        .unwrap();

        let ev = mcp_audit::recent(&pool, 10).await.unwrap();
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].arguments, Some(redacted_arguments().to_string()));
        assert!(!format!("{ev:?}").contains("481516"));

        let plain = layer.get("company_echo").unwrap();
        assert!(!plain.sensitive_args());
        assert!(
            UserMcpLayer::default()
                .get_with_sensitive_args("company_echo")
                .is_none()
        );
    }

    #[test]
    fn global_cache_key_is_user_independent() {
        let connector = global_connector(AuthKind::None);
        let shared = McpConnectionManager::global_cache_key(&connector);
        assert_eq!(shared, McpConnectionManager::global_cache_key(&connector));
        let owned = CacheKey::Owned {
            identity: ConnectionIdentity {
                owner: ConnectionOwner::User("u1".into()),
                connector_key: connector.key.clone(),
                connection_id: "connection".into(),
                generation: "credential".into(),
            },
            catalog_generation: connector.dispatch_generation.clone(),
        };
        assert_ne!(shared, owned);
        let mut changed = connector.clone();
        changed.dispatch_generation = uuid::Uuid::new_v4().to_string();
        assert_ne!(shared, McpConnectionManager::global_cache_key(&changed));
    }

    #[test]
    fn default_mode_only_gates_destructive_writes() {
        // read-only → always; destructive write → ask; everything else → always.
        assert_eq!(default_mode(true, false), ToolMode::Always);
        assert_eq!(default_mode(true, true), ToolMode::Always); // read-only wins
        assert_eq!(default_mode(false, true), ToolMode::Ask);
        assert_eq!(default_mode(false, false), ToolMode::Always); // un-annotated → usable
    }

    /// A fake MCP-bridged tool whose id is namespaced `mcp__demo__echo`, so its
    /// connector toggle key collapses to `mcp__demo` (like a real connector).
    struct FakeMcpTool;
    impl crate::server::tools::Tool for FakeMcpTool {
        fn id(&self) -> &str {
            "mcp__demo__echo"
        }
        fn schema(&self) -> shared::api::ToolDef {
            shared::api::ToolDef::function(
                "mcp__demo__echo",
                "demo",
                serde_json::json!({"type": "object"}),
            )
        }
        fn run<'a>(
            &'a self,
            _ctx: crate::server::tools::ToolContext,
            _args: serde_json::Value,
        ) -> crate::server::tools::ToolFuture<'a> {
            Box::pin(async { Ok(serde_json::json!({})) })
        }
    }

    #[test]
    fn enabled_tool_ids_gates_per_user_mcp_by_session_overlay() {
        // The core of "select an integration → its tools become available":
        // a connected connector's tools are exposed ONLY once its toggle key is
        // enabled in the conversation overlay.
        use std::collections::HashSet;
        let mut layer = UserMcpLayer::default();
        layer.insert_for_test(Arc::new(FakeMcpTool), "demo", ToolMode::Always);
        layer.insert_for_test(Arc::new(Echo), "demo", ToolMode::Always);

        // Nothing enabled → connector tools stay hidden (progressive disclosure).
        assert!(layer.enabled_tool_ids(&HashSet::new()).is_empty());

        // Enabling `mcp__demo` (what the composer writes when you pick the
        // integration) surfaces the connector's tool — and only it.
        let on: HashSet<String> = ["mcp__demo".to_string()].into_iter().collect();
        let ids = layer.enabled_tool_ids(&on);
        assert!(ids.contains(&"mcp__demo__echo".to_string()), "{ids:?}");
        assert!(!ids.contains(&"company_echo".to_string()), "{ids:?}");

        // A non-MCP tool keys on its own id, so `mcp__demo` doesn't pull it in.
        let echo_on: HashSet<String> = ["company_echo".to_string()].into_iter().collect();
        assert_eq!(
            layer.enabled_tool_ids(&echo_on),
            vec!["company_echo".to_string()]
        );
    }

    #[test]
    fn composite_unions_and_registry_wins_on_collision() {
        let registry = ToolRegistry::new().with(Echo); // id "company_echo"
        let mut layer = UserMcpLayer::default();
        // A distinct overlay tool + one that collides with the registry id.
        layer.insert_for_test(Arc::new(Echo), "gmail", ToolMode::Always); // collides: company_echo
        let composite = CompositeToolSource::new(&registry, &layer);

        // Registry wins on a colliding id (a connector can't shadow a built-in).
        assert!(composite.contains("company_echo"));
        // defs_for de-dups: the registry def is used, the overlay's collision
        // is dropped (registry provides it).
        let defs = composite.defs_for(&["company_echo".into()]);
        assert_eq!(defs.len(), 1);
        // get resolves registry first.
        assert!(ToolSource::get(&composite, "company_echo").is_some());
        // ids() includes both sources' ids.
        assert!(composite.ids().iter().any(|i| i == "company_echo"));
    }

    #[test]
    fn user_layer_tool_ids_reflect_inserts() {
        let mut layer = UserMcpLayer::default();
        assert!(layer.is_empty());
        layer.insert_for_test(Arc::new(Echo), "gmail", ToolMode::Always);
        assert!(!layer.is_empty());
        assert_eq!(layer.tool_ids(), vec!["company_echo".to_string()]);
        assert_eq!(layer.mode_of("company_echo"), Some(ToolMode::Always));
        assert_eq!(layer.connector_of("company_echo"), Some("gmail"));
    }

    #[tokio::test]
    async fn access_token_decrypt_failure_marks_connection_needing_reauth() {
        // Regression: a stored token sealed under a different key can't be
        // decrypted and won't self-heal (e.g. the encryption key changed). The
        // connection must flip from `connected` to `reauth` so it drops out of
        // `connected_keys`, stops silently re-failing (and log-spamming) every
        // turn, and the store UI asks for a new sign-in — the only thing that
        // actually fixes it — instead of showing a stale green "connected".
        for auth in [AuthKind::OAuth2, AuthKind::StaticBearer] {
            let mgr = manager().await;
            let server = MockServer::start().await;
            // FK: mcp_connections User ownership references users(id).
            sqlx::query(
                "INSERT INTO users (id, email, roles_json, created_at, updated_at) \
             VALUES ('u1','u@e','[]','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')",
            )
            .execute(&mgr.db)
            .await
            .unwrap();
            // Seal the token under a different ephemeral key than the manager's, so
            // the manager can't open it — exactly the production key-drift scenario.
            let sealed = Crypto::ephemeral().seal_str("stale-token").unwrap();
            let mut connector = global_connector(auth);
            connector.url = server.uri();
            connector.scope = Scope::PerUser;
            mcp_catalog::create(&mgr.db, catalog_input(&connector, connector.url.clone()))
                .await
                .unwrap();
            mcp_catalog::set_enabled(&mgr.db, &connector.key, true)
                .await
                .unwrap();
            let connector = mcp_catalog::get(&mgr.db, &connector.key)
                .await
                .unwrap()
                .unwrap();
            mcp_connections::upsert_connection(
                &mgr.db,
                mcp_connections::NewConnection {
                    owner: ConnectionOwner::User("u1".into()),
                    connector_key: "discord".into(),
                    catalog_generation: connector.dispatch_generation.clone(),
                    access_token_ct: sealed.ciphertext,
                    access_token_nonce: sealed.nonce,
                    refresh_token_ct: None,
                    refresh_token_nonce: None,
                    token_expires_at: None, // no expiry → fresh_enough → decrypt path
                    scopes: vec![],
                    dcr_client_id: None,
                    dcr_client_secret_ct: None,
                    dcr_client_secret_nonce: None,
                    token_url: None,
                },
            )
            .await
            .unwrap();
            let conn = mcp_connections::get_connection(
                &mgr.db,
                &ConnectionOwner::User("u1".into()),
                "discord",
            )
            .await
            .unwrap()
            .expect("connection was just inserted");
            assert_eq!(conn.status, "connected", "starts life connected");

            let err = mgr
                .access_token(&ConnectionOwner::User("u1".into()), &connector, &conn)
                .await
                .expect_err("decrypt under the wrong key must fail");
            assert!(
                err.needs_reauth,
                "an unreadable credential is only fixable by authorizing again"
            );

            let after = mcp_connections::get_connection(
                &mgr.db,
                &ConnectionOwner::User("u1".into()),
                "discord",
            )
            .await
            .unwrap()
            .expect("connection still present");
            assert_eq!(
                after.status,
                mcp_connections::STATUS_REAUTH,
                "a non-self-healing decrypt failure must ask the user to reconnect"
            );
            assert!(
                after.last_error.unwrap_or_default().contains("decrypt"),
                "last_error should record the decrypt failure"
            );
            assert!(
                mgr.ensure_connection(&ConnectionOwner::User("u1".into()), &connector)
                    .await
                    .is_err()
            );
            assert!(
                mcp_connections::connected_keys(&mgr.db, &ConnectionOwner::User("u1".into()))
                    .await
                    .unwrap()
                    .is_empty()
            );
            assert!(server.received_requests().await.unwrap().is_empty());
        }
    }
}
