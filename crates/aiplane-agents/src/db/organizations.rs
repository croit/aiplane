// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use std::collections::{BTreeSet, HashMap};

use jiff::Timestamp;
use serde_json::json;
use sqlx::{Row, sqlite::SqliteRow};

use super::{DbError, Pool, WriteTx, agent_audit};
use agent_audit::{AuditKind, NewEvent};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrganizationRow {
    pub id: String,
    pub name: String,
    pub description: String,
    pub revision: String,
    pub created_by: String,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminOrganizationRow {
    pub organization: OrganizationRow,
    pub member_groups: Vec<String>,
}

pub struct NewOrganization<'a> {
    pub name: &'a str,
    pub description: &'a str,
    pub member_groups: &'a [String],
}

pub struct OrganizationEdit<'a> {
    pub name: &'a str,
    pub description: &'a str,
    pub member_groups: &'a [String],
    pub expected_revision: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrganizationCreateResult {
    Created(AdminOrganizationRow),
    InvalidGroups,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrganizationEditResult {
    Missing,
    Conflict,
    InvalidGroups,
    Unchanged(AdminOrganizationRow),
    Saved(AdminOrganizationRow),
}

fn map_organization(row: &SqliteRow) -> Result<OrganizationRow, DbError> {
    Ok(OrganizationRow {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        description: row.try_get("description")?,
        revision: row.try_get("revision")?,
        created_by: row.try_get("created_by")?,
        created_at: super::parse_ts(row.try_get("created_at")?, "created_at")?,
        updated_at: super::parse_ts(row.try_get("updated_at")?, "updated_at")?,
    })
}

async fn valid_groups_in(tx: &mut WriteTx, groups: &[String]) -> Result<bool, DbError> {
    let unique: BTreeSet<_> = groups.iter().collect();
    if unique.len() != groups.len() {
        return Ok(false);
    }
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM gateway_groups WHERE name IN (SELECT value FROM json_each(?))",
    )
    .bind(serde_json::to_string(groups).expect("string vector serializes"))
    .fetch_one(&mut **tx)
    .await?;
    Ok(count == groups.len() as i64)
}

pub async fn get_admin_in(
    tx: &mut WriteTx,
    id: &str,
) -> Result<Option<AdminOrganizationRow>, DbError> {
    let Some(row) = sqlx::query("SELECT * FROM organizations WHERE id=?")
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
    else {
        return Ok(None);
    };
    let member_groups = sqlx::query_scalar("SELECT gateway_group FROM organization_groups WHERE organization_id=? ORDER BY gateway_group")
        .bind(id).fetch_all(&mut **tx).await?;
    Ok(Some(AdminOrganizationRow {
        organization: map_organization(&row)?,
        member_groups,
    }))
}

pub async fn get_admin(pool: &Pool, id: &str) -> Result<Option<AdminOrganizationRow>, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let result = get_admin_in(&mut tx, id).await?;
    tx.commit().await?;
    Ok(result)
}

pub async fn list_admin(pool: &Pool) -> Result<Vec<AdminOrganizationRow>, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let result = list_admin_in(&mut tx).await?;
    tx.commit().await?;
    Ok(result)
}

pub async fn list_admin_in(tx: &mut WriteTx) -> Result<Vec<AdminOrganizationRow>, DbError> {
    let rows = sqlx::query("SELECT * FROM organizations ORDER BY name,id")
        .fetch_all(&mut **tx)
        .await?;
    let bindings: Vec<(String, String)> = sqlx::query_as(
        "SELECT organization_id,gateway_group FROM organization_groups ORDER BY gateway_group",
    )
    .fetch_all(&mut **tx)
    .await?;
    let mut groups: HashMap<String, Vec<String>> = HashMap::new();
    for (id, group) in bindings {
        groups.entry(id).or_default().push(group);
    }
    rows.iter()
        .map(|row| {
            let organization = map_organization(row)?;
            let member_groups = groups.remove(&organization.id).unwrap_or_default();
            Ok(AdminOrganizationRow {
                organization,
                member_groups,
            })
        })
        .collect()
}

pub async fn create(
    pool: &Pool,
    new: &NewOrganization<'_>,
    actor: &str,
) -> Result<OrganizationCreateResult, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let result = create_in(&mut tx, new, actor).await?;
    tx.commit().await?;
    Ok(result)
}

pub async fn create_in(
    tx: &mut WriteTx,
    new: &NewOrganization<'_>,
    actor: &str,
) -> Result<OrganizationCreateResult, DbError> {
    if !valid_groups_in(tx, new.member_groups).await? {
        return Ok(OrganizationCreateResult::InvalidGroups);
    }
    let id = uuid::Uuid::new_v4().to_string();
    let revision = uuid::Uuid::new_v4().to_string();
    let now = Timestamp::now().to_string();
    sqlx::query("INSERT INTO organizations(id,name,description,revision,created_by,created_at,updated_at) VALUES(?,?,?,?,?,?,?)")
        .bind(&id).bind(new.name).bind(new.description).bind(revision).bind(actor).bind(&now).bind(&now).execute(&mut **tx).await?;
    for group in new.member_groups {
        sqlx::query("INSERT INTO organization_groups(organization_id,gateway_group) VALUES(?,?)")
            .bind(&id)
            .bind(group)
            .execute(&mut **tx)
            .await?;
    }
    sqlx::query("UPDATE organizations SET updated_at=? WHERE id=?")
        .bind(&now)
        .bind(&id)
        .execute(&mut **tx)
        .await?;
    let created = get_admin_in(tx, &id)
        .await?
        .expect("inserted organization exists");
    agent_audit::append(tx,NewEvent::new(AuditKind::OrganizationCreated,actor,json!({"organization_id":id,"revision":created.organization.revision,"name":new.name,"member_groups":created.member_groups})).by(Some(actor))).await?;
    Ok(OrganizationCreateResult::Created(created))
}

pub async fn edit(
    pool: &Pool,
    id: &str,
    edit: &OrganizationEdit<'_>,
    actor: &str,
) -> Result<OrganizationEditResult, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let result = edit_in(&mut tx, id, edit, actor).await?;
    tx.commit().await?;
    Ok(result)
}

pub async fn edit_in(
    tx: &mut WriteTx,
    id: &str,
    edit: &OrganizationEdit<'_>,
    actor: &str,
) -> Result<OrganizationEditResult, DbError> {
    let Some(current) = get_admin_in(tx, id).await? else {
        return Ok(OrganizationEditResult::Missing);
    };
    if current.organization.revision != edit.expected_revision {
        return Ok(OrganizationEditResult::Conflict);
    }
    if !valid_groups_in(tx, edit.member_groups).await? {
        return Ok(OrganizationEditResult::InvalidGroups);
    }
    let old: BTreeSet<_> = current.member_groups.iter().map(String::as_str).collect();
    let new: BTreeSet<_> = edit.member_groups.iter().map(String::as_str).collect();
    if current.organization.name == edit.name
        && current.organization.description == edit.description
        && old == new
    {
        return Ok(OrganizationEditResult::Unchanged(current));
    }
    for group in old.difference(&new) {
        sqlx::query("DELETE FROM organization_groups WHERE organization_id=? AND gateway_group=?")
            .bind(id)
            .bind(group)
            .execute(&mut **tx)
            .await?;
    }
    for group in new.difference(&old) {
        sqlx::query("INSERT INTO organization_groups(organization_id,gateway_group) VALUES(?,?)")
            .bind(id)
            .bind(group)
            .execute(&mut **tx)
            .await?;
    }
    sqlx::query("UPDATE organizations SET name=?,description=?,revision=?,updated_at=? WHERE id=?")
        .bind(edit.name)
        .bind(edit.description)
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(Timestamp::now().to_string())
        .bind(id)
        .execute(&mut **tx)
        .await?;
    let saved = get_admin_in(tx, id)
        .await?
        .expect("updated organization exists");
    agent_audit::append(tx,NewEvent::new(AuditKind::OrganizationUpdated,actor,json!({"organization_id":id,"revision":saved.organization.revision,"name":edit.name,"member_groups":saved.member_groups})).by(Some(actor))).await?;
    Ok(OrganizationEditResult::Saved(saved))
}

pub async fn is_member_in(
    tx: &mut WriteTx,
    id: &str,
    group_ids: &[String],
) -> Result<bool, DbError> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM organizations o JOIN organization_groups b ON b.organization_id=o.id JOIN gateway_groups g ON g.name=b.gateway_group WHERE o.id=? AND g.name IN (SELECT value FROM json_each(?)))")
        .bind(id).bind(serde_json::to_string(group_ids).expect("string vector serializes")).fetch_one(&mut **tx).await?)
}

pub async fn is_member(pool: &Pool, id: &str, groups: &[String]) -> Result<bool, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let result = is_member_in(&mut tx, id, groups).await?;
    tx.commit().await?;
    Ok(result)
}

pub async fn list_for_groups(
    pool: &Pool,
    groups: &[String],
) -> Result<Vec<OrganizationRow>, DbError> {
    list_for_groups_with(pool, groups).await
}

pub async fn list_for_groups_in(
    tx: &mut WriteTx,
    groups: &[String],
) -> Result<Vec<OrganizationRow>, DbError> {
    list_for_groups_with(&mut **tx, groups).await
}

async fn list_for_groups_with<'e, E>(
    executor: E,
    groups: &[String],
) -> Result<Vec<OrganizationRow>, DbError>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let rows = sqlx::query("SELECT o.* FROM organizations o WHERE EXISTS(SELECT 1 FROM organization_groups b JOIN gateway_groups g ON g.name=b.gateway_group WHERE b.organization_id=o.id AND g.name IN (SELECT value FROM json_each(?))) ORDER BY o.name,o.id")
        .bind(serde_json::to_string(groups).expect("string vector serializes")).fetch_all(executor).await?;
    rows.iter().map(map_organization).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::system_principals::NewPrincipal;
    use crate::db::{Pool, WriteTx, agent_audit, agents};

    fn created(result: OrganizationCreateResult) -> AdminOrganizationRow {
        match result {
            OrganizationCreateResult::Created(row) => row,
            OrganizationCreateResult::InvalidGroups => panic!("fixture groups exist"),
        }
    }

    async fn pool() -> Pool {
        let p = aiplane_core::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        sqlx::query("INSERT INTO users(id,email,roles_json,created_at,updated_at) VALUES('admin','admin@example.test','[]','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')").execute(&p).await.unwrap();
        sqlx::query("INSERT INTO gateway_groups(name,created_at,updated_at) VALUES('engineering','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')").execute(&p).await.unwrap();
        p
    }

    #[tokio::test]
    async fn organization_metadata_cas_noop_and_group_delete_preserve_actual_membership() {
        let p = pool().await;
        let groups = vec!["engineering".into()];
        let created = created(
            create(
                &p,
                &NewOrganization {
                    name: "Acme",
                    description: "Support",
                    member_groups: &groups,
                },
                "admin",
            )
            .await
            .unwrap(),
        );
        assert_eq!(
            created.organization.created_at,
            created.organization.updated_at
        );
        assert!(
            is_member(&p, &created.organization.id, &groups)
                .await
                .unwrap()
        );
        assert!(!is_member(&p, &created.organization.id, &[]).await.unwrap());
        let same = OrganizationEdit {
            name: "Acme",
            description: "Support",
            member_groups: &groups,
            expected_revision: &created.organization.revision,
        };
        assert!(matches!(
            edit(&p, &created.organization.id, &same, "admin")
                .await
                .unwrap(),
            OrganizationEditResult::Unchanged(_)
        ));
        sqlx::query("DELETE FROM gateway_groups WHERE name='engineering'")
            .execute(&p)
            .await
            .unwrap();
        assert!(
            !is_member(&p, &created.organization.id, &groups)
                .await
                .unwrap()
        );
        let after = get_admin(&p, &created.organization.id)
            .await
            .unwrap()
            .unwrap();
        assert!(after.member_groups.is_empty());
        assert_ne!(after.organization.revision, created.organization.revision);
        assert!(matches!(
            edit(&p, &created.organization.id, &same, "admin")
                .await
                .unwrap(),
            OrganizationEditResult::Conflict
        ));
        sqlx::query("INSERT INTO gateway_groups(name,created_at,updated_at) VALUES('engineering','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')").execute(&p).await.unwrap();
        assert!(
            !is_member(&p, &created.organization.id, &groups)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn organization_agent_owner_cap_and_creator_share_commit_together() {
        let p = pool().await;
        let organization = created(
            create(
                &p,
                &NewOrganization {
                    name: "Acme",
                    description: "Support",
                    member_groups: &["engineering".into()],
                },
                "admin",
            )
            .await
            .unwrap(),
        );
        let mut tx = WriteTx::begin(&p).await.unwrap();
        let (outcome, row) = agents::create_organization_agent_in(
            &mut tx,
            &organization.organization.id,
            &NewPrincipal {
                name: "Harald",
                display: "Harald Support",
                description: "Tickets",
            },
            r#"{"main":{}}"#,
            "admin",
            1,
        )
        .await
        .unwrap();
        assert_eq!(outcome, agents::AgentCreate::Created);
        let row = row.unwrap();
        let metadata = agents::get_agent_metadata_in(&mut tx, &row.principal.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            metadata.owner,
            agents::AgentOwner::Organization(organization.organization.id.clone())
        );
        assert_eq!(metadata.name, "Harald");
        assert_eq!(
            agents::agent_owner_in(&mut tx, &row.principal.id)
                .await
                .unwrap(),
            Some(metadata.owner.clone())
        );
        assert!(
            !agents::set_agent_active_in(
                &mut tx,
                &row.principal.id,
                &agents::AgentOwner::User("admin".into()),
                false,
                1,
                "admin"
            )
            .await
            .unwrap()
        );
        assert_ne!(row.principal.name, metadata.name);
        assert!(
            agents::shares_held_in(&mut tx, &row.principal.id, "admin", &[])
                .await
                .unwrap()
                .contains(&agents::Access::Write)
        );
        tx.commit().await.unwrap();
        assert_eq!(
            agents::agent_owner(&p, &row.principal.id).await.unwrap(),
            Some(metadata.owner.clone())
        );
        assert_eq!(
            agents::get_agent_metadata(&p, &row.principal.id)
                .await
                .unwrap(),
            Some(metadata.clone())
        );
        let batch =
            agents::agent_metadata_for_agents(&p, &[row.principal.id.clone(), "missing".into()])
                .await
                .unwrap();
        assert_eq!(batch.len(), 1);
        assert_eq!(batch.get(&row.principal.id), Some(&metadata));
        assert!(
            agents::personal_agent_owner(&p, &row.principal.id)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            agents::get_personal_agent(&p, "admin", &row.principal.id)
                .await
                .unwrap()
                .is_none()
        );
        let mut tx = WriteTx::begin(&p).await.unwrap();
        let (limit, second) = agents::create_organization_agent_in(
            &mut tx,
            &organization.organization.id,
            &NewPrincipal {
                name: "Second",
                display: "Second",
                description: "",
            },
            r#"{"main":{}}"#,
            "admin",
            1,
        )
        .await
        .unwrap();
        assert_eq!(limit, agents::AgentCreate::ActiveLimit);
        assert!(second.is_none());
        assert!(
            agents::set_agent_active_in(
                &mut tx,
                &row.principal.id,
                &metadata.owner,
                false,
                1,
                "admin"
            )
            .await
            .unwrap()
        );
        tx.commit().await.unwrap();
        sqlx::query("DELETE FROM users WHERE id='admin'")
            .execute(&p)
            .await
            .unwrap();
        assert!(agents::get(&p, &row.principal.id).await.unwrap().is_some());
        assert!(
            get_admin(&p, &organization.organization.id)
                .await
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn organization_metadata_audit_is_real_user_management_and_atomic() {
        let p = pool().await;
        let created = created(
            create(
                &p,
                &NewOrganization {
                    name: "Acme",
                    description: "Support",
                    member_groups: &[],
                },
                "admin",
            )
            .await
            .unwrap(),
        );
        let event: (String,String,Option<String>,String,i64) = sqlx::query_as("SELECT principal_id,actor_id,agent_id,chain_key,seq FROM agent_audit WHERE kind='organization_created'").fetch_one(&p).await.unwrap();
        assert_eq!(
            event,
            ("admin".into(), "admin".into(), None, "user:admin".into(), 1)
        );
        assert!(
            agent_audit::for_principal(&p, "admin")
                .await
                .unwrap()
                .is_empty()
        );
        let before = list_admin(&p).await.unwrap().len();
        sqlx::query("CREATE TRIGGER refuse_organization_audit BEFORE INSERT ON agent_audit WHEN NEW.kind='organization_created' BEGIN SELECT RAISE(ABORT,'fixture audit failure'); END").execute(&p).await.unwrap();
        assert!(
            create(
                &p,
                &NewOrganization {
                    name: "Rejected",
                    description: "",
                    member_groups: &[]
                },
                "admin"
            )
            .await
            .is_err()
        );
        assert_eq!(list_admin(&p).await.unwrap().len(), before);
        let changed = edit(
            &p,
            &created.organization.id,
            &OrganizationEdit {
                name: "Renamed",
                description: "Support",
                member_groups: &[],
                expected_revision: &created.organization.revision,
            },
            "admin",
        )
        .await
        .unwrap();
        assert!(matches!(changed, OrganizationEditResult::Saved(_)));
        let kinds: Vec<String> = sqlx::query_scalar(
            "SELECT kind FROM agent_audit WHERE chain_key='user:admin' ORDER BY seq",
        )
        .fetch_all(&p)
        .await
        .unwrap();
        assert_eq!(kinds, vec!["organization_created", "organization_updated"]);
    }

    #[tokio::test]
    async fn organization_invalid_group_input_is_typed_and_leaves_no_rows_or_audit() {
        let p = pool().await;
        for groups in [
            vec!["unknown".into()],
            vec!["engineering".into(), "engineering".into()],
        ] {
            assert_eq!(
                create(
                    &p,
                    &NewOrganization {
                        name: "Rejected",
                        description: "",
                        member_groups: &groups
                    },
                    "admin"
                )
                .await
                .unwrap(),
                OrganizationCreateResult::InvalidGroups
            );
        }
        assert!(list_admin(&p).await.unwrap().is_empty());
        let organization = created(
            create(
                &p,
                &NewOrganization {
                    name: "Acme",
                    description: "",
                    member_groups: &[],
                },
                "admin",
            )
            .await
            .unwrap(),
        );
        assert_eq!(
            edit(
                &p,
                &organization.organization.id,
                &OrganizationEdit {
                    name: "Changed",
                    description: "",
                    member_groups: &["unknown".into()],
                    expected_revision: &organization.organization.revision
                },
                "admin"
            )
            .await
            .unwrap(),
            OrganizationEditResult::InvalidGroups
        );
        assert_eq!(
            get_admin(&p, &organization.organization.id)
                .await
                .unwrap()
                .unwrap(),
            organization
        );
        assert!(
            list_for_groups(&p, &["engineering".into()])
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(list_admin(&p).await.unwrap().len(), 1);
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM agent_audit WHERE kind='organization_updated'",
        )
        .fetch_one(&p)
        .await
        .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn atomic_inventory_reads_observe_outer_transaction_and_rollback() {
        let p = pool().await;
        let groups = vec!["engineering".to_owned()];
        let mut tx = WriteTx::begin(&p).await.unwrap();
        let organization = created(
            create_in(
                &mut tx,
                &NewOrganization {
                    name: "Inventory",
                    description: "Actual resource",
                    member_groups: &groups,
                },
                "admin",
            )
            .await
            .unwrap(),
        );
        let (_, owned_agent) = agents::create_organization_agent_in(
            &mut tx,
            &organization.organization.id,
            &NewPrincipal {
                name: "Inventory agent",
                display: "Support",
                description: "Tickets",
            },
            r#"{"main":{}}"#,
            "admin",
            4,
        )
        .await
        .unwrap();
        let owned_agent = owned_agent.unwrap();
        let ids = vec![owned_agent.principal.id.clone()];
        assert_eq!(
            list_admin_in(&mut tx).await.unwrap(),
            vec![organization.clone()]
        );
        assert_eq!(
            list_for_groups_in(&mut tx, &groups).await.unwrap(),
            vec![organization.organization.clone()]
        );
        assert!(
            list_for_groups_in(&mut tx, &["other".into()])
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            agents::list_all_in(&mut tx).await.unwrap(),
            vec![owned_agent.clone()]
        );
        assert_eq!(
            agents::list_shared_with_in(&mut tx, "admin", &groups)
                .await
                .unwrap(),
            vec![(owned_agent.clone(), agents::Access::Write)]
        );
        assert!(
            agents::list_shared_with_in(&mut tx, "other", &[])
                .await
                .unwrap()
                .is_empty()
        );
        let metadata = agents::agent_metadata_for_agents_in(&mut tx, &ids)
            .await
            .unwrap();
        assert_eq!(metadata.len(), 1);
        assert_eq!(metadata[&owned_agent.principal.id].name, "Inventory agent");
        assert_eq!(
            metadata[&owned_agent.principal.id].owner,
            agents::AgentOwner::Organization(organization.organization.id.clone())
        );
        assert!(
            agents::agent_metadata_for_agents_in(&mut tx, &[])
                .await
                .unwrap()
                .is_empty()
        );
        drop(tx);
        assert!(list_admin(&p).await.unwrap().is_empty());
        assert!(list_for_groups(&p, &groups).await.unwrap().is_empty());
        assert!(agents::list_all(&p).await.unwrap().is_empty());
        assert!(
            agents::list_shared_with(&p, "admin", &groups)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            agents::agent_metadata_for_agents(&p, &ids)
                .await
                .unwrap()
                .is_empty()
        );
    }
}
