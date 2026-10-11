// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Whether a person holds what a system principal's grant hands out
//! (`docs/agents.md` → "Principals", `docs/auth.md` → "System principals and `gws_` tokens").
//!
//! One rule, used twice: a manager may grant only what they hold
//! (`aiplane-api`'s `json_principals`, at grant and token-issue time), and a
//! token a non-admin manager minted carries only what that manager holds at
//! authentication time ([`capped_to_minter`], from `require_bearer`). It is
//! the same rule that decides the person's own access to each resource.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use aiplane_agents::db::agents::{self as agents_db, Access};
use aiplane_core::server::db::{
    DbError, WriteTx, automatic_routes, mcp_catalog, rag as rag_db, users,
};
use aiplane_core::server::principal::{GrantKind, GrantSet, SystemPrincipal};
use aiplane_core::server::upstreams::{PoolAccess, PoolKind};

use crate::agents::access::effective_access;
use crate::rama_server::state::RamaState;
use crate::server::tools::mcp::MCP_ID_PREFIX;
use crate::server::{
    AppState,
    model_choices::{self, ModelChoice},
};

/// Why a grant could not be judged held: the resource does not exist, the
/// reference is not one, or only an admin may grant it.
#[derive(Debug, thiserror::Error)]
pub enum HoldRefusal {
    #[error("there is no {0} on this gateway")]
    Missing(String),
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    AdminOnly(String),
    #[error(transparent)]
    Db(#[from] DbError),
}

pub struct GrantScope {
    pub pools: Option<Vec<String>>,
}

enum HoldingSource<'a> {
    Pool,
    Transaction(&'a mut WriteTx),
}

impl HoldingSource<'_> {
    async fn connector(
        &mut self,
        state: &AppState,
        reference: &str,
    ) -> Result<Option<mcp_catalog::Connector>, DbError> {
        match self {
            Self::Pool => mcp_catalog::get(&state.db, reference).await,
            Self::Transaction(tx) => mcp_catalog::get_in(tx, reference).await,
        }
    }

    async fn collection(
        &mut self,
        state: &AppState,
        id: i64,
    ) -> Result<Option<rag_db::Collection>, DbError> {
        match self {
            Self::Pool => rag_db::find_collection_by_id(&state.db, id).await,
            Self::Transaction(tx) => rag_db::find_collection_by_id_in(tx, id).await,
        }
    }

    async fn agent_exists(&mut self, state: &AppState, reference: &str) -> Result<bool, DbError> {
        match self {
            Self::Pool => Ok(agents_db::get(&state.db, reference).await?.is_some()),
            Self::Transaction(tx) => Ok(agents_db::get_in(tx, reference).await?.is_some()),
        }
    }

    async fn access(
        &mut self,
        state: &RamaState,
        reference: &str,
        user_id: &str,
    ) -> Result<Option<Access>, DbError> {
        match self {
            Self::Pool => effective_access(state, reference, user_id).await,
            Self::Transaction(tx) => {
                crate::agents::access::effective_access_in(state, tx, reference, user_id).await
            }
        }
    }

    async fn choices(
        &mut self,
        state: &AppState,
        kind: PoolKind,
        access: &PoolAccess,
    ) -> Result<Vec<ModelChoice>, DbError> {
        match self {
            Self::Pool => Ok(model_choices::offered(state, kind, access).await),
            Self::Transaction(tx) => model_choices::offered_in(state, tx, kind, access).await,
        }
    }

    async fn is_automatic_route(&mut self, state: &AppState, model: &str) -> Result<bool, DbError> {
        match self {
            Self::Pool => Ok(automatic_routes::get(&state.db, model).await?.is_some()),
            Self::Transaction(tx) => Ok(automatic_routes::all_in(tx)
                .await?
                .iter()
                .any(|route| route.alias == model)),
        }
    }
}

pub async fn grant_scope_in(
    state: &RamaState,
    tx: &mut WriteTx,
    user_id: &str,
    kind: GrantKind,
    reference: &str,
    target_principal_id: Option<&str>,
) -> Result<Option<GrantScope>, HoldRefusal> {
    let Some((user, policy)) =
        crate::agents::access::current_user_policy_in(state, tx, user_id).await?
    else {
        return Ok(None);
    };
    let mut app = (**state).clone();
    app.rbac = Arc::new(policy);
    let target = if kind == GrantKind::Connector {
        connector_target_in(&app, tx, &user.id, target_principal_id).await?
    } else {
        None
    };
    let mut source = HoldingSource::Transaction(tx);
    if !held(
        state,
        &app,
        &mut source,
        &user,
        kind,
        reference,
        target.as_ref(),
    )
    .await?
    {
        return Ok(None);
    }
    let pools = if kind == GrantKind::Model {
        model_pools(&app, &mut source, &user, reference).await?
    } else {
        None
    };
    Ok(Some(GrantScope { pools }))
}

struct ConnectorTarget {
    dot_id: String,
    owner: agents_db::DotOwner,
    can_write: bool,
}

async fn connector_target_in(
    state: &AppState,
    tx: &mut WriteTx,
    user_id: &str,
    target: Option<&str>,
) -> Result<Option<ConnectorTarget>, DbError> {
    let Some(dot_id) = target else {
        return Ok(None);
    };
    let Some(owner @ agents_db::DotOwner::Organization(_)) =
        agents_db::dot_owner_in(tx, dot_id).await?
    else {
        return Ok(None);
    };
    let can_write = crate::agents::access::organization_access_in(state, tx, dot_id, user_id)
        .await?
        == Some(Access::Write);
    Ok(Some(ConnectorTarget {
        dot_id: dot_id.into(),
        owner,
        can_write,
    }))
}

async fn connector_held(
    state: &AppState,
    source: &mut HoldingSource<'_>,
    user: &users::User,
    connector: &mcp_catalog::Connector,
    target: Option<&ConnectorTarget>,
) -> Result<bool, HoldRefusal> {
    let roles = state.rbac.role_ids_for(&user.roles);
    let is_admin = state.rbac.is_admin(&roles);
    let key = format!("{MCP_ID_PREFIX}{}", connector.key);
    if let Some(target) = target {
        if !target.can_write
            || !connector.enabled
            || !connector.is_per_user()
            || connector.auth != mcp_catalog::AuthKind::OAuth2
            || !connector.allows(&roles, is_admin)
            || !state.mcp_grant_for(&user.roles).allows(&key, &key)
        {
            return Ok(false);
        }
        let HoldingSource::Transaction(tx) = source else {
            return Ok(false);
        };
        return Ok(
            crate::server::tools::mcp::identities::selected_principal_identity_in(
                tx,
                &target.owner,
                &target.dot_id,
                &connector.key,
            )
            .await?
            .is_some(),
        );
    }
    if !connector.has_shared_identity() {
        return Err(HoldRefusal::Invalid(format!(
            "connector `{}` signs in as each person, so a system principal cannot use it — only connectors with the `global` or `agent` scope can be granted",
            connector.key
        )));
    }
    Ok(if connector.is_agent() {
        connector.grantable_by(&roles, is_admin)
    } else {
        connector.allows(&roles, is_admin) && state.mcp_grant_for(&user.roles).allows(&key, &key)
    })
}

pub async fn grantable_connectors_in(
    state: &RamaState,
    tx: &mut WriteTx,
    user_id: &str,
    target_principal_id: Option<&str>,
) -> Result<BTreeSet<String>, HoldRefusal> {
    let Some((user, policy)) =
        crate::agents::access::current_user_policy_in(state, tx, user_id).await?
    else {
        return Ok(BTreeSet::new());
    };
    let mut app = (**state).clone();
    app.rbac = Arc::new(policy);
    let target = connector_target_in(&app, tx, &user.id, target_principal_id).await?;
    let connectors = mcp_catalog::list_enabled_in(tx).await?;
    let mut source = HoldingSource::Transaction(tx);
    let mut keys = BTreeSet::new();
    for connector in connectors {
        if target.is_none() && !connector.has_shared_identity() {
            continue;
        }
        if connector_held(&app, &mut source, &user, &connector, target.as_ref()).await? {
            keys.insert(connector.key);
        }
    }
    Ok(keys)
}

/// Whether `user` holds `kind` `reference` today.
pub async fn holds(
    state: &RamaState,
    user: &users::User,
    kind: GrantKind,
    reference: &str,
) -> Result<bool, HoldRefusal> {
    held(
        state,
        state,
        &mut HoldingSource::Pool,
        user,
        kind,
        reference,
        None,
    )
    .await
}

async fn held(
    authority: &RamaState,
    state: &AppState,
    source: &mut HoldingSource<'_>,
    user: &users::User,
    kind: GrantKind,
    reference: &str,
    target: Option<&ConnectorTarget>,
) -> Result<bool, HoldRefusal> {
    let role_ids = state.rbac.role_ids_for(&user.roles);
    let missing = |what: String| HoldRefusal::Missing(what);
    let held = match kind {
        GrantKind::Tool => {
            if reference.starts_with(MCP_ID_PREFIX) {
                return Err(HoldRefusal::Invalid(format!(
                    "`{reference}` is an MCP tool — grant its connector (kind `connector`) instead"
                )));
            }
            if !state.grantable_tool_ids().iter().any(|id| id == reference) {
                return Err(missing(format!("tool `{reference}`")));
            }
            let mut held = state.rbac.allowed_tools(&role_ids, &state.tools());
            state.expand_comfyui_tools(&mut held, &role_ids);
            held.iter().any(|id| id == reference)
        }
        GrantKind::Connector => {
            let connector = match source.connector(state, reference).await? {
                Some(c) if c.enabled => c,
                _ => return Err(missing(format!("enabled connector `{reference}`"))),
            };
            connector_held(state, source, user, &connector, target).await?
        }
        GrantKind::Skill => {
            let Some(store) = state.skills() else {
                return Err(missing(format!(
                    "skill `{reference}` (skills are not configured)"
                )));
            };
            let registry = store.current();
            if !registry.names().any(|n| n == reference) {
                return Err(missing(format!("skill `{reference}`")));
            }
            state
                .rbac
                .allowed_skills(&role_ids, &registry)
                .iter()
                .any(|n| n == reference)
        }
        GrantKind::RagCollection => {
            let Ok(id) = reference.parse::<i64>() else {
                return Err(HoldRefusal::Invalid(format!(
                    "a RAG collection is granted by its numeric id, not `{reference}`"
                )));
            };
            let Some(collection) = source.collection(state, id).await? else {
                return Err(missing(format!("RAG collection with id {id}")));
            };
            state
                .rbac
                .resource_allowed(&role_ids, &collection.allowed_groups)
        }
        GrantKind::Model => {
            if offers(state, source, &PoolAccess::all(), reference)
                .await?
                .is_none()
            {
                return Err(missing(format!("model `{reference}`")));
            }
            offers(
                state,
                source,
                &state.pool_access_for(&user.roles),
                reference,
            )
            .await?
            .is_some_and(|choice| choice.grantable())
        }
        GrantKind::A2aCaller => {
            // Letting another platform call an agent is a change to that
            // agent, so it takes what changing it takes: a `write` share.
            if !source.agent_exists(state, reference).await? {
                return Err(missing(format!("agent `{reference}`")));
            }
            source
                .access(authority, reference, &user.id)
                .await?
                .is_some_and(|a| a >= Access::Write)
        }
        GrantKind::A2aAgent => {
            let allow_private = state.config().network.allow_private_networks;
            if let Err(why) = crate::agents::a2a_client::check_card_url(reference, allow_private) {
                return Err(HoldRefusal::Invalid(format!(
                    "`{reference}` cannot be granted as an A2A agent: {why}"
                )));
            }
            // An external agent is nothing a person holds, and a grant on one
            // lets an agent send visitor data off the gateway: the
            // operator's call.
            if !state.rbac.is_admin(&role_ids) {
                return Err(HoldRefusal::AdminOnly(format!(
                    "cannot grant A2A agent `{reference}`: an external agent receives what its \
                     route sends it, so only an admin may grant one. Ask an admin to make this \
                     grant."
                )));
            }
            true
        }
    };
    Ok(held)
}

/// The pools a `model` grant `user` makes routes through: every pool of the
/// model's kind they may use — for an automatic route, of chat and selector
/// models — whether it serves the model right now or not, so a pool that is
/// down or not probed yet when the grant is made still counts once it serves
/// it. Routing takes those that serve it at request time. `None` for an
/// admin, whose grant routes through every pool serving it, as their own
/// requests do.
pub async fn model_grant_pools(
    state: &RamaState,
    user: &users::User,
    model: &str,
) -> Result<Option<Vec<String>>, DbError> {
    model_pools(state, &mut HoldingSource::Pool, user, model).await
}

async fn model_pools(
    state: &AppState,
    source: &mut HoldingSource<'_>,
    user: &users::User,
    model: &str,
) -> Result<Option<Vec<String>>, DbError> {
    if state.rbac.is_admin(&state.rbac.role_ids_for(&user.roles)) {
        return Ok(None);
    }
    let kinds = match source.is_automatic_route(state, model).await? {
        true => vec![PoolKind::Chat, PoolKind::SystemOne],
        false => kinds_of(state, source, model).await?,
    };
    Ok(Some(state.upstreams.pools_of_kinds(
        &kinds,
        &state.pool_access_for(&user.roles),
    )))
}

/// The kinds of model `model` is offered as anywhere on the gateway.
async fn kinds_of(
    state: &AppState,
    source: &mut HoldingSource<'_>,
    model: &str,
) -> Result<Vec<PoolKind>, DbError> {
    let mut kinds = Vec::new();
    for kind in grantable_kinds() {
        if source
            .choices(state, kind, &PoolAccess::all())
            .await?
            .iter()
            .any(|c| c.id == model)
        {
            kinds.push(kind);
        }
    }
    Ok(kinds)
}

/// The choice named `model` among the models of every kind an agent can use
/// that `access` may pick (`server::model_choices`).
async fn offers(
    state: &AppState,
    source: &mut HoldingSource<'_>,
    access: &PoolAccess,
    model: &str,
) -> Result<Option<ModelChoice>, DbError> {
    for kind in grantable_kinds() {
        if let Some(found) = source
            .choices(state, kind, access)
            .await?
            .into_iter()
            .find(|c| c.id == model)
        {
            return Ok(Some(found));
        }
    }
    Ok(None)
}

/// The kinds of model a model grant can name: every kind a request can
/// route to by name — an agent's chat, voice and image tools, a principal
/// token's `/v1` calls. The internal kinds are the gateway's own.
fn grantable_kinds() -> impl Iterator<Item = PoolKind> {
    PoolKind::ALL.into_iter().filter(|kind| !kind.is_internal())
}

/// `principal` as a token minted by user `minted_by` may use it: every grant
/// when the minter is an admin today, otherwise only the grants the minter
/// holds today — none once the minter is gone. A grant whose resource is
/// gone counts as not held.
pub async fn capped_to_minter(
    state: &RamaState,
    principal: SystemPrincipal,
    minted_by: &str,
) -> Result<SystemPrincipal, DbError> {
    let Some(minter) = users::find_by_id(&state.db, minted_by).await? else {
        return Ok(SystemPrincipal {
            grants: Arc::new(GrantSet::default()),
            ..principal
        });
    };
    if state.rbac.is_admin(&state.rbac.role_ids_for(&minter.roles)) {
        return Ok(principal);
    }
    let mut kept = Vec::new();
    let mut pools = Vec::new();
    for (kind, reference) in principal.grants.iter() {
        match holds(state, &minter, kind, reference).await {
            Ok(true) => {}
            Ok(false)
            | Err(HoldRefusal::Missing(_))
            | Err(HoldRefusal::Invalid(_))
            | Err(HoldRefusal::AdminOnly(_)) => continue,
            Err(HoldRefusal::Db(err)) => return Err(err),
        }
        if kind == GrantKind::Model {
            // The minter's own pools for it, narrowed further to the
            // grant's: a token never reaches a pool its minter could not use
            // or the grant does not name. None left, no grant.
            let mine: BTreeSet<String> = model_grant_pools(state, &minter, reference)
                .await?
                .unwrap_or_default()
                .into_iter()
                .collect();
            let reach: BTreeSet<String> = match principal.grants.model_pools(reference) {
                Some(granted) => granted.intersection(&mine).cloned().collect(),
                None => mine,
            };
            if reach.is_empty() {
                continue;
            }
            pools.push((reference.to_string(), reach));
        }
        kept.push((kind, reference.to_string()));
    }
    Ok(SystemPrincipal {
        grants: Arc::new(GrantSet::new(kept).with_model_pools(pools)),
        ..principal
    })
}

/// How long a token's capped grants are reused before they are worked out
/// again. Every in-process change that can shrink them — a login that
/// changes the minter's roles, an RBAC, settings or runtime reload, a
/// connector, a RAG collection's groups, an agent share — calls
/// [`GrantCaps::invalidate`], so the TTL only bounds what this process
/// cannot see: a skill directory edited on disk, a second gateway writing
/// the same database.
pub const CAP_TTL: Duration = Duration::from_secs(30);

/// A system token's `last_used_at` is written at most this often.
pub const TOUCH_EVERY: Duration = Duration::from_secs(60);

/// More tokens than this in either map and the stale entries are dropped.
const CAPS_PRUNE_AT: usize = 4096;

/// What [`capped_to_minter`] decided per system token, and when each token's
/// last use was written (`docs/auth.md` → "System principals and `gws_` tokens").
#[derive(Default)]
pub struct GrantCaps {
    capped: Mutex<CappedTokens>,
    touched: Mutex<HashMap<String, Instant>>,
}

#[derive(Default)]
struct CappedTokens {
    /// Bumped by every invalidation, so a cap worked out from what held
    /// before one is not stored after it.
    epoch: u64,
    by_token: HashMap<String, Capped>,
}

struct Capped {
    at: Instant,
    minted_by: String,
    /// The principal's own grants the cap was taken from: a grant added or
    /// removed since makes the entry miss, with no invalidation needed.
    granted: Arc<GrantSet>,
    capped: Arc<GrantSet>,
}

impl GrantCaps {
    /// Forget every cap: something the minters' rights depend on changed.
    pub fn invalidate(&self) {
        let mut caps = self.capped.lock().unwrap_or_else(|p| p.into_inner());
        caps.epoch += 1;
        caps.by_token.clear();
    }

    /// Whether `token_id`'s `last_used_at` is due a write; if so, it counts
    /// as written now.
    pub fn touch_due(&self, token_id: &str) -> bool {
        let mut touched = self.touched.lock().unwrap_or_else(|p| p.into_inner());
        if touched
            .get(token_id)
            .is_some_and(|at| at.elapsed() < TOUCH_EVERY)
        {
            return false;
        }
        if touched.len() >= CAPS_PRUNE_AT {
            touched.retain(|_, at| at.elapsed() < TOUCH_EVERY);
        }
        touched.insert(token_id.to_string(), Instant::now());
        true
    }

    fn get(
        &self,
        token_id: &str,
        minted_by: &str,
        granted: &GrantSet,
    ) -> (u64, Option<Arc<GrantSet>>) {
        let caps = self.capped.lock().unwrap_or_else(|p| p.into_inner());
        let hit = caps
            .by_token
            .get(token_id)
            .filter(|c| {
                c.at.elapsed() < CAP_TTL && c.minted_by == minted_by && *c.granted == *granted
            })
            .map(|c| c.capped.clone());
        (caps.epoch, hit)
    }

    fn put(&self, epoch: u64, token_id: &str, entry: Capped) {
        let mut caps = self.capped.lock().unwrap_or_else(|p| p.into_inner());
        if caps.epoch != epoch {
            return;
        }
        if caps.by_token.len() >= CAPS_PRUNE_AT {
            caps.by_token.retain(|_, c| c.at.elapsed() < CAP_TTL);
        }
        caps.by_token.insert(token_id.to_string(), entry);
    }
}

/// [`capped_to_minter`] for system token `token_id`, reusing the cap worked
/// out for it within [`CAP_TTL`] while the principal's grants, the minter
/// and everything [`GrantCaps::invalidate`] watches stay as they were.
pub async fn capped_for_token(
    state: &RamaState,
    token_id: &str,
    principal: SystemPrincipal,
    minted_by: &str,
) -> Result<SystemPrincipal, DbError> {
    let (epoch, hit) = state.grant_caps.get(token_id, minted_by, &principal.grants);
    if let Some(grants) = hit {
        return Ok(SystemPrincipal {
            grants,
            ..principal
        });
    }
    let granted = principal.grants.clone();
    let capped = capped_to_minter(state, principal, minted_by).await?;
    state.grant_caps.put(
        epoch,
        token_id,
        Capped {
            at: Instant::now(),
            minted_by: minted_by.to_string(),
            granted,
            capped: capped.grants.clone(),
        },
    );
    Ok(capped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn transactional_grant_scope_uses_current_policy_resources_and_model_pools() {
        use aiplane_core::server::db::{WriteTx, gateway_groups};
        let mut app = model_choices::tests::state().await;
        app.db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with((*app.db.connect_options()).clone())
            .await
            .unwrap();
        let pool = app
            .upstreams
            .pools()
            .into_iter()
            .find(|pool| pool.name == "open")
            .unwrap();
        pool.backends[0].set_models(["open-model".into(), "alternate-model".into()].into());
        gateway_groups::upsert_group(&app.db, "vip", "", false, false)
            .await
            .unwrap();
        gateway_groups::set_mappings_for_group(&app.db, "vip", &["claim-vip".into()])
            .await
            .unwrap();
        gateway_groups::set_tools_for_group(&app.db, "vip", &[format!("{MCP_ID_PREFIX}actual")])
            .await
            .unwrap();
        gateway_groups::upsert_group(&app.db, "admin", "", true, false)
            .await
            .unwrap();
        gateway_groups::set_mappings_for_group(&app.db, "admin", &["claim-admin".into()])
            .await
            .unwrap();
        let now = jiff::Timestamp::now();
        for (id, role) in [("manager", "claim-vip"), ("admin", "claim-admin")] {
            users::upsert(
                &app.db,
                &users::User {
                    id: id.into(),
                    email: format!("{id}@example.com"),
                    name: None,
                    roles: vec![role.into()],
                    created_at: now,
                    updated_at: now,
                    timezone: None,
                    speech_voice: None,
                },
            )
            .await
            .unwrap();
        }
        mcp_catalog::create(
            &app.db,
            mcp_catalog::ConnectorInput {
                key: "actual".into(),
                name: "Actual connector".into(),
                description: None,
                icon: None,
                category: None,
                url: "https://connector.example.com/mcp".into(),
                auth: mcp_catalog::AuthKind::None,
                scope: mcp_catalog::Scope::Global,
                audit: false,
                use_dcr: false,
                client_id: None,
                client_secret_ct: None,
                client_secret_nonce: None,
                authorize_url: None,
                token_url: None,
                registration_url: None,
                scopes: vec![],
                allowed_groups: vec!["vip".into()],
            },
        )
        .await
        .unwrap();
        mcp_catalog::set_enabled(&app.db, "actual", true)
            .await
            .unwrap();
        let collection = rag_db::create_collection(
            &app.db,
            &rag_db::NewCollection {
                name: "Actual collection".into(),
                description: None,
                git_url: "https://example.com/repository.git".into(),
                git_ref: "main".into(),
                pat: None,
                source: Default::default(),
                profile_id: None,
                extraction_model: None,
                embedding_model: "embedding-model".into(),
                include_globs: vec![],
                exclude_globs: vec![],
                chunk_size: 800,
                chunk_overlap: 100,
                search_mode: rag_db::SearchMode::Versioned,
                refresh_interval_mins: 0,
            },
        )
        .await
        .unwrap();
        rag_db::set_allowed_groups(&app.db, collection.id, &["vip".into()])
            .await
            .unwrap();
        app.rbac = Arc::new(aiplane_core::server::rbac::Resolver::from_snapshot(
            gateway_groups::load_snapshot(&app.db).await.unwrap(),
            vec![],
        ));
        let state = RamaState::new(
            app.clone(),
            aiplane_core::rama_server::SessionStore::new(app.db.clone(), [4; 32]),
            aiplane_core::server::usage::UsageHandle::disabled(),
        );
        let mut tx = WriteTx::begin(&state.db).await.unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            let scope = grant_scope_in(&state, &mut tx, "manager", GrantKind::Model, "vip-model", None).await.unwrap().unwrap();
            assert_eq!(scope.pools.unwrap().into_iter().collect::<BTreeSet<_>>(), BTreeSet::from(["open".into(), "vip".into()]));
            assert!(grant_scope_in(&state, &mut tx, "admin", GrantKind::Model, "vip-model", None).await.unwrap().unwrap().pools.is_none());
            assert!(grant_scope_in(&state, &mut tx, "missing-user", GrantKind::Model, "vip-model", None).await.unwrap().is_none());
            assert!(grant_scope_in(&state, &mut tx, "manager", GrantKind::Connector, "actual", None).await.unwrap().is_some());
            assert!(grant_scope_in(&state, &mut tx, "manager", GrantKind::RagCollection, &collection.id.to_string(), None).await.unwrap().is_some());
            sqlx::query("UPDATE mcp_catalog_connectors SET enabled=0 WHERE key='actual'").execute(&mut *tx).await.unwrap();
            assert!(matches!(grant_scope_in(&state, &mut tx, "manager", GrantKind::Connector, "actual", None).await, Err(HoldRefusal::Missing(_))));
            sqlx::query("UPDATE rag_collections SET allowed_groups='[\"other\"]' WHERE id=?").bind(collection.id).execute(&mut *tx).await.unwrap();
            assert!(grant_scope_in(&state, &mut tx, "manager", GrantKind::RagCollection, &collection.id.to_string(), None).await.unwrap().is_none());
            sqlx::query("DELETE FROM oidc_group_mappings WHERE gateway_group='vip'").execute(&mut *tx).await.unwrap();
            assert!(grant_scope_in(&state, &mut tx, "manager", GrantKind::Model, "vip-model", None).await.unwrap().is_none());
            sqlx::query("UPDATE automatic_route_candidates SET target='alternate-model' WHERE route_alias='auto' AND key='vip-model'").execute(&mut *tx).await.unwrap();
            let scope = grant_scope_in(&state, &mut tx, "manager", GrantKind::Model, "auto", None).await.unwrap().unwrap();
            assert_eq!(scope.pools.unwrap().into_iter().collect::<BTreeSet<_>>(), BTreeSet::from(["open".into(), "selector".into()]));
            sqlx::query("UPDATE users SET roles_json='[]' WHERE id='admin'").execute(&mut *tx).await.unwrap();
            assert!(grant_scope_in(&state, &mut tx, "admin", GrantKind::Model, "vip-model", None).await.unwrap().is_none());
        }).await.expect("transactional grant reads must use the held connection");
        drop(tx);
        assert!(
            holds(
                &state,
                &users::find_by_id(&state.db, "manager")
                    .await
                    .unwrap()
                    .unwrap(),
                GrantKind::Model,
                "vip-model"
            )
            .await
            .unwrap()
        );
    }

    #[tokio::test]
    async fn transactional_a2a_grant_requires_current_organization_membership_and_share() {
        let (state, _, dot) = crate::agents::access::tests::fixture().await;
        let mut tx = aiplane_core::server::db::WriteTx::begin(&state.db)
            .await
            .unwrap();
        assert!(
            grant_scope_in(&state, &mut tx, "creator", GrantKind::A2aCaller, &dot, None)
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            grant_scope_in(
                &state,
                &mut tx,
                "outsider",
                GrantKind::A2aCaller,
                &dot,
                None
            )
            .await
            .unwrap()
            .is_none()
        );
        sqlx::query("DELETE FROM oidc_group_mappings WHERE gateway_group='members'")
            .execute(&mut *tx)
            .await
            .unwrap();
        assert!(
            grant_scope_in(&state, &mut tx, "creator", GrantKind::A2aCaller, &dot, None)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            grant_scope_in(&state, &mut tx, "admin", GrantKind::A2aCaller, &dot, None)
                .await
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn every_public_kind_is_grantable_and_ocr_is_not() {
        assert_eq!(
            grantable_kinds().collect::<Vec<_>>(),
            vec![
                PoolKind::Chat,
                PoolKind::Transcription,
                PoolKind::Embedding,
                PoolKind::Image,
                PoolKind::Speech,
                PoolKind::Rerank,
                PoolKind::SystemOne,
            ]
        );
    }

    fn entry(granted: &GrantSet) -> Capped {
        Capped {
            at: Instant::now(),
            minted_by: "m".into(),
            granted: Arc::new(granted.clone()),
            capped: Arc::new(GrantSet::default()),
        }
    }

    #[test]
    fn a_cap_worked_out_before_an_invalidation_is_not_kept_after_it() {
        let caps = GrantCaps::default();
        let granted = GrantSet::new([(GrantKind::Tool, "time".to_string())]);
        let (epoch, hit) = caps.get("t1", "m", &granted);
        assert!(hit.is_none());
        caps.invalidate();
        caps.put(epoch, "t1", entry(&granted));
        assert!(caps.get("t1", "m", &granted).1.is_none());

        let (epoch, _) = caps.get("t1", "m", &granted);
        caps.put(epoch, "t1", entry(&granted));
        assert!(caps.get("t1", "m", &granted).1.is_some());
        assert!(
            caps.get("t1", "m", &GrantSet::default()).1.is_none(),
            "a changed grant set misses"
        );
        assert!(caps.get("t1", "other", &granted).1.is_none());
        caps.invalidate();
        assert!(caps.get("t1", "m", &granted).1.is_none());
    }

    #[test]
    fn a_tokens_last_use_is_written_at_most_once_a_minute() {
        let caps = GrantCaps::default();
        assert!(caps.touch_due("t1"));
        assert!(!caps.touch_due("t1"));
        assert!(caps.touch_due("t2"));
    }
    async fn organization_connector_fixture() -> (RamaState, String, String) {
        use aiplane_agents::db::{agents::DotOwner, dot_connections};
        use aiplane_core::server::db::{gateway_groups, mcp_connections};
        let (state, organization, dot) = crate::agents::access::tests::fixture().await;
        gateway_groups::set_tools_for_group(&state.db, "members", &["mcp__own-workspace".into()])
            .await
            .unwrap();
        sqlx::query("INSERT INTO mcp_catalog_connectors(key,name,url,auth,scope,use_dcr,client_id,enabled,allowed_groups,created_at,updated_at) VALUES('own-workspace','Actual Workspace','https://workspace.example.test/mcp','oauth2','per_user',0,'client',1,'[\"members\"]','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')")
            .execute(&state.db).await.unwrap();
        let catalog = mcp_catalog::get(&state.db, "own-workspace")
            .await
            .unwrap()
            .unwrap();
        let identity = mcp_connections::upsert_connection(
            &state.db,
            mcp_connections::NewConnection {
                owner: mcp_connections::ConnectionOwner::Principal(dot.clone()),
                connector_key: catalog.key,
                catalog_generation: catalog.dispatch_generation,
                access_token_ct: vec![1],
                access_token_nonce: vec![2],
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
        .unwrap()
        .unwrap();
        let owner = DotOwner::Organization(organization.clone());
        let mut tx = WriteTx::begin(&state.db).await.unwrap();
        assert!(matches!(
            dot_connections::save_in(
                &mut tx,
                dot_connections::Selection {
                    owner: &owner,
                    dot_id: &dot,
                    connector_key: "own-workspace",
                    identity_mode: dot_connections::IdentityMode::Principal,
                    connection_generation: &identity.generation,
                    expected_binding_generation: None,
                }
            )
            .await
            .unwrap(),
            dot_connections::SaveOutcome::Saved(Some(_))
        ));
        tx.commit().await.unwrap();
        (state, organization, dot)
    }

    #[tokio::test]
    async fn organization_connector_holding_uses_selected_own_identity_before_connector_grant() {
        use aiplane_core::server::db::gateway_groups;
        let (state, organization, dot) = organization_connector_fixture().await;
        let grants = aiplane_agents::db::system_principals::grants(&state.db, &dot)
            .await
            .unwrap();
        assert!(
            grants
                .iter()
                .all(|grant| grant.kind != GrantKind::Connector)
        );
        let mut tx = WriteTx::begin(&state.db).await.unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            let (user, policy) =
                crate::agents::access::current_user_policy_in(&state, &mut tx, "creator")
                    .await
                    .unwrap()
                    .unwrap();
            assert!(!policy.can_manage_dots(&policy.role_ids_for(&user.roles)));
            assert!(
                grant_scope_in(
                    &state,
                    &mut tx,
                    "creator",
                    GrantKind::Connector,
                    "own-workspace",
                    Some(&dot)
                )
                .await
                .unwrap()
                .is_some()
            );
            assert_eq!(
                grantable_connectors_in(&state, &mut tx, "creator", Some(&dot))
                    .await
                    .unwrap(),
                BTreeSet::from(["own-workspace".into()])
            );
            assert!(
                grantable_connectors_in(&state, &mut tx, "creator", None)
                    .await
                    .unwrap()
                    .is_empty()
            );
            assert!(
                grant_scope_in(
                    &state,
                    &mut tx,
                    "creator",
                    GrantKind::Connector,
                    "own-workspace",
                    None
                )
                .await
                .is_err()
            );
            for user in ["outsider", "responder", "admin"] {
                assert!(
                    grant_scope_in(
                        &state,
                        &mut tx,
                        user,
                        GrantKind::Connector,
                        "own-workspace",
                        Some(&dot)
                    )
                    .await
                    .unwrap()
                    .is_none()
                );
            }
            sqlx::query("UPDATE dots SET active=0 WHERE principal_id=?")
                .bind(&dot)
                .execute(&mut *tx)
                .await
                .unwrap();
            assert!(
                grant_scope_in(
                    &state,
                    &mut tx,
                    "creator",
                    GrantKind::Connector,
                    "own-workspace",
                    Some(&dot)
                )
                .await
                .unwrap()
                .is_some()
            );
            sqlx::query("DELETE FROM organization_groups WHERE organization_id=?")
                .bind(&organization)
                .execute(&mut *tx)
                .await
                .unwrap();
            assert!(
                grantable_connectors_in(&state, &mut tx, "creator", Some(&dot))
                    .await
                    .unwrap()
                    .is_empty()
            );
        })
        .await
        .expect("current grant holding uses the authority transaction");
        drop(tx);
        gateway_groups::set_tools_for_group(&state.db, "members", &[])
            .await
            .unwrap();
        let mut tx = WriteTx::begin(&state.db).await.unwrap();
        assert!(
            grantable_connectors_in(&state, &mut tx, "creator", Some(&dot))
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn organization_connector_holding_rechecks_saved_binding_credentials_and_catalogue_epochs()
     {
        let (state, _, dot) = organization_connector_fixture().await;
        for mutation in [
            "DELETE FROM dot_connector_bindings WHERE dot_id=?",
            "UPDATE dot_connector_bindings SET identity_mode='user' WHERE dot_id=?",
            "UPDATE dot_connector_bindings SET connection_generation='obsolete' WHERE dot_id=?",
            "UPDATE mcp_connections SET status='reauth' WHERE principal_id=?",
            "UPDATE mcp_connections SET catalog_generation=NULL WHERE principal_id=?",
            "UPDATE mcp_connections SET generation='obsolete' WHERE principal_id=?",
        ] {
            let mut tx = WriteTx::begin(&state.db).await.unwrap();
            assert!(
                grantable_connectors_in(&state, &mut tx, "creator", Some(&dot))
                    .await
                    .unwrap()
                    .contains("own-workspace")
            );
            sqlx::query(mutation)
                .bind(&dot)
                .execute(&mut *tx)
                .await
                .unwrap();
            assert!(
                grantable_connectors_in(&state, &mut tx, "creator", Some(&dot))
                    .await
                    .unwrap()
                    .is_empty(),
                "{mutation}"
            );
            assert!(
                grant_scope_in(
                    &state,
                    &mut tx,
                    "creator",
                    GrantKind::Connector,
                    "own-workspace",
                    Some(&dot)
                )
                .await
                .unwrap()
                .is_none(),
                "{mutation}"
            );
            drop(tx);
        }
        let mut tx = WriteTx::begin(&state.db).await.unwrap();
        sqlx::query("UPDATE mcp_catalog_connectors SET dispatch_generation='changed' WHERE key='own-workspace'").execute(&mut *tx).await.unwrap();
        assert!(
            grantable_connectors_in(&state, &mut tx, "creator", Some(&dot))
                .await
                .unwrap()
                .is_empty()
        );
    }
}
