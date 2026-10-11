// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use crate::server::{
    AppState,
    tools::{ToolContext, ToolError},
};
use aiplane_agents::db::dot_connections;
use aiplane_core::server::db::{
    DbError, WriteTx,
    mcp_catalog::{self, AuthKind},
    mcp_connections::{self, ConnectionIdentity, ConnectionOwner},
    users::{self, User},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoredMcpIdentity {
    pub connection: ConnectionIdentity,
    pub binding_generation: Option<String>,
    pub catalog_generation: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum McpDispatchFacts {
    Owned(StoredMcpIdentity),
    Shared {
        connector_key: String,
        catalog_generation: String,
    },
}

impl McpDispatchFacts {
    fn connector_key(&self) -> &str {
        match self {
            Self::Owned(identity) => &identity.connection.connector_key,
            Self::Shared { connector_key, .. } => connector_key,
        }
    }

    fn catalog_generation(&self) -> &str {
        match self {
            Self::Owned(identity) => &identity.catalog_generation,
            Self::Shared {
                catalog_generation, ..
            } => catalog_generation,
        }
    }
}

pub async fn connection_admitted_in(
    state: &AppState,
    tx: &mut WriteTx,
    initiator: &str,
    owner: &ConnectionOwner,
    key: &str,
    subject: Option<&str>,
) -> Result<bool, DbError> {
    if !connection_owner_admitted_in(state, tx, initiator, owner, subject).await? {
        return Ok(false);
    }
    let Some((user, policy)) =
        crate::agents::access::current_user_policy_in(state, tx, initiator).await?
    else {
        return Ok(false);
    };
    let Some(connector) = mcp_catalog::get_in(tx, key).await? else {
        return Ok(false);
    };
    let roles = policy.role_ids_for(&user.roles);
    if !connector.enabled
        || !connector.is_per_user()
        || !connector.allows(&roles, policy.is_admin(&roles))
    {
        return Ok(false);
    }
    Ok(!matches!(owner, ConnectionOwner::Principal(_)) || connector.auth == AuthKind::OAuth2)
}

pub async fn connection_owner_admitted_in(
    state: &AppState,
    tx: &mut WriteTx,
    initiator: &str,
    owner: &ConnectionOwner,
    subject: Option<&str>,
) -> Result<bool, DbError> {
    use aiplane_agents::db::agents;
    let Some((user, policy)) =
        crate::agents::access::current_user_policy_in(state, tx, initiator).await?
    else {
        return Ok(false);
    };
    let groups = policy.role_ids_for(&user.roles);
    match owner {
        ConnectionOwner::User(id) => Ok(id == initiator && subject.is_none()),
        ConnectionOwner::Principal(id) => {
            if subject != Some(id.as_str()) {
                return Ok(false);
            }
            match agents::dot_owner_in(tx, id).await? {
                Some(agents::DotOwner::User(user_id)) => {
                    Ok(user_id == initiator && policy.can_manage_dots(&groups))
                }
                Some(agents::DotOwner::Organization(_)) => Ok(
                    crate::agents::access::organization_access_in(state, tx, id, initiator).await?
                        == Some(agents::Access::Write),
                ),
                None => Ok(false),
            }
        }
    }
}

pub async fn authorization_admitted_in(
    state: &AppState,
    tx: &mut WriteTx,
    initiator: &str,
    owner: &ConnectionOwner,
    key: &str,
    subject: Option<&str>,
) -> Result<bool, DbError> {
    if !connection_admitted_in(state, tx, initiator, owner, key, subject).await? {
        return Ok(false);
    }
    Ok(mcp_catalog::get_in(tx, key)
        .await?
        .is_some_and(|connector| connector.auth == AuthKind::OAuth2))
}

pub async fn selected_principal_identity_in(
    tx: &mut WriteTx,
    owner: &aiplane_agents::db::agents::DotOwner,
    dot: &str,
    key: &str,
) -> Result<Option<StoredMcpIdentity>, DbError> {
    if !dot_connections::owned_dot_in(tx, owner, dot, false).await? {
        return Ok(None);
    }
    let Some(connector) = mcp_catalog::get_in(tx, key).await? else {
        return Ok(None);
    };
    if !connector.enabled || !connector.is_per_user() || connector.auth != AuthKind::OAuth2 {
        return Ok(None);
    }
    let Some(binding) = dot_connections::get_in(tx, owner, dot, key).await? else {
        return Ok(None);
    };
    if binding.identity_mode != dot_connections::IdentityMode::Principal {
        return Ok(None);
    }
    let Some(connection) =
        mcp_connections::get_connection_in(tx, &ConnectionOwner::Principal(dot.to_owned()), key)
            .await?
    else {
        return Ok(None);
    };
    if connection.is_errored()
        || connection.access_token_ct.is_none()
        || connection.access_token_nonce.is_none()
        || connection.catalog_generation.as_deref() != Some(connector.dispatch_generation.as_str())
        || binding.connection_generation != connection.generation
    {
        return Ok(None);
    }
    Ok(Some(StoredMcpIdentity {
        connection: connection.identity(),
        binding_generation: Some(binding.binding_generation),
        catalog_generation: connector.dispatch_generation,
    }))
}

pub async fn selected_personal_connection(
    state: &AppState,
    owner: &User,
    dot: &str,
    key: &str,
    require_active: bool,
) -> Result<Option<StoredMcpIdentity>, ToolError> {
    let mut tx = WriteTx::begin(&state.db).await.map_err(load_error)?;
    let selected = selected_in(state, &mut tx, &owner.id, dot, key, require_active)
        .await
        .map_err(load_error)?;
    tx.commit().await.map_err(load_error)?;
    Ok(selected)
}

async fn selected_in(
    state: &AppState,
    tx: &mut WriteTx,
    user_id: &str,
    dot: &str,
    key: &str,
    require_active: bool,
) -> Result<Option<StoredMcpIdentity>, DbError> {
    let Some(user) = users::find_by_id_in(tx, user_id).await? else {
        return Ok(None);
    };
    let owner = aiplane_agents::db::agents::DotOwner::User(user_id.to_owned());
    if !dot_connections::owned_dot_in(tx, &owner, dot, require_active).await? {
        return Ok(None);
    }
    let Some(connector) = mcp_catalog::get_in(tx, key).await? else {
        return Ok(None);
    };
    let roles = state.role_ids_for(&user.roles);
    if !connector.enabled
        || !connector.is_per_user()
        || !connector.allows(&roles, state.rbac.is_admin(&roles))
    {
        return Ok(None);
    }
    let binding = dot_connections::get_in(tx, &owner, dot, key).await?;
    if binding
        .as_ref()
        .is_some_and(|binding| binding.identity_mode == dot_connections::IdentityMode::Principal)
    {
        return selected_principal_identity_in(tx, &owner, dot, key).await;
    }
    let owner = ConnectionOwner::User(user_id.to_owned());
    let Some(connection) = mcp_connections::get_connection_in(tx, &owner, key).await? else {
        return Ok(None);
    };
    if connection.catalog_generation.as_deref() != Some(connector.dispatch_generation.as_str())
        || connection.is_errored()
        || connection.access_token_ct.is_none()
        || connection.access_token_nonce.is_none()
        || binding
            .as_ref()
            .is_some_and(|b| b.connection_generation != connection.generation)
    {
        return Ok(None);
    }
    Ok(Some(StoredMcpIdentity {
        connection: connection.identity(),
        binding_generation: binding.map(|b| b.binding_generation),
        catalog_generation: connector.dispatch_generation,
    }))
}

async fn organization_selection_in(
    tx: &mut WriteTx,
    run: &crate::agent_run::AgentRun,
    key: &str,
) -> Result<Option<StoredMcpIdentity>, ToolError> {
    let principal = crate::agents::profile::validate_organization_turn_in(tx, run)
        .await
        .map_err(|error| ToolError::Failed(error.to_string()))?;
    if !principal
        .grants
        .has(aiplane_core::server::principal::GrantKind::Connector, key)
    {
        return Ok(None);
    }
    let context = run.organization().ok_or_else(|| unavailable(key))?;
    selected_principal_identity_in(
        tx,
        &aiplane_agents::db::agents::DotOwner::Organization(context.organization_id.clone()),
        &principal.id,
        key,
    )
    .await
    .map_err(load_error)
}

pub(crate) async fn validate_organization_identity_in(
    tx: &mut WriteTx,
    ctx: &ToolContext,
    expected: &StoredMcpIdentity,
) -> Result<(), ToolError> {
    let key = &expected.connection.connector_key;
    let run = ctx.agent.as_ref().ok_or_else(|| unavailable(key))?;
    let context = run.organization().ok_or_else(|| unavailable(key))?;
    if !matches!(&ctx.principal, aiplane_core::server::principal::Principal::System(principal)
        if principal.id == run.system_principal().id)
        || ctx.session_id.as_deref() != Some(context.session_id.as_str())
        || ctx.assistant_turn_id.as_deref() != Some(context.assistant_turn_id.as_str())
        || organization_selection_in(tx, run, key).await?.as_ref() != Some(expected)
    {
        return Err(unavailable(key));
    }
    Ok(())
}

pub async fn layer_for_organization_run(
    state: &AppState,
    run: &crate::agent_run::AgentRun,
) -> Result<super::manager::UserMcpLayer, ToolError> {
    let mut tx = WriteTx::begin(&state.db).await.map_err(load_error)?;
    let principal = crate::agents::profile::validate_organization_turn_in(&mut tx, run)
        .await
        .map_err(|error| ToolError::Failed(error.to_string()))?;
    let context = run
        .organization()
        .ok_or_else(|| unavailable(&principal.id))?;
    let owner = aiplane_agents::db::agents::DotOwner::Organization(context.organization_id.clone());
    let mut selected = Vec::new();
    for connector in mcp_catalog::list_enabled_in(&mut tx)
        .await
        .map_err(load_error)?
    {
        if !principal.grants.has(
            aiplane_core::server::principal::GrantKind::Connector,
            &connector.key,
        ) {
            continue;
        }
        if let Some(identity) =
            selected_principal_identity_in(&mut tx, &owner, &principal.id, &connector.key)
                .await
                .map_err(load_error)?
        {
            selected.push((connector, identity));
        }
    }
    tx.commit().await.map_err(load_error)?;
    let mut layer = super::manager::UserMcpLayer::default();
    for (connector, identity) in selected {
        let Ok(tools) = state
            .mcp
            .ensure_connection(&identity.connection.owner, &connector)
            .await
        else {
            continue;
        };
        let mut tx = WriteTx::begin(&state.db).await.map_err(load_error)?;
        let current = organization_selection_in(&mut tx, run, &connector.key).await?;
        tx.commit().await.map_err(load_error)?;
        if current.as_ref() != Some(&identity) {
            continue;
        }
        let tools = tools
            .into_iter()
            .filter_map(|tool| {
                let (McpDispatchFacts::Owned(current), rbac) = tool.identity.as_ref()? else {
                    return None;
                };
                (current.connection == identity.connection
                    && current.catalog_generation == identity.catalog_generation)
                    .then(|| {
                        std::sync::Arc::new(
                            (*tool)
                                .clone()
                                .with_identity(identity.clone(), rbac.clone()),
                        )
                    })
            })
            .collect::<Vec<_>>();
        layer.add(
            &connector.key,
            &tools,
            &std::collections::HashMap::new(),
            super::manager::AskOffer::AskFirst,
            connector.audit.then_some(&state.db),
        );
    }
    Ok(layer)
}

pub(crate) async fn validate_dispatch(
    ctx: &ToolContext,
    dispatch: &McpDispatchFacts,
    id: &str,
    rbac: &aiplane_core::server::rbac::Resolver,
) -> Result<(), ToolError> {
    let facts = Some(serde_json::json!(dispatch));
    super::super::ask_first::validate_execution_facts(ctx, id, &facts)?;
    if let Some(execution) = ctx.personal_execution.as_ref() {
        crate::agents::router::local::validate_execution(execution).await?;
        if ctx.personal_dot.as_ref() != Some(execution.identity()) {
            return Err(unavailable(id));
        }
    }
    let mut tx = WriteTx::begin(&ctx.db).await.map_err(load_error)?;
    let Some(connector) = mcp_catalog::get_in(&mut tx, dispatch.connector_key())
        .await
        .map_err(load_error)?
    else {
        return Err(unavailable(id));
    };
    if !connector.enabled || connector.dispatch_generation != dispatch.catalog_generation() {
        return Err(unavailable(id));
    }
    if ctx
        .granted_tools
        .as_ref()
        .is_some_and(|granted| !granted.contains(id))
    {
        return Err(unavailable(id));
    }
    match &ctx.principal {
        aiplane_core::server::principal::Principal::User { id: user_id, .. } => {
            let Some(user) = users::find_by_id_in(&mut tx, user_id)
                .await
                .map_err(load_error)?
            else {
                return Err(unavailable(id));
            };
            let roles = rbac.role_ids_for(&user.roles);
            if !connector.allows(&roles, rbac.is_admin(&roles))
                || !rbac
                    .mcp_grant(&roles, super::MCP_ID_PREFIX)
                    .allows(id, super::super::catalog::entry_key_for(id))
            {
                return Err(unavailable(id));
            }
            match dispatch {
                McpDispatchFacts::Owned(_) if connector.is_per_user() => {}
                McpDispatchFacts::Shared { .. }
                    if connector.scope == mcp_catalog::Scope::Global => {}
                _ => return Err(unavailable(id)),
            }
        }
        aiplane_core::server::principal::Principal::System(initial) => {
            let owner = aiplane_agents::db::agents::dot_owner_in(&mut tx, &initial.id)
                .await
                .map_err(load_error)?;
            if matches!(
                owner,
                Some(aiplane_agents::db::agents::DotOwner::Organization(_))
            ) || ctx
                .agent
                .as_ref()
                .is_some_and(|run| run.organization().is_some())
            {
                let McpDispatchFacts::Owned(identity) = dispatch else {
                    return Err(unavailable(id));
                };
                validate_organization_identity_in(&mut tx, ctx, identity).await?;
                tx.commit().await.map_err(load_error)?;
                return Ok(());
            }
            let Some(principal) =
                aiplane_agents::db::system_principals::load_active_in(&mut tx, &initial.id)
                    .await
                    .map_err(load_error)?
            else {
                return Err(unavailable(id));
            };
            if !matches!(dispatch, McpDispatchFacts::Shared { .. })
                || !connector.has_shared_identity()
                || !initial.grants.has(
                    aiplane_core::server::principal::GrantKind::Connector,
                    &connector.key,
                )
                || !principal.grants.has(
                    aiplane_core::server::principal::GrantKind::Connector,
                    &connector.key,
                )
            {
                return Err(unavailable(id));
            }
        }
    }
    let McpDispatchFacts::Owned(identity) = dispatch else {
        tx.commit().await.map_err(load_error)?;
        return Ok(());
    };
    let Some(connection) = mcp_connections::get_connection_in(
        &mut tx,
        &identity.connection.owner,
        &identity.connection.connector_key,
    )
    .await
    .map_err(load_error)?
    else {
        return Err(unavailable(id));
    };
    if connection.catalog_generation.as_deref() != Some(dispatch.catalog_generation())
        || connection.is_errored()
        || connection.identity() != identity.connection
        || connection.access_token_ct.is_none()
        || connection.access_token_nonce.is_none()
    {
        return Err(unavailable(id));
    }
    if let Some(dot) = &ctx.personal_dot {
        let execution = ctx
            .personal_execution
            .as_ref()
            .ok_or_else(|| unavailable(id))?;
        let selected = selected_in(
            &execution.state,
            &mut tx,
            &dot.owner_user_id,
            &dot.dot_id,
            &identity.connection.connector_key,
            true,
        )
        .await
        .map_err(load_error)?;
        if selected.as_ref() != Some(identity) {
            return Err(unavailable(id));
        }
    } else if !matches!((&ctx.principal, &identity.connection.owner), (aiplane_core::server::principal::Principal::User { id, .. }, ConnectionOwner::User(owner)) if id == owner)
        || identity.binding_generation.is_some()
    {
        return Err(unavailable(id));
    }
    tx.commit().await.map_err(load_error)?;
    Ok(())
}
fn unavailable(id: &str) -> ToolError {
    ToolError::Failed(format!(
        "`{id}` connection identity is unavailable or changed. Reconnect and request a new approval before retrying."
    ))
}
fn load_error(error: DbError) -> ToolError {
    ToolError::Failed(format!(
        "Could not verify the MCP connection identity: {error}. Retry after restoring access."
    ))
}

pub async fn layer_for_personal_dot(
    state: &AppState,
    owner: &User,
    dot: &str,
    ask: super::manager::AskContext<'_>,
    require_active: bool,
) -> super::manager::UserMcpLayer {
    let mut layer = super::manager::UserMcpLayer::default();
    let Ok(mut tx) = WriteTx::begin(&state.db).await else {
        return layer;
    };
    let Ok(true) = dot_connections::owned_dot_in(
        &mut tx,
        &aiplane_agents::db::agents::DotOwner::User(owner.id.clone()),
        dot,
        require_active,
    )
    .await
    else {
        return layer;
    };
    if tx.commit().await.is_err() {
        return layer;
    }
    let Ok(Some(current)) = users::find_by_id(&state.db, &owner.id).await else {
        return layer;
    };
    let roles = state.role_ids_for(&current.roles);
    let Ok(connectors) = mcp_catalog::list_enabled(&state.db).await else {
        return layer;
    };
    for connector in connectors {
        if !connector.allows(&roles, state.rbac.is_admin(&roles)) {
            continue;
        }
        let tools = if connector.is_per_user() {
            let Ok(Some(selected)) =
                selected_personal_connection(state, &current, dot, &connector.key, require_active)
                    .await
            else {
                continue;
            };
            match state
                .mcp
                .ensure_connection(&selected.connection.owner, &connector)
                .await
            {
                Ok(tools) => tools
                    .into_iter()
                    .filter_map(|tool| {
                        let (McpDispatchFacts::Owned(current), rbac) = tool.identity.as_ref()?
                        else {
                            return None;
                        };
                        (current.connection == selected.connection
                            && current.catalog_generation == selected.catalog_generation)
                            .then(|| {
                                std::sync::Arc::new(
                                    (*tool)
                                        .clone()
                                        .with_identity(selected.clone(), rbac.clone()),
                                )
                            })
                    })
                    .collect::<Vec<_>>(),
                Err(_) => continue,
            }
        } else if connector.is_global() {
            match state.mcp.ensure_global(&connector).await {
                Ok(tools) => tools,
                Err(_) => continue,
            }
        } else {
            continue;
        };
        let (asks, modes) = state
            .mcp
            .ask_and_modes(&owner.id, &connector.key, ask)
            .await;
        layer.add(
            &connector.key,
            &tools,
            &modes,
            asks,
            connector.audit.then_some(&state.db),
        );
    }
    layer
}

pub async fn save_selection_in(
    state: &AppState,
    tx: &mut WriteTx,
    initiator: &str,
    selection: dot_connections::Selection<'_>,
) -> Result<Option<dot_connections::SaveOutcome>, DbError> {
    use aiplane_agents::db::agents::{self, DotOwner};
    if agents::dot_owner_in(tx, selection.dot_id).await?.as_ref() != Some(selection.owner) {
        return Ok(None);
    }
    let principal_owner = ConnectionOwner::Principal(selection.dot_id.to_owned());
    if !connection_owner_admitted_in(
        state,
        tx,
        initiator,
        &principal_owner,
        Some(selection.dot_id),
    )
    .await?
    {
        return Ok(None);
    }
    let Some(connector) = mcp_catalog::get_in(tx, selection.connector_key).await? else {
        return Ok(Some(dot_connections::SaveOutcome::Missing));
    };
    let selected_owner = match (selection.owner, selection.identity_mode) {
        (DotOwner::User(user), dot_connections::IdentityMode::User) => {
            ConnectionOwner::User(user.clone())
        }
        _ => principal_owner,
    };
    let subject =
        matches!(selected_owner, ConnectionOwner::Principal(_)).then_some(selection.dot_id);
    if !connection_admitted_in(
        state,
        tx,
        initiator,
        &selected_owner,
        selection.connector_key,
        subject,
    )
    .await?
    {
        return Ok(None);
    }
    if matches!(selection.owner, DotOwner::Organization(_))
        && selection.identity_mode == dot_connections::IdentityMode::User
    {
        return Ok(Some(dot_connections::save_in(tx, selection).await?));
    }
    let current =
        mcp_connections::get_connection_in(tx, &selected_owner, selection.connector_key).await?;
    if !current.is_some_and(|connection| {
        connection.catalog_generation.as_deref() == Some(connector.dispatch_generation.as_str())
    }) {
        return Ok(Some(dot_connections::SaveOutcome::Unavailable));
    }
    Ok(Some(dot_connections::save_in(tx, selection).await?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::tools::ToolRegistry;
    use aiplane_agents::db::{agents, system_principals::NewPrincipal};
    use aiplane_core::server::{
        db,
        rbac::{
            Resolver,
            config::{RbacConfig, RoleConfig, RoleMapping},
        },
        upstreams::UpstreamRegistry,
    };
    use std::sync::Arc;

    async fn fixture() -> (
        AppState,
        User,
        String,
        ConnectionIdentity,
        ConnectionIdentity,
    ) {
        let db = db::open(std::path::Path::new(":memory:")).await.unwrap();
        let now = jiff::Timestamp::now();
        let user = User {
            id: "owner".into(),
            email: "owner@example.test".into(),
            name: None,
            roles: vec!["allowed".into()],
            created_at: now,
            updated_at: now,
            timezone: None,
            speech_voice: None,
        };
        users::upsert(&db, &user).await.unwrap();
        aiplane_core::server::db::gateway_groups::upsert_group(&db, "member", "", true, false)
            .await
            .unwrap();
        aiplane_core::server::db::gateway_groups::set_mappings_for_group(
            &db,
            "member",
            &["allowed".into()],
        )
        .await
        .unwrap();
        sqlx::query("INSERT INTO mcp_catalog_connectors(key,name,url,auth,scope,allowed_groups,enabled,created_at,updated_at,dispatch_generation) VALUES('fixture','Fixture','https://provider.test/mcp','oauth2','per_user','[\"member\"]',1,?,?,?)").bind(now.to_string()).bind(now.to_string()).bind(uuid::Uuid::new_v4().to_string()).execute(&db).await.unwrap();
        let (_, dot) = agents::create_personal_dot(
            &db,
            &NewPrincipal {
                name: "IdentityDot",
                display: "IdentityDot",
                description: "",
            },
            "{\"main\":{\"tools\":[\"mcp__fixture__lookup\"]}}",
            &user.id,
            4,
        )
        .await
        .unwrap();
        let dot = dot.unwrap().principal.id;
        let state = AppState::new(
            Default::default(),
            db.clone(),
            UpstreamRegistry::new(&Default::default()).unwrap(),
            Arc::new(ToolRegistry::new()),
            Arc::new(
                Resolver::build(
                    RbacConfig {
                        default_role: None,
                        mappings: vec![RoleMapping {
                            oidc_claim: "roles".into(),
                            oidc_value: "allowed".into(),
                            role: "member".into(),
                        }],
                    },
                    vec![RoleConfig {
                        id: "member".into(),
                        admin: true,
                        models: vec!["*".into()],
                        tools: vec!["*".into()],
                        skills: vec![],
                    }],
                )
                .unwrap(),
            ),
        );
        let catalog_generation = mcp_catalog::get(&db, "fixture")
            .await
            .unwrap()
            .unwrap()
            .dispatch_generation;
        let credential = |owner, secret: &str| {
            let sealed = state.crypto.seal(secret.as_bytes()).unwrap();
            mcp_connections::NewConnection {
                owner,
                connector_key: "fixture".into(),
                catalog_generation: catalog_generation.clone(),
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
            }
        };
        let personal = mcp_connections::upsert_connection(
            &db,
            credential(ConnectionOwner::User(user.id.clone()), "user-token"),
        )
        .await
        .unwrap();
        let own = mcp_connections::upsert_connection(
            &db,
            credential(ConnectionOwner::Principal(dot.clone()), "dot-token"),
        )
        .await
        .unwrap();
        (state, user, dot, personal.unwrap(), own.unwrap())
    }

    #[tokio::test]
    async fn personal_selection_preserves_implicit_user_and_keeps_inactive_configuration_separate()
    {
        let (state, user, dot, personal, own) = fixture().await;
        let implicit = selected_personal_connection(&state, &user, &dot, "fixture", true)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(implicit.connection, personal.clone());
        assert_eq!(implicit.binding_generation, None);
        let mut tx = WriteTx::begin(&state.db).await.unwrap();
        let Some(dot_connections::SaveOutcome::Saved(Some(binding))) = save_selection_in(
            &state,
            &mut tx,
            &user.id,
            dot_connections::Selection {
                owner: &agents::DotOwner::User(user.id.clone()),
                dot_id: &dot,
                connector_key: "fixture",
                identity_mode: dot_connections::IdentityMode::Principal,
                connection_generation: &own.generation,
                expected_binding_generation: None,
            },
        )
        .await
        .unwrap() else {
            panic!("owned identity selection is saved");
        };
        tx.commit().await.unwrap();
        let selected = selected_personal_connection(&state, &user, &dot, "fixture", true)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(selected.connection, own.clone());
        assert_eq!(
            selected.binding_generation,
            Some(binding.binding_generation)
        );
        sqlx::query("UPDATE dots SET active=0 WHERE principal_id=?")
            .bind(&dot)
            .execute(&state.db)
            .await
            .unwrap();
        assert!(
            selected_personal_connection(&state, &user, &dot, "fixture", true)
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            selected_personal_connection(&state, &user, &dot, "fixture", false)
                .await
                .unwrap(),
            Some(selected)
        );
        sqlx::query("UPDATE dots SET active=1 WHERE principal_id=?")
            .bind(&dot)
            .execute(&state.db)
            .await
            .unwrap();
        sqlx::query("UPDATE system_principals SET disabled_at=? WHERE id=?")
            .bind(jiff::Timestamp::now().to_string())
            .bind(&dot)
            .execute(&state.db)
            .await
            .unwrap();
        assert!(
            selected_personal_connection(&state, &user, &dot, "fixture", true)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            selected_personal_connection(&state, &user, &dot, "fixture", false)
                .await
                .unwrap()
                .is_some()
        );
        mcp_connections::delete_connection(&state.db, &own.owner, "fixture")
            .await
            .unwrap();
        assert!(
            selected_personal_connection(&state, &user, &dot, "fixture", false)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            mcp_connections::get_connection(&state.db, &personal.owner, "fixture")
                .await
                .unwrap()
                .is_some()
        );
        assert_eq!(
            dot_connections::list_for_dot(
                &state.db,
                &agents::DotOwner::User(user.id.clone()),
                &dot
            )
            .await
            .unwrap()
            .len(),
            1
        );
    }

    #[tokio::test]
    async fn oauth_admission_rechecks_current_owner_permissions_catalogue_and_exact_subject() {
        let (state, user, dot, _, _) = fixture().await;
        let owner = ConnectionOwner::Principal(dot.clone());
        let mut tx = WriteTx::begin(&state.db).await.unwrap();
        assert!(
            authorization_admitted_in(&state, &mut tx, &user.id, &owner, "fixture", Some(&dot))
                .await
                .unwrap()
        );
        assert!(
            !authorization_admitted_in(&state, &mut tx, &user.id, &owner, "fixture", None)
                .await
                .unwrap()
        );
        assert!(
            !authorization_admitted_in(&state, &mut tx, "other", &owner, "fixture", Some(&dot))
                .await
                .unwrap()
        );
        assert!(
            authorization_admitted_in(
                &state,
                &mut tx,
                &user.id,
                &ConnectionOwner::User(user.id.clone()),
                "fixture",
                None
            )
            .await
            .unwrap()
        );
        assert!(
            !authorization_admitted_in(
                &state,
                &mut tx,
                &user.id,
                &ConnectionOwner::User(user.id.clone()),
                "fixture",
                Some(&dot)
            )
            .await
            .unwrap()
        );
        sqlx::query("UPDATE dots SET active=0 WHERE principal_id=?")
            .bind(&dot)
            .execute(&mut *tx)
            .await
            .unwrap();
        assert!(
            authorization_admitted_in(&state, &mut tx, &user.id, &owner, "fixture", Some(&dot))
                .await
                .unwrap()
        );
        sqlx::query("UPDATE mcp_catalog_connectors SET scope='global' WHERE key='fixture'")
            .execute(&mut *tx)
            .await
            .unwrap();
        assert!(
            !authorization_admitted_in(&state, &mut tx, &user.id, &owner, "fixture", Some(&dot))
                .await
                .unwrap()
        );
        sqlx::query(
            "UPDATE mcp_catalog_connectors SET scope='per_user',enabled=0 WHERE key='fixture'",
        )
        .execute(&mut *tx)
        .await
        .unwrap();
        assert!(
            !authorization_admitted_in(&state, &mut tx, &user.id, &owner, "fixture", Some(&dot))
                .await
                .unwrap()
        );
        sqlx::query("UPDATE mcp_catalog_connectors SET enabled=1 WHERE key='fixture'")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("UPDATE users SET roles_json='[]' WHERE id='owner'")
            .execute(&mut *tx)
            .await
            .unwrap();
        assert!(
            !authorization_admitted_in(&state, &mut tx, &user.id, &owner, "fixture", Some(&dot))
                .await
                .unwrap()
        );
        assert!(
            save_selection_in(
                &state,
                &mut tx,
                &user.id,
                dot_connections::Selection {
                    owner: &agents::DotOwner::User(user.id.clone()),
                    dot_id: &dot,
                    connector_key: "fixture",
                    identity_mode: dot_connections::IdentityMode::User,
                    connection_generation: "stale",
                    expected_binding_generation: None
                }
            )
            .await
            .unwrap()
            .is_none()
        );
        tx.commit().await.unwrap();
        assert!(
            selected_personal_connection(&state, &user, &dot, "fixture", false)
                .await
                .unwrap()
                .is_none()
        );
    }

    async fn mcp_server() -> wiremock::MockServer {
        use serde_json::{Value, json};
        use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};
        let server = MockServer::start().await;
        Mock::given(method("POST")).respond_with(|request: &wiremock::Request| {
            let body:Value=serde_json::from_slice(&request.body).unwrap();
            let result=match body["method"].as_str() {
                Some("initialize") => json!({"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}}),
                Some("tools/list") => json!({"tools":[{"name":"lookup","inputSchema":{"type":"object"},"annotations":{"readOnlyHint":true}}]}),
                Some("tools/call") => json!({"content":[{"type":"text","text":"dispatched"}],"isError":false}),
                Some("notifications/initialized") => return ResponseTemplate::new(202),
                other => panic!("unexpected MCP method: {other:?}"),
            };
            ResponseTemplate::new(200).insert_header("content-type","application/json").set_body_json(json!({"jsonrpc":"2.0","id":body["id"],"result":result}))
        }).mount(&server).await;
        server
    }

    #[tokio::test]
    async fn personal_layer_requires_owned_active_dot_even_for_global_connectors() {
        use super::super::manager::AskContext;
        let (state, user, dot, _, _) = fixture().await;
        let server = mcp_server().await;
        sqlx::query("UPDATE mcp_catalog_connectors SET scope='global',auth='none',url=? WHERE key='fixture'").bind(server.uri()).execute(&state.db).await.unwrap();
        sqlx::query("UPDATE dots SET active=0 WHERE principal_id=?")
            .bind(&dot)
            .execute(&state.db)
            .await
            .unwrap();
        assert!(
            layer_for_personal_dot(&state, &user, &dot, AskContext::Chat, true)
                .await
                .tool_ids()
                .is_empty()
        );
        assert!(
            layer_for_personal_dot(&state, &user, "missing-dot", AskContext::Chat, false)
                .await
                .tool_ids()
                .is_empty()
        );
        assert_eq!(
            layer_for_personal_dot(&state, &user, &dot, AskContext::Chat, false)
                .await
                .tool_ids()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn personal_call_audit_uses_the_current_selected_mcp_identity_and_keeps_resume_original()
    {
        use crate::persona::personal_dot::{
            PersonalDotContext, PersonalTurnContext, initialize_for_turn,
        };
        use crate::server::tools::{ToolContext, ToolSource, injection::InjectionScan, runner};
        use serde_json::{Value, json};
        let (base, user, dot, personal, own) = fixture().await;
        let server = mcp_server().await;
        sqlx::query("UPDATE mcp_catalog_connectors SET url=? WHERE key='fixture'")
            .bind(server.uri())
            .execute(&base.db)
            .await
            .unwrap();
        let pools = serde_json::from_value(json!({"pool":{"kind":"chat","models":["selected"],"backend":[{"name":"fixture","base_url":server.uri()}]}})).unwrap();
        let state = AppState::new(
            Default::default(),
            base.db.clone(),
            UpstreamRegistry::new(&pools).unwrap(),
            Arc::new(ToolRegistry::new()),
            base.rbac.clone(),
        )
        .with_crypto(base.crypto.clone());
        let sessions = aiplane_core::rama_server::SessionStore::new(state.db.clone(), [7; 32]);
        let state = Arc::new(crate::rama_server::state::RamaState::new(
            state,
            sessions,
            aiplane_core::server::usage::UsageHandle::disabled(),
        ));
        let session = session_core::db::create_session(&state.db, &user.id)
            .await
            .unwrap();
        assert!(
            aiplane_agents::db::dot_sessions::link(&state.db, &user.id, &dot, &session.id)
                .await
                .unwrap()
        );
        session_core::db::create_turn_pair_if_idle(
            &state.db,
            &session.id,
            "user-turn",
            "assistant-turn",
            "Explicit task",
            "selected",
            true,
        )
        .await
        .unwrap()
        .unwrap();
        let Some(PersonalDotContext::Active { actor, persona }) = initialize_for_turn(
            &state,
            &user,
            true,
            PersonalTurnContext {
                session_id: &session.id,
                assistant_turn_id: "assistant-turn",
                model: "selected",
                clock: crate::budget::system_clock(),
                resume_checkpoint: None,
                child_execution: None,
            },
        )
        .await
        .unwrap() else {
            panic!("personal execution expected");
        };
        let execution = actor.personal_execution().unwrap().clone();
        let mut ctx = ToolContext {
            principal: actor.principal(),
            personal_dot: Some(execution.identity().clone()),
            personal_execution: Some(execution),
            session_id: Some(session.id.clone()),
            assistant_turn_id: Some("assistant-turn".into()),
            model: Some("selected".into()),
            granted_tools: Some(Arc::new(
                ["mcp__fixture__lookup".into()].into_iter().collect(),
            )),
            ..ToolContext::for_test(state.db.clone())
        };
        let initial_layer = layer_for_personal_dot(
            &state,
            &user,
            &dot,
            super::super::manager::AskContext::Chat,
            true,
        )
        .await;
        let registry = state.tools();
        let composite =
            super::super::manager::CompositeToolSource::new(registry.as_ref(), &initial_layer);
        let live_state = Arc::new((**state).clone());
        let source = persona.source(&composite, Some(&live_state));
        assert_eq!(
            source
                .get("mcp__fixture__lookup")
                .unwrap()
                .execution_facts(),
            Some(json!(McpDispatchFacts::Owned(StoredMcpIdentity {
                connection: personal.clone(),
                binding_generation: None,
                catalog_generation: mcp_catalog::get(&state.db, "fixture")
                    .await
                    .unwrap()
                    .unwrap()
                    .dispatch_generation,
            })))
        );
        let mut tx = WriteTx::begin(&state.db).await.unwrap();
        let Some(dot_connections::SaveOutcome::Saved(Some(binding))) = save_selection_in(
            &state,
            &mut tx,
            &user.id,
            dot_connections::Selection {
                owner: &agents::DotOwner::User(user.id.clone()),
                dot_id: &dot,
                connector_key: "fixture",
                identity_mode: dot_connections::IdentityMode::Principal,
                connection_generation: &own.generation,
                expected_binding_generation: None,
            },
        )
        .await
        .unwrap() else {
            panic!("own selection expected");
        };
        tx.commit().await.unwrap();
        let call = |id: &str| runner::ToolCallRef {
            id: id.into(),
            name: "mcp__fixture__lookup".into(),
            arguments_raw: "{}".into(),
        };
        let result = runner::execute_tool_calls(
            &source,
            &ctx,
            &[call("own-call")],
            &InjectionScan::default(),
        )
        .await;
        assert!(!result[0].failed);
        let events = aiplane_agents::db::agent_audit::for_principal(&state.db, &user.id)
            .await
            .unwrap();
        let event = events
            .iter()
            .find(|event| event.kind == "tool_call" && event.detail["call_id"] == "own-call")
            .unwrap();
        assert_eq!(
            event.detail["identity"],
            json!({"owner":own.owner,"connector_key":"fixture","connection_id":own.connection_id,"generation":own.generation,"binding_generation":binding.binding_generation,"catalog_generation":mcp_catalog::get(&state.db,"fixture").await.unwrap().unwrap().dispatch_generation})
        );
        let mut tx = WriteTx::begin(&state.db).await.unwrap();
        let Some(dot_connections::SaveOutcome::Saved(Some(user_binding))) = save_selection_in(
            &state,
            &mut tx,
            &user.id,
            dot_connections::Selection {
                owner: &agents::DotOwner::User(user.id.clone()),
                dot_id: &dot,
                connector_key: "fixture",
                identity_mode: dot_connections::IdentityMode::User,
                connection_generation: &personal.generation,
                expected_binding_generation: Some(&binding.binding_generation),
            },
        )
        .await
        .unwrap() else {
            panic!("user selection expected");
        };
        tx.commit().await.unwrap();
        let result = runner::execute_tool_calls(
            &source,
            &ctx,
            &[call("user-call")],
            &InjectionScan::default(),
        )
        .await;
        assert!(!result[0].failed);
        let events = aiplane_agents::db::agent_audit::for_principal(&state.db, &user.id)
            .await
            .unwrap();
        let event = events
            .iter()
            .find(|event| event.kind == "tool_call" && event.detail["call_id"] == "user-call")
            .unwrap();
        assert_eq!(event.detail["identity"]["owner"], json!(personal.owner));
        assert_eq!(
            event.detail["identity"]["binding_generation"],
            user_binding.binding_generation
        );
        ctx.pending_execution_facts = Some(crate::server::tools::PendingExecutionFacts {
            execution_facts: Some(json!(McpDispatchFacts::Owned(StoredMcpIdentity {
                connection: own,
                binding_generation: Some(binding.binding_generation),
                catalog_generation: mcp_catalog::get(&state.db, "fixture")
                    .await
                    .unwrap()
                    .unwrap()
                    .dispatch_generation,
            }))),
        });
        let result = runner::execute_tool_calls(
            &source,
            &ctx,
            &[call("own-call")],
            &InjectionScan::default(),
        )
        .await;
        assert!(result[0].failed);
        let events = aiplane_agents::db::agent_audit::for_principal(&state.db, &user.id)
            .await
            .unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == "tool_call")
                .count(),
            2
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM agent_audit WHERE principal_id=? AND kind='tool_result'"
            )
            .bind(&user.id)
            .fetch_one(&state.db)
            .await
            .unwrap(),
            3
        );
        assert!(
            events
                .iter()
                .all(|event| !event.detail.to_string().contains("user-token")
                    && !event.detail.to_string().contains("dot-token"))
        );
        assert_eq!(
            server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .filter(
                    |request| serde_json::from_slice::<Value>(&request.body).unwrap()["method"]
                        == "tools/call"
                )
                .count(),
            2
        );
    }

    #[tokio::test]
    async fn cached_mcp_tool_cannot_dispatch_after_disconnect_replacement_or_live_rights_withdrawal()
     {
        use crate::server::tools::Tool;
        use serde_json::{Value, json};
        let (state, user, _, personal, _) = fixture().await;
        let server = mcp_server().await;
        let connected = super::super::connect_http_server("fixture", &server.uri(), None)
            .await
            .unwrap();
        let tool = connected.tools.into_iter().next().unwrap().with_identity(
            StoredMcpIdentity {
                connection: personal.clone(),
                binding_generation: None,
                catalog_generation: mcp_catalog::get(&state.db, "fixture")
                    .await
                    .unwrap()
                    .unwrap()
                    .dispatch_generation,
            },
            state.rbac.clone(),
        );
        let ctx = ToolContext {
            principal: aiplane_core::server::principal::Principal::User {
                id: user.id.clone(),
                roles: user.roles.clone(),
            },
            ..ToolContext::for_test(state.db.clone())
        };
        assert!(tool.run(ctx.clone(), json!({})).await.is_ok());
        sqlx::query("UPDATE users SET roles_json='[]' WHERE id='owner'")
            .execute(&state.db)
            .await
            .unwrap();
        assert!(tool.run(ctx.clone(), json!({})).await.is_err());
        sqlx::query("UPDATE users SET roles_json='[\"allowed\"]' WHERE id='owner'")
            .execute(&state.db)
            .await
            .unwrap();
        sqlx::query("UPDATE mcp_catalog_connectors SET enabled=0 WHERE key='fixture'")
            .execute(&state.db)
            .await
            .unwrap();
        assert!(tool.run(ctx.clone(), json!({})).await.is_err());
        sqlx::query("UPDATE mcp_catalog_connectors SET enabled=1 WHERE key='fixture'")
            .execute(&state.db)
            .await
            .unwrap();
        mcp_connections::delete_connection(&state.db, &personal.owner, "fixture")
            .await
            .unwrap();
        assert!(tool.run(ctx.clone(), json!({})).await.is_err());
        let sealed = state.crypto.seal(b"replacement-token").unwrap();
        let catalog_generation = mcp_catalog::get(&state.db, "fixture")
            .await
            .unwrap()
            .unwrap()
            .dispatch_generation;
        let replacement = mcp_connections::upsert_connection(
            &state.db,
            mcp_connections::NewConnection {
                owner: personal.owner.clone(),
                connector_key: "fixture".into(),
                catalog_generation,
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
        let replacement = replacement.unwrap();
        assert_ne!(replacement.generation, personal.generation);
        assert!(tool.run(ctx, json!({})).await.is_err());
        let requests = server.received_requests().await.unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(
                    |request| serde_json::from_slice::<Value>(&request.body).unwrap()["method"]
                        == "tools/call"
                )
                .count(),
            1
        );
    }

    async fn organization_oauth_fixture() -> (crate::rama_server::state::RamaState, String, String)
    {
        let (state, organization, dot) = crate::agents::access::tests::fixture().await;
        mcp_catalog::create(
            &state.db,
            mcp_catalog::ConnectorInput {
                key: "organization-oauth".into(),
                name: "Actual organization OAuth".into(),
                description: None,
                icon: None,
                category: None,
                url: "https://provider.example.com/mcp".into(),
                auth: AuthKind::OAuth2,
                scope: mcp_catalog::Scope::PerUser,
                audit: false,
                use_dcr: true,
                client_id: None,
                client_secret_ct: None,
                client_secret_nonce: None,
                authorize_url: None,
                token_url: None,
                registration_url: None,
                scopes: vec![],
                allowed_groups: vec!["members".into()],
            },
        )
        .await
        .unwrap();
        mcp_catalog::set_enabled(&state.db, "organization-oauth", true)
            .await
            .unwrap();
        (state, organization, dot)
    }

    #[tokio::test]
    async fn organization_oauth_requires_member_write_without_personal_dot_management() {
        let (state, _, dot) = organization_oauth_fixture().await;
        let mut tx = WriteTx::begin(&state.db).await.unwrap();
        let (user, policy) =
            crate::agents::access::current_user_policy_in(&state, &mut tx, "creator")
                .await
                .unwrap()
                .unwrap();
        assert!(!policy.can_manage_dots(&policy.role_ids_for(&user.roles)));
        let owner = ConnectionOwner::Principal(dot.clone());
        assert!(
            authorization_admitted_in(
                &state,
                &mut tx,
                "creator",
                &owner,
                "organization-oauth",
                Some(&dot)
            )
            .await
            .unwrap()
        );
        assert!(
            !authorization_admitted_in(
                &state,
                &mut tx,
                "responder",
                &owner,
                "organization-oauth",
                Some(&dot)
            )
            .await
            .unwrap()
        );
        assert!(
            !authorization_admitted_in(
                &state,
                &mut tx,
                "outsider",
                &owner,
                "organization-oauth",
                Some(&dot)
            )
            .await
            .unwrap()
        );
        assert!(
            !authorization_admitted_in(
                &state,
                &mut tx,
                "admin",
                &owner,
                "organization-oauth",
                Some(&dot)
            )
            .await
            .unwrap()
        );
        sqlx::query("UPDATE dots SET active=0 WHERE principal_id=?")
            .bind(&dot)
            .execute(&mut *tx)
            .await
            .unwrap();
        assert!(
            authorization_admitted_in(
                &state,
                &mut tx,
                "creator",
                &owner,
                "organization-oauth",
                Some(&dot)
            )
            .await
            .unwrap()
        );
        assert!(
            !authorization_admitted_in(
                &state,
                &mut tx,
                "creator",
                &owner,
                "organization-oauth",
                None
            )
            .await
            .unwrap()
        );
        assert!(
            !authorization_admitted_in(
                &state,
                &mut tx,
                "creator",
                &owner,
                "organization-oauth",
                Some("unrelated-principal")
            )
            .await
            .unwrap()
        );
        assert!(
            !authorization_admitted_in(
                &state,
                &mut tx,
                "creator",
                &ConnectionOwner::User("creator".into()),
                "organization-oauth",
                Some(&dot)
            )
            .await
            .unwrap()
        );
    }

    #[tokio::test]
    async fn organization_local_connection_authority_survives_catalogue_restriction_but_not_membership_loss()
     {
        let (state, organization, dot) = organization_oauth_fixture().await;
        let owner = ConnectionOwner::Principal(dot.clone());
        let mut tx = WriteTx::begin(&state.db).await.unwrap();
        assert!(
            connection_owner_admitted_in(&state, &mut tx, "creator", &owner, Some(&dot))
                .await
                .unwrap()
        );
        sqlx::query("UPDATE mcp_catalog_connectors SET enabled=0 WHERE key='organization-oauth'")
            .execute(&mut *tx)
            .await
            .unwrap();
        assert!(
            connection_owner_admitted_in(&state, &mut tx, "creator", &owner, Some(&dot))
                .await
                .unwrap()
        );
        assert!(
            !authorization_admitted_in(
                &state,
                &mut tx,
                "creator",
                &owner,
                "organization-oauth",
                Some(&dot)
            )
            .await
            .unwrap()
        );
        assert!(
            !connection_owner_admitted_in(&state, &mut tx, "admin", &owner, Some(&dot))
                .await
                .unwrap()
        );
        assert!(
            !connection_owner_admitted_in(&state, &mut tx, "creator", &owner, None)
                .await
                .unwrap()
        );
        assert!(
            connection_owner_admitted_in(
                &state,
                &mut tx,
                "creator",
                &ConnectionOwner::User("creator".into()),
                None
            )
            .await
            .unwrap()
        );
        assert!(
            !connection_owner_admitted_in(
                &state,
                &mut tx,
                "outsider",
                &ConnectionOwner::User("creator".into()),
                None
            )
            .await
            .unwrap()
        );
        sqlx::query(
            "DELETE FROM organization_groups WHERE organization_id=? AND gateway_group='members'",
        )
        .bind(&organization)
        .execute(&mut *tx)
        .await
        .unwrap();
        assert!(
            !connection_owner_admitted_in(&state, &mut tx, "creator", &owner, Some(&dot))
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn organization_selection_requires_exact_owner_current_write_and_current_credential_provenance()
     {
        let (state, organization, dot) = organization_oauth_fixture().await;
        let owner = agents::DotOwner::Organization(organization.clone());
        let connector = mcp_catalog::get(&state.db, "organization-oauth")
            .await
            .unwrap()
            .unwrap();
        let sealed = state.crypto.seal(b"actual-test-credential").unwrap();
        let credential = mcp_connections::upsert_connection(
            &state.db,
            mcp_connections::NewConnection {
                owner: ConnectionOwner::Principal(dot.clone()),
                connector_key: connector.key.clone(),
                catalog_generation: connector.dispatch_generation,
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
        .unwrap()
        .unwrap();
        let mut tx = WriteTx::begin(&state.db).await.unwrap();
        let selection = dot_connections::Selection {
            owner: &owner,
            dot_id: &dot,
            connector_key: "organization-oauth",
            identity_mode: dot_connections::IdentityMode::Principal,
            connection_generation: &credential.generation,
            expected_binding_generation: None,
        };
        assert!(
            save_selection_in(&state, &mut tx, "outsider", selection)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            save_selection_in(&state, &mut tx, "admin", selection)
                .await
                .unwrap()
                .is_none()
        );
        let wrong_owner = agents::DotOwner::User(organization.clone());
        assert!(
            save_selection_in(
                &state,
                &mut tx,
                "creator",
                dot_connections::Selection {
                    owner: &wrong_owner,
                    ..selection
                }
            )
            .await
            .unwrap()
            .is_none()
        );
        assert_eq!(
            save_selection_in(
                &state,
                &mut tx,
                "creator",
                dot_connections::Selection {
                    identity_mode: dot_connections::IdentityMode::User,
                    ..selection
                }
            )
            .await
            .unwrap(),
            Some(dot_connections::SaveOutcome::Unavailable)
        );
        let Some(dot_connections::SaveOutcome::Saved(Some(binding))) =
            save_selection_in(&state, &mut tx, "creator", selection)
                .await
                .unwrap()
        else {
            panic!("explicit own credential must be selected");
        };
        let selection = dot_connections::Selection {
            expected_binding_generation: Some(&binding.binding_generation),
            ..selection
        };
        sqlx::query("UPDATE agent_shares SET access='respond' WHERE principal_id=? AND subject_kind='user' AND subject_id='creator'").bind(&dot).execute(&mut *tx).await.unwrap();
        assert!(
            save_selection_in(&state, &mut tx, "creator", selection)
                .await
                .unwrap()
                .is_none()
        );
        sqlx::query("UPDATE agent_shares SET access='write' WHERE principal_id=? AND subject_kind='user' AND subject_id='creator'").bind(&dot).execute(&mut *tx).await.unwrap();
        sqlx::query("UPDATE mcp_catalog_connectors SET dispatch_generation=? WHERE key='organization-oauth'").bind(uuid::Uuid::new_v4().to_string()).execute(&mut *tx).await.unwrap();
        assert_eq!(
            save_selection_in(&state, &mut tx, "creator", selection)
                .await
                .unwrap(),
            Some(dot_connections::SaveOutcome::Unavailable)
        );
        assert_eq!(
            dot_connections::get_in(&mut tx, &owner, &dot, "organization-oauth")
                .await
                .unwrap(),
            Some(binding)
        );
    }

    #[tokio::test]
    async fn organization_oauth_revocation_is_current_inside_the_authorization_transaction() {
        let (state, organization, dot) = organization_oauth_fixture().await;
        let owner = ConnectionOwner::Principal(dot.clone());
        let mut tx = WriteTx::begin(&state.db).await.unwrap();
        assert!(
            authorization_admitted_in(
                &state,
                &mut tx,
                "creator",
                &owner,
                "organization-oauth",
                Some(&dot)
            )
            .await
            .unwrap()
        );
        sqlx::query("UPDATE gateway_groups SET can_manage_agents=0 WHERE name='members'")
            .execute(&mut *tx)
            .await
            .unwrap();
        assert!(
            !authorization_admitted_in(
                &state,
                &mut tx,
                "creator",
                &owner,
                "organization-oauth",
                Some(&dot)
            )
            .await
            .unwrap()
        );
        sqlx::query("UPDATE gateway_groups SET can_manage_agents=1 WHERE name='members'")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("DELETE FROM oidc_group_mappings WHERE gateway_group='members'")
            .execute(&mut *tx)
            .await
            .unwrap();
        assert!(
            !authorization_admitted_in(
                &state,
                &mut tx,
                "creator",
                &owner,
                "organization-oauth",
                Some(&dot)
            )
            .await
            .unwrap()
        );
        sqlx::query("INSERT INTO oidc_group_mappings(oidc_claim,oidc_value,gateway_group) VALUES('groups','claim-members','members')").execute(&mut *tx).await.unwrap();
        sqlx::query(
            "DELETE FROM organization_groups WHERE organization_id=? AND gateway_group='members'",
        )
        .bind(&organization)
        .execute(&mut *tx)
        .await
        .unwrap();
        assert!(
            !authorization_admitted_in(
                &state,
                &mut tx,
                "creator",
                &owner,
                "organization-oauth",
                Some(&dot)
            )
            .await
            .unwrap()
        );
        sqlx::query(
            "INSERT INTO organization_groups(organization_id,gateway_group) VALUES(?,'members')",
        )
        .bind(&organization)
        .execute(&mut *tx)
        .await
        .unwrap();
        sqlx::query("UPDATE agent_shares SET access='respond' WHERE principal_id=? AND subject_kind='user' AND subject_id='creator'").bind(&dot).execute(&mut *tx).await.unwrap();
        assert!(
            !authorization_admitted_in(
                &state,
                &mut tx,
                "creator",
                &owner,
                "organization-oauth",
                Some(&dot)
            )
            .await
            .unwrap()
        );
        sqlx::query("UPDATE agent_shares SET access='write' WHERE principal_id=? AND subject_kind='user' AND subject_id='creator'").bind(&dot).execute(&mut *tx).await.unwrap();
        assert!(
            authorization_admitted_in(
                &state,
                &mut tx,
                "creator",
                &owner,
                "organization-oauth",
                Some(&dot)
            )
            .await
            .unwrap()
        );
        drop(tx);
        let mut tx = WriteTx::begin(&state.db).await.unwrap();
        assert!(
            authorization_admitted_in(
                &state,
                &mut tx,
                "creator",
                &owner,
                "organization-oauth",
                Some(&dot)
            )
            .await
            .unwrap()
        );
    }

    #[tokio::test]
    async fn organization_oauth_rejects_admin_recovery_even_with_catalogue_access() {
        let (state, _, dot) = organization_oauth_fixture().await;
        let mut tx = WriteTx::begin(&state.db).await.unwrap();
        assert_eq!(
            crate::agents::access::effective_access_in(&state, &mut tx, &dot, "admin")
                .await
                .unwrap(),
            Some(agents::Access::Write)
        );
        sqlx::query(
            "UPDATE mcp_catalog_connectors SET allowed_groups='[]' WHERE key='organization-oauth'",
        )
        .execute(&mut *tx)
        .await
        .unwrap();
        assert!(
            !authorization_admitted_in(
                &state,
                &mut tx,
                "admin",
                &ConnectionOwner::Principal(dot.clone()),
                "organization-oauth",
                Some(&dot)
            )
            .await
            .unwrap()
        );
    }

    #[tokio::test]
    async fn organization_oauth_keeps_equal_textual_user_and_organization_ids_distinct() {
        let (state, organization, org_dot) = organization_oauth_fixture().await;
        let now = jiff::Timestamp::now();
        users::upsert(
            &state.db,
            &User {
                id: organization.clone(),
                email: "same-id@example.com".into(),
                name: None,
                roles: vec!["claim-members".into()],
                created_at: now,
                updated_at: now,
                timezone: None,
                speech_voice: None,
            },
        )
        .await
        .unwrap();
        aiplane_core::server::db::gateway_groups::set_can_manage_dots(&state.db, "members", true)
            .await
            .unwrap();
        let (_, personal) = agents::create_personal_dot(
            &state.db,
            &NewPrincipal {
                name: "Actual personal assistant",
                display: "Actual personal assistant",
                description: "",
            },
            "{}",
            &organization,
            4,
        )
        .await
        .unwrap();
        let personal = personal.unwrap().principal.id;
        let mut tx = WriteTx::begin(&state.db).await.unwrap();
        assert!(
            authorization_admitted_in(
                &state,
                &mut tx,
                &organization,
                &ConnectionOwner::Principal(personal.clone()),
                "organization-oauth",
                Some(&personal)
            )
            .await
            .unwrap()
        );
        assert!(
            !authorization_admitted_in(
                &state,
                &mut tx,
                &organization,
                &ConnectionOwner::Principal(org_dot.clone()),
                "organization-oauth",
                Some(&org_dot)
            )
            .await
            .unwrap()
        );
        assert!(
            !authorization_admitted_in(
                &state,
                &mut tx,
                "creator",
                &ConnectionOwner::Principal(personal.clone()),
                "organization-oauth",
                Some(&personal)
            )
            .await
            .unwrap()
        );
        sqlx::query("UPDATE gateway_groups SET can_manage_dots=0 WHERE name='members'")
            .execute(&mut *tx)
            .await
            .unwrap();
        assert!(
            !authorization_admitted_in(
                &state,
                &mut tx,
                &organization,
                &ConnectionOwner::Principal(personal.clone()),
                "organization-oauth",
                Some(&personal)
            )
            .await
            .unwrap()
        );
        assert!(
            authorization_admitted_in(
                &state,
                &mut tx,
                "creator",
                &ConnectionOwner::Principal(org_dot.clone()),
                "organization-oauth",
                Some(&org_dot)
            )
            .await
            .unwrap()
        );
    }

    async fn stored_selection_credential(
        state: &AppState,
        owner: ConnectionOwner,
        key: &str,
    ) -> ConnectionIdentity {
        let connector = mcp_catalog::get(&state.db, key).await.unwrap().unwrap();
        let sealed = state
            .crypto
            .seal(b"actual-test-resource-credential")
            .unwrap();
        mcp_connections::upsert_connection(
            &state.db,
            mcp_connections::NewConnection {
                owner,
                connector_key: key.into(),
                catalog_generation: connector.dispatch_generation,
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
        .unwrap()
        .unwrap()
    }

    #[tokio::test]
    async fn selected_principal_identity_requires_explicit_binding_without_connector_grant_or_activity()
     {
        let (state, organization, dot) = organization_oauth_fixture().await;
        let owner = agents::DotOwner::Organization(organization);
        let credential = stored_selection_credential(
            &state,
            ConnectionOwner::Principal(dot.clone()),
            "organization-oauth",
        )
        .await;
        let mut tx = WriteTx::begin(&state.db).await.unwrap();
        assert!(
            selected_principal_identity_in(&mut tx, &owner, &dot, "organization-oauth")
                .await
                .unwrap()
                .is_none()
        );
        let dot_connections::SaveOutcome::Saved(Some(binding)) = dot_connections::save_in(
            &mut tx,
            dot_connections::Selection {
                owner: &owner,
                dot_id: &dot,
                connector_key: "organization-oauth",
                identity_mode: dot_connections::IdentityMode::Principal,
                connection_generation: &credential.generation,
                expected_binding_generation: None,
            },
        )
        .await
        .unwrap() else {
            panic!("explicit selection expected");
        };
        let principal = aiplane_agents::db::system_principals::load_active_in(&mut tx, &dot)
            .await
            .unwrap()
            .unwrap();
        assert!(!principal.grants.has(
            aiplane_core::server::principal::GrantKind::Connector,
            "organization-oauth"
        ));
        let selected = selected_principal_identity_in(&mut tx, &owner, &dot, "organization-oauth")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(selected.connection, credential);
        assert_eq!(
            selected.binding_generation,
            Some(binding.binding_generation)
        );
        sqlx::query("UPDATE dots SET active=0 WHERE principal_id=?")
            .bind(&dot)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("UPDATE system_principals SET disabled_at=? WHERE id=?")
            .bind(jiff::Timestamp::now().to_string())
            .bind(&dot)
            .execute(&mut *tx)
            .await
            .unwrap();
        assert_eq!(
            selected_principal_identity_in(&mut tx, &owner, &dot, "organization-oauth")
                .await
                .unwrap(),
            Some(selected)
        );
        drop(tx);
    }

    #[tokio::test]
    async fn selected_principal_identity_never_uses_user_or_other_dot_credentials_or_wrong_owner() {
        let (state, organization, dot) = organization_oauth_fixture().await;
        let owner = agents::DotOwner::Organization(organization.clone());
        stored_selection_credential(
            &state,
            ConnectionOwner::User("creator".into()),
            "organization-oauth",
        )
        .await;
        let (_, other) = agents::create_personal_dot(
            &state.db,
            &NewPrincipal {
                name: "Other account",
                display: "Other account",
                description: "",
            },
            "{}",
            "creator",
            4,
        )
        .await
        .unwrap();
        let other = other.unwrap().principal.id;
        let other_credential = stored_selection_credential(
            &state,
            ConnectionOwner::Principal(other),
            "organization-oauth",
        )
        .await;
        let mut tx = WriteTx::begin(&state.db).await.unwrap();
        assert!(
            selected_principal_identity_in(&mut tx, &owner, &dot, "organization-oauth")
                .await
                .unwrap()
                .is_none()
        );
        sqlx::query("INSERT INTO dot_connector_bindings(dot_id,connector_key,identity_mode,connection_generation,binding_generation,updated_at) VALUES(?,'organization-oauth','principal',?,'actual-test-binding',?)")
            .bind(&dot).bind(&other_credential.generation).bind(jiff::Timestamp::now().to_string())
            .execute(&mut *tx).await.unwrap();
        assert!(
            selected_principal_identity_in(&mut tx, &owner, &dot, "organization-oauth")
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            selected_principal_identity_in(
                &mut tx,
                &agents::DotOwner::User(organization),
                &dot,
                "organization-oauth"
            )
            .await
            .unwrap()
            .is_none()
        );
        sqlx::query("UPDATE dot_connector_bindings SET identity_mode='user' WHERE dot_id=?")
            .bind(&dot)
            .execute(&mut *tx)
            .await
            .unwrap();
        assert!(
            selected_principal_identity_in(&mut tx, &owner, &dot, "organization-oauth")
                .await
                .unwrap()
                .is_none()
        );
        drop(tx);
    }

    #[tokio::test]
    async fn selected_principal_identity_rechecks_current_resource_status_and_epochs_inside_transaction()
     {
        let (state, organization, dot) = organization_oauth_fixture().await;
        let owner = agents::DotOwner::Organization(organization);
        let credential = stored_selection_credential(
            &state,
            ConnectionOwner::Principal(dot.clone()),
            "organization-oauth",
        )
        .await;
        let mut tx = WriteTx::begin(&state.db).await.unwrap();
        assert!(matches!(
            dot_connections::save_in(
                &mut tx,
                dot_connections::Selection {
                    owner: &owner,
                    dot_id: &dot,
                    connector_key: "organization-oauth",
                    identity_mode: dot_connections::IdentityMode::Principal,
                    connection_generation: &credential.generation,
                    expected_binding_generation: None,
                }
            )
            .await
            .unwrap(),
            dot_connections::SaveOutcome::Saved(Some(_))
        ));
        let original = selected_principal_identity_in(&mut tx, &owner, &dot, "organization-oauth")
            .await
            .unwrap()
            .unwrap();
        for (mutation, restore) in [
            (
                "UPDATE mcp_catalog_connectors SET enabled=0 WHERE key='organization-oauth'",
                "UPDATE mcp_catalog_connectors SET enabled=1 WHERE key='organization-oauth'",
            ),
            (
                "UPDATE mcp_catalog_connectors SET auth='static_bearer' WHERE key='organization-oauth'",
                "UPDATE mcp_catalog_connectors SET auth='oauth2' WHERE key='organization-oauth'",
            ),
            (
                "UPDATE mcp_catalog_connectors SET scope='global' WHERE key='organization-oauth'",
                "UPDATE mcp_catalog_connectors SET scope='per_user' WHERE key='organization-oauth'",
            ),
            (
                "UPDATE mcp_connections SET status='reauth' WHERE connector_key='organization-oauth'",
                "UPDATE mcp_connections SET status='connected' WHERE connector_key='organization-oauth'",
            ),
        ] {
            sqlx::query(mutation).execute(&mut *tx).await.unwrap();
            assert!(
                selected_principal_identity_in(&mut tx, &owner, &dot, "organization-oauth")
                    .await
                    .unwrap()
                    .is_none(),
                "{mutation}"
            );
            sqlx::query(restore).execute(&mut *tx).await.unwrap();
            assert_eq!(
                selected_principal_identity_in(&mut tx, &owner, &dot, "organization-oauth")
                    .await
                    .unwrap(),
                Some(original.clone())
            );
        }
        sqlx::query(
            "UPDATE dot_connector_bindings SET connection_generation='stale' WHERE dot_id=?",
        )
        .bind(&dot)
        .execute(&mut *tx)
        .await
        .unwrap();
        assert!(
            selected_principal_identity_in(&mut tx, &owner, &dot, "organization-oauth")
                .await
                .unwrap()
                .is_none()
        );
        sqlx::query("UPDATE dot_connector_bindings SET connection_generation=? WHERE dot_id=?")
            .bind(&credential.generation)
            .bind(&dot)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("UPDATE mcp_catalog_connectors SET dispatch_generation='actual-new-resource' WHERE key='organization-oauth'")
            .execute(&mut *tx).await.unwrap();
        assert!(
            selected_principal_identity_in(&mut tx, &owner, &dot, "organization-oauth")
                .await
                .unwrap()
                .is_none()
        );
        drop(tx);
    }

    #[tokio::test]
    async fn selected_principal_identity_matches_personal_explicit_own_without_changing_implicit_user()
     {
        let (state, user, dot, personal, own) = fixture().await;
        let owner = agents::DotOwner::User(user.id.clone());
        let mut tx = WriteTx::begin(&state.db).await.unwrap();
        assert!(
            selected_principal_identity_in(&mut tx, &owner, &dot, "fixture")
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            selected_in(&state, &mut tx, &user.id, &dot, "fixture", false)
                .await
                .unwrap()
                .unwrap()
                .connection,
            personal
        );
        assert!(matches!(
            dot_connections::save_in(
                &mut tx,
                dot_connections::Selection {
                    owner: &owner,
                    dot_id: &dot,
                    connector_key: "fixture",
                    identity_mode: dot_connections::IdentityMode::Principal,
                    connection_generation: &own.generation,
                    expected_binding_generation: None,
                }
            )
            .await
            .unwrap(),
            dot_connections::SaveOutcome::Saved(Some(_))
        ));
        let shared = selected_principal_identity_in(&mut tx, &owner, &dot, "fixture")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(shared.connection, own);
        assert_eq!(
            selected_in(&state, &mut tx, &user.id, &dot, "fixture", false)
                .await
                .unwrap(),
            Some(shared)
        );
        drop(tx);
    }

    async fn organization_mcp_run_fixture(
        server: &wiremock::MockServer,
    ) -> (
        Arc<crate::rama_server::state::RamaState>,
        Arc<crate::agent_run::AgentRun>,
        ToolContext,
        StoredMcpIdentity,
    ) {
        let (base, organization, dot) = organization_oauth_fixture().await;
        sqlx::query("UPDATE mcp_catalog_connectors SET url=? WHERE key='organization-oauth'")
            .bind(server.uri())
            .execute(&base.db)
            .await
            .unwrap();
        let pools = serde_json::from_value(serde_json::json!({"pool":{"kind":"chat","models":["selected"],"backend":[{"name":"fixture","base_url":server.uri()}]}})).unwrap();
        let app = AppState::new(
            Default::default(),
            base.db.clone(),
            UpstreamRegistry::new(&pools).unwrap(),
            Arc::new(ToolRegistry::new()),
            base.rbac.clone(),
        )
        .with_crypto(base.crypto.clone());
        let state = Arc::new(crate::rama_server::state::RamaState::new(
            app,
            aiplane_core::rama_server::SessionStore::new(base.db.clone(), [11; 32]),
            aiplane_core::server::usage::UsageHandle::disabled(),
        ));
        let credential = stored_selection_credential(
            &state,
            ConnectionOwner::Principal(dot.clone()),
            "organization-oauth",
        )
        .await;
        let mut tx = WriteTx::begin(&state.db).await.unwrap();
        let owner = agents::DotOwner::Organization(organization);
        assert!(matches!(
            dot_connections::save_in(
                &mut tx,
                dot_connections::Selection {
                    owner: &owner,
                    dot_id: &dot,
                    connector_key: "organization-oauth",
                    identity_mode: dot_connections::IdentityMode::Principal,
                    connection_generation: &credential.generation,
                    expected_binding_generation: None,
                }
            )
            .await
            .unwrap(),
            dot_connections::SaveOutcome::Saved(Some(_))
        ));
        let selected = selected_principal_identity_in(&mut tx, &owner, &dot, "organization-oauth")
            .await
            .unwrap()
            .unwrap();
        tx.commit().await.unwrap();
        for (kind, reference) in [
            (
                aiplane_core::server::principal::GrantKind::Model,
                "selected",
            ),
            (
                aiplane_core::server::principal::GrantKind::Connector,
                "organization-oauth",
            ),
        ] {
            aiplane_agents::db::system_principals::add_grant(
                &state.db, &dot, kind, reference, "creator",
            )
            .await
            .unwrap();
        }
        let profile = crate::agents::profile::RunProfile::load_from(
            &state,
            &dot,
            crate::agents::profile::SpecSource::Draft(
                serde_json::json!({"main":{"model":"selected"}}),
            ),
            crate::agents::profile::Role::Main,
            &Default::default(),
        )
        .await
        .unwrap();
        let (session, assistant) = crate::server::headless::open_session(
            &state.db,
            crate::server::headless::OpenParams {
                owner: crate::server::headless::Owner::Run {
                    principal_id: &dot,
                    parent_turn_id: None,
                    agent_version: Some(agents::DRAFT_VERSION),
                },
                title: "Actual organization draft",
                prompt: "Actual task",
                model: "selected",
                existing_session: None,
                dot_id: None,
            },
        )
        .await
        .unwrap();
        let chain = Arc::new(aiplane_core::server::run_chain::RunChain::root(
            &session,
            None,
            aiplane_core::server::run_chain::Frame::for_principal(&profile.principal, None),
        ));
        let params = profile.drive_params(&session, &assistant, chain).unwrap();
        let crate::agent_run::Actor::Agent(run) = params.actor else {
            panic!("principal actor expected");
        };
        let ctx = ToolContext {
            principal: run.principal(),
            agent: Some(run.clone()),
            session_id: Some(session),
            assistant_turn_id: Some(assistant),
            suspend: crate::suspend::Suspend::Available,
            ..ToolContext::for_test(state.db.clone())
        };
        (state, run, ctx, selected)
    }

    #[tokio::test]
    async fn organization_selected_layer_dispatches_only_own_and_preserves_original_pending_facts()
    {
        use crate::server::tools::{PendingExecutionFacts, ToolSource};
        use serde_json::json;
        let server = mcp_server().await;
        let (state, run, ctx, selected) = organization_mcp_run_fixture(&server).await;
        stored_selection_credential(
            &state,
            ConnectionOwner::User("creator".into()),
            "organization-oauth",
        )
        .await;
        let layer = state.mcp_layer_for_run(&run).await.unwrap();
        assert_eq!(
            layer.tool_ids(),
            vec!["mcp__organization-oauth__lookup".to_owned()]
        );
        let held = layer.get("mcp__organization-oauth__lookup").unwrap();
        let facts = json!(McpDispatchFacts::Owned(selected.clone()));
        assert_eq!(held.execution_facts(), Some(facts.clone()));
        held.run(ctx.clone(), json!({})).await.unwrap();
        let resumed = ToolContext {
            pending_execution_facts: Some(PendingExecutionFacts {
                execution_facts: Some(facts),
            }),
            ..ctx.clone()
        };
        held.run(resumed.clone(), json!({})).await.unwrap();
        let mut tx = WriteTx::begin(&state.db).await.unwrap();
        sqlx::query("UPDATE dot_connector_bindings SET binding_generation='actual-reselection' WHERE dot_id=?")
            .bind(&run.system_principal().id).execute(&mut *tx).await.unwrap();
        assert!(
            validate_organization_identity_in(&mut tx, &resumed, &selected)
                .await
                .is_err()
        );
        tx.commit().await.unwrap();
        assert!(held.run(resumed, json!({})).await.is_err());
        let calls = server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .filter(|request| {
                serde_json::from_slice::<serde_json::Value>(&request.body).unwrap()["method"]
                    == "tools/call"
            })
            .count();
        assert_eq!(calls, 2);
    }

    #[tokio::test]
    async fn organization_selected_layer_never_falls_back_to_granted_shared_or_employee_connection()
    {
        let own = mcp_server().await;
        let trap = mcp_server().await;
        let (state, run, _, _) = organization_mcp_run_fixture(&own).await;
        stored_selection_credential(
            &state,
            ConnectionOwner::User("creator".into()),
            "organization-oauth",
        )
        .await;
        mcp_catalog::create(
            &state.db,
            mcp_catalog::ConnectorInput {
                key: "shared-trap".into(),
                name: "Actual shared trap".into(),
                description: None,
                icon: None,
                category: None,
                url: trap.uri(),
                auth: AuthKind::None,
                scope: mcp_catalog::Scope::Agent,
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
        mcp_catalog::set_enabled(&state.db, "shared-trap", true)
            .await
            .unwrap();
        aiplane_agents::db::system_principals::add_grant(
            &state.db,
            &run.system_principal().id,
            aiplane_core::server::principal::GrantKind::Connector,
            "shared-trap",
            "creator",
        )
        .await
        .unwrap();
        let profile = crate::agents::profile::RunProfile::load_from(
            &state,
            &run.system_principal().id,
            crate::agents::profile::SpecSource::Draft(
                serde_json::json!({"main":{"model":"selected"}}),
            ),
            crate::agents::profile::Role::Main,
            &Default::default(),
        )
        .await
        .unwrap();
        let context = run.organization().unwrap();
        let params = profile
            .drive_params(
                &context.session_id,
                &context.assistant_turn_id,
                run.chain().clone(),
            )
            .unwrap();
        let crate::agent_run::Actor::Agent(run) = params.actor else {
            panic!("principal actor expected");
        };
        sqlx::query("DELETE FROM dot_connector_bindings WHERE dot_id=?")
            .bind(&run.system_principal().id)
            .execute(&state.db)
            .await
            .unwrap();
        assert!(state.mcp_layer_for_run(&run).await.unwrap().is_empty());
        assert!(own.received_requests().await.unwrap().is_empty());
        assert!(trap.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn organization_selected_mcp_held_reference_observes_current_connector_grant_and_dot_activation()
     {
        use crate::server::tools::ToolSource;
        let server = mcp_server().await;
        let (state, run, ctx, _) = organization_mcp_run_fixture(&server).await;
        let layer = state.mcp_layer_for_run(&run).await.unwrap();
        let held = layer.get("mcp__organization-oauth__lookup").unwrap();
        aiplane_agents::db::system_principals::remove_grant(
            &state.db,
            &run.system_principal().id,
            aiplane_core::server::principal::GrantKind::Connector,
            "organization-oauth",
            "creator",
        )
        .await
        .unwrap();
        assert!(held.run(ctx.clone(), serde_json::json!({})).await.is_err());
        assert!(state.mcp_layer_for_run(&run).await.unwrap().is_empty());
        aiplane_agents::db::system_principals::add_grant(
            &state.db,
            &run.system_principal().id,
            aiplane_core::server::principal::GrantKind::Connector,
            "organization-oauth",
            "creator",
        )
        .await
        .unwrap();
        sqlx::query("UPDATE dots SET active=0 WHERE principal_id=?")
            .bind(&run.system_principal().id)
            .execute(&state.db)
            .await
            .unwrap();
        assert!(held.run(ctx, serde_json::json!({})).await.is_err());
        assert!(state.mcp_layer_for_run(&run).await.is_err());
        assert!(server.received_requests().await.unwrap().iter().all(|request|
            serde_json::from_slice::<serde_json::Value>(&request.body).unwrap()["method"]!="tools/call"));
    }

    #[tokio::test]
    async fn organization_verifier_uses_own_source_and_refuses_revoked_facts_before_trusted_write()
    {
        let server = mcp_server().await;
        let (state, run, ctx, selected) = organization_mcp_run_fixture(&server).await;
        let schema = Arc::new(
            crate::agents::state::StateSchema::from_spec(&serde_json::json!({"state":{
                "verified":{"type":"string","set_by":["verifier:lookup"]}
            }}))
            .unwrap(),
        );
        let verifier = crate::agents::verifier::VerifierRun {
            state: state.clone(),
            principal: run.system_principal().clone(),
            schema,
            options: Default::default(),
        };
        crate::agents::verifier::call_connector(
            &verifier,
            &ctx,
            "organization-oauth",
            "lookup",
            serde_json::json!({}),
            false,
        )
        .await
        .unwrap();
        let admitted = ToolContext {
            pending_execution_facts: Some(crate::server::tools::PendingExecutionFacts {
                execution_facts: Some(serde_json::json!(McpDispatchFacts::Owned(selected))),
            }),
            ..ctx.clone()
        };
        aiplane_agents::db::system_principals::remove_grant(
            &state.db,
            &run.system_principal().id,
            aiplane_core::server::principal::GrantKind::Connector,
            "organization-oauth",
            "creator",
        )
        .await
        .unwrap();
        assert!(
            crate::agents::verifier::call_connector(
                &verifier,
                &admitted,
                "organization-oauth",
                "lookup",
                serde_json::json!({}),
                false
            )
            .await
            .is_err()
        );
        assert!(
            crate::agents::verifier::apply_writes(
                &verifier,
                &admitted,
                "lookup",
                "mcp__organization-oauth__lookup",
                &vec![(
                    "verified".into(),
                    crate::agents::verifier::WriteSource::ResultField("customer".into())
                )],
                &serde_json::from_value(serde_json::json!({"customer":"actual"})).unwrap(),
                &Default::default(),
                crate::agents::state::TrustedWriter::Verifier("lookup".into())
            )
            .await
            .is_err()
        );
        assert!(
            aiplane_agents::db::agent_state::for_session(
                &state.db,
                ctx.session_id.as_deref().unwrap()
            )
            .await
            .unwrap()
            .is_empty()
        );
        let calls = server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .filter(|request| {
                serde_json::from_slice::<serde_json::Value>(&request.body).unwrap()["method"]
                    == "tools/call"
            })
            .count();
        assert_eq!(calls, 1);
    }
    #[tokio::test]
    async fn organization_verifier_descriptors_capture_actual_send_check_and_lookup_dependencies() {
        let server = mcp_server().await;
        let (state, run, _, _) = organization_mcp_run_fixture(&server).await;
        let verifier = crate::agents::verifier::VerifierRun {
            state,
            principal: run.system_principal().clone(),
            schema: Arc::new(crate::agents::state::StateSchema::default()),
            options: Default::default(),
        };
        let spec = crate::agents::spec::AgentSpec::from_value(&serde_json::json!({"verifiers":{
            "otp":{"kind":"mcp_code","connector":"organization-oauth","email_slot":"email",
                "send_tool":"deliver","check_tool":"confirm","writes":{"verified":"result"}},
            "lookup":{"kind":"lookup","tool":"mcp__organization-oauth__lookup",
                "inputs":{"customer":{"const":"actual"}},"writes":{"verified":"result"},"assurance":"low"},
            "local":{"kind":"lookup","tool":"company_echo",
                "inputs":{"customer":{"const":"actual"}},"writes":{"verified":"result"},"assurance":"low"}
        }})).unwrap();
        let descriptors = crate::agents::verifier::tools(
            &crate::agents::verifier::Verifiers::from_spec(&spec),
            &verifier,
        );
        let dependencies = descriptors
            .into_iter()
            .map(|descriptor| {
                (
                    descriptor.tool.id().to_owned(),
                    descriptor.dependency_tool_ids,
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(
            dependencies["verify_otp_request_code"],
            vec![
                "mcp__organization-oauth__deliver",
                "mcp__organization-oauth__confirm"
            ]
        );
        assert_eq!(
            dependencies["verify_otp_submit_code"],
            vec!["mcp__organization-oauth__confirm"]
        );
        assert_eq!(
            dependencies["verify_lookup"],
            vec!["mcp__organization-oauth__lookup"]
        );
        assert_eq!(dependencies["verify_local"], vec!["company_echo"]);
    }
    async fn organization_approval_mcp_server(
        read_only: Option<bool>,
        destructive: bool,
    ) -> wiremock::MockServer {
        use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};
        let server = MockServer::start().await;
        Mock::given(method("POST")).respond_with(move |request:&wiremock::Request| {
            let body:serde_json::Value=serde_json::from_slice(&request.body).unwrap();
            let mut tool=serde_json::json!({"name":"lookup","inputSchema":{"type":"object"}});
            if let Some(read_only)=read_only {
                tool["annotations"]=serde_json::json!({"readOnlyHint":read_only,"destructiveHint":destructive});
            }
            let result=match body["method"].as_str() {
                Some("initialize")=>serde_json::json!({"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"approval fixture","version":"1"}}),
                Some("tools/list")=>serde_json::json!({"tools":[tool]}),
                Some("tools/call")=>serde_json::json!({"content":[{"type":"text","text":"dispatched"}],"isError":false}),
                Some("notifications/initialized")=>return ResponseTemplate::new(202),
                other=>panic!("unexpected method {other:?}"),
            };
            ResponseTemplate::new(200).insert_header("content-type","application/json")
                .set_body_json(serde_json::json!({"jsonrpc":"2.0","id":body["id"],"result":result}))
        }).mount(&server).await;
        server
    }

    async fn organization_approval_tool(
        server: &wiremock::MockServer,
        always_allow: bool,
    ) -> (
        Arc<crate::rama_server::state::RamaState>,
        ToolContext,
        Arc<dyn crate::server::tools::Tool>,
        StoredMcpIdentity,
    ) {
        let (state, previous, ctx, selected) = organization_mcp_run_fixture(server).await;
        let id = "mcp__organization-oauth__lookup";
        let mut draft = serde_json::json!({"main":{"model":"selected","tools":[id]}});
        if always_allow {
            draft["main"]["tool_resources"][id] = serde_json::json!({"permission":"always_allow"});
        }
        let profile = crate::agents::profile::RunProfile::load_from(
            &state,
            &previous.system_principal().id,
            crate::agents::profile::SpecSource::Draft(draft),
            crate::agents::profile::Role::Main,
            &Default::default(),
        )
        .await
        .unwrap();
        let params = profile
            .drive_params(
                ctx.session_id.as_deref().unwrap(),
                ctx.assistant_turn_id.as_deref().unwrap(),
                previous.chain().clone(),
            )
            .unwrap();
        let crate::agent_run::Actor::Agent(run) = params.actor else {
            panic!("agent expected")
        };
        let ctx = ToolContext {
            principal: run.principal(),
            agent: Some(run.clone()),
            ..ctx
        };
        let layer = state.mcp_layer_for_run(&run).await.unwrap();
        let source = crate::agents::profile::RunToolSource::new(&layer, Some(&run));
        let tool = crate::server::tools::ToolSource::get(&source, id).unwrap();
        (state, ctx, tool, selected)
    }

    #[tokio::test]
    async fn organization_normal_tool_defaults_ask_unknown_allow_positive_read_only_and_preserve_explicit_rules()
     {
        for (read_only, destructive, always_allow, should_ask) in [
            (None, false, false, true),
            (Some(true), false, false, false),
            (None, false, true, false),
            (Some(false), true, true, true),
        ] {
            let server = organization_approval_mcp_server(read_only, destructive).await;
            let (_, ctx, tool, selected) = organization_approval_tool(&server, always_allow).await;
            let body = tool.run(ctx, serde_json::json!({})).await.unwrap();
            let pause = crate::suspend::extract_suspend(&body);
            assert_eq!(
                pause.is_some(),
                should_ask,
                "readOnly={read_only:?}, destructive={destructive}, explicitAllow={always_allow}"
            );
            if let Some(pause) = pause {
                assert_eq!(pause.kind, session_core::db::SuspensionKind::Approval);
                assert_eq!(
                    pause.execution_facts,
                    Some(serde_json::json!(McpDispatchFacts::Owned(selected)))
                );
            }
            let calls = server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .filter(|request| {
                    serde_json::from_slice::<serde_json::Value>(&request.body).unwrap()["method"]
                        == "tools/call"
                })
                .count();
            assert_eq!(calls, usize::from(!should_ask));
        }
    }

    #[tokio::test]
    async fn organization_unknown_tool_durable_approval_retains_original_own_identity_and_runs_once()
     {
        use session_core::db::{self as chat, Decision, PendingCall, TurnSuspension};
        let server = organization_approval_mcp_server(None, false).await;
        let (state, ctx, tool, selected) = organization_approval_tool(&server, false).await;
        let request = crate::suspend::extract_suspend(
            &tool.run(ctx.clone(), serde_json::json!({})).await.unwrap(),
        )
        .expect("unknown metadata requires approval");
        let authority = crate::agents::profile::OrganizationResumeAuthority::capture(
            ctx.agent.as_ref().unwrap(),
        )
        .unwrap();
        let now = jiff::Timestamp::now();
        let pause = TurnSuspension {
            turn_id: ctx.assistant_turn_id.clone().unwrap(),
            request_id: uuid::Uuid::new_v4().to_string(),
            kind: request.kind,
            message: request.message,
            tool_call: PendingCall {
                id: "original-call".into(),
                name: tool.id().into(),
                arguments: "{}".into(),
                actions: request.actions,
                execution_facts: request.execution_facts,
            },
            tail: vec![],
            budget_used: Default::default(),
            child_turn: None,
            on_timeout: request.on_timeout,
            expires_at: now + jiff::SignedDuration::from_secs(300),
            created_at: now,
            run_context: Some(authority.attach_to(request.context).unwrap()),
        };
        assert!(chat::suspend_turn(&state.db, &pause).await.unwrap());
        let stored = chat::get_suspension(&state.db, &pause.turn_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            stored.tool_call.execution_facts,
            Some(serde_json::json!(McpDispatchFacts::Owned(selected)))
        );
        let claimed = crate::agents::resume::claim(
            &state,
            crate::agents::resume::AgentResume {
                agent_id: ctx.principal.subject_id(),
                session_id: ctx.session_id.as_deref().unwrap(),
                turn_id: &pause.turn_id,
                request_id: Some(&pause.request_id),
                decision: Decision::AllowOnce,
                by: crate::agents::resume::ResumedBy::Staff {
                    user_id: "creator".into(),
                },
            },
        )
        .await
        .unwrap();
        assert_eq!(claimed.turn_id(), pause.turn_id);
        let decided = ToolContext {
            suspend: crate::suspend::Suspend::Decided(stored.kind, Decision::AllowOnce),
            pending_execution_facts: Some(crate::server::tools::PendingExecutionFacts {
                execution_facts: stored.tool_call.execution_facts,
            }),
            pending_actions: Some(crate::server::tools::PendingActionFacts {
                actions: stored.tool_call.actions,
            }),
            ..ctx
        };
        assert!(
            crate::suspend::extract_suspend(
                &tool.run(decided, serde_json::json!({})).await.unwrap()
            )
            .is_none()
        );
        let calls = server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|request| {
                serde_json::from_slice::<serde_json::Value>(&request.body).unwrap()["method"]
                    == "tools/call"
            })
            .count();
        assert_eq!(calls, 1);
    }
}
