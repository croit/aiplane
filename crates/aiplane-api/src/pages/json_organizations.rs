// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use aiplane_agents::db::{agents, organizations, system_principals::NewPrincipal};
use aiplane_core::server::db::{WriteTx, gateway_groups, users};
use aiplane_core::server::rbac::Resolver;
use aiplane_runtime::agents::access::effective_access_in;
use aiplane_runtime::rama_server::state::RamaState;
use rama::http::service::web::extract::State;
use rama::http::{Request, Response, StatusCode};
use serde::{Deserialize, Serialize};
use session_core::i18n::{Lang, t};

use super::{internal, json_error, json_ok, raw_path_segment};

#[derive(Serialize, schemars::JsonSchema)]
pub struct OrganizationView {
    pub id: String,
    pub name: String,
    pub description: String,
    pub revision: String,
}

impl From<&organizations::OrganizationRow> for OrganizationView {
    fn from(row: &organizations::OrganizationRow) -> Self {
        Self {
            id: row.id.clone(),
            name: row.name.clone(),
            description: row.description.clone(),
            revision: row.revision.clone(),
        }
    }
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct AdminOrganizationView {
    #[serde(flatten)]
    pub organization: OrganizationView,
    pub member_groups: Vec<String>,
}

impl From<&organizations::AdminOrganizationRow> for AdminOrganizationView {
    fn from(row: &organizations::AdminOrganizationRow) -> Self {
        Self {
            organization: OrganizationView::from(&row.organization),
            member_groups: row.member_groups.clone(),
        }
    }
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct AdminOrganizationList {
    pub organizations: Vec<AdminOrganizationView>,
    pub groups: Vec<String>,
    pub member_group_user_counts: BTreeMap<String, u64>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct AdminOrganizationReply {
    pub organization: AdminOrganizationView,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct OrganizationListRow {
    #[serde(flatten)]
    pub organization: OrganizationView,
    pub can_create_dot: bool,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct OrganizationList {
    pub organizations: Vec<OrganizationListRow>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct OrganizationDetail {
    pub organization: OrganizationView,
    pub dots: Vec<super::json_agents::AgentView>,
    pub active_count: u32,
    pub active_limit: u32,
    pub can_create_dot: bool,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct OrganizationDotReply {
    pub agent: super::json_agents::AgentView,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OrganizationBody {
    pub name: String,
    pub description: String,
    pub member_groups: Vec<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OrganizationEditBody {
    pub name: String,
    pub description: String,
    pub member_groups: Vec<String>,
    pub expected_revision: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OrganizationDotBody {
    pub name: String,
    pub display: Option<String>,
    #[serde(default)]
    pub description: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OrganizationDotActiveBody {
    pub active: bool,
}

fn error(lang: Lang, status: StatusCode, kind: &str, key: &str) -> Response {
    json_error(status, kind, &t(lang, key))
}

fn invalid(lang: Lang) -> Response {
    error(
        lang,
        StatusCode::BAD_REQUEST,
        "invalid_organization",
        "organization-error-invalid",
    )
}

fn missing(lang: Lang) -> Response {
    error(
        lang,
        StatusCode::NOT_FOUND,
        "not_found",
        "organization-error-not-found",
    )
}

fn valid_name(name: &str) -> bool {
    !name.trim().is_empty()
        && name.trim().chars().count() <= 80
        && !name.chars().any(char::is_control)
}

pub(super) async fn current_policy_in(
    state: &RamaState,
    tx: &mut WriteTx,
    user_id: &str,
    lang: Lang,
) -> Result<(users::User, Resolver), Response> {
    aiplane_runtime::agents::access::current_user_policy_in(state, tx, user_id)
        .await
        .map_err(internal)?
        .ok_or_else(|| missing(lang))
}

fn require_admin(policy: &Resolver, user: &users::User, lang: Lang) -> Result<(), Response> {
    if policy.is_admin(&policy.role_ids_for(&user.roles)) {
        Ok(())
    } else {
        Err(error(
            lang,
            StatusCode::FORBIDDEN,
            "forbidden",
            "organization-error-admin-required",
        ))
    }
}

async fn member_in(
    state: &RamaState,
    tx: &mut WriteTx,
    user_id: &str,
    organization_id: &str,
    lang: Lang,
) -> Result<(organizations::AdminOrganizationRow, users::User, Resolver), Response> {
    let (user, policy) = current_policy_in(state, tx, user_id, lang).await?;
    let org = organizations::get_admin_in(tx, organization_id)
        .await
        .map_err(internal)?
        .ok_or_else(|| missing(lang))?;
    if !organizations::is_member_in(tx, organization_id, &policy.role_ids_for(&user.roles))
        .await
        .map_err(internal)?
    {
        return Err(missing(lang));
    }
    Ok((org, user, policy))
}

async fn preflight_admin(state: &RamaState, user_id: &str, lang: Lang) -> Result<(), Response> {
    let mut tx = WriteTx::begin(&state.db).await.map_err(internal)?;
    let (user, policy) = current_policy_in(state, &mut tx, user_id, lang).await?;
    require_admin(&policy, &user, lang)?;
    tx.commit().await.map_err(internal)
}

async fn preflight_member(
    state: &RamaState,
    user_id: &str,
    id: &str,
    manager: bool,
    lang: Lang,
) -> Result<(), Response> {
    let mut tx = WriteTx::begin(&state.db).await.map_err(internal)?;
    let (_, user, policy) = member_in(state, &mut tx, user_id, id, lang).await?;
    if manager && !policy.can_manage_agents(&policy.role_ids_for(&user.roles)) {
        return Err(error(
            lang,
            StatusCode::FORBIDDEN,
            "forbidden",
            "organization-error-permission-required",
        ));
    }
    tx.commit().await.map_err(internal)
}

pub async fn admin_list(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_, user) = require_session_json!(state, req);
    let lang = Lang::from_request(req.headers());
    async {
        let mut tx = WriteTx::begin(&state.db).await.map_err(internal)?;
        let (current, policy) = current_policy_in(&state, &mut tx, &user.id, lang).await?;
        require_admin(&policy, &current, lang)?;
        let organizations = organizations::list_admin_in(&mut tx)
            .await
            .map_err(internal)?
            .iter()
            .map(AdminOrganizationView::from)
            .collect::<Vec<_>>();
        let member_groups = organizations
            .iter()
            .flat_map(|organization| organization.member_groups.iter().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let member_group_user_counts =
            gateway_groups::group_member_counts_in(&mut tx, &member_groups)
                .await
                .map_err(internal)?;
        let groups = gateway_groups::list_group_names_in(&mut tx)
            .await
            .map_err(internal)?;
        tx.commit().await.map_err(internal)?;
        Ok::<_, Response>(json_ok(
            StatusCode::OK,
            AdminOrganizationList {
                organizations,
                groups,
                member_group_user_counts,
            },
        ))
    }
    .await
    .unwrap_or_else(|response| response)
}

pub async fn admin_create(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_, user) = require_session_json!(state, req);
    let lang = Lang::from_request(req.headers());
    async {
        preflight_admin(&state, &user.id, lang).await?;
        let body: OrganizationBody =
            super::read_json(req.into_body(), "the organization body").await?;
        if !valid_name(&body.name) || body.description.len() > 16384 {
            return Err(invalid(lang));
        }
        let mut tx = WriteTx::begin(&state.db).await.map_err(internal)?;
        let (current, policy) = current_policy_in(&state, &mut tx, &user.id, lang).await?;
        require_admin(&policy, &current, lang)?;
        let result = organizations::create_in(
            &mut tx,
            &organizations::NewOrganization {
                name: body.name.trim(),
                description: body.description.trim(),
                member_groups: &body.member_groups,
            },
            &current.id,
        )
        .await
        .map_err(internal)?;
        let organizations::OrganizationCreateResult::Created(row) = result else {
            return Err(invalid(lang));
        };
        tx.commit().await.map_err(internal)?;
        Ok::<_, Response>(json_ok(
            StatusCode::CREATED,
            AdminOrganizationReply {
                organization: AdminOrganizationView::from(&row),
            },
        ))
    }
    .await
    .unwrap_or_else(|response| response)
}

pub async fn admin_update(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_, user) = require_session_json!(state, req);
    let lang = Lang::from_request(req.headers());
    async {
        let id = raw_path_segment(&req, 0).ok_or_else(|| invalid(lang))?;
        preflight_admin(&state, &user.id, lang).await?;
        let body: OrganizationEditBody =
            super::read_json(req.into_body(), "the organization edit body").await?;
        if !valid_name(&body.name) || body.description.len() > 16384 {
            return Err(invalid(lang));
        }
        let mut tx = WriteTx::begin(&state.db).await.map_err(internal)?;
        let (current, policy) = current_policy_in(&state, &mut tx, &user.id, lang).await?;
        require_admin(&policy, &current, lang)?;
        let result = organizations::edit_in(
            &mut tx,
            &id,
            &organizations::OrganizationEdit {
                name: body.name.trim(),
                description: body.description.trim(),
                member_groups: &body.member_groups,
                expected_revision: &body.expected_revision,
            },
            &current.id,
        )
        .await
        .map_err(internal)?;
        let row = match result {
            organizations::OrganizationEditResult::Missing => return Err(missing(lang)),
            organizations::OrganizationEditResult::Conflict => {
                return Err(error(
                    lang,
                    StatusCode::CONFLICT,
                    "conflict",
                    "organization-error-conflict",
                ));
            }
            organizations::OrganizationEditResult::InvalidGroups => return Err(invalid(lang)),
            organizations::OrganizationEditResult::Unchanged(row)
            | organizations::OrganizationEditResult::Saved(row) => row,
        };
        tx.commit().await.map_err(internal)?;
        Ok::<_, Response>(json_ok(
            StatusCode::OK,
            AdminOrganizationReply {
                organization: AdminOrganizationView::from(&row),
            },
        ))
    }
    .await
    .unwrap_or_else(|response| response)
}

pub async fn list(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_, user) = require_session_json!(state, req);
    let lang = Lang::from_request(req.headers());
    async {
        let mut tx = WriteTx::begin(&state.db).await.map_err(internal)?;
        let (current, policy) = current_policy_in(&state, &mut tx, &user.id, lang).await?;
        let groups = policy.role_ids_for(&current.roles);
        let can_create_dot = policy.can_manage_agents(&groups);
        let rows = organizations::list_for_groups_in(&mut tx, &groups)
            .await
            .map_err(internal)?;
        let organizations = rows
            .iter()
            .map(|row| OrganizationListRow {
                organization: OrganizationView::from(row),
                can_create_dot,
            })
            .collect();
        tx.commit().await.map_err(internal)?;
        Ok::<_, Response>(json_ok(StatusCode::OK, OrganizationList { organizations }))
    }
    .await
    .unwrap_or_else(|response| response)
}

pub async fn detail(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_, user) = require_session_json!(state, req);
    let lang = Lang::from_request(req.headers());
    async {
        let id = raw_path_segment(&req, 0).ok_or_else(|| invalid(lang))?;
        let mut tx = WriteTx::begin(&state.db).await.map_err(internal)?;
        let (org, current, policy) = member_in(&state, &mut tx, &user.id, &id, lang).await?;
        let owner = agents::DotOwner::Organization(id);
        let active_count = agents::active_dot_count_in(&mut tx, &owner)
            .await
            .map_err(internal)?;
        let can_create_dot = policy.can_manage_agents(&policy.role_ids_for(&current.roles));
        let visible = super::json_agents::visible_agents_in(&state, &mut tx, &current.id).await?;
        let ids = visible
            .iter()
            .map(|(row, _)| row.principal.id.clone())
            .collect::<Vec<_>>();
        let metadata = agents::dot_metadata_for_agents_in(&mut tx, &ids)
            .await
            .map_err(internal)?;
        let rows = visible
            .into_iter()
            .filter(|(row, _)| {
                metadata
                    .get(&row.principal.id)
                    .is_some_and(|dot| dot.owner == owner)
            })
            .collect::<Vec<_>>();
        let dots = super::json_agents::agent_views_in(&mut tx, &rows).await?;
        let detail = OrganizationDetail {
            organization: OrganizationView::from(&org.organization),
            dots,
            active_count,
            active_limit: state.config().dots.max_active_per_organization,
            can_create_dot,
        };
        tx.commit().await.map_err(internal)?;
        Ok::<_, Response>(json_ok(StatusCode::OK, detail))
    }
    .await
    .unwrap_or_else(|response| response)
}

pub async fn create_dot(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_, user) = require_session_json!(state, req);
    let lang = Lang::from_request(req.headers());
    async {
        let id = raw_path_segment(&req, 1).ok_or_else(|| invalid(lang))?;
        preflight_member(&state, &user.id, &id, true, lang).await?;
        let body: OrganizationDotBody =
            super::read_json(req.into_body(), "the organization Dot body").await?;
        if !valid_name(&body.name) || body.description.len() > 16384 {
            return Err(invalid(lang));
        }
        let name = body.name.trim();
        let display = body
            .display
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(name);
        if display.chars().count() > 80 || display.chars().any(char::is_control) {
            return Err(invalid(lang));
        }
        let mut tx = WriteTx::begin(&state.db).await.map_err(internal)?;
        let (_, current, policy) = member_in(&state, &mut tx, &user.id, &id, lang).await?;
        if !policy.can_manage_agents(&policy.role_ids_for(&current.roles)) {
            return Err(error(
                lang,
                StatusCode::FORBIDDEN,
                "forbidden",
                "organization-error-permission-required",
            ));
        }
        let (outcome, agent) = agents::create_organization_dot_in(
            &mut tx,
            &id,
            &NewPrincipal {
                name,
                display,
                description: body.description.trim(),
            },
            "{}",
            &current.id,
            state.config().dots.max_active_per_organization,
        )
        .await
        .map_err(internal)?;
        let agent = match (outcome, agent) {
            (agents::DotCreate::Created, Some(agent)) => agent,
            (agents::DotCreate::NameTaken, _) => {
                return Err(error(
                    lang,
                    StatusCode::CONFLICT,
                    "name_conflict",
                    "organization-error-name-conflict",
                ));
            }
            _ => {
                return Err(error(
                    lang,
                    StatusCode::CONFLICT,
                    "active_limit",
                    "organization-error-active-limit",
                ));
            }
        };
        let agent = super::json_agents::agent_views_in(&mut tx, &[(agent, agents::Access::Write)])
            .await?
            .remove(0);
        tx.commit().await.map_err(internal)?;
        Ok::<_, Response>(json_ok(StatusCode::CREATED, OrganizationDotReply { agent }))
    }
    .await
    .unwrap_or_else(|response| response)
}

pub async fn set_active(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_, user) = require_session_json!(state, req);
    let lang = Lang::from_request(req.headers());
    async {
        let org_id = raw_path_segment(&req, 3).ok_or_else(|| invalid(lang))?;
        let dot_id = raw_path_segment(&req, 1).ok_or_else(|| invalid(lang))?;
        preflight_member(&state, &user.id, &org_id, false, lang).await?;
        let body: OrganizationDotActiveBody =
            super::read_json(req.into_body(), "the organization Dot active body").await?;
        let mut tx = WriteTx::begin(&state.db).await.map_err(internal)?;
        member_in(&state, &mut tx, &user.id, &org_id, lang).await?;
        if effective_access_in(&state, &mut tx, &dot_id, &user.id)
            .await
            .map_err(internal)?
            != Some(agents::Access::Write)
        {
            return Err(missing(lang));
        }
        let owner = agents::DotOwner::Organization(org_id);
        if agents::dot_owner_in(&mut tx, &dot_id)
            .await
            .map_err(internal)?
            .as_ref()
            != Some(&owner)
        {
            return Err(missing(lang));
        }
        if !agents::set_dot_active_in(
            &mut tx,
            &dot_id,
            &owner,
            body.active,
            state.config().dots.max_active_per_organization,
            &user.id,
        )
        .await
        .map_err(internal)?
        {
            return Err(error(
                lang,
                StatusCode::CONFLICT,
                "active_limit",
                "organization-error-active-limit",
            ));
        }
        let agent = agents::get_in(&mut tx, &dot_id)
            .await
            .map_err(internal)?
            .ok_or_else(|| missing(lang))?;
        let agent = super::json_agents::agent_views_in(&mut tx, &[(agent, agents::Access::Write)])
            .await?
            .remove(0);
        tx.commit().await.map_err(internal)?;
        Ok::<_, Response>(json_ok(StatusCode::OK, OrganizationDotReply { agent }))
    }
    .await
    .unwrap_or_else(|response| response)
}
