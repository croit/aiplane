// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The per-user connector flow for the `/tools/integrations` page.
//!
//! Every signed-in user can connect their own accounts (Gmail, Google
//! Calendar/Drive, GitHub, GitLab, Atlassian, …) to the assistant by
//! authorizing the gateway against each provider over OAuth 2.1. Once
//! connected, that connector's tools become available to the user's chats and
//! API tokens, each with a per-tool permission (always / ask / off).
//!
//! The catalog of connectable servers is admin-managed (`/admin/connectors`);
//! this page only surfaces the *enabled* ones, plus the caller's own
//! connection + per-tool state.
//!
//! OAuth flow (MCP Authorization spec): `connect` discovers the provider's
//! endpoints, optionally registers a client (DCR), and redirects the browser
//! to the provider; `callback` exchanges the code for tokens, encrypts them,
//! and persists the connection. See `server::auth::mcp_oauth`.

use std::sync::Arc;

use rama::http::service::web::extract::{Path, Query, State};
use rama::http::{Request, Response, StatusCode, header};
use serde::Deserialize;

use super::{flow_error_page, require_session_or_redirect};
use aiplane_core::server::auth::mcp_oauth::{self, Overrides};
use aiplane_core::server::db::mcp_catalog::{self, AuthKind};
use aiplane_core::server::db::mcp_connections::{
    self, ConnectionOwner, NewAuthorization, NewConnection,
};
use aiplane_core::server::db::{WriteTx, users::User};
use aiplane_runtime::rama_server::state::RamaState;
use aiplane_runtime::server::tools::mcp::identities::authorization_admitted_in;
use session_core::i18n::{self, Lang, t, t_args};

// ---------------------------------------------------------------------------
// POST /integrations/{key}/connect  → redirect to the provider

pub async fn integrations_connect(
    State(state): State<Arc<RamaState>>,
    Path(key): Path<String>,
    req: Request,
) -> Response {
    let lang = Lang::from_headers(req.headers());
    let (_session, user) = require_session!(state, req);
    connect_for_owner(
        &state,
        &user,
        &key,
        ConnectTarget {
            owner: ConnectionOwner::User(user.id.clone()),
            subject_principal_id: None,
            return_to: None,
            expected_connection: None,
        },
        lang,
    )
    .await
}

pub struct ConnectTarget {
    pub owner: ConnectionOwner,
    pub subject_principal_id: Option<String>,
    pub return_to: Option<String>,
    pub expected_connection: Option<(Option<String>, Option<String>)>,
}
pub async fn connect_for_owner(
    state: &Arc<RamaState>,
    user: &User,
    key: &str,
    target: ConnectTarget,
    lang: Lang,
) -> Response {
    let ConnectTarget {
        owner,
        subject_principal_id,
        return_to,
        expected_connection,
    } = target;
    let connector = match mcp_catalog::get(&state.db, key).await {
        Ok(Some(c)) if c.enabled => c,
        _ => {
            return flow_error_page(
                lang,
                StatusCode::FORBIDDEN,
                &t(lang, "integrations-error-unknown-connector"),
            );
        }
    };
    if matches!(&owner, ConnectionOwner::Principal(_))
        && (!connector.is_per_user() || connector.auth != AuthKind::OAuth2)
    {
        return flow_error_page(
            lang,
            StatusCode::FORBIDDEN,
            &t(lang, "integrations-error-not-oauth"),
        );
    }
    let role_ids = state.rbac.role_ids_for(&user.roles);
    if !connector.allows(&role_ids, state.rbac.is_admin(&role_ids)) {
        return flow_error_page(
            lang,
            StatusCode::FORBIDDEN,
            &t(lang, "integrations-error-forbidden-role"),
        );
    }
    if connector.is_global() {
        // Global connectors are shared by everyone — there's no per-user
        // connection to make. Their tools are already live on this page.
        return redirect("/tools/integrations");
    }
    if connector.auth == AuthKind::None {
        // No credentials to negotiate — the connection row exists only so the
        // user can opt this connector's tools in/out, same as any other.
        let empty = match state.mcp.crypto().seal_str("") {
            Ok(s) => s,
            Err(err) => {
                return flow_error_page(
                    lang,
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &format!("sealing placeholder: {err}"),
                );
            }
        };
        let new = NewConnection {
            owner: owner.clone(),
            connector_key: key.to_owned(),
            access_token_ct: empty.ciphertext,
            access_token_nonce: empty.nonce,
            refresh_token_ct: None,
            refresh_token_nonce: None,
            token_expires_at: None,
            scopes: Vec::new(),
            dcr_client_id: None,
            dcr_client_secret_ct: None,
            dcr_client_secret_nonce: None,
            token_url: None,
        };
        if let Err(err) = mcp_connections::upsert_connection(&state.db, new).await {
            return flow_error_page(
                lang,
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("saving connection: {err}"),
            );
        }
        state
            .mcp
            .invalidate(&ConnectionOwner::User(user.id.clone()), key)
            .await;
        return redirect("/tools/integrations");
    }
    if connector.auth != AuthKind::OAuth2 {
        return flow_error_page(
            lang,
            StatusCode::INTERNAL_SERVER_ERROR,
            &t(lang, "integrations-error-not-oauth"),
        );
    }

    let redirect_uri = format!("{}/integrations/callback", state.public_url());
    let ov = Overrides {
        authorize_url: connector.authorize_url.clone(),
        token_url: connector.token_url.clone(),
        registration_url: connector.registration_url.clone(),
    };
    let endpoints = match mcp_oauth::discover(&connector.url, &ov).await {
        Ok(e) => e,
        Err(err) => {
            return flow_error_page(
                lang,
                StatusCode::INTERNAL_SERVER_ERROR,
                &t_args(
                    lang,
                    "integrations-error-oauth-discovery-failed",
                    &i18n::args([("error", err.to_string().into())]),
                ),
            );
        }
    };

    // Resolve the client identity: a static configured client, or one
    // registered on the fly (DCR). The DCR client is stashed in the pending
    // row so the callback (and later refreshes) can reuse it.
    let (client_id, dcr_client_id, dcr_secret) = if let Some(cid) = connector.client_id.clone() {
        (cid, None, None)
    } else if connector.use_dcr {
        let Some(reg) = endpoints.registration_url.as_deref() else {
            return flow_error_page(
                lang,
                StatusCode::INTERNAL_SERVER_ERROR,
                &t(lang, "integrations-error-needs-setup-no-client"),
            );
        };
        match mcp_oauth::register_client(reg, &redirect_uri, "croit AIplane", &connector.scopes)
            .await
        {
            Ok((id, secret)) => {
                let sealed = match secret.as_deref() {
                    Some(s) => match state.mcp.crypto().seal_str(s) {
                        Ok(x) => Some(x),
                        Err(err) => {
                            return flow_error_page(
                                lang,
                                StatusCode::INTERNAL_SERVER_ERROR,
                                &t_args(
                                    lang,
                                    "integrations-error-sealing-client-secret",
                                    &i18n::args([("error", err.to_string().into())]),
                                ),
                            );
                        }
                    },
                    None => None,
                };
                (id.clone(), Some(id), sealed)
            }
            Err(err) => {
                return flow_error_page(
                    lang,
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &t_args(
                        lang,
                        "integrations-error-dcr-failed",
                        &i18n::args([("error", err.to_string().into())]),
                    ),
                );
            }
        }
    } else {
        return flow_error_page(
            lang,
            StatusCode::INTERNAL_SERVER_ERROR,
            &t(lang, "integrations-error-needs-setup-admin"),
        );
    };

    let pkce = mcp_oauth::pkce();
    let oauth_state = mcp_oauth::random_state();
    let resource = connector.url.clone();
    let authorize_url = match mcp_oauth::build_authorize_url(
        &endpoints.authorize_url,
        &client_id,
        &redirect_uri,
        &connector.scopes,
        &oauth_state,
        &pkce.challenge,
        Some(resource.as_str()),
    ) {
        Ok(u) => u,
        Err(err) => {
            return flow_error_page(
                lang,
                StatusCode::INTERNAL_SERVER_ERROR,
                &t_args(
                    lang,
                    "integrations-error-building-authorize-url",
                    &i18n::args([("error", err.to_string().into())]),
                ),
            );
        }
    };

    let new = NewAuthorization {
        state: oauth_state,
        initiator_user_id: user.id.clone(),
        owner: owner.clone(),
        subject_principal_id: subject_principal_id.clone(),
        connector_key: key.to_owned(),
        pkce_verifier: pkce.verifier,
        redirect_uri,
        token_url: endpoints.token_url,
        resource: Some(resource),
        dcr_client_id,
        dcr_client_secret_ct: dcr_secret.as_ref().map(|s| s.ciphertext.clone()),
        dcr_client_secret_nonce: dcr_secret.as_ref().map(|s| s.nonce.clone()),
        return_to,
    };
    let result = async {
        let mut tx = WriteTx::begin(&state.db).await?;
        if !authorization_admitted_in(
            state,
            &mut tx,
            &user.id,
            &owner,
            key,
            subject_principal_id.as_deref(),
        )
        .await?
        {
            return Ok(StatusCode::FORBIDDEN);
        }
        if let Some((expected_id, expected_generation)) = &expected_connection {
            let current = mcp_connections::get_connection_in(&mut tx, &owner, key).await?;
            let actual = current
                .as_ref()
                .map(|c| (c.id.as_str(), c.generation.as_str()));
            let expected = match (expected_id.as_deref(), expected_generation.as_deref()) {
                (None, None) => None,
                (Some(id), Some(generation)) => Some((id, generation)),
                _ => return Ok(StatusCode::CONFLICT),
            };
            if actual != expected {
                return Ok(StatusCode::CONFLICT);
            }
        }
        mcp_connections::begin_authorization_in(&mut tx, new).await?;
        tx.commit().await?;
        Ok::<StatusCode, aiplane_core::server::db::DbError>(StatusCode::OK)
    }
    .await;
    if let Ok(status) = &result
        && *status != StatusCode::OK
    {
        return flow_error_page(lang, *status, &t(lang, "integrations-error-auth-expired"));
    }
    if let Err(err) = result {
        return flow_error_page(
            lang,
            StatusCode::INTERNAL_SERVER_ERROR,
            &t_args(
                lang,
                "integrations-error-persisting-authorization",
                &i18n::args([("error", err.to_string().into())]),
            ),
        );
    }
    redirect(&authorize_url)
}

// ---------------------------------------------------------------------------
// GET /integrations/callback

#[derive(Deserialize)]
pub struct CallbackParams {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

pub async fn integrations_callback(
    State(state): State<Arc<RamaState>>,
    Query(params): Query<CallbackParams>,
    req: Request,
) -> Response {
    let lang = Lang::from_headers(req.headers());
    let (_session, user) = require_session!(state, req);
    let Some(st) = params.state else {
        return flow_error_page(
            lang,
            StatusCode::INTERNAL_SERVER_ERROR,
            &t(lang, "integrations-error-callback-missing"),
        );
    };
    let pending = match mcp_connections::take_pending(&state.db, &st).await {
        Ok(Some(p)) => p,
        Ok(None) => {
            return flow_error_page(
                lang,
                StatusCode::INTERNAL_SERVER_ERROR,
                &t(lang, "integrations-error-auth-expired"),
            );
        }
        Err(err) => {
            return flow_error_page(
                lang,
                StatusCode::INTERNAL_SERVER_ERROR,
                &t_args(
                    lang,
                    "integrations-error-loading-authorization",
                    &i18n::args([("error", err.to_string().into())]),
                ),
            );
        }
    };
    if pending.initiator_user_id != user.id {
        retire_authorization(&state, &pending).await;
        return flow_error_page(
            lang,
            StatusCode::FORBIDDEN,
            &t(lang, "integrations-error-state-mismatch"),
        );
    }
    if let Some(err) = params.error {
        retire_authorization(&state, &pending).await;
        let desc = params.error_description.unwrap_or_default();
        return flow_error_page(
            lang,
            StatusCode::INTERNAL_SERVER_ERROR,
            &t_args(
                lang,
                "integrations-error-provider-error",
                &i18n::args([("error", err.into()), ("desc", desc.into())]),
            ),
        );
    }
    let Some(code) = params.code else {
        retire_authorization(&state, &pending).await;
        return flow_error_page(
            lang,
            StatusCode::INTERNAL_SERVER_ERROR,
            &t(lang, "integrations-error-callback-missing"),
        );
    };
    let admitted = async {
        let mut tx = WriteTx::begin(&state.db).await?;
        let allowed = authorization_admitted_in(
            &state,
            &mut tx,
            &user.id,
            &pending.owner,
            &pending.connector_key,
            pending.subject_principal_id.as_deref(),
        )
        .await?
            && mcp_connections::authorization_is_current_in(&mut tx, &pending).await?;
        tx.commit().await?;
        Ok::<bool, aiplane_core::server::db::DbError>(allowed)
    }
    .await;
    if !matches!(admitted, Ok(true)) {
        retire_authorization(&state, &pending).await;
        return flow_error_page(
            lang,
            StatusCode::FORBIDDEN,
            &t(lang, "integrations-error-forbidden-role"),
        );
    }
    let connector = match mcp_catalog::get(&state.db, &pending.connector_key).await {
        Ok(Some(c)) => c,
        _ => {
            retire_authorization(&state, &pending).await;
            return flow_error_page(
                lang,
                StatusCode::INTERNAL_SERVER_ERROR,
                &t(lang, "integrations-error-connector-missing"),
            );
        }
    };

    // Client credentials: the DCR client minted at connect time, else the
    // catalog's static client.
    let (client_id, client_secret) = if let Some(dcr) = pending.dcr_client_id.clone() {
        let secret = match (
            &pending.dcr_client_secret_ct,
            &pending.dcr_client_secret_nonce,
        ) {
            (Some(ct), Some(nonce)) => match state.mcp.crypto().open_str(nonce, ct) {
                Ok(s) => Some(s),
                Err(err) => {
                    retire_authorization(&state, &pending).await;
                    return flow_error_page(
                        lang,
                        StatusCode::INTERNAL_SERVER_ERROR,
                        &t_args(
                            lang,
                            "integrations-error-decrypting-client-secret",
                            &i18n::args([("error", err.to_string().into())]),
                        ),
                    );
                }
            },
            _ => None,
        };
        (dcr, secret)
    } else {
        let Some(cid) = connector.client_id.clone() else {
            retire_authorization(&state, &pending).await;
            return flow_error_page(
                lang,
                StatusCode::INTERNAL_SERVER_ERROR,
                &t(lang, "integrations-error-connector-missing-client-id"),
            );
        };
        let secret = match state.mcp.decrypt_connector_secret(&connector) {
            Ok(s) => s,
            Err(err) => {
                retire_authorization(&state, &pending).await;
                return flow_error_page(
                    lang,
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &t_args(
                        lang,
                        "integrations-error-decrypting-client-secret",
                        &i18n::args([("error", err.to_string().into())]),
                    ),
                );
            }
        };
        (cid, secret)
    };

    let resource = pending
        .resource
        .clone()
        .unwrap_or_else(|| connector.url.clone());
    let tokens = match mcp_oauth::exchange_code(
        &pending.token_url,
        &code,
        &pending.pkce_verifier,
        &pending.redirect_uri,
        &client_id,
        client_secret.as_deref(),
        Some(resource.as_str()),
    )
    .await
    {
        Ok(t) => t,
        Err(err) => {
            retire_authorization(&state, &pending).await;
            return flow_error_page(lang, StatusCode::INTERNAL_SERVER_ERROR, &err.to_string());
        }
    };

    // Seal everything before it touches the DB.
    let access = match state.mcp.crypto().seal_str(&tokens.access_token) {
        Ok(s) => s,
        Err(err) => {
            retire_authorization(&state, &pending).await;
            return flow_error_page(
                lang,
                StatusCode::INTERNAL_SERVER_ERROR,
                &t_args(
                    lang,
                    "integrations-error-sealing-access-token",
                    &i18n::args([("error", err.to_string().into())]),
                ),
            );
        }
    };
    let refresh = match tokens.refresh_token.as_deref() {
        Some(rt) => match state.mcp.crypto().seal_str(rt) {
            Ok(s) => Some(s),
            Err(err) => {
                retire_authorization(&state, &pending).await;
                return flow_error_page(
                    lang,
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &t_args(
                        lang,
                        "integrations-error-sealing-refresh-token",
                        &i18n::args([("error", err.to_string().into())]),
                    ),
                );
            }
        },
        None => None,
    };
    let scopes = if tokens.scopes.is_empty() {
        connector.scopes.clone()
    } else {
        tokens.scopes.clone()
    };

    let new = NewConnection {
        owner: pending.owner.clone(),
        connector_key: pending.connector_key.clone(),
        access_token_ct: access.ciphertext,
        access_token_nonce: access.nonce,
        refresh_token_ct: refresh.as_ref().map(|s| s.ciphertext.clone()),
        refresh_token_nonce: refresh.as_ref().map(|s| s.nonce.clone()),
        token_expires_at: tokens.expires_at,
        scopes,
        dcr_client_id: pending.dcr_client_id.clone(),
        dcr_client_secret_ct: pending.dcr_client_secret_ct.clone(),
        dcr_client_secret_nonce: pending.dcr_client_secret_nonce.clone(),
        token_url: Some(pending.token_url.clone()),
    };
    let saved = async {
        let mut tx = WriteTx::begin(&state.db).await?;
        if !authorization_admitted_in(
            &state,
            &mut tx,
            &user.id,
            &pending.owner,
            &pending.connector_key,
            pending.subject_principal_id.as_deref(),
        )
        .await?
        {
            return Ok(None);
        }
        let identity = mcp_connections::complete_authorization_in(&mut tx, &pending, new).await?;
        tx.commit().await?;
        Ok::<_, aiplane_core::server::db::DbError>(identity)
    }
    .await;
    if matches!(saved, Ok(None)) {
        retire_authorization(&state, &pending).await;
        return flow_error_page(
            lang,
            StatusCode::FORBIDDEN,
            &t(lang, "integrations-error-auth-expired"),
        );
    }
    if let Err(err) = saved {
        retire_authorization(&state, &pending).await;
        return flow_error_page(
            lang,
            StatusCode::INTERNAL_SERVER_ERROR,
            &t_args(
                lang,
                "integrations-error-saving-connection",
                &i18n::args([("error", err.to_string().into())]),
            ),
        );
    }
    state
        .mcp
        .invalidate(&pending.owner, &pending.connector_key)
        .await;
    redirect(
        pending
            .return_to
            .as_deref()
            .filter(|path| aiplane_core::rama_server::session::is_safe_return_to(path))
            .unwrap_or("/tools/integrations"),
    )
}

// ---------------------------------------------------------------------------
// POST /integrations/{key}/token  (static-bearer connectors: user-supplied token)

// ---------------------------------------------------------------------------
// POST /integrations/{key}/retry — drop the cached connection and re-attempt
// on the next load (for transient "couldn't load tools" failures + token
// connectors). OAuth connectors use /connect to fully re-authorize instead.

pub async fn integrations_retry(
    State(state): State<Arc<RamaState>>,
    Path(key): Path<String>,
    req: Request,
) -> Response {
    let (_session, user) = require_session!(state, req);
    state
        .mcp
        .invalidate(&ConnectionOwner::User(user.id.clone()), &key)
        .await;
    redirect("/tools/integrations")
}

// ---------------------------------------------------------------------------
// POST /integrations/{key}/disconnect

// ---------------------------------------------------------------------------
// POST /integrations/{key}/tools/mode

// ---------------------------------------------------------------------------
// POST /integrations/{key}/tools/all — set every tool of a connector to one mode

// ---------------------------------------------------------------------------
// Rendering

fn redirect(location: &str) -> Response {
    Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header(header::LOCATION, location)
        .body("".into())
        .unwrap()
}

async fn retire_authorization(state: &RamaState, pending: &mcp_connections::PendingOauth) {
    if let Err(error) = mcp_connections::cancel_authorization(&state.db, pending).await {
        tracing::warn!(%error,"retiring MCP authorization failed");
    }
}
