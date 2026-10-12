// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Who may act on an agent (`docs/agents.md` → "Agent definition"): the one rule behind the
//! `/api/v0/agents` routes, the inbox's standing and the `a2a_caller` grant.
//!
//! An admin holds `write` on every agent without a share, so an agent whose
//! last writer left can always be recovered. Anyone else needs a share. A
//! `read` or `write` share takes effect only for a holder of the
//! agent-management permission, so a manager who loses it loses every agent
//! with it, whatever shares are left behind. A `respond` share needs no such
//! permission: it answers the agent's pending inbox items and shows nothing
//! else, and it is the only level that survives without the permission.

use aiplane_agents::db::agents::{self as agents_db, Access};
use aiplane_core::server::db::DbError;

use crate::server::AppState;

/// The access a person holds on agent `agent_id` today: `write`
/// for an admin, otherwise their strongest share that takes effect for them.
pub async fn effective_access(
    state: &AppState,
    agent_id: &str,
    user_id: &str,
) -> Result<Option<Access>, DbError> {
    let mut tx = aiplane_core::server::db::WriteTx::begin(&state.db).await?;
    let access = effective_access_in(state, &mut tx, agent_id, user_id).await?;
    tx.commit().await?;
    Ok(access)
}

pub async fn current_user_policy_in(
    state: &AppState,
    tx: &mut aiplane_core::server::db::WriteTx,
    user_id: &str,
) -> Result<
    Option<(
        aiplane_core::server::db::users::User,
        aiplane_core::server::rbac::Resolver,
    )>,
    DbError,
> {
    let Some(user) = aiplane_core::server::db::users::find_by_id_in(tx, user_id).await? else {
        return Ok(None);
    };
    let snapshot = aiplane_core::server::db::gateway_groups::load_snapshot_in(tx).await?;
    Ok(Some((user, state.rbac.with_snapshot(snapshot))))
}

pub async fn effective_access_in(
    state: &AppState,
    tx: &mut aiplane_core::server::db::WriteTx,
    agent_id: &str,
    user_id: &str,
) -> Result<Option<Access>, DbError> {
    access_in(state, tx, agent_id, user_id, false).await
}

pub async fn organization_access_in(
    state: &AppState,
    tx: &mut aiplane_core::server::db::WriteTx,
    agent_id: &str,
    user_id: &str,
) -> Result<Option<Access>, DbError> {
    access_in(state, tx, agent_id, user_id, true).await
}

/// The access a person holds on an organization agent today: `write` for an
/// admin, otherwise their strongest share that takes effect for them.
/// `None` for personal agents and for non-members.
pub async fn organization_access(
    state: &AppState,
    agent_id: &str,
    user_id: &str,
) -> Result<Option<Access>, DbError> {
    let mut tx = aiplane_core::server::db::WriteTx::begin(&state.db).await?;
    let access = organization_access_in(state, &mut tx, agent_id, user_id).await?;
    tx.commit().await?;
    Ok(access)
}

/// The role IDs a person resolves to under the current RBAC snapshot —
/// organization membership and the cross-boundary live recheck compare
/// against these, like every other organization membership check. `None`
/// when the user no longer exists.
pub async fn role_ids_for_user(state: &AppState, user_id: &str) -> Option<Vec<String>> {
    let user = aiplane_core::server::db::users::find_by_id(&state.db, user_id)
        .await
        .ok()
        .flatten()?;
    Some(state.role_ids_for(&user.roles))
}

async fn access_in(
    state: &AppState,
    tx: &mut aiplane_core::server::db::WriteTx,
    agent_id: &str,
    user_id: &str,
    organization_only: bool,
) -> Result<Option<Access>, DbError> {
    let Some((user, policy)) = current_user_policy_in(state, tx, user_id).await? else {
        return Ok(None);
    };
    let groups = policy.role_ids_for(&user.roles);
    if policy.is_admin(&groups) && !organization_only {
        return Ok(Some(Access::Write));
    }
    match agents_db::dot_owner_in(tx, agent_id).await? {
        Some(agents_db::DotOwner::Organization(organization)) => {
            if !aiplane_agents::db::organizations::is_member_in(tx, &organization, &groups).await? {
                return Ok(None);
            }
        }
        _ if organization_only => return Ok(None),
        _ => {}
    }
    if policy.is_admin(&groups) {
        return Ok(Some(Access::Write));
    }
    let manager = policy.can_manage_agents(&groups);
    Ok(agents_db::shares_held_in(tx, agent_id, user_id, &groups)
        .await?
        .into_iter()
        .filter(|access| manager || !access.needs_agent_manager())
        .max())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::rama_server::state::RamaState;
    use aiplane_agents::db::{organizations, system_principals::NewPrincipal};
    use aiplane_core::server::{
        db::{self, WriteTx, gateway_groups, users},
        rbac::Resolver,
        upstreams::UpstreamRegistry,
    };
    use std::sync::Arc;

    #[tokio::test]
    async fn organization_access_requires_current_membership_before_admin_recovery() {
        let (state, organization, dot) = fixture().await;
        let mut tx = WriteTx::begin(&state.db).await.unwrap();
        assert_eq!(
            effective_access_in(&state, &mut tx, &dot, "admin")
                .await
                .unwrap(),
            Some(Access::Write)
        );
        assert_eq!(
            organization_access_in(&state, &mut tx, &dot, "admin")
                .await
                .unwrap(),
            None
        );
        assert_eq!(
            organization_access_in(&state, &mut tx, &dot, "creator")
                .await
                .unwrap(),
            Some(Access::Write)
        );
        assert_eq!(
            organization_access_in(&state, &mut tx, &dot, "responder")
                .await
                .unwrap(),
            Some(Access::Respond)
        );
        assert_eq!(
            organization_access_in(&state, &mut tx, &dot, "outsider")
                .await
                .unwrap(),
            None
        );
        assert_eq!(
            organization_access_in(&state, &mut tx, &dot, "absent")
                .await
                .unwrap(),
            None
        );
        sqlx::query(
            "DELETE FROM organization_groups WHERE organization_id=? AND gateway_group='members'",
        )
        .bind(&organization)
        .execute(&mut *tx)
        .await
        .unwrap();
        assert_eq!(
            organization_access_in(&state, &mut tx, &dot, "creator")
                .await
                .unwrap(),
            None
        );
        assert_eq!(
            organization_access_in(&state, &mut tx, &dot, "responder")
                .await
                .unwrap(),
            Some(Access::Respond)
        );
        drop(tx);
        let mut tx = WriteTx::begin(&state.db).await.unwrap();
        assert_eq!(
            organization_access_in(&state, &mut tx, &dot, "creator")
                .await
                .unwrap(),
            Some(Access::Write)
        );
        tx.commit().await.unwrap();
    }

    pub(crate) async fn fixture() -> (RamaState, String, String) {
        let pool = db::open(std::path::Path::new(":memory:")).await.unwrap();
        for (group, manager) in [
            ("members", true),
            ("responders", false),
            ("outsiders", true),
        ] {
            gateway_groups::upsert_group(&pool, group, "", false, false)
                .await
                .unwrap();
            gateway_groups::set_can_manage_agents(&pool, group, manager)
                .await
                .unwrap();
            gateway_groups::set_mappings_for_group(&pool, group, &[format!("claim-{group}")])
                .await
                .unwrap();
        }
        let now = jiff::Timestamp::now();
        for (id, role) in [
            ("creator", "claim-members"),
            ("responder", "claim-responders"),
            ("outsider", "claim-outsiders"),
            ("admin", "break-glass"),
        ] {
            users::upsert(
                &pool,
                &users::User {
                    id: id.into(),
                    email: format!("{id}@example.com"),
                    name: None,
                    roles: vec![role.into()],
                    created_at: now,
                    updated_at: now,
                    timezone: None,
                    speech_voice: None,
                },
            )
            .await
            .unwrap();
        }
        let organizations::OrganizationCreateResult::Created(organization) = organizations::create(
            &pool,
            &organizations::NewOrganization {
                name: "Actual organization",
                description: "Actual description",
                member_groups: &["members".into(), "responders".into()],
            },
            "admin",
        )
        .await
        .unwrap() else {
            panic!("organization expected");
        };
        let mut tx = WriteTx::begin(&pool).await.unwrap();
        let (_, dot) = agents_db::create_organization_dot_in(
            &mut tx,
            &organization.organization.id,
            &NewPrincipal {
                name: "Organization assistant",
                display: "Organization assistant",
                description: "",
            },
            "{}",
            "creator",
            4,
        )
        .await
        .unwrap();
        let dot = dot.unwrap().principal.id;
        tx.commit().await.unwrap();
        for (user, access) in [("responder", Access::Respond), ("outsider", Access::Write)] {
            agents_db::set_share(
                &pool,
                &dot,
                agents_db::SubjectKind::User,
                user,
                access,
                "creator",
            )
            .await
            .unwrap();
        }
        let policy = Resolver::from_snapshot(
            gateway_groups::load_snapshot(&pool).await.unwrap(),
            vec!["break-glass".into()],
        );
        let app = crate::server::AppState::new(
            Default::default(),
            pool.clone(),
            UpstreamRegistry::new(&Default::default()).unwrap(),
            Arc::new(crate::server::tools::ToolRegistry::new()),
            Arc::new(policy),
        );
        let state = RamaState::new(
            app,
            aiplane_core::rama_server::SessionStore::new(pool, [9u8; 32]),
            aiplane_core::server::usage::UsageHandle::disabled(),
        );
        (state, organization.organization.id, dot)
    }

    #[tokio::test]
    async fn organization_access_requires_current_membership_and_share_with_admin_recovery() {
        let (state, organization, dot) = fixture().await;
        assert_eq!(
            effective_access(&state, &dot, "creator").await.unwrap(),
            Some(Access::Write)
        );
        assert_eq!(
            effective_access(&state, &dot, "responder").await.unwrap(),
            Some(Access::Respond)
        );
        assert_eq!(
            effective_access(&state, &dot, "outsider").await.unwrap(),
            None
        );
        assert_eq!(
            effective_access(&state, &dot, "admin").await.unwrap(),
            Some(Access::Write)
        );
        assert_eq!(
            effective_access(&state, &dot, "missing-user")
                .await
                .unwrap(),
            None
        );
        sqlx::query("DELETE FROM organization_groups WHERE organization_id=? AND gateway_group='responders'").bind(&organization).execute(&state.db).await.unwrap();
        assert_eq!(
            effective_access(&state, &dot, "responder").await.unwrap(),
            None
        );
        assert_eq!(
            effective_access(&state, &dot, "creator").await.unwrap(),
            Some(Access::Write)
        );
        sqlx::query("DELETE FROM organization_groups WHERE organization_id=?")
            .bind(&organization)
            .execute(&state.db)
            .await
            .unwrap();
        assert_eq!(
            effective_access(&state, &dot, "creator").await.unwrap(),
            None
        );
        assert_eq!(
            effective_access(&state, &dot, "admin").await.unwrap(),
            Some(Access::Write)
        );
    }

    #[tokio::test]
    async fn organization_access_uses_transaction_policy_without_global_reload_or_stale_user_roles()
    {
        let (state, _, dot) = fixture().await;
        let cached = state.role_ids_for(&["claim-members".into()]);
        assert!(state.rbac.can_manage_agents(&cached));
        let mut tx = WriteTx::begin(&state.db).await.unwrap();
        sqlx::query("UPDATE gateway_groups SET can_manage_agents=0 WHERE name='members'")
            .execute(&mut *tx)
            .await
            .unwrap();
        assert_eq!(
            effective_access_in(&state, &mut tx, &dot, "creator")
                .await
                .unwrap(),
            None
        );
        sqlx::query("UPDATE gateway_groups SET can_manage_agents=1 WHERE name='members'")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("DELETE FROM oidc_group_mappings WHERE gateway_group='members'")
            .execute(&mut *tx)
            .await
            .unwrap();
        assert_eq!(
            effective_access_in(&state, &mut tx, &dot, "creator")
                .await
                .unwrap(),
            None
        );
        sqlx::query("INSERT INTO oidc_group_mappings(oidc_value,gateway_group) VALUES('claim-members','members')").execute(&mut *tx).await.unwrap();
        sqlx::query("UPDATE users SET roles_json='[]' WHERE id='creator'")
            .execute(&mut *tx)
            .await
            .unwrap();
        assert_eq!(
            effective_access_in(&state, &mut tx, &dot, "creator")
                .await
                .unwrap(),
            None
        );
        assert_eq!(
            effective_access_in(&state, &mut tx, &dot, "responder")
                .await
                .unwrap(),
            Some(Access::Respond)
        );
        assert_eq!(
            effective_access_in(&state, &mut tx, &dot, "admin")
                .await
                .unwrap(),
            Some(Access::Write)
        );
        drop(tx);
        assert!(state.rbac.can_manage_agents(&cached));
        assert_eq!(
            effective_access(&state, &dot, "creator").await.unwrap(),
            Some(Access::Write)
        );
    }

    #[tokio::test]
    async fn organization_profile_refuses_live_and_pinned_execution_before_loading_a_model() {
        let (state, _, dot) = fixture().await;
        let state = Arc::new(state);
        for source in [
            crate::agents::profile::SpecSource::Live,
            crate::agents::profile::SpecSource::Pinned(1),
        ] {
            let result = crate::agents::profile::RunProfile::load_from(
                &state,
                &dot,
                source,
                crate::agents::profile::Role::Main,
                &Default::default(),
            )
            .await;
            assert!(matches!(
                result,
                Err(crate::agents::profile::AgentRunError::OrganizationExecutionUnavailable(_))
            ));
        }
    }
}
