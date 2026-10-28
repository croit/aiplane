// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! `agents`, their immutable `agent_versions` and their `agent_shares`
//! (`docs/agents.md` → "Agent definition").
//!
//! An agent is keyed by its system principal: [`create`] writes both in one
//! transaction and [`delete`] removes the principal, which cascades to
//! everything here. The spec is stored as JSON text and never interpreted at
//! this layer — validating it is `aiplane-runtime`'s job, so the rows stay
//! below every consumer. Every mutation records an [`agent_audit`] row in the
//! same transaction.

use std::collections::{BTreeSet, HashMap};

use aiplane_core::server::principal::GrantKind;
use jiff::Timestamp;
use serde_json::json;
use sqlx::Row;
use sqlx::sqlite::SqliteRow;

use super::agent_audit::{self, AuditKind};
use super::system_principals::{self as sp, NewPrincipal, PrincipalRow};
use super::{DbError, Pool, WriteTx};

/// What a share lets its holder do; each level includes the ones below it.
/// `Respond` answers the agent's pending inbox items from their minimal
/// context and shows nothing else of the agent, so unlike `Read` and `Write`
/// it needs no agent-management permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Access {
    Respond,
    Read,
    Write,
}

impl Access {
    pub const ALL: [Access; 3] = [Self::Respond, Self::Read, Self::Write];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Respond => "respond",
            Self::Read => "read",
            Self::Write => "write",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.as_str() == s)
    }

    /// Whether a share of this level takes effect only for a holder of the
    /// agent-management permission: it shows the spec and the conversations.
    pub fn needs_agent_manager(self) -> bool {
        self >= Self::Read
    }
}

/// Who a share is for: one person (`users.id`) or a gateway group (its name).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SubjectKind {
    User,
    Group,
}

impl SubjectKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Group => "group",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "user" => Some(Self::User),
            "group" => Some(Self::Group),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRow {
    pub principal: PrincipalRow,
    pub draft_spec: String,
    pub live_version: Option<i64>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonalDotRow {
    pub agent: AgentRow,
    pub owner_user_id: String,
    pub name: String,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrganizationDotRow {
    pub agent: AgentRow,
    pub organization_id: String,
    pub name: String,
    pub active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DotCreate {
    Created,
    NameTaken,
    ActiveLimit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionRow {
    pub version: i64,
    pub spec: String,
    pub published_by: String,
    pub published_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareRow {
    pub subject_kind: SubjectKind,
    pub subject_id: String,
    pub access: Access,
    /// Who gave the share; `None` for a share older than the record.
    pub granted_by: Option<String>,
    pub granted_at: Option<Timestamp>,
}

/// Who gave the share through which a person sees an agent, and when.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareOrigin {
    pub granted_by: String,
    pub granted_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum DotOwner {
    User(String),
    Organization(String),
}

impl DotOwner {
    pub(crate) fn columns(&self) -> (Option<&str>, Option<&str>) {
        match self {
            Self::User(id) => (Some(id), None),
            Self::Organization(id) => (None, Some(id)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DotMetadata {
    pub owner: DotOwner,
    pub name: String,
    pub active: bool,
}

fn map_dot_metadata(row: &SqliteRow) -> Result<DotMetadata, DbError> {
    let user: Option<String> = row.try_get("owner_user_id")?;
    let organization: Option<String> = row.try_get("organization_id")?;
    let owner = match (user, organization) {
        (Some(id), None) => DotOwner::User(id),
        (None, Some(id)) => DotOwner::Organization(id),
        _ => {
            return Err(DbError::Decode {
                column: "dot_owner",
                source: anyhow::anyhow!("a Dot requires exactly one owner"),
            });
        }
    };
    Ok(DotMetadata {
        owner,
        name: row.try_get("name")?,
        active: row.try_get("active")?,
    })
}

async fn dot_metadata_with<'e, E>(executor: E, id: &str) -> Result<Option<DotMetadata>, DbError>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let row = sqlx::query("SELECT d.owner_user_id,d.organization_id,d.name,d.active FROM dots d JOIN agents a ON a.principal_id=d.principal_id WHERE d.principal_id=?").bind(id).fetch_optional(executor).await?;
    row.as_ref().map(map_dot_metadata).transpose()
}

pub async fn get_dot_metadata(pool: &Pool, id: &str) -> Result<Option<DotMetadata>, DbError> {
    dot_metadata_with(pool, id).await
}
pub async fn get_dot_metadata_in(
    tx: &mut WriteTx,
    id: &str,
) -> Result<Option<DotMetadata>, DbError> {
    dot_metadata_with(&mut **tx, id).await
}
pub async fn dot_owner(pool: &Pool, id: &str) -> Result<Option<DotOwner>, DbError> {
    Ok(get_dot_metadata(pool, id).await?.map(|d| d.owner))
}
pub async fn dot_owner_in(tx: &mut WriteTx, id: &str) -> Result<Option<DotOwner>, DbError> {
    Ok(get_dot_metadata_in(tx, id).await?.map(|d| d.owner))
}

pub async fn dot_metadata_for_agents(
    pool: &Pool,
    ids: &[String],
) -> Result<HashMap<String, DotMetadata>, DbError> {
    dot_metadata_for_agents_with(pool, ids).await
}

pub async fn dot_metadata_for_agents_in(
    tx: &mut WriteTx,
    ids: &[String],
) -> Result<HashMap<String, DotMetadata>, DbError> {
    dot_metadata_for_agents_with(&mut **tx, ids).await
}

async fn dot_metadata_for_agents_with<'e, E>(
    executor: E,
    ids: &[String],
) -> Result<HashMap<String, DotMetadata>, DbError>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let rows = sqlx::query("SELECT d.principal_id,d.owner_user_id,d.organization_id,d.name,d.active FROM dots d JOIN agents a ON a.principal_id=d.principal_id WHERE d.principal_id IN (SELECT value FROM json_each(?))")
        .bind(serde_json::to_string(ids).expect("string vector serializes")).fetch_all(executor).await?;
    rows.iter()
        .map(|row| Ok((row.try_get("principal_id")?, map_dot_metadata(row)?)))
        .collect()
}

/// Why a share change was refused. Removing or downgrading the last `write`
/// share would leave an agent nobody can edit or share again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShareChange {
    Changed,
    Unchanged,
    NotFound,
    LastWriter,
}

fn agent_cols() -> String {
    let principal = sp::PRINCIPAL_COLS
        .split(", ")
        .map(|c| format!("p.{c}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{principal}, a.draft_spec, a.live_version, a.created_at AS agent_created_at, \
         a.updated_at AS agent_updated_at"
    )
}

fn map_agent(row: &SqliteRow) -> Result<AgentRow, DbError> {
    Ok(AgentRow {
        principal: sp::map_principal(row)?,
        draft_spec: row.try_get("draft_spec")?,
        live_version: row.try_get("live_version")?,
        created_at: super::parse_ts(row.try_get("agent_created_at")?, "created_at")?,
        updated_at: super::parse_ts(row.try_get("agent_updated_at")?, "updated_at")?,
    })
}

fn map_version(row: &SqliteRow) -> Result<VersionRow, DbError> {
    Ok(VersionRow {
        version: row.try_get("version")?,
        spec: row.try_get("spec")?,
        published_by: row.try_get("published_by")?,
        published_at: super::parse_ts(row.try_get("published_at")?, "published_at")?,
    })
}

fn decode_share(row: &SqliteRow) -> Result<ShareRow, DbError> {
    let kind: String = row.try_get("subject_kind")?;
    let access: String = row.try_get("access")?;
    Ok(ShareRow {
        subject_kind: SubjectKind::parse(&kind).ok_or_else(|| DbError::Decode {
            column: "subject_kind",
            source: anyhow::anyhow!("unknown share subject kind `{kind}`"),
        })?,
        subject_id: row.try_get("subject_id")?,
        access: Access::parse(&access).ok_or_else(|| DbError::Decode {
            column: "access",
            source: anyhow::anyhow!("unknown share access `{access}`"),
        })?,
        granted_by: row.try_get("granted_by")?,
        granted_at: row
            .try_get::<Option<String>, _>("granted_at")?
            .map(|at| super::parse_ts(at, "granted_at"))
            .transpose()?,
    })
}

/// Create the agent's principal (with no grants), the agent and the
/// creator's `write` share in one transaction. `Ok(None)` when the name is
/// taken.
pub async fn create(
    pool: &Pool,
    new: &NewPrincipal<'_>,
    draft_spec: &str,
    actor_id: &str,
) -> Result<Option<AgentRow>, DbError> {
    let now = Timestamp::now().to_string();
    let mut tx = WriteTx::begin(pool).await?;
    let Some(id) = sp::insert(&mut tx, new, actor_id).await? else {
        return Ok(None);
    };
    sqlx::query(
        "INSERT INTO agents (principal_id, draft_spec, live_version, created_at, updated_at)
         VALUES (?, ?, NULL, ?, ?)",
    )
    .bind(&id)
    .bind(draft_spec)
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO agent_shares (principal_id, subject_kind, subject_id, access, granted_by, granted_at)
         VALUES (?, 'user', ?, 'write', ?, ?)",
    )
    .bind(&id)
    .bind(actor_id)
    .bind(actor_id)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    agent_audit::append(
        &mut tx,
        agent_audit::NewEvent::new(AuditKind::AgentCreated, &id, json!({ "name": new.name }))
            .by(Some(actor_id)),
    )
    .await?;
    tx.commit().await?;
    get(pool, &id).await
}

/// Create a named personal Dot only when its owner has an active slot.
/// Principal, agent, personal owner and lifecycle record are committed atomically.
pub async fn create_personal_dot(
    pool: &Pool,
    new: &NewPrincipal<'_>,
    draft_spec: &str,
    owner_user_id: &str,
    active_limit: u32,
) -> Result<(DotCreate, Option<AgentRow>), DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let result =
        create_personal_dot_in(&mut tx, owner_user_id, new, draft_spec, active_limit).await?;
    tx.commit().await?;
    Ok(result)
}

pub async fn create_personal_dot_in(
    tx: &mut WriteTx,
    owner_user_id: &str,
    new: &NewPrincipal<'_>,
    draft_spec: &str,
    active_limit: u32,
) -> Result<(DotCreate, Option<AgentRow>), DbError> {
    create_dot_in(
        tx,
        &DotOwner::User(owner_user_id.into()),
        new,
        draft_spec,
        owner_user_id,
        active_limit,
    )
    .await
}

pub async fn create_organization_dot_in(
    tx: &mut WriteTx,
    organization_id: &str,
    new: &NewPrincipal<'_>,
    draft_spec: &str,
    creator: &str,
    active_limit: u32,
) -> Result<(DotCreate, Option<AgentRow>), DbError> {
    create_dot_in(
        tx,
        &DotOwner::Organization(organization_id.into()),
        new,
        draft_spec,
        creator,
        active_limit,
    )
    .await
}

pub async fn active_dot_count_in(tx: &mut WriteTx, owner: &DotOwner) -> Result<u32, DbError> {
    let (user, organization) = owner.columns();
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM dots WHERE owner_user_id IS ? AND organization_id IS ? AND active=1",
    )
    .bind(user)
    .bind(organization)
    .fetch_one(&mut **tx)
    .await?;
    u32::try_from(count).map_err(|error| DbError::Decode {
        column: "active_dots",
        source: error.into(),
    })
}

async fn create_dot_in(
    tx: &mut WriteTx,
    owner: &DotOwner,
    new: &NewPrincipal<'_>,
    draft_spec: &str,
    creator: &str,
    active_limit: u32,
) -> Result<(DotCreate, Option<AgentRow>), DbError> {
    let (user, organization) = owner.columns();
    let taken:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM dots WHERE owner_user_id IS ? AND organization_id IS ? AND name=?)")
        .bind(user).bind(organization).bind(new.name).fetch_one(&mut **tx).await?;
    if taken {
        return Ok((DotCreate::NameTaken, None));
    }
    if active_dot_count_in(tx, owner).await? >= active_limit {
        return Ok((DotCreate::ActiveLimit, None));
    }
    let prefix = match owner {
        DotOwner::User(_) => "personal",
        DotOwner::Organization(_) => "organization",
    };
    let name = format!("{prefix}-dot-{}", uuid::Uuid::new_v4());
    let Some(id) = sp::insert(
        tx,
        &NewPrincipal {
            name: &name,
            display: new.display,
            description: new.description,
        },
        creator,
    )
    .await?
    else {
        return Ok((DotCreate::NameTaken, None));
    };
    let now = Timestamp::now().to_string();
    sqlx::query("INSERT INTO agents(principal_id,draft_spec,live_version,created_at,updated_at) VALUES(?,?,NULL,?,?)")
        .bind(&id).bind(draft_spec).bind(&now).bind(&now).execute(&mut **tx).await?;
    sqlx::query("INSERT INTO dots(principal_id,owner_user_id,organization_id,name,active,created_at,updated_at) VALUES(?,?,?,?,1,?,?)")
        .bind(&id).bind(user).bind(organization).bind(new.name).bind(&now).bind(&now).execute(&mut **tx).await?;
    if matches!(owner, DotOwner::Organization(_)) {
        sqlx::query("INSERT INTO agent_shares(principal_id,subject_kind,subject_id,access,granted_by,granted_at) VALUES(?,'user',?,'write',?,?)")
            .bind(&id).bind(creator).bind(creator).bind(&now).execute(&mut **tx).await?;
    }
    agent_audit::append(
        tx,
        agent_audit::NewEvent::new(
            AuditKind::AgentCreated,
            &id,
            json!({"name":new.name,"owner":owner}),
        )
        .by(Some(creator)),
    )
    .await?;
    Ok((DotCreate::Created, get_with(&mut **tx, &id).await?))
}

pub async fn set_personal_dot_active(
    pool: &Pool,
    id: &str,
    owner_user_id: &str,
    active: bool,
    active_limit: u32,
) -> Result<bool, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let result = set_dot_active_in(
        &mut tx,
        id,
        &DotOwner::User(owner_user_id.into()),
        active,
        active_limit,
        owner_user_id,
    )
    .await?;
    tx.commit().await?;
    Ok(result)
}

pub async fn set_dot_active_in(
    tx: &mut WriteTx,
    id: &str,
    owner: &DotOwner,
    active: bool,
    active_limit: u32,
    actor: &str,
) -> Result<bool, DbError> {
    let Some(current) = get_dot_metadata_in(tx, id).await? else {
        return Ok(false);
    };
    if &current.owner != owner {
        return Ok(false);
    }
    if current.active == active {
        return Ok(true);
    }
    if active && active_dot_count_in(tx, owner).await? >= active_limit {
        return Ok(false);
    }
    sqlx::query("UPDATE dots SET active=?,updated_at=? WHERE principal_id=?")
        .bind(active)
        .bind(Timestamp::now().to_string())
        .bind(id)
        .execute(&mut **tx)
        .await?;
    agent_audit::append(
        tx,
        agent_audit::NewEvent::new(
            AuditKind::DotStateChanged,
            id,
            json!({"active":active,"owner":owner}),
        )
        .by(Some(actor)),
    )
    .await?;
    Ok(true)
}

/// List the personal Dots owned by one user, including inactive ones.
pub async fn list_personal_dots(
    pool: &Pool,
    owner_user_id: &str,
) -> Result<Vec<PersonalDotRow>, DbError> {
    let rows = sqlx::query(&format!(
        "SELECT {}, d.owner_user_id, d.name AS dot_name, d.active FROM agents a
         JOIN system_principals p ON p.id = a.principal_id
         JOIN dots d ON d.principal_id = a.principal_id
         WHERE d.owner_user_id = ? AND d.organization_id IS NULL ORDER BY a.created_at, p.name",
        agent_cols()
    ))
    .bind(owner_user_id)
    .fetch_all(pool)
    .await?;
    rows.iter().map(map_personal_dot).collect()
}

fn map_personal_dot(row: &SqliteRow) -> Result<PersonalDotRow, DbError> {
    Ok(PersonalDotRow {
        agent: map_agent(row)?,
        owner_user_id: row.try_get("owner_user_id")?,
        name: row.try_get("dot_name")?,
        active: row.try_get("active")?,
    })
}

/// The active organization Dots the user may discover as cross-boundary
/// receivers: one per organization the user is a current member of, so a
/// personal Dot can offer an organization Dot only while membership is live.
/// `role_ids` are the caller owner's role IDs resolved against the current
/// RBAC snapshot, the same values the organization membership checks use.
pub async fn list_organization_dots_for_member(
    pool: &Pool,
    role_ids: &[String],
) -> Result<Vec<OrganizationDotRow>, DbError> {
    let rows = sqlx::query(&format!(
        "SELECT {}, d.organization_id, d.name AS dot_name, d.active FROM agents a
         JOIN system_principals p ON p.id = a.principal_id
         JOIN dots d ON d.principal_id = a.principal_id
         WHERE d.owner_user_id IS NULL AND d.organization_id IS NOT NULL AND d.active = 1
           AND EXISTS (
               SELECT 1 FROM organization_groups b
               JOIN gateway_groups g ON g.name = b.gateway_group
               WHERE b.organization_id = d.organization_id
                 AND g.name IN (SELECT value FROM json_each(?))
           )
         ORDER BY a.created_at, p.name",
        agent_cols()
    ))
    .bind(serde_json::to_string(role_ids).expect("string vector serializes"))
    .fetch_all(pool)
    .await?;
    rows.iter().map(map_organization_dot).collect()
}

fn map_organization_dot(row: &SqliteRow) -> Result<OrganizationDotRow, DbError> {
    Ok(OrganizationDotRow {
        agent: map_agent(row)?,
        organization_id: row.try_get("organization_id")?,
        name: row.try_get("dot_name")?,
        active: row.try_get("active")?,
    })
}

/// Load one personal Dot only when `owner_user_id` owns it.
pub async fn get_personal_dot(
    pool: &Pool,
    owner_user_id: &str,
    id: &str,
) -> Result<Option<PersonalDotRow>, DbError> {
    let row = sqlx::query(&format!(
        "SELECT {}, d.owner_user_id, d.name AS dot_name, d.active FROM agents a
         JOIN system_principals p ON p.id = a.principal_id
         JOIN dots d ON d.principal_id = a.principal_id
         WHERE d.owner_user_id = ? AND d.organization_id IS NULL AND d.principal_id = ?",
        agent_cols()
    ))
    .bind(owner_user_id)
    .bind(id)
    .fetch_optional(pool)
    .await?;
    row.as_ref().map(map_personal_dot).transpose()
}

pub async fn personal_dot_owner(pool: &Pool, id: &str) -> Result<Option<String>, DbError> {
    Ok(sqlx::query_scalar("SELECT d.owner_user_id FROM dots d JOIN agents a ON a.principal_id = d.principal_id WHERE d.principal_id = ? AND d.owner_user_id IS NOT NULL AND d.organization_id IS NULL")
        .bind(id).fetch_optional(pool).await?)
}

pub async fn get(pool: &Pool, id: &str) -> Result<Option<AgentRow>, DbError> {
    get_with(pool, id).await
}

pub async fn get_in(tx: &mut WriteTx, id: &str) -> Result<Option<AgentRow>, DbError> {
    get_with(&mut **tx, id).await
}

async fn get_with<'e, E>(executor: E, id: &str) -> Result<Option<AgentRow>, DbError>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let row = sqlx::query(&format!(
        "SELECT {} FROM agents a JOIN system_principals p ON p.id = a.principal_id
          WHERE a.principal_id = ?",
        agent_cols()
    ))
    .bind(id)
    .fetch_optional(executor)
    .await?;
    row.as_ref().map(map_agent).transpose()
}

/// Every agent id, mapped to whether it has a live version. What the spec
/// validator checks sub-agent references against.
pub async fn publication_status(pool: &Pool) -> Result<HashMap<String, bool>, DbError> {
    let rows = sqlx::query("SELECT principal_id, live_version IS NOT NULL AS live FROM agents")
        .fetch_all(pool)
        .await?;
    rows.iter()
        .map(|row| Ok((row.try_get("principal_id")?, row.try_get("live")?)))
        .collect()
}

/// `(sql, binds)` matching the shares held by a person directly or through
/// one of their groups.
fn holder_clause(user_id: &str, group_ids: &[String]) -> (String, Vec<String>) {
    let mut binds = vec![user_id.to_string()];
    let mut sql = "(s.subject_kind = 'user' AND s.subject_id = ?)".to_string();
    if !group_ids.is_empty() {
        let marks = vec!["?"; group_ids.len()].join(", ");
        sql = format!("({sql} OR (s.subject_kind = 'group' AND s.subject_id IN ({marks})))");
        binds.extend(group_ids.iter().cloned());
    }
    (sql, binds)
}

/// Every share level a person holds on an agent, directly or through any of
/// `group_ids`. Which of them take effect ([`Access::needs_agent_manager`])
/// is the caller's check.
pub async fn shares_held(
    pool: &Pool,
    id: &str,
    user_id: &str,
    group_ids: &[String],
) -> Result<BTreeSet<Access>, DbError> {
    shares_held_with(pool, id, user_id, group_ids).await
}

pub async fn shares_held_in(
    tx: &mut WriteTx,
    id: &str,
    user_id: &str,
    group_ids: &[String],
) -> Result<BTreeSet<Access>, DbError> {
    shares_held_with(&mut **tx, id, user_id, group_ids).await
}

async fn shares_held_with<'e, E>(
    executor: E,
    id: &str,
    user_id: &str,
    group_ids: &[String],
) -> Result<BTreeSet<Access>, DbError>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let (holder, binds) = holder_clause(user_id, group_ids);
    let sql = format!("SELECT s.access FROM agent_shares s WHERE s.principal_id = ? AND {holder}");
    let mut query = sqlx::query(&sql).bind(id);
    for b in &binds {
        query = query.bind(b);
    }
    let rows = query.fetch_all(executor).await?;
    let mut held = BTreeSet::new();
    for row in &rows {
        let access: String = row.try_get("access")?;
        held.extend(Access::parse(&access));
    }
    Ok(held)
}

/// For each agent a person holds a share on, directly or through any of
/// `group_ids`, who gave the most recent of those shares and when. Shares
/// older than the record name no one and are left out.
pub async fn share_origins_in(
    tx: &mut WriteTx,
    user_id: &str,
    group_ids: &[String],
) -> Result<HashMap<String, ShareOrigin>, DbError> {
    let (holder, binds) = holder_clause(user_id, group_ids);
    let sql = format!(
        "SELECT s.principal_id, s.granted_by, s.granted_at FROM agent_shares s
          WHERE {holder} AND s.granted_by IS NOT NULL AND s.granted_at IS NOT NULL
          ORDER BY s.granted_at"
    );
    let mut query = sqlx::query(&sql);
    for b in &binds {
        query = query.bind(b);
    }
    let mut origins = HashMap::new();
    for row in query.fetch_all(&mut **tx).await? {
        origins.insert(
            row.try_get("principal_id")?,
            ShareOrigin {
                granted_by: row.try_get("granted_by")?,
                granted_at: super::parse_ts(row.try_get("granted_at")?, "granted_at")?,
            },
        );
    }
    Ok(origins)
}

/// The agents a person holds a `read` or `write` share on, by name, with
/// their strongest access. A `respond` share shows no agent.
pub async fn list_shared_with(
    pool: &Pool,
    user_id: &str,
    group_ids: &[String],
) -> Result<Vec<(AgentRow, Access)>, DbError> {
    shared_agents_with(pool, user_id, group_ids).await
}

pub async fn list_shared_with_in(
    tx: &mut WriteTx,
    user_id: &str,
    group_ids: &[String],
) -> Result<Vec<(AgentRow, Access)>, DbError> {
    shared_agents_with(&mut **tx, user_id, group_ids).await
}

async fn shared_agents_with<'e, E>(
    executor: E,
    user_id: &str,
    group_ids: &[String],
) -> Result<Vec<(AgentRow, Access)>, DbError>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let (holder, binds) = holder_clause(user_id, group_ids);
    let sql = format!(
        "SELECT {}, MAX(CASE s.access WHEN 'write' THEN 1 ELSE 0 END) AS can_write
           FROM agents a
           JOIN system_principals p ON p.id = a.principal_id
           JOIN agent_shares s ON s.principal_id = a.principal_id
          WHERE s.access IN ('read', 'write') AND {holder}
          GROUP BY a.principal_id
          ORDER BY p.name",
        agent_cols()
    );
    let mut query = sqlx::query(&sql);
    for b in &binds {
        query = query.bind(b);
    }
    let rows = query.fetch_all(executor).await?;
    rows.iter()
        .map(|row| {
            let access = if row.try_get::<i64, _>("can_write")? != 0 {
                Access::Write
            } else {
                Access::Read
            };
            Ok((map_agent(row)?, access))
        })
        .collect()
}

/// Every agent, by name — what an admin sees, shares or not.
pub async fn list_all(pool: &Pool) -> Result<Vec<AgentRow>, DbError> {
    list_all_with(pool).await
}

pub async fn list_all_in(tx: &mut WriteTx) -> Result<Vec<AgentRow>, DbError> {
    list_all_with(&mut **tx).await
}

async fn list_all_with<'e, E>(executor: E) -> Result<Vec<AgentRow>, DbError>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let sql = format!(
        "SELECT {} FROM agents a JOIN system_principals p ON p.id = a.principal_id ORDER BY p.name",
        agent_cols()
    );
    let rows = sqlx::query(&sql).fetch_all(executor).await?;
    rows.iter().map(map_agent).collect()
}

/// How many earlier drafts of one agent are kept for undo.
pub const MAX_DRAFT_REVISIONS: i64 = 50;

/// A saved draft: the revision that holds the draft as it was before, `None`
/// when the save changed nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DraftSaved {
    pub revision: Option<i64>,
}

/// An earlier draft, kept so a change can be undone.
#[derive(Debug, Clone)]
pub struct RevisionRow {
    pub id: i64,
    pub spec: String,
    /// The grants the change after this draft made.
    pub granted: Vec<(GrantKind, String)>,
    pub saved_by: String,
    pub saved_at: String,
}

/// What a draft save is, besides the new draft: for the revision it keeps
/// and the activity log.
#[derive(Debug, Clone, Copy, Default)]
pub struct DraftChange<'a> {
    /// Grants made for this change (an architect's), revoked when it is undone.
    pub granted: &'a [(GrantKind, String)],
    /// The revision this save restores (an undo).
    pub restored: Option<i64>,
    /// Grants the undo revokes after the save.
    pub revoking: &'a [(GrantKind, String)],
}

fn grants_json(grants: &[(GrantKind, String)]) -> serde_json::Value {
    grants
        .iter()
        .map(|(kind, reference)| json!({ "kind": kind.as_str(), "ref": reference }))
        .collect()
}

fn parse_grants(text: &str) -> Vec<(GrantKind, String)> {
    serde_json::from_str::<Vec<serde_json::Value>>(text)
        .unwrap_or_default()
        .iter()
        .filter_map(|g| {
            Some((
                GrantKind::parse(g.get("kind")?.as_str()?)?,
                g.get("ref")?.as_str()?.to_string(),
            ))
        })
        .collect()
}

/// Replace the draft, keeping the one it replaces as a revision (the newest
/// [`MAX_DRAFT_REVISIONS`] per agent). The live version is untouched.
/// `Ok(None)` when there is no such agent.
pub async fn update_draft(
    pool: &Pool,
    id: &str,
    draft_spec: &str,
    actor_id: &str,
    change: DraftChange<'_>,
) -> Result<Option<DraftSaved>, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let result = update_draft_in(&mut tx, id, draft_spec, actor_id, change).await?;
    tx.commit().await?;
    Ok(result)
}

pub async fn update_draft_in(
    tx: &mut WriteTx,
    id: &str,
    draft_spec: &str,
    actor_id: &str,
    change: DraftChange<'_>,
) -> Result<Option<DraftSaved>, DbError> {
    let before: Option<String> =
        sqlx::query_scalar("SELECT draft_spec FROM agents WHERE principal_id = ?")
            .bind(id)
            .fetch_optional(&mut **tx)
            .await?;
    let Some(before) = before else {
        return Ok(None);
    };
    let saved = save_draft(tx, id, &before, draft_spec, actor_id, change).await?;
    Ok(Some(saved))
}

#[derive(Debug, PartialEq, Eq)]
pub enum PersonalDotDraftSaved {
    Saved,
    Missing,
    Conflict,
}

pub async fn update_personal_dot_draft(
    pool: &Pool,
    id: &str,
    owner_user_id: &str,
    expected_spec: &str,
    draft_spec: &str,
) -> Result<PersonalDotDraftSaved, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let before: Option<String> = sqlx::query_scalar(
        "SELECT a.draft_spec FROM agents a JOIN dots d ON d.principal_id = a.principal_id
         WHERE a.principal_id = ? AND d.owner_user_id = ?",
    )
    .bind(id)
    .bind(owner_user_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(before) = before else {
        return Ok(PersonalDotDraftSaved::Missing);
    };
    if before != expected_spec {
        return Ok(PersonalDotDraftSaved::Conflict);
    }
    save_draft(
        &mut tx,
        id,
        &before,
        draft_spec,
        owner_user_id,
        DraftChange::default(),
    )
    .await?;
    tx.commit().await?;
    Ok(PersonalDotDraftSaved::Saved)
}

async fn save_draft(
    tx: &mut WriteTx,
    id: &str,
    before: &str,
    draft_spec: &str,
    actor_id: &str,
    change: DraftChange<'_>,
) -> Result<DraftSaved, DbError> {
    let now = Timestamp::now().to_string();
    let revision = if before == draft_spec {
        None
    } else {
        let revision: i64 = sqlx::query_scalar(
            "INSERT INTO agent_draft_revisions (principal_id, spec, granted, saved_by, saved_at)
             VALUES (?, ?, ?, ?, ?) RETURNING id",
        )
        .bind(id)
        .bind(before)
        .bind(grants_json(change.granted).to_string())
        .bind(actor_id)
        .bind(&now)
        .fetch_one(&mut **tx)
        .await?;
        sqlx::query(
            "DELETE FROM agent_draft_revisions WHERE principal_id = ? AND id NOT IN (
                SELECT id FROM agent_draft_revisions WHERE principal_id = ?
                 ORDER BY id DESC LIMIT ?)",
        )
        .bind(id)
        .bind(id)
        .bind(MAX_DRAFT_REVISIONS)
        .execute(&mut **tx)
        .await?;
        Some(revision)
    };
    sqlx::query("UPDATE agents SET draft_spec = ?, updated_at = ? WHERE principal_id = ?")
        .bind(draft_spec)
        .bind(&now)
        .bind(id)
        .execute(&mut **tx)
        .await?;
    agent_audit::append(
        tx,
        agent_audit::NewEvent::new(
            AuditKind::AgentDraftUpdated,
            id,
            json!({
                "bytes": draft_spec.len(),
                "revision": revision,
                "granted": grants_json(change.granted),
                "restored": change.restored,
                "revoking": grants_json(change.revoking),
            }),
        )
        .by(Some(actor_id)),
    )
    .await?;
    Ok(DraftSaved { revision })
}

/// Revision `revision` of agent `id`'s draft, if it is still kept.
pub async fn draft_revision(
    pool: &Pool,
    id: &str,
    revision: i64,
) -> Result<Option<RevisionRow>, DbError> {
    let row = sqlx::query(
        "SELECT id, spec, granted, saved_by, saved_at FROM agent_draft_revisions
          WHERE principal_id = ? AND id = ?",
    )
    .bind(id)
    .bind(revision)
    .fetch_optional(pool)
    .await?;
    row.map(|r| {
        Ok(RevisionRow {
            id: r.try_get("id")?,
            spec: r.try_get("spec")?,
            granted: parse_grants(&r.try_get::<String, _>("granted")?),
            saved_by: r.try_get("saved_by")?,
            saved_at: r.try_get("saved_at")?,
        })
    })
    .transpose()
}

/// Snapshot `spec` — the draft as the caller validated it — as the next
/// version and make it live. Taking the text rather than re-reading the
/// draft means the version is exactly what passed validation, even if
/// someone saved the draft in between. `Ok(None)` when there is no such
/// agent.
pub async fn publish(
    pool: &Pool,
    id: &str,
    spec: &str,
    actor_id: &str,
) -> Result<Option<i64>, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let result = publish_in(&mut tx, id, spec, actor_id).await?;
    tx.commit().await?;
    Ok(result)
}

pub async fn publish_in(
    tx: &mut WriteTx,
    id: &str,
    spec: &str,
    actor_id: &str,
) -> Result<Option<i64>, DbError> {
    let now = Timestamp::now().to_string();
    let exists = sqlx::query("SELECT 1 FROM agents WHERE principal_id = ?")
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .is_some();
    if !exists {
        return Ok(None);
    }
    let version: i64 = sqlx::query(
        "SELECT COALESCE(MAX(version), 0) + 1 AS next FROM agent_versions WHERE principal_id = ?",
    )
    .bind(id)
    .fetch_one(&mut **tx)
    .await?
    .try_get("next")?;
    sqlx::query(
        "INSERT INTO agent_versions (principal_id, version, spec, published_by, published_at)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(version)
    .bind(spec)
    .bind(actor_id)
    .bind(&now)
    .execute(&mut **tx)
    .await?;
    sqlx::query("UPDATE agents SET live_version = ?, updated_at = ? WHERE principal_id = ?")
        .bind(version)
        .bind(&now)
        .bind(id)
        .execute(&mut **tx)
        .await?;
    agent_audit::append(
        tx,
        agent_audit::NewEvent::new(AuditKind::AgentPublished, id, json!({ "version": version }))
            .by(Some(actor_id)),
    )
    .await?;
    Ok(Some(version))
}

/// Newest first.
pub async fn versions(pool: &Pool, id: &str) -> Result<Vec<VersionRow>, DbError> {
    versions_with(pool, id).await
}

pub async fn versions_in(tx: &mut WriteTx, id: &str) -> Result<Vec<VersionRow>, DbError> {
    versions_with(&mut **tx, id).await
}

async fn versions_with<'e, E>(executor: E, id: &str) -> Result<Vec<VersionRow>, DbError>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let rows = sqlx::query(
        "SELECT version, spec, published_by, published_at FROM agent_versions
          WHERE principal_id = ? ORDER BY version DESC",
    )
    .bind(id)
    .fetch_all(executor)
    .await?;
    rows.iter().map(map_version).collect()
}

pub async fn version(pool: &Pool, id: &str, version: i64) -> Result<Option<VersionRow>, DbError> {
    version_with(pool, id, version).await
}

pub async fn version_in(
    tx: &mut WriteTx,
    id: &str,
    version: i64,
) -> Result<Option<VersionRow>, DbError> {
    version_with(&mut **tx, id, version).await
}

async fn version_with<'e, E>(
    executor: E,
    id: &str,
    version: i64,
) -> Result<Option<VersionRow>, DbError>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let row = sqlx::query(
        "SELECT version, spec, published_by, published_at FROM agent_versions
          WHERE principal_id = ? AND version = ?",
    )
    .bind(id)
    .bind(version)
    .fetch_optional(executor)
    .await?;
    row.as_ref().map(map_version).transpose()
}

/// The version number an internal test chat records for a run of the draft.
/// Published versions start at 1, so a session whose `agent_version` is 0 is a
/// test conversation, and no published version can ever match it.
pub const DRAFT_VERSION: i64 = 0;

/// The live version of agent `id` and its spec; `None` when it was never
/// published or does not exist.
pub async fn live(pool: &Pool, id: &str) -> Result<Option<(i64, String)>, DbError> {
    let row = sqlx::query(
        "SELECT v.version, v.spec FROM agents a
           JOIN agent_versions v ON v.principal_id = a.principal_id AND v.version = a.live_version
          WHERE a.principal_id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    row.map(|r| Ok((r.try_get("version")?, r.try_get("spec")?)))
        .transpose()
}

/// The number of agent `id`'s live version, without its spec: what a caller
/// holding compiled versions reads to know which one is live. `None` when it
/// was never published or does not exist.
pub async fn live_version(pool: &Pool, id: &str) -> Result<Option<i64>, DbError> {
    let row = sqlx::query("SELECT live_version FROM agents WHERE principal_id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row
        .map(|r| r.try_get::<Option<i64>, _>("live_version"))
        .transpose()?
        .flatten())
}

/// Every published agent's live spec, by agent id. What the spec validator
/// walks the sub-agent graph over.
pub async fn live_specs(pool: &Pool) -> Result<HashMap<String, String>, DbError> {
    let rows = sqlx::query(
        "SELECT a.principal_id, v.spec FROM agents a
           JOIN agent_versions v ON v.principal_id = a.principal_id AND v.version = a.live_version",
    )
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|row| Ok((row.try_get("principal_id")?, row.try_get("spec")?)))
        .collect()
}

/// Point `live_version` at an existing version — a rollback, or a roll
/// forward again. `Ok(false)` when the agent has no such version.
pub async fn set_live(
    pool: &Pool,
    id: &str,
    version: i64,
    actor_id: &str,
) -> Result<bool, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let result = set_live_in(&mut tx, id, version, actor_id).await?;
    tx.commit().await?;
    Ok(result)
}

pub async fn set_live_in(
    tx: &mut WriteTx,
    id: &str,
    version: i64,
    actor_id: &str,
) -> Result<bool, DbError> {
    let changed = sqlx::query(
        "UPDATE agents SET live_version = ?1, updated_at = ?2
          WHERE principal_id = ?3
            AND EXISTS (SELECT 1 FROM agent_versions WHERE principal_id = ?3 AND version = ?1)",
    )
    .bind(version)
    .bind(Timestamp::now().to_string())
    .bind(id)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    if changed == 0 {
        return Ok(false);
    }
    agent_audit::append(
        tx,
        agent_audit::NewEvent::new(
            AuditKind::AgentLiveVersionSet,
            id,
            json!({ "version": version }),
        )
        .by(Some(actor_id)),
    )
    .await?;
    Ok(true)
}

pub async fn shares(pool: &Pool, id: &str) -> Result<Vec<ShareRow>, DbError> {
    shares_with(pool, id).await
}

pub async fn shares_in(tx: &mut WriteTx, id: &str) -> Result<Vec<ShareRow>, DbError> {
    shares_with(&mut **tx, id).await
}

async fn shares_with<'e, E>(executor: E, id: &str) -> Result<Vec<ShareRow>, DbError>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let rows = sqlx::query(
        "SELECT subject_kind, subject_id, access, granted_by, granted_at FROM agent_shares
          WHERE principal_id = ? ORDER BY subject_kind, subject_id",
    )
    .bind(id)
    .fetch_all(executor)
    .await?;
    rows.iter().map(decode_share).collect()
}

async fn other_writers(
    conn: &mut sqlx::SqliteConnection,
    id: &str,
    kind: SubjectKind,
    subject_id: &str,
) -> Result<i64, DbError> {
    Ok(sqlx::query(
        "SELECT COUNT(*) AS n FROM agent_shares
          WHERE principal_id = ? AND access = 'write'
            AND NOT (subject_kind = ? AND subject_id = ?)",
    )
    .bind(id)
    .bind(kind.as_str())
    .bind(subject_id)
    .fetch_one(conn)
    .await?
    .try_get("n")?)
}

async fn current_share(
    conn: &mut sqlx::SqliteConnection,
    id: &str,
    kind: SubjectKind,
    subject_id: &str,
) -> Result<Option<Access>, DbError> {
    let row = sqlx::query(
        "SELECT access FROM agent_shares
          WHERE principal_id = ? AND subject_kind = ? AND subject_id = ?",
    )
    .bind(id)
    .bind(kind.as_str())
    .bind(subject_id)
    .fetch_optional(conn)
    .await?;
    Ok(row
        .map(|r| r.try_get::<String, _>("access"))
        .transpose()?
        .and_then(|a| Access::parse(&a)))
}

/// Add a share or change its access. Whether the holder may hold one at all
/// is the caller's check.
pub async fn set_share(
    pool: &Pool,
    id: &str,
    kind: SubjectKind,
    subject_id: &str,
    access: Access,
    actor_id: &str,
) -> Result<ShareChange, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let result = set_share_in(&mut tx, id, kind, subject_id, access, actor_id).await?;
    tx.commit().await?;
    Ok(result)
}

pub async fn set_share_in(
    tx: &mut WriteTx,
    id: &str,
    kind: SubjectKind,
    subject_id: &str,
    access: Access,
    actor_id: &str,
) -> Result<ShareChange, DbError> {
    let current = current_share(tx, id, kind, subject_id).await?;
    if current == Some(access) {
        return Ok(ShareChange::Unchanged);
    }
    if current == Some(Access::Write) && other_writers(tx, id, kind, subject_id).await? == 0 {
        return Ok(ShareChange::LastWriter);
    }
    sqlx::query(
        "INSERT INTO agent_shares (principal_id, subject_kind, subject_id, access, granted_by, granted_at)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT(principal_id, subject_kind, subject_id) DO UPDATE SET
             access = excluded.access, granted_by = excluded.granted_by, granted_at = excluded.granted_at",
    )
    .bind(id)
    .bind(kind.as_str())
    .bind(subject_id)
    .bind(access.as_str())
    .bind(actor_id)
    .bind(Timestamp::now().to_string())
    .execute(&mut **tx)
    .await?;
    agent_audit::append(tx, agent_audit::NewEvent::new(AuditKind::AgentShareSet, id, json!({ "subject_kind": kind.as_str(), "subject_id": subject_id, "access": access.as_str() })).by(Some(actor_id)))
    .await?;
    Ok(ShareChange::Changed)
}

pub async fn remove_share(
    pool: &Pool,
    id: &str,
    kind: SubjectKind,
    subject_id: &str,
    actor_id: &str,
) -> Result<ShareChange, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let result = remove_share_in(&mut tx, id, kind, subject_id, actor_id).await?;
    tx.commit().await?;
    Ok(result)
}

pub async fn remove_share_in(
    tx: &mut WriteTx,
    id: &str,
    kind: SubjectKind,
    subject_id: &str,
    actor_id: &str,
) -> Result<ShareChange, DbError> {
    let Some(current) = current_share(tx, id, kind, subject_id).await? else {
        return Ok(ShareChange::NotFound);
    };
    if current == Access::Write && other_writers(tx, id, kind, subject_id).await? == 0 {
        return Ok(ShareChange::LastWriter);
    }
    sqlx::query(
        "DELETE FROM agent_shares WHERE principal_id = ? AND subject_kind = ? AND subject_id = ?",
    )
    .bind(id)
    .bind(kind.as_str())
    .bind(subject_id)
    .execute(&mut **tx)
    .await?;
    agent_audit::append(
        tx,
        agent_audit::NewEvent::new(
            AuditKind::AgentShareRemoved,
            id,
            json!({ "subject_kind": kind.as_str(), "subject_id": subject_id }),
        )
        .by(Some(actor_id)),
    )
    .await?;
    Ok(ShareChange::Changed)
}

/// Delete the agent together with its principal, grants, tokens, versions and
/// shares. The audit trail survives: it has no foreign keys. `Ok(false)` when
/// there is no such agent.
pub async fn delete(pool: &Pool, id: &str, actor_id: &str) -> Result<bool, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let result = delete_in(&mut tx, id, actor_id).await?;
    tx.commit().await?;
    if result {
        super::embed_keys::origins_changed();
    }
    Ok(result)
}

pub async fn delete_in(tx: &mut WriteTx, id: &str, actor_id: &str) -> Result<bool, DbError> {
    let name: Option<String> = sqlx::query(
        "SELECT p.name FROM agents a JOIN system_principals p ON p.id = a.principal_id
          WHERE a.principal_id = ?",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?
    .map(|r| r.try_get("name"))
    .transpose()?;
    let Some(name) = name else {
        return Ok(false);
    };
    sqlx::query("DELETE FROM system_principals WHERE id = ?")
        .bind(id)
        .execute(&mut **tx)
        .await?;
    agent_audit::append(
        tx,
        agent_audit::NewEvent::new(AuditKind::AgentDeleted, id, json!({ "name": name }))
            .by(Some(actor_id)),
    )
    .await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiplane_core::server::db::users::{self, User};
    use aiplane_core::server::principal::GrantKind;
    use jiff::Timestamp;
    use std::path::Path;

    async fn pool() -> Pool {
        aiplane_core::server::db::open(Path::new(":memory:"))
            .await
            .unwrap()
    }

    async fn agent(pool: &Pool, name: &str, creator: &str) -> AgentRow {
        create(
            pool,
            &NewPrincipal {
                name,
                display: "Support",
                description: "",
            },
            r#"{"main":{}}"#,
            creator,
        )
        .await
        .unwrap()
        .unwrap()
    }

    #[tokio::test]
    async fn personal_dot_draft_save_checks_owner_and_expected_revision() {
        let pool = pool().await;
        let now = Timestamp::now();
        users::upsert(
            &pool,
            &User {
                id: "alice".into(),
                email: "alice@example.com".into(),
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
        let before = r#"{"main":{"tools":[]}}"#;
        let (_, dot) = create_personal_dot(
            &pool,
            &NewPrincipal {
                name: "Access",
                display: "Access",
                description: "",
            },
            before,
            "alice",
            4,
        )
        .await
        .unwrap();
        let id = &dot.unwrap().principal.id;
        assert_eq!(
            personal_dot_owner(&pool, id).await.unwrap().as_deref(),
            Some("alice")
        );
        assert!(
            personal_dot_owner(&pool, "missing")
                .await
                .unwrap()
                .is_none()
        );
        let ordinary = agent(&pool, "Ordinary", "alice").await;
        assert!(
            personal_dot_owner(&pool, &ordinary.principal.id)
                .await
                .unwrap()
                .is_none()
        );
        sqlx::query("UPDATE system_principals SET created_by = 'someone_else' WHERE id = ?")
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            personal_dot_owner(&pool, id).await.unwrap().as_deref(),
            Some("alice")
        );
        let after = r#"{"main":{"tools":["recall"]}}"#;
        assert_eq!(
            update_personal_dot_draft(&pool, id, "bob", before, after)
                .await
                .unwrap(),
            PersonalDotDraftSaved::Missing
        );
        assert_eq!(
            update_personal_dot_draft(&pool, id, "alice", "stale", after)
                .await
                .unwrap(),
            PersonalDotDraftSaved::Conflict
        );
        assert_eq!(
            update_personal_dot_draft(&pool, id, "alice", before, after)
                .await
                .unwrap(),
            PersonalDotDraftSaved::Saved
        );
        assert_eq!(
            get_personal_dot(&pool, "alice", id)
                .await
                .unwrap()
                .unwrap()
                .agent
                .draft_spec,
            after
        );
        let revision: i64 =
            sqlx::query_scalar("SELECT id FROM agent_draft_revisions WHERE principal_id = ?")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            draft_revision(&pool, id, revision)
                .await
                .unwrap()
                .unwrap()
                .spec,
            before
        );
        assert_eq!(
            update_personal_dot_draft(&pool, id, "alice", before, "{}")
                .await
                .unwrap(),
            PersonalDotDraftSaved::Conflict
        );
    }

    #[tokio::test]
    async fn dots_enforce_active_capacity_and_can_be_reactivated() {
        let pool = pool().await;
        for (id, email) in [("alice", "alice@example.com"), ("bob", "bob@example.com")] {
            let now = Timestamp::now();
            users::upsert(
                &pool,
                &User {
                    id: id.into(),
                    email: email.into(),
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
        }
        let first = create_personal_dot(
            &pool,
            &NewPrincipal {
                name: "harald",
                display: "Harald",
                description: "",
            },
            r#"{"main":{}}"#,
            "alice",
            1,
        )
        .await
        .unwrap();
        let (DotCreate::Created, Some(first)) = first else {
            panic!("first Dot should be created")
        };
        assert_eq!(
            create_personal_dot(
                &pool,
                &NewPrincipal {
                    name: "research",
                    display: "Research",
                    description: ""
                },
                r#"{"main":{}}"#,
                "alice",
                1,
            )
            .await
            .unwrap()
            .0,
            DotCreate::ActiveLimit
        );
        assert!(
            set_personal_dot_active(&pool, &first.principal.id, "alice", false, 1)
                .await
                .unwrap()
        );
        let (DotCreate::Created, Some(second)) = create_personal_dot(
            &pool,
            &NewPrincipal {
                name: "research",
                display: "Research",
                description: "",
            },
            r#"{"main":{}}"#,
            "alice",
            1,
        )
        .await
        .unwrap() else {
            panic!("deactivation should free the slot")
        };
        assert!(
            !set_personal_dot_active(&pool, &first.principal.id, "alice", true, 1)
                .await
                .unwrap()
        );
        assert_eq!(list_personal_dots(&pool, "alice").await.unwrap().len(), 2);
        assert!(list_personal_dots(&pool, "bob").await.unwrap().is_empty());
        assert_eq!(
            create_personal_dot(
                &pool,
                &NewPrincipal {
                    name: "harald",
                    display: "Harald",
                    description: "",
                },
                r#"{"main":{}}"#,
                "bob",
                1,
            )
            .await
            .unwrap()
            .0,
            DotCreate::Created
        );
        assert_eq!(
            create_personal_dot(
                &pool,
                &NewPrincipal {
                    name: "harald",
                    display: "Another Harald",
                    description: "",
                },
                r#"{"main":{}}"#,
                "alice",
                2,
            )
            .await
            .unwrap()
            .0,
            DotCreate::NameTaken
        );
        assert!(
            set_personal_dot_active(&pool, &second.principal.id, "alice", false, 1)
                .await
                .unwrap()
        );
        assert!(
            set_personal_dot_active(&pool, &first.principal.id, "alice", true, 1)
                .await
                .unwrap()
        );
    }

    fn audit_kinds(events: &[agent_audit::AuditEvent]) -> Vec<&str> {
        events.iter().rev().map(|e| e.kind.as_str()).collect()
    }

    #[test]
    fn access_and_subject_kinds_round_trip_through_their_column_values() {
        for a in Access::ALL {
            assert_eq!(Access::parse(a.as_str()), Some(a));
        }
        for k in [SubjectKind::User, SubjectKind::Group] {
            assert_eq!(SubjectKind::parse(k.as_str()), Some(k));
        }
        assert_eq!(Access::parse("admin"), None);
        assert_eq!(SubjectKind::parse("role"), None);
        assert!(Access::Write > Access::Read && Access::Read > Access::Respond);
        assert!(!Access::Respond.needs_agent_manager());
        assert!(Access::Read.needs_agent_manager() && Access::Write.needs_agent_manager());
    }

    #[tokio::test]
    async fn a_respond_share_is_held_but_lists_no_agent() {
        let pool = pool().await;
        let a = agent(&pool, "support", "alice").await;
        let id = &a.principal.id;
        assert_eq!(
            set_share(
                &pool,
                id,
                SubjectKind::Group,
                "desk",
                Access::Respond,
                "alice"
            )
            .await
            .unwrap(),
            ShareChange::Changed
        );
        let desk = vec!["desk".to_string()];
        assert_eq!(
            shares_held(&pool, id, "sam", &desk).await.unwrap(),
            BTreeSet::from([Access::Respond])
        );
        assert!(
            list_shared_with(&pool, "sam", &desk)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            audit_kinds(&agent_audit::for_principal(&pool, id).await.unwrap()).last(),
            Some(&"agent_share_set")
        );
    }

    #[tokio::test]
    async fn creating_an_agent_creates_its_principal_and_a_write_share_for_the_creator() {
        let pool = pool().await;
        let a = agent(&pool, "support", "alice").await;
        assert_eq!(a.principal.name, "support");
        assert_eq!(a.live_version, None);
        assert_eq!(a.draft_spec, r#"{"main":{}}"#);
        assert!(
            sp::get(&pool, &a.principal.id).await.unwrap().is_some(),
            "the principal exists"
        );
        assert!(sp::grants(&pool, &a.principal.id).await.unwrap().is_empty());
        let created = shares(&pool, &a.principal.id).await.unwrap();
        assert_eq!(
            created,
            [ShareRow {
                subject_kind: SubjectKind::User,
                subject_id: "alice".into(),
                access: Access::Write,
                granted_by: Some("alice".into()),
                granted_at: created[0].granted_at,
            }]
        );
        assert!(
            created[0].granted_at.is_some(),
            "the creator's share is dated"
        );
        let audit = agent_audit::for_principal(&pool, &a.principal.id)
            .await
            .unwrap();
        assert_eq!(audit_kinds(&audit), ["principal_created", "agent_created"]);
    }

    #[tokio::test]
    async fn a_taken_name_creates_nothing() {
        let pool = pool().await;
        agent(&pool, "support", "alice").await;
        let second = create(
            &pool,
            &NewPrincipal {
                name: "support",
                display: "x",
                description: "",
            },
            "{}",
            "bob",
        )
        .await
        .unwrap();
        assert!(second.is_none());
        assert_eq!(publication_status(&pool).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn editing_the_draft_never_changes_the_live_version() {
        let pool = pool().await;
        let a = agent(&pool, "support", "alice").await;
        let id = &a.principal.id;
        assert_eq!(
            publish(&pool, id, "{\"v\":1}", "alice").await.unwrap(),
            Some(1)
        );
        assert!(
            update_draft(&pool, id, "{\"v\":2}", "alice", DraftChange::default())
                .await
                .unwrap()
                .is_some()
        );

        let now = get(&pool, id).await.unwrap().unwrap();
        assert_eq!(now.draft_spec, "{\"v\":2}");
        assert_eq!(now.live_version, Some(1));
        assert_eq!(
            version(&pool, id, 1).await.unwrap().unwrap().spec,
            "{\"v\":1}"
        );
    }

    #[tokio::test]
    async fn the_live_spec_follows_the_live_pointer_and_lists_only_published_agents() {
        let pool = pool().await;
        let a = agent(&pool, "support", "alice").await;
        let draft_only = agent(&pool, "billing", "alice").await;
        let id = &a.principal.id;
        assert_eq!(live(&pool, id).await.unwrap(), None);
        assert_eq!(live_version(&pool, id).await.unwrap(), None);
        publish(&pool, id, "one", "alice").await.unwrap();
        publish(&pool, id, "two", "alice").await.unwrap();
        assert_eq!(live(&pool, id).await.unwrap(), Some((2, "two".to_string())));
        assert_eq!(live_version(&pool, id).await.unwrap(), Some(2));
        set_live(&pool, id, 1, "alice").await.unwrap();
        assert_eq!(live(&pool, id).await.unwrap(), Some((1, "one".to_string())));
        assert_eq!(live_version(&pool, id).await.unwrap(), Some(1));

        let specs = live_specs(&pool).await.unwrap();
        assert_eq!(specs.get(id.as_str()).map(String::as_str), Some("one"));
        assert!(!specs.contains_key(&draft_only.principal.id));
        assert_eq!(live(&pool, "nope").await.unwrap(), None);
        assert_eq!(live_version(&pool, "nope").await.unwrap(), None);
    }

    #[tokio::test]
    async fn publishing_numbers_versions_and_rollback_moves_only_the_live_pointer() {
        let pool = pool().await;
        let a = agent(&pool, "support", "alice").await;
        let id = &a.principal.id;
        assert_eq!(publish(&pool, id, "one", "alice").await.unwrap(), Some(1));
        assert_eq!(publish(&pool, id, "two", "bob").await.unwrap(), Some(2));
        assert_eq!(get(&pool, id).await.unwrap().unwrap().live_version, Some(2));

        assert!(set_live(&pool, id, 1, "alice").await.unwrap());
        assert_eq!(get(&pool, id).await.unwrap().unwrap().live_version, Some(1));
        assert!(!set_live(&pool, id, 7, "alice").await.unwrap());
        assert_eq!(get(&pool, id).await.unwrap().unwrap().live_version, Some(1));

        let all = versions(&pool, id).await.unwrap();
        assert_eq!(
            all.iter()
                .map(|v| (v.version, v.spec.as_str(), v.published_by.as_str()))
                .collect::<Vec<_>>(),
            [(2, "two", "bob"), (1, "one", "alice")]
        );
        assert_eq!(publish(&pool, "nope", "x", "alice").await.unwrap(), None);
        assert!(publication_status(&pool).await.unwrap()[id.as_str()]);

        let audit = agent_audit::for_principal(&pool, id).await.unwrap();
        assert_eq!(
            audit_kinds(&audit),
            [
                "principal_created",
                "agent_created",
                "agent_published",
                "agent_published",
                "agent_live_version_set"
            ]
        );
        assert_eq!(audit[0].detail["version"], 1);
    }

    #[tokio::test]
    async fn every_share_level_is_held_directly_or_through_a_group() {
        let pool = pool().await;
        let a = agent(&pool, "support", "alice").await;
        let id = &a.principal.id;
        let groups = |g: &[&str]| g.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            shares_held(&pool, id, "alice", &[])
                .await
                .unwrap()
                .last()
                .copied(),
            Some(Access::Write)
        );
        assert_eq!(
            shares_held(&pool, id, "bob", &[])
                .await
                .unwrap()
                .last()
                .copied(),
            None
        );

        set_share(
            &pool,
            id,
            SubjectKind::Group,
            "support-team",
            Access::Read,
            "alice",
        )
        .await
        .unwrap();
        set_share(&pool, id, SubjectKind::User, "bob", Access::Write, "alice")
            .await
            .unwrap();
        assert_eq!(
            shares_held(&pool, id, "carol", &groups(&["support-team"]))
                .await
                .unwrap(),
            BTreeSet::from([Access::Read])
        );
        assert_eq!(
            shares_held(&pool, id, "bob", &groups(&["support-team"]))
                .await
                .unwrap(),
            BTreeSet::from([Access::Read, Access::Write])
        );

        let listed = list_shared_with(&pool, "carol", &groups(&["support-team", "x"]))
            .await
            .unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].1, Access::Read);
        assert!(
            list_shared_with(&pool, "dave", &[])
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(list_all(&pool).await.unwrap().len(), 1);
        let bob = list_shared_with(&pool, "bob", &groups(&["support-team"]))
            .await
            .unwrap();
        assert_eq!(bob[0].1, Access::Write);
    }

    #[tokio::test]
    async fn the_last_write_share_cannot_be_removed_or_downgraded() {
        let pool = pool().await;
        let a = agent(&pool, "support", "alice").await;
        let id = &a.principal.id;
        assert_eq!(
            remove_share(&pool, id, SubjectKind::User, "alice", "alice")
                .await
                .unwrap(),
            ShareChange::LastWriter
        );
        assert_eq!(
            set_share(&pool, id, SubjectKind::User, "alice", Access::Read, "alice")
                .await
                .unwrap(),
            ShareChange::LastWriter
        );
        assert_eq!(
            set_share(&pool, id, SubjectKind::User, "bob", Access::Write, "alice")
                .await
                .unwrap(),
            ShareChange::Changed
        );
        assert_eq!(
            set_share(&pool, id, SubjectKind::User, "bob", Access::Write, "alice")
                .await
                .unwrap(),
            ShareChange::Unchanged
        );
        assert_eq!(
            remove_share(&pool, id, SubjectKind::User, "alice", "bob")
                .await
                .unwrap(),
            ShareChange::Changed
        );
        assert_eq!(
            remove_share(&pool, id, SubjectKind::User, "alice", "bob")
                .await
                .unwrap(),
            ShareChange::NotFound
        );
        let audit = agent_audit::for_principal(&pool, id).await.unwrap();
        assert_eq!(audit[0].kind, "agent_share_removed");
        assert_eq!(audit[0].actor_id.as_deref(), Some("bob"));
        assert_eq!(audit[1].kind, "agent_share_set");
        assert_eq!(audit[1].detail["access"], "write");
    }

    #[tokio::test]
    async fn deleting_an_agent_removes_its_principal_but_keeps_the_audit_trail() {
        let pool = pool().await;
        let a = agent(&pool, "support", "alice").await;
        let id = &a.principal.id;
        sp::add_grant(&pool, id, GrantKind::Model, "chat", "alice")
            .await
            .unwrap();
        publish(&pool, id, "{}", "alice").await.unwrap();

        assert!(delete(&pool, id, "alice").await.unwrap());
        assert!(!delete(&pool, id, "alice").await.unwrap());
        assert!(get(&pool, id).await.unwrap().is_none());
        assert!(sp::get(&pool, id).await.unwrap().is_none());
        assert!(sp::grants(&pool, id).await.unwrap().is_empty());
        assert!(versions(&pool, id).await.unwrap().is_empty());
        assert!(shares(&pool, id).await.unwrap().is_empty());
        let audit = agent_audit::for_principal(&pool, id).await.unwrap();
        assert_eq!(audit[0].kind, "agent_deleted");
        assert_eq!(audit[0].detail["name"], "support");
    }

    #[tokio::test]
    async fn a_plain_system_principal_is_not_an_agent() {
        let pool = pool().await;
        let p = sp::create(
            &pool,
            &NewPrincipal {
                name: "ci",
                display: "CI",
                description: "",
            },
            "admin",
        )
        .await
        .unwrap()
        .unwrap();
        assert!(get(&pool, &p.id).await.unwrap().is_none());
        assert!(!delete(&pool, &p.id, "admin").await.unwrap());
        assert!(sp::get(&pool, &p.id).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn the_schema_refuses_unknown_share_kinds_and_access() {
        let pool = pool().await;
        let a = agent(&pool, "support", "alice").await;
        for (kind, access) in [("role", "read"), ("user", "admin")] {
            let res = sqlx::query(
                "INSERT INTO agent_shares (principal_id, subject_kind, subject_id, access)
                 VALUES (?, ?, 'x', ?)",
            )
            .bind(&a.principal.id)
            .bind(kind)
            .bind(access)
            .execute(&pool)
            .await;
            assert!(res.is_err(), "{kind}/{access} accepted");
        }
    }

    #[tokio::test]
    async fn a_saved_draft_keeps_the_one_it_replaced_for_undo() {
        let pool = pool().await;
        let a = agent(&pool, "undo", "alice").await;
        let id = &a.principal.id;

        let first = update_draft(&pool, id, r#"{"v":1}"#, "alice", DraftChange::default())
            .await
            .unwrap()
            .unwrap();
        let kept = draft_revision(&pool, id, first.revision.unwrap())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(kept.spec, r#"{"main":{}}"#);
        assert_eq!(kept.saved_by, "alice");
        assert!(kept.granted.is_empty());

        let granted = [(GrantKind::Tool, "fetch_url".to_string())];
        let tooled = update_draft(
            &pool,
            id,
            r#"{"v":"tooled"}"#,
            "alice",
            DraftChange {
                granted: &granted,
                ..DraftChange::default()
            },
        )
        .await
        .unwrap()
        .unwrap();
        let kept = draft_revision(&pool, id, tooled.revision.unwrap())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            kept.granted, granted,
            "the change's grants ride with its revision"
        );

        let same = update_draft(
            &pool,
            id,
            r#"{"v":"tooled"}"#,
            "alice",
            DraftChange::default(),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(same.revision, None, "an unchanged draft adds no revision");

        let mut last = None;
        for n in 2..(MAX_DRAFT_REVISIONS + 5) {
            last = update_draft(
                &pool,
                id,
                &format!(r#"{{"v":{n}}}"#),
                "bob",
                DraftChange::default(),
            )
            .await
            .unwrap()
            .unwrap()
            .revision;
        }
        let kept: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM agent_draft_revisions WHERE principal_id = ?")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(kept, MAX_DRAFT_REVISIONS);
        assert!(
            draft_revision(&pool, id, first.revision.unwrap())
                .await
                .unwrap()
                .is_none(),
            "the oldest revisions are dropped"
        );
        assert!(
            draft_revision(&pool, id, last.unwrap())
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            draft_revision(&pool, "other", last.unwrap())
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            update_draft(&pool, "missing", "{}", "alice", DraftChange::default())
                .await
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn atomic_agent_writers_share_outer_transaction_and_preserve_last_writer() {
        let pool = pool().await;
        let original = agent(&pool, "atomic-agent", "owner").await;
        let id = &original.principal.id;
        let baseline: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM agent_audit WHERE principal_id=?")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
        let mut tx = WriteTx::begin(&pool).await.unwrap();
        assert_eq!(get_in(&mut tx, id).await.unwrap(), Some(original.clone()));
        assert_eq!(shares_in(&mut tx, id).await.unwrap().len(), 1);
        let spec = r#"{"main":{"instructions":"Updated"}}"#;
        let saved = update_draft_in(&mut tx, id, spec, "owner", DraftChange::default())
            .await
            .unwrap()
            .unwrap();
        assert!(saved.revision.is_some());
        assert!(
            update_draft_in(&mut tx, id, spec, "owner", DraftChange::default())
                .await
                .unwrap()
                .unwrap()
                .revision
                .is_none()
        );
        let version = publish_in(&mut tx, id, spec, "owner")
            .await
            .unwrap()
            .unwrap();
        assert!(set_live_in(&mut tx, id, version, "owner").await.unwrap());
        assert!(
            !set_live_in(&mut tx, id, version + 1, "owner")
                .await
                .unwrap()
        );
        assert_eq!(
            remove_share_in(&mut tx, id, SubjectKind::User, "owner", "owner")
                .await
                .unwrap(),
            ShareChange::LastWriter
        );
        assert_eq!(
            set_share_in(
                &mut tx,
                id,
                SubjectKind::User,
                "second",
                Access::Write,
                "owner"
            )
            .await
            .unwrap(),
            ShareChange::Changed
        );
        assert_eq!(
            set_share_in(
                &mut tx,
                id,
                SubjectKind::User,
                "second",
                Access::Write,
                "owner"
            )
            .await
            .unwrap(),
            ShareChange::Unchanged
        );
        assert_eq!(
            remove_share_in(&mut tx, id, SubjectKind::User, "owner", "owner")
                .await
                .unwrap(),
            ShareChange::Changed
        );
        assert_eq!(get_in(&mut tx, id).await.unwrap().unwrap().draft_spec, spec);
        drop(tx);
        assert_eq!(get(&pool, id).await.unwrap(), Some(original.clone()));
        assert_eq!(shares(&pool, id).await.unwrap().len(), 1);
        assert!(versions(&pool, id).await.unwrap().is_empty());
        let after: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM agent_audit WHERE principal_id=?")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(after, baseline);
    }

    #[tokio::test]
    async fn atomic_agent_delete_rolls_back_cascade_and_audit_with_outer_transaction() {
        let pool = pool().await;
        let original = agent(&pool, "atomic-delete", "owner").await;
        let id = &original.principal.id;
        let mut tx = WriteTx::begin(&pool).await.unwrap();
        assert!(delete_in(&mut tx, id, "owner").await.unwrap());
        assert!(get_in(&mut tx, id).await.unwrap().is_none());
        assert!(!delete_in(&mut tx, id, "owner").await.unwrap());
        drop(tx);
        assert_eq!(get(&pool, id).await.unwrap(), Some(original.clone()));
        let deletions: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM agent_audit WHERE principal_id=? AND kind='agent_deleted'",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(deletions, 0);
        let mut tx = WriteTx::begin(&pool).await.unwrap();
        assert!(delete_in(&mut tx, id, "owner").await.unwrap());
        tx.commit().await.unwrap();
        assert!(get(&pool, id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn private_published_version_reader_uses_current_transaction_and_existing_pointer() {
        let pool = pool().await;
        let a = agent(&pool, "private-published-reader", "alice").await;
        let id = &a.principal.id;
        assert_eq!(
            publish(&pool, id, "first-authored-spec", "alice")
                .await
                .unwrap(),
            Some(1)
        );
        let first = version(&pool, id, 1).await.unwrap().unwrap();
        let mut tx = WriteTx::begin(&pool).await.unwrap();
        assert_eq!(
            publish_in(&mut tx, id, "second-authored-spec", "alice")
                .await
                .unwrap(),
            Some(2)
        );
        assert_eq!(
            get_in(&mut tx, id).await.unwrap().unwrap().live_version,
            Some(2)
        );
        assert_eq!(
            version_in(&mut tx, id, 1).await.unwrap(),
            Some(first.clone())
        );
        let second = version_in(&mut tx, id, 2).await.unwrap().unwrap();
        assert_eq!(second.version, 2);
        assert_eq!(second.spec, "second-authored-spec");
        assert_eq!(second.published_by, "alice");
        assert!(
            version_in(&mut tx, "foreign-principal", 2)
                .await
                .unwrap()
                .is_none()
        );
        for missing in [-1, 0, 3] {
            assert!(version_in(&mut tx, id, missing).await.unwrap().is_none());
        }
        assert!(set_live_in(&mut tx, id, 1, "alice").await.unwrap());
        assert_eq!(
            get_in(&mut tx, id).await.unwrap().unwrap().live_version,
            Some(1)
        );
        assert_eq!(version_in(&mut tx, id, 2).await.unwrap(), Some(second));
        drop(tx);
        assert_eq!(get(&pool, id).await.unwrap().unwrap().live_version, Some(1));
        assert_eq!(version(&pool, id, 1).await.unwrap(), Some(first));
        assert!(version(&pool, id, 2).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn versions_in_reads_same_transaction_pins_order_and_rolls_back() {
        let pool = pool().await;
        let first = agent(&pool, "versions-in-first", "alice").await;
        let other = agent(&pool, "versions-in-other", "alice").await;
        publish(
            &pool,
            &first.principal.id,
            r#"{"main":{"model":"first"}}"#,
            "alice",
        )
        .await
        .unwrap();
        publish(
            &pool,
            &other.principal.id,
            r#"{"main":{"model":"other"}}"#,
            "alice",
        )
        .await
        .unwrap();
        let original = versions(&pool, &first.principal.id).await.unwrap();
        let mut tx = WriteTx::begin(&pool).await.unwrap();
        assert_eq!(
            publish_in(
                &mut tx,
                &first.principal.id,
                r#"{"main":{"model":"second"}}"#,
                "bob"
            )
            .await
            .unwrap(),
            Some(2)
        );
        let observed = versions_in(&mut tx, &first.principal.id).await.unwrap();
        assert_eq!(
            observed.iter().map(|row| row.version).collect::<Vec<_>>(),
            vec![2, 1]
        );
        assert_eq!(observed[0].published_by, "bob");
        assert_eq!(observed[0].spec, r#"{"main":{"model":"second"}}"#);
        assert_eq!(observed[1], original[0]);
        assert!(versions_in(&mut tx, "missing").await.unwrap().is_empty());
        let other_rows = versions_in(&mut tx, &other.principal.id).await.unwrap();
        assert_eq!(other_rows.len(), 1);
        assert_eq!(other_rows[0].spec, r#"{"main":{"model":"other"}}"#);
        drop(tx);
        assert_eq!(
            versions(&pool, &first.principal.id).await.unwrap(),
            original
        );
    }

    #[tokio::test]
    async fn organization_dot_listing_is_membership_scoped_and_active_only() {
        use crate::db::{WriteTx, organizations, organizations::NewOrganization};
        let pool = pool().await;
        let now = Timestamp::now();
        users::upsert(
            &pool,
            &User {
                id: "alice".into(),
                email: "alice@example.com".into(),
                name: None,
                roles: vec!["engineering".into()],
                created_at: now,
                updated_at: now,
                timezone: None,
                speech_voice: None,
            },
        )
        .await
        .unwrap();
        users::upsert(
            &pool,
            &User {
                id: "carol".into(),
                email: "carol@example.com".into(),
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
        sqlx::query(
            "INSERT INTO gateway_groups(name,created_at,updated_at) VALUES('engineering', ?, ?)",
        )
        .bind(now.to_string())
        .bind(now.to_string())
        .execute(&pool)
        .await
        .unwrap();
        let org = match organizations::create(
            &pool,
            &NewOrganization {
                name: "Acme",
                description: "",
                member_groups: &["engineering".into()],
            },
            "alice",
        )
        .await
        .unwrap()
        {
            organizations::OrganizationCreateResult::Created(row) => row,
            _ => panic!("organization created"),
        };
        let mut tx = WriteTx::begin(&pool).await.unwrap();
        let (_, dot) = create_organization_dot_in(
            &mut tx,
            &org.organization.id,
            &NewPrincipal {
                name: "ticket",
                display: "Ticket",
                description: "",
            },
            r#"{"main":{}}"#,
            "alice",
            4,
        )
        .await
        .unwrap();
        let dot = dot.unwrap();
        let dot_id = dot.principal.id;
        tx.commit().await.unwrap();

        assert!(
            list_organization_dots_for_member(&pool, &[])
                .await
                .unwrap()
                .is_empty()
        );
        let listed = list_organization_dots_for_member(&pool, &["engineering".to_owned()])
            .await
            .unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].agent.principal.id, dot_id);
        assert_eq!(listed[0].organization_id, org.organization.id);
        assert_eq!(listed[0].name, "ticket");
        assert!(listed[0].active);

        create_personal_dot(
            &pool,
            &NewPrincipal {
                name: "personal",
                display: "Personal",
                description: "",
            },
            r#"{"main":{}}"#,
            "alice",
            4,
        )
        .await
        .unwrap();
        assert_eq!(
            list_organization_dots_for_member(&pool, &["engineering".to_owned()])
                .await
                .unwrap()
                .len(),
            1
        );

        let owner = DotOwner::Organization(org.organization.id.clone());
        let mut tx = WriteTx::begin(&pool).await.unwrap();
        assert!(
            set_dot_active_in(&mut tx, &dot_id, &owner, false, 4, "alice")
                .await
                .unwrap()
        );
        tx.commit().await.unwrap();
        assert!(
            list_organization_dots_for_member(&pool, &["engineering".to_owned()])
                .await
                .unwrap()
                .is_empty()
        );
    }
}
