// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use jiff::Timestamp;
use sqlx::Row;

use super::agent_audit::{AuditKind, Correlation, NewEvent};
use super::{DbError, Pool, WriteTx};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonalDotSession {
    pub session_id: String,
    pub owner_user_id: String,
    pub dot_id: String,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonalChildLink {
    pub parent_turn_id: String,
    pub parent_tool_call_id: String,
    pub caller_dot_id: String,
    pub edge_generation: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonalChildSession {
    pub session: PersonalDotSession,
    pub link: PersonalChildLink,
}

pub struct PersonalChildTurn {
    pub session: PersonalDotSession,
    pub assistant_turn: session_core::db::Turn,
    pub link: PersonalChildLink,
}

#[derive(Debug, Clone, Copy)]
pub struct NewPersonalChild<'a> {
    pub owner_user_id: &'a str,
    pub caller_dot_id: &'a str,
    pub receiver_dot_id: &'a str,
    pub parent_turn_id: &'a str,
    pub parent_tool_call_id: &'a str,
    pub edge_generation: &'a str,
    pub title: &'a str,
    pub message: &'a str,
    pub model: &'a str,
}

pub async fn open_child(
    pool: &Pool,
    new: NewPersonalChild<'_>,
) -> Result<Option<PersonalChildTurn>, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    if super::dot_delegation::authorized_in(
        &mut tx,
        new.owner_user_id,
        new.caller_dot_id,
        new.receiver_dot_id,
        new.edge_generation,
    )
    .await?
    .is_none()
    {
        return Ok(None);
    }
    let valid: bool = sqlx::query_scalar("SELECT EXISTS(
        SELECT 1 FROM chat_turns t
        JOIN chat_sessions s ON s.id = t.session_id
        JOIN personal_dot_sessions d ON d.session_id = s.id
        JOIN chat_tool_calls c ON c.turn_id = t.id AND c.id = ?
        WHERE t.id = ? AND t.role = 'assistant' AND t.status = 'in_progress'
          AND s.user_id = ? AND s.principal_id IS NULL AND s.shared = 0
          AND d.owner_user_id = ? AND d.dot_id = ?
          AND c.name = 'forward_request' AND c.status = 'running'
          AND NOT EXISTS (SELECT 1 FROM personal_dot_sessions child WHERE child.parent_turn_id = t.id AND child.parent_tool_call_id = c.id)
          AND NOT EXISTS (
              SELECT 1 FROM personal_dot_sessions child
              JOIN chat_turns busy ON busy.session_id = child.session_id
              WHERE child.parent_turn_id = t.id AND busy.role = 'assistant'
                AND busy.status IN ('in_progress', 'suspended')
          )
    )")
        .bind(new.parent_tool_call_id).bind(new.parent_turn_id)
        .bind(new.owner_user_id).bind(new.owner_user_id).bind(new.caller_dot_id)
        .fetch_one(&mut *tx).await?;
    if !valid {
        return Ok(None);
    }
    let session = session_core::db::create_session_in(&mut tx, new.owner_user_id).await?;
    sqlx::query("UPDATE chat_sessions SET parent_turn_id = ?, title = ? WHERE id = ?")
        .bind(new.parent_turn_id)
        .bind(new.title)
        .bind(&session.id)
        .execute(&mut *tx)
        .await?;
    let user_turn_id = uuid::Uuid::new_v4().to_string();
    let assistant_turn_id = uuid::Uuid::new_v4().to_string();
    let Some((_, assistant_turn)) = session_core::db::create_turn_pair_if_idle_in(
        &mut tx,
        &session.id,
        &user_turn_id,
        &assistant_turn_id,
        new.message,
        new.model,
        true,
    )
    .await?
    else {
        return Ok(None);
    };
    sqlx::query("INSERT INTO personal_dot_sessions (session_id, owner_user_id, dot_id, created_at, parent_turn_id, parent_tool_call_id, caller_dot_id, edge_generation) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
        .bind(&session.id).bind(new.owner_user_id).bind(new.receiver_dot_id).bind(Timestamp::now().to_string())
        .bind(new.parent_turn_id).bind(new.parent_tool_call_id).bind(new.caller_dot_id).bind(new.edge_generation)
        .execute(&mut *tx).await?;
    super::agent_audit::append(&mut tx, NewEvent::new(AuditKind::PersonalDotSessionStarted, new.receiver_dot_id, serde_json::json!({
        "caller_dot_id": new.caller_dot_id, "parent_turn_id": new.parent_turn_id,
        "parent_tool_call_id": new.parent_tool_call_id, "edge_generation": new.edge_generation,
    })).by(Some(new.owner_user_id)).at(Correlation {
        session_id: Some(session.id.clone()), turn_id: Some(assistant_turn.id.clone()), ..Correlation::default()
    })).await?;
    tx.commit().await?;
    Ok(Some(PersonalChildTurn {
        session: PersonalDotSession {
            session_id: session.id,
            owner_user_id: new.owner_user_id.into(),
            dot_id: new.receiver_dot_id.into(),
            active: true,
        },
        assistant_turn,
        link: PersonalChildLink {
            parent_turn_id: new.parent_turn_id.into(),
            parent_tool_call_id: new.parent_tool_call_id.into(),
            caller_dot_id: new.caller_dot_id.into(),
            edge_generation: new.edge_generation.into(),
        },
    }))
}

pub async fn link(
    pool: &Pool,
    owner_user_id: &str,
    dot_id: &str,
    session_id: &str,
) -> Result<bool, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let valid: bool = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM dots d
            JOIN chat_sessions s ON s.id = ?
            WHERE d.principal_id = ? AND d.owner_user_id = ? AND d.active = 1
              AND s.user_id = ? AND s.principal_id IS NULL
        )",
    )
    .bind(session_id)
    .bind(dot_id)
    .bind(owner_user_id)
    .bind(owner_user_id)
    .fetch_one(&mut *tx)
    .await?;
    if !valid {
        return Ok(false);
    }

    let existing: Option<(String, String)> = sqlx::query_as(
        "SELECT owner_user_id, dot_id FROM personal_dot_sessions WHERE session_id = ?",
    )
    .bind(session_id)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some((existing_owner, existing_dot)) = existing {
        return Ok(existing_owner == owner_user_id && existing_dot == dot_id);
    }

    sqlx::query(
        "INSERT INTO personal_dot_sessions (session_id, owner_user_id, dot_id, created_at)
         VALUES (?, ?, ?, ?)",
    )
    .bind(session_id)
    .bind(owner_user_id)
    .bind(dot_id)
    .bind(Timestamp::now().to_string())
    .execute(&mut *tx)
    .await?;
    super::agent_audit::append(
        &mut tx,
        NewEvent::new(
            AuditKind::PersonalDotSessionStarted,
            dot_id,
            serde_json::json!({}),
        )
        .by(Some(owner_user_id))
        .at(Correlation {
            session_id: Some(session_id.to_string()),
            ..Correlation::default()
        }),
    )
    .await?;
    tx.commit().await?;
    Ok(true)
}

pub async fn for_session(
    pool: &Pool,
    owner_user_id: &str,
    session_id: &str,
) -> Result<Option<PersonalDotSession>, DbError> {
    let row = sqlx::query(
        "SELECT p.session_id, p.owner_user_id, p.dot_id, COALESCE(d.active, 0) AS active
         FROM personal_dot_sessions p
         LEFT JOIN dots d ON d.principal_id = p.dot_id AND d.owner_user_id = p.owner_user_id
         JOIN chat_sessions s ON s.id = p.session_id
         WHERE p.session_id = ? AND p.owner_user_id = ? AND s.user_id = ?
           AND s.principal_id IS NULL",
    )
    .bind(session_id)
    .bind(owner_user_id)
    .bind(owner_user_id)
    .fetch_optional(pool)
    .await?;
    row.map(|row| {
        Ok(PersonalDotSession {
            session_id: row.try_get("session_id")?,
            owner_user_id: row.try_get("owner_user_id")?,
            dot_id: row.try_get("dot_id")?,
            active: row.try_get("active")?,
        })
    })
    .transpose()
}

pub async fn binding_valid(
    connection: &mut sqlx::SqliteConnection,
    owner_user_id: &str,
    dot_id: Option<&str>,
    previous_dot_id: Option<&str>,
) -> Result<bool, DbError> {
    if dot_id.is_none() || dot_id == previous_dot_id {
        return Ok(true);
    }
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM dots WHERE principal_id = ? AND owner_user_id = ? AND active = 1)",
    ).bind(dot_id).bind(owner_user_id).fetch_one(connection).await?)
}

pub async fn session_matches(
    connection: &mut sqlx::SqliteConnection,
    owner_user_id: &str,
    session_id: &str,
    dot_id: Option<&str>,
) -> Result<bool, DbError> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM chat_sessions s
         LEFT JOIN personal_dot_sessions d ON d.session_id = s.id
         WHERE s.id = ? AND s.user_id = ? AND s.principal_id IS NULL AND s.shared = 0
           AND ((? IS NULL AND d.session_id IS NULL) OR (d.dot_id = ? AND d.owner_user_id = ?)))",
    )
    .bind(session_id)
    .bind(owner_user_id)
    .bind(dot_id)
    .bind(dot_id)
    .bind(owner_user_id)
    .fetch_one(connection)
    .await?)
}

pub async fn scoped_session_ids(
    pool: &Pool,
    owner_user_id: &str,
    dot_id: Option<&str>,
) -> Result<std::collections::HashSet<String>, DbError> {
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT s.id FROM chat_sessions s
         LEFT JOIN personal_dot_sessions d ON d.session_id = s.id
         WHERE s.user_id = ? AND s.principal_id IS NULL AND s.shared = 0
           AND ((? IS NULL AND d.session_id IS NULL) OR (d.dot_id = ? AND d.owner_user_id = ?))",
    )
    .bind(owner_user_id)
    .bind(dot_id)
    .bind(dot_id)
    .bind(owner_user_id)
    .fetch_all(pool)
    .await?;
    Ok(ids.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::agents::{self, DotCreate};
    use crate::db::system_principals::NewPrincipal;
    use aiplane_core::server::db::open;
    use aiplane_core::server::db::users::{self, User};
    use jiff::Timestamp;
    use std::path::Path;

    async fn user(pool: &Pool, id: &str) {
        let now = Timestamp::now();
        users::upsert(
            pool,
            &User {
                id: id.into(),
                email: format!("{id}@example.com"),
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

    async fn dot(pool: &Pool, owner: &str, name: &str) -> String {
        let (result, row) = agents::create_personal_dot(
            pool,
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
        assert_eq!(result, DotCreate::Created);
        row.unwrap().principal.id
    }

    struct ChildFixture {
        pool: Pool,
        caller: String,
        receiver: String,
        generation: String,
        parent_session: String,
    }

    impl ChildFixture {
        fn input<'a>(&'a self, parent: &'a str, call: &'a str) -> NewPersonalChild<'a> {
            NewPersonalChild {
                owner_user_id: "alice",
                caller_dot_id: &self.caller,
                receiver_dot_id: &self.receiver,
                parent_turn_id: parent,
                parent_tool_call_id: call,
                edge_generation: &self.generation,
                title: "Receiver",
                message: "Review the provided text",
                model: "model",
            }
        }
    }

    async fn child_fixture() -> ChildFixture {
        use crate::db::dot_delegation::{self, ConsentDirection, ConsentEdit, ConsentSaved};
        let pool = open(Path::new(":memory:")).await.unwrap();
        user(&pool, "alice").await;
        user(&pool, "bob").await;
        let caller = dot(&pool, "alice", "sender").await;
        let receiver = dot(&pool, "alice", "receiver").await;
        let first = dot_delegation::set_consent(
            &pool,
            ConsentEdit {
                owner_user_id: "alice",
                dot_id: &caller,
                peer_id: &receiver,
                direction: ConsentDirection::Outbound,
                enabled: true,
                expected_generation: None,
            },
        )
        .await
        .unwrap();
        let ConsentSaved::Saved(first) = first else {
            panic!("initial consent failed");
        };
        let both = dot_delegation::set_consent(
            &pool,
            ConsentEdit {
                owner_user_id: "alice",
                dot_id: &receiver,
                peer_id: &caller,
                direction: ConsentDirection::Inbound,
                enabled: true,
                expected_generation: Some(&first.generation),
            },
        )
        .await
        .unwrap();
        let ConsentSaved::Saved(both) = both else {
            panic!("receiver consent failed");
        };
        let session = session_core::db::create_session(&pool, "alice")
            .await
            .unwrap();
        assert!(link(&pool, "alice", &caller, &session.id).await.unwrap());
        session_core::db::create_assistant_turn_in_progress(&pool, &session.id, "parent", "model")
            .await
            .unwrap();
        session_core::db::insert_running_tool_call(
            &pool,
            "parent",
            "forward",
            "forward_request",
            "{}",
        )
        .await
        .unwrap();
        ChildFixture {
            pool,
            caller,
            receiver,
            generation: both.generation,
            parent_session: session.id,
        }
    }

    #[tokio::test]
    async fn personal_child_refuses_another_pending_child_on_the_same_parent() {
        let fx = child_fixture().await;
        let first = open_child(&fx.pool, fx.input("parent", "forward"))
            .await
            .unwrap()
            .unwrap();
        session_core::db::insert_running_tool_call(
            &fx.pool,
            "parent",
            "second",
            "forward_request",
            "{}",
        )
        .await
        .unwrap();
        assert!(
            open_child(&fx.pool, fx.input("parent", "second"))
                .await
                .unwrap()
                .is_none()
        );
        sqlx::query("UPDATE chat_turns SET status = 'completed' WHERE id = ?")
            .bind(&first.assistant_turn.id)
            .execute(&fx.pool)
            .await
            .unwrap();
        assert!(
            open_child(&fx.pool, fx.input("parent", "second"))
                .await
                .unwrap()
                .is_some()
        );
        let other = session_core::db::create_session(&fx.pool, "alice")
            .await
            .unwrap();
        assert!(
            link(&fx.pool, "alice", &fx.caller, &other.id)
                .await
                .unwrap()
        );
        session_core::db::create_assistant_turn_in_progress(
            &fx.pool,
            &other.id,
            "other-parent",
            "model",
        )
        .await
        .unwrap();
        session_core::db::insert_running_tool_call(
            &fx.pool,
            "other-parent",
            "third",
            "forward_request",
            "{}",
        )
        .await
        .unwrap();
        assert!(
            open_child(&fx.pool, fx.input("other-parent", "third"))
                .await
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn personal_child_cleanup_follows_terminal_ancestors_after_the_root_is_shared() {
        use crate::db::{
            child_sessions,
            dot_delegation::{self, ConsentDirection, ConsentEdit, ConsentSaved},
        };
        let fx = child_fixture().await;
        let child = open_child(&fx.pool, fx.input("parent", "forward"))
            .await
            .unwrap()
            .unwrap();
        let third = dot(&fx.pool, "alice", "Third").await;
        let ConsentSaved::Saved(first) = dot_delegation::set_consent(
            &fx.pool,
            ConsentEdit {
                owner_user_id: "alice",
                dot_id: &fx.receiver,
                peer_id: &third,
                direction: ConsentDirection::Outbound,
                enabled: true,
                expected_generation: None,
            },
        )
        .await
        .unwrap() else {
            panic!("outbound consent failed");
        };
        let ConsentSaved::Saved(second) = dot_delegation::set_consent(
            &fx.pool,
            ConsentEdit {
                owner_user_id: "alice",
                dot_id: &third,
                peer_id: &fx.receiver,
                direction: ConsentDirection::Inbound,
                enabled: true,
                expected_generation: Some(&first.generation),
            },
        )
        .await
        .unwrap() else {
            panic!("inbound consent failed");
        };
        session_core::db::insert_running_tool_call(
            &fx.pool,
            &child.assistant_turn.id,
            "nested-forward",
            "forward_request",
            "{}",
        )
        .await
        .unwrap();
        let grandchild = open_child(
            &fx.pool,
            NewPersonalChild {
                owner_user_id: "alice",
                caller_dot_id: &fx.receiver,
                receiver_dot_id: &third,
                parent_turn_id: &child.assistant_turn.id,
                parent_tool_call_id: "nested-forward",
                edge_generation: &second.generation,
                title: "Third",
                message: "Explicit nested text",
                model: "model",
            },
        )
        .await
        .unwrap()
        .unwrap();
        for turn in ["parent", child.assistant_turn.id.as_str()] {
            session_core::db::finalize_turn(
                &fx.pool,
                turn,
                session_core::db::TurnStatus::Completed,
                None,
            )
            .await
            .unwrap();
        }
        assert!(
            session_core::db::set_shared(&fx.pool, "alice", &fx.parent_session, true)
                .await
                .unwrap()
        );
        assert_eq!(
            child_sessions::child_turns(&fx.pool, "alice", "parent")
                .await
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            child_sessions::active_children(&fx.pool, "alice", &child.assistant_turn.id)
                .await
                .unwrap()[0]
                .assistant_turn
                .id,
            grandchild.assistant_turn.id
        );
        for dot_id in [&fx.caller, &fx.receiver, &third] {
            assert_eq!(
                child_sessions::active_roots_for_dot(&fx.pool, "alice", dot_id)
                    .await
                    .unwrap(),
                vec!["parent".to_owned()]
            );
        }
        assert_eq!(
            child_sessions::active_roots_for_edge(
                &fx.pool,
                "alice",
                &fx.caller,
                &fx.receiver,
                &fx.generation
            )
            .await
            .unwrap(),
            vec!["parent".to_owned()]
        );
        assert_eq!(
            child_sessions::active_roots_for_edge(
                &fx.pool,
                "alice",
                &fx.receiver,
                &third,
                &second.generation
            )
            .await
            .unwrap(),
            vec!["parent".to_owned()]
        );
        assert!(
            child_sessions::root_session_id(&fx.pool, "alice", &grandchild.session.session_id)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            child_sessions::active_roots_for_dot(&fx.pool, "bob", &third)
                .await
                .unwrap()
                .is_empty()
        );
        let mut conn = fx.pool.acquire().await.unwrap();
        assert!(
            !child_sessions::validate_child_in(
                &mut conn,
                "alice",
                &child.assistant_turn.id,
                "parent",
                "forward",
                &fx.generation
            )
            .await
            .unwrap()
        );
        drop(conn);
        sqlx::query("UPDATE chat_sessions SET shared = 1 WHERE id = ?")
            .bind(&child.session.session_id)
            .execute(&fx.pool)
            .await
            .unwrap();
        assert!(
            child_sessions::child_turns(&fx.pool, "alice", &child.assistant_turn.id)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            child_sessions::active_roots_for_dot(&fx.pool, "alice", &third)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn personal_child_cleanup_uses_original_turn_and_retains_revoked_history_authority() {
        use crate::db::{
            child_sessions,
            dot_delegation::{self, ConsentDirection, ConsentEdit, ConsentSaved},
        };
        let fx = child_fixture().await;
        let child = open_child(&fx.pool, fx.input("parent", "forward"))
            .await
            .unwrap()
            .unwrap();
        sqlx::query("UPDATE chat_turns SET status = 'completed' WHERE id = 'parent'")
            .execute(&fx.pool)
            .await
            .unwrap();
        session_core::db::create_assistant_turn_in_progress(
            &fx.pool,
            &fx.parent_session,
            "unrelated-new-root",
            "model",
        )
        .await
        .unwrap();
        let children = child_sessions::active_children(&fx.pool, "alice", "parent")
            .await
            .unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].assistant_turn.id, child.assistant_turn.id);
        assert_eq!(children[0].link, child.link);
        assert!(
            child_sessions::active_children(&fx.pool, "bob", "parent")
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            child_sessions::active_children(&fx.pool, "alice", "unrelated-new-root")
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            child_sessions::active_roots_for_dot(&fx.pool, "alice", &fx.receiver)
                .await
                .unwrap(),
            vec!["parent".to_string()]
        );
        assert_eq!(
            child_sessions::active_roots_for_edge(
                &fx.pool,
                "alice",
                &fx.caller,
                &fx.receiver,
                &fx.generation
            )
            .await
            .unwrap(),
            vec!["parent".to_string()]
        );
        let ConsentSaved::Saved(revoked) = dot_delegation::set_consent(
            &fx.pool,
            ConsentEdit {
                owner_user_id: "alice",
                dot_id: &fx.caller,
                peer_id: &fx.receiver,
                direction: ConsentDirection::Outbound,
                enabled: false,
                expected_generation: Some(&fx.generation),
            },
        )
        .await
        .unwrap() else {
            panic!("revocation failed");
        };
        agents::set_personal_dot_active(&fx.pool, &fx.receiver, "alice", false, 4)
            .await
            .unwrap();
        assert_eq!(
            child_sessions::active_roots_for_edge(
                &fx.pool,
                "alice",
                &fx.caller,
                &fx.receiver,
                &fx.generation
            )
            .await
            .unwrap(),
            vec!["parent".to_string()]
        );
        assert!(
            child_sessions::active_roots_for_edge(
                &fx.pool,
                "alice",
                &fx.caller,
                &fx.receiver,
                &revoked.generation
            )
            .await
            .unwrap()
            .is_empty()
        );
        assert_eq!(
            child_sessions::active_children(&fx.pool, "alice", "parent")
                .await
                .unwrap()
                .len(),
            1
        );
        sqlx::query("UPDATE chat_turns SET status = 'completed' WHERE id = ?")
            .bind(&child.assistant_turn.id)
            .execute(&fx.pool)
            .await
            .unwrap();
        assert!(
            child_sessions::active_children(&fx.pool, "alice", "parent")
                .await
                .unwrap()
                .is_empty()
        );
        let terminal_children = child_sessions::child_turns(&fx.pool, "alice", "parent")
            .await
            .unwrap();
        assert_eq!(terminal_children.len(), 1);
        assert_eq!(
            terminal_children[0].assistant_turn.id,
            child.assistant_turn.id
        );
        assert_eq!(
            terminal_children[0].assistant_turn.status,
            session_core::db::TurnStatus::Completed
        );
        sqlx::query("UPDATE chat_tool_calls SET name = 'other_tool' WHERE turn_id = 'parent' AND id = 'forward'").execute(&fx.pool).await.unwrap();
        assert!(
            child_sessions::active_children(&fx.pool, "alice", "parent")
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            child_sessions::active_roots_for_dot(&fx.pool, "alice", &fx.receiver)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn personal_child_opens_private_session_pair_and_exact_immutable_link() {
        let fx = child_fixture().await;
        let child = open_child(&fx.pool, fx.input("parent", "forward"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(child.session.owner_user_id, "alice");
        assert_eq!(child.session.dot_id, fx.receiver);
        assert_eq!(child.link.parent_turn_id, "parent");
        assert_eq!(child.link.parent_tool_call_id, "forward");
        assert_eq!(child.link.caller_dot_id, fx.caller);
        assert_eq!(child.link.edge_generation, fx.generation);
        assert_eq!(child.assistant_turn.session_id, child.session.session_id);
        assert_eq!(
            child.assistant_turn.status,
            session_core::db::TurnStatus::InProgress
        );
        let (owner, principal, parent, title, shared): (String, Option<String>, String, String, bool) = sqlx::query_as("SELECT user_id, principal_id, parent_turn_id, title, shared FROM chat_sessions WHERE id = ?")
            .bind(&child.session.session_id).fetch_one(&fx.pool).await.unwrap();
        assert_eq!(owner, "alice");
        assert!(principal.is_none());
        assert_eq!(parent, "parent");
        assert_eq!(title, "Receiver");
        assert!(!shared);
        let turns = session_core::db::list_turns(&fx.pool, &child.session.session_id)
            .await
            .unwrap();
        assert_eq!(turns.len(), 2);
        assert_eq!(
            turns[0].turn.user_content.as_deref(),
            Some("Review the provided text")
        );
        assert!(
            open_child(&fx.pool, fx.input("parent", "forward"))
                .await
                .unwrap()
                .is_none()
        );
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chat_sessions")
            .fetch_one(&fx.pool)
            .await
            .unwrap();
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn personal_child_denials_never_create_session_or_turn_rows() {
        let fx = child_fixture().await;
        for (owner, caller, receiver, parent, call, generation) in [
            (
                "bob",
                fx.caller.as_str(),
                fx.receiver.as_str(),
                "parent",
                "forward",
                fx.generation.as_str(),
            ),
            (
                "alice",
                fx.receiver.as_str(),
                fx.caller.as_str(),
                "parent",
                "forward",
                fx.generation.as_str(),
            ),
            (
                "alice",
                fx.caller.as_str(),
                fx.receiver.as_str(),
                "missing",
                "forward",
                fx.generation.as_str(),
            ),
            (
                "alice",
                fx.caller.as_str(),
                fx.receiver.as_str(),
                "parent",
                "missing",
                fx.generation.as_str(),
            ),
            (
                "alice",
                fx.caller.as_str(),
                fx.receiver.as_str(),
                "parent",
                "forward",
                "stale",
            ),
        ] {
            let mut input = fx.input(parent, call);
            input.owner_user_id = owner;
            input.caller_dot_id = caller;
            input.receiver_dot_id = receiver;
            input.edge_generation = generation;
            assert!(open_child(&fx.pool, input).await.unwrap().is_none());
        }
        sqlx::query("UPDATE chat_tool_calls SET name = 'other_tool' WHERE turn_id = 'parent' AND id = 'forward'").execute(&fx.pool).await.unwrap();
        assert!(
            open_child(&fx.pool, fx.input("parent", "forward"))
                .await
                .unwrap()
                .is_none()
        );
        sqlx::query("UPDATE chat_tool_calls SET name = 'forward_request', status = 'completed' WHERE turn_id = 'parent' AND id = 'forward'").execute(&fx.pool).await.unwrap();
        assert!(
            open_child(&fx.pool, fx.input("parent", "forward"))
                .await
                .unwrap()
                .is_none()
        );
        sqlx::query("UPDATE chat_tool_calls SET status = 'running' WHERE turn_id = 'parent' AND id = 'forward'").execute(&fx.pool).await.unwrap();
        sqlx::query("UPDATE chat_sessions SET shared = 1 WHERE id = ?")
            .bind(&fx.parent_session)
            .execute(&fx.pool)
            .await
            .unwrap();
        assert!(
            open_child(&fx.pool, fx.input("parent", "forward"))
                .await
                .unwrap()
                .is_none()
        );
        sqlx::query("UPDATE chat_sessions SET shared = 0 WHERE id = ?")
            .bind(&fx.parent_session)
            .execute(&fx.pool)
            .await
            .unwrap();
        sqlx::query("UPDATE chat_turns SET status = 'completed' WHERE id = 'parent'")
            .execute(&fx.pool)
            .await
            .unwrap();
        assert!(
            open_child(&fx.pool, fx.input("parent", "forward"))
                .await
                .unwrap()
                .is_none()
        );
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chat_sessions")
            .fetch_one(&fx.pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn personal_child_storage_error_rolls_back_session_pair_and_link() {
        let fx = child_fixture().await;
        sqlx::query("CREATE TRIGGER reject_child BEFORE INSERT ON personal_dot_sessions WHEN NEW.parent_turn_id IS NOT NULL BEGIN SELECT RAISE(ABORT, 'fixture failure'); END")
            .execute(&fx.pool).await.unwrap();
        assert!(
            open_child(&fx.pool, fx.input("parent", "forward"))
                .await
                .is_err()
        );
        let sessions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chat_sessions")
            .fetch_one(&fx.pool)
            .await
            .unwrap();
        let turns: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chat_turns")
            .fetch_one(&fx.pool)
            .await
            .unwrap();
        assert_eq!(sessions, 1);
        assert_eq!(turns, 1);
    }

    #[tokio::test]
    async fn personal_child_lookup_validates_exact_relationship_and_keeps_deleted_dot_binding() {
        use crate::db::child_sessions::{self, LocalChild};
        let fx = child_fixture().await;
        let child = open_child(&fx.pool, fx.input("parent", "forward"))
            .await
            .unwrap()
            .unwrap();
        let loaded = child_sessions::child_of_turn(&fx.pool, &child.assistant_turn.id)
            .await
            .unwrap()
            .unwrap();
        let LocalChild::Personal(loaded) = loaded else {
            panic!("personal child became a principal");
        };
        assert_eq!(loaded.session, child.session);
        assert_eq!(loaded.link, child.link);
        assert_eq!(
            child_sessions::root_session_id(&fx.pool, "alice", &child.session.session_id)
                .await
                .unwrap(),
            Some(fx.parent_session.clone())
        );
        assert!(
            child_sessions::root_session_id(&fx.pool, "bob", &child.session.session_id)
                .await
                .unwrap()
                .is_none()
        );
        let mut tx = WriteTx::begin(&fx.pool).await.unwrap();
        assert!(
            child_sessions::validate_child_in(
                &mut tx,
                "alice",
                &child.assistant_turn.id,
                "parent",
                "forward",
                &fx.generation
            )
            .await
            .unwrap()
        );
        for (owner, parent, call, generation) in [
            ("bob", "parent", "forward", fx.generation.as_str()),
            ("alice", "wrong", "forward", fx.generation.as_str()),
            ("alice", "parent", "wrong", fx.generation.as_str()),
            ("alice", "parent", "forward", "stale"),
        ] {
            assert!(
                !child_sessions::validate_child_in(
                    &mut tx,
                    owner,
                    &child.assistant_turn.id,
                    parent,
                    call,
                    generation
                )
                .await
                .unwrap()
            );
        }
        tx.commit().await.unwrap();
        assert!(
            agents::delete(&fx.pool, &fx.receiver, "alice")
                .await
                .unwrap()
        );
        let loaded = child_sessions::child_of_turn(&fx.pool, &child.assistant_turn.id)
            .await
            .unwrap()
            .unwrap();
        let LocalChild::Personal(loaded) = loaded else {
            panic!("deleted personal child lost its binding");
        };
        assert_eq!(loaded.session.dot_id, fx.receiver);
        assert!(!loaded.session.active);
        assert_eq!(
            child_sessions::root_session_id(&fx.pool, "alice", &child.session.session_id)
                .await
                .unwrap(),
            Some(fx.parent_session.clone())
        );
        let mut tx = WriteTx::begin(&fx.pool).await.unwrap();
        assert!(
            !child_sessions::validate_child_in(
                &mut tx,
                "alice",
                &child.assistant_turn.id,
                "parent",
                "forward",
                &fx.generation
            )
            .await
            .unwrap()
        );
    }

    #[tokio::test]
    async fn personal_child_root_lookup_refuses_broken_links_and_cycles() {
        use crate::db::child_sessions;
        let fx = child_fixture().await;
        let child = open_child(&fx.pool, fx.input("parent", "forward"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            child_sessions::root_session_id(&fx.pool, "alice", &fx.parent_session)
                .await
                .unwrap(),
            Some(fx.parent_session.clone())
        );
        sqlx::query(
            "UPDATE personal_dot_sessions SET parent_tool_call_id = 'missing' WHERE session_id = ?",
        )
        .bind(&child.session.session_id)
        .execute(&fx.pool)
        .await
        .unwrap();
        assert!(
            child_sessions::root_session_id(&fx.pool, "alice", &child.session.session_id)
                .await
                .unwrap()
                .is_none()
        );
        sqlx::query(
            "UPDATE personal_dot_sessions SET parent_tool_call_id = 'forward' WHERE session_id = ?",
        )
        .bind(&child.session.session_id)
        .execute(&fx.pool)
        .await
        .unwrap();
        session_core::db::insert_running_tool_call(
            &fx.pool,
            &child.assistant_turn.id,
            "cycle",
            "forward_request",
            "{}",
        )
        .await
        .unwrap();
        sqlx::query("UPDATE chat_sessions SET parent_turn_id = ? WHERE id = ?")
            .bind(&child.assistant_turn.id)
            .bind(&fx.parent_session)
            .execute(&fx.pool)
            .await
            .unwrap();
        sqlx::query("UPDATE personal_dot_sessions SET parent_turn_id = ?, parent_tool_call_id = 'cycle', caller_dot_id = ?, edge_generation = ? WHERE session_id = ?")
            .bind(&child.assistant_turn.id).bind(&fx.receiver).bind(&fx.generation).bind(&fx.parent_session).execute(&fx.pool).await.unwrap();
        assert!(
            child_sessions::root_session_id(&fx.pool, "alice", &child.session.session_id)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn links_only_the_active_owners_private_chat_to_their_dot() {
        let pool = open(Path::new(":memory:")).await.unwrap();
        user(&pool, "alice").await;
        user(&pool, "bob").await;
        let alice_dot = dot(&pool, "alice", "helper").await;
        let alice_session = session_core::db::create_session(&pool, "alice")
            .await
            .unwrap();
        let bob_session = session_core::db::create_session(&pool, "bob")
            .await
            .unwrap();

        assert!(
            link(&pool, "alice", &alice_dot, &alice_session.id)
                .await
                .unwrap()
        );
        assert_eq!(
            for_session(&pool, "alice", &alice_session.id)
                .await
                .unwrap()
                .unwrap()
                .dot_id,
            alice_dot
        );
        assert!(
            for_session(&pool, "bob", &alice_session.id)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            !link(&pool, "alice", &alice_dot, &bob_session.id)
                .await
                .unwrap()
        );

        assert!(
            agents::set_personal_dot_active(&pool, &alice_dot, "alice", false, 4)
                .await
                .unwrap()
        );
        assert!(
            !for_session(&pool, "alice", &alice_session.id)
                .await
                .unwrap()
                .unwrap()
                .active
        );
    }

    #[tokio::test]
    async fn a_session_cannot_be_relinked_to_another_dot_or_owner() {
        let pool = open(Path::new(":memory:")).await.unwrap();
        user(&pool, "alice").await;
        user(&pool, "bob").await;
        let alice_dot = dot(&pool, "alice", "helper").await;
        let bob_dot = dot(&pool, "bob", "helper").await;
        let session = session_core::db::create_session(&pool, "alice")
            .await
            .unwrap();

        assert!(link(&pool, "alice", &alice_dot, &session.id).await.unwrap());
        assert!(!link(&pool, "alice", &bob_dot, &session.id).await.unwrap());
        assert!(!link(&pool, "bob", &alice_dot, &session.id).await.unwrap());
    }
    #[tokio::test]
    async fn scoped_bindings_preserve_deleted_dots_and_owner_isolation() {
        let pool = open(Path::new(":memory:")).await.unwrap();
        user(&pool, "alice").await;
        user(&pool, "bob").await;
        let dot_id = dot(&pool, "alice", "helper").await;
        let bound = session_core::db::create_session(&pool, "alice")
            .await
            .unwrap();
        let ordinary = session_core::db::create_session(&pool, "alice")
            .await
            .unwrap();
        assert!(link(&pool, "alice", &dot_id, &bound.id).await.unwrap());
        let mut connection = pool.acquire().await.unwrap();
        assert!(
            binding_valid(&mut connection, "alice", Some(&dot_id), None)
                .await
                .unwrap()
        );
        assert!(
            !binding_valid(&mut connection, "bob", Some(&dot_id), None)
                .await
                .unwrap()
        );
        assert!(
            !session_matches(&mut connection, "alice", &bound.id, None)
                .await
                .unwrap()
        );
        assert!(
            session_matches(&mut connection, "alice", &bound.id, Some(&dot_id))
                .await
                .unwrap()
        );
        assert!(
            !session_matches(&mut connection, "bob", &bound.id, Some(&dot_id))
                .await
                .unwrap()
        );
        drop(connection);
        assert_eq!(
            scoped_session_ids(&pool, "alice", None).await.unwrap(),
            std::collections::HashSet::from([ordinary.id])
        );
        assert_eq!(
            scoped_session_ids(&pool, "alice", Some(&dot_id))
                .await
                .unwrap(),
            std::collections::HashSet::from([bound.id.clone()])
        );
        assert!(agents::delete(&pool, &dot_id, "alice").await.unwrap());
        let binding = for_session(&pool, "alice", &bound.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(binding.dot_id, dot_id);
        assert!(!binding.active);
        let mut connection = pool.acquire().await.unwrap();
        assert!(
            !binding_valid(&mut connection, "alice", Some(&dot_id), None)
                .await
                .unwrap()
        );
        assert!(
            binding_valid(&mut connection, "alice", Some(&dot_id), Some(&dot_id))
                .await
                .unwrap()
        );
        assert!(
            !session_matches(&mut connection, "alice", &bound.id, None)
                .await
                .unwrap()
        );
    }
}
