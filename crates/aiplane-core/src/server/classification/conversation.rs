// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! A conversation's level and the audit of every change to it.
//!
//! The level only ever rises on its own ([`raise`]); [`lower`] is a policy
//! decision with a reason. The audit rows carry no message content.

use jiff::Timestamp;
use thiserror::Error;

use super::policy::Lowering;
use super::scheme::Scheme;
use crate::server::db::{DbError, Pool, WriteTx};

#[derive(Debug, Error)]
pub enum ClassificationError {
    #[error(
        "`{0}` is not a classification level of this installation; choose one of the levels \
         the administrator defined"
    )]
    UnknownLevel(String),
    #[error(
        "lowering a conversation's classification is not allowed by this installation's policy; \
         ask an administrator to permit lowering with a reason"
    )]
    LoweringNotAllowed,
    #[error(
        "lowering a conversation's classification needs a reason; state why the level may go down"
    )]
    ReasonRequired,
    #[error("the new level is not lower than the conversation's current level `{0}`")]
    NotLower(String),
    #[error(transparent)]
    Db(#[from] DbError),
}

impl From<sqlx::Error> for ClassificationError {
    fn from(error: sqlx::Error) -> Self {
        Self::Db(error.into())
    }
}

/// A change of level that was applied and audited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition {
    pub from: String,
    pub to: String,
}

/// The stored level id of a conversation; `None` means it never left the
/// scheme's lowest level.
pub async fn level_id(db: &Pool, session_id: &str) -> Result<Option<String>, DbError> {
    Ok(
        sqlx::query_scalar("SELECT level_id FROM chat_session_settings WHERE session_id = ?")
            .bind(session_id)
            .fetch_optional(db)
            .await?
            .flatten(),
    )
}

/// Store a conversation's level. Not audited and not checked against the
/// current level: the caller is creating the conversation or has already
/// decided through [`raise`] or [`lower`].
pub async fn set_level(db: &Pool, session_id: &str, level_id: &str) -> Result<(), DbError> {
    sqlx::query(
        r#"INSERT INTO chat_session_settings (session_id, level_id, updated_at)
           VALUES (?, ?, ?)
           ON CONFLICT(session_id) DO UPDATE SET
             level_id   = excluded.level_id,
             updated_at = excluded.updated_at"#,
    )
    .bind(session_id)
    .bind(level_id)
    .bind(Timestamp::now().to_string())
    .execute(db)
    .await?;
    Ok(())
}

pub(super) async fn write_event(
    conn: &mut sqlx::SqliteConnection,
    session_id: &str,
    from: &str,
    to: &str,
    cause: &str,
    actor: &str,
    reason: &str,
) -> Result<(), DbError> {
    sqlx::query(
        "INSERT INTO chat_classification_events
             (id, session_id, at, from_level, to_level, cause, actor, reason)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(session_id)
    .bind(Timestamp::now().to_string())
    .bind(from)
    .bind(to)
    .bind(cause)
    .bind(actor)
    .bind(reason)
    .execute(conn)
    .await?;
    Ok(())
}

/// Record a level that was set outside [`raise`] and [`lower`], such as at
/// conversation creation.
pub async fn record_event(
    db: &Pool,
    session_id: &str,
    from: &str,
    to: &str,
    cause: &str,
    actor: &str,
) -> Result<(), DbError> {
    let mut conn = db.acquire().await?;
    write_event(&mut conn, session_id, from, to, cause, actor, "").await
}

/// Raise the conversation to `to` if that is higher than where it is, and
/// audit the change. At or below the current level this does nothing and
/// returns `None`: `new = max(old, entered)`.
pub async fn raise(
    db: &Pool,
    scheme: &Scheme,
    session_id: &str,
    to: &str,
    cause: &str,
    actor: &str,
) -> Result<Option<Transition>, ClassificationError> {
    let target = scheme
        .level(to)
        .ok_or_else(|| ClassificationError::UnknownLevel(to.to_owned()))?;
    let mut tx = WriteTx::begin(db).await?;
    let stored = stored_level(&mut tx, session_id).await?;
    let current = scheme.level_or_lowest(stored.as_deref());
    if target.rank <= current.rank {
        return Ok(None);
    }
    store(&mut tx, session_id, &target.id).await?;
    write_event(
        &mut tx,
        session_id,
        &current.id,
        &target.id,
        cause,
        actor,
        "",
    )
    .await?;
    tx.commit().await?;
    Ok(Some(Transition {
        from: current.id.clone(),
        to: target.id.clone(),
    }))
}

/// Lower the conversation to `to`. Allowed only when the policy permits
/// lowering with a reason and a non-empty reason is given; always audited.
pub async fn lower(
    db: &Pool,
    scheme: &Scheme,
    policy: Lowering,
    session_id: &str,
    to: &str,
    actor: &str,
    reason: &str,
) -> Result<Transition, ClassificationError> {
    let target = scheme
        .level(to)
        .ok_or_else(|| ClassificationError::UnknownLevel(to.to_owned()))?;
    if policy != Lowering::WithReason {
        return Err(ClassificationError::LoweringNotAllowed);
    }
    let reason = reason.trim();
    if reason.is_empty() {
        return Err(ClassificationError::ReasonRequired);
    }
    let mut tx = WriteTx::begin(db).await?;
    let stored = stored_level(&mut tx, session_id).await?;
    let current = scheme.level_or_lowest(stored.as_deref());
    if target.rank >= current.rank {
        return Err(ClassificationError::NotLower(current.id.clone()));
    }
    store(&mut tx, session_id, &target.id).await?;
    write_event(
        &mut tx,
        session_id,
        &current.id,
        &target.id,
        "user",
        actor,
        reason,
    )
    .await?;
    tx.commit().await?;
    Ok(Transition {
        from: current.id.clone(),
        to: target.id.clone(),
    })
}

pub(super) async fn stored_level(
    conn: &mut sqlx::SqliteConnection,
    session_id: &str,
) -> Result<Option<String>, DbError> {
    Ok(
        sqlx::query_scalar("SELECT level_id FROM chat_session_settings WHERE session_id = ?")
            .bind(session_id)
            .fetch_optional(conn)
            .await?
            .flatten(),
    )
}

pub(super) async fn store(
    conn: &mut sqlx::SqliteConnection,
    session_id: &str,
    level_id: &str,
) -> Result<(), DbError> {
    sqlx::query(
        r#"INSERT INTO chat_session_settings (session_id, level_id, updated_at)
           VALUES (?, ?, ?)
           ON CONFLICT(session_id) DO UPDATE SET
             level_id   = excluded.level_id,
             updated_at = excluded.updated_at"#,
    )
    .bind(session_id)
    .bind(level_id)
    .bind(Timestamp::now().to_string())
    .execute(conn)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::server::classification::load_scheme;
    use crate::server::db::open;

    async fn conversation() -> (Pool, Scheme) {
        let db = open(Path::new(":memory:")).await.unwrap();
        sqlx::query(
            "INSERT INTO classification_levels (id, rank, name) VALUES
                 ('internal', 1, 'Internal'), ('secret', 2, 'Secret');
             INSERT INTO users (id, email, created_at, updated_at)
                 VALUES ('u1', 'u1@example.com', '2026-01-01', '2026-01-01');
             INSERT INTO chat_sessions (id, user_id, created_at, updated_at)
                 VALUES ('s1', 'u1', '2026-01-01', '2026-01-01');",
        )
        .execute(&db)
        .await
        .unwrap();
        let scheme = load_scheme(&db).await.unwrap();
        (db, scheme)
    }

    async fn events(db: &Pool) -> Vec<(String, String, String, String, String)> {
        sqlx::query_as(
            "SELECT from_level, to_level, cause, actor, reason
               FROM chat_classification_events ORDER BY at, rowid",
        )
        .fetch_all(db)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn a_conversation_without_a_stored_level_is_at_the_lowest() {
        let (db, scheme) = conversation().await;
        assert_eq!(level_id(&db, "s1").await.unwrap(), None);
        assert_eq!(scheme.level_or_lowest(None).id, "public");
    }

    #[tokio::test]
    async fn raising_only_ever_increases_and_is_audited() {
        let (db, scheme) = conversation().await;
        let up = raise(&db, &scheme, "s1", "internal", "system_one:pii", "u1")
            .await
            .unwrap();
        assert_eq!(
            up,
            Some(Transition {
                from: "public".into(),
                to: "internal".into()
            })
        );
        assert_eq!(
            level_id(&db, "s1").await.unwrap().as_deref(),
            Some("internal")
        );

        assert_eq!(
            raise(&db, &scheme, "s1", "public", "user", "u1")
                .await
                .unwrap(),
            None
        );
        assert_eq!(
            raise(&db, &scheme, "s1", "internal", "user", "u1")
                .await
                .unwrap(),
            None
        );
        assert_eq!(
            level_id(&db, "s1").await.unwrap().as_deref(),
            Some("internal")
        );

        raise(&db, &scheme, "s1", "secret", "user", "u1")
            .await
            .unwrap();
        assert_eq!(
            events(&db).await,
            [
                (
                    "public".into(),
                    "internal".into(),
                    "system_one:pii".into(),
                    "u1".into(),
                    String::new()
                ),
                (
                    "internal".into(),
                    "secret".into(),
                    "user".into(),
                    "u1".into(),
                    String::new()
                ),
            ]
        );
    }

    #[tokio::test]
    async fn an_unknown_level_is_refused_by_name() {
        let (db, scheme) = conversation().await;
        let err = raise(&db, &scheme, "s1", "top", "user", "u1")
            .await
            .unwrap_err();
        assert!(matches!(err, ClassificationError::UnknownLevel(ref l) if l == "top"));
        assert!(err.to_string().contains("`top`"));
    }

    #[tokio::test]
    async fn lowering_is_refused_unless_the_policy_allows_it_with_a_reason() {
        let (db, scheme) = conversation().await;
        raise(&db, &scheme, "s1", "secret", "user", "u1")
            .await
            .unwrap();

        let never = lower(&db, &scheme, Lowering::Never, "s1", "internal", "u1", "ok").await;
        assert!(matches!(
            never,
            Err(ClassificationError::LoweringNotAllowed)
        ));
        let no_reason = lower(
            &db,
            &scheme,
            Lowering::WithReason,
            "s1",
            "internal",
            "u1",
            "  ",
        )
        .await;
        assert!(matches!(
            no_reason,
            Err(ClassificationError::ReasonRequired)
        ));
        let not_lower = lower(
            &db,
            &scheme,
            Lowering::WithReason,
            "s1",
            "secret",
            "u1",
            "why",
        )
        .await;
        assert!(matches!(not_lower, Err(ClassificationError::NotLower(_))));
        assert_eq!(
            level_id(&db, "s1").await.unwrap().as_deref(),
            Some("secret")
        );

        let done = lower(
            &db,
            &scheme,
            Lowering::WithReason,
            "s1",
            "internal",
            "u1",
            "declassified by owner",
        )
        .await
        .unwrap();
        assert_eq!(
            done,
            Transition {
                from: "secret".into(),
                to: "internal".into()
            }
        );
        assert_eq!(
            level_id(&db, "s1").await.unwrap().as_deref(),
            Some("internal")
        );
        let last = events(&db).await.pop().unwrap();
        assert_eq!(last.4, "declassified by owner");
    }

    #[tokio::test]
    async fn the_effort_setting_and_the_level_do_not_disturb_each_other() {
        let (db, scheme) = conversation().await;
        crate::server::db::chat_session_settings::set_effort(&db, "s1", "high")
            .await
            .unwrap();
        raise(&db, &scheme, "s1", "internal", "user", "u1")
            .await
            .unwrap();
        crate::server::db::chat_session_settings::set_effort(&db, "s1", "low")
            .await
            .unwrap();
        assert_eq!(
            level_id(&db, "s1").await.unwrap().as_deref(),
            Some("internal")
        );
    }
}
