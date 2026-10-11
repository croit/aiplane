// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Conversations owned by a system principal (an agent run), and the reads
//! over `chat_sessions` that need to know who owns one.
//!
//! session-core keeps the conversation substrate owner-agnostic: it reads and
//! writes a person's chat by `user_id` and treats any other owner as opaque,
//! so a principal-owned run (`user_id` NULL) is never listed, searched,
//! opened or shared as anyone's chat, by construction. Everything that names
//! the other owner — `principal_id`, `agent_version`, `visitor_id`,
//! `parent_turn_id`, the run's `lang` — lives here, along with the agent
//! sweep of expired pauses and the inbox reads that tell a person's pause
//! from an agent's.

use jiff::Timestamp;
use session_core::db::suspensions::{
    TimeoutFallback, TurnSuspension, deadline_bound, map_suspension,
};
use session_core::i18n::Lang;
use sqlx::Row;
use sqlx::sqlite::SqliteRow;
use uuid::Uuid;

use super::{DbError, Pool, parse_optional_ts, parse_ts};

/// Who a conversation belongs to: a person, or a system principal running it
/// (an agent run). Exactly one, enforced by a CHECK on `chat_sessions`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionOwner {
    User(String),
    Principal(String),
}

/// Inputs to [`create_principal_session`].
#[derive(Debug, Clone, Copy)]
pub struct NewRunSession<'a> {
    pub principal_id: &'a str,
    pub title: Option<&'a str>,
    /// On a sub-agent run: the calling agent's turn.
    pub parent_turn_id: Option<&'a str>,
    /// The agent version the run executes, when the principal is an agent.
    pub agent_version: Option<i64>,
}

/// A conversation owned by a system principal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSession {
    pub id: String,
    pub principal_id: String,
    pub parent_turn_id: Option<String>,
    pub agent_version: Option<i64>,
    /// The visitor session the conversation serves, on a public agent's.
    pub visitor_id: Option<String>,
    /// The language its latest turn was asked in ([`set_run_lang`]).
    pub lang: Option<Lang>,
    pub title: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

/// Open a conversation owned by a system principal. Fails when the principal
/// does not exist (foreign key). Takes any executor so a caller can open it
/// inside its own transaction (a visitor session links to it atomically).
pub async fn create_principal_session<'e, E>(
    exec: E,
    new: &NewRunSession<'_>,
) -> Result<RunSession, DbError>
where
    E: sqlx::SqliteExecutor<'e>,
{
    let now = Timestamp::now();
    let s = RunSession {
        id: Uuid::new_v4().to_string(),
        principal_id: new.principal_id.to_string(),
        parent_turn_id: new.parent_turn_id.map(str::to_string),
        agent_version: new.agent_version,
        visitor_id: None,
        lang: None,
        title: new.title.map(str::to_string),
        created_at: now,
        updated_at: now,
    };
    sqlx::query(
        r#"INSERT INTO chat_sessions
             (id, principal_id, parent_turn_id, agent_version, title, created_at, updated_at)
           VALUES (?, ?, ?, ?, ?, ?, ?)"#,
    )
    .bind(&s.id)
    .bind(&s.principal_id)
    .bind(s.parent_turn_id.as_deref())
    .bind(s.agent_version)
    .bind(s.title.as_deref())
    .bind(s.created_at.to_string())
    .bind(s.updated_at.to_string())
    .execute(exec)
    .await?;
    Ok(s)
}

/// A principal-owned conversation, only if `principal_id` owns it.
pub async fn get_principal_session(
    pool: &Pool,
    principal_id: &str,
    session_id: &str,
) -> Result<Option<RunSession>, DbError> {
    let row = sqlx::query(&format!(
        "SELECT {RUN_SESSION_COLUMNS} FROM chat_sessions WHERE id = ? AND principal_id = ?"
    ))
    .bind(session_id)
    .bind(principal_id)
    .fetch_optional(pool)
    .await?;
    row.as_ref().map(map_run_session).transpose()
}

/// The principal-owned conversation `turn_id` belongs to; `None` when the
/// turn does not exist or is in a person's chat.
pub async fn run_session_of_turn(
    pool: &Pool,
    turn_id: &str,
) -> Result<Option<RunSession>, DbError> {
    let mut connection = pool.acquire().await?;
    run_session_of_turn_in(&mut connection, turn_id).await
}

pub async fn run_session_of_turn_in(
    connection: &mut sqlx::SqliteConnection,
    turn_id: &str,
) -> Result<Option<RunSession>, DbError> {
    let row = sqlx::query(&format!(
        "SELECT {RUN_SESSION_COLUMNS} FROM chat_sessions \
         WHERE principal_id IS NOT NULL \
           AND id = (SELECT session_id FROM chat_turns WHERE id = ?)"
    ))
    .bind(turn_id)
    .fetch_optional(connection)
    .await?;
    row.as_ref().map(map_run_session).transpose()
}

const RUN_SESSION_COLUMNS: &str = "id, principal_id, parent_turn_id, agent_version, visitor_id, \
                                   lang, title, created_at, updated_at";

fn map_run_session(r: &SqliteRow) -> Result<RunSession, DbError> {
    Ok(RunSession {
        id: r.try_get("id")?,
        principal_id: r.try_get("principal_id")?,
        parent_turn_id: r.try_get("parent_turn_id")?,
        agent_version: r.try_get("agent_version")?,
        visitor_id: r.try_get("visitor_id")?,
        lang: r
            .try_get::<Option<String>, _>("lang")?
            .as_deref()
            .and_then(Lang::from_code),
        title: r.try_get("title")?,
        created_at: parse_ts(r.try_get("created_at")?, "created_at")?,
        updated_at: parse_ts(r.try_get("updated_at")?, "updated_at")?,
    })
}

/// Record the language a turn of the principal-owned conversation
/// `session_id` was asked in, so a later resume of it speaks the same one.
pub async fn set_run_lang(pool: &Pool, session_id: &str, lang: Lang) -> Result<(), DbError> {
    sqlx::query("UPDATE chat_sessions SET lang = ? WHERE id = ? AND principal_id IS NOT NULL")
        .bind(lang.code())
        .bind(session_id)
        .execute(pool)
        .await?;
    Ok(())
}

fn owner_of(row: &SqliteRow) -> Result<SessionOwner, DbError> {
    let user: Option<String> = row.try_get("user_id")?;
    let principal: Option<String> = row.try_get("principal_id")?;
    match (user, principal) {
        (Some(user), None) => Ok(SessionOwner::User(user)),
        (None, Some(principal)) => Ok(SessionOwner::Principal(principal)),
        _ => Err(DbError::Decode {
            column: "user_id",
            source: anyhow::anyhow!(
                "a conversation must have exactly one owner (a user or a principal)"
            ),
        }),
    }
}

/// Who owns `session_id`; `None` when it does not exist.
pub async fn session_owner(pool: &Pool, session_id: &str) -> Result<Option<SessionOwner>, DbError> {
    let row = sqlx::query("SELECT user_id, principal_id FROM chat_sessions WHERE id = ?")
        .bind(session_id)
        .fetch_optional(pool)
        .await?;
    row.as_ref().map(owner_of).transpose()
}

/// The top-level suspension of an agent conversation past its deadline.
/// A pause inside a sub-agent run is settled through it, never on its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpiredRunSuspension {
    pub turn_id: String,
    pub session_id: String,
    pub principal_id: String,
    pub on_timeout: TimeoutFallback,
}

/// Every top-level suspension of an agent conversation whose deadline has
/// passed at `now`, oldest deadline first. A sub-agent run's own pause is
/// left out: its parent mirrors it, and is settled through the parent. The
/// counterpart of session-core's `expired_suspensions`, which sweeps only a
/// person's chats.
pub async fn expired_run_suspensions(
    pool: &Pool,
    now: Timestamp,
) -> Result<Vec<ExpiredRunSuspension>, DbError> {
    let rows = sqlx::query(
        r#"SELECT s.turn_id, s.on_timeout, s.expires_at, t.session_id, cs.principal_id
           FROM chat_turn_suspensions s
           JOIN chat_turns t ON t.id = s.turn_id
           JOIN chat_sessions cs ON cs.id = t.session_id
           WHERE cs.principal_id IS NOT NULL AND cs.parent_turn_id IS NULL
             AND s.expires_at < ?"#,
    )
    .bind(deadline_bound(now))
    .fetch_all(pool)
    .await?;
    let mut expired = Vec::new();
    for row in &rows {
        let expires_at = parse_ts(row.try_get("expires_at")?, "expires_at")?;
        if expires_at > now {
            continue;
        }
        let on_timeout: String = row.try_get("on_timeout")?;
        expired.push((
            expires_at,
            ExpiredRunSuspension {
                turn_id: row.try_get("turn_id")?,
                session_id: row.try_get("session_id")?,
                principal_id: row.try_get("principal_id")?,
                on_timeout: TimeoutFallback::parse(&on_timeout)?,
            },
        ));
    }
    expired.sort_by_key(|(at, _)| *at);
    Ok(expired.into_iter().map(|(_, s)| s).collect())
}

/// A conversation's own pause, with what the inbox needs to know about the
/// conversation: who owns it and, for an agent's, which version runs. A
/// pause inside a sub-agent run is never one of these: its conversation's
/// turn mirrors it.
#[derive(Clone, Debug, PartialEq)]
pub struct PendingSuspension {
    pub suspension: TurnSuspension,
    pub session_id: String,
    pub owner: SessionOwner,
    pub agent_version: Option<i64>,
    pub title: Option<String>,
    pub notified_at: Option<Timestamp>,
}

const PENDING_SELECT: &str = "SELECT s.turn_id, s.request_id, s.kind, s.message, s.tool_call, \
     s.tail, s.budget_used, s.child_turn, s.on_timeout, s.expires_at, s.created_at, \
     s.run_context, s.notified_at, t.session_id, cs.user_id, cs.principal_id, \
     cs.agent_version, cs.title \
     FROM chat_turn_suspensions s \
     JOIN chat_turns t ON t.id = s.turn_id \
     JOIN chat_sessions cs ON cs.id = t.session_id \
     WHERE cs.parent_turn_id IS NULL";

fn map_pending(row: &SqliteRow) -> Result<PendingSuspension, DbError> {
    Ok(PendingSuspension {
        suspension: map_suspension(row)?,
        session_id: row.try_get("session_id")?,
        owner: owner_of(row)?,
        agent_version: row.try_get("agent_version")?,
        title: row.try_get("title")?,
        notified_at: parse_optional_ts(row.try_get("notified_at")?, "notified_at")?,
    })
}

/// Every conversation's own pause, oldest first: a person's chat (a
/// scheduled or webhook run included) and an agent conversation alike.
pub async fn pending_suspensions(pool: &Pool) -> Result<Vec<PendingSuspension>, DbError> {
    let rows = sqlx::query(&format!(
        "{PENDING_SELECT} ORDER BY s.created_at, s.turn_id"
    ))
    .fetch_all(pool)
    .await?;
    rows.iter().map(map_pending).collect()
}

/// The conversation's own pause that `request_id` names, if it still waits.
pub async fn pending_by_request(
    pool: &Pool,
    request_id: &str,
) -> Result<Option<PendingSuspension>, DbError> {
    let row = sqlx::query(&format!("{PENDING_SELECT} AND s.request_id = ?"))
        .bind(request_id)
        .fetch_optional(pool)
        .await?;
    row.as_ref().map(map_pending).transpose()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use aiplane_core::server::db::users;
    use session_core::db::suspensions::{BudgetUsed, PendingCall, SuspensionKind};
    use session_core::db::{
        create_assistant_turn_in_progress, create_session, create_user_turn, expired_suspensions,
        insert_running_tool_call, mark_suspension_notified, suspend_turn,
    };

    use super::*;
    use crate::db::system_principals as sp;

    struct Fx {
        pool: Pool,
        principal: String,
    }

    async fn fixture() -> Fx {
        let pool = aiplane_core::server::db::open(Path::new(":memory:"))
            .await
            .unwrap();
        let now = Timestamp::now();
        users::upsert(
            &pool,
            &users::User {
                id: "u1".into(),
                email: "u1@example.com".into(),
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
        let principal = sp::create(
            &pool,
            &sp::NewPrincipal {
                name: "support-website",
                display: "Support website",
                description: "",
            },
            "u1",
        )
        .await
        .unwrap()
        .unwrap()
        .id;
        Fx { pool, principal }
    }

    impl Fx {
        fn run_session<'a>(&'a self, parent_turn_id: Option<&'a str>) -> NewRunSession<'a> {
            NewRunSession {
                principal_id: &self.principal,
                title: Some("visitor asks about invoices"),
                parent_turn_id,
                agent_version: Some(3),
            }
        }

        async fn running_run_turn(&self, turn_id: &str, parent_turn_id: Option<&str>) -> String {
            let run = create_principal_session(
                &self.pool,
                &NewRunSession {
                    agent_version: Some(1),
                    ..self.run_session(parent_turn_id)
                },
            )
            .await
            .unwrap();
            running_turn_in(&self.pool, &run.id, turn_id).await;
            run.id
        }

        async fn running_chat_turn(&self, turn_id: &str) -> String {
            let s = create_session(&self.pool, "u1").await.unwrap();
            running_turn_in(&self.pool, &s.id, turn_id).await;
            s.id
        }
    }

    async fn running_turn_in(pool: &Pool, session_id: &str, turn_id: &str) {
        create_user_turn(pool, session_id, &format!("{turn_id}-u"), "hi")
            .await
            .unwrap();
        create_assistant_turn_in_progress(pool, session_id, turn_id, "m")
            .await
            .unwrap();
        insert_running_tool_call(pool, turn_id, "call-1", "company_echo", "{}")
            .await
            .unwrap();
    }

    async fn completed_turn(pool: &Pool, session_id: &str, text: &str) -> String {
        let id = Uuid::new_v4().to_string();
        create_user_turn(pool, session_id, &id, text).await.unwrap();
        id
    }

    fn suspension(turn_id: &str, expires_at: Timestamp) -> TurnSuspension {
        TurnSuspension {
            turn_id: turn_id.into(),
            request_id: format!("req-{turn_id}"),
            kind: SuspensionKind::Approval,
            message: None,
            tool_call: PendingCall {
                id: "call-1".into(),
                name: "company_echo".into(),
                arguments: r#"{"message":"hi"}"#.into(),
                actions: None,
            },
            tail: vec![serde_json::json!({"role": "assistant", "content": null})],
            budget_used: BudgetUsed {
                rounds: 1,
                seconds: 3,
                tokens: 120,
                tree: None,
            },
            child_turn: None,
            on_timeout: TimeoutFallback::Deny,
            expires_at,
            created_at: Timestamp::now(),
            run_context: None,
        }
    }

    fn in_an_hour() -> Timestamp {
        Timestamp::now() + jiff::SignedDuration::from_hours(1)
    }

    #[tokio::test]
    async fn a_run_session_keeps_its_owner_and_link() {
        let fx = fixture().await;
        let parent = create_principal_session(&fx.pool, &fx.run_session(None))
            .await
            .unwrap();
        let parent_turn = completed_turn(&fx.pool, &parent.id, "route this").await;
        let child = create_principal_session(&fx.pool, &fx.run_session(Some(&parent_turn)))
            .await
            .unwrap();

        let read = get_principal_session(&fx.pool, &fx.principal, &child.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(read, child);
        assert_eq!(read.principal_id, fx.principal);
        assert_eq!(read.parent_turn_id.as_deref(), Some(parent_turn.as_str()));
        assert_eq!(read.agent_version, Some(3));
        assert_eq!(read.title.as_deref(), Some("visitor asks about invoices"));
        assert!(
            get_principal_session(&fx.pool, "p2", &child.id)
                .await
                .unwrap()
                .is_none(),
            "another principal cannot open the run"
        );
    }

    #[tokio::test]
    async fn a_run_session_keeps_the_language_it_was_last_asked_in() {
        let fx = fixture().await;
        let run = create_principal_session(&fx.pool, &fx.run_session(None))
            .await
            .unwrap();
        assert_eq!(run.lang, None);
        set_run_lang(&fx.pool, &run.id, Lang::De).await.unwrap();
        let turn = completed_turn(&fx.pool, &run.id, "Hallo").await;
        let read = run_session_of_turn(&fx.pool, &turn).await.unwrap().unwrap();
        assert_eq!(read.lang, Some(Lang::De));
        set_run_lang(&fx.pool, &run.id, Lang::Fr).await.unwrap();
        let read = get_principal_session(&fx.pool, &fx.principal, &run.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(read.lang, Some(Lang::Fr));
    }

    #[tokio::test]
    async fn a_run_is_found_from_any_of_its_turns_and_a_chat_never_is() {
        let fx = fixture().await;
        let run = create_principal_session(&fx.pool, &fx.run_session(None))
            .await
            .unwrap();
        let run_turn = completed_turn(&fx.pool, &run.id, "route this").await;
        let mine = create_session(&fx.pool, "u1").await.unwrap();
        let chat_turn = completed_turn(&fx.pool, &mine.id, "hello").await;

        assert_eq!(
            run_session_of_turn(&fx.pool, &run_turn).await.unwrap(),
            Some(run)
        );
        assert_eq!(
            run_session_of_turn(&fx.pool, &chat_turn).await.unwrap(),
            None
        );
        assert_eq!(run_session_of_turn(&fx.pool, "nope").await.unwrap(), None);
    }

    #[tokio::test]
    async fn the_owner_of_a_session_is_a_person_or_a_principal() {
        let fx = fixture().await;
        let mine = create_session(&fx.pool, "u1").await.unwrap();
        let run = create_principal_session(&fx.pool, &fx.run_session(None))
            .await
            .unwrap();

        assert_eq!(
            session_owner(&fx.pool, &mine.id).await.unwrap(),
            Some(SessionOwner::User("u1".into()))
        );
        assert_eq!(
            session_owner(&fx.pool, &run.id).await.unwrap(),
            Some(SessionOwner::Principal(fx.principal.clone()))
        );
        assert_eq!(session_owner(&fx.pool, "nope").await.unwrap(), None);
    }

    #[tokio::test]
    async fn a_run_session_for_an_unknown_principal_is_refused() {
        let fx = fixture().await;
        let refused = create_principal_session(
            &fx.pool,
            &NewRunSession {
                principal_id: "ghost",
                ..fx.run_session(None)
            },
        )
        .await;
        assert!(refused.is_err());
    }

    #[tokio::test]
    async fn the_agent_sweep_sees_only_the_top_of_an_agent_conversation() {
        let fx = fixture().await;
        let past = Timestamp::now() - jiff::SignedDuration::from_secs(5);
        fx.running_chat_turn("chat").await;
        let root_session = fx.running_run_turn("root", None).await;
        fx.running_run_turn("child", Some("root")).await;
        fx.running_run_turn("later", None).await;
        for (turn, at) in [
            ("chat", past),
            ("root", past),
            ("child", past),
            ("later", in_an_hour()),
        ] {
            suspend_turn(&fx.pool, &suspension(turn, at)).await.unwrap();
        }

        let expired = expired_run_suspensions(&fx.pool, Timestamp::now())
            .await
            .unwrap();
        assert_eq!(
            expired,
            [ExpiredRunSuspension {
                turn_id: "root".into(),
                session_id: root_session,
                principal_id: fx.principal.clone(),
                on_timeout: TimeoutFallback::Deny,
            }]
        );
    }

    /// A run owned by a system principal has no person to resume as; it is
    /// settled by the agent sweep, never by the chat sweeper.
    #[tokio::test]
    async fn the_chat_sweeper_never_sees_a_principal_owned_run() {
        let fx = fixture().await;
        fx.running_run_turn("agent-turn", None).await;
        let now = Timestamp::now();
        suspend_turn(
            &fx.pool,
            &suspension("agent-turn", now - jiff::SignedDuration::from_secs(5)),
        )
        .await
        .unwrap();

        assert!(expired_suspensions(&fx.pool, now).await.unwrap().is_empty());
        assert_eq!(
            expired_run_suspensions(&fx.pool, now).await.unwrap().len(),
            1
        );
    }

    #[tokio::test]
    async fn the_inbox_lists_each_conversations_own_pause_and_never_a_sub_agents() {
        let fx = fixture().await;
        let chat = fx.running_chat_turn("own").await;
        assert!(
            suspend_turn(&fx.pool, &suspension("own", in_an_hour()))
                .await
                .unwrap()
        );
        let root = fx.running_run_turn("main", None).await;
        let mut parent = suspension("main", in_an_hour());
        parent.child_turn = Some("child".into());
        assert!(suspend_turn(&fx.pool, &parent).await.unwrap());
        fx.running_run_turn("child", Some("main")).await;
        assert!(
            suspend_turn(&fx.pool, &suspension("child", in_an_hour()))
                .await
                .unwrap()
        );

        let pending = pending_suspensions(&fx.pool).await.unwrap();
        let turns: Vec<&str> = pending
            .iter()
            .map(|p| p.suspension.turn_id.as_str())
            .collect();
        assert_eq!(turns.len(), 2, "{turns:?}");
        assert!(turns.contains(&"own") && turns.contains(&"main"));
        let main = pending
            .iter()
            .find(|p| p.suspension.turn_id == "main")
            .unwrap();
        assert_eq!(main.owner, SessionOwner::Principal(fx.principal.clone()));
        assert_eq!(main.session_id, root);
        assert_eq!(main.agent_version, Some(1));
        let own = pending_by_request(&fx.pool, "req-own")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(own.owner, SessionOwner::User("u1".into()));
        assert_eq!(own.session_id, chat);
        assert!(
            pending_by_request(&fx.pool, "req-child")
                .await
                .unwrap()
                .is_none(),
            "a sub-agent's pause is answered through its conversation's"
        );
    }

    #[tokio::test]
    async fn the_inbox_sees_when_a_pause_was_announced() {
        let fx = fixture().await;
        fx.running_chat_turn("a1").await;
        suspend_turn(&fx.pool, &suspension("a1", in_an_hour()))
            .await
            .unwrap();
        let before = pending_by_request(&fx.pool, "req-a1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(before.notified_at, None);
        mark_suspension_notified(&fx.pool, "req-a1", Timestamp::now())
            .await
            .unwrap();
        let after = pending_by_request(&fx.pool, "req-a1")
            .await
            .unwrap()
            .unwrap();
        assert!(after.notified_at.is_some());
    }
}
