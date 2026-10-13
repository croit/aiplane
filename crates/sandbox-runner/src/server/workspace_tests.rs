// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use super::*;
use crate::backend::fake::{FakeBackend, req};
use crate::config::Config;
use rama::Service;
use rama::http::{Body, Method};
use shared::sandbox::{DotWorkspace, DotWorkspaceScope, WorkspaceListing, WorkspacePurpose};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Fixture {
    root: PathBuf,
    state: Arc<RunnerState>,
    identity: DotWorkspace,
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "aiplane-route-workspace-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        let root = std::fs::canonicalize(root).unwrap();
        let mut config = Config::for_test();
        config.work_root = Some(root.clone());
        Self {
            root,
            state: Arc::new(RunnerState {
                pool: Pool::new(Arc::new(FakeBackend::new()), Arc::new(config)),
            }),
            identity: DotWorkspace {
                owner_user_id: "person".into(),
                dot_id: "00000000-0000-4000-8000-000000000001".into(),
            },
        }
    }

    async fn send(&self, operation: WorkspaceOperation) -> Response {
        self.send_as(self.identity.clone(), operation).await
    }

    async fn send_as(&self, workspace: DotWorkspace, operation: WorkspaceOperation) -> Response {
        service(self.state.clone())
            .serve(
                Request::builder()
                    .method(Method::POST)
                    .uri("/workspace")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&WorkspaceRequest {
                            workspace,
                            operation,
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    fn dot_root(&self) -> PathBuf {
        self.root.join("dots").join(&self.identity.dot_id)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

async fn bytes(response: Response) -> Vec<u8> {
    let mut stream = response.into_body().into_data_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        bytes.extend_from_slice(&chunk.unwrap());
        assert!(bytes.len() <= MAX_RUN_REQUEST_BYTES);
    }
    bytes
}

fn write(path: &str, content: &[u8]) -> WorkspaceOperation {
    WorkspaceOperation::Write {
        path: path.into(),
        content_b64: shared::b64::encode(content),
    }
}

#[tokio::test]
async fn workspace_routes_manage_real_utf8_files_and_empty_folders() {
    let fixture = Fixture::new();
    assert_eq!(
        fixture
            .send(WorkspaceOperation::CreateFolder {
                path: "project".into()
            })
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    let content = "A UTF-8 boundary: ß".as_bytes();
    assert_eq!(
        fixture
            .send(write("project/notes.txt", content))
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    let response = fixture
        .send(WorkspaceOperation::List {
            path: "project".into(),
            after: None,
        })
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let listing: WorkspaceListing = serde_json::from_slice(&bytes(response).await).unwrap();
    assert_eq!(listing.entries.len(), 1);
    assert_eq!(listing.entries[0].name, "notes.txt");
    assert_eq!(
        listing.entries[0].kind,
        shared::sandbox::WorkspaceEntryKind::File
    );
    assert_eq!(listing.entries[0].size_bytes, Some(content.len() as u64));
    assert_eq!(listing.next_cursor, None);
    let response = fixture
        .send(WorkspaceOperation::Read {
            path: "project/notes.txt".into(),
        })
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "application/octet-stream"
    );
    assert_eq!(bytes(response).await, content);
    assert_eq!(
        fixture
            .send(WorkspaceOperation::Remove {
                path: "project".into()
            })
            .await
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        fixture
            .send(WorkspaceOperation::Remove {
                path: "project/notes.txt".into()
            })
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        fixture
            .send(WorkspaceOperation::Remove {
                path: "project".into()
            })
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    let response = fixture
        .send(WorkspaceOperation::List {
            path: "".into(),
            after: None,
        })
        .await;
    let listing: WorkspaceListing = serde_json::from_slice(&bytes(response).await).unwrap();
    assert!(listing.entries.is_empty());
}

#[tokio::test]
async fn workspace_routes_reject_foreign_ownership_without_changing_files_or_manifest() {
    let fixture = Fixture::new();
    assert_eq!(
        fixture.send(write("notes.txt", b"original")).await.status(),
        StatusCode::NO_CONTENT
    );
    let manifest = std::fs::read(fixture.dot_root().join("owner.json")).unwrap();
    let mut foreign = fixture.identity.clone();
    foreign.owner_user_id = "foreign-person".into();
    for operation in [
        WorkspaceOperation::List {
            path: "".into(),
            after: None,
        },
        WorkspaceOperation::Read {
            path: "notes.txt".into(),
        },
        write("notes.txt", b"replacement"),
        WorkspaceOperation::Remove {
            path: "notes.txt".into(),
        },
    ] {
        assert_eq!(
            fixture.send_as(foreign.clone(), operation).await.status(),
            StatusCode::CONFLICT
        );
    }
    assert_eq!(
        std::fs::read(fixture.dot_root().join("owner.json")).unwrap(),
        manifest
    );
    assert_eq!(
        std::fs::read(fixture.dot_root().join("files/notes.txt")).unwrap(),
        b"original"
    );
}

#[tokio::test]
async fn workspace_routes_refuse_traversal_and_symlinks_without_exposing_private_metadata() {
    let fixture = Fixture::new();
    assert_eq!(
        fixture.send(write("notes.txt", b"original")).await.status(),
        StatusCode::NO_CONTENT
    );
    for path in [
        "../owner.json",
        "../browserprofile",
        "/owner.json",
        "project/../notes.txt",
    ] {
        assert_eq!(
            fixture
                .send(WorkspaceOperation::Read { path: path.into() })
                .await
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    symlink(
        "../owner.json",
        fixture.dot_root().join("files/manifest-link"),
    )
    .unwrap();
    assert_eq!(
        fixture
            .send(WorkspaceOperation::Read {
                path: "manifest-link".into()
            })
            .await
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        fixture
            .send(write("manifest-link", b"replacement"))
            .await
            .status(),
        StatusCode::CONFLICT
    );
    let manifest: DotWorkspace =
        serde_json::from_slice(&std::fs::read(fixture.dot_root().join("owner.json")).unwrap())
            .unwrap();
    assert_eq!(manifest, fixture.identity);
}

#[tokio::test]
async fn workspace_bad_json_base64_and_oversized_body_leave_original_file_unchanged() {
    let fixture = Fixture::new();
    assert_eq!(
        fixture.send(write("notes.txt", b"original")).await.status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        fixture
            .send(WorkspaceOperation::Write {
                path: "notes.txt".into(),
                content_b64: "%%%".into()
            })
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    for (body, declared_size, expected) in [
        ("{", None, StatusCode::BAD_REQUEST),
        (
            "{}",
            Some(MAX_RUN_REQUEST_BYTES + 1),
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
    ] {
        let mut request = Request::builder().method(Method::POST).uri("/workspace");
        if let Some(size) = declared_size {
            request = request.header(header::CONTENT_LENGTH, size.to_string());
        }
        let response = service(fixture.state.clone())
            .serve(request.body(Body::from(body)).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
    assert_eq!(
        std::fs::read(fixture.dot_root().join("files/notes.txt")).unwrap(),
        b"original"
    );
}

#[tokio::test]
async fn workspace_routes_allow_busy_reads_but_require_idle_mutations() {
    let fixture = Fixture::new();
    assert_eq!(
        fixture.send(write("notes.txt", b"original")).await.status(),
        StatusCode::NO_CONTENT
    );
    let mut run = req();
    run.keep_alive = true;
    run.workspace = Some(DotWorkspaceScope {
        workspace: fixture.identity.clone(),
        turn_id: "workspace-turn".into(),
        purpose: WorkspacePurpose::Workspace,
    });
    let container = fixture
        .state
        .pool
        .run(&run)
        .await
        .unwrap()
        .container_id
        .unwrap();
    assert_eq!(
        fixture
            .send(WorkspaceOperation::List {
                path: "".into(),
                after: None
            })
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        bytes(
            fixture
                .send(WorkspaceOperation::Read {
                    path: "notes.txt".into()
                })
                .await
        )
        .await,
        b"original"
    );
    for operation in [
        write("notes.txt", b"replacement"),
        WorkspaceOperation::CreateFolder {
            path: "project".into(),
        },
        WorkspaceOperation::Remove {
            path: "notes.txt".into(),
        },
    ] {
        assert_eq!(fixture.send(operation).await.status(), StatusCode::CONFLICT);
    }
    fixture
        .state
        .pool
        .release_container(&container)
        .await
        .unwrap();
    assert_eq!(
        fixture
            .send(write("notes.txt", b"replacement"))
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
}
