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

pub async fn link(
    pool: &Pool,
    owner_user_id: &str,
    dot_id: &str,
    session_id: &str,
) -> Result<bool, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let valid: bool = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM personal_dots d
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
         LEFT JOIN personal_dots d ON d.principal_id = p.dot_id AND d.owner_user_id = p.owner_user_id
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
        "SELECT EXISTS(SELECT 1 FROM personal_dots WHERE principal_id = ? AND owner_user_id = ? AND active = 1)",
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
    use crate::db::agents::{self, PersonalDotCreate};
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
        assert_eq!(result, PersonalDotCreate::Created);
        row.unwrap().principal.id
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
