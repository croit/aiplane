// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use aiplane_core::server::db::{
    DbError, Pool, WriteTx,
    mcp_connections::{self, ConnectionOwner},
    parse_ts,
};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use sqlx::{Executor, Row, Sqlite};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityMode {
    User,
    Principal,
}
impl IdentityMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Principal => "principal",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DotConnectorBinding {
    pub dot_id: String,
    pub connector_key: String,
    pub identity_mode: IdentityMode,
    pub connection_generation: String,
    pub binding_generation: String,
    pub updated_at: Timestamp,
}
#[derive(Clone, Copy, Debug)]
pub struct Selection<'a> {
    pub owner_user_id: &'a str,
    pub dot_id: &'a str,
    pub connector_key: &'a str,
    pub identity_mode: IdentityMode,
    pub connection_generation: &'a str,
    pub expected_binding_generation: Option<&'a str>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaveOutcome {
    Saved(Option<DotConnectorBinding>),
    Missing,
    Unavailable,
    Conflict,
}

pub async fn owned_dot_in(
    tx: &mut WriteTx,
    owner: &str,
    dot: &str,
    require_active: bool,
) -> Result<bool, DbError> {
    Ok(sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM personal_dots d JOIN agents a ON a.principal_id=d.principal_id JOIN system_principals p ON p.id=d.principal_id WHERE d.owner_user_id=? AND d.principal_id=? AND (?=0 OR (d.active=1 AND p.disabled_at IS NULL)))").bind(owner).bind(dot).bind(require_active).fetch_one(&mut **tx).await?)
}

async fn load<'e, E: Executor<'e, Database = Sqlite>>(
    executor: E,
    owner: &str,
    dot: &str,
    key: &str,
) -> Result<Option<DotConnectorBinding>, DbError> {
    let row = sqlx::query("SELECT b.* FROM dot_connector_bindings b JOIN personal_dots d ON d.principal_id=b.dot_id JOIN agents a ON a.principal_id=d.principal_id WHERE d.owner_user_id=? AND b.dot_id=? AND b.connector_key=?").bind(owner).bind(dot).bind(key).fetch_optional(executor).await?;
    row.map(map_binding).transpose()
}
fn map_binding(row: sqlx::sqlite::SqliteRow) -> Result<DotConnectorBinding, DbError> {
    let mode: String = row.try_get("identity_mode")?;
    let identity_mode = match mode.as_str() {
        "user" => IdentityMode::User,
        "principal" => IdentityMode::Principal,
        _ => {
            return Err(DbError::Decode {
                column: "identity_mode",
                source: std::io::Error::other("Invalid saved connector identity mode.").into(),
            });
        }
    };
    Ok(DotConnectorBinding {
        dot_id: row.try_get("dot_id")?,
        connector_key: row.try_get("connector_key")?,
        identity_mode,
        connection_generation: row.try_get("connection_generation")?,
        binding_generation: row.try_get("binding_generation")?,
        updated_at: parse_ts(row.try_get("updated_at")?, "updated_at")?,
    })
}
pub async fn list_for_dot(
    pool: &Pool,
    owner: &str,
    dot: &str,
) -> Result<Vec<DotConnectorBinding>, DbError> {
    let rows = sqlx::query("SELECT b.* FROM dot_connector_bindings b JOIN personal_dots d ON d.principal_id=b.dot_id JOIN agents a ON a.principal_id=d.principal_id WHERE d.owner_user_id=? AND b.dot_id=? ORDER BY b.connector_key").bind(owner).bind(dot).fetch_all(pool).await?;
    rows.into_iter().map(map_binding).collect()
}
pub async fn get(
    pool: &Pool,
    owner: &str,
    dot: &str,
    key: &str,
) -> Result<Option<DotConnectorBinding>, DbError> {
    load(pool, owner, dot, key).await
}
pub async fn get_in(
    tx: &mut WriteTx,
    owner: &str,
    dot: &str,
    key: &str,
) -> Result<Option<DotConnectorBinding>, DbError> {
    load(&mut **tx, owner, dot, key).await
}

pub async fn save_in(tx: &mut WriteTx, new: Selection<'_>) -> Result<SaveOutcome, DbError> {
    if !owned_dot_in(tx, new.owner_user_id, new.dot_id, false).await? {
        return Ok(SaveOutcome::Missing);
    }
    let saved = get_in(tx, new.owner_user_id, new.dot_id, new.connector_key).await?;
    if saved.as_ref().map(|b| b.binding_generation.as_str()) != new.expected_binding_generation {
        return Ok(SaveOutcome::Conflict);
    }
    let owner = match new.identity_mode {
        IdentityMode::User => ConnectionOwner::User(new.owner_user_id.to_owned()),
        IdentityMode::Principal => ConnectionOwner::Principal(new.dot_id.to_owned()),
    };
    let Some(connection) =
        mcp_connections::get_connection_in(tx, &owner, new.connector_key).await?
    else {
        return Ok(SaveOutcome::Unavailable);
    };
    if connection.is_errored()
        || connection.access_token_ct.is_none()
        || connection.access_token_nonce.is_none()
        || connection.generation != new.connection_generation
    {
        return Ok(SaveOutcome::Unavailable);
    }
    if saved.as_ref().is_some_and(|b| {
        b.identity_mode == new.identity_mode && b.connection_generation == new.connection_generation
    }) || (saved.is_none() && new.identity_mode == IdentityMode::User)
    {
        return Ok(SaveOutcome::Saved(saved));
    }
    let binding = DotConnectorBinding {
        dot_id: new.dot_id.to_owned(),
        connector_key: new.connector_key.to_owned(),
        identity_mode: new.identity_mode,
        connection_generation: new.connection_generation.to_owned(),
        binding_generation: uuid::Uuid::new_v4().to_string(),
        updated_at: Timestamp::now(),
    };
    sqlx::query("INSERT INTO dot_connector_bindings(dot_id,connector_key,identity_mode,connection_generation,binding_generation,updated_at) VALUES(?,?,?,?,?,?) ON CONFLICT(dot_id,connector_key) DO UPDATE SET identity_mode=excluded.identity_mode,connection_generation=excluded.connection_generation,binding_generation=excluded.binding_generation,updated_at=excluded.updated_at").bind(&binding.dot_id).bind(&binding.connector_key).bind(binding.identity_mode.as_str()).bind(&binding.connection_generation).bind(&binding.binding_generation).bind(binding.updated_at.to_string()).execute(&mut **tx).await?;
    Ok(SaveOutcome::Saved(Some(binding)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{agents, system_principals::NewPrincipal};
    use aiplane_core::server::db::{
        self, WriteTx,
        mcp_connections::{self, ConnectionOwner, NewConnection},
    };

    async fn fixture() -> (db::Pool, String, String, String) {
        let pool = db::open(std::path::Path::new(":memory:")).await.unwrap();
        sqlx::query("INSERT INTO users(id,email,roles_json,created_at,updated_at) VALUES('owner','owner@example.com','[]','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z'),('other','other@example.com','[]','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO mcp_catalog_connectors(key,name,url,enabled,created_at,updated_at) VALUES('fixture','Fixture','https://provider.test/mcp',1,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')").execute(&pool).await.unwrap();
        let (_, dot) = agents::create_personal_dot(
            &pool,
            &NewPrincipal {
                name: "Caller",
                display: "Caller",
                description: "",
            },
            "{}",
            "owner",
            4,
        )
        .await
        .unwrap();
        let dot = dot.unwrap().principal.id;
        let catalog_generation = db::mcp_catalog::get(&pool, "fixture")
            .await
            .unwrap()
            .unwrap()
            .dispatch_generation;
        let credential = |owner| NewConnection {
            owner,
            connector_key: "fixture".into(),
            catalog_generation: catalog_generation.clone(),
            access_token_ct: vec![1],
            access_token_nonce: vec![2],
            refresh_token_ct: None,
            refresh_token_nonce: None,
            token_expires_at: None,
            scopes: Vec::new(),
            dcr_client_id: None,
            dcr_client_secret_ct: None,
            dcr_client_secret_nonce: None,
            token_url: None,
        };
        let user = mcp_connections::upsert_connection(
            &pool,
            credential(ConnectionOwner::User("owner".into())),
        )
        .await
        .unwrap()
        .unwrap();
        let own = mcp_connections::upsert_connection(
            &pool,
            credential(ConnectionOwner::Principal(dot.clone())),
        )
        .await
        .unwrap()
        .unwrap();
        (pool, dot, user.generation, own.generation)
    }

    #[tokio::test]
    async fn implicit_user_selection_is_a_noop_and_switch_back_retains_a_real_generation() {
        let (pool, dot, user_generation, own_generation) = fixture().await;
        let mut tx = WriteTx::begin(&pool).await.unwrap();
        let selection =
            |identity_mode, connection_generation, expected_binding_generation| Selection {
                owner_user_id: "owner",
                dot_id: &dot,
                connector_key: "fixture",
                identity_mode,
                connection_generation,
                expected_binding_generation,
            };
        assert_eq!(
            save_in(
                &mut tx,
                selection(IdentityMode::User, &user_generation, None)
            )
            .await
            .unwrap(),
            SaveOutcome::Saved(None)
        );
        assert!(
            get_in(&mut tx, "owner", &dot, "fixture")
                .await
                .unwrap()
                .is_none()
        );
        let SaveOutcome::Saved(Some(own)) = save_in(
            &mut tx,
            selection(IdentityMode::Principal, &own_generation, None),
        )
        .await
        .unwrap() else {
            panic!("own identity saved");
        };
        assert_eq!(own.identity_mode, IdentityMode::Principal);
        assert_eq!(
            save_in(
                &mut tx,
                selection(
                    IdentityMode::Principal,
                    &own_generation,
                    Some(&own.binding_generation)
                )
            )
            .await
            .unwrap(),
            SaveOutcome::Saved(Some(own.clone()))
        );
        let SaveOutcome::Saved(Some(user)) = save_in(
            &mut tx,
            selection(
                IdentityMode::User,
                &user_generation,
                Some(&own.binding_generation),
            ),
        )
        .await
        .unwrap() else {
            panic!("explicit User identity saved");
        };
        assert_ne!(own.binding_generation, user.binding_generation);
        assert_eq!(user.identity_mode, IdentityMode::User);
        assert_eq!(
            save_in(
                &mut tx,
                selection(
                    IdentityMode::Principal,
                    &own_generation,
                    Some(&own.binding_generation)
                )
            )
            .await
            .unwrap(),
            SaveOutcome::Conflict
        );
        tx.commit().await.unwrap();
        assert_eq!(
            get(&pool, "owner", &dot, "fixture").await.unwrap(),
            Some(user.clone())
        );
        assert_eq!(
            list_for_dot(&pool, "owner", &dot).await.unwrap(),
            vec![user]
        );
        assert!(list_for_dot(&pool, "other", &dot).await.unwrap().is_empty());
        assert!(
            get(&pool, "other", &dot, "fixture")
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn selected_connection_generation_is_validated_in_the_write_transaction() {
        let (pool, dot, _, own_generation) = fixture().await;
        let mut tx = WriteTx::begin(&pool).await.unwrap();
        let selection = |generation| Selection {
            owner_user_id: "owner",
            dot_id: &dot,
            connector_key: "fixture",
            identity_mode: IdentityMode::Principal,
            connection_generation: generation,
            expected_binding_generation: None,
        };
        assert_eq!(
            save_in(&mut tx, selection("stale")).await.unwrap(),
            SaveOutcome::Unavailable
        );
        assert!(
            get_in(&mut tx, "owner", &dot, "fixture")
                .await
                .unwrap()
                .is_none()
        );
        sqlx::query("UPDATE mcp_connections SET status='reauth' WHERE principal_id=?")
            .bind(&dot)
            .execute(&mut *tx)
            .await
            .unwrap();
        assert_eq!(
            save_in(&mut tx, selection(&own_generation)).await.unwrap(),
            SaveOutcome::Unavailable
        );
        assert!(
            get_in(&mut tx, "owner", &dot, "fixture")
                .await
                .unwrap()
                .is_none()
        );
        tx.commit().await.unwrap();
    }

    #[tokio::test]
    async fn inactive_configuration_is_owned_but_cannot_authorize_execution() {
        let (pool, dot, _, _) = fixture().await;
        let mut tx = WriteTx::begin(&pool).await.unwrap();
        assert!(owned_dot_in(&mut tx, "owner", &dot, true).await.unwrap());
        assert!(!owned_dot_in(&mut tx, "other", &dot, false).await.unwrap());
        sqlx::query("UPDATE personal_dots SET active=0 WHERE principal_id=?")
            .bind(&dot)
            .execute(&mut *tx)
            .await
            .unwrap();
        assert!(owned_dot_in(&mut tx, "owner", &dot, false).await.unwrap());
        assert!(!owned_dot_in(&mut tx, "owner", &dot, true).await.unwrap());
        sqlx::query("UPDATE personal_dots SET active=1 WHERE principal_id=?")
            .bind(&dot)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("UPDATE system_principals SET disabled_at=? WHERE id=?")
            .bind(jiff::Timestamp::now().to_string())
            .bind(&dot)
            .execute(&mut *tx)
            .await
            .unwrap();
        assert!(owned_dot_in(&mut tx, "owner", &dot, false).await.unwrap());
        assert!(!owned_dot_in(&mut tx, "owner", &dot, true).await.unwrap());
        tx.commit().await.unwrap();
    }
}
