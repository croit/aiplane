// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Voice in a visitor's conversation (`publish.voice`, `docs/embed.md` →
//! "Voice"): a recording transcribed on the agent's transcription model,
//! and a finished answer spoken on its speech model.
//!
//! Both calls run as the agent's principal on one model it holds a grant
//! on — the one its spec names for that direction, else the gateway's
//! default model for it ([`super::defaults::voice_model`]) — narrowed with
//! `PoolAccess::for_system_models`, exactly like a model round: the usage row is the agent run's (so it spends the owner's
//! budget) and the exchange is an `llm_exchange` in the conversation's
//! activity chain (`purpose: transcription` or `speech`). The log keeps the
//! transcript and the text spoken; it never keeps audio — a recording's
//! size and length stand in for it, and so do the size and type of the
//! speech returned.
//!
//! The endpoint owns the visitor: who may ask, rates and budget, and which
//! turn may be spoken. This module only makes the call.

use std::collections::VecDeque;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use aiplane_core::server::capped_read::{self, CappedReadError};
use aiplane_core::server::db::usage::{UsageKind, UsageRecord, UsageSource};
use aiplane_core::server::principal::{PrincipalKind, SystemPrincipal};
use aiplane_core::server::run_chain::RunChain;
use aiplane_core::server::upstreams::{PoolAccess, PoolKind};
use aiplane_features::server::speech::{SpokenMarkers, to_spoken};
use rama::bytes::Bytes;
use serde_json::{Value, json};
use session_core::i18n::{Lang, t};

use super::audit::{self, RunLog};
use crate::rama_server::state::RamaState;
use crate::server::side_call::SideExchange;

/// The longest a voice call may take upstream.
const VOICE_TIMEOUT: Duration = Duration::from_secs(60);
/// The largest transcription answer read: a minute of speech is a few KiB.
const TRANSCRIPT_BYTES: u64 = 256 * 1024;
/// The largest speech answer read: several minutes of compressed audio.
const SPEECH_BYTES: u64 = 8 * 1024 * 1024;
/// The most characters of an answer that are spoken. A longer answer is
/// spoken up to the last sentence that fits; the rest stays on screen.
pub const MAX_SPOKEN_CHARS: usize = 3_000;

/// The conversation a voice call belongs to.
pub struct Conversation<'a> {
    pub principal: &'a SystemPrincipal,
    pub version: i64,
    pub session_id: &'a str,
    pub visitor_id: &'a str,
}

impl Conversation<'_> {
    fn chain(&self) -> RunChain {
        RunChain::root(
            self.session_id,
            Some(self.visitor_id.to_string()),
            aiplane_core::server::run_chain::Frame::for_principal(
                self.principal,
                Some(self.version),
            ),
        )
    }
}

/// Why a voice call produced nothing. The visitor is told only that voice is
/// unavailable; this is for the activity log and the server log.
#[derive(Debug, thiserror::Error)]
pub enum VoiceError {
    #[error("routing to the {what} model failed: {message}")]
    Route { what: &'static str, message: String },
    #[error("the {what} backend could not be reached: {message}")]
    Transport { what: &'static str, message: String },
    #[error("the {what} backend answered {status}")]
    Upstream { what: &'static str, status: u16 },
    #[error("the {what} backend's answer could not be read: {message}")]
    Answer { what: &'static str, message: String },
}

/// A visitor's recording, already trimmed of silence: 16 kHz mono 16-bit
/// PCM in a WAV container.
pub struct Recording {
    pub wav: Bytes,
    pub seconds: f64,
}

/// Transcribe `recording` on model `model`. The transcript is returned to
/// the visitor to read and send; nothing is posted to the conversation.
pub async fn transcribe(
    state: &RamaState,
    model: &str,
    conversation: &Conversation<'_>,
    recording: Recording,
) -> Result<String, VoiceError> {
    const WHAT: &str = "transcription";
    let mut exchange = SideExchange::new("transcription");
    let mut usage = Usage::new(UsageKind::Transcription, Some(recording.seconds));
    let result = async {
        let access = PoolAccess::for_system_models(conversation.principal, [model]);
        let acquired = state
            .upstreams
            .route_access(model, PoolKind::Transcription, &access)
            .map_err(|e| VoiceError::Route {
                what: WHAT,
                message: e.to_string(),
            })?;
        let backend = acquired.backend();
        let real_model = acquired.resolved_model().to_string();
        exchange.model = Some(real_model.clone());
        exchange.backend = Some(backend.name.clone());
        exchange.request = json!({
            "model": real_model,
            "file": {
                "content_type": "audio/wav",
                "bytes": recording.wav.len(),
                "seconds": recording.seconds,
            },
            "response_format": "json",
        });
        usage.reached(&real_model, &backend.name);
        let file = reqwest::multipart::Part::bytes(recording.wav.to_vec())
            .file_name("recording.wav")
            .mime_str("audio/wav")
            .map_err(|e| VoiceError::Transport {
                what: WHAT,
                message: e.to_string(),
            })?;
        let form = reqwest::multipart::Form::new()
            .text("model", real_model.clone())
            .text("response_format", "json")
            .part("file", file);
        let mut req = state
            .http
            .post(format!("{}/audio/transcriptions", backend.base_url))
            .timeout(VOICE_TIMEOUT)
            .multipart(form);
        if let Some(key) = backend.api_key.as_deref() {
            req = req.bearer_auth(key);
        }
        let resp = req.send().await.map_err(|e| VoiceError::Transport {
            what: WHAT,
            message: e.to_string(),
        })?;
        let status = resp.status().as_u16();
        usage.status = status;
        let body = capped_read::read_capped(resp, TRANSCRIPT_BYTES)
            .await
            .map_err(|e| read_error(WHAT, e))?;
        drop(acquired);
        exchange.answered(status, &body);
        if !(200..300).contains(&status) {
            return Err(VoiceError::Upstream { what: WHAT, status });
        }
        exchange
            .response
            .get("text")
            .and_then(Value::as_str)
            .map(|text| text.trim().to_string())
            .ok_or_else(|| VoiceError::Answer {
                what: WHAT,
                message: "it names no `text`".into(),
            })
    }
    .await;
    finish(state, conversation, None, exchange, usage, &result).await;
    result
}

/// Speak `answer`, the final text of assistant turn `turn_id`, on model
/// `model` in `voice` (its pool's voice for `lang` when `None`). `Ok(None)` when
/// nothing of the answer is speakable (a bare code block). A turn already
/// spoken in the same voice is served from memory.
pub async fn speak(
    state: &RamaState,
    model: &str,
    voice: Option<&str>,
    conversation: &Conversation<'_>,
    turn_id: &str,
    answer: &str,
    lang: Lang,
) -> Result<Option<Bytes>, VoiceError> {
    const WHAT: &str = "speech";
    let (code, table) = (t(lang, "voice-code-marker"), t(lang, "voice-table-marker"));
    let spoken = within(
        &to_spoken(
            answer,
            &SpokenMarkers {
                code: &code,
                table: &table,
            },
        ),
        MAX_SPOKEN_CHARS,
    );
    if spoken.is_empty() {
        return Ok(None);
    }
    let access = PoolAccess::for_system_models(conversation.principal, [model]);
    let cache_key = format!(
        "{turn_id}|{model}|{}",
        voice.map_or_else(|| format!("lang:{}", lang.code()), str::to_string)
    );
    if let Some(audio) = SPOKEN.lock().ok().and_then(|c| c.get(&cache_key)) {
        return Ok(Some(audio));
    }

    let mut exchange = SideExchange::new("speech");
    let mut usage = Usage::new(UsageKind::Speech, Some(spoken.chars().count() as f64));
    let result = async {
        let acquired = state
            .upstreams
            .route_access(model, PoolKind::Speech, &access)
            .map_err(|e| VoiceError::Route {
                what: WHAT,
                message: e.to_string(),
            })?;
        let backend = acquired.backend();
        let real_model = acquired.resolved_model().to_string();
        let mut body = json!({
            "model": real_model,
            "input": spoken,
            "response_format": "mp3",
        });
        if let Some(v) = voice.or_else(|| acquired.voice_for(lang.code())) {
            body["voice"] = json!(v);
        }
        exchange.model = Some(real_model.clone());
        exchange.backend = Some(backend.name.clone());
        exchange.request = body.clone();
        usage.reached(&real_model, &backend.name);
        let mut req = state
            .http
            .post(format!("{}/audio/speech", backend.base_url))
            .timeout(VOICE_TIMEOUT)
            .json(&body);
        if let Some(key) = backend.api_key.as_deref() {
            req = req.bearer_auth(key);
        }
        let resp = req.send().await.map_err(|e| VoiceError::Transport {
            what: WHAT,
            message: e.to_string(),
        })?;
        let status = resp.status().as_u16();
        usage.status = status;
        exchange.status = Some(status);
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("audio/mpeg")
            .to_string();
        let audio = capped_read::read_capped(resp, SPEECH_BYTES)
            .await
            .map_err(|e| read_error(WHAT, e))?;
        drop(acquired);
        if !(200..300).contains(&status) {
            exchange.answered(status, &audio);
            return Err(VoiceError::Upstream { what: WHAT, status });
        }
        exchange.response = json!({ "content_type": content_type, "bytes": audio.len() });
        Ok(Bytes::from(audio))
    }
    .await;
    finish(state, conversation, Some(turn_id), exchange, usage, &result).await;
    let audio = result?;
    if let Ok(mut cache) = SPOKEN.lock() {
        cache.put(cache_key, audio.clone());
    }
    Ok(Some(audio))
}

/// The usage row of one voice call, filled in as the call gets further.
struct Usage {
    kind: UsageKind,
    units: Option<f64>,
    model: Option<String>,
    backend: String,
    status: u16,
    started: Instant,
}

impl Usage {
    fn new(kind: UsageKind, units: Option<f64>) -> Self {
        Self {
            kind,
            units,
            model: None,
            backend: String::new(),
            status: 502,
            started: Instant::now(),
        }
    }

    fn reached(&mut self, model: &str, backend: &str) {
        self.model = Some(model.to_string());
        self.backend = backend.to_string();
    }
}

/// Write the call's usage row (once it reached a backend) and its
/// `llm_exchange`, then anchor the conversation's chain so the event is
/// guarded before the next turn ends.
async fn finish<T>(
    state: &RamaState,
    conversation: &Conversation<'_>,
    turn_id: Option<&str>,
    mut exchange: SideExchange,
    usage: Usage,
    result: &Result<T, VoiceError>,
) {
    let chain = conversation.chain();
    if let Some(model) = usage.model
        && state.usage.is_enabled()
    {
        let pool_kind = match usage.kind {
            UsageKind::Speech => PoolKind::Speech,
            _ => PoolKind::Transcription,
        };
        let succeeded = (200..300).contains(&usage.status);
        state.usage.emit(
            UsageRecord {
                created_at: jiff::Timestamp::now(),
                user_id: conversation.principal.id.clone(),
                user_email: Some(conversation.principal.name.clone()),
                token_id: None,
                token_name: None,
                source: UsageSource::Agent,
                kind: usage.kind,
                backend: usage.backend,
                enforce_limits: state.upstreams.enforce_limits_for_model(&model, pool_kind),
                model,
                status: usage.status,
                duration_ms: i64::try_from(usage.started.elapsed().as_millis()).unwrap_or(i64::MAX),
                prompt_tokens: None,
                completion_tokens: None,
                total_tokens: None,
                input_units: usage.units.filter(|_| succeeded),
                output_units: None,
                principal_kind: PrincipalKind::System,
                agent_id: None,
                chain: None,
                stop_reason: None,
            }
            .in_run(Some(&chain)),
        );
    }
    if let Err(err) = result {
        exchange.error = Some(err.to_string());
        tracing::warn!(
            error = %err,
            agent = %conversation.principal.id,
            conversation = conversation.session_id,
            purpose = exchange.purpose,
            "a visitor's voice call failed"
        );
    }
    let log = RunLog::visitor(
        conversation.principal,
        conversation.version,
        conversation.session_id,
        conversation.visitor_id,
        turn_id,
    );
    log.record(&state.db, &exchange).await;
    audit::anchor(
        &state.db,
        &conversation.principal.id,
        conversation.session_id,
    )
    .await;
}

fn read_error(what: &'static str, err: CappedReadError) -> VoiceError {
    match err {
        CappedReadError::TooLarge { max } => VoiceError::Answer {
            what,
            message: format!("it is larger than {max} bytes"),
        },
        other => VoiceError::Transport {
            what,
            message: other.to_string(),
        },
    }
}

/// `text` cut to at most `max` characters, at the end of the last sentence
/// that fits — or at the last word, when no sentence ends in time.
fn within(text: &str, max: usize) -> String {
    let text = text.trim();
    let Some((cut, _)) = text.char_indices().nth(max) else {
        return text.to_string();
    };
    let head = &text[..cut];
    let end = head
        .rfind(['.', '!', '?', '。', '！', '？'])
        .map(|i| i + head[i..].chars().next().map_or(1, char::len_utf8))
        .or_else(|| head.rfind(char::is_whitespace))
        .unwrap_or(cut);
    head[..end].trim().to_string()
}

/// Spoken answers, by turn, model and voice: a visitor replaying an answer
/// costs no second synthesis. Bounded by entries and bytes; the oldest go
/// first.
static SPOKEN: LazyLock<Mutex<SpokenCache>> =
    LazyLock::new(|| Mutex::new(SpokenCache::new(256, 64 * 1024 * 1024)));

struct SpokenCache {
    entries: VecDeque<(String, Bytes)>,
    bytes: usize,
    max_entries: usize,
    max_bytes: usize,
}

impl SpokenCache {
    fn new(max_entries: usize, max_bytes: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            bytes: 0,
            max_entries,
            max_bytes,
        }
    }

    fn get(&self, key: &str) -> Option<Bytes> {
        self.entries
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    }

    fn put(&mut self, key: String, audio: Bytes) {
        if audio.len() > self.max_bytes || self.get(&key).is_some() {
            return;
        }
        while self.entries.len() >= self.max_entries || self.bytes + audio.len() > self.max_bytes {
            let Some((_, old)) = self.entries.pop_front() else {
                break;
            };
            self.bytes -= old.len();
        }
        self.bytes += audio.len();
        self.entries.push_back((key, audio));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_answer_is_spoken_up_to_the_last_sentence_that_fits() {
        assert_eq!(within("Short. Answer.", 100), "Short. Answer.");
        assert_eq!(within("One. Two three four.", 12), "One.");
        assert_eq!(within("no sentence end here at all", 12), "no sentence");
        assert_eq!(within("Grüße. Schön", 9), "Grüße.");
        assert_eq!(within("你好。世界很大", 4), "你好。");
    }

    #[test]
    fn the_spoken_cache_keeps_the_newest_answers_within_its_bounds() {
        let mut cache = SpokenCache::new(2, 10);
        cache.put("a".into(), Bytes::from_static(b"1234"));
        cache.put("b".into(), Bytes::from_static(b"5678"));
        cache.put("c".into(), Bytes::from_static(b"90"));
        assert_eq!(cache.get("a"), None, "over the entry bound");
        assert_eq!(cache.get("c").as_deref(), Some(&b"90"[..]));
        cache.put("d".into(), Bytes::from_static(b"abcdefgh"));
        assert_eq!(cache.get("b"), None, "over the byte bound");
        assert_eq!(cache.get("d").as_deref(), Some(&b"abcdefgh"[..]));
        assert!(cache.bytes <= 10);
        cache.put("huge".into(), Bytes::from(vec![0; 11]));
        assert_eq!(cache.get("huge"), None, "larger than the whole cache");
    }
}
