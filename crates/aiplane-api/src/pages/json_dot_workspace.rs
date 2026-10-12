// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use std::sync::Arc;

use aiplane_agents::db::agents::{self, PersonalDotRow};
use aiplane_core::server::db::users::User;
use aiplane_runtime::rama_server::state::RamaState;
use aiplane_runtime::server::tools::sandbox::{WorkspaceClientError, WorkspaceReply};
use rama::http::service::web::extract::State;
use rama::http::{Request, Response, StatusCode, header};
use serde::{Deserialize, Serialize};
use session_core::chrome::{CappedBodyError, read_body_capped};
use session_core::i18n::{Lang, t};
use shared::sandbox::{DotWorkspace, STAGE_TOTAL_MAX_BYTES, WorkspaceOperation, WorkspaceRequest};

// A cursor encodes `{path, after}`; both are bounded at 4096 UTF-8 bytes, so
// the base64 of the JSON must fit the worst case (~11 KiB) or pagination of a
// legitimately-named deep directory breaks.
const MAX_CURSOR_BYTES: usize = 16384;
const MAX_FOLDER_REQUEST_BYTES: usize = 16 * 1024;

#[derive(Serialize, schemars::JsonSchema)]
pub struct DotWorkspaceView {
    pub dot_id: String,
    pub dot_name: String,
    pub available: bool,
    pub busy: Option<bool>,
    pub path: String,
    pub entries: Option<Vec<DotWorkspaceEntry>>,
    pub next_cursor: Option<String>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct DotWorkspaceEntry {
    pub name: String,
    pub kind: DotWorkspaceEntryKind,
    pub size_bytes: Option<u64>,
    pub modified_at: Option<String>,
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DotWorkspaceEntryKind {
    File,
    Directory,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct WorkspaceFolderBody {
    pub path: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceCursor {
    path: String,
    after: String,
}

#[derive(Debug, thiserror::Error)]
#[error("workspace cursor is invalid or does not belong to this directory")]
struct CursorError;

fn valid_after(after: &str) -> bool {
    !after.is_empty()
        && after != "."
        && after != ".."
        && !after.contains('/')
        && !after.contains('\0')
}

fn cursor_for_next(path: &str, after: &str) -> Result<String, CursorError> {
    if !valid_after(after) {
        return Err(CursorError);
    }
    let bytes = serde_json::to_vec(&WorkspaceCursor {
        path: path.into(),
        after: after.into(),
    })
    .map_err(|_| CursorError)?;
    let encoded = shared::b64::encode(&bytes);
    if encoded.len() > MAX_CURSOR_BYTES {
        return Err(CursorError);
    }
    Ok(encoded)
}

fn cursor_for_directory(path: &str, cursor: Option<&str>) -> Result<Option<String>, CursorError> {
    let Some(encoded) = cursor else {
        return Ok(None);
    };
    if encoded.len() > MAX_CURSOR_BYTES {
        return Err(CursorError);
    }
    let bytes = shared::b64::decode(encoded).map_err(|_| CursorError)?;
    if shared::b64::encode(&bytes) != encoded {
        return Err(CursorError);
    }
    let cursor: WorkspaceCursor = serde_json::from_slice(&bytes).map_err(|_| CursorError)?;
    if cursor.path != path || !valid_after(&cursor.after) {
        return Err(CursorError);
    }
    Ok(Some(cursor.after))
}

fn listing_view(
    dot_id: &str,
    dot_name: &str,
    path: &str,
    response: shared::sandbox::WorkspaceListResponse,
) -> Result<DotWorkspaceView, CursorError> {
    let next_cursor = response
        .listing
        .next_cursor
        .as_deref()
        .map(|after| cursor_for_next(path, after))
        .transpose()?;
    Ok(DotWorkspaceView {
        dot_id: dot_id.into(),
        dot_name: dot_name.into(),
        available: true,
        busy: Some(response.busy),
        path: path.into(),
        entries: Some(
            response
                .listing
                .entries
                .into_iter()
                .map(|entry| DotWorkspaceEntry {
                    name: entry.name,
                    kind: match entry.kind {
                        shared::sandbox::WorkspaceEntryKind::File => DotWorkspaceEntryKind::File,
                        shared::sandbox::WorkspaceEntryKind::Directory => {
                            DotWorkspaceEntryKind::Directory
                        }
                    },
                    size_bytes: entry.size_bytes,
                    modified_at: entry.modified_at,
                })
                .collect(),
        ),
        next_cursor,
    })
}

fn unavailable_view(dot_id: &str, dot_name: &str, path: &str) -> DotWorkspaceView {
    DotWorkspaceView {
        dot_id: dot_id.into(),
        dot_name: dot_name.into(),
        available: false,
        busy: None,
        path: path.into(),
        entries: None,
        next_cursor: None,
    }
}

fn workspace_error(error: WorkspaceClientError, lang: Lang) -> Response {
    let (status, code, key) = match error {
        WorkspaceClientError::Unavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            "workspace_unavailable",
            "dot-workspace-error-unavailable",
        ),
        WorkspaceClientError::Conflict => (
            StatusCode::CONFLICT,
            "workspace_conflict",
            "dot-workspace-error-conflict",
        ),
        WorkspaceClientError::NotFound => (
            StatusCode::NOT_FOUND,
            "workspace_not_found",
            "dot-workspace-error-not-found",
        ),
        WorkspaceClientError::TooLarge => (
            StatusCode::PAYLOAD_TOO_LARGE,
            "workspace_too_large",
            "dot-workspace-error-too-large",
        ),
        WorkspaceClientError::InvalidRequest => (
            StatusCode::BAD_REQUEST,
            "workspace_invalid",
            "dot-workspace-error-invalid",
        ),
        WorkspaceClientError::Failed => (
            StatusCode::BAD_GATEWAY,
            "workspace_failed",
            "dot-workspace-error-failed",
        ),
    };
    super::json_error(status, code, &t(lang, key))
}

fn dot_title(dot: &PersonalDotRow) -> String {
    if dot.agent.principal.display.is_empty() {
        dot.name.clone()
    } else {
        dot.agent.principal.display.clone()
    }
}

async fn owned_dot(
    state: &RamaState,
    owner: &str,
    id: &str,
    lang: Lang,
) -> Result<PersonalDotRow, Response> {
    match agents::get_personal_dot(&state.db, owner, id).await {
        Ok(Some(dot)) => Ok(dot),
        Ok(None) => Err(super::not_found(t(lang, "dot-error-not-found"))),
        Err(error) => {
            tracing::warn!(error = %error, "reading personal Dot workspace ownership failed");
            Err(super::json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "workspace_failed",
                &t(lang, "dot-workspace-error-failed"),
            ))
        }
    }
}

async fn authority(
    state: &RamaState,
    req: &Request,
    from_end: usize,
) -> Result<(User, PersonalDotRow), Response> {
    let (_, user) = super::require_session_json(state, req).await?;
    let lang = Lang::from_request(req.headers());
    let id = super::raw_path_segment(req, from_end)
        .ok_or_else(|| workspace_error(WorkspaceClientError::InvalidRequest, lang))?;
    let dot = owned_dot(state, &user.id, &id, lang).await?;
    Ok((user, dot))
}

async fn operate(
    state: &RamaState,
    owner: &str,
    id: &str,
    operation: WorkspaceOperation,
    lang: Lang,
) -> Result<(PersonalDotRow, Result<WorkspaceReply, WorkspaceClientError>), Response> {
    let response = match state.sandbox_client() {
        Some(client) => {
            client
                .workspace_operation(WorkspaceRequest {
                    workspace: DotWorkspace {
                        owner_user_id: owner.into(),
                        dot_id: id.into(),
                    },
                    operation,
                })
                .await
        }
        None => Err(WorkspaceClientError::Unavailable),
    };
    // Re-validate after the storage request: a Dot deleted mid-call must not
    // surface the runner's result (callers already validated before the call).
    let dot = owned_dot(state, owner, id, lang).await?;
    Ok((dot, response))
}

fn required_path(req: &Request, lang: Lang) -> Result<String, Response> {
    super::query_map(req)
        .remove("path")
        .filter(|path| !path.is_empty())
        .ok_or_else(|| workspace_error(WorkspaceClientError::InvalidRequest, lang))
}

async fn capped_body(req: Request, cap: usize, lang: Lang) -> Result<rama::bytes::Bytes, Response> {
    let declared = req
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    if declared.is_some_and(|len| len > cap as u64) {
        return Err(workspace_error(WorkspaceClientError::TooLarge, lang));
    }
    read_body_capped(req.into_body(), cap)
        .await
        .map_err(|error| {
            workspace_error(
                match error {
                    CappedBodyError::TooLarge { .. } => WorkspaceClientError::TooLarge,
                    CappedBodyError::Read(_) => WorkspaceClientError::Failed,
                },
                lang,
            )
        })
}

pub async fn list(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let lang = Lang::from_request(req.headers());
    let (user, dot) = or_return!(authority(&state, &req, 1).await);
    let params = super::query_map(&req);
    let path = params.get("path").cloned().unwrap_or_default();
    let after = or_return!(
        cursor_for_directory(&path, params.get("cursor").map(String::as_str))
            .map_err(|_| workspace_error(WorkspaceClientError::InvalidRequest, lang))
    );
    let (dot, response) = or_return!(
        operate(
            &state,
            &user.id,
            &dot.agent.principal.id,
            WorkspaceOperation::List {
                path: path.clone(),
                after
            },
            lang
        )
        .await
    );
    let title = dot_title(&dot);
    match response {
        Ok(WorkspaceReply::Listing(response)) => {
            match listing_view(&dot.agent.principal.id, &title, &path, response) {
                Ok(view) => super::json_ok(StatusCode::OK, view),
                Err(_) => workspace_error(WorkspaceClientError::Failed, lang),
            }
        }
        Err(WorkspaceClientError::Unavailable) => super::json_ok(
            StatusCode::OK,
            unavailable_view(&dot.agent.principal.id, &title, &path),
        ),
        Err(error) => workspace_error(error, lang),
        _ => workspace_error(WorkspaceClientError::Failed, lang),
    }
}

pub async fn file_get(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let lang = Lang::from_request(req.headers());
    let (user, dot) = or_return!(authority(&state, &req, 2).await);
    let path = or_return!(required_path(&req, lang));
    let (_, response) = or_return!(
        operate(
            &state,
            &user.id,
            &dot.agent.principal.id,
            WorkspaceOperation::Read { path: path.clone() },
            lang
        )
        .await
    );
    match response {
        Ok(WorkspaceReply::File(bytes)) => download(&path, bytes, lang),
        Err(error) => workspace_error(error, lang),
        _ => workspace_error(WorkspaceClientError::Failed, lang),
    }
}

fn download(path: &str, bytes: Vec<u8>, lang: Lang) -> Response {
    let filename = path.rsplit('/').next().unwrap_or(path);
    let fallback: String = filename
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let encoded = match serde_urlencoded::to_string([("filename", filename)]) {
        Ok(value) => value
            .strip_prefix("filename=")
            .unwrap_or(&value)
            .replace('+', "%20"),
        Err(_) => return workspace_error(WorkspaceClientError::Failed, lang),
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_LENGTH, bytes.len())
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{fallback}\"; filename*=UTF-8''{encoded}"),
        )
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .body(bytes.into())
        .unwrap_or_else(|_| workspace_error(WorkspaceClientError::Failed, lang))
}

pub async fn file_put(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let lang = Lang::from_request(req.headers());
    let (user, dot) = or_return!(authority(&state, &req, 2).await);
    let path = or_return!(required_path(&req, lang));
    let bytes = or_return!(capped_body(req, STAGE_TOTAL_MAX_BYTES, lang).await);
    changed(
        &state,
        &user.id,
        &dot.agent.principal.id,
        WorkspaceOperation::Write {
            path,
            content_b64: shared::b64::encode(&bytes),
        },
        lang,
    )
    .await
}

pub async fn folder_create(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let lang = Lang::from_request(req.headers());
    let (user, dot) = or_return!(authority(&state, &req, 2).await);
    let bytes = or_return!(capped_body(req, MAX_FOLDER_REQUEST_BYTES, lang).await);
    let body: WorkspaceFolderBody = or_return!(
        serde_json::from_slice(&bytes)
            .map_err(|_| workspace_error(WorkspaceClientError::InvalidRequest, lang))
    );
    changed(
        &state,
        &user.id,
        &dot.agent.principal.id,
        WorkspaceOperation::CreateFolder { path: body.path },
        lang,
    )
    .await
}

pub async fn entry_delete(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let lang = Lang::from_request(req.headers());
    let (user, dot) = or_return!(authority(&state, &req, 2).await);
    let path = or_return!(required_path(&req, lang));
    changed(
        &state,
        &user.id,
        &dot.agent.principal.id,
        WorkspaceOperation::Remove { path },
        lang,
    )
    .await
}

async fn changed(
    state: &RamaState,
    owner: &str,
    id: &str,
    operation: WorkspaceOperation,
    lang: Lang,
) -> Response {
    let (_, response) = or_return!(operate(state, owner, id, operation, lang).await);
    match response {
        Ok(WorkspaceReply::Changed) => super::no_content(),
        Err(error) => workspace_error(error, lang),
        _ => workspace_error(WorkspaceClientError::Failed, lang),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_cursor_round_trip_is_bound_to_directory_and_rejects_malformed_data() {
        let encoded = cursor_for_next("project", "UTF8-ß.txt").unwrap();
        assert_eq!(
            cursor_for_directory("project", Some(&encoded)).unwrap(),
            Some("UTF8-ß.txt".into())
        );
        assert_eq!(cursor_for_directory("", None).unwrap(), None);
        assert!(cursor_for_directory("other", Some(&encoded)).is_err());
        for raw in [
            "not-json",
            "{\"path\":\"project\",\"after\":null}",
            "{\"path\":\"project\",\"after\":\"ok\",\"owner\":\"foreign\"}",
        ] {
            assert!(
                cursor_for_directory("project", Some(&shared::b64::encode(raw.as_bytes())))
                    .is_err()
            );
        }
        for after in ["", ".", "..", "nested/file", "nul\0name"] {
            let raw = serde_json::json!({"path":"project", "after":after});
            assert!(
                cursor_for_directory(
                    "project",
                    Some(&shared::b64::encode(&serde_json::to_vec(&raw).unwrap()))
                )
                .is_err()
            );
        }
        for malformed in ["%%%".to_string(), format!(" {encoded}"), "A".repeat(8193)] {
            assert!(cursor_for_directory("project", Some(&malformed)).is_err());
        }
    }

    #[test]
    fn workspace_download_encodes_unicode_and_header_delimiters_without_panicking() {
        let response = download("project/UTF8-ß \"\r\n.txt", b"data".to_vec(), Lang::En);
        assert_eq!(response.status(), StatusCode::OK);
        let disposition = response
            .headers()
            .get(header::CONTENT_DISPOSITION)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(disposition.starts_with("attachment; filename="));
        assert!(disposition.contains("filename*=UTF-8''UTF8-%C3%9F%20%22%0D%0A.txt"));
        assert!(!disposition.contains('\r'));
        assert!(!disposition.contains('\n'));
        assert_eq!(
            response.headers()[header::X_CONTENT_TYPE_OPTIONS],
            "nosniff"
        );
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    }

    #[test]
    fn workspace_listing_preserves_actual_nullable_metadata_and_encodes_cursor() {
        let view = listing_view(
            "actual-dot",
            "Actual Dot",
            "project",
            shared::sandbox::WorkspaceListResponse {
                busy: true,
                listing: shared::sandbox::WorkspaceListing {
                    entries: vec![shared::sandbox::WorkspaceEntry {
                        name: "actual-name".into(),
                        kind: shared::sandbox::WorkspaceEntryKind::File,
                        size_bytes: None,
                        modified_at: None,
                    }],
                    next_cursor: Some("actual-name".into()),
                },
            },
        )
        .unwrap();
        let value = serde_json::to_value(view).unwrap();
        assert_eq!(value["dot_id"], "actual-dot");
        assert_eq!(value["dot_name"], "Actual Dot");
        assert_eq!(value["available"], true);
        assert_eq!(value["busy"], true);
        assert_eq!(value["entries"][0]["name"], "actual-name");
        assert_eq!(value["entries"][0]["kind"], "file");
        assert!(value["entries"][0]["size_bytes"].is_null());
        assert!(value["entries"][0]["modified_at"].is_null());
        assert_eq!(
            cursor_for_directory("project", value["next_cursor"].as_str()).unwrap(),
            Some("actual-name".into())
        );
        let unavailable =
            serde_json::to_value(unavailable_view("actual-dot", "Actual Dot", "project")).unwrap();
        assert_eq!(unavailable["available"], false);
        for field in ["busy", "entries", "next_cursor"] {
            assert!(unavailable[field].is_null());
        }
    }
}
