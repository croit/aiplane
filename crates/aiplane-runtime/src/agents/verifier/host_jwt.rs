// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The `host_jwt` verifier: the embedding website vouches for the visitor
//! with a signed token, and its claims fill slots written as `host`.
//!
//! The website signs `{iss, aud, sub, exp, iat, jti?, …}` with HS256 (a
//! secret shared with the agent, sealed at rest) or RS256/ES256 (its public
//! key in the spec, or a JWKS address). The widget sends the token once per
//! conversation (`POST /api/v0/embed/identity`). Everything is checked here:
//!
//! - the header's `alg` must be the configured one, so a public key can
//!   never be used as an HMAC secret;
//! - signature, `exp`, `nbf`, `iss`, `aud` (30 s leeway);
//! - a short life: `exp - iat` at most `max_lifetime` (10 minutes unless
//!   the spec says otherwise), so a leaked token is soon worthless;
//! - a `jti`, when present, is accepted once per agent.
//!
//! A refused token writes nothing. An accepted one writes every mapped slot
//! or none.

use std::sync::LazyLock;
use std::time::Duration;

use aiplane_agents::db::agent_audit::{AuditKind, Correlation, NewEvent};
use aiplane_agents::db::agent_verifiers;
use aiplane_core::server::auth::jwks::JwksCache;
use aiplane_core::server::capped_read;
use aiplane_core::server::crypto::sha256_hex;
use aiplane_core::server::outbound_guard::{self, Policy};
use jiff::{SignedDuration, Timestamp};
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde_json::{Map, Value, json};

use super::JwtAlgorithm;
use crate::agents::embed::Refused;
use crate::agents::spec::AgentSpec;
use crate::agents::spec_cache::CompiledSpec;
use crate::agents::state::{StateSchema, TrustedWriter, write_trusted_all};
use crate::rama_server::state::RamaState;
use aiplane_agents::db::WriteTx;

const LEEWAY_SECS: u64 = 30;
const JWKS_TTL: Duration = Duration::from_secs(300);
/// The soonest a key set is fetched again for a `kid` it does not know.
const JWKS_REFETCH: Duration = Duration::from_secs(60);
const JWKS_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_JWKS_BYTES: usize = 64 * 1024;

impl JwtAlgorithm {
    fn jwt(self) -> Algorithm {
        match self {
            Self::Hs256 => Algorithm::HS256,
            Self::Rs256 => Algorithm::RS256,
            Self::Es256 => Algorithm::ES256,
        }
    }
}

/// Where the verifying key comes from.
#[derive(Debug, Clone)]
pub enum KeySource {
    /// The HS256 secret, sealed with the gateway's at-rest key.
    Sealed(String),
    PublicKey(String),
    Jwks(String),
}

/// What a claim fills: a slot with one claim, or a `subject` slot with an
/// object of field → claim.
#[derive(Debug, Clone, PartialEq)]
pub enum ClaimMap {
    Claim(String),
    Object(Vec<(String, String)>),
}

#[derive(Debug, Clone)]
pub struct HostJwt {
    pub id: String,
    pub algorithm: JwtAlgorithm,
    pub key: KeySource,
    pub issuer: String,
    pub audience: String,
    pub max_lifetime: SignedDuration,
    pub claims: Vec<(String, ClaimMap)>,
}

impl HostJwt {
    /// The spec's `host_jwt` verifier, when it has a complete one.
    pub fn from_spec(spec: &AgentSpec) -> Option<Self> {
        let (id, cfg) = spec.host_jwt()?;
        let algorithm = cfg.algorithm?;
        let key = match (&cfg.secret_sealed, &cfg.public_key, &cfg.jwks_url) {
            (Some(s), None, None) if algorithm == JwtAlgorithm::Hs256 => {
                KeySource::Sealed(s.clone())
            }
            (None, Some(pem), None) if algorithm != JwtAlgorithm::Hs256 => {
                KeySource::PublicKey(pem.clone())
            }
            (None, None, Some(url)) if algorithm != JwtAlgorithm::Hs256 => {
                KeySource::Jwks(url.clone())
            }
            _ => return None,
        };
        let claims: Vec<(String, ClaimMap)> = cfg
            .claims
            .iter()
            .map(|(slot, c)| (slot.clone(), c.clone()))
            .collect();
        Some(Self {
            id: id.to_string(),
            algorithm,
            key,
            issuer: cfg.issuer.clone()?,
            audience: cfg.audience.clone()?,
            max_lifetime: cfg.max_lifetime(),
            claims: (!claims.is_empty()).then_some(claims)?,
        })
    }
}

/// Why a token was refused. `code` is the stable API error code.
#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error(
        "this agent accepts no identity token: its spec has no complete `host_jwt` verifier. \
         The agent's owner configures one under `verifiers`"
    )]
    NotConfigured,
    #[error("the identity token was refused: {0}")]
    Invalid(Refusal),
    #[error(
        "this identity token was already used; the website has to sign a fresh one (with a new \
         `jti`) for each conversation"
    )]
    Replayed,
    /// `url` and `reason` are for the agent's owner (log and audit trail);
    /// the message a visitor sees names neither, so the endpoint cannot be
    /// used to probe what the gateway reaches.
    #[error(
        "the website's signing keys could not be fetched; try again shortly — the agent's owner \
         finds the reason in the agent's audit trail"
    )]
    KeysUnavailable { url: String, reason: String },
    #[error("storing the verified identity failed: {0}")]
    Storage(String),
}

impl IdentityError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotConfigured => "identity_not_configured",
            Self::Invalid(_) => "identity_token_invalid",
            Self::Replayed => "identity_token_replayed",
            Self::KeysUnavailable { .. } => "identity_keys_unavailable",
            Self::Storage(_) => "internal",
        }
    }
}

/// What was wrong with a token — for the website's developer, so each says
/// what to fix. None repeats a claim value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    #[error("it is not a JWT (three base64url parts separated by dots)")]
    Malformed,
    #[error("it is signed with `{found}`, but this agent expects `{expected}`")]
    WrongAlgorithm { expected: String, found: String },
    #[error("its signature does not match the configured key")]
    BadSignature,
    #[error("it has expired (`exp` is in the past); sign a fresh one")]
    Expired,
    #[error("it is not valid yet (`nbf` is in the future); check the website's clock")]
    NotYetValid,
    #[error("its `iss` is not the issuer this agent expects")]
    WrongIssuer,
    #[error("its `aud` does not name this agent's audience")]
    WrongAudience,
    #[error("it lacks the `{0}` claim")]
    MissingClaim(String),
    #[error(
        "it lives too long: `exp` - `iat` is {secs}s, and this agent accepts at most {max}s — \
         sign short-lived tokens"
    )]
    TooLong { secs: i64, max: i64 },
    #[error("no key in the website's JWKS matches its `kid`")]
    UnknownKey,
    #[error("its `{claim}` claim does not fit slot `{slot}`")]
    ClaimDoesNotFit { claim: String, slot: String },
}

impl Refusal {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Malformed => "malformed",
            Self::WrongAlgorithm { .. } => "wrong_algorithm",
            Self::BadSignature => "bad_signature",
            Self::Expired => "expired",
            Self::NotYetValid => "not_yet_valid",
            Self::WrongIssuer => "wrong_issuer",
            Self::WrongAudience => "wrong_audience",
            Self::MissingClaim(_) => "missing_claim",
            Self::TooLong { .. } => "lifetime_too_long",
            Self::UnknownKey => "unknown_key",
            Self::ClaimDoesNotFit { .. } => "claim_does_not_fit",
        }
    }
}

/// `pem` as the key `alg` verifies with, or why it is not one.
pub fn public_key(alg: JwtAlgorithm, pem: &str) -> Result<DecodingKey, String> {
    let parsed = match alg {
        JwtAlgorithm::Rs256 => DecodingKey::from_rsa_pem(pem.as_bytes()),
        JwtAlgorithm::Es256 => DecodingKey::from_ec_pem(pem.as_bytes()),
        JwtAlgorithm::Hs256 => return Err("HS256 takes a `secret`, not a public key".into()),
    };
    parsed.map_err(|e| {
        format!(
            "is not a PEM public key for this algorithm ({e}) — paste the website's \
             `-----BEGIN PUBLIC KEY-----` block"
        )
    })
}

/// Whether [`fetch_jwks`] may reach `url`, as far as that can be told before
/// resolving it: the same `outbound_guard` policy, so a spec is refused on
/// save for a URL its run would refuse.
pub fn check_jwks_url(url: &str, allow_private: bool) -> Result<(), String> {
    outbound_guard::check_url(url, Policy::agent(allow_private)).map(|_| ())
}

static JWKS: LazyLock<JwksCache<JwkSet>> =
    LazyLock::new(|| JwksCache::new(Some(JWKS_TTL), JWKS_REFETCH));

/// The key set at `url`, fetched through `outbound_guard`: the URL is
/// chosen by an agent's owner, so it gets the same resolve-and-pin, no
/// redirects and private-network rule as an A2A route.
async fn fetch_jwks(state: &RamaState, url: &str) -> Result<JwkSet, IdentityError> {
    let unavailable = |reason: String| {
        tracing::warn!(url, reason = %reason, "fetching a host_jwt JWKS failed");
        IdentityError::KeysUnavailable {
            url: url.to_string(),
            reason,
        }
    };
    let allow_private = state.config().network.allow_private_networks;
    let pinned = outbound_guard::pin(url, Policy::agent(allow_private), JWKS_TIMEOUT)
        .await
        .map_err(unavailable)?;
    let resp = pinned
        .client
        .get(pinned.url)
        .header("accept", "application/json")
        .send()
        .await
        .map_err(|e| unavailable(format!("the request failed: {e}")))?;
    if !resp.status().is_success() {
        return Err(unavailable(format!("it answered {}", resp.status())));
    }
    let bytes = capped_read::read_capped_for(resp, MAX_JWKS_BYTES, "the JWKS document")
        .await
        .map_err(unavailable)?;
    serde_json::from_slice(&bytes).map_err(|e| unavailable(format!("not a JWKS document: {e}")))
}

/// The JWKS key for `kid`: from the shared cache while it is fresh and knows
/// the key, refetched otherwise (a rotated key appears without a restart),
/// but never more than once per `JWKS_REFETCH` — a token with a made-up
/// `kid` must not make the gateway fetch the website's keys on every request.
async fn jwks_key(
    state: &RamaState,
    url: &str,
    kid: Option<&str>,
) -> Result<DecodingKey, IdentityError> {
    let find = |set: &JwkSet| match kid {
        Some(kid) => set.find(kid).cloned(),
        None if set.keys.len() == 1 => set.keys.first().cloned(),
        None => None,
    };
    let jwk = JWKS
        .key(url, find, || fetch_jwks(state, url))
        .await?
        .ok_or(IdentityError::Invalid(Refusal::UnknownKey))?;
    DecodingKey::from_jwk(&jwk).map_err(|_| IdentityError::Invalid(Refusal::UnknownKey))
}

async fn key_for(
    state: &RamaState,
    cfg: &HostJwt,
    kid: Option<&str>,
) -> Result<DecodingKey, IdentityError> {
    match &cfg.key {
        KeySource::Sealed(sealed) => {
            let secret = state.crypto.open_from_string(sealed).ok_or_else(|| {
                IdentityError::Storage(
                    "the host_jwt secret cannot be opened with this gateway's key — save the \
                     secret in the agent's spec again"
                        .into(),
                )
            })?;
            // An empty HMAC key is refused by jsonwebtoken rather than turned
            // into a verifier that accepts a token signed with nothing.
            DecodingKey::from_secret(secret.as_bytes()).map_err(|_| {
                IdentityError::Storage(
                    "the host_jwt secret is empty, and an empty key cannot verify a token — \
                     save a secret in the agent's spec"
                        .into(),
                )
            })
        }
        KeySource::PublicKey(pem) => public_key(cfg.algorithm, pem).map_err(IdentityError::Storage),
        KeySource::Jwks(url) => jwks_key(state, url, kid).await,
    }
}

fn refusal(err: &jsonwebtoken::errors::Error) -> Refusal {
    use jsonwebtoken::errors::ErrorKind as K;
    match err.kind() {
        K::ExpiredSignature => Refusal::Expired,
        K::ImmatureSignature => Refusal::NotYetValid,
        K::InvalidIssuer => Refusal::WrongIssuer,
        K::InvalidAudience => Refusal::WrongAudience,
        K::MissingRequiredClaim(c) => Refusal::MissingClaim(c.clone()),
        K::InvalidSignature => Refusal::BadSignature,
        _ => Refusal::Malformed,
    }
}

/// Verify `token` against `cfg`; its claims when it holds.
async fn verified_claims(
    state: &RamaState,
    cfg: &HostJwt,
    token: &str,
    now: Timestamp,
) -> Result<Map<String, Value>, IdentityError> {
    let invalid = IdentityError::Invalid;
    let header = decode_header(token).map_err(|_| invalid(Refusal::Malformed))?;
    let expected = cfg.algorithm.jwt();
    if header.alg != expected {
        return Err(invalid(Refusal::WrongAlgorithm {
            expected: format!("{expected:?}"),
            found: format!("{:?}", header.alg),
        }));
    }
    let key = key_for(state, cfg, header.kid.as_deref()).await?;
    let mut validation = Validation::new(expected);
    validation.leeway = LEEWAY_SECS;
    validation.validate_nbf = true;
    validation.set_issuer(&[&cfg.issuer]);
    validation.set_audience(&[&cfg.audience]);
    validation.set_required_spec_claims(&["exp", "iat", "iss", "aud"]);
    let data =
        decode::<Map<String, Value>>(token, &key, &validation).map_err(|e| invalid(refusal(&e)))?;
    let claims = data.claims;
    let (Some(exp), Some(iat)) = (
        claims.get("exp").and_then(Value::as_i64),
        claims.get("iat").and_then(Value::as_i64),
    ) else {
        return Err(invalid(Refusal::MissingClaim("iat".into())));
    };
    let max = cfg.max_lifetime.as_secs();
    if exp - iat > max {
        return Err(invalid(Refusal::TooLong {
            secs: exp - iat,
            max,
        }));
    }
    if iat > now.as_second() + LEEWAY_SECS as i64 {
        return Err(invalid(Refusal::NotYetValid));
    }
    Ok(claims)
}

fn claim<'a>(claims: &'a Map<String, Value>, name: &str) -> Result<&'a Value, Refusal> {
    claims
        .get(name)
        .filter(|v| !v.is_null())
        .ok_or_else(|| Refusal::MissingClaim(name.to_string()))
}

/// Every mapped slot's value, checked against the slot, or the first that
/// does not fit.
fn slot_values(
    cfg: &HostJwt,
    schema: &StateSchema,
    claims: &Map<String, Value>,
) -> Result<Vec<(String, Value)>, Refusal> {
    let mut out = Vec::new();
    for (slot, map) in &cfg.claims {
        let (value, named) = match map {
            ClaimMap::Claim(c) => (claim(claims, c)?.clone(), c.clone()),
            ClaimMap::Object(fields) => {
                let mut obj = Map::new();
                for (field, c) in fields {
                    obj.insert(field.clone(), claim(claims, c)?.clone());
                }
                let names: Vec<&str> = fields.iter().map(|(_, c)| c.as_str()).collect();
                (Value::Object(obj), names.join(", "))
            }
        };
        let fits = schema.slot(slot).is_some_and(|d| d.check(&value).is_ok());
        if !fits {
            return Err(Refusal::ClaimDoesNotFit {
                claim: named,
                slot: slot.clone(),
            });
        }
        out.push((slot.clone(), value));
    }
    Ok(out)
}

/// Accept `token` for conversation `session_id` of agent `agent_id`, whose
/// version runs `spec`: verify it and write its mapped slots as `host`.
/// Returns the slots written. The outcome is a `host_identity` event of the
/// conversation's chain.
pub async fn accept(
    state: &RamaState,
    agent_id: &str,
    session_id: &str,
    spec: &CompiledSpec,
    token: &str,
    now: Timestamp,
) -> Result<Vec<String>, IdentityError> {
    let outcome = accept_inner(state, agent_id, session_id, spec, token, now).await;
    let detail = match &outcome {
        Ok(slots) => json!({ "session_id": session_id, "outcome": "accepted", "slots": slots }),
        Err(IdentityError::Invalid(r)) => {
            json!({ "session_id": session_id, "outcome": "refused", "reason": r.as_str() })
        }
        Err(e @ IdentityError::KeysUnavailable { url, reason }) => json!({
            "session_id": session_id, "outcome": "refused", "reason": e.code(),
            "jwks_url": url, "error": reason,
        }),
        Err(e) => json!({ "session_id": session_id, "outcome": "refused", "reason": e.code() }),
    };
    match &outcome {
        Err(IdentityError::NotConfigured) => {}
        Ok(_) => {
            let event = NewEvent::new(AuditKind::HostIdentity, agent_id, detail).at(Correlation {
                session_id: Some(session_id.to_string()),
                ..Correlation::default()
            });
            let _ = crate::agents::audit::record_event(&state.db, event).await;
        }
        // A website (or someone posing as it) can post bad tokens as fast
        // as it likes; each reason is counted in a window, not a row each.
        Err(e) => {
            let refused = Refused {
                agent_id,
                subject: session_id,
                class: format!(
                    "host_identity:{}",
                    detail["reason"].as_str().unwrap_or(e.code())
                ),
                kind: AuditKind::HostIdentity,
                conversation_id: Some(session_id),
                detail,
            };
            state.refusals.fold(&state.db, refused).await;
        }
    }
    outcome
}

async fn accept_inner(
    state: &RamaState,
    agent_id: &str,
    session_id: &str,
    spec: &CompiledSpec,
    token: &str,
    now: Timestamp,
) -> Result<Vec<String>, IdentityError> {
    let parts = spec.parts().map_err(|_| IdentityError::NotConfigured)?;
    let cfg = HostJwt::from_spec(&parts.agent).ok_or(IdentityError::NotConfigured)?;
    let schema = &*parts.schema;
    let claims = verified_claims(state, &cfg, token.trim(), now).await?;
    let values = slot_values(&cfg, schema, &claims).map_err(IdentityError::Invalid)?;
    let storage = |e: &dyn std::fmt::Display| IdentityError::Storage(e.to_string());
    // One transaction: the `jti` is spent only together with every slot, so
    // a failed write neither leaves half an identity behind nor burns the
    // token the website will retry with.
    let mut tx = WriteTx::begin(&state.db).await.map_err(|e| storage(&e))?;
    if let Some(jti) = claims.get("jti").and_then(Value::as_str) {
        let exp = claims
            .get("exp")
            .and_then(Value::as_i64)
            .and_then(|s| Timestamp::from_second(s).ok())
            .unwrap_or(now);
        let first =
            agent_verifiers::use_jti(&mut tx, agent_id, &sha256_hex(jti.as_bytes()), exp, now)
                .await
                .map_err(|e| storage(&e))?;
        if !first {
            return Err(IdentityError::Replayed);
        }
    }
    write_trusted_all(
        &mut tx,
        schema,
        session_id,
        &values,
        TrustedWriter::Host,
        now,
    )
    .await
    .map_err(|e| storage(&e))?;
    tx.commit().await.map_err(|e| storage(&e))?;
    Ok(values.into_iter().map(|(slot, _)| slot).collect())
}
