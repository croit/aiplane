// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The models a caller may pick, by kind: model ids, backend aliases and —
//! for chat — automatic-route aliases. One list for every surface that
//! offers a model, so they never disagree: the chat picker
//! (`GET /api/v0/models`), what an agent manager may grant
//! (`holds`, `GET /api/v0/agent-resources`) and the gateway default an
//! agent's unset model key runs on (`docs/agents.md` → "Models").
//!
//! It sits here, not in `aiplane-core`, because every consumer is at this
//! layer or above, and it reads the automatic routes and the feature
//! defaults together with the registry.

use std::collections::HashMap;

use aiplane_core::server::db::automatic_routes::{self, AutomaticRoute};
use aiplane_core::server::feature_defaults::{self, Feature};
use aiplane_core::server::upstreams::{Compliance, PoolAccess, PoolKind};

use crate::server::AppState;

/// One model a caller may pick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelChoice {
    pub id: String,
    /// The data-handling flags the picker shows: a flag is clear only when
    /// every pool the name (or, for a route, any model it reaches) is served
    /// by is clear.
    pub compliance: Compliance,
    /// Set for an automatic route.
    pub route: Option<RouteChoice>,
}

/// What an automatic route in the list reaches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteChoice {
    pub candidates: Vec<String>,
    /// Whether the caller may use every model the route can send to: each
    /// candidate and the selector, as well as the fallback a listed route
    /// always has. Only such a route may be granted.
    pub whole: bool,
}

impl ModelChoice {
    /// Whether a manager may grant it: every model, and an automatic route
    /// whose every member they may use themselves.
    pub fn grantable(&self) -> bool {
        self.route.as_ref().is_none_or(|r| r.whole)
    }
}

/// The models of `kind` a caller with `access` may pick, sorted, the
/// gateway default for the kind first when it is among them. Chat adds each
/// automatic route whose alias the caller may name and whose fallback they
/// may reach.
pub async fn offered(state: &AppState, kind: PoolKind, access: &PoolAccess) -> Vec<ModelChoice> {
    let routes = if kind == PoolKind::Chat {
        automatic_routes::all(&state.db)
            .await
            .inspect_err(|err| tracing::warn!(error = %err, "listing automatic routes"))
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let configured = match feature_of(kind) {
        Some(feature) => feature_defaults::get(&state.db, feature).await,
        None => None,
    };
    assemble_choices(state, kind, access, routes, configured.as_deref())
}

pub async fn offered_in(
    state: &AppState,
    tx: &mut aiplane_core::server::db::WriteTx,
    kind: PoolKind,
    access: &PoolAccess,
) -> Result<Vec<ModelChoice>, aiplane_core::server::db::DbError> {
    let routes = if kind == PoolKind::Chat {
        automatic_routes::all_in(tx).await?
    } else {
        Vec::new()
    };
    let configured = match feature_of(kind) {
        Some(feature) => feature_defaults::get_in(tx, feature).await?,
        None => None,
    };
    Ok(assemble_choices(
        state,
        kind,
        access,
        routes,
        configured.as_deref(),
    ))
}

fn assemble_choices(
    state: &AppState,
    kind: PoolKind,
    access: &PoolAccess,
    routes: Vec<AutomaticRoute>,
    configured: Option<&str>,
) -> Vec<ModelChoice> {
    let mut choices: Vec<ModelChoice> = reachable(state, kind, access)
        .into_iter()
        .map(|(id, compliance)| ModelChoice {
            id,
            compliance,
            route: None,
        })
        .collect();
    if kind == PoolKind::Chat {
        for route in routes {
            if choices.iter().any(|c| c.id == route.alias) || !access.allows_model(&route.alias) {
                continue;
            }
            if let Some(choice) = route_choice(state, route, access) {
                choices.push(choice);
            }
        }
    }
    choices.sort_by(|a, b| a.id.cmp(&b.id));
    feature_defaults::promote(configured, &mut choices, |c| c.id.as_str());
    choices
}

/// The gateway's default model for `feature`: the admin's choice under
/// Models & routing → Default models when the gateway offers it, else the
/// first model it offers. What an agent's unset model key runs on.
pub async fn gateway_default(state: &AppState, feature: Feature) -> Option<String> {
    let offered: Vec<String> = offered(state, feature.pool_kind(), &PoolAccess::all())
        .await
        .into_iter()
        .map(|c| c.id)
        .collect();
    let configured = feature_defaults::get(&state.db, feature).await;
    feature_defaults::resolve(configured.as_deref(), &offered)
}

fn feature_of(kind: PoolKind) -> Option<Feature> {
    Feature::ALL.into_iter().find(|f| f.pool_kind() == kind)
}

/// The models of `kind` `access` may route to, with their merged flags.
fn reachable(state: &AppState, kind: PoolKind, access: &PoolAccess) -> HashMap<String, Compliance> {
    state
        .upstreams
        .models_with_compliance_for_kind_for(kind, access)
        .into_iter()
        .filter(|(id, _)| access.allows_model(id))
        .collect()
}

fn route_choice(
    state: &AppState,
    route: AutomaticRoute,
    access: &PoolAccess,
) -> Option<ModelChoice> {
    let targets = access.for_route_targets(&route.alias, route.members());
    let chat = reachable(state, PoolKind::Chat, &targets);
    let selectors = reachable(state, PoolKind::SystemOne, &targets);
    let mut compliance = *chat.get(&route.fallback_target)?;
    match selectors.get(&route.selector_model) {
        Some(selector) => {
            compliance.gdpr &= selector.gdpr;
            compliance.nda &= selector.nda;
        }
        None => {
            compliance.gdpr = false;
            compliance.nda = false;
        }
    }
    let mut whole = selectors.contains_key(&route.selector_model);
    for candidate in &route.candidates {
        match chat.get(&candidate.target) {
            Some(flags) => {
                compliance.gdpr &= flags.gdpr;
                compliance.nda &= flags.nda;
            }
            None => whole = false,
        }
    }
    Some(ModelChoice {
        id: route.alias,
        compliance,
        route: Some(RouteChoice {
            candidates: route.candidates.into_iter().map(|c| c.target).collect(),
            whole,
        }),
    })
}

#[cfg(test)]
pub(super) mod tests {
    use std::collections::{HashMap, HashSet};
    use std::sync::Arc;

    use aiplane_core::server::db::automatic_routes::{AutomaticRoute, AutomaticRouteCandidate};
    use aiplane_core::server::principal::{GrantKind, GrantSet, SystemPrincipal};
    use aiplane_core::server::upstreams::{
        self,
        config::{BackendConfig, PickerStrategy, UpstreamPoolConfig},
    };

    use super::*;

    fn pool(kind: PoolKind, groups: &[&str]) -> UpstreamPoolConfig {
        UpstreamPoolConfig {
            voices: Default::default(),
            offer_voices: Vec::new(),
            allowed_groups: groups.iter().map(|g| g.to_string()).collect(),
            fallback_offline: None,
            compliance: Default::default(),
            enforce_limits: true,
            kind,
            strategy: PickerStrategy::RoundRobin,
            models: Vec::new(),
            backend: vec![BackendConfig {
                alias: None,
                supports_edit: false,
                enabled: true,
                name: format!("{kind:?}-{}", groups.join("-")),
                base_url: "http://upstream.invalid/v1".into(),
                api_key_env: None,
                api_key: None,
                weight: 1,
                max_inflight: 16,
                health_path: "/models".into(),
                models: Vec::new(),
            }],
        }
    }

    /// Chat models `open` (everyone) and `vip` (group `vip` only), the
    /// selector `picker`, and the route `auto` over `open` and `vip`.
    pub(crate) async fn state() -> AppState {
        let db = aiplane_core::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        let pools = HashMap::from([
            ("open".to_string(), pool(PoolKind::Chat, &[])),
            ("vip".to_string(), pool(PoolKind::Chat, &["vip"])),
            ("selector".to_string(), pool(PoolKind::SystemOne, &[])),
            ("stt".to_string(), pool(PoolKind::Transcription, &[])),
        ]);
        let registry = upstreams::UpstreamRegistry::new(&pools).unwrap();
        for (name, model) in [
            ("open", "open-model"),
            ("vip", "vip-model"),
            ("selector", "picker"),
            ("stt", "whisper"),
        ] {
            let found = registry
                .pools()
                .into_iter()
                .find(|p| p.name == name)
                .unwrap();
            found.backends[0].set_models(HashSet::from([model.to_string()]));
        }
        let candidate = |target: &str| AutomaticRouteCandidate {
            key: target.into(),
            target: target.into(),
            description: target.into(),
        };
        automatic_routes::upsert(
            &db,
            &AutomaticRoute {
                alias: "auto".into(),
                selector_model: "picker".into(),
                objective: "balanced".into(),
                instructions: String::new(),
                minimum_confidence: 0.5,
                selector_timeout_ms: 1_000,
                fallback_target: "open-model".into(),
                session_affinity: false,
                session_ttl_seconds: 60,
                rollout: "active".into(),
                version: 0,
                candidates: vec![candidate("open-model"), candidate("vip-model")],
            },
        )
        .await
        .unwrap();
        AppState::new(
            aiplane_core::server::Config::default(),
            db,
            registry,
            Arc::new(crate::server::tools::ToolRegistry::new()),
            Arc::new(aiplane_core::server::rbac::Resolver::empty()),
        )
    }

    fn person(groups: &[&str]) -> PoolAccess {
        PoolAccess {
            role_ids: groups.iter().map(|g| g.to_string()).collect(),
            ..PoolAccess::default()
        }
    }

    fn ids(choices: &[ModelChoice]) -> Vec<&str> {
        choices.iter().map(|c| c.id.as_str()).collect()
    }

    #[tokio::test]
    async fn transactional_choices_use_current_routes_and_default_with_one_connection() {
        let mut state = state().await;
        state.db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with((*state.db.connect_options()).clone())
            .await
            .unwrap();
        let pool = state
            .upstreams
            .pools()
            .into_iter()
            .find(|pool| pool.name == "open")
            .unwrap();
        pool.backends[0].set_models(["open-model".into(), "alternate-model".into()].into());
        let before = offered(&state, PoolKind::Chat, &person(&[])).await;
        assert!(
            !before
                .iter()
                .find(|choice| choice.id == "auto")
                .unwrap()
                .grantable()
        );
        let mut tx = aiplane_core::server::db::WriteTx::begin(&state.db)
            .await
            .unwrap();
        sqlx::query("UPDATE automatic_route_candidates SET target='alternate-model' WHERE route_alias='auto' AND key='vip-model'")
            .execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO app_settings(key,value,updated_at) VALUES('default_model.chat','open-model',?)")
            .bind(jiff::Timestamp::now().to_string()).execute(&mut *tx).await.unwrap();
        let current = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            offered_in(&state, &mut tx, PoolKind::Chat, &person(&[])),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(ids(&current), ["open-model", "alternate-model", "auto"]);
        assert!(
            current
                .iter()
                .find(|choice| choice.id == "auto")
                .unwrap()
                .grantable()
        );
        assert_eq!(
            current
                .iter()
                .find(|choice| choice.id == "auto")
                .unwrap()
                .route
                .as_ref()
                .unwrap()
                .candidates,
            ["open-model", "alternate-model"]
        );
        drop(tx);
        assert_eq!(offered(&state, PoolKind::Chat, &person(&[])).await, before);
        let mut tx = aiplane_core::server::db::WriteTx::begin(&state.db)
            .await
            .unwrap();
        sqlx::query("DROP TABLE automatic_route_candidates")
            .execute(&mut *tx)
            .await
            .unwrap();
        assert!(
            tokio::time::timeout(
                std::time::Duration::from_secs(2),
                offered_in(&state, &mut tx, PoolKind::Chat, &person(&[]))
            )
            .await
            .unwrap()
            .is_err()
        );
    }

    #[tokio::test]
    async fn a_route_with_an_unusable_candidate_is_offered_but_not_grantable() {
        let state = state().await;
        let outsider = offered(&state, PoolKind::Chat, &person(&[])).await;
        assert_eq!(ids(&outsider), ["auto", "open-model"]);
        let auto = outsider.iter().find(|c| c.id == "auto").unwrap();
        assert!(!auto.grantable(), "the outsider may not use `vip-model`");

        let member = offered(&state, PoolKind::Chat, &person(&["vip"])).await;
        assert_eq!(ids(&member), ["auto", "open-model", "vip-model"]);
        assert!(member.iter().all(ModelChoice::grantable));
        assert_eq!(
            member[0].route.as_ref().unwrap().candidates,
            ["open-model", "vip-model"]
        );
    }

    #[tokio::test]
    async fn each_kind_lists_its_own_models_with_the_configured_default_first() {
        let state = state().await;
        feature_defaults::set(&state.db, Feature::Chat, Some("open-model"))
            .await
            .unwrap();
        let chat = offered(&state, PoolKind::Chat, &PoolAccess::all()).await;
        assert_eq!(ids(&chat), ["open-model", "auto", "vip-model"]);
        let stt = offered(&state, PoolKind::Transcription, &PoolAccess::all()).await;
        assert_eq!(ids(&stt), ["whisper"]);
        assert_eq!(
            gateway_default(&state, Feature::Chat).await.as_deref(),
            Some("open-model")
        );
        assert_eq!(
            gateway_default(&state, Feature::Speech).await,
            None,
            "nothing serves speech"
        );
    }

    #[tokio::test]
    async fn an_agent_is_offered_exactly_its_granted_models() {
        let state = state().await;
        let agent = |grants: &[&str]| {
            PoolAccess::for_system(&SystemPrincipal {
                id: "a".into(),
                name: "a".into(),
                grants: Arc::new(GrantSet::new(
                    grants.iter().map(|g| (GrantKind::Model, g.to_string())),
                )),
            })
        };
        assert!(
            offered(&state, PoolKind::Chat, &agent(&[]))
                .await
                .is_empty()
        );
        assert_eq!(
            ids(&offered(&state, PoolKind::Chat, &agent(&["vip-model"])).await),
            ["vip-model"]
        );
        let routed = offered(&state, PoolKind::Chat, &agent(&["auto"])).await;
        assert_eq!(ids(&routed), ["auto"]);
        assert!(routed[0].grantable(), "a granted route reaches its members");
    }
}
