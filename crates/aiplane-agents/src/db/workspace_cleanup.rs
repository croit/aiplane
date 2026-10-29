// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use aiplane_core::server::db::{DbError, Pool, WriteTx};
use jiff::Timestamp;
use sqlx::Row;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupClaim {
    pub agent_id: String,
    pub owner_user_id: String,
    pub claim_token: String,
    pub attempts: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupErrorCode {
    Unavailable,
    Conflict,
    Failed,
}

impl CleanupErrorCode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::Conflict => "conflict",
            Self::Failed => "failed",
        }
    }
}

pub async fn claim_due(
    pool: &Pool,
    now: Timestamp,
    limit: usize,
) -> Result<Vec<CleanupClaim>, DbError> {
    let limit = limit.min(16);
    if limit == 0 {
        return Ok(Vec::new());
    }
    let mut tx = WriteTx::begin(pool).await?;
    let rows = sqlx::query(
        "SELECT agent_id, owner_user_id, attempts FROM agent_workspace_cleanup
         WHERE julianday(next_attempt_at) <= julianday(?)
         AND (claim_until IS NULL OR julianday(claim_until) <= julianday(?))
         ORDER BY julianday(next_attempt_at), agent_id LIMIT ?",
    )
    .bind(now.to_string())
    .bind(now.to_string())
    .bind(limit as i64)
    .fetch_all(&mut *tx)
    .await?;
    let until = now + jiff::Span::new().seconds(120);
    let mut claims = Vec::with_capacity(rows.len());
    for row in rows {
        let agent_id: String = row.try_get("agent_id")?;
        let owner_user_id: String = row.try_get("owner_user_id")?;
        let attempts: i64 = row.try_get("attempts")?;
        let claim_token = uuid::Uuid::new_v4().to_string();
        let changed = sqlx::query(
            "UPDATE agent_workspace_cleanup SET claim_token = ?, claim_until = ?
             WHERE agent_id = ? AND owner_user_id = ? AND julianday(next_attempt_at) <= julianday(?)
             AND (claim_until IS NULL OR julianday(claim_until) <= julianday(?))",
        )
        .bind(&claim_token)
        .bind(until.to_string())
        .bind(&agent_id)
        .bind(&owner_user_id)
        .bind(now.to_string())
        .bind(now.to_string())
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if changed == 1 {
            claims.push(CleanupClaim {
                agent_id,
                owner_user_id,
                claim_token,
                attempts,
            });
        }
    }
    tx.commit().await?;
    Ok(claims)
}

pub async fn acknowledge(pool: &Pool, claim: &CleanupClaim) -> Result<bool, DbError> {
    Ok(sqlx::query(
        "DELETE FROM agent_workspace_cleanup WHERE agent_id = ? AND owner_user_id = ? AND claim_token = ?",
    )
    .bind(&claim.agent_id)
    .bind(&claim.owner_user_id)
    .bind(&claim.claim_token)
    .execute(pool)
    .await?
    .rows_affected() == 1)
}

pub async fn retry(
    pool: &Pool,
    claim: &CleanupClaim,
    now: Timestamp,
    error_code: CleanupErrorCode,
) -> Result<bool, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let attempts: Option<i64> = sqlx::query_scalar(
        "SELECT attempts FROM agent_workspace_cleanup WHERE agent_id = ? AND owner_user_id = ? AND claim_token = ?",
    )
    .bind(&claim.agent_id)
    .bind(&claim.owner_user_id)
    .bind(&claim.claim_token)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(attempts) = attempts else {
        tx.commit().await?;
        return Ok(false);
    };
    let attempts = attempts.saturating_add(1);
    let seconds = (30_i64 * (1_i64 << attempts.saturating_sub(1).min(7))).min(3600);
    let next = now + jiff::Span::new().seconds(seconds);
    let changed = sqlx::query(
        "UPDATE agent_workspace_cleanup SET attempts = ?, next_attempt_at = ?,
         claim_token = NULL, claim_until = NULL, last_error_code = ?
         WHERE agent_id = ? AND owner_user_id = ? AND claim_token = ?",
    )
    .bind(attempts)
    .bind(next.to_string())
    .bind(error_code.as_str())
    .bind(&claim.agent_id)
    .bind(&claim.owner_user_id)
    .bind(&claim.claim_token)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    tx.commit().await?;
    Ok(changed == 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{agents, organizations, system_principals::NewPrincipal};
    use aiplane_core::server::db::{
        self, Pool, WriteTx,
        users::{self, User},
    };
    use jiff::Timestamp;
    use std::path::Path;

    async fn seed(pool: &Pool) {
        let now = Timestamp::now();
        users::upsert(
            pool,
            &User {
                id: "alice".into(),
                email: "alice@example.com".into(),
                name: None,
                roles: vec![],
                created_at: now,
                updated_at: now,
                timezone: None,
                speech_voice: None,
            },
        )
        .await
        .unwrap();
    }

    async fn pool() -> Pool {
        let pool = db::open(Path::new(":memory:")).await.unwrap();
        seed(&pool).await;
        pool
    }

    async fn owned_agent(pool: &Pool, name: &str) -> String {
        let (_, owned_agent) = agents::create_personal_agent(
            pool,
            &NewPrincipal {
                name,
                display: name,
                description: "",
            },
            r#"{"main":{}}"#,
            "alice",
            4,
        )
        .await
        .unwrap();
        owned_agent.unwrap().principal.id
    }

    async fn removed(pool: &Pool, name: &str) -> String {
        let id = owned_agent(pool, name).await;
        assert!(agents::delete(pool, &id, "alice").await.unwrap());
        id
    }

    #[tokio::test]
    async fn deletion_queue_survives_direct_principal_and_account_cascades() {
        let pool = pool().await;
        let first = removed(&pool, "First").await;
        let second = owned_agent(&pool, "Second").await;
        sqlx::query("DELETE FROM system_principals WHERE id = ?")
            .bind(&second)
            .execute(&pool)
            .await
            .unwrap();
        let third = owned_agent(&pool, "Third").await;
        sqlx::query("DELETE FROM users WHERE id = 'alice'")
            .execute(&pool)
            .await
            .unwrap();
        let mut claims = claim_due(&pool, Timestamp::now(), 16).await.unwrap();
        claims.sort_by(|a, b| a.agent_id.cmp(&b.agent_id));
        let mut expected = vec![first, second, third];
        expected.sort();
        assert_eq!(
            claims
                .iter()
                .map(|claim| claim.agent_id.clone())
                .collect::<Vec<_>>(),
            expected
        );
        assert!(
            claims
                .iter()
                .all(|claim| claim.owner_user_id == "alice" && claim.attempts == 0)
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM users")
                .fetch_one(&pool)
                .await
                .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn rollback_deactivation_and_nonpersonal_deletion_do_not_enqueue_cleanup() {
        let pool = pool().await;
        let personal = owned_agent(&pool, "Personal").await;
        {
            let mut tx = WriteTx::begin(&pool).await.unwrap();
            assert!(
                agents::delete_in(&mut tx, &personal, "alice")
                    .await
                    .unwrap()
            );
            assert_eq!(
                sqlx::query_scalar::<_, i64>("SELECT count(*) FROM agent_workspace_cleanup")
                    .fetch_one(&mut *tx)
                    .await
                    .unwrap(),
                1
            );
        }
        assert!(
            agents::set_personal_agent_active(&pool, &personal, "alice", false, 4)
                .await
                .unwrap()
        );
        let ordinary = agents::create(
            &pool,
            &NewPrincipal {
                name: "Ordinary",
                display: "Ordinary",
                description: "",
            },
            r#"{"main":{}}"#,
            "alice",
        )
        .await
        .unwrap()
        .unwrap();
        assert!(
            agents::delete(&pool, &ordinary.principal.id, "alice")
                .await
                .unwrap()
        );
        let organization = organizations::create(
            &pool,
            &organizations::NewOrganization {
                name: "Organization",
                description: "",
                member_groups: &[],
            },
            "alice",
        )
        .await
        .unwrap();
        let organizations::OrganizationCreateResult::Created(organization) = organization else {
            panic!("The organization fixture must be created");
        };
        let mut tx = WriteTx::begin(&pool).await.unwrap();
        let (_, org_agent) = agents::create_organization_agent_in(
            &mut tx,
            &organization.organization.id,
            &NewPrincipal {
                name: "Organization agent",
                display: "Organization agent",
                description: "",
            },
            r#"{"main":{}}"#,
            "alice",
            4,
        )
        .await
        .unwrap();
        assert!(
            agents::delete_in(&mut tx, &org_agent.unwrap().principal.id, "alice")
                .await
                .unwrap()
        );
        tx.commit().await.unwrap();
        assert!(
            claim_due(&pool, Timestamp::now(), 16)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            agents::get_personal_agent(&pool, "alice", &personal)
                .await
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn expired_claim_replacement_blocks_stale_acknowledgements_and_retries() {
        let pool = pool().await;
        removed(&pool, "Harald").await;
        let now = Timestamp::now();
        let original = claim_due(&pool, now, 1).await.unwrap().pop().unwrap();
        assert!(
            claim_due(&pool, now + jiff::Span::new().seconds(119), 1)
                .await
                .unwrap()
                .is_empty()
        );
        let current = claim_due(&pool, now + jiff::Span::new().seconds(120), 1)
            .await
            .unwrap()
            .pop()
            .unwrap();
        assert_ne!(original.claim_token, current.claim_token);
        assert!(!acknowledge(&pool, &original).await.unwrap());
        assert!(
            !retry(&pool, &original, now, CleanupErrorCode::Failed)
                .await
                .unwrap()
        );
        assert!(acknowledge(&pool, &current).await.unwrap());
        assert!(!acknowledge(&pool, &current).await.unwrap());
    }

    #[tokio::test]
    async fn retry_backoff_is_due_bounded_and_attempts_saturate() {
        let pool = pool().await;
        let id = removed(&pool, "Harald").await;
        let now = Timestamp::now();
        let original = claim_due(&pool, now, 1).await.unwrap().pop().unwrap();
        assert!(
            retry(&pool, &original, now, CleanupErrorCode::Unavailable)
                .await
                .unwrap()
        );
        assert!(
            claim_due(&pool, now + jiff::Span::new().seconds(29), 1)
                .await
                .unwrap()
                .is_empty()
        );
        let second = claim_due(&pool, now + jiff::Span::new().seconds(30), 1)
            .await
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(second.attempts, 1);
        sqlx::query("UPDATE agent_workspace_cleanup SET attempts = ? WHERE agent_id = ?")
            .bind(i64::MAX)
            .bind(&id)
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            retry(&pool, &second, now, CleanupErrorCode::Conflict)
                .await
                .unwrap()
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT attempts FROM agent_workspace_cleanup WHERE agent_id = ?"
            )
            .bind(&id)
            .fetch_one(&pool)
            .await
            .unwrap(),
            i64::MAX
        );
        assert!(
            claim_due(&pool, now + jiff::Span::new().seconds(3599), 1)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            claim_due(&pool, now + jiff::Span::new().seconds(3600), 1)
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn concurrent_claims_are_exclusive_and_batches_are_bounded() {
        let pool = pool().await;
        removed(&pool, "Harald").await;
        let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(8));
        let now = Timestamp::now();
        let mut handles = Vec::new();
        for _ in 0..8 {
            let pool = pool.clone();
            let barrier = barrier.clone();
            handles.push(tokio::spawn(async move {
                barrier.wait().await;
                claim_due(&pool, now, 1).await.unwrap()
            }));
        }
        let mut claims = Vec::new();
        for handle in handles {
            claims.extend(handle.await.unwrap());
        }
        assert_eq!(claims.len(), 1);
        assert!(acknowledge(&pool, &claims[0]).await.unwrap());
        for index in 0..20 {
            removed(&pool, &format!("Agent {index}")).await;
        }
        let now = Timestamp::now();
        assert!(claim_due(&pool, now, 0).await.unwrap().is_empty());
        assert_eq!(claim_due(&pool, now, usize::MAX).await.unwrap().len(), 16);
        assert_eq!(claim_due(&pool, now, 16).await.unwrap().len(), 4);
    }

    #[tokio::test]
    async fn queued_cleanup_and_claim_lease_survive_reopening_sqlite() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cleanup.sqlite");
        let pool = db::open(&path).await.unwrap();
        seed(&pool).await;
        let id = removed(&pool, "Harald").await;
        let now = Timestamp::now();
        let original = claim_due(&pool, now, 1).await.unwrap().pop().unwrap();
        pool.close().await;
        let pool = db::open(&path).await.unwrap();
        assert!(claim_due(&pool, now, 1).await.unwrap().is_empty());
        let recovered = claim_due(&pool, now + jiff::Span::new().seconds(120), 1)
            .await
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(recovered.agent_id, id);
        assert_eq!(recovered.owner_user_id, "alice");
        assert_ne!(recovered.claim_token, original.claim_token);
        assert!(acknowledge(&pool, &recovered).await.unwrap());
        pool.close().await;
    }
}
