// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Skills + connectors + feedback + ComfyUI JSON surfaces for the SPA
//! (issue #22, P5). Thin JSON translations of the legacy handlers; the
//! legacy pages stay alive until phase 6.

use std::collections::HashMap;
use std::sync::Arc;

use rama::http::service::web::extract::State;
use rama::http::{Request, Response, StatusCode};

use aiplane_core::server::db;
use aiplane_core::server::db::user_mcp::ToolMode;
use aiplane_runtime::rama_server::state::RamaState;

use super::{bad_request, internal, json_error, json_ok, no_content, raw_path_segment};

// ---------------------------------------------------------------------------
// Skills (user + admin)

fn skill_json(
    skill: &aiplane_features::server::skills::Skill,
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

fn skill_archive_response(name: &str, skill: &aiplane_features::server::skills::Skill) -> Response {
    match skill.to_archive() {
        Ok(bytes) => Response::builder()
            .status(StatusCode::OK)
            .header(rama::http::header::CONTENT_TYPE, "application/zip")
            .header(rama::http::header::CONTENT_LENGTH, bytes.len())
            .header(
                rama::http::header::CONTENT_DISPOSITION,
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

/// GET /api/v0/skills — the caller's effective skills: the global set their
/// roles grant plus their private ones (which shadow on name collision).
pub async fn skills_list(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, user) = require_session_json!(state, req);
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
pub async fn skill_body(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, user) = require_session_json!(state, req);
    // Raw URI, not the `Path` extractor: rama lowercases path segments and
    // never percent-decodes them, and a skill name is stored with its case intact.
    let Some(name) = raw_path_segment(&req, 1) else {
        return bad_request("the URL is missing its skill name");
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
    match (skill.body(), skill.manifest_text()) {
        (Ok(body), Ok(manifest)) => json_ok(
            StatusCode::OK,
            serde_json::json!({ "name": name, "body": body, "manifest": manifest }),
        ),
        (Err(err), _) | (_, Err(err)) => internal(err),
    }
}

/// GET /api/v0/skills/{name}/archive — the skill packaged as a `.skill`
/// archive, so one can be moved between gateways (or kept as a backup).
///
/// A file download, not JSON: the body is the archive. Resolves against the
/// caller's private registry only — see the note in the body.
pub async fn skill_archive(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, user) = require_session_json!(state, req);
    // Raw URI, not the `Path` extractor: rama lowercases path segments and
    // never percent-decodes them, and a skill name is stored with its case intact.
    let Some(name) = raw_path_segment(&req, 1) else {
        return bad_request("the URL is missing its skill name");
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
    skill_archive_response(&name, skill)
}

/// POST /api/v0/skills — upload a private `.skill` archive (multipart with
/// a `file` part) or author inline (`{"name":…, "manifest":"…"}`).
pub async fn skills_upload(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, user) = require_session_json!(state, req);
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
    let target = if parsed.name.trim().is_empty() {
        match aiplane_features::server::skills::manifest_name(&parsed.manifest) {
            Some(name) => name,
            None => return bad_request("the SKILL.md frontmatter needs a valid name"),
        }
    } else {
        parsed.name
    };
    match store.save_manifest(&user.id, &target, &parsed.manifest) {
        Ok(_) => json_ok(StatusCode::CREATED, serde_json::json!({ "name": target })),
        Err(err) => bad_request(err.to_string()),
    }
}

/// DELETE /api/v0/skills/{name} — remove one of the caller's private skills.
pub async fn skills_delete(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, user) = require_session_json!(state, req);
    // Raw URI, not the `Path` extractor: rama lowercases path segments and
    // never percent-decodes them, and a skill name is stored with its case intact.
    let Some(name) = raw_path_segment(&req, 0) else {
        return bad_request("the URL is missing its skill name");
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
    let (_session, _admin) = require_admin_json!(state, req);
    let grants = db::skill_grants::all(&state.db).await.unwrap_or_default();
    let mut grant_map: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for (skill, group) in grants {
        grant_map.entry(skill).or_default().push(group);
    }
    for groups in grant_map.values_mut() {
        groups.sort();
    }
    let all_skills_groups = grant_map.remove("*").unwrap_or_default();
    let skills = match state.skills() {
        Some(store) => {
            let registry = store.current();
            registry
                .names()
                .filter_map(|n| registry.get(n))
                .map(|skill| {
                    let mut value = skill_json(skill, skill.body().ok());
                    value["all_skills_groups"] = serde_json::json!(all_skills_groups);
                    value["granted_groups"] =
                        serde_json::json!(grant_map.get(&skill.name).cloned().unwrap_or_default());
                    value
                })
                .collect::<Vec<_>>()
        }
        None => Vec::new(),
    };
    let source = state
        .config()
        .skills
        .as_ref()
        .map(|skills| skills.dir.display().to_string());
    json_ok(
        StatusCode::OK,
        serde_json::json!({
            "skills": skills,
            "configured": state.skills().is_some(),
            "directory_accessible": !state.skills_dir_inaccessible(),
            "source": source,
            "groups": db::gateway_groups::list_group_names(&state.db)
                .await
                .unwrap_or_default(),
        }),
    )
}

/// GET /api/v0/admin/skills/{name}/archive — package one global skill for
/// download. Admin-gated because the global catalog is not role-filtered.
pub async fn admin_skill_archive(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = require_admin_json!(state, req);
    let Some(name) = raw_path_segment(&req, 1) else {
        return bad_request("the URL is missing its skill name");
    };
    let Some(store) = state.skills() else {
        return internal("the skills directory is not configured");
    };
    let registry = store.current();
    let Some(skill) = registry.get(&name) else {
        return json_error(StatusCode::NOT_FOUND, "not_found", "no such skill");
    };
    skill_archive_response(&name, skill)
}

/// POST /api/v0/admin/skills — install a `.skill` archive into the global
/// catalog (multipart `file` part). Live without a restart.
pub async fn admin_skills_upload(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = require_admin_json!(state, req);
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
pub async fn admin_skills_delete(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = require_admin_json!(state, req);
    // Raw URI, not the `Path` extractor: rama lowercases path segments and
    // never percent-decodes them, and a skill name is stored with its case intact.
    let Some(name) = raw_path_segment(&req, 0) else {
        return bad_request("the URL is missing its skill name");
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
    let (_session, _admin) = require_admin_json!(state, req);
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return bad_request(msg),
    };
    let parsed: SkillGrantsBody = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => return bad_request(format!("parsing the grants body: {err}")),
    };
    let Some(store) = state.skills() else {
        return internal("the skills directory is not configured");
    };
    if store.current().get(&parsed.skill).is_none() {
        return json_error(StatusCode::NOT_FOUND, "not_found", "no such skill");
    }
    let groups: std::collections::HashSet<_> = db::gateway_groups::list_group_names(&state.db)
        .await
        .unwrap_or_default()
        .into_iter()
        .collect();
    let all_skills_groups: std::collections::HashSet<_> =
        db::skill_grants::roles_for_skill(&state.db, "*")
            .await
            .unwrap_or_default()
            .into_iter()
            .collect();
    let roles: Vec<_> = parsed
        .roles
        .into_iter()
        .filter(|role| groups.contains(role) && !all_skills_groups.contains(role))
        .collect();
    if let Err(err) = db::skill_grants::set_for_skill(&state.db, &parsed.skill, &roles).await {
        return internal(err);
    }
    // Re-seed the resolver overlay exactly like the form path.
    let grants = db::skill_grants::all(&state.db).await.unwrap_or_default();
    state.rbac.set_skill_grant_overlay(grants);
    json_ok(
        StatusCode::OK,
        serde_json::json!({ "skill": parsed.skill, "roles": roles }),
    )
}

// ---------------------------------------------------------------------------
// Admin connectors (MCP catalog)

/// GET /api/v0/admin/connectors — the MCP catalog + recent audit.
pub async fn admin_connectors_list(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = require_admin_json!(state, req);
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
                "icon": c.icon,
                "category": c.category,
                "base_url": c.url,
                "auth_type": c.auth.as_str(),
                "scope": c.scope.as_str(),
                "scopes": c.scopes,
                "enabled": c.enabled,
                "audit": c.audit,
                "use_dcr": c.use_dcr,
                "client_id": c.client_id,
                "has_secret": c.client_secret_ct.is_some(),
                "authorize_url": c.authorize_url,
                "token_url": c.token_url,
                "registration_url": c.registration_url,
                "groups": c.allowed_groups,
                "seeded": c.seeded,
                "needs_setup": c.needs_setup(),
            })
        })
        .collect();
    let groups = db::gateway_groups::list_group_names(&state.db)
        .await
        .unwrap_or_default();
    json_ok(
        StatusCode::OK,
        serde_json::json!({
            "connectors": out,
            "groups": groups,
            "redirect_uri": format!("{}/integrations/callback", state.public_url()),
        }),
    )
}

#[derive(serde::Deserialize)]
pub struct ConnectorInputBody {
    pub key: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub category: String,
    pub base_url: String,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub auth_type: String,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub client_secret: String,
    #[serde(default)]
    pub client_json: String,
    #[serde(default)]
    pub use_dcr: bool,
    #[serde(default)]
    pub authorize_url: String,
    #[serde(default)]
    pub token_url: String,
    #[serde(default)]
    pub registration_url: String,
    #[serde(default)]
    pub groups: Vec<String>,
    #[serde(default)]
    pub audit: bool,
    #[serde(default)]
    pub overwrite: bool,
}

#[derive(Default)]
struct OAuthClientJson {
    client_id: Option<String>,
    client_secret: Option<String>,
    authorize_url: Option<String>,
    token_url: Option<String>,
}

fn oauth_client_json(raw: &str) -> Option<OAuthClientJson> {
    let value = serde_json::from_str::<serde_json::Value>(raw).ok()?;
    let object = value
        .get("web")
        .or_else(|| value.get("installed"))
        .unwrap_or(&value);
    let string = |key: &str| {
        object
            .get(key)
            .and_then(|value| value.as_str())
            .map(str::to_owned)
    };
    let parsed = OAuthClientJson {
        client_id: string("client_id"),
        client_secret: string("client_secret"),
        authorize_url: string("auth_uri"),
        token_url: string("token_uri"),
    };
    parsed.client_id.is_some().then_some(parsed)
}

/// PUT /api/v0/admin/connectors — create/update a catalog entry. The
/// client secret is sealed before storage.
pub async fn admin_connectors_save(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = require_admin_json!(state, req);
    let (parts, body) = req.into_parts();
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
    let client_json = if parsed.client_json.trim().is_empty() {
        OAuthClientJson::default()
    } else {
        let Some(client) = oauth_client_json(parsed.client_json.trim()) else {
            return bad_request("the OAuth client JSON does not contain a client_id");
        };
        client
    };
    let client_id = client_json.client_id.or_else(|| {
        (!parsed.client_id.trim().is_empty()).then(|| parsed.client_id.trim().to_string())
    });
    let secret = client_json
        .client_secret
        .unwrap_or_else(|| parsed.client_secret.trim().to_string());
    // Seal a newly entered secret; blank keeps the stored one.
    let (secret_ct, secret_nonce) = if secret.is_empty() {
        (
            existing.as_ref().and_then(|c| c.client_secret_ct.clone()),
            existing
                .as_ref()
                .and_then(|c| c.client_secret_nonce.clone()),
        )
    } else {
        match state.crypto.seal_str(&secret) {
            Ok(s) => (Some(s.ciphertext), Some(s.nonce)),
            Err(err) => return internal(err),
        }
    };
    let auth = db::mcp_catalog::AuthKind::parse(&parsed.auth_type);
    let scope = db::mcp_catalog::Scope::parse(&parsed.scope);
    if scope == db::mcp_catalog::Scope::Global && auth == db::mcp_catalog::AuthKind::OAuth2 {
        return bad_request("a global connector cannot use per-user OAuth");
    }
    // Same gate as the pool and RAG saves: an unmatchable group name hides the
    // connector from everyone instead of restricting it to someone.
    match db::gateway_groups::unknown_groups(&state.db, &parsed.groups).await {
        Ok(unknown) if !unknown.is_empty() => {
            return bad_request(db::gateway_groups::unknown_groups_message(
                session_core::i18n::Lang::from_request(&parts.headers),
                &unknown,
            ));
        }
        Ok(_) => {}
        Err(err) => return internal(err),
    }
    let input = db::mcp_catalog::ConnectorInput {
        key: parsed.key.clone(),
        name: parsed.title,
        description: (!parsed.description.trim().is_empty()).then(|| parsed.description.clone()),
        icon: (!parsed.icon.trim().is_empty()).then(|| parsed.icon.trim().to_string()),
        category: (!parsed.category.trim().is_empty()).then(|| parsed.category.trim().to_string()),
        url: parsed.base_url.trim().to_string(),
        auth,
        scope,
        audit: parsed.audit,
        use_dcr: auth == db::mcp_catalog::AuthKind::OAuth2 && parsed.use_dcr,
        client_id,
        client_secret_ct: secret_ct,
        client_secret_nonce: secret_nonce,
        authorize_url: client_json.authorize_url.or_else(|| {
            (!parsed.authorize_url.trim().is_empty())
                .then(|| parsed.authorize_url.trim().to_string())
        }),
        token_url: client_json.token_url.or_else(|| {
            (!parsed.token_url.trim().is_empty()).then(|| parsed.token_url.trim().to_string())
        }),
        registration_url: (!parsed.registration_url.trim().is_empty())
            .then(|| parsed.registration_url.trim().to_string()),
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
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, _admin) = require_admin_json!(state, req);
    // Raw URI, not the `Path` extractor: rama lowercases path segments and
    // never percent-decodes them, and a connector key is stored with its case intact.
    let Some(key) = raw_path_segment(&req, 1) else {
        return bad_request("the URL is missing its connector key");
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
    if parsed.enabled
        && let Ok(Some(connector)) = db::mcp_catalog::get(&state.db, &key).await
        && connector.needs_setup()
    {
        return bad_request("this connector needs an OAuth client id before it can be enabled");
    }
    match db::mcp_catalog::set_enabled(&state.db, &key, parsed.enabled).await {
        Ok(_) => json_ok(
            StatusCode::OK,
            serde_json::json!({ "key": key, "enabled": parsed.enabled }),
        ),
        Err(err) => internal(err),
    }
}

/// GET /api/v0/admin/connectors/{key}/audit — newest 200 tool calls for one
/// connector, including actor, outcome, arguments or error, and session id.
pub async fn admin_connector_audit(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = require_admin_json!(state, req);
    let Some(key) = raw_path_segment(&req, 1) else {
        return bad_request("the URL is missing its connector key");
    };
    let connector = db::mcp_catalog::get(&state.db, &key).await.ok().flatten();
    let events = db::mcp_audit::recent_for_connector(&state.db, &key, 200)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|event| {
            serde_json::json!({
                "id": event.id,
                "user_id": event.user_id,
                "user_email": event.user_email,
                "tool_id": event.tool_id,
                "arguments": event.arguments,
                "outcome": event.outcome,
                "error": event.error,
                "session_id": event.session_id,
                "created_at": event.created_at.to_string(),
            })
        })
        .collect::<Vec<_>>();
    json_ok(
        StatusCode::OK,
        serde_json::json!({
            "connector": {
                "key": key,
                "title": connector.as_ref().map(|connector| connector.name.as_str()).unwrap_or(&key),
            },
            "events": events,
        }),
    )
}

/// DELETE /api/v0/admin/connectors/{key} — cascades every user connection.
pub async fn admin_connectors_delete(
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, _admin) = require_admin_json!(state, req);
    // Raw URI, not the `Path` extractor: rama lowercases path segments and
    // never percent-decodes them, and a connector key is stored with its case intact.
    let Some(key) = raw_path_segment(&req, 0) else {
        return bad_request("the URL is missing its connector key");
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
    let (_session, _admin) = require_admin_json!(state, req);
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
    let (_session, user) = require_session_json!(state, req);
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
    // One query for the caller's connections, not one per connector.
    let connections: HashMap<String, db::user_mcp::Connection> =
        db::user_mcp::list_connections(&state.db, &user.id)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|connection| (connection.connector_key.clone(), connection))
            .collect();
    let mut out = Vec::with_capacity(connectors.len());
    for c in &connectors {
        let connection = connections.get(&c.key);
        let connected = connection.is_some();
        let (tools, tool_error) = if c.is_global() || connected {
            match state.mcp.connector_tool_infos(&user.id, c).await {
                Ok(tools) => (
                    Some(
                        tools
                            .into_iter()
                            .map(|tool| {
                                serde_json::json!({
                                    "name": tool.remote_name,
                                    "description": tool.description,
                                    "read_only": tool.read_only,
                                    "mode": tool.mode.as_str(),
                                })
                            })
                            .collect::<Vec<_>>(),
                    ),
                    None,
                ),
                Err(err) => (None, Some(err)),
            }
        } else {
            (None, None)
        };
        out.push(serde_json::json!({
            "key": c.key,
            "title": c.name,
            "description": c.description,
            "icon": c.icon,
            "auth_type": c.auth.as_str(),
            "is_global": c.is_global(),
            "needs_setup": c.needs_setup(),
            "connected": connected,
            "errored": connection.is_some_and(|connection| connection.is_errored()),
            "needs_reauth": connection.is_some_and(|connection| connection.needs_reauth()),
            "tools": tools,
            "tool_error": tool_error,
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
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = require_session_json!(state, req);
    // Raw URI, not the `Path` extractor: rama lowercases path segments and
    // never percent-decodes them, and a connector key is stored with its case intact.
    let Some(key) = raw_path_segment(&req, 1) else {
        return bad_request("the URL is missing its connector key");
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
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = require_session_json!(state, req);
    // Raw URI, not the `Path` extractor: rama lowercases path segments and
    // never percent-decodes them, and a connector key is stored with its case intact.
    let Some(key) = raw_path_segment(&req, 1) else {
        return bad_request("the URL is missing its connector key");
    };
    match db::user_mcp::delete_connection(&state.db, &user.id, &key).await {
        Ok(_) => {
            state.mcp.invalidate(&user.id, &key).await;
            no_content()
        }
        Err(err) => internal(err),
    }
}

#[derive(serde::Deserialize)]
pub struct ToolModeBody {
    pub tool: String,
    pub mode: String,
}

#[derive(serde::Deserialize)]
pub struct ToolsAllBody {
    pub mode: String,
}

async fn integration_for_user(
    state: &RamaState,
    key: &str,
    roles: &[String],
) -> Result<db::mcp_catalog::Connector, Response> {
    let Some(connector) = db::mcp_catalog::get(&state.db, key).await.ok().flatten() else {
        return Err(json_error(
            StatusCode::NOT_FOUND,
            "not_found",
            "no such connector",
        ));
    };
    if !connector.enabled {
        return Err(json_error(
            StatusCode::NOT_FOUND,
            "not_found",
            "no such connector",
        ));
    }
    let role_ids = state.rbac.role_ids_for(roles);
    if !connector.allows(&role_ids, state.rbac.is_admin(&role_ids)) {
        return Err(json_error(
            StatusCode::FORBIDDEN,
            "forbidden",
            "your roles do not grant this connector",
        ));
    }
    Ok(connector)
}

async fn integration_body<T: serde::de::DeserializeOwned>(
    body: rama::http::Body,
) -> Result<T, Response> {
    let bytes = session_core::chrome::read_body_to_bytes(body)
        .await
        .map_err(bad_request)?;
    serde_json::from_slice(&bytes)
        .map_err(|err| bad_request(format!("parsing the request body: {err}")))
}

/// POST /api/v0/integrations/{key}/retry — clear a cached connection failure.
pub async fn integrations_retry_json(
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = require_session_json!(state, req);
    let Some(key) = raw_path_segment(&req, 1) else {
        return bad_request("the URL is missing its connector key");
    };
    if let Err(response) = integration_for_user(&state, &key, &user.roles).await {
        return response;
    }
    state.mcp.invalidate(&user.id, &key).await;
    no_content()
}

/// POST /api/v0/integrations/{key}/tools/mode — set one tool policy.
pub async fn integrations_tool_mode(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, user) = require_session_json!(state, req);
    let Some(key) = raw_path_segment(&req, 2) else {
        return bad_request("the URL is missing its connector key");
    };
    let (_, body) = req.into_parts();
    let parsed: ToolModeBody = match integration_body(body).await {
        Ok(parsed) => parsed,
        Err(response) => return response,
    };
    let Some(mode) = ToolMode::parse(&parsed.mode) else {
        return bad_request("invalid permission mode");
    };
    if let Err(response) = integration_for_user(&state, &key, &user.roles).await {
        return response;
    }
    if parsed.tool.trim().is_empty() {
        return bad_request("a tool name is required");
    }
    match db::user_mcp::set_tool_mode(&state.db, &user.id, &key, &parsed.tool, mode).await {
        Ok(()) => {
            state.mcp.invalidate(&user.id, &key).await;
            no_content()
        }
        Err(err) => internal(err),
    }
}

/// POST /api/v0/integrations/{key}/tools/all — set every exposed tool policy.
pub async fn integrations_tools_all(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, user) = require_session_json!(state, req);
    let Some(key) = raw_path_segment(&req, 2) else {
        return bad_request("the URL is missing its connector key");
    };
    let (_, body) = req.into_parts();
    let parsed: ToolsAllBody = match integration_body(body).await {
        Ok(parsed) => parsed,
        Err(response) => return response,
    };
    let Some(mode) = ToolMode::parse(&parsed.mode) else {
        return bad_request("invalid permission mode");
    };
    let connector = match integration_for_user(&state, &key, &user.roles).await {
        Ok(connector) => connector,
        Err(response) => return response,
    };
    let tools = match state.mcp.connector_tool_infos(&user.id, &connector).await {
        Ok(tools) => tools,
        Err(err) => return internal(err),
    };
    for tool in tools {
        if let Err(err) =
            db::user_mcp::set_tool_mode(&state.db, &user.id, &key, &tool.remote_name, mode).await
        {
            return internal(err);
        }
    }
    state.mcp.invalidate(&user.id, &key).await;
    no_content()
}
