// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Durable memory: `remember`, `recall`, `update_memory`, `forget`.
//!
//! Ordinary users own profile memories. Personal agents write their own lessons
//! and may recall their user's profile. Organization agents write only their own
//! lessons and recall shared organization knowledge as read-only context.
//! Organization operations use the admitted principal turn and the same write
//! transaction for current authority and scoped storage.

use serde::Deserialize;
use serde_json::{Value, json};
use shared::api::ToolDef;

use aiplane_agents::db::{agent_memories::MemoryScope, agents::AgentOwner};
use aiplane_core::server::db::user_memories::{self, Memory, MemoryKind};
use aiplane_runtime::server::tools::{Tool, ToolContext, ToolError, ToolFuture};

fn organization_unavailable(ctx: &ToolContext) -> ToolError {
    ToolError::Failed(
        aiplane_runtime::agents::profile::AgentRunError::OrganizationExecutionUnavailable(
            ctx.principal.subject_id().to_owned(),
        )
        .to_string(),
    )
}

async fn organization_transaction(
    ctx: &ToolContext,
    tool: &str,
) -> Result<
    Option<(
        aiplane_agents::db::WriteTx,
        aiplane_runtime::server::tools::memory::OrganizationMemory,
    )>,
    ToolError,
> {
    if !ctx
        .agent
        .as_ref()
        .is_some_and(|run| run.organization().is_some())
    {
        return Ok(None);
    }
    let mut tx = aiplane_agents::db::WriteTx::begin(&ctx.db)
        .await
        .map_err(|error| ToolError::Failed(error.to_string()))?;
    let scope = aiplane_runtime::server::tools::memory::organization_scope_in(&mut tx, ctx, tool)
        .await?
        .ok_or_else(|| organization_unavailable(ctx))?;
    Ok(Some((tx, scope)))
}

fn personal_agent_scope<'a>(
    ctx: &'a ToolContext,
    tool: &str,
) -> Result<Option<(&'a str, &'a str)>, ToolError> {
    let user_id = ctx.person(tool)?;
    match ctx.personal_agent.as_ref() {
        Some(owned_agent) if owned_agent.owner_user_id == user_id => {
            Ok(Some((&owned_agent.owner_user_id, &owned_agent.agent_id)))
        }
        Some(_) => Err(ToolError::Failed(format!(
            "`{tool}` refused: the personal agent owner does not match the acting user."
        ))),
        None => Ok(None),
    }
}

fn scope_name(scope: Option<(&str, &str)>) -> &'static str {
    if scope.is_some() { "agent" } else { "user" }
}

fn resolve_scope(
    requested: Option<&str>,
    agent_scope: Option<(&str, &str)>,
) -> Result<&'static str, ToolError> {
    match requested {
        None => Ok(scope_name(agent_scope)),
        Some("agent") if agent_scope.is_some() => Ok("agent"),
        Some("user") if agent_scope.is_none() => Ok("user"),
        Some("agent") => Err(ToolError::InvalidArgs(
            "Agent memory is available only in a personal agent chat".into(),
        )),
        Some("user") => Ok("user"),
        Some(other) => Err(ToolError::InvalidArgs(format!(
            "unknown memory scope `{other}` — use `agent` or `user`"
        ))),
    }
}

fn memory_value(memory: Memory, scope: &'static str) -> Value {
    json!({
        "id": memory.id,
        "scope": scope,
        "kind": memory.kind.as_str(),
        "content": memory.content,
        "remembered_at": memory.created_at.to_string(),
    })
}

/// Parse a caller-supplied `kind` string into a [`MemoryKind`].
/// `None`/absent → `Fact` (the generic bucket); an unrecognised string
/// is a hard error so typos surface instead of silently misfiling.
fn parse_kind(raw: Option<&str>) -> Result<MemoryKind, ToolError> {
    match raw.map(str::trim) {
        None | Some("") => Ok(MemoryKind::Fact),
        Some(s) => MemoryKind::parse(s).ok_or_else(|| {
            ToolError::InvalidArgs(format!(
                "unknown kind `{s}` — use one of preference / project / fact"
            ))
        }),
    }
}

/// Safety cap on how many memories `recall` hands back in one call.
/// Recall intentionally returns *everything* (newest first) so the
/// model never has to guess a good query; this just bounds a runaway
/// store. Far above any realistic per-user count.
const MAX_RECALL_LIMIT: i64 = 200;

/// Upper bound on a single stored fact. Memory is for short facts, not
/// pasted documents — that's what attachments are for.
const MAX_CONTENT_BYTES: usize = aiplane_agents::db::agent_memories::MAX_CONTENT_BYTES;

// ---------------------------------------------------------------------------
// remember

pub struct Remember;

#[derive(Deserialize)]
struct RememberArgs {
    content: String,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    kind: Option<String>,
}

impl Tool for Remember {
    fn id(&self) -> &str {
        "remember"
    }

    fn egress(&self) -> bool {
        false
    }

    fn schema(&self) -> ToolDef {
        ToolDef::function(
            self.id(),
            "Store a short, durable fact for the acting owner so you can recall it in future \
             conversations. In an agent run, store lessons and project knowledge owned by that agent; \
             shared owner knowledge is read-only. Use it for information worth remembering long-term. Do not \
             store secrets, passwords, or sensitive personal data unless the user explicitly \
             asks you to. If something you already remembered has CHANGED or turned out to be \
             wrong, do not store a second, contradicting fact — call `update_memory` with the \
             old memory's id (from `recall`) to correct it, or `forget` to drop it. Two \
             conflicting memories leave you guessing which one still holds.",
            json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["content"],
                "properties": {
                    "content": {
                        "type": "string",
                        "description": "The fact to remember, as a single concise sentence \
                                        (e.g. 'Prefers answers in metric units')."
                    },
                    "kind": {
                        "type": "string",
                        "enum": ["preference", "project", "fact"],
                        "description": "How to classify this memory: 'preference' for how the \
                                        owner's preferences, 'project' for ongoing project context, 'fact' for any other stable detail. \
                                        Defaults to 'fact'."
                    }
                }
            }),
        )
    }

    fn run<'a>(&'a self, ctx: ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let args: RememberArgs = serde_json::from_value(args).map_err(|e| {
                ToolError::InvalidArgs(format!("expected {{content: string}}: {e}"))
            })?;
            let kind = parse_kind(args.kind.as_deref())?;
            let content = args.content.trim();
            if content.is_empty() {
                return Err(ToolError::InvalidArgs("content must not be empty".into()));
            }
            if content.len() > MAX_CONTENT_BYTES {
                return Err(ToolError::InvalidArgs(format!(
                    "content too long ({} UTF-8 bytes); keep memories under {MAX_CONTENT_BYTES}",
                    content.len()
                )));
            }
            if let Some((mut tx, scope)) = organization_transaction(&ctx, self.id()).await? {
                if args
                    .scope
                    .as_deref()
                    .is_some_and(|requested| requested != "agent")
                {
                    return Err(organization_unavailable(&ctx));
                }
                let row = aiplane_agents::db::agent_memories::insert_in(
                    &mut tx,
                    scope.agent_scope(),
                    kind,
                    content,
                )
                .await
                .map_err(|error| ToolError::Failed(error.to_string()))?
                .ok_or_else(|| organization_unavailable(&ctx))?;
                tx.commit()
                    .await
                    .map_err(|error| ToolError::Failed(error.to_string()))?;
                return Ok(
                    json!({"status":"remembered","id":row.id,"scope":"agent","kind":row.kind.as_str(),"content":row.content}),
                );
            }
            let agent_scope = personal_agent_scope(&ctx, self.id())?;
            let row = match agent_scope {
                Some((owner, owned_agent)) => aiplane_agents::db::agent_memories::insert(
                    &ctx.db,
                    MemoryScope::Agent {
                        owner: &AgentOwner::User(owner.into()),
                        agent_id: owned_agent,
                    },
                    kind,
                    content,
                )
                .await
                .map_err(|e| ToolError::Failed(format!("storing agent memory: {e}")))?
                .ok_or_else(|| {
                    ToolError::Failed("the personal agent is no longer available".into())
                })?,
                None => user_memories::insert(&ctx.db, ctx.person(self.id())?, kind, content)
                    .await
                    .map_err(|e| ToolError::Failed(format!("storing memory: {e}")))?,
            };
            Ok(json!({
                "status": "remembered",
                "id": row.id,
                "scope": scope_name(agent_scope),
                "kind": row.kind.as_str(),
                "content": row.content,
            }))
        })
    }
}

// ---------------------------------------------------------------------------
// recall

pub struct Recall;

impl Tool for Recall {
    fn read_only(&self) -> bool {
        true
    }

    fn id(&self) -> &str {
        "recall"
    }

    fn egress(&self) -> bool {
        false
    }

    fn schema(&self) -> ToolDef {
        ToolDef::function(
            self.id(),
            "Retrieve stored memories available to the acting owner. agent runs return their own \
             lessons together with read-only user or organization knowledge, each labeled by scope. \
             Takes no arguments and returns memories newest first — you don't need to craft a query. \
             Use the relevant notes as context. \
             An empty result means nothing has been remembered yet. Each memory comes back with \
             an `id`; pass that id to `update_memory` or `forget` when something you stored \
             turns out to be wrong or out of date.",
            json!({
                "type": "object",
                "properties": {}
            }),
        )
    }

    fn run<'a>(&'a self, ctx: ToolContext, _args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            // Deliberately ignore any arguments and return the user's
            // full memory set (newest first, bounded by a safety cap).
            // No filtering — the model reasons over everything rather
            // than guessing a query that has to lexically match.
            if let Some((mut tx, scope)) = organization_transaction(&ctx, self.id()).await? {
                let mut rows = Vec::new();
                for (selected, name) in [
                    (scope.agent_scope(), "agent"),
                    (scope.organization_scope(), "organization"),
                ] {
                    let notes = aiplane_agents::db::agent_memories::list_in(
                        &mut tx,
                        selected,
                        MAX_RECALL_LIMIT,
                    )
                    .await
                    .map_err(|error| ToolError::Failed(error.to_string()))?
                    .ok_or_else(|| organization_unavailable(&ctx))?;
                    rows.extend(notes.into_iter().map(|note| (note, name)));
                }
                tx.commit()
                    .await
                    .map_err(|error| ToolError::Failed(error.to_string()))?;
                rows.sort_by_key(|(note, _)| std::cmp::Reverse(note.created_at));
                rows.truncate(MAX_RECALL_LIMIT as usize);
                let memories: Vec<Value> = rows
                    .into_iter()
                    .map(|(note, scope)| memory_value(note, scope))
                    .collect();
                return Ok(json!({"count":memories.len(),"memories":memories}));
            }
            let agent_scope = personal_agent_scope(&ctx, self.id())?;
            let mut rows: Vec<(Memory, &'static str)> = user_memories::recall_recent(
                &ctx.db,
                ctx.person(self.id())?,
                None,
                MAX_RECALL_LIMIT,
            )
            .await
            .map_err(|e| ToolError::Failed(format!("recalling user memories: {e}")))?
            .into_iter()
            .map(|memory| (memory, "user"))
            .collect();
            if let Some((owner, owned_agent)) = agent_scope {
                let agent_rows = aiplane_agents::db::agent_memories::list(
                    &ctx.db,
                    MemoryScope::Agent {
                        owner: &AgentOwner::User(owner.into()),
                        agent_id: owned_agent,
                    },
                    MAX_RECALL_LIMIT,
                )
                .await
                .map_err(|e| ToolError::Failed(format!("recalling agent memories: {e}")))?
                .ok_or_else(|| {
                    ToolError::Failed("the personal agent is no longer available".into())
                })?;
                rows.extend(agent_rows.into_iter().map(|memory| (memory, "agent")));
            }
            rows.sort_by_key(|(memory, _)| std::cmp::Reverse(memory.created_at));
            rows.truncate(MAX_RECALL_LIMIT as usize);
            let memories: Vec<Value> = rows
                .into_iter()
                .map(|(memory, scope)| memory_value(memory, scope))
                .collect();
            Ok(json!({
                "count": memories.len(),
                "memories": memories,
            }))
        })
    }
}

// ---------------------------------------------------------------------------
// update_memory

pub struct UpdateMemory;

#[derive(Deserialize)]
struct UpdateArgs {
    id: String,
    content: String,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    scope: Option<String>,
}

impl Tool for UpdateMemory {
    fn id(&self) -> &str {
        "update_memory"
    }

    fn egress(&self) -> bool {
        false
    }

    fn schema(&self) -> ToolDef {
        ToolDef::function(
            self.id(),
            "Correct a memory you stored earlier, in place. Use this — not a second \
             `remember` — when a fact has changed or was wrong: storing the new version \
             alongside the old one leaves two contradicting memories and you will not be \
             able to tell later which still holds. Get the `id` from `recall`.",
            json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["id", "content"],
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Id of the memory to correct, exactly as `recall` \
                                        returned it."
                    },
                    "content": {
                        "type": "string",
                        "description": "The corrected fact, as a single concise sentence. \
                                        Replaces the stored text entirely."
                    },
                    "kind": {
                        "type": "string",
                        "enum": ["preference", "project", "fact"],
                        "description": "Optional new classification. Omit to keep the \
                                        memory's current one — correcting the wording of a \
                                        preference should not silently refile it as a fact."
                    },
                    "scope": {
                        "type": "string",
                        "enum": ["agent", "user"],
                        "description": "Scope reported by `recall`; an agent may change only `agent`."
                    }
                }
            }),
        )
    }

    fn run<'a>(&'a self, ctx: ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let args: UpdateArgs = serde_json::from_value(args).map_err(|e| {
                ToolError::InvalidArgs(format!("expected {{id: string, content: string}}: {e}"))
            })?;
            let content = args.content.trim();
            if content.is_empty() {
                return Err(ToolError::InvalidArgs(
                    "content must not be empty — use `forget` to drop a memory".into(),
                ));
            }
            if content.len() > MAX_CONTENT_BYTES {
                return Err(ToolError::InvalidArgs(format!(
                    "content too long ({} UTF-8 bytes); keep memories under {MAX_CONTENT_BYTES}",
                    content.len()
                )));
            }

            if let Some((mut tx, scope)) = organization_transaction(&ctx, self.id()).await? {
                if args
                    .scope
                    .as_deref()
                    .is_some_and(|requested| requested != "agent")
                {
                    return Err(organization_unavailable(&ctx));
                }
                let Some(existing) = aiplane_agents::db::agent_memories::get_in(
                    &mut tx,
                    scope.agent_scope(),
                    &args.id,
                )
                .await
                .map_err(|error| ToolError::Failed(error.to_string()))?
                else {
                    return Ok(not_found(&args.id));
                };
                let kind = match args.kind.as_deref() {
                    Some(raw) => parse_kind(Some(raw))?,
                    None => existing.kind,
                };
                let row = aiplane_agents::db::agent_memories::update_in(
                    &mut tx,
                    scope.agent_scope(),
                    &args.id,
                    kind,
                    content,
                )
                .await
                .map_err(|error| ToolError::Failed(error.to_string()))?;
                tx.commit()
                    .await
                    .map_err(|error| ToolError::Failed(error.to_string()))?;
                return Ok(match row {
                    Some(row) => {
                        json!({"status":"updated","id":row.id,"scope":"agent","kind":row.kind.as_str(),"content":row.content,"previous_content":existing.content})
                    }
                    None => not_found(&args.id),
                });
            }
            let agent_scope = personal_agent_scope(&ctx, self.id())?;
            let scope = resolve_scope(args.scope.as_deref(), agent_scope)?;
            if scope == "user" && agent_scope.is_some() {
                return Err(ToolError::Failed(
                    "user-wide memories are read-only in a personal agent chat".into(),
                ));
            }
            if scope == "agent" && agent_scope.is_none() {
                return Err(ToolError::Failed(
                    "Agent memory can only be changed from that agent's chat".into(),
                ));
            }
            // Read the existing row first, scoped to this user: it gives us
            // the current `kind` to preserve, and it turns "someone else's id"
            // into the same not-found answer as "no such id" (no existence
            // leak across users).
            let existing = match (scope, agent_scope) {
                ("agent", Some((owner, owned_agent))) => aiplane_agents::db::agent_memories::get(
                    &ctx.db,
                    MemoryScope::Agent {
                        owner: &AgentOwner::User(owner.into()),
                        agent_id: owned_agent,
                    },
                    &args.id,
                )
                .await
                .map_err(|e| ToolError::Failed(format!("looking up agent memory: {e}")))?,
                _ => user_memories::get(&ctx.db, ctx.person(self.id())?, &args.id)
                    .await
                    .map_err(|e| ToolError::Failed(format!("looking up memory: {e}")))?,
            };
            let Some(existing) = existing else {
                return Ok(not_found(&args.id));
            };
            let kind = match args.kind.as_deref() {
                Some(raw) => parse_kind(Some(raw))?,
                None => existing.kind,
            };

            let updated = match (scope, agent_scope) {
                ("agent", Some((owner, owned_agent))) => {
                    aiplane_agents::db::agent_memories::update(
                        &ctx.db,
                        MemoryScope::Agent {
                            owner: &AgentOwner::User(owner.into()),
                            agent_id: owned_agent,
                        },
                        &args.id,
                        kind,
                        content,
                    )
                    .await
                    .map_err(|e| ToolError::Failed(format!("updating agent memory: {e}")))?
                }
                _ => {
                    user_memories::update(&ctx.db, ctx.person(self.id())?, &args.id, kind, content)
                        .await
                        .map_err(|e| ToolError::Failed(format!("updating memory: {e}")))?
                }
            };
            match updated {
                Some(row) => Ok(json!({
                    "status": "updated",
                    "id": row.id,
                    "scope": scope,
                    "kind": row.kind.as_str(),
                    "content": row.content,
                    "previous_content": existing.content,
                })),
                // Raced with a concurrent delete.
                None => Ok(not_found(&args.id)),
            }
        })
    }
}

// ---------------------------------------------------------------------------
// forget

pub struct Forget;

#[derive(Deserialize)]
struct ForgetArgs {
    id: String,
    #[serde(default)]
    scope: Option<String>,
}

impl Tool for Forget {
    fn id(&self) -> &str {
        "forget"
    }

    fn egress(&self) -> bool {
        false
    }

    fn schema(&self) -> ToolDef {
        ToolDef::function(
            self.id(),
            "Delete a memory you stored earlier. Use it when the user asks you to forget \
             something, or when a stored fact is obsolete and has no replacement — if it \
             has one, prefer `update_memory` so the history stays a single fact rather \
             than a deletion plus an addition. Get the `id` from `recall`. This cannot be \
             undone.",
            json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["id"],
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Id of the memory to delete, exactly as `recall` \
                                        returned it."
                    },
                    "scope": {
                        "type": "string",
                        "enum": ["agent", "user"],
                        "description": "Scope reported by `recall`; an agent may change only `agent`."
                    }
                }
            }),
        )
    }

    fn run<'a>(&'a self, ctx: ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let args: ForgetArgs = serde_json::from_value(args)
                .map_err(|e| ToolError::InvalidArgs(format!("expected {{id: string}}: {e}")))?;

            if let Some((mut tx, scope)) = organization_transaction(&ctx, self.id()).await? {
                if args
                    .scope
                    .as_deref()
                    .is_some_and(|requested| requested != "agent")
                {
                    return Err(organization_unavailable(&ctx));
                }
                let Some(existing) = aiplane_agents::db::agent_memories::get_in(
                    &mut tx,
                    scope.agent_scope(),
                    &args.id,
                )
                .await
                .map_err(|error| ToolError::Failed(error.to_string()))?
                else {
                    return Ok(not_found(&args.id));
                };
                let deleted = aiplane_agents::db::agent_memories::delete_in(
                    &mut tx,
                    scope.agent_scope(),
                    &args.id,
                )
                .await
                .map_err(|error| ToolError::Failed(error.to_string()))?;
                tx.commit()
                    .await
                    .map_err(|error| ToolError::Failed(error.to_string()))?;
                return Ok(if deleted {
                    json!({"status":"forgotten","id":existing.id,"scope":"agent","content":existing.content})
                } else {
                    not_found(&args.id)
                });
            }
            let agent_scope = personal_agent_scope(&ctx, self.id())?;
            let scope = resolve_scope(args.scope.as_deref(), agent_scope)?;
            if scope == "user" && agent_scope.is_some() {
                return Err(ToolError::Failed(
                    "user-wide memories are read-only in a personal agent chat".into(),
                ));
            }
            if scope == "agent" && agent_scope.is_none() {
                return Err(ToolError::Failed(
                    "Agent memory can only be changed from that agent's chat".into(),
                ));
            }
            // Fetch before deleting so the result can echo *what* was
            // forgotten — the model needs that to confirm to the user, and
            // after the delete it's gone.
            let existing = match (scope, agent_scope) {
                ("agent", Some((owner, owned_agent))) => aiplane_agents::db::agent_memories::get(
                    &ctx.db,
                    MemoryScope::Agent {
                        owner: &AgentOwner::User(owner.into()),
                        agent_id: owned_agent,
                    },
                    &args.id,
                )
                .await
                .map_err(|e| ToolError::Failed(format!("looking up agent memory: {e}")))?,
                _ => user_memories::get(&ctx.db, ctx.person(self.id())?, &args.id)
                    .await
                    .map_err(|e| ToolError::Failed(format!("looking up memory: {e}")))?,
            };
            let Some(existing) = existing else {
                return Ok(not_found(&args.id));
            };
            let deleted = match (scope, agent_scope) {
                ("agent", Some((owner, owned_agent))) => {
                    aiplane_agents::db::agent_memories::delete(
                        &ctx.db,
                        MemoryScope::Agent {
                            owner: &AgentOwner::User(owner.into()),
                            agent_id: owned_agent,
                        },
                        &args.id,
                    )
                    .await
                    .map_err(|e| ToolError::Failed(format!("deleting agent memory: {e}")))?
                }
                _ => user_memories::delete(&ctx.db, ctx.person(self.id())?, &args.id)
                    .await
                    .map_err(|e| ToolError::Failed(format!("deleting memory: {e}")))?,
            };
            if !deleted {
                return Ok(not_found(&args.id));
            }
            Ok(json!({
                "status": "forgotten",
                "id": args.id,
                "scope": scope,
                "content": existing.content,
            }))
        })
    }
}

/// The shared "no such memory" result.
///
/// Deliberately a successful result rather than a `ToolError`: a stale id is
/// something the model can recover from by re-reading `recall`, and phrasing
/// it as a failure tends to make models retry the same id or give up. Also
/// deliberately identical for "never existed" and "belongs to another user",
/// so the tool can't be used to probe for other people's memory ids.
fn not_found(id: &str) -> Value {
    json!({
        "status": "not_found",
        "id": id,
        "note": "No memory with that id exists for this user. Call `recall` to get the \
                 current ids — they change when memories are replaced.",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiplane_agents::db::{agents, system_principals::NewPrincipal};
    use aiplane_core::server::db;
    use aiplane_core::server::db::users::{self, User};
    use jiff::Timestamp;

    async fn ctx(pool: &db::Pool, user_id: &str) -> ToolContext {
        ToolContext {
            principal: aiplane_core::server::principal::Principal::User {
                id: user_id.into(),
                roles: vec![],
            },
            ..ToolContext::for_test(pool.clone())
        }
    }

    async fn fresh() -> db::Pool {
        db::open(std::path::Path::new(":memory:")).await.unwrap()
    }

    async fn personal_agent_ctx(pool: &db::Pool, user_id: &str) -> ToolContext {
        let now = Timestamp::now();
        users::upsert(
            pool,
            &User {
                id: user_id.to_string(),
                email: format!("{user_id}@example.com"),
                name: None,
                roles: vec![],
                created_at: now,
                updated_at: now,
                timezone: None,
                speech_voice: None,
            },
        )
        .await
        .unwrap();
        let (_, owned_agent) = agents::create_personal_agent(
            pool,
            &NewPrincipal {
                name: "memory-agent",
                display: "Memory agent",
                description: "",
            },
            r#"{"main":{}}"#,
            user_id,
            4,
        )
        .await
        .unwrap();
        let agent_id = owned_agent.unwrap().principal.id;
        ToolContext {
            personal_agent: Some(aiplane_runtime::agent_run::PersonalAgentIdentity {
                owner_user_id: user_id.to_string(),
                agent_id,
            }),
            ..ctx(pool, user_id).await
        }
    }

    /// A system principal has no person to remember things about. Its subject
    /// id must not quietly become a memory owner either: nothing is written.
    #[tokio::test]
    async fn a_system_principal_can_neither_remember_nor_recall() {
        use aiplane_core::server::principal::{GrantSet, Principal, SystemPrincipal};
        let pool = fresh().await;
        let system = || ToolContext {
            principal: Principal::System(SystemPrincipal {
                id: "p1".into(),
                name: "ci".into(),
                grants: std::sync::Arc::new(GrantSet::default()),
            }),
            ..ToolContext::for_test(pool.clone())
        };
        let err = Remember
            .run(system(), json!({"content": "x"}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("system principal `ci`"), "{err}");
        assert!(Recall.run(system(), Value::Null).await.is_err());
        assert!(
            user_memories::recall_recent(&pool, "p1", None, 10)
                .await
                .unwrap()
                .is_empty()
        );
    }

    // -----------------------------------------------------------------------
    // ids + correction (forget / update_memory)

    /// `forget` and `update_memory` address a memory by id, and `recall` is
    /// the only place the model ever learns one. Without this the correction
    /// tools are unreachable.
    #[tokio::test]
    async fn recall_exposes_ids_that_the_correction_tools_accept() {
        let pool = fresh().await;
        Remember
            .run(
                ctx(&pool, "alice").await,
                json!({"content": "in platform team"}),
            )
            .await
            .unwrap();
        let recalled = Recall
            .run(ctx(&pool, "alice").await, Value::Null)
            .await
            .unwrap();
        let id = recalled["memories"][0]["id"]
            .as_str()
            .expect("recall must expose an id")
            .to_string();

        let out = Forget
            .run(ctx(&pool, "alice").await, json!({"id": id}))
            .await
            .unwrap();
        assert_eq!(out["status"], "forgotten", "{out:?}");
    }

    #[tokio::test]
    async fn forget_removes_only_that_memory() {
        let pool = fresh().await;
        let c = ctx(&pool, "alice").await;
        let keep = Remember
            .run(c.clone(), json!({"content": "prefers metric"}))
            .await
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        let drop = Remember
            .run(c.clone(), json!({"content": "in platform team"}))
            .await
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();

        let out = Forget.run(c.clone(), json!({"id": drop})).await.unwrap();
        assert_eq!(out["status"], "forgotten");
        // Echoes what was dropped, so the model can confirm to the user.
        assert_eq!(out["content"], "in platform team");

        let left = Recall.run(c.clone(), Value::Null).await.unwrap();
        assert_eq!(left["count"], 1);
        assert_eq!(left["memories"][0]["id"], keep);
    }

    /// The whole point of `update_memory`: correcting must not leave two
    /// contradicting facts behind, which is what a second `remember` does.
    #[tokio::test]
    async fn update_replaces_instead_of_duplicating() {
        let pool = fresh().await;
        let c = ctx(&pool, "alice").await;
        let id = Remember
            .run(c.clone(), json!({"content": "in platform team"}))
            .await
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();

        let out = UpdateMemory
            .run(c.clone(), json!({"id": id, "content": "in storage team"}))
            .await
            .unwrap();
        assert_eq!(out["status"], "updated");
        assert_eq!(out["content"], "in storage team");
        assert_eq!(out["previous_content"], "in platform team");

        let recalled = Recall.run(c.clone(), Value::Null).await.unwrap();
        assert_eq!(recalled["count"], 1, "must not duplicate: {recalled:?}");
        assert_eq!(recalled["memories"][0]["content"], "in storage team");
    }

    #[tokio::test]
    async fn update_keeps_the_existing_kind_unless_told_otherwise() {
        let pool = fresh().await;
        let c = ctx(&pool, "alice").await;
        let id = Remember
            .run(
                c.clone(),
                json!({"content": "likes short answers", "kind": "preference"}),
            )
            .await
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();

        let kept = UpdateMemory
            .run(
                c.clone(),
                json!({"id": id.clone(), "content": "likes terse answers"}),
            )
            .await
            .unwrap();
        assert_eq!(kept["kind"], "preference", "rewording must not refile it");

        let moved = UpdateMemory
            .run(
                c.clone(),
                json!({"id": id, "content": "likes terse answers", "kind": "fact"}),
            )
            .await
            .unwrap();
        assert_eq!(moved["kind"], "fact");
    }

    #[tokio::test]
    async fn correction_tools_cannot_touch_another_users_memory() {
        let pool = fresh().await;
        let id = Remember
            .run(
                ctx(&pool, "alice").await,
                json!({"content": "alice secret"}),
            )
            .await
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();

        // Bob names a real id he has no business knowing. Both tools must
        // report the same thing they report for a nonexistent id — no
        // existence leak, and no mutation.
        let forget = Forget
            .run(ctx(&pool, "bob").await, json!({"id": id.clone()}))
            .await
            .unwrap();
        assert_eq!(forget["status"], "not_found", "{forget:?}");
        let update = UpdateMemory
            .run(
                ctx(&pool, "bob").await,
                json!({"id": id.clone(), "content": "hijacked"}),
            )
            .await
            .unwrap();
        assert_eq!(update["status"], "not_found", "{update:?}");

        // Alice's memory is untouched.
        let alice = Recall
            .run(ctx(&pool, "alice").await, Value::Null)
            .await
            .unwrap();
        assert_eq!(alice["count"], 1);
        assert_eq!(alice["memories"][0]["content"], "alice secret");
    }

    #[tokio::test]
    async fn unknown_id_is_a_recoverable_result_not_an_error() {
        let pool = fresh().await;
        // A stale id is something the model fixes by re-reading `recall`, so
        // it must not surface as a tool failure.
        let out = Forget
            .run(ctx(&pool, "alice").await, json!({"id": "no-such-id"}))
            .await
            .unwrap();
        assert_eq!(out["status"], "not_found");
        assert!(out["note"].as_str().unwrap().contains("recall"), "{out:?}");
    }

    #[tokio::test]
    async fn update_rejects_empty_content_and_points_at_forget() {
        let pool = fresh().await;
        let err = UpdateMemory
            .run(
                ctx(&pool, "alice").await,
                json!({"id": "x", "content": "  "}),
            )
            .await
            .unwrap_err();
        match err {
            ToolError::InvalidArgs(msg) => assert!(msg.contains("forget"), "{msg}"),
            other => panic!("expected InvalidArgs, got {other:?}"),
        }
    }

    #[test]
    fn correction_tool_schema_names_match_ids() {
        assert_eq!(Forget.id(), Forget.schema().function.name);
        assert_eq!(UpdateMemory.id(), UpdateMemory.schema().function.name);
    }

    /// `remember` has to actively steer the model away from storing a second,
    /// contradicting fact, or it will never reach for the correction tools.
    #[test]
    fn remember_description_points_at_the_correction_tools() {
        let desc = Remember.schema().function.description;
        assert!(desc.contains("update_memory"), "{desc}");
        assert!(desc.contains("forget"), "{desc}");
    }

    #[tokio::test]
    async fn remember_then_recall_roundtrips() {
        let pool = fresh().await;
        Remember
            .run(
                ctx(&pool, "alice").await,
                json!({"content": "prefers metric units"}),
            )
            .await
            .unwrap();
        let out = Recall
            .run(ctx(&pool, "alice").await, Value::Null)
            .await
            .unwrap();
        assert_eq!(out["count"], 1);
        assert_eq!(out["memories"][0]["content"], "prefers metric units");
    }

    #[tokio::test]
    async fn personal_agent_memory_tools_keep_agent_and_user_memory_scopes_separate() {
        let pool = fresh().await;
        let owned_agent_ctx = personal_agent_ctx(&pool, "alice").await;
        let user_memory = user_memories::insert(
            &pool,
            "alice",
            MemoryKind::Preference,
            "prefers concise replies",
        )
        .await
        .unwrap();
        let saved = Remember
            .run(
                owned_agent_ctx.clone(),
                json!({"content": "avoid the same deployment pitfall"}),
            )
            .await
            .unwrap();
        assert_eq!(saved["scope"], "agent");

        let recalled = Recall
            .run(owned_agent_ctx.clone(), Value::Null)
            .await
            .unwrap();
        assert_eq!(recalled["count"], 2);
        assert!(
            recalled["memories"]
                .as_array()
                .unwrap()
                .iter()
                .any(|m| { m["id"] == user_memory.id && m["scope"] == "user" })
        );
        let agent_memory = recalled["memories"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["scope"] == "agent")
            .unwrap();
        let agent_id = agent_memory["id"].as_str().unwrap();
        let updated = UpdateMemory
            .run(
                owned_agent_ctx.clone(),
                json!({"id": agent_id, "scope": "agent", "content": "avoid the verified deployment pitfall"}),
            )
            .await
            .unwrap();
        assert_eq!(updated["status"], "updated");
        assert_eq!(updated["scope"], "agent");
        let forgotten = Forget
            .run(
                owned_agent_ctx.clone(),
                json!({"id": agent_id, "scope": "agent"}),
            )
            .await
            .unwrap();
        assert_eq!(forgotten["status"], "forgotten");
        assert_eq!(forgotten["scope"], "agent");

        let cannot_update_profile = UpdateMemory
            .run(
                owned_agent_ctx.clone(),
                json!({"id": user_memory.id, "scope": "user", "content": "changed"}),
            )
            .await
            .unwrap_err();
        assert!(cannot_update_profile.to_string().contains("read-only"));
        let cannot_forget_profile = Forget
            .run(
                owned_agent_ctx.clone(),
                json!({"id": user_memory.id, "scope": "user"}),
            )
            .await
            .unwrap_err();
        assert!(cannot_forget_profile.to_string().contains("read-only"));
        assert_eq!(
            user_memories::get(&pool, "alice", &user_memory.id)
                .await
                .unwrap()
                .unwrap()
                .content,
            "prefers concise replies"
        );
    }

    #[tokio::test]
    async fn a_personal_agent_cannot_use_an_identity_for_a_different_user() {
        let pool = fresh().await;
        let mut owned_agent_ctx = personal_agent_ctx(&pool, "alice").await;
        owned_agent_ctx
            .personal_agent
            .as_mut()
            .unwrap()
            .owner_user_id = "bob".into();
        let error = Remember
            .run(owned_agent_ctx, json!({"content": "a private lesson"}))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("does not match the acting user"));
        assert!(
            user_memories::recall_recent(&pool, "alice", None, 10)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn recall_returns_all_memories_ignoring_args() {
        let pool = fresh().await;
        let c = ctx(&pool, "alice").await;
        Remember
            .run(c.clone(), json!({"content": "runs a Ceph cluster"}))
            .await
            .unwrap();
        Remember
            .run(c.clone(), json!({"content": "likes dark mode"}))
            .await
            .unwrap();
        // A stray `query` arg is ignored — recall always returns all.
        let out = Recall.run(c, json!({"query": "ceph"})).await.unwrap();
        assert_eq!(out["count"], 2);
    }

    #[tokio::test]
    async fn recall_is_scoped_to_the_caller() {
        let pool = fresh().await;
        Remember
            .run(
                ctx(&pool, "alice").await,
                json!({"content": "alice secret"}),
            )
            .await
            .unwrap();
        let out = Recall
            .run(ctx(&pool, "bob").await, Value::Null)
            .await
            .unwrap();
        assert_eq!(out["count"], 0);
    }

    #[tokio::test]
    async fn remember_stores_kind_and_recall_reports_it() {
        let pool = fresh().await;
        let c = ctx(&pool, "alice").await;
        Remember
            .run(
                c.clone(),
                json!({"content": "metric units", "kind": "preference"}),
            )
            .await
            .unwrap();
        let out = Recall.run(c, Value::Null).await.unwrap();
        assert_eq!(out["count"], 1);
        assert_eq!(out["memories"][0]["kind"], "preference");
        assert_eq!(out["memories"][0]["content"], "metric units");
    }

    #[tokio::test]
    async fn remember_rejects_unknown_kind() {
        let pool = fresh().await;
        let err = Remember
            .run(
                ctx(&pool, "alice").await,
                json!({"content": "x", "kind": "bogus"}),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn remember_rejects_empty_content() {
        let pool = fresh().await;
        let err = Remember
            .run(ctx(&pool, "alice").await, json!({"content": "   "}))
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)), "got {err:?}");
    }

    #[test]
    fn schema_names_match_ids() {
        assert_eq!(Remember.id(), Remember.schema().function.name);
        assert_eq!(Recall.id(), Recall.schema().function.name);
    }

    async fn organization_memory_context() -> (
        std::sync::Arc<aiplane_runtime::rama_server::state::RamaState>,
        ToolContext,
        String,
        String,
    ) {
        organization_memory_context_with_upstream("http://127.0.0.1:1").await
    }

    async fn organization_memory_context_with_upstream(
        upstream: &str,
    ) -> (
        std::sync::Arc<aiplane_runtime::rama_server::state::RamaState>,
        ToolContext,
        String,
        String,
    ) {
        use aiplane_agents::db::{WriteTx, organizations, system_principals};
        use aiplane_core::server::{
            config::Config, principal::GrantKind, rbac::Resolver, upstreams::UpstreamRegistry,
        };
        use aiplane_runtime::{
            agent_run::Actor,
            agents::profile::{Role, RunProfile, SpecSource},
            rama_server::state::RamaState,
            server::{
                headless::{OpenParams, Owner, open_session},
                state::AppState,
                tools::ToolRegistry,
            },
        };
        let pool = fresh().await;
        let _personal = personal_agent_ctx(&pool, "employee").await;
        user_memories::insert(
            &pool,
            "employee",
            MemoryKind::Preference,
            "PRIVATE EMPLOYEE MARKER",
        )
        .await
        .unwrap();
        let organizations::OrganizationCreateResult::Created(organization) = organizations::create(
            &pool,
            &organizations::NewOrganization {
                name: "Support",
                description: "Ticket knowledge",
                member_groups: &[],
            },
            "employee",
        )
        .await
        .unwrap() else {
            panic!("organization expected")
        };
        let organization = organization.organization.id;
        let mut tx = WriteTx::begin(&pool).await.unwrap();
        let (_, owned_agent) = agents::create_organization_agent_in(
            &mut tx,
            &organization,
            &NewPrincipal {
                name: "support",
                display: "Support",
                description: "",
            },
            r#"{"main":{}}"#,
            "employee",
            4,
        )
        .await
        .unwrap();
        let owned_agent = owned_agent.unwrap().principal.id;
        tx.commit().await.unwrap();
        for (kind, reference) in [
            (GrantKind::Model, "model"),
            (GrantKind::Tool, "remember"),
            (GrantKind::Tool, "recall"),
            (GrantKind::Tool, "update_memory"),
            (GrantKind::Tool, "forget"),
        ] {
            system_principals::add_grant(&pool, &owned_agent, kind, reference, "employee")
                .await
                .unwrap();
        }
        let registry = std::sync::Arc::new(
            ToolRegistry::new()
                .with(Remember)
                .with(Recall)
                .with(UpdateMemory)
                .with(Forget),
        );
        let pools = std::collections::HashMap::from([(
            "pool".to_string(),
            aiplane_core::server::upstreams::config::UpstreamPoolConfig {
                models: vec!["model".into()],
                ..aiplane_core::server::upstreams::config::UpstreamPoolConfig::new(
                    aiplane_core::server::upstreams::config::PoolKind::Chat,
                    vec![aiplane_core::server::upstreams::config::BackendConfig::new(
                        "fixture", upstream,
                    )],
                )
            },
        )]);
        let app = AppState::new(
            Config::default(),
            pool.clone(),
            UpstreamRegistry::new(&pools).unwrap(),
            registry,
            std::sync::Arc::new(Resolver::build(Default::default(), vec![]).unwrap()),
        );
        let state = std::sync::Arc::new(RamaState::new(
            app,
            aiplane_core::rama_server::SessionStore::new(pool.clone(), [17; 32]),
            aiplane_core::server::usage::UsageHandle::disabled(),
        ));
        let profile = RunProfile::load_from(&state, &owned_agent, SpecSource::Draft(json!({"main":{"model":"model","tools":["remember","recall","update_memory","forget"]}})), Role::Main, &Default::default()).await.unwrap();
        let (session, turn) = open_session(
            &pool,
            OpenParams {
                owner: Owner::Run {
                    principal_id: &owned_agent,
                    parent_turn_id: None,
                    agent_version: Some(agents::DRAFT_VERSION),
                },
                title: "Memory task",
                prompt: "Keep notes",
                model: "model",
                existing_session: None,
                personal_agent_id: None,
            },
        )
        .await
        .unwrap();
        let chain = std::sync::Arc::new(aiplane_core::server::run_chain::RunChain::root(
            &session,
            None,
            aiplane_core::server::run_chain::Frame::for_principal(&profile.principal, None),
        ));
        let params = profile.drive_params(&session, &turn, chain).unwrap();
        let Actor::Agent(run) = params.actor else {
            panic!("agent expected")
        };
        let ctx = ToolContext {
            principal: run.principal(),
            agent: Some(run),
            session_id: Some(session),
            assistant_turn_id: Some(turn),
            granted_tools: Some(std::sync::Arc::new(
                ["remember", "recall", "update_memory", "forget"]
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
            )),
            suspend: aiplane_runtime::suspend::Suspend::Available,
            ..ToolContext::for_test(pool)
        };
        (state, ctx, organization, owned_agent)
    }

    #[tokio::test]
    async fn organization_memory_tools_merge_own_and_shared_notes_without_employee_profiles() {
        use aiplane_agents::db::agent_memories;
        let (state, ctx, organization, owned_agent) = organization_memory_context().await;
        let owner = AgentOwner::Organization(organization.clone());
        let own = MemoryScope::Agent {
            owner: &owner,
            agent_id: &owned_agent,
        };
        let shared = MemoryScope::Organization {
            organization_id: &organization,
        };
        let global = agent_memories::insert(
            &state.db,
            shared,
            MemoryKind::Fact,
            "Organization procedure",
        )
        .await
        .unwrap()
        .unwrap();
        let remembered = Remember
            .run(ctx.clone(), json!({"content":"Own lesson"}))
            .await
            .unwrap();
        assert_eq!(remembered["scope"], "agent");
        let recalled = Recall.run(ctx.clone(), json!({})).await.unwrap();
        assert_eq!(recalled["count"], 2);
        assert!(!recalled.to_string().contains("PRIVATE EMPLOYEE MARKER"));
        assert!(
            recalled["memories"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["id"] == global.id && row["scope"] == "organization")
        );
        let id = remembered["id"].as_str().unwrap();
        assert_eq!(
            UpdateMemory
                .run(ctx.clone(), json!({"id":id,"content":"Corrected lesson"}))
                .await
                .unwrap()["status"],
            "updated"
        );
        assert_eq!(
            agent_memories::get(&state.db, own, id)
                .await
                .unwrap()
                .unwrap()
                .content,
            "Corrected lesson"
        );
        assert_eq!(
            Forget.run(ctx.clone(), json!({"id":id})).await.unwrap()["status"],
            "forgotten"
        );
        assert!(
            agent_memories::list(&state.db, own, 10)
                .await
                .unwrap()
                .unwrap()
                .is_empty()
        );
        for scope in ["user", "organization"] {
            assert!(
                Remember
                    .run(ctx.clone(), json!({"content":"forbidden", "scope":scope}))
                    .await
                    .is_err()
            );
            assert!(
                UpdateMemory
                    .run(
                        ctx.clone(),
                        json!({"id":global.id,"content":"forbidden","scope":scope})
                    )
                    .await
                    .is_err()
            );
            assert!(
                Forget
                    .run(ctx.clone(), json!({"id":global.id,"scope":scope}))
                    .await
                    .is_err()
            );
        }
        assert_eq!(
            agent_memories::get(&state.db, shared, &global.id)
                .await
                .unwrap()
                .unwrap()
                .content,
            "Organization procedure"
        );
        let mut tx = aiplane_agents::db::WriteTx::begin(&state.db).await.unwrap();
        let (_, other) = agents::create_organization_agent_in(
            &mut tx,
            &organization,
            &NewPrincipal {
                name: "other",
                display: "Other",
                description: "",
            },
            r#"{"main":{}}"#,
            "employee",
            4,
        )
        .await
        .unwrap();
        let other = other.unwrap().principal.id;
        let foreign = agent_memories::insert_in(
            &mut tx,
            MemoryScope::Agent {
                owner: &owner,
                agent_id: &other,
            },
            MemoryKind::Fact,
            "Other agent lesson",
        )
        .await
        .unwrap()
        .unwrap();
        tx.commit().await.unwrap();
        assert_eq!(
            UpdateMemory
                .run(ctx.clone(), json!({"id":foreign.id,"content":"forbidden"}))
                .await
                .unwrap()["status"],
            "not_found"
        );
        assert_eq!(
            Forget.run(ctx, json!({"id":foreign.id})).await.unwrap()["status"],
            "not_found"
        );
        assert_eq!(
            agent_memories::get(
                &state.db,
                MemoryScope::Agent {
                    owner: &owner,
                    agent_id: &other
                },
                &foreign.id
            )
            .await
            .unwrap()
            .unwrap()
            .content,
            "Other agent lesson"
        );
    }

    #[tokio::test]
    async fn organization_memory_current_grant_and_activity_revocation_refuse_cached_context() {
        use aiplane_agents::db::{WriteTx, agent_memories, system_principals};
        use aiplane_core::server::principal::GrantKind;
        for revoke_grant in [true, false] {
            let (state, ctx, organization, owned_agent) = organization_memory_context().await;
            let owner = AgentOwner::Organization(organization);
            let scope = MemoryScope::Agent {
                owner: &owner,
                agent_id: &owned_agent,
            };
            agent_memories::insert(&state.db, scope, MemoryKind::Fact, "unchanged")
                .await
                .unwrap();
            let mut tx = WriteTx::begin(&state.db).await.unwrap();
            if revoke_grant {
                system_principals::remove_grant_in(
                    &mut tx,
                    &owned_agent,
                    GrantKind::Tool,
                    "remember",
                    "employee",
                )
                .await
                .unwrap();
            } else {
                assert!(
                    agents::set_agent_active_in(
                        &mut tx,
                        &owned_agent,
                        &owner,
                        false,
                        4,
                        "employee"
                    )
                    .await
                    .unwrap()
                );
            }
            tx.commit().await.unwrap();
            assert!(
                Remember
                    .run(ctx, json!({"content":"must not write"}))
                    .await
                    .is_err()
            );
            let rows = agent_memories::list(&state.db, scope, 10)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].content, "unchanged");
        }
    }

    #[tokio::test]
    async fn organization_memory_dispatch_requires_mutation_approval_and_keeps_recall_read_only() {
        use aiplane_runtime::{agents::profile::RunToolSource, server::tools::ToolSource};
        let (state, ctx, organization, owned_agent) = organization_memory_context().await;
        let run = ctx.agent.as_ref().unwrap();
        let registry = state.tools();
        let source = RunToolSource::new(registry.as_ref(), Some(run));
        let tool = source.get("remember").unwrap();
        let result = tool
            .run(ctx.clone(), json!({"content":"pending approval"}))
            .await
            .unwrap();
        let pause = aiplane_runtime::suspend::extract_suspend(&result).unwrap();
        assert_eq!(pause.kind, session_core::db::SuspensionKind::Approval);
        let owner = AgentOwner::Organization(organization);
        assert!(
            aiplane_agents::db::agent_memories::list(
                &state.db,
                MemoryScope::Agent {
                    owner: &owner,
                    agent_id: &owned_agent
                },
                10
            )
            .await
            .unwrap()
            .unwrap()
            .is_empty()
        );
        let recalled = source
            .get("recall")
            .unwrap()
            .run(ctx, json!({}))
            .await
            .unwrap();
        assert!(aiplane_runtime::suspend::extract_suspend(&recalled).is_none());
        assert_eq!(recalled["count"], 0);
    }
    #[tokio::test]
    async fn organization_standing_memory_uses_own_shared_knowledge_and_current_recall_ceiling() {
        use aiplane_agents::db::{agent_memories, system_principals};
        use aiplane_core::server::principal::GrantKind;
        let (state, ctx, organization, owned_agent) = organization_memory_context().await;
        let owner = AgentOwner::Organization(organization.clone());
        agent_memories::insert(
            &state.db,
            MemoryScope::Agent {
                owner: &owner,
                agent_id: &owned_agent,
            },
            MemoryKind::Fact,
            "Own standing lesson",
        )
        .await
        .unwrap();
        agent_memories::insert(
            &state.db,
            MemoryScope::Organization {
                organization_id: &organization,
            },
            MemoryKind::Fact,
            "Shared standing knowledge",
        )
        .await
        .unwrap();
        let run = ctx.agent.as_ref().unwrap();
        let section = aiplane_runtime::server::tools::memory::organization_standing_context(run)
            .await
            .unwrap()
            .unwrap();
        assert!(section.contains("Own standing lesson"));
        assert!(section.contains("Shared standing knowledge"));
        assert!(section.contains("untrusted"));
        assert!(!section.contains("PRIVATE EMPLOYEE MARKER"));
        system_principals::remove_grant(
            &state.db,
            &owned_agent,
            GrantKind::Tool,
            "recall",
            "employee",
        )
        .await
        .unwrap();
        assert!(
            aiplane_runtime::server::tools::memory::organization_standing_context(run)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn organization_model_request_contains_scoped_notes_and_drops_them_after_recall_revocation()
     {
        use aiplane_agents::db::{agent_memories, system_principals};
        use aiplane_core::server::{db::usage::UsageSource, principal::GrantKind};
        use aiplane_runtime::{
            agent_run::Actor,
            agents::profile::{Role, RunProfile, SpecSource},
            server::headless::{DriveParams, OpenParams, Owner, drive, open_session},
        };
        use wiremock::{
            Mock, MockServer, ResponseTemplate,
            matchers::{method, path},
        };
        let server = MockServer::start().await;
        let chunk = json!({"id":"response","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","content":"Done"},"finish_reason":"stop"}]});
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(format!("data: {chunk}\n\ndata: [DONE]\n\n")),
            )
            .mount(&server)
            .await;
        let (state, ctx, organization, owned_agent) =
            organization_memory_context_with_upstream(&server.uri()).await;
        let owner = AgentOwner::Organization(organization.clone());
        agent_memories::insert(
            &state.db,
            MemoryScope::Agent {
                owner: &owner,
                agent_id: &owned_agent,
            },
            MemoryKind::Fact,
            "OWN MODEL NOTE",
        )
        .await
        .unwrap();
        agent_memories::insert(
            &state.db,
            MemoryScope::Organization {
                organization_id: &organization,
            },
            MemoryKind::Fact,
            "SHARED MODEL NOTE",
        )
        .await
        .unwrap();
        let first = ctx.assistant_turn_id.clone().unwrap();
        drive(
            &state,
            DriveParams {
                actor: Actor::Agent(ctx.agent.unwrap()),
                session_id: ctx.session_id.clone().unwrap(),
                assistant_turn_id: first.clone(),
                model: "model".into(),
                source: UsageSource::Agent,
                history_limit: None,
            },
        )
        .await;
        let turn =
            session_core::db::get_turn(&state.db, ctx.session_id.as_deref().unwrap(), &first)
                .await
                .unwrap()
                .unwrap();
        assert_eq!(
            turn.status,
            session_core::db::TurnStatus::Completed,
            "{:?}",
            turn.error_message
        );
        system_principals::remove_grant(
            &state.db,
            &owned_agent,
            GrantKind::Tool,
            "recall",
            "employee",
        )
        .await
        .unwrap();
        let profile=RunProfile::load_from(&state,&owned_agent,SpecSource::Draft(json!({"main":{"model":"model","tools":["remember","recall","update_memory","forget"]}})),Role::Main,&Default::default()).await.unwrap();
        let (session, assistant) = open_session(
            &state.db,
            OpenParams {
                owner: Owner::Run {
                    principal_id: &owned_agent,
                    parent_turn_id: None,
                    agent_version: Some(agents::DRAFT_VERSION),
                },
                title: "Next memory task",
                prompt: "Continue",
                model: "model",
                existing_session: ctx.session_id,
                personal_agent_id: None,
            },
        )
        .await
        .unwrap();
        let chain = std::sync::Arc::new(aiplane_core::server::run_chain::RunChain::root(
            &session,
            None,
            aiplane_core::server::run_chain::Frame::for_principal(&profile.principal, None),
        ));
        drive(
            &state,
            profile.drive_params(&session, &assistant, chain).unwrap(),
        )
        .await;
        let turn = session_core::db::get_turn(&state.db, &session, &assistant)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            turn.status,
            session_core::db::TurnStatus::Completed,
            "{:?}",
            turn.error_message
        );
        let bodies: Vec<Value> = server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .map(|request| serde_json::from_slice(&request.body).unwrap())
            .collect();
        assert_eq!(bodies.len(), 2);
        let initial = bodies[0]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|message| message["role"] == "system")
            .map(|message| message["content"].as_str().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(initial.contains("OWN MODEL NOTE"));
        assert!(initial.contains("SHARED MODEL NOTE"));
        assert!(!initial.contains("PRIVATE EMPLOYEE MARKER"));
        let next = bodies[1]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|message| message["role"] == "system")
            .map(|message| message["content"].as_str().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!next.contains("OWN MODEL NOTE"));
        assert!(!next.contains("SHARED MODEL NOTE"));
        assert!(!next.contains("PRIVATE EMPLOYEE MARKER"));
    }
}
