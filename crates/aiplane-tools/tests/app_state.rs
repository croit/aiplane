// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! `AppState` tool/skill authorization against the real tool set.
//!
//! These live here rather than beside the code they exercise because they span
//! both layers: the machinery under test is in `aiplane-core`, but the fixtures
//! are the *real* concrete tools, which are in `aiplane-tools`. A unit test
//! inside `aiplane-core` can't reach them — and swapping in invented doubles
//! would hide exactly the id/grouping drift these tests exist to catch.
//! (`echo` / `get_current_timestamp` stay in `aiplane-core` as the canonical
//! trivial tools, so the tests that only need *a* registered tool stay put.)

mod skill_overlay_tests {
    use aiplane_core::server::config::Config;
    use aiplane_core::server::db;
    use aiplane_core::server::rbac::Resolver;
    use aiplane_core::server::rbac::config::{RbacConfig, RoleConfig};
    use aiplane_core::server::upstreams::UpstreamRegistry;
    use aiplane_features::server::skills::{Skill, SkillRegistry, SkillStore, UserSkillStore};
    use aiplane_runtime::server::AppState;
    use aiplane_runtime::server::tools::ToolRegistry;
    use aiplane_tools::enable_tools::EnableTools;
    use aiplane_tools::read_skill::{READ_SKILL_ID, ReadSkill};
    use std::sync::Arc;

    /// Build an `AppState` whose single role grants every tool + model and
    /// the given `skill_grant` (`["*"]`, `["brand"]`, or `[]`), with one
    /// skill `brand` loaded and `read_skill` + `enable_tools` registered.
    async fn state_with_skill_grant(skill_grant: &[&str]) -> AppState {
        let pool = db::open(std::path::Path::new(":memory:")).await.unwrap();
        let registry = SkillRegistry::new([Skill {
            name: "brand".into(),
            title: "Brand".into(),
            description: "Enforce the brand.".into(),
            root: std::path::PathBuf::from("/nonexistent"),
        }]);
        let skills = Arc::new(SkillStore::with_registry(
            std::path::PathBuf::from("/nonexistent"),
            registry,
        ));
        let config = Config::default();
        // Groups live in the database now, so the grants a fixture wants are
        // built straight into the resolver rather than through a config block.
        let rbac_config = RbacConfig {
            default_role: Some("user".into()),
            mappings: vec![],
        };
        let roles = vec![RoleConfig {
            id: "user".into(),
            admin: false,
            tools: vec!["*".into()],
            models: vec!["*".into()],
            skills: skill_grant.iter().map(|s| (*s).to_string()).collect(),
        }];
        let rbac = Arc::new(Resolver::build(rbac_config, roles).unwrap());
        // Empty per-user store (its dir doesn't exist → scans to nothing), so
        // these tests exercise the global-skill path exactly as before.
        let user_skills = Arc::new(UserSkillStore::new(std::path::PathBuf::from(
            "/nonexistent/.users",
        )));
        let mut reg = ToolRegistry::new().with(ReadSkill::new(
            skills.clone(),
            user_skills.clone(),
            rbac.clone(),
        ));
        let et = EnableTools::from_registry(&reg);
        reg = reg.with(et);
        let upstreams = UpstreamRegistry::new(&Default::default()).unwrap();
        AppState::new(config, pool, upstreams, Arc::new(reg), rbac)
            .with_skills(skills)
            .with_user_skills(user_skills)
    }

    #[tokio::test]
    async fn read_skill_is_always_on_when_caller_has_a_permitted_skill() {
        // Fresh session, nothing enabled: `read_skill` rides in alongside the
        // bootstrap because the role permits the loaded `brand` skill — the
        // model can act on the system-message skill listing immediately, no
        // enable_tools round needed.
        let state = state_with_skill_grant(&["*"]).await;
        let allowed = state
            .allowed_tools_for_session(&["user".into()], "u1", "s1")
            .await;
        assert!(
            allowed.iter().any(|id| id == READ_SKILL_ID),
            "read_skill should be always-on with a permitted skill: {allowed:?}"
        );
    }

    #[tokio::test]
    async fn read_skill_stays_lazy_when_no_skill_is_permitted() {
        // Same loaded skill, but the role grants no skills: `read_skill` must
        // not be force-injected (it's RBAC-granted via `*` but falls back to
        // the normal lazy/enable_tools path).
        let state = state_with_skill_grant(&[]).await;
        let allowed = state
            .allowed_tools_for_session(&["user".into()], "u1", "s1")
            .await;
        assert!(
            !allowed.iter().any(|id| id == READ_SKILL_ID),
            "read_skill must stay lazy with no permitted skill: {allowed:?}"
        );
    }
}

mod token_gate_tests {
    use aiplane_core::server::auth::UserCtx;
    use aiplane_core::server::config::Config;
    use aiplane_core::server::db::{self, token_tool_prefs};
    use aiplane_core::server::rbac::Resolver;
    use aiplane_core::server::rbac::config::{RbacConfig, RoleConfig};
    use aiplane_core::server::upstreams::UpstreamRegistry;
    use aiplane_runtime::server::AppState;
    use aiplane_runtime::server::tools::ToolRegistry;
    use aiplane_runtime::server::tools::time::CurrentTimestamp;
    use aiplane_tools::search_web::SearchWeb;
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::Arc;

    /// AppState whose single role grants `*` (every registered tool), with
    /// a couple of easy-to-build tools registered. Enough to exercise the
    /// per-token gate without a live upstream.
    async fn star_state() -> AppState {
        let db = db::open(Path::new(":memory:")).await.unwrap();
        let upstreams = UpstreamRegistry::new(&HashMap::new()).unwrap();
        let tools = Arc::new(ToolRegistry::new().with(SearchWeb).with(CurrentTimestamp));
        let role = RoleConfig {
            id: "all".into(),
            admin: false,
            models: vec!["*".into()],
            tools: vec!["*".into()],
            skills: vec![],
        };
        let rbac = Arc::new(
            Resolver::build(
                RbacConfig {
                    default_role: Some("all".into()),
                    mappings: vec![],
                },
                vec![role],
            )
            .unwrap(),
        );
        AppState::new(Config::default(), db, upstreams, tools, rbac)
    }

    fn ctx(token_id: &str, tools_enabled: bool) -> UserCtx {
        UserCtx {
            user_id: "alice".into(),
            user_email: "alice@example.com".into(),
            token_id: token_id.into(),
            token_name: token_id.into(),
            roles: vec![], // empty → default role "all" applies
            tools_enabled,
            // These tests are about tool grants, not model scope; `None` is
            // the unrestricted default every token has.
            allowed_models: None,
        }
    }

    /// Seed a user + token so `token_tool_prefs` (FK to tokens) can hold
    /// rows for `token_id`.
    async fn seed_token(state: &AppState, token_id: &str) {
        let now = jiff::Timestamp::now();
        db::users::upsert(
            &state.db,
            &db::users::User {
                id: "alice".into(),
                email: "alice@example.com".into(),
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
        db::tokens::insert(
            &state.db,
            &db::tokens::Token {
                id: token_id.into(),
                user_id: "alice".into(),
                name: token_id.into(),
                hash: format!("hash-{token_id}"),
                created_at: now,
                last_used_at: None,
                expires_at: now + jiff::SignedDuration::from_hours(24),
                revoked_at: None,
                tools_enabled: true,
            },
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn master_off_yields_no_tools() {
        let state = star_state().await;
        // Default for a token is off → empty → proxy takes byte-dumb path.
        assert!(
            state
                .allowed_tools_for_token(&ctx("tok", false))
                .await
                .is_empty()
        );
    }

    #[tokio::test]
    async fn master_on_with_no_prefs_grants_the_full_user_set() {
        let state = star_state().await;
        let got = state.allowed_tools_for_token(&ctx("tok", true)).await;
        assert!(got.contains(&"search_web".to_string()));
        assert!(got.contains(&"get_current_timestamp".to_string()));
    }

    #[tokio::test]
    async fn master_on_subtracts_a_disabled_capability() {
        let state = star_state().await;
        seed_token(&state, "tok").await;
        token_tool_prefs::set(&state.db, "tok", "search_web", false)
            .await
            .unwrap();
        let got = state.allowed_tools_for_token(&ctx("tok", true)).await;
        assert!(
            !got.contains(&"search_web".to_string()),
            "disabled key removed"
        );
        assert!(
            got.contains(&"get_current_timestamp".to_string()),
            "siblings kept"
        );
    }

    #[tokio::test]
    async fn token_prefs_are_scoped_per_token() {
        // Disabling on one token must not leak to another.
        let state = star_state().await;
        seed_token(&state, "tok-a").await;
        token_tool_prefs::set(&state.db, "tok-a", "search_web", false)
            .await
            .unwrap();
        let other = state.allowed_tools_for_token(&ctx("tok-b", true)).await;
        assert!(other.contains(&"search_web".to_string()));
    }
}

/// The gate the standing-preferences section is built on.
///
/// `build_preferences_section` in the driver injects a user's preferences into
/// the leading system message only when `RECALL_TOOL_ID` is in
/// `allowed_tools_for_user`. That is the whole enforcement of the Memory
/// switch for injection: a user who turned memory off means "don't use this",
/// not "hide the tools but keep reading my memories in every prompt". Pinned
/// here against the *real* memory tools, so a regrouping that stopped folding
/// `recall` under the `memory` key would fail rather than silently reopen the
/// injection path.
mod memory_gate_tests {
    use aiplane_core::server::config::Config;
    use aiplane_core::server::db::{self, user_tool_prefs};
    use aiplane_core::server::rbac::Resolver;
    use aiplane_core::server::rbac::config::{RbacConfig, RoleConfig};
    use aiplane_core::server::tool_naming::RECALL_TOOL_ID;
    use aiplane_core::server::upstreams::UpstreamRegistry;
    use aiplane_runtime::server::AppState;
    use aiplane_runtime::server::tools::ToolRegistry;
    use aiplane_tools::memory::{Forget, Recall, Remember, UpdateMemory};
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::Arc;

    async fn memory_state() -> AppState {
        let db = db::open(Path::new(":memory:")).await.unwrap();
        let upstreams = UpstreamRegistry::new(&HashMap::new()).unwrap();
        let tools = Arc::new(
            ToolRegistry::new()
                .with(Remember)
                .with(Recall)
                .with(UpdateMemory)
                .with(Forget),
        );
        let rbac = Arc::new(
            Resolver::build(
                RbacConfig {
                    default_role: Some("all".into()),
                    mappings: vec![],
                },
                vec![RoleConfig {
                    id: "all".into(),
                    admin: false,
                    models: vec!["*".into()],
                    tools: vec!["*".into()],
                    skills: vec![],
                }],
            )
            .unwrap(),
        );
        AppState::new(Config::default(), db, upstreams, tools, rbac)
    }

    #[tokio::test]
    async fn memory_on_exposes_recall_so_preferences_may_be_injected() {
        let state = memory_state().await;
        let allowed = state.allowed_tools_for_user(&["all".into()], "alice").await;
        assert!(
            allowed.iter().any(|id| id == RECALL_TOOL_ID),
            "memory left on → the section's gate opens: {allowed:?}"
        );
    }

    #[tokio::test]
    async fn memory_off_closes_the_gate_for_the_whole_family() {
        let state = memory_state().await;
        user_tool_prefs::set(&state.db, "alice", "memory", false)
            .await
            .unwrap();
        let allowed = state.allowed_tools_for_user(&["all".into()], "alice").await;
        assert!(
            !allowed.iter().any(|id| id == RECALL_TOOL_ID),
            "memory switched off must close the injection gate: {allowed:?}"
        );
        // The switch governs the writers too — a user who turned memory off
        // must not keep a model that can still add to the store.
        assert!(
            allowed.is_empty(),
            "the whole memory family shares the switch: {allowed:?}"
        );
    }

    #[tokio::test]
    async fn the_gate_is_per_user() {
        let state = memory_state().await;
        user_tool_prefs::set(&state.db, "alice", "memory", false)
            .await
            .unwrap();
        let bob = state.allowed_tools_for_user(&["all".into()], "bob").await;
        assert!(
            bob.iter().any(|id| id == RECALL_TOOL_ID),
            "alice's choice must not close bob's gate: {bob:?}"
        );
    }
}
