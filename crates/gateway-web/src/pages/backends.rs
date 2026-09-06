// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Backend CRUD write handlers for the DB-backed upstream topology.
//!
//! A backend is a single OpenAI-compatible upstream (base URL, sealed API key,
//! weight, in-flight cap, health path, and a static model/alias list for
//! backends that don't expose `/models`). Edits are written to the DB via
//! [`gateway_core::server::db::upstreams_config`] but only take effect on the runtime
//! registry when the admin clicks "Apply changes" (POST
//! `/admin/upstreams/reload`).
//!
//! The page these handlers back is now [`super::upstreams`] (`/admin/upstreams`,
//! which merged the old `/admin/pools` + `/admin/backends`); this module keeps
//! only the POST handlers (their paths are unchanged) plus the aliases textarea
//! parse/serialise helpers and the runtime-health sparkline the upstreams page
//! renders.
//!
//! Every save/delete bumps the in-memory topology-dirty counter
//! ([`RamaState::topology_dirty_bump`]) and patches the `topologyDirty` datastar
//! signal so the apply bar updates without a reload.
//!
//! Gated on the `admin` role via [`super::require_admin_or_403`], same as the
//! other operator pages.

use std::sync::Arc;

use rama::http::service::web::extract::State;
use rama::http::{Request, Response};

use super::{
    checkbox_on, dirty_signal, field, overwrite_clear, overwrite_prompt, parse_csv, parse_u32,
    read_form, require_admin_or_403, toast,
};
use session_core::chrome::{FlashKind, sse_response, sse_toast};
use session_core::i18n::{self, Lang, t, t_args};

use gateway_core::server::db::upstreams_config::{self, AliasRow, BackendRow};
use gateway_runtime::rama_server::state::RamaState;

/// Serialise a backend's aliases back into the textarea format
/// (`name=target` per line, or a bare `name` for a target-less alias). Used by
/// the upstreams editor to pre-fill the aliases textarea.
pub(super) fn alias_lines(aliases: &[AliasRow]) -> String {
    aliases
        .iter()
        .map(|a| match &a.target {
            Some(t) => format!("{}={t}", a.alias),
            None => a.alias.clone(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Parse the aliases textarea: one `name=target` per line, or a bare `name`
/// for a target-less alias (binds to the backend's sole model at request time).
fn parse_aliases(v: &str) -> Vec<AliasRow> {
    v.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            match line.split_once('=') {
                Some((alias, target)) => Some(AliasRow {
                    alias: alias.trim().to_string(),
                    target: Some(target.trim().to_string()),
                }),
                None => Some(AliasRow {
                    alias: line.to_string(),
                    target: None,
                }),
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Backend CRUD handlers (POST) — write to the DB topology; the registry only
// picks the change up on POST /admin/upstreams/reload ("Apply changes").
// ---------------------------------------------------------------------------

/// POST /admin/backends/save — insert or update a backend from the editor form.
/// A blank `weight`/`max_inflight` falls back to the config defaults (1 / 16),
/// a blank `health_path` to `/models`. Reminds the admin to click "Apply
/// changes" since the registry isn't reloaded here.
pub async fn backends_save(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let lang = Lang::from_headers(req.headers());
    if let Err(resp) = require_admin_or_403(&state, &req).await {
        return resp;
    }
    let (_, body) = req.into_parts();
    let pairs: Vec<(String, String)> = match read_form(body).await {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    let name = field(&pairs, "name").trim();
    if name.is_empty() {
        return toast(FlashKind::Error, t(lang, "backends-error-name-required"));
    }
    // Add form + an existing name = an accidental overwrite: `name` is the
    // primary key, this is an upsert, and `set_backend_pool` below would move
    // the *existing* backend into the chosen pool — so "add a second backend to
    // this pool" silently ended up as one rewritten backend. Refuse the first
    // save and arm the form's confirmation; the next click carries
    // `overwrite=1`.
    if field(&pairs, "mode").trim() == "add" && !checkbox_on(field(&pairs, "overwrite")) {
        match upstreams_config::backend_exists(&state.db, name).await {
            Ok(true) => {
                return overwrite_prompt(
                    t_args(
                        lang,
                        "backends-error-name-exists",
                        &i18n::args([("name", name.to_string().into())]),
                    ),
                    "ovwBackend",
                );
            }
            Ok(false) => {}
            Err(e) => {
                return toast(
                    FlashKind::Error,
                    t_args(
                        lang,
                        "admin-db-error",
                        &i18n::args([("err", e.to_string().into())]),
                    ),
                );
            }
        }
    }
    let base_url = field(&pairs, "base_url").trim();
    if base_url.is_empty() {
        return toast(
            FlashKind::Error,
            t(lang, "backends-error-base-url-required"),
        );
    }
    let health_path = {
        let h = field(&pairs, "health_path").trim();
        if h.is_empty() {
            "/models".to_string()
        } else {
            h.to_string()
        }
    };
    let api_key_env = {
        let v = field(&pairs, "api_key_env").trim();
        (!v.is_empty()).then(|| v.to_string())
    };
    // API key value: a freshly entered key is sealed and stored; a blank field
    // on an existing backend keeps the current key (the form never echoes the
    // secret back), and a blank field on a new backend means "no stored key"
    // (the `api_key_env` fallback still applies). This is what lets an operator
    // add a backend with its key at runtime, no restart / new env var needed.
    let (api_key_ct, api_key_nonce) = {
        let entered = field(&pairs, "api_key").trim();
        if !entered.is_empty() {
            match state.crypto.seal_str(entered) {
                Ok(s) => (Some(s.ciphertext), Some(s.nonce)),
                Err(e) => {
                    return toast(
                        FlashKind::Error,
                        t_args(
                            lang,
                            "admin-db-upsert-error",
                            &i18n::args([("err", e.to_string().into())]),
                        ),
                    );
                }
            }
        } else {
            match upstreams_config::get_backend(&state.db, name).await {
                Ok(Some(existing)) => (existing.api_key_ct, existing.api_key_nonce),
                _ => (None, None),
            }
        }
    };
    // The maintenance switch has its own form, so the editor must preserve
    // whatever it is currently set to — a save is not a reason to put a drained
    // backend back into rotation. New backends start enabled.
    let enabled = match upstreams_config::get_backend(&state.db, name).await {
        Ok(Some(existing)) => existing.enabled,
        _ => true,
    };
    let row = BackendRow {
        name: name.to_string(),
        base_url: base_url.to_string(),
        api_key_env,
        api_key_ct,
        api_key_nonce,
        weight: parse_u32(field(&pairs, "weight"), 1),
        max_inflight: parse_u32(field(&pairs, "max_inflight"), 16),
        health_path,
        probe_models: checkbox_on(field(&pairs, "probe_models")),
        supports_edit: checkbox_on(field(&pairs, "supports_edit")),
        enabled,
        models: parse_csv(field(&pairs, "models")),
        aliases: parse_aliases(field(&pairs, "aliases")),
        created_at: jiff::Timestamp::now(),
        updated_at: jiff::Timestamp::now(),
    };
    if let Err(e) = upstreams_config::upsert_backend(&state.db, &row).await {
        return toast(
            FlashKind::Error,
            t_args(
                lang,
                "admin-db-upsert-error",
                &i18n::args([("err", e.to_string().into())]),
            ),
        );
    }
    // Single "Pool" select: set this backend's membership to exactly the chosen
    // pool (empty = none). Only when the form actually carried the field, so a
    // stale form missing it can't silently unassign the backend.
    if pairs.iter().any(|(k, _)| k == "pool") {
        let pool = {
            let p = field(&pairs, "pool").trim();
            (!p.is_empty()).then_some(p)
        };
        if let Err(e) = upstreams_config::set_backend_pool(&state.db, name, pool).await {
            return toast(
                FlashKind::Error,
                t_args(
                    lang,
                    "admin-db-upsert-error",
                    &i18n::args([("err", e.to_string().into())]),
                ),
            );
        }
    }
    let dirty = state.topology_dirty_bump();
    let mut events = vec![sse_toast(&session_core::chrome::Flash {
        kind: FlashKind::Success,
        message: t_args(
            lang,
            "backends-saved",
            &i18n::args([("name", name.to_string().into())]),
        ),
    })];
    // Catch a mistyped alias target at the moment it is typed. An alias only
    // resolves while its target is actually advertised, so pointing one at an id
    // the backend doesn't serve produces an alias that looks configured
    // everywhere and routes nowhere — with the real model id often being a full
    // repo path (`unsloth/Qwen3.8-27B-NVFP4`) that nobody types from memory.
    if let Some(warning) = unknown_alias_targets(&state, name, &row.aliases, lang) {
        events.push(sse_toast(&session_core::chrome::Flash {
            kind: FlashKind::Error,
            message: warning,
        }));
    }
    events.push(dirty_signal(dirty));
    events.push(overwrite_clear("ovwBackend"));
    sse_response(&events)
}

/// The "these alias targets are not served" warning for a just-saved backend,
/// or `None` when every target checks out (or there is nothing to check against).
///
/// Checked against the **running** registry, which is the only place that knows
/// what the backend actually advertises. Skipped entirely for a backend the
/// registry doesn't know yet (freshly added, not applied) or one that has
/// advertised nothing so far — there is no truth to compare against, and a false
/// alarm on every new backend would train the operator to ignore this.
fn unknown_alias_targets(
    state: &RamaState,
    backend_name: &str,
    aliases: &[AliasRow],
    lang: Lang,
) -> Option<String> {
    let served: std::collections::HashSet<String> = state
        .upstreams
        .pools()
        .iter()
        .flat_map(|p| p.backends.clone())
        .find(|b| b.name == backend_name)
        .map(|b| b.models_snapshot())?;
    if served.is_empty() {
        return None;
    }
    let unknown: Vec<String> = aliases
        .iter()
        .filter_map(|a| a.target.clone())
        .filter(|t| !served.contains(t))
        .collect();
    if unknown.is_empty() {
        return None;
    }
    let mut models: Vec<String> = served.into_iter().collect();
    models.sort();
    Some(t_args(
        lang,
        "backends-alias-target-unknown",
        &i18n::args([
            ("targets", unknown.join(", ").into()),
            ("models", models.join(", ").into()),
        ]),
    ))
}

/// POST /admin/backends/enabled — flip a backend's maintenance switch.
///
/// The one topology write on this page that does **not** wait for "Apply
/// changes": it flips the live registry first (so the next request already
/// routes around the drained backend) and then persists it, because a
/// maintenance switch that needs a second confirmation step is not a
/// maintenance switch. Consequently it also does not bump the dirty counter —
/// there is no pending change to apply.
///
/// Draining does not make the backend's models unknown, so a request for one of
/// them lands on a sibling backend, or — if this was the last one — waits for /
/// is told about a temporary outage, rather than being told the model does not
/// exist.
pub async fn backends_enabled(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let lang = Lang::from_headers(req.headers());
    if let Err(resp) = require_admin_or_403(&state, &req).await {
        return resp;
    }
    let (_, body) = req.into_parts();
    let pairs: Vec<(String, String)> = match read_form(body).await {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    let name = field(&pairs, "name").trim().to_string();
    if name.is_empty() {
        return toast(FlashKind::Error, t(lang, "backends-error-name-required"));
    }
    // The switch is a checkbox: present = "serve traffic", absent = drained.
    let enabled = checkbox_on(field(&pairs, "enabled"));

    if let Err(e) = upstreams_config::set_backend_enabled(&state.db, &name, enabled).await {
        return toast(
            FlashKind::Error,
            t_args(
                lang,
                "admin-db-error",
                &i18n::args([("err", e.to_string().into())]),
            ),
        );
    }
    // Live effect. A backend the registry doesn't know (added but never applied)
    // has nothing to flip — the DB write above is still what matters, and the
    // next reload picks it up.
    state.upstreams.set_backend_enabled(&name, enabled);

    let key = if enabled {
        "backends-enabled-on"
    } else {
        "backends-enabled-off"
    };
    toast(
        if enabled {
            FlashKind::Success
        } else {
            FlashKind::Info
        },
        t_args(lang, key, &i18n::args([("name", name.into())])),
    )
}

/// POST /admin/backends/delete — remove a backend and its dependent rows.
pub async fn backends_delete(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let lang = Lang::from_headers(req.headers());
    if let Err(resp) = require_admin_or_403(&state, &req).await {
        return resp;
    }
    let (_, body) = req.into_parts();
    let pairs: Vec<(String, String)> = match read_form(body).await {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    let name = field(&pairs, "name");
    match upstreams_config::delete_backend(&state.db, name).await {
        Ok(()) => {
            let dirty = state.topology_dirty_bump();
            sse_response(&[
                sse_toast(&session_core::chrome::Flash {
                    kind: FlashKind::Success,
                    message: t_args(
                        lang,
                        "backends-deleted",
                        &i18n::args([("name", name.to_string().into())]),
                    ),
                }),
                dirty_signal(dirty),
            ])
        }
        Err(e) => toast(
            FlashKind::Error,
            t_args(
                lang,
                "admin-db-error",
                &i18n::args([("err", e.to_string().into())]),
            ),
        ),
    }
}

// ---------------------------------------------------------------------------
// "Test connection" — probe an upstream from the editor, before saving
// ---------------------------------------------------------------------------

/// How long to wait for the tested upstream. Generous compared to the health
/// probe's 2 s: an operator pressing a button will happily wait, and a slow
/// answer is much more useful than a timeout they have to interpret.
const TEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(8);
/// Cap on how many discovered model ids to render.
const TEST_MAX_MODELS: usize = 40;

/// POST /admin/backends/test — call an upstream with the credentials currently
/// typed into the editor and report exactly what came back.
///
/// This is the answer to the failure that caused a production outage: an
/// `api_key_env` naming an unset variable, so `/models` answered 401, so the
/// backend advertised nothing, so an alias bound to nothing, so clients got
/// `404 model_not_found`. Every step of that was invisible. One button now
/// reports the HTTP status, whether authentication was accepted, **and the exact
/// model ids the upstream reports** — which is also the list an alias target has
/// to match character for character.
///
/// Nothing is written. It reads the form, not the database, so it tests what the
/// operator is about to save rather than what is already stored. The one thing
/// it will read from storage is the sealed key, and only when the key field was
/// left blank on an existing backend (the form never echoes a secret back).
pub async fn backends_test(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let lang = Lang::from_headers(req.headers());
    if let Err(resp) = require_admin_or_403(&state, &req).await {
        return resp;
    }
    let (_, body) = req.into_parts();
    let pairs: Vec<(String, String)> = match read_form(body).await {
        Ok(p) => p,
        Err(resp) => return resp,
    };

    // Where to render the answer. The editor supplies the id of its own result
    // box, so several open editors can each hold their own result.
    let target = {
        let t = field(&pairs, "result_id").trim();
        if t.is_empty() { "bt-add" } else { t }.to_string()
    };
    let base_url = field(&pairs, "base_url").trim().trim_end_matches('/');
    if base_url.is_empty() {
        return test_result(
            lang,
            &target,
            FlashKind::Error,
            t(lang, "backends-error-base-url-required"),
            &[],
        );
    }
    let health_path = {
        let h = field(&pairs, "health_path").trim();
        if h.is_empty() { "/models" } else { h }
    };
    let name = field(&pairs, "name").trim();

    // Key precedence mirrors the runtime's exactly, so a green test cannot mean
    // something different from what the gateway will actually send: a freshly
    // typed key, else the stored sealed one, else the named env var.
    let (key, key_source) = resolve_test_key(&state, &pairs, name, lang).await;

    let url = format!("{base_url}{health_path}");
    let mut request = state.http.get(&url).header(
        "user-agent",
        concat!(
            "llm-gateway/",
            env!("CARGO_PKG_VERSION"),
            " connection-test"
        ),
    );
    if let Some(k) = key.as_deref() {
        request = request.bearer_auth(k);
    }

    let resp = match tokio::time::timeout(TEST_TIMEOUT, request.send()).await {
        Ok(Ok(r)) => r,
        Ok(Err(err)) => {
            // reqwest's own Display is the opaque "error sending request"; the
            // concrete cause (refused / DNS / unreachable) is down the chain and
            // is the whole value of pressing this button.
            let mut cause = err.to_string();
            let mut src: Option<&(dyn std::error::Error + 'static)> =
                std::error::Error::source(&err);
            while let Some(e) = src {
                cause = e.to_string();
                src = std::error::Error::source(e);
            }
            return test_result(
                lang,
                &target,
                FlashKind::Error,
                t_args(
                    lang,
                    "backends-test-unreachable",
                    &i18n::args([("url", url.into()), ("err", cause.into())]),
                ),
                &[],
            );
        }
        Err(_) => {
            return test_result(
                lang,
                &target,
                FlashKind::Error,
                t_args(
                    lang,
                    "backends-test-timeout",
                    &i18n::args([
                        ("url", url.into()),
                        ("secs", TEST_TIMEOUT.as_secs().to_string().into()),
                    ]),
                ),
                &[],
            );
        }
    };

    let status = resp.status().as_u16();
    let body_bytes = resp.bytes().await.unwrap_or_default();

    if matches!(status, 401 | 403) {
        return test_result(
            lang,
            &target,
            FlashKind::Error,
            t_args(
                lang,
                "backends-test-auth-failed",
                &i18n::args([
                    ("status", status.to_string().into()),
                    ("source", key_source.into()),
                ]),
            ),
            &[],
        );
    }
    if !(200..300).contains(&status) {
        return test_result(
            lang,
            &target,
            FlashKind::Error,
            t_args(
                lang,
                "backends-test-http-error",
                &i18n::args([("status", status.to_string().into()), ("url", url.into())]),
            ),
            &[],
        );
    }

    let models = parse_model_ids(&body_bytes);
    if models.is_empty() {
        // 200 with an unparseable body: the backend is reachable and
        // authenticated, but discovery won't work, so it can only serve the
        // static `Models` list. Worth saying plainly — it is the difference
        // between "fine" and "fine but you must fill in Models".
        return test_result(
            lang,
            &target,
            FlashKind::Info,
            t_args(
                lang,
                "backends-test-ok-no-models",
                &i18n::args([("source", key_source.into())]),
            ),
            &[],
        );
    }
    let shown: Vec<String> = models.iter().take(TEST_MAX_MODELS).cloned().collect();
    test_result(
        lang,
        &target,
        FlashKind::Success,
        t_args(
            lang,
            "backends-test-ok",
            &i18n::args([
                ("count", models.len().to_string().into()),
                ("source", key_source.into()),
            ]),
        ),
        &shown,
    )
}

/// The key the test should send, plus a short human label for where it came
/// from — named in the result so a 401 immediately says *which* credential was
/// rejected (the classic cause being an env var that isn't set).
async fn resolve_test_key(
    state: &RamaState,
    pairs: &[(String, String)],
    name: &str,
    lang: Lang,
) -> (Option<String>, String) {
    let typed = field(pairs, "api_key").trim();
    if !typed.is_empty() {
        return (Some(typed.to_string()), t(lang, "backends-test-key-typed"));
    }
    if !name.is_empty()
        && let Ok(Some(existing)) = upstreams_config::get_backend(&state.db, name).await
        && let (Some(ct), Some(nonce)) = (existing.api_key_ct, existing.api_key_nonce)
        // `open_str` takes the nonce first — the two arguments are both `&[u8]`,
        // so a swap compiles and silently fails to decrypt.
        && let Ok(open) = state.crypto.open_str(&nonce, &ct)
    {
        return (Some(open), t(lang, "backends-test-key-stored"));
    }
    let env_name = field(pairs, "api_key_env").trim();
    if !env_name.is_empty() {
        return match std::env::var(env_name) {
            Ok(v) if !v.is_empty() => (
                Some(v),
                t_args(
                    lang,
                    "backends-test-key-env",
                    &i18n::args([("var", env_name.to_string().into())]),
                ),
            ),
            // The exact trap that started the incident. Say it before the
            // request even goes out, so a 401 is never a mystery.
            _ => (
                None,
                t_args(
                    lang,
                    "backends-test-key-env-unset",
                    &i18n::args([("var", env_name.to_string().into())]),
                ),
            ),
        };
    }
    (None, t(lang, "backends-test-key-none"))
}

/// Model ids out of an OpenAI `/models` envelope, sorted. Empty for any other
/// shape — deliberately no guessing: the gateway's own probe parses exactly this
/// envelope, so anything it can't read is something discovery can't use either.
fn parse_model_ids(body: &[u8]) -> Vec<String> {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(body) else {
        return Vec::new();
    };
    let Some(data) = v.get("data").and_then(|d| d.as_array()) else {
        return Vec::new();
    };
    let mut ids: Vec<String> = data
        .iter()
        .filter_map(|m| m.get("id").and_then(|i| i.as_str()))
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

/// Render the test outcome into the editor's result box.
fn test_result(
    lang: Lang,
    target: &str,
    kind: FlashKind,
    message: String,
    models: &[String],
) -> Response {
    let html = super::upstreams::render_test_result(lang, kind, &message, models).to_string();
    sse_response(&[session_core::chrome::sse_patch(
        Some(&format!("#{target}")),
        Some("inner"),
        &html,
    )])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_csv_trims_and_drops_empties() {
        assert_eq!(parse_csv(" a, b ,,c , "), vec!["a", "b", "c"]);
        assert!(parse_csv("   ").is_empty());
    }

    #[test]
    fn checkbox_on_recognises_present_values() {
        assert!(checkbox_on("on"));
        assert!(checkbox_on(" true "));
        assert!(!checkbox_on(""));
        assert!(!checkbox_on("off"));
    }

    /// The connection test parses exactly the envelope the gateway's own probe
    /// parses — anything else is something discovery cannot use either, and
    /// saying "0 models" is the useful answer rather than guessing.
    #[test]
    fn model_ids_come_from_the_openai_envelope_only() {
        let ok = br#"{"data":[{"id":"unsloth/Qwen3.8-27B-NVFP4"},{"id":"a"},{"id":""}]}"#;
        assert_eq!(
            parse_model_ids(ok),
            vec!["a".to_string(), "unsloth/Qwen3.8-27B-NVFP4".to_string()]
        );
        // Duplicates collapse; a foreign shape and non-JSON both yield nothing.
        assert_eq!(
            parse_model_ids(br#"{"data":[{"id":"x"},{"id":"x"}]}"#),
            vec!["x"]
        );
        assert!(parse_model_ids(br#"{"models":["x"]}"#).is_empty());
        assert!(parse_model_ids(b"<html>nope</html>").is_empty());
    }

    /// The aliases textarea round-trips through parse → serialise unchanged:
    /// `name=target` for explicit targets, a bare `name` for target-less ones.
    #[test]
    fn aliases_round_trip() {
        let parsed = parse_aliases("fast=qwen-7b\nbare\n\n  smart = qwen-32b ");
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].alias, "fast");
        assert_eq!(parsed[0].target.as_deref(), Some("qwen-7b"));
        assert_eq!(parsed[1].alias, "bare");
        assert!(parsed[1].target.is_none());
        assert_eq!(parsed[2].target.as_deref(), Some("qwen-32b"));
        // Serialising back yields the canonical `name=target` / bare form.
        assert_eq!(alias_lines(&parsed), "fast=qwen-7b\nbare\nsmart=qwen-32b");
    }
}
