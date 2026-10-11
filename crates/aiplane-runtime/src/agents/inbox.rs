// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The human-in-the-loop inbox (`docs/agent-hil.md`): what is
//! waiting for whom, and the notification when something starts to wait.
//!
//! **Who sees an item.**
//! - An agent conversation's approval or handoff: anyone holding access to
//!   the agent ([`super::access::effective_access`]) — an admin, a manager
//!   with a `read` or `write` share, or a holder of a `respond` share. A
//!   responder (`respond` only) sees the item and its minimal context only —
//!   the question, the handoff context, an approval's tool and arguments —
//!   never the spec or the conversations. A test conversation's pause stays in the test chat;
//!   a visitor's secure input is never anyone's item.
//! - A person's own conversation that waits — a scheduled action or a webhook
//!   run that paused: its owner, and nobody else.
//!
//! **Notification.** Once per pause ([`chat::mark_suspension_notified`]):
//! Web Push to everyone who may answer it, and every Slack or Discord channel
//! of the agent, narrowed by the handoff route's `notify` list. A message
//! carries the agent, the kind and a link to the item; the question or the
//! tool name only on a channel configured with `details`.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use aiplane_agents::db::agent_channels;
use aiplane_agents::db::agents::{self as agents_db, Access, SubjectKind};
use aiplane_agents::db::run_sessions;
use aiplane_agents::db::run_sessions::{PendingSuspension, SessionOwner};
use aiplane_agents::notify_channels::{self, Notice, Reach};
use aiplane_core::server::db::{DbError, users};
use aiplane_features::server::push::{self, PushMessage};
use jiff::Timestamp;
use serde::Serialize;
use serde_json::Value;
use session_core::db::{self as chat, Answerer, DecisionKind, SuspensionKind};
use session_core::i18n::{Lang, args, t, t_args};

use super::human::handoff_of;
use crate::rama_server::state::RamaState;

/// The person looking at the inbox.
#[derive(Debug, Clone)]
pub struct Viewer {
    pub user_id: String,
    pub groups: Vec<String>,
}

impl Viewer {
    pub fn of(state: &RamaState, user: &users::User) -> Self {
        Self {
            user_id: user.id.clone(),
            groups: state.rbac.role_ids_for(&user.roles),
        }
    }
}

/// Why the viewer may answer an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Standing {
    /// Admin, or a manager with a `read` or `write` share: may also open
    /// the agent.
    Manager,
    /// A `respond` share: the item and nothing else.
    Responder,
    /// Their own conversation (a scheduled or webhook run).
    Owner,
}

#[derive(Debug, Clone, PartialEq, Serialize, schemars::JsonSchema)]
pub struct AgentRef {
    pub id: String,
    pub name: String,
    pub display: String,
}

/// The waiting call of an approval, as the model issued it.
#[derive(Debug, Clone, PartialEq, Serialize, schemars::JsonSchema)]
pub struct WaitingCall {
    pub name: String,
    pub arguments: String,
}

/// One pending item, as the inbox shows it.
#[derive(Debug, Clone, PartialEq, Serialize, schemars::JsonSchema)]
pub struct InboxItem {
    /// The pause's `request_id`; answering names it.
    pub id: String,
    pub kind: SuspensionKind,
    pub standing: Standing,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentRef>,
    pub session_id: String,
    pub turn_id: String,
    /// The conversation's title, on the viewer's own run only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// A handoff's question.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub question: Option<String>,
    /// An approval's call — the innermost one, inside a sub-agent run too.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call: Option<WaitingCall>,
    /// What an approved call would do, in the tool's own words (the
    /// request's `message`, e.g. `schedule_action`'s preview).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// A handoff's context: the visitor's last message, the model's view of
    /// the slots, the transcript when the route hands it over.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<Value>,
    pub options: Vec<DecisionKind>,
    pub created_at: Timestamp,
    pub expires_at: Timestamp,
}

/// Every item `viewer` may answer, oldest first.
pub async fn list(state: &RamaState, viewer: &Viewer) -> Result<Vec<InboxItem>, DbError> {
    let pending = run_sessions::pending_suspensions(&state.db).await?;
    let mut standing_by_agent: BTreeMap<String, Option<Standing>> = BTreeMap::new();
    let mut items = Vec::new();
    for p in pending {
        let standing = match &p.owner {
            SessionOwner::User(owner) => (owner == &viewer.user_id).then_some(Standing::Owner),
            SessionOwner::Principal(agent) => {
                if !staff_item(&p) {
                    continue;
                }
                match standing_by_agent.get(agent) {
                    Some(known) => *known,
                    None => {
                        let found = agent_standing(state, viewer, agent).await?;
                        standing_by_agent.insert(agent.clone(), found);
                        found
                    }
                }
            }
        };
        if let Some(standing) = standing {
            items.push(item(state, p, standing).await?);
        }
    }
    Ok(items)
}

/// The item `request_id` names, if it waits and `viewer` may answer it.
pub async fn find(
    state: &RamaState,
    viewer: &Viewer,
    request_id: &str,
) -> Result<Option<InboxItem>, DbError> {
    let Some(p) = run_sessions::pending_by_request(&state.db, request_id).await? else {
        return Ok(None);
    };
    let standing = match &p.owner {
        SessionOwner::User(owner) => (owner == &viewer.user_id).then_some(Standing::Owner),
        SessionOwner::Principal(agent) if staff_item(&p) => {
            agent_standing(state, viewer, agent).await?
        }
        SessionOwner::Principal(_) => None,
    };
    match standing {
        Some(standing) => item(state, p, standing).await.map(Some),
        None => Ok(None),
    }
}

/// An agent conversation's pause that staff answer, outside the test chat.
fn staff_item(p: &PendingSuspension) -> bool {
    p.suspension.kind.answered_by() == Answerer::Staff
        && p.agent_version != Some(agents_db::DRAFT_VERSION)
}

/// How `viewer` stands to agent `agent_id`'s inbox, if at all.
pub async fn agent_standing(
    state: &RamaState,
    viewer: &Viewer,
    agent_id: &str,
) -> Result<Option<Standing>, DbError> {
    Ok(
        super::access::effective_access(state, agent_id, &viewer.user_id, &viewer.groups)
            .await?
            .map(|access| match access {
                Access::Respond => Standing::Responder,
                Access::Read | Access::Write => Standing::Manager,
            }),
    )
}

/// The pause that actually waits: the innermost of a chain of sub-agent
/// runs, which holds the real tool call of an approval.
async fn innermost(
    db: &aiplane_core::server::db::Pool,
    top: &chat::TurnSuspension,
) -> Result<chat::TurnSuspension, DbError> {
    let mut current = top.clone();
    for _ in 0..aiplane_core::server::run_chain::MAX_DEPTH {
        let Some(child) = current.child_turn.clone() else {
            break;
        };
        let owner = chat::user_for_turn(db, &current.turn_id).await?;
        let linked = match aiplane_agents::db::child_sessions::child_of_turn(db, &child).await? {
            Some(aiplane_agents::db::child_sessions::LocalChild::Principal(session)) => {
                owner.is_none()
                    && session.parent_turn_id.as_deref() == Some(current.turn_id.as_str())
            }
            Some(aiplane_agents::db::child_sessions::LocalChild::Personal(binding)) => {
                owner.as_deref() == Some(binding.session.owner_user_id.as_str())
                    && binding.link.parent_turn_id == current.turn_id
                    && binding.link.parent_tool_call_id == current.tool_call.id
                    && current.tool_call.name == super::router::FORWARD_TOOL_NAME
                    && aiplane_agents::db::child_sessions::root_session_id(
                        db,
                        &binding.session.owner_user_id,
                        &binding.session.session_id,
                    )
                    .await?
                    .is_some()
            }
            None => false,
        };
        if !linked {
            break;
        }
        match chat::get_suspension(db, &child).await? {
            Some(next) => current = next,
            None => break,
        }
    }
    Ok(current)
}

async fn item(
    state: &RamaState,
    p: PendingSuspension,
    standing: Standing,
) -> Result<InboxItem, DbError> {
    let agent = match &p.owner {
        SessionOwner::Principal(id) => agents_db::get(&state.db, id).await?.map(|a| AgentRef {
            id: a.principal.id,
            name: a.principal.name,
            display: a.principal.display,
        }),
        SessionOwner::User(_) => None,
    };
    let waiting = innermost(&state.db, &p.suspension).await?;
    let approval = waiting.kind == SuspensionKind::Approval;
    let call = approval.then(|| WaitingCall {
        name: waiting.tool_call.name.clone(),
        arguments: waiting.tool_call.arguments.clone(),
    });
    let detail = approval.then(|| waiting.message.clone()).flatten();
    let context = handoff_of(p.suspension.run_context.as_ref()).cloned();
    Ok(InboxItem {
        id: p.suspension.request_id.clone(),
        kind: p.suspension.kind,
        standing,
        agent,
        session_id: p.session_id.clone(),
        turn_id: p.suspension.turn_id.clone(),
        title: (standing == Standing::Owner)
            .then_some(p.title.clone())
            .flatten(),
        question: (p.suspension.kind == SuspensionKind::HumanAnswer)
            .then(|| p.suspension.message.clone())
            .flatten(),
        call,
        detail,
        context,
        options: p.suspension.kind.options().to_vec(),
        created_at: p.suspension.created_at,
        expires_at: p.suspension.expires_at,
    })
}

/// Everyone who may answer agent `agent_id`'s items, by user id: whoever
/// holds a share that takes effect, directly or through a group.
/// Admins without a share are not notified: they may answer everything, and
/// would be told about everything.
async fn agent_answerers(state: &RamaState, agent_id: &str) -> Result<Vec<String>, DbError> {
    let mut direct = BTreeSet::new();
    let mut groups = BTreeSet::new();
    for share in agents_db::shares(&state.db, agent_id).await? {
        match share.subject_kind {
            SubjectKind::User => direct.insert(share.subject_id),
            SubjectKind::Group => groups.insert(share.subject_id),
        };
    }
    let mut out = BTreeSet::new();
    for user in users::list_all(&state.db).await? {
        let viewer = Viewer::of(state, &user);
        let named = direct.contains(&user.id) || viewer.groups.iter().any(|g| groups.contains(g));
        if named && agent_standing(state, &viewer, agent_id).await?.is_some() {
            out.insert(user.id);
        }
    }
    Ok(out.into_iter().collect())
}

/// What a notice says about a pause, in `lang`.
struct Wording<'a> {
    agent: Option<&'a AgentRef>,
    title: Option<&'a str>,
    kind: SuspensionKind,
    detail: Option<String>,
}

impl Wording<'_> {
    fn headline(&self, lang: Lang) -> String {
        match (self.agent, self.title) {
            (Some(agent), _) => t_args(
                lang,
                "agent-inbox-notify-title",
                &args([("agent", agent.display.clone().into())]),
            ),
            (None, Some(title)) => session_core::text::truncate_chars(title, 80),
            (None, None) => t(lang, "agent-inbox-notify-run-title"),
        }
    }

    fn summary(&self, lang: Lang) -> String {
        t(
            lang,
            match (self.agent.is_some(), self.kind) {
                (false, _) => "agent-inbox-notify-run",
                (true, SuspensionKind::HumanAnswer) => "agent-inbox-notify-handoff",
                (true, _) => "agent-inbox-notify-approval",
            },
        )
    }
}

/// Announce the pause `request_id` once: the first caller sends, every
/// later one does nothing. Returns whether this call sent it.
pub async fn announce(state: &RamaState, request_id: &str) -> bool {
    let pending = match run_sessions::pending_by_request(&state.db, request_id).await {
        Ok(Some(p)) => p,
        Ok(None) => return false,
        Err(err) => {
            tracing::warn!(error = %err, request_id, "reading a pause to announce");
            return false;
        }
    };
    let (recipients, agent) = match &pending.owner {
        SessionOwner::User(owner) => (vec![owner.clone()], None),
        SessionOwner::Principal(agent_id) => {
            if !staff_item(&pending) {
                return false;
            }
            let agent = agents_db::get(&state.db, agent_id)
                .await
                .ok()
                .flatten()
                .map(|a| AgentRef {
                    id: a.principal.id,
                    name: a.principal.name,
                    display: a.principal.display,
                });
            let recipients = agent_answerers(state, agent_id)
                .await
                .unwrap_or_else(|err| {
                    tracing::warn!(error = %err, agent = %agent_id, "listing who may answer");
                    Vec::new()
                });
            (recipients, agent)
        }
    };
    match chat::mark_suspension_notified(&state.db, request_id, Timestamp::now()).await {
        Ok(true) => {}
        Ok(false) => return false,
        Err(err) => {
            tracing::warn!(error = %err, request_id, "recording a pause as announced");
            return false;
        }
    }
    let only: Option<Vec<String>> = handoff_of(pending.suspension.run_context.as_ref())
        .and_then(|h| h.get("notify"))
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        });
    let waiting = innermost(&state.db, &pending.suspension)
        .await
        .unwrap_or_else(|_| pending.suspension.clone());
    let wording = Wording {
        agent: agent.as_ref(),
        title: pending.title.as_deref(),
        kind: pending.suspension.kind,
        detail: match pending.suspension.kind {
            SuspensionKind::HumanAnswer => pending.suspension.message.clone(),
            _ => Some(waiting.tool_call.name.clone()),
        },
    };
    let path = format!("/inbox?item={request_id}");
    if wants(only.as_deref(), "push") {
        push(state, &recipients, &wording, &path, request_id).await;
    }
    if let Some(agent) = &agent {
        let link = format!("{}{path}", state.public_url().trim_end_matches('/'));
        channels(state, agent, &wording, &link, only.as_deref()).await;
    }
    true
}

/// Whether a handoff route's `notify` list (`None`: every channel) names
/// `channel`.
fn wants(only: Option<&[String]>, channel: &str) -> bool {
    only.is_none_or(|list| list.iter().any(|c| c == channel))
}

/// [`announce`] off the caller's path: a slow chat service must not hold up
/// the turn that just paused.
pub fn announce_in_background(state: Arc<RamaState>, request_id: String) {
    tokio::spawn(async move {
        announce(&state, &request_id).await;
    });
}

async fn push(
    state: &RamaState,
    recipients: &[String],
    wording: &Wording<'_>,
    path: &str,
    tag: &str,
) {
    let Some(sender) = state.push.clone() else {
        return;
    };
    for user in recipients {
        let compose = |lang| PushMessage {
            title: wording.headline(lang),
            body: wording.summary(lang),
            url: path.to_string(),
            tag: tag.to_string(),
        };
        if let Err(err) = push::send_to_user(&sender, &state.db, user, compose).await {
            tracing::warn!(error = %err, "push: announcing a waiting turn");
        }
    }
}

/// Where a notification channel set up by `configured_by` may post: an
/// admin's is the operator's own choice ([`Reach::Operator`]); anyone
/// else's — and one whose creator is gone — goes through `outbound_guard`.
/// Asked again on every post, so a channel follows its creator's standing
/// today, not the day it was made.
pub fn channel_reach(state: &RamaState, configured_by: Option<&users::User>) -> Reach {
    match configured_by {
        Some(user) if state.rbac.is_admin(&state.rbac.role_ids_for(&user.roles)) => Reach::Operator,
        _ => Reach::Guarded {
            allow_private: state.config().network.allow_private_networks,
        },
    }
}

async fn channels(
    state: &RamaState,
    agent: &AgentRef,
    wording: &Wording<'_>,
    link: &str,
    only: Option<&[String]>,
) {
    let targets = match agent_channels::targets(&state.db, &state.crypto, &agent.id).await {
        Ok(targets) => targets,
        Err(err) => {
            tracing::warn!(error = %err, agent = %agent.id, "reading notification channels");
            return;
        }
    };
    for target in targets {
        let kind = target.channel.kind;
        if !wants(only, kind.as_str()) {
            continue;
        }
        let lang = Lang::from_code(&target.channel.lang).unwrap_or(Lang::En);
        let notice = Notice {
            headline: wording.headline(lang),
            summary: wording.summary(lang),
            detail: target
                .channel
                .details
                .then(|| wording.detail.clone())
                .flatten(),
            link: link.to_string(),
            link_label: t(lang, "agent-inbox-notify-open"),
        };
        let payload = notify_channels::payload(kind, &notice);
        let creator = match users::find_by_id(&state.db, &target.channel.created_by).await {
            Ok(creator) => creator,
            Err(err) => {
                tracing::warn!(error = %err, channel = %target.channel.id, "reading a channel's creator");
                continue;
            }
        };
        let reach = channel_reach(state, creator.as_ref());
        if let Err(err) = notify_channels::post(&state.http, reach, &target.url, &payload).await {
            tracing::warn!(
                channel = %target.channel.id,
                kind = kind.as_str(),
                error = %err,
                "a waiting-turn notification could not be delivered"
            );
        }
    }
}

/// Whether `viewer` answers for at least one published agent — what decides,
/// next to a waiting item, that the inbox is worth showing them at all.
pub async fn answers_for_published(state: &RamaState, viewer: &Viewer) -> Result<bool, DbError> {
    for (agent_id, live) in agents_db::publication_status(&state.db).await? {
        if live && agent_standing(state, viewer, &agent_id).await?.is_some() {
            return Ok(true);
        }
    }
    Ok(false)
}

/// An item list, as the API answers it.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct InboxList {
    /// Oldest first.
    pub items: Vec<InboxItem>,
    pub count: usize,
    /// Whether the viewer answers for at least one published agent.
    pub answers: bool,
}

impl InboxList {
    pub fn new(items: Vec<InboxItem>, answers: bool) -> Self {
        Self {
            count: items.len(),
            items,
            answers,
        }
    }
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn personal_inbox_does_not_follow_a_forged_other_owner_child_reference() {
        let db = aiplane_core::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        let now = jiff::Timestamp::now();
        for owner in ["caller", "other"] {
            aiplane_core::server::db::users::upsert(
                &db,
                &aiplane_core::server::db::users::User {
                    id: owner.into(),
                    email: format!("{owner}@example.test"),
                    name: None,
                    roles: Vec::new(),
                    created_at: now,
                    updated_at: now,
                    timezone: None,
                    speech_voice: None,
                },
            )
            .await
            .unwrap();
        }
        let mut rows = Vec::new();
        for (owner, turn, child) in [
            ("caller", "root", Some("unrelated")),
            ("other", "unrelated", None),
        ] {
            let session = session_core::db::create_session(&db, owner).await.unwrap();
            session_core::db::create_assistant_turn_in_progress(&db, &session.id, turn, "model")
                .await
                .unwrap();
            let row = session_core::db::TurnSuspension {
                turn_id: turn.into(),
                request_id: format!("request-{turn}"),
                kind: session_core::db::SuspensionKind::Approval,
                message: None,
                tool_call: session_core::db::PendingCall {
                    execution_facts: None,
                    id: format!("call-{turn}"),
                    name: "forward_request".into(),
                    arguments: format!("{{\"private_owner\":\"{owner}\"}}"),
                    actions: None,
                },
                tail: Vec::new(),
                budget_used: Default::default(),
                child_turn: child.map(str::to_owned),
                on_timeout: session_core::db::TimeoutFallback::Deny,
                expires_at: now,
                created_at: now,
                run_context: None,
            };
            session_core::db::suspend_turn(&db, &row).await.unwrap();
            rows.push(row);
        }
        let visible = super::innermost(&db, &rows[0]).await.unwrap();
        assert_eq!(visible, rows[0]);
        assert!(!visible.tool_call.arguments.contains("other"));
    }

    use aiplane_agents::db::agent_channels::ChannelKind;

    #[test]
    fn a_channel_kind_names_itself_as_the_notify_list_does() {
        for kind in [ChannelKind::Slack, ChannelKind::Discord] {
            assert!(super::super::spec::NOTIFY_CHANNELS.contains(&kind.as_str()));
        }
    }
}
