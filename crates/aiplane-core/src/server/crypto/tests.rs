#[test]
fn constant_time_eq_still_compares_correctly() {
    use super::constant_time_eq;
    assert!(constant_time_eq(b"abc", b"abc"));
    assert!(!constant_time_eq(b"abc", b"abd"));
    assert!(!constant_time_eq(b"abc", b"ab"));
}

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
    Crypto::ephemeral()
}

fn session_secret() -> [u8; 32] {
    <[u8; 32]>::generate()
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
    let a = Crypto::ephemeral();
    let b = Crypto::ephemeral();
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
    let secret = session_secret();
    let a = Crypto::from_env_or_session(&secret);
    let b = Crypto::from_env_or_session(&secret);
    let sealed = a.seal_str("x").unwrap();
    // Same session secret → same derived key → b can open a's ciphertext.
    assert_eq!(b.open_str(&sealed.nonce, &sealed.ciphertext).unwrap(), "x");
}

#[test]
fn bad_nonce_length_rejected() {
    let c = crypto();
    let nonce = <[u8; 8]>::generate();
    let ciphertext = <[u8; 32]>::generate();
    assert!(c.open(&nonce, &ciphertext).is_err());
}

mod fingerprint {
    use super::super::*;

    #[test]
    fn a_long_key_yields_head_and_tail_around_an_ellipsis() {
        // Tavily-style key: the operator must be able to tell two keys
        // apart from the fragment alone.
        let key = "tvly-1234567890abcdef-6543210987fedcba";
        assert_eq!(secret_fingerprint(key).as_deref(), Some("tvly…dcba"));
    }

    #[test]
    fn the_fingerprint_never_contains_the_whole_secret() {
        let key = "sk-live-abcdef1234567890-ghijkl";
        let fp = secret_fingerprint(key).expect("long enough for a fingerprint");
        assert!(!fp.contains(key), "{fp} must not carry the key");
        assert_eq!(fp.chars().count(), 9, "4 + ellipsis + 4");
    }

    #[test]
    fn the_threshold_is_exactly_sixteen_chars() {
        assert_eq!(secret_fingerprint(&"a".repeat(15)), None);
        assert_eq!(
            secret_fingerprint(&"a".repeat(16)).as_deref(),
            Some("aaaa…aaaa")
        );
    }

    #[test]
    fn short_secrets_get_no_fingerprint_at_all() {
        assert_eq!(secret_fingerprint("abc"), None);
        assert_eq!(secret_fingerprint(""), None);
    }

    #[test]
    fn multibyte_chars_are_never_split() {
        // 16 chars: 15 × "a" + one "ß" at the tail.
        let key = "aaaaaaaaaaaaaaaß";
        assert_eq!(secret_fingerprint(key).as_deref(), Some("aaaa…aaaß"));
    }
}

/// Known-answer tests: the exact bytes an existing database depends on.
///
/// Everything else in this module proves the implementation agrees with
/// *itself* — seal here, open there. That would stay green if a dependency
/// bump changed the output format, and the first symptom would be a
/// production deployment whose backend API keys, OIDC client secret and
/// per-user OAuth tokens no longer decrypt. These vectors were captured
/// from a build that is known to read real databases, so they fail loudly
/// instead.
///
/// If one of these breaks after a dependency update, the update is not
/// safe to ship: it has changed either HMAC-SHA256 key derivation or the
/// AES-256-GCM ciphertext layout, and every value sealed by every previous
/// release has to be migrated before it can land. Never "re-bless" them.
mod known_answers {
    use super::*;

    /// HMAC-SHA256(session_secret, label) — what turns the session secret
    /// into the at-rest key.
    #[test]
    fn key_derivation_is_byte_for_byte_stable() {
        let secret = [9u8; 32];
        assert_eq!(
            derive(&secret, LABEL),
            [
                75, 152, 144, 145, 210, 196, 103, 66, 202, 168, 182, 222, 80, 206, 54, 74, 156,
                160, 211, 11, 87, 202, 22, 60, 138, 180, 92, 75, 128, 65, 247, 168
            ],
            "the at-rest key derivation changed — every sealed value in every \
                 existing database is now unreadable"
        );
    }

    /// A ciphertext written by an earlier release must still open, byte for
    /// byte, under the same key and nonce. This is the format itself: a
    /// 96-bit nonce and AES-256-GCM output with the tag appended.
    #[test]
    fn a_ciphertext_from_an_earlier_release_still_opens() {
        let nonce = [70, 115, 125, 239, 81, 156, 73, 132, 64, 88, 30, 127];
        let ciphertext = [
            47, 91, 32, 20, 4, 226, 4, 200, 36, 63, 214, 118, 149, 130, 31, 167, 57, 43, 249, 93,
            223, 154, 179, 72, 223, 59, 215, 242, 204, 245,
        ];
        let crypto = Crypto::from_key([7u8; 32]);
        assert_eq!(
            crypto.open(&nonce, &ciphertext).unwrap(),
            b"at-rest canary",
            "the at-rest ciphertext format changed — stored secrets from \
                 every previous release are unreadable"
        );
        // And the tag is still checked: one flipped bit must not open.
        let mut tampered = ciphertext;
        tampered[0] ^= 1;
        assert!(crypto.open(&nonce, &tampered).is_err());
    }
}
