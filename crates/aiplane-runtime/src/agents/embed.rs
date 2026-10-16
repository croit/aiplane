// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! What the public agent endpoint (`/api/v0/embed/*`, `docs/agent-visitors.md`)
//! needs from the runtime: the visitor-session settings read from an agent's
//! live spec, and the seam through which it runs one turn.
//!
//! The endpoint owns everything about the visitor — key, origin, token, TTL,
//! which conversation — and opens the turn rows. Running the turn as the
//! agent's principal is [`AgentTurnRunner`]'s job, so the endpoint never
//! touches the loop, grants or gates.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use session_core::SessionWorkers;
use session_core::workers::{ActiveWorker, RegisterOutcome, TurnUpdate};

use aiplane_agents::db::agent_audit::{AuditKind, Correlation, NewEvent};
use aiplane_agents::rates::{self, Inbound, Rate, RateExceeded, VisitorKey};
use aiplane_core::server::db::limits::{Dimension, EffectiveLimit, SubjectType, Window};
use aiplane_core::server::limits::LimitExceeded;
use jiff::{SignedDuration, Timestamp};
use serde_json::{Value, json};

use session_core::db as chat;

use super::profile::{AgentRunError, Role, RunOptions, RunProfile};
pub use super::resume::ClaimedResume;
use super::resume::run_claimed;
pub use super::run::OpenedTurn;
use super::run::drive_opened;
use super::spec::AgentSpec;
use crate::rama_server::state::RamaState;

/// A visitor session's idle TTL when the spec names no `publish.idle_ttl`.
pub const DEFAULT_IDLE_TTL: SignedDuration = SignedDuration::from_secs(30 * 60);

/// The absolute cap on one visitor session, however often it is used:
/// `sessionStorage` already ends it with the tab, and this ends a tab that
/// is never closed.
pub const MAX_VISITOR_SESSION: SignedDuration = SignedDuration::from_secs(24 * 60 * 60);

/// Messages one visitor session may send when the spec sets no
/// `publish.rate_limits.visitor`: a person typing, with room to spare.
pub const DEFAULT_VISITOR_RATE: Rate = Rate {
    max: 20,
    per: SignedDuration::from_secs(10 * 60),
};

/// Conversations started plus messages sent from one client IP when the spec
/// sets no `publish.rate_limits.ip`: an office behind one NAT, not a script.
pub const DEFAULT_IP_RATE: Rate = Rate {
    max: 60,
    per: SignedDuration::from_secs(10 * 60),
};

/// `publish.budget` of a spec as limits over the month: what the owner lets
/// the agent's conversations spend. None by default: an owner who sets no
/// budget relies on the operator's limits on the agent and its models.
pub fn owner_budget(spec: &AgentSpec) -> Vec<EffectiveLimit> {
    let monthly = |dimension, value: Option<f64>| {
        value.filter(|v| *v > 0.0).map(|value| EffectiveLimit {
            model: None,
            dimension,
            window: Window::Month,
            value,
            source: SubjectType::AgentSpec,
            is_total: true,
        })
    };
    let budget = &spec.publish.budget;
    [
        monthly(Dimension::Cost, budget.monthly_cost),
        monthly(Dimension::Tokens, budget.monthly_tokens.map(|t| t as f64)),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Who is asking: the visitor session once there is one, and the client IP
/// when the request carries one.
#[derive(Debug, Clone, Copy)]
pub struct Admission<'a> {
    pub visitor_id: Option<&'a str>,
    /// The A2A context asking, which counts like a visitor session.
    pub a2a_context: Option<&'a str>,
    pub ip: Option<&'a str>,
}

impl<'a> Admission<'a> {
    /// The conversation asking, whichever channel it came on.
    fn conversation(&self) -> Option<Inbound<'a>> {
        self.visitor_id
            .map(Inbound::Visitor)
            .or(self.a2a_context.map(Inbound::A2a))
    }
}

/// Why a visitor request was refused before it reached the agent.
#[derive(Debug, Clone)]
pub enum Refusal {
    /// Too many requests from this visitor or IP; wait `retry_after_secs`.
    Rate(RateExceeded),
    /// The agent's conversations have spent a budget; it is unavailable
    /// until usage leaves the window.
    Budget(LimitExceeded),
}

impl Refusal {
    pub fn retry_after_secs(&self) -> i64 {
        match self {
            Refusal::Rate(r) => r.retry_after_secs,
            Refusal::Budget(b) => b.retry_after_secs,
        }
    }

    /// What the agent's managers read in its audit trail and in the agents
    /// API: which limit, and the numbers behind it.
    pub fn detail(&self, visitor_id: Option<&str>) -> Value {
        match self {
            Refusal::Rate(r) => json!({
                "limit": format!("{}_rate", r.scope.as_str()),
                "visitor_id": visitor_id,
                "max": r.max,
                "per_secs": r.per.as_secs(),
                "retry_after_secs": r.retry_after_secs,
            }),
            Refusal::Budget(b) => json!(BudgetBreach::of(b)),
        }
    }
}

/// Who set a budget line: the agent's own spec, or the operator.
fn set_by(subject: SubjectType) -> &'static str {
    if subject == SubjectType::AgentSpec {
        "agent"
    } else {
        "operator"
    }
}

/// The budget breach as the owner reads it.
#[derive(Debug, Clone, serde::Serialize, schemars::JsonSchema)]
pub struct BudgetBreach {
    /// Always `budget`.
    pub limit: &'static str,
    /// `agent` (the spec's own budget) or `operator`.
    pub set_by: &'static str,
    pub dimension: &'static str,
    pub window: &'static str,
    pub max: f64,
    pub used: f64,
    pub retry_after_secs: i64,
}

impl BudgetBreach {
    pub fn of(b: &LimitExceeded) -> Self {
        Self {
            limit: "budget",
            set_by: set_by(b.subject),
            dimension: b.dimension.as_str(),
            window: b.window.as_str(),
            max: b.limit,
            used: b.used,
            retry_after_secs: b.retry_after_secs,
        }
    }
}

/// Gate one visitor request to `agent_id` on the agent's live
/// `publish.rate_limits` and budget, at `now` (a parameter so tests can
/// stand inside a window). An admitted request counts against the rates
/// (`rates::admit_visitor`), a refused one does not. Every refusal is
/// audited on the agent. An agent with no live version is not gated here;
/// the endpoint refuses it anyway.
pub async fn admit(
    state: &RamaState,
    agent_id: &str,
    who: Admission<'_>,
    now: Timestamp,
) -> Result<Admitted, Refusal> {
    let unrated = Admitted {
        agent_id: agent_id.to_string(),
        visitor_rate: None,
        at: now,
    };
    let compiled = match state.agent_specs.live_recent(&state.db, agent_id).await {
        Ok(Some(compiled)) => compiled,
        Ok(None) => return Ok(unrated),
        Err(err) => {
            tracing::warn!(error = %err, agent = agent_id, "reading the agent's limits; admitting");
            return Ok(unrated);
        }
    };
    let key = VisitorKey {
        principal_id: agent_id,
        conversation: who.conversation(),
        ip: who.ip,
    };
    // A version whose spec does not read cannot run; its visitors are held
    // to the default rates until the turn refuses them.
    let spec = compiled.agent().unwrap_or(AgentSpec::empty());
    let rates = spec.publish.visitor_rates();
    // The budget first: the rate counts what it admits, and a request the
    // budget refuses must not use up the visitor's rate.
    let refused = match state
        .enforcer
        .check_agent(agent_id, &owner_budget(spec), now)
        .await
    {
        Err(budget) => Refusal::Budget(budget),
        Ok(()) => match rates::admit_visitor(&state.db, &rates, &key, now).await {
            Err(rate) => Refusal::Rate(rate),
            Ok(()) => {
                return Ok(Admitted {
                    visitor_rate: Some(rates.visitor),
                    ..unrated
                });
            }
        },
    };
    let subject = who.visitor_id.or(who.a2a_context).or(who.ip).unwrap_or("");
    state
        .refusals
        .record(&state.db, agent_id, subject, refused.detail(who.visitor_id))
        .await;
    Err(refused)
}

/// A request [`admit`] let through.
#[derive(Debug)]
pub struct Admitted {
    agent_id: String,
    /// The per-conversation rate it was admitted under; `None` when no rate
    /// applied.
    visitor_rate: Option<Rate>,
    at: Timestamp,
}

impl Admitted {
    /// The request opened `conversation`, which had no id when it was
    /// admitted: count it there too, so the conversation's first message is
    /// in its window like every later one.
    pub async fn opened(&self, state: &RamaState, conversation: Inbound<'_>) {
        if let Some(rate) = self.visitor_rate {
            rates::count_opening(&state.db, &self.agent_id, rate, conversation, self.at).await;
        }
    }
}

const REFUSAL_WINDOW: std::time::Duration = std::time::Duration::from_secs(60);

/// How many refusal windows may be open at once. Past it, a refusal from a
/// subject with no open window is counted in its agent's overflow window
/// instead, so a storm from ever-new subjects (rotating IPs) cannot grow the
/// map without bound.
const MAX_OPEN_REFUSALS: usize = 10_000;

/// Keeps a flood of refused requests from becoming a flood of database
/// work: one event per (agent, subject, class) when a window opens, with
/// `count: 1`, and — when more were refused in it — one more as the window
/// closes, whose `count` is the rest and whose `folds` names the first. The
/// log is append-only, so the count is never written back into the first
/// event. One sweeper task, started with the first refusal, closes the
/// windows that have run their course. A visitor refused by a limit
/// (`limit_refused`, class = the limit) and a host identity token refused
/// for a conversation (`host_identity`, class = the reason) both go through
/// it.
#[derive(Clone)]
pub struct RefusalAudit {
    window: std::time::Duration,
    max_open: usize,
    open: Arc<Mutex<HashMap<RefusalKey, OpenRefusal>>>,
    sweeping: Arc<AtomicBool>,
}

/// `(agent, subject, class)`: one open refusal window per triple. The
/// overflow window of an agent has the empty subject.
type RefusalKey = (String, String, String);

/// One refused request, as [`RefusalAudit`] counts it.
pub struct Refused<'a> {
    pub agent_id: &'a str,
    /// Who was refused: a visitor, an A2A context, a client IP, or a
    /// conversation.
    pub subject: &'a str,
    /// What refused it: one window per agent, subject and class.
    pub class: String,
    pub kind: AuditKind,
    /// The conversation whose chain takes the events; `None` for the
    /// agent's own chain.
    pub conversation_id: Option<&'a str>,
    pub detail: Value,
}

struct OpenRefusal {
    /// The first refusal's detail; its `window_id` is what the closing
    /// event's `folds` names.
    detail: Value,
    kind: AuditKind,
    conversation_id: Option<String>,
    count: u64,
    opened: std::time::Instant,
}

impl Default for RefusalAudit {
    fn default() -> Self {
        Self::with_window(REFUSAL_WINDOW, MAX_OPEN_REFUSALS)
    }
}

impl RefusalAudit {
    pub fn with_window(window: std::time::Duration, max_open: usize) -> Self {
        Self {
            window,
            max_open,
            open: Arc::default(),
            sweeping: Arc::default(),
        }
    }

    /// A visitor request refused by a limit, as `limit_refused` on the
    /// agent's own chain.
    async fn record(
        &self,
        db: &aiplane_core::server::db::Pool,
        agent_id: &str,
        subject: &str,
        detail: Value,
    ) {
        let class = detail["limit"].as_str().unwrap_or("unknown").to_string();
        self.fold(
            db,
            Refused {
                agent_id,
                subject,
                class,
                kind: AuditKind::LimitRefused,
                conversation_id: None,
                detail,
            },
        )
        .await;
    }

    pub async fn fold(&self, db: &aiplane_core::server::db::Pool, refused: Refused<'_>) {
        let Refused {
            agent_id,
            subject,
            class,
            kind,
            mut detail,
            ..
        } = refused;
        let mut conversation_id = refused.conversation_id.map(str::to_string);
        let mut key = (agent_id.to_string(), subject.to_string(), class);
        {
            let mut open = self.open.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(entry) = open.get_mut(&key) {
                entry.count += 1;
                return;
            }
            if open.len() >= self.max_open {
                key.1 = String::new();
                detail["visitor_id"] = Value::Null;
                conversation_id = None;
                if let Some(entry) = open.get_mut(&key) {
                    entry.count += 1;
                    return;
                }
            }
            let window_id = uuid::Uuid::new_v4().to_string();
            detail["count"] = json!(1);
            detail["window_id"] = json!(window_id);
            open.insert(
                key,
                OpenRefusal {
                    detail: detail.clone(),
                    kind,
                    conversation_id: conversation_id.clone(),
                    count: 1,
                    opened: std::time::Instant::now(),
                },
            );
        }
        refusal_event(db, agent_id, kind, conversation_id.as_deref(), detail).await;
        self.start_sweeper(db);
    }

    /// Close every window older than `window` and write its final count,
    /// every half window, for as long as this audit exists.
    fn start_sweeper(&self, db: &aiplane_core::server::db::Pool) {
        if self.sweeping.swap(true, Ordering::AcqRel) {
            return;
        }
        let (open, window, db) = (Arc::downgrade(&self.open), self.window, db.clone());
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(window / 2);
            loop {
                tick.tick().await;
                let Some(open) = open.upgrade() else {
                    return;
                };
                let closed: Vec<(RefusalKey, OpenRefusal)> = open
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .extract_if(|_, e| e.opened.elapsed() >= window)
                    .collect();
                drop(open);
                for (key, entry) in closed.into_iter().filter(|(_, e)| e.count > 1) {
                    let mut detail = entry.detail;
                    detail["folds"] = detail["window_id"].take();
                    detail["count"] = json!(entry.count - 1);
                    refusal_event(
                        &db,
                        &key.0,
                        entry.kind,
                        entry.conversation_id.as_deref(),
                        detail,
                    )
                    .await;
                }
            }
        });
    }
}

async fn refusal_event(
    db: &aiplane_core::server::db::Pool,
    agent_id: &str,
    kind: AuditKind,
    conversation_id: Option<&str>,
    detail: Value,
) {
    let event = NewEvent::new(kind, agent_id, detail).at(Correlation {
        session_id: conversation_id.map(str::to_string),
        ..Correlation::default()
    });
    let _ = super::audit::record_event(db, event).await;
}

/// What an agent's managers see of its limits under the live spec `live`
/// (`None` before it is published): the visitor rates and retention in
/// force, each budget with what has been spent against it, and whether the
/// agent is turning visitors away right now and why.
pub async fn limits_view(
    state: &RamaState,
    agent_id: &str,
    live: Option<&AgentSpec>,
    now: Timestamp,
) -> LimitsView {
    let spec = live.unwrap_or(AgentSpec::empty());
    let rates = spec.publish.visitor_rates();
    let rate = |r: Rate| RateView {
        max: r.max,
        per_secs: r.per.as_secs(),
    };
    let statuses = state
        .enforcer
        .agent_statuses(agent_id, &owner_budget(spec), now)
        .await;
    let budget = statuses
        .iter()
        .map(|s| BudgetLine {
            set_by: set_by(s.source),
            dimension: s.dimension.as_str(),
            window: s.window.as_str(),
            max: s.limit,
            used: s.used,
            exceeded: s.exceeded(),
            refreshes_at: s.refreshes_at,
        })
        .collect();
    let exhausted = state
        .enforcer
        .check_agent(agent_id, &owner_budget(spec), now)
        .await
        .err();
    LimitsView {
        rate_limits: RateLimitsView {
            visitor: rate(rates.visitor),
            ip: rate(rates.ip),
        },
        retention_days: spec.publish.retention_days(),
        audit_retention_days: spec.publish.audit_retention_days(),
        budget,
        available: exhausted.is_none(),
        unavailable_reason: exhausted.as_ref().map(BudgetBreach::of),
    }
}

/// An agent's limits as its managers see them: the live version's visitor
/// rates and retention, and its budget with what has been spent against it.
#[derive(Debug, Clone, serde::Serialize, schemars::JsonSchema)]
#[schemars(rename = "AgentLimitsView")]
pub struct LimitsView {
    pub rate_limits: RateLimitsView,
    pub retention_days: i64,
    pub audit_retention_days: i64,
    pub budget: Vec<BudgetLine>,
    /// False while a budget is spent; `unavailable_reason` says which.
    pub available: bool,
    pub unavailable_reason: Option<BudgetBreach>,
}

#[derive(Debug, Clone, serde::Serialize, schemars::JsonSchema)]
pub struct RateLimitsView {
    /// Per visitor.
    pub visitor: RateView,
    /// Per client IP.
    pub ip: RateView,
}

/// At most `max` requests per `per_secs` seconds.
#[derive(Debug, Clone, serde::Serialize, schemars::JsonSchema)]
pub struct RateView {
    pub max: u32,
    pub per_secs: i64,
}

#[derive(Debug, Clone, serde::Serialize, schemars::JsonSchema)]
pub struct BudgetLine {
    /// `agent` (the spec's own budget) or `operator`.
    pub set_by: &'static str,
    pub dimension: &'static str,
    pub window: &'static str,
    pub max: f64,
    pub used: f64,
    pub exceeded: bool,
    /// When the sliding window next advances.
    pub refreshes_at: Timestamp,
}

/// Runs one opened turn of an agent conversation as the agent's principal.
///
/// Contract:
/// - The turn runs as `agent_id` with exactly its grants, on `version` (the
///   version the conversation started on), never as a person.
/// - Output is buffered (`OutputPolicy::Buffered`): the endpoint shows a
///   visitor an assistant answer only once its turn is terminal, so a runner
///   may write partial content as it likes.
/// - When `run` returns, `turn_id` is terminal or suspended (a tool paused
///   it for a decision; see [`crate::agents::resume`]). The endpoint errors a
///   turn left `in_progress`, so a runner that crashes cannot wedge the
///   conversation.
#[async_trait::async_trait]
pub trait AgentTurnRunner: Send + Sync {
    async fn run(&self, state: Arc<RamaState>, turn: OpenedTurn);

    /// Continue a suspended conversation whose decision won the claim. Same
    /// contract as [`Self::run`]: when this returns, the conversation's turn
    /// is terminal or suspended again, never `in_progress`. The run speaks
    /// the conversation's language, not the one of whoever decided.
    async fn resume(&self, state: Arc<RamaState>, claimed: ClaimedResume) {
        let turn = claimed.turn_id().to_string();
        if let Err(err) = run_claimed(&state, claimed, RunOptions::default()).await {
            tracing::warn!(error = %err, %turn, "a suspended visitor turn could not resume");
        }
    }
}

/// The production runner: the agent entry point of `agents::run`.
#[derive(Debug, Clone, Copy, Default)]
pub struct LiveAgentRunner;

#[async_trait::async_trait]
impl AgentTurnRunner for LiveAgentRunner {
    async fn run(&self, state: Arc<RamaState>, turn: OpenedTurn) {
        let profile = RunProfile::load_version(
            &state,
            &turn.agent_id,
            Some(turn.version),
            Role::Main,
            &RunOptions::default(),
        )
        .await;
        drive_or_error(&state, profile, &turn).await;
    }
}

/// Drive `turn` as `profile`, or record why it could not run on the turn.
async fn drive_or_error(
    state: &Arc<RamaState>,
    profile: Result<RunProfile, AgentRunError>,
    turn: &OpenedTurn,
) {
    let ran = match profile {
        Ok(profile) => drive_opened(state, &profile, turn).await.map(|_| ()),
        Err(err) => Err(err),
    };
    if let Err(err) = ran {
        tracing::warn!(error = %err, turn = %turn.turn_id, "agent turn could not run");
        if let Err(db) = chat::finalize_turn(
            &state.db,
            &turn.turn_id,
            chat::TurnStatus::Errored,
            Some(&err.to_string()),
        )
        .await
        {
            tracing::warn!(error = %db, turn = %turn.turn_id, "recording a failed agent turn");
        }
    }
}

/// A turn to produce in the background: a fresh one, a test-chat turn on the
/// agent's draft, or a suspended one whose decision won the claim.
pub enum TurnWork {
    Run(OpenedTurn),
    /// The profile prepared by authenticated private admission; public callers
    /// cannot load this Organization source through the ordinary runner.
    Private(Box<RunProfile>, OpenedTurn),
    Resume(ClaimedResume),
}

impl TurnWork {
    fn ids(&self) -> (String, String) {
        match self {
            Self::Run(turn) | Self::Private(_, turn) => {
                (turn.session_id.clone(), turn.turn_id.clone())
            }
            Self::Resume(claimed) => (
                claimed.session_id().to_string(),
                claimed.turn_id().to_string(),
            ),
        }
    }
}

/// Produce `work` on its own task under `claim`, so the turn finishes even
/// if the request that started it goes away. The claim is released only once
/// the turn is settled: a runner that panicked or broke its contract leaves
/// the turn errored, never `in_progress` behind a released claim.
pub fn spawn_guarded(
    state: Arc<RamaState>,
    runner: Arc<dyn AgentTurnRunner>,
    claim: TurnClaim,
    work: TurnWork,
) -> tokio::task::JoinHandle<()> {
    let (session_id, turn_id) = work.ids();
    tokio::spawn(async move {
        let _claim = claim;
        let run = tokio::spawn({
            let state = state.clone();
            async move {
                match work {
                    TurnWork::Run(turn) => runner.run(state, turn).await,
                    TurnWork::Private(profile, turn) => {
                        drive_or_error(&state, Ok(*profile), &turn).await
                    }
                    TurnWork::Resume(claimed) => runner.resume(state, claimed).await,
                }
            }
        });
        if let Err(err) = run.await {
            tracing::error!(error = %err, turn = %turn_id, "agent turn run panicked");
        }
        settle_unfinished(&state, &session_id, &turn_id).await;
    })
}

/// The runner contract says the turn is terminal when `run` returns; a
/// runner that broke it (or panicked) must not leave the conversation waiting
/// forever with every later message refused as `turn_in_progress`.
async fn settle_unfinished(state: &RamaState, session_id: &str, turn_id: &str) {
    match chat::get_turn(&state.db, session_id, turn_id).await {
        Ok(Some(t)) if t.status == chat::TurnStatus::InProgress => {
            tracing::warn!(turn = %turn_id, "agent turn runner returned with the turn unfinished");
            if let Err(err) = chat::finalize_turn(
                &state.db,
                turn_id,
                chat::TurnStatus::Errored,
                Some("the agent run ended without finishing its turn"),
            )
            .await
            {
                tracing::warn!(error = %err, turn = %turn_id, "erroring an unfinished agent turn");
            }
        }
        Ok(_) => {}
        Err(err) => tracing::warn!(error = %err, turn = %turn_id, "reading an agent turn"),
    }
}

/// A claim on an agent conversation: its worker in the session worker
/// registry (`RamaState::chats`), the one place every running turn — a
/// person's chat or an agent's — is known, cancelled and waited for. Agent
/// turns are keyed by the principal that owns the conversation, and are not
/// capped per principal: one agent answers many visitors at once. The worker
/// leaves the registry when the claim drops, which is what a stream waiting
/// for the answer ([`released`]) wakes on.
pub struct TurnClaim {
    workers: Arc<SessionWorkers>,
    principal_id: String,
    worker: ActiveWorker,
}

/// Claim `principal_id`'s conversation `session_id` for its turn `turn_id`.
/// `None` when a turn already holds it; the claim ends when the returned
/// guard drops.
pub fn claim(
    workers: &Arc<SessionWorkers>,
    principal_id: &str,
    session_id: &str,
    turn_id: &str,
) -> Option<TurnClaim> {
    match workers.register(principal_id, turn_id, session_id, usize::MAX) {
        RegisterOutcome::Registered { worker } => Some(TurnClaim {
            workers: workers.clone(),
            principal_id: principal_id.to_string(),
            worker,
        }),
        RegisterOutcome::Busy { .. } | RegisterOutcome::AtCapacity { .. } => None,
    }
}

impl TurnClaim {
    /// The worker this claim registered: its cancel flag and the channel its
    /// turn's driver reports on.
    pub fn worker(&self) -> &ActiveWorker {
        &self.worker
    }
}

impl Drop for TurnClaim {
    fn drop(&mut self) {
        self.workers.clear(&self.principal_id, &self.worker);
    }
}

/// Resolves once no worker holds turn `turn_id` of `principal_id`'s
/// conversation `session_id` — at once when none does. An agent turn's
/// worker is released only after the output filter has ruled on its answer,
/// so whatever the turn's row says then is what may be delivered.
pub async fn released(
    workers: &SessionWorkers,
    principal_id: &str,
    session_id: &str,
    turn_id: &str,
) {
    use tokio::sync::broadcast::error::RecvError;
    loop {
        let Some((holder, mut updates)) = workers.subscribe(principal_id, session_id) else {
            return;
        };
        if holder != turn_id {
            return;
        }
        loop {
            match updates.recv().await {
                Ok(TurnUpdate::Released) | Err(RecvError::Closed) => return,
                Ok(_) => {}
                // A missed frame may have been the release: look again.
                Err(RecvError::Lagged(_)) => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::spec::model::Publish;
    use serde_json::json;

    fn publish(spec: Value) -> Publish {
        AgentSpec::from_value(&spec).unwrap().publish
    }

    #[test]
    fn the_idle_ttl_comes_from_the_publish_settings_with_a_30_minute_default() {
        assert_eq!(
            publish(json!({ "publish": { "idle_ttl": "45m" } })).idle_ttl(),
            SignedDuration::from_secs(45 * 60)
        );
        assert_eq!(
            publish(json!({ "publish": {} })).idle_ttl(),
            DEFAULT_IDLE_TTL
        );
        assert_eq!(publish(json!({})).idle_ttl(), DEFAULT_IDLE_TTL);
        assert!(
            AgentSpec::from_value(&json!({ "publish": { "idle_ttl": "soon" } })).is_err(),
            "a duration that does not parse is refused, never a silent default"
        );
        assert_eq!(DEFAULT_IDLE_TTL, SignedDuration::from_secs(1800));
    }

    #[test]
    fn the_specs_origins_narrow_only_when_it_lists_some() {
        let open = publish(json!({ "publish": {} }));
        assert!(open.allows_origin("https://any.example"));
        let listed = publish(json!({ "publish": { "origins": ["https://a.example"] } }));
        assert!(listed.allows_origin("https://a.example"));
        assert!(!listed.allows_origin("https://b.example"));
        let empty = publish(json!({ "publish": { "origins": [] } }));
        assert!(!empty.allows_origin("https://a.example"));
    }

    #[test]
    fn visitor_rates_come_from_the_publish_settings_scope_by_scope() {
        let set = publish(json!({ "publish": { "rate_limits": {
            "visitor": { "max": 3, "per": "1m" }
        } } }))
        .visitor_rates();
        assert_eq!(
            set.visitor,
            Rate {
                max: 3,
                per: SignedDuration::from_secs(60)
            }
        );
        assert_eq!(set.ip, DEFAULT_IP_RATE, "an unset scope keeps its default");
        let none = publish(json!({})).visitor_rates();
        assert_eq!(none.visitor, DEFAULT_VISITOR_RATE);
        assert_eq!(DEFAULT_VISITOR_RATE.max, 20);
        assert_eq!(DEFAULT_IP_RATE.max, 60);
        let half = json!({ "publish": { "rate_limits": { "visitor": { "max": 3 } } } });
        assert!(
            AgentSpec::from_value(&half).is_err(),
            "a rate is both or neither"
        );
    }

    #[test]
    fn the_owner_budget_is_a_monthly_ceiling_per_dimension_and_none_by_default() {
        assert!(owner_budget(AgentSpec::empty()).is_empty());
        let both = owner_budget(
            &AgentSpec::from_value(&json!({ "publish": { "budget": {
                "monthly_cost": 25.5, "monthly_tokens": 1000
            } } }))
            .unwrap(),
        );
        let cells: Vec<(Dimension, Window, f64, SubjectType)> = both
            .iter()
            .map(|l| (l.dimension, l.window, l.value, l.source))
            .collect();
        assert_eq!(
            cells,
            [
                (Dimension::Cost, Window::Month, 25.5, SubjectType::AgentSpec),
                (
                    Dimension::Tokens,
                    Window::Month,
                    1000.0,
                    SubjectType::AgentSpec
                ),
            ]
        );
    }

    #[test]
    fn a_conversation_holds_one_claim_at_a_time() {
        let workers = Arc::new(SessionWorkers::default());
        let first = claim(&workers, "agent", "s1", "t1").expect("free");
        assert!(
            claim(&workers, "agent", "s1", "t2").is_none(),
            "already running"
        );
        assert!(
            claim(&workers, "agent", "s2", "t3").is_some(),
            "other conversations are free"
        );
        assert!(workers.holds("agent", "s1", "t1"));
        drop(first);
        assert!(!workers.holds("agent", "s1", "t1"));
        assert!(
            claim(&workers, "agent", "s1", "t2").is_some(),
            "released on drop"
        );
    }

    #[test]
    fn one_agent_answers_any_number_of_conversations_at_once() {
        let workers = Arc::new(SessionWorkers::default());
        let claims: Vec<TurnClaim> = (0..20)
            .map(|i| claim(&workers, "agent", &format!("s{i}"), "t").expect("no cap"))
            .collect();
        assert_eq!(workers.running_for_user("agent"), 20);
        drop(claims);
        assert_eq!(workers.active_count(), 0);
    }

    #[tokio::test]
    async fn releasing_a_claim_wakes_whoever_waits_for_its_turn() {
        let workers = Arc::new(SessionWorkers::default());
        let held = claim(&workers, "agent", "s1", "t1").expect("free");
        let waiter = tokio::spawn({
            let workers = workers.clone();
            async move { released(&workers, "agent", "s1", "t1").await }
        });
        tokio::task::yield_now().await;
        assert!(!waiter.is_finished(), "nothing was released yet");
        drop(held);
        tokio::time::timeout(std::time::Duration::from_secs(5), waiter)
            .await
            .expect("the release wakes the waiter")
            .unwrap();
    }

    #[tokio::test]
    async fn waiting_for_a_turn_nothing_holds_returns_at_once() {
        let workers = Arc::new(SessionWorkers::default());
        released(&workers, "agent", "s1", "t1").await;
        let _other = claim(&workers, "agent", "s1", "t2").expect("free");
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            released(&workers, "agent", "s1", "t1"),
        )
        .await
        .expect("a later turn of the conversation is not the one waited for");
    }

    #[test]
    fn only_the_claimed_turn_can_be_asked_to_stop() {
        let workers = Arc::new(SessionWorkers::default());
        assert!(!workers.cancel_turn("agent", "s1", "t1"), "nothing runs");
        let held = claim(&workers, "agent", "s1", "t1").expect("free");
        assert!(!workers.cancel_turn("agent", "s1", "t0"), "another turn");
        assert!(!held.worker().cancel.load(Ordering::SeqCst));
        assert!(workers.cancel_turn("agent", "s1", "t1"));
        assert!(
            held.worker().cancel.load(Ordering::SeqCst),
            "the driver sees it"
        );
        drop(held);
        let again = claim(&workers, "agent", "s1", "t2").expect("free again");
        assert!(
            !again.worker().cancel.load(Ordering::SeqCst),
            "a new turn starts with a fresh flag"
        );
    }

    #[tokio::test]
    async fn releasing_one_conversation_does_not_wake_another() {
        let workers = Arc::new(SessionWorkers::default());
        let a = claim(&workers, "agent", "a", "t1").expect("free");
        let _b = claim(&workers, "agent", "b", "t2").expect("free");
        let wb = released(&workers, "agent", "b", "t2");
        drop(a);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), wb)
                .await
                .is_err(),
            "b's claim is still held"
        );
    }

    #[tokio::test]
    async fn a_flood_of_refusals_writes_one_event_per_window_and_one_with_the_count() {
        let db = aiplane_core::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        let audit = RefusalAudit::with_window(std::time::Duration::from_millis(100), 100);
        let detail = json!({ "limit": "visitor_rate", "max": 2 });
        for _ in 0..1000 {
            audit
                .record(&db, "agent", "visitor-1", detail.clone())
                .await;
        }
        audit
            .record(&db, "agent", "visitor-2", detail.clone())
            .await;
        assert_eq!(rows(&db).await.len(), 2, "one per subject, not per request");

        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        let mut counts: Vec<i64> = rows(&db).await;
        counts.sort();
        assert_eq!(
            counts,
            [1, 1, 999],
            "the window's first event, and its closing event with the rest"
        );

        audit.record(&db, "agent", "visitor-1", detail).await;
        assert_eq!(rows(&db).await.len(), 4, "the next window audits again");
    }

    #[tokio::test]
    async fn a_storm_from_ever_new_subjects_stays_within_the_cap() {
        let db = aiplane_core::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        let audit = RefusalAudit::with_window(std::time::Duration::from_millis(100), 2);
        for subject in 0..50 {
            let detail = json!({ "limit": "ip_rate", "visitor_id": format!("v{subject}") });
            audit
                .record(&db, "agent", &subject.to_string(), detail)
                .await;
        }
        assert_eq!(
            audit.open.lock().unwrap().len(),
            3,
            "two subjects, then the agent's overflow window"
        );
        assert_eq!(rows(&db).await.len(), 3);

        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        assert!(
            audit.open.lock().unwrap().is_empty(),
            "the sweeper closed them"
        );
        let mut counts = rows(&db).await;
        counts.sort();
        assert_eq!(
            counts,
            [1, 1, 1, 47],
            "every refusal is still counted, the overflow window's rest in a closing event"
        );
        let events = aiplane_agents::db::agent_audit::for_principal(&db, "agent")
            .await
            .unwrap();
        let closing = events.iter().find(|e| e.detail["count"] == 47).unwrap();
        assert!(
            closing.detail["visitor_id"].is_null(),
            "the overflow window names no one visitor"
        );
        let opened = events
            .iter()
            .find(|e| e.detail["window_id"] == closing.detail["folds"])
            .expect("the closing event names the window's first");
        assert_eq!(opened.detail["count"], 1);
        assert!(
            aiplane_agents::db::agent_audit::verify(&db, "agent")
                .await
                .unwrap()
                .ok(),
            "nothing was written back into an event"
        );
    }

    async fn rows(db: &aiplane_core::server::db::Pool) -> Vec<i64> {
        aiplane_agents::db::agent_audit::for_principal(db, "agent")
            .await
            .unwrap()
            .into_iter()
            .filter(|e| e.kind == "limit_refused")
            .map(|e| e.detail["count"].as_i64().unwrap())
            .collect()
    }
}
