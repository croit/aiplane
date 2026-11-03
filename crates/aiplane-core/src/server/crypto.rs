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

use aes_gcm::Aes256Gcm;
use aes_gcm::aead::{Aead, Generate, KeyInit, Nonce};
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
/// so it is not a knob.
pub(crate) const LABEL: &[u8] = b"croit-aiplane/at-rest-encryption/v1";

/// Domain-separation label for the agent activity log's chain key, derived
/// from the at-rest key.
pub(crate) const ACTIVITY_LABEL: &[u8] = b"croit-aiplane/activity-log-chain/v1";

/// One key the agent activity log signs its hash chains with. `id` is a
/// fingerprint stored beside every event, so verification picks the key the
/// event was written under; it reveals nothing about the key.
#[derive(Clone)]
pub struct ActivityKey {
    pub id: String,
    /// Keyed once; every signature clones this state instead of re-running
    /// the HMAC key schedule.
    mac: Hmac<Sha256>,
}

impl std::fmt::Debug for ActivityKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ActivityKey({})", self.id)
    }
}

impl ActivityKey {
    fn derived_from(at_rest: &[u8; 32]) -> Self {
        let key = derive(at_rest, ACTIVITY_LABEL);
        Self {
            id: sha256_hex(&key)[..16].to_string(),
            mac: <Hmac<Sha256>>::new_from_slice(&key).expect("HMAC accepts any key length"),
        }
    }

    /// Lowercase-hex HMAC-SHA256 of `message` under this key.
    pub fn sign(&self, message: &[u8]) -> String {
        let mut mac = self.mac.clone();
        mac.update(message);
        hex_encode(&mac.finalize().into_bytes())
    }
}

/// HKDF-lite: HMAC-SHA256(session_secret, label).
pub(crate) fn derive(session_secret: &[u8; 32], label: &[u8]) -> [u8; 32] {
    let mut mac =
        <Hmac<Sha256>>::new_from_slice(session_secret).expect("HMAC accepts any key length");
    mac.update(label);
    mac.finalize().into_bytes().into()
}

fn open_with(key: &[u8; 32], nonce: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let nonce_arr: [u8; 12] = nonce.try_into().map_err(|_| CryptoError::Decrypt)?;
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| CryptoError::Decrypt)?;
    cipher
        .decrypt(&Nonce::<Aes256Gcm>::from(nonce_arr), ciphertext)
        .map_err(|_| CryptoError::Decrypt)
}

impl Crypto {
    /// Build from explicit 32-byte key material (used by tests).
    pub fn from_key(key: [u8; 32]) -> Self {
        Self { key }
    }

    /// A random, process-lifetime key. Used as the `AppState::new` default so
    /// the type is always present; production overrides it via
    /// [`Crypto::from_env_or_session`]. Stored secrets sealed under an
    /// ephemeral key won't survive a restart — acceptable for tests/dev.
    pub fn ephemeral() -> Self {
        Self {
            key: <[u8; 32]>::generate(),
        }
    }

    /// Resolve the key: `$AIPLANE_ENCRYPTION_KEY` (64 hex chars) wins; otherwise
    /// derive a stable key from the session secret; if that's all-zero
    /// (ephemeral session key path) we still derive deterministically from it
    /// so the process is internally consistent for its lifetime.
    pub fn from_env_or_session(session_secret: &[u8; 32]) -> Self {
        if let Ok(raw) = std::env::var("AIPLANE_ENCRYPTION_KEY")
            && !raw.is_empty()
        {
            let key: Option<[u8; 32]> = hex_decode(&raw).and_then(|bytes| bytes.try_into().ok());
            match key {
                Some(key) => return Self { key },
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
    /// the same session secret.
    pub fn from_session(session_secret: &[u8; 32]) -> Self {
        Self {
            key: derive(session_secret, LABEL),
        }
    }

    /// The activity log's key ring: the key derived from the at-rest key,
    /// which new events are signed with.
    pub fn activity_keys(&self) -> Vec<ActivityKey> {
        vec![ActivityKey::derived_from(&self.key)]
    }

    /// Encrypt `plaintext` under a fresh random nonce.
    pub fn seal(&self, plaintext: &[u8]) -> Result<Sealed, CryptoError> {
        let cipher = Aes256Gcm::new_from_slice(&self.key).map_err(|_| CryptoError::Encrypt)?;
        let nonce =
            Nonce::<Aes256Gcm>::try_generate().map_err(|e| CryptoError::Nonce(e.to_string()))?;
        let ciphertext = cipher
            .encrypt(&nonce, plaintext)
            .map_err(|_| CryptoError::Encrypt)?;
        Ok(Sealed {
            nonce: nonce.to_vec(),
            ciphertext,
        })
    }

    /// Convenience: seal a string.
    pub fn seal_str(&self, plaintext: &str) -> Result<Sealed, CryptoError> {
        self.seal(plaintext.as_bytes())
    }

    /// Decrypt a `(nonce, ciphertext)` pair.
    pub fn open(&self, nonce: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        open_with(&self.key, nonce, ciphertext)
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

/// Compares without an early return on the first differing byte, so the
/// comparison time does not leak how much of a guessed secret was right.
/// Compare digests of equal length (e.g. two [`sha256_hex`]) so the length
/// check does not leak the secret's length either.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
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

/// Secrets shorter than this get no fingerprint at all: below this length the
/// head + tail fragments would together expose most of the secret.
pub const FINGERPRINT_MIN_LEN: usize = 16;

/// A short, irreversible fragment of a stored secret — enough for an
/// operator to tell two keys apart ("which key is stored?"), never enough to
/// reconstruct one. The head and tail are joined with `…`; char-based, so a
/// multi-byte character is never split. `None` when the secret is too short
/// for any fragment to be safe ([`FINGERPRINT_MIN_LEN`]).
pub fn secret_fingerprint(secret: &str) -> Option<String> {
    let chars: Vec<char> = secret.chars().collect();
    if chars.len() < FINGERPRINT_MIN_LEN {
        return None;
    }
    let head: String = chars[..4].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    Some(format!("{head}…{tail}"))
}

#[cfg(test)]
mod activity_key_tests;

#[cfg(test)]
mod tests;
