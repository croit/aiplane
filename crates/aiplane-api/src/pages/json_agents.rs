// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! `/api/v0/agents` — agent definitions: drafts, immutable versions with a
//! live pointer, and shares (`docs/agents.md` → "Agent definition").
//!
//! Every route needs the agent-management permission, and every route on one
//! agent also needs a share on it: `read` to see it, `write` to change it.
//! Such a share takes effect only for a holder of the permission, so it is
//! refused for anyone who lacks it, and a holder who loses it loses access
//! with it. A `respond` share answers the agent's inbox items and needs no
//! permission; here it is no access at all. An agent nobody shares is
//! invisible: it answers 404, not 403.
//! Admins are the exception: they hold `write` on every agent without a
//! share, so an agent whose last writer left can always be recovered.
//!
//! The agent's grants are its principal's, managed through
//! `/api/v0/system-principals/{id}/grants` with the grant-time cap (`docs/agents.md` → "Principals");
//! those routes check the share here too ([`guard_principal`]).

use session_core::i18n::{Lang, t};
use std::collections::HashMap;
use std::sync::Arc;

use jiff::Timestamp;
use rama::http::service::web::extract::State;
use rama::http::{Request, Response, StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};

use super::json_principals::{GrantView, require_agent_manager};
use super::{
    bad_request, internal, json_error, json_error_with, json_ok, no_content, not_found,
    raw_path_segment,
};
use aiplane_agents::db::agent_delegation::ConsentSaved;
use aiplane_agents::db::agents::{
    self as agents_db, Access, DraftChange, ShareChange, SubjectKind,
};
use aiplane_agents::db::{
    agent_analytics, agent_audit, agent_delegation, system_principals as sp_db,
};
use aiplane_core::server::db::users;
use aiplane_core::server::principal::{GrantKind, GrantSet};
use aiplane_core::server::upstreams::PoolKind;
use aiplane_runtime::agents::access::effective_access;
use aiplane_runtime::agents::defaults;
use aiplane_runtime::agents::embed::LimitsView;
use aiplane_runtime::agents::spec::secrets;
use aiplane_runtime::agents::spec::{
    self, AgentSpec, ModelDefaults, SpecContext, SpecIssue, Stage,
};
use aiplane_runtime::rama_server::state::RamaState;

fn group_ids(state: &RamaState, user: &users::User) -> Vec<String> {
    state.rbac.role_ids_for(&user.roles)
}

/// The caller's access to agent `id` ([`effective_access`]) — a 404 when
/// they cannot see it, a 403 when they can read it but `need` more.
async fn access(
    state: &RamaState,
    user: &users::User,
    id: &str,
    need: Access,
) -> Result<Option<Access>, Response> {
    let held = if is_personal(state, id).await? {
        None
    } else {
        effective_access(state, id, &user.id)
            .await
            .map_err(internal)?
    };
    require_held(held, id, need)
}

/// The caller, if they may delete personal agent `id` of `owner`: the owner
/// while they may manage personal agents, or an admin. Anyone else learns
/// nothing about it.
async fn require_personal_deleter(
    state: &RamaState,
    req: &Request,
    owner: &str,
    id: &str,
) -> Result<users::User, Response> {
    let (_, user) = super::require_session_json(state, req).await?;
    let roles = state.rbac.role_ids_for(&user.roles);
    if (user.id == owner && state.rbac.can_manage_personal_agents(&roles))
        || state.rbac.is_admin(&roles)
    {
        Ok(user)
    } else {
        Err(not_found(format!(
            "there is no agent `{id}` shared with you — ask its owner for a share"
        )))
    }
}

/// Whether agent `id` is a personal agent. The agent builder's routes never
/// serve one, admins included, deletion aside ([`delete`]): its settings live on the owner-scoped
/// `/api/v0/agents/{id}/*` routes, which check every tool against the owner's
/// own grants (`docs/agents.md` → "Who sees an agent").
pub(super) async fn is_personal(state: &RamaState, id: &str) -> Result<bool, Response> {
    Ok(agents_db::personal_agent_owner(&state.db, id)
        .await
        .map_err(internal)?
        .is_some())
}

pub(super) fn require_held(
    held: Option<Access>,
    id: &str,
    need: Access,
) -> Result<Option<Access>, Response> {
    match held {
        Some(a) if a >= need => Ok(Some(a)),
        Some(a) if a >= Access::Read => Err(json_error(
            StatusCode::FORBIDDEN,
            "agent_write_required",
            "you can read this agent but not change it — ask someone with a `write` share to \
             upgrade yours",
        )),
        _ => Err(not_found(format!(
            "there is no agent `{id}` shared with you — ask its owner for a share"
        ))),
    }
}

pub(super) async fn begin_write(
    state: &RamaState,
    user: &users::User,
    id: &str,
) -> Result<aiplane_core::server::db::WriteTx, Response> {
    let mut tx = aiplane_core::server::db::WriteTx::begin(&state.db)
        .await
        .map_err(internal)?;
    let held = if is_personal(state, id).await? {
        None
    } else {
        aiplane_runtime::agents::access::effective_access_in(state, &mut tx, id, &user.id)
            .await
            .map_err(internal)?
    };
    require_held(held, id, Access::Write)?;
    Ok(tx)
}

/// The agent named by the path segment `from_end` back, if the caller holds
/// `need` on it.
pub(super) async fn agent_at(
    state: &RamaState,
    req: &Request,
    user: &users::User,
    from_end: usize,
    need: Access,
) -> Result<(agents_db::AgentRow, Access), Response> {
    let Some(id) = raw_path_segment(req, from_end) else {
        return Err(bad_request("the URL is missing the agent id"));
    };
    agent_by_id(state, user, &id, need).await
}

/// Agent `id`, if the caller holds `need` on it.
pub(super) async fn agent_by_id(
    state: &RamaState,
    user: &users::User,
    id: &str,
    need: Access,
) -> Result<(agents_db::AgentRow, Access), Response> {
    let Some(agent) = agents_db::get(&state.db, id).await.map_err(internal)? else {
        return Err(not_found(format!(
            "there is no agent `{id}` shared with you — ask its owner for a share"
        )));
    };
    let held = access(state, user, id, need).await?;
    Ok((agent, held.unwrap_or(need)))
}

/// For the `/api/v0/system-principals` routes. An agent's principal needs a
/// share on the agent, as on `/api/v0/agents`. Any other principal belongs to
/// whoever created it: only they or an admin may see or change it, so one
/// manager can never work with what another manager granted.
pub(crate) async fn guard_principal(
    state: &RamaState,
    user: &users::User,
    principal: &sp_db::PrincipalRow,
    need: Access,
) -> Result<(), Response> {
    if agents_db::get(&state.db, &principal.id)
        .await
        .map_err(internal)?
        .is_some()
    {
        return access(state, user, &principal.id, need).await.map(|_| ());
    }
    if principal.created_by == user.id || state.rbac.is_admin(&group_ids(state, user)) {
        return Ok(());
    }
    Err(not_found(not_managed_message(&principal.id)))
}

fn not_managed_message(principal_id: &str) -> String {
    format!(
        "there is no system principal `{principal_id}` you manage — only its creator or an admin \
         can see or change it"
    )
}

pub(super) fn parse_spec(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or(Value::Null)
}

/// An agent as the API shows it, with the caller's access to it.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct AgentView {
    pub id: String,
    pub name: String,
    pub display: String,
    pub description: String,
    pub created_by: String,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub disabled_at: Option<Timestamp>,
    pub live_version: Option<i64>,
    /// The caller's access to the agent.
    pub access: &'static str,
    pub kind: AgentKind,
    /// Who the agent belongs to: its user or organization, and for a public
    /// agent the person who created it.
    pub owner: OwnerView,
    /// Who shared the agent with the caller, and when: set in the caller's
    /// workspace on an agent that is not their own.
    pub shared_by: Option<ShareOriginView>,
    pub ownership: Option<AgentOwnershipView>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ShareOriginView {
    pub by: OwnerView,
    pub at: Timestamp,
}

/// Whom an agent works for (`docs/agents.md` → "Agent kinds").
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    /// Owned by one user, acting with that user's own rights.
    Personal,
    /// Owned by an organization, acting as its own principal.
    Team,
    /// Answers outside visitors, acting as its own principal.
    Public,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerView {
    /// `name` and `email` are absent when the user is no longer known.
    User {
        id: String,
        name: Option<String>,
        email: Option<String>,
    },
    Organization {
        id: String,
        name: String,
    },
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct AgentOwnershipView {
    pub name: String,
    pub active: bool,
}

pub(super) async fn agent_views(
    state: &RamaState,
    rows: &[(agents_db::AgentRow, Access)],
) -> Result<Vec<AgentView>, Response> {
    let mut tx = aiplane_core::server::db::WriteTx::begin(&state.db)
        .await
        .map_err(internal)?;
    let views = agent_views_in(&mut tx, rows).await?;
    tx.commit().await.map_err(internal)?;
    Ok(views)
}

pub(super) async fn agent_views_in(
    tx: &mut aiplane_core::server::db::WriteTx,
    rows: &[(agents_db::AgentRow, Access)],
) -> Result<Vec<AgentView>, Response> {
    let ids = rows
        .iter()
        .map(|(a, _)| a.principal.id.clone())
        .collect::<Vec<_>>();
    let metadata = agents_db::agent_metadata_for_agents_in(tx, &ids)
        .await
        .map_err(internal)?;
    let organizations = aiplane_agents::db::organizations::list_admin_in(tx)
        .await
        .map_err(internal)?;
    let names = organizations
        .into_iter()
        .map(|row| (row.organization.id, row.organization.name))
        .collect::<std::collections::HashMap<_, _>>();
    let people = people_in(
        tx,
        rows.iter()
            .filter_map(|(agent, _)| {
                match metadata
                    .get(&agent.principal.id)
                    .map(|owned_agent| &owned_agent.owner)
                {
                    Some(agents_db::AgentOwner::User(id)) => Some(id.clone()),
                    Some(agents_db::AgentOwner::Organization(_)) => None,
                    None => Some(agent.principal.created_by.clone()),
                }
            })
            .collect(),
    )
    .await?;
    let person_view = |id: &String| person_view(&people, id);
    rows.iter()
        .map(|(agent, access)| {
            let (kind, owner) = match metadata
                .get(&agent.principal.id)
                .map(|owned_agent| &owned_agent.owner)
            {
                Some(agents_db::AgentOwner::User(id)) => (AgentKind::Personal, person_view(id)),
                Some(agents_db::AgentOwner::Organization(id)) => {
                    let Some(name) = names.get(id) else {
                        return Err(internal(
                            "organization owner disappeared while reading agent metadata",
                        ));
                    };
                    (
                        AgentKind::Team,
                        OwnerView::Organization {
                            id: id.clone(),
                            name: name.clone(),
                        },
                    )
                }
                None => (AgentKind::Public, person_view(&agent.principal.created_by)),
            };
            let p = &agent.principal;
            Ok(AgentView {
                id: p.id.clone(),
                name: p.name.clone(),
                display: p.display.clone(),
                description: p.description.clone(),
                created_by: p.created_by.clone(),
                created_at: agent.created_at,
                updated_at: agent.updated_at,
                disabled_at: p.disabled_at,
                live_version: agent.live_version,
                access: access.as_str(),
                kind,
                owner,
                shared_by: None,
                ownership: metadata.get(&p.id).map(|owned_agent| AgentOwnershipView {
                    name: owned_agent.name.clone(),
                    active: owned_agent.active,
                }),
            })
        })
        .collect()
}

/// The users behind `ids`, each looked up once; `None` for one no longer known.
async fn people_in(
    tx: &mut aiplane_core::server::db::WriteTx,
    ids: Vec<String>,
) -> Result<HashMap<String, Option<users::User>>, Response> {
    let mut people = HashMap::new();
    for id in ids {
        if let std::collections::hash_map::Entry::Vacant(slot) = people.entry(id) {
            let found = users::find_by_id_in(tx, slot.key())
                .await
                .map_err(internal)?;
            slot.insert(found);
        }
    }
    Ok(people)
}

fn person_view(people: &HashMap<String, Option<users::User>>, id: &String) -> OwnerView {
    let found = people.get(id).and_then(Option::as_ref);
    OwnerView::User {
        id: id.clone(),
        name: found.and_then(|user| user.name.clone()),
        email: found.map(|user| user.email.clone()),
    }
}

pub(super) async fn agent_view_for(
    state: &RamaState,
    agent: &agents_db::AgentRow,
    access: Access,
) -> Result<AgentView, Response> {
    Ok(agent_views(state, &[(agent.clone(), access)])
        .await?
        .remove(0))
}

pub(super) async fn agent_json(
    state: &RamaState,
    a: &agents_db::AgentRow,
    access: Access,
) -> Result<Value, Response> {
    Ok(json!(agent_view_for(state, a, access).await?))
}

/// One share of an agent.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ShareView {
    /// `user` or `group`.
    pub subject_kind: &'static str,
    pub subject_id: String,
    /// `respond`, `read` or `write`.
    pub access: &'static str,
    /// A user's display name (null when they have none); absent on a group,
    /// or a user no longer known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<Option<String>>,
    /// Who gave the share and when; absent on a share older than the record.
    pub granted: Option<ShareOriginView>,
}

/// The shares as the API shows them: a person's share carries their display
/// name, so the people an agent is shared with are recognisable by whoever
/// may see its shares.
async fn share_views(
    state: &RamaState,
    rows: &[agents_db::ShareRow],
) -> Result<Vec<ShareView>, Response> {
    let mut tx = aiplane_core::server::db::WriteTx::begin(&state.db)
        .await
        .map_err(internal)?;
    let people = people_in(
        &mut tx,
        rows.iter()
            .filter(|s| s.subject_kind == SubjectKind::User)
            .map(|s| s.subject_id.clone())
            .chain(rows.iter().filter_map(|s| s.granted_by.clone()))
            .collect(),
    )
    .await?;
    tx.commit().await.map_err(internal)?;
    Ok(rows
        .iter()
        .map(|s| ShareView {
            subject_kind: s.subject_kind.as_str(),
            subject_id: s.subject_id.clone(),
            access: s.access.as_str(),
            name: (s.subject_kind == SubjectKind::User)
                .then(|| {
                    people
                        .get(&s.subject_id)
                        .and_then(Option::as_ref)
                        .map(|u| u.name.clone())
                })
                .flatten(),
            granted: match (&s.granted_by, s.granted_at) {
                (Some(by), Some(at)) => Some(ShareOriginView {
                    by: person_view(&people, by),
                    at,
                }),
                _ => None,
            },
        })
        .collect())
}

/// One published version.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct VersionView {
    pub version: i64,
    /// The agent spec as published (`docs/agents.md` → "Agent definition").
    pub spec: Value,
    pub published_by: String,
    pub published_at: Timestamp,
}

pub(crate) fn version_view(v: &agents_db::VersionRow) -> VersionView {
    VersionView {
        version: v.version,
        spec: parse_spec(&v.spec),
        published_by: v.published_by.clone(),
        published_at: v.published_at,
    }
}

/// Validate `spec` for agent `agent_id` against its grants as stored now.
async fn spec_issues(
    state: &RamaState,
    agent_id: &str,
    spec: &Value,
    stage: Stage,
) -> Result<Vec<SpecIssue>, Response> {
    Ok(spec_check(state, agent_id, spec, stage)
        .await?
        .err()
        .unwrap_or_default())
}

/// [`spec_issues`] at the publish stage: what blocks publishing `spec`.
pub(super) async fn publish_issues(
    state: &RamaState,
    agent_id: &str,
    spec: &Value,
) -> Result<Vec<SpecIssue>, Response> {
    spec_issues(state, agent_id, spec, Stage::Publish).await
}

/// [`spec_issues`], with the typed spec when there are none.
async fn spec_check(
    state: &RamaState,
    agent_id: &str,
    spec: &Value,
    stage: Stage,
) -> Result<Result<AgentSpec, Vec<SpecIssue>>, Response> {
    let world = SpecWorld::load(state, agent_id).await?;
    Ok(spec::check(
        spec,
        &SpecContext {
            agent_id,
            grants: &world.grants,
            agents: &world.agents,
            live_specs: &world.live_specs,
            model_defaults: &world.model_defaults,
            speech_voices: &world.speech_voices,
            action_catalogues: &world.action_catalogues,
            allow_private: world.allow_private,
        },
        stage,
    ))
}

/// What a spec of agent `agent_id` is checked against: its grants as
/// stored now, every agent's publication status and every live spec.
pub(super) struct SpecWorld {
    pub grants: GrantSet,
    pub agents: HashMap<String, bool>,
    pub live_specs: HashMap<String, Value>,
    pub model_defaults: ModelDefaults,
    /// The voices of every speech model the gateway serves, by model.
    pub speech_voices: HashMap<String, Vec<String>>,
    pub action_catalogues: std::collections::BTreeMap<String, std::collections::BTreeSet<String>>,
    pub allow_private: bool,
}

impl SpecWorld {
    pub(super) async fn load(state: &RamaState, agent_id: &str) -> Result<Self, Response> {
        let grants = sp_db::grants(&state.db, agent_id).await.map_err(internal)?;
        let grants = GrantSet::new(grants.into_iter().map(|g| (g.kind, g.reference)));
        let model_defaults = defaults::model_defaults(state).await;
        let agents = agents_db::publication_status(&state.db)
            .await
            .map_err(internal)?;
        let live_specs = agents_db::live_specs(&state.db)
            .await
            .map_err(internal)?
            .into_iter()
            .map(|(id, text)| (id, parse_spec(&text)))
            .collect();
        let speech_voices = state
            .upstreams
            .models_for_kind(PoolKind::Speech)
            .into_iter()
            .chain(model_defaults.speech.clone())
            .map(|model| {
                let voices = state.upstreams.speech_voices_of(&model);
                (model, voices)
            })
            .collect();
        Ok(Self {
            grants,
            agents,
            live_specs,
            model_defaults,
            speech_voices,
            action_catalogues: aiplane_runtime::server::tools::registry::action_catalogues(
                state.tools().as_ref(),
            ),
            allow_private: state.config().network.allow_private_networks,
        })
    }
}

/// Seal every verifier secret and A2A route credential once the spec is
/// valid (the plaintext is checked), before it is stored or echoed: it never
/// rests in a draft, a version or the audit trail in clear.
fn seal_secrets(state: &RamaState, spec: &mut Value) -> Result<(), Response> {
    secrets::seal_spec_secrets(spec, secrets::SPEC_SECRETS, &state.crypto).map_err(internal)
}

fn invalid_spec(what: &str, issues: &[SpecIssue]) -> Response {
    let first = &issues[0];
    let at = if first.path.is_empty() {
        "the spec".to_string()
    } else {
        format!("`{}`", first.path)
    };
    let more = match issues.len() {
        1 => String::new(),
        n => format!(" (and {} more — see `issues`)", n - 1),
    };
    json_error_with(
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_agent_spec",
        &format!("cannot {what}: at {at}, {}{more}", first.message),
        serde_json::Map::from_iter([("issues".to_string(), json!(issues))]),
    )
}

async fn require_valid(
    state: &RamaState,
    agent_id: &str,
    spec: &Value,
    stage: Stage,
    what: &str,
) -> Result<AgentSpec, Response> {
    spec_check(state, agent_id, spec, stage)
        .await?
        .map_err(|issues| invalid_spec(what, &issues))
}

/// GET /api/v0/agents — the caller's own agents and those shared with them;
/// with `?scope=all`, every user's agent for an admin (`docs/agents.md` →
/// "Who sees an agent"). A non-admin's `scope=all` is ignored, as on the
/// usage page.
pub async fn list(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let user = or_return!(require_agent_or_personal_agent_manager(&state, &req).await);
    let wants_all = super::query_map(&req).get("scope").map(String::as_str) == Some("all");
    let mut tx = or_return!(
        aiplane_core::server::db::WriteTx::begin(&state.db)
            .await
            .map_err(internal)
    );
    let (can_view_all, groups) = or_return!(caller_in(&state, &mut tx, &user.id).await);
    let show_all = can_view_all && wants_all;
    let rows = if show_all {
        or_return!(agents_db::list_all_in(&mut tx).await.map_err(internal))
            .into_iter()
            .map(|row| (row, Access::Write))
            .collect()
    } else {
        let may_manage_personal_agents = state.rbac.can_manage_personal_agents(&groups);
        let mut rows =
            or_return!(own_personal_agents_in(&mut tx, &user.id, may_manage_personal_agents).await);
        rows.extend(or_return!(
            visible_agents_in(&state, &mut tx, &user.id).await
        ));
        rows
    };
    let mut agents = or_return!(agent_views_in(&mut tx, &rows).await);
    if !show_all {
        or_return!(attach_share_origins_in(&mut tx, &mut agents, &user.id, &groups).await);
    }
    or_return!(tx.commit().await.map_err(internal));
    json_ok(
        StatusCode::OK,
        AgentList {
            scope: if show_all { "all" } else { "self" },
            can_view_all,
            agents,
        },
    )
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct AgentList {
    /// `all` or `self`.
    pub scope: &'static str,
    /// Whether the caller may ask for `scope=all`.
    pub can_view_all: bool,
    pub agents: Vec<AgentView>,
}

/// The caller, if they may build agents or personal agents: the list is
/// where both kinds live.
async fn require_agent_or_personal_agent_manager(
    state: &RamaState,
    req: &Request,
) -> Result<users::User, Response> {
    let (_, user) = super::require_session_json(state, req).await?;
    let role_ids = state.rbac.role_ids_for(&user.roles);
    if state.rbac.can_manage_agents(&role_ids) || state.rbac.can_manage_personal_agents(&role_ids) {
        return Ok(user);
    }
    Err(json_error(
        StatusCode::FORBIDDEN,
        "forbidden",
        &t(
            Lang::from_request(req.headers()),
            "agent-management-error-permission-required",
        ),
    ))
}

/// Whether the caller is an admin, and the groups they resolve to.
async fn caller_in(
    state: &RamaState,
    tx: &mut aiplane_core::server::db::WriteTx,
    user_id: &str,
) -> Result<(bool, Vec<String>), Response> {
    Ok(
        match aiplane_runtime::agents::access::current_user_policy_in(state, tx, user_id)
            .await
            .map_err(internal)?
        {
            Some((user, policy)) => {
                let groups = policy.role_ids_for(&user.roles);
                (policy.is_admin(&groups), groups)
            }
            None => (false, Vec::new()),
        },
    )
}

/// Names who shared each agent with `user_id`, on the agents that are not
/// theirs: an agent they created, or a share they gave themselves, names no
/// one.
async fn attach_share_origins_in(
    tx: &mut aiplane_core::server::db::WriteTx,
    views: &mut [AgentView],
    user_id: &str,
    groups: &[String],
) -> Result<(), Response> {
    let origins = agents_db::share_origins_in(tx, user_id, groups)
        .await
        .map_err(internal)?;
    let people = people_in(
        tx,
        origins
            .values()
            .map(|origin| origin.granted_by.clone())
            .collect(),
    )
    .await?;
    for view in views {
        if view.created_by != user_id
            && let Some(origin) = origins.get(&view.id)
        {
            view.shared_by = Some(ShareOriginView {
                by: person_view(&people, &origin.granted_by),
                at: origin.granted_at,
            });
        }
    }
    Ok(())
}

/// The agents shared with `user`, with their access — admins included:
/// another user's agent reaches an admin only through `scope=all`. Personal
/// agents are not among them: no one shares one, and its owner's list adds
/// it separately.
pub(super) async fn visible_agents(
    state: &RamaState,
    user: &users::User,
) -> Result<Vec<(agents_db::AgentRow, Access)>, Response> {
    let mut tx = aiplane_core::server::db::WriteTx::begin(&state.db)
        .await
        .map_err(internal)?;
    let rows = visible_agents_in(state, &mut tx, &user.id).await?;
    tx.commit().await.map_err(internal)?;
    Ok(rows)
}

pub(super) async fn visible_agents_in(
    state: &RamaState,
    tx: &mut aiplane_core::server::db::WriteTx,
    user_id: &str,
) -> Result<Vec<(agents_db::AgentRow, Access)>, Response> {
    let Some((current, policy)) =
        aiplane_runtime::agents::access::current_user_policy_in(state, tx, user_id)
            .await
            .map_err(internal)?
    else {
        return Ok(Vec::new());
    };
    let groups = policy.role_ids_for(&current.roles);
    if !policy.can_manage_agents(&groups) {
        return Ok(Vec::new());
    }
    let rows = agents_db::list_shared_with_in(tx, user_id, &groups)
        .await
        .map_err(internal)?;
    let ids = rows
        .iter()
        .map(|(row, _)| row.principal.id.clone())
        .collect::<Vec<_>>();
    let metadata = agents_db::agent_metadata_for_agents_in(tx, &ids)
        .await
        .map_err(internal)?;
    let memberships = aiplane_agents::db::organizations::list_for_groups_in(tx, &groups)
        .await
        .map_err(internal)?
        .into_iter()
        .map(|row| row.id)
        .collect::<std::collections::HashSet<_>>();
    Ok(rows
        .into_iter()
        .filter(|(row, access)| {
            *access >= Access::Read
                && metadata.get(&row.principal.id).is_none_or(|owned_agent| {
                    match &owned_agent.owner {
                        agents_db::AgentOwner::User(_) => false,
                        agents_db::AgentOwner::Organization(id) => memberships.contains(id),
                    }
                })
        })
        .collect())
}

/// The caller's own personal agents, when they may manage them: the list
/// shows them beside the shared ones, nothing else offers them as a target.
async fn own_personal_agents_in(
    tx: &mut aiplane_core::server::db::WriteTx,
    user_id: &str,
    may_manage: bool,
) -> Result<Vec<(agents_db::AgentRow, Access)>, Response> {
    if !may_manage {
        return Ok(Vec::new());
    }
    Ok(agents_db::list_personal_agents_in(tx, user_id)
        .await
        .map_err(internal)?
        .into_iter()
        .map(|owned_agent| (owned_agent.agent, Access::Write))
        .collect())
}

/// Whom `POST /api/v0/agents` creates the agent for. A team agent is
/// created under its organization instead.
#[derive(Clone, Copy, Default, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CreateKind {
    Personal,
    #[default]
    Public,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[schemars(rename = "AgentCreateBody")]
pub struct CreateBody {
    #[serde(default)]
    pub kind: CreateKind,
    pub name: String,
    #[serde(default)]
    pub display: Option<String>,
    #[serde(default)]
    pub description: String,
    /// The first draft's agent spec; `{}` when absent.
    #[serde(default)]
    pub spec: Option<Value>,
}

/// A new agent with its draft.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct CreatedAgent {
    #[serde(flatten)]
    pub agent: AgentView,
    pub draft_spec: Value,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct CreatedAgentReply {
    pub agent: CreatedAgent,
}

/// What `POST /api/v0/agents` answers: a public agent with its draft, or the
/// new personal agent.
#[derive(serde::Serialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum CreatedAgentAnyReply {
    Public(Box<CreatedAgentReply>),
    Personal(super::json_owned_agents::CreatedPersonalAgentReply),
}

/// POST /api/v0/agents — a personal agent for the caller (`kind: personal`),
/// or a public agent with a fresh principal (no grants) and a `write` share
/// for its creator.
pub async fn create(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    or_return!(super::require_session_json(&state, &req).await);
    let (parts, body) = req.into_parts();
    let req = Request::from_parts(parts, rama::http::Body::empty());
    let body: CreateBody = or_return!(super::read_json(body, "the agent body").await);
    if body.kind == CreateKind::Personal {
        if body.spec.is_some() {
            return bad_request(t(
                Lang::from_request(req.headers()),
                "agents-error-personal-spec",
            ));
        }
        return super::json_owned_agents::create(&state, &req, body).await;
    }
    let user = or_return!(require_agent_manager(&state, &req).await);
    match create_agent(&state, &user, body).await {
        Ok(agent) => json_ok(StatusCode::CREATED, CreatedAgentReply { agent }),
        Err(resp) => resp,
    }
}

/// Create an agent for `user` (who manages agents): the agent's JSON with
/// its draft.
pub(super) async fn create_agent(
    state: &RamaState,
    user: &users::User,
    body: CreateBody,
) -> Result<CreatedAgent, Response> {
    let name = body.name.trim();
    if let Some(reason) = sp_db::invalid_name_reason(name) {
        return Err(bad_request(reason));
    }
    let mut spec = body.spec.unwrap_or_else(|| json!({}));
    // The principal does not exist yet, so it holds nothing: a spec naming
    // any resource fails here, and its hint says how to grant one.
    require_valid(state, "{id}", &spec, Stage::Draft, "create the agent").await?;
    seal_secrets(state, &mut spec)?;
    let display = body
        .display
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .unwrap_or(name);
    let created = agents_db::create(
        &state.db,
        &sp_db::NewPrincipal {
            name,
            display,
            description: body.description.trim(),
        },
        &spec.to_string(),
        &user.id,
    )
    .await;
    match created {
        Ok(Some(a)) => Ok(CreatedAgent {
            agent: agent_view_for(state, &a, Access::Write).await?,
            draft_spec: spec,
        }),
        Ok(None) => Err(json_error(
            StatusCode::CONFLICT,
            "conflict",
            &format!(
                "a system principal named `{name}` already exists, and an agent's name is its \
                 principal's — pick another name"
            ),
        )),
        Err(err) => Err(internal(err)),
    }
}

/// GET /api/v0/agents/{id} — the agent, its draft and live spec, what
/// blocks publishing the draft, its limits and what has been spent against
/// them, its grants, shares and audit trail.
pub async fn detail(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let user = or_return!(require_agent_manager(&state, &req).await);
    let (agent, held) = or_return!(agent_at(&state, &req, &user, 0, Access::Read).await);
    let id = &agent.principal.id;
    let (grants, shares, audit) = match tokio::try_join!(
        sp_db::grants(&state.db, id),
        agents_db::shares(&state.db, id),
        agent_audit::for_principal(&state.db, id),
    ) {
        Ok(v) => v,
        Err(err) => return internal(err),
    };
    let live_spec = match agent.live_version {
        Some(n) => match agents_db::version(&state.db, id, n).await {
            Ok(v) => v.map(|v| parse_spec(&v.spec)),
            Err(err) => return internal(err),
        },
        None => None,
    };
    let draft = parse_spec(&agent.draft_spec);
    let publish_issues = or_return!(spec_issues(&state, id, &draft, Stage::Publish).await);

    let live_typed = live_spec
        .as_ref()
        .and_then(|s| AgentSpec::from_value(s).ok());
    let limits = aiplane_runtime::agents::embed::limits_view(
        &state,
        id,
        live_typed.as_ref(),
        jiff::Timestamp::now(),
    )
    .await;

    let shares = or_return!(share_views(&state, &shares).await);
    let audit = audit
        .into_iter()
        .map(|e| AgentAuditEntry {
            kind: e.kind,
            actor_id: e.actor_id,
            detail: e.detail,
            created_at: e.created_at,
        })
        .collect();
    json_ok(
        StatusCode::OK,
        AgentDetailReply {
            agent: AgentDetail {
                agent: or_return!(agent_view_for(&state, &agent, held).await),
                draft_spec: draft,
                live_spec,
                limits,
                publish_issues,
                grants: grants.iter().map(GrantView::of).collect(),
                shares,
                audit,
            },
        },
    )
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct AgentDetailReply {
    pub agent: AgentDetail,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct AgentDetail {
    #[serde(flatten)]
    pub agent: AgentView,
    pub draft_spec: Value,
    /// The live version's spec; `null` before the first publish.
    pub live_spec: Option<Value>,
    pub limits: LimitsView,
    /// What blocks publishing the draft.
    pub publish_issues: Vec<SpecIssue>,
    pub grants: Vec<GrantView>,
    pub shares: Vec<ShareView>,
    /// Newest first.
    pub audit: Vec<AgentAuditEntry>,
}

/// One management or run event on the agent.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct AgentAuditEntry {
    pub kind: String,
    pub actor_id: Option<String>,
    /// The event's own fields, which differ by `kind`.
    pub detail: Value,
    pub created_at: Timestamp,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct DraftBody {
    /// The agent spec (`docs/agents.md` → "Agent definition").
    pub spec: Value,
}

/// A saved draft.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct SavedDraft {
    pub draft_spec: Value,
    pub live_version: Option<i64>,
    /// The revision the replaced draft was kept as, if it was kept.
    pub revision: Option<i64>,
}

/// PUT /api/v0/agents/{id}/draft — replace the draft. What the live version
/// serves does not change.
pub async fn update_draft(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let lang = Lang::from_request(req.headers());
    let user = or_return!(require_agent_manager(&state, &req).await);
    let (agent, _) = or_return!(agent_at(&state, &req, &user, 1, Access::Write).await);
    let body: DraftBody = or_return!(super::read_json(req.into_body(), "the draft body").await);
    match save_draft(
        &state,
        &user,
        &agent,
        body.spec,
        DraftChange::default(),
        lang,
    )
    .await
    {
        Ok(saved) => json_ok(StatusCode::OK, saved),
        Err(resp) => resp,
    }
}

/// Validate `spec` as agent `agent`'s draft and save it, keeping the draft
/// it replaces as a revision: `{draft_spec, live_version, revision}`.
pub(super) async fn save_draft(
    state: &RamaState,
    user: &users::User,
    agent: &agents_db::AgentRow,
    mut spec: Value,
    change: DraftChange<'_>,
    lang: Lang,
) -> Result<SavedDraft, Response> {
    let id = &agent.principal.id;
    let validated_grants = sp_db::grants(&state.db, id).await.map_err(internal)?;
    require_valid(state, id, &spec, Stage::Draft, "save the draft").await?;
    seal_secrets(state, &mut spec)?;
    let mut tx = begin_write(state, user, id).await?;
    if sp_db::grants_in(&mut tx, id).await.map_err(internal)? != validated_grants {
        return Err(json_error(
            StatusCode::CONFLICT,
            "conflict",
            &t(lang, "agent-configuration-error-conflict"),
        ));
    }
    let saved = agents_db::update_draft_in(&mut tx, id, &spec.to_string(), &user.id, change)
        .await
        .map_err(internal)?;
    for (kind, reference) in change.revoking {
        sp_db::remove_grant_in(&mut tx, id, *kind, reference, &user.id)
            .await
            .map_err(internal)?;
    }
    tx.commit().await.map_err(internal)?;
    match Ok::<_, aiplane_core::server::db::DbError>(saved) {
        Ok(Some(saved)) => Ok(SavedDraft {
            draft_spec: spec,
            live_version: agent.live_version,
            revision: saved.revision,
        }),
        Ok(None) => Err(not_found(
            "the agent was deleted while its draft was being saved",
        )),
        Err(err) => Err(internal(err)),
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RestoreBody {
    pub revision: i64,
}

/// Whether `spec` needs `kind` `reference`: the validator finds more wrong
/// with it once that grant is gone. Asking the validator rather than listing
/// where a spec names a model or tool keeps this right as the spec grows. It
/// asks at the publish stage, the one that also checks the gateway default an
/// unset model key runs on: a draft that names no model still needs that
/// grant.
fn uses_grant(
    world: &SpecWorld,
    agent_id: &str,
    spec: &Value,
    kind: GrantKind,
    reference: &str,
) -> bool {
    let check = |grants: &GrantSet| {
        spec::validate(
            spec,
            &SpecContext {
                agent_id,
                grants,
                agents: &world.agents,
                live_specs: &world.live_specs,
                model_defaults: &world.model_defaults,
                speech_voices: &world.speech_voices,
                action_catalogues: &world.action_catalogues,
                allow_private: world.allow_private,
            },
            Stage::Publish,
        )
        .len()
    };
    let without = GrantSet::new(
        world
            .grants
            .iter()
            .filter(|(k, r)| !(*k == kind && *r == reference))
            .map(|(k, r)| (k, r.to_string())),
    );
    check(&without) > check(&world.grants)
}

/// POST /api/v0/agents/{id}/draft/restore — `{revision}`: make an earlier
/// draft the draft again (undo). It is validated like any save, and the
/// draft it replaces becomes a revision too, so a restore can be undone.
/// The grants the undone change made are revoked after the save, unless the
/// restored draft or the live version still uses them; the draft event in
/// the activity log names the restore and those grants, and each revocation
/// records its own `grant_removed`.
pub async fn restore_draft(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let lang = Lang::from_request(req.headers());
    let user = or_return!(require_agent_manager(&state, &req).await);
    let (agent, _) = or_return!(agent_at(&state, &req, &user, 2, Access::Write).await);
    let body: RestoreBody = or_return!(super::read_json(req.into_body(), "the restore body").await);
    let id = &agent.principal.id;
    let revision = match agents_db::draft_revision(&state.db, id, body.revision).await {
        Ok(Some(r)) => r,
        Ok(None) => {
            return not_found(format!(
                "agent `{}` keeps no earlier draft {} — only the newest {} are kept",
                agent.principal.name,
                body.revision,
                agents_db::MAX_DRAFT_REVISIONS
            ));
        }
        Err(err) => return internal(err),
    };
    let restored = parse_spec(&revision.spec);
    let world = or_return!(SpecWorld::load(&state, id).await);
    let live = match agent.live_version {
        Some(n) => match agents_db::version(&state.db, id, n).await {
            Ok(v) => v.map(|v| parse_spec(&v.spec)),
            Err(err) => return internal(err),
        },
        None => None,
    };
    let revoking: Vec<(GrantKind, String)> = revision
        .granted
        .iter()
        .filter(|(kind, reference)| world.grants.has(*kind, reference))
        .filter(|(kind, reference)| {
            !uses_grant(&world, id, &restored, *kind, reference)
                && !live
                    .as_ref()
                    .is_some_and(|l| uses_grant(&world, id, l, *kind, reference))
        })
        .cloned()
        .collect();
    let change = DraftChange {
        restored: Some(revision.id),
        revoking: &revoking,
        ..DraftChange::default()
    };
    let saved = or_return!(save_draft(&state, &user, &agent, restored, change, lang).await);

    let revoked = revoking
        .iter()
        .map(|(kind, reference)| GrantRef {
            kind: kind.as_str(),
            reference: reference.clone(),
        })
        .collect();
    json_ok(StatusCode::OK, RestoredDraft { saved, revoked })
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct RestoredDraft {
    #[serde(flatten)]
    pub saved: SavedDraft,
    /// The grants the undone change made, revoked because nothing uses them.
    pub revoked: Vec<GrantRef>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct GrantRef {
    pub kind: &'static str,
    #[serde(rename = "ref")]
    pub reference: String,
}

/// POST /api/v0/agents/{id}/publish — validate the draft for running and
/// snapshot it as the next version, which becomes live.
pub async fn publish(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let lang = Lang::from_request(req.headers());
    let user = or_return!(require_agent_manager(&state, &req).await);
    let (agent, _) = or_return!(agent_at(&state, &req, &user, 1, Access::Write).await);
    let id = &agent.principal.id;
    let draft = parse_spec(&agent.draft_spec);
    let validated_grants = or_return!(sp_db::grants(&state.db, id).await.map_err(internal));
    let checked =
        or_return!(require_valid(&state, id, &draft, Stage::Publish, "publish the agent").await);
    or_return!(super::json_agent_tests::require_green_suite(&state, &agent, &checked).await);
    let mut tx = or_return!(begin_write(&state, &user, id).await);
    let current = or_return!(agents_db::get_in(&mut tx, id).await.map_err(internal));
    let grants = or_return!(sp_db::grants_in(&mut tx, id).await.map_err(internal));
    if current
        .as_ref()
        .is_none_or(|current| current.draft_spec != agent.draft_spec)
        || grants != validated_grants
    {
        return json_error(
            StatusCode::CONFLICT,
            "conflict",
            &t(lang, "agent-configuration-error-conflict"),
        );
    }
    let result = agents_db::publish_in(&mut tx, id, &agent.draft_spec, &user.id).await;
    if result.is_ok() {
        or_return!(tx.commit().await.map_err(internal));
    }
    match result {
        Ok(Some(version)) => json_ok(
            StatusCode::CREATED,
            Published {
                version,
                live_version: version,
            },
        ),
        Ok(None) => not_found("the agent was deleted while it was being published"),
        Err(err) => internal(err),
    }
}

/// The version a publish created, which is now live.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct Published {
    pub version: i64,
    pub live_version: i64,
}

/// GET /api/v0/agents/{id}/versions — every published version, newest
/// first.
pub async fn versions(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let user = or_return!(require_agent_manager(&state, &req).await);
    let (agent, _) = or_return!(agent_at(&state, &req, &user, 1, Access::Read).await);
    match agents_db::versions(&state.db, &agent.principal.id).await {
        Ok(rows) => json_ok(
            StatusCode::OK,
            VersionList {
                live_version: agent.live_version,
                versions: rows.iter().map(version_view).collect(),
            },
        ),
        Err(err) => internal(err),
    }
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct VersionList {
    pub live_version: Option<i64>,
    /// Newest first.
    pub versions: Vec<VersionView>,
}

const MAX_ANALYTICS_DAYS: i64 = 366;

/// An analytics bound: an RFC 3339 instant, or a `YYYY-MM-DD` day in UTC. A
/// day as `to` means the end of that day, so `from=2026-10-01&to=2026-10-07`
/// covers seven whole days.
pub(super) fn analytics_bound(
    name: &str,
    value: &str,
    end_of_day: bool,
) -> Result<jiff::Timestamp, Response> {
    use jiff::ToSpan;
    if let Ok(at) = value.parse::<jiff::Timestamp>() {
        return Ok(at);
    }
    let day = value.parse::<jiff::civil::Date>().map_err(|_| {
        bad_request(format!(
            "`{name}` is `{value}`, which is neither an RFC 3339 time nor a YYYY-MM-DD day — \
             write it like `2026-10-01` or `2026-10-01T00:00:00Z`"
        ))
    })?;
    let start = day
        .to_zoned(jiff::tz::TimeZone::UTC)
        .map_err(|err| bad_request(format!("`{name}` is out of range: {err}")))?
        .timestamp();
    Ok(if end_of_day {
        start + 24.hours()
    } else {
        start
    })
}

/// GET /api/v0/agents/{id}/analytics?from=&to=&version= — counts of what the
/// agent did in the range (default: the last 30 days), without any visitor
/// content. `version` narrows to one published version; builder test
/// conversations are never counted.
pub async fn analytics(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    use jiff::ToSpan;
    let user = or_return!(require_agent_manager(&state, &req).await);
    let (agent, _) = or_return!(agent_at(&state, &req, &user, 1, Access::Read).await);
    let query = super::query_map(&req);
    let to = match query.get("to") {
        Some(v) => or_return!(analytics_bound("to", v, true)),
        None => jiff::Timestamp::now(),
    };
    let from = match query.get("from") {
        Some(v) => or_return!(analytics_bound("from", v, false)),
        None => to - (30 * 24).hours(),
    };
    if from >= to {
        return bad_request("`from` must be before `to` — swap them or widen the range");
    }
    if to.duration_since(from).as_secs() > MAX_ANALYTICS_DAYS * 86_400 {
        return bad_request(format!(
            "the range is longer than {MAX_ANALYTICS_DAYS} days — ask for a shorter one"
        ));
    }
    let version = match query.get("version") {
        None => None,
        Some(v) => match v.parse::<i64>() {
            Ok(n) if n >= 1 => Some(n),
            _ => {
                return bad_request(format!(
                    "`version` is `{v}`; it must be a published version number, 1 or more — list \
                     them with GET /api/v0/agents/{}/versions",
                    agent.principal.id
                ));
            }
        },
    };
    match agent_analytics::compute(
        &state.db,
        &agent.principal.id,
        agent_analytics::Range { from, to },
        version,
    )
    .await
    {
        Ok(analytics) => json_ok(
            StatusCode::OK,
            AnalyticsView {
                analytics,
                currency: state.config().usage.currency.clone(),
            },
        ),
        Err(err) => internal(err),
    }
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct AnalyticsView {
    #[serde(flatten)]
    pub analytics: agent_analytics::Analytics,
    /// The currency `usage.cost` and `daily[].cost` are in.
    pub currency: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct LiveBody {
    pub version: i64,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct LiveVersion {
    pub live_version: i64,
}

/// POST /api/v0/agents/{id}/live — make an existing version live: a
/// rollback, or forward again. Not re-validated: grants are not versioned,
/// and a reference whose grant was revoked meets default-deny at run time.
pub async fn set_live(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let user = or_return!(require_agent_manager(&state, &req).await);
    let (agent, _) = or_return!(agent_at(&state, &req, &user, 1, Access::Write).await);
    let id = agent.principal.id;
    let body: LiveBody =
        or_return!(super::read_json(req.into_body(), "the live-version body").await);
    let mut tx = or_return!(begin_write(&state, &user, &id).await);
    let result = agents_db::set_live_in(&mut tx, &id, body.version, &user.id).await;
    if result.is_ok() {
        or_return!(tx.commit().await.map_err(internal));
    }
    match result {
        Ok(true) => json_ok(
            StatusCode::OK,
            LiveVersion {
                live_version: body.version,
            },
        ),
        Ok(false) => not_found(format!(
            "`{}` has no version {} — list them with GET /api/v0/agents/{id}/versions",
            agent.principal.name, body.version
        )),
        Err(err) => internal(err),
    }
}

/// GET /api/v0/agents/{id}/shares
pub async fn shares(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let user = or_return!(require_agent_manager(&state, &req).await);
    let (agent, _) = or_return!(agent_at(&state, &req, &user, 1, Access::Read).await);
    let rows = match agents_db::shares(&state.db, &agent.principal.id).await {
        Ok(rows) => rows,
        Err(err) => return internal(err),
    };
    let shares = or_return!(share_views(&state, &rows).await);
    json_ok(StatusCode::OK, ShareList { shares })
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ShareList {
    pub shares: Vec<ShareView>,
}

/// The shortest query a share-subject search answers.
const SUBJECT_QUERY_MIN_CHARS: usize = 2;
/// The most users, and the most groups, one search returns.
const SUBJECT_MATCHES: usize = 8;

/// GET /api/v0/agents/{id}/share-subjects?q= — the users and groups matching
/// `q`, for whoever may change the agent's shares. A search, never a roster:
/// nothing for a query shorter than [`SUBJECT_QUERY_MIN_CHARS`], at most
/// [`SUBJECT_MATCHES`] of each, a user as id and display name, their address
/// only when `q` is exactly it. Whether a subject may hold `read` or `write`
/// is not shown; the share route says so when it may not.
pub async fn share_subjects(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let user = or_return!(require_agent_manager(&state, &req).await);
    let (agent, _) = or_return!(agent_at(&state, &req, &user, 1, Access::Write).await);
    let q = super::query_map(&req)
        .get("q")
        .map(|q| q.trim().to_owned())
        .unwrap_or_default();
    let mut tx = or_return!(begin_write(&state, &user, &agent.principal.id).await);
    let policy = state.rbac.with_snapshot(or_return!(
        aiplane_core::server::db::gateway_groups::load_snapshot_in(&mut tx)
            .await
            .map_err(internal)
    ));
    let owner = or_return!(
        agents_db::agent_owner_in(&mut tx, &agent.principal.id)
            .await
            .map_err(internal)
    );
    let org = match owner {
        Some(agents_db::AgentOwner::Organization(id)) => Some(id),
        _ => None,
    };
    let mut response = ShareSubjects {
        users: Vec::new(),
        groups: Vec::new(),
        truncated: false,
    };
    if q.chars().count() >= SUBJECT_QUERY_MIN_CHARS {
        let mut offset = 0;
        for _ in 0..20 {
            let limit = if org.is_some() {
                50
            } else {
                SUBJECT_MATCHES as u32
            };
            let page = or_return!(
                users::search_page_in(&mut tx, &q, limit, offset)
                    .await
                    .map_err(internal)
            );
            for user in page.users {
                if let Some(id) = &org {
                    let eligible = or_return!(
                        aiplane_agents::db::organizations::is_member_in(
                            &mut tx,
                            id,
                            &policy.role_ids_for(&user.roles)
                        )
                        .await
                        .map_err(internal)
                    );
                    if !eligible {
                        continue;
                    }
                }
                response.users.push(SubjectUser {
                    email: user.email.eq_ignore_ascii_case(&q).then_some(user.email),
                    id: user.id,
                    name: user.name,
                });
                if response.users.len() == SUBJECT_MATCHES {
                    break;
                }
            }
            if org.is_none() || response.users.len() == SUBJECT_MATCHES {
                break;
            }
            let Some(next) = page.next_offset else {
                break;
            };
            offset = next;
            if offset >= 1000 {
                response.truncated = true;
                break;
            }
        }
        let group_names = if let Some(id) = &org {
            or_return!(
                aiplane_agents::db::organizations::get_admin_in(&mut tx, id)
                    .await
                    .map_err(internal)
            )
            .map(|org| org.member_groups)
            .unwrap_or_default()
        } else {
            policy.group_names()
        };
        let needle = q.to_lowercase();
        response.groups = group_names
            .into_iter()
            .filter(|name| name.to_lowercase().contains(&needle))
            .take(SUBJECT_MATCHES)
            .collect();
    }
    or_return!(tx.commit().await.map_err(internal));
    json_ok(StatusCode::OK, response)
}

/// Users and groups matching a share-subject search.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ShareSubjects {
    pub users: Vec<SubjectUser>,
    pub groups: Vec<String>,
    pub truncated: bool,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct SubjectUser {
    pub id: String,
    pub name: Option<String>,
    /// Present only when the query is exactly this address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[schemars(rename = "AgentShareBody")]
pub struct ShareBody {
    /// `user` or `group`.
    pub subject_kind: String,
    pub subject_id: String,
    /// `respond`, `read` or `write`; required when adding a share.
    #[serde(default)]
    pub access: Option<String>,
}

fn parse_subject(body: &ShareBody) -> Result<(SubjectKind, &str), Response> {
    let kind = SubjectKind::parse(&body.subject_kind).ok_or_else(|| {
        bad_request(format!(
            "`{}` is not a share subject — use `user` or `group`",
            body.subject_kind
        ))
    })?;
    let subject = body.subject_id.trim();
    if subject.is_empty() {
        return Err(bad_request("a share needs a `subject_id`"));
    }
    Ok((kind, subject))
}

/// A share names a user or a group that exists. A `read` or `write` share only
/// takes effect for a holder of the agent-management permission, so one for
/// anybody else is refused rather than stored inert.
async fn require_share_subject(
    state: &RamaState,
    kind: SubjectKind,
    subject: &str,
    access: Access,
) -> Result<(), Response> {
    let holds = match kind {
        SubjectKind::User => {
            let Some(u) = users::find_by_id(&state.db, subject)
                .await
                .map_err(internal)?
            else {
                return Err(not_found(format!("there is no user `{subject}`")));
            };
            state.rbac.can_manage_agents(&group_ids(state, &u))
        }
        SubjectKind::Group => {
            if !state.rbac.has_group(subject) {
                return Err(not_found(format!("there is no group `{subject}`")));
            }
            state.rbac.can_manage_agents(&[subject.to_string()])
        }
    };
    if holds || !access.needs_agent_manager() {
        return Ok(());
    }
    Err(json_error(
        StatusCode::UNPROCESSABLE_ENTITY,
        "share_needs_agent_manager",
        &format!(
            "cannot share with {} `{subject}`: a `read` or `write` share only works for \
             holders of the agent-management permission, because it shows the spec and the \
             agent's conversations. Ask an admin to enable `can_manage_agents` on {}, or \
             give a `respond` share to let them answer the agent's inbox only.",
            kind.as_str(),
            match kind {
                SubjectKind::User => "one of their groups",
                SubjectKind::Group => "that group",
            }
        ),
    ))
}

async fn require_share_subject_in(
    state: &RamaState,
    tx: &mut aiplane_core::server::db::WriteTx,
    agent_id: &str,
    kind: SubjectKind,
    subject: &str,
    access: Access,
    lang: Lang,
) -> Result<(), Response> {
    let policy = state.rbac.with_snapshot(
        aiplane_core::server::db::gateway_groups::load_snapshot_in(tx)
            .await
            .map_err(internal)?,
    );
    let owner = agents_db::agent_owner_in(tx, agent_id)
        .await
        .map_err(internal)?;
    let org = match owner {
        Some(agents_db::AgentOwner::Organization(id)) => Some(id),
        Some(agents_db::AgentOwner::User(_)) | None => None,
    };
    let held = match kind {
        SubjectKind::User => {
            let user = users::find_by_id_in(tx, subject)
                .await
                .map_err(internal)?
                .ok_or_else(|| not_found(format!("there is no user `{subject}`")))?;
            let groups = policy.role_ids_for(&user.roles);
            if let Some(id) = &org
                && !aiplane_agents::db::organizations::is_member_in(tx, id, &groups)
                    .await
                    .map_err(internal)?
            {
                return Err(not_found(t(lang, "organization-error-not-found")));
            }
            policy.can_manage_agents(&groups)
        }
        SubjectKind::Group => {
            if !policy.has_group(subject) {
                return Err(not_found(format!("there is no group `{subject}`")));
            }
            if let Some(id) = &org {
                let row = aiplane_agents::db::organizations::get_admin_in(tx, id)
                    .await
                    .map_err(internal)?;
                if !row.is_some_and(|row| row.member_groups.iter().any(|group| group == subject)) {
                    return Err(not_found(t(lang, "organization-error-not-found")));
                }
            }
            policy.can_manage_agents(&[subject.to_owned()])
        }
    };
    if held || !access.needs_agent_manager() {
        Ok(())
    } else {
        Err(json_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "share_needs_agent_manager",
            &t(lang, "organization-error-permission-required"),
        ))
    }
}

fn last_writer() -> Response {
    json_error(
        StatusCode::CONFLICT,
        "last_writer",
        "this is the agent's last `write` share — give someone else `write` first, or the agent \
         could never be edited or shared again",
    )
}

/// The organization agent behind the path, if the caller is a current member
/// holding at least `need` on it: personal agents and non-members answer 404.
async fn organization_agent_at(
    state: &RamaState,
    req: &Request,
    user: &users::User,
    need: Access,
) -> Result<Option<(agents_db::AgentRow, agents_db::AgentMetadata)>, Response> {
    let Some(id) = raw_path_segment(req, 1) else {
        return Err(bad_request(t(
            Lang::from_request(req.headers()),
            "agent-error-id-missing",
        )));
    };
    let Some(metadata) = agents_db::get_agent_metadata(&state.db, &id)
        .await
        .map_err(internal)?
    else {
        return Ok(None);
    };
    if !matches!(metadata.owner, agents_db::AgentOwner::Organization(_)) {
        return Ok(None);
    }
    let Some(agent) = agents_db::get(&state.db, &id).await.map_err(internal)? else {
        return Ok(None);
    };
    let held = aiplane_runtime::agents::access::organization_access(state, &id, &user.id)
        .await
        .map_err(internal)?;
    match held {
        Some(a) if a >= need => Ok(Some((agent, metadata))),
        _ => Ok(None),
    }
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct AgentCommunicationOwner {
    pub id: String,
    pub name: Option<String>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct AgentCommunicationCaller {
    pub agent: super::json_owned_agents::OwnedAgentView,
    pub owner: AgentCommunicationOwner,
    pub incoming: super::json_owned_agents::AgentConsentView,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct AgentCommunicationView {
    pub agent: super::json_owned_agents::OwnedAgentView,
    pub callers: Vec<AgentCommunicationCaller>,
}

fn organization_communication_view(
    agent: &agents_db::AgentRow,
    metadata: &agents_db::AgentMetadata,
    callers: Vec<(
        agent_delegation::InboundCaller,
        agent_delegation::AgentDelegationEdge,
    )>,
) -> AgentCommunicationView {
    let p = &agent.principal;
    AgentCommunicationView {
        agent: super::json_owned_agents::OwnedAgentView {
            id: p.id.clone(),
            name: metadata.name.clone(),
            display: p.display.clone(),
            description: p.description.clone(),
            active: metadata.active,
        },
        callers: callers
            .into_iter()
            .map(|(caller, edge)| AgentCommunicationCaller {
                agent: super::json_owned_agents::OwnedAgentView {
                    id: caller.caller_agent_id.clone(),
                    name: caller.agent_name,
                    display: String::new(),
                    description: String::new(),
                    active: caller.active,
                },
                owner: AgentCommunicationOwner {
                    id: caller.owner_user_id,
                    name: caller.owner_name,
                },
                incoming: super::json_owned_agents::AgentConsentView::from_edge(Some(&edge)),
            })
            .collect(),
    }
}

async fn organization_communication(
    state: &RamaState,
    req: &Request,
    user: &users::User,
    need: Access,
) -> Result<AgentCommunicationView, Response> {
    let Some((agent, metadata)) = organization_agent_at(state, req, user, need).await? else {
        return Err(not_found(t(
            Lang::from_request(req.headers()),
            "organization-error-not-found",
        )));
    };
    let callers = agent_delegation::inbound_callers_for(&state.db, &agent.principal.id)
        .await
        .map_err(internal)?;
    Ok(organization_communication_view(&agent, &metadata, callers))
}

/// What `/api/v0/agents/{id}/communication` answers: a personal agent's
/// consent with its peers, or the callers a team agent accepts.
#[derive(serde::Serialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum CommunicationReply {
    Personal(super::json_owned_agents::OwnedAgentCommunicationView),
    Team(AgentCommunicationView),
}

/// One consent change: a personal agent's side of an edge, or a team agent's
/// inbound side.
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum CommunicationChange {
    Personal(super::json_owned_agents::OwnedAgentCommunicationBody),
    Team(AgentCommunicationBody),
}

/// Whether `/api/v0/agents/{id}/…` names a personal agent: the routes that
/// serve both kinds answer a personal agent's owner differently.
async fn addresses_personal_agent(state: &RamaState, req: &Request) -> Result<bool, Response> {
    let Some(id) = raw_path_segment(req, 1) else {
        let lang = Lang::from_request(req.headers());
        return Err(bad_request(t(lang, "agent-error-id-missing")));
    };
    is_personal(state, &id).await
}

/// GET /api/v0/agents/{id}/communication — a personal agent's consent with
/// its peers for its owner; for a team agent, the personal callers it accepts
/// requests from.
pub async fn communication_get(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    if or_return!(addresses_personal_agent(&state, &req).await) {
        super::json_owned_agents::communication_get(State(state), req).await
    } else {
        organization_communication_get(State(state), req).await
    }
}

/// PUT /api/v0/agents/{id}/communication — one consent change, on a personal
/// agent's side or a team agent's inbound side.
pub async fn communication_put(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    if or_return!(addresses_personal_agent(&state, &req).await) {
        super::json_owned_agents::communication_put(State(state), req).await
    } else {
        organization_communication_put(State(state), req).await
    }
}

/// GET /api/v0/agents/{id}/activity — a personal agent's runs for its owner;
/// any other agent's decision trail for those who may read it.
pub async fn activity(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    if or_return!(addresses_personal_agent(&state, &req).await) {
        super::json_owned_agents::activity(State(state), req).await
    } else {
        super::json_agent_activity::list(State(state), req).await
    }
}

/// The personal caller agents on the organization agent's cross-boundary
/// edges with their current inbound state, for the member writers' "Accepts
/// requests from" section.
async fn organization_communication_get(
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let user = match require_agent_manager(&state, &req).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    match organization_communication(&state, &req, &user, Access::Read).await {
        Ok(view) => json_ok(StatusCode::OK, view),
        Err(response) => response,
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentCommunicationBody {
    pub peer_id: String,
    pub enabled: bool,
    pub expected_generation: Option<String>,
}

/// A member writer granting or revoking `inbound` on a personal caller agent's cross-boundary edge. A
/// revocation that changes the generation cancels the in-flight delegations on
/// it, since the consent is their live recheck.
async fn organization_communication_put(
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let lang = Lang::from_request(req.headers());
    let user = match require_agent_manager(&state, &req).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    let (agent, metadata) = match organization_agent_at(&state, &req, &user, Access::Write).await {
        Ok(Some(value)) => value,
        Ok(None) => return not_found(t(lang, "organization-error-not-found")),
        Err(response) => return response,
    };
    let body: AgentCommunicationBody = match super::read_json_capped(
        req.into_body(),
        "the communication setting",
        16 * 1024,
    )
    .await
    {
        Ok(body) => body,
        Err(response) => return response,
    };
    let caller_owner = match agents_db::personal_agent_owner(&state.db, &body.peer_id)
        .await
        .map_err(internal)
    {
        Ok(Some(owner)) => owner,
        Ok(None) => return not_found(t(lang, "agent-error-not-found")),
        Err(response) => return response,
    };
    match agent_delegation::set_cross_boundary_consent(
        &state.db,
        agent_delegation::CrossBoundaryConsentEdit {
            actor_user_id: &user.id,
            caller_agent_id: &body.peer_id,
            receiver_agent_id: &agent.principal.id,
            direction: agent_delegation::ConsentDirection::Inbound,
            enabled: body.enabled,
            expected_generation: body.expected_generation.as_deref(),
        },
    )
    .await
    {
        Ok(ConsentSaved::Saved(edge)) => {
            if let Some(previous) = body.expected_generation.as_deref()
                && previous != edge.generation
                && let Err(error) = aiplane_runtime::agents::resume::cancel_personal_runs_for_edge(
                    &state,
                    &caller_owner,
                    &edge.caller_agent_id,
                    &edge.receiver_agent_id,
                    previous,
                )
                .await
            {
                return internal(error);
            }
            let callers =
                match agent_delegation::inbound_callers_for(&state.db, &agent.principal.id).await {
                    Ok(callers) => callers,
                    Err(error) => return internal(error),
                };
            json_ok(
                StatusCode::OK,
                organization_communication_view(&agent, &metadata, callers),
            )
        }
        Ok(ConsentSaved::Missing) => not_found(t(lang, "agent-error-not-found")),
        Ok(ConsentSaved::Invalid) => json_error(
            StatusCode::BAD_REQUEST,
            "invalid_consent",
            &t(lang, "agent-communication-error-invalid"),
        ),
        Ok(ConsentSaved::Conflict) => json_error(
            StatusCode::CONFLICT,
            "conflict",
            &t(lang, "agent-communication-error-conflict"),
        ),
        Ok(ConsentSaved::Unavailable) => json_error(
            StatusCode::CONFLICT,
            "inactive_agent",
            &t(lang, "agent-communication-error-inactive"),
        ),
        Err(error) => internal(error),
    }
}

/// POST /api/v0/agents/{id}/shares — add a share or change its access.
pub async fn share(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let lang = Lang::from_request(req.headers());
    let user = or_return!(require_agent_manager(&state, &req).await);
    let (agent, _) = or_return!(agent_at(&state, &req, &user, 1, Access::Write).await);
    let body: ShareBody = or_return!(super::read_json(req.into_body(), "the share body").await);
    let (kind, subject) = or_return!(parse_subject(&body));
    let access = match body.access.as_deref().map(Access::parse) {
        Some(Some(a)) => a,
        _ => return bad_request("a share needs `access`: `respond`, `read` or `write`"),
    };
    or_return!(require_share_subject(&state, kind, subject, access).await);
    let mut tx = or_return!(begin_write(&state, &user, &agent.principal.id).await);
    or_return!(
        require_share_subject_in(
            &state,
            &mut tx,
            &agent.principal.id,
            kind,
            subject,
            access,
            lang
        )
        .await
    );
    let changed = agents_db::set_share_in(
        &mut tx,
        &agent.principal.id,
        kind,
        subject,
        access,
        &user.id,
    )
    .await;
    if changed.is_ok() {
        or_return!(tx.commit().await.map_err(internal));
    }
    state.grant_caps.invalidate();
    match changed {
        Ok(ShareChange::LastWriter) => last_writer(),
        Ok(change) => json_ok(
            if change == ShareChange::Changed {
                StatusCode::CREATED
            } else {
                StatusCode::OK
            },
            ShareSet {
                subject_kind: kind.as_str(),
                subject_id: subject.to_string(),
                access: access.as_str(),
            },
        ),
        Err(err) => internal(err),
    }
}

/// The share as it now stands.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ShareSet {
    pub subject_kind: &'static str,
    pub subject_id: String,
    pub access: &'static str,
}

/// POST /api/v0/agents/{id}/shares/revoke — remove one share.
pub async fn revoke_share(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let user = or_return!(require_agent_manager(&state, &req).await);
    let (agent, _) = or_return!(agent_at(&state, &req, &user, 2, Access::Write).await);
    let body: ShareBody = or_return!(super::read_json(req.into_body(), "the share body").await);
    let (kind, subject) = or_return!(parse_subject(&body));
    let mut tx = or_return!(begin_write(&state, &user, &agent.principal.id).await);
    let removed =
        agents_db::remove_share_in(&mut tx, &agent.principal.id, kind, subject, &user.id).await;
    if removed.is_ok() {
        or_return!(tx.commit().await.map_err(internal));
    }
    state.grant_caps.invalidate();
    match removed {
        Ok(ShareChange::LastWriter) => last_writer(),
        Ok(ShareChange::NotFound) => not_found(format!(
            "`{}` is not shared with {} `{subject}`",
            agent.principal.name,
            kind.as_str()
        )),
        Ok(_) => no_content(),
        Err(err) => internal(err),
    }
}

/// DELETE /api/v0/agents/{id} — the agent and its principal, with every
/// grant, token, version and share. The audit trail stays.
pub async fn delete(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let Some(id) = raw_path_segment(&req, 0) else {
        return bad_request("the URL is missing the agent id");
    };
    let personal = or_return!(
        agents_db::personal_agent_owner(&state.db, &id)
            .await
            .map_err(internal)
    );
    let (user, agent, mut tx) = match personal {
        Some(owner) => {
            let user = or_return!(require_personal_deleter(&state, &req, &owner, &id).await);
            let Some(agent) = or_return!(agents_db::get(&state.db, &id).await.map_err(internal))
            else {
                return not_found(format!("there is no agent `{id}`"));
            };
            if let Err(error) =
                aiplane_runtime::agents::resume::cancel_personal_runs_for_agent(&state, &owner, &id)
                    .await
            {
                return internal(error);
            }
            let tx = or_return!(
                aiplane_core::server::db::WriteTx::begin(&state.db)
                    .await
                    .map_err(internal)
            );
            (user, agent, tx)
        }
        None => {
            let user = or_return!(require_agent_manager(&state, &req).await);
            let (agent, _) = or_return!(agent_by_id(&state, &user, &id, Access::Write).await);
            let tx = or_return!(begin_write(&state, &user, &agent.principal.id).await);
            (user, agent, tx)
        }
    };
    let schedules = or_return!(
        aiplane_runtime::server::scheduled::list_for_owner_in(
            &mut tx,
            &aiplane_runtime::server::background::AutomationOwner::Principal(
                agent.principal.id.clone()
            ),
        )
        .await
        .map_err(internal)
    );
    let webhooks = or_return!(
        aiplane_runtime::server::webhooks::list_for_owner_in(
            &mut tx,
            &aiplane_runtime::server::background::AutomationOwner::Principal(
                agent.principal.id.clone()
            )
        )
        .await
        .map_err(internal)
    );
    if !schedules.is_empty() || !webhooks.is_empty() {
        let lang = Lang::from_request(req.headers());
        return json_error(
            StatusCode::CONFLICT,
            "identity_retained",
            &t(lang, "scheduled-error-identity-retained"),
        );
    }
    let result = agents_db::delete_in(&mut tx, &agent.principal.id, &user.id).await;
    if let Ok(changed) = &result {
        or_return!(tx.commit().await.map_err(internal));
        if *changed {
            aiplane_agents::db::embed_keys::origins_changed();
        }
    }
    match result {
        Ok(_) => no_content(),
        Err(err) => internal(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_not_managed_message_is_one_line() {
        assert_eq!(
            not_managed_message("p1"),
            "there is no system principal `p1` you manage — only its creator or an admin can see or change it"
        );
    }

    fn body(kind: &str, id: &str) -> ShareBody {
        ShareBody {
            subject_kind: kind.into(),
            subject_id: id.into(),
            access: None,
        }
    }

    #[test]
    fn a_share_subject_is_a_user_or_a_group_with_an_id() {
        assert_eq!(
            parse_subject(&body("user", " bob ")).unwrap(),
            (SubjectKind::User, "bob")
        );
        assert_eq!(
            parse_subject(&body("group", "support")).unwrap(),
            (SubjectKind::Group, "support")
        );
        assert!(parse_subject(&body("role", "x")).is_err());
        assert!(parse_subject(&body("user", "  ")).is_err());
    }
}
