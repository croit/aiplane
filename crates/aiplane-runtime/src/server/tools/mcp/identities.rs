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
}

pub async fn authorization_admitted_in(
    state: &AppState,
    tx: &mut WriteTx,
    initiator: &str,
    owner: &ConnectionOwner,
    key: &str,
    subject: Option<&str>,
) -> Result<bool, DbError> {
    let Some(user) = users::find_by_id_in(tx, initiator).await? else {
        return Ok(false);
    };
    let Some(connector) = mcp_catalog::get_in(tx, key).await? else {
        return Ok(false);
    };
    let roles = state.role_ids_for(&user.roles);
    if !connector.enabled
        || !connector.is_per_user()
        || connector.auth != AuthKind::OAuth2
        || !connector.allows(&roles, state.rbac.is_admin(&roles))
    {
        return Ok(false);
    }
    match owner {
        ConnectionOwner::User(id) => Ok(id == initiator && subject.is_none()),
        ConnectionOwner::Principal(id) => Ok(subject == Some(id.as_str())
            && state.rbac.can_manage_dots(&roles)
            && dot_connections::owned_dot_in(tx, initiator, id, false).await?),
    }
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
    if !dot_connections::owned_dot_in(tx, user_id, dot, require_active).await? {
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
    let binding = dot_connections::get_in(tx, user_id, dot, key).await?;
    let owner = match binding.as_ref().map(|b| b.identity_mode) {
        Some(dot_connections::IdentityMode::Principal) if connector.auth == AuthKind::OAuth2 => {
            ConnectionOwner::Principal(dot.to_owned())
        }
        Some(dot_connections::IdentityMode::Principal) => return Ok(None),
        _ => ConnectionOwner::User(user_id.to_owned()),
    };
    let Some(connection) = mcp_connections::get_connection_in(tx, &owner, key).await? else {
        return Ok(None);
    };
    if connection.is_errored()
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
    }))
}

pub(crate) async fn validate_identity(
    ctx: &ToolContext,
    identity: &StoredMcpIdentity,
    id: &str,
    rbac: &aiplane_core::server::rbac::Resolver,
) -> Result<(), ToolError> {
    let facts = Some(serde_json::json!(identity));
    super::super::ask_first::validate_execution_facts(ctx, id, &facts)?;
    if let Some(execution) = ctx.personal_execution.as_ref() {
        crate::agents::router::local::validate_execution(execution).await?;
        if ctx.personal_dot.as_ref() != Some(execution.identity()) {
            return Err(unavailable(id));
        }
    }
    let mut tx = WriteTx::begin(&ctx.db).await.map_err(load_error)?;
    let aiplane_core::server::principal::Principal::User { id: user_id, .. } = &ctx.principal
    else {
        return Err(unavailable(id));
    };
    let Some(user) = users::find_by_id_in(&mut tx, user_id)
        .await
        .map_err(load_error)?
    else {
        return Err(unavailable(id));
    };
    let Some(connector) = mcp_catalog::get_in(&mut tx, &identity.connection.connector_key)
        .await
        .map_err(load_error)?
    else {
        return Err(unavailable(id));
    };
    let roles = rbac.role_ids_for(&user.roles);
    let family = super::super::catalog::entry_key_for(id);
    if !connector.enabled
        || !connector.is_per_user()
        || !connector.allows(&roles, rbac.is_admin(&roles))
        || !rbac
            .mcp_grant(&roles, super::MCP_ID_PREFIX)
            .allows(id, family)
    {
        return Err(unavailable(id));
    }

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
    if connection.is_errored()
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
    let Ok(true) = dot_connections::owned_dot_in(&mut tx, &owner.id, dot, require_active).await
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
                        let (current, rbac) = tool.identity.as_ref()?;
                        (current.connection == selected.connection).then(|| {
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

pub async fn save_personal_selection_in(
    state: &AppState,
    tx: &mut WriteTx,
    selection: dot_connections::Selection<'_>,
) -> Result<Option<dot_connections::SaveOutcome>, DbError> {
    let Some(user) = users::find_by_id_in(tx, selection.owner_user_id).await? else {
        return Ok(None);
    };
    let roles = state.role_ids_for(&user.roles);
    let Some(connector) = mcp_catalog::get_in(tx, selection.connector_key).await? else {
        return Ok(Some(dot_connections::SaveOutcome::Missing));
    };
    if !state.rbac.can_manage_dots(&roles)
        || !connector.enabled
        || !connector.is_per_user()
        || !connector.allows(&roles, state.rbac.is_admin(&roles))
        || (selection.identity_mode == dot_connections::IdentityMode::Principal
            && connector.auth != AuthKind::OAuth2)
    {
        return Ok(None);
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
        sqlx::query("INSERT INTO mcp_catalog_connectors(key,name,url,auth,scope,allowed_groups,enabled,created_at,updated_at) VALUES('fixture','Fixture','https://provider.test/mcp','oauth2','per_user','[\"member\"]',1,?,?)").bind(now.to_string()).bind(now.to_string()).execute(&db).await.unwrap();
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
        let credential = |owner, secret: &str| {
            let sealed = state.crypto.seal(secret.as_bytes()).unwrap();
            mcp_connections::NewConnection {
                owner,
                connector_key: "fixture".into(),
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
        (state, user, dot, personal, own)
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
        let Some(dot_connections::SaveOutcome::Saved(Some(binding))) = save_personal_selection_in(
            &state,
            &mut tx,
            dot_connections::Selection {
                owner_user_id: &user.id,
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
        sqlx::query("UPDATE personal_dots SET active=0 WHERE principal_id=?")
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
        sqlx::query("UPDATE personal_dots SET active=1 WHERE principal_id=?")
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
            dot_connections::list_for_dot(&state.db, &user.id, &dot)
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
        sqlx::query("UPDATE personal_dots SET active=0 WHERE principal_id=?")
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
            save_personal_selection_in(
                &state,
                &mut tx,
                dot_connections::Selection {
                    owner_user_id: &user.id,
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
        sqlx::query("UPDATE personal_dots SET active=0 WHERE principal_id=?")
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
            Some(json!(StoredMcpIdentity {
                connection: personal.clone(),
                binding_generation: None
            }))
        );
        let mut tx = WriteTx::begin(&state.db).await.unwrap();
        let Some(dot_connections::SaveOutcome::Saved(Some(binding))) = save_personal_selection_in(
            &state,
            &mut tx,
            dot_connections::Selection {
                owner_user_id: &user.id,
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
            json!({"owner":own.owner,"connector_key":"fixture","connection_id":own.connection_id,"generation":own.generation,"binding_generation":binding.binding_generation})
        );
        let mut tx = WriteTx::begin(&state.db).await.unwrap();
        let Some(dot_connections::SaveOutcome::Saved(Some(user_binding))) =
            save_personal_selection_in(
                &state,
                &mut tx,
                dot_connections::Selection {
                    owner_user_id: &user.id,
                    dot_id: &dot,
                    connector_key: "fixture",
                    identity_mode: dot_connections::IdentityMode::User,
                    connection_generation: &personal.generation,
                    expected_binding_generation: Some(&binding.binding_generation),
                },
            )
            .await
            .unwrap()
        else {
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
            execution_facts: Some(json!(StoredMcpIdentity {
                connection: own,
                binding_generation: Some(binding.binding_generation)
            })),
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
        let replacement = mcp_connections::upsert_connection(
            &state.db,
            mcp_connections::NewConnection {
                owner: personal.owner.clone(),
                connector_key: "fixture".into(),
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
}
