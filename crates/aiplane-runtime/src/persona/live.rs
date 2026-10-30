// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use std::sync::Arc;

use aiplane_agents::db::agents;
use aiplane_core::server::db::{user_tool_prefs, users};
use aiplane_core::server::principal::Principal;
use serde_json::Value;
use shared::api::ToolDef;

use crate::agents::approval::Permissions;
use crate::agents::spec::AgentSpec;
use crate::server::AppState;
use crate::server::tools::mcp::manager::{AskContext, CompositeToolSource};
use crate::server::tools::{
    GrantedToolSource, Tool, ToolContext, ToolError, ToolFuture, ToolSource, catalog,
};

pub(super) struct LivePersonalAgentTool {
    pub state: Arc<AppState>,
    pub initial: Arc<dyn Tool>,
    pub initial_bindings: std::collections::BTreeMap<String, crate::agents::bind::BindSource>,
}

impl Tool for LivePersonalAgentTool {
    fn execution_facts(&self) -> Option<Value> {
        binding_facts(&self.initial_bindings, self.initial.execution_facts())
    }

    fn phase(&self) -> crate::server::tools::ToolPhase {
        self.initial.phase()
    }

    fn id(&self) -> &str {
        self.initial.id()
    }

    fn action_catalogue(&self) -> Option<&'static [crate::server::tools::ToolAction]> {
        self.initial.action_catalogue()
    }
    fn classify_actions<'a>(
        &'a self,
        ctx: ToolContext,
        args: Value,
    ) -> crate::server::tools::ToolActionFuture<'a> {
        Box::pin(async move {
            let mut ctx = ctx;
            crate::server::tools::ask_first::validate_execution_facts(
                &ctx,
                self.id(),
                &self.execution_facts(),
            )?;
            if let Some(pending) = ctx.pending_execution_facts.as_mut() {
                pending.execution_facts = self.initial.execution_facts();
            }
            if self.initial_bindings.is_empty() {
                self.initial.classify_actions(ctx, args).await
            } else {
                crate::agents::bind::BoundTool::new(
                    self.initial.clone(),
                    self.initial_bindings.clone(),
                    None,
                    Arc::default(),
                )
                .classify_actions(ctx, args)
                .await
            }
        })
    }

    fn schema(&self) -> ToolDef {
        self.initial.schema()
    }

    fn max_duration(&self) -> Option<std::time::Duration> {
        self.initial.max_duration()
    }

    fn sensitive_args(&self) -> bool {
        self.initial.sensitive_args()
    }

    fn read_only(&self) -> bool {
        self.initial.read_only()
    }

    fn egress(&self) -> bool {
        self.initial.egress()
    }

    fn changes_state(&self) -> bool {
        self.initial.changes_state()
    }

    fn run<'a>(&'a self, mut ctx: ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let journal = ctx.clone();
            let fresh = ctx.pending_execution_facts.is_none();
            let mut admission_recorded = false;
            let result = async {
            if let Some(execution) = ctx.personal_execution.as_ref() {
                let root_turn = crate::agents::router::local::validate_execution(execution).await?;
                if execution.meter().remaining().is_none()
                    || execution.unknown_token_usage(execution.local_budget())
                {
                    return Err(self.unavailable());
                }
                if crate::server::background::for_turn(
                    &self.state.db,
                    &crate::server::background::AutomationOwner::User(execution.identity().owner_user_id.clone()),
                    &root_turn,
                )
                .await
                .map_err(Self::load_error)?
                .is_some_and(|context| !context.tools_enabled)
                {
                    return Err(self.unavailable());
                }
            }
            let identity = ctx
                .personal_agent
                .as_ref()
                .ok_or_else(|| self.unavailable())?;
            if !matches!(&ctx.principal, Principal::User { id, .. } if id == &identity.owner_user_id)
            {
                return Err(self.unavailable());
            }
            if let Some(turn_id) = ctx.assistant_turn_id.as_deref()
                && crate::server::background::for_turn(
                    &self.state.db,
                    &crate::server::background::AutomationOwner::User(identity.owner_user_id.clone()),
                    turn_id,
                )
                .await
                .map_err(Self::load_error)?
                .is_some_and(|context| !context.tools_enabled)
            {
                return Err(self.unavailable());
            }
            let owner = users::find_by_id(&self.state.db, &identity.owner_user_id)
                .await
                .map_err(Self::load_error)?
                .ok_or_else(|| self.unavailable())?;
            let owned_agent = agents::get_personal_agent(&self.state.db, &owner.id, &identity.agent_id)
                .await
                .map_err(Self::load_error)?
                .filter(|owned_agent| owned_agent.active && owned_agent.agent.principal.disabled_at.is_none())
                .ok_or_else(|| self.unavailable())?;
            let value = serde_json::from_str(&owned_agent.agent.draft_spec)
                .map_err(|error| ToolError::Failed(format!("Could not read the personal agent's tool rules: {error}. Correct its access settings before retrying.")))?;
            let spec = AgentSpec::from_value(&value)
                .map_err(|error| ToolError::Failed(format!("Could not read the personal agent's tool rules: {error}. Correct its access settings before retrying.")))?;
            if !spec.main.tools.iter().any(|id| id == self.id()) {
                return Err(self.unavailable());
            }
            let principal = Principal::User {
                id: owner.id.clone(),
                roles: owner.roles,
            };
            let layer = self.state.mcp_layer_for_agent(&principal, Some(identity), AskContext::Chat).await;
            let mut granted = self.state.granted_tool_ids(&principal, &layer).await;
            let disabled = user_tool_prefs::disabled_for_user(&self.state.db, &owner.id)
                .await
                .map_err(Self::load_error)?;
            catalog::retain_enabled(&mut granted, &disabled);
            granted.retain(|id| {
                spec.main.tools.contains(id)
                    && ctx
                        .granted_tools
                        .as_ref()
                        .is_some_and(|initial| initial.contains(id))
            });
            if !granted.iter().any(|id| id == self.id()) {
                return Err(self.unavailable());
            }
            let tools = self.state.tools();
            let comfyui = self.state.comfyui().as_ref().map(|handle| {
                crate::server::comfyui_tool::ComfyuiToolSource::new((**handle).clone())
            });
            let source =
                CompositeToolSource::new(tools.as_ref(), &layer).with_comfyui(comfyui.as_ref());
            let current = GrantedToolSource::new(&source, granted.iter().cloned());
            let tool = current.get(self.id()).ok_or_else(|| self.unavailable())?;
            let access =
                super::personal_agent::current_pool_access(&self.state, &principal, &ctx.pool_access);
            ctx.principal = principal;
            ctx.pool_access = access;
            ctx.granted_tools = Some(Arc::new(granted.into_iter().collect()));
            if fresh {
                admission_recorded = true;
                let identity = tool.execution_facts().and_then(|facts| serde_json::from_value::<crate::server::tools::mcp::McpDispatchFacts>(facts).ok());
                if !crate::agents::audit::record_personal_tool_call(&ctx, ctx.call_id.as_deref(), self.id(), true, "granted", identity.as_ref()).await {
                    return Err(ToolError::AuditUnavailable);
                }
            }
            let bindings=crate::agents::bind::ToolBinds::from_spec(&spec).for_tool(self.id(),&tool.schema()).map_err(|_| self.unavailable())?;
            let original_facts=tool.execution_facts();
            let facts=binding_facts(&bindings,original_facts.clone());
            crate::server::tools::ask_first::validate_execution_facts(&ctx,self.id(),&facts)?;
            if let Some(pending)=ctx.pending_execution_facts.as_mut() { pending.execution_facts=original_facts; }
            let gated=Permissions::from_spec(&spec).gate_agent(tool);
            let result=if bindings.is_empty() {
                gated.run(ctx,args).await
            } else {
                crate::agents::bind::BoundTool::new(gated,bindings,None,Arc::default()).run(ctx,args).await
            }?;
            match crate::suspend::extract_suspend(&result) {
                Some(mut request) if request.kind==session_core::db::SuspensionKind::Approval=> {
                    request.execution_facts=facts;
                    Ok(crate::suspend::tool_suspend(request))
                },
                _=>Ok(result),
            }
            }.await;
            if fresh
                && !admission_recorded
                && !crate::agents::audit::record_personal_tool_call(
                    &journal,
                    journal.call_id.as_deref(),
                    self.id(),
                    false,
                    "not_granted",
                    None,
                )
                .await
            {
                return Err(ToolError::AuditUnavailable);
            }
            result
        })
    }
}

fn binding_facts(
    bindings: &std::collections::BTreeMap<String, crate::agents::bind::BindSource>,
    facts: Option<Value>,
) -> Option<Value> {
    if bindings.is_empty() {
        return facts;
    }
    let descriptor: std::collections::BTreeMap<_, _> = bindings
        .iter()
        .map(|(name, source)| {
            let value = match source {
                crate::agents::bind::BindSource::Const(value) => serde_json::json!({"const":value}),
                crate::agents::bind::BindSource::State(path) => serde_json::json!({"state":path}),
                crate::agents::bind::BindSource::Route(path) => serde_json::json!({"route":path}),
            };
            (name, value)
        })
        .collect();
    let fingerprint = aiplane_core::server::crypto::sha256_hex(
        serde_json::to_string(&descriptor)
            .expect("binding descriptors serialize")
            .as_bytes(),
    );
    Some(serde_json::json!({"personal_agent_bindings":fingerprint,"tool":facts}))
}

impl LivePersonalAgentTool {
    fn unavailable(&self) -> ToolError {
        ToolError::Failed(format!(
            "`{}` is unavailable to this personal agent under its current access settings. Review the agent's active state, selected tools and the owner's permissions before retrying.",
            self.id()
        ))
    }

    fn load_error(error: aiplane_core::server::db::DbError) -> ToolError {
        ToolError::Failed(format!(
            "Could not load the personal agent's current access settings: {error}. Retry after the database is available."
        ))
    }
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn personal_bound_approval_replays_through_the_real_runner_and_classifies_bound_actions()
    {
        let (tool, mut ctx) = fixture_with_probe(Arc::new(FactsEcho)).await;
        let mut value = spec("always_ask");
        value["main"]["tool_resources"]["company_echo"]["bind"] =
            json!({"message":{"const":"Original approved collection"}});
        save(&tool, &ctx, value.clone()).await;
        let typed = AgentSpec::from_value(&value).unwrap();
        let persona = crate::persona::ChatPersona::new("Agent", Arc::new(ToolRegistry::new()))
            .with_tool_access(&typed);
        let registry = tool.state.tools();
        let source = persona.source(registry.as_ref(), Some(&tool.state));
        let calls = [crate::server::tools::runner::ToolCallRef {
            id: "binding-call".into(),
            name: "company_echo".into(),
            arguments_raw: "{}".into(),
        }];
        let scanner = crate::server::tools::injection::InjectionScan::default();
        let fresh =
            crate::server::tools::runner::execute_tool_calls(&source, &ctx, &calls, &scanner).await;
        assert!(!fresh[0].failed, "{}", fresh[0].body);
        let request = crate::suspend::extract_suspend(&fresh[0].body).expect("approval requested");
        assert_eq!(request.actions, Some(vec!["original".into()]));
        ctx.suspend = crate::suspend::Suspend::Decided(
            session_core::db::SuspensionKind::Approval,
            session_core::db::Decision::AllowOnce,
        );
        ctx.pending_execution_facts = Some(crate::server::tools::PendingExecutionFacts {
            execution_facts: request.execution_facts,
        });
        ctx.pending_actions = Some(crate::server::tools::PendingActionFacts {
            actions: request.actions,
        });
        let resumed =
            crate::server::tools::runner::execute_tool_calls(&source, &ctx, &calls, &scanner).await;
        assert!(!resumed[0].failed, "{}", resumed[0].body);
        assert_eq!(
            resumed[0].body,
            json!({"message":"Original approved collection"})
        );
        for binding in [
            Some(json!({"message":{"const":"Different collection"}})),
            None,
        ] {
            if let Some(binding) = binding {
                value["main"]["tool_resources"]["company_echo"]["bind"] = binding;
            } else {
                value["main"]["tool_resources"]["company_echo"]
                    .as_object_mut()
                    .unwrap()
                    .remove("bind");
            }
            save(&tool, &ctx, value.clone()).await;
            let changed = AgentSpec::from_value(&value).unwrap();
            let persona = crate::persona::ChatPersona::new("Agent", Arc::new(ToolRegistry::new()))
                .with_tool_access(&changed);
            let changed_source = persona.source(registry.as_ref(), Some(&tool.state));
            let refused = crate::server::tools::runner::execute_tool_calls(
                &changed_source,
                &ctx,
                &calls,
                &scanner,
            )
            .await;
            assert!(refused[0].failed, "{}", refused[0].body);
        }
    }

    #[tokio::test]
    async fn personal_binding_approval_preserves_underlying_facts_and_rejects_changes_or_removal() {
        let (tool, mut ctx) = fixture_with_probe(Arc::new(FactsEcho)).await;
        let mut value = spec("always_ask");
        value["main"]["tool_resources"]["company_echo"]["bind"] =
            json!({"message":{"const":"Original approved collection"}});
        save(&tool, &ctx, value.clone()).await;
        let pause = tool.run(ctx.clone(), json!({})).await.unwrap();
        let request = crate::suspend::extract_suspend(&pause).expect("approval requested");
        ctx.suspend = crate::suspend::Suspend::Decided(
            session_core::db::SuspensionKind::Approval,
            session_core::db::Decision::AllowOnce,
        );
        ctx.pending_execution_facts = Some(crate::server::tools::PendingExecutionFacts {
            execution_facts: request.execution_facts,
        });
        assert_eq!(
            tool.run(ctx.clone(), json!({})).await.unwrap(),
            json!({"message":"Original approved collection"})
        );
        value["main"]["tool_resources"]["company_echo"]["bind"]["message"] =
            json!({"const":"Different collection"});
        save(&tool, &ctx, value.clone()).await;
        assert!(tool.run(ctx.clone(), json!({})).await.is_err());
        value["main"]["tool_resources"]["company_echo"]
            .as_object_mut()
            .unwrap()
            .remove("bind");
        save(&tool, &ctx, value).await;
        assert!(
            tool.run(ctx, json!({"message":"Different collection"}))
                .await
                .is_err()
        );
    }

    struct FactsEcho;
    impl Tool for FactsEcho {
        fn id(&self) -> &str {
            "company_echo"
        }

        fn egress(&self) -> bool {
            false
        }
        fn schema(&self) -> ToolDef {
            crate::server::tools::echo::Echo.schema()
        }
        fn execution_facts(&self) -> Option<Value> {
            Some(json!({"credential":"fixture"}))
        }
        fn action_catalogue(&self) -> Option<&'static [crate::server::tools::ToolAction]> {
            Some(&[
                crate::server::tools::ToolAction {
                    id: "original",
                    description: "Read the original collection",
                },
                crate::server::tools::ToolAction {
                    id: "different",
                    description: "Read another collection",
                },
            ])
        }
        fn classify_actions<'a>(
            &'a self,
            _ctx: ToolContext,
            args: Value,
        ) -> crate::server::tools::ToolActionFuture<'a> {
            Box::pin(async move {
                Ok(Some(vec![
                    if args["message"] == "Original approved collection" {
                        "original"
                    } else {
                        "different"
                    }
                    .into(),
                ]))
            })
        }
        fn run<'a>(&'a self, ctx: ToolContext, args: Value) -> ToolFuture<'a> {
            Box::pin(async move {
                crate::server::tools::ask_first::validate_execution_facts(
                    &ctx,
                    self.id(),
                    &self.execution_facts(),
                )?;
                crate::server::tools::echo::Echo.run(ctx, args).await
            })
        }
    }

    #[tokio::test]
    async fn live_personal_tools_enforce_the_current_saved_constant_binding() {
        let (tool, ctx) = fixture_with_echo(true).await;
        let identity = ctx.personal_agent.as_ref().unwrap();
        let mut value = spec("always_allow");
        value["main"]["tool_resources"]["company_echo"]["bind"] =
            serde_json::json!({"message":{"const":"Current reviewed collection"}});
        sqlx::query("UPDATE agents SET draft_spec=? WHERE principal_id=?")
            .bind(value.to_string())
            .bind(&identity.agent_id)
            .execute(&ctx.db)
            .await
            .unwrap();
        let result = tool
            .run(ctx, serde_json::json!({"message":"Another collection"}))
            .await
            .unwrap();
        assert_eq!(
            result,
            serde_json::json!({"message":"Current reviewed collection"})
        );
    }

    #[tokio::test]
    async fn live_admission_is_audited_once_without_opaque_metadata_or_resume_duplicates() {
        let (tool, mut ctx) = fixture().await;
        ctx.call_id = Some("call-audit".into());
        assert!(
            tool.run(ctx.clone(), json!({"secret":"not-in-audit"}))
                .await
                .is_ok()
        );
        let events = aiplane_agents::db::agent_audit::for_principal(
            &ctx.db,
            &ctx.personal_agent.as_ref().unwrap().owner_user_id,
        )
        .await
        .unwrap();
        let calls: Vec<_> = events
            .iter()
            .filter(|event| event.kind == "tool_call")
            .collect();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].detail["decision"], "allowed");
        assert_eq!(calls[0].detail["call_id"], "call-audit");
        assert!(calls[0].detail.get("identity").is_none());
        assert!(!calls[0].detail.to_string().contains("not-in-audit"));
        ctx.pending_execution_facts = Some(crate::server::tools::PendingExecutionFacts {
            execution_facts: None,
        });
        assert!(tool.run(ctx.clone(), json!({})).await.is_ok());
        let events = aiplane_agents::db::agent_audit::for_principal(
            &ctx.db,
            &ctx.personal_agent.as_ref().unwrap().owner_user_id,
        )
        .await
        .unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == "tool_call")
                .count(),
            1
        );
        ctx.pending_execution_facts = None;
        ctx.call_id = Some("call-revoked".into());
        sqlx::query("UPDATE users SET roles_json='[]' WHERE id='owner'")
            .execute(&ctx.db)
            .await
            .unwrap();
        assert!(tool.run(ctx.clone(), json!({})).await.is_err());
        let events = aiplane_agents::db::agent_audit::for_principal(
            &ctx.db,
            &ctx.personal_agent.as_ref().unwrap().owner_user_id,
        )
        .await
        .unwrap();
        let calls: Vec<_> = events
            .iter()
            .filter(|event| event.kind == "tool_call")
            .collect();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].detail["decision"], "denied");
        assert!(calls[0].detail.get("identity").is_none());
    }

    #[tokio::test]
    async fn failed_admission_audit_stops_before_the_approval_gate() {
        let (tool, mut ctx) = fixture().await;
        ctx.call_id = Some("blocked-audit".into());
        save(&tool, &ctx, spec("always_ask")).await;
        sqlx::query("CREATE TRIGGER deny_call_audit BEFORE INSERT ON agent_audit WHEN NEW.kind='tool_call' BEGIN SELECT RAISE(ABORT, 'audit unavailable'); END").execute(&ctx.db).await.unwrap();
        let error = tool.run(ctx.clone(), json!({})).await.unwrap_err();
        assert!(
            error
                .to_string()
                .contains(crate::agents::audit::LOG_UNAVAILABLE)
        );
        let events = aiplane_agents::db::agent_audit::for_principal(&ctx.db, "owner")
            .await
            .unwrap();
        assert!(!events.iter().any(|event| event.kind == "tool_call"));
    }

    struct AuditedStep(crate::server::tools::ToolPhase);
    impl Tool for AuditedStep {
        fn id(&self) -> &str {
            "company_echo"
        }

        fn egress(&self) -> bool {
            false
        }
        fn schema(&self) -> ToolDef {
            Echo.schema()
        }
        fn phase(&self) -> crate::server::tools::ToolPhase {
            self.0
        }
        fn run<'a>(&'a self, ctx: ToolContext, args: Value) -> ToolFuture<'a> {
            Box::pin(async move {
                if !crate::agents::audit::record_personal_tool_call(
                    &ctx,
                    ctx.call_id.as_deref(),
                    "company_echo",
                    true,
                    "granted",
                    None,
                )
                .await
                {
                    return Err(ToolError::AuditUnavailable);
                }
                Echo.run(ctx, args).await
            })
        }
    }
    struct PendingSideEffect {
        phase: crate::server::tools::ToolPhase,
        release: Option<Arc<tokio::sync::Notify>>,
    }
    impl Tool for PendingSideEffect {
        fn id(&self) -> &str {
            "get_current_timestamp"
        }

        fn egress(&self) -> bool {
            false
        }
        fn schema(&self) -> ToolDef {
            crate::server::tools::time::CurrentTimestamp.schema()
        }
        fn phase(&self) -> crate::server::tools::ToolPhase {
            self.phase
        }
        fn run<'a>(&'a self, ctx: ToolContext, _args: Value) -> ToolFuture<'a> {
            Box::pin(async move {
                if let Some(release) = &self.release {
                    release.notified().await;
                }
                sqlx::query("INSERT INTO app_settings(key,value_json,updated_at) VALUES('audit-sibling-effect','true',?)").bind(jiff::Timestamp::now().to_string()).execute(&ctx.db).await.map_err(|error|ToolError::Failed(error.to_string()))?;
                Ok(json!({"stored":true}))
            })
        }
    }

    #[tokio::test]
    async fn fatal_audit_stops_pending_concurrent_siblings_and_later_phases_in_call_order() {
        use crate::server::tools::{
            ToolPhase,
            runner::{self, ToolFailureCause},
        };
        for concurrent in [false, true] {
            let (_, ctx) = fixture().await;
            sqlx::query("CREATE TRIGGER deny_step_audit BEFORE INSERT ON agent_audit WHEN NEW.kind='tool_call' BEGIN SELECT RAISE(ABORT, 'audit unavailable'); END").execute(&ctx.db).await.unwrap();
            let release = Arc::new(tokio::sync::Notify::new());
            let tools = ToolRegistry::new()
                .with(AuditedStep(if concurrent {
                    ToolPhase::Concurrent
                } else {
                    ToolPhase::WritesState
                }))
                .with(PendingSideEffect {
                    phase: if concurrent {
                        ToolPhase::Concurrent
                    } else {
                        ToolPhase::ActsOnState
                    },
                    release: concurrent.then(|| release.clone()),
                });
            let calls = [
                runner::ToolCallRef {
                    id: "fatal".into(),
                    name: "company_echo".into(),
                    arguments_raw: "{}".into(),
                },
                runner::ToolCallRef {
                    id: "pending".into(),
                    name: "get_current_timestamp".into(),
                    arguments_raw: "{}".into(),
                },
            ];
            let results = tokio::time::timeout(
                std::time::Duration::from_secs(1),
                runner::execute_tool_calls(
                    &tools,
                    &ctx,
                    &calls,
                    &crate::server::tools::injection::InjectionScan::default(),
                ),
            )
            .await
            .unwrap();
            release.notify_waiters();
            assert_eq!(
                results
                    .iter()
                    .map(|record| record.call_id.as_str())
                    .collect::<Vec<_>>(),
                vec!["fatal", "pending"]
            );
            assert!(results.iter().all(|record| record.failure_cause
                == Some(ToolFailureCause::AuditUnavailable)
                && record.failed));
            assert_eq!(
                sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(*) FROM app_settings WHERE key='audit-sibling-effect'"
                )
                .fetch_one(&ctx.db)
                .await
                .unwrap(),
                0
            );
        }
    }

    struct WithdrawingSource<'a> {
        inner: &'a dyn ToolSource,
        available: bool,
    }
    impl ToolSource for WithdrawingSource<'_> {
        fn personal_call_audit_is_deferred(&self) -> bool {
            self.inner.personal_call_audit_is_deferred()
        }
        fn get(&self, id: &str) -> Option<Arc<dyn Tool>> {
            self.available.then(|| self.inner.get(id)).flatten()
        }
        fn defs_for(&self, ids: &[String]) -> Vec<ToolDef> {
            self.inner.defs_for(ids)
        }
        fn ids(&self) -> Vec<String> {
            self.inner.ids()
        }
    }

    #[tokio::test]
    async fn disappeared_live_source_records_one_denied_call_and_preserves_resume_and_stateless_audit()
     {
        let (tool, ctx) = fixture().await;
        let value = spec("always_allow");
        let spec = AgentSpec::from_value(&value).unwrap();
        let persona = crate::persona::ChatPersona::new("Harald", Arc::new(ToolRegistry::new()))
            .with_tool_access(&spec);
        let registry = tool.state.tools();
        let live = persona.source(registry.as_ref(), Some(&tool.state));
        let stateless = persona.source(registry.as_ref(), None);
        assert!(live.personal_call_audit_is_deferred());
        assert!(!stateless.personal_call_audit_is_deferred());
        let source = WithdrawingSource {
            inner: &live,
            available: false,
        };
        let call = crate::server::tools::runner::ToolCallRef {
            id: "disappeared".into(),
            name: "company_echo".into(),
            arguments_raw: "{}".into(),
        };
        let results = crate::server::tools::runner::execute_tool_calls(
            &source,
            &ctx,
            std::slice::from_ref(&call),
            &crate::server::tools::injection::InjectionScan::default(),
        )
        .await;
        assert!(results[0].failed);
        let events = aiplane_agents::db::agent_audit::for_principal(&ctx.db, "owner")
            .await
            .unwrap();
        let calls: Vec<_> = events
            .iter()
            .filter(|event| event.kind == "tool_call")
            .collect();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].detail["decision"], "denied");
        assert!(calls[0].detail.get("identity").is_none());
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM agent_audit WHERE principal_id='owner' AND kind='tool_result'"
            )
            .fetch_one(&ctx.db)
            .await
            .unwrap(),
            1
        );
        let resumed = ToolContext {
            pending_execution_facts: Some(crate::server::tools::PendingExecutionFacts {
                execution_facts: None,
            }),
            ..ctx.clone()
        };
        crate::server::tools::runner::execute_tool_calls(
            &source,
            &resumed,
            std::slice::from_ref(&call),
            &crate::server::tools::injection::InjectionScan::default(),
        )
        .await;
        let missing_stateless = WithdrawingSource {
            inner: &stateless,
            available: false,
        };
        assert!(
            crate::agents::audit::record_personal_tool_call(
                &ctx,
                Some("stateless"),
                "company_echo",
                true,
                "granted",
                None
            )
            .await
        );
        let stateless_call = crate::server::tools::runner::ToolCallRef {
            id: "stateless".into(),
            ..call
        };
        crate::server::tools::runner::execute_tool_calls(
            &missing_stateless,
            &ctx,
            &[stateless_call],
            &crate::server::tools::injection::InjectionScan::default(),
        )
        .await;
        let events = aiplane_agents::db::agent_audit::for_principal(&ctx.db, "owner")
            .await
            .unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == "tool_call")
                .count(),
            2
        );
    }

    #[tokio::test]
    async fn live_personal_agent_wrapper_preserves_the_tools_phase() {
        let (mut tool, _) = fixture().await;
        tool.initial = Arc::new(crate::server::tools::tests::PhasedEcho);
        assert_eq!(tool.phase(), crate::server::tools::ToolPhase::ActsOnState);
        assert_eq!(
            tool.execution_facts(),
            Some(serde_json::json!({"resource":"fixture"}))
        );
    }
    use super::*;
    use crate::agent_run::PersonalAgentIdentity;
    use crate::server::tools::{ToolRegistry, echo::Echo};
    use crate::suspend::{Suspend, extract_suspend};
    use aiplane_agents::db::system_principals::NewPrincipal;
    use aiplane_core::server::db::users::User;
    use aiplane_core::server::rbac::{RbacConfig, Resolver, RoleConfig, RoleMapping};
    use serde_json::json;

    struct PrincipalEcho;

    impl Tool for PrincipalEcho {
        fn id(&self) -> &str {
            "company_echo"
        }

        fn egress(&self) -> bool {
            false
        }
        fn schema(&self) -> ToolDef {
            Echo.schema()
        }
        fn action_catalogue(&self) -> Option<&'static [crate::server::tools::ToolAction]> {
            Some(&[crate::server::tools::ToolAction {
                id: "read",
                description: "Read caller identity",
            }])
        }
        fn classify_actions<'a>(
            &'a self,
            _ctx: ToolContext,
            _args: Value,
        ) -> crate::server::tools::ToolActionFuture<'a> {
            Box::pin(async { Ok(Some(vec!["read".into()])) })
        }
        fn run<'a>(&'a self, ctx: ToolContext, _args: Value) -> ToolFuture<'a> {
            Box::pin(async move {
                let Principal::User { id, roles } = ctx.principal else {
                    panic!("user identity required")
                };
                Ok(json!({"user": id, "roles": roles}))
            })
        }
    }

    fn spec(permission: &str) -> Value {
        json!({"main": {"tools": ["company_echo"], "tool_resources": {
            "company_echo": {"permission": permission}
        }}})
    }

    async fn fixture() -> (LivePersonalAgentTool, ToolContext) {
        fixture_with_echo(false).await
    }

    async fn fixture_with_echo(echo: bool) -> (LivePersonalAgentTool, ToolContext) {
        let probe: Arc<dyn Tool> = if echo {
            Arc::new(crate::server::tools::echo::Echo)
        } else {
            Arc::new(PrincipalEcho)
        };
        fixture_with_probe(probe).await
    }

    async fn fixture_with_probe(probe: Arc<dyn Tool>) -> (LivePersonalAgentTool, ToolContext) {
        let db = aiplane_core::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        let now = jiff::Timestamp::now();
        users::upsert(
            &db,
            &User {
                id: "owner".into(),
                email: "owner@example.test".into(),
                name: None,
                roles: vec!["current-role".into()],
                created_at: now,
                updated_at: now,
                timezone: None,
                speech_voice: None,
            },
        )
        .await
        .unwrap();
        let (_, owned_agent) = agents::create_personal_agent(
            &db,
            &NewPrincipal {
                name: "Harald",
                display: "Harald",
                description: "",
            },
            &spec("always_allow").to_string(),
            "owner",
            4,
        )
        .await
        .unwrap();
        let state = Arc::new(AppState::new(
            Default::default(),
            db.clone(),
            aiplane_core::server::upstreams::UpstreamRegistry::new(&Default::default()).unwrap(),
            Arc::new(ToolRegistry::new().with_family_replaced("company_", vec![probe.clone()])),
            Arc::new(
                Resolver::build(
                    RbacConfig {
                        default_role: None,
                        mappings: vec![RoleMapping {
                            oidc_value: "current-role".into(),
                            role: "member".into(),
                        }],
                    },
                    vec![RoleConfig {
                        id: "member".into(),
                        admin: false,
                        tools: vec!["company_echo".into()],
                        skills: vec![],
                    }],
                )
                .unwrap(),
            ),
        ));
        let ctx = ToolContext {
            principal: Principal::User {
                id: "owner".into(),
                roles: vec!["old-role".into()],
            },
            personal_agent: Some(PersonalAgentIdentity {
                owner_user_id: "owner".into(),
                agent_id: owned_agent.unwrap().principal.id,
            }),
            granted_tools: Some(Arc::new(["company_echo".into()].into_iter().collect())),
            suspend: Suspend::Available,
            ..ToolContext::for_test(db)
        };
        (
            LivePersonalAgentTool {
                state,
                initial: probe,
                initial_bindings: Default::default(),
            },
            ctx,
        )
    }

    async fn save(tool: &LivePersonalAgentTool, ctx: &ToolContext, spec: Value) {
        agents::update_draft(
            &tool.state.db,
            &ctx.personal_agent.as_ref().unwrap().agent_id,
            &spec.to_string(),
            "owner",
            Default::default(),
        )
        .await
        .unwrap()
        .unwrap();
    }

    #[tokio::test]
    async fn disabled_personal_agent_cannot_execute_a_cached_builtin_tool() {
        let (tool, ctx) = fixture().await;
        assert!(tool.run(ctx.clone(), json!({})).await.is_ok());
        sqlx::query("UPDATE system_principals SET disabled_at=? WHERE id=?")
            .bind(jiff::Timestamp::now().to_string())
            .bind(&ctx.personal_agent.as_ref().unwrap().agent_id)
            .execute(&tool.state.db)
            .await
            .unwrap();
        assert!(tool.run(ctx, json!({})).await.is_err());
    }

    #[tokio::test]
    async fn live_agent_dispatch_refreshes_identity_and_permission_for_each_call() {
        let (tool, ctx) = fixture().await;
        let result = tool.run(ctx.clone(), json!({})).await.unwrap();
        assert_eq!(result, json!({"user": "owner", "roles": ["current-role"]}));
        save(&tool, &ctx, spec("always_ask")).await;
        assert!(extract_suspend(&tool.run(ctx.clone(), json!({})).await.unwrap()).is_some());
        save(&tool, &ctx, spec("always_allow")).await;
        assert!(extract_suspend(&tool.run(ctx, json!({})).await.unwrap()).is_none());
    }

    #[tokio::test]
    async fn live_agent_dispatch_refuses_removed_disabled_inactive_and_other_owner() {
        let (tool, ctx) = fixture().await;
        let mut other = ctx.clone();
        other.principal = ToolContext::test_user("other");
        assert!(tool.run(other, json!({})).await.is_err());
        save(&tool, &ctx, json!({"main": {"tools": []}})).await;
        assert!(tool.run(ctx.clone(), json!({})).await.is_err());
        save(&tool, &ctx, spec("always_allow")).await;
        user_tool_prefs::set(&tool.state.db, "owner", "company_echo", false)
            .await
            .unwrap();
        assert!(tool.run(ctx.clone(), json!({})).await.is_err());
        user_tool_prefs::set(&tool.state.db, "owner", "company_echo", true)
            .await
            .unwrap();
        agents::set_personal_agent_active(
            &tool.state.db,
            &ctx.personal_agent.as_ref().unwrap().agent_id,
            "owner",
            false,
            4,
        )
        .await
        .unwrap();
        assert!(tool.run(ctx, json!({})).await.is_err());
    }

    #[tokio::test]
    async fn live_agent_actions_reload_current_rule_and_preserve_grant_ceiling() {
        let (tool, ctx) = fixture().await;
        for (rule, expected) in [("allow", 0), ("ask", 1), ("deny", 2)] {
            save(&tool,&ctx,json!({"main":{"tools":["company_echo"],"tool_resources":{"company_echo":{"actions":{"read":rule}}}}})).await;
            let result = tool.run(ctx.clone(), json!({})).await;
            match expected {
                0 => assert_eq!(result.unwrap()["user"], json!("owner")),
                1 => assert_eq!(
                    extract_suspend(&result.unwrap()).unwrap().actions,
                    Some(vec!["read".into()])
                ),
                _ => assert!(result.is_err()),
            }
        }
        save(&tool,&ctx,json!({"main":{"tools":["company_echo"],"tool_resources":{"company_echo":{"actions":{"read":"allow"}}}}})).await;
        user_tool_prefs::set(&tool.state.db, "owner", "company_echo", false)
            .await
            .unwrap();
        assert!(tool.run(ctx, json!({})).await.is_err());
    }

    #[tokio::test]
    async fn live_agent_dispatch_does_not_gain_a_midturn_grant() {
        let (tool, mut ctx) = fixture().await;
        ctx.granted_tools = Some(Arc::new(Default::default()));
        assert!(tool.run(ctx, json!({})).await.is_err());
    }

    #[tokio::test]
    async fn live_agent_dispatch_refuses_an_owner_role_revoked_midturn() {
        let (tool, ctx) = fixture().await;
        assert!(tool.run(ctx.clone(), json!({})).await.is_ok());
        let mut user = users::find_by_id(&tool.state.db, "owner")
            .await
            .unwrap()
            .unwrap();
        user.roles.clear();
        users::upsert(&tool.state.db, &user).await.unwrap();
        assert!(tool.run(ctx, json!({})).await.is_err());
    }
    #[tokio::test]
    async fn personal_agent_live_tool_rechecks_background_trigger_ceiling_and_binding() {
        let (tool, mut ctx) = fixture().await;
        let identity = ctx.personal_agent.clone().unwrap();
        let session = session_core::db::create_session(&tool.state.db, "owner")
            .await
            .unwrap();
        aiplane_agents::db::agent_sessions::link(
            &tool.state.db,
            "owner",
            &identity.agent_id,
            &session.id,
        )
        .await
        .unwrap();
        session_core::db::create_assistant_turn_in_progress(
            &tool.state.db,
            &session.id,
            "live-turn",
            "model",
        )
        .await
        .unwrap();
        let now = jiff::Timestamp::now().to_string();
        sqlx::query("INSERT INTO scheduled_actions(id,user_id,personal_agent_id,name,prompt,model,cron,timezone,tools_enabled,created_at,updated_at) VALUES('action','owner',?,'job','prompt','model','* * * * *','UTC',1,?,?)").bind(&identity.agent_id).bind(&now).bind(&now).execute(&tool.state.db).await.unwrap();
        sqlx::query("INSERT INTO scheduled_runs(id,action_id,fired_at,session_id,assistant_turn_id,tools_enabled,configuration_generation,agent_version,created_at) SELECT 'run',id,?,?,'live-turn',tools_enabled,configuration_generation,agent_version,? FROM scheduled_actions WHERE id='action'").bind(&now).bind(&session.id).bind(&now).execute(&tool.state.db).await.unwrap();
        ctx.assistant_turn_id = Some("live-turn".into());
        ctx.session_id = Some(session.id);
        assert_eq!(
            tool.run(ctx.clone(), json!({})).await.unwrap()["user"],
            "owner"
        );
        sqlx::query("UPDATE scheduled_actions SET tools_enabled=0")
            .execute(&tool.state.db)
            .await
            .unwrap();
        assert!(tool.run(ctx.clone(), json!({})).await.is_err());
        sqlx::query("UPDATE scheduled_actions SET tools_enabled=1,personal_agent_id=NULL")
            .execute(&tool.state.db)
            .await
            .unwrap();
        assert!(tool.run(ctx, json!({})).await.is_err());
    }
}
