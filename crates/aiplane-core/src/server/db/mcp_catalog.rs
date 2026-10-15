// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The admin-managed catalog of connectable MCP servers (the "connector
//! store"). Rows live in `mcp_catalog_connectors` (migration 0023). A
//! built-in default set is seeded at boot — all disabled — so the admin only
//! has to flip a switch (and, for connectors that need a deployment-specific
//! OAuth client, paste its credentials).
//!
//! This layer is crypto-agnostic: the client secret is stored as an opaque
//! AES-GCM `(nonce, ciphertext)` pair the caller (which holds [`crate::server::crypto::Crypto`])
//! produces and consumes. We never see plaintext here.

use jiff::Timestamp;
use sqlx::Row;
use sqlx::sqlite::SqliteRow;

use crate::server::db::{DbError, Pool, WriteTx};

/// OAuth/auth scheme a connector uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthKind {
    /// MCP-native OAuth 2.1 (the common case).
    OAuth2,
    /// No auth — an open MCP server.
    None,
    /// A single static bearer token (stored encrypted in `client_secret`).
    StaticBearer,
}

impl AuthKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AuthKind::OAuth2 => "oauth2",
            AuthKind::None => "none",
            AuthKind::StaticBearer => "static_bearer",
        }
    }
    pub fn parse(s: &str) -> AuthKind {
        match s {
            "none" => AuthKind::None,
            "static_bearer" => AuthKind::StaticBearer,
            _ => AuthKind::OAuth2,
        }
    }
}

/// Whether a connector is connected once for the whole gateway (a shared
/// identity like a Discord bot) or by each user individually (their own OAuth
/// account or token).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Each user connects their own account/token; connection + credential
    /// live per-user in `user_mcp_connections`.
    PerUser,
    /// One shared connection for everyone RBAC allows; no per-user row. Any
    /// secret lives on the connector row (`client_secret_ct`).
    Global,
    /// One shared connection like `Global`, but invisible to every person:
    /// only system principals granted it can use it (`docs/agents.md` → "Principals").
    /// This is how an owner wires their own ERP or ticket system into an
    /// agent without exposing it to employees. `allowed_groups` here decides
    /// who may *grant* it, never who may use it.
    Agent,
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::PerUser => "per_user",
            Scope::Global => "global",
            Scope::Agent => "agent",
        }
    }
    pub fn parse(s: &str) -> Scope {
        match s {
            "global" => Scope::Global,
            "agent" => Scope::Agent,
            _ => Scope::PerUser,
        }
    }
}

/// A catalog connector row.
#[derive(Debug, Clone)]
pub struct Connector {
    pub key: String,
    pub dispatch_generation: String,
    pub name: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub category: Option<String>,
    pub url: String,
    pub auth: AuthKind,
    pub scope: Scope,
    /// When true, every one of this connector's tool calls is recorded to
    /// `mcp_tool_audit` (who ran what, when, outcome). Admin opt-in per
    /// connector — off by default to keep low-risk connectors quiet.
    pub audit: bool,
    pub use_dcr: bool,
    pub client_id: Option<String>,
    /// Encrypted client secret (`nonce`, `ciphertext`) — `None` when unset.
    /// For a `Global` + `StaticBearer` connector this holds the shared bearer
    /// token; for `OAuth2` it holds the OAuth client secret.
    pub client_secret_ct: Option<Vec<u8>>,
    pub client_secret_nonce: Option<Vec<u8>>,
    pub authorize_url: Option<String>,
    pub token_url: Option<String>,
    pub registration_url: Option<String>,
    pub scopes: Vec<String>,
    /// Gateway-group names allowed to see + connect this connector. Empty =
    /// unrestricted (everyone). Matched exactly like a pool's `allowed_groups`
    /// (admins bypass). Replaced the former single-valued `required_role`.
    pub allowed_groups: Vec<String>,
    pub enabled: bool,
    pub seeded: bool,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl Connector {
    /// Whether deployment-specific configuration still has to be supplied
    /// before this connector can be enabled. Two cases:
    /// - no server URL yet (e.g. a self-hosted connector whose endpoint is
    ///   deployment-specific, so it can't be seeded), or
    /// - OAuth2 without DCR and no client_id (a manual OAuth client is needed).
    pub fn needs_setup(&self) -> bool {
        self.url.trim().is_empty()
            || (self.auth == AuthKind::OAuth2 && !self.use_dcr && self.client_id.is_none())
    }

    /// A shared, connect-once connector every allowed person sees (vs. one
    /// each user connects themselves, or one reserved for agents).
    pub fn is_global(&self) -> bool {
        self.scope == Scope::Global
    }

    /// Each person connects their own account.
    pub fn is_per_user(&self) -> bool {
        self.scope == Scope::PerUser
    }

    /// Reserved for system principals; no person ever sees it.
    pub fn is_agent(&self) -> bool {
        self.scope == Scope::Agent
    }

    /// One gateway identity rather than a person's — the only connectors a
    /// system principal can use.
    pub fn has_shared_identity(&self) -> bool {
        matches!(self.scope, Scope::Global | Scope::Agent)
    }

    /// Whether a *person* holding `role_ids` (with `is_admin`) may see +
    /// connect this connector. Mirrors `PoolAccess::allows` /
    /// `Resolver::resource_allowed` so all three resource types gate
    /// identically: empty `allowed_groups` = everyone, admins bypass,
    /// otherwise the caller must hold a listed group. An `agent` connector is
    /// never allowed to a person, admin or not.
    pub fn allows(&self, role_ids: &[String], is_admin: bool) -> bool {
        if self.is_agent() {
            return false;
        }
        self.group_rule(role_ids, is_admin)
    }

    /// Whether an agent manager holding `role_ids` may grant this connector to
    /// a system principal: the same group rule [`Self::allows`] applies to
    /// people, which for an `agent` connector reads as "who may hand it out".
    pub fn grantable_by(&self, role_ids: &[String], is_admin: bool) -> bool {
        self.group_rule(role_ids, is_admin)
    }

    fn group_rule(&self, role_ids: &[String], is_admin: bool) -> bool {
        if is_admin || self.allowed_groups.is_empty() {
            return true;
        }
        self.allowed_groups
            .iter()
            .any(|g| role_ids.iter().any(|r| r == g))
    }
}

/// Fields an admin supplies when creating or editing a connector. The client
/// secret is passed pre-encrypted (or `None` to leave unchanged on update /
/// unset on create).
pub struct ConnectorInput {
    pub key: String,
    pub name: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub category: Option<String>,
    pub url: String,
    pub auth: AuthKind,
    pub scope: Scope,
    pub audit: bool,
    pub use_dcr: bool,
    pub client_id: Option<String>,
    pub client_secret_ct: Option<Vec<u8>>,
    pub client_secret_nonce: Option<Vec<u8>>,
    pub authorize_url: Option<String>,
    pub token_url: Option<String>,
    pub registration_url: Option<String>,
    pub scopes: Vec<String>,
    pub allowed_groups: Vec<String>,
}

fn map_row(row: &SqliteRow) -> Result<Connector, DbError> {
    let scopes_json: String = row.try_get("scopes_json")?;
    let scopes: Vec<String> = serde_json::from_str(&scopes_json).unwrap_or_default();
    Ok(Connector {
        key: row.try_get("key")?,
        dispatch_generation: row.try_get("dispatch_generation")?,
        name: row.try_get("name")?,
        description: row.try_get("description")?,
        icon: row.try_get("icon")?,
        category: row.try_get("category")?,
        url: row.try_get("url")?,
        auth: AuthKind::parse(&row.try_get::<String, _>("auth")?),
        scope: Scope::parse(&row.try_get::<String, _>("scope")?),
        audit: row.try_get::<i64, _>("audit")? != 0,
        use_dcr: row.try_get::<i64, _>("use_dcr")? != 0,
        client_id: row.try_get("client_id")?,
        client_secret_ct: row.try_get("client_secret_ct")?,
        client_secret_nonce: row.try_get("client_secret_nonce")?,
        authorize_url: row.try_get("authorize_url")?,
        token_url: row.try_get("token_url")?,
        registration_url: row.try_get("registration_url")?,
        scopes,
        allowed_groups: {
            let json: String = row.try_get("allowed_groups")?;
            serde_json::from_str(&json).unwrap_or_default()
        },
        enabled: row.try_get::<i64, _>("enabled")? != 0,
        seeded: row.try_get::<i64, _>("seeded")? != 0,
        created_at: super::parse_ts(row.try_get("created_at")?, "created_at")?,
        updated_at: super::parse_ts(row.try_get("updated_at")?, "updated_at")?,
    })
}

const COLS: &str = "key, dispatch_generation, name, description, icon, category, url, auth, scope, audit, use_dcr, \
     client_id, client_secret_ct, client_secret_nonce, authorize_url, token_url, \
     registration_url, scopes_json, allowed_groups, enabled, seeded, created_at, updated_at";

/// Every connector, alphabetical by display name (admin view).
pub async fn list_all(pool: &Pool) -> Result<Vec<Connector>, DbError> {
    let sql = format!("SELECT {COLS} FROM mcp_catalog_connectors ORDER BY name ASC, key ASC");
    let rows = sqlx::query(&sql).fetch_all(pool).await?;
    rows.iter().map(map_row).collect()
}

/// Only enabled connectors (the user-facing store shows these).
pub async fn list_enabled(pool: &Pool) -> Result<Vec<Connector>, DbError> {
    list_enabled_with(pool).await
}

pub async fn list_enabled_in(tx: &mut WriteTx) -> Result<Vec<Connector>, DbError> {
    list_enabled_with(&mut **tx).await
}

async fn list_enabled_with<'e, E>(executor: E) -> Result<Vec<Connector>, DbError>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let sql = format!(
        "SELECT {COLS} FROM mcp_catalog_connectors WHERE enabled = 1 ORDER BY name ASC, key ASC"
    );
    let rows = sqlx::query(&sql).fetch_all(executor).await?;
    rows.iter().map(map_row).collect()
}

/// One connector by key.
pub async fn get(pool: &Pool, key: &str) -> Result<Option<Connector>, DbError> {
    let sql = format!("SELECT {COLS} FROM mcp_catalog_connectors WHERE key = ?");
    let row = sqlx::query(&sql).bind(key).fetch_optional(pool).await?;
    row.as_ref().map(map_row).transpose()
}

pub async fn get_in(tx: &mut super::WriteTx, key: &str) -> Result<Option<Connector>, DbError> {
    let row = sqlx::query(&format!(
        "SELECT {COLS} FROM mcp_catalog_connectors WHERE key = ?"
    ))
    .bind(key)
    .fetch_optional(&mut **tx)
    .await?;
    row.as_ref().map(map_row).transpose()
}

/// Insert a new admin-created connector (enabled = 0, seeded = 0).
pub async fn create(pool: &Pool, input: ConnectorInput) -> Result<(), DbError> {
    validate_secret(&input)?;
    let now = Timestamp::now().to_string();
    let scopes_json = serde_json::to_string(&input.scopes).unwrap_or_else(|_| "[]".into());
    let allowed_groups_json =
        serde_json::to_string(&input.allowed_groups).unwrap_or_else(|_| "[]".into());
    sqlx::query(
        r#"INSERT INTO mcp_catalog_connectors
              (key, name, description, icon, category, url, auth, scope, audit, use_dcr,
               client_id, client_secret_ct, client_secret_nonce, authorize_url,
               token_url, registration_url, scopes_json, allowed_groups,
               enabled, seeded, created_at, updated_at, dispatch_generation)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, 0, ?, ?, ?)"#,
    )
    .bind(&input.key)
    .bind(&input.name)
    .bind(&input.description)
    .bind(&input.icon)
    .bind(&input.category)
    .bind(&input.url)
    .bind(input.auth.as_str())
    .bind(input.scope.as_str())
    .bind(input.audit as i64)
    .bind(input.use_dcr as i64)
    .bind(&input.client_id)
    .bind(&input.client_secret_ct)
    .bind(&input.client_secret_nonce)
    .bind(&input.authorize_url)
    .bind(&input.token_url)
    .bind(&input.registration_url)
    .bind(&scopes_json)
    .bind(&allowed_groups_json)
    .bind(&now)
    .bind(&now)
    .bind(uuid::Uuid::new_v4().to_string())
    .execute(pool)
    .await?;
    Ok(())
}

/// Update an existing connector's editable fields. When `client_secret_ct` is
/// `None` the stored secret is left untouched (so an edit that doesn't retype
/// the secret keeps it); pass `Some(empty-marker)` semantics are not used here.
fn canonical_scopes(scopes: &[String]) -> Vec<&str> {
    let mut scopes: Vec<_> = scopes.iter().map(String::as_str).collect();
    scopes.sort_unstable();
    scopes.dedup();
    scopes
}

fn dispatch_changed(current: &Connector, input: &ConnectorInput) -> bool {
    if current.url != input.url || current.auth != input.auth || current.scope != input.scope {
        return true;
    }
    let secret_changed = current.client_secret_ct != input.client_secret_ct
        || current.client_secret_nonce != input.client_secret_nonce;
    match input.auth {
        AuthKind::None => false,
        AuthKind::StaticBearer => input.scope != Scope::PerUser && secret_changed,
        AuthKind::OAuth2 => {
            secret_changed
                || current.use_dcr != input.use_dcr
                || current.client_id != input.client_id
                || current.authorize_url != input.authorize_url
                || current.token_url != input.token_url
                || current.registration_url != input.registration_url
                || canonical_scopes(&current.scopes) != canonical_scopes(&input.scopes)
        }
    }
}

fn validate_secret(input: &ConnectorInput) -> Result<(), DbError> {
    if input.client_secret_ct.is_some() != input.client_secret_nonce.is_some() {
        return Err(DbError::Decode {
            column: "client_secret",
            source: anyhow::anyhow!("client secret requires both ciphertext and nonce"),
        });
    }
    Ok(())
}

pub async fn dispatch_matches_in(
    tx: &mut WriteTx,
    key: &str,
    expected_generation: &str,
) -> Result<bool, DbError> {
    Ok(get_in(tx, key)
        .await?
        .is_some_and(|c| c.dispatch_generation == expected_generation))
}

pub async fn update(pool: &Pool, key: &str, mut input: ConnectorInput) -> Result<bool, DbError> {
    validate_secret(&input)?;
    let mut tx = WriteTx::begin(pool).await?;
    let Some(current) = get_in(&mut tx, key).await? else {
        tx.commit().await?;
        return Ok(false);
    };
    if input.client_secret_ct.is_none() {
        input.client_secret_ct = current.client_secret_ct.clone();
        input.client_secret_nonce = current.client_secret_nonce.clone();
    }
    let generation = if dispatch_changed(&current, &input) {
        uuid::Uuid::new_v4().to_string()
    } else {
        current.dispatch_generation
    };
    let affected = sqlx::query("UPDATE mcp_catalog_connectors SET name=?,description=?,icon=?,category=?,url=?,auth=?,scope=?,audit=?,use_dcr=?,client_id=?,client_secret_ct=?,client_secret_nonce=?,authorize_url=?,token_url=?,registration_url=?,scopes_json=?,allowed_groups=?,updated_at=?,dispatch_generation=? WHERE key=?")
        .bind(&input.name).bind(&input.description).bind(&input.icon).bind(&input.category).bind(&input.url)
        .bind(input.auth.as_str()).bind(input.scope.as_str()).bind(input.audit as i64).bind(input.use_dcr as i64)
        .bind(&input.client_id).bind(&input.client_secret_ct).bind(&input.client_secret_nonce)
        .bind(&input.authorize_url).bind(&input.token_url).bind(&input.registration_url)
        .bind(serde_json::to_string(&input.scopes).expect("string vector serializes"))
        .bind(serde_json::to_string(&input.allowed_groups).expect("string vector serializes"))
        .bind(Timestamp::now().to_string()).bind(generation).bind(key).execute(&mut *tx).await?.rows_affected();
    tx.commit().await?;
    Ok(affected == 1)
}

/// Flip a connector on/off.
pub async fn set_enabled(pool: &Pool, key: &str, enabled: bool) -> Result<bool, DbError> {
    let affected =
        sqlx::query("UPDATE mcp_catalog_connectors SET enabled = ?, updated_at = ? WHERE key = ?")
            .bind(enabled as i64)
            .bind(Timestamp::now().to_string())
            .bind(key)
            .execute(pool)
            .await?
            .rows_affected();
    Ok(affected > 0)
}

/// Delete a connector. (User connections to it cascade only via app logic —
/// they reference the key, not a FK, so a delete leaves orphaned connections
/// that simply stop resolving; the store hides them.)
pub async fn delete(pool: &Pool, key: &str) -> Result<bool, DbError> {
    let affected = sqlx::query("DELETE FROM mcp_catalog_connectors WHERE key = ?")
        .bind(key)
        .execute(pool)
        .await?
        .rows_affected();
    Ok(affected > 0)
}

/// A shipped default connector definition.
pub struct DefaultConnector {
    pub key: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub icon: &'static str,
    pub category: &'static str,
    pub url: &'static str,
    pub auth: AuthKind,
    pub scope: Scope,
    pub audit: bool,
    pub use_dcr: bool,
    pub scopes: &'static [&'static str],
    /// Explicit OAuth endpoints for providers that don't publish RFC 8414
    /// metadata / don't do DCR (e.g. GitHub). `None` → resolved by discovery.
    pub authorize_url: Option<&'static str>,
    pub token_url: Option<&'static str>,
}

/// The built-in connector set seeded at boot, all disabled. URLs point at the
/// public remote MCP servers for Atlassian / GitHub / GitLab. `use_dcr = true`
/// connectors work as soon as an admin enables them; GitHub needs a deployment
/// OAuth client (no DCR) and surfaces as "needs setup" until supplied. Google
/// Workspace is DCR but self-hosted, so it ships with an empty URL the admin
/// fills in (also "needs setup" until then).
pub const DEFAULT_CONNECTORS: &[DefaultConnector] = &[
    DefaultConnector {
        key: "google_workspace",
        scope: Scope::PerUser,
        audit: false,
        name: "Google Workspace",
        description: "Gmail, Calendar, Drive, Docs, Sheets, Slides, Tasks and more — one sign-in via a self-hosted Google Workspace MCP server (GA Google APIs, no developer preview).",
        icon: "google_workspace",
        category: "Google",
        // A self-hosted Google Workspace MCP server (e.g.
        // taylorwilsdon/google_workspace_mcp, streamable-HTTP at `/mcp` — no
        // trailing slash). The endpoint is deployment-specific, so the admin
        // sets the URL; the server is its own OAuth 2.1 / DCR provider (it holds
        // the Google OAuth client), so no client id is configured here. Google's
        // *hosted* MCP endpoints (gmailmcp/calendarmcp/drivemcp.googleapis.com)
        // are gated behind the Workspace Developer Preview Program and
        // intentionally not used — see docs/connectors.md.
        url: "",
        auth: AuthKind::OAuth2,
        use_dcr: true,
        // The server defaults to a base-only login (openid + email) and rejects
        // every tool call with "lack required scopes" unless the OAuth *client*
        // (this gateway) requests the service scopes up front. So we request a
        // sensible "one sign-in" read set + Gmail compose; the admin can trim
        // this on the connector. Changing it requires users to reconnect.
        scopes: &[
            "openid",
            "https://www.googleapis.com/auth/userinfo.email",
            "https://www.googleapis.com/auth/gmail.readonly",
            "https://www.googleapis.com/auth/gmail.compose",
            "https://www.googleapis.com/auth/calendar.readonly",
            "https://www.googleapis.com/auth/calendar.events",
            "https://www.googleapis.com/auth/drive.readonly",
            "https://www.googleapis.com/auth/drive.file",
            "https://www.googleapis.com/auth/documents.readonly",
            "https://www.googleapis.com/auth/spreadsheets.readonly",
            "https://www.googleapis.com/auth/presentations.readonly",
            "https://www.googleapis.com/auth/tasks.readonly",
        ],
        authorize_url: None,
        token_url: None,
    },
    DefaultConnector {
        key: "atlassian",
        scope: Scope::PerUser,
        audit: false,
        name: "Atlassian (Jira & Confluence)",
        description: "Search and work with Jira issues and Confluence pages in the user's Atlassian sites.",
        icon: "atlassian",
        category: "Developer",
        // Streamable-HTTP endpoint (`/v1/mcp`). The `/v1/sse` endpoint speaks
        // the legacy HTTP+SSE transport, which our MCP client doesn't support.
        url: "https://mcp.atlassian.com/v1/mcp",
        auth: AuthKind::OAuth2,
        use_dcr: true,
        scopes: &[],
        authorize_url: None,
        token_url: None,
    },
    DefaultConnector {
        key: "github",
        scope: Scope::PerUser,
        audit: false,
        name: "GitHub",
        description: "Browse repositories, issues, and pull requests in the user's GitHub account.",
        icon: "github",
        category: "Developer",
        url: "https://api.githubcopilot.com/mcp/",
        auth: AuthKind::OAuth2,
        // GitHub's authorization server publishes no RFC 8414 metadata and
        // offers no dynamic client registration, so we pin its OAuth endpoints
        // and require an admin-created GitHub OAuth App (client id/secret).
        use_dcr: false,
        scopes: &[
            "repo",
            "read:org",
            "read:user",
            "user:email",
            "read:project",
        ],
        authorize_url: Some("https://github.com/login/oauth/authorize"),
        token_url: Some("https://github.com/login/oauth/access_token"),
    },
    DefaultConnector {
        key: "kiwi",
        scope: Scope::PerUser,
        audit: false,
        name: "Kiwi.com Flight Search",
        description: "Search one-way and round-trip flights (dates, passengers, cabin class) via Kiwi.com.",
        icon: "kiwi",
        category: "Travel",
        // Public, unauthenticated streamable-HTTP endpoint — no OAuth, no API
        // key. Still opt-in per user (like every other connector) so each
        // user controls whether flight search is exposed to their chats.
        url: "https://mcp.kiwi.com",
        auth: AuthKind::None,
        use_dcr: false,
        scopes: &[],
        authorize_url: None,
        token_url: None,
    },
    DefaultConnector {
        key: "slack",
        scope: Scope::PerUser,
        audit: false,
        name: "Slack",
        description: "Search messages, files, and channels, and send messages in the user's Slack workspace.",
        icon: "slack",
        category: "Productivity",
        // JSON-RPC 2.0 over streamable HTTP; Slack does not support the
        // legacy HTTP+SSE transport for this endpoint.
        url: "https://mcp.slack.com/mcp",
        auth: AuthKind::OAuth2,
        // Slack requires a statically registered app (fixed client id, no
        // dynamic client registration) and only permits directory-published
        // or workspace-internal apps, so an admin must create one manually.
        use_dcr: false,
        scopes: &[
            "search:read.public",
            "search:read.private",
            "channels:read",
            "users:read",
            "users:read.email",
            "chat:write",
        ],
        authorize_url: Some("https://slack.com/oauth/v2_user/authorize"),
        token_url: Some("https://slack.com/api/oauth.v2.user.access"),
    },
    DefaultConnector {
        key: "gitlab",
        scope: Scope::PerUser,
        audit: false,
        name: "GitLab (SaaS / Premium)",
        description: "Work with projects, issues, and merge requests on GitLab.com. GitLab's native MCP server is a Duo feature (Premium/Ultimate); for Community Edition use the self-managed connector below.",
        icon: "gitlab",
        category: "Developer",
        url: "https://gitlab.com/api/v4/mcp",
        auth: AuthKind::OAuth2,
        use_dcr: true,
        scopes: &[],
        authorize_url: None,
        token_url: None,
    },
    DefaultConnector {
        key: "gitlab_selfmanaged",
        scope: Scope::PerUser,
        audit: false,
        name: "GitLab (self-managed / CE)",
        description: "Projects, issues, and merge requests on a self-managed GitLab — including Community Edition. Each user connects with their own GitLab personal access token.",
        icon: "gitlab",
        category: "Developer",
        // GitLab's *native* MCP (`/api/v4/mcp`) is a Duo feature requiring
        // Premium/Ultimate, so CE/Free can't use it. Instead point this at a
        // self-hosted community bridge (e.g. zereight/gitlab-mcp run with
        // STREAMABLE_HTTP=true + REMOTE_AUTHORIZATION=true against the
        // instance's /api/v4) which forwards each request's bearer to GitLab as
        // that user's PAT — so it's a static_bearer connector. The bridge URL is
        // deployment-specific → the admin sets it. See deploy/README.md.
        url: "",
        auth: AuthKind::StaticBearer,
        use_dcr: false,
        scopes: &[],
        authorize_url: None,
        token_url: None,
    },
    DefaultConnector {
        key: "discord",
        // A shared bot, not a per-user account: a Discord bot token
        // authenticates one bot for the whole server, so this is the one seeded
        // *global* connector.
        scope: Scope::Global,
        audit: true,
        name: "Discord",
        description: "Send and read messages (including DMs), and manage channels, threads, and webhooks via a shared Discord bot. Available to everyone the operator's roles allow — no per-user sign-in.",
        icon: "discord",
        category: "Communication",
        // Self-hosted HTTP bridge (SaseQ/discord-mcp, streamable HTTP on :8085
        // → `/mcp`). Deployment-specific, so the admin sets the URL (typically
        // http://discord-mcp:8085/mcp). The bot token lives in the bridge
        // container, reached only over the gateway's private network, so the
        // gateway sends no auth. See deploy/README.md.
        url: "",
        auth: AuthKind::None,
        use_dcr: false,
        scopes: &[],
        authorize_url: None,
        token_url: None,
    },
];

/// Seed the built-in connectors idempotently. Existing rows (matched by key)
/// are never overwritten, so admin edits and the enabled flag survive
/// restarts and upgrades; only missing keys are inserted (disabled). Returns
/// the number of rows newly inserted.
pub async fn seed_defaults(pool: &Pool) -> Result<u64, DbError> {
    let now = Timestamp::now().to_string();
    let mut inserted = 0u64;
    for d in DEFAULT_CONNECTORS {
        let scopes_json = serde_json::to_string(d.scopes).unwrap_or_else(|_| "[]".into());
        let affected = sqlx::query(
            r#"INSERT INTO mcp_catalog_connectors
                  (key, name, description, icon, category, url, auth, scope, audit, use_dcr,
                   authorize_url, token_url, scopes_json, enabled, seeded, created_at, updated_at, dispatch_generation)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, 1, ?, ?, ?)
               ON CONFLICT(key) DO NOTHING"#,
        )
        .bind(d.key)
        .bind(d.name)
        .bind(d.description)
        .bind(d.icon)
        .bind(d.category)
        .bind(d.url)
        .bind(d.auth.as_str())
        .bind(d.scope.as_str())
        .bind(d.audit as i64)
        .bind(d.use_dcr as i64)
        .bind(d.authorize_url)
        .bind(d.token_url)
        .bind(&scopes_json)
        .bind(&now)
        .bind(&now)
        .bind(uuid::Uuid::new_v4().to_string())
        .execute(pool)
        .await?
        .rows_affected();
        inserted += affected;
    }
    if inserted > 0 {
        tracing::info!(inserted, "seeded default MCP connectors (all disabled)");
    }
    Ok(inserted)
}

// ---- cached tool lists ----------------------------------------------------

/// One tool an MCP connector was observed to expose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedTool {
    /// The gateway-side id (`mcp__<server>__<tool>`) — exactly what a grant names.
    pub tool_id: String,
    pub description: String,
}

/// Replace the cached tool list for one connector.
///
/// Called after a successful connection, which is the only moment the list is
/// knowable. A connection that fails leaves the previous rows alone: a server
/// being briefly unreachable should not empty the admin's picker.
pub async fn replace_tools(
    pool: &Pool,
    connector_key: &str,
    expected_generation: &str,
    tools: &[CachedTool],
) -> Result<bool, DbError> {
    let now = Timestamp::now().to_string();
    let mut tx = WriteTx::begin(pool).await?;
    if !dispatch_matches_in(&mut tx, connector_key, expected_generation).await? {
        tx.commit().await?;
        return Ok(false);
    }
    sqlx::query("DELETE FROM mcp_connector_tools WHERE connector_key = ?")
        .bind(connector_key)
        .execute(&mut *tx)
        .await?;
    for tool in tools {
        sqlx::query(
            "INSERT INTO mcp_connector_tools (connector_key, tool_id, description, seen_at)
             VALUES (?, ?, ?, ?)",
        )
        .bind(connector_key)
        .bind(&tool.tool_id)
        .bind(&tool.description)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(true)
}

/// Every connector's cached tools, keyed by connector. Admin surfaces only —
/// nothing on the request path reads this.
pub async fn all_tools(pool: &Pool) -> Result<Vec<(String, CachedTool)>, DbError> {
    let rows = sqlx::query(
        "SELECT connector_key, tool_id, description FROM mcp_connector_tools
         ORDER BY connector_key, tool_id",
    )
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|row| {
            Ok((
                row.try_get("connector_key")?,
                CachedTool {
                    tool_id: row.try_get("tool_id")?,
                    description: row.try_get("description")?,
                },
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::db;

    async fn pool() -> Pool {
        db::open(std::path::Path::new(":memory:")).await.unwrap()
    }

    #[tokio::test]
    async fn seed_is_idempotent_and_disabled() {
        let pool = pool().await;
        let first = seed_defaults(&pool).await.unwrap();
        assert_eq!(first as usize, DEFAULT_CONNECTORS.len());
        // Second run inserts nothing.
        let second = seed_defaults(&pool).await.unwrap();
        assert_eq!(second, 0);
        let all = list_all(&pool).await.unwrap();
        assert_eq!(all.len(), DEFAULT_CONNECTORS.len());
        assert!(all.iter().all(|c| !c.enabled), "seeded rows start disabled");
        // Enabled list is empty until an admin flips one.
        assert!(list_enabled(&pool).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn seed_preserves_admin_enable_state() {
        let pool = pool().await;
        seed_defaults(&pool).await.unwrap();
        // Atlassian is the DCR default (no manual client needed).
        assert!(set_enabled(&pool, "atlassian", true).await.unwrap());
        // Re-seed (simulates a restart) must not reset the flag.
        seed_defaults(&pool).await.unwrap();
        let atl = get(&pool, "atlassian").await.unwrap().unwrap();
        assert!(atl.enabled, "admin enable survives re-seed");
        assert!(atl.use_dcr);
        assert!(!atl.needs_setup(), "DCR connector never needs manual setup");
        // GitHub pins explicit OAuth endpoints + no DCR → needs a client id.
        let gh = get(&pool, "github").await.unwrap().unwrap();
        assert!(!gh.use_dcr);
        assert!(gh.needs_setup());
        assert_eq!(
            gh.authorize_url.as_deref(),
            Some("https://github.com/login/oauth/authorize")
        );
    }

    #[tokio::test]
    async fn google_workspace_needs_setup_until_url_set() {
        let pool = pool().await;
        seed_defaults(&pool).await.unwrap();
        let gw = get(&pool, "google_workspace").await.unwrap().unwrap();
        // DCR connector (the self-hosted server is its own OAuth provider)…
        assert!(gw.use_dcr);
        assert!(gw.client_id.is_none());
        // …but it ships with no URL, so it still needs the admin to point it at
        // their self-hosted Google Workspace MCP server before enabling.
        assert!(gw.url.is_empty());
        assert!(
            gw.needs_setup(),
            "self-hosted connector needs its server URL first"
        );
    }

    #[tokio::test]
    async fn dcr_connector_with_url_is_ready() {
        let pool = pool().await;
        seed_defaults(&pool).await.unwrap();
        // Atlassian is DCR *and* ships a fixed URL → ready to enable as-is.
        let atl = get(&pool, "atlassian").await.unwrap().unwrap();
        assert!(atl.use_dcr);
        assert!(!atl.url.is_empty());
        assert!(!atl.needs_setup());
    }

    #[tokio::test]
    async fn kiwi_needs_no_setup_and_no_auth() {
        let pool = pool().await;
        seed_defaults(&pool).await.unwrap();
        // Public, unauthenticated server with a fixed URL → ready to enable
        // as-is, unlike every OAuth2/static_bearer default.
        let kiwi = get(&pool, "kiwi").await.unwrap().unwrap();
        assert_eq!(kiwi.auth, AuthKind::None);
        assert!(!kiwi.use_dcr);
        assert!(!kiwi.url.is_empty());
        assert!(!kiwi.needs_setup());
    }

    #[tokio::test]
    async fn slack_pins_oauth_endpoints_and_needs_manual_client() {
        let pool = pool().await;
        seed_defaults(&pool).await.unwrap();
        // Like GitHub: no DCR, no RFC 8414 metadata → an admin must supply a
        // client id before the connector can be enabled.
        let slack = get(&pool, "slack").await.unwrap().unwrap();
        assert_eq!(slack.auth, AuthKind::OAuth2);
        assert!(!slack.use_dcr);
        assert!(slack.needs_setup());
        assert_eq!(
            slack.authorize_url.as_deref(),
            Some("https://slack.com/oauth/v2_user/authorize")
        );
        assert_eq!(
            slack.token_url.as_deref(),
            Some("https://slack.com/api/oauth.v2.user.access")
        );
    }

    #[tokio::test]
    async fn gitlab_seeds_both_saas_and_self_managed() {
        let pool = pool().await;
        seed_defaults(&pool).await.unwrap();
        // SaaS/Premium connector is kept (native MCP, OAuth + DCR).
        let saas = get(&pool, "gitlab").await.unwrap().unwrap();
        assert_eq!(saas.auth, AuthKind::OAuth2);
        assert!(saas.use_dcr);
        // CE / self-managed: user-supplied-token bridge, no seeded URL.
        let ce = get(&pool, "gitlab_selfmanaged").await.unwrap().unwrap();
        assert_eq!(ce.auth, AuthKind::StaticBearer);
        assert!(ce.url.is_empty());
        assert!(ce.needs_setup(), "needs the bridge URL before enabling");
    }

    #[tokio::test]
    async fn google_workspace_requests_service_scopes() {
        let pool = pool().await;
        seed_defaults(&pool).await.unwrap();
        let gw = get(&pool, "google_workspace").await.unwrap().unwrap();
        // Must request the service scopes up front: the server otherwise does a
        // base-only login and every tool call fails with "lack required scopes".
        assert!(
            gw.scopes.iter().any(|s| s.contains("gmail.readonly")),
            "google_workspace must seed the gmail.readonly scope"
        );
    }

    #[tokio::test]
    async fn create_update_delete_roundtrip() {
        let pool = pool().await;
        create(
            &pool,
            ConnectorInput {
                key: "custom".into(),
                name: "Custom".into(),
                description: None,
                icon: None,
                category: Some("Other".into()),
                url: "https://example.test/mcp".into(),
                auth: AuthKind::OAuth2,
                scope: Scope::PerUser,
                audit: false,
                use_dcr: true,
                client_id: None,
                client_secret_ct: None,
                client_secret_nonce: None,
                authorize_url: None,
                token_url: None,
                registration_url: None,
                scopes: vec!["a".into(), "b".into()],
                allowed_groups: Vec::new(),
            },
        )
        .await
        .unwrap();
        let c = get(&pool, "custom").await.unwrap().unwrap();
        assert_eq!(c.scopes, vec!["a".to_string(), "b".to_string()]);
        assert!(!c.seeded);

        update(
            &pool,
            "custom",
            ConnectorInput {
                key: "custom".into(),
                name: "Renamed".into(),
                description: Some("now described".into()),
                icon: None,
                category: Some("Other".into()),
                url: "https://example.test/mcp".into(),
                auth: AuthKind::OAuth2,
                scope: Scope::PerUser,
                audit: false,
                use_dcr: true,
                client_id: None,
                client_secret_ct: None,
                client_secret_nonce: None,
                authorize_url: None,
                token_url: None,
                registration_url: None,
                scopes: vec![],
                allowed_groups: Vec::new(),
            },
        )
        .await
        .unwrap();
        let c = get(&pool, "custom").await.unwrap().unwrap();
        assert_eq!(c.name, "Renamed");
        assert!(c.scopes.is_empty());

        assert!(delete(&pool, "custom").await.unwrap());
        assert!(get(&pool, "custom").await.unwrap().is_none());
    }

    #[test]
    fn scope_parse_defaults_to_per_user() {
        assert_eq!(Scope::parse("global"), Scope::Global);
        assert_eq!(Scope::parse("per_user"), Scope::PerUser);
        assert_eq!(Scope::parse("nonsense"), Scope::PerUser);
        assert_eq!(Scope::parse("agent"), Scope::Agent);
        assert_eq!(Scope::Agent.as_str(), "agent");
        assert_eq!(Scope::Global.as_str(), "global");
        assert_eq!(Scope::PerUser.as_str(), "per_user");
    }

    #[tokio::test]
    async fn discord_seeds_as_global_no_auth_needs_setup() {
        let pool = pool().await;
        seed_defaults(&pool).await.unwrap();
        let d = get(&pool, "discord").await.unwrap().unwrap();
        // Shared bot → global, no auth from the gateway (token baked into the
        // sidecar), and ships without a URL so it needs the admin to point it
        // at the self-hosted bridge before enabling.
        assert_eq!(d.scope, Scope::Global);
        assert!(d.is_global());
        assert_eq!(d.auth, AuthKind::None);
        assert!(d.url.is_empty());
        assert!(d.needs_setup());
        // The shared bot ships with audit on by default (accountability for a
        // powerful shared identity); admins can turn it off.
        assert!(d.audit, "discord should default to audited");
    }

    #[tokio::test]
    async fn seeded_account_connectors_are_per_user() {
        let pool = pool().await;
        seed_defaults(&pool).await.unwrap();
        for key in ["google_workspace", "atlassian", "github", "slack", "gitlab"] {
            let c = get(&pool, key).await.unwrap().unwrap();
            assert_eq!(c.scope, Scope::PerUser, "{key} should be per-user");
            assert!(!c.is_global());
        }
    }

    #[tokio::test]
    async fn scope_survives_create_and_update() {
        let pool = pool().await;
        let input = |scope| ConnectorInput {
            key: "g".into(),
            name: "G".into(),
            description: None,
            icon: None,
            category: None,
            url: "http://localhost:8080/mcp".into(),
            auth: AuthKind::None,
            scope,
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
        };
        create(&pool, input(Scope::Global)).await.unwrap();
        assert_eq!(get(&pool, "g").await.unwrap().unwrap().scope, Scope::Global);
        update(&pool, "g", input(Scope::PerUser)).await.unwrap();
        assert_eq!(
            get(&pool, "g").await.unwrap().unwrap().scope,
            Scope::PerUser
        );
        update(&pool, "g", input(Scope::Agent)).await.unwrap();
        let agent = get(&pool, "g").await.unwrap().unwrap();
        assert_eq!(agent.scope, Scope::Agent);
        assert!(agent.is_agent() && agent.has_shared_identity());
        assert!(!agent.is_global() && !agent.is_per_user());
    }

    /// An `agent` connector is closed to every person — open groups and
    /// admin included — while its groups still decide who may grant it.
    #[tokio::test]
    async fn an_agent_connector_is_never_allowed_to_a_person() {
        let pool = pool().await;
        seed_connector(&pool, "erp").await;
        let mut c = get(&pool, "erp").await.unwrap().unwrap();
        c.scope = Scope::Agent;
        assert!(!c.allows(&[], false));
        assert!(!c.allows(&[], true));
        assert!(c.grantable_by(&[], false));
        c.allowed_groups = vec!["owners".into()];
        assert!(!c.allows(&["owners".into()], false));
        assert!(c.grantable_by(&["owners".into()], false));
        assert!(!c.grantable_by(&["others".into()], false));
        assert!(c.grantable_by(&["others".into()], true));
    }

    async fn seed_connector(pool: &Pool, key: &str) {
        create(
            pool,
            ConnectorInput {
                key: key.into(),
                name: key.into(),
                description: None,
                icon: None,
                category: None,
                url: "http://localhost:8080/mcp".into(),
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
                allowed_groups: Vec::new(),
            },
        )
        .await
        .unwrap();
    }

    fn tool(id: &str) -> CachedTool {
        CachedTool {
            tool_id: id.into(),
            description: String::new(),
        }
    }

    #[tokio::test]
    async fn cached_tools_replace_rather_than_accumulate() {
        let pool = crate::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        seed_connector(&pool, "slack").await;
        replace_tools(
            &pool,
            "slack",
            &get(&pool, "slack")
                .await
                .unwrap()
                .unwrap()
                .dispatch_generation,
            &[tool("mcp__slack__post"), tool("mcp__slack__read")],
        )
        .await
        .unwrap();
        // A later connection that sees fewer tools must not leave the old ones
        // behind, or the picker offers grants for tools that no longer exist.
        replace_tools(
            &pool,
            "slack",
            &get(&pool, "slack")
                .await
                .unwrap()
                .unwrap()
                .dispatch_generation,
            &[tool("mcp__slack__post")],
        )
        .await
        .unwrap();
        let cached = all_tools(&pool).await.unwrap();
        assert_eq!(
            cached,
            vec![("slack".to_string(), tool("mcp__slack__post"))]
        );
    }

    #[tokio::test]
    async fn cached_tools_go_with_their_connector() {
        let pool = crate::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        seed_connector(&pool, "slack").await;
        replace_tools(
            &pool,
            "slack",
            &get(&pool, "slack")
                .await
                .unwrap()
                .unwrap()
                .dispatch_generation,
            &[tool("mcp__slack__post")],
        )
        .await
        .unwrap();
        delete(&pool, "slack").await.unwrap();
        assert!(all_tools(&pool).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn connection_scoped_catalog_read_preserves_real_metadata() {
        let pool = pool().await;
        seed_defaults(&pool).await.unwrap();
        set_enabled(&pool, "atlassian", true).await.unwrap();
        let expected = get(&pool, "atlassian").await.unwrap().unwrap();
        let mut tx = db::WriteTx::begin(&pool).await.unwrap();
        let actual = get_in(&mut tx, "atlassian").await.unwrap().unwrap();
        assert_eq!(actual.name, expected.name);
        assert_eq!(actual.enabled, expected.enabled);
        assert_eq!(actual.allowed_groups, expected.allowed_groups);
        assert!(get_in(&mut tx, "missing").await.unwrap().is_none());
        tx.commit().await.unwrap();
    }

    fn dispatch_input(auth: AuthKind, scope: Scope) -> ConnectorInput {
        ConnectorInput {
            key: "dispatch".into(),
            name: "Dispatch".into(),
            description: None,
            icon: None,
            category: None,
            url: "https://provider.test/mcp".into(),
            auth,
            scope,
            audit: false,
            use_dcr: false,
            client_id: Some("client".into()),
            client_secret_ct: Some(vec![1, 2]),
            client_secret_nonce: Some(vec![3, 4]),
            authorize_url: None,
            token_url: None,
            registration_url: None,
            scopes: vec!["read".into(), "write".into()],
            allowed_groups: vec![],
        }
    }

    #[tokio::test]
    async fn dispatch_generation_tracks_effective_configuration_not_management_metadata() {
        let p = pool().await;
        create(&p, dispatch_input(AuthKind::OAuth2, Scope::PerUser))
            .await
            .unwrap();
        let original = get(&p, "dispatch")
            .await
            .unwrap()
            .unwrap()
            .dispatch_generation;
        assert!(!original.is_empty());
        let mut cosmetic = dispatch_input(AuthKind::OAuth2, Scope::PerUser);
        cosmetic.name = "Renamed".into();
        cosmetic.audit = true;
        cosmetic.allowed_groups = vec!["engineering".into()];
        cosmetic.scopes = vec!["write".into(), "read".into(), "read".into()];
        cosmetic.client_secret_ct = None;
        cosmetic.client_secret_nonce = None;
        update(&p, "dispatch", cosmetic).await.unwrap();
        set_enabled(&p, "dispatch", true).await.unwrap();
        let retained = get(&p, "dispatch").await.unwrap().unwrap();
        assert_eq!(retained.dispatch_generation, original);
        assert_eq!(retained.client_secret_ct, Some(vec![1, 2]));
        let mut changed = dispatch_input(AuthKind::OAuth2, Scope::PerUser);
        changed.url = "https://replacement.test/mcp".into();
        update(&p, "dispatch", changed).await.unwrap();
        let replacement = get(&p, "dispatch")
            .await
            .unwrap()
            .unwrap()
            .dispatch_generation;
        assert_ne!(replacement, original);
        update(
            &p,
            "dispatch",
            dispatch_input(AuthKind::OAuth2, Scope::PerUser),
        )
        .await
        .unwrap();
        let restored = get(&p, "dispatch")
            .await
            .unwrap()
            .unwrap()
            .dispatch_generation;
        assert_ne!(restored, original);
        assert_ne!(restored, replacement);
        delete(&p, "dispatch").await.unwrap();
        create(&p, dispatch_input(AuthKind::OAuth2, Scope::PerUser))
            .await
            .unwrap();
        assert_ne!(
            get(&p, "dispatch")
                .await
                .unwrap()
                .unwrap()
                .dispatch_generation,
            restored
        );
    }

    #[tokio::test]
    async fn dispatch_generation_ignores_unused_auth_fields_and_fences_cached_tools() {
        let p = pool().await;
        create(&p, dispatch_input(AuthKind::None, Scope::Global))
            .await
            .unwrap();
        let original = get(&p, "dispatch")
            .await
            .unwrap()
            .unwrap()
            .dispatch_generation;
        let mut unused = dispatch_input(AuthKind::None, Scope::Global);
        unused.use_dcr = true;
        unused.client_id = Some("unused".into());
        unused.token_url = Some("https://unused.test/token".into());
        unused.client_secret_ct = Some(vec![9]);
        unused.client_secret_nonce = Some(vec![8]);
        update(&p, "dispatch", unused).await.unwrap();
        assert_eq!(
            get(&p, "dispatch")
                .await
                .unwrap()
                .unwrap()
                .dispatch_generation,
            original
        );
        assert!(
            replace_tools(&p, "dispatch", &original, &[tool("current")])
                .await
                .unwrap()
        );
        let mut changed = dispatch_input(AuthKind::None, Scope::Global);
        changed.url = "https://replacement.test/mcp".into();
        update(&p, "dispatch", changed).await.unwrap();
        assert!(
            !replace_tools(&p, "dispatch", &original, &[tool("stale")])
                .await
                .unwrap()
        );
        assert_eq!(
            all_tools(&p).await.unwrap(),
            vec![("dispatch".into(), tool("current"))]
        );
        let mut tx = super::super::WriteTx::begin(&p).await.unwrap();
        assert!(
            !dispatch_matches_in(&mut tx, "dispatch", &original)
                .await
                .unwrap()
        );
        tx.commit().await.unwrap();
    }

    #[tokio::test]
    async fn dispatch_generation_rotates_each_effective_oauth_field() {
        let changes: [fn(&mut ConnectorInput); 9] = [
            |i| i.use_dcr = true,
            |i| i.client_id = Some("replacement".into()),
            |i| i.client_secret_ct = Some(vec![9]),
            |i| i.client_secret_nonce = Some(vec![9]),
            |i| i.authorize_url = Some("https://provider.test/authorize".into()),
            |i| i.token_url = Some("https://provider.test/token".into()),
            |i| i.registration_url = Some("https://provider.test/register".into()),
            |i| i.scopes.push("admin".into()),
            |i| i.scope = Scope::Global,
        ];
        for change in changes {
            let p = pool().await;
            create(&p, dispatch_input(AuthKind::OAuth2, Scope::PerUser))
                .await
                .unwrap();
            let old = get(&p, "dispatch")
                .await
                .unwrap()
                .unwrap()
                .dispatch_generation;
            let mut input = dispatch_input(AuthKind::OAuth2, Scope::PerUser);
            change(&mut input);
            update(&p, "dispatch", input).await.unwrap();
            assert_ne!(
                get(&p, "dispatch")
                    .await
                    .unwrap()
                    .unwrap()
                    .dispatch_generation,
                old
            );
        }
    }

    #[tokio::test]
    async fn dispatch_generation_static_secret_depends_on_actual_identity_scope() {
        for scope in [Scope::Global, Scope::Agent, Scope::PerUser] {
            let p = pool().await;
            create(&p, dispatch_input(AuthKind::StaticBearer, scope))
                .await
                .unwrap();
            let old = get(&p, "dispatch")
                .await
                .unwrap()
                .unwrap()
                .dispatch_generation;
            let mut input = dispatch_input(AuthKind::StaticBearer, scope);
            input.client_secret_ct = Some(vec![9]);
            update(&p, "dispatch", input).await.unwrap();
            assert_eq!(
                get(&p, "dispatch")
                    .await
                    .unwrap()
                    .unwrap()
                    .dispatch_generation
                    == old,
                scope == Scope::PerUser
            );
        }
        let p = pool().await;
        create(&p, dispatch_input(AuthKind::None, Scope::Global))
            .await
            .unwrap();
        let old = get(&p, "dispatch")
            .await
            .unwrap()
            .unwrap()
            .dispatch_generation;
        let mut invalid = dispatch_input(AuthKind::None, Scope::Global);
        invalid.client_secret_nonce = None;
        assert!(update(&p, "dispatch", invalid).await.is_err());
        assert_eq!(
            get(&p, "dispatch")
                .await
                .unwrap()
                .unwrap()
                .dispatch_generation,
            old
        );
    }
    #[tokio::test]
    async fn transactional_enabled_inventory_observes_name_generation_order_and_rollback() {
        let pool = pool().await;
        seed_defaults(&pool).await.unwrap();
        let first = get(&pool, "atlassian").await.unwrap().unwrap();
        let second = get(&pool, "github").await.unwrap().unwrap();
        let mut tx = WriteTx::begin(&pool).await.unwrap();
        sqlx::query(
            "UPDATE mcp_catalog_connectors SET enabled=1,name='B current' WHERE key='atlassian'",
        )
        .execute(&mut *tx)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE mcp_catalog_connectors SET enabled=1,name='A current' WHERE key='github'",
        )
        .execute(&mut *tx)
        .await
        .unwrap();
        let rows = list_enabled_in(&mut tx).await.unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].key, "github");
        assert_eq!(rows[0].name, "A current");
        assert_eq!(rows[0].dispatch_generation, second.dispatch_generation);
        assert_eq!(rows[1].key, "atlassian");
        assert_eq!(rows[1].name, "B current");
        assert_eq!(rows[1].dispatch_generation, first.dispatch_generation);
        drop(tx);
        assert!(list_enabled(&pool).await.unwrap().is_empty());
        for original in [first, second] {
            let restored = get(&pool, &original.key).await.unwrap().unwrap();
            assert_eq!(restored.name, original.name);
            assert_eq!(restored.enabled, original.enabled);
            assert_eq!(restored.dispatch_generation, original.dispatch_generation);
            assert_eq!(restored.allowed_groups, original.allowed_groups);
        }
    }
}
