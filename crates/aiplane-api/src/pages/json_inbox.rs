// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Human in the loop over HTTP (`docs/agent-hil.md`).
//!
//! - `/api/v0/agents/inbox` — what waits for the signed-in person: an agent's
//!   approvals and handoffs when they hold access to it (an admin, a share
//!   holder — `respond` answers without the agent-management permission),
//!   and their own paused scheduled or webhook runs. Every signed-in person may ask; most see nothing.
//! - `/api/v0/agents/inbox/{id}/answer` — answer one, through the same resume
//!   the staff route (`json_agent_test::resume_turn`, by [`resume_as_staff`])
//!   and the chat use.
//! - `/api/v0/agents/inbox/events` — a count frame whenever the set changes.
//! - `/api/v0/agents/{id}/channels` — where a waiting turn is announced
//!   besides Web Push. Managed with a share like the rest of the agent.

use std::sync::Arc;
use std::time::Duration;

use rama::http::service::web::extract::State;
use rama::http::{Request, Response, StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};
use session_core::chat_json::json_stream_response;
use session_core::db as chat;
use session_core::i18n::{Lang, t};

use super::agent_errors::resume_error;
use super::json_agents::agent_at;
use super::json_principals::require_agent_manager;
use super::{bad_request, internal, json_error, json_ok, no_content, not_found, raw_path_segment};
use aiplane_agents::db::agent_channels::{self, ChannelKind, NewChannel};
use aiplane_agents::db::agents::Access;
use aiplane_agents::notify_channels::validate_webhook_url;
use aiplane_runtime::agents::embed::{self as embed_rt, TurnWork};
use aiplane_runtime::agents::inbox::{self, Standing, Viewer};
use aiplane_runtime::agents::resume::{AgentResume, ResumedBy, claim};
use aiplane_runtime::rama_server::state::RamaState;

/// How often the event stream looks for a change.
const EVENT_POLL: Duration = Duration::from_secs(3);
/// A stream ends after this long; `EventSource` reconnects on its own.
const STREAM_LIMIT: Duration = Duration::from_secs(600);

const MAX_NAME_CHARS: usize = 80;

fn item_not_found(id: &str) -> Response {
    json_error(
        StatusCode::NOT_FOUND,
        "inbox_item_not_found",
        &format!(
            "nothing in your inbox is waiting as `{id}` — it was answered or expired, or it is \
             not yours to answer; reload the inbox"
        ),
    )
}

/// GET /api/v0/agents/inbox
pub async fn list(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_, user) = or_return!(super::require_session_json(&state, &req).await);
    let viewer = Viewer::of(&state, &user);
    let listed = match inbox::list(&state, &viewer).await {
        Ok(items) => items,
        Err(err) => return internal(err),
    };
    match inbox::answers_for_published(&state, &viewer).await {
        Ok(answers) => json_ok(StatusCode::OK, inbox::InboxList::new(listed, answers)),
        Err(err) => internal(err),
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnswerBody {
    /// One of the item's `options`.
    pub decision: chat::DecisionKind,
    /// The decision's value, where its kind takes one.
    #[serde(default)]
    pub value: Option<Value>,
}

/// The turn that runs again with the decision.
#[derive(serde::Serialize, schemars::JsonSchema)]
#[schemars(rename = "InboxResumed")]
pub struct Resumed {
    pub turn_id: String,
}

/// POST /api/v0/agents/inbox/{id}/answer — `202 {turn_id}` once the turn runs
/// again. The visitor gets the outcome on their own stream; the answering
/// person's inbox drops the item.
pub async fn answer(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let lang = Lang::from_request(req.headers());
    let (_, user) = or_return!(super::require_session_json(&state, &req).await);
    let Some(id) = raw_path_segment(&req, 1) else {
        return bad_request("the URL is missing the inbox item id");
    };
    let ctx = super::chat::json_api::request_ctx(&state, &req, false);
    let body: AnswerBody = or_return!(super::read_json(req.into_body(), "the answer").await);
    let decision = match super::chat::json_api::decision_from(body.decision, body.value) {
        Ok(decision) => decision,
        Err(msg) => return bad_request(msg),
    };
    let viewer = Viewer::of(&state, &user);
    let item = match inbox::find(&state, &viewer, &id).await {
        Ok(Some(item)) => item,
        Ok(None) => return item_not_found(&id),
        Err(err) => return internal(err),
    };
    if item.standing == Standing::Owner {
        let turn = match chat::get_turn(&state.db, &item.session_id, &item.turn_id).await {
            Ok(Some(turn)) => turn,
            Ok(None) => return item_not_found(&id),
            Err(err) => return internal(err),
        };
        return match super::chat::resume_turn(&state, &user, &turn, Some(&id), decision, ctx).await
        {
            Ok(()) => json_ok(
                StatusCode::ACCEPTED,
                Resumed {
                    turn_id: item.turn_id,
                },
            ),
            Err(super::chat::ResumeTurnError::Busy) => json_error(
                StatusCode::CONFLICT,
                "turn_in_progress",
                "another turn is running in that conversation or every parallel slot is in \
                 use — answer again once it has finished",
            ),
            Err(super::chat::ResumeTurnError::Unavailable) => json_error(
                StatusCode::CONFLICT,
                "dot_unavailable",
                &t(lang, "dot-background-error-unavailable"),
            ),
            Err(super::chat::ResumeTurnError::Context(err)) => internal(err),
            Err(super::chat::ResumeTurnError::Refused(refused)) => resume_error(
                aiplane_runtime::agents::resume::AgentResumeError::Refused(refused),
                lang,
            ),
        };
    }
    let Some(agent) = item.agent.clone() else {
        return item_not_found(&id);
    };
    resume_as_staff(
        state,
        StaffDecision {
            agent_id: &agent.id,
            session_id: &item.session_id,
            turn_id: &item.turn_id,
            request_id: Some(&id),
            decision,
            user_id: &user.id,
            lang,
        },
    )
    .await
}

/// A member of staff's decision on a suspended turn of an agent's
/// conversation.
pub(super) struct StaffDecision<'a> {
    pub lang: Lang,
    pub agent_id: &'a str,
    pub session_id: &'a str,
    pub turn_id: &'a str,
    pub request_id: Option<&'a str>,
    pub decision: chat::Decision,
    pub user_id: &'a str,
}

/// Claim the conversation and the pause for `decision`, and continue the turn
/// in the background: `202 {turn_id}`. The inbox's answer and the staff
/// resume route are this one path.
pub(super) async fn resume_as_staff(state: Arc<RamaState>, d: StaffDecision<'_>) -> Response {
    let Some(runner) = state.agent_runner.clone() else {
        return json_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "agent_runtime_unavailable",
            "this gateway cannot run agent conversations — answer again after it was updated",
        );
    };
    let Some(hold) = embed_rt::claim(&state.chats, d.agent_id, d.session_id, d.turn_id) else {
        return json_error(
            StatusCode::CONFLICT,
            "turn_in_progress",
            "the agent is busy with this conversation right now — answer again in a moment",
        );
    };
    let claimed = match claim(
        &state,
        AgentResume {
            agent_id: d.agent_id,
            session_id: d.session_id,
            turn_id: d.turn_id,
            request_id: d.request_id,
            decision: d.decision,
            by: ResumedBy::Staff {
                user_id: d.user_id.to_string(),
            },
        },
    )
    .await
    {
        Ok(claimed) => claimed,
        Err(err) => return resume_error(err, d.lang),
    };
    embed_rt::spawn_guarded(state, runner, hold, TurnWork::Resume(claimed));
    json_ok(
        StatusCode::ACCEPTED,
        Resumed {
            turn_id: d.turn_id.to_string(),
        },
    )
}

/// The one frame of the inbox event stream (SSE event `inbox`).
#[derive(serde::Serialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InboxEvent {
    /// How many items wait for the caller.
    Inbox { count: usize, answers: bool },
}

fn count_frame(count: usize, answers: bool) -> rama::bytes::Bytes {
    rama::bytes::Bytes::from(format!(
        "event: inbox\ndata: {}\n\n",
        json!(InboxEvent::Inbox { count, answers })
    ))
}

/// GET /api/v0/agents/inbox/events — `inbox {count}` on attach and whenever
/// the pending items change.
pub async fn events(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_, user) = or_return!(super::require_session_json(&state, &req).await);
    let viewer = Viewer::of(&state, &user);
    let ids = |items: &[inbox::InboxItem]| -> Vec<String> {
        items.iter().map(|i| i.id.clone()).collect()
    };
    let first = match inbox::list(&state, &viewer).await {
        Ok(items) => items,
        Err(err) => return internal(err),
    };
    let answers = match inbox::answers_for_published(&state, &viewer).await {
        Ok(answers) => answers,
        Err(err) => return internal(err),
    };
    let (tx, rx) = rama::futures::channel::mpsc::unbounded();
    let _ = tx.unbounded_send(Ok(count_frame(first.len(), answers)));
    let mut seen = ids(&first);
    tokio::spawn(async move {
        let started = tokio::time::Instant::now();
        let mut last_sent = started;
        while started.elapsed() < STREAM_LIMIT {
            tokio::time::sleep(EVENT_POLL).await;
            if tx.is_closed() {
                return;
            }
            match inbox::list(&state, &viewer).await {
                Ok(items) => {
                    let now = ids(&items);
                    if now != seen {
                        let _ = tx.unbounded_send(Ok(count_frame(items.len(), answers)));
                        seen = now;
                        last_sent = tokio::time::Instant::now();
                    }
                }
                Err(err) => tracing::warn!(error = %err, "inbox events: listing items"),
            }
            if last_sent.elapsed() >= super::SSE_KEEPALIVE {
                let _ = tx.unbounded_send(Ok(super::sse_keepalive()));
                last_sent = tokio::time::Instant::now();
            }
        }
    });
    json_stream_response(rx)
}

/// A notification channel, without its URL.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ChannelView {
    pub id: String,
    /// `slack` or `discord`.
    pub kind: &'static str,
    pub name: String,
    /// The host of the webhook URL; the URL itself is never shown.
    pub url_host: String,
    /// Messages name the question or the tool, not only the agent.
    pub details: bool,
    pub lang: String,
    pub created_by: String,
    pub created_at: jiff::Timestamp,
}

impl ChannelView {
    fn of(c: &agent_channels::Channel) -> Self {
        Self {
            id: c.id.clone(),
            kind: c.kind.as_str(),
            name: c.name.clone(),
            url_host: c.url_host.clone(),
            details: c.details,
            lang: c.lang.clone(),
            created_by: c.created_by.clone(),
            created_at: c.created_at,
        }
    }
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ChannelList {
    pub channels: Vec<ChannelView>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct CreatedChannel {
    pub channel: ChannelView,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChannelBody {
    /// `slack` or `discord`.
    pub kind: String,
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub details: bool,
    #[serde(default)]
    pub lang: Option<String>,
}

/// GET /api/v0/agents/{id}/channels — never a URL.
pub async fn channels(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let user = or_return!(require_agent_manager(&state, &req).await);
    let (agent, _) = or_return!(agent_at(&state, &req, &user, 1, Access::Read).await);
    match agent_channels::list(&state.db, &agent.principal.id).await {
        Ok(rows) => json_ok(
            StatusCode::OK,
            ChannelList {
                channels: rows.iter().map(ChannelView::of).collect(),
            },
        ),
        Err(err) => internal(err),
    }
}

/// POST /api/v0/agents/{id}/channels
pub async fn create_channel(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let user = or_return!(require_agent_manager(&state, &req).await);
    let (agent, _) = or_return!(agent_at(&state, &req, &user, 1, Access::Write).await);
    let body: ChannelBody = or_return!(super::read_json(req.into_body(), "the channel body").await);
    let Some(kind) = ChannelKind::parse(&body.kind) else {
        return bad_request(format!(
            "`{}` is not a channel kind — use `slack` or `discord`",
            body.kind
        ));
    };
    let name = body.name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
        return bad_request(format!(
            "a channel needs a `name` of 1 to {MAX_NAME_CHARS} characters"
        ));
    }
    let lang = body.lang.as_deref().unwrap_or("en");
    if Lang::from_code(lang).is_none() {
        return bad_request(format!(
            "`{lang}` is not a supported language — use en, de, fr, es, ru or zh"
        ));
    }
    let reach = inbox::channel_reach(&state, Some(&user));
    let host = match validate_webhook_url(kind, &body.url, reach) {
        Ok(host) => host,
        Err(why) => {
            return json_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid_webhook_url",
                &why,
            );
        }
    };
    let new = NewChannel {
        kind,
        name,
        url: body.url.trim(),
        url_host: &host,
        details: body.details,
        lang,
    };
    match agent_channels::create(
        &state.db,
        &state.crypto,
        &agent.principal.id,
        &new,
        &user.id,
    )
    .await
    {
        Ok(Some(channel)) => json_ok(
            StatusCode::CREATED,
            CreatedChannel {
                channel: ChannelView::of(&channel),
            },
        ),
        Ok(None) => json_error(
            StatusCode::CONFLICT,
            "channel_name_taken",
            &format!(
                "`{}` already has a channel named `{name}` — pick another name or delete that one",
                agent.principal.name
            ),
        ),
        Err(err) => internal(err),
    }
}

/// DELETE /api/v0/agents/{id}/channels/{channel_id}
pub async fn delete_channel(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let user = or_return!(require_agent_manager(&state, &req).await);
    let (agent, _) = or_return!(agent_at(&state, &req, &user, 2, Access::Write).await);
    let Some(channel_id) = raw_path_segment(&req, 0) else {
        return bad_request("the URL is missing the channel id");
    };
    match agent_channels::delete(&state.db, &agent.principal.id, &channel_id, &user.id).await {
        Ok(true) => no_content(),
        Ok(false) => not_found(format!(
            "`{}` has no channel `{channel_id}`",
            agent.principal.name
        )),
        Err(err) => internal(err),
    }
}
