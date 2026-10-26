// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The operator-uploaded company logo, one row per theme slot.
//!
//! The bytes are the stored, normalised image — raster uploads are resized
//! before they land here, so serving reads a single column. Schema lives in
//! `migrations/0086_branding.sql`.

use jiff::Timestamp;
use sqlx::Row;

use super::{DbError, Pool};

/// The theme slots a logo can be set for.
pub const SLOTS: [&str; 2] = ["light", "dark"];

/// True when `slot` is one of [`SLOTS`].
pub fn is_slot(slot: &str) -> bool {
    SLOTS.contains(&slot)
}

/// A stored logo: the image bytes and the content type to serve them with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Logo {
    pub bytes: Vec<u8>,
    pub content_type: String,
}

/// Read the logo for `slot`. `None` means no logo is set for that slot.
pub async fn get(pool: &Pool, slot: &str) -> Result<Option<Logo>, DbError> {
    let row = sqlx::query("SELECT bytes, content_type FROM branding WHERE slot = ?")
        .bind(slot)
        .fetch_optional(pool)
        .await?;
    row.map(|r| {
        Ok(Logo {
            bytes: r.try_get::<Vec<u8>, _>("bytes")?,
            content_type: r.try_get::<String, _>("content_type")?,
        })
    })
    .transpose()
}

/// Store the logo for `slot`, overwriting any previous one.
pub async fn set(
    pool: &Pool,
    slot: &str,
    bytes: Vec<u8>,
    content_type: &str,
) -> Result<(), DbError> {
    let now = Timestamp::now().to_string();
    sqlx::query(
        r#"INSERT INTO branding (slot, bytes, content_type, updated_at)
           VALUES (?, ?, ?, ?)
           ON CONFLICT(slot) DO UPDATE SET
             bytes        = excluded.bytes,
             content_type = excluded.content_type,
             updated_at   = excluded.updated_at"#,
    )
    .bind(slot)
    .bind(&bytes)
    .bind(content_type)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

/// Drop the logo for `slot`. No-op if it didn't exist.
pub async fn delete(pool: &Pool, slot: &str) -> Result<(), DbError> {
    sqlx::query("DELETE FROM branding WHERE slot = ?")
        .bind(slot)
        .execute(pool)
        .await?;
    Ok(())
}

/// Which slots currently carry a logo.
pub async fn set_slots(pool: &Pool) -> Result<Vec<String>, DbError> {
    let rows = sqlx::query("SELECT slot FROM branding ORDER BY slot")
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(|r| r.try_get::<String, _>("slot"))
        .collect::<Result<_, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::db::open;

    async fn fresh() -> Pool {
        open(std::path::Path::new(":memory:")).await.unwrap()
    }

    #[tokio::test]
    async fn get_returns_none_when_unset() {
        let pool = fresh().await;
        assert!(get(&pool, "light").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn set_then_get_round_trips() {
        let pool = fresh().await;
        set(&pool, "light", vec![1, 2, 3], "image/png")
            .await
            .unwrap();
        let logo = get(&pool, "light").await.unwrap().unwrap();
        assert_eq!(logo.bytes, vec![1, 2, 3]);
        assert_eq!(logo.content_type, "image/png");
        assert_eq!(set_slots(&pool).await.unwrap(), vec!["light".to_string()]);
    }

    #[tokio::test]
    async fn set_overwrites_the_slot() {
        let pool = fresh().await;
        set(&pool, "dark", vec![1], "image/svg+xml").await.unwrap();
        set(&pool, "dark", vec![2, 2], "image/png").await.unwrap();
        let logo = get(&pool, "dark").await.unwrap().unwrap();
        assert_eq!(logo.bytes, vec![2, 2]);
        assert_eq!(logo.content_type, "image/png");
        assert_eq!(set_slots(&pool).await.unwrap(), vec!["dark".to_string()]);
    }

    #[tokio::test]
    async fn delete_removes_and_is_idempotent() {
        let pool = fresh().await;
        set(&pool, "light", vec![9], "image/png").await.unwrap();
        delete(&pool, "light").await.unwrap();
        delete(&pool, "light").await.unwrap();
        assert!(get(&pool, "light").await.unwrap().is_none());
    }

    #[test]
    fn is_slot_accepts_only_the_two_theme_slots() {
        assert!(is_slot("light"));
        assert!(is_slot("dark"));
        assert!(!is_slot("bogus"));
        assert!(!is_slot(""));
    }
}
