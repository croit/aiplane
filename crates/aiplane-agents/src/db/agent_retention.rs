// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Deleting an agent's conversations once its retention period has passed
//! (`docs/agent-visitors.md` → "Retention").
//!
//! A conversation is a root `chat_sessions` row the agent's principal owns
//! (`parent_turn_id IS NULL`). It goes together with every sub-agent run it
//! started, however deep, because those child sessions hold the same
//! visitor's data. The foreign keys take the rest: turns, tool calls,
//! `agent_state`, the visitor session. A person's chat has `user_id` set and
//! is never selected, neither as a conversation nor as a child.

use jiff::Timestamp;

use super::{DbError, Pool};

/// What one sweep of one agent deleted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Swept {
    pub conversations: u64,
    pub sub_agent_runs: u64,
}

impl Swept {
    pub fn is_empty(&self) -> bool {
        self.conversations == 0 && self.sub_agent_runs == 0
    }
}

/// One conversation the sweep is about to take: its root session and every
/// sub-agent run it started, however deep.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversation {
    pub root: String,
    pub sub_agent_runs: Vec<String>,
}

impl Conversation {
    /// Every session that goes with it, sub-agent runs first.
    pub fn sessions(&self) -> impl Iterator<Item = &str> {
        self.sub_agent_runs
            .iter()
            .chain(std::iter::once(&self.root))
            .map(String::as_str)
    }
}

/// Agent `principal_id`'s conversations whose last activity (`updated_at`)
/// is before `idle_before`, each with its sub-agent runs. Only read: the
/// caller lists what they reference before [`delete_conversation`] takes
/// them, as every chat delete does.
///
/// A conversation still waiting for a decision is not one of them, however
/// idle: its pause is settled by a resume or by the expiry sweep (every
/// 30 s), and only then may it go. Deleting it first would take the request
/// a visitor or a member of staff is about to answer.
pub async fn idle_conversations(
    pool: &Pool,
    principal_id: &str,
    idle_before: Timestamp,
) -> Result<Vec<Conversation>, DbError> {
    let candidates: Vec<(String, String)> = sqlx::query_as(&format!(
        "SELECT id, updated_at FROM chat_sessions
         WHERE principal_id = ? AND {ROOT_AT_REST}"
    ))
    .bind(principal_id)
    .fetch_all(pool)
    .await?;
    let mut out = Vec::new();
    for (root, updated) in candidates {
        if !idle(&updated, idle_before) {
            continue;
        }
        let sub_agent_runs = sub_agent_runs(pool, &root).await?;
        out.push(Conversation {
            root,
            sub_agent_runs,
        });
    }
    Ok(out)
}

/// A root agent conversation with no pause pending.
const ROOT_AT_REST: &str = "user_id IS NULL AND parent_turn_id IS NULL
           AND NOT EXISTS (SELECT 1 FROM chat_turn_suspensions s
                           JOIN chat_turns t ON t.id = s.turn_id
                           WHERE t.session_id = chat_sessions.id)";

/// Parsed rather than compared as text: RFC 3339 with fractional seconds of
/// varying length does not order as a string.
fn idle(updated_at: &str, idle_before: Timestamp) -> bool {
    updated_at
        .parse::<Timestamp>()
        .is_ok_and(|t| t < idle_before)
}

async fn sub_agent_runs(pool: &Pool, root: &str) -> Result<Vec<String>, DbError> {
    let mut found: Vec<String> = Vec::new();
    let mut frontier = vec![root.to_string()];
    while !frontier.is_empty() {
        let mut next = Vec::new();
        for session in &frontier {
            let children: Vec<String> = sqlx::query_scalar(
                "SELECT c.id FROM chat_sessions c
                 JOIN chat_turns t ON t.id = c.parent_turn_id
                 WHERE t.session_id = ? AND c.user_id IS NULL",
            )
            .bind(session)
            .fetch_all(pool)
            .await?;
            next.extend(children);
        }
        next.retain(|id| !found.contains(id) && id != root);
        found.extend(next.iter().cloned());
        frontier = next;
    }
    Ok(found)
}

/// Delete `conversation` in one transaction, if its root is still idle
/// since before `idle_before` and waits for nothing: it may have moved on
/// since [`idle_conversations`] listed it. Whether it went.
pub async fn delete_conversation(
    pool: &Pool,
    principal_id: &str,
    conversation: &Conversation,
    idle_before: Timestamp,
) -> Result<bool, DbError> {
    let mut tx = pool.begin().await?;
    let updated: Option<String> = sqlx::query_scalar(&format!(
        "SELECT updated_at FROM chat_sessions
         WHERE id = ? AND principal_id = ? AND {ROOT_AT_REST}"
    ))
    .bind(&conversation.root)
    .bind(principal_id)
    .fetch_optional(&mut *tx)
    .await?;
    if !updated.is_some_and(|u| idle(&u, idle_before)) {
        return Ok(false);
    }
    for id in conversation.sessions() {
        sqlx::query("DELETE FROM chat_sessions WHERE id = ? AND user_id IS NULL")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::run_sessions;
    use crate::db::visitor_sessions::{self, NewVisitorSession};
    use crate::db::{agent_state, agents, embed_keys, system_principals as sp};
    use aiplane_core::server::db::users;
    use jiff::SignedDuration;
    use session_core::db as chat;
    use std::path::Path;

    const DAY: SignedDuration = SignedDuration::from_hours(24);

    fn t0() -> Timestamp {
        "2026-10-01T12:00:00Z".parse().unwrap()
    }

    async fn agent(pool: &Pool, name: &str) -> String {
        agents::create(
            pool,
            &sp::NewPrincipal {
                name,
                display: name,
                description: "",
            },
            "{}",
            "alice",
        )
        .await
        .unwrap()
        .unwrap()
        .principal
        .id
    }

    async fn last_active(pool: &Pool, session: &str, at: Timestamp) {
        sqlx::query("UPDATE chat_sessions SET updated_at = ? WHERE id = ?")
            .bind(at.to_string())
            .bind(session)
            .execute(pool)
            .await
            .unwrap();
    }

    async fn sweep_all(pool: &Pool, principal_id: &str, idle_before: Timestamp) -> Swept {
        let mut swept = Swept::default();
        for c in idle_conversations(pool, principal_id, idle_before)
            .await
            .unwrap()
        {
            if delete_conversation(pool, principal_id, &c, idle_before)
                .await
                .unwrap()
            {
                swept.conversations += 1;
                swept.sub_agent_runs += c.sub_agent_runs.len() as u64;
            }
        }
        swept
    }

    async fn count(pool: &Pool, sql: &str) -> i64 {
        sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
    }

    async fn visitor_conversation(pool: &Pool, agent: &str, key: &str, hash: &str) -> String {
        visitor_sessions::start(
            pool,
            &NewVisitorSession {
                principal_id: agent,
                embed_key_id: key,
                agent_version: 1,
                token_hash: hash,
                client_ip: None,
                idle_ttl: SignedDuration::from_mins(30),
                max_age: DAY,
                now: t0(),
            },
        )
        .await
        .unwrap()
        .session_id
    }

    async fn sub_agent_run(pool: &Pool, sub: &str, parent_session: &str) -> String {
        let parent_turn = uuid::Uuid::new_v4().to_string();
        chat::create_assistant_turn_in_progress(pool, parent_session, &parent_turn, "m")
            .await
            .unwrap();
        run_sessions::create_principal_session(
            pool,
            &run_sessions::NewRunSession {
                principal_id: sub,
                title: None,
                parent_turn_id: Some(&parent_turn),
                agent_version: Some(1),
            },
        )
        .await
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn an_idle_conversation_goes_with_its_sub_agent_runs_state_and_visitor() {
        let pool = aiplane_core::server::db::open(Path::new(":memory:"))
            .await
            .unwrap();
        let support = agent(&pool, "support").await;
        let billing = agent(&pool, "billing").await;
        let key = embed_keys::create(
            &pool,
            &embed_keys::NewEmbedKey {
                principal_id: &support,
                name: "site",
                origins: &["https://a.example".to_string()],
                key_hash: "kh",
            },
            "alice",
        )
        .await
        .unwrap()
        .id;
        let old = visitor_conversation(&pool, &support, &key, "old").await;
        let fresh = visitor_conversation(&pool, &support, &key, "fresh").await;
        let child = sub_agent_run(&pool, &billing, &old).await;
        let grandchild = sub_agent_run(&pool, &support, &child).await;
        let billings_own = visitor_conversation_without_key(&pool, &billing).await;
        agent_state::tests::put_committed(
            &pool,
            &old,
            "issue",
            &serde_json::json!("billing"),
            "llm",
            t0(),
        )
        .await
        .unwrap();
        for s in [&old, &child, &grandchild, &billings_own] {
            last_active(&pool, s, t0()).await;
        }
        last_active(&pool, &fresh, t0() + 20 * DAY).await;

        let swept = sweep_all(&pool, &support, t0() + 10 * DAY).await;
        assert_eq!(
            swept,
            Swept {
                conversations: 1,
                sub_agent_runs: 2
            }
        );
        let left: Vec<String> = sqlx::query_scalar("SELECT id FROM chat_sessions ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
        let mut expected = vec![fresh.clone(), billings_own.clone()];
        expected.sort();
        assert_eq!(
            left, expected,
            "the fresh conversation and the sub-agent's own one stay"
        );
        assert_eq!(count(&pool, "SELECT COUNT(*) FROM agent_state").await, 0);
        assert_eq!(
            count(&pool, "SELECT COUNT(*) FROM visitor_sessions").await,
            1,
            "only the fresh conversation's visitor remains"
        );
    }

    #[tokio::test]
    async fn a_conversation_waiting_for_a_decision_is_kept_until_it_is_settled() {
        let pool = aiplane_core::server::db::open(Path::new(":memory:"))
            .await
            .unwrap();
        let support = agent(&pool, "support").await;
        let waiting = visitor_conversation_without_key(&pool, &support).await;
        chat::create_assistant_turn_in_progress(&pool, &waiting, "paused", "m")
            .await
            .unwrap();
        let suspension = chat::TurnSuspension {
            turn_id: "paused".into(),
            request_id: "req-1".into(),
            kind: chat::SuspensionKind::Approval,
            message: None,
            tool_call: chat::PendingCall {
                execution_facts: None,
                id: "call-1".into(),
                name: "refund".into(),
                arguments: "{}".into(),
                actions: None,
            },
            tail: Vec::new(),
            budget_used: chat::BudgetUsed::default(),
            child_turn: None,
            on_timeout: chat::TimeoutFallback::Deny,
            expires_at: t0() + 90 * DAY,
            created_at: t0(),
            run_context: None,
        };
        assert!(chat::suspend_turn(&pool, &suspension).await.unwrap());
        last_active(&pool, &waiting, t0()).await;

        let swept = sweep_all(&pool, &support, t0() + 60 * DAY).await;
        assert_eq!(swept, Swept::default(), "a pending request keeps it");

        chat::cancel_suspended_turn(&pool, "paused").await.unwrap();
        last_active(&pool, &waiting, t0()).await;
        let swept = sweep_all(&pool, &support, t0() + 60 * DAY).await;
        assert_eq!(swept.conversations, 1, "settled, it goes like any other");
    }

    async fn visitor_conversation_without_key(pool: &Pool, agent: &str) -> String {
        run_sessions::create_principal_session(
            pool,
            &run_sessions::NewRunSession {
                principal_id: agent,
                title: None,
                parent_turn_id: None,
                agent_version: Some(1),
            },
        )
        .await
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn a_persons_chats_are_never_swept() {
        let pool = aiplane_core::server::db::open(Path::new(":memory:"))
            .await
            .unwrap();
        let support = agent(&pool, "support").await;
        let now = Timestamp::now();
        users::upsert(
            &pool,
            &users::User {
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
        let chat = chat::create_session(&pool, "alice").await.unwrap();
        last_active(&pool, &chat.id, t0()).await;

        let swept = sweep_all(&pool, &support, t0() + 100 * DAY).await;
        assert!(swept.is_empty());
        assert_eq!(count(&pool, "SELECT COUNT(*) FROM chat_sessions").await, 1);
    }
}
