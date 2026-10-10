// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Durable memory: `remember`, `recall`, `update_memory`, `forget`.
//!
//! `remember` stores a short free-text fact about the caller; `recall` pulls
//! those facts back in a later conversation, each with its `id`;
//! `update_memory` and `forget` correct or drop one by that id. All four are
//! scoped to `ctx.user_id`, so the model can only ever read/write the current
//! user's own memories — there is no cross-user path, and the scoping lives in
//! the SQL as well as here. State lives in `db::user_memories`.
//!
//! The correction pair is not a nicety. Without it the store is
//! append-only: told "I'm not in the platform team any more", the model can
//! only add a second, contradicting fact, and every later `recall` returns
//! both — so it has to guess which one still holds, and the guessing gets
//! worse the longer an account lives.
//!
//! These are intentionally low-risk: no code execution, no network, no
//! filesystem — just the gateway's own SQLite, keyed by the
//! authenticated user.

use serde::Deserialize;
use serde_json::{Value, json};
use shared::api::ToolDef;

use aiplane_core::server::db::user_memories::{self, Memory, MemoryKind};
use aiplane_runtime::server::tools::{Tool, ToolContext, ToolError, ToolFuture};

fn personal_dot_scope<'a>(
    ctx: &'a ToolContext,
    tool: &str,
) -> Result<Option<(&'a str, &'a str)>, ToolError> {
    let user_id = ctx.person(tool)?;
    match ctx.personal_dot.as_ref() {
        Some(dot) if dot.owner_user_id == user_id => Ok(Some((&dot.owner_user_id, &dot.dot_id))),
        Some(_) => Err(ToolError::Failed(format!(
            "`{tool}` refused: the personal Dot owner does not match the acting user."
        ))),
        None => Ok(None),
    }
}

fn scope_name(scope: Option<(&str, &str)>) -> &'static str {
    if scope.is_some() { "dot" } else { "user" }
}

fn resolve_scope(
    requested: Option<&str>,
    dot_scope: Option<(&str, &str)>,
) -> Result<&'static str, ToolError> {
    match requested {
        None => Ok(scope_name(dot_scope)),
        Some("dot") if dot_scope.is_some() => Ok("dot"),
        Some("user") if dot_scope.is_none() => Ok("user"),
        Some("dot") => Err(ToolError::InvalidArgs(
            "Dot memory is available only in a personal Dot chat".into(),
        )),
        Some("user") => Ok("user"),
        Some(other) => Err(ToolError::InvalidArgs(format!(
            "unknown memory scope `{other}` — use `dot` or `user`"
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
const MAX_CONTENT_LEN: usize = 2_000;

// ---------------------------------------------------------------------------
// remember

pub struct Remember;

#[derive(Deserialize)]
struct RememberArgs {
    content: String,
    #[serde(default)]
    kind: Option<String>,
}

impl Tool for Remember {
    fn id(&self) -> &str {
        "remember"
    }

    fn schema(&self) -> ToolDef {
        ToolDef::function(
            self.id(),
            "Store a short, durable fact about the user so you can recall it in future \
             conversations — e.g. their preferences, ongoing projects, or names they ask you to \
             keep. Use it when the user shares something worth remembering long-term. Do not \
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
                                        user likes things, 'project' for context about what \
                                        they're working on, 'fact' for any other stable detail. \
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
            if content.len() > MAX_CONTENT_LEN {
                return Err(ToolError::InvalidArgs(format!(
                    "content too long ({} chars); keep memories under {MAX_CONTENT_LEN}",
                    content.len()
                )));
            }
            let dot_scope = personal_dot_scope(&ctx, self.id())?;
            let row = match dot_scope {
                Some((owner, dot)) => {
                    aiplane_agents::db::dot_memories::insert(&ctx.db, owner, dot, kind, content)
                        .await
                        .map_err(|e| ToolError::Failed(format!("storing Dot memory: {e}")))?
                        .ok_or_else(|| {
                            ToolError::Failed("the personal Dot is no longer available".into())
                        })?
                }
                None => user_memories::insert(&ctx.db, ctx.person(self.id())?, kind, content)
                    .await
                    .map_err(|e| ToolError::Failed(format!("storing memory: {e}")))?,
            };
            Ok(json!({
                "status": "remembered",
                "id": row.id,
                "scope": scope_name(dot_scope),
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
    fn id(&self) -> &str {
        "recall"
    }

    fn schema(&self) -> ToolDef {
        ToolDef::function(
            self.id(),
            "Retrieve everything you've remembered about this user (their preferences, project \
             context, and facts). Takes no arguments and returns all stored memories, newest \
             first — you don't need to craft a query. Call it whenever the user refers to \
             themselves, their preferences, or earlier context, then use whatever is relevant. \
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
            let dot_scope = personal_dot_scope(&ctx, self.id())?;
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
            if let Some((owner, dot)) = dot_scope {
                let dot_rows =
                    aiplane_agents::db::dot_memories::list(&ctx.db, owner, dot, MAX_RECALL_LIMIT)
                        .await
                        .map_err(|e| ToolError::Failed(format!("recalling Dot memories: {e}")))?
                        .ok_or_else(|| {
                            ToolError::Failed("the personal Dot is no longer available".into())
                        })?;
                rows.extend(dot_rows.into_iter().map(|memory| (memory, "dot")));
            }
            rows.sort_by(|(a, _), (b, _)| b.created_at.cmp(&a.created_at));
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
                        "enum": ["dot", "user"],
                        "description": "Scope reported by `recall`; a personal Dot may change only `dot`."
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
            if content.len() > MAX_CONTENT_LEN {
                return Err(ToolError::InvalidArgs(format!(
                    "content too long ({} chars); keep memories under {MAX_CONTENT_LEN}",
                    content.len()
                )));
            }

            let dot_scope = personal_dot_scope(&ctx, self.id())?;
            let scope = resolve_scope(args.scope.as_deref(), dot_scope)?;
            if scope == "user" && dot_scope.is_some() {
                return Err(ToolError::Failed(
                    "user-wide memories are read-only in a personal Dot chat".into(),
                ));
            }
            if scope == "dot" && dot_scope.is_none() {
                return Err(ToolError::Failed(
                    "Dot memory can only be changed from that Dot's chat".into(),
                ));
            }
            // Read the existing row first, scoped to this user: it gives us
            // the current `kind` to preserve, and it turns "someone else's id"
            // into the same not-found answer as "no such id" (no existence
            // leak across users).
            let existing = match (scope, dot_scope) {
                ("dot", Some((owner, dot))) => {
                    aiplane_agents::db::dot_memories::get(&ctx.db, owner, dot, &args.id)
                        .await
                        .map_err(|e| ToolError::Failed(format!("looking up Dot memory: {e}")))?
                }
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

            let updated = match (scope, dot_scope) {
                ("dot", Some((owner, dot))) => aiplane_agents::db::dot_memories::update(
                    &ctx.db, owner, dot, &args.id, kind, content,
                )
                .await
                .map_err(|e| ToolError::Failed(format!("updating Dot memory: {e}")))?,
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
                        "enum": ["dot", "user"],
                        "description": "Scope reported by `recall`; a personal Dot may change only `dot`."
                    }
                }
            }),
        )
    }

    fn run<'a>(&'a self, ctx: ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let args: ForgetArgs = serde_json::from_value(args)
                .map_err(|e| ToolError::InvalidArgs(format!("expected {{id: string}}: {e}")))?;

            let dot_scope = personal_dot_scope(&ctx, self.id())?;
            let scope = resolve_scope(args.scope.as_deref(), dot_scope)?;
            if scope == "user" && dot_scope.is_some() {
                return Err(ToolError::Failed(
                    "user-wide memories are read-only in a personal Dot chat".into(),
                ));
            }
            if scope == "dot" && dot_scope.is_none() {
                return Err(ToolError::Failed(
                    "Dot memory can only be changed from that Dot's chat".into(),
                ));
            }
            // Fetch before deleting so the result can echo *what* was
            // forgotten — the model needs that to confirm to the user, and
            // after the delete it's gone.
            let existing = match (scope, dot_scope) {
                ("dot", Some((owner, dot))) => {
                    aiplane_agents::db::dot_memories::get(&ctx.db, owner, dot, &args.id)
                        .await
                        .map_err(|e| ToolError::Failed(format!("looking up Dot memory: {e}")))?
                }
                _ => user_memories::get(&ctx.db, ctx.person(self.id())?, &args.id)
                    .await
                    .map_err(|e| ToolError::Failed(format!("looking up memory: {e}")))?,
            };
            let Some(existing) = existing else {
                return Ok(not_found(&args.id));
            };
            let deleted = match (scope, dot_scope) {
                ("dot", Some((owner, dot))) => {
                    aiplane_agents::db::dot_memories::delete(&ctx.db, owner, dot, &args.id)
                        .await
                        .map_err(|e| ToolError::Failed(format!("deleting Dot memory: {e}")))?
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

    async fn personal_dot_ctx(pool: &db::Pool, user_id: &str) -> ToolContext {
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
        let (_, dot) = agents::create_personal_dot(
            pool,
            &NewPrincipal {
                name: "memory-dot",
                display: "Memory Dot",
                description: "",
            },
            r#"{"main":{}}"#,
            user_id,
            4,
        )
        .await
        .unwrap();
        let dot_id = dot.unwrap().principal.id;
        ToolContext {
            personal_dot: Some(aiplane_runtime::agent_run::PersonalDotIdentity {
                owner_user_id: user_id.to_string(),
                dot_id,
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
    async fn personal_dot_memory_tools_keep_dot_and_user_memory_scopes_separate() {
        let pool = fresh().await;
        let dot_ctx = personal_dot_ctx(&pool, "alice").await;
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
                dot_ctx.clone(),
                json!({"content": "avoid the same deployment pitfall"}),
            )
            .await
            .unwrap();
        assert_eq!(saved["scope"], "dot");

        let recalled = Recall.run(dot_ctx.clone(), Value::Null).await.unwrap();
        assert_eq!(recalled["count"], 2);
        assert!(
            recalled["memories"]
                .as_array()
                .unwrap()
                .iter()
                .any(|m| { m["id"] == user_memory.id && m["scope"] == "user" })
        );
        let dot_memory = recalled["memories"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["scope"] == "dot")
            .unwrap();
        let dot_id = dot_memory["id"].as_str().unwrap();
        let updated = UpdateMemory
            .run(
                dot_ctx.clone(),
                json!({"id": dot_id, "scope": "dot", "content": "avoid the verified deployment pitfall"}),
            )
            .await
            .unwrap();
        assert_eq!(updated["status"], "updated");
        assert_eq!(updated["scope"], "dot");
        let forgotten = Forget
            .run(dot_ctx.clone(), json!({"id": dot_id, "scope": "dot"}))
            .await
            .unwrap();
        assert_eq!(forgotten["status"], "forgotten");
        assert_eq!(forgotten["scope"], "dot");

        let cannot_update_profile = UpdateMemory
            .run(
                dot_ctx.clone(),
                json!({"id": user_memory.id, "scope": "user", "content": "changed"}),
            )
            .await
            .unwrap_err();
        assert!(cannot_update_profile.to_string().contains("read-only"));
        let cannot_forget_profile = Forget
            .run(
                dot_ctx.clone(),
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
    async fn a_personal_dot_cannot_use_an_identity_for_a_different_user() {
        let pool = fresh().await;
        let mut dot_ctx = personal_dot_ctx(&pool, "alice").await;
        dot_ctx.personal_dot.as_mut().unwrap().owner_user_id = "bob".into();
        let error = Remember
            .run(dot_ctx, json!({"content": "a private lesson"}))
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
}
