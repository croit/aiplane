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

pub(super) struct LivePersonalDotTool {
    pub state: Arc<AppState>,
    pub initial: Arc<dyn Tool>,
}

impl Tool for LivePersonalDotTool {
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
        self.initial.classify_actions(ctx, args)
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

    fn changes_state(&self) -> bool {
        self.initial.changes_state()
    }

    fn run<'a>(&'a self, mut ctx: ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            if let Some(execution) = ctx.personal_execution.as_ref() {
                let root_turn = crate::agents::router::local::validate_execution(execution).await?;
                if execution.meter().remaining().is_none()
                    || execution.unknown_token_usage(execution.local_budget())
                {
                    return Err(self.unavailable());
                }
                if crate::server::background::for_turn(
                    &self.state.db,
                    &execution.identity().owner_user_id,
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
                .personal_dot
                .as_ref()
                .ok_or_else(|| self.unavailable())?;
            if !matches!(&ctx.principal, Principal::User { id, .. } if id == &identity.owner_user_id)
            {
                return Err(self.unavailable());
            }
            if let Some(turn_id) = ctx.assistant_turn_id.as_deref()
                && crate::server::background::for_turn(
                    &self.state.db,
                    &identity.owner_user_id,
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
            let dot = agents::get_personal_dot(&self.state.db, &owner.id, &identity.dot_id)
                .await
                .map_err(Self::load_error)?
                .filter(|dot| dot.active)
                .ok_or_else(|| self.unavailable())?;
            let value = serde_json::from_str(&dot.agent.draft_spec)
                .map_err(|error| ToolError::Failed(format!("Could not read the personal Dot's tool rules: {error}. Correct its access settings before retrying.")))?;
            let spec = AgentSpec::from_value(&value)
                .map_err(|error| ToolError::Failed(format!("Could not read the personal Dot's tool rules: {error}. Correct its access settings before retrying.")))?;
            if !spec.main.tools.iter().any(|id| id == self.id()) {
                return Err(self.unavailable());
            }
            let principal = Principal::User {
                id: owner.id.clone(),
                roles: owner.roles,
            };
            let layer = self.state.mcp_layer_for(&principal, AskContext::Chat).await;
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
                super::personal_dot::current_pool_access(&self.state, &principal, &ctx.pool_access);
            ctx.principal = principal;
            ctx.pool_access = access;
            ctx.granted_tools = Some(Arc::new(granted.into_iter().collect()));
            Permissions::from_spec(&spec)
                .gate_personal_dot(tool)
                .run(ctx, args)
                .await
        })
    }
}

impl LivePersonalDotTool {
    fn unavailable(&self) -> ToolError {
        ToolError::Failed(format!(
            "`{}` is unavailable to this personal Dot under its current access settings. Review the Dot's active state, selected tools and the owner's permissions before retrying.",
            self.id()
        ))
    }

    fn load_error(error: aiplane_core::server::db::DbError) -> ToolError {
        ToolError::Failed(format!(
            "Could not load the personal Dot's current access settings: {error}. Retry after the database is available."
        ))
    }
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn live_personal_dot_wrapper_preserves_the_tools_phase() {
        let (mut tool, _) = fixture().await;
        tool.initial = Arc::new(crate::server::tools::tests::PhasedEcho);
        assert_eq!(tool.phase(), crate::server::tools::ToolPhase::ActsOnState);
    }
    use super::*;
    use crate::agent_run::PersonalDotIdentity;
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

    async fn fixture() -> (LivePersonalDotTool, ToolContext) {
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
        let (_, dot) = agents::create_personal_dot(
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
            Arc::new(ToolRegistry::new().with(PrincipalEcho)),
            Arc::new(
                Resolver::build(
                    RbacConfig {
                        default_role: None,
                        mappings: vec![RoleMapping {
                            oidc_claim: "roles".into(),
                            oidc_value: "current-role".into(),
                            role: "member".into(),
                        }],
                    },
                    vec![RoleConfig {
                        id: "member".into(),
                        admin: false,
                        models: vec![],
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
            personal_dot: Some(PersonalDotIdentity {
                owner_user_id: "owner".into(),
                dot_id: dot.unwrap().principal.id,
            }),
            granted_tools: Some(Arc::new(["company_echo".into()].into_iter().collect())),
            suspend: Suspend::Available,
            ..ToolContext::for_test(db)
        };
        (
            LivePersonalDotTool {
                state,
                initial: Arc::new(PrincipalEcho),
            },
            ctx,
        )
    }

    async fn save(tool: &LivePersonalDotTool, ctx: &ToolContext, spec: Value) {
        agents::update_draft(
            &tool.state.db,
            &ctx.personal_dot.as_ref().unwrap().dot_id,
            &spec.to_string(),
            "owner",
            Default::default(),
        )
        .await
        .unwrap()
        .unwrap();
    }

    #[tokio::test]
    async fn live_dot_dispatch_refreshes_identity_and_permission_for_each_call() {
        let (tool, ctx) = fixture().await;
        let result = tool.run(ctx.clone(), json!({})).await.unwrap();
        assert_eq!(result, json!({"user": "owner", "roles": ["current-role"]}));
        save(&tool, &ctx, spec("always_ask")).await;
        assert!(extract_suspend(&tool.run(ctx.clone(), json!({})).await.unwrap()).is_some());
        save(&tool, &ctx, spec("always_allow")).await;
        assert!(extract_suspend(&tool.run(ctx, json!({})).await.unwrap()).is_none());
    }

    #[tokio::test]
    async fn live_dot_dispatch_refuses_removed_disabled_inactive_and_other_owner() {
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
        agents::set_personal_dot_active(
            &tool.state.db,
            &ctx.personal_dot.as_ref().unwrap().dot_id,
            "owner",
            false,
            4,
        )
        .await
        .unwrap();
        assert!(tool.run(ctx, json!({})).await.is_err());
    }

    #[tokio::test]
    async fn live_dot_actions_reload_current_rule_and_preserve_grant_ceiling() {
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
    async fn live_dot_dispatch_does_not_gain_a_midturn_grant() {
        let (tool, mut ctx) = fixture().await;
        ctx.granted_tools = Some(Arc::new(Default::default()));
        assert!(tool.run(ctx, json!({})).await.is_err());
    }

    #[tokio::test]
    async fn live_dot_dispatch_refuses_an_owner_role_revoked_midturn() {
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
    async fn personal_dot_live_tool_rechecks_background_trigger_ceiling_and_binding() {
        let (tool, mut ctx) = fixture().await;
        let identity = ctx.personal_dot.clone().unwrap();
        let session = session_core::db::create_session(&tool.state.db, "owner")
            .await
            .unwrap();
        aiplane_agents::db::dot_sessions::link(
            &tool.state.db,
            "owner",
            &identity.dot_id,
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
        sqlx::query("INSERT INTO scheduled_actions(id,user_id,dot_id,name,prompt,model,cron,timezone,tools_enabled,created_at,updated_at) VALUES('action','owner',?,'job','prompt','model','* * * * *','UTC',1,?,?)").bind(&identity.dot_id).bind(&now).bind(&now).execute(&tool.state.db).await.unwrap();
        sqlx::query("INSERT INTO scheduled_runs(id,action_id,fired_at,session_id,assistant_turn_id,tools_enabled,created_at) VALUES('run','action',?,?,'live-turn',1,?)").bind(&now).bind(&session.id).bind(&now).execute(&tool.state.db).await.unwrap();
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
        sqlx::query("UPDATE scheduled_actions SET tools_enabled=1,dot_id=NULL")
            .execute(&tool.state.db)
            .await
            .unwrap();
        assert!(tool.run(ctx, json!({})).await.is_err());
    }
}
