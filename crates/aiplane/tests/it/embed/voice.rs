// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Voice in the widget (`docs/embed.md` → "Voice", `publish.voice`): a
//! visitor's recording transcribed for them to send, and a finished answer
//! spoken, each on the pool the agent's spec names, under the visitor's
//! rates and the owner's budget, and recorded in the conversation's
//! activity chain without the audio.

use std::sync::Arc;
use std::time::Duration;

use rama::Service;
use rama::http::{Body, Method, Request, StatusCode, header};
use serde_json::{Value, json};
use tokio::sync::Notify;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::{ANSWER, Embed, Reply, SITE, ScriptedRunner, code, new_key};
use crate::agents::{self, TIME, spec};
use crate::common;

use aiplane_core::server::db::usage::{self, UsageKind, UsageRecord, UsageSource};
use aiplane_core::server::principal::PrincipalKind;
use aiplane_core::server::upstreams::config::{PickerStrategy, PoolKind, UpstreamPoolConfig};
use aiplane_runtime::server::tools::ToolRegistry;
use aiplane_runtime::server::tools::time::CurrentTimestamp;
use session_core::db as chat;

const STT: &str = "stt";
const TTS: &str = "tts";
const WHISPER: &str = "whisper-1";
const TTS_MODEL: &str = "tts-1";
const SPOKEN: &[u8] = b"ID3\x04-not-really-mp3";
const TRANSCRIPT: &str = "Where is my order?";

/// `seconds` of a 440 Hz tone as 16 kHz mono 16-bit PCM WAV: what the
/// widget's recorder sends.
fn wav(seconds: f64) -> Vec<u8> {
    let samples = (seconds * 16_000.0) as usize;
    let mut out = Vec::with_capacity(44 + samples * 2);
    let data = (samples * 2) as u32;
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&16_000u32.to_le_bytes());
    out.extend_from_slice(&32_000u32.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for i in 0..samples {
        let v = (i as f64 * 440.0 * std::f64::consts::TAU / 16_000.0).sin() * 9_000.0;
        out.extend_from_slice(&(v as i16).to_le_bytes());
    }
    out
}

fn voice_pool(kind: PoolKind, upstream: &str, voices: &[(&str, &str)]) -> UpstreamPoolConfig {
    UpstreamPoolConfig {
        voices: voices
            .iter()
            .map(|(l, v)| (l.to_string(), v.to_string()))
            .collect(),
        offer_voices: Vec::new(),
        allowed_groups: Vec::new(),
        fallback_offline: None,
        compliance: Default::default(),
        enforce_limits: true,
        kind,
        strategy: PickerStrategy::RoundRobin,
        models: Vec::new(),
        backend: vec![common::mock_backend("voice", upstream)],
    }
}

/// A published agent with voice set to `voice` and the rest of `publish`
/// from `publish`, its speech and transcription models served by `upstream`
/// and granted to it.
async fn voice_embed(upstream: &MockServer, voice: Value, publish: Value) -> Embed {
    voice_embed_on(upstream, voice, publish, ScriptedRunner::answering()).await
}

async fn voice_embed_on(
    upstream: &MockServer,
    voice: Value,
    publish: Value,
    runner: ScriptedRunner,
) -> Embed {
    let mut fx = agents::fixture_with_pools(
        None,
        ToolRegistry::new().with(CurrentTimestamp),
        &[TIME],
        vec![
            (
                STT.into(),
                voice_pool(PoolKind::Transcription, &upstream.uri(), &[]),
                vec!["whisper-1".into()],
            ),
            (
                TTS.into(),
                voice_pool(
                    PoolKind::Speech,
                    &upstream.uri(),
                    &[("", "alloy"), ("de", "thorsten")],
                ),
                vec!["tts-1".into()],
            ),
        ],
    )
    .await;
    let runner = Arc::new(runner);
    fx.state = fx.state.clone().with_agent_runner(runner.clone());
    fx.state = fx.state.clone().with_trusted_proxies(
        aiplane_core::server::trusted_proxies::TrustedProxies::parse("10.0.0.0/8").unwrap(),
    );
    let agent = fx.runnable("support").await;
    for model in [WHISPER, TTS_MODEL] {
        assert_eq!(
            fx.grant(&fx.alice, &agent, "model", model).await,
            StatusCode::CREATED
        );
    }
    let mut s = spec("v1");
    s["publish"] = publish;
    s["publish"]["voice"] = voice;
    let (status, body) = fx.put_draft(&fx.alice, &agent, s).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = fx.publish(&fx.alice, &agent).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (key_id, key) = new_key(&fx, &agent, &[SITE]).await;
    Embed {
        fx,
        agent,
        key_id,
        key,
        runner,
    }
}

fn both() -> Value {
    json!({ "input": true, "output": true, "transcription_model": WHISPER, "speech_model": TTS_MODEL })
}

async fn upstream() -> MockServer {
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/audio/transcriptions"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "text": format!(" {TRANSCRIPT} ") })),
        )
        .mount(&mock)
        .await;
    Mock::given(method("POST"))
        .and(path("/audio/speech"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "audio/mpeg")
                .set_body_bytes(SPOKEN),
        )
        .mount(&mock)
        .await;
    mock
}

fn post(uri: &str, token: &str, content_type: &str, body: Body) -> Request {
    Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(header::ORIGIN, SITE)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, content_type)
        .body(body)
        .unwrap()
}

async fn reply(resp: rama::http::Response) -> (Reply, Vec<u8>) {
    let status = resp.status();
    let headers = resp.headers().clone();
    let bytes = common::read_body(resp).await.to_vec();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (
        Reply {
            status,
            headers,
            body,
        },
        bytes,
    )
}

impl Embed {
    async fn transcribe(&self, token: &str, audio: Vec<u8>) -> Reply {
        let req = post(
            "/api/v0/embed/transcribe",
            token,
            "audio/wav",
            Body::from(audio),
        );
        reply(common::serve_promptly(&self.fx.state, req).await)
            .await
            .0
    }

    async fn speak(&self, token: &str, turn_id: &str) -> (Reply, Vec<u8>) {
        let req = post(
            "/api/v0/embed/speak",
            token,
            "application/json",
            Body::from(json!({ "turn_id": turn_id }).to_string()),
        );
        reply(common::serve_promptly(&self.fx.state, req).await).await
    }

    /// Send `hello`, wait until the answer is final and released; its turn id.
    async fn answered(&self, token: &str) -> String {
        let r = self.say(token, "hello").await;
        assert_eq!(r.status, StatusCode::ACCEPTED, "{}", r.body);
        let turn = r.body["turn_id"].as_str().unwrap().to_string();
        let session = self.conversation_of(token).await;
        self.wait_terminal(&session, &turn).await;
        while self.fx.state.chats.get(&self.agent, &session).is_some() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        turn
    }

    /// The conversation's voice exchanges: `(purpose, turn_id, visitor_id, detail)`.
    async fn voice_events(
        &self,
        session: &str,
    ) -> Vec<(String, Option<String>, Option<String>, Value)> {
        let rows: Vec<(String, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT detail, turn_id, visitor_id FROM agent_audit \
             WHERE kind = 'llm_exchange' AND conversation_id = ? ORDER BY seq",
        )
        .bind(session)
        .fetch_all(&self.fx.state.db)
        .await
        .unwrap();
        rows.into_iter()
            .map(|(detail, turn, visitor)| {
                let detail: Value = serde_json::from_str(&detail).unwrap();
                (
                    detail["purpose"].as_str().unwrap_or("").to_string(),
                    turn,
                    visitor,
                    detail,
                )
            })
            .filter(|(purpose, ..)| purpose == "transcription" || purpose == "speech")
            .collect()
    }
}

#[tokio::test]
async fn a_recording_comes_back_as_text_for_the_visitor_to_send_and_is_not_kept() {
    let mock = upstream().await;
    let e = voice_embed(&mock, both(), json!({})).await;
    let token = e.visitor().await;
    let session = e.conversation_of(&token).await;

    let r = e.transcribe(&token, wav(1.0)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["text"], TRANSCRIPT);
    assert!(
        chat::list_turns(&e.fx.state.db, &session)
            .await
            .unwrap()
            .is_empty(),
        "the transcript is not sent on the visitor's behalf"
    );

    let sent = mock.received_requests().await.unwrap();
    let upload = sent
        .iter()
        .find(|r| r.url.path() == "/audio/transcriptions")
        .unwrap();
    let form = String::from_utf8_lossy(&upload.body);
    assert!(form.contains("whisper-1"), "the pool's model is asked");
    assert!(form.contains("RIFF"), "the recording is uploaded as WAV");

    let events = e.voice_events(&session).await;
    assert_eq!(events.len(), 1, "{events:?}");
    let (purpose, turn, visitor, detail) = &events[0];
    assert_eq!(purpose, "transcription");
    assert_eq!(turn, &None);
    assert!(visitor.is_some(), "the visitor is on the event");
    assert_eq!(
        detail["response"]["body"]["text"],
        format!(" {TRANSCRIPT} ")
    );
    assert_eq!(detail["request"]["file"]["seconds"], 1.0);
    let stored: Vec<String> = sqlx::query_scalar("SELECT detail FROM agent_audit")
        .fetch_all(&e.fx.state.db)
        .await
        .unwrap();
    assert!(
        stored.iter().all(|d| !d.contains("RIFF")),
        "no audio in the activity log"
    );
}

#[tokio::test]
async fn a_visitors_recording_is_booked_as_the_agents_usage() {
    let mock = upstream().await;
    let mut e = voice_embed(&mock, both(), json!({})).await;
    let metered = aiplane_core::server::usage::spawn(e.fx.state.db.clone(), 90);
    e.fx.state = e.fx.state.clone().with_usage(metered);
    let token = e.visitor().await;
    let r = e.transcribe(&token, wav(1.0)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);

    let mut rows: Vec<(String, String, String)> = Vec::new();
    for _ in 0..100 {
        rows = sqlx::query_as("SELECT user_id, source, kind FROM usage_events")
            .fetch_all(&e.fx.state.db)
            .await
            .unwrap();
        if !rows.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(
        rows,
        [(
            e.agent.clone(),
            "agent".to_string(),
            "transcription".to_string()
        )]
    );
}

#[tokio::test]
async fn voice_routes_are_not_there_for_an_agent_without_voice() {
    let mock = upstream().await;
    let e = voice_embed(
        &mock,
        json!({ "input": false, "output": false, "transcription_model": WHISPER, "speech_model": TTS_MODEL }),
        json!({}),
    )
    .await;
    let token = e.visitor().await;
    let r = e.transcribe(&token, wav(1.0)).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "{}", r.body);
    assert_eq!(code(&r), "voice_not_enabled");
    let (r, _) = e.speak(&token, "any").await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "{}", r.body);
    assert_eq!(code(&r), "voice_not_enabled");
    assert!(mock.received_requests().await.unwrap().is_empty());

    let (r, _) = reply(
        common::serve_promptly(
            &e.fx.state,
            post(
                "/api/v0/embed/transcribe",
                "gwv_nobody",
                "audio/wav",
                Body::from(wav(1.0)),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED, "a visitor token first");
}

#[tokio::test]
async fn a_recording_is_one_short_clip_of_the_recorders_format() {
    let mock = upstream().await;
    let e = voice_embed(&mock, both(), json!({})).await;
    let token = e.visitor().await;

    let r = e.transcribe(&token, vec![0; 2 * 1024 * 1024 + 1]).await;
    assert_eq!(r.status, StatusCode::PAYLOAD_TOO_LARGE, "{}", r.body);
    assert_eq!(code(&r), "payload_too_large");
    assert!(r.body.to_string().contains("2 MiB"), "{}", r.body);

    let r = reply(
        common::serve_promptly(
            &e.fx.state,
            post(
                "/api/v0/embed/transcribe",
                &token,
                "audio/wav",
                common::endless_body(),
            ),
        )
        .await,
    )
    .await
    .0;
    assert_eq!(r.status, StatusCode::PAYLOAD_TOO_LARGE, "{}", r.body);

    let r = e.transcribe(&token, wav(61.0)).await;
    assert_eq!(r.status, StatusCode::PAYLOAD_TOO_LARGE, "{}", r.body);
    assert_eq!(code(&r), "audio_too_long");

    let r = e.transcribe(&token, wav(0.2)).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.body);
    assert_eq!(code(&r), "audio_too_short");

    let r = e.transcribe(&token, b"OggS not a wav".to_vec()).await;
    assert_eq!(r.status, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{}", r.body);
    assert_eq!(code(&r), "unsupported_audio");

    assert!(
        mock.received_requests().await.unwrap().is_empty(),
        "nothing refused reached the pool"
    );
}

#[tokio::test]
async fn voice_counts_against_the_visitors_rate() {
    let mock = upstream().await;
    let e = voice_embed(
        &mock,
        both(),
        json!({ "rate_limits": { "visitor": { "max": 2, "per": "10m" } } }),
    )
    .await;
    let token = e.visitor().await;
    let turn = e.answered(&token).await;
    let (r, _) = e.speak(&token, &turn).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);

    let r = e.transcribe(&token, wav(1.0)).await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS, "{}", r.body);
    assert_eq!(code(&r), "visitor_rate_limited");
    assert!(r.headers.contains_key(header::RETRY_AFTER));
    let (r, _) = e.speak(&token, &turn).await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS, "{}", r.body);
}

#[tokio::test]
async fn a_spent_budget_silences_voice_too() {
    let mock = upstream().await;
    let e = voice_embed(
        &mock,
        both(),
        json!({ "budget": { "monthly_tokens": 100 } }),
    )
    .await;
    let token = e.visitor().await;
    let turn = e.answered(&token).await;
    usage::insert_batch(
        &e.fx.state.db,
        &[UsageRecord {
            created_at: jiff::Timestamp::now(),
            user_id: e.agent.clone(),
            user_email: None,
            token_id: None,
            token_name: None,
            source: UsageSource::Agent,
            kind: UsageKind::Chat,
            backend: "mock".into(),
            model: "m".into(),
            status: 200,
            duration_ms: 1,
            prompt_tokens: Some(150),
            completion_tokens: Some(0),
            total_tokens: Some(150),
            input_units: None,
            output_units: None,
            enforce_limits: true,
            principal_kind: PrincipalKind::System,
            agent_id: Some(e.agent.clone()),
            chain: None,
            stop_reason: None,
        }],
    )
    .await
    .unwrap();

    let r = e.transcribe(&token, wav(1.0)).await;
    assert_eq!(r.status, StatusCode::SERVICE_UNAVAILABLE, "{}", r.body);
    assert_eq!(code(&r), "agent_unavailable");
    let (r, _) = e.speak(&token, &turn).await;
    assert_eq!(r.status, StatusCode::SERVICE_UNAVAILABLE, "{}", r.body);
    assert!(mock.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn the_final_stored_answer_is_spoken_once_and_then_replayed() {
    let mock = upstream().await;
    let e = voice_embed(&mock, both(), json!({})).await;
    let token = e.visitor().await;
    let turn = e.answered(&token).await;
    let session = e.conversation_of(&token).await;
    let filtered = "Your order **ships** today. [Track it](https://example.com/t)";
    chat::set_content(&e.fx.state.db, &turn, filtered)
        .await
        .unwrap();

    let (r, audio) = e.speak(&token, &turn).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.headers[header::CONTENT_TYPE], "audio/mpeg");
    assert_eq!(audio, SPOKEN);
    let (r, again) = e.speak(&token, &turn).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(again, SPOKEN);

    let sent: Vec<Value> = mock
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == "/audio/speech")
        .map(|r| serde_json::from_slice(&r.body).unwrap())
        .collect();
    assert_eq!(sent.len(), 1, "a replay is served from memory");
    assert_eq!(sent[0]["input"], "Your order ships today. Track it");
    assert_ne!(sent[0]["input"], ANSWER, "the stored answer, as filtered");
    assert_eq!(sent[0]["voice"], "alloy");
    assert_eq!(sent[0]["model"], "tts-1");

    let events = e.voice_events(&session).await;
    assert_eq!(events.len(), 1, "{events:?}");
    let (purpose, at, _, detail) = &events[0];
    assert_eq!(purpose, "speech");
    assert_eq!(at.as_deref(), Some(turn.as_str()));
    assert_eq!(
        detail["request"]["input"],
        "Your order ships today. Track it"
    );
    assert_eq!(detail["response"]["body"]["bytes"], SPOKEN.len());
}

#[tokio::test]
async fn the_owners_voice_is_used_when_the_spec_names_one() {
    let mock = upstream().await;
    let mut voice = both();
    voice["voice"] = json!("thorsten");
    let e = voice_embed(&mock, voice, json!({})).await;
    let token = e.visitor().await;
    let turn = e.answered(&token).await;
    let (r, _) = e.speak(&token, &turn).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let sent = &mock.received_requests().await.unwrap()[0];
    let body: Value = serde_json::from_slice(&sent.body).unwrap();
    assert_eq!(body["voice"], "thorsten");
}

/// A speech model's voices do not depend on its backends being up: during
/// an outage the setup still lists them and a valid voice still publishes.
#[tokio::test]
async fn a_speech_models_voices_survive_an_outage_of_its_backends() {
    let mock = upstream().await;
    let e = voice_embed(&mock, both(), json!({})).await;
    for pool in e.fx.state.upstreams.pools() {
        for backend in &pool.backends {
            backend.set_healthy(false);
        }
    }
    let (_, body) = e.fx.get(&e.fx.alice, "/api/v0/agent-resources").await;
    let tts = body["models"]["speech"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == TTS_MODEL)
        .cloned()
        .unwrap_or_else(|| panic!("{TTS_MODEL} is not offered: {body}"));
    assert_eq!(tts["voices"], json!(["alloy", "thorsten"]), "{body}");

    let mut s = spec("v2");
    let mut voice = both();
    voice["voice"] = json!("thorsten");
    s["publish"]["voice"] = voice;
    let (status, body) = e.fx.put_draft(&e.fx.alice, &e.agent, s).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = e.fx.publish(&e.fx.alice, &e.agent).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

/// The setup offers a speech model's own voices, and publishing refuses
/// one it does not offer, naming those it does.
#[tokio::test]
async fn a_voice_the_speech_model_does_not_offer_is_refused_at_publish() {
    let mock = upstream().await;
    let e = voice_embed(&mock, both(), json!({})).await;
    let (status, body) = e.fx.get(&e.fx.alice, "/api/v0/agent-resources").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let tts = body["models"]["speech"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == TTS_MODEL)
        .cloned()
        .unwrap_or_else(|| panic!("{TTS_MODEL} is not offered: {body}"));
    assert_eq!(tts["voices"], json!(["alloy", "thorsten"]));

    let mut s = spec("v2");
    let mut voice = both();
    voice["voice"] = json!("nova");
    s["publish"]["voice"] = voice;
    let (status, body) = e.fx.put_draft(&e.fx.alice, &e.agent, s.clone()).await;
    assert_eq!(status, StatusCode::OK, "a draft may hold it: {body}");
    let (status, body) = e.fx.publish(&e.fx.alice, &e.agent).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let issue = &body["error"]["issues"][0];
    assert_eq!(issue["path"], "publish.voice.voice");
    let message = issue["message"].as_str().unwrap();
    assert!(
        message.contains("`nova`") && message.contains("alloy, thorsten"),
        "{issue}"
    );

    s["publish"]["voice"]["voice"] = json!("alloy");
    let (status, body) = e.fx.put_draft(&e.fx.alice, &e.agent, s).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = e.fx.publish(&e.fx.alice, &e.agent).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

#[tokio::test]
async fn only_a_finished_answer_of_this_visitors_conversation_is_spoken() {
    let mock = upstream().await;
    let gate = Arc::new(Notify::new());
    let e = voice_embed_on(&mock, both(), json!({}), ScriptedRunner::held(gate.clone())).await;
    let token = e.visitor().await;
    let r = e.say(&token, "hello").await;
    let running = r.body["turn_id"].as_str().unwrap().to_string();
    let user_turn = r.body["user_turn_id"].as_str().unwrap().to_string();

    let (r, _) = e.speak(&token, &running).await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.body);
    assert_eq!(code(&r), "turn_not_final");
    let (r, _) = e.speak(&token, &user_turn).await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.body);
    assert_eq!(code(&r), "turn_not_final");
    let (r, _) = e.speak(&token, "no-such-turn").await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "{}", r.body);
    assert_eq!(code(&r), "turn_not_found");

    gate.notify_one();
    let session = e.conversation_of(&token).await;
    e.wait_terminal(&session, &running).await;
    while e.fx.state.chats.get(&e.agent, &session).is_some() {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let other = e.visitor().await;
    let (r, _) = e.speak(&other, &running).await;
    assert_eq!(
        r.status,
        StatusCode::NOT_FOUND,
        "another visitor's answer: {}",
        r.body
    );
    let (r, _) = e.speak(&token, &running).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
}

#[tokio::test]
async fn the_widget_learns_which_voice_directions_the_agent_offers() {
    let mock = upstream().await;
    let e = voice_embed(
        &mock,
        json!({ "input": true, "transcription_model": WHISPER }),
        json!({}),
    )
    .await;
    let r = e
        .send(
            Method::POST,
            "/api/v0/embed/agent",
            Some(SITE),
            None,
            Some(json!({ "key": e.key })),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(
        r.body["agent"]["voice"],
        json!({ "input": true, "output": false })
    );
    let started = e.start_with(&e.key, Some(SITE)).await;
    assert_eq!(started.body["agent"]["voice"]["input"], true);
    assert_eq!(started.body["agent"]["color"], Value::Null);

    let mut coloured = spec("v2");
    coloured["profile"] = json!({ "color": "#FFD400" });
    let (status, body) = e.fx.put_draft(&e.fx.alice, &e.agent, coloured).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        e.fx.publish(&e.fx.alice, &e.agent).await.0,
        StatusCode::CREATED
    );
    let started = e.start_with(&e.key, Some(SITE)).await;
    assert_eq!(started.body["agent"]["color"], "#ffd400");
    assert_eq!(started.body["agent"]["voice"]["input"], false);

    let r = e
        .send(
            Method::POST,
            "/api/v0/embed/agent",
            Some("https://evil.example"),
            None,
            Some(json!({ "key": e.key })),
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{}", r.body);
}

#[tokio::test]
async fn the_recorder_worklet_is_served_to_the_widget() {
    let mock = upstream().await;
    let e = voice_embed(&mock, both(), json!({})).await;
    let resp = common::app(e.fx.state.clone())
        .serve(
            Request::builder()
                .uri("/api/v0/embed/recorder.js")
                .header(header::ORIGIN, SITE)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()[header::CONTENT_TYPE], "text/javascript");
    assert_eq!(resp.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN], SITE);
    let body = String::from_utf8(common::read_body(resp).await.to_vec()).unwrap();
    assert!(body.contains("registerProcessor('pcm-recorder'"), "{body}");
    assert_eq!(
        body,
        include_str!("../../../../../web/static/pcm-recorder.js"),
        "the SPA's worklet, not a copy"
    );
}

#[tokio::test]
async fn a_manager_is_offered_the_speech_and_transcription_models_they_hold() {
    let mock = upstream().await;
    let e = voice_embed(&mock, both(), json!({})).await;
    let (status, body) = e.fx.get(&e.fx.alice, "/api/v0/agent-resources").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(model_ids(&body, "speech"), [TTS_MODEL]);
    assert_eq!(model_ids(&body, "transcription"), [WHISPER]);
    assert_eq!(
        model_ids(&body, "chat"),
        ["m"],
        "each kind lists only its own models"
    );
}

fn model_ids(body: &Value, kind: &str) -> Vec<String> {
    body["models"][kind]
        .as_array()
        .unwrap_or_else(|| panic!("no models.{kind}: {body}"))
        .iter()
        .map(|m| m["id"].as_str().unwrap().to_string())
        .collect()
}

/// A direction that is on but names no model runs on the gateway's default
/// model for it (Models & routing → Default models), once the agent holds a
/// grant on it.
#[tokio::test]
async fn without_a_model_of_its_own_voice_runs_on_the_gateways_default_models() {
    use aiplane_core::server::feature_defaults::{self, Feature};

    let mock = upstream().await;
    let e = voice_embed(&mock, json!({ "input": true, "output": true }), json!({})).await;
    for p in e.fx.state.upstreams.pools() {
        if p.name == STT {
            p.backends[0].set_models(["whisper-1".into(), "whisper-large".into()].into());
        }
    }
    feature_defaults::set(
        &e.fx.state.db,
        Feature::Transcription,
        Some("whisper-large"),
    )
    .await
    .unwrap();
    let token = e.visitor().await;

    let r = e.transcribe(&token, wav(1.0)).await;
    assert_eq!(
        r.status,
        StatusCode::SERVICE_UNAVAILABLE,
        "an ungranted default is not used: {}",
        r.body
    );
    assert_eq!(
        e.fx.grant(&e.fx.alice, &e.agent, "model", "whisper-large")
            .await,
        StatusCode::CREATED
    );
    let r = e.transcribe(&token, wav(1.0)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["text"], TRANSCRIPT);
    let sent = mock.received_requests().await.unwrap();
    let upload = sent
        .iter()
        .find(|r| r.url.path() == "/audio/transcriptions")
        .unwrap();
    assert!(
        String::from_utf8_lossy(&upload.body).contains("whisper-large"),
        "the gateway's default transcription model is asked"
    );

    let turn = e.answered(&token).await;
    let (r, audio) = e.speak(&token, &turn).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(audio, SPOKEN);
}

/// Publishing a direction whose default model the agent holds no grant on
/// is refused with the step to fix it in; granting the model makes it
/// publishable.
#[tokio::test]
async fn voice_without_a_granted_model_is_refused_at_publish_naming_the_step() {
    use aiplane_core::server::feature_defaults::{self, Feature};

    let fx = agents::fixture_with_pools(
        None,
        ToolRegistry::new().with(CurrentTimestamp),
        &[TIME],
        vec![(
            STT.into(),
            voice_pool(PoolKind::Transcription, "http://127.0.0.1:9", &[]),
            vec!["whisper-1".into()],
        )],
    )
    .await;
    feature_defaults::set(&fx.state.db, Feature::Transcription, Some("whisper-1"))
        .await
        .unwrap();
    let agent = fx.runnable("support").await;
    let mut s = spec("v1");
    s["publish"]["voice"] = json!({ "input": true });
    let (status, body) = fx.put_draft(&fx.alice, &agent, s).await;
    assert_eq!(status, StatusCode::OK, "a draft may still lack it: {body}");

    let (status, body) = fx.publish(&fx.alice, &agent).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let issue = &body["error"]["issues"][0];
    assert_eq!(issue["path"], "publish.voice.transcription_model");
    assert!(
        issue["message"]
            .as_str()
            .unwrap()
            .contains("\"Website\" step"),
        "{issue}"
    );

    assert_eq!(
        fx.grant(&fx.alice, &agent, "model", WHISPER).await,
        StatusCode::CREATED
    );
    let (status, body) = fx.publish(&fx.alice, &agent).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

/// The setup's defaults are the gateway's, whoever asks; whether the manager
/// may grant one is whether their model list names it.
#[tokio::test]
async fn the_setup_defaults_are_the_gateways_and_the_lists_say_who_may_grant_them() {
    use aiplane_core::server::feature_defaults::{self, Feature};

    let mut private = voice_pool(PoolKind::Transcription, "http://127.0.0.1:9", &[]);
    private.allowed_groups = vec!["support".into()];
    let fx = agents::fixture_with_pools(
        Some("http://127.0.0.1:9"),
        ToolRegistry::new().with(CurrentTimestamp),
        &[TIME],
        vec![
            (
                STT.into(),
                voice_pool(PoolKind::Transcription, "http://127.0.0.1:9", &[]),
                vec![WHISPER.into()],
            ),
            ("stt-private".into(), private, vec!["whisper-large".into()]),
            (
                TTS.into(),
                voice_pool(PoolKind::Speech, "http://127.0.0.1:9", &[]),
                vec![TTS_MODEL.into()],
            ),
        ],
    )
    .await;
    feature_defaults::set(&fx.state.db, Feature::Transcription, Some("whisper-large"))
        .await
        .unwrap();
    feature_defaults::set(&fx.state.db, Feature::Speech, Some(TTS_MODEL))
        .await
        .unwrap();
    feature_defaults::set(&fx.state.db, Feature::Chat, Some("m"))
        .await
        .unwrap();

    let (status, body) = fx.get(&fx.alice, "/api/v0/agent-resources").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["defaults"],
        json!({ "chat": "m", "transcription": "whisper-large", "speech": TTS_MODEL }),
    );
    assert_eq!(
        model_ids(&body, "transcription"),
        [WHISPER],
        "the private pool's default is not the manager's to grant"
    );
    assert!(
        body.get("tiers").is_none() && body.get("pools").is_none(),
        "{body}"
    );

    let (_, body) = fx.get(&fx.root, "/api/v0/agent-resources").await;
    assert_eq!(
        model_ids(&body, "transcription"),
        ["whisper-large", WHISPER],
        "an admin may grant the default, which comes first"
    );
}
