// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The cross-boundary delegation link: a personal caller agent delegating to an
//! organization receiver agent. The child is a principal session (owned by the
//! receiver's principal), so it is not a row in `personal_agent_sessions`. This
//! table records the facts that connect it to the personal parent — the caller
//! agent, the parent turn and tool-call, and the consent edge generation — so
//! resume and cancellation can re-validate the chain. `session-core` stays
//! owner-agnostic; only this agent-layer module names the cross-boundary pair.

use jiff::Timestamp;
use sqlx::Row;

use super::{DbError, Pool, WriteTx, agent_delegation};

/// The immutable link between a personal caller and an organization receiver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrossBoundaryChildLink {
    pub child_session_id: String,
    pub caller_agent_id: String,
    pub receiver_agent_id: String,
    pub caller_owner_user_id: String,
    pub receiver_org_id: String,
    pub parent_turn_id: String,
    pub parent_tool_call_id: String,
    pub edge_generation: String,
}

#[derive(Debug, Clone, Copy)]
pub struct NewCrossBoundaryChild<'a> {
    pub child_session_id: &'a str,
    pub caller_agent_id: &'a str,
    pub receiver_agent_id: &'a str,
    pub caller_owner_user_id: &'a str,
    /// The caller owner's role IDs resolved against the current RBAC
    /// snapshot, for the live membership recheck.
    pub caller_role_ids: &'a [String],
    pub receiver_org_id: &'a str,
    pub parent_turn_id: &'a str,
    pub parent_tool_call_id: &'a str,
    pub edge_generation: &'a str,
}

/// Record the cross-boundary child link for an already-opened principal child
/// session. Returns `None` unless the child is a principal session owned by the
/// receiver, the owner types are personal-caller / organization-receiver, and
/// the consent edge is currently authorized.
pub async fn record_in(
    tx: &mut WriteTx,
    new: &NewCrossBoundaryChild<'_>,
) -> Result<Option<CrossBoundaryChildLink>, DbError> {
    let valid: bool = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM chat_sessions s
            JOIN agent_owners r ON r.principal_id = s.principal_id AND r.organization_id = ? AND r.owner_user_id IS NULL
            JOIN agent_owners c ON c.principal_id = ? AND c.owner_user_id = ? AND c.organization_id IS NULL
            WHERE s.id = ? AND s.user_id IS NULL
        )",
    )
    .bind(new.receiver_org_id)
    .bind(new.caller_agent_id)
    .bind(new.caller_owner_user_id)
    .bind(new.child_session_id)
    .fetch_one(&mut **tx)
    .await?;
    if !valid {
        return Ok(None);
    }
    if agent_delegation::authorized_cross_boundary_in(
        tx,
        new.caller_owner_user_id,
        new.caller_agent_id,
        new.receiver_org_id,
        new.receiver_agent_id,
        new.edge_generation,
        new.caller_role_ids,
    )
    .await?
    .is_none()
    {
        return Ok(None);
    }
    sqlx::query(
        "INSERT INTO cross_boundary_delegations (child_session_id, caller_agent_id, receiver_agent_id, \
         caller_owner_user_id, receiver_org_id, parent_turn_id, parent_tool_call_id, edge_generation, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(new.child_session_id)
    .bind(new.caller_agent_id)
    .bind(new.receiver_agent_id)
    .bind(new.caller_owner_user_id)
    .bind(new.receiver_org_id)
    .bind(new.parent_turn_id)
    .bind(new.parent_tool_call_id)
    .bind(new.edge_generation)
    .bind(Timestamp::now().to_string())
    .execute(&mut **tx)
    .await?;
    Ok(Some(CrossBoundaryChildLink {
        child_session_id: new.child_session_id.into(),
        caller_agent_id: new.caller_agent_id.into(),
        receiver_agent_id: new.receiver_agent_id.into(),
        caller_owner_user_id: new.caller_owner_user_id.into(),
        receiver_org_id: new.receiver_org_id.into(),
        parent_turn_id: new.parent_turn_id.into(),
        parent_tool_call_id: new.parent_tool_call_id.into(),
        edge_generation: new.edge_generation.into(),
    }))
}

pub async fn record(
    pool: &Pool,
    new: &NewCrossBoundaryChild<'_>,
) -> Result<Option<CrossBoundaryChildLink>, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let link = record_in(&mut tx, new).await?;
    tx.commit().await?;
    Ok(link)
}

/// The cross-boundary link recorded for a principal child session, if any.
pub async fn link_of_session(
    pool: &Pool,
    child_session_id: &str,
) -> Result<Option<CrossBoundaryChildLink>, DbError> {
    let mut connection = pool.acquire().await?;
    link_of_session_in(&mut connection, child_session_id).await
}

pub async fn link_of_session_in(
    connection: &mut sqlx::SqliteConnection,
    child_session_id: &str,
) -> Result<Option<CrossBoundaryChildLink>, DbError> {
    let row = sqlx::query(
        "SELECT child_session_id, caller_agent_id, receiver_agent_id, caller_owner_user_id, \
         receiver_org_id, parent_turn_id, parent_tool_call_id, edge_generation \
         FROM cross_boundary_delegations WHERE child_session_id = ?",
    )
    .bind(child_session_id)
    .fetch_optional(connection)
    .await?;
    row.map(|row| {
        Ok(CrossBoundaryChildLink {
            child_session_id: row.try_get("child_session_id")?,
            caller_agent_id: row.try_get("caller_agent_id")?,
            receiver_agent_id: row.try_get("receiver_agent_id")?,
            caller_owner_user_id: row.try_get("caller_owner_user_id")?,
            receiver_org_id: row.try_get("receiver_org_id")?,
            parent_turn_id: row.try_get("parent_turn_id")?,
            parent_tool_call_id: row.try_get("parent_tool_call_id")?,
            edge_generation: row.try_get("edge_generation")?,
        })
    })
    .transpose()
}

/// The cross-boundary children of a personal parent turn, for cancelling the
/// linked organization work when the caller's own run is cancelled.
pub async fn links_for_parent_turn(
    connection: &mut sqlx::SqliteConnection,
    parent_turn_id: &str,
) -> Result<Vec<CrossBoundaryChildLink>, DbError> {
    sqlx::query(
        "SELECT child_session_id, caller_agent_id, receiver_agent_id, caller_owner_user_id, \
         receiver_org_id, parent_turn_id, parent_tool_call_id, edge_generation \
         FROM cross_boundary_delegations WHERE parent_turn_id = ? ORDER BY child_session_id",
    )
    .bind(parent_turn_id)
    .fetch_all(connection)
    .await?
    .into_iter()
    .map(|row| {
        Ok(CrossBoundaryChildLink {
            child_session_id: row.try_get("child_session_id")?,
            caller_agent_id: row.try_get("caller_agent_id")?,
            receiver_agent_id: row.try_get("receiver_agent_id")?,
            caller_owner_user_id: row.try_get("caller_owner_user_id")?,
            receiver_org_id: row.try_get("receiver_org_id")?,
            parent_turn_id: row.try_get("parent_turn_id")?,
            parent_tool_call_id: row.try_get("parent_tool_call_id")?,
            edge_generation: row.try_get("edge_generation")?,
        })
    })
    .collect()
}

/// The caller's active personal root turns whose cross-boundary child runs on
/// this consent edge at this generation: the roots to cancel when the edge
/// is revoked.
pub async fn active_roots_for_edge(
    pool: &Pool,
    caller_owner_user_id: &str,
    caller_agent_id: &str,
    receiver_agent_id: &str,
    edge_generation: &str,
) -> Result<Vec<String>, DbError> {
    Ok(sqlx::query_scalar(concat!(
        "SELECT DISTINCT cb.parent_turn_id
             FROM cross_boundary_delegations cb
             JOIN chat_turns t ON t.id = cb.parent_turn_id
             JOIN chat_sessions s ON s.id = t.session_id
             WHERE t.role = 'assistant' AND t.status IN ('in_progress', 'suspended')
               AND s.user_id = ? AND s.principal_id IS NULL AND ",
        aiplane_core::unshared_sql!("s"),
        "
               AND cb.caller_owner_user_id = ? AND cb.caller_agent_id = ?
               AND cb.receiver_agent_id = ? AND cb.edge_generation = ?"
    ))
    .bind(caller_owner_user_id)
    .bind(caller_owner_user_id)
    .bind(caller_agent_id)
    .bind(receiver_agent_id)
    .bind(edge_generation)
    .fetch_all(pool)
    .await?)
}

/// Validate a cross-boundary child: the recorded link matches, the personal
/// parent turn is in-progress/suspended with a running `forward_request` in the
/// caller's own session, and the consent edge is currently authorized
/// (membership + both active + generation).
pub async fn validate_child_in(
    connection: &mut sqlx::SqliteConnection,
    new: &NewCrossBoundaryChild<'_>,
    expected_generation: &str,
) -> Result<bool, DbError> {
    let Some(link) = link_of_session_in(connection, new.child_session_id).await? else {
        return Ok(false);
    };
    if link.caller_agent_id != new.caller_agent_id
        || link.receiver_agent_id != new.receiver_agent_id
        || link.caller_owner_user_id != new.caller_owner_user_id
        || link.receiver_org_id != new.receiver_org_id
        || link.parent_turn_id != new.parent_turn_id
        || link.parent_tool_call_id != new.parent_tool_call_id
        || link.edge_generation != expected_generation
    {
        return Ok(false);
    }
    let parent_valid: bool = sqlx::query_scalar(concat!(
        "SELECT EXISTS(
        SELECT 1 FROM chat_turns t
        JOIN chat_sessions s ON s.id = t.session_id
        JOIN personal_agent_sessions d ON d.session_id = s.id
        JOIN chat_tool_calls c ON c.turn_id = t.id AND c.id = ?
        WHERE t.id = ? AND t.role = 'assistant' AND t.status IN ('in_progress', 'suspended')
          AND s.user_id = ? AND s.principal_id IS NULL AND ",
        aiplane_core::unshared_sql!("s"),
        "
          AND d.owner_user_id = ? AND d.agent_id = ?
          AND c.name = 'forward_request' AND c.status = 'running'
    )"
    ))
    .bind(new.parent_tool_call_id)
    .bind(new.parent_turn_id)
    .bind(new.caller_owner_user_id)
    .bind(new.caller_owner_user_id)
    .bind(new.caller_agent_id)
    .fetch_one(&mut *connection)
    .await?;
    if !parent_valid {
        return Ok(false);
    }
    Ok(agent_delegation::authorized_cross_boundary_in(
        connection,
        new.caller_owner_user_id,
        new.caller_agent_id,
        new.receiver_org_id,
        new.receiver_agent_id,
        expected_generation,
        new.caller_role_ids,
    )
    .await?
    .is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{
        WriteTx, agents,
        organizations::{self, NewOrganization},
        run_sessions::{self, NewRunSession},
        system_principals::NewPrincipal,
    };
    use aiplane_core::server::db::users::{self, User};
    use jiff::Timestamp;
    use std::path::Path;

    struct Fixture {
        pool: Pool,
        caller: String,
        receiver: String,
        org_id: String,
        generation: String,
        role_ids: Vec<String>,
    }

    async fn fixture() -> Fixture {
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

        let outbound = agent_delegation::set_cross_boundary_consent(
            &pool,
            agent_delegation::CrossBoundaryConsentEdit {
                actor_user_id: "alice",
                caller_agent_id: &caller,
                receiver_agent_id: &receiver,
                direction: agent_delegation::ConsentDirection::Outbound,
                enabled: true,
                expected_generation: None,
            },
        )
        .await
        .unwrap();
        let agent_delegation::ConsentSaved::Saved(outbound) = outbound else {
            panic!("outbound consent");
        };
        let both = agent_delegation::set_cross_boundary_consent(
            &pool,
            agent_delegation::CrossBoundaryConsentEdit {
                actor_user_id: "bob",
                caller_agent_id: &caller,
                receiver_agent_id: &receiver,
                direction: agent_delegation::ConsentDirection::Inbound,
                enabled: true,
                expected_generation: Some(&outbound.generation),
            },
        )
        .await
        .unwrap();
        let agent_delegation::ConsentSaved::Saved(both) = both else {
            panic!("inbound consent");
        };
        Fixture {
            pool,
            caller,
            receiver,
            org_id,
            generation: both.generation,
            role_ids: vec!["engineering".to_owned()],
        }
    }

    /// Alice's personal caller session with an in-progress assistant turn and a
    /// running `forward_request` tool call — the cross-boundary parent.
    async fn parent_turn(fx: &Fixture) -> (String, String) {
        let session = session_core::db::create_session(&fx.pool, "alice")
            .await
            .unwrap();
        agent_sessions_link(&fx.pool, &fx.caller, &session.id).await;
        session_core::db::create_assistant_turn_in_progress(
            &fx.pool,
            &session.id,
            "parent",
            "model",
        )
        .await
        .unwrap();
        session_core::db::insert_running_tool_call(
            &fx.pool,
            "parent",
            "forward",
            "forward_request",
            "{}",
        )
        .await
        .unwrap();
        (session.id, "parent".to_owned())
    }

    async fn agent_sessions_link(pool: &Pool, caller: &str, session_id: &str) {
        crate::db::agent_sessions::link(pool, "alice", caller, session_id)
            .await
            .unwrap();
    }

    /// Open the principal child session for the receiver and return its id.
    async fn principal_child(fx: &Fixture, parent_turn_id: &str) -> String {
        let mut tx = WriteTx::begin(&fx.pool).await.unwrap();
        let session = run_sessions::create_principal_session(
            &mut *tx,
            &NewRunSession {
                principal_id: &fx.receiver,
                title: Some("Ticket"),
                parent_turn_id: Some(parent_turn_id),
                agent_version: None,
            },
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        session.id
    }

    fn input<'a>(fx: &'a Fixture, child_session_id: &'a str) -> NewCrossBoundaryChild<'a> {
        NewCrossBoundaryChild {
            child_session_id,
            caller_agent_id: &fx.caller,
            receiver_agent_id: &fx.receiver,
            caller_owner_user_id: "alice",
            caller_role_ids: &fx.role_ids,
            receiver_org_id: &fx.org_id,
            parent_turn_id: "parent",
            parent_tool_call_id: "forward",
            edge_generation: &fx.generation,
        }
    }

    #[tokio::test]
    async fn record_requires_a_valid_principal_child_and_live_edge() {
        let fx = fixture().await;
        let (parent_session, parent_turn) = parent_turn(&fx).await;

        // A principal session owned by the receiver, with a live edge: records.
        let child = principal_child(&fx, &parent_turn).await;
        let mut tx = WriteTx::begin(&fx.pool).await.unwrap();
        let recorded = record_in(&mut tx, &input(&fx, &child)).await.unwrap();
        tx.commit().await.unwrap();
        assert!(recorded.is_some());

        // A personal (non-principal) session is not a cross-boundary child.
        let personal = session_core::db::create_session(&fx.pool, "alice")
            .await
            .unwrap();
        let mut tx = WriteTx::begin(&fx.pool).await.unwrap();
        let rejected = record_in(&mut tx, &input(&fx, &personal.id)).await.unwrap();
        tx.commit().await.unwrap();
        assert!(rejected.is_none());
        let _ = parent_session;
    }

    #[tokio::test]
    async fn record_refuses_a_stale_generation() {
        let fx = fixture().await;
        let (_parent_session, parent_turn) = parent_turn(&fx).await;
        let child = principal_child(&fx, &parent_turn).await;
        let mut stale = input(&fx, &child);
        stale.edge_generation = "stale";
        let mut tx = WriteTx::begin(&fx.pool).await.unwrap();
        let rejected = record_in(&mut tx, &stale).await.unwrap();
        tx.commit().await.unwrap();
        assert!(rejected.is_none());
    }

    #[tokio::test]
    async fn link_round_trips_through_the_table() {
        let fx = fixture().await;
        let (_parent_session, parent_turn) = parent_turn(&fx).await;
        let child = principal_child(&fx, &parent_turn).await;
        let recorded = record(&fx.pool, &input(&fx, &child))
            .await
            .unwrap()
            .unwrap();
        let mut conn = fx.pool.acquire().await.unwrap();
        let resolved = link_of_session_in(&mut conn, &child)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(resolved, recorded);
        assert!(
            link_of_session_in(&mut conn, "missing")
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn parent_turn_links_and_active_roots_follow_the_recorded_edge() {
        let fx = fixture().await;
        let (_parent_session, parent_turn) = parent_turn(&fx).await;
        let child = principal_child(&fx, &parent_turn).await;
        record(&fx.pool, &input(&fx, &child))
            .await
            .unwrap()
            .unwrap();

        let mut conn = fx.pool.acquire().await.unwrap();
        let links = links_for_parent_turn(&mut conn, &parent_turn)
            .await
            .unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].child_session_id, child);
        assert!(
            links_for_parent_turn(&mut conn, "no-such-turn")
                .await
                .unwrap()
                .is_empty()
        );
        drop(conn);

        // The caller's active root turn is the parent; a finished one is not.
        assert_eq!(
            active_roots_for_edge(&fx.pool, "alice", &fx.caller, &fx.receiver, &fx.generation)
                .await
                .unwrap(),
            vec![parent_turn.clone()]
        );
        sqlx::query("UPDATE chat_turns SET status = 'completed' WHERE id = ?")
            .bind(&parent_turn)
            .execute(&fx.pool)
            .await
            .unwrap();
        assert!(
            active_roots_for_edge(&fx.pool, "alice", &fx.caller, &fx.receiver, &fx.generation)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            active_roots_for_edge(&fx.pool, "alice", &fx.caller, &fx.receiver, "stale")
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn validate_requires_intact_link_valid_parent_and_live_edge() {
        let fx = fixture().await;
        let (_parent_session, parent_turn) = parent_turn(&fx).await;
        let child = principal_child(&fx, &parent_turn).await;
        record(&fx.pool, &input(&fx, &child))
            .await
            .unwrap()
            .unwrap();

        let mut conn = fx.pool.acquire().await.unwrap();
        assert!(
            validate_child_in(&mut conn, &input(&fx, &child), &fx.generation)
                .await
                .unwrap()
        );

        // A stale generation is not authorized.
        assert!(
            !validate_child_in(&mut conn, &input(&fx, &child), "stale")
                .await
                .unwrap()
        );

        // A mismatched link (wrong caller) is not the recorded child.
        let mut wrong = input(&fx, &child);
        wrong.caller_agent_id = "someone-else";
        assert!(
            !validate_child_in(&mut conn, &wrong, &fx.generation)
                .await
                .unwrap()
        );
        drop(conn);

        // When the parent turn completes, the child is no longer resumable.
        sqlx::query("UPDATE chat_turns SET status = 'completed' WHERE id = 'parent'")
            .execute(&fx.pool)
            .await
            .unwrap();
        let mut conn = fx.pool.acquire().await.unwrap();
        assert!(
            !validate_child_in(&mut conn, &input(&fx, &child), &fx.generation)
                .await
                .unwrap()
        );
    }
}
