// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Feedback widget — a floating button on every signed-in page that opens a
//! dialog and files the report as an issue.
//!
//! Ported from the `yachtlistings2` / `croit.erp` React widgets, rebuilt
//! natively for this stack:
//!   - The FAB + dialog live in the SvelteKit SPA
//!     (`web/src/lib/components/feedback/`), mounted once in the root layout
//!     so they survive client-side navigation.
//!   - Voice input reuses the existing in-browser recorder + the
//!     `/api/v0/transcriptions` endpoint (VAD + Whisper). The transcript is
//!     then turned into structured form fields by `POST /feedback/extract`
//!     (a side call, `aiplane_runtime::server::side_call`, metered to the
//!     reporter and under their spend limits).
//!   - A viewport screenshot is captured client-side (snapdom, or the
//!     pixel-exact `getDisplayMedia` path), annotated on a canvas and sent as
//!     base64 together with any images the reporter pasted in; the browser's
//!     console + network ring buffers ride along in `system_info`.
//!   - `POST /feedback` hands all of it to
//!     [`aiplane_features::server::issue_tracker`], which files it in
//!     whichever tracker the operator selected — GitHub or GitLab.
//!
//! Three JSON endpoints (plain `fetch`, not SSE — the client has to carry
//! image bytes and a transcript):
//!   - GET  /api/v0/feedback/config   → `{ enabled, voice_enabled, voice_model, provider, … }`
//!   - POST /api/v0/feedback/extract  → transcript → structured fields
//!   - POST /api/v0/feedback          → file the issue

use std::sync::Arc;
use std::time::Duration;

use rama::http::service::web::extract::State;
use rama::http::{Request, Response, StatusCode};
use serde::Deserialize;
use serde_json::json;
use session_core::chrome::read_body_to_bytes;
use session_core::i18n::{self, Lang, t, t_args};

use aiplane_core::rama_server::session::Session;
use aiplane_core::server::db::usage::UsageSource;
use aiplane_core::server::db::users;
use aiplane_core::server::upstreams::{PoolAccess, PoolKind};
use aiplane_features::server::issue_tracker::{self, IssueInput, TrackerError};
use aiplane_runtime::rama_server::state::RamaState;
use aiplane_runtime::server::side_call::{self, JsonShape, Payer, SideCall, SideCallError};

fn json_ok(value: impl serde::Serialize) -> Response {
    super::json_ok(StatusCode::OK, value)
}

fn json_err(status: StatusCode, code: &str, message: &str) -> Response {
    super::json_error(status, code, message)
}

/// Session gate that returns a 401 JSON envelope (not a redirect) on miss —
/// these are API-shaped endpoints called from the dialog.
async fn require_session_json(
    state: &RamaState,
    req: &Request,
    lang: Lang,
) -> Result<Session, Response> {
    match state.sessions.lookup_from_headers(req.headers()).await {
        Ok(Some(s)) => Ok(s),
        Ok(None) => Err(json_err(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            &t(lang, "feedback-err-no-session"),
        )),
        Err(err) => {
            tracing::warn!(error = %err, "feedback: session lookup");
            Err(json_err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                &t(lang, "feedback-err-session-lookup-failed"),
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// GET /feedback/config

/// Tells the client whether to reveal the FAB and the voice block, and which
/// transcription model to record against. Model *selection* is an operator
/// concern (config), never the end user's — the form has no model picker; the
/// client just needs the resolved voice model id to attach to its upload.
pub async fn feedback_config(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let lang = Lang::from_request(req.headers());
    if let Err(resp) = require_session_json(&state, &req, lang).await {
        return resp;
    }
    let enabled = state
        .config()
        .feedback
        .as_ref()
        .map(|f| f.is_configured())
        .unwrap_or(false);

    let transcription_models = state.upstreams.models_for_kind(PoolKind::Transcription);
    let chat_models = state.upstreams.models_for_kind(PoolKind::Chat);
    // Voice→fields needs both a transcription model (to record against) and a
    // chat model (to extract the fields).
    let voice_enabled = !transcription_models.is_empty() && !chat_models.is_empty();
    let voice_model = resolve_model(
        state
            .config()
            .feedback
            .as_ref()
            .and_then(|f| f.voice_model.clone()),
        &transcription_models,
    );

    // The provider is surfaced so the confirmation step can name the tracker
    // the report is about to land in ("a GitLab issue"), which is the one
    // thing a reporter deciding whether to attach a screenshot needs to know.
    let provider = state
        .config()
        .feedback
        .as_ref()
        .map(|f| f.provider().as_str())
        .unwrap_or("github");

    json_ok(FeedbackConfigView {
        enabled,
        voice_enabled,
        voice_model,
        provider,
        max_attachments: MAX_ATTACHMENTS,
    })
}

/// What the feedback dialog needs to know before it opens.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct FeedbackConfigView {
    /// Whether an issue tracker is configured, so the dialog is offered.
    pub enabled: bool,
    /// Whether a spoken note can be turned into the form's fields.
    pub voice_enabled: bool,
    /// The transcription model to record against.
    pub voice_model: Option<String>,
    /// The tracker reports are filed in: `github` or `gitlab`.
    pub provider: &'static str,
    /// How many pasted images one report may carry.
    pub max_attachments: usize,
}

/// Resolve a configured model id against the live advertised set: honour the
/// configured one if it's actually available, else fall back to the first
/// advertised model (or `None` when the pool is empty).
fn resolve_model(configured: Option<String>, available: &[String]) -> Option<String> {
    configured
        .filter(|m| !m.is_empty() && available.contains(m))
        .or_else(|| available.first().cloned())
}

// ---------------------------------------------------------------------------
// POST /feedback/extract — voice transcript → structured fields

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ExtractRequest {
    transcript: String,
    /// BCP-47 tag of the language the fields are written in.
    #[serde(default)]
    locale: Option<String>,
}

/// The report fields a spoken note was turned into; a field the note did not
/// cover is empty.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ExtractedFields {
    pub title: String,
    pub description: String,
    pub business_value: String,
    pub acceptance_criteria: String,
    /// `low`, `medium` or `high`.
    pub priority: &'static str,
}

pub async fn feedback_extract(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let lang = Lang::from_request(req.headers());
    let session = match require_session_json(&state, &req, lang).await {
        Ok(session) => session,
        Err(resp) => return resp,
    };
    let (_, body) = req.into_parts();
    let bytes = match read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => {
            return json_err(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                &t_args(
                    lang,
                    "feedback-err-body-read",
                    &i18n::args([("error", msg.into())]),
                ),
            );
        }
    };
    let parsed: ExtractRequest = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => {
            return json_err(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                &t_args(
                    lang,
                    "feedback-err-malformed-json",
                    &i18n::args([("error", err.to_string().into())]),
                ),
            );
        }
    };
    let transcript = parsed.transcript.trim();
    if transcript.is_empty() {
        return json_err(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            &t(lang, "feedback-err-empty-transcript"),
        );
    }

    // Model selection is an operator concern: the configured `extraction_model`
    // if it's a currently advertised chat model, else the first available chat
    // model. The client never picks the model.
    let chat_models = state.upstreams.models_for_kind(PoolKind::Chat);
    let model = resolve_model(
        state
            .config()
            .feedback
            .as_ref()
            .and_then(|f| f.extraction_model.clone()),
        &chat_models,
    );
    let Some(model) = model else {
        return json_err(
            StatusCode::SERVICE_UNAVAILABLE,
            "feedback_no_model",
            &t(lang, "feedback-err-no-chat-model"),
        );
    };

    let user = users::find_by_id(&state.db, &session.user_id)
        .await
        .ok()
        .flatten();
    let payer = Payer::person(
        &session.user_id,
        user.as_ref().map_or(&[][..], |u| u.roles.as_slice()),
        user.as_ref().map(|u| u.email.clone()),
        UsageSource::Chat,
    );
    match extract_fields(&state, &payer, &model, transcript, parsed.locale.as_deref()).await {
        Ok(fields) => json_ok(fields),
        Err(err) => {
            tracing::warn!(error = %err, %model, "feedback: field extraction failed");
            let status = match err {
                SideCallError::OverBudget(_) => StatusCode::TOO_MANY_REQUESTS,
                SideCallError::Failed(_) => StatusCode::BAD_GATEWAY,
            };
            json_err(
                status,
                "feedback_model_failed",
                &t_args(
                    lang,
                    "feedback-err-extraction-failed",
                    &i18n::args([("error", err.to_string().into())]),
                ),
            )
        }
    }
}

/// How long the transcript→fields pass may take: long enough for a
/// 1200-token answer on a busy backend.
const EXTRACT_TIMEOUT: Duration = Duration::from_secs(120);

/// System prompt for the transcript→fields pass. Forbids invention (empty
/// string when a field isn't derivable) and pins the output language to the
/// caller's UI locale regardless of the spoken language.
const EXTRACT_SYSTEM_PROMPT: &str = "You convert a spoken software-feedback note into a structured bug/feature report. \
Return ONLY a JSON object with these string fields: \
\"title\" (imperative, concise, max 120 chars), \
\"description\" (what happened / what is wanted, factual), \
\"business_value\" (why it matters / who is impacted), \
\"acceptance_criteria\" (a short markdown bullet list of concrete conditions), \
\"priority\" (one of \"low\", \"medium\", \"high\" — use \"high\" only on explicit cues like \"urgent\", \"blocking\", \"production\"). \
Do NOT invent details: if a field cannot be derived from the transcript, use an empty string (for priority default to \"medium\"). \
No preamble, no code fences, no reasoning — output the raw JSON object only.";

/// One side call that returns the structured fields, on the operator's
/// model under the gateway's own access. The answer is read leniently, so a
/// server that ignores the `response_format` hint still works.
async fn extract_fields(
    state: &RamaState,
    payer: &Payer,
    model: &str,
    transcript: &str,
    locale: Option<&str>,
) -> Result<ExtractedFields, SideCallError> {
    let lang_directive = match locale {
        Some(l) if !l.is_empty() => format!(
            "\n\nWrite every field value in the language with BCP-47 tag \"{l}\", \
             regardless of the transcript's language."
        ),
        _ => String::new(),
    };
    let input = format!("Transcript:\n{transcript}{lang_directive}");
    let obj = side_call::ask_json(
        state,
        payer,
        SideCall {
            purpose: "feedback_fields",
            model,
            access: &PoolAccess::all(),
            instructions: EXTRACT_SYSTEM_PROMPT,
            input: &input,
            max_tokens: Some(1200),
            no_think: true,
            timeout: EXTRACT_TIMEOUT,
        },
        JsonShape {
            name: "feedback_fields",
            schema: json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "title": { "type": "string" },
                    "description": { "type": "string" },
                    "business_value": { "type": "string" },
                    "acceptance_criteria": { "type": "string" },
                    "priority": { "type": "string", "enum": ["low", "medium", "high"] }
                },
                "required": ["title", "description", "business_value", "acceptance_criteria", "priority"]
            }),
        },
    )
    .await
    .answer?;

    // Re-shape into exactly the five fields the client expects, coercing
    // anything odd into a sane default.
    let pick = |key: &str| -> String {
        obj.get(key)
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim()
            .to_string()
    };
    let priority = match obj.get("priority").and_then(|p| p.as_str()).unwrap_or("") {
        "low" => "low",
        "high" => "high",
        _ => "medium",
    };
    Ok(ExtractedFields {
        title: pick("title"),
        description: pick("description"),
        business_value: pick("business_value"),
        acceptance_criteria: pick("acceptance_criteria"),
        priority,
    })
}

// ---------------------------------------------------------------------------
// POST /feedback — file the issue

#[derive(Deserialize, schemars::JsonSchema)]
pub struct SubmitRequest {
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    business_value: String,
    #[serde(default)]
    acceptance_criteria: String,
    #[serde(default)]
    priority: String,
    /// Raw standard-base64 PNG (no `data:` prefix). Optional.
    #[serde(default)]
    screenshot_base64: Option<String>,
    /// Extra images the reporter pasted or dropped into the dialog, same
    /// encoding. Bounded server-side by [`MAX_ATTACHMENTS`] so a scripted
    /// client can't turn one submission into an unbounded upload loop.
    #[serde(default)]
    attachments_base64: Vec<String>,
    /// Browser and page details, including the console and network logs.
    #[serde(default)]
    system_info: serde_json::Value,
}

/// How many pasted images one submission may carry. Mirrors the dialog's own
/// limit; the server enforces it because the dialog is not the only thing
/// that can POST here.
const MAX_ATTACHMENTS: usize = 5;

pub async fn feedback_submit(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let lang = Lang::from_request(req.headers());
    let session = match require_session_json(&state, &req, lang).await {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let Some(cfg) = state.config().feedback.clone() else {
        return json_err(
            StatusCode::SERVICE_UNAVAILABLE,
            "feedback_not_configured",
            &t(lang, "feedback-err-not-configured"),
        );
    };
    if !cfg.is_configured() {
        return json_err(
            StatusCode::SERVICE_UNAVAILABLE,
            "feedback_not_configured",
            &t(lang, "feedback-err-not-configured"),
        );
    }

    let (_, body) = req.into_parts();
    let bytes = match read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => {
            return json_err(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                &t_args(
                    lang,
                    "feedback-err-body-read",
                    &i18n::args([("error", msg.into())]),
                ),
            );
        }
    };
    let parsed: SubmitRequest = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => {
            return json_err(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                &t_args(
                    lang,
                    "feedback-err-malformed-json",
                    &i18n::args([("error", err.to_string().into())]),
                ),
            );
        }
    };

    let title = parsed.title.trim();
    if title.chars().count() < 4 {
        return json_err(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            &t(lang, "feedback-err-title-required"),
        );
    }
    if parsed.description.trim().is_empty() {
        return json_err(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            &t(lang, "feedback-err-description-required"),
        );
    }

    // Reporter email — best-effort, for attribution in the issue body.
    let reporter_email = users::find_by_id(&state.db, &session.user_id)
        .await
        .ok()
        .flatten()
        .map(|u| u.email)
        .unwrap_or_default();

    let input = IssueInput {
        title: title.to_string(),
        description: parsed.description,
        business_value: parsed.business_value,
        acceptance_criteria: parsed.acceptance_criteria,
        priority: parsed.priority,
        reporter_email,
        screenshot_png_base64: parsed.screenshot_base64.filter(|s| !s.is_empty()),
        attachments_png_base64: parsed
            .attachments_base64
            .into_iter()
            .filter(|s| !s.is_empty())
            .take(MAX_ATTACHMENTS)
            .collect(),
        system_info: parsed.system_info,
    };

    match issue_tracker::create_feedback_issue(&state.http, &cfg, input).await {
        Ok(result) => json_ok(FiledIssue {
            ok: true,
            number: result.number,
            url: result.url,
        }),
        Err(TrackerError::NotConfigured) => json_err(
            StatusCode::SERVICE_UNAVAILABLE,
            "feedback_not_configured",
            &t(lang, "feedback-err-not-configured"),
        ),
        Err(err) => {
            tracing::warn!(error = %err, "feedback: issue creation failed");
            json_err(
                StatusCode::BAD_GATEWAY,
                "feedback_submit_failed",
                &t(lang, "feedback-err-submit-failed"),
            )
        }
    }
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct FiledIssue {
    /// Always `true`.
    pub ok: bool,
    /// The issue number the tracker shows.
    pub number: u64,
    pub url: String,
}

// ---------------------------------------------------------------------------
// Chrome: the FAB + the dialog. Rendered once in `layout_authed`.
