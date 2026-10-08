// SPDX-License-Identifier: AGPL-3.0-only

// Copyright (C) 2026 croit GmbH

//! Dev / playwright harness: spins up the FULL rama gateway against an
//! in-memory SQLite, a wiremock OpenAI-style chat + transcription
//! backend, and a pre-seeded session — then listens on 127.0.0.1:8080.
//!
//! Every page is reachable here: `/`, `/login`, `/settings/tokens`, `/chat`,
//! `/theme/toggle`, the `/api/v0/*` JSON routes — same code path as
//! production, the only thing faked is the upstream LLM and the OIDC
//! handoff. Use this for browser-driven debugging of anything on the
//! UI surface, not just the chat composer.
//!
//! Run with `cargo run --example dev_ui -p gateway` (or
//! `mise run dev-ui`). The example prints the signed session cookie
//! to stdout so playwright (or curl) can inject it:
//!
//! ```bash
//! curl --cookie "id=<the printed value>" http://localhost:8080/chat
//! ```
//!
//! Not part of any test target; not run by CI. Strictly a local-only
//! convenience.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use aiplane::rama_server::{RamaState, SessionStore, router};
use aiplane_core::server::config::{
    ComfyuiConfig, FeedbackConfig, GatewayConfig, RagConfig, SkillsConfig, TypstConfig,
};
use aiplane_core::server::rbac::RoleConfig;
use aiplane_core::server::rbac::{Resolver, config::RbacConfig, config::RoleMapping};
use aiplane_core::server::upstreams::{
    self,
    config::{BackendConfig, PickerStrategy, PoolKind, UpstreamPoolConfig},
};
use aiplane_core::server::{Config, db};
use aiplane_features::server::comfyui::{ChatUpdateRegistry, Client, ComfyuiStore};
use aiplane_features::server::push::PushSender;
use aiplane_features::server::skills::{SkillStore, UserSkillStore};
use aiplane_runtime::server::AppState;
use aiplane_runtime::server::comfyui_tool::ComfyuiHandle;
use aiplane_tools::{enable_tools, read_skill};
use jiff::{Timestamp, ToSpan};
use rama::net::address::SocketAddress;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,gateway=debug".into()),
        )
        .init();
    let session_secret: [u8; 32] =
        aiplane_core::server::crypto::hex_decode(&aiplane_core::server::crypto::random_hex(32))
            .and_then(|bytes| bytes.try_into().ok())
            .expect("random session secret has the requested length");

    // --- Wiremock upstreams ------------------------------------------
    //
    // Two mock servers: one for the chat pool, one for the
    // transcription pool. With the auto-discovery routing layer
    // (`upstreams::health` parses each backend's `/models` response
    // and routes by what it sees), sharing a single mock between
    // pools would have both pools advertise every model — the
    // transcription dropdown would show chat models and vice versa.
    // Splitting them keeps each pool's discovered set realistic.
    //
    //   chat mock: GET /models → `demo-model`
    //              POST /chat/completions → SSE stream
    //   voice mock: GET /models → `demo-whisper`
    //              POST /audio/transcriptions → JSON
    let chat_mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "object": "list",
            // Two chat models so the feedback widget's "Text model" picker has
            // a real choice to render.
            "data": [
                { "id": "demo-model", "object": "model" },
                { "id": "demo-model-pro", "object": "model" },
            ],
        })))
        .mount(&chat_mock)
        .await;
    mount_agent_model(&chat_mock).await;
    mount_architect_model(&chat_mock).await;
    Mock::given(method("POST"))
        .and(path("/systemone"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "model": "demo-selector",
            "answers": {
                "gdpr": {"type": "noul", "noul": 0.9},
                "nda": {"type": "noul", "noul": 0.9}
            }
        })))
        .mount(&chat_mock)
        .await;
    // Feedback field-extraction: matched before the generic non-streaming
    // mock (first-mounted wins on ties) via the unique system-prompt phrase,
    // so the voice→fields flow returns a valid structured JSON object the
    // dialog can drop into its fields.
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(wiremock::matchers::body_string_contains(
            "structured bug/feature report",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "demo-extract",
            "object": "chat.completion",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "{\"title\":\"Save button does nothing on the settings page\",\"description\":\"Clicking Save on the settings page shows no feedback and the change is lost after reload.\",\"business_value\":\"Users cannot persist their preferences, leading to repeated support tickets.\",\"acceptance_criteria\":\"- Clicking Save persists the change\\n- A success toast confirms the save\\n- The value survives a reload\",\"priority\":\"high\"}",
                },
                "finish_reason": "stop",
            }],
        })))
        .mount(&chat_mock)
        .await;
    // Non-streaming response for the tool-loop branch — the runner
    // forces `stream:false` so it can inspect each round. Mounted
    // first so wiremock matches it before the streaming variant.
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(wiremock::matchers::body_string_contains("\"stream\":false"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "demo",
            "object": "chat.completion",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hi! How can I help?",
                },
                "finish_reason": "stop",
            }],
        })))
        .mount(&chat_mock)
        .await;
    // `DEV_UI_CHAT_DELAY_MS` holds the reply back before it starts streaming.
    //
    // Everything the composer does *during* a turn — queueing the next
    // message, interjecting into the running answer, interrupting to re-aim —
    // only exists while a turn is in flight, and against an upstream that
    // answers instantly there is no such moment to click in. Default 0, so the
    // normal harness is as fast as it was.
    let chat_delay_ms: u64 = std::env::var("DEV_UI_CHAT_DELAY_MS")
        .ok()
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(0);
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_delay(std::time::Duration::from_millis(chat_delay_ms))
                .set_body_string(concat!(
                    "data: {\"choices\":[{\"delta\":{\"content\":\"Hi! \"}}]}\n\n",
                    "data: {\"choices\":[{\"delta\":{\"content\":\"How can I help?\"}}]}\n\n",
                    "data: [DONE]\n\n",
                )),
        )
        .mount(&chat_mock)
        .await;

    let voice_mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "object": "list",
            // Two transcription models so the feedback widget's "Voice model"
            // picker has a real choice to render.
            "data": [
                { "id": "demo-whisper", "object": "model" },
                { "id": "demo-whisper-large", "object": "model" },
            ],
        })))
        .mount(&voice_mock)
        .await;
    Mock::given(method("POST"))
        .and(path("/audio/transcriptions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({ "text": "dev transcription stub" })),
        )
        .mount(&voice_mock)
        .await;

    // Spoken answers for the embed widget's read-aloud: half a second of a
    // tone as WAV, which the widget decodes like any speech answer.
    Mock::given(method("POST"))
        .and(path("/audio/speech"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "audio/wav")
                .set_body_bytes(tone_wav(0.5)),
        )
        .mount(&chat_mock)
        .await;

    // --- RamaState (in-memory SQLite + chat + transcription pools) ---
    let pool = db::open(std::path::Path::new(":memory:")).await?;
    let mut pools = HashMap::new();
    pools.insert(
        "chat".to_string(),
        UpstreamPoolConfig {
            voices: Default::default(),
            offer_voices: Vec::new(),
            allowed_groups: Vec::new(),
            fallback_offline: None,
            compliance: aiplane_core::server::upstreams::Compliance {
                gdpr: false,
                nda: false,
            },
            enforce_limits: true,
            kind: PoolKind::Chat,
            strategy: PickerStrategy::RoundRobin,
            models: Vec::new(),
            backend: vec![BackendConfig {
                alias: None,
                supports_edit: false,
                enabled: true,
                name: "wiremock-chat".into(),
                base_url: chat_mock.uri(),
                api_key_env: None,
                api_key: None,
                weight: 1,
                max_inflight: 16,
                health_path: "/models".into(),
                models: Vec::new(),
            }],
        },
    );
    pools.insert(
        "voice".to_string(),
        UpstreamPoolConfig {
            voices: Default::default(),
            offer_voices: Vec::new(),
            allowed_groups: Vec::new(),
            fallback_offline: None,
            compliance: Default::default(),
            enforce_limits: true,
            kind: PoolKind::Transcription,
            strategy: PickerStrategy::RoundRobin,
            models: Vec::new(),
            backend: vec![BackendConfig {
                alias: None,
                supports_edit: false,
                enabled: true,
                name: "wiremock-voice".into(),
                base_url: voice_mock.uri(),
                api_key_env: None,
                api_key: None,
                weight: 1,
                max_inflight: 16,
                health_path: "/models".into(),
                models: Vec::new(),
            }],
        },
    );
    pools.insert(
        "selector".to_string(),
        UpstreamPoolConfig {
            voices: Default::default(),
            offer_voices: Vec::new(),
            allowed_groups: Vec::new(),
            fallback_offline: None,
            compliance: Default::default(),
            enforce_limits: true,
            kind: PoolKind::SystemOne,
            strategy: PickerStrategy::RoundRobin,
            models: vec!["demo-selector".into()],
            backend: vec![BackendConfig {
                alias: None,
                supports_edit: false,
                enabled: true,
                name: "wiremock-selector".into(),
                base_url: chat_mock.uri(),
                api_key_env: None,
                api_key: None,
                weight: 1,
                max_inflight: 16,
                health_path: "/models".into(),
                models: vec!["demo-selector".into()],
            }],
        },
    );
    // Speech (TTS) pool — its mere presence flips `voice_available` on so the
    // chat composer renders the live-voice button (and modal). Points at the
    // chat mock's URL, which answers `/audio/speech` with a short tone for
    // the embed widget; an explicit pool model keeps it out of the
    // /models discovery path.
    pools.insert(
        "speech".to_string(),
        UpstreamPoolConfig {
            voices: Default::default(),
            offer_voices: Vec::new(),
            allowed_groups: Vec::new(),
            fallback_offline: None,
            compliance: Default::default(),
            enforce_limits: true,
            kind: PoolKind::Speech,
            strategy: PickerStrategy::RoundRobin,
            models: vec!["demo-tts".into()],
            backend: vec![BackendConfig {
                alias: None,
                supports_edit: false,
                enabled: true,
                name: "wiremock-speech".into(),
                base_url: chat_mock.uri(),
                api_key_env: None,
                api_key: None,
                weight: 1,
                max_inflight: 16,
                health_path: "/models".into(),
                models: vec!["demo-tts".into()],
            }],
        },
    );
    // Image-generation pool — advertises an image model so the chat's
    // image-generation affordance is present. Static model id; no discovery.
    pools.insert(
        "image".to_string(),
        UpstreamPoolConfig {
            voices: Default::default(),
            offer_voices: Vec::new(),
            allowed_groups: Vec::new(),
            fallback_offline: None,
            compliance: Default::default(),
            enforce_limits: true,
            kind: PoolKind::Image,
            strategy: PickerStrategy::RoundRobin,
            models: vec!["demo-image".into()],
            backend: vec![BackendConfig {
                alias: None,
                supports_edit: true,
                enabled: true,
                name: "wiremock-image".into(),
                base_url: chat_mock.uri(),
                api_key_env: None,
                api_key: None,
                weight: 1,
                max_inflight: 16,
                health_path: "/models".into(),
                models: vec!["demo-image".into()],
            }],
        },
    );
    // The database is the only source of topology now, so the fixture writes
    // rows and then builds the registry back out of them — the same path the
    // real boot takes, rather than a parallel one that could drift from it.
    // Backends here carry no API key, so the crypto instance never seals
    // anything. Fresh in-memory DB every boot, so this always runs.
    let crypto = aiplane_core::server::crypto::Crypto::from_env_or_session(&session_secret);
    for (sort_order, (name, cfg)) in pools.iter().enumerate() {
        for backend in &cfg.backend {
            db::upstreams_config::upsert_backend(
                &pool,
                &db::upstreams_config::BackendRow {
                    name: backend.name.clone(),
                    base_url: backend.base_url.clone(),
                    api_key_env: backend.api_key_env.clone(),
                    api_key_ct: None,
                    api_key_nonce: None,
                    weight: backend.weight,
                    max_inflight: backend.max_inflight,
                    health_path: backend.health_path.clone(),
                    supports_edit: backend.supports_edit,
                    enabled: backend.enabled,
                    models: backend.models.clone(),
                    aliases: Vec::new(),
                    created_at: jiff::Timestamp::now(),
                    updated_at: jiff::Timestamp::now(),
                },
            )
            .await?;
        }
        db::upstreams_config::upsert_pool(
            &pool,
            &db::upstreams_config::PoolRow {
                name: name.clone(),
                kind: cfg.kind.as_str().to_string(),
                strategy: cfg.strategy.as_str().to_string(),
                fallback_offline: cfg.fallback_offline.clone(),
                compliance_gdpr: cfg.compliance.gdpr,
                compliance_nda: cfg.compliance.nda,
                enforce_limits: cfg.enforce_limits,
                sort_order: sort_order as i64,
                backends: cfg.backend.iter().map(|b| b.name.clone()).collect(),
                models: cfg.models.clone(),
                voices: Vec::new(),
                offer_voices: Vec::new(),
                allowed_groups: cfg.allowed_groups.clone(),
                created_at: jiff::Timestamp::now(),
                updated_at: jiff::Timestamp::now(),
            },
        )
        .await?;
    }
    let snapshot = db::upstreams_config::load_snapshot(&pool).await?;
    let registry = upstreams::UpstreamRegistry::from_snapshot(&snapshot, &crypto)?;
    // Run the initial probe round so each backend's `/models` set is
    // populated before we start serving requests. Without this, the
    // first chat-page render lands on empty dropdowns until the
    // looping probe catches up 5 s later.
    upstreams::health::spawn(registry.clone(), None).await;
    // Skills (for the /admin/skills screenshot + local debugging): load the
    // repo's `data/skills` bundles into a hot-reloadable store, grant the dev
    // role every skill, and register `read_skill`. Mirrors `main.rs`.
    //
    // A small, realistic role set so the operator pages look like a real
    // deployment: a privileged `admin`, a baseline `user` (the default role
    // every signed-in user gets), and a few team roles resolved from OIDC
    // groups. `admin` grants every tool + skill so the seed `dev` user (mapped
    // to it via the `platform-admins` group) can reach /admin/*, /rag, and the
    // skills content. Set on `config.roles` too (not just the Resolver) so the
    // skills page's "Granted to" column and the users page's role columns
    // resolve. The wiremock backend doesn't actually invoke tools — the
    // gateway-side path is what we want for playwright / local-browser work.
    let roles = vec![
        RoleConfig {
            id: "admin".into(),
            admin: true,
            models: vec!["*".into()],
            tools: vec!["*".into()],
            skills: vec!["*".into()],
        },
        RoleConfig {
            id: "user".into(),
            admin: false,
            models: vec!["*".into()],
            tools: vec!["*".into()],
            skills: vec![],
        },
        RoleConfig {
            id: "engineering".into(),
            admin: false,
            models: vec!["*".into()],
            tools: vec!["*".into()],
            skills: vec![],
        },
        RoleConfig {
            id: "finance".into(),
            admin: false,
            models: vec!["demo-model".into()],
            tools: vec![],
            skills: vec![],
        },
        RoleConfig {
            id: "support".into(),
            admin: false,
            models: vec!["demo-model".into()],
            tools: vec!["search_web".into()],
            skills: vec![],
        },
    ];
    // Generic, non-croit demo skills shipped beside this example (the real
    // `data/skills` is gitignored local data) — keeps README screenshots clean.
    // Absolute, CARGO_MANIFEST_DIR-anchored so it resolves regardless of cwd.
    let skills_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/demo-skills");
    let comfyui_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/comfyui-workflows");
    // Unresolvable by default, so `/admin/comfyui` shows its "worker
    // unreachable" state out of the box. Point DEV_UI_COMFYUI_URL at a real
    // worker to exercise the healthy path (version, GPU, queue depth).
    let comfyui_url = std::env::var("DEV_UI_COMFYUI_URL")
        .unwrap_or_else(|_| "http://comfyui-worker:8188".to_string());
    let comfyui_config = ComfyuiConfig {
        enabled: true,
        base_url: comfyui_url,
        content_dir: comfyui_dir.clone(),
        timeout_secs: 900,
        queue_poll_interval_ms: 500,
        max_concurrent_jobs: 1,
    };
    // OIDC→role mapping (seed-only, mirrors a real deployment). Every signed-in
    // user gets `user` as a baseline; team/admin roles come from their OIDC
    // groups. `dev` carries the `platform-admins` group → resolves to admin.
    let dev_rbac = RbacConfig {
        default_role: Some("user".into()),
        mappings: vec![
            RoleMapping {
                oidc_claim: "groups".into(),
                oidc_value: "platform-admins".into(),
                role: "admin".into(),
            },
            RoleMapping {
                oidc_claim: "groups".into(),
                oidc_value: "admin".into(),
                role: "admin".into(),
            },
            RoleMapping {
                oidc_claim: "groups".into(),
                oidc_value: "engineering".into(),
                role: "engineering".into(),
            },
            RoleMapping {
                oidc_claim: "groups".into(),
                oidc_value: "finance".into(),
                role: "finance".into(),
            },
            RoleMapping {
                oidc_claim: "groups".into(),
                oidc_value: "support".into(),
                role: "support".into(),
            },
        ],
    };
    let config = Config {
        typst: Some(TypstConfig {
            templates_dir: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../examples/typst-templates"),
        }),
        skills: Some(SkillsConfig {
            dir: skills_dir.clone(),
        }),
        comfyui: Some(comfyui_config.clone()),
        // The fixture seeds two indexed collections and the comment above
        // promises /rag is reachable, so the block has to be present: the SPA
        // hides a feature's nav entry (and answers its URL with "not enabled")
        // when the settings section behind it is off.
        rag: Some(RagConfig::default()),
        // Turn on impersonation so the /admin/users page renders its
        // Impersonate action column (audited in production; harmless here).
        // `bootstrap_admin_groups` mirrors production's break-glass admin so the
        // `dev` user stays admin even after an `/admin/groups` edit reloads the
        // resolver from the (DB-seeded) group tables.
        gateway: GatewayConfig {
            allow_impersonation: true,
            bootstrap_admin_groups: vec!["platform-admins".into()],
            ..Default::default()
        },
        // Seed a feedback config so the floating feedback button + dialog
        // render in local UI debugging. The token is a dummy — recording,
        // transcription, model pickers, and field extraction all work against
        // the wiremock backend; only the final GitHub issue POST would fail
        // (which is fine for UI work). `extraction_model` left empty so the
        // picker defaults to the first chat model.
        feedback: Some(FeedbackConfig {
            provider: "github".into(),
            github_owner: "demo-owner".into(),
            github_repo: "demo-repo".into(),
            github_token: Some("dev-ui-dummy-token".into()),
            github_token_env: None,
            labels: vec!["feedback".into()],
            assets_branch: "feedback-assets".into(),
            // Operator-chosen models (the form has no picker). Empty would also
            // work (first available); set them explicitly for a deterministic
            // demo against the wiremock pools.
            extraction_model: Some("demo-model".into()),
            voice_model: Some("demo-whisper".into()),
            github_api_base: "https://api.github.com".into(),
            // The other tracker, left at its defaults: `provider` above is
            // what decides, so these are inert until it says `gitlab`.
            gitlab_url: "https://gitlab.com".into(),
            gitlab_project_id: String::new(),
            gitlab_token: None,
            gitlab_token_env: None,
        }),
        ..Config::default()
    };
    // Write the fixture's settings as rows. Not cosmetic: saving one section
    // in `/admin/settings` re-applies the whole stored set onto the config, so
    // without these the first save would drop the fixture's ComfyUI, feedback
    // and skills blocks.
    aiplane_core::server::settings::store(
        &pool,
        &crypto,
        &aiplane_core::server::settings::snapshot(&config),
    )
    .await
    .expect("dev_ui settings seed");
    // Seed the DB group tables, then build the resolver from the DB snapshot — so `/admin/groups`
    // shows the seeded groups and an edit + `reload_rbac` round-trips through the
    // same DB path production uses. `bootstrap_admin_groups` keeps `dev` admin.
    aiplane_core::server::db::gateway_groups::seed_roles(&pool, &dev_rbac, &roles)
        .await
        .expect("dev_ui RBAC seed");
    let group_snapshot = aiplane_core::server::db::gateway_groups::load_snapshot(&pool)
        .await
        .expect("dev_ui load groups");
    let rbac = Arc::new(Resolver::from_snapshot(
        group_snapshot,
        config.gateway.bootstrap_admin_groups.clone(),
    ));
    if let Ok(grants) = aiplane_core::server::db::skill_grants::all(&pool).await {
        rbac.set_skill_grant_overlay(grants);
    }
    let user_skill_store = Arc::new(UserSkillStore::new(skills_dir.join(".users")));
    let skill_store = Arc::new(SkillStore::load(skills_dir));
    let typst_family = aiplane::tool_families::typst();
    let typst_metas = aiplane_runtime::server::state::typst_template_metas(&config);
    let base_tools = aiplane::tool_registry::base_registry(rbac.clone(), None, true)
        .with(aiplane_tools::generate_image::GenerateImage)
        .with(aiplane_tools::edit_image::EditImage)
        .with_family_replaced(
            aiplane_core::server::tool_naming::TYPST_PREFIX,
            typst_family(&config, &Default::default()),
        );
    let bootstrap_tool = enable_tools::EnableTools::from_registry(&base_tools);
    let tools = Arc::new(
        base_tools
            .with(bootstrap_tool)
            .with(read_skill::ReadSkill::new(
                skill_store.clone(),
                user_skill_store.clone(),
                rbac.clone(),
            )),
    );
    let comfyui = Arc::new(ComfyuiHandle {
        store: Arc::new(ComfyuiStore::load(comfyui_dir)),
        client: Client::new(comfyui_config.base_url).expect("valid dev ComfyUI URL"),
        runner_poll_interval: std::time::Duration::from_millis(
            comfyui_config.queue_poll_interval_ms,
        ),
        runner_timeout: std::time::Duration::from_secs(comfyui_config.timeout_secs),
        s3: None,
        max_concurrent_jobs: comfyui_config.max_concurrent_jobs,
        job_slots: Arc::new(tokio::sync::Semaphore::new(
            comfyui_config.max_concurrent_jobs,
        )),
        chat_updates: ChatUpdateRegistry::default(),
    });
    let mut app = AppState::new(config, pool.clone(), registry, tools, rbac)
        .with_skills(skill_store)
        .with_user_skills(user_skill_store)
        .with_typst_templates(typst_metas)
        .with_tool_family_builder(typst_family)
        .with_comfyui(comfyui);
    if app.config().push.enabled {
        match PushSender::new(&app.db, &app.crypto, app.config().push.contact.clone()).await {
            Ok(sender) => app = app.with_push(Arc::new(sender)),
            Err(err) => tracing::warn!(error = %err, "dev UI Web Push unavailable"),
        }
    }
    // Enabled usage handle (90-day retention) so the /usage page renders real
    // aggregates instead of the "metrics disabled" banner. Spawn before the
    // pool is moved into the session store.
    let usage = aiplane_core::server::usage::spawn(pool.clone(), 90);
    let sessions = SessionStore::new(pool, session_secret);
    let state = RamaState::new(app, sessions, usage)
        .with_agent_runner(Arc::new(aiplane_runtime::agents::embed::LiveAgentRunner));

    // --- Seed a user + session so the authed UI is reachable ---------
    use aiplane_core::server::db::users;
    let now = Timestamp::now();
    users::upsert(
        &state.db,
        &users::User {
            id: "dev".into(),
            email: "dev@example.com".into(),
            name: Some("Dev User".into()),
            // Maps to the admin role via the RBAC mapping above.
            roles: vec!["platform-admins".into()],
            created_at: now,
            updated_at: now,
            timezone: None,
            speech_voice: None,
        },
    )
    .await?;
    // A second, NON-admin user (OIDC group `engineering` → the `engineering`
    // gateway group, no admin flag) so RBAC enforcement can be exercised in the
    // browser: this user only sees pools / RAG / MCP their groups permit, with
    // no admin bypass.
    users::upsert(
        &state.db,
        &users::User {
            id: "eng".into(),
            email: "eng@example.com".into(),
            name: Some("Eng User".into()),
            roles: vec!["engineering".into()],
            created_at: now,
            updated_at: now,
            timezone: None,
            speech_voice: None,
        },
    )
    .await?;

    // --- Seed representative (non-croit) demo data so the README pages
    // render populated instead of empty "create your first…" states.
    seed_demo_data(&state).await?;
    let erp_mock = erp_mock().await;
    seed_embed_agent(&state, &erp_mock.uri()).await?;

    let session = state.sessions.create("dev").await?;
    let cookie = state.sessions.sign(&session.id);
    let eng_session = state.sessions.create("eng").await?;
    let eng_cookie = state.sessions.sign(&eng_session.id);

    eprintln!("---------------------------------------------------------------");
    eprintln!(
        "dev gateway listening on http://{}",
        std::env::var("DEV_UI_BIND").unwrap_or_else(|_| "127.0.0.1:8080".to_string())
    );
    eprintln!("authed pages: /, /settings/tokens, /chat, /theme/toggle, /api/v0/*");
    eprintln!("embed widget: key {DEV_EMBED_KEY}");
    eprintln!("    serve examples/embed on :8000 (python3 -m http.server 8000 -d examples/embed)");
    eprintln!("    and open http://localhost:8000/?gateway=http://127.0.0.1:8080");
    eprintln!("    scripted agent model: mention an invoice, an email address or a human;");
    eprintln!(
        "    the demo ERP accepts the code {DEV_ERP_CODE}; eng@example.com answers the inbox"
    );
    eprintln!("seed cookie (paste into playwright / curl):");
    eprintln!("    id={cookie}");
    eprintln!("non-admin (engineering) seed cookie:");
    eprintln!("    eng_id={eng_cookie}");
    eprintln!("---------------------------------------------------------------");

    // rc1's `SocketAddress: FromStr` yields a boxed error that anyhow can't
    // absorb via `?`; stringify it. `DEV_UI_BIND` overrides the default bind
    // (e.g. to run alongside a real gateway already on :8080).
    let bind = std::env::var("DEV_UI_BIND").unwrap_or_else(|_| "127.0.0.1:8080".to_string());
    let addr: SocketAddress = bind
        .parse()
        .map_err(|e| anyhow::anyhow!("parse bind address: {e}"))?;
    // `router::serve` binds the socket and listens until SIGINT / panic.
    router::serve(Arc::new(state), addr).await?;
    drop(chat_mock);
    drop(voice_mock);
    drop(erp_mock);
    Ok(())
}

/// The embed key `examples/embed/index.html` ships with. Public by design (it
/// sits in page source), fixed so the example works without copy-pasting.
const DEV_EMBED_KEY: &str = "gwe_0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// The one-time code the dev ERP accepts for any address.
const DEV_ERP_CODE: &str = "123456";

/// A published support agent on the `chat` pool with an embed key for the
/// example page, served from `localhost:8000` or `127.0.0.1:8000`, wired so
/// every agent flow can be clicked through against [`mount_agent_model`]:
/// an `otp` verifier on the dev ERP connector, a `billing` route to a
/// published sub-agent behind that verifier, and a `staff` handoff route
/// answered by the `eng` user through a `respond` share.
async fn seed_embed_agent(state: &RamaState, erp_url: &str) -> anyhow::Result<()> {
    use aiplane_agents::db::agents::{Access, SubjectKind};
    use aiplane_agents::db::{agents, embed_keys, system_principals};
    use aiplane_core::server::auth::token;
    use aiplane_core::server::db::mcp_catalog;
    use aiplane_core::server::principal::GrantKind;

    mcp_catalog::create(
        &state.db,
        mcp_catalog::ConnectorInput {
            key: "erp".into(),
            name: "Demo ERP".into(),
            description: Some(format!(
                "Dev-only ERP: sends verification codes; accepts {DEV_ERP_CODE}."
            )),
            icon: None,
            category: None,
            url: erp_url.to_string(),
            auth: mcp_catalog::AuthKind::None,
            scope: mcp_catalog::Scope::Agent,
            audit: true,
            use_dcr: false,
            client_id: None,
            client_secret_ct: None,
            client_secret_nonce: None,
            authorize_url: None,
            token_url: None,
            registration_url: None,
            scopes: vec![],
            allowed_groups: vec![],
        },
    )
    .await?;
    mcp_catalog::set_enabled(&state.db, "erp", true).await?;

    let billing_spec = serde_json::json!({
        "main": {
            "model": "demo-model",
            "instructions": { "orchestration": "Explain the customer's open invoices." },
            "tools": []
        },
        "finish": { "schema": {
            "type": "object",
            "required": ["answer"],
            "properties": { "answer": { "type": "string" } }
        } }
    })
    .to_string();
    let billing = agents::create(
        &state.db,
        &system_principals::NewPrincipal {
            name: "billing-helper",
            display: "Billing helper",
            description: "Sub-agent: answers invoice questions for a verified customer.",
        },
        &billing_spec,
        "dev",
    )
    .await?
    .ok_or_else(|| anyhow::anyhow!("the dev billing agent already exists"))?;
    let billing_id = &billing.principal.id;
    system_principals::add_grant(&state.db, billing_id, GrantKind::Model, "demo-model", "dev")
        .await?;
    agents::publish(&state.db, billing_id, &billing_spec, "dev").await?;

    let spec = serde_json::json!({
        "main": {
            "model": "demo-model",
            "instructions": { "orchestration": "Answer visitors briefly and politely. \
                Verify the visitor's email before forwarding an invoice question." },
            "tools": []
        },
        "state": {
            "email": { "type": "email", "set_by": ["llm"] },
            "issue": { "type": "enum", "values": ["billing", "other"], "set_by": ["llm"] },
            "verified": { "type": "subject", "set_by": ["verifier:otp"] }
        },
        "verifiers": {
            "otp": { "kind": "mcp_code", "connector": "erp", "email_slot": "email",
                     "writes": { "verified": "result" } }
        },
        "profile": { "color": "#0b6bcb" },
        "publish": { "voice": {
            "input": true, "output": true,
            "transcription_model": "demo-whisper", "speech_model": "demo-tts"
        } },
        "router": { "kind": "rules", "order": ["billing", "staff"] },
        "routes": {
            "billing": {
                "when": { "all": [
                    { "slot": "issue", "eq": "billing" },
                    { "slot": "verified", "provenance": "verifier:otp" }
                ] },
                "agent": billing_id,
                "task": "Invoice question from customer {verified.customer_id}"
            },
            "staff": {
                "when": { "slot": "issue", "eq": "other" },
                "human": { "inbox": "support", "timeout": "30m" }
            }
        }
    })
    .to_string();
    let agent = agents::create(
        &state.db,
        &system_principals::NewPrincipal {
            name: "website-helper",
            display: "Website helper",
            description: "Answers visitors on the embed example page.",
        },
        &spec,
        "dev",
    )
    .await?
    .ok_or_else(|| anyhow::anyhow!("the dev embed agent already exists"))?;
    let id = &agent.principal.id;
    system_principals::add_grant(&state.db, id, GrantKind::Connector, "erp", "dev").await?;
    for model in ["demo-model", "demo-whisper", "demo-tts"] {
        system_principals::add_grant(&state.db, id, GrantKind::Model, model, "dev").await?;
    }
    agents::publish(&state.db, id, &spec, "dev").await?;
    agents::set_share(
        &state.db,
        id,
        SubjectKind::User,
        "eng",
        Access::Respond,
        "dev",
    )
    .await?;
    let key_hash = token::hash_embed_key(DEV_EMBED_KEY)
        .ok_or_else(|| anyhow::anyhow!("DEV_EMBED_KEY is not a well-formed embed key"))?;
    embed_keys::create(
        &state.db,
        &embed_keys::NewEmbedKey {
            principal_id: id,
            name: "embed example page",
            origins: &[
                "http://localhost:8000".to_string(),
                "http://127.0.0.1:8000".to_string(),
            ],
            key_hash: &key_hash,
        },
        "dev",
    )
    .await?;
    Ok(())
}

/// `seconds` of a 440 Hz tone as 16 kHz mono 16-bit WAV.
fn tone_wav(seconds: f64) -> Vec<u8> {
    let samples = (seconds * 16_000.0) as u32;
    let mut out = Vec::with_capacity(44 + samples as usize * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + samples * 2).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    for v in [16u32, 1 | (1 << 16), 16_000, 32_000, 2 | (16 << 16)] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(samples * 2).to_le_bytes());
    for i in 0..samples {
        let v = (f64::from(i) * 440.0 * std::f64::consts::TAU / 16_000.0).sin() * 6_000.0;
        out.extend_from_slice(&(v as i16).to_le_bytes());
    }
    out
}

/// The dev ERP: an MCP server with `send_code` and `check_code` that
/// accepts [`DEV_ERP_CODE`] for every address.
async fn erp_mock() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(|request: &wiremock::Request| {
            let Ok(body) = serde_json::from_slice::<serde_json::Value>(&request.body) else {
                return ResponseTemplate::new(400);
            };
            let answer = |v: serde_json::Value| {
                serde_json::json!({"content": [{"type": "text", "text": v.to_string()}]})
            };
            let result = match body["method"].as_str() {
                Some("initialize") => serde_json::json!({
                    "protocolVersion": "2025-06-18",
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "demo-erp", "version": "1"},
                }),
                Some("tools/list") => serde_json::json!({"tools": [
                    {"name": "send_code", "description": "Mail a one-time code.",
                     "inputSchema": {"type": "object"}},
                    {"name": "check_code", "description": "Check a one-time code.",
                     "inputSchema": {"type": "object"}},
                ]}),
                Some("tools/call") => {
                    let args = &body["params"]["arguments"];
                    match body["params"]["name"].as_str() {
                        Some("send_code") => answer(serde_json::json!({"sent": true})),
                        _ if args["code"] == DEV_ERP_CODE => {
                            answer(serde_json::json!({"valid": true, "customer_id": "K-10042"}))
                        }
                        _ => answer(serde_json::json!({"valid": false})),
                    }
                }
                _ => return ResponseTemplate::new(202),
            };
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/json")
                .set_body_json(
                    serde_json::json!({"jsonrpc": "2.0", "id": body["id"], "result": result}),
                )
        })
        .mount(&server)
        .await;
    server
}

/// A scripted model for agent runs, so the builder's flows run end to end
/// without a real LLM. It answers only requests that carry an agent's
/// synthetic tools (or the test-case judge), acts on keywords of the
/// visitor's last message and calls only the tools the request offers:
///
/// - an email address: `set_email` + `verify_otp_request_code`, and
///   `verify_otp_submit_code` after a wrong code;
/// - "invoice" / "billing" / "Rechnung": `set_issue(billing)` + `forward_request`;
/// - "human" / "person" / "staff" / "Mensch": `set_issue(other)` + `request_human`;
/// - a routed sub-agent (offered `finish`): `finish({answer})`.
///
/// Once the round's tools answered, it says what they returned.
async fn mount_agent_model(chat_mock: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(|request: &wiremock::Request| {
            serde_json::from_slice::<serde_json::Value>(&request.body)
                .is_ok_and(|body| is_judge(&body) || !agent_tools(&body).is_empty())
        })
        .respond_with(|request: &wiremock::Request| {
            let body: serde_json::Value =
                serde_json::from_slice(&request.body).unwrap_or_default();
            if is_judge(&body) {
                return completion(
                    &body,
                    serde_json::json!({ "content":
                        "{\"passed\": true, \"reason\": \"The agent answered the visitor politely.\"}" }),
                );
            }
            completion(&body, scripted_step(&body))
        })
        .mount(chat_mock)
        .await;
}

/// The agent architect's scripted model (`docs/agent-builder.md` → "Agent architect"), for requests that offer `update_agent_draft`. Per turn: create a
/// draft named "Harald" when the conversation plans no agent yet, then set
/// its task and topics, then (when the person said "test") try it, and say
/// what it did.
async fn mount_architect_model(chat_mock: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(|request: &wiremock::Request| {
            serde_json::from_slice::<serde_json::Value>(&request.body).is_ok_and(|body| {
                body["tools"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|t| t["function"]["name"] == "update_agent_draft")
            })
        })
        .respond_with(|request: &wiremock::Request| {
            let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap_or_default();
            completion(&body, architect_step(&body))
        })
        .mount(chat_mock)
        .await;
}

fn architect_step(body: &serde_json::Value) -> serde_json::Value {
    let messages = body["messages"].as_array().cloned().unwrap_or_default();
    let last_user = messages
        .iter()
        .rposition(|m| m["role"] == "user")
        .unwrap_or(0);
    let said = messages
        .get(last_user)
        .and_then(|m| m["content"].as_str())
        .unwrap_or_default()
        .to_lowercase();
    let done: Vec<(String, serde_json::Value)> = messages[last_user..]
        .iter()
        .filter(|m| m["role"] == "assistant")
        .flat_map(|m| m["tool_calls"].as_array().cloned().unwrap_or_default())
        .map(|c| {
            let name = c["function"]["name"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            let result = messages
                .iter()
                .find(|m| m["role"] == "tool" && m["tool_call_id"] == c["id"])
                .and_then(|m| m["content"].as_str())
                .and_then(|t| serde_json::from_str(t).ok())
                .unwrap_or_default();
            (name, result)
        })
        .collect();
    let system = body["messages"][0]["content"].as_str().unwrap_or_default();
    let planned = system
        .split("plans the agent `")
        .nth(1)
        .and_then(|rest| rest.split('`').next())
        .map(str::to_string);
    let agent = planned.or_else(|| {
        done.iter()
            .find(|(n, _)| n == "create_agent_draft")
            .and_then(|(_, r)| r["agent_id"].as_str().map(str::to_string))
    });
    let did = |tool: &str| done.iter().any(|(n, _)| n == tool);
    let Some(agent) = agent else {
        if did("create_agent_draft") {
            return serde_json::json!({ "content": "I could not create the agent — see the tool call above." });
        }
        return calls(&[(
            "create_agent_draft",
            serde_json::json!({ "display": "Harald", "description": "Answers questions about Acme orders" }),
        )]);
    };
    if !did("update_agent_draft") {
        return calls(&[(
            "update_agent_draft",
            serde_json::json!({ "agent_id": agent, "changes": {
                "task": "You answer questions about Acme orders. First ask for the order number.",
                "scope": { "topics": ["Acme orders", "Shipping"],
                           "refusal": "I can only help with Acme orders.", "strict": false },
                "slots": [{ "name": "order_number", "label": "Order number", "type": "text",
                            "choices": [] }]
            } }),
        )]);
    }
    if said.contains("test") && !did("run_test_turn") {
        return calls(&[(
            "run_test_turn",
            serde_json::json!({ "agent_id": agent, "message": "Where is my order?" }),
        )]);
    }
    serde_json::json!({ "content": format!(
        "I set the task, the topics and the order number to collect. You can undo that \
         change with the button above. When you are happy, review it and press **Publish** on \
         [the setup page](/agents/{agent})."
    ) })
}

fn is_judge(body: &serde_json::Value) -> bool {
    body["messages"][0]["content"]
        .as_str()
        .is_some_and(|s| s.starts_with("You grade a conversation"))
}

fn agent_tools(body: &serde_json::Value) -> Vec<String> {
    body["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| t["function"]["name"].as_str())
        .filter(|n| {
            n.starts_with("set_")
                || n.starts_with("verify_")
                || ["forward_request", "request_human", "finish"].contains(n)
        })
        .map(str::to_string)
        .collect()
}

fn scripted_step(body: &serde_json::Value) -> serde_json::Value {
    let offered = agent_tools(body);
    let has = |t: &str| offered.iter().any(|o| o == t);
    let messages = body["messages"].as_array().cloned().unwrap_or_default();
    let last_user = messages
        .iter()
        .rposition(|m| m["role"] == "user")
        .unwrap_or(0);
    let said = messages
        .get(last_user)
        .and_then(|m| m["content"].as_str())
        .unwrap_or_default()
        .to_string();
    let results: Vec<serde_json::Value> = messages[last_user..]
        .iter()
        .filter(|m| m["role"] == "tool")
        .map(|m| {
            let text = m["content"].as_str().unwrap_or_default();
            serde_json::from_str(text).unwrap_or_else(|_| serde_json::json!(text))
        })
        .collect();
    if has("finish") {
        return if results.is_empty() {
            calls(&[(
                "finish",
                serde_json::json!({ "result": {
                    "answer": "Invoice 2026-0815 over 120.00 EUR is open, due on 31 October."
                } }),
            )])
        } else {
            serde_json::json!({ "content": "Done." })
        };
    }
    if let Some(last) = results.last() {
        if last["reason"] == "wrong_code" && has("verify_otp_submit_code") {
            return calls(&[("verify_otp_submit_code", serde_json::json!({}))]);
        }
        return serde_json::json!({ "content": summarise(last) });
    }
    let lower = said.to_lowercase();
    let email = said
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '@' && c != '.'))
        .map(|w| w.trim_end_matches('.'))
        .find(|w| w.contains('@') && w.contains('.'));
    let mut plan = Vec::new();
    if let Some(email) = email.filter(|_| has("set_email")) {
        plan.push(("set_email", serde_json::json!({ "value": email })));
        if has("verify_otp_request_code") {
            plan.push(("verify_otp_request_code", serde_json::json!({})));
        }
    } else if ["invoice", "billing", "rechnung"]
        .iter()
        .any(|k| lower.contains(k))
    {
        if has("set_issue") {
            plan.push(("set_issue", serde_json::json!({ "value": "billing" })));
        }
        if has("forward_request") {
            plan.push(("forward_request", serde_json::json!({})));
        }
    } else if ["human", "person", "staff", "mensch"]
        .iter()
        .any(|k| lower.contains(k))
    {
        if has("set_issue") {
            plan.push(("set_issue", serde_json::json!({ "value": "other" })));
        }
        if has("request_human") {
            plan.push(("request_human", serde_json::json!({ "question": said })));
        }
    }
    if plan.is_empty() {
        return serde_json::json!({
            "content": "Hi! How can I help? Ask me about an **invoice**, or for a person."
        });
    }
    calls(&plan)
}

/// What the scripted model says once its round's tools answered.
fn summarise(result: &serde_json::Value) -> String {
    fn find_answer(v: &serde_json::Value) -> Option<String> {
        match v {
            serde_json::Value::Object(map) => map
                .get("answer")
                .and_then(|a| a.as_str().map(str::to_string))
                .or_else(|| map.values().find_map(find_answer)),
            serde_json::Value::Array(items) => items.iter().find_map(find_answer),
            _ => None,
        }
    }
    if let Some(answer) = find_answer(result) {
        return format!("Here is the answer: {answer}");
    }
    if result["reason"] == "no_open_route" {
        return "I can look at invoices once your email address is verified. \
                What is your email address?"
            .into();
    }
    if result["verified"] == true || result["valid"] == true {
        return "Thanks, you are verified. What would you like to know about your invoices?".into();
    }
    format!("Done: {result}")
}

fn calls(plan: &[(&str, serde_json::Value)]) -> serde_json::Value {
    let stamp = Timestamp::now().as_millisecond();
    let tool_calls: Vec<serde_json::Value> = plan
        .iter()
        .enumerate()
        .map(|(i, (name, args))| {
            serde_json::json!({
                "index": i,
                "id": format!("dev_{stamp}_{i}"),
                "type": "function",
                "function": { "name": name, "arguments": args.to_string() },
            })
        })
        .collect();
    serde_json::json!({ "tool_calls": tool_calls })
}

/// One answer in the shape the request asked for: an SSE stream of one
/// delta, or a whole completion.
fn completion(body: &serde_json::Value, delta: serde_json::Value) -> ResponseTemplate {
    if body["stream"] == true {
        let sse = format!(
            "data: {}\n\ndata: [DONE]\n\n",
            serde_json::json!({"choices": [{"index": 0, "delta": delta}]})
        );
        return ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream");
    }
    let mut message = delta;
    message["role"] = serde_json::json!("assistant");
    ResponseTemplate::new(200).set_body_json(serde_json::json!({
        "id": "dev-agent",
        "object": "chat.completion",
        "choices": [{ "index": 0, "message": message, "finish_reason": "stop" }],
    }))
}

/// Seed a handful of realistic, **non-croit** rows so the README
/// screenshots show populated pages: one finished chat conversation, a few
/// scheduled actions, and two indexed RAG collections. All owned by the
/// `dev` user. In-memory DB, so this is rebuilt fresh on every launch.
async fn seed_demo_data(state: &RamaState) -> anyhow::Result<()> {
    use aiplane_core::server::db::{chat_compactions, documents, rag};
    use aiplane_runtime::server::scheduled::{self, NewAction};
    use aiplane_runtime::server::webhooks::{self, NewWebhook};
    use session_core::attachments;
    use session_core::db::{self as chatdb, ToolCallStatus, TurnStatus};

    // --- ComfyUI job history -------------------------------------------
    // Spread over the last few hours with realistic durations: the run list
    // renders relative times and wall-clock durations, and a pile of jobs
    // all created "now" with a zero-second duration exercises neither.
    // `jobs::create`/`complete` stamp `Timestamp::now()`, so backdate the
    // pair afterwards — dev-harness data only, never a production path.
    struct SeedJob {
        workflow: &'static str,
        kind: &'static str,
        node: &'static str,
        minutes_ago: i64,
        duration_secs: i64,
        error: Option<&'static str>,
    }
    const fn seed(
        workflow: &'static str,
        kind: &'static str,
        node: &'static str,
        minutes_ago: i64,
        duration_secs: i64,
        error: Option<&'static str>,
    ) -> SeedJob {
        SeedJob {
            workflow,
            kind,
            node,
            minutes_ago,
            duration_secs,
            error,
        }
    }
    let recent_jobs = [
        seed("text_to_image", "image", "9", 8, 9, None),
        seed("merge_video_audio", "video", "9", 21, 3, None),
        seed("image_to_video", "video", "108", 34, 834, None),
        seed(
            "image_to_video",
            "video",
            "108",
            62,
            900,
            Some("Timed out after 900 seconds"),
        ),
        seed("text_to_music", "audio", "3", 95, 6, None),
        seed("clone_voice", "audio", "3", 140, 12, None),
        seed("text_to_image", "image", "9", 190, 8, None),
        seed("upscale_image", "image", "9", 260, 24, None),
    ];
    for (index, job) in recent_jobs.iter().enumerate() {
        let SeedJob {
            workflow,
            kind,
            node,
            minutes_ago,
            duration_secs,
            error,
        } = job;
        let id = aiplane_features::server::comfyui::jobs::create(
            &state.db,
            &format!("demo-prompt-{index}"),
            "demo-session",
            "demo-turn",
            "dev",
            workflow,
            kind,
            node,
            &format!("llmgw-{workflow}"),
        )
        .await?;
        match error {
            None => {
                let extension = match *kind {
                    "video" => "mp4",
                    "audio" => "mp3",
                    _ => "png",
                };
                aiplane_features::server::comfyui::jobs::complete(
                    &state.db,
                    id,
                    &format!("{workflow}-{id}.{extension}"),
                    match *kind {
                        "video" => "video/mp4",
                        "audio" => "audio/mpeg",
                        _ => "image/png",
                    },
                )
                .await?;
            }
            Some(message) => {
                aiplane_features::server::comfyui::jobs::timeout(&state.db, id, message).await?;
            }
        }
        let started = jiff::Timestamp::now() - jiff::SignedDuration::from_mins(*minutes_ago);
        let finished = started + jiff::SignedDuration::from_secs(*duration_secs);
        sqlx::query("UPDATE comfyui_jobs SET created_at = ?, completed_at = ? WHERE id = ?")
            .bind(started.to_string())
            .bind(finished.to_string())
            .bind(id)
            .execute(&state.db)
            .await?;
    }
    // One still in flight, so the page shows its pending state too.
    aiplane_features::server::comfyui::jobs::create(
        &state.db,
        "demo-prompt-running",
        "demo-session",
        "demo-turn",
        "dev",
        "talking_video",
        "video",
        "108",
        "llmgw-talking-video",
    )
    .await?;

    // --- An image-generation conversation (seeded FIRST so the gzip chat
    // below stays the most-recent session and `/chat` still lands on it).
    // The generated image is a self-contained SVG data URI carried via a
    // `gw-attachment` marker — the exact mechanism the image-gen tool uses,
    // minus S3 — so it renders inline like a real generated image.
    const DEMO_IMAGE_DATA_URI: &str = concat!(
        "data:image/svg+xml;base64,",
        include_str!("demo-genimg.b64")
    );
    let img = chatdb::create_session(&state.db, "dev").await?;
    chatdb::set_session_title(&state.db, &img.id, "Generate a hero image").await?;
    let iu = uuid::Uuid::new_v4().to_string();
    chatdb::create_user_turn(
        &state.db,
        &img.id,
        &iu,
        "Generate a wide hero image: a serene mountain lake at sunset, warm colors, digital art.",
    )
    .await?;
    let ia = uuid::Uuid::new_v4().to_string();
    chatdb::create_assistant_turn_in_progress(&state.db, &img.id, &ia, "demo-image").await?;
    let img_marker = attachments::marker_line(
        "mountain-lake-sunset.svg",
        "image/svg+xml",
        DEMO_IMAGE_DATA_URI,
        4096,
    );
    chatdb::append_content(
        &state.db,
        &ia,
        &format!("Here's your hero image — a serene mountain lake at sunset in a warm, painterly style.\n\n{img_marker}"),
    )
    .await?;
    chatdb::finalize_turn(&state.db, &ia, TurnStatus::Completed, None).await?;

    // --- A document-workspace conversation for exercising the docked canvas
    // and its session-scoped assets tab in browser tests.
    let workspace = chatdb::create_session(&state.db, "dev").await?;
    chatdb::set_session_title(&state.db, &workspace.id, "Draft a project brief").await?;
    let wu = uuid::Uuid::new_v4().to_string();
    chatdb::create_user_turn(
        &state.db,
        &workspace.id,
        &wu,
        "Draft a concise project brief and keep it in the document canvas.",
    )
    .await?;
    let wa = uuid::Uuid::new_v4().to_string();
    chatdb::create_assistant_turn_in_progress(&state.db, &workspace.id, &wa, "demo-model").await?;
    let draft_marker = attachments::marker_line(
        "project-brief.md",
        "text/markdown",
        &aiplane_features::server::chat_attachments::proxy_url(&wa, "project-brief.md"),
        860,
    );
    chatdb::append_content(
        &state.db,
        &wa,
        &format!("I drafted the brief in the canvas and attached an export.\n\n{draft_marker}"),
    )
    .await?;
    chatdb::finalize_turn(&state.db, &wa, TurnStatus::Completed, None).await?;
    let project_brief_id = documents::new_id();
    documents::create(
        &state.db,
        &project_brief_id,
        &workspace.id,
        "dev",
        "Project brief",
        documents::DocumentFormat::Markdown,
        "# Project brief\n\n## Goal\n\nShip a reliable, accessible gateway experience.\n\n## Success criteria\n\n- Complete feature parity\n- Clear operator workflows\n- Responsive layouts",
        Some(&wa),
    )
    .await?;
    documents::append_version(
        &state.db,
        &workspace.id,
        &project_brief_id,
        "# Project brief\n\n## Goal\n\nShip a reliable, accessible gateway experience.\n\n## Release criteria\n\n- Complete feature parity\n- Clear operator workflows\n- Responsive layouts\n- Verified document history",
        Some("Added release criteria"),
        Some(&wa),
        documents::VersionAuthor::Assistant,
    )
    .await?;

    // --- A finished `browser_control` conversation: the assistant working in
    // the user's own logged-in browser. Seeded because the tool's own path
    // needs a paired extension and a real Chrome, which neither a browser test
    // nor the Web Store screenshot run can stand up — and the listing has to
    // show the feature rather than describe it. Everything below is the shape
    // a real batch leaves in the database.
    const BROWSER_REASONING: &str = "The supplier portal is behind their login, so \
        `fetch_url` would get the sign-in page. This is what the paired browser is \
        for: navigate there in the session the user already has, then read the page \
        rather than guess at its markup.";
    const BROWSER_ANSWER_MD: &str = "Order **48217** shipped yesterday.\n\n\
        | Field | Value |\n\
        | --- | --- |\n\
        | Status | Shipped — 18 Sep, 16:42 |\n\
        | Carrier | DHL Freight |\n\
        | Tracking | `JD014600008912345678` |\n\
        | Expected | 22 Sep |\n\n\
        Two of the four pallets went out on an earlier truck, so the portal shows one \
        tracking number covering the whole order.\n\n\
        I read this in your browser, in your own session — nothing was submitted or \
        changed.";
    let bc = chatdb::create_session(&state.db, "dev").await?;
    chatdb::set_session_title(&state.db, &bc.id, "Has order 48217 shipped?").await?;
    let bcu = uuid::Uuid::new_v4().to_string();
    chatdb::create_user_turn(
        &state.db,
        &bc.id,
        &bcu,
        "The supplier portal only works when I'm signed in. Open order 48217 and tell me whether it shipped, with the tracking number.",
    )
    .await?;
    let bca = uuid::Uuid::new_v4().to_string();
    chatdb::create_assistant_turn_in_progress(&state.db, &bc.id, &bca, "demo-model").await?;
    chatdb::append_reasoning(&state.db, &bca, BROWSER_REASONING).await?;
    chatdb::set_reasoning_elapsed(&state.db, &bca, 900).await?;
    chatdb::insert_running_tool_call(
        &state.db,
        &bca,
        "call_browser",
        "browser_control",
        r#"{"actions":[{"action":"navigate","url":"https://portal.example-supplier.com/orders/48217"},{"action":"wait_for","text":"Order 48217"},{"action":"read_page"}]}"#,
    )
    .await?;
    chatdb::complete_tool_call(
        &state.db,
        &bca,
        "call_browser",
        r#"{"results":[{"action":"navigate","ok":true,"url":"https://portal.example-supplier.com/orders/48217","title":"Order 48217 — Supplier Portal"},{"action":"wait_for","ok":true},{"action":"read_page","ok":true,"text":"Order 48217 · Status: Shipped 18 Sep 16:42 · Carrier: DHL Freight · Tracking: JD014600008912345678 · Expected delivery: 22 Sep · 4 pallets, 2 dispatched early"}]}"#,
        ToolCallStatus::Completed,
    )
    .await?;
    chatdb::append_content(&state.db, &bca, BROWSER_ANSWER_MD).await?;
    chatdb::finalize_turn(&state.db, &bca, TurnStatus::Completed, None).await?;

    // --- A finished chat conversation showcasing the tool-call loop:
    // reasoning → web search → page fetch → a markdown answer with a source.
    const REASONING: &str = "This is a configuration question with a canonical \
        answer in the official nginx module docs, so I'll search for the \
        ngx_http_gzip_module page and quote the key directives rather than rely \
        on memory.";
    const ANSWER_MD: &str = "Here's a minimal gzip setup for nginx:\n\n\
        ```nginx\n\
        gzip on;\n\
        gzip_types text/plain text/css application/json application/javascript;\n\
        gzip_min_length 1024;\n\
        gzip_comp_level 5;\n\
        ```\n\n\
        - `gzip on;` turns compression on.\n\
        - `gzip_types` lists the MIME types to compress (HTML is always included).\n\
        - `gzip_min_length` skips tiny responses where compression isn't worth the CPU.\n\n\
        A table, because wide markdown tables are the layout case that breaks \
        first — every column here is long enough to push past the bubble:\n\n\
        | Directive | Default | Context | Notes |\n\
        | --- | --- | --- | --- |\n\
        | `gzip` | `off` | http, server, location, if in location | Enables or disables gzipping of responses. |\n\
        | `gzip_types` | `text/html` | http, server, location | `text/plain text/css application/json application/javascript text/xml application/xml application/xml+rss text/javascript` |\n\
        | `gzip_min_length` | `20` | http, server, location | Sets the minimum length of a response that will be gzipped, determined only from the `Content-Length` response header field. |\n\n\
        **Source:** [nginx — ngx_http_gzip_module](https://nginx.org/en/docs/http/ngx_http_gzip_module.html)";
    let s = chatdb::create_session(&state.db, "dev").await?;
    chatdb::set_session_title(&state.db, &s.id, "Enabling gzip in nginx").await?;
    let u = uuid::Uuid::new_v4().to_string();
    chatdb::create_user_turn(
        &state.db,
        &s.id,
        &u,
        "How do I turn on gzip compression in nginx? Give me a minimal config and cite the official docs.",
    )
    .await?;
    let a = uuid::Uuid::new_v4().to_string();
    chatdb::create_assistant_turn_in_progress(&state.db, &s.id, &a, "demo-model").await?;
    chatdb::append_reasoning(&state.db, &a, REASONING).await?;
    chatdb::set_reasoning_elapsed(&state.db, &a, 1400).await?;
    // Tool call 1 — web search.
    chatdb::insert_running_tool_call(
        &state.db,
        &a,
        "call_search",
        "search_web",
        r#"{"query":"nginx ngx_http_gzip_module enable gzip directives"}"#,
    )
    .await?;
    chatdb::complete_tool_call(
        &state.db,
        &a,
        "call_search",
        r#"{"results":[{"title":"Module ngx_http_gzip_module","url":"https://nginx.org/en/docs/http/ngx_http_gzip_module.html","snippet":"A filter that compresses responses with the gzip method. Directives: gzip, gzip_types, gzip_min_length, gzip_comp_level."}]}"#,
        ToolCallStatus::Completed,
    )
    .await?;
    // Tool call 2 — fetch the doc page.
    chatdb::insert_running_tool_call(
        &state.db,
        &a,
        "call_fetch",
        "fetch_url",
        r#"{"url":"https://nginx.org/en/docs/http/ngx_http_gzip_module.html"}"#,
    )
    .await?;
    chatdb::complete_tool_call(
        &state.db,
        &a,
        "call_fetch",
        r#"{"url":"https://nginx.org/en/docs/http/ngx_http_gzip_module.html","text":"Syntax: gzip on | off; Default: gzip off; Context: http, server, location. Enables or disables gzipping of responses. gzip_types, gzip_min_length and gzip_comp_level tune which responses are compressed and how hard."}"#,
        ToolCallStatus::Completed,
    )
    .await?;
    chatdb::append_content(&state.db, &a, ANSWER_MD).await?;
    chatdb::finalize_turn(&state.db, &a, TurnStatus::Completed, None).await?;
    chat_compactions::upsert(
        &state.db,
        &s.id,
        0,
        "The user asked how to enable gzip in nginx.",
        Some(180),
        Some(18),
    )
    .await?;

    // --- Loop retries ---------------------------------------------------
    //
    // Both shapes a looping turn ends in: one retried at a lower effort that
    // then answered (one collapsed attempt above the answer), and one that
    // looped on every try (two attempts and the translated warning).
    let loop_text = "Wait — is the OSD id the same after the swap? Let me check again. ".repeat(12);
    let s = chatdb::create_session(&state.db, "dev").await?;
    chatdb::set_session_title(&state.db, &s.id, "A loop retried at a lower effort").await?;
    let u = uuid::Uuid::new_v4().to_string();
    chatdb::create_user_turn(
        &state.db,
        &s.id,
        &u,
        "Which ceph OSD id does a replaced disk get?",
    )
    .await?;
    let a = uuid::Uuid::new_v4().to_string();
    chatdb::create_assistant_turn_in_progress(&state.db, &s.id, &a, "demo-model").await?;
    chatdb::append_reasoning(&state.db, &a, &loop_text).await?;
    let stop = |effort, retry_effort| chatdb::AttemptStop {
        effort,
        retry_effort,
        stop_reason: "loop",
    };
    chatdb::stash_attempt(&state.db, &a, (0, 0), stop("low", Some("off"))).await?;
    chatdb::append_content(
        &state.db,
        &a,
        "With `ceph osd destroy` followed by `ceph-volume lvm create --osd-id <id>`, the replacement keeps the old id.",
    )
    .await?;
    chatdb::finalize_turn(&state.db, &a, TurnStatus::Completed, None).await?;

    let s = chatdb::create_session(&state.db, "dev").await?;
    chatdb::set_session_title(&state.db, &s.id, "A loop on every retry").await?;
    let u = uuid::Uuid::new_v4().to_string();
    chatdb::create_user_turn(
        &state.db,
        &s.id,
        &u,
        "Explain every ceph config option in detail.",
    )
    .await?;
    let a = uuid::Uuid::new_v4().to_string();
    chatdb::create_assistant_turn_in_progress(&state.db, &s.id, &a, "demo-model").await?;
    chatdb::append_reasoning(&state.db, &a, &loop_text).await?;
    chatdb::stash_attempt(&state.db, &a, (0, 0), stop("low", Some("off"))).await?;
    chatdb::append_content(&state.db, &a, &"The option is the option. ".repeat(20)).await?;
    chatdb::stash_attempt(&state.db, &a, (0, 0), stop("off", None)).await?;
    chatdb::set_error_code(&state.db, &a, "loop_exhausted").await?;
    chatdb::finalize_turn(
        &state.db,
        &a,
        TurnStatus::Errored,
        Some("The model kept repeating itself on all 2 attempts, even with less thinking."),
    )
    .await?;

    // --- Scheduled actions, with the run history behind them -------------
    //
    // The three shapes /scheduled has to render, one each: a schedule that
    // opens a fresh chat every time (a list to link to), one that reuses a
    // single conversation (one chat to link straight into, however often it
    // fired), and one that has never run (nothing to link at all).
    let schedules = [
        (
            "Daily standup digest",
            "Summarize yesterday's merged PRs and open blockers into a short standup digest.",
            "0 8 * * 1-5",
            "2026-06-22T08:00:00Z",
            false,
            3,
        ),
        (
            "Weekly dependency report",
            "List dependencies with new releases this week and flag any security advisories.",
            "0 9 * * 1",
            "2026-06-22T09:00:00Z",
            true,
            4,
        ),
        (
            "Monthly cost summary",
            "Summarize this month's API usage and token spend, with the three biggest line items.",
            "0 7 1 * *",
            "2026-07-01T07:00:00Z",
            false,
            0,
        ),
    ];
    for (name, prompt, cron, next, reuse, runs) in schedules {
        let action = scheduled::create(
            &state.db,
            NewAction {
                user_id: "dev".into(),
                name: name.into(),
                prompt: prompt.into(),
                model: "demo-model".into(),
                cron: cron.into(),
                timezone: "Europe/Berlin".into(),
                tools_enabled: true,
                reuse_conversation: reuse,
                reuse_rounds: 5,
                next_run_at: Some(next.parse()?),
            },
        )
        .await?;
        let mut reused: Option<String> = None;
        for run in 0..runs {
            // A reusing schedule appends into the session the first run
            // opened; the others mint one per fire, exactly as the worker does.
            let session = match &reused {
                Some(id) => id.clone(),
                None => {
                    let s = chatdb::create_session(&state.db, "dev").await?;
                    chatdb::set_session_title(&state.db, &s.id, name).await?;
                    if reuse {
                        reused = Some(s.id.clone());
                    }
                    s.id
                }
            };
            let user_turn = uuid::Uuid::new_v4().to_string();
            chatdb::create_user_turn(&state.db, &session, &user_turn, prompt).await?;
            let turn = uuid::Uuid::new_v4().to_string();
            chatdb::create_assistant_turn_in_progress(&state.db, &session, &turn, "demo-model")
                .await?;
            chatdb::append_content(&state.db, &turn, "Here is the digest for this run.").await?;
            chatdb::finalize_turn(&state.db, &turn, TurnStatus::Completed, None).await?;

            // The last run of the dependency report failed, so the page has a
            // failure to render as well as successes.
            let failed = reuse && run + 1 == runs;
            let run_id = scheduled::record_run_start(&state.db, &action.id).await?;
            let (status, error) = if failed {
                ("error", Some("upstream timed out after 60s"))
            } else {
                ("ok", None)
            };
            scheduled::finish_run(&state.db, &run_id, status, Some(&session), error).await?;
            // Seeded in a tight loop, so every run would carry the same
            // timestamp and the history page would look broken. Space them a
            // day apart, oldest first, the way real fires arrive.
            let fired_at = (jiff::Timestamp::now()
                - jiff::SignedDuration::from_hours(24 * (runs - run - 1)))
            .to_string();
            sqlx::query("UPDATE scheduled_runs SET fired_at = ? WHERE id = ?")
                .bind(&fired_at)
                .bind(&run_id)
                .execute(&state.db)
                .await?;
            scheduled::mark_ran(
                &state.db,
                &action.id,
                status,
                Some(&session),
                Some(next.parse()?),
                error,
            )
            .await?;
        }
    }

    // --- Webhooks, with the run history behind them -----------------------
    //
    // The same three shapes /webhooks has to render as /scheduled: a hook
    // that opens a fresh chat per fire, one that reuses a single conversation,
    // and one that has never fired. The last also carries a stored payload, so
    // the rerun link has something to replay.
    let hooks = [
        (
            "Deploy digest",
            "Summarise this deploy payload and flag anything that looks risky.",
            false,
            false,
            3,
        ),
        (
            "Incident tracker",
            "Append this alert to the running incident summary.",
            true,
            true,
            4,
        ),
        (
            "Release notes draft",
            "Turn this changelog payload into customer-facing release notes.",
            false,
            true,
            0,
        ),
        // Fired exactly once: its chat is one click away *and* its history is
        // still reachable, because a webhook run records the payload it fired
        // with and the chat does not show that.
        (
            "Signup alert",
            "Summarise this signup event for the growth channel.",
            false,
            false,
            1,
        ),
    ];
    for (index, (name, prompt, reuse, synchronous, fires)) in hooks.into_iter().enumerate() {
        let hook = webhooks::create(
            &state.db,
            NewWebhook {
                user_id: "dev".into(),
                name: name.into(),
                prompt: prompt.into(),
                model: "demo-model".into(),
                tools_enabled: false,
                synchronous,
                reuse_conversation: reuse,
                reuse_rounds: 5,
                // A hash of nothing anyone can fire: the fixture demonstrates
                // the UI, and a guessable trigger secret would be a bad habit
                // to ship even in an example.
                secret_hash: format!("dev-ui-unfireable-{index}"),
            },
        )
        .await?;
        let mut reused: Option<String> = None;
        for fire in 0..fires {
            let session = match &reused {
                Some(id) => id.clone(),
                None => {
                    let s = chatdb::create_session(&state.db, "dev").await?;
                    chatdb::set_session_title(&state.db, &s.id, name).await?;
                    if reuse {
                        reused = Some(s.id.clone());
                    }
                    s.id
                }
            };
            let user_turn = uuid::Uuid::new_v4().to_string();
            chatdb::create_user_turn(&state.db, &session, &user_turn, prompt).await?;
            let turn = uuid::Uuid::new_v4().to_string();
            chatdb::create_assistant_turn_in_progress(&state.db, &session, &turn, "demo-model")
                .await?;
            chatdb::append_content(&state.db, &turn, "Here is the summary for this fire.").await?;
            chatdb::finalize_turn(&state.db, &turn, TurnStatus::Completed, None).await?;

            // The tracker's last fire failed, so the page has a failure to
            // render as well as successes.
            let failed = reuse && fire + 1 == fires;
            let (status, error) = if failed {
                ("error", Some("upstream returned 503"))
            } else {
                ("ok", None)
            };
            let run = webhooks::record_run_start(
                &state.db,
                &hook.id,
                &session,
                prompt,
                r#"{"ref":"refs/heads/main"}"#,
                "fire",
            )
            .await?;
            webhooks::finish_run(&state.db, &run, status, error).await?;
            webhooks::mark_fired(&state.db, &hook.id, status, Some(&session), error).await?;
            // The stored payload is what a rerun replays, so it only exists
            // once something has actually fired — a never-fired hook must not
            // offer a rerun link.
            webhooks::set_last_payload(&state.db, &hook.id, r#"{"ref":"refs/heads/main"}"#).await?;
            // Seeded in a tight loop; space the fires out so the history page
            // does not look like one instant.
            let fired_at = (jiff::Timestamp::now()
                - jiff::SignedDuration::from_hours(24 * (fires - fire - 1)))
            .to_string();
            sqlx::query("UPDATE webhook_runs SET fired_at = ? WHERE id = ?")
                .bind(&fired_at)
                .bind(&run)
                .execute(&state.db)
                .await?;
        }
    }

    // --- RAG collections (indexed → "ready", with a resolved commit) -----
    let collections = [
        (
            "acme-docs",
            "Product documentation for the Acme platform",
            "https://github.com/acme/docs.git",
            "main",
            "a1b2c3d",
        ),
        (
            "acme-api",
            "Backend API service — handlers, models, and OpenAPI specs",
            "https://github.com/acme/api.git",
            "release-2.4",
            "9f4e210",
        ),
    ];
    for (name, desc, git_url, git_ref, commit) in collections {
        let c = rag::create_collection(
            &state.db,
            &rag::NewCollection {
                name: name.into(),
                description: Some(desc.into()),
                git_url: git_url.into(),
                git_ref: git_ref.into(),
                pat: None,
                source: Default::default(),
                profile_id: None,
                extraction_model: None,
                embedding_model: "demo-embed".into(),
                include_globs: vec!["**/*.md".into(), "**/*.rs".into()],
                exclude_globs: vec!["target/**".into(), "node_modules/**".into()],
                chunk_size: 800,
                chunk_overlap: 100,
                search_mode: rag::SearchMode::Versioned,
                refresh_interval_mins: 0,
            },
        )
        .await?;
        let r = rag::add_ref(&state.db, c.id, git_ref, None, true).await?;
        rag::set_ref_status(&state.db, r.id, rag::CollectionStatus::Indexing).await?;
        rag::swap_ref_index(
            &state.db,
            r.id,
            &uuid::Uuid::new_v4().to_string(),
            commit,
            "ocr=false,office=false",
        )
        .await?;
    }

    // --- MCP connector catalog (for the /admin/connectors + /integrations
    // screenshots). Seed the built-in set, give the deployment-specific
    // connectors generic example.com URLs + a demo client id, then enable them
    // so both the admin store and the user connect surface render populated.
    // No real endpoints, credentials, or connections — nothing user-specific.
    use aiplane_core::server::db::mcp_catalog;
    mcp_catalog::seed_defaults(&state.db).await?;
    sqlx::query("UPDATE mcp_catalog_connectors SET url = ? WHERE key = ?")
        .bind("https://gworkspace-mcp.example.com/mcp")
        .bind("google_workspace")
        .execute(&state.db)
        .await?;
    sqlx::query("UPDATE mcp_catalog_connectors SET url = ? WHERE key = ?")
        .bind("https://gitlab-mcp.example.com/mcp")
        .bind("gitlab_selfmanaged")
        .execute(&state.db)
        .await?;
    sqlx::query(
        "UPDATE mcp_catalog_connectors SET client_id = 'demo-client-id' WHERE key = 'github'",
    )
    .execute(&state.db)
    .await?;
    for key in [
        "atlassian",
        "github",
        "gitlab",
        "gitlab_selfmanaged",
        "google_workspace",
    ] {
        mcp_catalog::set_enabled(&state.db, key, true).await?;
    }
    // The global, audited Discord connector (shared bot) — configured + enabled
    // and given a few sample tool-call audit rows so /admin/connectors shows the
    // Global/Audited badges + "Audit log" button and the audit page renders
    // populated. Generic data only.
    sqlx::query("UPDATE mcp_catalog_connectors SET url = ? WHERE key = 'discord'")
        .bind("http://discord-mcp:8085/mcp")
        .execute(&state.db)
        .await?;
    mcp_catalog::set_enabled(&state.db, "discord", true).await?;
    {
        use aiplane_core::server::db::mcp_audit;
        let dev_principal = aiplane_core::server::principal::Principal::User {
            id: "dev".into(),
            roles: vec![],
        };
        mcp_audit::record(
            &state.db,
            &dev_principal,
            "discord",
            "mcp__discord__send_private_message",
            Some(
                r#"{"userId":"826733236931526666","message":"Standup reminder in 10 minutes 🕙"}"#,
            ),
            "ok",
            None,
            Some("chat-a1"),
            None,
        )
        .await?;
        mcp_audit::record(
            &state.db,
            &dev_principal,
            "discord",
            "mcp__discord__send_message",
            Some(r#"{"channelId":"826729434073530409","message":"Deploy v1.4.2 finished ✅"}"#),
            "ok",
            None,
            Some("chat-a1"),
            None,
        )
        .await?;
        mcp_audit::record(
            &state.db,
            &dev_principal,
            "discord",
            "mcp__discord__create_webhook",
            Some(r#"{"channelId":"826729434073530409","name":"ci-bot"}"#),
            "error",
            Some("Missing permission: MANAGE_WEBHOOKS"),
            Some("chat-b2"),
            None,
        )
        .await?;
    }

    // --- API tokens (for the /settings/tokens screenshot) — a mix of active (one
    // recently used, one never) and a revoked one, all owned by `dev`. The
    // `hash` is a throwaway string: the page only lists tokens, it doesn't
    // authenticate with them. `created_at` is backdated; `touch`/`revoke`
    // stamp last-used / revoked-at to "now".
    use aiplane_core::server::db::tokens;
    use aiplane_core::server::db::users;
    let tnow = Timestamp::now();
    // (name, created N days ago, then: "used" | "unused" | "revoked")
    let seed_tokens = [
        ("Production API", 42i64, "used"),
        ("CI pipeline", 15i64, "revoked"),
        ("Local laptop", 4i64, "unused"),
    ];
    let mut token_ids: Vec<(String, String)> = Vec::new();
    for (name, age_days, state_kind) in seed_tokens {
        let id = uuid::Uuid::new_v4().to_string();
        token_ids.push((name.to_string(), id.clone()));
        tokens::insert(
            &state.db,
            &tokens::Token {
                id: id.clone(),
                user_id: "dev".into(),
                name: name.into(),
                hash: format!("seed-hash-{id}"),
                created_at: tnow - (age_days * 24).hours(),
                last_used_at: None,
                expires_at: tnow + (90i64 * 24).hours(),
                revoked_at: None,
                tools_enabled: true,
            },
        )
        .await?;
        match state_kind {
            "used" => tokens::touch(&state.db, &id).await?,
            "revoked" => {
                tokens::revoke(&state.db, "dev", &id).await?;
            }
            _ => {}
        }
    }

    // Per-token scope + quota, so the /settings/tokens panels and the /admin/tokens
    // register show a configured token rather than three default ones.
    if let Some((_, prod_id)) = token_ids.iter().find(|(n, _)| n == "Production API") {
        // The owner limits their own token to the two chat models.
        aiplane_core::server::db::token_models::set_for_token(
            &state.db,
            prod_id,
            &["demo-model".into(), "demo-model-pro".into()],
            limits::ManagedBy::Owner,
        )
        .await?;
        // …and the operator caps what that token may spend.
        limits::upsert(
            &state.db,
            SubjectType::Token,
            prod_id,
            None,
            Dimension::Cost,
            Window::Month,
            25.0,
        )
        .await?;
    }
    if let Some((_, ci_id)) = token_ids.iter().find(|(n, _)| n == "CI pipeline") {
        // An operator-set restriction, to show the two-list shape.
        aiplane_core::server::db::token_models::set_for_token(
            &state.db,
            ci_id,
            &["demo-model".into()],
            limits::ManagedBy::Admin,
        )
        .await?;
    }

    // --- Additional users (for /admin/users) with distinct OIDC groups so the
    // resolved gateway-role column varies (via the RBAC mappings in `main`).
    // Impersonation is enabled in config, so the action column populates.
    let unow = Timestamp::now();
    let seed_users = [
        (
            "u-anna",
            "anna.schmidt@example.com",
            "Anna Schmidt",
            vec!["engineering", "platform-admins"],
        ),
        (
            "u-ben",
            "ben.carter@example.com",
            "Ben Carter",
            vec!["engineering"],
        ),
        (
            "u-clara",
            "clara.novak@example.com",
            "Clara Novak",
            vec!["finance"],
        ),
        (
            "u-david",
            "david.kim@example.com",
            "David Kim",
            vec!["support"],
        ),
    ];
    for (id, email, name, groups) in seed_users {
        users::upsert(
            &state.db,
            &users::User {
                id: id.into(),
                email: email.into(),
                name: Some(name.into()),
                roles: groups.into_iter().map(String::from).collect(),
                created_at: unow,
                updated_at: unow,
                timezone: None,
                speech_voice: None,
            },
        )
        .await?;
    }

    // --- Usage events (for /usage). The page defaults to "Today" in the
    // viewer's timezone (dev = UTC), so we seed several of today's rows plus a
    // week of history across a couple of models, sources, and kinds — with a
    // few 4xx/5xx so the "errors" stat is non-zero. `insert_batch` writes both
    // the raw event and the daily rollup, so any period renders.
    use aiplane_core::server::db::usage as usage_db;
    use usage_db::{UsageKind, UsageRecord, UsageSource};
    let mut usage_rows: Vec<UsageRecord> = Vec::new();
    // Attribute the API traffic to the seeded token, so the per-token
    // breakdown on /usage and the spend line on /settings/tokens have real data.
    let prod_token_id: Option<String> = token_ids
        .iter()
        .find(|(n, _)| n == "Production API")
        .map(|(_, id)| id.clone());
    // (hours-ago, source, kind, model, status, prompt, completion)
    let events: &[(i64, UsageSource, UsageKind, &str, u16, i64, i64)] = &[
        (
            1,
            UsageSource::Chat,
            UsageKind::Chat,
            "demo-model",
            200,
            820,
            240,
        ),
        (
            2,
            UsageSource::Chat,
            UsageKind::Chat,
            "demo-model-pro",
            200,
            1450,
            610,
        ),
        (
            3,
            UsageSource::V1Api,
            UsageKind::Chat,
            "demo-model",
            200,
            320,
            110,
        ),
        (
            4,
            UsageSource::Chat,
            UsageKind::Image,
            "demo-image",
            200,
            0,
            0,
        ),
        (
            5,
            UsageSource::V1Api,
            UsageKind::Chat,
            "demo-model-pro",
            429,
            0,
            0,
        ),
        (
            7,
            UsageSource::Scheduled,
            UsageKind::Chat,
            "demo-model",
            200,
            2100,
            540,
        ),
        (
            9,
            UsageSource::Chat,
            UsageKind::Transcription,
            "demo-whisper",
            200,
            0,
            0,
        ),
        (
            26,
            UsageSource::Chat,
            UsageKind::Chat,
            "demo-model",
            200,
            640,
            180,
        ),
        (
            28,
            UsageSource::V1Api,
            UsageKind::Chat,
            "demo-model",
            500,
            0,
            0,
        ),
        (
            50,
            UsageSource::Chat,
            UsageKind::Chat,
            "demo-model-pro",
            200,
            1720,
            690,
        ),
        (
            74,
            UsageSource::Scheduled,
            UsageKind::Chat,
            "demo-model",
            200,
            1980,
            500,
        ),
        (
            99,
            UsageSource::Chat,
            UsageKind::Speech,
            "demo-tts",
            200,
            0,
            0,
        ),
        (
            122,
            UsageSource::V1Api,
            UsageKind::Chat,
            "demo-model",
            200,
            410,
            150,
        ),
        (
            146,
            UsageSource::Chat,
            UsageKind::Chat,
            "demo-model-pro",
            200,
            1300,
            520,
        ),
    ];
    let unow2 = Timestamp::now();
    for (h, source, kind, model, status, prompt, completion) in events.iter().copied() {
        let backend = match kind {
            UsageKind::Transcription | UsageKind::Speech => "wiremock-voice",
            UsageKind::Image => "wiremock-image",
            _ => "wiremock-chat",
        };
        usage_rows.push(UsageRecord {
            created_at: unow2 - h.hours(),
            user_id: "dev".into(),
            user_email: Some("dev@example.com".into()),
            token_id: (source == UsageSource::V1Api)
                .then(|| prod_token_id.clone())
                .flatten(),
            token_name: (source == UsageSource::V1Api).then(|| "Production API".to_string()),
            source,
            kind,
            backend: backend.into(),
            model: model.into(),
            status,
            duration_ms: 400 + (h % 7) * 130,
            prompt_tokens: (prompt > 0).then_some(prompt),
            completion_tokens: (completion > 0).then_some(completion),
            total_tokens: (prompt + completion > 0).then_some(prompt + completion),
            input_units: None,
            output_units: None,
            enforce_limits: true,
            principal_kind: aiplane_core::server::principal::PrincipalKind::User,
            agent_id: None,
            chain: None,
            stop_reason: None,
        });
    }
    // A couple of rows from other users so the admin "All users" view has more
    // than one row.
    for (uid, email, model, h) in [
        ("u-anna", "anna.schmidt@example.com", "demo-model-pro", 6i64),
        ("u-ben", "ben.carter@example.com", "demo-model", 20i64),
    ] {
        usage_rows.push(UsageRecord {
            created_at: unow2 - h.hours(),
            user_id: uid.into(),
            user_email: Some(email.into()),
            token_id: None,
            token_name: None,
            source: UsageSource::Chat,
            kind: UsageKind::Chat,
            backend: "wiremock-chat".into(),
            model: model.into(),
            status: 200,
            duration_ms: 620,
            prompt_tokens: Some(900),
            completion_tokens: Some(280),
            total_tokens: Some(1180),
            input_units: None,
            output_units: None,
            enforce_limits: true,
            principal_kind: aiplane_core::server::principal::PrincipalKind::User,
            agent_id: None,
            chain: None,
            stop_reason: None,
        });
    }
    // Price the two demo chat models BEFORE inserting usage, so the batched
    // writer computes a real `cost` on each seeded row (→ the /usage cost
    // column + the cost limit bar show non-zero spend).
    use aiplane_core::server::db::model_defaults;
    model_defaults::set_pricing(&state.db, "demo-model", Some(0.5), Some(1.5)).await?;
    model_defaults::set_pricing(&state.db, "demo-model-pro", Some(3.0), Some(15.0)).await?;

    usage_db::insert_batch(&state.db, &usage_rows).await?;

    // A spread of demo limit rules so /admin/limits shows a populated table and
    // the /usage "Your limits" bars render (global rules apply to `dev`).
    use aiplane_core::server::db::limits::{self, Dimension, SubjectType, Window};
    limits::upsert(
        &state.db,
        SubjectType::Global,
        "",
        None,
        Dimension::Requests,
        Window::Day,
        5_000.0,
    )
    .await?;
    limits::upsert(
        &state.db,
        SubjectType::Global,
        "",
        None,
        Dimension::Tokens,
        Window::Week,
        5_000_000.0,
    )
    .await?;
    limits::upsert(
        &state.db,
        SubjectType::Global,
        "",
        None,
        Dimension::Cost,
        Window::Month,
        50.0,
    )
    .await?;
    limits::upsert(
        &state.db,
        SubjectType::Global,
        "",
        Some("demo-model-pro"),
        Dimension::Tokens,
        Window::Week,
        1_000_000.0,
    )
    .await?;
    limits::upsert(
        &state.db,
        SubjectType::Role,
        "engineering",
        None,
        Dimension::Requests,
        Window::Hour,
        600.0,
    )
    .await?;

    Ok(())
}
