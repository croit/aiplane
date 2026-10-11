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
use shared::sandbox::{MAX_RUN_REQUEST_BYTES, RunError, RunRequest, RunnerHealth};

use crate::pool::{Pool, RunnerError};

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
        .with_delete("/container/{id}", release_container)
}

/// GET /healthz — liveness plus the runner's capabilities.
async fn healthz(State(state): State<Arc<RunnerState>>) -> Json<RunnerHealth> {
    Json(RunnerHealth {
        status: "ok".into(),
        egress: Some(state.pool.config().egress_available()),
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
        Err(RunnerError::Busy) => err(StatusCode::SERVICE_UNAVAILABLE, "sandbox at capacity"),
        Err(RunnerError::NetworkUnavailable) => err(
            StatusCode::BAD_REQUEST,
            "network egress requested but not configured on this runner. Note: if you \
             wanted the network to install packages, don't — the sandbox image is fixed \
             and single-use; retry with network=false using only preinstalled tools",
        ),
        Err(RunnerError::Backend(e)) => {
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
