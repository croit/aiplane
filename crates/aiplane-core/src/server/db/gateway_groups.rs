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
//! The DB is the runtime source of truth. [`seed_roles`]
//! imports the legacy `[rbac]` + `[[roles]]` config once; after that the
//! `/admin/groups` UI owns it. The resolver holds a snapshot ([`load_snapshot`])
//! that is rebuilt from these tables at startup and after every admin edit.

use jiff::Timestamp;
use sqlx::Row;

use super::{DbError, Pool};

/// A gateway group definition (without its grants — those live in the
/// `group_tool_grants` / `skill_role_grants` / per-resource tables).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupRow {
    pub name: String,
    pub description: String,
    pub is_admin: bool,
    pub is_default: bool,
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
        name: row.try_get("name")?,
        description: row.try_get("description")?,
        is_admin: row.try_get::<i64, _>("is_admin")? != 0,
        is_default: row.try_get::<i64, _>("is_default")? != 0,
    })
}

/// Every group, name-ordered. Powers the `/admin/groups` roster and the
/// group-picker `<datalist>` reused on every resource form.
pub async fn list_groups(pool: &Pool) -> Result<Vec<GroupRow>, DbError> {
    let rows = sqlx::query(
        "SELECT name, description, is_admin, is_default FROM gateway_groups ORDER BY name",
    )
    .fetch_all(pool)
    .await?;
    rows.iter().map(map_group).collect()
}

/// Just the group names, for the admin pickers, the admin roster's role list and
/// for validating a resource's `allowed_groups` before it is stored. Every one of
/// those used to re-derive the names from [`list_groups`] and throw the rest of
/// each row away.
pub async fn list_group_names(pool: &Pool) -> Result<Vec<String>, DbError> {
    let rows = sqlx::query("SELECT name FROM gateway_groups ORDER BY name")
        .fetch_all(pool)
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

/// Delete a group. `ON DELETE CASCADE` drops its mappings + tool grants; the
/// resource `allowed_groups` (plain JSON string lists, not FKs) are left with a
/// now-dangling name, which the resolver treats as "no user holds it" — i.e. a
/// deleted group silently stops granting access, which is the safe direction.
pub async fn delete_group(pool: &Pool, name: &str) -> Result<(), DbError> {
    sqlx::query("DELETE FROM gateway_groups WHERE name = ?")
        .bind(name)
        .execute(pool)
        .await?;
    Ok(())
}

// ---- OIDC → group mappings ------------------------------------------------

/// Every `(oidc_value, gateway_group)` mapping row.
pub async fn all_mappings(pool: &Pool) -> Result<Vec<(String, String)>, DbError> {
    let rows = sqlx::query("SELECT oidc_value, gateway_group FROM oidc_group_mappings")
        .fetch_all(pool)
        .await?;
    rows.iter()
        .map(|r| {
            Ok((
                r.try_get::<String, _>("oidc_value")?,
                r.try_get::<String, _>("gateway_group")?,
            ))
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .map_err(DbError::from)
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
    let rows = sqlx::query("SELECT gateway_group, tool_id FROM group_tool_grants")
        .fetch_all(pool)
        .await?;
    rows.iter()
        .map(|r| {
            Ok((
                r.try_get::<String, _>("gateway_group")?,
                r.try_get::<String, _>("tool_id")?,
            ))
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .map_err(DbError::from)
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

/// True when no group has been created yet — the trigger for a one-time
/// [`seed_roles`].
pub async fn is_empty(pool: &Pool) -> Result<bool, DbError> {
    let n: i64 = sqlx::query("SELECT COUNT(*) AS n FROM gateway_groups")
        .fetch_one(pool)
        .await?
        .try_get("n")?;
    Ok(n == 0)
}

/// Write a role set into the group tables, once, when they are empty.
///
/// The config file this was named for is gone; what remains is a way to
/// populate an empty deployment from an in-memory role set, which is what the
/// `dev_ui` fixture needs. Roles become gateway groups, mappings become OIDC
/// mappings, `default_role` becomes the default group, and each role's `tools`
/// / `skills` become grant rows. `models` is intentionally dropped: model
/// access is governed per-pool (see `allowed_groups` on pools), not per-role.
pub async fn seed_roles(
    pool: &Pool,
    rbac: &crate::server::rbac::config::RbacConfig,
    roles: &[crate::server::rbac::config::RoleConfig],
) -> Result<(), DbError> {
    if !is_empty(pool).await? {
        return Ok(());
    }
    if roles.is_empty() && rbac.default_role.is_none() && rbac.mappings.is_empty() {
        // Nothing to seed — leave the tables empty (a fresh deployment manages
        // everything in the UI from scratch).
        return Ok(());
    }
    for role in roles {
        let is_default = rbac.default_role.as_deref() == Some(role.id.as_str());
        upsert_group(pool, &role.id, "", role.admin, is_default).await?;
        if !role.tools.is_empty() {
            set_tools_for_group(pool, &role.id, &role.tools).await?;
        }
        // Skills fold into the existing `skill_role_grants` overlay, keyed by
        // group name (== role id) — the same table the `/admin/skills` editor
        // and the resolver already use.
        if !role.skills.is_empty() {
            super::skill_grants::add_grants_for_role(pool, &role.id, &role.skills).await?;
        }
    }
    // A default_role that isn't itself a `[[roles]]` entry still needs a group.
    if let Some(default) = rbac.default_role.as_deref()
        && !roles.iter().any(|r| r.id == default)
    {
        upsert_group(pool, default, "", false, true).await?;
    }
    for m in &rbac.mappings {
        // The mapping's target role is now a group; ensure it exists even if it
        // had no `[[roles]]` entry, so the FK holds.
        if !roles.iter().any(|r| r.id == m.role) && rbac.default_role.as_deref() != Some(&m.role) {
            upsert_group(pool, &m.role, "", false, false).await?;
        }
        sqlx::query(
            "INSERT OR IGNORE INTO oidc_group_mappings (oidc_claim, oidc_value, gateway_group) VALUES (?, ?, ?)",
        )
        .bind(&m.oidc_claim)
        .bind(&m.oidc_value)
        .bind(&m.role)
        .execute(pool)
        .await?;
    }
    Ok(())
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
    async fn groups_round_trip() {
        let pool = fresh().await;
        assert!(is_empty(&pool).await.unwrap());
        upsert_group(&pool, "developers", "Dev team", false, false)
            .await
            .unwrap();
        upsert_group(&pool, "admins", "", true, false)
            .await
            .unwrap();
        assert!(!is_empty(&pool).await.unwrap());
        let groups = list_groups(&pool).await.unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].name, "admins");
        assert!(groups[0].is_admin);
        assert_eq!(groups[1].name, "developers");
        assert_eq!(groups[1].description, "Dev team");
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

    #[tokio::test]
    async fn seed_roles_imports_roles_mappings_and_default() {
        use crate::server::rbac::config::{RbacConfig, RoleConfig, RoleMapping};
        let pool = fresh().await;
        let rbac = RbacConfig {
            default_role: Some("user".into()),
            mappings: vec![RoleMapping {
                oidc_claim: "groups".into(),
                oidc_value: "grp-net".into(),
                role: "network_admin".into(),
            }],
        };
        let roles = vec![
            RoleConfig {
                id: "user".into(),
                admin: false,
                models: vec!["*".into()],
                tools: vec!["search_web".into()],
                skills: vec![],
            },
            RoleConfig {
                id: "network_admin".into(),
                admin: true,
                models: vec![],
                tools: vec!["*".into()],
                skills: vec![],
            },
        ];
        seed_roles(&pool, &rbac, &roles).await.unwrap();

        let groups = list_groups(&pool).await.unwrap();
        assert_eq!(groups.len(), 2);
        let user = groups.iter().find(|g| g.name == "user").unwrap();
        assert!(user.is_default);
        let na = groups.iter().find(|g| g.name == "network_admin").unwrap();
        assert!(na.is_admin);
        assert_eq!(
            tools_for_group(&pool, "user").await.unwrap(),
            v(&["search_web"])
        );
        assert_eq!(
            mapped_values_for_group(&pool, "network_admin")
                .await
                .unwrap(),
            v(&["grp-net"])
        );

        // Idempotent: a second seed with different config is a no-op (tables
        // are no longer empty).
        seed_roles(&pool, &RbacConfig::default(), &[])
            .await
            .unwrap();
        assert_eq!(list_groups(&pool).await.unwrap().len(), 2);
    }
}
