// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Where a request for a model name goes, decided once for every caller
//! that names one: a chat turn, and an agent's own model calls (the topic
//! guard, the route classifier, the evaluation judge, the prompt
//! assistant). An automatic route's alias is resolved by its selector
//! ([`AutomaticRouter::select`]); any other name routes as it is.

use aiplane_core::server::automatic_routing::{
    AutomaticRouteAffinity, AutomaticRouteDecision, AutomaticRoutingError, SelectorGate,
};
use aiplane_core::server::upstreams::PoolAccess;
use serde_json::Value;

use crate::server::AppState;

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
pub async fn route_target(
    state: &AppState,
    model: &str,
    routing_state: &Value,
    access: &PoolAccess,
    affinity: Option<AutomaticRouteAffinity<'_>>,
) -> Result<RouteTarget, AutomaticRoutingError> {
    let gate = PriorityGate { state, access };
    let decision = state
        .automatic_router
        .select(model, routing_state, access, affinity, &gate)
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

struct PriorityGate<'a> {
    state: &'a AppState,
    access: &'a PoolAccess,
}
impl SelectorGate for PriorityGate<'_> {
    async fn admit(&self, _: &str) -> Result<(), aiplane_core::server::limits::LimitExceeded> {
        Ok(())
    }
    fn priority(&self) -> Option<aiplane_core::server::priority::Priority> {
        self.state.config().gateway.priority_enabled.then(|| {
            self.state.rbac.priority_for_roles(
                &self.access.role_ids,
                self.state.config().gateway.default_priority,
            )
        })
    }
    fn wait_budget(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.state.config().gateway.upstream_wait_secs)
    }
}
