// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use jiff::Timestamp;
use serde_json::json;
use sqlx::Row;

use super::{
    DbError, Pool, WriteTx,
    agent_audit::{self, AuditKind},
    parse_ts,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DotDelegationEdge {
    pub caller_dot_id: String,
    pub receiver_dot_id: String,
    pub outbound_enabled: bool,
    pub inbound_enabled: bool,
    pub generation: String,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsentDirection {
    Outbound,
    Inbound,
}

pub struct ConsentEdit<'a> {
    pub owner_user_id: &'a str,
    pub dot_id: &'a str,
    pub peer_id: &'a str,
    pub direction: ConsentDirection,
    pub enabled: bool,
    pub expected_generation: Option<&'a str>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ConsentSaved {
    Saved(DotDelegationEdge),
    Missing,
    Invalid,
    Conflict,
    Unavailable,
}

fn edge(row: sqlx::sqlite::SqliteRow) -> Result<DotDelegationEdge, DbError> {
    Ok(DotDelegationEdge {
        caller_dot_id: row.try_get("caller_dot_id")?,
        receiver_dot_id: row.try_get("receiver_dot_id")?,
        outbound_enabled: row.try_get("outbound_enabled")?,
        inbound_enabled: row.try_get("inbound_enabled")?,
        generation: row.try_get("generation")?,
        updated_at: parse_ts(row.try_get("updated_at")?, "updated_at")?,
    })
}

const OWNED_EDGES: &str = "SELECT e.* FROM dot_delegation_edges e
 JOIN personal_dots c ON c.principal_id = e.caller_dot_id
 JOIN personal_dots r ON r.principal_id = e.receiver_dot_id
 WHERE c.owner_user_id = ? AND r.owner_user_id = ?";

pub async fn list_for_dot(
    pool: &Pool,
    owner_user_id: &str,
    dot_id: &str,
) -> Result<Vec<DotDelegationEdge>, DbError> {
    sqlx::query(&format!("{OWNED_EDGES} AND (e.caller_dot_id = ? OR e.receiver_dot_id = ?) ORDER BY e.caller_dot_id, e.receiver_dot_id"))
        .bind(owner_user_id).bind(owner_user_id).bind(dot_id).bind(dot_id)
        .fetch_all(pool).await?.into_iter().map(edge).collect()
}

pub async fn authorized(
    pool: &Pool,
    owner_user_id: &str,
    caller_dot_id: &str,
    receiver_dot_id: &str,
    expected_generation: &str,
) -> Result<Option<DotDelegationEdge>, DbError> {
    let mut connection = pool.acquire().await?;
    authorized_in(
        &mut connection,
        owner_user_id,
        caller_dot_id,
        receiver_dot_id,
        expected_generation,
    )
    .await
}

pub async fn authorized_in(
    connection: &mut sqlx::SqliteConnection,
    owner_user_id: &str,
    caller_dot_id: &str,
    receiver_dot_id: &str,
    expected_generation: &str,
) -> Result<Option<DotDelegationEdge>, DbError> {
    sqlx::query(&format!("{OWNED_EDGES} AND e.caller_dot_id = ? AND e.receiver_dot_id = ? AND c.active = 1 AND r.active = 1 AND e.outbound_enabled = 1 AND e.inbound_enabled = 1 AND e.generation = ?"))
        .bind(owner_user_id).bind(owner_user_id).bind(caller_dot_id).bind(receiver_dot_id).bind(expected_generation)
        .fetch_optional(connection).await?.map(edge).transpose()
}

pub async fn set_consent(pool: &Pool, edit: ConsentEdit<'_>) -> Result<ConsentSaved, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let active: Option<(bool, bool)> = sqlx::query_as("SELECT d.active, p.active FROM personal_dots d JOIN personal_dots p ON p.principal_id = ? WHERE d.principal_id = ? AND d.owner_user_id = ? AND p.owner_user_id = ?")
        .bind(edit.peer_id).bind(edit.dot_id).bind(edit.owner_user_id).bind(edit.owner_user_id)
        .fetch_optional(&mut *tx).await?;
    let Some((dot_active, peer_active)) = active else {
        return Ok(ConsentSaved::Missing);
    };
    if edit.dot_id == edit.peer_id {
        return Ok(ConsentSaved::Invalid);
    }
    let (caller, receiver) = match edit.direction {
        ConsentDirection::Outbound => (edit.dot_id, edit.peer_id),
        ConsentDirection::Inbound => (edit.peer_id, edit.dot_id),
    };
    let current = sqlx::query(
        "SELECT * FROM dot_delegation_edges WHERE caller_dot_id = ? AND receiver_dot_id = ?",
    )
    .bind(caller)
    .bind(receiver)
    .fetch_optional(&mut *tx)
    .await?
    .map(edge)
    .transpose()?;
    if current.as_ref().map(|value| value.generation.as_str()) != edit.expected_generation {
        return Ok(ConsentSaved::Conflict);
    }
    let current_flag = current
        .as_ref()
        .map(|value| match edit.direction {
            ConsentDirection::Outbound => value.outbound_enabled,
            ConsentDirection::Inbound => value.inbound_enabled,
        })
        .unwrap_or(false);
    if let Some(current) = &current
        && current_flag == edit.enabled
    {
        return Ok(ConsentSaved::Saved(current.clone()));
    }
    if edit.enabled && (!dot_active || !peer_active) {
        return Ok(ConsentSaved::Unavailable);
    }
    let mut updated = current.unwrap_or_else(|| DotDelegationEdge {
        caller_dot_id: caller.into(),
        receiver_dot_id: receiver.into(),
        outbound_enabled: false,
        inbound_enabled: false,
        generation: String::new(),
        updated_at: Timestamp::now(),
    });
    match edit.direction {
        ConsentDirection::Outbound => updated.outbound_enabled = edit.enabled,
        ConsentDirection::Inbound => updated.inbound_enabled = edit.enabled,
    }
    updated.generation = uuid::Uuid::new_v4().to_string();
    updated.updated_at = Timestamp::now();
    sqlx::query("INSERT INTO dot_delegation_edges (caller_dot_id, receiver_dot_id, outbound_enabled, inbound_enabled, generation, updated_at) VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT(caller_dot_id, receiver_dot_id) DO UPDATE SET outbound_enabled = excluded.outbound_enabled, inbound_enabled = excluded.inbound_enabled, generation = excluded.generation, updated_at = excluded.updated_at")
        .bind(caller).bind(receiver).bind(updated.outbound_enabled).bind(updated.inbound_enabled)
        .bind(&updated.generation).bind(updated.updated_at.to_string()).execute(&mut *tx).await?;
    agent_audit::append(&mut tx, agent_audit::NewEvent::new(AuditKind::PersonalDotDelegationChanged, edit.dot_id, json!({
        "caller_dot_id": caller, "receiver_dot_id": receiver,
        "direction": match edit.direction { ConsentDirection::Outbound => "outbound", ConsentDirection::Inbound => "inbound" },
        "outbound_enabled": updated.outbound_enabled, "inbound_enabled": updated.inbound_enabled,
        "generation": updated.generation,
    })).by(Some(edit.owner_user_id))).await?;
    tx.commit().await?;
    Ok(ConsentSaved::Saved(updated))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{Pool, agents, system_principals::NewPrincipal};
    use aiplane_core::server::db::users::{self, User};
    use jiff::Timestamp;
    use std::path::Path;

    async fn fixture() -> (Pool, String, String, String) {
        let pool = aiplane_core::server::db::open(Path::new(":memory:"))
            .await
            .unwrap();
        for owner in ["alice", "bob"] {
            let now = Timestamp::now();
            users::upsert(
                &pool,
                &User {
                    id: owner.into(),
                    email: format!("{owner}@example.com"),
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
        let mut ids = Vec::new();
        for (name, owner) in [
            ("sender", "alice"),
            ("receiver", "alice"),
            ("foreign", "bob"),
        ] {
            let (_, dot) = agents::create_personal_dot(
                &pool,
                &NewPrincipal {
                    name,
                    display: name,
                    description: "",
                },
                r#"{"main":{}}"#,
                owner,
                4,
            )
            .await
            .unwrap();
            ids.push(dot.unwrap().principal.id);
        }
        (pool, ids.remove(0), ids.remove(0), ids.remove(0))
    }

    #[tokio::test]
    async fn directed_consent_schema_defaults_to_denied_and_rejects_self_edges() {
        let (pool, caller, receiver, _) = fixture().await;
        sqlx::query("INSERT INTO dot_delegation_edges (caller_dot_id, receiver_dot_id, generation, updated_at) VALUES (?, ?, ?, ?)")
            .bind(&caller).bind(&receiver).bind(uuid::Uuid::new_v4().to_string())
            .bind(Timestamp::now().to_string()).execute(&pool).await.unwrap();
        let flags: (bool, bool) = sqlx::query_as("SELECT outbound_enabled, inbound_enabled FROM dot_delegation_edges WHERE caller_dot_id = ? AND receiver_dot_id = ?")
            .bind(&caller).bind(&receiver).fetch_one(&pool).await.unwrap();
        assert_eq!(flags, (false, false));
        assert!(sqlx::query("INSERT INTO dot_delegation_edges (caller_dot_id, receiver_dot_id, generation, updated_at) VALUES (?, ?, ?, ?)")
            .bind(&caller).bind(&caller).bind(uuid::Uuid::new_v4().to_string())
            .bind(Timestamp::now().to_string()).execute(&pool).await.is_err());
        assert!(sqlx::query("INSERT INTO dot_delegation_edges (caller_dot_id, receiver_dot_id, generation, updated_at) VALUES (?, ?, ?, ?)")
            .bind(&caller).bind(&receiver).bind(uuid::Uuid::new_v4().to_string())
            .bind(Timestamp::now().to_string()).execute(&pool).await.is_err());
        sqlx::query("DELETE FROM personal_dots WHERE principal_id = ?")
            .bind(&receiver)
            .execute(&pool)
            .await
            .unwrap();
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM dot_delegation_edges")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    async fn edit(
        pool: &Pool,
        dot: &str,
        peer: &str,
        direction: ConsentDirection,
        enabled: bool,
        generation: Option<&str>,
    ) -> ConsentSaved {
        set_consent(
            pool,
            ConsentEdit {
                owner_user_id: "alice",
                dot_id: dot,
                peer_id: peer,
                direction,
                enabled,
                expected_generation: generation,
            },
        )
        .await
        .unwrap()
    }

    fn saved(result: ConsentSaved) -> DotDelegationEdge {
        match result {
            ConsentSaved::Saved(edge) => edge,
            other => panic!("expected saved consent, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn directed_consent_requires_both_decisions_and_exact_live_generation() {
        let (pool, caller, receiver, _) = fixture().await;
        assert!(
            list_for_dot(&pool, "alice", &caller)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            authorized(&pool, "alice", &caller, &receiver, "absent")
                .await
                .unwrap()
                .is_none()
        );
        let first = saved(
            edit(
                &pool,
                &caller,
                &receiver,
                ConsentDirection::Outbound,
                true,
                None,
            )
            .await,
        );
        assert!(first.outbound_enabled && !first.inbound_enabled);
        assert!(
            authorized(&pool, "alice", &caller, &receiver, &first.generation)
                .await
                .unwrap()
                .is_none()
        );
        let both = saved(
            edit(
                &pool,
                &receiver,
                &caller,
                ConsentDirection::Inbound,
                true,
                Some(&first.generation),
            )
            .await,
        );
        assert!(both.outbound_enabled && both.inbound_enabled);
        assert_ne!(first.generation, both.generation);
        assert!(
            authorized(&pool, "alice", &caller, &receiver, &first.generation)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            authorized(&pool, "alice", &caller, &receiver, &both.generation)
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            authorized(&pool, "alice", &receiver, &caller, &both.generation)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            authorized(&pool, "bob", &caller, &receiver, &both.generation)
                .await
                .unwrap()
                .is_none()
        );
        let edges = list_for_dot(&pool, "alice", &receiver).await.unwrap();
        assert_eq!(edges, vec![both.clone()]);
        let unchanged = saved(
            edit(
                &pool,
                &receiver,
                &caller,
                ConsentDirection::Inbound,
                true,
                Some(&both.generation),
            )
            .await,
        );
        assert_eq!(unchanged, both);
        assert!(matches!(
            edit(
                &pool,
                &receiver,
                &caller,
                ConsentDirection::Inbound,
                true,
                Some(&first.generation)
            )
            .await,
            ConsentSaved::Conflict
        ));
        let revoked = saved(
            edit(
                &pool,
                &caller,
                &receiver,
                ConsentDirection::Outbound,
                false,
                Some(&both.generation),
            )
            .await,
        );
        assert!(!revoked.outbound_enabled && revoked.inbound_enabled);
        assert!(
            authorized(&pool, "alice", &caller, &receiver, &both.generation)
                .await
                .unwrap()
                .is_none()
        );
        let renewed = saved(
            edit(
                &pool,
                &caller,
                &receiver,
                ConsentDirection::Outbound,
                true,
                Some(&revoked.generation),
            )
            .await,
        );
        assert_ne!(renewed.generation, revoked.generation);
        assert_ne!(renewed.generation, both.generation);
    }

    #[tokio::test]
    async fn directed_consent_owner_active_and_noop_rules_preserve_metadata_audit() {
        let (pool, caller, receiver, foreign) = fixture().await;
        assert!(matches!(
            edit(
                &pool,
                &caller,
                &foreign,
                ConsentDirection::Outbound,
                true,
                None
            )
            .await,
            ConsentSaved::Missing
        ));
        assert!(matches!(
            edit(
                &pool,
                &caller,
                &caller,
                ConsentDirection::Outbound,
                true,
                None
            )
            .await,
            ConsentSaved::Invalid
        ));
        let first = saved(
            edit(
                &pool,
                &caller,
                &receiver,
                ConsentDirection::Outbound,
                false,
                None,
            )
            .await,
        );
        assert!(!first.outbound_enabled && !first.inbound_enabled);
        let initial_audits: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM agent_audit WHERE kind = 'personal_dot_delegation_changed'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(initial_audits, 1);
        let (actor, detail): (String, String) = sqlx::query_as("SELECT actor_id, detail FROM agent_audit WHERE kind = 'personal_dot_delegation_changed'").fetch_one(&pool).await.unwrap();
        assert_eq!(actor, "alice");
        let detail: serde_json::Value = serde_json::from_str(&detail).unwrap();
        assert_eq!(detail["caller_dot_id"], caller);
        assert_eq!(detail["receiver_dot_id"], receiver);
        assert_eq!(detail["generation"], first.generation);
        assert_eq!(detail.as_object().unwrap().len(), 6);
        assert_eq!(
            saved(
                edit(
                    &pool,
                    &caller,
                    &receiver,
                    ConsentDirection::Outbound,
                    false,
                    Some(&first.generation)
                )
                .await
            ),
            first
        );
        let audit_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM agent_audit WHERE kind = 'personal_dot_delegation_changed'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(audit_count, initial_audits);
        let enabled = saved(
            edit(
                &pool,
                &caller,
                &receiver,
                ConsentDirection::Outbound,
                true,
                Some(&first.generation),
            )
            .await,
        );
        agents::set_personal_dot_active(&pool, &receiver, "alice", false, 4)
            .await
            .unwrap();
        assert_eq!(
            saved(
                edit(
                    &pool,
                    &caller,
                    &receiver,
                    ConsentDirection::Outbound,
                    true,
                    Some(&enabled.generation)
                )
                .await
            ),
            enabled
        );
        assert!(matches!(
            edit(
                &pool,
                &receiver,
                &caller,
                ConsentDirection::Inbound,
                true,
                Some(&enabled.generation)
            )
            .await,
            ConsentSaved::Unavailable
        ));
        let revoked = saved(
            edit(
                &pool,
                &caller,
                &receiver,
                ConsentDirection::Outbound,
                false,
                Some(&enabled.generation),
            )
            .await,
        );
        assert!(!revoked.outbound_enabled);
        assert!(matches!(
            edit(
                &pool,
                &caller,
                &receiver,
                ConsentDirection::Outbound,
                true,
                Some(&revoked.generation)
            )
            .await,
            ConsentSaved::Unavailable
        ));
        assert!(
            list_for_dot(&pool, "bob", &caller)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            list_for_dot(&pool, "alice", &caller).await.unwrap(),
            vec![revoked]
        );
        sqlx::query("DELETE FROM personal_dots WHERE principal_id = ?")
            .bind(&receiver)
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            list_for_dot(&pool, "alice", &caller)
                .await
                .unwrap()
                .is_empty()
        );
        let audit_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM agent_audit WHERE kind = 'personal_dot_delegation_changed'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(audit_count, 3);
    }
}
