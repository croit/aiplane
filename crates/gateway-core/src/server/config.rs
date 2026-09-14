// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Runtime configuration for the gateway.
//!
//! Loaded once at startup from a TOML file. Secrets (the upstream API key)
//! are sourced from environment variables — the file only names the env var.
//!
//! Lookup order:
//! 1. `$GATEWAY_CONFIG` (explicit path)
//! 2. `./gateway.toml`
//! 3. `/etc/gateway/config.toml`
//! 4. Built-in defaults (no upstream configured — proxy routes return 503).
//!

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;

use serde::Deserialize;
use thiserror::Error;

use crate::server::upstreams::config::FallbackConfig;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("reading config file `{path}`")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("parsing config file `{path}`")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    /// Two sources name the same thing and disagree, and guessing would be
    /// worse than stopping. Currently only the database path — see
    /// [`Config::db_path`].
    #[error("{0}")]
    Conflict(String),
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub gateway: GatewayConfig,
    /// Chat-page knobs that aren't routing-related — attachment
    /// storage + which model names are allowed to receive image
    /// content. Optional; defaults are conservative (S3 disabled,
    /// no vision models advertised so attachments error if S3
    /// isn't configured anyway).
    #[serde(default)]
    pub chat: ChatConfig,
    /// Typst-templated document generation. Optional — when unset,
    /// no `typst_*` tools register and the gateway boots fine.
    /// When set, the directory is scanned at startup for subdirs
    /// containing a `template.toml` manifest; each becomes one tool
    /// the model can call to produce a rendered PDF + PNG + .typ
    /// source from corporate-design templates.
    #[serde(default)]
    pub typst: Option<TypstConfig>,
    /// GeoIP (client-IP → coarse location) for the `get_user_location`
    /// tool. Optional — with no `[geoip]` block no database is loaded and
    /// the tool falls back to the browser-provided position (or reports
    /// the location as unknown). See `server::geoip`.
    #[serde(default)]
    pub geoip: Option<GeoipConfig>,
    /// RAG indexer state directory + tuning. Optional — with no `[rag]`
    /// block the indexer falls back to `data/rag` relative to the
    /// gateway's CWD, which is fine for local dev but NOT for the
    /// container image (its rootfs is read-only). Operators MUST point
    /// `data_dir` at a writable path (typically a subdirectory of the
    /// same named volume that backs `[db].path`).
    #[serde(default)]
    pub rag: Option<RagConfig>,
    /// Agent Skills the gateway makes available to the chat model.
    /// Optional — with no `[skills]` block none are loaded and the
    /// `read_skill` tool is not registered. When set, `dir` is scanned at
    /// startup for skill bundles (a directory holding a `SKILL.md`, or a
    /// `*.skill` zip of one); each becomes an operator-managed capability
    /// the model can load on demand. RBAC-gated per role via the role's
    /// `skills` list, exactly like `tools`. See `server::skills`.
    #[serde(default)]
    pub skills: Option<SkillsConfig>,
    /// Usage accounting (per-user / per-backend request metrics). Always
    /// present with sane defaults — there's no way to mis-configure it into
    /// failing a request, since recording is fire-and-forget. Set
    /// `[usage] enabled = false` to turn measurement off entirely, or tune
    /// `retention_days` to bound the raw-event window. See `server::usage`.
    #[serde(default)]
    pub usage: UsageConfig,
    /// Rate limits & quotas. Always present; the only knob is a kill switch
    /// (`[limits] enabled`, default true). Limits themselves are data — set
    /// per global/role/user in `/admin/limits`, not in the config file — so
    /// with no rules configured every caller is unlimited. See
    /// `server::limits`.
    #[serde(default)]
    pub limits: LimitsConfig,
    /// Code-execution sandbox. Optional — with no `[sandbox]` block the
    /// `run_in_sandbox` tool family is not registered and the gateway
    /// boots fine. When set, `runner_url` points at the standalone
    /// `sandbox-runner` service, which holds podman access and executes
    /// untrusted/LLM code in single-use sandboxes; the gateway only
    /// talks HTTP to it. See `server::tools::sandbox`.
    #[serde(default)]
    pub sandbox: Option<SandboxConfig>,
    /// Headless ComfyUI worker for image / video / audio workflows.
    /// Optional — with no `[comfyui]` block no `comfyui_*` tools register
    /// and the gateway boots fine. When set, `base_url` points at an
    /// internal ComfyUI instance and `content_dir` holds the curated
    /// workflow catalog (workflow.json + manifest.toml per subdirectory).
    /// See `docs/comfyui.md` and `server::comfyui`.
    #[serde(default)]
    pub comfyui: Option<ComfyuiConfig>,
    /// Feedback widget. Optional — with no `[feedback]` block the floating
    /// feedback button stays hidden (the `/feedback/config` endpoint reports
    /// it unconfigured, so the client never reveals the FAB). When set,
    /// every signed-in user gets a floating button that opens a form — with
    /// optional voice-to-fields dictation — and files the submission as a
    /// GitHub *or* GitLab issue, whichever [`FeedbackConfig::provider`]
    /// names. See `server::issue_tracker` + `rama_server::pages::feedback`.
    #[serde(default)]
    pub feedback: Option<FeedbackConfig>,
    /// Unknown-model fallback, per request kind. When a request names a model
    /// that no backend serves and that isn't an alias, the router substitutes
    /// the model named here for that kind instead of returning `404`. Optional;
    /// an unset kind keeps today's `model_not_found` behaviour. See
    /// [`FallbackConfig`] and `docs/upstreams.md`. Note the *offline* fallback
    /// (a known model whose replicas are all down) lives per-pool as
    /// `fallback_offline`, not here.
    #[serde(default)]
    pub fallback: FallbackConfig,
    /// Web Push notifications ("your turn finished"). Optional — with no
    /// `[push]` block it uses the defaults below (enabled, placeholder
    /// contact), which is fine: a VAPID keypair is generated on first boot
    /// and nothing is sent until a user opts in from the browser. Set
    /// `[push] enabled = false` to turn the feature (and its endpoints) off.
    #[serde(default)]
    pub push: PushConfig,
}

/// Web Push settings. Everything needed to send is self-generated (the VAPID
/// keypair persists in the DB); the only operator-facing knob besides the
/// master switch is the VAPID `contact`.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
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
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedbackConfig {
    /// `github` (default) or `gitlab` — which tracker receives the issues.
    #[serde(default)]
    pub provider: String,
    /// GitHub repository owner (user or org) that receives the issues,
    /// e.g. `croit`.
    #[serde(default)]
    pub github_owner: String,
    /// GitHub repository name, e.g. `llm-gateway`.
    #[serde(default)]
    pub github_repo: String,
    /// GitHub API token (classic PAT or fine-grained) able to open issues
    /// (`issues:write`) and — when screenshots are attached — commit asset
    /// files (`contents:write`). Per request this may live directly in the
    /// config file. If you'd rather keep it in the environment, leave this
    /// unset and use `github_token_env`; the direct value wins when both
    /// are present.
    #[serde(default)]
    pub github_token: Option<String>,
    /// Alternative to `github_token`: the NAME of an env var holding the
    /// token (same `*_env` convention as the rest of the config). Consulted
    /// only when `github_token` is unset/empty.
    #[serde(default)]
    pub github_token_env: Option<String>,
    /// Labels applied to every created issue, in addition to the automatic
    /// `priority:<p>`. Default `["feedback"]`.
    #[serde(default = "default_feedback_labels")]
    pub labels: Vec<String>,
    /// Orphan branch holding embedded screenshot assets: each screenshot is
    /// committed as a file and linked into the issue body via its raw URL.
    /// Created off the default branch on first use if missing. Default
    /// `feedback-assets`.
    #[serde(default = "default_feedback_assets_branch")]
    pub assets_branch: String,
    /// Chat model id used to turn a voice transcript into the structured
    /// form fields ("text model"). Unset/empty → the gateway picks the first
    /// available chat model at request time. This is an operator choice, not
    /// the end user's — the form never exposes a model picker.
    #[serde(default)]
    pub extraction_model: Option<String>,
    /// Transcription model id used to turn the voice recording into text
    /// ("voice model"). Unset/empty → the gateway picks the first available
    /// transcription model at request time. Operator choice, not the user's.
    #[serde(default)]
    pub voice_model: Option<String>,
    /// GitHub REST API base URL. Default `https://api.github.com`; override
    /// for GitHub Enterprise (`https://github.example.com/api/v3`).
    #[serde(default = "default_github_api_base")]
    pub github_api_base: String,
    /// GitLab instance base URL — the web origin, NOT the `/api/v4` path
    /// (which the client appends). Default `https://gitlab.com`; a
    /// self-managed instance is `https://gitlab.example.com`.
    #[serde(default = "default_gitlab_url")]
    pub gitlab_url: String,
    /// GitLab project the issues land in: either the numeric project id
    /// (`1234`) or the full path (`group/subgroup/project`), which the client
    /// URL-encodes. Both are what the GitLab REST API calls `:id`.
    #[serde(default)]
    pub gitlab_project_id: String,
    /// GitLab personal/project/group access token with the `api` scope (it
    /// needs to both create issues and POST to the project's `uploads`
    /// endpoint for the screenshot). Same inline-or-env rule as
    /// `github_token`.
    #[serde(default)]
    pub gitlab_token: Option<String>,
    /// Alternative to `gitlab_token`: the NAME of an env var holding it.
    #[serde(default)]
    pub gitlab_token_env: Option<String>,
}

fn default_feedback_labels() -> Vec<String> {
    vec!["feedback".to_string()]
}

fn default_feedback_assets_branch() -> String {
    "feedback-assets".to_string()
}

fn default_github_api_base() -> String {
    "https://api.github.com".to_string()
}

fn default_gitlab_url() -> String {
    "https://gitlab.com".to_string()
}

impl FeedbackConfig {
    /// The selected tracker.
    pub fn provider(&self) -> FeedbackProvider {
        FeedbackProvider::parse(&self.provider)
    }

    /// Resolve the GitHub token: the inline `github_token` first, then the
    /// env var named by `github_token_env`. Empty strings count as unset.
    pub fn github_token(&self) -> Option<String> {
        resolve_secret(
            self.github_token.as_deref(),
            self.github_token_env.as_deref(),
        )
    }

    /// Resolve the GitLab token, same inline-then-env rule.
    pub fn gitlab_token(&self) -> Option<String> {
        resolve_secret(
            self.gitlab_token.as_deref(),
            self.gitlab_token_env.as_deref(),
        )
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
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxConfig {
    /// Master switch. `true` (default) registers the sandbox tools; set
    /// `false` to turn the whole feature off without deleting the block
    /// (e.g. to keep `runner_url` around while disabling it). Per-tool and
    /// per-user/-token control is separate (RBAC + the `/tools` toggles).
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Base URL of the sandbox-runner service, e.g.
    /// `http://sandbox-runner:9000`. MUST be reachable only from the
    /// gateway (internal network / mTLS) — it executes arbitrary code.
    pub runner_url: String,
    /// HTTP timeout for a single `/run` call. Should exceed the runner's
    /// own per-job timeout plus sandbox cold-start margin.
    #[serde(default = "default_sandbox_timeout")]
    pub timeout_secs: u64,
    /// Largest single produced file the gateway will accept back from a
    /// run and store. Larger artifacts are dropped with a note in the
    /// tool result rather than bloating storage / the model context.
    #[serde(default = "default_sandbox_max_artifact")]
    pub max_artifact_bytes: u64,
}

fn default_sandbox_timeout() -> u64 {
    120
}

fn default_sandbox_max_artifact() -> u64 {
    25 * 1024 * 1024
}

/// Headless ComfyUI worker settings. The worker owns GPU + model files;
/// the gateway owns the curated workflow catalog and the typed tool
/// surface the model sees. See `docs/comfyui.md`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComfyuiConfig {
    /// Master switch. `true` (default) registers the `comfyui_*` tools;
    /// `false` keeps the config block but disables registration (handy
    /// for turning the feature off without deleting the block).
    #[serde(default = "default_true")]
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
    #[serde(default = "default_comfyui_timeout")]
    pub timeout_secs: u64,
    /// How often to poll ComfyUI's `/history/{prompt_id}` while waiting for
    /// a workflow to finish. Lower = snappier feedback, higher = less load
    /// on ComfyUI's history API. Default 500 ms.
    #[serde(default = "default_comfyui_poll_interval")]
    pub queue_poll_interval_ms: u64,
    /// Max workflows the gateway will let the model dispatch concurrently.
    /// Default 1 — a single 24 GB GPU realistically runs one diffusion job
    /// at a time. ComfyUI itself queues internally, so raising this mainly
    /// affects how many gateway-side slots are reserved.
    #[serde(default = "default_comfyui_concurrency")]
    pub max_concurrent_jobs: usize,
}

fn default_comfyui_timeout() -> u64 {
    600
}

fn default_comfyui_poll_interval() -> u64 {
    500
}

fn default_comfyui_concurrency() -> usize {
    1
}

/// Usage-metrics knobs. Recording is decoupled from the request path (a
/// bounded channel drained by a background batched writer), so these only
/// affect how much history is kept and whether measurement runs at all.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
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

/// Rate-limit / quota enforcement. Limits themselves live in the DB (set via
/// `/admin/limits`); this block only carries the deployment-wide kill switch.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LimitsConfig {
    /// Master switch. When `false`, the [`crate::server::limits::Enforcer`]
    /// never blocks (and skips its per-request read) regardless of configured
    /// rules — a production escape hatch. Default `true`. Note that with no
    /// rules configured everyone is unlimited anyway, so leaving this on costs
    /// nothing until you set a limit.
    pub enabled: bool,
}

impl Default for LimitsConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// Skills directory. Mirrors `[rag] data_dir` / `[typst] templates_dir`:
/// a single operator-owned folder scanned once at startup. No hot-reload —
/// restart to pick up new or changed skills.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillsConfig {
    /// Root holding one skill per entry. An entry is either a directory
    /// containing a `SKILL.md` (optionally nested one level, e.g. the
    /// folder a `*.skill` archive unzips to) or a `*.skill` file (a zip of
    /// such a directory), which is extracted into `<dir>/.cache/` at
    /// startup. The gateway only reads this directory; operators populate
    /// it out-of-band (drop a bundle in and restart), just like Typst
    /// templates. Default is `data/skills` relative to the gateway's
    /// working directory.
    #[serde(default = "default_skills_dir")]
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
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RagConfig {
    /// Root for all per-collection RAG storage. Each collection gets its
    /// own self-contained folder `<data_dir>/<uuid>/` holding `rag.sqlite`
    /// (chunk text + FTS index), `index.usearch` (vectors), and `clone/`
    /// (the git working tree). This is the only heavy / regenerable state,
    /// so keep it separate from the precious central `[db].path` — e.g. on
    /// a larger or cheaper drive/mount. The gateway `mkdir -p`s this on
    /// startup, so the **parent** must already exist + be writable by the
    /// runtime user (uid 1000 in the container image). Default is
    /// `data/rag` under [`data_dir`] — relative to the working directory in
    /// dev, and on the mounted volume inside the container.
    #[serde(default = "default_rag_data_dir")]
    pub data_dir: PathBuf,
    /// How many git clones the indexer runs at once, and how many
    /// collections it indexes in parallel. Raising this lets a bunch of
    /// repos/collections index concurrently instead of head-of-line
    /// blocking behind one slow clone; the cost is more simultaneous
    /// network + embedding load. `0` is treated as `1` (fully serial).
    /// Default `4`.
    #[serde(default = "default_clone_concurrency")]
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

fn default_true() -> bool {
    true
}

/// GeoIP settings. Points at an IP2Location LITE DB11 `.BIN` and,
/// optionally, names the env var holding an IP2Location download token
/// for the weekly auto-updater — same "the file holds the env-var NAME,
/// not the secret" pattern as [`S3Config`] / [`OidcConfig`].
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeoipConfig {
    /// Path to the IP2Location LITE DB11 BIN. A missing file is not an
    /// error — lookups simply return nothing until one appears. Changes
    /// are hot-reloaded (no restart).
    #[serde(default = "default_geoip_db_path")]
    pub db_path: PathBuf,
    /// Name of the env var holding the IP2Location download token. When
    /// set (and non-empty) a weekly background task refreshes `db_path`.
    /// Unset → no auto-update; operators can drop in their own BIN and it
    /// gets picked up by the hot-reload watcher.
    pub update_token_env: Option<String>,
    /// The token itself, as entered at `/admin/settings`, where it is sealed at
    /// rest. Wins over [`Self::update_token_env`].
    #[serde(default)]
    pub update_token: Option<String>,
}

fn default_geoip_db_path() -> PathBuf {
    PathBuf::from("data/ip2location/IP2LOCATION-LITE-DB11.BIN")
}

impl GeoipConfig {
    /// The stored download token, else whatever the legacy `update_token_env`
    /// variable names.
    pub fn update_token(&self) -> Option<String> {
        resolve_secret(
            self.update_token.as_deref(),
            self.update_token_env.as_deref(),
        )
    }
}

/// Typst document-rendering settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypstConfig {
    /// Root directory holding one subdirectory per template. Each
    /// subdir must contain `template.toml` (manifest) + `template.typ`
    /// (source). Co-located assets (logos, fonts) work because the
    /// typst compile is run with `--root` pointing at the subdir.
    pub templates_dir: PathBuf,
}

/// Chat-page settings. Attachments go to S3 (object storage) so the
/// DB doesn't bloat and the OpenAI API can fetch image URLs directly
/// from the bucket. We don't gate which model gets to receive
/// image content — operators are expected to wire only multi-modal
/// chat models into the gateway, and any capability mismatch
/// surfaces as the upstream's error on send.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ChatConfig {
    pub s3: Option<S3Config>,
    /// Optional automatic OCR enrichment for image/PDF attachments. The OCR
    /// pool must point at the document-aware OCR sidecar.
    #[serde(default)]
    pub ocr: OcrConfig,
    /// Automatic conversation compaction — summarise a session's oldest turns
    /// once its replayed context grows past a fraction of the model's window.
    #[serde(default)]
    pub compaction: CompactionConfig,
}

/// Automatic document OCR. Off by default; even switched on, nothing happens
/// unless a healthy `kind = "ocr"` pool serves a model.
///
/// Every limit here exists because OCR is the most expensive derived artefact
/// the gateway produces — an unbounded scan is N vision-model calls on someone
/// else's page count — so the defaults are deliberately conservative and an
/// operator raises them knowingly.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
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
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
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
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
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
    /// The credentials themselves, as entered at `/admin/settings`. They may
    /// live here directly because that store seals them at rest — the reason
    /// they were kept out of the config file does not apply to a sealed
    /// database row. Same shape as [`FeedbackConfig::github_token`].
    #[serde(default)]
    pub access_key: Option<String>,
    #[serde(default)]
    pub secret_key: Option<String>,
    /// Legacy: the NAME of an env var holding the access key id, so secrets
    /// stayed out of TOML. Still parsed, so an existing config file imports
    /// cleanly, and still consulted when no direct value is set.
    #[serde(default)]
    pub access_key_env: Option<String>,
    /// Legacy env-var name for the secret access key. See [`Self::access_key_env`].
    #[serde(default)]
    pub secret_key_env: Option<String>,
    /// Object-key prefix under which chat attachments live. Default
    /// `chat-attachments`. Useful when the bucket is shared with
    /// other workloads.
    #[serde(default = "default_s3_prefix")]
    pub key_prefix: String,
}

fn default_s3_prefix() -> String {
    "chat-attachments".to_string()
}

impl S3Config {
    /// The access key id: the stored value, else whatever the legacy
    /// `access_key_env` variable names. The direct value wins, so a credential
    /// entered at `/admin/settings` is not shadowed by a stale env var left
    /// over from before the migration.
    pub fn access_key(&self) -> Option<String> {
        resolve_secret(self.access_key.as_deref(), self.access_key_env.as_deref())
    }
    /// The secret access key. See [`Self::access_key`].
    pub fn secret_key(&self) -> Option<String> {
        resolve_secret(self.secret_key.as_deref(), self.secret_key_env.as_deref())
    }
}

/// A secret stored directly, else the value of the env var that the legacy
/// `*_env` field names. Shared by every block that made that move, so the
/// precedence is written down once instead of three times.
fn resolve_secret(direct: Option<&str>, env_name: Option<&str>) -> Option<String> {
    if let Some(v) = direct.filter(|v| !v.is_empty()) {
        return Some(v.to_owned());
    }
    env_name
        .filter(|n| !n.is_empty())
        .and_then(|n| std::env::var(n).ok())
        .filter(|v| !v.is_empty())
}

/// Knobs the gateway itself needs (separate from `bind` which only describes
/// where to listen).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayConfig {
    /// Public URL the gateway is reachable at, e.g. `https://gateway.example.com`.
    ///
    /// **Never read this directly** — call
    /// [`Config::public_url_fallback`] if you want the fallback, or
    /// `AppState::public_url()` if you want the live value.
    ///
    /// Set from `$GATEWAY_PUBLIC_URL`. Named for what it is rather than for the
    /// value it holds, because `public_url` is exactly the name of the accessor
    /// that returns the *correct* value: with both spelled the same, neither
    /// grep nor the compiler could tell a right read from a wrong one, and a
    /// wrong one silently yields `http://localhost:8080` on every
    /// wizard-configured deployment.
    #[serde(default = "default_public_url", rename = "public_url")]
    pub public_url_import_only: String,
    /// How long a freshly minted gateway token is valid for. Default 90 days.
    ///
    /// Seed-only, like the three below: imported into the database once and
    /// then edited at `/admin/settings`. See
    /// [`crate::server::settings::GATEWAY_KEYS_STAYING_IN_THE_FILE`] for the
    /// keys in this block that did *not* move.
    #[serde(default = "default_token_ttl_days")]
    pub token_ttl_days: i64,
    /// Browser-session idle timeout in days. Default 30. This is a *sliding*
    /// window: every request renews it (see `rama_server::session`), so it's
    /// how long you can stay away before having to sign in again, not how
    /// long a login lasts. Lower it for stricter deployments; the cookie
    /// itself always carries a long `Max-Age` so a browser or laptop restart
    /// alone never logs anyone out.
    #[serde(default = "default_session_ttl_days")]
    pub session_ttl_days: i64,
    /// Absolute cap on a session's lifetime in days, counted from login.
    /// Default 90. Unlike `session_ttl_days` this one does *not* slide: it
    /// forces everyone back through the identity provider periodically,
    /// which is also the only point where OIDC group claims are re-read.
    #[serde(default = "default_session_absolute_max_days")]
    pub session_absolute_max_days: i64,
    /// Whether admins may impersonate other users from `/admin/users`.
    /// Default `false` (opt-in) — impersonation is a powerful, privileged
    /// capability, so it's off unless explicitly enabled. Set
    /// `allow_impersonation = true` to turn it on; the Impersonate buttons
    /// then appear and `POST /admin/users/impersonate` works. While disabled
    /// the buttons are hidden and that endpoint returns 403. Stopping an
    /// already-active impersonation (`/impersonate/stop`) always works, so
    /// nobody is trapped mid-session if the flag is flipped at runtime.
    #[serde(default)]
    pub allow_impersonation: bool,
    /// Break-glass admin: raw OIDC claim values (group names) that ALWAYS
    /// resolve to admin, regardless of the DB group tables. This is the one
    /// RBAC decision that intentionally still matches raw OIDC claims rather
    /// than a gateway group — the anti-lockout anchor. Gateway groups (incl.
    /// which ones are admin) are managed in `/admin/groups`, but if that
    /// mapping is misconfigured you could otherwise lock yourself out of the
    /// very UI that fixes it. List at least one trusted group here so an
    /// operator can always get in. Empty by default.
    #[serde(default)]
    pub bootstrap_admin_groups: Vec<String>,
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
    #[serde(default = "default_upstream_wait_secs")]
    pub upstream_wait_secs: u64,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            public_url_import_only: default_public_url(),
            token_ttl_days: default_token_ttl_days(),
            session_ttl_days: default_session_ttl_days(),
            session_absolute_max_days: default_session_absolute_max_days(),
            allow_impersonation: false,
            bootstrap_admin_groups: Vec::new(),
            upstream_wait_secs: default_upstream_wait_secs(),
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

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OidcConfig {
    pub issuer: String,
    pub client_id: String,
    /// Name of the env var holding the OIDC client secret. Never the secret
    /// itself.
    pub client_secret_env: String,
    /// Scopes to request, on top of `openid` which is always included.
    #[serde(default = "default_scopes")]
    pub scopes: Vec<String>,
    /// OIDC claim that holds the user's role memberships (e.g. "groups").
    /// Mapped to internal roles in Phase 5.
    pub roles_claim: Option<String>,
}

fn default_scopes() -> Vec<String> {
    vec!["email".into(), "profile".into()]
}

impl OidcConfig {
    pub fn client_secret(&self) -> Option<String> {
        std::env::var(&self.client_secret_env)
            .ok()
            .filter(|v| !v.is_empty())
    }
}
/// The database filename, under whichever directory [`data_dir`] resolves to.
pub const DB_FILENAME: &str = "gateway.sqlite";

pub fn default_db_path() -> PathBuf {
    data_dir().join(DB_FILENAME)
}

/// Where the default database lived before `GATEWAY_DATA_DIR` existed: the
/// process working directory.
///
/// Derived from the same constant as [`default_db_path`] rather than spelled
/// out at the one call site that needs it, so renaming the file cannot leave a
/// stale literal behind in the upgrade guard that depends on it.
pub fn legacy_default_db_path() -> PathBuf {
    PathBuf::from(DB_FILENAME)
}

/// Root for everything the gateway must be able to WRITE: the SQLite
/// database, the RAG index store, and whatever persistent state gets added
/// later.
///
/// `$GATEWAY_DATA_DIR` when set, otherwise the process working directory —
/// so a `cargo run` in the repo still writes `./gateway.sqlite` and
/// `./data/rag` exactly as before. The container image points it at the
/// mounted volume, which is what makes a fresh container persist its state
/// without a config file and without a per-path environment variable each.
///
/// Read-only paths (typst templates, skills bundles) deliberately do NOT
/// hang off this — they ship in the image's read-only layers.
pub fn data_dir() -> PathBuf {
    data_dir_from(std::env::var_os("GATEWAY_DATA_DIR"))
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

/// The one variable the session key is read from. Named here so the deprecation
/// warning for `[gateway].session_key_env` and the boot-time read cannot drift
/// apart on the spelling.
pub const SESSION_KEY_VAR: &str = "GATEWAY_SESSION_KEY";

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
    /// `$GATEWAY_PUBLIC_URL` — a *fallback*, used only until the setup wizard
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
        &self.gateway.public_url_import_only
    }

    /// The socket to listen on: `$IP` / `$PORT`, else
    /// [`DEFAULT_BIND_HOST`]`:`[`DEFAULT_BIND_PORT`].
    ///
    /// Environment only. This is a property of *where the process runs*, decided
    /// by the same unit file or compose stanza that starts it, so it sits with
    /// the other process-level variables rather than in a file the gateway also
    /// treats as a one-time seed for database settings. See [`BindConfig`] for
    /// the block that used to be here.
    ///
    /// Takes `&self` despite reading nothing from it: the call site has a
    /// `Config` in hand and this keeps `[bind]`'s replacement discoverable from
    /// the same place.
    pub fn bind_address(&self) -> SocketAddr {
        bind_address_from(
            std::env::var("IP").ok().as_deref(),
            std::env::var("PORT").ok().as_deref(),
        )
    }

    /// Where the SQLite database lives: `$GATEWAY_DB_PATH`, else `[db].path`,
    /// else `gateway.sqlite` under [`data_dir`].
    ///
    /// The env var exists so a deployment needs no config file for this either.
    /// It is the last thing that forced one: the gateway has to find the
    /// database before it can read any setting *out* of the database, so unlike
    /// the operator settings this genuinely cannot move into it.
    ///
    /// **Disagreement is fatal, not resolved.** Every other two-source setting
    /// here picks a winner and warns, because the cost of picking wrong is a
    /// wrong port or a stale directory. Here the cost is that the gateway opens
    /// a database that is not the deployment's, finds no users, concludes it is
    /// a fresh install, and serves an open setup wizard while the real data sits
    /// untouched somewhere else — the exact failure
    /// [`legacy_default_db_path`]'s guard exists to prevent. A boot that stops
    /// with both paths printed costs an operator a minute; the alternative costs
    /// them a morning.
    pub fn db_path(&self) -> Result<PathBuf, ConfigError> {
        Ok(db_path_from(
            std::env::var("GATEWAY_DB_PATH").ok().as_deref(),
        ))
    }

    /// Raw OIDC claim values that always resolve to admin: the file's
    /// `[gateway].bootstrap_admin_groups` **plus** anything in
    /// `$GATEWAY_BOOTSTRAP_ADMIN_GROUPS` (comma-separated).
    ///
    /// A union, not an override, and deliberately: this is the anti-lockout
    /// anchor, so no source may take an escape hatch *away* from another. An
    /// operator adding the env var to get back in must not have to notice that
    /// it silently discarded the group already listed in the file.
    ///
    /// The env var exists so this can be set beside `$GATEWAY_SESSION_KEY` in
    /// the one environment file a deployment already has, instead of being the
    /// last reason to mount a config file.
    pub fn bootstrap_admin_groups(&self) -> Vec<String> {
        bootstrap_admin_groups_from(
            &self.gateway.bootstrap_admin_groups,
            std::env::var("GATEWAY_BOOTSTRAP_ADMIN_GROUPS")
                .ok()
                .as_deref(),
        )
    }

    /// Resolves the config file path and loads it. Missing files are not an
    /// error — we fall back to defaults so `mise run dev` can start without
    /// any setup.
    /// The in-memory runtime configuration.
    ///
    /// There is no config file any more. Everything an operator sets lives in
    /// the database — topology at `/admin/upstreams`, groups at
    /// `/admin/groups`, the OIDC provider in the setup wizard, and the rest at
    /// `/admin/settings`, which [`crate::server::settings::apply`] writes over
    /// these defaults on boot. What is left here is decided by the environment
    /// (see [`Self::db_path`], [`Self::bind_address`], [`data_dir`]), because
    /// it is a property of *where the process runs* rather than something the
    /// gateway can read out of a database it has not opened yet.
    pub fn load() -> Result<Self, ConfigError> {
        let mut config = Self::default();
        if let Some(url) = std::env::var("GATEWAY_PUBLIC_URL")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
        {
            config.gateway.public_url_import_only = url;
        }
        Ok(config)
    }
}

/// The pure half of [`Config::bind_address`], split out so it can be tested
/// without mutating process-global environment state (which races every other
/// test in the binary) — the same split as [`data_dir_from`].
///
/// The two halves resolve independently: a PaaS that injects only `$PORT` must
/// not also drag the host back from whatever the image set.
/// The pure half of [`Config::db_path`], split out so it can be tested without
/// mutating process-global environment state.
fn db_path_from(env: Option<&str>) -> PathBuf {
    env.map(str::trim)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(default_db_path)
}

/// The pure half of [`Config::bootstrap_admin_groups`]: a union, de-duplicated,
/// preserving the file's order and appending the environment's.
fn bootstrap_admin_groups_from(file: &[String], env: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = file
        .iter()
        .map(|g| g.trim().to_owned())
        .filter(|g| !g.is_empty())
        .collect();
    for group in env.unwrap_or_default().split(',') {
        let group = group.trim();
        if !group.is_empty() && !out.iter().any(|g| g == group) {
            out.push(group.to_owned());
        }
    }
    out
}

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
    fn unset_data_dir_keeps_the_historical_relative_paths() {
        // `cargo run` in a checkout must keep writing ./gateway.sqlite and
        // ./data/rag — introducing GATEWAY_DATA_DIR must not silently move a
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
        // mounted volume, so the container needs no config file for it.
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
        // Not read from a file any more, and the wizard's request-derived
        // suggestion is wrong behind a proxy that rewrites Host — so a
        // deployment has to be able to state it up front.
        assert_eq!(
            Config::default().public_url_fallback(),
            "http://localhost:8080"
        );
    }

    #[test]
    fn the_database_path_comes_from_the_environment_or_the_default() {
        // The only two sources left. `[db].path` is gone with the config file,
        // so the disagreement that used to be fatal cannot arise.
        assert_eq!(
            db_path_from(None),
            default_db_path(),
            "nobody said anything"
        );
        assert_eq!(
            db_path_from(Some("/from/env.sqlite")),
            PathBuf::from("/from/env.sqlite"),
            "the env var alone is enough — no config file needed for this"
        );
        assert_eq!(
            db_path_from(Some("  ")),
            default_db_path(),
            "an empty env var (a common compose/systemd accident) is not a path"
        );
    }

    #[test]
    fn bootstrap_admin_groups_union_never_drops_an_escape_hatch() {
        // A union, not an override: this is the anti-lockout anchor, so setting
        // the env var to get back in must not silently discard the group the
        // file already trusted.
        let file = vec!["platform-admins".to_string()];
        assert_eq!(
            bootstrap_admin_groups_from(&file, Some("ops-oncall")),
            vec!["platform-admins".to_string(), "ops-oncall".to_string()]
        );
        assert_eq!(
            bootstrap_admin_groups_from(&file, None),
            vec!["platform-admins".to_string()]
        );
        assert_eq!(
            bootstrap_admin_groups_from(&[], Some("a, b ,, a")),
            vec!["a".to_string(), "b".to_string()],
            "trimmed, de-duplicated, and empty entries dropped"
        );
        assert!(bootstrap_admin_groups_from(&[], None).is_empty());
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

    #[test]
    fn feedback_block_parses_with_inline_token_and_defaults() {
        let toml = r#"
            [feedback]
            github_owner = "croit"
            github_repo  = "llm-gateway"
            github_token = "ghp_inline"
        "#;
        let c: Config = toml::from_str(toml).unwrap();
        let f = c.feedback.expect("feedback block");
        assert!(f.is_configured());
        assert_eq!(f.github_token().as_deref(), Some("ghp_inline"));
        // Defaults applied.
        assert_eq!(f.labels, vec!["feedback"]);
        assert_eq!(f.assets_branch, "feedback-assets");
        assert_eq!(f.github_api_base, "https://api.github.com");
    }

    #[test]
    fn feedback_without_token_is_not_configured() {
        let toml = r#"
            [feedback]
            github_owner = "croit"
            github_repo  = "llm-gateway"
        "#;
        let c: Config = toml::from_str(toml).unwrap();
        let f = c.feedback.expect("feedback block");
        // No inline token and no env var named → not configured.
        assert!(f.github_token().is_none());
        assert!(!f.is_configured());
    }

    #[test]
    fn no_feedback_block_means_disabled() {
        let c: Config = toml::from_str("").unwrap();
        assert!(c.feedback.is_none());
    }

    // A block written before the GitLab option existed has no `provider` row
    // at all, and it must keep filing to the GitHub repo it always did.
    #[test]
    fn an_absent_provider_means_github() {
        let toml = r#"
            [feedback]
            github_owner = "croit"
            github_repo  = "llm-gateway"
            github_token = "ghp_inline"
        "#;
        let c: Config = toml::from_str(toml).unwrap();
        let f = c.feedback.expect("feedback block");
        assert_eq!(f.provider(), FeedbackProvider::Github);
        assert!(f.is_configured());
        // The GitLab half defaults to something usable but incomplete.
        assert_eq!(f.gitlab_url, "https://gitlab.com");
        assert!(f.gitlab_token().is_none());
    }

    #[test]
    fn gitlab_needs_a_url_a_project_and_a_token() {
        let toml = r#"
            [feedback]
            provider            = "gitlab"
            gitlab_url          = "https://gitlab.example.com"
            gitlab_project_id   = "group/sub/proj"
            gitlab_token        = "glpat-inline"
        "#;
        let c: Config = toml::from_str(toml).unwrap();
        let f = c.feedback.expect("feedback block");
        assert_eq!(f.provider(), FeedbackProvider::Gitlab);
        assert!(f.is_configured());
        assert_eq!(f.gitlab_token().as_deref(), Some("glpat-inline"));
    }

    // The whole point of an explicit selector: a deployment mid-migration has
    // BOTH credential sets in the database, and only the selected one counts.
    // Falling back to the other would file a customer's report into the repo
    // they are moving away from, which nobody would notice for weeks.
    #[test]
    fn a_complete_github_block_does_not_satisfy_a_gitlab_selection() {
        let toml = r#"
            [feedback]
            provider     = "gitlab"
            github_owner = "croit"
            github_repo  = "llm-gateway"
            github_token = "ghp_inline"
        "#;
        let c: Config = toml::from_str(toml).unwrap();
        let f = c.feedback.expect("feedback block");
        assert!(!f.is_configured(), "gitlab is selected but not configured");
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
