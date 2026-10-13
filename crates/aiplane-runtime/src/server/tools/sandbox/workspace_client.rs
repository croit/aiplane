// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use super::SandboxClient;
use aiplane_core::server::capped_read::{self, CappedReadError};
use shared::sandbox::{
    BrowserControlRequest, BrowserControlResponse, WorkspaceListResponse, WorkspaceOperation,
    WorkspaceRequest,
};
use std::sync::atomic::Ordering;

#[derive(Debug)]
pub enum WorkspaceReply {
    Listing(WorkspaceListResponse),
    File(Vec<u8>),
    Changed,
}

#[derive(Debug, thiserror::Error)]
pub enum WorkspaceClientError {
    #[error("Persistent Dot storage is unavailable. Check runner storage and retry.")]
    Unavailable,
    #[error(
        "The workspace operation conflicts with current storage or execution. Reload and retry when idle."
    )]
    Conflict,
    #[error("The workspace entry was not found. Reload the directory and try again.")]
    NotFound,
    #[error(
        "The workspace request or response exceeds its byte limit. Use a smaller file or directory."
    )]
    TooLarge,
    #[error("The workspace request is invalid. Check its path and operation before retrying.")]
    InvalidRequest,
    #[error("The workspace operation failed. Check runner availability and retry.")]
    Failed,
}

fn read_error(error: CappedReadError) -> WorkspaceClientError {
    match error {
        CappedReadError::TooLarge { .. } => WorkspaceClientError::TooLarge,
        _ => WorkspaceClientError::Failed,
    }
}

impl SandboxClient {
    pub async fn browser_control(
        &self,
        request: BrowserControlRequest,
    ) -> Result<BrowserControlResponse, WorkspaceClientError> {
        self.probe_capabilities().await;
        if !self.workspace.load(Ordering::Acquire) {
            return Err(WorkspaceClientError::Unavailable);
        }
        let response = self
            .http
            .post(format!(
                "{}/browser/control",
                self.cfg.runner_url.trim_end_matches('/')
            ))
            .json(&request)
            .send()
            .await
            .map_err(|_| WorkspaceClientError::Unavailable)?;
        let status = response.status();
        if !status.is_success() {
            capped_read::read_capped(response, 16 * 1024)
                .await
                .map_err(read_error)?;
            return Err(match status.as_u16() {
                400 => WorkspaceClientError::InvalidRequest,
                409 | 410 => WorkspaceClientError::Conflict,
                413 => WorkspaceClientError::TooLarge,
                503 | 502 => WorkspaceClientError::Unavailable,
                _ => WorkspaceClientError::Failed,
            });
        }
        capped_read::read_capped_json(response, 3 * 1024 * 1024)
            .await
            .map_err(read_error)
    }

    pub async fn workspace_operation(
        &self,
        request: WorkspaceRequest,
    ) -> Result<WorkspaceReply, WorkspaceClientError> {
        self.probe_capabilities().await;
        if !self.workspace.load(Ordering::Acquire) {
            return Err(WorkspaceClientError::Unavailable);
        }
        let response = self
            .http
            .post(format!(
                "{}/workspace",
                self.cfg.runner_url.trim_end_matches('/')
            ))
            .json(&request)
            .send()
            .await
            .map_err(|_| WorkspaceClientError::Unavailable)?;
        let status = response.status();
        if !status.is_success() {
            capped_read::read_capped(response, 16 * 1024)
                .await
                .map_err(read_error)?;
            return Err(match status.as_u16() {
                400 => WorkspaceClientError::InvalidRequest,
                404 => WorkspaceClientError::NotFound,
                409 => WorkspaceClientError::Conflict,
                413 => WorkspaceClientError::TooLarge,
                503 => WorkspaceClientError::Unavailable,
                _ => WorkspaceClientError::Failed,
            });
        }
        match request.operation {
            WorkspaceOperation::List { .. } if status == reqwest::StatusCode::OK => {
                capped_read::read_capped_json(response, 1024 * 1024)
                    .await
                    .map(WorkspaceReply::Listing)
                    .map_err(read_error)
            }
            WorkspaceOperation::Read { .. } if status == reqwest::StatusCode::OK => {
                capped_read::read_capped(response, shared::sandbox::STAGE_TOTAL_MAX_BYTES as u64)
                    .await
                    .map(WorkspaceReply::File)
                    .map_err(read_error)
            }
            WorkspaceOperation::Write { .. }
            | WorkspaceOperation::CreateFolder { .. }
            | WorkspaceOperation::Remove { .. }
            | WorkspaceOperation::Retire
                if status == reqwest::StatusCode::NO_CONTENT =>
            {
                Ok(WorkspaceReply::Changed)
            }
            _ => Err(WorkspaceClientError::Failed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::tools::sandbox::SandboxClient;
    use aiplane_core::server::config::SandboxConfig;
    use shared::sandbox::{DotWorkspace, WorkspaceOperation, WorkspaceRequest};
    use std::sync::Arc;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client(server: &MockServer) -> Arc<SandboxClient> {
        SandboxClient::new(
            Arc::new(SandboxConfig {
                enabled: true,
                runner_url: server.uri(),
                timeout_secs: 2,
                max_artifact_bytes: 1024,
            }),
            crate::server::state::RuntimeSettings::new_handle("https://gateway.example"),
        )
    }

    fn request(operation: WorkspaceOperation) -> WorkspaceRequest {
        WorkspaceRequest {
            workspace: DotWorkspace {
                owner_user_id: "owner".into(),
                dot_id: "00000000-0000-4000-8000-000000000001".into(),
            },
            operation,
        }
    }

    fn list() -> WorkspaceRequest {
        request(WorkspaceOperation::List {
            path: "".into(),
            after: None,
        })
    }

    async fn healthy(server: &MockServer) {
        Mock::given(method("GET"))
            .and(path("/healthz"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status":"ok", "workspace":true, "egress":false
            })))
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn workspace_client_requires_successful_explicit_healthy_storage_proof() {
        for (status, health) in [
            (500, serde_json::json!({"status":"ok","workspace":true})),
            (200, serde_json::json!({"status":"ok","workspace":false})),
            (200, serde_json::json!({"status":"ok"})),
            (
                200,
                serde_json::json!({"status":"starting","workspace":true}),
            ),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/healthz"))
                .respond_with(ResponseTemplate::new(status).set_body_json(health))
                .mount(&server)
                .await;
            Mock::given(method("POST"))
                .and(path("/workspace"))
                .respond_with(ResponseTemplate::new(200).set_body_json(
                    serde_json::json!({"entries":[],"next_cursor":null,"busy":false}),
                ))
                .mount(&server)
                .await;
            assert!(matches!(
                client(&server).workspace_operation(list()).await,
                Err(WorkspaceClientError::Unavailable)
            ));
        }
    }

    #[tokio::test]
    async fn workspace_client_refreshes_storage_proof_for_each_operation() {
        let server = MockServer::start().await;
        healthy(&server).await;
        Mock::given(method("POST"))
            .and(path("/workspace"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    serde_json::json!({"entries":[],"next_cursor":null,"busy":false}),
                ),
            )
            .mount(&server)
            .await;
        let client = client(&server);
        assert!(matches!(
            client.workspace_operation(list()).await.unwrap(),
            WorkspaceReply::Listing(_)
        ));
        server.reset().await;
        assert!(matches!(
            client.workspace_operation(list()).await,
            Err(WorkspaceClientError::Unavailable)
        ));
    }

    #[tokio::test]
    async fn workspace_client_preserves_actual_listing_and_raw_file_bytes() {
        let server = MockServer::start().await;
        healthy(&server).await;
        let listing = serde_json::json!({"entries":[{"name":"notes.txt","kind":"file","size_bytes":3,"modified_at":null}],"next_cursor":"notes.txt","busy":true});
        Mock::given(method("POST"))
            .and(path("/workspace"))
            .respond_with(ResponseTemplate::new(200).set_body_json(listing.clone()))
            .mount(&server)
            .await;
        let WorkspaceReply::Listing(actual) =
            client(&server).workspace_operation(list()).await.unwrap()
        else {
            panic!("A listing request must return the actual listing");
        };
        assert_eq!(serde_json::to_value(actual).unwrap(), listing);
        server.reset().await;
        healthy(&server).await;
        let content = "A UTF-8 boundary: ß".as_bytes();
        Mock::given(method("POST"))
            .and(path("/workspace"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(content))
            .mount(&server)
            .await;
        let WorkspaceReply::File(actual) = client(&server)
            .workspace_operation(request(WorkspaceOperation::Read {
                path: "notes.txt".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("A read request must return file bytes");
        };
        assert_eq!(actual, content);
    }

    #[tokio::test]
    async fn workspace_client_requires_no_content_for_every_mutation() {
        for operation in [
            WorkspaceOperation::Write {
                path: "notes.txt".into(),
                content_b64: "YQ==".into(),
            },
            WorkspaceOperation::CreateFolder {
                path: "project".into(),
            },
            WorkspaceOperation::Remove {
                path: "notes.txt".into(),
            },
            WorkspaceOperation::Retire,
        ] {
            let server = MockServer::start().await;
            healthy(&server).await;
            Mock::given(method("POST"))
                .and(path("/workspace"))
                .respond_with(ResponseTemplate::new(204))
                .mount(&server)
                .await;
            assert!(matches!(
                client(&server)
                    .workspace_operation(request(operation.clone()))
                    .await
                    .unwrap(),
                WorkspaceReply::Changed
            ));
            server.reset().await;
            healthy(&server).await;
            Mock::given(method("POST"))
                .and(path("/workspace"))
                .respond_with(ResponseTemplate::new(200).set_body_string("unexpected success"))
                .mount(&server)
                .await;
            assert!(matches!(
                client(&server)
                    .workspace_operation(request(operation))
                    .await,
                Err(WorkspaceClientError::Failed)
            ));
        }
    }

    #[tokio::test]
    async fn workspace_client_maps_refusals_without_exposing_runner_body_data() {
        for status in [400, 404, 409, 413, 503, 500] {
            let server = MockServer::start().await;
            healthy(&server).await;
            Mock::given(method("POST"))
                .and(path("/workspace"))
                .respond_with(
                    ResponseTemplate::new(status)
                        .set_body_string("private-token /host/private/path"),
                )
                .mount(&server)
                .await;
            let error = client(&server)
                .workspace_operation(list())
                .await
                .unwrap_err();
            match status {
                400 => assert!(matches!(error, WorkspaceClientError::InvalidRequest)),
                404 => assert!(matches!(error, WorkspaceClientError::NotFound)),
                409 => assert!(matches!(error, WorkspaceClientError::Conflict)),
                413 => assert!(matches!(error, WorkspaceClientError::TooLarge)),
                503 => assert!(matches!(error, WorkspaceClientError::Unavailable)),
                _ => assert!(matches!(error, WorkspaceClientError::Failed)),
            }
            assert!(!error.to_string().contains("private-token"));
            assert!(!error.to_string().contains("/host/private/path"));
        }
    }

    #[tokio::test]
    async fn workspace_client_bounds_listing_and_error_responses() {
        for (status, cap) in [(200, 1024 * 1024), (409, 16 * 1024)] {
            let server = MockServer::start().await;
            healthy(&server).await;
            Mock::given(method("POST"))
                .and(path("/workspace"))
                .respond_with(ResponseTemplate::new(status).set_body_bytes(vec![b' '; cap + 1]))
                .mount(&server)
                .await;
            assert!(matches!(
                client(&server).workspace_operation(list()).await,
                Err(WorkspaceClientError::TooLarge)
            ));
        }
    }

    async fn bounded_peer(response: Vec<u8>) -> (String, tokio::task::JoinHandle<()>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            for reply in [
                b"HTTP/1.1 200 OK\r\nContent-Length: 32\r\nConnection: close\r\n\r\n{\"status\":\"ok\",\"workspace\":true}".to_vec(),
                response,
            ] {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = vec![0; 8192];
                let _ = stream.read(&mut request).await.unwrap();
                let _ = stream.write_all(&reply).await;
                let _ = stream.shutdown().await;
            }
        });
        (url, task)
    }

    fn peer_client(url: String) -> Arc<SandboxClient> {
        SandboxClient::new(
            Arc::new(SandboxConfig {
                enabled: true,
                runner_url: url,
                timeout_secs: 2,
                max_artifact_bytes: 1024,
            }),
            crate::server::state::RuntimeSettings::new_handle("https://gateway.example"),
        )
    }

    #[tokio::test]
    async fn workspace_client_refuses_oversized_declared_file_without_buffering_it() {
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            shared::sandbox::STAGE_TOTAL_MAX_BYTES + 1
        )
        .into_bytes();
        let (url, peer) = bounded_peer(response).await;
        assert!(matches!(
            peer_client(url)
                .workspace_operation(request(WorkspaceOperation::Read {
                    path: "notes.txt".into(),
                }))
                .await,
            Err(WorkspaceClientError::TooLarge)
        ));
        tokio::time::timeout(std::time::Duration::from_secs(3), peer)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn workspace_client_bounds_chunked_listing_without_declared_length() {
        let mut response =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n".to_vec();
        for _ in 0..17 {
            response.extend_from_slice(b"10000\r\n");
            response.extend(std::iter::repeat_n(b' ', 65536));
            response.extend_from_slice(b"\r\n");
        }
        response.extend_from_slice(b"0\r\n\r\n");
        let (url, peer) = bounded_peer(response).await;
        assert!(matches!(
            peer_client(url).workspace_operation(list()).await,
            Err(WorkspaceClientError::TooLarge)
        ));
        tokio::time::timeout(std::time::Duration::from_secs(3), peer)
            .await
            .unwrap()
            .unwrap();
    }
}
