// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! What kind of server is behind a `base_url`, and what that implies.
//!
//! The gateway routes to anything that speaks the OpenAI wire, which is the
//! right abstraction for *requests* and the wrong one for two facts a request
//! depends on:
//!
//!   * **How much context does this model have?** vLLM answers it on
//!     `/models` (`max_model_len`); llama.cpp answers it in a different field
//!     there and a truer one on `/props`; Ollama answers nowhere the OpenAI
//!     surface can see, because its context is a server setting rather than a
//!     model attribute — but its own `/api/ps` states the figure actually
//!     allocated, which on a default install is 4096.
//!   * **How do I say "think harder"?** Every server spells it differently,
//!     and a server that does not know a spelling *discards it silently* —
//!     no 400, nothing to learn from, just an effort control that does
//!     nothing.
//!
//! Both were previously answered by assuming vLLM: the context came from a
//! vLLM-only field, and the reasoning spelling was guessed from the model
//! *name*. That guess is right on vLLM (which passes model-specific
//! parameters straight through) and wrong on Ollama (which normalises them
//! away), so the same `qwen3` behaved differently depending on something the
//! gateway never looked at.
//!
//! This module looks. [`detect`] fingerprints a backend with a handful of
//! cheap GETs and returns a [`BackendProfile`] plus whatever the server was
//! willing to say about itself. The operator never *configures* any of it —
//! the admin page shows what was identified, as a badge, but the context and
//! effort controls stay one vocabulary whatever is behind them. The profile
//! exists so those two can mean different bytes on the wire without meaning
//! different things to the user.
//!
//! Deliberately small. A profile answers only questions whose answer the
//! *server* determines. Anything a model determines (does it think at all,
//! does it do vision) stays with the model, where `model_defaults` and the
//! capability learner already handle it.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use serde::Deserialize;

use crate::server::reasoning::ReasoningStyle;

/// How long any single identification request may take.
///
/// Matched to the health probe's timeout deliberately. Detection runs
/// concurrently with the first probe round at startup and on every topology
/// apply, so anything longer would make *this* the thing an operator waits on
/// whenever a backend is unreachable — and it would be waiting to learn
/// nothing, since an unreachable server is `Generic` either way.
///
/// Two seconds is a generous bar for the servers this can actually identify:
/// they are local, and `/api/version` or `/props` answers in milliseconds. A
/// hosted provider has none of these endpoints and is correctly `Generic`
/// whether it refuses fast or never answers at all.
const DETECT_TIMEOUT: Duration = Duration::from_secs(2);

/// The kind of server behind a backend's `base_url`.
///
/// [`Generic`](Self::Generic) is not a failure — it is the honest answer for
/// every OpenAI-compatible server we have no specific knowledge of, including
/// hosted providers, and it reproduces exactly the behaviour the gateway had
/// before profiles existed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BackendProfile {
    /// Anything OpenAI-compatible we have nothing specific to say about:
    /// hosted providers, proxies, servers we failed to identify.
    #[default]
    Generic,
    /// vLLM. Reports `max_model_len` per model on `/models`.
    VLlm,
    /// Ollama. Normalises requests through its own API, so model-specific
    /// parameters never reach the model; its context is a server setting.
    Ollama,
    /// `llama-server` from llama.cpp, single-model or router mode.
    LlamaCpp,
    /// SGLang.
    SgLang,
}

impl BackendProfile {
    /// The canonical string, as stored and sent to the admin UI.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Generic => "generic",
            Self::VLlm => "vllm",
            Self::Ollama => "ollama",
            Self::LlamaCpp => "llamacpp",
            Self::SgLang => "sglang",
        }
    }

    /// Parse a stored value. Unknown / missing → [`Generic`](Self::Generic),
    /// which is the pre-profile behaviour, so a row written by a newer version
    /// degrades instead of breaking.
    pub fn parse(s: Option<&str>) -> Self {
        match s.map(str::trim) {
            Some("vllm") => Self::VLlm,
            Some("ollama") => Self::Ollama,
            Some("llamacpp") => Self::LlamaCpp,
            Some("sglang") => Self::SgLang,
            _ => Self::Generic,
        }
    }

    /// Whether this server honours `tool_choice`.
    ///
    /// The final tool round keeps the tool definitions in the request and
    /// relies on `tool_choice: "none"` to force a final answer (see
    /// `openai_driver::configure_final_tool_round`). Ollama's OpenAI layer has
    /// no `tool_choice` field at all, so the value is discarded and the model
    /// still sees its tools on the round that was supposed to end the turn —
    /// the "turn ends on a promise" failure, with nothing to detect it by.
    ///
    /// `false` here means the caller must withhold the tools themselves.
    /// Matched exhaustively so a server added later has to state its answer
    /// rather than inherit `true` from a wildcard — LM Studio, TGI and
    /// OpenRouter each have their own `tool_choice` story, and inheriting the
    /// optimistic one silently reproduces the Ollama bug for the next server.
    pub fn honors_tool_choice(self) -> bool {
        match self {
            Self::Ollama => false,
            Self::Generic | Self::VLlm | Self::LlamaCpp | Self::SgLang => true,
        }
    }

    /// The reasoning spelling this *server* dictates, if it dictates one.
    ///
    /// `None` means "ask the model", which is the right answer for every
    /// server that passes model-specific parameters through untouched: on
    /// vLLM, llama.cpp and SGLang a Qwen really does want Qwen's spelling and
    /// a GLM really does want GLM's, so the model name is the correct signal
    /// and [`ReasoningStyle::detect`] keeps deciding.
    ///
    /// Ollama is the exception. It re-encodes every request through its own
    /// API, so `chat_template_kwargs` never reaches the template no matter
    /// which model is loaded; what it does accept is `reasoning_effort` on a
    /// scale of its own, which is [`ReasoningStyle::Ollama`]. Without this the
    /// effort control is inert for every thinking model Ollama serves.
    pub fn reasoning_dialect(self) -> Option<ReasoningStyle> {
        match self {
            Self::Ollama => Some(ReasoningStyle::Ollama),
            Self::Generic | Self::VLlm | Self::LlamaCpp | Self::SgLang => None,
        }
    }

    /// The endpoint, relative to the server root, that states this server's
    /// *current* context allocation — or `None` when there is nothing extra to
    /// ask, because the figure already rides along on `/models`.
    ///
    /// This is read on every health tick rather than once at identification,
    /// and the difference matters more than it looks. What a server *is* only
    /// changes when someone reconfigures it; what it has currently *loaded* is
    /// runtime state. Ollama loads models on first use and unloads them on
    /// `keep_alive`, so `/api/ps` at apply time reports nothing at all — a
    /// freshly applied topology would have no window for any model, and every
    /// one of them would fall back to the 32768 guess against a 4096
    /// allocation. Which is the failure this module exists to remove, arriving
    /// through its own refresh policy.
    ///
    /// Matched exhaustively on purpose: a server added later must answer this
    /// question deliberately rather than inherit `None` from a wildcard.
    pub fn context_endpoint(self) -> Option<&'static str> {
        match self {
            Self::Ollama => Some("/api/ps"),
            // `/props` reports the context llama.cpp was actually started with,
            // which `/models`' `n_ctx_train` (the model's *trained* maximum)
            // does not.
            Self::LlamaCpp => Some("/props"),
            // vLLM puts `max_model_len` on `/models`, which the probe already
            // reads; SGLang likewise when it reports anything.
            Self::VLlm | Self::SgLang | Self::Generic => None,
        }
    }
}

/// What one detection round learned. Every field is "what the server said",
/// never "what the operator wants" — overrides live in the database and are
/// applied on top of this.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Detected {
    pub profile: BackendProfile,
    /// Context windows this round read, by model id.
    ///
    /// Identification-time only, and largely a boot seed: the *live* windows
    /// belong to the health probe, which re-reads them every tick because they
    /// are runtime state (see [`BackendProfile::context_endpoint`]). These are
    /// what gets persisted, so a gateway booting against an unreachable
    /// backend still knows a window rather than falling to the global
    /// assumption.
    ///
    /// Sources differ by server: vLLM's `max_model_len` and llama.cpp's
    /// `meta.n_ctx_train` come off `/models`, while Ollama's come off
    /// `/api/ps` and are the context the running instance actually allocated —
    /// so for Ollama this is precisely the loaded window, and for the others
    /// it is the model's maximum, which [`context_cap`](Self::context_cap)
    /// then bounds.
    ///
    /// Empty for a hosted provider, which reports no window anywhere.
    pub context_windows: HashMap<String, i64>,
    /// The context this server actually allocated, when it says so
    /// (llama.cpp's `/props`). A ceiling over [`context_windows`](Self::context_windows),
    /// not a replacement: a Gemma with `n_ctx_train` of 131072 started with
    /// `-c 8192` has 8192 tokens, and using the trained figure would put us
    /// right back at prompts the server silently truncates.
    ///
    /// Kept separate rather than folded into the map because the probe
    /// rewrites that map every tick and would otherwise restore the looser
    /// number each time.
    pub context_cap: Option<i64>,
    /// How many requests this server will genuinely run at once, when it says
    /// so. llama.cpp reports its slot count; nothing else we know of does.
    /// `None` means the configured `max_inflight` stands unchallenged.
    pub max_parallel: Option<u32>,
    /// The server's own version string, purely for the admin UI. Never parsed
    /// or compared — behaviour is decided by the profile, not by a version.
    pub version: Option<String>,
    /// When this was last identified, as stored. `None` on a fresh round —
    /// the database supplies it on the way back out.
    ///
    /// Shown to the operator because identification, unlike the context
    /// reading beside it, happens only when a topology is applied: "identified
    /// three weeks ago" is the difference between a profile that describes the
    /// server and one that describes the server it used to be.
    pub detected_at: Option<String>,
}

/// Identify `base_url` and read what it will tell us.
///
/// `None` means **nothing answered** — no socket, no HTTP status, nothing. The
/// caller must keep whatever it already knew rather than concluding anything,
/// because an unreachable server is not a plain OpenAI server.
///
/// `Some(Detected::default())` is a different answer: something *is* there and
/// is none of the servers we recognise, which is the truth for a hosted
/// provider. That result is allowed to overwrite a stale profile, and is what
/// lets a backend repointed from an Ollama box to a hosted API stop being
/// treated as Ollama.
///
/// `base_url` is the OpenAI base (`…/v1`); several probes need the server root
/// instead, which is why [`server_root`] exists.
pub async fn detect(
    http: &reqwest::Client,
    base_url: &str,
    api_key: Option<&str>,
) -> Option<Detected> {
    let base = base_url.trim_end_matches('/');
    let root = server_root(base);

    // One round trip each, all at once: identification is interactive (an
    // operator is watching a spinner) and these do not depend on each other.
    let (version_url, props_url, models_url, sglang_url, ps_url) = (
        format!("{root}/api/version"),
        format!("{root}/props"),
        format!("{base}/models"),
        format!("{root}/get_model_info"),
        format!("{root}/api/ps"),
    );
    let (ollama, props, models, sglang, ps) = tokio::join!(
        get_json(http, &version_url, api_key),
        get_json(http, &props_url, api_key),
        get_json(http, &models_url, api_key),
        get_json(http, &sglang_url, api_key),
        get_json(http, &ps_url, api_key),
    );

    let reached = [&ollama, &props, &models, &sglang, &ps]
        .iter()
        .any(|p| p.reached);
    let (ollama, props, models, sglang, ps) =
        (ollama.body, props.body, models.body, sglang.body, ps.body);
    if !reached {
        return None;
    }

    // Order matters: the first *positive* identification wins, and each test
    // keys on something only that server emits. `/v1/models` is last because
    // everyone serves it — it identifies vLLM only by a field, never by
    // existing.
    // Each test keys on a field only that server emits, never on "the endpoint
    // answered". A reverse proxy that returns 200 with an error body on every
    // unknown path would otherwise be classified as Ollama — and the
    // consequences are not cosmetic: every model behind it would get Ollama's
    // reasoning spelling instead of its own, and every final tool round would
    // have its tools stripped.
    let ollama_version = ollama
        .as_ref()
        .and_then(|v| v.get("version"))
        .and_then(|v| v.as_str())
        .filter(|v| !v.trim().is_empty());
    // Windows from `/models`, wanted by three of the four branches. Computed
    // once, and only the windows half is kept — the ids are the probe's
    // business, not identification's.
    let models_windows = models
        .as_ref()
        .and_then(|m| read_models(m).map(|(_, w)| w))
        .unwrap_or_default();

    if let Some(version) = ollama_version {
        return Some(Detected {
            profile: BackendProfile::Ollama,
            // Not on the OpenAI surface — Ollama's `/v1/models` carries no
            // window, because its context is a server setting
            // (`OLLAMA_CONTEXT_LENGTH`, divided by `OLLAMA_NUM_PARALLEL`)
            // rather than a model attribute. Its own `/api/ps` does report the
            // figure actually allocated, which is the number that matters and
            // is routinely the 4096 default. See [`ps_context_windows`].
            context_windows: ps.as_ref().map(ps_context_windows).unwrap_or_default(),
            version: Some(version.to_string()),
            ..Detected::default()
        });
    }

    if let Some(p) = props.as_ref() {
        // llama.cpp's `/props` is the only one of these shapes that carries a
        // slot count and a `default_generation_settings` block.
        if p.get("default_generation_settings").is_some() || p.get("total_slots").is_some() {
            return Some(Detected {
                profile: BackendProfile::LlamaCpp,
                context_windows: models_windows,
                context_cap: props_n_ctx(p),
                max_parallel: p
                    .get("total_slots")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|n| u32::try_from(n).ok())
                    .filter(|n| *n > 0),
                version: p
                    .get("build_info")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned),
                ..Detected::default()
            });
        }
    }

    // SGLang's `/get_model_info` names the model it loaded; an error body from
    // a proxy does not.
    let sglang_identified = sglang.as_ref().is_some_and(|v| {
        ["model_path", "served_model_names", "is_generation"]
            .iter()
            .any(|key| v.get(*key).is_some())
    });
    if sglang_identified {
        return Some(Detected {
            profile: BackendProfile::SgLang,
            context_windows: models_windows,
            ..Detected::default()
        });
    }

    // `max_model_len` is vLLM's; nobody else we have met emits it. An
    // OpenAI-shaped `/models` with no window at all stays Generic, which is the
    // truthful answer for a hosted provider.
    if !models_windows.is_empty() {
        return Some(Detected {
            profile: BackendProfile::VLlm,
            context_windows: models_windows,
            ..Detected::default()
        });
    }

    Some(Detected::default())
}

/// What `profile`'s context endpoint says right now: a window per loaded
/// model, and the server-wide allocation when it states one.
///
/// Called from the health probe on every tick, which is what keeps an Ollama
/// model's window arriving within five seconds of it being loaded instead of
/// waiting for the next topology apply. Cheap by construction — one GET
/// against an endpoint that answers from memory — and best-effort: a failure
/// returns nothing and leaves liveness alone.
pub async fn read_context(
    http: &reqwest::Client,
    base_url: &str,
    api_key: Option<&str>,
    profile: BackendProfile,
) -> Option<(HashMap<String, i64>, Option<i64>)> {
    let endpoint = profile.context_endpoint()?;
    let url = format!("{}{endpoint}", server_root(base_url.trim_end_matches('/')));
    // `None` when the endpoint did not answer usefully — a timeout, a 404
    // after an upgrade, a revoked key. The caller must then keep the figure it
    // had: overwriting with "nothing" drops every model on that backend onto
    // the assumed window, which is the silent truncation this exists to
    // prevent, arriving on a single bad tick.
    let body = get_json(http, &url, api_key).await.body?;
    Some(match profile {
        BackendProfile::Ollama => (ps_context_windows(&body), None),
        BackendProfile::LlamaCpp => (HashMap::new(), props_n_ctx(&body)),
        _ => (HashMap::new(), None),
    })
}

/// The server root for a backend whose `base_url` ends in the OpenAI prefix.
///
/// `/props`, `/api/version`, `/api/ps` and `/get_model_info` all live at the
/// root, not under `/v1` — asking for `…/v1/props` is a 404 on every server
/// that has one. `/tokenize` is the same convention, so
/// `messages::tokenize_url` builds on this rather than repeating the rule and
/// its edge cases.
pub fn server_root(base_url: &str) -> &str {
    let trimmed = base_url.trim_end_matches('/');
    trimmed.strip_suffix("/v1").unwrap_or(trimmed)
}

/// GET a URL and parse it as JSON. `None` for anything that is not a 2xx
/// carrying a JSON object — a 404 for an endpoint this server does not have
/// is the *expected* outcome of most of these calls, not an error worth
/// reporting.
/// One identification request's outcome.
///
/// `reached` is the distinction that decides whether a `Generic` result may
/// overwrite what we already knew. Collapsing "nothing answered" into "this is
/// an ordinary OpenAI server" is how a backend could never leave a profile: an
/// unreachable box looked exactly like a hosted provider, so the only safe
/// rule was to never overwrite — and a genuinely repointed backend then kept
/// the old server's profile forever.
struct Probe {
    reached: bool,
    body: Option<serde_json::Value>,
}

async fn get_json(http: &reqwest::Client, url: &str, api_key: Option<&str>) -> Probe {
    let mut req = http.get(url).header(
        "user-agent",
        concat!("llm-gateway/", env!("CARGO_PKG_VERSION"), " detect"),
    );
    if let Some(key) = api_key {
        req = req.bearer_auth(key);
    }
    // The timeout covers the body too, not just the headers. Bounding only
    // `send()` leaves `json()` unbounded, and neither client used here sets a
    // global timeout — so an upstream that answers 200 and then stalls
    // mid-body would hang this future forever. `spawn` awaits detection, so
    // that is not a slow probe: it is a gateway that never finishes starting,
    // and an admin "Test" request that never returns.
    tokio::time::timeout(DETECT_TIMEOUT, async {
        let Ok(resp) = req.send().await else {
            return Probe {
                reached: false,
                body: None,
            };
        };
        // An HTTP status — any status — means something is listening and
        // speaking HTTP. A 404 for an endpoint this server does not have is
        // the *expected* outcome of most of these calls.
        if !resp.status().is_success() {
            return Probe {
                reached: true,
                body: None,
            };
        }
        let body = resp
            .json::<serde_json::Value>()
            .await
            .ok()
            .filter(serde_json::Value::is_object);
        Probe {
            reached: true,
            body,
        }
    })
    .await
    .unwrap_or(Probe {
        reached: false,
        body: None,
    })
}

/// The `/models` entry shape, across the servers that put a window on it.
///
/// vLLM uses `max_model_len`; llama.cpp nests the model's trained context as
/// `meta.n_ctx_train`. Both are optional, and an entry carrying neither is
/// simply a model with no known window.
#[derive(Deserialize)]
struct ModelEntry {
    // No `id`: it is read straight off the raw value, so that a row whose
    // window field has an unexpected shape still yields a routable model.
    #[serde(default)]
    max_model_len: Option<i64>,
    #[serde(default)]
    meta: Option<ModelMeta>,
}

#[derive(Deserialize)]
struct ModelMeta {
    #[serde(default)]
    n_ctx_train: Option<i64>,
}

/// Everything a `/models` body says: the model ids it advertises, and the
/// context window for those that report one.
///
/// `None` means this was not an OpenAI model envelope at all — no `data`
/// array. That is a different thing from an envelope listing nothing, and the
/// caller must treat it differently: one leaves a backend's model set alone,
/// the other empties it.
///
/// One function rather than two because the health probe and detection read
/// the same response for different halves of it, and the halves drifted once
/// already: the probe understood only vLLM's `max_model_len`, so every
/// llama.cpp model fell through to the global 32768 guess even though the
/// answer was in the body it had just parsed.
///
/// Entries without a usable window are left out rather than defaulted —
/// "not reported" has to stay distinguishable from "reported as small",
/// because only one of those should make the admin page ask for a value.
pub fn read_models(body: &serde_json::Value) -> Option<(HashSet<String>, HashMap<String, i64>)> {
    let data = body.get("data")?.as_array()?;
    let mut ids = HashSet::new();
    let mut windows = HashMap::new();
    for raw in data {
        // Per entry, not the whole array at once. Deserialising
        // `Vec<ModelEntry>` in one go means a single odd row — a
        // `"max_model_len": "32768"` sent as a string, a `meta` that is not an
        // object — fails every row with it. The probe would then replace a
        // backend's advertised set with an empty one and make it unroutable,
        // where the honest answer is "that row made no sense, the others are
        // fine".
        // The id is read straight off the raw value, never through the typed
        // entry. A row whose *window* field has an unexpected shape is still a
        // model the backend serves, and dropping it with its window made that
        // model unroutable — a live model taken out of service by one odd
        // field, with nothing in any log.
        let Some(id) = raw
            .get("id")
            .and_then(serde_json::Value::as_str)
            .filter(|id| !id.is_empty())
        else {
            continue;
        };
        // Borrowed, not cloned: `&Value` is itself a `Deserializer`, so the
        // per-row tolerance costs nothing. Cloning here deep-copied every
        // entry's whole sub-tree — `permission` arrays and all — once per
        // model per backend on a five-second loop.
        if let Ok(entry) = ModelEntry::deserialize(raw)
            && let Some(window) = entry
                .max_model_len
                .or_else(|| entry.meta.as_ref().and_then(|meta| meta.n_ctx_train))
                .filter(|w| *w > 0)
        {
            windows.insert(id.to_string(), window);
        }
        ids.insert(id.to_string());
    }
    Some((ids, windows))
}

/// Context windows from Ollama's `/api/ps`, by model id.
///
/// The only place Ollama states a context, and it states the one that counts:
/// what the running instance actually allocated, after
/// `OLLAMA_CONTEXT_LENGTH` and the `OLLAMA_NUM_PARALLEL` division. Verified
/// against Ollama 0.34.0, where a default install reports 4096 — a figure far
/// enough below the gateway's 32768 fallback that believing the fallback is
/// what silently truncated long conversations.
///
/// Only *loaded* models appear. A model nobody has used since the server
/// started is absent rather than zero, which is the distinction the admin page
/// needs in order to ask for a value instead of showing a wrong one.
fn ps_context_windows(ps: &serde_json::Value) -> HashMap<String, i64> {
    let Some(models) = ps.get("models").and_then(|m| m.as_array()) else {
        return HashMap::new();
    };
    models
        .iter()
        .filter_map(|entry| {
            let id = entry
                .get("model")
                .or_else(|| entry.get("name"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())?;
            let window = entry
                .get("context_length")
                .and_then(serde_json::Value::as_i64)
                .filter(|w| *w > 0)?;
            Some((id.to_string(), window))
        })
        .collect()
}

/// llama.cpp's allocated context from `/props`. It has lived in two places
/// across versions, so try both rather than pin one.
fn props_n_ctx(props: &serde_json::Value) -> Option<i64> {
    props
        .pointer("/default_generation_settings/n_ctx")
        .or_else(|| props.get("n_ctx"))
        .and_then(serde_json::Value::as_i64)
        .filter(|n| *n > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn profile_strings_round_trip() {
        for profile in [
            BackendProfile::Generic,
            BackendProfile::VLlm,
            BackendProfile::Ollama,
            BackendProfile::LlamaCpp,
            BackendProfile::SgLang,
        ] {
            assert_eq!(BackendProfile::parse(Some(profile.as_str())), profile);
        }
    }

    /// A value from a newer version (or a corrupted row) must degrade to the
    /// pre-profile behaviour, never to a wrong specific profile.
    #[test]
    fn unknown_profile_degrades_to_generic() {
        assert_eq!(BackendProfile::parse(None), BackendProfile::Generic);
        assert_eq!(
            BackendProfile::parse(Some("something-new")),
            BackendProfile::Generic
        );
    }

    /// Ollama is the only server whose *wire* dictates the reasoning spelling.
    /// Everyone else passes model parameters through, so the model name stays
    /// the right signal and the dialect must not override it.
    #[test]
    fn only_ollama_dictates_a_reasoning_dialect() {
        assert_eq!(
            BackendProfile::Ollama.reasoning_dialect(),
            Some(ReasoningStyle::Ollama)
        );
        for profile in [
            BackendProfile::Generic,
            BackendProfile::VLlm,
            BackendProfile::LlamaCpp,
            BackendProfile::SgLang,
        ] {
            assert_eq!(profile.reasoning_dialect(), None, "{profile:?}");
        }
    }

    #[test]
    fn only_ollama_ignores_tool_choice() {
        assert!(!BackendProfile::Ollama.honors_tool_choice());
        for profile in [
            BackendProfile::Generic,
            BackendProfile::VLlm,
            BackendProfile::LlamaCpp,
            BackendProfile::SgLang,
        ] {
            assert!(profile.honors_tool_choice(), "{profile:?}");
        }
    }

    #[test]
    fn server_root_strips_the_openai_prefix_only() {
        assert_eq!(server_root("http://h:8080/v1"), "http://h:8080");
        assert_eq!(server_root("http://h:8080/v1/"), "http://h:8080");
        assert_eq!(server_root("http://h:8080"), "http://h:8080");
        // A path that merely contains "v1" is not the prefix.
        assert_eq!(server_root("http://h/v1beta"), "http://h/v1beta");
    }

    #[test]
    fn vllm_windows_come_from_max_model_len() {
        let body = json!({"data": [
            {"id": "qwen3", "max_model_len": 262144},
            {"id": "small", "max_model_len": 4096},
        ]});
        let windows = read_models(&body).unwrap().1;
        assert_eq!(windows.get("qwen3"), Some(&262144));
        assert_eq!(windows.get("small"), Some(&4096));
    }

    /// llama.cpp puts the model's trained context one level down. Reading only
    /// vLLM's spelling is what left every llama.cpp model on the 32768 guess.
    #[test]
    fn llamacpp_windows_come_from_meta_n_ctx_train() {
        let body = json!({"data": [
            {"id": "gemma.gguf", "meta": {"n_ctx_train": 131072}},
        ]});
        assert_eq!(
            read_models(&body).unwrap().1.get("gemma.gguf"),
            Some(&131072)
        );
    }

    /// One malformed row must not take the others with it. Deserialising the
    /// whole array at once did exactly that, and the probe then replaced the
    /// backend's advertised set with an empty one — a live backend turned
    /// unroutable by a single odd field.
    #[test]
    fn one_unparseable_entry_does_not_discard_the_rest() {
        let body = json!({"data": [
            {"id": "good", "max_model_len": 32768},
            // A string where a number belongs, and a `meta` that is not an
            // object: both plausible from a server we have not met.
            {"id": "stringly", "max_model_len": "32768"},
            {"id": "odd-meta", "meta": "not-an-object"},
            {"id": "also-good", "max_model_len": 8192},
        ]});
        let (ids, windows) = read_models(&body).unwrap();
        assert_eq!(windows.get("good"), Some(&32768));
        assert_eq!(windows.get("also-good"), Some(&8192));
        // And the odd rows keep their ids: a model whose *window* is
        // unreadable is still a model the backend serves. Dropping it made a
        // live model unroutable over one malformed field.
        assert_eq!(ids.len(), 4, "got {ids:?}");
        assert!(!windows.contains_key("stringly"));
    }

    /// A body with no `data` array is not an empty catalogue — it is not a
    /// catalogue. The caller has to tell those apart: one leaves a backend's
    /// model set alone, the other empties it.
    #[test]
    fn a_non_envelope_body_is_distinguishable_from_an_empty_one() {
        assert_eq!(read_models(&json!({"error": "nope"})), None);
        assert_eq!(read_models(&json!({"data": "not-an-array"})), None);
        let (ids, windows) = read_models(&json!({"data": []})).unwrap();
        assert!(ids.is_empty() && windows.is_empty());
    }

    /// "Not reported" and "reported as zero" must not collapse into the same
    /// thing: one means ask the operator, the other would be a 0-token budget.
    #[test]
    fn missing_and_nonpositive_windows_are_omitted() {
        let body = json!({"data": [
            {"id": "plain"},
            {"id": "zero", "max_model_len": 0},
            {"id": "negative", "max_model_len": -1},
            {"id": "", "max_model_len": 8192},
        ]});
        let (ids, windows) = read_models(&body).unwrap();
        assert!(windows.is_empty());
        // The ids still come back — a model without a window is still a model.
        assert_eq!(ids.len(), 3);
    }

    #[test]
    fn props_n_ctx_reads_either_location() {
        assert_eq!(
            props_n_ctx(&json!({"default_generation_settings": {"n_ctx": 8192}})),
            Some(8192)
        );
        assert_eq!(props_n_ctx(&json!({"n_ctx": 4096})), Some(4096));
        assert_eq!(props_n_ctx(&json!({"total_slots": 4})), None);
    }

    /// Only the servers that keep their allocation somewhere else are asked
    /// for it; vLLM and SGLang put it on `/models`, which the probe already
    /// reads.
    #[test]
    fn only_the_servers_with_a_separate_context_endpoint_are_asked() {
        assert_eq!(BackendProfile::Ollama.context_endpoint(), Some("/api/ps"));
        assert_eq!(BackendProfile::LlamaCpp.context_endpoint(), Some("/props"));
        assert_eq!(BackendProfile::VLlm.context_endpoint(), None);
        assert_eq!(BackendProfile::SgLang.context_endpoint(), None);
        assert_eq!(BackendProfile::Generic.context_endpoint(), None);
    }
}

#[cfg(test)]
mod detect_tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Mount a JSON GET route on `server`.
    async fn route(server: &MockServer, route: &str, body: serde_json::Value) {
        Mock::given(method("GET"))
            .and(path(route))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(server)
            .await;
    }

    /// `base_url` as an operator writes it: the OpenAI base, with `/v1`.
    fn base(server: &MockServer) -> String {
        format!("{}/v1", server.uri())
    }

    /// Ollama is identified by `/api/version`. Its `/v1/models` carries no
    /// window — the context is a server setting, not a model attribute — but
    /// `/api/ps` states what the running instance actually allocated, and that
    /// is the number a prompt has to fit. Shape taken from a live Ollama
    /// 0.34.0.
    #[tokio::test]
    async fn an_ollama_server_states_its_allocated_context_on_api_ps() {
        let server = MockServer::start().await;
        route(&server, "/api/version", json!({"version": "0.34.0"})).await;
        route(
            &server,
            "/v1/models",
            json!({"object": "list", "data": [{"id": "qwen3:0.6b"}]}),
        )
        .await;
        route(
            &server,
            "/api/ps",
            json!({"models": [
                {"name": "qwen3:0.6b", "model": "qwen3:0.6b", "context_length": 4096}
            ]}),
        )
        .await;

        let detected = detect(&reqwest::Client::new(), &base(&server), None)
            .await
            .expect("the mock answered, so identification must not report silence");
        assert_eq!(detected.profile, BackendProfile::Ollama);
        assert_eq!(detected.version.as_deref(), Some("0.34.0"));
        assert_eq!(detected.context_windows.get("qwen3:0.6b"), Some(&4_096));
        assert_eq!(detected.context_cap, None);
        // The consequences the rest of the gateway acts on.
        assert_eq!(
            detected.profile.reasoning_dialect(),
            Some(ReasoningStyle::Ollama)
        );
        assert!(!detected.profile.honors_tool_choice());
    }

    /// `/api/ps` lists only models the server currently has loaded. A model
    /// nobody has used is absent rather than zero — the distinction that lets
    /// the admin page ask for a value instead of showing a wrong one.
    #[tokio::test]
    async fn an_ollama_with_nothing_loaded_reports_no_window() {
        let server = MockServer::start().await;
        route(&server, "/api/version", json!({"version": "0.34.0"})).await;
        route(&server, "/api/ps", json!({"models": []})).await;
        route(
            &server,
            "/v1/models",
            json!({"object": "list", "data": [{"id": "qwen3:0.6b"}]}),
        )
        .await;

        let detected = detect(&reqwest::Client::new(), &base(&server), None)
            .await
            .expect("the mock answered, so identification must not report silence");
        assert_eq!(detected.profile, BackendProfile::Ollama);
        assert!(detected.context_windows.is_empty());
    }

    /// llama.cpp is identified by `/props`, and the context it actually
    /// allocated caps the trained figure `/v1/models` advertises. Believing
    /// `n_ctx_train` is how prompts end up past what the server will hold.
    #[tokio::test]
    async fn a_llamacpp_server_reports_its_allocated_context_as_a_cap() {
        let server = MockServer::start().await;
        route(
            &server,
            "/props",
            json!({
                "build_info": "b1234",
                "total_slots": 4,
                "default_generation_settings": {"n_ctx": 8192}
            }),
        )
        .await;
        route(
            &server,
            "/v1/models",
            json!({"object": "list", "data": [
                {"id": "gemma.gguf", "meta": {"n_ctx_train": 131072}}
            ]}),
        )
        .await;

        let detected = detect(&reqwest::Client::new(), &base(&server), None)
            .await
            .expect("the mock answered, so identification must not report silence");
        assert_eq!(detected.profile, BackendProfile::LlamaCpp);
        assert_eq!(detected.version.as_deref(), Some("b1234"));
        assert_eq!(detected.max_parallel, Some(4));
        // Reported and allocated are kept apart; the registry applies the cap.
        assert_eq!(detected.context_windows.get("gemma.gguf"), Some(&131_072));
        assert_eq!(detected.context_cap, Some(8_192));
        assert_eq!(detected.profile.reasoning_dialect(), None);
        assert!(detected.profile.honors_tool_choice());
    }

    /// vLLM has no identifying endpoint — it is recognised by the field only it
    /// puts on `/models`.
    #[tokio::test]
    async fn a_vllm_server_is_identified_by_max_model_len() {
        let server = MockServer::start().await;
        route(
            &server,
            "/v1/models",
            json!({"object": "list", "data": [
                {"id": "Qwen/Qwen3", "max_model_len": 262144}
            ]}),
        )
        .await;

        let detected = detect(&reqwest::Client::new(), &base(&server), None)
            .await
            .expect("the mock answered, so identification must not report silence");
        assert_eq!(detected.profile, BackendProfile::VLlm);
        assert_eq!(detected.context_windows.get("Qwen/Qwen3"), Some(&262_144));
        assert_eq!(detected.context_cap, None);
    }

    /// Identification keys on a field only that server emits, never on "the
    /// endpoint answered". A proxy that returns 200 with an error body on
    /// every unknown path must not be mistaken for Ollama — doing so would
    /// give every model behind it the wrong reasoning spelling and strip the
    /// tools from every final round.
    #[tokio::test]
    async fn a_proxy_that_answers_everything_is_not_mistaken_for_a_known_server() {
        let server = MockServer::start().await;
        for endpoint in ["/api/version", "/props", "/get_model_info", "/api/ps"] {
            route(&server, endpoint, json!({"error": "unknown path"})).await;
        }
        route(
            &server,
            "/v1/models",
            json!({"object": "list", "data": [{"id": "m"}]}),
        )
        .await;

        let detected = detect(&reqwest::Client::new(), &base(&server), None)
            .await
            .expect("the mock answered, so identification must not report silence");
        assert_eq!(detected.profile, BackendProfile::Generic);
    }

    /// An empty version string is no more an identification than a missing one.
    #[tokio::test]
    async fn an_empty_version_does_not_identify_ollama() {
        let server = MockServer::start().await;
        route(&server, "/api/version", json!({"version": "  "})).await;
        route(
            &server,
            "/v1/models",
            json!({"object": "list", "data": [{"id": "m"}]}),
        )
        .await;

        let detected = detect(&reqwest::Client::new(), &base(&server), None)
            .await
            .expect("the mock answered, so identification must not report silence");
        assert_eq!(detected.profile, BackendProfile::Generic);
    }

    /// A hosted provider serves `/models` and nothing else. `Generic` is the
    /// truthful answer, and it reproduces the pre-profile behaviour exactly:
    /// no dialect (the model name decides), `tool_choice` trusted, no context.
    #[tokio::test]
    async fn a_plain_openai_server_stays_generic() {
        let server = MockServer::start().await;
        route(
            &server,
            "/v1/models",
            json!({"object": "list", "data": [{"id": "gpt-5"}]}),
        )
        .await;

        let detected = detect(&reqwest::Client::new(), &base(&server), None)
            .await
            .expect("the mock answered, so identification must not report silence");
        assert_eq!(detected, Detected::default());
        assert_eq!(detected.profile.reasoning_dialect(), None);
        assert!(detected.profile.honors_tool_choice());
    }

    /// An unreachable server is *silence*, not an answer.
    ///
    /// Collapsing the two is how a backend could never leave a profile: an
    /// unreachable box looked exactly like a hosted provider, so the only safe
    /// rule was to never overwrite — and a genuinely repointed backend then
    /// kept the old server's profile forever.
    #[tokio::test]
    async fn an_unreachable_server_reports_silence_not_generic() {
        // A bound-then-dropped port: nothing is listening, so every probe
        // fails at connect rather than waiting out the timeout.
        let dead = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = dead.local_addr().unwrap();
        drop(dead);

        let detected = detect(&reqwest::Client::new(), &format!("http://{addr}/v1"), None).await;
        assert_eq!(
            detected, None,
            "nothing answered, so nothing may be concluded"
        );
    }

    /// The identifying endpoints live at the server root, not under `/v1` —
    /// asking for `…/v1/props` is a 404 on every server that has one.
    #[tokio::test]
    async fn identification_probes_the_server_root_not_the_openai_prefix() {
        let server = MockServer::start().await;
        // Mounted under /v1, where these endpoints never actually live.
        route(&server, "/v1/props", json!({"total_slots": 4})).await;
        route(&server, "/v1/api/version", json!({"version": "0.13.3"})).await;
        route(
            &server,
            "/v1/models",
            json!({"object": "list", "data": [{"id": "m"}]}),
        )
        .await;

        let detected = detect(&reqwest::Client::new(), &base(&server), None)
            .await
            .expect("the mock answered, so identification must not report silence");
        assert_eq!(
            detected.profile,
            BackendProfile::Generic,
            "a /v1-prefixed props must not be mistaken for llama.cpp's"
        );
    }
}

/// Identification and context reading against a **live** server, opt-in.
///
/// Everything else here runs against mock responses, which proves the parsing
/// and the URL construction but not the premise: that a real Ollama answers
/// the way its source and documentation say it does. It does not — not
/// entirely. The mocks were first written believing Ollama could not state a
/// context window at all; running one showed `/api/ps` reporting the allocated
/// 4096, which is the single most useful number this module produces. Running
/// one *again* showed that endpoint empty for a model that has been pulled but
/// never used, which is what moved the context reading out of identification
/// and onto the health probe.
///
/// So this stays, ignored by default, for the next time the premise needs
/// re-checking against a version we have not seen:
///
/// ```text
/// docker run -d --name ollama -p 11500:11434 ollama/ollama
/// docker exec ollama ollama pull qwen3:0.6b
/// GATEWAY_LIVE_OLLAMA=http://localhost:11500/v1 \
///   cargo nextest run -p gateway-core live_ollama --run-ignored all
/// ```
#[cfg(test)]
mod live_ollama {
    use super::*;

    fn base_url() -> String {
        std::env::var("GATEWAY_LIVE_OLLAMA")
            .expect("set GATEWAY_LIVE_OLLAMA to a running Ollama's /v1 base url")
    }

    /// Identification works whether or not anything is loaded — it asks
    /// `/api/version`, which is always there.
    #[tokio::test]
    #[ignore = "needs a running Ollama; set GATEWAY_LIVE_OLLAMA to its /v1 base url"]
    async fn a_real_ollama_is_identified() {
        let detected = detect(&reqwest::Client::new(), &base_url(), None)
            .await
            .expect("a running Ollama must answer");

        assert_eq!(detected.profile, BackendProfile::Ollama);
        assert!(
            detected.version.is_some(),
            "/api/version should carry a version string"
        );
        assert_eq!(
            detected.profile.reasoning_dialect(),
            Some(ReasoningStyle::Ollama)
        );
        assert!(!detected.profile.honors_tool_choice());
        assert_eq!(detected.profile.context_endpoint(), Some("/api/ps"));
    }

    /// The context a model actually got, read the way the probe reads it.
    ///
    /// Loads the model first, because that is the whole point: a pulled but
    /// unused model is absent from `/api/ps`, so a reading taken at apply time
    /// would find nothing. Verified against Ollama 0.34.0, where a default
    /// install reports 4096 — far enough below the gateway's 32768 fallback
    /// that believing the fallback is what silently truncated conversations.
    #[tokio::test]
    #[ignore = "needs a running Ollama; set GATEWAY_LIVE_OLLAMA to its /v1 base url"]
    async fn a_loaded_model_states_the_context_it_was_given() {
        let base = base_url();
        let http = reqwest::Client::new();

        // Use the model once, which is what makes Ollama load it.
        let models = get_json(
            &http,
            &format!("{}/models", base.trim_end_matches('/')),
            None,
        )
        .await
        .body
        .expect("/v1/models should answer");
        let (ids, _) = read_models(&models).expect("an OpenAI model envelope");
        let model = ids.iter().next().expect("pull a model first").clone();
        let _ = http
            .post(format!("{}/chat/completions", base.trim_end_matches('/')))
            .json(&serde_json::json!({
                "model": model,
                "messages": [{"role": "user", "content": "hi"}],
                "max_tokens": 1,
                "reasoning_effort": "none",
            }))
            .send()
            .await;

        let (windows, cap) = read_context(&http, &base, None, BackendProfile::Ollama)
            .await
            .expect("/api/ps must answer on a running Ollama");
        assert_eq!(cap, None, "Ollama states no server-wide cap");
        let window = windows
            .get(&model)
            .copied()
            .unwrap_or_else(|| panic!("no window for {model}; /api/ps said {windows:?}"));
        assert!(window > 0, "{model} reported a non-positive window");
    }
}
