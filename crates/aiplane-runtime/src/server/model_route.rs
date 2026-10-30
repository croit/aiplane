// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Where a request for a model name goes, decided once for every caller
//! that names one: a chat turn, and an agent's own model calls (the topic
//! guard, the route classifier, the evaluation judge, the prompt
//! assistant). An automatic route's alias is resolved by its selector
//! ([`AutomaticRouter::select`]); any other name routes as it is.

use aiplane_core::server::automatic_routing::{
    AutomaticRouteAffinity, AutomaticRouteDecision, AutomaticRoutingError, SelectorAdmissionError,
    SelectorGate,
};
use aiplane_core::server::principal::{GrantKind, Principal};
use aiplane_core::server::upstreams::PoolAccess;
use serde_json::Value;
use std::sync::Arc;

use crate::agent_run::AgentRun;
use crate::agents::profile::AgentRunError;
use crate::server::AppState;

#[derive(Clone)]
pub(crate) struct ModelAuthority {
    run: Arc<AgentRun>,
    purpose: ModelPurpose,
}

#[derive(Clone)]
enum ModelPurpose {
    Live,
    Compaction { requested_model: String },
}

impl ModelAuthority {
    pub(crate) fn live(run: Arc<AgentRun>) -> Self {
        Self {
            run,
            purpose: ModelPurpose::Live,
        }
    }

    pub(super) fn compaction(run: Arc<AgentRun>, requested_model: String) -> Self {
        Self {
            run,
            purpose: ModelPurpose::Compaction { requested_model },
        }
    }

    pub(crate) fn original_model<'a>(&'a self, fallback: &'a str) -> &'a str {
        match &self.purpose {
            ModelPurpose::Live => fallback,
            ModelPurpose::Compaction { requested_model } => requested_model,
        }
    }

    pub(crate) fn check_turn(
        &self,
        session: Option<&str>,
        assistant: Option<&str>,
    ) -> Result<(), String> {
        if let Some(context) = self.run.organization()
            && (session != Some(context.session_id.as_str())
                || assistant != Some(context.assistant_turn_id.as_str()))
        {
            return Err(AgentRunError::OrganizationExecutionUnavailable(
                self.run.system_principal().id.clone(),
            )
            .to_string());
        }
        Ok(())
    }

    pub(crate) async fn principal(
        &self,
        state: &AppState,
    ) -> Result<aiplane_core::server::principal::SystemPrincipal, String> {
        match self.purpose {
            ModelPurpose::Live => {
                crate::agents::profile::validate_organization_turn(state, &self.run).await
            }
            ModelPurpose::Compaction { .. } => {
                crate::agents::profile::validate_organization_compaction(state, &self.run).await
            }
        }
        .map_err(|error| error.to_string())
    }

    pub(super) fn is_organization_compaction(&self) -> bool {
        self.run.organization().is_some() && matches!(self.purpose, ModelPurpose::Compaction { .. })
    }

    pub(super) async fn compaction_access_in(
        &self,
        state: &AppState,
        tx: &mut aiplane_core::server::db::WriteTx,
        access: &PoolAccess,
        model: &str,
    ) -> Result<(), String> {
        if !self.is_organization_compaction() {
            return Err(AgentRunError::OrganizationExecutionUnavailable(
                self.run.system_principal().id.clone(),
            )
            .to_string());
        }
        let current = crate::agents::profile::validate_organization_compaction_in(tx, &self.run)
            .await
            .map_err(|error| error.to_string())?;
        let access = intersect_model_access(&current, self, access, model, None)?;
        state
            .upstreams
            .resolve_model_for(
                model,
                aiplane_core::server::upstreams::PoolKind::Chat,
                &access,
            )
            .ok_or_else(|| {
                crate::agents::profile::AgentRunError::OrganizationExecutionUnavailable(
                    self.run.system_principal().id.clone(),
                )
                .to_string()
            })?;
        Ok(())
    }
}

pub(crate) async fn live_model_access(
    state: &AppState,
    principal: &Principal,
    authority: Option<&ModelAuthority>,
    captured: &PoolAccess,
    named_model: &str,
    decision: Option<&AutomaticRouteDecision>,
) -> Result<PoolAccess, String> {
    let Some(current) = validate_model_authority(state, principal, authority).await? else {
        return Ok(captured.clone());
    };
    let authority = authority.ok_or_else(|| {
        AgentRunError::OrganizationExecutionUnavailable(current.id.clone()).to_string()
    })?;
    intersect_model_access(&current, authority, captured, named_model, decision)
}

pub(crate) fn validate_acquired_access(
    state: &AppState,
    access: &PoolAccess,
    acquired: &aiplane_core::server::upstreams::registry::Acquired,
) -> Result<(), String> {
    state
        .upstreams
        .pools()
        .iter()
        .any(|pool| {
            access.reaches(pool, acquired.resolved_model())
                && pool
                    .backends
                    .iter()
                    .any(|backend| std::ptr::eq(backend.as_ref(), acquired.backend()))
        })
        .then_some(())
        .ok_or_else(|| {
            aiplane_core::server::upstreams::RouteError::ModelNotAllowed(
                acquired.resolved_model().to_owned(),
            )
            .to_string()
        })
}

pub(crate) async fn validate_model_authority(
    state: &AppState,
    principal: &Principal,
    authority: Option<&ModelAuthority>,
) -> Result<Option<aiplane_core::server::principal::SystemPrincipal>, String> {
    let Principal::System(initial) = principal else {
        return Ok(None);
    };
    let organization = aiplane_agents::db::agents::agent_owner(&state.db, &initial.id)
        .await
        .map_err(|error| error.to_string())?;
    let is_organization = matches!(
        organization,
        Some(aiplane_agents::db::agents::AgentOwner::Organization(_))
    );
    if !is_organization && authority.is_none_or(|authority| authority.run.organization().is_none())
    {
        return Ok(None);
    }
    let authority = authority.ok_or_else(|| {
        AgentRunError::OrganizationExecutionUnavailable(initial.id.clone()).to_string()
    })?;
    if authority.run.system_principal().id != initial.id {
        return Err(
            AgentRunError::OrganizationExecutionUnavailable(initial.id.clone()).to_string(),
        );
    }
    authority.principal(state).await.map(Some)
}

fn intersect_model_access(
    current: &aiplane_core::server::principal::SystemPrincipal,
    authority: &ModelAuthority,
    captured: &PoolAccess,
    named_model: &str,
    decision: Option<&AutomaticRouteDecision>,
) -> Result<PoolAccess, String> {
    let named_model = authority.original_model(named_model);
    if !current.grants.has(GrantKind::Model, named_model) || !captured.allows_model(named_model) {
        return Err(
            AgentRunError::OrganizationExecutionUnavailable(current.id.clone()).to_string(),
        );
    }
    let captured_grants = captured.granted_models.as_ref().ok_or_else(|| {
        AgentRunError::OrganizationExecutionUnavailable(current.id.clone()).to_string()
    })?;
    let mut access =
        PoolAccess::for_system_models(current, captured_grants.keys().map(String::as_str));
    let mut grants = (**access.granted_models.as_ref().expect("system model grants")).clone();
    for (model, pools) in &mut grants {
        if let Some(Some(initial_pools)) = captured_grants.get(model) {
            *pools = Some(match pools.take() {
                Some(current_pools) => current_pools.intersection(initial_pools).cloned().collect(),
                None => initial_pools.clone(),
            });
        }
    }
    access.granted_models = Some(Arc::new(grants));
    access.allowed_models = captured.allowed_models.clone();
    access.expand_aliases = captured.expand_aliases;
    if let Some(decision) = decision {
        if decision.alias != named_model {
            return Err(
                AgentRunError::OrganizationExecutionUnavailable(current.id.clone()).to_string(),
            );
        }
        access = access.for_route_targets(named_model, [decision.effective_target.as_str()]);
    } else if matches!(authority.purpose, ModelPurpose::Compaction { .. }) {
        access = access.for_route_targets(named_model, captured_grants.keys().map(String::as_str));
    }
    Ok(access)
}

struct LiveSelectorGate<'a> {
    state: &'a AppState,
    principal: &'a Principal,
    authority: Option<&'a ModelAuthority>,
    captured: &'a PoolAccess,
    model: &'a str,
}

impl SelectorGate for LiveSelectorGate<'_> {
    async fn admit_acquired(
        &self,
        acquired: &aiplane_core::server::upstreams::registry::Acquired,
    ) -> Result<(), SelectorAdmissionError> {
        self.admit(acquired.resolved_model()).await?;
        let mut access = live_model_access(
            self.state,
            self.principal,
            self.authority,
            self.captured,
            self.model,
            None,
        )
        .await
        .map_err(SelectorAdmissionError::Unavailable)?;
        if self
            .authority
            .is_some_and(|authority| authority.run.organization().is_some())
        {
            access = access.for_route_targets(self.model, [acquired.resolved_model()]);
        }
        validate_acquired_access(self.state, &access, acquired)
            .map_err(SelectorAdmissionError::Unavailable)
    }

    fn priority(&self) -> Option<aiplane_core::server::priority::Priority> {
        self.state.config().gateway.priority_enabled.then(|| {
            self.state.rbac.priority_for_roles(
                &self.captured.role_ids,
                self.state.config().gateway.default_priority,
            )
        })
    }

    fn wait_budget(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.state.config().gateway.upstream_wait_secs)
    }

    async fn admit(&self, selector_model: &str) -> Result<(), SelectorAdmissionError> {
        let mut access = live_model_access(
            self.state,
            self.principal,
            self.authority,
            self.captured,
            self.model,
            None,
        )
        .await
        .map_err(SelectorAdmissionError::Unavailable)?;
        if self
            .authority
            .is_some_and(|authority| authority.run.organization().is_some())
        {
            access = access.for_route_targets(self.model, [selector_model]);
            self.state
                .upstreams
                .resolve_route_access(
                    selector_model,
                    aiplane_core::server::upstreams::PoolKind::SystemOne,
                    &access,
                )
                .map_err(|error| SelectorAdmissionError::Unavailable(error.to_string()))?;
        }
        Ok(())
    }
}

/// The model to route to, and the access to route it under.
#[derive(Debug, Clone)]
pub struct RouteTarget {
    pub model: String,
    /// The caller's access, widened by an automatic route's chosen target
    /// ([`PoolAccess::for_route_targets`]).
    pub access: PoolAccess,
    pub decision: Option<AutomaticRouteDecision>,
}

/// Resolve `model` for a request whose messages and tools `routing_state`
/// holds (`{"messages": [...], "tools": [...]}`), under `access`.
pub(crate) async fn route_target(
    state: &AppState,
    model: &str,
    routing_state: &Value,
    access: &PoolAccess,
    affinity: Option<AutomaticRouteAffinity<'_>>,
    principal: &Principal,
    authority: Option<&ModelAuthority>,
) -> Result<RouteTarget, AutomaticRoutingError> {
    let access = live_model_access(state, principal, authority, access, model, None)
        .await
        .map_err(AutomaticRoutingError::SelectorUnavailable)?;
    let gate = LiveSelectorGate {
        state,
        principal,
        authority,
        captured: &access,
        model,
    };
    let decision = state
        .automatic_router
        .select(model, routing_state, &access, affinity, &gate)
        .await?;
    Ok(match decision {
        Some(decision) => RouteTarget {
            model: decision.effective_target.clone(),
            access: access.for_route_targets(&decision.alias, [decision.effective_target.as_str()]),
            decision: Some(decision),
        },
        None => RouteTarget {
            model: model.to_string(),
            access: access.clone(),
            decision: None,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_run::Actor;
    use crate::server::headless::tests::{organization_draft_fixture, state_for};
    use aiplane_agents::db::system_principals;
    use aiplane_core::server::principal::GrantKind;
    use session_core::db::{self as chat, TurnStatus};
    use wiremock::MockServer;

    #[tokio::test]
    async fn acquired_model_access_requires_the_actual_backend_pool() {
        use aiplane_core::server::upstreams::PoolKind;
        use std::collections::{HashMap, HashSet};
        let (state, _, captured, provider) = organization_alias_fixture().await;
        let access = captured.for_route_targets("automatic", ["model-b"]);
        let acquired = state
            .upstreams
            .route_access("model-b", PoolKind::Chat, &access)
            .unwrap();
        assert!(validate_acquired_access(&state, &access, &acquired).is_ok());
        let restricted = PoolAccess {
            granted_models: Some(Arc::new(HashMap::from([(
                "model-b".into(),
                Some(HashSet::from(["other".into()])),
            )]))),
            ..access
        };
        assert!(
            state
                .upstreams
                .resolve_route_access("model-b", PoolKind::Chat, &restricted)
                .is_ok()
        );
        assert!(validate_acquired_access(&state, &restricted, &acquired).is_err());
        assert!(provider.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn selector_acquisition_rechecks_grants_revoked_while_queued() {
        use aiplane_core::server::priority::Priority;
        use aiplane_core::server::upstreams::{PoolKind, route_or_wait};
        use std::future::Future;
        use std::task::Poll;
        use std::time::Duration;
        let (state, run, captured, provider) = organization_alias_fixture().await;
        let principal = run.principal();
        let authority = ModelAuthority::live(run.clone());
        let gate = LiveSelectorGate {
            state: &state,
            principal: &principal,
            authority: Some(&authority),
            captured: &captured,
            model: "automatic",
        };
        gate.admit("selector-model").await.unwrap();
        let selector_access = captured.for_route_targets("automatic", ["selector-model"]);
        let mut occupied = Vec::new();
        for _ in 0..8 {
            occupied.push(
                state
                    .upstreams
                    .route_access("selector-model", PoolKind::SystemOne, &selector_access)
                    .unwrap(),
            );
        }
        let queued = route_or_wait(
            &state.upstreams,
            "selector-model",
            PoolKind::SystemOne,
            &selector_access,
            Duration::from_secs(2),
            None,
            Some(Priority::Medium),
        );
        tokio::pin!(queued);
        std::future::poll_fn(|cx| {
            assert!(queued.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        system_principals::remove_grant(
            &state.db,
            &run.system_principal().id,
            GrantKind::Model,
            "automatic",
            "creator",
        )
        .await
        .unwrap();
        drop(occupied);
        let acquired = tokio::time::timeout(Duration::from_secs(2), queued)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            gate.admit_acquired(&acquired).await,
            Err(SelectorAdmissionError::Unavailable(_))
        ));
        assert!(provider.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn organization_model_admission_rechecks_grants_without_widening_captured_models() {
        let upstream = MockServer::start().await;
        let state = state_for(&upstream.uri()).await;
        let (profile, params) = organization_draft_fixture(&state).await;
        let Actor::Agent(run) = &params.actor else {
            panic!("agent expected")
        };
        let principal = run.principal();
        let authority = ModelAuthority::live(run.clone());
        let captured = PoolAccess::for_system_models(&profile.principal, [&*params.model]);
        let before = live_model_access(
            &state,
            &principal,
            Some(&authority),
            &captured,
            &params.model,
            None,
        )
        .await
        .unwrap();
        assert!(before.allows_model(&params.model));
        system_principals::add_grant(
            &state.db,
            &profile.principal.id,
            GrantKind::Model,
            "new-model",
            "creator",
        )
        .await
        .unwrap();
        let after = live_model_access(
            &state,
            &principal,
            Some(&authority),
            &captured,
            &params.model,
            None,
        )
        .await
        .unwrap();
        assert!(!after.allows_model("new-model"));
        system_principals::remove_grant(
            &state.db,
            &profile.principal.id,
            GrantKind::Model,
            &params.model,
            "creator",
        )
        .await
        .unwrap();
        assert!(
            live_model_access(
                &state,
                &principal,
                Some(&authority),
                &captured,
                &params.model,
                None
            )
            .await
            .is_err()
        );
        assert!(upstream.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn organization_model_admission_requires_actual_context_and_compaction_completion() {
        let upstream = MockServer::start().await;
        let state = state_for(&upstream.uri()).await;
        let (profile, params) = organization_draft_fixture(&state).await;
        let Actor::Agent(run) = &params.actor else {
            panic!("agent expected")
        };
        let principal = run.principal();
        let captured = PoolAccess::for_system_models(&profile.principal, [&*params.model]);
        assert!(
            live_model_access(&state, &principal, None, &captured, &params.model, None)
                .await
                .is_err()
        );
        let live = ModelAuthority::live(run.clone());
        let compact = ModelAuthority::compaction(run.clone(), params.model.clone());
        assert!(
            live_model_access(
                &state,
                &principal,
                Some(&compact),
                &captured,
                &params.model,
                None
            )
            .await
            .is_err()
        );
        chat::finalize_turn(
            &state.db,
            &params.assistant_turn_id,
            TurnStatus::Completed,
            None,
        )
        .await
        .unwrap();
        assert!(
            live_model_access(
                &state,
                &principal,
                Some(&live),
                &captured,
                &params.model,
                None
            )
            .await
            .is_err()
        );
        assert!(
            live_model_access(
                &state,
                &principal,
                Some(&compact),
                &captured,
                &params.model,
                None
            )
            .await
            .unwrap()
            .allows_model(&params.model)
        );
        sqlx::query("UPDATE agent_owners SET active=0 WHERE principal_id=?")
            .bind(&profile.principal.id)
            .execute(&state.db)
            .await
            .unwrap();
        assert!(
            live_model_access(
                &state,
                &principal,
                Some(&compact),
                &captured,
                &params.model,
                None
            )
            .await
            .is_err()
        );
    }
    async fn organization_alias_fixture() -> (
        Arc<crate::rama_server::state::RamaState>,
        Arc<crate::agent_run::AgentRun>,
        PoolAccess,
        wiremock::MockServer,
    ) {
        use crate::agents::profile::{Role, RunProfile, SpecSource};
        use aiplane_core::server::db::automatic_routes::{
            self, AutomaticRoute, AutomaticRouteCandidate,
        };
        use aiplane_core::server::upstreams::{
            BackendConfig, PoolKind, UpstreamPoolConfig, UpstreamRegistry,
        };
        use wiremock::{
            Mock, MockServer, ResponseTemplate,
            matchers::{method, path},
        };
        let provider = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/systemone"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "answers":{"route":{"choice":"expert","confidence":0.95}}
            })))
            .mount(&provider)
            .await;
        let original = state_for(&provider.uri()).await;
        let (original_profile, params) = organization_draft_fixture(&original).await;
        let pools = [
            ("pool", PoolKind::Chat, vec!["model-a", "model-b"]),
            ("selector", PoolKind::SystemOne, vec!["selector-model"]),
            ("other", PoolKind::Chat, vec!["model-a", "model-b"]),
        ]
        .into_iter()
        .map(|(name, kind, models)| {
            (
                name.to_owned(),
                UpstreamPoolConfig {
                    kind,
                    strategy: Default::default(),
                    fallback_offline: None,
                    models: Vec::new(),
                    level_rank: 0,
                    enforce_limits: true,
                    voices: Default::default(),
                    offer_voices: Vec::new(),
                    allowed_groups: Vec::new(),
                    backend: vec![BackendConfig {
                        priority: Default::default(),
                        name: name.into(),
                        base_url: provider.uri(),
                        models: models.into_iter().map(str::to_owned).collect(),
                        api_key_env: None,
                        api_key: None,
                        weight: 1,
                        max_inflight: 8,
                        health_path: "/models".into(),
                        alias: Default::default(),
                        enabled: true,
                        extra_headers: Default::default(),
                        supports_edit: false,
                    }],
                },
            )
        })
        .collect();
        let app = crate::server::AppState::new(
            (*original.config()).clone(),
            original.db.clone(),
            UpstreamRegistry::new(&pools).unwrap(),
            original.tools(),
            original.rbac.clone(),
        );
        let state = Arc::new(crate::rama_server::state::RamaState::new(
            app,
            original.sessions.clone(),
            aiplane_core::server::usage::UsageHandle::disabled(),
        ));
        automatic_routes::upsert(
            &state.db,
            &AutomaticRoute {
                alias: "automatic".into(),
                description: String::new(),
                selector_model: "selector-model".into(),
                objective: "balanced".into(),
                instructions: "Choose a configured candidate.".into(),
                minimum_confidence: 0.7,
                selector_timeout_ms: 1000,
                fallback_target: "model-a".into(),
                session_affinity: false,
                session_ttl_seconds: 3600,
                rollout: "active".into(),
                version: 0,
                candidates: vec![
                    AutomaticRouteCandidate {
                        key: "fast".into(),
                        target: "model-a".into(),
                        description: "Configured first model".into(),
                    },
                    AutomaticRouteCandidate {
                        key: "expert".into(),
                        target: "model-b".into(),
                        description: "Configured second model".into(),
                    },
                ],
            },
        )
        .await
        .unwrap();
        system_principals::add_scoped_grant(
            &state.db,
            &original_profile.principal.id,
            GrantKind::Model,
            "automatic",
            Some(&["pool".into(), "selector".into()]),
            "creator",
        )
        .await
        .unwrap();
        let profile = RunProfile::load_from(
            &state,
            &original_profile.principal.id,
            SpecSource::Draft(
                serde_json::json!({"main":{"model":"automatic","tools":["company_echo"]}}),
            ),
            Role::Main,
            &Default::default(),
        )
        .await
        .unwrap();
        let Actor::Agent(original_run) = params.actor else {
            panic!("agent expected")
        };
        let params = profile
            .drive_params(
                &params.session_id,
                &params.assistant_turn_id,
                original_run.chain().clone(),
            )
            .unwrap();
        let Actor::Agent(run) = params.actor else {
            panic!("agent expected")
        };
        let captured = PoolAccess::for_system_models(&profile.principal, ["automatic"]);
        (state, run, captured, provider)
    }
    #[tokio::test]
    async fn organization_model_selector_current_pool_removal_is_fatal_without_fallback() {
        let (state, run, captured, provider) = organization_alias_fixture().await;
        let principal = run.principal();
        let authority = ModelAuthority::live(run.clone());
        assert!(
            !run.system_principal()
                .grants
                .has(GrantKind::Model, "model-b")
        );
        let target = route_target(
            &state,
            "automatic",
            &serde_json::json!({"messages":[]}),
            &captured,
            None,
            &principal,
            Some(&authority),
        )
        .await
        .unwrap();
        assert_eq!(target.model, "model-b");
        let access = live_model_access(
            &state,
            &principal,
            Some(&authority),
            &target.access,
            "automatic",
            target.decision.as_ref(),
        )
        .await
        .unwrap();
        assert!(
            state
                .upstreams
                .resolve_model_for(
                    &target.model,
                    aiplane_core::server::upstreams::PoolKind::Chat,
                    &access
                )
                .is_some()
        );
        assert_eq!(provider.received_requests().await.unwrap().len(), 1);
        system_principals::remove_grant(
            &state.db,
            &run.system_principal().id,
            GrantKind::Model,
            "automatic",
            "creator",
        )
        .await
        .unwrap();
        system_principals::add_scoped_grant(
            &state.db,
            &run.system_principal().id,
            GrantKind::Model,
            "automatic",
            Some(&["pool".into()]),
            "creator",
        )
        .await
        .unwrap();
        let gate = LiveSelectorGate {
            state: &state,
            principal: &principal,
            authority: Some(&authority),
            captured: &captured,
            model: "automatic",
        };
        let result = state
            .automatic_router
            .select(
                "automatic",
                &serde_json::json!({"messages":[]}),
                &captured,
                None,
                &gate,
            )
            .await;
        assert!(matches!(
            result,
            Err(AutomaticRoutingError::SelectorUnavailable(_))
        ));
        assert_eq!(provider.received_requests().await.unwrap().len(), 1);
        system_principals::remove_grant(
            &state.db,
            &run.system_principal().id,
            GrantKind::Model,
            "automatic",
            "creator",
        )
        .await
        .unwrap();
        assert!(
            live_model_access(
                &state,
                &principal,
                Some(&authority),
                &target.access,
                "automatic",
                target.decision.as_ref()
            )
            .await
            .is_err()
        );
        assert_eq!(provider.received_requests().await.unwrap().len(), 1);
    }
    #[tokio::test]
    async fn organization_model_final_target_rechecks_current_alias_pool_scope_without_widening() {
        let (state, run, captured, _provider) = organization_alias_fixture().await;
        let principal = run.principal();
        let authority = ModelAuthority::live(run.clone());
        let target = route_target(
            &state,
            "automatic",
            &serde_json::json!({"messages":[]}),
            &captured,
            None,
            &principal,
            Some(&authority),
        )
        .await
        .unwrap();
        assert_eq!(target.model, "model-b");
        system_principals::remove_grant(
            &state.db,
            &run.system_principal().id,
            GrantKind::Model,
            "automatic",
            "creator",
        )
        .await
        .unwrap();
        system_principals::add_scoped_grant(
            &state.db,
            &run.system_principal().id,
            GrantKind::Model,
            "automatic",
            Some(&["selector".into()]),
            "creator",
        )
        .await
        .unwrap();
        let access = live_model_access(
            &state,
            &principal,
            Some(&authority),
            &target.access,
            "automatic",
            target.decision.as_ref(),
        )
        .await
        .unwrap();
        assert!(
            state
                .upstreams
                .resolve_model_for(
                    &target.model,
                    aiplane_core::server::upstreams::PoolKind::Chat,
                    &access
                )
                .is_none()
        );
        system_principals::remove_grant(
            &state.db,
            &run.system_principal().id,
            GrantKind::Model,
            "automatic",
            "creator",
        )
        .await
        .unwrap();
        system_principals::add_scoped_grant(
            &state.db,
            &run.system_principal().id,
            GrantKind::Model,
            "automatic",
            Some(&["pool".into(), "selector".into(), "extra".into()]),
            "creator",
        )
        .await
        .unwrap();
        let access = live_model_access(
            &state,
            &principal,
            Some(&authority),
            &target.access,
            "automatic",
            target.decision.as_ref(),
        )
        .await
        .unwrap();
        assert!(
            state
                .upstreams
                .resolve_model_for(
                    &target.model,
                    aiplane_core::server::upstreams::PoolKind::Chat,
                    &access
                )
                .is_some()
        );
        let scope = access
            .granted_models
            .as_ref()
            .unwrap()
            .get("model-b")
            .unwrap()
            .as_ref()
            .unwrap();
        assert!(!scope.contains("extra"));
    }
}
