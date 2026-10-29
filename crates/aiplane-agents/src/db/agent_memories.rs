// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Durable agent lessons and shared organization knowledge.

use aiplane_core::server::db::user_memories::{Memory, MemoryKind};
use jiff::Timestamp;
use sqlx::Row;
use sqlx::sqlite::SqliteRow;
use uuid::Uuid;

use super::{DbError, Pool, WriteTx, agents::AgentOwner};

pub const MAX_CONTENT_BYTES: usize = 2_000;

#[derive(Clone, Copy, Debug)]
pub enum MemoryScope<'a> {
    Agent {
        owner: &'a AgentOwner,
        agent_id: &'a str,
    },
    Organization {
        organization_id: &'a str,
    },
}

impl<'a> MemoryScope<'a> {
    fn columns(self) -> (Option<&'a str>, Option<&'a str>) {
        match self {
            Self::Agent { agent_id, .. } => (Some(agent_id), None),
            Self::Organization { organization_id } => (None, Some(organization_id)),
        }
    }
}

async fn scope_exists_in(tx: &mut WriteTx, scope: MemoryScope<'_>) -> Result<bool, DbError> {
    match scope {
        MemoryScope::Agent { owner, agent_id } => {
            let (user, organization) = owner.columns();
            Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM agent_owners WHERE principal_id=? AND owner_user_id IS ? AND organization_id IS ?)")
                .bind(agent_id).bind(user).bind(organization).fetch_one(&mut **tx).await?)
        }
        MemoryScope::Organization { organization_id } => Ok(sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM organizations WHERE id=?)",
        )
        .bind(organization_id)
        .fetch_one(&mut **tx)
        .await?),
    }
}

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

pub async fn list_in(
    tx: &mut WriteTx,
    scope: MemoryScope<'_>,
    limit: i64,
) -> Result<Option<Vec<Memory>>, DbError> {
    if !scope_exists_in(tx, scope).await? {
        return Ok(None);
    }
    let (owned_agent, organization) = scope.columns();
    let rows = sqlx::query("SELECT id, kind, content, created_at FROM agent_memories WHERE agent_id IS ? AND organization_id IS ? ORDER BY created_at DESC LIMIT ?")
        .bind(owned_agent).bind(organization).bind(limit).fetch_all(&mut **tx).await?;
    rows.iter()
        .map(map_row)
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

pub async fn get_in(
    tx: &mut WriteTx,
    scope: MemoryScope<'_>,
    id: &str,
) -> Result<Option<Memory>, DbError> {
    if !scope_exists_in(tx, scope).await? {
        return Ok(None);
    }
    let (owned_agent, organization) = scope.columns();
    let row = sqlx::query("SELECT id, kind, content, created_at FROM agent_memories WHERE id=? AND agent_id IS ? AND organization_id IS ?")
        .bind(id).bind(owned_agent).bind(organization).fetch_optional(&mut **tx).await?;
    row.as_ref().map(map_row).transpose()
}

pub async fn insert_in(
    tx: &mut WriteTx,
    scope: MemoryScope<'_>,
    kind: MemoryKind,
    content: &str,
) -> Result<Option<Memory>, DbError> {
    if !scope_exists_in(tx, scope).await? {
        return Ok(None);
    }
    let (owned_agent, organization) = scope.columns();
    let query = match scope {
        MemoryScope::Agent { .. } => {
            "INSERT INTO agent_memories(id,agent_id,organization_id,kind,content,created_at,updated_at) VALUES(?,?,?,?,?,?,?) ON CONFLICT(agent_id,content) WHERE agent_id IS NOT NULL DO UPDATE SET kind=excluded.kind,updated_at=excluded.updated_at RETURNING id,kind,content,created_at"
        }
        MemoryScope::Organization { .. } => {
            "INSERT INTO agent_memories(id,agent_id,organization_id,kind,content,created_at,updated_at) VALUES(?,?,?,?,?,?,?) ON CONFLICT(organization_id,content) WHERE organization_id IS NOT NULL DO UPDATE SET kind=excluded.kind,updated_at=excluded.updated_at RETURNING id,kind,content,created_at"
        }
    };
    let now = Timestamp::now().to_string();
    let row = sqlx::query(query)
        .bind(Uuid::new_v4().to_string())
        .bind(owned_agent)
        .bind(organization)
        .bind(kind.as_str())
        .bind(content)
        .bind(&now)
        .bind(&now)
        .fetch_one(&mut **tx)
        .await?;
    map_row(&row).map(Some)
}

pub async fn update_in(
    tx: &mut WriteTx,
    scope: MemoryScope<'_>,
    id: &str,
    kind: MemoryKind,
    content: &str,
) -> Result<Option<Memory>, DbError> {
    if !scope_exists_in(tx, scope).await? {
        return Ok(None);
    }
    let (owned_agent, organization) = scope.columns();
    let row = sqlx::query("UPDATE agent_memories SET kind=?,content=?,updated_at=? WHERE id=? AND agent_id IS ? AND organization_id IS ? RETURNING id,kind,content,created_at")
        .bind(kind.as_str()).bind(content).bind(Timestamp::now().to_string()).bind(id).bind(owned_agent).bind(organization)
        .fetch_optional(&mut **tx).await?;
    row.as_ref().map(map_row).transpose()
}

pub async fn delete_in(
    tx: &mut WriteTx,
    scope: MemoryScope<'_>,
    id: &str,
) -> Result<bool, DbError> {
    if !scope_exists_in(tx, scope).await? {
        return Ok(false);
    }
    let (owned_agent, organization) = scope.columns();
    Ok(sqlx::query(
        "DELETE FROM agent_memories WHERE id=? AND agent_id IS ? AND organization_id IS ?",
    )
    .bind(id)
    .bind(owned_agent)
    .bind(organization)
    .execute(&mut **tx)
    .await?
    .rows_affected()
        > 0)
}

pub async fn list(
    pool: &Pool,
    scope: MemoryScope<'_>,
    limit: i64,
) -> Result<Option<Vec<Memory>>, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let result = list_in(&mut tx, scope, limit).await?;
    tx.commit().await?;
    Ok(result)
}

pub async fn get(pool: &Pool, scope: MemoryScope<'_>, id: &str) -> Result<Option<Memory>, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let result = get_in(&mut tx, scope, id).await?;
    tx.commit().await?;
    Ok(result)
}

pub async fn insert(
    pool: &Pool,
    scope: MemoryScope<'_>,
    kind: MemoryKind,
    content: &str,
) -> Result<Option<Memory>, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let result = insert_in(&mut tx, scope, kind, content).await?;
    tx.commit().await?;
    Ok(result)
}

pub async fn update(
    pool: &Pool,
    scope: MemoryScope<'_>,
    id: &str,
    kind: MemoryKind,
    content: &str,
) -> Result<Option<Memory>, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let result = update_in(&mut tx, scope, id, kind, content).await?;
    tx.commit().await?;
    Ok(result)
}

pub async fn delete(pool: &Pool, scope: MemoryScope<'_>, id: &str) -> Result<bool, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let result = delete_in(&mut tx, scope, id).await?;
    tx.commit().await?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::agents::{self, AgentCreate};
    use crate::db::system_principals::NewPrincipal;
    use aiplane_core::server::db::open;
    use aiplane_core::server::db::user_memories;
    use aiplane_core::server::db::users::{self, User};
    use jiff::Timestamp;
    use std::path::Path;

    async fn fixture() -> (Pool, String) {
        let pool = open(Path::new(":memory:")).await.unwrap();
        let now = Timestamp::now();
        users::upsert(
            &pool,
            &User {
                id: "alice".into(),
                email: "alice@example.com".into(),
                name: Some("Alice".into()),
                roles: vec![],
                created_at: now,
                updated_at: now,
                timezone: None,
                speech_voice: None,
            },
        )
        .await
        .unwrap();
        let (result, owned_agent) = agents::create_personal_agent(
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
        assert_eq!(result, AgentCreate::Created);
        (pool, owned_agent.unwrap().principal.id)
    }

    #[tokio::test]
    async fn agent_memories_are_isolated_from_users_and_other_agents() {
        let (pool, owned_agent) = fixture().await;
        let first = insert(
            &pool,
            MemoryScope::Agent {
                owner: &agents::AgentOwner::User("alice".into()),
                agent_id: &owned_agent,
            },
            MemoryKind::Fact,
            "learned pitfall",
        )
        .await
        .unwrap()
        .unwrap();
        let duplicate = insert(
            &pool,
            MemoryScope::Agent {
                owner: &agents::AgentOwner::User("alice".into()),
                agent_id: &owned_agent,
            },
            MemoryKind::Project,
            "learned pitfall",
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(duplicate.id, first.id);
        assert_eq!(duplicate.created_at, first.created_at);
        assert_eq!(duplicate.kind, MemoryKind::Project);
        user_memories::insert(&pool, "alice", MemoryKind::Preference, "German answers")
            .await
            .unwrap();
        let rows = list(
            &pool,
            MemoryScope::Agent {
                owner: &agents::AgentOwner::User("alice".into()),
                agent_id: &owned_agent,
            },
            20,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].content, "learned pitfall");
        assert!(
            list(
                &pool,
                MemoryScope::Agent {
                    owner: &agents::AgentOwner::User("bob".into()),
                    agent_id: &owned_agent
                },
                20
            )
            .await
            .unwrap()
            .is_none()
        );
    }

    #[tokio::test]
    async fn agent_memory_crud_is_owner_scoped() {
        let (pool, owned_agent) = fixture().await;
        let item = insert(
            &pool,
            MemoryScope::Agent {
                owner: &agents::AgentOwner::User("alice".into()),
                agent_id: &owned_agent,
            },
            MemoryKind::Fact,
            "first",
        )
        .await
        .unwrap()
        .unwrap();
        assert!(
            update(
                &pool,
                MemoryScope::Agent {
                    owner: &agents::AgentOwner::User("bob".into()),
                    agent_id: &owned_agent
                },
                &item.id,
                MemoryKind::Fact,
                "hacked"
            )
            .await
            .unwrap()
            .is_none()
        );
        let changed = update(
            &pool,
            MemoryScope::Agent {
                owner: &agents::AgentOwner::User("alice".into()),
                agent_id: &owned_agent,
            },
            &item.id,
            MemoryKind::Project,
            "second",
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(changed.content, "second");
        assert!(
            !delete(
                &pool,
                MemoryScope::Agent {
                    owner: &agents::AgentOwner::User("bob".into()),
                    agent_id: &owned_agent
                },
                &item.id
            )
            .await
            .unwrap()
        );
        assert!(
            delete(
                &pool,
                MemoryScope::Agent {
                    owner: &agents::AgentOwner::User("alice".into()),
                    agent_id: &owned_agent
                },
                &item.id
            )
            .await
            .unwrap()
        );
        assert!(
            list(
                &pool,
                MemoryScope::Agent {
                    owner: &agents::AgentOwner::User("alice".into()),
                    agent_id: &owned_agent
                },
                20
            )
            .await
            .unwrap()
            .unwrap()
            .is_empty()
        );
    }

    #[tokio::test]
    async fn deleting_a_agent_owner_removes_the_agent_principal_and_private_memory() {
        let (pool, owned_agent) = fixture().await;
        insert(
            &pool,
            MemoryScope::Agent {
                owner: &agents::AgentOwner::User("alice".into()),
                agent_id: &owned_agent,
            },
            MemoryKind::Fact,
            "private note",
        )
        .await
        .unwrap()
        .unwrap();

        sqlx::query("DELETE FROM users WHERE id = ?")
            .bind("alice")
            .execute(&pool)
            .await
            .unwrap();

        let principals: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM system_principals WHERE id = ?")
                .bind(&owned_agent)
                .fetch_one(&pool)
                .await
                .unwrap();
        let owned_agents: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM agent_owners WHERE principal_id = ?")
                .bind(&owned_agent)
                .fetch_one(&pool)
                .await
                .unwrap();
        let memories: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM agent_memories WHERE agent_id = ?")
                .bind(&owned_agent)
                .fetch_one(&pool)
                .await
                .unwrap();

        assert_eq!(principals, 0);
        assert_eq!(owned_agents, 0);
        assert_eq!(memories, 0);
    }

    async fn organization_memory_fixture(pool: &Pool) -> (String, String, String) {
        use crate::db::{WriteTx, organizations};
        let result = organizations::create(
            pool,
            &organizations::NewOrganization {
                name: "Operations",
                description: "Shared operating knowledge",
                member_groups: &[],
            },
            "alice",
        )
        .await
        .unwrap();
        let organizations::OrganizationCreateResult::Created(row) = result else {
            panic!("organization creation failed")
        };
        let organization = row.organization.id;
        let mut tx = WriteTx::begin(pool).await.unwrap();
        let mut owned_agents = Vec::new();
        for name in ["support", "research"] {
            let (outcome, owned_agent) = agents::create_organization_agent_in(
                &mut tx,
                &organization,
                &NewPrincipal {
                    name,
                    display: name,
                    description: "",
                },
                r#"{"main":{}}"#,
                "alice",
                4,
            )
            .await
            .unwrap();
            assert_eq!(outcome, AgentCreate::Created);
            owned_agents.push(owned_agent.unwrap().principal.id);
        }
        tx.commit().await.unwrap();
        (organization, owned_agents.remove(0), owned_agents.remove(0))
    }

    #[tokio::test]
    async fn typed_memory_scopes_isolate_agent_lessons_shared_knowledge_and_user_profiles() {
        let (pool, personal) = fixture().await;
        let (organization, first, second) = organization_memory_fixture(&pool).await;
        let personal_owner = agents::AgentOwner::User("alice".into());
        let owner = agents::AgentOwner::Organization(organization.clone());
        let personal_scope = MemoryScope::Agent {
            owner: &personal_owner,
            agent_id: &personal,
        };
        let first_scope = MemoryScope::Agent {
            owner: &owner,
            agent_id: &first,
        };
        let second_scope = MemoryScope::Agent {
            owner: &owner,
            agent_id: &second,
        };
        let shared = MemoryScope::Organization {
            organization_id: &organization,
        };
        let a = insert(&pool, personal_scope, MemoryKind::Fact, "same content")
            .await
            .unwrap()
            .unwrap();
        let b = insert(&pool, first_scope, MemoryKind::Fact, "same content")
            .await
            .unwrap()
            .unwrap();
        let c = insert(&pool, second_scope, MemoryKind::Fact, "same content")
            .await
            .unwrap()
            .unwrap();
        let d = insert(&pool, shared, MemoryKind::Fact, "same content")
            .await
            .unwrap()
            .unwrap();
        let ids: std::collections::BTreeSet<_> = [&a.id, &b.id, &c.id, &d.id].into_iter().collect();
        assert_eq!(ids.len(), 4);
        for (scope, expected) in [
            (personal_scope, &a.id),
            (first_scope, &b.id),
            (second_scope, &c.id),
            (shared, &d.id),
        ] {
            let rows = list(&pool, scope, 10).await.unwrap().unwrap();
            assert_eq!(rows.len(), 1);
            assert_eq!(&rows[0].id, expected);
        }
        assert!(get(&pool, first_scope, &c.id).await.unwrap().is_none());
        assert!(get(&pool, shared, &b.id).await.unwrap().is_none());
        assert!(
            update(&pool, second_scope, &b.id, MemoryKind::Fact, "foreign")
                .await
                .unwrap()
                .is_none()
        );
        assert!(!delete(&pool, shared, &b.id).await.unwrap());
        assert!(
            user_memories::list_for_user(&pool, "alice", 10)
                .await
                .unwrap()
                .is_empty()
        );
        let (other, _, _) = organization_memory_fixture(&pool).await;
        assert!(
            get(
                &pool,
                MemoryScope::Organization {
                    organization_id: &other
                },
                &d.id
            )
            .await
            .unwrap()
            .is_none()
        );
    }

    #[tokio::test]
    async fn scoped_duplicates_preserve_identity_and_creation_time_and_updates_keep_uniqueness() {
        let (pool, _) = fixture().await;
        let (organization, owned_agent, _) = organization_memory_fixture(&pool).await;
        let owner = agents::AgentOwner::Organization(organization.clone());
        for scope in [
            MemoryScope::Agent {
                owner: &owner,
                agent_id: &owned_agent,
            },
            MemoryScope::Organization {
                organization_id: &organization,
            },
        ] {
            let first = insert(&pool, scope, MemoryKind::Fact, "original")
                .await
                .unwrap()
                .unwrap();
            let duplicate = insert(&pool, scope, MemoryKind::Project, "original")
                .await
                .unwrap()
                .unwrap();
            assert_eq!(duplicate.id, first.id);
            assert_eq!(duplicate.created_at, first.created_at);
            assert_eq!(duplicate.kind, MemoryKind::Project);
            let other = insert(&pool, scope, MemoryKind::Fact, "other")
                .await
                .unwrap()
                .unwrap();
            assert!(
                update(&pool, scope, &other.id, MemoryKind::Fact, "original")
                    .await
                    .is_err()
            );
            assert_eq!(
                get(&pool, scope, &other.id).await.unwrap().unwrap().content,
                "other"
            );
            assert_eq!(list(&pool, scope, 1).await.unwrap().unwrap().len(), 1);
        }
    }

    #[tokio::test]
    async fn scope_crud_uses_one_write_transaction_and_drop_restores_committed_rows() {
        use crate::db::WriteTx;
        let (pool, _) = fixture().await;
        let (organization, owned_agent, _) = organization_memory_fixture(&pool).await;
        let owner = agents::AgentOwner::Organization(organization.clone());
        for scope in [
            MemoryScope::Agent {
                owner: &owner,
                agent_id: &owned_agent,
            },
            MemoryScope::Organization {
                organization_id: &organization,
            },
        ] {
            let committed = insert(&pool, scope, MemoryKind::Fact, "committed")
                .await
                .unwrap()
                .unwrap();
            let mut tx = WriteTx::begin(&pool).await.unwrap();
            let transient = insert_in(&mut tx, scope, MemoryKind::Project, "transient")
                .await
                .unwrap()
                .unwrap();
            assert_eq!(list_in(&mut tx, scope, 10).await.unwrap().unwrap().len(), 2);
            assert_eq!(
                get_in(&mut tx, scope, &transient.id)
                    .await
                    .unwrap()
                    .unwrap()
                    .content,
                "transient"
            );
            let changed = update_in(
                &mut tx,
                scope,
                &committed.id,
                MemoryKind::Preference,
                "changed",
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(changed.content, "changed");
            assert!(delete_in(&mut tx, scope, &transient.id).await.unwrap());
            drop(tx);
            let rows = list(&pool, scope, 10).await.unwrap().unwrap();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].id, committed.id);
            assert_eq!(rows[0].content, "committed");
        }
    }

    #[tokio::test]
    async fn exact_typed_agent_owners_never_alias_equal_user_and_organization_ids() {
        let (pool, personal) = fixture().await;
        let (organization, owned_agent, _) = organization_memory_fixture(&pool).await;
        let now = Timestamp::now();
        users::upsert(
            &pool,
            &User {
                id: organization.clone(),
                email: "same-id@example.com".into(),
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
        let wrong_user = agents::AgentOwner::User(organization.clone());
        let wrong_org = agents::AgentOwner::Organization("alice".into());
        for scope in [
            MemoryScope::Agent {
                owner: &wrong_user,
                agent_id: &owned_agent,
            },
            MemoryScope::Agent {
                owner: &wrong_org,
                agent_id: &personal,
            },
        ] {
            assert!(list(&pool, scope, 10).await.unwrap().is_none());
            assert!(
                insert(&pool, scope, MemoryKind::Fact, "forbidden")
                    .await
                    .unwrap()
                    .is_none()
            );
        }
        let missing = MemoryScope::Organization {
            organization_id: "missing",
        };
        assert!(list(&pool, missing, 10).await.unwrap().is_none());
        assert!(
            insert(&pool, missing, MemoryKind::Fact, "forbidden")
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn creator_deletion_preserves_organization_scopes_and_agent_deletion_preserves_shared_notes()
     {
        let (pool, _) = fixture().await;
        let (organization, owned_agent, second) = organization_memory_fixture(&pool).await;
        let owner = agents::AgentOwner::Organization(organization.clone());
        let own = MemoryScope::Agent {
            owner: &owner,
            agent_id: &owned_agent,
        };
        let shared = MemoryScope::Organization {
            organization_id: &organization,
        };
        let lesson = insert(&pool, own, MemoryKind::Fact, "lesson")
            .await
            .unwrap()
            .unwrap();
        let knowledge = insert(&pool, shared, MemoryKind::Fact, "shared knowledge")
            .await
            .unwrap()
            .unwrap();
        sqlx::query("DELETE FROM users WHERE id='alice'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            get(&pool, own, &lesson.id).await.unwrap().unwrap().content,
            "lesson"
        );
        assert_eq!(
            get(&pool, shared, &knowledge.id)
                .await
                .unwrap()
                .unwrap()
                .content,
            "shared knowledge"
        );
        sqlx::query("DELETE FROM system_principals WHERE id=?")
            .bind(&owned_agent)
            .execute(&pool)
            .await
            .unwrap();
        assert!(list(&pool, own, 10).await.unwrap().is_none());
        assert_eq!(
            get(&pool, shared, &knowledge.id)
                .await
                .unwrap()
                .unwrap()
                .content,
            "shared knowledge"
        );
        assert!(
            list(
                &pool,
                MemoryScope::Agent {
                    owner: &owner,
                    agent_id: &second
                },
                10
            )
            .await
            .unwrap()
            .unwrap()
            .is_empty()
        );
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_memories WHERE id=?")
            .bind(&lesson.id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }
}
