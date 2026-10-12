// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! HTTP surface of the runner, built on the same rama stack as the
//! gateway. Three routes:
//!   - `GET    /healthz`        — liveness/readiness for the Quadlet + gateway.
//!   - `POST   /run`            — execute one [`RunRequest`], return a [`RunResponse`].
//!     The body is read up to [`MAX_RUN_REQUEST_BYTES`] and refused with
//!     `413` past it, so not even the gateway can make the runner buffer
//!     without bound.
//!   - `DELETE /container/{id}` — release a kept-alive (leased) container.
//!
//! There is no auth here by design: the runner must be reachable **only**
//! from the gateway over an internal network (and, when remote, fronted by
//! mTLS). It executes arbitrary code — exposing it publicly is RCE-as-a-
//! service. The deployment (Quadlet network + firewall) is the boundary.

use std::sync::Arc;

use rama::futures::StreamExt;
use rama::http::layer::error_handling::ErrorHandlerLayer;
use rama::http::server::HttpServer;
use rama::http::service::web::Router;
use rama::http::service::web::extract::{Path, State};
use rama::http::service::web::response::{IntoResponse, Json};
use rama::http::{Request, Response, StatusCode, header};
use rama::layer::{ArcLayer, Layer};
use rama::net::address::SocketAddress;
use shared::sandbox::{
    MAX_RUN_REQUEST_BYTES, RunError, RunRequest, RunnerHealth, STAGE_TOTAL_MAX_BYTES,
    WorkspaceOperation, WorkspaceRequest,
};

use crate::pool::{Pool, RunnerError};

#[cfg(test)]
mod workspace_tests;

/// Shared handler state. The pool already carries its own `Arc<Config>`,
/// so the handlers only need the pool.
pub struct RunnerState {
    pub pool: Arc<Pool>,
}

pub fn router(state: Arc<RunnerState>) -> Router<Arc<RunnerState>> {
    Router::new_with_state(state)
        // `/healthz` doubles as the capability probe: the gateway reads
        // `egress` at boot and stops advertising the network-dependent tools
        // when this runner has no egress wired, instead of offering the model
        // a tool whose every call must fail. See `RunnerHealth`.
        .with_get("/healthz", healthz)
        .with_post("/run", run)
        .with_post("/workspace", workspace)
        .with_delete("/container/{id}", release_container)
}

/// GET /healthz — liveness plus the runner's capabilities.
async fn healthz(State(state): State<Arc<RunnerState>>) -> Json<RunnerHealth> {
    Json(RunnerHealth {
        status: "ok".into(),
        egress: Some(state.pool.config().egress_available()),
        workspace: Some(state.pool.workspace_available()),
    })
}

async fn release_container(
    State(state): State<Arc<RunnerState>>,
    Path(id): Path<String>,
) -> Response {
    match state.pool.release_container(&id).await {
        Ok(()) => (StatusCode::NO_CONTENT, ()).into_response(),
        Err(RunnerError::Busy) => err(
            StatusCode::CONFLICT,
            "The container is busy; retry release after its active operation finishes.",
        ),
        Err(error) => {
            tracing::warn!(container = %id, error = %error, "container release not confirmed");
            err(
                StatusCode::SERVICE_UNAVAILABLE,
                "Container removal could not be confirmed; retry release. The runner retains it for cleanup.",
            )
        }
    }
}

/// POST /run — decode the request, execute it, return the result.
async fn run(State(state): State<Arc<RunnerState>>, req: Request) -> Response {
    let bytes = match read_capped(req, MAX_RUN_REQUEST_BYTES).await {
        Ok(b) => b,
        Err(refusal) => return refusal,
    };
    let request: RunRequest = match serde_json::from_slice(&bytes) {
        Ok(r) => r,
        Err(e) => {
            return err(
                StatusCode::BAD_REQUEST,
                &format!("body is not a RunRequest: {e}"),
            );
        }
    };

    match state.pool.run(&request).await {
        Ok(resp) => json_ok(&resp),
        Err(error) => runner_failure(error),
    }
}

async fn workspace(State(state): State<Arc<RunnerState>>, req: Request) -> Response {
    let bytes = match read_capped(req, MAX_RUN_REQUEST_BYTES).await {
        Ok(bytes) => bytes,
        Err(refusal) => return refusal,
    };
    let request: WorkspaceRequest = match serde_json::from_slice(&bytes) {
        Ok(request) => request,
        Err(_) => {
            return err(
                StatusCode::BAD_REQUEST,
                "The workspace request is invalid. Supply the trusted Dot identity and a supported file operation.",
            );
        }
    };
    if matches!(request.operation, WorkspaceOperation::Retire) {
        return match state.pool.retire_workspace(&request.workspace).await {
            Ok(true) => (StatusCode::NO_CONTENT, ()).into_response(),
            Ok(false) | Err(RunnerError::Busy) => err(
                StatusCode::CONFLICT,
                "Dot storage retirement is pending. Retry after container teardown or the next bounded cleanup pass.",
            ),
            Err(error) => runner_failure(error),
        };
    }
    let result = tokio::task::spawn_blocking(move || {
        let mutating = matches!(
            &request.operation,
            WorkspaceOperation::Write { .. }
                | WorkspaceOperation::CreateFolder { .. }
                | WorkspaceOperation::Remove { .. }
        );
        state
            .pool
            .workspace_operation(&request.workspace, mutating, |files, busy| {
                match request.operation {
                    WorkspaceOperation::List { path, after } => {
                        Ok(json_ok(&shared::sandbox::WorkspaceListResponse {
                            listing: files.list(&path, after.as_deref())?,
                            busy,
                        }))
                    }
                    WorkspaceOperation::Read { path } => Ok((
                        StatusCode::OK,
                        [(header::CONTENT_TYPE, "application/octet-stream")],
                        files.read(&path)?,
                    )
                        .into_response()),
                    WorkspaceOperation::Write { path, content_b64 } => {
                        if content_b64.len() > STAGE_TOTAL_MAX_BYTES.div_ceil(3) * 4 {
                            return Err(crate::workspace::WorkspaceError::FileTooLarge);
                        }
                        let bytes = shared::b64::decode(&content_b64).map_err(|_| {
                            crate::workspace::WorkspaceError::InvalidPath("invalid file base64")
                        })?;
                        files.write(&path, &bytes)?;
                        Ok((StatusCode::NO_CONTENT, ()).into_response())
                    }
                    WorkspaceOperation::CreateFolder { path } => {
                        files.mkdir(&path)?;
                        Ok((StatusCode::NO_CONTENT, ()).into_response())
                    }
                    WorkspaceOperation::Remove { path } => {
                        files.remove(&path)?;
                        Ok((StatusCode::NO_CONTENT, ()).into_response())
                    }
                    // Retire is routed through `retire_workspace` above.
                    WorkspaceOperation::Retire => {
                        Err(crate::workspace::WorkspaceError::InvalidPath(
                            "retirement requires the checked lifecycle operation",
                        ))
                    }
                }
            })
    })
    .await;
    match result {
        Ok(Ok(response)) => response,
        Ok(Err(RunnerError::Busy)) => err(
            StatusCode::CONFLICT,
            "The Dot computer is in use. Finish or stop its current turn and wait for confirmed container removal before changing files.",
        ),
        Ok(Err(error)) => runner_failure(error),
        Err(_) => err(
            StatusCode::SERVICE_UNAVAILABLE,
            "The workspace operation could not finish. Retry after checking runner storage.",
        ),
    }
}

fn runner_failure(error: RunnerError) -> Response {
    match error {
        RunnerError::Busy => err(StatusCode::SERVICE_UNAVAILABLE, "sandbox at capacity"),
        RunnerError::NetworkUnavailable => err(
            StatusCode::BAD_REQUEST,
            "network egress requested but not configured on this runner. Note: if you \
             wanted the network to install packages, don't — the sandbox image is fixed \
             and single-use; retry with network=false using only preinstalled tools",
        ),
        RunnerError::WorkspaceUnavailable => err(
            StatusCode::SERVICE_UNAVAILABLE,
            "Persistent Dot storage is unavailable. Configure and verify the runner's persistent storage.",
        ),
        RunnerError::WorkspaceMismatch => err(
            StatusCode::CONFLICT,
            "The container does not belong to this Dot, turn or purpose. Start a new authorized container.",
        ),
        RunnerError::Workspace(error) => {
            let status = match &error {
                crate::workspace::WorkspaceError::FileTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
                crate::workspace::WorkspaceError::InvalidPath(_) => StatusCode::BAD_REQUEST,
                crate::workspace::WorkspaceError::OwnerMismatch
                | crate::workspace::WorkspaceError::UnsafeEntry => StatusCode::CONFLICT,
                crate::workspace::WorkspaceError::Io(error)
                    if error.kind() == std::io::ErrorKind::NotFound =>
                {
                    StatusCode::NOT_FOUND
                }
                crate::workspace::WorkspaceError::Io(error)
                    if error.raw_os_error().is_some_and(|code| {
                        [libc::EEXIST, libc::ENOTEMPTY, libc::ENOTDIR, libc::ELOOP].contains(&code)
                    }) =>
                {
                    StatusCode::CONFLICT
                }
                _ => StatusCode::SERVICE_UNAVAILABLE,
            };
            err(status, &error.to_string())
        }
        RunnerError::Backend(e) => {
            tracing::warn!(error = %e, "sandbox backend failed");
            err(StatusCode::BAD_GATEWAY, &format!("sandbox backend: {e}"))
        }
    }
}

/// The request body, or the refusal to answer with: `413` for a declared
/// length over `max` before anything is read, and the moment the running
/// total passes it otherwise. The runner sits outside the crate stack, so it
/// cannot use `session_core::chrome::read_body_capped`.
async fn read_capped(req: Request, max: usize) -> Result<Vec<u8>, Response> {
    let too_large = || {
        err(
            StatusCode::PAYLOAD_TOO_LARGE,
            &format!(
                "the request body is larger than {max} bytes, the runner's limit; stage fewer \
                 or smaller files"
            ),
        )
    };
    let declared = req
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    if declared.is_some_and(|len| len > max as u64) {
        return Err(too_large());
    }
    let mut body = Vec::new();
    let mut chunks = req.into_body().into_data_stream();
    while let Some(chunk) = chunks.next().await {
        let chunk = chunk.map_err(|e| {
            err(
                StatusCode::BAD_REQUEST,
                &format!("reading request body: {e}"),
            )
        })?;
        if body.len() + chunk.len() > max {
            return Err(too_large());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn json_ok<T: serde::Serialize>(value: &T) -> Response {
    match serde_json::to_string(value) {
        Ok(s) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            s,
        )
            .into_response(),
        Err(e) => err(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("serialising response: {e}"),
        ),
    }
}

/// Error responses use the shared [`RunError`] envelope so the gateway
/// gets one predictable shape on every non-2xx.
fn err(status: StatusCode, message: &str) -> Response {
    let body = serde_json::to_string(&RunError {
        error: message.to_string(),
    })
    .unwrap_or_else(|_| "{\"error\":\"serialisation failed\"}".to_string());
    (status, [(header::CONTENT_TYPE, "application/json")], body).into_response()
}

/// Build the servable service (router + the layers that make rama's
/// `Router` clone-able and infallible), mirroring the gateway's `service`.
fn service(
    state: Arc<RunnerState>,
) -> impl rama::Service<Request, Output = Response, Error = std::convert::Infallible> + Clone {
    let router = router(state);
    (ArcLayer::new(), ErrorHandlerLayer::default()).into_layer(router)
}

pub async fn serve(state: Arc<RunnerState>, addr: SocketAddress) -> anyhow::Result<()> {
    HttpServer::default()
        .listen(addr, service(state))
        .await
        .map_err(|e| anyhow::anyhow!("rama listen: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::fake::FakeBackend;
    use crate::config::Config;
    use rama::Service;
    use rama::http::{Body, Method};

    fn state() -> Arc<RunnerState> {
        let pool = Pool::new(
            Arc::new(FakeBackend::default()),
            Arc::new(Config::for_test()),
        );
        Arc::new(RunnerState { pool })
    }

    #[tokio::test]
    async fn workspace_operations_report_unavailable_without_verified_storage() {
        let app = service(state());
        for operation in [
            serde_json::json!({"kind":"list","path":"","after":null}),
            serde_json::json!({"kind":"read","path":"file"}),
            serde_json::json!({"kind":"write","path":"file","content_b64":"YQ=="}),
            serde_json::json!({"kind":"create_folder","path":"folder"}),
            serde_json::json!({"kind":"remove","path":"file"}),
        ] {
            let response = app.serve(Request::builder()
                .method(Method::POST).uri("/workspace")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&serde_json::json!({
                    "workspace":{"owner_user_id":"person","dot_id":"12345678-1234-4234-8234-123456789abc"},
                    "operation":operation
                })).unwrap())).unwrap()).await.unwrap();
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        }
    }

    #[tokio::test]
    async fn checked_teardown_release_route_reports_failure_until_retry_confirms_absence() {
        let backend = Arc::new(FakeBackend::new());
        let pool = Pool::new(backend.clone(), Arc::new(Config::for_test()));
        let mut request = crate::backend::fake::req();
        request.keep_alive = true;
        let id = pool.run(&request).await.unwrap().container_id.unwrap();
        let app = service(Arc::new(RunnerState { pool }));
        backend
            .fail_destroy
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let response = app
            .serve(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!("/container/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(backend.live_count(), 1);
        backend
            .fail_destroy
            .store(false, std::sync::atomic::Ordering::SeqCst);
        let response = app
            .serve(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!("/container/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert_eq!(backend.live_count(), 0);
    }

    #[tokio::test]
    async fn a_declared_length_over_the_limit_is_refused_with_413() {
        let req = Request::builder()
            .method(Method::POST)
            .uri("/run")
            .header(
                header::CONTENT_LENGTH,
                (shared::sandbox::MAX_RUN_REQUEST_BYTES + 1).to_string(),
            )
            .body(Body::from("{}"))
            .unwrap();
        let resp = service(state()).serve(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    /// A chunked body with no length: eight 1 KiB chunks against a 4 KiB
    /// cap. Finite, so a lost cap fails the assertion instead of buffering.
    fn chunked(chunks: usize) -> Request {
        let stream = rama::futures::stream::iter(
            (0..chunks).map(|_| Ok::<_, std::io::Error>(vec![b' '; 1024])),
        );
        Request::builder()
            .method(Method::POST)
            .uri("/run")
            .body(Body::from_stream(stream))
            .unwrap()
    }

    #[tokio::test]
    async fn a_body_without_a_length_is_cut_off_at_the_limit() {
        let refusal = read_capped(chunked(8), 4096).await.unwrap_err();
        assert_eq!(refusal.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn a_body_within_the_limit_is_read_whole() {
        assert_eq!(read_capped(chunked(4), 4096).await.unwrap().len(), 4096);
    }
}
