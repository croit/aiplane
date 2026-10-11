// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Personal Dot lifecycle endpoints, backed by the existing agent registry.

use std::sync::Arc;

use rama::http::service::web::extract::State;
use rama::http::{Request, Response, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::json;
use session_core::i18n::{Lang, t};

use super::{bad_request, internal, json_error, json_ok, not_found, raw_path_segment};
use aiplane_agents::db::agents::{self as agents_db, DotCreate};
use aiplane_agents::db::dot_delegation::{self, ConsentDirection, ConsentEdit, ConsentSaved};
use aiplane_agents::db::system_principals::NewPrincipal;
use aiplane_runtime::agents::spec::AgentSpec;
use aiplane_runtime::agents::spec::model::{ActionRule, Permission};
use aiplane_runtime::agents::spec::tool_access::ToolAccessSelection;
use aiplane_runtime::rama_server::state::RamaState;
use aiplane_runtime::server::tools::mcp::manager::{AskContext, CompositeToolSource};
use aiplane_runtime::server::tools::{ToolSource, catalog};

#[derive(Deserialize, schemars::JsonSchema)]
pub struct CreateDotBody {
    pub name: String,
    #[serde(default)]
    pub display: Option<String>,
    #[serde(default)]
    pub description: String,
}

#[derive(Clone, Serialize, schemars::JsonSchema)]
pub struct DotView {
    pub id: String,
    pub name: String,
    pub display: String,
    pub description: String,
    pub active: bool,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct DotList {
    pub dots: Vec<DotView>,
    pub active_limit: u32,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct DotConnectionsView {
    pub dot: DotView,
    pub connections: Vec<DotConnectionView>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct DotConnectionView {
    pub key: String,
    pub connector: Option<DotConnectorView>,
    pub supported_modes: Vec<&'static str>,
    pub selected: DotSelectedConnection,
    pub user_connection: Option<DotCredentialView>,
    pub principal_connection: Option<DotCredentialView>,
    pub can_select: bool,
    pub can_connect: bool,
    pub can_disconnect: bool,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct DotConnectorView {
    pub title: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub auth_type: &'static str,
    pub scope: &'static str,
    pub needs_setup: bool,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct DotSelectedConnection {
    pub mode: &'static str,
    pub binding_generation: Option<String>,
    pub connection_id: Option<String>,
    pub connection_generation: Option<String>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct DotCredentialView {
    pub id: Option<String>,
    pub generation: Option<String>,
    pub connected: bool,
    pub errored: bool,
    pub needs_reauth: bool,
    pub authorizing: bool,
    pub account_label: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DotConnectionSelectionBody {
    pub mode: String,
    pub expected_binding_generation: Option<String>,
    pub connection_id: String,
    pub connection_generation: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DotConnectionVersionBody {
    pub connection_id: Option<String>,
    pub connection_generation: Option<String>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct DotAuthorizationView {
    pub authorization_url: String,
}

fn credential_view(
    connection: Option<&aiplane_core::server::db::mcp_connections::Connection>,
    catalog_generation: Option<&str>,
) -> DotCredentialView {
    DotCredentialView {
        id: connection.map(|row| row.id.clone()),
        generation: connection.map(|row| row.generation.clone()),
        connected: connection.is_some_and(|row| {
            !row.is_errored() && row.access_token_ct.is_some() && row.access_token_nonce.is_some()
        }),
        errored: connection.is_some_and(|row| row.is_errored() && row.status != "pending"),
        needs_reauth: connection.is_some_and(|row| {
            row.needs_reauth()
                || catalog_generation
                    .is_some_and(|generation| row.catalog_generation.as_deref() != Some(generation))
        }),
        authorizing: connection.is_some_and(|row| {
            row.authorization_expires_at
                .is_some_and(|expiry| jiff::Timestamp::now() < expiry)
        }),
        account_label: None,
    }
}

struct ConnectionAccess {
    dot: DotView,
    owner: agents_db::DotOwner,
    user: aiplane_core::server::db::users::User,
    policy: aiplane_core::server::rbac::Resolver,
    can_manage: bool,
}

fn connection_permission_error(owner: &agents_db::DotOwner, lang: Lang) -> Response {
    match owner {
        agents_db::DotOwner::User(_) => json_error(
            StatusCode::FORBIDDEN,
            "forbidden",
            &t(lang, "dot-error-permission-required"),
        ),
        agents_db::DotOwner::Organization(_) => not_found(t(lang, "dot-error-not-found")),
    }
}

async fn connection_access_in(
    state: &RamaState,
    tx: &mut aiplane_core::server::db::WriteTx,
    user_id: &str,
    id: &str,
    write: bool,
    lang: Lang,
) -> Result<ConnectionAccess, Response> {
    use aiplane_core::server::db::mcp_connections::ConnectionOwner;
    let Some((user, policy)) =
        aiplane_runtime::agents::access::current_user_policy_in(state, tx, user_id)
            .await
            .map_err(internal)?
    else {
        return Err(not_found(t(lang, "dot-error-not-found")));
    };
    let Some(metadata) = agents_db::get_dot_metadata_in(tx, id)
        .await
        .map_err(internal)?
    else {
        return Err(not_found(t(lang, "dot-error-not-found")));
    };
    let can_manage = match &metadata.owner {
        agents_db::DotOwner::User(owner) if owner == user_id => {
            policy.can_manage_dots(&policy.role_ids_for(&user.roles))
        }
        agents_db::DotOwner::User(_) => return Err(not_found(t(lang, "dot-error-not-found"))),
        agents_db::DotOwner::Organization(_) => {
            if !aiplane_runtime::server::tools::mcp::identities::connection_owner_admitted_in(
                state,
                tx,
                user_id,
                &ConnectionOwner::Principal(id.to_owned()),
                Some(id),
            )
            .await
            .map_err(internal)?
            {
                return Err(not_found(t(lang, "dot-error-not-found")));
            }
            true
        }
    };
    if write && !can_manage {
        return Err(connection_permission_error(&metadata.owner, lang));
    }
    let Some(agent) = agents_db::get_in(tx, id).await.map_err(internal)? else {
        return Err(not_found(t(lang, "dot-error-not-found")));
    };
    Ok(ConnectionAccess {
        dot: DotView {
            id: id.to_owned(),
            name: metadata.name,
            display: agent.principal.display,
            description: agent.principal.description,
            active: metadata.active,
        },
        owner: metadata.owner,
        user,
        policy,
        can_manage,
    })
}

async fn connection_preflight(
    state: &RamaState,
    user_id: &str,
    id: &str,
    lang: Lang,
) -> Result<ConnectionAccess, Response> {
    let mut tx = aiplane_core::server::db::WriteTx::begin(&state.db)
        .await
        .map_err(internal)?;
    let access = connection_access_in(state, &mut tx, user_id, id, true, lang).await?;
    tx.commit().await.map_err(internal)?;
    Ok(access)
}

async fn connections_view_in(
    tx: &mut aiplane_core::server::db::WriteTx,
    access: ConnectionAccess,
) -> Result<DotConnectionsView, aiplane_core::server::db::DbError> {
    use aiplane_agents::db::dot_connections::{self, IdentityMode};
    use aiplane_core::server::db::{
        mcp_catalog,
        mcp_connections::{self, ConnectionOwner},
    };
    let id = &access.dot.id;
    let personal = matches!(&access.owner, agents_db::DotOwner::User(_));
    let roles = access.policy.role_ids_for(&access.user.roles);
    let catalog = mcp_catalog::list_enabled_in(tx).await?;
    let permitted: Vec<_> = catalog
        .into_iter()
        .filter(|row| {
            row.is_per_user()
                && row.auth == mcp_catalog::AuthKind::OAuth2
                && row.allows(&roles, access.policy.is_admin(&roles))
        })
        .collect();
    let bindings = dot_connections::list_for_dot_in(tx, &access.owner, id).await?;
    let owned_connections =
        mcp_connections::list_connections_in(tx, &ConnectionOwner::Principal(id.clone())).await?;
    let user_connections = if let agents_db::DotOwner::User(owner) = &access.owner {
        mcp_connections::list_connections_in(tx, &ConnectionOwner::User(owner.clone())).await?
    } else {
        Vec::new()
    };
    let mut keys: std::collections::BTreeSet<_> =
        permitted.iter().map(|row| row.key.clone()).collect();
    keys.extend(bindings.iter().map(|row| row.connector_key.clone()));
    keys.extend(
        owned_connections
            .iter()
            .map(|row| row.connector_key.clone()),
    );
    let mut connections = Vec::with_capacity(keys.len());
    for key in keys {
        let connector = permitted.iter().find(|row| row.key == key);
        let binding = bindings.iter().find(|row| row.connector_key == key);
        let principal_connection = owned_connections
            .iter()
            .find(|row| row.connector_key == key);
        let user_connection =
            connector.and_then(|_| user_connections.iter().find(|row| row.connector_key == key));
        let mode = binding.map(|row| row.identity_mode).unwrap_or(if personal {
            IdentityMode::User
        } else {
            IdentityMode::Principal
        });
        let selected = match mode {
            IdentityMode::User => user_connection,
            IdentityMode::Principal => {
                principal_connection.filter(|_| binding.is_some() && connector.is_some())
            }
        };
        let generation = binding
            .map(|row| row.connection_generation.clone())
            .or_else(|| selected.map(|row| row.generation.clone()));
        let selected_id = selected
            .filter(|row| Some(row.generation.as_str()) == generation.as_deref())
            .filter(|row| {
                !row.is_errored()
                    && row.access_token_ct.is_some()
                    && row.access_token_nonce.is_some()
            })
            .filter(|row| {
                connector.is_some_and(|connector| {
                    row.catalog_generation.as_deref()
                        == Some(connector.dispatch_generation.as_str())
                })
            })
            .map(|row| row.id.clone());
        let catalog_generation = connector.map(|row| row.dispatch_generation.as_str());
        connections.push(DotConnectionView {
            key,
            connector: connector.map(|row| DotConnectorView {
                title: row.name.clone(),
                description: row.description.clone(),
                icon: row.icon.clone(),
                auth_type: row.auth.as_str(),
                scope: row.scope.as_str(),
                needs_setup: row.needs_setup(),
            }),
            supported_modes: if connector.is_none() {
                vec![]
            } else if personal {
                vec!["user", "principal"]
            } else {
                vec!["principal"]
            },
            selected: DotSelectedConnection {
                mode: match mode {
                    IdentityMode::User => "user",
                    IdentityMode::Principal => "principal",
                },
                binding_generation: binding.map(|row| row.binding_generation.clone()),
                connection_id: selected_id,
                connection_generation: generation,
            },
            user_connection: personal.then(|| credential_view(user_connection, catalog_generation)),
            principal_connection: principal_connection
                .map(|row| credential_view(Some(row), catalog_generation)),
            can_select: access.can_manage && connector.is_some(),
            can_connect: access.can_manage && connector.is_some_and(|row| !row.needs_setup()),
            can_disconnect: access.can_manage && principal_connection.is_some(),
        });
    }
    Ok(DotConnectionsView {
        dot: access.dot,
        connections,
    })
}

async fn connection_response(state: &RamaState, user_id: &str, id: &str, lang: Lang) -> Response {
    let result = async {
        let mut tx = aiplane_core::server::db::WriteTx::begin(&state.db)
            .await
            .map_err(internal)?;
        let access = connection_access_in(state, &mut tx, user_id, id, false, lang).await?;
        let view = connections_view_in(&mut tx, access)
            .await
            .map_err(internal)?;
        tx.commit().await.map_err(internal)?;
        Ok::<_, Response>(view)
    }
    .await;
    match result {
        Ok(view) => json_ok(StatusCode::OK, view),
        Err(response) => response,
    }
}

pub async fn connections_get(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_, user) = require_session_json!(state, req);
    let lang = Lang::from_request(req.headers());
    let Some(id) = raw_path_segment(&req, 1) else {
        return bad_request(t(lang, "dot-error-id-missing"));
    };
    connection_response(&state, &user.id, &id, lang).await
}

pub async fn connections_put(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    use aiplane_agents::db::dot_connections::{IdentityMode, SaveOutcome, Selection};
    use aiplane_core::server::db::{
        WriteTx,
        mcp_connections::{self, ConnectionOwner},
    };
    let (_, user) = require_session_json!(state, req);
    let lang = Lang::from_request(req.headers());
    let (Some(id), Some(key)) = (raw_path_segment(&req, 2), raw_path_segment(&req, 0)) else {
        return bad_request(t(lang, "dot-connection-error-invalid"));
    };
    if let Err(response) = connection_preflight(&state, &user.id, &id, lang).await {
        return response;
    }
    let body: DotConnectionSelectionBody =
        match super::read_json_capped(req.into_body(), "the Dot connection selection", 16 * 1024)
            .await
        {
            Ok(body) => body,
            Err(response) => return response,
        };
    let mode = match body.mode.as_str() {
        "user" => IdentityMode::User,
        "principal" => IdentityMode::Principal,
        _ => return bad_request(t(lang, "dot-connection-error-invalid")),
    };
    let result = async {
        let mut tx = WriteTx::begin(&state.db).await.map_err(internal)?;
        let access = connection_access_in(&state, &mut tx, &user.id, &id, true, lang).await?;
        if matches!(&access.owner, agents_db::DotOwner::Organization(_))
            && mode == IdentityMode::User
        {
            return Ok(SaveOutcome::Unavailable);
        }
        let owner = match mode {
            IdentityMode::User => ConnectionOwner::User(user.id.clone()),
            IdentityMode::Principal => ConnectionOwner::Principal(id.clone()),
        };
        let connection = mcp_connections::get_connection_in(&mut tx, &owner, &key)
            .await
            .map_err(internal)?;
        if !connection.is_some_and(|row| {
            row.id == body.connection_id && row.generation == body.connection_generation
        }) {
            return Ok(SaveOutcome::Conflict);
        }
        let saved = aiplane_runtime::server::tools::mcp::identities::save_selection_in(
            &state,
            &mut tx,
            &user.id,
            Selection {
                owner: &access.owner,
                dot_id: &id,
                connector_key: &key,
                identity_mode: mode,
                connection_generation: &body.connection_generation,
                expected_binding_generation: body.expected_binding_generation.as_deref(),
            },
        )
        .await
        .map_err(internal)?
        .ok_or_else(|| connection_permission_error(&access.owner, lang))?;
        tx.commit().await.map_err(internal)?;
        Ok::<_, Response>(saved)
    }
    .await;
    match result {
        Ok(SaveOutcome::Saved(_)) => connection_response(&state, &user.id, &id, lang).await,
        Ok(SaveOutcome::Missing) => not_found(t(lang, "dot-error-not-found")),
        Ok(SaveOutcome::Unavailable) => json_error(
            StatusCode::CONFLICT,
            "connection_unavailable",
            &t(lang, "dot-connection-error-unavailable"),
        ),
        Ok(SaveOutcome::Conflict) => json_error(
            StatusCode::CONFLICT,
            "connection_conflict",
            &t(lang, "dot-connection-error-conflict"),
        ),
        Err(response) => response,
    }
}

pub async fn connections_connect(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    use aiplane_core::server::db::mcp_connections::ConnectionOwner;
    let (_, user) = require_session_json!(state, req);
    let lang = Lang::from_request(req.headers());
    let (Some(id), Some(key)) = (raw_path_segment(&req, 3), raw_path_segment(&req, 1)) else {
        return bad_request(t(lang, "dot-connection-error-invalid"));
    };
    let access = match connection_preflight(&state, &user.id, &id, lang).await {
        Ok(access) => access,
        Err(response) => return response,
    };
    let body: DotConnectionVersionBody =
        match super::read_json_capped(req.into_body(), "the Dot connection version", 16 * 1024)
            .await
        {
            Ok(body) => body,
            Err(response) => return response,
        };
    if body.connection_id.is_some() != body.connection_generation.is_some() {
        return bad_request(t(lang, "dot-connection-error-invalid"));
    }
    let return_to = match access.owner {
        agents_db::DotOwner::User(_) => format!("/dots?dot_id={id}"),
        agents_db::DotOwner::Organization(_) => format!("/agents/{id}?tab=setup"),
    };
    let response = super::integrations::connect_for_owner(
        &state,
        &access.user,
        &key,
        super::integrations::ConnectTarget {
            owner: ConnectionOwner::Principal(id.clone()),
            subject_principal_id: Some(id),
            return_to: Some(return_to),
            expected_connection: Some((body.connection_id, body.connection_generation)),
        },
        lang,
    )
    .await;
    if response.status() == StatusCode::SEE_OTHER
        && let Some(url) = response
            .headers()
            .get(rama::http::header::LOCATION)
            .and_then(|value| value.to_str().ok())
    {
        return json_ok(
            StatusCode::OK,
            DotAuthorizationView {
                authorization_url: url.to_owned(),
            },
        );
    }
    let status = response.status();
    let key = match status {
        StatusCode::CONFLICT => "dot-connection-error-conflict",
        StatusCode::FORBIDDEN => "dot-connection-error-unavailable",
        _ => "dot-connection-error-authorization",
    };
    json_error(status, "connection_authorization", &t(lang, key))
}

pub async fn connections_delete(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    use aiplane_core::server::db::{
        WriteTx,
        mcp_connections::{self, ConnectionOwner},
    };
    let (_, user) = require_session_json!(state, req);
    let lang = Lang::from_request(req.headers());
    let (Some(id), Some(key)) = (raw_path_segment(&req, 2), raw_path_segment(&req, 0)) else {
        return bad_request(t(lang, "dot-connection-error-invalid"));
    };
    if let Err(response) = connection_preflight(&state, &user.id, &id, lang).await {
        return response;
    }
    let body: DotConnectionVersionBody =
        match super::read_json_capped(req.into_body(), "the Dot connection version", 16 * 1024)
            .await
        {
            Ok(body) => body,
            Err(response) => return response,
        };
    if body.connection_id.is_some() != body.connection_generation.is_some() {
        return bad_request(t(lang, "dot-connection-error-invalid"));
    }
    let owner = ConnectionOwner::Principal(id.clone());
    let result = async {
        let mut tx = WriteTx::begin(&state.db).await.map_err(internal)?;
        connection_access_in(&state, &mut tx, &user.id, &id, true, lang).await?;
        let connection = mcp_connections::get_connection_in(&mut tx, &owner, &key)
            .await
            .map_err(internal)?;
        if connection.as_ref().map(|row| row.id.as_str()) != body.connection_id.as_deref()
            || connection.as_ref().map(|row| row.generation.as_str())
                != body.connection_generation.as_deref()
        {
            return Ok(false);
        }
        if let Some(connection) = connection
            && !mcp_connections::delete_connection_in(&mut tx, &connection.identity())
                .await
                .map_err(internal)?
        {
            return Ok(false);
        }
        tx.commit().await.map_err(internal)?;
        Ok::<_, Response>(true)
    }
    .await;
    match result {
        Ok(true) => {
            state.mcp.invalidate(&owner, &key).await;
            connection_response(&state, &user.id, &id, lang).await
        }
        Ok(false) => json_error(
            StatusCode::CONFLICT,
            "connection_conflict",
            &t(lang, "dot-connection-error-conflict"),
        ),
        Err(response) => response,
    }
}

pub(crate) fn view(row: &agents_db::PersonalDotRow) -> DotView {
    let p = &row.agent.principal;
    DotView {
        id: p.id.clone(),
        name: row.name.clone(),
        display: p.display.clone(),
        description: p.description.clone(),
        active: row.active,
    }
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct DotCommunicationView {
    pub dot: DotView,
    pub peers: Vec<DotCommunicationPeer>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct DotCommunicationPeer {
    pub dot: DotView,
    pub outgoing: DotConsentView,
    pub incoming: DotConsentView,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct DotConsentView {
    pub outbound_enabled: bool,
    pub inbound_enabled: bool,
    pub generation: Option<String>,
}

impl DotConsentView {
    fn from_edge(edge: Option<&dot_delegation::DotDelegationEdge>) -> Self {
        Self {
            outbound_enabled: edge.is_some_and(|edge| edge.outbound_enabled),
            inbound_enabled: edge.is_some_and(|edge| edge.inbound_enabled),
            generation: edge.map(|edge| edge.generation.clone()),
        }
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DotConsentDirection {
    Outbound,
    Inbound,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DotCommunicationBody {
    pub peer_id: String,
    pub direction: DotConsentDirection,
    pub enabled: bool,
    pub expected_generation: Option<String>,
}

async fn communication_view(
    state: &RamaState,
    owner: &str,
    id: &str,
) -> Result<Option<DotCommunicationView>, aiplane_core::server::db::DbError> {
    let Some(dot) = agents_db::get_personal_dot(&state.db, owner, id).await? else {
        return Ok(None);
    };
    let inventory = agents_db::list_personal_dots(&state.db, owner).await?;
    let edges = dot_delegation::list_for_dot(&state.db, owner, id).await?;
    let peers = inventory
        .iter()
        .filter(|peer| peer.agent.principal.id != id)
        .map(|peer| {
            let peer_id = &peer.agent.principal.id;
            DotCommunicationPeer {
                dot: view(peer),
                outgoing: DotConsentView::from_edge(
                    edges
                        .iter()
                        .find(|edge| edge.caller_dot_id == id && &edge.receiver_dot_id == peer_id),
                ),
                incoming: DotConsentView::from_edge(
                    edges
                        .iter()
                        .find(|edge| &edge.caller_dot_id == peer_id && edge.receiver_dot_id == id),
                ),
            }
        })
        .collect();
    Ok(Some(DotCommunicationView {
        dot: view(&dot),
        peers,
    }))
}

pub async fn communication_get(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_, user) = match super::require_session_json(&state, &req).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let lang = Lang::from_request(req.headers());
    let Some(id) = raw_path_segment(&req, 1) else {
        return bad_request(t(lang, "dot-error-id-missing"));
    };
    match communication_view(&state, &user.id, &id).await {
        Ok(Some(view)) => json_ok(StatusCode::OK, view),
        Ok(None) => not_found(t(lang, "dot-error-not-found")),
        Err(error) => internal(error),
    }
}

pub async fn communication_put(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let user = match require_dot_manager(&state, &req).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    let lang = Lang::from_request(req.headers());
    let Some(id) = raw_path_segment(&req, 1) else {
        return bad_request(t(lang, "dot-error-id-missing"));
    };
    match agents_db::get_personal_dot(&state.db, &user.id, &id).await {
        Ok(Some(_)) => {}
        Ok(None) => return not_found(t(lang, "dot-error-not-found")),
        Err(error) => return internal(error),
    }
    let body: DotCommunicationBody = match super::read_json_capped(
        req.into_body(),
        "the communication setting",
        16 * 1024,
    )
    .await
    {
        Ok(body) => body,
        Err(response) => return response,
    };
    let direction = match body.direction {
        DotConsentDirection::Outbound => ConsentDirection::Outbound,
        DotConsentDirection::Inbound => ConsentDirection::Inbound,
    };
    match dot_delegation::set_consent(
        &state.db,
        ConsentEdit {
            owner_user_id: &user.id,
            dot_id: &id,
            peer_id: &body.peer_id,
            direction,
            enabled: body.enabled,
            expected_generation: body.expected_generation.as_deref(),
        },
    )
    .await
    {
        Ok(ConsentSaved::Saved(edge)) => {
            if let Some(previous) = body.expected_generation.as_deref()
                && previous != edge.generation
                && let Err(error) = aiplane_runtime::agents::resume::cancel_personal_runs_for_edge(
                    &state,
                    &user.id,
                    &edge.caller_dot_id,
                    &edge.receiver_dot_id,
                    previous,
                )
                .await
            {
                return internal(error);
            }
            match communication_view(&state, &user.id, &id).await {
                Ok(Some(view)) => json_ok(StatusCode::OK, view),
                Ok(None) => not_found(t(lang, "dot-error-not-found")),
                Err(error) => internal(error),
            }
        }
        Ok(ConsentSaved::Missing) => not_found(t(lang, "dot-error-not-found")),
        Ok(ConsentSaved::Invalid) => json_error(
            StatusCode::BAD_REQUEST,
            "invalid_consent",
            &t(lang, "dot-communication-error-invalid"),
        ),
        Ok(ConsentSaved::Conflict) => json_error(
            StatusCode::CONFLICT,
            "conflict",
            &t(lang, "dot-communication-error-conflict"),
        ),
        Ok(ConsentSaved::Unavailable) => json_error(
            StatusCode::CONFLICT,
            "inactive_dot",
            &t(lang, "dot-communication-error-inactive"),
        ),
        Err(error) => internal(error),
    }
}

async fn require_dot_manager(
    state: &RamaState,
    req: &Request,
) -> Result<aiplane_core::server::db::users::User, Response> {
    let (_, user) = super::require_session_json(state, req).await?;
    let role_ids = state.rbac.role_ids_for(&user.roles);
    if !state.rbac.can_manage_dots(&role_ids) {
        let lang = Lang::from_request(req.headers());
        return Err(json_error(
            StatusCode::FORBIDDEN,
            "forbidden",
            &t(lang, "dot-error-permission-required"),
        ));
    }
    Ok(user)
}

pub async fn list(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let user = match require_dot_manager(&state, &req).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    let active_limit = state.config().dots.max_active_per_user;
    match agents_db::list_personal_dots(&state.db, &user.id).await {
        Ok(rows) => json_ok(
            StatusCode::OK,
            DotList {
                dots: rows.iter().map(view).collect(),
                active_limit,
            },
        ),
        Err(error) => internal(error),
    }
}

pub async fn create(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let user = match require_dot_manager(&state, &req).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    let lang = Lang::from_request(req.headers());
    let body: CreateDotBody = match super::read_json(req.into_body(), "the Dot body").await {
        Ok(body) => body,
        Err(response) => return response,
    };
    let name = body.name.trim();
    if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        return json_error(
            StatusCode::BAD_REQUEST,
            "invalid_name",
            &t(lang, "dot-error-name-invalid"),
        );
    }
    let display = body
        .display
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(name);
    let allowed = state.allowed_tools_for_user(&user.roles, &user.id).await;
    let memory_tools: Vec<_> = catalog::MEMORY_TOOL_IDS
        .iter()
        .copied()
        .filter(|id| allowed.iter().any(|allowed| allowed == id))
        .collect();
    let resources: serde_json::Map<String, serde_json::Value> = memory_tools
        .iter()
        .map(|id| ((*id).to_string(), json!({"permission":"always_allow"})))
        .collect();
    let spec = json!({"main":{"tools":memory_tools,"tool_resources":resources}}).to_string();
    match agents_db::create_personal_dot(
        &state.db,
        &NewPrincipal {
            name,
            display,
            description: body.description.trim(),
        },
        &spec,
        &user.id,
        state.config().dots.max_active_per_user,
    )
    .await
    {
        Ok((DotCreate::Created, Some(agent))) => json_ok(
            StatusCode::CREATED,
            json!({ "dot": { "id": agent.principal.id, "name": name,
                "display": agent.principal.display, "description": agent.principal.description,
                "active": true } }),
        ),
        Ok((DotCreate::NameTaken, _)) => json_error(
            StatusCode::CONFLICT,
            "conflict",
            &t(lang, "dot-error-name-taken"),
        ),
        Ok((DotCreate::ActiveLimit, _)) => json_error(
            StatusCode::CONFLICT,
            "active_dot_limit",
            &t(lang, "dot-error-active-limit"),
        ),
        Ok((DotCreate::Created, None)) => internal("created Dot could not be loaded"),
        Err(error) => internal(error),
    }
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct DotAccessView {
    pub tools: Vec<DotAccessTool>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct DotAccessTool {
    pub id: String,
    pub label: Option<String>,
    pub description: Option<String>,
    pub available: bool,
    pub selected: bool,
    pub permission: &'static str,
    pub read_only: bool,
    pub identity: &'static str,
    pub connector_key: Option<String>,
    pub connector_mode: Option<&'static str>,
    pub actions: Option<Vec<DotAccessAction>>,
    pub action_rules: Vec<DotActionRule>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct DotAccessAction {
    pub id: String,
    pub description: String,
}

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DotActionRule {
    pub id: String,
    pub rule: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DotAccessBody {
    pub tools: Vec<DotAccessSelection>,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DotAccessSelection {
    pub id: String,
    pub permission: String,
    #[serde(default)]
    pub actions: Vec<DotActionRule>,
}

async fn access_view(
    state: &RamaState,
    user: &aiplane_core::server::db::users::User,
    dot_id: &str,
    draft: &str,
    lang: Lang,
) -> Result<DotAccessView, Box<dyn std::error::Error + Send + Sync>> {
    let spec: AgentSpec = serde_json::from_str(draft)?;
    let layer = aiplane_runtime::server::tools::mcp::identities::layer_for_personal_dot(
        state,
        user,
        dot_id,
        AskContext::Chat,
        false,
    )
    .await;
    let principal = aiplane_core::server::principal::Principal::User {
        id: user.id.clone(),
        roles: user.roles.clone(),
    };
    let mut allowed = state.granted_tool_ids(&principal, &layer).await;
    let disabled =
        aiplane_core::server::db::user_tool_prefs::disabled_for_user(&state.db, &user.id).await?;
    catalog::retain_enabled(&mut allowed, &disabled);
    allowed.retain(|id| !catalog::is_hidden(id));
    allowed.sort();
    let entries = super::tool_toggles::entries_for_tools(state, &allowed, lang);
    let registry = state.tools();
    let comfyui = state.comfyui().as_ref().map(|handle| {
        aiplane_runtime::server::comfyui_tool::ComfyuiToolSource::new((**handle).clone())
    });
    let source = CompositeToolSource::new(&registry, &layer).with_comfyui(comfyui.as_ref());
    let mut tools = Vec::new();
    for id in allowed {
        let Some(tool) = source.get(&id) else {
            continue;
        };
        let def = tool.schema();
        let connector_key = layer.connector_of(&id).map(str::to_owned);
        let identity = match connector_key.as_deref() {
            Some(key) => match aiplane_core::server::db::mcp_catalog::get(&state.db, key).await? {
                Some(connector) if connector.is_global() => "shared",
                _ if tool
                    .execution_facts()
                    .and_then(|facts| {
                        serde_json::from_value::<
                            aiplane_runtime::server::tools::mcp::identities::McpDispatchFacts,
                        >(facts)
                        .ok()
                    })
                    .is_some_and(|facts| {
                        let aiplane_runtime::server::tools::mcp::identities::McpDispatchFacts::Owned(facts) = facts else {
                            return false;
                        };
                        matches!(
                            facts.connection.owner,
                            aiplane_core::server::db::mcp_connections::ConnectionOwner::Principal(
                                _
                            )
                        )
                    }) =>
                {
                    "principal"
                }
                _ => "user",
            },
            None => "builtin",
        };
        let entry = entries
            .iter()
            .find(|entry| entry.key == catalog::entry_key_for(&id));
        let read_only = tool.read_only();
        let permission = match spec
            .main
            .tool_resources
            .get(&id)
            .and_then(|resource| resource.permission)
        {
            Some(Permission::AlwaysAllow) => "always_allow",
            Some(Permission::AlwaysAsk) => "always_ask",
            None if read_only => "always_allow",
            None => "always_ask",
        };
        tools.push(DotAccessTool {
            selected: spec.main.tools.contains(&id),
            available: true,
            label: Some(
                entry
                    .map(|entry| entry.title.clone())
                    .unwrap_or_else(|| def.function.name.clone()),
            ),
            description: Some(
                super::tool_toggles::builtin_display_copy(&id, lang)
                    .map(|(_, description)| description)
                    .unwrap_or_else(|| def.function.description.clone()),
            ),
            permission,
            read_only,
            identity,
            connector_mode: layer.mode_of(&id).map(|mode| mode.as_str()),
            connector_key,
            actions: tool.action_catalogue().map(|actions| {
                actions
                    .iter()
                    .map(|action| DotAccessAction {
                        id: action.id.to_owned(),
                        description: action.description.to_owned(),
                    })
                    .collect()
            }),
            action_rules: spec
                .main
                .tool_resources
                .get(&id)
                .map(|resource| {
                    resource
                        .actions
                        .iter()
                        .map(|(id, rule)| DotActionRule {
                            id: id.clone(),
                            rule: match rule {
                                ActionRule::Allow => "allow",
                                ActionRule::Ask => "ask",
                                ActionRule::Deny => "deny",
                            }
                            .to_owned(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            id,
        });
    }
    for id in &spec.main.tools {
        if tools.iter().any(|tool| &tool.id == id) {
            continue;
        }
        tools.push(DotAccessTool {
            id: id.clone(),
            label: None,
            description: None,
            available: false,
            selected: true,
            permission: match spec
                .main
                .tool_resources
                .get(id)
                .and_then(|resource| resource.permission)
            {
                Some(Permission::AlwaysAllow) => "always_allow",
                _ => "always_ask",
            },
            read_only: false,
            identity: "unavailable",
            connector_key: None,
            connector_mode: None,
            actions: None,
            action_rules: spec
                .main
                .tool_resources
                .get(id)
                .map(|resource| {
                    resource
                        .actions
                        .iter()
                        .map(|(id, rule)| DotActionRule {
                            id: id.clone(),
                            rule: match rule {
                                ActionRule::Allow => "allow",
                                ActionRule::Ask => "ask",
                                ActionRule::Deny => "deny",
                            }
                            .to_owned(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
        });
    }
    Ok(DotAccessView { tools })
}

pub async fn access_get(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_, user) = require_session_json!(state, req);
    let lang = Lang::from_request(req.headers());
    let Some(id) = raw_path_segment(&req, 1) else {
        return bad_request(t(lang, "dot-error-id-missing"));
    };
    let dot = match agents_db::get_personal_dot(&state.db, &user.id, &id).await {
        Ok(Some(dot)) => dot,
        Ok(None) => return not_found(t(lang, "dot-error-not-found")),
        Err(error) => return internal(error),
    };
    match access_view(&state, &user, &id, &dot.agent.draft_spec, lang).await {
        Ok(view) => json_ok(StatusCode::OK, view),
        Err(error) => internal(error),
    }
}

pub async fn access_put(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_, user) = require_session_json!(state, req);
    let lang = Lang::from_request(req.headers());
    let Some(id) = raw_path_segment(&req, 1) else {
        return bad_request(t(lang, "dot-error-id-missing"));
    };
    let dot = match agents_db::get_personal_dot(&state.db, &user.id, &id).await {
        Ok(Some(dot)) => dot,
        Ok(None) => return not_found(t(lang, "dot-error-not-found")),
        Err(error) => return internal(error),
    };
    let body: DotAccessBody = match super::read_json(req.into_body(), "the Dot access rules").await
    {
        Ok(body) => body,
        Err(response) => return response,
    };
    let inventory = match access_view(&state, &user, &id, &dot.agent.draft_spec, lang).await {
        Ok(view) => view,
        Err(error) => return internal(error),
    };
    let mut selected = std::collections::HashSet::new();
    let mut selections = Vec::with_capacity(body.tools.len());
    for selection in &body.tools {
        if !selected.insert(&selection.id)
            || !matches!(selection.permission.as_str(), "always_allow" | "always_ask")
        {
            return bad_request(t(lang, "dot-access-error-invalid"));
        }
        let Some(tool) = inventory.tools.iter().find(|tool| tool.id == selection.id) else {
            return bad_request(t(lang, "dot-access-error-unavailable"));
        };
        if !tool.available {
            return bad_request(t(lang, "dot-access-error-unavailable"));
        }
        let mut actions = std::collections::BTreeMap::new();
        for action in &selection.actions {
            let rule = match action.rule.as_str() {
                "allow" => ActionRule::Allow,
                "ask" => ActionRule::Ask,
                "deny" => ActionRule::Deny,
                _ => return bad_request(t(lang, "dot-access-error-invalid")),
            };
            if actions.insert(action.id.clone(), rule).is_some() {
                return bad_request(t(lang, "dot-access-error-invalid"));
            }
            if !tool
                .actions
                .as_ref()
                .is_some_and(|catalogue| catalogue.iter().any(|known| known.id == action.id))
            {
                return bad_request(t(lang, "dot-access-error-unavailable"));
            }
        }
        selections.push(ToolAccessSelection {
            id: selection.id.clone(),
            permission: match selection.permission.as_str() {
                "always_allow" => Permission::AlwaysAllow,
                _ => Permission::AlwaysAsk,
            },
            actions,
        });
    }
    let draft = match aiplane_runtime::agents::spec::tool_access::replace_tool_access(
        &dot.agent.draft_spec,
        &selections,
    ) {
        Ok(draft) => draft,
        Err(error) => return internal(error),
    };
    match agents_db::update_personal_dot_draft(
        &state.db,
        &id,
        &user.id,
        &dot.agent.draft_spec,
        &draft,
    )
    .await
    {
        Ok(agents_db::PersonalDotDraftSaved::Saved) => {
            match access_view(&state, &user, &id, &draft, lang).await {
                Ok(view) => json_ok(StatusCode::OK, view),
                Err(error) => internal(error),
            }
        }
        Ok(agents_db::PersonalDotDraftSaved::Missing) => not_found(t(lang, "dot-error-not-found")),
        Ok(agents_db::PersonalDotDraftSaved::Conflict) => json_error(
            StatusCode::CONFLICT,
            "conflict",
            &t(lang, "dot-access-error-conflict"),
        ),
        Err(error) => internal(error),
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ActiveBody {
    pub active: bool,
}

pub async fn set_active(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let user = match require_dot_manager(&state, &req).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    let lang = Lang::from_request(req.headers());
    let Some(id) = raw_path_segment(&req, 1) else {
        return bad_request(t(lang, "dot-error-id-missing"));
    };
    let body: ActiveBody = match super::read_json(req.into_body(), "the Dot state").await {
        Ok(body) => body,
        Err(response) => return response,
    };
    match agents_db::set_personal_dot_active(
        &state.db,
        &id,
        &user.id,
        body.active,
        state.config().dots.max_active_per_user,
    )
    .await
    {
        Ok(true) => {
            if !body.active
                && let Err(error) = aiplane_runtime::agents::resume::cancel_personal_runs_for_dot(
                    &state, &user.id, &id,
                )
                .await
            {
                return internal(error);
            }
            json_ok(StatusCode::OK, json!({ "id": id, "active": body.active }))
        }
        Ok(false) => {
            let owned = agents_db::list_personal_dots(&state.db, &user.id).await;
            match owned {
                Ok(rows) if rows.iter().any(|row| row.agent.principal.id == id) => json_error(
                    StatusCode::CONFLICT,
                    "active_dot_limit",
                    &t(lang, "dot-error-active-limit"),
                ),
                Ok(_) => not_found(t(lang, "dot-error-not-found")),
                Err(error) => internal(error),
            }
        }
        Err(error) => internal(error),
    }
}

pub async fn create_session(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_, user) = require_session_json!(state, req);
    let lang = Lang::from_request(req.headers());
    let Some(dot_id) = raw_path_segment(&req, 1) else {
        return bad_request(t(lang, "dot-error-id-missing"));
    };
    let dot = match agents_db::get_personal_dot(&state.db, &user.id, &dot_id).await {
        Ok(Some(dot)) if dot.active => dot,
        Ok(Some(_)) => {
            return json_error(
                StatusCode::CONFLICT,
                "dot_inactive",
                &t(lang, "dot-error-inactive"),
            );
        }
        Ok(None) => return not_found(t(lang, "dot-error-not-found")),
        Err(error) => return internal(error),
    };
    let session = match session_core::db::create_session(&state.db, &user.id).await {
        Ok(session) => session,
        Err(error) => return internal(error),
    };
    match aiplane_agents::db::dot_sessions::link(&state.db, &user.id, &dot_id, &session.id).await {
        Ok(true) => {
            if let Err(error) =
                session_core::db::set_session_title(&state.db, &session.id, &dot.name).await
            {
                let _ = session_core::db::delete_session(&state.db, &user.id, &session.id).await;
                return internal(error);
            }
            let mut session = session;
            session.title = Some(dot.name);
            json_ok(StatusCode::CREATED, json!({ "session": session }))
        }
        Ok(false) => {
            let _ = session_core::db::delete_session(&state.db, &user.id, &session.id).await;
            json_error(
                StatusCode::CONFLICT,
                "dot_inactive",
                &t(lang, "dot-error-inactive"),
            )
        }
        Err(error) => {
            let _ = session_core::db::delete_session(&state.db, &user.id, &session.id).await;
            internal(error)
        }
    }
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct DotMemoryList {
    pub memories: Vec<DotMemoryView>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct DotMemoryView {
    pub id: String,
    pub kind: &'static str,
    pub content: String,
    pub created_at: String,
}

fn memory_view(memory: &aiplane_core::server::db::user_memories::Memory) -> DotMemoryView {
    DotMemoryView {
        id: memory.id.clone(),
        kind: memory.kind.as_str(),
        content: memory.content.clone(),
        created_at: memory.created_at.to_string(),
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct DotMemoryBody {
    pub kind: String,
    pub content: String,
}

fn parse_memory_body(
    body: DotMemoryBody,
) -> Result<(aiplane_core::server::db::user_memories::MemoryKind, String), DotMemoryInputError> {
    let kind = aiplane_core::server::db::user_memories::MemoryKind::parse(&body.kind)
        .ok_or(DotMemoryInputError::Kind)?;
    let content = body.content.trim().to_string();
    if content.is_empty() {
        return Err(DotMemoryInputError::Empty);
    }
    Ok((kind, content))
}

#[derive(Debug, Clone, Copy)]
enum DotMemoryInputError {
    Kind,
    Empty,
}

pub async fn memories_list(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_, user) = require_session_json!(state, req);
    let Some(dot_id) = raw_path_segment(&req, 1) else {
        return bad_request(t(Lang::from_request(req.headers()), "dot-error-id-missing"));
    };
    match aiplane_agents::db::dot_memories::list(&state.db, &user.id, &dot_id, 500).await {
        Ok(Some(memories)) => json_ok(
            StatusCode::OK,
            DotMemoryList {
                memories: memories.iter().map(memory_view).collect(),
            },
        ),
        Ok(None) => not_found(t(Lang::from_request(req.headers()), "dot-error-not-found")),
        Err(error) => internal(error),
    }
}

pub async fn memories_create(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_, user) = require_session_json!(state, req);
    let lang = Lang::from_request(req.headers());
    let Some(dot_id) = raw_path_segment(&req, 1) else {
        return bad_request(t(lang, "dot-error-id-missing"));
    };
    let body = match super::read_json::<DotMemoryBody>(req.into_body(), "the Dot memory body").await
    {
        Ok(body) => body,
        Err(response) => return response,
    };
    let (kind, content) = match parse_memory_body(body) {
        Ok(value) => value,
        Err(error) => {
            return bad_request(t(
                lang,
                match error {
                    DotMemoryInputError::Kind => "dot-memory-error-kind",
                    DotMemoryInputError::Empty => "dot-memory-error-empty",
                },
            ));
        }
    };
    match aiplane_agents::db::dot_memories::insert(&state.db, &user.id, &dot_id, kind, &content)
        .await
    {
        Ok(Some(memory)) => json_ok(StatusCode::CREATED, memory_view(&memory)),
        Ok(None) => not_found(t(lang, "dot-error-not-found")),
        Err(error) => internal(error),
    }
}

pub async fn memories_update(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_, user) = require_session_json!(state, req);
    let lang = Lang::from_request(req.headers());
    let (Some(dot_id), Some(memory_id)) = (raw_path_segment(&req, 2), raw_path_segment(&req, 0))
    else {
        return bad_request(t(lang, "dot-memory-error-id-missing"));
    };
    let body = match super::read_json::<DotMemoryBody>(req.into_body(), "the Dot memory body").await
    {
        Ok(body) => body,
        Err(response) => return response,
    };
    let (kind, content) = match parse_memory_body(body) {
        Ok(value) => value,
        Err(error) => {
            return bad_request(t(
                lang,
                match error {
                    DotMemoryInputError::Kind => "dot-memory-error-kind",
                    DotMemoryInputError::Empty => "dot-memory-error-empty",
                },
            ));
        }
    };
    match aiplane_agents::db::dot_memories::update(
        &state.db, &user.id, &dot_id, &memory_id, kind, &content,
    )
    .await
    {
        Ok(Some(memory)) => json_ok(StatusCode::OK, memory_view(&memory)),
        Ok(None) => not_found(t(lang, "dot-error-not-found")),
        Err(error) => internal(error),
    }
}

pub async fn memories_delete(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_, user) = require_session_json!(state, req);
    let lang = Lang::from_request(req.headers());
    let (Some(dot_id), Some(memory_id)) = (raw_path_segment(&req, 2), raw_path_segment(&req, 0))
    else {
        return bad_request(t(lang, "dot-memory-error-id-missing"));
    };
    match aiplane_agents::db::dot_memories::delete(&state.db, &user.id, &dot_id, &memory_id).await {
        Ok(true) => Response::builder()
            .status(StatusCode::NO_CONTENT)
            .body(rama::http::Body::empty())
            .expect("static empty response"),
        Ok(false) => not_found(t(lang, "dot-error-not-found")),
        Err(error) => internal(error),
    }
}

pub async fn activity(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_, user) = require_session_json!(state, req);
    let lang = Lang::from_request(req.headers());
    let Some(dot_id) = raw_path_segment(&req, 1) else {
        return bad_request(t(lang, "dot-error-id-missing"));
    };
    let dot = match agents_db::get_personal_dot(&state.db, &user.id, &dot_id).await {
        Ok(Some(dot)) => dot,
        Ok(None) => return not_found(t(lang, "dot-error-not-found")),
        Err(error) => return internal(error),
    };
    let params = super::query_map(&req);
    let limit = match super::parse_limit(&params, 100, 500) {
        Ok(limit) => limit,
        Err(response) => return response,
    };
    let cursor = match super::parse_cursor(&params) {
        Ok(cursor) => cursor,
        Err(response) => return response,
    };
    let query = aiplane_agents::db::agent_audit::ActivityQuery {
        agent_id: dot.agent.principal.id,
        cursor,
        order: aiplane_agents::db::agent_audit::Order::Desc,
        limit,
        max_bytes: 4 * 1024 * 1024,
        ..Default::default()
    };
    let page = match aiplane_agents::db::agent_audit::page(&state.db, &query).await {
        Ok(page) => page,
        Err(error) => return internal(error),
    };
    let mut reconstructor = aiplane_agents::db::agent_audit::Reconstructor::default();
    let mut events = Vec::with_capacity(page.events.len());
    for event in &page.events {
        match reconstructor.event_json(&state.db, event).await {
            Ok(event) => events.push(event),
            Err(error) => return internal(error),
        }
    }
    json_ok(
        StatusCode::OK,
        super::json_agent_activity::ActivityPageView {
            events,
            next_cursor: page.next_cursor,
            order: "desc",
        },
    )
}
