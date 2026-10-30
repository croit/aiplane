//! Whether anyone but the owner can read a conversation.
//!
//! A conversation is shared when its link flag is set or when it is shared
//! with people or groups (`chat_session_shares`). Everything that keeps a
//! personal agent, a private binding or a delegation out of conversations
//! others can read asks this one question.

use super::{DbError, Pool};

/// SQL condition for "the conversation `$alias` is shared in no way".
#[macro_export]
macro_rules! unshared_sql {
    ($alias:literal) => {
        concat!(
            "(",
            $alias,
            ".shared = 0 AND NOT EXISTS (SELECT 1 FROM chat_session_shares sh WHERE sh.session_id = ",
            $alias,
            ".id))"
        )
    };
}

pub async fn is_shared(pool: &Pool, session_id: &str) -> Result<bool, DbError> {
    Ok(sqlx::query_scalar(concat!(
        "SELECT EXISTS (SELECT 1 FROM chat_sessions s WHERE s.id = ? AND NOT ",
        unshared_sql!("s"),
        ")"
    ))
    .bind(session_id)
    .fetch_one(pool)
    .await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn pool_with_session() -> Pool {
        let pool = super::super::open(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        sqlx::query("INSERT INTO users (id, email, created_at, updated_at) VALUES ('alice', 'a@example.com', '2026-01-01', '2026-01-01')")
            .execute(&pool)
            .await
            .unwrap();
        pool
    }

    #[tokio::test]
    async fn a_conversation_is_shared_by_link_by_person_or_by_group_and_otherwise_not() {
        let pool = pool_with_session().await;
        let session = session_core::db::create_session(&pool, "alice")
            .await
            .unwrap();
        assert!(!is_shared(&pool, &session.id).await.unwrap());

        sqlx::query("INSERT INTO chat_session_shares (session_id, subject_kind, subject_id) VALUES (?, 'user', 'bob')")
            .bind(&session.id)
            .execute(&pool)
            .await
            .unwrap();
        assert!(is_shared(&pool, &session.id).await.unwrap());

        sqlx::query("DELETE FROM chat_session_shares")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO chat_session_shares (session_id, subject_kind, subject_id) VALUES (?, 'group', 'eng')")
            .bind(&session.id)
            .execute(&pool)
            .await
            .unwrap();
        assert!(is_shared(&pool, &session.id).await.unwrap());

        sqlx::query("DELETE FROM chat_session_shares")
            .execute(&pool)
            .await
            .unwrap();
        session_core::db::set_shared(&pool, "alice", &session.id, true)
            .await
            .unwrap();
        assert!(is_shared(&pool, &session.id).await.unwrap());
    }
}
