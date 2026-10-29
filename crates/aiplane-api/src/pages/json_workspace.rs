// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The user-facing `/api/v0` workspace surfaces for the SPA:
//! memories, scheduled actions, and webhooks. Thin JSON translations
//! of the legacy form handlers one layer down — the legacy pages stay
//! alive beside them until phase 6.

use std::sync::Arc;

use rama::http::service::web::extract::{Path, State};
use rama::http::{Request, Response, StatusCode};

use super::json_owned_agents::{self, OwnedAgentView};
use aiplane_agents::db::{WriteTx, agent_sessions, agents as owned_agents_db};
use aiplane_core::server::auth::token as auth_token;
use aiplane_core::server::db;
use aiplane_runtime::openai_driver;
use aiplane_runtime::rama_server::state::RamaState;
use aiplane_runtime::server::background::AutomationOwner;
use aiplane_runtime::server::scheduled::{self, cron::Cron};
use aiplane_runtime::server::webhooks;
use session_core::i18n::{Lang, t};

use super::{bad_request, internal, json_error, json_ok, not_found};

// ---------------------------------------------------------------------------
// Memories

/// GET /api/v0/memories — the caller's structured memories, and whether
/// their preferences reach the assistant's standing context.
pub async fn memories_list(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, user) = require_session_json!(state, req);
    let in_context = openai_driver::preferences_in_context(&state, &user.roles, &user.id).await;
    match db::user_memories::list_for_user(&state.db, &user.id, 500).await {
        Ok(rows) => json_ok(
            StatusCode::OK,
            MemoryList {
                memories: rows.iter().map(memory_json).collect(),
                preferences: PreferenceContext {
                    in_context,
                    max_count: openai_driver::PREFERENCE_FETCH_LIMIT,
                    char_budget: openai_driver::PREFERENCE_CHAR_BUDGET,
                },
            },
        ),
        Err(err) => internal(err),
    }
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct MemoryList {
    pub memories: Vec<MemoryView>,
    pub preferences: PreferenceContext,
}

/// Whether the caller's preferences ride in the standing context, and the
/// limits the chat driver applies when they do.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct PreferenceContext {
    /// `recall` is granted and memory switched on.
    pub in_context: bool,
    /// The newest this many preferences are considered.
    pub max_count: i64,
    /// Rendered until about this many characters; the newest always is.
    pub char_budget: usize,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct MemoryView {
    pub id: String,
    /// `preference`, `project` or `fact`.
    pub kind: &'static str,
    pub content: String,
    pub created_at: String,
}

fn memory_json(m: &db::user_memories::Memory) -> MemoryView {
    MemoryView {
        id: m.id.clone(),
        kind: m.kind.as_str(),
        content: m.content.clone(),
        created_at: m.created_at.to_string(),
    }
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct MemoryBody {
    /// `preference`, `project` or `fact`.
    pub kind: String,
    pub content: String,
}

fn parse_kind(kind: &str) -> Option<db::user_memories::MemoryKind> {
    match kind {
        "preference" => Some(db::user_memories::MemoryKind::Preference),
        "project" => Some(db::user_memories::MemoryKind::Project),
        "fact" => Some(db::user_memories::MemoryKind::Fact),
        _ => None,
    }
}

/// POST /api/v0/memories
pub async fn memories_create(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, user) = require_session_json!(state, req);
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return bad_request(msg),
    };
    let parsed: MemoryBody = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => return bad_request(format!("parsing the memory body: {err}")),
    };
    let Some(kind) = parse_kind(&parsed.kind) else {
        return bad_request(format!(
            "unknown memory kind: {} (preference | project | fact)",
            parsed.kind
        ));
    };
    let content = parsed.content.trim();
    if content.is_empty() {
        return bad_request("the memory content must not be empty");
    }
    match db::user_memories::insert(&state.db, &user.id, kind, content).await {
        Ok(m) => json_ok(StatusCode::CREATED, memory_json(&m)),
        Err(err) => internal(err),
    }
}

/// PUT /api/v0/memories/{id}
pub async fn memories_update(
    Path(id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = require_session_json!(state, req);
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return bad_request(msg),
    };
    let parsed: MemoryBody = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => return bad_request(format!("parsing the memory body: {err}")),
    };
    let Some(kind) = parse_kind(&parsed.kind) else {
        return bad_request(format!("unknown memory kind: {}", parsed.kind));
    };
    let content = parsed.content.trim();
    if content.is_empty() {
        return bad_request("the memory content must not be empty");
    }
    match db::user_memories::update(&state.db, &user.id, &id, kind, content).await {
        Ok(Some(m)) => json_ok(StatusCode::OK, memory_json(&m)),
        Ok(None) => json_error(StatusCode::NOT_FOUND, "not_found", "no such memory"),
        Err(err) => internal(err),
    }
}

/// DELETE /api/v0/memories/{id}
pub async fn memories_delete(
    Path(id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = require_session_json!(state, req);
    match db::user_memories::delete(&state.db, &user.id, &id).await {
        Ok(true) => Response::builder()
            .status(StatusCode::NO_CONTENT)
            .body(rama::http::Body::empty())
            .expect("static empty response"),
        Ok(false) => json_error(StatusCode::NOT_FOUND, "not_found", "no such memory"),
        Err(err) => internal(err),
    }
}

// ---------------------------------------------------------------------------
// Scheduled actions

/// Stamp `last_chat_deleted` on each row, so a list row can say its chat is
/// gone instead of linking into a 404. One query for the whole list.
///
/// Each row is its `last_session_id` and the `last_chat_deleted` flag to set.
async fn flag_deleted_last_chats_in<'a>(
    tx: &mut WriteTx,
    rows: impl IntoIterator<Item = (Option<&'a str>, &'a mut Option<bool>)>,
) -> Result<(), db::DbError> {
    let rows: Vec<_> = rows.into_iter().collect();
    let ids: Vec<&str> = rows.iter().filter_map(|(id, _)| *id).collect();
    let existing = session_core::db::existing_session_ids_in(tx, &ids).await?;
    for (id, flag) in rows {
        *flag = Some(id.is_some_and(|id| !existing.contains(id)));
    }
    Ok(())
}

/// One scheduled action as the list, create, update and runs routes return it.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ActionView {
    pub run_count: i64,
    pub chat_count: i64,
    pub id: String,
    pub personal_agent_id: Option<String>,
    pub principal_id: Option<String>,
    pub agent_version: Option<i64>,
    pub configuration_generation: String,
    pub agent: Option<OwnedAgentView>,
    pub name: String,
    pub prompt: String,
    pub model: Option<String>,
    /// 5-field cron expression.
    pub cron: String,
    /// The cron expression in words.
    pub schedule_summary: String,
    /// IANA timezone the cron expression is evaluated in.
    pub timezone: String,
    pub tools_enabled: bool,
    pub reuse_conversation: bool,
    pub reuse_rounds: i64,
    pub enabled: bool,
    pub next_run_at: Option<String>,
    pub last_run_at: Option<String>,
    pub last_session_id: Option<String>,
    pub last_status: Option<String>,
    pub last_error: Option<String>,
    /// Whether `last_session_id` names a chat that has since been deleted.
    /// Present on the list and update responses only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_chat_deleted: Option<bool>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ModelOption {
    pub id: String,
    /// The model's backend keeps data within GDPR safeguards.
    pub gdpr: bool,
    /// The model's backend is covered by a confidentiality agreement.
    pub nda: bool,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ScheduledList {
    pub principal_id: Option<String>,
    pub can_write: bool,
    pub versions: Vec<super::json_agents::VersionView>,
    pub agents: Vec<OwnedAgentView>,
    pub actions: Vec<ActionView>,
    /// The chat models an action can run on.
    pub models: Vec<ModelOption>,
    /// The caller's timezone, `UTC` when unset.
    pub default_timezone: Option<String>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ScheduledRuns {
    pub action: ActionView,
    /// The newest 50 runs.
    pub runs: Vec<ScheduledRunView>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ScheduledRunView {
    pub configuration_generation: Option<String>,
    pub agent_version: Option<i64>,
    pub id: String,
    pub fired_at: String,
    pub status: Option<String>,
    pub session_id: Option<String>,
    pub assistant_turn_id: Option<String>,
    pub chat_deleted: bool,
    pub error: Option<String>,
}

/// An update's answer: the saved row, or `{"ok": true}` when it could not be
/// read back.
#[derive(serde::Serialize, schemars::JsonSchema)]
#[serde(untagged)]
#[schemars(rename = "Updated{T}")]
pub enum Updated<T> {
    Row(Box<T>),
    Acknowledged(super::Done),
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct CronPreview {
    /// The cron expression in words.
    pub summary: String,
    /// The next three fire times.
    pub upcoming: Vec<String>,
}

fn action_json_with_counts(
    a: &scheduled::ScheduledAction,
    (run_count, chat_count): (i64, i64),
) -> ActionView {
    let schedule_summary = Cron::parse(&a.cron)
        .map(|cron| cron.describe())
        .unwrap_or_else(|_| format!("cron: {}", a.cron));
    ActionView {
        run_count,
        chat_count,
        id: a.id.clone(),
        personal_agent_id: a.personal_agent_id.clone(),
        principal_id: match &a.owner {
            aiplane_runtime::server::background::AutomationOwner::User(_) => None,
            aiplane_runtime::server::background::AutomationOwner::Principal(id) => Some(id.clone()),
        },
        agent_version: a.agent_version,
        configuration_generation: a.configuration_generation.clone(),
        agent: None,
        name: a.name.clone(),
        prompt: a.prompt.clone(),
        model: a.model.clone(),
        cron: a.cron.clone(),
        schedule_summary,
        timezone: a.timezone.clone(),
        tools_enabled: a.tools_enabled,
        reuse_conversation: a.reuse_conversation,
        reuse_rounds: a.reuse_rounds,
        enabled: a.enabled,
        next_run_at: a.next_run_at.map(|t| t.to_string()),
        last_run_at: a.last_run_at.map(|t| t.to_string()),
        last_session_id: a.last_session_id.clone(),
        last_status: a.last_status.clone(),
        last_error: a.last_error.clone(),
        last_chat_deleted: None,
    }
}

fn chat_model_options(state: &RamaState) -> Vec<ModelOption> {
    state
        .upstreams
        .models_with_compliance_for_kind(aiplane_core::server::upstreams::PoolKind::Chat)
        .into_iter()
        .map(|(id, compliance)| ModelOption {
            id,
            gdpr: compliance.gdpr,
            nda: compliance.nda,
        })
        .collect()
}

fn schedule_unavailable(lang: Lang) -> Response {
    not_found(t(lang, "scheduled-toast-not-found"))
}

fn invalid_schedule(lang: Lang) -> Response {
    bad_request(t(lang, "scheduled-error-invalid-configuration"))
}

fn schedule_conflict(lang: Lang) -> Response {
    json_error(
        StatusCode::CONFLICT,
        "configuration_conflict",
        &t(lang, "scheduled-error-configuration-conflict"),
    )
}

async fn schedule_access_in(
    state: &RamaState,
    tx: &mut WriteTx,
    user_id: &str,
    owner: &aiplane_runtime::server::background::AutomationOwner,
    lang: Lang,
) -> Result<(), Response> {
    let admitted = match owner {
        aiplane_runtime::server::background::AutomationOwner::User(id) => {
            id == user_id
                && aiplane_runtime::agents::access::current_user_policy_in(state, tx, user_id)
                    .await
                    .map_err(internal)?
                    .is_some()
        }
        aiplane_runtime::server::background::AutomationOwner::Principal(id) => {
            aiplane_runtime::agents::access::organization_access_in(state, tx, id, user_id)
                .await
                .map_err(internal)?
                == Some(owned_agents_db::Access::Write)
        }
    };
    if admitted {
        Ok(())
    } else {
        Err(schedule_unavailable(lang))
    }
}

async fn schedule_agent_in(
    tx: &mut WriteTx,
    owner: &aiplane_runtime::server::background::AutomationOwner,
    personal_agent_id: Option<&str>,
) -> Result<Option<OwnedAgentView>, db::DbError> {
    let id = match owner {
        aiplane_runtime::server::background::AutomationOwner::User(_) => personal_agent_id,
        aiplane_runtime::server::background::AutomationOwner::Principal(id) => Some(id.as_str()),
    };
    let Some(id) = id else { return Ok(None) };
    let Some(metadata) = owned_agents_db::get_agent_metadata_in(tx, id).await? else {
        return Ok(None);
    };
    let owner_matches = match owner {
        aiplane_runtime::server::background::AutomationOwner::User(user) => {
            metadata.owner == owned_agents_db::AgentOwner::User(user.clone())
        }
        aiplane_runtime::server::background::AutomationOwner::Principal(principal) => {
            id == principal
                && matches!(metadata.owner, owned_agents_db::AgentOwner::Organization(_))
        }
    };
    if !owner_matches {
        return Ok(None);
    };
    let Some(agent) = owned_agents_db::get_in(tx, id).await? else {
        return Ok(None);
    };
    Ok(Some(OwnedAgentView {
        id: id.into(),
        name: metadata.name,
        display: agent.principal.display,
        description: agent.principal.description,
        active: metadata.active,
    }))
}

async fn scheduled_view_in(
    tx: &mut WriteTx,
    action: &scheduled::ScheduledAction,
) -> Result<ActionView, db::DbError> {
    let counts = scheduled::run_counts_for_owner_in(tx, &action.owner).await?;
    let mut view =
        action_json_with_counts(action, counts.get(&action.id).copied().unwrap_or((0, 0)));
    view.agent = schedule_agent_in(tx, &action.owner, action.personal_agent_id.as_deref()).await?;
    flag_deleted_last_chats_in(
        tx,
        [(view.last_session_id.as_deref(), &mut view.last_chat_deleted)],
    )
    .await?;
    Ok(view)
}

async fn schedule_body_in(
    state: &RamaState,
    tx: &mut WriteTx,
    owner: &aiplane_runtime::server::background::AutomationOwner,
    parsed: &ScheduledBody,
    previous_agent: Option<&str>,
    lang: Lang,
) -> Result<Option<Option<String>>, Response> {
    if validate_scheduled(parsed).is_err() {
        return Err(invalid_schedule(lang));
    };
    match owner {
        aiplane_runtime::server::background::AutomationOwner::Principal(id) => {
            if parsed.principal_id.as_deref() != Some(id.as_str())
                || parsed.model.is_some()
                || parsed.personal_agent_id.is_some()
                || parsed.reuse_conversation
                || parsed.linked_session_id.is_some()
                || parsed.timezone.trim().is_empty()
                || parsed.agent_version.is_none_or(|v| v <= 0)
            {
                return Err(invalid_schedule(lang));
            }
            let _admitted = aiplane_runtime::agents::profile::organization_automation_version_in(
                state,
                tx,
                id,
                parsed.agent_version.expect("positive version checked"),
            )
            .await
            .map_err(|error| match error {
                aiplane_runtime::agents::profile::AgentRunError::Db(error) => internal(error),
                _ => invalid_schedule(lang),
            })?;
            Ok(Some(None))
        }
        aiplane_runtime::server::background::AutomationOwner::User(user) => {
            if parsed.principal_id.is_some()
                || parsed.agent_version.is_some()
                || parsed
                    .model
                    .as_deref()
                    .is_none_or(|model| model.trim().is_empty())
            {
                return Err(invalid_schedule(lang));
            };
            if !agent_sessions::binding_valid(
                tx,
                user,
                parsed.personal_agent_id.as_deref(),
                previous_agent,
            )
            .await
            .map_err(internal)?
            {
                return Err(bad_request(t(lang, "agent-background-error-binding")));
            };
            let Some(requested) = parsed.linked_session_id.as_deref().map(str::trim) else {
                return Ok(None);
            };
            if requested.is_empty() {
                return Ok(Some(None));
            };
            if !parsed.reuse_conversation
                || !agent_sessions::session_matches(
                    tx,
                    user,
                    requested,
                    parsed.personal_agent_id.as_deref(),
                )
                .await
                .map_err(internal)?
            {
                return Err(bad_request(t(lang, "agent-background-error-linked")));
            };
            Ok(Some(Some(requested.into())))
        }
    }
}

/// GET /api/v0/scheduled — the caller's actions.
pub async fn scheduled_list(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, user) = require_session_json!(state, req);
    let lang = Lang::from_request(req.headers());
    let principal_id = super::query_map(&req).get("principal_id").cloned();
    let owner = principal_id
        .as_ref()
        .map(|id| aiplane_runtime::server::background::AutomationOwner::Principal(id.clone()))
        .unwrap_or_else(|| {
            aiplane_runtime::server::background::AutomationOwner::User(user.id.clone())
        });
    let agents = if principal_id.is_none() {
        match owned_agents_db::list_personal_agents(&state.db, &user.id).await {
            Ok(rows) => rows.iter().map(json_owned_agents::view).collect(),
            Err(error) => return internal(error),
        }
    } else {
        Vec::new()
    };
    async {
        let mut tx = WriteTx::begin(&state.db).await.map_err(internal)?;
        schedule_access_in(&state, &mut tx, &user.id, &owner, lang).await?;
        let rows = scheduled::list_for_owner_in(&mut tx, &owner)
            .await
            .map_err(internal)?;
        let counts = scheduled::run_counts_for_owner_in(&mut tx, &owner)
            .await
            .map_err(internal)?;
        let (owned_agents, versions, models, default_timezone) = if let Some(id) = &principal_id {
            let owned_agent = schedule_agent_in(&mut tx, &owner, None)
                .await
                .map_err(internal)?
                .ok_or_else(|| schedule_unavailable(lang))?;
            let versions = owned_agents_db::versions_in(&mut tx, id)
                .await
                .map_err(internal)?
                .iter()
                .map(super::json_agents::version_view)
                .collect();
            (vec![owned_agent], versions, Vec::new(), None)
        } else {
            (
                agents,
                Vec::new(),
                chat_model_options(&state),
                Some(user.timezone.as_deref().unwrap_or("UTC").into()),
            )
        };
        let mut actions: Vec<ActionView> = rows
            .iter()
            .map(|action| {
                let mut view = action_json_with_counts(
                    action,
                    counts.get(&action.id).copied().unwrap_or((0, 0)),
                );
                view.agent = owned_agents
                    .iter()
                    .find(|owned_agent| {
                        principal_id.as_deref() == Some(owned_agent.id.as_str())
                            || action.personal_agent_id.as_deref() == Some(owned_agent.id.as_str())
                    })
                    .cloned();
                view
            })
            .collect();
        flag_deleted_last_chats_in(
            &mut tx,
            actions.iter_mut().map(|action| {
                (
                    action.last_session_id.as_deref(),
                    &mut action.last_chat_deleted,
                )
            }),
        )
        .await
        .map_err(internal)?;
        tx.commit().await.map_err(internal)?;
        Ok::<_, Response>(json_ok(
            StatusCode::OK,
            ScheduledList {
                principal_id,
                can_write: true,
                versions,
                agents: owned_agents,
                actions,
                models,
                default_timezone,
            },
        ))
    }
    .await
    .unwrap_or_else(|response| response)
}

/// GET /api/v0/scheduled/{id}/runs — one schedule's run history.
///
/// Owner-scoped through `scheduled::get` first: `list_runs` is keyed only by
/// action id, so reading it straight from the path would hand any signed-in
/// user another account's runs — and the chat session ids they opened — for
/// any id they can name. An id that is not the caller's reads as missing.
pub async fn scheduled_runs(
    Path(id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = require_session_json!(state, req);
    let lang = Lang::from_request(req.headers());
    async {
        let mut tx = WriteTx::begin(&state.db).await.map_err(internal)?;
        let action = scheduled::get_by_id_in(&mut tx, &id)
            .await
            .map_err(internal)?
            .ok_or_else(|| schedule_unavailable(lang))?;
        schedule_access_in(&state, &mut tx, &user.id, &action.owner, lang).await?;
        let action_view = scheduled_view_in(&mut tx, &action)
            .await
            .map_err(internal)?;
        let runs = scheduled::list_runs_in(&mut tx, &action.id, 50)
            .await
            .map_err(internal)?
            .iter()
            .map(scheduled_run_json)
            .collect();
        tx.commit().await.map_err(internal)?;
        Ok::<_, Response>(json_ok(
            StatusCode::OK,
            ScheduledRuns {
                action: action_view,
                runs,
            },
        ))
    }
    .await
    .unwrap_or_else(|response| response)
}

fn scheduled_run_json(r: &scheduled::ScheduledRun) -> ScheduledRunView {
    ScheduledRunView {
        configuration_generation: r.configuration_generation.clone(),
        agent_version: r.agent_version,
        id: r.id.clone(),
        fired_at: r.fired_at.to_string(),
        status: r.status.clone(),
        session_id: r.session_id.clone(),
        assistant_turn_id: r.assistant_turn_id.clone(),
        chat_deleted: r.chat_deleted,
        error: r.error.clone(),
    }
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct ScheduledBody {
    #[serde(default)]
    pub principal_id: Option<String>,
    #[serde(default)]
    pub agent_version: Option<i64>,
    #[serde(default)]
    pub personal_agent_id: Option<String>,
    pub name: String,
    pub prompt: String,
    #[serde(default)]
    pub model: Option<String>,
    pub cron: String,
    #[serde(default)]
    pub timezone: String,
    #[serde(default)]
    pub tools_enabled: bool,
    #[serde(default)]
    pub reuse_conversation: bool,
    #[serde(default)]
    pub reuse_rounds: i64,
    #[serde(default)]
    pub linked_session_id: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct ScheduledUpdateBody {
    #[serde(flatten)]
    pub action: ScheduledBody,
    pub expected_configuration_generation: String,
}

async fn compute_next(cron: &str, tz_name: &str) -> Result<Option<jiff::Timestamp>, String> {
    let parsed = Cron::parse(cron).map_err(|e| e.to_string())?;
    let tz =
        jiff::tz::TimeZone::get(tz_name).map_err(|_| format!("unknown timezone: {tz_name}"))?;
    Ok(parsed.next_after(jiff::Timestamp::now(), &tz))
}

/// POST /api/v0/scheduled
pub async fn scheduled_create(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, user) = require_session_json!(state, req);
    let lang = Lang::from_request(req.headers());
    let parsed: ScheduledBody =
        or_return!(super::read_json(req.into_body(), "the scheduled action").await);
    let owner = parsed
        .principal_id
        .as_ref()
        .map(|id| aiplane_runtime::server::background::AutomationOwner::Principal(id.clone()))
        .unwrap_or_else(|| {
            aiplane_runtime::server::background::AutomationOwner::User(user.id.clone())
        });
    async {
        let mut tx = WriteTx::begin(&state.db).await.map_err(internal)?;
        schedule_access_in(&state, &mut tx, &user.id, &owner, lang).await?;
        let link = schedule_body_in(&state, &mut tx, &owner, &parsed, None, lang).await?;
        let timezone = if parsed.timezone.trim().is_empty() {
            user.timezone.clone().unwrap_or_else(|| "UTC".into())
        } else {
            parsed.timezone.trim().into()
        };
        let next = compute_next(&parsed.cron, &timezone)
            .await
            .map_err(|_| invalid_schedule(lang))?;
        let action = scheduled::create_in(
            &mut tx,
            scheduled::NewAction {
                owner,
                created_by: user.id.clone(),
                personal_agent_id: parsed.personal_agent_id,
                linked_session_id: link.flatten(),
                name: parsed.name.trim().into(),
                prompt: parsed.prompt,
                model: parsed.model,
                agent_version: parsed.agent_version,
                cron: parsed.cron,
                timezone,
                tools_enabled: parsed.tools_enabled,
                reuse_conversation: parsed.reuse_conversation,
                reuse_rounds: parsed.reuse_rounds,
                next_run_at: next,
            },
        )
        .await
        .map_err(internal)?;
        let view = scheduled_view_in(&mut tx, &action)
            .await
            .map_err(internal)?;
        tx.commit().await.map_err(internal)?;
        Ok::<_, Response>(json_ok(StatusCode::CREATED, view))
    }
    .await
    .unwrap_or_else(|response| response)
}

/// What a body's `linked_session_id` asks for: `None` leaves the link alone,
/// `Some(None)` lets the next run open a fresh chat, `Some(Some(id))` continues
/// in chat `id`. Shared by schedules and webhooks, create and update.
///
/// The chat must be the caller's own: the link is where a run writes, so
/// naming someone else's chat would put a run's output into it.
/// The field rules both the create and the update path must apply.
///
/// One validator, called from both, because the split is how the update path
/// ended up with none: a `PUT` carrying empty strings left a live cron action
/// firing an empty prompt at an empty model id every minute. The caps match
/// the ones the form enforced.
fn validate_scheduled(parsed: &ScheduledBody) -> Result<(), String> {
    let name = parsed.name.trim();
    if name.is_empty() || name.len() > 128 {
        return Err("`name` must be 1..=128 characters".into());
    }
    let prompt = parsed.prompt.trim();
    if prompt.is_empty() || prompt.len() > 8000 {
        return Err("`prompt` must be 1..=8000 characters".into());
    }
    Ok(())
}

/// PUT /api/v0/scheduled/{id}
pub async fn scheduled_update(
    Path(id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = require_session_json!(state, req);
    let lang = Lang::from_request(req.headers());
    let body: ScheduledUpdateBody =
        or_return!(super::read_json(req.into_body(), "the scheduled action").await);
    let parsed = body.action;
    async {
        let mut tx = WriteTx::begin(&state.db).await.map_err(internal)?;
        let previous = scheduled::get_by_id_in(&mut tx, &id)
            .await
            .map_err(internal)?
            .ok_or_else(|| schedule_unavailable(lang))?;
        schedule_access_in(&state, &mut tx, &user.id, &previous.owner, lang).await?;
        if previous.configuration_generation != body.expected_configuration_generation {
            return Err(schedule_conflict(lang));
        };
        let link = schedule_body_in(
            &state,
            &mut tx,
            &previous.owner,
            &parsed,
            previous.personal_agent_id.as_deref(),
            lang,
        )
        .await?;
        let timezone = if parsed.timezone.trim().is_empty() {
            user.timezone.clone().unwrap_or_else(|| "UTC".into())
        } else {
            parsed.timezone.trim().into()
        };
        let next = compute_next(&parsed.cron, &timezone)
            .await
            .map_err(|_| invalid_schedule(lang))?;
        let edit = scheduled::EditAction {
            personal_agent_id: parsed.personal_agent_id,
            linked_session_id: link,
            name: parsed.name.trim().into(),
            prompt: parsed.prompt,
            model: parsed.model,
            agent_version: parsed.agent_version,
            cron: parsed.cron,
            timezone,
            tools_enabled: parsed.tools_enabled,
            reuse_conversation: parsed.reuse_conversation,
            reuse_rounds: parsed.reuse_rounds,
            next_run_at: next,
        };
        let result = scheduled::update_in(
            &mut tx,
            &previous.owner,
            &id,
            &body.expected_configuration_generation,
            &edit,
        )
        .await
        .map_err(internal)?;
        let action = match result {
            scheduled::EditOutcome::Saved(row) => row,
            scheduled::EditOutcome::Missing => return Err(schedule_unavailable(lang)),
            scheduled::EditOutcome::Conflict => return Err(schedule_conflict(lang)),
        };
        let view = scheduled_view_in(&mut tx, &action)
            .await
            .map_err(internal)?;
        tx.commit().await.map_err(internal)?;
        Ok::<_, Response>(json_ok(StatusCode::OK, Updated::Row(Box::new(view))))
    }
    .await
    .unwrap_or_else(|response| response)
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct ScheduledEnabledBody {
    pub enabled: bool,
    pub expected_configuration_generation: String,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ScheduledEnabledView {
    pub enabled: bool,
    pub configuration_generation: String,
}

#[derive(serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct EnabledBody {
    pub enabled: bool,
}

/// POST /api/v0/scheduled/{id}/toggle — pause/resume (recomputes next run
/// on resume, exactly like the form path).
pub async fn scheduled_toggle(
    Path(id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = require_session_json!(state, req);
    let lang = Lang::from_request(req.headers());
    let parsed: ScheduledEnabledBody =
        or_return!(super::read_json(req.into_body(), "the scheduled action toggle").await);
    async {
        let mut tx = WriteTx::begin(&state.db).await.map_err(internal)?;
        let action = scheduled::get_by_id_in(&mut tx, &id)
            .await
            .map_err(internal)?
            .ok_or_else(|| schedule_unavailable(lang))?;
        schedule_access_in(&state, &mut tx, &user.id, &action.owner, lang).await?;
        let next = if parsed.enabled {
            compute_next(&action.cron, &action.timezone)
                .await
                .map_err(|_| invalid_schedule(lang))?
        } else {
            None
        };
        let result = scheduled::set_enabled_in(
            &mut tx,
            &action.owner,
            &id,
            &parsed.expected_configuration_generation,
            parsed.enabled,
            next,
        )
        .await
        .map_err(internal)?;
        let row = match result {
            scheduled::EditOutcome::Saved(row) => row,
            scheduled::EditOutcome::Missing => return Err(schedule_unavailable(lang)),
            scheduled::EditOutcome::Conflict => return Err(schedule_conflict(lang)),
        };
        tx.commit().await.map_err(internal)?;
        Ok::<_, Response>(json_ok(
            StatusCode::OK,
            ScheduledEnabledView {
                enabled: row.enabled,
                configuration_generation: row.configuration_generation,
            },
        ))
    }
    .await
    .unwrap_or_else(|response| response)
}

/// DELETE /api/v0/scheduled/{id}
pub async fn scheduled_delete(
    Path(id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = require_session_json!(state, req);
    let lang = Lang::from_request(req.headers());
    async {
        let mut tx = WriteTx::begin(&state.db).await.map_err(internal)?;
        let action = scheduled::get_by_id_in(&mut tx, &id)
            .await
            .map_err(internal)?
            .ok_or_else(|| schedule_unavailable(lang))?;
        schedule_access_in(&state, &mut tx, &user.id, &action.owner, lang).await?;
        match scheduled::delete_in(&mut tx, &action.owner, &id)
            .await
            .map_err(internal)?
        {
            scheduled::DeleteOutcome::Missing => Err(schedule_unavailable(lang)),
            scheduled::DeleteOutcome::Pending => Err(json_error(
                StatusCode::CONFLICT,
                "run_pending",
                &t(lang, "agent-background-error-pending-delete"),
            )),
            scheduled::DeleteOutcome::Deleted => {
                tx.commit().await.map_err(internal)?;
                Ok(Response::builder()
                    .status(StatusCode::NO_CONTENT)
                    .body(rama::http::Body::empty())
                    .expect("static empty response"))
            }
        }
    }
    .await
    .unwrap_or_else(|response| response)
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct CronPreviewBody {
    pub cron: String,
    #[serde(default)]
    pub timezone: String,
}

/// POST /api/v0/scheduled/preview — validate + describe a cron expression,
/// with its next three fire times. Pure computation, no writes.
pub async fn scheduled_preview(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, user) = require_session_json!(state, req);
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return bad_request(msg),
    };
    let parsed: CronPreviewBody = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => return bad_request(format!("parsing the preview body: {err}")),
    };
    let tz_name = if parsed.timezone.trim().is_empty() {
        user.timezone.clone().unwrap_or_else(|| "UTC".into())
    } else {
        parsed.timezone.trim().to_string()
    };
    let Ok(tz) = jiff::tz::TimeZone::get(&tz_name) else {
        return bad_request(format!("unknown timezone: {tz_name}"));
    };
    let Ok(cron) = Cron::parse(&parsed.cron) else {
        return bad_request(format!(
            "not a valid 5-field cron expression: {}",
            parsed.cron
        ));
    };
    json_ok(
        StatusCode::OK,
        CronPreview {
            summary: cron.describe(),
            upcoming: cron
                .upcoming(jiff::Timestamp::now(), &tz, 3)
                .iter()
                .map(|t| t.to_string())
                .collect(),
        },
    )
}

// ---------------------------------------------------------------------------
// Webhooks

/// One webhook as the list, create and update routes return it.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct WebhookView {
    pub run_count: i64,
    pub chat_count: i64,
    pub id: String,
    pub personal_agent_id: Option<String>,
    pub agent: Option<OwnedAgentView>,
    pub name: String,
    pub prompt: String,
    pub model: Option<String>,
    pub principal_id: Option<String>,
    pub agent_version: Option<i64>,
    pub configuration_generation: String,
    pub tools_enabled: bool,
    /// The trigger waits for the run and answers with its output.
    pub synchronous: bool,
    pub reuse_conversation: bool,
    pub reuse_rounds: i64,
    pub enabled: bool,
    pub last_fired_at: Option<String>,
    pub last_status: Option<String>,
    pub last_session_id: Option<String>,
    pub last_error: Option<String>,
    /// A fire's payload is stored and can be rerun.
    pub has_payload: bool,
    /// Whether `last_session_id` names a chat that has since been deleted.
    /// Present on the list response only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_chat_deleted: Option<bool>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct WebhookList {
    pub principal_id: Option<String>,
    pub can_write: bool,
    pub versions: Vec<super::json_agents::VersionView>,
    pub agents: Vec<OwnedAgentView>,
    pub webhooks: Vec<WebhookView>,
    /// The chat models a webhook can run on.
    pub models: Vec<ModelOption>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct WebhookCreated {
    pub webhook: WebhookView,
    /// The trigger secret (`gwh_…`), shown this once.
    pub secret: String,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct WebhookSecret {
    /// The new trigger secret (`gwh_…`), shown this once.
    pub secret: String,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct WebhookRuns {
    /// The newest 50 runs.
    pub runs: Vec<WebhookRunView>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct WebhookRunView {
    pub agent_version: Option<i64>,
    pub configuration_generation: Option<String>,
    pub id: String,
    pub status: Option<String>,
    pub error: Option<String>,
    pub fired_at: String,
    /// What fired the run, e.g. a live trigger or `rerun`.
    pub source: String,
    pub session_id: Option<String>,
    pub assistant_turn_id: Option<String>,
    pub chat_deleted: bool,
    pub prompt: String,
    /// The raw request body the run processed.
    pub payload: String,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct RerunOutcome {
    /// The conversation the rerun ran in. A run that failed is a 502
    /// `run_failed` naming it instead.
    pub session_id: String,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct RerunAccepted {
    pub session_id: String,
    pub status: String,
}

/// `webhook_json` plus the hook's `(runs, chats)` totals — the two differ for
/// a `reuse_conversation` hook, and the list row's link uses exactly that
/// difference to decide whether to link to *the* chat or to the run history.
fn webhook_json_with_counts(
    w: &webhooks::Webhook,
    (run_count, chat_count): (i64, i64),
) -> WebhookView {
    WebhookView {
        run_count,
        chat_count,
        id: w.id.clone(),
        personal_agent_id: w.personal_agent_id.clone(),
        agent: None,
        name: w.name.clone(),
        prompt: w.prompt.clone(),
        model: w.model.clone(),
        principal_id: match &w.owner {
            AutomationOwner::User(_) => None,
            AutomationOwner::Principal(id) => Some(id.clone()),
        },
        agent_version: w.agent_version,
        configuration_generation: w.configuration_generation.clone(),
        tools_enabled: w.tools_enabled,
        synchronous: w.synchronous,
        reuse_conversation: w.reuse_conversation,
        reuse_rounds: w.reuse_rounds,
        enabled: w.enabled,
        last_fired_at: w.last_fired_at.map(|t| t.to_string()),
        last_status: w.last_status.clone(),
        last_session_id: w.last_session_id.clone(),
        last_error: w.last_error.clone(),
        has_payload: w.last_payload.is_some(),
        last_chat_deleted: None,
    }
}

fn webhook_unavailable(lang: Lang) -> Response {
    not_found(t(lang, "webhooks-toast-not-found"))
}
fn webhook_conflict(lang: Lang) -> Response {
    json_error(
        StatusCode::CONFLICT,
        "configuration_conflict",
        &t(lang, "scheduled-error-configuration-conflict"),
    )
}
async fn webhook_access_in(
    state: &RamaState,
    tx: &mut WriteTx,
    user: &str,
    owner: &AutomationOwner,
    lang: Lang,
) -> Result<(), Response> {
    schedule_access_in(state, tx, user, owner, lang)
        .await
        .map_err(|response| {
            if response.status() == StatusCode::NOT_FOUND {
                webhook_unavailable(lang)
            } else {
                response
            }
        })
}
async fn webhook_view_in(
    tx: &mut WriteTx,
    hook: &webhooks::Webhook,
) -> Result<WebhookView, db::DbError> {
    let counts = webhooks::run_counts_for_owner_in(tx, &hook.owner).await?;
    let mut row = webhook_json_with_counts(hook, counts.get(&hook.id).copied().unwrap_or((0, 0)));
    row.agent = schedule_agent_in(tx, &hook.owner, hook.personal_agent_id.as_deref()).await?;
    flag_deleted_last_chats_in(
        tx,
        [(row.last_session_id.as_deref(), &mut row.last_chat_deleted)],
    )
    .await?;
    Ok(row)
}

pub async fn webhooks_list(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, user) = require_session_json!(state, req);
    let lang = Lang::from_request(req.headers());
    let principal_id = super::query_map(&req).get("principal_id").cloned();
    let owner = principal_id
        .as_ref()
        .map(|id| AutomationOwner::Principal(id.clone()))
        .unwrap_or_else(|| AutomationOwner::User(user.id.clone()));
    let personal = if principal_id.is_none() {
        match owned_agents_db::list_personal_agents(&state.db, &user.id).await {
            Ok(rows) => rows.iter().map(json_owned_agents::view).collect(),
            Err(error) => return internal(error),
        }
    } else {
        Vec::new()
    };
    async {
        let mut tx = WriteTx::begin(&state.db).await.map_err(internal)?;
        webhook_access_in(&state, &mut tx, &user.id, &owner, lang).await?;
        let rows = webhooks::list_for_owner_in(&mut tx, &owner)
            .await
            .map_err(internal)?;
        let counts = webhooks::run_counts_for_owner_in(&mut tx, &owner)
            .await
            .map_err(internal)?;
        let (owned_agents, versions, models) = if let Some(id) = &principal_id {
            let owned_agent = schedule_agent_in(&mut tx, &owner, None)
                .await
                .map_err(internal)?
                .ok_or_else(|| webhook_unavailable(lang))?;
            let versions = owned_agents_db::versions_in(&mut tx, id)
                .await
                .map_err(internal)?
                .iter()
                .map(super::json_agents::version_view)
                .collect();
            (vec![owned_agent], versions, Vec::new())
        } else {
            (personal, Vec::new(), chat_model_options(&state))
        };
        let mut hooks: Vec<_> = rows
            .iter()
            .map(|hook| {
                let mut row =
                    webhook_json_with_counts(hook, counts.get(&hook.id).copied().unwrap_or((0, 0)));
                row.agent = owned_agents
                    .iter()
                    .find(|owned_agent| {
                        principal_id.as_deref() == Some(owned_agent.id.as_str())
                            || hook.personal_agent_id.as_deref() == Some(owned_agent.id.as_str())
                    })
                    .cloned();
                row
            })
            .collect();
        flag_deleted_last_chats_in(
            &mut tx,
            hooks
                .iter_mut()
                .map(|row| (row.last_session_id.as_deref(), &mut row.last_chat_deleted)),
        )
        .await
        .map_err(internal)?;
        tx.commit().await.map_err(internal)?;
        Ok::<_, Response>(json_ok(
            StatusCode::OK,
            WebhookList {
                principal_id,
                can_write: true,
                agents: owned_agents,
                versions,
                models,
                webhooks: hooks,
            },
        ))
    }
    .await
    .unwrap_or_else(|response| response)
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct WebhookBody {
    #[serde(default)]
    pub principal_id: Option<String>,
    #[serde(default)]
    pub agent_version: Option<i64>,
    #[serde(default)]
    pub personal_agent_id: Option<String>,
    pub name: String,
    pub prompt: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub tools_enabled: bool,
    #[serde(default)]
    pub synchronous: bool,
    #[serde(default)]
    pub reuse_conversation: bool,
    #[serde(default)]
    pub reuse_rounds: i64,
    #[serde(default)]
    pub linked_session_id: Option<String>,
}
#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct WebhookUpdateBody {
    #[serde(flatten)]
    pub webhook: WebhookBody,
    pub expected_configuration_generation: String,
}
#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct WebhookEnabledBody {
    pub enabled: bool,
    pub expected_configuration_generation: String,
}
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct WebhookEnabledView {
    pub enabled: bool,
    pub configuration_generation: String,
}
#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct WebhookRotateBody {
    pub expected_configuration_generation: String,
}

async fn webhook_body_in(
    state: &RamaState,
    tx: &mut WriteTx,
    owner: &AutomationOwner,
    body: &WebhookBody,
    previous: Option<&str>,
    lang: Lang,
) -> Result<Option<Option<String>>, Response> {
    if body.name.trim().is_empty()
        || body.name.trim().len() > 128
        || body.prompt.trim().is_empty()
        || body.prompt.trim().len() > 8000
    {
        return Err(invalid_schedule(lang));
    };
    match owner {
        AutomationOwner::Principal(id) => {
            if body.principal_id.as_deref() != Some(id.as_str())
                || body.agent_version.is_none_or(|v| v <= 0)
                || body.model.is_some()
                || body.personal_agent_id.is_some()
                || body.synchronous
                || body.reuse_conversation
                || body.linked_session_id.is_some()
            {
                return Err(invalid_schedule(lang));
            };
            aiplane_runtime::agents::profile::organization_automation_version_in(
                state,
                tx,
                id,
                body.agent_version.expect("positive version checked"),
            )
            .await
            .map_err(|error| match error {
                aiplane_runtime::agents::profile::AgentRunError::Db(error) => internal(error),
                _ => invalid_schedule(lang),
            })?;
            Ok(Some(None))
        }
        AutomationOwner::User(user) => {
            if body.principal_id.is_some()
                || body.agent_version.is_some()
                || body
                    .model
                    .as_deref()
                    .is_none_or(|model| model.trim().is_empty())
            {
                return Err(invalid_schedule(lang));
            };
            if !agent_sessions::binding_valid(tx, user, body.personal_agent_id.as_deref(), previous)
                .await
                .map_err(internal)?
            {
                return Err(bad_request(t(lang, "agent-background-error-binding")));
            };
            let Some(link) = body.linked_session_id.as_deref().map(str::trim) else {
                return Ok(None);
            };
            if link.is_empty() {
                return Ok(Some(None));
            };
            if !body.reuse_conversation
                || !agent_sessions::session_matches(
                    tx,
                    user,
                    link,
                    body.personal_agent_id.as_deref(),
                )
                .await
                .map_err(internal)?
            {
                return Err(bad_request(t(lang, "agent-background-error-linked")));
            };
            Ok(Some(Some(link.into())))
        }
    }
}

pub async fn webhooks_create(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, user) = require_session_json!(state, req);
    let lang = Lang::from_request(req.headers());
    let body: WebhookBody = or_return!(super::read_json(req.into_body(), "the webhook").await);
    let owner = body
        .principal_id
        .as_ref()
        .map(|id| AutomationOwner::Principal(id.clone()))
        .unwrap_or_else(|| AutomationOwner::User(user.id.clone()));
    let (secret, hash) = auth_token::mint_webhook();
    async {
        let mut tx = WriteTx::begin(&state.db).await.map_err(internal)?;
        webhook_access_in(&state, &mut tx, &user.id, &owner, lang).await?;
        let link = webhook_body_in(&state, &mut tx, &owner, &body, None, lang).await?;
        let hook = webhooks::create_in(
            &mut tx,
            webhooks::NewWebhook {
                owner,
                created_by: user.id.clone(),
                personal_agent_id: body.personal_agent_id,
                name: body.name.trim().into(),
                prompt: body.prompt,
                model: body.model,
                agent_version: body.agent_version,
                tools_enabled: body.tools_enabled,
                synchronous: body.synchronous,
                reuse_conversation: body.reuse_conversation,
                reuse_rounds: body.reuse_rounds,
                secret_hash: hash,
                linked_session_id: link.flatten(),
            },
        )
        .await
        .map_err(internal)?;
        let webhook = webhook_view_in(&mut tx, &hook).await.map_err(internal)?;
        tx.commit().await.map_err(internal)?;
        Ok::<_, Response>(json_ok(
            StatusCode::CREATED,
            WebhookCreated { webhook, secret },
        ))
    }
    .await
    .unwrap_or_else(|response| response)
}

pub async fn webhooks_update(
    Path(id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = require_session_json!(state, req);
    let lang = Lang::from_request(req.headers());
    let request: WebhookUpdateBody =
        or_return!(super::read_json(req.into_body(), "the webhook").await);
    let body = request.webhook;
    async {
        let mut tx = WriteTx::begin(&state.db).await.map_err(internal)?;
        let old = webhooks::get_by_id_in(&mut tx, &id)
            .await
            .map_err(internal)?
            .ok_or_else(|| webhook_unavailable(lang))?;
        webhook_access_in(&state, &mut tx, &user.id, &old.owner, lang).await?;
        if old.configuration_generation != request.expected_configuration_generation {
            return Err(webhook_conflict(lang));
        };
        let link = webhook_body_in(
            &state,
            &mut tx,
            &old.owner,
            &body,
            old.personal_agent_id.as_deref(),
            lang,
        )
        .await?;
        let edit = webhooks::EditWebhook {
            personal_agent_id: body.personal_agent_id,
            linked_session_id: link,
            name: body.name.trim().into(),
            prompt: body.prompt,
            model: body.model,
            agent_version: body.agent_version,
            tools_enabled: body.tools_enabled,
            synchronous: body.synchronous,
            reuse_conversation: body.reuse_conversation,
            reuse_rounds: body.reuse_rounds,
        };
        let outcome = webhooks::update_in(
            &mut tx,
            &old.owner,
            &id,
            &request.expected_configuration_generation,
            &edit,
        )
        .await
        .map_err(internal)?;
        let row = match outcome {
            webhooks::EditOutcome::Missing => return Err(webhook_unavailable(lang)),
            webhooks::EditOutcome::Conflict => return Err(webhook_conflict(lang)),
            webhooks::EditOutcome::Saved(row) => row,
        };
        let view = webhook_view_in(&mut tx, &row).await.map_err(internal)?;
        tx.commit().await.map_err(internal)?;
        Ok::<_, Response>(json_ok(StatusCode::OK, Updated::Row(Box::new(view))))
    }
    .await
    .unwrap_or_else(|response| response)
}

pub async fn webhooks_toggle(
    Path(id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = require_session_json!(state, req);
    let lang = Lang::from_request(req.headers());
    let body: WebhookEnabledBody =
        or_return!(super::read_json(req.into_body(), "the webhook toggle").await);
    async {
        let mut tx = WriteTx::begin(&state.db).await.map_err(internal)?;
        let hook = webhooks::get_by_id_in(&mut tx, &id)
            .await
            .map_err(internal)?
            .ok_or_else(|| webhook_unavailable(lang))?;
        webhook_access_in(&state, &mut tx, &user.id, &hook.owner, lang).await?;
        let row = match webhooks::set_enabled_in(
            &mut tx,
            &hook.owner,
            &id,
            &body.expected_configuration_generation,
            body.enabled,
        )
        .await
        .map_err(internal)?
        {
            webhooks::EditOutcome::Missing => return Err(webhook_unavailable(lang)),
            webhooks::EditOutcome::Conflict => return Err(webhook_conflict(lang)),
            webhooks::EditOutcome::Saved(row) => row,
        };
        tx.commit().await.map_err(internal)?;
        Ok::<_, Response>(json_ok(
            StatusCode::OK,
            WebhookEnabledView {
                enabled: row.enabled,
                configuration_generation: row.configuration_generation,
            },
        ))
    }
    .await
    .unwrap_or_else(|response| response)
}
pub async fn webhooks_rotate(
    Path(id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = require_session_json!(state, req);
    let lang = Lang::from_request(req.headers());
    let body: WebhookRotateBody =
        or_return!(super::read_json(req.into_body(), "the webhook secret rotation").await);
    let (secret, hash) = auth_token::mint_webhook();
    async {
        let mut tx = WriteTx::begin(&state.db).await.map_err(internal)?;
        let hook = webhooks::get_by_id_in(&mut tx, &id)
            .await
            .map_err(internal)?
            .ok_or_else(|| webhook_unavailable(lang))?;
        webhook_access_in(&state, &mut tx, &user.id, &hook.owner, lang).await?;
        match webhooks::rotate_secret_in(
            &mut tx,
            &hook.owner,
            &id,
            &body.expected_configuration_generation,
            &hash,
        )
        .await
        .map_err(internal)?
        {
            webhooks::EditOutcome::Missing => return Err(webhook_unavailable(lang)),
            webhooks::EditOutcome::Conflict => return Err(webhook_conflict(lang)),
            webhooks::EditOutcome::Saved(_) => {}
        }
        tx.commit().await.map_err(internal)?;
        Ok::<_, Response>(json_ok(StatusCode::OK, WebhookSecret { secret }))
    }
    .await
    .unwrap_or_else(|response| response)
}
pub async fn webhooks_delete(
    Path(id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = require_session_json!(state, req);
    let lang = Lang::from_request(req.headers());
    async {
        let mut tx = WriteTx::begin(&state.db).await.map_err(internal)?;
        let hook = webhooks::get_by_id_in(&mut tx, &id)
            .await
            .map_err(internal)?
            .ok_or_else(|| webhook_unavailable(lang))?;
        webhook_access_in(&state, &mut tx, &user.id, &hook.owner, lang).await?;
        match webhooks::delete_in(&mut tx, &hook.owner, &id)
            .await
            .map_err(internal)?
        {
            scheduled::DeleteOutcome::Missing => return Err(webhook_unavailable(lang)),
            scheduled::DeleteOutcome::Pending => {
                return Err(json_error(
                    StatusCode::CONFLICT,
                    "run_pending",
                    &t(lang, "agent-background-error-pending-delete"),
                ));
            }
            scheduled::DeleteOutcome::Deleted => {}
        }
        tx.commit().await.map_err(internal)?;
        Ok::<_, Response>(super::no_content())
    }
    .await
    .unwrap_or_else(|response| response)
}
fn webhook_run_json(run: &webhooks::WebhookRun) -> WebhookRunView {
    WebhookRunView {
        id: run.id.clone(),
        agent_version: run.agent_version,
        configuration_generation: run.configuration_generation.clone(),
        status: run.status.clone(),
        error: run.error.clone(),
        fired_at: run.fired_at.to_string(),
        source: run.source.clone(),
        session_id: run.session_id.clone(),
        assistant_turn_id: run.assistant_turn_id.clone(),
        chat_deleted: run.chat_deleted,
        prompt: run.prompt.clone(),
        payload: run.payload.clone(),
    }
}
pub async fn webhooks_runs(
    Path(id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = require_session_json!(state, req);
    let lang = Lang::from_request(req.headers());
    async {
        let mut tx = WriteTx::begin(&state.db).await.map_err(internal)?;
        let hook = webhooks::get_by_id_in(&mut tx, &id)
            .await
            .map_err(internal)?
            .ok_or_else(|| webhook_unavailable(lang))?;
        webhook_access_in(&state, &mut tx, &user.id, &hook.owner, lang).await?;
        let runs = webhooks::list_runs_in(&mut tx, &id, 50)
            .await
            .map_err(internal)?
            .iter()
            .map(webhook_run_json)
            .collect();
        tx.commit().await.map_err(internal)?;
        Ok::<_, Response>(json_ok(StatusCode::OK, WebhookRuns { runs }))
    }
    .await
    .unwrap_or_else(|response| response)
}
#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct RerunBody {
    pub prompt: String,
    #[serde(default)]
    pub run: Option<String>,
    pub expected_configuration_generation: String,
}

pub async fn webhooks_rerun(
    Path(id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, user) = require_session_json!(state, req);
    let lang = Lang::from_request(req.headers());
    let body: RerunBody = or_return!(super::read_json(req.into_body(), "the webhook replay").await);
    if body.prompt.trim().is_empty() || body.prompt.trim().len() > 8000 {
        return invalid_schedule(lang);
    };
    let prepared = async {
        let mut tx = WriteTx::begin(&state.db).await.map_err(internal)?;
        let hook = webhooks::get_by_id_in(&mut tx, &id)
            .await
            .map_err(internal)?
            .ok_or_else(|| webhook_unavailable(lang))?;
        webhook_access_in(&state, &mut tx, &user.id, &hook.owner, lang).await?;
        if hook.configuration_generation != body.expected_configuration_generation {
            return Err(webhook_conflict(lang));
        };
        let payload = match body.run.as_deref() {
            Some(run) => Some(
                webhooks::get_run_in(&mut tx, &id, run)
                    .await
                    .map_err(internal)?
                    .ok_or_else(|| webhook_unavailable(lang))?
                    .payload,
            ),
            None => hook.last_payload.clone(),
        }
        .ok_or_else(|| {
            json_error(
                StatusCode::CONFLICT,
                "no_payload",
                &t(lang, "webhooks-rerun-no-payload"),
            )
        })?;
        tx.commit().await.map_err(internal)?;
        Ok::<_, Response>((hook, payload))
    }
    .await;
    let (hook, payload) = match prepared {
        Ok(value) => value,
        Err(response) => return response,
    };
    let input = super::webhooks::build_input(
        body.prompt.trim(),
        "(replayed webhook payload)",
        "",
        &payload,
    );
    let opened = super::webhooks::admit_and_open(
        &state,
        &hook,
        webhooks::AdmissionKind::Replay,
        body.prompt.trim(),
        &payload,
        &input,
        Some(&user.id),
    )
    .await;
    let (session, turn, drive) = match opened {
        Ok(value) => value,
        Err(error) => return super::webhooks::admission_error(error, lang, false),
    };
    aiplane_runtime::server::headless::drive(&state, drive).await;
    let (status, error, _) = super::webhooks::outcome(&state.db, &session, &turn).await;
    if matches!(&hook.owner, AutomationOwner::Principal(_)) {
        let delivery = async {
            let mut tx = WriteTx::begin(&state.db).await.map_err(internal)?;
            webhook_access_in(&state, &mut tx, &user.id, &hook.owner, lang).await?;
            tx.commit().await.map_err(internal)
        }
        .await;
        if let Err(response) = delivery {
            return response;
        }
    }
    if status == "waiting" {
        return json_ok(
            StatusCode::ACCEPTED,
            RerunAccepted {
                session_id: session,
                status: "waiting".into(),
            },
        );
    };
    if status != "ok" {
        return super::json_error_with(
            StatusCode::BAD_GATEWAY,
            "run_failed",
            error.as_deref().unwrap_or("the run failed"),
            serde_json::Map::from_iter([
                ("session_id".into(), serde_json::json!(session)),
                ("status".into(), serde_json::json!(status)),
            ]),
        );
    };
    json_ok(
        StatusCode::OK,
        RerunOutcome {
            session_id: session,
        },
    )
}
