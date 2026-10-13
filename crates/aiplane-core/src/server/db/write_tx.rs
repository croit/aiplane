// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! A transaction that holds SQLite's write lock from its first statement.
//!
//! What reads a value and then writes based on it — a hash chain's head, a
//! rate window's count, a slot's old value — is only correct when no other
//! writer can slip in between. SQLite gives that to a transaction holding
//! the write lock, and a deferred one (`BEGIN`) takes it only at its first
//! write, after the reads. [`WriteTx`] can only be made by `BEGIN IMMEDIATE`,
//! so a function that takes one cannot be handed a transaction whose reads
//! are unprotected. Use it as the connection (`&mut *tx`) for queries.

use std::ops::{Deref, DerefMut};

use sqlx::{Sqlite, SqliteConnection, Transaction};

use super::{DbError, Pool};

pub struct WriteTx(Transaction<'static, Sqlite>);

impl WriteTx {
    /// Begin a transaction that takes the write lock first, waiting at most
    /// the pool's busy timeout for it.
    pub async fn begin(pool: &Pool) -> Result<Self, DbError> {
        Ok(Self(pool.begin_with("BEGIN IMMEDIATE").await?))
    }

    pub async fn commit(self) -> Result<(), DbError> {
        self.0.commit().await?;
        Ok(())
    }
}

impl Deref for WriteTx {
    type Target = SqliteConnection;

    fn deref(&self) -> &SqliteConnection {
        &self.0
    }
}

impl DerefMut for WriteTx {
    fn deref_mut(&mut self) -> &mut SqliteConnection {
        &mut self.0
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    /// A second writer waits for the first to commit instead of reading
    /// beside it: the lock is held from `begin`, before any write.
    #[tokio::test]
    async fn the_write_lock_is_held_from_the_first_statement() {
        let dir = std::env::temp_dir().join(format!("write-tx-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let pool = crate::server::db::open(&dir.join("db.sqlite"))
            .await
            .unwrap();
        let first = WriteTx::begin(&pool).await.unwrap();
        let second = tokio::time::timeout(Duration::from_millis(300), async {
            let mut tx = WriteTx::begin(&pool).await?;
            sqlx::query("SELECT 1").execute(&mut *tx).await?;
            Ok::<_, DbError>(())
        })
        .await;
        assert!(second.is_err(), "the second writer must wait for the first");
        first.commit().await.unwrap();
        let mut again = WriteTx::begin(&pool).await.unwrap();
        sqlx::query("SELECT 1").execute(&mut *again).await.unwrap();
        again.commit().await.unwrap();
        drop(pool);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
