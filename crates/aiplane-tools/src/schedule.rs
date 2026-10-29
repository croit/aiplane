// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! `schedule_action` / `list_scheduled_actions` / `delete_scheduled_action` —
//! let the model set up recurring prompts on the user's behalf.
//!
//! The cron stack itself is not new: the `/scheduled` page, the
//! `scheduled_actions` table and the worker that fires due actions have all
//! been running. What was missing was any way for the model to reach it, so
//! "remind me about the backup report on Mondays" could only be done by the
//! user clicking through a form — in the middle of a conversation where the
//! model already knew exactly what to schedule.
//!
//! ## Why these tools are careful
//!
//! A scheduled action later runs **as the user**, unattended, forever. That
//! makes it a different kind of side effect from every other tool here: a
//! model steered by an attacker-controlled web page or attachment could
//! otherwise plant a recurring prompt that outlives the conversation it was
//! injected into. The `/webhooks` page takes the same position for the same
//! reason (its actions default to tools off, because an anonymous caller
//! feeds text to a model running as you).
//!
//! So:
//!
//!   * **Tools are always off** for an action created here. The action can
//!     ask the model to write something; it cannot make it act. Turning tools
//!     on is a deliberate click on `/scheduled`.
//!   * **Creating and deleting need a person's approval**, through the
//!     durable approval every tool uses ([`approval`], the protocol of
//!     `AskFirst`): the call pauses the turn as an `approval` saying what would
//!     be scheduled, the person answers in the chat or the inbox, and only an
//!     approval runs it — after a restart too. The arguments are checked
//!     first, so nobody is asked about a schedule that cannot be saved. Where
//!     nothing can pause (`/v1`, kept off its tool list by
//!     `requires_chat_session` anyway) the write is refused. A scheduled run
//!     that calls them pauses like any other, and its owner decides in the
//!     inbox.
//!   * **Everything is scoped to `ctx.user_id`**, never to an id from the
//!     arguments — the store's functions all take the owner as a parameter.
//!
//! Listing is read-only and stays available everywhere.
//!
//! ## Why the response carries a preview
//!
//! `Cron::describe()` plus the next three run times are the same confirmation
//! the UI shows. Without them the model can only echo the cron expression back
//! at the user, and a wrong-but-valid expression (`0 0 * * 0` when they said
//! Monday) stays undetected until the first missed run.

use serde::Deserialize;
use serde_json::{Value, json};
use shared::api::ToolDef;

use aiplane_core::server::db::users;
use aiplane_runtime::server::scheduled::{self, NewAction, cron::Cron};
use aiplane_runtime::server::tools::{Tool, ToolContext, ToolError, ToolFuture};
use jiff::Timestamp;
use jiff::tz::TimeZone;

use aiplane_runtime::server::tools::ask_first::{Approval, DEFAULT_APPROVAL_TIMEOUT, approval};
use aiplane_runtime::suspend::SuspendRequest;

/// Bounds mirroring the `/scheduled` form's own validation, so an action
/// created here is one the edit form can also round-trip.
const MAX_NAME_LEN: usize = 128;
const MAX_PROMPT_LEN: usize = 8000;

/// How many upcoming run times to return. Three is what the UI previews:
/// enough to see the *pattern* (daily vs weekdays vs Mondays), which is what
/// catches a wrong expression.
const PREVIEW_RUNS: usize = 3;

// ---------------------------------------------------------------------------
// shared helpers

/// The pause for an approval of this call, with `preview` — what it would do —
/// shown next to the decision.
fn approval_request(preview: String) -> SuspendRequest {
    SuspendRequest {
        message: Some(preview),
        ..SuspendRequest::approval(DEFAULT_APPROVAL_TIMEOUT)
    }
}

/// The user's stored timezone, or UTC. Same source `get_current_timestamp`
/// reads, so "every day at 07:00" means the same thing in both tools.
async fn default_timezone(ctx: &ToolContext) -> String {
    users::find_by_id(&ctx.db, ctx.principal.subject_id())
        .await
        .ok()
        .flatten()
        .and_then(|u| u.timezone)
        .unwrap_or_else(|| "UTC".to_string())
}

fn resolve_tz(name: &str) -> Result<TimeZone, ToolError> {
    TimeZone::get(name).map_err(|_| {
        ToolError::InvalidArgs(format!(
            "unknown timezone `{name}` — use an IANA name like `Europe/Berlin` or `UTC`"
        ))
    })
}

/// Render an action for the model: the fields it needs to describe or change
/// the schedule, plus the same human preview the UI shows.
fn action_json(action: &scheduled::ScheduledAction) -> Value {
    let tz = TimeZone::get(&action.timezone).unwrap_or(TimeZone::UTC);
    let parsed = Cron::parse(&action.cron).ok();
    let upcoming: Vec<String> = parsed
        .as_ref()
        .map(|c| {
            c.upcoming(Timestamp::now(), &tz, PREVIEW_RUNS)
                .iter()
                .map(|t| {
                    t.to_zoned(tz.clone())
                        .strftime("%Y-%m-%d %H:%M")
                        .to_string()
                })
                .collect()
        })
        .unwrap_or_default();
    json!({
        "id": action.id,
        "name": action.name,
        "prompt": action.prompt,
        "model": action.model,
        "cron": action.cron,
        "timezone": action.timezone,
        "schedule": parsed.as_ref().map(Cron::describe),
        "next_runs": upcoming,
        "enabled": action.enabled,
        "tools_enabled": action.tools_enabled,
        "last_run_at": action.last_run_at.map(|t| t.to_string()),
        "last_status": action.last_status,
    })
}

// ---------------------------------------------------------------------------
// schedule_action

pub struct ScheduleAction;

#[derive(Deserialize)]
struct CreateArgs {
    name: String,
    prompt: String,
    cron: String,
    #[serde(default)]
    timezone: Option<String>,
}

impl Tool for ScheduleAction {
    fn id(&self) -> &str {
        "schedule_action"
    }

    fn schema(&self) -> ToolDef {
        ToolDef::function(
            self.id(),
            "Schedule a prompt to run automatically on a recurring schedule — \
             \"every Monday at 8, summarise last week's tickets\", \"remind me \
             about the backup report on the first of the month\". Each run opens \
             a conversation the user can read afterwards, using the model you are \
             running on now. \
             \
             The user has to approve before anything is saved, so use this when \
             they asked for something recurring — not to set yourself reminders. \
             The scheduled run has NO tools available (it can write, not act); if \
             the task needs tools, say so and point the user at the /scheduled \
             page. Say the schedule back to the user using the `schedule` and \
             `next_runs` in the result, so a wrong day or hour gets caught now \
             rather than at the first run.",
            json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["name", "prompt", "cron"],
                "properties": {
                    "name": {
                        "type": "string",
                        "description": format!(
                            "Short label the user will recognise in their list, e.g. \
                             \"Weekly ticket summary\". Max {MAX_NAME_LEN} characters."
                        )
                    },
                    "prompt": {
                        "type": "string",
                        "description": "The message to send at each run. Write it as a \
                                        standalone instruction — the run starts a fresh \
                                        conversation and cannot see this one, so include \
                                        every detail it needs."
                    },
                    "cron": {
                        "type": "string",
                        "description": "5-field cron expression: minute hour day-of-month \
                                        month day-of-week. `0 8 * * 1` = Mondays at 08:00, \
                                        `30 6 * * 1-5` = weekdays at 06:30, `0 9 1 * *` = \
                                        the 1st of each month at 09:00. Ranges, lists and \
                                        `*/n` steps are supported. Day-of-week: 0 or 7 = \
                                        Sunday, 1 = Monday."
                    },
                    "timezone": {
                        "type": "string",
                        "description": "IANA timezone the schedule is evaluated in, e.g. \
                                        `Europe/Berlin`. Defaults to the user's own \
                                        timezone — only set it if they ask for another."
                    }
                }
            }),
        )
    }

    fn run<'a>(&'a self, ctx: ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let args: CreateArgs = serde_json::from_value(args).map_err(|e| {
                ToolError::InvalidArgs(format!(
                    "expected {{name: string, prompt: string, cron: string, \
                     timezone?: string}}: {e}"
                ))
            })?;

            let name = args.name.trim();
            if name.is_empty() || name.chars().count() > MAX_NAME_LEN {
                return Err(ToolError::InvalidArgs(format!(
                    "`name` must be 1-{MAX_NAME_LEN} characters"
                )));
            }
            let prompt = args.prompt.trim();
            if prompt.is_empty() || prompt.chars().count() > MAX_PROMPT_LEN {
                return Err(ToolError::InvalidArgs(format!(
                    "`prompt` must be 1-{MAX_PROMPT_LEN} characters"
                )));
            }

            // The model this turn runs on. Without it we'd have to invent a
            // pool id, and an action pointing at a model that doesn't exist
            // fails silently at 06:00 rather than here.
            let model = ctx.model.clone().ok_or_else(|| {
                ToolError::Failed(
                    "this request path doesn't know which model it is running, so the \
                     action would have no model to run with. Ask the user to create it \
                     on the /scheduled page."
                        .into(),
                )
            })?;

            let timezone = match args.timezone.as_deref().map(str::trim) {
                Some(tz) if !tz.is_empty() => tz.to_string(),
                _ => default_timezone(&ctx).await,
            };
            let tz = resolve_tz(&timezone)?;

            // Parse before asking for confirmation: a bad expression is the
            // model's problem to fix, and the error names the field, so it can
            // correct itself instead of guessing. Bothering the user with a
            // card for a prompt that can't be saved would be worse.
            let cron = Cron::parse(&args.cron).map_err(|e| {
                ToolError::InvalidArgs(format!(
                    "{e}. Fields are: minute hour day-of-month month day-of-week."
                ))
            })?;
            let next_runs = cron.upcoming(Timestamp::now(), &tz, PREVIEW_RUNS);
            if next_runs.is_empty() {
                return Err(ToolError::InvalidArgs(format!(
                    "`{}` never occurs (e.g. February 30th), so it would never run. \
                     Pick a schedule that has a next occurrence.",
                    cron.as_str()
                )));
            }
            let pretty: Vec<String> = next_runs
                .iter()
                .map(|t| {
                    t.to_zoned(tz.clone())
                        .strftime("%Y-%m-%d %H:%M")
                        .to_string()
                })
                .collect();

            // The person sees what will be created — schedule in words, the
            // first run and the timezone — before it exists. Data, not wording:
            // the card around it is translated.
            let preview = format!("“{name}”: {} → {} ({timezone})", cron.describe(), pretty[0],);
            if let Approval::Ask(pause) = approval(&ctx, self.id(), approval_request(preview))? {
                return Ok(pause);
            }

            let action = scheduled::create(
                &ctx.db,
                NewAction {
                    personal_agent_id: ctx
                        .personal_agent
                        .as_ref()
                        .map(|owned_agent| owned_agent.agent_id.clone()),
                    linked_session_id: None,
                    owner: aiplane_runtime::server::background::AutomationOwner::User(
                        ctx.person(self.id())?.to_string(),
                    ),
                    created_by: ctx.person(self.id())?.to_string(),
                    agent_version: None,
                    name: name.to_string(),
                    prompt: prompt.to_string(),
                    model: Some(model),
                    cron: cron.as_str().to_string(),
                    timezone,
                    // Never from the model: see the module docs. A model-created
                    // action can write, not act.
                    tools_enabled: false,
                    // Each run starts fresh, so a run can't accumulate context
                    // from earlier runs the user never looked at.
                    reuse_conversation: false,
                    reuse_rounds: 1,
                    next_run_at: Some(next_runs[0]),
                },
            )
            .await
            .map_err(|e| ToolError::Failed(format!("saving the scheduled action: {e}")))?;

            let mut out = action_json(&action);
            out["created"] = json!(true);
            out["status"] = json!(
                "Scheduled. Tell the user the schedule in words and when it first runs, \
                 and that it runs without tools. They can change or remove it under \
                 Scheduled."
            );
            Ok(out)
        })
    }
}

// ---------------------------------------------------------------------------
// list_scheduled_actions

pub struct ListScheduledActions;

impl Tool for ListScheduledActions {
    fn id(&self) -> &str {
        "list_scheduled_actions"
    }

    fn schema(&self) -> ToolDef {
        ToolDef::function(
            self.id(),
            "List the user's scheduled actions — id, name, prompt, schedule in \
             words, the next runs, and how the last run went. Use it to answer \
             \"what have I got scheduled?\", to check whether something is \
             already scheduled before creating a duplicate, or to find the id to \
             pass to `delete_scheduled_action`.",
            json!({ "type": "object", "additionalProperties": false, "properties": {} }),
        )
    }

    fn run<'a>(&'a self, ctx: ToolContext, _args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let actions = scheduled::list_for_user(&ctx.db, ctx.person(self.id())?)
                .await
                .map_err(|e| ToolError::Failed(format!("listing scheduled actions: {e}")))?;
            let items: Vec<Value> = actions.iter().map(action_json).collect();
            Ok(json!({
                "actions": items,
                "count": items.len(),
            }))
        })
    }
}

// ---------------------------------------------------------------------------
// delete_scheduled_action

pub struct DeleteScheduledAction;

#[derive(Deserialize)]
struct DeleteArgs {
    id: String,
}

impl Tool for DeleteScheduledAction {
    fn id(&self) -> &str {
        "delete_scheduled_action"
    }

    fn schema(&self) -> ToolDef {
        ToolDef::function(
            self.id(),
            "Delete one of the user's scheduled actions, by the `id` from \
             `list_scheduled_actions`. The user has to approve first. Deletion is \
             permanent — the action and its schedule are gone, though the \
             conversations its past runs produced stay. Only use it when the user \
             asked for something to stop.",
            json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["id"],
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "The action's id, from `list_scheduled_actions`."
                    }
                }
            }),
        )
    }

    fn run<'a>(&'a self, ctx: ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let args: DeleteArgs = serde_json::from_value(args)
                .map_err(|e| ToolError::InvalidArgs(format!("expected {{id: string}}: {e}")))?;
            let id = args.id.trim();
            if id.is_empty() {
                return Err(ToolError::InvalidArgs("`id` is required".into()));
            }

            // Scoped read first: this both tells us the name (for a
            // confirmation the user can actually judge) and makes another
            // user's action indistinguishable from a nonexistent one.
            let action = scheduled::get(
                &ctx.db,
                &aiplane_runtime::server::background::AutomationOwner::User(
                    ctx.person(self.id())?.into(),
                ),
                id,
            )
            .await
            .map_err(|e| ToolError::Failed(format!("reading the scheduled action: {e}")))?
            .ok_or_else(|| {
                ToolError::InvalidArgs(format!(
                    "no scheduled action `{id}` — call `list_scheduled_actions` \
                         to see the ids"
                ))
            })?;

            let preview = format!(
                "“{}”: {}",
                action.name,
                Cron::parse(&action.cron)
                    .map(|c| c.describe())
                    .unwrap_or_else(|_| action.cron.clone()),
            );
            if let Approval::Ask(pause) = approval(&ctx, self.id(), approval_request(preview))? {
                return Ok(pause);
            }

            let removed = scheduled::delete(
                &ctx.db,
                &aiplane_runtime::server::background::AutomationOwner::User(
                    ctx.person(self.id())?.into(),
                ),
                id,
            )
            .await
            .map_err(|e| ToolError::Failed(format!("deleting the scheduled action: {e}")))?;
            let removed = match removed {
                scheduled::DeleteOutcome::Deleted => true,
                scheduled::DeleteOutcome::Missing => false,
                scheduled::DeleteOutcome::Pending => {
                    return Err(ToolError::Failed(session_core::i18n::t(
                        session_core::i18n::Lang::En,
                        "agent-background-error-pending-delete",
                    )));
                }
            };
            Ok(json!({
                "deleted": removed,
                "id": action.id,
                "name": action.name,
                "status": if removed {
                    "Deleted. It will not run again."
                } else {
                    "It was already gone — nothing to delete."
                },
            }))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiplane_core::server::db;
    use aiplane_runtime::suspend::{Suspend, extract_suspend};
    use session_core::db::{Decision, SuspensionKind};

    async fn seeded() -> db::Pool {
        let pool = db::open(std::path::Path::new(":memory:")).await.unwrap();
        sqlx::query(
            r#"INSERT INTO users (id, email, timezone, created_at, updated_at)
               VALUES ('u1', 'u1@example.com', 'Europe/Berlin',
                       '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"#,
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    /// A context off the chat path: a model, but nothing can pause, so an
    /// approval can never be given.
    fn ctx_unattended(pool: &db::Pool) -> ToolContext {
        ctx_with(pool, Suspend::Unavailable)
    }

    fn ctx_with(pool: &db::Pool, suspend: Suspend) -> ToolContext {
        ToolContext {
            principal: aiplane_core::server::principal::Principal::User {
                id: "u1".into(),
                roles: vec![],
            },
            model: Some("qwen-32b".into()),
            suspend,
            ..ToolContext::for_test(pool.clone())
        }
    }

    fn approved() -> Suspend {
        Suspend::Decided(SuspensionKind::Approval, Decision::AllowOnce)
    }

    async fn nightly(pool: &db::Pool) -> scheduled::ScheduledAction {
        scheduled::create(
            pool,
            NewAction {
                personal_agent_id: None,
                linked_session_id: None,
                owner: aiplane_runtime::server::background::AutomationOwner::User("u1".into()),
                created_by: "u1".into(),
                agent_version: None,
                name: "Nightly".into(),
                prompt: "check".into(),
                model: Some("qwen-32b".into()),
                cron: "0 3 * * *".into(),
                timezone: "UTC".into(),
                tools_enabled: false,
                reuse_conversation: false,
                reuse_rounds: 1,
                next_run_at: None,
            },
        )
        .await
        .unwrap()
    }

    fn create_args() -> Value {
        json!({
            "name": "Weekly summary",
            "prompt": "Summarise last week's tickets.",
            "cron": "0 8 * * 1",
        })
    }

    /// The safety property: where nobody can be asked, nothing is written.
    /// This is what stops a /v1 caller from planting scheduled actions.
    #[tokio::test]
    async fn unattended_creation_is_refused_and_writes_nothing() {
        let pool = seeded().await;
        let err = ScheduleAction
            .run(ctx_unattended(&pool), create_args())
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("cannot pause"), "{err}");
        assert!(
            scheduled::list_for_user(&pool, "u1")
                .await
                .unwrap()
                .is_empty(),
            "an unconfirmed action must not be stored"
        );
    }

    /// Same for deletion — without an approval the action stays.
    #[tokio::test]
    async fn unattended_deletion_is_refused_and_keeps_the_action() {
        let pool = seeded().await;
        let action = nightly(&pool).await;
        let err = DeleteScheduledAction
            .run(ctx_unattended(&pool), json!({"id": action.id}))
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("cannot pause"), "{err}");
        assert_eq!(
            scheduled::list_for_user(&pool, "u1").await.unwrap().len(),
            1
        );
    }

    /// In chat the call pauses for the person's approval, saying what would be
    /// scheduled, and nothing is written until they give it.
    #[tokio::test]
    async fn in_chat_creation_waits_for_an_approval_and_writes_nothing_yet() {
        let pool = seeded().await;
        let out = ScheduleAction
            .run(ctx_with(&pool, Suspend::Available), create_args())
            .await
            .unwrap();
        let request = extract_suspend(&out).expect("the call waits for an approval");
        assert_eq!(request.kind, SuspensionKind::Approval);
        let preview = request.message.expect("what would be scheduled");
        assert!(
            preview.contains("Weekly summary") && preview.contains("Europe/Berlin"),
            "{preview}"
        );
        assert!(
            scheduled::list_for_user(&pool, "u1")
                .await
                .unwrap()
                .is_empty()
        );
    }

    /// The approved path: the action is written, it carries the turn's model
    /// and the user's timezone, tools are off, and the result hands the model
    /// the human schedule + next runs to read back.
    #[tokio::test]
    async fn an_approved_action_is_created_with_tools_off_and_a_preview() {
        let pool = seeded().await;
        let out = ScheduleAction
            .run(ctx_with(&pool, approved()), create_args())
            .await
            .unwrap();

        assert_eq!(out["created"], true, "{out:?}");
        assert_eq!(out["cron"], "0 8 * * 1", "{out:?}");
        assert_eq!(
            out["timezone"], "Europe/Berlin",
            "must default to users.timezone: {out:?}"
        );
        assert_eq!(
            out["model"], "qwen-32b",
            "must inherit the turn's model: {out:?}"
        );
        assert_eq!(
            out["tools_enabled"], false,
            "a model-created action must never have tools: {out:?}"
        );
        assert!(out["schedule"].is_string(), "{out:?}");
        assert_eq!(out["next_runs"].as_array().unwrap().len(), PREVIEW_RUNS);

        let stored = scheduled::list_for_user(&pool, "u1").await.unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].name, "Weekly summary");
        assert!(!stored[0].tools_enabled);
        assert!(stored[0].next_run_at.is_some(), "must be armed to fire");
    }

    /// Nothing but an approval is one: a denial, if the tool ever saw one,
    /// writes nothing.
    #[tokio::test]
    async fn a_denied_action_is_not_created() {
        let pool = seeded().await;
        let denied = Suspend::Decided(
            SuspensionKind::Approval,
            Decision::Deny {
                reason: session_core::db::DenyReason::User,
            },
        );
        assert!(
            ScheduleAction
                .run(ctx_with(&pool, denied), create_args())
                .await
                .is_err()
        );
        assert!(
            scheduled::list_for_user(&pool, "u1")
                .await
                .unwrap()
                .is_empty()
        );
    }

    /// Deletion goes through the same gate: it waits, and an approved one
    /// really removes the row.
    #[tokio::test]
    async fn an_approved_deletion_removes_the_action() {
        let pool = seeded().await;
        let action = nightly(&pool).await;
        let asked = DeleteScheduledAction
            .run(
                ctx_with(&pool, Suspend::Available),
                json!({"id": action.id}),
            )
            .await
            .unwrap();
        let request = extract_suspend(&asked).expect("the call waits for an approval");
        assert!(request.message.unwrap().contains("Nightly"));
        assert_eq!(
            scheduled::list_for_user(&pool, "u1").await.unwrap().len(),
            1
        );

        let out = DeleteScheduledAction
            .run(ctx_with(&pool, approved()), json!({"id": action.id}))
            .await
            .unwrap();
        assert_eq!(out["deleted"], true);
        assert!(
            scheduled::list_for_user(&pool, "u1")
                .await
                .unwrap()
                .is_empty()
        );
    }

    /// A bad cron expression is rejected with the parse error, before any
    /// confirmation — so the model can fix it itself rather than asking the
    /// user about a schedule that can't be saved.
    #[tokio::test]
    async fn an_invalid_cron_expression_reports_the_parse_error() {
        let pool = seeded().await;
        for bad in ["0 8 * *", "99 8 * * 1", "not a cron"] {
            let err = ScheduleAction
                .run(
                    ctx_unattended(&pool),
                    json!({"name": "x", "prompt": "y", "cron": bad}),
                )
                .await
                .unwrap_err()
                .to_string();
            assert!(
                err.contains("cron") || err.contains("minute") || err.contains("5 fields"),
                "for {bad:?}: {err}"
            );
            // Crucially not the approval refusal: we never got that far.
            assert!(!err.contains("cannot pause"), "for {bad:?}: {err}");
        }
    }

    /// An expression that parses but never occurs would be a schedule that
    /// silently never fires.
    #[tokio::test]
    async fn a_schedule_with_no_occurrence_is_rejected() {
        let pool = seeded().await;
        let err = ScheduleAction
            .run(
                ctx_unattended(&pool),
                json!({"name": "x", "prompt": "y", "cron": "0 0 30 2 *"}),
            )
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("never occurs"), "{err}");
    }

    /// Off a path that knows the model there is nothing sane to schedule
    /// with, and the failure must name the reason rather than inventing a
    /// pool id that fails at 06:00.
    #[tokio::test]
    async fn without_a_known_model_it_refuses_early() {
        let pool = seeded().await;
        let ctx = ToolContext {
            principal: aiplane_core::server::principal::Principal::User {
                id: "u1".into(),
                roles: vec![],
            },
            ..ToolContext::for_test(pool.clone())
        };
        let err = ScheduleAction
            .run(ctx, create_args())
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("which model"), "{err}");
    }

    #[tokio::test]
    async fn an_unknown_timezone_is_rejected() {
        let pool = seeded().await;
        let err = ScheduleAction
            .run(
                ctx_unattended(&pool),
                json!({
                    "name": "x", "prompt": "y", "cron": "0 8 * * 1",
                    "timezone": "Mars/Olympus"
                }),
            )
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("unknown timezone"), "{err}");
    }

    /// Listing is per-user and carries the human preview the model needs to
    /// describe a schedule without re-deriving cron semantics.
    #[tokio::test]
    async fn listing_is_scoped_to_the_caller_and_includes_a_preview() {
        let pool = seeded().await;
        sqlx::query(
            r#"INSERT INTO users (id, email, created_at, updated_at)
               VALUES ('u2', 'u2@example.com', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"#,
        )
        .execute(&pool)
        .await
        .unwrap();
        for (user, name) in [("u1", "Mine"), ("u2", "Theirs")] {
            scheduled::create(
                &pool,
                NewAction {
                    personal_agent_id: None,
                    linked_session_id: None,
                    owner: aiplane_runtime::server::background::AutomationOwner::User(user.into()),
                    created_by: user.into(),
                    agent_version: None,
                    name: name.into(),
                    prompt: "p".into(),
                    model: Some("qwen-32b".into()),
                    cron: "0 8 * * 1".into(),
                    timezone: "Europe/Berlin".into(),
                    tools_enabled: false,
                    reuse_conversation: false,
                    reuse_rounds: 1,
                    next_run_at: None,
                },
            )
            .await
            .unwrap();
        }

        let out = ListScheduledActions
            .run(ctx_unattended(&pool), Value::Null)
            .await
            .unwrap();
        assert_eq!(out["count"], 1, "{out:?}");
        let row = &out["actions"][0];
        assert_eq!(row["name"], "Mine");
        // `describe()`'s own wording ("At 08:00, on Mon."), not a re-derivation
        // here — the point is that the human summary reaches the model at all,
        // so it never has to explain cron semantics to the user itself.
        let schedule = row["schedule"].as_str().unwrap();
        assert!(
            schedule.contains("08:00") && schedule.contains("Mon"),
            "describe() must reach the model: {row:?}"
        );
        assert_eq!(
            row["next_runs"].as_array().unwrap().len(),
            PREVIEW_RUNS,
            "{row:?}"
        );
        assert_eq!(
            row["tools_enabled"], false,
            "actions must never advertise tools they don't have: {row:?}"
        );
    }

    /// Another user's action is reported as nonexistent, not as forbidden —
    /// the same no-existence-leak behaviour the RAG collections have.
    #[tokio::test]
    async fn deleting_another_users_action_looks_like_a_missing_id() {
        let pool = seeded().await;
        sqlx::query(
            r#"INSERT INTO users (id, email, created_at, updated_at)
               VALUES ('u2', 'u2@example.com', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"#,
        )
        .execute(&pool)
        .await
        .unwrap();
        let theirs = scheduled::create(
            &pool,
            NewAction {
                personal_agent_id: None,
                linked_session_id: None,
                owner: aiplane_runtime::server::background::AutomationOwner::User("u2".into()),
                created_by: "u2".into(),
                agent_version: None,
                name: "Theirs".into(),
                prompt: "p".into(),
                model: Some("m".into()),
                cron: "0 8 * * 1".into(),
                timezone: "UTC".into(),
                tools_enabled: false,
                reuse_conversation: false,
                reuse_rounds: 1,
                next_run_at: None,
            },
        )
        .await
        .unwrap();

        let err = DeleteScheduledAction
            .run(ctx_unattended(&pool), json!({"id": theirs.id}))
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("no scheduled action"), "{err}");
        assert!(!err.contains("cannot pause"), "no approval asked: {err}");
        // Still there.
        assert_eq!(
            scheduled::list_for_user(&pool, "u2").await.unwrap().len(),
            1
        );
    }
    #[tokio::test]
    async fn a_agent_created_schedule_keeps_its_agent_binding() {
        let pool = seeded().await;
        let (_, owned_agent) = aiplane_agents::db::agents::create_personal_agent(
            &pool,
            &aiplane_agents::db::system_principals::NewPrincipal {
                name: "Scheduler",
                display: "Scheduler",
                description: "",
            },
            r#"{"main":{}}"#,
            "u1",
            4,
        )
        .await
        .unwrap();
        let agent_id = owned_agent.unwrap().principal.id;
        let mut context = ctx_with(&pool, approved());
        context.personal_agent = Some(aiplane_runtime::agent_run::PersonalAgentIdentity {
            owner_user_id: "u1".into(),
            agent_id: agent_id.clone(),
        });
        let result = ScheduleAction.run(context, create_args()).await.unwrap();
        assert_eq!(result["created"], true);
        let actions = scheduled::list_for_user(&pool, "u1").await.unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(
            actions[0].personal_agent_id.as_deref(),
            Some(agent_id.as_str())
        );
        assert!(!actions[0].tools_enabled);
    }
}
