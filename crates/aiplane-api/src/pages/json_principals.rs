// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! `/api/v0/system-principals` — create and configure non-person principals
//! (`docs/agents.md` → "Principals").
//!
//! Every route needs the agent-management permission (`can_manage_agents` on
//! one of the caller's groups; admin implies it). A grant is capped at what
//! the manager holds *at grant time*, checked with the same rule that decides
//! the manager's own access to that resource, and is never re-derived from the
//! manager afterwards. Every change is written to `agent_audit` by the db
//! layer, in the same transaction as the change.
//!
//! An agent's principal (`docs/agents.md` → "Agent definition") is also reachable here, so for
//! one of those the caller additionally needs a share on the agent — `read`
//! to see it, `write` to change it — exactly as on `/api/v0/agents`. Any
//! other principal is reachable only by its creator and by admins.
//!
//! A token carries every grant of its principal, so issuing one is held to
//! the grant-time cap for all of them at once; a non-admin's token is capped
//! again at every request, at what its minter holds then
//! (`grant_holding::capped_to_minter`).

use session_core::i18n::{Lang, t};
use std::sync::Arc;

use jiff::Timestamp;
use rama::http::service::web::extract::State;
use rama::http::{Request, Response, StatusCode};
use serde::Deserialize;

use super::json_agents::guard_principal;
use super::{bad_request, internal, json_error, json_ok, no_content, not_found, raw_path_segment};
use aiplane_agents::db::agents::{self as agents_db, Access};
use aiplane_agents::db::system_principals::GrantChange;
use aiplane_agents::db::{agent_audit, system_principals as sp_db};
use aiplane_core::server::auth::token;
use aiplane_core::server::db::users;
use aiplane_core::server::principal::GrantKind;
use aiplane_runtime::rama_server::state::RamaState;
use aiplane_runtime::server::grant_holding::{self, HoldRefusal};

/// Session + agent-management permission, or the 401/403 to return.
pub(crate) async fn require_agent_manager(
    state: &RamaState,
    req: &Request,
) -> Result<users::User, Response> {
    let (_, user) = super::require_session_json(state, req).await?;
    let role_ids = state.rbac.role_ids_for(&user.roles);
    if !state.rbac.can_manage_agents(&role_ids) {
        return Err(json_error(
            StatusCode::FORBIDDEN,
            "forbidden",
            &t(
                Lang::from_request(req.headers()),
                "agent-management-error-permission-required",
            ),
        ));
    }
    Ok(user)
}

macro_rules! require_agent_manager {
    ($state:expr, $req:expr) => {
        match require_agent_manager(&$state, &$req).await {
            Ok(user) => user,
            Err(resp) => return resp,
        }
    };
}

async fn begin_principal_write(
    state: &RamaState,
    manager: &users::User,
    id: &str,
    lang: Lang,
) -> Result<aiplane_core::server::db::WriteTx, Response> {
    let mut tx = aiplane_core::server::db::WriteTx::begin(&state.db)
        .await
        .map_err(internal)?;
    let (current, policy) =
        super::json_organizations::current_policy_in(state, &mut tx, &manager.id, lang).await?;
    let groups = policy.role_ids_for(&current.roles);
    if !policy.can_manage_agents(&groups) {
        return Err(json_error(
            StatusCode::FORBIDDEN,
            "forbidden",
            &t(lang, "agent-management-error-permission-required"),
        ));
    }
    let principal = sp_db::get_in(&mut tx, id)
        .await
        .map_err(internal)?
        .ok_or_else(|| not_found(t(lang, "organization-error-not-found")))?;
    if agents_db::get_in(&mut tx, id)
        .await
        .map_err(internal)?
        .is_some()
    {
        if super::json_agents::is_personal(state, id).await?
            || aiplane_runtime::agents::access::effective_access_in(state, &mut tx, id, &manager.id)
                .await
                .map_err(internal)?
                != Some(Access::Write)
        {
            return Err(not_found(t(lang, "organization-error-not-found")));
        }
    } else if principal.created_by != current.id && !policy.is_admin(&groups) {
        return Err(not_found(t(lang, "organization-error-not-found")));
    }
    Ok(tx)
}

async fn refuse_organization_execution(
    state: &RamaState,
    id: &str,
    lang: Lang,
) -> Result<(), Response> {
    if matches!(
        agents_db::agent_owner(&state.db, id)
            .await
            .map_err(internal)?,
        Some(agents_db::AgentOwner::Organization(_))
    ) {
        Err(json_error(
            StatusCode::FORBIDDEN,
            "organization_execution_unavailable",
            &t(lang, "organization-error-execution-unavailable"),
        ))
    } else {
        Ok(())
    }
}

/// A system principal as the API shows it.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct PrincipalView {
    pub id: String,
    pub name: String,
    pub display: String,
    pub description: String,
    pub created_by: String,
    pub created_at: Timestamp,
    pub disabled_at: Option<Timestamp>,
}

impl PrincipalView {
    fn of(p: &sp_db::PrincipalRow) -> Self {
        Self {
            id: p.id.clone(),
            name: p.name.clone(),
            display: p.display.clone(),
            description: p.description.clone(),
            created_by: p.created_by.clone(),
            created_at: p.created_at,
            disabled_at: p.disabled_at,
        }
    }
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct PrincipalWithGrants {
    #[serde(flatten)]
    pub principal: PrincipalView,
    pub grants: Vec<GrantView>,
}

/// One grant as the API shows it.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct GrantView {
    /// `tool`, `connector`, `skill`, `rag_collection`, `model`, `a2a_caller`
    /// or `a2a_agent`.
    pub kind: &'static str,
    #[serde(rename = "ref")]
    pub reference: String,
    pub granted_by: String,
    pub granted_at: Timestamp,
    /// A `model` grant's pools, `null` for every pool serving it; absent on
    /// every other kind.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pools: Option<Option<Vec<String>>>,
}

impl GrantView {
    pub(super) fn of(g: &sp_db::GrantRow) -> Self {
        Self {
            kind: g.kind.as_str(),
            reference: g.reference.clone(),
            granted_by: g.granted_by.clone(),
            granted_at: g.granted_at,
            pools: (g.kind == GrantKind::Model).then(|| g.pools.clone()),
        }
    }
}

/// A `gws_` token without its secret.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct SystemTokenView {
    pub id: String,
    pub name: String,
    pub created_by: String,
    pub created_at: Timestamp,
    pub last_used_at: Option<Timestamp>,
    pub expires_at: Timestamp,
    pub revoked: bool,
}

impl SystemTokenView {
    fn of(t: &sp_db::SystemToken) -> Self {
        Self {
            id: t.id.clone(),
            name: t.name.clone(),
            created_by: t.created_by.clone(),
            created_at: t.created_at,
            last_used_at: t.last_used_at,
            expires_at: t.expires_at,
            revoked: t.revoked_at.is_some(),
        }
    }
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct PrincipalList {
    pub principals: Vec<PrincipalWithGrants>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct CreatedPrincipal {
    pub principal: PrincipalWithGrants,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct PrincipalDetailReply {
    pub principal: PrincipalDetail,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct PrincipalDetail {
    #[serde(flatten)]
    pub principal: PrincipalView,
    pub grants: Vec<GrantView>,
    pub tokens: Vec<SystemTokenView>,
    /// Newest first.
    pub audit: Vec<PrincipalAuditEntry>,
}

/// One management or run event on the principal.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct PrincipalAuditEntry {
    pub kind: String,
    pub actor_id: Option<String>,
    /// The run's call chain on a run event; `null` on a management event.
    pub chain: Option<serde_json::Value>,
    /// The event's own fields, which differ by `kind`.
    pub detail: serde_json::Value,
    pub created_at: Timestamp,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct DisabledPrincipal {
    pub principal: PrincipalView,
}

/// What a grant request changed.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct GrantOutcome {
    pub kind: &'static str,
    #[serde(rename = "ref")]
    pub reference: String,
    /// The grant is new.
    pub added: bool,
    /// A `model` grant now reaches more pools.
    pub widened: bool,
}

/// A newly issued token. `plaintext` is shown here and nowhere else, ever.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct IssuedSystemToken {
    pub token: SystemTokenView,
    pub plaintext: String,
}

/// The principal named by the path segment `from_end` back, or the 404 —
/// also when `manager` may not reach it ([`guard_principal`]).
async fn principal_at(
    state: &RamaState,
    req: &Request,
    manager: &users::User,
    from_end: usize,
    need: Access,
) -> Result<sp_db::PrincipalRow, Response> {
    let Some(id) = raw_path_segment(req, from_end) else {
        return Err(bad_request("the URL is missing the principal id"));
    };
    let p = match sp_db::get(&state.db, &id).await {
        Ok(Some(p)) => p,
        Ok(None) => return Err(not_found(format!("there is no system principal `{id}`"))),
        Err(err) => return Err(internal(err)),
    };
    guard_principal(state, manager, &p, need).await?;
    Ok(p)
}

/// GET /api/v0/system-principals
pub async fn list(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let manager = require_agent_manager!(state, req);
    let rows = match sp_db::list(&state.db).await {
        Ok(rows) => rows,
        Err(err) => return internal(err),
    };
    let mut out = Vec::with_capacity(rows.len());
    for p in &rows {
        if guard_principal(&state, &manager, p, Access::Read)
            .await
            .is_err()
        {
            continue;
        }
        let grants = match sp_db::grants(&state.db, &p.id).await {
            Ok(g) => g,
            Err(err) => return internal(err),
        };
        out.push(PrincipalWithGrants {
            principal: PrincipalView::of(p),
            grants: grants.iter().map(GrantView::of).collect(),
        });
    }
    json_ok(StatusCode::OK, PrincipalList { principals: out })
}

#[derive(Deserialize, schemars::JsonSchema)]
#[schemars(rename = "PrincipalCreateBody")]
pub struct CreateBody {
    pub name: String,
    #[serde(default)]
    pub display: Option<String>,
    #[serde(default)]
    pub description: String,
}

/// POST /api/v0/system-principals — a new principal with no rights at all.
pub async fn create(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let manager = require_agent_manager!(state, req);
    let body: CreateBody = match super::read_json(req.into_body(), "the principal body").await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    let name = body.name.trim();
    if let Some(reason) = sp_db::invalid_name_reason(name) {
        return bad_request(reason);
    }
    let display = body
        .display
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .unwrap_or(name);
    let created = sp_db::create(
        &state.db,
        &sp_db::NewPrincipal {
            name,
            display,
            description: body.description.trim(),
        },
        &manager.id,
    )
    .await;
    match created {
        Ok(Some(p)) => json_ok(
            StatusCode::CREATED,
            CreatedPrincipal {
                principal: PrincipalWithGrants {
                    principal: PrincipalView::of(&p),
                    grants: Vec::new(),
                },
            },
        ),
        Ok(None) => json_error(
            StatusCode::CONFLICT,
            "conflict",
            &format!("a system principal named `{name}` already exists — pick another name"),
        ),
        Err(err) => internal(err),
    }
}

/// GET /api/v0/system-principals/{id} — the principal, its grants, tokens
/// and audit trail.
pub async fn detail(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let manager = require_agent_manager!(state, req);
    let p = match principal_at(&state, &req, &manager, 0, Access::Read).await {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    let (grants, tokens, audit) = match tokio::try_join!(
        sp_db::grants(&state.db, &p.id),
        sp_db::tokens(&state.db, &p.id),
        agent_audit::for_principal(&state.db, &p.id),
    ) {
        Ok(v) => v,
        Err(err) => return internal(err),
    };
    let audit = audit
        .into_iter()
        .map(|e| PrincipalAuditEntry {
            kind: e.kind,
            actor_id: e.actor_id,
            chain: e.chain,
            detail: e.detail,
            created_at: e.created_at,
        })
        .collect();
    json_ok(
        StatusCode::OK,
        PrincipalDetailReply {
            principal: PrincipalDetail {
                principal: PrincipalView::of(&p),
                grants: grants.iter().map(GrantView::of).collect(),
                tokens: tokens.iter().map(SystemTokenView::of).collect(),
                audit,
            },
        },
    )
}

/// POST /api/v0/system-principals/{id}/disable — disables it and revokes
/// every token it holds. Its grants stay, so re-enabling later is a
/// reconfiguration, not a rebuild.
pub async fn disable(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let lang = Lang::from_request(req.headers());
    let manager = require_agent_manager!(state, req);
    let p = match principal_at(&state, &req, &manager, 1, Access::Write).await {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    let mut tx = match begin_principal_write(&state, &manager, &p.id, lang).await {
        Ok(tx) => tx,
        Err(response) => return response,
    };
    let changed = match sp_db::disable_in(&mut tx, &p.id, &manager.id).await {
        Ok(changed) => changed,
        Err(error) => return internal(error),
    };
    if let Err(error) = tx.commit().await {
        return internal(error);
    }
    if changed {
        aiplane_agents::db::embed_keys::origins_changed();
        // Disabling a principal stops its personal agent's in-flight runs, the
        // same as deleting or deactivating the agent — so a suspended run never
        // orphans on a disabled principal.
        if let Some(owner) = aiplane_agents::db::agents::personal_agent_owner(&state.db, &p.id)
            .await
            .ok()
            .flatten()
            && let Err(error) = aiplane_runtime::agents::resume::cancel_personal_runs_for_agent(
                &state, &owner, &p.id,
            )
            .await
        {
            tracing::warn!(
                principal = %p.id,
                %error,
                "cancelling the disabled principal's personal runs"
            );
        }
    }
    match sp_db::get(&state.db, &p.id).await {
        Ok(Some(p)) => json_ok(
            StatusCode::OK,
            DisabledPrincipal {
                principal: PrincipalView::of(&p),
            },
        ),
        Ok(None) => not_found("the principal was removed while it was being disabled"),
        Err(err) => internal(err),
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct GrantBody {
    /// `tool`, `connector`, `skill`, `rag_collection`, `model`, `a2a_caller`
    /// or `a2a_agent`.
    pub kind: String,
    #[serde(rename = "ref")]
    pub reference: String,
}

fn parse_grant(body: &GrantBody) -> Result<(GrantKind, &str), Response> {
    let kind = GrantKind::parse(&body.kind).ok_or_else(|| {
        bad_request(format!(
            "`{}` is not a grant kind — use one of tool, connector, skill, rag_collection, pool, \
             a2a_caller, a2a_agent",
            body.kind
        ))
    })?;
    let reference = body.reference.trim();
    if reference.is_empty() {
        return Err(bad_request("a grant needs a `ref`"));
    }
    if reference.contains('*') {
        return Err(bad_request(
            "a system principal cannot be granted a wildcard — grant each resource by name",
        ));
    }
    Ok((kind, reference))
}

/// Whether `manager` holds `kind` `reference` today
/// ([`grant_holding::holds`]), or the response that says why it cannot be
/// granted at all.
async fn manager_holds(
    state: &RamaState,
    manager: &users::User,
    kind: GrantKind,
    reference: &str,
) -> Result<bool, Response> {
    if kind == GrantKind::A2aCaller
        && let Some(agent) = agents_db::get(&state.db, reference)
            .await
            .map_err(internal)?
    {
        guard_principal(state, manager, &agent.principal, Access::Write).await?;
    }
    grant_holding::holds(state, manager, kind, reference)
        .await
        .map_err(|refusal| match refusal {
            HoldRefusal::Missing(_) => not_found(refusal.to_string()),
            HoldRefusal::Invalid(why) => bad_request(why),
            HoldRefusal::AdminOnly(why) => {
                json_error(StatusCode::FORBIDDEN, "grant_exceeds_manager", &why)
            }
            HoldRefusal::Db(err) => internal(err),
        })
}

/// `Ok` when `manager` may hand out a token for `principal`: an admin always,
/// anyone else only while they hold every grant it has. A grant that could
/// not be made today (its resource is gone) counts as not held.
async fn manager_holds_every_grant(
    state: &RamaState,
    manager: &users::User,
    principal: &sp_db::PrincipalRow,
) -> Result<(), Response> {
    if state
        .rbac
        .is_admin(&state.rbac.role_ids_for(&manager.roles))
    {
        return Ok(());
    }
    let grants = sp_db::grants(&state.db, &principal.id)
        .await
        .map_err(internal)?;
    for g in &grants {
        let held = match manager_holds(state, manager, g.kind, &g.reference).await {
            Ok(held) => held,
            Err(resp) if resp.status() == StatusCode::INTERNAL_SERVER_ERROR => return Err(resp),
            Err(_) => false,
        };
        if !held {
            return Err(json_error(
                StatusCode::FORBIDDEN,
                "token_exceeds_manager",
                &format!(
                    "cannot issue a token for `{}`: it holds {} `{}`, which you do not hold \
                     yourself, and a token would hand it to whoever has the token. Ask an admin \
                     to issue the token, or revoke that grant first.",
                    principal.name,
                    kind_label(g.kind),
                    g.reference
                ),
            ));
        }
    }
    Ok(())
}

fn kind_label(kind: GrantKind) -> &'static str {
    match kind {
        GrantKind::Tool => "tool",
        GrantKind::Connector => "connector",
        GrantKind::Skill => "skill",
        GrantKind::RagCollection => "RAG collection",
        GrantKind::Model => "model",
        GrantKind::A2aCaller => "A2A caller",
        GrantKind::A2aAgent => "A2A agent",
    }
}

/// POST /api/v0/system-principals/{id}/grants — add one grant, capped at
/// what the caller holds right now: `{kind, ref, added, widened}`. `201`
/// when it is new; `200` otherwise, with `widened` true when a `model` grant
/// now reaches more pools (a regrant never narrows one).
pub async fn grant(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let lang = Lang::from_request(req.headers());
    let manager = require_agent_manager!(state, req);
    let p = match principal_at(&state, &req, &manager, 1, Access::Write).await {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    let body: GrantBody = match super::read_json(req.into_body(), "the grant body").await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    let (kind, reference) = match parse_grant(&body) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let change = match add_capped_grant(&state, &manager, &p.id, kind, reference, lang).await {
        Ok(change) => change,
        Err(resp) => return resp,
    };
    let added = change == GrantChange::Added;
    json_ok(
        if added {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        GrantOutcome {
            kind: kind.as_str(),
            reference: reference.to_string(),
            added,
            widened: change == GrantChange::Widened,
        },
    )
}

/// Grant `kind` `reference` to principal `principal_id` (whose access the
/// caller already checked), capped at what `manager` holds right now.
pub(crate) async fn add_capped_grant(
    state: &RamaState,
    manager: &users::User,
    principal_id: &str,
    kind: GrantKind,
    reference: &str,
    lang: Lang,
) -> Result<GrantChange, Response> {
    let mut tx = begin_principal_write(state, manager, principal_id, lang).await?;
    if kind == GrantKind::A2aCaller {
        let held = aiplane_runtime::agents::access::effective_access_in(
            state,
            &mut tx,
            reference,
            &manager.id,
        )
        .await
        .map_err(internal)?;
        super::json_agents::require_held(held, reference, Access::Write)?;
    }
    let scope = grant_holding::grant_scope_in(
        state,
        &mut tx,
        &manager.id,
        kind,
        reference,
        Some(principal_id),
    )
    .await
    .map_err(|refusal| match refusal {
        HoldRefusal::Missing(_) => not_found(refusal.to_string()),
        HoldRefusal::Invalid(why) => bad_request(why),
        HoldRefusal::AdminOnly(why) => {
            json_error(StatusCode::FORBIDDEN, "grant_exceeds_manager", &why)
        }
        HoldRefusal::Db(error) => internal(error),
    })?;
    let Some(scope) = scope else {
        return Err(json_error(
            StatusCode::FORBIDDEN,
            "grant_exceeds_manager",
            &format!(
                "cannot grant {} `{reference}`: you do not hold it yourself, and a manager can only grant what they hold. Ask an admin to grant it to one of your groups, or have someone who holds it make this grant.",
                kind_label(kind)
            ),
        ));
    };
    let result = sp_db::add_scoped_grant_in(
        &mut tx,
        principal_id,
        kind,
        reference,
        scope.pools.as_deref(),
        &manager.id,
    )
    .await
    .map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    Ok(result)
}

/// POST /api/v0/system-principals/{id}/grants/revoke — remove one grant. Any
/// manager may narrow a principal; no holding check.
pub async fn revoke_grant(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let lang = Lang::from_request(req.headers());
    let manager = require_agent_manager!(state, req);
    let p = match principal_at(&state, &req, &manager, 2, Access::Write).await {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    let body: GrantBody = match super::read_json(req.into_body(), "the grant body").await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    let Some(kind) = GrantKind::parse(&body.kind) else {
        return bad_request(format!("`{}` is not a grant kind", body.kind));
    };
    let reference = body.reference.trim();
    let mut tx = match begin_principal_write(&state, &manager, &p.id, lang).await {
        Ok(tx) => tx,
        Err(response) => return response,
    };
    let result = sp_db::remove_grant_in(&mut tx, &p.id, kind, reference, &manager.id).await;
    if result.is_ok()
        && let Err(error) = tx.commit().await
    {
        return internal(error);
    }
    match result {
        Ok(true) => no_content(),
        Ok(false) => not_found(format!(
            "`{}` holds no {} grant `{reference}`",
            p.name,
            kind_label(kind)
        )),
        Err(err) => internal(err),
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct TokenBody {
    pub name: String,
    #[serde(default)]
    pub ttl_days: Option<i64>,
}

/// POST /api/v0/system-principals/{id}/tokens — issue a `gws_` token, capped
/// like a grant ([`manager_holds_every_grant`]). The plaintext is in this
/// response and nowhere else, ever.
pub async fn issue_token(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let lang = Lang::from_request(req.headers());
    let manager = require_agent_manager!(state, req);
    let p = match principal_at(&state, &req, &manager, 1, Access::Write).await {
        Ok(p) => p,
        Err(resp) => return resp,
    };

    if let Err(response) = refuse_organization_execution(&state, &p.id, lang).await {
        return response;
    }
    if p.disabled_at.is_some() {
        return json_error(
            StatusCode::CONFLICT,
            "principal_disabled",
            &format!(
                "`{}` is disabled, so a token for it would never authenticate",
                p.name
            ),
        );
    }
    if let Err(resp) = manager_holds_every_grant(&state, &manager, &p).await {
        return resp;
    }
    let body: TokenBody = match super::read_json(req.into_body(), "the token body").await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    let (name, expires_at) = match token::token_terms(
        &body.name,
        body.ttl_days,
        state.config().gateway.token_ttl_days,
        Timestamp::now(),
    ) {
        Ok(terms) => terms,
        Err(message) => return bad_request(message),
    };
    let (plaintext, hash) = token::mint_system();
    match sp_db::insert_token(&state.db, &p.id, name, &hash, expires_at, &manager.id).await {
        Ok(t) => json_ok(
            StatusCode::CREATED,
            IssuedSystemToken {
                token: SystemTokenView::of(&t),
                plaintext,
            },
        ),
        Err(err) => internal(err),
    }
}

/// POST /api/v0/system-principals/{id}/tokens/{token_id}/revoke
pub async fn revoke_token(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let lang = Lang::from_request(req.headers());
    let manager = require_agent_manager!(state, req);
    let p = match principal_at(&state, &req, &manager, 3, Access::Write).await {
        Ok(p) => p,
        Err(resp) => return resp,
    };

    if let Err(response) = refuse_organization_execution(&state, &p.id, lang).await {
        return response;
    }
    let Some(token_id) = raw_path_segment(&req, 1) else {
        return bad_request("the URL is missing the token id");
    };
    match sp_db::revoke_token(&state.db, &p.id, &token_id, &manager.id).await {
        Ok(true) => no_content(),
        Ok(false) => not_found(format!("`{}` has no live token `{token_id}`", p.name)),
        Err(err) => internal(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wildcard_or_unknown_kind_is_refused_before_any_lookup() {
        let body = |kind: &str, reference: &str| GrantBody {
            kind: kind.into(),
            reference: reference.into(),
        };
        assert!(parse_grant(&body("tool", "*")).is_err());
        assert!(parse_grant(&body("skill", "brand*")).is_err());
        assert!(parse_grant(&body("pool", "chat")).is_err());
        assert!(parse_grant(&body("tool", "  ")).is_err());
        let ok = body("model", " qwen ");
        assert_eq!(parse_grant(&ok).unwrap(), (GrantKind::Model, "qwen"));
    }

    #[test]
    fn every_grant_kind_has_a_label() {
        for kind in GrantKind::ALL {
            assert!(!kind_label(kind).is_empty());
        }
    }
}
