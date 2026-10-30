// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Who may read a conversation besides its owner, and the one function that
//! decides it.
//!
//! A reader holds a conversation when they own it, or it is shared by link
//! and its level is within the policy's link ceiling, or they are listed, or
//! they belong to a listed group. The link ceiling is checked on every read,
//! so raising a shared conversation above it stops the link at once. A
//! delegated child conversation is private to its owner whatever else is set.

use aiplane_core::server::classification::Scheme;
use aiplane_core::server::db::users::{self, User};
use aiplane_core::server::db::{DbError, WriteTx};
use serde::{Deserialize, Serialize};
use session_core::db as chat;

use crate::server::AppState;

/// Who is reading: their id, the groups they belong to and the highest rank a
/// link-shared conversation may have for them to read it (`None`: no limit).
pub struct Reader {
    user_id: String,
    group_ids: Vec<String>,
    link_ceiling: Option<u32>,
}

/// The highest rank at which a conversation can still be shared by link; `None`
/// while the scheme is inactive. An empty or unknown configured level is the
/// lowest, like every setting that names a level.
pub fn link_ceiling(state: &AppState) -> Option<u32> {
    let scheme: std::sync::Arc<Scheme> = state.scheme();
    scheme.is_active().then(|| {
        scheme
            .resolve(&state.config().classification.link_share_max_level)
            .rank
    })
}

impl Reader {
    pub fn new(state: &AppState, user: &User) -> Self {
        Self {
            user_id: user.id.clone(),
            group_ids: state.rbac.role_ids_for(&user.roles),
            link_ceiling: link_ceiling(state),
        }
    }
}

/// Whether the reader may read `session_id`.
pub async fn can_read(
    state: &AppState,
    reader: &Reader,
    session_id: &str,
) -> Result<bool, DbError> {
    let groups = serde_json::to_string(&reader.group_ids).unwrap_or_else(|_| "[]".into());
    Ok(sqlx::query_scalar(
        r#"SELECT EXISTS (
             SELECT 1
             FROM chat_sessions s
             LEFT JOIN chat_session_settings cs ON cs.session_id = s.id
             LEFT JOIN classification_levels l ON l.id = cs.level_id
             WHERE s.id = ?1 AND (
               s.user_id = ?2
               OR (s.parent_turn_id IS NULL AND (
                    (s.shared = 1 AND (?3 IS NULL OR COALESCE(l.rank, 0) <= ?3))
                    OR EXISTS (SELECT 1 FROM chat_session_shares sh
                               WHERE sh.session_id = s.id AND sh.subject_kind = 'user'
                                 AND sh.subject_id = ?2)
                    OR EXISTS (SELECT 1 FROM chat_session_shares sh, json_each(?4) g
                               WHERE sh.session_id = s.id AND sh.subject_kind = 'group'
                                 AND sh.subject_id = g.value)
               ))
             )
           )"#,
    )
    .bind(session_id)
    .bind(&reader.user_id)
    .bind(reader.link_ceiling.map(i64::from))
    .bind(groups)
    .fetch_one(&state.db)
    .await?)
}

/// The conversation, when the reader may read it.
pub async fn readable_session(
    state: &AppState,
    reader: &Reader,
    session_id: &str,
) -> Result<Option<chat::Session>, DbError> {
    if !can_read(state, reader, session_id).await? {
        return Ok(None);
    }
    Ok(chat::get_session_by_id(&state.db, session_id).await?)
}

/// Whether the reader may read the conversation `turn_id` belongs to.
pub async fn turn_readable(
    state: &AppState,
    reader: &Reader,
    turn_id: &str,
) -> Result<bool, DbError> {
    match chat::session_id_of_turn(&state.db, turn_id).await? {
        Some(session_id) => can_read(state, reader, &session_id).await,
        None => Ok(false),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SubjectKind {
    User,
    Group,
}

impl SubjectKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Group => "group",
        }
    }
}

/// A person or group a conversation is shared with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subject {
    pub kind: SubjectKind,
    pub id: String,
}

/// How a conversation is shared, as the owner sees it.
pub struct Shares {
    /// Whether the link is open now: shared by link and within the ceiling.
    pub link: bool,
    /// Whether the conversation's level allows a link.
    pub link_allowed: bool,
    pub subjects: Vec<(Subject, String)>,
}

#[derive(Debug, thiserror::Error)]
pub enum ShareError {
    #[error(
        "the conversation is above the highest level that may be shared by link; share it with people or groups instead"
    )]
    LinkAboveCeiling,
    #[error("there is no user `{0}`; pick a user from the search")]
    UnknownUser(String),
    #[error("there is no group `{0}`; pick a group from the search")]
    UnknownGroup(String),
    #[error("the owner already has access; remove them from the list")]
    Owner,
    #[error(transparent)]
    Db(#[from] DbError),
}

async fn level_rank(state: &AppState, session_id: &str) -> Result<u32, DbError> {
    let stored = aiplane_core::server::classification::level_id(&state.db, session_id).await?;
    Ok(state.scheme().level_or_lowest(stored.as_deref()).rank)
}

async fn link_allowed(state: &AppState, session_id: &str) -> Result<bool, DbError> {
    match link_ceiling(state) {
        Some(ceiling) => Ok(level_rank(state, session_id).await? <= ceiling),
        None => Ok(true),
    }
}

/// The shares of an owner's conversation; `None` when `owner` does not own it.
pub async fn shares(
    state: &AppState,
    owner: &str,
    session_id: &str,
) -> Result<Option<Shares>, DbError> {
    let Some(session) = chat::get_session(&state.db, owner, session_id).await? else {
        return Ok(None);
    };
    let link_allowed = link_allowed(state, session_id).await?;
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT subject_kind, subject_id FROM chat_session_shares
         WHERE session_id = ? ORDER BY subject_kind, subject_id",
    )
    .bind(session_id)
    .fetch_all(&state.db)
    .await?;
    let mut subjects = Vec::new();
    for (kind, id) in rows {
        if kind == "group" {
            subjects.push((
                Subject {
                    kind: SubjectKind::Group,
                    id: id.clone(),
                },
                id,
            ));
        } else {
            let name = match users::find_by_id(&state.db, &id).await? {
                Some(user) => user.name.filter(|n| !n.is_empty()).unwrap_or(user.email),
                None => id.clone(),
            };
            subjects.push((
                Subject {
                    kind: SubjectKind::User,
                    id,
                },
                name,
            ));
        }
    }
    Ok(Some(Shares {
        link: session.shared && link_allowed,
        link_allowed,
        subjects,
    }))
}

/// Replace the conversation's shares: the link flag and the listed subjects, in
/// one transaction. `Ok(false)` when `owner` does not own the conversation.
pub async fn replace_shares(
    state: &AppState,
    owner: &str,
    session_id: &str,
    link: bool,
    subjects: &[Subject],
) -> Result<bool, ShareError> {
    if link && !link_allowed(state, session_id).await? {
        return Err(ShareError::LinkAboveCeiling);
    }
    let groups = state.rbac.group_names();
    for subject in subjects {
        match subject.kind {
            SubjectKind::User => {
                if subject.id == owner {
                    return Err(ShareError::Owner);
                }
                if users::find_by_id(&state.db, &subject.id).await?.is_none() {
                    return Err(ShareError::UnknownUser(subject.id.clone()));
                }
            }
            SubjectKind::Group => {
                if !groups.contains(&subject.id) {
                    return Err(ShareError::UnknownGroup(subject.id.clone()));
                }
            }
        }
    }
    let mut tx = WriteTx::begin(&state.db).await?;
    if !chat::set_shared_in(&mut tx, owner, session_id, link)
        .await
        .map_err(DbError::from)?
    {
        return Ok(false);
    }
    sqlx::query("DELETE FROM chat_session_shares WHERE session_id = ?")
        .bind(session_id)
        .execute(&mut *tx)
        .await
        .map_err(DbError::from)?;
    for subject in subjects {
        sqlx::query(
            "INSERT OR IGNORE INTO chat_session_shares (session_id, subject_kind, subject_id)
             VALUES (?, ?, ?)",
        )
        .bind(session_id)
        .bind(subject.kind.as_str())
        .bind(&subject.id)
        .execute(&mut *tx)
        .await
        .map_err(DbError::from)?;
    }
    tx.commit().await?;
    Ok(true)
}

/// Set only the link flag, as the single share toggle does. `Ok(false)` when
/// `owner` does not own the conversation.
pub async fn set_link(
    state: &AppState,
    owner: &str,
    session_id: &str,
    link: bool,
) -> Result<bool, ShareError> {
    if link && !link_allowed(state, session_id).await? {
        return Err(ShareError::LinkAboveCeiling);
    }
    Ok(chat::set_shared(&state.db, owner, session_id, link)
        .await
        .map_err(DbError::from)?)
}
