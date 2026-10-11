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
use aiplane_agents::db::agents::{
    self as agents_db, Access, DraftChange, ShareChange, SubjectKind,
};
use aiplane_agents::db::{agent_analytics, agent_audit, system_principals as sp_db};
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
    let held = effective_access(state, id, &user.id, &group_ids(state, user))
        .await
        .map_err(internal)?;
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
}

pub(super) fn agent_view(a: &agents_db::AgentRow, access: Access) -> AgentView {
    let p = &a.principal;
    AgentView {
        id: p.id.clone(),
        name: p.name.clone(),
        display: p.display.clone(),
        description: p.description.clone(),
        created_by: p.created_by.clone(),
        created_at: a.created_at,
        updated_at: a.updated_at,
        disabled_at: p.disabled_at,
        live_version: a.live_version,
        access: access.as_str(),
    }
}

pub(super) fn agent_json(a: &agents_db::AgentRow, access: Access) -> Value {
    json!(agent_view(a, access))
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
}

/// The shares as the API shows them: a person's share carries their display
/// name, so the people an agent is shared with are recognisable by whoever
/// may see its shares.
async fn share_views(
    state: &RamaState,
    rows: &[agents_db::ShareRow],
) -> Result<Vec<ShareView>, Response> {
    let mut out = Vec::with_capacity(rows.len());
    for s in rows {
        let name = if s.subject_kind == SubjectKind::User {
            users::find_by_id(&state.db, &s.subject_id)
                .await
                .map_err(internal)?
                .map(|u| u.name)
        } else {
            None
        };
        out.push(ShareView {
            subject_kind: s.subject_kind.as_str(),
            subject_id: s.subject_id.clone(),
            access: s.access.as_str(),
            name,
        });
    }
    Ok(out)
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

fn version_view(v: &agents_db::VersionRow) -> VersionView {
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

/// GET /api/v0/agents — the agents shared with the caller; every agent for
/// an admin.
pub async fn list(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let user = or_return!(require_agent_manager(&state, &req).await);
    let rows = or_return!(visible_agents(&state, &user).await);
    let agents = rows.iter().map(|(a, acc)| agent_view(a, *acc)).collect();
    json_ok(StatusCode::OK, AgentList { agents })
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct AgentList {
    pub agents: Vec<AgentView>,
}

/// The agents shared with `user`, with their access; every agent for an
/// admin.
pub(super) async fn visible_agents(
    state: &RamaState,
    user: &users::User,
) -> Result<Vec<(agents_db::AgentRow, Access)>, Response> {
    let groups = group_ids(state, user);
    if state.rbac.is_admin(&groups) {
        agents_db::list_all(&state.db)
            .await
            .map(|rows| rows.into_iter().map(|a| (a, Access::Write)).collect())
    } else {
        agents_db::list_shared_with(&state.db, &user.id, &groups).await
    }
    .map_err(internal)
}

#[derive(Deserialize, schemars::JsonSchema)]
#[schemars(rename = "AgentCreateBody")]
pub struct CreateBody {
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

/// POST /api/v0/agents — an agent with a fresh principal (no grants) and a
/// `write` share for its creator.
pub async fn create(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let user = or_return!(require_agent_manager(&state, &req).await);
    let body: CreateBody = or_return!(super::read_json(req.into_body(), "the agent body").await);
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
            agent: agent_view(&a, Access::Write),
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
                agent: agent_view(&agent, held),
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
    let user = or_return!(require_agent_manager(&state, &req).await);
    let (agent, _) = or_return!(agent_at(&state, &req, &user, 1, Access::Write).await);
    let body: DraftBody = or_return!(super::read_json(req.into_body(), "the draft body").await);
    match save_draft(&state, &user, &agent, body.spec, DraftChange::default()).await {
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
) -> Result<SavedDraft, Response> {
    let id = &agent.principal.id;
    require_valid(state, id, &spec, Stage::Draft, "save the draft").await?;
    seal_secrets(state, &mut spec)?;
    match agents_db::update_draft(&state.db, id, &spec.to_string(), &user.id, change).await {
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
    let saved = or_return!(save_draft(&state, &user, &agent, restored, change).await);
    for (kind, reference) in &revoking {
        if let Err(err) = sp_db::remove_grant(&state.db, id, *kind, reference, &user.id).await {
            return internal(err);
        }
    }
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
    let user = or_return!(require_agent_manager(&state, &req).await);
    let (agent, _) = or_return!(agent_at(&state, &req, &user, 1, Access::Write).await);
    let id = &agent.principal.id;
    let draft = parse_spec(&agent.draft_spec);
    let checked =
        or_return!(require_valid(&state, id, &draft, Stage::Publish, "publish the agent").await);
    or_return!(super::json_agent_tests::require_green_suite(&state, &agent, &checked).await);
    match agents_db::publish(&state.db, id, &agent.draft_spec, &user.id).await {
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
    match agents_db::set_live(&state.db, &id, body.version, &user.id).await {
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
    or_return!(agent_at(&state, &req, &user, 1, Access::Write).await);
    let q = super::query_map(&req)
        .get("q")
        .map(|q| q.trim().to_string())
        .unwrap_or_default();
    if q.chars().count() < SUBJECT_QUERY_MIN_CHARS {
        return json_ok(
            StatusCode::OK,
            ShareSubjects {
                users: Vec::new(),
                groups: Vec::new(),
            },
        );
    }
    let found = match users::search(&state.db, &q, SUBJECT_MATCHES as i64).await {
        Ok(found) => found,
        Err(err) => return internal(err),
    };
    let users = found
        .into_iter()
        .map(|u| SubjectUser {
            email: u.email.eq_ignore_ascii_case(&q).then_some(u.email),
            id: u.id,
            name: u.name,
        })
        .collect();
    let needle = q.to_lowercase();
    let groups: Vec<String> = state
        .rbac
        .group_names()
        .into_iter()
        .filter(|g| g.to_lowercase().contains(&needle))
        .take(SUBJECT_MATCHES)
        .collect();
    json_ok(StatusCode::OK, ShareSubjects { users, groups })
}

/// Users and groups matching a share-subject search.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ShareSubjects {
    pub users: Vec<SubjectUser>,
    pub groups: Vec<String>,
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

fn last_writer() -> Response {
    json_error(
        StatusCode::CONFLICT,
        "last_writer",
        "this is the agent's last `write` share — give someone else `write` first, or the agent \
         could never be edited or shared again",
    )
}

/// POST /api/v0/agents/{id}/shares — add a share or change its access.
pub async fn share(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let user = or_return!(require_agent_manager(&state, &req).await);
    let (agent, _) = or_return!(agent_at(&state, &req, &user, 1, Access::Write).await);
    let body: ShareBody = or_return!(super::read_json(req.into_body(), "the share body").await);
    let (kind, subject) = or_return!(parse_subject(&body));
    let access = match body.access.as_deref().map(Access::parse) {
        Some(Some(a)) => a,
        _ => return bad_request("a share needs `access`: `respond`, `read` or `write`"),
    };
    or_return!(require_share_subject(&state, kind, subject, access).await);
    let changed = agents_db::set_share(
        &state.db,
        &agent.principal.id,
        kind,
        subject,
        access,
        &user.id,
    )
    .await;
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
    let removed =
        agents_db::remove_share(&state.db, &agent.principal.id, kind, subject, &user.id).await;
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
    let user = or_return!(require_agent_manager(&state, &req).await);
    let (agent, _) = or_return!(agent_at(&state, &req, &user, 0, Access::Write).await);
    match agents_db::personal_dot_owner(&state.db, &agent.principal.id).await {
        Ok(Some(owner)) => {
            if let Err(error) = aiplane_runtime::agents::resume::cancel_personal_runs_for_dot(
                &state,
                &owner,
                &agent.principal.id,
            )
            .await
            {
                return internal(error);
            }
        }
        Ok(None) => {}
        Err(error) => return internal(error),
    }
    match agents_db::delete(&state.db, &agent.principal.id, &user.id).await {
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
