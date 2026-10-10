// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Durable memories owned by one personal Dot.

use aiplane_core::server::db::user_memories::{Memory, MemoryKind};
use jiff::Timestamp;
use sqlx::Row;
use sqlx::sqlite::SqliteRow;
use uuid::Uuid;

use super::{DbError, Pool};

fn map_row(row: &SqliteRow) -> Result<Memory, DbError> {
    let kind: String = row.try_get("kind")?;
    let created_at: String = row.try_get("created_at")?;
    Ok(Memory {
        id: row.try_get("id")?,
        kind: MemoryKind::parse(&kind).unwrap_or(MemoryKind::Fact),
        content: row.try_get("content")?,
        created_at: created_at
            .parse()
            .map_err(|e: jiff::Error| DbError::Decode {
                column: "created_at",
                source: e.into(),
            })?,
    })
}

/// List memories only when `owner_user_id` owns the personal Dot.
pub async fn list(
    pool: &Pool,
    owner_user_id: &str,
    dot_id: &str,
    limit: i64,
) -> Result<Option<Vec<Memory>>, DbError> {
    let owned = sqlx::query_scalar::<_, i64>(
        "SELECT EXISTS(SELECT 1 FROM personal_dots WHERE principal_id = ? AND owner_user_id = ?)",
    )
    .bind(dot_id)
    .bind(owner_user_id)
    .fetch_one(pool)
    .await?;
    if owned == 0 {
        return Ok(None);
    }
    let rows = sqlx::query(
        "SELECT m.id, m.kind, m.content, m.created_at FROM personal_dot_memories m
         JOIN personal_dots d ON d.principal_id = m.dot_id
         WHERE d.owner_user_id = ? AND d.principal_id = ?
         ORDER BY m.created_at DESC LIMIT ?",
    )
    .bind(owner_user_id)
    .bind(dot_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(map_row)
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

/// Store Dot-scoped memory, collapsing exact duplicates within that Dot.
pub async fn insert(
    pool: &Pool,
    owner_user_id: &str,
    dot_id: &str,
    kind: MemoryKind,
    content: &str,
) -> Result<Option<Memory>, DbError> {
    let owned = sqlx::query_scalar::<_, i64>(
        "SELECT EXISTS(SELECT 1 FROM personal_dots WHERE principal_id = ? AND owner_user_id = ?)",
    )
    .bind(dot_id)
    .bind(owner_user_id)
    .fetch_one(pool)
    .await?;
    if owned == 0 {
        return Ok(None);
    }
    let now = Timestamp::now();
    let now_s = now.to_string();
    let row = sqlx::query(
        "INSERT INTO personal_dot_memories (id, dot_id, kind, content, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT (dot_id, content) DO UPDATE SET
           kind = excluded.kind, updated_at = excluded.updated_at
         RETURNING id, kind, content, created_at",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(dot_id)
    .bind(kind.as_str())
    .bind(content)
    .bind(&now_s)
    .bind(&now_s)
    .fetch_one(pool)
    .await?;
    map_row(&row).map(Some)
}

/// Update a memory within an owned personal Dot.
pub async fn update(
    pool: &Pool,
    owner_user_id: &str,
    dot_id: &str,
    id: &str,
    kind: MemoryKind,
    content: &str,
) -> Result<Option<Memory>, DbError> {
    let now = Timestamp::now().to_string();
    let affected = sqlx::query(
        "UPDATE personal_dot_memories SET kind = ?, content = ?, updated_at = ?
         WHERE id = ? AND dot_id = ? AND EXISTS (
           SELECT 1 FROM personal_dots d WHERE d.principal_id = ? AND d.owner_user_id = ?)",
    )
    .bind(kind.as_str())
    .bind(content)
    .bind(now)
    .bind(id)
    .bind(dot_id)
    .bind(dot_id)
    .bind(owner_user_id)
    .execute(pool)
    .await?
    .rows_affected();
    if affected == 0 {
        return Ok(None);
    }
    let row =
        sqlx::query("SELECT id, kind, content, created_at FROM personal_dot_memories WHERE id = ?")
            .bind(id)
            .fetch_one(pool)
            .await?;
    map_row(&row).map(Some)
}

/// Delete a memory from an owned personal Dot.
pub async fn delete(
    pool: &Pool,
    owner_user_id: &str,
    dot_id: &str,
    id: &str,
) -> Result<bool, DbError> {
    let affected = sqlx::query(
        "DELETE FROM personal_dot_memories WHERE id = ? AND dot_id = ? AND EXISTS (
           SELECT 1 FROM personal_dots d WHERE d.principal_id = ? AND d.owner_user_id = ?)",
    )
    .bind(id)
    .bind(dot_id)
    .bind(dot_id)
    .bind(owner_user_id)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(affected > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::agents::{self, PersonalDotCreate};
    use crate::db::system_principals::NewPrincipal;
    use aiplane_core::server::db::open;
    use aiplane_core::server::db::user_memories;
    use std::path::Path;

    async fn fixture() -> (Pool, String) {
        let pool = open(Path::new(":memory:")).await.unwrap();
        let (result, dot) = agents::create_personal_dot(
            &pool,
            &NewPrincipal {
                name: "helper",
                display: "Helper",
                description: "",
            },
            r#"{"main":{}}"#,
            "alice",
            4,
        )
        .await
        .unwrap();
        assert_eq!(result, PersonalDotCreate::Created);
        (pool, dot.unwrap().principal.id)
    }

    #[tokio::test]
    async fn dot_memories_are_isolated_from_users_and_other_dots() {
        let (pool, dot) = fixture().await;
        let first = insert(&pool, "alice", &dot, MemoryKind::Fact, "learned pitfall")
            .await
            .unwrap()
            .unwrap();
        let duplicate = insert(&pool, "alice", &dot, MemoryKind::Project, "learned pitfall")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(duplicate.id, first.id);
        assert_eq!(duplicate.created_at, first.created_at);
        assert_eq!(duplicate.kind, MemoryKind::Project);
        user_memories::insert(&pool, "alice", MemoryKind::Preference, "German answers")
            .await
            .unwrap();
        let rows = list(&pool, "alice", &dot, 20).await.unwrap().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].content, "learned pitfall");
        assert!(list(&pool, "bob", &dot, 20).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn dot_memory_crud_is_owner_scoped() {
        let (pool, dot) = fixture().await;
        let item = insert(&pool, "alice", &dot, MemoryKind::Fact, "first")
            .await
            .unwrap()
            .unwrap();
        assert!(
            update(&pool, "bob", &dot, &item.id, MemoryKind::Fact, "hacked")
                .await
                .unwrap()
                .is_none()
        );
        let changed = update(
            &pool,
            "alice",
            &dot,
            &item.id,
            MemoryKind::Project,
            "second",
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(changed.content, "second");
        assert!(!delete(&pool, "bob", &dot, &item.id).await.unwrap());
        assert!(delete(&pool, "alice", &dot, &item.id).await.unwrap());
        assert!(
            list(&pool, "alice", &dot, 20)
                .await
                .unwrap()
                .unwrap()
                .is_empty()
        );
    }
}
