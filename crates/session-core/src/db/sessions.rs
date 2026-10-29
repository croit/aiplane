// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use super::*;

/// Create a freshly empty conversation for `user_id`. Returns the new
/// row; the caller's next step is usually to redirect to its URL.
pub async fn create_session(pool: &Pool, user_id: &str) -> Result<Session, DbError> {
    let mut connection = pool.acquire().await?;
    create_session_in(&mut connection, user_id).await
}

pub async fn create_session_in(
    connection: &mut sqlx::SqliteConnection,
    user_id: &str,
) -> Result<Session, DbError> {
    let now = Timestamp::now();
    let s = Session {
        id: Uuid::new_v4().to_string(),
        user_id: user_id.to_string(),
        title: None,
        created_at: now,
        updated_at: now,
        shared: false,
        pinned: false,
    };
    sqlx::query(
        r#"INSERT INTO chat_sessions (id, user_id, title, created_at, updated_at)
           VALUES (?, ?, ?, ?, ?)"#,
    )
    .bind(&s.id)
    .bind(&s.user_id)
    .bind(s.title.as_deref())
    .bind(s.created_at.to_string())
    .bind(s.updated_at.to_string())
    .execute(connection)
    .await?;
    Ok(s)
}

/// All conversations for a user. Pinned conversations float to the top
/// (in their own recency order); the rest follow, also most-recent first.
pub async fn list_sessions(pool: &Pool, user_id: &str) -> Result<Vec<Session>, DbError> {
    let rows = sqlx::query(
        r#"SELECT id, user_id, title, created_at, updated_at, shared, pinned
           FROM chat_sessions
           WHERE user_id = ?
           ORDER BY pinned DESC, updated_at DESC, id ASC"#,
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    rows.iter().map(map_session).collect()
}

/// The user's most recent blank conversation: untitled, unshared, without a
/// parent, with no turn yet. Every flow that opens a
/// conversation for a purpose (an architect, an agent, a background run) titles it
/// at creation, so a blank one is an unused plain chat.
pub async fn blank_session(pool: &Pool, user_id: &str) -> Result<Option<Session>, DbError> {
    let row = sqlx::query(
        r#"SELECT id, user_id, title, created_at, updated_at, shared, pinned
           FROM chat_sessions s
           WHERE user_id = ? AND title IS NULL AND shared = 0 AND parent_turn_id IS NULL
             AND NOT EXISTS (SELECT 1 FROM chat_turns t WHERE t.session_id = s.id)
           ORDER BY updated_at DESC, id ASC
           LIMIT 1"#,
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    row.as_ref().map(map_session).transpose()
}

/// Look up a session by id, but only if it belongs to this user. Caller
/// uses the None case to send a 404 / redirect to /chat.
pub async fn get_session(
    pool: &Pool,
    user_id: &str,
    session_id: &str,
) -> Result<Option<Session>, DbError> {
    let row = sqlx::query(
        r#"SELECT id, user_id, title, created_at, updated_at, shared, pinned
           FROM chat_sessions
           WHERE id = ? AND user_id = ?"#,
    )
    .bind(session_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    row.as_ref().map(map_session).transpose()
}

/// Owner of the chat session a given turn belongs to. Used by the
/// attachment-proxy route to authorize `GET /chat/attachment/<turn>/
/// <file>`: the caller's session must match the returned `user_id`,
/// otherwise user A could fetch user B's uploaded files by guessing
/// turn ids. `None` when the turn id doesn't exist, and for a turn of a
/// conversation no person owns (`user_id` NULL).
pub async fn user_for_turn(pool: &Pool, turn_id: &str) -> Result<Option<String>, DbError> {
    user_for_turn_in_scope(pool, turn_id, false).await
}

pub async fn user_for_root_turn(pool: &Pool, turn_id: &str) -> Result<Option<String>, DbError> {
    user_for_turn_in_scope(pool, turn_id, true).await
}

async fn user_for_turn_in_scope(
    pool: &Pool,
    turn_id: &str,
    roots_only: bool,
) -> Result<Option<String>, DbError> {
    let row = sqlx::query(
        r#"SELECT s.user_id AS user_id
           FROM chat_turns t
           JOIN chat_sessions s ON s.id = t.session_id
           WHERE t.id = ? AND (? = 0 OR s.parent_turn_id IS NULL)"#,
    )
    .bind(turn_id)
    .bind(roots_only)
    .fetch_optional(pool)
    .await?;
    Ok(row
        .map(|r| r.try_get::<Option<String>, _>("user_id"))
        .transpose()?
        .flatten())
}

pub async fn child_session_ids(
    pool: &Pool,
    owner: &str,
) -> Result<std::collections::HashSet<String>, DbError> {
    Ok(sqlx::query_scalar::<_, String>(
        "SELECT id FROM chat_sessions WHERE user_id = ? AND parent_turn_id IS NOT NULL",
    )
    .bind(owner)
    .fetch_all(pool)
    .await?
    .into_iter()
    .collect())
}

pub async fn is_root_session_owned(
    pool: &Pool,
    owner: &str,
    session: &str,
) -> Result<bool, DbError> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM chat_sessions WHERE id = ? AND user_id = ? AND parent_turn_id IS NULL)")
        .bind(session).bind(owner).fetch_one(pool).await?)
}

pub async fn session_has_parent(pool: &Pool, session: &str) -> Result<Option<bool>, DbError> {
    Ok(
        sqlx::query_scalar("SELECT parent_turn_id IS NOT NULL FROM chat_sessions WHERE id = ?")
            .bind(session)
            .fetch_optional(pool)
            .await?,
    )
}

/// Look up a session readable by `viewer_id`: either they own it, or it has
/// been shared (`shared = 1`). Used by the read-only paths (view, tail,
/// attachments) where any signed-in user holding the session's UUID may read
/// a shared conversation. Mutating paths must keep using the owner-only
/// [`get_session`].
pub async fn get_session_readable(
    pool: &Pool,
    viewer_id: &str,
    session_id: &str,
) -> Result<Option<Session>, DbError> {
    let row = sqlx::query(
        r#"SELECT id, user_id, title, created_at, updated_at, shared, pinned
           FROM chat_sessions
           WHERE id = ? AND (user_id = ? OR (shared = 1 AND parent_turn_id IS NULL))"#,
    )
    .bind(session_id)
    .bind(viewer_id)
    .fetch_optional(pool)
    .await?;
    row.as_ref().map(map_session).transpose()
}

/// Set (or clear) a session's shared flag — owner-only. Returns true when a
/// row was updated (the caller owns it); false otherwise, so a non-owner's
/// attempt is a silent no-op rather than leaking existence via an error.
pub async fn set_shared(
    pool: &Pool,
    user_id: &str,
    session_id: &str,
    shared: bool,
) -> Result<bool, DbError> {
    let res = sqlx::query(r#"UPDATE chat_sessions SET shared = ? WHERE id = ? AND user_id = ? AND parent_turn_id IS NULL"#)
        .bind(shared as i64)
        .bind(session_id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// Set (or clear) a session's pinned flag — owner-only. Returns true when a
/// row was updated (the caller owns it); false otherwise, so a non-owner's
/// attempt is a silent no-op rather than leaking existence via an error.
/// Same owner-scoping guarantee as [`set_shared`].
pub async fn set_pinned(
    pool: &Pool,
    user_id: &str,
    session_id: &str,
    pinned: bool,
) -> Result<bool, DbError> {
    let res = sqlx::query(r#"UPDATE chat_sessions SET pinned = ? WHERE id = ? AND user_id = ?"#)
        .bind(pinned as i64)
        .bind(session_id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// Whether the chat session a given turn belongs to is readable by
/// `viewer_id` — owner or shared. Backs the attachment proxy so files in a
/// shared conversation are fetchable by a viewer, while a private turn's
/// files stay owner-only. `false` when the turn id doesn't exist.
pub async fn turn_session_readable(
    pool: &Pool,
    turn_id: &str,
    viewer_id: &str,
) -> Result<bool, DbError> {
    let row = sqlx::query(
        r#"SELECT 1 AS ok
           FROM chat_turns t
           JOIN chat_sessions s ON s.id = t.session_id
           WHERE t.id = ? AND (s.user_id = ? OR (s.shared = 1 AND s.parent_turn_id IS NULL))"#,
    )
    .bind(turn_id)
    .bind(viewer_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.is_some())
}

/// Whether `turn_id` is a turn of `session_id`. The session-scoping gate
/// for model-supplied `<turn_id>/<filename>` attachment ids that carry no
/// marker in the conversation text — a typst render's hidden `.json` edit
/// base, for instance — and so can't be proven in-session by
/// `chat_attachments::list_session_attachments`. `false` for an unknown
/// turn, so a guessed id is refused rather than fetched.
pub async fn turn_in_session(
    pool: &Pool,
    turn_id: &str,
    session_id: &str,
) -> Result<bool, DbError> {
    let row = sqlx::query(r#"SELECT 1 AS ok FROM chat_turns WHERE id = ? AND session_id = ?"#)
        .bind(turn_id)
        .bind(session_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.is_some())
}

/// Which of `ids` still name a session. Lets a page that holds chat ids it did
/// not create (a schedule's runs, a webhook's fires) tell a deleted chat apart
/// from a live one in one query rather than one per row.
pub async fn existing_session_ids(
    pool: &Pool,
    ids: &[&str],
) -> Result<std::collections::HashSet<String>, DbError> {
    let mut connection = pool.acquire().await?;
    existing_session_ids_in(&mut connection, ids).await
}

pub async fn existing_session_ids_in(
    connection: &mut sqlx::SqliteConnection,
    ids: &[&str],
) -> Result<std::collections::HashSet<String>, DbError> {
    if ids.is_empty() {
        return Ok(std::collections::HashSet::new());
    }
    let placeholders = vec!["?"; ids.len()].join(", ");
    let sql = format!("SELECT id FROM chat_sessions WHERE id IN ({placeholders})");
    let mut query = sqlx::query_scalar::<_, String>(&sql);
    for id in ids {
        query = query.bind(*id);
    }
    Ok(query.fetch_all(connection).await?.into_iter().collect())
}

/// Delete a session (cascades to turns + tool_calls). Returns true iff
/// a row was actually removed — caller uses this to send a clean toast
/// vs a "not found" one.
pub async fn delete_session(pool: &Pool, user_id: &str, session_id: &str) -> Result<bool, DbError> {
    let result = sqlx::query(
        r#"DELETE FROM chat_sessions WHERE id = ? AND user_id = ? AND parent_turn_id IS NULL"#,
    )
    .bind(session_id)
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::pool;

    #[tokio::test]
    async fn existing_session_ids_in_sees_uncommitted_rows_and_preserves_rollback() {
        let pool = pool().await;
        let kept = create_session(&pool, "u1").await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        let pending = create_session_in(&mut tx, "u1").await.unwrap();
        let ids = [&*kept.id, &*pending.id, "missing", &*kept.id];
        assert_eq!(
            existing_session_ids_in(&mut tx, &ids).await.unwrap(),
            std::collections::HashSet::from([kept.id.clone(), pending.id.clone()]),
        );
        assert!(
            existing_session_ids_in(&mut tx, &[])
                .await
                .unwrap()
                .is_empty()
        );
        sqlx::query("DELETE FROM chat_sessions WHERE id=?")
            .bind(&kept.id)
            .execute(&mut *tx)
            .await
            .unwrap();
        assert_eq!(
            existing_session_ids_in(&mut tx, &ids).await.unwrap(),
            std::collections::HashSet::from([pending.id.clone()]),
        );
        tx.rollback().await.unwrap();
        assert_eq!(
            existing_session_ids(&pool, &ids).await.unwrap(),
            std::collections::HashSet::from([kept.id])
        );
    }

    async fn completed_turn(pool: &Pool, session_id: &str, text: &str) -> String {
        let id = Uuid::new_v4().to_string();
        create_user_turn(pool, session_id, &id, text).await.unwrap();
        id
    }

    #[tokio::test]
    async fn blank_session_is_only_an_untitled_unshared_root_conversation_without_turns() {
        let pool = pool().await;
        assert!(blank_session(&pool, "u1").await.unwrap().is_none());
        let blank = create_session(&pool, "u1").await.unwrap();
        let used = create_session(&pool, "u1").await.unwrap();
        completed_turn(&pool, &used.id, "hello").await;
        let titled = create_session(&pool, "u1").await.unwrap();
        set_session_title(&pool, &titled.id, "Plan support bot")
            .await
            .unwrap();
        let shared = create_session(&pool, "u1").await.unwrap();
        assert!(set_shared(&pool, "u1", &shared.id, true).await.unwrap());
        let child = create_session(&pool, "u1").await.unwrap();
        sqlx::query("UPDATE chat_sessions SET parent_turn_id = 'parent' WHERE id = ?")
            .bind(&child.id)
            .execute(&pool)
            .await
            .unwrap();
        crate::db::tests::unowned_session(&pool).await;

        assert_eq!(
            blank_session(&pool, "u1").await.unwrap().map(|s| s.id),
            Some(blank.id)
        );
    }

    #[tokio::test]
    async fn opaque_child_accessors_scope_mutation_and_feedback_to_roots() {
        let pool = pool().await;
        let root = create_session(&pool, "u1").await.unwrap();
        let child = create_session(&pool, "u1").await.unwrap();
        sqlx::query("UPDATE chat_sessions SET parent_turn_id = 'parent' WHERE id = ?")
            .bind(&child.id)
            .execute(&pool)
            .await
            .unwrap();
        create_turn_pair_if_idle(
            &pool,
            &root.id,
            "root-user",
            "root-assistant",
            "Root",
            "model",
            true,
        )
        .await
        .unwrap()
        .unwrap();
        create_turn_pair_if_idle(
            &pool,
            &child.id,
            "child-user",
            "child-assistant",
            "Child",
            "model",
            true,
        )
        .await
        .unwrap()
        .unwrap();
        assert!(is_root_session_owned(&pool, "u1", &root.id).await.unwrap());
        assert!(!is_root_session_owned(&pool, "u2", &root.id).await.unwrap());
        assert!(!is_root_session_owned(&pool, "u1", &child.id).await.unwrap());
        assert!(!is_root_session_owned(&pool, "u1", "missing").await.unwrap());
        assert_eq!(
            session_has_parent(&pool, &root.id).await.unwrap(),
            Some(false)
        );
        assert_eq!(
            session_has_parent(&pool, &child.id).await.unwrap(),
            Some(true)
        );
        assert_eq!(session_has_parent(&pool, "missing").await.unwrap(), None);
        assert_eq!(
            child_session_ids(&pool, "u1").await.unwrap(),
            std::collections::HashSet::from([child.id])
        );
        assert!(child_session_ids(&pool, "u2").await.unwrap().is_empty());
        assert_eq!(
            user_for_root_turn(&pool, "root-assistant")
                .await
                .unwrap()
                .as_deref(),
            Some("u1")
        );
        assert_eq!(
            user_for_root_turn(&pool, "child-assistant").await.unwrap(),
            None
        );
        assert_eq!(user_for_root_turn(&pool, "missing").await.unwrap(), None);
    }

    #[tokio::test]
    async fn delegated_child_is_private_even_if_shared_flag_is_set() {
        let pool = pool().await;
        let session = create_session(&pool, "u1").await.unwrap();
        sqlx::query("UPDATE chat_sessions SET parent_turn_id = 'parent', shared = 1 WHERE id = ?")
            .bind(&session.id)
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            get_session_readable(&pool, "u2", &session.id)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            get_session_readable(&pool, "u1", &session.id)
                .await
                .unwrap()
                .is_some()
        );
        assert!(!delete_session(&pool, "u1", &session.id).await.unwrap());
    }

    #[tokio::test]
    async fn delegated_child_attachments_are_private_even_if_shared_flag_is_set() {
        let pool = pool().await;
        let session = create_session(&pool, "u1").await.unwrap();
        create_turn_pair_if_idle(
            &pool,
            &session.id,
            "prompt",
            "reply",
            "task",
            "model",
            false,
        )
        .await
        .unwrap()
        .unwrap();
        sqlx::query("UPDATE chat_sessions SET parent_turn_id = 'parent', shared = 1 WHERE id = ?")
            .bind(&session.id)
            .execute(&pool)
            .await
            .unwrap();
        assert!(turn_session_readable(&pool, "reply", "u1").await.unwrap());
        assert!(!turn_session_readable(&pool, "reply", "u2").await.unwrap());
        assert!(!turn_session_readable(&pool, "missing", "u1").await.unwrap());
    }

    #[tokio::test]
    async fn delegated_child_cannot_be_shared_by_its_owner() {
        let pool = pool().await;
        let session = create_session(&pool, "u1").await.unwrap();
        sqlx::query("UPDATE chat_sessions SET parent_turn_id = 'parent' WHERE id = ?")
            .bind(&session.id)
            .execute(&pool)
            .await
            .unwrap();
        assert!(!set_shared(&pool, "u1", &session.id, true).await.unwrap());
        assert!(
            !get_session(&pool, "u1", &session.id)
                .await
                .unwrap()
                .unwrap()
                .shared
        );
    }

    #[tokio::test]
    async fn connection_session_and_turn_pair_rollback_as_one_unit() {
        let pool = pool().await;
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
        let session = create_session_in(&mut tx, "u1").await.unwrap();
        let pair = create_turn_pair_if_idle_in(
            &mut tx,
            &session.id,
            "prompt",
            "reply",
            "Explicit task",
            "model",
            false,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(pair.0.user_content.as_deref(), Some("Explicit task"));
        assert!(
            create_turn_pair_if_idle_in(
                &mut tx,
                &session.id,
                "next-prompt",
                "next-reply",
                "Other",
                "model",
                false
            )
            .await
            .unwrap()
            .is_none()
        );
        tx.rollback().await.unwrap();
        assert!(
            get_session(&pool, "u1", &session.id)
                .await
                .unwrap()
                .is_none()
        );
        assert!(list_turns(&pool, &session.id).await.unwrap().is_empty());

        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
        let session = create_session_in(&mut tx, "u1").await.unwrap();
        create_turn_pair_if_idle_in(
            &mut tx,
            &session.id,
            "prompt",
            "reply",
            "Explicit task",
            "model",
            false,
        )
        .await
        .unwrap()
        .unwrap();
        tx.commit().await.unwrap();
        assert!(
            get_session(&pool, "u1", &session.id)
                .await
                .unwrap()
                .is_some()
        );
        assert_eq!(list_turns(&pool, &session.id).await.unwrap().len(), 2);
    }

    /// A conversation another owner holds (`user_id` NULL) is opaque here:
    /// no person's view lists, searches, opens or shares it.
    #[tokio::test]
    async fn a_conversation_without_a_person_is_in_no_persons_view() {
        let pool = pool().await;
        let mine = create_session(&pool, "u1").await.unwrap();
        set_session_title(&pool, &mine.id, "invoices for march")
            .await
            .unwrap();
        let other = crate::db::tests::unowned_session(&pool).await;
        let other_turn = completed_turn(&pool, &other, "invoices please").await;

        let listed: Vec<String> = list_sessions(&pool, "u1")
            .await
            .unwrap()
            .into_iter()
            .map(|s| s.id)
            .collect();
        assert_eq!(listed, vec![mine.id.clone()]);
        assert!(get_session(&pool, "u1", &other).await.unwrap().is_none());
        assert!(
            get_session_readable(&pool, "u1", &other)
                .await
                .unwrap()
                .is_none()
        );
        let hits = search_sessions(&pool, "u1", "invoices", 10).await.unwrap();
        assert_eq!(
            hits.iter()
                .map(|h| h.session_id.as_str())
                .collect::<Vec<_>>(),
            vec![mine.id.as_str()]
        );
        assert_eq!(user_for_turn(&pool, &other_turn).await.unwrap(), None);
        assert!(
            !turn_session_readable(&pool, &other_turn, "u1")
                .await
                .unwrap()
        );
        assert!(!set_shared(&pool, "u1", &other, true).await.unwrap());
        assert!(!delete_session(&pool, "u1", &other).await.unwrap());
    }
}
