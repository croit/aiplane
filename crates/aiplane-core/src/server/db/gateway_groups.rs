// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Gateway groups — the DB-backed RBAC backbone (schema:
//! `migrations/0044_gateway_groups.sql`).
//!
//! A gateway group is the IdP-independent access unit. It is the same thing as
//! an internal "role id": resources reference groups by name in their
//! `allowed_groups`, and [`crate::server::rbac::Resolver::role_ids_for`] maps a
//! user's raw OIDC claim values onto the groups they hold.
//!
//! The DB is the runtime source of truth, edited in the `/admin/groups` UI.
//! The resolver holds a snapshot ([`load_snapshot`])
//! that is rebuilt from these tables at startup and after every admin edit.

use std::collections::BTreeMap;

use jiff::Timestamp;
use sqlx::Row;

use super::{DbError, Pool, WriteTx};

const GROUPS_SQL: &str = "SELECT name, description, is_admin, is_default, can_manage_agents, can_manage_dots, priority FROM gateway_groups ORDER BY name";
const MAPPINGS_SQL: &str = "SELECT oidc_value, gateway_group FROM oidc_group_mappings";
const TOOL_GRANTS_SQL: &str = "SELECT gateway_group, tool_id FROM group_tool_grants";

/// A gateway group definition (without its grants — those live in the
/// `group_tool_grants` / `skill_role_grants` / per-resource tables).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupRow {
    pub priority: Option<crate::server::priority::Priority>,
    pub name: String,
    pub description: String,
    pub is_admin: bool,
    pub is_default: bool,
    /// May create and configure system principals (`docs/agents.md` → "Principals").
    /// `is_admin` implies it; see `Resolver::can_manage_agents`.
    pub can_manage_agents: bool,
    pub can_manage_dots: bool,
}

/// Everything the [`crate::server::rbac::Resolver`] needs to answer access
/// questions, read in one shot so a reload is a single atomic swap.
#[derive(Debug, Clone, Default)]
pub struct GroupSnapshot {
    pub groups: Vec<GroupRow>,
    /// `(oidc_value, gateway_group)` mapping rows.
    pub mappings: Vec<(String, String)>,
    /// `(gateway_group, tool_id)` grant rows (`tool_id` may be `*`).
    pub tool_grants: Vec<(String, String)>,
}

fn map_group(row: &sqlx::sqlite::SqliteRow) -> Result<GroupRow, DbError> {
    Ok(GroupRow {
        priority: decode_priority(row.try_get("priority")?)?,
        name: row.try_get("name")?,
        description: row.try_get("description")?,
        is_admin: row.try_get::<i64, _>("is_admin")? != 0,
        is_default: row.try_get::<i64, _>("is_default")? != 0,
        can_manage_agents: row.try_get::<i64, _>("can_manage_agents")? != 0,
        can_manage_dots: row.try_get::<i64, _>("can_manage_dots")? != 0,
    })
}

/// Every group, name-ordered. Powers the `/admin/groups` roster and the
/// group-picker `<datalist>` reused on every resource form.
pub async fn list_groups(pool: &Pool) -> Result<Vec<GroupRow>, DbError> {
    let rows = sqlx::query(GROUPS_SQL).fetch_all(pool).await?;
    rows.iter().map(map_group).collect()
}

/// Just the group names, for the admin pickers, the admin roster's role list and
/// for validating a resource's `allowed_groups` before it is stored. Every one of
/// those used to re-derive the names from [`list_groups`] and throw the rest of
/// each row away.
pub async fn list_group_names(pool: &Pool) -> Result<Vec<String>, DbError> {
    list_group_names_with(pool).await
}

pub async fn list_group_names_in(tx: &mut super::WriteTx) -> Result<Vec<String>, DbError> {
    list_group_names_with(&mut **tx).await
}

async fn list_group_names_with<'e, E>(executor: E) -> Result<Vec<String>, DbError>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let rows = sqlx::query("SELECT name FROM gateway_groups ORDER BY name")
        .fetch_all(executor)
        .await?;
    rows.iter().map(|row| Ok(row.try_get("name")?)).collect()
}

/// The entries of `wanted` that name no existing group, in the order given.
///
/// Resource ACLs (`pools`, `rag_collections`, `mcp_catalog_connectors`) store
/// group names as opaque strings, so a name with a typo is stored happily and
/// then matches nobody — the resource goes invisible and unroutable with no
/// clue as to why. Callers reject on a non-empty result rather than filtering,
/// which would silently discard a grant the operator asked for.
async fn unknown_groups(pool: &Pool, wanted: &[String]) -> Result<Vec<String>, DbError> {
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    let known = list_group_names(pool).await?;
    Ok(wanted
        .iter()
        .filter(|name| !known.contains(name))
        .cloned()
        .collect())
}

/// Like [`unknown_groups`], but only for names this save *introduces*.
///
/// Deleting a group deliberately leaves its name behind in resource ACLs
/// ([`delete_group`]) — dropping it would turn a resource restricted to only
/// that group into an unrestricted one, so the dangling name is the fail-closed
/// choice. The editors resend the whole list on every save, so validating all of
/// it would make an inherited dangling name block every later edit of an
/// unrelated field. An edit may therefore keep a dangling name, but may not
/// introduce one.
async fn unknown_added_groups(
    pool: &Pool,
    wanted: &[String],
    previously: &[String],
) -> Result<Vec<String>, DbError> {
    let added: Vec<String> = wanted
        .iter()
        .filter(|name| !previously.contains(name))
        .cloned()
        .collect();
    unknown_groups(pool, &added).await
}

/// `Some(message)` when this save introduces group names that do not exist —
/// the 400 body for the pool, RAG collection and MCP connector saves, so an
/// operator gets the same wording and the same next step wherever they hit it.
/// `None` means the ACL is fine to store. The `DbError` stays separate so a
/// lookup failure is still a 500 rather than being reported as a bad request.
pub async fn unknown_added_groups_message(
    pool: &Pool,
    lang: session_core::i18n::Lang,
    wanted: &[String],
    previously: &[String],
) -> Result<Option<String>, DbError> {
    let unknown = unknown_added_groups(pool, wanted, previously).await?;
    Ok((!unknown.is_empty()).then(|| unknown_groups_message(lang, &unknown)))
}

/// The 400 body for a resource save that named groups which do not exist.
fn unknown_groups_message(lang: session_core::i18n::Lang, unknown: &[String]) -> String {
    session_core::i18n::t_args(
        lang,
        "admin-error-unknown-groups",
        &session_core::i18n::args([
            ("count", (unknown.len() as i64).into()),
            ("groups", unknown.join(", ").into()),
        ]),
    )
}

/// Insert a group or update its mutable fields (description, flags). `name` is
/// the stable key; renaming is a delete + create (resource ACLs reference the
/// name, so we don't cascade-rename).
pub async fn upsert_group(
    pool: &Pool,
    name: &str,
    description: &str,
    is_admin: bool,
    is_default: bool,
) -> Result<(), DbError> {
    let now = Timestamp::now().to_string();
    sqlx::query(
        r#"INSERT INTO gateway_groups (name, description, is_admin, is_default, created_at, updated_at)
           VALUES (?, ?, ?, ?, ?, ?)
           ON CONFLICT(name) DO UPDATE SET
               description = excluded.description,
               is_admin    = excluded.is_admin,
               is_default  = excluded.is_default,
               updated_at  = excluded.updated_at"#,
    )
    .bind(name)
    .bind(description)
    .bind(i64::from(is_admin))
    .bind(i64::from(is_default))
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(())
}

/// Set a group's agent-management capability. Separate from
/// [`upsert_group`] so a save that does not mention the flag leaves it as it
/// was. `Ok(false)` when there is no such group.
pub async fn set_can_manage_agents(
    pool: &Pool,
    name: &str,
    enabled: bool,
) -> Result<bool, DbError> {
    let changed = sqlx::query(
        "UPDATE gateway_groups SET can_manage_agents = ?, updated_at = ? WHERE name = ?",
    )
    .bind(i64::from(enabled))
    .bind(Timestamp::now().to_string())
    .bind(name)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(changed > 0)
}

pub async fn set_can_manage_dots(pool: &Pool, name: &str, enabled: bool) -> Result<bool, DbError> {
    let changed =
        sqlx::query("UPDATE gateway_groups SET can_manage_dots = ?, updated_at = ? WHERE name = ?")
            .bind(i64::from(enabled))
            .bind(Timestamp::now().to_string())
            .bind(name)
            .execute(pool)
            .await?
            .rows_affected();
    Ok(changed > 0)
}

/// Delete a group. `ON DELETE CASCADE` drops its mappings + tool grants; the
/// resource `allowed_groups` (plain JSON string lists, not FKs) are left with a
/// now-dangling name, which the resolver treats as "no user holds it" — i.e. a
/// deleted group silently stops granting access, which is the safe direction.
/// `false` when there was no such group.
pub async fn delete_group(pool: &Pool, name: &str) -> Result<bool, DbError> {
    let deleted = sqlx::query("DELETE FROM gateway_groups WHERE name = ?")
        .bind(name)
        .execute(pool)
        .await?;
    Ok(deleted.rows_affected() > 0)
}

// ---- OIDC → group mappings ------------------------------------------------

/// Every `(oidc_value, gateway_group)` mapping row.
pub async fn all_mappings(pool: &Pool) -> Result<Vec<(String, String)>, DbError> {
    sqlx::query(MAPPINGS_SQL)
        .fetch_all(pool)
        .await?
        .iter()
        .map(map_mapping)
        .collect()
}

fn map_mapping(row: &sqlx::sqlite::SqliteRow) -> Result<(String, String), DbError> {
    Ok((row.try_get("oidc_value")?, row.try_get("gateway_group")?))
}

/// The OIDC claim values mapped to `group`, for the group's edit form.
pub async fn mapped_values_for_group(pool: &Pool, group: &str) -> Result<Vec<String>, DbError> {
    let rows = sqlx::query(
        "SELECT oidc_value FROM oidc_group_mappings WHERE gateway_group = ? ORDER BY oidc_value",
    )
    .bind(group)
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|r| r.try_get::<String, _>("oidc_value").map_err(DbError::from))
        .collect()
}

/// Replace the full set of OIDC claim values mapped to `group` (deduplicated).
/// Transactional so a concurrent reader never sees a half-applied edit.
pub async fn set_mappings_for_group(
    pool: &Pool,
    group: &str,
    oidc_values: &[String],
) -> Result<(), DbError> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM oidc_group_mappings WHERE gateway_group = ?")
        .bind(group)
        .execute(&mut *tx)
        .await?;
    let mut seen: Vec<&str> = Vec::new();
    for value in oidc_values {
        let value = value.trim();
        if value.is_empty() || seen.contains(&value) {
            continue;
        }
        seen.push(value);
        sqlx::query(
            "INSERT INTO oidc_group_mappings (oidc_claim, oidc_value, gateway_group) VALUES ('groups', ?, ?)",
        )
        .bind(value)
        .bind(group)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

// ---- Tool grants ----------------------------------------------------------

/// Every `(gateway_group, tool_id)` grant row.
pub async fn all_tool_grants(pool: &Pool) -> Result<Vec<(String, String)>, DbError> {
    sqlx::query(TOOL_GRANTS_SQL)
        .fetch_all(pool)
        .await?
        .iter()
        .map(map_tool_grant)
        .collect()
}

fn map_tool_grant(row: &sqlx::sqlite::SqliteRow) -> Result<(String, String), DbError> {
    Ok((row.try_get("gateway_group")?, row.try_get("tool_id")?))
}

/// The tool ids granted to `group` (may include `*`).
pub async fn tools_for_group(pool: &Pool, group: &str) -> Result<Vec<String>, DbError> {
    let rows = sqlx::query(
        "SELECT tool_id FROM group_tool_grants WHERE gateway_group = ? ORDER BY tool_id",
    )
    .bind(group)
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|r| r.try_get::<String, _>("tool_id").map_err(DbError::from))
        .collect()
}

/// Replace the full set of tool grants for `group` (deduplicated).
pub async fn set_tools_for_group(
    pool: &Pool,
    group: &str,
    tool_ids: &[String],
) -> Result<(), DbError> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM group_tool_grants WHERE gateway_group = ?")
        .bind(group)
        .execute(&mut *tx)
        .await?;
    let mut seen: Vec<&str> = Vec::new();
    for tool in tool_ids {
        let tool = tool.trim();
        if tool.is_empty() || seen.contains(&tool) {
            continue;
        }
        seen.push(tool);
        sqlx::query("INSERT INTO group_tool_grants (gateway_group, tool_id) VALUES (?, ?)")
            .bind(group)
            .bind(tool)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

// ---- Snapshot + seeding ---------------------------------------------------

/// Load everything the resolver needs in one shot.
pub async fn load_snapshot(pool: &Pool) -> Result<GroupSnapshot, DbError> {
    Ok(GroupSnapshot {
        groups: list_groups(pool).await?,
        mappings: all_mappings(pool).await?,
        tool_grants: all_tool_grants(pool).await?,
    })
}

pub async fn load_snapshot_in(tx: &mut WriteTx) -> Result<GroupSnapshot, DbError> {
    let groups = sqlx::query(GROUPS_SQL)
        .fetch_all(&mut **tx)
        .await?
        .iter()
        .map(map_group)
        .collect::<Result<_, _>>()?;
    let mappings = sqlx::query(MAPPINGS_SQL)
        .fetch_all(&mut **tx)
        .await?
        .iter()
        .map(map_mapping)
        .collect::<Result<_, _>>()?;
    let tool_grants = sqlx::query(TOOL_GRANTS_SQL)
        .fetch_all(&mut **tx)
        .await?
        .iter()
        .map(map_tool_grant)
        .collect::<Result<_, _>>()?;
    Ok(GroupSnapshot {
        groups,
        mappings,
        tool_grants,
    })
}

/// Distinct OIDC claim values seen across all users' stored `roles`, for the
/// mapping form's autocomplete `<datalist>`. Best-effort: a decode hiccup on
/// one row is skipped rather than failing the page.
pub async fn observed_oidc_values(pool: &Pool) -> Result<Vec<String>, DbError> {
    let rows = sqlx::query("SELECT roles_json FROM users")
        .fetch_all(pool)
        .await?;
    let mut out: Vec<String> = Vec::new();
    for row in &rows {
        let json: String = row.try_get("roles_json")?;
        if let Ok(values) = serde_json::from_str::<Vec<String>>(&json) {
            for v in values {
                if !out.contains(&v) {
                    out.push(v);
                }
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Count locally known users that resolve to each requested group. Membership
/// follows the same stored claim mappings and default-group rule as RBAC.
pub async fn group_member_counts_in(
    tx: &mut WriteTx,
    groups: &[String],
) -> Result<BTreeMap<String, u64>, DbError> {
    if groups.is_empty() {
        return Ok(BTreeMap::new());
    }
    let groups = serde_json::to_string(groups).expect("string vector serializes");
    let rows = sqlx::query(
        r#"SELECT requested.value AS group_name, COUNT(DISTINCT u.id) AS user_count
           FROM json_each(?) AS requested
           LEFT JOIN gateway_groups AS g ON g.name = requested.value
           LEFT JOIN users AS u ON g.is_default = 1 OR EXISTS (
               SELECT 1
               FROM oidc_group_mappings AS mapping
               JOIN json_each(u.roles_json) AS role_value
                 ON role_value.value = mapping.oidc_value
               WHERE mapping.gateway_group = g.name
           )
           GROUP BY requested.value"#,
    )
    .bind(groups)
    .fetch_all(&mut **tx)
    .await?;

    rows.iter()
        .map(|row| {
            let group: String = row.try_get("group_name")?;
            let count: i64 = row.try_get("user_count")?;
            let count = u64::try_from(count).map_err(|source| DbError::Decode {
                column: "user_count",
                source: source.into(),
            })?;
            Ok((group, count))
        })
        .collect()
}

pub async fn set_priority(
    pool: &Pool,
    name: &str,
    priority: Option<crate::server::priority::Priority>,
) -> Result<(), DbError> {
    sqlx::query("UPDATE gateway_groups SET priority = ? WHERE name = ?")
        .bind(priority.map(|p| p.as_str()))
        .bind(name)
        .execute(pool)
        .await?;
    Ok(())
}

pub(super) fn decode_priority(
    value: Option<String>,
) -> Result<Option<crate::server::priority::Priority>, DbError> {
    value
        .map(|value| {
            serde_json::from_value(serde_json::Value::String(value)).map_err(|source| {
                DbError::Decode {
                    column: "priority",
                    source: source.into(),
                }
            })
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::db::open;
    use std::path::Path;

    async fn fresh() -> Pool {
        open(Path::new(":memory:")).await.unwrap()
    }

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    #[tokio::test]
    async fn transactional_snapshot_reads_uncommitted_current_policy_without_pool_fallback() {
        let pool = fresh().await;
        upsert_group(&pool, "members", "Members", false, true)
            .await
            .unwrap();
        set_can_manage_agents(&pool, "members", true).await.unwrap();
        set_mappings_for_group(&pool, "members", &v(&["member-claim"]))
            .await
            .unwrap();
        set_tools_for_group(&pool, "members", &v(&["company_echo"]))
            .await
            .unwrap();
        let original = load_snapshot(&pool).await.unwrap();
        let mut tx = super::super::WriteTx::begin(&pool).await.unwrap();
        let initial = load_snapshot_in(&mut tx).await.unwrap();
        assert_eq!(initial.groups, original.groups);
        assert_eq!(initial.mappings, original.mappings);
        assert_eq!(initial.tool_grants, original.tool_grants);
        sqlx::query(
            "UPDATE gateway_groups SET can_manage_agents=0,is_default=0 WHERE name='members'",
        )
        .execute(&mut *tx)
        .await
        .unwrap();
        sqlx::query("DELETE FROM oidc_group_mappings WHERE gateway_group='members'")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("DELETE FROM group_tool_grants WHERE gateway_group='members'")
            .execute(&mut *tx)
            .await
            .unwrap();
        let revoked = load_snapshot_in(&mut tx).await.unwrap();
        assert!(!revoked.groups[0].can_manage_agents);
        assert!(!revoked.groups[0].is_default);
        assert!(revoked.mappings.is_empty());
        assert!(revoked.tool_grants.is_empty());
        drop(tx);
        assert_eq!(load_snapshot(&pool).await.unwrap().groups, original.groups);
    }

    #[tokio::test]
    async fn groups_round_trip() {
        let pool = fresh().await;
        assert!(list_groups(&pool).await.unwrap().is_empty());
        upsert_group(&pool, "developers", "Dev team", false, false)
            .await
            .unwrap();
        upsert_group(&pool, "admins", "", true, false)
            .await
            .unwrap();
        assert!(!list_groups(&pool).await.unwrap().is_empty());
        let groups = list_groups(&pool).await.unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].name, "admins");
        assert!(groups[0].is_admin);
        assert_eq!(groups[1].name, "developers");
        assert_eq!(groups[1].description, "Dev team");
    }

    #[tokio::test]
    async fn member_counts_include_mapped_users_once_and_every_user_in_default_groups() {
        let pool = fresh().await;
        upsert_group(&pool, "admins", "", true, false)
            .await
            .unwrap();
        upsert_group(&pool, "everyone", "", false, true)
            .await
            .unwrap();
        upsert_group(&pool, "support", "", false, false)
            .await
            .unwrap();
        set_mappings_for_group(&pool, "admins", &v(&["admin-a", "admin-b"]))
            .await
            .unwrap();
        set_mappings_for_group(&pool, "support", &v(&["support-team"]))
            .await
            .unwrap();
        for (id, roles) in [
            ("one", r#"["admin-a","admin-b","support-team"]"#),
            ("two", r#"["admin-a"]"#),
            ("three", r#"["unmapped"]"#),
        ] {
            sqlx::query("INSERT INTO users(id,email,roles_json,created_at,updated_at) VALUES(?,?,?,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')")
                .bind(id)
                .bind(format!("{id}@example.test"))
                .bind(roles)
                .execute(&pool)
                .await
                .unwrap();
        }

        let mut tx = super::super::WriteTx::begin(&pool).await.unwrap();
        let counts =
            group_member_counts_in(&mut tx, &v(&["admins", "everyone", "support", "missing"]))
                .await
                .unwrap();

        assert_eq!(
            counts["admins"], 2,
            "one user matching two claims counts once"
        );
        assert_eq!(
            counts["everyone"], 3,
            "default groups include all known users"
        );
        assert_eq!(counts["support"], 1);
        assert_eq!(counts["missing"], 0);
    }

    #[tokio::test]
    async fn agent_management_is_off_by_default_and_survives_a_plain_upsert() {
        let pool = fresh().await;
        upsert_group(&pool, "agents", "", false, false)
            .await
            .unwrap();
        assert!(!list_groups(&pool).await.unwrap()[0].can_manage_agents);

        assert!(set_can_manage_agents(&pool, "agents", true).await.unwrap());
        upsert_group(&pool, "agents", "renamed", false, false)
            .await
            .unwrap();
        let group = &list_groups(&pool).await.unwrap()[0];
        assert!(group.can_manage_agents);
        assert_eq!(group.description, "renamed");

        assert!(!set_can_manage_agents(&pool, "ghost", true).await.unwrap());
    }

    #[tokio::test]
    async fn dot_management_defaults_false_and_is_independent_in_list_and_snapshot() {
        let pool = fresh().await;
        upsert_group(&pool, "managers", "", false, false)
            .await
            .unwrap();
        let group = &list_groups(&pool).await.unwrap()[0];
        assert!(!group.can_manage_dots);
        assert!(!group.can_manage_agents);
        set_can_manage_agents(&pool, "managers", true)
            .await
            .unwrap();
        assert!(!list_groups(&pool).await.unwrap()[0].can_manage_dots);
        assert!(set_can_manage_dots(&pool, "managers", true).await.unwrap());
        set_can_manage_agents(&pool, "managers", false)
            .await
            .unwrap();
        upsert_group(&pool, "managers", "edited", false, false)
            .await
            .unwrap();
        let group = &list_groups(&pool).await.unwrap()[0];
        assert!(group.can_manage_dots);
        assert!(!group.can_manage_agents);
        assert_eq!(group.description, "edited");
        assert!(load_snapshot(&pool).await.unwrap().groups[0].can_manage_dots);
        assert!(set_can_manage_dots(&pool, "managers", false).await.unwrap());
        assert!(!load_snapshot(&pool).await.unwrap().groups[0].can_manage_dots);
        assert!(!set_can_manage_dots(&pool, "missing", true).await.unwrap());
    }

    #[tokio::test]
    async fn transactional_group_names_observe_actual_uncommitted_rows_and_rollback() {
        let pool = fresh().await;
        upsert_group(&pool, "original", "", false, false)
            .await
            .unwrap();
        let mut tx = super::super::WriteTx::begin(&pool).await.unwrap();
        sqlx::query("INSERT INTO gateway_groups(name,created_at,updated_at) VALUES('added','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')")
            .execute(&mut *tx).await.unwrap();
        assert_eq!(
            list_group_names_in(&mut tx).await.unwrap(),
            v(&["added", "original"])
        );
        sqlx::query("DELETE FROM gateway_groups WHERE name='original'")
            .execute(&mut *tx)
            .await
            .unwrap();
        assert_eq!(list_group_names_in(&mut tx).await.unwrap(), v(&["added"]));
        drop(tx);
        assert_eq!(list_group_names(&pool).await.unwrap(), v(&["original"]));
    }

    #[tokio::test]
    async fn group_names_lists_every_group_sorted() {
        let pool = fresh().await;
        upsert_group(&pool, "developers", "", false, false)
            .await
            .unwrap();
        upsert_group(&pool, "admins", "", true, false)
            .await
            .unwrap();
        assert_eq!(
            list_group_names(&pool).await.unwrap(),
            v(&["admins", "developers"])
        );
    }

    #[tokio::test]
    async fn unknown_groups_names_only_what_is_missing() {
        let pool = fresh().await;
        upsert_group(&pool, "developers", "", false, false)
            .await
            .unwrap();
        assert_eq!(
            unknown_groups(&pool, &v(&["developers", "devlopers"]))
                .await
                .unwrap(),
            v(&["devlopers"])
        );
        assert!(
            unknown_groups(&pool, &v(&["developers"]))
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn unknown_groups_accepts_an_empty_list_without_a_query() {
        let pool = fresh().await;
        assert!(unknown_groups(&pool, &[]).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn unknown_added_groups_lets_an_inherited_dangling_name_through() {
        let pool = fresh().await;
        upsert_group(&pool, "developers", "", false, false)
            .await
            .unwrap();
        // `contractors` was deleted; the pool still lists it. Re-saving the pool
        // to change some other field must not be blocked by that.
        assert!(
            unknown_added_groups(
                &pool,
                &v(&["developers", "contractors"]),
                &v(&["developers", "contractors"])
            )
            .await
            .unwrap()
            .is_empty()
        );
    }

    #[tokio::test]
    async fn unknown_added_groups_still_refuses_a_newly_typed_name() {
        let pool = fresh().await;
        upsert_group(&pool, "developers", "", false, false)
            .await
            .unwrap();
        assert_eq!(
            unknown_added_groups(&pool, &v(&["developers", "devlopers"]), &v(&["developers"]))
                .await
                .unwrap(),
            v(&["devlopers"])
        );
    }

    #[tokio::test]
    async fn mappings_and_tools_round_trip() {
        let pool = fresh().await;
        upsert_group(&pool, "developers", "", false, false)
            .await
            .unwrap();
        set_mappings_for_group(
            &pool,
            "developers",
            &v(&["grp-dev-emea", "grp-dev-us", " "]),
        )
        .await
        .unwrap();
        let mut vals = mapped_values_for_group(&pool, "developers").await.unwrap();
        vals.sort();
        assert_eq!(vals, v(&["grp-dev-emea", "grp-dev-us"]));

        set_tools_for_group(
            &pool,
            "developers",
            &v(&["rag_search", "rag_search", "search_web"]),
        )
        .await
        .unwrap();
        let tools = tools_for_group(&pool, "developers").await.unwrap();
        assert_eq!(tools, v(&["rag_search", "search_web"]));
    }

    #[tokio::test]
    async fn delete_group_cascades_mappings_and_tools() {
        let pool = fresh().await;
        upsert_group(&pool, "developers", "", false, false)
            .await
            .unwrap();
        set_mappings_for_group(&pool, "developers", &v(&["grp-dev"]))
            .await
            .unwrap();
        set_tools_for_group(&pool, "developers", &v(&["rag_search"]))
            .await
            .unwrap();
        delete_group(&pool, "developers").await.unwrap();
        assert!(all_mappings(&pool).await.unwrap().is_empty());
        assert!(all_tool_grants(&pool).await.unwrap().is_empty());
    }
}
