// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Authentication primitives shared across the server: the OIDC client
//! (framework-neutral) and gateway token minting/hashing helpers.
//!
//! Session cookies + bearer middleware are framework-specific and live in
//! `crate::rama_server::session` + `aiplane_runtime::rama_server::auth`.

pub mod jwks;
pub mod mcp_oauth;
pub mod oidc;
pub mod pending;
pub mod token;

/// The caller resolved from a request's bearer token. Threaded into handlers
/// that need to know who's calling — currently the proxy path (in-handler
/// `require_bearer` in `rama_server::auth`).
///
/// A `gwk_` token resolves to a [`Principal::User`], a `gws_` token to a
/// [`Principal::System`]; see `docs/agents.md` → "Principals".
#[derive(Debug, Clone)]
pub struct UserCtx {
    pub priority: Option<crate::server::priority::Priority>,
    pub principal: crate::server::principal::Principal,
    /// Resolved during auth: the user's email, or a system principal's name.
    /// Denormalised onto usage rows so the metrics page needs no join and
    /// survives a deletion. Empty when unknown.
    pub user_email: String,
    pub token_id: String,
    /// The token's display name, resolved during auth. Denormalised onto
    /// usage rows for the per-token breakdown.
    pub token_name: String,
    /// The token's model allowlist, resolved during auth: `None` when the
    /// token is unrestricted (the default), otherwise the exact set it may
    /// use.
    ///
    /// Read here rather than at each handler on purpose. It is a property of
    /// the credential, so it belongs with the rest of what authenticating
    /// resolved — and a database error while reading it then fails the
    /// request like any other auth-path failure, instead of quietly handing a
    /// restricted token full access the way a fallible per-handler read would.
    pub allowed_models: Option<std::sync::Arc<std::collections::HashSet<String>>>,
    /// The token's master "tool use" switch. When `false` the request
    /// path injects no gateway tools at all (pure passthrough), so the
    /// per-capability `token_tool_prefs` never come into play. Default
    /// for every token is off — see `RamaState::allowed_tools_for_token`.
    pub tools_enabled: bool,
}
