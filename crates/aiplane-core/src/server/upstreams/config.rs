// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Configuration shape for the multi-provider routing layer, built from the
//! database topology by `db_bridge`.

use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone)]
pub struct UpstreamPoolConfig {
    pub kind: PoolKind,
    pub strategy: PickerStrategy,
    /// Backup model for this pool when a model it *knows* has no healthy
    /// backend right now (every replica down). On that `503`-shaped outage
    /// the router re-resolves the request to this model instead — typically a
    /// different tier (e.g. local GPUs down → a cloud model). Re-resolved
    /// through the normal path (so it may itself be an alias/group) and only a
    /// **single hop**: if the backup is also unavailable the original `503` is
    /// returned. Absent ⇒ the outage surfaces as `503`, as before. Distinct
    /// from [`crate::server::config::Config`]'s `fallback`, which handles a
    /// wholly *unknown* model name.
    pub fallback_offline: Option<String>,
    /// Pool-level model IDs, served by every backend in this pool that
    /// declares none of its own. Setting them turns discovery off for those
    /// backends — see [`BackendConfig::models`] for the precedence.
    pub models: Vec<String>,
    /// Data-handling/compliance attributes for every model this pool serves.
    /// Absent block ⇒ [`Compliance::default`] (all-clear): no UI warning. Set
    /// a flag to `false` to surface a per-conversation warning banner in the
    /// chat UI — purely advisory signalling, no request-blocking. Lives at the
    /// pool level because residency/coverage is a property of the upstream
    /// endpoint, not the individual model id.
    pub compliance: Compliance,
    /// Whether calls served by this pool count toward users' rate limits and
    /// quotas. Default `true`. Set `false` for pools whose usage should be
    /// recorded (it still shows on `/usage`) but never consume a budget —
    /// typically your own self-hosted GPU pools, which are free, while cloud
    /// pools stay enforced. Like [`compliance`](Self::compliance), it lives at
    /// the pool level because "is this endpoint billable" is a property of the
    /// upstream, not the individual model id. See `server::limits`.
    pub enforce_limits: bool,
    /// Language → voice name map, only meaningful for `kind = "speech"` pools.
    /// The voice-conversation flow picks the voice whose key matches the
    /// language the user spoke (from STT), falling back to the `""` (empty-key)
    /// entry as the default voice, then to no explicit voice at all. Keys are
    /// lowercase ISO-639-1 codes (`"de"`, `"en"`, …); values are backend voice
    /// ids. Empty ⇒ always send the backend/default voice.
    pub voices: HashMap<String, String>,
    /// Voices this pool offers users to *choose* from, in menu order. Only
    /// meaningful for `kind = "speech"` pools.
    ///
    /// Separate from [`Self::voices`] because that is a language → voice map
    /// and holds one voice per language: it answers "which voice for German",
    /// not "which German voices may a user pick". Empty (the default) means no
    /// menu, and the per-user picker falls back to whatever `voices` resolves
    /// to — one entry in a single-voice deployment, which hides the picker.
    pub offer_voices: Vec<String>,
    /// Gateway-group names allowed to see + route to this pool. Empty (default)
    /// = unrestricted. Managed in `/admin/upstreams`. See
    /// `db::gateway_groups` and `Resolver::resource_allowed`.
    pub allowed_groups: Vec<String>,
    pub backend: Vec<BackendConfig>,
}

impl UpstreamPoolConfig {
    /// A pool of `kind` over `backend` with every other setting at its default.
    pub fn new(kind: PoolKind, backend: Vec<BackendConfig>) -> Self {
        Self {
            kind,
            strategy: PickerStrategy::default(),
            fallback_offline: None,
            models: Vec::new(),
            compliance: Compliance::default(),
            enforce_limits: true,
            voices: HashMap::new(),
            offer_voices: Vec::new(),
            allowed_groups: Vec::new(),
            backend,
        }
    }

    /// Resolve the TTS voice for a spoken `language` (lowercase ISO-639-1).
    /// Exact match wins; then the default (`""`) entry; then `None` (let the
    /// backend use its own default voice). Pure so it's unit-tested.
    pub fn voice_for_language(&self, language: &str) -> Option<&str> {
        self.voices
            .get(language)
            .or_else(|| self.voices.get(""))
            .map(String::as_str)
    }
}

/// Per-pool data-handling attributes, surfaced to the user as chat-UI warnings
/// when a flag is `false`. Both default to `true` (compliant), so a pool with
/// no compliance flags set shows no warning — you opt **in** to a warning by
/// declaring `false`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Compliance {
    /// `false` ⇒ data sent here leaves the EU / has no GDPR safeguards. Warns
    /// the user not to enter personal data.
    pub gdpr: bool,
    /// `false` ⇒ the endpoint is not covered by a confidentiality agreement.
    /// Warns the user not to send NDA-protected / proprietary material.
    pub nda: bool,
}

impl Default for Compliance {
    fn default() -> Self {
        Self {
            gdpr: true,
            nda: true,
        }
    }
}

impl Compliance {
    /// True when every flag is clear — the common case, drawn with no warning.
    pub fn is_all_clear(&self) -> bool {
        self.gdpr && self.nda
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PoolKind {
    Chat,
    Transcription,
    Embedding,
    /// Image generation (OpenAI `/images/generations`-shaped). Routed like any
    /// other pool; a backend whose `/models` lists something else declares its
    /// model ids in `models` (see [`BackendConfig::models`]).
    Image,
    /// Text-to-speech. Backs `POST /v1/audio/speech` (OpenAI-shaped) and the
    /// session `POST /api/v0/speech` the voice-conversation UI calls. Dormant
    /// unless an operator configures a `kind = "speech"` pool — voice mode only
    /// appears when one exists, mirroring how transcription degrades.
    Speech,
    /// Dedicated document OCR. This is an internal capability pool; it is
    /// not exposed as a normal chat model endpoint.
    Ocr,
    /// Cross-encoder reranking: scores (query, passage) pairs. Backs
    /// knowledge-search reranking and `POST /v1/rerank`.
    Rerank,
    /// TypeSafe System One compatible decision models. Backs
    /// `POST /v1/systemone`; unlike chat models these return typed `noul`,
    /// `choice`, and `score` answers rather than generated text.
    SystemOne,
}

impl PoolKind {
    /// The wire name, matching `#[serde(rename_all = "snake_case")]`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Transcription => "transcription",
            Self::Embedding => "embedding",
            Self::Image => "image",
            Self::Speech => "speech",
            Self::Ocr => "ocr",
            Self::Rerank => "rerank",
            Self::SystemOne => "system_one",
        }
    }

    /// An internal capability pool: never listed in `/v1/models`, offered to
    /// a token or grantable. OCR serves the gateway's own document reading
    /// and has no `/v1` endpoint a client could call it through.
    pub fn is_internal(self) -> bool {
        matches!(self, Self::Ocr)
    }

    /// Every kind, in the order an operator should see them offered.
    ///
    /// The one place the vocabulary is written down. It had been copied into
    /// the admin API's validation, the OpenAPI spec and the SPA's `<option>`
    /// list, and the copy in the API was missing `Rerank` — so creating a
    /// rerank pool through the UI was rejected as an "unknown pool kind"
    /// even though the config loader accepts it.
    pub const ALL: [Self; 8] = [
        Self::Chat,
        Self::Transcription,
        Self::Embedding,
        Self::Image,
        Self::Speech,
        Self::Ocr,
        Self::Rerank,
        Self::SystemOne,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PickerStrategy {
    RoundRobin,
    #[default]
    LeastInflight,
    /// Send a conversation back to the replica that already holds its KV prefix,
    /// and only spread when load actually demands it.
    ///
    /// The right default for agent traffic against self-hosted replicas. Each
    /// vLLM instance caches prefixes separately, so a session whose turns
    /// alternate between two GPUs pays a full prefill on every turn — the
    /// balancing costs far more than it saves. This strategy maps a request's
    /// prefix (see [`crate::server::upstreams::affinity`]) onto a backend by
    /// weighted rendezvous hash, so the same conversation keeps landing on the
    /// same machine, while a backend that is genuinely busier than its peers
    /// still gets skipped.
    ///
    /// Requests with nothing to key on (embeddings, OCR, a body with no system
    /// prompt or user turn) fall back to `least_inflight`.
    PrefixAffinity,
}

impl PickerStrategy {
    /// The wire name, matching `#[serde(rename_all = "snake_case")]` and the
    /// strings `db_bridge::parse_strategy` accepts.
    ///
    /// Exists so nobody reaches for `format!("{:?}")` to produce one: that
    /// yields `LeastInflight`, which parses as unknown and silently falls back
    /// to the default while the admin UI shows a value not in its own dropdown.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RoundRobin => "round_robin",
            Self::LeastInflight => "least_inflight",
            Self::PrefixAffinity => "prefix_affinity",
        }
    }
}

#[derive(Debug, Clone)]
pub struct BackendConfig {
    pub priority: crate::server::priority::BackendPriority,
    pub name: String,
    pub base_url: String,
    pub api_key_env: Option<String>,
    /// The API key value itself: the decrypted stored key (see `db_bridge`).
    /// When present it takes precedence over `api_key_env`.
    pub api_key: Option<String>,
    pub weight: u32,
    pub max_inflight: u32,
    /// Custom health probe path. Defaults to `/models` (every OpenAI-compat
    /// server exposes it). For backends that don't, e.g. plain whisper.cpp,
    /// override here.
    pub health_path: String,
    /// The models this backend serves. Configured models are the catalog:
    /// the probe then only checks liveness and never discovers anything.
    ///
    /// Model resolution, highest first:
    ///   1. this backend's `models` (more specific than the pool's);
    ///   2. the pool's [`UpstreamPoolConfig::models`];
    ///   3. neither set → whatever the backend's `/models` probe reports.
    pub models: Vec<String>,
    /// Client-facing aliases this backend also answers to. An alias decouples
    /// the name clients send from the real model that's loaded: point clients
    /// at `qwen`, swap the loaded model, keep the alias, and nothing downstream
    /// changes. The same alias on several backends forms a load-balanced
    /// **group**. Both forms combine across backends into one group.
    ///
    /// Each alias maps to an optional explicit target. A bare alias (`None`)
    /// binds to the one model this backend serves; use that on single-model
    /// backends (the GPU norm). A targeted alias (`Some(real_id)`) names a
    /// specific real id; required on multi-model backends (e.g. a cloud
    /// provider serving many models behind one `base_url`), where a bare
    /// name couldn't tell which model it means. Empty means no aliases.
    ///
    /// See `docs/upstreams.md`.
    pub alias: HashMap<String, Option<String>>,
    /// Whether this backend may receive traffic. `false` is the maintenance
    /// switch: the backend keeps every setting and keeps being health-probed,
    /// but the picker skips it. Its models stay *known* to the gateway, so a
    /// request for one gets a sibling backend, or a temporary-outage answer —
    /// never "that model does not exist". Managed from `/admin/upstreams`
    /// (DB-backed).
    pub enabled: bool,
    /// Whether this backend supports image *editing* (image-to-image), not just
    /// text-to-image generation. Only meaningful on `kind = "image"` pools.
    /// Default `false` (text→image only, e.g. z.AI GLM-Image). A self-hosted
    /// edit-capable model (e.g. Qwen-Image-Edit) sets `true`, which is what
    /// lights up the `edit_image` tool / `/v1/images/edits` surface.
    pub supports_edit: bool,
    /// Operator-configured extra headers sent with every request the gateway
    /// makes to this backend — probe, identification, connection test and
    /// traffic alike. The escape hatch for upstream APIs that demand a header
    /// beyond the bearer credential (Anthropic: `anthropic-version`); see
    /// [`validate_extra_headers`]. Empty for every backend that does not need
    /// one.
    pub extra_headers: BTreeMap<String, String>,
}

/// The ceiling on one backend's configured extra headers.
pub const MAX_EXTRA_HEADERS: usize = 16;
/// The ceiling on a single extra header value.
pub const MAX_EXTRA_HEADER_VALUE_LEN: usize = 512;

/// Header names the gateway always sets itself on an upstream request. An
/// operator-supplied header may not shadow any of them — above all
/// `authorization`, where the stored key must stay authoritative, and
/// `content-length` / `transfer-encoding`, where a second value is a
/// malformed request.
const EXTRA_HEADER_DENYLIST: &[&str] = &[
    "authorization",
    "host",
    "content-type",
    "content-length",
    "transfer-encoding",
    "proxy-authorization",
];

/// Check one backend's configured extra headers. The admin API calls this
/// before saving (and before a connection test), so a malformed entry is a
/// `400` at the form rather than a silently broken probe five seconds later.
///
/// The message names the first violation and what to change — the same
/// contract as the other `bad_request` answers in the backend form.
pub fn validate_extra_headers(headers: &BTreeMap<String, String>) -> Result<(), String> {
    if headers.len() > MAX_EXTRA_HEADERS {
        return Err(format!(
            "at most {MAX_EXTRA_HEADERS} extra headers per backend, got {}",
            headers.len()
        ));
    }
    for (name, value) in headers {
        if name.trim().is_empty() {
            return Err("an extra header name may not be empty".into());
        }
        if !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~".contains(c))
        {
            return Err(format!(
                "extra header name `{name}` is not a valid HTTP token (alphanumerics and !#$%&'*+-.^_`|~ only)"
            ));
        }
        if EXTRA_HEADER_DENYLIST
            .iter()
            .any(|reserved| name.eq_ignore_ascii_case(reserved))
        {
            return Err(format!(
                "extra header `{name}` is set by the gateway itself and cannot be overridden"
            ));
        }
        if value.chars().count() > MAX_EXTRA_HEADER_VALUE_LEN {
            return Err(format!(
                "extra header value for `{name}` is longer than {MAX_EXTRA_HEADER_VALUE_LEN} characters"
            ));
        }
    }
    Ok(())
}

/// Attach a backend's configured extra headers to a request, after the
/// gateway's own credential. Every upstream request site calls this rather
/// than spelling the loop, so a new site cannot forget the headers (the
/// failure mode is a green form and a probe that 400s forever).
pub fn apply_extra_headers(
    req: reqwest::RequestBuilder,
    headers: &BTreeMap<String, String>,
) -> reqwest::RequestBuilder {
    headers
        .iter()
        .fold(req, |req, (name, value)| req.header(name, value))
}

/// Unknown-model fallback, keyed by request kind. When a request names a model
/// that is neither a real id nor any alias, the router substitutes the model
/// configured here for that kind (re-resolved through the normal path, single
/// hop). Answers "the client asked for something we've never heard of" — a typo
/// or a renamed model. An unset kind ⇒ the miss surfaces as `404
/// model_not_found`, as before. Per-kind because a chat model can't sensibly
/// rescue an embeddings or transcription miss. Distinct from a pool's
/// [`fallback_offline`](UpstreamPoolConfig::fallback_offline), which handles a
/// *known* model whose backends are all down.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FallbackConfig {
    pub chat: Option<String>,
    pub embedding: Option<String>,
    pub transcription: Option<String>,
    pub image: Option<String>,
}

impl FallbackConfig {
    /// The configured unknown-model fallback for `kind`, if any.
    pub fn for_kind(&self, kind: PoolKind) -> Option<&str> {
        match kind {
            PoolKind::Chat => self.chat.as_deref(),
            PoolKind::Embedding => self.embedding.as_deref(),
            PoolKind::Transcription => self.transcription.as_deref(),
            PoolKind::Image => self.image.as_deref(),
            // Speech has no unknown-model fallback: a mistyped voice/model just
            // surfaces the backend's own error. No sensible cross-substitution.
            PoolKind::Speech | PoolKind::Ocr | PoolKind::Rerank | PoolKind::SystemOne => None,
        }
    }
}

impl BackendConfig {
    /// A backend reachable at `base_url` with every other setting at its
    /// default: weight 1, 16 in-flight requests, probed at `/models`, enabled.
    pub fn new(name: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            priority: Default::default(),
            name: name.into(),
            base_url: base_url.into(),
            api_key_env: None,
            api_key: None,
            weight: 1,
            max_inflight: 16,
            health_path: "/models".into(),
            models: Vec::new(),
            alias: HashMap::new(),
            enabled: true,
            supports_edit: false,
            extra_headers: BTreeMap::new(),
        }
    }

    /// The effective API key: the direct [`api_key`](Self::api_key) value (from
    /// the DB, decrypted) wins; otherwise the referenced `api_key_env` var is
    /// read from the environment. `None` when neither yields a non-empty value.
    pub fn api_key(&self) -> Option<String> {
        self.api_key.clone().filter(|v| !v.is_empty()).or_else(|| {
            self.api_key_env
                .as_deref()
                .and_then(|name| std::env::var(name).ok())
                .filter(|v| !v.is_empty())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ocr_is_the_only_internal_kind() {
        let internal: Vec<PoolKind> = PoolKind::ALL
            .into_iter()
            .filter(|k| k.is_internal())
            .collect();
        assert_eq!(internal, vec![PoolKind::Ocr]);
    }

    fn pool(kind: PoolKind, backend: Vec<BackendConfig>) -> UpstreamPoolConfig {
        UpstreamPoolConfig::new(kind, backend)
    }

    #[test]
    fn speech_pool_resolves_the_voice_for_a_language() {
        let p = UpstreamPoolConfig {
            voices: HashMap::from([
                ("de".into(), "de-voice".into()),
                ("en".into(), "en-voice".into()),
                ("".into(), "fallback-voice".into()),
            ]),
            ..pool(
                PoolKind::Speech,
                vec![BackendConfig::new("tts", "http://tts:8000/v1")],
            )
        };
        assert_eq!(p.voice_for_language("de"), Some("de-voice"));
        assert_eq!(p.voice_for_language("en"), Some("en-voice"));
        assert_eq!(p.voice_for_language("fr"), Some("fallback-voice"));
    }

    #[test]
    fn voice_for_language_none_without_map() {
        let p = pool(
            PoolKind::Speech,
            vec![BackendConfig::new("tts", "http://tts:8000/v1")],
        );
        assert_eq!(p.voice_for_language("de"), None);
    }

    #[test]
    fn a_new_backend_carries_the_documented_defaults() {
        let b = BackendConfig::new("gpu-02", "http://gpu-02:8000/v1");
        assert_eq!(b.weight, 1);
        assert_eq!(b.max_inflight, 16);
        assert_eq!(b.health_path, "/models");
        assert!(b.enabled);
        assert!(!b.supports_edit);
        assert!(b.models.is_empty());
        assert!(b.alias.is_empty());
        assert!(b.extra_headers.is_empty());
    }

    #[test]
    fn extra_headers_validate_clean_for_the_anthropic_case() {
        let headers = BTreeMap::from([("anthropic-version".to_string(), "2023-06-01".to_string())]);
        assert!(validate_extra_headers(&headers).is_ok());
    }

    #[test]
    fn extra_headers_reject_names_the_gateway_sets_itself() {
        for name in [
            "authorization",
            "Authorization",
            "HOST",
            "content-type",
            "content-length",
            "transfer-encoding",
            "proxy-authorization",
        ] {
            let headers = BTreeMap::from([(name.to_string(), "x".to_string())]);
            let err = validate_extra_headers(&headers).unwrap_err();
            assert!(err.contains(name), "{name}: {err}");
        }
    }

    #[test]
    fn extra_headers_reject_invalid_token_names() {
        for name in [
            "",
            "  ",
            "has space",
            "colon:name",
            "semi;colon",
            "line\nbreak",
        ] {
            let headers = BTreeMap::from([(name.to_string(), "x".to_string())]);
            assert!(
                validate_extra_headers(&headers).is_err(),
                "{name:?} must be refused"
            );
        }
    }

    #[test]
    fn extra_headers_accept_the_full_token_character_set() {
        let headers = BTreeMap::from([("!#$%&'*+-.^_`|~a1".to_string(), "v".to_string())]);
        assert!(validate_extra_headers(&headers).is_ok());
    }

    #[test]
    fn extra_headers_enforce_the_count_and_value_ceilings() {
        let mut many = BTreeMap::new();
        for i in 0..=MAX_EXTRA_HEADERS {
            many.insert(format!("h{i}"), "v".to_string());
        }
        assert!(validate_extra_headers(&many).is_err());
        let mut ok = BTreeMap::new();
        ok.insert("h0".into(), "v".into());
        ok.insert("h1".into(), "x".repeat(MAX_EXTRA_HEADER_VALUE_LEN));
        assert!(validate_extra_headers(&ok).is_ok());
        ok.insert("h2".into(), "x".repeat(MAX_EXTRA_HEADER_VALUE_LEN + 1));
        assert!(validate_extra_headers(&ok).is_err());
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // test-only: the client is a RequestBuilder factory; nothing is sent
    fn apply_extra_headers_attaches_every_configured_header() {
        use reqwest::Method;

        let headers = BTreeMap::from([
            ("anthropic-version".to_string(), "2023-06-01".to_string()),
            ("x-custom".to_string(), "v".to_string()),
        ]);
        let request = apply_extra_headers(
            reqwest::Client::new().request(Method::GET, "http://x.invalid/v1"),
            &headers,
        )
        .build()
        .unwrap();
        assert_eq!(
            request.headers().get("anthropic-version").unwrap(),
            "2023-06-01"
        );
        assert_eq!(request.headers().get("x-custom").unwrap(), "v");
    }

    #[test]
    fn a_new_pool_carries_the_documented_defaults() {
        let p = pool(PoolKind::Chat, vec![BackendConfig::new("x", "http://x")]);
        assert_eq!(p.strategy, PickerStrategy::LeastInflight);
        assert!(p.models.is_empty());
        assert!(p.fallback_offline.is_none());
        assert!(p.enforce_limits);
        assert_eq!(p.compliance, Compliance::default());
        assert!(p.compliance.is_all_clear());
        assert!(p.compliance.gdpr && p.compliance.nda);
    }

    #[test]
    fn a_compliance_flag_set_false_leaves_the_other_true() {
        let c = Compliance {
            gdpr: false,
            ..Compliance::default()
        };
        assert!(!c.gdpr);
        assert!(c.nda);
        assert!(!c.is_all_clear());
    }

    #[test]
    fn fallback_config_reports_by_kind() {
        let fb = FallbackConfig {
            chat: Some("qwen".into()),
            embedding: Some("text-embedding-3-small".into()),
            ..FallbackConfig::default()
        };
        assert_eq!(fb.for_kind(PoolKind::Chat), Some("qwen"));
        assert_eq!(
            fb.for_kind(PoolKind::Embedding),
            Some("text-embedding-3-small")
        );
        assert_eq!(fb.for_kind(PoolKind::Transcription), None);
    }
}
