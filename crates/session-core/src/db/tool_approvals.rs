// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Calls approved for the rest of a conversation
//! (`chat_session_tool_approvals`): what [`Decision::AllowConversation`]
//! leaves behind, so the same tool with the same actions runs without asking
//! again in that conversation. A fork starts without them.

use super::*;

/// The actions part of the key: an approval covers exactly the actions it
/// was asked about.
fn actions_key(actions: Option<&[String]>) -> String {
    actions.map_or_else(String::new, |actions| {
        serde_json::to_string(actions).expect("plain strings serialize")
    })
}

/// Approve `tool` with `actions` for the rest of conversation `session_id`.
pub async fn approve_tool_for_session(
    pool: &Pool,
    session_id: &str,
    tool: &str,
    actions: Option<&[String]>,
) -> Result<(), DbError> {
    sqlx::query(
        r#"INSERT INTO chat_session_tool_approvals (session_id, tool, actions, approved_at)
           VALUES (?, ?, ?, ?)
           ON CONFLICT (session_id, tool, actions) DO NOTHING"#,
    )
    .bind(session_id)
    .bind(tool)
    .bind(actions_key(actions))
    .bind(Timestamp::now().to_string())
    .execute(pool)
    .await?;
    Ok(())
}

/// Whether `tool` with `actions` was approved for conversation `session_id`.
pub async fn tool_approved_for_session(
    pool: &Pool,
    session_id: &str,
    tool: &str,
    actions: Option<&[String]>,
) -> Result<bool, DbError> {
    let row = sqlx::query(
        "SELECT 1 FROM chat_session_tool_approvals \
         WHERE session_id = ? AND tool = ? AND actions = ?",
    )
    .bind(session_id)
    .bind(tool)
    .bind(actions_key(actions))
    .fetch_optional(pool)
    .await?;
    Ok(row.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::pool;

    #[tokio::test]
    async fn an_approval_covers_its_tool_and_actions_in_its_conversation_only() {
        let pool = pool().await;
        let session = create_session(&pool, "u1").await.unwrap();
        let other = create_session(&pool, "u1").await.unwrap();
        let read = ["read".to_string()];

        approve_tool_for_session(&pool, &session.id, "erp_invoke", None)
            .await
            .unwrap();
        approve_tool_for_session(&pool, &session.id, "erp_invoke", None)
            .await
            .unwrap();
        approve_tool_for_session(&pool, &session.id, "mail", Some(&read))
            .await
            .unwrap();

        assert!(
            tool_approved_for_session(&pool, &session.id, "erp_invoke", None)
                .await
                .unwrap()
        );
        assert!(
            !tool_approved_for_session(&pool, &other.id, "erp_invoke", None)
                .await
                .unwrap()
        );
        assert!(
            !tool_approved_for_session(&pool, &session.id, "erp_whoami", None)
                .await
                .unwrap()
        );
        assert!(
            tool_approved_for_session(&pool, &session.id, "mail", Some(&read))
                .await
                .unwrap()
        );
        assert!(
            !tool_approved_for_session(&pool, &session.id, "mail", Some(&["send".to_string()]))
                .await
                .unwrap()
        );
        assert!(
            !tool_approved_for_session(&pool, &session.id, "mail", None)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn deleting_the_conversation_deletes_its_approvals() {
        let pool = pool().await;
        let session = create_session(&pool, "u1").await.unwrap();
        approve_tool_for_session(&pool, &session.id, "erp_invoke", None)
            .await
            .unwrap();
        sqlx::query("DELETE FROM chat_sessions WHERE id = ?")
            .bind(&session.id)
            .execute(&pool)
            .await
            .unwrap();
        let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chat_session_tool_approvals")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(left, 0);
    }
}
