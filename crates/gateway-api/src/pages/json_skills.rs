// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Skills + connectors + feedback + ComfyUI JSON surfaces for the SPA
//! (issue #22, P5). Thin JSON translations of the legacy handlers; the
//! legacy pages stay alive until phase 6.

use std::sync::Arc;

use rama::http::service::web::extract::{Path, State};
use rama::http::{Request, Response, StatusCode};

use gateway_core::server::db;
use gateway_runtime::rama_server::state::RamaState;

use super::{json_error, json_ok, require_admin_json, require_session_json};

fn bad_request(message: impl Into<String>) -> Response {
    json_error(StatusCode::BAD_REQUEST, "invalid_request", &message.into())
}

fn internal(message: impl std::fmt::Display) -> Response {
    json_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal_error",
        &message.to_string(),
    )
}

fn no_content() -> Response {
    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(rama::http::Body::empty())
        .expect("static empty response")
}

// ---------------------------------------------------------------------------
// Skills (user + admin)

fn skill_json(
    skill: &gateway_features::server::skills::Skill,
    body: Option<String>,
) -> serde_json::Value {
    serde_json::json!({
        "name": skill.name,
        "title": skill.title,
        "description": skill.description,
        "files": skill.files(),
        "body": body,
    })
}

/// GET /api/v0/skills — the caller's effective skills: the global set their
/// roles grant plus their private ones (which shadow on name collision).
pub async fn skills_list(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, user) = match require_session_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let Some(user_store) = state.user_skills() else {
        // The feature is off, so the caller has no personal skills — and this
        // is the personal-skills surface. Falling back to `state.skills()`
        // would list the whole operator catalog, which is NOT role-filtered
        // by `skill_grants`, so a user would see (and via `/archive`
        // download) skills granted only to other roles. The page this
        // replaced returned an empty list here for exactly that reason.
        return json_ok(
            StatusCode::OK,
            serde_json::json!({ "skills": [], "user_skills_enabled": false }),
        );
    };
    let registry = user_store.registry_for(&user.id);
    let skills = registry
        .names()
        .filter_map(|n| registry.get(n))
        .map(|s| skill_json(s, None))
        .collect::<Vec<_>>();
    json_ok(
        StatusCode::OK,
        serde_json::json!({ "skills": skills, "user_skills_enabled": true }),
    )
}

/// GET /api/v0/skills/{name}/body — the SKILL.md body (frontmatter
/// stripped) for the caller's effective registry.
pub async fn skill_body(
    Path(name): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = match require_session_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    // No fallback to `state.skills()`: that registry is the operator catalog,
    // unfiltered by `skill_grants`, so serving it here would hand any signed-in
    // caller a skill granted only to another role. With personal skills off
    // there is simply nothing on this surface to resolve.
    let Some(store) = state.user_skills() else {
        return json_error(StatusCode::NOT_FOUND, "not_found", "no such skill");
    };
    let registry = store.registry_for(&user.id);
    let Some(skill) = registry.get(&name) else {
        return json_error(StatusCode::NOT_FOUND, "not_found", "no such skill");
    };
    match skill.body() {
        Ok(body) => json_ok(
            StatusCode::OK,
            serde_json::json!({ "name": name, "body": body }),
        ),
        Err(err) => internal(err),
    }
}

/// GET /api/v0/skills/{name}/archive — the skill packaged as a `.skill`
/// archive, so one can be moved between gateways (or kept as a backup).
///
/// A file download, not JSON: the body is the archive. Resolves against the
/// caller's private registry only — see the note in the body.
pub async fn skill_archive(
    Path(name): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = match require_session_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    // No fallback to `state.skills()`: that registry is the operator catalog,
    // unfiltered by `skill_grants`, so serving it here would hand any signed-in
    // caller a skill granted only to another role. With personal skills off
    // there is simply nothing on this surface to resolve.
    let Some(store) = state.user_skills() else {
        return json_error(StatusCode::NOT_FOUND, "not_found", "no such skill");
    };
    let registry = store.registry_for(&user.id);
    let Some(skill) = registry.get(&name) else {
        return json_error(StatusCode::NOT_FOUND, "not_found", "no such skill");
    };
    match skill.to_archive() {
        Ok(bytes) => Response::builder()
            .status(StatusCode::OK)
            .header(rama::http::header::CONTENT_TYPE, "application/zip")
            .header(rama::http::header::CONTENT_LENGTH, bytes.len())
            .header(
                rama::http::header::CONTENT_DISPOSITION,
                // `name` is validated to `[A-Za-z0-9._-]` on save, so it is
                // safe to interpolate into the filename unescaped.
                format!("attachment; filename=\"{name}.skill\""),
            )
            .header(rama::http::header::CACHE_CONTROL, "no-store")
            .body(bytes.into())
            .unwrap_or_else(|_| internal("packaging the skill failed")),
        Err(err) => {
            tracing::warn!(skill = %name, error = %err, "packaging skill for download");
            internal(err)
        }
    }
}

/// POST /api/v0/skills — upload a private `.skill` archive (multipart with
/// a `file` part) or author inline (`{"name":…, "manifest":"…"}`).
pub async fn skills_upload(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, user) = match require_session_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let Some(store) = state.user_skills() else {
        return json_error(
            StatusCode::FORBIDDEN,
            "forbidden",
            "personal skills are not enabled on this gateway",
        );
    };
    let content_type = req
        .headers()
        .get(rama::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return bad_request(msg),
    };
    if content_type.starts_with("multipart/form-data;") {
        let boundary = multer::parse_boundary(&content_type)
            .map_err(|e| format!("malformed multipart body: {e}"))
            .unwrap_or_default();
        if boundary.is_empty() {
            return bad_request("malformed multipart body");
        }
        let stream =
            rama::futures::stream::once(async move { Ok::<_, std::convert::Infallible>(bytes) });
        let mut mp = multer::Multipart::new(stream, boundary);
        let field = mp
            .next_field()
            .await
            .map_err(|e| format!("reading the upload: {e}"))
            .ok()
            .flatten();
        let Some(field) = field.filter(|f| f.name() == Some("file")) else {
            return bad_request("no `file` part in the upload");
        };
        let data = field
            .bytes()
            .await
            .map_err(|e| e.to_string())
            .unwrap_or_default();
        if data.is_empty() {
            return bad_request("the archive part is empty");
        }
        return match store.install_archive(&user.id, &data) {
            Ok(name) => json_ok(StatusCode::CREATED, serde_json::json!({ "name": name })),
            Err(err) => bad_request(err.to_string()),
        };
    }
    // Inline authoring.
    #[derive(serde::Deserialize)]
    struct InlineBody {
        name: String,
        manifest: String,
    }
    let Ok(parsed) = serde_json::from_slice::<InlineBody>(&bytes) else {
        return bad_request("expected multipart file upload or {name, manifest} JSON");
    };
    match store.save_manifest(&user.id, &parsed.name, &parsed.manifest) {
        Ok(_) => json_ok(
            StatusCode::CREATED,
            serde_json::json!({ "name": parsed.name }),
        ),
        Err(err) => bad_request(err.to_string()),
    }
}

/// DELETE /api/v0/skills/{name} — remove one of the caller's private skills.
pub async fn skills_delete(
    Path(name): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = match require_session_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let Some(store) = state.user_skills() else {
        return json_error(
            StatusCode::FORBIDDEN,
            "forbidden",
            "personal skills are not enabled on this gateway",
        );
    };
    match store.remove(&user.id, &name) {
        Ok(true) => no_content(),
        Ok(false) => json_error(StatusCode::NOT_FOUND, "not_found", "no such private skill"),
        Err(err) => internal(err),
    }
}

// ---------------------------------------------------------------------------
// Admin skills

/// GET /api/v0/admin/skills — the global catalog + the role-grant overlay.
pub async fn admin_skills_list(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = match require_admin_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let skills = match state.skills() {
        Some(store) => {
            let registry = store.current();
            registry
                .names()
                .filter_map(|n| registry.get(n))
                .map(|skill| skill_json(skill, None))
                .collect::<Vec<_>>()
        }
        None => Vec::new(),
    };
    // `all` yields (skill, role) pairs; fold into per-skill role lists.
    let mut grant_map: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for (skill, role) in db::skill_grants::all(&state.db).await.unwrap_or_default() {
        grant_map.entry(skill).or_default().push(role);
    }
    let grant_json: Vec<_> = grant_map
        .into_iter()
        .map(|(skill, roles)| serde_json::json!({ "skill": skill, "roles": roles }))
        .collect();
    json_ok(
        StatusCode::OK,
        serde_json::json!({
            "skills": skills,
            "grants": grant_json,
            "groups": db::gateway_groups::list_groups(&state.db)
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|g| g.name)
                .collect::<Vec<_>>(),
        }),
    )
}

/// POST /api/v0/admin/skills — install a `.skill` archive into the global
/// catalog (multipart `file` part). Live without a restart.
pub async fn admin_skills_upload(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = match require_admin_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let Some(store) = state.skills() else {
        return internal("the skills directory is not configured");
    };
    let content_type = req
        .headers()
        .get(rama::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let Some(boundary) = multer::parse_boundary(&content_type).ok() else {
        return bad_request("expected a multipart upload");
    };
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return bad_request(msg),
    };
    let stream =
        rama::futures::stream::once(async move { Ok::<_, std::convert::Infallible>(bytes) });
    let mut mp = multer::Multipart::new(stream, boundary);
    let field = mp
        .next_field()
        .await
        .map_err(|e| format!("reading the upload: {e}"))
        .ok()
        .flatten();
    let Some(field) = field.filter(|f| f.name() == Some("file")) else {
        return bad_request("no `file` part in the upload");
    };
    let data = field
        .bytes()
        .await
        .map_err(|e| e.to_string())
        .unwrap_or_default();
    if data.is_empty() {
        return bad_request("the archive part is empty");
    }
    match store.install_archive(&data) {
        Ok(name) => json_ok(StatusCode::CREATED, serde_json::json!({ "name": name })),
        Err(err) => bad_request(err.to_string()),
    }
}

/// DELETE /api/v0/admin/skills/{name} — remove a global skill + its grants.
pub async fn admin_skills_delete(
    Path(name): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, _admin) = match require_admin_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let Some(store) = state.skills() else {
        return internal("the skills directory is not configured");
    };
    let _ = db::skill_grants::delete_skill(&state.db, &name).await;
    match store.remove(&name) {
        Ok(true) => no_content(),
        Ok(false) => json_error(StatusCode::NOT_FOUND, "not_found", "no such skill"),
        Err(err) => internal(err),
    }
}

#[derive(serde::Deserialize)]
pub struct SkillGrantsBody {
    pub skill: String,
    pub roles: Vec<String>,
}

/// PUT /api/v0/admin/skills/grants — replace a skill's role-grant overlay
/// (on top of the static `[[roles]].skills` config), live via the RBAC
/// overlay.
pub async fn admin_skills_grants(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = match require_admin_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return bad_request(msg),
    };
    let parsed: SkillGrantsBody = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => return bad_request(format!("parsing the grants body: {err}")),
    };
    if let Err(err) = db::skill_grants::set_for_skill(&state.db, &parsed.skill, &parsed.roles).await
    {
        return internal(err);
    }
    // Re-seed the resolver overlay exactly like the form path.
    let grants = db::skill_grants::all(&state.db).await.unwrap_or_default();
    state.rbac.set_skill_grant_overlay(grants);
    json_ok(
        StatusCode::OK,
        serde_json::json!({ "skill": parsed.skill, "roles": parsed.roles }),
    )
}

// ---------------------------------------------------------------------------
// Admin connectors (MCP catalog)

/// GET /api/v0/admin/connectors — the MCP catalog + recent audit.
pub async fn admin_connectors_list(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = match require_admin_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let connectors = db::mcp_catalog::list_all(&state.db)
        .await
        .unwrap_or_default();
    let out: Vec<_> = connectors
        .iter()
        .map(|c| {
            serde_json::json!({
                "key": c.key,
                "title": c.name,
                "description": c.description,
                "base_url": c.url,
                "auth_type": c.auth.as_str(),
                "scopes": c.scopes,
                "enabled": c.enabled,
                "audit": c.audit,
                "groups": c.allowed_groups,
            })
        })
        .collect();
    json_ok(StatusCode::OK, serde_json::json!({ "connectors": out }))
}

#[derive(serde::Deserialize)]
pub struct ConnectorInputBody {
    pub key: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub base_url: String,
    #[serde(default)]
    pub auth_type: String,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub client_secret: String,
    #[serde(default)]
    pub groups: Vec<String>,
    #[serde(default)]
    pub audit: bool,
    #[serde(default)]
    pub overwrite: bool,
}

/// PUT /api/v0/admin/connectors — create/update a catalog entry. The
/// client secret is sealed before storage.
pub async fn admin_connectors_save(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = match require_admin_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return bad_request(msg),
    };
    let parsed: ConnectorInputBody = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => return bad_request(format!("parsing the connector body: {err}")),
    };
    if parsed.key.trim().is_empty() || parsed.base_url.trim().is_empty() {
        return bad_request("a connector needs a key and a base URL");
    }
    let existing = db::mcp_catalog::get(&state.db, &parsed.key)
        .await
        .ok()
        .flatten();
    if existing.is_some() && !parsed.overwrite {
        return json_error(
            StatusCode::CONFLICT,
            "name_exists",
            &format!("connector {} exists — resend with overwrite", parsed.key),
        );
    }
    // Seal a newly entered secret; blank keeps the stored one.
    let (secret_ct, secret_nonce) = if parsed.client_secret.trim().is_empty() {
        (
            existing.as_ref().and_then(|c| c.client_secret_ct.clone()),
            existing
                .as_ref()
                .and_then(|c| c.client_secret_nonce.clone()),
        )
    } else {
        match state.crypto.seal_str(parsed.client_secret.trim()) {
            Ok(s) => (Some(s.ciphertext), Some(s.nonce)),
            Err(err) => return internal(err),
        }
    };
    let auth = db::mcp_catalog::AuthKind::parse(&parsed.auth_type);
    let input = db::mcp_catalog::ConnectorInput {
        key: parsed.key.clone(),
        name: parsed.title,
        description: (!parsed.description.trim().is_empty()).then(|| parsed.description.clone()),
        icon: None,
        category: None,
        url: parsed.base_url.trim().to_string(),
        auth,
        scope: existing
            .as_ref()
            .map(|c| c.scope)
            .unwrap_or(db::mcp_catalog::Scope::PerUser),
        audit: parsed.audit,
        use_dcr: existing.as_ref().map(|c| c.use_dcr).unwrap_or(false),
        client_id: (!parsed.client_id.trim().is_empty()).then(|| parsed.client_id.clone()),
        client_secret_ct: secret_ct,
        client_secret_nonce: secret_nonce,
        authorize_url: existing.as_ref().and_then(|c| c.authorize_url.clone()),
        token_url: existing.as_ref().and_then(|c| c.token_url.clone()),
        registration_url: existing.as_ref().and_then(|c| c.registration_url.clone()),
        scopes: parsed.scopes,
        allowed_groups: parsed.groups,
    };
    let result = if existing.is_some() {
        db::mcp_catalog::update(&state.db, &parsed.key, input)
            .await
            .map(|_| ())
    } else {
        db::mcp_catalog::create(&state.db, input).await
    };
    match result {
        Ok(()) => json_ok(StatusCode::OK, serde_json::json!({ "key": parsed.key })),
        Err(err) => internal(err),
    }
}

/// POST /api/v0/admin/connectors/{key}/toggle
pub async fn admin_connectors_toggle(
    Path(key): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, _admin) = match require_admin_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return bad_request(msg),
    };
    let parsed: super::json_workspace::EnabledBody = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => return bad_request(format!("parsing the toggle body: {err}")),
    };
    match db::mcp_catalog::set_enabled(&state.db, &key, parsed.enabled).await {
        Ok(_) => json_ok(
            StatusCode::OK,
            serde_json::json!({ "key": key, "enabled": parsed.enabled }),
        ),
        Err(err) => internal(err),
    }
}

/// DELETE /api/v0/admin/connectors/{key} — cascades every user connection.
pub async fn admin_connectors_delete(
    Path(key): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, _admin) = match require_admin_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let _ = db::user_mcp::delete_all_for_connector(&state.db, &key).await;
    match db::mcp_catalog::delete(&state.db, &key).await {
        Ok(_) => no_content(),
        Err(err) => internal(err),
    }
}

/// POST /api/v0/admin/connectors/restore-defaults — re-seed the built-in
/// catalog entries.
pub async fn admin_connectors_restore_defaults(
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, _admin) = match require_admin_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    match db::mcp_catalog::seed_defaults(&state.db).await {
        Ok(count) => json_ok(StatusCode::OK, serde_json::json!({ "seeded": count })),
        Err(err) => internal(err),
    }
}

// ---------------------------------------------------------------------------
// Integrations (user connections)

/// GET /api/v0/integrations — the enabled connectors + the caller's
/// connection state per connector.
pub async fn integrations_list(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, user) = match require_session_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let connectors = db::mcp_catalog::list_enabled(&state.db)
        .await
        .unwrap_or_default();
    // A connector carries its own role allowlist, and listing one the caller
    // cannot use both leaks the operator's integration inventory and offers a
    // "Connect" button that the connect endpoints correctly refuse.
    let role_ids = state.rbac.role_ids_for(&user.roles);
    let is_admin = state.rbac.is_admin(&role_ids);
    let connectors: Vec<_> = connectors
        .into_iter()
        .filter(|c| c.allows(&role_ids, is_admin))
        .collect();
    let mut out = Vec::with_capacity(connectors.len());
    for c in &connectors {
        let connected = db::user_mcp::get_connection(&state.db, &user.id, &c.key)
            .await
            .ok()
            .flatten()
            .is_some();
        out.push(serde_json::json!({
            "key": c.key,
            "title": c.name,
            "description": c.description,
            "auth_type": c.auth.as_str(),
            "connected": connected,
        }));
    }
    json_ok(StatusCode::OK, serde_json::json!({ "connectors": out }))
}

#[derive(serde::Deserialize)]
pub struct TokenConnectBody {
    pub token: String,
}

/// POST /api/v0/integrations/{key}/token — connect a static-token
/// connector (token sealed at rest).
pub async fn integrations_connect_token(
    Path(key): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = match require_session_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return bad_request(msg),
    };
    let parsed: TokenConnectBody = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => return bad_request(format!("parsing the token body: {err}")),
    };
    let Some(connector) = db::mcp_catalog::get(&state.db, &key).await.ok().flatten() else {
        return json_error(StatusCode::NOT_FOUND, "not_found", "no such connector");
    };
    // Every one of these gates the *credential* this endpoint seals, so none
    // of them is cosmetic. The auth-kind check in particular is the only one
    // there is: `MoreMcp::ensure` sends a stored token as a bearer for any
    // `auth != None`, so writing a connection row for an OAuth2 connector here
    // would dispatch its tools with a caller-supplied credential and skip the
    // PKCE flow, the consent screen and the scope grant entirely.
    if !connector.enabled {
        return json_error(StatusCode::NOT_FOUND, "not_found", "no such connector");
    }
    let role_ids = state.rbac.role_ids_for(&user.roles);
    if !connector.allows(&role_ids, state.rbac.is_admin(&role_ids)) {
        return json_error(
            StatusCode::FORBIDDEN,
            "forbidden",
            "your roles do not grant this connector",
        );
    }
    if connector.is_global() {
        // A global static-bearer connector carries one token on the connector
        // row, set by an admin — there is no per-user credential to write.
        return json_error(
            StatusCode::CONFLICT,
            "conflict",
            "this connector is configured globally by an administrator",
        );
    }
    if connector.auth != db::mcp_catalog::AuthKind::StaticBearer {
        return json_error(
            StatusCode::CONFLICT,
            "conflict",
            "this connector does not authenticate with a static token",
        );
    }
    let token = parsed.token.trim();
    if token.is_empty() {
        // An empty token seals fine and leaves the connector permanently
        // "connected", sending `Authorization: Bearer ` on every call.
        return bad_request("a token is required".to_string());
    }
    let sealed = match state.crypto.seal_str(token) {
        Ok(s) => s,
        Err(err) => return internal(err),
    };
    let new = db::user_mcp::NewConnection {
        user_id: user.id.clone(),
        connector_key: key.clone(),
        access_token_ct: sealed.ciphertext,
        access_token_nonce: sealed.nonce,
        refresh_token_ct: None,
        refresh_token_nonce: None,
        token_expires_at: None,
        scopes: Vec::new(),
        dcr_client_id: None,
        dcr_client_secret_ct: None,
        dcr_client_secret_nonce: None,
        token_url: None,
    };
    if let Err(err) = db::user_mcp::upsert_connection(&state.db, new).await {
        return internal(err);
    }
    state.mcp.invalidate(&user.id, &key).await;
    json_ok(
        StatusCode::OK,
        serde_json::json!({ "key": key, "connected": true }),
    )
}

/// POST /api/v0/integrations/{key}/disconnect
pub async fn integrations_disconnect(
    Path(key): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = match require_session_json(&state, &req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    match db::user_mcp::delete_connection(&state.db, &user.id, &key).await {
        Ok(_) => {
            state.mcp.invalidate(&user.id, &key).await;
            no_content()
        }
        Err(err) => internal(err),
    }
}
