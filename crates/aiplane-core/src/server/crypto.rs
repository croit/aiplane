// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! At-rest encryption for per-user MCP OAuth tokens, admin-stored connector
//! client secrets, and upstream backend API keys.
//!
//! These are dynamic, admin/per-user-managed secrets that can't live in env
//! vars the way the gateway's other credentials do — an operator adds a backend
//! (with its key) through the admin UI at runtime, so the key must persist in
//! the database as AES-256-GCM ciphertext rather than a process env var. Each value is encrypted under a fresh
//! random 96-bit nonce; the DB layer keeps the `(nonce, ciphertext)` pair
//! opaquely and never sees plaintext.
//!
//! Key material comes from `$AIPLANE_ENCRYPTION_KEY` (64 hex chars = 32 bytes)
//! when set; otherwise it is derived from the session secret via HMAC-SHA256 so
//! a deployment that already configured `$AIPLANE_SESSION_KEY` gets stable,
//! restart-surviving encryption for free. With neither configured (dev), an
//! ephemeral key is used and a warning logged — stored secrets won't decrypt
//! after a restart (reconnect / re-enter them).

// `aes-gcm` 0.10 pulls `generic-array` 0.14 via `aead`/`crypto-common`, whose
// `GenericArray` re-export carries an "upgrade to generic-array 1.x"
// deprecation we can't act on without bumping the whole crypto stack. Scope the
// allow to this small, self-contained module so `clippy -D warnings` stays
// clean; revisit when `aes-gcm` moves to generic-array 1.x.
#![allow(deprecated)]

use aes_gcm::Aes256Gcm;
use aes_gcm::aead::generic_array::GenericArray;
use aes_gcm::aead::{Aead, KeyInit};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, Mac};
use rand::TryRng;
use sha2::Sha256;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("encryption failed")]
    Encrypt,
    #[error("decryption failed (wrong key, or value was stored under a different key)")]
    Decrypt,
    #[error("generating nonce: {0}")]
    Nonce(String),
}

/// A loaded encryption key wrapped behind AES-256-GCM. Cheap to clone (holds a
/// 32-byte key); share it via `Arc` in `AppState`.
#[derive(Clone)]
pub struct Crypto {
    key: [u8; 32],
    /// The keys every *retired* derivation label produces from the same
    /// session secret, tried in order by [`Crypto::open`] when the current key
    /// fails.
    ///
    /// A list rather than one slot, because the label has now been retired
    /// twice (see [`RETIRED_LABELS`]) and a deployment that skips releases must
    /// not fall through the gap. Each rename shipped for the same reason: every
    /// value sealed under the old label — backend API keys, connector client
    /// secrets, each user's OAuth tokens — would otherwise stop decrypting, and
    /// "re-enter every upstream credential" is not a migration path.
    /// [`Self::is_legacy_sealed`] lets [`crate::server::db::reseal`] notice and
    /// rewrite them under the current key.
    ///
    /// Empty when the key came from `$AIPLANE_ENCRYPTION_KEY`: an explicit key
    /// is used verbatim, so no label was ever involved and there is nothing to
    /// fall back to.
    legacy: Vec<[u8; 32]>,
}

impl std::fmt::Debug for Crypto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print the key.
        f.write_str("Crypto(<key elided>)")
    }
}

/// One encrypted value: a 96-bit nonce and the GCM ciphertext (which includes
/// the auth tag). Both are stored as SQLite BLOBs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sealed {
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
}

/// Domain-separation label for the at-rest key.
///
/// Changing it derives a different key and orphans everything already sealed,
/// so it is not a knob. A deliberate rotation means three things together:
/// move the current value to the front of [`RETIRED_LABELS`], set the new one
/// here, and let [`crate::server::db::reseal`] rewrite the stored values on the
/// next boot. Editing this in place without the other two loses every secret in
/// every existing database.
pub(crate) const LABEL: &[u8] = b"croit-aiplane/at-rest-encryption/v1";

/// Labels this key has been derived under before, newest first.
///
/// [`Crypto::open`] tries each in turn, so a database is readable no matter
/// which release sealed it — including one that skipped the release where a
/// label was retired. Nothing is ever removed from here without a breaking
/// release that says so; see `docs/renaming.md`.
///
/// * `croit-llm-gateway/at-rest-encryption/v1` — used until the project was
///   renamed to croit AIplane.
/// * `croit-llm-gateway/mcp-token-encryption/v1` — used while at-rest sealing
///   covered only MCP tokens.
pub(crate) const RETIRED_LABELS: &[&[u8]] = &[
    b"croit-llm-gateway/at-rest-encryption/v1",
    b"croit-llm-gateway/mcp-token-encryption/v1",
];

/// HKDF-lite: HMAC-SHA256(session_secret, label).
pub(crate) fn derive(session_secret: &[u8; 32], label: &[u8]) -> [u8; 32] {
    let mut mac =
        <Hmac<Sha256> as Mac>::new_from_slice(session_secret).expect("HMAC accepts any key length");
    mac.update(label);
    let derived = mac.finalize().into_bytes();
    let mut key = [0u8; 32];
    key.copy_from_slice(&derived);
    key
}

fn open_with(key: &[u8; 32], nonce: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let nonce_arr: [u8; 12] = nonce.try_into().map_err(|_| CryptoError::Decrypt)?;
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| CryptoError::Decrypt)?;
    cipher
        .decrypt(&GenericArray::from(nonce_arr), ciphertext)
        .map_err(|_| CryptoError::Decrypt)
}

impl Crypto {
    /// Build from explicit 32-byte key material (used by tests).
    pub fn from_key(key: [u8; 32]) -> Self {
        Self {
            key,
            legacy: Vec::new(),
        }
    }

    /// A random, process-lifetime key. Used as the `AppState::new` default so
    /// the type is always present; production overrides it via
    /// [`Crypto::from_env_or_session`]. Stored secrets sealed under an
    /// ephemeral key won't survive a restart — acceptable for tests/dev.
    pub fn ephemeral() -> Self {
        let mut key = [0u8; 32];
        // OsRng failing is catastrophic and vanishingly rare; fall back to a
        // fixed key rather than panic so a misconfigured host still boots.
        if rand::rngs::SysRng.try_fill_bytes(&mut key).is_err() {
            key = [0u8; 32];
        }
        Self {
            key,
            legacy: Vec::new(),
        }
    }

    /// Resolve the key: `$AIPLANE_ENCRYPTION_KEY` (64 hex chars) wins; otherwise
    /// derive a stable key from the session secret; if that's all-zero
    /// (ephemeral session key path) we still derive deterministically from it
    /// so the process is internally consistent for its lifetime.
    pub fn from_env_or_session(session_secret: &[u8; 32]) -> Self {
        if let Ok(raw) = crate::server::env::var("AIPLANE_ENCRYPTION_KEY")
            && !raw.is_empty()
        {
            match hex_decode(&raw) {
                Some(bytes) if bytes.len() == 32 => {
                    let mut key = [0u8; 32];
                    key.copy_from_slice(&bytes);
                    return Self {
                        key,
                        legacy: Vec::new(),
                    };
                }
                _ => {
                    tracing::warn!(
                        "AIPLANE_ENCRYPTION_KEY must be 64 hex chars (32 bytes); ignoring it and \
                         deriving the at-rest encryption key from the session secret instead"
                    );
                }
            }
        }
        Self::from_session(session_secret)
    }

    /// The derived path on its own, without consulting the environment.
    ///
    /// HKDF-lite: HMAC-SHA256(session_secret, [`LABEL`]). The label names this
    /// key's purpose and is what separates it from any other key derived from
    /// the same session secret. Every entry of [`RETIRED_LABELS`] is derived
    /// alongside it so values written under an older label still open — see
    /// [`Self::legacy`].
    pub fn from_session(session_secret: &[u8; 32]) -> Self {
        Self {
            key: derive(session_secret, LABEL),
            legacy: RETIRED_LABELS
                .iter()
                .map(|label| derive(session_secret, label))
                .collect(),
        }
    }

    /// Encrypt `plaintext` under a fresh random nonce.
    pub fn seal(&self, plaintext: &[u8]) -> Result<Sealed, CryptoError> {
        let cipher = Aes256Gcm::new_from_slice(&self.key).map_err(|_| CryptoError::Encrypt)?;
        let mut nonce_bytes = [0u8; 12];
        rand::rngs::SysRng
            .try_fill_bytes(&mut nonce_bytes)
            .map_err(|e| CryptoError::Nonce(e.to_string()))?;
        // The nonce GenericArray size is inferred (U12) from `encrypt`'s
        // expected `&Nonce<Aes256Gcm>` argument, so we never name the alias.
        let nonce = GenericArray::from(nonce_bytes);
        let ciphertext = cipher
            .encrypt(&nonce, plaintext)
            .map_err(|_| CryptoError::Encrypt)?;
        Ok(Sealed {
            nonce: nonce_bytes.to_vec(),
            ciphertext,
        })
    }

    /// Convenience: seal a string.
    pub fn seal_str(&self, plaintext: &str) -> Result<Sealed, CryptoError> {
        self.seal(plaintext.as_bytes())
    }

    /// Decrypt a `(nonce, ciphertext)` pair.
    pub fn open(&self, nonce: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if let Ok(plain) = open_with(&self.key, nonce, ciphertext) {
            return Ok(plain);
        }
        // Sealed under an older derivation label. Opening it is the whole
        // point — the alternative shipped once already, and it was "re-enter
        // every upstream credential".
        self.legacy
            .iter()
            .find_map(|legacy| open_with(legacy, nonce, ciphertext).ok())
            .ok_or(CryptoError::Decrypt)
    }

    /// Whether `nonce`/`ciphertext` needs a retired key, i.e. whether it should
    /// be re-sealed. `false` for anything the current key opens, and for
    /// anything no key opens (that is not a migration, it is a wrong key).
    pub fn is_legacy_sealed(&self, nonce: &[u8], ciphertext: &[u8]) -> bool {
        if open_with(&self.key, nonce, ciphertext).is_ok() {
            return false;
        }
        self.legacy
            .iter()
            .any(|legacy| open_with(legacy, nonce, ciphertext).is_ok())
    }

    /// [`Self::is_legacy_sealed`] for the `"<nonce>.<ciphertext>"` string form.
    pub fn is_legacy_sealed_string(&self, stored: &str) -> bool {
        let Some((nonce, ciphertext)) = stored.split_once('.') else {
            return false;
        };
        let Some(nonce) = URL_SAFE_NO_PAD.decode(nonce).ok() else {
            return false;
        };
        let Some(ciphertext) = URL_SAFE_NO_PAD.decode(ciphertext).ok() else {
            return false;
        };
        self.is_legacy_sealed(&nonce, &ciphertext)
    }

    /// Convenience: decrypt to a UTF-8 string.
    pub fn open_str(&self, nonce: &[u8], ciphertext: &[u8]) -> Result<String, CryptoError> {
        let bytes = self.open(nonce, ciphertext)?;
        String::from_utf8(bytes).map_err(|_| CryptoError::Decrypt)
    }

    /// Seal a string into the single-column `"<nonce>.<ciphertext>"` form
    /// (base64url, unpadded) used by every secret that lives in an
    /// `app_settings` row rather than in its own pair of BLOB columns —
    /// the VAPID private key, the Brave API key, the OIDC client secret.
    pub fn seal_to_string(&self, plaintext: &str) -> Result<String, CryptoError> {
        self.seal_bytes_to_string(plaintext.as_bytes())
    }

    /// [`Self::seal_to_string`] for material that is not UTF-8 — a raw private
    /// key, say. Same stored shape, so the two are interchangeable at rest.
    pub fn seal_bytes_to_string(&self, plaintext: &[u8]) -> Result<String, CryptoError> {
        let sealed = self.seal(plaintext)?;
        Ok(format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(&sealed.nonce),
            URL_SAFE_NO_PAD.encode(&sealed.ciphertext)
        ))
    }

    /// Inverse of [`Self::seal_to_string`]. `None` covers both a malformed
    /// value and one sealed under a different key — callers treat either as
    /// "not configured", which is the actionable truth, and log once.
    pub fn open_from_string(&self, stored: &str) -> Option<String> {
        let bytes = self.open_bytes_from_string(stored)?;
        String::from_utf8(bytes).ok()
    }

    /// Inverse of [`Self::seal_bytes_to_string`].
    pub fn open_bytes_from_string(&self, stored: &str) -> Option<Vec<u8>> {
        let (nonce, ciphertext) = stored.split_once('.')?;
        let nonce = URL_SAFE_NO_PAD.decode(nonce).ok()?;
        let ciphertext = URL_SAFE_NO_PAD.decode(ciphertext).ok()?;
        self.open(&nonce, &ciphertext).ok()
    }
}

/// Decode a lowercase/uppercase hex string into bytes. `None` on odd length or
/// a non-hex digit. The inverse of [`hex_encode`], and the one home for both.
pub fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

const HEX: &[u8; 16] = b"0123456789abcdef";

/// Lowercase hex. The one home for it — `auth::token` and `server::setup` both
/// hash identifiers into this form, and a second implementation is a second
/// thing to get subtly wrong.
pub fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(*b >> 4) as usize] as char);
        out.push(HEX[(*b & 0x0f) as usize] as char);
    }
    out
}

/// Lowercase-hex SHA-256. Used wherever a secret is stored as a digest rather
/// than as itself — API tokens, webhook secrets, the setup recovery token.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    hex_encode(&Sha256::digest(bytes))
}

/// `n` random bytes, lowercase hex — the shape every opaque credential in the
/// gateway takes. Panics only if the OS RNG fails, which is not a condition
/// any caller can sensibly handle.
pub fn random_hex(n: usize) -> String {
    let mut bytes = vec![0u8; n];
    rand::rngs::SysRng
        .try_fill_bytes(&mut bytes)
        .expect("OS RNG must succeed");
    hex_encode(&bytes)
}

#[cfg(test)]
mod tests {
    /// The OS RNG actually produces entropy.
    ///
    /// Worth pinning because it is a *rename* away from a mistake: rand 0.10
    /// moved this generator from `OsRng` to `SysRng`, and everything security
    /// bearing here — session ids, AES-GCM nonces, OIDC state, PKCE verifiers,
    /// the derived at-rest key — draws from it. A generator that silently
    /// produced zeros would still round-trip through every other test in this
    /// file.
    #[test]
    fn the_os_rng_produces_entropy() {
        use rand::TryRng;
        let mut a = [0u8; 32];
        let mut b = [0u8; 32];
        rand::rngs::SysRng.try_fill_bytes(&mut a).expect("OS RNG");
        rand::rngs::SysRng.try_fill_bytes(&mut b).expect("OS RNG");
        assert_ne!(a, [0u8; 32], "all zeros is not entropy");
        assert_ne!(a, b, "two draws must differ");
        // ~128 of 256 bits set. A wide band: this catches a constant or a
        // stuck source, not a statistical quality claim.
        let set_bits: u32 = a.iter().map(|byte| byte.count_ones()).sum();
        assert!(
            (80..176).contains(&set_bits),
            "suspicious bit balance: {set_bits}/256"
        );
    }

    use super::*;

    fn crypto() -> Crypto {
        Crypto::from_key([7u8; 32])
    }

    #[test]
    fn round_trips_a_token() {
        let c = crypto();
        let sealed = c.seal_str("ya29.secret-access-token").unwrap();
        // Nonce is 96-bit; ciphertext carries the 16-byte GCM tag so it's
        // strictly longer than the plaintext.
        assert_eq!(sealed.nonce.len(), 12);
        assert!(sealed.ciphertext.len() > "ya29.secret-access-token".len());
        let back = c.open_str(&sealed.nonce, &sealed.ciphertext).unwrap();
        assert_eq!(back, "ya29.secret-access-token");
    }

    #[test]
    fn nonces_differ_per_seal() {
        let c = crypto();
        let a = c.seal_str("same").unwrap();
        let b = c.seal_str("same").unwrap();
        assert_ne!(a.nonce, b.nonce, "each seal must use a fresh nonce");
        assert_ne!(a.ciphertext, b.ciphertext);
    }

    #[test]
    fn wrong_key_fails_to_open() {
        let a = Crypto::from_key([1u8; 32]);
        let b = Crypto::from_key([2u8; 32]);
        let sealed = a.seal_str("secret").unwrap();
        assert!(b.open(&sealed.nonce, &sealed.ciphertext).is_err());
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let c = crypto();
        let mut sealed = c.seal_str("secret").unwrap();
        sealed.ciphertext[0] ^= 0xff;
        assert!(c.open(&sealed.nonce, &sealed.ciphertext).is_err());
    }

    #[test]
    fn derivation_from_session_is_stable() {
        let secret = [9u8; 32];
        let a = Crypto::from_env_or_session(&secret);
        let b = Crypto::from_env_or_session(&secret);
        let sealed = a.seal_str("x").unwrap();
        // Same session secret → same derived key → b can open a's ciphertext.
        assert_eq!(b.open_str(&sealed.nonce, &sealed.ciphertext).unwrap(), "x");
    }

    #[test]
    fn bad_nonce_length_rejected() {
        let c = crypto();
        assert!(c.open(&[0u8; 8], &[0u8; 32]).is_err());
    }

    /// A value sealed under ANY retired derivation label must still open.
    ///
    /// This is the regression that matters: `7e7ec42` renamed the label with no
    /// migration, so every backend API key, connector client secret and stored
    /// OAuth token from before it became undecryptable — and the release notes
    /// answered "re-enter them". Verified against a real database before that
    /// fix: the July-era `backends.api_key_ct` row opened under
    /// `mcp-token-encryption/v1` and not under the label that replaced it.
    ///
    /// Looping over [`RETIRED_LABELS`] rather than naming one is the point: the
    /// label has been retired twice now (the second time with the rename to
    /// croit AIplane), and a deployment that skipped a release must not fall
    /// through the gap. A future rotation is covered by this test for free.
    #[test]
    fn a_value_sealed_under_any_retired_label_still_opens() {
        let session = [42u8; 32];
        let now = Crypto::from_session(&session);

        assert!(
            !RETIRED_LABELS.is_empty(),
            "the fallback list is what makes an upgrade survivable"
        );
        for label in RETIRED_LABELS {
            // What a build of that era would have written.
            let old = Crypto::from_key(derive(&session, label));
            let sealed = old.seal(b"sk-upstream-secret").unwrap();

            assert_ne!(
                now.key,
                old.key,
                "{} must derive a different key from the current label",
                String::from_utf8_lossy(label)
            );
            assert_eq!(
                now.open(&sealed.nonce, &sealed.ciphertext).unwrap(),
                b"sk-upstream-secret",
                "a value sealed under {} must still be readable",
                String::from_utf8_lossy(label)
            );
            assert!(
                now.is_legacy_sealed(&sealed.nonce, &sealed.ciphertext),
                "and must be reported as needing a re-seal"
            );
        }
    }

    /// Retiring a label must not quietly reuse one: two labels deriving the
    /// same key would make `is_legacy_sealed` answer `false` for values that do
    /// need rewriting, and the re-seal pass would skip them forever.
    #[test]
    fn every_label_derives_a_distinct_key() {
        let session = [42u8; 32];
        let mut keys: Vec<[u8; 32]> = vec![derive(&session, LABEL)];
        for label in RETIRED_LABELS {
            keys.push(derive(&session, label));
        }
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), keys.len(), "two labels derive the same key");
    }

    #[test]
    fn a_value_sealed_under_the_current_label_needs_no_reseal() {
        let now = Crypto::from_session(&[42u8; 32]);
        let sealed = now.seal(b"fresh").unwrap();
        assert_eq!(
            now.open(&sealed.nonce, &sealed.ciphertext).unwrap(),
            b"fresh"
        );
        assert!(!now.is_legacy_sealed(&sealed.nonce, &sealed.ciphertext));
    }

    /// The fallback must not weaken the failure case: a genuinely wrong key
    /// still fails, and is not misreported as a migration.
    #[test]
    fn an_unrelated_key_is_not_mistaken_for_a_legacy_seal() {
        let stranger = Crypto::from_key([9u8; 32]);
        let sealed = stranger.seal(b"other deployment").unwrap();

        let now = Crypto::from_session(&[42u8; 32]);
        assert!(now.open(&sealed.nonce, &sealed.ciphertext).is_err());
        assert!(!now.is_legacy_sealed(&sealed.nonce, &sealed.ciphertext));
    }

    /// An explicit `$AIPLANE_ENCRYPTION_KEY` is used verbatim, so there is no
    /// label and nothing to fall back to — a legacy value must NOT open, or the
    /// fallback would be silently widening which keys can read a database.
    #[test]
    fn an_explicit_key_has_no_legacy_fallback() {
        let session = [42u8; 32];
        let old = Crypto::from_key(derive(&session, RETIRED_LABELS[0]));
        let sealed = old.seal(b"secret").unwrap();

        let explicit = Crypto::from_key([1u8; 32]);
        assert!(explicit.legacy.is_empty());
        assert!(explicit.open(&sealed.nonce, &sealed.ciphertext).is_err());
    }

    #[test]
    fn the_string_form_reports_a_legacy_seal_too() {
        let session = [42u8; 32];
        for label in RETIRED_LABELS {
            let old = Crypto::from_key(derive(&session, label));
            let stored = old.seal_to_string("vapid-ish").unwrap();

            let now = Crypto::from_session(&session);
            assert_eq!(now.open_from_string(&stored).as_deref(), Some("vapid-ish"));
            assert!(now.is_legacy_sealed_string(&stored));
        }
        let now = Crypto::from_session(&session);
        assert!(!now.is_legacy_sealed_string("not-even-two-parts"));
    }
}
