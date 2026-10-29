// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use super::agents::AgentOwner;

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
pub struct AgentConnectorBinding {
    pub agent_id: String,
    pub connector_key: String,
    pub identity_mode: IdentityMode,
    pub connection_generation: String,
    pub binding_generation: String,
    pub updated_at: Timestamp,
}
#[derive(Clone, Copy, Debug)]
pub struct Selection<'a> {
    pub owner: &'a AgentOwner,
    pub agent_id: &'a str,
    pub connector_key: &'a str,
    pub identity_mode: IdentityMode,
    pub connection_generation: &'a str,
    pub expected_binding_generation: Option<&'a str>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaveOutcome {
    Saved(Option<AgentConnectorBinding>),
    Missing,
    Unavailable,
    Conflict,
}

pub async fn owned_agent_in(
    tx: &mut WriteTx,
    owner: &AgentOwner,
    owned_agent: &str,
    require_active: bool,
) -> Result<bool, DbError> {
    let (user, organization) = owner.columns();
    Ok(sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM agent_owners d JOIN agents a ON a.principal_id=d.principal_id JOIN system_principals p ON p.id=d.principal_id WHERE d.owner_user_id IS ? AND d.organization_id IS ? AND d.principal_id=? AND (?=0 OR (d.active=1 AND p.disabled_at IS NULL)))").bind(user).bind(organization).bind(owned_agent).bind(require_active).fetch_one(&mut **tx).await?)
}

async fn load<'e, E: Executor<'e, Database = Sqlite>>(
    executor: E,
    owner: &AgentOwner,
    owned_agent: &str,
    key: &str,
) -> Result<Option<AgentConnectorBinding>, DbError> {
    let (user, organization) = owner.columns();
    let row = sqlx::query("SELECT b.* FROM agent_connector_bindings b JOIN agent_owners d ON d.principal_id=b.agent_id JOIN agents a ON a.principal_id=d.principal_id WHERE d.owner_user_id IS ? AND d.organization_id IS ? AND b.agent_id=? AND b.connector_key=?").bind(user).bind(organization).bind(owned_agent).bind(key).fetch_optional(executor).await?;
    row.map(map_binding).transpose()
}
fn map_binding(row: sqlx::sqlite::SqliteRow) -> Result<AgentConnectorBinding, DbError> {
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
    Ok(AgentConnectorBinding {
        agent_id: row.try_get("agent_id")?,
        connector_key: row.try_get("connector_key")?,
        identity_mode,
        connection_generation: row.try_get("connection_generation")?,
        binding_generation: row.try_get("binding_generation")?,
        updated_at: parse_ts(row.try_get("updated_at")?, "updated_at")?,
    })
}
pub async fn list_for_agent(
    pool: &Pool,
    owner: &AgentOwner,
    owned_agent: &str,
) -> Result<Vec<AgentConnectorBinding>, DbError> {
    list_with(pool, owner, owned_agent).await
}
pub async fn list_for_agent_in(
    tx: &mut WriteTx,
    owner: &AgentOwner,
    owned_agent: &str,
) -> Result<Vec<AgentConnectorBinding>, DbError> {
    list_with(&mut **tx, owner, owned_agent).await
}
async fn list_with<'e, E: Executor<'e, Database = Sqlite>>(
    executor: E,
    owner: &AgentOwner,
    owned_agent: &str,
) -> Result<Vec<AgentConnectorBinding>, DbError> {
    let (user, organization) = owner.columns();
    let rows = sqlx::query("SELECT b.* FROM agent_connector_bindings b JOIN agent_owners d ON d.principal_id=b.agent_id JOIN agents a ON a.principal_id=d.principal_id WHERE d.owner_user_id IS ? AND d.organization_id IS ? AND b.agent_id=? ORDER BY b.connector_key").bind(user).bind(organization).bind(owned_agent).fetch_all(executor).await?;
    rows.into_iter().map(map_binding).collect()
}
pub async fn get(
    pool: &Pool,
    owner: &AgentOwner,
    owned_agent: &str,
    key: &str,
) -> Result<Option<AgentConnectorBinding>, DbError> {
    load(pool, owner, owned_agent, key).await
}
pub async fn get_in(
    tx: &mut WriteTx,
    owner: &AgentOwner,
    owned_agent: &str,
    key: &str,
) -> Result<Option<AgentConnectorBinding>, DbError> {
    load(&mut **tx, owner, owned_agent, key).await
}

pub async fn save_in(tx: &mut WriteTx, new: Selection<'_>) -> Result<SaveOutcome, DbError> {
    if !owned_agent_in(tx, new.owner, new.agent_id, false).await? {
        return Ok(SaveOutcome::Missing);
    }
    let saved = get_in(tx, new.owner, new.agent_id, new.connector_key).await?;
    if saved.as_ref().map(|b| b.binding_generation.as_str()) != new.expected_binding_generation {
        return Ok(SaveOutcome::Conflict);
    }
    let owner = match (new.owner, new.identity_mode) {
        (AgentOwner::User(user), IdentityMode::User) => ConnectionOwner::User(user.clone()),
        (AgentOwner::Organization(_), IdentityMode::User) => return Ok(SaveOutcome::Unavailable),
        (_, IdentityMode::Principal) => ConnectionOwner::Principal(new.agent_id.to_owned()),
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
    let binding = AgentConnectorBinding {
        agent_id: new.agent_id.to_owned(),
        connector_key: new.connector_key.to_owned(),
        identity_mode: new.identity_mode,
        connection_generation: new.connection_generation.to_owned(),
        binding_generation: uuid::Uuid::new_v4().to_string(),
        updated_at: Timestamp::now(),
    };
    sqlx::query("INSERT INTO agent_connector_bindings(agent_id,connector_key,identity_mode,connection_generation,binding_generation,updated_at) VALUES(?,?,?,?,?,?) ON CONFLICT(agent_id,connector_key) DO UPDATE SET identity_mode=excluded.identity_mode,connection_generation=excluded.connection_generation,binding_generation=excluded.binding_generation,updated_at=excluded.updated_at").bind(&binding.agent_id).bind(&binding.connector_key).bind(binding.identity_mode.as_str()).bind(&binding.connection_generation).bind(&binding.binding_generation).bind(binding.updated_at.to_string()).execute(&mut **tx).await?;
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
        let (_, owned_agent) = agents::create_personal_agent(
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
        let owned_agent = owned_agent.unwrap().principal.id;
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
            credential(ConnectionOwner::Principal(owned_agent.clone())),
        )
        .await
        .unwrap()
        .unwrap();
        (pool, owned_agent, user.generation, own.generation)
    }

    #[tokio::test]
    async fn implicit_user_selection_is_a_noop_and_switch_back_retains_a_real_generation() {
        let (pool, owned_agent, user_generation, own_generation) = fixture().await;
        let mut tx = WriteTx::begin(&pool).await.unwrap();
        let owner = AgentOwner::User("owner".into());
        let selection =
            |identity_mode, connection_generation, expected_binding_generation| Selection {
                owner: &owner,
                agent_id: &owned_agent,
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
            get_in(
                &mut tx,
                &AgentOwner::User("owner".into()),
                &owned_agent,
                "fixture"
            )
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
            get(
                &pool,
                &AgentOwner::User("owner".into()),
                &owned_agent,
                "fixture"
            )
            .await
            .unwrap(),
            Some(user.clone())
        );
        assert_eq!(
            list_for_agent(&pool, &AgentOwner::User("owner".into()), &owned_agent)
                .await
                .unwrap(),
            vec![user]
        );
        assert!(
            list_for_agent(&pool, &AgentOwner::User("other".into()), &owned_agent)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            get(
                &pool,
                &AgentOwner::User("other".into()),
                &owned_agent,
                "fixture"
            )
            .await
            .unwrap()
            .is_none()
        );
    }

    #[tokio::test]
    async fn selected_connection_generation_is_validated_in_the_write_transaction() {
        let (pool, owned_agent, _, own_generation) = fixture().await;
        let mut tx = WriteTx::begin(&pool).await.unwrap();
        let owner = AgentOwner::User("owner".into());
        let selection = |generation| Selection {
            owner: &owner,
            agent_id: &owned_agent,
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
            get_in(
                &mut tx,
                &AgentOwner::User("owner".into()),
                &owned_agent,
                "fixture"
            )
            .await
            .unwrap()
            .is_none()
        );
        sqlx::query("UPDATE mcp_connections SET status='reauth' WHERE principal_id=?")
            .bind(&owned_agent)
            .execute(&mut *tx)
            .await
            .unwrap();
        assert_eq!(
            save_in(&mut tx, selection(&own_generation)).await.unwrap(),
            SaveOutcome::Unavailable
        );
        assert!(
            get_in(
                &mut tx,
                &AgentOwner::User("owner".into()),
                &owned_agent,
                "fixture"
            )
            .await
            .unwrap()
            .is_none()
        );
        tx.commit().await.unwrap();
    }

    #[tokio::test]
    async fn inactive_configuration_is_owned_but_cannot_authorize_execution() {
        let (pool, owned_agent, _, _) = fixture().await;
        let mut tx = WriteTx::begin(&pool).await.unwrap();
        assert!(
            owned_agent_in(
                &mut tx,
                &AgentOwner::User("owner".into()),
                &owned_agent,
                true
            )
            .await
            .unwrap()
        );
        assert!(
            !owned_agent_in(
                &mut tx,
                &AgentOwner::User("other".into()),
                &owned_agent,
                false
            )
            .await
            .unwrap()
        );
        sqlx::query("UPDATE agent_owners SET active=0 WHERE principal_id=?")
            .bind(&owned_agent)
            .execute(&mut *tx)
            .await
            .unwrap();
        assert!(
            owned_agent_in(
                &mut tx,
                &AgentOwner::User("owner".into()),
                &owned_agent,
                false
            )
            .await
            .unwrap()
        );
        assert!(
            !owned_agent_in(
                &mut tx,
                &AgentOwner::User("owner".into()),
                &owned_agent,
                true
            )
            .await
            .unwrap()
        );
        sqlx::query("UPDATE agent_owners SET active=1 WHERE principal_id=?")
            .bind(&owned_agent)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("UPDATE system_principals SET disabled_at=? WHERE id=?")
            .bind(jiff::Timestamp::now().to_string())
            .bind(&owned_agent)
            .execute(&mut *tx)
            .await
            .unwrap();
        assert!(
            owned_agent_in(
                &mut tx,
                &AgentOwner::User("owner".into()),
                &owned_agent,
                false
            )
            .await
            .unwrap()
        );
        assert!(
            !owned_agent_in(
                &mut tx,
                &AgentOwner::User("owner".into()),
                &owned_agent,
                true
            )
            .await
            .unwrap()
        );
        tx.commit().await.unwrap();
    }
    async fn organization_connection_fixture() -> (Pool, agents::AgentOwner, String, String, String)
    {
        use crate::db::organizations::{self, NewOrganization, OrganizationCreateResult};
        let (pool, personal_agent, _, _) = fixture().await;
        sqlx::query("INSERT INTO gateway_groups(name,created_at,updated_at) VALUES('members','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')").execute(&pool).await.unwrap();
        let OrganizationCreateResult::Created(organization) = organizations::create(
            &pool,
            &NewOrganization {
                name: "Support organization",
                description: "Tickets",
                member_groups: &["members".into()],
            },
            "owner",
        )
        .await
        .unwrap() else {
            panic!("actual fixture group exists");
        };
        let owner = agents::AgentOwner::Organization(organization.organization.id);
        let mut tx = WriteTx::begin(&pool).await.unwrap();
        let (_, first) = agents::create_organization_agent_in(
            &mut tx,
            match &owner {
                agents::AgentOwner::Organization(id) => id,
                _ => unreachable!(),
            },
            &NewPrincipal {
                name: "Support",
                display: "Support",
                description: "Tickets",
            },
            "{}",
            "owner",
            4,
        )
        .await
        .unwrap();
        let (_, second) = agents::create_organization_agent_in(
            &mut tx,
            match &owner {
                agents::AgentOwner::Organization(id) => id,
                _ => unreachable!(),
            },
            &NewPrincipal {
                name: "Research",
                display: "Research",
                description: "Knowledge",
            },
            "{}",
            "owner",
            4,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        (
            pool,
            owner,
            first.unwrap().principal.id,
            second.unwrap().principal.id,
            personal_agent,
        )
    }

    async fn own_organization_credential(
        pool: &Pool,
        owned_agent: &str,
    ) -> mcp_connections::Connection {
        let catalog_generation = db::mcp_catalog::get(pool, "fixture")
            .await
            .unwrap()
            .unwrap()
            .dispatch_generation;
        let identity = mcp_connections::upsert_connection(
            pool,
            NewConnection {
                owner: ConnectionOwner::Principal(owned_agent.into()),
                connector_key: "fixture".into(),
                catalog_generation,
                access_token_ct: vec![1],
                access_token_nonce: vec![2],
                refresh_token_ct: None,
                refresh_token_nonce: None,
                token_expires_at: None,
                scopes: vec![],
                dcr_client_id: None,
                dcr_client_secret_ct: None,
                dcr_client_secret_nonce: None,
                token_url: None,
            },
        )
        .await
        .unwrap()
        .unwrap();
        mcp_connections::get_connection(pool, &identity.owner, "fixture")
            .await
            .unwrap()
            .unwrap()
    }

    #[tokio::test]
    async fn organization_selection_requires_exact_typed_owner_and_explicit_own_credential() {
        let (pool, owner, owned_agent, _, _) = organization_connection_fixture().await;
        let agents::AgentOwner::Organization(org_id) = &owner else {
            unreachable!()
        };
        sqlx::query("INSERT INTO users(id,email,roles_json,created_at,updated_at) VALUES(?,'same-id@example.test','[]','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')").bind(org_id).execute(&pool).await.unwrap();
        let same_text_user = agents::AgentOwner::User(org_id.clone());
        let wrong_org = agents::AgentOwner::Organization("missing".into());
        let user_generation = mcp_connections::get_connection(
            &pool,
            &ConnectionOwner::User("owner".into()),
            "fixture",
        )
        .await
        .unwrap()
        .unwrap()
        .generation;
        let mut tx = WriteTx::begin(&pool).await.unwrap();
        assert!(
            owned_agent_in(&mut tx, &owner, &owned_agent, true)
                .await
                .unwrap()
        );
        assert!(
            !owned_agent_in(&mut tx, &same_text_user, &owned_agent, false)
                .await
                .unwrap()
        );
        assert!(
            !owned_agent_in(&mut tx, &wrong_org, &owned_agent, false)
                .await
                .unwrap()
        );
        for candidate in [&owner, &same_text_user, &wrong_org] {
            let outcome = save_in(
                &mut tx,
                Selection {
                    owner: candidate,
                    agent_id: &owned_agent,
                    connector_key: "fixture",
                    identity_mode: IdentityMode::User,
                    connection_generation: &user_generation,
                    expected_binding_generation: None,
                },
            )
            .await
            .unwrap();
            assert_eq!(
                outcome,
                if candidate == &owner {
                    SaveOutcome::Unavailable
                } else {
                    SaveOutcome::Missing
                }
            );
        }
        assert_eq!(
            save_in(
                &mut tx,
                Selection {
                    owner: &owner,
                    agent_id: &owned_agent,
                    connector_key: "fixture",
                    identity_mode: IdentityMode::Principal,
                    connection_generation: &user_generation,
                    expected_binding_generation: None
                }
            )
            .await
            .unwrap(),
            SaveOutcome::Unavailable
        );
        assert!(
            list_for_agent_in(&mut tx, &owner, &owned_agent)
                .await
                .unwrap()
                .is_empty()
        );
        drop(tx);
        assert!(
            list_for_agent(&pool, &same_text_user, &owned_agent)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn organization_binding_cas_noop_and_outer_transaction_rollback_are_preserved() {
        let (pool, owner, owned_agent, other_owned_agent, _) =
            organization_connection_fixture().await;
        let credential = own_organization_credential(&pool, &owned_agent).await;
        let other = own_organization_credential(&pool, &other_owned_agent).await;
        let mut tx = WriteTx::begin(&pool).await.unwrap();
        let selection = |generation, expected| Selection {
            owner: &owner,
            agent_id: &owned_agent,
            connector_key: "fixture",
            identity_mode: IdentityMode::Principal,
            connection_generation: generation,
            expected_binding_generation: expected,
        };
        assert_eq!(
            save_in(&mut tx, selection(&other.generation, None))
                .await
                .unwrap(),
            SaveOutcome::Unavailable
        );
        let SaveOutcome::Saved(Some(binding)) =
            save_in(&mut tx, selection(&credential.generation, None))
                .await
                .unwrap()
        else {
            panic!("organization selection creates an explicit binding");
        };
        assert_eq!(binding.identity_mode, IdentityMode::Principal);
        assert_eq!(
            get_in(&mut tx, &owner, &owned_agent, "fixture")
                .await
                .unwrap(),
            Some(binding.clone())
        );
        assert_eq!(
            list_for_agent_in(&mut tx, &owner, &owned_agent)
                .await
                .unwrap(),
            vec![binding.clone()]
        );
        assert_eq!(
            save_in(&mut tx, selection(&credential.generation, None))
                .await
                .unwrap(),
            SaveOutcome::Conflict
        );
        assert_eq!(
            save_in(
                &mut tx,
                selection(&credential.generation, Some(&binding.binding_generation))
            )
            .await
            .unwrap(),
            SaveOutcome::Saved(Some(binding.clone()))
        );
        assert_eq!(
            save_in(
                &mut tx,
                selection("stale", Some(&binding.binding_generation))
            )
            .await
            .unwrap(),
            SaveOutcome::Unavailable
        );
        sqlx::query("UPDATE mcp_connections SET status='reauth' WHERE principal_id=?")
            .bind(&owned_agent)
            .execute(&mut *tx)
            .await
            .unwrap();
        assert_eq!(
            save_in(
                &mut tx,
                selection(&credential.generation, Some(&binding.binding_generation))
            )
            .await
            .unwrap(),
            SaveOutcome::Unavailable
        );
        assert_eq!(
            get_in(&mut tx, &owner, &owned_agent, "fixture")
                .await
                .unwrap(),
            Some(binding)
        );
        drop(tx);
        assert!(
            get(&pool, &owner, &owned_agent, "fixture")
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            !mcp_connections::get_connection(&pool, &credential.owner, "fixture")
                .await
                .unwrap()
                .unwrap()
                .is_errored()
        );
    }

    #[tokio::test]
    async fn organization_configuration_survives_inactivity_but_never_authorizes_inactive_execution()
     {
        let (pool, owner, owned_agent, _, _) = organization_connection_fixture().await;
        let credential = own_organization_credential(&pool, &owned_agent).await;
        let mut tx = WriteTx::begin(&pool).await.unwrap();
        sqlx::query("UPDATE agent_owners SET active=0 WHERE principal_id=?")
            .bind(&owned_agent)
            .execute(&mut *tx)
            .await
            .unwrap();
        assert!(
            owned_agent_in(&mut tx, &owner, &owned_agent, false)
                .await
                .unwrap()
        );
        assert!(
            !owned_agent_in(&mut tx, &owner, &owned_agent, true)
                .await
                .unwrap()
        );
        assert!(matches!(
            save_in(
                &mut tx,
                Selection {
                    owner: &owner,
                    agent_id: &owned_agent,
                    connector_key: "fixture",
                    identity_mode: IdentityMode::Principal,
                    connection_generation: &credential.generation,
                    expected_binding_generation: None
                }
            )
            .await
            .unwrap(),
            SaveOutcome::Saved(Some(_))
        ));
        sqlx::query("UPDATE agent_owners SET active=1 WHERE principal_id=?")
            .bind(&owned_agent)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("UPDATE system_principals SET disabled_at=? WHERE id=?")
            .bind(Timestamp::now().to_string())
            .bind(&owned_agent)
            .execute(&mut *tx)
            .await
            .unwrap();
        assert!(
            owned_agent_in(&mut tx, &owner, &owned_agent, false)
                .await
                .unwrap()
        );
        assert!(
            !owned_agent_in(&mut tx, &owner, &owned_agent, true)
                .await
                .unwrap()
        );
        tx.commit().await.unwrap();
        assert_eq!(
            list_for_agent(&pool, &owner, &owned_agent)
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn organization_credential_and_binding_survive_creator_deletion_with_personal_cascade() {
        let (pool, owner, owned_agent, _, personal_agent) = organization_connection_fixture().await;
        let credential = own_organization_credential(&pool, &owned_agent).await;
        let mut tx = WriteTx::begin(&pool).await.unwrap();
        assert!(matches!(
            save_in(
                &mut tx,
                Selection {
                    owner: &owner,
                    agent_id: &owned_agent,
                    connector_key: "fixture",
                    identity_mode: IdentityMode::Principal,
                    connection_generation: &credential.generation,
                    expected_binding_generation: None
                }
            )
            .await
            .unwrap(),
            SaveOutcome::Saved(Some(_))
        ));
        tx.commit().await.unwrap();
        sqlx::query("DELETE FROM users WHERE id='owner'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            get(&pool, &owner, &owned_agent, "fixture")
                .await
                .unwrap()
                .unwrap()
                .connection_generation,
            credential.generation
        );
        assert!(
            mcp_connections::get_connection(&pool, &credential.owner, "fixture")
                .await
                .unwrap()
                .is_some()
        );
        assert!(agents::get(&pool, &personal_agent).await.unwrap().is_none());
        assert!(
            mcp_connections::get_connection(
                &pool,
                &ConnectionOwner::Principal(personal_agent),
                "fixture"
            )
            .await
            .unwrap()
            .is_none()
        );
    }
}
