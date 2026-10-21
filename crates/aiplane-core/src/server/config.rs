// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Runtime configuration.
//!
//! **There is no config file.** Pools, backends, models, groups, the OIDC
//! provider and every operator setting live in the database and are edited in
//! the web UI ([`crate::server::settings`] applies them over these defaults on
//! boot). What is left here is decided by the environment, because it is a
//! property of *where the process runs* rather than something AIplane can read
//! out of a database it has not opened yet: the bind address, the data
//! directory, the database path, the public URL.
//!

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;

use thiserror::Error;

use crate::server::upstreams::config::FallbackConfig;

#[derive(Debug, Error)]
pub enum ConfigError {
    /// A variable the process reads at boot holds a value it cannot use, and
    /// guessing would be worse than stopping.
    #[error("{0}")]
    Invalid(String),
}

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub gateway: GatewayConfig,
    pub content_guard: ContentGuardConfig,
    /// Chat-page knobs that aren't routing-related — attachment
    /// storage + which model names are allowed to receive image
    /// content. Optional; defaults are conservative (S3 disabled,
    /// no vision models advertised so attachments error if S3
    /// isn't configured anyway).
    pub chat: ChatConfig,
    /// Typst-templated document generation. Optional — when unset,
    /// no `typst_*` tools register and the gateway boots fine.
    /// When set, the directory is scanned at startup for subdirs
    /// containing a `template.toml` manifest; each becomes one tool
    /// the model can call to produce a rendered PDF + PNG + .typ
    /// source from corporate-design templates.
    pub typst: Option<TypstConfig>,
    /// GeoIP (client-IP → coarse location) for the `get_user_location`
    /// tool. Optional — when unset no database is loaded and
    /// the tool falls back to the browser-provided position (or reports
    /// the location as unknown). See `server::geoip`.
    pub geoip: Option<GeoipConfig>,
    /// RAG indexer state directory + tuning. Optional — when unset
    /// the indexer uses `data/rag` under [`data_dir`]; the container image
    /// points `$AIPLANE_DATA_DIR` at its writable volume.
    pub rag: Option<RagConfig>,
    /// Agent Skills the gateway makes available to the chat model.
    /// Optional — when unset none are loaded and the
    /// `read_skill` tool is not registered. When set, `dir` is scanned at
    /// startup for skill bundles (a directory holding a `SKILL.md`, or a
    /// `*.skill` zip of one); each becomes an operator-managed capability
    /// the model can load on demand. RBAC-gated per role via the role's
    /// `skills` list, exactly like `tools`. See `server::skills`.
    pub skills: Option<SkillsConfig>,
    /// Usage accounting (per-user / per-backend request metrics). Always
    /// present with sane defaults — there's no way to mis-configure it into
    /// failing a request, since recording is fire-and-forget. Turn
    /// `enabled` off to stop measurement entirely, or tune
    /// `retention_days` to bound the raw-event window. See `server::usage`.
    pub usage: UsageConfig,
    /// Rate limits & quotas. Always present; the only knob is a kill switch
    /// (`enabled`, default true). Limits themselves are data — set
    /// per global/role/user in `/admin/limits` — so
    /// with no rules configured every caller is unlimited. See
    /// `server::limits`.
    pub limits: LimitsConfig,
    /// Active personal-Dot capacity, controlled from `/admin/settings`.
    pub dots: DotsConfig,
    /// Code-execution sandbox. Optional — when unset the
    /// `run_in_sandbox` tool family is not registered and the gateway
    /// boots fine. When set, `runner_url` points at the standalone
    /// `sandbox-runner` service, which holds podman access and executes
    /// untrusted/LLM code in single-use sandboxes; the gateway only
    /// talks HTTP to it. See `server::tools::sandbox`.
    pub sandbox: Option<SandboxConfig>,
    /// Headless ComfyUI worker for image / video / audio workflows.
    /// Optional — when unset no `comfyui_*` tools register
    /// and the gateway boots fine. When set, `base_url` points at an
    /// internal ComfyUI instance and `content_dir` holds the curated
    /// workflow catalog (workflow.json + manifest.toml per subdirectory).
    /// See `docs/comfyui.md` and `server::comfyui`.
    pub comfyui: Option<ComfyuiConfig>,
    /// Feedback widget. Optional — when unset the floating
    /// feedback button stays hidden (the `/feedback/config` endpoint reports
    /// it unconfigured, so the client never reveals the FAB). When set,
    /// every signed-in user gets a floating button that opens a form — with
    /// optional voice-to-fields dictation — and files the submission as a
    /// GitHub *or* GitLab issue, whichever [`FeedbackConfig::provider`]
    /// names. See `server::issue_tracker` + `rama_server::pages::feedback`.
    pub feedback: Option<FeedbackConfig>,
    /// Unknown-model fallback, per request kind. When a request names a model
    /// that no backend serves and that isn't an alias, the router substitutes
    /// the model named here for that kind instead of returning `404`. Optional;
    /// an unset kind keeps today's `model_not_found` behaviour. See
    /// [`FallbackConfig`] and `docs/upstreams.md`. Note the *offline* fallback
    /// (a known model whose replicas are all down) lives per-pool as
    /// `fallback_offline`, not here.
    pub fallback: FallbackConfig,
    /// Web Push notifications ("your turn finished"). Defaults are
    /// enabled with a placeholder contact: a VAPID keypair is generated on
    /// first boot and nothing is sent until a user opts in from the browser.
    /// Turn `enabled` off to disable the feature (and its endpoints).
    pub push: PushConfig,
    /// The Prometheus scrape endpoint, `GET /metrics`. Set only at
    /// `/admin/settings`.
    pub metrics: MetricsConfig,
    /// Where the gateway may connect on behalf of a user, a model or an
    /// agent's owner, set from the environment by [`Config::load`]; the
    /// default is the safe one. See `outbound_guard`.
    pub network: NetworkConfig,
}

/// What a URL someone other than the operator chose may reach.
#[derive(Debug, Clone, Default)]
pub struct NetworkConfig {
    /// Let a URL a user, a model or an agent's owner chooses — `fetch_url`,
    /// `load_image_url`, `tls_cert`, an A2A route (agent card, endpoint,
    /// OAuth token URL), a `host_jwt` verifier's JWKS URL — reach a host
    /// that resolves to a private, loopback or carrier-grade NAT address
    /// (and, for the agent destinations, plain `http`). Off by default: they
    /// then reach public hosts only, so nobody but the operator can point
    /// the gateway at its own network. Set by
    /// `$AIPLANE_ALLOW_PRIVATE_NETWORKS`. Link-local (including the cloud
    /// metadata address), unspecified, broadcast and multicast addresses
    /// stay refused either way.
    pub allow_private_networks: bool,
}

#[derive(Debug, Clone)]
pub struct ContentGuardConfig {
    pub enabled: bool,
    pub model: String,
    pub mode: ContentGuardMode,
    pub gdpr_action: ContentGuardPolicy,
    pub nda_action: ContentGuardPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ContentGuardMode {
    #[default]
    Monitor,
    Enforce,
}

impl ContentGuardMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Monitor => "monitor",
            Self::Enforce => "enforce",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ContentGuardPolicy {
    Allow,
    Confirm,
    #[default]
    Deny,
}

impl ContentGuardPolicy {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Confirm => "confirm",
            Self::Deny => "deny",
        }
    }

    pub fn parse(raw: &str) -> Self {
        match raw.trim() {
            "allow" => Self::Allow,
            "confirm" => Self::Confirm,
            "deny" => Self::Deny,
            _ => Self::Deny,
        }
    }
}

impl ContentGuardMode {
    pub fn parse(raw: &str) -> Self {
        match raw.trim() {
            "enforce" => Self::Enforce,
            "monitor" => Self::Monitor,
            _ => Self::Monitor,
        }
    }
}

impl Default for ContentGuardConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model: String::new(),
            mode: ContentGuardMode::Monitor,
            gdpr_action: ContentGuardPolicy::Deny,
            nda_action: ContentGuardPolicy::Deny,
        }
    }
}

/// Web Push settings. Everything needed to send is self-generated (the VAPID
/// keypair persists in the DB); the only operator-facing knob besides the
/// master switch is the VAPID `contact`.
#[derive(Debug, Clone)]
pub struct PushConfig {
    /// Master switch. `true` (default) serves the push endpoints and fires
    /// turn-complete notifications to opted-in browsers.
    pub enabled: bool,
    /// The VAPID `sub` claim: a `mailto:` or `https:` URI the push service can
    /// use to contact the operator about this application server. Not verified
    /// for reachability, but must be a well-formed URI. Override with a real
    /// address for production.
    pub contact: String,
}

impl Default for PushConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            contact: "mailto:admin@example.com".to_string(),
        }
    }
}

/// Which issue tracker a submission is filed in. One deployment files
/// everywhere at once would be a surprise, so this is a single choice rather
/// than "whatever happens to be configured": an operator who fills in GitLab
/// while a stale GitHub token is still in the database gets the tracker they
/// selected, not a coin flip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FeedbackProvider {
    #[default]
    Github,
    Gitlab,
}

impl FeedbackProvider {
    /// Parse the stored setting. Anything unrecognised (including an empty
    /// row on a database that predates the setting) is GitHub, which is what
    /// every deployment configured before GitLab existed was using.
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "gitlab" => Self::Gitlab,
            _ => Self::Github,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Github => "github",
            Self::Gitlab => "gitlab",
        }
    }
}

/// Feedback-widget settings: where issues are filed and how the voice
/// transcript is turned into structured fields.
///
/// Both trackers are described here in one struct rather than in a tagged
/// enum: an operator evaluating a move from one to the other keeps both sets
/// of credentials around, and `provider` decides which half is live.
#[derive(Debug, Clone)]
pub struct FeedbackConfig {
    /// `github` (default) or `gitlab` — which tracker receives the issues.
    pub provider: String,
    /// GitHub repository owner (user or org) that receives the issues,
    /// e.g. `croit`.
    pub github_owner: String,
    /// GitHub repository name, e.g. `aiplane`.
    pub github_repo: String,
    /// GitHub API token (classic PAT or fine-grained) able to open issues
    /// (`issues:write`) and — when screenshots are attached — commit asset
    /// files (`contents:write`). Sealed at rest.
    pub github_token: Option<String>,
    /// Labels applied to every created issue, in addition to the automatic
    /// `priority:<p>`. Default `["feedback"]`.
    pub labels: Vec<String>,
    /// Orphan branch holding embedded screenshot assets: each screenshot is
    /// committed as a file and linked into the issue body via its raw URL.
    /// Created off the default branch on first use if missing. Default
    /// `feedback-assets`.
    pub assets_branch: String,
    /// Chat model id used to turn a voice transcript into the structured
    /// form fields ("text model"). Unset/empty → the gateway picks the first
    /// available chat model at request time. This is an operator choice, not
    /// the end user's — the form never exposes a model picker.
    pub extraction_model: Option<String>,
    /// Transcription model id used to turn the voice recording into text
    /// ("voice model"). Unset/empty → the gateway picks the first available
    /// transcription model at request time. Operator choice, not the user's.
    pub voice_model: Option<String>,
    /// GitHub REST API base URL. Default `https://api.github.com`; override
    /// for GitHub Enterprise (`https://github.example.com/api/v3`).
    pub github_api_base: String,
    /// GitLab instance base URL — the web origin, NOT the `/api/v4` path
    /// (which the client appends). Default `https://gitlab.com`; a
    /// self-managed instance is `https://gitlab.example.com`.
    pub gitlab_url: String,
    /// GitLab project the issues land in: either the numeric project id
    /// (`1234`) or the full path (`group/subgroup/project`), which the client
    /// URL-encodes. Both are what the GitLab REST API calls `:id`.
    pub gitlab_project_id: String,
    /// GitLab personal/project/group access token with the `api` scope (it
    /// needs to both create issues and POST to the project's `uploads`
    /// endpoint for the screenshot). Sealed at rest.
    pub gitlab_token: Option<String>,
}

impl FeedbackConfig {
    /// The selected tracker.
    pub fn provider(&self) -> FeedbackProvider {
        FeedbackProvider::parse(&self.provider)
    }

    /// The GitHub token; an empty string counts as unset.
    pub fn github_token(&self) -> Option<String> {
        non_empty(self.github_token.as_deref())
    }

    /// The GitLab token; an empty string counts as unset.
    pub fn gitlab_token(&self) -> Option<String> {
        non_empty(self.gitlab_token.as_deref())
    }

    /// True when enough is configured for the *selected* provider to actually
    /// open an issue. Deliberately not "either provider is complete": a half
    /// -filled GitLab block must keep the widget hidden rather than silently
    /// filing to the GitHub repo the operator is migrating away from.
    pub fn is_configured(&self) -> bool {
        match self.provider() {
            FeedbackProvider::Github => {
                !self.github_owner.is_empty()
                    && !self.github_repo.is_empty()
                    && self.github_token().is_some()
            }
            FeedbackProvider::Gitlab => {
                !self.gitlab_url.trim().is_empty()
                    && !self.gitlab_project_id.trim().is_empty()
                    && self.gitlab_token().is_some()
            }
        }
    }
}

/// Sandbox tool settings. The heavy lifting (isolation, warm pool, egress
/// allowlist) lives in the separate `sandbox-runner` service; the gateway
/// just needs to know where it is and how patient to be.
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    /// Master switch. `true` (default) registers the sandbox tools; set
    /// `false` to turn the whole feature off while keeping `runner_url`. Per-tool and
    /// per-user/-token control is separate (RBAC + the `/tools` toggles).
    pub enabled: bool,
    /// Base URL of the sandbox-runner service, e.g.
    /// `http://sandbox-runner:9000`. MUST be reachable only from the
    /// gateway (internal network / mTLS) — it executes arbitrary code.
    pub runner_url: String,
    /// HTTP timeout for a single `/run` call. Should exceed the runner's
    /// own per-job timeout plus sandbox cold-start margin.
    pub timeout_secs: u64,
    /// Largest single produced file the gateway will accept back from a
    /// run and store. Larger artifacts are dropped with a note in the
    /// tool result rather than bloating storage / the model context.
    pub max_artifact_bytes: u64,
}

/// Headless ComfyUI worker settings. The worker owns GPU + model files;
/// the gateway owns the curated workflow catalog and the typed tool
/// surface the model sees. See `docs/comfyui.md`.
#[derive(Debug, Clone)]
pub struct ComfyuiConfig {
    /// Master switch. `true` (default) registers the `comfyui_*` tools;
    /// `false` disables registration while keeping the other settings.
    pub enabled: bool,
    /// Base URL of the ComfyUI instance, e.g. `http://comfyui-worker:8188`.
    /// MUST be reachable only from the gateway — ComfyUI has no auth and
    /// executes arbitrary workflows. Operators front it with an internal
    /// network / mTLS / an IP allowlist.
    pub base_url: String,
    /// Root directory holding one subdirectory per workflow. Each subdirectory
    /// must contain `manifest.toml` (tool surface) + `workflow.json` (ComfyUI
    /// prompt-API document). The directory is **not** part of the public
    /// repository — operators back it up out of band. See `docs/comfyui.md`.
    pub content_dir: PathBuf,
    /// Per-workflow execution timeout. Diffusion runs are slow; this is the
    /// upper bound on how long the gateway will poll ComfyUI's `/history`
    /// endpoint before giving up and returning a timeout error to the tool.
    pub timeout_secs: u64,
    /// How often to poll ComfyUI's `/history/{prompt_id}` while waiting for
    /// a workflow to finish. Lower = snappier feedback, higher = less load
    /// on ComfyUI's history API. Default 500 ms.
    pub queue_poll_interval_ms: u64,
    /// Max workflows the gateway will let the model dispatch concurrently.
    /// Default 1 — a single 24 GB GPU realistically runs one diffusion job
    /// at a time. ComfyUI itself queues internally, so raising this mainly
    /// affects how many gateway-side slots are reserved.
    pub max_concurrent_jobs: usize,
}

/// Usage-metrics knobs. Recording is decoupled from the request path (a
/// bounded channel drained by a background batched writer), so these only
/// affect how much history is kept and whether measurement runs at all.
#[derive(Debug, Clone)]
pub struct UsageConfig {
    /// Master switch. When `false`, no `UsageRecord`s are emitted, the
    /// writer/maintenance tasks aren't spawned, and hot paths skip the
    /// record-building work entirely — a production kill switch if metrics
    /// ever cost too much. The `/usage` page still renders (with a "metrics
    /// disabled" notice). Default `true`.
    pub enabled: bool,
    /// How many days of raw `usage_events` rows to keep. Older rows are
    /// pruned hourly; the `usage_daily` rollups are kept forever regardless.
    /// Must comfortably exceed the longest UI period ("start of last month",
    /// ~62 days back) so those queries stay on the precise raw path. Default
    /// 90.
    pub retention_days: i64,
    /// Display currency for cost accounting — a short label (ISO code or
    /// symbol) shown next to spend figures on `/usage` and `/admin/models`.
    /// Purely cosmetic: per-model prices (set in `/admin/models`) are plain
    /// numbers per 1M tokens and are assumed to share this one currency.
    /// Default `"USD"`.
    pub currency: String,
}

impl Default for UsageConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            retention_days: 90,
            currency: "USD".to_string(),
        }
    }
}

/// Who may scrape `GET /metrics`. The endpoint answers only when it is
/// enabled *and* at least one guard is set; with both set, a scrape must pass
/// both.
#[derive(Debug, Clone, Default)]
pub struct MetricsConfig {
    pub enabled: bool,
    /// The bearer token a scraper presents, never empty. Sealed at rest.
    pub token: Option<String>,
    pub allowed_ips: AllowedIps,
}

/// The client addresses `GET /metrics` admits, parsed once when the
/// settings are applied.
#[derive(Debug, Clone)]
pub enum AllowedIps {
    /// The entries as stored and the networks they name. No entries is no
    /// address guard.
    Listed {
        entries: Vec<String>,
        networks: crate::server::ip_networks::IpNetworks,
    },
    /// The stored value is not a list of addresses and networks — only a
    /// hand-edited row can be. It admits nobody until it is saved again, so
    /// a guard the gateway cannot read never turns into no guard.
    Unreadable { stored: String },
}

impl Default for AllowedIps {
    fn default() -> Self {
        Self::Listed {
            entries: Vec::new(),
            networks: Default::default(),
        }
    }
}

impl AllowedIps {
    /// Whether an address guard is configured, readable or not.
    pub fn is_set(&self) -> bool {
        match self {
            Self::Listed { entries, .. } => !entries.is_empty(),
            Self::Unreadable { .. } => true,
        }
    }

    /// Whether `ip` may scrape. Assumes [`Self::is_set`].
    pub fn admits(&self, ip: std::net::IpAddr) -> bool {
        match self {
            Self::Listed { networks, .. } => networks.contains(ip),
            Self::Unreadable { .. } => false,
        }
    }
}

/// Rate-limit / quota enforcement. Limits themselves live in the DB (set via
/// `/admin/limits`); this block only carries the deployment-wide kill switch.
#[derive(Debug, Clone)]
pub struct LimitsConfig {
    /// Master switch. When `false`, the [`crate::server::limits::Enforcer`]
    /// never blocks (and skips its per-request read) regardless of configured
    /// rules — a production escape hatch. Default `true`. Note that with no
    /// rules configured everyone is unlimited anyway, so leaving this on costs
    /// nothing until you set a limit.
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct DotsConfig {
    pub max_active_per_user: u32,
    pub max_active_per_organization: u32,
}

impl Default for DotsConfig {
    fn default() -> Self {
        Self {
            max_active_per_user: 4,
            max_active_per_organization: 4,
        }
    }
}

impl Default for LimitsConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// Skills directory: a single operator-owned folder scanned once at startup. No hot-reload —
/// restart to pick up new or changed skills.
#[derive(Debug, Clone)]
pub struct SkillsConfig {
    /// Root holding one skill per entry. An entry is either a directory
    /// containing a `SKILL.md` (optionally nested one level, e.g. the
    /// folder a `*.skill` archive unzips to) or a `*.skill` file (a zip of
    /// such a directory), which is extracted into `<dir>/.cache/` at
    /// startup. The gateway only reads this directory; operators populate
    /// it out-of-band (drop a bundle in and restart), just like Typst
    /// templates. Default is `data/skills` relative to the gateway's
    /// working directory.
    pub dir: PathBuf,
}

impl Default for SkillsConfig {
    fn default() -> Self {
        Self {
            dir: default_skills_dir(),
        }
    }
}

fn default_skills_dir() -> PathBuf {
    PathBuf::from("data/skills")
}

/// RAG indexer state directory + tuning knobs.
#[derive(Debug, Clone)]
pub struct RagConfig {
    /// Root for all per-collection RAG storage. Each collection gets its
    /// own self-contained folder `<data_dir>/<uuid>/` holding `rag.sqlite`
    /// (chunk text + FTS index), `index.usearch` (vectors), and `clone/`
    /// (the git working tree). This is the only heavy / regenerable state,
    /// so keep it separate from the precious central `$AIPLANE_DB_PATH` — e.g. on
    /// a larger or cheaper drive/mount. The gateway `mkdir -p`s this on
    /// startup, so the **parent** must already exist + be writable by the
    /// runtime user (uid 1000 in the container image). Default is
    /// `data/rag` under [`data_dir`] — relative to the working directory in
    /// dev, and on the mounted volume inside the container.
    pub data_dir: PathBuf,
    /// How many git clones the indexer runs at once, and how many
    /// collections it indexes in parallel. Raising this lets a bunch of
    /// repos/collections index concurrently instead of head-of-line
    /// blocking behind one slow clone; the cost is more simultaneous
    /// network + embedding load. `0` is treated as `1` (fully serial).
    /// Default `4`.
    pub clone_concurrency: usize,
}

impl Default for RagConfig {
    fn default() -> Self {
        Self {
            data_dir: default_rag_data_dir(),
            clone_concurrency: default_clone_concurrency(),
        }
    }
}

fn default_rag_data_dir() -> PathBuf {
    data_dir().join("data").join("rag")
}

fn default_clone_concurrency() -> usize {
    4
}

/// GeoIP settings. Points at an IP2Location LITE DB11 `.BIN` and,
/// optionally, holds an IP2Location download token for the weekly
/// auto-updater.
#[derive(Debug, Clone)]
pub struct GeoipConfig {
    /// Path to the IP2Location LITE DB11 BIN. A missing file is not an
    /// error — lookups simply return nothing until one appears. Changes
    /// are hot-reloaded (no restart).
    pub db_path: PathBuf,
    /// The IP2Location download token, as entered at `/admin/settings`, where
    /// it is sealed at rest. When set (and non-empty) a weekly background task
    /// refreshes `db_path`. Unset → no auto-update; operators can drop in their
    /// own BIN and it gets picked up by the hot-reload watcher.
    pub update_token: Option<String>,
}

impl GeoipConfig {
    /// The download token; an empty string counts as unset.
    pub fn update_token(&self) -> Option<String> {
        non_empty(self.update_token.as_deref())
    }
}

/// Typst document-rendering settings.
#[derive(Debug, Clone)]
pub struct TypstConfig {
    /// Root directory holding one subdirectory per template. Each
    /// subdir must contain `template.toml` (manifest) + `template.typ`
    /// (source). Co-located assets (logos, fonts) work because the
    /// typst compile is run with `--root` pointing at the subdir.
    pub templates_dir: PathBuf,
}

/// The operator's standing instructions for a person's chat.
///
/// Written over the in-memory config by the settings layer and read by the
/// turn driver, which puts it into the leading system message of every
/// person's chat turn while `enabled`: the operator's own `text` while it is
/// non-empty, the built-in prompt otherwise. An agent run is not affected:
/// its system message comes from its spec.
#[derive(Debug, Clone)]
pub struct PromptConfig {
    pub enabled: bool,
    pub text: String,
}

impl Default for PromptConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            text: String::new(),
        }
    }
}

/// Voice-turn settings.
#[derive(Debug, Clone, Default)]
pub struct VoiceConfig {
    /// The operator's own voice directive, injected into the leading system
    /// message of voice-mode turns. Empty means the built-in directive; the
    /// TTS pipeline only reads what the model writes, so any wording works —
    /// it is the operator's call to keep it suited for speech.
    pub directive: String,
}

/// Chat-page settings. Attachments go to S3 (object storage) so the
/// DB doesn't bloat and the OpenAI API can fetch image URLs directly
/// from the bucket. We don't gate which model gets to receive
/// image content — operators are expected to wire only multi-modal
/// chat models into the gateway, and any capability mismatch
/// surfaces as the upstream's error on send.
#[derive(Debug, Clone, Default)]
pub struct ChatConfig {
    pub s3: Option<S3Config>,
    /// The deployment-wide system prompt for a person's chat.
    pub prompt: PromptConfig,
    /// The operator's standing instructions for voice turns.
    pub voice: VoiceConfig,
    /// Optional automatic OCR enrichment for image/PDF attachments. The OCR
    /// pool must point at the document-aware OCR sidecar.
    pub ocr: OcrConfig,
    /// Automatic conversation compaction — summarise a session's oldest turns
    /// once its replayed context grows past a fraction of the model's window.
    pub compaction: CompactionConfig,
    /// How turns may overlap: parallel conversations per user.
    pub turns: TurnsConfig,
    /// What a chat turn does when its model starts repeating itself.
    pub loops: LoopsConfig,
}

/// Retries for a chat turn whose model collapsed into a loop.
///
/// A loop is cut off either way (`loop_guard`, `repeated_calls`); this decides
/// whether the turn then gives up or tries the call again one effort level
/// lower. Thinking less is what gets a model out of a reasoning loop most
/// often, and a turn that only ever stops leaves the user with nothing.
#[derive(Debug, Clone)]
pub struct LoopsConfig {
    /// How many times a turn is retried after a loop, each time one effort
    /// level lower. 0 gives up on the first loop. Read through
    /// [`LoopsConfig::retries`], which caps it.
    pub retries: usize,
}

impl LoopsConfig {
    /// The most retries a turn may make: each one is a full model call the
    /// user waits for.
    pub const MAX_RETRIES: usize = 5;

    pub fn retries(&self) -> usize {
        self.retries.min(Self::MAX_RETRIES)
    }
}

impl Default for LoopsConfig {
    fn default() -> Self {
        Self { retries: 2 }
    }
}

/// How many turns one user may have in flight at once.
///
/// The gateway ran one turn per *user* for its whole life, which meant a
/// question asked in a second conversation was refused while the first was
/// still thinking — a limit with nothing technical behind it, since the two
/// write to different transcripts. One writer per conversation is the
/// invariant that matters and is not configurable; this is the rest.
///
/// The default stays at 1 so an upgrade changes nothing until an operator
/// decides otherwise. Raising it multiplies what a single user can ask of the
/// upstream backends at once, so it is their call, not ours.
#[derive(Debug, Clone)]
pub struct TurnsConfig {
    /// Conversations one user may stream in at the same time. Values below 1
    /// are read as 1 — a ceiling of zero would refuse every turn.
    pub max_parallel: usize,
}

impl Default for TurnsConfig {
    fn default() -> Self {
        Self { max_parallel: 1 }
    }
}

/// Automatic document OCR. Off by default; even switched on, nothing happens
/// unless a healthy `kind = "ocr"` pool serves a model.
///
/// Every limit here exists because OCR is the most expensive derived artefact
/// the gateway produces — an unbounded scan is N vision-model calls on someone
/// else's page count — so the defaults are deliberately conservative and an
/// operator raises them knowingly.
#[derive(Debug, Clone)]
pub struct OcrConfig {
    pub enabled: bool,
    /// OCR model id. `None` uses whatever the `ocr` pool advertises, which is
    /// the right default for the single-backend case.
    pub model: Option<String>,
    /// Output-token cap per OCR inference call.
    pub max_tokens: usize,
    /// Unlimited-OCR's repetition-control window.
    pub ngram_window: usize,
    /// Largest document accepted for OCR. Bigger uploads are left to the
    /// normal attachment path (the model can still fetch them).
    pub max_bytes: usize,
    /// Page ceiling per document. A longer document is recognised up to this
    /// many pages and reported as partial rather than refused — a truncated
    /// 200-page contract is more useful than nothing.
    pub max_pages: usize,
    /// Rasterisation DPI the sidecar renders PDF pages at. Part of the cache
    /// identity: changing it invalidates cached results.
    pub dpi: u32,
    /// Ceiling on recognised text kept per document. Guards the chat context
    /// (and the cache row) against a pathological OCR run.
    pub max_output_chars: usize,
    /// Wall-clock budget for one document's OCR, including the sidecar's own
    /// rasterisation and per-page inference.
    pub timeout_secs: u64,
    /// How many documents may be OCR'd at once across the whole gateway. The
    /// backend is a GPU service; queueing is better than thrashing it.
    pub max_concurrency: usize,
    /// Auto-mode scan detector: a PDF page carrying at least this many
    /// non-whitespace characters in its text layer counts as "born digital".
    /// A document where fewer than half the pages clear the bar is treated as
    /// scanned and sent to OCR. Character counting (rather than words or any
    /// language-specific signal) keeps this working for every script.
    pub auto_min_text_chars_per_page: usize,
}

impl Default for OcrConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model: None,
            max_tokens: 32_768,
            ngram_window: 1_024,
            max_bytes: 32 * 1024 * 1024,
            max_pages: 64,
            dpi: 300,
            max_output_chars: 400_000,
            timeout_secs: 20 * 60,
            max_concurrency: 2,
            auto_min_text_chars_per_page: 40,
        }
    }
}

/// Tunables for automatic conversation compaction. All optional with sane
/// defaults, so an operator gets compaction out of the box and only touches
/// this to disable it or tune the aggressiveness.
#[derive(Debug, Clone)]
pub struct CompactionConfig {
    /// Master switch. `true` (default) enables the auto-trigger; `false`
    /// leaves every conversation replaying its full history.
    pub enabled: bool,
    /// Fallback context window in tokens for models without a per-model
    /// `context_window` set in `/admin/models`. Default 32768.
    pub default_context_window: i64,
    /// Fraction of the context window (0.0–1.0) at which compaction fires.
    /// Default 0.7 — compact once the replayed prompt reaches 70% of the
    /// window, leaving headroom for the model's own answer.
    pub trigger_ratio: f64,
    /// How many of the most recent turns to always keep verbatim (never fold
    /// into the summary). Default 6 — enough that the immediate back-and-forth
    /// the user is looking at is never lossily summarised.
    pub keep_recent_turns: usize,
    /// Anti-thrash floor: re-compaction runs only when at least this many turns
    /// have aged past the previous cutoff. Default 4 — avoids re-summarising the
    /// whole history for a one-turn gain.
    pub min_turns_to_compact: usize,
    /// Output-token cap for the summariser call. Default 1024.
    pub summary_max_tokens: i64,
    /// The operator's own summariser prompt. Empty means the built-in
    /// prompt; the gateway never parses the summary, so any wording works.
    pub summary_prompt: String,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            default_context_window: 32_768,
            trigger_ratio: 0.7,
            keep_recent_turns: 6,
            min_turns_to_compact: 4,
            summary_max_tokens: 1024,
            summary_prompt: String::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct S3Config {
    /// S3 (or S3-compatible) endpoint. e.g.
    /// `https://s3.amazonaws.com`, `https://minio.local`. The
    /// gateway both uploads to this host AND hands presigned URLs
    /// rooted at this host to the upstream LLM, so it must be
    /// reachable from the LLM's network (not just the gateway's).
    pub endpoint: String,
    /// AWS region; for S3-compatible stores (MinIO/Backblaze)
    /// this is often a placeholder like `us-east-1`.
    pub region: String,
    pub bucket: String,
    /// The credentials, as entered at `/admin/settings`, where they are sealed
    /// at rest.
    pub access_key: Option<String>,
    pub secret_key: Option<String>,
    /// Object-key prefix under which chat attachments live. Default
    /// `chat-attachments`. Useful when the bucket is shared with
    /// other workloads.
    pub key_prefix: String,
}

impl S3Config {
    /// The access key id; an empty string counts as unset.
    pub fn access_key(&self) -> Option<String> {
        non_empty(self.access_key.as_deref())
    }

    /// The secret access key; an empty string counts as unset.
    pub fn secret_key(&self) -> Option<String> {
        non_empty(self.secret_key.as_deref())
    }
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value.filter(|v| !v.is_empty()).map(str::to_owned)
}

/// Knobs the gateway itself needs.
#[derive(Debug, Clone)]
pub struct GatewayConfig {
    pub priority_enabled: bool,
    pub default_priority: crate::server::priority::Priority,
    /// Public URL the gateway is reachable at, e.g. `https://aiplane.example.com`.
    ///
    /// **Never read this directly** — call
    /// [`Config::public_url_fallback`] if you want the fallback, or
    /// `AppState::public_url()` if you want the live value.
    ///
    /// Set from `$AIPLANE_PUBLIC_URL`. Named for what it is rather than for the
    /// value it holds, because `public_url` is exactly the name of the accessor
    /// that returns the *correct* value: with both spelled the same, neither
    /// grep nor the compiler could tell a right read from a wrong one, and a
    /// wrong one silently yields `http://localhost:8080` on every
    /// wizard-configured deployment.
    pub public_url_fallback: String,
    /// How long a freshly minted gateway token is valid for. Default 90 days.
    ///
    /// Edited at `/admin/settings`.
    pub token_ttl_days: i64,
    /// Browser-session idle timeout in days. Default 30. This is a *sliding*
    /// window: every request renews it (see `rama_server::session`), so it's
    /// how long you can stay away before having to sign in again, not how
    /// long a login lasts. Lower it for stricter deployments; the cookie
    /// itself always carries a long `Max-Age` so a browser or laptop restart
    /// alone never logs anyone out.
    pub session_ttl_days: i64,
    /// Absolute cap on a session's lifetime in days, counted from login.
    /// Default 90. Unlike `session_ttl_days` this one does *not* slide: it
    /// forces everyone back through the identity provider periodically,
    /// which is also the only point where OIDC group claims are re-read.
    pub session_absolute_max_days: i64,
    /// Whether admins may impersonate other users from `/admin/users`.
    /// Default `false` (opt-in) — impersonation is a powerful, privileged
    /// capability, so it's off unless explicitly enabled. Once on, the
    /// Impersonate buttons appear and `POST /admin/users/impersonate` works. While disabled
    /// the buttons are hidden and that endpoint returns 403. Stopping an
    /// already-active impersonation (`/impersonate/stop`) always works, so
    /// nobody is trapped mid-session if the flag is flipped at runtime.
    pub allow_impersonation: bool,
    /// How long a request may wait for its pool to come back before the gateway
    /// gives up on it, in seconds. Default 120; `0` restores the old
    /// fail-immediately behaviour.
    ///
    /// When every backend serving a model is down (or every slot is taken), the
    /// gateway holds the request and retries routing until a backend answers its
    /// health probe again — see `upstreams::wait`. Nothing has reached the client
    /// yet, so a short outage becomes a pause rather than a failed turn, which is
    /// what lets an agent client keep its tool loop across an upstream restart.
    ///
    /// Set it well below the client's own request timeout (Claude Code's default
    /// is 10 minutes) — the point is to absorb outages the client would otherwise
    /// see, not to out-wait the client. Past the budget the request still gets a
    /// *retryable* answer (`503`/`529` with `Retry-After`), so the client's
    /// backoff continues where this left off.
    pub upstream_wait_secs: u64,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            public_url_fallback: default_public_url(),
            token_ttl_days: default_token_ttl_days(),
            session_ttl_days: default_session_ttl_days(),
            session_absolute_max_days: default_session_absolute_max_days(),
            allow_impersonation: false,
            upstream_wait_secs: default_upstream_wait_secs(),
            priority_enabled: false,
            default_priority: Default::default(),
        }
    }
}

/// Two minutes: long enough to ride out a vLLM restart or a model swap, short
/// enough to stay far inside any sane client timeout.
fn default_upstream_wait_secs() -> u64 {
    120
}

fn default_public_url() -> String {
    "http://localhost:8080".into()
}

fn default_token_ttl_days() -> i64 {
    90
}

/// Mirrors `session::DEFAULT_TTL` — 30 days of inactivity.
fn default_session_ttl_days() -> i64 {
    30
}

/// Mirrors `session::DEFAULT_ABSOLUTE_MAX` — 90 days since login.
fn default_session_absolute_max_days() -> i64 {
    90
}

/// The database filename, under whichever directory [`data_dir`] resolves to.
pub const DB_FILENAME: &str = "gateway.sqlite";

pub fn default_db_path() -> PathBuf {
    data_dir().join(DB_FILENAME)
}

/// Root for everything the gateway must be able to WRITE: the SQLite
/// database, the RAG index store, and whatever persistent state gets added
/// later.
///
/// `$AIPLANE_DATA_DIR` when set, otherwise the process working directory —
/// so a `cargo run` in the repo still writes `./gateway.sqlite` and
/// `./data/rag` exactly as before. The container image points it at the
/// mounted volume, which is what makes a fresh container persist its state
/// without a per-path environment variable each.
///
/// Read-only paths (typst templates, skills bundles) deliberately do NOT
/// hang off this — they ship in the image's read-only layers.
pub fn data_dir() -> PathBuf {
    data_dir_from(std::env::var_os("AIPLANE_DATA_DIR"))
}

/// The pure half of [`data_dir`], split out so it can be tested without
/// mutating process-global environment state (which races every other test
/// in the binary).
fn data_dir_from(raw: Option<std::ffi::OsString>) -> PathBuf {
    raw.filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// Loopback, so a gateway that was told nothing does not expose itself to the
/// network on the strength of a default. The container image overrides it.
pub const DEFAULT_BIND_HOST: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
pub const DEFAULT_BIND_PORT: u16 = 8080;

/// The one variable the session key is read from.
pub const SESSION_KEY_VAR: &str = "AIPLANE_SESSION_KEY";

/// Parse a listen-address component, warning instead of failing.
///
/// A bad value must not be fatal: refusing to boot over a typo in a field with
/// a perfectly good default is worse than using the default, and on a PaaS that
/// injects `$PORT` the operator may not even control the value. Falling back is
/// the recoverable behaviour — but silently, it would leave the gateway
/// listening somewhere nobody asked for.
fn parse_or_warn<T: std::str::FromStr>(raw: &str, what: &str) -> Option<T>
where
    T::Err: std::fmt::Display,
{
    match raw.trim().parse() {
        Ok(v) => Some(v),
        Err(err) => {
            tracing::warn!("ignoring {what}: {raw:?} is not valid ({err})");
            None
        }
    }
}

impl Config {
    /// `$AIPLANE_PUBLIC_URL` — a *fallback*, used only until the setup wizard
    /// records the real one in the database.
    ///
    /// The wizard suggests a URL derived from the request that reached it,
    /// which is right on a directly-exposed gateway and wrong behind a reverse
    /// proxy that rewrites Host. This is how a deployment states the answer up
    /// front instead of correcting it afterwards.
    ///
    /// For the value a request should actually use, call
    /// `AppState::public_url()`.
    pub fn public_url_fallback(&self) -> &str {
        &self.gateway.public_url_fallback
    }

    /// The socket to listen on: `$IP` / `$PORT`, else
    /// [`DEFAULT_BIND_HOST`]`:`[`DEFAULT_BIND_PORT`].
    ///
    /// Environment only. This is a property of *where the process runs*, decided
    /// by the same unit file or compose stanza that starts it.
    ///
    /// Takes `&self` despite reading nothing from it, so the listen address is
    /// discoverable from the same place as the other process-level settings.
    pub fn bind_address(&self) -> SocketAddr {
        bind_address_from(
            std::env::var("IP").ok().as_deref(),
            std::env::var("PORT").ok().as_deref(),
        )
    }

    /// Where the SQLite database lives: `$AIPLANE_DB_PATH`, else
    /// `gateway.sqlite` under [`data_dir`].
    ///
    /// The gateway has to find the database before it can read any setting out
    /// of it, so unlike the operator settings this is decided by the
    /// environment.
    pub fn db_path(&self) -> PathBuf {
        db_path_from(std::env::var("AIPLANE_DB_PATH").ok().as_deref())
    }

    /// Break-glass admin: raw OIDC claim values (group names) in
    /// `$AIPLANE_BOOTSTRAP_ADMIN_GROUPS` (comma-separated) that ALWAYS resolve
    /// to admin, regardless of the DB group tables. This is the one RBAC
    /// decision that intentionally still matches raw OIDC claims rather than a
    /// gateway group — the anti-lockout anchor. Gateway groups (incl. which
    /// ones are admin) are managed in `/admin/groups`, but if that mapping is
    /// misconfigured you could otherwise lock yourself out of the very UI that
    /// fixes it. List at least one trusted group here so an operator can always
    /// get in. Empty when unset.
    pub fn bootstrap_admin_groups(&self) -> Vec<String> {
        bootstrap_admin_groups_from(
            std::env::var("AIPLANE_BOOTSTRAP_ADMIN_GROUPS")
                .ok()
                .as_deref(),
        )
    }

    /// The reverse proxies whose `X-Forwarded-For` / `X-Real-IP` are believed:
    /// `$AIPLANE_TRUSTED_PROXIES`, comma-separated addresses or CIDR networks.
    /// Unset means none — every client IP is the TCP peer.
    ///
    /// Environment only, like [`Self::bind_address`]: which hops sit in front
    /// of the process is a fact about where it runs. A bad entry is fatal
    /// rather than skipped, because dropping it would silently stop trusting a
    /// proxy and collapse every visitor onto the proxy's address.
    pub fn trusted_proxies(
        &self,
    ) -> Result<crate::server::trusted_proxies::TrustedProxies, ConfigError> {
        crate::server::trusted_proxies::TrustedProxies::parse(
            std::env::var("AIPLANE_TRUSTED_PROXIES")
                .unwrap_or_default()
                .as_str(),
        )
        .map_err(|e| ConfigError::Invalid(e.to_string()))
    }

    /// The in-memory runtime configuration.
    ///
    /// Everything an operator sets lives in
    /// the database — topology at `/admin/upstreams`, groups at
    /// `/admin/groups`, the OIDC provider in the setup wizard, and the rest at
    /// `/admin/settings`, which [`crate::server::settings::apply`] writes over
    /// these defaults on boot. What is left here is decided by the environment
    /// (see [`Self::db_path`], [`Self::bind_address`], [`data_dir`]), because
    /// it is a property of *where the process runs* rather than something the
    /// gateway can read out of a database it has not opened yet.
    pub fn load() -> Result<Self, ConfigError> {
        let mut config = Self::default();
        if let Some(url) = std::env::var("AIPLANE_PUBLIC_URL")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
        {
            config.gateway.public_url_fallback = url;
        }
        config.network.allow_private_networks = flag_from(
            PRIVATE_NETWORKS_VAR,
            std::env::var(PRIVATE_NETWORKS_VAR).ok().as_deref(),
        )?;
        Ok(config)
    }
}

/// Environment only, like `$AIPLANE_TRUSTED_PROXIES`: whether the gateway may
/// reach its own network is a fact about where it runs, not a setting an
/// admin screen should be able to flip.
pub const PRIVATE_NETWORKS_VAR: &str = "AIPLANE_ALLOW_PRIVATE_NETWORKS";

/// A boolean environment variable: unset or empty is `false`; anything other
/// than the usual spellings stops the boot rather than guess.
fn flag_from(name: &str, value: Option<&str>) -> Result<bool, ConfigError> {
    match value.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        None | Some("" | "0" | "false" | "no" | "off") => Ok(false),
        Some("1" | "true" | "yes" | "on") => Ok(true),
        Some(other) => Err(ConfigError::Invalid(format!(
            "${name} is `{other}`; set it to `true` or `false`"
        ))),
    }
}

/// The pure half of [`Config::db_path`], split out so it can be tested without
/// mutating process-global environment state.
fn db_path_from(env: Option<&str>) -> PathBuf {
    env.map(str::trim)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(default_db_path)
}

/// The pure half of [`Config::bootstrap_admin_groups`]: trimmed and
/// de-duplicated, in the environment's order.
fn bootstrap_admin_groups_from(env: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for group in env.unwrap_or_default().split(',') {
        let group = group.trim();
        if !group.is_empty() && !out.iter().any(|g| g == group) {
            out.push(group.to_owned());
        }
    }
    out
}

/// The pure half of [`Config::bind_address`], split out so it can be tested
/// without mutating process-global environment state (which races every other
/// test in the binary) — the same split as [`data_dir_from`].
///
/// The two halves resolve independently: a PaaS that injects only `$PORT` must
/// not also drag the host back from whatever the image set.
fn bind_address_from(env_ip: Option<&str>, env_port: Option<&str>) -> SocketAddr {
    SocketAddr::new(
        env_ip
            .and_then(|raw| parse_or_warn::<IpAddr>(raw, "$IP"))
            .unwrap_or(DEFAULT_BIND_HOST),
        env_port
            .and_then(|raw| parse_or_warn::<u16>(raw, "$PORT"))
            .unwrap_or(DEFAULT_BIND_PORT),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_private_network_switch_is_off_unless_spelled_on() {
        for off in [None, Some(""), Some("0"), Some("false"), Some(" OFF ")] {
            assert!(!flag_from(PRIVATE_NETWORKS_VAR, off).unwrap(), "{off:?}");
        }
        for on in ["1", "true", "Yes", "on"] {
            assert!(flag_from(PRIVATE_NETWORKS_VAR, Some(on)).unwrap(), "{on}");
        }
        let refused = flag_from(PRIVATE_NETWORKS_VAR, Some("maybe"))
            .unwrap_err()
            .to_string();
        assert!(
            refused.contains(PRIVATE_NETWORKS_VAR) && refused.contains("true"),
            "{refused}"
        );
    }

    #[test]
    fn unset_data_dir_keeps_the_historical_relative_paths() {
        // `cargo run` in a checkout must keep writing ./gateway.sqlite and
        // ./data/rag — introducing AIPLANE_DATA_DIR must not silently move a
        // developer's database out from under them.
        let dir = data_dir_from(None);
        assert_eq!(dir.join("gateway.sqlite"), PathBuf::from("gateway.sqlite"));
        assert_eq!(dir.join("data").join("rag"), PathBuf::from("data/rag"));
    }

    #[test]
    fn empty_data_dir_is_treated_as_unset() {
        // An env var set to "" (a common accident in compose/systemd unit
        // files) must not turn every path into an absolute-root path.
        assert_eq!(data_dir_from(Some("".into())), PathBuf::new());
    }

    #[test]
    fn data_dir_relocates_db_and_rag_together() {
        // The whole point: ONE variable puts every writable path on the
        // mounted volume.
        let dir = data_dir_from(Some("/var/lib/gateway".into()));
        assert_eq!(
            dir.join("gateway.sqlite"),
            PathBuf::from("/var/lib/gateway/gateway.sqlite")
        );
        assert_eq!(
            dir.join("data").join("rag"),
            PathBuf::from("/var/lib/gateway/data/rag")
        );
    }

    #[test]
    fn the_public_url_falls_back_to_localhost_until_the_wizard_records_one() {
        // The wizard's request-derived suggestion is wrong behind a proxy that
        // rewrites Host — so a deployment has to be able to state it up front.
        assert_eq!(
            Config::default().public_url_fallback(),
            "http://localhost:8080"
        );
    }

    #[test]
    fn the_database_path_comes_from_the_environment_or_the_default() {
        assert_eq!(
            db_path_from(None),
            default_db_path(),
            "nobody said anything"
        );
        assert_eq!(
            db_path_from(Some("/from/env.sqlite")),
            PathBuf::from("/from/env.sqlite"),
            "the env var alone is enough"
        );
        assert_eq!(
            db_path_from(Some("  ")),
            default_db_path(),
            "an empty env var (a common compose/systemd accident) is not a path"
        );
    }

    #[test]
    fn bootstrap_admin_groups_are_trimmed_and_de_duplicated() {
        assert_eq!(
            bootstrap_admin_groups_from(Some("a, b ,, a")),
            vec!["a".to_string(), "b".to_string()],
            "trimmed, de-duplicated, and empty entries dropped"
        );
        assert!(bootstrap_admin_groups_from(None).is_empty());
    }

    #[test]
    fn defaults_bind_to_localhost() {
        assert_eq!(
            bind_address_from(None, None),
            "127.0.0.1:8080".parse::<SocketAddr>().unwrap(),
            "a gateway told nothing must not expose itself to the network"
        );
    }

    #[test]
    fn the_environment_sets_the_listen_socket() {
        assert_eq!(
            bind_address_from(Some("0.0.0.0"), Some("9000")),
            "0.0.0.0:9000".parse::<SocketAddr>().unwrap()
        );
    }

    #[test]
    fn each_half_of_the_socket_falls_back_on_its_own() {
        // A PaaS injecting only $PORT must not also drag the host back from
        // whatever the image set, and vice versa.
        assert_eq!(
            bind_address_from(None, Some("7000")),
            "127.0.0.1:7000".parse::<SocketAddr>().unwrap(),
        );
        assert_eq!(
            bind_address_from(Some("0.0.0.0"), None),
            "0.0.0.0:8080".parse::<SocketAddr>().unwrap(),
        );
    }

    #[test]
    fn a_nonsense_value_falls_back_instead_of_killing_the_boot() {
        // A typo in a variable with a perfectly good default must not be fatal,
        // and on a PaaS that injects $PORT the operator may not even control
        // the value. Each bad source is skipped, not the whole resolution.
        assert_eq!(
            bind_address_from(Some("not-an-ip"), Some("not-a-port")),
            "127.0.0.1:8080".parse::<SocketAddr>().unwrap(),
        );
        assert_eq!(
            bind_address_from(Some("localhost"), None),
            "127.0.0.1:8080".parse::<SocketAddr>().unwrap(),
            "a hostname is not a listen address",
        );
    }

    fn feedback(provider: &str) -> FeedbackConfig {
        FeedbackConfig {
            provider: provider.into(),
            github_owner: String::new(),
            github_repo: String::new(),
            github_token: None,
            labels: vec!["feedback".into()],
            assets_branch: "feedback-assets".into(),
            extraction_model: None,
            voice_model: None,
            github_api_base: "https://api.github.com".into(),
            gitlab_url: "https://gitlab.com".into(),
            gitlab_project_id: String::new(),
            gitlab_token: None,
        }
    }

    fn github_feedback() -> FeedbackConfig {
        FeedbackConfig {
            github_owner: "croit".into(),
            github_repo: "aiplane".into(),
            ..feedback("github")
        }
    }

    #[test]
    fn a_github_feedback_block_with_a_token_is_configured() {
        let f = FeedbackConfig {
            github_token: Some("ghp_inline".into()),
            ..github_feedback()
        };
        assert!(f.is_configured());
        assert_eq!(f.github_token().as_deref(), Some("ghp_inline"));
    }

    #[test]
    fn feedback_without_token_is_not_configured() {
        let f = github_feedback();
        assert!(f.github_token().is_none());
        assert!(!f.is_configured());
    }

    #[test]
    fn an_empty_token_counts_as_unset() {
        let f = FeedbackConfig {
            github_token: Some(String::new()),
            ..github_feedback()
        };
        assert!(f.github_token().is_none());
    }

    #[test]
    fn gitlab_needs_a_url_a_project_and_a_token() {
        let f = FeedbackConfig {
            gitlab_url: "https://gitlab.example.com".into(),
            gitlab_project_id: "group/sub/proj".into(),
            gitlab_token: Some("glpat-inline".into()),
            ..feedback("gitlab")
        };
        assert_eq!(f.provider(), FeedbackProvider::Gitlab);
        assert!(f.is_configured());
        assert_eq!(f.gitlab_token().as_deref(), Some("glpat-inline"));
        let without_project = FeedbackConfig {
            gitlab_project_id: String::new(),
            ..f
        };
        assert!(!without_project.is_configured());
    }

    // The whole point of an explicit selector: a deployment mid-migration has
    // BOTH credential sets in the database, and only the selected one counts.
    // Falling back to the other would file a customer's report into the repo
    // they are moving away from, which nobody would notice for weeks.
    #[test]
    fn a_complete_github_block_does_not_satisfy_a_gitlab_selection() {
        let f = FeedbackConfig {
            github_token: Some("ghp_inline".into()),
            ..github_feedback()
        };
        assert!(f.is_configured());
        let selected_gitlab = FeedbackConfig {
            provider: "gitlab".into(),
            ..f
        };
        assert!(
            !selected_gitlab.is_configured(),
            "gitlab is selected but not configured"
        );
    }

    #[test]
    fn provider_parsing_is_case_insensitive_and_defaults_to_github() {
        assert_eq!(FeedbackProvider::parse("GitLab"), FeedbackProvider::Gitlab);
        assert_eq!(
            FeedbackProvider::parse(" gitlab "),
            FeedbackProvider::Gitlab
        );
        assert_eq!(FeedbackProvider::parse("github"), FeedbackProvider::Github);
        assert_eq!(FeedbackProvider::parse(""), FeedbackProvider::Github);
        assert_eq!(
            FeedbackProvider::parse("bitbucket"),
            FeedbackProvider::Github
        );
        // The identifier is what the settings row stores, so it must survive
        // a parse/render round trip unchanged.
        for raw in ["github", "gitlab"] {
            assert_eq!(FeedbackProvider::parse(raw).as_str(), raw);
        }
    }
}
