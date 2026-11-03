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
    assert_eq!(p.level_rank, 0);
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
