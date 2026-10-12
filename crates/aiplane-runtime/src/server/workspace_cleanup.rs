// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use std::sync::Arc;
use std::time::{Duration, Instant};

use aiplane_agents::db::workspace_cleanup::{self, CleanupErrorCode};
use aiplane_core::server::db::{DbError, Pool};
use jiff::Timestamp;
use shared::sandbox::{DotWorkspace, WorkspaceOperation, WorkspaceRequest};

use crate::rama_server::state::RamaState;
use crate::server::tools::sandbox::{SandboxClient, WorkspaceClientError, WorkspaceReply};

pub fn spawn(state: Arc<RamaState>) {
    tokio::spawn(async move {
        loop {
            let client = state.sandbox_client();
            if let Err(error) = drain_once(&state.db, client.as_deref(), Timestamp::now()).await {
                tracing::warn!(error = %error, "Dot workspace cleanup pass failed; queued work remains retryable");
            }
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });
}

async fn drain_once(
    pool: &Pool,
    client: Option<&SandboxClient>,
    now: Timestamp,
) -> Result<usize, DbError> {
    let started = Instant::now();
    let mut processed = 0;
    for _ in 0..16 {
        let current = now + jiff::Span::new().seconds(started.elapsed().as_secs() as i64);
        let Some(claim) = workspace_cleanup::claim_due(pool, current, 1).await?.pop() else {
            break;
        };
        let request = WorkspaceRequest {
            workspace: DotWorkspace {
                owner_user_id: claim.owner_user_id.clone(),
                dot_id: claim.dot_id.clone(),
            },
            operation: WorkspaceOperation::Retire,
        };
        let outcome = match client {
            None => Err(CleanupErrorCode::Unavailable),
            Some(client) => match tokio::time::timeout(
                Duration::from_secs(30),
                client.workspace_operation(request),
            )
            .await
            {
                Ok(Ok(WorkspaceReply::Changed)) => Ok(()),
                Ok(Err(WorkspaceClientError::Unavailable)) => Err(CleanupErrorCode::Unavailable),
                Ok(Err(WorkspaceClientError::Conflict)) => Err(CleanupErrorCode::Conflict),
                _ => Err(CleanupErrorCode::Failed),
            },
        };
        match outcome {
            Ok(()) => {
                workspace_cleanup::acknowledge(pool, &claim).await?;
            }
            Err(code) => {
                let current = now + jiff::Span::new().seconds(started.elapsed().as_secs() as i64);
                workspace_cleanup::retry(pool, &claim, current, code).await?;
            }
        }
        processed += 1;
    }
    Ok(processed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::tools::sandbox::SandboxClient;
    use aiplane_core::server::{config::SandboxConfig, db};
    use std::{path::Path, sync::Arc};
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_json, method, path},
    };

    async fn queue() -> db::Pool {
        let pool = db::open(Path::new(":memory:")).await.unwrap();
        sqlx::query("INSERT INTO dot_workspace_cleanup(dot_id,owner_user_id,created_at,next_attempt_at) VALUES('00000000-0000-4000-8000-000000000001','deleted-owner','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')").execute(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn workspace_cleanup_keeps_unavailable_work_with_controlled_retry_state() {
        let pool = queue().await;
        let now = "2026-02-01T00:00:00Z".parse().unwrap();
        assert_eq!(drain_once(&pool, None, now).await.unwrap(), 1);
        let row: (i64, String, Option<String>, Option<String>) = sqlx::query_as(
            "SELECT attempts,last_error_code,claim_token,claim_until FROM dot_workspace_cleanup",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row, (1, "unavailable".into(), None, None));
        assert_eq!(drain_once(&pool, None, now).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn workspace_cleanup_acknowledges_only_completed_retirement_and_keeps_conflicts() {
        for (status, remaining, error) in [
            (204, 0, None),
            (409, 1, Some("conflict")),
            (500, 1, Some("failed")),
        ] {
            let pool = queue().await;
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/healthz"))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(serde_json::json!({"status":"ok","workspace":true})),
                )
                .mount(&server)
                .await;
            Mock::given(method("POST")).and(path("/workspace"))
                .and(body_json(serde_json::json!({"workspace":{"owner_user_id":"deleted-owner","dot_id":"00000000-0000-4000-8000-000000000001"},"operation":{"kind":"retire"}})))
                .respond_with(ResponseTemplate::new(status)).mount(&server).await;
            let client = SandboxClient::new(
                Arc::new(SandboxConfig {
                    enabled: true,
                    runner_url: server.uri(),
                    timeout_secs: 2,
                    max_artifact_bytes: 1024,
                }),
                crate::server::state::RuntimeSettings::new_handle("https://gateway.example"),
            );
            assert_eq!(
                drain_once(
                    &pool,
                    Some(client.as_ref()),
                    "2026-02-01T00:00:00Z".parse().unwrap()
                )
                .await
                .unwrap(),
                1
            );
            assert_eq!(
                sqlx::query_scalar::<_, i64>("SELECT count(*) FROM dot_workspace_cleanup")
                    .fetch_one(&pool)
                    .await
                    .unwrap(),
                remaining
            );
            if let Some(error) = error {
                assert_eq!(
                    sqlx::query_scalar::<_, String>(
                        "SELECT last_error_code FROM dot_workspace_cleanup"
                    )
                    .fetch_one(&pool)
                    .await
                    .unwrap(),
                    error
                );
            }
        }
    }
}
