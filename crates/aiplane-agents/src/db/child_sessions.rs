// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use sqlx::Row;

use super::{
    DbError, Pool,
    agent_sessions::{
        PersonalAgentSession, PersonalChildLink, PersonalChildSession, PersonalChildTurn,
    },
    run_sessions::{self, RunSession},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalChild {
    Principal(RunSession),
    Personal(PersonalChildSession),
}

pub async fn child_of_turn(
    pool: &Pool,
    child_turn_id: &str,
) -> Result<Option<LocalChild>, DbError> {
    let mut connection = pool.acquire().await?;
    child_of_turn_in(&mut connection, child_turn_id).await
}

pub async fn child_of_turn_in(
    connection: &mut sqlx::SqliteConnection,
    child_turn_id: &str,
) -> Result<Option<LocalChild>, DbError> {
    if let Some(session) = run_sessions::run_session_of_turn_in(connection, child_turn_id).await? {
        return Ok(Some(LocalChild::Principal(session)));
    }
    let row = sqlx::query(concat!("SELECT p.session_id, p.owner_user_id, p.agent_id, COALESCE(d.active, 0) AS active,
        p.parent_turn_id, p.parent_tool_call_id, p.caller_agent_id, p.edge_generation
        FROM personal_agent_sessions p JOIN chat_sessions s ON s.id = p.session_id
        JOIN chat_turns t ON t.session_id = s.id AND t.id = ?
        LEFT JOIN agent_owners d ON d.principal_id = p.agent_id AND d.owner_user_id = p.owner_user_id
        WHERE s.user_id = p.owner_user_id AND s.principal_id IS NULL AND ", aiplane_core::unshared_sql!("s"), "
          AND p.parent_turn_id IS NOT NULL AND s.parent_turn_id = p.parent_turn_id AND t.role = 'assistant'"))
        .bind(child_turn_id).fetch_optional(connection).await?;
    row.map(|row| {
        Ok(LocalChild::Personal(PersonalChildSession {
            session: PersonalAgentSession {
                session_id: row.try_get("session_id")?,
                owner_user_id: row.try_get("owner_user_id")?,
                agent_id: row.try_get("agent_id")?,
                active: row.try_get("active")?,
            },
            link: PersonalChildLink {
                parent_turn_id: row.try_get("parent_turn_id")?,
                parent_tool_call_id: row.try_get("parent_tool_call_id")?,
                caller_agent_id: row.try_get("caller_agent_id")?,
                edge_generation: row.try_get("edge_generation")?,
            },
        }))
    })
    .transpose()
}

pub async fn validate_child_in(
    connection: &mut sqlx::SqliteConnection,
    owner_user_id: &str,
    child_turn_id: &str,
    parent_turn_id: &str,
    parent_tool_call_id: &str,
    expected_generation: &str,
) -> Result<bool, DbError> {
    let Some(LocalChild::Personal(child)) = child_of_turn_in(connection, child_turn_id).await?
    else {
        return Ok(false);
    };
    if child.session.owner_user_id != owner_user_id
        || child.link.parent_turn_id != parent_turn_id
        || child.link.parent_tool_call_id != parent_tool_call_id
        || child.link.edge_generation != expected_generation
    {
        return Ok(false);
    }
    let parent_valid: bool = sqlx::query_scalar(concat!("SELECT EXISTS(
        SELECT 1 FROM chat_turns t JOIN chat_sessions s ON s.id = t.session_id
        JOIN personal_agent_sessions p ON p.session_id = s.id
        JOIN chat_tool_calls c ON c.turn_id = t.id AND c.id = ?
        WHERE t.id = ? AND t.role = 'assistant' AND t.status IN ('in_progress', 'suspended')
          AND s.user_id = ? AND s.principal_id IS NULL AND ", aiplane_core::unshared_sql!("s"), "
          AND p.owner_user_id = ? AND p.agent_id = ? AND c.name = 'forward_request' AND c.status = 'running'
    )"))
        .bind(parent_tool_call_id).bind(parent_turn_id).bind(owner_user_id).bind(owner_user_id).bind(&child.link.caller_agent_id)
        .fetch_one(&mut *connection).await?;
    if !parent_valid {
        return Ok(false);
    }
    Ok(super::agent_delegation::authorized_in(
        connection,
        owner_user_id,
        &child.link.caller_agent_id,
        &child.session.agent_id,
        expected_generation,
    )
    .await?
    .is_some())
}

pub async fn root_session_id(
    pool: &Pool,
    owner_user_id: &str,
    session_id: &str,
) -> Result<Option<String>, DbError> {
    let mut connection = pool.acquire().await?;
    let mut current = session_id.to_string();
    for _ in 0..aiplane_core::server::run_chain::MAX_DEPTH {
        let parents: Option<(Option<String>, Option<String>)> = sqlx::query_as(
            concat!("SELECT s.parent_turn_id, p.parent_turn_id FROM chat_sessions s
             LEFT JOIN personal_agent_sessions p ON p.session_id = s.id AND p.owner_user_id = s.user_id
             WHERE s.id = ? AND s.user_id = ? AND s.principal_id IS NULL AND ", aiplane_core::unshared_sql!("s")),
        ).bind(&current).bind(owner_user_id).fetch_optional(&mut *connection).await?;
        let Some((parent, stored_parent)) = parents else {
            return Ok(None);
        };
        if parent.is_none() && stored_parent.is_none() {
            return Ok(Some(current));
        }
        if parent.is_none() || parent != stored_parent {
            return Ok(None);
        }
        let Some((_, next)) = personal_parent_in(&mut connection, owner_user_id, &current).await?
        else {
            return Ok(None);
        };
        current = next;
    }
    Ok(None)
}

async fn personal_parent_in(
    connection: &mut sqlx::SqliteConnection,
    owner_user_id: &str,
    session_id: &str,
) -> Result<Option<(String, String)>, DbError> {
    Ok(sqlx::query_as(
            concat!("SELECT t.id, parent.id FROM personal_agent_sessions child
             JOIN chat_sessions s ON s.id = child.session_id AND s.parent_turn_id = child.parent_turn_id
             JOIN chat_turns t ON t.id = child.parent_turn_id AND t.role = 'assistant'
             JOIN chat_tool_calls c ON c.turn_id = t.id AND c.id = child.parent_tool_call_id AND c.name = 'forward_request'
             JOIN chat_sessions parent ON parent.id = t.session_id
             JOIN personal_agent_sessions caller ON caller.session_id = parent.id
             WHERE child.session_id = ? AND child.owner_user_id = ?
               AND s.user_id = ? AND s.principal_id IS NULL AND ", aiplane_core::unshared_sql!("s"), "
               AND parent.user_id = ? AND parent.principal_id IS NULL
               AND (", aiplane_core::unshared_sql!("parent"), " OR parent.parent_turn_id IS NULL)
               AND caller.owner_user_id = ? AND caller.agent_id = child.caller_agent_id"),
        ).bind(session_id).bind(owner_user_id).bind(owner_user_id).bind(owner_user_id).bind(owner_user_id)
            .fetch_optional(connection).await?)
}

async fn root_turn_in(
    connection: &mut sqlx::SqliteConnection,
    owner_user_id: &str,
    turn_id: &str,
) -> Result<Option<String>, DbError> {
    let mut current = turn_id.to_string();
    for _ in 0..aiplane_core::server::run_chain::MAX_DEPTH {
        let row: Option<(String, Option<String>, Option<String>)> = sqlx::query_as(
            concat!("SELECT s.id, s.parent_turn_id, p.parent_turn_id FROM chat_turns t
             JOIN chat_sessions s ON s.id = t.session_id
             JOIN personal_agent_sessions p ON p.session_id = s.id AND p.owner_user_id = s.user_id
             WHERE t.id = ? AND t.role = 'assistant' AND s.user_id = ? AND s.principal_id IS NULL AND (", aiplane_core::unshared_sql!("s"), " OR s.parent_turn_id IS NULL)"),
        ).bind(&current).bind(owner_user_id).fetch_optional(&mut *connection).await?;
        let Some((session, parent, stored_parent)) = row else {
            return Ok(None);
        };
        if parent.is_none() && stored_parent.is_none() {
            return Ok(Some(current));
        }
        if parent.is_none() || parent != stored_parent {
            return Ok(None);
        }
        let Some((next, _)) = personal_parent_in(connection, owner_user_id, &session).await? else {
            return Ok(None);
        };
        current = next;
    }
    Ok(None)
}

pub async fn active_children(
    pool: &Pool,
    owner_user_id: &str,
    parent_turn_id: &str,
) -> Result<Vec<PersonalChildTurn>, DbError> {
    Ok(child_turns(pool, owner_user_id, parent_turn_id)
        .await?
        .into_iter()
        .filter(|child| !child.assistant_turn.status.is_terminal())
        .collect())
}

pub async fn child_turns(
    pool: &Pool,
    owner_user_id: &str,
    parent_turn_id: &str,
) -> Result<Vec<PersonalChildTurn>, DbError> {
    let mut connection = pool.acquire().await?;
    let ids: Vec<String> = sqlx::query_scalar(concat!(
        "SELECT t.id FROM personal_agent_sessions p JOIN chat_sessions s ON s.id = p.session_id
         JOIN chat_turns t ON t.session_id = s.id
         WHERE p.owner_user_id = ? AND s.user_id = ? AND s.principal_id IS NULL AND ",
        aiplane_core::unshared_sql!("s"),
        "
           AND p.parent_turn_id = ? AND t.role = 'assistant' ORDER BY t.id"
    ))
    .bind(owner_user_id)
    .bind(owner_user_id)
    .bind(parent_turn_id)
    .fetch_all(&mut *connection)
    .await?;
    let mut verified = Vec::new();
    for id in ids {
        let Some(LocalChild::Personal(child)) = child_of_turn_in(&mut connection, &id).await?
        else {
            continue;
        };
        if personal_parent_in(&mut connection, owner_user_id, &child.session.session_id)
            .await?
            .is_some_and(|(parent, _)| parent == parent_turn_id)
        {
            verified.push((id, child));
        }
    }
    drop(connection);
    let mut children = Vec::new();
    for (id, child) in verified {
        if let Some(turn) = session_core::db::get_turn(pool, &child.session.session_id, &id).await?
        {
            children.push(PersonalChildTurn {
                session: child.session,
                assistant_turn: turn,
                link: child.link,
            });
        }
    }
    Ok(children)
}

const ACTIVE_PERSONAL_PATHS: &str = concat!("WITH RECURSIVE paths(leaf_turn_id, turn_id, depth) AS (
 SELECT t.id, t.id, 0 FROM personal_agent_sessions p
 JOIN chat_sessions s ON s.id = p.session_id JOIN chat_turns t ON t.session_id = s.id
 WHERE p.owner_user_id = ? AND s.user_id = ? AND s.principal_id IS NULL
 AND (", aiplane_core::unshared_sql!("s"), " OR s.parent_turn_id IS NULL)
 AND t.role = 'assistant' AND t.status IN ('in_progress', 'suspended')
 UNION ALL
 SELECT path.leaf_turn_id, p.parent_turn_id, path.depth + 1 FROM paths path
 JOIN chat_turns t ON t.id = path.turn_id JOIN personal_agent_sessions p ON p.session_id = t.session_id
 WHERE p.owner_user_id = ? AND p.parent_turn_id IS NOT NULL AND path.depth < ?
 ) SELECT DISTINCT path.leaf_turn_id FROM paths path
 JOIN chat_turns t ON t.id = path.turn_id JOIN personal_agent_sessions p ON p.session_id = t.session_id
 WHERE p.owner_user_id = ?");

async fn roots_for_turns(
    pool: &Pool,
    owner_user_id: &str,
    turns: Vec<String>,
) -> Result<Vec<String>, DbError> {
    let mut connection = pool.acquire().await?;
    let mut roots = std::collections::BTreeSet::new();
    for turn in turns {
        if let Some(root) = root_turn_in(&mut connection, owner_user_id, &turn).await? {
            roots.insert(root);
        }
    }
    Ok(roots.into_iter().collect())
}

pub async fn active_roots_for_agent(
    pool: &Pool,
    owner_user_id: &str,
    agent_id: &str,
) -> Result<Vec<String>, DbError> {
    let turns = sqlx::query_scalar(&format!(
        "{ACTIVE_PERSONAL_PATHS} AND (p.agent_id = ? OR p.caller_agent_id = ?)"
    ))
    .bind(owner_user_id)
    .bind(owner_user_id)
    .bind(owner_user_id)
    .bind(aiplane_core::server::run_chain::MAX_DEPTH as i64)
    .bind(owner_user_id)
    .bind(agent_id)
    .bind(agent_id)
    .fetch_all(pool)
    .await?;
    roots_for_turns(pool, owner_user_id, turns).await
}

pub async fn active_roots_for_edge(
    pool: &Pool,
    owner_user_id: &str,
    caller_agent_id: &str,
    receiver_agent_id: &str,
    old_generation: &str,
) -> Result<Vec<String>, DbError> {
    let turns = sqlx::query_scalar(&format!(
        "{ACTIVE_PERSONAL_PATHS} AND p.caller_agent_id = ? AND p.agent_id = ? AND p.edge_generation = ?"
    ))
    .bind(owner_user_id)
    .bind(owner_user_id)
    .bind(owner_user_id)
    .bind(aiplane_core::server::run_chain::MAX_DEPTH as i64)
    .bind(owner_user_id)
    .bind(caller_agent_id)
    .bind(receiver_agent_id)
    .bind(old_generation)
    .fetch_all(pool)
    .await?;
    roots_for_turns(pool, owner_user_id, turns).await
}
