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
pub struct AgentDelegationEdge {
    pub caller_agent_id: String,
    pub receiver_agent_id: String,
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
    pub agent_id: &'a str,
    pub peer_id: &'a str,
    pub direction: ConsentDirection,
    pub enabled: bool,
    pub expected_generation: Option<&'a str>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ConsentSaved {
    Saved(AgentDelegationEdge),
    Missing,
    Invalid,
    Conflict,
    Unavailable,
}

fn edge(row: sqlx::sqlite::SqliteRow) -> Result<AgentDelegationEdge, DbError> {
    Ok(AgentDelegationEdge {
        caller_agent_id: row.try_get("caller_agent_id")?,
        receiver_agent_id: row.try_get("receiver_agent_id")?,
        outbound_enabled: row.try_get("outbound_enabled")?,
        inbound_enabled: row.try_get("inbound_enabled")?,
        generation: row.try_get("generation")?,
        updated_at: parse_ts(row.try_get("updated_at")?, "updated_at")?,
    })
}

const OWNED_EDGES: &str = "SELECT e.* FROM agent_delegation_edges e
 JOIN agent_owners c ON c.principal_id = e.caller_agent_id
 JOIN agent_owners r ON r.principal_id = e.receiver_agent_id
 WHERE c.owner_user_id = ? AND r.owner_user_id = ?";

pub async fn list_for_agent(
    pool: &Pool,
    owner_user_id: &str,
    agent_id: &str,
) -> Result<Vec<AgentDelegationEdge>, DbError> {
    sqlx::query(&format!("{OWNED_EDGES} AND (e.caller_agent_id = ? OR e.receiver_agent_id = ?) ORDER BY e.caller_agent_id, e.receiver_agent_id"))
        .bind(owner_user_id).bind(owner_user_id).bind(agent_id).bind(agent_id)
        .fetch_all(pool).await?.into_iter().map(edge).collect()
}

/// The caller's cross-boundary consent edges: the caller is the user's
/// personal agent, the receiver an organization agent.
pub async fn list_cross_boundary_for_caller(
    pool: &Pool,
    owner_user_id: &str,
    caller_agent_id: &str,
) -> Result<Vec<AgentDelegationEdge>, DbError> {
    sqlx::query(
        "SELECT e.* FROM agent_delegation_edges e
         JOIN agent_owners c ON c.principal_id = e.caller_agent_id
         JOIN agent_owners r ON r.principal_id = e.receiver_agent_id
         WHERE c.owner_user_id = ? AND c.organization_id IS NULL
           AND r.owner_user_id IS NULL AND r.organization_id IS NOT NULL
           AND e.caller_agent_id = ? ORDER BY r.name, e.receiver_agent_id",
    )
    .bind(owner_user_id)
    .bind(caller_agent_id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(edge)
    .collect()
}

/// A personal caller agent on one of the receiver's cross-boundary edges, with
/// the caller's owner, for the organization side's **Accepts requests from**
/// list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundCaller {
    pub caller_agent_id: String,
    pub agent_name: String,
    pub owner_user_id: String,
    pub owner_name: Option<String>,
    pub active: bool,
}

/// The personal caller agents on the receiver's cross-boundary edges — granted
/// and revoked alike, with their consent state — for the organization agent's
/// configuration surface.
pub async fn inbound_callers_for(
    pool: &Pool,
    receiver_agent_id: &str,
) -> Result<Vec<(InboundCaller, AgentDelegationEdge)>, DbError> {
    let rows = sqlx::query(
        "SELECT c.principal_id AS caller_agent_id, c.name AS agent_name, c.owner_user_id,
                u.name AS owner_name, c.active, e.*
         FROM agent_delegation_edges e
         JOIN agent_owners c ON c.principal_id = e.caller_agent_id
         LEFT JOIN users u ON u.id = c.owner_user_id
         WHERE e.receiver_agent_id = ?
           AND c.owner_user_id IS NOT NULL AND c.organization_id IS NULL
         ORDER BY c.name, c.principal_id",
    )
    .bind(receiver_agent_id)
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|row| {
            let caller = InboundCaller {
                caller_agent_id: row.try_get("caller_agent_id")?,
                agent_name: row.try_get("agent_name")?,
                owner_user_id: row.try_get("owner_user_id")?,
                owner_name: row.try_get("owner_name")?,
                active: row.try_get("active")?,
            };
            Ok((caller, edge(row)?))
        })
        .collect()
}

pub async fn authorized(
    pool: &Pool,
    owner_user_id: &str,
    caller_agent_id: &str,
    receiver_agent_id: &str,
    expected_generation: &str,
) -> Result<Option<AgentDelegationEdge>, DbError> {
    let mut connection = pool.acquire().await?;
    authorized_in(
        &mut connection,
        owner_user_id,
        caller_agent_id,
        receiver_agent_id,
        expected_generation,
    )
    .await
}

pub async fn authorized_in(
    connection: &mut sqlx::SqliteConnection,
    owner_user_id: &str,
    caller_agent_id: &str,
    receiver_agent_id: &str,
    expected_generation: &str,
) -> Result<Option<AgentDelegationEdge>, DbError> {
    sqlx::query(&format!("{OWNED_EDGES} AND e.caller_agent_id = ? AND e.receiver_agent_id = ? AND c.active = 1 AND r.active = 1 AND e.outbound_enabled = 1 AND e.inbound_enabled = 1 AND e.generation = ?"))
        .bind(owner_user_id).bind(owner_user_id).bind(caller_agent_id).bind(receiver_agent_id).bind(expected_generation)
        .fetch_optional(connection).await?.map(edge).transpose()
}

/// The consent edge of an ordered agent pair, without the owner-type filter:
/// the caller and the receiver may live with different owners.
pub async fn edge_between_in(
    connection: &mut sqlx::SqliteConnection,
    caller_agent_id: &str,
    receiver_agent_id: &str,
) -> Result<Option<AgentDelegationEdge>, DbError> {
    sqlx::query(
        "SELECT e.* FROM agent_delegation_edges e WHERE e.caller_agent_id = ? AND e.receiver_agent_id = ?",
    )
    .bind(caller_agent_id)
    .bind(receiver_agent_id)
    .fetch_optional(connection)
    .await?
    .map(edge)
    .transpose()
}

/// Live check for a personal caller delegating to an organization receiver:
/// both flags set, generation match, both agents active, the caller is a
/// user-owned agent, the receiver is an organization-owned agent, and the caller
/// user is a current member of the receiver's organization.
pub async fn authorized_cross_boundary_in(
    connection: &mut sqlx::SqliteConnection,
    caller_owner_user_id: &str,
    caller_agent_id: &str,
    receiver_org_id: &str,
    receiver_agent_id: &str,
    expected_generation: &str,
    caller_role_ids: &[String],
) -> Result<Option<AgentDelegationEdge>, DbError> {
    sqlx::query(
        "SELECT e.* FROM agent_delegation_edges e
         JOIN agent_owners c ON c.principal_id = e.caller_agent_id
         JOIN agent_owners r ON r.principal_id = e.receiver_agent_id
         WHERE e.caller_agent_id = ? AND e.receiver_agent_id = ?
           AND c.owner_user_id = ? AND c.organization_id IS NULL AND c.active = 1
           AND r.organization_id = ? AND r.owner_user_id IS NULL AND r.active = 1
           AND e.outbound_enabled = 1 AND e.inbound_enabled = 1 AND e.generation = ?
           AND EXISTS (
               SELECT 1 FROM organization_groups b
               JOIN gateway_groups g ON g.name = b.gateway_group
               WHERE b.organization_id = r.organization_id
                 AND g.name IN (SELECT value FROM json_each(?))
           )",
    )
    .bind(caller_agent_id)
    .bind(receiver_agent_id)
    .bind(caller_owner_user_id)
    .bind(receiver_org_id)
    .bind(expected_generation)
    .bind(serde_json::to_string(caller_role_ids).expect("string vector serializes"))
    .fetch_optional(connection)
    .await?
    .map(edge)
    .transpose()
}

/// A cross-boundary consent edit. The caller is a user-owned personal agent and
/// the receiver is an organization-owned agent. The actor is the person making
/// the change (the personal owner for `Outbound`, an organization member
/// writer for `Inbound`); the per-side authority is enforced by the caller.
pub struct CrossBoundaryConsentEdit<'a> {
    pub actor_user_id: &'a str,
    pub caller_agent_id: &'a str,
    pub receiver_agent_id: &'a str,
    pub direction: ConsentDirection,
    pub enabled: bool,
    pub expected_generation: Option<&'a str>,
}

pub async fn set_cross_boundary_consent(
    pool: &Pool,
    edit: CrossBoundaryConsentEdit<'_>,
) -> Result<ConsentSaved, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM agent_owners WHERE principal_id = ?)
           AND EXISTS(SELECT 1 FROM agent_owners WHERE principal_id = ?)",
    )
    .bind(edit.caller_agent_id)
    .bind(edit.receiver_agent_id)
    .fetch_one(&mut *tx)
    .await?;
    if !exists {
        return Ok(ConsentSaved::Missing);
    }
    let valid_pair: bool = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM agent_owners c JOIN agent_owners r ON r.principal_id = ?
            WHERE c.principal_id = ?
              AND c.owner_user_id IS NOT NULL AND c.organization_id IS NULL
              AND r.organization_id IS NOT NULL AND r.owner_user_id IS NULL
        )",
    )
    .bind(edit.receiver_agent_id)
    .bind(edit.caller_agent_id)
    .fetch_one(&mut *tx)
    .await?;
    if !valid_pair {
        return Ok(ConsentSaved::Invalid);
    }
    let current = sqlx::query(
        "SELECT * FROM agent_delegation_edges WHERE caller_agent_id = ? AND receiver_agent_id = ?",
    )
    .bind(edit.caller_agent_id)
    .bind(edit.receiver_agent_id)
    .fetch_optional(&mut *tx)
    .await?
    .map(edge)
    .transpose()?;
    if current.as_ref().map(|value| value.generation.as_str()) != edit.expected_generation {
        return Ok(ConsentSaved::Conflict);
    }
    let (caller_active, receiver_active): (bool, bool) =
        sqlx::query_as("SELECT c.active = 1, r.active = 1 FROM agent_owners c JOIN agent_owners r ON r.principal_id = ? WHERE c.principal_id = ?")
            .bind(edit.receiver_agent_id)
            .bind(edit.caller_agent_id)
            .fetch_one(&mut *tx)
            .await?;
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
    if edit.enabled && (!caller_active || !receiver_active) {
        return Ok(ConsentSaved::Unavailable);
    }
    let mut updated = current.unwrap_or_else(|| AgentDelegationEdge {
        caller_agent_id: edit.caller_agent_id.into(),
        receiver_agent_id: edit.receiver_agent_id.into(),
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
    sqlx::query("INSERT INTO agent_delegation_edges (caller_agent_id, receiver_agent_id, outbound_enabled, inbound_enabled, generation, updated_at) VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT(caller_agent_id, receiver_agent_id) DO UPDATE SET outbound_enabled = excluded.outbound_enabled, inbound_enabled = excluded.inbound_enabled, generation = excluded.generation, updated_at = excluded.updated_at")
        .bind(edit.caller_agent_id).bind(edit.receiver_agent_id).bind(updated.outbound_enabled).bind(updated.inbound_enabled)
        .bind(&updated.generation).bind(updated.updated_at.to_string()).execute(&mut *tx).await?;
    agent_audit::append(&mut tx, agent_audit::NewEvent::new(AuditKind::PersonalAgentDelegationChanged, edit.caller_agent_id, json!({
        "caller_agent_id": edit.caller_agent_id, "receiver_agent_id": edit.receiver_agent_id,
        "direction": match edit.direction { ConsentDirection::Outbound => "outbound", ConsentDirection::Inbound => "inbound" },
        "outbound_enabled": updated.outbound_enabled, "inbound_enabled": updated.inbound_enabled,
        "generation": updated.generation, "cross_boundary": true,
    })).by(Some(edit.actor_user_id))).await?;
    tx.commit().await?;
    Ok(ConsentSaved::Saved(updated))
}

pub async fn set_consent(pool: &Pool, edit: ConsentEdit<'_>) -> Result<ConsentSaved, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let active: Option<(bool, bool)> = sqlx::query_as("SELECT d.active, p.active FROM agent_owners d JOIN agent_owners p ON p.principal_id = ? WHERE d.principal_id = ? AND d.owner_user_id = ? AND p.owner_user_id = ?")
        .bind(edit.peer_id).bind(edit.agent_id).bind(edit.owner_user_id).bind(edit.owner_user_id)
        .fetch_optional(&mut *tx).await?;
    let Some((owned_agent_active, peer_active)) = active else {
        return Ok(ConsentSaved::Missing);
    };
    if edit.agent_id == edit.peer_id {
        return Ok(ConsentSaved::Invalid);
    }
    let (caller, receiver) = match edit.direction {
        ConsentDirection::Outbound => (edit.agent_id, edit.peer_id),
        ConsentDirection::Inbound => (edit.peer_id, edit.agent_id),
    };
    let current = sqlx::query(
        "SELECT * FROM agent_delegation_edges WHERE caller_agent_id = ? AND receiver_agent_id = ?",
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
    if edit.enabled && (!owned_agent_active || !peer_active) {
        return Ok(ConsentSaved::Unavailable);
    }
    let mut updated = current.unwrap_or_else(|| AgentDelegationEdge {
        caller_agent_id: caller.into(),
        receiver_agent_id: receiver.into(),
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
    sqlx::query("INSERT INTO agent_delegation_edges (caller_agent_id, receiver_agent_id, outbound_enabled, inbound_enabled, generation, updated_at) VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT(caller_agent_id, receiver_agent_id) DO UPDATE SET outbound_enabled = excluded.outbound_enabled, inbound_enabled = excluded.inbound_enabled, generation = excluded.generation, updated_at = excluded.updated_at")
        .bind(caller).bind(receiver).bind(updated.outbound_enabled).bind(updated.inbound_enabled)
        .bind(&updated.generation).bind(updated.updated_at.to_string()).execute(&mut *tx).await?;
    agent_audit::append(&mut tx, agent_audit::NewEvent::new(AuditKind::PersonalAgentDelegationChanged, edit.agent_id, json!({
        "caller_agent_id": caller, "receiver_agent_id": receiver,
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
            let (_, owned_agent) = agents::create_personal_agent(
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
            ids.push(owned_agent.unwrap().principal.id);
        }
        (pool, ids.remove(0), ids.remove(0), ids.remove(0))
    }

    #[tokio::test]
    async fn directed_consent_schema_defaults_to_denied_and_rejects_self_edges() {
        let (pool, caller, receiver, _) = fixture().await;
        sqlx::query("INSERT INTO agent_delegation_edges (caller_agent_id, receiver_agent_id, generation, updated_at) VALUES (?, ?, ?, ?)")
            .bind(&caller).bind(&receiver).bind(uuid::Uuid::new_v4().to_string())
            .bind(Timestamp::now().to_string()).execute(&pool).await.unwrap();
        let flags: (bool, bool) = sqlx::query_as("SELECT outbound_enabled, inbound_enabled FROM agent_delegation_edges WHERE caller_agent_id = ? AND receiver_agent_id = ?")
            .bind(&caller).bind(&receiver).fetch_one(&pool).await.unwrap();
        assert_eq!(flags, (false, false));
        assert!(sqlx::query("INSERT INTO agent_delegation_edges (caller_agent_id, receiver_agent_id, generation, updated_at) VALUES (?, ?, ?, ?)")
            .bind(&caller).bind(&caller).bind(uuid::Uuid::new_v4().to_string())
            .bind(Timestamp::now().to_string()).execute(&pool).await.is_err());
        assert!(sqlx::query("INSERT INTO agent_delegation_edges (caller_agent_id, receiver_agent_id, generation, updated_at) VALUES (?, ?, ?, ?)")
            .bind(&caller).bind(&receiver).bind(uuid::Uuid::new_v4().to_string())
            .bind(Timestamp::now().to_string()).execute(&pool).await.is_err());
        sqlx::query("DELETE FROM agent_owners WHERE principal_id = ?")
            .bind(&receiver)
            .execute(&pool)
            .await
            .unwrap();
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_delegation_edges")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    async fn edit(
        pool: &Pool,
        owned_agent: &str,
        peer: &str,
        direction: ConsentDirection,
        enabled: bool,
        generation: Option<&str>,
    ) -> ConsentSaved {
        set_consent(
            pool,
            ConsentEdit {
                owner_user_id: "alice",
                agent_id: owned_agent,
                peer_id: peer,
                direction,
                enabled,
                expected_generation: generation,
            },
        )
        .await
        .unwrap()
    }

    fn saved(result: ConsentSaved) -> AgentDelegationEdge {
        match result {
            ConsentSaved::Saved(edge) => edge,
            other => panic!("expected saved consent, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn directed_consent_requires_both_decisions_and_exact_live_generation() {
        let (pool, caller, receiver, _) = fixture().await;
        assert!(
            list_for_agent(&pool, "alice", &caller)
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
        let edges = list_for_agent(&pool, "alice", &receiver).await.unwrap();
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
            "SELECT COUNT(*) FROM agent_audit WHERE kind = 'personal_agent_delegation_changed'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(initial_audits, 1);
        let (actor, detail): (String, String) = sqlx::query_as("SELECT actor_id, detail FROM agent_audit WHERE kind = 'personal_agent_delegation_changed'").fetch_one(&pool).await.unwrap();
        assert_eq!(actor, "alice");
        let detail: serde_json::Value = serde_json::from_str(&detail).unwrap();
        assert_eq!(detail["caller_agent_id"], caller);
        assert_eq!(detail["receiver_agent_id"], receiver);
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
            "SELECT COUNT(*) FROM agent_audit WHERE kind = 'personal_agent_delegation_changed'",
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
        agents::set_personal_agent_active(&pool, &receiver, "alice", false, 4)
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
            list_for_agent(&pool, "bob", &caller)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            list_for_agent(&pool, "alice", &caller).await.unwrap(),
            vec![revoked]
        );
        sqlx::query("DELETE FROM agent_owners WHERE principal_id = ?")
            .bind(&receiver)
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            list_for_agent(&pool, "alice", &caller)
                .await
                .unwrap()
                .is_empty()
        );
        let audit_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM agent_audit WHERE kind = 'personal_agent_delegation_changed'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(audit_count, 3);
    }

    async fn cross_boundary_fixture() -> (Pool, String, String, String) {
        use crate::db::{WriteTx, organizations, organizations::NewOrganization};
        let pool = aiplane_core::server::db::open(Path::new(":memory:"))
            .await
            .unwrap();
        let now = Timestamp::now();
        for (id, roles) in [
            ("alice", vec!["engineering".to_owned()]),
            ("bob", vec!["engineering".to_owned()]),
        ] {
            users::upsert(
                &pool,
                &User {
                    id: id.into(),
                    email: format!("{id}@example.com"),
                    name: None,
                    roles,
                    created_at: now,
                    updated_at: now,
                    timezone: None,
                    speech_voice: None,
                },
            )
            .await
            .unwrap();
        }
        sqlx::query(
            "INSERT INTO gateway_groups(name,created_at,updated_at) VALUES('engineering', ?, ?)",
        )
        .bind(now.to_string())
        .bind(now.to_string())
        .execute(&pool)
        .await
        .unwrap();
        let org = match organizations::create(
            &pool,
            &NewOrganization {
                name: "Acme",
                description: "Support",
                member_groups: &["engineering".to_owned()],
            },
            "bob",
        )
        .await
        .unwrap()
        {
            organizations::OrganizationCreateResult::Created(row) => row,
            _ => panic!("organization created"),
        };
        let org_id = org.organization.id;
        let (_, caller) = agents::create_personal_agent(
            &pool,
            &NewPrincipal {
                name: "caller",
                display: "Caller",
                description: "",
            },
            r#"{"main":{}}"#,
            "alice",
            4,
        )
        .await
        .unwrap();
        let caller = caller.unwrap().principal.id;
        let mut tx = WriteTx::begin(&pool).await.unwrap();
        let (_, receiver) = agents::create_organization_agent_in(
            &mut tx,
            &org_id,
            &NewPrincipal {
                name: "ticket",
                display: "Ticket",
                description: "",
            },
            r#"{"main":{}}"#,
            "bob",
            4,
        )
        .await
        .unwrap();
        let receiver = receiver.unwrap().principal.id;
        tx.commit().await.unwrap();
        (pool, caller, receiver, org_id)
    }

    #[tokio::test]
    async fn edge_between_reads_the_ordered_pair_across_owner_boundaries() {
        let (pool, caller, receiver, _) = cross_boundary_fixture().await;
        let mut conn = pool.acquire().await.unwrap();
        assert!(
            edge_between_in(&mut conn, &caller, &receiver)
                .await
                .unwrap()
                .is_none()
        );
        let first = saved(
            set_cross_boundary_consent(
                &pool,
                cb_edit(
                    &caller,
                    &receiver,
                    "alice",
                    ConsentDirection::Outbound,
                    true,
                    None,
                ),
            )
            .await
            .unwrap(),
        );
        let mut conn = pool.acquire().await.unwrap();
        let edge = edge_between_in(&mut conn, &caller, &receiver)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(edge, first);
        assert!(
            edge_between_in(&mut conn, &receiver, &caller)
                .await
                .unwrap()
                .is_none()
        );
    }

    fn cb_edit<'a>(
        caller: &'a str,
        receiver: &'a str,
        actor: &'a str,
        direction: ConsentDirection,
        enabled: bool,
        generation: Option<&'a str>,
    ) -> CrossBoundaryConsentEdit<'a> {
        CrossBoundaryConsentEdit {
            actor_user_id: actor,
            caller_agent_id: caller,
            receiver_agent_id: receiver,
            direction,
            enabled,
            expected_generation: generation,
        }
    }

    #[tokio::test]
    async fn cross_boundary_consent_requires_both_decisions_and_membership() {
        let (pool, caller, receiver, org_id) = cross_boundary_fixture().await;
        let mut conn = pool.acquire().await.unwrap();
        assert!(
            authorized_cross_boundary_in(
                &mut conn,
                "alice",
                &caller,
                &org_id,
                &receiver,
                "absent",
                &["engineering".to_owned()],
            )
            .await
            .unwrap()
            .is_none()
        );
        let first = saved(
            set_cross_boundary_consent(
                &pool,
                cb_edit(
                    &caller,
                    &receiver,
                    "alice",
                    ConsentDirection::Outbound,
                    true,
                    None,
                ),
            )
            .await
            .unwrap(),
        );
        assert!(first.outbound_enabled && !first.inbound_enabled);
        assert!(
            authorized_cross_boundary_in(
                &mut conn,
                "alice",
                &caller,
                &org_id,
                &receiver,
                &first.generation,
                &["engineering".to_owned()],
            )
            .await
            .unwrap()
            .is_none()
        );
        drop(conn);
        let both = saved(
            set_cross_boundary_consent(
                &pool,
                cb_edit(
                    &caller,
                    &receiver,
                    "bob",
                    ConsentDirection::Inbound,
                    true,
                    Some(&first.generation),
                ),
            )
            .await
            .unwrap(),
        );
        assert!(both.outbound_enabled && both.inbound_enabled);
        assert_ne!(first.generation, both.generation);
        let mut conn = pool.acquire().await.unwrap();
        assert!(
            authorized_cross_boundary_in(
                &mut conn,
                "alice",
                &caller,
                &org_id,
                &receiver,
                &both.generation,
                &["engineering".to_owned()],
            )
            .await
            .unwrap()
            .is_some()
        );
        assert!(
            authorized_cross_boundary_in(
                &mut conn,
                "alice",
                &caller,
                &org_id,
                &receiver,
                &first.generation,
                &["engineering".to_owned()],
            )
            .await
            .unwrap()
            .is_none()
        );
        assert!(
            authorized_cross_boundary_in(
                &mut conn,
                "alice",
                &caller,
                "missing-org",
                &receiver,
                &both.generation,
                &["engineering".to_owned()],
            )
            .await
            .unwrap()
            .is_none()
        );
        drop(conn);
        let now = Timestamp::now();
        users::upsert(
            &pool,
            &User {
                id: "carol".into(),
                email: "carol@example.com".into(),
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
        let (_, carol_agent) = agents::create_personal_agent(
            &pool,
            &NewPrincipal {
                name: "carol",
                display: "Carol",
                description: "",
            },
            r#"{"main":{}}"#,
            "carol",
            4,
        )
        .await
        .unwrap();
        let carol_agent = carol_agent.unwrap().principal.id;
        let carol_out = saved(
            set_cross_boundary_consent(
                &pool,
                cb_edit(
                    &carol_agent,
                    &receiver,
                    "carol",
                    ConsentDirection::Outbound,
                    true,
                    None,
                ),
            )
            .await
            .unwrap(),
        );
        saved(
            set_cross_boundary_consent(
                &pool,
                cb_edit(
                    &carol_agent,
                    &receiver,
                    "bob",
                    ConsentDirection::Inbound,
                    true,
                    Some(&carol_out.generation),
                ),
            )
            .await
            .unwrap(),
        );
        let mut conn = pool.acquire().await.unwrap();
        assert!(
            authorized_cross_boundary_in(
                &mut conn,
                "carol",
                &carol_agent,
                &org_id,
                &receiver,
                &carol_out.generation,
                &[],
            )
            .await
            .unwrap()
            .is_none()
        );
    }

    #[tokio::test]
    async fn cross_boundary_consent_enforces_owner_types_and_active_state() {
        let (pool, caller, receiver, _org_id) = cross_boundary_fixture().await;
        let (_, personal_receiver) = agents::create_personal_agent(
            &pool,
            &NewPrincipal {
                name: "personal",
                display: "Personal",
                description: "",
            },
            r#"{"main":{}}"#,
            "alice",
            4,
        )
        .await
        .unwrap();
        let personal_receiver = personal_receiver.unwrap().principal.id;
        assert!(matches!(
            set_cross_boundary_consent(
                &pool,
                cb_edit(
                    &caller,
                    &personal_receiver,
                    "alice",
                    ConsentDirection::Outbound,
                    true,
                    None
                )
            )
            .await,
            Ok(ConsentSaved::Invalid)
        ));
        assert!(matches!(
            set_cross_boundary_consent(
                &pool,
                cb_edit(
                    &receiver,
                    &caller,
                    "bob",
                    ConsentDirection::Outbound,
                    true,
                    None
                )
            )
            .await,
            Ok(ConsentSaved::Invalid)
        ));
        agents::set_personal_agent_active(&pool, &caller, "alice", false, 4)
            .await
            .unwrap();
        assert!(matches!(
            set_cross_boundary_consent(
                &pool,
                cb_edit(
                    &caller,
                    &receiver,
                    "alice",
                    ConsentDirection::Outbound,
                    true,
                    None
                )
            )
            .await,
            Ok(ConsentSaved::Unavailable)
        ));
    }

    #[tokio::test]
    async fn cross_boundary_listing_is_owner_and_caller_scoped() {
        let (pool, caller, receiver, _) = cross_boundary_fixture().await;
        assert!(
            list_cross_boundary_for_caller(&pool, "alice", &caller)
                .await
                .unwrap()
                .is_empty()
        );
        let first = saved(
            set_cross_boundary_consent(
                &pool,
                cb_edit(
                    &caller,
                    &receiver,
                    "alice",
                    ConsentDirection::Outbound,
                    true,
                    None,
                ),
            )
            .await
            .unwrap(),
        );
        saved(
            set_cross_boundary_consent(
                &pool,
                cb_edit(
                    &caller,
                    &receiver,
                    "bob",
                    ConsentDirection::Inbound,
                    true,
                    Some(&first.generation),
                ),
            )
            .await
            .unwrap(),
        );

        let edges = list_cross_boundary_for_caller(&pool, "alice", &caller)
            .await
            .unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(
            (
                edges[0].caller_agent_id.as_str(),
                edges[0].receiver_agent_id.as_str()
            ),
            (caller.as_str(), receiver.as_str())
        );
        assert!(edges[0].outbound_enabled && edges[0].inbound_enabled);

        assert!(
            list_cross_boundary_for_caller(&pool, "bob", &caller)
                .await
                .unwrap()
                .is_empty()
        );

        let (_, other) = agents::create_personal_agent(
            &pool,
            &NewPrincipal {
                name: "second",
                display: "Second",
                description: "",
            },
            r#"{"main":{}}"#,
            "alice",
            4,
        )
        .await
        .unwrap();
        let other = other.unwrap().principal.id;
        set_consent(
            &pool,
            ConsentEdit {
                owner_user_id: "alice",
                agent_id: &caller,
                peer_id: &other,
                direction: ConsentDirection::Outbound,
                enabled: true,
                expected_generation: None,
            },
        )
        .await
        .unwrap();
        let edges = list_cross_boundary_for_caller(&pool, "alice", &caller)
            .await
            .unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].receiver_agent_id, receiver);
    }

    #[tokio::test]
    async fn inbound_callers_list_the_personal_callers_with_their_inbound_state() {
        let (pool, caller, receiver, _) = cross_boundary_fixture().await;
        assert!(
            inbound_callers_for(&pool, &receiver)
                .await
                .unwrap()
                .is_empty()
        );
        let first = saved(
            set_cross_boundary_consent(
                &pool,
                cb_edit(
                    &caller,
                    &receiver,
                    "alice",
                    ConsentDirection::Outbound,
                    true,
                    None,
                ),
            )
            .await
            .unwrap(),
        );
        // The edge exists, so the caller is listed with inbound disabled.
        let entries = inbound_callers_for(&pool, &receiver).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert!(!entries[0].1.inbound_enabled);
        let both = saved(
            set_cross_boundary_consent(
                &pool,
                cb_edit(
                    &caller,
                    &receiver,
                    "bob",
                    ConsentDirection::Inbound,
                    true,
                    Some(&first.generation),
                ),
            )
            .await
            .unwrap(),
        );

        let entries = inbound_callers_for(&pool, &receiver).await.unwrap();
        assert_eq!(entries.len(), 1);
        let (info, edge) = &entries[0];
        assert_eq!(info.caller_agent_id, caller);
        assert_eq!(info.agent_name, "caller");
        assert_eq!(info.owner_user_id, "alice");
        assert!(info.active);
        assert!(edge.inbound_enabled);

        saved(
            set_cross_boundary_consent(
                &pool,
                cb_edit(
                    &caller,
                    &receiver,
                    "bob",
                    ConsentDirection::Inbound,
                    false,
                    Some(&both.generation),
                ),
            )
            .await
            .unwrap(),
        );
        // A revoked grant stays listed, disabled, so the organization side can
        // re-enable it.
        let entries = inbound_callers_for(&pool, &receiver).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert!(!entries[0].1.inbound_enabled);
    }
}
